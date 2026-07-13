import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import EvidencePanel from "./EvidencePanel";
import type { TimelineEntry } from "../lib/types";

describe("EvidencePanel", () => {
  it("renders projected evidence without requiring raw event JSON", () => {
    const entry: TimelineEntry = {
      id: "entry-1", task_id: "task-1", occurred_at: "2026-07-14T00:00:00Z", category: "failure",
      title: "Tests failed", summary: "One deterministic test failed", status: "failed",
      actor: null, step_id: "test", task_item_id: null, command_run_id: null, session_id: null,
      checkpoint_id: "checkpoint-1", source_event_id: null, raw_event_ids: [99], projection_version: 1,
      evidence: [{ kind: "test", label: "cargo test", uri: null, content_type: "text/plain", digest: null, redacted: false }],
    };
    render(<EvidencePanel entry={entry} />);
    expect(screen.getByRole("heading", { name: "Evidence" })).toBeVisible();
    expect(screen.getByText("cargo test")).toBeVisible();
    expect(screen.queryByText("99")).not.toBeInTheDocument();
  });
});
