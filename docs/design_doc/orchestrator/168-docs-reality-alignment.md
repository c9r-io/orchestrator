---
lifecycle: active
related_fr: FR-155
---

# DD-168: Documentation And Repository Reality Alignment

**Module**: Repository governance and maintainer onboarding
**Status**: Released (FR-155, 2026-08-02)
**Related QA**: `docs/qa/orchestrator/206-docs-reality-alignment.md`
**Created**: 2026-08-02
**Last Updated**: 2026-08-02

## Background

FR-155 found six independent ways in which high-authority documentation had
stopped describing the repository it governed: `AGENTS.md` taught a
deserialize-only Workspace alias and a command-only Agent, infrastructure
skills assumed generated Docker/Kubernetes assets, architecture documentation
collapsed the Web frontend into the Tauri shell and omitted the persistence
crate, active tickets were ignored, the FR and skill inventories were copied
by hand, and an orphan proto plus five orphan YAML files remained at the root.

The planning audit rebuilt every count from `57b02662` before implementation.
That corrected material errors in the request itself: 29 skills rather than
30, 11 non-FR old-proto references rather than ten design documents, 140
historical FR numbers rather than only the missing 55-number range, and two
live references to the supposedly unreferenced YAML directory.

## Goals

- Make onboarding examples executable through the real resource parser,
  validator, warning collector, and apply path.
- State component, persistence, runtime-path, and protobuf ownership exactly.
- Keep all skills discoverable while making generated-project applicability
  explicit and machine-checking their filesystem claims.
- Preserve active QA tickets and derive historical FR and skill inventories.
- Remove duplicate or orphan artifacts only after identifying their behavioral
  replacements.

## Non-goals

- Removing the compatibility parser for `Workspace.spec.root_path`.
- Adding Docker Compose or Kubernetes deployment assets to this repository.
- Implementing a second closed-ticket archive beside git history.
- Rewriting historical FR, design, or QA records merely to normalize style.

## Key Design

### Executable onboarding

`AGENTS.md` now teaches `spec.work_dir` and a typed `shell/cli` driver. Its
Workflow was also corrected after the behavioral test found a previously
unrecorded defect: the example omitted the required step `type` and could not
parse at all. `fixture_corpus_tests::agents_md_manifests_apply_without_legacy_warnings`
extracts every YAML fence, parses and dispatches each resource, validates it,
applies it to a real `OrchestratorConfig`, and rejects any `[legacy_*]` warning.

### Architecture and canonical ownership

`docs/architecture.md` distinguishes the root `gui/` React/Vite frontend from
the `crates/gui/` Tauri shell, names `crates/orchestrator-persistence`, retains
the already-present Slack gateway boundary, derives the registered migration
count as 37, and states the default `~/.orchestratord/` runtime root. The only
protobuf authority is `crates/proto/orchestrator.proto`; the 61-RPC root copy
was removed and all 11 non-FR references now point at the 121-RPC canonical
file.

`scripts/qa/test-docs-reality-alignment.sh`, invoked by the ci-required
`scripts/qa-doc-lint.sh`, derives the migration sequence from Rust, scans the
whole non-FR Markdown surface for the retired proto path, and verifies ticket,
onboarding, and retired-YAML facts. Five isolated mutations prove each check
can fail.

### Skills: existence plus exact scope

`.claude/skills/*/SKILL.md` remains the authority for all 29 skills. Six
infrastructure-heavy skills now lead with applicability: this repository uses
the host daemon, UDS, Rust workspace, `gui/`, and optional Slack gateway;
Docker/Kubernetes branches activate only when the target repository owns the
corresponding generated assets.

`scripts/lib/skill_docs.rb` parses filesystem candidates from inline code and
shell fences. Existing repository and skill-relative paths require no ledger
entry. Generated template paths, future outputs, and the optional
`orchestrator-integrations` companion use exact declarations in
`config/governance/skill-path-scopes.json`. Declarations are equality matches,
wildcards are forbidden, targets are validated, and reverse comparison rejects
declarations whose reference disappeared. The same helper generates
`SKILLS.md` from frontmatter; the mirror gate requires byte equality.

