import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import ConfirmDialog from "../components/ConfirmDialog";
import { useRole } from "../hooks/useRole";
import { recordUiMetric } from "../lib/telemetry";
import i18n from "../lib/i18n";
import type { AttentionAction, AttentionDelta, AttentionItem, AttentionListResult, Role } from "../lib/types";

interface Props { initialAttentionId?: string; nativeNotificationsEnabled: boolean; onOpenTask: (taskId: string) => void; onOpenSourceRoute?: (routeId: string) => void; }
interface Filters { state: string; severity: string; assignee: string; }
type PendingAction = { kind: "resolve"; item: AttentionItem } | { kind: "execute"; item: AttentionItem; action: AttentionAction };

function mutationKey(): string {
  return typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `gui-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function matchesAttentionFilters(item: AttentionItem, filters: Filters, currentActor?: string): boolean {
  const stateMatches = filters.state === "active" ? item.state !== "resolved" : item.state === filters.state;
  const assigneeMatches = !filters.assignee
    || (filters.assignee === "unassigned" ? !item.assignee : filters.assignee === "me" ? currentActor === undefined || item.assignee === currentActor : item.assignee === filters.assignee);
  return stateMatches && (!filters.severity || item.severity === filters.severity) && assigneeMatches;
}

function sortAttention(items: AttentionItem[]) {
  return [...items].sort((left, right) => {
    const severity = Number(right.severity === "intervention") - Number(left.severity === "intervention");
    return severity || right.last_occurred_at.localeCompare(left.last_occurred_at);
  });
}

export function reconcileAttentionDelta(current: AttentionItem[], delta: AttentionDelta, filters: Filters): AttentionItem[] {
  const without = current.filter((item) => item.id !== delta.item?.id);
  if (!delta.item || delta.kind === "remove" || !matchesAttentionFilters(delta.item, filters)) return without;
  return sortAttention([...without, delta.item]);
}

function requestedDecision(item: AttentionItem): string {
  if (!item.requested_decision_json) return item.summary;
  try {
    const parsed = JSON.parse(item.requested_decision_json) as Record<string, unknown>;
    return String(parsed.question ?? parsed.decision ?? parsed.summary ?? item.summary);
  } catch { return item.summary; }
}

function ageLabel(value: string): string {
  const minutes = Math.max(0, Math.floor((Date.now() - new Date(value).getTime()) / 60_000));
  if (minutes < 60) return `${minutes}m`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)}h`;
  return `${Math.floor(minutes / 1440)}d`;
}

