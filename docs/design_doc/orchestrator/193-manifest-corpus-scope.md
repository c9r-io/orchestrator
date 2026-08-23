---
lifecycle: active
related_fr: FR-176
---

# DD-193: The Corpus Derived Its Files And Hard-Coded Its Directory

**Status**: Released

## The problem

`core/src/fixture_corpus_tests.rs` feeds every fixture manifest to
`validate_manifests` and compares the result against
`config/governance/fixture-bundle-validity.json`, requiring that **a rejection
match its declared diagnostic** rather than merely being a rejection. The
mechanism is right, and its own header says why the scope must be derived:

> **Scope is derived, never listed.** The corpus comes from `git ls-files`, so a
> bundle added tomorrow is in scope tomorrow.

What it derived was the *file list inside one hard-coded directory*:

```rust
const BUNDLE_GLOB: &str = "fixtures/manifests/bundles/*.yaml";
```

So a manifest added to that directory tomorrow was in scope tomorrow, and a
manifest added to any *other* directory was never in scope at all. Nothing
declared which directories should be governed. This is §4.4 shape 7 applied to
the **scope** rather than to a fixture's target: the enumeration is invisible
because it looks like a derivation.

Measured at `764b93de` and re-measured at `778b587a` after 33 commits: **34
tracked manifests outside the glob across four directories, 12 refused by the
product.** Ten are legitimate fragment shapes that a ledger entry simply
declares. Two are rot. And three — since repaired — were user-facing templates
that had never applied even once.

## What the gap cost

`docs/workflow/fr-watch.yaml` and `scheduled-scan.yaml` omitted the required
step `type:`, which has been mandatory since `245a43c8`. Two of FR-077's five
progressive templates **had never applied**, and `hello-world.yaml`'s own header
printed an apply command that failed.

`scripts/qa/test-agent-driver-production-parity.sh` names two of those files and
reads them with Ruby's `YAML.load_stream` to compare `spec.command`. Ruby parses
them; the product cannot. **That gate was green for the entire period both files
were unusable** — a proxy standing in for the fact, in a directory no corpus
covered.

## The predicate names both ends, deliberately

The obvious widening — "content contains `apiVersion: orchestrator.dev/v2`" —
is what FR-176 originally proposed, and governance step 0 measured it wrong in a
direction the FR had not considered.

`fixtures/manifests/bundles/crd-test-invalid.yaml` declares
`apiVersion: extensions.orchestrator.dev/v1`: a CRD extension resource
(`PromptLibrary`) that is **in the corpus today** and carries a `dependent`
ledger entry. Matching only v2 would have dropped it, orphaning its declaration
into `declaration names a path outside the corpus`.

Relaxing the other way — any `apiVersion:` — swallows the four Kubernetes
manifests in `project-bootstrap`'s template assets.

| predicate | bundles | newly scoped | total |
|---|---|---|---|
| `orchestrator.dev/v2` | 48 (**drops 1**) | 34 | 82 |
| any `apiVersion:` | 49 | 38 (**4 unrelated**) | 87 |
| **`orchestrator.dev/`** | **49** | **34** | **83** |

Measured over the tree: 449 `orchestrator.dev/v2`, 2
`extensions.orchestrator.dev/v1`, 2 `apps/v1`, 2 `v1`.

This is §4.4 shape 10 in its cheapest form: **widening a matcher to catch what it
missed opens the opposite end unless the opposite end is stated.** The under-reach
was the visible defect; the over-reach would have cost nothing until someone
added a Kubernetes manifest and wondered why the corpus wanted a declaration for
it.

Classification is by content, not path, and a file that cannot be read is a panic
rather than a skip — a corpus that silently drops what it cannot open is the
green-and-worthless state the original header warns about.

## Why `Status` needed no new variant

FR-176 asked the implementer to justify any new variant. None was needed. The
twelve rejections sort into the five that existed:

- **`dependent`** (10) — a sibling manifest supplies what they reference. The
  five `fixtures/benchmarks/agent-*.yaml` name stores declared in
  `secrets-*.yaml` **in the same directory**, verified present in the index
  rather than assumed; `workflow-benchmark-bootstrap.yaml` needs those agents
  applied first; `docs/workflow/full-qa.yaml` and `self-bootstrap.yaml` need
  `execution-profiles.yaml`, which `scripts/run-full-qa.sh` applies before them;
  `fixtures/workflow/full-qa.yaml` inherits that dependency from the original it
  forks.
- **`environment`** (1) — `docs/workflow/self-evolution.yaml` needs `minimax`,
  which lives only in `docs/workflow/minimax-secret.yaml`, **gitignored**
  (`.gitignore:13`) as a developer's local secrets file. It can therefore never
  be accepted from the index alone, and that is not a defect awaiting repair: the
  alternative is committing a secrets file. The distinction from `dependent`
  matters — the missing half is not in the repository at all.
- **`rotted`** (2) — the two `fixtures/workflow/` forks carrying constructs
  FR-173 retired.

`rotted_count` moves 0 → 2. The ratchet compares for equality in both directions,
so retiring rot has to move the number down and cannot drift up unnoticed.

## What the widening costs

`docs/workflow/` is a user-facing template directory and is now governed. Adding
a template there means making it valid or declaring why it is not. **That is the
point** — it is precisely where three templates rotted unnoticed — but it is a
real obligation on a directory whose authors may not expect one, and it belongs
in this record rather than being discovered.

`observe()` shares one `InnerState` across all calls (the header records 93 calls
at 1.8s rather than 24s with a fresh `TestState` each). At 83 manifests the suite
runs in ~1.4s.

## The two fixtures, and why the second exists

`an_injected_retired_construct_is_rejected_by_its_own_diagnostic` derives its
target as *the first accepted, undeclared manifest*. `git ls-files` order places
`fixtures/manifests/bundles/` early, so after the widening that fixture can keep
passing on a bundle **while every newly-scoped directory goes unexamined** —
which is the state this FR exists to end.

`a_manifest_outside_the_old_bundle_root_is_judged_too` excludes the old root by
construction and fails when nothing outside it is available. Driven against a
reverted widening it reports `the corpus scanned nothing outside
fixtures/manifests/bundles/, so the FR-176 widening is not in effect` — a named
diagnostic rather than a bare exit code.

Both assert the same two halves: the product refuses the injected construct *by
name*, and the evaluator surfaces the file as undeclared rot. The first alone
would be satisfied by a corpus that scanned the new directories while having no
ledger opinion about them.

## Known limits

- The `fixtures/workflow/` forks are declared `rotted`, not removed. FR-176 left
  their fate open, and it is a real question: they exist only to redirect
  `ticket_dir` away from `docs/ticket`, their maintained originals carry the
  flows, and the two citing QA scenarios have been repointed at those originals.
  Deleting them needs a different `ticket_dir` isolation mechanism for those
  scenarios; the ratchet ensures the decision cannot be forgotten quietly, since
  `rotted_count` only moves down.
- The corpus judges a manifest **standalone**. `dependent` and `environment`
  record why an entry cannot be judged that way, but nothing verifies that the
  sibling actually supplies what the reason claims — that was checked by hand
  here, per file, and is not mechanised.
- Four Kubernetes manifests under `project-bootstrap`'s template are excluded by
  the predicate. If that scaffold ever grows an orchestrator manifest, it joins
  the corpus automatically, which is the intended behaviour and untested.
