//! Agent session rows: creation, state transitions, and reader/writer attachment.
//!
//! Moved to `orchestrator-persistence` (FR-130 Phase A) and re-exported here.
//! The tests stay in core: they open connections through `crate::db`, the admin
//! facade, which is core-side until this phase's last commit.

pub use orchestrator_persistence::session_store::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_database::AsyncDatabase;
    use rusqlite::params;
    use std::sync::Arc;
    use crate::db::{init_schema, open_conn};
    use tempfile::TempDir;

    fn make_db() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("sessions.db");
        init_schema(&db_path).expect("init schema");
        (dir, db_path)
    }

    fn make_session<'a>(
        id: &'a str,
        task_id: &'a str,
        step_id: &'a str,
        state: &'a str,
    ) -> NewSession<'a> {
        NewSession {
            id,
            task_id,
            task_item_id: Some("item-1"),
            step_id,
            phase: "qa",
            agent_id: "agent-a",
            state,
            pid: 100,
            pty_backend: "pty",
            cwd: "/tmp",
            command: "echo hi",
            input_fifo_path: "/tmp/in.fifo",
            stdout_path: "/tmp/stdout.log",
            stderr_path: "/tmp/stderr.log",
            transcript_path: "/tmp/transcript.log",
            output_json_path: Some("/tmp/output.json"),
        }
    }

    #[test]
    fn insert_load_and_update_session_lifecycle() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        let session = make_session("sess-1", "task-1", "qa", "active");
        insert_session(&conn, &session).expect("insert session");

        let inserted = load_session(&conn, "sess-1")
            .expect("load session")
            .expect("session should exist");
        assert_eq!(inserted.task_item_id.as_deref(), Some("item-1"));
        assert_eq!(
            inserted.output_json_path.as_deref(),
            Some("/tmp/output.json")
        );
        assert_eq!(inserted.state, "active");
        assert_eq!(inserted.pid, 100);
        assert_eq!(inserted.ended_at, None);
        assert_eq!(inserted.exit_code, None);

        update_session_pid(&conn, "sess-1", 4242).expect("update pid");
        update_session_state(&conn, "sess-1", "detached", Some(7), false).expect("detach session");

        let detached = load_session(&conn, "sess-1")
            .expect("reload session")
            .expect("session should still exist");
        assert_eq!(detached.pid, 4242);
        assert_eq!(detached.state, "detached");
        assert_eq!(detached.exit_code, Some(7));
        assert_eq!(detached.ended_at, None);

        update_session_state(&conn, "sess-1", "exited", None, true).expect("exit session");
        let exited = load_session(&conn, "sess-1")
            .expect("reload exited session")
            .expect("session should still exist");
        assert_eq!(exited.state, "exited");
        assert_eq!(exited.exit_code, Some(7));
        assert!(exited.ended_at.is_some());

        assert!(
            load_session(&conn, "missing")
                .expect("load missing session")
                .is_none()
        );
    }

    #[test]
    fn active_session_lookup_and_listing_filter_by_task() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-old", "task-1", "qa", "exited"))
            .expect("insert exited session");
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_session(
            &conn,
            &make_session("sess-active", "task-1", "qa", "active"),
        )
        .expect("insert active session");
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_session(
            &conn,
            &make_session("sess-detached", "task-1", "qa", "detached"),
        )
        .expect("insert detached session");
        insert_session(&conn, &make_session("sess-other", "task-2", "qa", "active"))
            .expect("insert other task session");

        let active = load_active_session_for_task_step(&conn, "task-1", "qa")
            .expect("query active session")
            .expect("task should have an active session");
        assert_eq!(active.id, "sess-detached");
        assert_eq!(active.state, "detached");

        let task_1_sessions = list_task_sessions(&conn, "task-1").expect("list sessions");
        let task_1_ids: Vec<&str> = task_1_sessions.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(task_1_ids.len(), 3);
        assert!(task_1_ids.contains(&"sess-old"));
        assert!(task_1_ids.contains(&"sess-active"));
        assert!(task_1_ids.contains(&"sess-detached"));

        assert!(
            load_active_session_for_task_step(&conn, "task-1", "missing-step")
                .expect("query missing step")
                .is_none()
        );
    }

    #[test]
    fn cleanup_stale_sessions_removes_old_exited_keeps_recent() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");

        // Insert an "exited" session and manually backdate updated_at
        insert_session(&conn, &make_session("old-exited", "task-1", "qa", "exited"))
            .expect("insert old exited");
        let old_ts = (chrono::Utc::now() - chrono::Duration::hours(100)).to_rfc3339();
        conn.execute(
            "UPDATE agent_sessions SET updated_at = ?2 WHERE id = ?1",
            params!["old-exited", old_ts],
        )
        .expect("backdate old session");

        // Insert an "active" session that is also old — should NOT be deleted
        insert_session(&conn, &make_session("old-active", "task-1", "qa", "active"))
            .expect("insert old active");
        conn.execute(
            "UPDATE agent_sessions SET updated_at = ?2 WHERE id = ?1",
            params!["old-active", old_ts],
        )
        .expect("backdate active session");

        // Insert a recent "exited" session — should NOT be deleted
        insert_session(&conn, &make_session("new-exited", "task-1", "qa", "exited"))
            .expect("insert new exited");

        let deleted = cleanup_stale_sessions(&conn, 72).expect("cleanup");
        assert_eq!(deleted, 1);

        // Verify correct session was deleted
        assert!(load_session(&conn, "old-exited").expect("load").is_none());
        assert!(load_session(&conn, "old-active").expect("load").is_some());
        assert!(load_session(&conn, "new-exited").expect("load").is_some());
    }

    #[test]
    fn writer_and_reader_attachments_round_trip() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-1", "task-1", "qa", "active"))
            .expect("insert session");

        assert!(acquire_writer(&conn, "sess-1", "writer-1").expect("acquire initial writer"));
        assert!(acquire_writer(&conn, "sess-1", "writer-1").expect("re-acquire same writer"));
        assert!(!acquire_writer(&conn, "sess-1", "writer-2").expect("reject second writer"));

        attach_reader(&conn, "sess-1", "reader-1").expect("attach reader");
        release_attachment(&conn, "sess-1", "reader-1", "done").expect("detach reader");
        release_attachment(&conn, "sess-1", "writer-1", "handoff").expect("detach writer");

        let session = load_session(&conn, "sess-1")
            .expect("reload session")
            .expect("session should exist");
        assert_eq!(session.writer_client_id, None);

        let writer_attachments: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_attachments WHERE session_id = ?1 AND mode = 'writer'",
                params!["sess-1"],
                |row| row.get(0),
            )
            .expect("count writer attachments");
        let detached_attachments: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_attachments WHERE session_id = ?1 AND detached_at IS NOT NULL",
                params!["sess-1"],
                |row| row.get(0),
            )
            .expect("count detached attachments");

        assert_eq!(writer_attachments, 2);
        assert_eq!(detached_attachments, 3);
    }

    #[test]
    fn writer_lease_fencing_rejects_stale_tokens() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-fence", "task-1", "qa", "active"))
            .expect("insert session");

        let first = acquire_writer_lease(&conn, "sess-fence", "actor-a", "client-a", 30)
            .expect("acquire first")
            .expect("first lease");
        assert!(validate_writer(&conn, "sess-fence", "client-a", first.fencing_token).unwrap());
        let renewed = heartbeat_writer(&conn, "sess-fence", "client-a", first.fencing_token, 60)
            .expect("heartbeat")
            .expect("current writer renews");
        assert!(renewed > first.expires_at);
        assert!(
            heartbeat_writer(&conn, "sess-fence", "client-b", first.fencing_token, 60,)
                .unwrap()
                .is_none()
        );
        assert!(
            release_writer(
                &conn,
                "sess-fence",
                "client-a",
                first.fencing_token,
                "handoff"
            )
            .unwrap()
        );

        let second = acquire_writer_lease(&conn, "sess-fence", "actor-b", "client-b", 30)
            .expect("acquire second")
            .expect("second lease");
        assert!(second.fencing_token > first.fencing_token);
        assert!(!validate_writer(&conn, "sess-fence", "client-a", first.fencing_token).unwrap());
        assert!(
            !release_writer(
                &conn,
                "sess-fence",
                "client-a",
                first.fencing_token,
                "stale"
            )
            .unwrap()
        );
        assert!(validate_writer(&conn, "sess-fence", "client-b", second.fencing_token).unwrap());
    }

    #[test]
    fn concurrent_writer_race_grants_exactly_one_client() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-race", "task-1", "qa", "active"))
            .expect("insert session");
        drop(conn);

        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for client in ["client-a", "client-b"] {
            let path = db_path.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let conn = open_conn(&path).expect("open racing connection");
                barrier.wait();
                acquire_writer_lease(&conn, "sess-race", client, client, 30)
                    .expect("race acquisition")
                    .is_some()
            }));
        }
        barrier.wait();
        let granted = handles
            .into_iter()
            .map(|handle| handle.join().expect("writer thread"))
            .filter(|granted| *granted)
            .count();
        assert_eq!(granted, 1);
    }

    #[test]
    fn process_fingerprint_changes_authority_from_pid_to_identity() {
        let pid = std::process::id();
        let fingerprint = capture_process_fingerprint(pid).expect("current process fingerprint");
        assert!(fingerprint.starts_with(&format!("{pid}:")));
        assert_eq!(
            capture_process_fingerprint(pid).as_deref(),
            Some(fingerprint.as_str())
        );
        assert!(capture_process_fingerprint(u32::MAX).is_none());
        assert_eq!(
            process_identity_status(pid as i64, Some(&fingerprint)),
            ProcessIdentityStatus::VerifiedLive
        );
        assert_eq!(
            process_identity_status(pid as i64, Some("stale-fingerprint")),
            ProcessIdentityStatus::Mismatch
        );
        assert_eq!(
            process_identity_status(pid as i64, None),
            ProcessIdentityStatus::Unsupported
        );
        assert_eq!(
            process_identity_status(u32::MAX as i64, Some("missing")),
            ProcessIdentityStatus::Dead
        );
    }

    #[test]
    fn reader_limit_and_expired_writer_are_bounded() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(
            &conn,
            &make_session("sess-bounds", "task-1", "qa", "active"),
        )
        .expect("insert session");
        for index in 0..8 {
            attach_reader(&conn, "sess-bounds", &format!("reader-{index}"))
                .expect("reader within bound");
        }
        attach_reader(&conn, "sess-bounds", "reader-0").expect("same reader is idempotent");
        assert!(attach_reader(&conn, "sess-bounds", "reader-9").is_err());
        let active_readers: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_attachments
                 WHERE session_id='sess-bounds' AND mode='reader' AND detached_at IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("count active readers");
        assert_eq!(active_readers, 8);

        let lease = acquire_writer_lease(&conn, "sess-bounds", "actor", "writer", 30)
            .expect("acquire writer")
            .expect("writer lease");
        conn.execute(
            "UPDATE agent_sessions SET writer_lease_expires_at='1970-01-01T00:00:00Z' WHERE id='sess-bounds'",
            [],
        )
        .expect("expire lease");
        assert_eq!(expire_writer_leases(&conn).unwrap(), vec!["sess-bounds"]);
        assert!(!validate_writer(&conn, "sess-bounds", "writer", lease.fencing_token).unwrap());
    }

    #[test]
    fn expired_writer_cleanup_never_resurrects_terminal_session() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(
            &conn,
            &make_session("sess-terminal", "task-1", "qa", "closed"),
        )
        .expect("insert terminal session");
        conn.execute(
            "UPDATE agent_sessions
             SET writer_client_id='old-writer', writer_actor='operator',
                 writer_lease_expires_at='1970-01-01T00:00:00Z'
             WHERE id='sess-terminal'",
            [],
        )
        .expect("seed expired terminal lease");

        assert_eq!(expire_writer_leases(&conn).unwrap(), vec!["sess-terminal"]);
        let row = load_session(&conn, "sess-terminal")
            .expect("load session")
            .expect("session exists");
        assert_eq!(row.state, "closed");
        assert!(row.writer_client_id.is_none());
    }

    #[test]
    fn reconciliation_distinguishes_dead_process_from_live_identity_mismatch() {
        let (dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        let transport = dir.path().join("input.fifo");
        let evidence = dir.path().join("transcript.log");
        std::fs::write(&transport, "transport").expect("create transport marker");
        std::fs::write(&evidence, "evidence").expect("create transcript evidence");

        insert_session(
            &conn,
            &make_session("sess-mismatch", "task-1", "qa", "active"),
        )
        .expect("insert mismatch session");
        conn.execute(
            "UPDATE agent_sessions
             SET pid=?2, process_fingerprint='stale-fingerprint',
                 input_fifo_path=?3, transcript_path=?4, stdout_path=?4
             WHERE id=?1",
            params![
                "sess-mismatch",
                std::process::id() as i64,
                transport.to_string_lossy(),
                evidence.to_string_lossy()
            ],
        )
        .expect("seed live mismatch");

        insert_session(&conn, &make_session("sess-dead", "task-1", "qa", "active"))
            .expect("insert dead session");
        conn.execute(
            "UPDATE agent_sessions
             SET pid=?2, process_fingerprint='missing', transcript_path=?3, stdout_path=?3
             WHERE id=?1",
            params!["sess-dead", u32::MAX as i64, evidence.to_string_lossy()],
        )
        .expect("seed dead session");

        let changes = reconcile_sessions(&conn).expect("reconcile sessions");
        assert!(changes.contains(&("sess-mismatch".into(), "failed".into())));
        assert!(changes.contains(&("sess-dead".into(), "closed".into())));
        assert_eq!(
            load_session(&conn, "sess-mismatch").unwrap().unwrap().state,
            "failed"
        );
        assert_eq!(
            load_session(&conn, "sess-dead").unwrap().unwrap().state,
            "closed"
        );
    }

    #[tokio::test]
    async fn async_session_store_exercises_all_wrapper_methods() {
        let (_dir, db_path) = make_db();
        let async_db = Arc::new(AsyncDatabase::open(&db_path).await.expect("open async db"));
        let store = AsyncSessionStore::new(async_db);

        let session = make_session("sess-async", "task-1", "qa", "active");
        store
            .insert_session(OwnedNewSession::from(&session))
            .await
            .expect("insert session");

        let loaded = store
            .load_session("sess-async")
            .await
            .expect("load session")
            .expect("session exists");
        assert_eq!(loaded.id, "sess-async");
        assert_eq!(loaded.state, "active");

        let active = store
            .load_active_session_for_task_step("task-1", "qa")
            .await
            .expect("load active session")
            .expect("active session exists");
        assert_eq!(active.id, "sess-async");

        let listed = store
            .list_task_sessions("task-1")
            .await
            .expect("list sessions");
        assert_eq!(listed.len(), 1);

        assert!(
            store
                .acquire_writer("sess-async", "writer-1")
                .await
                .expect("acquire writer")
        );
        assert!(
            !store
                .acquire_writer("sess-async", "writer-2")
                .await
                .expect("reject second writer")
        );

        store
            .attach_reader("sess-async", "reader-1")
            .await
            .expect("attach reader");
        store
            .update_session_pid("sess-async", 5150)
            .await
            .expect("update pid");
        store
            .update_session_state("sess-async", "failed", Some(9), true)
            .await
            .expect("update session state");
        store
            .release_attachment("sess-async", "reader-1", "done")
            .await
            .expect("release reader");
        store
            .release_attachment("sess-async", "writer-1", "done")
            .await
            .expect("release writer");

        let exited = store
            .load_session("sess-async")
            .await
            .expect("reload exited session")
            .expect("session still exists");
        assert_eq!(exited.pid, 5150);
        assert_eq!(exited.state, "failed");
        assert_eq!(exited.exit_code, Some(9));
        assert!(exited.ended_at.is_some());
        assert!(exited.writer_client_id.is_none());

        let conn = open_conn(&db_path).expect("open sync conn");
        let old_ts = (chrono::Utc::now() - chrono::Duration::hours(100)).to_rfc3339();
        conn.execute(
            "UPDATE agent_sessions SET updated_at = ?2 WHERE id = ?1",
            params!["sess-async", old_ts],
        )
        .expect("backdate session");

        let deleted = store
            .cleanup_stale_sessions(72)
            .await
            .expect("cleanup stale sessions");
        assert_eq!(deleted, 1);
        assert!(
            store
                .load_session("sess-async")
                .await
                .expect("load deleted session")
                .is_none()
        );
        assert!(
            store
                .load_session("missing")
                .await
                .expect("load missing session")
                .is_none()
        );
    }
}
