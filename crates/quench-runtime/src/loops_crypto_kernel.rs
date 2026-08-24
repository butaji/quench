#[derive(Clone, Copy)]
struct IntegerMultiplyFact {
    input: u16,
    output: u16,
    input_index: u16,
    output_index: u16,
    carry: u16,
    low: u16,
    high: u16,
    product: u16,
    x_low: u16,
    x_high: u16,
}

thread_local! {
    static INTEGER_FUNCTION: std::cell::RefCell<Option<std::rc::Weak<crate::value::FunctionValue>>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn execute_crypto_integer_function(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Option<Value> {
    recognized_integer_function(function)?;
    let [Value::Number(i), Value::Number(x), w, Value::Number(j), Value::Number(c), Value::Number(n), ..] = arguments else {
        return None;
    };
    let input_value = crate::vm::proven_own_data(receiver, "array")
        .map(crate::locals::resolved_replacement);
    let output_value = crate::vm::proven_own_data(w, "array")
        .map(crate::locals::resolved_replacement);
    let (Some(Value::Array(input)), Some(Value::Array(output))) = (input_value, output_value) else {
        return None;
    };
    let input = packed_numeric_array(input)?;
    let output = packed_numeric_array(output)?;
    let iterations = kernel_index(*n)?;
    let x = crate::vm::vm_arithmetic::numeric_to_int32(*x);
    let values = [*i, *j, *c, f64::from(x & 0x3fff), f64::from(x >> 14), *n];
    let (_, _, carry, _, _, _) = multiply_integer_cells(&input, &output, values, iterations)?;
    Some(Value::Number(carry))
}

fn packed_numeric_array(array: std::rc::Rc<crate::value::ArrayData>) -> Option<std::rc::Rc<crate::value::ArrayData>> {
    if array.is_packed_ordinary() && array.numeric_cells().is_some() {
        return Some(array);
    }
    let old = Value::Array(std::rc::Rc::clone(&array));
    let mut promoted = array.as_ref().clone();
    promoted.promote_sparse_numeric().then_some(())?;
    let promoted = std::rc::Rc::new(promoted);
    crate::locals::replace_value(&old, &Value::Array(std::rc::Rc::clone(&promoted)));
    Some(promoted)
}

fn recognized_integer_function(function: &std::rc::Rc<crate::value::FunctionValue>) -> Option<()> {
    if INTEGER_FUNCTION.with(|cached| {
        cached.borrow().as_ref().and_then(std::rc::Weak::upgrade)
            .is_some_and(|cached| std::rc::Rc::ptr_eq(&cached, function))
    }) {
        return Some(());
    }
    recognize_integer_function(function)?;
    INTEGER_FUNCTION.with(|cached| *cached.borrow_mut() = Some(std::rc::Rc::downgrade(function)));
    Some(())
}

fn recognize_integer_function(function: &crate::value::FunctionValue) -> Option<()> {
    use crate::ir::Opcode::*;
    const SHAPE: [crate::ir::Opcode; 24] = [
        LoadLocalChecked, GetN, Slow, Slow, LoadLocalChecked, GetN, Slow, Slow,
        LoadLocalChecked, LoadConst, Binary, Slow, Slow, LoadLocalChecked, LoadConst,
        Binary, Slow, Slow, LoadConst, Slow, LoadLocalChecked, Return, LoadConst, Return,
    ];
    (function.params == 6 && function.code.capture_slots().len() == 14).then_some(())?;
    let code = function.code.code()?;
    (code.len() == SHAPE.len()).then_some(())?;
    SHAPE.into_iter().enumerate().all(|(pc, opcode)| code.instruction(pc).is_some_and(|op| op.opcode == opcode)).then_some(())?;
    recognize_integer_function_graph(function, code)
}

fn recognize_integer_function_graph(
    function: &crate::value::FunctionValue,
    code: crate::machine::CodeView<'_>,
) -> Option<()> {
    let base = u16::try_from(function.captures.len()).ok()?;
    let slots = [
        base.checked_add(7)?,
        base.checked_add(2)?,
        base.checked_add(1)?,
        base.checked_add(1)?,
        base.checked_add(4)?,
    ];
    ([0, 4, 8, 13, 20].into_iter().zip(slots))
        .all(|(pc, slot)| code.instruction(pc).is_some_and(|op| op.b == slot)).then_some(())?;
    let mask = constant_number(code, 9, 0x3fff as f64)?;
    binary_is(code, 10, crate::ops::BinaryOp::BitwiseAnd, code.instruction(8)?.a, mask)?;
    let shift = constant_number(code, 14, 14.0)?;
    binary_is(code, 15, crate::ops::BinaryOp::ShiftRight, code.instruction(13)?.a, shift)?;
    recognize_integer_function_bindings(code, base)
}

fn recognize_integer_function_bindings(code: crate::machine::CodeView<'_>, base: u16) -> Option<()> {
    let bindings = [(3, "this_array"), (7, "w_array"), (12, "xl"), (17, "xh")];
    let mut slots = [0; 4];
    for (index, (pc, expected)) in bindings.into_iter().enumerate() {
        let crate::ops::Op::InitializeResolvedBinding { slot, name, .. } = code.cold_at(pc)? else { return None };
        (name == expected).then_some(())?;
        slots[index] = *slot;
    }
    let crate::ops::Op::Loop { test, body, update, .. } = code.cold_at(19)? else { return None };
    let fact = CountedForFact::recognize(test.code()?, update.code()?)?;
    let multiply = IntegerMultiplyFact::recognize(body.code()?)?;
    (fact.slot == base.checked_add(5)?
        && [multiply.input, multiply.output, multiply.x_low, multiply.x_high] == slots
        && multiply.input_index == base
        && multiply.output_index == base.checked_add(3)?
        && multiply.carry == base.checked_add(4)?).then_some(())
}

impl IntegerMultiplyFact {
    fn recognize(code: crate::machine::CodeView<'_>) -> Option<Self> {
        recognize_integer_multiply_shape(code)?;
        crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelShape);
        let (_, input) = recognized_static_load(code, 0)?;
        let (_, input_index) = recognized_static_load(code, 1)?;
        let low = initialization_slot(code, 5, 6, binary_result(code, 4)?)?;
        let high = initialization_slot(code, 13, 14, binary_result(code, 12)?)?;
        let product = initialization_slot(code, 22, 23, binary_result(code, 21)?)?;
        let (_, x_high) = recognized_static_load(code, 15)?;
        let (_, x_low) = recognized_static_load(code, 19)?;
        let (_, output) = recognized_static_load(code, 33)?;
        let (_, output_index) = recognized_static_load(code, 34)?;
        let (_, carry) = recognized_static_load(code, 37)?;
        validate_integer_multiply_graph(
            code,
            [input, input_index, output, output_index, carry, low, high, product, x_high],
        )?;
        Some(Self {
            input,
            output,
            input_index,
            output_index,
            carry,
            low,
            high,
            product,
            x_low,
            x_high,
        })
    }
}

fn recognize_integer_multiply_shape(code: crate::machine::CodeView<'_>) -> Option<()> {
    use crate::ir::Opcode::*;
    const SHAPE: [crate::ir::Opcode; 64] = [
        LoadLocalChecked, LoadLocalChecked, AGetI, LoadConst, Binary, Slow, Slow,
        LoadLocalChecked, UpdateLocal, Slow, AGetI, LoadConst, Binary, Slow, Slow,
        LoadLocalChecked, LoadLocalChecked, Mul, LoadLocalChecked, LoadLocalChecked, Mul, Add,
        Slow, Slow, LoadLocalChecked, LoadLocalChecked, Mul, LoadLocalChecked, LoadConst,
        Binary, LoadConst, Binary, Add, LoadLocalChecked, LoadLocalChecked, AGetI, Add,
        LoadLocalChecked, Add, StoreLocalChecked, Move, LoadLocalChecked, LoadConst, Binary,
        LoadLocalChecked, LoadConst, Binary, Add, LoadLocalChecked, LoadLocalChecked, Mul,
        Add, StoreLocalChecked, Move, LoadLocalChecked, Move, UpdateLocal, Slow, Move,
        LoadLocalChecked, LoadConst, Binary, ASetI, Move,
    ];
    (code.len() == SHAPE.len()).then_some(())?;
    SHAPE
        .into_iter()
        .enumerate()
        .all(|(pc, opcode)| code.instruction(pc).is_some_and(|op| op.opcode == opcode))
        .then_some(())
}

fn binary_result(code: crate::machine::CodeView<'_>, pc: usize) -> Option<u16> {
    code.binary_at(pc).map(|binary| binary.0)
}

fn binary_is(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Option<u16> {
    let (dst, actual, actual_lhs, actual_rhs) = code.binary_at(pc)?;
    (actual == operator && actual_lhs == lhs && actual_rhs == rhs).then_some(dst)
}

fn constant_number(code: crate::machine::CodeView<'_>, pc: usize, expected: f64) -> Option<u16> {
    let (dst, crate::ops::Constant::Number(value)) = code.constant_at(pc)? else {
        return None;
    };
    (value.to_bits() == expected.to_bits()).then_some(dst)
}

fn validate_integer_multiply_graph(
    code: crate::machine::CodeView<'_>,
    slots: [u16; 9],
) -> Option<()> {
    let [input, input_index, output, output_index, carry, low, high, product, x_high] = slots;
    let first = validate_integer_multiply_prefix(code, input, input_index, low, high)?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelPrefix);
    let sum = validate_integer_multiply_product(code, low, high, product, first)?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelProduct);
    validate_integer_multiply_stores(
        code,
        [output, output_index, carry, low, high, product, x_high],
        sum,
    )?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelStores);
    Some(())
}

