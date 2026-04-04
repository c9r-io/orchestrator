//! Log streaming and file tailing utilities.

use agent_orchestrator::anomaly::AnomalyRule;
use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::config_load::read_loaded_config;
use agent_orchestrator::dto::LogChunk;
use agent_orchestrator::env_resolve::collect_all_sensitive_store_values;
use agent_orchestrator::runner::redact_text;
use agent_orchestrator::state::InnerState;
use anyhow::{Context, Result};
use std::io::{Read, Seek};
use std::path::Path;
use std::time::{Duration, Instant};

use super::emit_anomaly_warning;
use super::task_queries::resolve_task_id;

const FOLLOW_POLL_MS: u64 = 500;
const LOG_UNAVAILABLE_MARKER: &str = "[log unavailable]";

/// Stream task logs with optional tail count and timestamps.
pub async fn stream_task_logs_impl(
    state: &InnerState,
    task_id: &str,
    tail_count: usize,
    show_timestamps: bool,
) -> Result<Vec<LogChunk>> {
    const PER_FILE_LINE_LIMIT: usize = 150;

    let resolved_id = resolve_task_id(state, task_id).await?;
    let runs = state.task_repo.list_task_log_runs(&resolved_id, 14).await?;
    let redaction_patterns = {
        let active = read_loaded_config(state)?;
        let mut patterns = active.config.runtime_policy().runner.redaction_patterns;
        if let Ok(summary) = state.task_repo.load_task_summary(&resolved_id).await {
            let effective = active
                .config
                .effective_project_id(Some(&summary.project_id));
            if let Some(project) = active.config.projects.get(effective) {
                patterns.extend(collect_all_sensitive_store_values(&project.secret_stores));
            }
        }
        patterns
    };

    let mut chunks = Vec::new();
    for row in runs {
        let run_id = row.run_id;
        let phase = row.phase;
        let stdout_path = row.stdout_path;
        let stderr_path = row.stderr_path;
        let started_at = row.started_at;
        let stdout_tail = tail_lines(Path::new(&stdout_path), PER_FILE_LINE_LIMIT)
            .with_context(|| format!("read stdout tail for run_id={run_id} path={stdout_path}"))
            .ok()
            .unwrap_or_default();
        let stderr_tail = tail_lines(Path::new(&stderr_path), PER_FILE_LINE_LIMIT)
            .with_context(|| format!("read stderr tail for run_id={run_id} path={stderr_path}"))
            .ok()
            .unwrap_or_default();

        let header = if show_timestamps {
            let ts = started_at.as_deref().unwrap_or("unknown");
            format!("[{}][{}][{}]", ts, run_id, phase)
        } else {
            format!("[{}][{}]", run_id, phase)
        };

        let log_body = if stdout_tail.is_empty() && stderr_tail.is_empty() {
            format!(
                "{} (stdout={}, stderr={})",
                LOG_UNAVAILABLE_MARKER, stdout_path, stderr_path
            )
        } else {
            redact_text(&stdout_tail, &redaction_patterns)
        };
        let content = format!(
            "{}\n{}{}",
            header,
            log_body,
            if stderr_tail.is_empty() {
                String::new()
            } else {
                format!(
                    "\n[stderr]\n{}",
                    redact_text(&stderr_tail, &redaction_patterns)
                )
            }
        );
        chunks.push(LogChunk {
            run_id,
            phase,
            content,
            stdout_path,
            stderr_path,
            started_at,
        });
    }
    chunks.reverse();

    if tail_count < chunks.len() {
        chunks = chunks.split_off(chunks.len() - tail_count);
    }

    Ok(chunks)
}

