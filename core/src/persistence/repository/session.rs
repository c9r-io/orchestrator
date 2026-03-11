use crate::async_database::{flatten_err, AsyncDatabase};
use crate::session_store::{self, OwnedNewSession, SessionRow};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn insert_session(&self, session: OwnedNewSession) -> Result<()>;
    async fn update_session_state(
        &self,
        session_id: &str,
        state: &str,
        exit_code: Option<i64>,
        ended: bool,
    ) -> Result<()>;
    async fn update_session_pid(&self, session_id: &str, pid: i64) -> Result<()>;
    async fn load_session(&self, session_id: &str) -> Result<Option<SessionRow>>;
    async fn load_active_session_for_task_step(
        &self,
        task_id: &str,
        step_id: &str,
    ) -> Result<Option<SessionRow>>;
    async fn list_task_sessions(&self, task_id: &str) -> Result<Vec<SessionRow>>;
    async fn acquire_writer(&self, session_id: &str, client_id: &str) -> Result<bool>;
    async fn attach_reader(&self, session_id: &str, client_id: &str) -> Result<()>;
    async fn cleanup_stale_sessions(&self, max_age_hours: u64) -> Result<usize>;
    async fn release_attachment(
        &self,
        session_id: &str,
        client_id: &str,
        reason: &str,
    ) -> Result<()>;
}

pub struct SqliteSessionRepository {
    async_db: Arc<AsyncDatabase>,
}

impl SqliteSessionRepository {
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self { async_db }
    }
}

#[async_trait]
impl SessionRepository for SqliteSessionRepository {
    async fn insert_session(&self, session: OwnedNewSession) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                let session = session_store::NewSession {
                    id: &session.id,
                    task_id: &session.task_id,
                    task_item_id: session.task_item_id.as_deref(),
                    step_id: &session.step_id,
                    phase: &session.phase,
                    agent_id: &session.agent_id,
                    state: &session.state,
                    pid: session.pid,
                    pty_backend: &session.pty_backend,
                    cwd: &session.cwd,
                    command: &session.command,
                    input_fifo_path: &session.input_fifo_path,
                    stdout_path: &session.stdout_path,
                    stderr_path: &session.stderr_path,
                    transcript_path: &session.transcript_path,
                    output_json_path: session.output_json_path.as_deref(),
                };
                session_store::insert_session(conn, &session)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn update_session_state(
        &self,
        session_id: &str,
        state: &str,
        exit_code: Option<i64>,
        ended: bool,
    ) -> Result<()> {
        let session_id = session_id.to_owned();
        let state = state.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                session_store::update_session_state(conn, &session_id, &state, exit_code, ended)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn update_session_pid(&self, session_id: &str, pid: i64) -> Result<()> {
        let session_id = session_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                session_store::update_session_pid(conn, &session_id, pid)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn load_session(&self, session_id: &str) -> Result<Option<SessionRow>> {
        let session_id = session_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                session_store::load_session(conn, &session_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn load_active_session_for_task_step(
        &self,
        task_id: &str,
        step_id: &str,
    ) -> Result<Option<SessionRow>> {
        let task_id = task_id.to_owned();
        let step_id = step_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                session_store::load_active_session_for_task_step(conn, &task_id, &step_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn list_task_sessions(&self, task_id: &str) -> Result<Vec<SessionRow>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                session_store::list_task_sessions(conn, &task_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn acquire_writer(&self, session_id: &str, client_id: &str) -> Result<bool> {
        let session_id = session_id.to_owned();
        let client_id = client_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                session_store::acquire_writer(conn, &session_id, &client_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn attach_reader(&self, session_id: &str, client_id: &str) -> Result<()> {
        let session_id = session_id.to_owned();
        let client_id = client_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                session_store::attach_reader(conn, &session_id, &client_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn cleanup_stale_sessions(&self, max_age_hours: u64) -> Result<usize> {
        self.async_db
            .writer()
            .call(move |conn| {
                session_store::cleanup_stale_sessions(conn, max_age_hours)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    async fn release_attachment(
        &self,
        session_id: &str,
        client_id: &str,
        reason: &str,
    ) -> Result<()> {
        let session_id = session_id.to_owned();
        let client_id = client_id.to_owned();
        let reason = reason.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                session_store::release_attachment(conn, &session_id, &client_id, &reason)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }
}
