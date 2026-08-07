#!/usr/bin/env node

const fs = require("fs");

const input = process.argv[2] || "target/compat/focused-stage-metrics.jsonl";
if (!fs.existsSync(input)) {
  console.error(`metrics file does not exist: ${input}`);
  process.exit(2);
}

const records = fs
  .readFileSync(input, "utf8")
  .split(/\r?\n/)
  .filter(Boolean)
  .map((line, index) => {
    try {
      return JSON.parse(line);
    } catch (error) {
      throw new Error(`invalid metrics record ${index + 1}: ${error.message}`);
    }
  });

if (records.length === 0) {
  console.error(`metrics file is empty: ${input}`);
  process.exit(2);
}

const durations = records
  .map((record) => Number(record.duration_ms))
  .filter(Number.isFinite)
  .sort((a, b) => a - b);
const percentile = (fraction) =>
  durations[
    Math.min(
      durations.length - 1,
      Math.floor((durations.length - 1) * fraction),
    )
  ];
const countBy = (field) =>
  Object.fromEntries(
    [
      ...records.reduce((counts, record) => {
        const value = String(record[field] ?? "unknown");
        counts.set(value, (counts.get(value) || 0) + 1);
        return counts;
      }, new Map()),
    ].sort(([a], [b]) => a.localeCompare(b)),
  );

const retried = records.filter((record) => Number(record.attempts) > 1).length;
const slowestStages = records
  .map((record) => ({
    stage: record.stage,
    duration_ms: Number(record.duration_ms),
    outcome: record.outcome,
    attempts: Number(record.attempts),
    isolation: record.isolation,
  }))
  .filter((record) => Number.isFinite(record.duration_ms))
  .sort((a, b) => b.duration_ms - a.duration_ms)
  .slice(0, 20);
const result = {
  records: records.length,
  outcomes: countBy("outcome"),
  isolation: countBy("isolation"),
  retried_records: retried,
  slowest_stages: slowestStages,
  duration_ms: {
    total: durations.reduce((sum, value) => sum + value, 0),
    min: durations[0],
    p50: percentile(0.5),
    p95: percentile(0.95),
    p99: percentile(0.99),
    max: durations.at(-1),
  },
};

process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