fn validate_integer_multiply_prefix(
    code: crate::machine::CodeView<'_>,
    input: u16,
    input_index: u16,
    low: u16,
    high: u16,
) -> Option<u16> {
    let first_get = code.instruction(2)?;
    let mask = constant_number(code, 3, 0x3fff as f64)?;
    (first_get.b == code.instruction(0)?.a && first_get.c == code.instruction(1)?.a).then_some(())?;
    binary_is(code, 4, crate::ops::BinaryOp::BitwiseAnd, first_get.a, mask)?;
    (recognized_static_load(code, 7)?.1 == input).then_some(())?;
    let increment = code.instruction(8)?;
    (increment.c == input_index && increment.flags == 0).then_some(())?;
    let second_get = code.instruction(10)?;
    let shift = constant_number(code, 11, 14.0)?;
    let shifted = binary_is(code, 12, crate::ops::BinaryOp::ShiftRight, second_get.a, shift)?;
    (second_get.b == code.instruction(7)?.a).then_some(())?;
    (recognized_static_load(code, 16)?.1 == low && recognized_static_load(code, 18)?.1 == high)
        .then_some(())?;
    Some(shifted)
}

fn validate_integer_multiply_product(
    code: crate::machine::CodeView<'_>,
    low: u16,
    _high: u16,
    product: u16,
    _shifted: u16,
) -> Option<u16> {
    let left_product = code.instruction(17)?;
    let right_product = code.instruction(20)?;
    let product_sum = code.instruction(21)?;
    (left_product.opcode == crate::ir::Opcode::Mul && right_product.opcode == crate::ir::Opcode::Mul)
        .then_some(())?;
    (product_sum.b == left_product.a && product_sum.c == right_product.a).then_some(())?;
    (recognized_static_load(code, 25)?.1 == low).then_some(())?;
    (recognized_static_load(code, 27)?.1 == product).then_some(())?;
    let product_mask = constant_number(code, 28, 0x3fff as f64)?;
    let masked = binary_is(code, 29, crate::ops::BinaryOp::BitwiseAnd, code.instruction(27)?.a, product_mask)?;
    let shift = constant_number(code, 30, 14.0)?;
    let shifted = binary_is(code, 31, crate::ops::BinaryOp::ShiftLeft, masked, shift)?;
    let combined = code.instruction(32)?;
    (combined.opcode == crate::ir::Opcode::Add && combined.c == shifted).then_some(combined.a)
}

