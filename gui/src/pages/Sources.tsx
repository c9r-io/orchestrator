import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SourceAutomationRoute, SourceEvent } from "../lib/types";
import { useRole } from "../hooks/useRole";
import i18n from "../lib/i18n";

interface Props { onOpenTask: (id: string) => void; }

export default function Sources({ onOpenTask }: Props) {
  const { canAccess } = useRole();
  const [events, setEvents] = useState<SourceEvent[]>([]);
  const [routes, setRoutes] = useState<Record<string, SourceAutomationRoute>>({});
  const [routingState, setRoutingState] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const nextEvents = await invoke<SourceEvent[]>("source_event_list", {
        project_id: null,
        task_id: null,
        routing_state: routingState || null,
      });
      setEvents(nextEvents);
      if (canAccess("operator")) {
        const routeEntries = await Promise.all(nextEvents
          .filter((event) => event.automation_route_id)
          .map(async (event) => [event.id, await invoke<SourceAutomationRoute>(
            "source_automation_route_get", { source_event_id: event.id },
          )] as const));
        setRoutes(Object.fromEntries(routeEntries));
      } else {
        setRoutes({});
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }, [canAccess, routingState]);

  useEffect(() => { void load(); }, [load]);

  const replay = async (id: string) => {
    try {
      await invoke("source_replay", { id });
      await load();
    } catch (reason) {
      setError(String(reason));
    }
  };

  return (
    <main aria-labelledby="sources-title">
      <header style={{ marginBottom: 16 }}>
        <h1 id="sources-title" className="page-title">{i18n.sources.title}</h1>
        <p>{i18n.sources.subtitle}</p>
      </header>
      <label style={{ display: "inline-flex", flexDirection: "column", gap: 4, marginBottom: 16 }}>
        <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>{i18n.sources.allStates}</span>
        <select value={routingState} onChange={(event) => setRoutingState(event.target.value)}>
          <option value="">{i18n.sources.allStates}</option>
          <option value="received">received</option><option value="routing">routing</option>
          <option value="routed">routed</option><option value="needs_attention">needs_attention</option>
          <option value="failed">failed</option><option value="ignored">ignored</option>
        </select>
      </label>
      {error && <p role="alert" style={{ color: "var(--danger)", marginBottom: 12 }}>{error}</p>}
      {!loading && events.length === 0 && <div className="liquid-glass">{i18n.sources.empty}</div>}
      <div role="list" aria-live="polite" style={{ display: "grid", gap: 12 }}>
        {events.map((event) => (
          <article key={event.id} role="listitem" className="liquid-glass">
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <span className="badge badge-info">{event.provider}</span>
              <span className="badge">{event.event_type}</span>
              <strong>{event.installation_id}</strong>
              <span className="badge">{event.routing_state}</span>
              <time style={{ marginLeft: "auto", color: "var(--text-tertiary)" }}>{event.received_at}</time>
            </div>
            <p style={{ marginTop: 8, color: "var(--text-secondary)", overflowWrap: "anywhere" }}>
              {event.conversation_id ?? "—"} / {event.thread_id ?? "—"}
            </p>
            {event.reaction_name && (
              <p style={{ marginTop: 8, color: "var(--text-secondary)", overflowWrap: "anywhere" }}>
                <strong>:{event.reaction_name}:</strong>
                {event.reaction_target_kind && event.reaction_target_id && (
                  <> · {event.reaction_target_kind} / {event.reaction_target_id}</>
                )}
              </p>
            )}
            {event.last_error_code && <p style={{ color: "var(--danger)" }}>{event.last_error_code}</p>}
            {event.automation_route_id && (
              <p style={{ marginTop: 8, color: "var(--text-secondary)", overflowWrap: "anywhere" }}>
                <span className="badge">{event.automation_status}</span>
                {event.automation_binding_name && <> · {event.automation_binding_name}</>}
                {event.automation_template_name && <> → {event.automation_template_name}</>}
              </p>
            )}
            <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
              {event.routed_task_id && <button className="btn btn-ghost" onClick={() => onOpenTask(event.routed_task_id!)}>{i18n.sources.openProcess}</button>}
              {canAccess("operator") && routes[event.id]?.permalink && (
                <a className="btn btn-ghost" href={routes[event.id].permalink!} target="_blank" rel="noreferrer">
                  {i18n.sources.openSlack}
                </a>
              )}
              {canAccess("admin") && ["failed", "needs_attention"].includes(event.routing_state) && (
                <button className="btn btn-secondary" onClick={() => replay(event.id)}>{i18n.sources.replay}</button>
              )}
            </div>
          </article>
        ))}
      </div>
    </main>
  );
}
