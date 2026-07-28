---
lifecycle: active
related_fr: FR-133
---

# DD-156: the shape of the dependency graph becomes a decision

**Status**: Released
**FR**: FR-133
**QA**: `docs/qa/orchestrator/194-dependency-policy-gate.md`

## The problem

Dependency governance had two legs: `cargo audit` for known advisories and
dependabot for version age. Both answer *is this version old or known-bad*.
Neither answers anything about **the shape of the graph** — how many versions of
a crate coexist, under what licenses, from what registries.

The state was not wrong. It was **undecided**, and nothing distinguished
"reviewed and accepted" from "arrived unnoticed". For a repository that gates on
CVEs, that is an asymmetry: a vulnerability is blocked and the graph's growth is
not even counted.

## What the FR got wrong

Every number was rebuilt at `1b5615e2` with `cargo 1.96.0` and
`cargo-deny 0.20.2`. Six statements did not survive.

### `443` reproduces, and it is not "transitive dependencies"

`cargo tree --workspace --prefix none --locked | sort -u` on
`aarch64-apple-darwin` gives exactly **443** `name version` lines — and **all 14
workspace members are inside it**, so transitive dependencies on the host graph
are **429**. A second route, `cargo metadata`, gives **667** packages / **653**
non-member, because the lock spans every target platform. That is the graph
`cargo deny` evaluates, so it is the number that matters.

### `37` reproduces by a named route, and twelve of the thirty-seven are not duplicates

`cargo tree -d --workspace` on the host reports **37 names / 85 entries**. But
`-d` also reports a crate that resolves **once** and appears twice in the tree
under different feature sets — the normal vs build/proc-macro split. Twelve are
in that class:

    serde  serde_core  serde_json  log  prost  time
    typenum  semver  smallvec  deranged  stable_deref_trait  tauri-utils

Each has exactly one `[[package]]` entry in `Cargo.lock`. Counting name@version
pairs on the same host graph gives **25** duplicated names / 30 extra copies.

This is the **category conflation** shape by name: "coexisting versions" and
"appears twice in the tree" are different constructs, and `cargo deny check
bans` counts the first.

### The number the gate enforces is 48

`cargo deny check bans` with `multiple-versions = "deny"`, `--workspace
--all-features`: **48 `error[duplicate]`**, identical without `--all-features`.
(`Cargo.lock` shows 50 duplicated names; `bit-vec` and `schemars` are lock
entries outside the resolved graph, as is `rand 0.10.2` — which is why the lock
says `rand ×3` and cargo-deny says 2.)

Classifying "the 37" would have left eleven-plus unclassified and failed the
gate on its first run. It fails in the direction that reads like completion.

### The "可统一" category is empty

Only four of the 48 are declared by any workspace crate, and none is unifiable:

| crate | we declare | the other version comes from |
|---|---|---|
| `base64` | `^0.22` in 4 crates | `0.21.7` ← `swift-rs` ← tauri |
| `sha2` | `^0.11` in 7 crates | `0.10.9` ← the GUI subtree |
| `reqwest` | `^0.12` in 4 crates | `0.13.4` ← tauri 2.11 |
| `rand` | `^0.8` in 4 crates | `0.9.5` ← `tauri-plugin-notification` |

`rand` looks like the exception and is not: `rand 0.8.7` is *also* pinned by
`cron 0.16 → phf 0.11 → phf_macros → phf_generator 0.11.3`, so raising our four
declarations across nine API-breaking call sites removes **zero** duplicates.

**No production dependency moved in this FR.** Every entry is an acceptance.

### Requirement 5 inverts: neither advisory tool subsumes the other

|  | `cargo audit` | `cargo deny check advisories` (`version = 2`) |
|---|---|---|
| exit | **0** | **1** |
| unmaintained | 17, as *warnings* | 17, as **errors** |
| unsound | **1** — RUSTSEC-2024-0429 | **not reported at all** |

