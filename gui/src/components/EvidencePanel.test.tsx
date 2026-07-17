import { render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import EvidencePanel from "./EvidencePanel";
import type { TimelineEntry } from "../lib/types";
import { RoleContext, hasAccess } from "../hooks/useRole";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

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

  it("fetches a protected Slack link only for an operator source entry", async () => {
    vi.mocked(invoke).mockResolvedValue({
      id: "route-1", source_event_id: "source-1", reaction: "agent_fix",
      binding_name: "fix", template_name: "fix-template", status: "completed",
      task_id: "task-1", permalink: "https://acme.slack.com/archives/C1/p1",
    });
    const entry: TimelineEntry = {
      id: "entry-source", task_id: "task-1", occurred_at: "2026-07-14T00:00:00Z", category: "source",
      title: "Source automation routed", summary: "badge=agent_fix", status: "routed",
      actor: null, step_id: null, task_item_id: null, command_run_id: null, session_id: null,
      checkpoint_id: null, source_event_id: "source-1", raw_event_ids: [101], projection_version: 1,
      evidence: [],
    };
    render(<RoleContext.Provider value={{ role: "operator", canAccess: (role) => hasAccess("operator", role) }}>
      <EvidencePanel entry={entry} />
    </RoleContext.Provider>);
    const link = await screen.findByRole("link", { name: "Open Slack message" });
    expect(link).toHaveAttribute("href", "https://acme.slack.com/archives/C1/p1");
    expect(invoke).toHaveBeenCalledWith("source_automation_route_get", { source_event_id: "source-1" });
  });
});
