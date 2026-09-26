pub type Atom = u32;
pub type Register = u16;

mod atoms;
mod control_flow;
mod instruction;
mod numeric_ops;
pub(crate) use atoms::AtomTable;
pub use instruction::Instr;
pub(crate) use instruction::{ConstructArguments, WideInstruction};
pub(crate) use numeric_ops::specialized_numeric_op;
pub(crate) const RETURN_REGISTER: Register = 1 << 15;
pub(crate) const SET_THIS_REGISTER: Register = 1 << 14;
pub(crate) const REGISTER_MASK: Register = SET_THIS_REGISTER - 1;
pub(crate) const NUMERIC_LOCAL_INC_STORE: u16 = 1;
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
    ConstructCountAndFlags,
    RegisterPair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResultLayout {
    NoResult,
    Register,
    Returnable,
    ReturnableAndThis,
    NumericReturnable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldLayout {
    Undeclared,
    Register,
    FunctionIndex,
    ConstructArguments,
    ElementCount,
    CacheSiteIndex,
    Operand,
    BinaryOperator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstructionField {
    #[allow(dead_code)]
    A,
    B,
    C,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImmediateRole {
    Undeclared,
    ConstantIndex,
    ClosureFunctionIndex,
    ArrayLength,
    AtomIndex,
    LocalSlot,
    FunctionNamePrefix,
    BooleanFlag,
    ArrayIndex,
    BinaryOperator,
    UnaryOperator,
    TemplateSiteIndex,
    JumpTarget,
    MethodSiteIndex,
    ObjectSiteIndex,
    SuperinstructionIndex,
    FieldLookup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldLookup {
    Atom(Atom),
    Site(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OperandLayout {
    a: FieldLayout,
    b: FieldLayout,
    c: FieldLayout,
}

impl OperandLayout {
    const UNDECLARED: Self = Self {
        a: FieldLayout::Undeclared,
        b: FieldLayout::Undeclared,
        c: FieldLayout::Undeclared,
    };

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
            Self::NoResult => NO_RESULT_FLAGS,
            Self::Register => NO_RESULT_FLAGS,
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
            Self::CaptureDepthAndSlot | Self::CallWindow | Self::CallWindowWithEvalFlags
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
    ($($name:ident => $effect:expr $(; layout $layout:ident)? $(; meaning $immediate_role:ident)? $(, @ $result:ident)? $(, @ fields($a:ident, $b:ident, $c:ident))?),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u16)]
        pub enum Op { $($name),+ }

        #[allow(dead_code)]
        impl Op {
            pub const COUNT: usize = [$(stringify!($name)),+].len();
            pub const NAMES: [&'static str; Self::COUNT] = [$(stringify!($name)),+];
            const EFFECTS: [Effect; Self::COUNT] = [$($effect),+];
            const RESULT_LAYOUTS: [ResultLayout; Self::COUNT] = [$(
                opcodes!(@result $($result)?)),+
            ];
            const IMMEDIATE_LAYOUTS: [ImmediateLayout; Self::COUNT] = [$(
                opcodes!(@layout $($layout)?)),+
            ];

            const OPERAND_LAYOUTS: [OperandLayout; Self::COUNT] = [$(
                opcodes!(@fields $($a, $b, $c)?)),+
            ];
            const IMMEDIATE_ROLES: [ImmediateRole; Self::COUNT] = [$(
                opcodes!(@immediate $($immediate_role)?)),+
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

            pub(crate) const fn field_layout(self, field: InstructionField) -> FieldLayout {
                Self::OPERAND_LAYOUTS[self as usize].field(field)
            }

            pub(crate) const fn immediate_role(self) -> ImmediateRole {
                Self::IMMEDIATE_ROLES[self as usize]
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
    (@layout) => { ImmediateLayout::Scalar };
    (@result $result:ident) => { ResultLayout::$result };
    (@result) => { ResultLayout::Register };
    (@fields $a:ident, $b:ident, $c:ident) => {
        OperandLayout { a: FieldLayout::$a, b: FieldLayout::$b, c: FieldLayout::$c }
    };
    (@fields) => { OperandLayout::UNDECLARED };
    (@immediate $role:ident) => { ImmediateRole::$role };
    (@immediate) => { ImmediateRole::Undeclared };
}
const READ_THROW: Effect = Effect::READS_HEAP.union(Effect::THROWS);
const WRITE_THROW: Effect = Effect::WRITES_HEAP.union(Effect::THROWS);
const CALL_EFFECT: Effect = READ_THROW.union(Effect::WRITES_HEAP);

opcodes!(
    Nop => Effect::PURE,
    CloneEnv => Effect::READS_HEAP.union(Effect::WRITES_HEAP),
    Wide => Effect::PURE,
    LoadConst => Effect::PURE; meaning ConstantIndex,
    LoadLocal => Effect::PURE; meaning LocalSlot,
    StoreLocal => Effect::PURE; meaning LocalSlot,
    LoadEnvLocal => Effect::READS_HEAP; meaning LocalSlot,
    StoreEnvLocal => Effect::WRITES_HEAP; meaning LocalSlot,
    LoadCapture => Effect::READS_HEAP; layout CaptureDepthAndSlot,
    StoreCapture => Effect::WRITES_HEAP; layout CaptureDepthAndSlot,
    LoadName => READ_THROW; meaning AtomIndex,
    LoadNameCall => READ_THROW; meaning AtomIndex,
    LoadNameTypeof => Effect::READS_HEAP; meaning AtomIndex,
    ResolveName => READ_THROW; meaning AtomIndex,
    LoadResolvedName => READ_THROW; meaning AtomIndex,
    DeleteName => READ_THROW; meaning AtomIndex,
    StoreName => WRITE_THROW; meaning AtomIndex,
    StoreResolvedName => WRITE_THROW; meaning AtomIndex,
    LoadThis => Effect::PURE,
    LoadImportMeta => Effect::READS_HEAP.union(Effect::WRITES_HEAP),
    MakeClosure => CALL_EFFECT; meaning ClosureFunctionIndex,
    MakeArray => CALL_EFFECT; meaning ArrayLength,
    MakeConstArray => CALL_EFFECT; meaning ConstantIndex, @ Register, @ fields(Undeclared, ElementCount, Undeclared),
    MakeObject => CALL_EFFECT,
    MakeObject2 => CALL_EFFECT; meaning ObjectSiteIndex, @ Returnable,
    SuperConstArrayObject2 => CALL_EFFECT; meaning SuperinstructionIndex, @ Returnable,
    GetIterator => READ_THROW, @ Register, @ fields(Undeclared, Register, Undeclared),
    GetAsyncIterator => READ_THROW, @ Register, @ fields(Undeclared, Register, Undeclared),
    IteratorClose => READ_THROW, @ NoResult, @ fields(Undeclared, Register, Undeclared),
    SpreadToArray => CALL_EFFECT, @ Register, @ fields(Undeclared, Register, Undeclared),
    RequireObjectCoercible => READ_THROW, @ NoResult, @ fields(Undeclared, Register, Undeclared),
    RequireIteratorResult => READ_THROW, @ NoResult, @ fields(Undeclared, Register, Undeclared),
    SuperCallCheck => READ_THROW, @ NoResult,
    IteratorCleanupPush => Effect::CONTROL, @ NoResult, @ fields(Register, Register, Undeclared),
    IteratorCleanupPop => Effect::CONTROL, @ NoResult,
    SetFunctionName => Effect::WRITES_HEAP; meaning AtomIndex,
    SetFunctionNameKey => Effect::READS_HEAP.union(Effect::WRITES_HEAP); meaning FunctionNamePrefix,
    InitializeTdz => Effect::PURE; meaning LocalSlot,
    Await => READ_THROW.union(Effect::CONTROL),
    Yield => READ_THROW.union(Effect::CONTROL),
    YieldStar => READ_THROW.union(Effect::CONTROL); layout RegisterPair,
    GetField => READ_THROW; meaning FieldLookup, @ ReturnableAndThis,
    GetIndex => READ_THROW, @ Register, @ fields(Undeclared, Operand, Operand),
    ToPropertyKey => READ_THROW,
    ToNumeric => READ_THROW,
    CopyDataProperties => CALL_EFFECT,
    MarkPrivateName => Effect::WRITES_HEAP; meaning AtomIndex,
    SetField => WRITE_THROW; meaning AtomIndex, @ NoResult, @ fields(Register, Register, CacheSiteIndex),
    DefineComputedField => WRITE_THROW, @ NoResult, @ fields(Register, Register, Register),
    SetThisField => WRITE_THROW; meaning AtomIndex, @ NoResult, @ fields(Register, Undeclared, CacheSiteIndex),
    SetIndex => WRITE_THROW; meaning BooleanFlag, @ NoResult, @ fields(Register, Register, Register),
    DefineArrayElement => WRITE_THROW; meaning ArrayIndex, @ NoResult, @ fields(Register, Register, Undeclared),
    Binary => READ_THROW; meaning BinaryOperator, @ NumericReturnable, @ fields(Undeclared, Operand, Operand),
    IncDec => READ_THROW; meaning BooleanFlag, @ Register, @ fields(Undeclared, Register, Undeclared),
    Unary => READ_THROW; meaning UnaryOperator, @ Register, @ fields(Undeclared, Register, Undeclared),
    Delete => READ_THROW; meaning BooleanFlag,
    CheckPrivate => READ_THROW; meaning AtomIndex,
    PrivateIn => READ_THROW; meaning AtomIndex,
    Move => Effect::PURE, @ Register, @ fields(Undeclared, Register, Undeclared),
    Call => CALL_EFFECT; layout CallWindowWithEvalFlags, @ Returnable,
    CallDirectEvalArray => CALL_EFFECT; layout CallWindowWithEvalFlags, @ Returnable,
    CallKnown => CALL_EFFECT; layout CallWindow, @ Returnable, @ fields(Undeclared, FunctionIndex, Undeclared),
    CallMethod => CALL_EFFECT; meaning MethodSiteIndex, @ Returnable, @ fields(Undeclared, Register, Undeclared),
    CallThisMethod => CALL_EFFECT; meaning MethodSiteIndex, @ Returnable,
    Construct => CALL_EFFECT; layout ConstructCountAndFlags, @ Returnable, @ fields(Undeclared, Register, ConstructArguments),
    Jump => Effect::CONTROL; meaning JumpTarget, @ NoResult,
    JumpFalse => Effect::CONTROL; meaning JumpTarget, @ NoResult, @ fields(Register, Undeclared, Undeclared),
    JumpBinaryFalse => READ_THROW.union(Effect::CONTROL); meaning JumpTarget, @ NoResult, @ fields(BinaryOperator, Operand, Operand),
    Return => Effect::CONTROL, @ NoResult, @ fields(Register, Undeclared, Undeclared),
    Throw => Effect::THROWS.union(Effect::CONTROL), @ NoResult, @ fields(Register, Undeclared, Undeclared),
    NumericAdd => READ_THROW; meaning BinaryOperator, @ NumericReturnable, @ fields(Undeclared, Operand, Operand),
    NumericMultiply => READ_THROW; meaning BinaryOperator, @ NumericReturnable, @ fields(Undeclared, Operand, Operand),
    InitializeThis => Effect::CONTROL,
    CacheTemplateObject => Effect::READS_HEAP.union(Effect::WRITES_HEAP); meaning TemplateSiteIndex,
    LoadCachedTemplateObject => Effect::READS_HEAP; meaning TemplateSiteIndex,
    DefineField => WRITE_THROW; meaning AtomIndex, @ NoResult, @ fields(Register, Register, Undeclared),
    ValidateClassHeritage => READ_THROW,
);
#[derive(Clone, Debug)]
pub struct Function {
    pub parent: Option<u32>,
    pub name: Option<Atom>,
    pub source_text: Option<String>,
    pub params: u16,
    pub length: u16,
    pub parameter_end_pc: u32,
    pub parameter_atoms: Vec<Atom>,
    pub rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_class_constructor: bool,
    pub derived_constructor: bool,
    /// Lexical HomeObject captured by methods and arrows containing `super`.
    pub super_home_atom: Option<Atom>,
    pub constructible: bool,
    pub class_field_initializer: bool,
    pub parameter_eval_arguments_error: bool,
    pub arguments_slot: Option<u16>,
    pub strict: bool,
    pub locals: u16,
    pub local_atoms: Vec<Atom>,
    pub lexical_atoms: Vec<Atom>,
    pub global_lexical_atoms: Vec<Atom>,
    pub global_var_atoms: Vec<Atom>,
    pub global_function_atoms: Vec<Atom>,
    pub global_immutable_atoms: Vec<Atom>,
    pub eval_sites: Vec<EvalSite>,
    pub code: Vec<Instr>,
    pub(crate) wide: Vec<WideInstruction>,
    pub registers: u16,
    pub(crate) dispatch: DispatchClass,
    pub(crate) handlers: Vec<Handler>,
    pub(crate) register_root_offset: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct EvalSite {
    pub(crate) resume_pc: u32,
    pub(crate) lexical_bindings: Vec<EvalBinding>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EvalBinding {
    pub(crate) atom: Atom,
    pub(crate) slot: u16,
    pub(crate) immutable: bool,
}

/// High bit of `Function::arguments_slot` marks a mapped (sloppy, simple
/// parameter-list) arguments object. Keeping this bit in the existing
/// residual field preserves the binary format while making the mapping an
/// explicit execution invariant.
pub(crate) const MAPPED_ARGUMENTS_BIT: u16 = 1 << 15;
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ObjectSite {
    pub atoms: [Atom; 2],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Superinstruction {
    pub code: [Instr; 4],
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldSite {
    pub base: FieldBase,
    pub first: (Atom, u16),
    pub second: Option<(Atom, u16)>,
    pub sink: Option<(Atom, u16)>,
}

#[derive(Clone, Copy, Debug)]
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

    pub(crate) fn constant(value: u32) -> Self {
        debug_assert!(value <= u32::from(Self::PAYLOAD_MASK));
        Self(((OperandKind::Constant as u16) << Self::TAG_SHIFT) | value as u16)
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

#[derive(Clone, Debug)]
pub struct ResidualProgram {
    pub(crate) specialized: bool,
    pub(crate) module: bool,
    pub(crate) module_requests: Vec<ModuleRequest>,
    pub(crate) module_imports: Vec<ModuleImportBinding>,
    pub(crate) module_link_plan: Option<ModuleLinkPlan>,
    pub(crate) source_name: String,
    pub(crate) atoms: AtomTable,
    pub(crate) constants: Vec<Constant>,
    pub(crate) functions: Vec<Function>,
    pub(crate) cache_sites: u16,
    pub(crate) method_sites: Vec<MethodSite>,
    pub(crate) method_arguments: Vec<Register>,
    pub(crate) field_sites: Vec<FieldSite>,
    pub(crate) object_sites: Vec<ObjectSite>,
    pub(crate) superinstructions: Vec<Superinstruction>,
    pub(crate) register_roots: Vec<u64>,
}

#[cold]
#[inline(never)]
fn local_loads_in_bounds(code: &[Instr], wide: &[WideInstruction], locals: u16) -> bool {
    code.iter().all(|instruction| {
        instruction.op() != Op::LoadLocal || instruction.local_slot() < usize::from(locals)
    }) && wide.iter().all(|instruction| {
        instruction.op() != Op::LoadLocal || instruction.local_slot() < usize::from(locals)
    })
}

impl ResidualProgram {
    pub const FORMAT_VERSION: u8 = 25;
    pub const RUNTIME_ABI_FINGERPRINT: u64 = 0x5251_4a00_0019_0000;

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
