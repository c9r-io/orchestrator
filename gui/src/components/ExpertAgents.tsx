import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import ConfirmDialog from "./ConfirmDialog";
import type { AgentInfo } from "../lib/types";

export default function ExpertAgents() {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [drainTarget, setDrainTarget] = useState<string | null>(null);
  const { canAccess } = useRole();

  const load = useCallback(async () => {
    setError(null);
    try {
      const data = await invoke<AgentInfo[]>("agent_list", {});
      setAgents(data);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const handleCordon = async (name: string) => {
    try {
      const msg = await invoke<string>("agent_cordon", { agentName: name });
      setActionMsg(msg);
      load();
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  };

  const handleUncordon = async (name: string) => {
    try {
      const msg = await invoke<string>("agent_uncordon", { agentName: name });
      setActionMsg(msg);
      load();
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  };

  const handleDrain = async () => {
    if (!drainTarget) return;
    const name = drainTarget;
    setDrainTarget(null);
    try {
      const msg = await invoke<string>("agent_drain", { agentName: name });
      setActionMsg(msg);
      load();
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  };

  return (
    <div>
      {error && <p style={{ color: "var(--danger)", fontSize: 13 }}>{error}</p>}
      {actionMsg && <p style={{ color: "var(--success)", fontSize: 13 }}>{actionMsg}</p>}

      {agents.length === 0 && !error && (
        <p style={{ color: "var(--text-secondary)" }}>暂无注册的 Agent</p>
      )}

      {agents.length > 0 && (
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th style={thStyle}>名称</th>
              <th style={thStyle}>状态</th>
              <th style={thStyle}>健康</th>
              <th style={thStyle}>在途任务</th>
              {canAccess("admin") && <th style={thStyle}>操作</th>}
            </tr>
          </thead>
          <tbody>
            {agents.map((agent) => (
              <tr key={agent.name} style={{ borderBottom: "1px solid var(--glass-border-subtle)" }}>
                <td style={tdStyle}>{agent.name}</td>
                <td style={tdStyle}>
                  <span
                    style={{
                      color: agent.lifecycle_state === "active" ? "var(--success)" : "var(--warning)",
                    }}
                  >
                    {agent.lifecycle_state}
                  </span>
                </td>
                <td style={tdStyle}>{agent.is_healthy ? "✅" : "🔴"}</td>
                <td style={tdStyle}>{agent.in_flight_items}</td>
                {canAccess("admin") && (
                  <td style={tdStyle}>
                    <div style={{ display: "flex", gap: 4 }}>
                      {agent.lifecycle_state === "active" ? (
                        <button
                          className="btn btn-ghost"
                          style={{ fontSize: 12, padding: "2px 8px" }}
                          onClick={() => handleCordon(agent.name)}
                        >
                          Cordon
                        </button>
                      ) : (
                        <button
                          className="btn btn-ghost"
                          style={{ fontSize: 12, padding: "2px 8px" }}
                          onClick={() => handleUncordon(agent.name)}
                        >
                          Uncordon
                        </button>
                      )}
                      <button
                        className="btn btn-ghost"
                        style={{ fontSize: 12, padding: "2px 8px", color: "var(--danger)" }}
                        onClick={() => setDrainTarget(agent.name)}
                      >
                        Drain
                      </button>
                    </div>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <ConfirmDialog
        open={!!drainTarget}
        title="Drain Agent"
        message={`确定要 drain agent "${drainTarget}" 吗？这将停止分配新任务并等待当前任务完成。`}
        confirmLabel="确认 Drain"
        destructive
        onConfirm={handleDrain}
        onCancel={() => setDrainTarget(null)}
      />
    </div>
  );
}

const thStyle: React.CSSProperties = {
  padding: "8px 12px",
  textAlign: "left",
  color: "var(--text-secondary)",
  fontSize: 12,
  fontWeight: 600,
};

const tdStyle: React.CSSProperties = {
  padding: "6px 12px",
  fontSize: 13,
};
