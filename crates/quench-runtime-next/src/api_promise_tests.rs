use super::{Engine, ExecutionRequest, Runtime};
use crate::Host;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct Capture(Rc<RefCell<Vec<String>>>);

impl Host for Capture {
    fn write_line(&mut self, text: &str) {
        self.0.borrow_mut().push(text.into());
    }

    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

#[test]
fn promises_use_the_vm_job_queue_for_reactions_and_chains() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "var original = Promise.resolve(1); print(Promise.resolve(original) === original); original.then(function(value) { return value + 1; }).then(function(value) { print(value); }); Promise.reject(7).catch(function(value) { print(value); });",
            "promise-jobs.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["true", "7", "2"]);
}

#[test]
fn promise_resolution_assimilates_thenables_and_rejects_self_resolution() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "Promise.resolve({ then: function(resolve) { resolve(9); } }).then(print); var resolve; var promise = new Promise(function(r) { resolve = r; }); resolve(promise); promise.catch(function() { print('self'); });",
            "promise-resolution.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["self", "9"]);
}

#[test]
fn promise_finally_runs_in_order_and_preserves_settlement() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "Promise.resolve(3).finally(function() { print('cleanup'); }).then(print); Promise.reject(4).finally(function() { print('reject-cleanup'); }).catch(print);",
            "promise-finally.js",
        ))
        .unwrap();
    assert_eq!(
        view.0.borrow().as_slice(),
        ["cleanup", "reject-cleanup", "3", "4"]
    );
}

#[test]
fn promise_all_and_race_use_rooted_aggregate_jobs() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "Promise.all([Promise.resolve(1), 2]).then(function(values) { print(values.join(',')); }); Promise.race([Promise.reject(3), Promise.resolve(4)]).catch(print);",
            "promise-aggregates.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["1,2", "3"]);
}

#[test]
fn promise_finally_waits_for_cleanup_promise() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "var resolveCleanup; var cleanup = new Promise(function(resolve) { resolveCleanup = resolve; }); Promise.resolve(1).finally(function() { return cleanup; }).then(print); Promise.resolve(2).finally(function() { return Promise.reject('cleanup'); }).catch(print); print('before'); resolveCleanup(9);",
            "promise-finally-adoption.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["before", "1", "cleanup"]);
}

#[test]
fn promise_all_settled_preserves_indexed_outcomes() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "Promise.allSettled([Promise.resolve(1), Promise.reject(2)]).then(function(values) { print(values[0].status); print(values[0].value); print(values[1].status); print(values[1].reason); });",
            "promise-all-settled.js",
        ))
        .unwrap();
    assert_eq!(
        view.0.borrow().as_slice(),
        ["fulfilled", "1", "rejected", "2"]
    );
}

#[test]
fn promise_any_short_circuits_and_reports_indexed_rejections() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "Promise.any([Promise.reject(1), Promise.resolve(2)]).then(print); Promise.any([Promise.reject(3), Promise.reject(4)]).catch(function(error) { print(error.name); print(error.errors.join(',')); });",
            "promise-any.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["2", "AggregateError", "3,4"]);
}

#[test]
fn promise_combinators_consume_shared_iterators() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "Promise.all(new Set([1, 2])).then(function(values) { print(values.join(',')); }); Promise.race('ab').then(print);",
            "promise-iterables.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["1,2", "a"]);
}

#[test]
fn async_functions_wrap_return_and_throw_in_promises() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    let program = Engine::compile(ExecutionRequest::script(
        "async function value() { return 7; } async function fail() { throw 8; } value().then(print); fail().catch(print);",
        "async-functions.js",
    ))
    .unwrap();
    let path = std::env::temp_dir().join(format!("rqj-async-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    runtime.execute(&decoded).unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["7", "8"]);
}

#[test]
fn async_await_resumes_through_the_promise_job_queue() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "async function value() { return await 7; } value().then(print); print('sync');",
            "async-await.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["sync", "7"]);
}

#[test]
fn async_await_rejection_enters_catch() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "async function value() { try { await Promise.reject(3); } catch (error) { return error + 1; } } value().then(print);",
            "async-await-catch.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["4"]);
}

#[test]
fn async_await_waits_for_pending_promises_and_propagates_rejection() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "var resolve; var pending = new Promise(function(next) { resolve = next; }); async function value() { return await pending; } value().then(print); print('before'); resolve(9); async function fail() { return await Promise.reject(6); } fail().catch(print);",
            "async-await-pending.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["before", "9", "6"]);
}

#[test]
fn generators_share_continuations_with_iterator_results() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "function* values() { var received = yield 1; yield received + 1; return 9; } function* sequence() { yield 2; yield 4; } var iterator = values(); var first = iterator.next(); print(first.value); var second = iterator.next(4); print(second.value); var done = iterator.next(); print(done.value); print(done.done); print(iterator.next().done); print(typeof iterator[Symbol.iterator]); var sum = 0; for (var value of sequence()) { sum = sum + value; } print(sum);",
            "generators.js",
        ))
        .unwrap();
    assert_eq!(
        view.0.borrow().as_slice(),
        ["1", "5", "9", "true", "true", "function", "6"]
    );
}

#[test]
fn async_for_of_uses_the_shared_await_continuation() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "async function sum() { var result = 0; for await (var value of [1, 2, 3]) { result = result + value; } return result; } sum().then(print);",
            "async-for-of.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["6"]);
}

#[test]
fn async_for_of_prefers_async_iterator_and_wraps_sync_iterators() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "var source = { index: 0, next: function() { var index = this.index; this.index = index + 1; return Promise.resolve({ value: index + 4, done: index > 1 }); }, [Symbol.asyncIterator]: function() { return this; } }; async function sum() { var result = 0; for await (var value of source) { result = result + value; } for await (var item of [1, 2]) { result = result + item; } return result; } sum().then(print);",
            "async-iterator.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["12"]);
}

#[test]
fn async_generator_uses_the_async_iterator_protocol() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "async function* values() { yield await Promise.resolve(2); yield 3; } async function sum() { var result = 0; for await (var value of values()) { result = result + value; } return result; } sum().then(print);",
            "async-generator.js",
        ))
        .unwrap();
    assert_eq!(view.0.borrow().as_slice(), ["5"]);
}

#[test]
fn generator_return_and_throw_are_state_transitions() {
    let host = Capture::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    runtime
        .compile_and_execute(ExecutionRequest::script(
            "function* values() { try { yield 1; } catch (error) { yield error; } } var iterator = values(); print(iterator.next().value); print(iterator.throw('x').value); print(iterator.next().done); var returned = values(); print(returned.return(9).value); print(returned.next().done); async function* asyncValues() { yield 2; } var asyncIterator = asyncValues(); asyncIterator.next().then(function(step) { print(step.value); return asyncIterator.return(8); }).then(function(step) { print(step.value); });",
            "generator-control.js",
        ))
        .unwrap();
    assert_eq!(
        view.0.borrow().as_slice(),
        ["1", "x", "true", "9", "true", "2", "8"]
    );
}
