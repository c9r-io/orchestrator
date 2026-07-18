import { describe, expect, it } from "vitest";
import { formatConsoleRoute, parseConsoleRoute } from "./routes";

describe("console routes", () => {
  it("defaults unknown and empty hashes to Attention", () => {
    expect(parseConsoleRoute("")).toEqual({ page: "attention", attentionId: undefined });
    expect(parseConsoleRoute("#/legacy")).toEqual({ page: "attention" });
  });

  it("round-trips stable resource identifiers", () => {
    const route = { page: "processes", taskId: "task/with space" } as const;
    expect(parseConsoleRoute(formatConsoleRoute(route))).toEqual(route);
    expect(formatConsoleRoute({ page: "sessions", sessionId: "session-1" })).toBe("#/sessions/session-1");
  });

  it("round-trips source automation and provenance deep links", () => {
    expect(parseConsoleRoute("#/sources/automations/routes/route%2F1")).toEqual({ page: "sources", section: "automations", automationView: "routes", resourceId: "route/1" });
    expect(formatConsoleRoute({ page: "sources", section: "automations", automationView: "bindings", resourceId: "badge analyze" })).toBe("#/sources/automations/bindings/badge%20analyze");
    expect(parseConsoleRoute("#/sources/events/event-1")).toEqual({ page: "sources", section: "events", resourceId: "event-1" });
    expect(parseConsoleRoute("#/sources/task-legacy")).toEqual({ page: "sources", taskId: "task-legacy" });
  });
});
