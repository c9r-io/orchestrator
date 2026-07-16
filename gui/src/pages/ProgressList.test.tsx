import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ProgressList from "./ProgressList";
import type { TaskSummary } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const listeners = new Map<string, (event: { payload: unknown }) => void>();
const stopped = vi.fn();

function task(id: string, status: string, updatedAt: string, overrides: Partial<TaskSummary> = {}): TaskSummary {
  return {
    id, name: id, status, total_items: 2, finished_items: 1, failed_items: 0,
    created_at: "2026-07-14T00:00:00Z", updated_at: updatedAt, project_id: "project-1",
    workflow_id: "qa-loop", goal: id, ...overrides,
  };
}

describe("ProgressList", () => {
  beforeEach(() => {
    listeners.clear();
    stopped.mockReset();
    vi.mocked(listen).mockImplementation(async (name, handler) => {
      listeners.set(String(name), handler as (event: { payload: unknown }) => void);
      return stopped;
    });
  });
  afterEach(cleanup);

  it("sorts by operational priority, watches active tasks, and opens rows by mouse or keyboard", async () => {
    const tasks = [
      task("completed", "completed", "2026-07-14T00:03:00Z", { finished_items: 2 }),
      task("failed", "failed", "2026-07-14T00:02:00Z", { failed_items: 1 }),
      task("running", "running", "2026-07-14T00:01:00Z"),
    ];
    vi.mocked(invoke).mockImplementation(async (command) => command === "task_list" ? tasks : null);
    const onSelect = vi.fn();
    const { unmount } = render(<ProgressList onSelect={onSelect} />);
    const rows = await screen.findAllByRole("button", { name: /任务:/ });
    expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual([
      "任务: running", "任务: failed", "任务: completed",
    ]);
    expect(invoke).toHaveBeenCalledWith("start_task_watch", { task_id: "running", interval_secs: 3 });
    expect(listen).toHaveBeenCalledTimes(1);
    fireEvent.click(rows[1]);
    fireEvent.keyDown(rows[0], { key: "Enter" });
    expect(onSelect.mock.calls).toEqual([["failed"], ["running"]]);

    act(() => listeners.get("task-watch-running")?.({
      payload: { task: task("running", "completed", "2026-07-14T00:04:00Z", { finished_items: 2 }), items: [] },
    }));
    await waitFor(() => expect(screen.getAllByRole("button", { name: /任务:/ })[1]).toHaveAccessibleName("任务: running"));
    unmount();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_task_watch", { task_id: "running" }));
    expect(stopped).toHaveBeenCalledOnce();
  });

  it("renders failures and recovers through Refresh", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("daemon unavailable").mockResolvedValueOnce([]);
    render(<ProgressList onSelect={vi.fn()} />);
    expect(await screen.findByText("daemon unavailable")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    expect(await screen.findByText("暂无任务")).toBeVisible();
  });
});
