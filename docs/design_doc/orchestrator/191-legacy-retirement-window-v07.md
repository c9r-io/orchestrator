---
lifecycle: active
related_fr: FR-173
---

# DD-191: The v0.7 Legacy Retirement, and What "Retire" Could Actually Mean

**Status**: Released
**QA**: [229](../../qa/orchestrator/229-legacy-retirement-window-v07.md)

## What was retired

Six compatibility surfaces and their `[legacy_*]` codes. Error codes drop from 18
to 12.

| Code | Surface | How it retired |
|---|---|---|
| `legacy_runner_executor_removed` | `RunnerSpec.executor` | field deleted, `deny_unknown_fields` added |
| `legacy_pipeline_variables_removed` | `store_inputs`, `store_outputs`, `step_vars`, `PostAction::StorePut` | fields deleted; the spec type's flattened catch-all downgrades them to a named warning |
| `legacy_coordination_removed` | `StepBehavior.captures` | field deleted, `deny_unknown_fields` added |
| `legacy_json_path_removed` | `PostAction::SpawnTasks`, `GenerateItems` | variants deleted |
| `legacy_agent_command_deprecated` | command-only Agent | promotion removed, `validate` now rejects |
| `legacy_agent_execution_removed` | driverless Agent at spawn | **code retired, guard kept** — see below |

Removed alongside them, because they only ever existed to serve those surfaces:
`steps` in `BUILTIN_CEL_VARS` and the prior-step-id lint skip; the legacy half of
the production-parity fixture; and the two gate assertions that required the
retired codes to be present.

Also deleted: three rejection functions, the 469-line test-only oracle holding
rollback evidence, 223 field initialisations across 17 files, and both glossaries'
six entries.

## What step 0 found, and what it did not change

**The dates argue against this work, and they were overruled deliberately.** All
six codes were introduced on 2026-07-25 or 2026-08-02 — 16 to 24 days before
retirement, all from one coordination collapse in late July. The release before
0.4.0 was 0.3.1 on 2026-04-06, a four-month gap, so a user still on 0.3.1 upgrades
straight from "`behavior.captures` works" to "it fails". FR-173's own requirement 1
asks for a window justified by how often users upgrade; one to two minors is not
that. The product owner is the only real user, accepted the breakage, and
reaffirmed the window after seeing these dates. This record states the argument
rather than the conclusion so that a future retirement is not justified by
pointing at this one.

**The surface was measured in the wrong unit.** The FR counted files from
`grep -rl`. By site, and split by kind: 13 implementation sites, 11 test sites, 86
documentation sites, and **37 gate/ledger sites** — the last understated 2.6× by
the file count, and it is the number that decides how much a retirement removes.

**Two hypotheses about reachability, both wrong, in opposite directions.**
`legacy_agent_execution_removed` looked dead — `normalize_config` promotes every
command-only Agent at persist time, so nothing should reach the scheduler without
a driver. It is not dead: `load_config` does **not** normalise, so a record
written before promotion existed and never re-applied survives a load intact.
Conversely, half of `task_ops`'s task-workspace guard *was* dead: it refused a
`GenerateItems` post-action that `[legacy_json_path_removed]` had already refused
at apply, so task creation never saw one.

## The mechanism question, which the FR got wrong

FR-173 said retiring the five rejection codes "does not change what is accepted,
only the quality of the diagnostic — from a named rejection to `deny_unknown_fields`'s
generic error". **None of the three structs had `deny_unknown_fields`.** serde's
default is to ignore unknown fields, so deleting a field would have had a user's
`behavior.captures` block accepted and dropped without a word — strictly worse
than the named rejection it replaced.

That turned "delete the check" into three different jobs depending on where the
field lived:

- **`WorkflowStepSpec`** carries `#[serde(flatten)] extra`, consumed by
  `collect_step_warnings` to produce `contains unknown field 'store_inputs'` with
  a did-you-mean hint. Deleting fields there degrades a rejection to a **named
  warning**. No attribute needed.
- **`StepBehavior`** and **`RunnerSpec`** have no catch-all. They needed
  `deny_unknown_fields`, which turns a retired field into a **stated
  deserialisation error**. Without it the retirement would have been silent.

The distinction is the whole safety argument, and it is what the four assertions
in `fr173_retirement` hold.

## What "retire" meant for the sixth

`legacy_agent_execution_removed` kept its guard and lost only its code. The line
immediately below it is `spawn_with_runner_and_capture`, which the comment there
marks as the direct-command substrate and *not* Agent execution. Deleting the
guard would let a driverless Agent take that path and run — the pre-driver
execution path the driver abstraction exists to have removed. One fewer error
code is not worth a silent wrong execution. Its message changed, because it had
been advising a re-apply that now fails.

## Prose that outlived its mechanism

Five places kept describing something that no longer exists. Each was found by
reading what pointed at the deleted thing, not by grepping for the deleted thing:

- The CEL prehook warning said a variable was referenced but "no prior step
  captures" it, while the captured set could only ever be empty. It now names
  what it actually checks: builtin CEL variables and prior step ids.
