import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useState, useCallback, useRef, useEffect } from "react";

interface UseStreamResult<T> {
  data: T[];
  active: boolean;
  start: () => Promise<void>;
  stop: () => Promise<void>;
}

/**
 * Hook for subscribing to Tauri streaming events (server-streaming gRPC).
 *
 * @param startCommand — Tauri command to start the stream (e.g. "start_task_follow")
 * @param stopCommand — Tauri command to stop the stream (e.g. "stop_task_follow")
 * @param eventName — Tauri event to listen for (e.g. "task-follow-abc123")
 * @param params — parameters passed to start/stop commands
 */
export function useStream<T>(
  startCommand: string,
  stopCommand: string,
  eventName: string,
  params: Record<string, unknown>
): UseStreamResult<T> {
  const [data, setData] = useState<T[]>([]);
  const [active, setActive] = useState(false);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const start = useCallback(async () => {
    setData([]);
    const unlisten = await listen<T>(eventName, (event) => {
      setData((prev) => [...prev, event.payload]);
    });
    unlistenRef.current = unlisten;
    await invoke(startCommand, params);
    setActive(true);
  }, [startCommand, eventName, params]);

  const stop = useCallback(async () => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
    try {
      await invoke(stopCommand, params);
    } catch {
      // stream may have already ended
    }
    setActive(false);
  }, [stopCommand, params]);

  useEffect(() => {
    return () => {
      if (unlistenRef.current) {
        unlistenRef.current();
      }
    };
  }, []);

  return { data, active, start, stop };
}
