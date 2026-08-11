//! Filesystem watcher for `source: filesystem` triggers.
//!
//! Lazily creates a `notify` watcher only when active filesystem triggers exist.
//! Zero filesystem triggers = zero overhead (no watcher, no fd, no threads).

use agent_orchestrator::state::InnerState;
use agent_orchestrator::trigger_engine::{TriggerEventPayload, broadcast_task_event};
use chrono::Utc;
use notify::Watcher;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Handle used by the rest of the system to request filesystem watcher reloads.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct FsWatcherHandle {
    pub(crate) reload_tx: mpsc::Sender<()>,
}

#[allow(dead_code)]
impl FsWatcherHandle {
    /// Notify the watcher to reload configuration (sync-safe).
    pub(crate) fn reload_sync(&self) -> bool {
        self.reload_tx.try_send(()).is_ok()
    }
}

/// Run the filesystem watcher loop. Returns when `shutdown_rx` fires.
pub(crate) async fn run_fs_watcher(
    state: Arc<InnerState>,
    mut reload_rx: mpsc::Receiver<()>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    info!("filesystem watcher: started (idle, no active watches)");

    // The actual notify watcher — None when no filesystem triggers exist.
    let mut watcher: Option<notify::RecommendedWatcher> = None;
    // Channel receiving raw notify events from the watcher callback.
    let (notify_tx, mut notify_rx) = mpsc::channel::<notify::Event>(256);
    // Currently watched paths → set of (project, trigger_name).
    let mut watched_paths: HashSet<PathBuf> = HashSet::new();
    // Collected filesystem trigger configs for event-type filtering.
    let mut trigger_configs: Vec<FsTriggerEntry> = Vec::new();

    // Initial config load.
    reload_watches(
        &state,
        &mut watcher,
        &notify_tx,
        &mut watched_paths,
        &mut trigger_configs,
    );

    loop {
        tokio::select! {
            // ── Notify events from the filesystem ─────────────────────
            Some(event) = notify_rx.recv() => {
                debug!(kind = ?event.kind, paths = ?event.paths, "filesystem watcher: raw notify event");
                handle_notify_event(&state, &event, &trigger_configs);
            }

            // ── Config reload request ─────────────────────────────────
            Some(()) = reload_rx.recv() => {
                debug!("filesystem watcher: reloading configuration");
                reload_watches(
                    &state,
                    &mut watcher,
                    &notify_tx,
                    &mut watched_paths,
                    &mut trigger_configs,
                );
            }

            // ── Shutdown ──────────────────────────────────────────────
            _ = shutdown_rx.changed() => {
                info!("filesystem watcher: shutting down");
                break;
            }
        }
    }
}

/// Creates a new `FsWatcherHandle` and the receiver for it.
pub(crate) fn new_handle() -> (FsWatcherHandle, mpsc::Receiver<()>) {
    let (reload_tx, reload_rx) = mpsc::channel(16);
    (FsWatcherHandle { reload_tx }, reload_rx)
}

// ── Internal types ───────────────────────────────────────────────────────────

/// Cached entry for a filesystem trigger's config (used for event-type filtering).
struct FsTriggerEntry {
    /// Allowed event types (empty = all).
    events: HashSet<String>,
    /// Resolved absolute paths being watched.
    paths: Vec<PathBuf>,
    /// Debounce window (informational — debounce is applied globally).
    #[allow(dead_code)]
    debounce_ms: u64,
}

// ── Reload logic ─────────────────────────────────────────────────────────────

/// Why a filesystem-trigger path must not be watched, or `None` to watch it.
///
/// `data_dir` is a parameter rather than an environment read, and that is the
/// whole of the fix. This rule used to consult `ORCHESTRATORD_DATA_DIR`
/// directly, so it did nothing at all in the default deployment — the variable
/// is unset there and the data directory resolves from the home directory
/// instead. A workspace rooted at `$HOME` therefore watched
/// `~/.orchestratord`, and the daemon's own database writes fed the trigger
/// that was watching them. Taking the resolved directory from `InnerState`
/// makes the skip fire in the case it was written for.
fn watch_exclusion(abs_path: &Path, data_dir: &Path) -> Option<&'static str> {
    let path_str = abs_path.to_string_lossy();
    if path_str.contains("/.git/") || path_str.ends_with("/.git") {
        return Some("skipping .git path");
    }
    if abs_path.starts_with(data_dir) {
        return Some("skipping daemon data directory");
    }
    None
}

