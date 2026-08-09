#!/usr/bin/env node
"use strict";

// Turn an existing differential report into a small, auditable next-action
// snapshot. This tool never runs fixtures and never changes compatibility
// state; it deliberately remains useful when the report is stale.
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const cp = require("child_process");

const [
  rootArg,
  reportPathArg,
  ownershipPathArg,
  previousPathArg,
  metricsPathArg,
  outputArg,
] = process.argv.slice(2);
if (!rootArg || !reportPathArg || !ownershipPathArg || !outputArg) {
  console.error(
    "usage: compat-decision-report.cjs ROOT REPORT OWNERSHIP [PREVIOUS] [METRICS] OUTPUT",
  );
  process.exit(2);
}

const root = path.resolve(rootArg);
const reportPath = path.resolve(reportPathArg);
const ownershipPath = path.resolve(ownershipPathArg);
const previousPath = previousPathArg ? path.resolve(previousPathArg) : null;
const metricsPath = metricsPathArg ? path.resolve(metricsPathArg) : null;
const outputPath = path.resolve(outputArg);

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`${label}: ${error.message}`);
  }
}

function sha256(file) {
  return crypto
    .createHash("sha256")
    .update(fs.readFileSync(file))
    .digest("hex");
}

function filesUnder(target) {
  if (!fs.existsSync(target)) return [];
  if (fs.statSync(target).isFile()) return [target];
  const files = [];
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    const full = path.join(target, entry.name);
    if (entry.isDirectory()) files.push(...filesUnder(full));
    else if (entry.isFile() && /\.(?:js|mjs|cjs)$/.test(full)) files.push(full);
  }
  return files.sort();
}

function fingerprint() {
  const fixtureRoot = path.resolve(
    root,
    report.fixture_root || "tests/node/test/parallel",
  );
  try {
    return JSON.parse(
      cp.execFileSync(
        process.execPath,
        [path.join(root, "tools/compat-fingerprint.cjs"), root, fixtureRoot],
        {
          cwd: root,
          encoding: "utf8",
          env: { ...process.env, QUENCH_NODE_BIN: report.quench_binary || "" },
        },
      ),
    );
  } catch (error) {
    return { error: error.message };
  }
}

function counts(results, field) {
  const output = {};
  for (const result of results) {
    const key = String(result[field] ?? "unknown");
    output[key] = (output[key] || 0) + 1;
  }
  return Object.fromEntries(
    Object.entries(output).sort(([a], [b]) => a.localeCompare(b)),
  );
}

function percentile(values, fraction) {
  const sorted = values.filter(Number.isFinite).sort((a, b) => a - b);
  return sorted.length
    ? sorted[
      Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * fraction))
    ]
    : null;
}

function stats(results) {
  const durations = results.map((result) => Number(result.duration_ms));
  return {
    fixtures: results.length,
    total_fixture_ms: durations
      .filter(Number.isFinite)
      .reduce((sum, value) => sum + value, 0),
    p50_fixture_ms: percentile(durations, 0.5),
    p95_fixture_ms: percentile(durations, 0.95),
  };
}

function classifyStatus(owner, fixtureEntry, prefixReason, ownership) {
  if (fixtureEntry || prefixReason) return "platform-limited";
  if (owner === ownership.default.owner) return ownership.default.status;
  return "owned";
}

function classify(result, ownership) {
  const fixtureEntry = Object.entries(
    ownership.platformLimitedFixtures || {},
  ).find(
    ([name]) => result.fixture.endsWith(name) || result.fixture.includes(name),
  );
  const prefixReason = ownership.platformLimited?.[result.prefix];
  const owner =
    Object.entries(ownership.streams || {}).find(([, prefixes]) =>
      prefixes.includes(result.prefix)
    )?.[0] || ownership.default.owner;
  const mappedReason = owner === ownership.default.owner
    ? ownership.default.reason
    : `Owned by workstream ${owner}`;
  return {
    owner,
    status: classifyStatus(owner, fixtureEntry, prefixReason, ownership),
    reason: fixtureEntry?.[1] || prefixReason || mappedReason,
  };
}

function loadMetrics(file) {
  if (!file || !fs.existsSync(file)) return { path: file, available: false };
  const records = fs
    .readFileSync(file, "utf8")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`metrics record ${index + 1}: ${error.message}`);
      }
    });
  const durations = records.map((record) => Number(record.duration_ms));
  return {
    path: path.relative(root, file),
    available: true,
    records: records.length,
    outcomes: counts(records, "outcome"),
    retries: records.filter((record) => Number(record.attempts) > 1).length,
    total_ms: durations
      .filter(Number.isFinite)
      .reduce((sum, value) => sum + value, 0),
    p50_ms: percentile(durations, 0.5),
    p95_ms: percentile(durations, 0.95),
  };
}

