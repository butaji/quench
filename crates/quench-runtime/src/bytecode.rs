use smallvec::SmallVec;

pub(crate) const INTRINSIC_REGEXP_BINDING: &str = "\0quench:intrinsic-regexp";
/// The engine-internal global a `with` statement calls to enter its object scope.
pub(crate) const WITH_ENTER_BINDING: &str = "\0quench:with-enter";
pub(crate) const LEXICAL_THIS_BINDING: &str = "\0quench:lexical-this";
pub(crate) const NEW_TARGET_BINDING: &str = "\0quench:new-target";

pub type Atom = u32;
pub type Register = u16;

mod atoms;
mod control_flow;
mod instruction;
mod name_bindings;
pub(crate) use name_bindings::{CapturedNames, NameBindings};
mod numeric_ops;
pub(crate) use atoms::{AtomSequence, AtomTable};

/// Infix of the compiler's hidden atom for a named function expression's own binding:
/// `{name}{SELF_BINDING_MARKER}{function id}`.
pub(crate) const SELF_BINDING_MARKER: &str = "\0quench:self-binding:";
/// Prefix of the compiler's hidden local for a block-scoped binding renamed apart from a
/// same-named binding of its function: `{BLOCK_BINDING_MARKER}{name}`.
pub(crate) const BLOCK_BINDING_MARKER: &str = "\0quench:block-binding:";

/// Whether a local named `name` must live in its function's environment whatever its
/// captures: other hidden bindings (lexical `this`, `new.target`, home objects, private
/// names, ...) are reached by name, not only through capture slots. A renamed block binding
/// is reached only through its slot.
pub(crate) fn local_name_requires_environment(name: &str) -> bool {
    name.starts_with('\0') && !name.starts_with(BLOCK_BINDING_MARKER)
}
pub(crate) use control_flow::branch_table_entries;
pub use instruction::Instr;
pub(crate) use instruction::{ConstructArguments, RegisterWindow, WideInstruction};
pub(crate) use numeric_ops::specialized_numeric_op;
pub(crate) const RETURN_REGISTER: Register = 1 << 15;
/// Property cache sites are 16-bit. A program with more sites than that gives every site
/// past the range this last one, for which the VM never records a cache entry, so those
/// sites always take the uncached lookup.
pub(crate) const SHARED_UNCACHED_SITE: u16 = u16::MAX - 1;
pub(crate) const SET_THIS_REGISTER: Register = 1 << 14;
pub(crate) const REGISTER_MASK: Register = SET_THIS_REGISTER - 1;
pub(crate) const INLINE_OBJECT_SITE_ATOMS: usize = 2;
pub(crate) const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;
pub(crate) const NO_REGISTER_ROOT_MAP: u32 = u32::MAX;
pub(crate) const NO_OPTIONAL_REGISTER: Register = 0;
pub(crate) const OPTIONAL_REGISTER_BIAS: Register = 1;

