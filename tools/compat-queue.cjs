#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");

const args = process.argv.slice(2);
const outputIndex = args.indexOf("--json-out");
const outputPath = outputIndex >= 0 ? args[outputIndex + 1] : undefined;
if (outputIndex >= 0 && !outputPath) throw new Error("--json-out requires a path");
const positional = args.filter((arg, index) => arg !== "--json-out" && index !== outputIndex + 1);
const [reportPath, limitArg = "25", previousPath] = positional;
if (outputIndex >= 0 && !outputPath) throw new Error("--json-out requires a path");
if (!reportPath || reportPath.startsWith("--")) {
  console.error("usage: tools/compat-queue.sh REPORT [LIMIT] [PREVIOUS_REPORT] [--json-out PATH]");
  process.exit(2);
}
const limit = Number.parseInt(limitArg, 10);
if (!Number.isInteger(limit) || limit < 1) throw new Error("limit must be a positive integer");
const load = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const report = load(reportPath);
const rows = Array.isArray(report.fixtures) ? report.fixtures : Array.isArray(report.results) ? report.results : [];
const ownershipPath = path.join(__dirname, "compat-ownership.json");
const ownership = fs.existsSync(ownershipPath) ? load(ownershipPath) : {};
const platformPrefixes = new Set(ownership.platformLimitedPrefixes || ownership.platformPrefixes || []);
const ownedPrefixes = new Map(Object.entries(ownership.prefixOwners || ownership.owners || {}));
const classify = (row) => {
  const fixture = row.fixture || row.file || row.name || "<unknown>";
  const prefix = row.prefix || path.basename(fixture).replace(/^test-/, "").split("-")[0];
  if (row.platformLimited === true || platformPrefixes.has(prefix)) return "platform-limited";
  if (row.owner || ownedPrefixes.has(prefix)) return "owned";
  return "unclassified";
};
const signature = (row) => String(row.signature || row.errorSignature || row.category || row.error || "unknown");
const groups = new Map();
for (const row of rows) {
  if (row.match === true || row.status === "pass" || row.outcome === "match") continue;
  const key = signature(row);
  const group = groups.get(key) || { signature: key, count: 0, fixtures: [], categories: new Set() };
  group.count++;
  const fixture = row.fixture || row.file || row.name || "<unknown>";
  group.fixtures.push(fixture);
  group.categories.add(classify(row));
  groups.set(key, group);
}
const sorted = [...groups.values()].sort((a, b) => b.count - a.count || a.signature.localeCompare(b.signature));
const counts = Object.fromEntries(["owned", "unclassified", "platform-limited"].map((kind) => [kind, rows.filter((row) => classify(row) === kind && row.match !== true && row.status !== "pass").length]));
console.log(`Compatibility queue: ${rows.length} fixtures, ${sorted.length} signatures`);
console.log(`Actionable: owned=${counts.owned} unclassified=${counts.unclassified} platform-limited=${counts["platform-limited"]}`);
for (const group of sorted.slice(0, limit)) {
  const categories = [...group.categories].sort().join(",");
  const reps = [...new Set(group.fixtures)].sort().slice(0, 3).join(", ");
  console.log(`${group.count}\t${categories}\t${group.signature}\t${reps}`);
}
const ranked = sorted.slice(0, limit).map((group, index) => ({
  rank: index + 1,
  signature: group.signature,
  count: group.count,
  categories: [...group.categories].sort(),
  fixtures: [...new Set(group.fixtures)].sort().slice(0, 3),
}));
if (outputPath) {
  fs.mkdirSync(path.dirname(path.resolve(outputPath)), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify({ fixtures: rows.length, signatures: sorted.length, queue: ranked }, null, 2)}\n`);
  console.log(`Wrote ${outputPath}: ${ranked.length} ranked signatures`);
}
if (previousPath) {
  const previous = load(previousPath);
  const currentFixtures = new Set(rows.filter((row) => row.match !== true).map((row) => row.fixture || row.file || row.name));
  const previousRows = Array.isArray(previous.fixtures) ? previous.fixtures : Array.isArray(previous.results) ? previous.results : [];
  const previousFixtures = new Set(previousRows.filter((row) => row.match !== true).map((row) => row.fixture || row.file || row.name));
  const resolved = [...previousFixtures].filter((fixture) => !currentFixtures.has(fixture)).sort();
  const regressions = [...currentFixtures].filter((fixture) => !previousFixtures.has(fixture)).sort();
  console.log(`Resolved: ${resolved.length}${resolved.length ? ` ${resolved.join(", ")}` : ""}`);
  console.log(`Regressions: ${regressions.length}${regressions.length ? ` ${regressions.join(", ")}` : ""}`);
}
