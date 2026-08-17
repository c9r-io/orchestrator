---
lifecycle: active
related_fr: FR-172
---

# DD-190: The Closure Note Is a Pointer, and This Is the Bound That Keeps It One

**Status**: Released
**QA**: [228](../../qa/orchestrator/228-governance-record-compaction.md)

## The problem

`docs/feature_request/README.md` held 193,821 bytes, of which **174,428 — 90.0%
— was hand-written closure notes**. 172 notes, bucketed by FR number, grew
monotonically in mean length: 212 characters for FR-000..019, 3,284 for
FR-160..179. A fifteenfold increase with nothing arresting it.

A closure note's job is to redirect. `.claude/skills/fr-governance/SKILL.md`
shows the template — *FR-XXX 已闭环删除；其设计与验证信息现由 … 承载* — and later
notes had become second copies of their design records, in a file
`doc-lifecycle.rb` does not govern (`DOC_ROOTS` is design_doc, qa, security) and
which can therefore never be marked stale. The duplicate cannot rot visibly.

## What step 0 changed

**Two of the FR's own claims did not survive, and both shrank the work.**

The FR said requirement 3's premise: that nobody had decided whether a
`lifecycle: superseded` document may be deleted. `CONTRIBUTING.md:192-194` had
decided it, with a reason: *"Do not delete it — the history is the audit trail,
and the prose banner is what a human reads."* The rule the FR asked for already
existed and was not found at filing. **Requirement 3 withdrawn.**

The FR said requirement 1's diagnosis: that the rule reads "following the
existing pattern" and the precedent it points at had drifted. It does not — the
skill **shows the template inline**. Practice diverged from an explicit template,
not from an ambiguous referent, so the repair is not to make the rule
self-referential. What the rule lacked was a *quantity*, and that is what this
change adds.

**Requirement 4 was folded into requirement 1.** It asked `doc-lifecycle.rb` to
print governed bytes so growth would be visible. That gate's three roots do not
include `docs/feature_request`, so the change would not have seen the problem the
FR is about. A bound that can be enforced makes growth visible by refusing it; a
second mechanism was not needed.

Four requirements became two.

## The measurement, and a defect in the measurement

The FR listed "overlap between notes and design records" as unmeasured, and
noted that if rulings existed *only* in notes then this was backfilling work
rather than compaction. It was measured: distinctive tokens in each note (code
spans and multi-digit numbers) checked for presence in the documents the note
names.

