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
const runs = Number(arg("--runs", "3"));
// Default ceilings express "best possible performance and memory profile,"
// per this suite's purpose: 3x wall time and 1.5x RSS vs. Node. RSS already
// runs ~0.5x Node in practice (quench's representation is genuinely lighter),
// so 1.5x is a real regression bar, not a courtesy margin. Wall time above 3x
// is a real finding worth reporting, not noise to average away — see the
// docs/deegen-micro-curriculum.md findings log for cases currently failing
// this bar (recursion, megamorphic property access, string-heavy workloads)
// and the 026.js for-in case, which is a confirmed correctness-relevant perf
// cliff, not just a slow path.
const wallCeiling = Number(arg("--wall-ratio-max", "3"));
const rssCeiling = Number(arg("--rss-ratio-max", "1.5"));

const timeBinary = existsSync("/usr/bin/time") ? "/usr/bin/time" : null;
const timeFlags = process.platform === "darwin" ? ["-l"] : ["-v"];

function peakRssBytes(stderr) {
  const mac = stderr.match(/([0-9]+)\s+maximum resident set size/i);
  if (mac) return Number(mac[1]);
  const macFootprint = stderr.match(/([0-9]+)\s+peak memory footprint/i);
  if (macFootprint) return Number(macFootprint[1]);
  const linux = stderr.match(/maximum resident set size[^:]*:\s*([0-9]+)/i);
  if (linux) return Number(linux[1]) * 1024;
  return null;
}

function median(values) {
  const ordered = values.filter(Number.isFinite).sort((a, b) => a - b);
  return ordered.length ? ordered[Math.floor(ordered.length / 2)] : null;
}

function geometricMean(values) {
  const usable = values.filter((value) => Number.isFinite(value) && value > 0);
  return usable.length ? Math.exp(usable.reduce((sum, value) => sum + Math.log(value), 0) / usable.length) : null;
}

function runTimed(program, name) {
  const args = [join(ROOT, name)];
  const command = timeBinary ? timeBinary : program;
  const commandArgs = timeBinary ? [...timeFlags, program, ...args] : args;
  const started = process.hrtime.bigint();
  const result = spawnSync(command, commandArgs, { encoding: "utf8", timeout, killSignal: "SIGKILL" });
  return {
    status: result.status,
    timedOut: result.error?.code === "ETIMEDOUT",
    stdout: result.stdout ?? "",
    wallNs: Number(process.hrtime.bigint() - started),
    peakRssBytes: timeBinary ? peakRssBytes(result.stderr ?? "") : null,
  };
}

function measurePerf(program, name) {
  const samples = Array.from({ length: Math.max(1, runs) }, () => runTimed(program, name));
  return {
    ok: samples.every((s) => s.status === 0 && !s.timedOut),
    wallNs: median(samples.map((s) => s.wallNs)),
    peakRssBytes: median(samples.map((s) => s.peakRssBytes)),
  };
}

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

