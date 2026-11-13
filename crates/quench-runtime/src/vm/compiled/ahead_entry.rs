//! The public native function entry of a function whose lane body is compiled ahead: a
//! trampoline that runs the body from the function's start and maps its `LaneExit` onto the
//! AOT driver's exits. A leave, a bounded run and a resume at a later PC all continue in the
//! per-PC semantic fallback, which executes one instruction at a time. The body's receiver and
//! dense captures are the payload's, bound once per activation (`JsStepDispatch::bind_captures`).
use super::*;
use crate::staged::{region::Region, Access, NumericStage, Stage};

/// The call context, step dispatch and result words the trampoline reads and writes.
const DISPATCH: Access = Region::Dispatch.mutable();

/// The native-entry parameters a trampoline reads (`native_entry_signature`): the call
/// context, the registers, and the address of the result word.
const PARAM_CONTEXT: usize = 0;
const PARAM_REGISTERS: usize = 3;
const PARAM_RESULT: usize = 4;

/// Where a trampoline reaches the bodies it joins: a body of the lane body ABI and the
/// per-PC semantic fallback, each as the call a publication binds (an address, or a symbol).
pub(in crate::vm) struct AheadTargets<'a> {
    /// The lane body's call on the body ABI's words, giving its status and result word.
    pub(in crate::vm) body: &'a mut dyn FnMut(&mut FunctionBuilder<'_>, &[ir::Value]) -> (ir::Value, ir::Value),
    /// The fallback's call on (context, pc, result word address), giving its status word.
    pub(in crate::vm) fallback: &'a mut dyn FnMut(&mut FunctionBuilder<'_>, [ir::Value; 3]) -> ir::Value,
}

/// The entry of function `function_id` of `instruction_count` records over `targets`, named
/// by the publication's own symbol space `namespace`.
pub(in crate::vm) fn lower_ahead_entry(
    namespace: u32,
    function_id: u32,
    instruction_count: usize,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    targets: AheadTargets<'_>,
) -> Result<ir::Function, String> {
    let count = u32::try_from(instruction_count).map_err(|_| "native instruction count exceeds u32")?;
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(namespace, function_id),
        native_entry_signature(isa.pointer_type(), isa.default_call_conv()),
    );
    regions::declare_regions(&mut native);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let params = builder.block_params(entry).to_vec();
    let blocks = AheadBlocks::new(&mut builder);
    emit_dispatch(&mut builder, &params, &blocks, targets.body);
    emit_steps(&mut builder, &params, &blocks, count, targets.fallback);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

/// The trampoline's blocks: the body's entry, the per-PC loop and its two ends; the invalid
/// end reads the loop's `pc`, which dominates it.
struct AheadBlocks {
    body: ir::Block,
    step: ir::Block,
    pc: ir::Value,
    stopped: ir::Block,
    invalid: ir::Block,
}

impl AheadBlocks {
    fn new(builder: &mut FunctionBuilder<'_>) -> Self {
        let step = builder.create_block();
        let pc = builder.append_block_param(step, types::I64);
        Self { body: builder.create_block(), step, pc, stopped: builder.create_block(), invalid: builder.create_block() }
    }
}

