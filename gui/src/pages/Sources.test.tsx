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
    occurred_at: "2026-07-14T00:00:00Z", received_at: "2026-07-14T00:00:01Z", normalized_json: "{}",
    routing_state: "needs_attention", routing_attempts: 1, routed_task_id: "task-1", last_error_code: "trigger_ambiguous" },
  { id: "source-2", project_id: "project-1", provider: "github", installation_id: "repo-demo",
    external_event_id: "evt-2", event_type: "pull_request", conversation_id: "pr-42", thread_id: null,
    occurred_at: "2026-07-14T00:02:00Z", received_at: "2026-07-14T00:02:01Z", normalized_json: "{}",
    routing_state: "routed", routing_attempts: 1, routed_task_id: "task-2", last_error_code: null },
];

function renderAs(role: Role, onOpenTask = vi.fn()) {
  return {
    onOpenTask,
    ...render(<RoleContext.Provider value={{ role, canAccess: (required) => hasAccess(role, required) }}>
      <Sources onOpenTask={onOpenTask} />
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
      if (command === "source_replay") return true;
      throw new Error(`unexpected ${command}`);
    });
  });
  afterEach(cleanup);

  it("keeps task correlation visible to read-only users without exposing replay", async () => {
    const { onOpenTask } = renderAs("read_only");
    expect(await screen.findAllByRole("listitem")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: "重新路由" })).not.toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "打开进程" })[0]);
    expect(onOpenTask).toHaveBeenCalledWith("task-1");
  });

  it("filters authoritatively and lets admins replay only actionable routing failures", async () => {
    renderAs("admin");
    expect(await screen.findAllByRole("listitem")).toHaveLength(2);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "needs_attention" } });
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "重新路由" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_replay", { id: "source-1" }));
  });
});
