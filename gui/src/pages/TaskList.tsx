import { useEffect } from "react";
import { useGrpc } from "../hooks/useGrpc";
import { useRole } from "../hooks/useRole";
import Skeleton from "../components/Skeleton";
import i18n from "../lib/i18n";
import type { TaskSummary } from "../lib/types";

interface Props {
  onSelect: (taskId: string) => void;
}

function statusBadgeClass(status: string): string {
  switch (status.toLowerCase()) {
    case "completed":
    case "succeeded":
      return "badge badge-success status-transition";
    case "running":
    case "in_progress":
      return "badge badge-info status-transition";
    case "failed":
    case "error":
      return "badge badge-danger status-transition";
    case "paused":
    case "pending":
      return "badge badge-warning status-transition";
    default:
      return "badge status-transition";
  }
}

function progressText(t: TaskSummary): string {
  if (t.total_items === 0) return "-";
  return `${t.finished_items}/${t.total_items}`;
}

export default function TaskList({ onSelect }: Props) {
  const { data, error, loading, call } = useGrpc<TaskSummary[]>("task_list");
  const { canAccess } = useRole();

  useEffect(() => {
    call();
  }, [call]);

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", marginBottom: 20 }}>
        <h1 className="page-title" style={{ marginBottom: 0 }}>{i18n.taskList.title}</h1>
        <button
          className="btn btn-ghost"
          style={{ marginLeft: 12 }}
          onClick={() => call()}
        >
          {i18n.taskList.refreshBtn}
        </button>
      </div>

      {loading && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <Skeleton height={48} />
          <Skeleton height={48} />
          <Skeleton height={48} />
        </div>
      )}
      {error && <p style={{ color: "var(--danger)" }}>{error}</p>}

      {data && data.length === 0 && (
        <div className="liquid-glass">
          <p style={{ color: "var(--text-secondary)" }}>{i18n.taskList.noTasks}</p>
        </div>
      )}

      {data && data.length > 0 && (
        <div className="liquid-glass" style={{ padding: 0, overflow: "hidden" }}>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr style={{ borderBottom: "1px solid var(--glass-border-subtle)" }}>
                <th style={thStyle}>{i18n.taskList.colName}</th>
                <th style={thStyle}>{i18n.taskList.colStatus}</th>
                <th style={thStyle}>{i18n.taskList.colProgress}</th>
                <th style={thStyle}>{i18n.taskList.colUpdated}</th>
                {canAccess("operator") && <th style={thStyle}>{i18n.taskList.colActions}</th>}
              </tr>
            </thead>
            <tbody>
              {data.map((task) => (
                <tr
                  key={task.id}
                  style={{ borderBottom: "1px solid var(--glass-border-subtle)", cursor: "pointer" }}
                  onClick={() => onSelect(task.id)}
                >
                  <td style={tdStyle}>{task.name || task.id}</td>
                  <td style={tdStyle}>
                    <span className={statusBadgeClass(task.status)}>{task.status}</span>
                  </td>
                  <td style={tdStyle}>{progressText(task)}</td>
                  <td style={tdStyle}>{task.updated_at}</td>
                  {canAccess("operator") && (
                    <td style={tdStyle}>
                      <button
                        className="btn btn-ghost"
                        onClick={(e) => { e.stopPropagation(); onSelect(task.id); }}
                      >
                        {i18n.taskList.viewBtn}
                      </button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

const thStyle: React.CSSProperties = {
  padding: "12px 16px",
  textAlign: "left",
  color: "var(--text-secondary)",
  fontSize: 13,
  fontWeight: 600,
};

const tdStyle: React.CSSProperties = {
  padding: "10px 16px",
  fontSize: 14,
};
