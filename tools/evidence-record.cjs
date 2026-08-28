#!/usr/bin/env node
"use strict";

// Measurement-only canonical record. It never reaches production crates or
// chooses an execution path; all summaries are derived from raw samples.
const cp = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const args = process.argv.slice(2);
const option = (name, fallback = null) => {
  const index = args.indexOf(name);
  return index >= 0 && index + 1 < args.length ? args[index + 1] : fallback;
};
const required = (name) => {
  const value = option(name);
  if (!value) throw new Error(`missing ${name}`);
  return value;
};
const readJson = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const sha256 = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const command = (program, argv) => {
  const result = cp.spawnSync(program, argv, { cwd: root, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
};
const median = (values) => {
  const sorted = values.filter(Number.isFinite).sort((left, right) => left - right);
  if (sorted.length === 0) return null;
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const mad = (values) => {
  const center = median(values);
  return center === null ? null : median(values.map((value) => Math.abs(value - center)));
};
const geometricMean = (values) =>
  values.length && values.every((value) => Number.isFinite(value) && value > 0)
    ? Math.exp(values.reduce((sum, value) => sum + Math.log(value), 0) / values.length)
    : null;

function fixtureRecord(name, raw) {
  const samples = raw.quench || [];
  const valid = raw.valid === true && samples.length > 0 && samples.every(
    (sample) => sample.status === 0 && !sample.timedOut && Number.isFinite(sample.score)
  );
  const scores = samples.map((sample) => sample.score);
  const walls = samples.map((sample) => sample.wallNs / 1e6);
  return {
    name,
    valid,
    raw: { node: raw.node || [], quench: samples },
    score: { median: median(scores), mad: mad(scores) },
    wall_ms: { median: median(walls), mad: mad(walls) },
  };
}

function leverage(fixtures) {
  const valid = fixtures.filter((fixture) => fixture.valid && fixture.score.median > 0);
  const overall = geometricMean(valid.map((fixture) => fixture.score.median));
  const totalWall = valid.reduce((sum, fixture) => sum + fixture.wall_ms.median, 0);
  return {
    valid_fixture_count: valid.length,
    all_valid: valid.length === fixtures.length,
    geometric_score: valid.length === fixtures.length ? overall : null,
    fixtures: fixtures.map((fixture) => ({
      name: fixture.name,
      score_log_weight: fixture.valid ? 1 / fixtures.length : null,
      wall_share: fixture.valid ? fixture.wall_ms.median / totalWall : null,
      // A fixture speedup f lifts the geometric suite score by f^(1/N),
      // before Amdahl interactions. This is a ranking bound, never a claim.
      suite_multiplier_if_fixture_10x: fixture.valid ? Math.pow(10, 1 / fixtures.length) : null,
    })),
  };
}

const input = path.resolve(required("--runs"));
const binary = path.resolve(required("--binary"));
const output = path.resolve(option("--out", path.join(root, "target", "evidence-record.json")));
const append = option("--append");
const raw = readJson(input);
const results = raw.results || {};
const fixtures = Object.entries(results).map(([name, entry]) => fixtureRecord(name, entry));
const dirtyDiff = command("git", ["diff", "--binary"]);
const record = {
  schema: "quench.evidence-record/v1",
  id: option("--id", crypto.randomUUID()),
  parent_id: option("--parent"),
  lane: option("--lane", "control"),
  captured_at: new Date().toISOString(),
  artifact: {
    binary,
    binary_sha256: sha256(binary),
    git_commit: command("git", ["rev-parse", "HEAD"]),
    dirty_diff_sha256: dirtyDiff
      ? crypto.createHash("sha256").update(dirtyDiff).digest("hex")
      : null,
    cargo_lock_sha256: sha256(path.join(root, "Cargo.lock")),
    rustc: command("rustc", ["-Vv"]),
    platform: `${process.platform}/${process.arch}`,
    cpu: os.cpus()[0]?.model || null,
  },
  raw_input: input,
  fixtures,
  score_leverage: leverage(fixtures),
  unavailable: {
    instructions: null,
    cycles: null,
    allocator_bytes: null,
    dwarf_uuid: null,
  },
};
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, JSON.stringify(record, null, 2) + "\n");
if (append) {
  const journal = path.resolve(append);
  fs.mkdirSync(path.dirname(journal), { recursive: true });
  fs.appendFileSync(journal, JSON.stringify(record) + "\n");
}
console.log(JSON.stringify(record.score_leverage, null, 2));
