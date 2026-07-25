---
lifecycle: active
related_fr: FR-128
---

# DD-140: Governance Ledger Regeneration And Review

**Status**: Implemented (FR-128)
**Related**: DD-136 (coordination strangler), DD-137 (legacy decommission), DD-138 (Agent driver migration), DD-139 (gate enforcement surface), QA 178

## Background

`config/governance/coordination-collapse-ledger.json` holds the reviewed state of FR-124/125/126: eleven
production Workflows with approved coordination touches, twenty production Agents each pinned by a
SHA-256 `manifestFingerprint` over its canonicalised `kind`/`metadata.name`/`spec`, and four source-touch
ratchets. `scripts/qa/coordination-governance.rb` compares the repository against it.

The comparison is strict equality. Changing one `args` element of one production Agent turns the gate red.
That friction is deliberate — it is the mechanism by which an Agent change reaches a reviewer. But before
FR-128 the tooling around it was:

- no `--emit`, `--fix`, or regenerate mode of any kind;
- a failure message reading, in full, `production Agent execution inventory differs from the reviewed
  ledger` — no agent name, no field, no fingerprint.

Friction without tooling does not produce better review. It produces one of two evasions: skipping the
gate, or degenerating the ledger update into "red means paste the new hash", which retires the word
*reviewed* from `reviewed ledger` while leaving it in the identifier.

### A corrected premise

FR-128 stated that the only route back to green was recomputing SHA-256 by hand. That is false, and worth
recording because the FR file is deleted. `--output` writes its report at `coordination-governance.rb`
before the error exit, so the report is produced even on failure, and it already carries the correct new
fingerprints under `.executionInventory.agents[]`. An undocumented recovery path existed. Nobody was doing
hash arithmetic; they were either guessing a `jq` incantation or not recovering at all. The real absence
was diagnosis, which is why the mismatch report below is the larger half of this change.

FR-128 also treated `sourceBaseline` as having "the same problem" as the fingerprint inventory. It does
not, and the difference is what §3 is about.

## Design

### Regeneration emits the compared value, not a second opinion

The inventory the gate compares and the inventory `--emit-inventory` prints are one expression:

```ruby
def production_agent_inventory(agents)
  agents
    .sort_by { |agent| [agent["file"], agent["name"]] }
    .map { |agent| agent.slice(*INVENTORY_FIELDS) }
end
```

A regenerated candidate therefore cannot differ from the checked value in ordering or field selection.
This is a structural guarantee rather than an asserted one: there is no second implementation that a test
must keep in step with. QA 178 case 1 nonetheless diffs emitter output against the ledger slice, so a
future refactor that reintroduces a parallel path fails immediately.

`--emit-baseline` does the same for the four source ratchets. `--write` merges a candidate into the ledger
and **refuses when `CI` is set**: in CI there is no reviewer, and an automatic ledger rewrite would convert
the review gate into decoration. That is the one requirement in this FR whose value is entirely negative —
it exists to prevent a convenience that would dissolve the mechanism.

`--write` must also be *quiet*. Ruby's `JSON.pretty_generate` renders an empty array as `[\n\n]` while the
reviewed ledger uses `[]`, so a naive round trip moved nineteen lines nobody asked to change and buried the
one real edit. `ledger_json` normalises both empty collections, and QA 178 case 3 asserts that a no-op
`--write` leaves the file byte-identical.

### The mismatch report, and why `HEAD` is the reviewed spec

Requirement 2 asked the gate to name the changed spec key. Taken literally against the ledger alone, that
is impossible: **the ledger stores a fingerprint and never a spec**, so the previous spec is recorded
nowhere in it. A hash is not invertible and does not decompose by key.

The reviewed spec is instead recovered from `git show HEAD:<file>`. This is well defined precisely because
of requirement 4's constraint that the ledger and the spec it describes land in one commit — if that holds,
`HEAD` *is* the last reviewed pairing. The rule is therefore not documentation sitting beside the tool; it
is the tool's precondition.

Which makes its violation detectable. If the spec was committed without the ledger, the working tree and
`HEAD` agree while the fingerprints do not, and the report says so rather than inventing a key list:

```
- production Agent execution inventory differs from the reviewed ledger
-   ~ docs/workflow/command-rules.yaml#session-agent: manifestFingerprint changed but the spec
    already matches HEAD, so the spec was committed without its ledger update; they must land in one commit
```

Against the intended workflow it names the field directly:

```
-   ~ docs/workflow/command-rules.yaml#session-agent: manifestFingerprint changed in spec key(s): driver
```

Added and removed agents are reported as `+` and `-`. Degradations are explicit: an unreadable `HEAD` copy
or an agent absent from it produce a stated limitation, never a guess.

