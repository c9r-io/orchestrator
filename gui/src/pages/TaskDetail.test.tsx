import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TaskDetail from "./TaskDetail";
import { RoleContext, hasAccess } from "../hooks/useRole";
import { useGrpc } from "../hooks/useGrpc";
import { useStream } from "../hooks/useStream";
import type { Role, TaskDetail as TaskDetailType } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../hooks/useGrpc", () => ({ useGrpc: vi.fn() }));
vi.mock("../hooks/useStream", () => ({ useStream: vi.fn() }));
vi.mock("../components/ProcessTimeline", () => ({ default: () => <div>semantic timeline</div> }));
vi.mock("../components/EvidencePanel", () => ({ default: () => <div>evidence panel</div> }));
vi.mock("../components/HandoffPanel", () => ({ default: ({ reviewRequest }: { reviewRequest: number }) => <div>handoff review {reviewRequest}</div> }));
vi.mock("../components/SessionPanel", () => ({ default: () => <div>session panel</div> }));
vi.mock("../components/SourcePanel", () => ({ default: () => <div>source panel</div> }));
vi.mock("../components/ExpertPanel", () => ({ default: () => <div>expert details</div> }));

function detail(status: string): TaskDetailType {
  return {
    id: "task-1", name: "Fix payment failure", status, goal: "Restore tests", total_items: 2,
    finished_items: status === "completed" ? 2 : 1, failed_items: status === "failed" ? 1 : 0,
    created_at: "2026-07-17T00:00:00Z", updated_at: "2026-07-17T00:01:00Z",
    project_id: "project-1", workflow_id: "qa-loop", items: [],
  };
}

function renderAs(role: Role, status: string, onBack = vi.fn()) {
  vi.mocked(useGrpc).mockReturnValue({ data: detail(status), error: null, loading: false, call: vi.fn() });
  vi.mocked(useStream).mockReturnValue({
    data: [{ line: "cargo test failed", timestamp: "2026-07-17T00:00:00Z" }, { line: "other output", timestamp: "2026-07-17T00:00:01Z" }],
    active: false, error: null, start: vi.fn(), stop: vi.fn(),
  });
  return { onBack, ...render(<RoleContext.Provider value={{ role, canAccess: (required) => hasAccess(role, required) }}>
    <TaskDetail taskId="task-1" onBack={onBack} />
  </RoleContext.Provider>) };
}

describe("TaskDetail", () => {
  beforeEach(() => {
    vi.mocked(listen).mockResolvedValue(() => undefined);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") return { items: [], latest_change_id: 0 };
      if (command === "agent_session_list") return [];
      if (command === "task_trace") return { trace_json: "{\"trace\":true}" };
      if (["task_pause", "task_recover", "task_delete"].includes(command)) return { message: `${command} succeeded` };
      return null;
    });
  });
  afterEach(cleanup);

  it("routes a failed process through reviewed resume and exposes bounded expert diagnostics", async () => {
    renderAs("operator", "failed");
    expect(screen.getByRole("heading", { name: "Fix payment failure" })).toBeVisible();
    expect(screen.getByText("handoff review 0")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Review safe resume" }));
    expect(screen.getByText("handoff review 1")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Expert off" }));
    fireEvent.click(screen.getByRole("button", { name: "Load trace JSON" }));
    expect(await screen.findByText(/"trace":true/)).toBeVisible();
    fireEvent.change(screen.getByLabelText("Search raw logs"), { target: { value: "cargo" } });
    expect(screen.getByRole("log")).toHaveTextContent("cargo test failed");
    expect(screen.getByRole("log")).not.toHaveTextContent("other output");
    fireEvent.click(screen.getByRole("button", { name: "Repair orphaned running items" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Repair orphaned running items" })).getByRole("button", { name: "Repair orphaned items" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("task_recover", { task_id: "task-1" }));
  });

  it("pauses running work for operators and reports the daemon result", async () => {
    renderAs("operator", "running");
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(await screen.findByRole("status")).toHaveTextContent("task_pause succeeded");
    expect(invoke).toHaveBeenCalledWith("task_pause", { task_id: "task-1" });
  });

  it("requires admin presentation access for destructive deletion", async () => {
    const operator = renderAs("operator", "failed");
    expect(screen.queryByRole("button", { name: "Delete" })).not.toBeInTheDocument();
    operator.unmount();
    const { onBack } = renderAs("admin", "failed");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "删除任务" })).getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("task_delete", { task_id: "task-1", force: true }));
    expect(onBack).toHaveBeenCalledOnce();
  });
});
