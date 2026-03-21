import { useEffect, useState } from "react";
import type { ConnectionState } from "../lib/types";

interface Props {
  state: ConnectionState;
  onRetry: () => void;
}

/**
 * Fixed-position banner at the top of the viewport showing connection status.
 * Only renders when the connection is disrupted or recently restored.
 */
export default function ConnectionBanner({ state, onRetry }: Props) {
  const [showRestored, setShowRestored] = useState(false);
  const [wasDisrupted, setWasDisrupted] = useState(false);

  useEffect(() => {
    if (
      state.kind === "Reconnecting" ||
      state.kind === "Failed"
    ) {
      setWasDisrupted(true);
    }

    if (state.kind === "Connected" && wasDisrupted) {
      setShowRestored(true);
      const timer = setTimeout(() => {
        setShowRestored(false);
        setWasDisrupted(false);
      }, 3000);
      return () => clearTimeout(timer);
    }
  }, [state, wasDisrupted]);

  if (state.kind === "Reconnecting") {
    return (
      <div className="connection-banner banner-warning" role="alert">
        <span className="banner-spinner" />
        连接中断，正在重连... (尝试 {state.attempt}/{state.max_attempts})
      </div>
    );
  }

  if (state.kind === "Failed") {
    return (
      <div className="connection-banner banner-danger" role="alert">
        连接失败：{state.message}
        <button className="btn btn-secondary" style={{ marginLeft: 12 }} onClick={onRetry}>
          重试
        </button>
      </div>
    );
  }

  if (showRestored) {
    return (
      <div className="connection-banner banner-success" role="status">
        已恢复连接
      </div>
    );
  }

  return null;
}
