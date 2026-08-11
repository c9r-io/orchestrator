//! Single-source derivation of the daemon runtime layout.
//!
//! Every path under the data directory — the socket, the pidfile, the database,
//! the control-plane directory — is spelled exactly once, here. This module
//! exists because it was not: FR-163 measured four independent derivations of
//! the data directory and a second spelling of `orchestrator.sock`,
//! `agent_orchestrator.db` and `daemon.pid` each, split across crates that
//! cannot see one another. The copies did not merely risk disagreeing; two of
//! them already did (see [`data_dir_from_db_path`] and the `fs_watcher` skip
//! that only fired when the environment variable was set).
//!
//! It lives in `orchestrator-config` rather than in `core` because the client
//! crate needs it too and does not depend on `core`. `core::config_load` re-
//! exports [`data_dir`] for the call sites that already spell it that way —
//! the same arrangement `now_ts` has, and for the same reason.
//!
//! The layout is **flat**: the database, the socket and the pidfile are direct
//! children of the data directory. Nothing nests them under a `data/`
//! subdirectory. A caller is free to *name* its data directory `data`, and that
//! is a name, not a layout — a distinction that cost a QA gate its secret key
//! before it was written down.

use std::path::{Path, PathBuf};

/// Environment variable overriding the data directory.
pub const DATA_DIR_ENV: &str = "ORCHESTRATORD_DATA_DIR";

/// Default data directory name, relative to the user's home.
pub const DATA_DIR_NAME: &str = ".orchestratord";

/// File name of the daemon Unix Domain Socket.
pub const SOCKET_FILE_NAME: &str = "orchestrator.sock";

/// File name of the daemon PID file.
pub const PID_FILE_NAME: &str = "daemon.pid";

/// File name of the runtime SQLite database.
pub const DB_FILE_NAME: &str = "agent_orchestrator.db";

/// Directory name holding control-plane material, relative to the data dir.
pub const CONTROL_PLANE_DIR_NAME: &str = "control-plane";

/// Returns the daemon data directory (`~/.orchestratord` by default).
///
/// Override with the `ORCHESTRATORD_DATA_DIR` environment variable. When the
/// home directory cannot be determined the fallback is the same name relative
/// to the current working directory — a CWD-dependent path, which is why every
/// diagnostic that reports a missing socket prints the resolved path rather
/// than the expected one.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    match dirs::home_dir() {
        Some(home) => home.join(DATA_DIR_NAME),
        None => PathBuf::from(DATA_DIR_NAME),
    }
}

/// Returns the path to the daemon Unix Domain Socket.
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SOCKET_FILE_NAME)
}

/// Returns the path to the daemon PID file.
pub fn pid_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PID_FILE_NAME)
}

/// Returns the path to the runtime SQLite database.
pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DB_FILE_NAME)
}

/// Returns the data directory containing `db_path` — the inverse of [`db_path`].
///
/// Exact inverse, not an inference: because [`db_path`] places the database as a
/// direct child, the parent *is* the data directory. This function used to guess
/// instead, treating a parent named `data` as a nested layout and returning its
/// grandparent; a QA gate that pointed `ORCHESTRATORD_DATA_DIR` at a directory
/// called `data` then had its key seeded in one place and read from another, so
/// SecretStore writes reported "no active encryption key" while `secret key
/// list` reported one active. The guess was patched with an environment
/// override before it was removed; both are gone now, and the round-trip
/// property in this module's tests is what keeps them gone.
///
/// Returns `None` only for a path with no parent component.
pub fn data_dir_from_db_path(db_path: &Path) -> Option<&Path> {
    db_path.parent()
}

/// Returns the control-plane directory, honouring an explicit override.
pub fn control_plane_dir(data_dir: &Path, override_dir: Option<&Path>) -> PathBuf {
    match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => data_dir.join(CONTROL_PLANE_DIR_NAME),
    }
}

/// Directory holding the *client's* control-plane bundle, relative to `$HOME`.
///
/// Note the name: `.orchestrator`, not the daemon's `.orchestratord`. It is a
/// user-level directory rather than daemon state, because a client may talk to
/// a daemon whose data directory is on another host entirely.
pub const CLIENT_DIR_NAME: &str = ".orchestrator";

/// Returns the directory the daemon writes a local user's client bundle into.
pub fn client_control_plane_dir(home: &Path) -> PathBuf {
    home.join(CLIENT_DIR_NAME).join(CONTROL_PLANE_DIR_NAME)
}

