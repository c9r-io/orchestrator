---
lifecycle: active
related_fr: FR-157
---

# Source Domain Decomposition And Action-Audit Vocabulary

**Module**: Orchestrator Daemon / gRPC Surface / Coverage Governance
**Status**: Released
**Related Plan**: FR-157 Source 域分解与测试补强
**Related QA**: `docs/qa/orchestrator/208-source-domain-decomposition.md`
**Created**: 2026-08-02
**Last Updated**: 2026-08-02

## Background

The 2026-08-01 debt audit named the Source domain as the workspace's largest
single point of structural imbalance: 37 of 121 RPCs, the largest production
file in the tree, and 12.21% line coverage over the 2,500 lines that terminate
the outward-facing Slack integration.

The FR that followed was a proposal, and rebuilding its numbers against the tree
at `65000dff` changed what the work should be. Those corrections are recorded
first, because two of them are errors of a kind the format invites.

## What The Audit Actually Measured

**A production-line count read as a total.** The FR listed
`source_connection.rs` at "≈2574 production lines, 75 test lines" and
`source_router.rs` at "≈1574 lines", and grouped both as under-tested. The first
pair is right (2572/75 measured). The second number is `source_router.rs`'s
*production* count; the file totals 2275 lines and carries **701 test lines** —
31% of itself — including end-to-end routing cases driven through an axum
loopback stub. It was never the under-tested file. Requirement 1 lost it and
kept `source_connection.rs` (2572/75) and `server/source.rs` (1433/0).

The tell is that the FR mixed units inside one sentence without saying so. Both
numbers were correct measurements of different things.

**A glob that matched more than it named.** "`*source*.rs` totals ≈20k lines
across 6 crates" — the glob also matches `resource*.rs`. Excluding those:
18,415 lines across `core`, `daemon`, `cli`, `gui`, `orchestrator-config` and
`orchestrator-persistence`.

