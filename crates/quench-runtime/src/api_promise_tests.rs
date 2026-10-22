use super::{Engine, ExecutionRequest, Runtime};
use crate::Host;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};



#[test]
fn async_functions_wrap_return_and_throw_in_promises() {
    let host = TestHost::default();
    let view = host.clone();
    let mut runtime = Runtime::new(host);
    let program = Engine::compile(ExecutionRequest::script(
        "async function value() { return 7; } async function fail() { throw 8; } value().then(print); fail().catch(print);",
        "async-functions.js",
    ))
    .unwrap();
    let path = std::env::temp_dir().join(format!("quench-async-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    runtime.execute(&decoded).unwrap();
    assert_eq!(view.lines().as_slice(), ["7", "8"]);
}

/// A host that gives every promise a context and records which are still held.
#[derive(Clone, Default)]
struct PromiseContexts {
    next: Rc<std::cell::Cell<u64>>,
    held: Rc<RefCell<std::collections::BTreeSet<u64>>>,
}

impl Host for PromiseContexts {
    fn write_line(&mut self, _text: &str) {}

    fn clock_millis(&mut self) -> f64 {
        0.0
    }

    fn observes_promise_lifecycle(&self) -> bool {
        true
    }

    fn create_promise_context(
        &mut self,
        _parent: Option<crate::HostExecutionContext>,
    ) -> Option<crate::HostExecutionContext> {
        let context = self.next.get() + 1;
        self.next.set(context);
        self.held.borrow_mut().insert(context);
        Some(crate::HostExecutionContext(context))
    }

    fn release_promise_contexts(&mut self, contexts: &[crate::HostExecutionContext]) {
        let mut held = self.held.borrow_mut();
        for context in contexts {
            assert!(held.remove(&context.0), "context {} released once", context.0);
        }
    }
}

#[test]
fn collected_promises_release_their_host_contexts() {
    let host = PromiseContexts::default();
    let (created, held) = (host.next.clone(), host.held.clone());
    let mut runtime = Runtime::new(host);
    let program = Engine::compile(ExecutionRequest::script(
        "globalThis.kept = new Promise(() => {}); for (let i = 0; i < 1000; i++) new Promise(() => {});",
        "promise-contexts.js",
    ))
    .unwrap();
    runtime.execute(&program).unwrap();
    assert!(created.get() > 1000);
    runtime.collect(&program).unwrap();
    // Only the promise the global keeps holds its context.
    assert_eq!(held.borrow().len(), 1);
}
