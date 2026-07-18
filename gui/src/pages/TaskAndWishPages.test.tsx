import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TaskList from "./TaskList";
import WishDetail from "./WishDetail";
import WishPool from "./WishPool";
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
    ...task("wish-1", status, { name: "Draft Slack automation" }),
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

describe("TaskList", () => {
  beforeEach(() => {
    vi.mocked(useGrpc).mockReturnValue({
      data: [
        task("done", "completed", { finished_items: 2 }),
        task("active", "running", { total_items: 0, name: "" }),
        task("broken", "failed"),
        task("waiting", "pending"),
        task("custom", "queued"),
      ],
      error: null,
      loading: false,
      call: vi.fn().mockResolvedValue(null),
    });
  });

  afterEach(cleanup);

  it("renders task state and progress, refreshes, and opens rows without double navigation", () => {
    const onSelect = vi.fn();
    renderAs("operator", <TaskList onSelect={onSelect} />);

    expect(screen.getByText("2/2")).toBeVisible();
    expect(screen.getByText("-")).toBeVisible();
    expect(screen.getByText("completed").className).toContain("badge-success");
    expect(screen.getByText("running").className).toContain("badge-info");
    expect(screen.getByText("failed").className).toContain("badge-danger");
    expect(screen.getByText("pending").className).toContain("badge-warning");
    expect(screen.getByText("queued").className).toContain("badge");

    fireEvent.click(screen.getByText("active"));
    fireEvent.click(screen.getAllByRole("button", { name: "View" })[0]);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(onSelect.mock.calls).toEqual([["active"], ["done"]]);
    expect(vi.mocked(useGrpc).mock.results[0].value.call).toHaveBeenCalledTimes(2);
  });

  it("hides operator actions and represents loading, errors, and empty results", () => {
    const call = vi.fn();
    vi.mocked(useGrpc).mockReturnValue({ data: [], error: "offline", loading: true, call });
    renderAs("read_only", <TaskList onSelect={vi.fn()} />);

    expect(screen.queryByText("Actions")).not.toBeInTheDocument();
    expect(screen.getByText("offline")).toBeVisible();
    expect(screen.getByText("No tasks found.")).toBeVisible();
    expect(document.querySelectorAll(".skeleton")).toHaveLength(3);
  });
});

describe("WishPool", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(cleanup);

  it("sorts, filters, and opens wishes with mouse and keyboard", async () => {
    vi.mocked(invoke).mockResolvedValue([
      task("old", "completed", { updated_at: "2026-07-18T00:00:00Z" }),
      task("new", "running", { updated_at: "2026-07-18T01:00:00Z" }),
      task("cancelled", "deleted", { updated_at: "2026-07-18T00:30:00Z" }),
    ]);
    const onSelectWish = vi.fn();
    renderAs("operator", <WishPool onSelectWish={onSelectWish} />);

    const wishes = await screen.findAllByRole("button", { name: /许愿:/ });
    expect(wishes.map((item) => item.getAttribute("aria-label"))).toEqual([
      "许愿: new",
      "许愿: cancelled",
      "许愿: old",
    ]);
    fireEvent.click(wishes[0]);
    fireEvent.keyDown(wishes[1], { key: "Enter" });
    expect(onSelectWish.mock.calls).toEqual([["new"], ["cancelled"]]);

    fireEvent.click(screen.getByRole("button", { name: "待确认" }));
    expect(screen.getByRole("button", { name: "许愿: old" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "许愿: new" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "已确认" }));
    expect(screen.getByText("没有匹配的许愿")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  });

  it("creates a trimmed wish from the button or shortcut and reports provider errors", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce({ task_id: "draft-2", status: "pending", message: "created" });
    const onSelectWish = vi.fn();
    const view = renderAs("operator", <WishPool onSelectWish={onSelectWish} />);
    expect(await screen.findByText("还没有许过愿，在上方输入你的第一个需求吧")).toBeVisible();

    const input = screen.getByRole("textbox", { name: "需求描述" });
    fireEvent.change(input, { target: { value: "  automate Slack  " } });
    fireEvent.click(screen.getByRole("button", { name: "提交许愿" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("task_create", {
      goal: "automate Slack",
      project_id: "wish-pool",
    }));
    expect(onSelectWish).toHaveBeenCalledWith("draft-2");

    vi.mocked(invoke).mockRejectedValueOnce("creation denied");
    fireEvent.change(input, { target: { value: "retry wish" } });
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    expect(await screen.findByText("creation denied")).toBeVisible();

    view.unmount();
    renderAs("read_only", <WishPool onSelectWish={vi.fn()} />);
    expect(screen.queryByRole("textbox", { name: "需求描述" })).not.toBeInTheDocument();
  });
});

describe("WishDetail", () => {
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
      <WishDetail taskId="wish-1" onBack={vi.fn()} onConfirmed={onConfirmed} />
    ));

    expect(await screen.findByRole("log", { name: "FR 草稿内容" })).toHaveTextContent("# FR draft");
    fireEvent.click(screen.getByRole("button", { name: "确认开发" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("task_create", {
      goal: "wish-1 goal",
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
    renderAs("operator", <WishDetail taskId="wish-1" onBack={onBack} onConfirmed={vi.fn()} />);

    expect(screen.getByText("stale")).toBeVisible();
    expect(screen.getByRole("log", { name: "FR 草稿内容" })).toHaveTextContent("live draft");
    fireEvent.click(screen.getByRole("button", { name: "修改需求" }));
    expect(onBack).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    const dialog = screen.getByRole("dialog", { name: "取消许愿" });
    fireEvent.click(within(dialog).getByRole("button", { name: "确认取消" }));
    expect(await screen.findByText("delete denied")).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("task_delete", { task_id: "wish-1", force: true });
  });

  it("uses streamed logs when completed-log loading fails and hides actions from readers", async () => {
    vi.mocked(useGrpc).mockReturnValue({ data: detail("completed"), error: null, loading: false, call });
    vi.mocked(invoke).mockRejectedValue(new Error("logs unavailable"));
    renderAs("read_only", <WishDetail taskId="wish-1" onBack={vi.fn()} onConfirmed={vi.fn()} />);

    expect(await screen.findByRole("log", { name: "FR 草稿内容" })).toHaveTextContent("live draft");
    expect(screen.queryByRole("button", { name: "确认开发" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消" })).not.toBeInTheDocument();
  });
});
