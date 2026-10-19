// V8-v7 max RSS board, run by quench-node itself:
//
//   quench-node quench-bench/v8-v7-rss.cjs --quench PATH [--qjs PATH] [--node PATH]
//     [--bun PATH] [--runs N] [--only richards,regexp] [--rss-exec PATH] [--out FILE]
//
// Each fixture is the pinned suite's base.js, the fixture and the stock reporting runner.
// Every round runs each engine once, in a fixed interleaved order, through `rss-exec`
// (quench-bench), which evicts the engine's executable from the page cache, runs it, and
// reports the child's peak RSS from wait4. A sample counts only with exit status 0 and a
// finite Score. Quench runs without and with --jit; Node runs --jitless and with its JIT;
// Bun runs with BUN_JSC_useJIT=0 and with its JIT. A fixture is won when every Quench
// sample, in both modes, is below every sample of every other engine.
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const root = path.resolve(__dirname, "..");
const suite = path.join(__dirname, "js-engine-benchmark", "v8-v7");
const FIXTURES = [
  "richards", "deltablue", "crypto", "raytrace",
  "earley-boyer", "regexp", "splay", "navier-stokes",
];
// The stock reporting runner, as run-quench-runtime.mjs appends it.
const RUNNER = `
let __quenchBenchSucceeded = true;
const __quenchBenchPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __quenchBenchPrint(name + ": " + result); },
  NotifyError(name, error) { __quenchBenchSucceeded = false; __quenchBenchPrint(name + ": " + error); },
  NotifyScore(score) {
    if (__quenchBenchSucceeded) {
      __quenchBenchPrint("----");
      __quenchBenchPrint("Score: " + score);
    }
  },
});
`;
const DEFAULT_RUNS = 5;
const DEFAULT_TIMEOUT_MS = 300000;
const RSS_LINE = /^rss-exec maxrss_kib=(\d+) status=(\d+)$/m;
const SCORE_LINE = /^Score:\s*([\d.]+)$/m;

function option(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && index + 1 < process.argv.length ? process.argv[index + 1] : fallback;
}

function usage(message) {
  if (message) console.error(message);
  console.error(
    "usage: quench-node quench-bench/v8-v7-rss.cjs --quench PATH [--qjs PATH] [--node PATH]" +
      " [--bun PATH] [--runs N] [--only a,b] [--rss-exec PATH] [--out FILE]"
  );
  process.exit(2);
}

const quench = option("--quench", null);
if (!quench) usage("--quench is required");
const runs = Number(option("--runs", String(DEFAULT_RUNS)));
if (!Number.isInteger(runs) || runs <= 0) usage("--runs must be a positive integer");
const timeout = Number(option("--timeout-ms", String(DEFAULT_TIMEOUT_MS)));
const rssExec = option("--rss-exec", path.join(root, "target", "release", "rss-exec"));
if (!fs.existsSync(rssExec)) {
  usage(`rss-exec not found at ${rssExec}: cargo build --release -p quench-bench --bin rss-exec`);
}
const selected = option("--only", FIXTURES.join(",")).split(",").filter(Boolean);
for (const fixture of selected) {
  if (!FIXTURES.includes(fixture)) usage(`unknown fixture ${fixture}`);
}

// Engines in the order each round runs them. Quench's two modes come first.
const engines = [
  { name: "quench", side: "quench", executable: quench, argv: [], env: {} },
  { name: "quench --jit", side: "quench", executable: quench, argv: ["--jit"], env: {} },
];
const qjs = option("--qjs", null);
if (qjs) engines.push({ name: "qjs", side: "other", executable: qjs, argv: [], env: {} });
const node = option("--node", null);
if (node) {
  engines.push({ name: "node --jitless", side: "other", executable: node, argv: ["--jitless"], env: {} });
  engines.push({ name: "node", side: "other", executable: node, argv: [], env: {} });
}
const bun = option("--bun", null);
if (bun) {
  engines.push({ name: "bun no-JIT", side: "other", executable: bun, argv: [], env: { BUN_JSC_useJIT: "0" } });
  engines.push({ name: "bun", side: "other", executable: bun, argv: [], env: {} });
}

function sample(engine, script) {
  const env = Object.assign({}, process.env, engine.env);
  const result = spawnSync(
    rssExec,
    ["--evict", engine.executable, "--", engine.executable, ...engine.argv, script],
    { encoding: "utf8", env, timeout, maxBuffer: 64 * 1024 * 1024 }
  );
  const rss = RSS_LINE.exec(result.stderr || "");
  const score = SCORE_LINE.exec(result.stdout || "");
  const valid = result.status === 0 && rss !== null && rss[2] === "0" && score !== null;
  return {
    kib: rss ? Number(rss[1]) : null,
    score: score ? Number(score[1]) : null,
    status: result.status,
    valid,
    stderr: valid ? undefined : (result.stderr || "").slice(-2000),
  };
}

function summary(values) {
  const sorted = values.slice().sort((a, b) => a - b);
  return { min: sorted[0], median: sorted[Math.floor(sorted.length / 2)], max: sorted[sorted.length - 1] };
}

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "v8-v7-rss-"));
const base = fs.readFileSync(path.join(suite, "base.js"), "utf8");
const report = { runs, rssExec, engines: engines.map(({ name, executable, argv }) => ({ name, executable, argv })), fixtures: {} };
let allWon = true;
let allValid = true;
try {
  for (const fixture of selected) {
    const script = path.join(scratch, `${fixture}.js`);
    fs.writeFileSync(
      script,
      base + "\n" + fs.readFileSync(path.join(suite, `${fixture}.js`), "utf8") + "\n" + RUNNER
    );
    const samples = engines.map(() => []);
    for (let round = 0; round < runs; round++) {
      engines.forEach((engine, index) => samples[index].push(sample(engine, script)));
    }
    const rows = engines.map((engine, index) => {
      const valid = samples[index].every((entry) => entry.valid);
      const kib = samples[index].map((entry) => entry.kib).filter((value) => value !== null);
      return { engine: engine.name, side: engine.side, valid, kib, ...summary(kib), samples: samples[index] };
    });
    const valid = rows.every((row) => row.valid);
    const quenchWorst = Math.max(...rows.filter((row) => row.side === "quench").map((row) => row.max));
    const others = rows.filter((row) => row.side === "other");
    const otherBest = others.length ? Math.min(...others.map((row) => row.min)) : Infinity;
    const won = valid && quenchWorst < otherBest;
    allWon = allWon && won;
    allValid = allValid && valid;
    report.fixtures[fixture] = { valid, won, quenchWorst, otherBest, rows };
    console.log(`${fixture}: ${won ? "WIN" : valid ? "LOSS" : "INVALID"} (Quench worst ${quenchWorst} KiB, best other ${otherBest} KiB)`);
    for (const row of rows) {
      console.log(
        `  ${row.engine.padEnd(15)} ${String(row.min).padStart(7)} ${String(row.median).padStart(7)} ${String(row.max).padStart(7)} KiB${row.valid ? "" : "  INVALID"}`
      );
    }
  }
} finally {
  fs.rmSync(scratch, { recursive: true, force: true });
}
report.allValid = allValid;
report.allWon = allWon;
const out = option("--out", null);
if (out) fs.writeFileSync(out, JSON.stringify(report, null, 2) + "\n", { flag: "wx" });
console.log(allWon ? "every fixture: Quench below every other engine" : "not every fixture won");
process.exitCode = allValid ? (allWon ? 0 : 1) : 2;
