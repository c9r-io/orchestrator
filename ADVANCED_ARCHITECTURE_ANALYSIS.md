# AI Native SDLC — Advanced Architecture Analysis Report

## Executive Summary

This report provides a comprehensive analysis of advanced subsystems in the AI Native SDLC platform, covering DAG execution, multi-agent collaboration, session management, state persistence, health monitoring, anomaly detection, output validation, scheduling, custom resource definitions (CRD), store abstraction, and task repository patterns.

---

## 1. DAG EXECUTION ENGINE

**File:** `core/src/dynamic_orchestration/dag.rs` (962 lines)

### Overview
The DAG engine implements a **directed acyclic graph** for workflow execution with cycle detection, topological sorting, and conditional branching.

### Data Structures

**WorkflowNode:**
```rust
pub struct WorkflowNode {
    pub id: String,                              // Unique identifier
    pub step_type: String,                       // Task type (qa, fix, etc.)
    pub agent_id: Option<String>,               // Optional agent assignment
    pub template: Option<String>,               // Execution template
    pub prehook: Option<PrehookConfig>,         // Dynamic condition hook
    pub is_guard: bool,                         // Guard node flag
    pub repeatable: bool,                       // Can be re-executed
}
```

**WorkflowEdge:**
```rust
pub struct WorkflowEdge {
    pub from: String,                           // Source node
    pub to: String,                             // Target node
    pub condition: Option<String>,              // CEL condition for edge traversal
}
```

**DynamicExecutionPlan:**
- HashMap of nodes (id → WorkflowNode)
- Vec of edges
- Optional entry point node ID
- Manages graph structure and transitions

**DagExecutionState:**
```rust
pub struct DagExecutionState {
    pub current_node: Option<String>,
    pub completed_nodes: HashSet<String>,      // Completed execution nodes
    pub skipped_nodes: HashSet<String>,        // Nodes that were skipped
    pub context: HashMap<String, serde_json::Value>, // Accumulated context
    pub branch_history: Vec<BranchRecord>,     // Debug trace of transitions
}
```

### Algorithms

#### 1. **Cycle Detection** (Lines 148-186)
- **Algorithm:** Depth-First Search (DFS) with recursion stack
- **Time Complexity:** O(V + E) where V = nodes, E = edges
- **Implementation:** 
  - Maintains `visited` set for traversed nodes
  - Maintains `rec_stack` for nodes in current recursion path
  - Back edges detected when target is in `rec_stack`
  - Returns true if any cycle found

```rust
fn dfs(node, plan, visited, rec_stack) -> bool {
    visited.insert(node);
    rec_stack.insert(node);
    
    for edge in plan.get_outgoing_edges(&node) {
        if !visited.contains(&target) {
            if dfs(target, ...) { return true; }
        } else if rec_stack.contains(&target) {
            return true;  // cycle found
        }
    }
    rec_stack.remove(&node);
    false
}
```

#### 2. **Topological Sort** (Lines 190-232)
- **Algorithm:** Kahn's Algorithm (queue-based in-degree reduction)
- **Time Complexity:** O(V + E)
- **Process:**
  1. Calculate in-degree (incoming edges) for all nodes
  2. Initialize queue with all nodes having in-degree 0
  3. Process queue, decrementing in-degrees of successors
  4. Add nodes reaching in-degree 0 to queue
  5. Verify all nodes processed (detects cycles)

```rust
pub fn topological_sort(&self) -> Result<Vec<String>> {
    if self.has_cycles() {
        return Err(anyhow!("graph has cycles"));
    }
    
    let mut in_degree: HashMap<&str, usize> = 
        self.nodes.keys().map(|k| (k.as_str(), 0)).collect();
    
    for edge in &self.edges {
        in_degree[edge.to] += 1;
    }
    
    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(k, _)| *k)
        .collect();
    
    let mut result = Vec::new();
    while let Some(node) = queue.pop() {
        result.push(node.to_string());
        for edge in self.get_outgoing_edges(node) {
            in_degree[edge.to] -= 1;
            if in_degree[edge.to] == 0 {
                queue.push(&edge.to);
            }
        }
    }
    
    if result.len() != self.nodes.len() {
        return Err(anyhow!("cycles detected"));
    }
    Ok(result)
}
```

### Graph Traversal Operations

1. **get_entry_nodes()** - All nodes with no incoming edges
2. **get_exit_nodes()** - All nodes with no outgoing edges
3. **get_outgoing_edges(node_id)** - Transitions from a node
4. **get_incoming_edges(node_id)** - Predecessors of a node
5. **find_next_nodes(current, context)** - Evaluates CEL conditions on outgoing edges to determine reachable next nodes

### Edge Case Handling

- **Self-loops:** Detected as cycles ✓
- **Disconnected components:** Topological sort tolerates them
- **Empty graph:** Returns empty Vec
- **Single node:** Returns [node_id]
- **Conditional branches:** CEL expressions evaluated at runtime

