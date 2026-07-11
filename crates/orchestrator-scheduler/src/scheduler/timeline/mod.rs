//! Semantic, cursor-paginated task timeline projection.

mod builder;
mod cursor;
mod model;

use std::collections::HashSet;

use agent_orchestrator::dto::TaskTimelineSource;
use anyhow::{Result, bail};

pub use model::{
    EvidenceRef, TIMELINE_PROJECTION_VERSION, TimelineActorRef, TimelineCategory, TimelineDelta,
    TimelineDeltaKind, TimelineEntry, TimelinePage,
};

use builder::project_timeline;
use cursor::{TimelineCursor, decode_cursor, encode_cursor};

/// Parsed query parameters for a timeline snapshot.
#[derive(Debug, Clone)]
pub struct TimelineQuery {
    /// Opaque cursor returned by the previous page.
    pub cursor: Option<String>,
    /// Maximum number of semantic entries to return.
    pub limit: usize,
    /// Optional semantic category filters.
    pub categories: Vec<String>,
}

/// Returns the source-event watermark embedded in a query cursor.
pub fn cursor_watermark(cursor: Option<&str>) -> Result<Option<i64>> {
    cursor
        .map(decode_cursor)
        .transpose()
        .map(|value| value.map(|cursor| cursor.snapshot_max_event_id))
}

/// Builds one stable cursor page from a fixed source snapshot.
pub fn build_timeline_page(
    source: &TaskTimelineSource,
    query: &TimelineQuery,
    redaction_patterns: &[String],
) -> Result<TimelinePage> {
    let cursor = query.cursor.as_deref().map(decode_cursor).transpose()?;
    if let Some(cursor) = &cursor {
        if cursor.snapshot_max_event_id != source.snapshot_max_event_id {
            bail!("timeline cursor watermark does not match source snapshot");
        }
    }
    let categories = parse_categories(&query.categories)?;
    let projected = project_timeline(source, redaction_patterns, &categories);
    let start_index = cursor
        .as_ref()
        .map(|cursor| {
            projected.partition_point(|entry| {
                (entry.source_order, entry.entry.id.as_str())
                    <= (cursor.source_order, cursor.entry_id.as_str())
            })
        })
        .unwrap_or(0);
    let limit = query.limit.clamp(1, 200);
    let end_index = start_index.saturating_add(limit).min(projected.len());
    let entries = projected[start_index..end_index]
        .iter()
        .map(|entry| entry.entry.clone())
        .collect::<Vec<_>>();
    let has_more = end_index < projected.len();
    let next_cursor = has_more.then(|| {
        let last = &projected[end_index - 1];
        encode_cursor(&TimelineCursor {
            snapshot_max_event_id: source.snapshot_max_event_id,
            source_order: last.source_order,
            entry_id: last.entry.id.clone(),
        })
    });
    Ok(TimelinePage {
        entries,
        next_cursor,
        has_more,
        snapshot_max_event_id: source.snapshot_max_event_id,
        projection_version: TIMELINE_PROJECTION_VERSION,
    })
}

/// Builds incremental upsert entries whose source events are newer than a watermark.
pub fn build_timeline_updates(
    source: &TaskTimelineSource,
    after_event_id: i64,
    categories: &[String],
    redaction_patterns: &[String],
) -> Result<Vec<TimelineEntry>> {
    let categories = parse_categories(categories)?;
    Ok(project_timeline(source, redaction_patterns, &categories)
        .into_iter()
        .filter(|projected| {
            projected
                .entry
                .raw_event_ids
                .iter()
                .any(|event_id| *event_id > after_event_id)
        })
        .map(|projected| projected.entry)
        .collect())
}

fn parse_categories(values: &[String]) -> Result<HashSet<TimelineCategory>> {
    values
        .iter()
        .map(|value| {
            TimelineCategory::parse(value)
                .ok_or_else(|| anyhow::anyhow!("unknown timeline category '{value}'"))
        })
        .collect()
}

#[cfg(test)]
mod tests;
