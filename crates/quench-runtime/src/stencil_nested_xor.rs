//! Three-dimensional counted XOR reductions over canonical residual loops.

use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;
const MAX_ITERATIONS: i128 = 1 << 20;
const MAX_SAFE_INTEGER: i128 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug)]
struct NestedXor {
    loops: [crate::stencil_counted_loop::CountedLoop; 3],
    total_slot: u16,
    mask: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FunctionNestedXor {
    reduction: NestedXor,
    initial_total: i64,
}

impl FunctionNestedXor {
    pub(crate) fn execute_native(self) -> Option<i64> {
        validate_range(self)?;
        let context = NestedXorContext::new(self);
        NESTED_MACHINE
            .with(|machine| execute_machine(machine, context))
            .map(|context| context.total)
    }
}

#[path = "stencil_nested_xor_select.rs"]
mod selection;

pub(crate) use selection::select_function;

#[repr(C)]
struct NestedXorContext {
    indices: [i32; 3],
    starts: [i32; 3],
    ends: [i32; 3],
    mask: u32,
    total: i64,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl NestedXorContext {
    fn new(fact: FunctionNestedXor) -> Self {
        let loops = fact.reduction.loops;
        let starts = loops.map(|counted| counted.start);
        Self {
            indices: starts,
            starts,
            ends: loops.map(|counted| counted.end),
            mask: fact.reduction.mask,
            total: fact.initial_total,
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        }
    }
}

struct NestedMachine {
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

thread_local! {
    static NESTED_MACHINE: RefCell<Option<NestedMachine>> = const { RefCell::new(None) };
}

impl NestedMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::nested_xor_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::NestedXorLoop;
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

    fn invoke(&mut self, context: &mut NestedXorContext) -> Option<u64> {
        let entry = self.entry()?;
        let lease =
            crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease
            .invoke(|call| call((context as *mut NestedXorContext).cast()))
            .ok()
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
        let entry = self
            .owner
            .borrow()
            .owned_nested_xor_loop_entry(address)
            .ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

fn execute_machine(
    machine: &RefCell<Option<NestedMachine>>,
    mut context: NestedXorContext,
) -> Option<NestedXorContext> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = NestedMachine::new();
    }
    let status = machine.as_mut()?.invoke(&mut context)?;
    drop(machine);
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
        crate::vm::current_context_or_default().clear_interrupt();
        finish_portable(&mut context);
    }
    matches!(
        status,
        crate::vm::NATIVE_DISPATCH_OK | crate::vm::NATIVE_DISPATCH_INTERRUPT
    )
    .then_some(context)
}

fn finish_portable(context: &mut NestedXorContext) {
    while context.indices[0] < context.ends[0] {
        if context.indices[1] >= context.ends[1] {
            next_outer(context);
        } else if context.indices[2] >= context.ends[2] {
            next_middle(context);
        } else {
            execute_iteration(context);
        }
    }
}

fn next_outer(context: &mut NestedXorContext) {
    context.indices[0] += 1;
    context.indices[1] = context.starts[1];
    context.indices[2] = context.starts[2];
}

fn next_middle(context: &mut NestedXorContext) {
    context.indices[1] += 1;
    context.indices[2] = context.starts[2];
}

fn execute_iteration(context: &mut NestedXorContext) {
    let value = context.indices[0] ^ context.indices[1] ^ context.indices[2];
    context.total += i64::from((value as u32) & context.mask);
    context.indices[2] += 1;
}

fn validate_range(fact: FunctionNestedXor) -> Option<()> {
    let mut spans = [0_i128; 3];
    for (index, counted) in fact.reduction.loops.into_iter().enumerate() {
        let span = counted.end.checked_sub(counted.start)?;
        (counted.start >= 0).then_some(())?;
        spans[index] = i128::from(span);
    }
    let middle_visits = spans[0].checked_mul(spans[1])?;
    let iterations = middle_visits.checked_mul(spans[2])?;
    let control_steps = spans[0]
        .checked_add(middle_visits)?
        .checked_add(iterations)?;
    (control_steps <= MAX_ITERATIONS).then_some(())?;
    let maximum =
        i128::from(fact.initial_total).abs() + iterations * i128::from(fact.reduction.mask);
    (maximum <= MAX_SAFE_INTEGER).then_some(())
}