### Validation Tests
- `test_dag_topological_sort`: Linear chain A→B ordering
- `test_dag_cycle_detection`: Cycle detection with bidirectional edges
- `test_dag_find_next_nodes`: Conditional edge selection

---

## 2. COLLABORATION & MULTI-AGENT SYSTEM

### Architecture Overview

The collaboration system (`core/src/collab/`) enables structured agent-to-agent communication, artifact sharing, and context propagation across workflow phases.

#### 2.1 **Artifact System** (`collab/artifact.rs` - 661 lines)

**Artifact Types:**
```rust
pub enum ArtifactKind {
    Ticket { severity: Severity, category: String },
    CodeChange { files: Vec<String> },
    TestResult { passed: u32, failed: u32 },
    Analysis { findings: Vec<Finding> },
    Decision { choice: String, rationale: String },
    Data { schema: String },
    Custom { name: String },
}
```

**Artifact Structure:**
```rust
pub struct Artifact {
    pub id: Uuid,                              // Unique ID
    pub kind: ArtifactKind,
    pub path: Option<String>,                 // File path if applicable
    pub content: Option<serde_json::Value>,  // Structured content
    pub checksum: String,                    // Integrity check
    pub created_at: DateTime<Utc>,
}
```

**ArtifactRegistry:**
- HashMap<phase_name, Vec<Artifact>>
- **Methods:**
  - `register(phase, artifact)` - Add to phase
  - `get_by_phase(phase)` - Retrieve all artifacts from phase
  - `get_by_kind(kind)` - Filter by type across phases
  - `get_latest(phase)` - Most recent artifact in phase
  - `count()` - Total artifacts
  - `all()` - Return complete map

**SharedState (Key-Value Store):**
```rust
pub struct SharedState {
    data: HashMap<String, serde_json::Value>,
}
```
- `set(key, value)` - Store value
- `get(key)` - Retrieve value
- `remove(key)` - Delete value
- `render_template(template)` - Substitute {key} placeholders

**Artifact Parsing:** (`parse_artifacts_from_output`)
- Attempts JSON array parsing (for multiple artifacts)
- Falls back to single JSON object parsing
- Scans for "artifacts" field within JSON
- Line-by-line ticket extraction for plaintext
- Supports nested artifact discovery

#### 2.2 **Agent Context** (`collab/context.rs` - 291 lines)

**Full Context (Runtime):**
```rust
pub struct AgentContext {
    pub task_id: String,
    pub item_id: String,
    pub cycle: u32,
    pub phase: String,
    pub workspace_root: PathBuf,
    pub workspace_id: String,
    pub execution_history: Vec<PhaseRecord>,
    pub upstream_outputs: Vec<AgentOutput>,
    pub artifacts: ArtifactRegistry,
    pub shared_state: SharedState,
}
```

**Lightweight Reference (for messages):**
```rust
pub struct AgentContextRef {
    pub task_id: String,
    pub item_id: String,
    pub cycle: u32,
    pub phase: Option<String>,
    pub workspace_root: String,
    pub workspace_id: String,
}
```

**Template Rendering:**
```rust
pub fn render_template_with_pipeline(
    &self, 
    template: &str, 
    pipeline: Option<&PipelineVariables>
) -> String
```

Substitutes:
- `{task_id}`, `{item_id}`, `{cycle}`, `{phase}`, `{workspace_root}`
- `{source_tree}` (alias for workspace_root)
- `{build_output}`, `{test_output}` (from pipeline)
- `{build_errors}`, `{test_failures}` (JSON arrays)
- Custom pipeline variables `{varname}`
- `upstream[i].exit_code`, `upstream[i].confidence`, `upstream[i].artifacts[j].content`
- `{artifacts.count}`

**Special Handling:**
- Bash escaping for shell injection protection (escapes: `\`, `$`, `` ` ``, `"`, `!`)
- Upstream outputs registered into artifact registry by phase

#### 2.3 **Message Bus** (`collab/message.rs` - 336 lines)

**Message Structure:**
```rust
pub struct AgentMessage {
    pub id: Uuid,
    pub msg_type: MessageType,           // Request/Response/Ack/Publish/Forward
    pub sender: AgentEndpoint,
    pub receivers: Vec<AgentEndpoint>,
    pub payload: MessagePayload,
    pub correlation_id: Option<Uuid>,   // Links response to request
    pub timestamp: DateTime<Utc>,
    pub ttl: Duration,                  // Time-to-live
    pub delivery_mode: DeliveryMode,    // FireAndForget/AtLeastOnce/ExactlyOnce/Broadcast
}
```

**Endpoints:**
```rust
pub struct AgentEndpoint {
    pub agent_id: String,
    pub phase: Option<String>,
    pub task_id: Option<String>,
    pub item_id: Option<String>,
}
```
- `agent(id)` - Global agent
- `for_phase(id, phase)` - Phase-specific
- `for_task_item(id, task, item)` - Task-specific

