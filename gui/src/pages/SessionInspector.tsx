import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import SessionPanel from "../components/SessionPanel";
import type { AgentSession } from "../lib/types";

export default function SessionInspector({ sessionId, onBack, onOpenProcess }: { sessionId: string; onBack: () => void; onOpenProcess: (id: string) => void }) {
  const { canAccess } = useRole();
  const [taskId, setTaskId] = useState<string | null>(null);
  useEffect(() => {
    invoke<AgentSession[]>("agent_session_list", { task_id: null })
      .then((sessions) => setTaskId(sessions.find((session) => session.session_id === sessionId)?.task_id ?? null))
      .catch(() => undefined);
  }, [sessionId]);
  return <main aria-labelledby="session-inspector-title">
    <header className="page-heading"><div><button className="btn btn-ghost" onClick={onBack}>← Sessions</button><h1 id="session-inspector-title" className="page-title">Session inspector</h1><p>Transcript, attachment state and guarded writer control.</p></div></header>
    <SessionPanel sessionId={sessionId} canControl={canAccess("operator")} onSelectSession={() => undefined} />
    {taskId && <button className="btn btn-ghost" onClick={() => onOpenProcess(taskId)}>Open linked process</button>}
  </main>;
}
