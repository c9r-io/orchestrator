import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RoleContext, hasAccess } from "./hooks/useRole";
import type { Role } from "./lib/types";
import ConnectionStatus from "./pages/ConnectionStatus";
import TaskList from "./pages/TaskList";
import TaskDetail from "./pages/TaskDetail";

type Tab = "status" | "tasks";

export default function App() {
  const [tab, setTab] = useState<Tab>("status");
  const [role, setRole] = useState<Role | null>(null);
  const [connected, setConnected] = useState(false);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);

  // Auto-connect on mount.
  useEffect(() => {
    (async () => {
      try {
        await invoke("connect", {});
        setConnected(true);
        const r = await invoke<string>("probe_role", {});
        setRole(r as Role);
      } catch {
        // Connection failed — will show on status page.
      }
    })();
  }, []);

  const roleCtx = useMemo(
    () => ({
      role,
      canAccess: (required: Role) => hasAccess(role, required),
    }),
    [role]
  );

  return (
    <RoleContext.Provider value={roleCtx}>
      <div className="page">
        <nav style={{ display: "flex", gap: 8, marginBottom: 20 }}>
          <button
            className={`btn ${tab === "status" ? "btn-primary" : "btn-ghost"}`}
            onClick={() => { setTab("status"); setSelectedTaskId(null); }}
          >
            Status
          </button>
          <button
            className={`btn ${tab === "tasks" ? "btn-primary" : "btn-ghost"}`}
            onClick={() => { setTab("tasks"); setSelectedTaskId(null); }}
          >
            Tasks
          </button>
          {role && (
            <span
              className="badge badge-info"
              style={{ marginLeft: "auto", alignSelf: "center" }}
            >
              {role}
            </span>
          )}
        </nav>

        {tab === "status" && (
          <ConnectionStatus connected={connected} />
        )}

        {tab === "tasks" && !selectedTaskId && (
          <TaskList onSelect={(id) => setSelectedTaskId(id)} />
        )}

        {tab === "tasks" && selectedTaskId && (
          <TaskDetail
            taskId={selectedTaskId}
            onBack={() => setSelectedTaskId(null)}
          />
        )}
      </div>
    </RoleContext.Provider>
  );
}
