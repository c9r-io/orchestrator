import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useRole } from "../hooks/useRole";
import i18n from "../lib/i18n";
import type { AttentionDelta, AttentionItem, AttentionListResult } from "../lib/types";

interface Props {
  onOpenTask: (taskId: string) => void;
}

function mutationKey(): string {
  return typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `gui-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export default function AttentionInbox({ onOpenTask }: Props) {
  const { canAccess } = useRole();
  const canMutate = canAccess("operator");
  const [items, setItems] = useState<AttentionItem[]>([]);
  const [selected, setSelected] = useState(0);
  const [stateFilter, setStateFilter] = useState("active");
  const [severity, setSeverity] = useState("");
  const [assignee, setAssignee] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const changeId = useRef(0);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<AttentionListResult>("attention_list", {
        item_state: stateFilter === "active" ? null : stateFilter,
        severity: severity || null,
        assignee: assignee || null,
      });
      const active = stateFilter === "active"
        ? result.items.filter((item) => item.state !== "resolved")
        : result.items;
      setItems(active);
      changeId.current = result.latest_change_id;
      setSelected((current) => Math.min(current, Math.max(0, active.length - 1)));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }, [assignee, severity, stateFilter]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      await load();
      unlisten = await listen<AttentionDelta>("attention-delta", (event) => {
        changeId.current = event.payload.change_id;
        const changed = event.payload.item;
        if (!changed) return;
        setItems((current) => {
          const without = current.filter((item) => item.id !== changed.id);
          if (event.payload.kind === "remove" && stateFilter === "active") return without;
          return [changed, ...without];
        });
      });
      await invoke("start_attention_follow", { after_change_id: changeId.current });
    })().catch((cause) => setError(String(cause)));
    return () => {
      unlisten?.();
      invoke("stop_attention_follow").catch(() => undefined);
    };
  }, [load, stateFilter]);

  const current = items[selected];

  const update = useCallback((item: AttentionItem) => {
    setItems((existing) => existing.map((entry) => entry.id === item.id ? item : entry));
  }, []);

  const claim = useCallback(async (item: AttentionItem) => {
    if (!canMutate) return;
    try {
      update(await invoke<AttentionItem>("attention_claim", {
        id: item.id,
        expected_version: item.version,
        idempotency_key: mutationKey(),
      }));
    } catch (cause) {
      setError(String(cause));
      await load();
    }
  }, [canMutate, load, update]);

  const resolve = useCallback(async (item: AttentionItem) => {
    if (!canMutate) return;
    try {
      const updated = await invoke<AttentionItem>("attention_resolve", {
        id: item.id,
        expected_version: item.version,
        idempotency_key: mutationKey(),
        reason: "acknowledged_in_attention_inbox",
      });
      setItems((existing) => existing.filter((entry) => entry.id !== updated.id));
    } catch (cause) {
      setError(String(cause));
      await load();
    }
  }, [canMutate, load]);

  const snooze = useCallback(async (item: AttentionItem) => {
    if (!canMutate) return;
    const until = new Date(Date.now() + 60 * 60 * 1000).toISOString();
    try {
      update(await invoke<AttentionItem>("attention_snooze", {
        id: item.id,
        expected_version: item.version,
        idempotency_key: mutationKey(),
        until,
      }));
    } catch (cause) {
      setError(String(cause));
      await load();
    }
  }, [canMutate, load, update]);

  const execute = useCallback(async (item: AttentionItem, actionId: string) => {
    if (!canMutate) return;
    const action = item.actions.find((candidate) => candidate.id === actionId);
    if (action?.confirmation === "required" && !window.confirm(`${action.label}?`)) return;
    try {
      const updated = await invoke<AttentionItem>("attention_execute_action", {
        id: item.id,
        expected_version: item.version,
        idempotency_key: mutationKey(),
        action_id: actionId,
        input_json: "{}",
      });
      setItems((existing) => existing.filter((entry) => entry.id !== updated.id));
    } catch (cause) {
      setError(String(cause));
      await load();
    }
  }, [canMutate, load]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (["INPUT", "SELECT", "TEXTAREA", "BUTTON"].includes(target.tagName)) return;
      if (event.key.toLowerCase() === "j") setSelected((value) => Math.min(items.length - 1, value + 1));
      if (event.key.toLowerCase() === "k") setSelected((value) => Math.max(0, value - 1));
      if (event.key.toLowerCase() === "c" && current) claim(current);
      if (event.key.toLowerCase() === "r" && current) resolve(current);
      if (event.key === "Enter" && current) onOpenTask(current.task_id);
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [claim, current, items.length, onOpenTask, resolve]);

  const counts = useMemo(() => ({
    intervention: items.filter((item) => item.severity === "intervention").length,
    attention: items.filter((item) => item.severity === "attention").length,
  }), [items]);

  return (
    <main aria-labelledby="attention-title">
      <header className="attention-header">
        <div>
          <h1 id="attention-title" className="page-title">{i18n.attention.title}</h1>
          <p>{i18n.attention.subtitle}</p>
        </div>
        <div className="attention-counts" aria-live="polite">
          <span className="badge badge-danger">{counts.intervention} intervention</span>
          <span className="badge badge-warning">{counts.attention} attention</span>
        </div>
      </header>

      <div className="attention-filters" aria-label="Attention filters">
        <select value={stateFilter} onChange={(event) => setStateFilter(event.target.value)}>
          <option value="active">{i18n.attention.allStates}</option>
          <option value="open">Open</option><option value="claimed">Claimed</option>
          <option value="snoozed">Snoozed</option><option value="resolved">Resolved</option>
        </select>
        <select value={severity} onChange={(event) => setSeverity(event.target.value)}>
          <option value="">{i18n.attention.allSeverities}</option>
          <option value="intervention">Intervention</option><option value="attention">Attention</option>
        </select>
        <select value={assignee} onChange={(event) => setAssignee(event.target.value)}>
          <option value="">{i18n.attention.allAssignees}</option>
          <option value="me">{i18n.attention.mine}</option><option value="unassigned">{i18n.attention.unassigned}</option>
        </select>
        <button className="btn btn-ghost" onClick={load}>{i18n.common.refresh}</button>
      </div>

      {!canMutate && <p className="attention-readonly">{i18n.attention.readOnly}</p>}
      {error && <p className="attention-error" role="alert">{error}</p>}
      {loading && <p>{i18n.common.loading}</p>}
      {!loading && items.length === 0 && <div className="liquid-glass attention-empty">{i18n.attention.empty}</div>}

      <div className="attention-list" role="listbox" aria-label="Attention items">
        {items.map((item, index) => (
          <article
            key={item.id}
            className={`liquid-glass attention-card ${index === selected ? "attention-card-selected" : ""}`}
            role="option"
            aria-selected={index === selected}
            tabIndex={0}
            onFocus={() => setSelected(index)}
            onClick={() => setSelected(index)}
          >
            <div className="attention-card-main">
              <div className="attention-card-title">
                <span className={`attention-severity attention-severity-${item.severity}`} />
                <strong>{item.title}</strong>
                <span className="badge badge-info">{item.kind}</span>
              </div>
              <p>{item.summary}</p>
              <div className="attention-meta">
                <span>{item.task_id}</span>
                {item.step_id && <span>step: {item.step_id}</span>}
                <span>{i18n.attention.occurrences(item.occurrence_count)}</span>
                {item.assignee && <span>@{item.assignee}</span>}
              </div>
            </div>
            <div className="attention-actions">
              {item.state === "open" && <button disabled={!canMutate} className="btn btn-secondary" onClick={() => claim(item)}>{i18n.attention.claim}</button>}
              <button disabled={!canMutate} className="btn btn-ghost" onClick={() => snooze(item)}>{i18n.attention.snooze}</button>
              {item.actions.filter((action) => action.id !== "acknowledge").map((action) => (
                <button key={action.id} disabled={!canMutate} className="btn btn-primary" onClick={() => execute(item, action.id)}>{action.label}</button>
              ))}
              <button disabled={!canMutate} className="btn btn-ghost" onClick={() => resolve(item)}>{i18n.attention.resolve}</button>
              <button className="btn btn-ghost" onClick={() => onOpenTask(item.task_id)}>{i18n.attention.timeline}</button>
            </div>
          </article>
        ))}
      </div>
      <p className="attention-keyboard">{i18n.attention.keyboard}</p>
    </main>
  );
}
