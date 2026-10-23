// Test262 case execution on a Node-compatible host (Node, Bun): the pinned harness
// composition, the $262 host object and the per-mode verdict. Shared by the Node
// oracle and the cross-engine case runner.
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const REPOSITORY = path.resolve(__dirname, '../..');
const SUITE = process.env.TEST262_DIR
  ? path.resolve(process.env.TEST262_DIR)
  : path.join(REPOSITORY, 'tests/test262');
// The longest an async case may take to call $DONE.
const ASYNC_DONE_TIMEOUT_MS = 5000;
const DEFAULT_HARNESS = ['assert.js', 'sta.js'];
const ASYNC_HARNESS = 'doneprintHandle.js';
// The lines doneprintHandle.js's $DONE prints, which report an async case's completion.
const ASYNC_COMPLETE = 'Test262:AsyncTestComplete';
const ASYNC_FAILURE_PREFIX = 'Test262:AsyncTestFailure:';

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

// `onPrint` receives each line the realm prints; an async case reports through it.
function host(context, onPrint = (line) => console.log(line)) {
  const global = vm.runInContext('this', context);
  const $262 = {
    global,
    createRealm: () => host(vm.createContext(), onPrint),
    evalScript: (source) => vm.runInContext(source, context),
    detachArrayBuffer: (buffer) => structuredClone(buffer, { transfer: [buffer] }),
    gc: () => globalThis.gc?.(),
    agent: {},
  };
  global.$262 = $262;
  global.print = (...values) => onPrint(values.join(' '));
  return $262;
}

async function runMode(relative, source, meta, strict) {
  const context = vm.createContext();
  let done;
  const completion = new Promise((resolve) => { done = resolve; });
  host(context, (line) => {
    if (line === ASYNC_COMPLETE) done(null);
    else if (line.startsWith(ASYNC_FAILURE_PREFIX)) done(new Error(line.slice(ASYNC_FAILURE_PREFIX.length)));
    else console.log(line);
  });
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

// A JavaScript engine's failures for every mode the case declares, or null when all pass.
async function caseFailures(relative) {
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

module.exports = { REPOSITORY, SUITE, caseFailures };
