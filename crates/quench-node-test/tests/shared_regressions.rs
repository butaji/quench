//! Complex guest regressions not owned by the upstream conformance suites.

use quench_node_test::case_process::{observe_case, CaseObservation, RunResult};
use std::{fs, path::Path, process::Command, time::Duration};

const SHARED_WORKER: &str = env!("CARGO_BIN_EXE_run-parallel");
const REGRESSION_TIMEOUT: Duration = Duration::from_secs(30);

fn observe_source(source: &str) -> CaseObservation {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("regression.js");
    fs::write(&fixture, source).unwrap();
    observe_case(Path::new(SHARED_WORKER), &fixture, REGRESSION_TIMEOUT).unwrap()
}

fn observe_node_source(source: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("oracle.js");
    fs::write(&fixture, source).unwrap();
    Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into()))
        .arg(fixture)
        .output()
        .unwrap()
}

#[test]
fn strict_assert_aliases_preserve_identity_and_strict_comparison() {
    let source = r#"
const assert = require('assert');
const strict = require('assert/strict');
for (const [alias, target] of [
  ['equal', 'strictEqual'], ['notEqual', 'notStrictEqual'],
  ['deepEqual', 'deepStrictEqual'], ['notDeepEqual', 'notDeepStrictEqual']
]) {
  assert.strictEqual(strict[alias], assert[target]);
  assert.strictEqual(strict[alias], strict[target]);
}
assert.strictEqual(strict, assert.strict);
strict.deepEqual({ value: 1 }, { value: 1 });
strict.notEqual(1, '1');
strict.notDeepEqual({ value: 1 }, { value: '1' });
assert.throws(() => strict.deepEqual({ value: 1 }, { value: '1' }), { code: 'ERR_ASSERTION' });
assert.throws(() => strict.notDeepEqual({ value: 1 }, { value: 1 }), { code: 'ERR_ASSERTION' });
assert.throws(() => strict.notEqual(1, 1), { code: 'ERR_ASSERTION' });
console.log('strict-assert-aliases: ok');
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

/// A lane read of a global name whose site cached the binding must raise once the global
/// object's property is deleted, as the binding's [[Get]] on an absent name does
/// (7c042285b): Node's global object reaches the miss path the runtime CLI's does not.
#[test]
fn deleted_global_read_in_a_warmed_function_raises_reference_error() {
    let source = r#"
globalThis.LateGlobal = 1;
function read() { try { return LateGlobal; } catch (error) { return error.name; } }
for (let i = 0; i < 20; i++) read();
delete globalThis.LateGlobal;
console.log(String(read()));
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    assert_eq!(String::from_utf8_lossy(&node.stdout).trim(), "ReferenceError");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

/// A base class constructor runs frameless when constructed; its plain calls keep taking a
/// frame, whose call raises the TypeError, however often the class was constructed first.
#[test]
fn warmed_class_constructor_still_rejects_calls_without_new() {
    let source = r#"
class A { #e; x = 1; constructor(a) { this.#e = a; } get e() { return this.#e; } }
let sum = 0;
for (let i = 0; i < 50; i++) sum += new A(i).e + new A(i).x;
function call(f) { return f(1); }
const errors = [];
for (let i = 0; i < 50; i++) { try { call(A); errors.push('none'); } catch (error) { errors.push(error.name); } }
class R { constructor(o) { if (o) return o; } }
const marker = {};
console.log(sum, errors[0], errors[49], new R(marker) === marker, new R(0) instanceof R);
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    assert_eq!(String::from_utf8_lossy(&node.stdout).trim(), "1275 TypeError TypeError true true");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

#[test]
fn strict_call_followed_by_return_of_another_register_matches_node() {
    let source = r#"
function g(a, b, c, x) { return arguments.length + 90; }
function p1(x) {
  "use strict";
  g(1, 2, 3, x);
  return x;
}
console.log(p1(7));
"#;

    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(
        quench.outcome(),
        RunResult::Pass,
        "Quench failed: {:?}",
        quench.worker
    );
    assert_eq!(quench.stdout, node.stdout, "Node and Quench output differ");
    assert_eq!(node.stdout, b"7\n");
}

#[test]
fn warmed_frameless_calls_keep_arguments_and_legacy_caller_like_node() {
    // `Array(...)` makes the closure-holding activation a frame between its `arguments`
    // reads, and `peek.caller` reads the frame stack from a frameless caller.
    let source = r#"
function outer(r) {
  function f() {
    var x = Array(arguments.length);
    for (var g = 0; g < x.length; g++) x[g] = arguments[g];
    function b() { r.apply(null, x); }
    b();
  }
  return function m() { f(null, 'body'); };
}
function peek() { return peek.caller ? Array.from(peek.caller.arguments).join(',') : 'none'; }
function host(a, b) { a = 9; return peek(); }
for (let k = 0; k < 6; k++) {
  outer(function (e, v) { console.log(k, e, v); })();
  console.log(host(1, 2));
}
"#;

    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(
        quench.outcome(),
        RunResult::Pass,
        "Quench failed: {:?}",
        quench.worker
    );
    assert_eq!(quench.stdout, node.stdout, "Node and Quench output differ");
}

#[test]
fn warmed_environment_rest_calls_with_fewer_values_than_parameters_match_node() {
    // A closure-holding function with fixed parameters before a rest list runs as a
    // frameless environment activation once warm; calls passing fewer values than its
    // fixed parameters gather an empty rest list (they panicked before cbaf372c1).
    let source = r#"
function outer() {
  function r(a, b, ...m) {
    const f = () => m.length + (a === undefined ? 0 : 1) + (b === undefined ? 0 : 10);
    return f();
  }
  let sum = 0;
  for (let i = 0; i < 3000; i++) sum += r() + r(1) + r(1, 2) + r(1, 2, 3, 4);
  return sum;
}
console.log(outer());
const o = { r(a, b, ...m) { const g = () => m; return g().length + (b ?? 0); } };
let t = 0;
for (let i = 0; i < 3000; i++) t += o.r(i) + o.r.call(o) + o.r.apply(o, [1]) + o.r(1, 2, 3);
console.log(t);
"#;

    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(
        quench.outcome(),
        RunResult::Pass,
        "Quench failed: {:?}",
        quench.worker
    );
    assert_eq!(quench.stdout, node.stdout, "Node and Quench output differ");
}

#[test]
fn deep_named_calls_raise_catchable_range_error_and_recover() {
    let source = r#"
function Box() {}
Box.prototype.walk = function (n, acc) {
  if (n === 0) return acc;
  return this.walk(n - 1, acc + 1);
};
let caught = false;
try {
  new Box().walk(__GUEST_DEPTH__, 0);
} catch (error) {
  caught = error instanceof RangeError && error.message === "Maximum call stack size exceeded";
}
if (!caught) throw new Error("deep call did not throw the stack RangeError");
if (new Box().walk(2, 0) !== 2) throw new Error("runtime did not recover after deep call");
"#;
    // Exercise the actual shared resource boundary, not a depth below its limit.
    let source = source.replace("__GUEST_DEPTH__", &(quench_stack::MAX_GUEST_CALL_DEPTH + 1).to_string());
    let observation = observe_source(&source);
    assert_eq!(
        observation.outcome(),
        RunResult::Pass,
        "deep recursion policy failed: {:?}",
        observation.worker
    );
}

#[test]
fn idle_task_combinator_finishes_on_the_shared_worker() {
    let observation = observe_source(include_str!("../../../tests/lanes/idle-task-run.js"));
    assert_eq!(
        observation.outcome(),
        RunResult::Pass,
        "idle task combinator failed: {:?}",
        observation.worker
    );
}

#[test]
fn webassembly_namespace_links_converts_and_shares_memory_like_node() {
    // (module (import "env" "log" (func (param i32))) (memory (export "memory") 1 10)
    //   (global (export "g") (mut i32) (i32.const 7)) and exports add, store, callLog, grow,
    //   i64add, fmul and trap over those resources.
    let source = r#"
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,36,7,96,1,127,0,96,2,127,127,1,127,96,2,127,127,0,96,1,127,1,127,96,2,126,126,1,126,96,2,124,124,1,124,96,0,0,2,11,1,3,101,110,118,3,108,111,103,0,0,3,8,7,1,2,0,3,4,5,6,5,4,1,1,1,10,6,6,1,127,1,65,7,11,7,68,9,6,109,101,109,111,114,121,2,0,1,103,3,0,3,97,100,100,0,1,5,115,116,111,114,101,0,2,7,99,97,108,108,76,111,103,0,3,4,103,114,111,119,0,4,6,105,54,52,97,100,100,0,5,4,102,109,117,108,0,6,4,116,114,97,112,0,7,10,53,7,7,0,32,0,32,1,106,11,9,0,32,0,32,1,54,2,0,11,6,0,32,0,16,0,11,6,0,32,0,64,0,11,7,0,32,0,32,1,124,11,7,0,32,0,32,1,162,11,3,0,0,11]);
const m = new WebAssembly.Module(bytes);
console.log(WebAssembly.validate(bytes), JSON.stringify(WebAssembly.Module.imports(m)));
const logged = [];
const e = new WebAssembly.Instance(m, { env: { log: x => logged.push(x) } }).exports;
console.log(Object.keys(e).join(), Object.isFrozen(e), Object.getPrototypeOf(e));
console.log(e.add(0x7fffffff, 1), e.add('4', 1.9), e.add.name, e.add.length);
e.store(16, 0x01020304);
console.log(Array.from(new Uint8Array(e.memory.buffer, 16, 4)).join());
e.callLog(42);
const old = e.memory.buffer;
console.log(logged.join(), e.grow(1), old.byteLength, e.memory.buffer.byteLength, e.memory.grow(1));
console.log(e.i64add(-1n, 0n), e.fmul(1.5, 2), e.g.value);
try { e.trap(); } catch (error) { console.log(error instanceof WebAssembly.RuntimeError); }
try { new WebAssembly.Instance(m, { env: {} }); } catch (error) { console.log(error.name); }
const table = new WebAssembly.Table({ element: 'anyfunc', initial: 1 });
table.set(0, e.add);
console.log(table.get(0) === e.add, table.grow(1), table.get(1));
WebAssembly.instantiate(m, { env: { log() {} } })
  .then(instance => console.log(instance instanceof WebAssembly.Instance));
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

#[test]
fn webassembly_internal_references_reach_javascript_like_node() {
    // (module (func "i31" (result i31ref) (ref.i31 -5)) (func "wide" (result anyref)
    //   (ref.i31 0x7fffffff)) (func "nullAny" (result anyref)) (func "nullNone" (result nullref))
    //   (func "host" (param externref) (result anyref) (any.convert_extern))
    //   (func "pair" (result anyref i32) (ref.i31 7) 1) (global "g" anyref (ref.i31 3)))
    let source = r#"
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,23,5,96,0,1,108,96,0,1,110,96,0,1,113,96,1,111,1,110,96,0,2,110,127,3,7,6,0,1,1,2,3,4,6,8,1,110,0,65,3,251,28,11,7,53,7,3,105,51,49,0,0,4,119,105,100,101,0,1,7,110,117,108,108,65,110,121,0,2,8,110,117,108,108,78,111,110,101,0,3,4,104,111,115,116,0,4,4,112,97,105,114,0,5,1,103,3,0,10,45,6,6,0,65,123,251,28,11,10,0,65,255,255,255,255,7,251,28,11,4,0,208,110,11,4,0,208,113,11,6,0,32,0,251,26,11,8,0,65,7,251,28,65,1,11]);
const e = new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports;
const object = {};
console.log(e.i31(), e.wide(), e.nullAny(), e.nullNone(), JSON.stringify(e.pair()), e.g.value);
console.log(e.host(object) === object, e.host('s'), e.host(1.5), e.host(42), e.host(null));
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

#[test]
fn webassembly_gc_objects_are_opaque_exotic_objects_like_node() {
    // (module (type $s (struct (field i32))) (func "st" (result anyref) (struct.new $s 1))
    //   (func "back" (param externref) (result i32) (ref.test (ref $s) (any.convert_extern))))
    let source = r#"
const wasmModule = new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,1,14,3,95,1,127,0,96,0,1,110,96,1,111,1,127,3,3,2,1,2,7,13,2,2,115,116,0,0,4,98,97,99,107,0,1,10,19,2,7,0,65,1,251,0,0,11,9,0,32,0,251,26,251,20,0,11,0,11,4,110,97,109,101,4,4,1,0,1,115]));
const e = new WebAssembly.Instance(wasmModule).exports;
const r = e.st();
const out = [];
const t = (n, f) => { try { const v = f(); out.push(n + '=' + (typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v))); } catch (err) { out.push(n + '!' + err.constructor.name); } };
t('typeof', () => typeof r); t('keys', () => Object.keys(r)); t('x', () => r.x); t('proto', () => Object.getPrototypeOf(r));
t('set', () => { 'use strict'; r.x = 1; }); t('str', () => String(r)); t('plus', () => r + 1); t('in', () => 'x' in r);
t('ext', () => Object.isExtensible(r)); t('frozen', () => Object.isFrozen(r)); t('wrap', () => Object(r) === r); t('back', () => e.back(r));
t('def', () => Object.defineProperty(r, 'y', { value: 1 })); t('setproto', () => Object.setPrototypeOf(r, {})); t('rsetproto', () => Reflect.setPrototypeOf(r, null));
t('del', () => { 'use strict'; delete r.x; }); t('freeze', () => Object.freeze(r)); t('rset', () => Reflect.set(r, 'x', 1)); t('rpe', () => Reflect.preventExtensions(r));
t('gopd', () => Object.getOwnPropertyDescriptor(r, 'x')); t('spread', () => ({ ...r })); t('create', () => Object.getPrototypeOf(Object.create(r)) === r);
t('json', () => JSON.stringify([r])); t('sym', () => { 'use strict'; r[Symbol.iterator] = 1; });
console.log(out.join(' '));
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

