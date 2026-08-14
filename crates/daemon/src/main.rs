//! Daemon entrypoint for the Agent Orchestrator control plane and worker loop.
//!
//! It hosts the gRPC API, background workers, and secure control-plane bootstrap.
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

mod control_plane;
mod daemonize;
mod fs_watcher;
mod lifecycle;
mod managed_source;
mod protection;
mod server;
mod slack_api;
mod slack_gateway;
mod source_router;
mod uds_security;
mod webhook;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use futures::FutureExt;
use tonic::transport::Server;
use tracing::{error, info};

/// How often the data directory's identity is re-checked (FR-169).
///
/// A constant rather than a flag, deliberately. The only argument for making it
/// configurable is that some filesystem might need a different value, which is a
/// guess until a real case appears; a parameter is permanent surface, while a
/// constant can become one at any time. Concept budget: zero.
const DATA_DIR_CHECK_PERIOD: std::time::Duration = std::time::Duration::from_secs(5);

/// Consecutive confirmations before the daemon acts on a vanished data directory.
///
/// Three tolerates two transient failures. With the period above, detection lands
/// within ~15s: far below any human or supervisor reaction time, and long enough
/// that a paused VM or one slow network round-trip does not end a healthy daemon.
/// Chosen by argument, not by measurement — there is no population to sample here,
/// since `stat` on a live local directory does not fail transiently, and demanding
/// a distribution would have manufactured one.
const DATA_DIR_CHECK_CONFIRMATIONS: u32 = 3;

/// Set when the vanish watcher initiated shutdown, so `shutdown_reason` can name
/// the cause instead of reporting the generic one.
static DATA_DIR_VANISHED: AtomicBool = AtomicBool::new(false);

/// Records that the data directory vanished, for `shutdown_reason`.
fn set_data_dir_vanished() {
    DATA_DIR_VANISHED.store(true, Ordering::SeqCst);
}

use agent_orchestrator::events::insert_event;
use agent_orchestrator::service::system::{clear_worker_stop_signal, worker_stop_signal_path};
use agent_orchestrator::state::{InnerState, task_semaphore};
use orchestrator_proto::OrchestratorServiceServer;
use orchestrator_scheduler::scheduler::safety::RestartRequestedError;
use orchestrator_scheduler::scheduler::{
    RunningTask, load_task_summary, register_running_task, run_task_loop, shutdown_running_tasks,
    unregister_running_task,
};
use orchestrator_scheduler::service::task::{SchedulerTaskEnqueuer, claim_next_pending_task};

#[derive(Debug, Parser)]
#[command(name = "orchestratord", version, about = "Agent Orchestrator daemon")]
struct Args {
    #[arg(short = 'f', long = "foreground")]
    foreground: bool,

    #[arg(long = "bind")]
    bind: Option<String>,

    #[cfg(feature = "dev-insecure")]
    #[arg(long = "insecure-bind")]
    insecure_bind: Option<String>,

    #[arg(long = "workers", default_value_t = 1)]
    workers: usize,

    #[arg(long = "control-plane-dir")]
    control_plane_dir: Option<PathBuf>,

