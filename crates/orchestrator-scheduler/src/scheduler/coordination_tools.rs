//! Authenticated run-scoped coordination tools hosted inside the daemon.

use agent_orchestrator::config::RunnerConfig;
use agent_orchestrator::driver::McpCallbackConfig;
use agent_orchestrator::events::insert_event;
use agent_orchestrator::runner::{
    ResolvedExecutionProfile, kill_child_process_group, spawn_with_runner_and_capture,
};
use agent_orchestrator::state::InnerState;
use agent_orchestrator::ticket::{
    create_ticket_for_qa_failure, scan_active_tickets_for_task_items,
};
use anyhow::{Context, Result, bail};
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

use super::item_generate::create_dynamic_task_items_async;
use super::runtime::load_task_runtime_context;

const MAX_REQUEST_BYTES: usize = 256 * 1024;
const MAX_GENERATED_ITEMS: usize = 100;
const TEST_TIMEOUT: Duration = Duration::from_secs(1800);

/// Inputs used to create one private tool host.
pub struct CoordinationHostRequest<'a> {
    /// Shared daemon state.
    pub state: Arc<InnerState>,
    /// Current task identifier.
    pub task_id: &'a str,
    /// Current task-item identifier.
    pub item_id: &'a str,
    /// Current command-run identifier.
    pub run_id: &'a str,
    /// Governed workspace root.
    pub workspace_root: &'a Path,
    /// Runner policy inherited from the agent command.
    pub runner: &'a RunnerConfig,
    /// Sandbox/profile inherited from the agent command.
    pub execution_profile: &'a ResolvedExecutionProfile,
    /// Agent environment passed through the common spawn path.
    pub extra_env: &'a HashMap<String, String>,
    /// Redaction patterns applied to tool output artifacts.
    pub redaction_patterns: &'a [String],
    /// Per-run artifact directory.
    pub artifacts_dir: &'a Path,
    /// Agent-owned provider whitelist.
    pub allowed_tools: &'a [String],
}

/// Lifetime handle for one loopback tool host.
pub struct CoordinationToolHost {
    callback: McpCallbackConfig,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl CoordinationToolHost {
    /// Returns the private callback descriptor passed into the provider config.
    pub fn callback(&self) -> &McpCallbackConfig {
        &self.callback
    }
}

impl Drop for CoordinationToolHost {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.abort();
    }
}

#[derive(Clone)]
struct ToolHostState {
    inner: Arc<InnerState>,
    task_id: String,
    item_id: String,
    run_id: String,
    workspace_root: PathBuf,
    runner: RunnerConfig,
    execution_profile: ResolvedExecutionProfile,
    extra_env: HashMap<String, String>,
    redaction_patterns: Vec<String>,
    artifacts_dir: PathBuf,
    allowed_tools: HashSet<String>,
    token: String,
    last_test: Arc<Mutex<Option<TestEvidence>>>,
}

#[derive(Clone)]
struct TestEvidence {
    exit_code: i64,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

/// Starts an authenticated loopback tool host for one command run.
pub async fn start_tool_host(request: CoordinationHostRequest<'_>) -> Result<CoordinationToolHost> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding run-scoped coordination tool host")?;
    let address = listener.local_addr()?;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let callback = McpCallbackConfig::new(format!("http://{address}/mcp"), token.clone())?;
    let state = ToolHostState {
        inner: request.state,
        task_id: request.task_id.to_string(),
        item_id: request.item_id.to_string(),
        run_id: request.run_id.to_string(),
        workspace_root: request.workspace_root.to_path_buf(),
        runner: request.runner.clone(),
        execution_profile: request.execution_profile.clone(),
        extra_env: request.extra_env.clone(),
        redaction_patterns: request.redaction_patterns.to_vec(),
        artifacts_dir: request.artifacts_dir.to_path_buf(),
        allowed_tools: normalize_allowed_tools(request.allowed_tools),
        token,
        last_test: Arc::new(Mutex::new(None)),
    };
    let router = Router::new()
        .route("/mcp", post(handle_mcp))
        .with_state(state);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });
    Ok(CoordinationToolHost {
        callback,
        shutdown: Some(shutdown_tx),
        task,
    })
}

fn normalize_allowed_tools(configured: &[String]) -> HashSet<String> {
    const ALL: &[&str] = &[
        "run_tests",
        "mark_item",
        "mark_done",
        "create_ticket",
        "scan_tickets",
        "generate_items",
    ];
    if configured.is_empty() || configured.iter().any(|tool| tool == "mcp__orch") {
        return ALL.iter().map(|tool| (*tool).to_string()).collect();
    }
    configured
        .iter()
        .filter_map(|tool| {
            let bare = tool.strip_prefix("mcp__orch__").unwrap_or(tool);
            ALL.contains(&bare).then(|| bare.to_string())
        })
        .collect()
}

