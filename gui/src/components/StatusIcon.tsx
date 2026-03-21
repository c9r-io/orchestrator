/** Status icon + badge component used across wish pool and progress pages. */
import i18n from "../lib/i18n";

interface Props {
  status: string;
  size?: "sm" | "md";
}

const STATUS_CONFIG: Record<string, { icon: string; cls: string; label: string }> = {
  running: { icon: "\u25CF", cls: "badge badge-info status-transition", label: i18n.status.running },
  in_progress: { icon: "\u25CF", cls: "badge badge-info status-transition", label: i18n.status.running },
  completed: { icon: "\u2713", cls: "badge badge-success status-transition", label: i18n.status.completed },
  succeeded: { icon: "\u2713", cls: "badge badge-success status-transition", label: i18n.status.completed },
  failed: { icon: "\u2717", cls: "badge badge-danger status-transition", label: i18n.status.failed },
  error: { icon: "\u2717", cls: "badge badge-danger status-transition", label: i18n.status.failed },
  paused: { icon: "\u2016", cls: "badge badge-warning status-transition", label: i18n.status.paused },
  pending: { icon: "\u25CB", cls: "badge badge-warning status-transition", label: i18n.status.pending },
  created: { icon: "\u25CB", cls: "badge badge-warning status-transition", label: i18n.status.created },
  deleted: { icon: "\u2014", cls: "badge status-transition", label: i18n.status.cancelled },
};

export default function StatusIcon({ status, size = "md" }: Props) {
  const key = status.toLowerCase();
  const cfg = STATUS_CONFIG[key] ?? { icon: "?", cls: "badge status-transition", label: status };
  const fontSize = size === "sm" ? 11 : 12;

  return (
    <span className={cfg.cls} style={{ fontSize }} aria-label={cfg.label}>
      {cfg.icon} {cfg.label}
    </span>
  );
}
