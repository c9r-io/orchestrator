---
lifecycle: active
related_fr: FR-172
self_referential_safe: true
---

# Orchestrator - Governance Record Compaction

**Module**: FR registry / governance tooling
**Scope**: that a closure note stays within the 400-character bound; that the bound
counts characters rather than bytes; that the check sees the closure-note section
and nothing above it; that its refusal names the offending note; and that the
content moved out of the notes landed in governed documents rather than being lost
**Design**: [DD-190](../../design_doc/orchestrator/190-governance-record-compaction.md)
**Safety**: read-only against the working tree. The gate's own cases build synthetic
READMEs under `$TMPDIR`; no daemon, no database, no network.

## Prerequisites

```bash
ruby --version   # macOS ships 2.6; the checker avoids filter_map for that reason
```

## Scenario 1: the bound holds, and the check names what breaks it

**Steps**

```bash
ruby scripts/lib/fr_registry.rb notes
echo "exit=$?"
```

**Expected result**

Exit 0 and `closure notes: 400-character bound holds`.

To see the failure shape, append a long note to a scratch copy and run the checker
against that root — the gate does exactly this:

```bash
tmp=$(mktemp -d) && mkdir -p "$tmp/docs/feature_request"
cp docs/feature_request/README.md "$tmp/docs/feature_request/"
python3 -c "
import io,sys
p=sys.argv[1]
open(p,'a').write('- FR-999 ' + 'x'*501 + chr(10))
" "$tmp/docs/feature_request/README.md"
ruby scripts/lib/fr_registry.rb notes "$tmp"; echo "exit=$?"
rm -rf "$tmp"
```

Exit 1, with `FR-999 closure note is 510 characters, over the 400 bound`. The
diagnostic must name the note and its length: an exit code alone cannot tell an
author which of 170 notes to shorten.

## Scenario 2: the three fixtures, and the mutation each applies

**Steps**

```bash
./scripts/qa/test-governance-ledger-tooling.sh
```

**Expected result**

Exit 0, 18 passed. Four of those cases belong to this FR:

| Case | What it would catch |
|---|---|
| every closure note is within the bound | the tracked file regressing |
| an over-long note fails and the diagnostic names it | a check that refuses without saying which note |
| a 1,400-character preamble line is out of scope | a check measuring the file instead of the notes |
| a 399-character Chinese note passes | a byte-counting bound, which fails Chinese notes at a third of their real length |

Each fixture builds **its own synthetic README** rather than copying the tracked
one. That is deliberate: a fixture that copies the real file inherits whatever
state the real file is in, and during this FR's own implementation the real file
had 54 violations — fixtures built from it would have failed for a reason that had
nothing to do with what they test.

## Scenario 3: the check is scoped and cheap

**Steps**

```bash
grep -n 'if mode == "notes"' -A 3 scripts/lib/fr_registry.rb
git clone -q --depth=1 "file://$PWD" /tmp/fr-shallow && \
  ruby scripts/lib/fr_registry.rb notes /tmp/fr-shallow; echo "exit=$?"; rm -rf /tmp/fr-shallow
```

**Expected result**

`notes` dispatches **before** `render(root)`, so it never walks git history. The
shallow clone therefore answers exit 0 rather than failing with `repository is
shallow`, which is what `check` correctly does — a note-length question does not
need history, and making it pay for one would be the reason nobody runs it.

## Scenario 4: what left the notes is in a governed document

**Steps**

```bash
rg -n '^## Certification record' docs/qa/orchestrator/19[78]-*.md docs/qa/orchestrator/210-*.md
rg -c '924|200|pub mod' docs/design_doc/orchestrator/142-core-boundary-freeze.md \
                        docs/design_doc/orchestrator/148-persistence-crate-extraction.md
ruby scripts/qa/doc-lifecycle.rb
```

**Expected result**

Three `Certification record` sections exist, carrying the FR-147, FR-149 and
FR-158 sweep evidence that appeared in no other document — revision, clean
worktree at both ends, a derived rather than typed gate set, the empty uncovered
set, and the two reds that were not regressions.

FR-130's interim metrics resolve in DD-142 and DD-148, which is what permitted its
two interim notes to be deleted rather than compacted.

`doc-lifecycle.rb` exits 0: the QA documents that received the certification
records are governed, which the FR registry README is not — that asymmetry is the
reason the notes could duplicate design records indefinitely without any gate
noticing.

## Scenario 5: the reduction, and the registry survived it

**Steps**

```bash
wc -c docs/feature_request/README.md
ruby scripts/lib/fr_registry.rb check; echo "exit=$?"
./scripts/qa/test-markdown-link-integrity.sh
./scripts/qa-doc-lint.sh
```

**Expected result**

82,607 bytes, down from 193,821 — a net reduction of **111,214 bytes (57.4%)**.
The closure-note section fell from 90% of the file to 32%.

`fr_registry.rb check` exits 0, proving the generated block between the markers was
not touched: every edit was below `<!-- END GENERATED FR REGISTRY -->`. The link
and lint gates exit 0.

## Checklist

- [ ] Scenario 1: `notes` exits 0 on the tracked file; an over-long note exits 1
      naming the note and its length
- [ ] Scenario 2: all four FR-172 cases pass inside the ledger-tooling gate, each
      against a synthetic README
- [ ] Scenario 2: the Chinese-note fixture passes, so the bound counts characters
- [ ] Scenario 3: `notes` answers on a shallow clone, so it does not walk history
- [ ] Scenario 4: three `Certification record` sections exist; FR-130's metrics
      resolve in DD-142/148; `doc-lifecycle.rb` exits 0
- [ ] Scenario 5: the file is 82,607 bytes and `fr_registry.rb check` exits 0

## Known limits

- The bound cannot tell a pointer from prose. 400 characters of duplicated design
  record passes it. It arrests growth; it does not enforce purpose.
- The 47 template rewrites rest on a token-presence proxy (62–100% coverage), not
  on a reading of each note against its design record. Content that lived only in
  the discarded prose and carried no distinctive token is gone from the working
  tree; git history retains it.
- Three batch-header entries are not closure notes but are governed by this rule
  because they start with `- FR-`. They were compacted to fit rather than
  exempted, so a fourth audit batch will meet the same friction.
