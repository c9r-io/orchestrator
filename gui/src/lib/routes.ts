import { useCallback, useEffect, useState } from "react";

export type ConsoleRoute =
  | { page: "attention"; attentionId?: string }
  | { page: "processes"; taskId?: string; reviewResume?: boolean }
  | { page: "sessions"; sessionId?: string }
  | { page: "sources"; taskId?: string; section?: "connections" | "events" | "bindings" | "automations"; automationView?: "templates" | "bindings" | "routes"; resourceId?: string }
  | { page: "system"; section?: string }
  | { page: "new-process"; draftId?: string };

// FR-166: the console shows Tasks, and the hash says so. The route discriminant stays
// `processes`/`new-process` because it is an internal identifier, not a label, so the
// page components are untouched; only the path a user sees or bookmarks changes.
// `pathForPage` is the single place a page becomes a URL segment — App.tsx builds nav
// hrefs directly rather than through formatConsoleRoute, so both call it.
const PAGE_PATH: Record<ConsoleRoute["page"], string> = {
  attention: "attention",
  processes: "tasks",
  sessions: "sessions",
  sources: "sources",
  system: "system",
  "new-process": "new-task",
};

export function pathForPage(page: ConsoleRoute["page"]): string {
  return PAGE_PATH[page];
}

const decode = (value: string | undefined) => value ? decodeURIComponent(value) : undefined;

export function parseConsoleRoute(hash: string): ConsoleRoute {
  const normalized = hash.replace(/^#/, "").replace(/^\//, "");
  const [path, search = ""] = normalized.split("?", 2);
  const params = new URLSearchParams(search);
  const [rawPage = "attention", id, child, resourceId] = path.split("/");
  // Hashes minted before FR-166 still resolve: a bookmark or a handoff note written
  // against #/processes must keep landing on the same page.
  const page = rawPage === "tasks" ? "processes" : rawPage === "new-task" ? "new-process" : rawPage;
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
      ? `#/${pathForPage("sources")}/automations/${route.automationView ?? "templates"}`
      : `#/${pathForPage("sources")}/${route.section}`;
    return `${base}${route.resourceId ? `/${encodeURIComponent(route.resourceId)}` : ""}`;
  }
  const id = route.page === "attention" ? route.attentionId
    : route.page === "processes" ? route.taskId
    : route.page === "sessions" ? route.sessionId
    : route.page === "sources" ? route.taskId
    : route.page === "system" ? route.section
    : route.draftId;
  const path = `#/${pathForPage(route.page)}${id ? `/${encodeURIComponent(id)}` : ""}`;
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
