# DD-139: QA Gate Enforcement Surface

**Status**: Implemented (FR-127)
**Related**: DD-138 (Agent driver execution migration), `guide-alignment.md`, QA 177

## Background

Governance in this repository was rigorous on the authoring side and nearly absent on the execution side. At the time FR-127 was filed:

| | |
|---|---|
| `scripts/qa/*.{sh,rb}` | 46 |
| referenced by `.github/workflows/` | 3 (two of them Slack certification) |
| referenced only by documentation | 38 |
| referenced by nothing | 5 |

The consequence was not a missing check but a missing *property*: a newly authored gate defaulted to "only the author knows to run it", and nothing detected that state. `scripts/qa-doc-lint.sh` — which invokes the FR-126 documentation alignment scan — ran in no workflow. `guide-alignment.md` asserted that the scan was called by "the FR-126 default release gate", but `release.yml` contained no such gate. A governance document had become a drift source about governance itself.

This is the structural reason FR-126 required four successive closure audits. Each audit found a new surface of drift that an executed gate would have caught the first time.

## Design

### The governed property

The design decision is to treat **enforcement status as a first-class artifact**, not as an emergent property of whoever last edited a workflow. Every `scripts/qa` gate must declare how it is enforced, and that declaration is checked against reality.

`config/governance/qa-gate-surface.json` holds one entry per gate:

- `enforcement`: `ci-required` | `manual-runbook` | `scheduled`
- `ci-required` additionally requires `workflow`, `job`, and `providerIsolation`; optionally `invokedBy` when a workflow runs the gate through a wrapper.
- `manual-runbook` and `scheduled` additionally require a non-empty `reason` and an `owner` document that exists on disk.

The manifest lives beside `coordination-collapse-ledger.json` under `config/governance/`, following the established convention that governance ledgers are machine-readable JSON checked by a script rather than prose in a design document.

### The five checks

`scripts/qa/test-qa-gate-surface.sh` verifies:

1. **Surface completeness** — disk and manifest agree in *both* directions. A new script cannot land unclassified, and a manifest entry cannot outlive its script.
2. **Reason and owner completeness** — every non-`ci-required` gate explains why it is not in CI and names a document that exists. "Temporarily unclassified" is not representable.
3. **Wiring truth** — the declared workflow job block is extracted and must actually reference the gate, directly or through the declared `invokedBy` wrapper. This is the durable form of "no gate may claim CI enforcement it does not have"; the claim that motivated the FR would now fail this check.
4. **Provider isolation** — see below.
5. **Stale enforcement claims** — no document may name a non-`ci-required` gate on a line that also asserts CI or release-gate enforcement.

Check 3 rather than check 5 is what makes the manifest self-correcting: a claim in JSON is verified against the workflow, so the manifest cannot drift from CI the way prose did.

### Provider isolation

The sharpest risk in wiring gates into CI is silently spending real provider credentials. Of 92 fixture bundles, four declare `provider: claude|codex`; two pin `binary: fake-*`; one is only applied, never executed. The fourth, `agent-driver-production-parity.yaml`, declares `provider: claude` with no override — its isolation rested entirely on a single line in `test-agent-driver-production-parity.sh`:

```bash
export PATH="$QA_ROOT/bin:$PATH"
```

If a refactor dropped that line, the test would still pass. It would simply start invoking the real `claude`, and nothing would report it. That is a silent-failure shape, and structural counting cannot detect it.

Three isolation modes are therefore declared and asserted per gate:

- `fixture-pinned` — every `claude|codex` agent in the named bundle also declares `binary: fake-*`.
- `path-shadow` — the gate copies a fake provider into an isolated bin directory *and* prepends it to `PATH`. Both halves are asserted.
- `no-provider` — no fixture bundle named by the gate carries an unpinned provider agent.

Defence in depth: the `governance` job also installs `claude` and `codex` stubs on `GITHUB_PATH` that print a diagnostic and exit 97. GitHub runners ship neither binary, so the stubs cost nothing today; their purpose is to convert a future accidental invocation from a silent quota charge into a build failure that names the cause.

### Fixture isolation

`--fixture-test` follows the `coverage-governance.sh --fixture-test` convention already wired into CI. Seven defects are injected into temporary copies of the governed inputs; the working tree is never modified.

Each fixture asserts more than "the gate failed". It asserts that the *targeted check* rejected the defect **and that every other check still passed on the same tree**. Without that second half, a fixture can pass for the wrong reason — a mangled copy trips check 1 and the fixture reports success while the check it was written to exercise was never evaluated.

### Certification aggregate

