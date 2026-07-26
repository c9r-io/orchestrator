use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::Path;

pub use crate::sqlite::SQLITE_BUSY_TIMEOUT_MS;

/// Counts returned after deleting all persisted state for one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectResetStats {
    /// Number of task rows removed.
    pub tasks: u64,
    /// Number of task-item rows removed.
    pub task_items: u64,
    /// Number of command-run rows removed.
    pub command_runs: u64,
    /// Number of event rows removed.
    pub events: u64,
    /// Number of ticket files removed from disk.
    pub tickets_cleaned: u64,
}

/// Minimal reference to a non-terminal task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskReference {
    /// Stable task identifier.
    pub task_id: String,
    /// Current task status label.
    pub status: String,
}

/// Execution metrics materialized into the persistence layer.
#[derive(Debug, Clone)]
pub struct TaskExecutionMetric {
    /// Stable task identifier.
    pub task_id: String,
    /// Task status associated with the metric sample.
    pub status: String,
    /// Workflow cycle active when the sample was recorded.
    pub current_cycle: u32,
    /// Number of unresolved task items at sample time.
    pub unresolved_items: i64,
    /// Total number of task items known for the task.
    pub total_items: i64,
    /// Number of failed task items at sample time.
    pub failed_items: i64,
    /// Number of command runs recorded so far.
    pub command_runs: i64,
    /// RFC 3339 timestamp when the metric was captured.
    pub created_at: String,
}

/// Audit payload written for one control-plane authorization decision.
#[derive(Debug, Clone)]
pub struct ControlPlaneAuditRecord {
    /// Correlation identifier shared with canonical action audit when available.
    pub request_id: Option<String>,
    /// Transport used by the incoming RPC, such as `tcp`.
    pub transport: String,
    /// Remote peer address when known.
    pub remote_addr: Option<String>,
    /// Fully qualified RPC name.
    pub rpc: String,
    /// Authenticated subject identifier when available.
    pub subject_id: Option<String>,
    /// Authentication outcome label.
    pub authn_result: String,
    /// Authorization outcome label.
    pub authz_result: String,
    /// Effective role assigned to the subject.
    pub role: Option<String>,
    /// Human-readable reason for denial or fallback behavior.
    pub reason: Option<String>,
    /// SHA-256 fingerprint of the presented client certificate.
    pub tls_fingerprint: Option<String>,
    /// Pipeline stage that rejected the request.
    pub rejection_stage: Option<String>,
    /// Traffic bucket selected for protection enforcement.
    pub traffic_class: Option<String>,
    /// Whether subject-scoped or global limits produced the decision.
    pub limit_scope: Option<String>,
    /// Final decision label written by the limiter.
    pub decision: Option<String>,
    /// Stable machine-readable reason code.
    pub reason_code: Option<String>,
    /// Executable path of the peer process (UDS only, forensic audit).
    pub peer_exe: Option<String>,
}

/// Audit payload for plugin-related authorization and execution decisions.
#[derive(Debug, Clone)]
pub struct PluginAuditRecord {
    /// Action: `crd_apply`, `plugin_execute`, or `hook_execute`.
    pub action: String,
    /// CRD kind that owns the plugin.
    pub crd_kind: String,
    /// Plugin name (None for hooks).
    pub plugin_name: Option<String>,
    /// Plugin type: `interceptor`, `transformer`, `cron`, or `hook`.
    pub plugin_type: Option<String>,
    /// Full command string.
    pub command: String,
    /// Caller identity (TLS subject_id or `uds:<pid>`).
    pub applied_by: Option<String>,
    /// Transport: `tcp` or `uds`.
    pub transport: Option<String>,
    /// Peer process ID (UDS only).
    pub peer_pid: Option<i32>,
    /// Verdict: `allowed`, `denied`, or `audit_warning`.
    pub result: String,
    /// Active policy mode: `deny`, `allowlist`, or `audit`.
    pub policy_mode: Option<String>,
    /// Name of the sandbox execution profile applied at runtime.
    pub sandbox_profile: Option<String>,
    /// Runtime policy verdict: `allowed`, `denied`, or `audit_warning`.
    pub policy_verdict: Option<String>,
}

/// Inserts one plugin-audit record into persistence.
pub fn insert_plugin_audit(db_path: &Path, record: &PluginAuditRecord) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT INTO plugin_audit (
            created_at, action, crd_kind, plugin_name, plugin_type,
            command, applied_by, transport, peer_pid, result, policy_mode,
            sandbox_profile, policy_verdict
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            crate::now_ts(),
            record.action,
            record.crd_kind,
            record.plugin_name,
            record.plugin_type,
            record.command,
            record.applied_by,
            record.transport,
            record.peer_pid,
            record.result,
            record.policy_mode,
            record.sandbox_profile,
            record.policy_verdict,
        ],
    )?;
    Ok(())
}

