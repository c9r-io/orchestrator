import { useEffect, useState } from "react";
import i18n from "../lib/i18n";
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
        {i18n.connectionBanner.reconnecting(state.attempt, state.max_attempts)}
      </div>
    );
  }

  if (state.kind === "Failed") {
    return (
      <div className="connection-banner banner-danger" role="alert">
        {i18n.connectionBanner.failed(state.message)}
        <button className="btn btn-secondary" style={{ marginLeft: 12 }} onClick={onRetry}>
          {i18n.connectionBanner.retry}
        </button>
      </div>
    );
  }

  if (showRestored) {
    return (
      <div className="connection-banner banner-success" role="status">
        {i18n.connectionBanner.restored}
      </div>
    );
  }

  return null;
}
