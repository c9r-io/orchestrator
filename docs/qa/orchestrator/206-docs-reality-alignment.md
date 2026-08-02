---
lifecycle: active
related_fr: FR-155
self_referential_safe: true
---

# Orchestrator - Documentation And Repository Reality Alignment

**Module**: Repository governance and maintainer onboarding
**Scope**: Executable AGENTS examples, source-derived architecture facts,
skill path scopes and inventory, tracked tickets, complete-history FR registry,
canonical proto ownership, and retired YAML residue
**Scenarios**: 5
**Priority**: High

## Background

FR-155 replaces copied inventories and unaudited documentation claims with
behavioral tests or source-derived checks. Design record:
`docs/design_doc/orchestrator/168-docs-reality-alignment.md`.

**Safety**: all commands are read-only tests over the working tree or temporary
repositories. They start no daemon, modify no database, contact no provider,
and consume no AI credits.

## Scenario 1: AGENTS manifest is executable and architecture facts are derived

### Preconditions

- Run from a full checkout at the revision under test.

### Goal

Prove onboarding behavior through the product and architecture facts through
their source, not through token-presence alone.

### Steps

```bash
cargo test -p agent-orchestrator agents_md_manifests_apply_without_legacy_warnings -- --nocapture
bash scripts/qa/test-docs-reality-alignment.sh
bash scripts/qa/test-docs-reality-alignment.sh --fixture-test
```

### Expected

- The Rust test parses, dispatches, validates, warning-checks, and applies all
  YAML fences in `AGENTS.md`; at least one typed Agent is observed and no
  `[legacy_*]` warning is produced.
- Reality verification prints 5 passed, 0 failed.
- Fixture mode prints 15 passed, 0 failed; mutations of `work_dir`, an AGENTS
  code-span path, migration version continuity, root-Web collapse into
  `crates/gui/`, root proto, ticket ignore state, and retired YAML are each
  isolated to their owning check. A meta assertion proves every registered
  check has at least one negative target, and a synthetic uncovered check proves
  that meta assertion can fail.

## Scenario 2: Skill paths and generated inventory are exact

### Preconditions

- Ruby, jq, git, and tar are available.

### Goal

Verify all authoritative skills remain discoverable and every parsed path is
existent or exactly classified.

### Steps

```bash
bash scripts/qa/test-skill-mirror-integrity.sh
bash scripts/qa/test-skill-mirror-integrity.sh --fixture-test
ruby scripts/lib/skill_docs.rb check-paths .
ruby scripts/lib/skill_docs.rb check-registry .
```

### Expected

- Verification reports 29 skills and 9 passed checks.
- Fixture mode reports 28 passed assertions, including missing path, stale
  declaration, wildcard declaration, and manual `SKILLS.md` drift.
- Both direct helper checks exit zero.

## Scenario 3: FR registry covers complete history and active tickets are tracked

### Preconditions

- The checkout is not shallow.

### Goal

Verify the registry's history boundary, collision evidence, and ticket version
control contract.

### Steps

```bash
ruby scripts/lib/fr_registry.rb check .
bash scripts/qa/test-governance-ledger-tooling.sh
if git check-ignore -v docs/ticket/fr155-qa-probe.md; then exit 1; fi
```

### Expected

- The registry check exits zero and the generated header reports all historical
  IDs and paths from the current `HEAD` ancestry.
- Ledger tooling reports 12 passed assertions; its temp history proves duplicate
  numbers, a newly committed unregistered FR failure, and shallow-clone failure.
- `git check-ignore` prints nothing and exits 1, proving an active ticket would
  be tracked.

## Scenario 4: Canonical proto and QA-105 replacements preserve behavior

### Preconditions

- Rust toolchain and vendored protoc are available.

### Goal

Prove removed root artifacts had no unique build or warning behavior.

### Steps

```bash
test ! -e proto/orchestrator.proto
test ! -d test-yaml-warnings
cargo test -p orchestrator-proto
cargo test -p agent-orchestrator fixture_driverless_tests -- --nocapture
cargo test -p agent-orchestrator collect_step_warnings -- --nocapture
```

### Expected

- Both retired root paths are absent.
- The canonical proto crate builds and tests successfully.
- All driverless-corpus tests pass with no subtree exclusion.
- Existing workflow warning tests pass, retaining the QA-105 behavior formerly
  represented by loose YAML files.

## Scenario 5: Documentation and enforcement surfaces close together

### Preconditions

- Scenarios 1–4 pass.

### Goal

Verify the new gate is reachable through the existing ci-required surface and
all lifecycle/index documentation is synchronized.

### Steps

```bash
bash scripts/qa-doc-lint.sh
bash scripts/qa/test-qa-gate-surface.sh
ruby scripts/qa/doc-lifecycle.rb
git diff --check
```

### Expected

- QA lint invokes the docs reality fixtures and exits zero.
- Gate-surface verification recognizes
  `scripts/qa/test-docs-reality-alignment.sh` as ci-required through
  `scripts/qa-doc-lint.sh`.
- Lifecycle index check and whitespace validation pass.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Executable AGENTS and derived architecture | ✅ | 2026-08-02 | Codex | Post-closure audit remediation: reality gate 5/5 and fixtures 15/15 |
| 2 | Skill paths and inventory | ✅ | 2026-08-02 | Codex | 29 skills, 9 checks, 28 fixture assertions |
| 3 | FR history and tracked tickets | ✅ | 2026-08-02 | Codex | Registry check and 12 ledger assertions passed; ticket probe is tracked |
| 4 | Proto/YAML retirement parity | ✅ | 2026-08-02 | Codex | Canonical proto, driverless corpus, and warning tests passed |
| 5 | Documentation enforcement and lifecycle | ✅ | 2026-08-02 | Codex | QA lint, gate-surface, lifecycle, and whitespace checks passed |
