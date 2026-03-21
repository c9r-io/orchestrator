import { useEffect, useMemo, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useGrpc } from "../hooks/useGrpc";
import { useStream } from "../hooks/useStream";
import { useRole } from "../hooks/useRole";
import ConfirmDialog from "../components/ConfirmDialog";
import StatusIcon from "../components/StatusIcon";
import type { TaskDetail, LogLine, TaskCreateResult, TaskLogChunk } from "../lib/types";

interface Props {
  taskId: string;
  onBack: () => void;
  onConfirmed: (newTaskId: string) => void;
}

const PROGRESS_PHASES = [
  { delay: 0, text: "正在理解你的需求..." },
  { delay: 3000, text: "正在设计功能方案..." },
  { delay: 8000, text: "正在撰写 FR 文档..." },
];

export default function WishDetail({ taskId, onBack, onConfirmed }: Props) {
  const { data, error, call } = useGrpc<TaskDetail>("task_info");
  const { canAccess } = useRole();
  const [showCancel, setShowCancel] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [draftContent, setDraftContent] = useState<string>("");
  const [phaseText, setPhaseText] = useState(PROGRESS_PHASES[0].text);
  const startTimeRef = useRef(Date.now());

  const streamParams = useMemo(() => ({ task_id: taskId }), [taskId]);
  const { data: logs, active, start, stop } = useStream<LogLine>(
    "start_task_follow",
    "stop_task_follow",
    `task-follow-${taskId}`,
    streamParams
  );

  useEffect(() => {
    call({ task_id: taskId });
    start();
    return () => { stop(); };
  }, [call, taskId, start, stop]);

  // Phased progress messages while drafting.
  useEffect(() => {
    if (data?.status.toLowerCase() === "completed") return;
    const timer = setInterval(() => {
      const elapsed = Date.now() - startTimeRef.current;
      for (let i = PROGRESS_PHASES.length - 1; i >= 0; i--) {
        if (elapsed >= PROGRESS_PHASES[i].delay) {
          setPhaseText(PROGRESS_PHASES[i].text);
          break;
        }
      }
    }, 500);
    return () => clearInterval(timer);
  }, [data?.status]);

  // When task completes, load full logs as FR draft.
  useEffect(() => {
    if (data?.status.toLowerCase() !== "completed") return;
    (async () => {
      try {
        const chunks = await invoke<TaskLogChunk[]>("task_logs", { task_id: taskId });
        const content = chunks.map((c) => c.content).join("\n");
        setDraftContent(content);
      } catch {
        // Fall back to streaming logs.
        setDraftContent(logs.map((l) => l.line).join("\n"));
      }
    })();
  }, [data?.status, taskId, logs]);

  // While still streaming, show live logs.
  const displayContent =
    data?.status.toLowerCase() === "completed"
      ? draftContent
      : logs.map((l) => l.line).join("\n");

  const isCompleted = data?.status.toLowerCase() === "completed" || data?.status.toLowerCase() === "succeeded";
  const isDrafting = !isCompleted && !data?.status.toLowerCase().includes("fail");

  const handleConfirm = async () => {
    if (!data) return;
    setActionError(null);
    try {
      const result = await invoke<TaskCreateResult>("task_create", {
        goal: data.goal,
        name: data.name,
      });
      onConfirmed(result.task_id);
    } catch (e) {
      setActionError(typeof e === "string" ? e : String(e));
    }
  };

  const handleCancel = async () => {
    setShowCancel(false);
    try {
      await invoke("task_delete", { task_id: taskId, force: true });
      onBack();
    } catch (e) {
      setActionError(typeof e === "string" ? e : String(e));
    }
  };

  return (
    <div>
      <button className="btn btn-ghost" onClick={onBack} style={{ marginBottom: 12 }}>
        &larr; 返回许愿池
      </button>

      {error && <p style={{ color: "var(--danger)" }}>{error}</p>}
      {actionError && <p style={{ color: "var(--danger)" }}>{actionError}</p>}

      {data && (
        <>
          {/* Original wish */}
          <div className="liquid-glass" style={{ marginBottom: 16 }}>
            <h3 style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 4 }}>
              原始需求
            </h3>
            <p>{data.goal || "(无描述)"}</p>
            <div style={{ marginTop: 8 }}>
              <StatusIcon status={data.status} />
            </div>
          </div>

          {/* FR Draft preview */}
          <div className="liquid-glass" style={{ marginBottom: 16 }}>
            <h3 style={{ color: "var(--text-secondary)", fontSize: 13, marginBottom: 8 }}>
              {isCompleted ? "FR 草稿预览" : phaseText}
            </h3>

            {isDrafting && !displayContent && (
              <div style={{ textAlign: "center", padding: 24 }}>
                <div
                  style={{
                    display: "inline-block",
                    width: 24,
                    height: 24,
                    border: "3px solid var(--glass-border-subtle)",
                    borderTopColor: "var(--accent)",
                    borderRadius: "50%",
                    animation: "spin 1s linear infinite",
                  }}
                />
                <p style={{ color: "var(--text-tertiary)", marginTop: 8 }}>
                  {phaseText}
                </p>
              </div>
            )}

            {displayContent && (
              <div
                style={{
                  background: "var(--bg-secondary)",
                  borderRadius: 12,
                  padding: 16,
                  minHeight: 120,
                  fontFamily: "monospace",
                  fontSize: 14,
                  lineHeight: 1.6,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                  maxHeight: 500,
                  overflowY: "auto",
                }}
                role="log"
                aria-label="FR 草稿内容"
              >
                {displayContent}
              </div>
            )}
          </div>

          {/* Actions */}
          {canAccess("operator") && (
            <div style={{ display: "flex", gap: 8 }}>
              {isCompleted && (
                <button
                  className="btn btn-primary"
                  onClick={handleConfirm}
                  aria-label="确认开发"
                >
                  确认开发
                </button>
              )}
              <button
                className="btn btn-secondary"
                onClick={onBack}
                aria-label="修改需求"
              >
                修改需求
              </button>
              <button
                className="btn btn-ghost"
                style={{ color: "var(--danger)" }}
                onClick={() => setShowCancel(true)}
                aria-label="取消许愿"
              >
                取消
              </button>
            </div>
          )}
        </>
      )}

      <ConfirmDialog
        open={showCancel}
        title="取消许愿"
        message="确定要取消这个许愿吗？此操作不可撤销。"
        confirmLabel="确认取消"
        destructive
        onConfirm={handleCancel}
        onCancel={() => setShowCancel(false)}
      />

      {/* Spinner animation */}
      <style>{`
        @keyframes spin {
          to { transform: rotate(360deg); }
        }
      `}</style>
    </div>
  );
}
