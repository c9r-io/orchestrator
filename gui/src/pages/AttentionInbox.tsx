import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import ConfirmDialog from "../components/ConfirmDialog";
import { useRole } from "../hooks/useRole";
import { recordUiMetric } from "../lib/telemetry";
import i18n from "../lib/i18n";
import type { AttentionAction, AttentionDelta, AttentionItem, AttentionListResult, Role } from "../lib/types";

interface Props {
  initialAttentionId?: string;
  nativeNotificationsEnabled: boolean;
  onOpenTask: (taskId: string, reviewResume?: boolean) => void;
  onOpenSourceRoute?: (routeId: string) => void;
}
interface Filters { state: string; severity: string; assignee: string; }
type PendingAction = { kind: "resolve"; item: AttentionItem } | { kind: "execute"; item: AttentionItem; action: AttentionAction };
type MutationKind = "claim" | "snooze" | "resolve" | "execute";
type ReconciliationState = "pending" | "confirmed" | "unconfirmed";
type ErrorCategory = "conflict" | "already_applied" | "not_found" | "invalid_request" | "permission" | "unavailable" | "timeout" | "internal";

interface SafeCommandError {
  category: ErrorCategory;
  message: string;
  request_id: string | null;
}

interface MutationFailure extends SafeCommandError {
  operation: MutationKind;
  itemId: string;
  itemTitle: string;
  projectId: string;
  targetVersion: number;
  reconciliation: ReconciliationState;
}

const ERROR_COPY: Record<ErrorCategory, string> = {
  conflict: "This item changed in another session. The latest state must be confirmed before retrying.",
  already_applied: "This request was already handled. Confirm the latest state before continuing.",
  not_found: "This attention item is no longer available. Confirm the latest queue state.",
  invalid_request: "This action is no longer valid for the current item state.",
  permission: "Your current role does not allow this action.",
  unavailable: "The daemon is unavailable. Check that it is running, then retry.",
  timeout: "The operation timed out. Confirm the latest state before retrying.",
  internal: "The operation failed without a confirmed state change. Retry the state check.",
};

export function normalizeAttentionError(value: unknown): SafeCommandError {
  const candidate = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const category = typeof candidate.category === "string" && candidate.category in ERROR_COPY
    ? candidate.category as ErrorCategory
    : "internal";
  const requestId = typeof candidate.request_id === "string" && /^[A-Za-z0-9._:-]{1,128}$/.test(candidate.request_id)
    ? candidate.request_id
    : null;
  return { category, message: ERROR_COPY[category], request_id: requestId };
}

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

function operationLabel(operation: MutationKind): string {
  return operation === "execute" ? "Action" : `${operation[0].toUpperCase()}${operation.slice(1)}`;
}

