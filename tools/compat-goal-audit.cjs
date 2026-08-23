#!/usr/bin/env node
"use strict";

// Assemble the evidence needed to decide what to implement next. This is
// deliberately read-only: it reports missing or stale evidence instead of
// silently turning partial verification into a release claim.
const fs = require("fs");
const path = require("path");
const cp = require("child_process");

const root = path.resolve(process.argv[2] || path.join(__dirname, ".."));
const output = process.argv[3] ? path.resolve(process.argv[3]) : null;
const readJson = (file) => {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (_) {
    return null;
  }
};
const exists = (file) => fs.existsSync(file);
const fingerprint = () => {
  try {
    return JSON.parse(cp.execFileSync(process.execPath, [
      path.join(root, "tools/compat-fingerprint.cjs"),
      root,
      path.join(root, "tests/node/test/parallel"),
    ], { cwd: root, encoding: "utf8" }));
  } catch (error) {
    return { error: error.message };
  }
};
const findings = [];
const add = (rank, area, finding, action, evidence) =>
  findings.push({ rank, area, finding, action, evidence });

const tasks = readJson(path.join(root, "tasks/index.json"));
const taskRows = tasks?.tasks || [];
const unfinished = taskRows.filter((task) => task.status !== "complete");
if (unfinished.length) {
  add(
    1,
    "scope",
    `${unfinished.length}/${taskRows.length} task records are unfinished`,
    "Use the task index as the completion checklist; do not close the goal from focused-stage green alone.",
    unfinished.map((task) => `${task.id}:${task.status}`).join(", "),
  );
}

const metrics = path.join(root, "target/compat/focused-stage-metrics.jsonl");
if (!exists(metrics)) {
  add(
    2,
    "verification",
    "No serial focused-stage metrics snapshot exists",
    "Run the focused gate before using stage counts as evidence.",
    "missing focused metrics",
  );
} else {
  const rows = fs.readFileSync(metrics, "utf8").trim().split(/\r?\n/).filter(
    Boolean,
  )
    .map((line) => JSON.parse(line));
  const failures = rows.filter((row) => row.outcome !== "pass");
  const retries = rows.filter((row) => Number(row.attempts) > 1);
  if (failures.length || retries.length) {
    add(
      3,
      "verification",
      `${failures.length} focused failures and ${retries.length} retries are recorded`,
      "Treat retry/failure records as instability data and investigate them before broad claims.",
      path.relative(root, metrics),
    );
  }
}

const inventory = readJson(path.join(root, "target/compat/inventory.json"));
if (!inventory) {
  add(
    4,
    "coverage",
    "The API/module/global inventory is missing",
    "Run tools/compat-inventory.sh before selecting broad surface work.",
    "missing target/compat/inventory.json",
  );
} else {
  const missing = inventory.modules?.missing?.length || 0;
  const sourceGaps = inventory.globals?.sourceGaps?.length || 0;
  if (missing || sourceGaps) {
    add(
      4,
      "coverage",
      `${missing} module registrations and ${sourceGaps} global assignments are not covered by inventory evidence`,
      "Use the inventory gaps to choose the next API cluster, then add focused behavior tests.",
      `modules_missing=${missing};global_source_gaps=${sourceGaps}`,
    );
  }
}

const report = readJson(
  path.join(root, "target/compat/differential-parallel.json"),
);
if (!report) {
  add(
    5,
    "triage",
    "No differential corpus report is available",
    "Run tools/diff-node-quench-parallel.sh on the fixture corpus or a scoped prefix.",
    "missing differential report",
  );
} else {
  const fp = report.fingerprints || {};
  const current = fingerprint();
  const stale = current.error ||
    ["source_digest", "fixture_digest", "focused_digest", "ownership_digest"]
      .some((field) => !fp[field] || fp[field] !== current[field]);
  if (stale) {
    add(
      5,
      "triage",
      "Differential report is stale or its freshness cannot be proven",
      "Regenerate the report before using its queue.",
      current.error || "fingerprint mismatch",
    );
  }
  const nonMatches = (report.results || []).filter((row) =>
    row.category !== "match"
  ).length;
  if (nonMatches) {
    add(
      6,
      "triage",
      `${nonMatches} differential fixture results are non-matches`,
      "Run tools/compat-queue.sh and work the largest owned signature cluster first.",
      path.relative(
        root,
        path.join("target/compat/differential-parallel.json"),
      ),
    );
  }
}

const appStages = [2047, 2069, 2080, 2081, 2104, 2251];
const appMetrics = path.join(root, "target/compat/application-gates.jsonl");
if (!exists(appMetrics)) {
  add(
    7,
    "release",
    "No application-gate result snapshot exists",
    "Run tools/check-application-stages.sh on every compatibility checkpoint.",
    appStages.join(","),
  );
} else {
  const appRows = fs.readFileSync(appMetrics, "utf8").trim().split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  const failures = appRows.filter((row) => row.status !== 0);
  if (failures.length) {
    add(
      7,
      "release",
      `${failures.length} application gates failed in the latest snapshot`,
      "Fix the application regression before claiming release readiness.",
      failures.map((row) => row.stage).join(","),
    );
  }
}

findings.sort((a, b) => a.rank - b.rank);
const result = {
  schema: 1,
  generated_at: new Date().toISOString(),
  root,
  task_counts: {
    total: taskRows.length,
    complete: taskRows.filter((task) => task.status === "complete").length,
    unfinished: unfinished.length,
  },
  findings,
  next_action: findings[0]?.action ||
    "Run the complete local verification gates.",
};
if (output) fs.writeFileSync(output, JSON.stringify(result, null, 2) + "\n");
console.log(JSON.stringify(result, null, 2));
