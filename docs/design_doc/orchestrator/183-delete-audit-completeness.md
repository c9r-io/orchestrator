---
lifecycle: active
related_fr: FR-167
---

# Orchestrator - Delete Audit Action Completeness

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-167 delete-path action naming, the discarded envelope, and the kind-dispatch defects underneath  
**Related QA**: `docs/qa/orchestrator/221-delete-audit-completeness.md`  
**Created**: 2026-08-13  
**Last Updated**: 2026-08-13

## Background

FR-164 closed the apply half of the gap DD-111 describes: every non-dry-run apply
now reserves an envelope, and every kind has its own action name. Its Phase 5
self-check asked §4.4's question one level up — *what state would satisfy every
criterion while the goal is unmet?* — and the answer was **delete**. Same handler
file, same audit layer, an irreversible mutation, and a guard weaker than the one
apply had before the fix:

```rust
let attempt = if force_references || is_source_task_binding { ... } else { None };
```

Eleven of the twelve kinds left no `control_action_audit` row when deleted.

The interesting part is the second clause, or rather its absence. Apply's old
condition began with `context.is_some()`, so a caller who did the right thing was
still audited; the gap was confined to callers who sent nothing. Delete's
condition had no such disjunct, so an envelope was received, parsed, and dropped.
That is not a hypothetical caller: `crates/cli/src/commands/resource.rs` fills in
`ActionAuditContext` on **every** delete. The default path was the discarded one,
and had been since the envelope was introduced.

And because `begin` is the only caller of `resolve_context`, the whole of
`action_audit_mode: enforced` was unreachable for delete. The mode neither
recorded an ordinary delete nor refused it — on the one operation with no undo.

## Key Design 1: The condition is `!dry_run`, and nothing else

Identical in form to FR-164's `audited_mutation`, and deliberately so: the two
verbs of the same surface should not have different reasons to be audited. A dry
run is not a mutation and stays out, asserted on its own so that "audit
everything" cannot be over-applied into a preview.

Everything else that used to be in the condition moves into the *naming*, which
is where it belonged: `force_references` selects the action, it does not select
whether there is one.

## Key Design 2: One vocabulary family per kind

| Kind | apply (shipped) | delete (this FR) |
|---|---|---|
| ten regular kinds | `resource.<snake>.apply` | `resource.<snake>.delete` |
| SourceTaskTemplate | `source.template.apply` | `source.template.delete` |
| SourceTaskBinding | `source.binding.apply` | `source.binding.delete` (shipped) |
| unresolvable kind | `resource.apply` | `resource.delete` |
| `--force-references` | — | `delete_references` (shipped, unchanged) |

FR-167 as written proposed `resource.<snake_kind>.delete` throughout, with
`source.binding.delete` as the single exception because it is already in stored
rows. Taken literally that gives `resource.source_task_template.delete` beside
`source.template.apply`, and the cost is invisible until someone needs it: an
auditor asking "everything that happened to this source template" would have to
know to query two prefixes, and querying one would return half the story with no
indication that it was half. Consistency of the rule was traded for consistency
of the family, which is the thing a reader actually uses.

`apply_and_delete_share_a_family` asserts the property rather than trusting it —
it compares everything before the final `.` of both names for every kind. A
thirteenth variant that broke the pairing would fail there rather than being
noticed years later by whoever ran the query.

`delete_and_apply_names_never_collide` covers the other direction. Both verbs
share one `action` column, so a collision would make `--action` return two
different operations with no way to separate them again — and unlike a missing
name, a collision is not visible in either match on its own.

**These names are permanent.** DD-177 Key Design 3 records why: a recorded action
name cannot be renamed without falsifying stored history, which is what forced
FR-164 to keep two irregular apply spellings. FR-166 made the same commitment
from the other side — DD-182 Decisions 2 and 3 record `resource.env_store.apply`
and `resource.trigger.apply` as the reason the EnvStore/SecretStore merge and the
Trigger split are permanently off the table. That reverse requirement was the one
FR-167 asked FR-166 to honour, and it was honoured; the dependency the FR
declared is therefore discharged, not deferred.

## Key Design 3: `target_type` is shared, because it names the object

