import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useRole } from "../../hooks/useRole";
import type { ConsoleRoute } from "../../lib/routes";
import type { DedicatedLifecycle, DedicatedProvisioning, SourceConnection, SourceConnectionCatalog, SourceConnectionIntent } from "../../lib/types";
import ReviewedActionDialog from "../../components/ReviewedActionDialog";
import SourceConnectionTransferDialog from "./SourceConnectionTransferDialog";

const INTENT_KEY = "orchestrator.sourceConnectionIntent.v1";
const DEDICATED_KEY = "orchestrator.dedicatedSlackProvisioning.v1";

interface Props { selectedId?: string; onNavigate: (route: ConsoleRoute) => void; }

export default function SourceConnections({ selectedId, onNavigate }: Props) {
  const { canAccess } = useRole();
  const [projectId, setProjectId] = useState("default");
  const [catalog, setCatalog] = useState<SourceConnectionCatalog | null>(null);
  const [connections, setConnections] = useState<SourceConnection[]>([]);
  const [intent, setIntent] = useState<SourceConnectionIntent | null>(null);
  const [label, setLabel] = useState("Slack workspace");
  const [dedicatedLabel, setDedicatedLabel] = useState("Private Slack workspace");
  const [configToken, setConfigToken] = useState("");
  const [migrationTarget, setMigrationTarget] = useState("");
  const [dedicated, setDedicated] = useState<DedicatedProvisioning | null>(null);
  const [reviewDedicated, setReviewDedicated] = useState(false);
  const [lifecycleTarget, setLifecycleTarget] = useState<SourceConnection | null>(null);
  const [lifecycleToken, setLifecycleToken] = useState("");
  const [lifecycle, setLifecycle] = useState<DedicatedLifecycle | null>(null);
  const [reviewLifecycle, setReviewLifecycle] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<SourceConnection | null>(null);
  const [deleteToken, setDeleteToken] = useState("");
  const [deleteAppId, setDeleteAppId] = useState("");
  const [deleteReason, setDeleteReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [reviewedAction, setReviewedAction] = useState<{ kind: "reauthorize" | "disconnect" | "migrate_to_shared"; connection: SourceConnection } | null>(null);
  const [transferConnection, setTransferConnection] = useState<SourceConnection | null>(null);
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
        invoke<SourceConnection[]>("source_connection_list", { project_id: projectId, include_disconnected: true }),
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

  useEffect(() => {
    const saved = localStorage.getItem(DEDICATED_KEY);
    if (!saved) return;
    try {
      const value = JSON.parse(saved) as { id: string; project: string };
      if (!value.id || !value.project) throw new Error("invalid provisioning checkpoint");
      setProjectId(value.project);
      void invoke<DedicatedProvisioning>("source_connection_dedicated_get", {
        project_id: value.project,
        provisioning_id: value.id,
      }).then((next) => {
        setDedicated(next);
        if (["completed", "abandoned"].includes(next.status)) localStorage.removeItem(DEDICATED_KEY);
      }).catch((cause) => setError(String(cause)));
    } catch { localStorage.removeItem(DEDICATED_KEY); }
  }, []);

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

  const previewDedicated = async () => {
    const token = configToken;
    setConfigToken("");
    setBusy(true); setError(null);
    try {
      const next = await invoke<DedicatedProvisioning>("source_connection_dedicated_preview", {
        project_id: projectId,
        display_label: dedicatedLabel,
        config_token: token,
        reason: "Validate the fixed dedicated Slack App manifest",
        idempotency_key: `gui-dedicated-preview-${crypto.randomUUID()}`,
        target_connection_id: migrationTarget || null,
      });
      setDedicated(next);
      localStorage.setItem(DEDICATED_KEY, JSON.stringify({ id: next.id, project: projectId }));
    } catch (cause) { setError(String(cause)); } finally {
      setConfigToken("");
      setBusy(false);
    }
  };

  const previewLifecycle = async () => {
    if (!lifecycleTarget) return;
    const token = lifecycleToken;
    setLifecycleToken(""); setBusy(true); setError(null);
    try {
      const next = await invoke<DedicatedLifecycle>("source_connection_dedicated_upgrade_preview", {
        project_id: projectId, id: lifecycleTarget.id, expected_version: lifecycleTarget.version,
        config_token: token, reason: "Review fixed dedicated Slack App manifest upgrade",
        idempotency_key: `gui-dedicated-upgrade-preview-${crypto.randomUUID()}`,
      });
      setLifecycle(next);
    } catch (cause) { setError(String(cause)); } finally { setLifecycleToken(""); setBusy(false); }
  };

  const applyLifecycle = async (reason: string) => {
    if (!lifecycleTarget || !lifecycle) return;
    setBusy(true); setError(null);
    try {
      const next = await invoke<DedicatedLifecycle>("source_connection_dedicated_upgrade_apply", {
        project_id: projectId, id: lifecycleTarget.id, expected_version: lifecycleTarget.version,
        lifecycle_id: lifecycle.lifecycle_id, reason,
        idempotency_key: `gui-dedicated-upgrade-apply-${crypto.randomUUID()}`,
      });
      setLifecycle(next);
      if (next.oauth_intent_id) {
        localStorage.setItem(INTENT_KEY, JSON.stringify({ id: next.oauth_intent_id, project: projectId }));
        void pollIntent(next.oauth_intent_id, projectId);
        stopPolling(); pollRef.current = window.setInterval(() => void pollIntent(next.oauth_intent_id!, projectId), 2000);
      }
      if (next.authorize_url) await invoke("open_source_connection_oauth", { authorize_url: next.authorize_url });
      await load();
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const migrateToShared = async (connection: SourceConnection, reason: string) => {
    setBusy(true); setError(null);
    try {
      const next = await invoke<SourceConnectionIntent>("source_connection_migrate_to_shared", {
        project_id: projectId, id: connection.id, expected_version: connection.version, reason,
        idempotency_key: `gui-migrate-shared-${crypto.randomUUID()}`,
      });
      setIntent(next); localStorage.setItem(INTENT_KEY, JSON.stringify({ id: next.id, project: projectId }));
      if (next.authorize_url) await invoke("open_source_connection_oauth", { authorize_url: next.authorize_url });
      stopPolling(); pollRef.current = window.setInterval(() => void pollIntent(next.id, projectId), 2000);
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const deleteDedicatedApp = async () => {
    if (!deleteTarget) return;
    const token = deleteToken; const typedAppId = deleteAppId;
    setDeleteToken(""); setDeleteAppId(""); setBusy(true); setError(null);
    try {
      await invoke("source_connection_dedicated_delete", {
        project_id: projectId, id: deleteTarget.id, expected_version: deleteTarget.version,
        config_token: token, typed_app_id: typedAppId, reason: deleteReason,
        idempotency_key: `gui-dedicated-delete-${crypto.randomUUID()}`,
      });
      setDeleteTarget(null); setDeleteReason(""); await load();
    } catch (cause) { setError(String(cause)); } finally { setDeleteToken(""); setDeleteAppId(""); setBusy(false); }
  };

  const approveDedicated = async (reason: string) => {
    if (!dedicated) return;
    setBusy(true); setError(null);
    try {
      const next = await invoke<DedicatedProvisioning>("source_connection_dedicated_approve", {
        project_id: projectId,
        provisioning_id: dedicated.id,
        reason,
        idempotency_key: `gui-dedicated-approve-${crypto.randomUUID()}`,
      });
      setDedicated(next);
      localStorage.setItem(DEDICATED_KEY, JSON.stringify({ id: next.id, project: projectId }));
      if (next.oauth_intent_id) {
        localStorage.setItem(INTENT_KEY, JSON.stringify({ id: next.oauth_intent_id, project: projectId }));
        void pollIntent(next.oauth_intent_id, projectId);
        stopPolling();
        pollRef.current = window.setInterval(() => void pollIntent(next.oauth_intent_id!, projectId), 2000);
      }
      if (next.authorize_url) await invoke("open_source_connection_oauth", { authorize_url: next.authorize_url });
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const abandonDedicated = async () => {
    if (!dedicated) return;
    setBusy(true); setError(null);
    try {
      const next = await invoke<DedicatedProvisioning>("source_connection_dedicated_abandon", {
        project_id: projectId,
        provisioning_id: dedicated.id,
        reason: "Abandon dedicated Slack App provisioning",
        idempotency_key: `gui-dedicated-abandon-${crypto.randomUUID()}`,
      });
      setDedicated(next); localStorage.removeItem(DEDICATED_KEY);
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const reauthorize = async (connection: SourceConnection, reason: string) => {
    setBusy(true); setError(null);
    try {
      const next = await invoke<SourceConnectionIntent>("source_connection_reauthorize", {
        project_id: projectId, id: connection.id, expected_version: connection.version,
        reason, idempotency_key: `gui-reauth-${crypto.randomUUID()}`,
      });
      setIntent(next); localStorage.setItem(INTENT_KEY, JSON.stringify({ id: next.id, project: projectId }));
      if (next.authorize_url) await invoke("open_source_connection_oauth", { authorize_url: next.authorize_url });
      stopPolling();
      pollRef.current = window.setInterval(() => void pollIntent(next.id, projectId), 2000);
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const disconnect = async (connection: SourceConnection, reason: string) => {
    setBusy(true); setError(null);
    try {
      await invoke("source_connection_disconnect", {
        project_id: projectId, id: connection.id, expected_version: connection.version,
        reason, idempotency_key: `gui-disconnect-${crypto.randomUUID()}`,
      });
      await load();
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const transfer = async (connection: SourceConnection, targetDaemonId: string, reason: string) => {
    setBusy(true); setError(null);
    try {
      await invoke("source_connection_transfer", {
        project_id: projectId, id: connection.id, expected_version: connection.version,
        target_daemon_id: targetDaemonId, reason,
        idempotency_key: `gui-transfer-${crypto.randomUUID()}`,
      });
      setTransferConnection(null); await load();
    } catch (cause) { setError(String(cause)); } finally { setBusy(false); }
  };

  const shared = catalog?.modes.find((mode) => mode.mode === "managed_shared");
  const dedicatedMode = catalog?.modes.find((mode) => mode.mode === "managed_dedicated");
  const sharedConnections = connections.filter((value) => value.provisioning_mode === "managed_shared" && value.state === "active");
  const selected = selectedId ? connections.find((value) => value.id === selectedId) : undefined;
  return <section aria-labelledby="connections-heading">
    <div className="pane-heading"><div><h2 id="connections-heading">Slack connections</h2><p>Install the official app once. Credentials remain in the Gateway and never enter task context.</p></div><label className="source-filter">Project<input value={projectId} onChange={(event) => setProjectId(event.target.value)} /></label></div>
    {error && <p role="alert" className="attention-error">{error}</p>}
    <div className="connection-mode-grid" aria-label="Slack provisioning modes">
      <article className="liquid-glass connection-mode-card"><span className="badge badge-info">Recommended</span><h3>Instant — Official Orchestrator App</h3><p>One Slack consent screen, managed delivery, automatic Trigger setup.</p>{shared?.available ? canAccess("admin") ? <div className="connection-connect"><label>Connection label<input value={label} onChange={(event) => setLabel(event.target.value)} /></label><button className="btn btn-primary" disabled={busy || intent?.status === "pending"} onClick={() => void connect()}>Connect workspace</button></div> : <p>Ask an administrator to connect a workspace.</p> : <p className="field-warning">{shared?.unavailable_reason ?? "Gateway capability unavailable"}</p>}</article>
      <article className={`liquid-glass connection-mode-card ${dedicatedMode?.available ? "" : "is-disabled"}`} aria-disabled={!dedicatedMode?.available}><span className="badge">Advanced isolation</span><h3>Dedicated — Private workspace app</h3><p>Creates one workspace-owned app with isolated credentials and event URL. Requires a short-lived Slack Configuration Token plus OAuth consent; it never falls back to the shared app.</p>{dedicatedMode?.available ? canAccess("admin") ? <div className="connection-connect"><label>Dedicated connection label<input value={dedicatedLabel} onChange={(event) => setDedicatedLabel(event.target.value)} /></label><label>Migration source (optional)<select value={migrationTarget} onChange={(event) => setMigrationTarget(event.target.value)}><option value="">New workspace connection</option>{sharedConnections.map((connection) => <option key={connection.id} value={connection.id}>Replace {connection.display_label}</option>)}</select></label><label>One-time Configuration Token<input type="password" autoComplete="off" spellCheck={false} value={configToken} onChange={(event) => setConfigToken(event.target.value)} /></label><button className="btn btn-secondary" disabled={busy || !configToken.trim() || dedicated?.status === "awaiting_approval"} onClick={() => void previewDedicated()}>Validate manifest</button></div> : <p>Ask an administrator to provision a private workspace app.</p> : <p className="field-warning">{dedicatedMode?.unavailable_reason ?? "Gateway capability unavailable"}</p>}</article>
      <article className="liquid-glass connection-mode-card"><span className="badge">Manual</span><h3>Existing app — Manual credentials</h3><p>Keep using SecretStore + Trigger when the workspace already owns an app.</p><a className="btn btn-ghost" href="#/sources/automations/bindings">Open automation setup</a></article>
    </div>
    {dedicated && !["completed", "abandoned"].includes(dedicated.status) && <section className="dedicated-provisioning" aria-labelledby="dedicated-provisioning-heading"><div className="dedicated-provisioning-heading"><div><span className="badge">{dedicated.status}</span><h3 id="dedicated-provisioning-heading">Dedicated app provisioning review</h3><p>Manifest {dedicated.manifest_version} · digest {dedicated.manifest_digest.slice(0, 12)}… · expires {dedicated.expires_at}</p></div></div>{dedicated.diff.length > 0 && <div className="dedicated-diff" role="list" aria-label="Dedicated app manifest changes">{dedicated.diff.map((entry) => <article key={entry.field} role="listitem" className={entry.permission_expansion ? "permission-expansion" : ""}><strong>{entry.field}</strong><span>{entry.change}{entry.permission_expansion ? " · permission expansion" : ""}</span><small>Before: {entry.before.join(", ") || "none"}</small><small>After: {entry.after.join(", ") || "none"}</small></article>)}</div>}<div className="decision-actions">{dedicated.status === "awaiting_approval" && canAccess("admin") && <button className="btn btn-primary" disabled={busy} onClick={() => setReviewDedicated(true)}>Approve and create app</button>}{dedicated.status === "handoff_pending" && canAccess("admin") && <button className="btn btn-primary" disabled={busy} onClick={() => setReviewDedicated(true)}>Resume secure import</button>}{dedicated.authorize_url && <button className="btn btn-secondary" onClick={() => void invoke("open_source_connection_oauth", { authorize_url: dedicated.authorize_url })}>Open Slack consent</button>}{canAccess("admin") && !["creating", "oauth_pending"].includes(dedicated.status) && <button className="btn btn-ghost" disabled={busy} onClick={() => void abandonDedicated()}>Abandon</button>}</div>{dedicated.error_code && <p role="alert" className="attention-error">{dedicated.error_code}. The daemon will not create another App automatically.</p>}</section>}
    {lifecycleTarget && <section className="dedicated-provisioning" aria-labelledby="dedicated-upgrade-heading"><h3 id="dedicated-upgrade-heading">Upgrade {lifecycleTarget.display_label}</h3><p>Export and compare the exact workspace App before applying the fixed manifest. A permission expansion suspends delivery until OAuth completes.</p><label>Fresh Configuration Token<input type="password" autoComplete="off" spellCheck={false} value={lifecycleToken} onChange={(event) => setLifecycleToken(event.target.value)} /></label><div className="decision-actions"><button className="btn btn-secondary" disabled={busy || !lifecycleToken.trim()} onClick={() => void previewLifecycle()}>Validate upgrade</button><button className="btn btn-ghost" onClick={() => { setLifecycleTarget(null); setLifecycle(null); setLifecycleToken(""); }}>Cancel upgrade</button></div>{lifecycle && <><div className="dedicated-diff" role="list" aria-label="Dedicated app upgrade changes">{lifecycle.diff.map((entry) => <article key={entry.field} role="listitem" className={entry.permission_expansion ? "permission-expansion" : ""}><strong>{entry.field}</strong><span>{entry.change}{entry.permission_expansion ? " · permission expansion" : ""}</span><small>Before: {entry.before.join(", ") || "none"}</small><small>After: {entry.after.join(", ") || "none"}</small></article>)}</div>{lifecycle.status === "awaiting_approval" && <button className="btn btn-primary" disabled={busy} onClick={() => setReviewLifecycle(true)}>Approve manifest upgrade</button>}</>}</section>}
    {deleteTarget && <section className="dedicated-provisioning" aria-labelledby="dedicated-delete-heading"><h3 id="dedicated-delete-heading">Delete workspace-owned Slack App</h3><p>This is separate from Disconnect and is irreversible. Enter a fresh Configuration Token and the exact Slack App ID.</p><label>Fresh Configuration Token<input type="password" autoComplete="off" spellCheck={false} value={deleteToken} onChange={(event) => setDeleteToken(event.target.value)} /></label><label>Exact Slack App ID<input autoComplete="off" spellCheck={false} value={deleteAppId} onChange={(event) => setDeleteAppId(event.target.value)} /></label><label>Audit reason<textarea value={deleteReason} onChange={(event) => setDeleteReason(event.target.value)} /></label><div className="decision-actions"><button className="btn btn-danger" disabled={busy || !deleteToken.trim() || !deleteAppId.trim() || !deleteReason.trim()} onClick={() => void deleteDedicatedApp()}>Permanently delete App</button><button className="btn btn-ghost" onClick={() => { setDeleteTarget(null); setDeleteToken(""); setDeleteAppId(""); setDeleteReason(""); }}>Cancel delete</button></div></section>}
    {intent?.status === "pending" && <div className="connection-intent" role="status"><div><strong>Waiting for Slack consent</strong><p>This page can be refreshed safely. Intent expires at {intent.expires_at}.</p></div><div className="decision-actions">{intent.authorize_url && <button className="btn btn-secondary" onClick={() => void invoke("open_source_connection_oauth", { authorize_url: intent.authorize_url })}>Open Slack again</button>}<button className="btn btn-ghost" disabled={busy} onClick={() => void cancel()}>Cancel</button></div></div>}
    {intent && intent.status !== "pending" && intent.status !== "completed" && <p role="alert" className="attention-error">OAuth {intent.status}: {intent.error_code ?? "No credential was stored"}</p>}
    <div className="connection-list" role="list" aria-live="polite">
      {connections.map((connection) => <article key={connection.id} role="listitem" className={`liquid-glass connection-card ${selected?.id === connection.id ? "selected" : ""}`}><button className="connection-card-main" onClick={() => onNavigate({ page: "sources", section: "connections", resourceId: connection.id })}><span><strong>{connection.display_label}</strong><span className={`badge ${connection.state === "active" ? "badge-success" : ""}`}>{connection.state}</span></span><small>{connection.provisioning_mode} · generation {connection.generation} · {connection.trigger_name ?? "Trigger pending"}</small>{connection.provisioning_mode === "managed_dedicated" && <small>Workspace App {connection.app_id_digest ? `${connection.app_id_digest.slice(0, 12)}…` : "identity pending"} · manifest {connection.manifest_version ?? "pending"} · provisioning {connection.provision_state ?? "unknown"}</small>}<small>Last delivery: {connection.last_delivery_at ?? "No events yet"} · cursor {connection.last_acked_cursor}</small></button>{canAccess("admin") && <div className="decision-actions">{connection.state === "active" && <button className="btn btn-ghost" disabled={busy} onClick={() => setReviewedAction({ kind: "reauthorize", connection })}>Reauthorize</button>}{connection.provisioning_mode === "managed_dedicated" && connection.state === "active" && <button className="btn btn-ghost" disabled={busy} onClick={() => { setLifecycleTarget(connection); setLifecycle(null); }}>Review manifest upgrade</button>}{connection.provisioning_mode === "managed_dedicated" && connection.state === "active" && <button className="btn btn-ghost" disabled={busy} onClick={() => setReviewedAction({ kind: "migrate_to_shared", connection })}>Migrate to Official App</button>}<button className="btn btn-ghost" disabled={busy || connection.state !== "active"} onClick={() => setTransferConnection(connection)}>Transfer</button>{connection.state !== "disconnected" && <button className="btn btn-danger" disabled={busy} onClick={() => setReviewedAction({ kind: "disconnect", connection })}>Disconnect</button>}{connection.provisioning_mode === "managed_dedicated" && connection.state === "disconnected" && connection.provision_state !== "app_deleted" && <button className="btn btn-danger" disabled={busy} onClick={() => setDeleteTarget(connection)}>Delete workspace App</button>}</div>}{connection.last_error_code && <p className="field-error">{connection.last_error_code}</p>}</article>)}
      {connections.length === 0 && <div className="operations-state">No Slack connections in this project.</div>}
    </div>
    {intent?.connection && <div className="connection-next-steps"><h3>Connection active</h3><p>Reaction routing stays disabled until you explicitly configure a template and binding.</p><a className="btn btn-primary" href="#/sources/automations/templates">Configure badge automation</a></div>}
    <ReviewedActionDialog open={reviewedAction !== null} title={reviewedAction?.kind === "disconnect" ? `Disconnect ${reviewedAction.connection.display_label}` : reviewedAction?.kind === "migrate_to_shared" ? `Migrate ${reviewedAction.connection.display_label} to Official App` : `Reauthorize ${reviewedAction?.connection.display_label ?? "connection"}`} description={reviewedAction?.kind === "disconnect" ? "Gateway and local credentials will be destroyed. Existing task and source evidence is retained." : reviewedAction?.kind === "migrate_to_shared" ? "OAuth atomically replaces the dedicated App owner with the official App. The old endpoint is fenced and task evidence is retained." : "Slack OAuth will rotate this connection credential and invalidate the old generation."} confirmLabel={reviewedAction?.kind === "disconnect" ? "Disconnect" : reviewedAction?.kind === "migrate_to_shared" ? "Continue migration" : "Continue to Slack"} destructive={reviewedAction?.kind === "disconnect"} onCancel={() => setReviewedAction(null)} onConfirm={(reason) => { const action = reviewedAction; setReviewedAction(null); if (!action) return; if (action.kind === "disconnect") void disconnect(action.connection, reason); else if (action.kind === "migrate_to_shared") void migrateToShared(action.connection, reason); else void reauthorize(action.connection, reason); }} />
    <ReviewedActionDialog open={reviewDedicated} title={dedicated?.status === "handoff_pending" ? "Resume dedicated app import" : "Create dedicated Slack App"} description="This reviewed action creates a workspace-owned App with the displayed scopes, events, and callback origins. The Configuration Token is already cleared from the UI and is never persisted." confirmLabel={dedicated?.status === "handoff_pending" ? "Resume secure import" : "Create app"} onCancel={() => setReviewDedicated(false)} onConfirm={(reason) => { setReviewDedicated(false); void approveDedicated(reason); }} />
    <ReviewedActionDialog open={reviewLifecycle} title="Apply dedicated Slack App manifest" description="The exact App identity and semantic diff were verified with a fresh Configuration Token. Permission expansion suspends delivery until OAuth reauthorization succeeds." confirmLabel="Apply manifest" onCancel={() => setReviewLifecycle(false)} onConfirm={(reason) => { setReviewLifecycle(false); void applyLifecycle(reason); }} />
    <SourceConnectionTransferDialog connection={transferConnection} busy={busy} onCancel={() => setTransferConnection(null)} onConfirm={(target, reason) => { if (transferConnection) void transfer(transferConnection, target, reason); }} />
  </section>;
}
