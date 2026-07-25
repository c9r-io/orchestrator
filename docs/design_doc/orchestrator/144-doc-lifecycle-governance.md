---
lifecycle: active
related_fr: FR-132
---

# DD-144: Design Doc And QA Doc Lifecycle Governance

**Module**: Governance / Documentation
**Status**: Implemented (FR-132)
**Related Plan**: FR-132
**Related QA**: `docs/qa/orchestrator/182-doc-lifecycle-governance.md`
**Related**: DD-139 (QA gate enforcement surface), DD-140 (governance ledger regeneration), DD-142 (core boundary freeze), DD-143 (docs publishing integrity)
**Created**: 2026-07-25
**Last Updated**: 2026-07-25

## Background

This repository closes every feature request into a design doc and a QA doc, and often a guide page,
a translation and a showcase. The traceability is genuinely good. The cost is that the document
surface only ever grows, and nothing records that a document stopped being true.

At the time FR-132 was filed there were 150 design docs and 226 QA docs, and the only way to answer
"does this document still describe the system?" was to read it against the code. FR-126 is the
worked example: it needed four successive manual audit rounds to discover that DD-101, DD-102 and
DD-103 described a streaming execution seam that had been deleted, and the fix was to hand-write a
prose banner into each. Nothing had marked them, and nothing would have.

`docs/feature_request/README.md` carries the closure notes mapping each FR to the documents that now
hold its design and verification, but that map only runs one way. From a design doc there was no way
to reach the feature request that produced it, and no way at all to reach whatever replaced it.

### A corrected premise

The FR's own numbers were rebuilt before planning. Its document counts were exact at its authoring
commit (`ec048a15`: 209 QA docs, 141 orchestrator design docs). Four of its other claims were not.

- **`status` was the wrong name.** The FR proposed a frontmatter field `status: active | superseded`.
  71 of 145 orchestrator design docs already carry a `**Status**:` header, holding `Approved` (44),
  `Implemented` (9), `Released` (8) and free-text variants. That axis is implementation maturity at
  authoring time, not document currency, and the two are independent: DD-101 is `**Status**:
  Released` *and* superseded. One word cannot carry both meanings inside one file. The field is
  therefore `lifecycle`.
- **DD-127 is not superseded.** The FR's background listed it beside DD-101/102/103 as a document
  that had needed a retrofitted "this is a historical record, not current configuration guidance"
  fence. DD-127 is `**Status**: Released` and is cited as the *current* driver abstraction by
  DD-129, DD-130, DD-138 and `docs/architecture.md`. Its banner is a post-release update, which is
  not the same thing as a supersession fence. Marking it superseded would have asserted something
  false. The fenced set is exactly DD-101, DD-102, DD-103. (The FR's acceptance criterion named only
  those three; only its background conflated the two banner kinds.)
- **A length ratchet is the weakest available rule.** The FR gated its exemption list by *length*,
  monotonically non-increasing. That passes when one entry is removed and another added — the list
  changes and the count does not. It is the same defect FR-128 found, where `capturesOrJsonPath` sat
  at 54 against a reviewed 55 for a full FR cycle, and that FR-130 replaced with exact equality
  across this repository's ledgers.
- **The doc:code ratio.** The FR's `100,213` production lines does not reproduce: raw non-test `.rs`
  is 148,733, and under this repository's own reviewed production definition
  (`scripts/lib/rust_source.rb`, inline `cfg(test)` stripped) it is 108,710. The real ratio is
  ≈ 0.73 : 1, not 0.86 : 1. The direction of the argument is unaffected.

One more thing the FR did not state: **no design doc had YAML frontmatter at all** (0 of 150), while
226 QA docs mostly did, carrying `self_referential_safe` only. So "every document carries valid
frontmatter" started from roughly 172 violations across two incompatible encodings, which is the
number its phased backfill was implicitly sized against.

## Design

### One axis, a new word

