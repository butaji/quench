#!/usr/bin/env node
// Runs one spec-testsuite script, converted by `wast-json`, on a JavaScript engine's
// WebAssembly API (Node, Bun), as one process per script, so a driver can measure each
// engine's process the way it measures Quench's `run --worker`.
//
//   node tools/wast-engine-run.cjs SCRIPT.json
//
// Prints `passed=N failed=M unsupported=K` and exits 0 when no command failed. A command
// the JS API cannot express (v128 or host references crossing the boundary, threads)
// counts as unsupported, not as a failure.
'use strict';

const fs = require('node:fs');

const FAIL_STATUS = 1;
const USAGE_STATUS = 2;
// The values `spectest` exports, as the reference interpreter defines them.
const SPECTEST_GLOBAL_INT = 666;
const SPECTEST_GLOBAL_FLOAT = 666.6;
const SPECTEST_TABLE = { initial: 10, maximum: 20 };
const SPECTEST_MEMORY = { initial: 1, maximum: 2 };

class Unsupported extends Error {}

function spectest() {
  const print = () => {};
  return {
    print, print_i32: print, print_i64: print, print_f32: print, print_f64: print,
    print_i32_f32: print, print_f64_f64: print,
    global_i32: new WebAssembly.Global({ value: 'i32' }, SPECTEST_GLOBAL_INT),
    global_i64: new WebAssembly.Global({ value: 'i64' }, BigInt(SPECTEST_GLOBAL_INT)),
    global_f32: new WebAssembly.Global({ value: 'f32' }, SPECTEST_GLOBAL_FLOAT),
    global_f64: new WebAssembly.Global({ value: 'f64' }, SPECTEST_GLOBAL_FLOAT),
    table: new WebAssembly.Table({ element: 'anyfunc', ...SPECTEST_TABLE }),
    memory: new WebAssembly.Memory(SPECTEST_MEMORY),
  };
}

const f32 = new Float32Array(1);
const u32 = new Uint32Array(f32.buffer);
const f64 = new Float64Array(1);
const u64 = new BigUint64Array(f64.buffer);

// Host references stand for themselves: one object per id, compared by identity.
const references = new Map();
function reference(id) {
  if (!references.has(id)) references.set(id, { reference: id });
  return references.get(id);
}

function argument(value) {
  switch (value.type) {
    case 'i32': return Number(BigInt.asIntN(32, BigInt(value.bits)));
    case 'i64': return BigInt.asIntN(64, BigInt(value.bits));
    case 'f32': u32[0] = Number(value.bits); return f32[0];
    case 'f64': u64[0] = BigInt(value.bits); return f64[0];
    case 'null': return null;
    case 'externref': return reference(value.id);
    default: throw new Unsupported(`argument ${value.type}`);
  }
}

function matches(expected, got) {
  switch (expected.type) {
    case 'i32': return typeof got === 'number' && (got >>> 0) === Number(expected.bits);
    case 'i64': return typeof got === 'bigint' && BigInt.asUintN(64, got) === BigInt(expected.bits);
    case 'f32':
      if (expected.nan) return Number.isNaN(got);
      f32[0] = got;
      return u32[0] === Number(expected.bits);
    case 'f64':
      if (expected.nan) return Number.isNaN(got);
      f64[0] = got;
      return u64[0] === BigInt(expected.bits);
    case 'null': return got === null;
    case 'externref': return expected.id === null ? got !== null : got === reference(expected.id);
    case 'funcref': return typeof got === 'function';
    case 'ref': return got !== null && got !== undefined;
    case 'either': return expected.values.some((option) => matches(option, got));
    default: throw new Unsupported(`result ${expected.type}`);
  }
}

class Store {
  constructor() {
    this.registered = new Map([['spectest', spectest()]]);
    this.named = new Map();
    this.definitions = new Map();
    this.current = null;
  }