/// Follow task logs in real-time, sending each chunk via `output_fn`.
///
/// `output_fn(text, is_stderr)` is called synchronously for every log chunk.
pub async fn follow_task_logs<F>(state: &InnerState, task_id: &str, output_fn: &mut F) -> Result<()>
where
    F: FnMut(String, bool) -> anyhow::Result<()>,
{
    let redaction_patterns = {
        let active = read_loaded_config(state)?;
        let mut patterns = active.config.runtime_policy().runner.redaction_patterns;
        let resolved_id = resolve_task_id(state, task_id).await?;
        if let Ok(summary) = state.task_repo.load_task_summary(&resolved_id).await {
            let effective = active
                .config
                .effective_project_id(Some(&summary.project_id));
            if let Some(project) = active.config.projects.get(effective) {
                patterns.extend(collect_all_sensitive_store_values(&project.secret_stores));
            }
        }
        patterns
    };

    let mut stdout_pos: u64 = 0;
    let mut stderr_pos: u64 = 0;
    let mut current_phase = String::new();
    let mut waiting_notice_printed = false;
    let mut last_warning_at: Option<Instant> = None;

    loop {
        let latest =
            agent_orchestrator::events::query_latest_step_log_paths_async(state, task_id).await;
        let (phase, stdout_path, stderr_path) = match latest {
            Ok(Some(info)) => info,
            Ok(None) => {
                if !waiting_notice_printed {
                    output_fn("[waiting for first log stream]\n".to_string(), true)?;
                    waiting_notice_printed = true;
                }
                if let Ok(Some(status)) = state.task_repo.load_task_status(task_id).await {
                    if status == "completed" || status == "failed" {
                        output_fn(format!("\n--- task {} ---\n", status), true)?;
                        return Ok(());
                    }
                }
                tokio::time::sleep(Duration::from_millis(FOLLOW_POLL_MS)).await;
                continue;
            }
            Err(err) => {
                emit_anomaly_warning(
                    &AnomalyRule::TransientReadError,
                    &format!("{err}"),
                    &mut last_warning_at,
                );
                tokio::time::sleep(Duration::from_millis(FOLLOW_POLL_MS)).await;
                continue;
            }
        };

        if phase != current_phase {
            if !current_phase.is_empty() {
                output_fn(
                    format!("\n--- step changed: {} -> {} ---\n", current_phase, phase),
                    true,
                )?;
            }
            current_phase = phase;
            stdout_pos = 0;
            stderr_pos = 0;
        }
        waiting_notice_printed = false;

        if let Err(err) = follow_one_stream(
            &stdout_path,
            &mut stdout_pos,
            false,
            &redaction_patterns,
            output_fn,
        )
        .await
        {
            emit_anomaly_warning(
                &AnomalyRule::TransientReadError,
                &format!("{err}"),
                &mut last_warning_at,
            );
        }

        if let Err(err) = follow_one_stream(
            &stderr_path,
            &mut stderr_pos,
            true,
            &redaction_patterns,
            output_fn,
        )
        .await
        {
            emit_anomaly_warning(
                &AnomalyRule::TransientReadError,
                &format!("{err}"),
                &mut last_warning_at,
            );
        }

        if let Ok(Some(status)) = state.task_repo.load_task_status(task_id).await {
            if status == "completed" || status == "failed" {
                tokio::time::sleep(Duration::from_millis(200)).await;
                output_fn(format!("\n--- task {} ---\n", status), true)?;
                return Ok(());
            }
        }

        tokio::time::sleep(Duration::from_millis(FOLLOW_POLL_MS)).await;
    }
}