async fn handle_mcp(
    State(state): State<ToolHostState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BYTES {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let expected = format!("Bearer {}", state.token);
    if headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some(expected.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let request: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    Json(dispatch_rpc(&state, &request).await).into_response()
}

async fn dispatch_rpc(state: &ToolHostState, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match method {
        "initialize" => rpc_result(
            id,
            json!({
                "protocolVersion": request.pointer("/params/protocolVersion")
                    .cloned().unwrap_or_else(|| json!("2024-11-05")),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "orchestrator-coordination", "version": env!("CARGO_PKG_VERSION")},
            }),
        ),
        "notifications/initialized" => rpc_result(id, Value::Null),
        "tools/list" => rpc_result(id, json!({"tools": tool_schemas(&state.allowed_tools)})),
        "tools/call" => dispatch_tool_call(state, request, id).await,
        _ => rpc_error(id, -32601, "method not found"),
    }
}

async fn dispatch_tool_call(state: &ToolHostState, request: &Value, id: Value) -> Value {
    let name = request.pointer("/params/name").and_then(Value::as_str);
    let Some(name) = name else {
        return rpc_error(id, -32602, "tool name is required");
    };
    if !state.allowed_tools.contains(name) {
        return rpc_error(id, -32604, "tool is not allowed for this run");
    }
    let arguments = request
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let call_id = id.to_string();
    let _ = insert_event(
        &state.inner,
        &state.task_id,
        Some(&state.item_id),
        "coordination_tool_started",
        json!({"run_id": state.run_id, "call_id": call_id, "tool": name}),
    )
    .await;
    let started = Instant::now();
    let outcome = execute_tool(state, name, arguments).await;
    let (result, is_error) = match outcome {
        Ok(result) => (result, false),
        Err(error) => (json!({"error": error.to_string()}), true),
    };
    let _ = insert_event(
        &state.inner,
        &state.task_id,
        Some(&state.item_id),
        "coordination_tool_completed",
        json!({
            "run_id": state.run_id,
            "call_id": call_id,
            "tool": name,
            "is_error": is_error,
            "duration_ms": started.elapsed().as_millis() as u64,
        }),
    )
    .await;
    rpc_result(
        id,
        json!({
            "content": [{"type": "text", "text": result.to_string()}],
            "structuredContent": result,
            "isError": is_error,
        }),
    )
}

async fn execute_tool(state: &ToolHostState, name: &str, arguments: Value) -> Result<Value> {
    match name {
        "run_tests" => run_tests(state, arguments).await,
        "mark_item" => mark_item(state, arguments).await,
        "mark_done" => mark_done(state, arguments).await,
        "create_ticket" => create_ticket(state).await,
        "scan_tickets" => scan_tickets(state).await,
        "generate_items" => generate_items(state, arguments).await,
        _ => bail!("unknown coordination tool"),
    }
}

async fn run_tests(state: &ToolHostState, arguments: Value) -> Result<Value> {
    let target = arguments
        .get("target")
        .and_then(Value::as_str)
        .context("target is required")?;
    let command = match target {
        "workspace" => "cargo test --workspace",
        "core" => "cargo test -p agent-orchestrator",
        "runner" => "cargo test -p orchestrator-runner",
        "scheduler" => "cargo test -p orchestrator-scheduler",
        _ => bail!("unsupported test target '{target}'"),
    };
    let directory = state.artifacts_dir.join("coordination-tests");
    std::fs::create_dir_all(&directory)?;
    let stem = Uuid::new_v4().simple().to_string();
    let stdout_path = directory.join(format!("{stem}.stdout.log"));
    let stderr_path = directory.join(format!("{stem}.stderr.log"));
    let stdout = File::create(&stdout_path)?;
    let stderr = File::create(&stderr_path)?;
    let captured = spawn_with_runner_and_capture(
        &state.runner,
        command,
        &state.workspace_root,
        stdout,
        stderr,
        state.redaction_patterns.clone(),
        &state.extra_env,
        false,
        &state.execution_profile,
    )?;
    let mut child = captured.child;
    let status = match tokio::time::timeout(TEST_TIMEOUT, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            kill_child_process_group(&mut child).await;
            bail!(
                "test execution timed out after {} seconds",
                TEST_TIMEOUT.as_secs()
            );
        }
    };
    captured.output_capture.wait().await?;
    let exit_code = status.code().unwrap_or(-1) as i64;
    *state.last_test.lock().await = Some(TestEvidence {
        exit_code,
        stdout_path: stdout_path.clone(),
        stderr_path: stderr_path.clone(),
    });
    let stdout_text = std::fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr_text = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    let (passed, failed) = parse_test_counts(&stdout_text);
    Ok(json!({
        "target": target,
        "success": status.success(),
        "exit_code": exit_code,
        "passed": passed,
        "failed": failed,
        "stdout_tail": tail(&stdout_text, 40),
        "stderr_tail": tail(&stderr_text, 20),
    }))
}