`apply_target_type` became `resource_target_type` and both verbs read it. The
alternative — a `delete_target_type` twin — would have started identical and been
free to drift, and drift in this table is silent: a row stored under the wrong
target type is still a row.

The reuse was safe to make because the three values delete already stored
(`source_task_binding` for the binding delete, `source_task_template` and
`trigger` for the two cleanup targets) are exactly what the apply table already
returns for those kinds. Nothing moved. `shared_target_types_match_the_values_delete_already_stored`
pins those three so a future edit to the shared table cannot move a delete row's
target type while only the apply tests are being watched.

## Key Design 4: Kinds outside the enum are named, not skipped

`crd`, `customresourcedefinition` and every CRD-defined custom kind are deletable
and were as unaudited as the rest. The FR did not mention them, which is itself
an instance of the shape it was written about: a scope drawn around the twelve
enum variants is a fact about the enum, not about the delete surface.

They record `resource.delete` / `resource_manifest` — one new name rather than
two more per-kind names, and symmetric with apply, where a bundle or an
unparseable manifest already records `resource.apply` / `resource_manifest`. The
row still carries the `target_id` it was asked to delete, so the generic action
does not mean an anonymous row.

## Key Design 5: One alias table, because two fail silently

The delete path had **four** hand-written alias tables — `delete_resource_by_kind`,
`canonical_project_kind`, `delete_resource_from_project`, and the daemon's
`describe_summary` — and they had already diverged (`runtime_policy` was in none
of them, `stt`/`stb` in some).

For this FR that stops being a tidiness question. The audit action name is
resolved from a kind string; the removal is dispatched from the same string. If
the two halves disagree, the resource is deleted and the row records the generic
name — no error, no warning, nothing in any log. It is §4.4 shape 2 with the
symptom removed.

`resource::kind_aliases` is now the single table, exhaustive over `ResourceKind`
with no `_` arm, and `canonical_project_kind` resolves through it rather than
carrying a copy. `every_alias_of_a_project_scoped_kind_is_accepted_by_both_halves`
is derived from that table rather than listing strings, so a future alias joins
the assertion without anyone remembering to add it.

## What the behavioural tests found

Both defects below were found by *writing an assertion that deletes something*,
not by reading the code. Neither had a test, because nothing had ever deleted
these kinds through the production path.

### SecretStore was undeletable, and could delete an EnvStore instead

`delete_resource_from_project` routed `secretstore | secret-store | secret_store`
into `proj.env_stores`, sharing an arm with the EnvStore aliases. The two kinds
have **separate maps** — `ProjectConfig::env_stores` and
`ProjectConfig::secret_stores` — so:

- `orchestrator delete secretstore/x --force` reported `secretstore/x not found in
  project` for a store that existed. SecretStores could not be deleted at all.
- When an EnvStore happened to share the name, the delete removed **that**, while
  `canonical_project_kind` returned `SecretStore` — so the tombstone row, the
  deletion guard and the resource-row delete all named a kind nothing had
  touched.

The dry-run existence check carried the same conflation, and additionally had no
`ExecutionProfile` arm at all, so it previewed profiles as absent through the
`_ => false` catch-all.

Two things are worth keeping from this. The first is that FR-166 had, three days
earlier, written into the guide that "moving a value between the two is a delete
and re-apply" — correct advice about a code path that could not perform it. A
document can be right about intent and wrong about capability, and nothing
connects the two unless something executes the sentence.

The second is the mechanism. A string-keyed `match` lets two kinds share an arm,
and sharing an arm looks like deduplication. Dispatching on the resolved
`ResourceKind` with no `_` arm makes it structurally impossible: `EnvStore` and
`SecretStore` are separate patterns and cannot be written as one without saying
so. That is why the repair changed the dispatch rather than the arm.

### The `_ => false` arm was hiding the gap

Both halves now match on `Option<ResourceKind>` with every variant named,
including `Project | RuntimePolicy` in the refusing position. A catch-all would
have absorbed the missing `ExecutionProfile` arm forever, and it did.

## The RuntimePolicy asymmetry