fn reload_watches(
    state: &InnerState,
    watcher: &mut Option<notify::RecommendedWatcher>,
    notify_tx: &mpsc::Sender<notify::Event>,
    watched_paths: &mut HashSet<PathBuf>,
    trigger_configs: &mut Vec<FsTriggerEntry>,
) {
    let snap = state.config_runtime.load();
    let config = &snap.active_config.config;

    // Collect all filesystem trigger paths.
    let mut desired_paths: HashSet<PathBuf> = HashSet::new();
    let mut new_configs: Vec<FsTriggerEntry> = Vec::new();
    // Minimum debounce across all triggers (for watcher-level debounce).
    let mut min_debounce_ms: u64 = 500;

    for project in config.projects.values() {
        // Resolve workspace root_path for path safety.
        let root_path = project
            .workspaces
            .values()
            .next()
            .map(|ws| PathBuf::from(&ws.root_path))
            .unwrap_or_else(|| PathBuf::from("."));
        let root_path = if root_path.is_absolute() {
            root_path
        } else {
            std::env::current_dir().unwrap_or_default().join(&root_path)
        };

        for trigger in project.triggers.values() {
            if trigger.suspend {
                continue;
            }
            let event = match &trigger.event {
                Some(e) if e.source == "filesystem" => e,
                _ => continue,
            };
            let fs_config = match &event.filesystem {
                Some(fs) => fs,
                None => continue,
            };

            let mut resolved_paths = Vec::new();
            for rel_path in &fs_config.paths {
                let abs_path = root_path.join(rel_path);
                // Safety: must be within root_path.
                if let Ok(canonical_root) = root_path.canonicalize()
                    && let Ok(canonical_path) = abs_path.canonicalize()
                    && !canonical_path.starts_with(&canonical_root)
                {
                    warn!(
                        path = %abs_path.display(),
                        root = %canonical_root.display(),
                        "filesystem trigger path outside root_path, skipping"
                    );
                    continue;
                }
                // Skip .git and the daemon's own data directory.
                if let Some(reason) = watch_exclusion(&abs_path, &state.data_dir) {
                    warn!(path = %abs_path.display(), "{reason}");
                    continue;
                }
                // Canonicalize to resolve symlinks (e.g. /var → /private/var on macOS).
                let canonical = abs_path.canonicalize().unwrap_or(abs_path);
                resolved_paths.push(canonical);
            }

            for p in &resolved_paths {
                desired_paths.insert(p.clone());
            }

            let events: HashSet<String> = fs_config.events.iter().cloned().collect();
            if fs_config.debounce_ms > 0 && fs_config.debounce_ms < min_debounce_ms {
                min_debounce_ms = fs_config.debounce_ms;
            }

            new_configs.push(FsTriggerEntry {
                events,
                paths: resolved_paths,
                debounce_ms: fs_config.debounce_ms,
            });
        }
    }

    *trigger_configs = new_configs;

    // No filesystem triggers → release watcher.
    if desired_paths.is_empty() {
        if watcher.is_some() {
            info!("filesystem watcher: no active filesystem triggers, releasing watcher");
            *watcher = None;
            watched_paths.clear();
        }
        return;
    }

    // Ensure watcher exists.
    if watcher.is_none() {
        let tx = notify_tx.clone();
        match notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    if tx.try_send(event).is_err() {
                        eprintln!("[fs_watcher] event channel full or closed, dropping event");
                    }
                }
                Err(e) => {
                    eprintln!("[fs_watcher] error: {e}");
                }
            },
        ) {
            Ok(w) => {
                info!(
                    count = desired_paths.len(),
                    "filesystem watcher: created (watching {} paths)",
                    desired_paths.len()
                );
                *watcher = Some(w);
            }
            Err(e) => {
                error!(error = %e, "failed to create filesystem watcher");
                return;
            }
        }
    }

    let w = match watcher.as_mut() {
        Some(w) => w,
        None => return,
    };

    // Unwatch removed paths.
    let to_unwatch: Vec<PathBuf> = watched_paths.difference(&desired_paths).cloned().collect();
    for path in &to_unwatch {
        if let Err(e) = w.unwatch(path) {
            debug!(path = %path.display(), error = %e, "failed to unwatch path (may not exist)");
        }
        watched_paths.remove(path);
    }

    // Watch new paths.
    let to_watch: Vec<PathBuf> = desired_paths.difference(watched_paths).cloned().collect();
    for path in &to_watch {
        if !path.exists() {
            warn!(path = %path.display(), "filesystem trigger path does not exist, skipping");
            continue;
        }
        if let Err(e) = w.watch(path, notify::RecursiveMode::NonRecursive) {
            error!(path = %path.display(), error = %e, "failed to watch path");
        } else {
            debug!(path = %path.display(), "watching path");
            watched_paths.insert(path.clone());
        }
    }
}

