//! Exact-i32 two-state masked recurrences over bounded counted loops.

use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;
const MAX_ITERATIONS: usize = 1 << 20;

#[derive(Clone, Copy, Debug)]
struct TwoState {
    counted: crate::stencil_counted_loop::CountedLoop,
    first_slot: u16,
    second_slot: u16,
    sum_mask: i32,
    index_mask: i32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FunctionTwoState {
    selected: TwoState,
    first: i32,
    second: i32,
}

#[path = "stencil_two_state_i32_select.rs"]
mod selection;

pub(crate) use selection::select_function;

impl FunctionTwoState {
    pub(crate) fn execute_native(self) -> Result<Option<i32>, crate::execute::VmError> {
        if validate_range(self.selected.counted).is_none() {
            return Ok(None);
        }
        let context = TwoStateContext::new(self);
        TWO_STATE_MACHINE.with(|machine| execute_machine(machine, context))
    }
}

fn validate_range(counted: crate::stencil_counted_loop::CountedLoop) -> Option<()> {
    let iterations = usize::try_from(counted.end.checked_sub(counted.start)?).ok()?;
    (counted.start >= 0 && iterations <= MAX_ITERATIONS).then_some(())
}

#[repr(C)]
struct TwoStateContext {
    index: i32,
    end: i32,
    first: i32,
    second: i32,
    sum_mask: i32,
    index_mask: i32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl TwoStateContext {
    fn new(fact: FunctionTwoState) -> Self {
        Self {
            index: fact.selected.counted.start,
            end: fact.selected.counted.end,
            first: fact.first,
            second: fact.second,
            sum_mask: fact.selected.sum_mask,
            index_mask: fact.selected.index_mask,
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        }
    }
}

struct TwoStateMachine {
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

thread_local! {
    static TWO_STATE_MACHINE: RefCell<Option<TwoStateMachine>> = const { RefCell::new(None) };
}

impl TwoStateMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::two_state_i32_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::TwoStateI32Loop;
        let view = crate::stencil_select::select_physical_for_abi(key, abi)?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        crate::machine::validate_physical_view(view.record, view.stencil).ok()?;
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            owner: Rc::new(RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(MACHINE_SLAB_BYTES).ok()?,
            )),
            image,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    fn invoke(&mut self, context: &mut TwoStateContext) -> Option<u64> {
        let entry = self.entry()?;
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease.invoke(|call| call((context as *mut TwoStateContext).cast())).ok()
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>> {
        if let Some(entry) = self
            .installed
            .filter(|entry| self.owner.borrow().entry_token_is_live(*entry))
        {
            return Some(entry);
        }
        let address = self
            .owner
            .borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .ok()?;
        let entry = self.owner.borrow().owned_two_state_i32_loop_entry(address).ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

fn execute_machine(
    machine: &RefCell<Option<TwoStateMachine>>,
    mut context: TwoStateContext,
) -> Result<Option<i32>, crate::execute::VmError> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = TwoStateMachine::new();
    }
    let Some(status) = machine.as_mut().and_then(|machine| machine.invoke(&mut context)) else {
        return Ok(None);
    };
    drop(machine);
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
        crate::vm::current_context_or_default().clear_interrupt();
        finish_portable(&mut context);
    }
    if matches!(status, crate::vm::NATIVE_DISPATCH_OK | crate::vm::NATIVE_DISPATCH_INTERRUPT) {
        return Ok(Some(context.second));
    }
    Err(crate::execute::VmError::EvalError(
        "two-state i32 region returned an invalid post-entry status".into(),
    ))
}

fn finish_portable(context: &mut TwoStateContext) {
    while context.index < context.end {
        let next = context.first.wrapping_add(context.second) & context.sum_mask;
        context.first = context.second;
        context.second = next ^ (context.index & context.index_mask);
        context.index += 1;
    }
}