Both read the same RustSec database; cargo-deny freshly fetched its own clone
into `~/.cargo/advisory-dbs/`. So "keep one" would have dropped a live finding
whichever one it kept, and adopting cargo-deny's advisories as-is would have
turned 17 upstream-archived transitives into a red build on day one.

RUSTSEC-2024-0429 is `glib 0.18.5`: `VariantStrIter::impl_get` passes `&p` to a
C function that writes through the pointer as an out-argument; recent rustc
disregards those writes, so `CStr::from_ptr` receives NULL. Affects
`>=0.15.0,<0.20.0`, patched in `>=0.20.0`, reachable only as
`orchestrator-gui → tauri 2.11 → gtk 0.18 → glib 0.18.5`.

### Requirement 4 needs no new 口径

DD-153 already fixed the rule: *exact equality for quantities derived from the
tree, thresholds only for quantities that are measured.* A skip-list length is
neither — it is a committed file, so a baseline number in a second committed
file is one fact stored twice, changing in the same commit, where review already
sees the diff.

## The design

### `deny.toml`, and why it has no blanket

48 crates, **70** accepted extra copies, each naming the dependency that
introduces it and the version being kept. The entries were generated from
cargo-deny's own output rather than typed, so the "來源依賴" the FR asked for is
derived rather than remembered.

There is deliberately **no `skip-tree`**. One `skip-tree = [{ crate = "tauri" }]`
absorbs 28 of the 48 in a single line — and goes on absorbing duplicates *that
do not exist yet*, forever, silently.

This is worth naming as a shape, because it is the mirror of one already on
record. §4.4 shape 2 says a hand-listed set guards exactly what was known the
day it was written, and its tell is a list that grows by one entry per audit
round. **A blanket is the other failure mode, and it is harder to see**: it
guards nothing at all and never produces a line in any log. A list that stops
growing at least looks suspicious.

### The shape of the acceptance list, measured

`cargo deny check bans --exclude orchestrator-gui` reports **20**, so **28 of the
48 exist only because the desktop GUI is in the workspace**. Ten of the
remaining twenty are the `windows-*` import libraries for targets we do not
build. The daemon and CLI graph carries **ten**:

    cpufeatures  crypto-common  getrandom  hashbrown  phf
    phf_shared   r-efi          syn        thiserror  thiserror-impl

That is the reviewable list, and it is the axis the reasons are written along.
Twenty-eight of the reasons share one sentence about the Tauri 2 / gtk-rs 0.18
generation; writing 48 *different* sentences would have been fiction.

### The advisory split

**One tool per question, no overlap.**

- `cargo-deny` owns graph shape: `bans`, `licenses`, `sources`. Its CI
  invocation names exactly those three — never `advisories`, never `all`.
- `cargo audit` owns the advisory database, and now runs `--deny unsound`.

RUSTSEC-2024-0429 goes into `.cargo/audit.toml` with its reason and the
condition that retires it (`cargo tree -i glib` reporting 0.20). The 17
unmaintained stay warnings: they are upstream-archived crates we cannot move off
while Tauri 2 ships gtk-rs 0.18, and denying them would produce an 18-entry
ignore file that *is* the policy.

The net change is small and it is the FR's actual goal: a silent `exit 0` over a
real unsoundness becomes a dated acceptance, and the *next* unsound advisory
reds the build instead of joining a pile of eighteen.

### The third question, and why it needs a gate of its own

`cargo deny` proves the policy **holds**. Nothing proved it still **binds** —
that the invocation carries the flags that make it enforce anything. That is a
different claim, and it is the one this repository keeps finding broken: FR-127
that wired is not running, FR-137 that an aggregation nobody guarded swallowed
every failure, FR-144 that a gate can print PASS over input it could not read.

`scripts/qa/dependency-policy.rb` reads four artefacts and asks whether they
still agree. Seven rules:

