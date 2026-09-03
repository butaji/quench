#!/usr/bin/env node
// Gradual Deegen-mechanism validation runner. See docs/deegen-micro-curriculum.md
// for the design and manifest.json for the case list. Unlike quench-bench/micros/
// verify.mjs (a neutral perf-regression corpus), this checks two things per case:
//   1. Observable output matches the Node oracle exactly (same as micros/).
//   2. Where the case declares an `expect.instrumentation` clause, the engine's
//      execution_trace JSON snapshot (QUENCH_EXEC_TRACE=1) satisfies it — proving
//      the mechanism under test actually engaged, not just that the program
//      happened to compute the right answer via a slower fallback path.
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(ROOT, "manifest.json"), "utf8"));
const CASE_COUNT = manifest.count;
const metadataById = new Map(manifest.cases.map((item) => [item.id, item]));

function checkCorpus() {
  const expected = Array.from({ length: CASE_COUNT }, (_, index) => `${String(index + 1).padStart(3, "0")}.js`);
  const actual = readdirSync(ROOT).filter((name) => /^\d{3}\.js$/.test(name)).sort();
  const errors = [];
  if (actual.length !== expected.length) errors.push(`expected ${expected.length} numbered scripts, found ${actual.length}`);
  for (let index = 0; index < expected.length; index++) {
    if (actual[index] !== expected[index]) errors.push(`missing or unexpected script: ${expected[index]}`);
  }
  return errors;
}

const arg = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};
const engine = arg("--engine", process.execPath);
const oracle = arg("--oracle", process.execPath);
const timeout = Number(arg("--timeout-ms", "10000"));
const first = Math.max(1, Number(arg("--from", "1")));
const last = Math.min(CASE_COUNT, Number(arg("--to", String(CASE_COUNT))));
const outputFile = arg("--out", null);

const corpusErrors = checkCorpus();
if (corpusErrors.length) {
  console.error(corpusErrors.join("\n"));
  process.exit(1);
}

