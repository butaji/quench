//! The opcode catalog's tier facts, each declared once per opcode: its operation family
//! (`Class`, `class_of!`) and its orthogonal tier traits (`Traits`, `traits_of!`), which the
//! `opcodes!` table expands into `Op::class` and `Op::traits`.
use super::Op;
pub(crate) use super::opcode_table::{OPCODE_COUNT, opcode_name};

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
    WasmI32SubtractImmediate,
    WasmI32AddJump,
    WasmI32ShiftRightUnsignedAnd,
    WasmI32Unary,
    WasmI64Binary,
    WasmI64Unary,
    WasmScalarConvert,
    WasmSelect,
    WasmI32Load,
    WasmI32Load8U,
    WasmI32Load16U,
    WasmI32Load16S,
    WasmI32Load8S,
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
    CallMethod,
    ArgumentsLength,
    ArgumentGet,
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
    WasmMemorySize,
    WasmMemoryFill,
    WasmMemoryCopy,
    WasmIndirectTarget,
    WasmRefFunc,
    WasmUnreachable,
    WasmRefGetDesc,
    WasmArrayNewData,
    WasmArrayNewElem,
    WasmArrayNew,
    WasmArrayNewDefault,
    WasmArrayNewFixed,
    WasmDescriptorTest,
    WasmDescriptorCast,
    WasmRefTest,
    WasmRefCast,
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
    MarkPrivateName,
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
    (WasmI32SubtractImmediate) => { Class::WasmI32SubtractImmediate };
    (WasmI32AddImmediateJumpNonZero) => { Class::WasmI32AddJump };
    (WasmI32ShiftRightUnsignedAndImmediate) => { Class::WasmI32ShiftRightUnsignedAnd };
    (WasmI32Unary) => { Class::WasmI32Unary };
    (WasmI64Binary) => { Class::WasmI64Binary };
    (WasmI64Unary) => { Class::WasmI64Unary };
    (WasmScalarConvert) => { Class::WasmScalarConvert };
    (WasmSelect) => { Class::WasmSelect };
    (WasmI32Load) => { Class::WasmI32Load };
    (WasmI32Load8U) => { Class::WasmI32Load8U };
    (WasmI32Load16U) => { Class::WasmI32Load16U };
    (WasmI32Load16S) => { Class::WasmI32Load16S };
    (WasmI32Load8S) => { Class::WasmI32Load8S };
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
    (CallMethod) => { Class::CallMethod };
    (ArgumentsLength) => { Class::ArgumentsLength };
    (ArgumentGet) => { Class::ArgumentGet };
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
    (WasmMemorySize) => { Class::WasmMemorySize };
    (WasmMemoryFill) => { Class::WasmMemoryFill };
    (WasmMemoryCopy) => { Class::WasmMemoryCopy };
    (WasmIndirectTarget) => { Class::WasmIndirectTarget };
    (WasmRefFunc) => { Class::WasmRefFunc };
    (WasmUnreachable) => { Class::WasmUnreachable };
    (WasmRefGetDesc) => { Class::WasmRefGetDesc };
    (WasmArrayNewData) => { Class::WasmArrayNewData };
    (WasmArrayNewElem) => { Class::WasmArrayNewElem };
    (WasmArrayNew) => { Class::WasmArrayNew };
    (WasmArrayNewDefault) => { Class::WasmArrayNewDefault };
    (WasmArrayNewFixed) => { Class::WasmArrayNewFixed };
    (WasmDescriptorTest) => { Class::WasmDescriptorTest };
    (WasmDescriptorCast) => { Class::WasmDescriptorCast };
    (WasmRefTest) => { Class::WasmRefTest };
    (WasmRefCast) => { Class::WasmRefCast };

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
    (MarkPrivateName) => { Class::MarkPrivateName };
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
    (WasmSimdShuffle) => { Class::WasmSimdShuffle };
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
    /// An op reading nothing of its activation (environment, `this`, captures, names), making no closure or call.
    pub(crate) const ACTIVATION_FREE: Self = Self(1 << 17);
    /// An op a call-free loop region lowers beside numeric binaries (`static_typed_regions`).
    pub(crate) const SCALAR_REGION: Self = Self(1 << 18);
    /// An indexed element read or write, which a loop region views its arrays for.
    pub(crate) const ELEMENT_ACCESS: Self = Self(1 << 19);
    /// A general step that reads and writes only the frame, its registers and heap values.
    pub(crate) const LANE_IN_PLACE: Self = Self(1 << 20);
    /// A frame-local step that runs beside frameless activations.
    pub(crate) const LANE_RESULT_STEP: Self = Self(1 << 21);
    /// An op whose result may return from the frame instead of the register file.
    pub(crate) const FRAME_RETURNABLE: Self = Self(1 << 22);
    /// An op that reads and writes only registers and control, no heap value.
    pub(crate) const FRAME_ONLY: Self = Self(1 << 23);
    /// An op the lifetime-role loop admission can rename a register through.
    pub(crate) const LIFETIME_SAFE: Self = Self(1 << 24);
    /// A call form a read memo's call site may answer.
    pub(crate) const SCRIPT_CALL: Self = Self(1 << 25);
    /// An op whose handler keeps its call target hint in the record's `call_link`.
    pub(crate) const CALL_LINK: Self = Self(1 << 26);
    /// A by-name load or store access.
    pub(crate) const GLOBAL_NAME_ACCESS: Self = Self(1 << 27);
    /// A Wasm op that calls out to the VM or ends the body, which no unrolled loop admits.
    pub(crate) const UNROLL_EXCLUDED: Self = Self(1 << 28);

    pub(crate) const fn has(self, other: Self) -> bool { self.0 & other.0 != 0 }
    pub(crate) const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
}

