// Focused speed and peak-RSS comparison: runs run-npm on each package RUNS times and
// keeps each engine's fastest wall time with that run's CPU time and peak RSS.
// Usage: quench-node compare.mjs RUN_NPM NODE QUENCH BUN RUNS PACKAGE...
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const [runNpm, node, quench, bun, runsArg, ...packages] = process.argv.slice(2);
const runs = Number(runsArg);
if (!runNpm || !(runs > 0) || packages.length === 0) {
  console.error('usage: compare.mjs RUN_NPM NODE QUENCH BUN RUNS PACKAGE...');
  process.exit(2);
}
const engines = ['quench', 'node', 'bun'];
const DEADLINE_SECONDS = '60';
const report = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'npm-compare-')), 'report.json');
for (const name of packages) {
  const fastest = {};
  for (let run = 0; run < runs; run++) {
    const result = spawnSync(runNpm, [node, quench, report, DEADLINE_SECONDS, '--rss', bun, name], { encoding: 'utf8' });
    if (result.status !== 0) {
      console.log(`${name}: run-npm failed: ${(result.stdout + result.stderr).trim()}`);
      break;
    }
    const record = JSON.parse(fs.readFileSync(report, 'utf8')).records[0];
    for (const engine of engines) {
      const observation = record[engine];
      if (!fastest[engine] || observation.wall_nanos < fastest[engine].wall_nanos) fastest[engine] = observation;
    }
  }
  if (!fastest.quench) continue;
  const cell = engine => {
    const o = fastest[engine];
    return `${engine} ${(o.wall_nanos / 1e6).toFixed(0).padStart(6)} ms (cpu ${(o.cpu_nanos / 1e6).toFixed(0).padStart(5)}) ${(o.peak_rss_bytes / 2 ** 20).toFixed(1).padStart(6)} MiB`;
  };
  console.log(`${name.padEnd(28)} ${engines.map(cell).join(' | ')}`);
}
