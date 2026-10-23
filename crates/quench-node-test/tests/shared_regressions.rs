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