function runPlain(program, name) {
  const result = spawnSync(program, [join(ROOT, name)], { encoding: "utf8", timeout, killSignal: "SIGKILL" });
  return { status: result.status, timedOut: result.error?.code === "ETIMEDOUT", stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

function runTraced(program, name) {
  const result = spawnSync(program, [join(ROOT, name)], {
    encoding: "utf8",
    timeout,
    killSignal: "SIGKILL",
    env: { ...process.env, QUENCH_EXEC_TRACE: "1" },
  });
  const stderr = result.stderr ?? "";
  const marker = "QUENCH_EXEC_TRACE ";
  const line = stderr.split("\n").find((l) => l.startsWith(marker));
  let snapshot = null;
  let parseError = null;
  if (line) {
    try {
      snapshot = JSON.parse(line.slice(marker.length));
    } catch (error) {
      parseError = String(error);
    }
  }
  return { status: result.status, timedOut: result.error?.code === "ETIMEDOUT", snapshot, parseError, hadLine: Boolean(line) };
}

function getPath(obj, path) {
  return path.split(".").reduce((node, key) => (node && typeof node === "object" && key in node ? node[key] : undefined), obj);
}

function parseAssert(assertion) {
  const match = /^(>=|<=|==|!=|>|<)\s*(-?\d+(?:\.\d+)?)$/.exec(assertion.trim());
  if (!match) throw new Error(`invalid assert expression: ${assertion}`);
  return { op: match[1], value: Number(match[2]) };
}

function compare(actual, assertion) {
  const { op, value } = parseAssert(assertion);
  if (typeof actual !== "number" || Number.isNaN(actual)) return false;
  switch (op) {
    case ">=": return actual >= value;
    case "<=": return actual <= value;
    case "==": return actual === value;
    case "!=": return actual !== value;
    case ">": return actual > value;
    case "<": return actual < value;
    default: return false;
  }
}

function sumMapField(map, field) {
  if (!map || typeof map !== "object") return 0;
  return Object.values(map).reduce((sum, entry) => sum + (Number(entry?.[field]) || 0), 0);
}

function sumArrayField(array, field) {
  if (!Array.isArray(array)) return 0;
  return array.reduce((sum, entry) => sum + (Number(entry?.[field]) || 0), 0);
}

// Returns { ok: boolean, detail: string } for one instrumentation clause.
function evalClause(snapshot, clause) {
  switch (clause.kind) {
    case "none":
      return { ok: true, detail: `skipped (${clause.reason ?? "no instrumentation declared"})`, skipped: true };
    case "path": {
      const actual = getPath(snapshot, clause.path);
      const ok = compare(actual, clause.assert);
      return { ok, detail: `${clause.path} = ${actual} ${clause.assert} -> ${ok}` };
    }
    case "map_sum": {
      const actual = sumMapField(getPath(snapshot, clause.path), clause.field);
      const ok = compare(actual, clause.assert);
      return { ok, detail: `sum(${clause.path}[*].${clause.field}) = ${actual} ${clause.assert} -> ${ok}` };
    }
    case "array_field_sum": {
      const actual = sumArrayField(getPath(snapshot, clause.path), clause.field);
      const ok = compare(actual, clause.assert);
      return { ok, detail: `sum(${clause.path}[].${clause.field}) = ${actual} ${clause.assert} -> ${ok}` };
    }
    case "all": {
      const results = clause.clauses.map((sub) => evalClause(snapshot, sub));
      return { ok: results.every((r) => r.ok), detail: results.map((r) => r.detail).join(" && ") };
    }
    default:
      return { ok: false, detail: `unknown clause kind: ${clause.kind}` };
  }
}

const results = [];
let failures = 0;
let instrumentationSkipped = 0;

for (let id = first; id <= last; id++) {
  const name = `${String(id).padStart(3, "0")}.js`;
  const meta = metadataById.get(String(id).padStart(3, "0"));
  const oracleRun = runPlain(oracle, name);
  const engineRun = runPlain(engine, name);
  const observableOk =
    oracleRun.status === 0 &&
    engineRun.status === oracleRun.status &&
    !oracleRun.timedOut &&
    !engineRun.timedOut &&
    engineRun.stdout === oracleRun.stdout;

  let instrumentation = { ok: true, detail: "not evaluated", skipped: true };
  if (observableOk && meta?.expect?.instrumentation && meta.expect.instrumentation.kind !== "none") {
    const traced = runTraced(engine, name);
    if (!traced.hadLine) {
      instrumentation = { ok: false, detail: "engine did not emit a QUENCH_EXEC_TRACE line (was it built with --features execution-trace?)" };
    } else if (traced.parseError) {
      instrumentation = { ok: false, detail: `failed to parse QUENCH_EXEC_TRACE JSON: ${traced.parseError}` };
    } else {
      instrumentation = evalClause(traced.snapshot, meta.expect.instrumentation);
    }
  } else if (meta?.expect?.instrumentation?.kind === "none") {
    instrumentation = { ok: true, detail: meta.expect.instrumentation.reason ?? "correctness-only case", skipped: true };
    instrumentationSkipped++;
  }

  const ok = observableOk && instrumentation.ok;
  results.push({
    id: String(id).padStart(3, "0"),
    title: meta?.title ?? null,
    stage: meta?.stage ?? null,
    mechanism: meta?.mechanism ?? null,
    observable_ok: observableOk,
    instrumentation_ok: instrumentation.ok,
    instrumentation_skipped: Boolean(instrumentation.skipped),
    instrumentation_detail: instrumentation.detail,
    ok,
  });

  if (!ok) {
    failures++;
    console.error(`${name} [${meta?.stage ?? "?"}]: FAIL`);
    if (!observableOk) {
      console.error(`  observable mismatch (oracle status=${oracleRun.status}, engine status=${engineRun.status})`);
      if (oracleRun.stdout !== engineRun.stdout) {
        console.error(`  oracle stdout: ${JSON.stringify(oracleRun.stdout)}`);
        console.error(`  engine stdout: ${JSON.stringify(engineRun.stdout)}`);
      }
      if (engineRun.stderr) console.error(`  engine stderr: ${engineRun.stderr.trim()}`);
    }
    if (!instrumentation.ok) {
      console.error(`  instrumentation: ${instrumentation.detail}`);
    }
  } else {
    const tag = instrumentation.skipped ? "correctness-only" : "verified";
    console.log(`${name} [${meta?.stage ?? "?"}]: ok (${tag})`);
  }
}

const summary = {
  from: first,
  to: last,
  total: results.length,
  passed: results.filter((r) => r.ok).length,
  failed: failures,
  instrumentation_skipped: instrumentationSkipped,
  instrumentation_gaps_note: manifest.instrumentation_gaps,
  cases: results,
};

console.log(`\n${summary.passed}/${summary.total} passed (${instrumentationSkipped} correctness-only, no instrumentation check declared)`);
if (outputFile) {
  const fs = await import("node:fs");
  fs.writeFileSync(outputFile, JSON.stringify(summary, null, 2));
}
process.exit(failures ? 1 : 0);