fn validate_integer_multiply_stores(
    code: crate::machine::CodeView<'_>,
    slots: [u16; 7],
    sum: u16,
) -> Option<()> {
    let [output, output_index, carry, low, high, product, x_high] = slots;
    (recognized_static_load(code, 33)?.1 == output).then_some(())?;
    (recognized_static_load(code, 34)?.1 == output_index).then_some(())?;
    let get = code.instruction(35)?;
    (get.b == code.instruction(33)?.a && get.c == code.instruction(34)?.a).then_some(())?;
    let with_output = code.instruction(36)?;
    (with_output.opcode == crate::ir::Opcode::Add && with_output.b == sum && with_output.c == get.a)
        .then_some(())?;
    (recognized_static_load(code, 37)?.1 == carry).then_some(())?;
    let with_carry = code.instruction(38)?;
    (with_carry.opcode == crate::ir::Opcode::Add && with_carry.b == with_output.a).then_some(())?;
    kernel_checked_store(code, 39, low, with_carry.a)?;
    validate_integer_multiply_tail(
        code,
        [output, output_index, carry, low, high, product, x_high],
    )
}

fn validate_integer_multiply_tail(
    code: crate::machine::CodeView<'_>,
    slots: [u16; 7],
) -> Option<()> {
    let [output, output_index, carry, low, high, product, x_high] = slots;
    (recognized_static_load(code, 41)?.1 == low).then_some(())?;
    let shift_28 = constant_number(code, 42, 28.0)?;
    let low_high = binary_is(
        code,
        43,
        crate::ops::BinaryOp::ShiftRight,
        code.instruction(41)?.a,
        shift_28,
    )?;
    (recognized_static_load(code, 44)?.1 == product).then_some(())?;
    let shift_14 = constant_number(code, 45, 14.0)?;
    let product_high = binary_is(
        code,
        46,
        crate::ops::BinaryOp::ShiftRight,
        code.instruction(44)?.a,
        shift_14,
    )?;
    validate_integer_carry(code, [carry, high, x_high], low_high, product_high)?;
    validate_integer_output(code, output, output_index, low)
}

