import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentSession } from "../lib/types";

export default function SessionList({ onSelect }: { onSelect: (id: string) => void }) {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [state, setState] = useState("active");
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async () => {
    try { setSessions(await invoke<AgentSession[]>("agent_session_list", { task_id: null })); }
    catch (reason) { setError(String(reason)); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  const visible = useMemo(() => sessions.filter((session) => state === "all"
    || (state === "active" ? !["closed", "exited", "failed"].includes(session.state) : session.state === state)), [sessions, state]);
  return <main aria-labelledby="sessions-title">
    <header className="page-heading"><div><h1 id="sessions-title" className="page-title">Sessions</h1><p>Observe active and detached agent sessions, then acquire writer control explicitly.</p></div><button className="btn btn-ghost" onClick={load}>Refresh</button></header>
    <div className="toolbar"><label>State <select value={state} onChange={(event) => setState(event.target.value)}><option value="active">Active</option><option value="detached">Detached</option><option value="closed">Closed</option><option value="all">All</option></select></label></div>
    {error && <p role="alert" className="inline-error">{error}</p>}
    <div className="dense-list" role="list">
      {visible.map((session) => <button key={session.session_id} className="dense-row session-row" role="listitem" onClick={() => onSelect(session.session_id)}>
        <span className={`status-shape status-${session.state}`} aria-hidden="true" />
        <span><strong>{session.agent_id}</strong><small>{session.task_id} · {session.step_id}</small></span>
        <span className="badge">{session.state}</span><span>{session.writer_actor ? `writer: ${session.writer_actor}` : "read-only"}</span>
      </button>)}
      {!error && visible.length === 0 && <p className="empty-state">No sessions match this view.</p>}
    </div>
  </main>;
}
