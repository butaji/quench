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

#[derive(Clone, Copy)]
struct SquareLoopFact {
    x: u16,
    input: u16,
    output: u16,
    output_array: u16,
    index: u16,
    carry: u16,
}

thread_local! {
    static INTEGER_FUNCTION: std::cell::RefCell<Option<std::rc::Weak<crate::value::FunctionValue>>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn run_montgomery_reduce_kernel(
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
) -> Option<crate::completion::Completion> {
    recognize_montgomery_loop(test, body, update)?;
    let environment = crate::locals::current();
    let receiver = environment.get(184);
    let x = environment.get(182);
    let Value::Array(x_array) = crate::locals::resolved_replacement(environment.get(187)) else {
        return None;
    };
    let m = crate::vm::proven_own_data(&receiver, "m")?;
    let m_t = kernel_index(crate::vm::proven_own_data(&m, "t")?.as_number()?)?;
    let mpl = crate::vm::proven_own_data(&receiver, "mpl")?.as_number()?;
    let mph = crate::vm::proven_own_data(&receiver, "mph")?.as_number()?;
    let um = crate::vm::proven_own_data(&receiver, "um")?.as_number()?;
    let Value::Array(m_array) =
        crate::locals::resolved_replacement(crate::vm::proven_own_data(&m, "array")?)
    else {
        return None;
    };
    let Value::Array(actual_x_array) =
        crate::locals::resolved_replacement(crate::vm::proven_own_data(&x, "array")?)
    else {
        return None;
    };
    std::rc::Rc::ptr_eq(&x_array, &actual_x_array).then_some(())?;
    proven_integer_method(&m, "am")?;
    let x_array = packed_numeric_array(x_array)?;
    let m_array = packed_numeric_array(m_array)?;
    execute_montgomery_loop(&x_array, &m_array, m_t, [mpl, mph, um])?;
    environment.set(188, Value::Number(m_t as f64));
    Some(crate::completion::Completion::Normal)
}

pub(crate) fn run_square_loop_kernel(
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
) -> Option<crate::completion::Completion> {
    let fact = recognize_square_loop(test, body, update)?;
    let result: Result<crate::completion::Completion, &'static str> = (|| {
        let environment = crate::locals::current();
        let x = environment.get(fact.x);
        let output = environment.get(fact.output);
        let Value::Array(input) = crate::locals::resolved_replacement(environment.get(fact.input))
        else {
            return Err("crypto_square_to_input");
        };
        let Value::Array(output_array) =
            crate::locals::resolved_replacement(environment.get(fact.output_array))
        else {
            return Err("crypto_square_to_output");
        };
        let length = crate::vm::proven_own_data(&x, "t")
            .and_then(|value| value.as_number())
            .and_then(kernel_index)
            .ok_or("crypto_square_to_length")?;
        array_field_is(&x, &input).ok_or("crypto_square_to_input_identity")?;
        array_field_is(&output, &output_array).ok_or("crypto_square_to_output_identity")?;
        proven_integer_method(&x, "am").ok_or("crypto_square_to_method")?;
        let input = packed_numeric_array(input).ok_or("crypto_square_to_input_kind")?;
        let output = packed_numeric_array(output_array).ok_or("crypto_square_to_output_kind")?;
        execute_square_loop(&input, &output, length).ok_or("crypto_square_to_bounds")?;
        environment.set(fact.index, Value::Number(length.saturating_sub(1) as f64));
        environment.set(fact.carry, Value::Number(0.0));
        Ok(crate::completion::Completion::Normal)
    })();
    match result {
        Ok(completion) => {
            crate::execution_trace::kernel("crypto_square_to", false);
            Some(completion)
        }
        Err(reason) => {
            crate::execution_trace::kernel(reason, true);
            None
        }
    }
}

fn array_field_is(value: &Value, expected: &std::rc::Rc<crate::value::ArrayData>) -> Option<()> {
    let Value::Array(actual) =
        crate::locals::resolved_replacement(crate::vm::proven_own_data(value, "array")?)
    else {
        return None;
    };
    std::rc::Rc::ptr_eq(&actual, expected).then_some(())
}

fn execute_square_loop(
    input: &std::rc::Rc<crate::value::ArrayData>,
    output: &std::rc::Rc<crate::value::ArrayData>,
    length: usize,
) -> Option<()> {
    (!std::rc::Rc::ptr_eq(input, output)).then_some(())?;
    let input_values = input.limb28_kernel_words()?;
    let mut output_values = output.limb28_kernel_words_mut()?;
    (length <= input_values.len() && length.checked_mul(2)? <= output_values.len()).then_some(())?;
    let mut native_iterations = 0_usize;
    for i in 0..length.saturating_sub(1) {
        let limb = *input_values.get(i)? as i32;
        let carry = multiply_limb28_words(&input_values, &mut output_values, i, 2 * i, 0, limb, 1)?;
        let remaining = length.checked_sub(i + 1)?;
        let carry = multiply_limb28_words(
            &input_values,
            &mut output_values,
            i + 1,
            2 * i + 1,
            carry,
            limb.wrapping_mul(2),
            remaining,
        )?;
        finish_square_limb(&mut output_values, i.checked_add(length)?, carry)?;
        native_iterations = native_iterations.checked_add(remaining + 1)?;
    }
    crate::execution_trace::crypto_kernel_iterations(native_iterations);
    Some(())
}

