//! The opcode catalog's tier facts, each declared once per opcode: its operation family
//! (`Class`, `class_of!`) and its orthogonal tier traits (`Traits`, `traits_of!`), which the
//! `opcodes!` table expands into `Op::class` and `Op::traits`.

/// A tier's operation family, declared once by the catalog (`Op::class`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Class {
    Other,
    Move,
    LoadConst,
    WasmFloatBinary32,
    WasmFloatBinary64,
    WasmFloatUnary32,
    WasmFloatUnary64,
    WasmI32AddRegister,
    WasmI32AddImmediate,
    WasmI32AddJump,
    WasmI32ShiftRightUnsignedAnd,
    WasmI32Unary,
    WasmI64Binary,
    WasmI64Unary,
    WasmScalarConvert,
    WasmSelect,
    WasmI32Load,
    WasmI32Load8U,
    WasmJumpI32EqualImmediate,
    WasmJumpI32NotEqualImmediate,
    GetField,
    LoadCapture,
    SetIndex,
    GetIndex,
    JumpBinaryFalse,
    LoadThis,
    Nop,
    Jump,
    JumpTrue,
    Unary,
    Return,
    IncDec,
    Binary,
    Call,
    Construct,
    RequireObjectCoercible,
    ToNumeric,
    CallKnown,
    Throw,
    Await,
    Yield,
    DefineArrayElement,
    StoreLocalPlain,
    LoadLocalPlain,
    NumericAdd,
    NumericMultiply,
    JumpFalse,
    CloneEnv,
    LoadName,
}

impl Class {
    /// Equality usable in a `const` context (the derived `PartialEq` is not const).
    pub(crate) const fn eq(self, other: Self) -> bool { self as u8 == other as u8 }
}

/// The class of each opcode, the catalog's one home for the family.
macro_rules! class_of {
    (Move) => { Class::Move };
    (LoadConst) => { Class::LoadConst };
    (WasmF32Binary) => { Class::WasmFloatBinary32 };
    (WasmF64Binary) => { Class::WasmFloatBinary64 };
    (WasmF32Unary) => { Class::WasmFloatUnary32 };
    (WasmF64Unary) => { Class::WasmFloatUnary64 };
    (WasmI32Add) => { Class::WasmI32AddRegister };
    (WasmI32AddImmediate) => { Class::WasmI32AddImmediate };
    (WasmI32AddImmediateJumpNonZero) => { Class::WasmI32AddJump };
    (WasmI32ShiftRightUnsignedAndImmediate) => { Class::WasmI32ShiftRightUnsignedAnd };
    (WasmI32Unary) => { Class::WasmI32Unary };
    (WasmI64Binary) => { Class::WasmI64Binary };
    (WasmI64Unary) => { Class::WasmI64Unary };
    (WasmScalarConvert) => { Class::WasmScalarConvert };
    (WasmSelect) => { Class::WasmSelect };
    (WasmI32Load) => { Class::WasmI32Load };
    (WasmI32Load8U) => { Class::WasmI32Load8U };
    (WasmJumpI32EqualImmediate) => { Class::WasmJumpI32EqualImmediate };
    (WasmJumpI32NotEqualImmediate) => { Class::WasmJumpI32NotEqualImmediate };
    (GetField) => { Class::GetField };
    (LoadCapture) => { Class::LoadCapture };
    (SetIndex) => { Class::SetIndex };
    (GetIndex) => { Class::GetIndex };
    (JumpBinaryFalse) => { Class::JumpBinaryFalse };
    (LoadThis) => { Class::LoadThis };
    (Nop) => { Class::Nop };
    (Jump) => { Class::Jump };
    (JumpTrue) => { Class::JumpTrue };
    (Unary) => { Class::Unary };
    (Return) => { Class::Return };
    (IncDec) => { Class::IncDec };
    (Binary) => { Class::Binary };
    (Call) => { Class::Call };
    (Construct) => { Class::Construct };
    (RequireObjectCoercible) => { Class::RequireObjectCoercible };
    (ToNumeric) => { Class::ToNumeric };
    (CallKnown) => { Class::CallKnown };
    (Throw) => { Class::Throw };
    (Await) => { Class::Await };
    (Yield) => { Class::Yield };
    (DefineArrayElement) => { Class::DefineArrayElement };
    (StoreLocalPlain) => { Class::StoreLocalPlain };
    (LoadLocalPlain) => { Class::LoadLocalPlain };
    (NumericAdd) => { Class::NumericAdd };
    (NumericMultiply) => { Class::NumericMultiply };
    (JumpFalse) => { Class::JumpFalse };
    (CloneEnv) => { Class::CloneEnv };
    (LoadName) => { Class::LoadName };
    ($other:ident) => { Class::Other };
}

/// Orthogonal tier traits of an opcode, each declared once by the catalog.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Traits(u16);

