//! Bounded facts for pure decrementing int32 recurrences.

use crate::{ir::Opcode, machine::CodeView};
use std::{cell::RefCell, rc::Rc};

const BODY_LEN: usize = 24;
const COUNTER_MACHINE_SLAB_BYTES: usize = 4096;
const MAX_EXACT_JS_INTEGER: u128 = 9_007_199_254_740_991;
const MAX_I32_MAGNITUDE: u128 = 1_u128 << 31;
pub(super) const MAX_ITERATIONS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct I32CounterRecurrence {
    pub(crate) value_parameter: u16,
    pub(crate) counter_parameter: u16,
    pub(crate) multiplier: i32,
    pub(crate) addend: i32,
    pub(crate) threshold: i32,
    pub(crate) decrement: i32,
}

impl I32CounterRecurrence {
    pub(crate) fn execute(self, arguments: &[crate::value::Value]) -> Option<i32> {
        let mut value = f64::from(exact_i32(number(arguments.first()?)?)?);
        let mut counter = number(arguments.get(1)?)?;
        let iterations = bounded_f64_iterations(counter, self.threshold, self.decrement)?;
        for _ in 0..iterations {
            counter -= f64::from(self.decrement);
            let next = value * f64::from(self.multiplier) + counter + f64::from(self.addend);
            value = f64::from(crate::vm::numeric_to_int32(next));
        }
        Some(crate::vm::numeric_to_int32(value))
    }

    pub(crate) fn execute_native(self, arguments: &[crate::value::Value]) -> Option<(i32, bool)> {
        let value = exact_i32(number(arguments.first()?)?)?;
        let counter = exact_i32(number(arguments.get(1)?)?)?;
        let end = bounded_iterations(counter, self.threshold, self.decrement)?;
        native_range_is_exact(self, value, counter, end).then_some(())?;
        let context = CounterLoopContext::new(self, value, counter, end)?;
        COUNTER_MACHINE.with(|machine| execute_machine(machine, self, context))
    }
}

pub(crate) fn execute_increasing(
    value: i32,
    start: i32,
    end: i32,
    multiplier: i32,
    addend: i32,
) -> Option<(i32, bool)> {
    let iterations = if start < end {
        usize::try_from(end.checked_sub(start)?).ok()?
    } else {
        0
    };
    (iterations <= MAX_ITERATIONS).then_some(())?;
    increasing_step_is_exact(start, end, multiplier, addend).then_some(())?;
    let fact = I32CounterRecurrence {
        value_parameter: 0,
        counter_parameter: 0,
        multiplier,
        addend,
        threshold: end,
        decrement: -1,
    };
    let counter = if iterations == 0 {
        start
    } else {
        start.checked_sub(1)?
    };
    let context = CounterLoopContext::new(fact, value, counter, iterations)?;
    COUNTER_MACHINE.with(|machine| execute_machine(machine, fact, context))
}

fn increasing_step_is_exact(start: i32, end: i32, multiplier: i32, addend: i32) -> bool {
    let product = MAX_I32_MAGNITUDE * u128::from(multiplier.unsigned_abs());
    let induction = u128::from(start.unsigned_abs().max(end.unsigned_abs()));
    product
        .checked_add(induction)
        .and_then(|bound| bound.checked_add(u128::from(addend.unsigned_abs())))
        .is_some_and(|bound| bound <= MAX_EXACT_JS_INTEGER)
}

#[repr(C)]
struct CounterLoopContext {
    index: usize,
    end: usize,
    value: i32,
    multiplier: i32,
    counter: i32,
    decrement: i32,
    addend: i32,
    _padding: u32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl CounterLoopContext {
    fn new(fact: I32CounterRecurrence, value: i32, counter: i32, end: usize) -> Option<Self> {
        let vm = crate::vm::current_context_or_default();
        Some(Self {
            index: 0,
            end,
            value,
            multiplier: fact.multiplier,
            counter,
            decrement: fact.decrement,
            addend: fact.addend,
            _padding: 0,
            interrupt: vm.interrupt_flag(),
        })
    }
}

struct CounterMachine {
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

thread_local! {
    static COUNTER_MACHINE: RefCell<Option<CounterMachine>> = const { RefCell::new(None) };
}

impl CounterMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::i32_counter_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::I32CounterLoop;
        let view = crate::stencil_select::select_physical_for_abi(key, abi)?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        crate::machine::validate_physical_view(view.record, view.stencil).ok()?;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            owner: Rc::new(RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(COUNTER_MACHINE_SLAB_BYTES).ok()?,
            )),
            image,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    fn invoke(&mut self, context: &mut CounterLoopContext) -> Option<u64> {
        let entry = self.entry()?;
        let lease =
            crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease
            .invoke(|call| call((context as *mut CounterLoopContext).cast()))
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
            .owned_i32_counter_loop_entry(address)
            .ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

fn execute_machine(
    machine: &RefCell<Option<CounterMachine>>,
    fact: I32CounterRecurrence,
    mut context: CounterLoopContext,
) -> Option<(i32, bool)> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = CounterMachine::new();
    }
    let status = machine.as_mut()?.invoke(&mut context)?;
    drop(machine);
    finish_native(fact, context, status).map(|value| (value, true))
}