function keyedField(map, key, field) {
  if (!map || typeof map !== "object") return 0;
  return Number(map[key]?.[field]) || 0;
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
    case "map_key_field": {
      const actual = keyedField(getPath(snapshot, clause.path), clause.key, clause.field);
      const ok = compare(actual, clause.assert);
      return { ok, detail: `${clause.path}.${clause.key}.${clause.field} = ${actual} ${clause.assert} -> ${ok}` };
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

  let perf = { ok: true, detail: "not measured", wallRatio: null, rssRatio: null };
  if (observableOk) {
    const oraclePerf = measurePerf(oracle, name);
    const enginePerf = measurePerf(engine, name);
    const wallRatio = oraclePerf.wallNs && enginePerf.wallNs ? enginePerf.wallNs / oraclePerf.wallNs : null;
    const rssRatio = oraclePerf.peakRssBytes && enginePerf.peakRssBytes ? enginePerf.peakRssBytes / oraclePerf.peakRssBytes : null;
    const wallOk = wallRatio === null || wallRatio <= wallCeiling;
    const rssOk = rssRatio === null || rssRatio <= rssCeiling;
    perf = {
      ok: enginePerf.ok && wallOk && rssOk,
      wallRatio,
      rssRatio,
      wallNs: enginePerf.wallNs,
      oracleWallNs: oraclePerf.wallNs,
      peakRssBytes: enginePerf.peakRssBytes,
      oraclePeakRssBytes: oraclePerf.peakRssBytes,
      detail: !enginePerf.ok
        ? "engine timed out or crashed during perf measurement"
        : `wall ${wallRatio === null ? "n/a" : wallRatio.toFixed(2)}x, rss ${rssRatio === null ? "n/a" : rssRatio.toFixed(2)}x vs oracle (ceiling ${wallCeiling}x/${rssCeiling}x)`,
    };
  }

  const ok = observableOk && instrumentation.ok && perf.ok;
  results.push({
    id: String(id).padStart(3, "0"),
    title: meta?.title ?? null,
    stage: meta?.stage ?? null,
    mechanism: meta?.mechanism ?? null,
    observable_ok: observableOk,
    instrumentation_ok: instrumentation.ok,
    instrumentation_skipped: Boolean(instrumentation.skipped),
    instrumentation_detail: instrumentation.detail,
    perf_ok: perf.ok,
    perf_detail: perf.detail,
    wall_ratio: perf.wallRatio,
    rss_ratio: perf.rssRatio,
    engine_wall_ns: perf.wallNs,
    oracle_wall_ns: perf.oracleWallNs,
    engine_peak_rss_bytes: perf.peakRssBytes,
    oracle_peak_rss_bytes: perf.oraclePeakRssBytes,
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
    if (!perf.ok) {
      console.error(`  perf: ${perf.detail}`);
    }
  } else {
    const tag = instrumentation.skipped ? "correctness-only" : "verified";
    console.log(`${name} [${meta?.stage ?? "?"}]: ok (${tag}, ${perf.detail})`);
  }
}

const passedCases = results.filter((r) => r.observable_ok);
const speedScore = (() => {
  const ratios = passedCases.map((r) => r.wall_ratio).filter((v) => Number.isFinite(v) && v > 0).map((v) => 1 / v);
  const mean = geometricMean(ratios);
  return mean === null ? null : 100 * mean;
})();
const memoryScore = (() => {
  const ratios = passedCases.map((r) => r.rss_ratio).filter((v) => Number.isFinite(v) && v > 0).map((v) => 1 / v);
  const mean = geometricMean(ratios);
  return mean === null ? null : 100 * mean;
})();

const summary = {
  from: first,
  to: last,
  total: results.length,
  passed: results.filter((r) => r.ok).length,
  failed: failures,
  instrumentation_skipped: instrumentationSkipped,
  instrumentation_gaps_note: manifest.instrumentation_gaps,
  // 100 = engine matches oracle wall-time/RSS exactly; >100 = engine faster/lighter than Node; <100 = slower/heavier.
  // This is descriptive evidence for this suite's cases, not the project's regression-gate score (that's micros/).
  speed_score_vs_oracle: speedScore,
  memory_score_vs_oracle: memoryScore,
  wall_ratio_ceiling: wallCeiling,
  rss_ratio_ceiling: rssCeiling,
  cases: results,
};

console.log(`\n${summary.passed}/${summary.total} passed (${instrumentationSkipped} correctness-only, no instrumentation check declared)`);
console.log(`speed score vs oracle: ${speedScore === null ? "n/a" : speedScore.toFixed(1)} (100 = parity with Node, higher = faster)`);
console.log(`memory score vs oracle: ${memoryScore === null ? "n/a" : memoryScore.toFixed(1)} (100 = parity with Node, higher = lighter)`);
const worstWall = passedCases.filter((r) => Number.isFinite(r.wall_ratio)).sort((a, b) => b.wall_ratio - a.wall_ratio).slice(0, 5);
if (worstWall.length) {
  console.log("slowest cases vs oracle:");
  for (const r of worstWall) console.log(`  ${r.id} [${r.stage}]: ${r.wall_ratio.toFixed(2)}x wall time`);
}
if (outputFile) {
  const fs = await import("node:fs");
  fs.writeFileSync(outputFile, JSON.stringify(summary, null, 2));
}
process.exit(failures ? 1 : 0);
