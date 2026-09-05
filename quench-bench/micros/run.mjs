#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { loadCatalog, scenarios, editionIdentity } from "./lib/catalog.mjs";
import { resolveBinary } from "./lib/process.mjs";
import { nextExperiment } from "./lib/diagnostics.mjs";
import { newReport, saveReport, correctness, measureScenario, diagnoseScenario, finalize, assertEdition } from "./lib/study.mjs";

function parseArgs(args) {
  const positional = [], flags = {};
  const booleans = new Set(["include-legacy", "idle-confirmed", "lifecycle", "reserved"]);
  const values = new Set(["engine", "bun", "oracle", "out", "size", "limit", "pairs", "warmup-ms", "window-ms",
    "timeout-ms", "instrument", "site", "trace-engine", "report", "edition", "variant", "epoch-calls"]);
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (!arg.startsWith("--")) { positional.push(arg); continue; }
    const key = arg.slice(2);
    if (booleans.has(key)) flags[key] = true;
    else if (values.has(key) && args[i + 1] && !args[i + 1].startsWith("--")) flags[key] = args[++i];
    else throw new Error(`unknown option or missing value: ${arg}`);
  }
  return { command: positional[0] || "help", selection: positional[1], flags };
}

function integer(flags, name, fallback, min = 1) {
  const value = Number(flags[name] ?? fallback);
  if (!Number.isSafeInteger(value) || value < min) throw new Error(`--${name} must be an integer >= ${min}`);
  return value;
}

function makeOptions(flags, config, qualification) {
  const p = config.protocol;
  const options = { ...p, timeoutMs: integer(flags, "timeout-ms", qualification || flags.lifecycle ? 600000 : 30000),
    lifecycle: qualification || !!flags.lifecycle, instrument: flags.instrument || "counters", site: flags.site,
    traceEngine: flags["trace-engine"] ? resolveBinary(flags["trace-engine"]) : null };
  if (!qualification) Object.assign(options, { timingPairs: integer(flags, "pairs", 3, 2), memoryPairs: integer(flags, "pairs", 3, 2),
    warmupMs: integer(flags, "warmup-ms", 100, 0), windowMs: integer(flags, "window-ms", 50), windows: 3,
    epochCalls: integer(flags, "epoch-calls", p.epochCalls) });
  if (!["counters", "sites", "events"].includes(options.instrument)) throw new Error("invalid instrumentation level");
  return options;
}

function selection(catalog, requested, includeLegacy) {
  const all = [...catalog.cases, ...catalog.legacy];
  if (!requested || requested === "all") return includeLegacy ? all : catalog.cases;
  const result = all.filter((c) => c.id === requested || c.alias === requested);
  if (!result.length) throw new Error(`unknown experiment: ${requested}`);
  return result;
}

function qualificationChecks(command, flags, catalog, requested) {
  if (command !== "qualify") return;
  if (!flags["idle-confirmed"]) throw new Error("Qualification requires --idle-confirmed on an otherwise idle host. The harness will not stop unrelated work.");
  if (process.platform !== "darwin" || process.arch !== "arm64") throw new Error("Edition 1 qualification targets ARM64 macOS.");
  const overridden = ["variant", "size", "pairs", "warmup-ms", "window-ms", "epoch-calls"].some((key) => flags[key] !== undefined);
  if (requested || overridden) throw new Error("Qualification protocol and coverage cannot be overridden; use measure instead.");
  assertEdition(catalog, flags.edition || catalog.config.edition);
}

function help() {
  console.log(`Architecture-neutral VM microbench experiments
Usage: node quench-bench/micros/run.mjs <command> [experiment] [options]
Commands: list, smoke, measure, diagnose, next, qualify, identity
--engine PATH (default target/production/quench-node), --bun PATH, --oracle PATH
--size small|medium|large|all, --variant NAME, --include-legacy, --limit N, --reserved
--pairs N, --warmup-ms N, --window-ms N, --timeout-ms N, --out FILE.json
--instrument counters|sites|events --trace-engine PATH [--site CODE:PC]
--lifecycle [--epoch-calls N]; next --report FILE.json
qualify --edition 1 --idle-confirmed (hours; --limit records incomplete qualification)
No command edits, builds, or optimizes a tested engine. See README.md.`);
}