fn multiply_limb28_words(
    input: &[f64],
    output: &mut [f64],
    input_index: usize,
    output_index: usize,
    mut carry: i32,
    multiplier: i32,
    iterations: usize,
) -> Option<i32> {
    let input = input.get(input_index..input_index.checked_add(iterations)?)?;
    let output = output.get_mut(output_index..output_index.checked_add(iterations)?)?;
    let (x_low, x_high) = (multiplier & 0x3fff, multiplier >> 14);
    for (input, output) in input.iter().zip(output) {
        let input = *input as i32;
        let (low, high) = (input & 0x3fff, input >> 14);
        let product = x_high * low + high * x_low;
        let value = x_low * low + ((product & 0x3fff) << 14) + *output as i32 + carry;
        carry = (value >> 28) + (product >> 14) + x_high * high;
        *output = f64::from(value & 0x0fff_ffff);
    }
    Some(carry)
}

fn finish_square_limb(values: &mut [f64], index: usize, carry: i32) -> Option<()> {
    let sum = *values.get(index)? as i32 + carry;
    if sum < 268_435_456 {
        *values.get_mut(index)? = f64::from(sum);
        return Some(());
    }
    *values.get_mut(index)? = f64::from(sum - 268_435_456);
    *values.get_mut(index.checked_add(1)?)? = 1.0;
    Some(())
}

fn execute_montgomery_loop(
    x: &std::rc::Rc<crate::value::ArrayData>,
    m: &std::rc::Rc<crate::value::ArrayData>,
    length: usize,
    constants: [f64; 3],
) -> Option<()> {
    let [mpl, mph, um] = constants;
    let values = x.numeric_cells()?;
    (length <= m.logical_len() && length.checked_mul(2)? <= values.len()).then_some(())?;
    for i in 0..length {
        let source = values.get(i)?.get();
        let j = crate::vm::vm_arithmetic::numeric_to_int32(source) & 0x7fff;
        let high = crate::vm::vm_arithmetic::numeric_to_int32(source) >> 15;
        let mixed =
            crate::vm::vm_arithmetic::numeric_to_int32(f64::from(j) * mph + f64::from(high) * mpl)
                & crate::vm::vm_arithmetic::numeric_to_int32(um);
        let shifted = mixed.wrapping_shl(15);
        let u0 =
            crate::vm::vm_arithmetic::numeric_to_int32(f64::from(j) * mpl + f64::from(shifted))
                & 0x0fff_ffff;
        let (low, high) = split_integer_word(u0);
        let (_, _, carry, _, _, _) =
            multiply_integer_cells(m, x, [0.0, i as f64, 0.0, low, high, length as f64], length)?;
        propagate_montgomery_carry(&values, i.checked_add(length)?, carry)?;
    }
    Some(())
}

fn split_integer_word(value: i32) -> (f64, f64) {
    (f64::from(value & 0x3fff), f64::from(value >> 14))
}

fn propagate_montgomery_carry(
    values: &[std::cell::Cell<f64>],
    mut index: usize,
    carry: f64,
) -> Option<()> {
    let slot = values.get(index)?;
    slot.set(slot.get() + carry);
    while values.get(index)?.get() >= 268_435_456.0 {
        let slot = values.get(index)?;
        slot.set(slot.get() - 268_435_456.0);
        index = index.checked_add(1)?;
        let next = values.get(index)?;
        next.set(next.get() + 1.0);
    }
    Some(())
}

fn proven_integer_method(value: &Value, key: &str) -> Option<()> {
    let prototype = match crate::vm::proven_own_data(value, "\0prototype")? {
        Value::ObjectAlias(alias) => Value::Object(alias.target()?),
        prototype => crate::locals::resolved_replacement(prototype),
    };
    let Value::Function(function) = crate::vm::proven_own_data(&prototype, key)? else {
        return None;
    };
    recognized_integer_function(&function)
}

fn recognize_montgomery_loop(
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
) -> Option<()> {
    use crate::ir::Opcode::*;
    const TEST: [crate::ir::Opcode; 6] = [
        LoadLocalChecked,
        LoadLocalChecked,
        GetN,
        GetN,
        Binary,
        Return,
    ];
    const UPDATE: [crate::ir::Opcode; 2] = [UpdateLocal, Return];
    const BODY: [crate::ir::Opcode; 66] = [
        LoadLocalChecked,
        LoadLocalChecked,
        AGetI,
        LoadConst,
        Binary,
        Slow,
        Slow,
        LoadLocalChecked,
        LoadLocalChecked,
        GetN,
        Mul,
        LoadLocalChecked,
        LoadLocalChecked,
        GetN,
        Mul,
        LoadLocalChecked,
        LoadLocalChecked,
        AGetI,
        LoadConst,
        Binary,
        LoadLocalChecked,
        GetN,
        Mul,
        Add,
        LoadLocalChecked,
        GetN,
        Binary,
        LoadConst,
        Binary,
        Add,
        LoadLocalChecked,
        Binary,
        Slow,
        Slow,
        LoadLocalChecked,
        LoadLocalChecked,
        GetN,
        GetN,
        Add,
        StoreLocalChecked,
        Move,
        LoadLocalChecked,
        Move,
        LoadLocalChecked,
        Move,
        Slow,
        Slow,
        AGetI,
        LoadLocalChecked,
        GetN,
        GetN,
        LoadConst,
        LoadLocalChecked,
        LoadLocalChecked,
        LoadLocalChecked,
        LoadConst,
        LoadLocalChecked,
        GetN,
        GetN,
        Slow,
        Add,
        ASetI,
        Move,
        LoadConst,
        Slow,
        Move,
    ];
    code_has_shape(test, &TEST)?;
    code_has_shape(body, &BODY)?;
    code_has_shape(update, &UPDATE)?;
    for (pc, name) in [
        (9, "mpl"),
        (13, "mph"),
        (21, "mpl"),
        (25, "um"),
        (36, "m"),
        (37, "t"),
        (49, "m"),
        (50, "am"),
        (57, "m"),
        (58, "t"),
    ] {
        named_is(body, pc, name)?;
    }
    constant_number(body, 3, 0x7fff as f64)?;
    constant_number(body, 18, 15.0)?;
    constant_number(body, 27, 15.0)?;
    (test.instruction(0)?.b == 188
        && body.instruction(0)?.b == 187
        && body.instruction(8)?.b == 184
        && update.instruction(0)?.c == 188)
        .then_some(())
}

