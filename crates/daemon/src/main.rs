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
mod lifecycle;
mod protection;
mod server;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures::FutureExt;
use tonic::transport::Server;
use tracing::{error, info};

use agent_orchestrator::events::insert_event;
use agent_orchestrator::scheduler::safety::RestartRequestedError;
use agent_orchestrator::scheduler::{
    load_task_summary, register_running_task, run_task_loop, shutdown_running_tasks,
    unregister_running_task, RunningTask,
};
use agent_orchestrator::scheduler_service::{
    claim_next_pending_task, clear_worker_stop_signal, worker_stop_signal_path,
};
use agent_orchestrator::state::{task_semaphore, InnerState};
use orchestrator_proto::OrchestratorServiceServer;

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

    let subscriber = tracing_subscriber::fmt()
        .with_target(false)
        .with_ansi(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("failed to set tracing subscriber")?;

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

        let socket_path = lifecycle::socket_path(&inner.app_root);
        let pid_path = lifecycle::pid_path(&inner.app_root);

        // Write PID file
        lifecycle::write_pid_file(&pid_path)?;

        info!(
            socket = %socket_path.display(),
            pid_file = %pid_path.display(),
            version = env!("CARGO_PKG_VERSION"),
            git_hash = env!("BUILD_GIT_HASH"),
            "orchestratord starting"
        );

        // Clear any stale stop signal from a previous run
        let _ = clear_worker_stop_signal(&inner);

        // Shutdown coordination: watch channel shared between server and workers
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        // Restart coordination: worker sends binary path when restart is requested
        let (restart_tx, restart_rx) =
            tokio::sync::watch::channel::<Option<std::path::PathBuf>>(None);

        // Spawn worker tasks
        let mut worker_handles = Vec::with_capacity(args.workers);
        for idx in 0..args.workers {
            let rx = shutdown_rx.clone();
            let st = inner.clone();
            let rtx = restart_tx.clone();
            let handle = tokio::spawn(worker_loop(st, idx, rx, rtx));
            worker_handles.push(handle);
        }
        drop(restart_tx); // drop original sender so only workers hold it
        info!(workers = args.workers, "background workers started");

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
                .unwrap_or_else(|| inner.app_root.join("data/archive/events"));
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

        let shutdown_notify = Arc::new(tokio::sync::Notify::new());

        let protection = Arc::new(protection::ControlPlaneProtection::load_or_bootstrap(
            &inner.app_root,
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
                &inner.app_root,
                &inner.db_path,
                &addr,
                args.control_plane_dir.as_deref(),
            )?;
            info!(%addr, "listening on TCP");
            Server::builder()
                .layer(protection.clone().layer())
                .tls_config(secure.tls)?
                .add_service(OrchestratorServiceServer::new(server::OrchestratorServer::new(
                    inner.clone(),
                    shutdown_notify.clone(),
                    Some(secure.security),
                )))
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
                    .add_service(OrchestratorServiceServer::new(service))
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
                Server::builder()
                    .layer(protection.clone().layer())
                    .add_service(OrchestratorServiceServer::new(service))
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

        // Wait for all workers to finish (with a timeout)
        let drain = futures::future::join_all(worker_handles);
        match tokio::time::timeout(std::time::Duration::from_secs(30), drain).await {
            Ok(results) => {
                for (i, r) in results.into_iter().enumerate() {
                    if let Err(e) = r {
                        error!(worker = i + 1, error = %e, "worker task panicked");
                    }
                }
                info!("all workers stopped");
            }
            Err(_) => {
                error!("timed out waiting for workers to drain (30s)");
            }
        }

        // Check if this was a restart request
        if let Some(binary_path) = restart_rx.borrow().clone() {
            info!(binary = %binary_path.display(), "exec-ing new daemon binary");
            lifecycle::cleanup(&socket_path, &pid_path);

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
                &state.inner.app_root,
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

/// Background worker loop: polls for pending tasks, claims and executes them.
async fn worker_loop(
    state: Arc<InnerState>,
    worker_idx: usize,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    restart_tx: tokio::sync::watch::Sender<Option<std::path::PathBuf>>,
) {
    let stop_path = worker_stop_signal_path(&state);
    let poll_interval = std::time::Duration::from_millis(2000);
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

    loop {
        // Check shutdown
        if *shutdown.borrow() {
            break;
        }

        // Check external stop signal file
        if stop_path.exists() {
            info!(worker = worker_num, "stop signal detected, exiting");
            break;
        }

        // Acquire concurrency permit
        let permit = match task_semaphore().clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                info!(worker = worker_num, "semaphore closed, exiting");
                break;
            }
        };

        match claim_next_pending_task(&state).await {
            Ok(Some(task_id)) => {
                info!(worker = worker_num, %task_id, "claimed task");
                let runtime = RunningTask::new();
                state.daemon_runtime.worker_became_busy();
                emit_daemon_event(
                    &state,
                    "worker_state_changed",
                    serde_json::json!({
                        "worker_id": worker_num,
                        "from_state": "idle",
                        "to_state": "busy",
                        "task_id": task_id,
                    }),
                )
                .await;
                let _ = register_running_task(&state, &task_id, runtime.clone()).await;
                let run_result =
                    std::panic::AssertUnwindSafe(run_task_loop(state.clone(), &task_id, runtime))
                        .catch_unwind()
                        .await;
                unregister_running_task(&state, &task_id).await;
                state.daemon_runtime.worker_became_idle();
                emit_daemon_event(
                    &state,
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
                    Ok(Ok(())) => {
                        if let Ok(summary) = load_task_summary(&state, &task_id).await {
                            info!(worker = worker_num, %task_id, status = %summary.status, "task finished");
                        }
                    }
                    Ok(Err(e)) => {
                        if let Some(restart) = e.downcast_ref::<RestartRequestedError>() {
                            info!(worker = worker_num, "restart requested, signalling daemon");
                            state.daemon_runtime.request_shutdown();
                            let _ = restart_tx.send(Some(restart.binary_path.clone()));
                            break;
                        }
                        error!(worker = worker_num, %task_id, error = %e, "task failed");
                    }
                    Err(panic) => {
                        error!(worker = worker_num, %task_id, "task panicked");
                        drop(panic);
                        break;
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