/// The field encoding of an optional register operand.
pub(crate) const fn encode_optional_register(register: Option<Register>) -> Register {
    match register {
        Some(register) => register + OPTIONAL_REGISTER_BIAS,
        None => NO_OPTIONAL_REGISTER,
    }
}
pub(crate) const SINGLE_ARGUMENT_CALL_ARGUMENT_COUNT: u16 = 1;
pub(crate) const NUMERIC_LOCAL_DECREMENT_FLAG: Register = RETURN_REGISTER;
pub(crate) const NUMERIC_LOCAL_INC_STORE: u16 = 1;
pub(crate) const NO_NUMERIC_LOCAL_STORE_MARKER: u16 = 0;
pub(crate) const NUMERIC_LOCAL_TARGET: u16 = SET_THIS_REGISTER;
pub(crate) const FUNCTION_NAME_PREFIX_NONE: u32 = 0;
pub(crate) const FUNCTION_NAME_PREFIX_GETTER: u32 = 1;
pub(crate) const FUNCTION_NAME_PREFIX_SETTER: u32 = 2;
pub(crate) const ARRAY_INDEX_SENTINEL: u32 = u32::MAX;
const NO_RESULT_FLAGS: Register = 0;
#[derive(Clone, Debug)]
pub enum Constant {
    Number(f64),
    String(String),
    StringUnits(Vec<u16>),
    BigInt(String),
    WasmBits64(u64),
    WasmV128([u8; crate::wasm::V128_BYTES]),
    Boolean(bool),
    Null,
    Undefined,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Effect(u8);

impl Effect {
    const PURE: Self = Self(0);
    const READS_HEAP: Self = Self(1 << 0);
    const WRITES_HEAP: Self = Self(1 << 1);
    const THROWS: Self = Self(1 << 2);
    const CONTROL: Self = Self(1 << 3);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImmediateLayout {
    Scalar,
    CaptureDepthAndSlot,
    CallWindow,
    CallWindowWithEvalFlags,
    SingleArgumentCallWindowWithEvalFlags,
    ConstructCountAndFlags,
    RegisterPair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResultLayout {
    Register,
    RegisterReadWrite,
    Returnable,
    ReturnableAndThis,
    NumericReturnable,
}

/// The value an activation's registers start with. Wasm functions start at
/// i32 zero, the default of i32 and f32 locals, so those locals need no
/// prologue fill; both values are non-references, which collection skips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum InitialRegister {
    #[default]
    Undefined,
    I32Zero,
}

impl InitialRegister {
    pub(crate) fn value(self) -> crate::Value {
        match self {
            Self::Undefined => crate::Value::UNDEFINED,
            Self::I32Zero => crate::Value::integer(0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlFlowLayout {
    Fallthrough,
    Call,
    Jump,
    ConditionalJump,
    /// Continues at one of the `Jump` entries that follow the instruction;
    /// the immediate counts the cases before the final default entry.
    BranchTable,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldLayout {
    ResultRegister,
    Unused,
    Register,
    WriteRegister,
    ReadWriteRegister,
    OptionalRegister,
    NumericLocalTarget,
    NumericLocalStoreMarker,
    FunctionIndex,
    ConstructArguments,
    ElementCount,
    RegisterWindowBase,
    RegisterCount,
    FieldBase,
    CacheSiteIndex,
    FieldLookupCacheSiteIndex,
    WideIndexChunk,
    BooleanFlag,
    Operand,
    NumericIndexOperand,
    BinaryOperator,
    /// A 16-bit two's-complement operand carried in the instruction.
    SignedConstant,
}

impl FieldLayout {
    pub(crate) const fn is_register_field(self) -> bool {
        matches!(
            self,
            Self::ResultRegister
                | Self::Register
                | Self::RegisterWindowBase
                | Self::WriteRegister
                | Self::ReadWriteRegister
        )
    }

    pub(crate) const fn reads_register(self) -> bool {
        matches!(
            self,
            Self::Register | Self::RegisterWindowBase | Self::ReadWriteRegister
        )
    }

    pub(crate) const fn writes_register(self) -> bool {
        matches!(self, Self::WriteRegister | Self::ReadWriteRegister)
    }

    pub(crate) const fn is_operand_field(self) -> bool {
        matches!(self, Self::Operand | Self::NumericIndexOperand)
    }
}

impl Op {
    pub(crate) const fn has_result_register(self) -> bool {
        matches!(
            self.field_layout(InstructionField::A),
            FieldLayout::ResultRegister
        )
    }

    /// Whether the call immediate carries direct-eval flags, which make the call site reach
    /// bindings by name when its callee is the realm's `eval`.
    pub(crate) const fn is_call_with_eval_flags(self) -> bool {
        matches!(
            self.immediate_layout(),
            ImmediateLayout::CallWindowWithEvalFlags
                | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstructionField {
    #[allow(dead_code)]
    A,
    B,
    C,
}

impl InstructionField {
    pub(crate) const ALL: &'static [Self] = &[Self::A, Self::B, Self::C];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImmediateRole {
    LayoutEncoded,
    Unused,
    ConstantIndex,
    EnvironmentCloneIndex,
    ClosureFunctionIndex,
    ArrayLength,
    AtomIndex,
    LocalSlot,
    FunctionNamePrefix,
    PropertyDefinitionMode,
    BooleanFlag,
    ArrayIndex,
    BinaryOperator,
    AdditionOperator,
    MultiplicationOperator,
    UnaryOperator,
    WasmSignatureIndex,
    WasmFunctionIndex,
    WasmSimdOperator,
    WasmMemoryLoadOperator,
    WasmMemoryStoreOperator,
    WasmMemoryOffset,
    WasmInstanceSlot,
    WasmAtomicOperator,
    WasmI32Immediate,
    WasmI32UnaryOperator,
    WasmI31Operator,
    WasmExternalConversion,
    WasmReferenceTarget,
    WasmNonNullCheck,
    WasmGcTypeIndex,
    WasmStructFieldIndex,
    WasmExceptionFieldIndex,
    WasmI64BinaryOperator,
    WasmI64UnaryOperator,
    WasmScalarConversionOperator,
    WasmF32BinaryOperator,
    WasmF32UnaryOperator,
    WasmF64BinaryOperator,
    WasmF64UnaryOperator,
    TemplateSiteIndex,
    RegExpLiteralSiteIndex,
    JumpTarget,
    BranchTableCases,
    MethodSiteIndex,
    ObjectSiteIndex,
    SuperinstructionIndex,
    FieldLookup,
    WideInstructionIndex,
}

macro_rules! property_definition_modes {
    ($($mode:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum PropertyDefinitionMode { $($mode),+ }

        impl PropertyDefinitionMode {
            pub(crate) const ALL: &'static [Self] = &[$(Self::$mode),+];

            pub(crate) fn from_word(word: u32) -> Option<Self> {
                Self::ALL.get(usize::try_from(word).ok()?).copied()
            }

            pub(crate) const fn word(self) -> u32 {
                self as u32
            }
        }
    };
}

property_definition_modes!(
    Method,
    ReadonlyMethod,
    Getter,
    Setter,
    EnumerableGetter,
    EnumerableSetter,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldLookup {
    Atom {
        atom: Atom,
        base: FieldBase,
        cache_site: u16,
    },
    Site(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NumericLocalStoreTarget {
    pub(crate) register: Register,
    pub(crate) decrement: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OperandLayout {
    a: FieldLayout,
    b: FieldLayout,
    c: FieldLayout,
}

impl OperandLayout {
    const fn field(self, field: InstructionField) -> FieldLayout {
        match field {
            InstructionField::A => self.a,
            InstructionField::B => self.b,
            InstructionField::C => self.c,
        }
    }
}

impl ResultLayout {
    const fn allowed_flags(self) -> Register {
        match self {
            Self::Register => NO_RESULT_FLAGS,
            Self::RegisterReadWrite => NO_RESULT_FLAGS,
            Self::Returnable => RETURN_REGISTER,
            Self::ReturnableAndThis => RETURN_REGISTER | SET_THIS_REGISTER,
            Self::NumericReturnable => RETURN_REGISTER | NUMERIC_LOCAL_TARGET,
        }
    }

    const fn allows_return(self) -> bool {
        matches!(
            self,
            Self::Returnable | Self::ReturnableAndThis | Self::NumericReturnable
        )
    }

    const fn allows_this_write(self) -> bool {
        matches!(self, Self::ReturnableAndThis)
    }

    pub(crate) const fn reads_result_register(self) -> bool {
        matches!(self, Self::RegisterReadWrite)
    }

    const fn allows_numeric_local(self) -> bool {
        matches!(self, Self::NumericReturnable)
    }
}

impl ImmediateLayout {
    const DIRECT_EVAL_FLAG_BIT: u32 = u32::BITS - 1;
    const PARAMETER_EVAL_FLAG_BIT: u32 = Self::DIRECT_EVAL_FLAG_BIT - 1;
    const CALL_BASE_SHIFT: u32 = u16::BITS;
    const CALL_BASE_WIDTH: u32 = Self::PARAMETER_EVAL_FLAG_BIT - Self::CALL_BASE_SHIFT;
    const CALL_BASE_MASK: u32 = ((1 << Self::CALL_BASE_WIDTH) - 1) << Self::CALL_BASE_SHIFT;
    const ARGUMENT_COUNT_MASK: u32 = u16::MAX as u32;
    const DIRECT_EVAL_FLAG: u32 = 1 << Self::DIRECT_EVAL_FLAG_BIT;
    const PARAMETER_EVAL_FLAG: u32 = 1 << Self::PARAMETER_EVAL_FLAG_BIT;
    const SUPER_CONSTRUCT_FLAG: u32 = Self::DIRECT_EVAL_FLAG;
    const CONSTRUCT_ARRAY_FLAG: u32 = Self::PARAMETER_EVAL_FLAG;
    const PAIR_SECOND_SHIFT: u32 = u16::BITS;
    const PAIR_FIELD_MASK: u32 = u16::MAX as u32;

    pub(crate) const fn uses_packed_pair(self) -> bool {
        matches!(
            self,
            Self::CaptureDepthAndSlot
                | Self::CallWindow
                | Self::CallWindowWithEvalFlags
                | Self::SingleArgumentCallWindowWithEvalFlags
        )
    }

    pub(crate) const fn call_immediate(
        base: Register,
        count: u16,
        direct_eval: bool,
        parameter_eval: bool,
    ) -> u32 {
        ((base as u32) << Self::CALL_BASE_SHIFT)
            | (count as u32)
            | (if direct_eval {
                Self::DIRECT_EVAL_FLAG
            } else {
                0
            })
            | (if parameter_eval {
                Self::PARAMETER_EVAL_FLAG
            } else {
                0
            })
    }

    pub(crate) const fn direct_eval(imm: u32) -> bool {
        imm & Self::DIRECT_EVAL_FLAG != 0
    }

    pub(crate) const fn parameter_eval(imm: u32) -> bool {
        imm & Self::PARAMETER_EVAL_FLAG != 0
    }

    pub(crate) const fn call_window_base(imm: u32) -> Register {
        ((imm & Self::CALL_BASE_MASK) >> Self::CALL_BASE_SHIFT) as Register
    }

    pub(crate) const fn argument_count(imm: u32) -> u16 {
        (imm & Self::ARGUMENT_COUNT_MASK) as u16
    }

    pub(crate) const fn capture_depth(imm: u32) -> u16 {
        (imm >> Self::PAIR_SECOND_SHIFT) as u16
    }

    pub(crate) const fn capture_slot(imm: u32) -> u16 {
        (imm & Self::PAIR_FIELD_MASK) as u16
    }

    pub(crate) const fn capture_immediate(depth: usize, slot: u16) -> u32 {
        ((depth as u32) << Self::PAIR_SECOND_SHIFT) | slot as u32
    }

    pub(crate) const fn register_pair(imm: u32) -> (Register, Register) {
        (
            (imm & Self::PAIR_FIELD_MASK) as Register,
            (imm >> Self::PAIR_SECOND_SHIFT) as Register,
        )
    }

    pub(crate) const fn register_pair_immediate(first: Register, second: Register) -> u32 {
        ((second as u32) << Self::PAIR_SECOND_SHIFT) | first as u32
    }

    pub(crate) const fn construct_immediate(
        count: u16,
        super_call: bool,
        array_arguments: bool,
    ) -> u32 {
        (count as u32)
            | (if super_call {
                Self::SUPER_CONSTRUCT_FLAG
            } else {
                0
            })
            | (if array_arguments {
                Self::CONSTRUCT_ARRAY_FLAG
            } else {
                0
            })
    }

    pub(crate) const fn super_construct(imm: u32) -> bool {
        imm & Self::SUPER_CONSTRUCT_FLAG != 0
    }

    pub(crate) const fn construct_array_arguments(imm: u32) -> bool {
        imm & Self::CONSTRUCT_ARRAY_FLAG != 0
    }
}

macro_rules! opcodes {
    ($($name:ident => $effect:expr; layout $layout:ident $(; meaning $immediate_role:ident)? $(; flow $flow:ident)? , @ $result:ident, @ fields($a:ident, $b:ident, $c:ident)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u16)]
        pub enum Op { $($name),+ }

        #[allow(dead_code)]
        impl Op {
            pub const COUNT: usize = [$(stringify!($name)),+].len();
            pub const NAMES: [&'static str; Self::COUNT] = [$(stringify!($name)),+];
            const EFFECTS: [Effect; Self::COUNT] = [$($effect),+];
            const RESULT_LAYOUTS: [ResultLayout; Self::COUNT] = [$(
                opcodes!(@result $result)),+
            ];
            const CONTROL_FLOW_LAYOUTS: [ControlFlowLayout; Self::COUNT] = [$(
                opcodes!(@flow $($flow)?)),+
            ];
            const IMMEDIATE_LAYOUTS: [ImmediateLayout; Self::COUNT] = [$(
                opcodes!(@layout $layout)),+
            ];

            const OPERAND_LAYOUTS: [OperandLayout; Self::COUNT] = [$(
                opcodes!(@fields $a, $b, $c)),+
            ];
            const IMMEDIATE_ROLES: [ImmediateRole; Self::COUNT] = [$(
                opcodes!(@immediate $($immediate_role)?; $layout)),+
            ];

            pub(crate) const fn effect(self) -> Effect {
                Self::EFFECTS[self as usize]
            }

            pub(crate) const fn immediate_layout(self) -> ImmediateLayout {
                Self::IMMEDIATE_LAYOUTS[self as usize]
            }

            pub(crate) const fn result_layout(self) -> ResultLayout {
                Self::RESULT_LAYOUTS[self as usize]
            }

            pub(crate) const fn control_flow_layout(self) -> ControlFlowLayout {
                Self::CONTROL_FLOW_LAYOUTS[self as usize]
            }

            pub(crate) const fn field_layout(self, field: InstructionField) -> FieldLayout {
                Self::OPERAND_LAYOUTS[self as usize].field(field)
            }

            pub(crate) const fn immediate_role(self) -> ImmediateRole {
                Self::IMMEDIATE_ROLES[self as usize]
            }

            pub(crate) const fn is_wide_marker(self) -> bool {
                matches!(self.immediate_role(), ImmediateRole::WideInstructionIndex)
            }

            pub(crate) const fn from_index(index: usize) -> Option<Self> {
                if index < Self::COUNT {
                    // SAFETY: `opcodes!` emits a contiguous repr(u16) enum.
                    Some(unsafe { std::mem::transmute::<u16, Self>(index as u16) })
                } else {
                    None
                }
            }
        }
    };
    (@layout $layout:ident) => { ImmediateLayout::$layout };
    (@result $result:ident) => { ResultLayout::$result };
    (@flow $flow:ident) => { ControlFlowLayout::$flow };
    (@flow) => { ControlFlowLayout::Fallthrough };
    (@fields $a:ident, $b:ident, $c:ident) => {
        OperandLayout { a: FieldLayout::$a, b: FieldLayout::$b, c: FieldLayout::$c }
    };
    (@immediate $role:ident; $layout:ident) => { ImmediateRole::$role };
    (@immediate $role:ident;) => { ImmediateRole::$role };
    (@immediate; $layout:ident) => { ImmediateRole::LayoutEncoded };
    (@immediate;) => { compile_error!("opcode must declare an immediate role or layout") };
}
const READ_THROW: Effect = Effect::READS_HEAP.union(Effect::THROWS);
const WRITE_THROW: Effect = Effect::WRITES_HEAP.union(Effect::THROWS);
const CALL_EFFECT: Effect = READ_THROW.union(Effect::WRITES_HEAP);

opcodes!(
    Nop => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(Unused, Unused, Unused),
    CloneEnv => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning EnvironmentCloneIndex, @ Register, @ fields(Unused, Unused, Unused),
    Wide => Effect::PURE; layout Scalar; meaning WideInstructionIndex, @ Register, @ fields(WideIndexChunk, WideIndexChunk, WideIndexChunk),
    LoadConst => Effect::PURE; layout Scalar; meaning ConstantIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadLocal => Effect::PURE; layout Scalar; meaning LocalSlot, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocal => Effect::PURE; layout Scalar; meaning LocalSlot, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    StoreVarBinding => CALL_EFFECT; layout Scalar; meaning LocalSlot, @ Register, @ fields(Register, Unused, Unused),
    LoadEnvLocal => Effect::READS_HEAP; layout Scalar; meaning LocalSlot, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreEnvLocal => Effect::WRITES_HEAP; layout Scalar; meaning LocalSlot, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    LoadCapture => Effect::READS_HEAP; layout CaptureDepthAndSlot, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreCapture => Effect::WRITES_HEAP; layout CaptureDepthAndSlot, @ Register, @ fields(Register, Unused, Unused),
    LoadName => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, Unused, CacheSiteIndex),
    LoadNameCall => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, WriteRegister, CacheSiteIndex),
    LoadNameTypeof => Effect::READS_HEAP; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, Unused, CacheSiteIndex),
    ResolveName => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, BooleanFlag, CacheSiteIndex),
    LoadResolvedName => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, Register, BooleanFlag),
    DeleteName => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreName => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, BooleanFlag, CacheSiteIndex),
    StoreResolvedName => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Register, BooleanFlag),
    LoadThis => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadImportMeta => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeClosure => CALL_EFFECT; layout Scalar; meaning ClosureFunctionIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeArray => CALL_EFFECT; layout Scalar; meaning ArrayLength, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeConstArray => CALL_EFFECT; layout Scalar; meaning ConstantIndex, @ Register, @ fields(ResultRegister, ElementCount, Unused),
    MakeObject => CALL_EFFECT; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeObject2 => CALL_EFFECT; layout Scalar; meaning ObjectSiteIndex, @ Returnable, @ fields(ResultRegister, Register, Register),
    MakeObjectLiteral => CALL_EFFECT; layout Scalar; meaning ObjectSiteIndex, @ Returnable, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    SuperConstArrayObject2 => CALL_EFFECT; layout Scalar; meaning SuperinstructionIndex, @ Returnable, @ fields(ResultRegister, Unused, Unused),
    GetIterator => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    GetAsyncIterator => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    CreateRegExpLiteral => CALL_EFFECT; layout Scalar; meaning RegExpLiteralSiteIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    IteratorClose => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    // ArrayAccumulation past a literal's static prefix: `length` elisions, then the optional
    // value (iterated when the flag marks a spread), appended at the array's length.
    AppendArray => CALL_EFFECT; layout Scalar; meaning ArrayLength, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    RequireObjectCoercible => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Unused, Register, Unused),
    RequireIteratorResult => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Unused, Register, Unused),
    SuperCallCheck => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Unused, Unused, Unused),
    IteratorCleanupPush => Effect::CONTROL; layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Unused),
    IteratorCleanupPop => Effect::CONTROL; layout Scalar; meaning Unused, @ Register, @ fields(Unused, Unused, Unused),
    SetFunctionName => Effect::WRITES_HEAP; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, Unused),
    SetFunctionNameKey => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning FunctionNamePrefix, @ Register, @ fields(Register, Register, Unused),
    InitializeTdz => Effect::PURE; layout Scalar; meaning LocalSlot, @ Register, @ fields(Unused, Unused, Unused),
    Await => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    Yield => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    YieldStar => READ_THROW.union(Effect::CONTROL); layout RegisterPair, @ RegisterReadWrite, @ fields(ResultRegister, Register, ReadWriteRegister),
    GetField => READ_THROW; layout Scalar; meaning FieldLookup, @ ReturnableAndThis, @ fields(ResultRegister, FieldBase, FieldLookupCacheSiteIndex),
    GetIndex => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, NumericIndexOperand, NumericIndexOperand),
    ToPropertyKey => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    ToNumeric => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    CopyDataProperties => CALL_EFFECT; layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Register),
    MarkPrivateName => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Unused, Register, Register),
    DefinePrivateField => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Register, Register),
    SetField => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Register, CacheSiteIndex),
    DefineComputedField => WRITE_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Register),
    SetThisField => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    SetIndex => WRITE_THROW; layout Scalar; meaning BooleanFlag, @ Register, @ fields(Register, Register, Register),
    DefineArrayElement => WRITE_THROW; layout Scalar; meaning ArrayIndex, @ Register, @ fields(Register, Register, Unused),
    Binary => READ_THROW; layout Scalar; meaning BinaryOperator, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    IncDec => READ_THROW; layout Scalar; meaning BooleanFlag, @ Register, @ fields(ResultRegister, Register, Unused),
    Unary => READ_THROW; layout Scalar; meaning UnaryOperator, @ Returnable, @ fields(ResultRegister, Register, Unused),
    Delete => READ_THROW; layout Scalar; meaning BooleanFlag, @ Register, @ fields(ResultRegister, Register, Register),
    CheckPrivate => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, Unused),
    PrivateIn => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    Move => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    Call => CALL_EFFECT; layout CallWindowWithEvalFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallArray => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallKnown => CALL_EFFECT; layout CallWindow; flow Call, @ Returnable, @ fields(ResultRegister, FunctionIndex, Unused),
    CallMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call, @ Returnable, @ fields(ResultRegister, Register, Unused),
    CallThisMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call, @ Returnable, @ fields(ResultRegister, Unused, Unused),
    Construct => CALL_EFFECT; layout ConstructCountAndFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, ConstructArguments),
    Jump => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow Jump, @ Register, @ fields(Unused, Unused, Unused),
    JumpFalse => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Unused, Unused),
    JumpBinaryFalse => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(BinaryOperator, Operand, Operand),
    Return => Effect::CONTROL; layout Scalar; meaning Unused; flow Terminal, @ Register, @ fields(Register, Unused, Unused),
    Throw => Effect::THROWS.union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal, @ Register, @ fields(Register, Unused, Unused),
    NumericAdd => READ_THROW; layout Scalar; meaning AdditionOperator, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    NumericMultiply => READ_THROW; layout Scalar; meaning MultiplicationOperator, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    InitializeThis => CALL_EFFECT.union(Effect::CONTROL); layout Scalar; meaning Unused, @ Register, @ fields(Register, Unused, Unused),
    CacheTemplateObject => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning TemplateSiteIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadCachedTemplateObject => Effect::READS_HEAP; layout Scalar; meaning TemplateSiteIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    DefineField => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Register, Unused),
    ValidateClassHeritage => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Register, Unused, Unused),
    SetFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Register, CacheSiteIndex),
    SetThisFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    // The general loop's fast arms dispatch on these with the opcodes above, so they share
    // one dense range of opcode numbers.
    LoadLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    JumpTrue => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Unused, Unused),
    // `Call` whose last window argument the instruction before it loaded from the activation's
    // `arguments` binding (`CallPlan::forwards_arguments`).
    CallForwardingArguments => CALL_EFFECT; layout CallWindowWithEvalFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, Register),
    WasmI32Add => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Subtract => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Multiply => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32DivideSigned => Effect::THROWS; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32DivideUnsigned => Effect::THROWS; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RemainderSigned => Effect::THROWS; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RemainderUnsigned => Effect::THROWS; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32And => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Or => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Xor => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32ShiftLeft => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32ShiftRightSigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32ShiftRightUnsigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RotateLeft => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RotateRight => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Equal => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32NotEqual => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessSigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessUnsigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterSigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterUnsigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessEqualSigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessEqualUnsigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterEqualSigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterEqualUnsigned => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32AddImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32SubtractImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32MultiplyImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32DivideSignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32DivideUnsignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RemainderSignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RemainderUnsignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32AndImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32OrImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32XorImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32ShiftLeftImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32ShiftRightSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32ShiftRightUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RotateLeftImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RotateRightImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32EqualImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32NotEqualImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessEqualSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessEqualUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterEqualSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterEqualUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, Unused),
    // `(b >>> c) & imm`: a bit-field extraction fused from its two operators.
    WasmI32ShiftRightUnsignedAndImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    // `a = b + c`, then jump when `a` is nonzero: a counted loop's step and test.
    WasmI32AddImmediateJumpNonZero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    WasmInstanceBinding => Effect::READS_HEAP; layout Scalar; meaning WasmInstanceSlot, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmFillRegisters => Effect::PURE; layout Scalar; meaning ConstantIndex, @ Register, @ fields(Unused, RegisterWindowBase, RegisterCount),
    WasmSelect => Effect::PURE; layout RegisterPair, @ Register, @ fields(ResultRegister, Register, Register),
    WasmJumpI32Zero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Unused, Unused),
    WasmJumpI32NonZero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Unused, Unused),
    WasmBranchTable => Effect::CONTROL; layout Scalar; meaning BranchTableCases; flow BranchTable, @ Register, @ fields(Register, Unused, Unused),
    WasmJumpI32Equal => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32NotEqual => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessEqualSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessEqualUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterEqualSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterEqualUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32EqualImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32NotEqualImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessEqualSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessEqualUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterEqualSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterEqualUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmI32Unary => Effect::PURE; layout Scalar; meaning WasmI32UnaryOperator, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI64Binary => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmI64BinaryOperator, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Unary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmI64UnaryOperator, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmScalarConvert => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmScalarConversionOperator, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmF32Binary => Effect::PURE; layout Scalar; meaning WasmF32BinaryOperator, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF32Unary => Effect::PURE; layout Scalar; meaning WasmF32UnaryOperator, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmF64Binary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmF64BinaryOperator, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF64Unary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmF64UnaryOperator, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableInit => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmIndirectTarget => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmSignatureIndex, @ Register, @ fields(ResultRegister, Register, Register),
    WasmGlobalGet => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmGlobalSet => WRITE_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Unused),
    WasmRefFunc => CALL_EFFECT; layout Scalar; meaning WasmFunctionIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmRefIsNull => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableSize => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmTableSet => Effect::WRITES_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Register),
    WasmTableGrow => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout RegisterPair, @ Register, @ fields(ResultRegister, Register, Register),
    WasmTableFill => Effect::WRITES_HEAP.union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmTableCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmMemoryInit => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmMemoryCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmMemoryFill => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmMemoryAddress => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning ConstantIndex, @ Register, @ fields(ResultRegister, Register, Register),
    WasmMemoryLoad => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryLoadOperator, @ Register, @ fields(ResultRegister, Register, Register),
    WasmMemoryStore => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryStoreOperator, @ Register, @ fields(Register, Register, Register),
    WasmI32Load => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF32Load => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF64Load => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load8S => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load8U => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load16S => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load16U => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load8S => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load8U => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load16S => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load16U => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load32S => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load32U => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmI64Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmF32Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmF64Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmI32Store8 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmI32Store16 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmI64Store8 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmI64Store16 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmI64Store32 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(Register, Register, Register),
    WasmMemorySize => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmMemoryGrow => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmUnreachable => Effect::THROWS.union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal, @ Register, @ fields(Unused, Unused, Unused),
    WasmSimd => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmSimdOperator, @ Register, @ fields(ResultRegister, Register, Register),
    WasmSimdShuffle => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning ConstantIndex, @ Register, @ fields(ResultRegister, Register, Register),
    DefinePropertyRecord => CALL_EFFECT; layout Scalar; meaning PropertyDefinitionMode, @ Register, @ fields(Register, Register, Register),
    WasmRefAsNonNull => Effect::THROWS; layout Scalar; meaning WasmNonNullCheck, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI31 => Effect::THROWS; layout Scalar; meaning WasmI31Operator, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefTest => Effect::READS_HEAP; layout Scalar; meaning WasmReferenceTarget, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefCast => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefEq => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmStructNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmStructNewDefault => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmStructGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmStructFieldIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmStructGetS => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmStructFieldIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmStructGetU => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmStructFieldIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmStructSet => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmStructFieldIndex, @ Register, @ fields(Register, Register, Unused),

    WasmArrayNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayNewDefault => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmArrayNewFixed => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayLen => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),

    WasmArrayGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayGetS => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayGetU => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArraySet => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Register),
    WasmExternalConversion => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmExternalConversion, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmArrayFill => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmArrayCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmArrayNewData => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayNewElem => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayInitData => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmArrayInitElem => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),

    WasmExceptionNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmExceptionMatch => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmExceptionPayload => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmExceptionFieldIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmThrowRef => Effect::READS_HEAP.union(Effect::THROWS).union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal, @ Register, @ fields(Register, Unused, Unused),
    WasmStructNewDesc => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmStructNewDefaultDesc => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmRefGetDesc => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmDescriptorTest => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget, @ Register, @ fields(ResultRegister, Register, Register),
    WasmDescriptorCast => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget, @ Register, @ fields(ResultRegister, Register, Register),

    WasmAtomicAccess => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmAtomicOperator, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmAtomicFence => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused, @ Register, @ fields(Unused, Unused, Unused),
    SetFunctionDebugName => Effect::WRITES_HEAP; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, Unused),

);

