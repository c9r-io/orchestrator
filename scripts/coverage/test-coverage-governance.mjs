#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import {
  compareSummary,
  normalizeSourcePath,
  summarizeRust,
} from "./coverage-governance.mjs";

const fixtureRoot = path.resolve("scripts/coverage/fixtures");
const rust = JSON.parse(fs.readFileSync(path.join(fixtureRoot, "llvm-coverage.json"), "utf8"));
const supported = summarizeRust(rust, "/Users/qa/orchestrator", "supported");

assert.equal(
  normalizeSourcePath(
    "/Users/qa/orchestrator/crates/daemon/src/server/attention.rs",
    "/Users/qa/orchestrator",
  ),
  "crates/daemon/src/server/attention.rs",
);
assert.equal(
  normalizeSourcePath(
    String.raw`C:\agent\orchestrator\crates\cli\src\main.rs`,
    String.raw`C:\agent\orchestrator`,
  ),
  "crates/cli/src/main.rs",
);
assert.equal(supported.branchStatus, "supported");
// 70/75. The near-miss sibling described below belongs to the daemon component; it
// is only the key module it must stay out of.
assert.equal(supported.components["daemon adapter"].lines.percent, 93.33);
assert.equal(supported.keyModules["daemon/attention"].branches.percent, 50);

// FR-157 split `source_connection.rs` into a directory. The key-module prefix has to
// reach both forms, and it must not reach the test sources beneath them.
//
// The fixture holds `source_connection/oauth.rs` (10 lines, 8 covered),
// `source_connection.rs` (5 lines, 4 covered), `source_connection/tests/mod.rs`
// (100 lines, all covered) and `source_connections.rs` (50 lines, all covered) — a
// sibling whose name merely begins with the module's. A prefix ending in `.rs` sees
// only the second; counting the test file takes the module to 112/115; and a bare
// suffixless prefix swallows the near-miss sibling and reports 62/65 = 95.38%. All three
// mistakes change the numbers below, so this asserts the measured set rather than
// the spelling of a path.
assert.equal(supported.keyModules["daemon/source_connection"].lines.count, 15);
assert.equal(supported.keyModules["daemon/source_connection"].lines.covered, 12);
assert.equal(supported.keyModules["daemon/source_connection"].lines.percent, 80);

const unsupported = summarizeRust(rust, "/Users/qa/orchestrator", "unsupported");
assert.equal(unsupported.branchStatus, "unsupported");
assert.equal(unsupported.workspace.branches.status, "unsupported");
assert.equal(unsupported.workspace.branches.percent, null);

const baseline = {
  policy: { percentageTolerance: 0 },
  rust: {
    components: { "daemon adapter": supported.components["daemon adapter"] },
    keyModules: { "daemon/attention": supported.keyModules["daemon/attention"] },
  },
  frontend: {
    lines: { percent: 80 },
    functions: { percent: 80 },
    branches: { percent: 80, status: "supported" },
  },
  playwright: { minimumScenarios: 2 },
};
const passing = {
  rust: supported,
  frontend: {
    lines: { percent: 80 },
    functions: { percent: 81 },
    branches: { percent: 82, status: "supported" },
  },
  playwright: { total: 2, failed: 0 },
};
assert.deepEqual(compareSummary(passing, baseline), []);

const regression = structuredClone(passing);
regression.rust.components["daemon adapter"].lines.percent = 79.99;
regression.frontend.functions.percent = 70;
regression.playwright.total = 1;
const failures = compareSummary(regression, baseline);
assert.equal(failures.length, 3);
assert.ok(failures.some((failure) => failure.includes("daemon adapter lines")));
assert.ok(failures.some((failure) => failure.includes("React functions")));
assert.ok(failures.some((failure) => failure.includes("Playwright scenarios")));

