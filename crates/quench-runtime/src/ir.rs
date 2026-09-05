//! Compact executable IR produced by lowering.
//!
//! Instructions contain only integer operands. Constants and uncommon source
//! information live in pools owned by `Program`, so dispatch never walks AST.

use crate::facts::{
    ControlFlow, OperationEffect, OperationGuard, OperationSpec, ResultShape, WordKind,
};
use crate::ops::Constant;
use std::collections::HashMap;

pub type Register = u16;
pub const MAX_REGISTER_ID: Register = u16::MAX;
pub type ConstantId = u16;
pub const GETN_GLOBAL_FLAG: u8 = 1;
pub const GETN_LENGTH_FLAG: u8 = 1 << 1;
/// `AddConst` keeps the source register in `b` and the pool entry in `c`.
/// This bit records whether the pool entry was the left operand in the
/// canonical `Binary(Add)` operation.  It is physical lowering metadata, not
/// a second arithmetic semantic.
pub const ADD_CONST_LEFT_FLAG: u8 = 1;

/// Operand roles derived from an operation's control fact.
///
/// The compact instruction keeps its canonical three-word shape; this view
/// gives the interpreter the semantic role of those words without a second
/// opcode/control table.  `Loop` intentionally preserves all three words for
/// the future loop residual, rather than guessing a physical convention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlOperands {
    Next,
    Branch { condition: Register, target: u16 },
    Jump { target: u16 },
    Return { source: Register },
    Loop { a: u16, b: u16, c: u16 },
}

/// Canonical register use/definition roles for the compact instruction.
/// Immediate slots and local/constant operands are excluded; `complete` is
/// false when an opcode carries a structured payload that needs the ordinary
/// handler rather than bounded physical composition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterFlow {
    pub uses: [Option<Register>; 3],
    pub definition: Option<Register>,
    pub complete: bool,
}

impl RegisterFlow {
    pub const fn none() -> Self {
        Self { uses: [None; 3], definition: None, complete: true }
    }

    pub const fn unary(definition: Register, source: Register) -> Self {
        Self { uses: [Some(source), None, None], definition: Some(definition), complete: true }
    }

    pub const fn binary(definition: Register, left: Register, right: Register) -> Self {
        Self { uses: [Some(left), Some(right), None], definition: Some(definition), complete: true }
    }

    pub const fn store(source: Register) -> Self {
        Self { uses: [Some(source), None, None], definition: None, complete: true }
    }
}

/// Uniform signature for generated compact dispatch handlers.
pub(crate) type CompactHandler =
    for<'a> fn(
        crate::machine::CodeView<'a>,
        usize,
        Instruction,
        &mut crate::register_file::RegisterFile,
        &crate::vm::VmContext,
    ) -> Result<crate::vm::DispatchTransition, crate::vm::VmError>;

macro_rules! vm_op {
    ($($name:ident = $id:literal / $width:literal => [$($effect:ident),*] / $fallback:ident / $result:ident / $control:ident / [$($guard:ident),*] / $handler:ident $(/ $operator:ident)?),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub enum Opcode { $($name = $id),+ }

        impl Opcode {
            pub const COUNT: u8 = vm_op!(@last $($id),+);

            /// Canonical opcode sequence generated from this declaration.
            /// Consumers that need exhaustive coverage (for example the
            /// physical dispatch catalog) borrow this view instead of
            /// maintaining a second runtime list.
            pub const ALL: &'static [Self] = &[$(Self::$name),+];

            pub const fn from_u8(value: u8) -> Option<Self> {
                match value { $($id => Some(Self::$name),)+ _ => None }
            }

            pub const fn operand_width(self) -> u8 {
                self.spec().operand_width
            }

            pub const fn compact_len(self) -> usize {
                2 + self.spec().operand_width as usize * 2
            }

            pub const fn operands_are_canonical(self, operands: [u16; 3]) -> bool {
                match self.spec().operand_width {
                    0 => operands[0] == 0 && operands[1] == 0 && operands[2] == 0,
                    1 => operands[1] == 0 && operands[2] == 0,
                    2 => operands[2] == 0,
                    _ => true,
                }
            }

            /// Decode exactly this operation's declared operand payload.
            pub fn decode_operands(self, bytes: &[u8]) -> Result<[u16; 3], &'static str> {
                let expected = usize::from(self.spec().operand_width) * 2;
                if bytes.len() != expected {
                    return Err("compact instruction has invalid operand width");
                }
                let mut operands = [0u16; 3];
                let mut index = 0;
                while index < usize::from(self.spec().operand_width) {
                    let start = index * 2;
                    operands[index] = u16::from_le_bytes([bytes[start], bytes[start + 1]]);
                    index += 1;
                }
                Ok(operands)
            }

            /// Return the generated semantic facts for this operation.
            pub const fn spec(self) -> &'static OperationSpec {
                &OPERATION_SPECS[self as usize - 1]
            }

            pub const fn name(self) -> &'static str {
                self.spec().name
            }

            pub const fn effects(self) -> &'static [OperationEffect] {
                self.spec().effects
            }

            pub const fn fallback(self) -> &'static str {
                self.spec().fallback
            }

            pub const fn has_effect(self, effect: OperationEffect) -> bool {
                self.spec().has_effect(effect)
            }

            pub const fn result_shape(self) -> ResultShape {
                self.spec().result
            }

            pub const fn control_flow(self) -> ControlFlow {
                self.spec().control
            }

            /// Decode control operand roles from the same catalog row that
            /// supplies [`control_flow`](Self::control_flow).
            pub const fn control_operands(self, instruction: Instruction) -> ControlOperands {
                match self {
                    $(Self::$name => vm_op!(@control $control, instruction)),+
                }
            }

            pub const fn guards(self) -> &'static [OperationGuard] {
                self.spec().guards
            }

            pub const fn has_guard(self, guard: OperationGuard) -> bool {
                self.spec().has_guard(guard)
            }

            pub const fn result_word_kind(self) -> WordKind {
                self.spec().result_word_kind()
            }

            pub const fn guarded_word_kind(self, guard: OperationGuard) -> Option<WordKind> {
                self.spec().guarded_word_kind(guard)
            }

            pub(crate) const fn handler(self) -> CompactHandler {
                match self {
                    $(Self::$name => crate::vm::$handler),+
                }
            }

            /// Generated direct dispatch for the interpreter hot loop.
            ///
            /// The same opcode facts still own the handler mapping; this
            /// direct view avoids an indirect function-pointer call at every
            /// retired instruction and lets LLVM inline eligible handlers.
            #[inline(always)]
            pub(crate) fn dispatch<'a>(
                self,
                code: crate::machine::CodeView<'a>,
                pc: usize,
                instruction: Instruction,
                registers: &mut crate::register_file::RegisterFile,
                context: &crate::vm::VmContext,
            ) -> Result<crate::vm::DispatchTransition, crate::vm::VmError> {
                match self {
                    $(Self::$name => crate::vm::$handler(
                        code, pc, instruction, registers, context,
                    )),+
                }
            }

            pub const fn handler_name(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($handler)),+
                }
            }

            pub const fn is_quickenable(self) -> bool {
                self.spec().is_quickenable()
            }

            /// Canonical certainty consumed by region-key derivation.  The
            /// stencil tier does not maintain a second eligibility table.
            pub const fn stencil_certainty(self) -> crate::facts::Certainty {
                if self.guards().is_empty() {
                    crate::facts::Certainty::Proven
                } else {
                    // Observable effects describe semantic behavior, not
                    // fact uncertainty. A guarded operation remains guarded;
                    // its complete fallback still owns those effects.
                    crate::facts::Certainty::Guarded
                }
            }

            pub const fn builder(self) -> CompactInstructionBuilder {
                CompactInstructionBuilder::new(self)
            }

            /// Numeric operators are derived from the same declaration as
            /// opcode IDs and effects. Non-arithmetic operations return None.
            pub const fn numeric_operator(self) -> Option<crate::ops::BinaryOp> {
                match self {
                    $(Self::$name => vm_op!(@operator $($operator)?)),+
                }
            }

        }

        const DISPATCH_TABLE: [u8; Opcode::COUNT as usize + 1] =
            [0, $($id),+];

        /// Generated view of the operation facts.  The opcode declaration is
        /// the only source for names, widths, effects, and fallback labels.
        pub const OPERATION_SPECS: &[OperationSpec] = &[
            $(OperationSpec {
                opcode: $id,
                name: stringify!($name),
                operand_width: $width,
                effects: &[$(OperationEffect::$effect),*],
                fallback: stringify!($fallback),
                result: ResultShape::$result,
                control: ControlFlow::$control,
                guards: &[$(OperationGuard::$guard),*],
            }),+
        ];

        const _: () = {
            let mut index = 0;
            while index < OPERATION_SPECS.len() {
                assert!(OPERATION_SPECS[index].validate());
                assert!(OPERATION_SPECS[index].opcode == (index as u8) + 1);
                assert!(OPERATION_SPECS[index].operand_width <= 3);
                index += 1;
            }
        };
    };
    (@last $head:literal, $($tail:literal),+) => { vm_op!(@last $($tail),+) };
    (@last $last:literal) => { $last };
    (@operator $operator:ident) => { Some(crate::ops::BinaryOp::$operator) };
    (@operator) => { None };
    (@control Next, $instruction:ident) => { ControlOperands::Next };
    (@control Branch, $instruction:ident) => {
        ControlOperands::Branch { condition: $instruction.a, target: $instruction.b }
    };
    (@control Jump, $instruction:ident) => {
        ControlOperands::Jump { target: $instruction.a }
    };
    (@control Return, $instruction:ident) => {
        ControlOperands::Return { source: $instruction.a }
    };
    (@control Loop, $instruction:ident) => {
        ControlOperands::Loop { a: $instruction.a, b: $instruction.b, c: $instruction.c }
    };
}

