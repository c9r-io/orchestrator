import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import type { ConsoleRoute } from "../lib/routes";
import type { SourceAutomationRoute, SourceEvent } from "../lib/types";
import i18n from "../lib/i18n";
import SourceAutomations from "./source-automation/SourceAutomations";
import SourceConnections from "./source-connections/SourceConnections";

interface Props { route: Extract<ConsoleRoute, { page: "sources" }>; onNavigate: (route: ConsoleRoute) => void; onOpenTask: (id: string) => void; }

export default function Sources({ route, onNavigate, onOpenTask }: Props) {
  const section = route.section ?? "connections";
  return <main aria-labelledby="sources-title">
    <header className="sources-header"><div><h1 id="sources-title" className="page-title">{i18n.sources.title}</h1><p>{i18n.sources.subtitle}</p></div></header>
    <nav className="source-primary-nav" aria-label="Source management">
      <a href="#/sources/connections" className={section === "connections" ? "active" : ""} aria-current={section === "connections" ? "page" : undefined}>Connections</a>
      <a href="#/sources/events" className={section === "events" ? "active" : ""} aria-current={section === "events" ? "page" : undefined}>Events</a>
      <a href="#/sources/bindings" className={section === "bindings" ? "active" : ""} aria-current={section === "bindings" ? "page" : undefined}>Process bindings</a>
      <a href="#/sources/automations/templates" className={section === "automations" ? "active" : ""} aria-current={section === "automations" ? "page" : undefined}>Automations</a>
    </nav>
    {section === "connections" && <SourceConnections selectedId={route.resourceId} onNavigate={onNavigate} />}
    {section === "events" && <SourceEvents selectedId={route.resourceId} onNavigate={onNavigate} onOpenTask={onOpenTask} />}
    {section === "bindings" && <ProcessBindings selectedTaskId={route.resourceId} onOpenTask={onOpenTask} />}
    {section === "automations" && <SourceAutomations view={route.automationView ?? "templates"} resourceId={route.resourceId} onNavigate={onNavigate} onOpenTask={onOpenTask} />}
  </main>;
}

function SourceEvents({ selectedId, onNavigate, onOpenTask }: { selectedId?: string; onNavigate: (route: ConsoleRoute) => void; onOpenTask: (id: string) => void }) {
  const { canAccess } = useRole(); const [events, setEvents] = useState<SourceEvent[]>([]); const [routes, setRoutes] = useState<Record<string, SourceAutomationRoute>>({}); const [routingState, setRoutingState] = useState(""); const [error, setError] = useState<string | null>(null); const [loading, setLoading] = useState(true);
  const load = useCallback(async () => { setLoading(true); setError(null); try { const nextEvents = selectedId ? [await invoke<SourceEvent>("source_event_get", { id: selectedId })] : await invoke<SourceEvent[]>("source_event_list", { project_id: null, task_id: null, routing_state: routingState || null }); setEvents(nextEvents); if (canAccess("operator")) { const entries = await Promise.all(nextEvents.filter((event) => event.automation_route_id).map(async (event) => [event.id, await invoke<SourceAutomationRoute>("source_automation_route_get", { source_event_id: event.id })] as const)); setRoutes(Object.fromEntries(entries)); } else setRoutes({}); } catch (cause) { setError(String(cause)); } finally { setLoading(false); } }, [canAccess, routingState, selectedId]);
  useEffect(() => { void load(); }, [load]);
  const replay = async (id: string) => { try { await invoke("source_replay", { id }); await load(); } catch (cause) { setError(String(cause)); } };
  return <section aria-labelledby="events-heading"><div className="pane-heading"><div><h2 id="events-heading">Source events</h2><p>Bounded provenance only; raw normalized payloads stay behind the daemon boundary.</p></div>{selectedId && <button className="btn btn-ghost" onClick={() => onNavigate({ page: "sources", section: "events" })}>All events</button>}</div>{!selectedId && <label className="source-filter">Routing state<select value={routingState} onChange={(event) => setRoutingState(event.target.value)}><option value="">{i18n.sources.allStates}</option>{["received", "routing", "routed", "needs_attention", "failed", "ignored"].map((item) => <option key={item}>{item}</option>)}</select></label>}{error && <p role="alert" className="attention-error">{error}</p>}{!loading && events.length === 0 && <div className="liquid-glass">{i18n.sources.empty}</div>}<div role="list" aria-live="polite" className="source-event-list">{events.map((event) => <article key={event.id} role="listitem" className="liquid-glass source-event-card"><div className="source-event-heading"><span className="badge badge-info">{event.provider}</span><span className="badge">{event.event_type}</span><strong>{event.installation_id}</strong><span className="badge">{event.routing_state}</span><time>{event.received_at}</time></div><p>{event.conversation_id ?? "—"} / {event.thread_id ?? "—"}</p>{event.reaction_name && <p><strong>:{event.reaction_name}:</strong>{event.reaction_target_kind && event.reaction_target_id && <> · {event.reaction_target_kind} / {event.reaction_target_id}</>}</p>}{event.last_error_code && <p className="field-error">{event.last_error_code}</p>}{event.automation_route_id && <p><span className="badge">{event.automation_status}</span>{event.automation_binding_name && <> · {event.automation_binding_name}</>}{event.automation_template_name && <> → {event.automation_template_name}</>}</p>}<div className="decision-actions">{event.routed_task_id && <button className="btn btn-ghost" onClick={() => onOpenTask(event.routed_task_id!)}>{i18n.sources.openProcess}</button>}{event.automation_route_id && <button className="btn btn-ghost" onClick={() => onNavigate({ page: "sources", section: "automations", automationView: "routes", resourceId: event.automation_route_id! })}>Open route</button>}{canAccess("operator") && routes[event.id]?.permalink && <a className="btn btn-ghost" href={routes[event.id].permalink!} target="_blank" rel="noreferrer">{i18n.sources.openSlack}</a>}{canAccess("admin") && ["failed", "needs_attention"].includes(event.routing_state) && <button className="btn btn-secondary" onClick={() => void replay(event.id)}>{i18n.sources.replay}</button>}</div></article>)}</div></section>;
}

function ProcessBindings({ selectedTaskId, onOpenTask }: { selectedTaskId?: string; onOpenTask: (id: string) => void }) {
  const [taskId, setTaskId] = useState(selectedTaskId ?? ""); const [bindings, setBindings] = useState<Array<{ id: string; task_id: string; provider: string; installation_id: string; conversation_id: string | null; thread_id: string | null; binding_type: string; created_at: string }>>([]); const [error, setError] = useState<string | null>(null);
  const load = async () => { if (!taskId.trim()) return; setError(null); try { setBindings(await invoke("source_binding_list", { task_id: taskId.trim() })); } catch (cause) { setError(String(cause)); } };
  useEffect(() => { if (selectedTaskId) void load(); }, []);
  return <section className="liquid-glass process-bindings"><h2>Process source bindings</h2><p>Inspect the conversations and threads correlated with a process.</p><div className="route-filters"><label>Process ID<input value={taskId} onChange={(event) => setTaskId(event.target.value)} /></label><button className="btn btn-secondary" onClick={() => void load()}>Find bindings</button></div>{error && <p role="alert" className="attention-error">{error}</p>}{bindings.map((item) => <article className="automation-row" key={item.id}><strong>{item.provider} · {item.binding_type}</strong><small>{item.installation_id} / {item.conversation_id ?? "—"} / {item.thread_id ?? "—"}</small></article>)}{bindings.length > 0 && <button className="btn btn-ghost" onClick={() => onOpenTask(taskId)}>Open process</button>}</section>;
}
