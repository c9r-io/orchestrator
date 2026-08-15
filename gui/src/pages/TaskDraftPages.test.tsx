import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TaskDraftDetail from "./TaskDraftDetail";
import TaskDraftList from "./TaskDraftList";
import { RoleContext, hasAccess } from "../hooks/useRole";
import { useGrpc } from "../hooks/useGrpc";
import { useStream } from "../hooks/useStream";
import type {
  Role,
  TaskDetail as TaskDetailType,
  TaskSummary,
} from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../hooks/useGrpc", () => ({ useGrpc: vi.fn() }));
vi.mock("../hooks/useStream", () => ({ useStream: vi.fn() }));

function task(
  id: string,
  status: string,
  overrides: Partial<TaskSummary> = {},
): TaskSummary {
  return {
    id,
    name: id,
    status,
    total_items: 2,
    finished_items: 1,
    failed_items: 0,
    created_at: "2026-07-18T00:00:00Z",
    updated_at: "2026-07-18T00:01:00Z",
    project_id: "project-1",
    workflow_id: "qa-loop",
    goal: `${id} goal`,
    ...overrides,
  };
}

function detail(status: string): TaskDetailType {
  return {
    ...task("draft-1", status, { name: "Draft Slack automation" }),
    items: [],
  };
}

function renderAs(role: Role, child: React.ReactNode) {
  return render(
    <RoleContext.Provider
      value={{ role, canAccess: (required) => hasAccess(role, required) }}
    >
      {child}
    </RoleContext.Provider>,
  );
}

describe("TaskDraftList", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(cleanup);

  it("sorts, filters, and opens drafts with mouse and keyboard", async () => {
    vi.mocked(invoke).mockResolvedValue([
      task("old", "completed", { updated_at: "2026-07-18T00:00:00Z" }),
      task("new", "running", { updated_at: "2026-07-18T01:00:00Z" }),
      task("cancelled", "deleted", { updated_at: "2026-07-18T00:30:00Z" }),
    ]);
    const onSelectDraft = vi.fn();
    renderAs("operator", <TaskDraftList onSelectDraft={onSelectDraft} />);

    const drafts = await screen.findAllByRole("button", { name: /任务草稿:/ });
    expect(drafts.map((item) => item.getAttribute("aria-label"))).toEqual([
      "任务草稿: new",
      "任务草稿: cancelled",
      "任务草稿: old",
    ]);
    fireEvent.click(drafts[0]);
    fireEvent.keyDown(drafts[1], { key: "Enter" });
    expect(onSelectDraft.mock.calls).toEqual([["new"], ["cancelled"]]);

    fireEvent.click(screen.getByRole("button", { name: "待确认" }));
    expect(screen.getByRole("button", { name: "任务草稿: old" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "任务草稿: new" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "已确认" }));
    expect(screen.getByText("没有匹配的草稿")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  });

  it("creates a trimmed draft from the button or shortcut and reports provider errors", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce({ task_id: "draft-2", status: "pending", message: "created" });
    const onSelectDraft = vi.fn();
    const view = renderAs("operator", <TaskDraftList onSelectDraft={onSelectDraft} />);
    expect(await screen.findByText("还没有草稿，在上方输入你的第一个需求吧")).toBeVisible();

    const input = screen.getByRole("textbox", { name: "需求描述" });
    fireEvent.change(input, { target: { value: "  automate Slack  " } });
    fireEvent.click(screen.getByRole("button", { name: "提交草稿" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("task_create", {
      goal: "automate Slack",
      project_id: "wish-pool",
    }));
    expect(onSelectDraft).toHaveBeenCalledWith("draft-2");

    vi.mocked(invoke).mockRejectedValueOnce("creation denied");
    fireEvent.change(input, { target: { value: "retry draft" } });
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    expect(await screen.findByText("creation denied")).toBeVisible();

    view.unmount();
    renderAs("read_only", <TaskDraftList onSelectDraft={vi.fn()} />);
    expect(screen.queryByRole("textbox", { name: "需求描述" })).not.toBeInTheDocument();
  });
});

describe("TaskDraftDetail", () => {
  const call = vi.fn();
  const start = vi.fn();
  const stop = vi.fn();

  beforeEach(() => {
    call.mockReset();
    start.mockReset();
    stop.mockReset();
    vi.mocked(invoke).mockReset();
    vi.mocked(useStream).mockReturnValue({
      data: [{ line: "live draft", timestamp: "2026-07-18T00:00:00Z" }],
      active: true,
      error: null,
      start,
      stop,
    });
  });

  afterEach(cleanup);

  it("loads a completed draft, confirms development, and stops following on exit", async () => {
    vi.mocked(useGrpc).mockReturnValue({ data: detail("completed"), error: null, loading: false, call });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "task_logs") {
        return [{ run_id: "run-1", phase: "draft", content: "# FR draft", started_at: null }];
      }
      if (command === "task_create") {
        return { task_id: "implementation-1", status: "pending", message: "created" };
      }
      return null;
    });
    const onConfirmed = vi.fn();
    const view = renderAs("operator", (
      <TaskDraftDetail taskId="draft-1" onBack={vi.fn()} onConfirmed={onConfirmed} />
    ));

    expect(await screen.findByRole("log", { name: "FR 草稿内容" })).toHaveTextContent("# FR draft");
    fireEvent.click(screen.getByRole("button", { name: "确认开发" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("task_create", {
      goal: "draft-1 goal",
      name: "Draft Slack automation",
    }));
    expect(onConfirmed).toHaveBeenCalledWith("implementation-1");
    view.unmount();
    expect(stop).toHaveBeenCalledOnce();
  });

  it("falls back to live output, exposes drafting state, and handles cancellation failures", async () => {
    vi.mocked(useGrpc).mockReturnValue({ data: detail("running"), error: "stale", loading: false, call });
    vi.mocked(invoke).mockRejectedValue("delete denied");
    const onBack = vi.fn();
    renderAs("operator", <TaskDraftDetail taskId="draft-1" onBack={onBack} onConfirmed={vi.fn()} />);

    expect(screen.getByText("stale")).toBeVisible();
    expect(screen.getByRole("log", { name: "FR 草稿内容" })).toHaveTextContent("live draft");
    fireEvent.click(screen.getByRole("button", { name: "修改需求" }));
    expect(onBack).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    const dialog = screen.getByRole("dialog", { name: "取消草稿" });
    fireEvent.click(within(dialog).getByRole("button", { name: "确认取消" }));
    expect(await screen.findByText("delete denied")).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("task_delete", { task_id: "draft-1", force: true });
  });

  it("uses streamed logs when completed-log loading fails and hides actions from readers", async () => {
    vi.mocked(useGrpc).mockReturnValue({ data: detail("completed"), error: null, loading: false, call });
    vi.mocked(invoke).mockRejectedValue(new Error("logs unavailable"));
    renderAs("read_only", <TaskDraftDetail taskId="draft-1" onBack={vi.fn()} onConfirmed={vi.fn()} />);

    expect(await screen.findByRole("log", { name: "FR 草稿内容" })).toHaveTextContent("live draft");
    expect(screen.queryByRole("button", { name: "确认开发" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消" })).not.toBeInTheDocument();
  });
});
