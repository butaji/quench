#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Opcode;
    use crate::quickening::QuickeningSite;
    use crate::stencil_fact::{Hole, PatchValues, Stencil};

    const CONDITIONAL_BYTES: &[u8] = &[
        0x00, 0x00, 0x00, 0x54, // b.eq target
        0x00, 0x00, 0x00, 0x14, // b other
    ];
    const CONDITIONAL_HOLES: &[Hole] = &[
        Hole {
            offset: 0,
            kind: HoleKind::CondBranch19,
        },
        Hole {
            offset: 4,
            kind: HoleKind::Branch26,
        },
    ];
    const EXIT_BYTES: &[u8] = &[0xc0, 0x03, 0x5f, 0xd6];
    const NO_HOLES: &[Hole] = &[];
    const CONDITIONAL_STENCIL: Stencil = Stencil {
        bytes: CONDITIONAL_BYTES,
        holes: CONDITIONAL_HOLES,
    };
    const EXIT_STENCIL: Stencil = Stencil {
        bytes: EXIT_BYTES,
        holes: NO_HOLES,
    };

    #[test]
    fn canonical_view_derives_every_declared_successor_edge() {
        let key = crate::stencil_select::fallthrough_region_key();
        let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let image = compose_selected_region(view, &values).expect("compose selected view");
        let bytes = image.bytes();
        assert_eq!(image.cache_signature(), view.cache_signature(&values));
        assert_eq!(
            bytes.len(),
            view.stencil.bytes.len() + view.fallthrough.unwrap().stencil.bytes.len()
        );
        assert_ne!(bytes, view.stencil.bytes);
    }

    #[test]
    fn mismatched_selected_relocation_is_transactional() {
        let key = crate::stencil_select::fallthrough_region_key();
        let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
        if !view.generated {
            return;
        }
        let bad = Box::leak(Box::new([PhysicalRelocation {
            target: "not_the_declared_successor",
            ..view.relocations[0]
        }]));
        let bad_view = PhysicalStencilView {
            relocations: bad,
            ..view
        };
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        assert_eq!(
            compose_selected_region(bad_view, &values).map(|_| ()),
            Err(LayoutError::RelocationContract)
        );
    }

    #[test]
    fn selected_control_must_match_the_residual_span() {
        let key = crate::stencil_select::fallthrough_region_key();
        let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
        let valid = crate::stencil_cfg::RegionControlPlan::linear(7, 2).expect("linear plan");
        let short = crate::stencil_cfg::RegionControlPlan::linear(7, 1).expect("short plan");
        assert_eq!(validate_selected_control(view, &valid), Ok(()));
        assert_eq!(
            validate_selected_control(view, &short),
            Err(LayoutError::RelocationContract)
        );
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        assert_eq!(
            compose_selected_controlled_region(view, &short, &values).map(|_| ()),
            Err(LayoutError::RelocationContract)
        );
    }

    #[test]
    fn compare_branch_control_requires_two_terminal_exits() {
        let instructions = [
            crate::ir::Instruction::binary_operator(0, crate::ops::BinaryOp::LessThan, 1, 2),
            crate::ir::Instruction::jump_if_false(0, 3),
            crate::ir::Instruction::ret(0),
            crate::ir::Instruction::ret(1),
        ];
        let entries: Vec<_> = instructions.iter().copied().map(baseline_entry).collect();
        let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 4]);
        let control = facts.region_control(0, 2).expect("branch control");
        let view = crate::stencil_select::select_physical(
            crate::stencil_select::compare_less_branch_region_key(),
        )
        .expect("compare branch view");
        assert_eq!(validate_compare_branch_control(view, &control), Ok(()));
        let linear = crate::stencil_cfg::RegionControlPlan::linear(0, 2).unwrap();
        assert_eq!(
            validate_compare_branch_control(view, &linear),
            Err(LayoutError::RelocationContract)
        );
    }

    #[test]
    fn controlled_fixups_follow_canonical_branch_and_backedge_edges() {
        let instructions = [
            crate::ir::Instruction::jump_if_false(0, 0),
            crate::ir::Instruction::ret(0),
        ];
        let entries: Vec<_> = instructions.iter().copied().map(baseline_entry).collect();
        let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 2]);
        let control = facts.region_control(0, 2).expect("bounded branch loop");
        let placements = placements();
        let backedge = fixup(0, ENTRY_LABEL);
        let fallthrough = fixup(0, FALLTHROUGH_LABEL);
        assert!(validate_controlled_fixups(
            &control,
            &[Opcode::JumpIfFalse, Opcode::Return],
            &placements,
            &[backedge, fallthrough],
        )
        .is_ok());
        assert_eq!(
            validate_controlled_fixups(
                &control,
                &[Opcode::JumpIfFalse, Opcode::Return],
                &placements,
                &[backedge],
            ),
            Err(LayoutError::RelocationContract)
        );
    }

    #[test]
    fn controlled_fixups_reject_duplicate_or_missing_edges() {
        let instructions = [
            crate::ir::Instruction::jump_if_false(0, 0),
            crate::ir::Instruction::ret(0),
        ];
        let entries: Vec<_> = instructions.iter().copied().map(baseline_entry).collect();
        let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None]);
        let control = facts.region_control(0, 1).expect("bounded branch exits");
        let placements = [
            FragmentPlacement {
                label: ENTRY_LABEL,
                point: RegionPoint::Operation(0),
            },
            FragmentPlacement {
                label: FALLTHROUGH_LABEL,
                point: RegionPoint::Exit(1),
            },
        ];
        let backedge = fixup(0, ENTRY_LABEL);
        let exit = fixup(0, FALLTHROUGH_LABEL);
        assert_eq!(
            validate_controlled_fixups(&control, &[Opcode::JumpIfFalse], &placements, &[backedge]),
            Err(LayoutError::RelocationContract)
        );
        assert_eq!(
            validate_controlled_fixups(
                &control,
                &[Opcode::JumpIfFalse],
                &placements,
                &[backedge, exit, exit],
            ),
            Err(LayoutError::RelocationContract)
        );
    }

    #[test]
    fn controlled_fixups_preserve_exact_external_exit_pcs() {
        let instructions = [
            crate::ir::Instruction::jump_if_false(0, 2),
            crate::ir::Instruction::ret(0),
            crate::ir::Instruction::ret(1),
        ];
        let entries: Vec<_> = instructions.iter().copied().map(baseline_entry).collect();
        let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 3]);
        let control = facts.region_control(0, 1).expect("conditional exit region");
        let placements = [
            FragmentPlacement {
                label: ENTRY_LABEL,
                point: RegionPoint::Operation(0),
            },
            FragmentPlacement {
                label: FALLTHROUGH_LABEL,
                point: RegionPoint::Exit(1),
            },
            FragmentPlacement {
                label: LabelId(2),
                point: RegionPoint::Exit(2),
            },
        ];
        let exits = [fixup(0, FALLTHROUGH_LABEL), fixup(0, LabelId(2))];
        assert!(
            validate_controlled_fixups(&control, &[Opcode::JumpIfFalse], &placements, &exits,)
                .is_ok()
        );
        let mut invalid = placements;
        invalid[2].point = RegionPoint::Exit(3);
        assert_eq!(
            validate_controlled_fixups(&control, &[Opcode::JumpIfFalse], &invalid, &exits,),
            Err(LayoutError::RelocationContract)
        );
    }

    #[test]
    fn controlled_composition_patches_conditional_external_exits() {
        let instructions = [
            crate::ir::Instruction::jump_if_false(0, 2),
            crate::ir::Instruction::ret(0),
            crate::ir::Instruction::ret(1),
        ];
        let entries: Vec<_> = instructions.iter().copied().map(baseline_entry).collect();
        let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 3]);
        let control = facts.region_control(0, 1).expect("conditional exit region");
        let site = QuickeningSite::<1>::new(Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site);
        let fragments = conditional_fragments(values);
        let placements = conditional_placements();
        let fixups = [
            fixup_at(0, 0, FALLTHROUGH_LABEL),
            fixup_at(0, 4, LabelId(2)),
        ];
        let mut output = Vec::new();
        compose_controlled_region(
            &control,
            &[Opcode::JumpIfFalse],
            &fragments,
            &placements,
            &fixups,
            &mut output,
        )
        .expect("compose conditional exits");
        assert_eq!(output.len(), 16);
        assert_ne!(&output[..8], CONDITIONAL_BYTES);
    }

    #[test]
    fn planned_region_derives_labels_and_branch_bindings_from_cfg_points() {
        let (control, operations) = branched_control();
        let site = QuickeningSite::<1>::new(Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site);
        let fragments = planned_conditional_fragments(values);
        let transfers = planned_conditional_transfers();
        let mut output = Vec::new();
        compose_planned_region(&control, &operations, &fragments, &transfers, &mut output)
            .expect("compose point-bound region");
        assert_eq!(output.len(), 16);
        assert_eq!(
            u32::from_le_bytes(output[0..4].try_into().unwrap()),
            0x5400_0040
        );
        assert_eq!(
            u32::from_le_bytes(output[4..8].try_into().unwrap()),
            0x1400_0002
        );
    }

    #[test]
    fn planned_region_rejects_incomplete_cfg_transactionally() {
        let (control, operations) = branched_control();
        let site = QuickeningSite::<1>::new(Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site);
        let fragments = planned_conditional_fragments(values);
        let transfers = planned_conditional_transfers();
        let mut output = vec![7, 8, 9];
        assert_eq!(
            compose_planned_region(
                &control,
                &operations,
                &fragments,
                &transfers[..1],
                &mut output,
            ),
            Err(LayoutError::RelocationContract)
        );
        assert_eq!(output, [7, 8, 9]);
    }

    #[test]
    fn planned_region_rejects_ambiguous_or_unknown_points() {
        let (control, operations) = branched_control();
        let site = QuickeningSite::<1>::new(Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site);
        let mut fragments = planned_conditional_fragments(values);
        fragments[2].point = RegionPoint::Operation(1);
        let mut output = vec![4, 5, 6];
        assert_eq!(
            compose_planned_region(
                &control,
                &operations,
                &fragments,
                &planned_conditional_transfers(),
                &mut output,
            ),
            Err(LayoutError::RelocationContract)
        );
        let valid = planned_conditional_fragments(values);
        let mut transfers = planned_conditional_transfers();
        transfers[1].target = RegionPoint::Exit(99);
        assert_eq!(
            compose_planned_region(&control, &operations, &valid, &transfers, &mut output),
            Err(LayoutError::RelocationContract)
        );
        assert_eq!(output, [4, 5, 6]);
    }

    fn branched_control() -> (crate::stencil_cfg::RegionControlPlan, [Opcode; 3]) {
        let instructions = [
            crate::ir::Instruction::jump_if_false(0, 2),
            crate::ir::Instruction::move_(0, 1),
            crate::ir::Instruction::ret(0),
        ];
        let entries: Vec<_> = instructions.iter().copied().map(baseline_entry).collect();
        let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 3]);
        let control = facts.region_control(0, 3).expect("branched region");
        (control, [Opcode::JumpIfFalse, Opcode::Move, Opcode::Return])
    }

    fn planned_conditional_fragments(
        values: PatchValues<'_, 1>,
    ) -> [PlannedFragment<'static, '_, 1>; 3] {
        [
            PlannedFragment {
                point: RegionPoint::Operation(0),
                stencil: &CONDITIONAL_STENCIL,
                values,
            },
            PlannedFragment {
                point: RegionPoint::Operation(1),
                stencil: &EXIT_STENCIL,
                values,
            },
            PlannedFragment {
                point: RegionPoint::Operation(2),
                stencil: &EXIT_STENCIL,
                values,
            },
        ]
    }

    fn planned_conditional_transfers() -> [PlannedTransfer; 2] {
        [
            PlannedTransfer {
                source: RegionPoint::Operation(0),
                offset: 0,
                target: RegionPoint::Operation(1),
                addend: 0,
                kind: FixupKind::Aarch64CondBranch19,
            },
            PlannedTransfer {
                source: RegionPoint::Operation(0),
                offset: 4,
                target: RegionPoint::Operation(2),
                addend: 0,
                kind: FixupKind::Aarch64Branch26,
            },
        ]
    }

    fn conditional_fragments(
        values: PatchValues<'_, 1>,
    ) -> [crate::stencil_layout::StencilFragment<'static, '_, 1>; 3] {
        [
            crate::stencil_layout::StencilFragment {
                label: ENTRY_LABEL,
                stencil: &CONDITIONAL_STENCIL,
                values,
            },
            crate::stencil_layout::StencilFragment {
                label: FALLTHROUGH_LABEL,
                stencil: &EXIT_STENCIL,
                values,
            },
            crate::stencil_layout::StencilFragment {
                label: LabelId(2),
                stencil: &EXIT_STENCIL,
                values,
            },
        ]
    }

    fn conditional_placements() -> [FragmentPlacement; 3] {
        [
            FragmentPlacement {
                label: ENTRY_LABEL,
                point: RegionPoint::Operation(0),
            },
            FragmentPlacement {
                label: FALLTHROUGH_LABEL,
                point: RegionPoint::Exit(1),
            },
            FragmentPlacement {
                label: LabelId(2),
                point: RegionPoint::Exit(2),
            },
        ]
    }

    fn placements() -> [FragmentPlacement; 2] {
        [
            FragmentPlacement {
                label: ENTRY_LABEL,
                point: RegionPoint::Operation(0),
            },
            FragmentPlacement {
                label: FALLTHROUGH_LABEL,
                point: RegionPoint::Operation(1),
            },
        ]
    }

    fn fixup(fragment: u8, target: LabelId) -> Fixup {
        Fixup {
            fragment,
            offset: 0,
            target,
            addend: 0,
            kind: FixupKind::Aarch64CondBranch19,
        }
    }

    fn fixup_at(fragment: u8, offset: u16, target: LabelId) -> Fixup {
        Fixup {
            fragment,
            offset,
            target,
            addend: 0,
            kind: if offset == 0 {
                FixupKind::Aarch64CondBranch19
            } else {
                FixupKind::Aarch64Branch26
            },
        }
    }

    fn baseline_entry(instruction: crate::ir::Instruction) -> crate::machine::BaselineEntry {
        crate::machine::BaselineEntry {
            instruction,
            handler: instruction.opcode.handler(),
            control: instruction.opcode.control_operands(instruction),
        }
    }
}
