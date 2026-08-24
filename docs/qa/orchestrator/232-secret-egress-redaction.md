---
lifecycle: active
related_fr: FR-175
self_referential_safe: true
---

# QA: Secret Egress Redaction (FR-175)

Verifies that a SecretStore value never leaves the daemon in cleartext through
any authorized read, and that the two halves of the contract — redaction on the
way out, refusal on the way back in — hold together.

**Module**: orchestrator
**Scope**: `manifest export` (yaml/json), `debug --component config`, the
`get`/`describe` SecretStore paths, and the re-apply refusal
**Scenarios**: 5
**Priority**: High

Design: [DD-194](../../design_doc/orchestrator/194-secret-egress-redaction.md).

---

## Background

The in-memory config holds **decrypted** SecretStore values — the load path runs
`decrypt_resource_spec_json` over every resource — so any surface that renders a
whole config is a place secrets can leave. Two did:

| Egress | Before FR-175 | Least role that reaches it |
|---|---|---|
| `manifest export -o yaml` / `-o json` | cleartext | **read-only** |
| `debug --component config` | cleartext | admin |
| GUI `manifest_export` | cleartext (same RPC) | read-only |

The user guide had documented both as redacted since FR-166. The implementation
never was.

**Every scenario asserts the pair: the value is absent _and_ `[ENCRYPTED]` is
present.** Absence alone is satisfied by a regression that drops SecretStore from
the output entirely — a different bug wearing this bug's colour, and one the
scenarios below were mutation-tested against (see Scenario 5, mutation C).

### Safety

`self_referential_safe: true`. The automated gate starts its **own** daemon under
a `mktemp -d` `ORCHESTRATORD_DATA_DIR` with `HOME` redirected, and never reads or
writes `~/.orchestratord`. No Agent is applied and no task is created, so no
provider binary is reachable. The `cargo test` scenarios touch no daemon at all.

The run does hold a cleartext test secret on disk for its duration, which is why
the gate is registered `manual-runbook` rather than ci-required. The property
itself **is** CI-enforced, at the service boundary, by the `cargo test` scenarios
below — `cargo test --workspace` is a blocking CI job.

---

## Scenario 1: `manifest export` redacts, in both formats, separately asserted

### Steps

```bash
cargo test -p agent-orchestrator --lib \
  manifest_export_redacts_secret_store_values_in_yaml
cargo test -p agent-orchestrator --lib \
  manifest_export_redacts_secret_store_values_in_json
```

### Expected

- Both pass.
- The two are asserted **separately**. They share `builtin_docs` today, so one
  assertion would cover both — but sharing is an implementation detail, and an
  assertion that leans on it is asserting the wrong thing.
- Each fails with a diagnostic naming its own format. Verified by mutation: with
  the redaction in `service::resource::export_manifests` commented out, both
  report `manifest export -o {yaml,json} emitted the SecretStore value in
  cleartext`.

---

## Scenario 2: `debug --component config` redacts; `state` and `dag` emit no config

### Steps

```bash
cargo test -p agent-orchestrator --lib \
  debug_component_config_redacts_secret_store_values
cargo test -p agent-orchestrator --lib debug_state_and_dag_do_not_emit_config
```

### Expected — the `config` component (AC3)

- Passes.
- The test does not stop at a whole-document `contains`. It parses the rendered
  YAML and asserts the **mirrored** SecretStore under `resource_store` carries
  `[ENCRYPTED]`. That half matters because `manifest export` reads only the typed
  `projects[].secret_stores` map: redacting the typed map alone would leave this
  path leaking through the mirror `crd::writeback` keeps, and a whole-file scan
  would report the leak without saying which half broke.
- With the redaction commented out, it reports `debug --component config emitted
  the SecretStore value in cleartext`.

### Expected — `state` and `dag` (AC4)

- Passes, asserting the absence of the secret, of `[ENCRYPTED]`, **and** of the
  store's name. The placeholder assertion is the load-bearing one: these
  components must not begin emitting config, so a *redacted* config appearing
  here is as much a regression as a cleartext one. A test that only looked for
  the secret would go green on it.
- Each also carries a **positive** condition — `state` must still say
  `Debug Information` and `dag` `DAG Debug Information`. Three absences and
  nothing else are satisfied by a component that returned the empty string, which
  is a regression that reads as a pass. "Emits no config" has to be distinguished
  from "emits nothing".

---

## Scenario 3: the `get` paths stay redacted (AC5)

### Steps

```bash
cargo test -p agent-orchestrator --lib get_secret_store_paths_stay_redacted
```

### Expected

- Passes. `get secretstore/<name>` renders `[ENCRYPTED]` and not the value;
  `get secretstores` names the store and emits no value.
