//! Compact executable IR produced by lowering.
//!
//! Instructions contain only integer operands. Constants and uncommon source
//! information live in pools owned by `Program`, so dispatch never walks AST.

use crate::ops::Constant;
use std::collections::HashMap;

pub type Register = u16;
pub const MAX_REGISTER_ID: Register = u16::MAX;
pub type ConstantId = u16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Opcode {
    LoadConst = 1,
    Move = 2,
    Add = 3,
    AddConst = 4,
    JumpIfFalse = 5,
    Return = 6,
    Slow = 7,
}

impl Opcode {
    pub const COUNT: u8 = 7;

    pub const fn is_compact(self) -> bool {
        (self as u8) <= Self::COUNT
    }
    pub const fn is_slow(self) -> bool {
        matches!(self, Self::Slow)
    }
}
/// Four words makes hot instructions fixed-width and trivially indexable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Instruction {
    pub opcode: Opcode,
    pub flags: u8,
    pub a: u16,
    pub b: u16,
    pub c: u16,
}

impl Instruction {
    pub const BYTE_WIDTH: usize = 8;

    pub const fn load_const(dst: Register, constant: ConstantId) -> Self {
        Self {
            opcode: Opcode::LoadConst,
            flags: 0,
            a: dst,
            b: constant,
            c: 0,
        }
    }
    pub const fn move_(dst: Register, src: Register) -> Self {
        Self {
            opcode: Opcode::Move,
            flags: 0,
            a: dst,
            b: src,
            c: 0,
        }
    }
    pub const fn add(dst: Register, left: Register, right: Register) -> Self {
        Self {
            opcode: Opcode::Add,
            flags: 0,
            a: dst,
            b: left,
            c: right,
        }
    }
    pub const fn add_const(dst: Register, src: Register, constant: ConstantId) -> Self {
        Self {
            opcode: Opcode::AddConst,
            flags: 0,
            a: dst,
            b: src,
            c: constant,
        }
    }
    pub const fn ret(src: Register) -> Self {
        Self {
            opcode: Opcode::Return,
            flags: 0,
            a: src,
            b: 0,
            c: 0,
        }
    }

