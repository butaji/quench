//! The dispatch loop of a call-threaded function and of a region dispatcher: one block per
//! instruction (or region), the PC that selects among them through a jump table, and the exits
//! for a PC none has and for an operation that stopped.
use super::*;
use super::super::specialize::Continuation;
use super::super::compiled::emit_machine::lane_of;
use super::super::compiled::{NATIVE_JS_INVALID_PC, NATIVE_JS_STOPPED, native_entry_signature, native_js_start_pc};
use crate::staged::machine::Unary;

const NATIVE_JS_REGION_EXIT: u32 = NATIVE_JS_INVALID_PC + 1;
// The shared entry ABI's memory-base word is unused by JavaScript bodies.
const REGION_START_PC_PARAMETER: usize = 1;
/// The entry ABI's result-word parameter (context, pc, operands, unused, result).
const RESULT_PARAMETER: usize = 4;

/// The blocks every dispatch loop shares: a PC arrives at `resume`, `select` jumps by the index
/// the PC names, and `invalid` leaves with a PC that names no block.
struct DispatchLoop {
    resume: ir::Block,
    pc: ir::Value,
    select: ir::Block,
    selector: ir::Value,
    invalid: ir::Block,
    invalid_pc: ir::Value,
}

impl DispatchLoop {
    fn new(builder: &mut FunctionBuilder<'_>) -> Self {
        let resume = builder.new_block();
        let pc = builder.block_parameter(resume, Lane::I64);
        let select = builder.new_block();
        let selector = builder.block_parameter(select, Lane::I32);
        let invalid = builder.new_block();
        let invalid_pc = builder.block_parameter(invalid, Lane::I64);
        Self { resume, pc, select, selector, invalid, invalid_pc }
    }

    /// The end of `resume`: a PC in range selects the block of `index` among `blocks`; any other
    /// is stored in `result` and left with `invalid_status`. The full PC is checked before it
    /// is narrowed to `index`: a high word must not alias a valid block and replay guest effects.
    fn finish(&self, builder: &mut FunctionBuilder<'_>, (blocks, result): (&[ir::Block], ir::Value), (in_range, index): (ir::Value, ir::Value), invalid_status: u32) {
        builder.branch_if(in_range, (self.select, &[index]), (self.invalid, &[self.pc]));
        builder.enter_open(self.select);
        let table = builder.jump_table(blocks, (self.invalid, &[self.pc]));
        builder.dispatch(self.selector, table);
        builder.enter_open(self.invalid);
        builder.raw_store(Lane::I64, MemFlagsData::trusted(), (self.invalid_pc, false), result, 0);
        let status = builder.raw_const(Lane::I32, i64::from(invalid_status));
        builder.leave(&[status]);
    }
}

/// Where the instructions of one function, or one region of it, continue.
struct Instructions<'a> {
    start: usize,
    blocks: &'a [ir::Block],
    resume: ir::Block,
    stopped: ir::Block,
}

/// One instruction: a computed transfer, or its operation and the exact continuation it reports.
fn lower_site<F: FnMut(&mut FunctionBuilder<'_>, &InstructionSite, ir::Value, ir::Value) -> ir::Value>(
    builder: &mut FunctionBuilder<'_>, instructions: &Instructions<'_>, (context, result): (ir::Value, ir::Value),
    (offset, site): (usize, &InstructionSite), operation: &mut F,
) {
    if let Continuation::Static(target) = site.continuation {
        match target.checked_sub(instructions.start).and_then(|offset| instructions.blocks.get(offset)) {
            Some(block) => builder.goto(*block, &[]),
            None => {
                let target = builder.raw_const(Lane::I64, target as i64);
                builder.goto(instructions.resume, &[target]);
            }
        }
        return;
    }
    let status = operation(builder, site, context, result);
    let continued = builder.raw_compare_imm(Relation::Equal, status, 0);
    let successor = builder.new_block();
    builder.branch_if(continued, (successor, &[]), (instructions.stopped, &[]));
    builder.enter_open(successor);
    let next = builder.raw_load(Lane::I64, MemFlagsData::trusted(), result, 0);
    match instructions.blocks.get(offset + 1) {
        Some(block) if site.continuation == Continuation::Fallthrough => {
            let falls = builder.raw_compare_imm(Relation::Equal, next, (site.pc + 1) as i64);
            builder.branch_if(falls, (*block, &[]), (instructions.resume, &[next]));
        }
        _ => builder.goto(instructions.resume, &[next]),
    }
}

/// What `build_body` lowers: the function's sites, the range of them this body holds (a private
/// region, or all) and its length.
struct BodyPlan<'a> {
    sites: &'a [InstructionSite],
    range: Range<usize>,
    count: u32,
    is_region: bool,
}

fn build_body<F: FnMut(&mut FunctionBuilder<'_>, &InstructionSite, ir::Value, ir::Value) -> ir::Value>(
    builder: &mut FunctionBuilder<'_>, plan: &BodyPlan<'_>, operation: &mut F,
) {
    let parameters = enter_native_entry(builder);
    let (context, result) = (parameters[0], parameters[RESULT_PARAMETER]);
    let start_pc = if plan.is_region { parameters[REGION_START_PC_PARAMETER] } else { native_js_start_pc(builder, context) };
    let dispatch = DispatchLoop::new(builder);
    let stopped = builder.new_block();
    let blocks: Vec<_> = (0..plan.count).map(|_| builder.new_block()).collect();
    builder.goto(dispatch.resume, &[start_pc]);
    let instructions = Instructions { start: plan.range.start, blocks: &blocks, resume: dispatch.resume, stopped };
    for (offset, block) in blocks.iter().copied().enumerate() {
        builder.enter_open(block);
        lower_site(builder, &instructions, (context, result), (offset, &plan.sites[plan.range.start + offset]), operation);
    }
    builder.enter_open(dispatch.resume);
    let offset = builder.raw_binary_imm(Binary::Add, dispatch.pc, -(plan.range.start as i64));
    let in_range = builder.raw_compare_imm(Relation::UnsignedLess, offset, i64::from(plan.count));
    let index = builder.raw_unary(Unary::Narrow(Lane::I32), offset);
    let invalid_status = if plan.is_region { NATIVE_JS_REGION_EXIT } else { NATIVE_JS_INVALID_PC };
    dispatch.finish(builder, (blocks.as_slice(), result), (in_range, index), invalid_status);
    builder.enter_open(stopped);
    let status = builder.raw_const(Lane::I32, i64::from(NATIVE_JS_STOPPED));
    builder.leave(&[status]);
}