const _: () = {
    let mut index = 0;
    while index < Op::COUNT {
        let fields = Op::OPERAND_LAYOUTS[index];
        let immediate_role = Op::IMMEDIATE_ROLES[index];
        let immediate_layout = Op::IMMEDIATE_LAYOUTS[index];
        assert!(!matches!(
            (immediate_role, immediate_layout),
            (ImmediateRole::LayoutEncoded, ImmediateLayout::Scalar)
        ));
        assert!(
            matches!(Op::RESULT_LAYOUTS[index], ResultLayout::Register)
                || matches!(fields.a, FieldLayout::ResultRegister)
        );
        if matches!(fields.a, FieldLayout::NumericLocalStoreMarker)
            || matches!(fields.b, FieldLayout::NumericLocalStoreMarker)
            || matches!(fields.c, FieldLayout::NumericLocalStoreMarker)
        {
            assert!(matches!(
                (fields.b, fields.c),
                (
                    FieldLayout::NumericLocalTarget,
                    FieldLayout::NumericLocalStoreMarker
                )
            ));
        }
        if matches!(fields.a, FieldLayout::ConstructArguments)
            || matches!(fields.b, FieldLayout::ConstructArguments)
            || matches!(fields.c, FieldLayout::ConstructArguments)
        {
            assert!(matches!(
                (fields.c, immediate_layout),
                (
                    FieldLayout::ConstructArguments,
                    ImmediateLayout::ConstructCountAndFlags
                )
            ));
        }
        if matches!(fields.a, FieldLayout::FieldLookupCacheSiteIndex)
            || matches!(fields.b, FieldLayout::FieldLookupCacheSiteIndex)
            || matches!(fields.c, FieldLayout::FieldLookupCacheSiteIndex)
        {
            assert!(matches!(
                (fields.b, fields.c, immediate_role),
                (
                    FieldLayout::FieldBase,
                    FieldLayout::FieldLookupCacheSiteIndex,
                    ImmediateRole::FieldLookup
                )
            ));
        }
        if matches!(fields.a, FieldLayout::WideIndexChunk)
            || matches!(fields.b, FieldLayout::WideIndexChunk)
            || matches!(fields.c, FieldLayout::WideIndexChunk)
        {
            assert!(matches!(
                (fields.a, fields.b, fields.c, immediate_role),
                (
                    FieldLayout::WideIndexChunk,
                    FieldLayout::WideIndexChunk,
                    FieldLayout::WideIndexChunk,
                    ImmediateRole::WideInstructionIndex
                )
            ));
        }
        index += 1;
    }
};

