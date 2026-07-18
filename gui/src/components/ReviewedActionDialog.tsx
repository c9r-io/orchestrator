import { useEffect, useId, useRef, useState } from "react";

interface Props {
  open: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  destructive?: boolean;
  allowAdoptCurrent?: boolean;
  onConfirm: (reason: string, adoptCurrent: boolean) => void;
  onCancel: () => void;
}

export default function ReviewedActionDialog({ open, title, description, confirmLabel, destructive = false, allowAdoptCurrent = false, onConfirm, onCancel }: Props) {
  const [reason, setReason] = useState("");
  const [adoptCurrent, setAdoptCurrent] = useState(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    setReason(""); setAdoptCurrent(false);
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); onCancel(); return; }
      if (event.key !== "Tab") return;
      const controls = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), textarea:not([disabled])") ?? []);
      if (!controls.length) return;
      const first = controls[0]; const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", handler);
    requestAnimationFrame(() => dialogRef.current?.querySelector<HTMLElement>("textarea")?.focus());
    return () => { document.removeEventListener("keydown", handler); previousFocus.current?.focus(); };
  }, [onCancel, open]);

  if (!open) return null;
  return <div className="dialog-overlay" role="presentation" onClick={onCancel}>
    <div ref={dialogRef} className="liquid-glass dialog-content" role="dialog" aria-modal="true" aria-labelledby={titleId} onClick={(event) => event.stopPropagation()}>
      <h2 id={titleId}>{title}</h2><p>{description}</p>
      <label className="field-stack">Audit reason<textarea value={reason} onChange={(event) => setReason(event.target.value)} placeholder="Why is this reviewed change needed?" maxLength={500} /></label>
      {allowAdoptCurrent && <label className="checkbox-row"><input type="checkbox" checked={adoptCurrent} onChange={(event) => setAdoptCurrent(event.target.checked)} />Adopt the current binding/template revision for replay</label>}
      <div className="dialog-actions"><button className="btn btn-ghost" onClick={onCancel}>Cancel</button><button className={`btn ${destructive ? "btn-destructive" : "btn-primary"}`} disabled={!reason.trim()} onClick={() => onConfirm(reason.trim(), adoptCurrent)}>{confirmLabel}</button></div>
    </div>
  </div>;
}