vm_op! {
    LoadConst = 1 / 2 => [Pure] / load_const / Value / Next / [] / run_load_const,
    Move = 2 / 2 => [Pure] / move / Value / Next / [] / run_move,
    Add = 3 / 3 => [MayThrow] / add / Value / Next / [Number] / run_arithmetic / Add,
    AddConst = 4 / 3 => [MayThrow] / add_const / Value / Next / [Number] / run_compact_add_const / Add,
    JumpIfFalse = 5 / 2 => [MayThrow, Control] / jump_if_false / None / Branch / [] / run_instruction_fallback,
    Return = 6 / 1 => [Control] / return_value / Value / Return / [] / run_return,
    Slow = 7 / 1 => [MayThrow, Observable] / slow / Value / Next / [] / run_instruction_fallback,
    LoadLocal = 8 / 2 => [Pure] / load_local / Value / Next / [] / run_local,
    Sub = 9 / 3 => [MayThrow] / subtract / Value / Next / [Number] / run_arithmetic / Subtract,
    Mul = 10 / 3 => [MayThrow] / multiply / Value / Next / [Number] / run_arithmetic / Multiply,
    Div = 11 / 3 => [MayThrow] / divide / Value / Next / [Number] / run_arithmetic / Divide,
    GetProperty = 12 / 3 => [ReadHeap, MayThrow, Observable] / get_property / Value / Next / [Shape] / run_compact_get_property,
    Call = 13 / 3 => [ReadHeap, MayThrow, Observable] / call / Value / Next / [Callable] / run_compact_call,
    Jump = 14 / 1 => [Control] / jump / None / Jump / [] / run_instruction_fallback,
    IncI = 15 / 2 => [MayThrow] / increment_integer / Value / Next / [] / run_compact_numeric_update,
    ForI = 16 / 3 => [Control] / for_integer / None / Loop / [] / run_instruction_fallback,
    AGetI = 17 / 3 => [ReadHeap, MayThrow, Observable] / get_element / Value / Next / [Shape] / run_compact_get_index,
    ASetI = 18 / 3 => [WriteHeap, MayThrow, Observable] / set_element / None / Next / [Shape] / run_compact_set_index,
    AGetIInc = 19 / 3 => [ReadHeap, WriteHeap, MayThrow, Observable] / get_element_increment / Value / Next / [Shape] / run_compact_get_index_inc,
    GetN = 20 / 3 => [ReadHeap, MayThrow, Observable] / get_named / Value / Next / [Shape] / run_compact_get_named,
    SetN = 21 / 3 => [WriteHeap, MayThrow, Observable] / set_named / None / Next / [Shape] / run_compact_set_named,
    CallN = 22 / 3 => [ReadHeap, MayThrow, Observable] / call_named / Value / Next / [Shape, Callable] / run_compact_call_named,
    UpdateLocal = 23 / 3 => [Pure] / update_local / Value / Next / [] / run_update_local,
    LoadLocalChecked = 24 / 2 => [MayThrow] / load_local_checked / Value / Next / [] / run_load_local_checked,
    Binary = 25 / 3 => [MayThrow] / binary / Value / Next / [] / run_binary_instruction,
    StoreLocalChecked = 26 / 2 => [MayThrow] / store_local_checked / None / Next / [] / run_store_local_checked,
    InitLocal = 27 / 2 => [Pure] / init_local / None / Next / [] / run_init_local,
    StoreLocal = 28 / 2 => [Pure] / store_local / None / Next / [] / run_store_local,
    GetPropertyQuickened = 29 / 3 => [ReadHeap, MayThrow, Observable] / get_property / Value / Next / [] / run_compact_get_property,
    GetNQuickened = 30 / 3 => [ReadHeap, MayThrow, Observable] / get_named / Value / Next / [] / run_compact_get_named,
    AGetIQuickened = 31 / 3 => [ReadHeap, MayThrow, Observable] / get_element / Value / Next / [] / run_compact_get_index,
    Unary = 32 / 3 => [MayThrow] / unary / Value / Next / [] / run_unary_instruction,
}

