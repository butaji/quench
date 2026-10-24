use crate::bytecode::{
    ControlFlowLayout, FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout,
    ImmediateRole, Instr, InstructionField, Operand, Register, Superinstruction, WideInstruction,
};

pub(super) type MethodSite = (u32, u16, Vec<Register>, Option<(u32, u16)>);

pub(crate) fn derive(
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
    let transfers = transfers(function, methods, fields, superinstructions)?;
    Some(analyze_transfers(function, &transfers))
}

/// Each instruction's (uses, definitions) register masks, for a function whose registers fit
/// the masks; liveness iterates only the live sets over this fixed table.
pub(super) fn transfers(
    function: &Function,
    methods: &[MethodSite],
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> Option<Vec<(u64, u64)>> {
    if function.registers > 64 {
        return None;
    }
    Some(
        function
            .instructions()
            .map(|instruction| {
                (
                    uses(instruction, methods, fields, superinstructions),
                    definitions(instruction, superinstructions),
                )
            })
            .collect(),
    )
}

/// Live registers before each instruction. A reverse pass settles every instruction whose
/// successors all follow it, so only code with a backward successor iterates to a fixed point.
pub(super) fn analyze_transfers(function: &Function, transfers: &[(u64, u64)]) -> Vec<u64> {
    let decoded = function.instructions().collect::<Vec<_>>();
    let has_backward_successor = decoded.iter().enumerate().any(|(pc, &instruction)| {
        let mut backward = false;
        for_each_successor(function, pc, instruction, |successor| {
            backward |= successor <= pc
        });
        backward
    });
    let mut live = vec![0; function.code.len() + 1];
    loop {
        let mut changed = false;
        for pc in (0..function.code.len()).rev() {
            let output = live_out(function, &live, pc, decoded[pc]);
            let (read, written) = transfers[pc];
            let next = read | (output & !written);
            if live[pc] != next {
                live[pc] = next;
                changed = true;
            }
        }
        if !changed || !has_backward_successor {
            break;
        }
    }
    add_suspended_exception_roots(function, &mut live);
    live
}

/// The instructions control can reach from `instruction` at `pc`, normally or through a
/// handler covering it. `pc + 1` stands for the function's end after its last instruction.
fn for_each_successor(
    function: &Function,
    pc: usize,
    instruction: WideInstruction,
    mut successor: impl FnMut(usize),
) {
    match instruction.op().control_flow_layout() {
        ControlFlowLayout::Jump => successor(instruction.jump_target() as usize),
        ControlFlowLayout::ConditionalJump => {
            successor(pc + 1);
            successor(instruction.jump_target() as usize);
        }
        ControlFlowLayout::BranchTable => {
            crate::bytecode::branch_table_entries(pc, instruction).for_each(&mut successor)
        }
        ControlFlowLayout::Terminal => {}
        ControlFlowLayout::Call | ControlFlowLayout::Fallthrough
            if instruction.returns_from_frame() => {}
        ControlFlowLayout::Call | ControlFlowLayout::Fallthrough => successor(pc + 1),
    }
    for handler in &function.handlers {
        if pc as u32 >= handler.start && (pc as u32) < handler.end {
            successor(handler.target as usize);
            if let Some(target) = handler.return_target {
                successor(target as usize);
            }
        }
    }
}

pub(super) fn live_out(
    function: &Function,
    live: &[u64],
    pc: usize,
    instruction: WideInstruction,
) -> u64 {
    let mut output = 0;
    for_each_successor(function, pc, instruction, |successor| {
        output |= live.get(successor).copied().unwrap_or(0);
    });
    output
}

fn add_suspended_exception_roots(function: &Function, live: &mut [u64]) {
    for (pc, instruction) in function.instructions().enumerate() {
        if instruction.op().control_flow_layout() != ControlFlowLayout::Call {
            continue;
        }
        for handler in &function.handlers {
            if pc as u32 >= handler.start && (pc as u32) < handler.end {
                live[pc + 1] |= live[handler.target as usize];
            }
        }
    }
}

pub(super) fn uses(
    instruction: WideInstruction,
    methods: &[MethodSite],
    fields: &[FieldSite],
    superinstructions: &[Superinstruction],
) -> u64 {
    let field_reads = InstructionField::ALL
        .iter()
        .copied()
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
            mask | nested_member(*nested).map_or(u64::MAX, |nested| {
                uses(nested, methods, fields, superinstructions)
            })
        }),
        _ => 0,
    };
    field_reads | result | packed_uses | indexed_uses
}