  imports(module) {
    const imports = {};
    for (const { module: from, name } of WebAssembly.Module.imports(module)) {
      const exports = this.registered.get(from);
      if (!exports || !(name in exports)) continue;
      (imports[from] ??= {})[name] = exports[name];
    }
    return imports;
  }

  instantiate(module) {
    return new WebAssembly.Instance(module, this.imports(module));
  }

  instance(name) {
    const instance = name ? this.named.get(name) : this.current;
    if (!instance) throw new Error(`no module ${name ?? '(current)'}`);
    return instance;
  }

  act(action) {
    if (action.type === 'module') {
      this.instantiate(compile(action.binary));
      return [];
    }
    const exported = this.instance(action.module).exports[action.field];
    if (exported === undefined) throw new Error(`unknown export ${action.field}`);
    if (action.type === 'get') return [exported.value];
    const result = exported(...action.args.map(argument));
    return Array.isArray(result) ? result : result === undefined ? [] : [result];
  }
}

function compile(binary) {
  if (binary === null) throw new WebAssembly.CompileError('text-level malformed module');
  return new WebAssembly.Module(Buffer.from(binary, 'hex'));
}

function throwsAs(run, accepts) {
  try {
    run();
  } catch (error) {
    if (error instanceof Unsupported) throw error;
    return accepts(error);
  }
  return false;
}

function execute(store, command) {
  switch (command.cmd) {
    case 'module': {
      const instance = store.instantiate(compile(command.binary));
      store.current = instance;
      if (command.name) store.named.set(command.name, instance);
      return true;
    }
    case 'module_definition':
      store.definitions.set(command.name, compile(command.binary));
      return true;
    case 'module_instance': {
      const instance = store.instantiate(store.definitions.get(command.module));
      store.current = instance;
      if (command.instance) store.named.set(command.instance, instance);
      return true;
    }
    case 'register':
      store.registered.set(command.as, store.instance(command.module).exports);
      return true;
    case 'action':
      store.act(command.action);
      return true;
    case 'assert_return': {
      // A v128 result cannot cross into JavaScript: the call itself throws a TypeError.
      if (command.expected.some((expected) => expected.type === 'v128')) throw new Unsupported('result v128');
      const got = store.act(command.action);
      return got.length === command.expected.length
        && command.expected.every((expected, index) => matches(expected, got[index]));
    }
    case 'assert_trap':
      return throwsAs(() => store.act(command.action), (error) => error instanceof WebAssembly.RuntimeError);
    case 'assert_exhaustion':
      return throwsAs(() => store.act(command.action), (error) => error instanceof RangeError);
    case 'assert_exception':
      return throwsAs(() => store.act(command.action), (error) => error instanceof WebAssembly.Exception);
    case 'assert_malformed':
    case 'assert_invalid':
      return throwsAs(() => compile(command.binary), (error) => error instanceof WebAssembly.CompileError);
    case 'assert_unlinkable':
      return throwsAs(
        () => store.instantiate(compile(command.binary)),
        (error) => error instanceof WebAssembly.LinkError,
      );
    default:
      throw new Unsupported(command.kind ?? command.cmd);
  }
}

function main() {
  const script = process.argv[2];
  if (!script) {
    console.error('usage: wast-engine-run.cjs SCRIPT.json');
    return USAGE_STATUS;
  }
  const { commands } = JSON.parse(fs.readFileSync(script, 'utf8'));
  const store = new Store();
  const counts = { passed: 0, failed: 0, unsupported: 0 };
  for (const command of commands) {
    let passed;
    try {
      passed = execute(store, command);
    } catch (error) {
      if (error instanceof Unsupported) {
        counts.unsupported += 1;
        continue;
      }
      passed = false;
    }
    counts[passed ? 'passed' : 'failed'] += 1;
  }
  console.log(`passed=${counts.passed} failed=${counts.failed} unsupported=${counts.unsupported}`);
  return counts.failed === 0 ? 0 : FAIL_STATUS;
}

process.exitCode = main();
