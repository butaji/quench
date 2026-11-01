//! Static publication of the JIT's typed, register-resident region IR.

use super::*;

pub(crate) struct ScalarRegion {
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) function: ir::Function,
}

fn scalar(instruction: WideInstruction, constants: &[Value]) -> bool {
    if instruction.result_flags() != 0
        || instruction.writes_current_this()
        || instruction.numeric_local_target().is_some()
    {
        return false;
    }
    match instruction.op() {
        Op::Move | Op::IncDec | Op::ToNumeric => true,
        Op::LoadConst => constants
            .get(instruction.constant_index())
            .is_some_and(|v| v.as_number().is_some()),
        op if op.js_denotation() == JsDenotation::NumericUnary => {
            super::super::operations::NumberUnary::from_js_instruction(
                op,
                instruction.unary_operator(),
            )
            .is_some()
        }
        op if op.js_denotation() == JsDenotation::NumericBinary => {
            super::super::operations::NumberArithmetic::from_js_instruction(
                op,
                instruction.binary_operator(),
            )
            .is_some()
                || super::super::operations::NumberBitwise::from_operator(
                    instruction.binary_operator(),
                )
                .is_some()
        }
        _ => false,
    }
}

pub(crate) fn lower_regions(
    program: &ResidualProgram,
    function_id: u32,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Vec<ScalarRegion> {
    if isa.pointer_type() != types::I64 {
        return Vec::new();
    }
    let function = program.function(function_id as usize);
    let decoded: Vec<_> = function
        .code
        .iter()
        .map(|packed| super::super::dispatch_fast::decoded_instruction(function, *packed))
        .collect();
    // Scalar admission only consumes Number constants; other constants must
    // remain outside the numeric domain without materializing guest objects.
    let constants: Vec<_> = program
        .constants
        .iter()
        .map(|value| match value {
            crate::bytecode::Constant::Number(number) => Value::number(*number),
            _ => Value::UNDEFINED,
        })
        .collect();
    let mut leaders = std::collections::BTreeSet::new();
    for instruction in &decoded {
        if matches!(
            instruction.op().control_flow_layout(),
            ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump
        ) {
            leaders.insert(instruction.jump_target() as usize);
        }
    }
    // Without liveness, the OSR fallback conservatively treats every register
    // as live. A fresh function can have uninitialized temporaries, so such a
    // region would guard unused values and never enter its numeric path.
    let Some(live) = program.shared_register_root_masks(function_id) else {
        return Vec::new();
    };
    let no_int32_elements = |_: u16| false;
    let widened = std::collections::BTreeSet::new();
    let observation = Observation {
        register: None,
        int32_elements: &no_int32_elements,
        only_int32: &no_int32_elements,
        captures: &[],
        fields: &[],
        native_field_loads: &[],
        widened: &widened,
    };
    let mut regions = Vec::new();
    let mut start = 0;
    while start < decoded.len() {
        if !scalar(decoded[start], &constants) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < decoded.len() && !leaders.contains(&end) && scalar(decoded[end], &constants) {
            end += 1;
        }
        let range = start..end;
        let arithmetic = decoded[range.clone()].iter().any(|instruction| {
            !matches!(instruction.op(), Op::Move | Op::LoadConst | Op::ToNumeric)
        });
        let mut produced = std::collections::BTreeSet::new();
        let mut removes_roundtrip = false;
        for instruction in &decoded[range.clone()] {
            removes_roundtrip |= instruction.op() == Op::IncDec
                || osr_loop::number_reads(*instruction)
                    .iter()
                    .any(|register| produced.contains(register));
            if let Some(register) = instruction.defined_register() {
                produced.insert(register);
            }
        }
        // Retain a region when it removes a boxed producer/consumer roundtrip
        // or an IncDec semantic callout; isolated already-native math needs no wrapper.
        if end < decoded.len()
            && arithmetic
            && removes_roundtrip
            && let (Ok(header), Ok(last)) = (u32::try_from(start), u32::try_from(end - 1))
            && let Ok(region) =
                osr_loop::region(function, &decoded, &constants, &[], Some(live), header, last)
            && region.arrays.is_empty()
            && region.captures.is_empty()
            && region.fields.is_empty()
            && region.field_store_sites.is_empty()
            && let Some((native, _)) = osr_loop::lower(&region, &observation, isa, None, false)
        {
            regions.push(ScalarRegion {
                range,
                function: native,
            });
        }
        start = end;
    }
    regions
}

/// Connect a typed region to the canonical frame and exact baseline PC entries.
/// The region shares the JIT's SSA lowerer and exit materialization protocol.
pub(super) fn emit_entry(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    range: std::ops::Range<usize>,
    symbol: u32,
    context: ir::Value,
    payload: ir::Value,
    registers: ir::Value,
    result: ir::Value,
    baseline: &[ir::Block],
    entries: &[ir::Block],
    invalid: ir::Block,
) {
    builder.switch_to_block(entries[range.start]);
    // Parameter initialization may stop at any instruction inside the region.
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

    let word_bytes = std::mem::size_of::<u64>();
    let record = builder.create_sized_stack_slot(ir::StackSlotData::new(
        ir::StackSlotKind::ExplicitSlot,
        u32::try_from(osr_loop::ENTRY_CAPTURES * word_bytes).expect("scalar entry header size"),
        word_bytes.trailing_zeros() as u8,
    ));
    let zero = builder.ins().iconst(types::I64, 0);
    builder
        .ins()
        .stack_store(isa.pointer_type(), zero, record, 0);
    let record = builder.ins().stack_addr(isa.pointer_type(), record, 0);
    let signature = builder.import_signature(native_entry_signature(
        isa.pointer_type(),
        isa.default_call_conv(),
    ));
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
    builder
        .ins()
        .call(callee, &[context, registers, zero, record, result]);
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
}

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_region_reads_each_number_input_once() {
        let program = crate::Engine::specialize(
            "function region(x, y) { return ((x + y) * (x - y)) / 3; }",
            "scalar-region.js",
        )
        .unwrap();
        let id = program
            .functions
            .iter()
            .position(|body| {
                body.header()
                    .name
                    .is_some_and(|name| &program.atoms[name as usize] == "region")
            })
            .unwrap();
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let regions = lower_regions(&program, id as u32, &*isa);
        let region = regions
            .iter()
            .find(|region| region.range.len() >= 3)
            .expect("adjacent arithmetic must share a typed region");
        let entry = region.function.layout.entry_block().unwrap();
        let registers = region.function.dfg.block_params(entry)[1];
        let frame_loads = region
            .function
            .layout
            .blocks()
            .flat_map(|block| region.function.layout.block_insts(block))
            .filter(|inst| {
                region.function.dfg.insts[*inst].opcode() == ir::Opcode::Load
                    && region.function.dfg.inst_args(*inst).first() == Some(&registers)
            })
            .count();
        assert_eq!(
            frame_loads,
            2,
            "only the two live-in Numbers are loaded from canonical homes; {:?}\n{}\n{}",
            region.range,
            program.disassemble(),
            region.function.display()
        );
        let ir = region.function.display().to_string();
        assert!(ir.contains("fadd") && ir.contains("fsub") && ir.contains("fmul"));
    }
}
