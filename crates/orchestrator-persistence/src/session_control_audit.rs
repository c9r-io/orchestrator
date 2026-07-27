//! The `session_control_actions` table: idempotency envelopes for interactive
//! session control.
//!
//! What lives here is the table and the statements over it. What deliberately
//! does not is the control contract above them — what a reused idempotency key
//! means, which replayed result may be returned to a caller and which must be an
//! error, and when an attempt is allowed to reserve at all. Those stay in the
//! daemon's session handlers, which settle them and then call one of these.
//!
//! The split shows up in [`Reservation`], and it is the same split
//! [`crate::control_action_audit`] made for its own table: `INSERT OR IGNORE`
//! followed by a read is one storage operation, so it is one function here, but
//! *which* prior row came back is what the conflict rule turns on. This module
//! names the case it took rather than deciding what it means.
//!
//! The statements are the ones that ran in `crates/daemon/src/server/session.rs`
//! before FR-141 B2, transcribed rather than rewritten — including their
//! original column order — so the move can be read as a move.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::async_database::{AsyncDatabase, flatten_err};

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
}

/// The prior row an idempotency key already had, when it had one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reservation {
    /// No row existed; this attempt owns the action.
    Reserved,
    /// A row existed. The caller decides whether the stored hash matches and
    /// what the stored result permits.
    Replayed {
        /// The `request_hash` recorded when the action was first accepted.
        request_hash: String,
        /// The `result` the action reached, or `reserved` if it is still running.
        result: String,
    },
}

/// One attempt to claim `send_input` for a session under an idempotency key.
#[derive(Debug, Clone)]
pub struct SendInputReservation {
    /// Session the input is addressed to.
    pub session_id: String,
    /// Trusted actor identity.
    pub actor: String,
    /// Calling client.
    pub client_id: String,
    /// Retry identity.
    pub idempotency_key: String,
    /// Hash of the canonical request, computed by the caller.
    pub request_hash: String,
    /// Writer lease fencing token.
    pub fencing_token: i64,
    /// Timestamp to record.
    pub created_at: String,
    /// Audit request identifier.
    pub request_id: String,
}

/// One attempt to claim `close` for a session under an idempotency key.
#[derive(Debug, Clone)]
pub struct CloseReservation {
    /// Session the action is addressed to.
    pub session_id: String,
    /// Trusted actor identity.
    pub actor: String,
    /// Retry identity.
    pub idempotency_key: String,
    /// Hash of the canonical request, computed by the caller.
    pub request_hash: String,
    /// Bounded operator explanation.
    pub reason: String,
    /// Timestamp to record.
    pub created_at: String,
    /// Audit request identifier.
    pub request_id: String,
}

