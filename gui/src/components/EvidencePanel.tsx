import type { TimelineEntry } from "../lib/types";

export default function EvidencePanel({ entry }: { entry: TimelineEntry | null }) {
  return <section className="context-panel" aria-labelledby="evidence-title">
    <h3 id="evidence-title">Evidence</h3>
    {!entry && <p className="context-empty">Select a timeline entry to inspect its evidence.</p>}
    {entry && <>
      <p className="context-summary">{entry.summary || entry.title}</p>
      <dl className="evidence-context">
        {entry.step_id && <div><dt>Step</dt><dd>{entry.step_id}</dd></div>}
        {entry.session_id && <div><dt>Session</dt><dd>{entry.session_id}</dd></div>}
        {entry.checkpoint_id && <div><dt>Checkpoint</dt><dd>{entry.checkpoint_id}</dd></div>}
      </dl>
      <ul className="evidence-list">
        {entry.evidence.map((evidence, index) => <li key={`${evidence.kind}-${evidence.label}-${index}`}>
          <span className="status-shape" aria-hidden="true" /><span><strong>{evidence.label}</strong><small>{evidence.kind}{evidence.redacted ? " · redacted" : ""}</small></span>
        </li>)}
      </ul>
      {entry.evidence.length === 0 && <p className="context-empty">No evidence references were projected for this entry.</p>}
    </>}
  </section>;
}
