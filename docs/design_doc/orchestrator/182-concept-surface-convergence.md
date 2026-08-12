---
lifecycle: active
related_fr: FR-166
---

# 182. Concept surface convergence

**Status**: Released

The governance surface has a concept budget: DD-172 makes a new `ci-required`
gate name the shape it catches, and `check_new_gates_name_their_shape` enforces
it. The product surface had no counterpart, and had accumulated the things that
absence produces — one object with two names, two kinds with identical specs and
undocumented different behaviour, one kind quietly doing four jobs, and a
built-in list that had drifted from the enum with nothing comparing them.

This record is about four decisions and the defects that finding them exposed.
Three of the four decisions were *not to change the code*. That is the intended
shape of this FR: per FR-160 requirement 4, a written reason is a valid outcome.

## What step 0 changed about the work

The FR's own facts were rebuilt before planning, and four did not survive. Two
changed what the work was, which is the argument for doing the rebuild at all.

**The FR said Trigger had three jobs and cited `02-resource-model.md:441-452`.**
Trigger has four — `TriggerEventSpec` (`cli_types.rs:605-620`) carries `webhook`
*and* `filesystem` alongside cron and task-lifecycle events — and the cited lines
belong to `## 12. SourceTaskBinding`, not `## 10. Trigger`. Adjudicating "three
jobs" as written would have decided nothing about the filesystem job. This is the
category conflation the fact-verification step exists to catch, and it was found
by reading the struct rather than the sentence.

**The FR said the English table of contents linked Chinese content on one row.**
It linked five. Requirement 2 as written would have closed a fifth of the defect
it named and reported success.

Two smaller corrections: `"Wish"` appears zero times in `docs/guide`
case-sensitively but twice in lowercase in `zh/08`, so the undefined term had
already reached user documentation — the conclusion the claim supported was
stronger than the claim. And the FR's argument that renaming the CLI would touch
"127 commands" overstated by about nine times: 14 leaf paths sit under `task`.
The conclusion held for a different reason (Task is frozen into
`control_action_audit` action names), so the argument was replaced rather than
repeated.

## Decision 1: the console says Task

Every surface except the GUI called this object a task. The GUI called it a
Process, and its own route type said `{ page: "processes"; taskId }` — both
vocabularies on one line.

Renaming Task was never available. `resource.<snake_kind>.apply` names are in
`control_action_audit`, and recorded action names are never renamed; FR-164 kept
`source.template.apply` for precisely this reason, and FR-167 is about to mint
the matching `.delete` surface. So the console moved.

What moved is **presentation**: nav label, page headings, the `New task`
affordance, the Chinese string table, and the hash a user sees or bookmarks
(`#/tasks`, `#/new-task`). What deliberately did not move:

- **The route discriminant.** `ConsoleRoute` still discriminates on `"processes"`,
  so no page component changed shape. A `PAGE_PATH` map turns a page into a URL
  segment in one place, and both `formatConsoleRoute` and the nav — which builds
  hrefs directly — go through it.
- **`wish-pool`.** It reads like a label and is a wire value: `WishPool.tsx` sends
  it as `project_id`/`project_filter` and `crates/gui/src/commands/stream.rs:365`
  branches on it. Renaming it is a data migration, which is not what a label
  decision authorises.
- **`ConsoleFeature` keys, component filenames, feature-flag keys.**
- **The product name.** The *Agent Process Console* keeps its name. It is a
  proper noun carried by three design records and two guides, and the harm this
  FR addresses — a reader unable to map what they see to `orchestrator task` —
  comes from the object noun, not the product name.

`#/processes` and `#/new-process` still parse. DD-137's parseable-rejection
pattern does **not** apply here and was not used: that pattern is for removing an
authoring surface, and nothing is removed. Compatibility here is acceptance, and
the assertion that matters is that the old hash still lands on the same page.

"Wish" became "draft" on the same reasoning. It had no definition anywhere in
`docs/guide`, its own route already read `new-process`, and it was a fourth noun
for a thing three other nouns already covered.

