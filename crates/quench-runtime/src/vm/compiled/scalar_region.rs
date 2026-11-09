//! Static publication of the JIT's typed, register-resident region IR.

use super::*;

pub(crate) struct ScalarRegion {
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) fields: Vec<FieldUse>,
    pub(crate) frame: Vec<Register>,
    pub(crate) function: ir::Function,
}

/// Scalar regions were published by a second lowering. They stay disabled until the lane
/// unit carries these ranges; the typed lane regions remain the AOT path.
pub(crate) fn lower_regions(
    _program: &ResidualProgram,
    _function_id: u32,
    _isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Vec<ScalarRegion> {
    Vec::new()
}

/// Connect a typed region to the canonical frame and exact baseline PC entries.
/// The region shares the JIT's SSA lowerer and exit materialization protocol.
pub(super) fn emit_entry(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    entry: &StaticRegionEntry<'_>,
    gate: ir::Block,
    context: ir::Value,
    payload: ir::Value,
    registers: ir::Value,
    result: ir::Value,
    baseline: &[ir::Block],
    entries: &[ir::Block],
    invalid: ir::Block,
    control: &NativeStopControl,
) -> ir::Inst {
    let range = entry.range.clone();
    builder.switch_to_block(gate);
    // An activation whose bound was not discharged may stop inside the region.
    if matches!(control, NativeStopControl::Unproven) {
        let stop = builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            payload,
            std::mem::offset_of!(JsStepDispatch, stop_pc) as i32,
        );
        let after_start =
            builder
                .ins()
                .icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, stop, range.start as i64);
        let before_end = builder
            .ins()
            .icmp_imm_u(IntCC::UnsignedLessThan, stop, range.end as i64);
        let stops_inside = builder.ins().band(after_start, before_end);
        let native = builder.create_block();
        builder
            .ins()
            .brif(stops_inside, baseline[range.start], &[], native, &[]);
        builder.switch_to_block(native);
    }

    let word_bytes = std::mem::size_of::<u64>();
    let record_slot = builder.create_sized_stack_slot(ir::StackSlotData::new(
        ir::StackSlotKind::ExplicitSlot,
        u32::try_from((osr_loop::ENTRY_CAPTURES + entry.fields.len()) * word_bytes)
            .expect("typed entry record size"),
        word_bytes.trailing_zeros() as u8,
    ));
    let zero = builder.ins().iconst(types::I64, 0);
    builder
        .ins()
        .stack_store(isa.pointer_type(), zero, record_slot, 0);
    // Entry binding performs only guarded data reads: no accessor, coercion,
    // allocation or guest call can run before the typed region's input guards.
    // The region admits no writes, so these values remain current for its run.
    for (index, field) in entry.fields.iter().enumerate() {
        let offset = |index: usize| {
            i32::try_from((osr_loop::ENTRY_CAPTURES + index) * word_bytes)
                .expect("typed field entry offset")
        };
        let receiver = match field.receiver {
            FieldReceiver::This => JsFieldReceiver::FixedThis,
            FieldReceiver::Register(register) => {
                JsFieldReceiver::Register(i32::from(register) * word_bytes as i32)
            }
            FieldReceiver::Field(parent) => {
                assert!(parent < index, "entry field paths are ordered by dependency");
                JsFieldReceiver::Value(builder.ins().stack_load(
                    isa.pointer_type(),
                    types::I64,
                    record_slot,
                    offset(parent),
                ))
            }
            // Scalar ranges hold no capture reads (`scalar`), so no path roots at one.
            FieldReceiver::Capture(_) => unreachable!("scalar regions read no captures"),
        };
        let (address, _) = emit_js_cached_field_slot(
            builder,
            registers,
            payload,
            receiver,
            field.cache_site,
            true,
            baseline[range.start],
            None,
            None,
            None,
        );
        let bits = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), address, 0);
        builder
            .ins()
            .stack_store(types::I64, bits, record_slot, offset(index));
    }
    let record = builder.ins().stack_addr(isa.pointer_type(), record_slot, 0);
    let signature = builder.import_signature(native_entry_signature(
        isa.pointer_type(),
        isa.default_call_conv(),
    ));
    let call = match entry.target {
        StaticRegionTarget::Symbol(symbol) => {
            let name = builder
                .func
                .declare_imported_user_function(ir::UserExternalName::new(
                    super::super::native_object::NATIVE_FUNCTION_NAMESPACE,
                    symbol,
                ));
            let callee = builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name),
                signature,
                colocated: false,
                patchable: false,
            });
            builder.ins().call(callee, &[context, registers, zero, record, result])
        }
        #[cfg(feature = "aot")]
        StaticRegionTarget::Address(address) => {
            let callee = builder.ins().iconst(isa.pointer_type(), address as i64);
            builder.ins().call_indirect(
                signature,
                callee,
                &[context, registers, zero, record, result],
            )
        }
    };
    let word = builder
        .ins()
        .load(types::I64, MemFlagsData::trusted(), result, 0);
    let complete = builder.ins().icmp_imm_u(
        IntCC::Equal,
        word,
        RegionExit::Resume(range.end as u32).encode() as i64,
    );
    let deopt = builder.create_block();
    builder
        .ins()
        .brif(complete, entries[range.end], &[], deopt, &[]);
    builder.switch_to_block(deopt);
    let kind = builder
        .ins()
        .ushr_imm_u(word, i64::from(RegionExit::KIND_SHIFT));
    let missed = builder.ins().icmp_imm_u(
        IntCC::Equal,
        kind,
        (RegionExit::EntryMiss(0).encode() >> RegionExit::KIND_SHIFT) as i64,
    );
    let exact = builder.create_block();
    builder
        .ins()
        .brif(missed, baseline[range.start], &[], exact, &[]);
    builder.switch_to_block(exact);
    let pc = builder
        .ins()
        .band_imm_u(word, RegionExit::PAYLOAD_MASK as i64);
    let relative = builder.ins().iadd_imm_s(pc, -(range.start as i64));
    let selector = builder.ins().ireduce(types::I32, relative);
    let cases = baseline[range]
        .iter()
        .map(|block| builder.func.dfg.block_call(*block, &[]))
        .collect::<Vec<_>>();
    let default = builder
        .func
        .dfg
        .block_call(invalid, &[ir::BlockArg::Value(pc)]);
    let table = builder.create_jump_table(ir::JumpTableData::new(default, &cases));
    builder.ins().br_table(selector, table);
    call
}
