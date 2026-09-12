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
    image: VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<F64Entry>,
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
        let operations = series.operations().collect::<Vec<_>>();
        Self::from_operations(policy, owner, &operations)
    }

    /// Compose a register-independent binary chain from the continuation
    /// fragments declared by the opcode catalog. The caller owns the register
    /// bindings; this plan owns only the generated image and lease.
    pub(crate) fn from_operations(
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
        operations: &[crate::ops::BinaryOp],
    ) -> Option<Self> {
        policy.native_leaves.then_some(())?;
        let views = operations
            .iter()
            .copied()
            .map(series_view)
            .collect::<Option<Vec<_>>>()?;
        if views.len() < 2 {
            return None;
        }
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::Add);
        let values = PatchValues::from_site(&site);
        let image = compose_fragment_chain(&views, &values).ok()?;
        #[cfg(test)]
        let witness = linear_witness(&image, &views)?;
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
            #[cfg(test)]
            witness,
            #[cfg(test)]
            entries: 0,
            #[cfg(test)]
            last_entered: false,
        })
    }

    pub(crate) fn execute(&mut self, lhs: f64, rhs: f64) -> Option<f64> {
        if self.physical.is_installed() {
            if let Ok(value) = self.physical.invoke_cached(|call| call(lhs, rhs)) {
                #[cfg(test)]
                {
                    self.entries = self.entries.saturating_add(1);
                    self.last_entered = true;
                }
                return Some(value);
            }
        }
        let entry = self
            .physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_f64_entry(address),
            )
            .ok()?;
        let value = self.physical.invoke(entry, |call| call(lhs, rhs)).ok()?;
        #[cfg(test)]
        {
            self.entries = self.entries.saturating_add(1);
            self.last_entered = true;
        }
        Some(value)
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

/// One composed baseline entry for a pure register arithmetic chain. The
/// physical image receives the first accumulator and the repeated rhs; the
/// canonical register destination is materialized at the chain exit.
pub(crate) struct NativeBinarySeriesPlan {
    chain: NativeLinearF64Plan,
    lhs: u16,
    rhs: u16,
    output: u16,
    binary_span: u16,
    span: u16,
    terminal: bool,
}

impl NativeBinarySeriesPlan {
    pub(crate) fn new(
        entries: &[crate::machine::BaselineEntry],
        pc: usize,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
        live_in: &[std::collections::BTreeSet<u16>],
    ) -> Option<Self> {
        let first = entries.get(pc)?.instruction;
        let lhs = first.b;
        let rhs = first.c;
        let mut operations = vec![first.opcode.binary_operator(first.flags)?];
        if first.a == rhs || first.a == lhs {
            return None;
        }
        let mut previous = first;
        let mut cursor = pc.checked_add(1)?;
        loop {
            let instruction = entries.get(cursor)?.instruction;
            let Some(operator) = instruction.opcode.binary_operator(instruction.flags) else {
                break;
            };
            if instruction.b != previous.a
                || instruction.c != rhs
                || instruction.a == rhs
                || instruction.a == lhs
            {
                break;
            }
            operations.push(operator);
            previous = instruction;
            cursor = cursor.checked_add(1)?;
            if operations.len() >= crate::stencil_layout::MAX_LAYOUT_FRAGMENTS - 1 {
                break;
            }
        }
        if operations.len() < 2 {
            return None;
        }
        let output = previous.a;
        let ret = entries.get(cursor)?.instruction;
        let mut terminal_output = output;
        let mut live_pc = cursor;
        let mut end_pc = cursor;
        let terminal = if ret.opcode == crate::ir::Opcode::Return && ret.a == output {
            end_pc = cursor.checked_add(1)?;
            true
        } else if ret.opcode == crate::ir::Opcode::Move && ret.b == output {
            let return_pc = cursor.checked_add(1)?;
            let following = entries.get(return_pc)?.instruction;
            if following.opcode == crate::ir::Opcode::Return && following.a == ret.a {
                terminal_output = ret.a;
                live_pc = return_pc;
                end_pc = return_pc.checked_add(1)?;
                true
            } else {
                false
            }
        } else {
            false
        };
        let live_in_after = live_in.get(live_pc)?;
        operations.iter().enumerate().try_for_each(|(offset, _)| {
            let destination = entries.get(pc + offset)?.instruction.a;
            (destination == terminal_output || !live_in_after.contains(&destination)).then_some(())
        })?;
        let chain = NativeLinearF64Plan::from_operations(policy, owner, &operations)?;
        let binary_span = u16::try_from(cursor.checked_sub(pc)?).ok()?;
        let span = u16::try_from(end_pc.checked_sub(pc)?).ok()?;
        Some(Self {
            chain,
            lhs,
            rhs,
            output: terminal_output,
            binary_span,
            span,
            terminal,
        })
    }

