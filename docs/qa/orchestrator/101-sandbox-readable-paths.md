---
lifecycle: active
related_fr: FR-093
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

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Config field accepts readable_paths | ☐ | | | |
| 2 | Path expansion utility | ☐ | | | |
| 3 | Profile resolution applies expansion + workspace join | ☐ | | | |
| 4 | Linux sandbox bind-mounts readable_paths read-only | ☐ | | | |
| 5 | macOS Seatbelt profile is unchanged for read access | ☐ | | | |

Environment injection, validation, and workspace gates continue in `docs/qa/orchestrator/101b-sandbox-readable-paths-regression.md`.