async fn mark_item(state: &ToolHostState, arguments: Value) -> Result<Value> {
    const ALLOWED: &[&str] = &[
        "qa_passed",
        "qa_failed",
        "fixed",
        "verified",
        "skipped",
        "unresolved",
        "eliminated",
    ];
    let status = arguments
        .get("status")
        .and_then(Value::as_str)
        .context("status is required")?;
    if !ALLOWED.contains(&status) {
        bail!("unsupported item status '{status}'");
    }
    ensure_current_item(state).await?;
    Ok(json!({
        "accepted": true,
        "task_id": state.task_id,
        "item_id": state.item_id,
        "status": status,
        "summary": arguments.get("summary").and_then(Value::as_str).unwrap_or_default(),
    }))
}

async fn mark_done(state: &ToolHostState, arguments: Value) -> Result<Value> {
    ensure_current_item(state).await?;
    Ok(json!({
        "accepted": true,
        "task_id": state.task_id,
        "item_id": state.item_id,
        "status": "verified",
        "summary": arguments.get("summary").and_then(Value::as_str).unwrap_or_default(),
    }))
}

async fn ensure_current_item(state: &ToolHostState) -> Result<()> {
    let items = state
        .inner
        .task_repo
        .list_task_items_for_cycle(&state.task_id)
        .await?;
    if items.iter().any(|item| item.id == state.item_id) {
        Ok(())
    } else {
        bail!("current task item does not belong to the authenticated run")
    }
}

async fn create_ticket(state: &ToolHostState) -> Result<Value> {
    let evidence = state
        .last_test
        .lock()
        .await
        .clone()
        .context("run_tests must be called before create_ticket")?;
    if evidence.exit_code == 0 {
        bail!("create_ticket requires failing test evidence");
    }
    let runtime = load_task_runtime_context(&state.inner, &state.task_id).await?;
    let items = state
        .inner
        .task_repo
        .list_task_items_for_cycle(&state.task_id)
        .await?;
    let item = items
        .iter()
        .find(|item| item.id == state.item_id)
        .context("current task item is missing")?;
    let task_name = state
        .inner
        .task_repo
        .load_task_name(&state.task_id)
        .await?
        .unwrap_or_else(|| state.task_id.clone());
    let path = create_ticket_for_qa_failure(
        &runtime.workspace_root,
        &runtime.ticket_dir,
        &task_name,
        &item.qa_file_path,
        evidence.exit_code,
        &evidence.stdout_path.to_string_lossy(),
        &evidence.stderr_path.to_string_lossy(),
    )?;
    Ok(json!({"created": path.is_some(), "path": path}))
}

async fn scan_tickets(state: &ToolHostState) -> Result<Value> {
    let runtime = load_task_runtime_context(&state.inner, &state.task_id).await?;
    let items = state
        .inner
        .task_repo
        .list_task_items_for_cycle(&state.task_id)
        .await?;
    let paths: Vec<String> = items.iter().map(|item| item.qa_file_path.clone()).collect();
    let grouped = scan_active_tickets_for_task_items(&runtime, &paths)?;
    let current_path = items
        .iter()
        .find(|item| item.id == state.item_id)
        .map(|item| item.qa_file_path.as_str())
        .context("current task item is missing")?;
    let tickets = grouped.get(current_path).cloned().unwrap_or_default();
    Ok(json!({"count": tickets.len(), "tickets": tickets}))
}

#[derive(Deserialize)]
struct GenerateItemsInput {
    items: Vec<GeneratedItemInput>,
    #[serde(default)]
    replace: bool,
}

#[derive(Deserialize)]
struct GeneratedItemInput {
    id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    vars: HashMap<String, String>,
}