**Payload Types:**
```rust
pub enum MessagePayload {
    ExecutionRequest(ExecutionRequest),     // Command to execute
    ExecutionResult(ExecutionResult),       // Result with AgentOutput
    Artifact(Artifact),                     // Artifact publication
    ContextUpdate(ContextUpdate),           // Shared state modification
    ControlSignal(ControlSignal),          // Cancel/Pause/Resume/Retry/Skip
    Custom(serde_json::Value),
}
```

**Message Bus Implementation:**
```rust
pub struct MessageBus {
    tx: mpsc::Sender<AgentMessage>,
    message_store: Arc<RwLock<HashMap<Uuid, AgentMessage>>>,
}
```
- Async channel for publication (1000-message buffer)
- RwLock-protected store for message history
- `publish()` stores and sends to all receivers
- Broadcast messages have empty receivers (still stored)

**Delivery Guarantees:**
- **FireAndForget:** No receipt confirmation
- **AtLeastOnce:** Default (300s TTL)
- **ExactlyOnce:** Duplicate detection expected at consumer
- **Broadcast:** Published to all subscribers (0 explicit receivers)

#### 2.4 **Agent Output** (`collab/output.rs` - 167 lines)

```rust
pub struct AgentOutput {
    pub run_id: Uuid,
    pub agent_id: String,
    pub phase: String,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub artifacts: Vec<Artifact>,
    pub metrics: ExecutionMetrics,
    pub confidence: f32,                    // [0.0, 1.0]
    pub quality_score: f32,                 // [0.0, 1.0]
    pub created_at: DateTime<Utc>,
    pub build_errors: Vec<BuildError>,
    pub test_failures: Vec<TestFailure>,
}
```

**Metrics:**
```rust
pub struct ExecutionMetrics {
    pub duration_ms: u64,
    pub tokens_consumed: Option<u64>,
    pub api_calls: Option<u32>,
    pub retry_count: u32,
}
```

**Builder Pattern:**
```rust
output
    .with_confidence(0.85)
    .with_quality_score(0.9)
    .with_metrics(metrics)
    .with_artifacts(vec![...])
```

---

## 3. SESSION MANAGEMENT

**File:** `core/src/session_store.rs` (22+ KB)

### Session Lifecycle

**Session States:**
- `active` - Currently executing
- `detached` - Interrupted but available for reconnection
- `exited` - Completed normally
- `failed` - Terminated with error

**SessionRow Structure:**
```rust
pub struct SessionRow {
    pub id: String,
    pub task_id: String,
    pub task_item_id: Option<String>,
    pub step_id: String,
    pub phase: String,
    pub agent_id: String,
    pub state: String,                  // Session state
    pub pid: i64,                       // Process ID
    pub pty_backend: String,            // PTY implementation
    pub cwd: String,                    // Working directory
    pub command: String,                // Executed command
    pub input_fifo_path: String,        // Input FIFO
    pub stdout_path: String,            // Output files
    pub stderr_path: String,
    pub transcript_path: String,        // Full session transcript
    pub output_json_path: Option<String>,
    pub writer_client_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i64>,
}
```

### Operations

**AsyncSessionStore Interface:**
- `insert_session(session)` - Create new session
- `update_session_state(id, state, exit_code, ended)` - Update status
- `update_session_pid(id, pid)` - Set process ID
- `load_session(id)` - Retrieve session
- `load_active_session_for_task_step(task_id, step_id)` - Find active session
- `list_task_sessions(task_id)` - All sessions for task
- `acquire_writer(session_id, client_id)` - Acquire write lock
- `attach_reader(session_id, client_id)` - Attach reader
- `cleanup_stale_sessions(max_age_hours)` - Delete old sessions
- `release_attachment(session_id, client_id, reason)` - Detach client

### Writer/Reader Model
- Single writer (exclusive lock)
- Multiple readers (tracked in `session_attachments`)
- Exclusive access prevents concurrent writes to stdin
- Readers can observe output without interference

### Multi-Process Safety
- **Synchronous inserts via NewSession** (minimal data copy)
- **Async reads via tokio_rusqlite** 
- **RwLock-based state management** for concurrent access

---

## 4. STATE MANAGEMENT

**File:** `core/src/state.rs` (229 lines)

### InnerState Structure

```rust
pub struct InnerState {
    pub app_root: PathBuf,
    pub db_path: PathBuf,
    pub unsafe_mode: bool,
    pub async_database: Arc<AsyncDatabase>,
    pub logs_dir: PathBuf,
    pub active_config: RwLock<ActiveConfig>,        // Workflow config
    pub active_config_error: RwLock<Option<String>>, // Parse error
    pub active_config_notice: RwLock<Option<ConfigSelfHealReport>>,
    pub running: Mutex<HashMap<String, RunningTask>>,  // Process tracking
    pub agent_health: RwLock<HashMap<String, AgentHealthState>>,
    pub agent_metrics: RwLock<HashMap<String, AgentMetrics>>,
    pub message_bus: Arc<MessageBus>,
    pub event_sink: RwLock<Arc<dyn EventSink>>,
    pub db_writer: Arc<DbWriteCoordinator>,
    pub session_store: Arc<AsyncSessionStore>,
    pub task_repo: Arc<AsyncSqliteTaskRepository>,
    pub store_manager: StoreManager,
}
```

