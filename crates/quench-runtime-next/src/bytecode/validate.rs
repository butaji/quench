use super::control_flow::instruction_at;
use super::{
    DispatchClass, FieldBase, Op, Operand, OperandKind, REGISTER_MASK, Register, ResidualProgram,
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
    atom as usize <= atoms.saturating_sub(1)
}

fn cache_in_bounds(cache: u16, caches: u16) -> bool {
    cache < caches
}

fn register_window_in_bounds(base: u16, count: u32, registers: u16) -> bool {
    u32::from(base)
        .checked_add(count)
        .is_some_and(|end| end <= u32::from(registers))
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
            for (pc, packed) in function.code.iter().enumerate() {
                let Some(instruction) = instruction_at(function, *packed) else {
                    return Err(format!("function {index} wide instruction is invalid"));
                };
                if instruction.op() == Op::Wide {
                    return Err(format!("function {index} contains nested wide instruction"));
                }
                if !instruction.result_flags_valid() {
                    return Err(format!("function {index} result flags are invalid"));
                }
                let register = |value: u16| register_in_bounds(value, function.registers, 0);
                let destination =
                    |value: u16| register_in_bounds(value & REGISTER_MASK, function.registers, 0);
                let cache = |value: u16| cache_in_bounds(value, self.cache_sites);
                let atom = |value: u32| atom_in_bounds(value, self.atoms.len());
                match instruction.op() {
                    Op::LoadConst if instruction.constant_index() >= self.constants.len() => {
                        return Err(format!("function {index} constant load is invalid"));
                    }
                    Op::LoadLocal | Op::LoadEnvLocal
                        if instruction.local_slot() >= usize::from(function.locals) =>
                    {
                        return Err(format!("function {index} local access is invalid"));
                    }
                    Op::StoreLocal | Op::StoreEnvLocal | Op::InitializeTdz
                        if instruction.local_slot() >= usize::from(function.locals) =>
                    {
                        return Err(format!("function {index} local store is invalid"));
                    }
                    Op::LoadCapture | Op::StoreCapture
                        if usize::from(instruction.capture_depth()) >= self.functions.len() =>
                    {
                        return Err(format!("function {index} capture depth is invalid"));
                    }
                    Op::LoadName | Op::LoadNameTypeof | Op::StoreName | Op::DeleteName
                        if !atom(instruction.atom_index()) || !cache(instruction.c()) =>
                    {
                        return Err(format!("function {index} name site is invalid"));
                    }
                    Op::LoadResolvedName
                        if !atom(instruction.atom_index())
                            || !register(instruction.a())
                            || !register(instruction.b())
                            || instruction.c() > 1 =>
                    {
                        return Err(format!("function {index} resolved name site is invalid"));
                    }
                    Op::LoadNameCall
                        if !atom(instruction.atom_index())
                            || !cache(instruction.c())
                            || !destination(instruction.a())
                            || !destination(instruction.b()) =>
                    {
                        return Err(format!("function {index} call-name site is invalid"));
                    }
                    Op::ResolveName
                        if !atom(instruction.atom_index())
                            || !destination(instruction.a())
                            || instruction.b() > 1 =>
                    {
                        return Err(format!("function {index} name resolution is invalid"));
                    }
                    Op::StoreResolvedName
                        if !atom(instruction.atom_index())
                            || !register(instruction.a())
                            || !register(instruction.b())
                            || instruction.c() > 1 =>
                    {
                        return Err(format!("function {index} resolved name store is invalid"));
                    }
                    Op::MarkPrivateName
                        if !atom(instruction.atom_index())
                            || !register(instruction.b())
                            || !register(instruction.c()) =>
                    {
                        return Err(format!("function {index} private-name mark is invalid"));
                    }
                    Op::SetFunctionName
                        if !register(instruction.a()) || !atom(instruction.atom_index()) =>
                    {
                        return Err(format!("function {index} function name is invalid"));
                    }
                    Op::SetFunctionNameKey
                        if !register(instruction.a())
                            || !register(instruction.b())
                            || instruction.function_name_prefix()
                                > crate::bytecode::FUNCTION_NAME_PREFIX_SETTER =>
                    {
                        return Err(format!(
                            "function {index} computed function name is invalid"
                        ));
                    }
                    Op::MakeClosure
                        if instruction.closure_function_index() as usize
                            >= self.functions.len()
                            || !destination(instruction.a()) =>
                    {
                        return Err(format!("function {index} closure site is invalid"));
                    }
                    Op::MakeConstArray
                        if !destination(instruction.a())
                            || instruction
                                .constant_index()
                                .checked_add(instruction.element_count() as usize)
                                .is_none_or(|end| end > self.constants.len()) =>
                    {
                        return Err(format!("function {index} constant array is invalid"));
                    }
                    Op::MakeArray
                        if !destination(instruction.a())
                            || instruction.array_length() > usize::from(u16::MAX) =>
                    {
                        return Err(format!("function {index} array allocation is invalid"));
                    }
                    Op::GetField
                        if !destination(instruction.a())
                            || !field_base_in_bounds(instruction.b(), function.registers)
                            || match instruction.field_lookup() {
                                super::FieldLookup::Site(site) => site >= self.field_sites.len(),
                                super::FieldLookup::Atom(atom_index) => {
                                    !atom(atom_index) || !cache(instruction.c())
                                }
                            } =>
                    {
                        return Err(format!("function {index} field load is invalid"));
                    }
                    Op::SetField
                        if !register(instruction.register_a())
                            || !register(instruction.register_b())
                            || !atom(instruction.atom_index())
                            || !cache(instruction.cache_site_index()) =>
                    {
                        return Err(format!("function {index} field store is invalid"));
                    }
                    Op::DefineField
                        if !register(instruction.register_a())
                            || !register(instruction.register_b())
                            || !atom(instruction.atom_index()) =>
                    {
                        return Err(format!("function {index} field definition is invalid"));
                    }
                    Op::DefineComputedField
                        if !register(instruction.register_a())
                            || !register(instruction.register_b())
                            || !register(instruction.register_c()) =>
                    {
                        return Err(format!(
                            "function {index} computed field definition is invalid"
                        ));
                    }
                    Op::SetThisField
                        if !register(instruction.register_a())
                            || !atom(instruction.atom_index())
                            || !cache(instruction.cache_site_index()) =>
                    {
                        return Err(format!("function {index} this-field store is invalid"));
                    }
                    Op::CheckPrivate
                        if !register(instruction.a()) || !atom(instruction.atom_index()) =>
                    {
                        return Err(format!("function {index} private check is invalid"));
                    }
                    Op::PrivateIn
                        if !destination(instruction.a())
                            || !register(instruction.b())
                            || !atom(instruction.atom_index()) =>
                    {
                        return Err(format!("function {index} private-in operation is invalid"));
                    }
                    Op::GetIndex
                        if !register(instruction.result_register())
                            || !operand_in_bounds(
                                instruction.operand_b().0,
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            )
                            || (function.dispatch == DispatchClass::Numeric
                                && (!is_numeric_index_operand(instruction.operand_b())
                                    || !is_numeric_index_operand(instruction.operand_c())))
                            || !operand_in_bounds(
                                instruction.operand_c().0,
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            ) =>
                    {
                        return Err(format!("function {index} indexed load is invalid"));
                    }
                    Op::SetIndex
                        if !register(instruction.register_a())
                            || !register(instruction.register_b())
                            || !register(instruction.register_c())
                            || instruction.boolean_flag().is_none() =>
                    {
                        return Err(format!("function {index} indexed store is invalid"));
                    }
                    Op::DefineArrayElement
                        if !register(instruction.register_a())
                            || !register(instruction.register_b())
                            || instruction.array_index() == super::ARRAY_INDEX_SENTINEL =>
                    {
                        return Err(format!("function {index} array literal element is invalid"));
                    }
                    Op::Binary | Op::NumericAdd | Op::NumericMultiply
                        if !destination(instruction.a())
                            || match instruction.op() {
                                Op::Binary => {
                                    instruction.binary_operator()
                                        > oxc_ast::ast::BinaryOperator::Instanceof as u32
                                }
                                Op::NumericAdd => {
                                    instruction.binary_operator()
                                        != oxc_ast::ast::BinaryOperator::Addition as u32
                                }
                                Op::NumericMultiply => {
                                    instruction.binary_operator()
                                        != oxc_ast::ast::BinaryOperator::Multiplication as u32
                                }
                                _ => unreachable!("matched binary opcode family"),
                            }
                            || !operand_in_bounds(
                                instruction.operand_b().0,
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            )
                            || !operand_in_bounds(
                                instruction.operand_c().0,
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            ) =>
                    {
                        return Err(format!("function {index} binary operand is invalid"));
                    }
                    Op::Unary | Op::IncDec
                        if !register(instruction.result_register())
                            || !register(instruction.register_b())
                            || (instruction.op() == Op::Unary
                                && instruction.unary_operator()
                                    > oxc_ast::ast::UnaryOperator::Void as u32)
                            || (instruction.op() == Op::IncDec
                                && instruction.boolean_flag().is_none()) =>
                    {
                        return Err(format!("function {index} unary operand is invalid"));
                    }
                    Op::Delete
                        if !register(instruction.a())
                            || !register(instruction.b())
                            || !register(instruction.c())
                            || instruction.boolean_flag().is_none() =>
                    {
                        return Err(format!("function {index} delete operand is invalid"));
                    }
                    Op::Move
                        if !register(instruction.result_register())
                            || !register(instruction.register_b()) =>
                    {
                        return Err(format!("function {index} move operand is invalid"));
                    }
                    Op::LoadImportMeta if !destination(instruction.a()) => {
                        return Err(format!("function {index} import-meta load is invalid"));
                    }
                    Op::CacheTemplateObject if !register(instruction.a()) => {
                        return Err(format!("function {index} template-object cache is invalid"));
                    }
                    Op::LoadCachedTemplateObject if !destination(instruction.a()) => {
                        return Err(format!(
                            "function {index} cached template-object load is invalid"
                        ));
                    }
                    Op::CopyDataProperties
                        if !register(instruction.register_a())
                            || !register(instruction.register_b())
                            || !register(instruction.register_c()) =>
                    {
                        return Err(format!(
                            "function {index} copy-data-properties operand is invalid"
                        ));
                    }
                    Op::InitializeThis if !register(instruction.a()) => {
                        return Err(format!(
                            "function {index} initialized this operand is invalid"
                        ));
                    }
                    Op::GetIterator | Op::GetAsyncIterator | Op::SpreadToArray
                        if !register(instruction.result_register())
                            || !register(instruction.register_b()) =>
                    {
                        return Err(format!("function {index} iterator register is invalid"));
                    }
                    Op::Return | Op::Throw if !register(instruction.register_a()) => {
                        return Err(format!("function {index} result register is invalid"));
                    }
                    Op::IteratorClose | Op::RequireObjectCoercible | Op::RequireIteratorResult
                        if !register(instruction.register_b()) =>
                    {
                        return Err(format!("function {index} iterator operand is invalid"));
                    }
                    Op::IteratorCleanupPush
                        if !register(instruction.register_a())
                            || !register(instruction.register_b()) =>
                    {
                        return Err(format!(
                            "function {index} iterator cleanup register is invalid"
                        ));
                    }
                    Op::JumpFalse if !register(instruction.register_a()) => {
                        return Err(format!("function {index} branch register is invalid"));
                    }
                    Op::JumpBinaryFalse
                        if instruction.binary_operator_field()
                            > oxc_ast::ast::BinaryOperator::Instanceof as u32
                            || !operand_in_bounds(
                                instruction.operand_b().0,
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            )
                            || !operand_in_bounds(
                                instruction.operand_c().0,
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            ) =>
                    {
                        return Err(format!("function {index} branch operand is invalid"));
                    }
                    Op::Call | Op::CallDirectEvalArray
                        if !destination(instruction.a())
                            || !register(instruction.b())
                            || !register(instruction.c())
                            || !register_window_in_bounds(
                                u16::from(instruction.call_window().base),
                                u32::from(instruction.call_window().count),
                                function.registers,
                            ) =>
                    {
                        return Err(format!("function {index} call is invalid"));
                    }
                    Op::CallDirectEvalArray if instruction.call_window().count != 1 => {
                        return Err(format!(
                            "function {index} direct eval arguments are invalid"
                        ));
                    }
                    Op::CallKnown
                        if !destination(instruction.a())
                            || instruction.known_function_index() as usize
                                >= self.functions.len()
                            || !register_window_in_bounds(
                                u16::from(instruction.call_window().base),
                                u32::from(instruction.call_window().count),
                                function.registers,
                            ) =>
                    {
                        return Err(format!("function {index} known call is invalid"));
                    }
                    Op::CallMethod | Op::CallThisMethod
                        if !destination(instruction.a())
                            || instruction.method_site_index() >= self.method_sites.len() =>
                    {
                        return Err(format!("function {index} method call is invalid"));
                    }
                    Op::Construct
                        if !destination(instruction.a())
                            || !register(instruction.b())
                            || match instruction.construct_arguments() {
                                super::ConstructArguments::Registers(window) => {
                                    !register_window_in_bounds(
                                        window.base,
                                        u32::from(window.count),
                                        function.registers,
                                    )
                                }
                                super::ConstructArguments::Array(array_register) => {
                                    !register(array_register)
                                }
                            } =>
                    {
                        return Err(format!("function {index} construct is invalid"));
                    }
                    Op::MakeObject2
                        if !destination(instruction.a())
                            || !register(instruction.b())
                            || !register(instruction.c())
                            || instruction.object_site_index() >= self.object_sites.len() =>
                    {
                        return Err(format!("function {index} object site is invalid"));
                    }
                    Op::SuperConstArrayObject2
                        if !destination(instruction.a())
                            || instruction.superinstruction_index()
                                >= self.superinstructions.len() =>
                    {
                        return Err(format!("function {index} superinstruction is invalid"));
                    }
                    Op::Jump | Op::JumpFalse | Op::JumpBinaryFalse
                        if instruction.jump_target() >= code_len =>
                    {
                        return Err(format!("function {index} branch at {pc} is out of bounds"));
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
