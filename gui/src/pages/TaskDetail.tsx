import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useGrpc } from "../hooks/useGrpc";
import { useStream } from "../hooks/useStream";
import { useRole } from "../hooks/useRole";
import ProgressBar from "../components/ProgressBar";
import StatusIcon from "../components/StatusIcon";
import ConfirmDialog from "../components/ConfirmDialog";
import ExpertPanel from "../components/ExpertPanel";
import ProcessTimeline from "../components/ProcessTimeline";
import EvidencePanel from "../components/EvidencePanel";
import HandoffPanel from "../components/HandoffPanel";
import SessionPanel from "../components/SessionPanel";
import SourcePanel from "../components/SourcePanel";
import i18n from "../lib/i18n";
import type { AgentSession, AttentionListResult, LogLine, TaskDetail as TaskDetailType, TimelineEntry, WatchSnapshot } from "../lib/types";

interface Props { taskId: string; onBack: () => void; }
const LOG_LIMIT = 500;

export default function TaskDetail({ taskId, onBack }: Props) {
  const { data, error, call } = useGrpc<TaskDetailType>("task_info");
  const { canAccess } = useRole();
  const [liveData, setLiveData] = useState<TaskDetailType | null>(null);
  const [expert, setExpert] = useState(false);
  const [selectedEntry, setSelectedEntry] = useState<TimelineEntry | null>(null);
  const [showDelete, setShowDelete] = useState(false);
  const [showRecover, setShowRecover] = useState(false);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [actionErr, setActionErr] = useState<string | null>(null);
  const [traceJson, setTraceJson] = useState<string | null>(null);
  const [openAttention, setOpenAttention] = useState(0);
  const [activeSessions, setActiveSessions] = useState(0);
  const [autoScroll, setAutoScroll] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");
  const logContainerRef = useRef<HTMLDivElement>(null);
  const logEndRef = useRef<HTMLDivElement>(null);

  const streamParams = useMemo(() => ({ task_id: taskId }), [taskId]);
  const { data: allLogs, active, start, stop } = useStream<LogLine>("start_task_follow", "stop_task_follow", `task-follow-${taskId}`, streamParams);
  const logs = allLogs.length > LOG_LIMIT ? allLogs.slice(-LOG_LIMIT) : allLogs;
  const reload = useCallback(() => { call({ task_id: taskId }); }, [call, taskId]);

  useEffect(() => { reload(); }, [reload]);
  useEffect(() => {
    Promise.all([
      invoke<AttentionListResult>("attention_list", { project_id: null, item_state: null, kind: null, severity: null, assignee: null, task_id: taskId }),
      invoke<AgentSession[]>("agent_session_list", { task_id: taskId }),
    ]).then(([attention, sessions]) => {
      setOpenAttention(attention.items.filter((item) => item.state !== "resolved").length);
      setActiveSessions(sessions.filter((session) => !["closed", "exited", "failed"].includes(session.state)).length);
    }).catch(() => undefined);
  }, [taskId]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    (async () => {
      unlisten = await listen<WatchSnapshot>(`task-watch-${taskId}`, ({ payload: snapshot }) => {
        if (cancelled) return;
        setLiveData((previous) => ({
          id: snapshot.task.id, name: snapshot.task.name, status: snapshot.task.status,
          goal: previous?.goal ?? snapshot.task.goal ?? "", total_items: snapshot.task.total_items,
          finished_items: snapshot.task.finished_items, failed_items: snapshot.task.failed_items,
          created_at: snapshot.task.created_at, updated_at: snapshot.task.updated_at,
          project_id: snapshot.task.project_id, workflow_id: snapshot.task.workflow_id, items: snapshot.items,
        }));
      });
      try { await invoke("start_task_watch", { task_id: taskId, interval_secs: 2 }); } catch { /* Terminal tasks need no watch. */ }
    })();
    return () => { cancelled = true; unlisten?.(); invoke("stop_task_watch", { task_id: taskId }).catch(() => undefined); };
  }, [taskId]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "e") { event.preventDefault(); setExpert((value) => !value); }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, []);

  useEffect(() => {
    const container = logContainerRef.current;
    if (!container) return;
    const handler = () => setAutoScroll(container.scrollHeight - container.scrollTop - container.clientHeight < 40);
    container.addEventListener("scroll", handler);
    return () => container.removeEventListener("scroll", handler);
  }, [expert]);
  useEffect(() => { if (autoScroll) logEndRef.current?.scrollIntoView({ behavior: "smooth" }); }, [autoScroll, logs]);

  const displayData = liveData ?? data;
  const status = displayData?.status.toLowerCase() ?? "";
  const isRunning = status === "running" || status === "in_progress";
  const isFailed = status === "failed" || status === "error";
  const isPaused = status === "paused";

  const doAction = async (command: string, params: Record<string, unknown>) => {
    setActionErr(null); setActionMsg(null);
    try { const result = await invoke<{ message: string }>(command, params); setActionMsg(result.message); reload(); }
    catch (reason) { setActionErr(String(reason)); }
  };
  const handleDelete = async () => { setShowDelete(false); await doAction("task_delete", { task_id: taskId, force: true }); onBack(); };
  const handleTrace = async () => {
    try { setTraceJson((await invoke<{ trace_json: string }>("task_trace", { task_id: taskId, verbose: true })).trace_json); }
    catch (reason) { setActionErr(String(reason)); }
  };

  const filteredLogs = searchQuery ? logs.filter((log) => log.line.toLowerCase().includes(searchQuery.toLowerCase())) : logs;

  return <main aria-labelledby="process-title">
    <header className="process-header">
      <div><button className="btn btn-ghost" onClick={onBack} aria-label={i18n.taskDetail.backLabel}>← Processes</button><h1 id="process-title" className="page-title">{displayData?.name || displayData?.id || "Process"}</h1></div>
      <div className="process-actions">
        {canAccess("operator") && isRunning && <button className="btn btn-secondary" onClick={() => void doAction("task_pause", { task_id: taskId })}>Pause</button>}
        {canAccess("operator") && isFailed && <button className="btn btn-primary" onClick={() => setShowRecover(true)}>Review recovery</button>}
        <button className={`btn ${expert ? "btn-primary" : "btn-ghost"}`} onClick={() => setExpert((value) => !value)} aria-pressed={expert}>Expert {expert ? "on" : "off"}</button>
        {canAccess("admin") && <button className="btn btn-destructive" onClick={() => setShowDelete(true)}>Delete</button>}
      </div>
    </header>
    {actionMsg && <p className="inline-success" role="status">{actionMsg}</p>}
    {(actionErr || error) && <p className="inline-error" role="alert">{actionErr || error}</p>}

    {displayData && <>
      <section className="process-overview liquid-glass" aria-label="Process overview">
        <div className="process-state"><StatusIcon status={displayData.status} /><span><strong>{displayData.status}</strong><small>{displayData.goal || "No goal summary"}</small></span></div>
        <dl className="process-facts"><div><dt>Workflow</dt><dd>{displayData.workflow_id}</dd></div><div><dt>Project</dt><dd>{displayData.project_id}</dd></div><div><dt>Open attention</dt><dd>{openAttention}</dd></div><div><dt>Active sessions</dt><dd>{activeSessions}</dd></div></dl>
        {displayData.total_items > 0 && <ProgressBar finished={displayData.finished_items} total={displayData.total_items} />}
      </section>

      {!expert ? <div className="process-workspace-grid">
        <section className="process-timeline-column"><ProcessTimeline taskId={taskId} selectedEntryId={selectedEntry?.id} onSelectEntry={setSelectedEntry} /></section>
        <aside className="process-context-rail" aria-label="Process context and controls">
          <EvidencePanel entry={selectedEntry} />
          <HandoffPanel taskId={taskId} canGenerate={canAccess("operator")} canExecute={canAccess("operator") && (isPaused || isFailed)} onExecuted={reload} />
          <SessionPanel taskId={taskId} canControl={canAccess("operator")} />
          <SourcePanel taskId={taskId} />
        </aside>
      </div> : <section aria-label="Expert process details">
        <div className="expert-utilities">
          <button className="btn btn-secondary" onClick={() => void handleTrace()}>Load trace JSON</button>
          {!active ? <button className="btn btn-primary" onClick={start}>Follow raw logs</button> : <button className="btn btn-secondary" onClick={stop}>Stop raw logs</button>}
        </div>
        {traceJson && <div className="liquid-glass expert-raw"><div><h3>Trace JSON</h3><button className="btn btn-ghost" onClick={() => setTraceJson(null)}>Close</button></div><pre>{traceJson}</pre></div>}
        <div className="liquid-glass expert-raw"><div><h3>Raw task logs</h3><span>{allLogs.length > LOG_LIMIT ? `Latest ${LOG_LIMIT}` : `${logs.length} lines`}</span></div><input value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder={i18n.taskDetail.searchPlaceholder} aria-label="Search raw logs" />
          <div ref={logContainerRef} className="raw-log" role="log" aria-live="polite">{filteredLogs.map((log, index) => <div key={`${log.timestamp}-${index}`}><time>{log.timestamp}</time><span>{log.line}</span></div>)}{filteredLogs.length === 0 && <p>{active ? "Waiting for log output…" : "Start following to receive raw logs."}</p>}<div ref={logEndRef} /></div>
          {!autoScroll && logs.length > 0 && <button className="btn btn-ghost" onClick={() => { setAutoScroll(true); logEndRef.current?.scrollIntoView({ behavior: "smooth" }); }}>Scroll to bottom</button>}
        </div>
        <ExpertPanel taskDetail={displayData} />
      </section>}
    </>}

    <ConfirmDialog open={showRecover} title="Review process recovery" message="Recovering re-enqueues execution from the daemon-approved task boundary. Workspace files are not rolled back; inspect the handoff panel for a boundary-specific resume." confirmLabel="Recover process" onConfirm={() => { setShowRecover(false); void doAction("task_recover", { task_id: taskId }); }} onCancel={() => setShowRecover(false)} />
    <ConfirmDialog open={showDelete} title={i18n.taskDetail.deleteTitle} message={i18n.taskDetail.deleteMessage} confirmLabel={i18n.taskDetail.deleteConfirm} destructive onConfirm={() => void handleDelete()} onCancel={() => setShowDelete(false)} />
  </main>;
}
