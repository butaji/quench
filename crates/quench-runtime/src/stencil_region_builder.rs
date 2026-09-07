//! Bounded composition of compatible selected physical fragments.
//!
//! This is deliberately smaller than an assembler: canonical operations own
//! meaning, selected views own bytes and ABI, and this module only repeats one
//! audited internal continuation contract before final layout/publication.

use crate::stencil_fact::{PatchValues, RegionId, RegionKey};
use crate::stencil_layout::LayoutError;
use crate::stencil_region_layout::{
    compose_planned_region, PlannedFragment, RegionImageIdentity, RegionPoint, VerifiedRegionImage,
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
    #[cfg(test)]
    witness: NativeLinearWitness,
    #[cfg(test)]
    entries: u64,
    #[cfg(test)]
    last_entered: bool,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeLinearWitness {
    pub(crate) identity: RegionImageIdentity,
    pub(crate) fragments: u8,
    pub(crate) generated_fragments: u8,
}

impl NativeLinearF64Plan {
    pub(crate) fn binary_series(
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
        series: crate::stencil_plan::NumericSeries,
    ) -> Option<Self> {
        policy.native_leaves.then_some(())?;
        let views = series
            .operations()
            .map(series_view)
            .collect::<Option<Vec<_>>>()?;
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::Add);
        let values = PatchValues::from_site(&site);
        let image = compose_fragment_chain(&views, &values).ok()?;
        #[cfg(test)]
        let witness = linear_witness(&image, &views)?;
        Some(Self {
            owner,
            image,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
            #[cfg(test)]
            witness,
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
    pub(crate) fn last_native_witness(&self) -> Option<NativeLinearWitness> {
        self.last_entered.then_some(self.witness)
    }
}

#[cfg(test)]
fn linear_witness(
    image: &VerifiedRegionImage,
    views: &[PhysicalStencilView],
) -> Option<NativeLinearWitness> {
    Some(NativeLinearWitness {
        identity: image.identity(),
        fragments: u8::try_from(views.len()).ok()?,
        generated_fragments: u8::try_from(views.iter().filter(|view| view.generated).count())
            .ok()?,
    })
}

fn series_view(operator: crate::ops::BinaryOp) -> Option<PhysicalStencilView> {
    let opcode = match operator {
        crate::ops::BinaryOp::Add => crate::ir::Opcode::Add,
        crate::ops::BinaryOp::Subtract => crate::ir::Opcode::Sub,
        crate::ops::BinaryOp::Multiply => crate::ir::Opcode::Mul,
        crate::ops::BinaryOp::Divide => crate::ir::Opcode::Div,
        _ => return None,
    };
    let key = crate::stencil_select::continuation_region_key(opcode)?;
    crate::stencil_select::select_physical_for_abi(
        key,
        crate::stencil_select::RegionAbi::ScalarF64Binary,
    )
}

pub(crate) fn compose_linear_chain<const N: usize>(
    view: PhysicalStencilView,
    repetitions: u8,
    values: &PatchValues<'_, N>,
) -> Result<VerifiedRegionImage, LayoutError> {
    validate_linear_view(view, repetitions)?;
    let views = vec![view; usize::from(repetitions)];
    compose_fragment_chain(&views, values)
}

pub(crate) fn compose_fragment_chain<const N: usize>(
    views: &[PhysicalStencilView],
    values: &PatchValues<'_, N>,
) -> Result<VerifiedRegionImage, LayoutError> {
    validate_fragment_chain(views)?;
    let operations = chain_operations(views)?;
    let control = crate::stencil_cfg::RegionControlPlan::linear(0, operations.len())
        .ok_or(LayoutError::RelocationContract)?;
    let fragments = chain_fragments(views, *values)?;
    let transfers = chain_transfers(views)?;
    let mut bytes = Vec::new();
    compose_planned_region(&control, &operations, &fragments, &transfers, &mut bytes)?;
    let identity = RegionImageIdentity {
        key: RegionKey::from_opcodes(LINEAR_COMPOSITION_ID, &operations),
        cache_signature: chain_signature(views, values),
        abi: views[0].abi,
    };
    Ok(VerifiedRegionImage::from_composed(identity, bytes))
}

fn chain_operations(views: &[PhysicalStencilView]) -> Result<Vec<crate::ir::Opcode>, LayoutError> {
    let mut operations = views
        .iter()
        .map(|view| view.record.operations[0])
        .collect::<Vec<_>>();
    operations.push(
        *views
            .last()
            .and_then(|view| view.record.operations.get(1))
            .ok_or(LayoutError::RelocationContract)?,
    );
    Ok(operations)
}

fn validate_fragment_chain(views: &[PhysicalStencilView]) -> Result<(), LayoutError> {
    let first = views.first().ok_or(LayoutError::RelocationContract)?;
    if views.len() >= crate::stencil_layout::MAX_LAYOUT_FRAGMENTS {
        return Err(LayoutError::RelocationContract);
    }
    views
        .iter()
        .all(|view| compatible_fragment(*first, *view))
        .then_some(())
        .ok_or(LayoutError::RelocationContract)
}

fn compatible_fragment(first: PhysicalStencilView, view: PhysicalStencilView) -> bool {
    let contract = view.contract();
    contract.operations.len() == 2
        && contract.has_single_entry()
        && contract.abi_is_well_formed()
        && contract.executable
        && !contract.template_calls_helper
        && view.abi == first.abi
        && view.continuation_abi == first.continuation_abi
        && view.continuation_abi != crate::stencil_select::ContinuationAbi::None
        && view.stencil.validate()
        && view.fallthrough.is_some_and(|tail| tail.stencil.validate())
}

fn validate_linear_view(view: PhysicalStencilView, repetitions: u8) -> Result<(), LayoutError> {
    let contract = view.contract();
    let valid = repetitions > 0
        && usize::from(repetitions) < crate::stencil_layout::MAX_LAYOUT_FRAGMENTS
        && contract.operations.len() == 2
        && contract.has_single_entry()
        && contract.abi_is_well_formed()
        && !contract.template_calls_helper
        && view.continuation_abi == crate::stencil_select::ContinuationAbi::F64AccumulatorD0AddD1
        && view.fallthrough.is_some();
    valid.then_some(()).ok_or(LayoutError::RelocationContract)
}

fn chain_fragments<'values, const N: usize>(
    views: &[PhysicalStencilView],
    values: PatchValues<'values, N>,
) -> Result<Vec<PlannedFragment<'static, 'values, N>>, LayoutError> {
    let mut fragments = Vec::with_capacity(views.len() + 1);
    for (operation, view) in views.iter().enumerate() {
        fragments.push(PlannedFragment {
            point: RegionPoint::Operation(operation_index(operation)?),
            stencil: view.stencil,
            values,
        });
    }
    let exit = operation_index(views.len())?;
    fragments.push(PlannedFragment {
        point: RegionPoint::Operation(exit),
        stencil: views
            .last()
            .ok_or(LayoutError::MissingSuccessor)?
            .fallthrough
            .ok_or(LayoutError::MissingSuccessor)?
            .stencil,
        values,
    });
    Ok(fragments)
}

fn chain_transfers(
    views: &[PhysicalStencilView],
) -> Result<Vec<crate::stencil_region_layout::PlannedTransfer>, LayoutError> {
    let mut transfers = Vec::new();
    for (operation, view) in views.iter().enumerate() {
        let operation = operation_index(operation)?;
        transfers.extend(crate::stencil_region_links::selected_transfers_between(
            *view,
            RegionPoint::Operation(operation),
            RegionPoint::Operation(operation + 1),
        )?);
    }
    Ok(transfers)
}

fn chain_signature<const N: usize>(
    views: &[PhysicalStencilView],
    values: &PatchValues<'_, N>,
) -> u64 {
    views.iter().fold(0xcbf2_9ce4_8422_2325, |hash, view| {
        hash.wrapping_mul(0x1000_0000_01b3)
            .wrapping_add(view.cache_signature(values))
    })
}

fn operation_index(index: usize) -> Result<u8, LayoutError> {
    u8::try_from(index).map_err(|_| LayoutError::RelocationContract)
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
    fn fragment_chain_is_the_single_linear_composition_path() {
        let view =
            crate::stencil_select::select_physical(crate::stencil_select::fallthrough_region_key())
                .expect("fallthrough view");
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let declared = compose_fragment_chain(&[view, view, view], &values).expect("view chain");
        let repeated = compose_linear_chain(view, 3, &values).expect("repeat adapter");
        assert_eq!(declared.identity(), repeated.identity());
        assert_eq!(declared.bytes(), repeated.bytes());
        assert!(compose_fragment_chain(&[], &values).is_err());
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
        let incompatible =
            crate::stencil_select::select_physical(crate::stencil_select::add_chain_region_key())
                .expect("linked fragment with a different continuation ABI");
        assert!(compose_linear_chain(add, 2, &values).is_err());
        assert!(compose_linear_chain(bridge, 2, &values).is_err());
        assert!(compose_linear_chain(incompatible, 2, &values).is_err());
        let mut detached_links =
            crate::stencil_select::select_physical(crate::stencil_select::fallthrough_region_key())
                .expect("linked view");
        detached_links.links = &[];
        assert!(compose_linear_chain(detached_links, 2, &values).is_err());
    }
}
