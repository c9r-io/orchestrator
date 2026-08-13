---
lifecycle: active
related_fr: FR-167
self_referential_safe: true
---

# Orchestrator - Delete Audit Action Completeness

**Module**: Orchestrator
**Scope**: Unconditional delete auditing, per-kind action naming, enforced-mode conformance, the
generic name for kinds outside the enum, and the kind-dispatch defects the behavioural tests exposed
**Scenarios**: 8
**Priority**: High

## Background

`crates/daemon/src/server/resource.rs::delete` reserved an audit envelope only when

```rust
let attempt = if force_references || is_source_task_binding { ... }
```

Three consequences, and the second is what separates this from FR-164:

1. Eleven of the twelve kinds wrote **no `control_action_audit` row** when deleted. Only a
   `SourceTaskBinding` (`source.binding.delete`) and a `--force-references` cleanup
   (`delete_references`) were recorded.
2. The condition had **no `context.is_some()` disjunct at all**. Apply's pre-FR-164 condition had
   one, so an enveloped apply was still audited; a delete's envelope was accepted and discarded.
   The CLI sends one on **every** delete (`crates/cli/src/commands/resource.rs`), so the default
   path was the dropped one.
3. `begin` is the only caller of `resolve_context`, so `action_audit_mode: enforced` neither
   audited an ordinary delete nor refused it — on the one operation that cannot be undone.

**Read the audit table by name.** A successful delete also writes a `resource_versions` tombstone
(`version = -1`, `spec_json = '"deleted"'`) whose `author` is the constant `"daemon-delete"`. An
assertion phrased as "a row exists" is satisfied by a row written whether or not this behaviour
works; these scenarios assert against `control_action_audit` specifically.

## Verification Commands

```bash
cargo test -p orchestratord resource_delete_audit_tests   # behavioural, real OrchestratorServer
cargo test -p orchestratord delete_action_naming          # per-kind mapping
cargo test -p orchestrator-cli resource_delete_envelope_tests
cargo test -p agent-orchestrator alias_table_is_single_sourced
./scripts/qa/test-expert-resources-governed-editing.sh
./scripts/qa/test-source-task-binding.sh
```

The behavioural tests live in the daemon crate, not `crates/integration-tests`. That harness's
`delete` reimplements the RPC by calling `delete_resource` directly
(`crates/integration-tests/src/lib.rs:1365-1389`), so it never enters the audit path — the same
structural blindness FR-164 found in its `apply`.

---

## Scenario 1: Envelope-less Mutating Delete Is Audited

### Steps

1. Run `cargo test -p orchestratord resource_delete_audit_tests::envelope_less_secret_store_delete_is_audited`.
2. Inspect the assertion target: `control_action_audit`, via `AsyncActionAuditRepository::list`.

### Expected

- Exactly one row with `action` = `resource.secret_store.delete`, `target_type` = `secret_store`,
  `target_id` = `secretstore/fr167-store`.
- `reason_code` = `legacy_client` — the envelope-less client is recorded, not dropped.
- `status` = `succeeded`.

### Negative Fixture

Restore the pre-FR-167 guard, expressed against the resolved kind so it still compiles:

```rust
let attempt = if force_references
    || resolved_kind == Some(agent_orchestrator::cli_types::ResourceKind::SourceTaskBinding) {
```

**Measured**: five tests fail. This scenario reports `left: 0, right: 1` with "envelope-less
SecretStore delete must leave exactly one named audit row". The kind sweep fails on
`resource.trigger.delete` — the *first* kind in reverse order — which is what distinguishes this
fixture's log from Scenario 5's.

---

## Scenario 2: An Enveloped Delete Keeps The Client's Envelope

This is the branch that has no counterpart in FR-164 and was red before this FR.

### Steps

1. Run `cargo test -p orchestratord resource_delete_audit_tests::enveloped_delete_preserves_the_clients_reason_code`.
2. The test passes `reason_code: operator_resource_delete`, an operator reason, and an idempotency
   key.

### Expected

- One row whose `reason_code` is `operator_resource_delete` — **not** `legacy_client`.
- `operator_reason` and `idempotency_key` are the client's, unmodified.

Asserting the client's own values rather than "a row exists" is what separates *the envelope was
honoured* from *something got recorded*. Under the old guard this returns zero rows; under a fix
that audited unconditionally while ignoring the context it would return one row reading
`legacy_client`, and only the reason-code assertion catches that second state.

---

## Scenario 3: Dry Run Stays Unaudited

### Steps

1. Run `cargo test -p orchestratord resource_delete_audit_tests::dry_run_delete_is_not_audited`.

### Expected

- A `dry_run: true` delete reserves no envelope; the audit row count is unchanged from the seeding
  apply, and no `resource.secret_store.delete` row exists.
- This is the one case the unconditional `!dry_run` must still exclude, asserted separately so that
  "audit everything" cannot be over-applied.

