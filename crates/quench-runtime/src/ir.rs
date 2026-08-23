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
pub enum Opcode {
    LoadConst = 1,
    Move = 2,
    Add = 3,
    AddConst = 4,
    JumpIfFalse = 5,
    Return = 6,
    Slow = 7,
    LoadLocal = 8,
    Sub = 9,
    Mul = 10,
    Div = 11,
    GetProperty = 12,
    Call = 13,
}

impl Opcode {
    pub const COUNT: u8 = 13;
    pub const fn is_compact(self) -> bool {
        (self as u8) <= Self::COUNT
    }
    pub const fn is_slow(self) -> bool {
        matches!(self, Self::Slow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Instruction {
    pub opcode: Opcode,
    pub flags: u8,
    pub a: u16,
    pub b: u16,
    pub c: u16,
}

/// Deterministic summary of the compact instruction stream.
///
/// Counters are derived directly from the existing instruction data and do
/// not participate in dispatch or duplicate runtime semantics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpcodeMetrics {
    pub frequency: [u64; 14],
    pub operand_words: [u64; 14],
}

impl OpcodeMetrics {
    pub fn for_instructions(instructions: &[Instruction]) -> Self {
        let mut metrics = Self::default();
        for instruction in instructions {
            let index = usize::from(instruction.opcode as u8);
            metrics.frequency[index] += 1;
            metrics.operand_words[index] += u64::from(operand_width(instruction.opcode));
        }
        metrics
    }
}
/// Dispatch implementation selected for compact instructions.
///
/// Both strategies consume the canonical [`Opcode`] representation; the table
/// is only a lookup policy and never a second semantic model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchStrategy {
    Match,
    Table,
}

impl DispatchStrategy {
    /// Dispatch one opcode to its canonical handler slot.
    ///
    /// Both policies consume the canonical [`Opcode`] representation; the
    /// table is only a lookup policy and never a second semantic model.
    pub const fn dispatch(self, opcode: Opcode) -> u8 {
        match self {
            Self::Match => match opcode {
                Opcode::LoadConst => 1,
                Opcode::Move => 2,
                Opcode::Add => 3,
                Opcode::AddConst => 4,
                Opcode::JumpIfFalse => 5,
                Opcode::Return => 6,
                Opcode::Slow => 7,
                Opcode::LoadLocal => 8,
                Opcode::Sub => 9,
                Opcode::Mul => 10,
                Opcode::Div => 11,
                Opcode::GetProperty => 12,
                Opcode::Call => 13,
            },
            Self::Table => DISPATCH_TABLE[opcode as usize],
        }
    }

    pub const fn handler_slot(self, opcode: Opcode) -> u8 {
        self.dispatch(opcode)
    }

    /// Measure dispatch work for an instruction stream without executing it.
    pub fn measure(self, instructions: &[Instruction]) -> DispatchMeasurement {
        let mut measurement = DispatchMeasurement::default();
        for instruction in instructions {
            measurement.instructions += 1;
            measurement.handler_slots += u64::from(self.dispatch(instruction.opcode));
        }
        measurement
    }
}

/// Deterministic counters used to compare dispatch policies in focused tests
/// and profiling callers. They are derived from instructions and have no
/// effect on execution.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DispatchMeasurement {
    pub instructions: u64,
    pub handler_slots: u64,
}

/// Deterministic footprint comparison for the canonical instruction stream.
///
/// `fixed_bytes` is the owned `Instruction` array footprint (including its
/// fixed eight-byte record width). `compact_bytes` is the serialized footprint
/// of the same instructions: one opcode byte, one flags byte, and only the
/// operand words used by that opcode. This is measurement only; execution
/// continues to consume `Instruction` and therefore retains the complete
/// slow-path semantics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InstructionEncodingMetrics {
    pub instructions: u64,
    pub fixed_bytes: u64,
    pub compact_bytes: u64,
}

impl InstructionEncodingMetrics {
    pub fn for_instructions(instructions: &[Instruction]) -> Self {
        let instructions_count = instructions.len() as u64;
        let compact_bytes = instructions
            .iter()
            .map(|instruction| 2 + u64::from(operand_width(instruction.opcode)) * 2)
            .sum();
        Self {
            instructions: instructions_count,
            fixed_bytes: instructions_count * Instruction::BYTE_WIDTH as u64,
            compact_bytes,
        }
    }

    /// Select the smaller representation, preferring fixed-width on a tie.
    pub const fn selection(self) -> InstructionEncoding {
        if self.compact_bytes < self.fixed_bytes {
            InstructionEncoding::Compact
        } else {
            InstructionEncoding::FixedWidth
        }
    }
}

