type UiMetricName =
  | "page_load"
  | "stream_reconnect"
  | "timeline_render"
  | "action_confirmed"
  | "action_cancelled"
  | "action_result";

/** Local structured telemetry. Values are identifiers/durations only; content is rejected by type. */
export function recordUiMetric(
  name: UiMetricName,
  fields: { duration_ms?: number; page?: string; result?: string; target_id?: string } = {},
) {
  console.info("orchestrator.ui.metric", { name, ...fields, recorded_at: new Date().toISOString() });
}