### Tickets and historical FR registry

Active `docs/ticket/*.md` files are tracked. `qa-testing` creates one
immediately on failure; `ticket-fix` deletes it only after the original
scenario passes. The fixing commit and deleted file remain the archive.

`scripts/lib/fr_registry.rb` derives the README registry from the complete
`HEAD` ancestry using case-insensitive FR filenames. Removed requests are
Closed, current files retain their declared status, and the five duplicate
number collisions list every historical filename. Thirteen pre-registry README
entries with no FR-file history are exact reviewed exceptions in
`config/governance/fr-registry-legacy.json`. A shallow repository fails before
rendering.

### Artifact retirement and parity

The root proto had no build consumer; `crates/proto/build.rs` and the canonical
proto build test preserve the production behavior. The five
`test-yaml-warnings/` files had no execution consumer: three cases already
exist in `fixtures/manifests/bundles/qa105-*`, while correct/captured variants
are unit-tested in `workflow_steps.rs`. Their temporary subtree exclusion and
its stale-exclusion test were removed with the directory.

## Alternatives And Tradeoffs

- Keeping a deprecated root proto header would retain two files that can drift;
  deletion makes ownership unambiguous.
- Moving infrastructure skills outside the repository would reduce inventory
  but make generated-project operations undiscoverable. Applicability gates and
  exact template declarations preserve both use cases.
- A generated table from current files alone would lose closed requests. Full
  history is authoritative but deliberately costs several seconds and requires
  `fetch-depth: 0`.
- A `closed/` ticket archive would duplicate git history without a writer or
  reader. Verified deletion has one lifecycle and one source of evidence.

## Risks And Mitigations

- **Path extraction false positives**: candidates are limited to repository and
  skill path roots; every exceptional classification is exact and stale-checked.
- **Self-certifying generators**: negative temp repositories add history without
  regenerating output; the check compares independent git inputs to the file.
- **Historical truncation**: shallow clones fail closed, and CI checks out full
  history.
- **Text checks replacing behavior**: AGENTS uses a Rust behavioral test; proto
  removal is paired with canonical build verification and QA-105 behavior tests.

## Observability

No runtime telemetry changes. CI diagnostics name the missing path,
classification, stale declaration, historical collision, shallow-history
condition, architecture token, or retired artifact. Every new gate prints a
complete pass/fail summary.

## Operations / Release

- No database migration, API change, or runtime configuration change.
- Rollback is a normal commit revert. Restoring the root proto or orphan YAML
  would also require removing the gates that intentionally reject them.
- Compatibility: `root_path` remains accepted by the product, but new docs and
  fixtures teach only `work_dir`; generated-project Docker/Kubernetes skills
  remain available behind explicit applicability checks.

## Test Plan

- Rust behavior: AGENTS parse/validate/apply and full driverless fixture corpus.
- Governance: skill path/registry positive checks plus 28 negative assertions;
  FR registry collision/drift/shallow fixtures; docs reality positive and five
  negative mutations.
- Artifact parity: canonical proto crate tests and existing QA-105 warning tests.
- Closure: `qa-doc-lint`, gate-surface checks, lifecycle index, workspace test,
  Clippy, and formatting on a pinned clean revision.

## QA Docs

- `docs/qa/orchestrator/206-docs-reality-alignment.md`

## Acceptance Criteria

- AGENTS examples apply without legacy warnings.
- Architecture names all four required component paths and the source-derived
  migration count.
- Skill paths and the generated skill inventory are exact and falsifiable.
- Ticket tracking, FR history coverage, collision reporting, and shallow failure
  match their documented semantics.
- The root proto and retired YAML corpus cannot silently return.
