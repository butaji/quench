//! The opcode catalog's column types. The `opcodes!` table (`bytecode.rs`) is the one row per
//! opcode, and each fact is a column of it: operand shape, effects, flow, its operation family
//! (`Class`), its semantic source (`Decoder`: the staged operation every tier steps;
//! `GenericStep`: the interpreter's step a lane or a body makes in place) and its orthogonal
//! tier traits (`Traits`). The table expands into `Op::class`, `Op::staged`,
//! `Op::generic_step` and `Op::traits`.
use super::Op;
pub use super::opcode_table::{OPCODE_COUNT, opcode_name};
/// A tier's operation family, declared once by the catalog (`Op::class`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
pub enum Class {
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
    /// `a = b op c` on i64, `c` a signed 16-bit constant and the operator the selector.
    WasmI64BinaryImmediate,
    WasmI64Unary,
    /// The i64 operations over raw registers (`compile::liveness::raw_webs`): their i64
    /// operands and results are machine words, not `Value`s.
    WasmI64BinaryRaw,
    WasmI64BinaryImmediateRaw,
    WasmI64UnaryRaw,
    WasmFloatBinary64Raw,
    WasmFloatUnary64Raw,
    /// A Wasm return of a raw i64 register, which boxes the word where a `Value` cannot hold it.
    WasmReturn64,
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
    WasmMemoryGrow,
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
    CallRest,
    RestGet,
    RestLength,
    CheckPrivate,
    CopyDataProperties,
    CreateRegExpLiteral,
    DefineField,
    DefinePrivateField,
    MarkPrivateName,
    ForInNext,
    YieldStar,
    DefinePropertyRecord,
    Delete,
    InitializeTdz,
    GetIterator, IteratorClose,
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
    SetFunctionName,
    AppendArray,
    RequireIteratorResult,
    StoreCapture,
    StoreEnvLocal,
    StoreLocal,
    StoreName,
    StoreVarBinding,
    StringCaseIndex,
    SuperConstArrayObject2,
    ToPropertyKey,
    ForInStart,
    WasmBranchTable,
    WasmGlobalGet,
    WasmGlobalSet,
    WasmInstanceBinding,
    WasmJumpI32NonZero,
    WasmJumpI32Zero,
    WasmSimd,
    WasmSimdShuffle,
    WasmTableSize,
    WasmTableGet,
    WasmTableSet,
    WasmTableGrow,
    WasmTableFill,
    WasmTableCopy,
    WasmTableInit,
    WasmMemoryInit,
    WasmSegmentDrop,
    TierPrologue,
    BodyEntry,
}

impl Class {
    /// Equality usable in a `const` context (the derived `PartialEq` is not const).
    pub(crate) const fn eq(self, other: Self) -> bool { self as u8 == other as u8 }
}

/// The decoder of an opcode's staged operation (`Operation::decode`): the one column that
/// says which opcodes have a staged operation and which family builds it. An opcode with no
/// decoder is the interpreter's hand path alone (a tier leaves at its exact PC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decoder {
    /// A register move, a constant, a jump, a return or a suspension (`await`, `yield`, `yield*`).
    Flow,
    /// A frame local's load or store.
    Local,
    /// A value step the words decide (`Binary`, `ToPropertyKey`, `JumpBinaryFalse`).
    Generic,
    /// A value, `this` or capture operation.
    Value,
    /// A named field read, a named field store or `delete`.
    Property,
    /// An indexed element read or write.
    Element,
    /// A call or a construction through its forms.
    Call,
    /// An environment, global, iteration or spread operation.
    Names,
    /// A by-name read decoded under a tier's proof of the global it names
    /// (`Operation::decode_load_name`), so `Operation::decode` has none.
    Proved,
    /// A literal's allocation (`Operation::Allocate` over `Allocation`) or an array literal's
    /// element definition (`Operation::DefineElement`), decoded by `Operation::decode_literal`
    /// under the tier's proof of the allocation, so `Operation::decode` has none.
    Literal,
    /// A Wasm i32 or i64 register or immediate operator (`staged::wasm_int`).
    WasmInt,
    /// The float and conversion operators of Wasm (`staged::wasm_float`): the scalar operators a
    /// `WasmFloat` record names, with `WasmSelect` as its word copy.
    WasmFloat,
    /// A Wasm i32 conditional jump or fused add-and-jump: int32 operands, and an edge the row
    /// answers (`staged::WasmJump`).
    WasmJump,
    /// The Wasm instance's state: a global's load and store, and the fill of a register window
    /// with one constant (`staged::wasm_state`).
    WasmState,
    /// A Wasm direct load or store of the function's memory (`staged::wasm_access`).
    WasmMemory,
    /// A Wasm SIMD operator or byte shuffle on the lane's vector registers (`staged::wasm_simd`).
    WasmVector,
    /// A row only the lane's frame answers, which the plan does not decode (`Operation::decode_lane`):
    /// a string-case index and an `await` the lane suspends.
    Lane,
}

