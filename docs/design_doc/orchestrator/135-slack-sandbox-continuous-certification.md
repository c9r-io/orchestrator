---
lifecycle: active
related_fr: FR-123
---

# Orchestrator - Slack Sandbox Continuous Certification

**Module**: Orchestrator / Slack Integration Gateway

**Status**: Approved

**Related Plan**: FR-123 shared and dedicated Slack sandbox recertification,
checkpoint recovery, secret custody, evidence freshness, and cleanup governance

**Related QA**: `docs/qa/orchestrator/173-slack-sandbox-continuous-certification.md`

**Created**: 2026-07-25
**Last Updated**: 2026-07-25

## Background

FR-114 and FR-115 proved the real Slack boundaries for shared official-App
OAuth and dedicated per-workspace App provisioning. Their provider runs were
documented correctly, but execution remained split between one shared smoke
script and long manual runbooks. The resulting records did not have a common
schema, expiry semantics, resumable run state, or one cleanup inventory.

Real OAuth, Events API delivery, App Manifest behavior, and Cloudflare callback
configuration can drift independently of the deterministic fake-provider
suite. A controlled live run therefore remains necessary, but it must never
turn ordinary CI into a credentialed provider test.

## Goals

- Provide one opt-in entry point for `shared`, `dedicated`, and `both`.
- Persist stable, non-secret checkpoints across browser consent and provider
  risk-control pauses.
- Pass only stage-minimal environment variables to live subprocesses.
- Produce allowlisted evidence with explicit freshness and cleanup state.
- Keep destructive provider cleanup reviewed and rerunnable.
- Replay sanitized provider shapes in ordinary network-free CI.

## Non-goals

- Bypassing Slack verification, consent, workspace policy, or Configuration
  Token controls.
- Storing live tokens in the repository, CI, SecretStore fixtures, or evidence.
- Claiming that recorded provider payloads are live certification.
- Adding a daemon RPC, database table, or Process Console page.
- Automatically deleting a Slack workspace, App, or external domain on exit.

## Scope

- In scope: Bash controller/library, inert mode-`0600` environment parsing,
  XDG state, stage checkpoints, same-message badge smoke, sanitized recorded
  fixtures, evidence status/promotion, cleanup inventory, CI/release status,
  and runbook synchronization.
- Out of scope: Slack product-state changes, Gateway schema changes, daemon
  migrations, browser automation that bypasses consent, or unattended
  destructive provider operations.

## Interfaces

The canonical operator interface is:

```bash
./scripts/qa/certify-slack-managed-live.sh run --mode shared|dedicated|both
./scripts/qa/certify-slack-managed-live.sh resume --run-id {run_id}
./scripts/qa/certify-slack-managed-live.sh checkpoint \
  --run-id {run_id} --stage {stage} --result pass \
  --evidence-code {safe_code}
./scripts/qa/certify-slack-managed-live.sh cleanup --run-id {run_id}
./scripts/qa/certify-slack-managed-live.sh status
```

Exit code `20` means the run is safely waiting for a human/provider
checkpoint. It is not a product failure. `checkpoint` accepts only known manual
stages and safe evidence-code characters. The dedicated delete checkpoint and
external destructive cleanup require the exact run ID as a second explicit
confirmation.

The actual environment defaults to:

```text
~/.config/orchestrator/qa/slack-live.env
```

`FR114_LIVE_ENV_FILE` remains a compatibility alias. The file is parsed as a
small allowlisted data format and is never sourced as shell code.

## Data And Evidence

There are no product database changes.

Private run state is stored under:

```text
${XDG_STATE_HOME:-~/.local/state}/orchestrator/slack-certification/{run_id}/
```

The directory and files use `0700`/`0600`. `private-state.json` may contain raw
external object identifiers required for cleanup, but never Configuration
Tokens, OAuth codes/states, client/signing secrets, installation tokens, or
Gateway keys.

`safe-result.json` projects only:

- run/mode/status and timestamps;
- build commit and stable evidence codes;
- stage results;
- salted per-run identity digests;
- secret-scan counts/result;
- cleanup action/result and destructive-confirmation state.

After evidence review, `promote` may update
`docs/qa/evidence/slack-live-certification-latest.json`. `status` derives
`fresh` or `stale` at read time. The default TTL is 30 days. `stale` means
`recertification_required_not_product_regression`; the historical PASS result
is not rewritten.

## Key Design

1. **Certification is a QA control plane.** Slack ownership, OAuth, delivery,
   badge matching, and task mutation remain in Gateway/daemon authority. The
   controller sequences and observes those existing boundaries.
2. **Human checkpoints are first-class.** OAuth consent, Slack risk controls,
   permission review, and App deletion pause with durable state rather than
   attempting to automate around provider policy.
3. **Secret input is inert and stage-minimal.** The parser rejects unknown
   variables, unsafe file modes, command substitution, backticks, and
   multiline input. Aggregate tests receive no live secrets. Badge smoke runs
   under `env -i` with only the selected installation, driver token, and safe
   process basics.