fn validate_integer_carry(
    code: crate::machine::CodeView<'_>,
    slots: [u16; 3],
    low_high: u16,
    product_high: u16,
) -> Option<()> {
    let [carry, high, x_high] = slots;
    let partial_carry = code.instruction(47)?;
    (partial_carry.opcode == crate::ir::Opcode::Add
        && partial_carry.b == low_high
        && partial_carry.c == product_high)
        .then_some(())?;
    (recognized_static_load(code, 48)?.1 == x_high).then_some(())?;
    (recognized_static_load(code, 49)?.1 == high).then_some(())?;
    let high_product = code.instruction(50)?;
    (high_product.opcode == crate::ir::Opcode::Mul).then_some(())?;
    let carry_sum = code.instruction(51)?;
    (carry_sum.opcode == crate::ir::Opcode::Add
        && carry_sum.b == partial_carry.a
        && carry_sum.c == high_product.a)
        .then_some(())?;
    kernel_checked_store(code, 52, carry, carry_sum.a)
}

fn validate_integer_output(
    code: crate::machine::CodeView<'_>,
    output: u16,
    output_index: u16,
    low: u16,
) -> Option<()> {
    (recognized_static_load(code, 54)?.1 == output).then_some(())?;
    (recognized_static_load(code, 59)?.1 == low).then_some(())?;
    let update = code.instruction(56)?;
    (update.c == output_index && update.flags == 0).then_some(())?;
    let mask = constant_number(code, 60, 0xfffffff as f64)?;
    let value = binary_is(code, 61, crate::ops::BinaryOp::BitwiseAnd, code.instruction(59)?.a, mask)?;
    let set = code.instruction(62)?;
    (set.a == code.instruction(58)?.a && set.c == value).then_some(())
}

fn run_crypto_integer_kernel(
    loop_fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
) -> Option<crate::completion::Completion> {
    let fact = IntegerMultiplyFact::recognize(body)?;
    (loop_fact.timing == CountedStepTiming::BeforeTest
        && loop_fact.comparison == crate::ops::BinaryOp::GreaterEqual
        && loop_fact.step == -1.0
        && matches!(loop_fact.bound, CountedBound::Constant(0.0)))
    .then_some(())?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelHeader);
    execute_integer_multiply(fact, loop_fact)?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelHit);
    Some(crate::completion::Completion::Normal)
}

