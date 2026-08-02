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
assert.equal(supported.components["daemon adapter"].lines.percent, 80);
assert.equal(supported.keyModules["daemon/attention"].branches.percent, 50);

// FR-157 split `source_connection.rs` into a directory. The key-module prefix has to
// reach both forms, and it must not reach the test sources beneath them.
//
// The fixture holds `source_connection/oauth.rs` (10 lines, 8 covered),
// `source_connection.rs` (5 lines, 4 covered) and `source_connection/tests/mod.rs`
// (100 lines, all covered). A prefix ending in `.rs` would see only the second, and
// counting the test file would take the module to 112/115. Both mistakes change the
// number below, so this asserts the measured set rather than the spelling of a path.
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

const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "fr122-coverage-"));
fs.writeFileSync(path.join(temporary, "pass"), "fixture tests reached filesystem cleanup\n");
fs.rmSync(temporary, { recursive: true });

console.log("coverage governance fixtures: PASS");
