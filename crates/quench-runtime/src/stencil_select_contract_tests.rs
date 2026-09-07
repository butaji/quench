use super::*;

#[test]
fn every_generated_region_has_one_external_entry_and_exact_ops() {
    for record in CANONICAL_REGION_TABLE {
        assert_eq!(record.entry, 0);
        assert_eq!(
            select_region(record.key).unwrap().operations,
            record.operations
        );
        assert!(has_single_entry_point(
            u32::from(record.entry),
            &[RegionBlock {
                id: 0,
                predecessors: &[],
                external_entry: true
            }]
        ));
    }
    assert!(!has_single_entry_point(
        0,
        &[
            RegionBlock {
                id: 0,
                predecessors: &[],
                external_entry: true
            },
            RegionBlock {
                id: 1,
                predecessors: &[0],
                external_entry: true
            },
        ]
    ));
}

#[test]
fn generated_abi_classification_matches_physical_entry_shape() {
    for record in CANONICAL_REGION_TABLE {
        match record.abi {
            RegionAbi::ScalarF64Binary | RegionAbi::ScalarF64Unary | RegionAbi::ScalarF64x3 => {
                assert!(!record.stencil.bytes.is_empty());
                assert_ne!(record.stencil.bytes.len(), 44);
                assert_ne!(record.stencil.bytes.len(), 76);
            }
            RegionAbi::TaggedWord => {
                assert!(matches!(record.stencil.bytes.len(), 4 | 8));
                assert!(matches!(
                    record.operations.first(),
                    Some(
                        crate::ir::Opcode::Move
                            | crate::ir::Opcode::LoadLocal
                            | crate::ir::Opcode::StoreLocal
                            | crate::ir::Opcode::GetN
                            | crate::ir::Opcode::SetN,
                    )
                ));
            }
            RegionAbi::PropertyGuard => {
                assert!(
                    matches!(record.stencil.bytes.len(), 48 | 80)
                        || (record.name == "prototype_property"
                            && matches!(record.stencil.bytes.len(), 1 | 292))
                );
                assert_eq!(record.operations, [crate::ir::Opcode::GetN]);
            }
            RegionAbi::PropertyWriteGuard => {
                assert!(matches!(record.stencil.bytes.len(), 48 | 80));
                assert_eq!(record.operations, [crate::ir::Opcode::SetN]);
            }
            RegionAbi::ConstantWord => {
                assert!(matches!(record.stencil.bytes.len(), 11 | 16));
                assert!(matches!(
                    record.operations,
                    [crate::ir::Opcode::LoadConst, crate::ir::Opcode::Return]
                ));
            }
            RegionAbi::ScalarBool => {
                if matches!(record.operations, [crate::ir::Opcode::JumpIfFalse]) {
                    assert!(matches!(record.stencil.bytes.len(), 23 | 28));
                } else {
                    assert!(matches!(
                        record.operations,
                        [crate::ir::Opcode::Binary, crate::ir::Opcode::Return]
                    ));
                    assert!(matches!(record.stencil.bytes.len(), 11 | 12 | 16 | 20));
                }
            }
            RegionAbi::ScalarWordBool => {
                assert_scalar_word_shape(record);
            }
            RegionAbi::ScalarWordPairBool => {
                assert!(matches!(record.stencil.bytes.len(), 10 | 12));
                assert!(matches!(
                    record.operations,
                    [crate::ir::Opcode::Binary, crate::ir::Opcode::Return]
                ));
            }
            RegionAbi::ScalarI32 => {
                assert!(matches!(record.stencil.bytes.len(), 5 | 8));
                assert!(matches!(
                    record.operations,
                    [crate::ir::Opcode::Binary, crate::ir::Opcode::Return]
                        | [crate::ir::Opcode::Unary, crate::ir::Opcode::Return]
                ));
            }
            RegionAbi::ScalarU32 => {
                assert!(matches!(record.stencil.bytes.len(), 7 | 8));
                assert!(record
                    .operations
                    .starts_with(&[crate::ir::Opcode::Binary, crate::ir::Opcode::Return]));
            }
            RegionAbi::Bridge => {
                assert!(
                    matches!(record.stencil.bytes.len(), 12 | 16),
                    "bridge rows use the dispatch trampoline"
                );
            }
            RegionAbi::ArrayKernel => {
                assert!(matches!(record.stencil.bytes.len(), 12 | 20 | 32 | 44))
            }
            RegionAbi::ArrayNumericLoop => assert_eq!(record.stencil.bytes.len(), 100),
            RegionAbi::CompareBranch => {
                assert_eq!(
                    record.operations,
                    [crate::ir::Opcode::Binary, crate::ir::Opcode::JumpIfFalse]
                );
                assert_eq!(record.stencil.bytes.len(), 56);
            }
        }
    }
}

