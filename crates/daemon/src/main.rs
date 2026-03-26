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
mod protection;
mod server;
mod webhook;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures::FutureExt;
use tonic::transport::Server;
use tracing::{error, info};

use agent_orchestrator::events::insert_event;
use agent_orchestrator::scheduler_service::{
    claim_next_pending_task, clear_worker_stop_signal, worker_stop_signal_path,
};
use agent_orchestrator::state::{InnerState, task_semaphore};
use orchestrator_proto::OrchestratorServiceServer;
use orchestrator_scheduler::scheduler::safety::RestartRequestedError;
use orchestrator_scheduler::scheduler::{
    RunningTask, load_task_summary, register_running_task, run_task_loop, shutdown_running_tasks,
    unregister_running_task,
};

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

    /// Bind address for the HTTP webhook server (disabled if not set).
    #[arg(long = "webhook-bind")]
    webhook_bind: Option<String>,

    /// Shared secret for webhook HMAC-SHA256 signature verification.
    #[arg(long = "webhook-secret", env = "ORCHESTRATOR_WEBHOOK_SECRET")]
    webhook_secret: Option<String>,

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

fn main() -> Result<()> {
    let args = Args::parse();

    // Daemonize before starting any threads or the tokio runtime.
    // In daemon mode, stdout/stderr are redirected to data/daemon.log
    // so ANSI escape codes are disabled.
    let use_ansi = if args.foreground {
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
        let state = agent_orchestrator::service::bootstrap::init_state_async(false)
            .await
            .context("failed to initialize orchestrator state")?;
        let inner = state.inner.clone();
        inner.daemon_runtime.set_configured_workers(args.workers);

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

        // Spawn event cleanup sweep (TTL-based)
        if args.event_retention_days > 0 {
            let cleanup_state = inner.clone();
            let mut cleanup_shutdown = shutdown_rx.clone();
            let retention_days = args.event_retention_days;
            let archive_enabled = args.event_archive_enabled;
            let archive_dir = args
                .event_archive_dir
                .clone()
                .unwrap_or_else(|| inner.data_dir.join("archive/events"));
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
                            if log_days > 0 {
                                if let Err(e) = agent_orchestrator::log_cleanup::cleanup_old_logs(
                                    &lifecycle_state.async_database,
                                    &lifecycle_state.logs_dir,
                                    log_days,
                                ).await {
                                    tracing::warn!(error = %e, "log cleanup sweep failed");
                                }
                            }
                            if task_days > 0 {
                                if let Err(e) = agent_orchestrator::task_cleanup::cleanup_old_tasks(
                                    &lifecycle_state.async_database,
                                    &lifecycle_state.logs_dir,
                                    task_days,
                                    50,
                                ).await {
                                    tracing::warn!(error = %e, "task cleanup sweep failed");
                                }
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
                            match stall_state.task_repo.recover_stalled_running_items(stall_threshold_secs).await {
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

        // Spawn webhook HTTP server if configured.
        if let Some(ref webhook_addr) = args.webhook_bind {
            let addr: std::net::SocketAddr = webhook_addr
                .parse()
                .context("invalid --webhook-bind address")?;
            let wh_state = webhook::WebhookState {
                inner: inner.clone(),
                secret: args.webhook_secret.clone(),
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
        }

        let shutdown_notify = Arc::new(tokio::sync::Notify::new());

        let protection = Arc::new(protection::ControlPlaneProtection::load_or_bootstrap(
            &inner.data_dir,
            &inner.db_path,
            args.control_plane_dir.as_deref(),
        )?);

        let service = server::OrchestratorServer::new(
            inner.clone(),
            shutdown_notify.clone(),
            None,
        );

        // Shutdown future: listen for OS signals, restart request, or RPC shutdown
        let shutdown_fut = {
            let inner2 = inner.clone();
            let mut restart_rx2 = restart_rx.clone();
            let notify = shutdown_notify.clone();
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
            }
        };

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
            Server::builder()
                .layer(protection.clone().layer())
                .tls_config(secure.tls)?
                .add_service(
                    OrchestratorServiceServer::new(server::OrchestratorServer::new(
                        inner.clone(),
                        shutdown_notify.clone(),
                        Some(secure.security),
                    ))
                    .max_encoding_message_size(64 * 1024 * 1024),
                )
                .serve_with_shutdown(addr, shutdown_fut)
                .await
                .context("gRPC server error")?;
        } else {
            #[cfg(feature = "dev-insecure")]
            let insecure_addr = args.insecure_bind.as_deref();
            #[cfg(not(feature = "dev-insecure"))]
            let insecure_addr: Option<&str> = None;

            if let Some(addr) = insecure_addr {
                let addr = addr.parse().context("invalid insecure bind address")?;
                info!(%addr, "listening on insecure TCP");
                tracing::warn!("insecure TCP control-plane enabled; use only for local development");
                Server::builder()
                    .layer(protection.clone().layer())
                    .add_service(
                        OrchestratorServiceServer::new(service)
                            .max_encoding_message_size(64 * 1024 * 1024),
                    )
                    .serve_with_shutdown(addr, shutdown_fut)
                    .await
                    .context("gRPC server error")?;
            } else {
                // UDS transport
                use tokio::net::UnixListener;

                // Remove stale socket
                let _ = std::fs::remove_file(&socket_path);
                let uds = UnixListener::bind(&socket_path).context("failed to bind UDS")?;
                let uds_stream = tokio_stream::wrappers::UnixListenerStream::new(uds);

                info!(socket = %socket_path.display(), "listening on UDS");
                emit_daemon_event(&inner, "daemon_socket_ready", serde_json::json!({
                    "socket": socket_path.to_string_lossy(),
                })).await;
                Server::builder()
                    .layer(protection.clone().layer())
                    .add_service(
                        OrchestratorServiceServer::new(service)
                            .max_encoding_message_size(64 * 1024 * 1024),
                    )
                    .serve_with_incoming_shutdown(uds_stream, shutdown_fut)
                    .await
                    .context("gRPC server error")?;
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

            // Blanket-reset: ensure no running items/tasks survive across exec().
            // Handles race where requesting worker already removed its task
            // from state.running before shutdown_running_tasks could pause it.
            match inner.task_repo.pause_all_running_tasks_and_items().await {
                Ok(count) if count > 0 => {
                    info!(count, "blanket-reset running items before exec");
                }
                Err(e) => {
                    error!(error = %e, "failed to blanket-reset running items before exec");
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
        lifecycle::cleanup(&socket_path, &pid_path);
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
                        info!(worker = worker_num, "restart requested, signalling daemon");
                        state.daemon_runtime.request_shutdown();
                        return WorkerIterationOutcome::RestartRequested(
                            restart.binary_path.clone(),
                        );
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

async fn emit_daemon_event(state: &InnerState, event_type: &str, payload: serde_json::Value) {
    let _ = insert_event(state, "", None, event_type, payload.clone()).await;
    state.emit_event("", None, event_type, payload);
}

fn shutdown_reason(
    state: &InnerState,
    restart_binary: Option<&std::path::PathBuf>,
) -> &'static str {
    if restart_binary.is_some() {
        "restart"
    } else if worker_stop_signal_path(state).exists() {
        "external_stop_signal"
    } else if state.daemon_runtime.snapshot().shutdown_requested {
        "shutdown"
    } else {
        "unknown"
    }
}
