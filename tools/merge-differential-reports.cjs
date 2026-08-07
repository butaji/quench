#!/usr/bin/env node
const fs = require("fs");

const [output, ...inputs] = process.argv.slice(2);
if (!output || inputs.length === 0) {
  console.error("usage: merge-differential-reports.cjs OUTPUT REPORT...");
  process.exit(2);
}
const reports = inputs.map((file) => JSON.parse(fs.readFileSync(file, "utf8")));
const fingerprints = JSON.stringify(reports[0].fingerprints || {});
const results = [];
const seen = new Set();
for (const report of reports) {
  if (JSON.stringify(report.fingerprints || {}) !== fingerprints) {
    throw new Error("differential report fingerprints do not match");
  }
  for (const result of report.results || []) {
    if (seen.has(result.fixture)) {
      throw new Error(`duplicate fixture: ${result.fixture}`);
    }
    seen.add(result.fixture);
    results.push(result);
  }
}
results.sort((a, b) => a.fixture.localeCompare(b.fixture));
const merged = {
  ...reports[0],
  tool: "merge-differential-reports",
  merged_at: new Date().toISOString(),
  results,
};
fs.writeFileSync(output, `${JSON.stringify(merged)}\n`);
const counts = {};
for (const result of results) {
  counts[result.category] = (counts[result.category] || 0) + 1;
}
console.log(`fixtures=${results.length} output=${output}`);
for (const key of Object.keys(counts).sort()) {
  console.log(`${key}=${counts[key]}`);
}
