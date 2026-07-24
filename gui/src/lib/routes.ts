import { useCallback, useEffect, useState } from "react";

export type ConsoleRoute =
  | { page: "attention"; attentionId?: string }
  | { page: "processes"; taskId?: string; reviewResume?: boolean }
  | { page: "sessions"; sessionId?: string }
  | { page: "sources"; taskId?: string; section?: "connections" | "events" | "bindings" | "automations"; automationView?: "templates" | "bindings" | "routes"; resourceId?: string }
  | { page: "system"; section?: string }
  | { page: "new-process"; draftId?: string };

const decode = (value: string | undefined) => value ? decodeURIComponent(value) : undefined;

export function parseConsoleRoute(hash: string): ConsoleRoute {
  const normalized = hash.replace(/^#/, "").replace(/^\//, "");
  const [path, search = ""] = normalized.split("?", 2);
  const params = new URLSearchParams(search);
  const [page = "attention", id, child, resourceId] = path.split("/");
  switch (page) {
    case "processes": {
      const taskId = decode(id);
      return params.get("review") === "safe-resume"
        ? { page, taskId, reviewResume: true }
        : { page, taskId };
    }
    case "sessions": return { page, sessionId: decode(id) };
    case "sources": {
      if (id === "connections") return { page, section: "connections", resourceId: decode(child) };
      if (id === "events") return { page, section: "events", resourceId: decode(child) };
      if (id === "bindings") return { page, section: "bindings", resourceId: decode(child) };
      if (id === "automations" && ["templates", "bindings", "routes"].includes(child)) {
        return { page, section: "automations", automationView: child as "templates" | "bindings" | "routes", resourceId: decode(resourceId) };
      }
      return { page, taskId: decode(id) };
    }
    case "system": return { page, section: decode(id) };
    case "new-process": return { page, draftId: decode(id) };
    case "attention": return { page, attentionId: decode(id) };
    default: return { page: "attention" };
  }
}

export function formatConsoleRoute(route: ConsoleRoute): string {
  if (route.page === "sources" && route.section) {
    const base = route.section === "automations"
      ? `#/sources/automations/${route.automationView ?? "templates"}`
      : `#/sources/${route.section}`;
    return `${base}${route.resourceId ? `/${encodeURIComponent(route.resourceId)}` : ""}`;
  }
  const id = route.page === "attention" ? route.attentionId
    : route.page === "processes" ? route.taskId
    : route.page === "sessions" ? route.sessionId
    : route.page === "sources" ? route.taskId
    : route.page === "system" ? route.section
    : route.draftId;
  const path = `#/${route.page}${id ? `/${encodeURIComponent(id)}` : ""}`;
  return route.page === "processes" && route.reviewResume ? `${path}?review=safe-resume` : path;
}

export function useConsoleRoute() {
  const [route, setRoute] = useState(() => parseConsoleRoute(window.location.hash));

  useEffect(() => {
    if (!window.location.hash) window.history.replaceState(null, "", "#/attention");
    const onChange = () => setRoute(parseConsoleRoute(window.location.hash));
    window.addEventListener("hashchange", onChange);
    onChange();
    return () => window.removeEventListener("hashchange", onChange);
  }, []);

  const navigate = useCallback((next: ConsoleRoute) => {
    const hash = formatConsoleRoute(next);
    if (window.location.hash === hash) setRoute(next);
    else window.location.hash = hash.slice(1);
  }, []);

  return { route, navigate };
}
