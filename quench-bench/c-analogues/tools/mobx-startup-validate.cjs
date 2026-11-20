// Node reference for the C mobx-startup analogue (quench-bench/c-analogues/mobx-startup.c).
//
// usage: node mobx-startup-validate.cjs [jetstream-root] [--time-only]
//   e.g.  node mobx-startup-validate.cjs /home/user/quench/quench-bench/jetstream
//
// 1. Builds the 30 per-iteration bundle copies exactly as StartupBenchmark.createIterationSourceCode does.
// 2. Timing pass (default, un-instrumented bundle/mobx/dist/bundle.es6.min.js): per iteration it times
//    the parse+compile of the copy (vm.Script), the full evaluation of the copy (eval: parse + module body),
//    and runTest(). The JS row's own time is eval + runTest.
// 3. Counting pass (instrumented copy of the unminified bundle.es6.js, same MobX code): each call of a
//    global MobX function (reportObserved, propagateChanged, startBatch, endBatch, runReactions, shouldCompute,
//    hasInterceptors, hasListeners, addObserver, removeObserver) and each call of a prototype method
//    (runReaction_, computeValue_, trackAndCompute) is counted. These counts are what the C analogue
//    counts and checks on every iteration.
// 4. Module body shape: see mobx-body-shape.cjs (functions, classes, objects, arrays created by the body).
// 5. The log text hash: FNV-1a 64 over every log line followed by "\n".
//
// The printed numbers are the ones hard-coded in mobx-startup.c.
'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');

// The JetStream root holds mobx/dist/bundle.es6.min.js and bundle.es6.js (argument 1, default: the worktree's jetstream dir).
const root = process.argv.find((a, i) => i > 1 && !a.startsWith('--')) || path.join(__dirname, '..', '..', 'jetstream');
const dist = path.join(root, 'mobx/dist');
const minSrc = fs.readFileSync(path.join(dist, 'bundle.es6.min.js'), 'utf8');
const esSrc = fs.readFileSync(path.join(dist, 'bundle.es6.js'), 'utf8');
const timeOnly = process.argv.includes('--time-only');
const ITER = 30;

// StartupBenchmark.createIterationSourceCode: cache-busting comments become /*cacheId*/, reuse 8 per id.
const CACHE_RE = /\n\/\*ThouShaltNotCache\*\/\n/g;
function iterationSource(src, i) {
  const cacheId = Math.floor(i / 8) * 8;
  return src.replaceAll(CACHE_RE, `/*${cacheId}*/`);
}
const cacheComments = minSrc.match(CACHE_RE).length;
if (cacheComments !== 464) throw new Error('expected 464 cache comments, got ' + cacheComments);
const sources = [];
for (let i = 0; i < ITER; i++) sources.push(iterationSource(minSrc, i));

function runIteration(src) {
  let MobXBenchmark;
  eval(src);
  return MobXBenchmark.runTest();
}

function fnv1a64(lines) {
  const bytes = Buffer.from(lines.join('\n') + '\n', 'utf8');
  let h = 0xcbf29ce484222325n;
  for (let i = 0; i < bytes.length; i++) h = BigInt.asUintN(64, (h ^ BigInt(bytes[i])) * 0x100000001b3n);
  return h.toString(16).padStart(16, '0');
}

if (!timeOnly) {
  // ---- counting pass on the instrumented unminified bundle ----
  // Global MobX functions are counted under their name ("startBatch"); prototype methods under
  // "Class.method()" ("_proto.runReaction_" is "runReaction_()"). Each call of a counted function
  // increments the counter, so the counts are the calls that run.
  const globalRe = /function\s+([A-Za-z_$][\w$]*)\(([^()]*)\)\s*\{/g;
  const methodRe = /_proto\.([A-Za-z_$][\w$]*)=function\s*\(([^()]*)\)\s*\{/g;
  const inject = (key) => `__C[${JSON.stringify(key)}]=(__C[${JSON.stringify(key)}]|0)+1;`;
  let instrumented = esSrc
    .replace(globalRe, (m, name, args) => `function ${name}(${args}){${inject(name)}`)
    .replace(methodRe, (m, name, args) => `_proto.${name}=function(${args}){${inject(name + '()')}`);
  const perIter = [];
  let lastLines = null;
  for (let i = 0; i < ITER; i++) {
    globalThis.__C = {};
    lastLines = runIteration(instrumented.replace('/*ThouShaltNotCache*/', `/*${Math.floor(i / 8) * 8}*/`));
    perIter.push(globalThis.__C);
  }
  const first = perIter[0];
  const same = perIter.every((c) => JSON.stringify(c) === JSON.stringify(first));
  console.log('counts per iteration (identical across the 30 iterations: ' + same + ')');
  const keys = Object.keys(first).sort();
  for (const k of keys) console.log(`  ${k} ${first[k]}`);
  console.log('log entries ' + lastLines.length + ' hash ' + fnv1a64(lastLines));
}

// ---- timing pass on the minified bundle, as the JS row runs it ----
let total = 0, parseTotal = 0, evalTotal = 0, runTotal = 0;
const hashes = new Set();
for (let i = 0; i < ITER; i++) {
  const src = sources[i];
  let t0 = performance.now();
  new vm.Script(src, { filename: 'iteration' + i + '.js' });
  const tParse = performance.now() - t0;
  t0 = performance.now();
  let MobXBenchmark;
  eval(src);
  const tEval = performance.now() - t0;
  t0 = performance.now();
  const lines = MobXBenchmark.runTest();
  const tRun = performance.now() - t0;
  if (lines.length !== 6824) throw new Error('log length ' + lines.length);
  if (i === 0 || i === ITER - 1) hashes.add(fnv1a64(lines));
  parseTotal += tParse; evalTotal += tEval; runTotal += tRun;
  total += tEval + tRun;
}
console.log(`timing over ${ITER} iterations (ms): parse+compile ${parseTotal.toFixed(2)}, eval (parse+body) ${evalTotal.toFixed(2)}, runTest ${runTotal.toFixed(2)}, JS row (eval+runTest) ${total.toFixed(2)}`);
console.log(`per iteration: parse ${(parseTotal / ITER).toFixed(3)} ms, runTest ${(runTotal / ITER).toFixed(3)} ms`);
console.log('timing-pass log hashes (iterations 0 and last): ' + [...hashes].join(' '));
