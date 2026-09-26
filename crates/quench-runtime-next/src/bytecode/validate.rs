use super::control_flow::instruction_at;
use super::{
    DispatchClass, FieldBase, FieldLayout, ImmediateLayout, InstructionField, Op, Operand,
    OperandKind, REGISTER_MASK, Register, ResidualProgram, ResultLayout,
};

fn register_in_bounds(register: u16, limit: u16, flags: u16) -> bool {
    register & !(REGISTER_MASK | flags) == 0 && register & REGISTER_MASK < limit
}

fn field_base_in_bounds(base: u16, limit: u16) -> bool {
    base == FieldBase::THIS.0 || base == FieldBase::NESTED || base < REGISTER_MASK && base < limit
}

fn operand_in_bounds(
    operand: u16,
    registers: u16,
    locals: u16,
    constants: usize,
    fields: usize,
) -> bool {
    let operand = Operand(operand);
    match operand.kind() {
        Some(OperandKind::Register) => register_in_bounds(operand.payload(), registers, 0),
        Some(OperandKind::Constant) => usize::from(operand.payload()) < constants,
        Some(OperandKind::Field) => usize::from(operand.payload()) < fields,
        Some(OperandKind::Local) => operand.payload() < locals,
        None => false,
    }
}

fn is_numeric_index_operand(operand: Operand) -> bool {
    matches!(
        operand.kind(),
        Some(OperandKind::Register | OperandKind::Local)
    )
}

fn atom_in_bounds(atom: u32, atoms: usize) -> bool {
    (atom as usize) < atoms
}

fn cache_in_bounds(cache: u16, caches: u16) -> bool {
    cache < caches
}

fn register_window_in_bounds(base: u16, count: u32, registers: u16) -> bool {
    u32::from(base)
        .checked_add(count)
        .is_some_and(|end| end <= u32::from(registers))
}

#[derive(Clone, Copy)]
struct ValidationBounds {
    registers: u16,
    locals: u16,
    functions: usize,
    constants: usize,
    atoms: usize,
    field_sites: usize,
    cache_sites: u16,
    method_sites: usize,
    object_sites: usize,
    superinstructions: usize,
    code_len: u32,
}

fn field_domains_in_bounds(instruction: super::WideInstruction, bounds: ValidationBounds) -> bool {
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    if instruction.op().result_layout() != ResultLayout::NoResult
        && !register_in_bounds(instruction.result_register(), bounds.registers, 0)
    {
        return false;
    }

    fields.into_iter().all(
        |(field, value)| match instruction.op().field_layout(field) {
            FieldLayout::Register | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister => {
                register_in_bounds(value, bounds.registers, 0)
            }
            FieldLayout::OptionalRegister => instruction
                .optional_register_b()
                .is_none_or(|register| register_in_bounds(register, bounds.registers, 0)),
            FieldLayout::CacheSiteIndex if instruction.op() != Op::GetField => {
                cache_in_bounds(value, bounds.cache_sites)
            }
            FieldLayout::BooleanFlag => instruction.boolean_field(field).is_some(),
            FieldLayout::FunctionIndex => usize::from(value) < bounds.functions,
            FieldLayout::ElementCount => instruction
                .constant_index()
                .checked_add(usize::from(value))
                .is_some_and(|end| end <= bounds.constants),
            FieldLayout::Operand => operand_in_bounds(
                value,
                bounds.registers,
                bounds.locals,
                bounds.constants,
                bounds.field_sites,
            ),
            FieldLayout::BinaryOperator => {
                u32::from(value) <= oxc_ast::ast::BinaryOperator::Instanceof as u32
            }
            FieldLayout::FieldBase => match instruction.field_lookup() {
                super::FieldLookup::Site(site) => site < bounds.field_sites,
                super::FieldLookup::Atom {
                    atom,
                    base,
                    cache_site,
                } => {
                    field_base_in_bounds(base.0, bounds.registers)
                        && atom_in_bounds(atom, bounds.atoms)
                        && cache_in_bounds(cache_site, bounds.cache_sites)
                }
            },
            _ => true,
        },
    )
}

