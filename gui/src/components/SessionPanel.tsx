import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AgentSession, SessionOutputChunk } from "../lib/types";

interface Props { taskId: string; canControl: boolean; }

export default function SessionPanel({ taskId, canControl }: Props) {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [selected, setSelected] = useState<AgentSession | null>(null);
  const [text, setText] = useState("");
  const [transcript, setTranscript] = useState("");
  const [fencingToken, setFencingToken] = useState<number | null>(null);
  const [leaseExpiry, setLeaseExpiry] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  const offsetRef = useRef(0);
  const clientId = useRef(`gui-${crypto.randomUUID()}`);

  const reload = useCallback(async () => {
    const rows = await invoke<AgentSession[]>("agent_session_list", { task_id: taskId });
    setSessions(rows);
    setSelected((current) => rows.find((row) => row.session_id === current?.session_id) ?? rows[0] ?? null);
  }, [taskId]);

  useEffect(() => { reload().catch((value) => setError(String(value))); }, [reload]);

  useEffect(() => {
    if (!selected) return;
    let disposed = false;
    let unlistenOutput: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    const start = async () => {
      unlistenOutput = await listen<SessionOutputChunk>(`agent-session-output-${selected.session_id}`, ({ payload }) => {
        if (payload.next_offset <= offsetRef.current) return;
        setTranscript((current) => current + payload.text);
        offsetRef.current = payload.next_offset;
        setConnected(!payload.eof);
      });
      unlistenError = await listen<string>(`stream-error-agent-session-${selected.session_id}`, ({ payload }) => {
        setConnected(false); setError(payload);
        if (!disposed) window.setTimeout(() => invoke("start_agent_session_read", { session_id: selected.session_id, offset: offsetRef.current }).catch(() => {}), 1000);
      });
      await invoke("start_agent_session_read", { session_id: selected.session_id, offset: offsetRef.current });
      setConnected(true);
    };
    setTranscript(""); offsetRef.current = 0; setError(null);
    start().catch((value) => setError(String(value)));
    return () => { disposed = true; unlistenOutput?.(); unlistenError?.(); invoke("stop_agent_session_read", { session_id: selected.session_id }).catch(() => {}); };
  }, [selected?.session_id]);

  useEffect(() => {
    if (!selected || fencingToken === null) return;
    const timer = window.setInterval(() => invoke<string>("agent_session_heartbeat", { session_id: selected.session_id, client_id: clientId.current, fencing_token: fencingToken }).then(setLeaseExpiry).catch((value) => { setError(String(value)); setFencingToken(null); }), 10_000);
    return () => window.clearInterval(timer);
  }, [selected, fencingToken]);

  const acquire = async () => {
    if (!selected) return;
    const lease = await invoke<{ fencing_token: number; lease_expires_at: string }>("agent_session_attach", { session_id: selected.session_id, client_id: clientId.current, mode: "writer" });
    setFencingToken(lease.fencing_token); setLeaseExpiry(lease.lease_expires_at); await reload();
  };
  const send = async () => {
    if (!selected || fencingToken === null || !text) return;
    await invoke("agent_session_send_input", { session_id: selected.session_id, client_id: clientId.current, fencing_token: fencingToken, text, idempotency_key: crypto.randomUUID() });
    setText("");
  };
  const detach = async () => {
    if (!selected || fencingToken === null) return;
    await invoke("agent_session_detach", { session_id: selected.session_id, client_id: clientId.current, mode: "writer", fencing_token: fencingToken });
    setFencingToken(null); setLeaseExpiry(null); await reload();
  };
  const close = async () => {
    if (!selected) return;
    await invoke("agent_session_close", { session_id: selected.session_id, state_version: selected.state_version, reason: "Closed from task detail", idempotency_key: crypto.randomUUID() });
    await reload();
  };

  if (sessions.length === 0) return null;
  return <section className="liquid-glass" style={{ marginBottom: 16 }} aria-label="Agent sessions">
    <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
      <h3 style={{ flex: 1, fontSize: 14 }}>Agent session</h3>
      <span style={{ color: connected ? "var(--success)" : "var(--text-tertiary)", fontSize: 12 }}>{connected ? "Following" : "Disconnected"}</span>
      <select value={selected?.session_id ?? ""} onChange={(event) => setSelected(sessions.find((row) => row.session_id === event.target.value) ?? null)} aria-label="Select agent session">
        {sessions.map((row) => <option key={row.session_id} value={row.session_id}>{row.agent_id} · {row.step_id} · {row.state}</option>)}
      </select>
    </div>
    {selected && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 8 }}>State: {selected.state} · PID: {selected.pid} · Writer: {selected.writer_actor ?? "none"}{leaseExpiry ? ` · Lease ${leaseExpiry}` : ""}</div>}
    <pre role="log" aria-live="polite" style={{ background: "var(--bg-secondary)", minHeight: 120, maxHeight: 320, overflow: "auto", padding: 12, borderRadius: 12, whiteSpace: "pre-wrap" }}>{transcript || "No transcript output yet."}</pre>
    {error && <p style={{ color: "var(--danger)", fontSize: 12 }}>{error}</p>}
    {canControl && selected && <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
      {fencingToken === null ? <button className="btn btn-secondary" onClick={() => acquire().catch((value) => setError(String(value)))}>Request control</button> : <>
        <input value={text} onChange={(event) => setText(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") send().catch((value) => setError(String(value))); }} aria-label="Session input" style={{ flex: 1 }} />
        <button className="btn btn-primary" onClick={() => send().catch((value) => setError(String(value)))}>Send</button>
        <button className="btn btn-ghost" onClick={() => detach().catch((value) => setError(String(value)))}>Release control</button>
      </>}
      <button className="btn btn-destructive" onClick={() => close().catch((value) => setError(String(value)))}>Close session</button>
    </div>}
  </section>;
}