fn execute_integer_multiply(fact: IntegerMultiplyFact, loop_fact: CountedForFact) -> Option<()> {
    let environment = crate::locals::current();
    let Value::Array(input) = environment.get(fact.input) else { return None };
    let Value::Array(output) = environment.get(fact.output) else { return None };
    let i = kernel_index(environment.get_number(fact.input_index)?)?;
    let j = kernel_index(environment.get_number(fact.output_index)?)?;
    let carry = environment.get_number(fact.carry)?;
    let remaining = environment.get_number(loop_fact.slot)?;
    let x_low = environment.get_number(fact.x_low)?;
    let x_high = environment.get_number(fact.x_high)?;
    let iterations = kernel_iteration_count(loop_fact, remaining, 0.0)?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelInputs);
    if iterations == 0 {
        environment.set(loop_fact.slot, Value::Number(remaining - 1.0));
        return Some(());
    }
    execute_integer_multiply_nonempty(
        &environment,
        fact,
        loop_fact.slot,
        input,
        output,
        [i as f64, j as f64, carry, x_low, x_high, remaining],
        iterations,
    )
}

fn execute_integer_multiply_nonempty(
    environment: &crate::environment::Environment,
    fact: IntegerMultiplyFact,
    remaining_slot: u16,
    input: std::rc::Rc<crate::value::ArrayData>,
    output: std::rc::Rc<crate::value::ArrayData>,
    values: [f64; 6],
    iterations: usize,
) -> Option<()> {
    let remaining = values[5];
    let (i, j, carry, low, high, product) =
        multiply_integer_ranges(&input, &output, values, iterations)?;
    flush_integer_multiply(
        environment,
        fact,
        remaining_slot,
        i,
        j,
        carry,
        low,
        high,
        product,
        remaining,
        iterations,
    );
    Some(())
}

fn multiply_integer_ranges(
    input: &std::rc::Rc<crate::value::ArrayData>,
    output: &std::rc::Rc<crate::value::ArrayData>,
    values: [f64; 6],
    iterations: usize,
) -> Option<(usize, usize, f64, f64, f64, f64)> {
    let [i, j, mut carry, x_low, x_high, _] = values;
    let (mut i, mut j) = (kernel_index(i)?, kernel_index(j)?);
    let (input_values, mut output_values) =
        integer_multiply_ranges(input, output, i, j, iterations)?;
    trace_integer_multiply_storage();
    let (low, high, product) = integer_multiply_loop(
        &input_values, &mut output_values, &mut i, &mut j, &mut carry, x_low, x_high,
    );
    replace_integer_output(output, j - output_values.len(), &output_values);
    Some((i, j, carry, low, high, product))
}

fn multiply_integer_cells(
    input: &std::rc::Rc<crate::value::ArrayData>,
    output: &std::rc::Rc<crate::value::ArrayData>,
    values: [f64; 6],
    iterations: usize,
) -> Option<(usize, usize, f64, f64, f64, f64)> {
    (!std::rc::Rc::ptr_eq(input, output)).then_some(())?;
    let [i, j, mut carry, x_low, x_high, _] = values;
    let (mut i, mut j) = (kernel_index(i)?, kernel_index(j)?);
    let input_values = input.numeric_cells()?;
    let output_values = output.numeric_cells()?;
    (i.checked_add(iterations)? <= input_values.len()
        && j.checked_add(iterations)? <= output_values.len()).then_some(())?;
    let (low, high, product) = integer_multiply_cell_loop(
        &input_values[i..i + iterations], &output_values[j..j + iterations],
        &mut i, &mut j, &mut carry, x_low, x_high,
    );
    Some((i, j, carry, low, high, product))
}