### The ratchets are a different mechanism, and both halves were wrong

`productionAgents` is compared by strict equality. `sourceBaseline` was compared as `count > baseline` — a
monotonic ratchet in which a *decrease passes silently*. Two live defects followed, neither fixable by
regeneration tooling, since regenerating from a wrong scanner only re-blesses a wrong number.

**Scope infidelity.** `sourceBaseline.scope` reads "excluding inline `cfg(test)` modules". The
implementation stripped a single **trailing** `#[cfg(test)] mod tests { … }` per file, so a test module
named anything else, or followed by production code, was scanned in full. Ten counted lines lived inside
`cfg(test)` modules: nine `PipelineVariables` in `crates/orchestrator-scheduler/src/scheduler/item_executor/{dispatch,apply}.rs`
and one `output_json_path` in `core/src/task_repository/mod.rs`. `pipelineVariables: 39` was 30 production
plus 9 test. `strip_test_modules` now brace-matches every `cfg(test)` module.

**Silent slack.** `capturesOrJsonPath` stood at 54 against a reviewed 55 — green, and false. The comparison
is now exact in both directions, with the diagnostic naming the direction and pointing at `--emit-baseline`.
Corrected baselines: **53 / 30 / 9 / 0**.

Exactness is affordable only because regeneration is now cheap; the two halves of this FR are load-bearing
for each other. `scripts/qa/test-legacy-coordination-decommission.sh` keeps a `<= 53` bound as defence in
depth rather than duplicating the exact figure, so the Ruby gate remains the single source of exactness.

### Verification by mutation

QA 178's eight cases were each shown to fail against a targeted mutation of the implementation, because a
gate that has only ever been observed passing has not been observed doing anything. Reverting the empty-collection
normalisation breaks case 3 (19 lines moved); reverting the stripper to the trailing-`mod tests` form breaks
case 7; reverting `!=` to `>` breaks case 8; comparing only fingerprints breaks case 5; removing the report
breaks case 4; reversing the emitter's field order breaks case 1; removing the `CI` guard breaks case 2.

Two cases were rewritten because the mutation run exposed them as weaker than intended, which is the point
of running it:

- case 5 originally grepped for the word `classification`, so removing the *report* failed it even though
  the bypass protection was intact. It now asserts the precondition that the doctored ledger's fingerprint
  is already current, so a surviving failure can only come from a field the reviewer did not update.
- case 7 originally `eval`'d `strip_test_modules` out of the source and tested it directly. That proves a
  function exists, not that the counting path calls it — the same textual-presence-as-execution-fact error
  FR-134 documents in the FR-127 gate. It now injects a mid-file `#[cfg(test)] mod fr128_scope_probe`
  containing counted tokens and requires the *emitted baseline* not to move.

## Review workflow

The ledger is updated by a human reading a diff, never by a job.

1. Change a production Agent spec under `docs/workflow/`.
2. `ruby scripts/qa/coordination-governance.rb` fails and names the agent and the changed spec keys.
3. `ruby scripts/qa/coordination-governance.rb --emit-inventory` prints the candidate; review it against
   the manifest change that caused it. `--emit-baseline` does the same for the source ratchets.
4. Apply it with `--write` (locally only), or redirect the output yourself.
5. **Commit the ledger with the spec change, in one commit.** This is a constraint, not a convention: it is
   what makes step 2's diff well defined, and splitting it leaves every intermediate revision failing the
   gate. The tool reports the violation explicitly when it happens.

## Consequences

### Accepted costs

- Exact source ratchets mean any refactor touching `captures`, `json_path`, `PipelineVariables`, or
  `cel_interpreter` requires a same-commit ledger edit, including refactors that *reduce* the counts.
  This is the intended trade: the alternative is a baseline that drifts upward from reality unobserved.
- `--write` rewrites the whole ledger through a JSON round trip. Byte-identity is asserted (case 3) rather
  than assumed, but the guarantee is only as good as `ledger_json`'s normalisation of Ruby's formatting.

### Known limits

- The mismatch report's spec diff is top-level keys only. A change nested inside `spec.driver.options` is
  reported as `driver`, and the reviewer reads the manifest diff for the rest.
- `git show HEAD:` makes the report depend on repository state. In a shallow or detached checkout without
  the parent blob the report degrades to naming the agent — correct, but less useful.
- Fingerprint canonicalisation is unchanged by design, so every historical fingerprint remains valid; the
  cost is that the ledger still cannot answer "what did this spec used to be" without git.
- The four ratchets remain line-count regexes. They cannot distinguish a genuine new consumer from a
  comment mentioning `captures`, and FR-128 did not change that.
