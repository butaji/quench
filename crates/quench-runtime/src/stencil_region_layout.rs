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
    validate_edge_coverage(control, operations, placements, fixups)
}

fn validate_edge_coverage(
    control: &crate::stencil_cfg::RegionControlPlan,
    operations: &[crate::ir::Opcode],
    placements: &[FragmentPlacement],
    fixups: &[Fixup],
) -> Result<(), LayoutError> {
    for edge in control.edges() {
        let offset = edge.from.saturating_sub(control.start());
        let Some(opcode) = operations.get(offset) else {
            return Err(LayoutError::RelocationContract);
        };
        if !matches!(
            opcode.control_flow(),
            crate::facts::ControlFlow::Branch | crate::facts::ControlFlow::Jump
        ) {
            continue;
        }
        let expected = control
            .edges()
            .iter()
            .filter(|candidate| *candidate == edge)
            .count();
        let actual = fixups
            .iter()
            .filter(|fixup| fixup_edge(control, placements, **fixup) == Some(*edge))
            .count();
        if actual != expected {
            return Err(LayoutError::RelocationContract);
        }
    }
    Ok(())
}

fn fixup_edge(
    control: &crate::stencil_cfg::RegionControlPlan,
    placements: &[FragmentPlacement],
    fixup: Fixup,
) -> Option<crate::stencil_cfg::RegionEdge> {
    let RegionPoint::Operation(source) = placements.get(usize::from(fixup.fragment))?.point else {
        return None;
    };
    let target = placements.iter().find(|item| item.label == fixup.target)?;
    Some(crate::stencil_cfg::RegionEdge {
        from: control.start().checked_add(usize::from(source))?,
        to: point_pc(control, target.point).ok()?,
    })
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
        HoleKind::CondBranch19 => Some(FixupKind::Aarch64CondBranch19),
        _ => None,
    }
}

#[cfg(test)]
include!("stencil_region_layout_tests.rs");