/// Canonical representation choice for an instruction stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionEncoding {
    FixedWidth,
    Compact,
}

const DISPATCH_TABLE: [u8; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];

/// Number of u16 operand words consumed by this opcode.
const fn operand_width(opcode: Opcode) -> u8 {
    match opcode {
        Opcode::Slow => 0,
        Opcode::LoadConst
        | Opcode::Move
        | Opcode::AddConst
        | Opcode::JumpIfFalse
        | Opcode::LoadLocal
        | Opcode::Return
        | Opcode::Call => 2,
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::GetProperty => 3,
    }
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
impl Instruction {
    pub const fn load_local(dst: Register, slot: Register) -> Self {
        Self {
            opcode: Opcode::LoadLocal,
            flags: 0,
            a: dst,
            b: slot,
            c: 0,
        }
    }
    pub const fn binary(opcode: Opcode, dst: Register, lhs: Register, rhs: Register) -> Self {
        Self {
            opcode,
            flags: 0,
            a: dst,
            b: lhs,
            c: rhs,
        }
    }
    pub const fn get_property(dst: Register, object: Register, key: Register) -> Self {
        Self {
            opcode: Opcode::GetProperty,
            flags: 1,
            a: dst,
            b: object,
            c: key,
        }
    }
    pub const fn call_zero_args(dst: Register, callee: Register) -> Self {
        Self {
            opcode: Opcode::Call,
            flags: 0,
            a: dst,
            b: callee,
            c: 0,
        }
    }
}

/// Lossless lowering for the fixed-width subset of the canonical Op IR.
/// Operations carrying pools, vectors, or nested code remain on the slow path.
pub fn lower_compact(op: &crate::ops::Op) -> Option<Instruction> {
    use crate::ops::{BinaryOp, Op};
    match op {
        Op::Move { dst, src } => Some(Instruction::move_(*dst, *src)),
        Op::LoadLocal { dst, slot } => Some(Instruction::load_local(*dst, *slot)),
        Op::Binary {
            dst,
            operator: BinaryOp::Add,
            lhs,
            rhs,
        } => Some(Instruction::binary(Opcode::Add, *dst, *lhs, *rhs)),
        Op::Binary {
            dst,
            operator: BinaryOp::Subtract,
            lhs,
            rhs,
        } => Some(Instruction::binary(Opcode::Sub, *dst, *lhs, *rhs)),
        Op::Binary {
            dst,
            operator: BinaryOp::Multiply,
            lhs,
            rhs,
        } => Some(Instruction::binary(Opcode::Mul, *dst, *lhs, *rhs)),
        Op::Binary {
            dst,
            operator: BinaryOp::Divide,
            lhs,
            rhs,
        } => Some(Instruction::binary(Opcode::Div, *dst, *lhs, *rhs)),
        Op::GetPropertyDynamic { dst, object, key } => {
            Some(Instruction::get_property(*dst, *object, *key))
        }
        Op::Call {
            dst,
            callee,
            receiver: None,
            args,
            spreads,
        } if args.is_empty() && spreads.is_empty() => {
            Some(Instruction::call_zero_args(*dst, *callee))
        }
        Op::Return { src } => Some(Instruction::ret(*src)),
        _ => None,
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

/// Deterministic footprint summary for the canonical constant pool.
///
/// `payload_bytes` counts each unique constant once (including a one-byte
/// type tag), while `index_bytes` is the fixed-width ID table.  This is an
/// accounting metric, not an allocator-size claim.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConstantPoolMetrics {
    pub entries: usize,
    pub payload_bytes: usize,
    pub index_bytes: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ConstantKey {
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
    /// Return the canonical ID without allocating or mutating the pool.
    pub fn lookup(&self, value: &Constant) -> Option<ConstantId> {
        self.ids.get(&ConstantKey::from(value)).copied()
    }

    pub fn metrics(&self) -> ConstantPoolMetrics {
        let payload_bytes = self
            .values
            .iter()
            .map(|value| {
                1 + match value {
                    Constant::Number(_) => 8,
                    Constant::Boolean(_) => 1,
                    Constant::String(value) => value.len(),
                    Constant::StringUnits(value) => value.len() * 2,
                    Constant::BigInt(value) => value.len(),
                    Constant::Null | Constant::Undefined => 0,
                }
            })
            .sum();
        ConstantPoolMetrics {
            entries: self.values.len(),
            payload_bytes,
            index_bytes: self.values.len() * std::mem::size_of::<ConstantId>(),
        }
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
    /// Append the fixed-width form when representable; callers retain the
    /// canonical Op for the slow path when this returns false.
    pub fn lower_op(&mut self, op: &crate::ops::Op) -> bool {
        if let Some(instruction) = lower_compact(op) {
            self.instructions.push(instruction);
            true
        } else {
            false
        }
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
        assert_eq!(Opcode::COUNT, 13);
        assert!(Opcode::Slow.is_compact());
    }
    #[test]
    fn lowers_common_ops_to_fixed_width_instructions() {
        use crate::ops::{BinaryOp, Op};
        assert_eq!(
            lower_compact(&Op::Move { dst: 1, src: 2 }),
            Some(Instruction::move_(1, 2))
        );
        assert_eq!(
            lower_compact(&Op::Binary {
                dst: 3,
                operator: BinaryOp::Add,
                lhs: 1,
                rhs: 2
            }),
            Some(Instruction::binary(Opcode::Add, 3, 1, 2))
        );
        assert_eq!(
            lower_compact(&Op::Call {
                dst: 0,
                callee: 4,
                receiver: None,
                args: vec![],
                spreads: vec![]
            }),
            Some(Instruction::call_zero_args(0, 4))
        );
        assert!(lower_compact(&Op::Call {
            dst: 0,
            callee: 4,
            receiver: None,
            args: vec![1],
            spreads: vec![false]
        })
        .is_none());
    }
    #[test]
    fn lowering_and_encoding_selection_share_operand_widths() {
        use crate::ops::{BinaryOp, Op};

        let ops = [
            Op::Move { dst: 0, src: 1 },
            Op::Binary {
                dst: 2,
                operator: BinaryOp::Add,
                lhs: 0,
                rhs: 1,
            },
        ];
        let instructions: Vec<_> = ops.iter().filter_map(lower_compact).collect();
        assert_eq!(instructions.len(), ops.len());

        // Move encodes to 6 compact bytes and Add to 8; aggregate selection
        // must use the same operand widths used by lowering.
        let metrics = InstructionEncodingMetrics::for_instructions(&instructions);
        assert_eq!(metrics.fixed_bytes, 16);
        assert_eq!(metrics.compact_bytes, 14);
        assert_eq!(metrics.selection(), InstructionEncoding::Compact);

        // Execution still owns fixed-width records regardless of the
        // measurement-only representation choice.
        assert!(instructions
            .iter()
            .all(|instruction| std::mem::size_of_val(instruction) == Instruction::BYTE_WIDTH));
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
    fn dispatch_policies_execute_same_handler_selection() {
        let opcodes = [
            Opcode::LoadConst,
            Opcode::Move,
            Opcode::Add,
            Opcode::AddConst,
            Opcode::JumpIfFalse,
            Opcode::Return,
            Opcode::Slow,
            Opcode::LoadLocal,
            Opcode::Sub,
            Opcode::Mul,
            Opcode::Div,
            Opcode::GetProperty,
            Opcode::Call,
        ];
        for opcode in opcodes {
            assert_eq!(
                DispatchStrategy::Match.dispatch(opcode),
                DispatchStrategy::Table.dispatch(opcode)
            );
        }
    }

    #[test]
    fn dispatch_strategies_match_and_measure_same_instruction_stream() {
        let instructions = [
            Instruction::load_const(0, 0),
            Instruction::add(1, 0, 2),
            Instruction::get_property(3, 1, 4),
            Instruction::ret(3),
            Instruction::slow(0),
        ];
        let matched = DispatchStrategy::Match.measure(&instructions);
        let table = DispatchStrategy::Table.measure(&instructions);
        assert_eq!(
            matched,
            DispatchMeasurement {
                instructions: 5,
                handler_slots: 29
            }
        );
        assert_eq!(matched, table);
    }

    #[test]
    fn constant_pool_lookup_is_read_only_and_ids_are_stable() {
        let mut pool = ConstantPool::default();
        let values = [
            Constant::Number(f64::NAN),
            Constant::Number(-0.0),
            Constant::String("ok".into()),
        ];
        let ids = values
            .iter()
            .cloned()
            .map(|value| pool.intern(value))
            .collect::<Vec<_>>();
        let before = pool.metrics();

        assert_eq!(ids, vec![0, 1, 2]);
        for (value, id) in values.iter().zip(ids.iter().copied()) {
            assert_eq!(pool.lookup(value), Some(id));
            assert_eq!(pool.intern(value.clone()), id);
            assert_eq!(pool.lookup(value), Some(id));
        }
        assert_eq!(pool.metrics(), before);
        assert_eq!(before.entries, ids.len());
        assert_eq!(
            before.index_bytes,
            before.entries * std::mem::size_of::<ConstantId>()
        );
    }
}