fn integer_multiply_cell_loop(
    input: &[std::cell::Cell<f64>],
    output: &[std::cell::Cell<f64>],
    i: &mut usize,
    j: &mut usize,
    carry: &mut f64,
    x_low: f64,
    x_high: f64,
) -> (f64, f64, f64) {
    let (mut low, mut high, mut product) = (0.0, 0.0, 0.0);
    for (input, output) in input.iter().zip(output) {
        low = f64::from(crate::vm::vm_arithmetic::numeric_to_int32(input.get()) & 0x3fff);
        high = f64::from(crate::vm::vm_arithmetic::numeric_to_int32(input.get()) >> 14);
        *i += 1;
        product = x_high * low + high * x_low;
        low = x_low * low
            + f64::from((crate::vm::vm_arithmetic::numeric_to_int32(product) & 0x3fff) << 14)
            + output.get()
            + *carry;
        *carry = f64::from(crate::vm::vm_arithmetic::numeric_to_int32(low) >> 28)
            + f64::from(crate::vm::vm_arithmetic::numeric_to_int32(product) >> 14)
            + x_high * high;
        output.set(f64::from(crate::vm::vm_arithmetic::numeric_to_int32(low) & 0xfffffff));
        *j += 1;
    }
    (low, high, product)
}

fn trace_integer_multiply_storage() {
    use crate::execution_trace::{event, Event::*};
    event(CryptoKernelInputStorage);
    event(CryptoKernelOutputStorage);
    event(CryptoKernelStorage);
    event(CryptoKernelBounds);
}

fn integer_multiply_ranges(
    input: &std::rc::Rc<crate::value::ArrayData>,
    output: &std::rc::Rc<crate::value::ArrayData>,
    i: usize,
    j: usize,
    iterations: usize,
) -> Option<(Vec<f64>, Vec<f64>)> {
    (!std::rc::Rc::ptr_eq(input, output)).then_some(())?;
    let input = input.numeric_kernel_range(i, i.checked_add(iterations)?)?;
    let output = output.numeric_kernel_range(j, j.checked_add(iterations)?)?;
    Some((input, output))
}

fn integer_multiply_loop(
    input: &[f64],
    output: &mut [f64],
    i: &mut usize,
    j: &mut usize,
    carry: &mut f64,
    x_low: f64,
    x_high: f64,
) -> (f64, f64, f64) {
    let (mut low, mut high, mut product) = (0.0, 0.0, 0.0);
    for (input, output) in input.iter().zip(output) {
        low = f64::from(crate::vm::vm_arithmetic::numeric_to_int32(*input) & 0x3fff);
        high = f64::from(crate::vm::vm_arithmetic::numeric_to_int32(*input) >> 14);
        *i += 1;
        product = x_high * low + high * x_low;
        low = x_low * low
            + f64::from((crate::vm::vm_arithmetic::numeric_to_int32(product) & 0x3fff) << 14)
            + *output
            + *carry;
        *carry = f64::from(crate::vm::vm_arithmetic::numeric_to_int32(low) >> 28)
            + f64::from(crate::vm::vm_arithmetic::numeric_to_int32(product) >> 14)
            + x_high * high;
        *output = f64::from(crate::vm::vm_arithmetic::numeric_to_int32(low) & 0xfffffff);
        *j += 1;
    }
    (low, high, product)
}

fn replace_integer_output(array: &std::rc::Rc<crate::value::ArrayData>, start: usize, values: &[f64]) {
    let old = Value::Array(std::rc::Rc::clone(array));
    let mut updated = array.as_ref().clone();
    for (offset, value) in values.iter().copied().enumerate() {
        updated.set_index(start + offset, Value::Number(value));
    }
    let new = Value::Array(std::rc::Rc::new(updated));
    crate::locals::replace_value(&old, &new);
}

fn flush_integer_multiply(
    environment: &crate::environment::Environment,
    fact: IntegerMultiplyFact,
    remaining_slot: u16,
    i: usize,
    j: usize,
    carry: f64,
    low: f64,
    high: f64,
    product: f64,
    remaining: f64,
    iterations: usize,
) {
    environment.set(fact.input_index, Value::Number(i as f64));
    environment.set(fact.output_index, Value::Number(j as f64));
    environment.set(fact.carry, Value::Number(carry));
    environment.set(fact.low, Value::Number(low));
    environment.set(fact.high, Value::Number(high));
    environment.set(fact.product, Value::Number(product));
    environment.set(remaining_slot, Value::Number(remaining - iterations as f64 - 1.0));
}
