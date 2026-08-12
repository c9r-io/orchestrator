import { expect, test, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

async function installTauriMock(
  page: Page,
  role: "read_only" | "operator" | "admin" = "operator",
  connectionMode: "shared" | "dedicated" | "dedicated_disconnected" = "shared",
  attentionClaimConflict = false,
) {
  await page.addInitScript(({ roleName, sourceConnectionMode, shouldConflictAttentionClaim }) => {
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners = new Map<string, number>();
    const sessionCalls: Array<{ command: string; args: Record<string, unknown> }> = [];
    const processCalls: Array<{ command: string; args: Record<string, unknown> }> = [];
    let nextId = 1;
    let taskStatus = "failed";
    let updateStatusOnResume = false;
    let conflictAttentionClaim = shouldConflictAttentionClaim;
    const items = [
      { id: "attention-1", title: "Approval required", taskId: "task-1", severity: "intervention" },
      { id: "attention-2", title: "Choose recovery", taskId: "task-2", severity: "attention" },
    ];
    let attentionRows = items.map((item) => ({
      id: item.id, project_id: "project-1", task_id: item.taskId, task_item_id: null, step_id: "test", session_id: "session-1", source_route_id: item.id === "attention-1" ? "route-1" : null, source_binding_name: item.id === "attention-1" ? "analyze-badge" : null,
      kind: "step_failed", severity: item.severity, state: "open", title: item.title, summary: "A human decision is required",
      requested_decision_json: JSON.stringify({ question: "Retry from the verified boundary?" }),
      actions: [{ id: "retry_failed_item", label: "Retry safely", required_role: "operator", confirmation: "required", input_schema_json: "{}" }],
      assignee: null, occurrence_count: 1, reopen_count: 0, version: 1,
      created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:00:00Z", last_occurred_at: "2026-07-14T00:00:00Z", snoozed_until: null, resolved_at: null,
    }));
    const tasks = [
      { id: "task-running", name: "Active implementation", status: "running", total_items: 4, finished_items: 2, failed_items: 0, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:03:00Z", project_id: "project-1", workflow_id: "delivery-loop", goal: "Ship the active change" },
      { id: "task-1", name: "Fix payment failure", status: "failed", total_items: 1, finished_items: 0, failed_items: 1, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", project_id: "project-1", workflow_id: "qa-loop", goal: "Restore the failed payment test" },
      { id: "task-complete", name: "Completed documentation", status: "completed", total_items: 2, finished_items: 2, failed_items: 0, created_at: "2026-07-13T00:00:00Z", updated_at: "2026-07-14T00:02:00Z", project_id: "project-1", workflow_id: "docs-loop", goal: "Publish the operator guide" },
    ];
    const sourceEvents = [
      { id: "source-1", project_id: "project-1", provider: "slack", installation_id: "workspace-demo", external_event_id: "evt-1", event_type: "message", conversation_id: "channel-1", thread_id: "thread-1", occurred_at: "2026-07-14T00:00:00Z", received_at: "2026-07-14T00:00:01Z", normalized_json: "{}", routing_state: "needs_attention", routing_attempts: 1, routed_task_id: "task-1", last_error_code: "trigger_ambiguous" },
      { id: "source-2", project_id: "project-1", provider: "github", installation_id: "repo-demo", external_event_id: "evt-2", event_type: "pull_request", conversation_id: "pr-42", thread_id: null, occurred_at: "2026-07-14T00:02:00Z", received_at: "2026-07-14T00:02:01Z", normalized_json: "{}", routing_state: "routed", routing_attempts: 1, routed_task_id: "task-complete", last_error_code: null },
    ];
    const automationCatalog = {
      project_id: "default",
      templates: [{ name: "analyze", revision: "template-revision", skill_name: "analyze", skill_invocation: "$analyze", skill_args: ["--safe"], workflow: "analysis", workspace: "main", start: true, initial_vars: {}, goal_template: "{skill_invocation} {source_message_url}", allowed_variables: ["skill_invocation", "source_message_url"] }],
      bindings: [{ name: "analyze-badge", revision: "binding-revision", trigger_ref: "slack-main", installation_id: "T123", reaction: "agent-analyze", channels: ["C123"], all_channels: false, template_ref: "analyze", allowed_actor_roles: ["operator"], suspended: false }],
      installations: [{ trigger_name: "slack-main", installation_id: "T123", actor_ids: ["U123"], actor_roles: ["operator"], suspended: false, reaction_routing: "bindings" }],
      workflows: ["analysis", "docs"], workspaces: ["main"],
    };
    const automationRoute = { id: "route-1", project_id: "default", source_event_id: "source-1", provider: "slack", reaction: "agent-analyze", binding_name: "analyze-badge", binding_revision: "binding-revision", template_name: "analyze", template_hash: "template-revision", status: "needs_attention", error_code: "task_create_failed", error_category: "internal", task_id: "task-1", permalink: null, request_id: "request-1", generation: 1, version: 4, attempt_count: 1, max_attempts: 3, next_attempt_at: null, suspended_scope: null, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", completed_at: null };
    const sourceConnectionCatalog = { protocol_version: 1, gateway_configured: true, permalink_proxy: true, modes: [{ mode: "managed_shared", available: true, unavailable_reason: null }, { mode: "managed_dedicated", available: true, unavailable_reason: null }, { mode: "manual", available: true, unavailable_reason: null }] };
    const sharedConnection = { id: "conn-installation-1", project_id: "default", provider: "slack", display_label: "Product Slack", provisioning_mode: "managed_shared", app_ownership: "orchestrator", app_id_digest: null, manifest_version: null, provision_state: null, provision_error_code: null, installation_id: "installation-1", installation_id_digest: "team-digest", enterprise_id_digest: null, owner_daemon_id: "daemon-1", generation: 1, version: 1, state: "active", capabilities: ["delivery_v1"], scopes: ["reactions:read"], trigger_name: "slack-installation-1", last_delivery_at: null, last_acked_cursor: 0, delivery_lag: 0, last_error_code: null, created_at: "2026-07-18T00:00:00Z", updated_at: "2026-07-18T00:00:00Z", reauthorized_at: null, disconnected_at: null };
    const dedicatedConnection = { ...sharedConnection, id: "conn-dedicated-1", display_label: "Private Product Slack", provisioning_mode: "managed_dedicated", app_ownership: "workspace", app_id_digest: "dedicated-app-digest", manifest_version: "orchestrator-slack-dedicated-v1", provision_state: "completed", installation_id: "installation-dedicated-1", version: sourceConnectionMode === "dedicated_disconnected" ? 4 : 3, state: sourceConnectionMode === "dedicated_disconnected" ? "disconnected" : "active", disconnected_at: sourceConnectionMode === "dedicated_disconnected" ? "2026-07-18T02:00:00Z" : null };
    const sourceConnections = sourceConnectionMode === "shared" ? [sharedConnection] : [dedicatedConnection];
    let session = { session_id: "session-1", task_id: "task-1", task_item_id: null, step_id: "test", agent_id: "coder", state: "detached", pid: 42, writer_client_id: null as string | null, writer_actor: null as string | null, writer_lease_expires_at: null as string | null, state_version: 1 };
    const expertResources: Record<string, Array<{ kind: string; name: string; project_id: string; revision: string; source: string }>> = {
      workspaces: [{ kind: "Workspace", name: "default", project_id: "default", revision: "a".repeat(64), source: "resource_store" }],
      workflows: [{ kind: "Workflow", name: "delivery-loop", project_id: "default", revision: "b".repeat(64), source: "resource_store" }],
      agents: [{ kind: "Agent", name: "coder", project_id: "default", revision: "c".repeat(64), source: "resource_store" }],
      steptemplates: [{ kind: "StepTemplate", name: "implement", project_id: "default", revision: "d".repeat(64), source: "resource_store" }],
      executionprofiles: [{ kind: "ExecutionProfile", name: "sandbox", project_id: "default", revision: "e".repeat(64), source: "resource_store" }],
    };
    let expertManifest = "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: default\nspec:\n  workDir: workspace/default\n";
    const invoke = async (command: string, args: Record<string, unknown> = {}) => {
      processCalls.push({ command, args });
      if (command === "plugin:event|listen") { listeners.set(String(args.event), Number(args.handler)); return nextId++; }
      if (command === "plugin:event|unlisten") return null;
      if (command.startsWith("start_agent_session") || command.startsWith("stop_agent_session")) { sessionCalls.push({ command, args }); return null; }
      if (command.includes("notification")) return true;
      if (command === "connect") {
        queueMicrotask(() => { const handler = listeners.get("connection-state-changed"); if (handler) callbacks.get(handler)?.({ event: "connection-state-changed", id: 1, payload: { kind: "Connected" } }); });
        return null;
      }
      if (command === "probe_role") return roleName;
      if (command === "agent_list") return [];
      if (command === "resource_list") return { resources: expertResources[String(args.resourceType)] ?? [], next_cursor: null };
      if (command === "resource_describe") {
        const path = String(args.resource);
        const summary = Object.values(expertResources).flat().find((resource) => `${resource.kind.toLowerCase()}/${resource.name}` === path);
        return { content: path === "workspace/default" ? expertManifest : `kind: ${summary?.kind}\nmetadata:\n  name: ${summary?.name}\n`, format: "yaml", resource: summary ?? null };
      }
      if (command === "process_metric_record") return true;
      if (command === "process_metrics_get") {
        const metric = (name: string, value: number, sampleCount = 1, extra: Record<string, unknown> = {}) => ({
          name, labels: {}, sample_count: sampleCount, sum: value, min: value, max: value,
          value, numerator: null, denominator: null, histogram: {}, buckets: [], ...extra,
        });
        return {
          schema_version: 1, project_id: String(args.project_id),
          window_start: "2026-07-13T00:00:00Z", window_end: "2026-07-14T00:00:00Z",
          generated_at: new Date().toISOString(), coverage_start: "2026-07-01T00:00:00Z",
          partial: false, collection_enabled: true,
          metrics: [
            metric("attention_open_total", 4, 4), metric("attention_active", 2, 2),
            metric("attention_time_to_claim_seconds", 12, 2), metric("process_human_attention_seconds", 60, 2),
            metric("process_autonomous_completion_ratio", 0.75, 0, { numerator: 3, denominator: 4 }),
            metric("handoff_to_productive_action_seconds", 20), metric("resume_attempt_total", 2, 2, { labels: { mode: "restart_step", result: "succeeded" } }),
            metric("session_attachment_total", 3, 3, { labels: { mode: "writer", result: "succeeded" } }),
            metric("source_event_deduplicated_total", 2, 2, { labels: { provider: "slack" } }),
            metric("process_repeated_failure_rate", 0.25, 0, { numerator: 1, denominator: 4 }),
            metric("process_degenerate_loop_rate", 0.1, 0, { numerator: 1, denominator: 10 }),
            metric("timeline_projection_seconds", 0.08, 2), metric("timeline_response_bytes", 4096, 2),
            metric("stream_reconnect_total", 1),
          ],
          projector_health: [{ projector: "attention", project_id: "", cursor: "42", lag_count: 0, failure_count: 0, last_error_code: null, last_success_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:00:00Z" }],
        };
      }
      if (command === "attention_list") return { items: attentionRows, latest_change_id: 2 };
      if (["attention_claim", "attention_snooze", "attention_resolve"].includes(command)) {
        if (command === "attention_claim" && conflictAttentionClaim) {
          conflictAttentionClaim = false;
          attentionRows = attentionRows.map((item) => item.id === args.id
            ? { ...item, state: "claimed", assignee: "operator-b", version: 2 }
            : item);
          throw {
            category: "conflict",
            message: "provider token=must-not-render",
            request_id: "req-playwright-conflict-121",
          };
        }
        const nextState = command === "attention_claim" ? "claimed" : command === "attention_snooze" ? "snoozed" : "resolved";
        const updated = { ...attentionRows.find((item) => item.id === args.id)!, state: nextState, version: 2 };
        attentionRows = attentionRows.map((item) => item.id === args.id ? updated : item);
        return updated;
      }
      if (command === "task_list") return tasks;
      if (command === "task_info" && args.task_id === "task-non-code") return { id: "task-non-code", name: "Warehouse reply", status: "running", goal: "Prepare an inventory-backed Slack reply", total_items: 1, finished_items: 0, failed_items: 0, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", project_id: "project-1", workflow_id: "warehouse-assistant", workspace_kind: "task", items: [{ id: "item-task", qa_file_path: "__TASK__", item_kind: "task", status: "running", order_no: 1 }] };
      if (command === "task_info") return { id: "task-1", name: "Fix payment failure", status: taskStatus, goal: "Restore the failed payment test", total_items: 1, finished_items: 0, failed_items: taskStatus === "failed" ? 1 : 0, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", project_id: "project-1", workflow_id: "qa-loop", workspace_kind: "code_repo", items: [{ id: "item-1", qa_file_path: "tests/payment.rs", item_kind: "qa_file", status: taskStatus === "failed" ? "failed" : "pending", order_no: 1 }] };
      if (command === "task_timeline") return { entries: [{ id: "entry-1", task_id: "task-1", occurred_at: "2026-07-14T00:00:00Z", category: "failure", title: "Test failed", summary: "The payment fixture assertion failed", status: "failed", actor: null, step_id: "test", task_item_id: "item-1", command_run_id: "run-1", session_id: "session-1", checkpoint_id: "checkpoint-1", source_event_id: null, evidence: [{ kind: "test", label: "cargo test payment", uri: null, content_type: "text/plain", digest: null, redacted: false }], raw_event_ids: [1], projection_version: 1 }], next_cursor: null, has_more: false, snapshot_max_event_id: 1, projection_version: 1 };
      if (command === "agent_session_list") return [session];
      if (command === "agent_session_attach") {
        sessionCalls.push({ command, args });
        session = { ...session, state: "active", writer_client_id: String(args.client_id), writer_actor: "operator", writer_lease_expires_at: "2026-07-14T00:01:00Z", state_version: session.state_version + 1 };
        return { fencing_token: 7, lease_expires_at: session.writer_lease_expires_at };
      }
      if (command === "agent_session_heartbeat") { sessionCalls.push({ command, args }); return "2026-07-14T00:02:00Z"; }
      if (command === "agent_session_send_input") { sessionCalls.push({ command, args }); return String(args.text).length; }
      if (command === "agent_session_detach") {
        sessionCalls.push({ command, args });
        session = { ...session, state: "detached", writer_client_id: null, writer_actor: null, writer_lease_expires_at: null, state_version: session.state_version + 1 };
        return true;
      }
      if (command === "agent_session_close") { sessionCalls.push({ command, args }); session = { ...session, state: "draining", state_version: session.state_version + 1 }; return session; }
      if (command === "source_binding_list") return [];
      if (command === "source_event_list") return sourceEvents.filter((event) => !args.routing_state || event.routing_state === args.routing_state);
      if (command === "source_event_get") return sourceEvents.find((event) => event.id === args.id);
      if (command === "source_replay") return true;
      if (command === "source_connection_catalog_get") return sourceConnectionCatalog;
      if (command === "source_connection_list") return sourceConnections;
      if (command === "source_connection_connect") return { id: "intent-1", project_id: "default", provider: "slack", provisioning_mode: "managed_shared", status: "pending", connection_id: null, error_code: null, expires_at: "2026-07-18T01:00:00Z", authorize_url: "https://slack.com/oauth/v2/authorize?state=opaque", connection: null };
      if (command === "source_connection_dedicated_preview") return { id: "dedicated-1", project_id: "default", status: "awaiting_approval", manifest_version: "orchestrator-slack-dedicated-v1", manifest_digest: "a".repeat(64), diff: [{ field: "oauth_config.scopes.bot", change: "set", before: [], after: ["reactions:read"], permission_expansion: true }], app_id_digest: null, oauth_intent_id: null, authorize_url: null, error_code: null, expires_at: "2026-07-18T01:00:00Z" };
      if (command === "source_connection_dedicated_approve") return { id: "dedicated-1", project_id: "default", status: "oauth_pending", manifest_version: "orchestrator-slack-dedicated-v1", manifest_digest: "a".repeat(64), diff: [], app_id_digest: "b".repeat(64), oauth_intent_id: "intent-1", authorize_url: "https://slack.com/oauth/v2/authorize?state=dedicated", error_code: null, expires_at: "2026-07-18T01:00:00Z" };
      if (command === "source_connection_dedicated_get") return { id: "dedicated-1", project_id: "default", status: "attention", manifest_version: "orchestrator-slack-dedicated-v1", manifest_digest: "a".repeat(64), diff: [], app_id_digest: null, oauth_intent_id: null, authorize_url: null, error_code: "provisioning_session_lost", expires_at: "2026-07-18T01:00:00Z" };
      if (command === "source_connection_dedicated_abandon") return { id: "dedicated-1", project_id: "default", status: "abandoned", manifest_version: "orchestrator-slack-dedicated-v1", manifest_digest: "a".repeat(64), diff: [], app_id_digest: null, oauth_intent_id: null, authorize_url: null, error_code: "provisioning_abandoned", expires_at: "2026-07-18T01:00:00Z" };
      if (command === "source_connection_dedicated_upgrade_preview") return { lifecycle_id: "lifecycle-1", connection_id: "conn-dedicated-1", status: "awaiting_approval", manifest_version: "orchestrator-slack-dedicated-v1", manifest_digest: "c".repeat(64), permission_expansion: true, expires_at: "2026-07-18T01:00:00Z", oauth_intent_id: null, authorize_url: null, connection: dedicatedConnection, diff: [{ field: "oauth.scopes.bot", change: "add", before: ["reactions:read"], after: ["chat:write", "reactions:read"], permission_expansion: true }] };
      if (command === "source_connection_dedicated_upgrade_apply") return { lifecycle_id: "lifecycle-1", connection_id: "conn-dedicated-1", status: "reauthorization_required", manifest_version: "orchestrator-slack-dedicated-v1", manifest_digest: "c".repeat(64), permission_expansion: true, expires_at: "2026-07-18T01:00:00Z", oauth_intent_id: "intent-upgrade", authorize_url: "https://slack.com/oauth/v2/authorize?state=upgrade", connection: { ...dedicatedConnection, state: "suspended", version: 5, provision_state: "reauthorization_required" }, diff: [] };
      if (command === "source_connection_migrate_to_shared") return { id: "intent-migrate", project_id: "default", provider: "slack", provisioning_mode: "managed_shared", status: "pending", connection_id: null, error_code: null, expires_at: "2026-07-18T01:00:00Z", authorize_url: "https://slack.com/oauth/v2/authorize?state=migrate", connection: null };
      if (command === "source_connection_dedicated_delete") return { ...dedicatedConnection, version: 5, provision_state: "app_deleted" };
      if (command === "source_connection_intent_get") return { id: "intent-1", project_id: "default", provider: "slack", provisioning_mode: "managed_shared", status: "pending", connection_id: null, error_code: null, expires_at: "2026-07-18T01:00:00Z", authorize_url: "https://slack.com/oauth/v2/authorize?state=opaque", connection: null };
      if (command === "source_connection_transfer") return { ...sourceConnections[0], owner_daemon_id: String(args.target_daemon_id), state: "suspended", version: 2, last_error_code: "owner_transfer_pending_acceptance" };
      if (["open_source_connection_oauth", "start_source_connection_watch", "stop_source_connection_watch", "source_connection_cancel"].includes(command)) return true;
      if (command === "source_automation_catalog_get") return automationCatalog;
      if (command === "manifest_validate") return { valid: true, errors: [], message: "valid", diagnostics: [] };
      if (command === "source_task_template_preview") return { name: String(args.name), skill_name: "docs", skill_invocation: "$docs", skill_args: [], goal: `$docs: inspect ${String(args.message_url)}`, workflow: "analysis", workspace: "main", start: false, initial_vars: {}, revision: "draft", warnings: ["sample_url_not_verified_against_installation"] };
      if (command === "source_task_binding_simulate") return { status: "matched", reason: "binding_matched", resolved_role: "operator", binding_id: "analyze-badge", template_ref: "analyze", binding_revision: "draft" };
      if (command === "resource_apply") {
        if (String(args.content).includes("kind: Workspace")) {
          expertManifest = String(args.content);
          expertResources.workspaces[0] = { ...expertResources.workspaces[0], revision: "f".repeat(64) };
          return { message: "updated workspace default", request_id: "req-expert-resource-119" };
        }
        return automationRoute;
      }
      if (["source_task_binding_suspend", "source_task_binding_resume", "source_automation_replay", "source_automation_ignore", "start_source_automation_watch", "stop_source_automation_watch"].includes(command)) return automationRoute;
      if (command === "source_automation_list") return { routes: [automationRoute].filter((route) => !args.route_state || route.status === args.route_state).filter((route) => !args.binding_name || route.binding_name === args.binding_name).filter((route) => !args.task_id || route.task_id === args.task_id), next_page_token: null };
      if (command === "source_automation_status_get") return { project_id: "default", backlog_count: 1, oldest_age_seconds: 30, active_leases: 0, retrying_count: 0, needs_attention_count: 1, failure_categories: [["internal", 1]] };
      if (command === "source_automation_get") return { route: automationRoute, attempts: [{ attempt_no: 1, generation: 1, started_at: automationRoute.created_at, completed_at: automationRoute.updated_at, result_state: automationRoute.status, error_code: automationRoute.error_code, error_category: automationRoute.error_category, retry_after_seconds: null }] };
      if (command === "handoff_generate") return {
        id: "handoff-1", task_id: "task-1", source_event_cursor: 42, projection_version: 1,
        briefing: { goal: "Restore the failed payment test", current_state: { status: "failed" }, last_success: null, failure: { step: "test" }, test_evidence: [], changed_files: ["tests/payment.rs"], constraints: [], decisions: [], open_questions: [], recommendations: ["Retry from the verified test boundary"] },
        content_hash: "abcdef1234567890", state_version: "state-1", created_at: "2026-07-14T00:00:00Z",
      };
      if (command === "resume_boundary_list") return [{ id: "boundary-1", task_id: "task-1", cycle: 1, step_id: "test", task_item_id: "item-1", provider_session_available: false, side_effect_class: "workspace_only", replay_safe: true, reason: "Failed step can be replayed", state_version: "state-1" }];
      if (command === "resume_plan") return { id: "plan-1", task_id: "task-1", boundary: null, mode: String(args.mode), expected_state_version: "state-1", consequence: { repeated_steps: ["test"], workspace_rollback: false }, elevated_confirmation_required: false, expires_at: "2026-07-14T01:00:00Z", status: "review_required" };
      if (command === "resume_execute") {
        if (updateStatusOnResume) taskStatus = "running";
        return { execution_id: "execution-1", plan_id: "plan-1", accepted: true, status: "succeeded", child_task_id: "task-child" };
      }
      return null;
    };
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        invoke,
        transformCallback: (callback: (payload: unknown) => void) => { const id = nextId++; callbacks.set(id, callback); return id; },
        unregisterCallback: (id: number) => callbacks.delete(id),
        convertFileSrc: (path: string) => path,
      },
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => undefined },
      __SESSION_TEST__: {
        calls: sessionCalls,
        emit: (event: string, payload: unknown) => {
          const handler = listeners.get(event);
          if (handler) callbacks.get(handler)?.({ event, id: 1, payload });
        },
      },
      __PROCESS_TEST__: {
        calls: processCalls,
        updateStatusOnResume: () => { updateStatusOnResume = true; },
      },
    });
  }, {
    roleName: role,
    sourceConnectionMode: connectionMode,
    shouldConflictAttentionClaim: attentionClaimConflict,
  });
}

test("Attention is the default and opens the semantic failed-process workspace", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Attention", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Approval required" })).toBeVisible();
  await expect(page.getByText("Autonomous background task")).toHaveCount(0);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/processes\/task-1/);
  await expect(page.getByRole("heading", { name: "Fix payment failure" })).toBeVisible();
  await expect(page.getByText("cargo test payment", { exact: true })).toBeVisible();
});

