---
lifecycle: active
related_fr: FR-171
self_referential_safe: true
---

# Orchestrator - Resource Observability Tiers

**Module**: Core resource service / CLI read surface / Console resource catalog
**Scope**: that every `ResourceKind` is reachable through the read entry points its
ruling grants it and unreachable through the ones it does not; that support is
measured by entry-point reachability rather than by the arm count of any one
function; that a secret value never leaves through a read and a redacted manifest
cannot be applied back; and that the refusals an unserved kind receives are told
apart by their diagnostics
**Design**: [DD-189](../../design_doc/orchestrator/189-resource-observability-tiers.md)
**Safety**: read-only plus in-process `TestState` fixtures. No daemon is started,
no socket is bound, and `~/.orchestratord` is never touched — each test builds its
own temp data directory. Safe to run against this repository.

## Prerequisites

```bash
cargo --version   # workspace toolchain; every scenario below is a cargo test
```

No scenario needs a running daemon. Every property here is decidable in-process,
so spawning one buys nothing and a leaked daemon is the failure mode avoided.

> **On apparent hangs.** A fresh test binary is ~63 MB and macOS revalidates its
> signature on first execution; the process blocks in `_dyld_start` before `main`
> and prints nothing. A run that looks stuck for tens of seconds after `Running
> unittests` is usually that, not a deadlock — `sample <pid>` showing only
> `_dyld_start` confirms it. Check the **test binary's** pid, not the wrapper
> shell's: `pgrep -f` also matches the shell whose command line contains the
> binary name, and that shell is legitimately asleep waiting on its child.

## Scenario 1: reachability matches the ruling, and the measurement is not an arm count

**Steps**

```bash
cargo test -p agent-orchestrator --lib resource_observability_matrix -- --nocapture
```

Then apply this mutation to `core/src/service/resource/query.rs` and re-run:

```rust
// in describe_builtin_resource, replace:
_ => return Ok(None),
// with:
_ => return Err(classify_resource_error(
    "resource.describe", anyhow::anyhow!("MUTATION unknown kind"))),
```

**Expected result**

Unmutated: exit 0, 5 tests. For each of the twelve `ResourceKind` variants the
matrix probes `get <plural>`, `get <singular>/<name>` and `describe
<singular>/<name>`, and compares recognition against the ruling — twelve
recognised for a single read, eleven for a list, RuntimePolicy excluded from the
list. Recognition is probed with **no instance applied**, which is what separates
knowing a type from having one: a recognised singular read of an absent name says
`<Kind> not found`, an unrecognised one says `unknown resource type`.

Mutated: **exit 101**, naming the kind and both outcomes:

```
`describe sourcetasktemplate/fr171-absent-probe` and `get sourcetasktemplate/fr171-absent-probe`
must return the same outcome for SourceTaskTemplate
  left: Err("resource.describe: MUTATION unknown kind")
```

Restore the arm; the suite returns to exit 0. All three states were run.

This mutation exists because the FR was filed on an error of exactly this shape —
five typed arms read as five supported kinds — and because **the first version of
this test passed it**. That version asked only whether describe's error mentioned
`unknown resource type`, so a different message read as recognised. If this
mutation ever goes green again, the assertion has decayed into a proxy.

Two further mutations were run against this module.

Deleting the EnvStore arm from `get_list_resource`, while the ruling still claims
it, turns two tests red naming the kind and the entry point — `list recognition
for EnvStore via 'get envstores'`. The catalog test stays green, correctly: the
two surfaces are independent.

Removing `ResourceKind::Trigger` from `adjudicated()` fails to compile at
`core/src/service/resource/tests.rs:791` with `ResourceKind::Trigger not
covered`, because the ruling is a wildcard-free match.

> **What this does not show.** Adding a *thirteenth* variant also fails to
> compile, but in `orchestrator-config`'s own exhaustive match
> (`cli_types.rs:57`), which builds first — the ruling's barrier is never
> reached and the diagnostic names another file. A kind cannot join the enum
> silently, but the ruling is a later link in that chain, not the first, and
> nothing verifies a developer reaches it.

Also asserted here: the ruling's `list` column agrees with `CrdScope !=
Singleton`; RuntimePolicy is the only `Singleton` builtin; and the cross-check's
skipped set is exactly `["Trigger"]`, because the two registries are not mirrors
and a comparison set that shrinks silently reports success over ground it stopped
covering.

## Scenario 2: the four added kinds actually return resources

**Steps**

```bash
cargo test -p agent-orchestrator --lib \
  resource_observability_matrix::kinds_added_by_fr171_are_actually_retrievable
```

**Expected result**

Exit 0. A seeded EnvStore and SecretStore appear in their list pages and come back
from a single read; `get projects` contains the default project; `get
runtimepolicy/<any name>` returns a RuntimePolicy; and `get runtimepolicies` fails
with `unknown list resource type`.

The last two are asserted together deliberately. Either alone is satisfiable while
the ruling is broken: a listable RuntimePolicy passes the first, and an unreadable
one passes the second.

## Scenario 3: secrets do not leave through a read, and a redacted manifest cannot be applied back

**Steps**

```bash
cargo test -p agent-orchestrator --lib \
  resource_observability_matrix::kinds_added_by_fr171_are_actually_retrievable
