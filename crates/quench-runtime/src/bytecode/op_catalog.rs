//! The opcode catalog's tier facts, each declared once per opcode: its operation family
//! (`Class`, `class_of!`) and its orthogonal tier traits (`Traits`, `traits_of!`), which the
//! `opcodes!` table expands into `Op::class` and `Op::traits`.
use super::Op;

/// A tier's operation family, declared once by the catalog (`Op::class`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
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
    WasmFillRegisters,
    WasmMemoryAddress,
    WasmMemoryLoad,
    WasmMemoryStore,
    CallArray,
    CallForwardingArguments,
    CallSpread,
    CheckPrivate,
    CopyDataProperties,
    CreateRegExpLiteral,
    DefineField,
    DefinePrivateField,
    DefinePropertyRecord,
    Delete,
    InitializeTdz,
    IteratorCleanupPop,
    IteratorCleanupPush,
    LoadCallee,
    LoadEnvLocal,
    LoadLocal,
    LoadNameCall,
    LoadNameTypeof,
    MakeArray,
    MakeClosure,
    MakeConstArray,
    MakeObject,
    MakeObject2,
    MakeObjectLiteral,
    PrivateIn,
    RequireIteratorResult,
    StoreCapture,
    StoreEnvLocal,
    StoreLocal,
    StoreName,
    StringCaseIndex,
    SuperConstArrayObject2,
    ToPropertyKey,
    WasmBranchTable,
    WasmGlobalGet,
    WasmGlobalSet,
    WasmInstanceBinding,
    WasmJumpI32NonZero,
    WasmJumpI32Zero,
    WasmSimd,
    WasmSimdShuffle,
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
    (WasmFillRegisters) => { Class::WasmFillRegisters };
    (WasmMemoryAddress) => { Class::WasmMemoryAddress };
    (WasmMemoryLoad) => { Class::WasmMemoryLoad };
    (WasmMemoryStore) => { Class::WasmMemoryStore };
    (CallArray) => { Class::CallArray };
    (CallForwardingArguments) => { Class::CallForwardingArguments };
    (CallSpread) => { Class::CallSpread };
    (CheckPrivate) => { Class::CheckPrivate };
    (CopyDataProperties) => { Class::CopyDataProperties };
    (CreateRegExpLiteral) => { Class::CreateRegExpLiteral };
    (DefineField) => { Class::DefineField };
    (DefinePrivateField) => { Class::DefinePrivateField };
    (DefinePropertyRecord) => { Class::DefinePropertyRecord };
    (Delete) => { Class::Delete };
    (InitializeTdz) => { Class::InitializeTdz };
    (IteratorCleanupPop) => { Class::IteratorCleanupPop };
    (IteratorCleanupPush) => { Class::IteratorCleanupPush };
    (LoadCallee) => { Class::LoadCallee };
    (LoadEnvLocal) => { Class::LoadEnvLocal };
    (LoadLocal) => { Class::LoadLocal };
    (LoadNameCall) => { Class::LoadNameCall };
    (LoadNameTypeof) => { Class::LoadNameTypeof };
    (MakeArray) => { Class::MakeArray };
    (MakeClosure) => { Class::MakeClosure };
    (MakeConstArray) => { Class::MakeConstArray };
    (MakeObject) => { Class::MakeObject };
    (MakeObject2) => { Class::MakeObject2 };
    (MakeObjectLiteral) => { Class::MakeObjectLiteral };
    (PrivateIn) => { Class::PrivateIn };
    (RequireIteratorResult) => { Class::RequireIteratorResult };
    (StoreCapture) => { Class::StoreCapture };
    (StoreEnvLocal) => { Class::StoreEnvLocal };
    (StoreLocal) => { Class::StoreLocal };
    (StoreName) => { Class::StoreName };
    (StringCaseIndex) => { Class::StringCaseIndex };
    (SuperConstArrayObject2) => { Class::SuperConstArrayObject2 };
    (ToPropertyKey) => { Class::ToPropertyKey };
    (WasmBranchTable) => { Class::WasmBranchTable };
    (WasmGlobalGet) => { Class::WasmGlobalGet };
    (WasmGlobalSet) => { Class::WasmGlobalSet };
    (WasmInstanceBinding) => { Class::WasmInstanceBinding };
    (WasmJumpI32NonZero) => { Class::WasmJumpI32NonZero };
    (WasmJumpI32Zero) => { Class::WasmJumpI32Zero };
    (WasmSimd) => { Class::WasmSimd };
    ($other:ident) => { Class::Other };
}