    /// Maximum role for UDS callers when no uds-policy.yaml exists.
    #[arg(
        long = "uds-max-role",
        default_value = "operator",
        env = "ORCHESTRATOR_UDS_MAX_ROLE"
    )]
    uds_max_role: control_plane::Role,

    /// Number of days to retain events before automatic cleanup (0 = disabled).
    #[arg(long = "event-retention-days", default_value_t = 30)]
    event_retention_days: u32,

    /// Interval in seconds between automatic event cleanup sweeps.
    #[arg(long = "event-cleanup-interval-secs", default_value_t = 3600)]
    event_cleanup_interval_secs: u64,

    /// Enable event archival to JSONL before cleanup.
    #[arg(long = "event-archive-enabled")]
    event_archive_enabled: bool,

    /// Override the directory used for event archive JSONL files.
    #[arg(long = "event-archive-dir")]
    event_archive_dir: Option<PathBuf>,

    /// Number of days to retain log files before automatic cleanup (0 = disabled).
    #[arg(long = "log-retention-days", default_value_t = 30)]
    log_retention_days: u32,

    /// Number of days to retain terminated tasks before automatic cleanup (0 = disabled).
    #[arg(long = "task-retention-days", default_value_t = 0)]
    task_retention_days: u32,

    /// Bind address for the HTTP webhook server.
    /// Defaults to 127.0.0.1:19090 (loopback only). Set to "none" to disable.
    /// Use 0.0.0.0:19090 for Docker/K8s (requires --webhook-secret or PKI).
    #[arg(
        long = "webhook-bind",
        default_value = "127.0.0.1:19090",
        env = "ORCHESTRATOR_WEBHOOK_BIND"
    )]
    webhook_bind: String,

    /// Shared secret for webhook HMAC-SHA256 signature verification.
    #[arg(long = "webhook-secret", env = "ORCHESTRATOR_WEBHOOK_SECRET")]
    webhook_secret: Option<String>,

    /// Allow the webhook server to run without signature verification on
    /// non-loopback addresses. Insecure — use only behind a trusted reverse
    /// proxy or in test environments.
    #[arg(
        long = "webhook-allow-unsigned",
        env = "ORCHESTRATOR_WEBHOOK_ALLOW_UNSIGNED",
        default_value_t = false
    )]
    webhook_allow_unsigned: bool,

    /// Optional public Slack Integration Gateway origin. Managed Slack remains
    /// fully disabled when this and the enrollment key are absent.
    #[arg(long, env = "ORCHESTRATOR_SLACK_GATEWAY_URL")]
    slack_gateway_url: Option<String>,

    /// Pre-install enrollment credential for creating managed OAuth intents.
    /// Installation-scoped pairing credentials are used after consent.
    #[arg(
        long,
        env = "ORCHESTRATOR_SLACK_GATEWAY_ENROLLMENT_KEY",
        hide_env_values = true
    )]
    slack_gateway_enrollment_key: Option<String>,

    /// Slack Web API origin used only by local dedicated App provisioning.
    /// Production accepts slack.com; loopback overrides exist for isolated QA.
    #[arg(
        long,
        env = "ORCHESTRATOR_SLACK_API_BASE",
        default_value = "https://slack.com"
    )]
    slack_api_base: String,

    /// Minutes before a running item is considered stalled (0 = disabled).
    #[arg(long = "stall-timeout-mins", default_value_t = 30)]
    stall_timeout_mins: u64,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(subcommand)]
    ControlPlane(ControlPlaneCommands),

    /// Print the webhook HMAC secret derived from the control-plane CA certificate.
    WebhookSecret {
        #[arg(long = "control-plane-dir")]
        control_plane_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ControlPlaneCommands {
    IssueClient {
        #[arg(long = "bind")]
        bind: String,

        #[arg(long = "subject")]
        subject: String,

        #[arg(long = "role", default_value = "operator")]
        role: control_plane::Role,

        #[arg(long = "home")]
        home: Option<PathBuf>,

        #[arg(long = "control-plane-dir")]
        control_plane_dir: Option<PathBuf>,
    },
}

async fn force_server_shutdown(started: Arc<tokio::sync::Notify>) {
    started.notified().await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Daemonize before starting any threads or the tokio runtime.
    // In daemon mode, stdout/stderr are redirected to data/daemon.log
    // so ANSI escape codes are disabled.
    // Subcommands skip daemonization — they run in the foreground and exit.
    let use_ansi = if args.foreground || args.command.is_some() {
        true
    } else {
        let data_dir = agent_orchestrator::config_load::data_dir();
        let log_path = data_dir.join("daemon.log");
        daemonize::daemonize(&log_path)?;
        false
    };

    // Build log filter: ORCHESTRATOR_LOG > RUST_LOG > default "info"
    let filter = if let Ok(level_str) = std::env::var("ORCHESTRATOR_LOG") {
        let level = agent_orchestrator::config::LogLevel::parse(&level_str).unwrap_or_default();
        tracing_subscriber::EnvFilter::new(level.as_tracing_level().to_string())
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };

    // Build subscriber: ORCHESTRATOR_LOG_FORMAT controls output format
    let format = std::env::var("ORCHESTRATOR_LOG_FORMAT")
        .ok()
        .and_then(|f| agent_orchestrator::config::LoggingFormat::parse(&f))
        .unwrap_or_default();

    match format {
        agent_orchestrator::config::LoggingFormat::Json => {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_target(false)
                .with_ansi(false)
                .with_env_filter(filter)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .context("failed to set tracing subscriber")?;
        }
        agent_orchestrator::config::LoggingFormat::Pretty => {
            let subscriber = tracing_subscriber::fmt()
                .with_target(false)
                .with_ansi(use_ansi)
                .with_env_filter(filter)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .context("failed to set tracing subscriber")?;
        }
    }

    // Install panic hook that appends to daemon_crash.log before the default hook.
    {
        let data_dir = agent_orchestrator::config_load::data_dir();
        let crash_log = data_dir.join("daemon_crash.log");
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&crash_log)
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = writeln!(f, "[epoch={ts}] {info}");
            }
            default_hook(info);
        }));
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;

    if let Some(command) = args.command {
        return handle_subcommand(command);
    }

    rt.block_on(async move {
        let state = agent_orchestrator::service::bootstrap::init_state_async_with_enqueuer(
            false,
            std::sync::Arc::new(SchedulerTaskEnqueuer),
        )
        .await
        .context("failed to initialize orchestrator state")?;
        let inner = state.inner.clone();
        inner.daemon_runtime.set_configured_workers(args.workers);
        // Both the cleanup sweep below and the EventCleanup RPC and `db status`
        // read this back through resolved_event_archive_dir, so the flag is
        // honoured everywhere the archive directory is named.
        inner
            .daemon_runtime
            .set_event_archive_dir(args.event_archive_dir.clone());
        let slack_gateway = match (
            args.slack_gateway_url.as_deref(),
            args.slack_gateway_enrollment_key.clone(),
        ) {
            (Some(origin), Some(enrollment_key)) => Some(Arc::new(
                slack_gateway::SlackGatewayClient::new(origin, enrollment_key)?,
            )),
            (None, None) => None,
            _ => bail!(
                "managed Slack requires both --slack-gateway-url and --slack-gateway-enrollment-key"
            ),
        };
        let slack_api_url = url::Url::parse(&args.slack_api_base)
            .context("invalid Slack API base URL")?;
        let slack_api_official = slack_api_url.scheme() == "https"
            && slack_api_url.host_str() == Some("slack.com");
        let slack_api_loopback = matches!(
            slack_api_url.host_str(),
            Some("localhost" | "127.0.0.1" | "::1")
        );
        if !slack_api_official && !slack_api_loopback {
            bail!("Slack API base override is restricted to loopback tests");
        }
        let slack_manifest_client = Arc::new(
            orchestrator_slack_gateway::slack::SlackClient::new(
                &args.slack_api_base,
                std::time::Duration::from_secs(15),
            )?,
        );
        if let Some(gateway) = slack_gateway.as_ref() {
            let provider: Arc<dyn agent_orchestrator::source_connection::SourceConnectionProvider> =
                Arc::new(slack_gateway::SlackGatewayProvider::new(
                    inner.clone(),
                    gateway.clone(),
                ));
            inner
                .source_connection_provider
                .write()
                .map_err(|_| anyhow::anyhow!("managed source provider lock poisoned"))?
                .clone_from(&provider);
        }

        // Increment persistent incarnation counter on every startup (including exec() restarts)
        let incarnation = agent_orchestrator::persistence::repository::daemon_meta::increment_incarnation(
            &inner.async_database,
        )
        .await
        .unwrap_or(0);
        inner.daemon_runtime.set_incarnation(incarnation);

        let socket_path = lifecycle::socket_path(&inner.data_dir);
        let pid_path = lifecycle::pid_path(&inner.data_dir);

        // Detect stale PID from a previous crash before overwriting
        let stale_pid_detected = lifecycle::detect_stale_pid(&pid_path);

        // Refuse to start if another daemon instance is already running.
        // This prevents socket destruction when multiple daemons race to bind
        // the same UDS path (e.g. after a self-restart exec() where the PID is
        // preserved but the socket is transiently unavailable).
        if let Some(existing_pid) = lifecycle::detect_running_daemon(&pid_path) {
            anyhow::bail!(
                "another orchestratord is already running (PID {existing_pid}); \
                 not starting a second instance"
            );
        }

        // Write PID file
        lifecycle::write_pid_file(&pid_path)?;

        info!(
            socket = %socket_path.display(),
            pid_file = %pid_path.display(),
            version = env!("CARGO_PKG_VERSION"),
            git_hash = env!("BUILD_GIT_HASH"),
            incarnation,
            "orchestratord starting"
        );

        emit_daemon_event(
            &inner,
            "daemon_incarnation_started",
            serde_json::json!({
                "incarnation": incarnation,
                "version": env!("CARGO_PKG_VERSION"),
                "git_hash": env!("BUILD_GIT_HASH"),
            }),
        )
        .await;

        // Emit crash recovery event if stale PID was detected
        if stale_pid_detected {
            info!("stale PID file detected — previous daemon likely crashed");
            emit_daemon_event(
                &inner,
                "daemon_crash_recovered",
                serde_json::json!({ "source": "stale_pid_detection" }),
            )
            .await;
        }

        // Recover orphaned running items from a previous crash
        match inner.task_repo.recover_orphaned_running_items().await {
            Ok(recovered) => {
                for (task_id, item_ids) in &recovered {
                    info!(
                        task_id = %task_id,
                        items = item_ids.len(),
                        "recovered orphaned running items"
                    );
                    emit_daemon_event(
                        &inner,
                        "orphaned_items_recovered",
                        serde_json::json!({
                            "task_id": task_id,
                            "recovered_item_ids": item_ids,
                            "count": item_ids.len(),
                        }),
                    )
                    .await;
                }
                if !recovered.is_empty() {
                    let total: usize = recovered.iter().map(|(_, ids)| ids.len()).sum();
                    info!(
                        tasks = recovered.len(),
                        items = total,
                        "startup orphan recovery complete"
                    );
                    inner.worker_notify.notify_waiters();
                }
            }
            Err(e) => {
                error!(error = %e, "failed to recover orphaned running items at startup");
            }
        }

        // Clear any stale stop signal from a previous run
        let _ = clear_worker_stop_signal(&inner);

        // Shutdown coordination: watch channel shared between server and workers
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        // Restart coordination: worker sends binary path when restart is requested
        let (restart_tx, restart_rx) =
            tokio::sync::watch::channel::<Option<std::path::PathBuf>>(None);
        let config_mutation_lock = Arc::new(tokio::sync::Mutex::new(()));

        // Spawn worker supervisor (owns restart_tx, manages worker lifecycle)
        let supervisor_handle = {
            let sup_state = inner.clone();
            let sup_shutdown = shutdown_rx.clone();
            let worker_count = args.workers;
            tokio::spawn(worker_supervisor(
                sup_state,
                worker_count,
                sup_shutdown,
                restart_tx,
            ))
        };
        info!(workers = args.workers, "worker supervisor started");

        if let Some(gateway) = slack_gateway.clone() {
            let managed_state = inner.clone();
            let managed_shutdown = shutdown_rx.clone();
            let managed_config_lock = config_mutation_lock.clone();
            tokio::spawn(async move {
                managed_source::run(
                    managed_state,
                    gateway,
                    managed_config_lock,
                    managed_shutdown,
                )
                .await;
            });
            info!("managed Slack delivery worker started");
        }

        // Spawn trigger engine (cron + event-driven task creation)
        {
            let (engine, handle) =
                agent_orchestrator::trigger_engine::TriggerEngine::new(inner.clone());
            // Store handle so resource apply/delete can notify the engine to reload.
            if let Ok(mut guard) = inner.trigger_engine_handle.lock() {
                *guard = Some(handle);
            }
            let trig_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                engine.run(trig_shutdown).await;
            });
        }

        // Spawn filesystem watcher (lazy — only activates when source: filesystem triggers exist)
        {
            let (fs_handle, fs_reload_rx) = fs_watcher::new_handle();
            if let Ok(mut guard) = inner.fs_watcher_reload_tx.lock() {
                *guard = Some(fs_handle.reload_tx.clone());
            }
            let fs_state = inner.clone();
            let fs_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                fs_watcher::run_fs_watcher(fs_state, fs_reload_rx, fs_shutdown).await;
            });
        }

        // Spawn agent drain timeout sweep (runs every 10s)
        {
            let drain_state = inner.clone();
            let mut drain_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            agent_orchestrator::agent_lifecycle::drain_timeout_sweep(&drain_state).await;
                        }
                        _ = drain_shutdown.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        // Reconcile persisted interactive sessions with process identity and lease expiry.
        {
            let session_state = inner.clone();
            let mut session_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let result = agent_orchestrator::session_store::reconcile_sessions_async(
                                &session_state.async_database,
                            ).await;
                            match result {
                                Ok(outcome) => {
                                    if !outcome.changes.is_empty() {
                                        info!(changes = outcome.changes.len(), "interactive session reconciliation updated state");
                                    }
                                    reclaim_orphaned_sessions(
                                        &session_state,
                                        &outcome.reclaim_candidates,
                                        agent_orchestrator::session_store::ReclaimSignal::Immediate,
                                    ).await;
                                }
                                Err(error) => error!(%error, "interactive session reconciliation failed"),
                            }
                        }
                        _ = session_shutdown.changed() => break,
                    }
                }
            });
        }

        // Project durable task events into the cross-task attention queue.
        {
            let attention_state = inner.clone();
            let attention_metrics =
                agent_orchestrator::process_metrics::AsyncProcessMetricsRepository::new(
                    inner.async_database.clone(),
                );
            let mut attention_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_millis(750));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match orchestrator_scheduler::service::attention::reconcile_attention_once(&attention_state).await {
                                Ok(processed) if processed > 0 => {
                                    let cursor = attention_state.attention_repo.projector_cursor().await.unwrap_or_default();
                                    let lag = attention_state.attention_repo.projector_lag().await.unwrap_or_default();
                                    if let Err(error) = attention_metrics.projector_success("attention", "", &cursor.to_string(), lag).await {
                                        error!(error = %error, "attention projector health update failed");
                                    }
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    let lag = attention_state.attention_repo.projector_lag().await.unwrap_or_default();
                                    if let Err(metrics_error) = attention_metrics.projector_failure("attention", "", "reconcile_failed", lag).await {
                                        error!(error = %metrics_error, "attention projector failure metric update failed");
                                    }
                                    error!(error = %error, "attention inbox reconciliation failed");
                                }
                            }
                        }
                        _ = attention_shutdown.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        // Route durably accepted external source events after persistence.
        {
            let source_state = inner.clone();
            let source_metrics =
                agent_orchestrator::process_metrics::AsyncProcessMetricsRepository::new(
                    inner.async_database.clone(),
                );
            let mut source_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_millis(500));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match source_router::reconcile_source_once(&source_state).await {
                                Ok(processed) if processed > 0 => {
                                    let repository = agent_orchestrator::source::AsyncSourceRepository::new(source_state.async_database.clone());
                                    let lag = repository.routing_lag().await.unwrap_or_default();
                                    if let Err(error) = source_metrics.projector_success("source_router", "", "queue", lag).await {
                                        error!(error = %error, "source projector health update failed");
                                    }
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    let repository = agent_orchestrator::source::AsyncSourceRepository::new(source_state.async_database.clone());
                                    let lag = repository.routing_lag().await.unwrap_or_default();
                                    if let Err(metrics_error) = source_metrics.projector_failure("source_router", "", "reconcile_failed", lag).await {
                                        error!(error = %metrics_error, "source projector failure metric update failed");
                                    }
                                    error!(error = %error, "source routing reconciliation failed");
                                }
                            }
                            match source_router::reconcile_source_automation_once(&source_state).await {
                                Ok(processed) if processed > 0 => {
                                    let repository = agent_orchestrator::source::AsyncSourceRepository::new(source_state.async_database.clone());
                                    let lag = repository.routing_lag().await.unwrap_or_default();
                                    if let Err(error) = source_metrics.projector_success("source_automation", "", "queue", lag).await {
                                        error!(error = %error, "source automation health update failed");
                                    }
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    let repository = agent_orchestrator::source::AsyncSourceRepository::new(source_state.async_database.clone());
                                    let lag = repository.routing_lag().await.unwrap_or_default();
                                    if let Err(metrics_error) = source_metrics.projector_failure("source_automation", "", "reconcile_failed", lag).await {
                                        error!(error = %metrics_error, "source automation failure metric update failed");
                                    }
                                    error!(error = %error, "source automation reconciliation failed");
                                }
                            }
                        }
                        _ = source_shutdown.changed() => break,
                    }
                }
            });
        }

        // Spawn event cleanup sweep (TTL-based)
        if args.event_retention_days > 0 {
            let cleanup_state = inner.clone();
            let mut cleanup_shutdown = shutdown_rx.clone();
            let retention_days = args.event_retention_days;
            let archive_enabled = args.event_archive_enabled;
            let archive_dir = inner
                .daemon_runtime
                .resolved_event_archive_dir(&inner.data_dir);
            let interval_secs = args.event_cleanup_interval_secs;
            info!(
                retention_days,
                interval_secs, archive_enabled, "event cleanup sweep started"
            );
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let result = if archive_enabled {
                                agent_orchestrator::event_cleanup::archive_events(
                                    &cleanup_state.async_database,
                                    &archive_dir,
                                    retention_days,
                                    1000,
                                )
                                .await
                            } else {
                                agent_orchestrator::event_cleanup::cleanup_old_events(
                                    &cleanup_state.async_database,
                                    retention_days,
                                    1000,
                                )
                                .await
                            };
                            if let Err(e) = result {
                                tracing::warn!(error = %e, "event cleanup sweep failed");
                            }
                            if let Err(error) = agent_orchestrator::source_automation::AsyncSourceAutomationRepository::new(
                                cleanup_state.async_database.clone(),
                            )
                            .cleanup_metadata(retention_days, 1000)
                            .await
                            {
                                tracing::warn!(error = %error, "source automation metadata cleanup failed");
                            }
                        }
                        _ = cleanup_shutdown.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        // Spawn log + task cleanup sweep (piggybacks on event cleanup interval)
        if args.log_retention_days > 0 || args.task_retention_days > 0 {
            let lifecycle_state = inner.clone();
            let mut lifecycle_shutdown = shutdown_rx.clone();
            let log_days = args.log_retention_days;
            let task_days = args.task_retention_days;
            let interval_secs = args.event_cleanup_interval_secs;
            info!(
                log_retention_days = log_days,
                task_retention_days = task_days,
                interval_secs,
                "data lifecycle sweep started"
            );
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if log_days > 0
                                && let Err(e) = agent_orchestrator::log_cleanup::cleanup_old_logs(
                                    &lifecycle_state.async_database,
                                    &lifecycle_state.logs_dir,
                                    log_days,
                                ).await {
                                    tracing::warn!(error = %e, "log cleanup sweep failed");
                                }
                            if task_days > 0
                                && let Err(e) = agent_orchestrator::task_cleanup::cleanup_old_tasks(
                                    &lifecycle_state.async_database,
                                    &lifecycle_state.logs_dir,
                                    task_days,
                                    50,
                                ).await {
                                    tracing::warn!(error = %e, "task cleanup sweep failed");
                                }
                        }
                        _ = lifecycle_shutdown.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        // Spawn stall detection sweep
        if args.stall_timeout_mins > 0 {
            let stall_state = inner.clone();
            let mut stall_shutdown = shutdown_rx.clone();
            let stall_threshold_secs = args.stall_timeout_mins * 60;
            info!(
                stall_timeout_mins = args.stall_timeout_mins,
                "stall detection sweep started"
            );
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(300));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            // Collect task IDs with active workers — these
                            // should not be touched by stall recovery (their
                            // items may be slow, not stalled).
                            let active_task_ids: std::collections::HashSet<String> = {
                                let running = stall_state.running.lock().await;
                                running.keys().cloned().collect()
                            };
                            match stall_state.task_repo.recover_stalled_running_items(stall_threshold_secs, active_task_ids).await {
                                Ok(recovered) => {
                                    for (task_id, item_ids) in &recovered {
                                        for item_id in item_ids {
                                            emit_daemon_event(
                                                &stall_state,
                                                "item_stall_recovered",
                                                serde_json::json!({
                                                    "task_id": task_id,
                                                    "item_id": item_id,
                                                    "stall_threshold_secs": stall_threshold_secs,
                                                }),
                                            )
                                            .await;
                                        }
                                    }
                                    if !recovered.is_empty() {
                                        stall_state.worker_notify.notify_waiters();
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "stall detection sweep failed");
                                }
                            }
                        }
                        _ = stall_shutdown.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        // Spawn webhook HTTP server (enabled by default on 127.0.0.1:19090).
        let webhook_bind = args.webhook_bind.as_str();
        if webhook_bind != "none" {
            let addr: std::net::SocketAddr = webhook_bind
                .parse()
                .context("invalid --webhook-bind address (use \"none\" to disable)")?;
            // Resolve webhook secret: explicit flag > derived from control-plane CA > none.
            let webhook_secret = args.webhook_secret.clone().or_else(|| {
                let derived = control_plane::derive_webhook_secret(
                    &inner.data_dir,
                    args.control_plane_dir.as_deref(),
                );
                if derived.is_some() {
                    info!(%addr, "webhook secret derived from control-plane CA certificate");
                }
                derived
            });
            if webhook_secret.is_none() {
                if addr.ip().is_loopback() {
                    info!(
                        %addr,
                        "webhook server on loopback without signature verification \
                         (safe for local development)"
                    );
                } else if args.webhook_allow_unsigned {
                    tracing::warn!(
                        %addr,
                        "webhook server starting without signature verification on a \
                         non-loopback address (--webhook-allow-unsigned); this is insecure"
                    );
                } else {
                    bail!(
                        "refusing to start webhook server on {addr} without signature \
                         verification.\n\n\
                         The webhook server is bound to a non-loopback address but no \
                         HMAC secret is configured. This would accept unsigned requests \
                         from the network.\n\n\
                         Options:\n  \
                         1. Set --webhook-secret <secret> or ORCHESTRATOR_WEBHOOK_SECRET\n  \
                         2. Configure control-plane PKI (auto-derives a secret)\n  \
                         3. Use --webhook-bind 127.0.0.1:19090 for local-only access\n  \
                         4. Pass --webhook-allow-unsigned to override this check\n  \
                         5. Set --webhook-bind none to disable the webhook server"
                    );
                }
            }
            let wh_state = webhook::WebhookState {
                inner: inner.clone(),
                secret: webhook_secret,
                ingest_failure_throttle: Default::default(),
            };
            let router = webhook::router(wh_state);
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("failed to bind webhook on {addr}"))?;
            info!(%addr, "webhook HTTP server started");
            let mut wh_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        let _ = wh_shutdown.changed().await;
                    })
                    .await
                    .ok();
            });
        } else {
            info!("webhook HTTP server disabled (--webhook-bind none)");
        }

        let shutdown_notify = Arc::new(tokio::sync::Notify::new());

        let protection = Arc::new(protection::ControlPlaneProtection::load_or_bootstrap(
            &inner.data_dir,
            &inner.db_path,
            args.control_plane_dir.as_deref(),
        )?);

        // Phase 3a: warn if data_dir has overly permissive permissions.
        check_data_dir_permissions(&inner.data_dir);

        // FR-169: stop being a process that cannot serve anyone.
        //
        // A daemon whose data directory is removed underneath it keeps running
        // indefinitely: its socket went with the directory, so no client can
        // reach it, and it holds the database open on an unlinked inode. Measured
        // before this existed — 22h34m in the field, and in an isolated repro it
        // stayed alive at t+2/5/10/20/30s while writing zero bytes of log.
        //
        // The check is on identity rather than path presence because
        // delete-and-recreate is the worse half: the path reads healthy while the
        // old daemon writes an orphaned inode and a second daemon takes the name.
        // See lifecycle::data_dir_identity.
        //
        // This does not add an exit path. It triggers `shutdown_notify`, the same
        // handle the RPC shutdown uses, so there is one shutdown sequence and not
        // a second one that has to be kept in agreement with it.
        if let Some(expected_identity) = lifecycle::data_dir_identity(&inner.data_dir) {
            let watcher_notify = shutdown_notify.clone();
            let watcher_state = inner.clone();
            let mut watcher_shutdown = shutdown_rx.clone();
            let data_dir = inner.data_dir.clone();
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(DATA_DIR_CHECK_PERIOD);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                // Consecutive confirmations. Reset on any match, so a single
                // transient stat failure cannot end the daemon.
                let mut confirmations = 0u32;
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let current = lifecycle::data_dir_identity(&data_dir);
                            if !lifecycle::observe_data_dir(
                                expected_identity,
                                current,
                                &mut confirmations,
                                DATA_DIR_CHECK_CONFIRMATIONS,
                            ) {
                                continue;
                            }
                            // The one line that makes this answerable afterwards.
                            // Before FR-169 this event produced no output at all.
                            tracing::error!(
                                data_dir = %data_dir.display(),
                                expected_dev = expected_identity.0,
                                expected_ino = expected_identity.1,
                                observed = ?current,
                                confirmations,
                                "data directory is gone; this daemon can no longer serve \
                                 anyone and is shutting down"
                            );
                            watcher_state.daemon_runtime.request_shutdown();
                            set_data_dir_vanished();
                            watcher_notify.notify_waiters();
                            return;
                        }
                        _ = watcher_shutdown.changed() => return,
                    }
                }
            });
        } else {
            // Startup already requires the directory; if it cannot be stat'd here
            // something is wrong enough to say so rather than silently skip the
            // watcher for the rest of the process's life.
            tracing::warn!(
                data_dir = %inner.data_dir.display(),
                "cannot read the data directory's identity; the vanish watcher is not armed"
            );
        }

        let uds_policy = uds_security::load_uds_policy(
            &inner.data_dir,
            args.control_plane_dir.as_deref(),
        )?;

        // Phase 3b: when no policy file exists, construct an ephemeral policy
        // from the --uds-max-role flag (default: Operator).
        let uds_policy = match uds_policy {
            Some(p) => Some(p),
            None => {
                let policy_path = agent_orchestrator::paths::control_plane_dir(
                    &inner.data_dir,
                    args.control_plane_dir.as_deref(),
                )
                .join("uds-policy.yaml");
                info!(
                    role = %args.uds_max_role.as_str(),
                    "UDS policy: no uds-policy.yaml found; using --uds-max-role default. \
                     Create {} to configure explicitly.",
                    policy_path.display()
                );
                Some(uds_security::UdsAuthPolicy {
                    max_role: args.uds_max_role,
                    audit_all_reads: false,
                })
            }
        };

        let service = server::OrchestratorServer::new(
            inner.clone(),
            shutdown_notify.clone(),
            None,
            uds_policy,
            slack_gateway.clone(),
            slack_manifest_client.clone(),
            config_mutation_lock.clone(),
        );

        // Shutdown future: listen for OS signals, restart request, or RPC shutdown
        let server_shutdown_started = Arc::new(tokio::sync::Notify::new());
        let shutdown_fut = {
            let inner2 = inner.clone();
            let mut restart_rx2 = restart_rx.clone();
            let notify = shutdown_notify.clone();
            let server_shutdown_started = server_shutdown_started.clone();
            async move {
                tokio::select! {
                    result = lifecycle::shutdown_signal(inner2) => {
                        if let Err(error) = result {
                            tracing::error!(%error, "failed to initialize shutdown signal handling");
                        }
                    }
                    _ = restart_rx2.changed() => {}
                    _ = notify.notified() => {
                        tracing::info!("shutdown triggered via RPC");
                    }
                }
                server_shutdown_started.notify_one();
            }
        };

        // Identity of the socket this daemon binds, so teardown can tell its own
        // socket from a successor's at the same path (FR-170). Stays `None` on
        // both TCP paths, which bind no socket file and so have none to remove.
        let mut socket_identity: Option<(u64, u64)> = None;

        // Determine bind address: UDS by default, secure TCP if --bind provided
        if let Some(addr) = args.bind.as_deref() {
            let addr = addr.parse().context("invalid bind address")?;
            let secure = control_plane::prepare_secure_server(
                &inner.data_dir,
                &inner.db_path,
                &addr,
                args.control_plane_dir.as_deref(),
            )?;
            info!(%addr, "listening on TCP");
            let serving = Server::builder()
                .layer(protection.clone().layer())
                .tls_config(secure.tls)?
                .add_service(
                    OrchestratorServiceServer::new(server::OrchestratorServer::new(
                        inner.clone(),
                        shutdown_notify.clone(),
                        Some(secure.security),
                        None,
                        slack_gateway.clone(),
                        slack_manifest_client.clone(),
                        config_mutation_lock.clone(),
                    ))
                    .max_encoding_message_size(64 * 1024 * 1024),
                )
                .serve_with_shutdown(addr, shutdown_fut);
            tokio::select! {
                result = serving => result.context("gRPC server error")?,
                _ = force_server_shutdown(server_shutdown_started.clone()) => {
                    tracing::warn!("forcing gRPC server shutdown after connection drain timeout");
                }
            }
        } else {
            #[cfg(feature = "dev-insecure")]
            let insecure_addr = args.insecure_bind.as_deref();
            #[cfg(not(feature = "dev-insecure"))]
            let insecure_addr: Option<&str> = None;

            if let Some(addr) = insecure_addr {
                let addr = addr.parse().context("invalid insecure bind address")?;
                info!(%addr, "listening on insecure TCP");
                tracing::warn!("insecure TCP control-plane enabled; use only for local development");
                let serving = Server::builder()
                    .layer(protection.clone().layer())
                    .add_service(
                        OrchestratorServiceServer::new(service)
                            .max_encoding_message_size(64 * 1024 * 1024),
                    )
                    .serve_with_shutdown(addr, shutdown_fut);
                tokio::select! {
                    result = serving => result.context("gRPC server error")?,
                    _ = force_server_shutdown(server_shutdown_started.clone()) => {
                        tracing::warn!("forcing gRPC server shutdown after connection drain timeout");
                    }
                }
            } else {
                // UDS transport
                use tokio::net::UnixListener;

                // Remove a stale socket left by a previous daemon.
                //
                // This stays unconditional on purpose, re-argued rather than
                // inherited (FR-170). Reaching here means the pidfile guard
                // above found no live daemon, and a socket left behind by a
                // SIGKILL must be removable or the daemon could never restart —
                // a state orchestrator-client's connect.rs already explains to
                // users. What is no longer inherited is discarding the error: a
                // failure that is not "it was not there" is a real obstacle, and
                // reporting it as `failed to bind UDS` names the wrong file.
                match std::fs::remove_file(&socket_path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(anyhow::Error::new(e).context(format!(
                            "failed to remove existing socket at {}",
                            socket_path.display()
                        )));
                    }
                }
                let uds = UnixListener::bind(&socket_path).context("failed to bind UDS")?;

                // Harden socket permissions to owner-only regardless of umask.
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        &socket_path,
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .context("failed to set UDS socket permissions to 0600")?;
                }

                // Recorded after bind, so it is the identity of the socket this
                // process is actually listening on. Teardown removes the socket
                // only while the path still resolves here (FR-170).
                socket_identity = lifecycle::path_identity(&socket_path);

                // Wrap accepted connections with peer-credential validation.
                // Connections from a different UID are dropped before entering
                // the gRPC layer.  Valid connections are wrapped as UdsStream so
                // that UdsPeerInfo is available via request extensions.
                use futures::StreamExt;
                let uds_stream =
                    tokio_stream::wrappers::UnixListenerStream::new(uds).filter_map(
                        |result| async {
                            match result {
                                Ok(stream) => match uds_security::validate_peer(&stream) {
                                    Ok(peer) => {
                                        Some(Ok(uds_security::UdsStream::new(stream, peer)))
                                    }
                                    Err(e) => {
                                        tracing::warn!(error = %e, "rejected UDS connection");
                                        None
                                    }
                                },
                                Err(e) => Some(Err(e)),
                            }
                        },
                    );

                info!(socket = %socket_path.display(), mode = "0600", "listening on UDS");
                emit_daemon_event(&inner, "daemon_socket_ready", serde_json::json!({
                    "socket": socket_path.to_string_lossy(),
                })).await;
                let serving = Server::builder()
                    .layer(protection.clone().layer())
                    .add_service(
                        OrchestratorServiceServer::new(service)
                            .max_encoding_message_size(64 * 1024 * 1024),
                    )
                    .serve_with_incoming_shutdown(uds_stream, shutdown_fut);
                tokio::select! {
                    result = serving => result.context("gRPC server error")?,
                    _ = force_server_shutdown(server_shutdown_started.clone()) => {
                        tracing::warn!("forcing gRPC server shutdown after connection drain timeout");
                    }
                }
            }
        }

        emit_daemon_event(&inner, "daemon_shutdown_requested", serde_json::json!({
            "reason": shutdown_reason(&inner, restart_rx.borrow().as_ref()),
        }))
        .await;

        // Server has shut down — notify workers to stop
        info!("signalling workers to shut down");
        inner.daemon_runtime.request_shutdown();
        let _ = shutdown_tx.send(true);
        let _ = clear_worker_stop_signal(&inner);

        let draining_tasks = agent_orchestrator::service::daemon::runtime_snapshot(&inner).running_tasks;
        if draining_tasks > 0 {
            emit_daemon_event(&inner, "task_drain_started", serde_json::json!({
                "running_tasks": draining_tasks,
                "timeout_ms": 5_000_u64,
            }))
            .await;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let remaining = agent_orchestrator::service::daemon::runtime_snapshot(&inner).running_tasks;
            if remaining > 0 {
                shutdown_running_tasks(inner.clone()).await;
            }
            emit_daemon_event(&inner, "task_drain_completed", serde_json::json!({
                "remaining_after_grace": remaining,
                "forced_task_count": remaining,
            }))
            .await;
        }

        // Drain interactive sessions.
        //
        // `shutdown_running_tasks` above cannot reach them: a tty child is never
        // stored in `runtime.child`, because the tty branch of
        // `phase_runner::spawn` returns before the assignment. Every kill path
        // in the scheduler goes through that field, so this is not a further
        // layer of defence over an existing one — it is the only graceful
        // reclamation an interactive session has ever had (FR-159).
        //
        // Best-effort by construction: under `SIGKILL` this never runs, which is
        // why the periodic reconciliation is the real backstop. `Graceful` here
        // rather than the reconciler's `Immediate` because these sessions are
        // still healthy and may flush on `SIGTERM` — every group in the recorded
        // triage exited on `SIGTERM` without needing `SIGKILL`.
        {
            let sessions = agent_orchestrator::session_store::live_sessions_by_path(
                &inner.db_path,
            );
            match sessions {
                Ok(candidates) if !candidates.is_empty() => {
                    emit_daemon_event(&inner, "session_drain_started", serde_json::json!({
                        "sessions": candidates.len(),
                    }))
                    .await;
                    reclaim_orphaned_sessions(
                        &inner,
                        &candidates,
                        agent_orchestrator::session_store::ReclaimSignal::Graceful,
                    )
                    .await;
                    emit_daemon_event(&inner, "session_drain_completed", serde_json::json!({
                        "sessions": candidates.len(),
                    }))
                    .await;
                }
                Ok(_) => {}
                Err(error) if DATA_DIR_VANISHED.load(Ordering::SeqCst) => {
                    // The database went with the data directory, so this query
                    // cannot succeed and its failure carries no new information —
                    // the watcher already said what happened, at error level. Left
                    // at warn rather than silenced: "the drain did not run" is
                    // still a fact about this shutdown. Raising it to error here
                    // would train readers to skip an error line that is expected
                    // on the one path where it is guaranteed.
                    tracing::warn!(
                        %error,
                        "skipping the interactive-session drain: the data directory is gone"
                    );
                }
                Err(error) => {
                    error!(%error, "failed to enumerate interactive sessions for shutdown drain");
                }
            }
        }

        // Wait for supervisor (and all workers) to finish
        match tokio::time::timeout(std::time::Duration::from_secs(30), supervisor_handle).await {
            Ok(Ok(())) => {
                info!("all workers stopped");
            }
            Ok(Err(e)) => {
                error!(error = %e, "worker supervisor panicked");
            }
            Err(_) => {
                error!("timed out waiting for workers to drain (30s)");
            }
        }

        // Check if this was a restart request
        if let Some(binary_path) = restart_rx.borrow().clone() {
            info!(binary = %binary_path.display(), "exec-ing new daemon binary");
            // Keep the PID file intact: exec() preserves the PID, so the file
            // remains valid and prevents other processes from starting a
            // competing daemon during the restart window.

            // Targeted reset: only pause tasks that requested the restart
            // (status = restart_pending). Other tasks were already drained via
            // the deferred-restart mechanism and should not be disturbed.
            match inner
                .task_repo
                .pause_restart_pending_tasks_and_items()
                .await
            {
                Ok(count) if count > 0 => {
                    info!(count, "reset restart-pending items before exec");
                }
                Err(e) => {
                    error!(
                        error = %e,
                        "failed to reset restart-pending items before exec"
                    );
                }
                _ => {}
            }

            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new(&binary_path)
                .args(std::env::args_os().skip(1))
                .exec();
            // exec() only returns on error
            error!("exec failed: {}", err);
            std::process::exit(1);
        }

        // Normal shutdown
        inner.daemon_runtime.mark_stopped();
        lifecycle::cleanup(&socket_path, socket_identity, &pid_path);
        emit_daemon_event(&inner, "daemon_shutdown_completed", serde_json::json!({
            "reason": shutdown_reason(&inner, restart_rx.borrow().as_ref()),
        }))
        .await;
        info!("orchestratord stopped");
        Ok(())
    })
}

