//! Convert verified physical successor links into symbolic region transfers.
//!
//! Machine targets stay in the selected artifact view. Semantic destinations
//! are supplied by role, so linear and branching composition share one path.

use crate::stencil_fact::HoleKind;
use crate::stencil_layout::{FixupKind, LayoutError};
use crate::stencil_region_layout::{PlannedTransfer, RegionPoint};
use crate::stencil_select::{PhysicalLink, PhysicalRelocation, PhysicalStencilView, SuccessorRole};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SuccessorPlacement {
    pub(crate) role: SuccessorRole,
    pub(crate) target: RegionPoint,
}

pub(crate) fn selected_transfers_between(
    view: PhysicalStencilView,
    source: RegionPoint,
    target: RegionPoint,
) -> Result<Vec<PlannedTransfer>, LayoutError> {
    let tail = view.fallthrough.ok_or(LayoutError::MissingSuccessor)?;
    if !view
        .links
        .iter()
        .all(|link| link.role == SuccessorRole::Next && link.target == tail.target)
    {
        return Err(LayoutError::RelocationContract);
    }
    selected_transfers_by_role(
        view,
        source,
        &[SuccessorPlacement {
            role: SuccessorRole::Next,
            target,
        }],
    )
}

pub(crate) fn selected_transfers_by_role(
    view: PhysicalStencilView,
    source: RegionPoint,
    successors: &[SuccessorPlacement],
) -> Result<Vec<PlannedTransfer>, LayoutError> {
    validate_links(view, successors)?;
    if view.generated {
        return generated_transfers(view, source, successors);
    }
    view.links
        .iter()
        .map(|link| transfer_for_link(*link, 0, source, successors))
        .collect()
}

fn validate_links(
    view: PhysicalStencilView,
    successors: &[SuccessorPlacement],
) -> Result<(), LayoutError> {
    let relative_holes = view
        .stencil
        .holes
        .iter()
        .filter(|hole| relative_kind(hole.kind).is_some())
        .count();
    let valid = view.links.len() == relative_holes
        && unique_roles(successors)
        && view
            .links
            .iter()
            .all(|link| has_hole(view, link) && successor_target(successors, link.role).is_some())
        && successors
            .iter()
            .all(|successor| view.links.iter().any(|link| link.role == successor.role));
    valid.then_some(()).ok_or(LayoutError::RelocationContract)
}

fn has_hole(view: PhysicalStencilView, link: &PhysicalLink) -> bool {
    view.stencil
        .holes
        .iter()
        .any(|hole| hole.offset == link.offset && hole.kind == link.kind)
}

fn unique_roles(successors: &[SuccessorPlacement]) -> bool {
    successors.iter().enumerate().all(|(index, successor)| {
        !successors[..index]
            .iter()
            .any(|prior| prior.role == successor.role)
    })
}

fn generated_transfers(
    view: PhysicalStencilView,
    source: RegionPoint,
    successors: &[SuccessorPlacement],
) -> Result<Vec<PlannedTransfer>, LayoutError> {
    if view.relocations.len() != view.links.len() {
        return Err(LayoutError::RelocationContract);
    }
    view.relocations
        .iter()
        .map(|relocation| generated_transfer(view, relocation, source, successors))
        .collect()
}

fn generated_transfer(
    view: PhysicalStencilView,
    relocation: &PhysicalRelocation,
    source: RegionPoint,
    successors: &[SuccessorPlacement],
) -> Result<PlannedTransfer, LayoutError> {
    let link = view
        .links
        .iter()
        .find(|link| relocation_matches(link, relocation))
        .ok_or(LayoutError::RelocationContract)?;
    let addend = i32::try_from(relocation.addend).map_err(|_| LayoutError::RelocationContract)?;
    transfer_for_link(*link, addend, source, successors)
}

fn relocation_matches(link: &&PhysicalLink, relocation: &PhysicalRelocation) -> bool {
    link.offset == relocation.offset
        && link.kind == relocation.kind
        && link.target == relocation.target
}

fn transfer_for_link(
    link: PhysicalLink,
    addend: i32,
    source: RegionPoint,
    successors: &[SuccessorPlacement],
) -> Result<PlannedTransfer, LayoutError> {
    Ok(PlannedTransfer {
        source,
        offset: link.offset,
        target: successor_target(successors, link.role).ok_or(LayoutError::RelocationContract)?,
        addend,
        kind: relative_kind(link.kind).ok_or(LayoutError::RelocationContract)?,
    })
}

fn successor_target(successors: &[SuccessorPlacement], role: SuccessorRole) -> Option<RegionPoint> {
    successors
        .iter()
        .find_map(|successor| (successor.role == role).then_some(successor.target))
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
mod tests {
    use super::*;

    #[test]
    fn successor_roles_are_unique_and_explicit() {
        let target = RegionPoint::Operation(2);
        let next = SuccessorPlacement {
            role: SuccessorRole::Next,
            target,
        };
        assert!(unique_roles(&[next]));
        assert!(!unique_roles(&[next, next]));
        assert_eq!(successor_target(&[next], SuccessorRole::Next), Some(target));
        assert_eq!(successor_target(&[next], SuccessorRole::True), None);
    }
}
