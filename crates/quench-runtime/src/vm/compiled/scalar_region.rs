//! Static publication of the JIT's typed, register-resident region IR.

use super::*;

/// The namespace of a region's own IR name, by which its caller's body inlines it (`emit_entry`).
const SCALAR_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QSCR");

pub(crate) struct ScalarRegion {
    pub(crate) range: std::ops::Range<usize>,
    pub(crate) fields: Vec<FieldUse>,
    pub(crate) function: ir::Function,
}

fn scalar(instruction: WideInstruction, constants: &[Value]) -> bool {
    if instruction.result_flags() != 0
        || instruction.writes_current_this()
        || instruction.numeric_local_target().is_some()
    {
        return false;
    }
    match instruction.op().class() {
        Class::Move | Class::IncDec | Class::ToNumeric | Class::GetField => true,
        Class::LoadConst => constants
            .get(instruction.constant_index())
            .is_some_and(|v| v.as_number().is_some()),
        _ if instruction.op().js_denotation() == JsDenotation::NumericUnary => {
            super::super::operations::NumberUnary::from_js_instruction(
                instruction.op(),
                instruction.unary_operator(),
            )
            .is_some()
        }
        _ if instruction.op().js_denotation() == JsDenotation::NumericBinary => {
            super::super::operations::NumberArithmetic::from_js_instruction(
                instruction.op(),
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
        .map(|constant| super::facts::immediate(constant).filter(|value| value.as_number().is_some())
            .unwrap_or(Value::UNDEFINED))
        .collect();
    let mut leaders = std::collections::BTreeSet::new();
    for instruction in &decoded {
        if instruction.op().control_flow_layout().jumps() {
            leaders.insert(instruction.jump_target() as usize);
        }
    }
    // Without liveness, the OSR fallback conservatively treats every register
    // as live. A fresh function can have uninitialized temporaries, so such a
    // region would guard unused values and never enter its numeric path.
    let Some(live) = program.shared_register_root_masks(function_id) else {
        return Vec::new();
    };
    let (no_int32_elements, widened) = (|_: u16| false, std::collections::BTreeSet::new());
    let observation = Observation {
        register: None,
        int32_elements: &no_int32_elements,
        only_int32: &no_int32_elements,
        captures: &[],
        fields: &[],
        native_field_loads: &[],
        widened: &widened,
    };
    let lower = |range: std::ops::Range<usize>| {
        let (start, end) = (range.start, range.end);
        let arithmetic = decoded[range.clone()]
            .iter()
            .filter(|instruction| {
                !matches!(
                    instruction.op(),
                    Op::Move | Op::LoadConst | Op::ToNumeric | Op::GetField
                )
            })
            .count();
        let mut produced = std::collections::BTreeSet::new();
        let mut removes_roundtrip = false;
        for instruction in &decoded[range.clone()] {
            removes_roundtrip |= instruction.op().is_inc_dec()
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
            && arithmetic != 0
            && removes_roundtrip
            && let (Ok(header), Ok(last)) = (u32::try_from(start), u32::try_from(end - 1))
            && let Ok(region) = osr_loop::region(
                function,
                &decoded,
                &constants,
                &program.field_sites,
                Some(live),
                header,
                last,
            )
            && region.arrays.is_empty()
            && region.captures.is_empty()
            && !region.reads_heap_natively()
            && region.field_store_sites.is_empty()
            // One field plus one operation only trades a frame transfer for
            // an entry-record transfer. Keep its existing numeric region;
            // field binding needs shared inputs or a longer arithmetic chain.
            && (region.fields.len() != 1 || arithmetic > 1)
            && let Some((mut native, _)) = osr_loop::lower(&region, &observation, isa, None)
        {
            native.name = ir::UserFuncName::user(SCALAR_REGION_NAMESPACE, header);
            Some(ScalarRegion {
                range,
                fields: region.fields,
                function: native,
            })
        } else {
            None
        }
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
        if let Some(region) = lower(start..end) {
            regions.push(region);
        } else {
            // A changing receiver cannot be snapshotted. Preserve the existing
            // arithmetic regions on each side of field reads rather than
            // letting an inadmissible heap projection erase numeric coverage.
            let mut scalar_start = start;
            for (pc, instruction) in decoded.iter().enumerate().take(end).skip(start) {
                if instruction.op().is_get_field() {
                    if scalar_start < pc
                        && let Some(region) = lower(scalar_start..pc)
                    {
                        regions.push(region);
                    }
                    scalar_start = pc + 1;
                }
            }
            if scalar_start > start
                && scalar_start < end
                && let Some(region) = lower(scalar_start..end)
            {
                regions.push(region);
            }
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
) {
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
    let name = builder.func.declare_imported_user_function(
        entry.function.name.get_user().expect("a scalar region has a user name").clone());
    let callee = builder.import_function(ir::ExtFuncData {
        name: ir::ExternalName::user(name),
        signature,
        colocated: false,
        patchable: false,
    });
    builder.ins().call(callee, &[context, registers, zero, record, result]);
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
    fn bound_data_fields_reach_native_arithmetic_after_cache_warmup() {
        use cranelift_codegen::cursor::{Cursor, FuncCursor};
        let program = crate::Engine::specialize(
            r#"
            function calc(o) { let x=o.left, y=o.right; let sum=x+y; return sum*3; }
            const o={pad:0,left:2,right:3}; let total=0;
            for(let i=0;i<32;i++) total+=calc(o);
            if(total!==480) throw 'wrong field arithmetic';
        "#,
            "bound-fields.js",
        )
        .unwrap();
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        let arithmetic = Rc::new(std::cell::Cell::new(0_u64));
        let mut code = Vec::new();
        let mut operands = Vec::new();
        let mut entries = Vec::new();
        for id in 0..program.function_count() {
            let regions = if id == 0 {
                Vec::new()
            } else {
                lower_regions(&program, id as u32, &*isa)
            };
            if regions.is_empty() {
                let (memory, entry, sites) =
                    super::super::tests::live_native_function(&program, id as u32, None);
                code.push(memory);
                operands.push(sites);
                // SAFETY: only the opaque context's Rust spelling differs.
                entries.push(unsafe {
                    std::mem::transmute::<Entry, crate::NativeJsFunctionEntry>(entry)
                });
                continue;
            }
            let mut bound = Vec::new();
            for mut region in regions {
                let addition = region
                    .function
                    .layout
                    .blocks()
                    .flat_map(|block| region.function.layout.block_insts(block))
                    .find(|instruction| {
                        region.function.dfg.insts[*instruction].opcode() == ir::Opcode::Fadd
                    })
                    .expect("field addition in the typed body");
                // Count after the entry guards, immediately before arithmetic.
                // The counter belongs only to this test's live publication.
                let mut cursor = FuncCursor::new(&mut region.function);
                cursor.goto_inst(addition);
                let address = cursor
                    .ins()
                    .iconst(types::I64, arithmetic.as_ptr() as usize as i64);
                let previous = cursor
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), address, 0);
                let next = cursor.ins().iadd_imm_s(previous, 1);
                cursor
                    .ins()
                    .store(MemFlagsData::trusted(), next, address, 0);
                bound.push((region.range, region.fields, region.function));
            }
            let plans: Vec<_> = bound
                .iter()
                .map(|(range, fields, function)| StaticRegionEntry { range: range.clone(), fields, function })
                .collect();
            let (native, _) = lower_js_general_dispatch(
                &program,
                id as u32,
                &*isa,
                JsGeneralCallout::Dynamic,
                &plans,
                &[],
            )
            .unwrap();
            let memory = publish_eager_graph(native, &*isa).unwrap();
            // SAFETY: the shared entry ABI and this exact program were used
            // above; the program owner retains the body and every called region.
            entries.push(unsafe {
                std::mem::transmute::<*mut u8, crate::NativeJsFunctionEntry>(memory.pointer())
            });
            code.push(memory);
        }
        // SAFETY: every entry belongs to its function and TestHost; the owner
        // keeps all code, operand storage and the instrumentation address alive.
        let native = unsafe {
            crate::NativeJsProgram::from_entries(
                program,
                entries,
                Rc::new((code, operands, arithmetic.clone())),
            )
        }
        .unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        vm.execute_native_program(&native, true).unwrap();
        assert!(
            arithmetic.get() >= 30,
            "warm calls must run the typed body, not only baseline fallback"
        );
    }

    #[test]
    fn data_fields_feed_the_shared_typed_region_without_callouts() {
        let program = crate::Engine::specialize(
            "function subject(o) { let x=o.left, y=o.right; let sum=x+y; return sum*3; }",
            "typed-fields.js",
        )
        .unwrap();
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        let regions = lower_regions(&program, 1, &*isa);
        let region = regions
            .iter()
            .find(|region| region.fields.len() == 2)
            .unwrap_or_else(|| {
                panic!(
                    "two stable field inputs need a typed region:\n{}",
                    program.disassemble()
                )
            });
        assert!(
            region
                .function
                .layout
                .blocks()
                .flat_map(|block| region.function.layout.block_insts(block))
                .all(|instruction| !matches!(
                    region.function.dfg.insts[instruction].opcode(),
                    ir::Opcode::Call | ir::Opcode::CallIndirect
                ))
        );
        assert!(region.function.to_string().contains("fadd"));
        assert!(
            region
                .fields
                .iter()
                .all(|field| matches!(field.receiver, FieldReceiver::Register(_)))
        );
    }

    #[test]
    fn isolated_data_reads_do_not_create_numeric_regions() {
        let program = crate::Engine::specialize(
            "function subject(o) { let x=o.left; let y=x; return y; }",
            "field-copy.js",
        )
        .unwrap();
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        assert!(lower_regions(&program, 1, &*isa).is_empty());
    }

    #[test]
    fn one_field_increment_keeps_its_existing_numeric_region() {
        let program = crate::Engine::specialize(
            "function subject(o) { let n=o.count; n++; return n; }",
            "field-increment.js",
        )
        .unwrap();
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        let regions = lower_regions(&program, 1, &*isa);
        assert!(!regions.is_empty());
        assert!(regions.iter().all(|region| region.fields.is_empty()));
    }

    #[test]
    fn changing_field_receivers_keep_existing_arithmetic_regions() {
        let program = crate::Engine::specialize(
            "function subject(o, other, x) { const v=(x+1)*(x-1); const left=o.left; o=other; const right=o.right; const sum=left+right; return sum+v; }",
            "changing-fields.js",
        ).unwrap();
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        assert!(
            lower_regions(&program, 1, &*isa)
                .iter()
                .any(|region| region.fields.is_empty()
                    && region.function.to_string().contains("fmul"))
        );
    }

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
        let isa = super::super::super::jit_backend::target_isa().unwrap();
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