fn handle_subcommand(command: Commands) -> Result<()> {
    match command {
        Commands::ControlPlane(ControlPlaneCommands::IssueClient {
            bind,
            subject,
            role,
            home,
            control_plane_dir,
        }) => {
            let state = agent_orchestrator::service::bootstrap::init_state(false)
                .context("failed to initialize orchestrator state")?;
            let addr = bind.parse().context("invalid bind address")?;
            let home = home
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                .ok_or_else(|| anyhow::anyhow!("HOME is not set; pass --home explicitly"))?;
            let client_dir = control_plane::issue_client_materials(
                &state.inner.data_dir,
                &addr,
                control_plane_dir.as_deref(),
                &home,
                &subject,
                role,
            )?;
            println!("{}", client_dir.display());
            Ok(())
        }
        Commands::WebhookSecret { control_plane_dir } => {
            let data_dir = agent_orchestrator::config_load::data_dir();
            match control_plane::derive_webhook_secret(&data_dir, control_plane_dir.as_deref()) {
                Some(secret) => {
                    println!("{secret}");
                    Ok(())
                }
                None => {
                    bail!(
                        "no control-plane CA certificate found; \
                           run the daemon with --bind first to bootstrap PKI"
                    )
                }
            }
        }
    }
}

/// Outcome of a single worker iteration (one poll cycle).
enum WorkerIterationOutcome {
    /// Continue polling for more tasks.
    Continue,
    /// Worker should shut down cleanly.
    Shutdown,
    /// A restart was requested; propagate the binary path.
    RestartRequested(std::path::PathBuf),
}

