use super::{ExecutionRequest, Runtime};
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
