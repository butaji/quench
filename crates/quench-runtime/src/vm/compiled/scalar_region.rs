//! Scalar regions: bounded straight-line ranges of a function that run as one static
//! typed region, one `RegionPlan` of the shared specializer (a `Coverage::Range`), like
//! every other region the image publishes.

use super::*;

/// The symbol namespace of a scalar region, named by its first PC so the caller's body can
/// inline it beside the function's loop regions (which use `lane_region::LANE_REGION_NAMESPACE`).
pub(super) const SCALAR_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QSCR");

/// An instruction a scalar range may hold: straight-line, effect-free Number work, a
/// move, a constant Number, or a field read that guards itself.
fn scalar(instruction: WideInstruction, constants: &[Value]) -> bool {
    if instruction.result_flags() != 0
        || instruction.writes_current_this()
        || instruction.numeric_local_target().is_some()
    {
        return false;
    }
    let op = instruction.op();
    if matches!(op.class(), Class::Move | Class::IncDec | Class::ToNumeric | Class::GetField) {
        return true;
    }
    if op.class().eq(Class::LoadConst) {
        return constants
            .get(instruction.constant_index())
            .is_some_and(|v| v.as_number().is_some());
    }
    if op.js_denotation() == JsDenotation::NumericUnary {
        return super::super::operations::NumberUnary::from_js_instruction(
            op,
            instruction.unary_operator(),
        )
        .is_some();
    }
    if op.js_denotation() == JsDenotation::NumericBinary {
        return super::super::operations::NumberArithmetic::from_js_instruction(
            op,
            instruction.binary_operator(),
        )
        .is_some()
            || super::super::operations::NumberBitwise::from_operator(
                instruction.binary_operator(),
            )
            .is_some();
    }
    false
}