fn immediate_domains_in_bounds(
    instruction: super::WideInstruction,
    bounds: ValidationBounds,
) -> bool {
    match instruction.op().immediate_role() {
        super::ImmediateRole::ConstantIndex => instruction.constant_index() < bounds.constants,
        super::ImmediateRole::ClosureFunctionIndex => {
            (instruction.closure_function_index() as usize) < bounds.functions
        }
        super::ImmediateRole::LocalSlot => instruction.local_slot() < usize::from(bounds.locals),
        super::ImmediateRole::AtomIndex => atom_in_bounds(instruction.atom_index(), bounds.atoms),
        super::ImmediateRole::BooleanFlag => instruction.boolean_flag().is_some(),
        super::ImmediateRole::ArrayIndex => {
            instruction.array_index() != super::ARRAY_INDEX_SENTINEL
        }
        super::ImmediateRole::BinaryOperator => {
            instruction.binary_operator() <= oxc_ast::ast::BinaryOperator::Instanceof as u32
        }
        super::ImmediateRole::UnaryOperator => {
            instruction.unary_operator() <= oxc_ast::ast::UnaryOperator::Void as u32
        }
        super::ImmediateRole::FunctionNamePrefix => {
            instruction.function_name_prefix() <= super::FUNCTION_NAME_PREFIX_SETTER
        }
        super::ImmediateRole::ArrayLength => instruction.array_length() <= usize::from(u16::MAX),
        super::ImmediateRole::MethodSiteIndex => {
            (instruction.method_site_index() as usize) < bounds.method_sites
        }
        super::ImmediateRole::ObjectSiteIndex => {
            (instruction.object_site_index() as usize) < bounds.object_sites
        }
        super::ImmediateRole::SuperinstructionIndex => {
            (instruction.superinstruction_index() as usize) < bounds.superinstructions
        }
        super::ImmediateRole::JumpTarget => instruction.jump_target() < bounds.code_len,
        _ => true,
    }
}

fn packed_layout_domains_in_bounds(
    instruction: super::WideInstruction,
    bounds: ValidationBounds,
) -> bool {
    match instruction.op().immediate_layout() {
        ImmediateLayout::CaptureDepthAndSlot => {
            usize::from(instruction.capture_depth()) < bounds.functions
        }
        ImmediateLayout::CallWindow | ImmediateLayout::CallWindowWithEvalFlags => {
            let window = instruction.call_window();
            register_window_in_bounds(
                u16::from(window.base),
                u32::from(window.count),
                bounds.registers,
            )
        }
        ImmediateLayout::ConstructCountAndFlags => match instruction.construct_arguments() {
            super::ConstructArguments::Registers(window) => {
                register_window_in_bounds(window.base, u32::from(window.count), bounds.registers)
            }
            super::ConstructArguments::Array(register) => {
                register_in_bounds(register, bounds.registers, 0)
            }
        },
        ImmediateLayout::RegisterPair => {
            let (first, second) = instruction.register_pair();
            register_in_bounds(first, bounds.registers, 0)
                && register_in_bounds(second, bounds.registers, 0)
        }
        ImmediateLayout::Scalar => true,
    }
}

