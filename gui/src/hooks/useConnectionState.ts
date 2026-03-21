import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useCallback } from "react";
import type { ConnectionState } from "../lib/types";

/**
 * Hook that tracks the connection state emitted from the Rust backend
 * via the `connection-state-changed` Tauri event.
 */
export function useConnectionState() {
  const [connectionState, setConnectionState] = useState<ConnectionState>({
    kind: "Disconnected",
  });

  useEffect(() => {
    const unlisten = listen<ConnectionState>(
      "connection-state-changed",
      (event) => {
        setConnectionState(event.payload);
      }
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const reconnect = useCallback(async () => {
    try {
      await invoke("connect", {});
    } catch {
      // State will be updated via the event
    }
  }, []);

  return { connectionState, reconnect };
}