/// Execute a single worker iteration: acquire permit, claim task, run it.
async fn worker_iteration(
    state: &Arc<InnerState>,
    worker_num: usize,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
    is_busy: &mut bool,
) -> WorkerIterationOutcome {
    let stop_path = worker_stop_signal_path(state);
    let poll_interval = std::time::Duration::from_millis(2000);

    // Check shutdown
    if *shutdown.borrow() {
        return WorkerIterationOutcome::Shutdown;
    }

    // Check external stop signal file
    if stop_path.exists() {
        info!(worker = worker_num, "stop signal detected, exiting");
        return WorkerIterationOutcome::Shutdown;
    }

    // Acquire concurrency permit
    let permit = match task_semaphore().clone().acquire_owned().await {
        Ok(p) => p,
        Err(_) => {
            info!(worker = worker_num, "semaphore closed, exiting");
            return WorkerIterationOutcome::Shutdown;
        }
    };

    match claim_next_pending_task(state).await {
        Ok(Some(task_id)) => {
            info!(worker = worker_num, %task_id, "claimed task");
            let runtime = RunningTask::new();
            state.daemon_runtime.worker_became_busy();
            *is_busy = true;
            emit_daemon_event(
                state,
                "worker_state_changed",
                serde_json::json!({
                    "worker_id": worker_num,
                    "from_state": "idle",
                    "to_state": "busy",
                    "task_id": task_id,
                }),
            )
            .await;
            let _ = register_running_task(state, &task_id, runtime.clone()).await;
            let run_result = run_task_loop(state.clone(), &task_id, runtime).await;
            unregister_running_task(state, &task_id).await;

            // Check if a deferred restart can now proceed (all other tasks drained).
            if let Some(binary_path) = state.daemon_runtime.take_deferred_restart() {
                let running_count = {
                    let running = state.running.lock().await;
                    running.len()
                };
                if running_count == 0 {
                    info!(
                        worker = worker_num,
                        "all tasks drained, executing deferred restart"
                    );
                    state.daemon_runtime.request_shutdown();
                    return WorkerIterationOutcome::RestartRequested(binary_path);
                } else {
                    // Not yet drained, put the binary path back.
                    state.daemon_runtime.set_deferred_restart(binary_path);
                }
            }

            state.daemon_runtime.worker_became_idle();
            *is_busy = false;
            emit_daemon_event(
                state,
                "worker_state_changed",
                serde_json::json!({
                    "worker_id": worker_num,
                    "from_state": "busy",
                    "to_state": "idle",
                    "task_id": task_id,
                }),
            )
            .await;
            match run_result {
                Ok(()) => {
                    if let Ok(summary) = load_task_summary(state, &task_id).await {
                        info!(worker = worker_num, %task_id, status = %summary.status, "task finished");
                    }
                }
                Err(e) => {
                    if let Some(restart) = e.downcast_ref::<RestartRequestedError>() {
                        let other_running = {
                            let running = state.running.lock().await;
                            running
                                .keys()
                                .filter(|id| id.as_str() != task_id.as_str())
                                .count()
                        };
                        if other_running == 0 {
                            info!(worker = worker_num, "restart requested, signalling daemon");
                            state.daemon_runtime.request_shutdown();
                            return WorkerIterationOutcome::RestartRequested(
                                restart.binary_path.clone(),
                            );
                        } else {
                            info!(
                                worker = worker_num,
                                other_tasks = other_running,
                                "deferring restart until other tasks complete"
                            );
                            state
                                .daemon_runtime
                                .set_deferred_restart(restart.binary_path.clone());
                            // Task remains in restart_pending status.
                            // Worker will continue to next iteration.
                        }
                    }
                    error!(worker = worker_num, %task_id, error = %e, "task failed");
                }
            }
            drop(permit);
        }
        Ok(None) => {
            drop(permit);
            // No task available — wait for in-process wakeup, timeout fallback, or shutdown.
            tokio::select! {
                _ = state.worker_notify.notified() => {}
                _ = tokio::time::sleep(poll_interval) => {}
                _ = shutdown.changed() => {}
            }
        }
        Err(e) => {
            drop(permit);
            error!(worker = worker_num, error = %e, "claim error");
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                _ = shutdown.changed() => {}
            }
        }
    }
    WorkerIterationOutcome::Continue
}

