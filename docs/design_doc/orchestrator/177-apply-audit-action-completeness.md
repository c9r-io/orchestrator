---
lifecycle: active
related_fr: FR-164
---

# Orchestrator - Apply Audit Action Completeness

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-164 apply-path action naming and the envelope-less audit gap  
**Related QA**: `docs/qa/orchestrator/214-apply-audit-action-completeness.md`  
**Created**: 2026-08-11  
**Last Updated**: 2026-08-11

## Background

DD-111 makes one bounded envelope "the durable source of truth for every
process-console mutation", and states that compatibility mode records an
envelope-less caller under `legacy_client` while enforced mode rejects it before
mutation. `resolve_context` implements both branches faithfully.

The `Apply` RPC did not reach them. `audited_mutation` read:

```rust
let audited_mutation = !dry_run
    && (context.is_some()
        || contains_driver_raw_args
        || /* SourceTaskTemplate | SourceTaskBinding */);
```

`action_audit::begin` — the only caller of `resolve_context` — ran only when
that was true. Two defects follow from one condition:

1. **No row.** An envelope-less apply of a SecretStore, Workflow, Trigger,
   RuntimePolicy, or any other kind outside the three special cases wrote no
   `control_action_audit` row.
2. **No rejection.** The first disjunct is `context.is_some()` — the presence of
   the very envelope enforced mode exists to demand. So under
   `action_audit_mode: enforced`, an envelope-less apply was neither audited nor
   refused. Enabling enforcement returned a false assurance, which is worse than
   not offering it.

Separately, only three kinds had named actions. The other nine shared the
generic `resource.apply`, so an audit reader could not distinguish a SecretStore
write from a Workspace edit. FR-160's binding gate false-positive (QA 157) was a
symptom of the same mechanism.

The live exposure was not hypothetical. `orchestrator apply` and the GUI both
sent envelopes; `orchestrator tool secret-rotate` — which rewrites a SecretStore
**value** — sent `audit: None`.

## Goals

- Every non-dry-run apply reserves an envelope, so DD-111's stated contract holds
  for the apply path as it already did for the other 26 named actions.
- Every `ResourceKind` has its own action name, derived from the enum rather than
  from a hand-maintained list.
- Secret rotation is attributable.
- No behaviour change for callers already sending envelopes.

## Key Design

1. **`audited_mutation` is `!dry_run`.** Unconditional for mutations. In
   compatibility mode (the default, and the only mode any shipped manifest
   configures) an envelope-less apply now records a row under `legacy_client`
   instead of nothing. In enforced mode it is rejected, as DD-111 promised.
   Dry runs remain unaudited and are asserted separately so "audit everything"
   cannot be over-applied.

2. **Action and target type come from wildcard-free matches.** `apply_action`
   and `apply_target_type` match every `ResourceKind` with no `_` arm, so a
   thirteenth variant fails to compile rather than silently inheriting the
   generic name. **This compile-time obligation is the derivation** — the test's
   `ALL_KINDS` array is a convenience that `covers_every_variant` keeps in sync,
   not the guarantee. An enumeration is exactly what §4.4 shape 2 warns about;
   here the enumeration cannot drift because the compiler holds the other end.

3. **Naming: `resource.<snake_kind>.apply`, with two documented exceptions.**
   `source.template.apply` and `source.binding.apply` keep their shipped
   spellings. They appear in DD-111, QA 157 and stored audit rows; renaming them
   for regularity would falsify records that are correct. Regularity is worth
   less than not invalidating history. An Agent manifest carrying
   `driver.rawArgs` keeps `agent.driver.raw_args.apply` — a property of the
   payload, not of the kind, so the caller substitutes it rather than the match.

4. **Bundles keep one aggregate row.** `control_action_audit` is keyed by
   `request_id`, and DD-111's reservation/replay path (`should_execute`) is
   request-scoped; expanding a bundle into N rows would mean synthesising N
   request IDs and reworking that path.

5. **Secret rotation carries an envelope**, with its request construction
   extracted into `secret_rotate_apply_request` so the envelope is assertable on
   the request actually sent rather than by searching the source.

## Alternatives And Tradeoffs

- **Per-document bundle rows.** Would make bundle kinds visible without a schema
  change, but requires reworking DD-111's request-scoped reservation. Rejected as
  disproportionate to the value.
- **A persisted `document_inventory` column.** Would genuinely deliver a
  queryable bundle inventory, at the cost of a schema migration plus a proto
  field, and cuts against DD-111's bounded-evidence design. Deferred; the path is
  recorded above should the need become concrete.
