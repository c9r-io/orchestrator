import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RoleContext, hasAccess } from "../../hooks/useRole";
import type { Role, SourceAutomationCatalog, SourceAutomationRoute } from "../../lib/types";
import SourceAutomations from "./SourceAutomations";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => vi.fn()) }));

const catalog: SourceAutomationCatalog = {
  project_id: "default",
  templates: [{ name: "analyze", revision: "template-revision", skill_name: "analyze", skill_invocation: "$analyze", skill_args: ["--safe"], workflow: "analysis", workspace: "main", start: true, initial_vars: {}, goal_template: "{skill_invocation} {source_message_url}", allowed_variables: ["skill_invocation", "source_message_url"] }],
  bindings: [{ name: "analyze-badge", revision: "binding-revision", trigger_ref: "slack-main", installation_id: "T123", reaction: "agent-analyze", channels: ["C123"], all_channels: false, template_ref: "analyze", allowed_actor_roles: ["operator"], suspended: false }],
  installations: [{ trigger_name: "slack-main", installation_id: "T123", actor_ids: ["U123"], actor_roles: ["operator"], suspended: false, reaction_routing: "bindings" }],
  workflows: ["analysis", "docs"], workspaces: ["main"],
};

const route: SourceAutomationRoute = {
  id: "route-1", project_id: "default", source_event_id: "event-1", provider: "slack", reaction: "agent-analyze",
  binding_name: "analyze-badge", binding_revision: "binding-revision", template_name: "analyze", template_hash: "template-revision",
  status: "needs_attention", error_code: "task_create_failed", error_category: "internal", task_id: "task-1", permalink: null,
  request_id: "request-1", generation: 1, version: 4, attempt_count: 1, max_attempts: 3, next_attempt_at: null,
  suspended_scope: null, created_at: "2026-07-18T00:00:00Z", updated_at: "2026-07-18T00:01:00Z", completed_at: null,
};

function renderView(role: Role, view: "templates" | "bindings" | "routes", resourceId?: string) {
  const onNavigate = vi.fn(); const onOpenTask = vi.fn();
  render(<RoleContext.Provider value={{ role, canAccess: (required) => hasAccess(role, required) }}><SourceAutomations view={view} resourceId={resourceId} onNavigate={onNavigate} onOpenTask={onOpenTask} /></RoleContext.Provider>);
  return { onNavigate, onOpenTask };
}

describe("SourceAutomations", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "source_automation_catalog_get") return catalog;
      if (command === "manifest_validate") return { valid: true, errors: [], message: "valid", diagnostics: [] };
      if (command === "source_task_template_preview") return { name: "analyze", skill_name: "analyze", skill_invocation: "$analyze", skill_args: ["--safe"], goal: "$analyze https://example.slack.com/archives/C123/p1234567890000100", workflow: "analysis", workspace: "main", start: true, initial_vars: {}, revision: "draft", warnings: ["sample_url_not_verified_against_installation"] };
      if (command === "source_task_binding_simulate") return { status: "matched", reason: "binding_matched", resolved_role: "operator", binding_id: "analyze-badge", template_ref: "analyze", binding_revision: "draft" };
      if (command === "source_automation_list") return { routes: [route], next_page_token: null };
      if (command === "source_automation_status_get") return { project_id: "default", backlog_count: 1, oldest_age_seconds: 30, active_leases: 0, retrying_count: 0, needs_attention_count: 1, failure_categories: [["internal", 1]] };
      if (command === "attention_list") return { items: [{ id: "attention-1", source_route_id: "route-1" }], latest_change_id: 1 };
      if (command === "source_automation_get") return { route, attempts: [{ attempt_no: 1, generation: 1, started_at: route.created_at, completed_at: route.updated_at, result_state: "needs_attention", error_code: route.error_code, error_category: route.error_category, retry_after_seconds: null }] };
      if (["start_source_automation_watch", "stop_source_automation_watch", "resource_apply", "source_task_binding_suspend", "source_automation_replay"].includes(command)) return null;
      throw new Error(`unexpected ${command}`);
    });
  });
  afterEach(cleanup);

  it("uses daemon validation and preview, then applies with revision and audit reason", async () => {
    renderView("operator", "templates", "analyze");
    fireEvent.click(await screen.findByRole("button", { name: "Render preview" }));
    expect(await screen.findByText(/\$analyze https:\/\/example\.slack\.com/)).toBeInTheDocument();
    expect(screen.getByText(/no task created/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Review and save" }));
    const reason = await screen.findByLabelText("Audit reason"); fireEvent.change(reason, { target: { value: "Update reviewed skill parameters" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply template" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("resource_apply", expect.objectContaining({ expected_revision: "template-revision", require_absent: false, reason: "Update reviewed skill parameters" })));
  });

  it("projects structured daemon diagnostics onto the responsible field", async () => {
    vi.mocked(invoke).mockImplementationOnce(async () => catalog).mockImplementationOnce(async () => ({ valid: false, errors: ["unknown variable"], message: "invalid", diagnostics: [{ source: "manifest_validate", rule: "config_build_failed", severity: "error", passed: false, blocking: true, message: "unknown variable source_body", scope: "spec.goalTemplate", suggested_fix: null }] }));
    renderView("operator", "templates", "analyze");
    fireEvent.click(await screen.findByRole("button", { name: "Render preview" }));
    expect(await screen.findByText("unknown variable source_body")).toBeInTheDocument();
    expect(screen.getByDisplayValue("{skill_invocation} {source_message_url}").closest("label")).toContainElement(screen.getByText("unknown variable source_body"));
  });

  it("simulates exact trusted-role matching and offers reversible suspension", async () => {
    renderView("operator", "bindings", "analyze-badge");
    fireEvent.click(await screen.findByRole("button", { name: "Simulate badge" }));
    expect(await screen.findByText("binding_matched")).toBeInTheDocument();
    expect(screen.getByText(/no mutation or network call/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Suspend binding" }));
    fireEvent.change(await screen.findByLabelText("Audit reason"), { target: { value: "Pause noisy badge while triaging" } });
    fireEvent.click(screen.getByRole("button", { name: "Suspend" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_task_binding_suspend", expect.objectContaining({ expected_revision: "binding-revision" })));
  });

  it("hides all mutation controls for read-only users", async () => {
    renderView("read_only", "bindings", "analyze-badge");
    expect(await screen.findByText(/Read-only access/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Review and save" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Suspend binding" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Simulate badge" })).toBeInTheDocument();
  });

  it("replays a pinned route only after a reason and carries the expected version", async () => {
    renderView("operator", "routes", "route-1");
    fireEvent.click(await screen.findByRole("button", { name: "Replay" }));
    const confirm = screen.getByRole("button", { name: "Replay route" }); expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Audit reason"), { target: { value: "Retry after dependency recovery" } });
    fireEvent.click(screen.getByLabelText(/Adopt the current/)); fireEvent.click(confirm);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_automation_replay", expect.objectContaining({ route_id: "route-1", expected_version: 4, adopt_current_config: true })));
  });
});
