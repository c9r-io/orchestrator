import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
      <h1 className="page-title">无法连接到 orchestratord</h1>

      <div className="liquid-glass" style={{ marginBottom: 20 }}>
        <h3 style={{ marginBottom: 16, fontWeight: 600 }}>可能的原因</h3>

        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 20 }}>1.</span>
            <div>
              <strong>守护进程未启动</strong>
              <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
                请在终端执行：<code style={{ background: "var(--bg-tertiary)", padding: "2px 6px", borderRadius: 4 }}>orchestratord --foreground</code>
              </p>
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 20 }}>2.</span>
            <div>
              <strong>连接地址不正确</strong>
              <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
                检查 <code style={{ background: "var(--bg-tertiary)", padding: "2px 6px", borderRadius: 4 }}>ORCHESTRATOR_SOCKET</code> 环境变量是否指向正确的 socket 文件
              </p>
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 20 }}>3.</span>
            <div>
              <strong>远程连接证书问题</strong>
              <p style={{ color: "var(--text-secondary)", marginTop: 4 }}>
                检查 <code style={{ background: "var(--bg-tertiary)", padding: "2px 6px", borderRadius: 4 }}>~/.orchestrator/control-plane/</code> 下的 TLS 证书配置
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
          {isRetrying ? "连接中..." : "重试连接"}
        </button>
        <button
          className="btn btn-secondary"
          onClick={() => setShowManual(!showManual)}
        >
          {showManual ? "收起手动配置" : "手动配置"}
        </button>
      </div>

      {showManual && (
        <div className="liquid-glass">
          <h3 style={{ marginBottom: 12, fontWeight: 600 }}>手动配置连接</h3>
          <p style={{ color: "var(--text-secondary)", marginBottom: 12 }}>
            指定 control-plane 配置文件路径（YAML），用于连接远程 daemon。
          </p>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              type="text"
              value={configPath}
              onChange={(e) => setConfigPath(e.target.value)}
              placeholder="/path/to/config.yaml"
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
              {connecting ? "连接中..." : "连接"}
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