/// An opcode's general step a lane or a body makes in place, by one helper of the
/// interpreter's own definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenericStep {
    /// A body's helper only: it may replace the frame's environment, which the lane's own
    /// dispatch caches (the scope, operator, constant, name, literal, element, field and class steps).
    Body,
    /// The lane makes it in place: it reads and writes only the frame, its registers and heap
    /// values, and continues or throws.
    Lane,
    /// `Lane`, and a call-out row (`Operation::call_out`) whose result the lane writes back
    /// to its frame (`Vm::call_out`).
    LaneResult,
}

/// The step of an activation's own variables an opcode makes, over the `VarOwner` that runs it
/// (`Vm::owner_step`): a frame in the interpreter, the window of a frameless activation in a lane's body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedStep {
    LoadLocal,
    StoreVarBinding,
    StoreLocal,
    InitializeTdz,
    StoreEnvLocal,
    LoadCapture,
    StoreCapture,
    LoadName,
    LoadNameCall,
    LoadNameTypeof,
    StoreName,
    StoreGlobalLexical,
}

/// The step an opcode makes over operand values read from registers and constants, which an
/// activation with no frame of its own makes as a frame does (`Vm::binary`, `Vm::unary`,
/// `Vm::binary_truthy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueStep {
    Binary,
    Unary,
    CompareJump,
}

/// Orthogonal tier traits of an opcode, each declared once by the catalog.
#[derive(Clone, Copy, PartialEq, Eq)]
/// Two words: the first 128 traits, then the rest (`Self(0, 1 << n)`).
pub struct Traits(pub(crate) u128, pub(crate) u64);

