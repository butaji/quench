#!/usr/bin/env node
"use strict";

// Audit completed task claims against the repository's per-implementation
// definition of done. This is intentionally read-only and fails closed.
const fs = require("fs");
const path = require("path");

const root = path.resolve(process.argv[2] || path.join(__dirname, ".."));
const indexPath = path.join(root, "tasks", "index.json");
const index = JSON.parse(fs.readFileSync(indexPath, "utf8"));
const required = [
  "related Node API fixture",
  "focused command",
  "upstream Node test command",
  "measured result",
  "remaining failures or unsupported/platform-limited cases",
];
const rows = [];

for (const task of index.tasks || []) {
  if (task.status !== "complete") continue;
  const file = path.join(root, "tasks", task.file);
  const text = fs.readFileSync(file, "utf8");
  const gaps = [];
  if (!/tests\/node-compat\/stage-\d+(?:\/[^`\s)]+)?/.test(text))
    gaps.push(required[0]);
  if (!/`?(?:cargo run[^\n`]*run(?:-compat|)|tools\/check-[^\s`]+|cargo test[^\n`]*)`?/.test(text))
    gaps.push(required[1]);
  if (!/(?:run-parallel|upstream|test-[a-z0-9-]+\.js)/i.test(text))
    gaps.push(required[2]);
  if (!/(?:\bpass(?:es|ed)?\b|\bfailed?\b|\bgreen\b|\bresult\b)/i.test(text))
    gaps.push(required[3]);
  if (!/(?:remain(?:s|ing)|unresolved|unsupported|platform-limited|gap)/i.test(text))
    gaps.push(required[4]);
  rows.push({ id: task.id, file: task.file, status: gaps.length ? "gap" : "evidence-present", gaps });
}

const report = {
  contract: "tasks/index.json validation.implementation_definition_of_done",
  completed: rows.length,
  evidence_present: rows.filter((row) => row.status === "evidence-present").length,
  gaps: rows.filter((row) => row.status === "gap").length,
  tasks: rows,
};
console.log(JSON.stringify(report, null, 2));
process.exitCode = report.gaps ? 1 : 0;