test("keyboard selection is stable and read-only mutations are disabled", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/");
  const listbox = page.getByRole("listbox");
  await expect(listbox).toHaveAttribute("aria-activedescendant", "attention-attention-1");
  await page.keyboard.press("ArrowDown");
  await expect(listbox).toHaveAttribute("aria-activedescendant", "attention-attention-2");
  await expect(page.getByRole("button", { name: "Resolve" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Retry safely" })).toHaveCount(0);
});

test("failed process uses reviewed resume and never routes the primary action to orphan repair", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/#/tasks/task-1");
  const panelResume = page.getByRole("button", { name: "Preview resume" });
  await panelResume.click();
  let dialog = page.getByRole("dialog", { name: "Resume consequence preview" });
  await expect(dialog.getByRole("button", { name: "Close resume dialog" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(panelResume).toBeFocused();

  const review = page.getByRole("button", { name: "Review safe resume" });
  await review.click();
  dialog = page.getByRole("dialog", { name: "Resume consequence preview" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Create preview" }).click();
  await dialog.getByLabel("Operator reason").fill("Reviewed the failed test evidence");
  await dialog.getByRole("button", { name: "Execute reviewed plan" }).click();
  await expect(dialog.getByRole("status")).toContainText("succeeded");
  const commands = await page.evaluate(() => (window as any).__PROCESS_TEST__.calls.map((call: any) => call.command));
  expect(commands).toEqual(expect.arrayContaining(["resume_boundary_list", "resume_plan", "resume_execute"]));
  expect(commands).not.toContain("task_recover");
  await dialog.getByRole("button", { name: "Close resume dialog" }).click();
  await expect(review).toBeFocused();
});

test("Attention one-click safe resume auto-opens and restores a stable process control", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Review safe resume" }).click();

  await expect(page).toHaveURL(/#\/processes\/task-1$/);
  const dialog = page.getByRole("dialog", { name: "Resume consequence preview" });
  const close = dialog.getByRole("button", { name: "Close resume dialog" });
  const createPreview = dialog.getByRole("button", { name: "Create preview" });
  await expect(close).toBeFocused();
  expect((await new AxeBuilder({ page }).include(".resume-dialog").analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);

  await page.keyboard.press("Shift+Tab");
  await expect(createPreview).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(close).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);

  const resume = page.getByRole("button", { name: "Preview resume" });
  await expect(resume).toBeFocused();
  await page.getByRole("button", { name: "切换到深色模式" }).click();
  await page.getByRole("button", { name: /Reduce transparency/ }).click();
  await page.keyboard.press("Tab");
  await resume.focus();
  const focusStyle = await resume.evaluate((element) => {
    const style = getComputedStyle(element);
    return { width: style.outlineWidth, style: style.outlineStyle, color: style.outlineColor };
  });
  expect(Number.parseFloat(focusStyle.width)).toBeGreaterThanOrEqual(2);
  expect(focusStyle.style).not.toBe("none");
  expect(focusStyle.color).not.toBe("transparent");
});

test("successful resume refresh falls back safely when initiating controls disappear", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/#/tasks/task-1");
  await page.evaluate(() => (window as any).__PROCESS_TEST__.updateStatusOnResume());
  await page.getByRole("button", { name: "Review safe resume" }).click();

  const dialog = page.getByRole("dialog", { name: "Resume consequence preview" });
  await dialog.getByRole("button", { name: "Create preview" }).click();
  await dialog.getByLabel("Operator reason").fill("Resume after verified failure");
  await dialog.getByRole("button", { name: "Execute reviewed plan" }).click();
  await expect(dialog.getByRole("status")).toContainText("succeeded");
  await expect(page.getByRole("button", { name: "Review safe resume" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Preview resume" })).toHaveCount(0);

  await dialog.getByRole("button", { name: "Close resume dialog" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Handoff & safe resume" })).toBeFocused();
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(false);
});

test("confirmation dialogs trap focus, close with Escape, and restore focus", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  const resolve = page.getByRole("button", { name: "Resolve" });
  await resolve.click();
  const dialog = page.getByRole("dialog", { name: "Confirm resolution" });
  const cancel = dialog.getByRole("button", { name: "取消" });
  const confirm = dialog.getByRole("button", { name: "Resolve item" });
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(confirm).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(resolve).toBeFocused();
});

test("Attention and failed-process workspace have no serious axe violations", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
  await page.goto("/#/tasks/task-1");
  await expect(page.getByRole("heading", { name: "Fix payment failure" })).toBeVisible();
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("reduced-motion preference suppresses UI transitions", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await installTauriMock(page);
  await page.goto("/");
  const duration = await page.getByRole("button", { name: "Refresh snapshot" }).evaluate((element) => getComputedStyle(element).transitionDuration);
  expect(Number.parseFloat(duration)).toBeLessThanOrEqual(0.00001);
});

test("read-only users reach all resource catalogs and open details with the keyboard", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/");
  await page.getByRole("navigation", { name: "主导航" }).getByRole("link", { name: /System/ }).click();
  await page.getByRole("button", { name: "Workflows & Resources" }).click();

  const workspace = page.getByRole("button", { name: "打开 Workspace default" });
  await expect(workspace).toBeVisible();
  await workspace.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Workspace/default" })).toBeFocused();
  await expect(page.getByRole("button", { name: "复制" })).toBeVisible();
  await expect(page.getByRole("button", { name: "编辑" })).toHaveCount(0);
  await page.getByRole("button", { name: "← 返回列表" }).click();
  await expect(page.getByRole("button", { name: "打开 Workspace default" })).toBeFocused();

  for (const [tab, row] of [
    ["Workflows", "打开 Workflow delivery-loop"],
    ["Agents", "打开 Agent coder"],
    ["Step Templates", "打开 StepTemplate implement"],
    ["Execution Profiles", "打开 ExecutionProfile sandbox"],
  ]) {
    await page.getByRole("tab", { name: tab, exact: true }).click();
    await expect(page.getByRole("button", { name: row })).toBeVisible();
  }
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("operator resource editing is reviewed, revision-fenced, audited, and focus-safe", async ({ page }) => {
  await installTauriMock(page, "operator");
  await page.goto("/#/system/resources");
  await page.getByRole("button", { name: "打开 Workspace default" }).click();
  await page.getByRole("button", { name: "编辑" }).click();
  const editor = page.getByLabel("资源 Manifest");
  await editor.fill("apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: default\nspec:\n  workDir: workspace/reviewed\n");
  const apply = page.getByRole("button", { name: "应用" });
  await apply.click();
  const dialog = page.getByRole("dialog", { name: "确认应用资源变更" });
  await expect(dialog.getByLabel("Audit reason")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(apply).toBeFocused();

  await apply.click();
  await dialog.getByLabel("Audit reason").fill("reviewed resource workspace change");
  await dialog.getByRole("button", { name: "应用已审查变更" }).click();
  await expect(page.getByRole("status")).toContainText("req-expert-resource-119");
  await expect(page.getByText(/workspace\/reviewed/)).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) =>
    call.command === "resource_apply" && String(call.args.content).includes("workspace/reviewed"),
  )?.args)).toMatchObject({
    project_id: "default",
    expected_revision: "a".repeat(64),
    require_absent: false,
    reason: "reviewed resource workspace change",
  });
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("Slack connections presents explicit provisioning choices and starts resumable OAuth", async ({ page }) => {
  await installTauriMock(page, "admin");
  await page.goto("/#/sources/connections");
  await expect(page.getByRole("heading", { name: "Slack connections" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Instant — Official Orchestrator App" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Dedicated — Private workspace app" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Existing app — Manual credentials" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Product Slack active" })).toBeVisible();
  await page.getByLabel("Connection label", { exact: true }).fill("Engineering Slack");
  await page.getByRole("button", { name: "Connect workspace" }).click();
  await expect(page.getByText("Waiting for Slack consent")).toBeVisible();
  const calls = await page.evaluate(() => (window as any).__PROCESS_TEST__.calls);
  expect(calls).toEqual(expect.arrayContaining([
    expect.objectContaining({ command: "source_connection_connect", args: expect.objectContaining({ project_id: "default", display_label: "Engineering Slack" }) }),
    expect.objectContaining({ command: "open_source_connection_oauth", args: { authorize_url: "https://slack.com/oauth/v2/authorize?state=opaque" } }),
  ]));
  await page.getByRole("button", { name: "Transfer" }).click();
  const dialog = page.getByRole("dialog", { name: "Transfer Product Slack" });
  await dialog.getByLabel("Target daemon ID").fill("daemon-2");
  await dialog.getByLabel("Audit reason").fill("move to replacement daemon");
  await dialog.getByRole("button", { name: "Transfer ownership" }).click();
  await expect.poll(async () => (await page.evaluate(() => (window as any).__PROCESS_TEST__.calls)).some((call: any) => call.command === "source_connection_transfer" && call.args.target_daemon_id === "daemon-2" && call.args.expected_version === 1)).toBe(true);
});

test("Slack connections clears dedicated token and requires manifest approval", async ({ page }) => {
  await installTauriMock(page, "admin");
  await page.goto("/#/sources/connections");
  const token = page.getByLabel("One-time Configuration Token");
  await expect(token).toHaveAttribute("type", "password");
  await expect(token).toHaveAttribute("autocomplete", "off");
  await page.getByLabel("Dedicated connection label").fill("Private Engineering");
  await token.fill("xoxe-browser-only-marker");
  await page.getByRole("button", { name: "Validate manifest" }).click();
  await expect(page.getByRole("heading", { name: "Dedicated app provisioning review" })).toBeVisible();
  await expect(token).toHaveValue("");
  await expect(page.getByText("set · permission expansion")).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("orchestrator.dedicatedSlackProvisioning.v1"))).toBe(JSON.stringify({ id: "dedicated-1", project: "default" }));
  expect(await page.locator("body").textContent()).not.toContain("xoxe-browser-only-marker");
  await page.getByRole("button", { name: "Approve and create app" }).click();
  const dialog = page.getByRole("dialog", { name: "Create dedicated Slack App" });
  await expect(dialog.getByRole("button", { name: "Create app" })).toBeDisabled();
  await dialog.getByLabel("Audit reason").fill("isolate regulated workspace");
  await dialog.getByRole("button", { name: "Create app" }).click();
  await expect(page.getByText("Waiting for Slack consent")).toBeVisible();
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("Slack dedicated provisioning binds an explicit shared migration target", async ({ page }) => {
  await installTauriMock(page, "admin");
  await page.goto("/#/sources/connections");
  await page.getByLabel("Migration source (optional)").selectOption("conn-installation-1");
  await page.getByLabel("One-time Configuration Token").fill("xoxe-migration-marker");
  await page.getByRole("button", { name: "Validate manifest" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) => call.command === "source_connection_dedicated_preview")?.args.target_connection_id)).toBe("conn-installation-1");
  expect(await page.locator("body").textContent()).not.toContain("xoxe-migration-marker");
});

test("Slack dedicated App upgrade uses a fresh token, semantic review, and version fence", async ({ page }) => {
  await installTauriMock(page, "admin", "dedicated");
  await page.goto("/#/sources/connections");
  await page.getByRole("button", { name: "Review manifest upgrade" }).click();
  const token = page.getByLabel("Fresh Configuration Token");
  await token.fill("xoxe-upgrade-marker");
  await page.getByRole("button", { name: "Validate upgrade" }).click();
  await expect(token).toHaveValue("");
  await expect(page.getByText("oauth.scopes.bot")).toBeVisible();
  await expect(page.getByText("add · permission expansion")).toBeVisible();
  await page.getByRole("button", { name: "Approve manifest upgrade" }).click();
  const dialog = page.getByRole("dialog", { name: "Apply dedicated Slack App manifest" });
  await dialog.getByLabel("Audit reason").fill("approve reviewed permission expansion");
  await dialog.getByRole("button", { name: "Apply manifest" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) => call.command === "source_connection_dedicated_upgrade_apply")?.args)).toMatchObject({ lifecycle_id: "lifecycle-1", expected_version: 3 });
  expect(await page.locator("body").textContent()).not.toContain("xoxe-upgrade-marker");
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("Slack dedicated to official migration is a reviewed OAuth handoff", async ({ page }) => {
  await installTauriMock(page, "admin", "dedicated");
  await page.goto("/#/sources/connections");
  await page.getByRole("button", { name: "Migrate to Official App" }).click();
  const dialog = page.getByRole("dialog", { name: "Migrate Private Product Slack to Official App" });
  await dialog.getByLabel("Audit reason").fill("return sandbox to the official App");
  await dialog.getByRole("button", { name: "Continue migration" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) => call.command === "source_connection_migrate_to_shared")?.args)).toMatchObject({ id: "conn-dedicated-1", expected_version: 3 });
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.some((call: any) => call.command === "open_source_connection_oauth" && String(call.args.authorize_url).includes("state=migrate")))).toBe(true);
});

test("Slack dedicated App deletion is separate, typed, audited, and narrow-safe", async ({ page }) => {
  await installTauriMock(page, "admin", "dedicated_disconnected");
  await page.setViewportSize({ width: 640, height: 820 });
  await page.goto("/#/sources/connections");
  await page.getByRole("button", { name: "Delete workspace App" }).click();
  await page.getByLabel("Fresh Configuration Token").fill("xoxe-delete-marker");
  await page.getByLabel("Exact Slack App ID").fill("A123DELETE");
  await page.getByLabel("Audit reason").fill("retire controlled sandbox App");
  await page.getByRole("button", { name: "Permanently delete App" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) => call.command === "source_connection_dedicated_delete")?.args)).toMatchObject({ typed_app_id: "A123DELETE", expected_version: 4, reason: "retire controlled sandbox App" });
  expect(await page.locator("body").textContent()).not.toContain("xoxe-delete-marker");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("Slack connections keeps credentials and mutations hidden from read-only users", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/#/sources/connections");
  await expect(page.getByText("Product Slack")).toBeVisible();
  await expect(page.getByText(/generation 1/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Connect workspace" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Reauthorize" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Transfer" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Disconnect" })).toHaveCount(0);
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("narrow layout exposes the menu and reduced-transparency fallback", async ({ page }) => {
  await installTauriMock(page);
  await page.setViewportSize({ width: 640, height: 820 });
  await page.goto("/");
  await page.getByRole("button", { name: /Menu/ }).click();
  await page.getByRole("button", { name: /Reduce transparency/ }).click();
  await expect(page.locator("html")).toHaveAttribute("data-transparency", "reduced");
  await page.getByRole("link", { name: /Sessions/ }).click();
  await expect(page.getByRole("heading", { name: "Sessions" })).toBeVisible();
});

test("read-only Operations renders process health, switches windows, and remains accessible", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/#/system/operations");
  await expect(page.getByRole("heading", { name: "Operations" })).toBeVisible();
  await expect(page.getByText("75%")).toBeVisible();
  await expect(page.getByText("Fresh snapshot")).toBeVisible();
  await expect(page.getByRole("cell", { name: "attention" })).toBeVisible();
  await page.getByRole("button", { name: "7 days" }).click();
  await expect.poll(() => page.evaluate(() => {
    const calls = (window as any).__PROCESS_TEST__.calls.filter((call: any) => call.command === "process_metrics_get");
    return calls.at(-1)?.args.window;
  })).toBe("7d");
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("session inspector commits offsets, controls one writer, and links to its process", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/#/sessions");
  await page.getByRole("listitem").click();
  await expect(page.getByRole("heading", { name: "Session inspector" })).toBeVisible();

  await page.evaluate(() => (window as any).__SESSION_TEST__.emit("agent-session-output-session-1", {
    offset: 0, next_offset: 5, text: "hello", eof: false, redacted: false,
  }));
  await page.evaluate(() => (window as any).__SESSION_TEST__.emit("agent-session-output-session-1", {
    offset: 0, next_offset: 5, text: "duplicate", eof: false, redacted: false,
  }));
  await expect(page.getByRole("log")).toHaveText("hello");

  await page.evaluate(() => (window as any).__SESSION_TEST__.emit("stream-error-agent-session-session-1", "disconnected"));
  await expect.poll(() => page.evaluate(() => {
    const starts = (window as any).__SESSION_TEST__.calls.filter((call: any) => call.command === "start_agent_session_read");
    return starts.at(-1)?.args.offset;
  })).toBe(5);

  await page.getByRole("button", { name: "Request control" }).click();
  await page.getByLabel("Session input").fill("hello-agent");
  await page.getByRole("button", { name: "Send" }).click();
  await page.getByRole("button", { name: "Release control" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__SESSION_TEST__.calls.map((call: any) => call.command)))
    .toEqual(expect.arrayContaining(["agent_session_attach", "agent_session_send_input", "agent_session_detach"]));

  await page.getByRole("button", { name: "Open linked process" }).click();
  await expect(page).toHaveURL(/#\/processes\/task-1/);
});

test("read-only session inspector has no focusable mutation controls", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/#/sessions");
  await page.getByRole("listitem").click();
  await expect(page.getByText(/Read-only access/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Request control" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Close session" })).toHaveCount(0);
  await expect(page.getByLabel("Session input")).toHaveCount(0);
});

test("Processes prioritizes active and failed work while preserving keyboard reachability", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/#/tasks");
  await expect(page.getByRole("heading", { name: "进度观察" })).toBeVisible();
  const processCards = page.locator('[role="button"][aria-label^="任务:"]');
  await expect(processCards).toHaveCount(3);
  await expect(processCards.nth(0)).toHaveAccessibleName("任务: Active implementation");
  await expect(processCards.nth(1)).toHaveAccessibleName("任务: Fix payment failure");
  await expect(processCards.nth(2)).toHaveAccessibleName("任务: Completed documentation");
  await processCards.nth(1).focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/processes\/task-1/);
});