/// Background worker loop: polls for pending tasks, claims and executes them.
/// Wraps each iteration in catch_unwind so panics are recovered instead of killing the worker.
async fn worker_loop(
    state: Arc<InnerState>,
    worker_idx: usize,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    restart_tx: tokio::sync::watch::Sender<Option<std::path::PathBuf>>,
) {
    let worker_num = worker_idx + 1;

    state.daemon_runtime.worker_started();
    emit_daemon_event(
        &state,
        "worker_state_changed",
        serde_json::json!({
            "worker_id": worker_num,
            "from_state": "new",
            "to_state": "idle",
        }),
    )
    .await;
    info!(worker = worker_num, "worker started");

    let mut is_busy = false;

    loop {
        // Shutdown/stop checks are infallible — check before entering catch_unwind.
        if *shutdown.borrow() {
            break;
        }

        let result = std::panic::AssertUnwindSafe(worker_iteration(
            &state,
            worker_num,
            &mut shutdown,
            &mut is_busy,
        ))
        .catch_unwind()
        .await;

        match result {
            Ok(WorkerIterationOutcome::Continue) => {}
            Ok(WorkerIterationOutcome::Shutdown) => break,
            Ok(WorkerIterationOutcome::RestartRequested(binary_path)) => {
                let _ = restart_tx.send(Some(binary_path));
                break;
            }
            Err(_panic) => {
                error!(worker = worker_num, "worker iteration panicked, recovering");
                state.daemon_runtime.record_worker_restart();

                // If we panicked while busy, fix the counters
                if is_busy {
                    state.daemon_runtime.worker_became_idle();
                    is_busy = false;
                }

                emit_daemon_event(
                    &state,
                    "worker_panic_recovered",
                    serde_json::json!({ "worker_id": worker_num }),
                )
                .await;

                // Brief delay before retrying to avoid tight panic loops
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        }
    }

    state.daemon_runtime.worker_stopped(false);
    emit_daemon_event(
        &state,
        "worker_state_changed",
        serde_json::json!({
            "worker_id": worker_num,
            "from_state": "idle",
            "to_state": "stopped",
        }),
    )
    .await;
    info!(worker = worker_num, "worker stopped");
}

/// Supervisor that spawns and monitors workers, respawning any that finish unexpectedly.
async fn worker_supervisor(
    state: Arc<InnerState>,
    worker_count: usize,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    restart_tx: tokio::sync::watch::Sender<Option<std::path::PathBuf>>,
) {
    let mut handles: Vec<(usize, tokio::task::JoinHandle<()>)> = Vec::with_capacity(worker_count);

    // Spawn initial workers
    for idx in 0..worker_count {
        let rx = shutdown.clone();
        let st = state.clone();
        let rtx = restart_tx.clone();
        let handle = tokio::spawn(worker_loop(st, idx, rx, rtx));
        handles.push((idx, handle));
    }
    info!(workers = worker_count, "initial workers spawned");

    let health_interval = std::time::Duration::from_secs(30);

    loop {
        tokio::select! {
            _ = tokio::time::sleep(health_interval) => {}
            _ = shutdown.changed() => {
                // Shutdown requested — stop respawning
                break;
            }
        }

        if *shutdown.borrow() {
            break;
        }

        // Health check: find finished workers and respawn them
        let mut respawn_indices = Vec::new();
        for (idx, (worker_idx, handle)) in handles.iter().enumerate() {
            if handle.is_finished() {
                info!(
                    worker = worker_idx + 1,
                    "detected dead worker, scheduling respawn"
                );
                respawn_indices.push((idx, *worker_idx));
            }
        }

        for (vec_idx, worker_idx) in respawn_indices.into_iter().rev() {
            let (_, old_handle) = handles.remove(vec_idx);
            if let Err(e) = old_handle.await {
                error!(worker = worker_idx + 1, error = %e, "dead worker had panicked");
            }

            // Brief delay before respawn
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            if *shutdown.borrow() {
                break;
            }

            let rx = shutdown.clone();
            let st = state.clone();
            let rtx = restart_tx.clone();
            let handle = tokio::spawn(worker_loop(st, worker_idx, rx, rtx));
            handles.push((worker_idx, handle));
            state.daemon_runtime.record_worker_restart();

            emit_daemon_event(
                &state,
                "worker_respawned",
                serde_json::json!({ "worker_id": worker_idx + 1 }),
            )
            .await;
            info!(worker = worker_idx + 1, "worker respawned by supervisor");
        }

        // Warn if live workers are below configured count
        let live = handles.iter().filter(|(_, h)| !h.is_finished()).count();
        if live < worker_count {
            tracing::warn!(
                live_workers = live,
                configured = worker_count,
                "live workers below configured count"
            );
        }
    }

    // Wait for all workers to finish
    for (worker_idx, handle) in handles {
        if let Err(e) = handle.await {
            error!(worker = worker_idx + 1, error = %e, "worker panicked during shutdown");
        }
    }
}

/// Reads the effective `session_reclaim_enabled` policy.
///
/// Fails closed: if the active configuration cannot be read at all, no signal is
/// sent. A daemon that cannot tell whether it is permitted to kill processes
/// must not kill any.
fn session_reclaim_enabled(state: &InnerState) -> bool {
    use agent_orchestrator::config_ext::OrchestratorConfigExt;
    agent_orchestrator::config_load::read_active_config(state)
        .map(|active| {
            active
                .config
                .global_runtime_policy()
                .session_reclaim_enabled
        })
        .unwrap_or(false)
}

/// Reclaims the process groups of sessions reconciliation found orphaned.
///
/// The kill lives here rather than in `reconcile_sessions` for two reasons: this
/// is the layer that can read policy and reach the event sink, and the identity
/// re-check that `reclaim_process_group` performs is only meaningful if it runs
/// adjacent to the signal rather than inside an earlier database pass.
///
/// Every outcome is recorded — reclaimed and refused alike. A refusal is the
/// interesting case: it is the daemon declining to signal a PID it cannot prove
/// is the right one, and it needs to be visible rather than silent, both so an
/// operator can see a PID-reuse near miss and so QA can assert that a mismatched
/// fingerprint produced no signal.
async fn reclaim_orphaned_sessions(
    state: &InnerState,
    candidates: &[agent_orchestrator::session_store::ReclaimCandidate],
    signal: agent_orchestrator::session_store::ReclaimSignal,
) {
    use agent_orchestrator::session_store::reclaim_process_group;

    if candidates.is_empty() {
        return;
    }
    if !session_reclaim_enabled(state) {
        info!(
            candidates = candidates.len(),
            "session reclamation is disabled by runtime policy; leaving orphaned processes alone"
        );
        return;
    }

    for candidate in candidates {
        let result = reclaim_process_group(
            candidate.pid,
            candidate.process_fingerprint.as_deref(),
            signal,
        );
        let payload = match &result {
            Ok(sent) => {
                // The directory goes only after a signal was actually sent, so
                // a refusal never destroys the evidence of what it refused.
                let removed_dir = candidate.session_dir.as_ref().map(|dir| {
                    let removed = std::fs::remove_dir_all(dir).is_ok();
                    serde_json::json!({ "path": dir.to_string_lossy(), "removed": removed })
                });
                info!(
                    session_id = %candidate.session_id,
                    pid = candidate.pid,
                    sigterm = sent.sigterm,
                    sigkill = sent.sigkill,
                    "reclaimed orphaned interactive session process group"
                );
                serde_json::json!({
                    "session_id": candidate.session_id,
                    "pid": candidate.pid,
                    "process_fingerprint": candidate.process_fingerprint,
                    "outcome": "reclaimed",
                    "sigterm": sent.sigterm,
                    "sigkill": sent.sigkill,
                    "exited_on_sigterm": sent.exited_on_sigterm,
                    "session_dir": removed_dir,
                })
            }
            Err(refusal) => {
                info!(
                    session_id = %candidate.session_id,
                    pid = candidate.pid,
                    reason = refusal.as_str(),
                    "refused to reclaim interactive session process group"
                );
                serde_json::json!({
                    "session_id": candidate.session_id,
                    "pid": candidate.pid,
                    "process_fingerprint": candidate.process_fingerprint,
                    "outcome": "refused",
                    "reason": refusal.as_str(),
                })
            }
        };
        let _ = insert_event(
            state,
            &candidate.task_id,
            None,
            "session_process_reclaimed",
            payload.clone(),
        )
        .await;
        state.emit_event(
            &candidate.task_id,
            None,
            "session_process_reclaimed",
            payload,
        );
    }
}

async fn emit_daemon_event(state: &InnerState, event_type: &str, payload: serde_json::Value) {
    let _ = insert_event(state, "", None, event_type, payload.clone()).await;
    state.emit_event("", None, event_type, payload);
}

fn shutdown_reason(
    state: &InnerState,
    restart_binary: Option<&std::path::PathBuf>,
) -> &'static str {
    if DATA_DIR_VANISHED.load(Ordering::SeqCst) {
        // Ahead of the other arms on purpose: once the data directory is gone,
        // `worker_stop_signal_path(state).exists()` is false because the path it
        // reads is inside that directory, and `shutdown_requested` is true because
        // the watcher set it. Both of those would name the wrong cause.
        "data_dir_vanished"
    } else if restart_binary.is_some() {
        "restart"
    } else if worker_stop_signal_path(state).exists() {
        "external_stop_signal"
    } else if state.daemon_runtime.snapshot().shutdown_requested {
        "shutdown"
    } else {
        "unknown"
    }
}

/// Warn if the data directory has group or world read/write bits set.
fn check_data_dir_permissions(data_dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(data_dir) {
        let mode = meta.permissions().mode();
        if mode & 0o077 != 0 {
            tracing::warn!(
                data_dir = %data_dir.display(),
                mode = format!("{mode:#o}"),
                "data directory has group/world-accessible permissions; \
                 consider restricting to 0700 for multi-user hosts"
            );
        }
    }
}
