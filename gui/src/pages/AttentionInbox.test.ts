import { describe, expect, it } from "vitest";
import { matchesAttentionFilters, reconcileAttentionDelta } from "./AttentionInbox";
import type { AttentionItem } from "../lib/types";

const item = (overrides: Partial<AttentionItem> = {}): AttentionItem => ({
  id: "attention-1", project_id: "project-1", task_id: "task-1", task_item_id: null,
  step_id: "test", session_id: null, kind: "step_failed", severity: "intervention",
  state: "open", title: "Tests failed", summary: "Choose a safe recovery", requested_decision_json: null,
  actions: [], assignee: null, occurrence_count: 1, reopen_count: 0, version: 1,
  created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:00:00Z",
  last_occurred_at: "2026-07-14T00:00:00Z", snoozed_until: null, resolved_at: null,
  ...overrides,
});

const active = { state: "active", severity: "", assignee: "" };

describe("Attention snapshot reconciliation", () => {
  it("upserts by stable ID without duplicates after reconnect", () => {
    const changed = item({ version: 2, occurrence_count: 2 });
    const result = reconcileAttentionDelta([item()], { kind: "upsert", change_id: 2, item: changed }, active);
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({ id: "attention-1", version: 2, occurrence_count: 2 });
  });

  it("removes resolved items from the default open queue", () => {
    const resolved = item({ state: "resolved", version: 2 });
    expect(reconcileAttentionDelta([item()], { kind: "upsert", change_id: 2, item: resolved }, active)).toEqual([]);
    expect(matchesAttentionFilters(resolved, { ...active, state: "resolved" })).toBe(true);
  });

  it("does not insert deltas that violate the active severity filter", () => {
    const filters = { ...active, severity: "attention" };
    expect(reconcileAttentionDelta([], { kind: "upsert", change_id: 1, item: item() }, filters)).toEqual([]);
  });

  it("matches Mine only for the trusted current actor when an actor is available", () => {
    const mine = { ...active, assignee: "me" };
    expect(matchesAttentionFilters(item({ assignee: "spiffe://operator/alice" }), mine, "spiffe://operator/alice")).toBe(true);
    expect(matchesAttentionFilters(item({ assignee: "spiffe://operator/bob" }), mine, "spiffe://operator/alice")).toBe(false);
    expect(matchesAttentionFilters(item({ assignee: null }), mine, "spiffe://operator/alice")).toBe(false);
  });

  it("applies remove deltas even when a notification descriptor is present", () => {
    const result = reconcileAttentionDelta([item()], {
      kind: "remove",
      change_id: 3,
      item: item({ version: 2 }),
      notification: {
        title: "Tests failed",
        dedupe_key: "attention-1:2",
        attention_item_id: "attention-1",
        item_version: 2,
        severity: "intervention",
        process_id: "task-1",
        deep_link: "#/attention/attention-1",
      },
    }, active);
    expect(result).toEqual([]);
  });
});
