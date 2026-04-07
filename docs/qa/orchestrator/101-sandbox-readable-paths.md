---
self_referential_safe: true
---
# QA: FR-093 Sandbox Configurable Readable Paths

Verifies that ExecutionProfile supports an `readable_paths` field that grants
explicit read-only access to paths outside the workspace, with tilde and env
var expansion.

## Scenario 1: Config field accepts readable_paths

**Steps:**

```bash
rg 'pub readable_paths: Vec<String>' crates/orchestrator-config/src/config/execution_profile.rs
rg 'pub readable_paths: Vec<String>' crates/orchestrator-config/src/cli_types.rs
rg 'pub readable_paths: Vec<PathBuf>' crates/orchestrator-runner/src/runner/profile.rs
```

**Expected result:** Three matches — `ExecutionProfileConfig`, `ExecutionProfileSpec`, and `ResolvedExecutionProfile` all expose the field.

## Scenario 2: Path expansion utility

**Steps:**

```bash
cargo test -p orchestrator-runner runner::path_expand 2>&1 | grep "test result"
```

**Expected result:** All `path_expand` tests pass (tilde, env var, mixed, unset var, no expansion).

## Scenario 3: Profile resolution applies expansion + workspace join

**Steps:**

```bash
cargo test -p orchestrator-runner runner::profile::tests 2>&1 | grep "test result"
```

**Expected result:** All `profile::tests` pass — absolute, relative, tilde, and empty cases.

## Scenario 4: Linux sandbox bind-mounts readable_paths read-only

**Steps:**

```bash
cargo test -p orchestrator-runner --target x86_64-unknown-linux-gnu \
    linux_fs_isolation 2>&1 | grep "test result" || true
# On non-Linux hosts, inspect the source instead:
rg 'remount,ro,bind' crates/orchestrator-runner/src/runner/sandbox_linux.rs
```

**Expected result:** Linux generates `mount --bind {p} {p} && mount -o remount,ro,bind {p} {p}` for each readable path.

## Scenario 5: macOS Seatbelt profile is unchanged for read access

**Steps:**

```bash
rg 'allow file-read\*' crates/orchestrator-runner/src/runner/sandbox_macos.rs
rg 'readable_paths' crates/orchestrator-runner/src/runner/sandbox_macos.rs
```

**Expected result:** macOS unconditionally allows `(allow file-read*)`, so `readable_paths` is intentionally a no-op there. The code includes a comment explaining this and a `let _ = &execution_profile.readable_paths` to suppress unused warnings.

## Scenario 6: ORCHESTRATOR_READABLE_PATHS env var injected

**Steps:**

```bash
rg 'ORCHESTRATOR_READABLE_PATHS' crates/orchestrator-scheduler/src/scheduler/phase_runner/setup.rs
```

**Expected result:** `setup.rs` inserts `ORCHESTRATOR_READABLE_PATHS` (colon-joined) into `resolved_extra_env` when `execution_profile.readable_paths` is non-empty.

## Scenario 7: Validation rejects readable_paths on host profile

**Steps:**

```bash
cargo test -p agent-orchestrator exec_profile_rejects_host_mode_with_readable_paths 2>&1 | tail -5
```

**Expected result:** Test passes — host-mode profile with `readable_paths` is rejected with "sandbox-only fields" error.

## Scenario 8: Full unit test suite

**Steps:**

```bash
cargo test --workspace 2>&1 | grep "test result:" | awk '{p+=$4; f+=$6} END {print "Passed:", p, "Failed:", f}'
```

**Expected result:** All workspace tests pass; 0 failures.

## Scenario 9: Clippy clean

**Steps:**

```bash
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

**Expected result:** `Finished dev profile` with no error/warning output.
