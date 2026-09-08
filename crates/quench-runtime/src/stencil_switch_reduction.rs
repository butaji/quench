//! Bounded integer switch reductions selected from ordinary residual control flow.

use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;
const MAX_CASES: usize = 8;
const MAX_ITERATIONS: i128 = 1 << 20;
const MAX_SAFE_INTEGER: i128 = 9_007_199_254_740_991;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActionKind {
    AddSigned = 1,
    Xor = 2,
    AddIndexMasked = 3,
    ShiftLeftOr = 4,
    ShiftRightUnsigned = 5,
}

#[derive(Clone, Copy, Debug)]
struct Action {
    kind: ActionKind,
    a: i32,
    b: i32,
}

#[derive(Clone, Debug)]
struct SwitchReduction {
    counted: crate::stencil_counted_loop::CountedLoop,
    total_slot: u16,
    selector_sign: i32,
    selector_bias: i32,
    divisor: i32,
    cases: Vec<(i32, Action)>,
    default: Action,
}

#[derive(Clone, Debug)]
pub(crate) struct FunctionSwitchReduction {
    reduction: SwitchReduction,
    initial_total: i64,
}

impl FunctionSwitchReduction {
    pub(crate) fn execute_native(self) -> Option<i64> {
        validate_range(&self)?;
        let context = SwitchReductionContext::new(&self);
        SWITCH_MACHINE
            .with(|machine| execute_machine(machine, context))
            .map(|context| context.total)
    }
}

#[path = "stencil_switch_reduction_select.rs"]
mod selection;

pub(crate) use selection::select_function;

#[repr(C)]
struct SwitchReductionContext {
    index: i32,
    end: i32,
    total: i64,
    selector_sign: i32,
    selector_bias: i32,
    divisor: i32,
    case_count: u32,
    case_values: [i32; MAX_CASES],
    action_kinds: [u32; MAX_CASES],
    action_a: [i32; MAX_CASES],
    action_b: [i32; MAX_CASES],
    default_kind: u32,
    default_a: i32,
    default_b: i32,
    _padding: u32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl SwitchReductionContext {
    fn new(fact: &FunctionSwitchReduction) -> Self {
        let reduction = &fact.reduction;
        let mut context = Self::empty(fact);
        for (index, (case, action)) in reduction.cases.iter().enumerate() {
            context.case_values[index] = *case;
            context.action_kinds[index] = action.kind as u32;
            context.action_a[index] = action.a;
            context.action_b[index] = action.b;
        }
        context
    }

    fn empty(fact: &FunctionSwitchReduction) -> Self {
        let reduction = &fact.reduction;
        Self {
            index: reduction.counted.start,
            end: reduction.counted.end,
            total: fact.initial_total,
            selector_sign: reduction.selector_sign,
            selector_bias: reduction.selector_bias,
            divisor: reduction.divisor,
            case_count: reduction.cases.len() as u32,
            case_values: [0; MAX_CASES],
            action_kinds: [0; MAX_CASES],
            action_a: [0; MAX_CASES],
            action_b: [0; MAX_CASES],
            default_kind: reduction.default.kind as u32,
            default_a: reduction.default.a,
            default_b: reduction.default.b,
            _padding: 0,
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        }
    }
}

struct SwitchMachine {
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

thread_local! {
    static SWITCH_MACHINE: RefCell<Option<SwitchMachine>> = const { RefCell::new(None) };
}

impl SwitchMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::switch_reduction_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::SwitchReductionLoop;
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

    fn invoke(&mut self, context: &mut SwitchReductionContext) -> Option<u64> {
        let entry = self.entry()?;
        let lease =
            crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease
            .invoke(|call| call((context as *mut SwitchReductionContext).cast()))
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
            .owned_switch_reduction_loop_entry(address)
            .ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

fn execute_machine(
    machine: &RefCell<Option<SwitchMachine>>,
    mut context: SwitchReductionContext,
) -> Option<SwitchReductionContext> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = SwitchMachine::new();
    }
    let status = machine.as_mut()?.invoke(&mut context)?;
    drop(machine);
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
        crate::vm::current_context_or_default().clear_interrupt();
        finish_portable(&mut context)?;
    }
    matches!(
        status,
        crate::vm::NATIVE_DISPATCH_OK | crate::vm::NATIVE_DISPATCH_INTERRUPT
    )
    .then_some(context)
}

