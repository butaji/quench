#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::{Engine, ExecutionMode};

/// What a program printed in the interpreter and as compiled, where a loop of a function entered once runs as a
/// loop region.
fn run_regions(source: &str) -> (Vec<String>, Vec<String>) {
    let program = Engine::specialize(source, "base-region.js").unwrap();
    let printed = |mode: ExecutionMode| {
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        vm.set_execution_mode(mode);
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        output.take()
    };
    (printed(ExecutionMode::Interpreted), printed(ExecutionMode::Compiled))
}

/// A hot loop that calls a small function by its global name: the call site inlines the callee (its
/// dead implicit `return undefined` dropped, its constants folded), whose window starts past the frame's
/// environment window, and the region runs with no leave but where the loop ends.
#[test]
fn a_hot_loop_calling_a_small_function_by_name_runs_as_a_region_with_the_callee_inlined() {
    let (interpreted, compiled) = run_regions(
        "function sq(x) { return x * x; }
         function run(n) { var s = 0; for (var i = 0; i < n; i++) s = (s + sq(i & 7)) | 0; return s; }
         print(run(40000)); print(run(10));",
    );
    assert_eq!(interpreted, compiled);
}

/// A guard that fails inside the inlined callee of a region (an int32 overflow after some iterations
/// of the callee's own loop) leaves inside the callee: the lane makes its activation from the registers
/// the region held for it, the interpreter finishes the callee and the loop, and the output is the
/// interpreter's.
#[test]
fn a_guard_failing_inside_an_inlined_callee_resumes_in_the_callee() {
    let (interpreted, compiled) = run_regions(
        "function walk(n, x) { var s = 0, i = 0; while (i < n) { s = s + x; i = i + 1; } return s; }
         function run(m) { var t = 0; for (var j = 0; j < m; j++) { t = (t + walk(4, j < 30000 ? 3 : 1000000000)) | 0; } return t; }
         print(run(32000));",
    );
    assert_eq!(interpreted, compiled);
}

/// A callee too large or too varied to inline, whose body is published, would be called by its entry
/// from the region; a call in a loop region costs more than the interpreter's call today, so the
/// region is not built (`function::interprets`) and the output is the interpreter's. Once the region
/// call protocol is cheaper than the interpreter's call, the region runs and lends its frame.
#[test]
fn a_loop_calling_a_published_body_is_not_a_region_while_a_region_call_costs_more_than_the_interpreter() {
    let (interpreted, compiled) = run_regions(
        "function work(a) { var s = 0; for (var k = 0; k < 4; k++) s = s + Math.floor((a + k) / 3); return s; }
         function warm(f) { var t = 0; for (var i = 0; i < 2000; i++) t = (t + f(i)) | 0; return t; }
         function run(n, f) { var s = 0; for (var i = 0; i < n; i++) s = (s + f(i)) | 0; return s; }
         print(warm(work)); print(run(40000, work)); print(run(30, work));",
    );
    assert_eq!(interpreted, compiled);
}