function inspection(command, flags, catalog) {
  if (command === "help") { help(); return true; }
  if (command === "identity") { console.log(JSON.stringify(editionIdentity(catalog), null, 2)); return true; }
  if (command === "list") {
    console.log(JSON.stringify(catalog.cases.map(({ id, question, variants, requires, memory }) => ({ id, question, variants, requires, memory: !!memory })), null, 2));
    return true;
  }
  if (command !== "next") return false;
  if (!flags.report) throw new Error("next requires --report");
  console.log(JSON.stringify(nextExperiment(JSON.parse(fs.readFileSync(flags.report)), catalog.cases), null, 2));
  return true;
}

async function execute(command, selected, tasks, catalog, flags, options, qualification) {
  const binaries = resolveEngines(flags);
  const report = newReport(command, catalog, binaries, options);
  const out = path.resolve(flags.out || `target/micros/${Date.now()}-${process.pid}/report.json`);
  if (!out.endsWith(".json") || fs.existsSync(out)) throw new Error("--out must be a new .json artifact path; prior attempts are preserved");
  report.reproduction = [process.execPath, ...process.argv.slice(1)];
  report.selectedExperiments = selected.map((c) => c.id);
  report.totalScenarios = tasks.length;
  report.limit = integer(flags, "limit", tasks.length);
  const byId = new Map(selected.map((c) => [c.id, c]));
  for (const scenario of tasks.slice(0, report.limit)) {
    const c = byId.get(scenario.experiment);
    const verify = await correctness(c, scenario, binaries, options);
    const row = await executeRow(command, c, scenario, binaries, options, verify, report);
    report.results.push(row);
    saveReport(report, out, catalog);
    console.error(`${report.results.length}/${tasks.length} ${scenario.id}: ${row.correctness}`);
  }
  finalize(report, catalog, tasks.length, qualification);
  saveReport(report, out, catalog);
  console.log(JSON.stringify({ report: out, complete: report.complete, scenarios: report.results.length,
    correctnessFailures: report.results.filter((r) => r.correctness !== "pass").length, qualification: report.qualification, next: report.next }, null, 2));
  process.exitCode = exitStatus(report, qualification);
}

function resolveEngines(flags) {
  return { oracle: resolveBinary(flags.oracle || process.execPath), comparator: resolveBinary(flags.bun || "bun"), candidate: resolveBinary(flags.engine || "target/production/quench-node") };
}

function exitStatus(report, qualification) { return report.results.some((r) => r.correctness !== "pass") || (qualification && report.qualification !== "pass") ? 1 : 0; }

function executeRow(command, c, scenario, binaries, options, verify, report) {
  if (command === "measure" || command === "qualify") return measureScenario(c, scenario, binaries, options, verify);
  if (command === "diagnose") return diagnoseScenario(c, scenario, binaries, options, verify, report);
  return { scenario, ...verify };
}

async function main() {
  const { command, selection: requested, flags } = parseArgs(process.argv.slice(2));
  const catalog = loadCatalog();
  if (inspection(command, flags, catalog)) return;
  if (!["smoke", "measure", "diagnose", "qualify"].includes(command)) throw new Error(`unknown command: ${command}`);
  const qualification = command === "qualify";
  qualificationChecks(command, flags, catalog, requested);
  const selected = selection(catalog, requested, qualification || flags["include-legacy"]);
  const tasks = selectScenarios(selected, catalog.config, flags, qualification);
  await execute(command, selected, tasks, catalog, flags, makeOptions(flags, catalog.config, qualification), qualification);
}

function selectScenarios(selected, config, flags, qualification) {
  const size = flags.size || "small";
  if (size !== "all" && !Object.hasOwn(config.sizes, size)) throw new Error("invalid size");
  let tasks = qualification || flags.reserved ? scenarios(selected, config, true)
    : (size === "all" ? Object.keys(config.sizes) : [size]).flatMap((s) => scenarios(selected, config, false, s));
  tasks = filterReserved(tasks, flags, qualification, size);
  if (flags.variant) tasks = tasks.filter((s) => s.variant === flags.variant);
  if (!tasks.length) throw new Error("selection contains no scenarios");
  return tasks;
}

function filterReserved(tasks, flags, qualification, size) {
  return flags.reserved && !qualification && size !== "all" ? tasks.filter((s) => s.size === size || s.legacy) : tasks;
}

main().catch((error) => { console.error(error.stack || error.message); process.exitCode = 2; });
