//! Crate-wide test helpers for serializing process-environment mutations.
//!
//! Multiple test modules in `orchestrator-runner` mutate the same env vars
//! (`HOME`, `RUNNER_*`, `CLAUDECODE`, …) — they all run in the same test
//! binary and would race against each other under cargo's default
//! parallel-test schedule.  This module provides a single shared
//! [`ENV_LOCK`] (sync flavour) plus an [`EnvGuard`] RAII helper that
//! snapshot/restore env vars on drop, so callers cannot leak mutations
//! to subsequent tests even when the test body panics.
//!
//! For `#[tokio::test]` callers that need to hold the lock across an
//! `await` point, use [`AsyncEnvGuard`] which wraps the same logical
//! lock with `tokio::sync::Mutex` semantics.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};
use tokio::sync::Mutex as TokioMutex;

/// Process-local lock that serializes every test in this crate which
/// mutates or reads connect-related / runtime-related environment
/// variables.  All sync env-touching tests must take this lock via
/// [`EnvGuard::new`] before any `set_var` / `remove_var` call.
pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Async counterpart to [`ENV_LOCK`].  Used by `#[tokio::test]` cases
/// that need to hold the lock across an `await` point — `std::sync::Mutex`
/// would either deadlock or trigger the multi-thread runtime's
/// `holding-MutexGuard-across-await` lint.
///
/// IMPORTANT: this is a *separate* mutex from [`ENV_LOCK`].  Never mix
/// the two — sync and async tests in the same test binary cannot share
/// state safely.  The runner crate currently has exactly one async
/// test that touches env (`runner::tests::test_spawn_with_runner_…`),
/// and the convention is: if you add more, route them all through
/// `ASYNC_ENV_LOCK`, not [`ENV_LOCK`].
pub(crate) static ASYNC_ENV_LOCK: TokioMutex<()> = TokioMutex::const_new(());

/// RAII guard that takes the sync [`ENV_LOCK`], snapshots the listed
/// environment variables on construction, and restores them on drop.
///
/// Mutex poisoning is recovered automatically (`into_inner`) so a single
/// panicking test does not cascade-fail every other env-using test in
/// the binary.
pub(crate) struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    snapshot: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    /// Acquires [`ENV_LOCK`] and records the current value of each var.
    /// On drop the values are written back exactly as they were.
    pub(crate) fn new(vars: &[&'static str]) -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let snapshot = vars
            .iter()
            .map(|&k| (k, std::env::var_os(k)))
            .collect();
        Self {
            _lock: lock,
            snapshot,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: ENV_LOCK ensures no other test in this crate is reading
        // or writing the env while we restore it.
        for (key, value) in &self.snapshot {
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

/// Async counterpart of [`EnvGuard`].  Holds [`ASYNC_ENV_LOCK`] across
/// the test's `await` points and restores env vars on drop.
pub(crate) struct AsyncEnvGuard {
    _lock: tokio::sync::MutexGuard<'static, ()>,
    snapshot: Vec<(&'static str, Option<OsString>)>,
}

impl AsyncEnvGuard {
    /// Acquires [`ASYNC_ENV_LOCK`] and records the current value of each var.
    pub(crate) async fn new(vars: &[&'static str]) -> Self {
        let lock = ASYNC_ENV_LOCK.lock().await;
        let snapshot = vars
            .iter()
            .map(|&k| (k, std::env::var_os(k)))
            .collect();
        Self {
            _lock: lock,
            snapshot,
        }
    }
}

impl Drop for AsyncEnvGuard {
    fn drop(&mut self) {
        // SAFETY: ASYNC_ENV_LOCK is still held; no other async test in
        // this crate can mutate env while we restore it.
        for (key, value) in &self.snapshot {
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}
