# Design Doc 53: Self-Restart Socket Continuity

**FR**: FR-041
**Status**: Implemented
**Date**: 2026-03-14

## Problem

After a `self_restart` step exec()'s the new daemon binary, the Unix domain socket (`data/orchestrator.sock`) can become unreachable. The root cause was that `lifecycle::cleanup()` removed both the socket and PID file *before* calling `exec()`. If exec() failed or the new binary crashed during startup, the CLI had no socket to connect to and the PID file pointed to a non-existent process.

Additionally, the CLI had no retry logic — a single failed connection attempt resulted in an immediate error, even if the daemon was in the process of restarting and would be available within seconds.

### Observed Symptoms

1. `Connection refused (os error 61)` on CLI commands after self_restart
2. PID file pointing to a dead process while the old daemon was still running
3. Manual kill + restart required to recover

## Solution

### 1. Safer exec() Cleanup (daemon)

**File**: `crates/daemon/src/main.rs`

Changed the pre-exec cleanup to only remove the PID file, not the socket:

```rust
// Before
lifecycle::cleanup(&socket_path, &pid_path);  // removes socket + PID

// After
lifecycle::cleanup_pid_only(&pid_path);  // removes only PID
```

Rationale: The new binary already removes stale sockets at startup (`let _ = std::fs::remove_file(&socket_path)` before `UnixListener::bind`), so pre-emptive socket removal is redundant and harmful. If exec() fails, the socket file persists but is inert — better than no socket file at all.

Added `lifecycle::cleanup_pid_only()` helper in `crates/daemon/src/lifecycle.rs`.

If exec() fails (only returns on error), a "last resort" PID file is written before `process::exit(1)` so that stale PID detection works correctly on the next manual start.

### 2. CLI Connection Retry (client)

**File**: `crates/cli/src/client.rs`

Added retry logic to `connect_uds()`: 3 attempts with 1-second intervals. This tolerates the transient unavailability window while the new daemon binds its socket after exec().

### 3. Socket Ready Event (daemon)

**File**: `crates/daemon/src/main.rs`

Emits a `daemon_socket_ready` event after the UDS listener is successfully bound. This provides observability into the socket lifecycle and can be used for post-restart verification.

## Design Decisions

- **Socket-last cleanup over socket-first cleanup**: Keeping the socket file during exec() is safe because the new binary unconditionally removes it before binding. The alternative (removing socket before exec) creates an irrecoverable state if exec fails.
- **Retry in CLI over health-check in daemon**: CLI-side retry is simpler and covers all transient connection failures, not just restart scenarios. 3 attempts with 1s interval provides a 3-second window — matching the FR's acceptance criterion.
- **No TCP fallback**: The FR suggested TCP as a long-term alternative (方案 C), but this was deferred as it introduces network exposure and the UDS issues are fully addressed by the current changes.