/// Entries after which a function's packed code is expanded into fixed-width instructions.
/// Decoding costs 1.5x the packed code's memory, so cold bootstrap and one-shot code keeps
/// decoding on the fly while repeatedly entered functions skip the per-dispatch unpacking.
/// A function with a loop is primed to expand on its first entry instead (`prime_loop`):
/// one entry can dispatch without bound.
const HOT_DECODE_ENTRIES: u32 = 64;

#[derive(Clone, Debug, Default)]
pub(crate) struct HotDecoding {
    entries: std::cell::Cell<u32>,
    // A thin pointer: every `Function` pays for this field, almost all stay cold.
    #[allow(clippy::box_collection)]
    code: std::cell::OnceCell<Box<Vec<WideInstruction>>>,
}

impl HotDecoding {
    /// Counts one entry and returns the expanded code once the function is hot.
    #[inline(always)]
    pub(crate) fn on_entry(
        &self,
        code: &[Instr],
        wide: &[WideInstruction],
    ) -> Option<&[WideInstruction]> {
        if let Some(decoded) = self.code.get() {
            return Some(decoded);
        }
        let entries = self.entries.get() + 1;
        self.entries.set(entries);
        if entries >= HOT_DECODE_ENTRIES {
            return Some(self.expand(code, wide));
        }
        None
    }

    /// Makes a function whose code loops expand on its first entry. Priming at installation
    /// leaves the entry path itself unchanged.
    pub(crate) fn prime_loop(&self, code: &[Instr], wide: &[WideInstruction]) {
        if self.entries.get() == 0 && has_backward_branch(code, wide) {
            self.entries.set(HOT_DECODE_ENTRIES - 1);
        }
    }

    #[cold]
    #[inline(never)]
    fn expand(&self, code: &[Instr], wide: &[WideInstruction]) -> &[WideInstruction] {
        self.code
            .get_or_init(|| {
                code.iter()
                    .map(|packed| {
                        if packed.is_wide() {
                            wide[packed.wide_index()]
                        } else {
                            packed.as_wide()
                        }
                    })
                    .collect::<Vec<_>>()
                    .into()
            })
            .as_slice()
    }
}

/// Whether some jump in `code` targets its own or an earlier instruction: a loop.
fn has_backward_branch(code: &[Instr], wide: &[WideInstruction]) -> bool {
    code.iter().enumerate().any(|(pc, packed)| {
        let instruction = if packed.is_wide() {
            wide[packed.wide_index()]
        } else {
            packed.as_wide()
        };
        matches!(
            instruction.op().control_flow_layout(),
            ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump
        ) && instruction.jump_target() as usize <= pc
    })
}

/// A byte range of a program's source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub parent: Option<u32>,
    pub name: Option<Atom>,
    pub is_arrow: bool,
    pub self_binding_slot: Option<u16>,
    /// The function's source text as a range of the program's `source`, which
    /// `Function.prototype.toString` reads on demand.
    pub source_span: Option<SourceSpan>,
    pub params: u16,
    pub length: u16,
    pub parameter_end_pc: u32,
    pub parameter_atoms: Vec<Atom>,
    pub rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_class_constructor: bool,
    pub derived_constructor: bool,
    /// Instance-element initialization run when this derived constructor binds `this`.
    pub instance_initializer: Option<u32>,
    /// Lexical HomeObject captured by methods and arrows containing `super`.
    pub super_home_atom: Option<Atom>,
    pub constructible: bool,
    pub class_initializer: bool,
    pub parameter_eval_arguments_error: bool,
    pub arguments_slot: Option<u16>,
    pub simple_parameters: bool,
    pub strict: bool,
    pub locals: u16,
    pub local_atoms: Vec<Atom>,
    /// Names visible through this function's captured declarative environment.
    pub environment_atoms: Vec<Atom>,
    /// Source local slots owned by a closure environment when this activation
    /// escapes. `None` retains legacy whole-frame promotion; `Some` is the
    /// selective layout, including an empty set for closure-only activations.
    pub(crate) selective_capture_slots: Option<Vec<u16>>,
    /// Binding slots assigned stable frame registers by lowering.
    pub(crate) local_registers: Vec<LocalRegister>,
    /// The function was created while an enclosing `with` scope was active,
    /// so unresolved names in descendants can still observe its outer locals.
    pub(crate) inherited_with_scope: bool,
    pub lexical_atoms: Vec<Atom>,
    pub global_lexical_atoms: Vec<Atom>,
    pub global_var_atoms: Vec<Atom>,
    pub global_function_atoms: Vec<Atom>,
    pub global_annex_b_var_atoms: Vec<Atom>,
    pub global_immutable_atoms: Vec<Atom>,
    /// Bindings a dynamic name lookup in this function can see.
    pub(crate) name_bindings: NameBindings,
    /// Lexical projections at eval calls and dynamic name operations.
    pub binding_sites: Vec<BindingSite>,
    pub(crate) source_positions: Vec<SourcePosition>,
    /// The plan of each CloneEnv instruction.
    pub environment_clones: Vec<EnvironmentClone>,
    pub code: Vec<Instr>,
    pub(crate) wide: Vec<WideInstruction>,
    pub registers: u16,
    pub(crate) dispatch: DispatchClass,
    /// Fixed-width view of `code`, derived once the function proves hot.
    pub(crate) decoded: HotDecoding,
    /// Lazily derived `plain_local_slots`; never serialized.
    pub(crate) plain_locals: std::cell::OnceCell<Box<[bool]>>,
    /// Lazily derived `activation_bindings_observable`; never serialized.
    pub(crate) activation_bindings: std::cell::OnceCell<bool>,
    /// Lazily derived `reflects_live_parameters`; never serialized.
    pub(crate) live_parameter_reflection: std::cell::OnceCell<bool>,
    /// Lazily derived `names_resolve_statically`; never serialized.
    pub(crate) static_name_scope: std::cell::OnceCell<bool>,
    /// Lazily derived `call_plan`; never serialized.
    pub(crate) call_plan: std::cell::OnceCell<CallPlan>,
    pub(crate) handlers: Vec<Handler>,
    pub(crate) register_root_offset: u32,
    /// A register that holds `undefined` for the whole activation, set at frame entry and
    /// never written by code: the receiver of plain calls.
    pub(crate) undefined_register: Option<Register>,
    /// First register of the parameters when the body keeps its locals in
    /// registers; activation then writes arguments there, not into locals.
    pub(crate) parameter_registers: Option<u16>,
    /// The value every register holds when an activation starts.
    pub(crate) initial_register: InitialRegister,
}

