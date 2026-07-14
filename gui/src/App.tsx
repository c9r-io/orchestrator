import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isPermissionGranted, onAction, requestPermission } from "@tauri-apps/plugin-notification";
import { RoleContext, hasAccess } from "./hooks/useRole";
import { useConnectionState } from "./hooks/useConnectionState";
import { useTheme } from "./hooks/useTheme";
import { useTransparency } from "./hooks/useTransparency";
import { featureEnabled, type ConsoleFeature } from "./lib/features";
import { useConsoleRoute, type ConsoleRoute } from "./lib/routes";
import { recordUiMetric } from "./lib/telemetry";
import type { Role } from "./lib/types";
import i18n from "./lib/i18n";
import ConnectionBanner from "./components/ConnectionBanner";
import ConnectionStatus from "./pages/ConnectionStatus";
import AttentionInbox from "./pages/AttentionInbox";
import ProcessList from "./pages/ProcessList";
import ProcessWorkspace from "./pages/ProcessWorkspace";
import SessionList from "./pages/SessionList";
import SessionInspector from "./pages/SessionInspector";
import Sources from "./pages/Sources";
import System from "./pages/System";
import WishPool from "./pages/WishPool";
import WishDetail from "./pages/WishDetail";

const nav: Array<{ page: ConsoleFeature; label: string; icon: string; shortcut: string }> = [
  { page: "attention", label: "Attention", icon: "!", shortcut: "1" },
  { page: "processes", label: "Processes", icon: "◫", shortcut: "2" },
  { page: "sessions", label: "Sessions", icon: ">_", shortcut: "3" },
  { page: "sources", label: "Sources", icon: "↗", shortcut: "4" },
  { page: "system", label: "System", icon: "⚙", shortcut: "5" },
];

function routeFor(page: ConsoleFeature): ConsoleRoute { return { page } as ConsoleRoute; }

