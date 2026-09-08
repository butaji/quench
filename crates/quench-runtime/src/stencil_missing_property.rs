//! Guarded negative named lookup followed by an exact undefined return.

use crate::ir::Opcode;
use crate::machine::{BaselineEntry, CodeView};
use std::{cell::RefCell, rc::Rc};

const REGION_LEN: usize = 3;
const PROPERTY_OFFSET: usize = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MissingPropertySelection {
    receiver_slot: u16,
}

pub(crate) struct NativeMissingPropertyPlan {
    selection: MissingPropertySelection,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    view: crate::stencil_select::PhysicalStencilView,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<extern "C" fn() -> u64>>,
}

#[derive(Clone, Copy)]
pub(crate) enum MissingPropertyExecution {
    Completed(u64),
    GuardMiss,
    NotSelected,
}

impl NativeMissingPropertyPlan {
    pub(crate) fn new(
        selection: MissingPropertySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.local_fusions.property().then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::guarded_missing_property_return_region_key(),
            crate::stencil_select::RegionAbi::ConstantWord,
        )?;
        view.generated.then_some(Self {
            selection,
            owner,
            view,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        start: usize,
        environment: &crate::environment::Environment,
    ) -> MissingPropertyExecution {
        let Some(metadata) = start
            .checked_add(PROPERTY_OFFSET)
            .and_then(|pc| code.metadata_at(pc))
        else {
            return MissingPropertyExecution::NotSelected;
        };
        if !crate::vm::named_cache_has_missing_terminal(&metadata.named_cache) {
            return MissingPropertyExecution::NotSelected;
        }
        let Some(key) = metadata.name.as_deref() else {
            return MissingPropertyExecution::NotSelected;
        };
        let missing = environment.with_proven_object(self.selection.receiver_slot, |object| {
            crate::vm::get_named_cached_missing(object, key, &metadata.named_cache)
        });
        if missing != Some(true) {
            return MissingPropertyExecution::GuardMiss;
        }
        let Some(entry) = self.entry() else {
            return MissingPropertyExecution::GuardMiss;
        };
        let Ok(lease) = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry)
        else {
            return MissingPropertyExecution::GuardMiss;
        };
        lease
            .invoke(|call| call())
            .map(MissingPropertyExecution::Completed)
            .unwrap_or(MissingPropertyExecution::GuardMiss)
    }

    fn entry(&mut self) -> Option<crate::stencil_arena::EntryToken<extern "C" fn() -> u64>> {
        if let Some(entry) = self
            .installed
            .filter(|entry| self.owner.borrow().entry_token_is_live(*entry))
        {
            return Some(entry);
        }
        self.installed = None;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::GetN);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let address = self
            .owner
            .borrow_mut()
            .render_physical_view_or_get(&mut self.cache, self.view, &values)
            .ok()?;
        self.owner.borrow_mut().make_executable(address).ok()?;
        let entry = self
            .owner
            .borrow()
            .owned_constant_word_entry(address)
            .ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

pub(crate) fn select_missing_property(
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<MissingPropertySelection> {
    let end = start.checked_add(REGION_LEN)?;
    let [load, property, ret] = entries.get(start..end)? else {
        return None;
    };
    let load = load.instruction;
    let property = property.instruction;
    let ret = ret.instruction;
    (load.opcode == Opcode::LoadLocal
        && property.opcode == Opcode::GetN
        && property.b == load.a
        && ret.opcode == Opcode::Return
        && ret.a == property.a
        && cfg.region_entry_is_legal(start, end))
    .then_some(MissingPropertySelection {
        receiver_slot: load.b,
    })
}
