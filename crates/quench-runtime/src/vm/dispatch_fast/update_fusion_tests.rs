//! A loop site that retires without a region gives its update the fusion the interpreter's loader gives
//! it (`update_fusion`), so a declined loop runs as many dispatches as `--no-jit`'s.

use super::*;
use crate::test_host::TestHost;
use crate::{Engine, ExecutionMode, JitMode};

/// The loop's `**` has no region row (`osr_loop::arithmetic`): its site is rejected, and retires at its first hot jump.
const SOURCE: &str = "function f(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i ** 2) | 0; } return s; }
    let t = 0; for (let k = 0; k < 64; k++) { t = (t + f(500)) | 0; } globalThis.answer = t;";

/// `f`'s loop sites, and the handlers of its updates, after the program ran under `mode`.
fn after_run(mode: JitMode) -> (usize, Vec<*const ()>) {
    let program = Engine::specialize(SOURCE, "update-fusion.js").unwrap();
    let function = program.functions.iter().position(|body| body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "f")).unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(ExecutionMode::Compiled);
    crate::jit_mode::with_mode(mode, || vm.execute(&program)).unwrap();
    let views = vm.programs.lane_views(super::super::program_store::ProgramId::MAIN).unwrap();
    let view = views.get(function).and_then(super::super::program_store::derived_lane_view).expect("the function's view");
    let updates = program.function(function).instructions().enumerate().filter(|(_, instruction)| instruction.op().is_inc_dec()).map(|(pc, _)| pc);
    // SAFETY: each update's PC is inside the view's records.
    (view.loops.len(), updates.map(|pc| unsafe { (*view.records().add(pc)).handler }).collect())
}

#[test]
fn a_retired_loop_site_fuses_its_update_as_the_interpreter_does() {
    let (no_sites, interpreted) = after_run(JitMode::Interp);
    let (sites, retired) = after_run(JitMode::Full);
    assert!(no_sites == 0 && sites > 0, "only a jit run installs loop sites: {no_sites} and {sites}");
    assert!(!interpreted.is_empty(), "the loop has an update");
    assert_eq!(retired, interpreted, "the retired loop's update runs the records the interpreter's does");
}

/// The fusion of an update and the unconditional jump after it is the `IncDec`/`Jump` pair's record,
/// as the loader makes it; any other record before a jump fuses nothing.
#[test]
fn an_update_fuses_with_the_jump_after_it() {
    let record = |handler: *const (), a: u16| LaneInstruction { handler, operand: LaneOperand { jump: 0 }, a, b: 0, c: 0, call_link: std::cell::Cell::new(NO_CALL_LINK) };
    let update = record(generated_row::<TestHost, { opc::IncDec }, ACC_NONE, FLAG_UNREAD> as Handler as *const (), 0);
    let jump = record(jump_row::<TestHost, { opc::Jump }>(true) as *const (), 0);
    let fused = update_fusion::<TestHost>(&update, &jump).map(|handler| handler as *const ());
    assert_eq!(fused, Some(lane_frame::fused_row::<TestHost, { opc::IncDec }, { opc::Jump }, { crate::staged::FUSED_PLAIN }>(true) as *const ()));
    assert!(update_fusion::<TestHost>(&jump, &jump).is_none(), "only an `IncDec` fuses with the jump after it");
}
