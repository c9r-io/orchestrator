import { invoke } from "@tauri-apps/api/core";

type UiMetricName =
  | "page_load"
  | "stream_reconnect";

/** Durable, bounded local telemetry. Content and high-cardinality identifiers are not accepted. */
export function recordUiMetric(
  name: UiMetricName,
  fields: { duration_ms?: number; page?: string; result?: string } = {},
) {
  const metricName = name === "page_load" ? "ui_page_load_seconds" : "stream_reconnect_total";
  const dimensions: Record<string, string> = {};
  if (fields.page) dimensions.page = fields.page.slice(0, 64);
  if (fields.result && name === "stream_reconnect") dimensions.result = fields.result.slice(0, 64);
  void invoke("process_metric_record", {
    project_id: "default",
    metric_name: metricName,
    dimensions,
    value: name === "page_load" ? Math.max(0, fields.duration_ms ?? 0) / 1000 : 1,
    source_key: crypto.randomUUID(),
  }).catch(() => undefined);
}
