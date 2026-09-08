//! Bounded call-body facts derived from canonical residual operations.

use crate::function_physical::NumericAffineI32;
use crate::machine::CodeView;
use crate::ops::{Constant, Op};

#[derive(Clone)]
pub(crate) struct IntegerSwitchI32 {
    branches: Vec<(i32, NumericAffineI32)>,
    default: NumericAffineI32,
}

#[derive(Clone)]
pub(crate) struct OwnFieldAddMethod {
    pub(crate) field: std::rc::Rc<str>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct OwnFieldAddReturn {
    pub(crate) field: std::rc::Rc<str>,
    pub(crate) addend: i32,
}

impl IntegerSwitchI32 {
    pub(crate) fn select(&self, discriminant: i32) -> NumericAffineI32 {
        self.branches
            .iter()
            .find_map(|(value, fact)| (*value == discriminant).then_some(*fact))
            .unwrap_or(self.default)
    }
}

pub(crate) fn numeric_affine_callable(
    function: &crate::value::FunctionValue,
) -> Option<NumericAffineI32> {
    (function.params == 1 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let fact = numeric_affine_body(function.code.code()?)?;
    (usize::from(fact.parameter_slot) == function.captures.len()).then_some(fact)
}

pub(crate) fn forwards_one_argument(function: &crate::value::FunctionValue) -> Option<()> {
    (function.params == 2 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let code = function.code.code()?;
    let [callee, argument, call, ret, _, _] = instruction_array::<6>(code)?;
    let first = u16::try_from(function.captures.len()).ok()?;
    (callee.opcode == crate::ir::Opcode::LoadLocal
        && callee.b == first
        && argument.opcode == crate::ir::Opcode::LoadLocal
        && argument.b == first.checked_add(1)?
        && call.opcode == crate::ir::Opcode::Call
        && call.flags == 1
        && call.b == callee.a
        && call.c == argument.a
        && ret == crate::ir::Instruction::ret(call.a)
        && call_window_matches(code, argument.a)
        && has_undefined_tail(code, 4))
    .then_some(())
}

pub(crate) fn integer_switch_callable(
    function: &crate::value::FunctionValue,
) -> Option<IntegerSwitchI32> {
    (function.params == 2 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let code = function.code.code()?;
    let [kind, initial, switch, _, _] = instruction_array::<5>(code)?;
    let first = u16::try_from(function.captures.len()).ok()?;
    let value_slot = first.checked_add(1)?;
    (kind.opcode == crate::ir::Opcode::LoadLocal
        && kind.b == first
        && is_undefined(code, initial)
        && switch.opcode == crate::ir::Opcode::Slow
        && has_undefined_tail(code, 3))
    .then_some(())?;
    let Op::Switch {
        discriminant,
        cases,
        dst: _,
    } = code.cold(switch)?
    else {
        return None;
    };
    (*discriminant == kind.a).then_some(())?;
    switch_cases(cases, value_slot)
}

pub(crate) fn own_field_add_method(
    function: &crate::value::FunctionValue,
) -> Option<OwnFieldAddMethod> {
    (function.params == 1 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let code = function.code.code()?;
    let ops = instruction_array::<13>(code)?;
    let parameter = u16::try_from(function.captures.len()).ok()?;
    let receiver = parameter.checked_add(function.params)?.checked_add(1)?;
    validate_field_add_ops(code, &ops, parameter, receiver)?;
    Some(OwnFieldAddMethod {
        field: code.metadata_at(4)?.name.clone()?,
    })
}

pub(crate) fn own_field_add_return(
    function: &crate::value::FunctionValue,
) -> Option<OwnFieldAddReturn> {
    (function.params == 1 && crate::functions::direct_call_eligible(function)).then_some(())?;
    let code = function.code.code()?;
    let [receiver, get, add, ret, _, _] = instruction_array::<6>(code)?;
    let parameter = u16::try_from(function.captures.len()).ok()?;
    (receiver.opcode == crate::ir::Opcode::LoadLocal
        && receiver.b == parameter
        && matches!(
            get.opcode,
            crate::ir::Opcode::GetN | crate::ir::Opcode::GetNQuickened
        )
        && get.b == receiver.a
        && add.opcode == crate::ir::Opcode::AddConst
        && add.b == get.a
        && ret == crate::ir::Instruction::ret(add.a)
        && has_undefined_tail(code, 4))
    .then_some(OwnFieldAddReturn {
        field: code.metadata_at(1)?.name.clone()?,
        addend: integer_constant(code, add.c)?,
    })
}

pub(crate) fn stable_own_field_add_return(
    installed: &mut Option<OwnFieldAddReturn>,
    function: &crate::value::FunctionValue,
) -> Option<OwnFieldAddReturn> {
    let fact = own_field_add_return(function)?;
    let expected = installed.get_or_insert_with(|| fact.clone());
    (*expected == fact).then_some(fact)
}

fn validate_field_add_ops(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; 13],
    parameter: u16,
    receiver: u16,
) -> Option<()> {
    use crate::ir::Opcode;
    let expected = [
        Opcode::LoadLocalChecked,
        Opcode::Move,
        Opcode::Move,
        Opcode::LoadLocalChecked,
        Opcode::GetNQuickened,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::SetN,
        Opcode::LoadLocalChecked,
        Opcode::GetNQuickened,
        Opcode::Return,
        Opcode::LoadConst,
        Opcode::Return,
    ];
    ops.iter()
        .zip(expected)
        .all(|(op, expected)| op.opcode == expected)
        .then_some(())?;
    let field = code.metadata_at(4)?.name.as_deref()?;
    (ops[0].b == receiver
        && ops[3].b == receiver
        && ops[5].b == parameter
        && ops[8].b == receiver
        && ops[4].b == ops[3].a
        && ops[6].b == ops[4].a
        && ops[6].c == ops[5].a
        && aliases_receiver(&ops[..3], ops[7].a, ops[0].a)
        && ops[7].b == ops[6].a
        && ops[9].b == ops[8].a
        && ops[10] == crate::ir::Instruction::ret(ops[9].a)
        && code.metadata_at(7)?.name.as_deref() == Some(field)
        && code.metadata_at(9)?.name.as_deref() == Some(field)
        && has_undefined_tail(code, 11))
    .then_some(())
}

fn aliases_receiver(ops: &[crate::ir::Instruction], mut register: u16, receiver: u16) -> bool {
    for op in ops.iter().rev() {
        if register == receiver {
            return true;
        }
        if op.opcode == crate::ir::Opcode::Move && op.a == register {
            register = op.b;
        }
    }
    register == receiver
}

fn switch_cases(
    cases: &[(
        Option<crate::machine::FunctionCode>,
        crate::machine::FunctionCode,
    )],
    value_slot: u16,
) -> Option<IntegerSwitchI32> {
    let mut branches = Vec::with_capacity(cases.len());
    let mut default = None;
    for (test, body) in cases {
        let body = body.code()?;
        let fact = numeric_affine_body(body)
            .filter(|fact| fact.parameter_slot == value_slot)
            .or_else(|| constant_switch_fact(body, value_slot))?;
        match test {
            Some(test) => branches.push((constant_return_i32(test.code()?)?, fact)),
            None if default.is_none() => default = Some(fact),
            None => return None,
        }
    }
    (branches.len() <= 4).then_some(IntegerSwitchI32 {
        branches,
        default: default?,
    })
}

fn numeric_affine_body(code: CodeView<'_>) -> Option<NumericAffineI32> {
    affine_add(code)
        .or_else(|| affine_multiply(code))
        .or_else(|| affine_subtract(code))
        .or_else(|| constant_affine(code))
}

fn affine_add(code: CodeView<'_>) -> Option<NumericAffineI32> {
    let load = code.instruction(0)?;
    let add = code.instruction(1)?;
    (core_len(code)? == 3
        && load.opcode == crate::ir::Opcode::LoadLocal
        && add.opcode == crate::ir::Opcode::AddConst
        && !add.add_const_is_left()
        && add.b == load.a
        && code.instruction(2)? == crate::ir::Instruction::ret(add.a))
    .then_some(NumericAffineI32 {
        parameter_slot: load.b,
        multiplier: 1,
        addend: integer_constant(code, add.c)?,
    })
}

fn affine_multiply(code: CodeView<'_>) -> Option<NumericAffineI32> {
    affine_binary(code, crate::ir::Opcode::Mul).map(|(slot, constant)| NumericAffineI32 {
        parameter_slot: slot,
        multiplier: constant,
        addend: 0,
    })
}

fn affine_subtract(code: CodeView<'_>) -> Option<NumericAffineI32> {
    let (slot, constant) = affine_binary(code, crate::ir::Opcode::Sub)?;
    Some(NumericAffineI32 {
        parameter_slot: slot,
        multiplier: 1,
        addend: constant.checked_neg()?,
    })
}

fn affine_binary(code: CodeView<'_>, opcode: crate::ir::Opcode) -> Option<(u16, i32)> {
    (core_len(code)? == 4).then_some(())?;
    let load = code.instruction(0)?;
    let constant = code.instruction(1)?;
    let binary = code.instruction(2)?;
    (load.opcode == crate::ir::Opcode::LoadLocal
        && constant.opcode == crate::ir::Opcode::LoadConst
        && binary.opcode == opcode
        && binary.b == load.a
        && binary.c == constant.a
        && code.instruction(3)? == crate::ir::Instruction::ret(binary.a))
    .then_some((load.b, integer_constant(code, constant.b)?))
}

fn constant_affine(code: CodeView<'_>) -> Option<NumericAffineI32> {
    let load = code.instruction(0)?;
    (core_len(code)? == 2
        && load.opcode == crate::ir::Opcode::LoadConst
        && code.instruction(1)? == crate::ir::Instruction::ret(load.a))
    .then_some(NumericAffineI32 {
        parameter_slot: 0,
        multiplier: 0,
        addend: integer_constant(code, load.b)?,
    })
}

fn core_len(code: CodeView<'_>) -> Option<usize> {
    let len = code.len();
    if len >= 2 && has_undefined_tail(code, len - 2) {
        Some(len - 2)
    } else {
        Some(len)
    }
}

fn constant_return_i32(code: CodeView<'_>) -> Option<i32> {
    let load = code.instruction(0)?;
    (core_len(code)? == 2
        && load.opcode == crate::ir::Opcode::LoadConst
        && code.instruction(1)? == crate::ir::Instruction::ret(load.a))
    .then(|| integer_constant(code, load.b))?
}

fn constant_switch_fact(code: CodeView<'_>, parameter_slot: u16) -> Option<NumericAffineI32> {
    Some(NumericAffineI32 {
        parameter_slot,
        multiplier: 0,
        addend: constant_result_i32(code)?,
    })
}

fn constant_result_i32(code: CodeView<'_>) -> Option<i32> {
    constant_return_i32(code).or_else(|| negative_constant_return_i32(code))
}

fn negative_constant_return_i32(code: CodeView<'_>) -> Option<i32> {
    let load = code.instruction(0)?;
    let negate = code.instruction(1)?;
    (core_len(code)? == 3
        && load.opcode == crate::ir::Opcode::LoadConst
        && negate.opcode == crate::ir::Opcode::Unary
        && crate::ir::compact_unary_operator(negate.flags) == Some(crate::ops::UnaryOp::Minus)
        && negate.b == load.a
        && code.instruction(2)? == crate::ir::Instruction::ret(negate.a))
    .then(|| integer_constant(code, load.b)?.checked_neg())?
}

fn has_undefined_tail(code: CodeView<'_>, pc: usize) -> bool {
    let Some(load) = code.instruction(pc) else {
        return false;
    };
    is_undefined(code, load)
        && code.instruction(pc + 1) == Some(crate::ir::Instruction::ret(load.a))
}

fn is_undefined(code: CodeView<'_>, instruction: crate::ir::Instruction) -> bool {
    instruction.opcode == crate::ir::Opcode::LoadConst
        && matches!(code.constant(instruction.b), Some(Constant::Undefined))
}

fn integer_constant(code: CodeView<'_>, id: u16) -> Option<i32> {
    let Constant::Number(value) = code.constant(id)? else {
        return None;
    };
    crate::stencil_numeric_integer_selection::exact_i32(*value)
}

fn call_window_matches(code: CodeView<'_>, argument: u16) -> bool {
    code.operand_window_at(2)
        .is_none_or(|window| window == [argument])
}

fn instruction_array<const N: usize>(code: CodeView<'_>) -> Option<[crate::ir::Instruction; N]> {
    (code.len() == N).then(|| std::array::from_fn(|pc| code.instruction(pc).unwrap()))
}
