---
lifecycle: active
related_fr: FR-165
---

# 181. Two-sided ratchets: coverage drift and advisory acceptances

**Status**: Released

FR-165 requirements 3 and 4 are one lesson applied to two ledgers. Both were
ratchets that could only ever tighten in one direction, and in both cases the
missing direction was not a lost opportunity to reward good news — it was the loss
of the detection the ratchet existed for.

## Requirement 3: a one-sided coverage ratchet stops measuring

`scripts/coverage/coverage-governance.mjs` had exactly one comparison:

```js
if (current.percent + tolerance < approved.percent) { ... }
```

Nothing failed when an entry sat above its approved value, and one had:
`coverage/boundary-baseline.json`'s own reapproval note recorded CLI at 52.86%
against an approved 35.49% and said the entries "keep passing while
under-ratcheted, which is the same gap the 2026-07-27 note records" — the same
observation, twice, a week apart, with nothing arranged to make it stop.

The consequence is worse than the annotation suggests. With approved at 35.49 and
actual at 53.02, **CLI coverage could have fallen seventeen points and the gate
would have stayed green.** Everything down to the stale approved value is
permitted. A drifted baseline is not a conservative ruler; it is a ruler that has
stopped measuring, and the drift is invisible precisely because the gate is
passing.

### The band, and why 3.0

Failing on any improvement was rejected. It turns every coverage-raising change red
until someone edits a JSON file, and answering the gate rather than the question is
the reflex FR-158's freshness work was built to avoid. So `policy.improvementSlack`
declares how far above approved an entry may drift before it must be re-approved.

The number is derived from the measured distribution rather than chosen. Measured
at `15f54289` (macos-aarch64, cargo-llvm-cov 0.8.5, 22 metric pairs across
components and key modules):

| Drift | Pairs |
|---|---|
| exactly 0.00 | 12 |
| +0.95 … +1.25 | 4 |
| +4.87 … +5.17 | 2 |
| +10.88 … +24.12 | 4 |

The interval (1.25, 4.87) is **empty**. A band inside it is stable: moving it a
point does not change which entries fail unless the distribution itself changes.
3.0 sits in that gap, nearer the incidental side so the check errs toward asking —
about 2.4× the largest drift that came from unrelated work, about 1.6× below the
smallest deliberate gap.

A band of 5.0 was rejected for a specific reason worth recording: it splits
`daemon/session` across its own two metrics (lines +5.17 fails, functions +4.87
passes). A rule that treats one module's two measurements differently is a rule
fitted to a number rather than to a property, and it would have been the cheaper
choice — 5.0 leaves one fewer entry to re-approve.

### Absent is not zero

`improvementSlack` missing from a baseline means "not declared", and the code
distinguishes that from `0` explicitly. Defaulting a missing field to zero would
have failed every entry in every pre-existing baseline on the first run, and an
upgrade that breaks on arrival gets reverted rather than adopted. Two fixtures pin
the distinction: 50 points above with no declaration passes, 0.01 above with
`slack: 0` fails.

### The re-approval reverses an earlier decision, deliberately

Three entries moved: CLI, `cli/commands`, `daemon/session`. Everything else was
left alone because its drift is inside the band.

The 2026-08-02 note declined to move CLI on the grounds that "that movement belongs
to the FRs that caused it, and this run is not their evidence." The principle is
sound and the consequence was not. Attribution is a reason to credit the
improvement to another FR in the record; it is not a reason to leave the ruler
wrong. `coverage/README.md` now says so, because that sentence is what a future
reviewer will reach for when the same situation arises.

### The 52.86% did not reproduce

The second derivation is **53.02%** at this revision, on a denominator that grew
7373 → 7456. Neither figure was wrong: the earlier one was measured before commits
that added CLI code. That is the argument for pinning a revision to every coverage
number, and `coverage/README.md`'s Baseline Updates list now requires it. The FR
carried 52.86% as a single-source figure taken from the baseline's own prose, which
is how it came to be quoted for two weeks without anyone re-measuring it.

