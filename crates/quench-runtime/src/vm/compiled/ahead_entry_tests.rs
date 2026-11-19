//! A harness around `run_js_native_entry` for the AOT trampoline: it lowers the trampoline over
//! a stub body and runs it on a `JsStepDispatch`, so the completion it maps is observed without a
//! program.
use super::*;
use std::cell::Cell;

const BODY_RESULT: u64 = 0x5eed;
const BODY_RETURNED_PC: usize = 3;
const ENTRY_NAMESPACE: u32 = 0;
const ENTRY_FUNCTION: u32 = 1;

thread_local! {
    static BODY_RAN: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "C" fn stub_body(_: *mut CallContext, _: u64, _: u64, _: *const u64) -> BodyReturn {
    BODY_RAN.with(|ran| ran.set(true));
    BodyReturn { status: LANE_BODY_RETURNED, value: BODY_RESULT }
}

/// A trampoline over the stub body, published.
fn trampoline() -> (CodeMemory, crate::NativeJsFunctionEntry) {
    let isa = super::super::jit_backend::target_isa().unwrap();
    let mut body_signature = None;
    let function = lower_ahead_entry(ENTRY_NAMESPACE, ENTRY_FUNCTION, &*isa, AheadTargets {
        body: &mut |builder, words| {
            let signature = *body_signature.get_or_insert_with(|| builder.import_signature(
                body_signature_in_lane(isa.pointer_type(), isa.default_call_conv())));
            let target = builder.ins().iconst(isa.pointer_type(), stub_body as *const () as usize as i64);
            let call = builder.ins().call_indirect(signature, target, words);
            let results = builder.inst_results(call);
            (results[0], results[1])
        },
    }).unwrap();
    compile_native_function(function, &*isa, usize::MAX, |code| {
        let memory = publish_code(code)?;
        // SAFETY: the trampoline is lowered with `native_entry_signature`.
        let entry = unsafe { std::mem::transmute::<*mut u8, crate::NativeJsFunctionEntry>(memory.pointer()) };
        Some((memory, entry))
    }).unwrap()
}

/// The status and word a run returned, and whether the body ran.
struct Run {
    status: u32,
    word: u64,
    body: bool,
}

/// Runs the trampoline from the first record.
fn run() -> Run {
    let (_code, entry) = trampoline();
    let mut vm = tests::semantic_vm("throw 7;");
    BODY_RAN.with(|ran| ran.set(false));
    let (status, word) = JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, false, |activation, vm| {
        let dispatch = &mut activation.dispatch;
        dispatch.returned_pc = BODY_RETURNED_PC;
        let interrupt = vm.execution_interrupt.atomic_flag() as *const _;
        run_js_native_entry(entry, vm, ProgramId::MAIN, (dispatch as *mut JsStepDispatch).cast(), interrupt, None)
    }).unwrap().unwrap();
    Run { status, word, body: BODY_RAN.with(Cell::get) }
}

#[test]
fn the_entry_runs_the_body_and_maps_its_completion() {
    let run = run();
    assert!(run.body, "the entry calls its body");
    assert_eq!(run.status, RETURN_STATUS_BASE + BODY_RETURNED_PC as u32, "the completion returns its record's status");
    assert_eq!(run.word, BODY_RESULT);
}
