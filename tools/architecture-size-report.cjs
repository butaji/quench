#!/usr/bin/env node
'use strict';

// Read-only complexity inventory for the generated interpreter.  It reports
// source/static footprint and, when an artifact is supplied, the platform
// text/data segments.  It never selects a runtime path or inspects fixtures.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const artifact = process.argv[2] ? path.resolve(process.argv[2]) : null;
const sourceRoots = [
  path.join(root, 'crates', 'quench-runtime', 'src'),
  path.join(root, 'crates', 'quench-runtime', 'build.rs'),
];

function rustFiles(entry) {
  const stat = fs.statSync(entry);
  if (stat.isFile()) return entry.endsWith('.rs') ? [entry] : [];
  const out = [];
  for (const child of fs.readdirSync(entry)) out.push(...rustFiles(path.join(entry, child)));
  return out;
}

const files = sourceRoots.flatMap(rustFiles).sort();
const source = files.reduce((total, file) => total + fs.statSync(file).size, 0);
const lines = files.reduce((total, file) => total + fs.readFileSync(file, 'utf8').split('\n').length, 0);
const catalog = fs.readFileSync(path.join(root, 'crates', 'quench-runtime', 'src', 'ir.rs'), 'utf8')
  .split('\n').filter((line) => /^\s*[A-Z][A-Za-z0-9]+\s*=\s*\d+\s*\//.test(line)).length;

function segmentSizes() {
  if (!artifact || !fs.existsSync(artifact)) return { available: false, reason: 'artifact-not-found' };
  const result = spawnSync('size', ['-m', artifact], { encoding: 'utf8' });
  if (result.status !== 0) return { available: false, reason: 'size-command-failed' };
  const sections = {};
  for (const line of result.stdout.split('\n')) {
    const match = line.match(/^\s*Section (__[A-Za-z0-9_]+):\s+(\d+)/);
    if (match) sections[match[1]] = Number(match[2]);
  }
  return { available: true, sections };
}

const report = {
  schema: 1,
  artifact: artifact && fs.existsSync(artifact) ? path.relative(root, artifact) : null,
  source: { rust_files: files.length, bytes: source, lines },
  generated_catalog_rows: catalog,
  segments: segmentSizes(),
  accounting: [
    'source includes quench-runtime Rust only',
    'segments are artifact-level static/text footprint when measured',
    'heap references and disposable cache state remain runtime counters, not admission rules',
  ],
};
console.log(JSON.stringify(report, null, 2));
