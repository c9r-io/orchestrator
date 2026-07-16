import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useTimeline } from "./useTimeline";
import type { TimelineEntry } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const listeners = new Map<string, (event: { payload: unknown }) => void>();

function entry(id: string, title = id): TimelineEntry {
  return {
    id, task_id: "task-1", occurred_at: "2026-07-14T00:00:00Z", category: "step",
    title, summary: title, status: "completed", actor: null, step_id: "test",
    task_item_id: null, command_run_id: null, session_id: null, checkpoint_id: null,
    source_event_id: null, evidence: [], raw_event_ids: [1], projection_version: 1,
  };
}

function page(entries: TimelineEntry[], nextCursor: string | null = null) {
  return {
    entries, next_cursor: nextCursor, has_more: nextCursor !== null,
    snapshot_max_event_id: 7, projection_version: 1,
  };
}

describe("useTimeline", () => {
  beforeEach(() => {
    listeners.clear();
    vi.mocked(listen).mockImplementation(async (name, handler) => {
      listeners.set(String(name), handler as (event: { payload: unknown }) => void);
      return () => undefined;
    });
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "task_timeline") return page([entry("entry-1")]);
      if (command === "start_task_timeline_follow") return null;
      if (command === "stop_task_timeline_follow") return null;
      throw new Error(`unexpected ${command} ${JSON.stringify(args)}`);
    });
  });

  it("loads a snapshot, follows from its durable cursor, and upserts by stable ID", async () => {
    const { result } = renderHook(() => useTimeline("task-1"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.entries).toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("start_task_timeline_follow", {
      task_id: "task-1", after_event_id: 7, categories: [],
    });

    act(() => listeners.get("task-timeline-task-1")?.({
      payload: { kind: "upsert", entry: entry("entry-1", "updated"), snapshot_max_event_id: 8 },
    }));
    expect(result.current.entries).toHaveLength(1);
    expect(result.current.entries[0].title).toBe("updated");
  });

  it("paginates with the opaque cursor and keeps existing entries", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "task_timeline") {
        return (args as { cursor: string | null }).cursor === "cursor-2"
          ? page([entry("entry-2")])
          : page([entry("entry-1")], "cursor-2");
      }
      return null;
    });
    const { result } = renderHook(() => useTimeline("task-1"));
    await waitFor(() => expect(result.current.hasMore).toBe(true));
    await act(async () => result.current.loadMore());
    expect(result.current.entries.map((item) => item.id)).toEqual(["entry-1", "entry-2"]);
    expect(result.current.hasMore).toBe(false);
  });

  it("refreshes the authoritative snapshot when the stream requests a reset", async () => {
    let snapshot = page([entry("entry-1")]);
    vi.mocked(invoke).mockImplementation(async (command) => command === "task_timeline" ? snapshot : null);
    const { result } = renderHook(() => useTimeline("task-1"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    snapshot = page([entry("entry-reset")]);
    act(() => listeners.get("task-timeline-task-1")?.({
      payload: { kind: "reset_required", entry: null, snapshot_max_event_id: 9 },
    }));
    await waitFor(() => expect(result.current.entries[0].id).toBe("entry-reset"));
  });

  it("surfaces follow errors without discarding the last readable snapshot", async () => {
    const { result } = renderHook(() => useTimeline("task-1"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    act(() => listeners.get("stream-error-timeline-task-1")?.({ payload: "follow disconnected" }));
    expect(result.current.error).toBe("follow disconnected");
    expect(result.current.entries[0].id).toBe("entry-1");
  });
});