| rule | asserts |
|---|---|
| `deny-job-present` | a job runs `cargo deny`, and no step of it is `continue-on-error` |
| `ratchet-armed` | that invocation carries `--deny unmatched-skip` |
| `checks-partitioned` | its check list is exactly `bans licenses sources` |
| `severity-binding` | `deny.toml` sets the four severities to `deny` |
| `every-acceptance-reasoned` | every skip has a non-empty `reason`; every licence exception has a comment |
| `no-blanket` | `skip-tree` is absent or empty |
| `skip-is-live` | every skip names a crate that is really duplicated in `Cargo.lock`, at the version written |
| `audit-unsound-denied` | `cargo audit` runs `--deny unsound`, and every ignored id carries a reason |

Everything is **parsed, never grepped** — the workflow through
`scripts/lib/workflow_model.rb`, so a commented-out step is not a step; the TOML
through a small reader in the gate itself, because counting brackets per line is
§4.4 shape 3 and this gate's whole subject is not letting a proxy stand alone.
The reader recognises comments only outside strings, which is why `skip-tree`
appearing inside a `reason` string or in this file's own prose is not a
`skip-tree`.

All seven have **zero violations today**. Each is a guard rather than a repair,
which makes the fixtures the only evidence any of them works.

### `--deny unmatched-skip` does less than it sounds like

This was measured, not assumed, and the first version of the fixture is what
found it.

`unmatched-skip` asks whether a `skip` entry matched a crate **in the graph**. It
says nothing about whether that crate is still *duplicated*. A skip for
`serde@1.0.229` — one version, no duplicate — passes cargo-deny cleanly. Only
the shape where the named version has left the graph entirely fails.

So the ratchet takes two observers, and neither alone closes it:

| the acceptance's version… | caught by |
|---|---|
| has left the graph | `cargo deny --deny unmatched-skip` |
| is still present, but no longer duplicated | `skip-is-live`, from `Cargo.lock` |

`skip-is-live` also needs no cargo-deny binary, which is why it can run in
ci.yml's governance job where there is none. QA 194 scenario 5 case 15b is the
fixture that holds the two apart; it exists because case 15 failed and the
reason turned out to be this.

## Where it runs

| | workflow | job | blocking |
|---|---|---|---|
| `cargo deny check bans licenses sources` | `security.yml` | `cargo-deny` | yes, no `continue-on-error` |
| `test-dependency-policy.sh --tool-fixtures` | `security.yml` | `cargo-deny` | yes |
| `cargo audit --deny unsound` | `security.yml` | `cargo-audit` | yes |
| `dependency-policy.rb` | `ci.yml` | `governance` | via `OUTCOMES` |
| `test-dependency-policy.sh` | `ci.yml` | `governance` | via `OUTCOMES` |

`cargo-deny` is the first `ci-required` gate this repository has executed outside
`ci.yml`; the enforcement surface already carried `workflow` per entry, so no
change was needed to accommodate it. `ci-step-cost.json`'s 2700 s budget covers
only ci.yml's `governance` and `ci-environment-parity`, so the security workflow
does not consume it.

## Known limits

- **The gate finds the *first* `cargo deny` invocation.** A second job running
  cargo-deny differently is invisible to it. Acceptable while `security.yml` has
  two jobs; if the workflow grows, the rule should iterate.
- **`skip-is-live` reads the lock, not the resolved graph.** A crate present in
  `Cargo.lock` under two versions but resolvable to one — which is exactly what
  `bit-vec` and `schemars` are — would satisfy it while cargo-deny reports no
  duplicate. The direction is safe (it under-reports rather than over-reports)
  and the tool covers the other side, but it is not the same set.
- **Check 8 of `test-qa-gate-surface.sh` cannot tell `command -v cargo` from
  running cargo.** `test-dependency-policy.sh` writes `>/dev/null` without
  `2>&1` for that reason, with a comment saying so. The rule is right; the
  spelling is the accommodation.
