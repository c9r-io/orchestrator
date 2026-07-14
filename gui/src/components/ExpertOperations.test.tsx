import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ExpertOperations from "./ExpertOperations";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const emptySnapshot = {
  schema_version: 1, project_id: "default", window_start: "2026-07-13T00:00:00Z",
  window_end: "2026-07-14T00:00:00Z", generated_at: new Date().toISOString(),
  coverage_start: null, partial: false, collection_enabled: true, projector_health: [],
  metrics: [
    { name: "process_autonomous_completion_ratio", labels: {}, sample_count: 0, sum: 0, min: null, max: null, value: 0, numerator: 0, denominator: 0, histogram: {}, buckets: [] },
  ],
};

describe("ExpertOperations", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());
  afterEach(cleanup);

  it("renders an explicit empty and freshness state", async () => {
    vi.mocked(invoke).mockResolvedValue(emptySnapshot);
    render(<ExpertOperations />);
    expect(await screen.findByText("No process activity in this window")).toBeVisible();
    expect(screen.getByText("Fresh snapshot")).toBeVisible();
    expect(screen.getByText("No projector health samples yet.")).toBeVisible();
  });

  it("renders a retryable error without stale data", async () => {
    vi.mocked(invoke).mockResolvedValue(emptySnapshot);
    render(<ExpertOperations />);
    await screen.findByText("No process activity in this window");
    fireEvent.change(screen.getByLabelText("Metrics project"), { target: { value: "" } });
    expect(await screen.findByRole("alert")).toHaveTextContent("Project is required");
    expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
  });
});
