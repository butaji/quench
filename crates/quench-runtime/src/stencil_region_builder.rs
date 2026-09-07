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
use std::cell::RefCell;
use std::rc::Rc;

const LINEAR_COMPOSITION_ID: RegionId = RegionId(0x5143_0001);

type F64Entry = extern "C" fn(f64, f64) -> f64;

pub(crate) struct NativeLinearF64Plan {
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<F64Entry>>,
    view: PhysicalStencilView,
    #[cfg(test)]
    entries: u64,
    #[cfg(test)]
    last_entered: bool,
}

impl NativeLinearF64Plan {
    pub(crate) fn repeated_add(
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.native_leaves.then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::fallthrough_region_key(),
            crate::stencil_select::RegionAbi::ScalarF64Binary,
        )?;
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::Add);
        let values = PatchValues::from_site(&site);
        let image = compose_linear_chain(view, 2, &values).ok()?;
        Some(Self {
            owner,
            image,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
            view,
            #[cfg(test)]
            entries: 0,
            #[cfg(test)]
            last_entered: false,
        })
    }

    pub(crate) fn execute(&mut self, lhs: f64, rhs: f64) -> Option<f64> {
        if let Some(entry) = self.installed {
            if let Ok(value) = self.invoke(entry, lhs, rhs) {
                return Some(value);
            }
            self.installed = None;
        }
        let address = self
            .owner
            .borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .ok()?;
        let entry = self.owner.borrow().owned_f64_entry(address).ok()?;
        self.installed = Some(entry);
        self.invoke(entry, lhs, rhs).ok()
    }

    fn invoke(
        &mut self,
        entry: crate::stencil_arena::EntryToken<F64Entry>,
        lhs: f64,
        rhs: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry)?;
        let value = lease.invoke(|call| call(lhs, rhs))?;
        #[cfg(test)]
        {
            self.entries = self.entries.saturating_add(1);
            self.last_entered = true;
        }
        Ok(value)
    }

    #[cfg(test)]
    pub(crate) const fn native_entry_count(&self) -> u64 {
        self.entries
    }

    #[cfg(test)]
    pub(crate) fn last_native_view(&self) -> Option<PhysicalStencilView> {
        self.last_entered.then_some(self.view)
    }
}

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