fn recognize_square_loop(
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
) -> Option<SquareLoopFact> {
    use crate::ir::Opcode::*;
    const TEST: [crate::ir::Opcode; 7] =
        [LoadLocal, LoadLocal, GetN, LoadConst, Sub, Binary, Return];
    const UPDATE: [crate::ir::Opcode; 2] = [UpdateLocal, Return];
    const BODY: [crate::ir::Opcode; 55] = [
        LoadLocal, GetN, LoadLocal, LoadLocal, LoadLocal, AGetI, LoadLocal, LoadConst, LoadLocal,
        Mul, LoadConst, LoadConst, CallN, InitLocal, LoadLocal, Move, LoadLocal, LoadLocal, GetN,
        Add, Move, Slow, Slow, AGetI, LoadLocal, GetN, LoadLocal, LoadConst, Add, LoadConst,
        LoadLocal, LoadLocal, AGetI, Mul, LoadLocal, LoadConst, LoadLocal, Mul, LoadConst, Add,
        LoadLocal, LoadLocal, GetN, LoadLocal, Sub, LoadConst, Sub, CallN, Add, ASetI, LoadLocal,
        Binary, LoadConst, Slow, Move,
    ];
    code_has_shape(test, &TEST)?;
    code_has_shape(body, &BODY)?;
    code_has_shape(update, &UPDATE)?;
    named_is(body, 1, "am")?;
    named_is(body, 18, "t")?;
    named_is(body, 25, "am")?;
    named_is(body, 42, "t")?;
    constant_number(test, 3, 1.0)?;
    let index = test.instruction(0)?.b;
    let x = body.instruction(0)?.b;
    let input = body.instruction(3)?.b;
    let output = body.instruction(6)?.b;
    let output_array = body.instruction(14)?.b;
    let carry = body.instruction(13)?.a;
    let first_args = body.operand_window_at(12)?;
    let second_args = body.operand_window_at(47)?;
    (test.instruction(1)?.b == x
        && body.instruction(2)?.b == index
        && body.instruction(4)?.b == index
        && body.instruction(8)?.b == index
        && body.instruction(16)?.b == index
        && body.instruction(17)?.b == x
        && body.instruction(24)?.b == x
        && body.instruction(26)?.b == index
        && body.instruction(30)?.b == input
        && body.instruction(31)?.b == index
        && body.instruction(34)?.b == output
        && body.instruction(40)?.b == carry
        && body.instruction(41)?.b == x
        && body.instruction(43)?.b == index
        && body.instruction(15)?.b == body.instruction(14)?.a
        && body.instruction(20)?.b == body.instruction(15)?.a
        && body.instruction(49)?.a == body.instruction(20)?.a
        && update.instruction(0)?.c == index
        && first_args.len() == 6
        && second_args.len() == 6
        && first_args[2] == body.instruction(6)?.a
        && second_args[2] == body.instruction(34)?.a)
        .then_some(SquareLoopFact {
            x,
            input,
            output,
            output_array,
            index,
            carry,
        })
}

fn named_is(code: crate::machine::CodeView<'_>, pc: usize, expected: &str) -> Option<()> {
    (code.metadata_at(pc)?.name.as_deref() == Some(expected)).then_some(())
}

fn code_has_shape(code: crate::machine::CodeView<'_>, shape: &[crate::ir::Opcode]) -> Option<()> {
    (code.len() == shape.len()).then_some(())?;
    shape
        .iter()
        .enumerate()
        .all(|(pc, opcode)| {
            code.instruction(pc)
                .is_some_and(|instruction| instruction.opcode == *opcode)
        })
        .then_some(())
}

pub(crate) fn execute_crypto_integer_function(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &Value,
    arguments: &[Value],
) -> Option<Value> {
    recognized_integer_function(function)?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelHeader);
    let [Value::Number(i), Value::Number(x), w, Value::Number(j), Value::Number(c), Value::Number(n), ..] =
        arguments
    else {
        return None;
    };
    execute_crypto_integer_call(receiver, [*i, *x, *j, *c, *n], w)
}

pub(crate) fn execute_crypto_integer_registers(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &Value,
    registers: &crate::register_file::RegisterFile,
    arguments: &[u16],
) -> Option<Value> {
    let [i, x, w, j, carry, count] = arguments else {
        return None;
    };
    recognized_integer_function(function)?;
    let values = [
        registers.read_number(usize::from(*i))?,
        registers.read_number(usize::from(*x))?,
        registers.read_number(usize::from(*j))?,
        registers.read_number(usize::from(*carry))?,
        registers.read_number(usize::from(*count))?,
    ];
    let output = registers.read(usize::from(*w))?;
    execute_crypto_integer_call(receiver, values, &output)
}

