import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useRole } from "../../hooks/useRole";
import type { ConsoleRoute } from "../../lib/routes";
import type { SourceConnection, SourceConnectionCatalog, SourceConnectionIntent } from "../../lib/types";

const INTENT_KEY = "orchestrator.sourceConnectionIntent.v1";

interface Props { selectedId?: string; onNavigate: (route: ConsoleRoute) => void; }

export default function SourceConnections({ selectedId, onNavigate }: Props) {
  const { canAccess } = useRole();
  const [projectId, setProjectId] = useState("default");
  const [catalog, setCatalog] = useState<SourceConnectionCatalog | null>(null);
  const [connections, setConnections] = useState<SourceConnection[]>([]);
  const [intent, setIntent] = useState<SourceConnectionIntent | null>(null);
  const [label, setLabel] = useState("Slack workspace");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pollRef = useRef<number | null>(null);

  const stopPolling = useCallback(() => {
    if (pollRef.current !== null) window.clearInterval(pollRef.current);
    pollRef.current = null;
  }, []);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [nextCatalog, nextConnections] = await Promise.all([
        invoke<SourceConnectionCatalog>("source_connection_catalog_get"),
        invoke<SourceConnection[]>("source_connection_list", { project_id: projectId, include_disconnected: false }),
      ]);
      setCatalog(nextCatalog);
      setConnections(nextConnections);
    } catch (cause) { setError(String(cause)); }
  }, [projectId]);

  const pollIntent = useCallback(async (id: string, project: string) => {
    try {
      const next = await invoke<SourceConnectionIntent>("source_connection_intent_get", { project_id: project, intent_id: id });
      setIntent(next);
      if (next.status === "completed") {
        localStorage.removeItem(INTENT_KEY);
        stopPolling();
        await load();
      } else if (next.status !== "pending") {
        localStorage.removeItem(INTENT_KEY);
        stopPolling();
      }
    } catch (cause) { setError(String(cause)); }
  }, [load, stopPolling]);

  useEffect(() => {
    void load();
    let unlisten: UnlistenFn | undefined;
    void listen("source-connection-delta", () => void load()).then((value) => { unlisten = value; });
    void invoke("start_source_connection_watch", { project_id: projectId, after_cursor: null });
    return () => { unlisten?.(); void invoke("stop_source_connection_watch"); };
  }, [load, projectId]);

  useEffect(() => {
    const saved = localStorage.getItem(INTENT_KEY);
    if (!saved) return;
    try {
      const value = JSON.parse(saved) as { id: string; project: string };
      setProjectId(value.project);
      void pollIntent(value.id, value.project);
      stopPolling();
      pollRef.current = window.setInterval(() => void pollIntent(value.id, value.project), 2000);
    } catch { localStorage.removeItem(INTENT_KEY); }
    return stopPolling;
  }, [pollIntent, stopPolling]);

  const connect = async () => {
    setBusy(true); setError(null);
    try {
      const next = await invoke<SourceConnectionIntent>("source_connection_connect", {
        project_id: projectId, display_label: label,
        reason: "Connect official Orchestrator Slack App",
        idempotency_key: `gui-connect-${crypto.randomUUID()}`,
      });
      setIntent(next);
      localStorage.setItem(INTENT_KEY, JSON.stringify({ id: next.id, project: projectId }));
      if (next.authorize_url) await invoke("open_source_connection_oauth", { authorize_url: next.authorize_url });
      stopPolling();
      pollRef.current = window.setInterval(() => void pollIntent(next.id, projectId), 2000);
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const cancel = async () => {
    if (!intent) return;
    setBusy(true);
    try {
      const next = await invoke<SourceConnectionIntent>("source_connection_cancel", {
        project_id: projectId, intent_id: intent.id, reason: "Cancel Slack OAuth installation",
        idempotency_key: `gui-cancel-${crypto.randomUUID()}`,
      });
      setIntent(next); localStorage.removeItem(INTENT_KEY); stopPolling();
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const reauthorize = async (connection: SourceConnection) => {
    setBusy(true); setError(null);
    try {
      const next = await invoke<SourceConnectionIntent>("source_connection_reauthorize", {
        project_id: projectId, id: connection.id, expected_version: connection.version,
        reason: "Reauthorize managed Slack connection", idempotency_key: `gui-reauth-${crypto.randomUUID()}`,
      });
      setIntent(next); localStorage.setItem(INTENT_KEY, JSON.stringify({ id: next.id, project: projectId }));
      if (next.authorize_url) await invoke("open_source_connection_oauth", { authorize_url: next.authorize_url });
      stopPolling();
      pollRef.current = window.setInterval(() => void pollIntent(next.id, projectId), 2000);
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const disconnect = async (connection: SourceConnection) => {
    if (!window.confirm(`Disconnect ${connection.display_label}? Existing task evidence is retained.`)) return;
    setBusy(true); setError(null);
    try {
      await invoke("source_connection_disconnect", {
        project_id: projectId, id: connection.id, expected_version: connection.version,
        reason: "Disconnect managed Slack connection", idempotency_key: `gui-disconnect-${crypto.randomUUID()}`,
      });
      await load();
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const shared = catalog?.modes.find((mode) => mode.mode === "managed_shared");
  const selected = selectedId ? connections.find((value) => value.id === selectedId) : undefined;
  return <section aria-labelledby="connections-heading">
    <div className="pane-heading"><div><h2 id="connections-heading">Slack connections</h2><p>Install the official app once. Credentials remain in the Gateway and never enter task context.</p></div><label className="source-filter">Project<input value={projectId} onChange={(event) => setProjectId(event.target.value)} /></label></div>
    {error && <p role="alert" className="attention-error">{error}</p>}
    <div className="connection-mode-grid" aria-label="Slack provisioning modes">
      <article className="liquid-glass connection-mode-card"><span className="badge badge-info">Recommended</span><h3>Instant — Official Orchestrator App</h3><p>One Slack consent screen, managed delivery, automatic Trigger setup.</p>{shared?.available ? canAccess("admin") ? <div className="connection-connect"><label>Connection label<input value={label} onChange={(event) => setLabel(event.target.value)} /></label><button className="btn btn-primary" disabled={busy || intent?.status === "pending"} onClick={() => void connect()}>Connect workspace</button></div> : <p>Ask an administrator to connect a workspace.</p> : <p className="field-warning">{shared?.unavailable_reason ?? "Gateway capability unavailable"}</p>}</article>
      <article className="liquid-glass connection-mode-card is-disabled" aria-disabled="true"><span className="badge">Planned</span><h3>Dedicated — Private workspace app</h3><p>Reserved for FR-115. It will never silently fall back to the shared app.</p></article>
      <article className="liquid-glass connection-mode-card"><span className="badge">Manual</span><h3>Existing app — Manual credentials</h3><p>Keep using SecretStore + Trigger when the workspace already owns an app.</p><a className="btn btn-ghost" href="#/sources/automations/bindings">Open automation setup</a></article>
    </div>
    {intent?.status === "pending" && <div className="connection-intent" role="status"><div><strong>Waiting for Slack consent</strong><p>This page can be refreshed safely. Intent expires at {intent.expires_at}.</p></div><div className="decision-actions">{intent.authorize_url && <button className="btn btn-secondary" onClick={() => void invoke("open_source_connection_oauth", { authorize_url: intent.authorize_url })}>Open Slack again</button>}<button className="btn btn-ghost" disabled={busy} onClick={() => void cancel()}>Cancel</button></div></div>}
    {intent && intent.status !== "pending" && intent.status !== "completed" && <p role="alert" className="attention-error">OAuth {intent.status}: {intent.error_code ?? "No credential was stored"}</p>}
    <div className="connection-list" role="list" aria-live="polite">
      {connections.map((connection) => <article key={connection.id} role="listitem" className={`liquid-glass connection-card ${selected?.id === connection.id ? "selected" : ""}`}><button className="connection-card-main" onClick={() => onNavigate({ page: "sources", section: "connections", resourceId: connection.id })}><span><strong>{connection.display_label}</strong><span className={`badge ${connection.state === "active" ? "badge-success" : ""}`}>{connection.state}</span></span><small>{connection.provisioning_mode} · generation {connection.generation} · {connection.trigger_name ?? "Trigger pending"}</small><small>Last delivery: {connection.last_delivery_at ?? "No events yet"} · cursor {connection.last_acked_cursor}</small></button>{canAccess("admin") && <div className="decision-actions"><button className="btn btn-ghost" disabled={busy} onClick={() => void reauthorize(connection)}>Reauthorize</button><button className="btn btn-danger" disabled={busy} onClick={() => void disconnect(connection)}>Disconnect</button></div>}{connection.last_error_code && <p className="field-error">{connection.last_error_code}</p>}</article>)}
      {connections.length === 0 && <div className="operations-state">No Slack connections in this project.</div>}
    </div>
    {intent?.connection && <div className="connection-next-steps"><h3>Connection active</h3><p>Reaction routing stays disabled until you explicitly configure a template and binding.</p><a className="btn btn-primary" href="#/sources/automations/templates">Configure badge automation</a></div>}
  </section>;
}
