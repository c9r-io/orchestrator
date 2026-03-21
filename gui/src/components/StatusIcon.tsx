/** Status icon + badge component used across wish pool and progress pages. */

interface Props {
  status: string;
  size?: "sm" | "md";
}

const STATUS_CONFIG: Record<string, { icon: string; cls: string; label: string }> = {
  running: { icon: "●", cls: "badge badge-info", label: "运行中" },
  in_progress: { icon: "●", cls: "badge badge-info", label: "运行中" },
  completed: { icon: "✓", cls: "badge badge-success", label: "已完成" },
  succeeded: { icon: "✓", cls: "badge badge-success", label: "已完成" },
  failed: { icon: "✗", cls: "badge badge-danger", label: "失败" },
  error: { icon: "✗", cls: "badge badge-danger", label: "失败" },
  paused: { icon: "‖", cls: "badge badge-warning", label: "已暂停" },
  pending: { icon: "○", cls: "badge badge-warning", label: "等待中" },
  created: { icon: "○", cls: "badge badge-warning", label: "已创建" },
  deleted: { icon: "—", cls: "badge", label: "已取消" },
};

export default function StatusIcon({ status, size = "md" }: Props) {
  const key = status.toLowerCase();
  const cfg = STATUS_CONFIG[key] ?? { icon: "?", cls: "badge", label: status };
  const fontSize = size === "sm" ? 11 : 12;

  return (
    <span className={cfg.cls} style={{ fontSize }} aria-label={cfg.label}>
      {cfg.icon} {cfg.label}
    </span>
  );
}
