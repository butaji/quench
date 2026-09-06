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

pub(crate) fn validate_selected_control(
    view: PhysicalStencilView,
    control: &crate::stencil_cfg::RegionControlPlan,
) -> Result<(), LayoutError> {
    if control.span_len() != view.record.operations.len() || !control.is_linear() {
        return Err(LayoutError::RelocationContract);
    }
    view.fallthrough
        .is_some()
        .then_some(())
        .ok_or(LayoutError::MissingSuccessor)
}

pub(crate) fn compose_selected_region<const N: usize>(
    view: PhysicalStencilView,
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
    compose_region(&fragments, &fixups, output)
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
    }
}
