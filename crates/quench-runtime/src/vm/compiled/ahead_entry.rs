//! The public native function entry of a function whose lane body is compiled ahead: a
//! trampoline that runs the body from the function's start and maps its `LaneExit` onto the
//! AOT driver's exits. A leave, a bounded run and a resume at a later PC all continue in the
//! per-PC semantic fallback, which executes one instruction at a time. The body's receiver and
//! dense captures are the payload's, bound once per activation (`JsStepDispatch::bind_captures`).
use super::*;

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

/// The trampoline's blocks: the body's entry, the per-PC loop and its two ends.
struct AheadBlocks {
    body: ir::Block,
    step: ir::Block,
    pc: ir::Value,
    stopped: ir::Block,
    invalid: ir::Block,
    invalid_pc: ir::Value,
}

impl AheadBlocks {
    fn new(builder: &mut FunctionBuilder<'_>) -> Self {
        let step = builder.create_block();
        let pc = builder.append_block_param(step, types::I64);
        let invalid = builder.create_block();
        let invalid_pc = builder.append_block_param(invalid, types::I64);
        Self { body: builder.create_block(), step, pc, stopped: builder.create_block(), invalid, invalid_pc }
    }
}

/// The entry parameter the lane body takes through unchanged, after the call context.
const ENTRY_BODY_ARGUMENTS: usize = 3;
/// The entry parameter addressing the cell the next PC or the completion's value goes through.
const ENTRY_RESULT_CELL: usize = 4;

/// Starts the lane body where the run is the whole function (it starts at the first record
/// with no bound), the per-PC loop otherwise, and maps the body's status.
fn emit_dispatch(
    builder: &mut FunctionBuilder<'_>, params: &[ir::Value], blocks: &AheadBlocks,
    body: &mut dyn FnMut(&mut FunctionBuilder<'_>, &[ir::Value]) -> (ir::Value, ir::Value),
) {
    let payload = builder.ins().load(types::I64, MemFlagsData::trusted(), params[0],
        std::mem::offset_of!(CallContext, adapter_payload) as i32);
    let start_pc = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, start_pc));
    let stop_pc = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, stop_pc));
    let starts = builder.ins().icmp_imm_u(IntCC::Equal, start_pc, 0);
    let unbounded = builder.ins().icmp_imm_u(IntCC::Equal, stop_pc, JS_NO_STOP_PC as i64);
    let whole = builder.ins().band(starts, unbounded);
    builder.ins().brif(whole, blocks.body, &[], blocks.step, &[ir::BlockArg::Value(start_pc)]);
    builder.switch_to_block(blocks.body);
    let this = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, receiver));
    let values = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_values));
    let length = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, capture_len));
    let length = builder.ins().ishl_imm_u(length, i64::from(entry_cell::CAPTURE_ADDRESS_BITS));
    let captures = builder.ins().bor(values, length);
    let (status, word) = body(builder, &[params[0], this, captures, params[ENTRY_BODY_ARGUMENTS]]);
    emit_body_status(builder, params, blocks, payload, (status, word));
}

/// The body's status: a completion returns its record's status and value, a leave continues
/// in the per-PC loop at its PC, and any other status (termination) is the entry's own.
fn emit_body_status(
    builder: &mut FunctionBuilder<'_>, params: &[ir::Value], blocks: &AheadBlocks, payload: ir::Value,
    (status, word): (ir::Value, ir::Value),
) {
    let (returned, leave) = (builder.create_block(), builder.create_block());
    let completed = builder.ins().icmp_imm_u(IntCC::Equal, status, i64::from(LANE_BODY_RETURNED));
    let other = builder.create_block();
    builder.ins().brif(completed, returned, &[], other, &[]);
    builder.switch_to_block(other);
    let stopped = builder.ins().icmp_imm_u(IntCC::Equal, status, i64::from(NATIVE_JS_STOPPED));
    let passed = builder.create_block();
    builder.ins().brif(stopped, leave, &[], passed, &[]);
    builder.switch_to_block(passed);
    builder.ins().return_(&[status]);
    builder.switch_to_block(returned);
    builder.ins().store(MemFlagsData::trusted(), word, params[ENTRY_RESULT_CELL], 0);
    let record = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, returned_pc));
    let record = builder.ins().ireduce(types::I32, record);
    let status = builder.ins().iadd_imm_u(record, i64::from(RETURN_STATUS_BASE));
    builder.ins().return_(&[status]);
    builder.switch_to_block(leave);
    // A failed entry proof resumes at the first record; any other word is the leave's PC.
    let entry = builder.ins().icmp_imm_u(IntCC::Equal, word, LaneExit::Entry.word() as i64);
    let first = builder.ins().iconst(types::I64, 0);
    let pc = builder.ins().select(entry, first, word);
    builder.ins().jump(blocks.step, &[ir::BlockArg::Value(pc)]);
}

/// The per-PC loop: one semantic operation per record until one stops the run, and the two
/// exits, a stop (its transition is in the payload) and a PC outside the function.
fn emit_steps(
    builder: &mut FunctionBuilder<'_>, params: &[ir::Value], blocks: &AheadBlocks, count: u32,
    fallback: &mut dyn FnMut(&mut FunctionBuilder<'_>, [ir::Value; 3]) -> ir::Value,
) {
    builder.switch_to_block(blocks.step);
    let run = builder.create_block();
    let in_range = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, blocks.pc, i64::from(count));
    builder.ins().brif(in_range, run, &[], blocks.invalid, &[ir::BlockArg::Value(blocks.pc)]);
    builder.switch_to_block(run);
    let status = fallback(builder, [params[0], blocks.pc, params[ENTRY_RESULT_CELL]]);
    let failed = builder.ins().icmp_imm_u(IntCC::NotEqual, status, 0);
    let next = builder.ins().load(types::I64, MemFlagsData::trusted(), params[ENTRY_RESULT_CELL], 0);
    builder.ins().brif(failed, blocks.stopped, &[], blocks.step, &[ir::BlockArg::Value(next)]);
    builder.switch_to_block(blocks.stopped);
    let stopped = builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
    builder.ins().return_(&[stopped]);
    builder.switch_to_block(blocks.invalid);
    builder.ins().store(MemFlagsData::trusted(), blocks.invalid_pc, params[ENTRY_RESULT_CELL], 0);
    let invalid = builder.ins().iconst(types::I32, i64::from(NATIVE_JS_INVALID_PC));
    builder.ins().return_(&[invalid]);
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
                let target = builder.ins().iconst(isa.pointer_type(), 8);
                let call = builder.ins().call_indirect(signature, target, words);
                let results = builder.inst_results(call);
                (results[0], results[1])
            },
            fallback: &mut |builder, [context, pc, result]| {
                let signature = builder.import_signature(native_js_operation_signature(&*isa));
                let target = builder.ins().iconst(isa.pointer_type(), 8);
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
