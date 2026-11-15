use super::*;

#[test]
fn native_unsigned_widening_multiply_preserves_lane_products() {
    let isa = super::super::jit_backend::target_isa().unwrap();
    // Exhaust both single-input shuffle encodings and all lane selectors,
    // including the unaffected cases, against scalar unsigned products.
    for shuffle in std::iter::once(None).chain(
        (0..=u8::MAX).flat_map(|mask| [Some((mask, false)), Some((mask, true))]),
    ) {
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 3]);
        let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);
        let args = builder.block_params(block).to_vec();
        let mut input = |address| {
            let value = builder.ins().load(types::I8X16, MemFlagsData::trusted(), address, 0);
            let value = if let Some((selector, rhs)) = shuffle {
                let lanes: Vec<u8> = (0..16).map(|byte| {
                    ((selector >> ((byte / 4) * 2)) & 3) * 4 + byte % 4 + if rhs { 16 } else { 0 }
                }).collect();
                let mask = builder.func.dfg.immediates.push(ir::ConstantData::from(lanes));
                let poison = builder.func.dfg.constants.insert(ir::ConstantData::from(vec![0x5a; 16]));
                let poison = builder.ins().vconst(types::I8X16, poison);
                let (left, right) = if rhs { (poison, value) } else { (value, poison) };
                builder.ins().shuffle(left, right, mask)
            } else { value };
            builder.ins().bitcast(types::I32X4, MemFlagsData::new().with_endianness(ir::Endianness::Little), value)
        };
        let left = input(args[0]);
        let right = input(args[1]);
        let left = builder.ins().uwiden_low(left);
        let right = builder.ins().uwiden_low(right);
        let product = builder.ins().imul(left, right);
        builder.ins().store(MemFlagsData::trusted(), product, args[2], 0);
        builder.ins().return_(&[]);
        builder.finalize(isa.frontend_config());
        let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: generated code reads two four-u32 inputs and writes two u64 outputs.
        // All arrays below outlive the call, as does the executable code mapping.
        let multiply = unsafe {
            std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const u32, *const u32, *mut u64)>(code.pointer())
        };
        let cases = [0, 1, 2, 0xffff, 0x10000, 0x7fff_ffff, 0x8000_0000, u32::MAX];
        for a in cases {
            for b in cases {
                // Different lane values and poisoned unused lanes expose lane selection
                // errors as well as accidental signed extension or truncation.
                let left = [a, !b, u32::MAX, 0];
                let right = [b, !a, 0, u32::MAX];
                let mut actual = [0u64; 2];
                unsafe { multiply(left.as_ptr(), right.as_ptr(), actual.as_mut_ptr()) };
                let indices = shuffle.map_or([0, 1], |(selector, _)| [usize::from(selector & 3), usize::from((selector >> 2) & 3)]);
                let expected = indices.map(|index| u64::from(left[index]) * u64::from(right[index]));
                assert_eq!(actual, expected, "shuffle {shuffle:?}, inputs {left:?} * {right:?}");
            }
        }
    }
}

#[test]
fn native_entry_sampling_is_sparse_and_deterministic() {
    assert!(!native_entry_sample_due(0));
    assert!(!native_entry_sample_due(1));
    assert!(native_entry_sample_due(JIT_NATIVE_ENTRY_SAMPLE_INTERVAL));
    assert!(!native_entry_sample_due(
        JIT_NATIVE_ENTRY_SAMPLE_INTERVAL + 1
    ));
    assert!(native_entry_sample_due(
        JIT_NATIVE_ENTRY_SAMPLE_INTERVAL * 2
    ));
}

#[test]
fn root_only_jit_defaults_to_none_and_accepts_explicit_opt_level() {
    assert_eq!(parse_jit_root_opt_level(true, None), Ok("none"));
    assert_eq!(parse_jit_root_opt_level(false, None), Ok("speed"));
    assert_eq!(parse_jit_root_opt_level(true, Some("none")), Ok("none"));
    assert_eq!(parse_jit_root_opt_level(true, Some("speed")), Ok("speed"));
    assert_eq!(
        parse_jit_root_opt_level(true, Some("speed_and_size")),
        Ok("speed_and_size")
    );
    assert_eq!(parse_jit_root_opt_level(true, Some("invalid")), Err(()));
}

#[test]
fn sparse_analysis_states_preserve_register_joins() {
    assert_eq!(Slot::Memory32Address.join(Slot::Memory32Address), Slot::Memory32Address);
    assert_eq!(Slot::Memory32Address.join(Slot::Value(Kind::I64)), Slot::Value(Kind::I64));
    assert_eq!(Slot::Value(Kind::I64).join(Slot::Memory32Address), Slot::Value(Kind::I64));
    assert_eq!(Slot::Memory32Address.join(Slot::DEFAULT), Slot::Conflict);
    const FACTS: [Slot; 9] = [
        Slot::DEFAULT,
        Slot::Value(Kind::I64),
        Slot::Memory32Address,
        Slot::Vector,
        Slot::Binding(7),
        Slot::Memory,
        Slot::Import(9),
        Slot::IndirectReference {
            signature: 11,
            table: 13,
        },
        Slot::Conflict,
    ];
    let states = FACTS
        .iter()
        .flat_map(|first| {
            FACTS.iter().flat_map(move |second| {
                FACTS.iter().map(move |third| vec![*first, *second, *third])
            })
        })
        .collect::<Vec<_>>();
    let sparse = |dense: &[Slot]| {
        let registers = u16::try_from(dense.len()).expect("small test state");
        let mut state = AnalysisState::new();
        for (index, slot) in dense.iter().copied().enumerate() {
            state
                .set(
                    u16::try_from(index).expect("small test register"),
                        registers,
                        slot,
                    )
                    .expect("register in range");
            }
            state
        };

        for left in &states {
            for right in &states {
                let mut expected = left.clone();
                let mut changed = false;
                for (slot, incoming) in expected.iter_mut().zip(right) {
                    let joined = slot.join(*incoming);
                    changed |= joined != *slot;
                    *slot = joined;
                }

                let mut actual = sparse(left);
                assert_eq!(actual.join_from(&sparse(right)), changed);
                for (index, expected) in expected.iter().copied().enumerate() {
                    assert_eq!(actual.get(index as u16, 3), Some(expected));
                }
            }
        }

        // Once a loop backedge turns a fact into Conflict, later iterations
        // must not make it precise again.
        let mut loop_state = AnalysisState::new();
        let mut incoming = AnalysisState::new();
        incoming
            .set(0, 1, Slot::Value(Kind::I64))
            .expect("register in range");
        assert!(loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert!(!loop_state.join_from(&AnalysisState::new()));
        assert!(!loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert_eq!(loop_state.get(1, 1), None);

        loop_state
            .set(0, 1, Slot::DEFAULT)
            .expect("register in range");
        assert_eq!(loop_state.get(0, 1), Some(Slot::DEFAULT));
    }