fn finish_native(
    fact: I32CounterRecurrence,
    mut context: CounterLoopContext,
    status: u64,
) -> Option<i32> {
    if status == crate::vm::NATIVE_DISPATCH_OK && context.index == context.end {
        return Some(context.value);
    }
    (status == crate::vm::NATIVE_DISPATCH_INTERRUPT).then_some(())?;
    crate::vm::current_context_or_default().clear_interrupt();
    while context.index < context.end {
        context.counter = context.counter.checked_sub(fact.decrement)?;
        context.value = context
            .value
            .wrapping_mul(fact.multiplier)
            .wrapping_add(context.counter)
            .wrapping_add(fact.addend);
        context.index += 1;
    }
    Some(context.value)
}

pub(crate) fn select(code: CodeView<'_>) -> Option<I32CounterRecurrence> {
    let ops = instructions(code)?;
    let (counter_parameter, decrement, threshold) = select_test(code, &ops)?;
    let (value_parameter, multiplier, addend) = select_body(code, &ops, counter_parameter)?;
    select_exit(code, &ops, value_parameter)?;
    Some(I32CounterRecurrence {
        value_parameter,
        counter_parameter,
        multiplier,
        addend,
        threshold,
        decrement,
    })
}

fn select_test(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; BODY_LEN],
) -> Option<(u16, i32, i32)> {
    let counter = ops[1];
    (counter.opcode == Opcode::LoadLocal).then_some(())?;
    let decrement = integer_constant(code, ops[2])?;
    is_binary(
        ops[3],
        crate::ops::BinaryOp::NumericSubtract,
        counter.a,
        ops[2].a,
    )?;
    (ops[4].opcode == Opcode::StoreLocal && ops[4].a == counter.b && ops[4].b == ops[3].a)
        .then_some(())?;
    is_unary(ops[5], crate::ops::UnaryOp::ToNumeric, counter.a)?;
    let threshold = integer_constant(code, ops[6])?;
    is_binary(
        ops[7],
        crate::ops::BinaryOp::GreaterThan,
        ops[5].a,
        ops[6].a,
    )?;
    (ops[8] == crate::ir::Instruction::jump_if_false(ops[7].a, 20)).then_some(())?;
    (decrement > 0).then_some((counter.b, decrement, threshold))
}

fn select_body(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; BODY_LEN],
    counter_slot: u16,
) -> Option<(u16, i32, i32)> {
    (ops[9].opcode == Opcode::LoadLocal).then_some(())?;
    let multiplier = integer_constant(code, ops[10])?;
    is_numeric(ops[11], Opcode::Mul, ops[9].a, ops[10].a)?;
    (ops[12].opcode == Opcode::LoadLocal && ops[12].b == counter_slot).then_some(())?;
    is_numeric(ops[13], Opcode::Add, ops[11].a, ops[12].a)?;
    let addend = add_const(code, ops[14], ops[13].a)?;
    let zero = integer_constant(code, ops[15])?;
    (zero == 0).then_some(())?;
    is_binary(
        ops[16],
        crate::ops::BinaryOp::BitwiseOr,
        ops[14].a,
        ops[15].a,
    )?;
    (ops[17].opcode == Opcode::StoreLocal && ops[17].a == ops[9].b && ops[17].b == ops[16].a)
        .then_some(())?;
    (ops[18].opcode == Opcode::Move && ops[18].b == ops[16].a).then_some(())?;
    (ops[19] == crate::ir::Instruction::jump(1)).then_some(())?;
    Some((ops[9].b, multiplier, addend))
}

fn select_exit(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; BODY_LEN],
    value_slot: u16,
) -> Option<()> {
    (ops[20].opcode == Opcode::LoadLocal && ops[20].b == value_slot).then_some(())?;
    (ops[21].opcode == Opcode::Return && ops[21].a == ops[20].a).then_some(())?;
    matches!(
        code.constant_at(22),
        Some((_, crate::ops::Constant::Undefined))
    )
    .then_some(())?;
    (ops[23].opcode == Opcode::Return).then_some(())
}

