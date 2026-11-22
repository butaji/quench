use super::*;
use quench_bjit::call_record as protocol;

const SHAPE: CallShape = CallShape { call_pc: 9, callee: 1, this: 2, window: 3, argc: 2, result: 6, returning: false, form: CallForm::Plain, method: None, op: quench_runtime::bytecode::Op::Call };

fn facts(registers: u16, params: u16) -> DirectFacts {
    let cell = CellFacts { registers, room: registers, params, fill: quench_runtime::staged::call::FILL_ALL, entry: ENTRY_ACTIVATES as u8 };
    DirectFacts { callee: 7, cell: 0x1000, validity: 5, facts: cell, call: 0x3000, caller_base: 0x4000, function: 0, captures: 0, env: 0, capture: 0, global_this: None, lexical: false, registers, construction: false, held: false, descriptor: 0 }
}

/// Each call opcode has its form, and an instruction that is no call, or carries a result flag, has none.
#[test]
fn each_call_opcode_has_its_form() {
    use quench_runtime::bytecode::{Op, RETURN_REGISTER};
    let form = |op| CallForm::of(WideInstruction::new(op, 1, 2, 3, 0));
    assert_eq!((form(Op::Call), form(Op::Construct)), (Some(CallForm::Plain), Some(CallForm::Construct)));
    for op in [Op::CallMethod, Op::CallThisMethod, Op::CallArray, Op::CallSpread, Op::CallForwardingArguments] {
        assert_eq!(form(op), Some(CallForm::Stepped), "{op:?}");
    }
    assert_eq!(form(Op::CallKnown), None, "the Wasm compiler's call is no JavaScript call form");
    assert_eq!(form(Op::Move), None);
    assert_eq!(CallForm::of(WideInstruction::new(Op::Call, 1 | RETURN_REGISTER, 2, 3, 0)), None);
    let binds_this = WideInstruction::new(Op::Call, 1 | quench_runtime::bytecode::SET_THIS_REGISTER, 2, 3, 0);
    assert_eq!(CallForm::of(binds_this), binds_this.writes_current_this().then_some(CallForm::Stepped), "a call that binds `this` is the helper step");
    let [plain, construct, stepped] = [CallForm::Plain, CallForm::Construct, CallForm::Stepped].map(|form| form.helper() as usize);
    assert!(plain != construct && construct != stepped && plain != stepped, "each form has its own helper");
}

#[test]
fn a_stable_cell_that_describes_the_callee_is_called_directly() {
    assert_eq!(choose(&SHAPE, Some(&facts(4, 2))), Choice::Direct);
    assert_eq!(choose(&SHAPE, Some(&facts(4, 5))), Choice::Direct, "fewer arguments than params");
    let returning = CallShape { returning: true, ..SHAPE };
    assert_eq!(choose(&returning, Some(&facts(4, 2))), Choice::Direct, "a returning candidate is direct; the host's tail-call filter decides which exist");
}

#[test]
fn every_other_call_takes_the_helper_with_its_reason() {
    let helper = |why| Choice::Helper(why);
    assert_eq!(choose(&SHAPE, None), helper(Declined::NoTarget));
    let many = CallShape { argc: MAX_DIRECT_ARGC as u16 + 1, ..SHAPE };
    assert_eq!(choose(&many, Some(&facts(4, 9))), helper(Declined::Arguments));
    assert_eq!(choose(&SHAPE, Some(&DirectFacts { construction: true, ..facts(4, 2) })), helper(Declined::Construction));
    assert_eq!(choose(&SHAPE, Some(&facts(0, 2))), helper(Declined::Window));
    assert_eq!(choose(&SHAPE, Some(&facts(4, 1))), helper(Declined::Facts), "more arguments than params");
    for form in [CallForm::Construct, CallForm::Stepped] {
        assert_eq!(choose(&CallShape { form, ..SHAPE }, Some(&facts(4, 2))), helper(Declined::Form));
    }
    let unpublished = DirectFacts { facts: CellFacts { room: 9, ..facts(4, 2).facts }, ..facts(4, 2) };
    assert_eq!(choose(&SHAPE, Some(&unpublished)), helper(Declined::Facts), "the cell describes another window");
    let other_kind = DirectFacts { facts: CellFacts { entry: 1, ..facts(4, 2).facts }, ..facts(4, 2) };
    assert_eq!(choose(&SHAPE, Some(&other_kind)), helper(Declined::Facts), "not a plain activation");
}

