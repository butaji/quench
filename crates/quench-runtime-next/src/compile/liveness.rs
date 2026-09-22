use crate::bytecode::{
    FieldBase, FieldSite, Function, Instr, NUMERIC_LOCAL_TARGET, Op, Operand, REGISTER_MASK,
    RETURN_REGISTER, Register, Superinstruction,
};

type MethodSite = (u32, u16, Vec<Register>, Option<(u32, u16)>);

pub(super) fn derive(
    functions: &mut [Function],
    methods: &[MethodSite],
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Vec<u64> {
    let mut roots = Vec::new();
    for function in functions {
        if let Some(map) = analyze(function, methods, fields, superinstructions) {
            function.register_root_offset = roots.len() as u32;
            roots.extend(map);
        }
    }
    roots.shrink_to_fit();
    roots
}

pub(super) fn analyze(
    function: &Function,
    methods: &[MethodSite],
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<Vec<u64>> {
    if !function.wide.is_empty() {
        return None;
    }
    if function.registers > 64 {
        return None;
    }
    let mut live = vec![0; function.code.len() + 1];
    loop {
        let mut changed = false;
        for pc in (0..function.code.len()).rev() {
            let instruction = function.code[pc];
            let output = successors(function, &live, pc, instruction);
            let next = uses(instruction, methods, fields, superinstructions)
                | (output & !definitions(instruction, superinstructions));
            if live[pc] != next {
                live[pc] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    add_suspended_exception_roots(function, &mut live);
    Some(live)
}

fn successors(function: &Function, live: &[u64], pc: usize, instruction: Instr) -> u64 {
    let fallthrough = live.get(pc + 1).copied().unwrap_or(0);
    let normal = match instruction.op() {
        Op::Jump => live[instruction.imm() as usize],
        Op::JumpFalse | Op::JumpBinaryFalse => fallthrough | live[instruction.imm() as usize],
        Op::Return | Op::Throw => 0,
        _ if instruction.a() & RETURN_REGISTER != 0 => 0,
        _ => fallthrough,
    };
    function
        .handlers
        .iter()
        .filter(|handler| pc as u32 >= handler.start && (pc as u32) < handler.end)
        .fold(normal, |mask, handler| mask | live[handler.target as usize])
}

fn add_suspended_exception_roots(function: &Function, live: &mut [u64]) {
    for (pc, instruction) in function.code.iter().enumerate() {
        if !matches!(
            instruction.op(),
            Op::Call | Op::CallKnown | Op::CallMethod | Op::CallThisMethod | Op::Construct
        ) {
            continue;
        }
        for handler in &function.handlers {
            if pc as u32 >= handler.start && (pc as u32) < handler.end {
                live[pc + 1] |= live[handler.target as usize];
            }
        }
    }
}

fn uses(
    instruction: Instr,
    methods: &[MethodSite],
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> u64 {
    match instruction.op() {
        Op::StoreLocal | Op::StoreEnvLocal | Op::StoreCapture | Op::StoreName => {
            bit(instruction.a())
        }
        Op::GetIterator | Op::GetAsyncIterator => bit(instruction.b()),
        Op::GetField => field_base(instruction, fields),
        Op::GetIndex => operand(instruction.b(), fields) | operand(instruction.c(), fields),
        Op::MakeObject2 => bit(instruction.b()) | bit(instruction.c()),
        Op::SuperConstArrayObject2 => superinstructions[instruction.imm() as usize]
            .code
            .iter()
            .fold(0, |mask, nested| {
                mask | uses(*nested, methods, fields, superinstructions)
            }),
        Op::SetField => bit(instruction.a()) | bit(instruction.b()),
        Op::SetThisField => bit(instruction.a()),
        Op::SetIndex => bit(instruction.a()) | bit(instruction.b()) | bit(instruction.c()),
        Op::Binary | Op::NumericAdd | Op::NumericMultiply | Op::JumpBinaryFalse => {
            operand(instruction.b(), fields) | operand(instruction.c(), fields)
        }
        Op::IncDec | Op::Unary | Op::Move => bit(instruction.b()),
        Op::JumpFalse | Op::Return | Op::Throw => bit(instruction.a()),
        Op::Call => {
            bit(instruction.b())
                | bit(instruction.c())
                | range((instruction.imm() >> 16) as u16, instruction.imm() as u16)
        }
        Op::CallKnown => range((instruction.imm() >> 16) as u16, instruction.imm() as u16),
        Op::CallMethod => bit(instruction.b()) | method_arguments(instruction, methods),
        Op::CallThisMethod => method_arguments(instruction, methods),
        Op::Construct => bit(instruction.b()) | range(instruction.c(), instruction.imm() as u16),
        _ => 0,
    }
}

fn definitions(instruction: Instr, superinstructions: &[Superinstruction]) -> u64 {
    match instruction.op() {
        Op::Binary | Op::NumericAdd | Op::NumericMultiply
            if instruction.a() & NUMERIC_LOCAL_TARGET != 0 =>
        {
            0
        }
        Op::LoadConst
        | Op::LoadLocal
        | Op::LoadEnvLocal
        | Op::LoadCapture
        | Op::LoadName
        | Op::LoadThis
        | Op::MakeClosure
        | Op::MakeArray
        | Op::MakeConstArray
        | Op::MakeObject
        | Op::MakeObject2
        | Op::GetIterator
        | Op::GetAsyncIterator
        | Op::GetField
        | Op::GetIndex
        | Op::Binary
        | Op::NumericAdd
        | Op::NumericMultiply
        | Op::IncDec
        | Op::Unary
        | Op::Move
        | Op::Call
        | Op::CallKnown
        | Op::CallMethod
        | Op::CallThisMethod
        | Op::Construct
            if instruction.a() & RETURN_REGISTER == 0 =>
        {
            bit(instruction.a() & REGISTER_MASK)
        }
        Op::SuperConstArrayObject2 => superinstructions[instruction.imm() as usize]
            .code
            .iter()
            .fold(0, |mask, nested| {
                mask | definitions(*nested, superinstructions)
            }),
        Op::StoreLocal | Op::StoreEnvLocal if instruction.b() != 0 => bit(instruction.b() - 1),
        _ => 0,
    }
}

fn field_base(instruction: Instr, fields: &[FieldSite]) -> u64 {
    if instruction.b() == FieldBase::NESTED {
        fields
            .get(instruction.imm() as usize)
            .and_then(|site| site.base.register_index())
            .map_or(0, bit)
    } else {
        FieldBase(instruction.b()).register_index().map_or(0, bit)
    }
}

fn operand(value: u16, fields: &[FieldSite]) -> u64 {
    let operand = Operand(value);
    if let Some(register) = operand.register_index() {
        return bit(register);
    }
    if operand.tag() == 2 {
        return fields
            .get(operand.payload() as usize)
            .and_then(|site| site.base.register_index())
            .map_or(0, bit);
    }
    0
}

fn method_arguments(instruction: Instr, methods: &[MethodSite]) -> u64 {
    methods[instruction.imm() as usize]
        .2
        .iter()
        .copied()
        .fold(0, |mask, register| mask | bit(register))
}

fn range(base: u16, count: u16) -> u64 {
    (0..count).fold(0, |mask, offset| mask | bit(base + offset))
}

fn bit(register: u16) -> u64 {
    1 << register
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_roots_are_derived_from_successor_uses() {
        let function = Function {
            parent: None,
            name: None,
            params: 0,
            rest: false,
            is_async: false,
            is_generator: false,
            arguments_slot: None,
            locals: 0,
            code: vec![
                Instr::new(Op::LoadConst, 0, 0, 0, 0),
                Instr::new(Op::Jump, 0, 0, 0, 3),
                Instr::new(Op::Return, 0, 0, 0, 0),
                Instr::new(Op::Return, 1, 0, 0, 0),
            ],
            wide: vec![],
            registers: 2,
            dispatch: crate::bytecode::DispatchClass::General,
            handlers: vec![],
            register_root_offset: u32::MAX,
        };
        let roots = analyze(&function, &[], &[], &[]).unwrap();
        assert_eq!(roots[3], bit(1));
        assert_eq!(roots[1], bit(1));
        assert_eq!(roots[1] & bit(0), 0);
    }

    #[test]
    fn local_target_does_not_define_a_register() {
        let instruction = Instr::new(Op::Binary, NUMERIC_LOCAL_TARGET | 2, 0, 1, 8);
        assert_eq!(definitions(instruction, &[]), 0);
    }
}
