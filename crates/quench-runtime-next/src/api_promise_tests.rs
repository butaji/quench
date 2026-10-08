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
fn async_functions_wrap_return_and_throw_in_promises() {
    let host = Capture::default();
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
    assert_eq!(view.0.borrow().as_slice(), ["7", "8"]);
}