impl Traits {
    pub(crate) const NONE: Self = Self(0, 0);
    pub(crate) const LEAF_VALUE: Self = Self(1 << 0, 0);
    pub(crate) const LEAF_JUMP: Self = Self(1 << 1, 0);
    pub(crate) const LEAF_BINARY: Self = Self(1 << 2, 0);
    pub const THIS_LEAF: Self = Self(1 << 3, 0);
    pub const NAME_LEAF: Self = Self(1 << 4, 0);
    pub const FIELD_LEAF: Self = Self(1 << 5, 0);
    pub(crate) const NAME_OPERATION: Self = Self(1 << 6, 0);
    pub(crate) const MAKES_CLOSURE: Self = Self(1 << 7, 0);
    pub const RETURNING_FIELD: Self = Self(1 << 8, 0);
    pub const RETURNING_UNARY: Self = Self(1 << 9, 0);
    pub const RETURNING_BINARY: Self = Self(1 << 10, 0);
    pub const RETURNING_OBJECT: Self = Self(1 << 11, 0);
    pub const RETURNING_OBJECT_LITERAL: Self = Self(1 << 12, 0);
    pub const RETURNING_LITERAL: Self = Self(1 << 13, 0);
    /// An op an inlined OSR callee may hold.
    pub const INLINABLE: Self = Self(1 << 14, 0);
    /// An op a call's argument setup may run with no repeatable effect.
    pub const PURE_SETUP: Self = Self(1 << 15, 0);
    /// A Wasm memory access through the function's memory register.
    pub(crate) const WASM_MEMORY: Self = Self(1 << 16, 0);
    /// An op reading nothing of its activation (environment, `this`, captures, names), making no closure or call.
    pub const ACTIVATION_FREE: Self = Self(1 << 17, 0);
    /// An op a call-free loop region lowers beside numeric binaries (`static_typed_regions`).
    pub const SCALAR_REGION: Self = Self(1 << 18, 0);
    /// An indexed element read or write, which a loop region views its arrays for.
    pub const ELEMENT_ACCESS: Self = Self(1 << 19, 0);
    /// An op whose result may return from the frame instead of the register file.
    pub const FRAME_RETURNABLE: Self = Self(1 << 20, 0);
    /// An op that reads and writes only registers and control, no heap value.
    pub const FRAME_ONLY: Self = Self(1 << 21, 0);
    /// An op the lifetime-role loop admission can rename a register through.
    pub(crate) const LIFETIME_SAFE: Self = Self(1 << 22, 0);
    /// An op whose handler keeps its call target hint in the record's `call_link`.
    pub const CALL_LINK: Self = Self(1 << 23, 0);
    /// A by-name load or store access.
    pub const GLOBAL_NAME_ACCESS: Self = Self(1 << 24, 0);
    /// A Wasm op that calls out to the VM or ends the body, which no unrolled loop admits.
    pub const UNROLL_EXCLUDED: Self = Self(1 << 25, 0);
    /// An op that always completes abruptly: it raises the value of its operand as an exception.
    pub(crate) const RAISES: Self = Self(1 << 26, 0);
    /// An op whose step binds the frame's `this` (`super()` completing a derived constructor's receiver).
    pub(crate) const BINDS_THIS: Self = Self(1 << 27, 0);
    /// An op an inlined callee's unit holds by its own rewrite or an exit (`lane_unit::emit_inlined`), which
    /// `osr_loop::renamed` is not asked about: a call, a return, `this`, a name read, a construction and a throw.
    pub(crate) const UNIT_REWRITTEN: Self = Self(1 << 28, 0);
    /// An op whose result may be an arbitrary boxed value: a read of a capture, a field, an element or a global name.
    pub(crate) const BOXED_RESULT: Self = Self(1 << 29, 0);
    /// A lane step that leaves its loop (`ForInNext` once no key is left), which the lane's loop
    /// dispatch makes with its own handler.
    pub const LANE_LOOP_EXIT: Self = Self(1 << 30, 0);
    /// An op whose lane handler is its staged row (`generated_row`), generated rather than written; the
    /// flow rows that fall through have theirs without the trait.
    pub const ROW_HANDLER: Self = Self(1 << 31, 0);
    /// An op that stores its register A into the frame local slot its immediate names (`LocalSlot`). When its
    /// optional register B is set, the stored value is also written there, and every tier honours B: the
    /// interpreter's owner step and plain-store arm, the lane windows' local stores, and the base tier, which
    /// leaves a store that sets B to the interpreter. `StoreEnvLocal` names an environment slot, so it is not one.
    /// Tagged: `StoreLocal`, `StoreLocalPlain`.
    pub(crate) const FRAME_LOCAL_STORE: Self = Self(1 << 32, 0);
    /// An op that loads the frame local slot its immediate names (`LocalSlot`) into its result register A, and
    /// writes no other register. `LoadEnvLocal` names an environment slot, so it is not one.
    /// Tagged: `LoadLocal`, `LoadLocalPlain`.
    pub(crate) const FRAME_LOCAL_LOAD: Self = Self(1 << 33, 0);
    /// An op that jumps to its `JumpTarget` when its register A is falsy, and falls through otherwise.
    /// `JumpTrue` jumps on the opposite test, and `JumpBinaryFalse` fuses its operator into the test, so
    /// neither is one. Tagged: `JumpFalse`.
    pub(crate) const JUMPS_ON_FALSE: Self = Self(1 << 34, 0);
    /// An op that writes the value of its `BinaryOperator` immediate applied to its two operands into its
    /// result register A, and writes no other register. The arithmetic rows name their operator in their own
    /// immediate role, so they are not one. Tagged: `Binary`.
    pub(crate) const BINARY_RESULT: Self = Self(1 << 35, 0);
    /// An op that stores its register A as the property its immediate atom names on `this`, through the cache
    /// site in C, and makes no other write. A field read may take it as its field site's sink (`FieldSite.sink`,
    /// a fused `GetSetThis`). Tagged: `SetThisField` alone. `SetThisFieldStrict` is not: the inlined expansion
    /// of a sink re-emits it as a `SetThisField`, so the strict variant has no sink in every tier.
    pub(crate) const THIS_FIELD_SINK: Self = Self(1 << 36, 0);
    /// The op that does nothing: the canonical filler a rewrite leaves where it removed an instruction.
    /// Tagged: `Nop`.
    pub(crate) const NO_OP: Self = Self(1 << 37, 0);
    /// An op that copies register B into its result register A and writes no other register. Tagged: `Move`.
    pub(crate) const REGISTER_COPY: Self = Self(1 << 38, 0);
    /// A pure op whose only effect is writing its result, so a write nothing reads is removable.
    /// Tagged: `Move`, `LoadConst`.
    pub(crate) const REMOVABLE_WHEN_DEAD: Self = Self(1 << 39, 0);
    /// An op that always jumps to its `JumpTarget`. Tagged: `Jump`.
    pub(crate) const UNCONDITIONAL_JUMP: Self = Self(1 << 40, 0);
    /// An op that jumps to its `JumpTarget` when its register A is truthy, falling through otherwise: the
    /// opposite test of `JUMPS_ON_FALSE`, one polarity of a conditional branch. Tagged: `JumpTrue`.
    pub(crate) const JUMPS_ON_TRUE: Self = Self(1 << 41, 0);
    /// An op that ends its activation, returning its register A. Tagged: `Return`.
    pub(crate) const RETURN_VALUE: Self = Self(1 << 42, 0);
    /// An op that suspends its activation (`await`, `yield`). Tagged: `Await`, `Yield`.
    pub const SUSPENDS: Self = Self(1 << 43, 0);
    /// A property store with sloppy-mode semantics, whose strict form (`STRICT_STORE`) shares its encoding.
    /// Tagged: `SetField`, `SetThisField`.
    pub(crate) const SLOPPY_STORE: Self = Self(1 << 44, 0);
    /// A property store with strict-mode semantics, the strict form of a `SLOPPY_STORE` row of the same
    /// encoding. Tagged: `SetFieldStrict`, `SetThisFieldStrict`.
    pub(crate) const STRICT_STORE: Self = Self(1 << 45, 0);
    /// The fused form of a comparison and the jump that tests it (`BinaryJumpFalse`'s target). Tagged: `JumpBinaryFalse`.
    pub(crate) const FUSED_BINARY_JUMP: Self = Self(1 << 46, 0);
    /// The first op of the `[MakeConstArray, Binary, Binary, MakeObject2]` superinstruction pattern. Tagged: `MakeConstArray`.
    pub(crate) const CONST_ARRAY_OBJECT2_HEAD: Self = Self(1 << 47, 0);
    /// An element op of the `CONST_ARRAY_OBJECT2` pattern (the two `Binary` rows). Tagged: `Binary`.
    pub(crate) const CONST_ARRAY_OBJECT2_OPERAND: Self = Self(1 << 48, 0);
    /// The last op of the `CONST_ARRAY_OBJECT2` pattern. Tagged: `MakeObject2`.
    pub(crate) const CONST_ARRAY_OBJECT2_TAIL: Self = Self(1 << 49, 0);
    /// The superinstruction that fuses `CONST_ARRAY_OBJECT2`. Tagged: `SuperConstArrayObject2`.
    pub(crate) const FUSED_CONST_ARRAY_OBJECT2: Self = Self(1 << 50, 0);
    /// The op that copies a whole activation environment for a per-iteration or block scope. Tagged: `CloneEnv`.
    pub(crate) const ENVIRONMENT_CLONE: Self = Self(1 << 71, 0);
    /// The op that loads an environment-resident local slot into its result register. Tagged: `LoadEnvLocal`.
    pub(crate) const ENV_LOCAL_LOAD: Self = Self(1 << 72, 0);
    /// The op that stores its register A into an environment-resident local slot. Tagged: `StoreEnvLocal`.
    pub(crate) const ENV_LOCAL_STORE: Self = Self(1 << 73, 0);
    /// The op that loads a captured slot into its result register. Tagged: `LoadCapture`.
    pub(crate) const CAPTURE_LOAD: Self = Self(1 << 74, 0);
    /// The op that stores its register A into a captured slot. Tagged: `StoreCapture`.
    pub(crate) const CAPTURE_STORE: Self = Self(1 << 75, 0);
    /// The indexed element read of an operand register (`obj[key]`). Tagged: `GetIndex`.
    pub(crate) const ELEMENT_LOAD: Self = Self(1 << 76, 0);
    /// A call that spreads the array of its one-value argument window. Tagged: `CallSpread`.
    pub(crate) const SPREAD_CALL: Self = Self(1 << 77, 0);
    /// A call that spreads a rest parameter's kept values in place of its array. Tagged: `CallRest`.
    pub const REST_SPREAD_CALL: Self = Self(1 << 78, 0);
    /// A read of one kept rest value by index. Tagged: `RestGet`.
    pub(crate) const REST_INDEX_READ: Self = Self(1 << 79, 0);
    /// The read of a rest parameter's length from its kept values. Tagged: `RestLength`.
    pub(crate) const REST_LENGTH_READ: Self = Self(1 << 80, 0);
    /// The call whose callee and arguments are both registers, without a spread or a method site. Tagged: `Call`.
    pub(crate) const PLAIN_CALL: Self = Self(1 << 81, 0);
    /// The raw-register form of a Wasm i64 or f64 operation (`raw_webs`). Tagged: every `*Raw` row (the i64/f64 operators and the GC field accesses of an i64 or f64 field) and `WasmReturn64`.
    pub const RAW_FORM: Self = Self(1 << 82, 0);
    /// The family of the i64 binary operation, boxed or raw. Tagged: `WasmI64Binary`, `WasmI64BinaryRaw`.
    pub(crate) const I64_BINARY_FAMILY: Self = Self(1 << 83, 0);
    /// The family of the i64 binary operation with an immediate operand. Tagged: `WasmI64BinaryImmediate`, `WasmI64BinaryImmediateRaw`.
    pub(crate) const I64_BINARY_IMMEDIATE_FAMILY: Self = Self(1 << 84, 0);
    /// The family of the i64 unary operation. Tagged: `WasmI64Unary`, `WasmI64UnaryRaw`.
    pub(crate) const I64_UNARY_FAMILY: Self = Self(1 << 85, 0);
    /// The family of the f64 binary operation. Tagged: `WasmF64Binary`, `WasmF64BinaryRaw`.
    pub(crate) const F64_BINARY_FAMILY: Self = Self(1 << 86, 0);
    /// The family of the f64 unary operation. Tagged: `WasmF64Unary`, `WasmF64UnaryRaw`.
    pub(crate) const F64_UNARY_FAMILY: Self = Self(1 << 87, 0);
    /// The family of a function's return, boxed or raw. Tagged: `Return`, `WasmReturn64`.
    pub const RETURN_FAMILY: Self = Self(1 << 88, 0);
    /// The family of a struct field read, boxed or raw. Tagged: `WasmStructGet`, `WasmStructGetRaw`.
    pub(crate) const STRUCT_GET_FAMILY: Self = Self(1 << 111, 0);
    /// The family of a struct field write, boxed or raw. Tagged: `WasmStructSet`, `WasmStructSetRaw`.
    pub(crate) const STRUCT_SET_FAMILY: Self = Self(1 << 112, 0);
    /// The family of an array element read, boxed or raw. Tagged: `WasmArrayGet`, `WasmArrayGetRaw`.
    pub(crate) const ARRAY_GET_FAMILY: Self = Self(1 << 113, 0);
    /// The family of an array element write, boxed or raw. Tagged: `WasmArraySet`, `WasmArraySetRaw`.
    pub(crate) const ARRAY_SET_FAMILY: Self = Self(1 << 114, 0);
    /// The op that puts a frame local slot into its temporal-dead-zone state. Tagged: `InitializeTdz`.
    pub(crate) const TDZ_INITIALIZE: Self = Self(1 << 91, 0);
    /// The op that checks a class's `extends` value before the class is built. Tagged: `ValidateClassHeritage`.
    pub(crate) const CLASS_HERITAGE_CHECK: Self = Self(1 << 92, 0);
    /// The op that marks a private method or accessor name onto its home object. Tagged: `MarkPrivateName`.
    pub(crate) const PRIVATE_NAME_MARK: Self = Self(1 << 93, 0);
    /// The op that converts a computed property key to its property key value. Tagged: `ToPropertyKey`.
    pub(crate) const PROPERTY_KEY_CONVERT: Self = Self(1 << 94, 0);
    /// The op that sets a function's `name` from a computed key and a name prefix. Tagged: `SetFunctionNameKey`.
    pub(crate) const FUNCTION_NAME_KEY_SET: Self = Self(1 << 95, 0);
    /// The op that calls a callee with a receiver and a contiguous argument window. Tagged: `Call`.
    pub(crate) const CALL_INVOKE: Self = Self(1 << 96, 0);
    /// The op that stores a value at an element index of an object. Tagged: `SetIndex`.
    pub(crate) const ELEMENT_STORE: Self = Self(1 << 97, 0);
    /// The op that stores a value into a named own field with sloppy-mode semantics. Tagged: `SetField`.
    pub(crate) const OBJECT_FIELD_STORE: Self = Self(1 << 98, 0);
    /// The op that defines a private class field on an instance. Tagged: `DefinePrivateField`.
    pub(crate) const PRIVATE_FIELD_DEFINE: Self = Self(1 << 99, 0);
    /// The op that defines a computed-key class field on an instance. Tagged: `DefineComputedField`.
    pub(crate) const COMPUTED_FIELD_DEFINE: Self = Self(1 << 100, 0);
    /// The op that defines a named class field on an instance. Tagged: `DefineField`.
    pub(crate) const FIELD_DEFINE: Self = Self(1 << 101, 0);
    /// The op that stores a value into a frame local slot as a binding initialization. Tagged: `StoreLocal`.
    pub(crate) const LOCAL_BINDING_STORE: Self = Self(1 << 102, 0);
    /// The op that defines a method or accessor property from a property definition record. Tagged: `DefinePropertyRecord`.
    pub(crate) const PROPERTY_RECORD_DEFINE: Self = Self(1 << 103, 0);
    /// The op that reads a template site's cached template object. Tagged: `LoadCachedTemplateObject`.
    pub(crate) const TEMPLATE_CACHE_LOAD: Self = Self(1 << 104, 0);
    /// The op that stores a freshly built template object into its template site's cache. Tagged: `CacheTemplateObject`.
    pub(crate) const TEMPLATE_CACHE_STORE: Self = Self(1 << 105, 0);
    /// The op that builds an array from a window of elements of a given length. Tagged: `MakeArray`.
    pub(crate) const ARRAY_MAKE: Self = Self(1 << 106, 0);
    /// The op that allocates a plain object with no properties. Tagged: `MakeObject`.
    pub(crate) const OBJECT_MAKE: Self = Self(1 << 107, 0);
    /// The op that loads a constant into its result register. Tagged: `LoadConst`.
    pub(crate) const CONSTANT_LOAD: Self = Self(1 << 108, 0);
    /// The op that concatenates a window of string parts into one string. Tagged: `StringConcat`.
    pub(crate) const STRING_CONCAT: Self = Self(1 << 109, 0);
    /// The op that runs a first-call stub's function body as its compiled chunk. Tagged: `CallFirstChunk`.
    pub const FIRST_CALL_CHUNK: Self = Self(1 << 110, 0);
    /// A suspension point of an activation: `await`, `yield` and `yield*`. Tagged: `Await`, `Yield`, `YieldStar`.
    pub(crate) const SUSPENSION_POINT: Self = Self(1 << 51, 0);
    /// An access that reads its object register as an indexed receiver. Tagged: `GetIndex`, `SetIndex`, `RequireObjectCoercible`.
    pub(crate) const INDEXED_RECEIVER: Self = Self(1 << 52, 0);
    /// A store to a binding by name. Tagged: `StoreName`.
    pub(crate) const NAME_STORE: Self = Self(1 << 54, 0);
    /// A load of a binding by name, throwing or `typeof`. Tagged: `LoadName`, `LoadNameCall`, `LoadNameTypeof`.
    pub(crate) const NAME_LOAD: Self = Self(1 << 55, 0);
    /// A read of the activation's passed arguments. Tagged: `ArgumentsLength`, `ArgumentGet`.
    pub(crate) const ARGUMENTS_READ: Self = Self(1 << 56, 0);
    /// A call through a callee register, with a window or a spread array. Tagged: `Call`, `CallArray`.
    pub(crate) const ORDINARY_CALL: Self = Self(1 << 57, 0);
    /// An op that captures the current environment: a closure or an environment clone. Tagged: `MakeClosure`, `CloneEnv`.
    pub(crate) const ENVIRONMENT_CAPTURE: Self = Self(1 << 59, 0);
    /// A load or store of an environment-resident local. Tagged: `LoadEnvLocal`, `StoreEnvLocal`.
    pub(crate) const ENV_LOCAL_ACCESS: Self = Self(1 << 60, 0);
    /// A frame local access that a capture layout can move into the environment. Tagged: `LoadLocal`, `StoreLocal`.
    pub(crate) const RELOCATABLE_LOCAL: Self = Self(1 << 61, 0);
    /// A load or store of a captured slot. Tagged: `LoadCapture`, `StoreCapture`.
    pub(crate) const CAPTURE_ACCESS: Self = Self(1 << 62, 0);
    /// A store of register A to a `this` field, through a named field site. Tagged: `SetThisField`, `SetThisFieldStrict`.
    pub(crate) const THIS_FIELD_STORE: Self = Self(1 << 63, 0);
    /// A store of register A into a frame or environment local slot its immediate names, not a plain one. Tagged: `StoreLocal`, `StoreEnvLocal`.
    pub(crate) const LOCAL_SLOT_STORE: Self = Self(1 << 65, 0);
    /// An op that reads or binds the implicit bindings an environment holds, by their form. Tagged: `ResolveName`,
    /// `LoadResolvedName`, `DeleteName`, `StoreResolvedName`, `StoreVarBinding`, `InitializeThis`, `SuperCallCheck`.
    pub(crate) const BINDING_FORM: Self = Self(1 << 66, 0);
    /// An op that defines an instance element of a class constructor. Tagged: `DefineField`, `DefinePrivateField`, `DefineComputedField`, `MarkPrivateName`.
    pub(crate) const INSTANCE_ELEMENT_DEFINE: Self = Self(1 << 67, 0);
    /// The check a `super(...)` construction makes of the parent's result. Tagged: `SuperCallCheck`.
    pub(crate) const SUPER_CALL_CHECK: Self = Self(1 << 69, 0);
    /// An op that constructs a value from a callee and a register window or array. Tagged: `Construct`.
    pub(crate) const CONSTRUCTION: Self = Self(1 << 70, 0);
    /// The op that loads a frame local slot's binding (`LocalSlot`) into its result register: the binding
    /// form of `FRAME_LOCAL_LOAD`, without its plain form. Tagged: `LoadLocal`.
    pub(crate) const LOCAL_BINDING_LOAD: Self = Self(1 << 116, 0);
    /// The op that loads a plain frame local slot (`Function::plain_local_slots`) into its result register,
    /// the plain form of `LOCAL_BINDING_LOAD`. Tagged: `LoadLocalPlain`.
    pub(crate) const PLAIN_LOCAL_LOAD: Self = Self(1 << 117, 0);
    /// The op that stores its register A into a plain frame local slot, the plain form of
    /// `LOCAL_BINDING_STORE`. Tagged: `StoreLocalPlain`.
    pub(crate) const PLAIN_LOCAL_STORE: Self = Self(1 << 118, 0);
    /// The op that steps its register B by one (up or down, by its `BooleanFlag`) into its result register A.
    /// Tagged: `IncDec`.
    pub(crate) const INCREMENT_UPDATE: Self = Self(1 << 119, 0);
    /// The op that allocates a plain object literal from a pair of registers, its two values in its object
    /// site's atom order. Tagged: `MakeObject2`.
    pub(crate) const PAIR_LITERAL_MAKE: Self = Self(1 << 120, 0);
    /// The op that allocates a plain object literal from a window of registers, one value per object-site
    /// atom in order. Tagged: `MakeObjectLiteral`.
    pub(crate) const WINDOW_LITERAL_MAKE: Self = Self(1 << 121, 0);
    /// The op that stores its register A into a named field of the object its register B holds (`obj.x = v`),
    /// with sloppy or strict semantics. `SetThisField` stores into `this`, so it is not one. Tagged: `SetField`,
    /// `SetFieldStrict`.
    pub(crate) const NAMED_FIELD_WRITE: Self = Self(1 << 122, 0);
    /// An op that reads a frame slot into its result register: a local's binding or an arguments slot,
    /// the slot its `LocalSlot` immediate names. Tagged: `LoadLocal`, `ArgumentsLength`, `ArgumentGet`.
    pub(crate) const FRAME_SLOT_READ: Self = Self(1 << 123, 0);
    /// A value producer whose result register is its A field alone, which reads neither the register it
    /// writes nor any register it does not name, and whose abrupt completion is caught only by handlers
    /// that do not read that register. `coalesce_distant_producer_moves` may then write its value straight
    /// into a later `Move`'s destination, beyond the adjacent `ProducerMove` recipe. Ops that `plain_producer`
    /// already admits need no tag. Tagged: `GetIndex`, `ToNumeric`, `ToPropertyKey`, `MakeArray`,
    /// `MakeConstArray`, `MakeObject`, `MakeObjectLiteral`, `MakeLiteralTemplate`, `CreateRegExpLiteral`,
    /// `LoadCallee`, `LoadNameTypeof`, `LoadCachedTemplateObject`, `StringConcat`, `ArgumentsLength`,
    /// `ArgumentGet`, `NumericAdd`, `NumericMultiply`, `PrivateIn`.
    pub(crate) const DISTANT_PRODUCER: Self = Self(1 << 89, 0);
    /// The op that appends an element window to an array under construction. Tagged: `AppendArray`.
    /// The Wasm op that copies one of two registers by a condition. Tagged: `WasmSelect`.
    pub(crate) const WASM_SELECT: Self = Self(1 << 64, 0);
    /// The Wasm op that fills a register run from one constant. Tagged: `WasmFillRegisters`.
    pub(crate) const REGISTER_FILL: Self = Self(1 << 68, 0);
    pub(crate) const ARRAY_APPEND: Self = Self(1 << 124, 0);
    /// An op that reaches a binding by its name (an atom immediate): `LoadName`, `LoadNameCall`,
    /// `LoadNameTypeof`, `ResolveName`, `StoreName`, `DeleteName`.
    pub(crate) const BY_NAME_ACCESS: Self = Self(1 << 125, 0);
    /// A Wasm op whose register window's length the atomic operator in its immediate fixes. Tagged: `WasmAtomicAccess`.
    pub(crate) const WASM_ATOMIC_WINDOW: Self = Self(1 << 126, 0);
    /// A Wasm op whose register window holds an exception's inputs. Tagged: `WasmExceptionNew`.
    pub(crate) const WASM_EXCEPTION_WINDOW: Self = Self(1 << 127, 0);
    /// A Wasm op whose constant is a 64-bit memory offset. Tagged: `WasmMemoryAddress`.
    pub(crate) const WASM_MEMORY_OFFSET_CONSTANT: Self = Self(1 << 53, 0);
    /// A Wasm op whose constant is a V128 lane shuffle. Tagged: `WasmSimdShuffle`.
    pub(crate) const WASM_SHUFFLE_CONSTANT: Self = Self(1 << 58, 0);

