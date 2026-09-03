#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const cp = require("node:child_process");
const os = require("node:os");

const root = path.resolve(__dirname, "..");
const benchmarkScript = path.join(root, "quench-bench", "run-quench-runtime.mjs");
const args = process.argv.slice(2);
const DEFAULT_TIMEOUT_MS = 120_000;

const parseArg = (name, fallback = null) => {
  const idx = args.indexOf(name);
  if (idx < 0 || idx + 1 >= args.length) {
    return fallback;
  }
  return args[idx + 1];
};

const binary = parseArg("--binary", path.join(root, "target", "debug", "quench-node"));
const output = parseArg("--out", path.join(root, "target", "bench-perf-index.json"));
const baseInput = parseArg("--base");
const only = parseArg(
  "--only",
  "richards,deltablue,crypto,raytrace,earley-boyer,regexp,splay,navier-stokes"
);
const repeats = Math.max(1, Number.parseInt(parseArg("--repeat", "1"), 10) || 1);
const timeoutMs = Number(parseArg("--timeout-ms", String(DEFAULT_TIMEOUT_MS)));
if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
  throw new Error("--timeout-ms must be a positive number");
}
const processTimeoutMs = Math.max(
  DEFAULT_TIMEOUT_MS,
  timeoutMs * Math.max(1, only.split(",").filter(Boolean).length) * 2 + 5_000,
);

const parseTimeMetric = (stderr, fallback = null) => {
  if (!stderr) {
    return fallback;
  }

  const text = String(stderr);

  const unix = text.match(/\b(\d+)\b\s*maxresident(?:\(kbytes\)|\(KB\)|,)?/i);
  if (unix) {
    return Number(unix[1]) * 1024;
  }

  const bytes = text.match(/Maximum resident set size \(kbytes\):\s*(\d+)/i);
  if (bytes) {
    return Number(bytes[1]) * 1024;
  }

  const linux = text.match(/\b\d+\b(?=\n?$)/);
  if (linux) {
    return Number(linux[0]);
  }

  return fallback;
};

const parseBenchOutput = (json) => {
  const results = json.results || {};
  const suites = Object.entries(results).map(([name, entry]) => {
    const score = Number(entry.Score);
    return {
      name,
      score: Number.isFinite(score) ? score : null,
      raw: entry,
      wall_ms: Number(entry.__time_ms) || 0,
    };
  });

  const valid = suites.filter((entry) => Number.isFinite(entry.score) && entry.wall_ms > 0);
  return {
    suites,
    count: valid.length,
    min_score: valid.length ? Math.min(...valid.map((v) => v.score) ) : 0,
    max_score: valid.length ? Math.max(...valid.map((v) => v.score) ) : 0,
    mean_score: valid.length ? valid.reduce((acc, value) => acc + value.score, 0) / valid.length : 0,
    mean_wall_ms: valid.length ? valid.reduce((acc, value) => acc + value.wall_ms, 0) / valid.length : 0,
    run_payload: json,
  };
};

const parseMetricEnvelope = (payload) => {
  if (!payload || typeof payload !== "object") {
    return null;
  }

  if (payload.metric && typeof payload.metric === "object") {
    return payload.metric;
  }

  if (payload.aggregate && typeof payload.aggregate === "object" && payload.aggregate.metric) {
    return payload.aggregate.metric;
  }

  return null;
};

const runSingle = (runLabel) => {
  const out = path.join(
    os.tmpdir(),
    `quench-bench-${Date.now()}-${Math.random().toString(16).slice(2)}.json`
  );
  const timeArgs = [benchmarkScript, "--binary", binary, "--out", out, "--only", only,
    "--timeout-ms", String(timeoutMs)];

  const isDarwin = process.platform === "darwin";
  const proc = isDarwin
    ? cp.spawnSync(
        "/usr/bin/time",
        ["-l", process.execPath, ...timeArgs],
        {
          cwd: root,
          encoding: "utf8",
          stdio: ["ignore", "pipe", "pipe"],
          timeout: processTimeoutMs,
          killSignal: "SIGKILL",
        }
      )
    : cp.spawnSync("/usr/bin/time", ["-f", "%M", process.execPath, ...timeArgs], {
        cwd: root,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        timeout: processTimeoutMs,
        killSignal: "SIGKILL",
      });

  const stdout = proc.stdout || "";
  const stderr = proc.stderr || "";
  const status = proc.status ?? 1;
  const result = fs.existsSync(out)
    ? JSON.parse(fs.readFileSync(out, "utf8"))
    : { results: {}, error: "missing-output" };

  return {
    status,
    runLabel,
    outJson: out,
    json: result,
    stdout,
    stderr,
    peak_rss_bytes: parseTimeMetric(stderr, null),
    error: status === 0 ? null : `runtime exited ${status}`,
  };
};

const computeIndex = (metric, base) => {
  const ratioParts = [];
  const t = metric.t === 0 ? 1 : metric.t;
  const m = metric.m === 0 || Number.isNaN(metric.m) ? 1 : metric.m;
  const r = metric.r || 1;
  const a = metric.a || 1;
  const b = metric.b || 1;

  const baseT = base.t === 0 ? 1 : base.t;
  const baseM = base.m === 0 || Number.isNaN(base.m) ? 1 : base.m;
  const baseR = base.r || 1;
  const baseA = base.a || 1;
  const baseB = base.b || 1;

  ratioParts.push(baseR / r);
  ratioParts.push(baseM / m);
  ratioParts.push(baseA / a);
  ratioParts.push(baseT / t);
  ratioParts.push(baseB / b);

  return Math.pow(ratioParts.reduce((acc, value) => acc * value, 1), 1 / 5);
};

const runAll = () => {
  const runs = [];
  for (let i = 0; i < repeats; i += 1) {
    const result = runSingle(`run-${i + 1}`);
    if (!result.outJson || !fs.existsSync(result.outJson)) {
      runs.push({
        ...result,
        parsed: { suites: [], count: 0, mean_wall_ms: Number.NaN },
      });
      continue;
    }

    const parsed = parseBenchOutput(result.json);
    runs.push({
      ...result,
      parsed,
    });
  }

  const successful = runs.filter((run) => run.status === 0);
  const summary = {
    timestamp: new Date().toISOString(),
    platform: process.platform,
    binary,
    only,
    repeats,
    runs: successful.length,
    samples: successful,
  };

  const aggregateWall = successful.length
    ? successful.reduce((acc, run) => acc + (run.parsed.mean_wall_ms || 0), 0) / successful.length
    : 0;
  const aggregateRss = successful.length
    ? successful.reduce((acc, run) => {
      const rss = run.peak_rss_bytes || 0;
      return acc + (Number.isFinite(rss) ? rss : 0);
    }, 0) / successful.length
    : 0;

  const aggregateMetric = {
    t: aggregateWall,
    m: aggregateRss,
    r: 1,
    a: 1,
    b: 1,
  };

  const baseMetric = baseInput
    ? parseMetricEnvelope(JSON.parse(fs.readFileSync(baseInput, "utf8")))
    : null;

  const perf = baseMetric ? computeIndex(aggregateMetric, baseMetric) : null;

  const snapshot = {
    ...summary,
    aggregate: {
      wall_ms: aggregateWall,
      peak_rss_bytes: aggregateRss,
      metric: aggregateMetric,
      perfIndexOverBase: perf,
      baseRef: baseInput ? path.resolve(baseInput) : null,
    },
  };

  fs.writeFileSync(output, JSON.stringify(snapshot, null, 2) + "\n", "utf8");
  console.log(JSON.stringify(snapshot, null, 2));
};

runAll();