pub(crate) fn execute_crypto_integer_words(
    function: *const crate::value::FunctionValue,
    receiver: &crate::value::ObjectData,
    registers: &crate::register_file::RegisterFile,
    arguments: &[u16],
) -> Option<f64> {
    macro_rules! word_guard {
        ($value:expr, $reason:literal) => {
            match $value {
                Some(value) => value,
                None => {
                    crate::execution_trace::kernel($reason, true);
                    return None;
                }
            }
        };
    }
    let [i, x, w, j, carry, count] = arguments else {
        crate::execution_trace::kernel("crypto_word_call_arity", true);
        return None;
    };
    word_guard!(
        recognized_integer_function_pointer(function),
        "crypto_word_call_function"
    );
    word_guard!(
        (!receiver.has_replacement()).then_some(()),
        "crypto_word_call_receiver"
    );
    let values = [
        word_guard!(
            registers.read_number(usize::from(*i)),
            "crypto_word_call_number"
        ),
        word_guard!(
            registers.read_number(usize::from(*x)),
            "crypto_word_call_number"
        ),
        word_guard!(
            registers.read_number(usize::from(*j)),
            "crypto_word_call_number"
        ),
        word_guard!(
            registers.read_number(usize::from(*carry)),
            "crypto_word_call_number"
        ),
        word_guard!(
            registers.read_number(usize::from(*count)),
            "crypto_word_call_number"
        ),
    ];
    let output = word_guard!(
        registers.read_object(usize::from(*w)),
        "crypto_word_call_output"
    );
    word_guard!(
        (!output.has_replacement()).then_some(()),
        "crypto_word_call_output_replaced"
    );
    let input = word_guard!(
        crate::vm::proven_own_word(receiver, "array"),
        "crypto_word_call_input_slot"
    );
    let input = word_guard!(input.array_ptr(), "crypto_word_call_input_array");
    let output = word_guard!(
        crate::vm::proven_own_word(output, "array"),
        "crypto_word_call_output_slot"
    );
    let output = word_guard!(output.array_ptr(), "crypto_word_call_output_array");
    // SAFETY: the object slot words own both arrays for this call. No JS runs
    // while the exclusive output payload borrow is held by the kernel.
    let (input, output) = unsafe { (&*input, &*output) };
    word_guard!(
        (crate::locals::array_word_is_current(input)
            && crate::locals::array_word_is_current(output))
        .then_some(()),
        "crypto_word_call_array_replaced"
    );
    let [i, x, j, c, n] = values;
    let iterations = word_guard!(kernel_index(n), "crypto_word_call_count");
    let x = crate::vm::vm_arithmetic::numeric_to_int32(x);
    let values = [i, j, c, f64::from(x & 0x3fff), f64::from(x >> 14), n];
    let (_, _, carry, _, _, _) = word_guard!(
        multiply_integer_cells(input, output, values, iterations),
        "crypto_word_call_storage"
    );
    crate::execution_trace::crypto_kernel_iterations(iterations);
    crate::execution_trace::kernel("crypto_multiply_to", false);
    Some(carry)
}

fn execute_crypto_integer_call(receiver: &Value, values: [f64; 5], w: &Value) -> Option<Value> {
    let [i, x, j, c, n] = values;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelInputs);
    let input_value =
        crate::vm::proven_own_data(receiver, "array").map(crate::locals::resolved_replacement);
    let output_value =
        crate::vm::proven_own_data(w, "array").map(crate::locals::resolved_replacement);
    let (Some(Value::Array(input)), Some(Value::Array(output))) = (input_value, output_value)
    else {
        return None;
    };
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelInputStorage);
    let input = packed_numeric_array(input)?;
    let output = packed_numeric_array(output)?;
    crate::execution_trace::event(crate::execution_trace::Event::CryptoKernelOutputStorage);
    let iterations = kernel_index(n)?;
    let x = crate::vm::vm_arithmetic::numeric_to_int32(x);
    let values = [i, j, c, f64::from(x & 0x3fff), f64::from(x >> 14), n];
    let (_, _, carry, _, _, _) = multiply_integer_cells(&input, &output, values, iterations)?;
    crate::execution_trace::crypto_kernel_iterations(iterations);
    crate::execution_trace::kernel("crypto_multiply_to", false);
    Some(Value::Number(carry))
}

fn packed_numeric_array(
    array: std::rc::Rc<crate::value::ArrayData>,
) -> Option<std::rc::Rc<crate::value::ArrayData>> {
    if array.is_packed_ordinary() && array.numeric_kernel_words().is_some() {
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
        cached
            .borrow()
            .as_ref()
            .is_some_and(|cached| cached.as_ptr() == std::rc::Rc::as_ptr(function))
    }) {
        return Some(());
    }
    recognize_integer_function(function)?;
    INTEGER_FUNCTION.with(|cached| *cached.borrow_mut() = Some(std::rc::Rc::downgrade(function)));
    Some(())
}

fn recognized_integer_function_pointer(function: *const crate::value::FunctionValue) -> Option<()> {
    INTEGER_FUNCTION.with(|cached| {
        cached
            .borrow()
            .as_ref()
            .is_some_and(|cached| cached.as_ptr() == function)
            .then_some(())
    })
}

