# `fixtures/workflow/` holds blueprints that cannot apply, and nothing checks it

**Status**: open
**Found**: 2026-08-18, during FR-173's retirement sweep
**Severity**: medium — no user-facing breakage today, but two QA documents and one
design document cite these files as if they were runnable

## What

`fixtures/workflow/self-bootstrap.yaml` and `fixtures/workflow/self-evolution.yaml`
declare `behavior.captures` and `post_actions: [{type: generate_items, ...}]`. Both
constructs are refused at apply, and **were refused before FR-173** — `main`'s
`reject_retired_authoring` bailed with `[legacy_coordination_removed]` and
`[legacy_json_path_removed]` respectively. FR-173 changed which mechanism refuses
them (`deny_unknown_fields` and a deleted enum variant), not whether they apply.

So these two files have been unapplicable for weeks and nobody noticed.

## Why nobody noticed

`fixture_corpus_tests.rs` governs `fixtures/manifests/bundles/*.yaml` and nothing
else. `fixtures/workflow/` is outside the glob, so no test parses these files, and
no ledger declares whether they are meant to be valid or intentionally invalid —
the two states a corpus fixture can legitimately be in. A third state, "used to be
valid", is the one they are actually in and the one the corpus cannot express
because it never looks.

`full-qa.yaml` in the same directory is unaffected and uses no retired construct.

## Who cites them

- `docs/qa/orchestrator/80-item-scoped-git-worktree-isolation.md`
- `docs/qa/orchestrator/99-long-lived-command-guard.md`
- `docs/design_doc/orchestrator/step-scope-roundtrip-leak.md`

A reader following any of these gets a manifest the daemon refuses.

## Why this was not fixed inside FR-173

The conversion is not mechanical. `generate_items`' JSONPath mapping is what
produces the items that `select_best` then ranks, and `captures` is what feeds the
score it ranks them by. Moving to the `generate_items` coordination tool means the
step's Agent has to produce the item list directly and the score has to arrive by
some other route — a design decision about the self-evolution flow, not a find and
replace. Doing it under a retirement FR would have meant redesigning a workflow
while claiming to remove a field.

## What a fix should decide

1. Whether these two blueprints are still wanted. If the flows they describe are
   not maintained, deleting them and updating the three citing documents is the
   smaller change and the honest one.
2. If they are wanted: how a step publishes items and a score without the engine
   parsing them out of stdout — presumably `generate_items` plus `record_metric`,
   both of which exist as coordination tools.
3. Independently of 1 and 2: whether `fixtures/workflow/` should be inside the
   corpus glob. It is the only fixture directory holding manifests that no test
   parses, which is why a two-week-old breakage was found by a grep rather than by
   a gate. Note that widening the glob is not free — every file under it then needs
   a declaration, which is the point, but it is work.