### Shared State Pattern

**ManagedState (Arc wrapper):**
```rust
pub struct ManagedState {
    pub inner: Arc<InnerState>,
}
```
- Cloneable reference to shared state
- Used throughout async contexts

### Concurrency Primitives

| Component | Type | Purpose |
|-----------|------|---------|
| active_config | RwLock | Allow multiple readers, exclusive writers |
| agent_health | RwLock | Health state tracking |
| agent_metrics | RwLock | Performance metrics |
| running | Mutex | Task process management |
| event_sink | RwLock | Event broadcasting |

### Safe Accessor Functions

```rust
pub fn read_agent_health(state: &InnerState) -> RwLockReadGuard<HashMap<String, AgentHealthState>>
pub fn write_agent_health(state: &InnerState) -> RwLockWriteGuard<HashMap<String, AgentHealthState>>
```

**Poisoning Recovery:** If lock poisoned, uses `into_inner()` to recover data (last known state).

### RunningTask Model

```rust
pub struct RunningTask {
    pub stop_flag: Arc<AtomicBool>,        // Shared across forks
    pub child: Arc<Mutex<Option<Child>>>,  // Process handle
}

impl RunningTask {
    pub fn fork(&self) -> Self {
        Self {
            stop_flag: Arc::clone(&self.stop_flag),  // Shared
            child: Arc::new(Mutex::new(None)),        // New slot
        }
    }
}
```

**Fork Purpose:** Parallel item execution within task — stopping task sets shared flag, all forked items observe it.

### Concurrency Guarantees

- **MAX_CONCURRENT_TASKS:** 10 (semaphore-based limiting)
- **Async database:** tokio_rusqlite for non-blocking I/O
- **Lock poisoning recovery:** Preserves availability over strict safety

---

## 5. HEALTH MONITORING SYSTEM

**File:** `core/src/health.rs` (399 lines)

### Health Tracking Model

```rust
pub struct AgentHealthState {
    pub diseased_until: Option<DateTime<Utc>>,   // Quarantine deadline
    pub consecutive_errors: u32,                 // Recent failures
    pub total_lifetime_errors: u32,              // Cumulative count
    pub capability_health: HashMap<String, CapabilityHealth>,
}

pub struct CapabilityHealth {
    pub success_count: u32,
    pub failure_count: u32,
    pub last_error_at: Option<DateTime<Utc>>,
}
```

### Health Status Functions

**is_agent_healthy(health_map, agent_id)**
- Returns true if:
  - Agent not in map (unknown = healthy)
  - No diseased_until set
  - diseased_until in past (quarantine expired)
- Returns false if diseased_until in future

**is_capability_healthy(health_map, agent_id, capability)**
- When not diseased: returns true
- When diseased:
  - Success rate = successes / (successes + failures)
  - Returns true if rate ≥ 0.5 (50% threshold)
  - Returns false if no capability data

### State Mutations

**increment_consecutive_errors(state, agent_id)**
- Increments consecutive and lifetime error counts
- Emits `agent_health_changed` event
- Returns consecutive error count

**mark_agent_diseased(state, agent_id)**
- Sets `diseased_until = now + 5 hours` (DISEASE_DURATION_HOURS)
- Used after threshold of errors

**reset_consecutive_errors(state, agent_id)**
- Zeroes consecutive count (lifetime persists)
- Emits event

**update_capability_health(state, agent_id, capability, success)**
- Increments success or failure counter
- Records last_error_at on failure

### Event Emissions

All mutations emit `agent_health_changed` events:
```json
{
  "agent_id": "qa_agent",
  "healthy": false,
  "diseased_until": "2025-01-10T15:30:00Z",
  "consecutive_errors": 3
}
```

---

## 6. ANOMALY DETECTION SYSTEM

**File:** `core/src/anomaly.rs` (306 lines)

### Anomaly Rules (11 Total)

| Rule | Severity | Escalation | Description |
|------|----------|-----------|-------------|
| `LowOutput` | Warning | Intervene | Minimal/empty execution output |
| `LongRunning` | Info | Notice | Execution exceeded time threshold |
| `TransientReadError` | Warning | Attention | Temporary I/O error (may retry) |
| `DuplicateRunner` | Error | Intervene | Multiple executors of same task |
| `OverlappingCycles` | Error | Intervene | Cycles running simultaneously |
| `OverlappingSteps` | Error | Intervene | Steps executing in wrong order |
| `MissingStepEnd` | Warning | Attention | Step started but not completed |
| `EmptyCycle` | Info | Notice | Cycle produced no work |
| `OrphanCommand` | Warning | Attention | Untracked command execution |
| `NonzeroExit` | Warning | Attention | Command failed with exit code > 0 |
| `UnexpandedTemplateVar` | Warning | Attention | Unresolved template variable |

