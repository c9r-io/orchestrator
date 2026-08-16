import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
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
  reviewReturnTargetRef?: RefObject<HTMLElement | null>;
  onExecuted: () => void;
}

const focusableSelector = [
  "button:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "input:not([disabled])",
  "a[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(", ");

function isVisibleTarget(element: HTMLElement | null): element is HTMLElement {
  if (!element?.isConnected || element === document.body || element === document.documentElement) return false;
  if (element.hidden || element.closest("[hidden], [aria-hidden='true']")) return false;
  if (element.getAttribute("aria-disabled") === "true") return false;
  if ("disabled" in element && Boolean((element as HTMLButtonElement).disabled)) return false;
  const style = window.getComputedStyle(element);
  return style.display !== "none" && style.visibility !== "hidden";
}

function isFocusableTarget(element: HTMLElement | null): element is HTMLElement {
  return isVisibleTarget(element) && element.matches(focusableSelector);
}

const modes = [
  ["continue_task", "Continue task"],
  ["retry_item", "Retry failed item"],
  ["restart_from_boundary", "Restart from boundary"],
  ["resume_provider_session", "Resume provider session"],
] as const;

export default function HandoffPanel({
  taskId,
  canGenerate,
  canExecute,
  reviewRequest,
  reviewReturnTargetRef,
  onExecuted,
}: Props) {
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
  const panelRef = useRef<HTMLElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const resumeButtonRef = useRef<HTMLButtonElement>(null);
  const boundaryRef = useRef<HTMLSelectElement>(null);
  const reasonRef = useRef<HTMLTextAreaElement>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const wasDialogOpenRef = useRef(false);
  const hadPlanRef = useRef(false);
  const mountedRef = useRef(true);
  const openRequestRef = useRef(0);
  const handledReviewRequestRef = useRef(0);
  const activeTaskIdRef = useRef(taskId);
  const busyRef = useRef(false);
  busyRef.current = busy;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      openRequestRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (activeTaskIdRef.current === taskId) return;
    activeTaskIdRef.current = taskId;
    openRequestRef.current += 1;
    handledReviewRequestRef.current = 0;
    setDialogOpen(false);
    setBoundaries([]);
    setPlan(null);
    setResult(null);
    setError(null);
  }, [taskId]);

  const closeDialog = useCallback(() => {
    if (!busyRef.current) setDialogOpen(false);
  }, []);

  useEffect(() => {
    if (!dialogOpen) return;
    const dialog = dialogRef.current;
    const focusable = () => Array.from(dialog?.querySelectorAll<HTMLElement>(focusableSelector) ?? [])
      .filter(isFocusableTarget);
    focusable()[0]?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeDialog();
        return;
      }
      if (event.key !== "Tab") return;
      const controls = focusable();
      if (controls.length === 0) return;
      const first = controls[0];
      const last = controls[controls.length - 1];
      const active = document.activeElement;
      if (!dialog?.contains(active)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [closeDialog, dialogOpen]);

  useEffect(() => {
    const wasOpen = wasDialogOpenRef.current;
    wasDialogOpenRef.current = dialogOpen;
    if (!wasOpen || dialogOpen) return;
    const frame = requestAnimationFrame(() => {
      const candidates = [
        returnFocusRef.current,
        reviewReturnTargetRef?.current ?? null,
        resumeButtonRef.current,
      ];
      returnFocusRef.current = null;
      for (const candidate of candidates) {
        if (!isFocusableTarget(candidate)) continue;
        candidate.focus();
        return;
      }
      if (isVisibleTarget(panelRef.current)) panelRef.current.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [dialogOpen, reviewReturnTargetRef]);

  useEffect(() => {
    if (!dialogOpen) {
      hadPlanRef.current = false;
      return;
    }
    const hadPlan = hadPlanRef.current;
    hadPlanRef.current = Boolean(plan);
    if (Boolean(plan) === hadPlan) return;
    if (plan) reasonRef.current?.focus();
    else boundaryRef.current?.focus();
  }, [dialogOpen, plan]);

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

  /// Returns whether the dialog actually opened.
  ///
  /// The caller needs that answer: an open can be abandoned after its await —
  /// the component unmounted, or a newer open superseded this one — and a
  /// caller that assumed success would record a request as handled that never
  /// produced a dialog.
  const openResume = useCallback(async (source?: HTMLElement | null): Promise<boolean> => {
    const request = ++openRequestRef.current;
    const focusCandidate = source ?? null;
    returnFocusRef.current = isFocusableTarget(focusCandidate)
      ? focusCandidate
      : resumeButtonRef.current;
    setBusy(true);
    setError(null);
    try {
      const values = await invoke<ResumeBoundary[]>("resume_boundary_list", { task_id: taskId });
      if (!mountedRef.current || request !== openRequestRef.current) return false;
      setBoundaries(values);
      setBoundaryId(values[0]?.id ?? "");
      setPlan(null);
      setResult(null);
      setDialogOpen(true);
      return true;
    } catch (value) {
      if (mountedRef.current && request === openRequestRef.current) setError(String(value));
      return false;
    } finally {
      if (mountedRef.current && request === openRequestRef.current) setBusy(false);
    }
  }, [taskId]);

  useEffect(() => {
    if (reviewRequest <= handledReviewRequestRef.current || !canExecute) return;
    // Recorded on success, not on dispatch. Marking it here would be marking a
    // request handled that may never open a dialog: this effect's cleanup sets
    // mountedRef false and bumps openRequestRef, so an open still awaiting
    // `resume_boundary_list` is abandoned — and with the request already
    // recorded, nothing retries it. StrictMode's mount/cleanup/mount makes that
    // sequence certain rather than occasional, which is how it surfaced: under
    // React 19 the one-click safe resume opened nothing at all. The same race is
    // reachable in production whenever that call is slower than a re-render.
    let superseded = false;
    void openResume(reviewReturnTargetRef?.current ?? resumeButtonRef.current).then((opened) => {
      if (opened && !superseded) handledReviewRequestRef.current = reviewRequest;
    });
    return () => {
      superseded = true;
    };
  }, [canExecute, openResume, reviewRequest, reviewReturnTargetRef]);

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
    <section
      ref={panelRef}
      className="liquid-glass handoff-panel"
      aria-labelledby="handoff-heading"
      aria-busy={busy || undefined}
      tabIndex={-1}
    >
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
            <button
              ref={resumeButtonRef}
              className="btn btn-primary"
              onClick={(event) => void openResume(event.currentTarget)}
              disabled={busy}
            >
              Preview resume
            </button>
          )}
        </div>
      </div>

      {error && !dialogOpen && <p className="handoff-error" role="alert">{error}</p>}
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
              <button className="btn btn-ghost" onClick={closeDialog} disabled={busy} aria-label="Close resume dialog">Close</button>
            </div>
            {error && <p className="handoff-error" role="alert">{error}</p>}

            {!plan ? (
              <>
                <label>
                  Logical boundary
                  <select ref={boundaryRef} value={boundaryId} onChange={(event) => { setBoundaryId(event.target.value); setPlan(null); }}>
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
                  <textarea ref={reasonRef} value={reason} onChange={(event) => setReason(event.target.value)} rows={3} required />
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