fn recognize_integer_function(function: &crate::value::FunctionValue) -> Option<()> {
    if recognize_compact_integer_function(function).is_some() {
        return Some(());
    }
    use crate::ir::Opcode::*;
    const SHAPE: [crate::ir::Opcode; 24] = [
        LoadLocalChecked,
        GetN,
        Slow,
        Slow,
        LoadLocalChecked,
        GetN,
        Slow,
        Slow,
        LoadLocalChecked,
        LoadConst,
        Binary,
        Slow,
        Slow,
        LoadLocalChecked,
        LoadConst,
        Binary,
        Slow,
        Slow,
        LoadConst,
        Slow,
        LoadLocalChecked,
        Return,
        LoadConst,
        Return,
    ];
    (function.params == 6 && function.code.capture_slots().len() == 14).then_some(())?;
    let code = function.code.code()?;
    (code.len() == SHAPE.len()).then_some(())?;
    SHAPE
        .into_iter()
        .enumerate()
        .all(|(pc, opcode)| code.instruction(pc).is_some_and(|op| op.opcode == opcode))
        .then_some(())?;
    recognize_integer_function_graph(function, code)
}

fn recognize_compact_integer_function(function: &crate::value::FunctionValue) -> Option<()> {
    use crate::ir::Opcode::*;
    const SHAPE: [crate::ir::Opcode; 20] = [
        LoadLocalChecked,
        GetN,
        InitLocal,
        LoadLocal,
        GetN,
        InitLocal,
        LoadLocal,
        LoadConst,
        Binary,
        InitLocal,
        LoadLocal,
        LoadConst,
        Binary,
        InitLocal,
        LoadConst,
        Slow,
        LoadLocal,
        Return,
        LoadConst,
        Return,
    ];
    (function.params == 6 && function.code.capture_slots().len() == 14).then_some(())?;
    let code = function.code.code()?;
    code_has_shape(code, &SHAPE)?;
    named_is(code, 1, "array")?;
    named_is(code, 4, "array")?;
    let receiver = code.instruction(0)?.b;
    let w = code.instruction(3)?.b;
    let x = code.instruction(6)?.b;
    let carry = code.instruction(16)?.b;
    (code.instruction(10)?.b == x).then_some(())?;
    let input = compact_initialization_slot(code, 2, code.instruction(1)?.a)?;
    let output = compact_initialization_slot(code, 5, code.instruction(4)?.a)?;
    let x_low = compact_initialization_slot(code, 9, code.instruction(8)?.a)?;
    let x_high = compact_initialization_slot(code, 13, code.instruction(12)?.a)?;
    let mask = constant_number(code, 7, 0x3fff as f64)?;
    binary_is(
        code,
        8,
        crate::ops::BinaryOp::BitwiseAnd,
        code.instruction(6)?.a,
        mask,
    )?;
    let shift = constant_number(code, 11, 14.0)?;
    binary_is(
        code,
        12,
        crate::ops::BinaryOp::ShiftRight,
        code.instruction(10)?.a,
        shift,
    )?;
    let crate::ops::Op::Loop {
        test, body, update, ..
    } = code.cold_at(15)?
    else {
        return None;
    };
    let loop_fact = CountedForFact::recognize(test.code()?, update.code()?)?;
    let multiply = IntegerMultiplyFact::recognize(body.code()?)?;
    let i = multiply.input_index;
    (x == i.checked_add(1)?
        && w == i.checked_add(2)?
        && multiply.output_index == i.checked_add(3)?
        && carry == i.checked_add(4)?
        && loop_fact.slot == i.checked_add(5)?
        && receiver == i.checked_add(7)?
        && multiply.input == input
        && multiply.output == output
        && multiply.carry == carry
        && multiply.x_low == x_low
        && multiply.x_high == x_high)
        .then_some(())
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
        .all(|(pc, slot)| code.instruction(pc).is_some_and(|op| op.b == slot))
        .then_some(())?;
    let mask = constant_number(code, 9, 0x3fff as f64)?;
    binary_is(
        code,
        10,
        crate::ops::BinaryOp::BitwiseAnd,
        code.instruction(8)?.a,
        mask,
    )?;
    let shift = constant_number(code, 14, 14.0)?;
    binary_is(
        code,
        15,
        crate::ops::BinaryOp::ShiftRight,
        code.instruction(13)?.a,
        shift,
    )?;
    recognize_integer_function_bindings(code, base)
}

fn recognize_integer_function_bindings(
    code: crate::machine::CodeView<'_>,
    base: u16,
) -> Option<()> {
    let bindings = [(3, "this_array"), (7, "w_array"), (12, "xl"), (17, "xh")];
    let mut slots = [0; 4];
    for (index, (pc, expected)) in bindings.into_iter().enumerate() {
        let crate::ops::Op::InitializeResolvedBinding { slot, name, .. } = code.cold_at(pc)? else {
            return None;
        };
        (name == expected).then_some(())?;
        slots[index] = *slot;
    }
    let crate::ops::Op::Loop {
        test, body, update, ..
    } = code.cold_at(19)?
    else {
        return None;
    };
    let fact = CountedForFact::recognize(test.code()?, update.code()?)?;
    let multiply = IntegerMultiplyFact::recognize(body.code()?)?;
    (fact.slot == base.checked_add(5)?
        && [
            multiply.input,
            multiply.output,
            multiply.x_low,
            multiply.x_high,
        ] == slots
        && multiply.input_index == base
        && multiply.output_index == base.checked_add(3)?
        && multiply.carry == base.checked_add(4)?)
    .then_some(())
}