/// Opens a SQLite connection using the orchestrator persistence defaults.
pub fn open_conn(db_path: &Path) -> Result<Connection> {
    crate::sqlite::open_conn(db_path)
}

/// Applies the standard busy timeout and pragma configuration to a connection.
pub fn configure_conn(conn: &Connection) -> Result<()> {
    crate::sqlite::configure_conn(conn)
}

/// Ensures the persistence schema exists and is migrated to the current version.
pub fn init_schema(db_path: &Path) -> Result<()> {
    crate::schema::PersistenceBootstrap::ensure_current(db_path)?;
    Ok(())
}

/// Counts running or pending tasks for one project workspace pair.
pub fn count_non_terminal_tasks_by_workspace(
    conn: &Connection,
    project_id: &str,
    workspace_id: &str,
) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE project_id = ?1
           AND workspace_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')",
        params![project_id, workspace_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Counts running or pending tasks for one project workflow pair.
pub fn count_non_terminal_tasks_by_workflow(
    conn: &Connection,
    project_id: &str,
    workflow_id: &str,
) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE project_id = ?1
           AND workflow_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')",
        params![project_id, workflow_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Lists the oldest non-terminal tasks for one project workspace pair.
pub fn list_non_terminal_tasks_by_workspace(
    conn: &Connection,
    project_id: &str,
    workspace_id: &str,
    limit: usize,
) -> Result<Vec<TaskReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, status FROM tasks
         WHERE project_id = ?1
           AND workspace_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')
         ORDER BY created_at ASC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![project_id, workspace_id, limit], |row| {
        Ok(TaskReference {
            task_id: row.get(0)?,
            status: row.get(1)?,
        })
    })?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row?);
    }
    Ok(tasks)
}

/// Lists the oldest non-terminal tasks for one project workflow pair.
pub fn list_non_terminal_tasks_by_workflow(
    conn: &Connection,
    project_id: &str,
    workflow_id: &str,
    limit: usize,
) -> Result<Vec<TaskReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, status FROM tasks
         WHERE project_id = ?1
           AND workflow_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')
         ORDER BY created_at ASC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![project_id, workflow_id, limit], |row| {
        Ok(TaskReference {
            task_id: row.get(0)?,
            status: row.get(1)?,
        })
    })?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row?);
    }
    Ok(tasks)
}

/// Deletes persisted runtime data from a database path after guarding against active tasks.
pub fn reset_db_by_path(db_path: &Path, include_history: bool, include_config: bool) -> Result<()> {
    let conn = open_conn(db_path)?;

    let active_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE status IN ('running', 'restart_pending')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if active_count > 0 {
        anyhow::bail!(
            "db reset blocked: {active_count} active task(s) with status running/restart_pending. \
             Use `project reset <project> --force` for project-scoped cleanup instead."
        );
    }

    conn.execute("DELETE FROM events", [])?;
    let _ = conn.execute("DELETE FROM task_graph_snapshots", []);
    let _ = conn.execute("DELETE FROM task_graph_runs", []);
    conn.execute("DELETE FROM command_runs", [])?;
    conn.execute("DELETE FROM task_items", [])?;
    conn.execute("DELETE FROM tasks", [])?;
    conn.execute("DELETE FROM task_execution_metrics", [])?;
    let _ = conn.execute("DELETE FROM control_plane_audit", []);
    if include_config {
        conn.execute("DELETE FROM orchestrator_config_versions", [])?;
    } else if include_history {
        conn.execute(
            "DELETE FROM orchestrator_config_versions WHERE version < (SELECT COALESCE(MAX(version), 0) FROM orchestrator_config_versions)",
            [],
        )?;
    }
    Ok(())
}

/// Inserts one control-plane audit record into persistence.
pub fn insert_control_plane_audit(db_path: &Path, record: &ControlPlaneAuditRecord) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT INTO control_plane_audit (
            created_at, transport, remote_addr, rpc, subject_id, authn_result,
            authz_result, role, reason, tls_fingerprint, rejection_stage,
            traffic_class, limit_scope, decision, reason_code, peer_exe, request_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            crate::now_ts(),
            record.transport,
            record.remote_addr,
            record.rpc,
            record.subject_id,
            record.authn_result,
            record.authz_result,
            record.role,
            record.reason,
            record.tls_fingerprint,
            record.rejection_stage,
            record.traffic_class,
            record.limit_scope,
            record.decision,
            record.reason_code,
            record.peer_exe,
            record.request_id,
        ],
    )?;
    Ok(())
}

/// Inserts one task execution metric sample into persistence.
pub fn insert_task_execution_metric(db_path: &Path, metric: &TaskExecutionMetric) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT INTO task_execution_metrics (task_id, status, current_cycle, unresolved_items, total_items, failed_items, command_runs, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            metric.task_id,
            metric.status,
            metric.current_cycle as i64,
            metric.unresolved_items,
            metric.total_items,
            metric.failed_items,
            metric.command_runs,
            metric.created_at
        ],
    )?;
    Ok(())
}

