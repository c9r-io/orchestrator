---
lifecycle: active
related_fr: FR-171
---

# DD-189: Four Kinds That Could Be Written But Not Read

**Status**: Released
**QA**: [227](../../qa/orchestrator/227-resource-observability-tiers.md)

## The problem

`ResourceKind` has twelve members and `apply` accepts all twelve. Reading did
not. `get_list_resource` recognised eight, `get_single_resource` eight, and both
excluded every builtin kind from the CRD fallback via `is_builtin_kind`, so
**Project, RuntimePolicy, EnvStore and SecretStore** answered `unknown list
resource type` / `unknown resource type`. The Console's catalog served five.

`manifest export` was the only way to see any of the four, and it is a dump: no
kind filter, only `--output`. So the answer to "what projects exist on this
machine" was to serialise every resource in the daemon and read it yourself.

Project is the acute case because it is the first layer of the resource model.
Chapter 02 defines it as the isolation domain and every resource command takes
`--project`, while nothing could enumerate them. `CLAUDE.md` treats "this seems
to require deleting the database" as evidence of missing isolation; being able
to enumerate an isolation domain is the floor of that mechanism, not a
convenience.

## What step 0 changed

The FR was filed with a three-tier table — twelve apply, eight list, **five
describe** — and the describe column was wrong. `describe_builtin_resource` has
five typed arms and returns `Ok(None)` for everything else, at which point
`describe_resource` falls back to `get_resource` → `get_single_resource`, which
handles eight. `describe` covered eight. The FR's "seven gaps (four unlistable
plus three undescribable)" was four gaps.

The error's shape is worth naming because the gate this FR proposes could have
inherited it: **an arm count was read as a command's capability**. Requirement 2
was rewritten to compute support by entry reachability, and acceptance gained a
mutation that turns the `_ => Ok(None)` arm into an error — a gate counting arms
stays green under it.

Two of the FR's four "unverified" items closed. No alias is available at one
entry point only: `get_list_resource` takes singular and plural while
`get_single_resource` takes singular, which is a deliberate list/get split.
And there is no enumeration source besides `manifest export` — checked against
the 122 gRPC RPCs (only the workflow-store `StoreList`, a different concept), the
nine `debug` leaves (all sandbox probes) and the GUI's Tauri commands (only
`secret_key_*`).

## The four rulings

| Kind | List | Single read | Why |
|---|---|---|---|
| EnvStore | yes | yes | `ProjectConfig` already projects it; one arm |
| SecretStore | yes | yes | listing yields names only; a single read redacts |
| Project | yes | yes | enumeration reuses `export_manifest_resources`' answer |
| RuntimePolicy | **no** | yes | a resolved singleton, not a collection |

### RuntimePolicy was already decided, twice

This is the ruling that looks most like a judgement and is least like one. The
codebase had encoded it in two independent places and never written the reason:

- `builtin_crd_definitions()` marks RuntimePolicy `CrdScope::Singleton`, and it
  is the **only** builtin so marked. The enum's own doc comment reads "Singleton
  resources such as RuntimePolicy".
- `is_builtin_alias` reserves a plural name against CRD use for eleven of twelve
  kinds and reserves only the singular for RuntimePolicy — verified kind by kind.

Behaviourally: `get_from_project` returns the effective policy for *any* name,
composing project → `_system` → defaults at read time, and `delete_from_project`
is hardcoded `false`. Stored rows do exist, one per scope, since a project may
override `_system`. Listing them would show the overrides while leaving the
effective policy — the only thing that governs a task — unstated. So the single
read answers and the list does not exist.

The test asserts the ruling against the declared scope rather than deriving from
it. §4.4's rule is to derive expectations from a ledger and never restate them,
but a judgement has no ledger; here the two facts are independent and are
cross-checked, so changing a scope fails naming the kind instead of silently
carrying the test along.

### The rendering paths are out of scope

`describe` has three: five kinds render through `RegisteredResource::to_yaml()`,
three through `get_single_resource`, which itself tries `resource_store` (with
labels) before falling back to the in-memory config (without). Which one you get
depends on the kind and on whether the resource is in the store. This affects all
eight readable kinds, not the four this FR is about, and converging it requires
first ruling on whether `describe` should show labels — a separate product
question. Recorded here, not fixed here.

## Two defects the work surfaced

Neither was reachable before, so neither is a regression; both would have shipped
as new ones.

**A silent namespace mismatch.** `get_list_resource` resolved labels with
`resource_store.get_namespaced(crd_kind, project_id, name)`, which is a raw
`{kind}/{project}/{name}` lookup that does not resolve scope. Project is the one
kind `is_project_scoped` excludes — it lives under `_system` — so a label query
against projects would have matched nothing and dropped every row **without an
error**. The namespace is now resolved through `is_project_scoped`, which leaves
the other eleven unchanged.

**A missing instance reported as an unknown type.** Serving the new kinds by
asking `describe_builtin_resource` alone conflates the two cases it cannot tell
apart, since `Ok(None)` means both. `get project/absent` said `unknown resource
type: project` — the exact conflation requirement 3 exists to remove. The kind is
now resolved separately through the builtin CRD registry, which already carries
every alias, with plural forms excluded: `get workspaces/foo` keeps saying
"unknown resource type" rather than asserting `Workspace not found: foo` about a
name never looked up. Widening the matcher to reach the directory-style case and
then asking what it newly admits is DD-170's shape 10, applied here.

