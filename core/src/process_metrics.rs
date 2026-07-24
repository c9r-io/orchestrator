//! Privacy-safe, project-scoped operational metrics for the Process Console.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
#[cfg(test)]
use chrono::Timelike;
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;

/// Version of the public Process Console metrics contract.
pub const PROCESS_METRICS_SCHEMA_VERSION: u32 = 1;
/// Supported materialized bucket widths, in seconds.
pub const SUPPORTED_BUCKET_SECONDS: &[u64] = &[60, 300, 900, 3_600, 21_600, 86_400];

const MAX_SOURCE_KEY_BYTES: usize = 256;
const MAX_DIMENSION_VALUE_BYTES: usize = 64;
const MAX_SCAN_ROWS: usize = 100_000;
const HISTOGRAM_BOUNDS: &[f64] = &[
    1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 900.0, 3_600.0, 14_400.0, 86_400.0,
];

/// One accepted optional observation, such as a UI reconnect or timeline duration.
#[derive(Debug, Clone)]
pub struct MetricObservation {
    /// Project isolation scope.
    pub project_id: String,
    /// Stable metric family name.
    pub metric_name: String,
    /// Closed, low-cardinality dimensions.
    pub dimensions: BTreeMap<String, String>,
    /// Numeric sample value.
    pub value: f64,
    /// Daemon-issued RFC3339 occurrence time.
    pub occurred_at: String,
    /// Stable source family used for idempotency.
    pub source_kind: String,
    /// Internal correlation key; never returned as an aggregate label.
    pub source_key: String,
}

/// One time bucket in an aggregate series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricBucket {
    /// Inclusive RFC3339 bucket start.
    pub start: String,
    /// Number of samples.
    pub sample_count: u64,
    /// Sum of sample values.
    pub sum: f64,
    /// Minimum sample value.
    pub min: f64,
    /// Maximum sample value.
    pub max: f64,
}

/// One stable aggregate family and dimension set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricAggregate {
    /// Stable metric family name.
    pub name: String,
    /// Closed low-cardinality label set.
    pub labels: BTreeMap<String, String>,
    /// Number of samples represented by this aggregate.
    pub sample_count: u64,
    /// Sum of sample values.
    pub sum: f64,
    /// Minimum sample value, when samples exist.
    pub min: Option<f64>,
    /// Maximum sample value, when samples exist.
    pub max: Option<f64>,
    /// Convenience value for counters, gauges, and ratios.
    pub value: f64,
    /// Ratio numerator, when applicable.
    pub numerator: Option<u64>,
    /// Ratio denominator, when applicable.
    pub denominator: Option<u64>,
    /// Cumulative histogram counts keyed by stable upper bounds.
    pub histogram: BTreeMap<String, u64>,
    /// Materialized buckets for optional runtime observations.
    pub buckets: Vec<MetricBucket>,
}

impl MetricAggregate {
    fn empty(name: &str, labels: BTreeMap<String, String>) -> Self {
        Self {
            name: name.to_string(),
            labels,
            sample_count: 0,
            sum: 0.0,
            min: None,
            max: None,
            value: 0.0,
            numerator: None,
            denominator: None,
            histogram: BTreeMap::new(),
            buckets: Vec::new(),
        }
    }

    fn sample(&mut self, value: f64, histogram: bool) {
        self.sample_count += 1;
        self.sum += value;
        self.value = self.sum;
        self.min = Some(self.min.map_or(value, |current| current.min(value)));
        self.max = Some(self.max.map_or(value, |current| current.max(value)));
        if histogram {
            for bound in HISTOGRAM_BOUNDS {
                if value <= *bound {
                    *self
                        .histogram
                        .entry(format!("le_{}", *bound as u64))
                        .or_default() += 1;
                }
            }
            *self.histogram.entry("le_inf".to_string()).or_default() += 1;
        }
    }
}

/// Durable health for one replayable projector.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectorHealth {
    /// Stable projector name.
    pub projector: String,
    /// Project scope, empty for global projectors.
    pub project_id: String,
    /// Last committed replay cursor.
    pub cursor: String,
    /// Number of durable source rows behind the cursor.
    pub lag_count: u64,
    /// Number of failed projector batches.
    pub failure_count: u64,
    /// Stable last failure category.
    pub last_error_code: Option<String>,
    /// Last successful batch timestamp.
    pub last_success_at: Option<String>,
    /// Last health update timestamp.
    pub updated_at: String,
}

/// Versioned Process Console metrics read model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessOperationsMetrics {
    /// Public schema version.
    pub schema_version: u32,
    /// Required project isolation scope.
    pub project_id: String,
    /// Inclusive query window start.
    pub window_start: String,
    /// Exclusive query window end.
    pub window_end: String,
    /// Requested materialized bucket width.
    pub bucket_seconds: u64,
    /// Daemon generation timestamp.
    pub generated_at: String,
    /// Earliest optional observation included in local coverage.
    pub coverage_start: Option<String>,
    /// True when historical source records predate exact metric projections.
    pub partial: bool,
    /// Current collection feature state.
    pub collection_enabled: bool,
    /// Stable aggregate series.
    pub metrics: Vec<MetricAggregate>,
    /// Projector failure and lag state.
    pub projector_health: Vec<ProjectorHealth>,
}

/// Validated bounded metrics query.
#[derive(Debug, Clone)]
pub struct ProcessMetricsQuery {
    /// Required project isolation scope.
    pub project_id: String,
    /// Query window in seconds.
    pub window_seconds: u64,
    /// Materialized bucket in seconds.
    pub bucket_seconds: u64,
    /// Current collection feature state.
    pub collection_enabled: bool,
}

/// Parses a compact duration accepted by the Process Console API.
pub fn parse_duration_seconds(value: &str) -> Result<u64> {
    if value.len() < 2 {
        bail!("duration must use a positive integer followed by m, h, or d");
    }
    let (number, suffix) = value.split_at(value.len() - 1);
    let number = number
        .parse::<u64>()
        .context("duration must start with a positive integer")?;
    if number == 0 {
        bail!("duration must be greater than zero");
    }
    let multiplier = match suffix {
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => bail!("duration suffix must be m, h, or d"),
    };
    number
        .checked_mul(multiplier)
        .context("duration is too large")
}

/// Validates a public window and bucket pair against configured limits.
pub fn validate_window_bucket(
    window: &str,
    bucket: &str,
    max_window_days: u32,
) -> Result<(u64, u64)> {
    let window_seconds = parse_duration_seconds(window)?;
    let bucket_seconds = parse_duration_seconds(bucket)?;
    if window_seconds > u64::from(max_window_days.clamp(1, 365)) * 86_400 {
        bail!("window exceeds configured maximum of {max_window_days} days");
    }
    if !SUPPORTED_BUCKET_SECONDS.contains(&bucket_seconds) {
        bail!("bucket must be one of 1m, 5m, 15m, 1h, 6h, or 1d");
    }
    if bucket_seconds > window_seconds {
        bail!("bucket cannot exceed window");
    }
    if window_seconds / bucket_seconds > 744 {
        bail!("window would return more than 744 buckets");
    }
    Ok((window_seconds, bucket_seconds))
}