/// Deletes all persisted records and ticket files associated with one project.
///
/// Takes the database path rather than the daemon state, so this layer does not
/// need to know that a daemon exists. `crate::db::reset_project_data` in core
/// keeps the state-taking signature its callers use.
pub fn reset_project_data_by_path(db_path: &Path, project_id: &str) -> Result<ProjectResetStats> {
    let conn = open_conn(db_path)?;
    let tx = conn.unchecked_transaction()?;

    let tasks: i64 = tx.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )?;
    let task_items: i64 = tx.query_row(
        "SELECT COUNT(*) FROM task_items WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
        |row| row.get(0),
    )?;
    let command_runs: i64 = tx.query_row(
        "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (
            SELECT ti.id FROM task_items ti
            JOIN tasks t ON t.id = ti.task_id
            WHERE t.project_id = ?1
        )",
        params![project_id],
        |row| row.get(0),
    )?;
    let events: i64 = tx.query_row(
        "SELECT COUNT(*) FROM events WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
        |row| row.get(0),
    )?;

    tx.execute(
        "DELETE FROM task_graph_snapshots WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM task_graph_runs WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM command_runs WHERE task_item_id IN (
            SELECT ti.id FROM task_items ti
            JOIN tasks t ON t.id = ti.task_id
            WHERE t.project_id = ?1
        )",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM events WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM task_items WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM task_execution_metrics WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM tasks WHERE project_id = ?1",
        params![project_id],
    )?;

    tx.commit()?;

    Ok(ProjectResetStats {
        tasks: tasks.max(0) as u64,
        task_items: task_items.max(0) as u64,
        command_runs: command_runs.max(0) as u64,
        events: events.max(0) as u64,
        tickets_cleaned: 0,
    })
}

/// Deletes every `resources` row belonging to one project.
///
/// Separate from [`reset_project_data_by_path`], which clears a project's task
/// history: this is the declarative-resource half, run when the project itself
/// is deleted. It is one statement in its own transaction because the caller
/// has already mutated the in-memory config and is about to persist it; a
/// failure here has to leave the table untouched rather than half-cleared.
pub fn delete_project_resources(db_path: &Path, project: &str) -> Result<usize> {
    let conn = open_conn(db_path)?;
    let tx = conn.unchecked_transaction()?;
    let removed = tx.execute("DELETE FROM resources WHERE project = ?1", params![project])?;
    tx.commit()?;
    Ok(removed)
}

/// The queries a deletion guard needs to decide whether a workspace or workflow
/// can be removed.
///
/// A port, and the reason it exists is that its caller is not persistence code.
/// `core::config_load::enforce_deletion_guards` diffs two `OrchestratorConfig`
/// snapshots and formats a refusal message; the only thing it needs from the
/// database is "how many non-terminal tasks, and name a few". Taking a
/// `&rusqlite::Connection` for that put the driver's type in the signature of a
/// function that has no other relationship to it, and made the guard logic
/// untestable without a real database.
///
/// Implemented for [`rusqlite::Connection`], which is also how a caller holding
/// a `Transaction` uses it — `Transaction` derefs to `Connection`, so `&*tx`
/// satisfies it and the guard runs inside the caller's transaction as before.
pub trait DeletionGuardQueries {
    /// Counts non-terminal tasks belonging to a workspace.
    fn non_terminal_tasks_in_workspace(&self, project_id: &str, workspace_id: &str) -> Result<i64>;
    /// Names up to `limit` non-terminal tasks belonging to a workspace.
    fn sample_non_terminal_tasks_in_workspace(
        &self,
        project_id: &str,
        workspace_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskReference>>;
    /// Counts non-terminal tasks belonging to a workflow.
    fn non_terminal_tasks_in_workflow(&self, project_id: &str, workflow_id: &str) -> Result<i64>;
    /// Names up to `limit` non-terminal tasks belonging to a workflow.
    fn sample_non_terminal_tasks_in_workflow(
        &self,
        project_id: &str,
        workflow_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskReference>>;
}

impl DeletionGuardQueries for Connection {
    fn non_terminal_tasks_in_workspace(&self, project_id: &str, workspace_id: &str) -> Result<i64> {
        count_non_terminal_tasks_by_workspace(self, project_id, workspace_id)
    }

    fn sample_non_terminal_tasks_in_workspace(
        &self,
        project_id: &str,
        workspace_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskReference>> {
        list_non_terminal_tasks_by_workspace(self, project_id, workspace_id, limit)
    }

    fn non_terminal_tasks_in_workflow(&self, project_id: &str, workflow_id: &str) -> Result<i64> {
        count_non_terminal_tasks_by_workflow(self, project_id, workflow_id)
    }

    fn sample_non_terminal_tasks_in_workflow(
        &self,
        project_id: &str,
        workflow_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskReference>> {
        list_non_terminal_tasks_by_workflow(self, project_id, workflow_id, limit)
    }
}
