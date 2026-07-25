---
lifecycle: active
self_referential_safe: true
---

# Orchestrator Runner - Codex Session Resume Conformance

**Module**: Orchestrator Runner / Agent Driver
**Scope**: Codex CLI version pin, resume grammar, real contextual continuity, JSONL normalization, and session privacy
**Scenarios**: 5
**Priority**: Medium

## Background

`codex/cli` session attachment is supported only for a later step in the same daemon lifetime. Default verification replays a sanitized capture without network access. The explicit live certification uses a real authenticated Codex CLI and consumes two short model turns.

## Automated Entry Points

```bash
# Deterministic and offline; safe for normal CI.
./scripts/qa/test-codex-session-resume.sh

# Explicit live certification; requires auth, network, and codex-cli 0.144.5.
./scripts/qa/certify-codex-session-resume.sh
```

---

## Scenario 1: Pinned Resume Command Grammar

### Preconditions

- The repository is available locally.
- Rust dependencies have already been fetched or are available to Cargo.

### Goal

Verify that the provider adapter constructs the exact CLI shape certified against `codex-cli 0.144.5`.

### Steps

1. Run `cargo test -p orchestrator-runner codex_resume_command_matches_certified_cli_grammar`.
2. Inspect `build_codex_command` with and without `session_ref`.
3. Confirm the session ID and prompt are shell-quoted.

### Expected

- A new turn uses `codex exec --json ... -- <PROMPT>`.
- A resumed turn uses `codex exec resume <SESSION_ID> --json ... -- <PROMPT>`.
- Provider flags remain inside `crates/orchestrator-runner/src/driver/providers.rs`.
- No session ID is sourced from a manifest or public DTO.

---

## Scenario 2: Recorded Protocol Replay

### Preconditions

- `fixtures/driver/codex-cli-0.144.5-resume.json` exists.

### Goal

Verify deterministic normalization of the exact provider fields observed during certification.

### Steps

1. Run `./scripts/qa/test-codex-session-resume.sh`.
2. Confirm fixture metadata names provider `codex`, transport `cli`, and version `0.144.5`.
3. Replay `first_events` and `resume_events` through `parse_codex_event`.
4. Inspect normalized session, assistant text, and token counts.

### Expected

- Both streams produce the same `<SESSION_ID>` reference.
- The first assistant event is `ORCH_RESUME_ANCHOR_ALPHA`.
- The resumed assistant event contains `ORCH_RESUME_ANCHOR_BETA:ORCH_RESUME_ANCHOR_ALPHA`.
- `turn.completed` maps input/output tokens to `DriverEvent::Usage`; provider-only cached/reasoning counts are safely ignored.

---

## Scenario 3: Controlled Live Context Resume

### Preconditions

- `codex-cli 0.144.5` is on `PATH`.
- `{source_codex_home}/auth.json` contains valid Codex authentication and has private permissions.
- Network access is available and two short model turns are authorized.

### Goal

Prove actual context inheritance rather than command parsing alone.

### Steps

1. Optionally set `CODEX_RESUME_SOURCE_HOME={source_codex_home}`.
2. Run `./scripts/qa/certify-codex-session-resume.sh`.
3. Observe the four PASS lines and final zero-failure summary.

### Expected

- The script rejects any Codex version other than `0.144.5` by default.
- Initial and resumed processes both exit successfully.
- The resumed stream exposes the same thread ID internally.
- The resumed response correctly returns the first-turn anchor, proving context continuity.
- Both streams have `thread.started,turn.started,item.completed,turn.completed` in order.

---

## Scenario 4: Authentication And Session Privacy

### Preconditions

- The recorded fixture and both QA scripts are present.

### Goal

Verify that live credentials and provider identifiers do not enter repository artifacts or retained temporary state.

### Steps

1. Search the fixture for a UUID-shaped value.
2. Run the recursive session-field redaction unit test.
3. Inspect the live script's temporary-home setup, output policy, and cleanup trap.
4. Run the live script with `CODEX_RESUME_PRINT_FIXTURE=1` and inspect the emitted candidate fixture.

### Expected

- The committed fixture contains `<SESSION_ID>` and stable item placeholders, not live identifiers.
- `thread_id` is replaced with `[REDACTED]` before provider output persistence.
- Raw stdout/stderr and `auth.json` content are never printed.
- The temporary `CODEX_HOME`, session store, scratch workspace, and authentication copy are deleted on success or failure.

---

## Scenario 5: Capability And Repository Regression

### Preconditions

- Scenarios 1-4 pass.

### Goal

Verify that the certified capability remains consistent with the broader driver and repository contracts.

### Steps

1. Run `cargo test --workspace`.
2. Run `cargo clippy --workspace --all-targets -- -D warnings`.
3. Run `cargo fmt --all --check`.
4. Run `./scripts/qa/test-agent-driver-abstraction.sh` from a clean worktree or with the documented local iteration override.
5. Run `./scripts/qa-doc-lint.sh`.

### Expected

- `codex/cli` advertises `session_resume: true` and remains one-shot for live input.
- Existing Claude, shell, scheduler, sandbox, privacy, and capability-gate tests remain green.
- No compiler, Clippy, formatting, QA, or documentation-lint regression remains.

---

## Execution Evidence

- `./scripts/qa/certify-codex-session-resume.sh`: 4 passed, 0 failed against real `codex-cli 0.144.5`.
- `./scripts/qa/test-codex-session-resume.sh`: 3 passed, 0 failed.
- `./scripts/qa/test-agent-driver-abstraction.sh`: 6 passed, 0 failed from a clean worktree.
- `cargo test --workspace --quiet`: all workspace unit, integration, and doctest suites passed; only the documented opt-in tests remained ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, and `./scripts/qa-doc-lint.sh`: passed.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Pinned resume command grammar | PASS | 2026-07-22 | Codex | Exact provider command assertion passes |
| 2 | Recorded protocol replay | PASS | 2026-07-22 | Codex | Sanitized `0.144.5` initial/resume streams replay offline |
| 3 | Controlled live context resume | PASS | 2026-07-22 | Codex | Same thread and prior-turn anchor verified with real CLI |
| 4 | Authentication and session privacy | PASS | 2026-07-22 | Codex | UUID rejection, redaction, private temp home, and cleanup verified |
| 5 | Capability and repository regression | PASS | 2026-07-22 | Codex | Full workspace, strict Clippy, formatting, FR-116 QA, and doc lint gates pass |