`test-agent-driver-execution-migration.sh` in default mode re-runs `cargo fmt`, strict Clippy, `cargo test --workspace`, `test-coordination-strangler.sh`, `coverage-governance.sh --fixture-test`, and `qa-doc-lint.sh` — every one of which already runs as a separate CI job. Running it unmodified in CI would roughly double the workflow.

DD-138 previously stated that the default aggregate *is* the release gate and that `FR126_FAST=1` is non-certifying. FR-127 amends that: **the certifying aggregate is the CI workflow, not any single script invocation.** The `governance` job runs the script with `FR126_FAST=1`, and the gates fast mode skips execute as sibling jobs in the same workflow. Outside CI, `FR126_FAST=1` remains non-certifying on its own, because a local run that skips those gates has not reproduced the aggregate.

### Orphan disposition

The five unreferenced scripts were each decided, with no "keep pending" option:

| Script | Decision | Rationale |
|---|---|---|
| `test-qa83-mixed-text.sh` | deleted | wrote to the legacy `data/agent_orchestrator.db` path that `qa-doc-lint.sh` bans in documentation; QA 83 stands alone as a runbook |
| `auto-regress.sh` | deleted | unmaintained generic runner over `./target/release/orchestrator`, zero callers, superseded by per-topic gates |
| `test-coordination-governance.sh` | `ci-required` | ruby-only wrapper, no daemon, no build |
| `test-filesystem-trigger.sh` | `ci-required` | static structure assertions over the fs watcher |
| `test-per-trigger-webhook-auth.sh` | `manual-runbook` | needs a daemon and webhook port 19091; bound to QA 129 |

Two further gates that were deterministic and daemon-free but unwired — `test-codex-session-resume.sh` (asserts a recorded transcript; the live counterpart is the separate `certify-codex-session-resume.sh`) and `test-legacy-coordination-decommission.sh` — were also promoted to `ci-required`. The surface is 12 `ci-required` of 45 gates.

## Consequences

### What the change proved on first run

Wiring produced two findings that structural review had not:

1. `check_no_stale_claims` found a third stale enforcement claim on its first execution — DD-124 described a human-invoked script as a "fail-fast clean-tree release gate". Two claims had been found by hand; the check found the one that was missed.
2. `test-legacy-coordination-decommission.sh` had been **failing** since FR-126. It asserted exactly 4 legacy command-only Agents; FR-126 migrated all of them and drove the count to 0. The gate was red for an entire FR cycle and nobody knew, because nothing executed it. The ratchet now asserts 0 and only tightens.

The second finding is the clearest statement of why this FR existed: the cost of an unwired gate is not that it might catch something later, but that it can be silently wrong for arbitrarily long.

### A corrected premise

FR-127 asserted that "the 5 Slack credential scripts" plus `certify-codex-session-resume.sh` must be classified non-`ci-required` with the reason "consumes real credentials/quota". Classifying by actual behavior found only **two** scripts that consume real provider credentials: `certify-slack-managed-live.sh` in its live subcommands, and `test-slack-managed-live-smoke.sh`, which posts to a real workspace using `SLACK_LIVE_*` bot tokens. The remaining Slack gates address synthetic hostnames (`qa-workspace.slack.com`, `private-fr114-workspace.slack.com`) or start a local fake Slack API server; they are non-CI because they need a daemon or the Node browser stack, which is a different reason and is recorded as such.

Classifying six scripts as credential-consuming when two are would have been a false statement inside the artifact whose purpose is to stop false statements about enforcement. Each entry therefore carries the reason that is true of it.

### Accepted costs

- `test-filesystem-trigger.sh` scenarios 1–2 re-run `cargo test --workspace` and strict Clippy, duplicating the sibling `test` and `clippy` jobs. Accepted because the `governance` job already builds the workspace and shares the Rust cache; recorded in its manifest entry rather than left implicit.
- `certify-slack-managed-live.sh` is `ci-required` because CI genuinely runs its read-only `status` subcommand. Its live subcommands consume real credentials and are human-invoked. The manifest records this split in a `note`, since `enforcement` is per-script and cannot express per-subcommand policy.

### Known limits

- The retired-semantics scan matches a curated list of literal phrases, not the general shape of a retired-configuration claim. A novel wording passes. FR-127 wires the existing assertion into CI; broadening it belongs to DD-138.
- `no-provider` is verified by checking fixture bundles a gate names literally. A gate that constructs a bundle path dynamically would evade it. The `GITHUB_PATH` stubs are the backstop for that case.
- Enforcement is classified per script. A gate whose subcommands differ in credential exposure (currently only `certify-slack-managed-live.sh`) relies on a prose `note`, which no check verifies.
