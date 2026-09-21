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
            let root_end = function
                .register_root_offset
                .checked_add(u32::from(function.registers))
                .ok_or_else(|| format!("function {index} root map overflows"))?;
            if root_end as usize > self.register_roots.len() {
                return Err(format!("function {index} root map is out of bounds"));
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
