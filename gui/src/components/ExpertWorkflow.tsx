import type { TaskDetail } from "../lib/types";

interface Props {
  taskDetail: TaskDetail;
}

function statusIcon(status: string): string {
  switch (status.toLowerCase()) {
    case "completed":
    case "succeeded":
      return "✅";
    case "running":
    case "in_progress":
      return "🔵";
    case "failed":
    case "error":
      return "🔴";
    default:
      return "⬜";
  }
}

/** Simple DAG visualization using CSS — shows items as a sequential flow. */
export default function ExpertWorkflow({ taskDetail }: Props) {
  const items = taskDetail.items;

  if (items.length === 0) {
    return (
      <p style={{ color: "var(--text-secondary)" }}>暂无工作流步骤数据</p>
    );
  }

  return (
    <div>
      <h4 style={{ marginBottom: 12, color: "var(--text-secondary)", fontSize: 13 }}>
        步骤进度 ({taskDetail.finished_items}/{taskDetail.total_items})
      </h4>
      <div style={{ display: "flex", flexDirection: "column", gap: 0 }}>
        {items.map((item, idx) => (
          <div key={item.id} style={{ display: "flex", alignItems: "stretch" }}>
            {/* Connector line */}
            <div
              style={{
                width: 24,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
              }}
            >
              <div
                style={{
                  width: 2,
                  flex: 1,
                  background: idx === 0 ? "transparent" : "var(--glass-border-subtle)",
                }}
              />
              <span style={{ fontSize: 16, lineHeight: 1 }}>{statusIcon(item.status)}</span>
              <div
                style={{
                  width: 2,
                  flex: 1,
                  background:
                    idx === items.length - 1 ? "transparent" : "var(--glass-border-subtle)",
                }}
              />
            </div>
            {/* Node content */}
            <div
              style={{
                flex: 1,
                padding: "8px 12px",
                fontSize: 14,
                display: "flex",
                alignItems: "center",
                gap: 8,
              }}
            >
              <span style={{ color: "var(--text-tertiary)", fontSize: 12, minWidth: 20 }}>
                {item.order_no}.
              </span>
              <span
                style={{
                  flex: 1,
                  color:
                    item.status.toLowerCase() === "running"
                      ? "var(--accent)"
                      : "var(--text-primary)",
                  fontWeight: item.status.toLowerCase() === "running" ? 600 : 400,
                }}
              >
                {item.qa_file_path || `Step ${item.order_no}`}
              </span>
              <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
                {item.status}
              </span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