/// Orthogonal tier traits of an opcode, each declared once by the catalog.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Traits(u32);

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
    /// A Wasm memory access through the function's memory register.
    pub(crate) const WASM_MEMORY: Self = Self(1 << 16);

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
    (WasmMemorySize) => { Traits::WASM_MEMORY };
    (WasmMemoryFill) => { Traits::WASM_MEMORY };
    (WasmMemoryCopy) => { Traits::WASM_MEMORY };
    (WasmMemoryAddress) => { Traits::WASM_MEMORY };
    (WasmMemoryLoad) => { Traits::WASM_MEMORY };
    (WasmMemoryStore) => { Traits::WASM_MEMORY };
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
    /// A fresh ordinary object with the realm's `Object.prototype` (`Vm::object`).
    ObjectAllocation,
    /// A literal's own data property definition, by name, through its cache site.
    FieldDefinition,
}

/// Direct opcode predicates the runtime paths use: a catalog-generated projection with no
/// table load (unlike `Op::class()`), for per-instruction loops.
macro_rules! op_predicates {
    ($($method:ident => $($op:ident)|+),+ $(,)?) => {
        #[allow(dead_code)]
        impl Op { $(
            pub(crate) const fn $method(self) -> bool { matches!(self, $(Op::$op)|+) }
        )+ }
    };
}
op_predicates! {
    is_return => Return,
    is_yield => Yield,
    is_await => Await,
    is_get_field => GetField,
    is_jump_true => JumpTrue,
    is_binary => Binary,
    is_move => Move,
    is_get_index => GetIndex,
    is_set_index => SetIndex,
    is_unary => Unary,
    is_numeric_add => NumericAdd,
    is_numeric_multiply => NumericMultiply,
    is_load_const => LoadConst,
    is_load_capture => LoadCapture,
    is_jump_binary_false => JumpBinaryFalse,
    is_jump_false => JumpFalse,
    is_jump => Jump,
    is_inc_dec => IncDec,
    is_to_numeric => ToNumeric,
    is_load_this => LoadThis,
    is_nop => Nop,
    is_call => Call,
    is_set_field => SetField,
    is_set_this_field => SetThisField,
    is_require_object_coercible => RequireObjectCoercible,
    is_define_array_element => DefineArrayElement,
    is_string_case_index => StringCaseIndex,
    is_construct => Construct,
    is_require_iterator_result => RequireIteratorResult,
    is_call_forwarding_arguments => CallForwardingArguments,
    is_wasm_memory_address => WasmMemoryAddress,
    is_wasm_instance_binding => WasmInstanceBinding,
    is_wasm_branch_table => WasmBranchTable,
    is_wasm_i32_add_immediate => WasmI32AddImmediate,
    is_wasm_i32_and_immediate => WasmI32AndImmediate,
    is_wasm_i32_shift_right_unsigned_and_immediate => WasmI32ShiftRightUnsignedAndImmediate,
    is_wasm_i32_load => WasmI32Load,
    is_wasm_i32_store => WasmI32Store,
    is_wasm_f64_load => WasmF64Load,
    is_wasm_f64_store => WasmF64Store,
    is_wasm_f64_binary => WasmF64Binary,
    is_throw => Throw,
    is_wasm_simd => WasmSimd,
    is_wasm_jump_i32_non_zero => WasmJumpI32NonZero,
    is_wasm_memory_size => WasmMemorySize,
    is_wasm_memory_copy => WasmMemoryCopy,
    is_wasm_memory_fill => WasmMemoryFill,
    is_call_array => CallArray,
    is_call_known => CallKnown,
    is_load_name_call => LoadNameCall,
    is_make_object => MakeObject,
    is_store_capture => StoreCapture,
    is_wasm_ref_cast => WasmRefCast,
    is_yield_star => YieldStar,
}

#[cfg(test)]
mod class_table_tests {
    use super::{Class, Op, Traits};

    #[test]
    fn catalog_tables_are_compact_and_the_runtime_uses_direct_predicates() {
        // One byte per opcode's class and four per traits: ~2 KiB for the whole catalog,
        // read only by planning code; the per-instruction loop calls the generated
        // `is_*` predicates, which compile to direct enum comparisons.
        assert_eq!(core::mem::size_of::<Class>(), 1);
        assert_eq!(core::mem::size_of::<Traits>(), 4);
        let bytes = Op::COUNT * (core::mem::size_of::<Class>() + core::mem::size_of::<Traits>());
        assert!(bytes <= 4096, "catalog tables take {bytes} bytes");
        assert!(Op::Call.class().eq(Class::Call) && !Op::Return.class().eq(Class::Call));
        assert!(Op::Return.class().eq(Class::Return) && !Op::Call.class().eq(Class::Return));
    }
}
