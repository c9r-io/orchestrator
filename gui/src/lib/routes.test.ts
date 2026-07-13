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
});