// FR-166 renamed the canonical hash to #/tasks. This one case deliberately stays on
// the pre-rename spelling so that the compatibility alias is exercised through a real
// browser, not only through parseConsoleRoute in a unit test.
test("non-code process uses task semantics in the console", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/#/processes/task-non-code");
  await expect(page.getByRole("heading", { name: "Warehouse reply" })).toBeVisible();
  await expect(page.getByText("Workspace type").locator("..")).toContainText("Task");
  await expect(page.getByText("__TASK__")).toHaveCount(0);
  await page.getByRole("button", { name: "Expert off" }).click();
  await expect(page.getByLabel("Expert process details").getByText("Task", { exact: true })).toBeVisible();
});

test("Attention mutations use guarded commands and resolved work leaves the open queue", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Claim" }).click();
  await expect(page.getByText("claim succeeded for Approval required")).toBeAttached();
  await page.getByRole("button", { name: "Resolve" }).click();
  await page.getByRole("dialog", { name: "Confirm resolution" }).getByRole("button", { name: "Resolve item" }).click();
  await expect(page.getByRole("heading", { name: "Approval required" })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Choose recovery" })).toBeVisible();
  const mutationCalls = await page.evaluate(() => (window as any).__PROCESS_TEST__.calls
    .filter((call: any) => call.command.startsWith("attention_"))
    .map((call: any) => ({ command: call.command, args: call.args })));
  expect(mutationCalls.map((call: any) => call.command)).toEqual(expect.arrayContaining(["attention_claim", "attention_resolve"]));
  expect(mutationCalls.find((call: any) => call.command === "attention_claim").args.idempotency_key).toBeTruthy();
});

