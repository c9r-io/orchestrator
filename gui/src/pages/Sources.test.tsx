import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Sources from "./Sources";
import { RoleContext, hasAccess } from "../hooks/useRole";
import type { Role, SourceEvent } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const events: SourceEvent[] = [
  { id: "source-1", project_id: "project-1", provider: "slack", installation_id: "workspace-demo",
    external_event_id: "evt-1", event_type: "message", conversation_id: "channel-1", thread_id: "thread-1",
    reaction_name: null, reaction_target_kind: null, reaction_target_id: null,
    occurred_at: "2026-07-14T00:00:00Z", received_at: "2026-07-14T00:00:01Z",
    routing_state: "needs_attention", routing_attempts: 1, routed_task_id: "task-1", last_error_code: "trigger_ambiguous",
    automation_route_id: null, automation_status: null, automation_binding_name: null,
    automation_template_name: null, automation_template_hash: null },
  { id: "source-2", project_id: "project-1", provider: "github", installation_id: "repo-demo",
    external_event_id: "evt-2", event_type: "pull_request", conversation_id: "pr-42", thread_id: null,
    reaction_name: null, reaction_target_kind: null, reaction_target_id: null,
    occurred_at: "2026-07-14T00:02:00Z", received_at: "2026-07-14T00:02:01Z",
    routing_state: "routed", routing_attempts: 1, routed_task_id: "task-2", last_error_code: null,
    automation_route_id: null, automation_status: null, automation_binding_name: null,
    automation_template_name: null, automation_template_hash: null },
  { id: "source-3", project_id: "project-1", provider: "slack", installation_id: "workspace-demo",
    external_event_id: "evt-3", event_type: "reaction_added", reaction_name: "agent_fix",
    reaction_target_kind: "message", reaction_target_id: "channel-1:1712345678.000100",
    conversation_id: "channel-1", thread_id: "1712345678.000100",
    occurred_at: "2026-07-14T00:03:00Z", received_at: "2026-07-14T00:03:01Z",
    routing_state: "routed", routing_attempts: 1, routed_task_id: "task-3", last_error_code: null,
    automation_route_id: "route-3", automation_status: "routed", automation_binding_name: "fix-binding",
    automation_template_name: "fix-template", automation_template_hash: "hash-3" },
];

function renderAs(role: Role, onOpenTask = vi.fn()) {
  return {
    onOpenTask,
    ...render(<RoleContext.Provider value={{ role, canAccess: (required) => hasAccess(role, required) }}>
      <Sources route={{ page: "sources", section: "events" }} onNavigate={vi.fn()} onOpenTask={onOpenTask} />
    </RoleContext.Provider>),
  };
}

describe("Sources", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "source_event_list") {
        const state = (args as { routing_state: string | null }).routing_state;
        return events.filter((event) => !state || event.routing_state === state);
      }
      if (command === "source_event_get") return events.find((event) => event.id === (args as { id: string }).id);
      if (command === "source_binding_list") return [
        { id: "binding-1", task_id: "task-1", provider: "slack", installation_id: "workspace-demo",
          conversation_id: "channel-1", thread_id: "thread-1", binding_type: "thread", created_at: "now" },
      ];
      if (command === "source_replay") return true;
      if (command === "source_automation_route_get") {
        return { id: "route-3", source_event_id: "source-3", reaction: "agent_fix",
          binding_name: "fix-binding", template_name: "fix-template", status: "completed",
          task_id: "task-3", permalink: "https://acme.slack.com/archives/channel-1/p1712345678000100" };
      }
      throw new Error(`unexpected ${command}`);
    });
  });
  afterEach(cleanup);

  it("keeps task correlation visible to read-only users without exposing replay", async () => {
    const { onOpenTask } = renderAs("read_only");
    expect(await screen.findAllByRole("listitem")).toHaveLength(3);
    expect(screen.queryByRole("button", { name: "重新路由" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "打开 Slack 消息" })).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("source_automation_route_get", expect.anything());
    fireEvent.click(screen.getAllByRole("button", { name: "打开任务" })[0]);
    expect(onOpenTask).toHaveBeenCalledWith("task-1");
  });

  it("filters authoritatively and lets admins replay only actionable routing failures", async () => {
    renderAs("admin");
    expect(await screen.findAllByRole("listitem")).toHaveLength(3);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "needs_attention" } });
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "重新路由" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_replay", { id: "source-1" }));
  });

  it("shows bounded automation provenance and an operator-only protected Slack link", async () => {
    renderAs("admin");
    const reactionType = await screen.findByText("reaction_added");
    const reactionCard = reactionType.closest("article");

    expect(reactionCard).toHaveTextContent(":agent_fix:");
    expect(reactionCard).toHaveTextContent("message / channel-1:1712345678.000100");
    expect(reactionCard).toHaveTextContent("fix-binding");
    expect(reactionCard).toHaveTextContent("fix-template");
    expect(reactionCard).not.toHaveTextContent("private body");
    const link = await screen.findByRole("link", { name: "打开 Slack 消息" });
    expect(link).toHaveAttribute("href", "https://acme.slack.com/archives/channel-1/p1712345678000100");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noreferrer");
  });

  it("opens a selected event deep link and returns to the bounded event collection", async () => {
    const onNavigate = vi.fn();
    const onOpenTask = vi.fn();
    render(<RoleContext.Provider value={{ role: "admin", canAccess: (required) => hasAccess("admin", required) }}>
      <Sources route={{ page: "sources", section: "events", resourceId: "source-3" }}
        onNavigate={onNavigate} onOpenTask={onOpenTask} />
    </RoleContext.Provider>);

    expect(await screen.findAllByRole("listitem")).toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("source_event_get", { id: "source-3" });
    fireEvent.click(screen.getByRole("button", { name: "Open route" }));
    expect(onNavigate).toHaveBeenCalledWith({ page: "sources", section: "automations", automationView: "routes", resourceId: "route-3" });
    fireEvent.click(screen.getByRole("button", { name: "All events" }));
    expect(onNavigate).toHaveBeenCalledWith({ page: "sources", section: "events" });
  });

  it("loads process bindings from a deep link and opens the correlated process", async () => {
    const onOpenTask = vi.fn();
    render(<RoleContext.Provider value={{ role: "read_only", canAccess: (required) => hasAccess("read_only", required) }}>
      <Sources route={{ page: "sources", section: "bindings", resourceId: "task-1" }}
        onNavigate={vi.fn()} onOpenTask={onOpenTask} />
    </RoleContext.Provider>);

    expect(await screen.findByText("slack · thread")).toBeVisible();
    expect(screen.getByText("workspace-demo / channel-1 / thread-1")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Open process" }));
    expect(onOpenTask).toHaveBeenCalledWith("task-1");
    fireEvent.change(screen.getByRole("textbox", { name: "Process ID" }), { target: { value: " task-2 " } });
    fireEvent.click(screen.getByRole("button", { name: "Find bindings" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_binding_list", { task_id: "task-2" }));
  });
});
