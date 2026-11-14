use crate::vm::tier_events::{Event, TierEvents};
use crate::test_host::TestHost;
use crate::{Engine, ExecutionMode, JitMode};

/// What a program printed in the interpreter and as BaseJIT loop regions (`--b-jit`), the runs of
/// lane-ABI regions the second made (all, and what they did: `Event::OsrLent` and its kin) and the call sites the
/// stitch inlined meanwhile.
struct Regions {
    interpreted: Vec<String>,
    compiled: Vec<String>,
    runs: u64,
    kinds: TierEvents,
    inlined: u64,
}

/// Runs `source` in both modes; a loop of a function entered once runs as a region.
fn run_regions(source: &str) -> Regions {
    let program = Engine::specialize(source, "base-region.js").unwrap();
    let printed = |mode: ExecutionMode| {
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = crate::vm::Vm::new(TestHost::writing_to(output.clone()));
        vm.set_execution_mode(mode);
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        vm.execute(&program).unwrap();
        (output.take(), vm.tier_events().clone())
    };
    let (interpreted, _) = printed(ExecutionMode::Interpreted);
    let before = super::inlined_sites();
    let (compiled, kinds) = crate::jit_mode::with_mode(JitMode::BaseOnly, || printed(ExecutionMode::Compiled));
    Regions { interpreted, compiled, runs: kinds.count(Event::OsrRun), kinds, inlined: super::inlined_sites() - before }
}

/// A hot loop that calls a small function runs the callee's rows inside its region: the call site is
/// inlined, the region runs, and no run of it leaves anywhere but where the loop ends.
#[test]
fn a_hot_loop_calling_a_small_function_runs_inlined_with_no_leaves() {
    let ran = run_regions(
        "function sq(x) { return x * x; }
         function run(n) { var s = 0; for (var i = 0; i < n; i++) s = (s + sq(i & 7)) | 0; return s; }
         print(run(40000)); print(run(10));",
    );
    assert_eq!(ran.interpreted, ran.compiled);
    assert!(ran.runs > 0, "the region ran");
    assert!(ran.inlined > 0, "the callee's site was inlined into the region");
    assert_eq!(ran.kinds.count(Event::OsrExited), 0, "no run left the loop but where it ended: {:?}", ran.kinds);
}

/// A guard that fails inside the inlined callee of a region (an int32 overflow after some iterations
/// of the callee's own loop) leaves inside the callee: the lane makes its activation from the registers
/// the region held for it, the interpreter finishes the callee and the loop, and the output is the
/// interpreter's.
#[test]
fn a_guard_failing_inside_an_inlined_callee_resumes_in_the_callee() {
    let ran = run_regions(
        "function walk(n, x) { var s = 0, i = 0; while (i < n) { s = s + x; i = i + 1; } return s; }
         function run(m) { var t = 0; for (var j = 0; j < m; j++) { t = (t + walk(4, j < 30000 ? 3 : 1000000000)) | 0; } return t; }
         print(run(32000));",
    );
    assert_eq!(ran.interpreted, ran.compiled);
    assert!(ran.inlined > 0, "the callee's site was inlined into the region");
    assert!(ran.kinds.count(Event::OsrInlined) > 0, "a run left inside the inlined callee: {:?}", ran.kinds);
}

/// A callee too large or too varied to inline, whose body is published, is called by its entry from
/// the region: the region runs on the lane frame's payload, and prints what the interpreter prints.
#[test]
fn a_callee_with_a_published_body_is_called_by_its_entry_from_a_region() {
    let ran = run_regions(
        "function work(a) { var s = 0; for (var k = 0; k < 4; k++) s = s + Math.floor((a + k) / 3); return s; }
         function warm() { var t = 0; for (var i = 0; i < 2000; i++) t = (t + work(i)) | 0; return t; }
         function run(n) { var s = 0; for (var i = 0; i < n; i++) s = (s + work(i)) | 0; return s; }
         print(warm()); print(run(40000)); print(run(30));",
    );
    assert_eq!(ran.interpreted, ran.compiled);
    assert!(ran.runs > 0, "a region ran");
    assert!(ran.kinds.count(Event::OsrLent) > 0, "a region called its callee by the entry: {:?}", ran.kinds);
}