- FR-171 made these redact. This scenario is a regression pin against the present
  change, not a re-test of FR-171's work.

---

## Scenario 4: re-applying a redacted export is refused by name, and changes nothing (AC6)

### Steps

```bash
cargo test -p agent-orchestrator --lib \
  applying_a_redacted_export_is_refused_by_name_and_changes_nothing
```

### Expected

- Passes, on **three** conditions rather than one, because an exit code cannot
  say which branch refused:
  1. an error carrying `[secret_value_placeholder_rejected]`,
  2. the diagnostic **names the offending key** (`OPENAI_API_KEY`), so the
     operator knows what to supply,
  3. the stored value is unchanged afterwards, and no config version was
     persisted.
- Condition 3 is what makes redaction safe to ship: without the refusal, a
  redacted export applied back would overwrite real secrets with the literal
  placeholder, silently.

---

## Scenario 5: end to end through a real daemon, and the mutations that prove it bites (AC7)

### Steps

```bash
cargo build -p orchestratord -p orchestrator-cli
bash scripts/qa/test-secret-egress-redaction.sh > /tmp/fr175.log 2>&1; echo $?
```

Capture `$?` directly. Piping into `tail`/`head` reports the pager's status.

Then, for each mutation below: apply it, rebuild the two binaries, re-run, and
restore. Each is **commented out rather than deleted** — deletion is the case the
author had in mind, so it proves less.

| # | Mutation | Where |
|---|---|---|
| A | redaction removed from the export path | `core/src/service/resource/mod.rs`, `export_manifests` |
| B | redaction removed from the debug path | `core/src/service/system.rs`, the `"config"` arm |
| C | `store.data.clear()` instead of writing the placeholder | `core/src/config_load/redact.rs`, `RedactedConfig::new` |

### Expected — the clean run

- Exit 0 and the summary line `Secret egress redaction QA: 13 passed, 0 failed`.
  The summary line's **presence** is part of the expectation: an EXIT trap prints
  `TRUNCATED:` if the run ends before reaching it, because an early abort
  otherwise reads exactly like a completed run.
- 13 checks: the premise, both export formats, `debug --component config` plus its
  mirror count, both no-config debug components, both `get` paths, the refusal and
  the store's survival, the sweep control, and the sweep.
- This scenario exists because acceptance criterion 7 does: the originating ticket
  was first probed at the pure-function layer, which is where the leak looked
  closed and was not.

### Expected — the mutations

Each produces a **verdict** — a summary line and named failures, never a timeout
and never a silent abort:

- **A** → `9 passed, 4 failed`. Both export formats fail by name; the sweep names
  `out/export.yaml` and `out/export.json`; and the re-apply check fails as a
  consequence, since an unredacted export applies back cleanly.
- **B** → `11 passed, 2 failed`. `debug --component config` fails by name and the
  sweep names `out/debug-config.txt`.
- **C** → `9 passed, 4 failed`, and this is the informative one: **the sweep
  passes.** The secret is genuinely absent from every output. Only the
  placeholder-present half fails — `omits the value but carries no [ENCRYPTED]`.
  A gate built on the sweep alone would have certified C as green while every
  operator lost the ability to see which keys a store defines.

---

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | Export redacts in yaml and json, asserted per format (AC1, AC2) | ☑ | mutation A names each format |
| 2 | `debug --component config` redacts incl. mirror; `state`/`dag` emit no config (AC3, AC4) | ☑ | parses the YAML, asserts the resource_store copy |
| 3 | `get` paths stay redacted (AC5) | ☑ | FR-171 regression pin |
| 4 | Re-apply refused by name, store intact (AC6) | ☑ | three conditions, not one |
| 5 | End to end through daemon + CLI, three mutations (AC7) | ☑ | 13 passed, 0 failed; C passes the sweep and fails the pair |

---

## Known limitations

- **Non-CLI/GUI egress is not covered here.** Event payloads, task logs and
  Slack/webhook projections run a separate `redaction_patterns` mechanism
  (DD-17, DD-127). FR-175 did not verify its completeness and neither does this
  document. Single-method, unverified.
- **The raw-serde egress is a call-site rule, not a type.** `export_manifests`
  reaches its output through helpers that accept only a `RedactedConfig`;
  `debug_info` reaches it through `serde_yaml`, which serializes whatever it is
  handed. Scenario 2 and the Scenario 5 sweep hold that site — the compiler does
  not. A *new* surface that serializes `active.config` directly would be caught by
  neither until someone adds a check. See DD-194's known limits.
- **`manifest export` is no longer a restorable backup.** No path now exports a
  config that can be applied back intact. Whether operators need a separate
  encrypted backup-and-restore pair was not evaluated by FR-175.