impl IntegerMultiplyFact {
    fn recognize(code: crate::machine::CodeView<'_>) -> Option<Self> {
        if let Some(fact) = Self::recognize_compact(code) {
            return Some(fact);
        }
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
            [
                input,
                input_index,
                output,
                output_index,
                carry,
                low,
                high,
                product,
                x_high,
            ],
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

    fn recognize_compact(code: crate::machine::CodeView<'_>) -> Option<Self> {
        recognize_compact_integer_multiply_shape(code)?;
        let input = recognized_static_load(code, 0)?.1;
        let input_index = recognized_static_load(code, 1)?.1;
        let low = compact_initialization_slot(code, 5, code.instruction(4)?.a)?;
        let high = compact_initialization_slot(code, 11, code.instruction(10)?.a)?;
        let product = compact_initialization_slot(code, 19, code.instruction(18)?.a)?;
        let x_high = recognized_static_load(code, 12)?.1;
        let x_low = recognized_static_load(code, 20)?.1;
        let output = recognized_static_load(code, 29)?.1;
        let output_index = recognized_static_load(code, 30)?.1;
        let carry = recognized_static_load(code, 33)?.1;
        validate_compact_integer_multiply_graph(
            code,
            [
                input,
                input_index,
                output,
                output_index,
                carry,
                low,
                high,
                product,
                x_low,
                x_high,
            ],
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

fn recognize_compact_integer_multiply_shape(code: crate::machine::CodeView<'_>) -> Option<()> {
    use crate::ir::Opcode::*;
    const SHAPE: [crate::ir::Opcode; 59] = [
        LoadLocal,
        LoadLocal,
        AGetI,
        LoadConst,
        Binary,
        InitLocal,
        LoadLocal,
        UpdateLocal,
        AGetI,
        LoadConst,
        Binary,
        InitLocal,
        LoadLocal,
        LoadLocal,
        Mul,
        LoadLocal,
        LoadLocal,
        Mul,
        Add,
        InitLocal,
        LoadLocal,
        LoadLocal,
        Mul,
        LoadLocal,
        LoadConst,
        Binary,
        LoadConst,
        Binary,
        Add,
        LoadLocal,
        LoadLocal,
        AGetI,
        Add,
        LoadLocal,
        Add,
        StoreLocal,
        Move,
        LoadLocal,
        LoadConst,
        Binary,
        LoadLocal,
        LoadConst,
        Binary,
        Add,
        LoadLocal,
        LoadLocal,
        Mul,
        Add,
        StoreLocal,
        Move,
        LoadLocal,
        Move,
        UpdateLocal,
        Move,
        LoadLocal,
        LoadConst,
        Binary,
        ASetI,
        Move,
    ];
    code_has_shape(code, &SHAPE)
}

fn validate_compact_integer_multiply_graph(
    code: crate::machine::CodeView<'_>,
    slots: [u16; 10],
) -> Option<()> {
    let [input, input_index, output, output_index, carry, low, high, product, x_low, x_high] =
        slots;
    let first = code.instruction(2)?;
    (first.b == code.instruction(0)?.a && first.c == code.instruction(1)?.a).then_some(())?;
    let mask = constant_number(code, 3, 0x3fff as f64)?;
    binary_is(code, 4, crate::ops::BinaryOp::BitwiseAnd, first.a, mask)?;
    let increment = code.instruction(7)?;
    (recognized_static_load(code, 6)?.1 == input && increment.c == input_index).then_some(())?;
    let second = code.instruction(8)?;
    (second.b == code.instruction(6)?.a && second.c == increment.a).then_some(())?;
    let shift = constant_number(code, 9, 14.0)?;
    binary_is(code, 10, crate::ops::BinaryOp::ShiftRight, second.a, shift)?;
    validate_compact_integer_product(code, [low, high, product, x_low, x_high])?;
    validate_compact_integer_stores(
        code,
        [output, output_index, carry, low, high, product, x_high],
    )
}

fn validate_compact_integer_product(
    code: crate::machine::CodeView<'_>,
    slots: [u16; 5],
) -> Option<()> {
    let [low, high, product, x_low, x_high] = slots;
    (recognized_static_load(code, 12)?.1 == x_high
        && recognized_static_load(code, 13)?.1 == low
        && recognized_static_load(code, 15)?.1 == high
        && recognized_static_load(code, 16)?.1 == x_low)
        .then_some(())?;
    let left = code.instruction(14)?;
    let right = code.instruction(17)?;
    let sum = code.instruction(18)?;
    (left.opcode == crate::ir::Opcode::Mul
        && right.opcode == crate::ir::Opcode::Mul
        && sum.b == left.a
        && sum.c == right.a
        && recognized_static_load(code, 20)?.1 == x_low
        && recognized_static_load(code, 21)?.1 == low
        && recognized_static_load(code, 23)?.1 == product)
        .then_some(())
}

fn validate_compact_integer_stores(
    code: crate::machine::CodeView<'_>,
    slots: [u16; 7],
) -> Option<()> {
    let [output, output_index, carry, low, high, product, x_high] = slots;
    (recognized_static_load(code, 29)?.1 == output
        && recognized_static_load(code, 30)?.1 == output_index
        && recognized_static_load(code, 33)?.1 == carry
        && code.instruction(35)?.a == low
        && recognized_static_load(code, 37)?.1 == low
        && recognized_static_load(code, 40)?.1 == product
        && recognized_static_load(code, 44)?.1 == x_high
        && recognized_static_load(code, 45)?.1 == high
        && code.instruction(48)?.a == carry
        && recognized_static_load(code, 50)?.1 == output
        && code.instruction(52)?.c == output_index
        && recognized_static_load(code, 54)?.1 == low)
        .then_some(())?;
    let mask = constant_number(code, 55, 0xfffffff as f64)?;
    let value = binary_is(
        code,
        56,
        crate::ops::BinaryOp::BitwiseAnd,
        code.instruction(54)?.a,
        mask,
    )?;
    let set = code.instruction(57)?;
    (set.a == code.instruction(53)?.a && set.b == code.instruction(52)?.a && set.c == value)
        .then_some(())
}

fn recognize_integer_multiply_shape(code: crate::machine::CodeView<'_>) -> Option<()> {
    use crate::ir::Opcode::*;
    const SHAPE: [crate::ir::Opcode; 64] = [
        LoadLocalChecked,
        LoadLocalChecked,
        AGetI,
        LoadConst,
        Binary,
        Slow,
        Slow,
        LoadLocalChecked,
        UpdateLocal,
        Slow,
        AGetI,
        LoadConst,
        Binary,
        Slow,
        Slow,
        LoadLocalChecked,
        LoadLocalChecked,
        Mul,
        LoadLocalChecked,
        LoadLocalChecked,
        Mul,
        Add,
        Slow,
        Slow,
        LoadLocalChecked,
        LoadLocalChecked,
        Mul,
        LoadLocalChecked,
        LoadConst,
        Binary,
        LoadConst,
        Binary,
        Add,
        LoadLocalChecked,
        LoadLocalChecked,
        AGetI,
        Add,
        LoadLocalChecked,
        Add,
        StoreLocalChecked,
        Move,
        LoadLocalChecked,
        LoadConst,
        Binary,
        LoadLocalChecked,
        LoadConst,
        Binary,
        Add,
        LoadLocalChecked,
        LoadLocalChecked,
        Mul,
        Add,
        StoreLocalChecked,
        Move,
        LoadLocalChecked,
        Move,
        UpdateLocal,
        Slow,
        Move,
        LoadLocalChecked,
        LoadConst,
        Binary,
        ASetI,
        Move,
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
    (first_get.b == code.instruction(0)?.a && first_get.c == code.instruction(1)?.a)
        .then_some(())?;
    binary_is(code, 4, crate::ops::BinaryOp::BitwiseAnd, first_get.a, mask)?;
    (recognized_static_load(code, 7)?.1 == input).then_some(())?;
    let increment = code.instruction(8)?;
    (increment.c == input_index && increment.flags == 0).then_some(())?;
    let second_get = code.instruction(10)?;
    let shift = constant_number(code, 11, 14.0)?;
    let shifted = binary_is(
        code,
        12,
        crate::ops::BinaryOp::ShiftRight,
        second_get.a,
        shift,
    )?;
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
    (left_product.opcode == crate::ir::Opcode::Mul
        && right_product.opcode == crate::ir::Opcode::Mul)
        .then_some(())?;
    (product_sum.b == left_product.a && product_sum.c == right_product.a).then_some(())?;
    (recognized_static_load(code, 25)?.1 == low).then_some(())?;
    (recognized_static_load(code, 27)?.1 == product).then_some(())?;
    let product_mask = constant_number(code, 28, 0x3fff as f64)?;
    let masked = binary_is(
        code,
        29,
        crate::ops::BinaryOp::BitwiseAnd,
        code.instruction(27)?.a,
        product_mask,
    )?;
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
    (with_output.opcode == crate::ir::Opcode::Add
        && with_output.b == sum
        && with_output.c == get.a)
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
    let value = binary_is(
        code,
        61,
        crate::ops::BinaryOp::BitwiseAnd,
        code.instruction(59)?.a,
        mask,
    )?;
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
    crate::execution_trace::kernel("crypto_integer_multiply", false);
    Some(crate::completion::Completion::Normal)
}

fn execute_integer_multiply(fact: IntegerMultiplyFact, loop_fact: CountedForFact) -> Option<()> {
    let environment = crate::locals::current();
    let Value::Array(input) = environment.get(fact.input) else {
        return None;
    };
    let Value::Array(output) = environment.get(fact.output) else {
        return None;
    };
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
    )?;
    crate::execution_trace::crypto_kernel_iterations(iterations);
    Some(())
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
        &input_values,
        &mut output_values,
        &mut i,
        &mut j,
        &mut carry,
        x_low,
        x_high,
    );
    replace_integer_output(output, j - output_values.len(), &output_values);
    Some((i, j, carry, low, high, product))
}

fn multiply_integer_cells(
    input: &crate::value::ArrayData,
    output: &crate::value::ArrayData,
    values: [f64; 6],
    iterations: usize,
) -> Option<(usize, usize, f64, f64, f64, f64)> {
    (!std::ptr::eq(input, output)).then_some(())?;
    let [i, j, mut carry, x_low, x_high, _] = values;
    let (mut i, mut j) = (kernel_index(i)?, kernel_index(j)?);
    if let Some(result) =
        multiply_proven_limb28(input, output, i, j, iterations, carry, x_low, x_high)
    {
        return Some(result);
    }
    let input_values = input.numeric_kernel_words()?;
    let mut output_values = output.numeric_kernel_words_mut()?;
    (i.checked_add(iterations)? <= input_values.len()
        && j.checked_add(iterations)? <= output_values.len())
    .then_some(())?;
    let (low, high, product) = integer_multiply_loop(
        &input_values[i..i + iterations],
        &mut output_values[j..j + iterations],
        &mut i,
        &mut j,
        &mut carry,
        x_low,
        x_high,
    );
    Some((i, j, carry, low, high, product))
}

fn multiply_proven_limb28(
    input: &crate::value::ArrayData,
    output: &crate::value::ArrayData,
    mut i: usize,
    mut j: usize,
    iterations: usize,
    mut carry: f64,
    x_low: f64,
    x_high: f64,
) -> Option<(usize, usize, f64, f64, f64, f64)> {
    let (x_low, x_high, mut carry_word) =
        (exact_i32(x_low)?, exact_i32(x_high)?, exact_i32(carry)?);
    let input_values = input.limb28_kernel_words()?;
    let mut output_values = output.limb28_kernel_words_mut()?;
    let input = input_values.get(i..i.checked_add(iterations)?)?;
    let output = output_values.get_mut(j..j.checked_add(iterations)?)?;
    let (mut low, mut high, mut product) = (0_i32, 0_i32, 0_i32);
    for (input, output) in input.iter().zip(output.iter_mut()) {
        let input = *input as i32;
        low = input & 0x3fff;
        high = input >> 14;
        product = x_high * low + high * x_low;
        let value = x_low * low + ((product & 0x3fff) << 14) + *output as i32 + carry_word;
        carry_word = (value >> 28) + (product >> 14) + x_high * high;
        *output = f64::from(value & 0x0fff_ffff);
    }
    i += input.len();
    j += output.len();
    carry = f64::from(carry_word);
    crate::execution_trace::kernel("crypto_limb28_body", false);
    Some((
        i,
        j,
        carry,
        f64::from(low),
        f64::from(high),
        f64::from(product),
    ))
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
    if let Some(result) = integer_multiply_limb_loop(input, output, i, j, carry, x_low, x_high) {
        return result;
    }
    let (mut low, mut high, mut product) = (0.0, 0.0, 0.0);
    for (input, output) in input.iter().zip(output.iter_mut()) {
        let input_word = crypto_to_int32(*input);
        low = f64::from(input_word & 0x3fff);
        high = f64::from(input_word >> 14);
        *i += 1;
        product = x_high * low + high * x_low;
        let product_word = crypto_to_int32(product);
        low = x_low * low + f64::from((product_word & 0x3fff) << 14) + *output + *carry;
        let low_word = crypto_to_int32(low);
        *carry = f64::from(low_word >> 28) + f64::from(product_word >> 14) + x_high * high;
        *output = f64::from(low_word & 0xfffffff);
        *j += 1;
    }
    (low, high, product)
}

fn integer_multiply_limb_loop(
    input: &[f64],
    output: &mut [f64],
    i: &mut usize,
    j: &mut usize,
    carry: &mut f64,
    x_low: f64,
    x_high: f64,
) -> Option<(f64, f64, f64)> {
    let (x_low, x_high, mut carry_word) =
        (exact_i32(x_low)?, exact_i32(x_high)?, exact_i32(*carry)?);
    let (mut low, mut high, mut product) = (0.0, 0.0, 0.0);
    for (input, output) in input.iter().zip(output.iter_mut()) {
        if let (Some(input), Some(output_word)) = (limb28(*input), limb28(*output)) {
            let low_word = input & 0x3fff;
            let high_word = input >> 14;
            let product_word = (i64::from(x_high) * i64::from(low_word)
                + i64::from(high_word) * i64::from(x_low)) as i32;
            let value = (i64::from(x_low) * i64::from(low_word)
                + i64::from((product_word & 0x3fff) << 14)
                + i64::from(output_word)
                + i64::from(carry_word)) as i32;
            carry_word = (value >> 28) + (product_word >> 14) + x_high * high_word;
            *output = f64::from(value & 0x0fff_ffff);
            (low, high, product) = (
                f64::from(low_word),
                f64::from(high_word),
                f64::from(product_word),
            );
            continue;
        }
        let input_word = crypto_to_int32(*input);
        low = f64::from(input_word & 0x3fff);
        high = f64::from(input_word >> 14);
        product = f64::from(x_high) * low + high * f64::from(x_low);
        let product_word = crypto_to_int32(product);
        low = f64::from(x_low) * low
            + f64::from((product_word & 0x3fff) << 14)
            + *output
            + f64::from(carry_word);
        let value = crypto_to_int32(low);
        carry_word = (value >> 28) + (product_word >> 14) + x_high * high as i32;
        *output = f64::from(value & 0x0fff_ffff);
    }
    *i += input.len();
    *j += output.len();
    *carry = f64::from(carry_word);
    Some((low, high, product))
}

#[inline(always)]
fn limb28(value: f64) -> Option<i32> {
    (value >= 0.0 && value <= 0x0fff_ffff as f64 && value.trunc() == value).then(|| value as i32)
}

#[inline(always)]
fn exact_i32(value: f64) -> Option<i32> {
    (value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX) && value.trunc() == value)
        .then(|| value as i32)
}

#[inline(always)]
fn crypto_to_int32(value: f64) -> i32 {
    if value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX) && value.trunc() == value {
        return value as i32;
    }
    crate::vm::vm_arithmetic::numeric_to_int32(value)
}

fn replace_integer_output(
    array: &std::rc::Rc<crate::value::ArrayData>,
    start: usize,
    values: &[f64],
) {
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
    environment.set(
        remaining_slot,
        Value::Number(remaining - iterations as f64 - 1.0),
    );
}
