//! Generators and async functions suspend and resume identically in the interpreter and
//! under the JIT (`fixtures/suspended-frames.js`).

use quench_runtime::{Engine, ExecutionMode, Runtime, SystemHost};

const SOURCE: &str = include_str!("fixtures/suspended-frames.js");

fn run(mode: ExecutionMode) -> String {
    let outcome = Engine::specialize(SOURCE, "suspended-frames.js")
        .map_err(|errors| format!("{errors:?}"))
        .and_then(|program| {
            let mut runtime = Runtime::new(SystemHost);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).map_err(|error| error.to_string())
        });
    format!("{outcome:?}")
}

#[test]
fn suspended_frames_end_as_the_interpreter_does() {
    let interpreted = run(ExecutionMode::Interpreted);
    assert_eq!(interpreted, run(ExecutionMode::Compiled), "{interpreted}");
}

// Links the runtime layer crates (quench-layers).
extern crate quench_layers;
