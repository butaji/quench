import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { ROOT, hash } from "./catalog.mjs";

export function resolveBinary(name) {
  const candidates = name.includes(path.sep) ? [path.resolve(name)] : (process.env.PATH || "").split(path.delimiter).map((p) => path.join(p, name));
  const result = candidates.find((p) => { try { fs.accessSync(p, fs.constants.X_OK); return fs.statSync(p).isFile(); } catch { return false; } });
  if (!result) throw new Error(`engine unavailable: ${name}`);
  return fs.realpathSync(result);
}

export function binaryIdentity(binary) {
  return { path: binary, sha256: hash(fs.readFileSync(binary)),
    version: spawnSync(binary, ["--version"], { encoding: "utf8", timeout: 3000, maxBuffer: 4096 }).stdout?.trim() || "unavailable" };
}

export function payloadSource(c, scenario, options) {
  let source = c.source;
  if (c.legacy) source = `registerMicro({setup:function(){return {};},variants:{original:(function(console,print){return function(){\n${source}\nreturn result;};})({log:function(){}},function(){})}});`;
  if (scenario.sourceForm === "wrapped") source = `(function(){\n${source}\n}());`;
  const config = { ...scenario, ...options };
  return `var __microSpec; function registerMicro(s){__microSpec=s;}\n${source}\nvar __microConfig=${JSON.stringify(config)};\n${fs.readFileSync(path.join(ROOT, "payload.js"), "utf8")}`;
}

export function parseRss(stderr, platform = process.platform) {
  const re = platform === "darwin" ? /(\d+)\s+maximum resident set size/i : /Maximum resident set size \(kbytes\):\s*(\d+)/i;
  const match = stderr.match(re);
  return match ? Number(match[1]) * (platform === "darwin" ? 1 : 1024) : null;
}

export function childEnvironment(options) {
  const env = { ...(options.env || process.env) };
  if (options.mode !== "diagnostic") {
    delete env.QUENCH_EXEC_TRACE;
    delete env.QUENCH_LOOP_TRACE;
  }
  return env;
}

export function parseOutput(stdout) {
  const lines = stdout.split(/\r?\n/);
  if (lines.some((l) => l.startsWith("MICRO_ERROR "))) throw new Error(lines.find((l) => l.startsWith("MICRO_ERROR ")));
  const results = lines.filter((l) => l.startsWith("MICRO_RESULT "));
  if (results.length !== 1) throw new Error("expected exactly one completed result (including async completion)");
  const p = JSON.parse(results[0].slice(13));
  validatePayload(p);
  return p;
}

function validatePayload(p) {
  if (p.schema !== 1 || typeof p.result !== "string" || !p.validations || !p.windows?.length) throw new Error("malformed result");
  for (const w of p.windows) validateWindow(w);
}

function validateWindow(w) { if (!Number.isSafeInteger(w.calls) || w.calls < 1 || !Number.isFinite(w.elapsed_ns) || w.elapsed_ns <= 0) throw new Error("invalid window"); }

function sampleRss(pid, epoch, samples, started) {
  return new Promise((resolve) => {
    const ps = spawn("ps", ["-o", "rss=", "-p", String(pid)]);
    let output = "";
    ps.stdout.on("data", (chunk) => { output += chunk; });
    ps.on("error", () => resolve());
    ps.on("close", () => {
      const rss = Number(output.trim()) * 1024;
      if (rss > 0) samples.push({ elapsedMs: Date.now() - started, epoch, rss });
      resolve();
    });
  });
}

function startSampler(child, state, options) {
  if (options.mode !== "lifecycle") return { stop: async () => {} };
  let stopped = false, pending = Promise.resolve();
  const timer = setInterval(() => {
    if (!stopped && !state.sampling) {
      state.sampling = true;
      pending = sampleRss(child.pid, state.epoch, state.rssSamples, state.started).finally(() => { state.sampling = false; });
    }
  }, options.rssSampleMs || 100);
  return { stop: async () => { stopped = true; clearInterval(timer); await pending; } };
}

function attachOutput(child, state) {
  function append(kind, chunk) {
    state[kind] += chunk;
    if (state[kind].length > 32 * 1024 * 1024) { state.overflow = true; try { process.kill(-child.pid, "SIGKILL"); } catch {} }
  }
  child.stderr.on("data", (c) => append("stderr", c));
  child.stdout.on("data", (chunk) => {
    append("stdout", chunk);
    state.partial += chunk;
    const lines = state.partial.split("\n"); state.partial = lines.pop();
    for (const line of lines) {
      if (line.startsWith("MICRO_PHASE ")) {
        try { const p = JSON.parse(line.slice(12)); state.epoch = p.epoch; state.phases.push({ ...p, elapsedMs: Date.now() - state.started }); } catch { state.badPhase = true; }
      }
    }
  });
}

export async function runProcess(binary, c, scenario, options = {}) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "quench-micros-"));
  const script = path.join(temp, "case.js"), timeFile = path.join(temp, "time.txt");
  fs.writeFileSync(script, payloadSource(c, scenario, options));
  const useTime = options.mode !== "lifecycle" && fs.existsSync("/usr/bin/time");
  const flags = process.platform === "darwin" ? ["-l"] : ["-v"];
  const args = useTime ? [...flags, "-o", timeFile, binary, script] : [script];
  const state = { started: Date.now(), stdout: "", stderr: "", partial: "", epoch: 0, phases: [], rssSamples: [] };
  const child = spawn(useTime ? "/usr/bin/time" : binary, args, { env: childEnvironment(options), detached: true });
  attachOutput(child, state);
  const sampler = startSampler(child, state, options);
  const timer = setTimeout(() => { state.timedOut = true; try { process.kill(-child.pid, "SIGKILL"); } catch {} }, options.timeoutMs || 120000);
  const status = await new Promise((resolve) => { child.on("error", (e) => { state.error = e.message; resolve(null); }); child.on("close", resolve); });
  clearTimeout(timer); await sampler.stop();
  const accounting = fs.existsSync(timeFile) ? fs.readFileSync(timeFile, "utf8") : "";
  const sample = finishSample(state, status, accounting);
  fs.rmSync(temp, { recursive: true, force: true });
  return sample;
}

function finishSample(state, status, accounting) {
  let payload = null, reason = state.error;
  try { payload = parseOutput(state.stdout); } catch (e) { reason = e.message; }
  const valid = status === 0 && !state.timedOut && !state.overflow && !state.badPhase && payload !== null;
  return { valid, status, reason: valid ? null : reason || "process failed", timedOut: !!state.timedOut,
    overflow: !!state.overflow, wallMs: Date.now() - state.started, peakRss: parseRss(accounting), payload,
    rssSamples: state.rssSamples, phases: state.phases, stdout: state.stdout, stderr: state.stderr, accounting,
    rssAttribution: "external process sampling; epochs based on received phase markers, not synchronous heap snapshots" };
}
