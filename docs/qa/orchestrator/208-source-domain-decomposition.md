---
lifecycle: active
related_fr: FR-157
self_referential_safe: true
---

# Orchestrator - Source Domain Decomposition And Action-Audit Vocabulary

**Module**: Orchestrator Daemon / gRPC Surface / Coverage Governance
**Scope**: SourceConnection handler behaviour against a Gateway stub, module size
ceiling, key-module coverage measurement, action-audit vocabulary consolidation
**Scenarios**: 6
**Priority**: High

## Background

FR-157 raised `daemon/source_connection` from 12.21% to 85.51% line coverage,
split its 2572-line implementation into six modules, and collapsed the
action-audit vocabulary to one definition site per term. Design:
`docs/design_doc/orchestrator/170-source-domain-decomposition.md`.

Every scenario here is safe to run against a developer machine. Nothing starts a
daemon, opens the runtime database, or writes outside `target/`. The behavioural
scenarios run in-process against `TestState` (a `tempfile` directory) and an
axum stub bound to `127.0.0.1:0`; the structural scenarios read files.

## Scenario 1: The handler suite passes and actually reaches the Gateway

**Steps**

```bash
cargo test -p orchestratord source_connection
```

**Expected result**

`test result: ok. 60 passed; 0 failed`.

The count matters less than what the assertions observe. Spot-check that the
suite is behavioural rather than structural:

```bash
rg -c 'stub\.call\(|stub\.was_called\(|stub\.paths\(\)' \
  crates/daemon/src/server/source_connection/tests/
```

Every test file that drives a mutating handler asserts on a recorded stub call.
A handler that silently skipped a Gateway request would still return `Ok`; these
assertions are what distinguish the two.

## Scenario 2: The suite fails when the state machine is broken

The negative fixture comments a line out rather than deleting it — deletion is
the mutation the author had in mind, and the assertion must survive the one they
did not.

**Steps**

```bash
cp crates/daemon/src/server/source_connection/oauth.rs /tmp/oauth.bak

# Comment out the owner-boundary fence in reconcile_intent, leaving the block
# it guarded in place so the file still compiles.
python3 - <<'PY'
p = "crates/daemon/src/server/source_connection/oauth.rs"
s = open(p).read()
old = """    if installation.owner_daemon_id != daemon_id || installation.owner_project_id != project_id {
        return Err(Status::permission_denied(
            "Gateway installation owner boundary mismatch",
        ));
    }"""
assert old in s, "the fence moved; re-derive it before concluding anything"
s = s.replace(old, "\n".join("    // " + l.strip() for l in old.split("\n")))
open(p, "w").write(s)
PY

cargo test -p orchestratord source_connection > /tmp/mutated.log 2>&1
echo "exit=$?"
grep -c 'FAILED' /tmp/mutated.log
cp /tmp/oauth.bak crates/daemon/src/server/source_connection/oauth.rs
```

**Expected result**

`exit=101`, and
`a_completed_intent_owned_by_another_daemon_is_refused` appears among the
failures. The `assert!(old in s)` is not decoration: if the fence has moved, the
run must fail on that assertion rather than proceed to a green result that means
nothing.

Restore the file and re-run scenario 1 to confirm the tree is clean again.

## Scenario 3: The module size ceiling is derived, not listed

**Steps**

```bash
cargo test -p orchestratord module_shape
find crates/daemon/src/server/source_connection -name '*.rs' -not -path '*/tests/*' \
  | xargs wc -l | sort -n
```

**Expected result**

Two tests pass. Every production file under the module is below 1000 lines, and
there are at least four of them.

The second test exists because a size ceiling alone is satisfiable without
decomposing anything: one 999-line file passes it. Confirm the derivation is
live rather than vacuous:

```bash
# A scan that finds nothing must fail, not pass.
mv crates/daemon/src/server/source_connection /tmp/sc-moved 2>/dev/null \
  && cargo test -p orchestratord module_shape 2>&1 | tail -5 \
  ; mv /tmp/sc-moved crates/daemon/src/server/source_connection
```

The build will not compile with the module moved, which is itself the failure —
the point is that the scan cannot report success over an empty set.

## Scenario 4: The key-module coverage prefix measures the split module

This is the check that would have failed silently. The split moved the
implementation into a directory; the normalizer matched a prefix ending in `.rs`,
which reaches a file and nothing beneath a directory of the same name.

**Steps**

```bash
node scripts/coverage/test-coverage-governance.mjs; echo "exit=$?"

cp scripts/coverage/coverage-governance.mjs /tmp/cg.bak
BARE='"daemon/source_connection": ["crates/daemon/src/server/source_connection"],'
DOTRS='"daemon/source_connection": ["crates/daemon/src/server/source_connection.rs"],'

# Fixture 1: the over-reaching suffixless prefix.
perl -0pi -e 's/"daemon\/source_connection": \[\n[^\]]*\],/'"$BARE"'/' scripts/coverage/coverage-governance.mjs
node scripts/coverage/test-coverage-governance.mjs > /tmp/m1.log 2>&1; echo "exit=$?"
grep -m1 'actual:' /tmp/m1.log
cp /tmp/cg.bak scripts/coverage/coverage-governance.mjs

# Fixture 2: the original .rs-only prefix.
perl -0pi -e 's/"daemon\/source_connection": \[\n[^\]]*\],/'"$DOTRS"'/' scripts/coverage/coverage-governance.mjs
node scripts/coverage/test-coverage-governance.mjs > /tmp/m2.log 2>&1; echo "exit=$?"
grep -m1 'actual:' /tmp/m2.log
cp /tmp/cg.bak scripts/coverage/coverage-governance.mjs

# Fixture 3: drop the /tests/ exclusion.
perl -pi -e 's/sourcePath\.includes\("\/tests\/"\) \|\|/false ||/' scripts/coverage/coverage-governance.mjs
node scripts/coverage/test-coverage-governance.mjs > /tmp/m3.log 2>&1; echo "exit=$?"
grep -m1 'actual:' /tmp/m3.log
cp /tmp/cg.bak scripts/coverage/coverage-governance.mjs

node scripts/coverage/test-coverage-governance.mjs; echo "exit=$?"
```

**Expected result**

Unmutated: `coverage governance fixtures: PASS`, `exit=0`. Each fixture exits `1`
with its own number, which is what distinguishes them from one another:

| Mutation | `actual:` | Meaning |
|---|---|---|
| suffixless prefix | `65` | it swallowed the near-miss sibling `source_connections.rs` |
| `.rs`-only prefix | `5` | it saw only the pre-split file, none of the directory |
| no `/tests/` rule | `97.14` | the component counted a 100-line test source |

Restored: `PASS`, `exit=0`.

An exit code alone cannot distinguish the branch a gate failed through from any
other, which is why the table asserts the diagnostic rather than the status.

Capture the exit status directly as shown. Piping into `head` or `tail` reports
the pager's status, not the script's.

## Scenario 5: The coverage number reproduces, by more than one route

**Steps**