async fn generate_items(state: &ToolHostState, arguments: Value) -> Result<Value> {
    let input: GenerateItemsInput = serde_json::from_value(arguments)?;
    if input.items.is_empty() || input.items.len() > MAX_GENERATED_ITEMS {
        bail!("generate_items requires 1..={MAX_GENERATED_ITEMS} items");
    }
    let mut items = Vec::with_capacity(input.items.len());
    let mut seen = HashSet::new();
    for item in input.items {
        validate_item_id(&item.id)?;
        if !seen.insert(item.id.clone()) {
            bail!("duplicate generated item id '{}'", item.id);
        }
        items.push(agent_orchestrator::config::NewDynamicItem {
            item_id: item.id,
            label: item.label,
            vars: item.vars,
        });
    }
    let created =
        create_dynamic_task_items_async(&state.inner, &state.task_id, &items, input.replace)
            .await?;
    insert_event(
        &state.inner,
        &state.task_id,
        Some(&state.item_id),
        "items_generated",
        json!({"count": created, "replace": input.replace, "source": "coordination_tool"}),
    )
    .await?;
    Ok(json!({"created": created, "replace": input.replace}))
}

fn validate_item_id(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || value.len() > 512
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        bail!("generated item id must be a bounded workspace-relative value");
    }
    Ok(())
}

fn parse_test_counts(output: &str) -> (u64, u64) {
    let mut passed = 0;
    let mut failed = 0;
    for line in output.lines().filter(|line| line.contains("test result:")) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        for pair in tokens.windows(2) {
            match pair {
                [count, "passed;"] => passed += count.parse::<u64>().unwrap_or(0),
                [count, "failed;"] => failed += count.parse::<u64>().unwrap_or(0),
                _ => {}
            }
        }
    }
    (passed, failed)
}

fn tail(value: &str, lines: usize) -> String {
    let selected: Vec<&str> = value.lines().rev().take(lines).collect();
    selected.into_iter().rev().collect::<Vec<_>>().join("\n")
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_schemas(allowed: &HashSet<String>) -> Vec<Value> {
    let schemas = [
        json!({
            "name": "run_tests",
            "description": "Run an allowlisted project test target through the governed runner.",
            "inputSchema": {"type": "object", "properties": {"target": {"type": "string", "enum": ["workspace", "core", "runner", "scheduler"]}}, "required": ["target"]}
        }),
        json!({
            "name": "mark_item",
            "description": "Request a validated terminal status for the current task item.",
            "inputSchema": {"type": "object", "properties": {"status": {"type": "string", "enum": ["qa_passed", "qa_failed", "fixed", "verified", "skipped", "unresolved", "eliminated"]}, "summary": {"type": "string"}}, "required": ["status", "summary"]}
        }),
        json!({
            "name": "mark_done",
            "description": "Compatibility alias that marks the current item verified.",
            "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}}, "required": ["summary"]}
        }),
        json!({
            "name": "create_ticket",
            "description": "Create a deduplicated QA ticket from the latest failing run_tests evidence.",
            "inputSchema": {"type": "object", "properties": {}}
        }),
        json!({
            "name": "scan_tickets",
            "description": "List active tickets associated with the current task item.",
            "inputSchema": {"type": "object", "properties": {}}
        }),
        json!({
            "name": "generate_items",
            "description": "Create bounded dynamic task items for the authenticated task.",
            "inputSchema": {"type": "object", "properties": {"items": {"type": "array", "maxItems": MAX_GENERATED_ITEMS, "items": {"type": "object", "properties": {"id": {"type": "string"}, "label": {"type": "string"}, "vars": {"type": "object", "additionalProperties": {"type": "string"}}}, "required": ["id"]}}, "replace": {"type": "boolean"}}, "required": ["items"]}
        }),
    ];
    schemas
        .into_iter()
        .filter(|schema| {
            schema
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| allowed.contains(name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_tool_normalization_is_fail_closed() {
        let allowed = normalize_allowed_tools(&[
            "mcp__orch__run_tests".to_string(),
            "mcp__other__escape".to_string(),
        ]);
        assert_eq!(allowed, HashSet::from(["run_tests".to_string()]));
    }

    #[test]
    fn test_count_parser_sums_cargo_suites() {
        let output = "test result: ok. 3 passed; 0 failed;\n\
                      test result: FAILED. 2 passed; 1 failed;";
        assert_eq!(parse_test_counts(output), (5, 1));
    }

    #[test]
    fn generated_item_ids_cannot_escape_workspace() {
        assert!(validate_item_id("docs/qa/test.md").is_ok());
        assert!(validate_item_id("../outside").is_err());
        assert!(validate_item_id("/absolute").is_err());
    }

    #[test]
    fn schemas_only_advertise_allowed_tools() {
        let schemas = tool_schemas(&HashSet::from(["mark_item".to_string()]));
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["name"], "mark_item");
    }
}
