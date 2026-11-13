//! A harness around `run_js_native_entry` for the AOT trampoline: it lowers the trampoline over
//! stub targets and runs it on a `JsStepDispatch` the test scripts, so its stop bound and its
//! invalid-PC exit are observed without a program.
use super::*;
use std::cell::RefCell;

const RECORDS: usize = 4;
const BODY_RESULT: u64 = 0x5eed;
const BODY_RETURNED_PC: usize = 3;
/// The stub fallback's status for the record at the stop bound.
const FALLBACK_STOPPED: u64 = 1;
const FALLBACK_CONTINUES: u64 = 0;
const ENTRY_NAMESPACE: u32 = 0;
const ENTRY_FUNCTION: u32 = 1;

/// What the stubs saw: the records the fallback ran, in order, and whether the body ran.
#[derive(Default)]
struct Reached {
    records: Vec<u64>,
    body: bool,
}

thread_local! {
    static REACHED: RefCell<Reached> = RefCell::new(Reached::default());
}

unsafe extern "C" fn stub_body(_: *mut CallContext, _: u64, _: u64, _: *const u64) -> BodyReturn {
    REACHED.with(|reached| reached.borrow_mut().body = true);
    BodyReturn { status: LANE_BODY_RETURNED, value: BODY_RESULT }
}

/// Runs a record: the bound's record stops the run, any other continues at its successor.
unsafe extern "C" fn stub_fallback(context: *mut std::ffi::c_void, pc: u64, _: *const u64, result: *mut u64) -> u64 {
    // SAFETY: the trampoline passes the activation's context, whose payload is the harness's dispatch.
    let dispatch = unsafe { &*(*context.cast::<CallContext>()).adapter_payload.cast::<JsStepDispatch>() };
    REACHED.with(|reached| reached.borrow_mut().records.push(pc));
    if pc as usize == dispatch.stop_pc {
        return FALLBACK_STOPPED;
    }
    // SAFETY: the trampoline passes the address of its successor word.
    unsafe { *result = pc + 1 };
    FALLBACK_CONTINUES
}

/// A trampoline over the stubs, published.
fn trampoline() -> (CodeMemory, crate::NativeJsFunctionEntry) {
    let isa = super::super::jit_backend::target_isa().unwrap();
    let (mut body_signature, mut fallback_signature) = (None, None);
    let function = lower_ahead_entry(ENTRY_NAMESPACE, ENTRY_FUNCTION, RECORDS, &*isa, AheadTargets {
        body: &mut |builder, words| {
            let signature = *body_signature.get_or_insert_with(|| builder.import_signature(
                lane_body_signature(isa.pointer_type(), isa.default_call_conv())));
            let target = builder.ins().iconst(isa.pointer_type(), stub_body as *const () as usize as i64);
            let call = builder.ins().call_indirect(signature, target, words);
            let results = builder.inst_results(call);
            (results[0], results[1])
        },
        fallback: &mut |builder, [context, pc, result]| {
            let signature = *fallback_signature.get_or_insert_with(|| builder.import_signature(native_js_operation_signature(&*isa)));
            let target = builder.ins().iconst(isa.pointer_type(), stub_fallback as *const () as usize as i64);
            let call = builder.ins().call_indirect(signature, target, &[context, pc, result, result]);
            builder.inst_results(call)[0]
        },
    }).unwrap();
    compile_native_function(function, &*isa, usize::MAX, |code| {
        let memory = publish_code(code)?;
        // SAFETY: the trampoline is lowered with `native_entry_signature`.
        let entry = unsafe { std::mem::transmute::<*mut u8, crate::NativeJsFunctionEntry>(memory.pointer()) };
        Some((memory, entry))
    }).unwrap()
}

/// The status and word a run returned, and what the stubs reached.
struct Run {
    status: u32,
    word: u64,
    reached: Reached,
}

/// Runs the trampoline on a payload that starts at `start_pc` and stops at `stop_pc`.
fn run(start_pc: usize, stop_pc: usize) -> Run {
    let (_code, entry) = trampoline();
    let mut vm = tests::semantic_vm("throw 7;");
    REACHED.with(|reached| *reached.borrow_mut() = Reached::default());
    let (status, word) = JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, start_pc, false, false, |activation, vm| {
        let dispatch = &mut activation.dispatch;
        dispatch.stop_pc = stop_pc;
        dispatch.returned_pc = BODY_RETURNED_PC;
        let interrupt = vm.execution_interrupt.atomic_flag() as *const _;
        run_js_native_entry(entry, vm, ProgramId::MAIN, (dispatch as *mut JsStepDispatch).cast(), interrupt, None)
    }).unwrap().unwrap();
    Run { status, word, reached: REACHED.with(|reached| std::mem::take(&mut *reached.borrow_mut())) }
}

#[test]
fn an_unbounded_run_from_the_first_record_runs_the_body() {
    let run = run(0, JS_NO_STOP_PC);
    assert!(run.reached.body && run.reached.records.is_empty());
    assert_eq!(run.status, RETURN_STATUS_BASE + BODY_RETURNED_PC as u32, "the body's completion returns its record's status");
    assert_eq!(run.word, BODY_RESULT);
}

#[test]
fn a_bounded_run_skips_the_body_and_stops_at_its_bound() {
    let run = run(0, 2);
    assert!(!run.reached.body);
    assert_eq!(run.reached.records, [0, 1, 2], "no record past the bound runs");
    assert_eq!(run.status, NATIVE_JS_STOPPED);
}

#[test]
fn a_bound_at_the_first_record_stops_before_any_successor() {
    let run = run(0, 0);
    assert_eq!(run.reached.records, [0]);
    assert_eq!(run.status, NATIVE_JS_STOPPED);
}

#[test]
fn stepping_past_the_last_record_is_an_invalid_pc() {
    let run = run(1, JS_NO_STOP_PC);
    assert!(!run.reached.body);
    assert_eq!(run.reached.records, [1, 2, 3]);
    assert_eq!((run.status, run.word), (NATIVE_JS_INVALID_PC, RECORDS as u64));
}

#[test]
fn a_start_outside_the_function_is_an_invalid_pc_before_any_record_runs() {
    let start = RECORDS + 3;
    let run = run(start, JS_NO_STOP_PC);
    assert!(!run.reached.body && run.reached.records.is_empty());
    assert_eq!((run.status, run.word), (NATIVE_JS_INVALID_PC, start as u64));
}