fn instructions(code: CodeView<'_>) -> Option<[crate::ir::Instruction; BODY_LEN]> {
    (code.len() == BODY_LEN)
        .then(|| std::array::from_fn(|pc| code.instruction(pc).expect("bounded code")))
}

fn constant_at(code: CodeView<'_>, instruction: crate::ir::Instruction) -> Option<f64> {
    (instruction.opcode == Opcode::LoadConst).then_some(())?;
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    Some(*value)
}

fn add_const(code: CodeView<'_>, instruction: crate::ir::Instruction, source: u16) -> Option<i32> {
    (instruction.opcode == Opcode::AddConst && instruction.b == source).then_some(())?;
    (!instruction.add_const_is_left()).then_some(())?;
    let crate::ops::Constant::Number(value) = code.constant(instruction.c)? else {
        return None;
    };
    exact_i32(*value)
}

fn is_binary(
    instruction: crate::ir::Instruction,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Option<()> {
    (instruction.opcode == Opcode::Binary
        && crate::ir::compact_binary_operator(instruction.flags) == Some(operator)
        && instruction.b == lhs
        && instruction.c == rhs)
        .then_some(())
}

fn is_numeric(
    instruction: crate::ir::Instruction,
    opcode: Opcode,
    lhs: u16,
    rhs: u16,
) -> Option<()> {
    (instruction.opcode == opcode && instruction.b == lhs && instruction.c == rhs).then_some(())
}

fn is_unary(
    instruction: crate::ir::Instruction,
    operator: crate::ops::UnaryOp,
    source: u16,
) -> Option<()> {
    (instruction.opcode == Opcode::Unary
        && crate::ir::compact_unary_operator(instruction.flags) == Some(operator)
        && instruction.b == source)
        .then_some(())
}

fn bounded_iterations(counter: i32, threshold: i32, decrement: i32) -> Option<usize> {
    let mut counter = counter;
    for count in 0..=MAX_ITERATIONS {
        if counter <= threshold {
            return Some(count);
        }
        counter = counter.checked_sub(decrement)?;
    }
    None
}

fn bounded_f64_iterations(counter: f64, threshold: i32, decrement: i32) -> Option<usize> {
    counter.is_finite().then_some(())?;
    let mut counter = counter;
    for count in 0..=MAX_ITERATIONS {
        if counter <= f64::from(threshold) {
            return Some(count);
        }
        counter -= f64::from(decrement);
    }
    None
}

fn native_range_is_exact(
    fact: I32CounterRecurrence,
    value: i32,
    counter: i32,
    iterations: usize,
) -> bool {
    let Some(last_counter) = (i64::from(fact.decrement))
        .checked_mul(iterations as i64)
        .and_then(|delta| i64::from(counter).checked_sub(delta))
    else {
        return false;
    };
    let counter_bound = i64::from(counter)
        .unsigned_abs()
        .max(last_counter.unsigned_abs()) as u128;
    recurrence_bound(fact, value, counter_bound, iterations)
        .is_some_and(|bound| bound <= MAX_EXACT_JS_INTEGER)
}

fn recurrence_bound(
    fact: I32CounterRecurrence,
    value: i32,
    counter_bound: u128,
    iterations: usize,
) -> Option<u128> {
    let multiplier = u128::from(fact.multiplier.unsigned_abs());
    let term = counter_bound.checked_add(u128::from(fact.addend.unsigned_abs()))?;
    if multiplier == 0 {
        return Some(term);
    }
    if multiplier == 1 {
        return u128::from(value.unsigned_abs()).checked_add(term.checked_mul(iterations as u128)?);
    }
    let power = bounded_power(multiplier, iterations)?;
    let series = power.checked_sub(1)?.checked_div(multiplier - 1)?;
    u128::from(value.unsigned_abs())
        .checked_mul(power)?
        .checked_add(term.checked_mul(series)?)
}

fn bounded_power(mut base: u128, mut exponent: usize) -> Option<u128> {
    let mut result = 1u128;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result = result.checked_mul(base)?;
            (result <= MAX_EXACT_JS_INTEGER).then_some(())?;
        }
        exponent >>= 1;
        if exponent != 0 {
            base = base.checked_mul(base)?;
            (base <= MAX_EXACT_JS_INTEGER).then_some(())?;
        }
    }
    Some(result)
}

fn integer_constant(code: CodeView<'_>, instruction: crate::ir::Instruction) -> Option<i32> {
    exact_i32(constant_at(code, instruction)?)
}

fn number(value: &crate::value::Value) -> Option<f64> {
    let crate::value::Value::Number(value) = value else {
        return None;
    };
    Some(*value)
}

fn exact_i32(value: f64) -> Option<i32> {
    crate::stencil_numeric_integer_selection::exact_i32(value)
}