### Two renames a substitution pass gets wrong

Chinese collapses two unrelated senses onto 进程: the unit of work, and an
operating-system process. `i18n.ts` holds five strings of the first kind and one
of the second — `connection.cause1Title: "守护进程未启动"`, the diagnostic shown
when the daemon is down. A blanket 进程 → 任务 pass satisfies every vocabulary
assertion and turns that string into advice for a different problem, so it is
asserted unchanged and that test is documented as the negative fixture for the
rename.

The second is the nav label and the nav href. Changing the label alone leaves the
nav linking `#/processes`; changing the route alone leaves the console saying
Processes. Either passes an assertion that reads only the other, so both are read
from the same rendered element in one test.

Nine strings in `i18n.nav` were deleted rather than reworded. They named a
four-item navigation (Attention Inbox / 许愿池 / 进度观察 / 来源) the console
stopped having; only `mainNav` was read. A dead string table is a third
vocabulary and the next reader has no way to tell it is not authoritative.

## Decision 2: EnvStore and SecretStore are not merged

The guide said a SecretStore "has the same structure as EnvStore but is intended
for sensitive values", and nothing else. `rg -i 'encrypt|rotat'` over guide 02
and 05 returned **zero**. The specs are byte-identical (`data: HashMap<String,
String>`), so the documentation offered a reader no way to choose.

The kind is not a label. It is the switch three behaviours read:

| | EnvStore | SecretStore |
|---|---|---|
| Spec at rest | plaintext JSON | AEAD-encrypted, bound to project and name (`encrypt_resource_spec_json`) |
| Export and overview | values shown | values replaced with a placeholder (`config_load/persist.rs:40`) |
| Key operations | none | six `orchestrator secret key` leaves |

Merging them would strand one of two already-recorded audit action names. The
decision is therefore permanent in a way the manifest does not show, and the
guide now says so — including that moving a value between the two is a delete and
re-apply, not a rename.

## Decision 3: Trigger is not split

The candidate was extracting the webhook credential-holding job into its own
kind. It was rejected: `SourceTaskBinding.triggerRef` depends on the Trigger
owning installation identity and the external-actor-to-role mapping, and
separating the credential holder from the endpoint would put a mandatory join
between a delivery and its own actor roles. `resource.trigger.apply` is already
recorded, so this decision is permanent on the same terms as decision 2.

The defect the adjudication found is larger than the question it answered.
Chapter 02's Trigger section documented two of the four jobs. Webhook appeared
only as a fragment under **SourceTaskBinding's** heading — the section a reader
looking for Trigger's capabilities would not open — and filesystem appeared
nowhere in the guide at all.

Both now have examples in the Trigger section, and those examples are **parsed
out of the chapter by a test** rather than restated in one. `TriggerSpec` and its
children declare `deny_unknown_fields`, and `debounce_ms` is snake_case while
every webhook field beside it is camelCase — nothing in the surrounding YAML
tells an author that, and the camelCase they would guess is rejected. Writing the
example was not enough; a copy of the example inside a test proves the copy
parses. `guide_trigger_examples_deserialize_as_written` reads the fences from the
chapter, filters to `spec:` fences containing an `event:` key (derived, so a
future example joins without anyone remembering), and deserializes each. Verified
by mutation: `debounceMs` fails the test naming the field.

## Decision 4: `type:` does not become mandatory

Step execution mode is inferred from the step `id`. The guide documented two
rules — known builtin IDs, known agent IDs — which read as a closed set. The
third rule is the one that matters: the registry
(`config/step_conventions.rs`) accepts *any* ID and falls back to
`required_capability = <id verbatim>`. A typo in `id:` is therefore not a
validation error; `loop_gaurd` silently stops being the loop guard and starts
demanding an agent with a `loop_gaurd` capability, discovered later and
elsewhere.

Making `type:` mandatory would invalidate every existing workflow, so it was
rejected. The mitigation is to write the third rule down where authors read it,
with the advice that follows: state `builtin` or `required_capability` explicitly
when a step's behaviour matters.