### Severity Levels
- **Error:** Critical system issue requiring intervention
- **Warning:** Notable problem, may indicate bug
- **Info:** Informational, normal operation

### Escalation Levels
- **Intervene:** Stop and require human/agent action
- **Attention:** Log and monitor, but continue
- **Notice:** Informational only

### Anomaly Structure

```rust
pub struct Anomaly {
    pub rule: String,              // canonical_name()
    pub severity: Severity,
    pub escalation: Escalation,
    pub message: String,
    pub at: Option<String>,        // Location/timestamp
}

impl Anomaly {
    pub fn new(rule: AnomalyRule, message: String, at: Option<String>) -> Self {
        // Severity and escalation determined by rule defaults
    }
}
```

### Rule Metadata

Each rule provides:
- `canonical_name()` - String ID (snake_case)
- `default_severity()` - Error/Warning/Info
- `escalation()` - Intervene/Attention/Notice
- `display_tag()` - SCREAMING_SNAKE_CASE
- `from_canonical(name)` - Deserialize from string

---

## 7. OUTPUT VALIDATION

**File:** `core/src/output_validation.rs` (400+ lines)

### Phase Classification

**Strict Phases (require JSON stdout):**
- `qa`, `fix`, `retest`, `guard`, `adaptive_plan`
- Must produce single JSON object: `{confidence, quality_score, artifacts, ...}`

**Build Phases (structured errors):**
- `build`, `lint`
- Parses `build_errors` from output

**Test Phases (structured failures):**
- `test`
- Parses `test_failures` from output

**Other Phases:**
- Can output any format (logs/plaintext)

### Validation Pipeline

```rust
pub fn validate_phase_output(
    phase: &str,
    run_id: Uuid,
    agent_id: &str,
    exit_code: i64,
    stdout: &str,
    stderr: &str,
) -> Result<ValidationOutcome>
```

**Step 1: Fatal Error Detection**
```rust
fn detect_fatal_agent_error(stdout: &str, stderr: &str) -> Option<&'static str>
```
- Scans stderr fully for provider errors
- Filters JSON lines from stdout (to avoid false positives in embedded code)
- Patterns: `rate-limited`, `quota exceeded`, `authentication failed`
- Returns reason string if match found

**Step 2: Phase-Specific JSON Parsing**
- Strict phases: Must parse as valid JSON
- Other phases: Optional JSON (falls back to plaintext)

**Step 3: Confidence/Quality Extraction**
```rust
pub struct AgentOutput {
    pub confidence: f32,        // Extracted from JSON or defaults to 1.0
    pub quality_score: f32,     // Extracted from JSON or defaults to 1.0
    ...
}
```
- Values clamped to [0.0, 1.0]

**Step 4: Artifact Extraction**
- From JSON "artifacts" field
- Via line-by-line parsing

**Step 5: Diagnostic Parsing**

### Diagnostic Parser Trait

```rust
trait DiagnosticParser: Default {
    type Item;
    fn process_line(&mut self, line: &str);
    fn finish(self) -> Vec<Self::Item>;
}
```

Generic driver iterates stderr+stdout lines, calling `process_line()` sequentially.

### Build Error Parser

**Pattern Matching:**
- Lines starting with `error:` → BuildError(level: Error)
- Lines starting with `warning:` → BuildError(level: Warning)
- Locator lines `" --> src/file.rs:10:5"` → Extracts file/line/column

**LocationLine Parser:**
```
--> src/main.rs:10:5
    ↓
file: Some("src/main.rs")
line: Some(10)
column: Some(5)
```
- Uses `rsplitn(3, ':')` to handle filenames with colons

### Test Failure Parser

**State Machine:**
- `---- test_name stdout ----` → Starts failure block
- Captures lines until next block or section end
- `test module::name ... FAILED` → Quick detection
- Accumulates message lines

---

## 8. SCHEDULER SERVICE

**File:** `core/src/scheduler_service.rs` (200+ lines)

### Scheduling Operations

**enqueue_task(state, task_id)**
- Sets status to `pending`
- Touches worker wake signal file
- Emits scheduler_enqueued event

**claim_next_pending_task(state)**
- **Atomicity:** Uses IMMEDIATE transaction
- **Priority:** `restart_pending` > `pending`
- **Query:** Ordered by created_at ascending
- **Status Update:** `running` (atomic within transaction)
- **Concurrency:** Prevents duplicate claims via transaction isolation

**pending_task_count(state)**
- Simple COUNT query on tasks WHERE status = 'pending'

### Worker Signals

**Signal Files (in app_root/data/):**
- `worker.stop` → Stop signal
- `worker.wakeup` → Wake/notification signal