/// Starts the lane body where the run is the whole function (it starts at the first record
/// with no bound), the per-PC loop otherwise, and maps the body's status.
fn emit_dispatch(
    builder: &mut FunctionBuilder<'_>, params: &[ir::Value], blocks: &AheadBlocks,
    body: &mut dyn FnMut(&mut FunctionBuilder<'_>, &[ir::Value]) -> (ir::Value, ir::Value),
) {
    let mut stage = super::object_slots::emit(builder);
    let payload = stage.load_u64(DISPATCH, params[PARAM_CONTEXT], std::mem::offset_of!(CallContext, adapter_payload) as i32);
    let start_pc = emit_js_payload_word(stage.builder, payload, std::mem::offset_of!(JsStepDispatch, start_pc));
    let stop_pc = emit_js_payload_word(stage.builder, payload, std::mem::offset_of!(JsStepDispatch, stop_pc));
    let starts = stage.eq_imm(start_pc, 0u32);
    let unbounded = stage.eq_imm(stop_pc, JS_NO_STOP_PC as u64);
    let whole = stage.both(starts, unbounded);
    stage.builder.ins().brif(whole, blocks.body, &[], blocks.step, &[ir::BlockArg::Value(start_pc)]);
    stage.builder.switch_to_block(blocks.body);
    let this = emit_js_payload_word(stage.builder, payload, std::mem::offset_of!(JsStepDispatch, receiver));
    let values = emit_js_payload_word(stage.builder, payload, std::mem::offset_of!(JsStepDispatch, capture_values));
    let length = emit_js_payload_word(stage.builder, payload, std::mem::offset_of!(JsStepDispatch, capture_len));
    let length = stage.mul_imm(length, 1u64 << entry_cell::CAPTURE_ADDRESS_BITS);
    let captures = stage.or(values, length);
    let (status, word) = body(stage.builder, &[params[PARAM_CONTEXT], this, captures, params[PARAM_REGISTERS]]);
    emit_body_status(builder, params, blocks, payload, (status, word));
}

/// The body's status: a completion returns its record's status and value, a leave continues
/// in the per-PC loop at its PC, and any other status (termination) is the entry's own.
fn emit_body_status(
    builder: &mut FunctionBuilder<'_>, params: &[ir::Value], blocks: &AheadBlocks, payload: ir::Value,
    (status, word): (ir::Value, ir::Value),
) {
    let (returned, leave) = (builder.create_block(), builder.create_block());
    let mut stage = super::object_slots::emit(builder);
    let wide = stage.uextend32(status);
    let running = stage.ne_imm(wide, u64::from(LANE_BODY_RETURNED));
    stage.guard(running, returned);
    let continuing = stage.ne_imm(wide, u64::from(NATIVE_JS_STOPPED));
    stage.guard(continuing, leave);
    stage.builder.ins().return_(&[status]);
    stage.builder.switch_to_block(returned);
    stage.store_u64(DISPATCH, params[PARAM_RESULT], 0, word);
    let record = emit_js_payload_word(stage.builder, payload, std::mem::offset_of!(JsStepDispatch, returned_pc));
    let record = stage.and_imm(record, u64::from(u32::MAX));
    let wide = stage.add_imm(record, u64::from(RETURN_STATUS_BASE));
    let status = stage.builder.ins().ireduce(types::I32, wide);
    stage.builder.ins().return_(&[status]);
    stage.builder.switch_to_block(leave);
    // A failed entry proof resumes at the first record; any other word is the leave's PC.
    let entry = stage.eq_imm(word, LaneExit::Entry.word());
    let first = stage.constant(0);
    let pc = stage.select(entry, first, word);
    stage.builder.ins().jump(blocks.step, &[ir::BlockArg::Value(pc)]);
}

/// The per-PC loop: one semantic operation per record until one stops the run, and the two
/// exits, a stop (its transition is in the payload) and a PC outside the function.
fn emit_steps(
    builder: &mut FunctionBuilder<'_>, params: &[ir::Value], blocks: &AheadBlocks, count: u32,
    fallback: &mut dyn FnMut(&mut FunctionBuilder<'_>, [ir::Value; 3]) -> ir::Value,
) {
    let mut stage = super::object_slots::emit(builder);
    stage.builder.switch_to_block(blocks.step);
    let records = stage.constant(u64::from(count));
    let in_range = stage.ult(blocks.pc, records);
    stage.guard(in_range, blocks.invalid);
    let status = fallback(stage.builder, [params[PARAM_CONTEXT], blocks.pc, params[PARAM_RESULT]]);
    let wide = stage.uextend32(status);
    let succeeded = stage.eq_imm(wide, 0u32);
    stage.guard(succeeded, blocks.stopped);
    let next = stage.load_u64(DISPATCH, params[PARAM_RESULT], 0);
    stage.builder.ins().jump(blocks.step, &[ir::BlockArg::Value(next)]);
    stage.builder.switch_to_block(blocks.stopped);
    let stopped = stage.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
    stage.builder.ins().return_(&[stopped]);
    stage.builder.switch_to_block(blocks.invalid);
    stage.store_u64(DISPATCH, params[PARAM_RESULT], 0, blocks.pc);
    let invalid = stage.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_INVALID_PC));
    stage.builder.ins().return_(&[invalid]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trampoline over `functions` names no instruction of its own: its blocks are the
    /// dispatch, the status map and the loop, and it verifies as one function.
    #[test]
    fn a_trampoline_verifies_over_any_targets() {
        let isa = crate::vm::jit_backend::target_isa().unwrap();
        let mut signature = None;
        let function = lower_ahead_entry(0, 1, 4, &*isa, AheadTargets {
            body: &mut |builder, words| {
                let signature = *signature.get_or_insert_with(|| builder.import_signature(
                    lane_body_signature(isa.pointer_type(), isa.default_call_conv())));
                let target = object_slots::emit(builder).constant(8);
                let call = builder.ins().call_indirect(signature, target, words);
                let results = builder.inst_results(call);
                (results[0], results[1])
            },
            fallback: &mut |builder, [context, pc, result]| {
                let signature = builder.import_signature(native_js_operation_signature(&*isa));
                let target = object_slots::emit(builder).constant(8);
                let call = builder.ins().call_indirect(signature, target, &[context, pc, result, result]);
                builder.inst_results(call)[0]
            },
        }).unwrap();
        cranelift_codegen::verify_function(&function, &*isa).unwrap();
    }
}

/// The shrink-only budget of records the trampolines leave to the per-PC fallback in the
/// benchmark sources AOT compiles (`aot-exact-steps.txt`): a source whose count grows fails
/// the check, and one whose count shrinks must lower its budget, as `oversized.txt` ratchets
/// file sizes.
#[cfg(all(test, feature = "aot"))]
mod exact_steps {
    use super::*;

    const BUDGET: &str = include_str!("../../../aot-exact-steps.txt");

    /// The records of every function of `source` that its ahead lane body does not lower, so a
    /// trampoline hands them to the per-PC fallback: a function with no body counts all its
    /// records. The root body runs once and is not counted.
    fn fallback_records(source: &str, name: &str) -> usize {
        let program = crate::Engine::specialize(source, name).expect("benchmark source compiles");
        let isa = crate::vm::jit_backend::target_isa().expect("host ISA");
        let globals = frozen_globals(&program);
        let fields = frozen_fields(&program);
        let facts = VmFacts { frozen_fields: Some(&fields), ..VmFacts::STATIC };
        let helpers = LaneHelpers::Linked(0, u32::try_from(program.function_count()).expect("native namespace"));
        (1..program.function_count()).map(|id| {
            let function = program.function(id);
            let reads = frozen_global_reads(&program, function, &globals);
            let lowered = lowered_lane_body(&program, id as u32, &*isa, helpers, facts, &reads);
            function.code.len().saturating_sub(lowered.map_or(0, |region| region.native))
        }).sum()
    }

    #[test]
    fn aot_exact_steps_stay_within_budget() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut failures = Vec::new();
        for line in BUDGET.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let (budget, path) = line.split_once(' ').expect("`count path` budget line");
            let budget: usize = budget.parse().expect("budget count");
            let source = std::fs::read_to_string(root.join(path)).expect("budgeted benchmark source");
            let count = fallback_records(&source, path);
            if count != budget {
                failures.push(format!("{path}: {count} fallback records, budget {budget}"));
            }
        }
        assert!(failures.is_empty(), "fallback records left the budget (lower it when they shrink; \
            lower an instruction natively when they grow):\n{}", failures.join("\n"));
    }
}
