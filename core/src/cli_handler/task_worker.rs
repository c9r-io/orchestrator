use crate::cli::TaskWorkerCommands;
use crate::scheduler::{load_task_summary, run_task_loop, RunningTask};
use crate::scheduler_service::{
    claim_next_pending_task, clear_worker_stop_signal, pending_task_count, signal_worker_stop,
    worker_stop_signal_path, worker_wake_signal_path,
};
use crate::state::TASK_SEMAPHORE;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex as StdMutex};

use super::{cli_runtime, CliHandler};

impl CliHandler {
    pub(super) fn handle_task_worker(&self, cmd: &TaskWorkerCommands) -> Result<i32> {
        match cmd {
            TaskWorkerCommands::Start { poll_ms, workers } => {
                let worker_count = (*workers).max(1);
                clear_worker_stop_signal(&self.state)?;
                println!(
                    "Worker started (poll={}ms, workers={})",
                    poll_ms, worker_count
                );
                let stop_file = worker_stop_signal_path(&self.state);
                let wake_file = worker_wake_signal_path(&self.state);
                if let Some(parent) = wake_file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if !wake_file.exists() {
                    std::fs::write(&wake_file, "init")?;
                }
                let wake_pair: Arc<(StdMutex<u64>, Condvar)> =
                    Arc::new((StdMutex::new(0), Condvar::new()));
                let watching = Arc::new(AtomicBool::new(true));
                let wake_pair_monitor = wake_pair.clone();
                let wake_file_monitor = wake_file.clone();
                let watching_monitor = watching.clone();
                let wake_monitor = std::thread::spawn(move || {
                    let mut last_modified = std::fs::metadata(&wake_file_monitor)
                        .and_then(|meta| meta.modified())
                        .ok();
                    while watching_monitor.load(AtomicOrdering::SeqCst) {
                        let current_modified = std::fs::metadata(&wake_file_monitor)
                            .and_then(|meta| meta.modified())
                            .ok();
                        if current_modified != last_modified {
                            last_modified = current_modified;
                            let (lock, cv) = &*wake_pair_monitor;
                            if let Ok(mut version) = lock.lock() {
                                *version += 1;
                                cv.notify_all();
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                });

                let mut handles = Vec::new();
                for worker_idx in 0..worker_count {
                    let state = self.state.clone();
                    let stop_file = stop_file.clone();
                    let wake_pair = wake_pair.clone();
                    let poll_ms = *poll_ms;
                    handles.push(std::thread::spawn(move || -> Result<()> {
                        loop {
                            if stop_file.exists() {
                                break;
                            }
                            let permit = cli_runtime()
                                .block_on(TASK_SEMAPHORE.clone().acquire_owned())
                                .map_err(|e| {
                                    anyhow::anyhow!("Failed to acquire semaphore: {}", e)
                                })?;
                            if let Some(task_id) = claim_next_pending_task(&state)? {
                                println!("Worker-{} claimed task: {}", worker_idx + 1, task_id);

                                // Post-restart binary verification: if this task was
                                // restart_pending, confirm the running binary matches
                                // the SHA256 recorded before the restart.
                                match crate::scheduler::safety::verify_post_restart_binary(&state, &task_id) {
                                    Ok(true) => {} // verified or no event to check
                                    Ok(false) => {
                                        eprintln!(
                                            "Worker-{} WARNING: binary SHA256 mismatch for restart task {}",
                                            worker_idx + 1, task_id
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "Worker-{} binary verification skipped: {}",
                                            worker_idx + 1, e
                                        );
                                    }
                                }

                                let runtime = RunningTask::new();
                                let run_res = cli_runtime().block_on(run_task_loop(
                                    state.clone(),
                                    &task_id,
                                    runtime,
                                ));
                                drop(permit);

                                match run_res {
                                    Ok(()) => {
                                        let summary = load_task_summary(&state, &task_id)?;
                                        println!(
                                            "Worker-{} finished task: {} status={}",
                                            worker_idx + 1,
                                            summary.id,
                                            summary.status
                                        );
                                    }
                                    Err(err) => {
                                        eprintln!(
                                            "Worker-{} task failed: {} error={}",
                                            worker_idx + 1,
                                            task_id,
                                            err
                                        );
                                    }
                                }
                            } else {
                                drop(permit);
                                let (lock, cv) = &*wake_pair;
                                let mut version = lock
                                    .lock()
                                    .map_err(|_| anyhow::anyhow!("worker wake lock poisoned"))?;
                                let start_version = *version;
                                while *version == start_version && !stop_file.exists() {
                                    let (new_version, _timeout) = cv
                                        .wait_timeout(
                                            version,
                                            std::time::Duration::from_millis(poll_ms),
                                        )
                                        .map_err(|_| {
                                            anyhow::anyhow!("worker wake condvar poisoned")
                                        })?;
                                    version = new_version;
                                    if *version != start_version {
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(())
                    }));
                }

                let mut worker_errors = Vec::new();
                for handle in handles {
                    match handle.join() {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => worker_errors.push(e),
                        Err(_) => {
                            worker_errors.push(anyhow::anyhow!("worker thread panicked"))
                        }
                    }
                }
                watching.store(false, AtomicOrdering::SeqCst);
                {
                    let (lock, cv) = &*wake_pair;
                    if let Ok(mut version) = lock.lock() {
                        *version += 1;
                        cv.notify_all();
                    }
                }
                let _ = wake_monitor.join();
                // Always clear stop signal, even if workers errored
                clear_worker_stop_signal(&self.state)?;
                println!("Worker stopped");
                if let Some(first_err) = worker_errors.into_iter().next() {
                    return Err(first_err);
                }
                Ok(0)
            }
            TaskWorkerCommands::Stop => {
                signal_worker_stop(&self.state)?;
                println!("Worker stop signal written");
                Ok(0)
            }
            TaskWorkerCommands::Status => {
                let pending = pending_task_count(&self.state)?;
                let stop_signal = worker_stop_signal_path(&self.state).exists();
                println!("pending_tasks: {}", pending);
                println!("stop_signal: {}", stop_signal);
                Ok(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::CliHandler;
    use crate::cli::{Cli, Commands, TaskCommands, TaskWorkerCommands};
    use crate::db::open_conn;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    #[test]
    fn worker_start_multi_consumers_drain_pending_queue() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let qa_file = fixture
            .temp_root()
            .join("workspace/default/docs/qa/worker-test.md");
        std::fs::write(&qa_file, "# worker test\n").expect("seed qa file");

        let mut created_ids = Vec::new();
        for i in 0..6 {
            let task = create_task_impl(
                &state,
                CreateTaskPayload {
                    name: Some(format!("worker-multi-{i}")),
                    goal: Some("worker multi".to_string()),
                    ..Default::default()
                },
            )
            .expect("create task");
            created_ids.push(task.id);
        }

        let state_for_thread = state.clone();
        let worker_thread = std::thread::spawn(move || {
            let worker_handler = CliHandler::new(state_for_thread);
            worker_handler.execute(&Cli {
                command: Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Start {
                    poll_ms: 50,
                    workers: 3,
                })),
                verbose: false,
                log_level: None,
                log_format: None,
            })
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let conn = open_conn(&state.db_path).expect("open sqlite");
            let pending: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE status='pending'",
                    [],
                    |row| row.get(0),
                )
                .expect("query pending");
            let running: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE status='running'",
                    [],
                    |row| row.get(0),
                )
                .expect("query running");
            if pending == 0 && running == 0 {
                break;
            }
            if std::time::Instant::now() > deadline {
                assert!(
                    std::time::Instant::now() <= deadline,
                    "timeout waiting worker queue drain"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        handler
            .execute(&Cli {
                command: Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Stop)),
                verbose: false,
                log_level: None,
                log_format: None,
            })
            .expect("stop worker");

        let worker_result = worker_thread.join().expect("worker thread join");
        assert_eq!(worker_result.expect("worker should exit cleanly"), 0);

        let conn = open_conn(&state.db_path).expect("open sqlite");
        for task_id in created_ids {
            let status: String = conn
                .query_row(
                    "SELECT status FROM tasks WHERE id = ?1",
                    rusqlite::params![task_id],
                    |row| row.get(0),
                )
                .expect("load task status");
            assert!(matches!(status.as_str(), "completed" | "failed"));
        }
    }
}
