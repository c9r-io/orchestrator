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
import i18n from "../lib/i18n";
import type { TaskDetail as TaskDetailType, LogLine, WatchSnapshot } from "../lib/types";

interface Props {
  taskId: string;
  onBack: () => void;
}

const LOG_LIMIT = 500;

export default function TaskDetail({ taskId, onBack }: Props) {
  const { data, error, call } = useGrpc<TaskDetailType>("task_info");
  const { canAccess } = useRole();
  const logContainerRef = useRef<HTMLDivElement>(null);
  const logEndRef = useRef<HTMLDivElement>(null);
  const [expert, setExpert] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [actionErr, setActionErr] = useState<string | null>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");

  const streamParams = useMemo(() => ({ task_id: taskId }), [taskId]);
  const { data: allLogs, active, start, stop } = useStream<LogLine>(
    "start_task_follow",
    "stop_task_follow",
    `task-follow-${taskId}`,
    streamParams
  );

  // Limit displayed logs.
  const logs = allLogs.length > LOG_LIMIT ? allLogs.slice(-LOG_LIMIT) : allLogs;
  const truncated = allLogs.length > LOG_LIMIT;

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

  // Detect user scroll to pause auto-scroll.
  useEffect(() => {
    const container = logContainerRef.current;
    if (!container) return;
    const handler = () => {
      const { scrollTop, scrollHeight, clientHeight } = container;
      const atBottom = scrollHeight - scrollTop - clientHeight < 40;
      setAutoScroll(atBottom);
    };
    container.addEventListener("scroll", handler);
    return () => container.removeEventListener("scroll", handler);
  }, []);

  // Auto-scroll log viewer.
  useEffect(() => {
    if (autoScroll) {
      logEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [logs, autoScroll]);

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
  const handleRecover = () => doAction("task_recover", { task_id: taskId });
  const handleDelete = async () => {
    setShowDelete(false);
    await doAction("task_delete", { task_id: taskId, force: true });
    onBack();
  };

  const [traceJson, setTraceJson] = useState<string | null>(null);
  const handleTrace = async () => {
    try {
      const r = await invoke<{ trace_json: string }>("task_trace", { task_id: taskId, verbose: true });
      setTraceJson(r.trace_json);
    } catch (e) { setActionErr(typeof e === "string" ? e : String(e)); }
  };

  const status = displayData?.status.toLowerCase() ?? "";
  const isRunning = status === "running" || status === "in_progress";
  const isPaused = status === "paused";
  const isFailed = status === "failed" || status === "error";

  // Highlight matching text in a log line.
  const highlightLine = (text: string) => {
    if (!searchQuery) return text;
    const idx = text.toLowerCase().indexOf(searchQuery.toLowerCase());
    if (idx === -1) return text;
    return (
      <>
        {text.slice(0, idx)}
        <mark style={{ background: "var(--warning)", color: "#000", borderRadius: 2, padding: "0 2px" }}>
          {text.slice(idx, idx + searchQuery.length)}
        </mark>
        {text.slice(idx + searchQuery.length)}
      </>
    );
  };

  const filteredLogs = searchQuery
    ? logs.filter((l) => l.line.toLowerCase().includes(searchQuery.toLowerCase()))
    : logs;

  return (
    <div>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
        <button className="btn btn-ghost" onClick={onBack} aria-label={i18n.taskDetail.backLabel}>
          {i18n.common.backToList}
        </button>
        <span style={{ flex: 1 }} />

        {/* Action buttons */}
        {canAccess("operator") && (
          <>
            {isRunning && (
              <button className="btn btn-secondary" onClick={handlePause} aria-label={i18n.taskDetail.pauseLabel}>
                {i18n.taskDetail.pause}
              </button>
            )}
            {isPaused && (
              <button className="btn btn-secondary" onClick={handleResume} aria-label={i18n.taskDetail.resumeLabel}>
                {i18n.taskDetail.resume}
              </button>
            )}
            {isFailed && displayData?.items.some((i) => i.status.toLowerCase() === "failed") && (
              <button
                className="btn btn-secondary"
                onClick={() => {
                  const failedItem = displayData.items.find((i) => i.status.toLowerCase() === "failed");
                  if (failedItem) doAction("task_retry", { task_item_id: failedItem.id });
                }}
                aria-label={i18n.taskDetail.retryLabel}
              >
                {i18n.taskDetail.retry}
              </button>
            )}
            {isFailed && (
              <button className="btn btn-secondary" onClick={handleRecover} aria-label={i18n.taskDetail.recoverLabel}>
                {i18n.taskDetail.recover}
              </button>
            )}
          </>
        )}

        <button className="btn btn-ghost" onClick={handleTrace} aria-label={i18n.taskDetail.traceLabel} style={{ fontSize: 13 }}>
          {i18n.taskDetail.trace}
        </button>

        <button
          className={`btn ${expert ? "btn-primary" : "btn-ghost"}`}
          onClick={() => setExpert((v) => !v)}
          aria-label={i18n.taskDetail.expertToggle}
        >
          {expert ? i18n.taskDetail.expertOn : i18n.taskDetail.expertOff}
        </button>

        {canAccess("admin") && (
          <button
            className="btn btn-destructive"
            onClick={() => setShowDelete(true)}
            aria-label={i18n.taskDetail.deleteLabel}
          >
            {i18n.common.delete}
          </button>
        )}
      </div>

      {/* Status messages */}
      {actionMsg && <p style={{ color: "var(--success)", fontSize: 13, marginBottom: 8 }}>{actionMsg}</p>}
      {actionErr && <p style={{ color: "var(--danger)", fontSize: 13, marginBottom: 8 }}>{actionErr}</p>}
      {error && <p style={{ color: "var(--danger)", marginBottom: 8 }}>{error}</p>}

      {traceJson && (
        <div className="liquid-glass" style={{ marginBottom: 16 }}>
          <div style={{ display: "flex", alignItems: "center", marginBottom: 8 }}>
            <h3 style={{ flex: 1, fontSize: 14 }}>{i18n.taskDetail.traceTitle}</h3>
            <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={() => setTraceJson(null)}>{i18n.common.close}</button>
          </div>
          <pre style={{ background: "var(--bg-secondary)", borderRadius: 12, padding: 12,
            fontSize: 12, whiteSpace: "pre-wrap", maxHeight: 300, overflowY: "auto" }}>
            {traceJson}
          </pre>
        </div>
      )}

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
                  {i18n.taskDetail.stepProgress}
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
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
                <h3 style={{ flex: 1 }}>{i18n.taskDetail.liveLog}</h3>
                {truncated && (
                  <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
                    {i18n.taskDetail.logLimitHint(LOG_LIMIT)}
                  </span>
                )}
                {!active ? (
                  <button className="btn btn-primary" onClick={start} aria-label={i18n.taskDetail.followLabel}>
                    {i18n.taskDetail.follow}
                  </button>
                ) : (
                  <button className="btn btn-secondary" onClick={stop} aria-label={i18n.taskDetail.stopFollowLabel}>
                    {i18n.taskDetail.stopFollow}
                  </button>
                )}
              </div>

              {/* Search bar */}
              <div style={{ marginBottom: 8 }}>
                <input
                  type="text"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder={i18n.taskDetail.searchPlaceholder}
                  style={{
                    width: "100%",
                    padding: "6px 10px",
                    borderRadius: 8,
                    border: "1px solid var(--glass-border-subtle)",
                    background: "var(--bg-secondary)",
                    color: "var(--text-primary)",
                    fontSize: 13,
                    outline: "none",
                  }}
                />
              </div>

              <div
                ref={logContainerRef}
                role="log"
                aria-label={i18n.taskDetail.logLabel}
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
                {filteredLogs.length === 0 && (
                  <p style={{ color: "var(--text-tertiary)" }}>
                    {active ? i18n.taskDetail.logWaiting : i18n.taskDetail.logHint}
                  </p>
                )}
                {filteredLogs.map((log, i) => (
                  <div key={i}>
                    <span style={{ color: "var(--text-tertiary)", marginRight: 8 }}>
                      {log.timestamp}
                    </span>
                    <span>{highlightLine(log.line)}</span>
                  </div>
                ))}
                <div ref={logEndRef} />
              </div>

              {/* Scroll-to-bottom button when auto-scroll is paused */}
              {!autoScroll && logs.length > 0 && (
                <div style={{ textAlign: "center", marginTop: 4 }}>
                  <button
                    className="btn btn-ghost"
                    style={{ fontSize: 12 }}
                    onClick={() => {
                      setAutoScroll(true);
                      logEndRef.current?.scrollIntoView({ behavior: "smooth" });
                    }}
                  >
                    {i18n.taskDetail.scrollToBottom}
                  </button>
                </div>
              )}
            </div>
          )}
        </>
      )}

      <ConfirmDialog
        open={showDelete}
        title={i18n.taskDetail.deleteTitle}
        message={i18n.taskDetail.deleteMessage}
        confirmLabel={i18n.taskDetail.deleteConfirm}
        destructive
        onConfirm={handleDelete}
        onCancel={() => setShowDelete(false)}
      />
    </div>
  );
}
