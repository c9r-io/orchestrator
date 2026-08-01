---
lifecycle: active
related_fr: FR-152
---

# DD-163: First-Run Path Modernization

**Status**: Released
**Source**: FR-152 (closed 2026-08-01), from the 2026-08-01 technical-debt
audit at `9bcfaa96`; implemented at `7d3abb8f`..HEAD.

## Problem

Every step of a new user's first run hit a broken reference or a deprecation
warning: the README applied a `manifest.yaml` that exists nowhere in the
repository; both quickstart guides taught the driverless Agent form that
apply warns about (`[legacy_agent_command_deprecated]`); 137 of 147 fixture
Agent documents modeled that same deprecated form; the bracketed machine
error codes concentrated on the first-run path had no queryable entry point;
and `install.sh` unpacked the skills tarball into whatever directory
`curl | sh` happened to run from.

## Decisions

### Quickstart manifest is a corpus bundle

`fixtures/manifests/bundles/quickstart.yaml` is the file README.md and both
quickstart guides reference (as real markdown links, so the link-integrity
gate guards the paths). Landing it under `bundles/` places it in
`fixture_corpus_tests`' derived scope — "the quickstart manifest still
applies" is a permanent Rust-test invariant with no new gate registration —
and a second test collects warnings the way the apply path does
(`collect_warnings` per dispatched resource) and asserts the first apply
prints none. Corpus validity alone cannot see warnings; grepping the YAML
for `driver:` would assert spelling, not behavior.

### Fixture corpus migration and the driverless ratchet

A document-aware one-off (split on `---`, YAML-probe `kind`/`driver`, insert
the minimal `driver: {provider: shell, transport: cli}` after the `spec:`
line) migrated 118 documents. String-matching `command:` was rejected
explicitly: workflow step-level `command` occurs 52 times in the corpus and
must not be touched. The 14 top-level `fixtures/*.yaml` — 13 referenced
nowhere, 7 byte-identical to same-named `bundles/` files — were deleted, not
migrated; CONTRIBUTING.md now points at the bundles copy.

Seven documents stay driverless because live gates assert the legacy warning
on them (production-parity counts **exactly 3**; abstraction and strangler
assert theirs by name; the DD-137 baseline must stay rejectable). Each
carries the new machine-parseable per-document comment:

```yaml
---
# fixture-driverless-exempt: <which gate asserts what on this document>
```

`core/src/fixture_driverless_tests.rs` enforces the ratchet: scope derived
from `git ls-files '*.yaml' '*.yml'` (empty scan fails; the
`test-yaml-warnings/` exclusion fails as stale the day FR-155 deletes the
directory), pure evaluator, violations for driverless-without-exemption,
empty exemption reasons, and — in the reverse direction — a typed document
still carrying the exempt comment. The negative fixture comments out (never
deletes) the driver block of a victim derived from the real corpus, and
asserts the premise ("the mutation removed the driver") before asserting the
verdict, per §4.4 shape 7.

Migration parity evidence (§4.3): the five consuming gates were run before
and after the migration commit at pinned revisions (`6ca3822f` baseline,
`d09648ec` after); PASS lines are byte-identical — abstraction 8/8, parity
11/11, collapse 13/13, strangler 20/20, non-code 7/7 — and
`cargo test --workspace` is green on both sides.

### Error-code glossary is derived, in both directions

`docs/guide/error-codes.md` (+ ZH mirror) documents all 16 bracketed codes.
The FR's own inventory claimed 10 codes and zero guide coverage; both claims
were false (§4.4 shape 4: a literal-bracket grep cannot see the seven
`driver_error("[{code}]")` requirement codes, and agent-driver-model.md
already carried a six-code fix table). Hence the gate
(`scripts/qa/test-error-code-glossary.sh`, invoked from qa-doc-lint,
ci-required) derives the set by three anchored rules — string-opening
bracket literals, `driver_error()` first arguments, interpolated
SCREAMING_CASE consts resolved to their declarations — and asserts set
equality doc↔source plus ZH == EN. `fs_watcher` is excluded with a written
reason and a staleness assertion. In/out calls: `empty_change_check` is in
(it reaches users through `task logs` after an implement step);
`FILE_SHARING_GLOBAL_SKILL_UNTRUSTED` is in (apply-time authorization
refusal); `fs_watcher` is out (daemon stderr log label, no remediation
semantics).

The CLI side: apply output appends one hint line naming
`orchestrator guide error-codes` when a warning/error carries a bracketed
code (diagnostics always do), and `guide` gains an `error-codes` entry. The
code list itself lives only in the doc — embedding it in Rust would have
required a second parity gate.

### install.sh writes only an announced target

Skills land in `${INSTALL_ORCHESTRATOR_SKILLS_DIR:-$HOME/.claude/skills}`
(`none` skips), announced before unpacking; the tarball unpacks into the
temp dir and is copied out. A `--skills-dir` flag was rejected: `curl | sh`
cannot receive flags, and env-var configuration is the file's existing
convention. Two behavioral checks in `test-release-publish-surface.sh` run
the real script end to end against a stubbed local release and compare the
CWD entry listing before and after; the negative fixture mutates the default
back to `.` — the one-token regression — and requires the diagnostic to name
the pollution.

## Known limits and recorded findings

- **`scripts/lib/rust_source.rb`'s basename filter `/test.*\.rs\z/` swallows
  the production module `crates/orchestrator-scheduler/src/scheduler/safety/self_test.rs`**
  (unconditional `mod self_test;`), and with it the `[empty_change_check]`
  emission — found while deriving the glossary set, when rule A silently
  returned 15 codes instead of 16. The glossary gate therefore uses an
  anchored exclusion (`tests.rs`, `*_tests.rs`) with its own scope. The
  ledgers built on `rust_source_files` under-scan the same file; their
  numbers are self-consistent (both sides of each ratchet use the same
  scope), so this FR records the finding rather than moving reviewed ledger
  states. A future FR that touches the ledger tooling should widen the
  filter deliberately.
- **`scripts/qa/fixtures/coordination-governance-cases.json` embeds a
  driverless Agent document inline** (case
  `new-command-only-agent-is-rejected`, which asserts the promotion
  behavior). It is JSON, outside the driverless gate's `*.yaml` scope, and
  deliberately legacy — the yaml gate does not see it and does not need to.
- **The Homebrew install path ships no skills**; only install.sh does. The
  two install paths disagreed before this FR and still do — recorded here so
  the asymmetry is a decision, not an oversight. Unifying them belongs to a
  packaging FR.
- **The quickstart's zero-warning guarantee is asserted at the manifest
  layer** (parse + dispatch + `collect_warnings`), not by a daemon-spawning
  gate. The one recorded end-to-end run lives in QA doc 201; a permanent
  daemon-based gate was rejected as costing a governance step for marginal
  value over the Rust test.

## Artifacts

- QA: `docs/qa/orchestrator/201-first-run-path-modernization.md`
- Gates: `core/src/fixture_driverless_tests.rs`,
  `scripts/qa/test-error-code-glossary.sh`, checks 5/6 + fixture 5 of
  `scripts/qa/test-release-publish-surface.sh`
- Docs: `docs/guide/error-codes.md` (+ zh), README Quick Start,
  `docs/guide/01-quickstart.md` (+ zh), `fixtures/manifests/README.md`
  (exemption convention)