```bash
git rev-parse HEAD                       # record it; compare it after
mkdir -p target/coverage-governance
cargo llvm-cov --workspace --all-targets --all-features \
  --json --output-path target/coverage-governance/rust.json
cargo llvm-cov report --lcov --output-path target/coverage-governance/rust.lcov

# Route 1 — the gate's own summarizer.
node --input-type=module -e '
import fs from "node:fs";
const { summarizeRust } = await import(process.cwd() + "/scripts/coverage/coverage-governance.mjs");
const raw = JSON.parse(fs.readFileSync("target/coverage-governance/rust.json", "utf8"));
const m = summarizeRust(raw, process.cwd(), "unsupported").keyModules["daemon/source_connection"];
console.log("route 1:", JSON.stringify(m.lines));
'

# Route 2 — the same JSON, selected independently; route 3 — LCOV DA records.
python3 - <<'PY'
import json, os
root = os.getcwd() + "/"
prefix = "crates/daemon/src/server/source_connection"
files = [f for e in json.load(open("target/coverage-governance/rust.json"))["data"]
         for f in e.get("files", [])]
sel = [(f["summary"]["lines"]["count"], f["summary"]["lines"]["covered"])
       for f in files
       if f["filename"].replace(root, "").startswith(prefix)
       and "/tests/" not in f["filename"]]
c, v = sum(a for a, _ in sel), sum(b for _, b in sel)
print(f"route 2: {v}/{c} = {v*100/c:.2f}%")
cur, da = None, {}
for line in open("target/coverage-governance/rust.lcov"):
    line = line.strip()
    if line.startswith("SF:"):
        cur = line[3:].replace(root, "")
    elif line.startswith("DA:") and cur and cur.startswith(prefix) and "/tests/" not in cur:
        n, h = line[3:].split(",")[:2]
        da[(cur, int(n))] = da.get((cur, int(n)), 0) + int(h)
hit = sum(1 for x in da.values() if x > 0)
print(f"route 3: {hit}/{len(da)} = {hit*100/len(da):.2f}%")
PY
git rev-parse HEAD                       # must equal the first reading
```

**Expected result**

Routes 1 and 2 agree exactly. Route 3 counts a different unit (LCOV `DA` records
rather than llvm region-derived lines) and lands within about 1.5 points. All
three are far above the `daemon adapter` mean of 28.77% that FR-157 set as the
floor. Recorded at `497339b9` on macos-aarch64 with cargo-llvm-cov 0.8.5:

| Route | Result |
|---|---|
| `summarizeRust` | 2089/2443 = 85.51% |
| JSON per-file, independent selection | 2089/2443 = 85.51% |
| LCOV `DA` | 2029/2335 = 86.90% |

A single route is not evidence. `grep -c` counting lines instead of occurrences,
and a text pattern standing in for a reachability property, are both recorded
failures of exactly this shape.

**Denominator check.** `2443` must be at least 95% of the approved `2482`. It is
98.4%. The 39-line fall is the module's inline tests moving into `tests/`;
holding the old denominator, the same numerator is 84.2%, so the rise is
executed production code rather than a shrunken denominator. Run this check
whenever the baseline is re-approved — it is the one direction in which a
coverage rise can be manufactured.

## Scenario 6: The action-audit vocabulary has one definition site per term

**Steps**

```bash
# Structural: the derived scan.
cargo test -p orchestrator-config action_audit_vocabulary

# Behavioural: what actually reaches the audit table.
cargo test -p orchestratord boundary_contract

# Independent cross-check by a second tool, counting occurrences not lines.
grep -rno '"legacy_client"\|"compatibility"\|"enforced"' crates/ core/ --include='*.rs'
```

**Expected result**

Both test invocations pass. The `grep -rno` output is exactly four lines:

```
crates/orchestrator-config/src/cli_types.rs:1177:"compatibility"
crates/orchestrator-config/src/cli_types.rs:1180:"enforced"
crates/daemon/src/server/action_audit.rs:82:"legacy_client"
crates/orchestrator-scheduler/src/scheduler/coordination_tools.rs:1019:"compatibility"
```

The first three are the constant definitions. The fourth is a task summary that
happens to be the same word; its file names none of `action_audit_mode`,
`fallback_reason_code` or `reason_code`, so the derived scan never considers it.
It is excluded by derivation, not by an allowlist — confirm that rather than
taking it on trust:

```bash
grep -c 'action_audit_mode\|fallback_reason_code\|reason_code' \
  crates/orchestrator-scheduler/src/scheduler/coordination_tools.rs   # → 0
```