    pub(crate) fn span(&self) -> usize {
        usize::from(self.span)
    }

    pub(crate) fn binary_span(&self) -> usize {
        usize::from(self.binary_span)
    }

    pub(crate) fn terminal(&self) -> bool {
        self.terminal
    }

    pub(crate) fn lhs(&self) -> u16 {
        self.lhs
    }

    pub(crate) fn rhs(&self) -> u16 {
        self.rhs
    }

    fn execute_with_values(
        &mut self,
        registers: &mut crate::register_file::RegisterFile,
        lhs: f64,
        rhs: f64,
    ) -> Option<(usize, f64)> {
        let value = self.chain.execute(lhs, rhs)?;
        registers.write_number(usize::from(self.output), value);
        Some((usize::from(self.span), value))
    }

    fn execute_with_lhs(
        &mut self,
        registers: &mut crate::register_file::RegisterFile,
        lhs: f64,
    ) -> Option<(usize, f64)> {
        let rhs = registers.read_number(usize::from(self.rhs))?;
        self.execute_with_values(registers, lhs, rhs)
    }

    pub(crate) fn execute(
        &mut self,
        registers: &mut crate::register_file::RegisterFile,
    ) -> Option<(usize, f64)> {
        let lhs = registers.read_number(usize::from(self.lhs))?;
        self.execute_with_lhs(registers, lhs)
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.chain.native_entry_count()
    }
}

/// A numeric `LoadConst` seed followed by a composed register binary series.
/// The tagged constant register is skipped entirely; its liveness is proven
/// dead at the chain handoff before this admission is published.
pub(crate) struct NativeConstantBinarySeriesPlan {
    constant: f64,
    chain: NativeBinarySeriesPlan,
    seed_lhs: bool,
    seed_rhs: bool,
    span: u16,
}

impl NativeConstantBinarySeriesPlan {
    pub(crate) fn new(
        code: crate::machine::CodeView<'_>,
        entries: &[crate::machine::BaselineEntry],
        pc: usize,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
        live_in: &[std::collections::BTreeSet<u16>],
    ) -> Option<Self> {
        let load = entries.get(pc)?.instruction;
        if load.opcode != crate::ir::Opcode::LoadConst {
            return None;
        }
        let (_, constant) = code.constant_at(pc)?;
        let crate::ops::Constant::Number(constant) = constant else {
            return None;
        };
        let chain_pc = pc.checked_add(1)?;
        let chain = NativeBinarySeriesPlan::new(entries, chain_pc, policy, owner, live_in)?;
        let seed_lhs = chain.lhs() == load.a;
        let seed_rhs = chain.rhs() == load.a;
        if !seed_lhs && !seed_rhs {
            return None;
        }
        let handoff = chain_pc.checked_add(chain.binary_span())?;
        if live_in.get(handoff)?.contains(&load.a) {
            return None;
        }
        let span = u16::try_from(chain.span().checked_add(1)?).ok()?;
        Some(Self {
            constant: *constant,
            chain,
            seed_lhs,
            seed_rhs,
            span,
        })
    }

    pub(crate) fn span(&self) -> usize {
        usize::from(self.span)
    }

    pub(crate) fn terminal(&self) -> bool {
        self.chain.terminal()
    }

    pub(crate) fn execute(
        &mut self,
        registers: &mut crate::register_file::RegisterFile,
    ) -> Option<(usize, f64)> {
        let lhs = if self.seed_lhs {
            self.constant
        } else {
            registers.read_number(usize::from(self.chain.lhs()))?
        };
        let rhs = if self.seed_rhs || self.chain.rhs() == self.chain.lhs() {
            self.constant
        } else {
            registers.read_number(usize::from(self.chain.rhs()))?
        };
        let (chain_span, value) = self.chain.execute_with_values(registers, lhs, rhs)?;
        Some((chain_span.checked_add(1)?, value))
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
    let opcode = crate::ir::Opcode::binary_opcode(operator)?;
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
