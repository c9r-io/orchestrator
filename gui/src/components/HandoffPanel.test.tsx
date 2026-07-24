import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });
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

  it("auto-opens a safe review and hides unavailable provider resume", async () => {
    const safeBoundary = {
      ...boundary,
      provider_session_available: false,
      side_effect_class: "workspace_only",
      replay_safe: true,
      reason: "Workspace-only replay",
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resume_boundary_list") return [safeBoundary];
      return null;
    });
    render(
      <HandoffPanel
        taskId="task-1"
        canGenerate
        canExecute
        reviewRequest={1}
        onExecuted={vi.fn()}
      />,
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Resume consequence preview",
    });
    expect(screen.getByText(/Replay-safe: Workspace-only replay/)).toBeVisible();
    expect(
      screen.queryByRole("option", { name: "Resume provider session" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Close resume dialog" })).toHaveFocus();

    fireEvent.keyDown(dialog, { key: "Escape" });

    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    await waitFor(() => expect(screen.getByRole("button", { name: "Preview resume" })).toHaveFocus());
  });

  it("traps focus and returns manual Escape and close exits to the initiating control", async () => {
    vi.mocked(invoke).mockResolvedValue([boundary]);
    render(<HandoffPanel taskId="task-1" canGenerate canExecute reviewRequest={0} onExecuted={vi.fn()} />);
    const resume = screen.getByRole("button", { name: "Preview resume" });
    resume.focus();
    fireEvent.click(resume);

    const dialog = await screen.findByRole("dialog", { name: "Resume consequence preview" });
    const close = screen.getByRole("button", { name: "Close resume dialog" });
    const createPreview = screen.getByRole("button", { name: "Create preview" });
    expect(close).toHaveFocus();

    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(createPreview).toHaveFocus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(close).toHaveFocus();

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    await waitFor(() => expect(resume).toHaveFocus());

    fireEvent.click(resume);
    await screen.findByRole("dialog", { name: "Resume consequence preview" });
    fireEvent.click(screen.getByRole("button", { name: "Close resume dialog" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() => expect(resume).toHaveFocus());
  });

  it("falls back to the persistent handoff panel when the initiating control is removed", async () => {
    vi.mocked(invoke).mockResolvedValue([boundary]);
    const view = render(<HandoffPanel taskId="task-1" canGenerate canExecute reviewRequest={0} onExecuted={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Preview resume" }));
    await screen.findByRole("dialog", { name: "Resume consequence preview" });

    view.rerender(<HandoffPanel taskId="task-1" canGenerate canExecute={false} reviewRequest={0} onExecuted={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Close resume dialog" }));

    const panel = screen.getByRole("region", { name: "Handoff & safe resume" });
    await waitFor(() => expect(panel).toHaveFocus());
    expect(document.activeElement).not.toBe(document.body);
  });

  it("ignores late boundary responses after unmount", async () => {
    let resolveBoundaries!: (value: typeof boundary[]) => void;
    vi.mocked(invoke).mockImplementation(() => new Promise((resolve) => {
      resolveBoundaries = resolve as (value: typeof boundary[]) => void;
    }));
    const view = render(<HandoffPanel taskId="task-1" canGenerate canExecute reviewRequest={0} onExecuted={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Preview resume" }));
    view.unmount();

    await act(async () => {
      resolveBoundaries([boundary]);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("does not allow Escape or Close to hide an in-flight reviewed operation", async () => {
    let resolvePlan!: (value: ResumePlanFixture) => void;
    let resolveExecution!: (value: ResumeExecutionFixture) => void;
    type ResumePlanFixture = {
      id: string; task_id: string; boundary: typeof boundary; mode: string;
      expected_state_version: string; consequence: Record<string, unknown>;
      elevated_confirmation_required: boolean; expires_at: string; status: string;
    };
    type ResumeExecutionFixture = {
      execution_id: string; plan_id: string; accepted: boolean; status: string; child_task_id: string | null;
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resume_boundary_list") return [boundary];
      if (command === "resume_plan") return new Promise((resolve) => {
        resolvePlan = resolve as (value: ResumePlanFixture) => void;
      });
      if (command === "resume_execute") return new Promise((resolve) => {
        resolveExecution = resolve as (value: ResumeExecutionFixture) => void;
      });
      return null;
    });
    render(<HandoffPanel taskId="task-1" canGenerate canExecute reviewRequest={0} onExecuted={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Preview resume" }));
    const dialog = await screen.findByRole("dialog", { name: "Resume consequence preview" });
    fireEvent.click(screen.getByRole("button", { name: "Create preview" }));

    expect(screen.getByRole("button", { name: "Close resume dialog" })).toBeDisabled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(dialog).toBeVisible();

    await act(async () => resolvePlan({
      id: "plan-1", task_id: "task-1", boundary, mode: "restart_from_boundary",
      expected_state_version: "state-1", consequence: { repeated_steps: ["publish"] },
      elevated_confirmation_required: false, expires_at: "2026-07-14T01:00:00Z",
      status: "review_required",
    }));
    await waitFor(() => expect(screen.getByLabelText("Operator reason")).toHaveFocus());
    expect(screen.getByRole("button", { name: "Close resume dialog" })).toBeEnabled();

    fireEvent.change(screen.getByLabelText("Operator reason"), { target: { value: "Reviewed replay" } });
    fireEvent.click(screen.getByRole("button", { name: "Execute reviewed plan" }));
    expect(screen.getByRole("button", { name: "Close resume dialog" })).toBeDisabled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(dialog).toBeVisible();

    await act(async () => resolveExecution({
      execution_id: "execution-1", plan_id: "plan-1", accepted: true,
      status: "succeeded", child_task_id: null,
    }));
    expect(await screen.findByRole("status")).toHaveTextContent("Resume succeeded");
    expect(screen.getByRole("button", { name: "Close resume dialog" })).toBeEnabled();
  });

  it("keeps the reviewed dialog recoverable when preview or execution fails", async () => {
    const onExecuted = vi.fn();
    let failPreview = true;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resume_boundary_list") return [boundary];
      if (command === "resume_plan") {
        if (failPreview) {
          failPreview = false;
          throw new Error("stale boundary");
        }
        return {
          id: "plan-1", task_id: "task-1", boundary, mode: "restart_from_boundary",
          expected_state_version: "state-1", consequence: { repeated_steps: ["publish"] },
          elevated_confirmation_required: true, expires_at: "2026-07-14T01:00:00Z",
          status: "review_required",
        };
      }
      if (command === "resume_execute") throw new Error("version conflict");
      return null;
    });
    render(
      <HandoffPanel
        taskId="task-1"
        canGenerate
        canExecute
        reviewRequest={0}
        onExecuted={onExecuted}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Preview resume" }));
    await screen.findByRole("dialog", { name: "Resume consequence preview" });

    const createPreview = screen.getByRole("button", { name: "Create preview" });
    createPreview.focus();
    fireEvent.click(createPreview);
    expect(await screen.findByRole("alert")).toHaveTextContent("stale boundary");
    expect(screen.getByRole("button", { name: "Create preview" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Create preview" })).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Create preview" }));
    const execute = await screen.findByRole("button", { name: "Execute reviewed plan" });
    fireEvent.change(screen.getByLabelText("Operator reason"), {
      target: { value: "Reviewed retry" },
    });
    fireEvent.click(screen.getByRole("checkbox"));
    execute.focus();
    fireEvent.click(execute);

    expect(await screen.findByRole("alert")).toHaveTextContent("version conflict");
    expect(execute).toBeEnabled();
    expect(execute).toHaveFocus();
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
    expect(onExecuted).not.toHaveBeenCalled();
  });

  it("removes generation and execution controls for read-only viewers", () => {
    render(<HandoffPanel taskId="task-1" canGenerate={false} canExecute={false} reviewRequest={0} onExecuted={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Generate handoff" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Preview resume" })).not.toBeInTheDocument();
    expect(screen.getByText(/Read-only access/)).toBeVisible();
  });
});