/// Reserves `send_input` under an idempotency key, or reports the prior row.
pub async fn reserve_send_input(
    db: &AsyncDatabase,
    reservation: SendInputReservation,
) -> Result<Reservation> {
    db.writer()
        .call(move |conn| {
            let inserted = conn
                .execute(
                    "INSERT OR IGNORE INTO session_control_actions(session_id,actor,client_id,action,idempotency_key,request_hash,result,fencing_token,created_at,request_id) VALUES(?1,?2,?3,'send_input',?4,?5,'reserved',?6,?7,?8)",
                    params![
                        reservation.session_id,
                        reservation.actor,
                        reservation.client_id,
                        reservation.idempotency_key,
                        reservation.request_hash,
                        reservation.fencing_token,
                        reservation.created_at,
                        reservation.request_id
                    ],
                )
                .map_err(|error| other(error.into()))?;
            if inserted == 1 {
                return Ok(Reservation::Reserved);
            }
            let prior = conn
                .query_row(
                    "SELECT request_hash,result FROM session_control_actions WHERE session_id=?1 AND idempotency_key=?2",
                    params![reservation.session_id, reservation.idempotency_key],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(|error| other(error.into()))?;
            Ok(Reservation::Replayed {
                request_hash: prior.0,
                result: prior.1,
            })
        })
        .await
        .map_err(flatten_err)
}

/// Reserves `close` under an idempotency key, or reports the prior row.
///
/// `'close'` stays a literal in the statement, as it was before the move. The
/// action column is not a parameter here because making it one would rewrite
/// the statement rather than relocate it, and FR-141 moves SQL without changing
/// it.
pub async fn reserve_close(
    db: &AsyncDatabase,
    reservation: CloseReservation,
) -> Result<Reservation> {
    db.writer()
        .call(move |conn| {
            let inserted = conn
                .execute(
                    "INSERT OR IGNORE INTO session_control_actions(session_id,actor,action,idempotency_key,request_hash,result,reason,created_at,request_id) VALUES(?1,?2,'close',?3,?4,'reserved',?5,?6,?7)",
                    params![
                        reservation.session_id,
                        reservation.actor,
                        reservation.idempotency_key,
                        reservation.request_hash,
                        reservation.reason,
                        reservation.created_at,
                        reservation.request_id
                    ],
                )
                .map_err(|error| other(error.into()))?;
            if inserted == 1 {
                return Ok(Reservation::Reserved);
            }
            let request_hash = conn
                .query_row(
                    "SELECT request_hash FROM session_control_actions WHERE session_id=?1 AND idempotency_key=?2",
                    params![reservation.session_id, reservation.idempotency_key],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| other(error.into()))?;
            // The close path read only the hash before FR-141 B2 and decided
            // replay eligibility from it alone. Reporting the result alongside
            // it would be a wider read than the statement it replaces.
            Ok(Reservation::Replayed {
                request_hash,
                result: String::new(),
            })
        })
        .await
        .map_err(flatten_err)
}

/// Reclaims a reservation whose previous attempt ended `failed`.
///
/// Returns whether this caller won it. A `false` means another retry took the
/// row between the read and this update, which the caller reports as a
/// concurrent retry rather than as an error.
pub async fn reclaim_failed_reservation(
    db: &AsyncDatabase,
    session_id: String,
    idempotency_key: String,
    request_id: String,
    created_at: String,
) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            let changed = conn.execute(
                "UPDATE session_control_actions
                         SET result='reserved',request_id=?3,created_at=?4
                         WHERE session_id=?1 AND idempotency_key=?2 AND result='failed'",
                params![session_id, idempotency_key, request_id, created_at],
            )?;
            Ok(changed == 1)
        })
        .await
        .map_err(flatten_err)
}

/// Records that an action failed.
pub async fn record_failed(
    db: &AsyncDatabase,
    session_id: String,
    idempotency_key: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE session_control_actions SET result='failed'
                     WHERE session_id=?1 AND idempotency_key=?2",
                params![session_id, idempotency_key],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Reads the prior `(request_hash, result)` for an idempotency key, if any.
pub async fn read_prior_outcome(
    db: &AsyncDatabase,
    session_id: String,
    idempotency_key: String,
) -> Result<Option<(String, String)>> {
    db.reader()
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT request_hash,result FROM session_control_actions
                         WHERE session_id=?1 AND idempotency_key=?2",
                    params![session_id, idempotency_key],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?)
        })
        .await
        .map_err(flatten_err)
}

/// Records the outcome an action reached.
pub async fn record_result(
    db: &AsyncDatabase,
    session_id: String,
    idempotency_key: String,
    result: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE session_control_actions SET result=?3 WHERE session_id=?1 AND idempotency_key=?2",
                params![session_id, idempotency_key, result],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Records that an action was accepted.
///
/// A separate statement from [`record_result`] because it is the one the daemon
/// issues on a path that ignores failures, and collapsing the two would hide
/// that difference behind an argument.
pub async fn record_accepted(
    db: &AsyncDatabase,
    session_id: String,
    idempotency_key: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE session_control_actions SET result='accepted' WHERE session_id=?1 AND idempotency_key=?2",
                params![session_id, idempotency_key],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Writes one already-terminal control action, bypassing reservation.
#[allow(clippy::too_many_arguments)]
pub async fn insert_terminal(
    db: &AsyncDatabase,
    session_id: String,
    actor: String,
    client_id: Option<String>,
    action: String,
    idempotency_key: Option<String>,
    request_hash: String,
    result: String,
    reason: Option<String>,
    fencing_token: Option<i64>,
    created_at: String,
    request_id: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "INSERT INTO session_control_actions
                 (session_id,actor,client_id,action,idempotency_key,request_hash,result,reason,
                  fencing_token,created_at,request_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    session_id,
                    actor,
                    client_id,
                    action,
                    idempotency_key,
                    request_hash,
                    result,
                    reason,
                    fencing_token,
                    created_at,
                    request_id
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}