macro_rules! compact_binary_operators {
    ($($operator:ident = $id:literal),+ $(,)?) => {
        pub const fn compact_binary_id(operator: crate::ops::BinaryOp) -> u8 {
            match operator { $(crate::ops::BinaryOp::$operator => $id),+ }
        }

        pub const fn compact_binary_operator(id: u8) -> Option<crate::ops::BinaryOp> {
            match id { $($id => Some(crate::ops::BinaryOp::$operator),)+ _ => None }
        }
    };
}

compact_binary_operators! {
    Add = 0,
    Subtract = 1,
    Multiply = 2,
    Divide = 3,
    Remainder = 4,
    Exponentiate = 5,
    NumericAdd = 6,
    NumericSubtract = 7,
    Equal = 8,
    NotEqual = 9,
    StrictEqual = 10,
    StrictNotEqual = 11,
    LessThan = 12,
    LessEqual = 13,
    GreaterThan = 14,
    GreaterEqual = 15,
    BitwiseOr = 16,
    BitwiseXor = 17,
    BitwiseAnd = 18,
    ShiftLeft = 19,
    ShiftRight = 20,
    ShiftRightZeroFill = 21,
    Instanceof = 22,
}

macro_rules! compact_unary_operators {
    ($($operator:ident = $id:literal),+ $(,)?) => {
        pub const fn compact_unary_id(operator: crate::ops::UnaryOp) -> u8 {
            match operator { $(crate::ops::UnaryOp::$operator => $id),+ }
        }

        pub const fn compact_unary_operator(id: u8) -> Option<crate::ops::UnaryOp> {
            match id { $($id => Some(crate::ops::UnaryOp::$operator),)+ _ => None }
        }
    };
}

compact_unary_operators! {
    Plus = 0,
    Minus = 1,
    Not = 2,
    BitwiseNot = 3,
    Void = 4,
    Typeof = 5,
    ToString = 6,
    ToNumeric = 7,
    Delete = 8,
    IsNullish = 9,
}

impl Opcode {
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpcodeMetrics {
    pub frequency: [u64; Opcode::COUNT as usize + 1],
    pub operand_words: [u64; Opcode::COUNT as usize + 1],
}

impl Default for OpcodeMetrics {
    fn default() -> Self {
        Self {
            frequency: [0; Opcode::COUNT as usize + 1],
            operand_words: [0; Opcode::COUNT as usize + 1],
        }
    }
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
            Self::Match => opcode as u8,
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

/// Number of u16 operand words consumed by this opcode.
const fn operand_width(opcode: Opcode) -> u8 {
    opcode.operand_width()
}

/// Catalog-backed builder for the fixed-width instruction record.
///
/// Frontends can construct mechanical bytecode through this type without
/// copying opcode widths or inventing a second instruction representation.
/// Semantic fallback selection remains owned by the operation catalog and the
/// ordinary interpreter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactInstructionBuilder {
    opcode: Opcode,
    flags: u8,
    operands: [u16; 3],
}

impl CompactInstructionBuilder {
    pub const fn new(opcode: Opcode) -> Self {
        Self {
            opcode,
            flags: 0,
            operands: [0; 3],
        }
    }

    pub const fn flags(mut self, flags: u8) -> Self {
        self.flags = flags;
        self
    }

    pub const fn operands(mut self, a: u16, b: u16, c: u16) -> Self {
        self.operands = [a, b, c];
        self
    }

    pub const fn build(self) -> Result<Instruction, &'static str> {
        let width = self.opcode.operand_width();
        if width > 3 {
            return Err("operation declares too many operands");
        }
        if !self.opcode.operands_are_canonical(self.operands) {
            return Err("unused operand must be zero");
        }
        Ok(Instruction {
            opcode: self.opcode,
            flags: self.flags,
            a: self.operands[0],
            b: self.operands[1],
            c: self.operands[2],
        })
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

    /// Copy one proven local to another and leave the assigned value in `dst`.
    /// The Move opcode remains the single semantic declaration; its flag only
    /// selects the physical word owners named by the operands.
    pub const fn move_local(dst: Register, source: u16, target: u16) -> Self {
        Self {
            opcode: Opcode::Move,
            flags: 1,
            a: dst,
            b: source,
            c: target,
        }
    }
    pub const fn binary_operator(
        dst: Register,
        operator: crate::ops::BinaryOp,
        lhs: Register,
        rhs: Register,
    ) -> Self {
        Self {
            opcode: Opcode::Binary,
            flags: compact_binary_id(operator),
            a: dst,
            b: lhs,
            c: rhs,
        }
    }

    pub const fn unary_operator(dst: Register, operator: crate::ops::UnaryOp, src: Register) -> Self {
        Self {
            opcode: Opcode::Unary,
            flags: compact_unary_id(operator),
            a: dst,
            b: src,
            c: 0,
        }
    }