    /// An op whose step may allocate a heap value or call guest code, so its step may make a collection due
    /// (`note_allocation`). Conservative: every op is tagged unless its step only moves registers, jumps or
    /// reads and writes a frame local, or is a Wasm op whose effect is pure or control alone.
    pub const MAY_ALLOCATE: Self = Self(0, 1 << 0);
    /// A Wasm opcode (its catalog name begins `Wasm`): its own compiler owns the memory, tables and
    /// typed registers it reads, so the b-jit compiles no function holding one (`base_region::covers`).
    pub const WASM_OPCODE: Self = Self(0, 1 << 1);
    /// An op whose staged row is a register move, a local slot access or a jump, which decodes over the
    /// register file alone (`Operation::decode_flow`, `decode_local`). Tagged: `Move`, `LoadLocalPlain`,
    /// `StoreLocalPlain`, `Jump`, `JumpFalse`, `JumpTrue`.
    pub(crate) const REGISTER_ROW: Self = Self(0, 1 << 2);
    /// An op the general step does nothing for (`Nop`), or that is never dispatched (`Wide`, the wide marker).
    pub(crate) const NO_STEP: Self = Self(0, 1 << 3);
    /// An op whose step is its owner's own (`Vm::owner_step`) alone, with no staged row first: an environment
    /// local's load or store and a by-name store.
    pub(crate) const OWNER_ONLY: Self = Self(0, 1 << 4);
    /// An op its staged row decides on the interpreter's frame alone, which never declines: the general step
    /// runs the row and nothing else.
    pub(crate) const ROW_ONLY: Self = Self(0, 1 << 5);

