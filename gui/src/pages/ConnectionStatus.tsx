import { useEffect } from "react";
import { useGrpc } from "../hooks/useGrpc";
import type { PingInfo } from "../lib/types";

interface Props {
  connected: boolean;
}

export default function ConnectionStatus({ connected }: Props) {
  const { data, error, loading, call } = useGrpc<PingInfo>("ping");

  useEffect(() => {
    if (connected) {
      call();
    }
  }, [connected, call]);

  return (
    <div>
      <h1 className="page-title">Connection Status</h1>

      {!connected && (
        <div className="liquid-glass" style={{ color: "var(--danger)" }}>
          Not connected to daemon. Is <code>orchestratord</code> running?
        </div>
      )}

      {loading && <p style={{ color: "var(--text-secondary)" }}>Connecting...</p>}

      {error && (
        <div className="liquid-glass" style={{ color: "var(--danger)" }}>
          {error}
        </div>
      )}

      {data && (
        <div className="liquid-glass">
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <tbody>
              <tr>
                <td style={{ padding: 8, color: "var(--text-secondary)" }}>Version</td>
                <td style={{ padding: 8 }}>{data.version}</td>
              </tr>
              <tr>
                <td style={{ padding: 8, color: "var(--text-secondary)" }}>Git Hash</td>
                <td style={{ padding: 8 }}>
                  <code>{data.git_hash}</code>
                </td>
              </tr>
              <tr>
                <td style={{ padding: 8, color: "var(--text-secondary)" }}>Uptime</td>
                <td style={{ padding: 8 }}>{data.uptime_secs}s</td>
              </tr>
            </tbody>
          </table>
        </div>
      )}

      {connected && (
        <button
          className="btn btn-secondary"
          style={{ marginTop: 12 }}
          onClick={() => call()}
        >
          Refresh
        </button>
      )}
    </div>
  );
}