### Tauri: measured, not exempted, and not given an invented target

Tauri Rust is 9.42% / 7.73% and `tauri/commands` 5.45% / 3.47% — the lowest
measurable surfaces in the workspace. The FR asked for a lift plan or a named
exemption. Neither is written as a number, and `policy.tauriLift` records why: no
work here touched that code, and a target chosen without having tried to test it is
exactly the figure DD-172's expansion boundary refuses. What is recorded is the
measurement and the reason it is low — `crates/gui` is a `#[tauri::command]` layer
reachable only through the Tauri runtime, and the workspace has no in-process
harness that can invoke it. That is the same shape as `daemon/source_connection`
before FR-157, which went 12.21% → 85.51% once a fixture could reach it, and which
was low for the same reason: not hard to test, never entered.

An exemption was specifically **not** written, because an exemption is what stops
anyone looking again. The two-sided ratchet now applies to these entries like any
other, so the first real lift is caught and re-approved instead of accumulating.

## Requirement 4: an acceptance nobody can retire never retires

`.cargo/audit.toml` carries 18 advisory acceptances (derived twice:
`grep -c '^\s*"RUSTSEC'` = 18, and `grep -o '"RUSTSEC-[0-9-]*"' | wc -l` = 18; a
plain `rg -c RUSTSEC` gives 20 because it counts comment lines, which is where the
FR's original "19" came from). `dependency-policy.rb`'s `check_audit` asserted only
that each ignore had *a* comment above it.

That is §4.4 shape 1 in the place it costs most. The retirement conditions were
there — every block states a `cargo tree -i <crate>` — but they were prose, for a
human, and nobody read them. An acceptance whose crate had left the tree stayed
forever: accepting nothing, and holding the advisory ID reserved against the day
something else brings that crate back. cargo-audit has no
`--deny unmatched-ignore`; `cargo deny --deny unmatched-skip` is the nearest thing
and governs the other file.

### Declared, not inferred

Each entry now carries `# retire-when: crate=<name> absent` or
`# retire-when: crate=<name> patched>=<version>`, and
`check_audit_retirement` enforces it against `Cargo.lock`.

Parsing the existing prose was considered and rejected. The gtk block states
`cargo tree -i gtk` **once above eleven entries**, each of which then says "retires
with the block condition above" — so walking up from an entry reaches its own
one-line comment and never the group's command. Reading that indirection would be
treating a paragraph's layout as data. A declaration per entry is one shape, and an
entry without one fails: that is the ratchet, since a new acceptance cannot be
added without saying what would end it.

### The reverse instance is the whole point

`skip-is-live` on the deny side has three branches, and FR-133 recorded that
`--deny unmatched-skip` cannot reach the third: a crate that is still present but
no longer duplicated. The audit analogue is **present but already fixed**, and a
presence-only check cannot see it — glib reaching 0.20 retires RUSTSEC-2024-0429
while glib stays in the lock. The FR named this explicitly so the gap would not
repeat on the audit side, and `patched>=` is the answer.

Which form applies is the advisory's kind, not a preference. An *unmaintained*
advisory has no patched release — the crate is archived, that is the advisory — so
`absent` is the only condition it can have. Seventeen of the eighteen are that.

### Version comparison is numeric, and both directions are pinned

`"1.0.15" >= "0.9.0"` is **false** as text, because "1" sorts before "9". A lexical
comparison would go on accepting a fixed advisory and say nothing. The committed
state exercises the other direction — glib 0.18.5 against a 0.20.0 bound, where a
text compare of the minor component ("18" < "20") happens to agree — which is why
the fixture that matters is the `paste 1.0.15` against `patched>=0.9.0` case. Both
are asserted, along with a mirror case (`gtk` 0.18.2 against a 0.18.3 bound stays
accepted) so the check cannot satisfy every case by reporting "fixed"
unconditionally.

## What the closure self-check found

Two defects in this FR's own work, both from asking §5's question — what state
satisfies every acceptance criterion while the goal is unmet.

**The fixtures proved the mechanism and said nothing about the committed band.**
Every band case declares its own `slack`, so a baseline shipping
`improvementSlack: 100` passed all of them while restoring exactly the unbounded
interval requirement 3 exists to close. The criterion ("the baseline is updated and
the two-sided rule is written down, with a behavioural assertion that the next
improvement is capped") was fully satisfied by that state. The repair asserts the
committed value is ≤ 5.0, because the drifts this requirement was filed to catch
began at +4.87: a band at or above 5 admits them, which is the same as not having
one. Raising the ceiling now has to happen in a diff rather than by editing a data
file, which is the reviewable act the rule wants. Verified by setting the committed
slack to 100 and watching the assertion name it.

**`.cargo/audit.toml`'s header described behaviour that did not exist.** It said
"the gate prints which form each entry used, so a `patched>=` that quietly became
`absent` is visible in the log" — and the gate printed no such thing. The claim was
written while designing the check and never implemented, so the file documented a
safeguard the repository did not have, which is worse than documenting no
safeguard. `dependency-policy.rb` now prints `Advisory acceptances: 18 total — 17
absent, 1 patched>=` on every run, derived from the markers rather than restated,
which also makes the header's "17 unmaintained acceptances" checkable by reading
one line of output.

The second is the one worth generalising: this FR spent its whole length on gates
that certify claims, and it still shipped a prose claim about its own gate's
behaviour that nothing checked. The header text and the implementation were written
in the same sitting, in different files, and only re-reading the file as a reader
rather than as its author caught it.

## Known limits

- ~~**The first CI run is the verification of the re-approved baseline.**~~
  **Verified.** Run `31565308833` at `0d9d09f6` reports
  `Boundary coverage non-regression: success` — the three re-approved entries and
  the 3.0-point band reproduce on `macos-latest`, so the measurement taken here
  transferred without needing the 0.05 tolerance as cover. The prediction that it
  would is recorded above rather than deleted, because the reason it was uncertain
  (a figure measured on one host governing a job on another) is the reason
  `coverage/README.md` now requires a pinned revision beside every number.
- **Frontend and Playwright coverage were not measured here.** Node 26 breaks the
  GUI unit suite (`docs/ticket/20260812-node-version-unpinned-locally.md`) and this
  host has no Playwright browsers. The Rust side is what requirement 3 changed and
  what was measured; the two-sided rule applies to the React entries through the
  same code path but has never been exercised against a real React measurement.
- **`.cargo/audit.toml` still states counts about itself in prose** — "an 18-entry
  ignore file", "The 17 unmaintained acceptances", "11 × gtk-rs", "5 × unic-*",
  "1 × paste". All five are correct today and all are now derivable from the
  `retire-when` markers (18 total, 1 with a `patched>=` bound, 17 with `absent`),
  so the deny-side `check_prose_counts` has a straightforward analogue here. It was
  not built: requirement 4's criterion is the unmatched ratchet, and DD-179 set the
  precedent of recording this class of gap rather than expanding scope into it.
  They will go stale again.
- **The band is per metric, not per entry.** An entry whose lines drift +2.9 and
  functions +3.1 fails on one metric and not the other, which is correct but reads
  oddly in a log. Rejecting 5.0 avoided the case where that split is caused by the
  band's own placement; it does not prevent a future measurement from landing
  astride 3.0.
- **A misclassified `retire-when` passes.** The gate checks that the named crate is
  in the lock and, if a bound is declared, that the lock is below it. It cannot
  check that the named crate is the advisory's actual subject, or that an
  `absent`-form entry really has no patched release upstream. Both are human
  judgements, which is why each entry keeps its prose justification alongside the
  marker.