**A literal inventory short by three, missing a seventh file.** `"legacy_client"`
was recorded at 17 sites across six handlers. It is 20 sites across seven:
`server/action_audit.rs` was missed, and there the string is a *comparison*
(`mode == enforced && reason_code == legacy_client`) rather than an assignment —
19 assignments plus the one check that gives them meaning. `"compatibility"` was
recorded at 7; it is 10 lines / 11 occurrences (`crd/builtin_defs.rs` carries two
on one line, in a JSON schema's `enum` and its `default`), and they straddle the
`core` boundary, which the FR did not anticipate.

Everything the FR said about the proto surface and the coverage baseline survived
verification unchanged: 121 RPCs in a single `service OrchestratorService`, 37
`Source*`, 250 messages, 0 enums; `daemon/source_connection` 12.21%,
`daemon/session` 15.52%, `core/domain` 84.29%, `daemon adapter` 28.77%.

## Why The Module Measured 12%

Not because the handlers were untestable. `BoundaryFixture` constructs a real
`OrchestratorServer` but passes `slack_gateway: None`, so almost every handler
returned at its `"Slack Gateway is not configured"` branch. The OAuth intent
lifecycle, dedicated provisioning, manifest upgrade, App deletion, disconnect and
ownership transfer were not weakly covered — they were never entered.

The seam that makes them reachable already existed and was unused:
`SlackGatewayClient::new` permits `http` on loopback and rejects it everywhere
else. A fixture that binds `127.0.0.1:0`, serves both the Gateway protocol
(`/v1/**`) and the Slack manifest API (`/api/apps.manifest.*`) from one axum
app, and hands the resulting origin to both clients, reaches every one of those
state machines in process with no TLS and no network.

**The stub records rather than only answers.** Each inbound request is kept with
its path, body and bearer secret. That distinction matters: a handler that
silently skips a Gateway call still returns `Ok`, and an assertion on the return
value alone cannot tell the two apart. The tests assert that
`/v1/dedicated/import` was reached, that it was authorised by the slot secret and
not the enrollment key, and that the App's client secret travelled in its body.

**Unconfigured paths answer 503, not 404.** A handler that reaches an endpoint
the test did not anticipate fails loudly rather than taking a not-found branch
that resembles a legitimate error.

### The one path this cannot reach, and why it is recorded rather than worked around

`dedicated_preview` derives the reviewed manifest's OAuth callback and Events
URLs from the Gateway origin. `render_manifest_endpoints` refuses a plaintext
endpoint outright — a dedicated Slack App must not be reviewed with `http://`
callbacks — so against a loopback origin the preview cannot complete. It fails
with `Internal` naming the HTTPS requirement, *after* negotiating capabilities
and *before* any Configuration Token reaches Slack.

A `#[cfg(test)]` seam that relaxed the scheme was available; the repository has
precedent for one (`slack_api::set_test_api_base`). It was rejected: the HTTPS
requirement is the security property under test, and a seam that bypasses it
buys coverage by disabling the thing being covered.

Instead the behaviour is asserted as a behaviour — end to end in
`preview_checks_gateway_capabilities_then_refuses_a_plaintext_origin`, and again
at the composition level in `projection.rs`, where the same render is shown to
succeed against `https://gateway.example` and fail against `http://127.0.0.1:9`.
The approval flow beyond it is reached by seeding the session `dedicated_preview`
would have produced, which is exactly the state the durable checkpoint records.

## The Split

`source_connection.rs` was 2572 production lines — the largest production file
in the workspace, ahead of `cli/src/commands/guide.rs` at 2103 — against a
`server/` median of 335 and a largest-non-source of `session.rs` at 1064.

| Module | Production lines | Holds |
|---|---|---|
| `mod.rs` | 84 | shared types, session stores, re-exports |
| `query.rs` | 154 | `list`, `get`, `watch`, `catalog` |
| `dedicated.rs` | 631 | provisioning preview, approve, abandon, checkpoint reads |
| `oauth.rs` | 572 | intents: connect, poll, cancel, reauthorize, migrate |
| `lifecycle.rs` | 506 | manifest upgrade, App deletion |
| `transfer.rs` | 295 | disconnect, ownership transfer, default Trigger |
| `projection.rs` | 382 | pure projections, manifest diffing, validation |

The move was verified to be a move. Normalising only the four adjustments the
split forces — `super::authorize` / `super::map_core_error` becoming
`crate::server::*` because `super` now means `source_connection` rather than
`server`, the bundled manifest's `include_str!` path gaining a directory, and
cross-module helpers becoming `pub(super)` — the multiset of body lines before
and after is identical: 2498 lines, zero added, zero removed, zero changed.

`module_shape.rs` keeps the ceiling by walking the directory rather than listing
module names, so a submodule added tomorrow is covered the moment it exists. It
carries two assertions the size check alone would not give: that the scan found
any files at all (otherwise a renamed directory reports success having examined
nothing), and that the module is decomposed rather than merely trimmed — a
single 999-line file satisfies a size ceiling while changing nothing.

## The Split Would Have Broken Its Own Measurement

`scripts/coverage/coverage-governance.mjs` mapped the key module to the exact
path `crates/daemon/src/server/source_connection.rs`, and `matchingBucket`
compares with `startsWith`. A prefix ending in `.rs` matches the single file and
nothing beneath a directory of the same name.

Every submodule would therefore have dropped out of `daemon/source_connection`
silently. The gate would not have failed. It would have reported a *higher*
percentage over a near-empty denominator — the coverage-governance analogue of
`fr-governance` §4.4 shape 2, landing on the instrument rather than on the code.

The prefix now omits the suffix, so it reaches the directory, the pre-split file,
or both. The fixture guards the measured set rather than the spelling: it holds a
submodule, the single-file form, and a 100-line test source under `tests/`, and
asserts the module comes to 12/15 lines. Restoring the `.rs` suffix fails it
(exit 1, `expected: 15`); dropping the `/tests/` exclusion fails it too
(exit 1, `expected: 80`). Both mutations were run, not reasoned about.

### Where the test sources live, and why

They live under `source_connection/tests/`, which `isExcluded` drops. Coverage
that counts its own test bodies rewards writing tests over covering code: a
1,500-line test file scores near 100% on itself and lifts the module percentage
without executing one more production line.

Measured, `cargo-llvm-cov` does not report files under a `tests/` directory at
all, so the normalizer's rule is a second condition rather than the only one —
which is the correct arrangement, and the fixture above proves the rule works
should such a path ever appear.

## Coverage

`daemon/source_connection` 12.21% → **85.51%** (303/2482 → 2089/2443),
macos-aarch64, cargo-llvm-cov 0.8.5, at `497339b9`.

Derived three ways before being written down, per §6.1:

| Route | Result |
|---|---|
| `summarizeRust` — the gate's own code | 2089/2443 = 85.51% |
| The same JSON's per-file summaries, selected independently | 2089/2443 = 85.51% |
| LCOV `DA` records — a different unit (per executable line) | 2029/2335 = 86.90% |

The denominator fell 2482 → 2443 because the module's 75 inline test lines moved
into `tests/`. That is the one direction a coverage rise can be manufactured, so
it is checked rather than explained: holding the *old* denominator, the same
numerator is 84.2%. The rise is executed production code.

Three baseline entries move and only three — the key module,
`daemon/action_audit` (the new boundary assertions exercise it, and its
denominator grew by the FR-157 audit constant), and the `daemon adapter`
component containing both.

### A ratchet finding this FR is not fixing

The remaining entries stay at their 2026-07-27 values, and one of them has
drifted a long way: **CLI now measures 52.86% against an approved 35.49%**, on a
denominator that grew 6038 → 7373; `cli/commands` likewise 33.78% → 44.45%.
Because `metricRegression` only fails on the falling direction, those entries
keep passing while under-ratcheted indefinitely. That is the gap the 2026-07-27
re-approval note already named, now with a measured size. The movement belongs to
the FRs that caused it and this run is not their evidence, so it is recorded here
rather than silently absorbed.

## The Action-Audit Vocabulary

`"legacy_client"` at 20 sites, `"compatibility"` and `"enforced"` as bare
literals on both sides of the `core` boundary.

**The constants do not go where the FR said.** The FR required them in `core`,
reasoning that most `"compatibility"` sites are core-side. The dependency runs
the other way — `core` depends on `orchestrator-config` — and `cli_types.rs`,
which holds the serde default, is below core. Putting them in
`orchestrator-config` reaches every call site through the existing
`pub use orchestrator_config::cli_types` re-export and leaves core's public
surface untouched: the boundary ledger still reads 128 files, 49 `pub mod`, 611
public items, so **the ledger regeneration the plan budgeted for is not needed.**

`FALLBACK_REASON_LEGACY_CLIENT` stays in `crates/daemon/src/server/action_audit.rs`,
beside the comparison that gives it meaning, because all 20 sites are in that
crate and no core file names it.

`orchestrator-scheduler`'s `json!({"summary":"compatibility"})` keeps its
literal. It is not an exemption: it is a task summary that happens to be the same
word, in a file that names neither `action_audit_mode` nor
`fallback_reason_code`, so the gate below never considers it.

### Two behavioural assertions, because the count proves nothing

Replacing a literal with a constant compiles, satisfies every count-based check,
and can silently change what reaches the audit table. So:

- a mutation carrying no audit context is still recorded with reason code
  `"legacy_client"` — asserted against **the literal the wire has always
  carried**, not against the constant — together with the synthesised
  `legacy:<request-id>` idempotency key;
- under a real `RuntimePolicy` with `action_audit_mode: enforced`, the same call
  is refused with `reason_code is required`, while an explicit reason code is
  still admitted under the identical policy.

The second one had to be written twice. The first version passed a `None`
context, which enforced mode refuses one check earlier for a different reason
(`action audit context is required`) — the assertion was green while never
entering the branch it named. A context with a *blank* reason code is what
actually falls back to `legacy_client`.

### The gate derives its scope and its exemptions

It walks every Rust source in the workspace and keeps the files that name
`action_audit_mode` or `fallback_reason_code`. A file that starts using the
vocabulary is picked up the moment it does; one that merely contains the word
"compatibility" in an unrelated payload names neither and is never considered.
Occurrences are counted with `matches`, not lines, so two on one line count two.

Test code is exempt on purpose — the assertion pinning the recorded reason code
to `"legacy_client"` has to spell the literal to do its job — and "test code" is
itself derived three ways: a `tests/` directory, a module a parent declares under
`#[cfg(test)]`, and everything after a file's own first inline `#[cfg(test)]`.

Three mutations were run, each failing with a distinct diagnostic: a production
reference commented out (not deleted) with the literal restored — exit 101,
naming file and line; a second `const … = "legacy_client"` — exit 101, "must have
exactly one definition site"; and the scan's own markers changed so nothing
matches — exit 101, "the derivation is broken", guarding the case where the gate
reports success having read no input.

A fourth mutation was attempted first and correctly ignored: it targeted a
literal inside `core/src/resource/project.rs`, which sits after that file's own
`#[cfg(test)]` at line 106. The gate was right and the mutation was badly chosen.
It is recorded because "the gate did not fire" and "the gate is broken" look
identical until you check which line you actually changed.

## Proto Surface Governance

FR-157 requirement 3 asked for a decision, not a change. Measured at `65000dff`:
one `service OrchestratorService`, 121 `rpc`, 250 top-level `message`, **0
`enum`**, and **0 `reserved`** declarations anywhere in the file.

Domain distribution of the 121:

| Prefix | RPCs | | Prefix | RPCs |
|---|---|---|---|---|
| `SourceConnection*` | 18 | | `Process*` | 4 |
| `Task*` | 17 | | `Trigger*` | 3 |
| `Agent*` | 13 | | `Handoff*` | 2 |
| `SourceAutomation*` | 9 | | `ActionAudit*` | 2 |
| `Attention*` | 7 | | `Resource*` | 1 |
| `Source*` (bare) | 6 | | *(no domain prefix)* | 15 |
| `Secret*` | 6 | | | |
| `Store*` | 5 | | | |
| `SourceTask*` | 4 | | | |

The 15 without a prefix are the daemon's own verbs — `Ping`, `Init`, `Shutdown`,
`Apply`, `Get`, `Describe`, `Delete`, `Check`, `MaintenanceMode`, `WorkerStatus`,
`QaDoctor`, `RunStep`, `ResumePlan`, `ResumeExecute`, `ResumeBoundaryList`.

### Decision: one service, with a stated ownership rule

Splitting `OrchestratorService` into per-domain services was assessed and
rejected. The cost is concrete and the benefit is not: every service is a
separate client stub, every CLI and GUI call site names one, and the daemon
serves all of them over a single UDS with one authorization chokepoint
(`server::authorize`, keyed by RPC name). Splitting multiplies the client
surface and the mounting code to relieve a file-size problem the module split
already solved on the side where it hurt.

**The rule for a new RPC**, recorded so the 122nd does not need a judgement call:

1. It takes the prefix of the resource it acts on, spelled exactly as that
   resource's proto message — `SourceConnectionDedicatedApprove`, not
   `ApproveDedicatedSourceConnection`. Verb last.
2. A resource with sub-resources nests prefixes rather than inventing a new
   top-level one: `SourceTask*` and `SourceAutomation*` sit under Source and are
   counted there.
3. Only a daemon-lifecycle or whole-config verb may go unprefixed. That set is
   the 15 above; adding to it needs a reason in the reviewing FR.
4. Its request and response messages take the RPC's own name plus
   `Request`/`Response`. This is what lets the domain histogram above be derived
   from the file rather than maintained by hand.

### Decision: the string enums stay strings, and the reason is not inertia

Three of the bare-string fields have genuinely closed value sets, verified in
code rather than assumed:

| Field | Closed set | Enforced at |
|---|---|---|
| `action_audit_mode` | `compatibility`, `enforced` | `core/src/resource/runtime_policy.rs` |
| `provisioning_mode` | `managed_shared`, `managed_dedicated`, `manual` | `SourceConnectionMode` |
| dedicated `status` | 7 values | `valid_provision_status` |

`fallback_reason_code` is *not* one of them — it is an open vocabulary
(`legacy_client`, `dedicated_slack_app`, `managed_connection_migration`,
`operator_force_reference_cleanup`, …) and would be wrong to close.

Converting the three to proto enums is wire-compatible in the narrow sense — a
`string` field and an `enum` field with the same number are not — so it would
require new field numbers, and this file has **no `reserved` declarations at
all**, meaning there is no established discipline for retiring a number. The
migration would be: add `mode_v2` as an enum, populate both, teach every reader
to prefer the enum, then retire the string and reserve its number. That is three
releases of dual-write for a property that is already enforced in Rust at the
only boundary that can violate it.

The decision is to keep the strings and make the vocabulary single-sourced
instead, which requirement 4 did. The condition that would change this decision
is stated so it can be checked rather than re-argued: **if a fourth reader of
`action_audit_mode` appears outside the Rust workspace** — a non-Rust client, a
stored projection, a webhook payload consumer — the Rust-side validation stops
being the only boundary and the enum becomes worth its three releases.

## Known Limits

1. `dedicated_preview`'s post-review path is not reachable in process, for the
   reason given above. Its durable effects are covered through the seeded
   approval flow; its manifest composition is covered directly.
2. `server/source.rs` (1433 production lines, 0 test lines) is untouched. FR-157
   named it in its inventory but scoped requirements 1 and 2 to
   `source_connection`. It remains the largest zero-test production file in
   `server/`.
3. The coverage baseline's non-source entries are under-ratcheted, measurably so
   for CLI. Recorded above; not this FR's to fix.
4. The proto ownership rule (above) is prose, not a gate. Nothing fails a build
   when the 122nd RPC ignores it.
