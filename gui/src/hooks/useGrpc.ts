import { invoke } from "@tauri-apps/api/core";
import { useState, useCallback } from "react";

interface UseGrpcResult<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  call: (...args: unknown[]) => Promise<T | null>;
}

/**
 * Generic hook for invoking Tauri gRPC commands.
 *
 * @param command — the Tauri command name (e.g. "task_list")
 */
export function useGrpc<T>(command: string): UseGrpcResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const call = useCallback(
    async (...args: unknown[]): Promise<T | null> => {
      setLoading(true);
      setError(null);
      try {
        const result = await invoke<T>(command, args[0] as Record<string, unknown> ?? {});
        setData(result);
        return result;
      } catch (e) {
        const msg = typeof e === "string" ? e : String(e);
        setError(msg);
        return null;
      } finally {
        setLoading(false);
      }
    },
    [command]
  );

  return { data, error, loading, call };
}