// ── FR-165 requirement 3: the ratchet is two-sided ───────────────────────────
//
// The assertion at line 80 above is now load-bearing in a second way: that
// baseline declares no `improvementSlack`, and `supported` sits well above it in
// places, so it also proves an undeclared slack leaves the over-ratchet side off.
// That distinction is the difference between an upgrade that can be adopted and
// one that fails on arrival against every baseline written before the rule.
//
// Each case below builds its own objects. The existing regression case mutates a
// structuredClone of a shared summary, and reusing that here would have the cases
// observing each other's edits.
function pair(percent) {
  return { count: 100, covered: percent, percent };
}
// Branches must be present and explicitly unsupported. METRICS includes them, so
// omitting the key makes every case report "missing percentage" for branches on
// top of whatever it meant to test — which is how the first draft of these
// fixtures got three failures where it asserted two, and it would have been read
// as the band misfiring rather than as the fixture being wrong.
const noBranches = { status: "unsupported", count: null, covered: null, percent: null };
function entry(percent) {
  return { lines: pair(percent), functions: pair(percent), branches: noBranches };
}
function bandFixture({ approved, actual, slack }) {
  const declared = { percentageTolerance: 0 };
  if (slack !== undefined) {
    declared.improvementSlack = slack;
  }
  return [
    {
      rust: {
        components: { CLI: entry(actual) },
        keyModules: {},
      },
      frontend: { lines: pair(80), functions: pair(80), branches: { percent: 80, status: "supported" } },
      playwright: { total: 2, failed: 0 },
    },
    {
      policy: declared,
      rust: {
        components: { CLI: entry(approved) },
        keyModules: {},
      },
      frontend: { lines: pair(80), functions: pair(80), branches: { percent: 80, status: "supported" } },
      playwright: { minimumScenarios: 2 },
    },
  ];
}
const band = (options) => compareSummary(...bandFixture(options));

// The gap this requirement exists for: CLI measured 17.53 points above its
// approved value and kept passing, twice recorded and twice left alone.
const over = band({ approved: 35.49, actual: 53.02, slack: 3 });
assert.equal(over.length, 2, `expected both metrics to fail, got ${JSON.stringify(over)}`);
assert.ok(over.every((failure) => failure.includes("CLI")));
assert.ok(over.some((failure) => failure.includes("exceeds the approved 35.49%")));
assert.ok(over.some((failure) => failure.includes("17.53 points")));
assert.ok(
  over.every((failure) => failure.includes("re-approve")),
  "the diagnostic must say what to do; 'too high' alone reads like a bug in the gate",
);

// Ordinary drift from unrelated work must cost nothing, or the gate gets answered
// rather than read.
assert.deepEqual(band({ approved: 84.29, actual: 85.27, slack: 3 }), []);

// The boundary, from both sides. `>` rather than `>=` is the choice being pinned:
// a declared slack of 3 means three points are allowed, not two-point-nine-nine.
assert.deepEqual(band({ approved: 50, actual: 53, slack: 3 }), []);
assert.equal(band({ approved: 50, actual: 53.01, slack: 3 }).length, 2);

// Absent and zero are different declarations, and conflating them is how this
// change would have broken every existing baseline on its first run.
assert.deepEqual(band({ approved: 10, actual: 60 }), []);
assert.equal(band({ approved: 10, actual: 10.01, slack: 0 }).length, 2);

// Both sides at once: declaring a slack must not switch off regression detection,
// which is the direction the gate already had and the one it must not lose.
const under = band({ approved: 60, actual: 40, slack: 3 });
assert.equal(under.length, 2);
assert.ok(under.every((failure) => failure.includes("40% < 60%")));

// A metric the toolchain cannot produce must stay skipped rather than reading as
// an infinite improvement over `null`.
const unsupportedBranch = compareSummary(
  {
    rust: {
      components: { CLI: entry(50) },
      keyModules: {},
    },
    frontend: { lines: pair(80), functions: pair(80), branches: { percent: 80, status: "supported" } },
    playwright: { total: 2, failed: 0 },
  },
  {
    policy: { percentageTolerance: 0, improvementSlack: 3 },
    rust: {
      components: { CLI: entry(50) },
      keyModules: {},
    },
    frontend: { lines: pair(80), functions: pair(80), branches: { percent: 80, status: "supported" } },
    playwright: { minimumScenarios: 2 },
  },
);
assert.deepEqual(unsupportedBranch, []);

// The committed baseline must declare the band, or every assertion above is
// about a rule the repository does not actually apply.
const committed = JSON.parse(fs.readFileSync("coverage/boundary-baseline.json", "utf8"));
assert.equal(
  typeof committed.policy.improvementSlack,
  "number",
  "coverage/boundary-baseline.json must declare policy.improvementSlack",
);
assert.ok(
  String(committed.policy.improvementSlackRationale ?? "").length > 200,
  "the band needs its derivation written down; a bare number is the thing DD-172 refuses",
);

const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "fr122-coverage-"));
fs.writeFileSync(path.join(temporary, "pass"), "fixture tests reached filesystem cleanup\n");
fs.rmSync(temporary, { recursive: true });

console.log("coverage governance fixtures: PASS");
