use oxc_ast::ast::Expression;

use crate::bytecode::{Constant, Function, Instr, Op, REGISTER_MASK};

#[derive(Clone, Copy)]
pub(super) enum BindingTime<T> {
    Unknown,
    Static(T),
    Dynamic,
}

impl<T> BindingTime<T> {
    pub(super) fn static_value(self) -> Option<T> {
        match self {
            Self::Static(value) => Some(value),
            Self::Unknown | Self::Dynamic => None,
        }
    }
}

impl<T: Copy + PartialEq> BindingTime<T> {
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, value) | (value, Self::Unknown) => value,
            (Self::Static(left), Self::Static(right)) if left == right => Self::Static(left),
            _ => Self::Dynamic,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StaticValue {
    Constant(u32),
    Function(u32),
}

pub(super) fn expression(value: &Expression<'_>) -> BindingTime<Constant> {
    match value {
        Expression::NumericLiteral(value) => BindingTime::Static(Constant::Number(value.value)),
        Expression::StringLiteral(value) => BindingTime::Static(super::string::constant(value)),
        Expression::BigIntLiteral(value) => {
            BindingTime::Static(Constant::BigInt(value.value.to_string()))
        }
        Expression::BooleanLiteral(value) => BindingTime::Static(Constant::Boolean(value.value)),
        Expression::NullLiteral(_) => BindingTime::Static(Constant::Null),
        _ => BindingTime::Dynamic,
    }
}

pub(super) fn apply(functions: &mut [Function]) {
    let mut bindings = analyze_root(functions);
    invalidate_captured(functions, &mut bindings);
    materialize_constants(functions, &bindings);
    materialize_calls(functions, &bindings);
}

fn analyze_root(functions: &[Function]) -> Vec<BindingTime<StaticValue>> {
    let Some(root) = functions.first() else {
        return vec![];
    };
    if !root.wide.is_empty() {
        return vec![BindingTime::Dynamic; root.locals as usize];
    }
    let mut bindings = vec![BindingTime::Unknown; root.locals as usize];
    let mut stores = vec![0u16; root.locals as usize];
    let mut registers = vec![BindingTime::Unknown; root.registers as usize];
    for instruction in &root.code {
        match instruction.op() {
            Op::LoadConst => {
                registers[instruction.a() as usize] =
                    BindingTime::Static(StaticValue::Constant(instruction.imm()));
            }
            Op::MakeClosure => {
                registers[instruction.a() as usize] =
                    BindingTime::Static(StaticValue::Function(instruction.imm()));
            }
            Op::Move => registers[instruction.a() as usize] = registers[instruction.b() as usize],
            Op::StoreLocal | Op::StoreEnvLocal => {
                let slot = instruction.imm() as usize;
                stores[slot] += 1;
                bindings[slot] = bindings[slot].join(registers[instruction.a() as usize]);
            }
            Op::StoreCapture
            | Op::StoreName
            | Op::SetField
            | Op::SetThisField
            | Op::SetIndex
            | Op::Jump
            | Op::JumpFalse
            | Op::JumpBinaryFalse
            | Op::Return
            | Op::Throw => {}
            _ if instruction.a() < root.registers => {
                registers[instruction.a() as usize] = BindingTime::Dynamic;
            }
            _ => {}
        }
    }
    for (binding, stores) in bindings.iter_mut().zip(stores) {
        if stores != 1 {
            *binding = BindingTime::Dynamic;
        }
    }
    bindings
}

fn invalidate_captured(functions: &[Function], bindings: &mut [BindingTime<StaticValue>]) {
    for function in functions
        .iter()
        .skip(1)
        .filter(|function| function.parent == Some(0) && function.wide.is_empty())
    {
        for instruction in &function.code {
            if instruction.op() == Op::StoreCapture && instruction.imm() >> 16 == 0 {
                bindings[instruction.imm() as u16 as usize] = BindingTime::Dynamic;
            }
        }
    }
}

fn materialize_constants(functions: &mut [Function], bindings: &[BindingTime<StaticValue>]) {
    for function in direct_children(functions) {
        for instruction in &mut function.code {
            if instruction.op() == Op::LoadCapture
                && instruction.imm() >> 16 == 0
                && let BindingTime::Static(StaticValue::Constant(constant)) =
                    bindings[instruction.imm() as u16 as usize]
            {
                *instruction = Instr::new(
                    Op::LoadConst,
                    instruction.a(),
                    instruction.b(),
                    instruction.c(),
                    constant,
                );
            }
        }
    }
}

fn materialize_calls(functions: &mut [Function], bindings: &[BindingTime<StaticValue>]) {
    for function in direct_children(functions) {
        let mut known = vec![None; function.registers as usize];
        let mut origins = vec![None; function.registers as usize];
        let mut dead = Vec::new();
        for (index, instruction) in function.code.iter_mut().enumerate() {
            if matches!(
                instruction.op(),
                Op::Jump | Op::JumpFalse | Op::JumpBinaryFalse
            ) {
                known.fill(None);
                origins.fill(None);
            }
            if instruction.op() == Op::Call
                && instruction.imm() & 0x8000_0000 == 0
                && let Some((target, callee_origin)) = known[instruction.b() as usize]
            {
                dead.push(callee_origin);
                if let Some(this_origin) = origins[instruction.c() as usize] {
                    dead.push(this_origin);
                }
                *instruction = Instr::new(
                    Op::CallKnown,
                    instruction.a(),
                    target as u16,
                    0,
                    instruction.imm(),
                );
            }
            let output = instruction.a() & REGISTER_MASK;
            if output < function.registers {
                known[output as usize] = if instruction.op() == Op::LoadCapture
                    && instruction.imm() >> 16 == 0
                    && let BindingTime::Static(StaticValue::Function(target)) =
                        bindings[instruction.imm() as u16 as usize]
                {
                    Some((target, index))
                } else {
                    None
                };
                origins[output as usize] = Some(index);
            }
        }
        for index in dead {
            function.code[index] = Instr::new(Op::Nop, 0, 0, 0, 0);
        }
    }
}

fn direct_children(functions: &mut [Function]) -> impl Iterator<Item = &mut Function> {
    functions
        .iter_mut()
        .skip(1)
        .filter(|function| function.parent == Some(0) && function.wide.is_empty())
}
