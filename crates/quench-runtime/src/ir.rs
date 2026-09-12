//! Compact executable IR produced by lowering.
//!
//! Instructions contain only integer operands. Constants and uncommon source
//! information live in pools owned by `Program`, so dispatch never walks AST.

use crate::facts::{
    ControlFlow, OperationEffect, OperationGuard, OperationSpec, ResultShape, WordKind,
};
// Keep compact IR operands tied to the native execute word while semantic
// values remain owned by `crate::value`.
const _: () = assert!(crate::native_core::WORD_BYTES == std::mem::size_of::<u64>());
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
    Throw { source: Register },
    Loop { a: u16, b: u16, c: u16 },
}

/// Generated payload families for the generic value/control Bridge. The
/// opcode declaration owns this physical spelling; runtime admission only
/// validates the instance payload and whether the selected artifact exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GenericBridgePayload {
    Plain,
    InitLocal,
    Move,
    AddConst,
    Increment,
    Binary,
    Unary,
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
        Self {
            uses: [None; 3],
            definition: None,
            complete: true,
        }
    }

    pub const fn unary(definition: Register, source: Register) -> Self {
        Self {
            uses: [Some(source), None, None],
            definition: Some(definition),
            complete: true,
        }
    }

    pub const fn define(definition: Register) -> Self {
        Self {
            uses: [None; 3],
            definition: Some(definition),
            complete: true,
        }
    }

    pub const fn binary(definition: Register, left: Register, right: Register) -> Self {
        Self {
            uses: [Some(left), Some(right), None],
            definition: Some(definition),
            complete: true,
        }
    }

    pub const fn store(source: Register) -> Self {
        Self {
            uses: [Some(source), None, None],
            definition: None,
            complete: true,
        }
    }

    /// Highest VM register referenced by this canonical operand view.
    /// Structured residuals keep `complete == false`; their physical handler
    /// remains conservative and may use compact operand words directly.
    pub const fn highest_register(self) -> Option<Register> {
        let mut highest = self.definition;
        let mut index = 0;
        while index < self.uses.len() {
            if let Some(register) = self.uses[index] {
                highest = match highest {
                    Some(current) if current >= register => Some(current),
                    _ => Some(register),
                };
            }
            index += 1;
        }
        highest
    }
}