#[test]
fn webassembly_values_enter_reference_types_like_node() {
    // Exports taking anyref, eqref, i31ref, structref, (ref null $s), (ref $s) and nullref;
    // anyref and (ref null $s) globals and tables; imports returning anyref and (ref null $s);
    // and the JS API's reference type names for WebAssembly.Global and WebAssembly.Table.
    let source = r#"
let ret = null;
const wasmModule = new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,1,59,12,95,1,127,0,95,1,126,0,96,0,1,110,96,0,1,99,0,96,1,110,1,110,96,1,109,1,127,96,1,108,1,127,96,1,107,1,127,96,1,99,0,1,127,96,1,100,0,1,127,96,1,113,1,127,96,0,1,127,2,22,2,3,101,110,118,3,103,101,116,0,2,3,101,110,118,4,103,101,116,115,0,3,3,12,11,4,5,6,7,8,9,10,2,2,2,11,4,8,2,110,0,2,99,0,0,2,6,12,2,110,1,208,110,11,99,0,1,208,0,11,7,86,15,3,97,110,121,0,2,2,101,113,0,3,3,105,51,49,0,4,2,115,116,0,5,1,115,0,6,3,115,110,110,0,7,4,110,111,110,101,0,8,2,109,107,0,9,3,109,107,116,0,10,3,105,109,112,0,11,4,105,109,112,115,0,12,1,103,3,0,2,103,115,3,1,3,116,97,98,1,0,4,116,97,98,115,1,1,10,70,11,4,0,32,0,11,7,0,32,0,251,20,108,11,6,0,32,0,251,29,11,5,0,32,0,209,11,5,0,32,0,209,11,4,0,65,1,11,4,0,65,1,11,7,0,65,1,251,0,0,11,7,0,66,1,251,0,1,11,4,0,16,0,11,5,0,16,1,209,11,0,28,4,110,97,109,101,1,12,2,0,3,103,101,116,1,4,103,101,116,115,4,7,2,0,1,115,1,1,116]));
const e = new WebAssembly.Instance(wasmModule, { env: { get: () => ret, gets: () => ret } }).exports;
const s = e.mk(), t = e.mkt(), o = {};
const show = x => x === null ? 'null' : typeof x === 'object' ? (x === o ? 'o' : x === s ? 's' : x === t ? 't' : 'obj') : typeof x + ':' + String(x);
const out = [];
const c = (n, f) => { try { out.push(n + '=' + show(f())); } catch (err) { out.push(n + '!' + err.constructor.name); } };
for (const [name, x] of [['null', null], ['undef', undefined], ['num', 5], ['big', 2 ** 30], ['frac', 1.5], ['o', o], ['s', s], ['t', t], ['bigint', 1n]]) {
  for (const f of ['any', 'eq', 'i31', 'st', 's', 'snn', 'none']) c(f + '(' + name + ')', () => e[f](x));
  c('g=' + name, () => { e.g.value = x; return e.g.value; });
  c('gs=' + name, () => { e.gs.value = x; return e.gs.value; });
  c('tab=' + name, () => { e.tab.set(0, x); return e.tab.get(0); });
  c('tabs=' + name, () => { e.tabs.set(0, x); return e.tabs.get(0); });
  c('imp=' + name, () => { ret = x; return e.imp(); });
  c('imps=' + name, () => { ret = x; return e.imps(); });
}
for (const ty of ['anyfunc', 'funcref', 'externref', 'anyref', 'eqref', 'i31ref', 'structref', 'arrayref', 'nullref', 'exnref']) {
  c('global:' + ty, () => { const g = new WebAssembly.Global({ value: ty, mutable: true }); g.value = 7; return g.value; });
  c('table:' + ty, () => { const table = new WebAssembly.Table({ element: ty, initial: 1 }); table.set(0, 7); return table.get(0); });
}
console.log(out.join(' '));
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

