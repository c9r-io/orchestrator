import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import i18n from "../lib/i18n";
import type { ConnectionState } from "../lib/types";

interface Props {
  state: ConnectionState;
  onRetry: () => void;
}

/**
 * Connection wizard page — shown when the GUI cannot connect to the daemon.
 * Guides the user through common causes and provides manual config option.
 */
export default function ConnectionStatus({ state, onRetry }: Props) {
  const [showManual, setShowManual] = useState(false);
  const [configPath, setConfigPath] = useState("");
  const [manualError, setManualError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);

  const handleManualConnect = async () => {
    if (!configPath.trim()) return;
    setConnecting(true);
    setManualError(null);
    try {
      await invoke("connect", { config_path: configPath.trim() });
    } catch (e) {
      setManualError(typeof e === "string" ? e : String(e));
    } finally {
      setConnecting(false);
    }
  };

  const isRetrying = state.kind === "Connecting" || state.kind === "Reconnecting";
  const errorMessage = state.kind === "Failed" ? state.message : null;

  return (
    <div>
      <h1 className="page-title">{i18n.connection.title}</h1>

      <div className="liquid-glass" style={{ marginBottom: 20 }}>
        <h3 style={{ marginBottom: 16, fontWeight: 600 }}>{i18n.connection.possibleCauses}</h3>

        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 20 }}>1.</span>
            <div>
              <strong>{i18n.connection.cause1Title}</strong>
              <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
                {i18n.connection.cause1Desc}<code style={{ background: "var(--bg-tertiary)", padding: "2px 6px", borderRadius: 4 }}>{i18n.connection.cause1Cmd}</code>
              </p>
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 20 }}>2.</span>
            <div>
              <strong>{i18n.connection.cause2Title}</strong>
              <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
                {i18n.connection.cause2Desc}
              </p>
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 20 }}>3.</span>
            <div>
              <strong>{i18n.connection.cause3Title}</strong>
              <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
                {i18n.connection.cause3Desc}
              </p>
            </div>
          </div>
        </div>
      </div>

      {errorMessage && (
        <div className="liquid-glass" style={{ color: "var(--danger)", marginBottom: 20 }}>
          {errorMessage}
        </div>
      )}

      <div style={{ display: "flex", gap: 12, marginBottom: 20 }}>
        <button
          className="btn btn-primary"
          onClick={onRetry}
          disabled={isRetrying}
        >
          {isRetrying ? i18n.connection.connecting : i18n.connection.retryConnect}
        </button>
        <button
          className="btn btn-secondary"
          onClick={() => setShowManual(!showManual)}
        >
          {showManual ? i18n.connection.collapseManual : i18n.connection.manualConfig}
        </button>
      </div>

      {showManual && (
        <div className="liquid-glass">
          <h3 style={{ marginBottom: 12, fontWeight: 600 }}>{i18n.connection.manualTitle}</h3>
          <p style={{ color: "var(--text-secondary)", marginBottom: 12 }}>
            {i18n.connection.manualDesc}
          </p>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              type="text"
              value={configPath}
              onChange={(e) => setConfigPath(e.target.value)}
              placeholder={i18n.connection.manualPlaceholder}
              style={{
                flex: 1,
                padding: "8px 12px",
                borderRadius: 8,
                border: "1px solid var(--glass-border-subtle)",
                background: "var(--bg-secondary)",
                color: "var(--text-primary)",
                fontSize: 14,
              }}
            />
            <button
              className="btn btn-primary"
              onClick={handleManualConnect}
              disabled={connecting || !configPath.trim()}
            >
              {connecting ? i18n.connection.connecting : i18n.connection.connect}
            </button>
          </div>
          {manualError && (
            <p style={{ color: "var(--danger)", marginTop: 8 }}>{manualError}</p>
          )}
        </div>
      )}
    </div>
  );
}