fn finish_portable(context: &mut SwitchReductionContext) -> Option<()> {
    while context.index < context.end {
        let numerator = context
            .index
            .checked_mul(context.selector_sign)?
            .checked_add(context.selector_bias)?;
        let selector = numerator % context.divisor;
        let action = context.action(selector)?;
        context.total = apply_action(context.total, context.index, action);
        context.index += 1;
    }
    context.total = i64::from(context.total as i32);
    Some(())
}

impl SwitchReductionContext {
    fn action(&self, selector: i32) -> Option<Action> {
        let count = usize::try_from(self.case_count).ok()?.min(MAX_CASES);
        let index = self.case_values[..count]
            .iter()
            .position(|value| *value == selector);
        match index {
            Some(index) => decode_action(
                self.action_kinds[index],
                self.action_a[index],
                self.action_b[index],
            ),
            None => decode_action(self.default_kind, self.default_a, self.default_b),
        }
    }
}

fn decode_action(kind: u32, a: i32, b: i32) -> Option<Action> {
    let kind = match kind {
        1 => ActionKind::AddSigned,
        2 => ActionKind::Xor,
        3 => ActionKind::AddIndexMasked,
        4 => ActionKind::ShiftLeftOr,
        5 => ActionKind::ShiftRightUnsigned,
        _ => return None,
    };
    Some(Action { kind, a, b })
}

fn apply_action(total: i64, index: i32, action: Action) -> i64 {
    match action.kind {
        ActionKind::AddSigned => total + i64::from(action.a),
        ActionKind::Xor => i64::from((total as i32) ^ action.a),
        ActionKind::AddIndexMasked => total + i64::from(index & action.a) * i64::from(action.b),
        ActionKind::ShiftLeftOr => {
            i64::from((total as i32).wrapping_shl((action.a & 31) as u32) | action.b)
        }
        ActionKind::ShiftRightUnsigned => i64::from((total as u32) >> ((action.a & 31) as u32)),
    }
}

fn validate_range(fact: &FunctionSwitchReduction) -> Option<()> {
    let reduction = &fact.reduction;
    let iterations = i128::from(reduction.counted.end.checked_sub(reduction.counted.start)?);
    (reduction.counted.start >= 0 && iterations <= MAX_ITERATIONS).then_some(())?;
    (reduction.divisor > 0 && reduction.cases.len() <= MAX_CASES).then_some(())?;
    validate_selector_range(reduction)?;
    let per_iteration = reduction
        .cases
        .iter()
        .map(|(_, action)| *action)
        .chain([reduction.default])
        .map(maximum_delta)
        .max()?;
    (i128::from(fact.initial_total).abs() + iterations * per_iteration <= MAX_SAFE_INTEGER)
        .then_some(())
}

fn validate_selector_range(reduction: &SwitchReduction) -> Option<()> {
    let first = i64::from(reduction.counted.start) * i64::from(reduction.selector_sign)
        + i64::from(reduction.selector_bias);
    let last_index = reduction
        .counted
        .end
        .saturating_sub(1)
        .max(reduction.counted.start);
    let last = i64::from(last_index) * i64::from(reduction.selector_sign)
        + i64::from(reduction.selector_bias);
    [first, last]
        .into_iter()
        .all(|value| i32::try_from(value).is_ok())
        .then_some(())
}

fn maximum_delta(action: Action) -> i128 {
    match action.kind {
        ActionKind::AddSigned => i128::from(action.a).abs(),
        ActionKind::AddIndexMasked => i128::from(action.a as u32),
        ActionKind::Xor | ActionKind::ShiftLeftOr | ActionKind::ShiftRightUnsigned => {
            i128::from(u32::MAX)
        }
    }
}