fn plan<'a>(direct: Option<&'a DirectFacts>, spills: &'a [(Place, Register)], reloads: &'a [(Place, Register)]) -> CallPlan<'a> {
    let direct = direct.map_or(&[][..], std::slice::from_ref);
    CallPlan { shape: SHAPE, direct, helper: HelperPlace::Address(0x4000), spills, reloads, proven_args: 0, read: None, chain: Vec::new(), builtin: None }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn a_direct_chain_is_spills_direct_helper_reloads_with_the_direct_row_skipping_the_helper() {
    let (spills, reloads) = ([(Place::Pin(0), 1), (Place::PinF(2), 5)], [(Place::Pin(0), 1)]);
    let direct = facts(4, 2);
    let chain = plan_call(&plan(Some(&direct), &spills, &reloads)).expect("a plan");
    assert_eq!(chain.choice, Choice::Direct);
    assert_eq!(chain.parts.len(), spills.len() + 2 + reloads.len());
    let (direct_at, helper_at) = (spills.len(), spills.len() + 1);
    assert_eq!(chain.parts[direct_at].taken, Some(helper_at + 1), "completion skips the helper row");
    assert_eq!(chain.parts[direct_at].exits, [Exit::Status]);
    assert_eq!(chain.parts[helper_at].exits, [Exit::At(SHAPE.call_pc), Exit::Status]);
    assert_eq!(chain.parts[helper_at].holes[..2], [u64::from(SHAPE.call_pc), 0x4000]);
    let record = chain.records.into_iter().next().expect("the direct row's record");
    assert_eq!(chain.parts[direct_at].holes[0], std::ptr::from_ref(&*record).addr() as u64);
    assert_eq!((record.callee, record.cell, record.expected, record.result_register), (7, 0x1000, 5, 6));
    assert_eq!(chain.parts[spills.len() + 2].holes[0], SLOT);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn a_helper_chain_has_no_record_and_its_helper_row_is_last_before_the_reloads() {
    let chain = plan_call(&plan(None, &[], &[(Place::PinF(0), 3)])).expect("a plan");
    assert_eq!((chain.choice, chain.records.is_empty(), chain.parts.len()), (Choice::Helper(Declined::NoTarget), true, 2));
    assert!(chain.parts.iter().all(|part| part.taken.is_none()));
    let nowhere = CallPlan { helper: HelperPlace::Address(0), ..plan(None, &[], &[]) };
    assert!(plan_call(&nowhere).is_err(), "a helper row needs its address");
}

#[cfg(target_arch = "x86_64")]
#[test]
fn a_step_chain_is_spills_helper_reloads() {
    let (spills, reloads) = ([(Place::Pin(0), 1)], [(Place::PinF(1), 5), (Place::Pin(0), 1)]);
    let parts = plan_step(9, HelperPlace::Address(0x4000), None, &spills, &reloads).expect("a chain");
    assert_eq!(parts.len(), spills.len() + 1 + reloads.len());
    assert_eq!(parts[spills.len()].holes[..2], [9, 0x4000]);
    assert_eq!(parts[spills.len()].exits, [Exit::At(9), Exit::Status]);
    assert!(plan_step(9, HelperPlace::Address(0), None, &[], &[]).is_err(), "a helper row needs its address");
}

#[test]
fn a_sloppy_callee_takes_the_sloppy_row_with_its_global() {
    let sloppy = DirectFacts { global_this: Some(0xabc), ..facts(4, 2) };
    let chain = plan_call(&plan(Some(&sloppy), &[], &[])).expect("a plan");
    assert_eq!(chain.records.into_iter().next().expect("a record").global_this, 0xabc);
    let row = CallRow::Direct { argc: 2, this: ThisRow::Sloppy, checked: true, pinned: false };
    let name = row.name().expect("a row");
    assert_eq!(name, "call_direct_a2_sloppy_window", "every argument is in the window");
    assert!(std::ptr::eq(chain.parts[0].stencil, call::stencil(row).expect(&name)));
}

/// A pinned argument dead after the call is read from its pin by the direct row (its place in the record), and is
/// still spilled before the call: the direct row's refusal passes every pinned register dead, so a spill behind it
/// would hand the helper junk.
#[cfg(target_arch = "x86_64")]
#[test]
fn a_dead_pinned_argument_is_passed_by_its_pin_and_spilled_before_the_call() {
    let spills = [(Place::Pin(1), SHAPE.window), (Place::PinF(0), 9)];
    let reloads: [(Place, Register); 0] = [];
    let direct = facts(4, 2);
    let chain = plan_call(&plan(Some(&direct), &spills, &reloads)).expect("a plan");
    assert_eq!(chain.parts.len(), 4, "both spills, the direct row, the helper row");
    assert_eq!(chain.parts[0].holes[0], slot(SHAPE.window), "the argument's spill runs before the direct row");
    assert_eq!(chain.parts[2].taken, Some(4), "completion skips the helper row");
    let record = chain.records.into_iter().next().expect("the direct row's record");
    assert_eq!(record.argument_places, [Place::Pin(1).code(), Place::Mem.code()]);
}

/// The call protocol the stencils are compiled with (`quench_bjit::call_record`,
/// `stencils/layout.rs`) is the runtime's own.
#[test]
fn call_protocol_matches_the_runtime() {
    use super::super::super as compiled;
    use compiled::entry_cell::{EntryCell, LANE_BODY_NOT_ENTERED};
    use stencil_layout as mirror;
    assert_eq!(
        [compiled::LANE_CALL_COMPLETED, compiled::LANE_CALL_MISSED, compiled::LANE_CALL_LEFT, compiled::LANE_CALL_TERMINATED, compiled::LANE_CALL_BRANCHED, compiled::LaneExit::Callee.word()],
        [protocol::LANE_CALL_COMPLETED, protocol::LANE_CALL_MISSED, protocol::LANE_CALL_LEFT, protocol::LANE_CALL_TERMINATED, protocol::LANE_CALL_BRANCHED, protocol::CALLEE_EXIT]
    );
    assert_eq!(
        [compiled::RETURN_STATUS_BASE, compiled::LANE_BODY_RETURNED, LANE_BODY_NOT_ENTERED, compiled::NATIVE_JS_STOPPED, compiled::TERMINATED_STATUS as u32],
        [protocol::GENERAL_RETURN_STATUS_BASE, protocol::LANE_BODY_RETURNED, protocol::LANE_BODY_NOT_ENTERED, protocol::NATIVE_JS_STOPPED, protocol::TERMINATED_STATUS]
    );
    assert_eq!(protocol::REGISTER_BYTES, std::mem::size_of::<quench_runtime::value::Value>());
    assert_eq!(mirror::FRAME_CONTEXT_WORDS, quench_runtime::vm::register_file::FRAME_CONTEXT);
    use crate::dispatch_fast::LaneRecoverySpace;
    assert_eq!(
        [std::mem::offset_of!(EntryCell, entry), std::mem::offset_of!(EntryCell, recovery), std::mem::offset_of!(EntryCell, body), std::mem::offset_of!(LaneRecoverySpace, values), std::mem::offset_of!(LaneRecoverySpace, activations)],
        [mirror::CELL_ENTRY_AT, mirror::CELL_RECOVERY_AT, mirror::CELL_BODY_AT, mirror::RECOVERY_VALUES_AT, mirror::RECOVERY_ACTIVATIONS_AT]
    );
}

/// A direct row's record carries the arguments the caller proves Numbers, which its row passes to the callee's entry.
#[cfg(target_arch = "x86_64")]
#[test]
fn a_direct_record_carries_the_proven_arguments() {
    let direct = facts(4, 2);
    let chain = plan_call(&CallPlan { proven_args: 0b11, ..plan(Some(&direct), &[], &[]) }).expect("a plan");
    assert_eq!(chain.records.first().map(|record| record.proven_args), Some(0b11));
}

/// A method call's chain is the spills, its site read (the callee into the scratch register), the checked direct row
/// that compares that callee and takes the receiver as `this`, then the helper row; a read miss goes to the helper row.
#[test]
fn a_method_site_reads_its_callee_before_the_direct_row() {
    let shape = CallShape { callee: 7, form: CallForm::Method, method: Some(Method { cache: 3, scratch: 7 }), ..SHAPE };
    let direct = facts(4, 2);
    assert_eq!(choose(&shape, Some(&direct)), Choice::Direct);
    assert_eq!(choose(&CallShape { method: None, ..shape }, Some(&direct)), Choice::Helper(Declined::Form), "a method form needs its read");
    let read = quench_bjit::call::stencil(quench_bjit::call::CallRow::Reload(quench_bjit::numeric::Place::Pin(0))).expect("a row");
    let spills = [(quench_bjit::numeric::Place::Pin(0), 2)];
    let plan = CallPlan { shape, read: Some((read, quench_bjit::padded_holes([0]))), ..plan(Some(&direct), &spills, &[]) };
    let chain = plan_call(&plan).expect("a plan");
    assert_eq!(chain.choice, Choice::Direct);
    assert_eq!(chain.parts.len(), 4, "spill, read, direct row, helper row");
    assert!(std::ptr::eq(chain.parts[1].stencil, read), "the read follows the spills");
    assert_eq!(chain.parts[1].exits, vec![super::super::Exit::Ahead(2)], "a miss takes the helper row behind the direct row");
    assert_eq!(chain.parts[2].taken, Some(4), "the direct row completes past the helper row");
    assert_eq!(chain.records.first().map(|record| record.callee_register), Some(7), "the direct row checks the scratch register");
}

/// The stencils' layout numbers, mounted as the stencil crate mounts them (`call_protocol_matches_the_runtime`).
#[allow(dead_code)]
#[path = "../../../../../../quench-bjit/stencils/layout.rs"]
mod stencil_layout;
