use crate::{Engine, ExecutionMode, JitMode};
use crate::test_host::TestHost;

/// What a program prints, run in the interpreter and under `--b-jit`, and the runs of lane bodies
/// the second made (all, and those that completed their activation).
fn run_both(source: &str) -> (Vec<String>, Vec<String>, [usize; 2]) {
    let program = Engine::specialize(source, "base-body.js").unwrap();
    let printed = |mode: ExecutionMode| {
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = crate::vm::Vm::new(TestHost::writing_to(output.clone()));
        vm.set_execution_mode(mode);
        vm.programs.enable_js_lane_methods();
        vm.execute(&program).unwrap();
        output.take()
    };
    let interpreted = printed(ExecutionMode::Interpreted);
    super::super::LANE_BODY_RUNS.with(|runs| runs.set([0; 2]));
    let compiled = crate::jit_mode::with_mode(JitMode::BaseOnly, || printed(ExecutionMode::Compiled));
    (interpreted, compiled, super::super::LANE_BODY_RUNS.with(std::cell::Cell::get))
}

/// A loop function entered many times runs as a whole-function body, which completes its
/// activations, and prints what the interpreter does.
#[test]
fn a_loop_function_runs_as_a_body() {
    let (interpreted, compiled, [runs, returns]) = run_both(
        "function f(a, b) { let s = 0; for (let i = 0; i < b; i++) s = s + a * i; return s; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + f(k, 20)) | 0;
         print(t); print(f(3, 5)); print(f(0.5, 8)); print(f(2147483647, 3)); print(f('3', 4));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100 && returns > 100, "the body ran {runs} times and completed {returns}");
}

/// An instruction with no row in the middle of the function leaves the body at its PC, and the
/// interpreter resumes there with the registers the rows computed.
#[test]
fn an_uncovered_instruction_in_the_middle_leaves_and_resumes() {
    let (interpreted, compiled, [runs, returns]) = run_both(
        "function g(a, b) { let s = 0; for (let i = 0; i < b; i++) { s = s + a * i; } const m = Math.floor(s / 3); let r = m; for (let j = 0; j < 4; j++) r = r + j * a; return r + a; }
         let t = 0; for (let k = 0; k < 300; k++) t = (t + g(k, 9)) | 0;
         print(t); print(g(7, 11)); print(g(-5, 4)); print(g(1.5, 6));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100 && returns < runs, "the body left {} of {runs} runs", runs - returns);
}

/// Recursion: each activation is its own body run or interpreter run, and the results agree.
#[test]
fn recursion_prints_what_the_interpreter_prints() {
    let (interpreted, compiled, _) = run_both(
        "function fib(n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); }
         print(fib(22)); print(fib(5)); print(fib(1.5));",
    );
    assert_eq!(interpreted, compiled);
}