export default function App() {
  const [role, setRole] = useState<Role | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [nativeNotificationsEnabled, setNativeNotificationsEnabled] = useState(false);
  const { connectionState, reconnect } = useConnectionState();
  const { theme, toggleTheme } = useTheme();
  const { transparency, toggleTransparency } = useTransparency();
  const { route, navigate } = useConsoleRoute();
  const connected = connectionState.kind === "Connected";

  useEffect(() => {
    (async () => {
      try {
        await invoke("connect", {});
        setRole(await invoke<Role>("probe_role", {}));
      } catch { /* ConnectionStatus owns retry presentation. */ }
      try {
        const granted = await isPermissionGranted();
        setNativeNotificationsEnabled(granted || await requestPermission() === "granted");
      } catch { setNativeNotificationsEnabled(false); }
    })();
  }, []);

  useEffect(() => {
    let disposed = false;
    let unregister: (() => Promise<void>) | undefined;
    onAction((notification) => {
      const deepLink = notification.extra?.deep_link;
      if (typeof deepLink === "string" && /^#\/(attention|processes)\//.test(deepLink)) {
        window.location.hash = deepLink.slice(1);
      }
    }).then((listener) => {
      if (disposed) void listener.unregister();
      else unregister = () => listener.unregister();
    }).catch(() => undefined);
    return () => { disposed = true; if (unregister) void unregister(); };
  }, []);

  useEffect(() => {
    if (!connected || role) return;
    invoke<Role>("probe_role", {}).then(setRole).catch(() => undefined);
  }, [connected, role]);

  useEffect(() => {
    const started = performance.now();
    requestAnimationFrame(() => recordUiMetric("page_load", { page: route.page, duration_ms: Math.round(performance.now() - started) }));
  }, [route.page]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) return;
      const target = nav.find((item) => item.shortcut === event.key);
      if (target && featureEnabled(target.page)) {
        event.preventDefault(); navigate(routeFor(target.page));
      }
      if (event.key.toLowerCase() === "n") {
        event.preventDefault(); navigate({ page: "new-process" });
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [navigate]);

  const roleContext = useMemo(() => ({ role, canAccess: (required: Role) => hasAccess(role, required) }), [role]);
  const go = useCallback((next: ConsoleRoute) => { setMenuOpen(false); navigate(next); }, [navigate]);

  if (!connected && connectionState.kind !== "Reconnecting") {
    return <RoleContext.Provider value={roleContext}>
      <ConnectionBanner state={connectionState} onRetry={reconnect} />
      <div className="page"><ConnectionStatus state={connectionState} onRetry={reconnect} /></div>
    </RoleContext.Provider>;
  }

  const content = (() => {
    if (!featureEnabled(route.page === "new-process" ? "processes" : route.page)) {
      return <section className="liquid-glass"><h1>Feature unavailable</h1><p>This console page is disabled by its rollout flag.</p></section>;
    }
    switch (route.page) {
      case "attention": return <AttentionInbox initialAttentionId={route.attentionId} nativeNotificationsEnabled={nativeNotificationsEnabled} onOpenTask={(taskId) => go({ page: "processes", taskId })} />;
      case "processes": return route.taskId
        ? <ProcessWorkspace taskId={route.taskId} onBack={() => go({ page: "processes" })} />
        : <ProcessList onSelect={(taskId) => go({ page: "processes", taskId })} />;
      case "sessions": return route.sessionId
        ? <SessionInspector sessionId={route.sessionId} onBack={() => go({ page: "sessions" })} onOpenProcess={(taskId) => go({ page: "processes", taskId })} />
        : <SessionList onSelect={(sessionId) => go({ page: "sessions", sessionId })} />;
      case "sources": return route.taskId
        ? <ProcessWorkspace taskId={route.taskId} onBack={() => go({ page: "sources" })} />
        : <Sources onOpenTask={(taskId) => go({ page: "sources", taskId })} />;
      case "system": return <System initialSection={route.section} />;
      case "new-process": return route.draftId
        ? <WishDetail taskId={route.draftId} onBack={() => go({ page: "new-process" })} onConfirmed={(taskId) => go({ page: "processes", taskId })} />
        : <WishPool onSelectWish={(draftId) => go({ page: "new-process", draftId })} />;
    }
  })();

  return <RoleContext.Provider value={roleContext}>
    <ConnectionBanner state={connectionState} onRetry={reconnect} />
    <div className="console-shell">
      <button className="mobile-menu btn btn-ghost" aria-expanded={menuOpen} aria-controls="console-sidebar" onClick={() => setMenuOpen((value) => !value)}>☰ Menu</button>
      <aside id="console-sidebar" className={`console-sidebar ${menuOpen ? "console-sidebar-open" : ""}`}>
        <div className="console-brand"><span className="brand-mark">AO</span><span><strong>Orchestrator</strong><small>Process Console</small></span></div>
        <nav className="console-nav" aria-label={i18n.nav.mainNav}>
          {nav.filter((item) => featureEnabled(item.page)).map((item) => <a key={item.page} href={`#/${item.page}`} className={route.page === item.page ? "active" : ""} aria-current={route.page === item.page ? "page" : undefined} onClick={() => setMenuOpen(false)}>
            <span className="nav-icon" aria-hidden="true">{item.icon}</span><span>{item.label}</span><kbd>⌘{item.shortcut}</kbd>
          </a>)}
        </nav>
        <button className="btn btn-primary new-process" onClick={() => go({ page: "new-process" })}>＋ New process</button>
        <div className="console-preferences">
          {role && <span className="badge badge-info">{role}</span>}
          <button className="btn btn-ghost" onClick={toggleTheme} aria-label={theme === "light" ? i18n.theme.toggleDark : i18n.theme.toggleLight}>{theme === "light" ? "◐" : "◑"} Theme</button>
          <button className="btn btn-ghost" onClick={toggleTransparency} aria-pressed={transparency === "reduced"}>{transparency === "reduced" ? "●" : "○"} Reduce transparency</button>
        </div>
      </aside>
      <div className="console-content">{content}</div>
    </div>
  </RoleContext.Provider>;
}
