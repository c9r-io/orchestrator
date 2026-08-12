#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const METRICS = ["lines", "functions", "branches"];

function usage() {
  console.error(`Usage:
  node scripts/coverage/coverage-governance.mjs summarize \\
    --rust <llvm.json> --frontend <coverage-summary.json> \\
    --playwright <playwright.json> --output <summary.json> \\
    [--repo-root <path>] [--branch-status supported|unsupported]
  node scripts/coverage/coverage-governance.mjs check \\
    --summary <summary.json> --baseline <baseline.json>
`);
}

function parseArgs(argv) {
  const [command, ...rest] = argv;
  const options = {};
  for (let index = 0; index < rest.length; index += 1) {
    const key = rest[index];
    if (!key.startsWith("--") || index + 1 >= rest.length) {
      throw new Error(`invalid argument: ${key}`);
    }
    options[key.slice(2)] = rest[index + 1];
    index += 1;
  }
  return { command, options };
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

export function normalizeSourcePath(filename, repoRoot = process.cwd()) {
  const normalized = filename.replaceAll("\\", "/");
  const root = path.resolve(repoRoot).replaceAll("\\", "/").replace(/\/+$/, "");
  if (normalized === root) {
    return "";
  }
  if (normalized.startsWith(`${root}/`)) {
    return normalized.slice(root.length + 1);
  }
  const workspaceMarker = "/orchestrator/";
  const markerIndex = normalized.lastIndexOf(workspaceMarker);
  if (markerIndex >= 0) {
    return normalized.slice(markerIndex + workspaceMarker.length);
  }
  return normalized.replace(/^[A-Za-z]:\//, "").replace(/^\/+/, "");
}

function emptyMetric() {
  return { count: 0, covered: 0, percent: 0 };
}

function emptyCoverage(branchStatus) {
  return {
    lines: emptyMetric(),
    functions: emptyMetric(),
    branches:
      branchStatus === "supported"
        ? emptyMetric()
        : { status: "unsupported", count: null, covered: null, percent: null },
  };
}

function addMetric(target, source) {
  if (!source || typeof source.count !== "number" || typeof source.covered !== "number") {
    return;
  }
  target.count += source.count;
  target.covered += source.covered;
}

function finishCoverage(coverage, branchStatus) {
  for (const metric of METRICS) {
    if (metric === "branches" && branchStatus !== "supported") {
      coverage.branches = {
        status: "unsupported",
        count: null,
        covered: null,
        percent: null,
      };
      continue;
    }
    const value = coverage[metric];
    value.percent =
      value.count === 0 ? 100 : Number(((value.covered / value.count) * 100).toFixed(2));
  }
  return coverage;
}

const COMPONENTS = {
  "core/domain": [
    "core/",
    "crates/orchestrator-collab/",
    "crates/orchestrator-config/",
    "crates/orchestrator-runner/",
    "crates/orchestrator-security/",
    "crates/orchestrator-scheduler/",
    "crates/orchestrator-client/",
    "crates/slack-gateway/",
  ],
  "daemon adapter": ["crates/daemon/"],
  CLI: ["crates/cli/"],
  "Tauri Rust": ["crates/gui/"],
};

const KEY_MODULES = {
  "daemon/attention": ["crates/daemon/src/server/attention.rs"],
  "daemon/handoff": ["crates/daemon/src/server/handoff.rs"],
  "daemon/session": ["crates/daemon/src/server/session.rs"],
  // Two prefixes, both exact. `matchingBucket` compares with `startsWith`, so a
  // prefix ending in `.rs` matches the single file and nothing beneath a directory
  // of the same name — FR-157 split this module into `source_connection/`, and with
  // the old prefix every submodule would have dropped out of the key module
  // silently, shrinking the denominator and raising the percentage by measuring
  // less. Dropping the suffix fixes that and overshoots in the other direction: a
  // bare `crates/daemon/src/server/source_connection` also matches any sibling
  // whose name merely begins with it, so a future `source_connections.rs` would
  // dilute the module without anyone choosing that. Naming the file and the
  // directory separately is the only form that is exact in both directions.
  "daemon/source_connection": [
    "crates/daemon/src/server/source_connection.rs",
    "crates/daemon/src/server/source_connection/",
  ],
  "daemon/action_audit": ["crates/daemon/src/server/action_audit.rs"],
  "cli/commands": ["crates/cli/src/commands/"],
  "tauri/commands": ["crates/gui/src/commands/"],
};

function isExcluded(sourcePath) {
  return (
    sourcePath.includes("/target/") ||
    sourcePath.startsWith("target/") ||
    sourcePath.includes("/tests/") ||
    sourcePath.endsWith("/build.rs") ||
    sourcePath === "core/src/test_utils.rs"
  );
}

function matchingBucket(sourcePath, buckets) {
  return Object.entries(buckets).find(([, prefixes]) =>
    prefixes.some((prefix) => sourcePath === prefix || sourcePath.startsWith(prefix)),
  )?.[0];
}

export function summarizeRust(raw, repoRoot, requestedBranchStatus) {
  const files = raw?.data?.flatMap((entry) => entry.files ?? []) ?? [];
  const hasBranchCounts = files.some(
    (file) =>
      typeof file?.summary?.branches?.count === "number" &&
      file.summary.branches.count > 0,
  );
  const branchStatus =
    requestedBranchStatus === "unsupported" || !hasBranchCounts ? "unsupported" : "supported";
  const components = Object.fromEntries(
    Object.keys(COMPONENTS).map((name) => [name, emptyCoverage(branchStatus)]),
  );
  const keyModules = Object.fromEntries(
    Object.keys(KEY_MODULES).map((name) => [name, emptyCoverage(branchStatus)]),
  );
  const workspace = emptyCoverage(branchStatus);
  let includedFiles = 0;

  for (const file of files) {
    const sourcePath = normalizeSourcePath(file.filename, repoRoot);
    if (isExcluded(sourcePath)) {
      continue;
    }
    const component = matchingBucket(sourcePath, COMPONENTS);
    if (!component) {
      continue;
    }
    includedFiles += 1;
    const module = matchingBucket(sourcePath, KEY_MODULES);
    for (const metric of METRICS) {
      if (metric === "branches" && branchStatus !== "supported") {
        continue;
      }
      addMetric(workspace[metric], file.summary?.[metric]);
      addMetric(components[component][metric], file.summary?.[metric]);
      if (module) {
        addMetric(keyModules[module][metric], file.summary?.[metric]);
      }
    }
  }

  return {
    branchStatus,
    includedFiles,
    workspace: finishCoverage(workspace, branchStatus),
    components: Object.fromEntries(
      Object.entries(components).map(([name, value]) => [
        name,
        finishCoverage(value, branchStatus),
      ]),
    ),
    keyModules: Object.fromEntries(
      Object.entries(keyModules).map(([name, value]) => [
        name,
        finishCoverage(value, branchStatus),
      ]),
    ),
    exclusions: [
      "target/**",
      "**/tests/**",
      "**/build.rs",
      "core/src/test_utils.rs",
      "generated sources outside the repository root",
    ],
  };
}

function frontendMetric(value) {
  if (!value || typeof value.total !== "number" || typeof value.covered !== "number") {
    return emptyMetric();
  }
  return {
    count: value.total,
    covered: value.covered,
    percent: Number(value.pct ?? 0),
  };
}

export function summarizeFrontend(raw) {
  const total = raw?.total ?? {};
  return {
    lines: frontendMetric(total.lines),
    functions: frontendMetric(total.functions),
    branches: {
      ...frontendMetric(total.branches),
      status: "supported",
    },
  };
}

function countPlaywrightTests(suite) {
  let total = 0;
  let passed = 0;
  let failed = 0;
  let skipped = 0;
  for (const spec of suite?.specs ?? []) {
    for (const test of spec.tests ?? []) {
      total += 1;
      const results = test.results ?? [];
      const last = results.at(-1);
      if (test.status === "skipped" || last?.status === "skipped") {
        skipped += 1;
      } else if (test.status === "expected" && last?.status === "passed") {
        passed += 1;
      } else {
        failed += 1;
      }
    }
  }
  for (const child of suite?.suites ?? []) {
    const nested = countPlaywrightTests(child);
    total += nested.total;
    passed += nested.passed;
    failed += nested.failed;
    skipped += nested.skipped;
  }
  return { total, passed, failed, skipped };
}

export function summarizePlaywright(raw) {
  const aggregate = { total: 0, passed: 0, failed: 0, skipped: 0 };
  for (const suite of raw?.suites ?? []) {
    const counts = countPlaywrightTests(suite);
    for (const key of Object.keys(aggregate)) {
      aggregate[key] += counts[key];
    }
  }
  return {
    ...aggregate,
    status: aggregate.failed === 0 ? "passed" : "failed",
    coverageMetric: "scenario",
  };
}

// The comparison is two-sided, and the second side is why (FR-165 requirement 3).
//
// Until now this returned only on regression, so an entry could sit indefinitely
// far *above* its approved value and keep passing. That is not a hypothetical:
// the baseline's own 2026-08-02 reapproval note recorded CLI at 52.86% against an
// approved 35.49% and wrote that the entries "keep passing while
// under-ratcheted, which is the same gap the 2026-07-27 note records" — the same
// observation, twice, a week apart, with nothing to make it stop. A one-sided
// ratchet does not merely fail to reward improvement; it silently loses the
// ability to detect a *later* regression, because everything down to the stale
// approved value is permitted. CLI could have fallen 17 points and stayed green.
//
// Improvement is not made to fail outright, which would turn every
// coverage-raising change red until someone edited a JSON file, and that reflex —
// answer the gate rather than the question — is what the FR-158 freshness work
// warns about. Instead `improvementSlack` declares how far above approved an
// entry may drift before it must be re-approved, so ordinary movement from
// unrelated work costs nothing and an accumulated gap has to be looked at.
function metricDrift(current, approved, tolerance, slack) {
  if (approved?.status === "unsupported") {
    return null;
  }
  if (current?.status === "unsupported") {
    return "became unsupported";
  }
  if (typeof current?.percent !== "number" || typeof approved?.percent !== "number") {
    return "missing percentage";
  }
  if (current.percent + tolerance < approved.percent) {
    return `${current.percent}% < ${approved.percent}%`;
  }
  // `slack === null` disables the over-ratchet side entirely, which is different
  // from `0` (every improvement must be re-approved immediately) and different
  // from a huge number. Distinguishing them matters because a baseline written
  // before this rule existed has no `improvementSlack` at all, and defaulting a
  // missing field to `0` would fail every entry in it on the first run — an
  // upgrade that breaks on arrival gets reverted rather than adopted.
  if (slack !== null && current.percent - approved.percent > slack) {
    return (
      `${current.percent}% exceeds the approved ${approved.percent}% by ` +
      `${(current.percent - approved.percent).toFixed(2)} points, over the ${slack}-point ` +
      `improvement slack; re-approve the baseline (see coverage/README.md "Baseline Updates")`
    );
  }
  return null;
}

export function compareSummary(summary, baseline) {
  const failures = [];
  const tolerance = Number(baseline?.policy?.percentageTolerance ?? 0);
  // Absent means "not declared", not "zero". See metricDrift.
  const declaredSlack = baseline?.policy?.improvementSlack;
  const slack = typeof declaredSlack === "number" ? declaredSlack : null;
  for (const [name, approvedCoverage] of Object.entries(baseline?.rust?.components ?? {})) {
    const currentCoverage = summary?.rust?.components?.[name];
    if (!currentCoverage) {
      failures.push(`Rust component missing: ${name}`);
      continue;
    }
    for (const metric of METRICS) {
      const regression = metricDrift(
        currentCoverage[metric],
        approvedCoverage[metric],
        tolerance,
        slack,
      );
      if (regression) {
        failures.push(`Rust ${name} ${metric}: ${regression}`);
      }
    }
  }
  for (const [name, approvedCoverage] of Object.entries(baseline?.rust?.keyModules ?? {})) {
    const currentCoverage = summary?.rust?.keyModules?.[name];
    if (!currentCoverage) {
      failures.push(`Rust key module missing: ${name}`);
      continue;
    }
    for (const metric of METRICS) {
      const regression = metricDrift(
        currentCoverage[metric],
        approvedCoverage[metric],
        tolerance,
        slack,
      );
      if (regression) {
        failures.push(`Rust ${name} ${metric}: ${regression}`);
      }
    }
  }
  for (const metric of METRICS) {
    const regression = metricDrift(
      summary?.frontend?.[metric],
      baseline?.frontend?.[metric],
      tolerance,
      slack,
    );
    if (regression) {
      failures.push(`React ${metric}: ${regression}`);
    }
  }
  const minimumPlaywright = Number(baseline?.playwright?.minimumScenarios ?? 0);
  if ((summary?.playwright?.total ?? 0) < minimumPlaywright) {
    failures.push(
      `Playwright scenarios: ${summary?.playwright?.total ?? 0} < ${minimumPlaywright}`,
    );
  }
  if ((summary?.playwright?.failed ?? 0) > 0) {
    failures.push(`Playwright failures: ${summary.playwright.failed}`);
  }
  return failures;
}

async function main() {
  let parsed;
  try {
    parsed = parseArgs(process.argv.slice(2));
  } catch (error) {
    usage();
    throw error;
  }
  const { command, options } = parsed;
  if (command === "summarize") {
    for (const required of ["rust", "frontend", "playwright", "output"]) {
      if (!options[required]) {
        throw new Error(`--${required} is required`);
      }
    }
    const summary = {
      schemaVersion: 1,
      generatedAt: new Date().toISOString(),
      rust: summarizeRust(
        readJson(options.rust),
        options["repo-root"] ?? process.cwd(),
        options["branch-status"] ?? "supported",
      ),
      frontend: summarizeFrontend(readJson(options.frontend)),
      playwright: summarizePlaywright(readJson(options.playwright)),
    };
    writeJson(options.output, summary);
    console.log(`coverage summary written to ${options.output}`);
    return;
  }
  if (command === "check") {
    if (!options.summary || !options.baseline) {
      throw new Error("--summary and --baseline are required");
    }
    const failures = compareSummary(readJson(options.summary), readJson(options.baseline));
    if (failures.length > 0) {
      console.error("coverage governance failed:");
      for (const failure of failures) {
        console.error(`- ${failure}`);
      }
      process.exitCode = 1;
      return;
    }
    console.log("coverage governance passed");
    return;
  }
  usage();
  throw new Error(`unknown command: ${command ?? "<missing>"}`);
}

if (process.argv[1] && import.meta.url === new URL(`file://${path.resolve(process.argv[1])}`).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