test("Attention conflict preserves the error while reconciling authoritative state and focus", async ({ page }) => {
  await installTauriMock(page, "operator", "shared", true);
  await page.goto("/");
  const claim = page.getByRole("button", { name: "Claim" });
  await claim.focus();
  await claim.click();

  const alert = page.getByRole("alert");
  await expect(alert).toContainText("Claim failed for Approval required");
  await expect(alert).toContainText("latest daemon state has been restored");
  await expect(alert).toContainText("req-playwright-conflict-121");
  await expect(alert).not.toContainText("must-not-render");
  await expect(page.getByText("operator-b")).toBeVisible();
  await expect(page.getByRole("button", { name: "Claim", exact: true })).toHaveCount(0);
  await expect(page.getByRole("listbox", { name: "Attention queue" })).toBeFocused();
  expect((await new AxeBuilder({ page }).include(".attention-error-panel").analyze()).violations
    .filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);

  const calls = await page.evaluate(() => (window as any).__PROCESS_TEST__.calls);
  expect(calls.filter((call: any) => call.command === "attention_claim")).toHaveLength(1);
  expect(calls.filter((call: any) => call.command === "attention_list").length).toBeGreaterThanOrEqual(2);
  expect(calls.some((call: any) => call.command === "process_metric_record"
    && call.args.metric_name === "attention_mutation_total"
    && call.args.dimensions.error_category === "conflict")).toBe(true);
  expect(calls.some((call: any) => call.command === "process_metric_record"
    && call.args.metric_name === "attention_reconciliation_total"
    && call.args.dimensions.result === "confirmed")).toBe(true);
});