/// Bindings choose linker symbols or live JIT addresses; the graph and semantic
/// continuation protocol are identical in both publication modes.
pub(in crate::vm) fn lower_body(
    sites: &[InstructionSite],
    function_id: u32,
    isa: &dyn TargetIsa,
    region: Option<Range<usize>>,
    mut operation: impl FnMut(&mut FunctionBuilder<'_>, &InstructionSite, ir::Value, ir::Value) -> ir::Value,
) -> Result<ir::Function, String> {
    u32::try_from(sites.len()).map_err(|_| "native instruction count exceeds u32")?;
    let is_region = region.is_some();
    let range = region.unwrap_or(0..sites.len());
    if range.start > range.end || range.end > sites.len() {
        return Err("invalid native region bounds".into());
    }
    let count = u32::try_from(range.len()).map_err(|_| "native region instruction count exceeds u32")?;
    let mut native = native_function(function_id, native_entry_signature(isa.pointer_type(), isa.default_call_conv()));
    let mut builder_context = crate::vm::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    build_body(&mut builder, &BodyPlan { sites, range, count, is_region }, &mut operation);
    builder.seal_all();
    builder.finalize(isa.frontend_config());
    crate::vm::jit_backend::builder::keep_context(builder_context);
    Ok(native)
}

/// Private regions reuse the native ABI, carrying their starting PC in its
/// otherwise unused memory-base word. Guest arguments remain in the VM frame.
pub(in crate::vm) fn region_call_arguments(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn TargetIsa,
    context: ir::Value,
    pc: ir::Value,
    result: ir::Value,
) -> [ir::Value; 5] {
    let length = builder.raw_const(Lane::I64, 0);
    let arguments = builder.raw_const(lane_of(isa.pointer_type()), 0);
    [context, pc, length, arguments, result]
}

/// The regions of an instruction count: how many instructions each spans, and how many there are.
struct RegionPlan {
    count: u32,
    width: usize,
    regions: usize,
}

fn build_region_dispatch<F: FnMut(&mut FunctionBuilder<'_>, usize, ir::Value, ir::Value, ir::Value) -> ir::Value>(
    builder: &mut FunctionBuilder<'_>, plan: &RegionPlan, call: &mut F,
) {
    let parameters = enter_native_entry(builder);
    let (context, result) = (parameters[0], parameters[RESULT_PARAMETER]);
    let start = native_js_start_pc(builder, context);
    let dispatch = DispatchLoop::new(builder);
    let stopped = builder.new_block();
    let status = builder.block_parameter(stopped, Lane::I32);
    let blocks: Vec<_> = (0..plan.regions).map(|_| builder.new_block()).collect();
    builder.goto(dispatch.resume, &[start]);
    for (index, block) in blocks.iter().copied().enumerate() {
        builder.enter_open(block);
        let status = call(builder, index, context, dispatch.pc, result);
        let continued = builder.raw_compare_imm(Relation::Equal, status, i64::from(NATIVE_JS_REGION_EXIT));
        let next = builder.new_block();
        builder.branch_if(continued, (next, &[]), (stopped, &[status]));
        builder.enter_open(next);
        let next_pc = builder.raw_load(Lane::I64, MemFlagsData::trusted(), result, 0);
        builder.goto(dispatch.resume, &[next_pc]);
    }
    builder.enter_open(dispatch.resume);
    let in_range = builder.raw_compare_imm(Relation::UnsignedLess, dispatch.pc, i64::from(plan.count));
    let divisor = builder.raw_const(Lane::I64, plan.width as i64);
    let region = builder.raw_binary(Binary::DivideUnsigned, dispatch.pc, divisor);
    let index = builder.raw_unary(Unary::Narrow(Lane::I32), region);
    dispatch.finish(builder, (blocks.as_slice(), result), (in_range, index), NATIVE_JS_INVALID_PC);
    builder.enter_open(stopped);
    builder.leave(&[status]);
}

pub(in crate::vm) fn lower_region_dispatch(
    function_id: u32,
    instruction_count: usize,
    width: usize,
    region_count: usize,
    isa: &dyn TargetIsa,
    mut call: impl FnMut(&mut FunctionBuilder<'_>, usize, ir::Value, ir::Value, ir::Value) -> ir::Value,
) -> Result<ir::Function, String> {
    let count = u32::try_from(instruction_count).map_err(|_| "native instruction count exceeds u32")?;
    if width == 0 || instruction_count.div_ceil(width) != region_count {
        return Err("invalid native region plan".into());
    }
    let mut native = native_function(function_id, native_entry_signature(isa.pointer_type(), isa.default_call_conv()));
    let mut context_builder = crate::vm::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut native, &mut context_builder);
    build_region_dispatch(&mut builder, &RegionPlan { count, width, regions: region_count }, &mut call);
    builder.seal_all();
    builder.finalize(isa.frontend_config());
    crate::vm::jit_backend::builder::keep_context(context_builder);
    Ok(native)
}