cargo test -p agent-orchestrator --lib secret_store::tests
```

**Expected result**

Exit 0 for both; 17 tests in the second.

`get secretstore/api-keys` output does **not** contain the seeded value, **does**
contain `[ENCRYPTED]`, and **does** contain the key name. All three are required:
asserting only the value's absence would also pass if SecretStore dropped out of
the read path entirely, which is the broken state this excludes. The placeholder
assertion distinguishes redacted from missing; the key-name assertion is what
makes the read useful, since inspecting which keys a store defines is the question
a read answers. A paged catalog row is separately asserted to carry no value.

On the apply side, a `spec.data` value **equal to** `[ENCRYPTED]` fails validation
with `[secret_value_placeholder_rejected]` and the message names the offending
key; a value that merely **contains** the placeholder text passes. The second is
the inverted form of the first — a real secret embedding that substring is still a
real secret, and a `contains` check would make a legitimate value unstorable, an
over-reach that costs nothing until someone writes that value.

Why this pair belongs in one scenario: reads redact, and a described resource
stays an apply-compatible manifest by existing design, so describe → edit → apply
would otherwise overwrite real secrets with the literal placeholder. Neither half
is safe without the other.

## Scenario 4: the catalog pages eight kinds and explains the rest

**Steps**

```bash
cargo test -p agent-orchestrator --lib \
  resource_observability_matrix::the_resource_catalog_pages_eight_kinds_and_explains_the_rest
```

**Expected result**

Exit 0. `envstores`, `secretstores` and `projects` page successfully, and four
refusals are distinguished **by message**:

| Query | Refusal |
|---|---|
| `runtimepolicies` | says `singleton` |
| `sourcetasktemplates` | names `SourceTaskTemplate` and `typed renderer` |
| `triggers` | names `triggers` and `typed renderer`, but no canonical kind |
| `nonsuchkind` | `unknown resource catalog type` |

The `triggers` row is asserted as it actually behaves. `Trigger` is a
`ResourceKind` with no builtin CRD definition, so the lookup that supplies
canonical names cannot supply one; the refusal gives the reason without the
spelling. Asserting a canonical name there would assert something the code cannot
produce.

Exit codes are insufficient evidence here: four refusals that differ only in
status cannot tell an operator which of four reasons applies.

## Scenario 5: documentation states both rulings, in both languages

**Steps**

```bash
rg -n 'get projects' docs/guide/02-resource-model.md docs/guide/zh/02-resource-model.md
rg -n 'runtimepolicies' docs/guide/02-resource-model.md docs/guide/zh/02-resource-model.md
./scripts/qa/test-docs-reality-alignment.sh
./scripts/qa/test-markdown-link-integrity.sh
./scripts/qa/test-error-code-glossary.sh
```

**Expected result**

Chapter 02 §5 shows how to list and read projects and states that Project is not
project-scoped; §6 states that RuntimePolicy is a resolved singleton, that there
is deliberately no list, and why listing stored rows would not answer the question
anyone asks. Both languages carry both. All three gates exit 0, the glossary gate
confirming EN and ZH document the same code set including
`secret_value_placeholder_rejected`.

## Checklist

- [ ] Scenario 1: all twelve kinds recognised exactly where the ruling grants it,
      probed through public entry points with no instance applied
- [ ] Scenario 1: the `_ => Ok(None)` mutation turns the suite red with a
      diagnostic naming the kind and both outcomes, and green again on restore
- [ ] Scenario 1: removing a supported list arm turns the suite red naming the
      kind and entry point; removing a kind from the ruling fails to compile
      naming that kind
- [ ] Scenario 1: the ruling agrees with the declared `CrdScope`, RuntimePolicy is
      the only `Singleton`, and the skipped set is exactly `["Trigger"]`
- [ ] Scenario 2: EnvStore, SecretStore and Project return seeded resources from
      both list and single read; RuntimePolicy reads and does not list
- [ ] Scenario 3: a SecretStore read omits the value, shows `[ENCRYPTED]`, and
      shows the key name; a catalog row carries no value
- [ ] Scenario 3: a placeholder value is rejected naming the key, and a value
      merely containing the placeholder is accepted
- [ ] Scenario 4: three added kinds page, and four refusals are distinguished by
      message rather than by exit status
- [ ] Scenario 5: chapter 02 states both rulings in EN and ZH; docs-reality,
      link-integrity and error-code-glossary gates green

## Known limitations

- `manifest export` still emits SecretStore values in cleartext. Tracked as
  `docs/ticket/secretstore_manifest-export_scenario0_260817_224518.md`; out of
  scope here and asserted nowhere in this document.
- No scenario drives the real CLI or the Console. Every property is asserted at
  the service layer, so a defect confined to the gRPC handler, the CLI printer or
  the Console's fetch would not be caught. The Console's kind list in particular is
  a hardcoded array with nothing comparing it to what the daemon serves.
- `describe`'s three rendering paths are unasserted. Scenario 1 compares describe
  and get only for **absent** instances, because for a present resource the two
  render differently by design — the convergence FR-171 excluded.