test("Sources supports routing filters, process correlation, and admin-only replay", async ({ page }) => {
  await installTauriMock(page, "admin");
  await page.goto("/#/sources/events");
  await expect(page.getByRole("heading", { name: "Sources" })).toBeVisible();
  await expect(page.getByRole("listitem")).toHaveCount(2);
  await page.getByRole("combobox").selectOption("needs_attention");
  await expect(page.getByRole("listitem")).toHaveCount(1);
  await page.getByRole("button", { name: "重新路由" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.some((call: any) => call.command === "source_replay" && call.args.id === "source-1"))).toBe(true);
  await page.getByRole("button", { name: "打开进程" }).click();
  await expect(page).toHaveURL(/#\/processes\/task-1/);
  await expect(page.getByRole("heading", { name: "Fix payment failure" })).toBeVisible();
});

test("read-only Sources exposes correlation without replay controls", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/#/sources/events");
  await expect(page.getByRole("button", { name: "打开进程" })).toHaveCount(2);
  await expect(page.getByRole("button", { name: "重新路由" })).toHaveCount(0);
});

test("operator creates and previews a daemon-rendered task template with an audited CAS apply", async ({ page }) => {
  await installTauriMock(page, "operator");
  await page.goto("/#/sources/automations/templates");
  await expect(page.getByRole("heading", { name: "Slack reaction automations" })).toBeVisible();
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByLabel("Name", { exact: true }).fill("docs-badge-template");
  await page.getByLabel("Skill name").fill("docs");
  await page.getByLabel("Skill invocation").fill("$docs");
  await page.getByLabel("Workflow").selectOption("analysis");
  await page.getByLabel("Workspace").selectOption("main");
  await page.getByRole("button", { name: "Render preview" }).click();
  await expect(page.getByText(/\$docs: inspect https:\/\/example\.slack\.com/)).toBeVisible();
  await expect(page.getByText(/no task created/)).toBeVisible();
  await page.getByRole("button", { name: "Review and save" }).click();
  const dialog = page.getByRole("dialog", { name: "Apply task template" });
  await dialog.getByLabel("Audit reason").fill("Create reviewed documentation automation");
  await dialog.getByRole("button", { name: "Apply template" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) => call.command === "resource_apply")?.args)).toMatchObject({ require_absent: true, expected_revision: null, reason: "Create reviewed documentation automation" });
});

