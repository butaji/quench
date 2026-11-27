#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Runs `source` interpreted and compiled with lane OSR, and returns the compiled output,
/// equal to the interpreter's, and the OSR diagnostics of the function named `function`.
pub(crate) fn osr_run(source: &str, function: &str) -> (Vec<String>, quench_runtime::vm::engine_types::OsrDiagnostics) {
    let program = quench_runtime::Engine::specialize(source, "lane-osr-admission.js").unwrap();
    let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(expected.clone())), &program).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(output.clone()));
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(*output.borrow(), *expected.borrow(), "compiled output matches the interpreter");
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == function)).unwrap();
    let views = vm.programs.views_in_lane(super::super::program_store::ProgramId::MAIN).unwrap();
    let osr = views.get(id).and_then(super::super::program_store::derived_lane_view)
        .unwrap().osr_diagnostics();
    assert!(osr.native_region_runs > 0, "the loop ran natively: {osr:?}");
    let lines = output.borrow().clone();
    (lines, osr)
}