**Functions:**
- `touch_worker_wake_signal()` - Create/update wakeup file
- `signal_worker_stop()` - Create stop file + wake signal
- `clear_worker_stop_signal()` - Delete stop file

### Single-Winner Guarantee

```rust
pub async fn claim_next_pending_task(state: &InnerState) -> Result<Option<String>> {
    state.async_database.writer().call(|conn| {
        let tx = conn.transaction_with_behavior(
            rusqlite::TransactionBehavior::Immediate  // ← Immediate lock
        )?;
        
        // SELECT FOR UPDATE equivalent (via Immediate mode)
        let task_id = tx.query_row(
            "SELECT id FROM tasks WHERE status IN (...) ORDER BY ... LIMIT 1",
            ...
        ).optional()?;
        
        // Atomic update within same transaction
        let updated = tx.execute(
            "UPDATE tasks SET status = 'running' WHERE id = ?1 AND status IN (...)",
            ...
        )?;
        
        tx.commit()?;
        Ok(if updated == 1 { Some(task_id) } else { None })
    }).await?;
}
```

**Transaction Isolation:**
- Immediate mode acquires reserved lock immediately
- Prevents other connections from reading/writing same rows
- Only one thread succeeds with `updated == 1`

---

## 9. CUSTOM RESOURCE DEFINITIONS (CRD)

**Files:** `core/src/crd/` (11 modules)

### CRD Meta-Schema

```rust
pub struct CustomResourceDefinition {
    pub kind: String,                  // PascalCase, e.g., "PromptLibrary"
    pub plural: String,                // CLI plural, e.g., "promptlibraries"
    pub short_names: Vec<String>,      // e.g., ["pl"]
    pub group: String,                 // e.g., "extensions.orchestrator.dev"
    pub versions: Vec<CrdVersion>,
    pub hooks: CrdHooks,               // Lifecycle hooks
    pub scope: CrdScope,               // Namespaced/Cluster/Singleton
    pub builtin: bool,                 // Cannot be deleted/overwritten
}

pub struct CrdVersion {
    pub name: String,                  // "v1", "v2", etc.
    pub schema: serde_json::Value,     // JSON Schema subset
    pub served: bool,                  // Is this version active?
    pub cel_rules: Vec<CelValidationRule>,
}
```

### Validation Flow

**validate_crd_definition(config, manifest)**
1. Validate metadata.name (DNS-safe)
2. Verify kind is PascalCase, doesn't conflict with builtins
3. Verify plural and short_names don't conflict with builtin aliases
4. Validate group non-empty
5. Require ≥1 version with served=true
6. Validate each version's JSON Schema
7. Pre-compile all CEL rules (syntax check)
8. Check kind+group uniqueness

**validate_custom_resource(config, manifest)**
1. Validate metadata.name
2. Find CRD for manifest.kind
3. Resolve version from manifest.apiVersion
4. Validate spec against JSON Schema
5. Evaluate CEL rules against spec

### JSON Schema Validator

**Supported Keywords:**
- `type` - null/boolean/object/array/number/string
- `required` - Array of required field names
- `properties` - Object with nested schemas
- `items` - Schema for array elements
- `enum` - Allowed values
- `minLength`/`maxLength` - String length constraints
- `minimum`/`maximum` - Numeric constraints
- `minItems`/`maxItems` - Array size constraints
- `additionalProperties` - Boolean (false = strict mode)
- `pattern` - Glob-style string matching

**Path Tracking:**
```
$.field1.field2[3].name → Full path for error messages
```

### CEL Rule Validation

**Compilation Time:**
- Pre-compiled to detect syntax errors
- `self` variable bound to spec value
- Wrapped in panic handler (prevents parser crashes)

**Runtime:**
- Evaluate against actual spec
- Expect boolean result
- On false: emit rule.message as error

### Hooks System

```rust
pub struct CrdHooks {
    pub on_create: Option<String>,     // Shell command
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

pub fn execute_hook(
    hook_command: &str,
    kind: &str,
    name: &str,
    action: &str,
    spec: &serde_json::Value,
) -> Result<()>
```

**Environment Variables Passed:**
- `RESOURCE_KIND` - CRD kind
- `RESOURCE_NAME` - Instance name
- `RESOURCE_ACTION` - "create"/"update"/"delete"
- `RESOURCE_SPEC` - JSON serialized spec

**Failure Behavior:** Hook failure blocks the operation.

### Resource Storage Key

Format: `{kind}/{name}` (e.g., `PromptLibrary/my-library`)

### Lifecycle

**Create:**
1. Validate
2. Execute on_create hook
3. Store with generation=1
4. Return ApplyResult::Created

**Update:**
1. Validate
2. Compare spec + apiVersion + metadata
3. Execute on_update hook (if changed)
4. Increment generation
5. Return ApplyResult::Unchanged or ApplyResult::Configured

**Delete:**
1. Find CRD for hooks (best-effort)
2. Execute on_delete hook
3. Remove from storage
4. Return true if existed

---

## 10. STORE ABSTRACTION

