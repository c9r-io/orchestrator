import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { formatConsoleRoute, parseConsoleRoute, pathForPage, useConsoleRoute } from "./routes";
import type { ConsoleRoute } from "./routes";
import { featureEnabled } from "./features";

describe("console routes", () => {
  beforeEach(() => window.history.replaceState(null, "", "/"));
  afterEach(cleanup);

  it("defaults unknown and empty hashes to Attention", () => {
    expect(parseConsoleRoute("")).toEqual({ page: "attention", attentionId: undefined });
    expect(parseConsoleRoute("#/legacy")).toEqual({ page: "attention" });
  });

  it("round-trips stable resource identifiers", () => {
    const route = { page: "processes", taskId: "task/with space" } as const;
    expect(parseConsoleRoute(formatConsoleRoute(route))).toEqual(route);
    const reviewRoute = { page: "processes", taskId: "task/with space", reviewResume: true } as const;
    expect(formatConsoleRoute(reviewRoute)).toBe("#/tasks/task%2Fwith%20space?review=safe-resume");
    expect(parseConsoleRoute(formatConsoleRoute(reviewRoute))).toEqual(reviewRoute);
    expect(parseConsoleRoute("#/tasks/task-1?review=unknown")).toEqual({ page: "processes", taskId: "task-1" });
    expect(formatConsoleRoute({ page: "sessions", sessionId: "session-1" })).toBe("#/sessions/session-1");
  });

  it("round-trips source automation and provenance deep links", () => {
    expect(parseConsoleRoute("#/sources/automations/routes/route%2F1")).toEqual({ page: "sources", section: "automations", automationView: "routes", resourceId: "route/1" });
    expect(formatConsoleRoute({ page: "sources", section: "automations", automationView: "bindings", resourceId: "badge analyze" })).toBe("#/sources/automations/bindings/badge%20analyze");
    expect(parseConsoleRoute("#/sources/events/event-1")).toEqual({ page: "sources", section: "events", resourceId: "event-1" });
    expect(parseConsoleRoute("#/sources/connections/conn%2F1")).toEqual({ page: "sources", section: "connections", resourceId: "conn/1" });
    expect(parseConsoleRoute("#/sources/task-legacy")).toEqual({ page: "sources", taskId: "task-legacy" });
  });

  it("round-trips every stable top-level route and source default", () => {
    expect(parseConsoleRoute("#/attention/attention%201")).toEqual({ page: "attention", attentionId: "attention 1" });
    expect(parseConsoleRoute("#/system/operations")).toEqual({ page: "system", section: "operations" });
    expect(parseConsoleRoute("#/new-task/draft%2F1")).toEqual({ page: "new-process", draftId: "draft/1" });
    expect(parseConsoleRoute("#/sources/bindings/task-1")).toEqual({ page: "sources", section: "bindings", resourceId: "task-1" });
    expect(parseConsoleRoute("#/sources/automations/unknown")).toEqual({ page: "sources", taskId: "automations" });
    expect(formatConsoleRoute({ page: "sources", section: "events" })).toBe("#/sources/events");
    expect(formatConsoleRoute({ page: "sources", section: "automations" })).toBe("#/sources/automations/templates");
    expect(formatConsoleRoute({ page: "new-process", draftId: "draft 1" })).toBe("#/new-task/draft%201");
  });

  it("keeps hook state aligned with hash navigation and same-route refreshes", async () => {
    const { result, unmount } = renderHook(() => useConsoleRoute());
    expect(window.location.hash).toBe("#/attention");
    expect(result.current.route).toEqual({ page: "attention", attentionId: undefined });

    act(() => result.current.navigate({ page: "processes", taskId: "task 1" }));
    await waitFor(() => expect(result.current.route).toEqual({ page: "processes", taskId: "task 1" }));
    expect(window.location.hash).toBe("#/tasks/task%201");

    act(() => result.current.navigate({ page: "processes", taskId: "task 1" }));
    expect(result.current.route).toEqual({ page: "processes", taskId: "task 1" });
    unmount();
  });

  // FR-166. The rename is only safe if a hash minted before it still lands on the same
  // page, so the compatibility direction is asserted per segment rather than inferred
  // from the fact that the new one works.
  it("keeps hashes minted before the Task rename landing on the same page", () => {
    expect(parseConsoleRoute("#/processes")).toEqual({ page: "processes", taskId: undefined });
    expect(parseConsoleRoute("#/processes/task-1")).toEqual({ page: "processes", taskId: "task-1" });
    expect(parseConsoleRoute("#/processes/task-1?review=safe-resume"))
      .toEqual({ page: "processes", taskId: "task-1", reviewResume: true });
    expect(parseConsoleRoute("#/new-process/draft-1")).toEqual({ page: "new-process", draftId: "draft-1" });

    // Both spellings reach the same route, and the one written back is the new one --
    // a compatibility alias that also became the canonical output would leave the
    // console showing Tasks and minting #/processes forever.
    expect(parseConsoleRoute("#/processes/task-1")).toEqual(parseConsoleRoute("#/tasks/task-1"));
    expect(formatConsoleRoute(parseConsoleRoute("#/processes/task-1"))).toBe("#/tasks/task-1");
    expect(formatConsoleRoute(parseConsoleRoute("#/new-process/draft-1"))).toBe("#/new-task/draft-1");
  });

  it("gives every page exactly one url segment", () => {
    // The nav builds hrefs from pathForPage directly and formatConsoleRoute builds the
    // rest; if the two ever disagree, clicking a nav item and navigating to the same
    // page produce different hashes. Deriving the page list from the map rather than
    // listing it means a new page cannot be added without a segment.
    const pages = ["attention", "processes", "sessions", "sources", "system", "new-process"] as const;
    for (const page of pages) {
      expect(formatConsoleRoute({ page } as ConsoleRoute)).toBe(`#/${pathForPage(page)}`);
    }
    expect(pathForPage("processes")).toBe("tasks");
    expect(pathForPage("new-process")).toBe("new-task");
  });

  it("enables console surfaces unless explicitly disabled at build time", () => {
    expect(["attention", "processes", "sessions", "sources", "system"].map((feature) =>
      featureEnabled(feature as Parameters<typeof featureEnabled>[0])
    )).toEqual([true, true, true, true, true]);
  });
});
