//! Bounded composition of compatible selected physical fragments.
//!
//! This is deliberately smaller than an assembler: canonical operations own
//! meaning, selected views own bytes and ABI, and this module only repeats one
//! audited internal continuation contract before final layout/publication.

use crate::stencil_fact::{PatchValues, RegionId, RegionKey};
use crate::stencil_layout::LayoutError;
use crate::stencil_region_layout::{
    compose_planned_region, selected_transfers_between, PlannedFragment, RegionImageIdentity,
    RegionPoint, VerifiedRegionImage,
};
use crate::stencil_select::PhysicalStencilView;

const LINEAR_COMPOSITION_ID: RegionId = RegionId(0x5143_0001);

pub(crate) fn compose_linear_chain<const N: usize>(
    view: PhysicalStencilView,
    repetitions: u8,
    values: &PatchValues<'_, N>,
) -> Result<VerifiedRegionImage, LayoutError> {
    let operations = linear_operations(view, repetitions)?;
    let control = crate::stencil_cfg::RegionControlPlan::linear(0, operations.len())
        .ok_or(LayoutError::RelocationContract)?;
    let fragments = linear_fragments(view, repetitions, *values)?;
    let transfers = linear_transfers(view, repetitions)?;
    let mut bytes = Vec::new();
    compose_planned_region(&control, &operations, &fragments, &transfers, &mut bytes)?;
    let identity = RegionImageIdentity {
        key: RegionKey::from_opcodes(LINEAR_COMPOSITION_ID, &operations),
        cache_signature: view.cache_signature(values),
        abi: view.abi,
    };
    Ok(VerifiedRegionImage::from_composed(identity, bytes))
}

fn linear_operations(
    view: PhysicalStencilView,
    repetitions: u8,
) -> Result<Vec<crate::ir::Opcode>, LayoutError> {
    validate_linear_view(view, repetitions)?;
    let mut operations = vec![view.record.operations[0]; usize::from(repetitions)];
    operations.push(view.record.operations[1]);
    Ok(operations)
}

fn validate_linear_view(view: PhysicalStencilView, repetitions: u8) -> Result<(), LayoutError> {
    let contract = view.contract();
    let valid = repetitions > 0
        && usize::from(repetitions) < crate::stencil_layout::MAX_LAYOUT_FRAGMENTS
        && contract.operations.len() == 2
        && contract.has_single_entry()
        && contract.abi_is_well_formed()
        && !contract.template_calls_helper
        && view.fallthrough.is_some();
    valid.then_some(()).ok_or(LayoutError::RelocationContract)
}

fn linear_fragments<'values, const N: usize>(
    view: PhysicalStencilView,
    repetitions: u8,
    values: PatchValues<'values, N>,
) -> Result<Vec<PlannedFragment<'static, 'values, N>>, LayoutError> {
    let mut fragments = Vec::with_capacity(usize::from(repetitions) + 1);
    for operation in 0..repetitions {
        fragments.push(PlannedFragment {
            point: RegionPoint::Operation(operation),
            stencil: view.stencil,
            values,
        });
    }
    fragments.push(PlannedFragment {
        point: RegionPoint::Operation(repetitions),
        stencil: view
            .fallthrough
            .ok_or(LayoutError::MissingSuccessor)?
            .stencil,
        values,
    });
    Ok(fragments)
}

fn linear_transfers(
    view: PhysicalStencilView,
    repetitions: u8,
) -> Result<Vec<crate::stencil_region_layout::PlannedTransfer>, LayoutError> {
    let mut transfers = Vec::new();
    for operation in 0..repetitions {
        transfers.extend(selected_transfers_between(
            view,
            RegionPoint::Operation(operation),
            RegionPoint::Operation(operation + 1),
        )?);
    }
    Ok(transfers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Opcode;
    use crate::quickening::QuickeningSite;

    #[test]
    fn linear_chain_identity_depends_on_semantic_depth() {
        let view =
            crate::stencil_select::select_physical(crate::stencil_select::fallthrough_region_key())
                .expect("fallthrough view");
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let one = compose_linear_chain(view, 1, &values).expect("one operation");
        let two = compose_linear_chain(view, 2, &values).expect("two operations");
        assert_ne!(one.identity().key, two.identity().key);
        assert!(two.bytes().len() > one.bytes().len());
    }

    #[test]
    fn linear_chain_rejects_unlinked_or_helper_views() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let add =
            crate::stencil_select::select_physical(crate::stencil_select::add_const_region_key())
                .expect("whole leaf");
        let bridge =
            crate::stencil_select::select_physical(crate::stencil_select::dispatch_region_key())
                .expect("bridge view");
        assert!(compose_linear_chain(add, 2, &values).is_err());
        assert!(compose_linear_chain(bridge, 2, &values).is_err());
    }
}
