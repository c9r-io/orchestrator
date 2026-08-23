---
lifecycle: active
related_fr: FR-176
self_referential_safe: true
---

# QA: Manifest Corpus Scope (FR-176)

Verifies that the fixture corpus derives its scope from the index rather than
from a hard-coded directory, that every tracked manifest is either accepted or
declared, and that a manifest outside the old bundle root is actually judged.

Every scenario is `cargo test` or a read-only scan. No daemon starts, no database
is touched, no provider is invoked. `self_referential_safe: true`.

Design: [DD-193](../../design_doc/orchestrator/193-manifest-corpus-scope.md).

---

## Scenario 1: the scope is derived, and covers both ends of the predicate

### Steps

```bash
cargo test -p agent-orchestrator --lib fixture_corpus

# the predicate, checked directly
git ls-files -z '*.yaml' '*.yml' \
  | xargs -0 grep -lE "^apiVersion: .*orchestrator\.dev/" | wc -l
git ls-files -z '*.yaml' '*.yml' \
  | xargs -0 grep -lE "^apiVersion: (v1|apps/v1)$"
```

### Expected

- The suite passes and the corpus test reports **83** manifests.
- The scan returns the same 83.
- The second command lists only `project-bootstrap`'s four Kubernetes template
  files — correctly **excluded**.
- `fixtures/manifests/bundles/crd-test-invalid.yaml` is **included** despite being
  `extensions.orchestrator.dev/v1`. Matching only `orchestrator.dev/v2` would drop
  it and orphan its ledger entry; matching any `apiVersion:` would swallow the
  Kubernetes four. **Both ends have to be named** — assert both directions, not
  just the count.

---

## Scenario 2: every manifest is accepted or declared, with a matching diagnostic

### Steps

```bash
cargo test -p agent-orchestrator --lib every_tracked_bundle_is_accepted_or_declared
ruby -rjson -e 'd=JSON.parse(File.read("config/governance/fixture-bundle-validity.json"));
  puts "declared=#{d["bundles"].size} rotted_count=#{d["rotted_count"]}"'
```

### Expected

- Test passes; **25** declarations, `rotted_count` **2**.
- A rejection whose real diagnostic is not among its declared `expect` strings
  fails as `wrong diagnostic` — an exit code cannot distinguish which branch a
  gate failed through, which is why the ledger records the text.

---

## Scenario 3: the repaired templates stay repaired

This is what turns the widening into a guard rather than an inventory.

### Steps

```bash
cp docs/workflow/hello-world.yaml /tmp/hw.bak
# revert it to the broken shape: qa_targets back to []
cargo test -p agent-orchestrator --lib every_tracked_bundle_is_accepted_or_declared
cp /tmp/hw.bak docs/workflow/hello-world.yaml
```

### Expected

- Mutated, the corpus **fails** with a named diagnostic:
  `undeclared rejection: docs/workflow/hello-world.yaml ... [CODE_REPO_QA_TARGETS_REQUIRED]`.
- Restored, it passes.
- Before FR-176 this mutation was invisible: `docs/workflow/` was outside the
  corpus entirely, and the gate that names these files reads them with Ruby,
  which parses what the product refuses.

---

## Scenario 4: the rot ratchet is exact in both directions

### Steps

```bash
cp config/governance/fixture-bundle-validity.json /tmp/led.bak
# delete one `rotted` entry without changing rotted_count
cargo test -p agent-orchestrator --lib every_tracked_bundle_is_accepted_or_declared
cp /tmp/led.bak config/governance/fixture-bundle-validity.json
```

### Expected

- Two violations, not one: `undeclared rejection: fixtures/workflow/self-bootstrap.yaml`
  **and** `rot ratchet: rotted_count says 2 but 1 entries are declared rotted`.
- The ratchet compares for equality, so retiring rot must move the number down
  and it cannot drift up unnoticed.

---

## Scenario 5: a manifest outside the old bundle root is judged

### Steps

```bash
cargo test -p agent-orchestrator --lib a_manifest_outside_the_old_bundle_root_is_judged_too
```

To confirm it bites, revert the scope predicate to a path prefix and re-run.

### Expected

- Passes normally.
- With the widening reverted it fails with
  `the corpus scanned nothing outside fixtures/manifests/bundles/, so the FR-176
  widening is not in effect`.
- This case exists because the older injection fixture derives its target as *the
  first accepted, undeclared manifest*, and `git ls-files` order puts the old root
  early — so that fixture can stay green on a bundle while every newly-scoped
  directory goes unexamined, which is the exact state FR-176 exists to end.

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | S1 scope derived, both ends named | ☑ | 83 manifests; CRD extension in, Kubernetes four out |
| 2 | S2 accepted or declared, diagnostic matched | ☑ | 25 declarations, rotted_count 2 |
| 3 | S3 repaired templates stay repaired | ☑ | mutation caught with a named diagnostic |
| 4 | S4 ratchet exact both ways | ☑ | two violations reported, not one |
| 5 | S5 outside the old root is judged | ☑ | reverted widening reports it by name |
