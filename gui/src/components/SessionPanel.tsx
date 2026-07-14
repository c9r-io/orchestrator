import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AgentSession, SessionOutputChunk } from "../lib/types";

interface Props {
  taskId?: string;
  sessionId?: string;
  canControl: boolean;
  onSelectSession?: (sessionId: string) => void;
}

export default function SessionPanel({ taskId, sessionId, canControl, onSelectSession }: Props) {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [selected, setSelected] = useState<AgentSession | null>(null);
  const [text, setText] = useState("");
  const [transcript, setTranscript] = useState("");
  const [fencingToken, setFencingToken] = useState<number | null>(null);
  const [leaseExpiry, setLeaseExpiry] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  const offsetsRef = useRef(new Map<string, number>());
  const transcriptsRef = useRef(new Map<string, string>());
  const clientId = useRef(`gui-${crypto.randomUUID()}`);

  const reload = useCallback(async () => {
    const rows = await invoke<AgentSession[]>("agent_session_list", { task_id: taskId ?? null });
    setSessions(rows);
    setSelected((current) => rows.find((row) => row.session_id === sessionId)
      ?? rows.find((row) => row.session_id === current?.session_id) ?? rows[0] ?? null);
  }, [sessionId, taskId]);

  useEffect(() => {
    if (!sessionId) return;
    setSelected((current) => sessions.find((row) => row.session_id === sessionId) ?? current);
  }, [sessionId, sessions]);

  useEffect(() => { reload().catch((value) => setError(String(value))); }, [reload]);

  useEffect(() => {
    if (!selected) return;
    let disposed = false;
    let unlistenOutput: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let retryTimer: number | undefined;
    const id = selected.session_id;
    const scheduleReconnect = () => {
      if (disposed || retryTimer !== undefined) return;
      retryTimer = window.setTimeout(() => {
        retryTimer = undefined;
        void connect();
      }, 1000);
    };
    const connect = async () => {
      try {
        await invoke("start_agent_session_read", {
          session_id: id,
          offset: offsetsRef.current.get(id) ?? 0,
        });
        if (!disposed) setConnected(true);
      } catch (value) {
        if (!disposed) {
          setConnected(false);
          setError(String(value));
          scheduleReconnect();
        }
      }
    };
    const start = async () => {
      unlistenOutput = await listen<SessionOutputChunk>(`agent-session-output-${id}`, ({ payload }) => {
        const committed = offsetsRef.current.get(id) ?? 0;
        if (payload.next_offset <= committed) return;
        const nextTranscript = (transcriptsRef.current.get(id) ?? "") + payload.text;
        transcriptsRef.current.set(id, nextTranscript);
        offsetsRef.current.set(id, payload.next_offset);
        setTranscript(nextTranscript);
        setConnected(!payload.eof);
      });
      unlistenError = await listen<string>(`stream-error-agent-session-${id}`, ({ payload }) => {
        setConnected(false);
        setError(payload);
        scheduleReconnect();
      });
      await connect();
    };
    setTranscript(transcriptsRef.current.get(id) ?? "");
    setError(null);
    start().catch((value) => {
      setError(String(value));
      scheduleReconnect();
    });
    return () => {
      disposed = true;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
      unlistenOutput?.();
      unlistenError?.();
      invoke("stop_agent_session_read", { session_id: id }).catch(() => {});
    };
  }, [selected?.session_id]);

  useEffect(() => {
    if (!selected || fencingToken === null) return;
    const timer = window.setInterval(() => invoke<string>("agent_session_heartbeat", { session_id: selected.session_id, client_id: clientId.current, fencing_token: fencingToken }).then(setLeaseExpiry).catch((value) => {
      setError(String(value)); setFencingToken(null); setLeaseExpiry(null); void reload();
    }), 10_000);
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
      <select value={selected?.session_id ?? ""} onChange={(event) => {
        setSelected(sessions.find((row) => row.session_id === event.target.value) ?? null);
        onSelectSession?.(event.target.value);
      }} aria-label="Select agent session">
        {sessions.map((row) => <option key={row.session_id} value={row.session_id}>{row.agent_id} · {row.step_id} · {row.state}</option>)}
      </select>
    </div>
    {selected && <div className="session-metadata">State: {selected.state} · Task: {selected.task_id} · Step: {selected.step_id} · Agent: {selected.agent_id} · Working directory: governed workspace · Reader: {connected ? "attached" : "detached"} · Writer: {selected.writer_actor ?? "none"}{leaseExpiry ? ` · Lease ${leaseExpiry}` : ""}</div>}
    <pre role="log" aria-live="polite" style={{ background: "var(--bg-secondary)", minHeight: 120, maxHeight: 320, overflow: "auto", padding: 12, borderRadius: 12, whiteSpace: "pre-wrap" }}>{transcript || "No transcript output yet."}</pre>
    {error && <p style={{ color: "var(--danger)", fontSize: 12 }}>{error}</p>}
    {canControl && selected ? <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
      {fencingToken === null ? <button className="btn btn-secondary" onClick={() => acquire().catch((value) => setError(String(value)))}>Request control</button> : <>
        <input value={text} onChange={(event) => setText(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") send().catch((value) => setError(String(value))); }} aria-label="Session input" style={{ flex: 1 }} />
        <button className="btn btn-primary" onClick={() => send().catch((value) => setError(String(value)))}>Send</button>
        <button className="btn btn-ghost" onClick={() => detach().catch((value) => setError(String(value)))}>Release control</button>
      </>}
      <button className="btn btn-destructive" onClick={() => close().catch((value) => setError(String(value)))}>Close session</button>
    </div> : selected && <p className="readonly-reason">Read-only access: writer lease, input and close controls are unavailable.</p>}
  </section>;
}
