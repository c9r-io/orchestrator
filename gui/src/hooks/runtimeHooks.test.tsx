import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useConnectionState } from "./useConnectionState";
import { useGrpc } from "./useGrpc";
import { useStream } from "./useStream";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const listeners = new Map<string, (event: { payload: unknown }) => void>();
const unlistenCalls = new Map<string, number>();

describe("runtime hooks", () => {
  beforeEach(() => {
    listeners.clear();
    unlistenCalls.clear();
    vi.mocked(invoke).mockReset();
    vi.mocked(listen).mockImplementation(async (name, handler) => {
      const key = String(name);
      listeners.set(key, handler as (event: { payload: unknown }) => void);
      return () => unlistenCalls.set(key, (unlistenCalls.get(key) ?? 0) + 1);
    });
  });

  it("useGrpc returns data and clears loading after a successful command", async () => {
    vi.mocked(invoke).mockResolvedValue({ id: "task-1" });
    const { result } = renderHook(() => useGrpc<{ id: string }>("task_info"));
    let response: { id: string } | null = null;
    await act(async () => { response = await result.current.call({ task_id: "task-1" }); });
    expect(response).toEqual({ id: "task-1" });
    expect(result.current).toMatchObject({ data: { id: "task-1" }, error: null, loading: false });
    expect(invoke).toHaveBeenCalledWith("task_info", { task_id: "task-1" });
  });

  it("useGrpc normalizes rejected values and returns null", async () => {
    vi.mocked(invoke).mockRejectedValue("permission denied");
    const { result } = renderHook(() => useGrpc("task_delete"));
    await act(async () => { expect(await result.current.call()).toBeNull(); });
    expect(result.current.error).toBe("permission denied");
    expect(result.current.loading).toBe(false);
  });

  it("useStream starts, collects payloads, reports stream errors, and stops cleanly", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    const params = { task_id: "task-1" };
    const { result } = renderHook(() => useStream<{ line: string }>(
      "start_task_follow", "stop_task_follow", "task-follow-task-1", params, "task-follow-error-task-1"
    ));
    await act(async () => result.current.start());
    expect(result.current.active).toBe(true);
    act(() => listeners.get("task-follow-task-1")?.({ payload: { line: "first" } }));
    expect(result.current.data).toEqual([{ line: "first" }]);
    act(() => listeners.get("task-follow-error-task-1")?.({ payload: "stream lost" }));
    expect(result.current.error).toBe("stream lost");
    expect(result.current.active).toBe(false);

    await act(async () => result.current.stop());
    expect(invoke).toHaveBeenCalledWith("stop_task_follow", params);
    expect(unlistenCalls.get("task-follow-task-1")).toBe(1);
    expect(unlistenCalls.get("task-follow-error-task-1")).toBe(1);
  });

  it("useStream tolerates an already-ended stop command and cleans listeners on unmount", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "stop") throw new Error("already ended");
      return null;
    });
    const params = {};
    const { result, unmount } = renderHook(() => useStream("start", "stop", "data", params));
    await act(async () => result.current.start());
    await act(async () => result.current.stop());
    expect(result.current.active).toBe(false);
    await act(async () => result.current.start());
    unmount();
    expect(unlistenCalls.get("data")).toBe(2);
  });

  it("useConnectionState applies daemon events, reconnects, and unregisters", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    const { result, unmount } = renderHook(() => useConnectionState());
    expect(result.current.connectionState).toEqual({ kind: "Disconnected" });
    await waitFor(() => expect(listeners.has("connection-state-changed")).toBe(true));
    act(() => listeners.get("connection-state-changed")?.({ payload: { kind: "Connected" } }));
    expect(result.current.connectionState).toEqual({ kind: "Connected" });
    await act(async () => result.current.reconnect());
    expect(invoke).toHaveBeenCalledWith("connect", {});
    unmount();
    await waitFor(() => expect(unlistenCalls.get("connection-state-changed")).toBe(1));
  });

  it("useConnectionState leaves error presentation to emitted state when reconnect rejects", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("offline"));
    const { result } = renderHook(() => useConnectionState());
    await act(async () => result.current.reconnect());
    expect(result.current.connectionState).toEqual({ kind: "Disconnected" });
  });
});