4. **Private and safe inventories are separate.** The private inventory makes
   cleanup actionable. Safe evidence carries only digests and outcomes.
5. **Cleanup never guesses authority.** Synthetic messages and local temporary
   material are bounded by traps. App/workspace/domain cleanup stays pending
   until a reviewed external action and explicit run-ID confirmation.
6. **Live and recorded evidence remain distinct.** CI validates controller
   behavior and four sanitized shapes: OAuth callback, Events API delivery,
   manifest diff, and Gateway import receipt. It never calls Slack.
7. **Badge proof uses one message.** Both reactions target one synthetic
   message, proving distinct binding/task fan-out while remove/add retry of one
   reaction still converges.

## Alternatives And Tradeoffs

- **Store certification in the daemon database**: this would improve UI
  visibility but would mix provider-test secrets/cleanup identifiers with
  product state and make certification depend on the system under test.
- **Use a CI secret environment**: this could schedule live runs, but ordinary
  pull requests would gain provider authority and become sensitive to Slack
  outage/risk controls.
- **Fully automate OAuth and App deletion**: this would reduce clicks but would
  violate explicit consent and destructive-review requirements.
- **Keep only Markdown records**: this is readable but cannot derive expiry,
  compare modes, or enforce safe promotion mechanically.

The selected design keeps live work explicitly operator-controlled while making
every deterministic part reproducible.

## Risks And Mitigations

- Risk: private state survives a failed run.
  - Mitigation: every exit emits a safe cleanup inventory; `cleanup` is
    independently rerunnable and never hides pending external objects.
- Risk: a child prints a credential before failure.
  - Mitigation: live output stays in a private log directory, known values and
    generic token patterns are scanned, and only the safe projection is
    promotable.
- Risk: an operator attests the wrong manual stage.
  - Mitigation: only the current known stage is accepted; evidence codes are
    bounded; automated state/smoke assertions and independent evidence review
    remain required.
- Risk: provider drift is mistaken for a regression.
  - Mitigation: `blocked`, `failed`, freshness, and safe evidence codes are
    separate. Expiry never changes a historical result.
- Risk: a historical record is silently treated as current code proof.
  - Mitigation: evidence retains its exact candidate commit and manifest
    digest; release status displays certification time and expiry.

## Observability

- Logs: private per-stage files under the run directory.
- Structured state: stage result, update time, evidence code, overall status,
  cleanup result, and scan count.
- Exit semantics: `0` completed, `1` test/product failure, `2` configuration
  misuse, `20` safe human/provider pause.
- Release display: shared/dedicated result, freshness, certification time,
  expiry, and run ID in the GitHub step summary.
- Metrics/tracing: none; this operator-only harness does not add production
  telemetry.

## Operations / Release

- Copy `config/qa/slack-live.env.example` outside the repository and set mode
  `0600`.
- Run only against isolated sandbox workspaces, Gateway, daemons, and
  echo-only fixtures.
- Do not use shell xtrace, browser HAR capture, or process/environment dumps.
- Promote evidence only after its privacy and cleanup results are PASS.
- Ordinary CI runs `scripts/qa/test-slack-live-certification.sh`; release CI
  displays status but does not load secrets or reinterpret stale as failure.
- Rollback removes the controller/CI integration and returns to the manual
  runbook. Product runtime and databases require no rollback.

## Test Plan

- Unit/shell: inert env parsing, file permissions, unknown keys, checkpoints,
  private/safe inventory, redaction, cleanup confirmation, expiry, missing
  live input, and safe evidence schema.
- Recorded integration: sanitized OAuth callback, Events API reaction,
  manifest diff, and Gateway receipt shapes on macOS and Linux CI.
- Existing regression: FR-114 shared aggregate and FR-115 dedicated aggregate,
  including Gateway/core/daemon, Clippy, Vitest, Playwright, and privacy gates.
- Live: shared and dedicated OAuth/lifecycle matrices, same-message two-badge
  routing, duplicate convergence, cursor recovery, disconnect fail-closed, and
  reviewed cleanup.

## QA Docs

- `docs/qa/orchestrator/173-slack-sandbox-continuous-certification.md`
- `docs/guide/slack-managed-sandbox-certification-runbook.md`

## Acceptance Criteria

- One entry supports shared, dedicated, combined, and resumable checkpoints.
- The committed example is complete while real secrets stay out of git,
  stdout/stderr, and promotable artifacts.
- Both live matrices remain repeatable under the controlled runbook.
- Same-message badges, duplicate delivery, cursor recovery, and disconnect
  fail-closed have explicit evidence stages.
- Every run emits expiry and cleanup inventory with safe/private separation.
- Interrupted cleanup is rerunnable and destructive actions require explicit
  confirmation.
- Ordinary CI uses recorded fixtures and never fails because live secrets are
  absent.