test("operator simulates trusted badge matching and suspends with a revision", async ({ page }) => {
  await installTauriMock(page, "operator");
  await page.goto("/#/sources/automations/bindings/analyze-badge");
  await page.getByRole("button", { name: "Simulate badge" }).click();
  await expect(page.getByText("binding_matched")).toBeVisible();
  await expect(page.getByText(/no mutation or network call/)).toBeVisible();
  await page.getByRole("button", { name: "Suspend binding" }).click();
  const dialog = page.getByRole("dialog", { name: "Suspend badge binding" });
  await dialog.getByLabel("Audit reason").fill("Pause while investigating source noise");
  await dialog.getByRole("button", { name: "Suspend" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) => call.command === "source_task_binding_suspend")?.args)).toMatchObject({ name: "analyze-badge", expected_revision: "binding-revision" });
});

test("route diagnosis deep-links provenance and replays only after reviewed consequences", async ({ page }) => {
  await installTauriMock(page, "operator");
  await page.goto("/#/sources/automations/routes/route-1");
  await expect(page.getByRole("complementary", { name: "Route detail" }).locator("p.attention-error")).toContainText("task_create_failed");
  await expect(page.getByRole("button", { name: "Open Attention" })).toBeVisible();
  await page.getByRole("button", { name: "Replay" }).click();
  const dialog = page.getByRole("dialog", { name: "Replay automation route" });
  await expect(dialog.getByRole("button", { name: "Replay route" })).toBeDisabled();
  await dialog.getByLabel("Audit reason").fill("Dependency recovered; replay pinned evidence");
  await dialog.getByLabel(/Adopt the current/).check();
  await dialog.getByRole("button", { name: "Replay route" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__PROCESS_TEST__.calls.find((call: any) => call.command === "source_automation_replay")?.args)).toMatchObject({ route_id: "route-1", expected_version: 4, adopt_current_config: true });
  await page.getByRole("button", { name: "Open Attention" }).click();
  await expect(page).toHaveURL(/#\/attention\/attention-1/);
});

test("read-only automation UI is accessible, narrow, and leaks no source secrets into DOM or storage", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.setViewportSize({ width: 640, height: 820 });
  await page.goto("/#/sources/automations/bindings/analyze-badge");
  await expect(page.getByText(/Read-only access/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Review and save" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Suspend binding" })).toHaveCount(0);
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
  const snapshot = await page.evaluate(() => ({ text: document.body.textContent, html: document.body.innerHTML, local: JSON.stringify(localStorage), session: JSON.stringify(sessionStorage) }));
  expect(JSON.stringify(snapshot)).not.toMatch(/signing.secret|bot.token|normalized_json|private message body/i);
});

test("global shortcuts, visible navigation, theme, and handoff remain integrated", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  await expect(page.getByRole("navigation", { name: "主导航" }).getByRole("link")).toHaveCount(5);
  await page.keyboard.press("Control+5");
  await expect(page).toHaveURL(/#\/system/);
  await expect(page.getByRole("heading", { name: "System" })).toBeVisible();
  await page.keyboard.press("Control+3");
  await expect(page.getByRole("heading", { name: "Sessions" })).toBeVisible();
  await page.getByRole("button", { name: "切换到深色模式" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

  await page.goto("/#/tasks/task-1");
  await page.getByRole("button", { name: "Generate handoff" }).click();
  await expect(page.getByText("Changed files:")).toBeVisible();
  await expect(page.getByText("tests/payment.rs")).toBeVisible();
  await expect(page.getByText("Snapshot abcdef123456")).toBeVisible();
});
