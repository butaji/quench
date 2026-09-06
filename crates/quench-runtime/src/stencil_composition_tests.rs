#[cfg(test)]
mod composition_tests {
    use super::*;

    const START: LabelId = LabelId(1);
    const MIDDLE: LabelId = LabelId(2);
    const END: LabelId = LabelId(3);
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

    fn patch_values(site: &crate::quickening::QuickeningSite<1>) -> PatchValues<'_, 1> {
        PatchValues::from_site(site)
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

    fn stencil_fragments(values: PatchValues<'_, 1>) -> [StencilFragment<'static, '_, 1>; 3] {
        [
            bound(START, &JUMP_STENCIL, values),
            bound(MIDDLE, &JUMP_STENCIL, values),
            bound(END, &RETURN_STENCIL, values),
        ]
    }

    fn bound<'values>(
        label: LabelId,
        stencil: &'static Stencil,
        values: PatchValues<'values, 1>,
    ) -> StencilFragment<'static, 'values, 1> {
        StencilFragment {
            label,
            stencil,
            values,
        }
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
    fn rejects_missing_and_duplicate_edges_transactionally() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let fragments = stencil_fragments(patch_values(&site));
        let mut output = vec![0xA5];
        assert_eq!(
            compose_region(&fragments, &[x86_fixup(0, MIDDLE)], &mut output),
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
    fn rejects_undeclared_edge_transactionally() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let fragments = [bound(START, &RETURN_STENCIL, patch_values(&site))];
        let mut output = vec![0xA5];
        assert_eq!(
            compose_region(&fragments, &[x86_fixup(0, START)], &mut output),
            Err(LayoutError::UnexpectedFixup(0, 1))
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn rejects_invalid_labels_before_patching() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let values = patch_values(&site);
        let fragment = bound(START, &RETURN_STENCIL, values);
        let mut output = vec![0xA5];
        assert_eq!(
            compose_region(&[fragment, fragment], &[], &mut output),
            Err(LayoutError::DuplicateLabel(START))
        );
        let jump = bound(START, &JUMP_STENCIL, values);
        assert_eq!(
            compose_region(&[jump], &[x86_fixup(0, END)], &mut output),
            Err(LayoutError::UndefinedLabel(END))
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn enforces_budgets_before_copying() {
        let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
        let values = patch_values(&site);
        let fragment = bound(START, &RETURN_STENCIL, values);
        let fragments = [fragment; MAX_LAYOUT_FRAGMENTS + 1];
        let mut output = vec![0xA5];
        assert_eq!(
            compose_region(&fragments, &[], &mut output),
            Err(LayoutError::FragmentBudget)
        );
        assert_hole_budget(values, &mut output);
        assert_eq!(output, [0xA5]);
    }

    fn assert_hole_budget(values: PatchValues<'_, 1>, output: &mut Vec<u8>) {
        const TOO_MANY: [crate::stencil_fact::Hole; MAX_LAYOUT_HOLES + 1] =
            [REL32_HOLE; MAX_LAYOUT_HOLES + 1];
        const OVERFULL: Stencil = Stencil {
            bytes: &JUMP_BYTES,
            holes: &TOO_MANY,
        };
        let fragments = [bound(START, &OVERFULL, values)];
        assert_eq!(
            compose_region(&fragments, &[], output),
            Err(LayoutError::HoleBudget)
        );
    }

    #[test]
    fn patches_literal_and_successor_from_one_view() {
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
            bound(START, &HEAD, values),
            bound(END, &RETURN_STENCIL, values),
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
    fn each_fragment_owns_its_patch_bindings() {
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
        let fragments = [bound(START, &LITERAL, first), bound(END, &LITERAL, second)];
        let mut output = Vec::new();
        compose_region(&fragments, &[], &mut output).unwrap();
        assert_eq!(&output[..8], &0x1111_2222_3333_4444u64.to_le_bytes());
        assert_eq!(&output[8..], &0xaaaa_bbbb_cccc_ddddu64.to_le_bytes());
    }
}
