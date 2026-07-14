import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface MetricBucket { start: string; sample_count: number; sum: number; min: number; max: number }
interface MetricAggregate {
  name: string;
  labels: Record<string, string>;
  sample_count: number;
  sum: number;
  min: number | null;
  max: number | null;
  value: number;
  numerator: number | null;
  denominator: number | null;
  histogram: Record<string, number>;
  buckets: MetricBucket[];
}
interface ProjectorHealth {
  projector: string;
  project_id: string;
  cursor: string;
  lag_count: number;
  failure_count: number;
  last_error_code: string | null;
  last_success_at: string | null;
  updated_at: string;
}
interface ProcessOperationsMetrics {
  schema_version: number;
  project_id: string;
  window_start: string;
  window_end: string;
  generated_at: string;
  coverage_start: string | null;
  partial: boolean;
  collection_enabled: boolean;
  metrics: MetricAggregate[];
  projector_health: ProjectorHealth[];
}

const windows = [
  { value: "1h", bucket: "5m", label: "1 hour" },
  { value: "24h", bucket: "1h", label: "24 hours" },
  { value: "7d", bucket: "6h", label: "7 days" },
];

const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });
const duration = (seconds: number) => seconds >= 3600 ? `${number.format(seconds / 3600)}h` : seconds >= 60 ? `${number.format(seconds / 60)}m` : `${number.format(seconds)}s`;