The count is compared against the post-seed baseline rather than against zero: the seeding apply
writes its own row, so an assertion of "the table is empty" would be false for a reason that has
nothing to do with delete.

---

## Scenario 4: Enforced Mode Rejects An Envelope-less Delete

### Steps

1. Run `cargo test -p orchestratord resource_delete_audit_tests::enforced_mode_rejects_envelope_less_delete`.
2. The test seeds a RuntimePolicy with `action_audit_mode: enforced`, then deletes with
   `audit: None`.

### Expected

- Rejected with `InvalidArgument` and the diagnostic `action audit context is required`.
- No `resource.secret_store.delete` row for the rejected attempt.
- An **enveloped** delete of the same resource then succeeds — so the refusal is a refusal, not a
  path that fails for some unrelated reason.

The **diagnostic string** is asserted, not only the gRPC code. `InvalidArgument` is also what a
delete without `--force` and a malformed `kind/name` return, so a code-only assertion would pass on
a delete that never reached the audit layer — which is the pre-FR-167 behaviour this scenario must
fail against.

### Negative Fixture

Under the restored old guard the envelope-less delete **succeeds**, and the test fails at
`expect_err` with "enforced mode must reject an envelope-less delete". That failure is the bypass,
reproduced.

---

## Scenario 5: Every ResourceKind Records Its Own Delete Action

### Steps

1. Run `cargo test -p orchestratord resource_delete_audit_tests::every_resource_kind_records_its_named_delete_action`.
2. Run `cargo test -p orchestratord delete_action_naming`.

### Expected

- Twelve kinds applied, then deleted in reverse dependency order, each recording exactly one row
  with its own `action` and `target_type`, `reason_code` = `legacy_client`.
- `resource.<kind>.delete` for ten kinds; `source.template.delete` and `source.binding.delete` for
  the two Source kinds.
- Eleven rows read `status = succeeded`. **RuntimePolicy reads `status = failed`** — see below.

### The RuntimePolicy Asymmetry

`RuntimePolicy` is not deletable. `canonical_project_kind` has no arm for it, because there is no
`ProjectConfig` map to remove it from, so the delete fails with
`unknown resource type for project delete: runtimepolicy`. The audit row is still reserved *before*
execution, so the attempt is recorded under its own name with `status = failed`.

This is asserted rather than skipped. An attempt to delete a project's runtime policy is exactly
the thing an audit trail exists to record, and a scenario that quietly exercised eleven kinds while
claiming twelve would be §4.4 shape 2 applied to its own coverage.

### How The Set Is Derived

`delete_action` and `resource_target_type` are exhaustive matches with **no `_` arm**, so a
thirteenth `ResourceKind` variant fails to compile rather than silently inheriting
`resource.delete`. The test's list is length-checked against
`agent_orchestrator::resource::ALL_RESOURCE_KINDS` rather than against the literal 12, and that
constant is itself guarded by `all_resource_kinds_covers_every_variant`.

### Negative Fixture

Collapse one arm to the generic name — `ResourceKind::SecretStore => "resource.delete"`. Chosen over
deleting the function because a half-reverted match is the realistic regression, not a missing
symbol.

**Measured**: six tests fail across two modules, and every diagnostic names SecretStore —
"SecretStore still falls back to the generic delete action name", "SecretStore applies as
resource.secret_store.apply but deletes as resource.delete", "an action name is claimed twice
across the apply and delete surfaces (left: 28, right: 27)", and
"resource.secret_store.delete must record exactly one row". Distinct from Scenario 1's fixture,
whose sweep failure names `resource.trigger.delete` instead, so the log says which way it broke.

---

## Scenario 6: Kinds Outside The Enum Record The Generic Action

### Steps

1. Run `cargo test -p orchestratord resource_delete_audit_tests::crd_and_custom_resource_deletes_record_the_generic_action`.

### Expected

- A CRD-defined custom resource and the CRD itself are both deleted and both record `action` =
  `resource.delete`, `target_type` = `resource_manifest`, `status` = `succeeded`.
- Each row still carries the `target_id` it was asked to delete, so the generic name does not mean
  an anonymous row.

These are deletable and were equally unaudited before FR-167; the FR document did not mention them,
which the governance pass corrected. The naming mirrors apply, where a manifest that resolves to no
single builtin descriptor records `resource.apply` / `resource_manifest`.

---

## Scenario 7: The Shipped Cleanup Action Does Not Regress

### Steps

1. Run `cargo test -p orchestratord resource_delete_audit_tests::force_references_still_records_delete_references_alone`.
2. Run `./scripts/qa/test-source-task-binding.sh` — its own `source.binding.delete` assertion is
   independent of this FR's tests and was left untouched.

### Expected

- A `--force-references` SourceTaskTemplate delete records exactly one `delete_references` row with
  `target_type` = `source_task_template` and `reason_code` = `operator_force_reference_cleanup`.
- **No `source.template.delete` row is recorded for the same request.** A cleanup removes bindings
  the caller never named, so it is not the per-kind delete of its target.

