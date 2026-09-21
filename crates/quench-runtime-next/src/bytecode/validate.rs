use super::{Op, ResidualProgram};

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
            for (pc, instruction) in function.code.iter().enumerate() {
                match instruction.op() {
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
            locals: 0,
            code,
            registers,
            dispatch: DispatchClass::General,
            handlers: vec![],
            register_root_offset: root,
        }
    }

    fn program(function: Function, roots: Vec<u64>) -> ResidualProgram {
        ResidualProgram {
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
}
