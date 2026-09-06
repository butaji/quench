//! Physical-region planning derived from one selected artifact view.
//!
//! Selection owns ABI and relocation identity. This module turns that data
//! into the bounded symbolic layout consumed by publication; it does not
//! rediscover branch sites from machine bytes or JavaScript opcodes.

use crate::stencil_fact::{HoleKind, PatchValues};
use crate::stencil_layout::{compose_region, Fixup, FixupKind, LabelId, LayoutError};
use crate::stencil_select::{PhysicalRelocation, PhysicalStencilView};

const ENTRY_LABEL: LabelId = LabelId(0);
const FALLTHROUGH_LABEL: LabelId = LabelId(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RegionPoint {
    Operation(u8),
    Exit(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FragmentPlacement {
    pub(crate) label: LabelId,
    pub(crate) point: RegionPoint,
}

pub(crate) fn validate_selected_control(
    view: PhysicalStencilView,
    control: &crate::stencil_cfg::RegionControlPlan,
) -> Result<(), LayoutError> {
    if control.span_len() != view.record.operations.len() || !control.is_linear() {
        return Err(LayoutError::RelocationContract);
    }
    view.fallthrough.ok_or(LayoutError::MissingSuccessor)?;
    let placements = [
        FragmentPlacement {
            label: ENTRY_LABEL,
            point: RegionPoint::Operation(0),
        },
        FragmentPlacement {
            label: FALLTHROUGH_LABEL,
            point: RegionPoint::Operation(1),
        },
    ];
    validate_controlled_fixups(
        control,
        view.record.operations,
        &placements,
        &selected_fixups(view)?,
    )
}

pub(crate) fn validate_controlled_fixups(
    control: &crate::stencil_cfg::RegionControlPlan,
    operations: &[crate::ir::Opcode],
    placements: &[FragmentPlacement],
    fixups: &[Fixup],
) -> Result<(), LayoutError> {
    validate_placements(control, operations, placements)?;
    for fixup in fixups {
        let source = placements
            .get(usize::from(fixup.fragment))
            .ok_or(LayoutError::InvalidFragment(fixup.fragment))?;
        let target = placements
            .iter()
            .find(|placement| placement.label == fixup.target)
            .ok_or(LayoutError::UndefinedLabel(fixup.target))?;
        let RegionPoint::Operation(source) = source.point else {
            return Err(LayoutError::RelocationContract);
        };
        let target = point_pc(control, target.point)?;
        if !control.permits_transfer(operations, usize::from(source), target) {
            return Err(LayoutError::RelocationContract);
        }
    }
    Ok(())
}

fn validate_placements(
    control: &crate::stencil_cfg::RegionControlPlan,
    operations: &[crate::ir::Opcode],
    placements: &[FragmentPlacement],
) -> Result<(), LayoutError> {
    if control.span_len() != operations.len() || placements.len() < operations.len() {
        return Err(LayoutError::RelocationContract);
    }
    for (index, placement) in placements.iter().enumerate() {
        if placements[..index]
            .iter()
            .any(|prior| prior.label == placement.label)
        {
            return Err(LayoutError::RelocationContract);
        }
    }
    for offset in 0..operations.len() {
        let offset = u8::try_from(offset).map_err(|_| LayoutError::RelocationContract)?;
        let count = placements
            .iter()
            .filter(|placement| placement.point == RegionPoint::Operation(offset))
            .count();
        if count != 1 {
            return Err(LayoutError::RelocationContract);
        }
    }
    Ok(())
}

fn point_pc(
    control: &crate::stencil_cfg::RegionControlPlan,
    point: RegionPoint,
) -> Result<usize, LayoutError> {
    match point {
        RegionPoint::Operation(offset) => control
            .start()
            .checked_add(usize::from(offset))
            .filter(|pc| *pc < control.end()),
        RegionPoint::Exit(pc) => (!(control.start()..control.end()).contains(&pc)).then_some(pc),
    }
    .ok_or(LayoutError::RelocationContract)
}

pub(crate) fn validate_compare_branch_control(
    view: PhysicalStencilView,
    control: &crate::stencil_cfg::RegionControlPlan,
) -> Result<(), LayoutError> {
    let operations = view.record.operations;
    if operations != [crate::ir::Opcode::Binary, crate::ir::Opcode::JumpIfFalse]
        || control.terminal_conditional_exits().is_none()
        || control.has_backedge()
    {
        return Err(LayoutError::RelocationContract);
    }
    Ok(())
}

pub(crate) fn compose_selected_region<const N: usize>(
    view: PhysicalStencilView,
    values: &PatchValues<'_, N>,
    output: &mut Vec<u8>,
) -> Result<(), LayoutError> {
    let control = crate::stencil_cfg::RegionControlPlan::linear(0, view.record.operations.len())
        .ok_or(LayoutError::RelocationContract)?;
    compose_selected_controlled_region(view, &control, values, output)
}

pub(crate) fn compose_selected_controlled_region<const N: usize>(
    view: PhysicalStencilView,
    control: &crate::stencil_cfg::RegionControlPlan,
    values: &PatchValues<'_, N>,
    output: &mut Vec<u8>,
) -> Result<(), LayoutError> {
    let tail = view.fallthrough.ok_or(LayoutError::MissingSuccessor)?;
    let fragments = [
        crate::stencil_layout::StencilFragment {
            label: ENTRY_LABEL,
            stencil: view.stencil,
            values: *values,
        },
        crate::stencil_layout::StencilFragment {
            label: FALLTHROUGH_LABEL,
            stencil: tail.stencil,
            values: *values,
        },
    ];
    let fixups = selected_fixups(view)?;
    let placements = [
        FragmentPlacement {
            label: ENTRY_LABEL,
            point: RegionPoint::Operation(0),
        },
        FragmentPlacement {
            label: FALLTHROUGH_LABEL,
            point: RegionPoint::Operation(1),
        },
    ];
    compose_controlled_region(
        control,
        view.record.operations,
        &fragments,
        &placements,
        &fixups,
        output,
    )
}

pub(crate) fn compose_controlled_region<const N: usize>(
    control: &crate::stencil_cfg::RegionControlPlan,
    operations: &[crate::ir::Opcode],
    fragments: &[crate::stencil_layout::StencilFragment<'_, '_, N>],
    placements: &[FragmentPlacement],
    fixups: &[Fixup],
    output: &mut Vec<u8>,
) -> Result<(), LayoutError> {
    if fragments.len() != placements.len() {
        return Err(LayoutError::RelocationContract);
    }
    validate_controlled_fixups(control, operations, placements, fixups)?;
    compose_region(fragments, fixups, output)
}

fn selected_fixups(view: PhysicalStencilView) -> Result<Vec<Fixup>, LayoutError> {
    let tail = view.fallthrough.ok_or(LayoutError::MissingSuccessor)?;
    if view.generated {
        return generated_fixups(view, tail.target);
    }
    view.stencil
        .holes
        .iter()
        .copied()
        .filter(|hole| relative_kind(hole.kind).is_some())
        .map(|hole| physical_fixup(hole.offset, hole.kind, 0))
        .collect()
}

fn generated_fixups(view: PhysicalStencilView, target: &str) -> Result<Vec<Fixup>, LayoutError> {
    if view.relocations.len() != view.stencil.holes.len() {
        return Err(LayoutError::RelocationContract);
    }
    view.relocations
        .iter()
        .map(|relocation| generated_fixup(view, relocation, target))
        .collect()
}

fn generated_fixup(
    view: PhysicalStencilView,
    relocation: &PhysicalRelocation,
    target: &str,
) -> Result<Fixup, LayoutError> {
    let declared = view
        .stencil
        .holes
        .iter()
        .any(|hole| hole.offset == relocation.offset && hole.kind == relocation.kind);
    if !declared || relocation.target != target {
        return Err(LayoutError::RelocationContract);
    }
    let addend = i32::try_from(relocation.addend).map_err(|_| LayoutError::RelocationContract)?;
    physical_fixup(relocation.offset, relocation.kind, addend)
}

fn physical_fixup(offset: u16, kind: HoleKind, addend: i32) -> Result<Fixup, LayoutError> {
    Ok(Fixup {
        fragment: 0,
        offset,
        target: FALLTHROUGH_LABEL,
        addend,
        kind: relative_kind(kind).ok_or(LayoutError::RelocationContract)?,
    })
}

const fn relative_kind(kind: HoleKind) -> Option<FixupKind> {
    match kind {
        HoleKind::Rel32 => Some(FixupKind::X86Rel32),
        HoleKind::Branch26 => Some(FixupKind::Aarch64Branch26),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Opcode;
    use crate::quickening::QuickeningSite;
    use crate::stencil_fact::PatchValues;

    #[test]
    fn canonical_view_derives_every_declared_successor_edge() {
        let key = crate::stencil_select::fallthrough_region_key();
        let view = crate::stencil_select::select_physical(key).expect("fallthrough view");
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let mut bytes = Vec::new();
        compose_selected_region(view, &values, &mut bytes).expect("compose selected view");
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
        let mut bytes = vec![1, 2, 3];
        assert_eq!(
            compose_selected_region(bad_view, &values, &mut bytes),
            Err(LayoutError::RelocationContract)
        );
        assert_eq!(bytes, [1, 2, 3]);
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
        let mut output = vec![9, 8, 7];
        assert_eq!(
            compose_selected_controlled_region(view, &short, &values, &mut output),
            Err(LayoutError::RelocationContract)
        );
        assert_eq!(output, [9, 8, 7]);
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
                &[fixup(1, ENTRY_LABEL)],
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

    fn baseline_entry(instruction: crate::ir::Instruction) -> crate::machine::BaselineEntry {
        crate::machine::BaselineEntry {
            instruction,
            handler: instruction.opcode.handler(),
            control: instruction.opcode.control_operands(instruction),
        }
    }
}
