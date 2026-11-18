//! The opcode catalog's column types. The `opcodes!` table (`bytecode.rs`) is the one row per
//! opcode, and each fact is a column of it: operand shape, effects, flow, its operation family
//! (`Class`), its semantic source (`Decoder`: the staged operation every tier steps;
//! `GenericStep`: the interpreter's step a lane or a body makes in place) and its orthogonal
//! tier traits (`Traits`). The table expands into `Op::class`, `Op::staged`,
//! `Op::generic_step` and `Op::traits`.
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
}

impl Class {
    /// Equality usable in a `const` context (the derived `PartialEq` is not const).
    pub(crate) const fn eq(self, other: Self) -> bool { self as u8 == other as u8 }
}

/// The decoder of an opcode's staged operation (`Operation::decode`): the one column that
/// says which opcodes have a staged operation and which family builds it. An opcode with no
/// decoder is the interpreter's hand path alone (a tier leaves at its exact PC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decoder {
    /// A register move, a constant, a jump or a return.
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
pub(crate) enum GenericStep {
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
pub(crate) enum OwnedStep {
    LoadLocal,
    StoreVarBinding,
    StoreLocal,
    StoreEnvLocal,
    LoadCapture,
    StoreCapture,
    LoadName,
    LoadNameCall,
    LoadNameTypeof,
    StoreName,
}

/// The step an opcode makes over operand values read from registers and constants, which an
/// activation with no frame of its own makes as a frame does (`Vm::binary`, `Vm::unary`,
/// `Vm::binary_truthy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueStep {
    Binary,
    Unary,
    CompareJump,
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
    /// An op whose result may return from the frame instead of the register file.
    pub(crate) const FRAME_RETURNABLE: Self = Self(1 << 20);
    /// An op that reads and writes only registers and control, no heap value.
    pub(crate) const FRAME_ONLY: Self = Self(1 << 21);
    /// An op the lifetime-role loop admission can rename a register through.
    pub(crate) const LIFETIME_SAFE: Self = Self(1 << 22);
    /// An op whose handler keeps its call target hint in the record's `call_link`.
    pub(crate) const CALL_LINK: Self = Self(1 << 23);
    /// A by-name load or store access.
    pub(crate) const GLOBAL_NAME_ACCESS: Self = Self(1 << 24);
    /// A Wasm op that calls out to the VM or ends the body, which no unrolled loop admits.
    pub(crate) const UNROLL_EXCLUDED: Self = Self(1 << 25);
    /// An op that always completes abruptly: it raises the value of its operand as an exception.
    pub(crate) const RAISES: Self = Self(1 << 26);
    /// An op whose step binds the frame's `this` (`super()` completing a derived constructor's receiver).
    pub(crate) const BINDS_THIS: Self = Self(1 << 27);
    /// An op an inlined callee's unit holds by its own rewrite or an exit (`lane_unit::emit_inlined`), which
    /// `osr_loop::renamed` is not asked about: a call, a return, `this`, a name read, a construction and a throw.
    pub(crate) const UNIT_REWRITTEN: Self = Self(1 << 28);
    /// An op whose result may be an arbitrary boxed value: a read of a capture, a field, an element or a global name.
    pub(crate) const BOXED_RESULT: Self = Self(1 << 29);
    /// A lane step that leaves its loop (`ForInNext` once no key is left), which the lane's loop
    /// dispatch makes with its own handler.
    pub(crate) const LANE_LOOP_EXIT: Self = Self(1 << 30);
    /// An op whose lane handler is its staged row (`lane_row`), generated rather than written; the
    /// flow rows that fall through have theirs without the trait.
    pub(crate) const ROW_HANDLER: Self = Self(1u32 << 31);

    pub(crate) const fn has(self, other: Self) -> bool { self.0 & other.0 != 0 }
    pub(crate) const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
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