const report = readJson(reportPath, "report");
const ownership = readJson(ownershipPath, "ownership");
const previous = previousPath && fs.existsSync(previousPath)
  ? readJson(previousPath, "previous report")
  : null;
const results = Array.isArray(report.results) ? report.results : [];
const currentFingerprint = fingerprint();
const freshnessFields = [
  "source_digest",
  "fixture_digest",
  "focused_digest",
  "ownership_digest",
];
if (report.git_commit) freshnessFields.push("git_commit");
const staleReasons = freshnessFields.filter((field) => {
  const reportValue = field === "git_commit"
    ? report.git_commit
    : report.fingerprints?.[field];
  return reportValue !== currentFingerprint[field];
});
if (
  report.comparator_sha256 !==
    sha256(path.join(root, "tools/diff-node-quench.sh"))
) {
  staleReasons.push("comparator_sha256");
}
if (
  report.node_runner_sha256 !==
    sha256(path.join(root, "tools/run-node-fixture.cjs"))
) {
  staleReasons.push("node_runner_sha256");
}

const grouped = new Map();
for (const result of results) {
  if (result.category === "match") continue;
  const classification = classify(result, ownership);
  const sideError = result.quench?.error || result.node?.error;
  const structuredDetail = sideError
    ? [
      sideError.phase,
      sideError.name,
      sideError.code,
      sideError.callback_index,
      sideError.callback_expected,
      sideError.callback_actual,
      sideError.message,
    ].join("|")
    : "";
  const key = [
    result.prefix,
    result.category,
    result.signature,
    structuredDetail,
    classification.status,
    classification.owner,
  ].join("\u0000");
  const group = grouped.get(key) || {
    prefix: result.prefix,
    category: result.category,
    signature: result.signature,
    structured_error: sideError,
    classification,
    fixtures: [],
    durations: [],
  };
  group.fixtures.push(result.fixture);
  group.durations.push(Number(result.duration_ms));
  grouped.set(key, group);
}
const categoryRank = {
  "quench-failed": 0,
  "output-mismatch": 1,
  timeout: 2,
  "both-failed": 3,
  "node-failed": 4,
};
const queue = [...grouped.values()]
  .map((group) => ({
    prefix: group.prefix,
    category: group.category,
    signature: group.signature,
    structured_error: group.structured_error || null,
    owner: group.classification.owner,
    status: group.classification.status,
    reason: group.classification.reason,
    fixtures: group.fixtures.length,
    representative: group.fixtures[0],
    observed_fixture_ms: group.durations
      .filter(Number.isFinite)
      .reduce((sum, value) => sum + value, 0),
    p50_fixture_ms: percentile(group.durations, 0.5),
  }))
  .sort(
    (a, b) =>
      (a.status === "owned" ? 0 : a.status === "unclassified" ? 1 : 2) -
        (b.status === "owned" ? 0 : b.status === "unclassified" ? 1 : 2) ||
      (categoryRank[a.category] ?? 9) - (categoryRank[b.category] ?? 9) ||
      b.fixtures - a.fixtures ||
      a.signature.localeCompare(b.signature),
  )
  .slice(0, 25);

const currentByFixture = new Map(
  results.map((result) => [result.fixture, result]),
);
const trend = {
  available: Boolean(previous),
  resolved: 0,
  regressions: 0,
  category_delta: {},
};
if (previous && Array.isArray(previous.results)) {
  const previousByFixture = new Map(
    previous.results.map((result) => [result.fixture, result]),
  );
  for (const [fixture, current] of currentByFixture) {
    const before = previousByFixture.get(fixture);
    if (!before) continue;
    if (before.category !== "match" && current.category === "match") {
      trend.resolved++;
    }
    if (before.category === "match" && current.category !== "match") {
      trend.regressions++;
    }
  }
  for (
    const category of new Set([
      ...Object.keys(counts(previous.results, "category")),
      ...Object.keys(counts(results, "category")),
    ])
  ) {
    trend.category_delta[category] =
      (counts(results, "category")[category] || 0) -
      (counts(previous.results, "category")[category] || 0);
  }
}

