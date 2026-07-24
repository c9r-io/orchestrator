import { invoke } from "@tauri-apps/api/core";

type UiMetricName =
  | "page_load"
  | "stream_reconnect"
  | "attention_mutation"
  | "attention_reconciliation";

interface UiMetricFields {
  duration_ms?: number;
  page?: string;
  project_id?: string;
  action?: string;
  result?: string;
  error_category?: string;
}

/** Durable, bounded local telemetry. Content and high-cardinality identifiers are not accepted. */
export function recordUiMetric(
  name: UiMetricName,
  fields: UiMetricFields = {},
) {
  const metricName = {
    page_load: "ui_page_load_seconds",
    stream_reconnect: "stream_reconnect_total",
    attention_mutation: "attention_mutation_total",
    attention_reconciliation: "attention_reconciliation_total",
  }[name];
  const dimensions: Record<string, string> = {};
  if (fields.page) dimensions.page = fields.page.slice(0, 64);
  if (fields.action && name.startsWith("attention_")) dimensions.action = fields.action.slice(0, 64);
  if (fields.result && name !== "page_load") dimensions.result = fields.result.slice(0, 64);
  if (fields.error_category && name === "attention_mutation") {
    dimensions.error_category = fields.error_category.slice(0, 64);
  }
  void invoke("process_metric_record", {
    project_id: fields.project_id?.slice(0, 128) || "default",
    metric_name: metricName,
    dimensions,
    value: name === "page_load" ? Math.max(0, fields.duration_ms ?? 0) / 1000 : 1,
    source_key: crypto.randomUUID(),
  }).catch(() => undefined);
}