export default function ExpertOperations() {
  const [project, setProject] = useState("default");
  const [selectedWindow, setSelectedWindow] = useState(windows[1]);
  const [snapshot, setSnapshot] = useState<ProcessOperationsMetrics | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    if (!project.trim()) { setError("Project is required."); return; }
    setLoading(true); setError(null);
    void invoke<ProcessOperationsMetrics>("process_metrics_get", {
        project_id: project.trim(), window: selectedWindow.value, bucket: selectedWindow.bucket,
      })
      .then(setSnapshot)
      .catch((cause) => setError(String(cause)))
      .finally(() => setLoading(false));
  }, [project, selectedWindow]);

  useEffect(() => { load(); }, [load]);

  const grouped = useMemo(() => {
    const values = new Map<string, MetricAggregate[]>();
    snapshot?.metrics.forEach((metric) => values.set(metric.name, [...(values.get(metric.name) ?? []), metric]));
    return values;
  }, [snapshot]);
  const sum = (name: string) => (grouped.get(name) ?? []).reduce((total, metric) => total + metric.value, 0);
  const samples = (name: string) => (grouped.get(name) ?? []).reduce((total, metric) => total + metric.sample_count, 0);
  const average = (name: string) => { const count = samples(name); return count ? sum(name) / count : 0; };
  const metric = (name: string) => grouped.get(name)?.[0];
  const hasActivity = snapshot?.metrics.some((item) => item.sample_count > 0 || (item.denominator ?? 0) > 0 || item.value > 0) ?? false;
  const stale = snapshot ? Date.now() - new Date(snapshot.generated_at).getTime() > 120_000 : false;
  const cards = snapshot ? [
    ["Attention opened", number.format(sum("attention_open_total")), `${number.format(sum("attention_active"))} active`],
    ["Time to claim", duration(average("attention_time_to_claim_seconds")), `${samples("attention_time_to_claim_seconds")} claimed episodes`],
    ["Human attention", duration(sum("process_human_attention_seconds")), "actionable time only"],
    ["Autonomous completion", `${number.format((metric("process_autonomous_completion_ratio")?.value ?? 0) * 100)}%`, `${metric("process_autonomous_completion_ratio")?.numerator ?? 0}/${metric("process_autonomous_completion_ratio")?.denominator ?? 0} tasks`],
    ["Handoff to action", duration(average("handoff_to_productive_action_seconds")), `${samples("handoff_to_productive_action_seconds")} productive handoffs`],
    ["Resume attempts", number.format(sum("resume_attempt_total")), `${(grouped.get("resume_attempt_total") ?? []).filter((item) => item.labels.result === "succeeded").reduce((total, item) => total + item.value, 0)} succeeded`],
    ["Session attachments", number.format(sum("session_attachment_total")), "reader and writer"],
    ["Source deduplicated", number.format(sum("source_event_deduplicated_total")), "retained local observations"],
    ["Repeated failure", `${number.format((metric("process_repeated_failure_rate")?.value ?? 0) * 100)}%`, "failed command runs"],
    ["Degenerate loops", `${number.format((metric("process_degenerate_loop_rate")?.value ?? 0) * 100)}%`, "item/phase groups"],
  ] : [];

  return <div className="operations" aria-busy={loading}>
    <header className="operations-header">
      <div><h2>Operations</h2><p>Local, project-scoped process health. Aggregates support triage; audit and timeline remain forensic truth.</p></div>
      <button className="btn btn-secondary" onClick={load} disabled={loading}>Refresh</button>
    </header>
    <div className="operations-controls">
      <label>Project<input value={project} onChange={(event) => setProject(event.target.value)} aria-label="Metrics project" /></label>
      <fieldset><legend>Window</legend>{windows.map((item) => <button key={item.value} className={`btn ${selectedWindow.value === item.value ? "btn-primary" : "btn-ghost"}`} aria-pressed={selectedWindow.value === item.value} onClick={() => setSelectedWindow(item)}>{item.label}</button>)}</fieldset>
    </div>
    {error && <div className="operations-state operations-error" role="alert"><strong>Metrics unavailable</strong><span>{error}</span><button className="btn btn-secondary" onClick={load}>Retry</button></div>}
    {!error && loading && <div className="operations-state" role="status">Loading operational metrics…</div>}
    {!error && !loading && snapshot && <>
      <div className={`operations-freshness ${stale || !snapshot.collection_enabled ? "is-stale" : ""}`} role="status">
        <span>{snapshot.collection_enabled ? stale ? "Stale snapshot" : "Fresh snapshot" : "Collection disabled"}</span>
        <time dateTime={snapshot.generated_at}>Generated {new Date(snapshot.generated_at).toLocaleString()}</time>
        {snapshot.partial && <span>Partial historical coverage{snapshot.coverage_start ? ` since ${new Date(snapshot.coverage_start).toLocaleString()}` : ""}</span>}
      </div>
      {!hasActivity ? <div className="operations-state"><strong>No process activity in this window</strong><span>Try a larger window or confirm the selected project.</span></div> : <div className="operations-grid">
        {cards.map(([label, value, hint]) => <article className="operations-card" key={label}><span>{label}</span><strong>{value}</strong><small>{hint}</small></article>)}
      </div>}
      <section className="operations-detail" aria-labelledby="runtime-metrics-title">
        <h3 id="runtime-metrics-title">Projection and timeline health</h3>
        <div className="operations-runtime-grid">
          <div><span>Timeline average</span><strong>{duration(average("timeline_projection_seconds"))}</strong><small>max {duration(metric("timeline_projection_seconds")?.max ?? 0)}</small></div>
          <div><span>Timeline response</span><strong>{number.format(average("timeline_response_bytes") / 1024)} KiB</strong><small>{samples("timeline_response_bytes")} projections</small></div>
          <div><span>Stream reconnects</span><strong>{number.format(sum("stream_reconnect_total"))}</strong><small>bounded UI telemetry</small></div>
        </div>
        {snapshot.projector_health.length === 0 ? <p className="operations-muted">No projector health samples yet.</p> : <table className="operations-table"><thead><tr><th>Projector</th><th>Lag</th><th>Failures</th><th>Last success</th><th>Error</th></tr></thead><tbody>{snapshot.projector_health.map((item) => <tr key={`${item.projector}-${item.project_id}`}><td>{item.projector}</td><td>{item.lag_count}</td><td>{item.failure_count}</td><td>{item.last_success_at ? new Date(item.last_success_at).toLocaleString() : "—"}</td><td>{item.last_error_code ?? "—"}</td></tr>)}</tbody></table>}
      </section>
    </>}
  </div>;
}