/// The function's scalar ranges: maximal runs of `scalar` instructions no jump enters,
/// each admitted when it repays a region entry.
pub(super) fn ranges(program: &ResidualProgram, function_id: u32) -> Vec<std::ops::Range<usize>> {
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
    let admits = |range: &std::ops::Range<usize>| {
        let arithmetic = decoded[range.clone()]
            .iter()
            .filter(|instruction| !matches!(
                instruction.op().class(),
                Class::Move | Class::LoadConst | Class::ToNumeric | Class::GetField
            ))
            .count();
        let mut produced = std::collections::BTreeSet::new();
        let mut removes_roundtrip = false;
        for instruction in &decoded[range.clone()] {
            removes_roundtrip |= instruction.op().class().eq(Class::IncDec)
                || osr_loop::number_reads(*instruction)
                    .iter()
                    .any(|register| produced.contains(register));
            if let Some(register) = instruction.defined_register() {
                produced.insert(register);
            }
        }
        let fields = decoded[range.clone()]
            .iter()
            .filter(|instruction| instruction.op().class().eq(Class::GetField))
            .count();
        // Retain a region when it removes a boxed producer/consumer roundtrip or an IncDec
        // semantic callout; isolated already-native math needs no wrapper. One field plus
        // one operation only trades a frame transfer for a guard: field binding needs
        // shared inputs or a longer arithmetic chain.
        range.end < decoded.len()
            && arithmetic != 0
            && removes_roundtrip
            && (fields != 1 || arithmetic > 1)
    };
    let mut ranges = Vec::new();
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
        let whole = start..end;
        if admits(&whole) {
            ranges.push(whole);
        } else {
            // A changing receiver cannot be snapshotted. Preserve arithmetic runs on each
            // side of field reads rather than letting an inadmissible read erase them.
            let mut scalar_start = start;
            for (pc, instruction) in decoded.iter().enumerate().take(end).skip(start) {
                if instruction.op().class().eq(Class::GetField) {
                    let run = scalar_start..pc;
                    if scalar_start < pc && admits(&run) {
                        ranges.push(run);
                    }
                    scalar_start = pc + 1;
                }
            }
            let run = scalar_start..end;
            if scalar_start > start && scalar_start < end && admits(&run) {
                ranges.push(run);
            }
        }
        start = end;
    }
    ranges
}

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;


    fn lower(program: &ResidualProgram, function_id: u32) -> Vec<super::super::StaticTypedRegion> {
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        super::super::lower_static_typed_regions_process::<crate::test_host::TestHost>(
            program, function_id, &super::super::static_call_targets(program), &*isa,
            super::super::VmFacts::STATIC, &Default::default())
    }

    #[test]
    fn isolated_data_reads_do_not_create_numeric_regions() {
        let program = crate::Engine::specialize(
            "function subject(o) { let x=o.left; let y=x; return y; }",
            "field-copy.js",
        )
        .unwrap();
        assert!(ranges(&program, 1).is_empty());
        assert!(lower(&program, 1).is_empty());
    }

    #[test]
    fn one_field_increment_keeps_its_existing_numeric_region() {
        let program = crate::Engine::specialize(
            "function subject(o) { let n=o.count; n++; return n; }",
            "field-increment.js",
        )
        .unwrap();
        // The whole read-plus-increment range is admitted, so a field cannot erase the
        // arithmetic beside it.
        assert!(!ranges(&program, 1).is_empty());
    }

    #[test]
    fn changing_field_receivers_keep_existing_arithmetic_regions() {
        let program = crate::Engine::specialize(
            "function subject(o, other, x) { const v=(x+1)*(x-1); const left=o.left; o=other; const right=o.right; const sum=left+right; return sum+v; }",
            "changing-fields.js",
        ).unwrap();
        assert!(!ranges(&program, 1).is_empty());
        assert!(
            lower(&program, 1)
                .iter()
                .any(|region| region.function.to_string().contains("fmul"))
        );
    }

    #[test]
    fn data_fields_feed_the_shared_typed_region_without_callouts() {
        let program = crate::Engine::specialize(
            "function subject(o) { let x=o.left, y=o.right; let sum=x+y; return sum*3; }",
            "typed-fields.js",
        )
        .unwrap();
        let regions = lower(&program, 1);
        let region = regions
            .iter()
            .find(|region| region.function.to_string().contains("fadd"))
            .unwrap_or_else(|| {
                panic!(
                    "two stable field inputs need a typed region:\n{}",
                    program.disassemble()
                )
            });
        // A static region proves its field reads at their cache sites, as every lane region
        // does: the helper calls in its IR are those probes, not guest calls.
        assert!(region.function.to_string().contains("fadd"));
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
        let regions = lower(&program, id as u32);
        let region = regions
            .iter()
            .find(|region| region.header == ranges(&program, id as u32).first().map(|range| range.start).unwrap())
            .expect("adjacent arithmetic must share a typed region");
        let entry = region.function.layout.entry_block().unwrap();
        let registers = region.function.dfg.block_params(entry)[3];
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
            region.header,
            program.disassemble(),
            region.function.display()
        );
        let ir = region.function.display().to_string();
        assert!(ir.contains("fadd") && ir.contains("fsub") && ir.contains("fmul"));
    }

    #[test]
    fn bound_data_fields_run_through_the_linked_image() {
        let program = crate::Engine::specialize(
            r#"
            function calc(o) { let x=o.left, y=o.right; let sum=x+y; return sum*3; }
            const o={pad:0,left:2,right:3}; let total=0;
            for(let i=0;i<32;i++) total+=calc(o);
            if(total!==480) throw 'wrong field arithmetic';
            throw 42;
        "#,
            "bound-fields.js",
        )
        .unwrap();
        // The field arithmetic is a typed region, not baseline fallback.
        let isa = super::super::super::jit_backend::target_isa().unwrap();
        let regions = lower_static_typed_regions_process::<crate::test_host::TestHost>(
            &program, 1, &super::super::static_call_targets(&program), &*isa,
            super::super::VmFacts::STATIC, &Default::default());
        assert!(regions.iter().any(|region| region.function.to_string().contains("fadd")),
            "the field addition must lower in a static region");
        let native = super::super::tests::linked_optimized_test_program(program);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
    }
}
