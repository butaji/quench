//! Stable facts in a lane region: what its code folds under code dependencies, and the
//! invalidation points where it learns that one changed.

use super::*;

/// The Stable facts a region's code folded (`typed_ir::BindingTime::Stable`), each the subject
/// of a code dependency its publication takes (`ProgramStore::publish_lane_method`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StableFacts {
    /// Exact shapes of prototypes whose values it holds (`heap::subscribe_prototype_values`).
    pub(crate) holders: Vec<u32>,
    /// Atoms it stores without a method-cache test (`MethodCacheAtoms::readers`).
    pub(crate) atoms: Vec<Atom>,
}

/// The Stable facts a lowering folded so far.
#[derive(Default)]
pub(super) struct StableUses {
    /// Holder shapes whose prototypes' values the code took as constants.
    holders: BTreeSet<u32>,
    /// Atoms it stores directly because no method cache named them.
    atoms: BTreeSet<Atom>,
}

impl StableUses {
    pub(super) fn take(&mut self) -> StableFacts {
        StableFacts { holders: std::mem::take(&mut self.holders).into_iter().collect(),
            atoms: std::mem::take(&mut self.atoms).into_iter().collect() }
    }
}

impl Lowering<'_, '_> {
    /// The division this lowering specializes under: online where it may take code
    /// dependencies on the VM (`VmFacts::online`), offline for code shared across VMs.
    pub(super) fn division(&self) -> Division {
        if self.vm.online.is_some() { Division::Online } else { Division::Offline }
    }

    /// The code's invalidation word, as it is now (`code_dependencies::Invalidation`).
    fn invalidation_word(&mut self, online: OnlineFacts<'_>) -> ir::Value {
        let address = self.ins().iconst(types::I64, online.invalidation as i64);
        self.ins().load(types::I64, MemFlagsData::trusted(), address, 0)
    }

    /// Holds every Stable fact the code folded from here to the block's next helper that may
    /// change one: an invalidation point, which reads the word once and leaves through `exit`
    /// where a change retired the code (Execution laws 2: the change triggers, a use checks
    /// nothing).
    fn hold_stable(&mut self, state: &mut State, exit: ir::Block) {
        if state.proofs.stable { return; }
        let online = self.vm.online.expect("only the online division folds Stable facts");
        let invalidated = self.invalidation_word(online);
        let held = self.ins().icmp_imm_u(IntCC::Equal, invalidated, 0);
        emit_js_guard(&mut self.builder, held, exit);
        state.proofs.stable = true;
    }


    /// Holds the values of prototypes of exact shape `holder` (`hold_stable`).
    pub(super) fn hold_holder(&mut self, state: &mut State, exit: ir::Block, holder: u32) {
        self.hold_stable(state, exit);
        self.stable.holders.insert(holder);
    }

    /// Where no method cache names `atom` now, a store admitted while `specialized` holds:
    /// that is a Stable fact, which a cache naming it retires (`MethodCacheAtoms::name`), so the
    /// store reads the invalidation word, not the cache's names.
    pub(super) fn store_held_unnamed(&mut self, atom: Atom, specialized: ir::Value) -> Option<ir::Value> {
        let online = self.vm.online.filter(|online| !online.names(atom))?;
        self.stable.atoms.insert(atom);
        let invalidated = self.invalidation_word(online);
        let held = self.builder.ins().icmp_imm_u(IntCC::Equal, invalidated, 0);
        Some(self.builder.ins().band(specialized, held))
    }
}
