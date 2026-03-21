import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useGrpc } from "../hooks/useGrpc";
import { useStream } from "../hooks/useStream";
import { useRole } from "../hooks/useRole";
import ConfirmDialog from "../components/ConfirmDialog";
import StatusIcon from "../components/StatusIcon";
import type { TaskDetail, LogLine, TaskCreateResult } from "../lib/types";

interface Props {
  taskId: string;
  onBack: () => void;
  onConfirmed: (newTaskId: string) => void;
}

export default function WishDetail({ taskId, onBack, onConfirmed }: Props) {
  const { data, error, call } = useGrpc<TaskDetail>("task_info");
  const { canAccess } = useRole();
  const [showCancel, setShowCancel] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const streamParams = useMemo(() => ({ task_id: taskId }), [taskId]);
  const { data: logs, active, start, stop } = useStream<LogLine>(
    "start_task_follow",
    "stop_task_follow",
    `task-follow-${taskId}`,
    streamParams
  );

  useEffect(() => {
    call({ task_id: taskId });
    // Auto-start log streaming for drafting wishes
    start();
    return () => { stop(); };
  }, [call, taskId, start, stop]);

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

  // Combine logs into a draft preview (log lines as the FR content)
  const draftContent = logs.map((l) => l.line).join("\n");

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
              FR 草稿预览
            </h3>
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
              }}
              role="log"
              aria-label="FR 草稿内容"
            >
              {draftContent || (
                <span style={{ color: "var(--text-tertiary)" }}>
                  {active ? "正在为您设计方案..." : "暂无草稿内容"}
                </span>
              )}
            </div>
          </div>

          {/* Actions */}
          {canAccess("operator") && (
            <div style={{ display: "flex", gap: 8 }}>
              <button
                className="btn btn-primary"
                onClick={handleConfirm}
                aria-label="确认开发"
              >
                确认开发
              </button>
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
    </div>
  );
}