/// Call-setup facts derived once per function from its static layout, so an activation reads
/// one cached word instead of re-testing each flag.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CallPlan {
    /// `plain_activation`.
    pub(crate) plain: bool,
    /// The dispatch loop may push the activation in place: an ordinary body that is no
    /// generator, async function, class constructor or field initializer and raises no
    /// parameter-scope `arguments` error.
    pub(crate) in_loop: bool,
    /// Legacy `caller`/`arguments` reflection may observe the activation
    /// (`!has_restricted_legacy_caller_access`).
    pub(crate) legacy_reflection: bool,
    /// `reflects_live_parameters`.
    pub(crate) live_parameters: bool,
    /// Some instruction can reach the frame's local slots: one names a slot
    /// (`addressed_local_slots`), creates a closure or clones an environment. A plain
    /// activation without that needs no local storage, since promoted bindings live in
    /// registers and nothing else reads its locals.
    pub(crate) addresses_locals: bool,
    /// The activation passes its `arguments` binding only straight into
    /// `CallForwardingArguments` calls, so it keeps the argument values instead of an
    /// `arguments` object until a call needs one (`Function::forwards_arguments_only`).
    pub(crate) forwards_arguments: bool,
    /// The activation's whole state is its registers, plus the values it was passed when it
    /// forwards them: it reaches no local but a forwarded `arguments` binding, and legacy
    /// reflection, if any, reads its parameter registers or the forwarded values. A lane
    /// may run such a body without a frame.
    pub(crate) registers_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalRegister {
    pub(crate) local: u16,
    pub(crate) register: Register,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourcePosition {
    pub(crate) pc: u32,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlainLocalContextIneligibility {
    RootFunction,
    AsyncFunction,
    GeneratorFunction,
    ClassConstructor,
    ClassInitializer,
    NonSimpleParameters,
    LegacyCaptureLayout,
    DynamicNameResolution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlainLocalSlotIneligibility {
    MissingAtom,
    MappedArgument,
    Tdz,
    SelfBinding,
    Captured,
    HiddenName,
}

impl Function {
    /// Reusing an activation is observable while its call is protected by a handler.
    /// A terminal return may skip its following Return instruction without being a
    /// proper tail call: the caller must still catch the callee's abrupt completion.
    pub(crate) fn permits_js_tail_call_at(&self, pc: u32) -> bool {
        self.strict && !self.class_initializer && self.handler_at(pc).is_none()
    }

    pub(crate) fn handler_at(&self, pc: u32) -> Option<Handler> {
        self.handlers
            .iter()
            .find(|handler| pc >= handler.start && pc < handler.end)
            .copied()
    }

    pub(crate) fn promoted_register(&self, local: u16) -> Option<Register> {
        self.local_registers
            .binary_search_by_key(&local, |entry| entry.local)
            .ok()
            .map(|index| self.local_registers[index].register)
    }

    /// Whether the instruction at `pc` returns `register`. A call is in tail position only
    /// when the instruction after it returns that call's own result.
    pub(crate) fn has_restricted_legacy_caller_access(&self) -> bool {
        self.strict || !self.constructible || self.is_class_constructor
    }

    pub(crate) fn arguments_are_mapped(&self) -> bool {
        !self.is_arrow && !self.strict && self.simple_parameters
    }

    /// Parameters an activation binds into frame locals.
    pub(crate) const fn local_parameter_count(&self) -> u16 {
        if self.parameter_registers.is_some() {
            0
        } else {
            self.params
        }
    }

    pub(crate) fn plain_local_context_is_safe(&self) -> bool {
        self.plain_local_context_ineligibility().is_none()
    }

    pub(crate) fn plain_local_context_ineligibility(
        &self,
    ) -> Option<PlainLocalContextIneligibility> {
        if self.parent.is_none() {
            return Some(PlainLocalContextIneligibility::RootFunction);
        }
        if self.is_async {
            return Some(PlainLocalContextIneligibility::AsyncFunction);
        }
        if self.is_generator {
            return Some(PlainLocalContextIneligibility::GeneratorFunction);
        }
        if self.is_class_constructor {
            return Some(PlainLocalContextIneligibility::ClassConstructor);
        }
        if self.class_initializer {
            return Some(PlainLocalContextIneligibility::ClassInitializer);
        }
        if !self.simple_parameters {
            return Some(PlainLocalContextIneligibility::NonSimpleParameters);
        }
        let selective = self.selective_capture_slots.is_some();
        let has_legacy_environment_access = self.code.iter().any(|instruction| {
            instruction.op() == Op::MakeClosure && !selective
                || matches!(instruction.op(), Op::LoadEnvLocal | Op::StoreEnvLocal) && !selective
        }) || self.wide.iter().any(|instruction| {
            instruction.op() == Op::MakeClosure && !selective
                || matches!(instruction.op(), Op::LoadEnvLocal | Op::StoreEnvLocal) && !selective
        });
        if has_legacy_environment_access {
            return Some(PlainLocalContextIneligibility::LegacyCaptureLayout);
        }
        let has_dynamic_name_resolution = self.code.iter().any(|instruction| {
            instruction.op() == Op::ResolveName
                || instruction.op().is_call_with_eval_flags() && instruction.direct_eval()
        }) || self.wide.iter().any(|instruction| {
            instruction.op() == Op::ResolveName
                || instruction.op().is_call_with_eval_flags()
                    && ImmediateLayout::direct_eval(instruction.imm())
        });
        has_dynamic_name_resolution.then_some(PlainLocalContextIneligibility::DynamicNameResolution)
    }

    /// Which local slots are plain: read and written only through the frame's local array,
    /// with no environment, TDZ, mapped-argument or dynamic-scope observer. Compilation
    /// specializes these slots' operations, validation re-derives the same proof, and the
    /// interpreter memoizes it in `plain_locals` to read plain local operands directly.
    pub(crate) fn plain_local_slots<'a>(
        &self,
        atom_name: impl Fn(Atom) -> Option<&'a str>,
    ) -> Vec<bool> {
        let mut plain = vec![false; usize::from(self.locals)];
        if !self.plain_local_context_is_safe() {
            return plain;
        }
        let mut tdz_slots = vec![false; usize::from(self.locals)];
        for instruction in &self.code {
            if instruction.op() == Op::InitializeTdz
                && let Some(slot) = tdz_slots.get_mut(instruction.local_slot())
            {
                *slot = true;
            }
        }
        for instruction in &self.wide {
            if instruction.op() == Op::InitializeTdz
                && let Some(slot) = tdz_slots.get_mut(instruction.local_slot())
            {
                *slot = true;
            }
        }
        for (slot, plain) in plain.iter_mut().enumerate() {
            *plain = self
                .local_atoms
                .get(slot)
                .and_then(|atom| atom_name(*atom))
                .is_some_and(|name| self.plain_local_slot_is_safe(slot, name, tdz_slots[slot]));
        }
        plain
    }

    /// Slots whose uninitialized state no access can observe: in a plain-local context, each
    /// slot that is plain but for its TDZ, and whose every access other than a TDZ reset
    /// (`InitializeTdz`) or an initializing store runs, on every normal and exceptional path
    /// from entry, only after an initializing store with no reset since. Resetting such a
    /// slot to its uninitialized hole then changes nothing any access sees.
    pub(crate) fn unobservable_tdz_slots<'a>(
        &self,
        atom_name: impl Fn(Atom) -> Option<&'a str>,
    ) -> Vec<bool> {
        let locals = usize::from(self.locals);
        let mut candidates = vec![false; locals];
        if !self.plain_local_context_is_safe() {
            return candidates;
        }
        let decoded = self.instructions().collect::<Vec<_>>();
        for instruction in &decoded {
            if instruction.op() == Op::InitializeTdz
                && let Some(candidate) = candidates.get_mut(instruction.local_slot())
            {
                *candidate = true;
            }
        }
        for (slot, candidate) in candidates.iter_mut().enumerate() {
            *candidate = *candidate
                && self
                    .local_atoms
                    .get(slot)
                    .and_then(|atom| atom_name(*atom))
                    .is_some_and(|name| self.plain_local_slot_is_safe(slot, name, false));
        }
        if !candidates.contains(&true) {
            return candidates;
        }
        let initializing = |instruction: WideInstruction| {
            matches!(instruction.op(), Op::StoreLocal | Op::StoreLocalPlain)
                && instruction.boolean_field(InstructionField::C) == Some(true)
        };
        // Must-initialized slots on entry to each instruction. An instruction no path reaches
        // keeps every slot (it never runs); `visited` makes each reached one transfer once.
        let words = locals.div_ceil(u64::BITS as usize);
        let len = decoded.len();
        let mut entry = vec![vec![u64::MAX; words]; len + 1];
        entry[0].fill(0);
        let mut visited = vec![false; len + 1];
        let mut work = vec![0];
        while let Some(pc) = work.pop() {
            if pc >= len {
                continue;
            }
            visited[pc] = true;
            let instruction = decoded[pc];
            let before = entry[pc].clone();
            let mut after = before.clone();
            if instruction.op() == Op::InitializeTdz || initializing(instruction) {
                let slot = instruction.local_slot();
                let (word, bit) = (
                    slot / u64::BITS as usize,
                    1u64 << (slot % u64::BITS as usize),
                );
                if instruction.op() == Op::InitializeTdz {
                    after[word] &= !bit;
                } else {
                    after[word] |= bit;
                }
            }
            let mut meet = |target: usize, state: &[u64]| {
                let Some(current) = entry.get_mut(target) else {
                    return;
                };
                let mut changed = false;
                for (word, incoming) in current.iter_mut().zip(state) {
                    let next = *word & incoming;
                    changed |= next != *word;
                    *word = next;
                }
                if changed || !visited[target] {
                    visited[target] = true;
                    work.push(target);
                }
            };
            match instruction.op().control_flow_layout() {
                ControlFlowLayout::Jump => meet(instruction.jump_target() as usize, &after),
                ControlFlowLayout::ConditionalJump => {
                    meet(pc + 1, &after);
                    meet(instruction.jump_target() as usize, &after);
                }
                ControlFlowLayout::BranchTable => {
                    for target in branch_table_entries(pc, instruction) {
                        meet(target, &after);
                    }
                }
                ControlFlowLayout::Terminal => {}
                ControlFlowLayout::Call | ControlFlowLayout::Fallthrough
                    if instruction.returns_from_frame() => {}
                ControlFlowLayout::Call | ControlFlowLayout::Fallthrough => meet(pc + 1, &after),
            }
            for handler in &self.handlers {
                if pc as u32 >= handler.start && (pc as u32) < handler.end {
                    meet(handler.target as usize, &before);
                    if let Some(target) = handler.return_target {
                        meet(target as usize, &before);
                    }
                }
            }
        }
        for (pc, instruction) in decoded.iter().enumerate() {
            if instruction.op().immediate_role() != ImmediateRole::LocalSlot
                || instruction.op() == Op::InitializeTdz
                || initializing(*instruction)
            {
                continue;
            }
            let slot = instruction.local_slot();
            let (word, bit) = (
                slot / u64::BITS as usize,
                1u64 << (slot % u64::BITS as usize),
            );
            if entry[pc][word] & bit == 0
                && let Some(candidate) = candidates.get_mut(slot)
            {
                *candidate = false;
            }
        }
        candidates
    }

    /// Whether anything can read an activation's implicit `new.target` and lexical-this
    /// bindings: by-name resolution or direct eval in the body, closures or cloned scopes
    /// that can capture the activation, `with` scopes, and constructors, initializers,
    /// generators or async bodies whose `this` or activation outlives ordinary dispatch.
    /// Otherwise the frame's own `this` is authoritative and the bindings are never read.
    /// By-name loads of source names resolve only their own atom; the implicit `this` and
    /// `new.target` bindings are reachable by name only through engine-internal atoms.
    pub(crate) fn activation_bindings_observable(&self, atoms: &AtomTable) -> bool {
        *self.activation_bindings.get_or_init(|| {
            self.is_async
                || self.is_generator
                || self.is_class_constructor
                || self.derived_constructor
                || self.class_initializer
                || self.instance_initializer.is_some()
                || self.inherited_with_scope
                || !self.binding_sites.is_empty()
                || self.instructions().any(|instruction| {
                    matches!(
                        instruction.op(),
                        Op::LoadName | Op::LoadNameCall | Op::LoadNameTypeof
                    ) && usize::try_from(instruction.atom_index())
                        .is_ok_and(|atom| atom >= atoms.len() || atoms[atom].starts_with('\0'))
                        || matches!(
                            instruction.op(),
                            Op::ResolveName
                                | Op::LoadResolvedName
                                | Op::DeleteName
                                | Op::StoreName
                                | Op::StoreResolvedName
                                | Op::StoreVarBinding
                                | Op::MakeClosure
                                | Op::CloneEnv
                                | Op::InitializeThis
                                | Op::SuperCallCheck
                        )
                        || (instruction.op().is_call_with_eval_flags()
                            && (instruction.direct_eval() || instruction.parameter_eval()))
                })
        })
    }

    /// Every instruction in fixed-width form.
    pub(crate) fn instructions(&self) -> impl Iterator<Item = WideInstruction> + '_ {
        self.code.iter().map(|packed| {
            if packed.is_wide() {
                self.wide[packed.wide_index()]
            } else {
                packed.as_wide()
            }
        })
    }

    /// Whether a closure environment may hold local `slot`: the selective layout names it or,
    /// without one, the function captures its whole frame by creating closures or reaching its
    /// environment.
    fn local_slot_captured(&self, slot: usize) -> bool {
        match &self.selective_capture_slots {
            Some(captured) => {
                u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
            }
            None => self.instructions().any(|instruction| {
                matches!(
                    instruction.op(),
                    Op::MakeClosure | Op::LoadEnvLocal | Op::StoreEnvLocal
                )
            }),
        }
    }

    /// Whether legacy `f.arguments` reads each passed parameter's current frame value, as V8
    /// does for parameters that live only in the frame: no code or closure uses an `arguments`
    /// binding, the parameters are simple, and each is a plain or register-promoted local.
    /// Otherwise the arguments are reflected as passed.
    pub(crate) fn reflects_live_parameters(&self, atoms: &AtomTable) -> bool {
        *self.live_parameter_reflection.get_or_init(|| {
            let arguments_used = self.local_atoms.iter().enumerate().any(|(slot, &atom)| {
                usize::try_from(atom)
                    .is_ok_and(|atom| atom < atoms.len() && &atoms[atom] == "arguments")
                    && (self.local_slot_captured(slot)
                        || self.instructions().any(|instruction| {
                            instruction.op().immediate_role() == ImmediateRole::LocalSlot
                                && instruction.local_slot() == slot
                        }))
            });
            self.arguments_slot.is_none()
                && !arguments_used
                && self.simple_parameters
                && !self.rest
                && (0..self.params).all(|parameter| {
                    self.plain_local(atoms, usize::from(parameter))
                        || self
                            .local_registers
                            .iter()
                            .any(|entry| entry.local == parameter)
                })
        })
    }

    /// Whether an activation is fully described by the function's static layout: a nested,
    /// non-arrow, non-derived function whose parameters all live in registers, with no rest
    /// array, self binding, `arguments` object, parameter-scope `arguments` error, inherited
    /// `with` scope or observable implicit bindings. Such a frame needs only its storage sized
    /// (cleared when no root map lets the collector skip stale registers), its parameters
    /// placed and its scalar state set; see `activate_plain_frame`.
    pub(crate) fn plain_activation(&self, atoms: &AtomTable) -> bool {
        self.call_plan(atoms).plain
    }

    /// The function's `CallPlan`, derived on first use.
    #[inline(always)]
    pub(crate) fn call_plan(&self, atoms: &AtomTable) -> CallPlan {
        *self.call_plan.get_or_init(|| {
            let promoted_parameters = self
                .local_registers
                .iter()
                .filter(|entry| entry.local < self.params)
                .count();
            // Plain but for an `arguments` binding.
            let ordinary = self.parent.is_some()
                && !self.is_arrow
                && !self.derived_constructor
                && !self.rest
                && self.self_binding_slot.is_none()
                && !self.parameter_eval_arguments_error
                && !self.inherited_with_scope
                && !self.is_generator
                && !self.is_async
                && promoted_parameters == usize::from(self.params)
                && !self.activation_bindings_observable(atoms);
            let in_loop = !self.is_async
                && !self.is_generator
                && !self.is_class_constructor
                && !self.derived_constructor
                && !self.class_initializer
                && !self.parameter_eval_arguments_error;
            let legacy_reflection = !self.has_restricted_legacy_caller_access();
            let live_parameters = self.reflects_live_parameters(atoms);
            let forwards_arguments = self.forwards_arguments_only();
            let addresses_locals = self.instructions().any(|instruction| {
                matches!(instruction.op(), Op::MakeClosure | Op::CloneEnv)
                    || instruction.addressed_local_slots().next().is_some()
            });
            let reaches_only_forwarded_arguments = forwards_arguments
                && self.instructions().all(|instruction| {
                    instruction
                        .addressed_local_slots()
                        .all(|slot| Some(slot) == self.arguments_slot)
                });
            CallPlan {
                plain: ordinary && self.arguments_slot.is_none(),
                in_loop,
                legacy_reflection,
                live_parameters,
                addresses_locals,
                forwards_arguments,
                registers_only: ordinary
                    && in_loop
                    && self.initial_register == InitialRegister::Undefined
                    && if self.arguments_slot.is_none() {
                        !addresses_locals && (!legacy_reflection || live_parameters)
                    } else {
                        reaches_only_forwarded_arguments
                    },
            }
        })
    }

    /// Whether every read of the `arguments` binding loads it into the last argument of the
    /// `CallForwardingArguments` right after, and nothing else can reach the binding: no
    /// closure, environment clone, dynamic scope access or promoted register, and no
    /// suspension. An object it later materializes must equal one made on entry: unmapped,
    /// or mapped onto no parameters.
    pub(crate) fn forwards_arguments_only(&self) -> bool {
        let Some(slot) = self.arguments_slot else {
            return false;
        };
        if self.is_generator
            || self.is_async
            || (self.arguments_are_mapped() && self.params != 0)
            || self.local_registers.iter().any(|entry| entry.local == slot)
            || self.has_dynamic_scope_access()
        {
            return false;
        }
        let instructions = self.instructions().collect::<Vec<_>>();
        let mut forwarded = false;
        for (pc, instruction) in instructions.iter().enumerate() {
            if matches!(instruction.op(), Op::MakeClosure | Op::CloneEnv) {
                return false;
            }
            if !instruction
                .addressed_local_slots()
                .any(|addressed| addressed == slot)
            {
                continue;
            }
            let feeds_forwarding_call =
                matches!(instruction.op(), Op::LoadLocal | Op::LoadLocalPlain)
                    && instructions.get(pc + 1).is_some_and(|next| {
                        next.op() == Op::CallForwardingArguments
                            && next.call_window().last() == Some(instruction.result_register())
                    });
            if !feeds_forwarding_call {
                return false;
            }
            forwarded = true;
        }
        forwarded
    }

    /// Whether the function itself can reach bindings by name at run time: it was created
    /// inside a `with` scope, records by-name lexical sites, resolves names dynamically, or
    /// calls direct eval (also from a parameter scope).
    pub(crate) fn has_dynamic_scope_access(&self) -> bool {
        self.inherited_with_scope
            || !self.binding_sites.is_empty()
            || self.instructions().any(|instruction| {
                instruction.op() == Op::ResolveName
                    || (instruction.op().is_call_with_eval_flags()
                        && (ImmediateLayout::direct_eval(instruction.imm())
                            || ImmediateLayout::parameter_eval(instruction.imm())))
            })
    }

    /// Whether the body enters a `with` statement, whose by-name loads may denote
    /// any enclosing function's binding. `is_with_enter` recognizes the atom id of
    /// [`WITH_ENTER_BINDING`] in the owning program's atom table.
    pub(crate) fn contains_with_statement(&self, is_with_enter: impl Fn(usize) -> bool) -> bool {
        self.instructions().any(|instruction| {
            matches!(instruction.op(), Op::LoadName | Op::LoadNameCall)
                && usize::try_from(instruction.atom_index()).is_ok_and(&is_with_enter)
        })
    }

    /// Whether neither this function nor any enclosing one has dynamic scope access or
    /// contains a `with` statement. The compiler then resolved every lexical binding the body
    /// names, so a by-name load in it can only denote a global binding.
    pub(crate) fn names_resolve_statically(
        &self,
        functions: &[Function],
        atoms: &AtomTable,
    ) -> bool {
        *self.static_name_scope.get_or_init(|| {
            !self.has_dynamic_scope_access()
                && !self.contains_with_statement(|atom| {
                    atom < atoms.len() && &atoms[atom] == WITH_ENTER_BINDING
                })
                && self.parent.is_none_or(|parent| {
                    functions
                        .get(parent as usize)
                        .is_some_and(|parent| parent.names_resolve_statically(functions, atoms))
                })
        })
    }

    /// The memoized `plain_local_slots` projection for this residual's atoms.
    #[inline(always)]
    pub(crate) fn plain_local(&self, atoms: &AtomTable, slot: usize) -> bool {
        self.plain_locals
            .get_or_init(|| {
                self.plain_local_slots(|atom| {
                    usize::try_from(atom)
                        .ok()
                        .filter(|atom| *atom < atoms.len())
                        .map(|atom| &atoms[atom])
                })
                .into_boxed_slice()
            })
            .get(slot)
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn plain_local_slot_is_safe(
        &self,
        slot: usize,
        atom_name: &str,
        has_tdz: bool,
    ) -> bool {
        self.plain_local_slot_ineligibility(slot, atom_name, has_tdz)
            .is_none()
    }

    pub(crate) fn plain_local_slot_ineligibility(
        &self,
        slot: usize,
        atom_name: &str,
        has_tdz: bool,
    ) -> Option<PlainLocalSlotIneligibility> {
        if self.local_atoms.get(slot).is_none() {
            return Some(PlainLocalSlotIneligibility::MissingAtom);
        }
        if has_tdz {
            return Some(PlainLocalSlotIneligibility::Tdz);
        }
        if self.is_self_binding_slot(slot) {
            return Some(PlainLocalSlotIneligibility::SelfBinding);
        }
        if self
            .selective_capture_slots
            .as_ref()
            .is_some_and(|captured| {
                u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
            })
        {
            return Some(PlainLocalSlotIneligibility::Captured);
        }
        if slot < usize::from(self.params)
            && self.arguments_slot.is_some()
            && self.arguments_are_mapped()
        {
            return Some(PlainLocalSlotIneligibility::MappedArgument);
        }
        if local_name_requires_environment(atom_name) {
            return Some(PlainLocalSlotIneligibility::HiddenName);
        }
        None
    }

    pub(crate) fn is_self_binding_slot(&self, slot: usize) -> bool {
        self.self_binding_slot
            .is_some_and(|self_slot| usize::from(self_slot) == slot)
    }

    pub(crate) fn local_slot_uses_environment(&self, slot: usize) -> bool {
        self.selective_capture_slots
            .as_ref()
            .is_none_or(|captured| {
                u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
            })
    }
}

/// Scoped binding projections at an instruction resume PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BindingSite {
    pub(crate) resume_pc: u32,
    pub(crate) bindings: Vec<EvalBinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EvalBinding {
    pub(crate) atom: Atom,
    pub(crate) location: EvalBindingLocation,
    /// Active with scopes outside this declaration, relative to its activation.
    pub(crate) with_depth: u16,
    pub(crate) kind: LexicalBindingKind,
    pub(crate) declaration: EvalBindingDeclaration,
}

impl EvalBinding {
    pub(crate) fn conflicts_with_var(self) -> bool {
        matches!(self.location, EvalBindingLocation::Local(_))
            && self.declaration == EvalBindingDeclaration::Lexical
            && self.kind != LexicalBindingKind::FunctionName
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum EvalBindingDeclaration {
    Lexical = 0,
    CatchParameter = 1,
    Variable = 2,
}

impl EvalBindingDeclaration {
    pub(crate) fn from_binary_tag(tag: u8) -> Result<Self, String> {
        [Self::Lexical, Self::CatchParameter, Self::Variable]
            .into_iter()
            .find(|declaration| *declaration as u8 == tag)
            .ok_or_else(|| "invalid eval binding declaration".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum EvalBindingLocation {
    Local(u16),
    Capture { depth: u16, slot: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum LexicalBindingKind {
    Mutable = 0,
    Immutable = 1,
    FunctionName = 2,
    PrivateName = 3,
}

impl LexicalBindingKind {
    pub(crate) const ALL: [Self; 4] = [
        Self::Mutable,
        Self::Immutable,
        Self::FunctionName,
        Self::PrivateName,
    ];

    pub(crate) fn capture_prefix(self) -> &'static str {
        match self {
            Self::Mutable => "\0quench:lexical-capture:",
            Self::Immutable => "\0quench:immutable-capture:",
            Self::FunctionName => "\0quench:function-name-capture:",
            Self::PrivateName => "\0quench:private-name-capture:",
        }
    }

    pub(crate) fn from_binary_tag(tag: u8) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|kind| *kind as u8 == tag)
            .ok_or_else(|| "invalid lexical binding kind".into())
    }
}

pub(crate) fn private_identity_label(identity: &str) -> Option<String> {
    let identity = identity.strip_prefix("\0quench:private:")?;
    let (_, label) = identity.rsplit_once(':')?;
    (!label.is_empty()).then(|| label.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum DispatchClass {
    General,
    Numeric,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MethodSite {
    pub atom: Atom,
    pub cache: u16,
    pub argument_start: u32,
    pub argument_count: u16,
    pub receiver_path: Option<(Atom, u16)>,
}

#[derive(Clone, Debug)]
pub(crate) struct ObjectSite {
    pub atoms: SmallVec<[Atom; INLINE_OBJECT_SITE_ATOMS]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegExpLiteralSite {
    pub pattern_constant: u32,
    pub flags_constant: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Superinstruction {
    pub code: [Instr; 4],
    /// `code` expanded once for dispatch, which reads every operand of every part.
    pub wide: [WideInstruction; 4],
}

impl Superinstruction {
    pub(crate) fn new(code: [Instr; 4]) -> Self {
        Self {
            code,
            wide: code.map(Instr::as_wide),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Handler {
    pub start: u32,
    pub end: u32,
    pub target: u32,
    pub slot: Option<u16>,
    pub return_target: Option<u32>,
    pub return_slot: Option<u16>,
    pub with_depth: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct FieldBase(pub(crate) u16);

impl FieldBase {
    pub(crate) const THIS: Self = Self(u16::MAX);
    pub(crate) const NESTED: u16 = u16::MAX - 1;

    pub(crate) fn register(value: Register) -> Self {
        debug_assert!(value < Self::NESTED);
        Self(value)
    }

    pub(crate) fn register_index(self) -> Option<Register> {
        (self != Self::THIS).then_some(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldPart {
    First,
    Second,
    Sink,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldSite {
    pub base: FieldBase,
    pub first: (Atom, u16),
    pub second: Option<(Atom, u16)>,
    pub sink: Option<(Atom, u16)>,
    pub sources: [Option<(u32, u32)>; 3],
}
impl FieldSite {
    pub(crate) fn source(self, part: FieldPart) -> Option<(u32, u32)> {
        self.sources[match part {
            FieldPart::First => 0,
            FieldPart::Second => 1,
            FieldPart::Sink => 2,
        }]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct Operand(pub(crate) u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum OperandKind {
    Register = 0,
    Constant = 1,
    Field = 2,
    Local = 3,
}
#[cfg(feature = "profile-aggregate")]
impl OperandKind {
    pub(crate) const COUNT: usize = Self::Local as usize + 1;
}

impl Operand {
    const TAG_SHIFT: u16 = 14;
    const PAYLOAD_MASK: u16 = (1 << Self::TAG_SHIFT) - 1;

    pub(crate) const fn kind(self) -> Option<OperandKind> {
        match self.0 >> Self::TAG_SHIFT {
            tag if tag == OperandKind::Register as u16 => Some(OperandKind::Register),
            tag if tag == OperandKind::Constant as u16 => Some(OperandKind::Constant),
            tag if tag == OperandKind::Field as u16 => Some(OperandKind::Field),
            tag if tag == OperandKind::Local as u16 => Some(OperandKind::Local),
            _ => None,
        }
    }

    pub(crate) fn register(value: Register) -> Self {
        debug_assert!(value <= Self::PAYLOAD_MASK);
        Self(((OperandKind::Register as u16) << Self::TAG_SHIFT) | value)
    }

    /// A constant-pool operand, when the pool index fits the operand payload;
    /// larger programs load the constant into a register instead.
    pub(crate) fn constant(index: u32) -> Option<Self> {
        u16::try_from(index)
            .ok()
            .filter(|index| *index <= Self::PAYLOAD_MASK)
            .map(|index| Self(((OperandKind::Constant as u16) << Self::TAG_SHIFT) | index))
    }

    pub(crate) fn local(slot: u16) -> Self {
        debug_assert!(slot <= Self::PAYLOAD_MASK);
        Self(((OperandKind::Local as u16) << Self::TAG_SHIFT) | slot)
    }

    pub(crate) fn tag(self) -> u16 {
        self.0 >> Self::TAG_SHIFT
    }

    pub(crate) fn payload(self) -> u16 {
        self.0 & Self::PAYLOAD_MASK
    }

    pub(crate) fn register_index(self) -> Option<Register> {
        (self.kind() == Some(OperandKind::Register)).then_some(self.payload())
    }
}

/// A CloneEnv plan. The clone owns the `fresh` slots, copied from the source
/// environment, and the `reset` slots, cleared: block bindings out of scope at
/// the clone's site, whose next scope entry initializes them again. Every other
/// slot is shared with its owner. Both lists are sorted local slots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentClone {
    pub fresh: Vec<u16>,
    pub reset: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct ModuleRequest {
    pub source: String,
    pub phase: ModuleRequestPhase,
    pub module_type: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ModuleImportBinding {
    pub source: String,
    pub phase: ModuleRequestPhase,
    pub module_type: Option<String>,
    pub imported: ModuleImportName,
    pub local: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ModuleLinkPlan {
    pub locals: Vec<(String, String)>,
    pub reexports: Vec<ModuleReexport>,
    pub hoisted_functions: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub(crate) enum ModuleReexport {
    Named {
        source: String,
        imported: String,
        exported: String,
    },
    Star {
        source: String,
    },
    Namespace {
        source: String,
        exported: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ModuleReexportKind {
    Named,
    Star,
    Namespace,
}

impl ModuleReexportKind {
    const NAMED_BINARY_TAG: u8 = 0;
    const STAR_BINARY_TAG: u8 = 1;
    const NAMESPACE_BINARY_TAG: u8 = 2;

    pub(crate) const fn binary_tag(self) -> u8 {
        match self {
            Self::Named => Self::NAMED_BINARY_TAG,
            Self::Star => Self::STAR_BINARY_TAG,
            Self::Namespace => Self::NAMESPACE_BINARY_TAG,
        }
    }

    pub(crate) const fn from_binary_tag(tag: u8) -> Option<Self> {
        match tag {
            Self::NAMED_BINARY_TAG => Some(Self::Named),
            Self::STAR_BINARY_TAG => Some(Self::Star),
            Self::NAMESPACE_BINARY_TAG => Some(Self::Namespace),
            _ => None,
        }
    }
}

impl ModuleReexport {
    pub(crate) const fn kind(&self) -> ModuleReexportKind {
        match self {
            Self::Named { .. } => ModuleReexportKind::Named,
            Self::Star { .. } => ModuleReexportKind::Star,
            Self::Namespace { .. } => ModuleReexportKind::Namespace,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ModuleImportName {
    Namespace,
    Named(String),
}

impl ModuleImportName {
    pub(crate) const fn binary_tag(&self) -> u8 {
        match self {
            Self::Namespace => ModuleImportNameKind::Namespace.binary_tag(),
            Self::Named(_) => ModuleImportNameKind::Named.binary_tag(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ModuleImportNameKind {
    Namespace,
    Named,
}

impl ModuleImportNameKind {
    const fn binary_tag(self) -> u8 {
        match self {
            Self::Namespace => 0,
            Self::Named => 1,
        }
    }

    pub(crate) const fn from_binary_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Namespace),
            1 => Some(Self::Named),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleRequestPhase {
    Evaluation,
    Defer,
    Source,
}

impl ModuleRequestPhase {
    pub(crate) const fn binary_tag(self) -> u8 {
        match self {
            Self::Evaluation => 0,
            Self::Defer => 1,
            Self::Source => 2,
        }
    }

    pub(crate) const fn from_binary_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Evaluation),
            1 => Some(Self::Defer),
            2 => Some(Self::Source),
            _ => None,
        }
    }

    pub(crate) const fn runtime_value(self) -> f64 {
        self.binary_tag() as f64
    }

    pub(crate) fn from_runtime_value(value: f64) -> Option<Self> {
        [Self::Evaluation, Self::Defer, Self::Source]
            .into_iter()
            .find(|phase| phase.runtime_value() == value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProgramKind {
    Script,
    Module,
    Eval,
    Wasm,
}

impl ProgramKind {
    pub(crate) const fn binary_tag(self) -> u8 {
        match self {
            Self::Script => 0,
            Self::Module => 1,
            Self::Eval => 2,
            Self::Wasm => 3,
        }
    }

    pub(crate) const fn from_binary_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Script),
            1 => Some(Self::Module),
            2 => Some(Self::Eval),
            3 => Some(Self::Wasm),
            _ => None,
        }
    }
}

/// Compiler source snapshot and wrapper origin, retained for diagnostics.
#[derive(Clone, Debug)]
pub(crate) struct ProgramSource {
    pub(crate) text: std::sync::Arc<str>,
    pub(crate) line_offset: u32,
    pub(crate) column_offset: u32,
}

/// Starts of JavaScript source lines, including an empty line after a final terminator.
pub(crate) fn source_line_starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(text.char_indices().filter_map(|(index, character)| {
        let crlf_prefix = character == '\r' && text.as_bytes().get(index + 1) == Some(&b'\n');
        (matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}') && !crlf_prefix)
            .then_some(index + character.len_utf8())
    }))
}

impl ProgramSource {
    pub(crate) fn source_line(&self, line: u32) -> Option<&str> {
        let compiled_line = line.checked_add(self.line_offset)?.checked_sub(1)?;
        let mut starts = source_line_starts(&self.text);
        let start = starts.nth(compiled_line as usize)?;
        let end = starts.next().unwrap_or(self.text.len());
        let text = self
            .text
            .get(start..end)?
            .trim_end_matches(['\r', '\n', '\u{2028}', '\u{2029}']);
        if line != 1 {
            return Some(text);
        }
        let mut offset = 0;
        for (byte, character) in text.char_indices() {
            if offset == self.column_offset {
                return Some(&text[byte..]);
            }
            offset += character.len_utf16() as u32;
        }
        (offset == self.column_offset).then_some("")
    }
}

#[derive(Clone, Debug)]
pub struct ResidualProgram {
    pub(crate) specialized: bool,
    pub(crate) kind: ProgramKind,
    pub(crate) module_requests: Vec<ModuleRequest>,
    pub(crate) module_imports: Vec<ModuleImportBinding>,
    pub(crate) module_link_plan: Option<ModuleLinkPlan>,
    pub(crate) source_name: String,
    pub(crate) source: Option<ProgramSource>,
    pub(crate) atoms: AtomTable,
    pub(crate) constants: Vec<Constant>,
    pub(crate) functions: Vec<Function>,
    pub(crate) cache_sites: u16,
    pub(crate) method_sites: Vec<MethodSite>,
    pub(crate) method_arguments: Vec<Register>,
    pub(crate) field_sites: Vec<FieldSite>,
    pub(crate) object_sites: Vec<ObjectSite>,
    pub(crate) regexp_literal_sites: Vec<RegExpLiteralSite>,
    pub(crate) superinstructions: Vec<Superinstruction>,
    pub(crate) register_roots: Vec<u64>,
}

#[cold]
#[inline(never)]
fn local_loads_in_bounds(code: &[Instr], wide: &[WideInstruction], locals: u16) -> bool {
    code.iter().all(|instruction| {
        !matches!(
            instruction.op(),
            Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::StoreEnvLocal
        ) || instruction.local_slot() < usize::from(locals)
    }) && wide.iter().all(|instruction| {
        !matches!(
            instruction.op(),
            Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::StoreEnvLocal
        ) || instruction.local_slot() < usize::from(locals)
    })
}

impl ResidualProgram {
    pub(crate) fn source_position(&self, function: u32, pc: u32) -> Option<SourcePosition> {
        let positions = &self.functions.get(function as usize)?.source_positions;
        let position = *positions.get(
            positions
                .partition_point(|position| position.pc <= pc)
                .checked_sub(1)?,
        )?;
        self.adjust_source_position(position)
    }

    pub(crate) fn adjust_source_position(
        &self,
        mut position: SourcePosition,
    ) -> Option<SourcePosition> {
        if let Some(source) = &self.source {
            position.line = position
                .line
                .checked_sub(source.line_offset)
                .filter(|line| *line > 0)?;
            if position.line == 1 {
                position.column = position
                    .column
                    .checked_sub(source.column_offset)
                    .filter(|column| *column > 0)?;
            }
        }
        Some(position)
    }

    pub const FORMAT_VERSION: u8 = 93;
    pub const RUNTIME_ABI_FINGERPRINT: u64 = {
        const ABI_SIGNATURE: u64 = 0x5251_4a00_0000_0000;
        const FORMAT_VERSION_SHIFT: u32 = 16;
        ABI_SIGNATURE | ((Self::FORMAT_VERSION as u64) << FORMAT_VERSION_SHIFT)
    };

    pub(crate) fn is_module(&self) -> bool {
        self.kind == ProgramKind::Module
    }

    pub(crate) fn root_variables_are_local(&self) -> bool {
        self.is_module()
            || (self.kind == ProgramKind::Eval
                && self.functions.first().is_some_and(|root| root.strict))
    }

    /// The root function's slots project no global var or global lexical binding, so each
    /// slot of its environment is the binding itself.
    pub(crate) fn root_slots_are_bindings(&self) -> bool {
        self.functions.first().is_some_and(|root| {
            (self.root_variables_are_local() || root.global_var_atoms.is_empty())
                && (self.kind != ProgramKind::Script || root.global_lexical_atoms.is_empty())
        })
    }

    pub fn function_count(&self) -> usize {
        self.functions.len()
    }
    pub fn instruction_count(&self) -> usize {
        self.functions
            .iter()
            .map(|function| function.code.len())
            .sum()
    }
    pub fn write_binary(&self, path: &std::path::Path) -> Result<(), String> {
        binary::write_program(self, path)
    }

    pub fn read_binary(path: &std::path::Path) -> Result<Self, String> {
        binary::read_program(path)
    }
}
mod binary;
mod disassemble;
#[cfg(test)]
mod tests;
mod validate;