/// Tail the last N lines from a file.
pub(crate) fn tail_lines(path: &Path, limit: usize) -> Result<String> {
    if limit == 0 {
        return Ok(String::new());
    }
    const CHUNK_SIZE: usize = 8192;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to read log file: {}", path.display()))?;
    let mut pos = file
        .seek(std::io::SeekFrom::End(0))
        .with_context(|| format!("failed to seek log file: {}", path.display()))?;
    if pos == 0 {
        return Ok(String::new());
    }

    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut newline_count = 0usize;
    while pos > 0 && newline_count <= limit {
        let chunk_len = (pos as usize).min(CHUNK_SIZE);
        let start = pos - chunk_len as u64;
        file.seek(std::io::SeekFrom::Start(start))
            .with_context(|| format!("failed to seek log file: {}", path.display()))?;
        let mut buf = vec![0u8; chunk_len];
        file.read_exact(&mut buf)
            .with_context(|| format!("failed to read log file: {}", path.display()))?;
        newline_count += buf.iter().filter(|b| **b == b'\n').count();
        chunks.push(buf);
        pos = start;
    }

    let mut data = Vec::new();
    for chunk in chunks.iter().rev() {
        data.extend_from_slice(chunk);
    }

    if newline_count > limit {
        let mut to_skip = newline_count - limit;
        let mut idx = 0usize;
        while idx < data.len() && to_skip > 0 {
            if data[idx] == b'\n' {
                to_skip -= 1;
            }
            idx += 1;
        }
        data = data[idx..].to_vec();
    }

    Ok(String::from_utf8_lossy(&data).trim_end().to_string())
}