`lifecycle` is `active` or `superseded`. `superseded` requires `superseded_by`, a repository-relative
path to the successor. `related_fr` is optional and format-checked when present.

```yaml
---
lifecycle: superseded
superseded_by: docs/design_doc/orchestrator/138-agent-driver-execution-migration.md
---
```

The existing `**Status**`, `**Related Plan**` and `self_referential_safe` conventions are untouched,
and so are the prose banners. The gate reads metadata; a human reads the banner. Neither replaces
the other, and the FR was right to ask for both.

### No exemption list

The FR proposed a phased backfill behind an exemption list under a ratchet. Because `lifecycle:
active` is mechanical for every document that is not one of the three known fences, the whole
surface was backfilled in one pass instead: 399 documents, 396 active, 3 superseded, 244 carrying
`related_fr`.

This is strictly stronger than a shrinking exemption list. There is no list to grow, no ratchet to
be defeated by a swap, and no partial state where the gate is green while most documents are
unclassified. It also avoids reimplementing, one FR after FR-130 removed it, the exact ledger shape
this repository had just decided against.

The FR's other exemption — structural index files — is expressed as a **scope rule** rather than a
list: files named `README.md` and any path containing a component beginning with `_` are out of
scope. A rule cannot acquire an entry at a time.

### `related_fr` is recorded, not inferred

Only two sources were used: the closure notes in `docs/feature_request/README.md`, which are
authored and reviewed sentences naming an FR and the documents that carry it, and the
`**Related Plan**:` header where its value *begins* with an FR id. Most `**Related Plan**` values are
free prose describing the plan rather than naming it, and those were left alone.

That yields 244 of 399 documents. The remaining 136 have no `related_fr`, which is the honest
result: a gate enforcing 136 plausible guesses would be worse than a gate enforcing 244 facts.

### Parsing, not matching

The frontmatter is parsed with `YAML.safe_load`. A hand-rolled `key: value` regex was written first
and rejected during the backfill, because `docs/qa` already contains shapes it silently mis-reads —
block sequences under `self_referential_safe_scenarios`, and `#` comment lines explaining which
scenarios are unsafe. Both would have been reported as malformed by the regex and skipped by a more
forgiving one.

### Coverage is walked, not listed

`governed_documents` globs the three roots. Nothing anywhere names the set of scanned files. This is
the specific failure mode a hand-listed roster has: it guards exactly what was known when it was
written, and the next document lands outside it in silence.

### The index

`config/governance/doc-lifecycle-index.json` is generated by `--emit-index` and compared by exact
equality in both directions, in the same idiom as `core-boundary-ledger.json`. It carries each
document's lifecycle, its `related_fr`, its supersession edge, and the two reverse maps —
`byFeatureRequest` and `supersedes` — that `docs/feature_request/README.md` structurally cannot
express.

`--write` refuses to run when `CI` is set. An automatic index rewrite in CI would convert the review
gate into decoration.

## Verification by mutation

A gate observed only passing has not been observed doing anything. Eleven defects were introduced
into `doc-lifecycle.rb` one at a time, each against a fresh copy of the document tree:

| Mutation | Defect introduced | Caught by |
|---|---|---|
| M1 | missing frontmatter is tolerated | case 3 |
| M2 | superseded with no successor is tolerated | case 4 |
| M3 | `superseded_by` existence is not checked | case 5 |
| M4 | self-referential `superseded_by` is tolerated | case 6 |
| M5 | supersession cycles are not detected | case 7 |
| M6 | any `lifecycle` value is accepted | case 8 |
| M7 | `related_fr` format is not checked | case 9 |
| M8 | coverage read from a roster, not the filesystem | case 10 |
| M9 | `--write` no longer refuses under CI | case 11 |
| M10 | the committed index is never compared | case 12 |
| M11 | the emitted index drifts from the committed one | case 2 |

Every mutation is caught by exactly its intended case. Case 1 is the positive control — it asserts
the gate passes on the repository — and is not isolated by any of these, because all eleven weaken
the gate rather than break it.