- The did-you-mean map suggested `behavior.captures` — advice pointing at a
  deleted field.
- `orchestrator guide error-codes` told users codes look like
  `[legacy_agent_command_deprecated]`, naming a retired one.
- The coordination ledger's `pipelineVariables.scope` said "Rejected at apply
  with `[legacy_pipeline_variables_removed]`", and `capturesOrJsonPath.internalMachine`
  named an oracle this change deleted.
- The fixture `legacy-shell-pilot` and QA 164's "shell pilot equivalence"
  scenario compared a command-only Agent against an explicit one. With the
  command-only form gone the two were identical, so the fixture was renamed
  `second-shell-pilot` and the scenario now asserts the refusal instead of an
  equivalence.

## Two gates were guarding the retired surface, and one guarded itself with it

`qa-doc-lint` went red on the finished tree for reasons that were not documentation
drift:

- `test-error-code-glossary.sh` derives the error-code set from source and then
  runs a **sanity check on its own derivation**, asserting that a known-present
  code is in the result — a derivation that lost its scope would otherwise report
  a small set as a correct one. The sentinel was `legacy_agent_command_deprecated`.
  Deleting it left the gate reporting *"the derivation lost its scope"* about a
  scope that was fine. The anchor moved to `driver_config_invalid`, chosen from a
  different file and a different layer than the ones the derivation reads first:
  an anchor taken from the file the scan starts in would make the check agree
  with itself.
- `test-agent-driver-documentation-alignment.sh` required both guides to *name*
  `legacy_runner_executor_removed`, and required three retired codes to exist in
  production source. Naming a retired code is the drift this gate exists to
  catch. The doc assertions now require the two statements a reader still needs —
  the field is gone, and a manifest carrying it is refused by name — and the
  source assertions collapsed to the one binding specific to this gate, because
  the general obligation is already covered in **derived** form by the glossary
  gate. Three hand-listed codes were an enumeration standing in for coverage
  (§4.4 shape 2) that happened to be complete.

A gate written to hold a compatibility surface in place has to retire with it,
and the self-check is the part that reads least like part of the surface.

## A sixth piece of prose, found by the tests rather than by reading

Guide 02 still told authors, in both languages, that a command-only manifest is
"promoted with a warning" and named `[legacy_agent_command_deprecated]` — three
statements that survived the `STALE_PATTERN` sweep because that pattern enumerates
phrases seen before. The sweep is a list, and a list covers what was known when it
was written (§4.4 shape 2). What found these was the doc-lint failure, not the scan.

## `steps.<id>.<var>` was never a mechanism

Removing `captures` exposed something older. `BUILTIN_CEL_VARS` contained `steps`,
and `collect_step_warnings` skipped any identifier matching a prior step id, so
`steps.step_a.regression_target_ids` linted clean — and a test asserted that it
must. **`build_step_prehook_cel_context` binds no variable named `steps`, and no
version of it ever did.** The expression fails at execution with "no such
variable". Captured variables were reachable by their bare name, which
`bind_compatibility_vars` still does for whatever populates `vars`; the dotted
form was an accommodation the evaluator never honoured, and `captures` was the
only thing that made it look like one. No tracked blueprint uses it. Both the
builtin entry and the skip are gone, the diagnostic no longer offers "or a prior
step id" as if that were a way to bind a name, and the test now asserts the
warning — naming `steps` itself, since naming only the trailing member sends an
author after the wrong mistake.

This is the same error as the five above with the polarity reversed: there the
prose outlived the mechanism, here a **lint exemption** outlived one, and a lint
exemption that certifies a form which cannot run is worse than a stale sentence,
because it answers an author who asked.

## The parity fixture's legacy half collapsed into its typed half

`agent-driver-production-parity.yaml` carried a `<name>-legacy` Agent beside each
`<name>-typed` one, and the gate ran both and required the two to agree *and* to
match a recorded pre-migration baseline. With the command-only form retired the
two declarations differed **only in capability name**, so the first conjunct
compared a run against a rename of itself. Six documents were removed and the
gate now runs the typed half against the baseline — which is where the evidence
always was (§4.3): the right-hand side of that comparison is a hash recorded
before the migration, not anything the gate produces. Collapsing it also removed
a hazard the old form did not have: `jq -r` yields the string `"null"` for an
absent baseline entry, and two absent entries compare equal, so the missing-entry
case is now a named failure rather than a pass.

The apply assertion moved from "exactly three promotion warnings" to zero — and
inverting it alone would have been satisfied by an apply that failed outright and
therefore warned about nothing, so it now also requires the three Agents to be
present with a shell driver afterwards.

## The documentation sweep was scoped wrong the first time

The first pass scanned tracked YAML for retired keys and reported it clean. It was
clean, and the scope was wrong twice over:

- **Rust string manifests.** Four inline RuntimePolicy manifests in daemon tests
  still carried `executor:`, and `deny_unknown_fields` refused them by name — five
  failing tests, which is the attribute doing exactly its job. A manifest is not a
  file type.
