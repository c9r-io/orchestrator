import { useEffect, useState, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RoleContext, hasAccess } from "./hooks/useRole";
import type { Role } from "./lib/types";
import ConnectionStatus from "./pages/ConnectionStatus";
import WishPool from "./pages/WishPool";
import WishDetail from "./pages/WishDetail";
import ProgressList from "./pages/ProgressList";
import TaskDetail from "./pages/TaskDetail";

type Tab = "wishes" | "progress";

export default function App() {
  const [tab, setTab] = useState<Tab>("wishes");
  const [role, setRole] = useState<Role | null>(null);
  const [connected, setConnected] = useState(false);
  const [selectedWishId, setSelectedWishId] = useState<string | null>(null);
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

  // Keyboard shortcuts.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey) {
        if (e.key === "1") {
          e.preventDefault();
          setTab("wishes");
          setSelectedWishId(null);
          setSelectedTaskId(null);
        } else if (e.key === "2") {
          e.preventDefault();
          setTab("progress");
          setSelectedWishId(null);
          setSelectedTaskId(null);
        }
      }
      if (e.key === "Escape") {
        if (selectedWishId) setSelectedWishId(null);
        else if (selectedTaskId) setSelectedTaskId(null);
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [selectedWishId, selectedTaskId]);

  const roleCtx = useMemo(
    () => ({
      role,
      canAccess: (required: Role) => hasAccess(role, required),
    }),
    [role]
  );

  // When a wish is confirmed, navigate to the progress tab with the new task.
  const handleWishConfirmed = useCallback((newTaskId: string) => {
    setTab("progress");
    setSelectedWishId(null);
    setSelectedTaskId(newTaskId);
  }, []);

  // Show connection status if not connected.
  if (!connected) {
    return (
      <RoleContext.Provider value={roleCtx}>
        <div className="page">
          <ConnectionStatus connected={false} />
        </div>
      </RoleContext.Provider>
    );
  }

  return (
    <RoleContext.Provider value={roleCtx}>
      <div className="page">
        {/* Navigation */}
        <nav
          style={{ display: "flex", gap: 8, marginBottom: 20, alignItems: "center" }}
          aria-label="主导航"
        >
          <button
            className={`btn ${tab === "wishes" ? "btn-primary" : "btn-ghost"}`}
            onClick={() => {
              setTab("wishes");
              setSelectedWishId(null);
              setSelectedTaskId(null);
            }}
            aria-label="许愿池 (Cmd+1)"
          >
            许愿池
          </button>
          <button
            className={`btn ${tab === "progress" ? "btn-primary" : "btn-ghost"}`}
            onClick={() => {
              setTab("progress");
              setSelectedWishId(null);
              setSelectedTaskId(null);
            }}
            aria-label="进度观察 (Cmd+2)"
          >
            进度观察
          </button>

          {role && (
            <span
              className="badge badge-info"
              style={{ marginLeft: "auto" }}
              aria-label={`当前角色: ${role}`}
            >
              {role}
            </span>
          )}
        </nav>

        {/* Wish Pool tab */}
        {tab === "wishes" && !selectedWishId && (
          <WishPool onSelectWish={(id) => setSelectedWishId(id)} />
        )}
        {tab === "wishes" && selectedWishId && (
          <WishDetail
            taskId={selectedWishId}
            onBack={() => setSelectedWishId(null)}
            onConfirmed={handleWishConfirmed}
          />
        )}

        {/* Progress Observer tab */}
        {tab === "progress" && !selectedTaskId && (
          <ProgressList onSelect={(id) => setSelectedTaskId(id)} />
        )}
        {tab === "progress" && selectedTaskId && (
          <TaskDetail
            taskId={selectedTaskId}
            onBack={() => setSelectedTaskId(null)}
          />
        )}
      </div>
    </RoleContext.Provider>
  );
}
