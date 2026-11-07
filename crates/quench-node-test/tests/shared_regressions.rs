//! Complex guest regressions not owned by the upstream conformance suites.

mod support;

use quench_node_test::case_process::RunResult;
use support::{observe_node_source, observe_source};

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
fn server_request_start_publishes_and_emits_like_node() {
    // A parsed server request enters JavaScript once: it makes the request and response,
    // publishes `http.server.request.start` to a subscriber snapshot with Node's message
    // order and the channel name, then emits `request` on the server.
    let source = r#"
const dc = require('diagnostics_channel');
const http = require('http');
const seen = [];
let starts = 0;
function start(message, name) {
  seen.push(['start', Object.keys(message).join(), name, this === undefined || this === globalThis,
    message.request instanceof http.IncomingMessage, message.response.req === message.request,
    message.server === server]);
  if (++starts === 2) dc.unsubscribe('http.server.request.start', start);
}
dc.subscribe('http.server.request.start', start);
const server = http.createServer(function (request, response) {
  seen.push(['request', this === server, arguments.length, request.method, request.url,
    response.statusCode, response.req === request]);
  response.end('ok');
});
server.listen(0, async () => {
  const { port } = server.address();
  for (const path of ['/a', '/b?c=1', '/d']) {
    await new Promise((done) => http.get({ port, path }, (response) => {
      response.resume();
      response.on('end', done);
    }));
  }
  console.log(JSON.stringify(seen));
  server.close();
});
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
fn diagnostics_subscriber_throws_are_uncaught_on_the_next_tick_like_node() {
    // A subscriber's throw neither ends `publish` nor the server's request start: Node
    // reports it as an uncaught exception on the next tick and runs the other subscribers.
    let source = r#"
const http = require('http');
const dc = require('diagnostics_channel');
const seen = [];
process.on('uncaughtException', (e) => { seen.push('uncaught ' + e.message); });
dc.subscribe('http.server.request.start', () => { seen.push('sub1'); throw new Error('sub1'); });
dc.subscribe('http.server.request.start', () => { seen.push('sub2'); });
const ch = dc.channel('custom');
ch.subscribe(() => { seen.push('c1'); throw new Error('c1'); });
ch.subscribe(() => { seen.push('c2'); });
ch.publish({}); seen.push('after publish');
const srv = http.createServer((req, res) => { seen.push('request'); res.end('x'); });
srv.listen(0, () => { http.get({ port: srv.address().port }, (r) => { r.resume(); r.on('end', () => { seen.push('done ' + r.statusCode); setImmediate(() => { console.log(JSON.stringify(seen)); srv.close(); }); }); }); });
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
    let source = source.replace(
        "__GUEST_DEPTH__",
        &(quench_stack::MAX_GUEST_CALL_DEPTH + 1).to_string(),
    );
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

/// Promise jobs that capture no store and no resource run in a fresh, empty context: job order
/// across `then`, `await` and `queueMicrotask`, execution ids, thrown handlers, unhandled
/// rejections, and an `enterWith` inside a job, which reaches the job's own continuations and
/// no later job, as Node's do.
#[test]
fn promise_jobs_isolate_async_local_storage_and_keep_order_like_node() {
    let source = r#"
const { AsyncLocalStorage, executionAsyncId, triggerAsyncId } = require('async_hooks');
const out = [];
const ids = () => executionAsyncId() + '/' + triggerAsyncId();
process.on('unhandledRejection', (reason) => out.push('unhandled ' + (reason && reason.message)));
const p = Promise.resolve(1);
p.then(() => out.push('t1 ' + ids()));
queueMicrotask(() => out.push('q1'));
(async () => { out.push('a0'); await null; out.push('a1 ' + ids()); await p; out.push('a2'); })();
p.then(() => { out.push('t2'); throw new Error('thrown'); }).catch((e) => out.push('caught ' + e.message));
p.then(() => { throw new Error('nobody'); });
Promise.reject(new Error('rejected')).then(() => out.push('never'));
let als;
p.then(() => {
  als = new AsyncLocalStorage();
  out.push('e0 ' + als.getStore());
  als.enterWith('E');
  out.push('e1 ' + als.getStore());
  Promise.resolve().then(() => out.push('e-inner ' + als.getStore()));
});
p.then(() => out.push('after-e ' + als.getStore()));
queueMicrotask(() => { out.push('q2 ' + als.getStore()); als.enterWith('Q'); });
p.then(() => out.push('after-q ' + als.getStore()));
p.then(() => als.run('R', () => p.then(() => { out.push('run ' + als.getStore()); als.enterWith('R2'); }).then(() => out.push('run2 ' + als.getStore()))));
p.then(() => out.push('last ' + als.getStore()));
setTimeout(() => { out.push('timer ' + als.getStore()); console.log(out.join('\n')); }, 10);
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "Quench failed: {:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout, "Node and Quench output differ");
}

#[test]
fn bigint_conversions_and_wasm_export_calls_report_like_node() {
    // ToBigInt and the BigInt constructor name the value they reject as V8 does, and a call
    // of a Wasm export converts its arguments in order, once each, through the same
    // conversions (i64 arguments are BigInts).
    let source = r#"
const cases = [[], [1.5], [NaN], [Infinity], [undefined], [null], [Symbol('s')], [Symbol()], ['x'], ['1.5'], [{}], [{ valueOf() { return 2.5; } }], [1e21], [-0], ['  12  ']];
for (const args of cases)
  try { console.log('ok', String(BigInt(...args))); } catch (e) { console.log(e.constructor.name + ': ' + e.message); }
for (const v of [{ [Symbol.toPrimitive]() { return 1; } }, 1n, '0x10', '', 'abc', 12.0])
  try { console.log('ok', String(BigInt.asUintN(8, v))); } catch (e) { console.log(e.constructor.name + ': ' + e.message); }
try { 1n + 1; } catch (e) { console.log(e.constructor.name + ': ' + e.message); }
// Export-call semantics: coercion order and side effects, f64, i64 <-> BigInt, missing
// arguments, multi-value results, errors.
const section = (id, bytes) => [id, bytes.length, ...bytes];
const types = [3,
  0x60, 2, 0x7f, 0x7c, 1, 0x7c,   // (i32, f64) -> f64
  0x60, 1, 0x7e, 1, 0x7e,         // (i64) -> i64
  0x60, 0, 2, 0x7f, 0x7f];        // () -> (i32, i32)
const funcs = [3, 0, 1, 2];
const name = (s) => [s.length, ...[...s].map((c) => c.charCodeAt(0))];
const exps = [3, ...name("mix"), 0, 0, ...name("inc64"), 0, 1, ...name("pair"), 0, 2];
const body = (code) => [code.length + 1, 0, ...code];
const code = [3,
  ...body([0x20, 0, 0xb7, 0x20, 1, 0xa0, 0x0b]),
  ...body([0x20, 0, 0x42, 1, 0x7c, 0x0b]),
  ...body([0x41, 1, 0x41, 2, 0x0b])];
const bytes = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0,
  ...section(1, types), ...section(3, funcs), ...section(7, exps), ...section(10, code)]);
const { mix, inc64, pair } = new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports;
const log = [];
const tracked = (label, value) => ({ valueOf() { log.push(label); return value; } });
const show = (f) => { try { return String(f()); } catch (e) { return e.constructor.name + ': ' + e.message; } };
console.log(show(() => mix(tracked('a', 2.9), tracked('b', 0.25))), log.join(','));
console.log(show(() => mix()), show(() => mix(-1, 1.5, 'extra')), show(() => mix(2 ** 32 + 5, NaN)));
console.log(show(() => inc64(41n)), show(() => inc64(2n ** 63n - 1n)), show(() => inc64(1)));
console.log(show(() => JSON.stringify(pair())), mix.length, inc64.length, pair.length, mix.name);
console.log(show(() => mix({ valueOf() { throw new RangeError('boom'); } })));
"#;
    let node = observe_node_source(source);
    assert!(node.status.success(), "Node oracle failed: {node:?}");
    let quench = observe_source(source);
    assert_eq!(quench.outcome(), RunResult::Pass, "{:?}", quench.worker);
    assert_eq!(quench.stdout, node.stdout);
}
