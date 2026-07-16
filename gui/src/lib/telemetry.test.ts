import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { recordUiMetric } from "./telemetry";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("recordUiMetric", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.spyOn(crypto, "randomUUID").mockReturnValue("00000000-0000-4000-8000-000000000003");
  });

  it("bounds page dimensions and converts page-load milliseconds to seconds", () => {
    vi.mocked(invoke).mockResolvedValue(null);
    recordUiMetric("page_load", { page: "x".repeat(100), duration_ms: -20, result: "ignored" });
    expect(invoke).toHaveBeenCalledWith("process_metric_record", {
      project_id: "default", metric_name: "ui_page_load_seconds",
      dimensions: { page: "x".repeat(64) }, value: 0,
      source_key: "00000000-0000-4000-8000-000000000003",
    });
  });

  it("records reconnect result dimensions and ignores telemetry rejection", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("metrics disabled"));
    recordUiMetric("stream_reconnect", { page: "attention", result: "error".repeat(20) });
    expect(invoke).toHaveBeenCalledWith("process_metric_record", expect.objectContaining({
      metric_name: "stream_reconnect_total", dimensions: { page: "attention", result: "error".repeat(12) + "erro" }, value: 1,
    }));
    await Promise.resolve();
  });
});
