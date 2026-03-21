import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import type { ResourceResult } from "../lib/types";

interface TriggerAction {
  name: string;
  status: "running" | "done" | "error";
  message: string;
}

export default function ExpertTrigger() {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastAction, setLastAction] = useState<TriggerAction | null>(null);
  const [triggerName, setTriggerName] = useState("");
  const { canAccess } = useRole();

  const load = useCallback(async () => {
    setError(null);
    try {
      const result = await invoke<ResourceResult>("resource_get", {
        resource: "triggers",
        outputFormat: "yaml",
      });
      setContent(result.content);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const doAction = async (action: "trigger_suspend" | "trigger_resume" | "trigger_fire") => {
    if (!triggerName.trim()) return;
    setError(null);
    try {
      if (action === "trigger_fire") {
        const r = await invoke<{ task_id: string; message: string }>(action, {
          triggerName: triggerName.trim(),
        });
        setLastAction({ name: triggerName, status: "done", message: `${r.message} (task: ${r.task_id})` });
      } else {
        const m = await invoke<string>(action, { triggerName: triggerName.trim() });
        setLastAction({ name: triggerName, status: "done", message: m });
      }
      load();
    } catch (e) {
      setLastAction({ name: triggerName, status: "error", message: typeof e === "string" ? e : String(e) });
    }
  };

  return (
    <div>
      {error && <p style={{ color: "var(--danger)", fontSize: 13 }}>{error}</p>}
      {lastAction && (
        <p style={{ color: lastAction.status === "error" ? "var(--danger)" : "var(--success)", fontSize: 13 }}>
          {lastAction.message}
        </p>
      )}

      {/* Trigger name input + actions */}
      {canAccess("operator") && (
        <div style={{ display: "flex", gap: 4, marginBottom: 12, alignItems: "center" }}>
          <input placeholder="trigger 名称" value={triggerName}
            onChange={(e) => setTriggerName(e.target.value)}
            style={{ flex: 1, padding: "4px 8px", borderRadius: 8,
              border: "1px solid var(--glass-border-subtle)",
              background: "var(--bg-secondary)", color: "var(--text-primary)", fontSize: 13 }} />
          <button className="btn btn-secondary" style={{ fontSize: 12 }}
            onClick={() => doAction("trigger_suspend")} disabled={!triggerName}>暂停</button>
          <button className="btn btn-secondary" style={{ fontSize: 12 }}
            onClick={() => doAction("trigger_resume")} disabled={!triggerName}>恢复</button>
          <button className="btn btn-primary" style={{ fontSize: 12 }}
            onClick={() => doAction("trigger_fire")} disabled={!triggerName}>触发</button>
        </div>
      )}

      {/* Trigger list */}
      {content && (
        <pre style={{ background: "var(--bg-secondary)", borderRadius: 8, padding: 8,
          fontSize: 12, whiteSpace: "pre-wrap", maxHeight: 400, overflowY: "auto" }}>
          {content}
        </pre>
      )}
      {!content && !error && (
        <p style={{ color: "var(--text-tertiary)", fontSize: 13 }}>暂无触发器</p>
      )}
    </div>
  );
}
