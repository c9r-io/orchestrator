import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import HandoffPanel from "./HandoffPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const boundary = {
  id: "boundary-1", task_id: "task-1", cycle: 1, step_id: "publish", task_item_id: null,
  provider_session_available: true, side_effect_class: "external", replay_safe: false,
  reason: "Publishing may repeat an external side effect", state_version: "state-1",
};

describe("HandoffPanel", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());
  afterEach(cleanup);

  it("renders a concise briefing generated from the governed snapshot", async () => {
    vi.mocked(invoke).mockResolvedValue({
      id: "handoff-1", task_id: "task-1", source_event_cursor: 42, projection_version: 1,
      briefing: { goal: "Ship safely", current_state: { status: "failed" }, last_success: null,
        failure: { step: "test" }, test_evidence: [], changed_files: ["src/main.rs"], constraints: [],
        decisions: [], open_questions: [], recommendations: ["Review the failed boundary"] },
      content_hash: "abcdef1234567890", state_version: "state-1", created_at: "2026-07-14T00:00:00Z",
    });
    render(<HandoffPanel taskId="task-1" canGenerate canExecute={false} reviewRequest={0} onExecuted={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Generate handoff" }));
    expect(await screen.findByText("src/main.rs")).toBeVisible();
    expect(screen.getByText("Review the failed boundary")).toBeVisible();
    expect(screen.getByText("Snapshot abcdef123456")).toBeVisible();
  });

  it("requires an operator reason and elevated confirmation before risky replay", async () => {
    const onExecuted = vi.fn();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resume_boundary_list") return [boundary];
      if (command === "resume_plan") return {
        id: "plan-1", task_id: "task-1", boundary, mode: "restart_from_boundary",
        expected_state_version: "state-1", consequence: { repeated_steps: ["publish"] },
        elevated_confirmation_required: true, expires_at: "2026-07-14T01:00:00Z", status: "review_required",
      };
      if (command === "resume_execute") return {
        execution_id: "execution-1", plan_id: "plan-1", accepted: true, status: "succeeded", child_task_id: "task-child",
      };
      return null;
    });
    render(<HandoffPanel taskId="task-1" canGenerate canExecute reviewRequest={0} onExecuted={onExecuted} />);
    fireEvent.click(screen.getByRole("button", { name: "Preview resume" }));
    expect(await screen.findByRole("dialog", { name: "Resume consequence preview" })).toBeVisible();
    expect(screen.getByRole("option", { name: "Resume provider session" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Create preview" }));
    const execute = await screen.findByRole("button", { name: "Execute reviewed plan" });
    expect(execute).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Operator reason"), { target: { value: "Reviewed external effects" } });
    expect(execute).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox"));
    expect(execute).toBeEnabled();
    fireEvent.click(execute);
    expect(await screen.findByRole("status")).toHaveTextContent("Resume succeeded · child task-child");
    await waitFor(() => expect(onExecuted).toHaveBeenCalledOnce());
  });

  it("removes generation and execution controls for read-only viewers", () => {
    render(<HandoffPanel taskId="task-1" canGenerate={false} canExecute={false} reviewRequest={0} onExecuted={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Generate handoff" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Preview resume" })).not.toBeInTheDocument();
    expect(screen.getByText(/Read-only access/)).toBeVisible();
  });
});