impl ResidualProgram {
    /// Validate all cross-table references before a VM can observe the program.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.functions.is_empty() {
            return Err("program has no entry function".into());
        }
        if self.register_roots.len() > u32::MAX as usize {
            return Err("register root table is too large".into());
        }
        for (index, function) in self.functions.iter().enumerate() {
            if function.code.is_empty() || !super::control_flow::is_bounded(function) {
                return Err(format!("function {index} can fall off its code"));
            }
            if function.registers > REGISTER_MASK {
                return Err(format!("function {index} has too many registers"));
            }
            if function
                .global_var_atoms
                .iter()
                .any(|atom| !atom_in_bounds(*atom, self.atoms.len()))
            {
                return Err(format!("function {index} has an invalid global var atom"));
            }
            if function
                .parent
                .is_some_and(|p| p as usize >= self.functions.len())
            {
                return Err(format!("function {index} has an invalid parent"));
            }
            let code_len = function.code.len() as u32;
            let bounds = ValidationBounds {
                registers: function.registers,
                locals: function.locals,
                functions: self.functions.len(),
                constants: self.constants.len(),
                atoms: self.atoms.len(),
                field_sites: self.field_sites.len(),
                cache_sites: self.cache_sites,
                method_sites: self.method_sites.len(),
                object_sites: self.object_sites.len(),
                superinstructions: self.superinstructions.len(),
                code_len,
            };
            if function.parameter_end_pc > code_len
                || function.parameter_end_pc != 0 && !function.is_generator
            {
                return Err(format!(
                    "function {index} has an invalid parameter boundary"
                ));
            }
            if function.register_root_offset != u32::MAX {
                let root_end = function
                    .register_root_offset
                    .checked_add(code_len.saturating_add(1))
                    .ok_or_else(|| format!("function {index} root map overflows"))?;
                if root_end as usize > self.register_roots.len() {
                    return Err(format!("function {index} root map is out of bounds"));
                }
                if function.registers <= 64 {
                    let mask = if function.registers == 0 {
                        0
                    } else {
                        u64::MAX >> (64 - u32::from(function.registers))
                    };
                    let start = function.register_root_offset as usize;
                    let end = root_end as usize;
                    if self.register_roots[start..end]
                        .iter()
                        .any(|roots| roots & !mask != 0)
                    {
                        return Err(format!("function {index} root map has an invalid register"));
                    }
                }
            }
            for packed in &function.code {
                let Some(instruction) = instruction_at(function, *packed) else {
                    return Err(format!("function {index} wide instruction is invalid"));
                };
                if instruction.op() == Op::Wide {
                    return Err(format!("function {index} contains nested wide instruction"));
                }
                if !instruction.result_flags_valid() {
                    return Err(format!("function {index} result flags are invalid"));
                }
                if !instruction.unused_operands_are_zero() {
                    return Err(format!(
                        "function {index} {:?} has nonzero unused operands",
                        instruction.op()
                    ));
                }
                if !field_domains_in_bounds(instruction, bounds)
                    || !immediate_domains_in_bounds(instruction, bounds)
                    || !packed_layout_domains_in_bounds(instruction, bounds)
                {
                    return Err(format!(
                        "function {index} {:?} has an out-of-domain operand",
                        instruction.op()
                    ));
                }
                match instruction.op() {
                    Op::LoadLocal
                        if !instruction.numeric_local_store_fields_valid()
                            || instruction
                                .numeric_local_store_target()
                                .is_some_and(|target| {
                                    !register_in_bounds(target.register, function.registers, 0)
                                }) =>
                    {
                        return Err(format!("function {index} numeric local target is invalid"));
                    }
                    Op::GetIndex
                        if function.dispatch == DispatchClass::Numeric
                            && (!is_numeric_index_operand(instruction.operand_b())
                                || !is_numeric_index_operand(instruction.operand_c())) =>
                    {
                        return Err(format!("function {index} indexed load is invalid"));
                    }
                    Op::NumericAdd
                        if instruction.binary_operator()
                            != oxc_ast::ast::BinaryOperator::Addition as u32 =>
                    {
                        return Err(format!("function {index} numeric add opcode is invalid"));
                    }
                    Op::NumericMultiply
                        if instruction.binary_operator()
                            != oxc_ast::ast::BinaryOperator::Multiplication as u32 =>
                    {
                        return Err(format!(
                            "function {index} numeric multiply opcode is invalid"
                        ));
                    }
                    Op::CallDirectEvalArray if instruction.call_window().count != 1 => {
                        return Err(format!(
                            "function {index} direct eval arguments are invalid"
                        ));
                    }
                    _ => {}
                }
            }
            for handler in &function.handlers {
                if handler.start > handler.end
                    || handler.end > code_len
                    || handler.target >= code_len
                    || handler
                        .return_target
                        .is_some_and(|target| target >= code_len)
                    || handler.return_target.is_some() != handler.return_slot.is_some()
                {
                    return Err(format!("function {index} has an invalid handler"));
                }
                if handler.slot.is_some_and(|slot| slot >= function.locals) {
                    return Err(format!("function {index} handler slot is out of bounds"));
                }
                if handler
                    .return_slot
                    .is_some_and(|slot| slot >= function.locals)
                {
                    return Err(format!(
                        "function {index} handler return slot is out of bounds"
                    ));
                }
            }
        }
        for site in &self.method_sites {
            if !atom_in_bounds(site.atom, self.atoms.len())
                || !cache_in_bounds(site.cache, self.cache_sites)
                || (site.argument_start as usize)
                    .checked_add(site.argument_count as usize)
                    .is_none_or(|end| end > self.method_arguments.len())
                || site.receiver_path.is_some_and(|(atom, cache)| {
                    !atom_in_bounds(atom, self.atoms.len())
                        || !cache_in_bounds(cache, self.cache_sites)
                })
            {
                return Err("invalid method site".into());
            }
        }
        for site in &self.field_sites {
            if !field_base_in_bounds(site.base.0, REGISTER_MASK)
                || !atom_in_bounds(site.first.0, self.atoms.len())
                || !cache_in_bounds(site.first.1, self.cache_sites)
                || site.second.is_some_and(|(atom, cache)| {
                    !atom_in_bounds(atom, self.atoms.len())
                        || !cache_in_bounds(cache, self.cache_sites)
                })
                || site.sink.is_some_and(|(atom, cache)| {
                    !atom_in_bounds(atom, self.atoms.len())
                        || !cache_in_bounds(cache, self.cache_sites)
                })
            {
                return Err("invalid field site".into());
            }
        }
        for site in &self.object_sites {
            if site
                .atoms
                .iter()
                .any(|atom| !atom_in_bounds(*atom, self.atoms.len()))
            {
                return Err("invalid object site".into());
            }
        }
        if self
            .method_arguments
            .iter()
            .any(|register: &Register| *register > REGISTER_MASK)
        {
            return Err("invalid method argument register".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{AtomTable, DispatchClass, Function, Instr, ResidualProgram};

    fn function(code: Vec<Instr>, registers: u16, root: u32) -> Function {
        Function {
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
            code,
            wide: vec![],
            registers,
            dispatch: DispatchClass::General,
            handlers: vec![],
            register_root_offset: root,
        }
    }

    fn program(function: Function, roots: Vec<u64>) -> ResidualProgram {
        ResidualProgram {
            specialized: true,
            module: false,
            module_requests: Vec::new(),
            module_imports: Vec::new(),
            module_link_plan: None,
            source_name: String::new(),
            atoms: AtomTable::default(),
            constants: vec![],
            functions: vec![function],
            cache_sites: 0,
            method_sites: vec![],
            method_arguments: vec![],
            field_sites: vec![],
            object_sites: vec![],
            superinstructions: vec![],
            register_roots: roots,
        }
    }

    #[test]
    fn unmapped_functions_are_valid_and_mapped_roots_cover_each_pc() {
        let unmapped = program(
            function(vec![Instr::new(Op::Return, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(unmapped.validate().is_ok());

        let mapped = program(
            function(vec![Instr::new(Op::Return, 0, 0, 0, 0)], 1, 0),
            vec![1, 0],
        );
        assert!(mapped.validate().is_ok());
    }

    #[test]
    fn root_maps_reject_out_of_range_register_bits() {
        let invalid = program(
            function(vec![Instr::new(Op::Return, 0, 0, 0, 0)], 1, 0),
            vec![2, 0],
        );
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn table_and_operand_references_are_checked_before_execution() {
        let invalid_constant = program(
            function(vec![Instr::new(Op::LoadConst, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(invalid_constant.validate().is_err());

        let invalid_field = program(
            function(vec![Instr::new(Op::GetField, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(invalid_field.validate().is_err());

        let invalid_call = program(
            function(
                vec![Instr::new(
                    Op::Call,
                    0,
                    0,
                    0,
                    super::super::ImmediateLayout::call_immediate(1, 1, false, false),
                )],
                1,
                u32::MAX,
            ),
            vec![],
        );
        assert!(invalid_call.validate().is_err());
    }

    #[test]
    fn reachable_control_flow_cannot_fall_off_code() {
        let fallthrough = program(
            function(vec![Instr::new(Op::Nop, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(fallthrough.validate().is_err());

        let branch_fallthrough = program(
            function(vec![Instr::new(Op::JumpFalse, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(branch_fallthrough.validate().is_err());

        let loop_forever = program(
            function(vec![Instr::new(Op::Jump, 0, 0, 0, 0)], 1, u32::MAX),
            vec![],
        );
        assert!(loop_forever.validate().is_ok());
    }
}