**Files:** `core/src/store/` (5 modules)

### Three-Layer Pattern (K8s-inspired)

1. **StoreBackendProvider CRD:** Defines HOW a backend works
2. **WorkflowStore CRD:** Defines WHAT store to use
3. **Store Entries:** Actual persisted data

### Store Operations

```rust
pub enum StoreOp {
    Get { store_name: String, project_id: String, key: String },
    Put { store_name: String, project_id: String, key: String, value: String, task_id: String },
    Delete { store_name: String, project_id: String, key: String },
    List { store_name: String, project_id: String, limit: u64, offset: u64 },
    Prune { store_name: String, project_id: String, max_entries: Option<u64>, ttl_days: Option<u64> },
}

pub enum StoreOpResult {
    Value(Option<serde_json::Value>),
    Entries(Vec<StoreEntry>),
    Ok,
}
```

### Backend Types

#### 1. **LocalStoreBackend** (SQLite)
- Async database queries
- Table: `workflow_store_entries` (store_name, project_id, key, value_json, task_id, updated_at)
- Operations: CRUD + pagination
- Prune: Deletes old entries (TTL or count-based)

#### 2. **FileStoreBackend** (Filesystem)
- Directory structure: `data/stores/{store_name}/{project_id}/`
- Files: `{key}.json`
- Synchronous operations
- Prune: Removes oldest files

#### 3. **CommandAdapter** (Generic Shell)
- Executes user-defined provider commands
- Environment variables: STORE_NAME, PROJECT_ID, KEY, VALUE, LIMIT, OFFSET, MAX_ENTRIES, TTL_DAYS
- Output parsing modes: Value/Entries/None
- Exit code checking

### StoreManager Dispatch

```rust
pub async fn execute(
    &self,
    custom_resources: &HashMap<String, CustomResource>,
    op: StoreOp,
) -> Result<StoreOpResult>
```

**Steps:**
1. Resolve WorkflowStore config (auto-provision with defaults)
2. Validate schema on Put operations
3. Lookup provider from store config
4. Dispatch to appropriate backend
5. Return result

**Provider Resolution:**
- Looks up `WorkflowStore/{store_name}` in custom resources
- Falls back to default config if not found
- Provider field determines which backend handles operation

---

## 11. TASK REPOSITORY

**Files:** `core/src/task_repository/` (12 modules)

### Data Access Layer

**TaskRepository Trait:**
```rust
pub trait TaskRepository {
    fn resolve_task_id(&self, task_id_or_prefix: &str) -> Result<String>;
    fn load_task_summary(&self, task_id: &str) -> Result<TaskSummary>;
    fn load_task_detail_rows(&self, task_id: &str) -> Result<(Vec<TaskItemDto>, Vec<CommandRunDto>, Vec<EventDto>)>;
    fn load_task_item_counts(&self, task_id: &str) -> Result<(i64, i64, i64)>;
    fn list_task_ids_ordered_by_created_desc(&self) -> Result<Vec<String>>;
    fn find_latest_resumable_task_id(&self, include_pending: bool) -> Result<Option<String>>;
    // ... 20+ additional methods
}
```

**SqliteTaskRepository:**
- Wraps connection source (async-aware)
- Implements all trait methods via queries

### Query Patterns

#### 1. **Task Resolution**
```sql
SELECT id FROM tasks WHERE id = ?1  -- Exact match
SELECT id FROM tasks WHERE id LIKE '?1%'  -- Prefix match
```
- Returns error if 0 matches
- Returns error if multiple matches
- Single match returned as resolved ID

#### 2. **Task Summary Load**
```sql
SELECT id, name, status, started_at, completed_at, goal, 
       target_files_json, project_id, workspace_id, workflow_id, 
       created_at, updated_at, parent_task_id, spawn_reason, spawn_depth
FROM tasks WHERE id = ?1
```
- Deserializes `target_files_json` array

#### 3. **Task Detail Rows** (Batch Query)
```sql
-- Items
SELECT id, task_id, order_no, qa_file_path, status, 
       ticket_files_json, ticket_content_json, fix_required, fixed, 
       last_error, started_at, completed_at, updated_at
FROM task_items WHERE task_id = ?1 ORDER BY order_no

-- Runs (last 120)
SELECT cr.id, cr.task_item_id, cr.phase, cr.command, cr.cwd, 
       cr.workspace_id, cr.agent_id, cr.exit_code, cr.stdout_path, 
       cr.stderr_path, cr.started_at, cr.ended_at, cr.interrupted
FROM command_runs cr
JOIN task_items ti ON ti.id = cr.task_item_id
WHERE ti.task_id = ?1
ORDER BY cr.started_at DESC LIMIT 120

-- Events (last 200)
SELECT id, task_id, task_item_id, event_type, payload_json, created_at
FROM events WHERE task_id = ?1
ORDER BY id DESC LIMIT 200
```

### State Mutations

