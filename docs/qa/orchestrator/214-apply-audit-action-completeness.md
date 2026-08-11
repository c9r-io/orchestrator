---
lifecycle: active
related_fr: FR-164
self_referential_safe: true
---

# Orchestrator - Apply Audit Action Completeness

**Module**: Orchestrator
**Scope**: Unconditional apply auditing, per-kind action naming, enforced-mode conformance, and secret-rotation attribution
**Scenarios**: 5
**Priority**: High

## Background

`audited_mutation` in `crates/daemon/src/server/resource.rs` required an audit
envelope, raw driver args, or one of two Source kinds. Two consequences:

1. An envelope-less apply of a SecretStore, Workflow, Trigger or RuntimePolicy
   wrote **no `control_action_audit` row**.
2. Because the first disjunct was `context.is_some()`, `action_audit::begin` was
   never reached for an envelope-less caller — so the `enforced`-mode rejection
   in `resolve_context` was unreachable in exactly the case it exists to refuse.
   Enabling `action_audit_mode: enforced` produced a false assurance.

Separately, only three kinds had named actions; the other nine shared the
generic `resource.apply`.

**Read the audit table by name.** Every successful non-dry-run apply also writes
`resource_versions` and `orchestrator_config_versions` rows whose `author` is the
constant `"daemon-apply"`. An assertion phrased as "an audit row exists" is
satisfied by a row written whether or not this behaviour works; these scenarios
therefore assert against `control_action_audit` specifically.

## Verification Commands

```bash
cargo test -p orchestratord resource_audit_tests   # behavioural, real OrchestratorServer
cargo test -p orchestratord apply_action_naming    # per-kind mapping
cargo test -p orchestrator-cli secret_rotate_tests # rotation envelope
./scripts/qa/test-expert-resources-governed-editing.sh
./scripts/qa/test-source-task-binding.sh
```

The behavioural tests live in the daemon crate, not `crates/integration-tests`.
That harness's `apply` reimplements the RPC by calling `apply_manifests`
directly (`crates/integration-tests/src/lib.rs:1281`), so it never enters the
audit path and cannot observe any of this.

---

## Scenario 1: Envelope-less Mutating Apply Is Audited

### Steps

1. Run `cargo test -p orchestratord resource_audit_tests::envelope_less_secret_store_apply_is_audited`.
2. Inspect the assertion target: `control_action_audit`, via
   `AsyncActionAuditRepository::list`.

### Expected

- Exactly one row with `action` = `resource.secret_store.apply`,
  `target_type` = `secret_store`, `target_id` = `SecretStore/fr164-store`.
- `reason_code` = `legacy_client` — the envelope-less client is recorded, not
  dropped.
- `status` = `succeeded`.

### Negative Fixture

Restore the pre-FR-164 disjunction in `audited_mutation`:

```rust
let audited_mutation = !dry_run
    && (context.is_some() || contains_driver_raw_args || /* two Source kinds */);
```

Four tests must fail. This scenario reports `left: 0, right: 1` with
"envelope-less SecretStore apply must leave exactly one named audit row".

---

## Scenario 2: Dry Run Stays Unaudited

### Steps

1. Run `cargo test -p orchestratord resource_audit_tests::dry_run_apply_is_not_audited`.

### Expected

- A `dry_run: true` apply reserves no envelope; the audit table is empty.
- This is the one case the unconditional `!dry_run` must still exclude, and it
  is asserted separately so that "audit everything" cannot be over-applied.

---

## Scenario 3: Enforced Mode Rejects An Envelope-less Apply

### Steps

1. Run `cargo test -p orchestratord resource_audit_tests::enforced_mode_rejects_envelope_less_apply`.
2. The test seeds a RuntimePolicy with `action_audit_mode: enforced`, then
   applies a SecretStore with `audit: None`.

### Expected

- The apply is rejected with `InvalidArgument` and the diagnostic
  `action audit context is required`.
- No `resource.secret_store.apply` row is recorded for the rejected attempt.

The **diagnostic string** is asserted, not only the gRPC code. `InvalidArgument`
is also returned by manifest parse failures and project mismatches, so a
code-only assertion would pass on an apply that never reached the audit layer —
which is the pre-FR-164 behaviour this scenario must fail against.

### Negative Fixture

Under the restored old disjunction the envelope-less apply **succeeds**, and the
test fails with "enforced mode must reject an envelope-less apply". That failure
is the bypass, reproduced.

---

## Scenario 4: Every ResourceKind Records Its Own Action

### Steps

1. Run `cargo test -p orchestratord resource_audit_tests::every_resource_kind_records_its_named_action`.
2. Run `cargo test -p orchestratord apply_action_naming`.

### Expected

