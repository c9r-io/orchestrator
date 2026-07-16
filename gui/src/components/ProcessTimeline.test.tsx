import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import ProcessTimeline from "./ProcessTimeline";
import { useTimeline } from "../hooks/useTimeline";
import type { TimelineEntry } from "../lib/types";

vi.mock("../hooks/useTimeline", () => ({ useTimeline: vi.fn() }));

const entry: TimelineEntry = {
  id: "entry-1", task_id: "task-1", occurred_at: "2026-07-17T00:00:00Z", category: "failure",
  title: "Tests failed", summary: "One assertion failed", status: "failed",
  actor: { actor_type: "agent", actor_id: "qa" }, step_id: "test", task_item_id: "item-1",
  command_run_id: "run-1", session_id: "session-1", checkpoint_id: "checkpoint-1",
  source_event_id: null, evidence: [{ kind: "test", label: "cargo test", uri: null,
    content_type: "text/plain", digest: null, redacted: true }], raw_event_ids: [1], projection_version: 1,
};

describe("ProcessTimeline", () => {
  afterEach(cleanup);

  it("selects the first semantic entry and exposes evidence context", () => {
    const onSelectEntry = vi.fn();
    vi.mocked(useTimeline).mockReturnValue({
      entries: [entry], hasMore: false, loading: false, loadingMore: false, error: null, loadMore: vi.fn(),
    });
    render(<ProcessTimeline taskId="task-1" onSelectEntry={onSelectEntry} />);
    expect(onSelectEntry).toHaveBeenCalledWith(entry);
    expect(screen.getByRole("button", { name: /Tests failed/ })).toHaveTextContent("agent: qa");
    expect(screen.getByLabelText("证据引用")).toHaveTextContent("test: cargo test · redacted");
    fireEvent.click(screen.getByRole("button", { name: /Tests failed/ }));
    expect(onSelectEntry).toHaveBeenCalledTimes(2);
  });

  it("renders errors and delegates bounded pagination", () => {
    const loadMore = vi.fn();
    vi.mocked(useTimeline).mockReturnValue({
      entries: [entry], hasMore: true, loading: false, loadingMore: false,
      error: "projection lagged", loadMore,
    });
    render(<ProcessTimeline taskId="task-1" selectedEntryId="entry-1" />);
    expect(screen.getByText("projection lagged")).toBeVisible();
    const more = screen.getByRole("button", { name: "加载更多记录" });
    fireEvent.click(more);
    expect(loadMore).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: /Tests failed/ })).toHaveAttribute("aria-pressed", "true");
  });
});
