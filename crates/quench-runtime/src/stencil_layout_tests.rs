#[cfg(test)]
mod tests {
    use super::*;

    const START: LabelId = LabelId(1);
    const MIDDLE: LabelId = LabelId(2);
    const END: LabelId = LabelId(3);

    fn x86_jump() -> [u8; 5] {
        [0xE9, 0, 0, 0, 0]
    }

    fn x86_fixup(fragment: u8, target: LabelId) -> Fixup {
        Fixup {
            fragment,
            offset: 1,
            target,
            addend: 0,
            kind: FixupKind::X86Rel32,
        }
    }

    #[test]
    fn resolves_distinct_forward_and_backward_labels() {
        let jump = x86_jump();
        let fragments = [
            Fragment {
                label: START,
                bytes: &jump,
            },
            Fragment {
                label: MIDDLE,
                bytes: &jump,
            },
            Fragment {
                label: END,
                bytes: &[0xC3],
            },
        ];
        let fixups = [x86_fixup(0, END), x86_fixup(1, START)];
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &fixups)
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(i32::from_le_bytes(output[1..5].try_into().unwrap()), 5);
        assert_eq!(i32::from_le_bytes(output[6..10].try_into().unwrap()), -10);
    }

    #[test]
    fn fixup_addend_targets_inside_labeled_fragment() {
        let jump = x86_jump();
        let fragments = [
            Fragment {
                label: START,
                bytes: &jump,
            },
            Fragment {
                label: END,
                bytes: &[0x90, 0xC3],
            },
        ];
        let fixup = Fixup {
            addend: 1,
            ..x86_fixup(0, END)
        };
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &[fixup])
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(i32::from_le_bytes(output[1..5].try_into().unwrap()), 1);
    }

    #[test]
    fn rejects_duplicate_and_undefined_labels_transactionally() {
        let duplicate = [
            Fragment {
                label: START,
                bytes: &[0x90],
            },
            Fragment {
                label: START,
                bytes: &[0xC3],
            },
        ];
        let mut output = vec![0xA5];
        assert_eq!(
            StencilLayout::new(&duplicate, &[]).finalize_into(&mut output),
            Err(LayoutError::DuplicateLabel(START))
        );
        assert_eq!(output, [0xA5]);

        let jump = x86_jump();
        let fragments = [Fragment {
            label: START,
            bytes: &jump,
        }];
        assert_eq!(
            StencilLayout::new(&fragments, &[x86_fixup(0, END)]).finalize_into(&mut output),
            Err(LayoutError::UndefinedLabel(END))
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn rejects_overlap_and_out_of_bounds_transactionally() {
        let bytes = [0xE9, 0, 0, 0, 0, 0xC3];
        let fragments = [Fragment {
            label: START,
            bytes: &bytes,
        }];
        let overlap = [
            x86_fixup(0, START),
            Fixup {
                offset: 2,
                ..x86_fixup(0, START)
            },
        ];
        let mut output = vec![0xA5];
        assert_eq!(
            StencilLayout::new(&fragments, &overlap).finalize_into(&mut output),
            Err(LayoutError::OverlappingFixups)
        );
        let outside = [Fixup {
            offset: 3,
            ..x86_fixup(0, START)
        }];
        assert_eq!(
            StencilLayout::new(&fragments, &outside).finalize_into(&mut output),
            Err(LayoutError::FixupOutOfBounds)
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn enforces_fragment_fixup_and_byte_budgets() {
        let fragment = Fragment {
            label: START,
            bytes: &[],
        };
        let fragments = [fragment; MAX_LAYOUT_FRAGMENTS + 1];
        assert_eq!(
            StencilLayout::new(&fragments, &[]).finalize_into(&mut Vec::new()),
            Err(LayoutError::FragmentBudget)
        );
        let fixup = x86_fixup(0, START);
        let fixups = [fixup; MAX_LAYOUT_FIXUPS + 1];
        assert_eq!(
            StencilLayout::new(&[fragment], &fixups).finalize_into(&mut Vec::new()),
            Err(LayoutError::FixupBudget)
        );
        let oversized = [0u8; MAX_LAYOUT_BYTES + 1];
        let fragments = [Fragment {
            label: START,
            bytes: &oversized,
        }];
        assert_eq!(
            StencilLayout::new(&fragments, &[]).finalize_into(&mut Vec::new()),
            Err(LayoutError::ByteBudget)
        );
    }

    #[test]
    fn aarch64_branch26_to_next_fragment_becomes_fallthrough() {
        let branch = 0x1400_0000u32.to_le_bytes();
        let fragments = [
            Fragment {
                label: START,
                bytes: &branch,
            },
            Fragment {
                label: END,
                bytes: &0xD65F_03C0u32.to_le_bytes(),
            },
        ];
        let fixup = Fixup {
            fragment: 0,
            offset: 0,
            target: END,
            addend: 0,
            kind: FixupKind::Aarch64Branch26,
        };
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &[fixup])
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(output[..4].try_into().unwrap()),
            0xD503_201F
        );
        assert_eq!(&output[4..], &0xD65F_03C0u32.to_le_bytes());
    }

    #[test]
    fn aarch64_branch26_keeps_non_fallthrough_edge() {
        let branch = 0x1400_0000u32.to_le_bytes();
        let fragments = [
            Fragment {
                label: START,
                bytes: &branch,
            },
            Fragment {
                label: MIDDLE,
                bytes: &0xD503_201Fu32.to_le_bytes(),
            },
            Fragment {
                label: END,
                bytes: &0xD65F_03C0u32.to_le_bytes(),
            },
        ];
        let fixup = Fixup {
            fragment: 0,
            offset: 0,
            target: END,
            addend: 0,
            kind: FixupKind::Aarch64Branch26,
        };
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &[fixup])
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(output[..4].try_into().unwrap()),
            0x1400_0002
        );
    }

    #[test]
    fn aarch64_conditional_fixup_targets_symbolic_cold_block() {
        let branch = 0x5400_0001u32.to_le_bytes();
        let fragments = [
            Fragment {
                label: START,
                bytes: &branch,
            },
            Fragment {
                label: MIDDLE,
                bytes: &0xD503_201Fu32.to_le_bytes(),
            },
            Fragment {
                label: END,
                bytes: &0xD65F_03C0u32.to_le_bytes(),
            },
        ];
        let fixup = Fixup {
            fragment: 0,
            offset: 0,
            target: END,
            addend: 0,
            kind: FixupKind::Aarch64CondBranch19,
        };
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &[fixup])
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(output[..4].try_into().unwrap()),
            0x5400_0041
        );
    }

    #[test]
    fn malformed_aarch64_branch_keeps_output_unchanged() {
        let not_branch = 0x5400_0000u32.to_le_bytes();
        let fragments = [Fragment {
            label: START,
            bytes: &not_branch,
        }];
        let fixup = Fixup {
            fragment: 0,
            offset: 0,
            target: START,
            addend: 0,
            kind: FixupKind::Aarch64Branch26,
        };
        let mut output = vec![0xA5];
        assert_eq!(
            StencilLayout::new(&fragments, &[fixup]).finalize_into(&mut output),
            Err(LayoutError::Patch(PatchError::UnsupportedOffset))
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn fallthrough_peephole_is_transactional_when_later_fixup_fails() {
        let mut head = [0u8; 8];
        head[..4].copy_from_slice(&0x1400_0000u32.to_le_bytes());
        head[4..].copy_from_slice(&0x5400_0000u32.to_le_bytes());
        let tail = 0xD65F_03C0u32.to_le_bytes();
        let fragments = [
            Fragment {
                label: START,
                bytes: &head,
            },
            Fragment {
                label: END,
                bytes: &tail,
            },
        ];
        let fixups = [
            Fixup {
                fragment: 0,
                offset: 0,
                target: END,
                addend: -4,
                kind: FixupKind::Aarch64Branch26,
            },
            Fixup {
                fragment: 0,
                offset: 4,
                target: END,
                addend: 0,
                kind: FixupKind::Aarch64Branch26,
            },
        ];
        let mut output = vec![0xA5, 0x5A];
        assert_eq!(
            StencilLayout::new(&fragments, &fixups).finalize_into(&mut output),
            Err(LayoutError::Patch(PatchError::UnsupportedOffset))
        );
        assert_eq!(output, [0xA5, 0x5A]);
    }

    const REL32_HOLE: crate::stencil_fact::Hole = crate::stencil_fact::Hole {
        offset: 1,
        kind: HoleKind::Rel32,
    };
    const JUMP_BYTES: [u8; 5] = [0xE9, 0, 0, 0, 0];
    const JUMP_STENCIL: Stencil = Stencil {
        bytes: &JUMP_BYTES,
        holes: &[REL32_HOLE],
    };
    const RETURN_STENCIL: Stencil = Stencil {
        bytes: &[0xC3],
        holes: &[],
    };

    fn stencil_fragments(values: PatchValues<'_, 1>) -> [StencilFragment<'static, '_, 1>; 3] {
        [
            StencilFragment {
                label: START,
                stencil: &JUMP_STENCIL,
                values,
            },
            StencilFragment {
                label: MIDDLE,
                stencil: &JUMP_STENCIL,
                values,
            },
            StencilFragment {
                label: END,
                stencil: &RETURN_STENCIL,
                values,
            },
        ]
    }

    fn patch_values(site: &crate::quickening::QuickeningSite<1>) -> PatchValues<'_, 1> {
        PatchValues::from_site(site)
    }

    #[test]
    fn composes_three_stencils_from_declared_successors() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let fragments = stencil_fragments(patch_values(&site));
        let fixups = [x86_fixup(0, MIDDLE), x86_fixup(1, END)];
        let mut output = Vec::new();
        compose_region(&fragments, &fixups, &mut output).unwrap();
        assert_eq!(i32::from_le_bytes(output[1..5].try_into().unwrap()), 0);
        assert_eq!(i32::from_le_bytes(output[6..10].try_into().unwrap()), 0);
        assert_eq!(output[10], 0xC3);
    }

    #[test]
    fn stencil_composition_rejects_missing_and_duplicate_edges_transactionally() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let values = patch_values(&site);
        let fragments = stencil_fragments(values);
        let mut output = vec![0xA5];
        let one_edge = [x86_fixup(0, MIDDLE)];
        assert_eq!(
            compose_region(&fragments, &one_edge, &mut output),
            Err(LayoutError::MissingFixup(1, 1))
        );
        let duplicate = [x86_fixup(0, MIDDLE), x86_fixup(0, END), x86_fixup(1, END)];
        assert_eq!(
            compose_region(&fragments, &duplicate, &mut output),
            Err(LayoutError::DuplicateFixup(0, 1))
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn stencil_composition_rejects_undeclared_edge_transactionally() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let fragments = [StencilFragment {
            label: START,
            stencil: &RETURN_STENCIL,
            values: patch_values(&site),
        }];
        let fixups = [x86_fixup(0, START)];
        let mut output = vec![0xA5];
        assert_eq!(
            compose_region(&fragments, &fixups, &mut output),
            Err(LayoutError::UnexpectedFixup(0, 1))
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn stencil_composition_enforces_budgets_before_copying() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let values = patch_values(&site);
        let fragment = StencilFragment {
            label: START,
            stencil: &RETURN_STENCIL,
            values,
        };
        let fragments = [fragment; MAX_LAYOUT_FRAGMENTS + 1];
        let mut output = vec![0xA5];
        assert_eq!(
            compose_region(&fragments, &[], &mut output),
            Err(LayoutError::FragmentBudget)
        );

        const TOO_MANY: [crate::stencil_fact::Hole; MAX_LAYOUT_HOLES + 1] =
            [REL32_HOLE; MAX_LAYOUT_HOLES + 1];
        const OVERFULL: Stencil = Stencil {
            bytes: &JUMP_BYTES,
            holes: &TOO_MANY,
        };
        let fragments = [StencilFragment {
            label: START,
            stencil: &OVERFULL,
            values,
        }];
        assert_eq!(
            compose_region(&fragments, &[], &mut output),
            Err(LayoutError::HoleBudget)
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn stencil_composition_patches_literal_and_successor_from_one_view() {
        const BYTES: [u8; 13] = [0, 0, 0, 0, 0, 0, 0, 0, 0xE9, 0, 0, 0, 0];
        const HOLES: [crate::stencil_fact::Hole; 2] = [
            crate::stencil_fact::Hole {
                offset: 0,
                kind: HoleKind::Literal64,
            },
            crate::stencil_fact::Hole {
                offset: 9,
                kind: HoleKind::Rel32,
            },
        ];
        const HEAD: Stencil = Stencil {
            bytes: &BYTES,
            holes: &HOLES,
        };
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let values = patch_values(&site).with_constant_bits(0x0123_4567_89ab_cdef);
        let fragments = [
            StencilFragment {
                label: START,
                stencil: &HEAD,
                values,
            },
            StencilFragment {
                label: END,
                stencil: &RETURN_STENCIL,
                values,
            },
        ];
        let fixup = Fixup {
            offset: 9,
            ..x86_fixup(0, END)
        };
        let mut output = Vec::new();
        compose_region(&fragments, &[fixup], &mut output).unwrap();
        assert_eq!(&output[..8], &0x0123_4567_89ab_cdefu64.to_le_bytes());
        assert_eq!(i32::from_le_bytes(output[9..13].try_into().unwrap()), 0);
        assert_eq!(output[13], 0xC3);
    }

    #[test]
    fn each_stencil_fragment_owns_its_patch_bindings() {
        const BYTES: [u8; 8] = [0; 8];
        const HOLES: [crate::stencil_fact::Hole; 1] = [crate::stencil_fact::Hole {
            offset: 0,
            kind: HoleKind::Literal64,
        }];
        const LITERAL: Stencil = Stencil {
            bytes: &BYTES,
            holes: &HOLES,
        };
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::LoadConst);
        let first = patch_values(&site).with_constant_bits(0x1111_2222_3333_4444);
        let second = patch_values(&site).with_constant_bits(0xaaaa_bbbb_cccc_dddd);
        let fragments = [
            StencilFragment {
                label: START,
                stencil: &LITERAL,
                values: first,
            },
            StencilFragment {
                label: END,
                stencil: &LITERAL,
                values: second,
            },
        ];
        let mut output = Vec::new();
        compose_region(&fragments, &[], &mut output).unwrap();
        assert_eq!(&output[..8], &0x1111_2222_3333_4444u64.to_le_bytes());
        assert_eq!(&output[8..], &0xaaaa_bbbb_cccc_ddddu64.to_le_bytes());
    }
}