/// Returns the client control-plane config the daemon generates for local use.
///
/// The writer and the auto-discovery reader used to spell this differently —
/// the daemon wrote `~/.orchestrator/control-plane/config.yaml` and the client
/// looked in `~/.orchestratord/control-plane/config.yaml`, one character apart.
/// Transport discovery's step 4 therefore never once fired on the daemon's own
/// output, and every QA gate that needed TLS set
/// `ORCHESTRATOR_CONTROL_PLANE_CONFIG` by hand to route around it. Both sides
/// call this now (FR-163).
pub fn client_control_plane_config(home: &Path) -> PathBuf {
    client_control_plane_dir(home).join("config.yaml")
}

/// The location auto-discovery *documented* before FR-163, still accepted.
///
/// Nothing has ever written here — it was the reader's own invention — so this
/// exists only for an operator who placed a config by hand after reading the
/// old `connect()` documentation. New material goes to
/// [`client_control_plane_config`].
pub fn legacy_client_control_plane_config(home: &Path) -> PathBuf {
    home.join(DATA_DIR_NAME)
        .join(CONTROL_PLANE_DIR_NAME)
        .join("config.yaml")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property that replaces the retired layout heuristic. The `data`-named
    /// directory is the case that used to fail: the old resolver returned the
    /// grandparent for it, so this assertion is red on the pre-FR-163 code and
    /// green after. `data/data` is the same trap one level deeper, and the
    /// single-component case pins that a relative data dir round-trips too.
    #[test]
    fn db_path_and_data_dir_from_db_path_round_trip() {
        for dir in [
            "/srv/orchestratord",
            "/srv/data",
            "/srv/data/data",
            "/data",
            ".orchestratord",
        ] {
            let data_dir = Path::new(dir);
            let db = db_path(data_dir);
            assert_eq!(
                data_dir_from_db_path(&db),
                Some(data_dir),
                "round trip failed for data dir {dir}"
            );
        }
    }

    /// A directory literally named `data` is a name, not a layout. Stated
    /// separately from the round-trip loop because this is the regression the
    /// heuristic caused, and a failure here should name itself.
    #[test]
    fn a_data_dir_named_data_is_not_treated_as_a_nested_layout() {
        let data_dir = Path::new("/srv/qa-root/data");
        let db = db_path(data_dir);
        assert_eq!(db, Path::new("/srv/qa-root/data/agent_orchestrator.db"));
        assert_eq!(
            data_dir_from_db_path(&db),
            Some(data_dir),
            "the parent is the data dir; the old heuristic returned /srv/qa-root"
        );
    }

    #[test]
    fn runtime_files_are_direct_children_of_the_data_dir() {
        let data_dir = Path::new("/srv/orchestratord");
        assert_eq!(
            socket_path(data_dir),
            Path::new("/srv/orchestratord/orchestrator.sock")
        );
        assert_eq!(
            pid_path(data_dir),
            Path::new("/srv/orchestratord/daemon.pid")
        );
        assert_eq!(
            db_path(data_dir),
            Path::new("/srv/orchestratord/agent_orchestrator.db")
        );
    }

    #[test]
    fn control_plane_dir_prefers_an_explicit_override() {
        let data_dir = Path::new("/srv/orchestratord");
        assert_eq!(
            control_plane_dir(data_dir, None),
            Path::new("/srv/orchestratord/control-plane")
        );
        assert_eq!(
            control_plane_dir(data_dir, Some(Path::new("/etc/cp"))),
            Path::new("/etc/cp")
        );
    }

    /// The disagreement FR-163 found, pinned so it cannot recur silently. The
    /// daemon's bundle directory and the client's auto-discovery target must be
    /// the same place; they differed by one character (`.orchestrator` versus
    /// `.orchestratord`) and nothing in the tree compared them, so transport
    /// discovery's step 4 never fired on the daemon's own output.
    #[test]
    fn the_client_bundle_is_written_where_auto_discovery_looks() {
        let home = Path::new("/home/dev");
        assert_eq!(
            client_control_plane_config(home),
            client_control_plane_dir(home).join("config.yaml"),
            "the config must sit inside the bundle directory"
        );
        assert_eq!(
            client_control_plane_config(home),
            Path::new("/home/dev/.orchestrator/control-plane/config.yaml")
        );
    }

    /// The two locations must stay distinct, or accepting both is meaningless
    /// and the compatibility branch silently tests nothing.
    #[test]
    fn the_legacy_discovery_path_is_a_different_place() {
        let home = Path::new("/home/dev");
        assert_ne!(
            client_control_plane_config(home),
            legacy_client_control_plane_config(home)
        );
        assert_eq!(
            legacy_client_control_plane_config(home),
            Path::new("/home/dev/.orchestratord/control-plane/config.yaml")
        );
    }

    #[test]
    fn data_dir_from_db_path_has_no_parent_for_a_bare_file_name() {
        assert_eq!(
            data_dir_from_db_path(Path::new("agent_orchestrator.db")),
            Some(Path::new(""))
        );
        assert_eq!(data_dir_from_db_path(Path::new("/")), None);
    }
}
