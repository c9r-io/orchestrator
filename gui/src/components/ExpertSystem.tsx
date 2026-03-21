import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import ConfirmDialog from "./ConfirmDialog";

interface WorkerStatus {
  pending_tasks: number;
  active_workers: number;
  idle_workers: number;
  running_tasks: number;
  configured_workers: number;
  lifecycle_state: string;
  shutdown_requested: boolean;
}

interface DbStatus {
  db_path: string;
  current_version: number;
  target_version: number;
  is_current: boolean;
  pending_names: string[];
}

export default function ExpertSystem() {
  const [worker, setWorker] = useState<WorkerStatus | null>(null);
  const [db, setDb] = useState<DbStatus | null>(null);
  const [checkResult, setCheckResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [showShutdown, setShowShutdown] = useState(false);
  const { canAccess } = useRole();

  const load = useCallback(async () => {
    setError(null);
    try {
      const [w, d] = await Promise.all([
        invoke<WorkerStatus>("worker_status", {}),
        invoke<DbStatus>("db_status", {}),
      ]);
      setWorker(w);
      setDb(d);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const runCheck = async () => {
    try {
      const r = await invoke<{ content: string }>( "check", {});
      setCheckResult(r.content);
    } catch (e) { setError(typeof e === "string" ? e : String(e)); }
  };

  const handleShutdown = async () => {
    setShowShutdown(false);
    try {
      const m = await invoke<string>("shutdown", { graceful: true });
      setMsg(m);
    } catch (e) { setError(typeof e === "string" ? e : String(e)); }
  };

  const toggleMaintenance = async (enable: boolean) => {
    try {
      const r = await invoke<{ message: string }>("maintenance_mode", { enable });
      setMsg(r.message);
    } catch (e) { setError(typeof e === "string" ? e : String(e)); }
  };

  return (
    <div>
      {error && <p style={{ color: "var(--danger)", fontSize: 13 }}>{error}</p>}
      {msg && <p style={{ color: "var(--success)", fontSize: 13 }}>{msg}</p>}

      {/* Worker Status */}
      {worker && (
        <div style={{ marginBottom: 16 }}>
          <h4 style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 8 }}>Worker 状态</h4>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 8 }}>
            {[
              ["活跃", worker.active_workers],
              ["空闲", worker.idle_workers],
              ["运行中任务", worker.running_tasks],
              ["待处理任务", worker.pending_tasks],
              ["配置数", worker.configured_workers],
              ["生命周期", worker.lifecycle_state],
            ].map(([label, val]) => (
              <div key={String(label)} style={{ fontSize: 13 }}>
                <span style={{ color: "var(--text-tertiary)" }}>{label}: </span>
                <span>{String(val)}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* DB Status */}
      {db && (
        <div style={{ marginBottom: 16 }}>
          <h4 style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 8 }}>数据库状态</h4>
          <div style={{ fontSize: 13 }}>
            <p>路径: <code>{db.db_path}</code></p>
            <p>版本: {db.current_version}/{db.target_version} {db.is_current ? "✅" : "⚠️ 需迁移"}</p>
            {db.pending_names.length > 0 && (
              <p style={{ color: "var(--warning)" }}>待迁移: {db.pending_names.join(", ")}</p>
            )}
          </div>
        </div>
      )}

      {/* Actions */}
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <button className="btn btn-secondary" style={{ fontSize: 12 }} onClick={runCheck}>预检查</button>
        <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={load}>刷新</button>
        {canAccess("admin") && (
          <>
            <button className="btn btn-secondary" style={{ fontSize: 12 }}
              onClick={() => toggleMaintenance(true)}>进入维护模式</button>
            <button className="btn btn-secondary" style={{ fontSize: 12 }}
              onClick={() => toggleMaintenance(false)}>退出维护模式</button>
            <button className="btn btn-destructive" style={{ fontSize: 12 }}
              onClick={() => setShowShutdown(true)}>关闭 Daemon</button>
          </>
        )}
      </div>

      {checkResult && (
        <pre style={{ marginTop: 12, background: "var(--bg-secondary)", borderRadius: 8,
          padding: 8, fontSize: 12, whiteSpace: "pre-wrap", maxHeight: 300, overflowY: "auto" }}>
          {checkResult}
        </pre>
      )}

      <ConfirmDialog open={showShutdown} title="关闭 Daemon"
        message="确定要优雅关闭 daemon 吗？所有正在运行的任务将被中断。"
        confirmLabel="确认关闭" destructive onConfirm={handleShutdown}
        onCancel={() => setShowShutdown(false)} />
    </div>
  );
}
