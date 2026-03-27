---
self_referential_safe: true
---

# QA-132: Filesystem Trigger

Validates FR-085: native filesystem change detection as a trigger source, with lazy watcher lifecycle, path safety, and CEL filter integration.

See also: [132b-filesystem-trigger-advanced.md](132b-filesystem-trigger-advanced.md), [132c-filesystem-trigger-regression.md](132c-filesystem-trigger-regression.md)

## Scenario 1: Compilation and tests

**Steps:**
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

**Expected:** All tests pass, no clippy warnings.

## Scenario 2: `source: filesystem` accepted in manifest validation

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: fs-test
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - docs/
      events:
        - create
      debounce_ms: 500
  action:
    workflow: test-wf
    workspace: default
YAML
```

**Expected:** Validation passes (no `event.source` error).

## Scenario 3: filesystem trigger requires filesystem block

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
  action:
    workflow: wf
    workspace: ws
YAML
```

**Expected:** Validation fails with "requires a 'filesystem' configuration block".

## Scenario 4: filesystem trigger rejects empty paths

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
    filesystem:
      paths: []
  action:
    workflow: wf
    workspace: ws
YAML
```

**Expected:** Validation fails with "paths must not be empty".

## Scenario 5: filesystem trigger rejects invalid event types

**Steps:**
```bash
cat <<'YAML' | orchestrator manifest validate -f -
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - src/
      events:
        - rename
  action:
    workflow: wf
    workspace: ws
YAML
```

**Expected:** Validation fails with "filesystem.events must be one of".

## Checklist

- [ ] Scenario 1: Compilation and tests
- [ ] Scenario 2: `source: filesystem` accepted in manifest validation
- [ ] Scenario 3: filesystem trigger requires filesystem block
- [ ] Scenario 4: filesystem trigger rejects empty paths
- [ ] Scenario 5: filesystem trigger rejects invalid event types
