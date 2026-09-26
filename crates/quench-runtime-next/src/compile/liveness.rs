use crate::bytecode::{
    FieldBase, FieldLookup, FieldSite, Function, Instr, Op, Operand, Register, Superinstruction,
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
        Op::Jump => live[instruction.jump_target() as usize],
        Op::JumpFalse | Op::JumpBinaryFalse => {
            fallthrough | live[instruction.jump_target() as usize]
        }
        Op::Return | Op::Throw => 0,
        _ if instruction.returns_from_frame() => 0,
        _ => fallthrough,
    };
    function
        .handlers
        .iter()
        .filter(|handler| pc as u32 >= handler.start && (pc as u32) < handler.end)
        .fold(normal, |mask, handler| {
            let exceptional = live[handler.target as usize];
            let returned = handler
                .return_target
                .map(|target| live[target as usize])
                .unwrap_or(0);
            mask | exceptional | returned
        })
}

fn add_suspended_exception_roots(function: &Function, live: &mut [u64]) {
    for (pc, instruction) in function.code.iter().enumerate() {
        if !matches!(
            instruction.op(),
            Op::Call
                | Op::CallDirectEvalArray
                | Op::CallKnown
                | Op::CallMethod
                | Op::CallThisMethod
                | Op::Construct
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
        Op::StoreResolvedName => bit(instruction.a()) | bit(instruction.b()),
        Op::LoadResolvedName => bit(instruction.b()),
        Op::ResolveName | Op::DeleteName | Op::LoadNameCall => 0,
        Op::GetIterator
        | Op::GetAsyncIterator
        | Op::IteratorClose
        | Op::SpreadToArray
        | Op::RequireObjectCoercible
        | Op::RequireIteratorResult => bit(instruction.register_b()),
        Op::IteratorCleanupPush => bit(instruction.register_a()) | bit(instruction.register_b()),
        Op::SetFunctionName => bit(instruction.a()),
        Op::SetFunctionNameKey => bit(instruction.a()) | bit(instruction.b()),
        Op::GetField => field_base(instruction, fields),
        Op::CheckPrivate => bit(instruction.a()),
        Op::PrivateIn => bit(instruction.b()),
        Op::GetIndex => {
            operand(instruction.operand_b().0, fields) | operand(instruction.operand_c().0, fields)
        }
        Op::ToPropertyKey | Op::ToNumeric => bit(instruction.b()),
        Op::CopyDataProperties => {
            bit(instruction.a()) | bit(instruction.b()) | bit(instruction.c())
        }
        Op::MakeObject2 => bit(instruction.b()) | bit(instruction.c()),
        Op::SuperConstArrayObject2 => superinstructions[instruction.superinstruction_index()]
            .code
            .iter()
            .fold(0, |mask, nested| {
                mask | uses(*nested, methods, fields, superinstructions)
            }),
        Op::SetField | Op::DefineField => {
            bit(instruction.register_a()) | bit(instruction.register_b())
        }
        Op::DefineComputedField => {
            bit(instruction.register_a())
                | bit(instruction.register_b())
                | bit(instruction.register_c())
        }
        Op::SetThisField => bit(instruction.register_a()),
        Op::InitializeThis => bit(instruction.a()),
        Op::YieldStar => {
            let (state, next_method) = instruction.register_pair();
            bit(instruction.a())
                | bit(instruction.b())
                | bit(instruction.c())
                | bit(state)
                | bit(next_method)
        }
        Op::SetIndex => {
            bit(instruction.register_a())
                | bit(instruction.register_b())
                | bit(instruction.register_c())
        }
        Op::DefineArrayElement => bit(instruction.register_a()) | bit(instruction.register_b()),
        Op::Binary | Op::NumericAdd | Op::NumericMultiply | Op::JumpBinaryFalse => {
            operand(instruction.operand_b().0, fields) | operand(instruction.operand_c().0, fields)
        }
        Op::IncDec | Op::Unary | Op::Move => bit(instruction.register_b()),
        Op::Delete => bit(instruction.b()) | bit(instruction.c()),
        Op::JumpFalse | Op::Return | Op::Throw => bit(instruction.register_a()),
        Op::Call | Op::CallDirectEvalArray => {
            let window = instruction.call_window();
            bit(instruction.b()) | bit(instruction.c()) | range(window.base, window.count)
        }
        Op::CallKnown => {
            let window = instruction.call_window();
            range(window.base, window.count)
        }
        Op::CallMethod => bit(instruction.b()) | method_arguments(instruction, methods),
        Op::CallThisMethod => method_arguments(instruction, methods),
        Op::Construct => {
            let arguments = match instruction.construct_arguments() {
                crate::bytecode::ConstructArguments::Registers(window) => {
                    range(window.base, window.count)
                }
                crate::bytecode::ConstructArguments::Array(register) => bit(register),
            };
            bit(instruction.b()) | arguments
        }
        _ => 0,
    }
}

fn definitions(instruction: Instr, superinstructions: &[Superinstruction]) -> u64 {
    match instruction.op() {
        Op::LoadNameCall => bit(instruction.a()) | bit(instruction.b()),
        Op::Binary | Op::NumericAdd | Op::NumericMultiply if instruction.writes_numeric_local() => {
            0
        }
        Op::LoadConst
        | Op::LoadLocal
        | Op::LoadEnvLocal
        | Op::LoadCapture
        | Op::LoadName
        | Op::LoadNameTypeof
        | Op::ResolveName
        | Op::LoadResolvedName
        | Op::ToPropertyKey
        | Op::ToNumeric
        | Op::LoadThis
        | Op::LoadImportMeta
        | Op::MakeClosure
        | Op::MakeArray
        | Op::MakeConstArray
        | Op::MakeObject
        | Op::MakeObject2
        | Op::GetIterator
        | Op::GetAsyncIterator
        | Op::SpreadToArray
        | Op::GetField
        | Op::GetIndex
        | Op::PrivateIn
        | Op::Binary
        | Op::NumericAdd
        | Op::NumericMultiply
        | Op::IncDec
        | Op::Unary
        | Op::Delete
        | Op::Move
        | Op::Call
        | Op::CallDirectEvalArray
        | Op::CallKnown
        | Op::CallMethod
        | Op::CallThisMethod
        | Op::Construct
            if !instruction.returns_from_frame() =>
        {
            bit(instruction.result_register())
        }
        Op::YieldStar => {
            let (state, next_method) = instruction.register_pair();
            bit(instruction.a()) | bit(instruction.c()) | bit(state) | bit(next_method)
        }
        Op::SuperConstArrayObject2 => superinstructions[instruction.superinstruction_index()]
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
    match instruction.field_lookup() {
        FieldLookup::Site(index) => fields
            .get(index)
            .and_then(|site| site.base.register_index())
            .map_or(0, bit),
        FieldLookup::Atom(_) => FieldBase(instruction.b()).register_index().map_or(0, bit),
    }
}

fn operand(value: u16, fields: &[FieldSite]) -> u64 {
    let operand = Operand(value);
    if let Some(register) = operand.register_index() {
        return bit(register);
    }
    if operand.kind() == Some(crate::bytecode::OperandKind::Field) {
        return fields
            .get(operand.payload() as usize)
            .and_then(|site| site.base.register_index())
            .map_or(0, bit);
    }
    0
}

fn method_arguments(instruction: Instr, methods: &[MethodSite]) -> u64 {
    methods[instruction.method_site_index()]
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
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![],
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            super_home_atom: None,
            constructible: true,
            class_field_initializer: false,
            parameter_eval_arguments_error: false,
            arguments_slot: None,
            strict: false,
            locals: 0,
            local_atoms: vec![],
            lexical_atoms: vec![],
            global_lexical_atoms: vec![],
            global_var_atoms: vec![],
            global_function_atoms: vec![],
            global_immutable_atoms: vec![],
            eval_sites: vec![],
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
        let instruction = Instr::new(
            Op::Binary,
            crate::bytecode::NUMERIC_LOCAL_TARGET | 2,
            0,
            1,
            8,
        );
        assert_eq!(definitions(instruction, &[]), 0);
    }
}
