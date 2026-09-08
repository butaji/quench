//! Guarded affine-u32 recurrences with a two-arm integer reduction.

use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;
const MAX_ITERATIONS: usize = 1 << 20;
const TWO_TO_64: f64 = 18_446_744_073_709_551_616.0;
const MAX_SAFE_INTEGER: i128 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Delta {
    sign: i32,
    mask: u32,
}

#[derive(Clone, Copy, Debug)]
struct Recurrence {
    score_slot: u16,
    state_slot: u16,
    start: i32,
    end: i32,
    multiplier: f64,
    addend: f64,
    predicate_mask: u32,
    predicate_expected: u32,
    predicate_invert: bool,
    when_true: Delta,
    when_false: Delta,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FunctionRecurrence {
    recurrence: Recurrence,
    initial_score: i64,
    initial_state: u32,
}

impl FunctionRecurrence {
    pub(crate) fn execute_native(self) -> Option<i64> {
        validate_range(self)?;
        let context = BranchRecurrenceContext::new(self);
        BRANCH_MACHINE
            .with(|machine| execute_machine(machine, context))
            .map(|context| context.score)
    }
}

#[path = "stencil_branch_recurrence_select.rs"]
mod selection;

pub(crate) use selection::select_function;

#[repr(C)]
struct BranchRecurrenceContext {
    index: i32,
    end: i32,
    state: f64,
    score: i64,
    multiplier: f64,
    addend: f64,
    predicate_mask: u32,
    predicate_expected: u32,
    predicate_invert: u32,
    true_mask: u32,
    true_sign: i32,
    false_mask: u32,
    false_sign: i32,
    _padding: u32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl BranchRecurrenceContext {
    fn new(fact: FunctionRecurrence) -> Self {
        let recurrence = fact.recurrence;
        Self {
            index: recurrence.start,
            end: recurrence.end,
            state: f64::from(fact.initial_state),
            score: fact.initial_score,
            multiplier: recurrence.multiplier,
            addend: recurrence.addend,
            predicate_mask: recurrence.predicate_mask,
            predicate_expected: recurrence.predicate_expected,
            predicate_invert: u32::from(recurrence.predicate_invert),
            true_mask: recurrence.when_true.mask,
            true_sign: recurrence.when_true.sign,
            false_mask: recurrence.when_false.mask,
            false_sign: recurrence.when_false.sign,
            _padding: 0,
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        }
    }
}

struct BranchMachine {
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

thread_local! {
    static BRANCH_MACHINE: RefCell<Option<BranchMachine>> = const { RefCell::new(None) };
}

impl BranchMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::branch_recurrence_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::BranchRecurrenceLoop;
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

    fn invoke(&mut self, context: &mut BranchRecurrenceContext) -> Option<u64> {
        let entry = self.entry()?;
        let lease =
            crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease
            .invoke(|call| call((context as *mut BranchRecurrenceContext).cast()))
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
            .owned_branch_recurrence_loop_entry(address)
            .ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

fn execute_machine(
    machine: &RefCell<Option<BranchMachine>>,
    mut context: BranchRecurrenceContext,
) -> Option<BranchRecurrenceContext> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = BranchMachine::new();
    }
    let status = machine.as_mut()?.invoke(&mut context)?;
    drop(machine);
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
        crate::vm::current_context_or_default().clear_interrupt();
        finish_portable(&mut context);
    }
    (status == crate::vm::NATIVE_DISPATCH_OK || status == crate::vm::NATIVE_DISPATCH_INTERRUPT)
        .then_some(context)
}

fn finish_portable(context: &mut BranchRecurrenceContext) {
    while context.index < context.end {
        context.state = (context.state * context.multiplier) + context.addend;
        let state = crate::construct::to_uint32(context.state);
        context.state = f64::from(state);
        let matched = (state & context.predicate_mask) == context.predicate_expected;
        let take_true = matched ^ (context.predicate_invert != 0);
        let (mask, sign) = if take_true {
            (context.true_mask, context.true_sign)
        } else {
            (context.false_mask, context.false_sign)
        };
        context.score += i64::from(context.index & mask as i32) * i64::from(sign);
        context.index += 1;
    }
}

fn validate_range(fact: FunctionRecurrence) -> Option<()> {
    let recurrence = fact.recurrence;
    let iterations = usize::try_from(recurrence.end.checked_sub(recurrence.start)?).ok()?;
    (recurrence.start >= 0 && iterations <= MAX_ITERATIONS).then_some(())?;
    let maximum = f64::from(u32::MAX) * recurrence.multiplier + recurrence.addend;
    (recurrence.multiplier >= 0.0 && recurrence.addend >= 0.0 && maximum < TWO_TO_64)
        .then_some(())?;
    let score = i128::from(fact.initial_score);
    let span = i128::from(recurrence.end) * i128::try_from(iterations).ok()?;
    ((score.abs() + span) <= MAX_SAFE_INTEGER).then_some(())
}