- **Nothing judges the *quality* of a reason.** `every-acceptance-reasoned`
  requires a non-empty string; a future entry reading `reason = "x"` satisfies
  it. The 70 committed reasons were generated from cargo-deny's output, so each
  names the dependency that introduces it, but that is a property of how they
  were written and not one the gate enforces. Review is the only thing on that
  path, which is the honest position: a machine cannot tell a justification from
  a placeholder.
- **The advisory acceptance is dated, not scheduled.** Nothing reminds anyone to
  re-check RUSTSEC-2024-0429; `cargo audit` simply stops reporting it once Tauri
  moves, and the entry then has to be removed by hand. cargo-audit has no
  unmatched-ignore diagnostic to lean on the way cargo-deny does.

## What the certification sweep found in someone else's gate

The sweep is the reason this is written down. Running 41 derived gates back to
back, `scripts/qa-doc-lint.sh` reported

    FAIL: CHANGELOG [Unreleased] does not name the removed runner selection seam

with `RunnerExecutorKind` sitting at CHANGELOG line 74, inside the `[Unreleased]`
extent. Ten isolated re-runs passed. The assertion is

    printf '%s' "$UNRELEASED" | rg -q 'RunnerExecutorKind' || fail "..."

and `rg -q` exits on its first match. The section is **90047 bytes**, past the
64 KB pipe buffer, so `printf` is still writing when rg leaves, dies of EPIPE,
and `set -o pipefail` hands that status to the `||`. Measured: **10 spurious
failures in 400 runs under CPU load**, and recorded here as **0 in 400 idle**;
with a here-string, **0 in 400 under the same load**. Four sites in that gate
were converted and re-measured.

**Both of the paragraph's conclusions were wrong, and FR-145 corrected them.**
The idle rate is **8-13 in 400** on the same machine and the same input, so
CPU contention raises the rate by about a quarter rather than being what makes
the defect possible. And it does not fail *closed*: that holds only where the
match feeds the passing branch. Where the match feeds the **failing** branch —
`! producer | grep -q SECRET`, which is how three of this repository's leak
assertions were written — the same code reports a real violation as clean,
measured 2 in 200. See DD-157.

The failing-open half is why it matters that the reasoning below was recorded at
all: the direction was inferred from the single instance that happened to be
observed, and the instance that was observed was the harmless one.

FR-133 did not introduce it: the pipe predates this FR and the section already
exceeded the buffer. It did make it likelier, by adding ~4 KB of very long lines.
The systemic case — recorded here as **42 sites across 9 ci-required gates**,
which FR-145 re-derived as **35 executable sites across 7**, because `grep -c`
counted four comment lines describing the pattern; the repository-wide figure is
**63 sites across 22 files** — is `docs/design_doc/orchestrator/157-pipefail-short-circuit.md`
and QA-195. "Most have provably bounded producers" did not survive either: what
decides the trigger is match position and line structure, not size. With the
measurement method rather than a blanket rewrite, because converting 42 sites
without measuring which producers can exceed the buffer would turn one supported
fix into an unmeasured sweep across nine unrelated gates.

## Measurement

Derived at `1b5615e2` unless stated; `cargo 1.96.0`, `cargo-deny 0.20.2`
(prebuilt `aarch64-apple-darwin`, sha256 `fe67d82a…`, matching the published
checksum), system Ruby 2.6.

| | FR-133 as filed | measured |
|---|---|---|
| dependencies | 443 "transitive" | **443** on the host tree *including* 14 members → **429**; **653** external in the lock |
| duplicated crates | 37 | **48** as cargo-deny counts them; 25 by name@version on the host; 37 by `cargo tree -d`, twelve of which have one version |
| accepted extra copies | not counted | **70** |
| unifiable by us | "升级/统一" implied | **0** |
| GUI-only duplicates | not identified | **28** of 48 |
| licence expressions | not counted | **34**, zero missing, **1** needing an exception |
| `sources` findings | implied outstanding | **0** — a guard, not a repair |
| advisories | "重叠" | **18** from cargo audit, **17** from cargo deny, and neither set contains the other |
