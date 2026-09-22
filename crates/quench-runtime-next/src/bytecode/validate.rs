use super::control_flow::instruction_at;
use super::{
    FieldBase, Op, Operand, REGISTER_MASK, RETURN_REGISTER, Register, ResidualProgram,
    SET_THIS_REGISTER,
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
    match operand.tag() {
        0 => register_in_bounds(operand.payload(), registers, 0),
        1 => usize::from(operand.payload()) < constants,
        2 => usize::from(operand.payload()) < fields,
        3 => operand.payload() < locals,
        _ => false,
    }
}

fn atom_in_bounds(atom: u32, atoms: usize) -> bool {
    atom as usize <= atoms.saturating_sub(1)
}

fn cache_in_bounds(cache: u16, caches: u16) -> bool {
    cache < caches
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
                .parent
                .is_some_and(|p| p as usize >= self.functions.len())
            {
                return Err(format!("function {index} has an invalid parent"));
            }
            let code_len = function.code.len() as u32;
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
                let register = |value: u16| register_in_bounds(value, function.registers, 0);
                let destination = |value: u16| {
                    register_in_bounds(
                        value,
                        function.registers,
                        RETURN_REGISTER | SET_THIS_REGISTER,
                    )
                };
                let cache = |value: u16| cache_in_bounds(value, self.cache_sites);
                let atom = |value: u32| atom_in_bounds(value, self.atoms.len());
                match instruction.op() {
                    Op::LoadConst if instruction.imm() as usize >= self.constants.len() => {
                        return Err(format!("function {index} constant load is invalid"));
                    }
                    Op::LoadLocal | Op::LoadEnvLocal
                        if instruction.imm() >= u32::from(function.locals) =>
                    {
                        return Err(format!("function {index} local access is invalid"));
                    }
                    Op::StoreLocal | Op::StoreEnvLocal
                        if instruction.imm() >= u32::from(function.locals) =>
                    {
                        return Err(format!("function {index} local store is invalid"));
                    }
                    Op::LoadCapture | Op::StoreCapture
                        if instruction.imm() >> 16 >= self.functions.len() as u32 =>
                    {
                        return Err(format!("function {index} capture depth is invalid"));
                    }
                    Op::LoadName | Op::StoreName
                        if !atom(instruction.imm()) || !cache(instruction.c()) =>
                    {
                        return Err(format!("function {index} name site is invalid"));
                    }
                    Op::MakeClosure
                        if instruction.imm() as usize >= self.functions.len()
                            || !destination(instruction.a()) =>
                    {
                        return Err(format!("function {index} closure site is invalid"));
                    }
                    Op::MakeConstArray
                        if !destination(instruction.a())
                            || (instruction.imm() as usize)
                                .checked_add(instruction.b() as usize)
                                .is_none_or(|end| end > self.constants.len()) =>
                    {
                        return Err(format!("function {index} constant array is invalid"));
                    }
                    Op::MakeArray
                        if !destination(instruction.a())
                            || instruction.imm() > u32::from(u16::MAX) =>
                    {
                        return Err(format!("function {index} array allocation is invalid"));
                    }
                    Op::GetField
                        if !destination(instruction.a())
                            || !field_base_in_bounds(instruction.b(), function.registers)
                            || if instruction.b() == FieldBase::NESTED {
                                instruction.imm() as usize >= self.field_sites.len()
                            } else {
                                !atom(instruction.imm()) || !cache(instruction.c())
                            } =>
                    {
                        return Err(format!("function {index} field load is invalid"));
                    }
                    Op::SetField
                        if !register(instruction.a())
                            || !register(instruction.b())
                            || !atom(instruction.imm())
                            || !cache(instruction.c()) =>
                    {
                        return Err(format!("function {index} field store is invalid"));
                    }
                    Op::SetThisField
                        if !register(instruction.a())
                            || !atom(instruction.imm())
                            || !cache(instruction.c()) =>
                    {
                        return Err(format!("function {index} this-field store is invalid"));
                    }
                    Op::GetIndex
                        if !register(instruction.a())
                            || !register(instruction.b())
                            || !register(instruction.c()) =>
                    {
                        return Err(format!("function {index} indexed load is invalid"));
                    }
                    Op::SetIndex
                        if !register(instruction.a())
                            || !register(instruction.b())
                            || !register(instruction.c()) =>
                    {
                        return Err(format!("function {index} indexed store is invalid"));
                    }
                    Op::Binary | Op::NumericAdd | Op::NumericMultiply
                        if !destination(instruction.a())
                            || !operand_in_bounds(
                                instruction.b(),
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            )
                            || !operand_in_bounds(
                                instruction.c(),
                                function.registers,
                                function.locals,
                                self.constants.len(),
                                self.field_sites.len(),
                            ) =>
                    {
                        return Err(format!("function {index} binary operand is invalid"));
                    }
                    Op::Unary | Op::IncDec
                        if !register(instruction.a()) || !register(instruction.b()) =>
                    {
                        return Err(format!("function {index} unary operand is invalid"));
                    }
                    Op::Move if !register(instruction.a()) || !register(instruction.b()) => {
                        return Err(format!("function {index} move operand is invalid"));
                    }
                    Op::GetIterator | Op::GetAsyncIterator | Op::Return | Op::Throw
                        if !register(instruction.a()) =>
                    {
                        return Err(format!("function {index} result register is invalid"));
                    }
                    Op::JumpFalse if !register(instruction.a()) => {
                        return Err(format!("function {index} branch register is invalid"));
                    }
                    Op::JumpBinaryFalse
                        if !operand_in_bounds(
                            instruction.b(),
                            function.registers,
                            function.locals,
                            self.constants.len(),
                            self.field_sites.len(),
                        ) || !operand_in_bounds(
                            instruction.c(),
                            function.registers,
                            function.locals,
                            self.constants.len(),
                            self.field_sites.len(),
                        ) =>
                    {
                        return Err(format!("function {index} branch operand is invalid"));
                    }
                    Op::Call | Op::CallKnown
                        if !destination(instruction.a())
                            || !register(instruction.b())
                            || !register(instruction.c())
                            || (instruction.imm() & u16::MAX as u32) > 8
                            || ((instruction.imm() >> 16) as u16)
                                .checked_add((instruction.imm() & u16::MAX as u32) as u16)
                                .is_none_or(|end| end > function.registers)
                            || (instruction.op() == Op::CallKnown
                                && instruction.b() as usize >= self.functions.len()) =>
                    {
                        return Err(format!("function {index} call is invalid"));
                    }
                    Op::CallMethod | Op::CallThisMethod
                        if !destination(instruction.a())
                            || instruction.imm() as usize >= self.method_sites.len() =>
                    {
                        return Err(format!("function {index} method call is invalid"));
                    }
                    Op::Construct
                        if !destination(instruction.a())
                            || !register(instruction.b())
                            || (instruction.imm() > 8)
                            || instruction
                                .c()
                                .checked_add(instruction.imm() as u16)
                                .is_none_or(|end| end > function.registers) =>
                    {
                        return Err(format!("function {index} construct is invalid"));
                    }
                    Op::MakeObject2
                        if !destination(instruction.a())
                            || !register(instruction.b())
                            || !register(instruction.c())
                            || instruction.imm() as usize >= self.object_sites.len() =>
                    {
                        return Err(format!("function {index} object site is invalid"));
                    }
                    Op::SuperConstArrayObject2
                        if !destination(instruction.a())
                            || instruction.imm() as usize >= self.superinstructions.len() =>
                    {
                        return Err(format!("function {index} superinstruction is invalid"));
                    }
                    Op::Jump | Op::JumpFalse | Op::JumpBinaryFalse
                        if instruction.imm() >= code_len =>
                    {
                        return Err(format!("function {index} branch at {pc} is out of bounds"));
                    }
                    Op::MakeClosure if instruction.imm() as usize >= self.functions.len() => {
                        return Err(format!("function {index} closure target is invalid"));
                    }
                    Op::CallKnown => {
                        if instruction.b() as usize >= self.functions.len() {
                            return Err(format!("function {index} call target is invalid"));
                        }
                        if instruction.imm() as u16 > 8 {
                            return Err(format!("function {index} call has too many arguments"));
                        }
                    }
                    _ => {}
                }
            }
            for handler in &function.handlers {
                if handler.start > handler.end
                    || handler.end > code_len
                    || handler.target >= code_len
                {
                    return Err(format!("function {index} has an invalid handler"));
                }
                if handler.slot.is_some_and(|slot| slot >= function.locals) {
                    return Err(format!("function {index} handler slot is out of bounds"));
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
            rest: false,
            is_async: false,
            is_generator: false,
            locals: 0,
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
                vec![Instr::new(Op::Call, 0, 0, 0, (1 << 16) | 1)],
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
