//! Backlinks from an audited row to the control-plane request that produced it.
//!
//! Three tables carry a `request_id` column that is filled in *after* the row
//! exists, because the audit envelope is only final once the action has
//! succeeded. The statements are one-liners; what they are doing here rather
//! than at the call site is FR-141's answer to a narrower question — the daemon
//! held a connection to run them, and that is the capability being withdrawn.
//!
//! [`SourceAuditTable`] replaces a `match table { "source_events" => …, _ =>
//! return Err(Status::internal("invalid source audit table")) }` in the daemon.
//! The table name was a `&str` chosen two frames above the statement, so an
//! unexpected value was a runtime error on a path that could not otherwise
//! fail. An enum cannot be given a fourth value by accident, and the arm that
//! reported the impossible case is gone with it.

use anyhow::Result;
use rusqlite::params;

use crate::async_database::{AsyncDatabase, flatten_err};

/// The source-ingestion tables that carry an audit backlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceAuditTable {
    /// The `source_events` table.
    Events,
    /// The `source_bindings` table.
    Bindings,
}

impl SourceAuditTable {
    fn statement(self) -> &'static str {
        match self {
            Self::Events => "UPDATE source_events SET request_id=?2 WHERE id=?1",
            Self::Bindings => "UPDATE source_bindings SET request_id=?2 WHERE id=?1",
        }
    }
}

/// Links a source row to the request that produced it.
pub async fn link_source_row(
    db: &AsyncDatabase,
    table: SourceAuditTable,
    id: String,
    request_id: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(table.statement(), params![id, request_id])?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Links an attention action to the request that produced it.
pub async fn link_attention_action(
    db: &AsyncDatabase,
    attention_item_id: String,
    idempotency_key: String,
    request_id: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE attention_actions SET request_id=?3 WHERE attention_item_id=?1 AND idempotency_key=?2",
                params![attention_item_id, idempotency_key, request_id],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}