// ── Event handling ───────────────────────────────────────────────────────────

fn handle_notify_event(
    state: &InnerState,
    event: &notify::Event,
    trigger_configs: &[FsTriggerEntry],
) {
    let event_type_str = match &event.kind {
        notify::EventKind::Create(_) => "create",
        notify::EventKind::Modify(_) => "modify",
        notify::EventKind::Remove(_) => "delete",
        _ => return, // Ignore Access, Other, etc.
    };

    for path in &event.paths {
        // Check if any trigger is interested in this event.
        let any_interested = trigger_configs.iter().any(|tc| {
            // Check path is under one of the trigger's watched directories.
            let path_match = tc.paths.iter().any(|wp| path.starts_with(wp));
            if !path_match {
                return false;
            }
            // Check event type filter (empty = all).
            tc.events.is_empty() || tc.events.contains(event_type_str)
        });

        if !any_interested {
            continue;
        }

        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Skip hidden files (editor temp files, .swp, etc.).
        if filename.starts_with('.') {
            debug!(path = %path.display(), "skipping hidden file event");
            continue;
        }

        let dir = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let payload = serde_json::json!({
            "path": path.to_string_lossy(),
            "filename": filename,
            "dir": dir,
            "event_type": event_type_str,
            "timestamp": Utc::now().to_rfc3339(),
        });

        debug!(
            event_type = event_type_str,
            path = %path.display(),
            "filesystem event → trigger broadcast"
        );

        broadcast_task_event(
            state,
            TriggerEventPayload {
                event_type: "filesystem".to_string(),
                task_id: String::new(),
                payload: Some(payload),
                project: None,
                exclude_trigger: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression FR-163 fixed. `data_dir` here is a path that appears in no
    /// environment variable, which is the point: the previous rule read
    /// `ORCHESTRATORD_DATA_DIR` and so returned `None` for this input — the
    /// default deployment's exact situation, where the variable is unset and the
    /// data directory comes from the home directory instead.
    #[test]
    fn a_path_inside_the_data_dir_is_excluded_without_consulting_the_environment() {
        let data_dir = Path::new("/home/dev/.orchestratord");
        assert_eq!(
            watch_exclusion(Path::new("/home/dev/.orchestratord/logs"), data_dir),
            Some("skipping daemon data directory")
        );
        assert_eq!(
            watch_exclusion(Path::new("/home/dev/.orchestratord"), data_dir),
            Some("skipping daemon data directory")
        );
    }

    /// The over-reach the repair could have introduced. Matching was a string
    /// prefix before, so a sibling whose name merely begins with the data dir's
    /// would have been silently excluded too — the failure that costs nothing
    /// until someone creates the directory. `Path::starts_with` compares whole
    /// components, so it does not.
    #[test]
    fn a_sibling_whose_name_merely_begins_with_the_data_dir_is_still_watched() {
        let data_dir = Path::new("/home/dev/.orchestratord");
        assert_eq!(
            watch_exclusion(Path::new("/home/dev/.orchestratord-backup/src"), data_dir),
            None
        );
        assert_eq!(
            watch_exclusion(Path::new("/home/dev/.orchestratord2"), data_dir),
            None
        );
    }

    #[test]
    fn ordinary_workspace_paths_are_watched() {
        let data_dir = Path::new("/home/dev/.orchestratord");
        assert_eq!(
            watch_exclusion(Path::new("/home/dev/repo/src"), data_dir),
            None
        );
    }

    #[test]
    fn git_paths_are_excluded_anywhere() {
        let data_dir = Path::new("/home/dev/.orchestratord");
        assert_eq!(
            watch_exclusion(Path::new("/home/dev/repo/.git"), data_dir),
            Some("skipping .git path")
        );
        assert_eq!(
            watch_exclusion(Path::new("/home/dev/repo/.git/refs"), data_dir),
            Some("skipping .git path")
        );
    }
}