`RuntimePolicy` cannot be deleted. There is no `ProjectConfig` map holding one, so
`canonical_project_kind` refuses it with `unknown resource type for project
delete: runtimepolicy`. FR-167's requirement 2 asked for twelve kinds with named
delete actions and behavioural assertions; twelve *successes* is not available.

The resolution is that the audit row is reserved before execution, so the attempt
records `resource.runtime_policy.delete` with `status = failed`, and the test
asserts exactly that — including the diagnostic. An attempt to delete a project's
runtime policy is precisely what an audit trail exists to record.

The alternative considered and rejected was making it deletable. That is a
capability change with consequences of its own (what does a project without a
runtime policy do?), and it is not an audit fix. Folding it into this FR would
have been the scope-widening that the naming decisions above are all trying to
avoid.

## Alternatives considered

- **A `delete_target_type` table.** Rejected: see Key Design 3. Two tables that
  begin identical and are free to drift, in a column where drift is silent.
- **`resource.crd.delete` and `resource.custom_resource.delete`.** Rejected: two
  more permanent names, and asymmetric with apply, which records CRD applies
  under the generic `resource.apply`. The generic name is not a gap here — these
  targets genuinely have no `ResourceKind`.
- **Leaving CRD deletes unaudited.** Rejected: it reproduces the FR's own defect
  one level down, and the enforced mode would stay unreachable for them.
- **Folding `delete_references` into the per-kind surface.** Rejected: a cleanup
  removes bindings the caller never named, so it is not the delete of its target.
  The test asserts the negative half — a cleanup must not *also* record
  `source.template.delete` — because an implementation that recorded both would
  satisfy any check phrased as "the cleanup is audited".

## Where the shape stood, and where it stands now

FR-164 and FR-167 are two instances of one shape: **a handler that gates entry to
the audit layer on the very condition that layer exists to adjudicate**. Two
occurrences meant it was worth counting the rest.

Counted at `d7ef4faf`: of the production `action_audit::begin` call sites in
`crates/daemon/src/server/` — `session.rs` (5), `source.rs` (5),
`source_connection/` (12 across four files), `attention.rs` (4), `handoff.rs` (3),
`trigger.rs` (1), `resource.rs` (2) — `resource.rs`'s delete guard was the
**last** conditional one. Every other site calls `begin` unconditionally. The
shape is now closed in the daemon, and that is a finding worth more than either
FR: it converts "we fixed two" into "there are none left", which is a different
claim and the one a reader actually wants.

## Known limits

- **`resource.delete` is one name for three situations** — a CRD, a custom
  resource, and an unresolvable kind string. Distinguishable by `target_id`, not
  by `action`. Mirrors apply's `resource.apply`.
- **The tombstone author is still a constant.** `"daemon-delete"`, or
  `"project-delete"` for a project. `control_action_audit` now carries the real
  actor, so attribution exists — but the two tables disagree and correlating them
  means joining on timestamp rather than identity. Unchanged here; worth its own
  ticket.
- **A delete row does not retain the deleted spec.** `canonical_request` is never
  persisted (only its SHA-256), and the tombstone writes `spec_json = '"deleted"'`.
  Recovering what was deleted means reading the previous `resource_versions` row.
- **`core/src/resource/parse.rs::delete_resource_by_kind` has no production
  callers.** Only its own tests call it, yet it carries a thirteen-branch alias
  table — including `RuntimePolicy` and CRD arms the live path does not have, and
  a `SecretStoreResource::delete_from` that targets the *correct* map. The dead
  path was right and the live one was wrong, which is FR-167's own "inverted
  retirement target" shape found inside the file it was written about. Recorded
  rather than removed: deleting it is a separate change with its own test
  fallout.
- **`kind_as_str_covers_all_resource_kinds`** in `core/tests/integration_test.rs`
  still asserts only three of the twelve kinds despite its name — inherited from
  DD-177's known limits and left as found. The coverage this FR needed is
  asserted by `every_alias_round_trips_and_is_unique` and
  `kind_canonical_name_matches_debug`, both derived from `ALL_RESOURCE_KINDS`.
- **The GUI's `resource_delete` command has no frontend caller** and sends
  `force: false`, which the service layer refuses. It was given an envelope for
  symmetry with `resource_apply`; whether the command should exist at all is a
  separate question.