The first extractor was wrong. `/`([^`]{3,60})`/` pairs the second and third
backticks in `` `a` 与 `b` ``, capturing the Chinese prose *between* code spans as
a token that can never match. Splitting on backticks and taking odd indices
corrected it, and the corrected numbers are stronger:

| | first extractor | corrected |
|---|---|---|
| all 116 comparable notes, median coverage | 77% | **80%** |
| the 50 over-bound notes, minimum | 54% | **62%** |
| the 50 over-bound notes, median | 82% | **90%** |
| over-bound notes below 50% coverage | 0 | **0** |

**This is a proxy and bounds the work rather than deciding any note.** A token
present in the design record does not mean the claim built from it is; a token
absent does not mean the claim is. Its use was to find where reading was needed.

Per-sentence, it found that of 334 sentences across the 54 over-bound notes, only
**30 sentences in 12 notes** had more than half their tokens missing. That turned
"read 54 notes and 54 design records" into "read 30 sentences", and those 30 fell
into three kinds.

## What was actually moved

**Certification records (FR-147, FR-149, FR-158).** Each recorded one full gate
sweep meeting §4.6's validity conditions: the revision, a clean worktree at both
ends, a gate set *derived* from the manifest rather than typed, 46/46 or 54/53
invocations with the uncovered set empty, exit codes taken directly from `$?`, and
the two reds that were not regressions. None of it appeared in the QA documents.
Each was moved into a **Certification record** section in QA 198, 197 and 210 —
governed documents — and then removed from the note. This is the one case where
the FR's "backfill first, then remove" procedure was actually needed.

**FR-130's three notes collapsed to one.** Two were interim status while the FR
was still open — requirement 1 and 3 closed, then requirement 2 Phase A. The
final closure note supersedes both. Their metrics (52 `pub mod`, 924 public
items, 200 `rusqlite` references, 37 migrations producing 46 tables and 92
indexes) were each confirmed present in DD-142 and DD-148 before deletion.

**Batch headers kept, compacted.** Three entries are not closure notes at all:
two describe an audit batch (*FR-127 至 FR-133 源自 2026-07-25 的技术负债深挖*,
*FR-150 至 FR-158 源自 2026-08-01 的全维度技术负债审计*) and one records that
FR-159 came from a different source — a direct observation of a development
machine rather than a static audit. That provenance exists nowhere else and is
worth keeping; each was compacted to fit the bound rather than exempted. **No
exemption mechanism was added**, deliberately: a subtree or category exemption
absorbs instances that do not exist yet and never produces a line in any log
(§4.4 shape 8).

**The remaining 47 were rewritten to the template** from the document paths the
note already named.

Result: **193,821 → 82,607 bytes, a net reduction of 111,214 (57.4%)**. The
closure-note section fell from 90% of the file to 32%.

## The bound

400 characters. Derived from the note's job: the template carrying two real paths
runs about 150 characters — measured against FR-171's own 229-character note,
written the day before this work as template plus one sentence — and a sentence
in this repository's documentation style runs 60–120. 400 admits the template
plus at most two sentences and admits nothing more.

**It is a judgement, and it was made with the distribution already in hand.**
FR-140 warns against deriving a threshold from the measurement it will govern.
The derivation above does not use the distribution, but the author had seen it,
and whoever revisits this should re-derive rather than inherit. For the record,
the checks that "118 of 172 already complied" and that a 300 bound would have
touched 70 notes were run *after* choosing, as consequences.

Characters, not bytes. These notes are mostly Chinese — three bytes per character
in UTF-8 — so a byte-counting implementation would fail them at a third of their
real length. A fixture holds a 399-character Chinese note, 1,197 bytes, which
must pass.

## Where the check lives

`fr_registry.rb notes`, invoked by `test-governance-ledger-tooling.sh`. **No new
script and no new entry in `qa-gate-surface.json`**: the gate already governs this
file through `fr_registry.rb check`, so extending it holds the gate count flat —
which matters both to this FR's subject and to FR-174's cost argument.

`notes` runs before the history walk `render` performs, so it costs nothing and a
shallow clone — which cannot be asked about history — can still be asked whether
its notes are within bounds.

Three fixtures, each building its own synthetic README rather than copying the
tracked one, so each fails for its own reason instead of inheriting the state of
the real file:

- a 510-character note fails **and the diagnostic names it and its length**;
- a 1,400-character preamble line above the generated block is out of scope — a
  check that failed on any long line would be measuring the file, not the notes;
- a 399-character Chinese note passes, which a byte-counting implementation
  cannot do.

## Known limits

- **The bound does not distinguish a pointer from prose.** 400 characters of
  duplicated design record passes. The bound arrests growth; it does not enforce
  the note's purpose, and no gate can — that judgement was made once, here.
- **The three batch headers now fit the bound but are still not closure notes.**
  They are governed by a rule written for a different kind of entry. A fourth
  batch would sit in the same gap.
- **Coverage was measured, not verified.** The 47 template rewrites rest on a
  token-presence proxy at 62–100% coverage, not on a reading of each note against
  its design record. Anything that lived only in the discarded prose and carried
  no distinctive token is gone from the working tree; git history retains it.
- **`docs/qa` and `docs/design_doc` still grow monotonically.** This FR bounded
  one file. 489 governed documents, 15 of them superseded and none retired, is
  the same shape at a larger scale, and `CONTRIBUTING.md:192` decides retention
  without bounding cost.