const metrics = loadMetrics(metricsPath);
const inventoryPath = path.join(root, "target/compat/inventory.json");
let capabilityInventory = null;
if (fs.existsSync(inventoryPath)) {
  try {
    capabilityInventory = readJson(inventoryPath, "capability inventory");
  } catch (_) {
    capabilityInventory = null;
  }
}
const summaryFile = path.join(root, "target/compat/focused-latest.txt");
const summaryText = fs.existsSync(summaryFile)
  ? fs.readFileSync(summaryFile, "utf8")
  : "";
const summaryRecords = Number(
  summaryText.match(/^stage_metrics_records=(\d+)/m)?.[1],
);
const summaryCommit = summaryText.match(/^git_commit=(.+)$/m)?.[1] || null;
const focusedJoin = {
  metrics_available: metrics.available,
  record_count_matches_summary:
    !metrics.available || !Number.isFinite(summaryRecords)
      ? false
      : metrics.records === summaryRecords,
  summary_commit_matches_current: Boolean(summaryCommit) &&
    summaryCommit === currentFingerprint.git_commit,
  report_focused_digest_matches_current:
    report.fingerprints?.focused_digest === currentFingerprint.focused_digest,
  report_commit_matches_current:
    report.git_commit === currentFingerprint.git_commit,
};
focusedJoin.valid = Object.values(focusedJoin).every(Boolean);
const missingData = [];
if (!previous) {
  missingData.push(
    "previous differential report for trend/regression measurement",
  );
}
if (Object.values(ownership.streams || {}).flat().length === 0) {
  missingData.push("ownership mappings");
}
if (
  results.some(
    (result) =>
      !Number.isFinite(Number(result.node_duration_ms)) ||
      !Number.isFinite(Number(result.quench_duration_ms)),
  )
) {
  missingData.push("complete per-side duration telemetry");
}
if (
  results.some(
    (result) =>
      !Number.isFinite(Number(result.fixture_started_ms)) ||
      !Number.isFinite(Number(result.fixture_finished_ms)),
  )
) {
  missingData.push("fixture interval telemetry for concurrency analysis");
}
missingData.push("fixture retry/flake history");
if (
  results.some((result) => {
    const errors = [result.node?.error, result.quench?.error].filter(Boolean);
    return errors.some((error) => !Array.isArray(error.frames));
  })
) {
  missingData.push("structured failure frames");
}
if (
  !capabilityInventory?.modules?.runtimeStatus ||
  !capabilityInventory?.globals?.node
) {
  missingData.push("capability probes");
}
missingData.push(
  "worker-level queue/startup timing for persistent-worker or cache decisions",
);
if (
  metrics.available &&
  Number.isFinite(summaryRecords) &&
  summaryRecords !== metrics.records
) {
  missingData.push(
    "focused metrics/run identity: JSONL records do not match focused-latest.txt",
  );
}
if (!focusedJoin.valid) {
  missingData.push(
    "valid differential/focused join: report, focused summary, and metrics identities do not match",
  );
}

const actionableQueue = focusedJoin.valid ? queue : [];

const output = {
  schema: 1,
  generated_at: new Date().toISOString(),
  report: path.relative(root, reportPath),
  freshness: {
    status: staleReasons.length ? "stale" : "current",
    reasons: staleReasons,
    current_worktree_dirty: currentFingerprint.working_tree_dirty,
  },
  corpus: {
    ...stats(results),
    categories: counts(results, "category"),
    nonmatches: results.filter((result) => result.category !== "match").length,
  },
  differential_telemetry: report.telemetry || {
    available: false,
    reason: "report does not contain run-level throughput telemetry",
  },
  trend,
  queue: actionableQueue,
  focused_join: focusedJoin,
  focused_metrics: metrics,
  missing_decision_data: [...new Set(missingData)],
  next_action: actionableQueue.find((entry) => entry.status === "owned") ||
    actionableQueue.find((entry) => entry.status === "unclassified") ||
    null,
};
fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`);
console.log(`decision_report=${path.relative(root, outputPath)}`);
console.log(
  `freshness=${output.freshness.status}${
    staleReasons.length ? ` reasons=${staleReasons.join(",")}` : ""
  }`,
);
console.log(
  `corpus=${output.corpus.fixtures} matches=${
    output.corpus.categories.match || 0
  } nonmatches=${output.corpus.nonmatches}`,
);
console.log(
  `queue_top=${
    output.next_action
      ? `${output.next_action.prefix}/${output.next_action.category} fixtures=${output.next_action.fixtures} status=${output.next_action.status}`
      : "none"
  }`,
);
console.log(`focused_join=${focusedJoin.valid ? "valid" : "invalid"}`);
console.log(`missing_data=${output.missing_decision_data.length}`);
