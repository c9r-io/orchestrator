import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { formatConsoleRoute, parseConsoleRoute, useConsoleRoute } from "./routes";
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
    expect(formatConsoleRoute(reviewRoute)).toBe("#/processes/task%2Fwith%20space?review=safe-resume");
    expect(parseConsoleRoute(formatConsoleRoute(reviewRoute))).toEqual(reviewRoute);
    expect(parseConsoleRoute("#/processes/task-1?review=unknown")).toEqual({ page: "processes", taskId: "task-1" });
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
    expect(parseConsoleRoute("#/new-process/draft%2F1")).toEqual({ page: "new-process", draftId: "draft/1" });
    expect(parseConsoleRoute("#/sources/bindings/task-1")).toEqual({ page: "sources", section: "bindings", resourceId: "task-1" });
    expect(parseConsoleRoute("#/sources/automations/unknown")).toEqual({ page: "sources", taskId: "automations" });
    expect(formatConsoleRoute({ page: "sources", section: "events" })).toBe("#/sources/events");
    expect(formatConsoleRoute({ page: "sources", section: "automations" })).toBe("#/sources/automations/templates");
    expect(formatConsoleRoute({ page: "new-process", draftId: "draft 1" })).toBe("#/new-process/draft%201");
  });

  it("keeps hook state aligned with hash navigation and same-route refreshes", async () => {
    const { result, unmount } = renderHook(() => useConsoleRoute());
    expect(window.location.hash).toBe("#/attention");
    expect(result.current.route).toEqual({ page: "attention", attentionId: undefined });

    act(() => result.current.navigate({ page: "processes", taskId: "task 1" }));
    await waitFor(() => expect(result.current.route).toEqual({ page: "processes", taskId: "task 1" }));
    expect(window.location.hash).toBe("#/processes/task%201");

    act(() => result.current.navigate({ page: "processes", taskId: "task 1" }));
    expect(result.current.route).toEqual({ page: "processes", taskId: "task 1" });
    unmount();
  });

  it("enables console surfaces unless explicitly disabled at build time", () => {
    expect(["attention", "processes", "sessions", "sources", "system"].map((feature) =>
      featureEnabled(feature as Parameters<typeof featureEnabled>[0])
    )).toEqual([true, true, true, true, true]);
  });
});