The negative half is asserted because an implementation that recorded both actions would satisfy a
bare "`delete_references` exists" check while doubling every cleanup in the audit trail.

---

## Scenario 8: The CLI's Envelope Reaches The Row

### Steps

1. Run `cargo test -p orchestrator-cli resource_delete_envelope_tests`.
2. Run `./scripts/qa/test-expert-resources-governed-editing.sh`.

### Expected

From the unit tests, on the request actually constructed:

- An ordinary delete carries `reason_code` = `operator_resource_delete`, **no** `operator_reason`,
  and a key prefixed `cli-resource-delete-` that is not in the `cli-resource-delete-references-`
  space.
- A `--force-references` delete keeps `operator_force_reference_cleanup` and its cleanup reason.
- Successive deletes get distinct retry identities, so a second delete is not refused as a replay.

From the gate, end to end through the real CLI and daemon:

- A SecretStore applied through the CLI is deleted through the CLI.
- `audit list --action resource.secret_store.delete` returns exactly one row with
  `reason_code` = `operator_resource_delete` — the operator's, not the `legacy_client` fallback.
- `get secretstore/...` afterwards fails: the store is actually gone.

The last assertion exists because every other check in this scenario is satisfied by an
implementation that records the envelope and skips the removal.

Before this FR the CLI sent the force-references `operator_reason` on every delete — "atomically
delete SourceTaskTemplate binding references" — which cost nothing while the daemon discarded it
and would have been persisted as the operator's stated reason on every plain delete afterwards.

### Negative Fixture

Comment the envelope out rather than deleting it, and set `audit: None`:

```rust
audit: None,
/*
audit: Some(orchestrator_proto::ActionAuditContext {
    reason_code: if force_references {
*/
```

**Measured**: all three CLI tests fail with "an ordinary delete must be audited" / "a cleanup must
be audited", while `grep -c 'operator_resource_delete' crates/cli/src/commands/resource.rs` still
returns **2**. A text-presence check certifies the broken state as working; the assertion is on the
request actually constructed.

---

## Checklist

- [ ] 场景 1：无信封 SecretStore delete 在 `control_action_audit`（点名该表）留下
      `resource.secret_store.delete` / `legacy_client` 行
- [ ] 场景 2：**携带信封**的普通 delete 记下客户端自己的 `reason_code`
      （`operator_resource_delete`，非 `legacy_client`）、operator_reason 与幂等键
- [ ] 场景 3：dry-run delete 不预留信封（与 seed 后基线比对，非与零比对）
- [ ] 场景 4：`enforced` + 无信封 delete 被拒，断言诊断串 `action audit context is required`；
      随后带信封的同一删除成功
- [ ] 场景 5：12 个 kind 各记一行具名动作；11 行 `succeeded`，RuntimePolicy 一行 `failed`
      且诊断点名 `runtimepolicy`；`delete_action_naming` 五项全绿
- [ ] 场景 6：CRD 与自定义资源删除记 `resource.delete` / `resource_manifest`，且仍带 target_id
- [ ] 场景 7：`delete_references` 单行不回归，且**不**同时记 `source.template.delete`；
      `test-source-task-binding.sh` 重跑全绿
- [ ] 场景 8：`resource_delete_envelope_tests` 三项全绿；治理编辑 gate 中 CLI 删除留下
      `operator_resource_delete` 行，且资源确实消失
- [ ] 三个负夹具各自实测目击红，且诊断互不相同（恢复旧守卫 / 塌回通用名 / 注释掉 CLI 信封——
      第三个同时验证 `grep -c` 仍返回 2）
- [ ] `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、
      `cargo fmt --all -- --check` 三项退出码直取为 0

## Known Limits

- **`resource.delete` is one name for three situations.** A CRD, a CRD-defined custom resource, and
  a kind string that resolves to nothing all record it. They are distinguishable by `target_id`
  but not by `action`, so filtering by action name will not separate them. This mirrors apply,
  where a bundle and an unparseable manifest share `resource.apply`.
- **A delete row does not retain the deleted spec.** The tombstone in `resource_versions` writes
  `spec_json = '"deleted"'`, and `canonical_request` is never persisted — `core/src/action_audit.rs`
  stores only its SHA-256. Recovering *what* was deleted means reading the previous
  `resource_versions` row for the same `(kind, project, name)`.
- **The tombstone author is still a constant.** `"daemon-delete"`, or `"project-delete"` for a
  project. The `control_action_audit` row now carries the real actor, so attribution exists — but
  the two tables disagree, and correlating them means joining on timestamp rather than on identity.
  Unchanged by this FR and worth its own ticket.
- **`RuntimePolicy` and `Project` cannot be deleted through the project-scoped path.** Project has
  its own branch and works; RuntimePolicy has none and is refused. Making it deletable is a
  capability change with its own consequences, not an audit fix, and was left out deliberately.