macro_rules! vm_op {
    ($($name:ident = $id:literal / $width:literal => [$($effect:ident),*] / $fallback:ident / $result:ident / $control:ident / [$($guard:ident),*] / $handler:ident $(/ $operator:ident)? $( @ $marker:ident)*),+ $(,)?) => {
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

            pub fn from_name(name: &str) -> Option<Self> {
                match name { $(stringify!($name) => Some(Self::$name),)+ _ => None }
            }

            /// Decode a textual operation name to a typed cold-row spelling.
            /// This is retained for diagnostics and legacy textual data; the
            /// production lowering path uses generated `Op::cold_opcode`.
            pub fn from_operation_name(name: &str) -> Option<Self> {
                let opcode = match name {
                    "Call" => Some(Self::CallSlow),
                    name => Self::from_name(name),
                }?;
                opcode.is_typed_cold_marker().then_some(opcode)
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

            /// Validate an instruction's operand payload, including the one
            /// physical Move spelling that uses all three words to describe
            /// a proven local copy.  The ordinary opcode shape remains the
            /// two-register Move contract; flags select this internal form.
            pub const fn operands_are_canonical_with_flags(
                self,
                flags: u8,
                operands: [u16; 3],
            ) -> bool {
                match self {
                    Self::Move if flags == 1 => true,
                    _ => self.operands_are_canonical(operands),
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

            /// Find the dedicated opcode for a binary operator from the same
            /// generated catalog. Generic `Binary` remains the fallback when
            /// no dedicated row is declared.
            #[inline(always)]
            pub const fn binary_opcode(operator: crate::ops::BinaryOp) -> Option<Self> {
                Self::BINARY_OPCODE_BY_ID[operator.compact_id() as usize]
            }

            const fn binary_opcode_table() -> [Option<Self>; crate::ops::BinaryOp::COUNT as usize + 1] {
                let mut table = [None; crate::ops::BinaryOp::COUNT as usize + 1];
                $(vm_op!(@binary_assign table, $name $(/ $operator)?);)+
                table
            }

            const BINARY_OPCODE_BY_ID:
                [Option<Self>; crate::ops::BinaryOp::COUNT as usize + 1] =
                Self::binary_opcode_table();

            /// Decode the binary operator represented by a physical opcode.
            /// Dedicated rows and the generic flagged row share this one
            /// catalog-derived view; consumers must not repeat the mapping.
            pub const fn binary_operator(self, flags: u8) -> Option<crate::ops::BinaryOp> {
                match self {
                    Self::Binary => compact_binary_operator(flags),
                    Self::AddConst => None,
                    _ => self.numeric_operator(),
                }
            }

            /// Whether this opcode is a physical spelling of the generic
            /// binary operation family. Dedicated arithmetic rows that have
            /// their own ordinary dispatch arm stay out of this view; all
            /// other catalog-derived binary rows can share Binary consumers.
            pub const fn is_binary_family(self) -> bool {
                matches!(self, Self::Binary) || self.numeric_operator().is_some()
            }

            /// Whether this opcode stores an out-of-line canonical operation
            /// for the shared fallback handler. This is the explicit `@ cold`
            /// fact in the canonical declaration; adding a cold row cannot
            /// require a second hand-maintained opcode list.
            pub const fn is_cold_marker(self) -> bool {
                match self {
                    $(Self::$name => vm_op!(@marker $($marker)*)),+
                }
            }

            /// Whether the canonical declaration exposes this opcode to the
            /// generic value/control bridge. This is a declaration marker,
            /// not a second selector table: operand/flag and physical-artifact
            /// checks remain instance-specific at the admission boundary.
            pub const fn is_generic_bridge_candidate(self) -> bool {
                self.spec().generic_bridge
            }

            /// Classify the instance payload consumed by the generic Bridge.
            /// This is derived beside the canonical opcode declaration so
            /// machine admission does not maintain a second opcode-family
            /// table. Dedicated numeric rows share the binary artifact path.
            pub const fn generic_bridge_payload(self) -> GenericBridgePayload {
                match self {
                    $(Self::$name => vm_op!(@payload Self::$name, $($marker)*)),+
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
                generic_bridge: vm_op!(@bridge $($marker)*),
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
    (@binary_assign $table:ident, AddConst / $operator:ident) => {};
    (@binary_assign $table:ident, $name:ident / $operator:ident) => {
        $table[crate::ops::BinaryOp::$operator as usize] = Some(Self::$name);
    };
    (@binary_assign $table:ident, $name:ident) => {};
    (@marker cold $($rest:ident)*) => { true };
    (@marker $head:ident $($rest:ident)*) => { vm_op!(@marker $($rest)*) };
    (@marker) => { false };
    (@bridge bridge $($rest:ident)*) => { true };
    (@bridge $head:ident $($rest:ident)*) => { vm_op!(@bridge $($rest)*) };
    (@bridge) => { false };
    (@payload $opcode:expr, bridge_init_local $($rest:ident)*) => {
        GenericBridgePayload::InitLocal
    };
    (@payload $opcode:expr, bridge_move $($rest:ident)*) => {
        GenericBridgePayload::Move
    };
    (@payload $opcode:expr, bridge_add_const $($rest:ident)*) => {
        GenericBridgePayload::AddConst
    };
    (@payload $opcode:expr, bridge_increment $($rest:ident)*) => {
        GenericBridgePayload::Increment
    };
    (@payload $opcode:expr, bridge_binary $($rest:ident)*) => {
        GenericBridgePayload::Binary
    };
    (@payload $opcode:expr, bridge_unary $($rest:ident)*) => {
        GenericBridgePayload::Unary
    };
    (@payload $opcode:expr, $head:ident $($rest:ident)*) => {
        vm_op!(@payload $opcode, $($rest)*)
    };
    (@payload $opcode:expr,) => {
        if $opcode.numeric_operator().is_some() {
            GenericBridgePayload::Binary
        } else {
            GenericBridgePayload::Plain
        }
    };
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
    (@control Throw, $instruction:ident) => {
        ControlOperands::Throw { source: $instruction.a }
    };
    (@control Loop, $instruction:ident) => {
        ControlOperands::Loop { a: $instruction.a, b: $instruction.b, c: $instruction.c }
    };
}

vm_op! {
    LoadConst = 1 / 2 => [Pure] / load_const / Value / Next / [] / run_load_const @ bridge,
    Move = 2 / 2 => [Pure] / move / Value / Next / [] / run_move @ bridge @ bridge_move,
    Add = 3 / 3 => [MayThrow] / add / Value / Next / [Number] / run_arithmetic / Add @ bridge,
    AddConst = 4 / 3 => [MayThrow] / add_const / Value / Next / [Number] / run_compact_add_const / Add @ bridge @ bridge_add_const,
    JumpIfFalse = 5 / 2 => [MayThrow, Control] / jump_if_false / None / Branch / [] / run_instruction_fallback @ bridge,
    Return = 6 / 1 => [Control] / return_value / Value / Return / [] / run_return @ bridge,
    Slow = 7 / 1 => [MayThrow, Observable] / slow / Value / Next / [] / run_instruction_fallback @ cold,
    LoadLocal = 8 / 2 => [Pure] / load_local / Value / Next / [] / run_local @ bridge,
    Sub = 9 / 3 => [MayThrow] / subtract / Value / Next / [Number] / run_arithmetic / Subtract @ bridge,
    Mul = 10 / 3 => [MayThrow] / multiply / Value / Next / [Number] / run_arithmetic / Multiply @ bridge,
    Div = 11 / 3 => [MayThrow] / divide / Value / Next / [Number] / run_arithmetic / Divide @ bridge,
    GetProperty = 12 / 3 => [ReadHeap, MayThrow, Observable] / get_property / Value / Next / [Shape] / run_compact_get_property,
    Call = 13 / 3 => [ReadHeap, MayThrow, Observable] / call / Value / Next / [Callable] / run_compact_call,
    Jump = 14 / 1 => [Control] / jump / None / Jump / [] / run_instruction_fallback @ bridge,
    IncI = 15 / 2 => [MayThrow] / increment_integer / Value / Next / [] / run_compact_numeric_update @ bridge @ bridge_increment,
    ForI = 16 / 3 => [Control] / for_integer / None / Loop / [] / run_instruction_fallback @ cold,
    AGetI = 17 / 3 => [ReadHeap, MayThrow, Observable] / get_element / Value / Next / [Shape] / run_compact_get_index,
    ASetI = 18 / 3 => [WriteHeap, MayThrow, Observable] / set_element / None / Next / [Shape] / run_compact_set_index,
    AGetIInc = 19 / 3 => [ReadHeap, WriteHeap, MayThrow, Observable] / get_element_increment / Value / Next / [Shape] / run_compact_get_index_inc,
    GetN = 20 / 3 => [ReadHeap, MayThrow, Observable] / get_named / Value / Next / [Shape] / run_compact_get_named,
    SetN = 21 / 3 => [WriteHeap, MayThrow, Observable] / set_named / None / Next / [Shape] / run_compact_set_named,
    CallN = 22 / 3 => [ReadHeap, MayThrow, Observable] / call_named / Value / Next / [Shape, Callable] / run_compact_call_named,
    UpdateLocal = 23 / 3 => [Pure] / update_local / Value / Next / [] / run_update_local @ bridge,
    LoadLocalChecked = 24 / 2 => [MayThrow] / load_local_checked / Value / Next / [] / run_load_local_checked @ bridge,
    Binary = 25 / 3 => [MayThrow] / binary / Value / Next / [] / run_binary_instruction @ bridge @ bridge_binary,
    StoreLocalChecked = 26 / 2 => [MayThrow] / store_local_checked / None / Next / [] / run_store_local_checked @ bridge,
    InitLocal = 27 / 2 => [Pure] / init_local / None / Next / [] / run_init_local @ bridge @ bridge_init_local,
    StoreLocal = 28 / 2 => [Pure] / store_local / None / Next / [] / run_store_local @ bridge,
    GetPropertyQuickened = 29 / 3 => [ReadHeap, MayThrow, Observable] / get_property / Value / Next / [] / run_compact_get_property,
    GetNQuickened = 30 / 3 => [ReadHeap, MayThrow, Observable] / get_named / Value / Next / [] / run_compact_get_named,
    AGetIQuickened = 31 / 3 => [ReadHeap, MayThrow, Observable] / get_element / Value / Next / [] / run_compact_get_index,
    Unary = 32 / 3 => [MayThrow] / unary / Value / Next / [] / run_unary_instruction @ bridge @ bridge_unary,
    Remainder = 33 / 3 => [MayThrow] / remainder / Value / Next / [] / run_binary_instruction / Remainder @ bridge,
    Exponentiate = 34 / 3 => [MayThrow] / exponentiate / Value / Next / [] / run_binary_instruction / Exponentiate @ bridge,
    MarkUninitialized = 35 / 3 => [Pure] / mark_uninitialized / None / Next / [] / run_mark_uninitialized @ cold @ bridge,
    MarkImmutable = 36 / 3 => [Pure] / mark_immutable / None / Next / [] / run_mark_immutable @ cold @ bridge,
    RequireObjectCoercible = 37 / 3 => [MayThrow] / require_object_coercible / None / Next / [] / run_require_object_coercible @ cold @ bridge,
    NumericAdd = 38 / 3 => [MayThrow] / numeric_add / Value / Next / [Number] / run_binary_instruction / NumericAdd @ bridge,
    NumericSubtract = 39 / 3 => [MayThrow] / numeric_subtract / Value / Next / [Number] / run_binary_instruction / NumericSubtract @ bridge,
    Equal = 40 / 3 => [MayThrow] / equal / Value / Next / [] / run_binary_instruction / Equal @ bridge,
    NotEqual = 41 / 3 => [MayThrow] / not_equal / Value / Next / [] / run_binary_instruction / NotEqual @ bridge,
    StrictEqual = 42 / 3 => [Pure] / strict_equal / Value / Next / [] / run_binary_instruction / StrictEqual @ bridge,
    StrictNotEqual = 43 / 3 => [Pure] / strict_not_equal / Value / Next / [] / run_binary_instruction / StrictNotEqual @ bridge,
    LessThan = 44 / 3 => [MayThrow] / less_than / Value / Next / [] / run_binary_instruction / LessThan @ bridge,
    LessEqual = 45 / 3 => [MayThrow] / less_equal / Value / Next / [] / run_binary_instruction / LessEqual @ bridge,
    GreaterThan = 46 / 3 => [MayThrow] / greater_than / Value / Next / [] / run_binary_instruction / GreaterThan @ bridge,
    GreaterEqual = 47 / 3 => [MayThrow] / greater_equal / Value / Next / [] / run_binary_instruction / GreaterEqual @ bridge,
    BitwiseOr = 48 / 3 => [MayThrow] / bitwise_or / Value / Next / [] / run_binary_instruction / BitwiseOr @ bridge,
    BitwiseXor = 49 / 3 => [MayThrow] / bitwise_xor / Value / Next / [] / run_binary_instruction / BitwiseXor @ bridge,
    BitwiseAnd = 50 / 3 => [MayThrow] / bitwise_and / Value / Next / [] / run_binary_instruction / BitwiseAnd @ bridge,
    ShiftLeft = 51 / 3 => [MayThrow] / shift_left / Value / Next / [] / run_binary_instruction / ShiftLeft @ bridge,
    ShiftRight = 52 / 3 => [MayThrow] / shift_right / Value / Next / [] / run_binary_instruction / ShiftRight @ bridge,
    ShiftRightZeroFill = 53 / 3 => [MayThrow] / shift_right_zero_fill / Value / Next / [] / run_binary_instruction / ShiftRightZeroFill @ bridge,
    Instanceof = 54 / 3 => [MayThrow] / instanceof / Value / Next / [] / run_binary_instruction / Instanceof @ bridge,
    Loop = 55 / 1 => [MayThrow] / loop / None / Next / [] / run_instruction_fallback @ cold,
    TailCall = 56 / 1 => [MayThrow] / tail_call / Value / Next / [] / run_instruction_fallback @ cold,
    MakeArray = 57 / 1 => [Allocate, MayThrow] / make_array / Value / Next / [] / run_instruction_fallback @ cold,
    MakeFunctionWithKind = 58 / 1 => [Allocate, MayThrow] / make_function / Value / Next / [] / run_instruction_fallback @ cold,
    SetFunctionName = 59 / 1 => [MayThrow] / set_function_name / None / Next / [] / run_instruction_fallback @ cold,
    MakeObject = 60 / 1 => [Allocate, MayThrow] / make_object / Value / Next / [] / run_instruction_fallback @ cold,
    Construct = 61 / 1 => [Allocate, MayThrow] / construct / Value / Next / [] / run_instruction_fallback @ cold,
    ForOf = 62 / 1 => [MayThrow] / for_of / None / Next / [] / run_instruction_fallback @ cold,
    CallSlow = 63 / 1 => [MayThrow, Observable] / call_slow / Value / Next / [] / run_instruction_fallback @ cold,
    Try = 64 / 1 => [MayThrow] / try / None / Next / [] / run_instruction_fallback @ cold,
    Await = 65 / 1 => [MayThrow] / await / Value / Next / [] / run_instruction_fallback @ cold,
    MakeBuiltin = 66 / 1 => [Allocate, MayThrow] / make_builtin / Value / Next / [] / run_instruction_fallback @ cold,
    ValidateClassHeritage = 67 / 1 => [MayThrow] / validate_class_heritage / None / Next / [] / run_instruction_fallback @ cold,
    GetClassPrototype = 68 / 1 => [MayThrow] / get_class_prototype / Value / Next / [] / run_instruction_fallback @ cold,
    MakeFunction = 69 / 1 => [Allocate, MayThrow] / make_function / Value / Next / [] / run_instruction_fallback @ cold,
    StaticBlock = 70 / 1 => [MayThrow] / static_block / None / Next / [] / run_instruction_fallback @ cold,
    AppendInstanceField = 71 / 1 => [MayThrow] / append_instance_field / None / Next / [] / run_instruction_fallback @ cold,
    PrivateScope = 72 / 1 => [MayThrow] / private_scope / None / Next / [] / run_instruction_fallback @ cold,
    LoadParameter = 73 / 2 => [Pure] / load_parameter / Value / Next / [] / run_load_parameter @ bridge,
    InitializeLocal = 74 / 1 => [Pure] / initialize_local / None / Next / [] / run_initialize_local @ bridge,
    CheckInitialized = 75 / 1 => [MayThrow] / check_initialized / None / Next / [] / run_check_initialized @ bridge,
    Throw = 76 / 1 => [MayThrow, Control] / throw_value / None / Throw / [] / run_throw @ bridge,
}

/// Compatibility names used by compact instruction consumers. The canonical
/// operator declaration owns both directions; IR only exposes the typed view.
#[inline(always)]
pub const fn compact_binary_id(operator: crate::ops::BinaryOp) -> u8 {
    operator.compact_id()
}

#[inline(always)]
pub const fn compact_binary_operator(id: u8) -> Option<crate::ops::BinaryOp> {
    crate::ops::BinaryOp::from_compact_id(id)
}

/// Compatibility names used by compact instruction consumers. The canonical
/// unary operator declaration owns both directions.
#[inline(always)]
pub const fn compact_unary_id(operator: crate::ops::UnaryOp) -> u8 {
    operator.compact_id()
}

#[inline(always)]
pub const fn compact_unary_operator(id: u8) -> Option<crate::ops::UnaryOp> {
    crate::ops::UnaryOp::from_compact_id(id)
}

impl Opcode {
    pub const fn is_compact(self) -> bool {
        (self as u8) <= Self::COUNT
    }
    /// Whether dispatch enters a cold semantic handler, including typed cold
    /// rows and the legacy generic gateway.
    pub const fn is_slow(self) -> bool {
        self.is_cold_marker()
    }

    /// Cold operations with a dedicated typed spelling in the compact stream.
    /// The generic `Slow` gateway remains available for operations that do not
    /// have a declared opcode row (for example future host extensions).
    pub const fn is_typed_cold_marker(self) -> bool {
        self.is_cold_marker() && !matches!(self, Self::Slow)
    }

    /// Return the semantic opcode represented by a quickened physical alias.
    /// Quickening changes only the guarded execution view; JSON/trace
    /// contracts and fallback reasoning must continue to observe the same
    /// canonical operation.
    #[inline(always)]
    pub const fn semantic_opcode(self) -> Self {
        match self {
            Self::GetPropertyQuickened => Self::GetProperty,
            Self::GetNQuickened => Self::GetN,
            Self::AGetIQuickened => Self::AGetI,
            // `ForI` is the typed physical spelling of the canonical
            // structured `Loop` operation. Keep execution-profile IR and
            // semantic diagnostics on the operation name while the payload
            // travels through the typed cold marker.
            Self::ForI => Self::Loop,
            _ => self,
        }
    }

    /// Return the guarded physical alias for a quickenable semantic opcode.
    /// Keeping both directions together prevents selectors and diagnostics
    /// from carrying separate quickening maps.
    #[inline(always)]
    pub const fn quickened_opcode(self) -> Option<Self> {
        match self {
            Self::GetProperty => Some(Self::GetPropertyQuickened),
            Self::GetN => Some(Self::GetNQuickened),
            Self::AGetI => Some(Self::AGetIQuickened),
            _ => None,
        }
    }

    pub(crate) fn matches_physical_contract(self, actual: Self) -> bool {
        self == actual
            || self == actual.semantic_opcode()
            || (self == Self::Binary
                && actual != Self::AddConst
                && actual.numeric_operator().is_some())
            || (self == Self::Slow && actual.is_typed_cold_marker())
    }

    /// Validate operand words after accepting a semantic alias.  Cold
    /// markers carry their out-of-line operation index in the typed payload,
    /// so the generic `Slow` declaration cannot apply its one-word shape to
    /// those physical spellings.
    pub(crate) fn operands_match_physical_contract(self, actual: Self, operands: [u16; 3]) -> bool {
        self.matches_physical_contract(actual)
            && ((self == Self::Slow && actual.is_typed_cold_marker())
                || (self == actual && actual.is_typed_cold_marker())
                || (self == actual.semantic_opcode() && actual.is_typed_cold_marker())
                || actual.operands_are_canonical(operands))
    }

    pub(crate) fn operands_match_physical_contract_with_flags(
        self,
        actual: Self,
        flags: u8,
        operands: [u16; 3],
    ) -> bool {
        self.matches_physical_contract(actual)
            && ((self == Self::Slow && actual.is_typed_cold_marker())
                || (self == actual && actual.is_typed_cold_marker())
                || (self == actual.semantic_opcode() && actual.is_typed_cold_marker())
                || actual.operands_are_canonical_with_flags(flags, operands))
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
        if !self
            .opcode
            .operands_are_canonical_with_flags(self.flags, self.operands)
        {
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
    /// Build the generic flagged binary gateway for legacy/structured selector
    /// inputs. Canonical `Op::Binary` lowering uses dedicated generated rows
    /// through [`Opcode::binary_opcode`] instead.
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

    pub const fn unary_operator(
        dst: Register,
        operator: crate::ops::UnaryOp,
        src: Register,
    ) -> Self {
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
    pub const fn load_parameter(dst: Register, slot: u16) -> Self {
        Self {
            opcode: Opcode::LoadParameter,
            flags: 0,
            a: dst,
            b: slot,
            c: 0,
        }
    }
    pub const fn initialize_local(slot: u16) -> Self {
        Self {
            opcode: Opcode::InitializeLocal,
            flags: 0,
            a: slot,
            b: 0,
            c: 0,
        }
    }
    pub const fn check_initialized(slot: u16) -> Self {
        Self {
            opcode: Opcode::CheckInitialized,
            flags: 0,
            a: slot,
            b: 0,
            c: 0,
        }
    }
    pub const fn throw_(src: Register) -> Self {
        Self {
            opcode: Opcode::Throw,
            flags: 0,
            a: src,
            b: 0,
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

    pub const fn cold_marker(opcode: Opcode, slot: Register, flags: u8, index: u32) -> Self {
        Self {
            opcode,
            flags,
            a: slot,
            b: index as u16,
            c: (index >> 16) as u16,
        }
    }

    pub const fn cold_index(self) -> Option<u32> {
        if !self.opcode.is_cold_marker() {
            return None;
        }
        if matches!(self.opcode, Opcode::Slow) {
            Some(self.a as u32 | (self.b as u32) << 16)
        } else {
            Some(self.b as u32 | (self.c as u32) << 16)
        }
    }

    /// Whether this compact opcode carries an out-of-line canonical `Op`.
    /// Typed cold markers and the legacy `Slow` gateway share the same lookup
    /// contract; the opcode only chooses the mechanical dispatch family.
    pub fn is_cold_marker(self) -> bool {
        self.cold_index().is_some()
    }

    pub fn register_flow(self) -> RegisterFlow {
        use Opcode::*;
        match self.opcode {
            LoadConst => RegisterFlow {
                uses: [None; 3],
                definition: Some(self.a),
                complete: true,
            },
            Move if self.flags == 0 => RegisterFlow::unary(self.a, self.b),
            Move | LoadLocal | LoadLocalChecked | LoadParameter => RegisterFlow::define(self.a),
            Add | Sub | Mul | Div | Binary | Remainder | Exponentiate | NumericAdd
            | NumericSubtract | Equal | NotEqual | StrictEqual | StrictNotEqual | LessThan
            | LessEqual | GreaterThan | GreaterEqual | BitwiseOr | BitwiseXor | BitwiseAnd
            | ShiftLeft | ShiftRight | ShiftRightZeroFill | Instanceof => {
                RegisterFlow::binary(self.a, self.b, self.c)
            }
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
            InitializeLocal | CheckInitialized => RegisterFlow::none(),
            Throw => RegisterFlow::store(self.a),
            Jump | MarkUninitialized | MarkImmutable => RegisterFlow::none(),
            RequireObjectCoercible => RegisterFlow::store(self.a),
            Loop | TailCall | MakeArray | MakeFunctionWithKind | SetFunctionName | MakeObject
            | Construct | ForOf | CallSlow | Try | Await | MakeBuiltin => RegisterFlow::none(),
            ValidateClassHeritage | GetClassPrototype => RegisterFlow::none(),
            MakeFunction | StaticBlock | AppendInstanceField => RegisterFlow::none(),
            PrivateScope => RegisterFlow::none(),
            Slow => RegisterFlow {
                uses: [None; 3],
                definition: None,
                complete: false,
            },
            ForI => RegisterFlow {
                uses: [None; 3],
                definition: None,
                complete: false,
            },
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
    pub const fn call_registered_arguments(dst: Register, callee: Register, argc: u8) -> Self {
        Self {
            opcode: Opcode::Call,
            flags: argc,
            a: dst,
            b: callee,
            c: 0,
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

/// The physical boundary selected for one canonical operation instance.
///
/// This is a diagnostic/selection view, not a second semantic operation set:
/// `Compact` reaches the fixed-width stream (possibly after the encoder adds
/// range-owned data such as a constant-pool ID), `TypedCold` carries the
/// catalog-declared cold spelling, and `GenericSlow` preserves the original
/// operation in the out-of-line semantic store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoweringBoundary {
    Compact,
    TypedCold(Opcode),
    GenericSlow,
}

/// Classify the one operation boundary without discarding its canonical value.
pub fn lowering_boundary(op: &crate::ops::Op) -> LoweringBoundary {
    op.lowering_boundary()
}

/// Report whether an operation reaches the fixed-width stream after the
/// encoder supplies any range-owned data it needs. `Const` is the one such
/// operation whose pool ID cannot be known by this module alone; the
/// `CodeArena` encoder materializes it as `LoadConst` from the canonical pool.
/// Keeping that exception here makes the diagnostic boundary agree with the
/// production encoder without adding a second operation representation.
pub fn has_compact_boundary(op: &crate::ops::Op) -> bool {
    matches!(op, crate::ops::Op::Const { .. }) || lower_compact(op).is_some()
}

/// Return the payload words owned by a typed cold marker. The cold-store index
/// occupies the remaining words; keeping this small physical view beside the
/// lowering boundary gives `CodeArena` one source for marker payload layout.
#[inline(always)]
pub fn cold_marker_payload(op: &crate::ops::Op) -> (Register, u8) {
    use crate::ops::Op;
    match op {
        Op::MarkUninitialized { slot, shared } => (*slot, u8::from(*shared)),
        Op::MarkImmutable { slot } => (*slot, 0),
        Op::RequireObjectCoercible { src } => (*src, 0),
        _ => (0, 0),
    }
}

/// Classify an operation without introducing a second semantic representation.
pub fn lower(op: &crate::ops::Op) -> LoweredInstruction {
    lower_compact(op)
        .map(LoweredInstruction::Fast)
        .unwrap_or_else(|| LoweredInstruction::Slow(op.clone()))
}

/// Lossless lowering for the fixed-width subset of the canonical Op IR.
pub fn lower_compact(op: &crate::ops::Op) -> Option<Instruction> {
    use crate::ops::Op;
    match op {
        Op::Move { dst, src } => Some(Instruction::move_(*dst, *src)),
        Op::LoadLocal { dst, slot } => Some(Instruction::load_local(*dst, *slot)),
        Op::LoadParameter { dst, slot } => Some(Instruction::load_parameter(*dst, *slot)),
        Op::InitializeLocal { slot } => Some(Instruction::initialize_local(*slot)),
        Op::CheckInitialized { slot, .. } => Some(Instruction::check_initialized(*slot)),
        Op::Throw { src } => Some(Instruction::throw_(*src)),
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
            operator,
            lhs,
            rhs,
        } => Opcode::binary_opcode(*operator)
            .map(|opcode| Instruction::binary(opcode, *dst, *lhs, *rhs)),
        Op::GetPropertyDynamic { dst, object, key } => {
            Some(Instruction::binary(Opcode::AGetI, *dst, *object, *key))
        }
        Op::GetProperty { dst, object, key } => {
            Some(Instruction::get_named(*dst, *object, key == "length"))
        }
        // ResolveName is scope-sensitive: a `with` object (or direct eval)
        // may shadow a global builtin.  It therefore must stay on the
        // complete semantic path; lowering it to GetGlobalNamed would erase
        // the dynamic environment and return the wrong function identity.
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
        _ => op.generated_lowering_fallback(),
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
            if !instruction.opcode.operands_are_canonical_with_flags(
                instruction.flags,
                [instruction.a, instruction.b, instruction.c],
            ) {
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
        assert_eq!(Opcode::COUNT, Opcode::Throw as u8);
        assert!(Opcode::AGetIQuickened.is_compact());
        assert!(Opcode::Slow.is_compact());
    }

    #[test]
    fn opcode_names_round_trip_through_generated_catalog() {
        for &opcode in Opcode::ALL {
            assert_eq!(Opcode::from_name(opcode.name()), Some(opcode));
        }
        assert_eq!(Opcode::from_name("not_an_opcode"), None);
    }

    #[test]
    fn binary_opcode_lookup_uses_declared_dedicated_rows() {
        use crate::ops::BinaryOp;
        assert_eq!(Opcode::binary_opcode(BinaryOp::Add), Some(Opcode::Add));
        assert_eq!(
            Opcode::binary_opcode(BinaryOp::Remainder),
            Some(Opcode::Remainder)
        );
        assert_eq!(
            Opcode::binary_opcode(BinaryOp::Exponentiate),
            Some(Opcode::Exponentiate)
        );
        assert_eq!(Opcode::binary_opcode(BinaryOp::Equal), Some(Opcode::Equal));

        let mut seen = [false; Opcode::COUNT as usize + 1];
        let mut missing = Vec::new();
        for operator in BinaryOp::ALL {
            let Some(opcode) = Opcode::binary_opcode(*operator) else {
                missing.push(*operator);
                continue;
            };
            let index = opcode as usize;
            assert!(!seen[index], "duplicate dedicated opcode for {operator:?}");
            seen[index] = true;
            if opcode != Opcode::AddConst {
                assert_eq!(opcode.numeric_operator(), Some(*operator));
            }
        }
        // Every declared operator currently has a dedicated semantic opcode.
        // The generic `Binary` gateway remains for legacy/unknown decoding,
        // but a newly added catalog operator must gain a row or make this
        // audit fail rather than silently widening that gateway.
        assert!(
            missing.is_empty(),
            "operators without dedicated rows: {missing:?}"
        );
    }

    #[test]
    fn physical_binary_operator_view_uses_one_catalog_mapping() {
        use crate::ops::BinaryOp;
        assert_eq!(Opcode::Add.binary_operator(0), Some(BinaryOp::Add));
        assert_eq!(
            Opcode::Binary.binary_operator(compact_binary_id(BinaryOp::LessThan)),
            Some(BinaryOp::LessThan)
        );
        assert_eq!(Opcode::AddConst.binary_operator(0), None);
        assert_eq!(Opcode::Return.binary_operator(0), None);
        assert!(Opcode::Binary.is_binary_family());
        for operator in BinaryOp::ALL {
            let opcode = Opcode::binary_opcode(*operator).expect("dedicated row");
            assert!(opcode.is_binary_family());
        }
        assert!(!Opcode::Return.is_binary_family());
    }

    #[test]
    fn quickening_aliases_round_trip_through_one_opcode_mapping() {
        for (semantic, quickened) in [
            (Opcode::GetProperty, Opcode::GetPropertyQuickened),
            (Opcode::GetN, Opcode::GetNQuickened),
            (Opcode::AGetI, Opcode::AGetIQuickened),
        ] {
            assert_eq!(semantic.quickened_opcode(), Some(quickened));
            assert_eq!(quickened.semantic_opcode(), semantic);
            assert_eq!(semantic.semantic_opcode(), semantic);
        }
        assert_eq!(Opcode::Return.quickened_opcode(), None);
        assert_eq!(Opcode::Return.semantic_opcode(), Opcode::Return);
        assert_eq!(Opcode::ForI.semantic_opcode(), Opcode::Loop);
    }

    #[test]
    fn binary_catalog_keeps_physical_hints_with_operator_identity() {
        use crate::ops::BinaryOp;
        assert_eq!(BinaryOp::Equal.region_name(), Some("compare_equal"));
        assert_eq!(BinaryOp::StrictEqual.region_name(), Some("compare_equal"));
        assert_eq!(BinaryOp::BitwiseAnd.region_name(), Some("bitwise_and"));
        assert_eq!(BinaryOp::NumericAdd.region_name(), Some("increment"));
        assert_eq!(BinaryOp::NumericSubtract.region_name(), Some("decrement"));
        assert_eq!(BinaryOp::Instanceof.region_name(), None);
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
        assert!(!Opcode::GetProperty.spec().generic_bridge);
        assert!(Opcode::Add.spec().generic_bridge);
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
        assert_eq!(
            Opcode::Throw.control_flow(),
            crate::facts::ControlFlow::Throw
        );
        assert_eq!(
            Opcode::Throw.control_operands(Instruction::throw_(4)),
            ControlOperands::Throw { source: 4 }
        );
        assert!(Opcode::GetProperty.has_guard(crate::facts::OperationGuard::Shape));
        assert!(!Opcode::Move.has_guard(crate::facts::OperationGuard::Shape));
        assert!(get_property
            .effects
            .contains(&crate::facts::OperationEffect::MayThrow));
        assert!(get_property.is_observable());
        assert!(Opcode::GetProperty.has_effect(crate::facts::OperationEffect::ReadHeap));
        for opcode in [
            Opcode::MakeArray,
            Opcode::MakeFunction,
            Opcode::MakeFunctionWithKind,
            Opcode::MakeObject,
            Opcode::MakeBuiltin,
            Opcode::Construct,
        ] {
            assert!(
                opcode.has_effect(crate::facts::OperationEffect::Allocate),
                "allocating opcode lost its allocation effect: {opcode:?}"
            );
        }
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
    fn generated_bridge_markers_cannot_cross_heap_or_control_loop_boundaries() {
        for opcode in Opcode::ALL.iter().copied() {
            if !opcode.is_generic_bridge_candidate() {
                continue;
            }
            for effect in [
                crate::facts::OperationEffect::ReadHeap,
                crate::facts::OperationEffect::WriteHeap,
                crate::facts::OperationEffect::Allocate,
                crate::facts::OperationEffect::Observable,
            ] {
                assert!(
                    !opcode.has_effect(effect),
                    "bridge marker on {opcode:?} crosses {effect:?}"
                );
            }
            assert_ne!(
                opcode.control_flow(),
                crate::facts::ControlFlow::Loop,
                "bridge marker on structured loop {opcode:?}"
            );
        }
    }

    #[test]
    fn generated_bridge_payload_families_cover_physical_aliases() {
        assert_eq!(
            Opcode::InitLocal.generic_bridge_payload(),
            GenericBridgePayload::InitLocal
        );
        assert_eq!(
            Opcode::Move.generic_bridge_payload(),
            GenericBridgePayload::Move
        );
        assert_eq!(
            Opcode::AddConst.generic_bridge_payload(),
            GenericBridgePayload::AddConst
        );
        assert_eq!(
            Opcode::IncI.generic_bridge_payload(),
            GenericBridgePayload::Increment
        );
        for opcode in [
            Opcode::Binary,
            Opcode::Add,
            Opcode::NumericAdd,
            Opcode::Remainder,
            Opcode::Instanceof,
        ] {
            assert_eq!(
                opcode.generic_bridge_payload(),
                GenericBridgePayload::Binary
            );
        }
        assert_eq!(
            Opcode::Unary.generic_bridge_payload(),
            GenericBridgePayload::Unary
        );
        assert_eq!(
            Opcode::Return.generic_bridge_payload(),
            GenericBridgePayload::Plain
        );
    }

    #[test]
    fn generated_builder_rejects_noncanonical_unused_operands() {
        assert_eq!(
            Opcode::Return.builder().operands(7, 1, 0).build(),
            Err("unused operand must be zero")
        );
    }

    #[test]
    fn generated_builder_accepts_only_the_proven_local_move_spelling() {
        assert_eq!(
            Opcode::Move.builder().flags(1).operands(2, 17, 19).build(),
            Ok(Instruction::move_local(2, 17, 19))
        );
        assert_eq!(
            Opcode::Move.builder().flags(2).operands(2, 17, 19).build(),
            Err("unused operand must be zero")
        );
    }

    #[test]
    fn generated_op_variant_view_is_exhaustive_and_unique() {
        assert!(!crate::ops::Op::VARIANT_NAMES.is_empty());
        let mut names = std::collections::BTreeSet::new();
        for name in crate::ops::Op::VARIANT_NAMES {
            assert!(
                names.insert(*name),
                "duplicate canonical Op variant: {name}"
            );
        }
        assert_eq!(names.len(), crate::ops::Op::VARIANT_NAMES.len());
    }

    #[test]
    fn generated_op_lowering_matrix_matches_variant_view_and_catalog() {
        let rows = crate::ops::Op::LOWERING_MATRIX;
        assert_eq!(rows.len(), crate::ops::Op::VARIANT_NAMES.len());
        for (row, name) in rows.iter().zip(crate::ops::Op::VARIANT_NAMES) {
            assert_eq!(row.name, *name);
            if let Some(opcode) = row.physical_opcode {
                let spec = row
                    .operation_spec()
                    .expect("physical matrix row must borrow catalog facts");
                assert_eq!(spec.opcode, opcode as u8);
                assert_eq!(spec.name, opcode.name());
            }
            if let Some(opcode) = row.typed_cold_opcode {
                assert!(
                    row.physical_opcode.is_some(),
                    "typed cold row for {name} must have a physical family"
                );
                assert!(
                    opcode.is_cold_marker(),
                    "canonical {name} maps to non-cold opcode {opcode:?}"
                );
                assert!(
                    crate::ir::Opcode::ALL.contains(&opcode),
                    "canonical {name} maps outside the opcode catalog"
                );
            }
        }
    }

    #[test]
    fn generated_physical_opcode_view_preserves_declared_aliases() {
        use crate::machine::FunctionCode;
        use crate::ops::{Constant, Op};

        assert_eq!(
            Op::Const {
                dst: 0,
                value: Constant::Number(1.0),
            }
            .physical_opcode(),
            Some(Opcode::LoadConst)
        );
        assert_eq!(
            Op::Call {
                dst: 0,
                callee: 1,
                receiver: None,
                args: Vec::new(),
                spreads: Vec::new(),
            }
            .physical_opcode(),
            Some(Opcode::Call)
        );
        assert_eq!(
            Op::Call {
                dst: 0,
                callee: 1,
                receiver: None,
                args: Vec::new(),
                spreads: Vec::new(),
            }
            .cold_opcode(),
            Some(Opcode::CallSlow)
        );
        let empty = || FunctionCode::from_ops(Vec::new());
        assert_eq!(
            Op::Loop {
                label: None,
                init: empty(),
                test: empty(),
                body: empty(),
                update: empty(),
                post_test: false,
                dst: 0,
                per_iteration: Vec::new(),
            }
            .physical_opcode(),
            Some(Opcode::ForI)
        );

        let aliases = [
            (
                Op::LoadBinding {
                    dst: 0,
                    slot: 1,
                    name: "x".into(),
                    dynamic: false,
                },
                Opcode::LoadLocalChecked,
            ),
            (
                Op::GetProperty {
                    dst: 0,
                    object: 1,
                    key: "value".into(),
                },
                Opcode::GetN,
            ),
            (
                Op::GetPropertyDynamic {
                    dst: 0,
                    object: 1,
                    key: 2,
                },
                Opcode::AGetI,
            ),
            (
                Op::SetProperty {
                    object: 0,
                    key: "value".into(),
                    src: 1,
                    strict: false,
                },
                Opcode::SetN,
            ),
            (
                Op::SetPropertyDynamic {
                    object: 0,
                    key: 1,
                    src: 2,
                    strict: false,
                },
                Opcode::ASetI,
            ),
        ];
        for (operation, expected) in aliases {
            assert_eq!(operation.physical_opcode(), Some(expected));
            assert_eq!(
                lower_compact(&operation).map(|instruction| instruction.opcode),
                Some(expected)
            );
        }
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
        let local = Instruction::load_local(3, u16::MAX).register_flow();
        assert_eq!(local.definition, Some(3));
        assert_eq!(local.uses, [None; 3]);
        let local_move = Instruction::move_local(5, u16::MAX, u16::MAX - 1).register_flow();
        assert_eq!(local_move.definition, Some(5));
        assert_eq!(local_move.uses, [None; 3]);
        assert_eq!(
            Instruction::move_(5, 4).register_flow().uses,
            [Some(4), None, None]
        );
        assert!(
            !Instruction {
                opcode: Opcode::ForI,
                flags: 0,
                a: 0,
                b: 1,
                c: 2,
            }
            .register_flow()
            .complete
        );
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
        for operator in crate::ops::BinaryOp::ALL {
            let id = compact_binary_id(*operator);
            assert_eq!(compact_binary_operator(id), Some(*operator));
        }
        assert_eq!(
            compact_binary_operator(crate::ops::BinaryOp::COUNT + 1),
            None
        );
    }

    #[test]
    fn compact_unary_fact_table_round_trips_every_operator() {
        for operator in crate::ops::UnaryOp::ALL {
            let id = compact_unary_id(*operator);
            assert_eq!(compact_unary_operator(id), Some(*operator));
        }
        assert_eq!(compact_unary_operator(crate::ops::UnaryOp::COUNT + 1), None);
    }
    #[test]
    fn lowers_common_ops_to_fixed_width_instructions() {
        use crate::ops::{BinaryOp, Op};
        assert_eq!(
            lower_compact(&Op::Move { dst: 1, src: 2 }),
            Some(Instruction::move_(1, 2))
        );
        assert_eq!(
            lower_compact(&Op::LoadParameter { dst: 3, slot: 4 }),
            Some(Instruction::load_parameter(3, 4))
        );
        assert_eq!(
            lower_compact(&Op::InitializeLocal { slot: 5 }),
            Some(Instruction::initialize_local(5))
        );
        assert_eq!(
            lower_compact(&Op::CheckInitialized {
                slot: 6,
                name: "binding".into(),
            }),
            Some(Instruction::check_initialized(6))
        );
        assert_eq!(
            lower_compact(&Op::Throw { src: 7 }),
            Some(Instruction::throw_(7))
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
        for (operator, opcode) in [
            (BinaryOp::Subtract, Opcode::Sub),
            (BinaryOp::Multiply, Opcode::Mul),
            (BinaryOp::Divide, Opcode::Div),
        ] {
            assert_eq!(
                lower_compact(&Op::Binary {
                    dst: 3,
                    operator,
                    lhs: 1,
                    rhs: 2,
                }),
                Some(Instruction::binary(opcode, 3, 1, 2)),
                "{operator:?} must use its dedicated arithmetic opcode"
            );
        }
        assert_eq!(
            lower_compact(&Op::Binary {
                dst: 3,
                operator: BinaryOp::Remainder,
                lhs: 1,
                rhs: 2,
            }),
            Some(Instruction::binary(Opcode::Remainder, 3, 1, 2)),
            "remainder uses its dedicated canonical opcode"
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
    fn lowering_boundary_names_compact_typed_and_generic_paths() {
        use crate::ops::{BinaryOp, Op};
        let move_op = Op::Move { dst: 1, src: 2 };
        assert_eq!(lowering_boundary(&move_op), LoweringBoundary::Compact);
        assert_eq!(move_op.lowering_boundary(), LoweringBoundary::Compact);
        assert_eq!(move_op.generic_fallback_name(), None);
        assert_eq!(
            lowering_boundary(&Op::Const {
                dst: 1,
                value: Constant::Number(7.0),
            }),
            LoweringBoundary::Compact,
            "range-owned constant IDs are materialized as LoadConst by CodeArena"
        );
        assert_eq!(
            Op::ResolveName {
                dst: 1,
                key: "userDefinedBinding".into(),
            }
            .generated_lowering_fallback(),
            None,
            "generated residual arm must preserve generic fallback ownership"
        );
        assert_eq!(
            lowering_boundary(&Op::MarkImmutable { slot: 1 }),
            LoweringBoundary::TypedCold(Opcode::MarkImmutable)
        );
        assert_eq!(
            lowering_boundary(&Op::Loop {
                label: None,
                init: crate::machine::FunctionCode::from_ops(Vec::new()),
                test: crate::machine::FunctionCode::from_ops(Vec::new()),
                body: crate::machine::FunctionCode::from_ops(Vec::new()),
                update: crate::machine::FunctionCode::from_ops(Vec::new()),
                post_test: false,
                dst: 0,
                per_iteration: Vec::new(),
            }),
            LoweringBoundary::TypedCold(Opcode::ForI)
        );
        assert_eq!(
            lowering_boundary(&Op::Binary {
                dst: 1,
                operator: BinaryOp::Instanceof,
                lhs: 2,
                rhs: 3,
            }),
            LoweringBoundary::Compact
        );
        let generic = Op::ResolveName {
            dst: 1,
            key: "userDefinedBinding".into(),
        };
        assert_eq!(lowering_boundary(&generic), LoweringBoundary::GenericSlow);
        assert_eq!(generic.generic_fallback_name(), Some("ResolveName"));
    }

    #[test]
    fn generic_fallback_witnesses_preserve_residual_family_identity() {
        use crate::machine::FunctionCode;
        use crate::ops::Op;

        // These representatives cover structured control, call, suspension,
        // host and mutation families. Their out-of-line boundary must retain
        // the original canonical operation instead of collapsing into an
        // anonymous `Slow` case. Single-register throws have a compact row and
        // are tested separately.
        let residuals = vec![
            Op::ParameterEnd,
            Op::OptionalCall {
                dst: 0,
                callee: 1,
                receiver: None,
                guard_receiver: false,
                args: vec![],
                spreads: vec![],
            },
            Op::Branch {
                condition: 0,
                then_ops: FunctionCode::from_ops(vec![]),
                else_ops: FunctionCode::from_ops(vec![]),
            },
            Op::Yield { src: 1 },
            Op::MakeRest {
                slot: 0,
                arguments: 1,
                skip: 0,
            },
            Op::SetPrototype {
                object: 0,
                prototype: 1,
            },
            Op::Eval {
                dst: 0,
                callee: 1,
                source: 2,
                strict: false,
                global: false,
                direct: true,
                tail: false,
                bindings: vec![],
                reusable_var_names: vec![],
                forbidden_var_names: vec![],
            },
            Op::ForIn {
                label: None,
                object: 0,
                slot: 1,
                body: FunctionCode::from_ops(vec![]),
                per_iteration: false,
                iteration_slots: vec![],
                dst: 2,
            },
            Op::DynamicImport {
                dst: 0,
                specifier: 1,
                options: None,
                deferred: false,
            },
        ];

        for op in residuals {
            let name = op.variant_name();
            assert_eq!(
                lowering_boundary(&op),
                LoweringBoundary::GenericSlow,
                "residual {name} must have an explicit generic boundary"
            );
            assert_eq!(op.generic_fallback_name(), Some(name));
            match lower(&op) {
                LoweredInstruction::Slow(retained) => {
                    assert_eq!(retained.variant_name(), name);
                }
                LoweredInstruction::Fast(_) => {
                    panic!("residual {name} unexpectedly entered compact lowering")
                }
            }
        }
    }

    #[test]
    fn cold_marker_payload_uses_the_canonical_operation_fields() {
        use crate::ops::Op;
        assert_eq!(
            cold_marker_payload(&Op::MarkUninitialized {
                slot: 7,
                shared: true,
            }),
            (7, 1)
        );
        assert_eq!(cold_marker_payload(&Op::MarkImmutable { slot: 9 }), (9, 0));
        assert_eq!(
            cold_marker_payload(&Op::RequireObjectCoercible { src: 11 }),
            (11, 0)
        );
        assert_eq!(cold_marker_payload(&Op::Move { dst: 1, src: 2 }), (0, 0));
    }

    #[test]
    fn binary_lowering_covers_the_entire_operator_catalog() {
        use crate::ops::{BinaryOp, Op};
        for &operator in BinaryOp::ALL {
            let lowered = lower_compact(&Op::Binary {
                dst: 3,
                operator,
                lhs: 1,
                rhs: 2,
            })
            .expect("every binary operator has a fixed-width gateway");
            let expected = Opcode::binary_opcode(operator)
                .expect("the generated binary catalog must cover every operator");
            assert_eq!(
                lowered.opcode, expected,
                "binary lowering must use the generated dedicated row for {operator:?}"
            );
            assert_eq!(expected.numeric_operator(), Some(operator));
        }
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
        assert!(Opcode::Loop.is_slow());
        assert!(Opcode::CallSlow.is_slow());
        assert!(!Opcode::JumpIfFalse.is_slow());
        assert!(!Opcode::Jump.is_slow());
        assert!(Opcode::ForI.is_slow());
        assert!(!Opcode::Add.is_slow());
        assert_eq!(std::mem::size_of::<Register>(), 2);
        assert_eq!(MAX_REGISTER_ID, u16::MAX);
    }
    #[test]
    fn builtin_name_stays_dynamic_for_scope_correctness() {
        // A name that happens to match a realm builtin is still observable
        // through `with` and direct-eval scope objects. Keep the semantic
        // ResolveName path so those dynamic bindings cannot be bypassed by a
        // global GetN fast path.
        assert!(lower_compact(&crate::ops::Op::ResolveName {
            dst: 7,
            key: "Math".to_string(),
        })
        .is_none());

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

    #[test]
    fn operation_name_catalog_resolves_only_typed_cold_rows() {
        assert_eq!(Opcode::from_operation_name("Call"), Some(Opcode::CallSlow));
        assert_eq!(
            Opcode::from_operation_name("MarkImmutable"),
            Some(Opcode::MarkImmutable)
        );
        assert_eq!(Opcode::from_operation_name("Move"), None);
        assert_eq!(Opcode::from_operation_name("not_an_operation"), None);
    }
}
