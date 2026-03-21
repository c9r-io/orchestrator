import { useEffect, useState, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";
import { RoleContext, hasAccess } from "./hooks/useRole";
import { useConnectionState } from "./hooks/useConnectionState";
import { useTheme } from "./hooks/useTheme";
import type { Role } from "./lib/types";
import i18n from "./lib/i18n";
import ConnectionBanner from "./components/ConnectionBanner";
import ConnectionStatus from "./pages/ConnectionStatus";
import WishPool from "./pages/WishPool";
import WishDetail from "./pages/WishDetail";
import ProgressList from "./pages/ProgressList";
import TaskDetail from "./pages/TaskDetail";

type Tab = "wishes" | "progress";

export default function App() {
  const [tab, setTab] = useState<Tab>("wishes");
  const [role, setRole] = useState<Role | null>(null);
  const { connectionState, reconnect } = useConnectionState();
  const [selectedWishId, setSelectedWishId] = useState<string | null>(null);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const { theme, toggleTheme } = useTheme();

  const connected = connectionState.kind === "Connected";

  // Auto-connect on mount and request notification permission.
  useEffect(() => {
    (async () => {
      try {
        await invoke("connect", {});
        const r = await invoke<string>("probe_role", {});
        setRole(r as Role);
      } catch {
        // Connection failed — will show on wizard page.
      }

      // Request notification permission.
      try {
        const granted = await isPermissionGranted();
        if (!granted) {
          await requestPermission();
        }
      } catch {
        // Notification not available on this platform.
      }
    })();
  }, []);

  // Re-probe role when reconnected.
  useEffect(() => {
    if (connected && !role) {
      (async () => {
        try {
          const r = await invoke<string>("probe_role", {});
          setRole(r as Role);
        } catch {
          // Will retry on next reconnect.
        }
      })();
    }
  }, [connected, role]);

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

  // Show connection wizard if not connected (and not currently reconnecting from a prior connection).
  if (!connected && connectionState.kind !== "Reconnecting") {
    return (
      <RoleContext.Provider value={roleCtx}>
        <ConnectionBanner state={connectionState} onRetry={reconnect} />
        <div className="page">
          <ConnectionStatus state={connectionState} onRetry={reconnect} />
        </div>
      </RoleContext.Provider>
    );
  }

  return (
    <RoleContext.Provider value={roleCtx}>
      <ConnectionBanner state={connectionState} onRetry={reconnect} />
      <div className="page">
        {/* Navigation */}
        <nav
          style={{ display: "flex", gap: 8, marginBottom: 20, alignItems: "center" }}
          aria-label={i18n.nav.mainNav}
        >
          <button
            className={`btn ${tab === "wishes" ? "btn-primary" : "btn-ghost"}`}
            onClick={() => {
              setTab("wishes");
              setSelectedWishId(null);
              setSelectedTaskId(null);
            }}
            aria-label={i18n.nav.wishPoolShortcut}
          >
            {i18n.nav.wishPool}
          </button>
          <button
            className={`btn ${tab === "progress" ? "btn-primary" : "btn-ghost"}`}
            onClick={() => {
              setTab("progress");
              setSelectedWishId(null);
              setSelectedTaskId(null);
            }}
            aria-label={i18n.nav.progressShortcut}
          >
            {i18n.nav.progress}
          </button>

          <span style={{ flex: 1 }} />

          {role && (
            <span
              className="badge badge-info"
              aria-label={i18n.nav.currentRole(role)}
            >
              {role}
            </span>
          )}

          <button
            className="btn btn-ghost theme-toggle"
            onClick={toggleTheme}
            aria-label={theme === "light" ? i18n.theme.toggleDark : i18n.theme.toggleLight}
            title={theme === "light" ? i18n.theme.toggleDark : i18n.theme.toggleLight}
          >
            {theme === "light" ? "\u{1F319}" : "\u{2600}\u{FE0F}"}
          </button>
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