export default function AttentionInbox({ initialAttentionId, nativeNotificationsEnabled, onOpenTask, onOpenSourceRoute }: Props) {
  const { canAccess } = useRole();
  const canMutate = canAccess("operator");
  const [items, setItems] = useState<AttentionItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(initialAttentionId ?? null);
  const [filters, setFilters] = useState<Filters>({ state: "active", severity: "", assignee: "" });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [notificationFallback, setNotificationFallback] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingAction | null>(null);
  const changeId = useRef(0);

  const load = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const result = await invoke<AttentionListResult>("attention_list", {
        item_state: filters.state === "active" ? null : filters.state,
        active_only: filters.state === "active",
        severity: filters.severity || null, assignee: filters.assignee || null,
        project_id: null, kind: null, task_id: null,
      });
      const visible = sortAttention(result.items.filter((item) => matchesAttentionFilters(item, filters)));
      setItems(visible); changeId.current = result.latest_change_id;
      setSelectedId((current) => visible.some((item) => item.id === current) ? current : visible[0]?.id ?? null);
    } catch (cause) { setError(String(cause)); }
    finally { setLoading(false); }
  }, [filters]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let unlistenError: UnlistenFn | undefined;
    let unlistenFallback: UnlistenFn | undefined;
    let disposed = false;
    (async () => {
      await load();
      if (disposed) return;
      unlisten = await listen<AttentionDelta>("attention-delta", ({ payload }) => {
        changeId.current = payload.change_id;
        setItems((current) => reconcileAttentionDelta(current, payload, filters));
        if (payload.notification) setAnnouncement(`New attention item: ${payload.notification.title}`);
      });
      unlistenError = await listen<string>("stream-error-attention", ({ payload }) => {
        recordUiMetric("stream_reconnect", { page: "attention", result: "error" }); setError(payload);
      });
      unlistenFallback = await listen<string>("attention-notification-fallback", ({ payload }) => {
        setNotificationFallback(payload); setAnnouncement(payload);
      });
      await invoke("start_attention_follow", {
        after_change_id: changeId.current,
        project_id: null,
        item_state: filters.state === "active" ? null : filters.state,
        kind: null,
        severity: filters.severity || null,
        assignee: filters.assignee || null,
        task_id: null,
        active_only: filters.state === "active",
        native_notifications_enabled: nativeNotificationsEnabled,
      });
    })().catch((cause) => setError(String(cause)));
    return () => { disposed = true; unlisten?.(); unlistenError?.(); unlistenFallback?.(); invoke("stop_attention_follow").catch(() => undefined); };
  }, [filters, load, nativeNotificationsEnabled]);

  useEffect(() => {
    if (selectedId && items.some((item) => item.id === selectedId)) return;
    setSelectedId(items[0]?.id ?? null);
  }, [items, selectedId]);

  const current = items.find((item) => item.id === selectedId) ?? null;
  const update = useCallback((item: AttentionItem) => setItems((existing) => reconcileAttentionDelta(existing, { kind: "upsert", change_id: changeId.current, item }, filters)), [filters]);

  const mutate = useCallback(async (operation: "claim" | "snooze", item: AttentionItem) => {
    if (!canMutate) return;
    try {
      const command = operation === "claim" ? "attention_claim" : "attention_snooze";
      const args = { id: item.id, expected_version: item.version, idempotency_key: mutationKey(), ...(operation === "snooze" ? { until: new Date(Date.now() + 3_600_000).toISOString() } : {}) };
      update(await invoke<AttentionItem>(command, args)); setAnnouncement(`${operation} succeeded for ${item.title}`);
    } catch (cause) { setError(String(cause)); await load(); }
  }, [canMutate, load, update]);

  const confirmPending = useCallback(async () => {
    if (!pending || !canMutate) return;
    const action = pending; setPending(null);
    try {
      const updated = action.kind === "resolve"
        ? await invoke<AttentionItem>("attention_resolve", { id: action.item.id, expected_version: action.item.version, idempotency_key: mutationKey(), reason: "acknowledged_in_attention_inbox" })
        : await invoke<AttentionItem>("attention_execute_action", { id: action.item.id, expected_version: action.item.version, idempotency_key: mutationKey(), action_id: action.action.id, input_json: "{}" });
      setItems((existing) => reconcileAttentionDelta(existing, { kind: "upsert", change_id: changeId.current, item: updated }, filters));
    } catch (cause) { setError(String(cause)); await load(); }
  }, [canMutate, filters, load, pending]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (["INPUT", "SELECT", "TEXTAREA", "BUTTON"].includes(target.tagName) || event.metaKey || event.ctrlKey || event.altKey) return;
      const index = Math.max(0, items.findIndex((item) => item.id === selectedId));
      if (["j", "ArrowDown"].includes(event.key)) { event.preventDefault(); setSelectedId(items[Math.min(items.length - 1, index + 1)]?.id ?? null); }
      if (["k", "ArrowUp"].includes(event.key)) { event.preventDefault(); setSelectedId(items[Math.max(0, index - 1)]?.id ?? null); }
      if (event.key.toLowerCase() === "c" && current) void mutate("claim", current);
      if (event.key.toLowerCase() === "s" && current) void mutate("snooze", current);
      if (event.key.toLowerCase() === "r" && current && canMutate) setPending({ kind: "resolve", item: current });
      if (event.key === "Enter" && current?.task_id) onOpenTask(current.task_id);
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [canMutate, current, items, mutate, onOpenTask, selectedId]);

  const counts = useMemo(() => ({ intervention: items.filter((item) => item.severity === "intervention").length, attention: items.filter((item) => item.severity === "attention").length }), [items]);
  const actionAllowed = (action: AttentionAction) => ["read_only", "operator", "admin"].includes(action.required_role) && canAccess(action.required_role as Role);

  return <main aria-labelledby="attention-title">
    <header className="attention-header"><div><h1 id="attention-title" className="page-title">Attention</h1><p>{i18n.attention.subtitle}</p></div><div className="attention-counts"><span className="badge badge-danger">◆ {counts.intervention} intervention</span><span className="badge badge-warning">● {counts.attention} attention</span></div></header>
    <div className="sr-live" aria-live="polite" aria-atomic="true">{announcement}</div>
    {(!nativeNotificationsEnabled || notificationFallback) && <p className="attention-notification-fallback" role="status">{notificationFallback ?? "Desktop notifications are unavailable; in-app Attention remains active."}</p>}
    {error && <p className="attention-error" role="alert">{error}</p>}
    <div className="attention-workbench">
      <aside className="attention-filter-pane" aria-label="Attention filters">
        <h2>Queue</h2>
        <label>Status<select value={filters.state} onChange={(event) => setFilters((value) => ({ ...value, state: event.target.value }))}><option value="active">Open queue</option><option value="open">Open</option><option value="claimed">Claimed</option><option value="snoozed">Snoozed</option><option value="resolved">Resolved history</option></select></label>
        <label>Severity<select value={filters.severity} onChange={(event) => setFilters((value) => ({ ...value, severity: event.target.value }))}><option value="">All</option><option value="intervention">Intervention</option><option value="attention">Attention</option></select></label>
        <label>Assignee<select value={filters.assignee} onChange={(event) => setFilters((value) => ({ ...value, assignee: event.target.value }))}><option value="">All</option><option value="me">Mine</option><option value="unassigned">Unassigned</option></select></label>
        <button className="btn btn-ghost" onClick={load}>Refresh snapshot</button>
      </aside>
      <section className="attention-list-pane" aria-label="Actionable attention items">
        {loading && <p className="empty-state">{i18n.common.loading}</p>}
        {!loading && items.length === 0 && <p className="empty-state">{i18n.attention.empty}</p>}
        <div role="listbox" aria-label="Attention queue" tabIndex={0} aria-activedescendant={selectedId ? `attention-${selectedId}` : undefined}>
          {items.map((item) => <button id={`attention-${item.id}`} key={item.id} className={`attention-row ${item.id === selectedId ? "selected" : ""}`} role="option" aria-selected={item.id === selectedId} onClick={() => setSelectedId(item.id)}>
            <span className={`attention-severity attention-severity-${item.severity}`} aria-hidden="true" /><span className="attention-row-copy"><strong>{item.title}</strong><small>{item.task_id || "Unbound source"} · {requestedDecision(item)}</small></span><span className="attention-age">{ageLabel(item.last_occurred_at)}</span>
          </button>)}
        </div>
      </section>
      <aside className="liquid-glass attention-decision-pane" aria-label="Decision context">
        {!current ? <p className="empty-state">Select an item to inspect its decision context.</p> : <>
          <div className="decision-heading"><span className={`badge ${current.severity === "intervention" ? "badge-danger" : "badge-warning"}`}>{current.severity}</span><span className="badge">{current.kind}</span></div>
          <h2>{current.title}</h2><p className="decision-summary">{requestedDecision(current)}</p>
          <dl className="decision-facts"><div><dt>Process</dt><dd>{current.task_id || "Awaiting correlation"}</dd></div><div><dt>Step</dt><dd>{current.step_id ?? "—"}</dd></div><div><dt>Assignee</dt><dd>{current.assignee ?? "Unassigned"}</dd></div><div><dt>Occurrences</dt><dd>{current.occurrence_count}</dd></div></dl>
          {!canMutate && <p className="readonly-reason">Read-only access. Resolution, recovery and writer mutations are unavailable.</p>}
          <div className="decision-actions">
            {current.state === "open" && <button className="btn btn-secondary" disabled={!canMutate} title={!canMutate ? "Requires operator role" : undefined} onClick={() => void mutate("claim", current)}>Claim</button>}
            <button className="btn btn-ghost" disabled={!canMutate} onClick={() => void mutate("snooze", current)}>Snooze 1h</button>
            {current.actions.filter((action) => !["acknowledge", "retry_failed_item", "resume_task"].includes(action.id)).map((action) => <button key={action.id} className="btn btn-primary" disabled={!actionAllowed(action)} title={!actionAllowed(action) ? `Requires ${action.required_role}` : undefined} onClick={() => setPending({ kind: "execute", item: current, action })}>{action.label}</button>)}
            <button className="btn btn-ghost" disabled={!canMutate} onClick={() => setPending({ kind: "resolve", item: current })}>Resolve</button>
            {current.task_id && <button className="btn btn-secondary" onClick={() => onOpenTask(current.task_id)}>Open process</button>}
            {current.source_route_id && onOpenSourceRoute && <button className="btn btn-secondary" onClick={() => onOpenSourceRoute(current.source_route_id!)}>Open automation route</button>}
          </div>
        </>}
      </aside>
    </div>
    <p className="attention-keyboard">J/K or ↑/↓ select · C claim · S snooze · R resolve · Enter open process</p>
    <ConfirmDialog open={!!pending} title={pending?.kind === "execute" ? "Confirm process action" : "Confirm resolution"} message={pending?.kind === "execute" ? `Execute “${pending.action.label}”. This can change process execution state and will be audited.` : "Resolve this item and remove it from the open Attention queue. The audit history remains available."} confirmLabel={pending?.kind === "execute" ? "Execute reviewed action" : "Resolve item"} destructive onConfirm={() => void confirmPending()} onCancel={() => setPending(null)} />
  </main>;
}