fn field_uses(instruction: WideInstruction, field: InstructionField, fields: &[FieldSite]) -> u64 {
    let layout = instruction.op().field_layout(field);
    match layout {
        FieldLayout::RegisterWindowBase => {
            let window = instruction.register_window();
            range(window.base, window.count)
        }
        layout if layout.reads_register() => bit(field_register(instruction, field)),
        layout if layout.is_operand_field() => {
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

pub(super) fn definitions(
    instruction: WideInstruction,
    superinstructions: &[Superinstruction],
) -> u64 {
    let result = if instruction.op().has_result_register()
        && !instruction.returns_from_frame()
        && !instruction.writes_numeric_local()
    {
        bit(instruction.result_register())
    } else {
        0
    };
    let field_writes = InstructionField::ALL
        .iter()
        .copied()
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
            mask | nested_member(*nested).map_or(0, |nested| definitions(nested, superinstructions))
        }),
        _ => 0,
    };
    result | field_writes | packed_definitions | indexed_definitions
}

fn field_definitions(instruction: WideInstruction, field: InstructionField) -> u64 {
    let layout = instruction.op().field_layout(field);
    match layout {
        layout if layout.writes_register() => bit(field_register(instruction, field)),
        FieldLayout::OptionalRegister => instruction.optional_register_b().map_or(0, bit),
        FieldLayout::NumericLocalTarget => instruction
            .numeric_local_store_target()
            .map_or(0, |target| bit(target.register)),
        _ => 0,
    }
}

fn field_register(instruction: WideInstruction, field: InstructionField) -> Register {
    instruction.field_value(field)
}

fn field_base(instruction: WideInstruction, fields: &[FieldSite]) -> u64 {
    match instruction.field_lookup() {
        None => 0,
        Some(FieldLookup::Site(index)) => fields
            .get(index)
            .and_then(|site| site.base.register_index())
            .map_or(0, bit),
        Some(FieldLookup::Atom { base, .. }) => base.register_index().map_or(0, bit),
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

fn method_arguments(instruction: WideInstruction, methods: &[MethodSite]) -> u64 {
    methods[instruction.method_site_index()]
        .2
        .iter()
        .copied()
        .fold(0, |mask, register| mask | bit(register))
}

/// A superinstruction member in fixed-width form. Members are packed without a wide table, so
/// a wide member has no decoding here: it counts as reading every register and writing none,
/// which only lengthens liveness.
fn nested_member(member: Instr) -> Option<WideInstruction> {
    (!member.is_wide()).then(|| member.as_wide())
}

fn range(base: u16, count: u16) -> u64 {
    (0..count).fold(0, |mask, offset| mask | bit(base + offset))
}

fn bit(register: u16) -> u64 {
    if u32::from(register) < u64::BITS {
        1 << register
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    #[test]
    fn branch_roots_are_derived_from_successor_uses() {
        let function = Function {
            parent: None,
            name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_span: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![],
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom: None,
            constructible: true,
            class_initializer: false,
            parameter_eval_arguments_error: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 0,
            local_atoms: vec![],
            environment_atoms: vec![],
            selective_capture_slots: None,
            local_registers: Vec::new(),
            inherited_with_scope: false,
            lexical_atoms: vec![],
            global_lexical_atoms: vec![],
            global_var_atoms: vec![],
            global_function_atoms: vec![],
            global_annex_b_var_atoms: vec![],
            global_immutable_atoms: vec![],
            name_bindings: Default::default(),
            binding_sites: vec![],
            source_positions: vec![],
            environment_clones: vec![],
            code: vec![
                Instr::new(Op::LoadConst, 0, 0, 0, 0),
                Instr::new(Op::Jump, 0, 0, 0, 3),
                Instr::new(Op::Return, 0, 0, 0, 0),
                Instr::new(Op::Return, 1, 0, 0, 0),
            ],
            wide: vec![],
            registers: 2,
            dispatch: crate::bytecode::DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            call_plan: Default::default(),
            handlers: vec![],
            register_root_offset: u32::MAX,
            undefined_register: None,
            parameter_registers: None,
            initial_register: Default::default(),
        };
        let roots = analyze(&function, &[], &[], &[]).unwrap();
        assert_eq!(roots[3], bit(1));
        assert_eq!(roots[1], bit(1));
        assert_eq!(roots[1] & bit(0), 0);
    }

    #[test]
    fn operand_windows_root_every_pending_input() {
        for op in [
            Op::WasmStructNew,
            Op::WasmStructNewDesc,
            Op::WasmStructNewDefaultDesc,
            Op::WasmArrayNewFixed,
            Op::WasmArrayNewData,
            Op::WasmArrayNewElem,
            Op::WasmExceptionNew,
            Op::WasmAtomicAccess,
        ] {
            let instruction = Instr::new(op, 0, 1, 3, 0);
            assert_eq!(
                uses(instruction.as_wide(), &[], &[], &[]),
                bit(1) | bit(2) | bit(3)
            );
            assert_eq!(definitions(instruction.as_wide(), &[]), bit(0));
            assert_eq!(uses(Instr::new(op, 0, 0, 0, 0).as_wide(), &[], &[], &[]), 0);
        }
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
        assert_eq!(definitions(instruction.as_wide(), &[]), 0);
    }
}