/// Async repository for operational observations, rollups, and snapshots.
#[derive(Clone)]
pub struct AsyncProcessMetricsRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncProcessMetricsRepository {
    /// Creates a repository over the daemon's shared SQLite connections.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Records one allowlisted observation and its materialized rollups idempotently.
    pub async fn record(&self, observation: MetricObservation) -> Result<bool> {
        self.db
            .writer()
            .call(move |conn| record_observation(conn, observation).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Returns a bounded project-scoped snapshot.
    pub async fn query(&self, query: ProcessMetricsQuery) -> Result<ProcessOperationsMetrics> {
        self.db
            .reader()
            .call(move |conn| query_snapshot(conn, query).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Rebuilds all materialized rollups for one project from retained observations.
    pub async fn rebuild(&self, project_id: &str) -> Result<u64> {
        let project_id = project_id.to_string();
        self.db
            .writer()
            .call(move |conn| rebuild_rollups(conn, &project_id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Deletes optional observations and rollups older than the configured retention.
    pub async fn prune(&self, retention_days: u32) -> Result<u64> {
        self.db
            .writer()
            .call(move |conn| prune(conn, retention_days).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Records a successful projector batch without changing execution authority.
    pub async fn projector_success(
        &self,
        projector: &str,
        project_id: &str,
        cursor: &str,
        lag_count: u64,
    ) -> Result<()> {
        let projector = projector.to_string();
        let project_id = project_id.to_string();
        let cursor = cursor.to_string();
        self.db.writer().call(move |conn| {
            let now = now_ts();
            conn.execute(
                "INSERT INTO process_metric_projector_state
                 (projector,project_id,cursor,lag_count,failure_count,last_error_code,last_success_at,updated_at)
                 VALUES(?1,?2,?3,?4,0,NULL,?5,?5)
                 ON CONFLICT(projector,project_id) DO UPDATE SET
                   cursor=excluded.cursor,lag_count=excluded.lag_count,last_error_code=NULL,
                   last_success_at=excluded.last_success_at,updated_at=excluded.updated_at",
                params![projector, project_id, cursor, lag_count as i64, now],
            )?;
            Ok(())
        }).await.map_err(flatten_err)
    }

    /// Records a failed projector batch while retaining its prior cursor.
    pub async fn projector_failure(
        &self,
        projector: &str,
        project_id: &str,
        error_code: &str,
        lag_count: u64,
    ) -> Result<()> {
        let projector = projector.to_string();
        let project_id = project_id.to_string();
        let error_code = normalize_error_code(error_code);
        self.db
            .writer()
            .call(move |conn| {
                let now = now_ts();
                conn.execute(
                    "INSERT INTO process_metric_projector_state
                 (projector,project_id,cursor,lag_count,failure_count,last_error_code,updated_at)
                 VALUES(?1,?2,'',?3,1,?4,?5)
                 ON CONFLICT(projector,project_id) DO UPDATE SET
                   lag_count=excluded.lag_count,failure_count=failure_count+1,
                   last_error_code=excluded.last_error_code,updated_at=excluded.updated_at",
                    params![projector, project_id, lag_count as i64, error_code, now],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }
}

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
}

fn normalize_error_code(value: &str) -> String {
    let code = value
        .split(|character: char| character == ':' || character.is_ascii_whitespace())
        .next()
        .unwrap_or_default();
    let normalized = code
        .chars()
        .take(64)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.is_empty() {
        "unknown".to_string()
    } else {
        normalized
    }
}

fn allowed_dimensions(metric: &str) -> Option<&'static [&'static str]> {
    match metric {
        "stream_reconnect_total" => Some(&["page", "result"]),
        "timeline_projection_seconds" | "timeline_response_bytes" => Some(&[]),
        "source_event_deduplicated_total" => Some(&["provider"]),
        "ui_page_load_seconds" => Some(&["page"]),
        "dashboard_error_total" => Some(&["result"]),
        "attention_mutation_total" => Some(&["action", "result", "error_category"]),
        "attention_reconciliation_total" => Some(&["action", "result"]),
        _ => None,
    }
}

fn validate_observation(observation: &MetricObservation) -> Result<(String, String)> {
    if observation.project_id.is_empty() || observation.project_id.len() > 128 {
        bail!("project_id must contain 1-128 characters");
    }
    let allowed = allowed_dimensions(&observation.metric_name)
        .context("metric_name is not accepted by the local telemetry sink")?;
    if observation.dimensions.len() > allowed.len() {
        bail!("metric dimensions exceed the allowlist");
    }
    for (key, value) in &observation.dimensions {
        if !allowed.contains(&key.as_str()) {
            bail!(
                "dimension {key} is not allowed for {}",
                observation.metric_name
            );
        }
        if value.is_empty() || value.len() > MAX_DIMENSION_VALUE_BYTES {
            bail!("dimension values must contain 1-64 bytes");
        }
    }
    if !observation.value.is_finite() || observation.value < 0.0 {
        bail!("metric value must be a finite non-negative number");
    }
    if observation.source_kind.is_empty() || observation.source_kind.len() > 64 {
        bail!("source_kind must contain 1-64 characters");
    }
    if observation.source_key.is_empty() || observation.source_key.len() > MAX_SOURCE_KEY_BYTES {
        bail!("source_key must contain 1-256 characters");
    }
    parse_ts(&observation.occurred_at)
        .context("occurred_at must be an RFC3339 daemon timestamp")?;
    let dimension_key = observation
        .dimensions
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("|");
    let dimensions_json = serde_json::to_string(&observation.dimensions)?;
    Ok((dimension_key, dimensions_json))
}

fn record_observation(conn: &Connection, observation: MetricObservation) -> Result<bool> {
    let (dimension_key, dimensions_json) = validate_observation(&observation)?;
    let tx = conn.unchecked_transaction()?;
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO process_metric_observations
         (project_id,metric_name,dimension_key,dimensions_json,value,occurred_at,source_kind,source_key,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            observation.project_id,
            observation.metric_name,
            dimension_key,
            dimensions_json,
            observation.value,
            observation.occurred_at,
            observation.source_kind,
            observation.source_key,
            now_ts(),
        ],
    )? == 1;
    if inserted {
        for bucket_seconds in SUPPORTED_BUCKET_SECONDS {
            upsert_rollup(
                &tx,
                &RollupSample {
                    project_id: &observation.project_id,
                    metric_name: &observation.metric_name,
                    dimension_key: &dimension_key,
                    dimensions_json: &dimensions_json,
                    value: observation.value,
                    occurred_at: &observation.occurred_at,
                },
                *bucket_seconds,
            )?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

struct RollupSample<'a> {
    project_id: &'a str,
    metric_name: &'a str,
    dimension_key: &'a str,
    dimensions_json: &'a str,
    value: f64,
    occurred_at: &'a str,
}

fn upsert_rollup(conn: &Connection, sample: &RollupSample<'_>, bucket_seconds: u64) -> Result<()> {
    let bucket_start = floor_bucket(parse_ts(sample.occurred_at)?, bucket_seconds).to_rfc3339();
    conn.execute(
        "INSERT INTO process_metric_rollups
         (project_id,metric_name,dimension_key,dimensions_json,bucket_start,bucket_seconds,
          sample_count,sum_value,min_value,max_value,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,1,?7,?7,?7,?8)
         ON CONFLICT(project_id,metric_name,dimension_key,bucket_start,bucket_seconds) DO UPDATE SET
           sample_count=sample_count+1,sum_value=sum_value+excluded.sum_value,
           min_value=MIN(min_value,excluded.min_value),max_value=MAX(max_value,excluded.max_value),
           updated_at=excluded.updated_at",
        params![
            sample.project_id,
            sample.metric_name,
            sample.dimension_key,
            sample.dimensions_json,
            bucket_start,
            bucket_seconds as i64,
            sample.value,
            now_ts(),
        ],
    )?;
    Ok(())
}

fn rebuild_rollups(conn: &Connection, project_id: &str) -> Result<u64> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM process_metric_rollups WHERE project_id=?1",
        params![project_id],
    )?;
    let rows = {
        let mut stmt = tx.prepare(
            "SELECT metric_name,dimension_key,dimensions_json,value,occurred_at
             FROM process_metric_observations WHERE project_id=?1 ORDER BY id LIMIT ?2",
        )?;
        stmt.query_map(params![project_id, MAX_SCAN_ROWS as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (metric, dimension_key, dimensions_json, value, occurred_at) in &rows {
        for bucket_seconds in SUPPORTED_BUCKET_SECONDS {
            upsert_rollup(
                &tx,
                &RollupSample {
                    project_id,
                    metric_name: metric,
                    dimension_key,
                    dimensions_json,
                    value: *value,
                    occurred_at,
                },
                *bucket_seconds,
            )?;
        }
    }
    tx.commit()?;
    Ok(rows.len() as u64)
}

fn prune(conn: &Connection, retention_days: u32) -> Result<u64> {
    let cutoff =
        (Utc::now() - Duration::days(i64::from(retention_days.clamp(1, 365)))).to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    let observations = tx.execute(
        "DELETE FROM process_metric_observations WHERE occurred_at < ?1",
        params![cutoff],
    )?;
    let rollups = tx.execute(
        "DELETE FROM process_metric_rollups WHERE bucket_start < ?1",
        params![cutoff],
    )?;
    tx.commit()?;
    Ok((observations + rollups) as u64)
}

fn query_snapshot(
    conn: &Connection,
    query: ProcessMetricsQuery,
) -> Result<ProcessOperationsMetrics> {
    if query.project_id.is_empty() || query.project_id.len() > 128 {
        bail!("project_id must contain 1-128 characters");
    }
    if !SUPPORTED_BUCKET_SECONDS.contains(&query.bucket_seconds) {
        bail!("unsupported bucket size");
    }
    let generated = Utc::now();
    let from = generated - Duration::seconds(query.window_seconds as i64);
    let mut aggregates = HashMap::<String, MetricAggregate>::new();
    collect_rollups(conn, &query, from, generated, &mut aggregates)?;
    let attention_partial =
        collect_attention(conn, &query.project_id, from, generated, &mut aggregates)?;
    collect_autonomous(conn, &query.project_id, from, generated, &mut aggregates)?;
    collect_audit_metrics(conn, &query.project_id, from, generated, &mut aggregates)?;
    collect_handoff_productivity(conn, &query.project_id, from, generated, &mut aggregates)?;
    collect_session_metrics(conn, &query.project_id, from, generated, &mut aggregates)?;
    collect_loop_metrics(conn, &query.project_id, from, generated, &mut aggregates)?;
    collect_source_automation_metrics(conn, &query.project_id, from, generated, &mut aggregates)?;
    let coverage_start = conn.query_row(
        "SELECT MIN(occurred_at) FROM process_metric_observations WHERE project_id=?1",
        params![query.project_id],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let projector_health = read_projector_health(conn, &query.project_id)?;
    let mut metrics = aggregates.into_values().collect::<Vec<_>>();
    metrics.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.labels.cmp(&right.labels))
    });
    Ok(ProcessOperationsMetrics {
        schema_version: PROCESS_METRICS_SCHEMA_VERSION,
        project_id: query.project_id,
        window_start: from.to_rfc3339(),
        window_end: generated.to_rfc3339(),
        bucket_seconds: query.bucket_seconds,
        generated_at: generated.to_rfc3339(),
        coverage_start,
        partial: attention_partial,
        collection_enabled: query.collection_enabled,
        metrics,
        projector_health,
    })
}

fn aggregate_key(name: &str, labels: &BTreeMap<String, String>) -> String {
    format!(
        "{name}|{}",
        labels
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("|")
    )
}

fn aggregate<'a>(
    aggregates: &'a mut HashMap<String, MetricAggregate>,
    name: &str,
    labels: BTreeMap<String, String>,
) -> &'a mut MetricAggregate {
    let key = aggregate_key(name, &labels);
    aggregates
        .entry(key)
        .or_insert_with(|| MetricAggregate::empty(name, labels))
}

fn labels(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn collect_rollups(
    conn: &Connection,
    query: &ProcessMetricsQuery,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT metric_name,dimensions_json,bucket_start,sample_count,sum_value,min_value,max_value
         FROM process_metric_rollups
         WHERE project_id=?1 AND bucket_seconds=?2 AND bucket_start>=?3 AND bucket_start<?4
         ORDER BY metric_name,dimension_key,bucket_start LIMIT ?5",
    )?;
    let rows = stmt.query_map(
        params![
            query.project_id,
            query.bucket_seconds as i64,
            floor_bucket(from, query.bucket_seconds).to_rfc3339(),
            to.to_rfc3339(),
            MAX_SCAN_ROWS as i64,
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, f64>(6)?,
            ))
        },
    )?;
    for row in rows {
        let (name, dimensions_json, start, count, sum, min, max) = row?;
        let metric_labels: BTreeMap<String, String> = serde_json::from_str(&dimensions_json)?;
        let metric = aggregate(aggregates, &name, metric_labels);
        metric.sample_count += count;
        metric.sum += sum;
        metric.value = metric.sum;
        metric.min = Some(metric.min.map_or(min, |current| current.min(min)));
        metric.max = Some(metric.max.map_or(max, |current| current.max(max)));
        metric.buckets.push(MetricBucket {
            start,
            sample_count: count,
            sum,
            min,
            max,
        });
    }
    Ok(())
}

#[derive(Default)]
struct AttentionEpisode {
    started_at: Option<DateTime<Utc>>,
    actionable_since: Option<DateTime<Utc>>,
    actionable_seconds: f64,
    claimed: bool,
}

fn collect_attention(
    conn: &Connection,
    project_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<bool> {
    let mut stmt = conn.prepare(
        "SELECT c.attention_item_id,c.change_kind,COALESCE(c.resulting_state,''),c.created_at,
                i.kind,i.severity
         FROM attention_changes c JOIN attention_items i ON i.id=c.attention_item_id
         WHERE c.project_id=?1 AND c.created_at<?2
         ORDER BY c.attention_item_id,c.id LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(
            params![project_id, to.to_rfc3339(), MAX_SCAN_ROWS as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut current_id = String::new();
    let mut episode = AttentionEpisode::default();
    let mut partial = false;
    for (item_id, change_kind, stored_state, created_at, kind, severity) in rows {
        if current_id != item_id {
            finish_attention_episode(&mut episode, from, to, aggregates);
            current_id = item_id;
            episode = AttentionEpisode::default();
        }
        let at = parse_ts(&created_at)?;
        let state = if stored_state.is_empty() {
            partial = true;
            match change_kind.as_str() {
                "open" | "reopen" => "open",
                "remove" => "resolved",
                _ => continue,
            }
        } else {
            stored_state.as_str()
        };
        if matches!(change_kind.as_str(), "open" | "reopen") {
            finish_attention_episode(&mut episode, from, at, aggregates);
            episode = AttentionEpisode {
                started_at: Some(at),
                actionable_since: Some(at),
                actionable_seconds: 0.0,
                claimed: false,
            };
            if at >= from && at < to {
                aggregate(
                    aggregates,
                    "attention_open_total",
                    labels(&[("kind", &kind), ("severity", &severity)]),
                )
                .sample(1.0, false);
            }
            continue;
        }
        match state {
            "claimed" => {
                if let Some(started_at) = episode.started_at
                    && !episode.claimed
                    && at >= from
                    && at < to
                {
                    aggregate(
                        aggregates,
                        "attention_time_to_claim_seconds",
                        BTreeMap::new(),
                    )
                    .sample((at - started_at).num_milliseconds() as f64 / 1_000.0, true);
                }
                episode.claimed = true;
                episode.actionable_since.get_or_insert(at);
            }
            "snoozed" => close_actionable(&mut episode, from, at),
            "open" => {
                episode.actionable_since.get_or_insert(at);
            }
            "resolved" => {
                close_actionable(&mut episode, from, at);
                if let Some(started_at) = episode.started_at
                    && at >= from
                    && at < to
                {
                    aggregate(
                        aggregates,
                        "attention_time_to_resolution_seconds",
                        BTreeMap::new(),
                    )
                    .sample((at - started_at).num_milliseconds() as f64 / 1_000.0, true);
                }
                finish_attention_episode(&mut episode, from, at, aggregates);
                episode = AttentionEpisode::default();
            }
            _ => partial = true,
        }
    }
    finish_attention_episode(&mut episode, from, to, aggregates);

    let mut active_stmt = conn.prepare(
        "SELECT kind,severity,COUNT(*) FROM attention_items
         WHERE project_id=?1 AND state IN ('open','claimed','snoozed') GROUP BY kind,severity",
    )?;
    let active = active_stmt.query_map(params![project_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, u64>(2)?,
        ))
    })?;
    for row in active {
        let (kind, severity, count) = row?;
        let metric = aggregate(
            aggregates,
            "attention_active",
            labels(&[("kind", &kind), ("severity", &severity)]),
        );
        metric.value = count as f64;
        metric.sample_count = count;
        metric.sum = count as f64;
        metric.min = Some(count as f64);
        metric.max = Some(count as f64);
    }
    Ok(partial)
}

fn close_actionable(episode: &mut AttentionEpisode, from: DateTime<Utc>, at: DateTime<Utc>) {
    if let Some(start) = episode.actionable_since.take() {
        let clipped_start = start.max(from);
        if at > clipped_start {
            episode.actionable_seconds += (at - clipped_start).num_milliseconds() as f64 / 1_000.0;
        }
    }
}

fn finish_attention_episode(
    episode: &mut AttentionEpisode,
    from: DateTime<Utc>,
    at: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) {
    close_actionable(episode, from, at);
    if episode.started_at.is_some() && episode.actionable_seconds > 0.0 {
        aggregate(
            aggregates,
            "process_human_attention_seconds",
            BTreeMap::new(),
        )
        .sample(episode.actionable_seconds, false);
    }
}

fn collect_autonomous(
    conn: &Connection,
    project_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id,COALESCE(started_at,created_at),completed_at FROM tasks
         WHERE project_id=?1 AND status='completed' AND completed_at>=?2 AND completed_at<?3
         ORDER BY completed_at LIMIT ?4",
    )?;
    let tasks = stmt
        .query_map(
            params![
                project_id,
                from.to_rfc3339(),
                to.to_rfc3339(),
                MAX_SCAN_ROWS as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut numerator = 0u64;
    for (task_id, started_at, completed_at) in &tasks {
        if !has_human_intervention(conn, task_id, started_at, completed_at)? {
            numerator += 1;
        }
    }
    let denominator = tasks.len() as u64;
    let metric = aggregate(
        aggregates,
        "process_autonomous_completion_ratio",
        BTreeMap::new(),
    );
    metric.numerator = Some(numerator);
    metric.denominator = Some(denominator);
    metric.value = if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    };
    Ok(())
}

fn has_human_intervention(
    conn: &Connection,
    task_id: &str,
    started_at: &str,
    completed_at: &str,
) -> Result<bool> {
    let count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM control_action_audit a
         WHERE a.status='succeeded' AND a.completed_at>=?2 AND a.completed_at<=?3 AND (
           (a.target_type='task' AND a.target_id=?1 AND a.action IN
             ('task.pause','task.resume','task.retry','task.recover'))
           OR (a.action LIKE 'attention.%' AND EXISTS
             (SELECT 1 FROM attention_items i WHERE i.id=a.target_id AND i.task_id=?1))
           OR (a.action='resume.execute' AND EXISTS
             (SELECT 1 FROM resume_plans p WHERE p.id=a.target_id AND p.task_id=?1))
           OR (a.action IN ('session.send_input','session.close') AND EXISTS
             (SELECT 1 FROM agent_sessions s WHERE s.id=a.target_id AND s.task_id=?1))
         )",
        params![task_id, started_at, completed_at],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn collect_audit_metrics(
    conn: &Connection,
    project_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<()> {
    let mut handoff = conn.prepare(
        "SELECT created_at,completed_at FROM control_action_audit
         WHERE project_id=?1 AND action='handoff.generate' AND status='succeeded'
           AND completed_at>=?2 AND completed_at<?3 AND completed_at IS NOT NULL LIMIT ?4",
    )?;
    for row in handoff.query_map(
        params![
            project_id,
            from.to_rfc3339(),
            to.to_rfc3339(),
            MAX_SCAN_ROWS as i64
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )? {
        let (started, completed) = row?;
        aggregate(aggregates, "handoff_generation_seconds", BTreeMap::new())
            .sample(seconds_between(&started, &completed)?, true);
    }

    let mut resumes = conn.prepare(
        "SELECT COALESCE(p.mode,'unknown'),a.status,COUNT(*)
         FROM control_action_audit a LEFT JOIN resume_plans p ON p.id=a.target_id
         WHERE a.project_id=?1 AND a.action='resume.execute' AND a.status!='reserved'
           AND a.completed_at>=?2 AND a.completed_at<?3
         GROUP BY COALESCE(p.mode,'unknown'),a.status",
    )?;
    for row in resumes.query_map(
        params![project_id, from.to_rfc3339(), to.to_rfc3339()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        },
    )? {
        let (mode, result, count) = row?;
        let metric = aggregate(
            aggregates,
            "resume_attempt_total",
            labels(&[("mode", &mode), ("result", &result)]),
        );
        metric.sample_count += count;
        metric.sum += count as f64;
        metric.value += count as f64;
    }
    Ok(())
}

fn collect_handoff_productivity(
    conn: &Connection,
    project_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT task_id,created_at FROM handoff_snapshots
         WHERE project_id=?1 AND created_at<?2 ORDER BY created_at LIMIT ?3",
    )?;
    let snapshots = stmt
        .query_map(
            params![project_id, to.to_rfc3339(), MAX_SCAN_ROWS as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (task_id, created_at) in snapshots {
        let productive_at: Option<String> = conn
            .query_row(
                "SELECT MIN(at) FROM (
                   SELECT created_at AS at FROM events
                     WHERE task_id=?1 AND event_type='step_started' AND created_at>?2
                   UNION ALL
                   SELECT a.completed_at AS at FROM control_action_audit a
                     JOIN resume_plans p ON p.id=a.target_id
                     WHERE p.task_id=?1 AND a.action='resume.execute' AND a.status='succeeded'
                       AND a.completed_at>?2
                   UNION ALL
                   SELECT a.completed_at AS at FROM control_action_audit a
                     JOIN agent_sessions s ON s.id=a.target_id
                     WHERE s.task_id=?1 AND a.action IN ('session.writer_attach','session.send_input')
                       AND a.status='succeeded' AND a.completed_at>?2
                 )",
                params![task_id, created_at],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if let Some(productive_at) = productive_at {
            let productive = parse_ts(&productive_at)?;
            if productive >= from && productive < to {
                aggregate(
                    aggregates,
                    "handoff_to_productive_action_seconds",
                    BTreeMap::new(),
                )
                .sample(seconds_between(&created_at, &productive_at)?, true);
            }
        }
    }
    Ok(())
}

fn collect_session_metrics(
    conn: &Connection,
    project_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<()> {
    let mut successes = conn.prepare(
        "SELECT a.mode,COUNT(*) FROM session_attachments a
         JOIN agent_sessions s ON s.id=a.session_id JOIN tasks t ON t.id=s.task_id
         WHERE t.project_id=?1 AND a.attached_at>=?2 AND a.attached_at<?3 GROUP BY a.mode",
    )?;
    for row in successes.query_map(
        params![project_id, from.to_rfc3339(), to.to_rfc3339()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
    )? {
        let (mode, count) = row?;
        let metric = aggregate(
            aggregates,
            "session_attachment_total",
            labels(&[("mode", &mode), ("result", "succeeded")]),
        );
        metric.sample_count = count;
        metric.sum = count as f64;
        metric.value = count as f64;
    }
    let mut failures = conn.prepare(
        "SELECT status,COUNT(*) FROM control_action_audit
         WHERE project_id=?1 AND action='session.writer_attach' AND status IN ('failed','denied')
           AND completed_at>=?2 AND completed_at<?3 GROUP BY status",
    )?;
    for row in failures.query_map(
        params![project_id, from.to_rfc3339(), to.to_rfc3339()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
    )? {
        let (result, count) = row?;
        let metric = aggregate(
            aggregates,
            "session_attachment_total",
            labels(&[("mode", "writer"), ("result", &result)]),
        );
        metric.sample_count = count;
        metric.sum = count as f64;
        metric.value = count as f64;
    }
    Ok(())
}

fn collect_loop_metrics(
    conn: &Connection,
    project_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT r.task_item_id,r.phase,r.exit_code FROM command_runs r
         JOIN task_items i ON i.id=r.task_item_id JOIN tasks t ON t.id=i.task_id
         WHERE t.project_id=?1 AND r.started_at>=?2 AND r.started_at<?3
         ORDER BY r.task_item_id,r.phase,r.started_at LIMIT ?4",
    )?;
    let rows = stmt
        .query_map(
            params![
                project_id,
                from.to_rfc3339(),
                to.to_rfc3339(),
                MAX_SCAN_ROWS as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut failed = 0u64;
    let total = rows.len() as u64;
    let mut groups = HashMap::<(String, String), (u64, u64)>::new();
    for (item, phase, exit_code) in rows {
        let entry = groups.entry((item, phase)).or_default();
        if exit_code.is_some_and(|code| code != 0) {
            failed += 1;
            entry.0 += 1;
            entry.1 = entry.1.max(entry.0);
        } else {
            entry.0 = 0;
        }
    }
    let repeated = aggregate(aggregates, "process_repeated_failure_rate", BTreeMap::new());
    repeated.numerator = Some(failed);
    repeated.denominator = Some(total);
    repeated.value = ratio(failed, total);
    let degenerate = groups.values().filter(|(_, maximum)| *maximum >= 3).count() as u64;
    let group_count = groups.len() as u64;
    let metric = aggregate(aggregates, "process_degenerate_loop_rate", BTreeMap::new());
    metric.numerator = Some(degenerate);
    metric.denominator = Some(group_count);
    metric.value = ratio(degenerate, group_count);
    Ok(())
}

fn add_counter(metric: &mut MetricAggregate, count: u64) {
    metric.sample_count += count;
    metric.sum += count as f64;
    metric.value += count as f64;
}

fn safe_source_provider(value: &str) -> &str {
    match value {
        "slack" => "slack",
        "fixture" => "fixture",
        _ => "other",
    }
}

fn collect_source_automation_metrics(
    conn: &Connection,
    project_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    aggregates: &mut HashMap<String, MetricAggregate>,
) -> Result<()> {
    let mut received = conn.prepare(
        "SELECT provider,COUNT(*) FROM source_events
         WHERE project_id=?1 AND event_type='reaction_added'
           AND received_at>=?2 AND received_at<?3 GROUP BY provider",
    )?;
    for row in received.query_map(
        params![project_id, from.to_rfc3339(), to.to_rfc3339()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
    )? {
        let (provider, count) = row?;
        add_counter(
            aggregate(
                aggregates,
                "source_reaction_received_total",
                labels(&[
                    ("provider", safe_source_provider(&provider)),
                    ("result", "accepted"),
                ]),
            ),
            count,
        );
    }

    let mut matches = conn.prepare(
        "SELECT provider,
          CASE
            WHEN automation_route_id IS NOT NULL THEN 'matched'
            WHEN routing_state='needs_attention' AND last_error_code IN ('binding_ambiguous','trigger_ambiguous') THEN 'ambiguous'
            ELSE 'no_match' END AS result,
          COUNT(*)
         FROM source_events
         WHERE project_id=?1 AND event_type='reaction_added'
           AND received_at>=?2 AND received_at<?3
         GROUP BY provider,result",
    )?;
    for row in matches.query_map(
        params![project_id, from.to_rfc3339(), to.to_rfc3339()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        },
    )? {
        let (provider, result, count) = row?;
        add_counter(
            aggregate(
                aggregates,
                "source_binding_match_total",
                labels(&[
                    ("provider", safe_source_provider(&provider)),
                    ("result", &result),
                ]),
            ),
            count,
        );
    }

    let mut transitions = conn.prepare(
        "SELECT r.provider,c.state,COUNT(*)
         FROM source_automation_route_changes c
         JOIN source_automation_routes r ON r.id=c.route_id
         WHERE r.project_id=?1 AND c.created_at>=?2 AND c.created_at<?3
           AND c.state IN ('rendered','creating','routed','needs_attention','failed')
         GROUP BY r.provider,c.state",
    )?;
    for row in transitions.query_map(
        params![project_id, from.to_rfc3339(), to.to_rfc3339()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        },
    )? {
        let (provider, state, count) = row?;
        match state.as_str() {
            "rendered" => add_counter(
                aggregate(
                    aggregates,
                    "source_permalink_resolution_total",
                    labels(&[
                        ("provider", safe_source_provider(&provider)),
                        ("result", "resolved"),
                    ]),
                ),
                count,
            ),
            "creating" => add_counter(
                aggregate(
                    aggregates,
                    "source_task_render_total",
                    labels(&[("result", "rendered")]),
                ),
                count,
            ),
            "routed" => add_counter(
                aggregate(
                    aggregates,
                    "source_task_creation_total",
                    labels(&[
                        ("provider", safe_source_provider(&provider)),
                        ("result", "created"),
                    ]),
                ),
                count,
            ),
            "needs_attention" | "failed" => add_counter(
                aggregate(
                    aggregates,
                    "source_task_creation_total",
                    labels(&[
                        ("provider", safe_source_provider(&provider)),
                        ("result", "failed"),
                    ]),
                ),
                count,
            ),
            _ => {}
        }
    }

    let mut retries = conn.prepare(
        "SELECT COALESCE(a.error_category,'unknown'),COUNT(*)
         FROM source_automation_route_attempts a
         JOIN source_automation_routes r ON r.id=a.route_id
         WHERE r.project_id=?1 AND a.result_state='retrying'
           AND a.completed_at>=?2 AND a.completed_at<?3
         GROUP BY COALESCE(a.error_category,'unknown')",
    )?;
    for row in retries.query_map(
        params![project_id, from.to_rfc3339(), to.to_rfc3339()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
    )? {
        let (reason, count) = row?;
        add_counter(
            aggregate(
                aggregates,
                "source_route_retry_total",
                labels(&[("reason", &normalize_error_code(&reason))]),
            ),
            count,
        );
    }

    let mut latency = conn.prepare(
        "SELECT provider,created_at,completed_at FROM source_automation_routes
         WHERE project_id=?1 AND status='routed' AND completed_at>=?2 AND completed_at<?3
           AND completed_at IS NOT NULL LIMIT ?4",
    )?;
    for row in latency.query_map(
        params![
            project_id,
            from.to_rfc3339(),
            to.to_rfc3339(),
            MAX_SCAN_ROWS as i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )? {
        let (provider, created_at, completed_at) = row?;
        aggregate(
            aggregates,
            "source_route_latency_seconds",
            labels(&[
                ("provider", safe_source_provider(&provider)),
                ("result", "routed"),
            ]),
        )
        .sample(seconds_between(&created_at, &completed_at)?, true);
    }
    Ok(())
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn read_projector_health(conn: &Connection, project_id: &str) -> Result<Vec<ProjectorHealth>> {
    let mut stmt = conn.prepare(
        "SELECT projector,project_id,cursor,lag_count,failure_count,last_error_code,last_success_at,updated_at
         FROM process_metric_projector_state WHERE project_id IN ('',?1)
         ORDER BY projector,project_id LIMIT 64",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(ProjectorHealth {
            projector: row.get(0)?,
            project_id: row.get(1)?,
            cursor: row.get(2)?,
            lag_count: row.get::<_, u64>(3)?,
            failure_count: row.get::<_, u64>(4)?,
            last_error_code: row.get(5)?,
            last_success_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn parse_ts(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return Ok(value.with_timezone(&Utc));
    }
    let parsed = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .with_context(|| format!("invalid timestamp: {value}"))?;
    Ok(DateTime::from_naive_utc_and_offset(parsed, Utc))
}

fn seconds_between(start: &str, end: &str) -> Result<f64> {
    Ok((parse_ts(end)? - parse_ts(start)?).num_milliseconds() as f64 / 1_000.0)
}

fn floor_bucket(value: DateTime<Utc>, bucket_seconds: u64) -> DateTime<Utc> {
    let seconds = value.timestamp();
    let floored = seconds - seconds.rem_euclid(bucket_seconds as i64);
    Utc.timestamp_opt(floored, 0).single().unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use tempfile::tempdir;

    #[test]
    fn duration_validation_is_bounded() {
        assert_eq!(
            validate_window_bucket("24h", "1h", 30).unwrap(),
            (86_400, 3_600)
        );
        assert!(validate_window_bucket("31d", "1h", 30).is_err());
        assert!(validate_window_bucket("1h", "1d", 30).is_err());
        assert!(validate_window_bucket("24h", "2h", 30).is_err());
    }

    #[tokio::test]
    async fn observations_are_allowlisted_idempotent_and_rebuildable() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("metrics.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let repo = AsyncProcessMetricsRepository::new(db.clone());
        let observation = MetricObservation {
            project_id: "p1".into(),
            metric_name: "source_event_deduplicated_total".into(),
            dimensions: labels(&[("provider", "slack")]),
            value: 1.0,
            occurred_at: "2026-07-14T00:00:00Z".into(),
            source_kind: "source_delivery".into(),
            source_key: "delivery-1".into(),
        };
        assert!(repo.record(observation.clone()).await.unwrap());
        assert!(!repo.record(observation).await.unwrap());
        assert_eq!(repo.rebuild("p1").await.unwrap(), 1);
        let count: u64 = db
            .reader()
            .call(|conn| {
                conn.query_row(
                    "SELECT sample_count FROM process_metric_rollups
                     WHERE project_id='p1' AND bucket_seconds=3600",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn high_cardinality_dimensions_are_rejected() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("metrics.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let repo = AsyncProcessMetricsRepository::new(db);
        let result = repo
            .record(MetricObservation {
                project_id: "p1".into(),
                metric_name: "stream_reconnect_total".into(),
                dimensions: labels(&[("task_id", "secret-task")]),
                value: 1.0,
                occurred_at: "2026-07-14T00:00:00Z".into(),
                source_kind: "ui".into(),
                source_key: "ui-1".into(),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn attention_observations_accept_only_privacy_safe_dimensions() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("metrics.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let repo = AsyncProcessMetricsRepository::new(db);
        let accepted = repo
            .record(MetricObservation {
                project_id: "p1".into(),
                metric_name: "attention_mutation_total".into(),
                dimensions: labels(&[
                    ("action", "claim"),
                    ("result", "failure"),
                    ("error_category", "conflict"),
                ]),
                value: 1.0,
                occurred_at: "2026-07-14T00:00:00Z".into(),
                source_kind: "ui".into(),
                source_key: "ui-attention-1".into(),
            })
            .await;
        assert!(accepted.unwrap());

        let rejected = repo
            .record(MetricObservation {
                project_id: "p1".into(),
                metric_name: "attention_reconciliation_total".into(),
                dimensions: labels(&[
                    ("action", "claim"),
                    ("result", "unconfirmed"),
                    ("requested_decision", "secret body"),
                ]),
                value: 1.0,
                occurred_at: "2026-07-14T00:00:01Z".into(),
                source_kind: "ui".into(),
                source_key: "ui-attention-2".into(),
            })
            .await;
        assert!(rejected.is_err());
    }

    #[tokio::test]
    async fn retention_prunes_only_expired_optional_metric_state() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("metrics.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let repo = AsyncProcessMetricsRepository::new(db.clone());
        repo.record(MetricObservation {
            project_id: "p1".into(),
            metric_name: "source_event_deduplicated_total".into(),
            dimensions: labels(&[("provider", "slack")]),
            value: 1.0,
            occurred_at: "2000-01-01T00:00:00Z".into(),
            source_kind: "source_delivery".into(),
            source_key: "expired-delivery".into(),
        })
        .await
        .expect("record expired observation");

        assert!(repo.prune(30).await.expect("prune") > 0);
        let remaining: u64 = db
            .reader()
            .call(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM process_metric_observations",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .await
            .expect("count retained observations");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn projector_failure_retains_the_last_successful_cursor() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("metrics.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let repo = AsyncProcessMetricsRepository::new(db);
        repo.projector_success("attention", "p1", "42", 0)
            .await
            .expect("projector success");
        repo.projector_failure("attention", "p1", "SQLITE_BUSY: raw detail", 7)
            .await
            .expect("projector failure");

        let snapshot = repo
            .query(ProcessMetricsQuery {
                project_id: "p1".into(),
                window_seconds: 3_600,
                bucket_seconds: 60,
                collection_enabled: true,
            })
            .await
            .expect("query projector health");
        let health = snapshot
            .projector_health
            .iter()
            .find(|health| health.projector == "attention")
            .expect("attention projector health");
        assert_eq!(health.cursor, "42");
        assert_eq!(health.lag_count, 7);
        assert_eq!(health.failure_count, 1);
        assert_eq!(health.last_error_code.as_deref(), Some("sqlite_busy"));
    }

    #[tokio::test]
    async fn deterministic_fixture_produces_exact_process_metrics() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("metrics.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let base = (Utc::now() - Duration::hours(1))
            .with_nanosecond(0)
            .expect("whole second");
        let times = (0..=400)
            .map(|offset| (base + Duration::seconds(offset)).to_rfc3339())
            .collect::<Vec<_>>();
        db.writer()
            .call(
                move |conn: &mut rusqlite::Connection| -> tokio_rusqlite::Result<()> {
                for task in ["t1", "t2", "t3", "t4"] {
                    conn.execute(
                        "INSERT INTO tasks
                         (id,name,status,started_at,completed_at,goal,target_files_json,mode,
                          workspace_id,workflow_id,project_id,workspace_root,qa_targets_json,
                          ticket_dir,execution_plan_json,loop_mode,current_cycle,init_done,
                          created_at,updated_at)
                         VALUES(?1,?1,'completed',?2,?3,'fixture','[]','default','default',
                                'default','p1','/tmp','[]','docs/ticket','{}','once',1,1,?2,?3)",
                        params![task, times[0], times[300]],
                    )?;
                }
                conn.execute(
                    "INSERT INTO task_items
                     (id,task_id,order_no,qa_file_path,status,ticket_files_json,ticket_content_json,
                      fix_required,fixed,last_error,created_at,updated_at)
                     VALUES('item-1','t1',0,'fixture.md','completed','[]','[]',0,0,'',?1,?1)",
                    params![times[0]],
                )?;
                for (index, exit_code) in [1_i64, 1, 1, 0].into_iter().enumerate() {
                    conn.execute(
                        "INSERT INTO command_runs
                         (id,task_item_id,phase,command,cwd,workspace_id,agent_id,project_id,
                          exit_code,stdout_path,stderr_path,output_json,artifacts_json,
                          validation_status,started_at,ended_at)
                         VALUES(?1,'item-1','test','fixture','/tmp','default','fixture','p1',?2,
                                '/tmp/out','/tmp/err','{}','[]','valid',?3,?3)",
                        params![format!("run-{index}"), exit_code, times[10 + index]],
                    )?;
                }
                conn.execute(
                    "INSERT INTO attention_items
                     (id,project_id,task_id,kind,severity,state,title,summary,actions_json,
                      dedupe_key,source_event_id,occurrence_count,reopen_count,version,
                      created_at,updated_at,last_occurred_at,resolved_at)
                     VALUES('attn-1','p1','t1','step_failed','intervention','resolved','Failure',
                            'Fixture','[]','fixture','event-1',2,1,6,?1,?2,?2,?2)",
                    params![times[0], times[80]],
                )?;
                for (index, (kind, state, offset)) in [
                    ("open", "open", 0_usize),
                    ("upsert", "claimed", 10),
                    ("remove", "resolved", 40),
                    ("reopen", "open", 60),
                    ("upsert", "claimed", 65),
                    ("remove", "resolved", 80),
                ]
                .into_iter()
                .enumerate()
                {
                    conn.execute(
                        "INSERT INTO attention_changes
                         (attention_item_id,change_kind,item_version,created_at,project_id,resulting_state)
                         VALUES('attn-1',?1,?2,?3,'p1',?4)",
                        params![kind, index as i64 + 1, times[offset], state],
                    )?;
                }
                for (request_id, target_type, target_id, action, created, completed) in [
                    ("audit-retry", "task", "t4", "task.retry", 240_usize, 250_usize),
                    (
                        "audit-handoff",
                        "task",
                        "t1",
                        "handoff.generate",
                        90,
                        92,
                    ),
                    (
                        "audit-resume",
                        "resume_plan",
                        "plan-1",
                        "resume.execute",
                        180,
                        181,
                    ),
                ] {
                    conn.execute(
                        "INSERT INTO control_action_audit
                         (request_id,schema_version,project_id,transport,target_type,target_id,
                          action,reason_code,request_hash,status,created_at,updated_at,completed_at)
                         VALUES(?1,1,'p1','uds',?2,?3,?4,'fixture','hash','succeeded',?5,?6,?6)",
                        params![request_id, target_type, target_id, action, times[created], times[completed]],
                    )?;
                }
                conn.execute(
                    "INSERT INTO handoff_snapshots
                     (id,project_id,task_id,source_event_cursor,projection_version,briefing_json,
                      content_hash,state_version,generated_by,created_at)
                     VALUES('handoff-1','p1','t1',0,1,'{}','hash','v1','fixture',?1)",
                    params![times[100]],
                )?;
                conn.execute(
                    "INSERT INTO events(task_id,event_type,payload_json,created_at)
                     VALUES('t1','step_started','{}',?1)",
                    params![times[120]],
                )?;
                conn.execute(
                    "INSERT INTO resume_plans
                     (id,project_id,task_id,boundary_id,mode,expected_state_version,
                      side_effect_class,replay_safe,elevated_confirmation_required,
                      consequence_json,execution_input_json,status,expires_at,created_by,created_at)
                     VALUES('plan-1','p1','t4','boundary-1','restart_step','v1','idempotent',1,0,
                            '{}','{}','executed',?1,'fixture',?2)",
                    params![times[400], times[170]],
                )?;
                conn.execute(
                    "INSERT INTO agent_sessions
                     (id,task_id,step_id,phase,agent_id,state,pid,pty_backend,cwd,command,
                      input_fifo_path,stdout_path,stderr_path,transcript_path,created_at,updated_at)
                     VALUES('session-1','t2','implement','implement','fixture','active',0,'script',
                            '/tmp','fixture','/tmp/in','/tmp/out','/tmp/err','/tmp/transcript',?1,?1)",
                    params![times[140]],
                )?;
                conn.execute(
                    "INSERT INTO session_attachments(session_id,client_id,mode,attached_at)
                     VALUES('session-1','client-1','reader',?1)",
                    params![times[150]],
                )?;
                    Ok(())
                },
            )
            .await
            .expect("seed deterministic metrics");

        let repository = AsyncProcessMetricsRepository::new(db);
        repository
            .record(MetricObservation {
                project_id: "p1".into(),
                metric_name: "source_event_deduplicated_total".into(),
                dimensions: labels(&[("provider", "slack")]),
                value: 1.0,
                occurred_at: (base + Duration::seconds(200)).to_rfc3339(),
                source_kind: "source_delivery".into(),
                source_key: "duplicate-1".into(),
            })
            .await
            .expect("record source dedup");
        let snapshot = repository
            .query(ProcessMetricsQuery {
                project_id: "p1".into(),
                window_seconds: 86_400,
                bucket_seconds: 3_600,
                collection_enabled: true,
            })
            .await
            .expect("query exact fixture");
        let find = |name: &str| {
            snapshot
                .metrics
                .iter()
                .find(|metric| metric.name == name)
                .unwrap_or_else(|| panic!("missing metric {name}"))
        };
        assert_eq!(find("attention_open_total").value, 2.0);
        assert_eq!(find("attention_time_to_claim_seconds").sum, 15.0);
        assert_eq!(find("attention_time_to_resolution_seconds").sum, 60.0);
        assert_eq!(find("process_human_attention_seconds").sum, 60.0);
        assert_eq!(
            find("process_autonomous_completion_ratio").numerator,
            Some(3)
        );
        assert_eq!(
            find("process_autonomous_completion_ratio").denominator,
            Some(4)
        );
        assert_eq!(find("handoff_generation_seconds").sum, 2.0);
        assert_eq!(find("handoff_to_productive_action_seconds").sum, 20.0);
        assert_eq!(find("resume_attempt_total").value, 1.0);
        assert_eq!(find("session_attachment_total").value, 1.0);
        assert_eq!(find("source_event_deduplicated_total").value, 1.0);
        assert_eq!(find("process_repeated_failure_rate").numerator, Some(3));
        assert_eq!(find("process_repeated_failure_rate").denominator, Some(4));
        assert_eq!(find("process_degenerate_loop_rate").numerator, Some(1));
        assert_eq!(find("process_degenerate_loop_rate").denominator, Some(1));
    }

    #[tokio::test]
    async fn source_automation_metrics_are_authoritative_and_privacy_safe() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("source-automation-metrics.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let base = (Utc::now() - Duration::minutes(10))
            .with_nanosecond(0)
            .expect("whole second");
        let received = base.to_rfc3339();
        let rendered = (base + Duration::seconds(5)).to_rfc3339();
        let creating = (base + Duration::seconds(10)).to_rfc3339();
        let completed = (base + Duration::seconds(20)).to_rfc3339();
        db.writer()
            .call(
                move |conn: &mut rusqlite::Connection| -> tokio_rusqlite::Result<()> {
                    conn.execute(
                        "INSERT INTO source_events
                         (id,project_id,provider,installation_id,external_event_id,event_type,
                          occurred_at,received_at,normalized_payload_json,payload_hash,routing_state,
                          automation_route_id)
                         VALUES('event-private','p-source','slack','T_SECRET','external-private',
                                'reaction_added',?1,?1,'{}','hash','routed','route-private')",
                        params![received],
                    )?;
                    conn.execute(
                        "INSERT INTO source_automation_routes
                         (id,project_id,automation_key,source_event_id,provider,installation_id,
                          message_identity,channel_id,message_ts,reaction,resolved_role,binding_name,
                          binding_revision,template_name,template_hash,binding_snapshot_json,
                          template_snapshot_json,credential_store,credential_key,request_id,
                          deterministic_task_id,status,created_at,updated_at,completed_at)
                         VALUES('route-private','p-source','key-private','event-private','slack',
                                'T_SECRET','C_SECRET:1.23','C_SECRET','1.23','agent-analyze',
                                'operator','binding-private','revision-private','template-private',
                                'template-hash','{}','{}','secret-store','TOKEN','request-private',
                                'task-private','routed',?1,?2,?2)",
                        params![received, completed],
                    )?;
                    for (version, state, at) in [
                        (2_i64, "rendered", rendered.as_str()),
                        (3_i64, "creating", creating.as_str()),
                        (4_i64, "routed", completed.as_str()),
                    ] {
                        conn.execute(
                            "INSERT INTO source_automation_route_changes
                             (route_id,route_version,state,created_at)
                             VALUES('route-private',?1,?2,?3)",
                            params![version, state, at],
                        )?;
                    }
                    conn.execute(
                        "INSERT INTO source_automation_route_attempts
                         (route_id,generation,attempt_no,lease_token,started_at,completed_at,
                          result_state,error_code,error_category,retry_after_seconds)
                         VALUES('route-private',1,1,'lease-private',?1,?2,'retrying',
                                'slack_rate_limited','rate_limit',30)",
                        params![received, rendered],
                    )?;
                    Ok(())
                },
            )
            .await
            .expect("seed source automation metrics");

        let snapshot = AsyncProcessMetricsRepository::new(db)
            .query(ProcessMetricsQuery {
                project_id: "p-source".into(),
                window_seconds: 3_600,
                bucket_seconds: 60,
                collection_enabled: true,
            })
            .await
            .expect("query metrics");
        for expected in [
            "source_reaction_received_total",
            "source_binding_match_total",
            "source_permalink_resolution_total",
            "source_task_render_total",
            "source_task_creation_total",
            "source_route_retry_total",
            "source_route_latency_seconds",
        ] {
            assert!(
                snapshot
                    .metrics
                    .iter()
                    .any(|metric| metric.name == expected),
                "missing {expected}"
            );
        }
        let encoded = serde_json::to_string(&snapshot).expect("serialize metrics");
        for secret_identity in [
            "T_SECRET",
            "C_SECRET",
            "binding-private",
            "template-private",
            "secret-store",
        ] {
            assert!(!encoded.contains(secret_identity));
        }
    }

    #[tokio::test]
    #[ignore = "release-mode deterministic performance fixture"]
    async fn large_fixture_query_meets_process_metrics_budget() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("metrics-performance.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(path).await.expect("db"));
        let now = Utc::now().with_nanosecond(0).expect("whole second");
        let created_at = (now - Duration::hours(1)).to_rfc3339();
        db.writer()
            .call(
                move |conn: &mut rusqlite::Connection| -> tokio_rusqlite::Result<()> {
                    let tx = conn.unchecked_transaction()?;
                    tx.execute(
                        "INSERT INTO tasks
                         (id,name,status,goal,target_files_json,mode,workspace_id,workflow_id,
                          project_id,workspace_root,qa_targets_json,ticket_dir,execution_plan_json,
                          loop_mode,current_cycle,init_done,created_at,updated_at)
                         VALUES('perf-task','perf-task','running','fixture','[]','default','default',
                                'default','perf','/tmp','[]','docs/ticket','{}','once',1,1,?1,?1)",
                        params![created_at],
                    )?;
                    for index in 0..50_000_u64 {
                        tx.execute(
                            "INSERT INTO events(task_id,event_type,payload_json,created_at)
                             VALUES('perf-task','step_finished','{}',?1)",
                            params![created_at],
                        )?;
                        if index < 5_000 {
                            let id = format!("attention-{index}");
                            tx.execute(
                                "INSERT INTO attention_items
                                 (id,project_id,task_id,kind,severity,state,title,summary,actions_json,
                                  dedupe_key,source_event_id,created_at,updated_at,last_occurred_at)
                                 VALUES(?1,'perf','perf-task','step_failed','attention','open','Failure',
                                        'Fixture','[]',?1,?1,?2,?2,?2)",
                                params![id, created_at],
                            )?;
                            tx.execute(
                                "INSERT INTO attention_changes
                                 (attention_item_id,change_kind,item_version,created_at,project_id,resulting_state)
                                 VALUES(?1,'open',1,?2,'perf','open')",
                                params![id, created_at],
                            )?;
                        }
                    }
                    tx.execute(
                        "INSERT INTO process_metric_rollups
                         (project_id,metric_name,dimension_key,dimensions_json,bucket_start,
                          bucket_seconds,sample_count,sum_value,min_value,max_value,updated_at)
                         VALUES('perf','timeline_projection_seconds','','{}',?1,3600,50000,2500,0.01,0.2,?1)",
                        params![created_at],
                    )?;
                    tx.commit()?;
                    Ok(())
                },
            )
            .await
            .expect("seed large fixture");
        let repository = AsyncProcessMetricsRepository::new(db);
        let started = std::time::Instant::now();
        let snapshot = repository
            .query(ProcessMetricsQuery {
                project_id: "perf".into(),
                window_seconds: 86_400,
                bucket_seconds: 3_600,
                collection_enabled: true,
            })
            .await
            .expect("query large fixture");
        let elapsed = started.elapsed();
        let bytes = serde_json::to_vec(&snapshot)
            .expect("serialize snapshot")
            .len();
        assert!(
            elapsed <= std::time::Duration::from_millis(300),
            "metrics query exceeded 300ms: {elapsed:?}"
        );
        assert!(
            bytes <= 256 * 1024,
            "metrics response exceeded 256KiB: {bytes}"
        );
    }
}
