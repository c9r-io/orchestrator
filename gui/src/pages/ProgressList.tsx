import { useEffect, useCallback, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import ProgressBar from "../components/ProgressBar";
import StatusIcon from "../components/StatusIcon";
import Skeleton from "../components/Skeleton";
import i18n from "../lib/i18n";
import type { TaskSummary, WatchSnapshot } from "../lib/types";

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

function isActive(status: string): boolean {
  const s = status.toLowerCase();
  return s === "running" || s === "in_progress" || s === "paused";
}

export default function ProgressList({ onSelect }: Props) {
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const watchedRef = useRef<Set<string>>(new Set());
  const unlistenersRef = useRef<Map<string, UnlistenFn>>(new Map());

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await invoke<TaskSummary[]>("task_list", {});
      setTasks(data.sort(sortTasks));
      return data;
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
      return [];
    } finally {
      setLoading(false);
    }
  }, []);

  // Subscribe to TaskWatch for active tasks.
  const startWatching = useCallback(async (taskList: TaskSummary[]) => {
    const activeTasks = taskList.filter((t) => isActive(t.status));
    for (const task of activeTasks) {
      if (watchedRef.current.has(task.id)) continue;
      watchedRef.current.add(task.id);

      // Listen for watch events.
      const unlisten = await listen<WatchSnapshot>(
        `task-watch-${task.id}`,
        (event) => {
          const snapshot = event.payload;
          setTasks((prev) =>
            prev
              .map((t) => (t.id === snapshot.task.id ? snapshot.task : t))
              .sort(sortTasks)
          );
        }
      );
      unlistenersRef.current.set(task.id, unlisten);

      // Start the watch stream.
      try {
        await invoke("start_task_watch", {
          task_id: task.id,
          interval_secs: 3,
        });
      } catch {
        // Task may have completed before watch started.
      }
    }
  }, []);

  // Cleanup all watches.
  const stopAllWatches = useCallback(async () => {
    for (const [taskId, unlisten] of unlistenersRef.current) {
      unlisten();
      try {
        await invoke("stop_task_watch", { task_id: taskId });
      } catch {
        // Ignore errors on cleanup.
      }
    }
    unlistenersRef.current.clear();
    watchedRef.current.clear();
  }, []);

  useEffect(() => {
    (async () => {
      const data = await load();
      await startWatching(data);
    })();
    return () => {
      stopAllWatches();
    };
  }, [load, startWatching, stopAllWatches]);

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", marginBottom: 20 }}>
        <h1 className="page-title" style={{ marginBottom: 0 }}>{i18n.progressList.title}</h1>
        <button className="btn btn-ghost" style={{ marginLeft: 12 }} onClick={load}>
          {i18n.common.refresh}
        </button>
      </div>

      {loading && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <Skeleton height={80} />
          <Skeleton height={80} />
          <Skeleton height={80} />
        </div>
      )}
      {error && <p style={{ color: "var(--danger)" }}>{error}</p>}

      {!loading && tasks.length === 0 && (
        <div className="liquid-glass" style={{ textAlign: "center" }}>
          <p style={{ color: "var(--text-secondary)" }}>{i18n.progressList.noTasks}</p>
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
            aria-label={i18n.progressList.taskLabel(task.name || task.id)}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
              <StatusIcon status={task.status} />
              <span style={{ flex: 1, fontWeight: 500, fontSize: 15 }}>
                {task.name || task.goal?.slice(0, 40) || task.id.slice(0, 8)}
              </span>
              {isActive(task.status) && (
                <span style={{ fontSize: 11, color: "var(--accent)" }}>{i18n.progressList.realtime}</span>
              )}
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
              <span>{i18n.progressList.startedAt(task.created_at)}</span>
              {task.failed_items > 0 && (
                <span style={{ color: "var(--danger)" }}>
                  {i18n.progressList.failedItems(task.failed_items)}
                </span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
