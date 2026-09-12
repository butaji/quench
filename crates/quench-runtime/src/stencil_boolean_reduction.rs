//! Pure counted boolean reductions selected from residual use/def facts.

use crate::{ir::Opcode, machine::CodeView, value::Value};
use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AtomKind {
    MaskEquals = 1,
    RemainderEquals = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Atom {
    kind: AtomKind,
    operand: i32,
    expected: i32,
}

#[derive(Clone, Copy, Debug)]
struct Reduction {
    index_slot: u16,
    count_slot: u16,
    start: i32,
    end: i32,
    left: Atom,
    right: Atom,
    truth_table: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FunctionReduction {
    reduction: Reduction,
    initial_count: i32,
}

impl FunctionReduction {
    pub(crate) fn execute_native(self) -> Option<i32> {
        let context = BooleanReductionContext::new(self.reduction, self.initial_count)?;
        BOOLEAN_MACHINE
            .with(|machine| execute_machine(machine, context))
            .map(|context| context.count)
    }
}

#[derive(Clone)]
enum Symbol {
    Index,
    Constant(i32),
    Partial(AtomKind, i32),
    Predicate(Atom),
}

#[derive(Clone, Copy)]
enum FlowValue {
    Bool(bool),
    Local(u16),
    One,
    Increment(u16),
    Other,
}

pub(crate) fn execute_structured(
    init: CodeView<'_>,
    test: CodeView<'_>,
    body: CodeView<'_>,
    update: CodeView<'_>,
    dst: u16,
    per_iteration: &[u16],
    registers: &mut crate::register_file::RegisterFile,
) -> Option<Result<crate::completion::Completion, crate::execute::VmError>> {
    let reduction = selection::select(init, test, body, update, per_iteration)?;
    let result = crate::locals::with_current_ref(|environment| {
        execute(reduction, environment?, dst, registers)
    })?;
    record(body);
    Some(Ok(result))
}

#[path = "stencil_boolean_reduction_select.rs"]
mod selection;

pub(crate) use selection::select_function;
fn execute(
    reduction: Reduction,
    environment: &crate::environment::Environment,
    dst: u16,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<crate::completion::Completion> {
    let number = environment.get_number(reduction.count_slot)?;
    let count = crate::stencil_numeric_integer_selection::exact_i32(number)?;
    let iterations = usize::try_from(reduction.end.checked_sub(reduction.start)?).ok()?;
    count.checked_add(i32::try_from(iterations).ok()?)?;
    let context = BooleanReductionContext::new(reduction, count)?;
    let context = BOOLEAN_MACHINE.with(|machine| execute_machine(machine, context))?;
    environment.set(
        reduction.index_slot,
        Value::Number(f64::from(context.index)),
    );
    environment.set(
        reduction.count_slot,
        Value::Number(f64::from(context.count)),
    );
    crate::execute::write_value(registers, dst, Value::Undefined);
    Some(crate::completion::Completion::Normal)
}

#[repr(C)]
struct BooleanReductionContext {
    index: i32,
    end: i32,
    count: i32,
    left_kind: u32,
    left_operand: i32,
    left_expected: i32,
    right_kind: u32,
    right_operand: i32,
    right_expected: i32,
    truth_table: u32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl BooleanReductionContext {
    fn new(reduction: Reduction, count: i32) -> Option<Self> {
        Some(Self {
            index: reduction.start,
            end: reduction.end,
            count,
            left_kind: reduction.left.kind as u32,
            left_operand: reduction.left.operand,
            left_expected: reduction.left.expected,
            right_kind: reduction.right.kind as u32,
            right_operand: reduction.right.operand,
            right_expected: reduction.right.expected,
            truth_table: u32::from(reduction.truth_table),
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        })
    }
}

struct BooleanMachine {
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

thread_local! {
    static BOOLEAN_MACHINE: RefCell<Option<BooleanMachine>> = const { RefCell::new(None) };
}

impl BooleanMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::boolean_reduction_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::BooleanReductionLoop;
        let view = crate::stencil_select::select_physical_for_abi(key, abi)?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        crate::machine::validate_physical_view(view.record, view.stencil).ok()?;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        let owner = Rc::new(RefCell::new(
            crate::stencil_arena::SharedStencilSlab::new(MACHINE_SLAB_BYTES).ok()?,
        ));
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
        })
    }

    fn invoke(&mut self, context: &mut BooleanReductionContext) -> Option<u64> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut BooleanReductionContext).cast())
            })
            .ok()
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>> {
        self.physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_boolean_reduction_loop_entry(address),
            )
            .ok()
    }
}

fn execute_machine(
    machine: &RefCell<Option<BooleanMachine>>,
    mut context: BooleanReductionContext,
) -> Option<BooleanReductionContext> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = BooleanMachine::new();
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

fn finish_portable(context: &mut BooleanReductionContext) {
    while context.index < context.end {
        let left = atom_matches(
            context.index,
            context.left_kind,
            context.left_operand,
            context.left_expected,
        );
        let right = atom_matches(
            context.index,
            context.right_kind,
            context.right_operand,
            context.right_expected,
        );
        let bit = ((u8::from(left) << 1) | u8::from(right)) as u32;
        context.count += ((context.truth_table >> bit) & 1) as i32;
        context.index += 1;
    }
}

fn atom_matches(index: i32, kind: u32, operand: i32, expected: i32) -> bool {
    let value = if kind == AtomKind::MaskEquals as u32 {
        index & operand
    } else {
        index % operand
    };
    value == expected
}

fn record(body: CodeView<'_>) {
    crate::execution_trace::stencil_observation(body, 0, "boolean_short_circuit_region", true);
    crate::execution_trace::event(crate::execution_trace::Event::LeafHit);
    #[cfg(test)]
    crate::test_execution_profile::dynamic_region_route(["boolean_short_circuit"]);
}