#### set_task_status(conn, task_id, status, set_completed)
- Conditional logic based on status:
  - `completed` statuses: Set completed_at
  - `running`: Clear completed_at, set started_at
  - `pending`/`paused`/etc: Clear completed_at
  - Others: Update status only

#### prepare_task_for_start_batch(conn, task_id)
- Pre-checks: Task exists, not already running
- Special case: `restart_pending` resumes without item reset
- For failed tasks: Reset unresolved items to pending
- Sets status to running

#### update_task_cycle_state(conn, task_id, cycle, init_done)
- Updates current_cycle and init_done flag

### Concurrent Design

- **AsyncSqliteTaskRepository:** Wraps SqliteTaskRepository with tokio_rusqlite
- Reader calls use `.call(|conn| {...})` on reader pool
- Writer calls use `.call(|conn| {...})` on writer pool
- Errors converted via `flatten_err` utility

---

## 12. EXISTING ANALYSIS REPORTS

**Location:** `docs/report/`

| Report | Purpose |
|--------|---------|
| `sandbox-network-enforcement-governance.md` | Sandbox isolation and network policy |
| `self-bootstrap-smoke-runbook.md` | Initial bootstrapping verification |
| `self-bootstrap-survival-extended-smoke-runbook.md` | Extended system resilience testing |
| `self-bootstrap-survival-self-repair-extended-smoke-runbook.md` | Self-healing verification |
| `self-bootstrap-survival-smoke-runbook.md` | Core survival smoke tests |
| `self-evolution-test-report.md` | System self-evolution capabilities |

---

## CROSS-CUTTING PATTERNS

### Error Handling

**Strategies:**
- `Result<T>` with anyhow for propagation
- Lock poisoning recovery: `into_inner()`
- Transaction rollback on error
- Event emission for observable failures

### Serialization

- **serde_json:** Flexible JSON handling (Value type)
- **JSON Schema:** Subset validation (type, properties, constraints)
- **Escape for bash:** Special char escaping in templates

### Async Patterns

- **tokio channels:** Message bus (mpsc, 1000-msg buffer)
- **tokio_rusqlite:** Async database via thread pool
- **Arc/RwLock/Mutex:** Shared state coordination
- **task_semaphore:** MAX_CONCURRENT_TASKS=10 limiting

### Concurrency Safety

- **Type system:** Rust's ownership prevents use-after-free
- **Lock hierarchy:** Prevents deadlock (single lock per component)
- **Transaction isolation:** Immediate mode for atomic operations
- **Atomic flags:** `AtomicBool` for signaling

---

## EDGE CASES & RESILIENCE

| Component | Edge Case | Handling |
|-----------|-----------|----------|
| **DAG** | Self-loops | Cycle detection catches |
| **DAG** | Disconnected nodes | Topological sort tolerates |
| **DAG** | Empty graph | Returns empty result |
| **Message Bus** | Broadcast (no receivers) | Still stored in message_store |
| **Session** | Stale sessions | cleanup_stale_sessions() removes old |
| **Health** | Unknown agent | Defaults to healthy |
| **Health** | Zero capability data | Defaults to unhealthy if diseased |
| **Output** | JSON + plaintext lines | Line-by-line parsing extracts artifacts |
| **Scheduler** | Concurrent claims | Transaction isolation: single winner |
| **CRD** | Unknown kind | Error on validation |
| **Store** | Missing key | Returns None value |
| **Task Repo** | Prefix ambiguity | Error: "multiple tasks match" |

---

## PERFORMANCE CHARACTERISTICS

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| Cycle detection | O(V+E) | DFS, acceptable for typical DAGs |
| Topological sort | O(V+E) | Kahn's algorithm, optimal |
| Artifact lookup | O(1) phase + O(n) filter | HashMap + vec iteration |
| Task resolution | O(log n) | Binary index on task id |
| Claim task | O(1) | Single row transaction |
| Message publish | O(m) | m = number of receivers |
| Store get/put | O(1) | Direct key access |

---

## SECURITY CONSIDERATIONS

1. **Bash Injection Prevention:** Template vars escaped (backslash, dollar, backtick, quote, bang)
2. **Lock Poisoning Recovery:** Prevents denial of service from failed lock holders
3. **Transaction Isolation:** Prevents race conditions in task claiming
4. **Hook Validation:** Pre-compiled CEL expressions prevent injection
5. **Schema Enforcement:** Custom resource specs validated before storage
6. **File Permissions:** Stores use file system permissions

---

## RECOMMENDATIONS

1. **DAG Visualization:** Implement graphviz export for debugging workflows
2. **Health Dashboard:** Real-time display of agent health state + capabilities
3. **Message Tracing:** Correlation IDs for end-to-end request tracking
4. **Audit Logging:** Track all CRD lifecycle events (create/update/delete)
5. **Metrics Export:** Prometheus-compatible health/metric endpoints
6. **Schema Versioning:** Document migration paths for CRD version upgrades
7. **Store Replication:** Enable failover for persistent store backends

