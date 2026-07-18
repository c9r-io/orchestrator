import { useEffect, useId, useRef, useState } from "react";
import type { SourceConnection } from "../../lib/types";

interface Props {
  connection: SourceConnection | null;
  busy: boolean;
  onConfirm: (targetDaemonId: string, reason: string) => void;
  onCancel: () => void;
}

export default function SourceConnectionTransferDialog({ connection, busy, onConfirm, onCancel }: Props) {
  const [targetDaemonId, setTargetDaemonId] = useState("");
  const [reason, setReason] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const onCancelRef = useRef(onCancel);
  const busyRef = useRef(busy);
  const titleId = useId();
  onCancelRef.current = onCancel;
  busyRef.current = busy;

  useEffect(() => {
    if (!connection) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    setTargetDaemonId(""); setReason("");
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busyRef.current) { event.preventDefault(); onCancelRef.current(); return; }
      if (event.key !== "Tab") return;
      const controls = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), textarea:not([disabled])") ?? []);
      if (!controls.length) return;
      const first = controls[0]; const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", handler);
    dialogRef.current?.querySelector<HTMLElement>("input")?.focus();
    return () => { document.removeEventListener("keydown", handler); previousFocus.current?.focus(); };
  }, [connection]);

  if (!connection) return null;
  const valid = targetDaemonId.trim().length > 0
    && targetDaemonId.trim() !== connection.owner_daemon_id
    && reason.trim().length > 0;
  return <div className="dialog-overlay" role="presentation" onClick={() => { if (!busy) onCancel(); }}>
    <div ref={dialogRef} className="liquid-glass dialog-content" role="dialog" aria-modal="true" aria-labelledby={titleId} onClick={(event) => event.stopPropagation()}>
      <h2 id={titleId}>Transfer {connection.display_label}</h2>
      <p>The current daemon loses access immediately. The connection stays suspended until the target daemon adopts the encrypted Gateway handoff.</p>
      <label className="field-stack">Target daemon ID<input value={targetDaemonId} onChange={(event) => setTargetDaemonId(event.target.value)} placeholder="daemon-…" autoComplete="off" /></label>
      {targetDaemonId.trim() === connection.owner_daemon_id && <p className="field-error">Choose a different daemon.</p>}
      <label className="field-stack">Audit reason<textarea value={reason} onChange={(event) => setReason(event.target.value)} placeholder="Why is ownership moving?" maxLength={500} /></label>
      <div className="dialog-actions"><button className="btn btn-ghost" disabled={busy} onClick={onCancel}>Cancel</button><button className="btn btn-primary" disabled={busy || !valid} onClick={() => onConfirm(targetDaemonId.trim(), reason.trim())}>Transfer ownership</button></div>
    </div>
  </div>;
}
