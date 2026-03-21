import { useEffect, useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ProgressBar from "../components/ProgressBar";
import StatusIcon from "../components/StatusIcon";
import type { TaskSummary } from "../lib/types";

interface Props {
  onSelect: (taskId: string) => void;
}

const STATUS_ORDER: Record<string, number> = {
  running: 0,
  in_progress: 0,
  paused: 1,
  failed: 2,
  error: 2,
  completed: 3,
  succeeded: 3,
};

function sortTasks(a: TaskSummary, b: TaskSummary): number {
  const oa = STATUS_ORDER[a.status.toLowerCase()] ?? 4;
  const ob = STATUS_ORDER[b.status.toLowerCase()] ?? 4;
  if (oa !== ob) return oa - ob;
  return b.updated_at.localeCompare(a.updated_at);
}

export default function ProgressList({ onSelect }: Props) {
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await invoke<TaskSummary[]>("task_list", {});
      setTasks(data.sort(sortTasks));
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", marginBottom: 20 }}>
        <h1 className="page-title" style={{ marginBottom: 0 }}>进度观察</h1>
        <button className="btn btn-ghost" style={{ marginLeft: 12 }} onClick={load}>
          刷新
        </button>
      </div>

      {loading && <p style={{ color: "var(--text-secondary)" }}>加载中...</p>}
      {error && <p style={{ color: "var(--danger)" }}>{error}</p>}

      {!loading && tasks.length === 0 && (
        <div className="liquid-glass" style={{ textAlign: "center" }}>
          <p style={{ color: "var(--text-secondary)" }}>暂无任务</p>
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        {tasks.map((task) => (
          <div
            key={task.id}
            className="liquid-glass"
            style={{ cursor: "pointer", padding: 16 }}
            onClick={() => onSelect(task.id)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => e.key === "Enter" && onSelect(task.id)}
            aria-label={`任务: ${task.name || task.id}`}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
              <StatusIcon status={task.status} />
              <span style={{ flex: 1, fontWeight: 500, fontSize: 15 }}>
                {task.name || task.id.slice(0, 8)}
              </span>
            </div>

            {task.total_items > 0 && (
              <ProgressBar finished={task.finished_items} total={task.total_items} />
            )}

            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                marginTop: 8,
                fontSize: 12,
                color: "var(--text-tertiary)",
              }}
            >
              <span>开始于 {task.created_at}</span>
              {task.failed_items > 0 && (
                <span style={{ color: "var(--danger)" }}>
                  {task.failed_items} 项失败
                </span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