- **Uniform renaming of all twelve kinds.** Rejected: see Key Design 3.
- **Keeping the envelope optional and only adding names.** Rejected once step 0
  established that optionality was never DD-111's intent. This is a conformance
  repair, not a design revision.

## Known Limits

- **A bundle's per-document kinds are not recoverable from the audit surface.**
  The obvious fix — putting an inventory in `canonical_request` — does not work
  and is worth recording so it is not attempted again: `canonical_request` is
  deliberately never persisted (`core/src/action_audit.rs:196-198` stores only
  its SHA-256). An inventory there would be invisible, and being derived from
  the same content that `content_hash` already covers, it would add no hash
  entropy either. The workaround is correlating `resource_versions.created_at`
  with `control_action_audit.created_at`.
- **An unparseable manifest records `resource.apply` / `resource_manifest`.**
  Correct — it has no resolvable identity — but it means a gate exercising both
  valid and invalid input cannot filter on a single action name and still see the
  whole sequence. `test-expert-resources-governed-editing.sh` now lists without
  `--action` for this reason.
- **The gRPC integration harness cannot observe any of this.**
  `TestOrchestratorServer::apply` (`crates/integration-tests/src/lib.rs:1281`)
  reimplements the RPC by calling `apply_manifests` directly, bypassing the
  daemon handler. Behavioural coverage therefore lives in the daemon crate
  against a real `OrchestratorServer`. This is a standing hazard beyond this FR:
  a harness that mirrors production RPCs by reimplementation will diverge
  silently, and a test written against it certifies the mirror. Worth an audit of
  which other RPCs that harness re-implements.
- **`kind_as_str_covers_all_resource_kinds`** (`core/tests/integration_test.rs`)
  asserts three of twelve kinds despite its name — an instance of §4.4 shape 2 in
  existing tests. Left as found; recorded so it is not mistaken for coverage.
- **`Delete` has the same defect and a worse form of it — filed as FR-167.**
  Asking §5's question ("what state satisfies every criterion while the goal is
  unmet?") of this FR's criteria answers: deletion. `delete` gates its envelope on
  `force_references || is_source_task_binding`, so an ordinary delete of any of
  the other eleven kinds records no `control_action_audit` row, and enforced mode
  is unreachable for it exactly as it was here. It is worse than apply's
  pre-FR-164 state in one specific way: that condition does not include
  `context.is_some()`, so **even a client that correctly sends an envelope gets
  no row** — the envelope is accepted and discarded. The residue is a tombstone
  `resource_versions` row (`version = -1`, `spec_json = '"deleted"'`) whose author
  is again the constant `"daemon-apply"`, and which does not retain the deleted
  spec.

  The shape generalises past both RPCs and is the reason this is recorded here
  rather than only in the new FR: **a handler that gates entry to the audit layer
  on a condition the audit layer itself exists to adjudicate satisfies neither
  branch of the policy while appearing to implement both.** Two instances in one
  file is not a coincidence, so every call site of `action_audit::begin` deserves
  its guard read with that question in mind.

## Observability

- Actions: `resource.<kind>.apply` for ten kinds, `source.template.apply` and
  `source.binding.apply` for the two legacy names,
  `agent.driver.raw_args.apply` for raw-args Agent manifests, and
  `resource.apply` for bundles and unparseable manifests.
- Target types: the per-kind `target_type` (`secret_store`, `workflow`, …), or
  `resource_manifest` for the aggregate cases.
- Envelope-less callers appear with `reason_code` = `legacy_client`, which is
  also the signal DD-111 names for measuring readiness to switch a project to
  enforced mode. That measurement was previously blind to the apply path, so
  "legacy traffic has reached zero" could have been read off an incomplete
  surface.

## Verification

Negative fixtures were executed, not merely described, and each reports a
distinct diagnostic so the log identifies which way it broke:

| Mutation | Effect |
|---|---|
| Restore the old `audited_mutation` disjunction | 4 tests fail; the enforced-mode test fails because the apply **succeeds** — the bypass, reproduced |
| `ResourceKind::SecretStore => "resource.apply"` | 3 tests fail naming SecretStore |
| Comment out the rotation envelope (not delete it) | 2 tests fail while `grep -c 'operator_secret_rotate'` still returns 2 |

The third is the one that matters for method: a text-presence check would have
certified the broken state as working.
