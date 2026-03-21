import { useEffect, useRef, useMemo, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useGrpc } from "../hooks/useGrpc";
import { useStream } from "../hooks/useStream";
import { useRole } from "../hooks/useRole";
import ProgressBar from "../components/ProgressBar";
import StatusIcon from "../components/StatusIcon";
import ConfirmDialog from "../components/ConfirmDialog";
import ExpertPanel from "../components/ExpertPanel";
import type { TaskDetail as TaskDetailType, LogLine, WatchSnapshot } from "../lib/types";

interface Props {
  taskId: string;
  onBack: () => void;
}

export default function TaskDetail({ taskId, onBack }: Props) {
  const { data, error, call } = useGrpc<TaskDetailType>("task_info");
  const { canAccess } = useRole();
  const logEndRef = useRef<HTMLDivElement>(null);
  const [expert, setExpert] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [actionErr, setActionErr] = useState<string | null>(null);

  const streamParams = useMemo(() => ({ task_id: taskId }), [taskId]);
  const { data: logs, active, start, stop } = useStream<LogLine>(
    "start_task_follow",
    "stop_task_follow",
    `task-follow-${taskId}`,
    streamParams
  );

  const reload = useCallback(() => {
    call({ task_id: taskId });
  }, [call, taskId]);

  useEffect(() => {
    reload();
  }, [reload]);

  // TaskWatch: live status/items updates.
  const [liveData, setLiveData] = useState<TaskDetailType | null>(null);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    (async () => {
      unlisten = await listen<WatchSnapshot>(`task-watch-${taskId}`, (event) => {
        if (cancelled) return;
        const snap = event.payload;
        // Merge watch snapshot into detail view.
        setLiveData((prev) => ({
          id: snap.task.id,
          name: snap.task.name,
          status: snap.task.status,
          goal: prev?.goal ?? snap.task.goal ?? "",
          total_items: snap.task.total_items,
          finished_items: snap.task.finished_items,
          failed_items: snap.task.failed_items,
          created_at: snap.task.created_at,
          updated_at: snap.task.updated_at,
          project_id: snap.task.project_id,
          workflow_id: snap.task.workflow_id,
          items: snap.items,
        }));
      });
      try {
        await invoke("start_task_watch", { task_id: taskId, interval_secs: 2 });
      } catch {
        // Task may already be completed.
      }
    })();

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      invoke("stop_task_watch", { task_id: taskId }).catch(() => {});
    };
  }, [taskId]);

  // Use live data if available, otherwise fall back to initial load.
  const displayData = liveData ?? data;

  // Auto-scroll log viewer.
  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  // Keyboard shortcuts.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "e") {
        e.preventDefault();
        setExpert((v) => !v);
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, []);

  const doAction = async (cmd: string, params: Record<string, unknown>) => {
    setActionErr(null);
    setActionMsg(null);
    try {
      const result = await invoke<{ message: string }>(cmd, params);
      setActionMsg(result.message);
      reload();
    } catch (e) {
      setActionErr(typeof e === "string" ? e : String(e));
    }
  };

  const handlePause = () => doAction("task_pause", { task_id: taskId });
  const handleResume = () => doAction("task_resume", { task_id: taskId });
  const handleDelete = async () => {
    setShowDelete(false);
    await doAction("task_delete", { task_id: taskId, force: true });
    onBack();
  };

  const isRunning = displayData?.status.toLowerCase() === "running" || data?.status.toLowerCase() === "in_progress";
  const isPaused = displayData?.status.toLowerCase() === "paused";
  const isFailed = displayData?.status.toLowerCase() === "failed" || data?.status.toLowerCase() === "error";

  return (
    <div>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
        <button className="btn btn-ghost" onClick={onBack} aria-label="返回列表">
          &larr; 返回
        </button>
        <span style={{ flex: 1 }} />

        {/* Action buttons */}
        {canAccess("operator") && (
          <>
            {isRunning && (
              <button className="btn btn-secondary" onClick={handlePause} aria-label="暂停任务">
                暂停
              </button>
            )}
            {isPaused && (
              <button className="btn btn-secondary" onClick={handleResume} aria-label="恢复任务">
                恢复
              </button>
            )}
            {isFailed && displayData?.items.some((i) => i.status.toLowerCase() === "failed") && (
              <button
                className="btn btn-secondary"
                onClick={() => {
                  const failedItem = displayData.items.find((i) => i.status.toLowerCase() === "failed");
                  if (failedItem) doAction("task_retry", { task_item_id: failedItem.id });
                }}
                aria-label="重试失败项"
              >
                重试
              </button>
            )}
          </>
        )}

        <button
          className={`btn ${expert ? "btn-primary" : "btn-ghost"}`}
          onClick={() => setExpert((v) => !v)}
          aria-label="切换专家模式 (Cmd+E)"
        >
          {expert ? "专家 ✓" : "专家"}
        </button>

        {canAccess("admin") && (
          <button
            className="btn btn-destructive"
            onClick={() => setShowDelete(true)}
            aria-label="删除任务"
          >
            删除
          </button>
        )}
      </div>

      {/* Status messages */}
      {actionMsg && <p style={{ color: "var(--success)", fontSize: 13, marginBottom: 8 }}>{actionMsg}</p>}
      {actionErr && <p style={{ color: "var(--danger)", fontSize: 13, marginBottom: 8 }}>{actionErr}</p>}
      {error && <p style={{ color: "var(--danger)", marginBottom: 8 }}>{error}</p>}

      {displayData && (
        <>
          {/* Task info card */}
          <div className="liquid-glass" style={{ marginBottom: 16 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
              <StatusIcon status={displayData.status} />
              <h2 style={{ flex: 1, fontSize: 18 }}>{displayData.name || displayData.id}</h2>
            </div>

            {displayData.total_items > 0 && (
              <ProgressBar finished={displayData.finished_items} total={displayData.total_items} />
            )}

            {displayData.goal && (
              <p style={{ marginTop: 8, color: "var(--text-secondary)", fontSize: 14 }}>
                {displayData.goal}
              </p>
            )}

            {/* Items list */}
            {displayData.items.length > 0 && (
              <div style={{ marginTop: 16 }}>
                <h3 style={{ fontSize: 14, marginBottom: 8, color: "var(--text-secondary)" }}>
                  步骤进度
                </h3>
                {displayData.items.map((item) => (
                  <div
                    key={item.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "4px 0",
                      borderBottom: "1px solid var(--glass-border-subtle)",
                      fontSize: 13,
                    }}
                  >
                    <span style={{ color: "var(--text-tertiary)", minWidth: 24 }}>
                      {item.order_no}.
                    </span>
                    <StatusIcon status={item.status} size="sm" />
                    <span
                      style={{
                        flex: 1,
                        color:
                          item.status.toLowerCase() === "running"
                            ? "var(--accent)"
                            : "var(--text-primary)",
                      }}
                    >
                      {item.qa_file_path || `Item ${item.order_no}`}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Expert mode panel OR log streaming */}
          {expert ? (
            <ExpertPanel taskDetail={displayData} />
          ) : (
            <div className="liquid-glass">
              <div style={{ display: "flex", alignItems: "center", marginBottom: 12 }}>
                <h3 style={{ flex: 1 }}>实时日志</h3>
                {!active ? (
                  <button className="btn btn-primary" onClick={start} aria-label="开始追踪日志">
                    追踪
                  </button>
                ) : (
                  <button className="btn btn-secondary" onClick={stop} aria-label="停止追踪日志">
                    停止
                  </button>
                )}
              </div>

              <div
                role="log"
                aria-label="任务实时日志"
                aria-live="polite"
                style={{
                  background: "var(--bg-secondary)",
                  borderRadius: 12,
                  padding: 12,
                  maxHeight: 400,
                  overflowY: "auto",
                  fontFamily: "monospace",
                  fontSize: 13,
                  lineHeight: 1.6,
                }}
              >
                {logs.length === 0 && (
                  <p style={{ color: "var(--text-tertiary)" }}>
                    {active ? "等待日志输出..." : "点击「追踪」开始接收日志流。"}
                  </p>
                )}
                {logs.map((log, i) => (
                  <div key={i}>
                    <span style={{ color: "var(--text-tertiary)", marginRight: 8 }}>
                      {log.timestamp}
                    </span>
                    <span>{log.line}</span>
                  </div>
                ))}
                <div ref={logEndRef} />
              </div>
            </div>
          )}
        </>
      )}

      <ConfirmDialog
        open={showDelete}
        title="删除任务"
        message="确定要删除这个任务吗？此操作不可撤销。"
        confirmLabel="确认删除"
        destructive
        onConfirm={handleDelete}
        onCancel={() => setShowDelete(false)}
      />
    </div>
  );
}