fn assert_scalar_word_shape(record: &RegionRecord) {
    if record.continuation_abi == ContinuationAbi::WordX0 {
        assert!(record.stencil.bytes.is_empty());
        assert!(matches!(
            record.operations,
            [crate::ir::Opcode::JumpIfFalse] | [crate::ir::Opcode::Return]
        ));
        return;
    }
    assert!(matches!(
        record.stencil.bytes.len(),
        6 | 8 | 20 | 24 | 27 | 32
    ));
    assert!(matches!(
        record.operations,
        [crate::ir::Opcode::Unary, crate::ir::Opcode::Return] | [crate::ir::Opcode::JumpIfFalse]
    ));
}

#[test]
fn raw_array_rows_advertise_execution_only_for_their_emitter_target() {
    for record in CANONICAL_REGION_TABLE {
        if matches!(record.abi, RegionAbi::ArrayKernel) {
            assert_eq!(
                record.executable,
                cfg!(target_arch = "aarch64"),
                "raw array ABI must not route trampoline bytes as a kernel"
            );
        }
        if record.name == "prototype_property" {
            assert_eq!(record.executable, cfg!(target_arch = "aarch64"));
        }
    }
}

#[test]
fn generated_contracts_reuse_opcode_effects_and_entry_rules() {
    let scalar = select_region(loop_region_key())
        .expect("scalar row")
        .contract();
    assert_eq!(scalar.abi, RegionAbi::ScalarF64Binary);
    assert!(scalar.has_effect(crate::facts::OperationEffect::MayThrow));
    assert!(!scalar.has_effect(crate::facts::OperationEffect::WriteHeap));
    assert!(scalar.legal_external_entry(0));
    assert!(!scalar.legal_external_entry(1));

    let array = select_region(array_numeric_loop_region_key())
        .expect("numeric loop row")
        .contract();
    assert_eq!(array.abi, RegionAbi::ArrayNumericLoop);
    assert!(array.has_effect(crate::facts::OperationEffect::ReadHeap));
    assert!(array.has_effect(crate::facts::OperationEffect::WriteHeap));
    assert!(array.has_effect(crate::facts::OperationEffect::Control));
    assert!(array.requires_semantic_boundary());
    assert!(array.has_single_entry());
    assert_eq!(
        select_region(property_region_key())
            .expect("property row")
            .abi,
        RegionAbi::PropertyGuard
    );
    assert_eq!(
        select_region(move_region_key()).expect("move row").abi,
        RegionAbi::TaggedWord
    );
}

#[test]
fn generated_region_index_is_total_and_identity_preserving() {
    for (expected, record) in CANONICAL_REGION_TABLE.iter().enumerate() {
        assert_eq!(canonical_region_index(record.key), Some(expected));
        assert!(std::ptr::eq(
            canonical_region_lookup(record.key).expect("declared region"),
            record
        ));
    }
    let unknown = crate::stencil_fact::RegionKey(u64::MAX);
    assert_eq!(canonical_region_index(unknown), None);
    assert!(select_physical(unknown).is_none());
}

#[test]
fn abi_contracts_keep_scalar_bridge_and_raw_entries_distinct() {
    for abi in [
        RegionAbi::ScalarF64Binary,
        RegionAbi::ScalarF64Unary,
        RegionAbi::ScalarF64x3,
    ] {
        assert_eq!(abi.contract().context_arg_words, 0);
        assert!(abi.contract().preserves_vm_registers);
    }
    assert_eq!(RegionAbi::TaggedWord.contract().context_arg_words, 0);
    assert!(RegionAbi::TaggedWord.contract().preserves_vm_registers);
    assert_eq!(RegionAbi::PropertyGuard.contract().context_arg_words, 1);
    assert!(!RegionAbi::PropertyGuard.contract().may_call_helper);
    assert_eq!(
        RegionAbi::PropertyWriteGuard.contract().context_arg_words,
        1
    );
    assert!(!RegionAbi::PropertyWriteGuard.contract().may_call_helper);
    assert_eq!(RegionAbi::ScalarI32.contract().context_arg_words, 0);
    assert!(RegionAbi::ScalarI32.contract().preserves_vm_registers);
    assert_eq!(RegionAbi::ScalarU32.contract().context_arg_words, 0);
    assert!(RegionAbi::ScalarU32.contract().preserves_vm_registers);
    assert!(RegionAbi::Bridge.contract().may_call_helper);
    assert_eq!(RegionAbi::ArrayKernel.contract().context_arg_words, 1);
    assert!(!RegionAbi::ArrayKernel.contract().may_call_helper);
    assert!(!RegionAbi::Bridge.contract().interruptible_backedge);
    assert!(
        RegionAbi::ArrayNumericLoop
            .contract()
            .interruptible_backedge
    );
    for record in CANONICAL_REGION_TABLE {
        assert!(record.contract().abi_is_well_formed());
        assert_eq!(
            record.abi.accepts_region_context(),
            record.contract().abi_contract().context_arg_words == 1
        );
    }
    assert_eq!(
        RegionAbi::ScalarF64Binary.contract().hardware_clobber_mask,
        0
    );
    assert_eq!(RegionAbi::ArrayNumericLoop.contract().live_out_mask, 0x0003);
    assert!(RegionAbi::Bridge.contract().root_materialization_required);
}