/// The traits of each opcode, the catalog's one home for them.
macro_rules! traits_of {
    (Nop) => { Traits::LEAF_VALUE.union(Traits::SCALAR_REGION).union(Traits::FRAME_ONLY) };
    (LoadConst) => { Traits::LEAF_VALUE.union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::ACTIVATION_FREE).union(Traits::SCALAR_REGION).union(Traits::LIFETIME_SAFE) };
    (Move) => { Traits::LEAF_VALUE.union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::ACTIVATION_FREE).union(Traits::SCALAR_REGION).union(Traits::FRAME_ONLY).union(Traits::LIFETIME_SAFE) };
    (Unary) => { Traits::LEAF_VALUE.union(Traits::RETURNING_UNARY).union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::ACTIVATION_FREE).union(Traits::FRAME_RETURNABLE).union(Traits::LIFETIME_SAFE) };
    (IncDec) => { Traits::LEAF_VALUE.union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::SCALAR_REGION).union(Traits::LIFETIME_SAFE) };
    (Return) => { Traits::LEAF_VALUE.union(Traits::INLINABLE).union(Traits::ACTIVATION_FREE).union(Traits::SCALAR_REGION) };
    (Jump) => { Traits::LEAF_JUMP.union(Traits::INLINABLE).union(Traits::ACTIVATION_FREE).union(Traits::SCALAR_REGION).union(Traits::FRAME_ONLY).union(Traits::LIFETIME_SAFE) };
    (JumpFalse) => { Traits::LEAF_JUMP.union(Traits::INLINABLE).union(Traits::ACTIVATION_FREE).union(Traits::SCALAR_REGION).union(Traits::FRAME_ONLY).union(Traits::LIFETIME_SAFE) };
    (JumpTrue) => { Traits::LEAF_JUMP.union(Traits::INLINABLE).union(Traits::ACTIVATION_FREE).union(Traits::SCALAR_REGION).union(Traits::FRAME_ONLY).union(Traits::LIFETIME_SAFE) };
    (Binary) => { Traits::LEAF_BINARY.union(Traits::RETURNING_BINARY).union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::ACTIVATION_FREE).union(Traits::LIFETIME_SAFE) };
    (NumericAdd) => { Traits::LEAF_BINARY.union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::ACTIVATION_FREE).union(Traits::LIFETIME_SAFE) };
    (NumericMultiply) => { Traits::LEAF_BINARY.union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::ACTIVATION_FREE).union(Traits::LIFETIME_SAFE) };
    (JumpBinaryFalse) => { Traits::LEAF_BINARY.union(Traits::INLINABLE).union(Traits::ACTIVATION_FREE).union(Traits::SCALAR_REGION).union(Traits::FRAME_ONLY).union(Traits::LIFETIME_SAFE) };
    (LoadThis) => { Traits::THIS_LEAF };
    (LoadName) => { Traits::NAME_LEAF.union(Traits::NAME_OPERATION).union(Traits::GLOBAL_NAME_ACCESS) };
    (LoadNameCall) => { Traits::NAME_OPERATION.union(Traits::CALL_LINK).union(Traits::GLOBAL_NAME_ACCESS) };
    (LoadNameTypeof) => { Traits::NAME_OPERATION.union(Traits::GLOBAL_NAME_ACCESS) };
    (ResolveName) => { Traits::NAME_OPERATION };
    (LoadResolvedName) => { Traits::NAME_OPERATION };
    (DeleteName) => { Traits::NAME_OPERATION };
    (StoreName) => { Traits::NAME_OPERATION.union(Traits::GLOBAL_NAME_ACCESS) };
    (StoreResolvedName) => { Traits::NAME_OPERATION };
    (StoreVarBinding) => { Traits::NAME_OPERATION };
    (CloneEnv) => { Traits::NAME_OPERATION };
    (InitializeThis) => { Traits::NAME_OPERATION };
    (SuperCallCheck) => { Traits::NAME_OPERATION.union(Traits::CALL_LINK) };
    (MakeClosure) => { Traits::MAKES_CLOSURE };
    (GetField) => { Traits::FIELD_LEAF.union(Traits::RETURNING_FIELD).union(Traits::INLINABLE).union(Traits::PURE_SETUP).union(Traits::FRAME_RETURNABLE) };
    (MakeObject2) => { Traits::RETURNING_OBJECT.union(Traits::FRAME_RETURNABLE) };
    (MakeObjectLiteral) => { Traits::RETURNING_OBJECT_LITERAL.union(Traits::FRAME_RETURNABLE) };
    (SuperConstArrayObject2) => { Traits::RETURNING_LITERAL.union(Traits::FRAME_RETURNABLE) };
    (Call) => { Traits::INLINABLE.union(Traits::FRAME_RETURNABLE).union(Traits::SCRIPT_CALL).union(Traits::CALL_LINK) };
    (SetField) => { Traits::INLINABLE };
    (SetThisField) => { Traits::INLINABLE };
    (LoadCapture) => { Traits::INLINABLE };
    (ToNumeric) => { Traits::INLINABLE.union(Traits::PURE_SETUP).union(Traits::LIFETIME_SAFE) };
    (GetIndex) => { Traits::INLINABLE.union(Traits::PURE_SETUP).union(Traits::ELEMENT_ACCESS).union(Traits::LIFETIME_SAFE) };
    (SetIndex) => { Traits::INLINABLE.union(Traits::ELEMENT_ACCESS).union(Traits::LIFETIME_SAFE) };
    (RequireObjectCoercible) => { Traits::INLINABLE.union(Traits::PURE_SETUP).union(Traits::LIFETIME_SAFE) };
    (WasmMemorySize) => { Traits::WASM_MEMORY };
    (WasmMemoryFill) => { Traits::WASM_MEMORY.union(Traits::UNROLL_EXCLUDED) };
    (WasmMemoryCopy) => { Traits::WASM_MEMORY.union(Traits::UNROLL_EXCLUDED) };
    (WasmMemoryAddress) => { Traits::WASM_MEMORY };
    (WasmMemoryLoad) => { Traits::WASM_MEMORY };
    (WasmMemoryStore) => { Traits::WASM_MEMORY };
    (DefineField) => { Traits::LANE_IN_PLACE };
    (DefineComputedField) => { Traits::LANE_IN_PLACE };
    (DefinePrivateField) => { Traits::LANE_IN_PLACE };
    (CheckPrivate) => { Traits::LANE_IN_PLACE };
    (PrivateIn) => { Traits::LANE_IN_PLACE };
    (MarkPrivateName) => { Traits::LANE_IN_PLACE };
    (SetFunctionName) => { Traits::LANE_IN_PLACE };
    (SetFunctionNameKey) => { Traits::LANE_IN_PLACE };
    (SetFunctionDebugName) => { Traits::LANE_IN_PLACE };
    (DefineArrayElement) => { Traits::LANE_IN_PLACE };
    (AppendArray) => { Traits::LANE_IN_PLACE.union(Traits::LANE_RESULT_STEP) };
    (ToPropertyKey) => { Traits::LANE_IN_PLACE.union(Traits::LANE_RESULT_STEP) };
    (CopyDataProperties) => { Traits::LANE_IN_PLACE };
    (GetIterator) => { Traits::LANE_IN_PLACE.union(Traits::LANE_RESULT_STEP) };
    (ForInStart) => { Traits::LANE_IN_PLACE.union(Traits::LANE_RESULT_STEP) };
    (ForInNext) => { Traits::LANE_IN_PLACE };
    (RequireIteratorResult) => { Traits::LANE_IN_PLACE };
    (IteratorClose) => { Traits::LANE_IN_PLACE.union(Traits::LANE_RESULT_STEP) };
    (IteratorCleanupPush) => { Traits::LANE_IN_PLACE };
    (IteratorCleanupPop) => { Traits::LANE_IN_PLACE };
    (CallForwardingArguments) => { Traits::FRAME_RETURNABLE.union(Traits::SCRIPT_CALL).union(Traits::CALL_LINK) };
    (Construct) => { Traits::FRAME_RETURNABLE.union(Traits::SCRIPT_CALL).union(Traits::CALL_LINK) };
    (LoadLocal) => { Traits::ACTIVATION_FREE };
    (LoadLocalPlain) => { Traits::ACTIVATION_FREE };
    (StoreLocal) => { Traits::ACTIVATION_FREE };
    (StoreLocalPlain) => { Traits::ACTIVATION_FREE };
    (CallArray) => { Traits::SCRIPT_CALL.union(Traits::CALL_LINK) };
    (CallKnown) => { Traits::SCRIPT_CALL.union(Traits::CALL_LINK) };
    (CallMethod) => { Traits::SCRIPT_CALL.union(Traits::CALL_LINK) };
    (CallSpread) => { Traits::SCRIPT_CALL.union(Traits::CALL_LINK) };
    (CallThisMethod) => { Traits::SCRIPT_CALL.union(Traits::CALL_LINK) };
    (WasmMemoryGrow) => { Traits::UNROLL_EXCLUDED };
    (WasmMemoryInit) => { Traits::UNROLL_EXCLUDED };
    (WasmTableGet) => { Traits::UNROLL_EXCLUDED };
    (WasmTableSet) => { Traits::UNROLL_EXCLUDED };
    (WasmTableGrow) => { Traits::UNROLL_EXCLUDED };
    (WasmTableFill) => { Traits::UNROLL_EXCLUDED };
    (WasmTableCopy) => { Traits::UNROLL_EXCLUDED };
    (WasmTableInit) => { Traits::UNROLL_EXCLUDED };
    (WasmTableSize) => { Traits::UNROLL_EXCLUDED };
    (WasmIndirectTarget) => { Traits::UNROLL_EXCLUDED };
    ($other:ident) => { Traits::NONE };
}


pub(crate) use super::denotation::JsDenotation;

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
    is_wasm_simd_shuffle => WasmSimdShuffle,
    is_wasm_f64_unary => WasmF64Unary,
    is_wasm_fill_registers => WasmFillRegisters,
    is_store_env_local => StoreEnvLocal,
    is_wasm_i32_add => WasmI32Add,
    is_wasm_i32_subtract_immediate => WasmI32SubtractImmediate,
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