export default function AttentionInbox({ initialAttentionId, nativeNotificationsEnabled, onOpenTask, onOpenSourceRoute }: Props) {
  const { canAccess } = useRole();
  const canMutate = canAccess("operator");
  const [items, setItems] = useState<AttentionItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(initialAttentionId ?? null);
  const [filters, setFilters] = useState<Filters>({ state: "active", severity: "", assignee: "" });
  const [loading, setLoading] = useState(true);
  const [queryError, setQueryError] = useState<SafeCommandError | null>(null);
  const [streamError, setStreamError] = useState<SafeCommandError | null>(null);
  const [mutationError, setMutationError] = useState<MutationFailure | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [notificationFallback, setNotificationFallback] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingAction | null>(null);
  const [busyMutation, setBusyMutation] = useState<MutationKind | null>(null);
  const changeId = useRef(0);
  const listboxRef = useRef<HTMLDivElement>(null);

  const fetchSnapshot = useCallback(async () => {
    try {
      return await invoke<AttentionListResult>("attention_list", {
        item_state: filters.state === "active" ? null : filters.state,
        active_only: filters.state === "active",
        severity: filters.severity || null, assignee: filters.assignee || null,
        project_id: null, kind: null, task_id: null,
      });
    } catch (cause) {
      throw normalizeAttentionError(cause);
    }
  }, [filters]);

  const applySnapshot = useCallback((result: AttentionListResult, preferredId?: string) => {
      const visible = sortAttention(result.items.filter((item) => matchesAttentionFilters(item, filters)));
      setItems(visible); changeId.current = result.latest_change_id;
      setSelectedId((current) => {
        const preferred = preferredId ?? current;
        return visible.some((item) => item.id === preferred) ? preferred : visible[0]?.id ?? null;
      });
  }, [filters]);

  const load = useCallback(async () => {
    setLoading(true); setQueryError(null);
    try {
      applySnapshot(await fetchSnapshot());
      return true;
    } catch (cause) {
      setQueryError(normalizeAttentionError(cause));
      return false;
    }
    finally { setLoading(false); }
  }, [applySnapshot, fetchSnapshot]);

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
        setStreamError(null);
        if (payload.notification) setAnnouncement(`New attention item: ${payload.notification.title}`);
      });
      unlistenError = await listen<SafeCommandError>("stream-error-attention", ({ payload }) => {
        recordUiMetric("stream_reconnect", { page: "attention", result: "error" }); setStreamError(normalizeAttentionError(payload));
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
      setStreamError(null);
    })().catch((cause) => setStreamError(normalizeAttentionError(cause)));
    return () => { disposed = true; unlisten?.(); unlistenError?.(); unlistenFallback?.(); invoke("stop_attention_follow").catch(() => undefined); };
  }, [filters, load, nativeNotificationsEnabled]);

  useEffect(() => {
    if (selectedId && items.some((item) => item.id === selectedId)) return;
    setSelectedId(items[0]?.id ?? null);
  }, [items, selectedId]);

  const current = items.find((item) => item.id === selectedId) ?? null;
  const update = useCallback((item: AttentionItem) => setItems((existing) => reconcileAttentionDelta(existing, { kind: "upsert", change_id: changeId.current, item }, filters)), [filters]);

  const restoreFocusAfterReconciliation = useCallback((origin: HTMLElement | null) => {
    requestAnimationFrame(() => {
      if (origin?.isConnected) return;
      listboxRef.current?.focus();
    });
  }, []);

  const reconcileFailure = useCallback(async (
    failure: MutationFailure,
    origin: HTMLElement | null,
    clearOnSuccess = false,
  ) => {
    setLoading(true);
    try {
      applySnapshot(await fetchSnapshot(), failure.itemId);
      setQueryError(null);
      setMutationError((currentFailure) => {
        if (!currentFailure || currentFailure.itemId !== failure.itemId || currentFailure.operation !== failure.operation) return currentFailure;
        return clearOnSuccess ? null : { ...currentFailure, reconciliation: "confirmed" };
      });
      setAnnouncement(clearOnSuccess ? `Latest state confirmed for ${failure.itemTitle}` : "");
      recordUiMetric("attention_reconciliation", {
        project_id: failure.projectId, action: failure.operation, result: "confirmed",
      });
    } catch (cause) {
      const safeError = normalizeAttentionError(cause);
      setQueryError(safeError);
      setMutationError((currentFailure) => {
        if (!currentFailure || currentFailure.itemId !== failure.itemId || currentFailure.operation !== failure.operation) return currentFailure;
        return { ...currentFailure, reconciliation: "unconfirmed" };
      });
      recordUiMetric("attention_reconciliation", {
        project_id: failure.projectId, action: failure.operation, result: "unconfirmed",
      });
    } finally {
      setLoading(false);
      restoreFocusAfterReconciliation(origin);
    }
  }, [applySnapshot, fetchSnapshot, restoreFocusAfterReconciliation]);

  const runMutation = useCallback(async (
    operation: MutationKind,
    item: AttentionItem,
    action?: AttentionAction,
  ) => {
    if (!canMutate || busyMutation) return;
    const origin = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const idempotencyKey = mutationKey();
    setBusyMutation(operation);
    setAnnouncement("");
    try {
      let updated: AttentionItem;
      if (operation === "claim") {
        updated = await invoke("attention_claim", { id: item.id, expected_version: item.version, idempotency_key: idempotencyKey });
      } else if (operation === "snooze") {
        updated = await invoke("attention_snooze", { id: item.id, expected_version: item.version, idempotency_key: idempotencyKey, until: new Date(Date.now() + 3_600_000).toISOString() });
      } else if (operation === "resolve") {
        updated = await invoke("attention_resolve", { id: item.id, expected_version: item.version, idempotency_key: idempotencyKey, reason: "acknowledged_in_attention_inbox" });
      } else {
        updated = await invoke("attention_execute_action", { id: item.id, expected_version: item.version, idempotency_key: idempotencyKey, action_id: action?.id, input_json: "{}" });
      }
      update(updated);
      setMutationError((failure) => failure?.itemId === item.id && failure.operation === operation ? null : failure);
      setAnnouncement(`${operation} succeeded for ${item.title}`);
      recordUiMetric("attention_mutation", {
        project_id: item.project_id, action: operation, result: "success",
      });
    } catch (cause) {
      const error = normalizeAttentionError(cause);
      const failure: MutationFailure = {
        ...error,
        operation,
        itemId: item.id,
        itemTitle: item.title,
        projectId: item.project_id,
        targetVersion: item.version,
        reconciliation: "pending",
      };
      setMutationError(failure);
      recordUiMetric("attention_mutation", {
        project_id: item.project_id, action: operation, result: "failure", error_category: error.category,
      });
      await reconcileFailure(failure, origin);
    } finally {
      setBusyMutation(null);
    }
  }, [busyMutation, canMutate, reconcileFailure, update]);

  const confirmPending = useCallback(async () => {
    if (!pending || !canMutate || busyMutation) return;
    const action = pending; setPending(null);
    await runMutation(action.kind, action.item, action.kind === "execute" ? action.action : undefined);
  }, [busyMutation, canMutate, pending, runMutation]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (["INPUT", "SELECT", "TEXTAREA", "BUTTON"].includes(target.tagName) || event.metaKey || event.ctrlKey || event.altKey) return;
      const index = Math.max(0, items.findIndex((item) => item.id === selectedId));
      if (["j", "ArrowDown"].includes(event.key)) { event.preventDefault(); setSelectedId(items[Math.min(items.length - 1, index + 1)]?.id ?? null); }
      if (["k", "ArrowUp"].includes(event.key)) { event.preventDefault(); setSelectedId(items[Math.max(0, index - 1)]?.id ?? null); }
      if (event.key.toLowerCase() === "c" && current) void runMutation("claim", current);
      if (event.key.toLowerCase() === "s" && current) void runMutation("snooze", current);
      if (event.key.toLowerCase() === "r" && current && canMutate) setPending({ kind: "resolve", item: current });
      if (event.key === "Enter" && current?.task_id) onOpenTask(current.task_id);
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [canMutate, current, items, onOpenTask, runMutation, selectedId]);

  const counts = useMemo(() => ({ intervention: items.filter((item) => item.severity === "intervention").length, attention: items.filter((item) => item.severity === "attention").length }), [items]);
  const actionAllowed = (action: AttentionAction) => ["read_only", "operator", "admin"].includes(action.required_role) && canAccess(action.required_role as Role);
  const safeResumeAction = current?.actions.find((action) => ["retry_failed_item", "resume_task"].includes(action.id));

  return <main aria-labelledby="attention-title">
    <header className="attention-header"><div><h1 id="attention-title" className="page-title">Attention</h1><p>{i18n.attention.subtitle}</p></div><div className="attention-counts"><span className="badge badge-danger">◆ {counts.intervention} intervention</span><span className="badge badge-warning">● {counts.attention} attention</span></div></header>
    <div className="sr-live" aria-live="polite" aria-atomic="true">{announcement}</div>
    {(!nativeNotificationsEnabled || notificationFallback) && <p className="attention-notification-fallback" role="status">{notificationFallback ?? "Desktop notifications are unavailable; in-app Attention remains active."}</p>}
    {mutationError && <section className={`attention-error-panel ${mutationError.reconciliation === "unconfirmed" ? "is-unconfirmed" : ""}`} role="alert" aria-atomic="true">
      <div>
        <strong>{operationLabel(mutationError.operation)} failed for {mutationError.itemTitle}.</strong>
        <p>{mutationError.message}</p>
        <p className="attention-reconciliation-state">
          {mutationError.reconciliation === "pending" && "Confirming the latest daemon state…"}
          {mutationError.reconciliation === "confirmed" && "The latest daemon state has been restored. The failed action was not announced as successful."}
          {mutationError.reconciliation === "unconfirmed" && "Latest state is not confirmed. Do not repeat the action until the state check succeeds."}
        </p>
        {mutationError.request_id && <small>Request ID: {mutationError.request_id}</small>}
      </div>
      <div className="attention-error-actions">
        <button className="btn btn-secondary" disabled={loading} onClick={() => {
          const origin = document.activeElement instanceof HTMLElement ? document.activeElement : null;
          void reconcileFailure(mutationError, origin, true);
        }}>Retry latest state check</button>
        <button className="btn btn-ghost" onClick={() => setMutationError(null)} aria-label={`Dismiss ${mutationError.operation} error`}>Dismiss</button>
      </div>
    </section>}
    {queryError && <section className="attention-error-panel is-unconfirmed" role="alert">
      <div><strong>Latest Attention state could not be loaded.</strong><p>{queryError.message}</p>{queryError.request_id && <small>Request ID: {queryError.request_id}</small>}</div>
      <button className="btn btn-secondary" disabled={loading} onClick={() => void load()}>Retry snapshot</button>
    </section>}
    {streamError && <section className="attention-stream-error" role="status">
      <span>Live updates are disconnected. {streamError.message}</span>
      <button className="btn btn-ghost" onClick={() => setStreamError(null)} aria-label="Dismiss live update error">Dismiss</button>
    </section>}
    <div className="attention-workbench">
      <aside className="attention-filter-pane" aria-label="Attention filters">
        <h2>Queue</h2>
        <label>Status<select value={filters.state} onChange={(event) => setFilters((value) => ({ ...value, state: event.target.value }))}><option value="active">Open queue</option><option value="open">Open</option><option value="claimed">Claimed</option><option value="snoozed">Snoozed</option><option value="resolved">Resolved history</option></select></label>
        <label>Severity<select value={filters.severity} onChange={(event) => setFilters((value) => ({ ...value, severity: event.target.value }))}><option value="">All</option><option value="intervention">Intervention</option><option value="attention">Attention</option></select></label>
        <label>Assignee<select value={filters.assignee} onChange={(event) => setFilters((value) => ({ ...value, assignee: event.target.value }))}><option value="">All</option><option value="me">Mine</option><option value="unassigned">Unassigned</option></select></label>
        <button className="btn btn-ghost" disabled={loading} onClick={() => void load()}>Refresh snapshot</button>
      </aside>
      <section className="attention-list-pane" aria-label="Actionable attention items">
        {loading && <p className="empty-state">{i18n.common.loading}</p>}
        {!loading && items.length === 0 && <p className="empty-state">{i18n.attention.empty}</p>}
        <div ref={listboxRef} role="listbox" aria-label="Attention queue" tabIndex={0} aria-activedescendant={selectedId ? `attention-${selectedId}` : undefined}>
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
            {current.state === "open" && <button className="btn btn-secondary" disabled={!canMutate || !!busyMutation} title={!canMutate ? "Requires operator role" : undefined} onClick={() => void runMutation("claim", current)}>Claim</button>}
            <button className="btn btn-ghost" disabled={!canMutate || !!busyMutation} onClick={() => void runMutation("snooze", current)}>Snooze 1h</button>
            {current.actions.filter((action) => !["acknowledge", "retry_failed_item", "resume_task"].includes(action.id)).map((action) => <button key={action.id} className="btn btn-primary" disabled={!actionAllowed(action) || !!busyMutation} title={!actionAllowed(action) ? `Requires ${action.required_role}` : undefined} onClick={() => setPending({ kind: "execute", item: current, action })}>{action.label}</button>)}
            {current.task_id && safeResumeAction && canMutate && <button className="btn btn-primary" disabled={!actionAllowed(safeResumeAction) || !!busyMutation} title={!actionAllowed(safeResumeAction) ? `Requires ${safeResumeAction.required_role}` : undefined} onClick={() => onOpenTask(current.task_id!, true)}>Review safe resume</button>}
            <button className="btn btn-ghost" disabled={!canMutate || !!busyMutation} onClick={() => setPending({ kind: "resolve", item: current })}>Resolve</button>
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