    pub const fn slow(flags: u8) -> Self {
        Self {
            opcode: Opcode::Slow,
            flags,
            a: 0,
            b: 0,
            c: 0,
        }
    }
    pub const fn jump_if_false(condition: Register, target: u16) -> Self {
        Self {
            opcode: Opcode::JumpIfFalse,
            flags: 0,
            a: condition,
            b: target,
            c: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RareMetadata {
    pub source_spans: Vec<(u32, u32)>,
    pub names: Vec<String>,
    pub debug_flags: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ConstantPool {
    values: Vec<Constant>,
    ids: HashMap<ConstantKey, ConstantId>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ConstantKey {
    Number(u64),
    Boolean(bool),
    String(String),
    StringUnits(Vec<u16>),
    BigInt(String),
    Null,
    Undefined,
}

impl ConstantPool {
    pub fn try_intern(&mut self, value: Constant) -> Result<ConstantId, &'static str> {
        let key = ConstantKey::from(&value);
        if let Some(&id) = self.ids.get(&key) {
            return Ok(id);
        }
        let id = u16::try_from(self.values.len()).map_err(|_| "constant pool exceeds u16 IDs")?;
        self.values.push(value);
        self.ids.insert(key, id);
        Ok(id)
    }

    pub fn intern(&mut self, value: Constant) -> ConstantId {
        self.try_intern(value)
            .expect("constant pool exceeds u16 IDs")
    }

    pub fn get(&self, id: ConstantId) -> Option<&Constant> {
        self.values.get(usize::from(id))
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl From<&Constant> for ConstantKey {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(v) => Self::Number(v.to_bits()),
            Constant::Boolean(v) => Self::Boolean(*v),
            Constant::String(v) => Self::String(v.clone()),
            Constant::StringUnits(v) => Self::StringUnits(v.clone()),
            Constant::BigInt(v) => Self::BigInt(v.clone()),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub instructions: Vec<Instruction>,
    pub constants: ConstantPool,
    pub rare: RareMetadata,
}

impl Program {
    pub fn load_constant(&mut self, dst: Register, value: Constant) {
        let id = self.constants.intern(value);
        self.instructions.push(Instruction::load_const(dst, id));
    }
    /// Fuse the measured hot pair `LoadConst; Add` without changing fallback semantics.
    pub fn fuse_load_const_add(&mut self) {
        let mut out = Vec::with_capacity(self.instructions.len());
        let mut i = 0;
        while i < self.instructions.len() {
            if i + 1 < self.instructions.len()
                && self.instructions[i].opcode == Opcode::LoadConst
                && self.instructions[i + 1].opcode == Opcode::Add
                && self.instructions[i].a == self.instructions[i + 1].b
            {
                let load = self.instructions[i];
                let add = self.instructions[i + 1];
                out.push(Instruction::add_const(add.a, add.c, load.b));
                i += 2;
            } else {
                out.push(self.instructions[i]);
                i += 1;
            }
        }
        self.instructions = out;
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        for instruction in &self.instructions {
            match instruction.opcode {
                Opcode::LoadConst | Opcode::AddConst
                    if self.constants.get(instruction.b).is_none() =>
                {
                    return Err("instruction references missing constant");
                }
                Opcode::JumpIfFalse if usize::from(instruction.b) >= self.instructions.len() => {
                    return Err("conditional jump target is out of range");
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_shared_and_instructions_fixed_width() {
        assert_eq!(std::mem::size_of::<Instruction>(), Instruction::BYTE_WIDTH);
        assert_eq!(std::mem::size_of::<Opcode>(), 1);
        assert_eq!(std::mem::size_of::<Instruction>(), 8);
        let mut p = Program::default();
        p.load_constant(0, Constant::Number(4.0));
        p.load_constant(1, Constant::Number(4.0));
        assert_eq!(p.constants.len(), 1);
    }

    #[test]
    fn opcodes_remain_compact_byte_identifiers() {
        assert_eq!(Opcode::COUNT, 7);
        assert!(Opcode::Slow.is_compact());
    }
    #[test]
    fn registers_are_compact_integer_ids() {
        assert!(Opcode::Slow.is_slow());
        assert!(!Opcode::Add.is_slow());
        assert_eq!(std::mem::size_of::<Register>(), 2);
        assert_eq!(MAX_REGISTER_ID, u16::MAX);
    }
    #[test]
    fn conditional_branch_uses_compact_register_and_target_operands() {
        let instruction = Instruction::jump_if_false(3, 7);
        assert_eq!(instruction.opcode, Opcode::JumpIfFalse);
        assert_eq!((instruction.a, instruction.b, instruction.c), (3, 7, 0));
    }
    #[test]
    fn fusion_preserves_register_and_constant_ids() {
        let mut p = Program::default();
        p.load_constant(0, Constant::Number(2.0));
        p.instructions.push(Instruction::add(2, 0, 1));
        p.fuse_load_const_add();
        assert_eq!(p.instructions, vec![Instruction::add_const(2, 1, 0)]);
    }

    #[test]
    fn validation_rejects_missing_constants() {
        let mut p = Program::default();
        p.instructions.push(Instruction::load_const(0, 4));
        assert_eq!(p.validate(), Err("instruction references missing constant"));
        p.load_constant(0, Constant::Undefined);
        p.instructions[0].b = 0;
        assert_eq!(p.validate(), Ok(()));
    }
    #[test]
    fn generic_slow_instruction_preserves_fallback_flags() {
        let instruction = Instruction::slow(3);
        assert_eq!(instruction.opcode, Opcode::Slow);
        assert_eq!(instruction.flags, 3);
        assert_eq!((instruction.a, instruction.b, instruction.c), (0, 0, 0));
    }
}
