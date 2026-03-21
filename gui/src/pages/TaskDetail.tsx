import { useEffect, useRef, useMemo } from "react";
import { useGrpc } from "../hooks/useGrpc";
import { useStream } from "../hooks/useStream";
import { useRole } from "../hooks/useRole";
import type { TaskDetail as TaskDetailType, LogLine } from "../lib/types";

interface Props {
  taskId: string;
  onBack: () => void;
}

export default function TaskDetail({ taskId, onBack }: Props) {
  const { data, error, call } = useGrpc<TaskDetailType>("task_info");
  const { canAccess } = useRole();
  const logEndRef = useRef<HTMLDivElement>(null);

  const streamParams = useMemo(() => ({ task_id: taskId }), [taskId]);
  const { data: logs, active, start, stop } = useStream<LogLine>(
    "start_task_follow",
    "stop_task_follow",
    `task-follow-${taskId}`,
    streamParams
  );

  useEffect(() => {
    call({ task_id: taskId });
  }, [call, taskId]);

  // Auto-scroll log viewer.
  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  return (
    <div>
      <button className="btn btn-ghost" onClick={onBack} style={{ marginBottom: 12 }}>
        &larr; Back
      </button>

      {error && <p style={{ color: "var(--danger)" }}>{error}</p>}

      {data && (
        <div className="liquid-glass" style={{ marginBottom: 16 }}>
          <h2 style={{ marginBottom: 12 }}>{data.name || data.id}</h2>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
            <div>
              <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>Status</span>
              <p>{data.status}</p>
            </div>
            <div>
              <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>Progress</span>
              <p>{data.total_items > 0 ? `${data.finished_items}/${data.total_items}` : "-"}</p>
            </div>
            <div>
              <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>Created</span>
              <p>{data.created_at}</p>
            </div>
            <div>
              <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>Updated</span>
              <p>{data.updated_at}</p>
            </div>
          </div>
          {data.goal && (
            <div style={{ marginTop: 12 }}>
              <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>Goal</span>
              <p>{data.goal}</p>
            </div>
          )}

          {data.items.length > 0 && (
            <>
              <h3 style={{ marginTop: 16, marginBottom: 8 }}>Items</h3>
              <table style={{ width: "100%", borderCollapse: "collapse" }}>
                <thead>
                  <tr>
                    <th style={thStyle}>#</th>
                    <th style={thStyle}>File</th>
                    <th style={thStyle}>Status</th>
                  </tr>
                </thead>
                <tbody>
                  {data.items.map((item) => (
                    <tr key={item.id}>
                      <td style={tdStyle}>{item.order_no}</td>
                      <td style={tdStyle}>{item.qa_file_path}</td>
                      <td style={tdStyle}>{item.status}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </div>
      )}

      {/* Log Streaming */}
      <div className="liquid-glass">
        <div style={{ display: "flex", alignItems: "center", marginBottom: 12 }}>
          <h3 style={{ flex: 1 }}>Live Logs</h3>
          {!active ? (
            <button className="btn btn-primary" onClick={start}>
              Follow
            </button>
          ) : (
            <button className="btn btn-secondary" onClick={stop}>
              Stop
            </button>
          )}
        </div>

        <div
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
              {active ? "Waiting for log output..." : "Click Follow to stream logs."}
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

      {/* RBAC-gated actions */}
      {canAccess("operator") && (
        <div style={{ marginTop: 16, display: "flex", gap: 8 }}>
          <button className="btn btn-secondary">Pause</button>
          <button className="btn btn-secondary">Resume</button>
          {canAccess("admin") && (
            <button className="btn btn-destructive">Delete</button>
          )}
        </div>
      )}
    </div>
  );
}

const thStyle: React.CSSProperties = {
  padding: "8px 12px",
  textAlign: "left",
  color: "var(--text-secondary)",
  fontSize: 12,
};

const tdStyle: React.CSSProperties = {
  padding: "6px 12px",
  fontSize: 13,
};
