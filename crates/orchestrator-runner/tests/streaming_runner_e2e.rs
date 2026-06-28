//! End-to-end test for the streaming agent runner.
//!
//! Proves the first cut: a single step driven through `RunnerExecutorKind::Streaming`
//! launches the real `claude` CLI in `stream-json` mode, the agent calls the
//! orchestrator-owned `run_tests` MCP tool (served by the `orch-mcp-tools`
//! binary), and the orchestrator-computed result flows back into the agent's
//! reasoning — all observed through the Rust runner.
//!
//! Ignored by default: it requires the `claude` CLI on PATH, valid auth, network
//! access, and consumes tokens. Run explicitly:
//!
//! ```sh
//! cargo test -p orchestrator-runner --test streaming_runner_e2e -- --ignored --nocapture
//! ```

use orchestrator_config::config::{RunnerConfig, RunnerExecutorKind, RunnerPolicy};
use orchestrator_runner::runner::{ResolvedExecutionProfile, spawn_with_runner_and_capture};
use std::collections::HashMap;
use std::fs::File;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires real claude CLI, auth, network; costs tokens"]
async fn streaming_runner_drives_single_step_with_mcp_tool() {
    // Point the streaming runner at the freshly-built MCP tools binary.
    // SAFETY: single-threaded test setup before any concurrent env access.
    unsafe {
        std::env::set_var("ORCH_MCP_TOOLS_BIN", env!("CARGO_BIN_EXE_orch-mcp-tools"));
    }

    let temp = tempfile::tempdir().expect("create tempdir");
    let stdout_path = temp.path().join("stdout.log");
    let stderr_path = temp.path().join("stderr.log");
    let stdout = File::create(&stdout_path).expect("create stdout file");
    let stderr = File::create(&stderr_path).expect("create stderr file");

    // Unsafe policy: pass the full environment through so `claude` can
    // authenticate (keychain/OAuth). Streaming executor selects our runner.
    let runner = RunnerConfig {
        executor: RunnerExecutorKind::Streaming,
        policy: RunnerPolicy::Unsafe,
        ..RunnerConfig::default()
    };

    // The runner appends the stream-json + MCP flags; the prompt is delivered
    // via the existing `-p` argument (single-turn, no bidirectional stdin).
    let command = "claude -p \"Use the run_tests tool with target core to run the \
        project tests, then reply in one sentence stating how many tests failed \
        and the exact name of the failing test.\" --model haiku";

    let captured = spawn_with_runner_and_capture(
        &runner,
        command,
        temp.path(),
        stdout,
        stderr,
        Vec::new(), // no redaction so we can assert on the raw event stream
        &HashMap::new(),
        false,
        &ResolvedExecutionProfile::host(),
    )
    .expect("spawn streaming runner");

    let mut child = captured.child;
    let output_capture = captured.output_capture;

    let status = tokio::time::timeout(Duration::from_secs(180), child.wait())
        .await
        .expect("claude run timed out")
        .expect("wait for claude child");
    output_capture.wait().await.expect("flush output capture");

    let stdout_output = std::fs::read_to_string(&stdout_path).expect("read stdout");
    let stderr_output = std::fs::read_to_string(&stderr_path).expect("read stderr");
    eprintln!("---- claude stream-json stdout ----\n{stdout_output}");
    eprintln!("---- claude stderr ----\n{stderr_output}");

    assert!(
        status.success(),
        "claude should exit 0; stderr: {stderr_output}"
    );

    // Proof 1: the agent invoked our orchestrator-owned MCP tool.
    assert!(
        stdout_output.contains("mcp__orch__run_tests"),
        "expected a tool_use for the orchestrator MCP tool in the event stream"
    );
    // Proof 2: the orchestrator-computed result (its fingerprint failing-test
    // name) reached the agent — the structured loop closed.
    assert!(
        stdout_output.contains("core::selection::picks_healthy_agent"),
        "expected the orchestrator-owned tool result to flow back to the agent"
    );
}