    /// A named field store (`SetField`, `SetFieldStrict`, `SetThisField`, `SetThisFieldStrict`): its receiver is
    /// field B's register, or the frame's `this` with `FIELD_STORE_THIS`; a rejected write throws with
    /// `FIELD_STORE_STRICT`. Names for the store roles the rows carry, not traits of their own.
    pub(crate) const FIELD_STORE: Self = Self::SLOPPY_STORE.union(Self::STRICT_STORE);
    pub(crate) const FIELD_STORE_THIS: Self = Self::THIS_FIELD_STORE;
    pub(crate) const FIELD_STORE_STRICT: Self = Self::STRICT_STORE;
    /// A binary operation whose arithmetic operator the operand's `binary_operator` names (`Binary`,
    /// `NumericAdd`, `NumericMultiply`).
    pub(crate) const ARITHMETIC_BINARY: Self = Self(0, 1 << 6);

    #[inline(always)]
    pub const fn has(self, other: Self) -> bool { self.0 & other.0 != 0 || self.1 & other.1 != 0 }
    pub const fn union(self, other: Self) -> Self { Self(self.0 | other.0, self.1 | other.1) }
}

pub use super::denotation::JsDenotation;

/// Direct opcode predicates the runtime paths use: a catalog-generated projection with no
/// table load (unlike `Op::class()`), for per-instruction loops.
macro_rules! op_predicates {
    ($($method:ident => $($op:ident)|+),+ $(,)?) => {
        #[allow(dead_code)]
        impl Op { $(
            pub const fn $method(self) -> bool { matches!(self, $(Op::$op)|+) }
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
    is_load_name => LoadName,
    is_load_env_local => LoadEnvLocal,
    is_make_array => MakeArray,
    is_make_const_array => MakeConstArray,
    is_make_object2 => MakeObject2,
    is_jump_binary_false => JumpBinaryFalse,
    is_jump_false => JumpFalse,
    is_jump => Jump,
    is_inc_dec => IncDec,
    is_to_numeric => ToNumeric,
    is_load_this => LoadThis,
    is_nop => Nop,
    is_call => Call,
    is_call_array => CallArray,
    is_load_name_call => LoadNameCall,
    is_store_capture => StoreCapture,
    is_wasm_jump_i32_non_zero => WasmJumpI32NonZero,
    is_wasm_memory_copy => WasmMemoryCopy,
    is_wasm_memory_fill => WasmMemoryFill,
    is_wasm_memory_size => WasmMemorySize,
    is_wasm_simd => WasmSimd,
    is_yield_star => YieldStar,
    is_call_known => CallKnown,
    is_throw => Throw,
    is_wasm_ref_cast => WasmRefCast,
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
    is_wasm_simd_shuffle => WasmSimdShuffle,
    is_wasm_f64_unary => WasmF64Unary,
    is_wasm_fill_registers => WasmFillRegisters,
    is_store_env_local => StoreEnvLocal,
    is_wasm_i32_add => WasmI32Add,
    is_wasm_i32_subtract_immediate => WasmI32SubtractImmediate,
}

#[cfg(test)]
mod class_table_tests;