### A defect the run found in this gate's own tests

M7 initially **survived**. Case 9 sets a document's `related_fr` to free text and asserted only that
the gate exits non-zero. It does — but with the format check removed it fails on *index drift*,
because editing `related_fr` also moves the index. The case was passing for a reason unrelated to
the thing it names.

This is the same shape FR-130's case 9 had, and the same correction applies: an exit code is not an
attribution. Case 9 now requires the diagnostic `is not FR-<number>` in the gate's stderr, which no
other failure produces.

## 2026-07-25 follow-up: `docs/security` added to the governed roots

The post-closure audit of FR-132 found that `DOC_ROOTS` held two entries while `docs/security`
held **19 governed documents with zero frontmatter**, and the gate reported `12 passed, 0 failed`
throughout — green because those files were never looked at, which is the failure this ledger
exists to prevent.

They are not a different class of document. `docs/security/authorization/02-file-sharing-ceiling.md`
and `docs/security/file-security/02-workspace-home-isolation.md` are named in
`docs/feature_request/README.md` as the closure artifacts carrying FR-117 and FR-117-A, on the same
footing as a DD or a QA doc. `qa-doc-gen`'s own cross-doc scan has always listed `docs/security/`
beside `docs/qa/`. A security document goes stale exactly the way a design record does.

The cause was in FR-132's own wording — it scoped itself to "`docs/design_doc/**` 与 `docs/qa/**`" —
so the implementation was faithful to a requirement that was too narrow. This is the third instance
of the pattern FR-134 tracks: **an enumerated surface guards what was known when it was written.**
`governed_documents` derives coverage from the filesystem within each root, but the roster of roots
was itself a hand-written list of two.

Changes: `DOC_ROOTS` and `SCOPE` gained `docs/security`; all 19 documents were backfilled with
`lifecycle: active` and no `related_fr`. Attribution was deliberately left empty rather than
inferred from the first `FR-NNN` appearing in each file — that string is usually a citation, not
authorship, and guessing it would have put 11 unreviewed attributions into the reverse index.
The index moved from 380 to 399 documents; `byFeatureRequest` is unchanged at 244 across 121 FRs,
which is the visible consequence of not guessing.

Residual: `docs/uiux/` (11 documents) and `docs/showcases/` (36, both locales) remain outside the roots.
Showcases are published product surface governed by DD-143's publishing gate rather than closure
records, so they are deliberately out. `docs/uiux/` is the open question — it was not examined in
this pass and should be resolved the next time an FR touches it.

## Consequences

### What this establishes

- "Is this document still true?" is answerable by reading one line, for all 399 documents.
- The three documents FR-126 had to fence by hand now say so structurally, pointing at DD-138.
- A design doc reaches its feature request, and a superseded document reaches its successor.
- A new DD or QA document cannot land unclassified: the wrapper's case 1 runs the gate against the
  working tree inside a `ci-required` step.

### Accepted costs

- Every FR closure now regenerates the index in the same commit. It is one command, and it matches
  the discipline already established for the coordination and core-boundary ledgers.
- 378 documents gained a frontmatter block in the backfill, which shows up as a large one-time diff.
- The `**Status**` and `lifecycle` fields coexist. That is deliberate — they mean different things —
  but it does mean a reader must know which question they are asking.

### Known limits

- `lifecycle: active` on a backfilled document asserts only that nobody has marked it superseded. It
  is not evidence that the document was re-read against the code. The three supersessions are the
  only reviewed classifications here; the rest are a starting position that future FRs correct as
  they touch each area, which is what the FR's opportunistic strategy asked for.
- 136 documents carry no `related_fr`, so `byFeatureRequest` is a partial index. Completing it
  requires human attribution, not a script.
- The gate checks that `superseded_by` resolves to a file. It cannot check that the file it names
  actually describes the replacement — QA-182 covers that by human inspection for the three
  documents where it currently matters.
