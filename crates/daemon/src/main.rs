mod lifecycle;
mod server;

use std::sync::Arc;

use anyhow::{Context, Result};
use tonic::transport::Server;
use tracing::{error, info};

use agent_orchestrator::scheduler::safety::RestartRequestedError;
use agent_orchestrator::scheduler::{load_task_summary, run_task_loop, RunningTask};
use agent_orchestrator::scheduler_service::{
    claim_next_pending_task, clear_worker_stop_signal, worker_stop_signal_path,
    worker_wake_signal_path,
};
use agent_orchestrator::state::{task_semaphore, InnerState};
use orchestrator_proto::OrchestratorServiceServer;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let _foreground = args.iter().any(|a| a == "--foreground" || a == "-f");
    let bind_addr = args
        .iter()
        .position(|a| a == "--bind")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let worker_count: usize = args
        .iter()
        .position(|a| a == "--workers")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

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

    rt.block_on(async move {
        let state = agent_orchestrator::service::bootstrap::init_state_async(false)
            .await
            .context("failed to initialize orchestrator state")?;
        let inner = state.inner.clone();

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
        let mut worker_handles = Vec::with_capacity(worker_count);
        for idx in 0..worker_count {
            let rx = shutdown_rx.clone();
            let st = inner.clone();
            let rtx = restart_tx.clone();
            let handle = tokio::spawn(worker_loop(st, idx, rx, rtx));
            worker_handles.push(handle);
        }
        drop(restart_tx); // drop original sender so only workers hold it
        info!(workers = worker_count, "background workers started");

        let service = server::OrchestratorServer::new(inner.clone());
        let grpc_service = OrchestratorServiceServer::new(service);

        // Shutdown future: listen for OS signals OR restart request from a worker
        let shutdown_fut = {
            let inner2 = inner.clone();
            let mut restart_rx2 = restart_rx.clone();
            async move {
                tokio::select! {
                    _ = lifecycle::shutdown_signal(inner2) => {}
                    _ = restart_rx2.changed() => {}
                }
            }
        };

        // Determine bind address: UDS by default, TCP if --bind provided
        if let Some(addr) = bind_addr {
            let addr = addr.parse().context("invalid bind address")?;
            info!(%addr, "listening on TCP");
            Server::builder()
                .add_service(grpc_service)
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
                .add_service(grpc_service)
                .serve_with_incoming_shutdown(uds_stream, shutdown_fut)
                .await
                .context("gRPC server error")?;
        }

        // Server has shut down — notify workers to stop
        info!("signalling workers to shut down");
        let _ = shutdown_tx.send(true);

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
        lifecycle::cleanup(&socket_path, &pid_path);
        info!("orchestratord stopped");
        Ok(())
    })
}

/// Background worker loop: polls for pending tasks, claims and executes them.
async fn worker_loop(
    state: Arc<InnerState>,
    worker_idx: usize,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    restart_tx: tokio::sync::watch::Sender<Option<std::path::PathBuf>>,
) {
    let wake_path = worker_wake_signal_path(&state);
    let stop_path = worker_stop_signal_path(&state);
    let poll_interval = std::time::Duration::from_millis(2000);
    let worker_num = worker_idx + 1;

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
                match run_task_loop(state.clone(), &task_id, runtime).await {
                    Ok(()) => {
                        if let Ok(summary) = load_task_summary(&state, &task_id).await {
                            info!(worker = worker_num, %task_id, status = %summary.status, "task finished");
                        }
                    }
                    Err(e) => {
                        // Check if this is a restart request (not a real error)
                        if let Some(restart) = e.downcast_ref::<RestartRequestedError>() {
                            info!(worker = worker_num, "restart requested, signalling daemon");
                            let _ = restart_tx.send(Some(restart.binary_path.clone()));
                            return; // worker exits cleanly
                        }
                        error!(worker = worker_num, %task_id, error = %e, "task failed");
                    }
                }
                drop(permit);
            }
            Ok(None) => {
                drop(permit);
                // No task available — sleep or wait for wake signal, whichever comes first
                tokio::select! {
                    _ = wait_for_wake_signal(&wake_path) => {
                        // Wake signal received, loop immediately to claim
                    }
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

    info!(worker = worker_num, "worker stopped");
}

/// Wait until the wake signal file appears, then consume it.
async fn wait_for_wake_signal(path: &std::path::Path) {
    // Simple polling for the signal file; this is a lightweight check
    loop {
        if path.exists() {
            let _ = std::fs::remove_file(path);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
