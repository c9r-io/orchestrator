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
