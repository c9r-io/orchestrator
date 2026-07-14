import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  HandoffSnapshot,
  ResumeBoundary,
  ResumeExecution,
  ResumePlan,
} from "../lib/types";

interface Props {
  taskId: string;
  canGenerate: boolean;
  canExecute: boolean;
  reviewRequest: number;
  onExecuted: () => void;
}

const modes = [
  ["continue_task", "Continue task"],
  ["retry_item", "Retry failed item"],
  ["restart_from_boundary", "Restart from boundary"],
  ["resume_provider_session", "Resume provider session"],
] as const;

export default function HandoffPanel({ taskId, canGenerate, canExecute, reviewRequest, onExecuted }: Props) {
  const [snapshot, setSnapshot] = useState<HandoffSnapshot | null>(null);
  const [boundaries, setBoundaries] = useState<ResumeBoundary[]>([]);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [boundaryId, setBoundaryId] = useState("");
  const [mode, setMode] = useState("restart_from_boundary");
  const [plan, setPlan] = useState<ResumePlan | null>(null);
  const [reason, setReason] = useState("");
  const [elevated, setElevated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ResumeExecution | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const resumeButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!dialogOpen) return;
    const dialog = dialogRef.current;
    const focusable = () => Array.from(dialog?.querySelectorAll<HTMLElement>(
      "button:not([disabled]), select:not([disabled]), textarea:not([disabled]), input:not([disabled])"
    ) ?? []);
    focusable()[0]?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setDialogOpen(false);
        return;
      }
      if (event.key !== "Tab") return;
      const controls = focusable();
      if (controls.length === 0) return;
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    dialog?.addEventListener("keydown", handleKeyDown);
    return () => {
      dialog?.removeEventListener("keydown", handleKeyDown);
      resumeButtonRef.current?.focus();
    };
  }, [dialogOpen]);

  const generate = async () => {
    setBusy(true);
    setError(null);
    try {
      setSnapshot(await invoke<HandoffSnapshot>("handoff_generate", { task_id: taskId }));
    } catch (value) {
      setError(String(value));
    } finally {
      setBusy(false);
    }
  };

  const openResume = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const values = await invoke<ResumeBoundary[]>("resume_boundary_list", { task_id: taskId });
      setBoundaries(values);
      setBoundaryId(values[0]?.id ?? "");
      setPlan(null);
      setResult(null);
      setDialogOpen(true);
    } catch (value) {
      setError(String(value));
    } finally {
      setBusy(false);
    }
  }, [taskId]);

  useEffect(() => {
    if (reviewRequest > 0 && canExecute) void openResume();
  }, [canExecute, openResume, reviewRequest]);

  const preview = async () => {
    setBusy(true);
    setError(null);
    try {
      setPlan(await invoke<ResumePlan>("resume_plan", {
        task_id: taskId,
        boundary_id: boundaryId,
        mode,
      }));
    } catch (value) {
      setError(String(value));
    } finally {
      setBusy(false);
    }
  };

  const execute = async () => {
    if (!plan || !reason.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const execution = await invoke<ResumeExecution>("resume_execute", {
        plan_id: plan.id,
        expected_state_version: plan.expected_state_version,
        operator_reason: reason.trim(),
        idempotency_key: crypto.randomUUID(),
        elevated_confirmation: elevated,
      });
      setResult(execution);
      onExecuted();
    } catch (value) {
      setError(String(value));
    } finally {
      setBusy(false);
    }
  };

  const selected = boundaries.find((boundary) => boundary.id === boundaryId);
  const visibleModes = modes.filter(([value]) =>
    value !== "resume_provider_session" || selected?.provider_session_available
  );

  return (
    <section className="liquid-glass handoff-panel" aria-labelledby="handoff-heading">
      <div className="handoff-heading-row">
        <div>
          <h3 id="handoff-heading">Handoff & safe resume</h3>
          <p>Capture concise evidence, preview consequences, then resume from a logical boundary.</p>
        </div>
        <div className="handoff-actions">
          {canGenerate && <button className="btn btn-secondary" onClick={generate} disabled={busy}>
            Generate handoff
          </button>}
          {canExecute && (
            <button ref={resumeButtonRef} className="btn btn-primary" onClick={openResume} disabled={busy}>
              Preview resume
            </button>
          )}
        </div>
      </div>

      {error && <p className="handoff-error" role="alert">{error}</p>}
      {!canGenerate && <p className="readonly-reason">Read-only access: existing handoff context is inspectable, but generating or executing a new handoff is unavailable.</p>}
      {snapshot && (
        <div className="handoff-briefing">
          <p><strong>Current:</strong> {String(snapshot.briefing.current_state.status ?? "unknown")}</p>
          {snapshot.briefing.failure && <p><strong>Failure:</strong> evidence captured at cursor {snapshot.source_event_cursor}</p>}
          {snapshot.briefing.changed_files.length > 0 && (
            <p><strong>Changed files:</strong> {snapshot.briefing.changed_files.join(", ")}</p>
          )}
          <ul>
            {snapshot.briefing.recommendations.map((recommendation) => <li key={recommendation}>{recommendation}</li>)}
          </ul>
          <code title={snapshot.content_hash}>Snapshot {snapshot.content_hash.slice(0, 12)}</code>
        </div>
      )}

      {dialogOpen && (
        <div className="resume-dialog-backdrop" role="presentation">
          <div ref={dialogRef} className="resume-dialog liquid-glass" role="dialog" aria-modal="true" aria-labelledby="resume-title">
            <div className="handoff-heading-row">
              <h3 id="resume-title">Resume consequence preview</h3>
              <button className="btn btn-ghost" onClick={() => setDialogOpen(false)} aria-label="Close resume dialog">Close</button>
            </div>

            {!plan ? (
              <>
                <label>
                  Logical boundary
                  <select value={boundaryId} onChange={(event) => { setBoundaryId(event.target.value); setPlan(null); }}>
                    {boundaries.map((boundary) => (
                      <option key={boundary.id} value={boundary.id}>
                        {boundary.step_id ?? "current state"} · {boundary.side_effect_class}
                      </option>
                    ))}
                  </select>
                </label>
                {selected && (
                  <p className={selected.replay_safe ? "handoff-safe" : "handoff-warning"}>
                    {selected.replay_safe ? "Replay-safe" : "Elevated confirmation required"}: {selected.reason}
                  </p>
                )}
                <label>
                  Resume mode
                  <select value={mode} onChange={(event) => setMode(event.target.value)}>
                    {visibleModes.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                  </select>
                </label>
                <button className="btn btn-primary" onClick={preview} disabled={!boundaryId || busy}>Create preview</button>
              </>
            ) : (
              <>
                <div className="resume-consequence">
                  <p><strong>Mode:</strong> {plan.mode}</p>
                  <p><strong>Workspace rollback:</strong> never</p>
                  <p><strong>Expires:</strong> {new Date(plan.expires_at).toLocaleString()}</p>
                  <pre>{JSON.stringify(plan.consequence, null, 2)}</pre>
                </div>
                <label>
                  Operator reason
                  <textarea value={reason} onChange={(event) => setReason(event.target.value)} rows={3} required />
                </label>
                {plan.elevated_confirmation_required && (
                  <label className="resume-confirmation">
                    <input type="checkbox" checked={elevated} onChange={(event) => setElevated(event.target.checked)} />
                    I confirm this may repeat a non-idempotent external effect.
                  </label>
                )}
                <div className="handoff-actions">
                  <button className="btn btn-secondary" onClick={() => setPlan(null)}>Back</button>
                  <button
                    className={plan.elevated_confirmation_required ? "btn btn-destructive" : "btn btn-primary"}
                    onClick={execute}
                    disabled={busy || !reason.trim() || (plan.elevated_confirmation_required && !elevated)}
                  >
                    Execute reviewed plan
                  </button>
                </div>
                {result && <p className="handoff-safe" role="status">Resume {result.status}{result.child_task_id ? ` · child ${result.child_task_id}` : ""}</p>}
              </>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
