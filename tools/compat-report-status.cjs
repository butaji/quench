#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const cp = require("child_process");

const [, , root, reportPath, fixturePath] = process.argv;
if (!root || !reportPath || !fixturePath) {
  console.error("usage: compat-report-status.cjs ROOT REPORT FIXTURES");
  process.exit(2);
}

function fail(message) {
  console.error(`invalid compatibility report: ${message}`);
  process.exitCode = 1;
}

function sha256(file) {
  try {
    return require("crypto").createHash("sha256").update(fs.readFileSync(file))
      .digest("hex");
  } catch (error) {
    fail(`cannot hash ${file}: ${error.message}`);
    return null;
  }
}

let report;
try {
  report = JSON.parse(fs.readFileSync(reportPath, "utf8"));
} catch (error) {
  fail(error.message);
  process.exit(1);
}

const expected = new Set();
function collect(target) {
  const stat = fs.statSync(target);
  if (stat.isFile()) {
    if (/\.(?:js|mjs|cjs)$/.test(target)) {
      expected.add(path.relative(root, target));
    }
    return;
  }
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    const full = path.join(target, entry.name);
    if (entry.isDirectory()) collect(full);
    else if (entry.isFile() && /\.(?:js|mjs|cjs)$/.test(full)) {
      expected.add(path.relative(root, full));
    }
  }
}
collect(fixturePath);

if (!Array.isArray(report.results)) fail("results is not an array");
const seen = new Set();
for (const result of report.results || []) {
  if (!result || typeof result.fixture !== "string") {
    fail("result has no fixture");
  }
  if (seen.has(result.fixture)) fail(`duplicate fixture: ${result.fixture}`);
  seen.add(result.fixture);
  if (
    !new Set([
      "match",
      "output-mismatch",
      "node-failed",
      "quench-failed",
      "both-failed",
      "timeout",
    ]).has(result.category)
  ) {
    fail(`unknown category for ${result.fixture}: ${result.category}`);
  }
}
for (const fixture of expected) {
  if (!seen.has(fixture)) fail(`missing fixture: ${fixture}`);
}
for (const fixture of seen) {
  if (!expected.has(fixture)) fail(`fixture is outside selection: ${fixture}`);
}

const comparatorFile = path.join(root, "tools/diff-node-quench.sh");
const nodeRunnerFile = path.join(root, "tools/run-node-fixture.cjs");
if (report.comparator_sha256 !== sha256(comparatorFile)) {
  fail("comparator_sha256 does not match the current comparator");
}
if (report.node_runner_sha256 !== sha256(nodeRunnerFile)) {
  fail("node_runner_sha256 does not match the current Node runner");
}
if (typeof report.parallel_sides !== "boolean") {
  fail("parallel_sides must be boolean");
}
if (
  !report.audit || typeof report.audit.node_environment_limited !== "number"
) {
  fail("audit.node_environment_limited is missing");
} else {
  const environmentLimited = (report.results || []).filter(
    (result) => result.node_environment_limited === true,
  ).length;
  if (report.audit.node_environment_limited !== environmentLimited) {
    fail(
      `audit.node_environment_limited=${report.audit.node_environment_limited} does not match results=${environmentLimited}`,
    );
  }
}

const fingerprint = JSON.parse(cp.execFileSync(
  process.execPath,
  [path.join(root, "tools/compat-fingerprint.cjs"), root, fixturePath],
  { cwd: root, encoding: "utf8" },
));
const checks = {
  schema: report.schema === 2,
  fixture_count: report.results?.length === expected.size,
  fixture_digest:
    report.fingerprints?.fixture_digest === fingerprint.fixture_digest,
  source_digest:
    report.fingerprints?.source_digest === fingerprint.source_digest,
  focused_digest:
    report.fingerprints?.focused_digest === fingerprint.focused_digest,
  ownership_digest:
    report.fingerprints?.ownership_digest === fingerprint.ownership_digest,
  node_version: report.node_version === fingerprint.node_version,
  git_commit: report.git_commit === fingerprint.git_commit,
  comparator_sha256: report.comparator_sha256 === sha256(comparatorFile),
  node_runner_sha256: report.node_runner_sha256 === sha256(nodeRunnerFile),
};
const focusedSummaryPath = path.join(root, "target/compat/focused-latest.txt");
let focusedPolicy = { conflicts: [] };
try {
  focusedPolicy = JSON.parse(fs.readFileSync(
    path.join(root, "tools/focused-compat-policy.json"),
    "utf8",
  ));
} catch (_) {
  focusedPolicy = { conflicts: [] };
}
const allowedFocusedFailures = new Set(
  (focusedPolicy.conflicts || []).flatMap((conflict) => conflict.stages || [])
    .map(String),
);
let focusedSummary = {};
if (fs.existsSync(focusedSummaryPath)) {
  for (
    const line of fs.readFileSync(focusedSummaryPath, "utf8").split(/\r?\n/)
  ) {
    const separator = line.indexOf("=");
    if (separator > 0) {
      focusedSummary[line.slice(0, separator)] = line.slice(separator + 1);
    }
  }
}
const focusedEvidence =
  focusedSummary.focused_digest === fingerprint.focused_digest &&
  (focusedSummary.focused_stage_fail === "0" ||
    (Number(focusedSummary.focused_stage_fail) > 0 &&
      Number(focusedSummary.focused_stage_fail) <=
        allowedFocusedFailures.size &&
      allowedFocusedFailures.has(String(focusedSummary.failed_stages)))) &&
  Number(focusedSummary.focused_stage_total) > 0 &&
  focusedSummary.stage_from === "0" &&
  focusedSummary.stage_to === "2147483647" &&
  focusedSummary.stage_selection === "tests/node-compat/stage-*";
checks.focused_evidence = focusedEvidence;
checks.binary_sha256 =
  report.quench_binary_sha256 === fingerprint.binary_digest;
const stale = Object.entries(checks).filter(([, ok]) => !ok).map(([name]) =>
  name
);
console.log(`report=${path.relative(root, reportPath)}`);
console.log(
  `fixtures=${report.results?.length || 0} expected=${expected.size}`,
);
console.log(`report_finished_at=${report.finished_at || "unknown"}`);
console.log(`report_git_commit=${report.git_commit || "unknown"}`);
console.log(`current_git_commit=${fingerprint.git_commit || "unknown"}`);
console.log(`working_tree_dirty=${fingerprint.working_tree_dirty}`);
console.log(
  `focused_evidence=${focusedEvidence ? "current" : "stale-or-missing"}`,
);
console.log(`freshness=${stale.length ? "stale" : "current"}`);
if (stale.length) {
  console.log(`stale_reasons=${stale.join(",")}`);
  process.exitCode = 1;
}
