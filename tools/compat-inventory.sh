#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output=${1:-"$root/target/compat/inventory.json"}
binary=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}
if [ ! -x "$binary" ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi
mkdir -p "$(dirname -- "$output")"

QUENCH_COMPAT_ROOT="$root" QUENCH_NODE_BIN="$binary" node - "$output" <<'NODE'
const fs = require("fs");
const path = require("path");
const cp = require("child_process");

const root = process.env.QUENCH_COMPAT_ROOT;
const output = process.argv[2];
const sourceRoot = path.join(root, "crates/quench-node/polyfills");
const sourceFiles = [];
function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (entry.name.endsWith(".js")) sourceFiles.push(full);
  }
}
walk(sourceRoot);
const source = sourceFiles.map((file) => fs.readFileSync(file, "utf8")).join("\n");
const builtinModules = JSON.parse(cp.execFileSync(process.execPath, ["-e", "process.stdout.write(JSON.stringify(require('module').builtinModules))"], { encoding: "utf8" }));
const canonicalModules = builtinModules.filter((name) => !name.startsWith("_"));
// Experimental built-ins are not present in every host Node release, but are
// still exercised by the upstream fixture corpus under their feature flags.
const experimentalModules = ["stream/iter"];
const normalizeModule = (name) => name.replace(/^node:/, "");
const probeCode = `const names=${JSON.stringify(canonicalModules)}; const result={}; for (const name of names) { try { require(name); result[name]={status:"available"}; } catch (error) { result[name]={status:"unsupported", code:error?.code || null, message:String(error?.message || error)}; } } console.log(JSON.stringify(result));`;
const quenchProbe = JSON.parse(cp.execFileSync(process.env.QUENCH_NODE_BIN, ["-e", probeCode], { encoding: "utf8" }).trim().split("\n").at(-1));
const quenchBuiltinModules = Object.entries(quenchProbe).filter(([, value]) => value.status === "available").map(([name]) => name);
const moduleCandidates = new Set();
for (const match of source.matchAll(/(?:replace\(\/\^node:\/,[^)]*\)|name)\s*(?:===|==)\s*["']([^"']+)["']/g)) moduleCandidates.add(match[1]);
for (const match of source.matchAll(/name\s*===\s*["']([^"']+)["']/g)) moduleCandidates.add(match[1]);
for (const match of source.matchAll(/String\([^)]*\)\s*(?:===|==)\s*["']([^"']+)["']/g)) moduleCandidates.add(match[1]);
for (const match of source.matchAll(/\[\s*["']([^"']+)["']\s*,/g)) moduleCandidates.add(match[1]);
for (const match of source.matchAll(/["']([a-z0-9_:@/-]+(?:\s+[a-z0-9_:@/-]+)+)["']\.split\(\s*["']\s+["']\s*\)/gi))
  for (const name of match[1].split(/\s+/)) moduleCandidates.add(name);
// Nested dispatch helpers may compare normalized names outside the simple
// patterns above; account for their literal module names as well.
for (const name of canonicalModules) {
  if (source.includes(`"${name}"`) || source.includes(`'${name}'`)) {
    moduleCandidates.add(name);
  }
}
const registeredModules = canonicalModules.filter((name) => moduleCandidates.has(name));
const runtimeMissingModules = canonicalModules.filter((name) => quenchProbe[name]?.status !== "available");
const moduleStatus = Object.fromEntries(canonicalModules.map((name) => [name, registeredModules.includes(name) ? "registered" : "missing"]));
const experimentalStatus = Object.fromEntries(experimentalModules.map((name) => [name, {
  registered: moduleCandidates.has(name),
  quenchAvailable: (() => { try { cp.execFileSync(process.env.QUENCH_NODE_BIN, ["-e", `require(${JSON.stringify(name)})`], { stdio: "ignore" }); return true; } catch (_) { return false; } })(),
  hostAvailable: (() => { try { require(name); return true; } catch (_) { return false; } })()
}]));

const nodeGlobals = JSON.parse(cp.execFileSync(process.execPath, ["-e", "process.stdout.write(JSON.stringify(Object.getOwnPropertyNames(globalThis).sort()))"], { encoding: "utf8" }));
const assignedGlobals = [...source.matchAll(/globalThis\.([A-Za-z_$][A-Za-z0-9_$]*)\s*(?:=|\|\|=|\?\?=)/g)].map((match) => match[1]);
const globalAssignments = [...new Set(assignedGlobals)].sort();
const assignedSet = new Set(globalAssignments);
const sourceGlobalGaps = nodeGlobals.filter((name) => !assignedSet.has(name));

const parallel = path.join(root, "tests/node/test/parallel");
const fixtureFiles = fs.readdirSync(parallel).filter((name) => /\.(?:js|mjs|cjs)$/.test(name)).sort();
const fixturePrefixes = {};
for (const fixture of fixtureFiles) {
  const name = fixture.startsWith("test-") ? fixture.slice(5) : fixture;
  const prefix = name.split("-")[0] || "unprefixed";
  fixturePrefixes[prefix] = (fixturePrefixes[prefix] || 0) + 1;
}
const ownership = JSON.parse(fs.readFileSync(path.join(root, "tools/compat-ownership.json"), "utf8"));
for (const [name, reason] of Object.entries(ownership.platformLimitedModules || {})) {
  if (quenchProbe[name]) quenchProbe[name].classification = "platform-limited", quenchProbe[name].reason = reason;
}
const ownerByPrefix = {};
for (const [owner, prefixes] of Object.entries(ownership.streams)) for (const prefix of prefixes) ownerByPrefix[prefix] = owner;
const fixtureOwnership = Object.fromEntries(Object.entries(fixturePrefixes).sort(([a], [b]) => a.localeCompare(b)).map(([prefix, count]) => [prefix, {
  fixtures: count,
  owner: ownerByPrefix[prefix] || ownership.default.owner,
  status: ownership.platformLimited[prefix] ? "platform-limited" : (ownerByPrefix[prefix] ? "owned" : ownership.default.status),
  reason: ownership.platformLimited[prefix] || ownership.default.reason,
}]));

const report = {
  schema: 1,
  generatedAt: new Date().toISOString(),
  modules: { canonical: canonicalModules, registered: registeredModules, missing: canonicalModules.filter((name) => !registeredModules.includes(name)), experimental: experimentalStatus, runtimeAvailable: quenchBuiltinModules, runtimeMissing: runtimeMissingModules, runtimeStatus: quenchProbe, platformLimited: ownership.platformLimitedModules || {}, status: moduleStatus },
  globals: { node: nodeGlobals, assignedByPolyfills: globalAssignments, sourceGaps: sourceGlobalGaps },
  upstream: { parallelFixtures: fixtureFiles.length, prefixes: fixtureOwnership },
};
fs.writeFileSync(output, JSON.stringify(report, null, 2) + "\n");
console.log(`modules=${canonicalModules.length} registered=${registeredModules.length} static_missing=${report.modules.missing.length} runtime_missing=${runtimeMissingModules.length}`);
console.log(`node_globals=${nodeGlobals.length} polyfill_assignments=${globalAssignments.length}`);
console.log(`upstream_parallel_fixtures=${fixtureFiles.length} prefixes=${Object.keys(fixtureOwnership).length}`);
console.log(`report=${output}`);
NODE
