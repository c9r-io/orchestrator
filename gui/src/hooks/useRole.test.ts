import { describe, expect, it } from "vitest";
import { hasAccess } from "./useRole";

describe("role presentation boundary", () => {
  it("keeps read-only callers below every mutation role", () => {
    expect(hasAccess("read_only", "read_only")).toBe(true);
    expect(hasAccess("read_only", "operator")).toBe(false);
    expect(hasAccess("read_only", "admin")).toBe(false);
  });

  it("allows admin to reach all presentation levels", () => {
    expect(hasAccess("admin", "read_only")).toBe(true);
    expect(hasAccess("admin", "operator")).toBe(true);
    expect(hasAccess("admin", "admin")).toBe(true);
  });
});
