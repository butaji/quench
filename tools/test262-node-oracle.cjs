#!/usr/bin/env node
// Re-adjudicates Quench's Node waivers: each waived pinned Test262 case must
// also fail under the local Node with the pinned harness. With path
// arguments, reports Node's verdict for those cases instead.
//
//   node tools/test262-node-oracle.cjs            # verify the waiver manifest
//   node tools/test262-node-oracle.cjs PATH...    # triage cases (test-relative)
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const REPOSITORY = path.resolve(__dirname, '..');
const MANIFEST = path.join(REPOSITORY, 'crates/quench-test262/node-waivers.json');
const SUITE = process.env.TEST262_DIR
  ? path.resolve(process.env.TEST262_DIR)
  : path.join(REPOSITORY, 'tests/test262');
// The longest an async case may take to call $DONE.
const ASYNC_DONE_TIMEOUT_MS = 5000;
const DEFAULT_HARNESS = ['assert.js', 'sta.js'];
const ASYNC_HARNESS = 'doneprintHandle.js';
// Manifest marker for a waived case the oracle cannot run.
const UNVERIFIABLE = 'unverifiable';

function frontmatter(source) {
  const match = /\/\*---([\s\S]*?)---\*\//.exec(source);
  return match ? match[1] : '';
}

function yamlList(yaml, key) {
  const inline = new RegExp(`^\\s*${key}:\\s*\\[([^\\]]*)\\]`, 'm').exec(yaml);
  if (inline) return inline[1].split(',').map((item) => item.trim()).filter(Boolean);
  const block = new RegExp(`^\\s*${key}:\\s*\\n((?:\\s+-\\s+.*\\n?)+)`, 'm').exec(yaml);
  if (!block) return [];
  return block[1].split('\n').map((line) => line.replace(/^\s*-\s*/, '').trim()).filter(Boolean);
}

function metadata(source) {
  const yaml = frontmatter(source);
  const negative = yaml.indexOf('negative:');
  return {
    flags: yamlList(yaml, 'flags'),
    includes: yamlList(yaml, 'includes'),
    negativeType: negative < 0 ? null : (/type:\s*(\w+)/.exec(yaml.slice(negative)) || [])[1],
  };
}

function host(context) {
  const global = vm.runInContext('this', context);
  const $262 = {
    global,
    createRealm: () => host(vm.createContext()),
    evalScript: (source) => vm.runInContext(source, context),
    detachArrayBuffer: (buffer) => structuredClone(buffer, { transfer: [buffer] }),
    gc: () => globalThis.gc?.(),
    agent: {},
  };
  global.$262 = $262;
  global.print = (...values) => console.log(...values);
  return $262;
}

async function runMode(relative, source, meta, strict) {
  const context = vm.createContext();
  host(context);
  let done;
  const completion = new Promise((resolve) => { done = resolve; });
  vm.runInContext('this', context).$DONE = (error) => done(error === undefined ? null : error);
  const harness = meta.flags.includes('raw')
    ? []
    : [...DEFAULT_HARNESS, ...(meta.flags.includes('async') ? [ASYNC_HARNESS] : []), ...meta.includes];
  try {
    for (const file of harness) {
      vm.runInContext(fs.readFileSync(path.join(SUITE, 'harness', file), 'utf8'), context, { filename: file });
    }
    vm.runInContext((strict ? '"use strict";\n' : '') + source, context, { filename: relative });
    if (meta.flags.includes('async')) {
      let timer;
      const timeout = new Promise((resolve) => {
        timer = setTimeout(() => resolve(new Error('$DONE was not called')), ASYNC_DONE_TIMEOUT_MS);
      });
      const error = await Promise.race([completion, timeout]);
      clearTimeout(timer);
      if (error) throw error;
    }
    return meta.negativeType ? `expected ${meta.negativeType}` : null;
  } catch (error) {
    const name = error?.constructor?.name;
    if (meta.negativeType && name === meta.negativeType) return null;
    return error?.message === undefined ? String(error) : `${name}: ${error.message}`;
  }
}

// Node's failures for every mode the case declares, or null when all pass.
async function nodeFailures(relative) {
  const source = fs.readFileSync(path.join(SUITE, 'test', relative), 'utf8');
  const meta = metadata(source);
  if (meta.flags.includes('module')) return { unverifiable: 'module cases need a module loader' };
  const strictModes = meta.flags.includes('onlyStrict')
    ? [true]
    : meta.flags.includes('noStrict') || meta.flags.includes('raw') ? [false] : [false, true];
  const failures = [];
  for (const strict of strictModes) {
    const failure = await runMode(relative, source, meta, strict);
    if (failure !== null) failures.push(`${strict ? 'strict' : 'sloppy'}: ${failure}`);
  }
  return failures.length === 0 ? null : { failures };
}

async function main() {
  const triage = process.argv.slice(2);
  if (triage.length > 0) {
    for (const relative of triage) {
      const verdict = await nodeFailures(relative);
      const label = verdict === null ? 'node-pass' : verdict.unverifiable ? 'node-unverifiable' : 'node-fail';
      console.log(`${label} ${relative}${verdict ? `  ${JSON.stringify(verdict)}` : ''}`);
    }
    return;
  }
  const manifest = JSON.parse(fs.readFileSync(MANIFEST, 'utf8'));
  if (process.version !== manifest.oracle.version) {
    console.log(`note: manifest adjudicated with ${manifest.oracle.version}; running ${process.version}`);
  }
  let contradicted = 0;
  let byRule = 0;
  let total = 0;
  for (const group of manifest.groups) {
    let groupFailures = 0;
    for (const { path: relative, node } of group.cases) {
      total += 1;
      const verdict = await nodeFailures(relative);
      // A case Node cannot run is waived by its group's Node-arbitrated rule
      // only when the manifest says so; any Node verdict must then be a failure.
      if (node === UNVERIFIABLE && verdict?.unverifiable) {
        byRule += 1;
      } else if (verdict === null || verdict.unverifiable) {
        contradicted += 1;
        console.log(`contradicted ${group.id} ${relative}: ${verdict ? verdict.unverifiable : 'Node passes'}`);
      } else {
        groupFailures += 1;
      }
    }
    console.log(`${group.id}: node fails ${groupFailures}/${group.cases.length}`);
  }
  console.log(`waivers=${total} by-rule=${byRule} contradicted=${contradicted} node=${process.version}`);
  process.exitCode = contradicted === 0 ? 0 : 1;
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 2;
});
