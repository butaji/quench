//! WebAssembly JS-API regressions, each compared with Node.

mod support;

use quench_node_test::case_process::RunResult;
use support::{observe_node_source, observe_source};

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

/// Exported, constructed and `JSTag` tags are all `WebAssembly.Tag` objects: .NET's runtime
/// asserts that of its module's exported `__cpp_exception` tag.
#[test]
fn webassembly_tags_are_tag_objects_like_node() {
    // (module (tag (export "t") (param i32)))
    let source = r#"
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,5,1,96,1,127,0,13,3,1,0,0,7,5,1,1,116,4,0]);
const exported = new WebAssembly.Instance(new WebAssembly.Module(bytes), {}).exports.t;
for (const tag of [exported, new WebAssembly.Tag({ parameters: ['i32'] }), WebAssembly.JSTag]) {
  console.log(tag instanceof WebAssembly.Tag, Object.getPrototypeOf(tag) === WebAssembly.Tag.prototype, String(tag));
}
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

/// A Wasm export called from a lane's frameless activations runs beside them
/// (`Native::runs_beside_activations`): its results, the stack its JavaScript import observes
/// (the activations, then frames, in order), and its trap are a frame-stack call's. The
/// trap's `RuntimeError` names the JavaScript callers it left, not the frame that caught it.
#[test]
fn wasm_export_calls_beside_frameless_activations_report_like_node() {
    let source = r#"
const section = (id, bytes) => [id, bytes.length, ...bytes];
const name = (s) => [s.length, ...[...s].map((c) => c.charCodeAt(0))];
const types = [2, 0x60, 0, 0, 0x60, 1, 0x7f, 1, 0x7f];
const imports = [1, ...name('env'), ...name('cb'), 0, 0];
const funcs = [1, 1];
const exps = [1, ...name('run'), 0, 1];
const run = [0x20, 0, 0x45, 0x04, 0x40, 0x00, 0x0b,
  0x20, 0, 0x41, 1, 0x46, 0x04, 0x40, 0x10, 0, 0x0b,
  0x20, 0, 0x41, 1, 0x6a, 0x0b];
const code = [1, run.length + 1, 0, ...run];
const bytes = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, ...section(1, types), ...section(2, imports),
  ...section(3, funcs), ...section(7, exps), ...section(10, code)]);
const callers = (stack) => stack.split('\n').map((line) => /at (\w+) /.exec(line)?.[1])
  .filter((name) => ['cb', 'inner', 'middle', 'outer'].includes(name)).join(' < ');
const wasm = new WebAssembly.Instance(new WebAssembly.Module(bytes), {
  env: { cb() { console.log('import sees', callers(new Error('cb').stack)); } },
}).exports;
function inner(x) { return wasm.run(x); }
function middle(x) { return inner(x) + 0; }
function outer(n) { let sum = 0; for (let i = 2; i < n; i++) sum += middle(i); return sum; }
console.log(outer(3000), outer(3000));
function calls(x) { return middle(x); }
calls(1);
try { calls(0); } catch (e) { console.log(e.constructor.name, e.message, callers(e.stack)); }
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}

/// A shared memory's plain loads and stores address its bytes in place, in the lane and in
/// compiled code, before and after the memory grows, and agree with its JavaScript views and
/// atomics.
#[test]
fn shared_memory_accesses_agree_with_views_across_growth_like_node() {
    // (module (import "env" "mem" (memory 1 4 shared))
    //   (func (export "sum") (param i32) (result i32)) sums the first n i32s,
    //   (func (export "put") (param i32 i32)) and (func (export "get") (param i32) (result i32)).
    let source = r#"
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,11,2,96,1,127,1,127,96,2,127,127,0,2,13,1,3,101,110,118,3,109,101,109,2,3,1,4,3,4,3,0,1,0,7,19,3,3,115,117,109,0,0,3,112,117,116,0,1,3,103,101,116,0,2,10,54,3,34,1,2,127,3,64,32,2,32,1,65,2,116,40,2,0,106,33,2,32,1,65,1,106,34,1,32,0,73,13,0,11,32,2,11,9,0,32,0,32,1,54,2,0,11,7,0,32,0,40,2,0,11]);
const memory = new WebAssembly.Memory({ initial: 1, maximum: 4, shared: true });
const e = new WebAssembly.Instance(new WebAssembly.Module(bytes), { env: { mem: memory } }).exports;
const view = new Int32Array(memory.buffer);
for (let i = 0; i < 1000; i++) view[i] = i;
let total = 0;
for (let k = 0; k < 300; k++) total = e.sum(1000);
e.put(8, 77);
console.log(total, view[2], e.get(8));
memory.grow(1);
const grown = new Int32Array(memory.buffer);
e.put(65540, 5);
console.log(grown[16385], e.get(65540), memory.buffer.byteLength, view.length);
try { e.get(131072); } catch (error) { console.log(error instanceof WebAssembly.RuntimeError); }
console.log(Atomics.add(grown, 2, 1), e.get(8));
for (let k = 0; k < 300; k++) total = e.sum(1000);
console.log(total);
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}
