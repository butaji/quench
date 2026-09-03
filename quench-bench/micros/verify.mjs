#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(ROOT, "manifest.json"), "utf8"));
const CASE_COUNT = manifest.count;
const families = manifest.families;
const metadataById = new Map(manifest.cases.map((item) => [item.id, item]));

function checkCorpus() {
  const expected = Array.from({ length: CASE_COUNT }, (_, index) => `${String(index + 1).padStart(3, "0")}.js`);
  const actual = readdirSync(ROOT).filter((name) => /^\d{3}\.js$/.test(name)).sort();
  const errors = [];
  if (actual.length !== expected.length) errors.push(`expected ${expected.length} numbered scripts, found ${actual.length}`);
  for (let index = 0; index < expected.length; index++) {
    if (actual[index] !== expected[index]) errors.push(`missing or unexpected script: ${expected[index]}`);
  }
  const manifestFiles = manifest.cases.map(({ file }) => file).sort();
  if (manifestFiles.length !== expected.length || manifestFiles.some((name, index) => name !== expected[index])) {
    errors.push("manifest case files do not match the numbered corpus");
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
const runs = Number(arg("--runs", "1"));
const outputFile = arg("--out", null);
const first = Math.max(1, Number(arg("--from", "1")));
const last = Math.min(CASE_COUNT, Number(arg("--to", String(CASE_COUNT))));

const corpusErrors = checkCorpus();
if (corpusErrors.length) {
  console.error(corpusErrors.join("\n"));
  process.exit(1);
}
if (!Number.isInteger(first) || !Number.isInteger(last) || first > last) throw new Error("invalid --from/--to range");
if (!Number.isInteger(runs) || runs < 1) throw new Error("invalid --runs");
if (!Number.isFinite(timeout) || timeout <= 0) throw new Error("invalid --timeout-ms");

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

function metric(stderr, label) {
  const escaped = label.replace(/[.*+?^${}()|[\\]\\\\]/g, "\\\\$&");
  const before = stderr.match(new RegExp(`([0-9]+)\\s+${escaped}`, "i"));
  if (before) return Number(before[1]);
  const after = stderr.match(new RegExp(`${escaped}[^0-9]*([0-9]+)`, "i"));
  return after ? Number(after[1]) : null;
}

function median(values) {
  const ordered = values.filter(Number.isFinite).sort((a, b) => a - b);
  return ordered.length ? ordered[Math.floor(ordered.length / 2)] : null;
}

function geometricMean(values) {
  const usable = values.filter((value) => Number.isFinite(value) && value > 0);
  return usable.length ? Math.exp(usable.reduce((sum, value) => sum + Math.log(value), 0) / usable.length) : null;
}

const run = (program, name) => {
  const args = [join(ROOT, name)];
  const command = timeBinary ? timeBinary : program;
  const commandArgs = timeBinary ? [...timeFlags, program, ...args] : args;
  const started = process.hrtime.bigint();
  const result = spawnSync(command, commandArgs, { encoding: "utf8", timeout, killSignal: "SIGKILL" });
  return {
    status: result.status,
    timedOut: result.error?.code === "ETIMEDOUT",
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    wallNs: Number(process.hrtime.bigint() - started),
    peakRssBytes: timeBinary ? peakRssBytes(result.stderr ?? "") : null,
    instructions: timeBinary ? metric(result.stderr ?? "", "instructions retired") : null,
    cycles: timeBinary ? metric(result.stderr ?? "", "cycles elapsed") : null,
    pageFaults: timeBinary ? metric(result.stderr ?? "", "page faults") : null,
    pageReclaims: timeBinary ? metric(result.stderr ?? "", "page reclaims") : null,
  };
};

const cases = [];
let failures = 0;
for (let id = first; id <= last; id++) {
  const name = `${String(id).padStart(3, "0")}.js`;
  const oracleRuns = Array.from({ length: runs }, () => run(oracle, name));
  const engineRuns = Array.from({ length: runs }, () => run(engine, name));
  const expected = oracleRuns.at(-1);
  const result = engineRuns.at(-1);
  const ok = expected.status === 0 && result.status === expected.status && !expected.timedOut && !result.timedOut && result.stdout === expected.stdout;
  const oracleWall = median(oracleRuns.map((sample) => sample.wallNs));
  const engineWall = median(engineRuns.map((sample) => sample.wallNs));
  const oracleRss = median(oracleRuns.map((sample) => sample.peakRssBytes));
  const engineRss = median(engineRuns.map((sample) => sample.peakRssBytes));
  const ratio = (candidate, baseline) => candidate !== null && baseline ? candidate / baseline : null;
  cases.push({
    id,
    file: name,
    operation: metadataById.get(id)?.operation ?? null,
    memory_profile: metadataById.get(id)?.memory_profile ?? null,
    work_units: metadataById.get(id)?.work_units ?? null,
    ok,
    oracle: { wall_ns: oracleWall, peak_rss_bytes: oracleRss, instructions: median(oracleRuns.map((sample) => sample.instructions)), cycles: median(oracleRuns.map((sample) => sample.cycles)), page_faults: median(oracleRuns.map((sample) => sample.pageFaults)), page_reclaims: median(oracleRuns.map((sample) => sample.pageReclaims)) },
    engine: { wall_ns: engineWall, peak_rss_bytes: engineRss, instructions: median(engineRuns.map((sample) => sample.instructions)), cycles: median(engineRuns.map((sample) => sample.cycles)), page_faults: median(engineRuns.map((sample) => sample.pageFaults)), page_reclaims: median(engineRuns.map((sample) => sample.pageReclaims)) },
    engine_over_oracle: { wall_time_ratio: ratio(engineWall, oracleWall), rss_ratio: ratio(engineRss, oracleRss) },
  });
  if (!ok) {
    failures++;
    console.error(`${name}: output/status differs (oracle=${expected.status}, engine=${result.status})`);
    if (expected.stdout !== result.stdout) console.error(`oracle stdout: ${JSON.stringify(expected.stdout)}\nengine stdout: ${JSON.stringify(result.stdout)}`);
    if (result.stderr) console.error(result.stderr.trim());
  }
}
const validCases = cases.filter((item) => item.ok);
const aggregate = (field) => {
  const oracleValues = validCases.map((item) => item.oracle[field]).filter(Number.isFinite);
  const engineValues = validCases.map((item) => item.engine[field]).filter(Number.isFinite);
  const oracleTotal = oracleValues.reduce((sum, value) => sum + value, 0);
  const engineTotal = engineValues.reduce((sum, value) => sum + value, 0);
  return {
    oracle_total: oracleTotal,
    engine_total: engineTotal,
    engine_over_oracle: oracleTotal ? engineTotal / oracleTotal : null,
  };
};
const scoreMetric = (field) => {
  if (failures) return null;
  const ratios = validCases.map((item) => {
    const oracleValue = item.oracle[field];
    const engineValue = item.engine[field];
    return Number.isFinite(oracleValue) && Number.isFinite(engineValue) && engineValue > 0 ? oracleValue / engineValue : null;
  });
  const mean = geometricMean(ratios);
  return mean === null ? null : 100 * mean;
};
const speedScore = scoreMetric("wall_ns");
const memoryScore = scoreMetric("peak_rss_bytes");
const overallScore = geometricMean([speedScore, memoryScore]);
const familyScores = families.map(({ name, first, last }) => {
  const members = validCases.filter((item) => item.id >= first && item.id <= last);
  const metricScore = (field) => {
    if (failures || !members.length) return null;
    const ratios = members.map((item) => item.oracle[field] / item.engine[field]);
    const mean = geometricMean(ratios);
    return mean === null ? null : 100 * mean;
  };
  const speed = metricScore("wall_ns");
  const memory = metricScore("peak_rss_bytes");
  return { name, count: members.length, speed_score: speed, memory_score: memory, overall_score: geometricMean([speed, memory]) };
});
const profileNames = [...new Set(manifest.cases.map(({ memory_profile }) => memory_profile))].sort();
const profileScores = profileNames.map((memory_profile) => {
  const members = validCases.filter((item) => item.memory_profile === memory_profile);
  const metricScore = (field) => {
    if (failures || !members.length) return null;
    const ratios = members.map((item) => item.oracle[field] / item.engine[field]);
    const mean = geometricMean(ratios);
    return mean === null ? null : 100 * mean;
  };
  const speed = metricScore("wall_ns");
  const memory = metricScore("peak_rss_bytes");
  return { memory_profile, count: members.length, speed_score: speed, memory_score: memory, overall_score: geometricMean([speed, memory]) };
});
const report = {
  schema: 1,
  oracle,
  engine,
  runs,
  timeout_ms: timeout,
  range: { first, last },
  count: cases.length,
  passed: validCases.length,
  failed: failures,
  aggregate: { wall_ns: aggregate("wall_ns"), peak_rss_bytes: aggregate("peak_rss_bytes"), instructions: aggregate("instructions"), cycles: aggregate("cycles"), page_faults: aggregate("page_faults"), page_reclaims: aggregate("page_reclaims") },
  score: {
    version: 1,
    reference: "oracle",
    speed_score: speedScore,
    memory_score: memoryScore,
    overall_score: overallScore,
    family_scores: familyScores,
    memory_profile_scores: profileScores,
  },
  cases,
};
if (outputFile) writeFileSync(outputFile, JSON.stringify(report, null, 2) + "\n");
if (failures) {
  console.error(`${failures} of ${last - first + 1} micro-cases failed under ${engine}`);
  process.exit(1);
}
console.log(`Score: ${overallScore === null ? "unavailable" : overallScore.toFixed(3)}`);
console.log(JSON.stringify(report));