- Twelve kinds applied in dependency order, each recording exactly one row with
  its own `action` and `target_type`, `status` = `succeeded`,
  `reason_code` = `legacy_client`.
- `resource.<kind>.apply` for ten kinds; `source.template.apply` and
  `source.binding.apply` keep their pre-FR-164 spellings because DD-111, QA 157
  and stored rows already name them.
- Action names and target types are pairwise distinct, and none is the generic
  fallback.

### How The Set Is Derived

`apply_action` and `apply_target_type` are exhaustive matches with **no `_`
arm**, so a thirteenth `ResourceKind` variant fails to compile rather than
silently inheriting `resource.apply`. That compile-time obligation is the
derivation from the enum; `covers_every_variant` then fails until the new
variant is added to the test's list too, so the list cannot fall behind the enum
it claims to enumerate. Neither the array nor a count is load-bearing on its own.

### Negative Fixture

Collapse one arm to the generic name — `ResourceKind::SecretStore =>
"resource.apply"`. Chosen over deleting the function because a half-reverted
match is the realistic regression, not a missing symbol. Three tests fail, with
diagnostics that name the kind: "SecretStore still falls back to the generic
action name" and "resource.secret_store.apply must record exactly one row" —
distinct from Scenario 1's diagnostic, so the log says which way it broke.

---

## Scenario 5: Secret Rotation Is Attributable

### Steps

1. Run `cargo test -p orchestrator-cli secret_rotate_tests`.

### Expected

- `secret_rotate_apply_request` carries `audit: Some(..)` with `reason_code`
  `operator_secret_rotate` and an idempotency key prefixed
  `cli-secret-rotate-`.
- Successive rotations get distinct retry identities, so a second rotation is
  not rejected as a replay of the first.
- `dry_run` is false — a rotation is a mutation.

`orchestrator tool secret-rotate` rewrites a SecretStore value. Before FR-164 it
sent `audit: None`, so the only trace was an unattributable `resource_versions`
row, and under enforced mode it slipped past the rejection instead of being
refused. `secret_key_audit` does not cover this: it records encryption-key
lifecycle, not store-value writes.

### Negative Fixture

Comment the envelope out rather than deleting it, and set `audit: None`:

```rust
audit: None,
// audit: Some(orchestrator_proto::ActionAuditContext {
//     reason_code: "operator_secret_rotate".to_string(),
```

Both tests fail with "secret rotation must carry an audit envelope", while
`grep -c 'operator_secret_rotate' crates/cli/src/commands/tool.rs` still returns
**2**. A text-presence check would certify the broken state as working; the
assertion is on the request actually constructed.

---

## Checklist

- [ ] 场景 1：无信封 SecretStore apply 在 `control_action_audit`（点名该表）留下
      `resource.secret_store.apply` / `legacy_client` 行
- [ ] 场景 2：dry-run 不预留信封（审计表为空）
- [ ] 场景 3：`enforced` + 无信封 apply 被拒，断言诊断串
      `action audit context is required` 而非 gRPC code
- [ ] 场景 4：12 个 kind 各记一行具名动作，`apply_action_naming` 四项全绿
- [ ] 场景 5：`secret_rotate_tests` 两项全绿（信封在场 + 重试身份互异）
- [ ] 三个负夹具各自实测目击红，且诊断互不相同（恢复旧析取项 / 塌回通用名 /
      注释掉轮换信封——第三个同时验证 `grep -c` 仍返回 2）
- [ ] `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、
      `cargo fmt --all -- --check` 三项退出码直取为 0
- [ ] `test-expert-resources-governed-editing.sh` 与 `test-source-task-binding.sh`
      重跑全绿（前者的审计断言已随具名动作改写）

## Known Limits

- **A bundle carries no per-document kind list.** A multi-document apply records
  one aggregate row (`resource.apply` / `resource_manifest`, `target_id` a
  content hash). `canonical_request` is by design never persisted —
  `core/src/action_audit.rs:196-198` stores only its SHA-256 — so an inventory
  placed there would be invisible, and being derived from content the hash
  already covers it would add no entropy either. Recovering what a bundle
  touched requires correlating `resource_versions.created_at` with
  `control_action_audit.created_at`. A queryable inventory would need a new
  persisted column plus a proto field.
- **An unparseable manifest also records the generic name.** It has no
  resolvable identity, so `resource.apply` / `resource_manifest` is correct
  rather than a gap — but it means filtering by a per-kind action name will not
  return the full sequence for a gate that also exercises invalid input. List
  without `--action` in that case; `test-expert-resources-governed-editing.sh`
  does exactly this.
- **`kind_as_str_covers_all_resource_kinds`** in `core/tests/integration_test.rs`
  asserts only three of the twelve kinds despite its name. It is adjacent to this
  work and was left as found; the naming coverage this FR needed is asserted by
  `apply_action_naming` instead.