The marker set is deliberately wider than the two exact field names. Scoping to
those alone covers today's tree and goes blind on a new production file writing a
reason code through a differently named field; `reason_code` widens the
considered set from 19 files to 52, and the gate still passes.

**The structural check alone is not sufficient** and must never be run without
the behavioural one. Replacing a literal with a constant compiles, satisfies
every count, and can change what reaches the audit table.
`a_context_free_mutation_is_still_audited_under_the_legacy_client_reason_code`
asserts the recorded `reason_code` against the literal string the wire has
always carried, and
`enforced_mode_refuses_a_mutation_that_falls_back_to_legacy_client` drives a real
`RuntimePolicy` to prove the enforced branch still rejects it while an explicit
reason code is still admitted.

### Negative fixtures

```bash
BAK=$(mktemp -d); cp crates/daemon/src/server/action_audit.rs "$BAK/"
cp crates/orchestrator-config/src/cli_types.rs "$BAK/"

# A: a production reference commented out, the literal restored.
python3 -c "
p='crates/daemon/src/server/action_audit.rs';s=open(p).read()
old='            agent_orchestrator::cli_types::ACTION_AUDIT_MODE_COMPATIBILITY.to_string()'
assert old in s, 'the call site moved; re-derive before concluding anything'
s=s.replace(old,'            // '+old.strip()+chr(10)+'            \"compatibility\".to_string()',1)
open(p,'w').write(s)"
cargo test -p orchestrator-config action_audit_vocabulary > /tmp/a.log 2>&1; echo "A exit=$?"
grep -m1 -A2 'spelled out in production' /tmp/a.log
cp "$BAK/action_audit.rs" crates/daemon/src/server/

# B: a second definition site.
python3 -c "
p='crates/daemon/src/server/action_audit.rs';s=open(p).read()
s=s.replace('pub(crate) const FALLBACK_REASON_LEGACY_CLIENT: &str = \"legacy_client\";',
            'pub(crate) const FALLBACK_REASON_LEGACY_CLIENT: &str = \"legacy_client\";\nconst LEGACY_ALIAS: &str = \"legacy_client\";')
open(p,'w').write(s)"
cargo test -p orchestrator-config action_audit_vocabulary > /tmp/b.log 2>&1; echo "B exit=$?"
grep -m1 'exactly one definition site' /tmp/b.log
cp "$BAK/action_audit.rs" crates/daemon/src/server/

# C: the scan's own derivation stops matching anything.
python3 -c "
p='crates/orchestrator-config/src/cli_types.rs';s=open(p).read()
s=s.replace('const MARKERS: [&str; 2] = [\"action_audit_mode\", \"fallback_reason_code\"];',
            'const MARKERS: [&str; 2] = [\"nothing_matches_this\", \"nor_this\"];')
open(p,'w').write(s)"
cargo test -p orchestrator-config action_audit_vocabulary > /tmp/c.log 2>&1; echo "C exit=$?"
grep -m1 'derivation is broken' /tmp/c.log
cp "$BAK/cli_types.rs" crates/orchestrator-config/src/

cargo test -p orchestrator-config action_audit_vocabulary; echo "restored exit=$?"
```

**Expected result**

All three mutations exit `101` with distinct diagnostics — the file and line of
the stray literal; `"legacy_client" must have exactly one definition site`; and
`the derivation is broken`. The restored run passes.

Fixture C guards the case a green log cannot distinguish from success: a scan
whose derivation stopped matching reports zero strays and zero duplicate
definitions over zero files. Assert the diagnostic, not only the exit code.

**A mutation that correctly does nothing.** Reintroducing the literal inside
`core/src/resource/project.rs` does *not* fail the gate: that file's
`action_audit_mode` site sits after its own `#[cfg(test)]` at line 106, so it is
test code by the same derivation the gate uses everywhere else. Recorded because
"the gate did not fire" and "the gate is broken" look identical until you check
which line you actually changed.
