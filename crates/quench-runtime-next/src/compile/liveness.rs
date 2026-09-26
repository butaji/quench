use crate::bytecode::{
    ControlFlowLayout, FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout,
    ImmediateRole, Instr, InstructionField, Op, Operand, Register, ResultLayout, Superinstruction,
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
    let normal = match instruction.op().control_flow_layout() {
        ControlFlowLayout::Jump => live[instruction.jump_target() as usize],
        ControlFlowLayout::ConditionalJump => {
            fallthrough | live[instruction.jump_target() as usize]
        }
        ControlFlowLayout::Terminal => 0,
        ControlFlowLayout::Fallthrough if instruction.returns_from_frame() => 0,
        ControlFlowLayout::Fallthrough => fallthrough,
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
    let field_reads = [
        InstructionField::A,
        InstructionField::B,
        InstructionField::C,
    ]
    .into_iter()
    .fold(0, |mask, field| {
        mask | field_uses(instruction, field, fields)
    });
    let result = if instruction.op().result_layout().reads_result_register() {
        bit(instruction.result_register())
    } else {
        0
    };
    let packed_uses = match instruction.op().immediate_layout() {
        ImmediateLayout::CallWindow
        | ImmediateLayout::CallWindowWithEvalFlags
        | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags => {
            let window = instruction.call_window();
            range(window.base, window.count)
        }
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            bit(first) | bit(second)
        }
        _ => 0,
    };
    let indexed_uses = match instruction.op().immediate_role() {
        ImmediateRole::MethodSiteIndex => method_arguments(instruction, methods),
        ImmediateRole::SuperinstructionIndex => superinstructions
            [instruction.superinstruction_index()]
        .code
        .iter()
        .fold(0, |mask, nested| {
            mask | uses(*nested, methods, fields, superinstructions)
        }),
        _ => 0,
    };
    field_reads | result | packed_uses | indexed_uses
}

fn field_uses(instruction: Instr, field: InstructionField, fields: &[FieldSite]) -> u64 {
    let layout = instruction.op().field_layout(field);
    match layout {
        FieldLayout::Register | FieldLayout::ReadWriteRegister => {
            bit(field_register(instruction, field))
        }
        FieldLayout::Operand | FieldLayout::NumericIndexOperand => {
            let input_operand = match field {
                InstructionField::B => instruction.operand_b(),
                InstructionField::C => instruction.operand_c(),
                InstructionField::A => return 0,
            };
            operand(input_operand.0, fields)
        }
        FieldLayout::FieldBase => field_base(instruction, fields),
        FieldLayout::ConstructArguments => match instruction.construct_arguments() {
            crate::bytecode::ConstructArguments::Registers(window) => {
                range(window.base, window.count)
            }
            crate::bytecode::ConstructArguments::Array(register) => bit(register),
        },
        _ => 0,
    }
}

fn definitions(instruction: Instr, superinstructions: &[Superinstruction]) -> u64 {
    let result = if instruction.op().result_layout() != ResultLayout::NoResult
        && !instruction.returns_from_frame()
        && !instruction.writes_numeric_local()
    {
        bit(instruction.result_register())
    } else {
        0
    };
    let field_writes = [
        InstructionField::A,
        InstructionField::B,
        InstructionField::C,
    ]
    .into_iter()
    .fold(0, |mask, field| {
        mask | field_definitions(instruction, field)
    });
    let packed_definitions = match instruction.op().immediate_layout() {
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            bit(first) | bit(second)
        }
        _ => 0,
    };
    let indexed_definitions = match instruction.op().immediate_role() {
        ImmediateRole::SuperinstructionIndex => superinstructions
            [instruction.superinstruction_index()]
        .code
        .iter()
        .fold(0, |mask, nested| {
            mask | definitions(*nested, superinstructions)
        }),
        _ => 0,
    };
    result | field_writes | packed_definitions | indexed_definitions
}

fn field_definitions(instruction: Instr, field: InstructionField) -> u64 {
    match instruction.op().field_layout(field) {
        FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister => {
            bit(field_register(instruction, field))
        }
        FieldLayout::OptionalRegister => instruction.optional_register_b().map_or(0, bit),
        FieldLayout::NumericLocalTarget => instruction
            .numeric_local_store_target()
            .map_or(0, |target| bit(target.register)),
        _ => 0,
    }
}

fn field_register(instruction: Instr, field: InstructionField) -> Register {
    match field {
        InstructionField::A => instruction.register_a(),
        InstructionField::B => instruction.register_b(),
        InstructionField::C => instruction.register_c(),
    }
}

fn field_base(instruction: Instr, fields: &[FieldSite]) -> u64 {
    match instruction.field_lookup() {
        FieldLookup::Site(index) => fields
            .get(index)
            .and_then(|site| site.base.register_index())
            .map_or(0, bit),
        FieldLookup::Atom { base, .. } => base.register_index().map_or(0, bit),
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