## The trap this change would otherwise have shipped

Reads redact secret values. A described resource stays an apply-compatible
manifest by existing design — `get_resource_supports_named_queries_describe_and_selector_helpers`
asserts it parses. Those two facts compose into describe → edit → apply
overwriting real secrets with the literal `[ENCRYPTED]`, silently and
irreversibly.

`SecretStoreResource::validate` now rejects the placeholder with
`[secret_value_placeholder_rejected]`, naming the offending key. Deliberately
**not** implemented as "placeholder means keep the stored value": that makes a
manifest's meaning depend on prior state, and these manifests are declarative.
The check is equality, not `contains` — a real secret that embeds the placeholder
text is still a real secret, and a fixture holds that case.

The diagnostic was corrected once during authoring. It said omitting a key leaves
it unchanged; `apply_to_map` inserts the whole incoming value, so omitting
**deletes**. A user-facing error had nearly described data loss as the safe move.

## Assertion design

The support matrix probes `get_resource` and `describe_resource` — the functions
a command calls — never the private arms. Recognition is probed with **no
instance applied**, which is what separates knowing a type from having one: a
recognised singular read of an absent name says `<Kind> not found`, an
unrecognised one says `unknown resource type`.

Verified by mutation in three states: green, `_ => Ok(None)` returning an error,
green again. **The first version of the test passed the mutation.** Its
recognition predicate was "the error text is not `unknown resource type`", so a
different error read as recognised — §4.4 shape 1, written into the test built to
guard against it. It now compares the describe and get outcomes for equality,
which needs no taxonomy of error text and fails naming the kind and both sides.
Absent instances are compared deliberately; a present resource renders
differently by the design above.

The catalog refusals are asserted by message, not exit code: four refusals that
differ only in status cannot tell an operator which of four reasons applies.

Two more mutations were run, and one of them corrected a claim this record
originally made.

**Removing a supported arm.** Deleting the EnvStore arm from `get_list_resource`
while the ruling still claims it turns two tests red, naming the kind and the
entry point: `list recognition for EnvStore via 'get envstores'`. The catalog test
stays green, correctly — the two surfaces are independent, and the test set says
so.

**A thirteenth variant.** The first draft of this record said a new
`ResourceKind` fails to compile *in the ruling*, "the compiler being the
derivation from the enum". Run, that is not what happens: adding a variant fails
in `orchestrator-config`'s own exhaustive match at `cli_types.rs:57`, which
compiles before the crate holding the ruling, so the ruling's barrier is never
reached and the diagnostic names a different file. The property — a kind cannot
join the enum silently — holds through a chain, and the ruling is a *later* link
in it, not the first.

Exercising the ruling's own barrier needs the mutation from the other side:
removing `ResourceKind::Trigger` from `adjudicated()` fails at
`core/src/service/resource/tests.rs:791` with `ResourceKind::Trigger not
covered`. That is the check this record can claim, and it is the one claimed.

## Known limits

- **The two registries are not mirrors, and it is user-visible.** `Trigger` is a
  `ResourceKind` with no builtin CRD definition; the registry separately carries
  `WorkflowStore` and `StoreBackendProvider`, which are not `ResourceKind`
  variants — the drift DD-182 found in the guide's built-in kinds list. A
  consequence reaches users: a `triggers` catalog query is told *why* it is
  refused but cannot be told the canonical spelling, because the lookup that
  yields canonical names has no Trigger entry. The scope cross-check runs over the
  intersection and asserts the skipped set is exactly `["Trigger"]`.
- **`manifest export` still emits SecretStore values in cleartext.** Confirmed by
  execution and by tracing the chain; carried as
  `docs/ticket/secretstore_manifest-export_scenario0_260817_224518.md` and not
  touched here. The read path added by this FR is redacted, which is the one leak
  this work closes.
- **The ruling's compile-time barrier is not the first one a new kind meets.**
  `orchestrator-config` rejects an unhandled variant before the crate holding the
  ruling compiles, so a developer adding a kind is told about `cli_types.rs`
  first and reaches the ruling only after satisfying the upstream matches. Nothing
  verifies that they do reach it; the evidence here is that the ruling rejects a
  kind removed *from it*, not that it is the barrier a new kind hits.
- **The Console's kind list is a hardcoded array of eight.** The daemon is
  authoritative and an unserved kind now gets an explanatory refusal, so a stale
  list is visible rather than silent — but there is no RPC advertising which
  catalog types exist, so the array can fall behind without failing anything.
- **`config.projects`' production build path was never located.** `apply` computes
  a `project_id` for namespaced storage without writing `config.projects`
  (`resource/helpers.rs:118-127`), and whether applying a resource under
  `--project foo` makes `foo` appear in the enumeration is therefore unknown.
  Reusing `export_manifest_resources`' loop sidesteps the question at the cost of
  inheriting its semantics rather than ruling on them.
- **Project deletion is unaddressed.** What deleting a project means — cascade,
  refuse, or block-and-report — is FR-168's class of question and needs its own
  ruling.
