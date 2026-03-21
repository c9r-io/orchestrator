import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import ConfirmDialog from "./ConfirmDialog";
import i18n from "../lib/i18n";

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
          <h4 style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 8 }}>{i18n.expertSystem.workerTitle}</h4>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 8 }}>
            {[
              [i18n.expertSystem.active, worker.active_workers],
              [i18n.expertSystem.idle, worker.idle_workers],
              [i18n.expertSystem.runningTasks, worker.running_tasks],
              [i18n.expertSystem.pendingTasks, worker.pending_tasks],
              [i18n.expertSystem.configuredCount, worker.configured_workers],
              [i18n.expertSystem.lifecycle, worker.lifecycle_state],
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
          <h4 style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 8 }}>{i18n.expertSystem.dbTitle}</h4>
          <div style={{ fontSize: 13 }}>
            <p>{i18n.expertSystem.dbPath}: <code>{db.db_path}</code></p>
            <p>{i18n.expertSystem.dbVersion(db.current_version, db.target_version)} {db.is_current ? "\u2705" : `\u26A0\uFE0F ${i18n.expertSystem.dbNeedsMigration}`}</p>
            {db.pending_names.length > 0 && (
              <p style={{ color: "var(--warning)" }}>{i18n.expertSystem.dbPendingMigrations(db.pending_names.join(", "))}</p>
            )}
          </div>
        </div>
      )}

      {/* Actions */}
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <button className="btn btn-secondary" style={{ fontSize: 12 }} onClick={runCheck}>{i18n.expertSystem.precheck}</button>
        <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={load}>{i18n.common.refresh}</button>
        {canAccess("admin") && (
          <>
            <button className="btn btn-secondary" style={{ fontSize: 12 }}
              onClick={() => toggleMaintenance(true)}>{i18n.expertSystem.enterMaintenance}</button>
            <button className="btn btn-secondary" style={{ fontSize: 12 }}
              onClick={() => toggleMaintenance(false)}>{i18n.expertSystem.exitMaintenance}</button>
            <button className="btn btn-destructive" style={{ fontSize: 12 }}
              onClick={() => setShowShutdown(true)}>{i18n.expertSystem.shutdownDaemon}</button>
          </>
        )}
      </div>

      {checkResult && (
        <pre style={{ marginTop: 12, background: "var(--bg-secondary)", borderRadius: 8,
          padding: 8, fontSize: 12, whiteSpace: "pre-wrap", maxHeight: 300, overflowY: "auto" }}>
          {checkResult}
        </pre>
      )}

      <ConfirmDialog open={showShutdown} title={i18n.expertSystem.shutdownTitle}
        message={i18n.expertSystem.shutdownMessage}
        confirmLabel={i18n.expertSystem.shutdownConfirm} destructive onConfirm={handleShutdown}
        onCancel={() => setShowShutdown(false)} />
    </div>
  );
}