#[test]
fn non_callable_calls_name_their_callee_like_node() {
    // V8's CallPrinter: names, property chains, computed keys, sequences, operators,
    // literals, nested calls and constructions, optional chains and private fields.
    let source = r#"
'use strict';
const o = { a: { b: 2, 1: 3 }, n: null, s: 'str', arr: [1] };
const value = { valueOf: 1 };
let x = 5, k = 'b', f = () => 1, u;
class C { #p = 1; m() { return this.#p(); } static s() { return super.toString.call(1)(); } }
const cases = [
  () => value.valueOf(), () => o.a.b(), () => o.a['b'](), () => o.a[1](), () => o.a[k](), () => (0, value.valueOf)(),
  () => x(), () => u(), () => o.s(), () => o.n(), () => o.zz(), () => o?.a.b(), () => o.a?.b(), () => o.a.b?.(),
  () => f()(), () => (x || o.a.b)(), () => (x && 3)(), () => (x + 1)(), () => (-x)(), () => (typeof x)(), () => (void 0)(),
  () => (x++)(), () => (x ? 1 : 2)(), () => (x = 3)(), () => [1, 2]['length'](), () => [](), () => ({}).a(), () => ({ a: 1, b: 2 })(),
  () => 'abc'(), () => 'abc'.length(), () => (1)(), () => /re/g.lastIndex(), () => new C().m(), () => this(),
  () => new o.a.b(), () => new x(), () => new (f())(), () => new o.a['b'](), () => new f(),
  () => (function () {})()(), () => (() => 1)()(), () => o.arr[0](), () => new Date().getTime()(), () => `t`(), () => (null)(), () => (true)(),
  () => o.a
    .b(), () => [...o.arr].x(), () => JSON.parse('1')(), () => (x, k, o.a.b)(), () => (x == 1)(), () => (x in o)(), () => (x instanceof Object)(),
];
for (const c of cases) { try { c(); console.log('no error'); } catch (e) { console.log(e.constructor.name + ': ' + e.message); } }
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

#[test]
fn vm_context_lexicals_survive_collection_and_prototypes_end_in_the_context() {
    // A context's script-scope `let` is reachable only through the realm while its code
    // runs; collections inside that code must keep it alive.
    let source = r#"
const vm = require('vm');
const context = vm.createContext({});
let source = 'let kept = [';
for (let i = 0; i < 50; i++) source += `{"id":${i},"params":{"a":[1,"x${i}",{"b":${i}}]}},`;
source += '{"z":1}];';
vm.runInContext(source, context);
console.log(vm.runInContext(`
  let total = 0;
  for (let round = 0; round < 20; round++) {
    const garbage = [];
    for (let i = 0; i < 100000; i++) garbage.push({ i });
    total += JSON.stringify(kept).length;
  }
  total`, context));
// Every chain in the context ends at its own %Object.prototype%.
console.log(vm.runInContext('Object.getPrototypeOf(Object.prototype) === null', context));
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

#[test]
fn promise_resolution_with_promises_keeps_job_order_and_species_reads() {
    // Resolving a promise with an intrinsic promise takes one extra job; the species lookup
    // `then` performs stays observable (getter order and throws), as in Node.
    let source = r#"
const log = [];
const a = Promise.resolve(1);
new Promise(r => r(a)).then(v => log.push('b' + v));
Promise.resolve().then(() => log.push('t1')).then(() => log.push('t2')).then(() => log.push('t3'));
class P extends Promise {}
new Promise(r => r(P.resolve(2))).then(v => log.push('c' + v));
const d = Promise.resolve(3);
Object.defineProperty(d, 'constructor', { get() { log.push('ctor-get'); return Promise; } });
new Promise(r => r(d)).then(v => log.push('e' + v));
new Promise(r => r(Promise.reject(new Error('x')))).catch(error => log.push('g:' + error.message));
const h = Promise.resolve(4);
Object.defineProperty(h, 'constructor', { get() { throw new Error('boom'); } });
new Promise(r => r(h)).catch(error => log.push('h:' + error.message));
setTimeout(() => console.log(log.join(' ')), 0);
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}