## The kinds list, and why fixing the text was not enough

`05-advanced-features.md:7` listed ten built-in kinds: it omitted Project,
SourceTaskTemplate and SourceTaskBinding, and named WorkflowStore, which is a CRD.
`rg 'ResourceKind' scripts/ config/` returned nothing — no gate connected the
prose to the enum, so a text-only repair re-drifts by §4.4 shape 2.

`check_resource_kind_catalog` derives the expected set from `ResourceKind` and
diffs it against the prose, naming each kind rather than reporting a count. It
has a fixture in **each direction** — a kind added to the enum, and a kind added
to the prose — because a matcher is exact in one direction and wrong in the
other, and two different diagnostics mean the log says which way it broke.

## Measuring which documents are in Chinese

Requirement 2 needed to know which files in the English source directory are
actually Chinese. The obvious measure — the ratio of Han characters to Latin
characters — does not work, and the failure is instructive: technical Chinese is
dense with Latin tokens (CLI flags, YAML keys, product names), so the four
Chinese Slack runbooks scored **0.44–0.52** while a partly bilingual English
chapter scored **0.15**. No threshold separates a set whose members are that
close.

Asking instead what fraction of *prose lines* are written in Chinese gives
**0.94–0.97** for those four, **0.37** for the bilingual chapter and **0.00** for
everything else. The half threshold sits in a wide empty band, and the question it
answers — is this document written in Chinese — is the one that was being asked.

`check_guide_language_parity` uses it to derive both halves: the Chinese files by
measuring them, the permitted ones from `translationGaps`. A Chinese file dropped
into the English source directory tomorrow is caught without anyone editing a
list. The four runbooks are declared through the mechanism
`showcases/full-qa-execution` already used for exactly this — Chinese text
occupying the `en` source slot — which the guide collection had never used because
it sets `requireBilingual: false`.

Translating `docs/guide/08` removed the last ZH-only chapter, which falsified
`docs-publishing.json`'s own `requireBilingualReason`: it said "seven asymmetric
chapters (six EN-only, one ZH-only)", derived correct at the time. It is now
6/0, and the sentence was rewritten to stop restating a number that moves.

## The rule, and why it is not gated

`CONTRIBUTING.md` and the `orchestrator-guide` skill both carry the requirement
that a new `ResourceKind` or top-level command group justify itself against
existing concepts, with a reviewer checklist. The `fr-governance` skill's
authoring contract carries it too, since that is where FRs proposing new concepts
are written.

It is deliberately ungated. Whether a concept is a parameter in disguise is a
judgement, and a gate could only check that *some* justification text exists —
§4.4 shape 1, a text-presence proxy certifying a review it cannot observe, which
is worse than no gate because it converts an unknown into an assurance. DD-172's
counterpart is gated because a gate's `shape` field is a fixed slot on a manifest
entry with a machine-checkable presence condition; a PR description is not.

## Known limits

- **The vocabulary assertions are per-key and positive.** A *newly written*
  string using the old vocabulary is not caught — §4.4 shape 2, applied to this
  FR's own tests. Gating it would need a lexicon check that cannot distinguish
  the two senses of 进程, which is why this is a recorded limit and not a gate.
- **`check_guide_language_parity`'s threshold is calibrated, not principled.**
  The measured margin today is 0.37 to 0.94. A document that is genuinely half
  Chinese and half English would land near the line, and the check would decide
  it by a coin flip. Nothing in the tree is near it; if something arrives, the
  answer is to split the document, not to move the threshold.
- **Four Slack runbooks remain Chinese-only.** Deferred with reasons recorded per
  slug in `translationGaps`, not silently. They are platform-operations runbooks
  outside the product concept surface this FR governs.
- **`agent-driver-model.md` is 37% Chinese prose lines** and sits below the
  threshold as an English document with Chinese passages. That mixing is real and
  is not addressed here.
- **The guide's YAML examples are not generally executable.** This FR made the
  Trigger event examples parse-tested because it added two of them; the other
  fences in chapter 02 remain unverified prose.
