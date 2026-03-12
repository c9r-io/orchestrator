use crate::dto::EventDto;
use chrono::TimeZone;

use super::model::TraceTaskMeta;

pub(super) fn compute_wall_time(
    task_meta: &TraceTaskMeta<'_>,
    events: &[&EventDto],
) -> Option<f64> {
    let start_ts = task_meta
        .started_at
        .or_else(|| (!task_meta.created_at.is_empty()).then_some(task_meta.created_at))
        .or_else(|| events.first().map(|e| e.created_at.as_str()))?;

    let end_ts = if matches!(task_meta.status, "completed" | "failed") {
        task_meta
            .completed_at
            .or_else(|| (!task_meta.updated_at.is_empty()).then_some(task_meta.updated_at))
            .or_else(|| events.last().map(|e| e.created_at.as_str()))?
    } else {
        events
            .last()
            .map(|e| e.created_at.as_str())
            .or_else(|| (!task_meta.updated_at.is_empty()).then_some(task_meta.updated_at))?
    };

    let start = parse_trace_timestamp(start_ts)?;
    let end = parse_trace_timestamp(end_ts)?;
    let duration = end.signed_duration_since(start);
    Some(duration.num_milliseconds() as f64 / 1000.0)
}

pub(super) fn parse_trace_timestamp(ts: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
        return Some(parsed);
    }

    let zero_offset = chrono::FixedOffset::east_opt(0)?;
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(ts, fmt) {
            if let Some(parsed) = zero_offset.from_local_datetime(&naive).single() {
                return Some(parsed);
            }
        }
    }

    None
}