impl Traits {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const LEAF_VALUE: Self = Self(1 << 0);
    pub(crate) const LEAF_JUMP: Self = Self(1 << 1);
    pub(crate) const LEAF_BINARY: Self = Self(1 << 2);
    pub(crate) const THIS_LEAF: Self = Self(1 << 3);
    pub(crate) const NAME_LEAF: Self = Self(1 << 4);
    pub(crate) const FIELD_LEAF: Self = Self(1 << 5);
    pub(crate) const NAME_OPERATION: Self = Self(1 << 6);
    pub(crate) const MAKES_CLOSURE: Self = Self(1 << 7);
    pub(crate) const RETURNING_FIELD: Self = Self(1 << 8);
    pub(crate) const RETURNING_UNARY: Self = Self(1 << 9);
    pub(crate) const RETURNING_BINARY: Self = Self(1 << 10);
    pub(crate) const RETURNING_OBJECT: Self = Self(1 << 11);
    pub(crate) const RETURNING_OBJECT_LITERAL: Self = Self(1 << 12);
    pub(crate) const RETURNING_LITERAL: Self = Self(1 << 13);
    /// An op an inlined OSR callee may hold.
    pub(crate) const INLINABLE: Self = Self(1 << 14);
    /// An op a call's argument setup may run with no repeatable effect.
    pub(crate) const PURE_SETUP: Self = Self(1 << 15);

    pub(crate) const fn has(self, other: Self) -> bool { self.0 & other.0 != 0 }
    pub(crate) const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
}

/// The traits of each opcode, the catalog's one home for them.
macro_rules! traits_of {
    (Nop) => { Traits::LEAF_VALUE };
    (LoadConst) => { Traits::LEAF_VALUE.union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (Move) => { Traits::LEAF_VALUE.union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (Unary) => { Traits::LEAF_VALUE.union(Traits::RETURNING_UNARY).union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (IncDec) => { Traits::LEAF_VALUE.union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (Return) => { Traits::LEAF_VALUE.union(Traits::INLINABLE) };
    (Jump) => { Traits::LEAF_JUMP.union(Traits::INLINABLE) };
    (JumpFalse) => { Traits::LEAF_JUMP.union(Traits::INLINABLE) };
    (JumpTrue) => { Traits::LEAF_JUMP.union(Traits::INLINABLE) };
    (Binary) => { Traits::LEAF_BINARY.union(Traits::RETURNING_BINARY).union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (NumericAdd) => { Traits::LEAF_BINARY.union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (NumericMultiply) => { Traits::LEAF_BINARY.union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (JumpBinaryFalse) => { Traits::LEAF_BINARY.union(Traits::INLINABLE) };
    (LoadThis) => { Traits::THIS_LEAF };
    (LoadName) => { Traits::NAME_LEAF.union(Traits::NAME_OPERATION) };
    (LoadNameCall) => { Traits::NAME_OPERATION };
    (LoadNameTypeof) => { Traits::NAME_OPERATION };
    (ResolveName) => { Traits::NAME_OPERATION };
    (LoadResolvedName) => { Traits::NAME_OPERATION };
    (DeleteName) => { Traits::NAME_OPERATION };
    (StoreName) => { Traits::NAME_OPERATION };
    (StoreResolvedName) => { Traits::NAME_OPERATION };
    (StoreVarBinding) => { Traits::NAME_OPERATION };
    (CloneEnv) => { Traits::NAME_OPERATION };
    (InitializeThis) => { Traits::NAME_OPERATION };
    (SuperCallCheck) => { Traits::NAME_OPERATION };
    (MakeClosure) => { Traits::MAKES_CLOSURE };
    (GetField) => { Traits::FIELD_LEAF.union(Traits::RETURNING_FIELD).union(Traits::INLINABLE).union(Traits::PURE_SETUP) };
    (MakeObject2) => { Traits::RETURNING_OBJECT };
    (MakeObjectLiteral) => { Traits::RETURNING_OBJECT_LITERAL };
    (SuperConstArrayObject2) => { Traits::RETURNING_LITERAL };
    (Call) => { Traits::INLINABLE };
    (SetField) => { Traits::INLINABLE };
    (SetThisField) => { Traits::INLINABLE };
    (LoadCapture) => { Traits::INLINABLE };
    (ToNumeric) => { Traits::INLINABLE.union(Traits::PURE_SETUP) };
    (GetIndex) => { Traits::INLINABLE.union(Traits::PURE_SETUP) };
    (SetIndex) => { Traits::INLINABLE };
    (RequireObjectCoercible) => { Traits::INLINABLE.union(Traits::PURE_SETUP) };
    ($other:ident) => { Traits::NONE };
}

/// The guarded JavaScript denotation available to the shared compiled region
/// analyzer. This describes an instruction operation, never a source pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JsDenotation {
    Unsupported,
    Noop,
    Jump,
    LoadConst,
    Move,
    GetField,
    FieldStore,
    NumericBinary,
    NumericUnary,
    /// ToString of each register of a window, concatenated in order.
    StringConcatenation,
    CapturedValue,
    PlainLocalLoad,
    PlainLocalStore,
    Return,
}