- **Documentation teaching the retired surface as current.** `docs/guide/03` and
  `05`, their Chinese mirrors, both `orchestrator-guide` skill references, and the
  infinite-evolution showcase in both languages documented `behavior.captures`,
  `spawn_tasks` and `generate_items` as working features with no removal notice.
  A user following guide 03 wrote a manifest the daemon refuses — and had been able
  to for weeks, since these were refused before FR-173 too. The skill references are
  the worse half: they are what an authoring agent reads, so the wrong shape gets
  written at machine speed.

The lesson is the unit again, as in step 0: the first sweep counted *files with the
key* and the thing that mattered was *surfaces that teach or parse the key*.

## Two blueprints were already dead, and this change did not kill them

`fixtures/workflow/self-bootstrap.yaml` and `self-evolution.yaml` use
`behavior.captures` and the `generate_items` post-action. Checked against `main`
rather than assumed: `reject_retired_authoring` already bailed on both with
`[legacy_coordination_removed]` and `[legacy_json_path_removed]`. They have not
applied for weeks. `fixture_corpus_tests.rs` globs `fixtures/manifests/bundles/*.yaml`
and nothing else, so no test parses them and no ledger says whether they are meant
to be valid — which is why a grep found this and no gate did. Converting them is a
design decision about the self-evolution flow (the JSONPath mapping produces the
items `select_best` ranks, and `captures` produces the score it ranks by), not a
find and replace, so it is filed as
`docs/ticket/fixtures-workflow_ungoverned-dead-blueprints_260818_071500.md` rather
than done here under cover of a retirement.

## A fingerprint block that could not tell a renamed test from a passing one

`test-agent-driver-execution-migration.sh` re-runs a named set of tests as a
fingerprint, one `cargo test` invocation per package with several filters after
the `--`. FR-173 renamed two of the names. **Nothing failed.** `cargo test --
a_name_that_no_longer_exists` runs zero tests and exits 0, so the block stays
green while certifying nothing — §4.4 shape 5, a check that can report PASS
having read no input, in the one place where the input is the evidence.

It surfaced by accident: the gate was killed by a wall-clock limit, and reading
why showed the filter matching nothing. Had it completed it would have passed.

Each invocation now goes through `run_named_tests`, which states the expected
count and reads the observed one out of libtest's own summary (1, 2, 5, 3, 1 —
each measured, not assumed). A renamed test is now a named failure rather than a
silent subtraction from what the gate covers.

## A third gate held the surface, through a fixture it derives

`test-coordination-strangler.sh` builds its tool fixture by filtering
`coordination-strangler-parity.yaml`, keeping every non-Workflow document. Two of
those are command-only Agents, so the derived fixture stopped applying — the gate
found the retirement through a file it does not own. Both Agents now declare
`driver: {provider: shell, transport: cli}`, and their `fixture-driverless-exempt`
comments went with the form they authorised.

Its other assertion required the legacy fixture to fail *with*
`[legacy_coordination_removed]` or `[legacy_json_path_removed]`. It now requires
`[parse_error]` and `unknown field \`captures\``, which is what
`deny_unknown_fields` emits — still a named diagnostic rather than a bare exit
code, because an exit code cannot distinguish this rejection from a typo in the
fixture path.

## Ledgers re-derived rather than rewritten

`fixture-bundle-validity.json` records the diagnostic each intentionally-invalid
bundle produces. Those diagnostics all changed. They were **re-observed by running
the corpus test and reading what the product printed**, not rewritten from the old
wording — the ledger's value is that it holds what the product actually says, and
a plausible-looking guess would have turned it into a record of a sentence nobody
emits. The gate's own message says the shape out loud: *"it is rejected for a
reason nobody wrote down."*

`coordination-governance.rb`'s `pipeline_consumer_kinds` is now an empty frozen
array rather than deleted, so the consumer scan still runs and still derives the
zero that the ledger's `productionConsumerCount` asserts.

## Known limits

- **The window is 16–24 days.** Recorded above. Nothing here makes that a
  precedent.
- **`generate_items` as a coordination tool has no task-workspace guard.** The
  invariant that a task workspace keeps one implicit item is enforced against the
  declarative surface only. This pre-dates FR-173 and is neither created nor
  closed by it.
- **A `WorkflowStepConfig` — the internal type, deserialised from stored config —
  has no catch-all and did not get `deny_unknown_fields`.** The manifest boundary
  is `WorkflowStepSpec`, and adding the attribute to the internal type would break
  database loads for keys a user never wrote.
- **A prehook cannot read a prior step's output at all.** Removing `captures`
  removed the only producer of per-step variables, and the `steps.<id>.<var>`
  form that looked like the consumer never worked. Nothing regressed — the dotted
  form always failed at execution — but the product now has no declarative way to
  pass a value from one step's output into a later step's prehook condition. That
  is the intended end state of the coordination collapse (read the store from the
  step instead), and it is recorded here because the lint used to imply otherwise.
- **No `skip_serializing_if` runway was given.** The safe two-release sequence —
  stop writing the field, wait for stored configs to be rewritten, then delete —
  was skipped because the database is being reset. On a deployment with users this
  retirement would need that runway, and the absence of a rule saying so is what
  FR-173's requirement 4 was for.