    pub const fn load_local_checked(dst: Register, slot: u16) -> Self {
        Self {
            opcode: Opcode::LoadLocalChecked,
            flags: 0,
            a: dst,
            b: slot,
            c: 0,
        }
    }
    pub const fn store_local_checked(slot: u16, src: Register) -> Self {
        Self {
            opcode: Opcode::StoreLocalChecked,
            flags: 0,
            a: slot,
            b: src,
            c: 0,
        }
    }
    pub const fn store_local(slot: u16, src: Register) -> Self {
        Self {
            opcode: Opcode::StoreLocal,
            flags: 0,
            a: slot,
            b: src,
            c: 0,
        }
    }
    pub const fn init_local(slot: u16, src: Register) -> Self {
        Self {
            opcode: Opcode::InitLocal,
            flags: 0,
            a: slot,
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

    pub const fn add_const_left(dst: Register, src: Register, constant: ConstantId) -> Self {
        Self {
            opcode: Opcode::AddConst,
            flags: ADD_CONST_LEFT_FLAG,
            a: dst,
            b: src,
            c: constant,
        }
    }

    pub fn add_const_is_left(self) -> bool {
        self.opcode == Opcode::AddConst && self.flags & ADD_CONST_LEFT_FLAG != 0
    }
    pub const fn inc_i(dst: Register, src: Register, decrement: bool) -> Self {
        Self {
            opcode: Opcode::IncI,
            flags: decrement as u8,
            a: dst,
            b: src,
            c: 0,
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
    pub const fn slow_at(index: u32) -> Self {
        Self {
            opcode: Opcode::Slow,
            flags: 0,
            a: index as u16,
            b: (index >> 16) as u16,
            c: 0,
        }
    }

    pub const fn cold_index(self) -> Option<u32> {
        if matches!(self.opcode, Opcode::Slow) {
            Some(self.a as u32 | (self.b as u32) << 16)
        } else {
            None
        }
    }

    pub fn register_flow(self) -> RegisterFlow {
        use Opcode::*;
        match self.opcode {
            LoadConst => RegisterFlow { uses: [None; 3], definition: Some(self.a), complete: true },
            Move | LoadLocal | LoadLocalChecked => RegisterFlow::unary(self.a, self.b),
            Add | Sub | Mul | Div | Binary => RegisterFlow::binary(self.a, self.b, self.c),
            AddConst | Unary | IncI => RegisterFlow::unary(self.a, self.b),
            JumpIfFalse | Return => RegisterFlow::store(self.a),
            Call | CallN => RegisterFlow {
                uses: [Some(self.b), (self.flags != 0).then_some(self.c), None],
                definition: Some(self.a),
                complete: true,
            },
            AGetI | AGetIQuickened | AGetIInc | GetProperty | GetPropertyQuickened => {
                RegisterFlow::binary(self.a, self.b, self.c)
            }
            ASetI => RegisterFlow {
                uses: [Some(self.a), Some(self.b), Some(self.c)],
                definition: None,
                complete: true,
            },
            GetN | GetNQuickened if self.flags & GETN_GLOBAL_FLAG != 0 => RegisterFlow {
                uses: [None; 3],
                definition: Some(self.a),
                complete: true,
            },
            GetN | GetNQuickened => RegisterFlow::unary(self.a, self.b),
            SetN => RegisterFlow {
                uses: [Some(self.a), Some(self.b), None],
                definition: None,
                complete: true,
            },
            UpdateLocal => RegisterFlow {
                uses: [Some(self.a), Some(self.b), None],
                definition: None,
                complete: true,
            },
            InitLocal | StoreLocal | StoreLocalChecked => RegisterFlow::store(self.b),
            Jump | Slow => RegisterFlow::none(),
            ForI => RegisterFlow { uses: [None; 3], definition: None, complete: false },
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
    pub const fn jump(target: u16) -> Self {
        Self {
            opcode: Opcode::Jump,
            flags: 0,
            a: target,
            b: 0,
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
    pub const fn update_local(
        old: Register,
        updated: Register,
        slot: u16,
        decrement: bool,
    ) -> Self {
        Self {
            opcode: Opcode::UpdateLocal,
            flags: decrement as u8,
            a: old,
            b: updated,
            c: slot,
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
    pub const fn get_named(dst: Register, object: Register, length: bool) -> Self {
        Self {
            opcode: Opcode::GetN,
            flags: if length { GETN_LENGTH_FLAG } else { 0 },
            a: dst,
            b: object,
            c: 0,
        }
    }
    pub const fn get_global_named(dst: Register) -> Self {
        Self {
            opcode: Opcode::GetN,
            flags: GETN_GLOBAL_FLAG,
            a: dst,
            b: 0,
            c: 0,
        }
    }
    pub const fn set_named(object: Register, src: Register, strict: bool) -> Self {
        Self {
            opcode: Opcode::SetN,
            flags: strict as u8,
            a: object,
            b: src,
            c: 0,
        }
    }
    pub const fn array_set(object: Register, key: Register, src: Register, strict: bool) -> Self {
        Self {
            opcode: Opcode::ASetI,
            flags: strict as u8,
            a: object,
            b: key,
            c: src,
        }
    }
    pub const fn array_get_index_inc(dst: Register, object: Register, index: Register) -> Self {
        Self {
            opcode: Opcode::AGetIInc,
            flags: 0,
            a: dst,
            b: object,
            c: index,
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
    pub const fn call_one_arg(dst: Register, callee: Register, argument: Register) -> Self {
        Self {
            opcode: Opcode::Call,
            flags: 1,
            a: dst,
            b: callee,
            c: argument,
        }
    }
    pub const fn call_named(dst: Register, object: Register, argument: Option<Register>) -> Self {
        let (flags, argument) = match argument {
            Some(argument) => (1, argument),
            None => (0, MAX_REGISTER_ID),
        };
        Self {
            opcode: Opcode::CallN,
            flags,
            a: dst,
            b: object,
            c: argument,
        }
    }
    pub const fn call_registered_one(dst: Register, object: Register, callee: Register) -> Self {
        Self::call_registered_window(dst, object, callee, 1)
    }
    pub const fn call_registered_window(
        dst: Register,
        object: Register,
        callee: Register,
        argc: u8,
    ) -> Self {
        Self {
            opcode: Opcode::CallN,
            flags: argc,
            a: dst,
            b: object,
            c: callee,
        }
    }
}

fn is_consecutive_argument_window(dst: Register, args: &[Register]) -> bool {
    let Ok(argc) = u16::try_from(args.len()) else {
        return false;
    };
    let Some(first) = dst.checked_sub(argc) else {
        return false;
    };
    args.iter().copied().eq(first..dst)
}
impl Instruction {
    /// Encode the canonical instruction into its deterministic compact wire form.
    ///
    /// The first two bytes are the opcode and flags, followed by exactly the
    /// operand words required by that opcode in little-endian order.  This is
    /// an interchange/measurement format; execution retains the fixed-width
    /// [`Instruction`] record and therefore the slow path remains authoritative.
    pub fn encode_compact(self) -> Vec<u8> {
        let width = usize::from(self.opcode.operand_width());
        let mut bytes = Vec::with_capacity(2 + width * 2);
        bytes.push(self.opcode as u8);
        bytes.push(self.flags);
        for operand in [self.a, self.b, self.c].into_iter().take(width) {
            bytes.extend_from_slice(&operand.to_le_bytes());
        }
        bytes
    }

    /// Decode one compact instruction, rejecting unknown opcodes and truncated
    /// or overlong records rather than silently accepting an invalid state.
    pub fn decode_compact(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 2 {
            return Err("compact instruction missing opcode and flags");
        }
        let opcode = Opcode::from_u8(bytes[0]).ok_or("unknown compact opcode")?;
        if bytes.len() != opcode.compact_len() {
            return Err("compact instruction has invalid width");
        }
        let operands = opcode.decode_operands(&bytes[2..])?;
        Ok(Self {
            opcode,
            flags: bytes[1],
            a: operands[0],
            b: operands[1],
            c: operands[2],
        })
    }
}

/// Result of lowering one canonical operation.
///
/// `Fast` owns only the fixed-width instruction used by the compact executor.
/// `Slow` retains the original operation as the semantic authority. Unsupported
/// operations are therefore never silently discarded.
#[derive(Debug, Clone, PartialEq)]
pub enum LoweredInstruction {
    Fast(Instruction),
    Slow(crate::ops::Op),
}

/// Classify an operation without introducing a second semantic representation.
pub fn lower(op: &crate::ops::Op) -> LoweredInstruction {
    lower_compact(op)
        .map(LoweredInstruction::Fast)
        .unwrap_or_else(|| LoweredInstruction::Slow(op.clone()))
}

/// Lossless lowering for the fixed-width subset of the canonical Op IR.
pub fn lower_compact(op: &crate::ops::Op) -> Option<Instruction> {
    use crate::ops::{BinaryOp, Op};
    match op {
        Op::Move { dst, src } => Some(Instruction::move_(*dst, *src)),
        Op::LoadLocal { dst, slot } => Some(Instruction::load_local(*dst, *slot)),
        Op::StoreLocal { slot, src } => Some(Instruction::store_local(*slot, *src)),
        Op::Unary { dst, operator, src } => {
            Some(Instruction::unary_operator(*dst, *operator, *src))
        }
        Op::LoadBinding {
            dst,
            slot,
            dynamic: false,
            ..
        } => Some(Instruction::load_local_checked(*dst, *slot)),
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
        Op::Binary {
            dst,
            operator,
            lhs,
            rhs,
        } => Some(Instruction::binary_operator(*dst, *operator, *lhs, *rhs)),
        Op::GetPropertyDynamic { dst, object, key } => {
            Some(Instruction::binary(Opcode::AGetI, *dst, *object, *key))
        }
        Op::GetProperty { dst, object, key } => {
            Some(Instruction::get_named(*dst, *object, key == "length"))
        }
        Op::ResolveName { dst, key } if crate::globals::builtin(key).is_some() => {
            Some(Instruction::get_global_named(*dst))
        }
        Op::SetProperty {
            object,
            src,
            strict,
            ..
        } => Some(Instruction::set_named(*object, *src, *strict)),
        Op::SetPropertyDynamic {
            object,
            key,
            src,
            strict,
        } => Some(Instruction::array_set(*object, *key, *src, *strict)),
        Op::Call {
            dst,
            callee,
            receiver: None,
            args,
            spreads,
        } if spreads.iter().all(|spread| !spread) && args.len() <= 1 => match args.as_slice() {
            [] => Some(Instruction::call_zero_args(*dst, *callee)),
            [argument] => Some(Instruction::call_one_arg(*dst, *callee, *argument)),
            _ => None,
        },
        Op::Return { src } => Some(Instruction::ret(*src)),
        Op::CallMethod {
            dst,
            object,
            callee: Some(callee),
            args,
            spreads,
            ..
        } if !args.is_empty()
            && args.len() <= u8::MAX as usize
            && spreads.iter().all(|spread| !spread)
            && is_consecutive_argument_window(*dst, args) =>
        {
            Some(Instruction::call_registered_window(
                *dst,
                *object,
                *callee,
                args.len() as u8,
            ))
        }
        _ => None,
    }
}

/// Out-of-line metadata indexed by the canonical instruction position.
///
/// Each populated vector is either empty (metadata omitted) or exactly as long
/// as `Program::instructions`. `Program` owns the vectors and keeps them in
/// lockstep when instructions are fused; an entry is never interpreted when
/// its vector is empty. This makes missing metadata an explicit, valid state
/// rather than a sentinel embedded in hot instruction records.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RareMetadata {
    pub source_spans: Vec<(u32, u32)>,
    pub names: Vec<String>,
    pub debug_flags: Vec<u8>,
}

impl RareMetadata {
    fn is_aligned(&self, instruction_count: usize) -> bool {
        [
            self.source_spans.len(),
            self.names.len(),
            self.debug_flags.len(),
        ]
        .into_iter()
        .all(|len| len == 0 || len == instruction_count)
    }

    fn retain_fused(&mut self, keep: &[bool]) {
        for values in [
            MetadataVector::Spans(&mut self.source_spans),
            MetadataVector::Names(&mut self.names),
            MetadataVector::Flags(&mut self.debug_flags),
        ] {
            values.retain(keep);
        }
    }
}

enum MetadataVector<'a> {
    Spans(&'a mut Vec<(u32, u32)>),
    Names(&'a mut Vec<String>),
    Flags(&'a mut Vec<u8>),
}

impl MetadataVector<'_> {
    fn retain(self, keep: &[bool]) {
        match self {
            Self::Spans(values) => values.retain_with_index(keep),
            Self::Names(values) => values.retain_with_index(keep),
            Self::Flags(values) => values.retain_with_index(keep),
        }
    }
}

trait RetainWithIndex {
    fn retain_with_index(&mut self, keep: &[bool]);
}

impl<T> RetainWithIndex for Vec<T> {
    fn retain_with_index(&mut self, keep: &[bool]) {
        if self.is_empty() {
            return;
        }
        assert_eq!(self.len(), keep.len());
        let mut index = 0;
        self.retain(|_| {
            let retained = keep[index];
            index += 1;
            retained
        });
    }
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
        let instruction = Opcode::LoadConst
            .builder()
            .operands(dst, id, 0)
            .build()
            .expect("generated LoadConst operation must remain representable");
        self.instructions.push(instruction);
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
        let mut keep = Vec::with_capacity(self.instructions.len());
        let mut i = 0;
        while i < self.instructions.len() {
            if i + 1 < self.instructions.len()
                && self.instructions[i].opcode == Opcode::LoadConst
                && self.instructions[i + 1].opcode == Opcode::Add
                && self.instructions[i].flags == 0
                && self.instructions[i + 1].flags == 0
                && (self.instructions[i].a == self.instructions[i + 1].b
                    || self.instructions[i].a == self.instructions[i + 1].c)
            {
                let load = self.instructions[i];
                let add = self.instructions[i + 1];
                let fused = if load.a == add.b {
                    Instruction::add_const_left(add.a, add.c, load.b)
                } else {
                    Instruction::add_const(add.a, add.b, load.b)
                };
                out.push(fused);
                keep.push(true);
                keep.push(false);
                i += 2;
            } else {
                out.push(self.instructions[i]);
                keep.push(true);
                i += 1;
            }
        }
        self.rare.retain_fused(&keep);
        self.instructions = out;
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.rare.is_aligned(self.instructions.len()) {
            return Err("rare metadata is not aligned with instructions");
        }
        for instruction in &self.instructions {
            if !instruction.opcode.operands_are_canonical([
                instruction.a,
                instruction.b,
                instruction.c,
            ]) {
                return Err("instruction has non-canonical unused operands");
            }
            match instruction.opcode {
                Opcode::LoadConst if self.constants.get(instruction.b).is_none() => {
                    return Err("instruction references missing constant");
                }
                Opcode::AddConst if self.constants.get(instruction.c).is_none() => {
                    return Err("instruction references missing constant");
                }
                Opcode::JumpIfFalse if usize::from(instruction.b) >= self.instructions.len() => {
                    return Err("conditional jump target is out of range");
                }
                Opcode::Jump if usize::from(instruction.a) >= self.instructions.len() => {
                    return Err("jump target is out of range");
                }
                _ => {}
            }
        }
        Ok(())
    }
}
impl Program {
    /// Execute the validated fixed-width subset with caller-owned registers.
    /// This is a test-only wire-format helper. Production execution uses the
    /// catalog-backed VM handler table; keeping this out of release builds
    /// avoids a second semantic interpreter.
    #[cfg(test)]
    pub fn execute(
        &self,
        registers: &mut crate::register_file::RegisterFile,
    ) -> Result<crate::value::Value, crate::vm::VmError> {
        self.validate()
            .map_err(|message| crate::vm::VmError::EvalError(message.into()))?;
        let mut pc = 0usize;
        while let Some(instruction) = self.instructions.get(pc).copied() {
            let read = |id: Register| {
                registers
                    .get(usize::from(id))
                    .ok_or(crate::vm::VmError::RegisterOutOfBounds(id))
            };
            match instruction.opcode {
                Opcode::LoadConst => {
                    let value = self
                        .constants
                        .get(instruction.b)
                        .cloned()
                        .ok_or(crate::vm::VmError::EvalError("missing constant".into()))?;
                    let dst = usize::from(instruction.a);
                    registers.resize(registers.len().max(dst + 1), crate::value::Value::Undefined);
                    registers.write(dst, (&value).into());
                }
                Opcode::Move => {
                    let value = read(instruction.b)?;
                    let dst = usize::from(instruction.a);
                    registers.resize(registers.len().max(dst + 1), crate::value::Value::Undefined);
                    registers.write(dst, value);
                }
                Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::AddConst => {
                    let source = read(instruction.b)?;
                    let (left, right) = if instruction.opcode == Opcode::AddConst {
                        let constant: crate::value::Value = (&self
                            .constants
                            .get(instruction.c)
                            .cloned()
                            .ok_or(crate::vm::VmError::EvalError("missing constant".into()))?)
                            .into();
                        if instruction.add_const_is_left() {
                            (constant, source)
                        } else {
                            (source, constant)
                        }
                    } else {
                        (source, read(instruction.c)?)
                    };
                    let (crate::value::Value::Number(lhs), crate::value::Value::Number(rhs)) =
                        (left, right)
                    else {
                        return Err(crate::vm::VmError::EvalError(
                            "compact arithmetic requires numbers".into(),
                        ));
                    };
                    let result = match instruction.opcode {
                        Opcode::Add | Opcode::AddConst => lhs + rhs,
                        Opcode::Sub => lhs - rhs,
                        Opcode::Mul => lhs * rhs,
                        Opcode::Div => lhs / rhs,
                        _ => unreachable!(),
                    };
                    let dst = usize::from(instruction.a);
                    registers.resize(registers.len().max(dst + 1), crate::value::Value::Undefined);
                    registers.write(dst, crate::value::Value::Number(result));
                }
                Opcode::Return => return read(instruction.a),
                _ => {
                    return Err(crate::vm::VmError::EvalError(
                        "unsupported compact instruction".into(),
                    ));
                }
            }
            pc += 1;
        }
        Err(crate::vm::VmError::MissingReturn)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn executes_canonical_numeric_stream_and_reports_missing_return() {
        let mut program = Program::default();
        program.load_constant(0, Constant::Number(2.0));
        program.load_constant(1, Constant::Number(3.0));
        program.instructions.push(Instruction::add(2, 0, 1));
        program.instructions.push(Instruction::ret(2));
        let mut registers = crate::register_file::RegisterFile::new();
        assert_eq!(
            program.execute(&mut registers),
            Ok(crate::value::Value::Number(5.0))
        );
        program.instructions.pop();
        assert_eq!(
            program.execute(&mut registers),
            Err(crate::vm::VmError::MissingReturn)
        );
    }
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
    fn validates_add_const_pool_operand_in_canonical_source() {
        let mut program = Program::default();
        program.load_constant(0, Constant::Number(1.0));
        program.instructions.push(Instruction::add_const(1, 0, 7));
        assert_eq!(
            program.validate(),
            Err("instruction references missing constant")
        );
    }

    #[test]
    fn opcodes_remain_compact_byte_identifiers() {
        assert_eq!(Opcode::COUNT, Opcode::Unary as u8);
        assert!(Opcode::AGetIQuickened.is_compact());
        assert!(Opcode::Slow.is_compact());
    }

    #[test]
    fn generated_operation_facts_are_the_opcode_source_of_truth() {
        assert_eq!(OPERATION_SPECS.len(), usize::from(Opcode::COUNT));
        let get_property = Opcode::GetProperty.spec();
        assert_eq!(get_property.opcode, Opcode::GetProperty as u8);
        assert_eq!(
            get_property.operand_width,
            Opcode::GetProperty.operand_width()
        );
        assert_eq!(get_property.fallback, "get_property");
        assert_eq!(Opcode::GetProperty.fallback(), "get_property");
        assert_eq!(
            Opcode::GetProperty.result_shape(),
            crate::facts::ResultShape::Value
        );
        assert_eq!(Opcode::Jump.control_flow(), crate::facts::ControlFlow::Jump);
        assert_eq!(Opcode::ForI.control_flow(), crate::facts::ControlFlow::Loop);
        assert_eq!(Opcode::ForI.fallback(), "for_integer");
        assert_eq!(
            Opcode::Jump.control_operands(Instruction::jump(9)),
            ControlOperands::Jump { target: 9 }
        );
        assert_eq!(
            Opcode::JumpIfFalse.control_operands(Instruction::jump_if_false(2, 11)),
            ControlOperands::Branch {
                condition: 2,
                target: 11,
            }
        );
        assert_eq!(
            Opcode::Return.control_operands(Instruction::ret(4)),
            ControlOperands::Return { source: 4 }
        );
        assert!(Opcode::GetProperty.has_guard(crate::facts::OperationGuard::Shape));
        assert!(!Opcode::Move.has_guard(crate::facts::OperationGuard::Shape));
        assert!(get_property
            .effects
            .contains(&crate::facts::OperationEffect::MayThrow));
        assert!(get_property.is_observable());
        assert!(Opcode::GetProperty.has_effect(crate::facts::OperationEffect::ReadHeap));
        assert!(!Opcode::Move.spec().is_observable());
        assert!(Opcode::Jump.spec().is_control());
        assert!(Opcode::GetProperty.is_quickenable());
        assert!(!Opcode::Move.is_quickenable());
        assert!(!Opcode::Jump.is_quickenable());
        assert_eq!(Opcode::Add.handler_name(), "run_arithmetic");
        assert_eq!(
            Opcode::GetProperty.handler_name(),
            "run_compact_get_property"
        );
        assert_eq!(
            Opcode::Add.numeric_operator(),
            Some(crate::ops::BinaryOp::Add)
        );
        assert_eq!(
            Opcode::AddConst.numeric_operator(),
            Some(crate::ops::BinaryOp::Add)
        );
        assert_eq!(Opcode::Move.numeric_operator(), None);

        let built = Opcode::Add
            .builder()
            .flags(0)
            .operands(3, 1, 2)
            .build()
            .expect("catalog width admits three operands");
        assert_eq!(built, Instruction::add(3, 1, 2));
    }

    #[test]
    fn generated_builder_rejects_noncanonical_unused_operands() {
        assert_eq!(
            Opcode::Return.builder().operands(7, 1, 0).build(),
            Err("unused operand must be zero")
        );
    }

    #[test]
    fn register_flow_excludes_constants_and_tracks_cfg_operands() {
        let arithmetic = Instruction::add(4, 1, 2).register_flow();
        assert_eq!(arithmetic.definition, Some(4));
        assert_eq!(arithmetic.uses, [Some(1), Some(2), None]);
        let constant = Instruction::add_const(4, 1, 9).register_flow();
        assert_eq!(constant.uses, [Some(1), None, None]);
        let branch = Instruction::jump_if_false(4, 8).register_flow();
        assert_eq!(branch.uses, [Some(4), None, None]);
        assert!(!Instruction {
            opcode: Opcode::ForI,
            flags: 0,
            a: 0,
            b: 1,
            c: 2,
        }
        .register_flow()
        .complete);
    }

    #[test]
    fn opcodes_have_checked_compact_byte_decoding() {
        assert_eq!(std::mem::size_of::<Opcode>(), 1);
        for value in 1..=Opcode::COUNT {
            let opcode = Opcode::from_u8(value).expect("assigned opcode must decode");
            assert_eq!(opcode as u8, value);
        }
        assert_eq!(Opcode::from_u8(0), None);
        assert_eq!(Opcode::from_u8(Opcode::COUNT + 1), None);
    }

    #[test]
    fn compact_binary_fact_table_round_trips_every_operator() {
        use crate::ops::BinaryOp::*;
        let operators = [
            Add,
            Subtract,
            Multiply,
            Divide,
            Remainder,
            Exponentiate,
            NumericAdd,
            NumericSubtract,
            Equal,
            NotEqual,
            StrictEqual,
            StrictNotEqual,
            LessThan,
            LessEqual,
            GreaterThan,
            GreaterEqual,
            BitwiseOr,
            BitwiseXor,
            BitwiseAnd,
            ShiftLeft,
            ShiftRight,
            ShiftRightZeroFill,
            Instanceof,
        ];
        for operator in operators {
            let id = compact_binary_id(operator);
            assert_eq!(compact_binary_operator(id), Some(operator));
        }
        assert_eq!(compact_binary_operator(operators.len() as u8), None);
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
            lower_compact(&Op::SetPropertyDynamic {
                object: 4,
                key: 5,
                src: 6,
                strict: true,
            }),
            Some(Instruction::array_set(4, 5, 6, true))
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
        assert_eq!(
            lower_compact(&Op::Call {
                dst: 0,
                callee: 4,
                receiver: None,
                args: vec![1],
                spreads: vec![false]
            }),
            Some(Instruction::call_one_arg(0, 4, 1))
        );
    }
    #[test]
    fn lowers_two_argument_method_window_without_operand_storage() {
        let op = crate::ops::Op::CallMethod {
            dst: 5,
            object: 1,
            key: "method".into(),
            callee: Some(2),
            args: vec![3, 4],
            spreads: vec![false, false],
        };
        assert_eq!(
            lower_compact(&op),
            Some(Instruction::call_registered_window(5, 1, 2, 2))
        );
    }
    #[test]
    fn lowers_six_argument_method_window_without_operand_storage() {
        let op = crate::ops::Op::CallMethod {
            dst: 10,
            object: 1,
            key: "method".into(),
            callee: Some(2),
            args: vec![4, 5, 6, 7, 8, 9],
            spreads: vec![false; 6],
        };
        assert_eq!(
            lower_compact(&op),
            Some(Instruction::call_registered_window(10, 1, 2, 6))
        );
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
    fn proven_builtin_name_uses_compact_global_get() {
        let instruction = lower_compact(&crate::ops::Op::ResolveName {
            dst: 7,
            key: "Math".to_string(),
        })
        .expect("known global builtin is compact");
        assert_eq!(instruction.opcode, Opcode::GetN);
        assert_eq!(instruction.flags, GETN_GLOBAL_FLAG);
        assert_eq!((instruction.a, instruction.b, instruction.c), (7, 0, 0));

        assert!(lower_compact(&crate::ops::Op::ResolveName {
            dst: 7,
            key: "userBinding".to_string(),
        })
        .is_none());
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
        assert_eq!(p.instructions, vec![Instruction::add_const_left(2, 1, 0)]);
    }

    #[test]
    fn fusion_preserves_flagged_instructions_for_slow_path() {
        let mut p = Program::default();
        p.load_constant(0, Constant::Number(2.0));
        p.instructions.push(Instruction {
            opcode: Opcode::Add,
            flags: 1,
            a: 2,
            b: 0,
            c: 1,
        });
        p.fuse_load_const_add();
        assert_eq!(p.instructions.len(), 2);
        assert_eq!(p.instructions[0].opcode, Opcode::LoadConst);
        assert_eq!(p.instructions[1].flags, 1);
        assert_eq!(p.validate(), Ok(()));
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
    fn validation_rejects_unreachable_jump_targets() {
        let mut p = Program::default();
        p.instructions.push(Instruction::jump(2));
        assert_eq!(p.validate(), Err("jump target is out of range"));

        p.instructions.push(Instruction::ret(0));
        p.instructions.push(Instruction::ret(0));
        assert_eq!(p.validate(), Ok(()));
    }

    #[test]
    fn validation_rejects_noncanonical_unused_operands() {
        let mut p = Program::default();
        p.instructions.push(Instruction {
            opcode: Opcode::Return,
            flags: 0,
            a: 0,
            b: 1,
            c: 0,
        });
        assert_eq!(
            p.validate(),
            Err("instruction has non-canonical unused operands")
        );
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
    fn opcode_metrics_use_the_complete_dispatch_domain() {
        let instructions = [
            Instruction::load_const(0, 0),
            Instruction::add(1, 0, 0),
            Instruction::slow(0),
        ];
        let metrics = OpcodeMetrics::for_instructions(&instructions);
        assert_eq!(metrics.frequency.len(), Opcode::COUNT as usize + 1);
        assert_eq!(metrics.frequency[Opcode::LoadConst as usize], 1);
        assert_eq!(metrics.frequency[Opcode::Add as usize], 1);
        assert_eq!(metrics.frequency[Opcode::Slow as usize], 1);
        assert_eq!(metrics.operand_words[Opcode::LoadConst as usize], 2);
        assert_eq!(metrics.operand_words[Opcode::Add as usize], 3);
        assert_eq!(metrics.operand_words[Opcode::Slow as usize], 1);
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
    #[test]
    fn rare_metadata_stays_aligned_when_fusing_instructions() {
        let mut p = Program::default();
        p.load_constant(0, Constant::Number(2.0));
        p.instructions.push(Instruction::add(1, 0, 0));
        p.instructions.push(Instruction::ret(1));
        p.rare.source_spans = vec![(1, 2), (3, 4), (5, 6)];
        p.rare.names = vec!["load".into(), "add".into(), "return".into()];
        p.rare.debug_flags = vec![1, 2, 3];
        p.fuse_load_const_add();
        assert_eq!(p.rare.source_spans, vec![(1, 2), (5, 6)]);
        assert_eq!(p.rare.names, vec!["load".to_string(), "return".to_string()]);
        assert_eq!(p.rare.debug_flags, vec![1, 3]);
        assert_eq!(p.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_partial_rare_metadata() {
        let mut p = Program::default();
        p.instructions.push(Instruction::ret(0));
        p.instructions.push(Instruction::ret(0));
        p.rare.debug_flags.push(1);
        assert_eq!(
            p.validate(),
            Err("rare metadata is not aligned with instructions")
        );
    }
    #[test]
    fn compact_encoding_round_trips_operands_flags_and_unused_zeroes() {
        let instructions = [
            Instruction::load_const(0x1234, 0xabcd),
            Instruction::get_property(7, 8, 9),
            Instruction::slow(0x5a),
        ];
        for instruction in instructions {
            let encoded = instruction.encode_compact();
            assert_eq!(Instruction::decode_compact(&encoded), Ok(instruction));
            assert_eq!(encoded[0], instruction.opcode as u8);
            assert_eq!(encoded[1], instruction.flags);
        }
        let decoded_return =
            Instruction::decode_compact(&Instruction::ret(1).encode_compact()).unwrap();
        assert_eq!(
            (decoded_return.a, decoded_return.b, decoded_return.c),
            (1, 0, 0)
        );
    }

    #[test]
    fn compact_encoding_rejects_invalid_opcode_and_boundaries() {
        assert_eq!(
            Instruction::decode_compact(&[]),
            Err("compact instruction missing opcode and flags")
        );
        assert_eq!(
            Instruction::decode_compact(&[0, 0]),
            Err("unknown compact opcode")
        );
        let valid = Instruction::ret(3).encode_compact();
        assert_eq!(
            Instruction::decode_compact(&valid[..valid.len() - 1]),
            Err("compact instruction has invalid width")
        );
        let mut overlong = valid;
        overlong.push(0);
        assert_eq!(
            Instruction::decode_compact(&overlong),
            Err("compact instruction has invalid width")
        );
    }

    #[test]
    fn lowering_classifies_fast_and_retains_slow_source() {
        use crate::ops::Op;

        let fast = lower(&Op::Move { dst: 1, src: 2 });
        assert_eq!(fast, LoweredInstruction::Fast(Instruction::move_(1, 2)));

        let source = Op::Const {
            dst: 0,
            value: Constant::Number(3.0),
        };
        let slow = lower(&source);
        assert_eq!(slow, LoweredInstruction::Slow(source.clone()));
        assert!(matches!(slow, LoweredInstruction::Slow(op) if op == source));
    }
}