async fn follow_one_stream<F>(
    path: &str,
    pos: &mut u64,
    stderr: bool,
    redaction_patterns: &[String],
    output_fn: &mut F,
) -> Result<()>
where
    F: FnMut(String, bool) -> anyhow::Result<()>,
{
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncSeekExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open stream {path}"))?;
    let meta = file
        .metadata()
        .await
        .with_context(|| format!("stat stream {path}"))?;
    if meta.len() <= *pos {
        return Ok(());
    }

    file.seek(tokio::io::SeekFrom::Start(*pos))
        .await
        .with_context(|| format!("seek stream {path}"))?;
    let mut buf = vec![0u8; (meta.len() - *pos) as usize];
    let read = file
        .read(&mut buf)
        .await
        .with_context(|| format!("read stream {path}"))?;
    if read == 0 {
        return Ok(());
    }

    let text = redact_text(&String::from_utf8_lossy(&buf[..read]), redaction_patterns);
    output_fn(text, stderr)?;
    *pos += read as u64;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{first_item_id, seed_task, test_dir};
    use super::*;
    use agent_orchestrator::config_load::now_ts;
    use agent_orchestrator::task_repository::NewCommandRun;
    use agent_orchestrator::test_utils::TestState;
    use std::io::Write;

    #[test]
    fn tail_lines_zero_limit_returns_empty() {
        let dir = test_dir("zero");
        let path = dir.join("log.txt");
        std::fs::write(&path, "line1\nline2\n").expect("write test log");
        let result = tail_lines(&path, 0).expect("tail zero lines");
        assert_eq!(result, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_empty_file_returns_empty() {
        let dir = test_dir("empty");
        let path = dir.join("log.txt");
        std::fs::write(&path, "").expect("write empty log");
        let result = tail_lines(&path, 10).expect("tail empty log");
        assert_eq!(result, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_returns_last_n_lines() {
        let dir = test_dir("lastn");
        let path = dir.join("log.txt");
        // Use trailing newline so each "line" is terminated
        let content = (1..=20)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, &content).expect("write multi-line log");

        let result = tail_lines(&path, 3).expect("tail last n lines");
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line 18");
        assert_eq!(lines[2], "line 20");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_returns_all_when_limit_exceeds_file() {
        let dir = test_dir("exceed");
        let path = dir.join("log.txt");
        std::fs::write(&path, "line1\nline2\nline3").expect("write small log");

        let result = tail_lines(&path, 100).expect("tail whole file");
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_missing_file_returns_error() {
        let result = tail_lines(Path::new("/nonexistent/path"), 10);
        assert!(result.is_err());
    }

    #[test]
    fn tail_lines_large_file() {
        let dir = test_dir("large");
        let path = dir.join("big.txt");
        let mut f = std::fs::File::create(&path).expect("create large log");
        for i in 0..500 {
            writeln!(f, "line {:04}", i).expect("append line");
        }
        drop(f);

        let result = tail_lines(&path, 5).expect("tail large file");
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4], "line 0499");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_returns_log_chunks() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        // Create actual log files on disk
        let dir = test_dir("stream-logs");
        let stdout_path = dir.join("stream_stdout.log");
        let stderr_path = dir.join("stream_stderr.log");
        std::fs::write(&stdout_path, "line 1\nline 2\nline 3\n").expect("write stdout");
        std::fs::write(&stderr_path, "").expect("write stderr");

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-stream-1".to_string(),
                task_item_id: item_id,
                phase: "qa".to_string(),
                command: "echo stream".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: now_ts(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].run_id, "run-stream-1");
        assert_eq!(chunks[0].phase, "qa");
        assert!(chunks[0].content.contains("line 1"));
        assert!(chunks[0].content.contains("line 3"));
        // No stderr section since stderr is empty
        assert!(!chunks[0].content.contains("[stderr]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_works_when_active_config_is_not_runnable() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-invalid-active");
        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        std::fs::write(&stdout_path, "token=redacted\nvisible line\n").expect("write stdout");
        std::fs::write(&stderr_path, "").expect("write stderr");

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-invalid-active-1".to_string(),
                task_item_id: item_id,
                phase: "qa".to_string(),
                command: "echo stream".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: now_ts(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        agent_orchestrator::state::replace_active_config_status(
            &state,
            Some("active config is not runnable".to_string()),
            None,
        );

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("[REDACTED]"));
        assert!(chunks[0].content.contains("visible line"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_with_stderr() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-stderr");
        let stdout_path = dir.join("out.log");
        let stderr_path = dir.join("err.log");
        std::fs::write(&stdout_path, "stdout content\n").expect("write stdout");
        std::fs::write(&stderr_path, "warning: something\n").expect("write stderr");

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-stream-err".to_string(),
                task_item_id: item_id,
                phase: "implement".to_string(),
                command: "echo err".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 1,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: now_ts(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("stdout content"));
        assert!(chunks[0].content.contains("[stderr]"));
        assert!(chunks[0].content.contains("warning: something"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_with_timestamps() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-ts");
        let stdout_path = dir.join("ts_out.log");
        let stderr_path = dir.join("ts_err.log");
        std::fs::write(&stdout_path, "data\n").expect("write stdout");
        std::fs::write(&stderr_path, "").expect("write stderr");

        let ts = now_ts();
        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-ts-1".to_string(),
                task_item_id: item_id,
                phase: "qa".to_string(),
                command: "echo ts".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: ts.clone(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, true)
            .await
            .expect("stream with timestamps");
        assert_eq!(chunks.len(), 1);
        // When show_timestamps is true, header includes the timestamp
        assert!(
            chunks[0].content.contains(&ts),
            "content should include timestamp"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_tail_count_limits_output() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-tail");

        // Insert 3 command runs with distinct log files
        for i in 0..3 {
            let stdout_path = dir.join(format!("tail_out_{}.log", i));
            let stderr_path = dir.join(format!("tail_err_{}.log", i));
            std::fs::write(&stdout_path, format!("run {} output\n", i)).expect("write tail stdout");
            std::fs::write(&stderr_path, "").expect("write tail stderr");

            state
                .task_repo
                .insert_command_run(NewCommandRun {
                    id: format!("run-tail-{}", i),
                    task_item_id: item_id.clone(),
                    phase: "qa".to_string(),
                    command: format!("echo {}", i),
                    command_template: None,
                    cwd: "/tmp".to_string(),
                    workspace_id: "default".to_string(),
                    agent_id: "echo".to_string(),
                    exit_code: 0,
                    stdout_path: stdout_path.to_string_lossy().to_string(),
                    stderr_path: stderr_path.to_string_lossy().to_string(),
                    started_at: format!("2026-01-01T00:00:0{}Z", i),
                    ended_at: now_ts(),
                    interrupted: 0,
                    output_json: "{}".to_string(),
                    artifacts_json: "[]".to_string(),
                    confidence: None,
                    quality_score: None,
                    validation_status: "unknown".to_string(),
                    session_id: None,
                    machine_output_source: "stdout".to_string(),
                    output_json_path: None,
                    command_rule_index: None,
                })
                .await
                .expect("insert command run");
        }

        // Request only 2 tail entries
        let chunks = stream_task_logs_impl(&state, &task_id, 2, false)
            .await
            .expect("stream with tail limit");
        assert_eq!(chunks.len(), 2, "should be limited to 2 chunks");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_no_runs_returns_empty() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream empty logs");
        assert!(chunks.is_empty());
    }

    #[tokio::test]
    async fn stream_task_logs_impl_returns_placeholder_when_logs_missing() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-missing-logs".to_string(),
                task_item_id: item_id,
                phase: "qa".to_string(),
                command: "echo missing".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: "/nonexistent/stdout.log".to_string(),
                stderr_path: "/nonexistent/stderr.log".to_string(),
                started_at: now_ts(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains(LOG_UNAVAILABLE_MARKER));
        assert!(
            chunks[0].content.contains("/nonexistent/stdout.log"),
            "should include stdout path in unavailable marker"
        );
    }

    #[tokio::test]
    async fn stream_task_logs_impl_redacts_secret_store_values() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        // Inject a sensitive store into the active config
        agent_orchestrator::state::update_config_runtime(&state, |current| {
            let mut next = current.clone();
            std::sync::Arc::make_mut(&mut next.active_config)
                .config
                .project_mut(None)
                .expect("default project")
                .secret_stores
                .insert(
                    "secrets".to_string(),
                    agent_orchestrator::config::SecretStoreConfig {
                        data: [("API_KEY".to_string(), "super-secret-value".to_string())].into(),
                    },
                );
            (next, ())
        });

        let dir = test_dir("stream-secret-store");
        let stdout_path = dir.join("secret_out.log");
        let stderr_path = dir.join("secret_err.log");
        std::fs::write(&stdout_path, "key=super-secret-value output\n").expect("write stdout");
        std::fs::write(&stderr_path, "").expect("write stderr");

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-secret-store-1".to_string(),
                task_item_id: item_id,
                phase: "qa".to_string(),
                command: "echo secret".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: now_ts(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].content.contains("[REDACTED]"),
            "secret store value should be redacted"
        );
        assert!(
            !chunks[0].content.contains("super-secret-value"),
            "raw secret value must not appear in output"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_redacts_non_default_project_secret_store_values() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        // Point the task at a non-default project via direct SQL update
        {
            let conn = agent_orchestrator::db::open_conn(&state.db_path).expect("open db");
            conn.execute(
                "UPDATE tasks SET project_id = ?1 WHERE id = ?2",
                rusqlite::params!["custom-project", &task_id],
            )
            .expect("update project_id");
        }

        // Inject a secret store into the non-default project
        agent_orchestrator::state::update_config_runtime(&state, |current| {
            let mut next = current.clone();
            std::sync::Arc::make_mut(&mut next.active_config)
                .config
                .ensure_project(Some("custom-project"))
                .secret_stores
                .insert(
                    "vault".to_string(),
                    agent_orchestrator::config::SecretStoreConfig {
                        data: [("DB_PASSWORD".to_string(), "non-default-secret-42".to_string())]
                            .into(),
                    },
                );
            (next, ())
        });

        let dir = test_dir("stream-nondefault-secret");
        let stdout_path = dir.join("nd_out.log");
        let stderr_path = dir.join("nd_err.log");
        std::fs::write(&stdout_path, "password=non-default-secret-42 output\n")
            .expect("write stdout");
        std::fs::write(&stderr_path, "").expect("write stderr");

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-nondefault-secret-1".to_string(),
                task_item_id: item_id,
                phase: "qa".to_string(),
                command: "echo secret".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: now_ts(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].content.contains("[REDACTED]"),
            "non-default project secret store value should be redacted"
        );
        assert!(
            !chunks[0].content.contains("non-default-secret-42"),
            "raw non-default project secret value must not appear in output"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stream_task_logs_impl_returns_partial_results_when_one_run_is_unavailable() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);
        let dir = test_dir("partial-logs");

        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        std::fs::write(&stdout_path, "available output\n").expect("write available stdout");
        std::fs::write(&stderr_path, "").expect("write available stderr");

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-partial-good".to_string(),
                task_item_id: item_id.clone(),
                phase: "qa".to_string(),
                command: "echo ok".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: "2026-01-01T00:00:00Z".to_string(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert good run");
        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-partial-missing".to_string(),
                task_item_id: item_id,
                phase: "implement".to_string(),
                command: "echo missing".to_string(),
                command_template: None,
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: "/nonexistent/stdout.log".to_string(),
                stderr_path: "/nonexistent/stderr.log".to_string(),
                started_at: "2026-01-01T00:00:01Z".to_string(),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert missing run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 2);
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.content.contains("available output"))
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.content.contains(LOG_UNAVAILABLE_MARKER))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn follow_one_stream_uses_callback_for_stdout() {
        let dir = test_dir("follow-cb-stdout");
        let path = dir.join("stdout.log");
        std::fs::write(&path, "hello world\nsecond line\n").expect("write test log");

        let mut captured: Vec<(String, bool)> = Vec::new();
        let mut pos: u64 = 0;
        follow_one_stream(
            path.to_str().unwrap(),
            &mut pos,
            false,
            &[],
            &mut |text, is_stderr| {
                captured.push((text, is_stderr));
                Ok(())
            },
        )
        .await
        .expect("follow_one_stream should succeed");

        assert_eq!(captured.len(), 1);
        assert!(captured[0].0.contains("hello world"));
        assert!(captured[0].0.contains("second line"));
        assert!(!captured[0].1, "is_stderr should be false for stdout");
        assert_eq!(pos, 24); // "hello world\nsecond line\n" = 24 bytes
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn follow_one_stream_uses_callback_for_stderr() {
        let dir = test_dir("follow-cb-stderr");
        let path = dir.join("stderr.log");
        std::fs::write(&path, "error: something broke\n").expect("write test log");

        let mut captured: Vec<(String, bool)> = Vec::new();
        let mut pos: u64 = 0;
        follow_one_stream(
            path.to_str().unwrap(),
            &mut pos,
            true,
            &[],
            &mut |text, is_stderr| {
                captured.push((text, is_stderr));
                Ok(())
            },
        )
        .await
        .expect("follow_one_stream should succeed");

        assert_eq!(captured.len(), 1);
        assert!(captured[0].0.contains("error: something broke"));
        assert!(captured[0].1, "is_stderr should be true for stderr");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn follow_one_stream_callback_incremental_read() {
        let dir = test_dir("follow-cb-incr");
        let path = dir.join("incr.log");
        std::fs::write(&path, "first chunk\n").expect("write initial");

        let mut captured: Vec<String> = Vec::new();
        let mut pos: u64 = 0;

        // First read
        follow_one_stream(
            path.to_str().unwrap(),
            &mut pos,
            false,
            &[],
            &mut |text, _| {
                captured.push(text);
                Ok(())
            },
        )
        .await
        .expect("first read");
        assert_eq!(captured.len(), 1);
        assert!(captured[0].contains("first chunk"));

        // Append more data
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open for append");
        writeln!(f, "second chunk").expect("append");
        drop(f);

        // Second read should only get new data
        follow_one_stream(
            path.to_str().unwrap(),
            &mut pos,
            false,
            &[],
            &mut |text, _| {
                captured.push(text);
                Ok(())
            },
        )
        .await
        .expect("second read");
        assert_eq!(captured.len(), 2);
        assert!(captured[1].contains("second chunk"));
        assert!(!captured[1].contains("first chunk"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
