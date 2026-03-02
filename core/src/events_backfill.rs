use anyhow::Result;
use std::path::Path;

pub struct BackfillStats {
    pub scanned: u64,
    pub updated: u64,
    pub skipped: u64,
}

pub fn backfill_event_step_scope(db_path: &Path) -> Result<BackfillStats> {
    let conn = crate::db::open_conn(db_path)?;

    let scanned: u64 = conn.query_row(
        "SELECT COUNT(*) FROM events
         WHERE event_type IN ('step_started','step_finished','step_skipped','step_spawned','step_timeout')
           AND payload_json NOT LIKE '%step_scope%'",
        [],
        |r| r.get(0),
    )?;

    if scanned == 0 {
        return Ok(BackfillStats {
            scanned: 0,
            updated: 0,
            skipped: 0,
        });
    }

    let mut stmt = conn.prepare(
        "SELECT id, task_item_id, payload_json FROM events
         WHERE event_type IN ('step_started','step_finished','step_skipped','step_spawned','step_timeout')
           AND payload_json NOT LIKE '%step_scope%'",
    )?;

    let rows: Vec<(i64, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut updated: u64 = 0;
    let mut skipped: u64 = 0;

    for (id, task_item_id, payload_json) in &rows {
        let mut payload: serde_json::Value = match serde_json::from_str(payload_json) {
            Ok(v) => v,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };

        if payload.get("step_scope").is_some() {
            skipped += 1;
            continue;
        }

        let inferred_scope = if task_item_id.is_some() {
            "item"
        } else {
            "task"
        };

        payload["step_scope"] = serde_json::Value::String(inferred_scope.to_string());

        let new_json = serde_json::to_string(&payload)?;
        conn.execute(
            "UPDATE events SET payload_json = ?1 WHERE id = ?2",
            rusqlite::params![new_json, id],
        )?;
        updated += 1;
    }

    Ok(BackfillStats {
        scanned,
        updated,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::insert_event;
    use serde_json::json;

    #[test]
    fn backfill_returns_zero_stats_on_empty_db() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();
        let stats = backfill_event_step_scope(&state.db_path).expect("backfill empty db");
        assert_eq!(stats.scanned, 0);
        assert_eq!(stats.updated, 0);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn backfill_infers_item_scope_when_task_item_id_present() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item-1"),
            "step_started",
            json!({"step": "qa"}),
        )
        .expect("insert legacy item-scoped event");

        let stats = backfill_event_step_scope(&state.db_path).expect("backfill item scope");
        assert_eq!(stats.scanned, 1);
        assert_eq!(stats.updated, 1);
        assert_eq!(stats.skipped, 0);

        let events =
            crate::events::query_step_events(&state.db_path, "task1").expect("query step events");
        assert_eq!(
            events[0].step_scope,
            Some(crate::events::ObservedStepScope::Item)
        );
    }

    #[test]
    fn backfill_infers_task_scope_when_task_item_id_absent() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            None,
            "step_started",
            json!({"step": "plan"}),
        )
        .expect("insert legacy task-scoped event");

        let stats = backfill_event_step_scope(&state.db_path).expect("backfill task scope");
        assert_eq!(stats.scanned, 1);
        assert_eq!(stats.updated, 1);

        let events =
            crate::events::query_step_events(&state.db_path, "task1").expect("query step events");
        assert_eq!(
            events[0].step_scope,
            Some(crate::events::ObservedStepScope::Task)
        );
    }

    #[test]
    fn backfill_is_idempotent() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item-1"),
            "step_finished",
            json!({"step": "qa", "success": true}),
        )
        .expect("insert finished event");

        let stats1 = backfill_event_step_scope(&state.db_path).expect("first backfill pass");
        assert_eq!(stats1.updated, 1);

        let stats2 = backfill_event_step_scope(&state.db_path).expect("second backfill pass");
        assert_eq!(
            stats2.scanned, 0,
            "second pass should find nothing to backfill"
        );
        assert_eq!(stats2.updated, 0);
    }

    #[test]
    fn backfill_skips_events_already_having_step_scope() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item-1"),
            "step_started",
            json!({"step": "qa", "step_scope": "item"}),
        )
        .expect("insert scoped event");

        let stats = backfill_event_step_scope(&state.db_path).expect("backfill pre-scoped event");
        assert_eq!(stats.scanned, 0);
        assert_eq!(stats.updated, 0);
    }

    #[test]
    fn backfill_does_not_touch_non_step_events() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(&state, "task1", None, "cycle_started", json!({"cycle": 1}))
            .expect("insert cycle_started event");
        insert_event(&state, "task1", None, "task_completed", json!({}))
            .expect("insert task_completed event");

        let stats = backfill_event_step_scope(&state.db_path).expect("backfill non-step events");
        assert_eq!(stats.scanned, 0, "non-step events should not be scanned");
        assert_eq!(stats.updated, 0);
    }

    #[test]
    fn backfill_handles_mixed_events() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        // Legacy event (no step_scope)
        insert_event(
            &state,
            "task1",
            Some("item-1"),
            "step_started",
            json!({"step": "qa"}),
        )
        .expect("insert mixed legacy item event");
        // Modern event (has step_scope)
        insert_event(
            &state,
            "task1",
            Some("item-1"),
            "step_finished",
            json!({"step": "qa", "step_scope": "item", "success": true}),
        )
        .expect("insert mixed modern event");
        // Another legacy event
        insert_event(
            &state,
            "task1",
            None,
            "step_started",
            json!({"step": "plan"}),
        )
        .expect("insert mixed legacy task event");

        let stats = backfill_event_step_scope(&state.db_path).expect("backfill mixed events");
        assert_eq!(stats.scanned, 2);
        assert_eq!(stats.updated, 2);
    }
}
