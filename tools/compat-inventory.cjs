#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const root = path.resolve(__dirname, "..");
const output = process.argv[2] ?? path.join(root, "target/compat/inventory.json");
const sourceRoot = path.join(root, "crates/quench-node/src");
const parallelRoot = path.join(root, "tests/node/test/parallel");

function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(dir, entry.name);
    return entry.isDirectory() ? walk(file) : [file];
  });
}
function readSources() {
  return walk(sourceRoot).filter((file) => /\.(rs|js)$/.test(file))
    .map((file) => fs.readFileSync(file, "utf8")).join("\n");
}
const source = readSources();
const modules = JSON.parse(execFileSync(process.execPath, ["-e", "console.log(JSON.stringify(require('node:module').builtinModules))"], { encoding: "utf8" }));
const registered = new Set([...source.matchAll(/name\s*===\s*[\"']([^\"']+)[\"']/g)].map((m) => m[1]));
const globals = Object.getOwnPropertyNames(globalThis).sort();
const assignments = [...source.matchAll(/globalThis\.([A-Za-z_$][\w$]*)\s*=/g)].map((m) => m[1]);
const fixtures = walk(parallelRoot).filter((file) => file.endsWith(".js"));
const prefixes = [...new Set(fixtures.map((file) => {
  const name = path.basename(file, ".js").replace(/^test-/, "");
  return name.split("-")[0];
}))].sort();
const report = {
  generatedAt: new Date().toISOString(),
  modules: { public: modules, count: modules.length, registered: modules.filter((name) => registered.has(name.replace(/^node:/, ""))), missing: modules.filter((name) => !registered.has(name.replace(/^node:/, ""))) },
  globals: { nodeCount: globals.length, node: globals, polyfillAssignmentCount: assignments.length, polyfillAssignments: [...new Set(assignments)].sort() },
  fixtures: { parallelCount: fixtures.length, prefixCount: prefixes.length, prefixes },
  detector: { registrationPattern: "name === <string>", sourceFiles: walk(sourceRoot).filter((file) => /\.(rs|js)$/.test(file)).length }
};
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
console.log(`Wrote ${path.relative(root, output)}: ${modules.length} modules, ${report.modules.registered.length} registered, ${globals.length} globals, ${fixtures.length} fixtures`);
