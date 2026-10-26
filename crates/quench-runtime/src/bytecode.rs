use smallvec::SmallVec;

pub(crate) const INTRINSIC_REGEXP_BINDING: &str = "\0quench:intrinsic-regexp";
/// Internal global bindings, which compiled code loads by name and no source text can
/// declare: the for-in key list and its per-key enumerability check, iterator closing and
/// `with` scope exit (`WITH_ENTER_BINDING` enters).
pub(crate) const FOR_IN_KEYS_BINDING: &str = "\0quench:for-in-keys";
pub(crate) const FOR_IN_KEY_IS_ENUMERABLE_BINDING: &str = "\0quench:for-in-key-is-enumerable";
pub(crate) const ITERATOR_CLOSE_BINDING: &str = "\0quench:iterator-close";
/// The intrinsic a `super` access reads its home's parent with (`Native::ReflectGetPrototypeOf`).
pub(crate) const SUPER_BASE_BINDING: &str = "\0quench:super-base";
pub(crate) const WITH_EXIT_BINDING: &str = "\0quench:with-exit";
/// Every internal global the realm installs on its global object.
pub(crate) const INTERNAL_GLOBAL_BINDINGS: [&str; 5] = [
    FOR_IN_KEYS_BINDING,
    FOR_IN_KEY_IS_ENUMERABLE_BINDING,
    ITERATOR_CLOSE_BINDING,
    WITH_ENTER_BINDING,
    WITH_EXIT_BINDING,
];

/// The native an engine-internal intrinsic name stands for: a by-name read of it resolves to
/// that native before consulting any binding (`Vm::intrinsic_name_value`), and no source
/// identifier can spell it, so the read observes no scope.
pub(crate) fn intrinsic_name_native(name: &str) -> Option<crate::heap::Native> {
    use crate::heap::Native;
    Some(match name {
        "\0quench:dynamic-import" => Native::DynamicImport,
        "\0quench:intrinsic-promise" => Native::Promise,
        "\0quench:super-get" => Native::ReflectGet,
        "\0quench:super-set" => Native::SuperSet,
        SUPER_BASE_BINDING => Native::ReflectGetPrototypeOf,
        "\0quench:object-literal-prototype" => Native::ObjectLiteralPrototype,
        "\0quench:object-define-property" => Native::ObjectDefineProperty,
        "\0quench:object-freeze" => Native::ObjectFreeze,
        // What the compiler's own throws, class setup and `using` declarations call: the
        // realm's intrinsics, which a binding of the same name in scope does not replace.
        INTRINSIC_TYPE_ERROR_BINDING => Native::TypeError,
        INTRINSIC_REFERENCE_ERROR_BINDING => Native::ReferenceError,
        INTRINSIC_SET_PROTOTYPE_OF_BINDING => Native::ObjectSetPrototypeOf,
        INTRINSIC_DISPOSABLE_STACK_BINDING => Native::DisposableStack,
        // The iteration helpers each realm also installs on its global object.
        ITERATOR_CLOSE_BINDING => Native::IteratorClose,
        FOR_IN_KEYS_BINDING => Native::ForInKeys,
        FOR_IN_KEY_IS_ENUMERABLE_BINDING => Native::ForInKeyIsEnumerable,
        _ => return None,
    })
}

/// Whether a by-name read of `name` is an intrinsic's (`intrinsic_name_native` or the RegExp
/// intrinsic), which reads no binding.
pub(crate) fn is_intrinsic_name(name: &str) -> bool {
    name == INTRINSIC_REGEXP_BINDING || intrinsic_name_native(name).is_some()
}
pub(crate) const INTRINSIC_TYPE_ERROR_BINDING: &str = "\0quench:intrinsic-type-error";
pub(crate) const INTRINSIC_REFERENCE_ERROR_BINDING: &str = "\0quench:intrinsic-reference-error";
pub(crate) const INTRINSIC_SET_PROTOTYPE_OF_BINDING: &str = "\0quench:intrinsic-set-prototype-of";
pub(crate) const INTRINSIC_DISPOSABLE_STACK_BINDING: &str = "\0quench:intrinsic-disposable-stack";
/// The engine-internal global a `with` statement calls to enter its object scope.
pub(crate) const WITH_ENTER_BINDING: &str = "\0quench:with-enter";
pub(crate) const LEXICAL_THIS_BINDING: &str = "\0quench:lexical-this";
/// The `Unary` operator a template literal substitution compiles to: ToString (ECMA-262
/// 13.15.5.1 step 5), which no source operator spells. It extends oxc's operator numbering.
pub(crate) const UNARY_TO_STRING: u32 = oxc_ast::ast::UnaryOperator::Delete as u32 + 1;
/// The names `typeof` yields (ECMA-262 13.5.3.1), indexed as `typeof_name_index` (vm/coercion.rs) classifies
/// values.
pub(crate) const TYPEOF_NAMES: [&str; 8] =
    ["undefined", "object", "boolean", "number", "string", "bigint", "symbol", "function"];
/// The `Unary` operators that compare `typeof` of their operand with a name in
/// `TYPEOF_NAMES` (`typeof x === "function"`): this plus the name's index tests equality,
/// and that plus `TYPEOF_NAMES.len()` inequality. The comparison of two strings that
/// `typeof` and a literal would make has no effects, so it needs neither string.
pub(crate) const UNARY_TYPEOF_IS: u32 = UNARY_TO_STRING + 1;
const UNARY_TYPEOF_TESTS: u32 = 2 * TYPEOF_NAMES.len() as u32;

/// The `UNARY_TYPEOF_IS` operator testing `typeof x` against `name`, negated for an
/// inequality; `None` when no value's `typeof` is `name`.
pub(crate) fn typeof_test_operator(name: &str, negated: bool) -> Option<u32> {
    let index = typeof_name_index(name)? as u32;
    Some(UNARY_TYPEOF_IS + index + if negated { TYPEOF_NAMES.len() as u32 } else { 0 })
}

/// The `Unary` operator giving the index in `TYPEOF_NAMES` of the name `typeof` yields for
/// its operand, as an integer: `switch (typeof x)` with string cases compares it with each
/// case's index (`FunctionCompiler::switch_statement`), as `typeof x === "name"` would
/// compare the two strings, and like that comparison it needs neither string.
pub(crate) const UNARY_TYPEOF_INDEX: u32 = UNARY_TYPEOF_IS + UNARY_TYPEOF_TESTS;

/// The index in `TYPEOF_NAMES` of `name`, if some value's `typeof` is `name`.
pub(crate) fn typeof_name_index(name: &str) -> Option<usize> {
    TYPEOF_NAMES.iter().position(|candidate| *candidate == name)
}

/// The name index and negation a `UNARY_TYPEOF_IS` operator tests.
#[inline(always)]
pub(crate) fn typeof_test(operator: u32) -> Option<(usize, bool)> {
    let offset = operator.checked_sub(UNARY_TYPEOF_IS).filter(|offset| *offset < UNARY_TYPEOF_TESTS)?;
    let names = TYPEOF_NAMES.len() as u32;
    Some(((offset % names) as usize, offset >= names))
}
pub(crate) const NEW_TARGET_BINDING: &str = "\0quench:new-target";

pub type Atom = u32;
pub type Register = u16;

mod atoms;
mod control_flow;
pub(crate) mod initialization;
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

/// The name a hidden renamed block binding has in source; other names are unchanged.
pub(crate) fn source_binding_name(name: &str) -> &str {
    name.strip_prefix(BLOCK_BINDING_MARKER).unwrap_or(name)
}

/// Whether a local named `name` must live in its function's environment whatever its
/// captures: other hidden bindings (lexical `this`, `new.target`, home objects, private
/// names, ...) are reached by name, not only through capture slots. A renamed block binding
/// is reached only through its slot.
pub(crate) fn local_name_requires_environment(name: &str) -> bool {
    name.starts_with('\0') && !name.starts_with(BLOCK_BINDING_MARKER)
}
/// The prefix of a class's home-object binding, which private brand checks and `super`
/// resolve by name through the enclosing environments.
pub(crate) const HOME_BINDING_PREFIX: &str = "\0quench:home:";
/// Whether the runtime finds local `name` by name from outside its own activation, so a
/// closure that never captures it may still read it: a home object (`HOME_BINDING_PREFIX`),
/// or a compiler binding its function's environment declares (`declares_environment_atom`),
/// which by-name loads of compiler bindings resolve.
pub(crate) fn local_name_read_by_name(name: &str, declared_in_environment: bool) -> bool {
    name.starts_with(HOME_BINDING_PREFIX)
        || (declared_in_environment && local_name_requires_environment(name))
}
pub(crate) use control_flow::{ControlEdge, branch_table_entries, for_each_instruction_successor};
pub use instruction::Instr;
pub(crate) use instruction::{ConstructArguments, RegisterWindow, WideInstruction};
pub(crate) use numeric_ops::specialized_numeric_op;
pub(crate) const RETURN_REGISTER: Register = 1 << 15;
/// Property cache sites are 16-bit. A program with more sites than that gives every site
/// past the range this last one, for which the VM never records a cache entry, so those
/// sites always take the uncached lookup.
pub(crate) const SHARED_UNCACHED_SITE: u16 = u16::MAX - 1;
/// The sites below `SHARED_UNCACHED_SITE` a program shares by property name once its
/// `UNIQUE_CACHE_SITES` run out: every named load, or every named store, of one atom past
/// that point names the same one (`Compiler::name_shared_cache_site`). A site's entries are
/// right for its single atom whichever instruction records them; the loads and stores of a
/// name keep separate sites, since a load's entry (a read-only field, an inherited holder)
/// is no store's. TypeScript's 3.6 MB bundle runs out of unique sites before its node and
/// symbol constructors, whose `this` field adds went uncached.
pub(crate) const NAME_SHARED_CACHE_SITES: u16 = 8192;
/// The sites a program numbers one per instruction, below the name-shared ones.
pub(crate) const UNIQUE_CACHE_SITES: u16 = SHARED_UNCACHED_SITE - NAME_SHARED_CACHE_SITES;
pub(crate) const SET_THIS_REGISTER: Register = 1 << 14;
pub(crate) const REGISTER_MASK: Register = SET_THIS_REGISTER - 1;
pub(crate) const INLINE_OBJECT_SITE_ATOMS: usize = 2;
pub(crate) const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;
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
    /// Shared with the heap strings materialized from it (`JsString::from_shared_text`), so a
    /// long literal's text is held once.
    String(std::rc::Rc<str>),
    StringUnits(Vec<u16>),
    BigInt(num_bigint::BigInt),
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
    /// A register the instruction may also write its result to (`NO_OPTIONAL_REGISTER` for
    /// none): an output.
    OptionalRegister,
    /// A register the instruction reads when present (`NO_OPTIONAL_REGISTER` for none): an
    /// input, encoded as `OptionalRegister` is.
    OptionalReadRegister,
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
    LiteralTemplateIndex,
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

impl Op {
    /// Whether an instruction of this op with field `b` names an atom by its immediate: an
    /// atom index, or a field lookup by name (`FieldLookup::Atom`; through a nested field
    /// site it names the site).
    pub(crate) fn immediate_names_atom(self, b: u16) -> bool {
        match self.immediate_role() {
            ImmediateRole::AtomIndex => true,
            ImmediateRole::FieldLookup => b != FieldBase::NESTED,
            _ => false,
        }
    }
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

    /// A lane `Call` record whose result register is dead after the call (`derive_lane_view`).
    /// Lane call records carry no eval flags (`lane_call_fits`), so the flag reuses the
    /// direct-eval bit; a callee whose result is unobservable may skip building it.
    pub(crate) const LANE_CALL_RESULT_UNUSED_FLAG: u32 = Self::DIRECT_EVAL_FLAG;

    pub(crate) const fn lane_call_result_used(imm: u32) -> bool {
        imm & Self::LANE_CALL_RESULT_UNUSED_FLAG == 0
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
    ($($name:ident => $effect:expr; layout $layout:ident $(; meaning $immediate_role:ident)? $(; flow $flow:ident)? $(; js $js_denotation:ident)? , @ $result:ident, @ fields($a:ident, $b:ident, $c:ident)),+ $(,)?) => {
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
            const JS_DENOTATIONS: [JsDenotation; Self::COUNT] = [$(
                opcodes!(@js $($js_denotation)?)),+
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

            pub(crate) const fn js_denotation(self) -> JsDenotation {
                Self::JS_DENOTATIONS[self as usize]
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
    (@js $denotation:ident) => { JsDenotation::$denotation };
    (@js) => { JsDenotation::Unsupported };
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
    CapturedValue,
    PlainLocalLoad,
    PlainLocalStore,
    Return,
}
const READ_THROW: Effect = Effect::READS_HEAP.union(Effect::THROWS);
const WRITE_THROW: Effect = Effect::WRITES_HEAP.union(Effect::THROWS);
const CALL_EFFECT: Effect = READ_THROW.union(Effect::WRITES_HEAP);

opcodes!(
    Nop => Effect::PURE; layout Scalar; meaning Unused; js Noop, @ Register, @ fields(Unused, Unused, Unused),
    CloneEnv => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning EnvironmentCloneIndex, @ Register, @ fields(Unused, Unused, Unused),
    Wide => Effect::PURE; layout Scalar; meaning WideInstructionIndex, @ Register, @ fields(WideIndexChunk, WideIndexChunk, WideIndexChunk),
    LoadConst => Effect::PURE; layout Scalar; meaning ConstantIndex; js LoadConst, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadLocal => Effect::PURE; layout Scalar; meaning LocalSlot, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocal => Effect::PURE; layout Scalar; meaning LocalSlot, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    StoreVarBinding => CALL_EFFECT; layout Scalar; meaning LocalSlot, @ Register, @ fields(Register, Unused, Unused),
    LoadEnvLocal => Effect::READS_HEAP; layout Scalar; meaning LocalSlot, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreEnvLocal => Effect::WRITES_HEAP; layout Scalar; meaning LocalSlot, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    LoadCapture => Effect::READS_HEAP; layout CaptureDepthAndSlot; js CapturedValue, @ Register, @ fields(ResultRegister, Unused, Unused),
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
    MakeLiteralTemplate => CALL_EFFECT; layout Scalar; meaning LiteralTemplateIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
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
    AppendArray => CALL_EFFECT; layout Scalar; meaning ArrayLength, @ Register, @ fields(Register, OptionalReadRegister, BooleanFlag),
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
    GetField => READ_THROW; layout Scalar; meaning FieldLookup; js GetField, @ ReturnableAndThis, @ fields(ResultRegister, FieldBase, FieldLookupCacheSiteIndex),
    GetIndex => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, NumericIndexOperand, NumericIndexOperand),
    ToPropertyKey => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    ToNumeric => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    CopyDataProperties => CALL_EFFECT; layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Register),
    MarkPrivateName => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Unused, Register, Register),
    DefinePrivateField => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Register, Register),
    SetField => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldStore, @ Register, @ fields(Register, Register, CacheSiteIndex),
    DefineComputedField => WRITE_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Register, Register, Register),
    SetThisField => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    SetIndex => WRITE_THROW; layout Scalar; meaning BooleanFlag, @ Register, @ fields(Register, Register, Register),
    DefineArrayElement => WRITE_THROW; layout Scalar; meaning ArrayIndex, @ Register, @ fields(Register, Register, Unused),
    Binary => READ_THROW; layout Scalar; meaning BinaryOperator; js NumericBinary, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    IncDec => READ_THROW; layout Scalar; meaning BooleanFlag, @ Register, @ fields(ResultRegister, Register, Unused),
    Unary => READ_THROW; layout Scalar; meaning UnaryOperator; js NumericUnary, @ Returnable, @ fields(ResultRegister, Register, Unused),
    Delete => READ_THROW; layout Scalar; meaning BooleanFlag, @ Register, @ fields(ResultRegister, Register, Register),
    CheckPrivate => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, Unused),
    PrivateIn => READ_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    Move => Effect::PURE; layout Scalar; meaning Unused; js Move, @ Register, @ fields(ResultRegister, Register, Unused),
    Call => CALL_EFFECT; layout CallWindowWithEvalFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallArray => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallSpread => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallKnown => CALL_EFFECT; layout CallWindow; flow Call, @ Returnable, @ fields(ResultRegister, FunctionIndex, Unused),
    CallMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call, @ Returnable, @ fields(ResultRegister, Register, Unused),
    CallThisMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call, @ Returnable, @ fields(ResultRegister, Unused, Unused),
    Construct => CALL_EFFECT; layout ConstructCountAndFlags; flow Call, @ Returnable, @ fields(ResultRegister, Register, ConstructArguments),
    Jump => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow Jump; js Jump, @ Register, @ fields(Unused, Unused, Unused),
    JumpFalse => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, Unused, Unused),
    JumpBinaryFalse => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(BinaryOperator, Operand, Operand),
    Return => Effect::CONTROL; layout Scalar; meaning Unused; flow Terminal; js Return, @ Register, @ fields(Register, Unused, Unused),
    Throw => Effect::THROWS.union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal, @ Register, @ fields(Register, Unused, Unused),
    NumericAdd => READ_THROW; layout Scalar; meaning AdditionOperator; js NumericBinary, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    NumericMultiply => READ_THROW; layout Scalar; meaning MultiplicationOperator; js NumericBinary, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    InitializeThis => CALL_EFFECT.union(Effect::CONTROL); layout Scalar; meaning Unused, @ Register, @ fields(Register, Unused, Unused),
    CacheTemplateObject => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning TemplateSiteIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadCachedTemplateObject => Effect::READS_HEAP; layout Scalar; meaning TemplateSiteIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    DefineField => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Register, CacheSiteIndex),
    ValidateClassHeritage => READ_THROW; layout Scalar; meaning Unused, @ Register, @ fields(Register, Unused, Unused),
    SetFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldStore, @ Register, @ fields(Register, Register, CacheSiteIndex),
    SetThisFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    // The general loop's fast arms dispatch on these with the opcodes above, so they share
    // one dense range of opcode numbers.
    LoadLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot; js PlainLocalLoad, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot; js PlainLocalStore, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
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
    // `arguments.length` and `arguments[key]` of the `arguments` binding in the local slot:
    // an unmaterialized binding answers from the values its activation kept
    // (`Function::kept_arguments`); any other value is read as `GetField` and
    // `GetIndex` read it.
    ArgumentsLength => READ_THROW; layout Scalar; meaning LocalSlot, @ Register, @ fields(ResultRegister, Unused, Unused),
    ArgumentGet => READ_THROW; layout Scalar; meaning LocalSlot, @ Register, @ fields(ResultRegister, Register, Unused),

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

/// Membership views of a function's declared-name lists, which stay the authority for
/// their order and serialization: a name's declaration kind is one lookup, not a scan.
#[derive(Clone, Debug, Default)]
pub(crate) struct DeclaredAtomSets {
    global_lexical: rustc_hash::FxHashSet<Atom>,
    global_var: rustc_hash::FxHashSet<Atom>,
    global_immutable: rustc_hash::FxHashSet<Atom>,
    environment: rustc_hash::FxHashSet<Atom>,
    /// Per local slot: whether its name is a global var or global lexical declaration.
    slots: Box<[SlotDeclaration]>,
}

/// Whether a local slot's name is among `global_var_atoms` and `global_lexical_atoms`.
/// What a root environment slot holds for the reads through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootSlotRule {
    /// The binding itself.
    Binding,
    /// A Script's global lexical binding: the slot itself outside a dynamic script's
    /// evaluation, the realm's record during one.
    GlobalLexical,
    /// A global var or module import, which reads go through.
    Projection,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SlotDeclaration {
    pub(crate) global_var: bool,
    pub(crate) global_lexical: bool,
}

/// The names a Script's or eval's GlobalDeclarationInstantiation (EvalDeclarationInstantiation)
/// creates: lexical bindings, var-scoped names, hoisted functions, Annex B function vars and
/// the immutable (const and class) lexical names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalDeclarations {
    pub lexical: Vec<Atom>,
    pub var: Vec<Atom>,
    pub function: Vec<Atom>,
    pub annex_b_var: Vec<Atom>,
    pub immutable: Vec<Atom>,
}

impl GlobalDeclarations {
    /// The declarations, boxed only when there are any.
    pub fn boxed(self) -> Option<Box<Self>> {
        let empty = self.lexical.is_empty()
            && self.var.is_empty()
            && self.function.is_empty()
            && self.annex_b_var.is_empty()
            && self.immutable.is_empty();
        (!empty).then(|| Box::new(self))
    }
}

#[derive(Clone, Debug)]
pub struct Function {
    pub parent: Option<u32>,
    pub name: Option<Atom>,
    /// The name NamedEvaluation gives an anonymous function definition at a statically
    /// named position (`const f = () => {}`): its closures are made with it, as
    /// SetFunctionName would name them right after creation.
    pub inferred_name: Option<Atom>,
    pub is_arrow: bool,
    pub self_binding_slot: Option<u16>,
    /// The function's source text as a range of the program's `source`, which
    /// `Function.prototype.toString` reads on demand.
    pub source_span: Option<SourceSpan>,
    pub params: u16,
    pub length: u16,
    pub parameter_end_pc: u32,
    pub(crate) parameter_atoms: crate::heap::ThinBoxSlice<Atom>,
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
    /// An ordinary function expression whose `prototype` no program text observes until a
    /// callee's legacy `caller` reaches it (`compile::call_only`): its closures make none
    /// at creation.
    pub prototype_unobservable: bool,
    pub class_initializer: bool,
    pub parameter_eval_arguments_error: bool,
    /// A mapped `arguments` binding whose mapping nothing can observe: nothing in the body
    /// rebinds a parameter, and it only reads `arguments.length` and `arguments[key]` or
    /// passes the object on after its last parameter reference
    /// (`compile::early::FixedRegisterLocalFacts::arguments_mapping_unobserved`). No
    /// parameter is written, and none is read after an element could be, so the object
    /// keeps the mapped layout without parameter links (`arguments_alias_parameters`).
    pub unaliased_arguments: bool,
    pub arguments_slot: Option<u16>,
    pub simple_parameters: bool,
    pub strict: bool,
    pub locals: u16,
    pub(crate) local_atoms: crate::heap::ThinBoxSlice<Atom>,
    /// Names visible through this function's captured declarative environment.
    pub(crate) environment_atoms: crate::heap::ThinBoxSlice<Atom>,
    /// Source local slots owned by a closure environment when this activation
    /// escapes. `None` retains legacy whole-frame promotion; `Some` is the
    /// selective layout, including an empty set for closure-only activations.
    pub(crate) selective_capture_slots: Option<Vec<u16>>,
    /// Binding slots assigned stable frame registers by lowering.
    pub(crate) local_registers: crate::heap::ThinBoxSlice<LocalRegister>,
    /// The function was created while an enclosing `with` scope was active,
    /// so unresolved names in descendants can still observe its outer locals.
    pub(crate) inherited_with_scope: bool,
    pub(crate) lexical_atoms: crate::heap::ThinBoxSlice<Atom>,
    /// What a Script or eval body declares in the global scope; `None` for every other
    /// function, which declares nothing there.
    pub global_declarations: Option<Box<GlobalDeclarations>>,
    /// Bindings a dynamic name lookup in this function can see.
    pub(crate) name_bindings: NameBindings,
    /// Lexical projections at eval calls and dynamic name operations.
    pub binding_sites: crate::heap::ThinBoxSlice<BindingSite>,
    pub(crate) source_positions: SourcePositions,
    /// The plan of each CloneEnv instruction.
    pub environment_clones: crate::heap::ThinBoxSlice<EnvironmentClone>,
    pub code: Vec<Instr>,
    pub(crate) wide: Vec<WideInstruction>,
    pub registers: u16,
    pub(crate) dispatch: DispatchClass,
    /// Fixed-width view of `code`, derived once the function proves hot.
    pub(crate) decoded: HotDecoding,
    /// Lazily derived `plain_local_slots`; never serialized.
    pub(crate) plain_locals: std::cell::OnceCell<Box<[bool]>>,
    /// Lazily derived `declared_atoms`; never serialized.
    pub(crate) declared_atom_sets: std::cell::OnceCell<Box<DeclaredAtomSets>>,
    /// Lazily derived `activation_bindings_observable`; never serialized.
    pub(crate) activation_bindings: std::cell::OnceCell<bool>,
    /// Lazily derived `reflects_live_parameters`; never serialized.
    pub(crate) live_parameter_reflection: std::cell::OnceCell<bool>,
    /// Lazily derived `names_resolve_statically`; never serialized.
    pub(crate) static_name_scope: std::cell::OnceCell<bool>,
    /// Lazily derived `has_dynamic_scope_access`; never serialized.
    pub(crate) dynamic_scope_access: std::cell::OnceCell<bool>,
    /// Whether the JavaScript tier's candidate filter admits the function
    /// (`js_compiled::function_candidate`), derived on its first entry; never serialized.
    pub(crate) js_tier_candidate: std::cell::OnceCell<bool>,
    /// Lazily derived `call_plan`; never serialized.
    pub(crate) call_plan: std::cell::OnceCell<CallPlan>,
    /// Lazily derived `initializer_stores`; never serialized.
    pub(crate) initializer: std::cell::OnceCell<Option<Box<[InitializerStore]>>>,
    pub(crate) handlers: crate::heap::ThinBoxSlice<Handler>,
    pub(crate) register_roots: RegisterRoots,
    /// A register that holds `undefined` for the whole activation, set at frame entry and
    /// never written by code: the receiver of plain calls.
    pub(crate) undefined_register: Option<Register>,
    /// First register of the parameters when the body keeps its locals in
    /// registers; activation then writes arguments there, not into locals.
    pub(crate) parameter_registers: Option<u16>,
    /// The value every register holds when an activation starts.
    pub(crate) initial_register: InitialRegister,
}

/// The facts about a function that are read before its first call: when its closure is
/// made, by `IsConstructor` and brand checks, by `Function.prototype.toString`, and by an
/// enclosing activation that asks whether a closure it makes is an arrow. A projection of
/// the compiled record (`FunctionHeader::of`), so a slot answers them without its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FunctionHeader {
    pub(crate) parent: Option<u32>,
    pub(crate) name: Option<Atom>,
    pub(crate) inferred_name: Option<Atom>,
    pub(crate) source_span: Option<SourceSpan>,
    pub(crate) length: u16,
    pub(crate) dispatch: DispatchClass,
    pub(crate) is_arrow: bool,
    pub(crate) is_async: bool,
    pub(crate) is_generator: bool,
    pub(crate) is_class_constructor: bool,
    pub(crate) constructible: bool,
    pub(crate) prototype_unobservable: bool,
    pub(crate) inherited_with_scope: bool,
}

impl FunctionHeader {
    pub(crate) fn of(function: &Function) -> Self {
        Self {
            parent: function.parent,
            name: function.name,
            inferred_name: function.inferred_name,
            source_span: function.source_span,
            length: function.length,
            dispatch: function.dispatch,
            is_arrow: function.is_arrow,
            is_async: function.is_async,
            is_generator: function.is_generator,
            is_class_constructor: function.is_class_constructor,
            constructible: function.constructible,
            prototype_unobservable: function.prototype_unobservable,
            inherited_with_scope: function.inherited_with_scope,
        }
    }
}

/// One entry of a program's function table: the header, always present, and the compiled
/// record, written once. Reading the record through `Deref` requires that it was compiled.
/// Clones of a program share its records (installing an entry clones it into the VM); a
/// change through `DerefMut` copies a shared record first.
///
/// A slot is in one of three states: compiled (`record` set); unlowered, holding the record
/// its load finished but for the lowering its first read completes (`compile::lower_settled`);
/// or released, its first read compiling the record again from source or decoding it.
#[derive(Clone, Debug)]
pub struct FunctionSlot {
    header: FunctionHeader,
    record: std::cell::OnceCell<std::rc::Rc<Function>>,
    unlowered: std::cell::RefCell<Option<std::rc::Rc<Function>>>,
}

impl FunctionSlot {
    pub(crate) fn compiled(function: Function) -> Self {
        Self {
            header: FunctionHeader::of(&function),
            record: std::cell::OnceCell::from(std::rc::Rc::new(function)),
            unlowered: Default::default(),
        }
    }

    /// A function whose record its program compiles again on the function's first read.
    pub(crate) fn released(header: FunctionHeader) -> Self {
        Self {
            header,
            record: std::cell::OnceCell::new(),
            unlowered: Default::default(),
        }
    }

    /// A function whose load settled its dispatch class, the header's last fact, and left
    /// the rest of its lowering to its first read.
    pub(crate) fn unlowered(function: Function) -> Self {
        Self {
            header: FunctionHeader::of(&function),
            record: std::cell::OnceCell::new(),
            unlowered: std::cell::RefCell::new(Some(std::rc::Rc::new(function))),
        }
    }

    /// Whether the slot holds no record, compiled or unlowered: its first read compiles
    /// the record again or decodes it.
    #[cfg(test)]
    pub(crate) fn is_released(&self) -> bool {
        self.record.get().is_none() && self.unlowered.borrow().is_none()
    }

    #[inline(always)]
    pub(crate) fn header(&self) -> &FunctionHeader {
        &self.header
    }

    /// The compiled record, when it exists.
    #[inline(always)]
    pub(crate) fn record(&self) -> Option<&Function> {
        self.record.get().map(|function| &**function)
    }

    /// Drops the record, which its program compiles again on the function's next read.
    fn release(&mut self) {
        self.record.take();
        self.unlowered.get_mut().take();
    }
}

impl std::ops::Deref for FunctionSlot {
    type Target = Function;

    #[inline(always)]
    fn deref(&self) -> &Function {
        self.record()
            .expect("a function's record is read only once it is compiled")
    }
}

impl std::ops::DerefMut for FunctionSlot {
    fn deref_mut(&mut self) -> &mut Function {
        std::rc::Rc::make_mut(
            self.record
                .get_mut()
                .expect("a function's record is changed only once it is compiled"),
        )
    }
}

impl From<Function> for FunctionSlot {
    fn from(function: Function) -> Self {
        Self::compiled(function)
    }
}

/// Call-setup facts derived once per function from its static layout, so an activation reads
/// one cached word instead of re-testing each flag.
/// What a constructor that only constructs its parent passes on
/// (`Function::super_forwarding`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SuperForwarded {
    /// Every value it was passed.
    Passed,
    /// Its parameters, those not passed `undefined`.
    Parameters(u16),
}

/// How an activation keeps its `arguments` binding as the values it was passed
/// (`Function::kept_arguments`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeptArguments {
    /// Every read answers from the values or forwards them, as an unmaterialized binding.
    Forwards,
    /// Such reads, then a read that makes the object, which no read of the binding follows.
    Passes,
}

/// The facts before the call plan in `Function::derived_facts`' packing.
const DERIVED_FACT_COUNT: u32 = 4;

/// `CallPlan`'s flags as bits, in this order; the struct literal names every field, so a
/// field left out of the list does not compile.
macro_rules! call_plan_bits {
    ($($field:ident),* $(,)?) => {
        impl CallPlan {
            fn to_bits(self) -> u32 {
                let mut bits = 0;
                let mut index = 0;
                $(bits |= u32::from(self.$field) << index; index += 1;)*
                let _ = index;
                bits
            }

            fn from_bits(bits: u32) -> Self {
                let mut index = 0;
                $(let $field = bits & (1 << index) != 0; index += 1;)*
                let _ = index;
                Self { $($field),* }
            }
        }
    };
}
call_plan_bits!(
    plain,
    in_loop,
    settles_in_loop,
    legacy_reflection,
    live_parameters,
    addresses_locals,
    forwards_arguments,
    keeps_arguments,
    reads_passed_arguments,
    registers_only,
    environment_activation,
);

#[derive(Clone, Copy, Debug)]
pub(crate) struct CallPlan {
    /// `plain_activation`.
    pub(crate) plain: bool,
    /// The dispatch loop may push the activation in place: an ordinary body that is no
    /// generator, async function, class constructor or field initializer and raises no
    /// parameter-scope `arguments` error.
    pub(crate) in_loop: bool,
    /// An async function whose body never suspends (`Function::suspends`): a plain call may
    /// push it in place as `in_loop` bodies are pushed, settling the promise it returns with
    /// the body's completion (`PendingGeneralCall::settles`).
    pub(crate) settles_in_loop: bool,
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
    /// `arguments` object until a call needs one (`Function::kept_arguments`).
    pub(crate) forwards_arguments: bool,
    /// `forwards_arguments`, or a frameless activation answers the binding from the values
    /// it kept and makes the object only at its last read (`KeptArguments::Passes`). Its
    /// frame makes the object on entry.
    pub(crate) keeps_arguments: bool,
    /// `Function::reads_passed_arguments_only`: an environment activation keeps every value
    /// passed after its locals, past its parameters too.
    pub(crate) reads_passed_arguments: bool,
    /// The activation's whole state is its registers, plus the values it was passed when it
    /// forwards them: it reaches no local but a forwarded `arguments` binding, and legacy
    /// reflection, if any, reads its parameter registers or the forwarded values. A lane
    /// may run such a body without a frame.
    pub(crate) registers_only: bool,
    /// The activation's state is its registers, its frame locals and the environment its
    /// closures capture, which it may make on entry: an ordinary body but for the closures
    /// it makes, with no `arguments`, self binding, handler, binding site, environment clone,
    /// dynamic scope access or superinstruction, whose every local access is a plain or
    /// environment load, store or dead-zone opening of a selectively captured layout. A lane
    /// may run such a body without a frame (`CallEntry::Environment`).
    pub(crate) environment_activation: bool,
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

/// The name of a parameter slot whose argument a list's patterns or defaults bind apart
/// (`FunctionCompiler::parameter_name`): an argument's own slot, which no source name reaches.
pub(crate) const HIDDEN_PARAMETER_PREFIX: &str = "\0quench:param:";

/// Whether register liveness covers a function's register forms, so a frame may leave its
/// dead registers stale; the pcs of its `Call`s whose result register is dead after the
/// call, which the compiler derives once from the same liveness (`liveness::mark_register_roots`);
/// and the per-pc live register masks the collector scans, derived when a collection first
/// finds the function on the stack. Most of a program's functions never run during a
/// collection, so their masks are never built. The loop tier's regions share them
/// (`osr_loop::LoopRegion::live`) rather than copying a function's masks for each loop.
#[derive(Clone, Debug, Default)]
pub(crate) struct RegisterRoots {
    covered: bool,
    dead_call_results: Box<[u32]>,
    masks: std::cell::OnceCell<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
}

impl RegisterRoots {
    pub(crate) fn new(covered: bool) -> Self {
        Self::with_dead_call_results(covered, Box::default())
    }

    /// `dead_call_results` in increasing pc order.
    pub(crate) fn with_dead_call_results(covered: bool, dead_call_results: Box<[u32]>) -> Self {
        Self { covered, dead_call_results, masks: std::cell::OnceCell::new() }
    }

    pub(crate) fn covered(&self) -> bool {
        self.covered
    }

    pub(crate) fn dead_call_results(&self) -> &[u32] {
        &self.dead_call_results
    }

    /// Whether the `Call` at `pc` leaves its result register dead.
    pub(crate) fn call_result_dead(&self, pc: usize) -> bool {
        u32::try_from(pc).is_ok_and(|pc| self.dead_call_results.binary_search(&pc).is_ok())
    }

    /// The masks a collection derived so far; none until one finds the function running.
    #[cfg(feature = "profile-memory")]
    pub(crate) fn capacity_bytes(&self) -> usize {
        self.masks.get().map_or(0, |masks| masks.bytes())
    }
}


/// A function's source positions in increasing pc order, each delta-coded against the one
/// before it as LEB128 varints: the pc advance, the line change (zigzag), then the column,
/// as a change (zigzag) on the same line and absolute on a new one. Most entries take three
/// bytes, a quarter of a `SourcePosition`, and a function keeps one per source location
/// change while it compiles and for as long as it lives.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourcePositions {
    bytes: Vec<u8>,
    /// The final entry, which the next one is coded against.
    last: Option<SourcePosition>,
    /// Where decoding may start, made on the first lookup of a table longer than one interval
    /// (`SourcePositions::at`): a stack trace through a large function otherwise decoded its
    /// table from the start for every frame (Babel: about 20K instructions a frame).
    checkpoints: std::cell::OnceCell<Box<[SourcePositionCheckpoint]>>,
}

/// The entries between two `SourcePositions` checkpoints.
const SOURCE_POSITION_CHECKPOINT_INTERVAL: usize = 32;

/// A point decoding may resume from: the byte offset of an entry, the entry before it
/// (which it is coded against) and the entry's own pc.
#[derive(Clone, Copy, Debug)]
struct SourcePositionCheckpoint {
    offset: usize,
    previous: SourcePosition,
    pc: u32,
}

/// Two tables are equal when their entries are; the checkpoints are a memo of them.
impl PartialEq for SourcePositions {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes && self.last == other.last
    }
}
impl Eq for SourcePositions {}

/// The payload bits of one LEB128 byte; the high bit marks a continuation.
const LEB128_PAYLOAD_BITS: u32 = 7;
const LEB128_CONTINUATION: u8 = 1 << LEB128_PAYLOAD_BITS;

impl SourcePositions {
    pub(crate) fn last(&self) -> Option<SourcePosition> {
        self.last
    }

    /// The coded entries and the final one, as a residual stores them
    /// (`BinaryWriter::source_positions`).
    pub(crate) fn encoded(&self) -> (&[u8], Option<SourcePosition>) {
        (&self.bytes, self.last)
    }

    /// The table a residual stored (`encoded`); validation judges it against its function.
    pub(crate) fn from_encoded(bytes: Vec<u8>, last: Option<SourcePosition>) -> Self {
        Self { bytes, last, checkpoints: std::cell::OnceCell::new() }
    }

    /// Appends `position`. Compiled code orders positions by pc; a decoded table keeps any
    /// order exactly, for validation to judge.
    pub(crate) fn push(&mut self, position: SourcePosition) {
        let previous = self.last.unwrap_or(SourcePosition { pc: 0, line: 0, column: 0 });
        let line_change = i64::from(position.line) - i64::from(previous.line);
        self.write(u64::from(position.pc.wrapping_sub(previous.pc)));
        self.write(zigzag(line_change));
        self.write(if line_change == 0 {
            zigzag(i64::from(position.column) - i64::from(previous.column))
        } else {
            u64::from(position.column)
        });
        self.last = Some(position);
        self.checkpoints = std::cell::OnceCell::new();
    }

    fn write(&mut self, mut value: u64) {
        loop {
            let byte = (value & u64::from(!LEB128_CONTINUATION)) as u8;
            value >>= LEB128_PAYLOAD_BITS;
            if value == 0 {
                self.bytes.push(byte);
                return;
            }
            self.bytes.push(byte | LEB128_CONTINUATION);
        }
    }

    pub(crate) fn iter(&self) -> SourcePositionIter<'_> {
        self.iter_from(0, SourcePosition { pc: 0, line: 0, column: 0 })
    }

    fn iter_from(&self, offset: usize, previous: SourcePosition) -> SourcePositionIter<'_> {
        SourcePositionIter { bytes: &self.bytes[offset..], previous }
    }

    /// The position in effect at `pc`: the last entry at or before it, decoded from the last
    /// checkpoint at or before it.
    pub(crate) fn at(&self, pc: u32) -> Option<SourcePosition> {
        let checkpoints = self.checkpoints.get_or_init(|| self.make_checkpoints());
        let start = checkpoints.partition_point(|checkpoint| checkpoint.pc <= pc);
        let mut iter = match start.checked_sub(1) {
            Some(index) => self.iter_from(checkpoints[index].offset, checkpoints[index].previous),
            None => self.iter(),
        };
        let mut found = None;
        for position in iter.by_ref() {
            if position.pc > pc {
                break;
            }
            found = Some(position);
        }
        found
    }

    /// A checkpoint at every `SOURCE_POSITION_CHECKPOINT_INTERVAL`th entry after the first;
    /// none for a table shorter than one interval, which decodes as fast from its start.
    fn make_checkpoints(&self) -> Box<[SourcePositionCheckpoint]> {
        let mut checkpoints = Vec::new();
        let mut iter = self.iter();
        let mut entry = 0;
        loop {
            let offset = self.bytes.len() - iter.bytes.len();
            let previous = iter.previous;
            let Some(position) = iter.next() else { break };
            if entry > 0 && entry % SOURCE_POSITION_CHECKPOINT_INTERVAL == 0 {
                checkpoints.push(SourcePositionCheckpoint { offset, previous, pc: position.pc });
            }
            entry += 1;
        }
        checkpoints.into_boxed_slice()
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.bytes.shrink_to_fit();
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn capacity_bytes(&self) -> usize {
        self.bytes.capacity()
    }
}

impl FromIterator<SourcePosition> for SourcePositions {
    fn from_iter<I: IntoIterator<Item = SourcePosition>>(positions: I) -> Self {
        let mut table = Self::default();
        for position in positions {
            table.push(position);
        }
        table
    }
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> (i64::BITS - 1))) as u64
}

fn unzigzag(value: u64) -> i64 {
    (value >> 1) as i64 ^ -((value & 1) as i64)
}

/// Decodes a `SourcePositions` table in order.
#[derive(Clone)]
pub(crate) struct SourcePositionIter<'a> {
    bytes: &'a [u8],
    previous: SourcePosition,
}

impl SourcePositionIter<'_> {
    fn read(&mut self) -> u64 {
        let mut value = 0;
        let mut shift = 0;
        while let Some((&byte, rest)) = self.bytes.split_first() {
            self.bytes = rest;
            // A stored table is validated after decoding; an overlong varint decodes to junk
            // the validation rejects rather than shifting past the value's width.
            value |= u64::from(byte & !LEB128_CONTINUATION).checked_shl(shift).unwrap_or(0);
            if byte & LEB128_CONTINUATION == 0 {
                break;
            }
            shift += LEB128_PAYLOAD_BITS;
        }
        value
    }
}

impl Iterator for SourcePositionIter<'_> {
    type Item = SourcePosition;

    fn next(&mut self) -> Option<SourcePosition> {
        if self.bytes.is_empty() {
            return None;
        }
        let previous = self.previous;
        let pc = previous.pc.wrapping_add(self.read() as u32);
        let line_change = unzigzag(self.read());
        let column = self.read();
        let column = if line_change == 0 {
            (i64::from(previous.column) + unzigzag(column)) as u32
        } else {
            column as u32
        };
        let line = (i64::from(previous.line) + line_change) as u32;
        self.previous = SourcePosition { pc, line, column };
        Some(self.previous)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlainLocalContextIneligibility {
    RootFunction,
    LegacyCaptureLayout,
    DynamicNameResolution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlainLocalSlotIneligibility {
    MissingAtom,
    MappedArgument,
    SelfBinding,
    Captured,
}

impl Function {
    /// Releases the spare capacity of the tables a compilation grew, once it is finished.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.code.shrink_to_fit();
        self.wide.shrink_to_fit();
        self.source_positions.shrink_to_fit();
    }

    fn global_declarations_or_none(&self) -> Option<&GlobalDeclarations> {
        self.global_declarations.as_deref()
    }

    pub fn global_lexical_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.lexical)
    }

    pub fn global_var_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.var)
    }

    pub fn global_function_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.function)
    }

    pub fn global_annex_b_var_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.annex_b_var)
    }

    /// The slots a module body's environment holds for its importers and linker: its imports
    /// (whatever their phase), its exported locals and its hoisted functions, which `link_plan`
    /// names and the linker addresses by slot. The body's other bindings are its own.
    pub(crate) fn module_environment_slots<'a>(
        &'a self,
        link_plan: &'a ModuleLinkPlan,
        imports: &'a [ModuleImportBinding],
        atom_text: impl Fn(Atom) -> Option<&'a str> + 'a,
    ) -> impl Iterator<Item = u16> + 'a {
        // A set, once: a large module's locals against its whole link plan was quadratic.
        let linked: rustc_hash::FxHashSet<&str> = imports
            .iter()
            .map(|import| import.local.as_str())
            .chain(link_plan.locals.iter().chain(&link_plan.hoisted_functions).map(|(local, _)| local.as_str()))
            .collect();
        self.local_atoms
            .iter()
            .enumerate()
            .filter(move |(_, atom)| atom_text(**atom).is_some_and(|name| linked.contains(name)))
            .filter_map(|(slot, _)| u16::try_from(slot).ok())
    }

    pub fn global_immutable_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.immutable)
    }

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

    /// Whether the `arguments` object's indices alias the parameters: a mapped object
    /// whose mapping some write could observe (`unaliased_arguments`).
    pub(crate) fn arguments_alias_parameters(&self) -> bool {
        self.arguments_are_mapped() && !self.unaliased_arguments
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
        let selective = self.selective_capture_slots.is_some();
        // A root's locals are its script's global bindings, unless it is a module body whose
        // activation splits (only a module body's may): then its uncaptured locals are its own.
        if self.parent.is_none() && !selective {
            return Some(PlainLocalContextIneligibility::RootFunction);
        }
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
    /// with no environment, mapped-argument or dynamic-scope observer, and no read inside a
    /// temporal dead zone (`initialization`). Compilation specializes these slots'
    /// operations, validation re-derives the same proof, and the interpreter memoizes it in
    /// `plain_locals` to read plain local operands directly.
    pub(crate) fn plain_local_slots(&self, superinstructions: &[Superinstruction]) -> Vec<bool> {
        let mut plain = vec![false; usize::from(self.locals)];
        if !self.plain_local_context_is_safe() {
            return plain;
        }
        let mut dead_zone = vec![false; usize::from(self.locals)];
        for instruction in self.instructions() {
            if instruction.op() == Op::InitializeTdz
                && let Some(slot) = dead_zone.get_mut(instruction.local_slot())
            {
                *slot = true;
            }
        }
        for (slot, plain) in plain.iter_mut().enumerate() {
            *plain = self.plain_local_slot_is_safe(slot);
        }
        for (slot, dead_zone) in dead_zone.iter_mut().enumerate() {
            *dead_zone &= plain[slot];
        }
        let initialized = initialization::initialized_before_reads(self, superinstructions, &dead_zone, &[]);
        for (slot, plain) in plain.iter_mut().enumerate() {
            *plain &= !dead_zone[slot] || initialized[slot];
        }
        plain
    }

    fn declared_atoms(&self) -> &DeclaredAtomSets {
        self.declared_atom_sets.get_or_init(|| {
            let global_lexical: rustc_hash::FxHashSet<Atom> =
                self.global_lexical_atoms().iter().copied().collect();
            let global_var: rustc_hash::FxHashSet<Atom> =
                self.global_var_atoms().iter().copied().collect();
            let slots = self
                .local_atoms
                .iter()
                .map(|atom| SlotDeclaration {
                    global_var: global_var.contains(atom),
                    global_lexical: global_lexical.contains(atom),
                })
                .collect();
            Box::new(DeclaredAtomSets {
                global_lexical,
                global_var,
                global_immutable: self.global_immutable_atoms().iter().copied().collect(),
                environment: self.environment_atoms.iter().copied().collect(),
                slots,
            })
        })
    }

    /// The declaration kinds of local `slot`'s name (none for a slot outside the function).
    pub(crate) fn slot_declaration(&self, slot: usize) -> SlotDeclaration {
        self.declared_atoms()
            .slots
            .get(slot)
            .copied()
            .unwrap_or_default()
    }

    /// Whether `atom` is among `global_lexical_atoms`.
    pub(crate) fn declares_global_lexical(&self, atom: Atom) -> bool {
        self.declared_atoms().global_lexical.contains(&atom)
    }

    /// Whether `atom` is among `global_var_atoms`.
    pub(crate) fn declares_global_var(&self, atom: Atom) -> bool {
        self.declared_atoms().global_var.contains(&atom)
    }

    /// Whether `atom` is among `global_immutable_atoms`.
    pub(crate) fn declares_global_immutable(&self, atom: Atom) -> bool {
        self.declared_atoms().global_immutable.contains(&atom)
    }

    /// Whether `atom` is among `environment_atoms`.
    pub(crate) fn declares_environment_atom(&self, atom: Atom) -> bool {
        self.declared_atoms().environment.contains(&atom)
    }

    /// Whether anything can read an activation's implicit `new.target` and lexical-this
    /// bindings: by-name resolution or direct eval in the body, closures or cloned scopes
    /// that can capture the activation, `with` scopes, and constructors, initializers,
    /// generators or async bodies whose `this` or activation outlives ordinary dispatch.
    /// Otherwise the frame's own `this` is authoritative and the bindings are never read.
    /// By-name loads of source names resolve only their own atom; the implicit `this` and
    /// `new.target` bindings are reachable by name only through engine-internal atoms.
    /// Whether the activation's implicit `this` and `new.target` bindings can be read: by
    /// its own name lookups (`activation_binding_reads`) or by an arrow function it makes,
    /// which inherits them. Any other closure has its own. `functions` is the owning
    /// program's table.
    pub(crate) fn activation_bindings_observable(
        &self,
        atoms: &AtomTable,
        functions: &[FunctionSlot],
    ) -> bool {
        *self.activation_bindings.get_or_init(|| self.derive_activation_bindings_observable(atoms, functions))
    }

    fn derive_activation_bindings_observable(&self, atoms: &AtomTable, functions: &[FunctionSlot]) -> bool {
        self.activation_binding_reads(atoms)
            || self.instructions().any(|instruction| {
                instruction.op() == Op::MakeClosure
                    && functions
                        .get(instruction.closure_function_index() as usize)
                        .is_none_or(|closure| closure.header().is_arrow)
            })
    }

    /// Whether the activation itself may read its implicit bindings through a name or scope
    /// operation, or must keep them for its kind of activation.
    fn activation_binding_reads(&self, atoms: &AtomTable) -> bool {
        self.is_class_constructor || self.binding_reads_beside_class(atoms)
    }

    /// `activation_binding_reads` but for a class constructor's own kind: a base class
    /// constructor's body that reaches none of its implicit bindings (`new.target`, `super`
    /// and the like read hidden names) needs none to run frameless when constructed.
    fn binding_reads_beside_class(&self, atoms: &AtomTable) -> bool {
        self.is_async
            || self.is_generator
            || self.derived_constructor
            // A field initializer or static block keeps its instance's implicit bindings; an
            // arrow inside one (also `class_initializer`) binds none of its own.
            || (self.class_initializer && !self.is_arrow)
            || self.instance_initializer.is_some()
            || self.inherited_with_scope
            || !self.binding_sites.is_empty()
            || self.instructions().any(|instruction| {
                matches!(
                    instruction.op(),
                    Op::LoadName | Op::LoadNameCall | Op::LoadNameTypeof
                ) && usize::try_from(instruction.atom_index()).is_ok_and(|atom| {
                    atom >= atoms.len()
                        || atoms[atom].starts_with('\0') && !is_intrinsic_name(&atoms[atom])
                })
                    || matches!(
                        instruction.op(),
                        Op::ResolveName
                            | Op::LoadResolvedName
                            | Op::DeleteName
                            | Op::StoreName
                            | Op::StoreResolvedName
                            | Op::StoreVarBinding
                            | Op::CloneEnv
                            | Op::InitializeThis
                            | Op::SuperCallCheck
                    )
                    || (instruction.op().is_call_with_eval_flags()
                        && (instruction.direct_eval() || instruction.parameter_eval()))
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
    pub(crate) fn reflects_live_parameters(&self, program: &ResidualProgram) -> bool {
        *self.live_parameter_reflection.get_or_init(|| self.derive_reflects_live_parameters(program))
    }

    fn derive_reflects_live_parameters(&self, program: &ResidualProgram) -> bool {
        {
            let atoms = &program.atoms;
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
                    self.plain_local(program, usize::from(parameter))
                        || self
                            .local_registers
                            .iter()
                            .any(|entry| entry.local == parameter)
                })
        }
    }

    /// Whether an activation is fully described by the function's static layout: a nested,
    /// non-derived function whose parameters all live in registers, with no rest
    /// array, self binding, `arguments` object, parameter-scope `arguments` error, inherited
    /// `with` scope or observable implicit bindings. Such a frame needs only its storage sized
    /// (cleared when no root map lets the collector skip stale registers), its parameters
    /// placed and its scalar state set; see `activate_plain_frame`.
    pub(crate) fn plain_activation(&self, program: &ResidualProgram) -> bool {
        self.call_plan(program).plain
    }

    /// Whether every parameter slot is named by its binding rather than a hidden parameter
    /// name, as a list that binds directly names them (`FunctionCompiler::binds_parameters_directly`):
    /// a rest list of identifiers then fills its last slot with the rest array and binds
    /// nothing else.
    /// `name` spells an atom of the function's table (the program's, or the compiler's while
    /// it lays the function out).
    fn parameters_bind_directly<'n>(&self, name: impl Fn(usize) -> Option<&'n str>) -> bool {
        self.local_atoms[..usize::from(self.params).min(self.local_atoms.len())]
            .iter()
            .all(|atom| usize::try_from(*atom).ok().and_then(&name).is_some_and(|name| !name.starts_with('\0')))
    }

    /// Whether each parameter lives in its own named slot: a simple list, or a rest list that
    /// binds directly (`parameters_bind_directly`). Selective capture layouts, environment
    /// activations and promoted locals all require it, and the compiler and the validator
    /// read this one rule.
    pub(crate) fn parameters_own_their_slots<'n>(&self, name: impl Fn(usize) -> Option<&'n str>) -> bool {
        self.simple_parameters || self.rest && self.parameters_bind_directly(name)
    }

    /// Whether each argument arrives in a slot of its own that only the activation's own
    /// loads and stores reach: `parameters_own_their_slots`, or a list without a rest
    /// parameter whose patterns or defaults bind its names apart, each argument then in its
    /// hidden parameter slot (`HIDDEN_PARAMETER_PREFIX`). Promoted locals require it.
    pub(crate) fn arguments_own_their_slots<'n>(&self, name: impl Fn(usize) -> Option<&'n str>) -> bool {
        self.parameters_own_their_slots(&name)
            || !self.rest
                && self.local_atoms[..usize::from(self.params).min(self.local_atoms.len())].iter().all(|atom| {
                    usize::try_from(*atom)
                        .ok()
                        .and_then(&name)
                        .is_some_and(|name| name.starts_with(HIDDEN_PARAMETER_PREFIX))
                })
    }

    /// Whether an activation can suspend: an `await`, or a `yield` of a generator.
    pub(crate) fn suspends(&self) -> bool {
        self.instructions()
            .any(|instruction| matches!(instruction.op(), Op::Await | Op::Yield | Op::YieldStar))
    }

    /// The function's `CallPlan`, derived on first use.
    #[inline(always)]
    pub(crate) fn call_plan(&self, program: &ResidualProgram) -> CallPlan {
        *self.call_plan.get_or_init(|| self.derive_call_plan(program))
    }

    fn derive_call_plan(&self, program: &ResidualProgram) -> CallPlan {
        {
            let atoms = &program.atoms;
            let promoted_parameters = self
                .local_registers
                .iter()
                .filter(|entry| entry.local < self.params)
                .count();
            // Plain but for an `arguments` binding.
            // An arrow's `this` is its environment's, which its activation binds
            // (`Vm::lexical_activation_this`). One that reaches `super` reads a hidden name
            // (`activation_binding_reads`) or its home as a capture, as any closure reads one.
            // Ordinary but for a rest parameter, which a frameless activation gathers
            // (`CallEntry::Gathers`) and a plain frame does not.
            let registers_layout = self.parent.is_some()
                && !self.derived_constructor
                && self.self_binding_slot.is_none()
                && !self.parameter_eval_arguments_error
                && !self.inherited_with_scope
                && !self.is_generator
                && !self.is_async
                && promoted_parameters == usize::from(self.params)
                // A plain activation makes no closure, whichever bindings it would see.
                && !self.instructions().any(|instruction| instruction.op() == Op::MakeClosure);
            let ordinary_with_rest = registers_layout && !self.activation_binding_reads(atoms);
            // A base class constructor laid out as an ordinary function runs frameless when
            // constructed (`lane_script_construct`); its calls, which throw, keep a frame.
            let constructs_frameless = registers_layout
                && self.is_class_constructor
                && !self.rest
                && !self.binding_reads_beside_class(atoms);
            let ordinary = ordinary_with_rest && !self.rest;
            // A field initializer is an ordinary method call on the instance whose
            // activation keeps its implicit bindings (`activation_binding_reads`); only the
            // derived instance initializer, which `super()` runs, is entered natively.
            let in_loop = !self.is_async
                && !self.is_generator
                && !self.is_class_constructor
                && !self.derived_constructor
                && !self.parameter_eval_arguments_error;
            let settles_in_loop = self.is_async
                && !self.is_generator
                && !self.is_class_constructor
                && !self.derived_constructor
                && !self.parameter_eval_arguments_error
                && !self.suspends();
            let legacy_reflection = !self.has_restricted_legacy_caller_access();
            let live_parameters = self.reflects_live_parameters(program);
            let kept_arguments = self.kept_arguments();
            let forwards_arguments = kept_arguments == Some(KeptArguments::Forwards);
            let addresses_locals = self.instructions().any(|instruction| {
                matches!(instruction.op(), Op::MakeClosure | Op::CloneEnv)
                    || instruction.addressed_local_slots().next().is_some()
            });
            let reaches_only_forwarded_arguments = kept_arguments.is_some()
                && self.instructions().all(|instruction| {
                    instruction
                        .addressed_local_slots()
                        .all(|slot| Some(slot) == self.arguments_slot)
                });
            let environment_activation = in_loop
                && self.parent.is_some()
                && !self.is_arrow
                && !self.is_class_constructor
                // A rest list binding directly fills its slot with the gathered array
                // (`lane_activate_environment`).
                && self.parameters_own_their_slots(|atom| atoms.get(atom))
                && self.self_binding_slot.is_none()
                && !self.inherited_with_scope
                && self.local_registers.is_empty()
                && (self.arguments_slot.is_none() || self.reads_passed_arguments_only())
                && self.binding_sites.is_empty()
                && self.selective_capture_slots.is_some()
                && self.initial_register == InitialRegister::Undefined
                && !self.has_dynamic_scope_access()
                && !self.activation_binding_reads(atoms)
                && self.instructions().all(|instruction| {
                    let op = instruction.op();
                    op.immediate_role() != ImmediateRole::SuperinstructionIndex
                        && op != Op::CloneEnv
                        && (instruction.addressed_local_slots().next().is_none()
                            || matches!(
                                op,
                                Op::LoadLocal
                                    | Op::LoadLocalPlain
                                    | Op::LoadEnvLocal
                                    | Op::StoreLocal
                                    | Op::StoreLocalPlain
                                    | Op::StoreEnvLocal
                                    | Op::InitializeTdz
                            )
                            // A read whose base is a plain local reads the window's local
                            // (`lane_plain_local_operand`).
                            || (op == Op::GetField
                                && instruction.addressed_local_slots().eq(instruction.local_field_base()))
                            // The window keeps every value passed, which these answer.
                            || matches!(op, Op::ArgumentsLength | Op::ArgumentGet))
                });
            CallPlan {
                environment_activation,
                plain: ordinary && self.arguments_slot.is_none(),
                in_loop,
                settles_in_loop,
                legacy_reflection,
                live_parameters,
                addresses_locals,
                forwards_arguments,
                keeps_arguments: kept_arguments.is_some(),
                reads_passed_arguments: self.reads_passed_arguments_only(),
                registers_only: (ordinary_with_rest && in_loop || constructs_frameless)
                    && self.initial_register == InitialRegister::Undefined
                    && if self.arguments_slot.is_none() {
                        // A rest body's activation keeps every value passed
                        // (`CallEntry::Gathers`), which legacy reflection reads.
                        !addresses_locals && (!legacy_reflection || live_parameters || self.rest)
                    } else {
                        reaches_only_forwarded_arguments
                    },
            }
        }
    }

    /// The facts below and the call plan, each derived now and packed as an encoding keeps
    /// them (`binary::write_function`): derived lazily, they had cost 3-7% of the instructions
    /// of cached npm runs (axios, playwright), which each run derived again.
    pub(crate) fn derived_facts(&self, program: &ResidualProgram) -> u32 {
        let facts = [
            self.activation_bindings_observable(&program.atoms, &program.functions),
            self.reflects_live_parameters(program),
            self.names_resolve_statically(program),
            self.has_dynamic_scope_access(),
        ];
        let flags = facts.iter().enumerate().fold(0, |bits, (index, &fact)| bits | (u32::from(fact) << index));
        flags | (self.call_plan(program).to_bits() << DERIVED_FACT_COUNT)
    }

    /// Seeds the facts `derived_facts` packed; a fact already derived keeps its value.
    pub(crate) fn seed_derived_facts(&self, bits: u32) {
        let fact = |index: u32| bits & (1 << index) != 0;
        let _ = self.activation_bindings.set(fact(0));
        let _ = self.live_parameter_reflection.set(fact(1));
        let _ = self.static_name_scope.set(fact(2));
        let _ = self.dynamic_scope_access.set(fact(3));
        let _ = self.call_plan.set(CallPlan::from_bits(bits >> DERIVED_FACT_COUNT));
    }

    /// Whether each fact `derived_facts` packs is this function's own derivation, whether it
    /// was derived or seeded (`ResidualProgram::validate`).
    pub(crate) fn derived_facts_hold(&self, program: &ResidualProgram) -> bool {
        self.activation_bindings_observable(&program.atoms, &program.functions)
            == self.derive_activation_bindings_observable(&program.atoms, &program.functions)
            && self.reflects_live_parameters(program) == self.derive_reflects_live_parameters(program)
            && self.names_resolve_statically(program) == self.derive_names_resolve_statically(program)
            && self.has_dynamic_scope_access() == self.derive_dynamic_scope_access()
            && self.call_plan(program).to_bits() == self.derive_call_plan(program).to_bits()
    }

    /// Whether the body's whole effect is storing parameters into fields of `this`, each as
    /// passed or a Number in its place where it is falsy, then returning `undefined`
    /// (`initializer_stores`). `constants` is the owning program's table.
    pub(crate) fn only_initializes_this_fields(&self, constants: &[Constant]) -> bool {
        self.initializer_stores(constants).is_some()
    }

    /// The function's `initializer_stores`, derived on first use.
    #[inline(always)]
    pub(crate) fn initializer_stores(&self, constants: &[Constant]) -> Option<&[InitializerStore]> {
        self.initializer.get_or_init(|| self.derive_initializer_stores(constants)).as_deref()
    }

    /// The stores of a body whose whole effect is storing parameters into fields of `this`,
    /// then returning `undefined`, in order; `None` for any other body. A store's value is
    /// a parameter as passed (`SetThisField` of the parameter's register), or a Number
    /// constant in its place where the parameter is falsy: `this.f = (p ? p : k)` (a
    /// `JumpFalse` of the parameter over its `Move` into a temporary, else the constant's
    /// `LoadConst` into it, then the temporary's `SetThisField`) or `if (!p) p = k` before
    /// the parameter's stores (a `JumpTrue` of the parameter over the constant's `LoadConst`
    /// into it). The body completes with the return of an undefined constant or of a
    /// register nothing writes. In a plain activation, where parameter `k` lives in
    /// register `k` (`CallPlan::registers_only`), these stores are its whole effect.
    fn derive_initializer_stores(&self, constants: &[Constant]) -> Option<Box<[InitializerStore]>> {
        if !self.handlers.is_empty() {
            return None;
        }
        let instructions = self.instructions().collect::<Vec<_>>();
        let number = |load: &WideInstruction| match constants.get(load.constant_index()) {
            Some(Constant::Number(number)) if load.op() == Op::LoadConst => Some(*number),
            _ => None,
        };
        let parameter = |register: Register| (register < self.params).then_some(register);
        let mut fallbacks: SmallVec<[Option<f64>; 4]> = smallvec::smallvec![None; usize::from(self.params)];
        let mut stores = Vec::new();
        let mut pc = 0;
        let store = |instruction: &WideInstruction| {
            matches!(instruction.op(), Op::SetThisField | Op::SetThisFieldStrict).then_some(())
        };
        loop {
            let at = |offset: usize| instructions.get(pc + offset);
            let target = |offset: usize| (pc + offset) as u32;
            match at(0)? {
                // `if (!p) p = k`, before any store of `p`.
                jump if jump.op() == Op::JumpTrue
                    && let Some(p) = parameter(jump.register_a())
                    && jump.jump_target() == target(2)
                    && let Some(load) = at(1)
                    && load.result_register() == p
                    && let Some(k) = number(load)
                    && fallbacks[usize::from(p)].is_none()
                    && !stores.iter().any(|stored: &InitializerStore| stored.parameter == p) =>
                {
                    fallbacks[usize::from(p)] = Some(k);
                    pc += 2;
                }
                // `this.f = (p ? p : k)`.
                jump if jump.op() == Op::JumpFalse
                    && let Some(p) = parameter(jump.register_a())
                    && jump.jump_target() == target(3)
                    && let (Some(copy), Some(skip), Some(load), Some(set)) = (at(1), at(2), at(3), at(4))
                    && copy.op() == Op::Move
                    && copy.register_b() == p
                    && copy.result_register() >= self.params
                    && skip.op() == Op::Jump
                    && skip.jump_target() == target(4)
                    && load.result_register() == copy.result_register()
                    && let Some(k) = number(load)
                    && store(set).is_some()
                    && set.register_a() == copy.result_register()
                    && fallbacks[usize::from(p)].is_none() =>
                {
                    stores.push(InitializerStore { atom: set.imm(), site: set.c(), parameter: p, fallback: Some(k) });
                    pc += 5;
                }
                set if store(set).is_some() && let Some(p) = parameter(set.register_a()) => {
                    stores.push(InitializerStore { atom: set.imm(), site: set.c(), parameter: p, fallback: fallbacks[usize::from(p)] });
                    pc += 1;
                }
                _ => break,
            }
        }
        let undefined_load = |load: &WideInstruction| {
            load.op() == Op::LoadConst && matches!(constants.get(load.constant_index()), Some(Constant::Undefined))
        };
        let completes = match &instructions[pc..] {
            [completion] => {
                (undefined_load(completion) && completion.returns_from_frame())
                    || (completion.op() == Op::Return
                        && completion.register_a() >= self.params
                        && self.initial_register == InitialRegister::Undefined)
            }
            [load, completion] => {
                undefined_load(load)
                    && !load.returns_from_frame()
                    && completion.op() == Op::Return
                    && completion.register_a() == load.result_register()
            }
            _ => false,
        };
        // A defaulted parameter none of the stores reads is a store the model leaves out.
        let all_read = fallbacks.iter().enumerate().all(|(p, fallback)| {
            fallback.is_none() || stores.iter().any(|stored: &InitializerStore| usize::from(stored.parameter) == p)
        });
        (completes && all_read && !stores.is_empty()).then(|| stores.into_boxed_slice())
    }

    /// Whether the body reaches its `arguments` binding only by reading its length or an
    /// element (`ArgumentsLength`, `ArgumentGet`), with no closure capturing the binding, so
    /// the values passed and the parameters answer every read: an element a parameter
    /// aliases (`arguments_alias_parameters`) is that parameter's local, which no closure
    /// captures, and any other the value passed. An environment activation then keeps those
    /// values after its locals.
    pub(crate) fn reads_passed_arguments_only(&self) -> bool {
        let Some(slot) = self.arguments_slot else {
            return false;
        };
        !self.is_generator
            && !self.is_async
            && (!self.arguments_alias_parameters()
                || (0..usize::from(self.params)).all(|parameter| !self.local_slot_uses_environment(parameter)))
            && !self.local_slot_uses_environment(usize::from(slot))
            && !self.has_dynamic_scope_access()
            && self.instructions().all(|instruction| {
                matches!(instruction.op(), Op::ArgumentsLength | Op::ArgumentGet)
                    || !instruction
                        .addressed_local_slots()
                        .any(|addressed| addressed == slot)
            })
    }

    /// What a derived constructor whose whole body constructs its parent and returns the
    /// receiver passes on (`SuperForwarded`): the implicit `constructor(...args) {
    /// super(...args); }`, or `constructor(a, b) { super(a, b); }` with its parameters in
    /// order. Each reads the active function's parent (`SUPER_BASE_BINDING` of the
    /// `\0quench:super` binding) and makes the `super` construction, then binds the
    /// receiver. Without fields or private elements of its own (`instance_initializer`), its
    /// construction is the parent's with the class as `new.target`.
    pub(crate) fn super_forwarding(&self, atoms: &AtomTable, constants: &[Constant]) -> Option<SuperForwarded> {
        if !self.derived_constructor
            || !self.handlers.is_empty()
            || self.arguments_slot.is_some()
            || self.instance_initializer.is_some()
            || self.is_async
            || self.is_generator
        {
            return None;
        }
        let records = self.instructions().collect::<SmallVec<[WideInstruction; 16]>>();
        // The parent: the active function's [[GetPrototypeOf]], called with no receiver.
        let parent_of = |active: &WideInstruction, base: &WideInstruction, receiver: &WideInstruction, call: &WideInstruction| {
            active.op() == Op::LoadCapture
                && base.op() == Op::LoadName
                && usize::try_from(base.atom_index()).ok().and_then(|atom| atoms.get(atom)) == Some(SUPER_BASE_BINDING)
                && receiver.op() == Op::LoadConst
                && matches!(constants.get(receiver.constant_index()), Some(Constant::Undefined))
                && call.op() == Op::Call
                && call.b() == base.result_register()
                && call.c() == receiver.result_register()
                && ImmediateLayout::argument_count(call.imm()) == 1
                && ImmediateLayout::call_window_base(call.imm()) == active.result_register()
        };
        if self.rest {
            // The implicit constructor spreads its rest list, then returns the receiver; the
            // records past its return are unreachable.
            let [list, active, base, receiver, call, construct, binds, completion, ..] = records.as_slice() else {
                return None;
            };
            let shaped = self.params == 1
                && list.op() == Op::LoadLocalPlain
                && list.local_slot() == 0
                && parent_of(active, base, receiver, call)
                && construct.op() == Op::Construct
                && construct.is_super_construct()
                && construct.result_flags() == 0
                && construct.b() == call.result_register()
                && matches!(
                    construct.construct_arguments(),
                    crate::bytecode::ConstructArguments::Array(spread) if spread == list.result_register()
                )
                && binds.op() == Op::InitializeThis
                && binds.a() == construct.result_register()
                && completion.op() == Op::Return
                && completion.register_a() == construct.result_register();
            return shaped.then_some(SuperForwarded::Passed);
        }
        if !self.simple_parameters {
            return None;
        }
        let count = usize::from(self.params);
        let (head, tail) = records.split_at_checked(5)?;
        let (loads, tail) = tail.split_at_checked(count)?;
        let ([active, base, receiver, parent, spare], [construct, check, binds, undefined, completion]) =
            (head, tail)
        else {
            return None;
        };
        let undefined_load = |load: &WideInstruction| {
            load.op() == Op::LoadConst
                && load.result_flags() == 0
                && matches!(constants.get(load.constant_index()), Some(Constant::Undefined))
        };
        let crate::bytecode::ConstructArguments::Registers(window) = construct.construct_arguments() else {
            return None;
        };
        let forwards_parameters = usize::from(window.count) == count
            && loads.iter().enumerate().all(|(index, load)| {
                load.op() == Op::LoadLocalPlain
                    && load.result_flags() == 0
                    && load.local_slot() == index
                    && usize::from(load.result_register()) == usize::from(window.base) + index
            });
        let shaped = parent_of(active, base, receiver, parent)
            && undefined_load(spare)
            && construct.op() == Op::Construct
            && construct.is_super_construct()
            && construct.result_flags() == 0
            && construct.b() == parent.result_register()
            && forwards_parameters
            && check.op() == Op::SuperCallCheck
            && binds.op() == Op::InitializeThis
            && binds.a() == construct.result_register()
            && undefined_load(undefined)
            && completion.op() == Op::Return
            && completion.register_a() == undefined.result_register();
        shaped.then_some(SuperForwarded::Parameters(self.params))
    }

    /// The method a constructor body forwards its construction to, when the body is exactly
    /// `this.<method>.apply(this, arguments)` and the return of `undefined` (Prototype's
    /// `Class.create` constructor): its seven records read `this.<method>`, read `apply` from
    /// it, load `this` and the forwarded `arguments` binding into the call window, make the
    /// forwarding call, then return `undefined`. No handler covers them.
    pub(crate) fn forwarded_this_method(&self, constants: &[Constant]) -> Option<ForwardedThisMethod> {
        if !self.handlers.is_empty() {
            return None;
        }
        let arguments_slot = self.arguments_slot?;
        let records = self.instructions().collect::<SmallVec<[WideInstruction; 8]>>();
        let [method, apply, this, arguments, call, undefined, completion] = records.as_slice() else {
            return None;
        };
        let window = ImmediateLayout::call_window_base(call.imm());
        let plain_result = |record: &WideInstruction| record.result_flags() == 0;
        let shaped = method.op() == Op::GetField
            && method.b() == FieldBase::THIS.0
            && plain_result(method)
            && apply.op() == Op::GetField
            && apply.b() == method.result_register()
            && plain_result(apply)
            && this.op() == Op::LoadThis
            && plain_result(this)
            && this.result_register() == window
            && arguments.op() == Op::LoadLocalPlain
            && plain_result(arguments)
            && arguments.result_register() == window + 1
            && arguments.local_slot() == usize::from(arguments_slot)
            && call.op() == Op::CallForwardingArguments
            && call.b() == apply.result_register()
            && call.c() == method.result_register()
            && ImmediateLayout::argument_count(call.imm()) == FORWARDED_APPLY_ARGUMENTS
            && plain_result(call)
            && undefined.op() == Op::LoadConst
            && plain_result(undefined)
            && matches!(constants.get(undefined.constant_index()), Some(Constant::Undefined))
            && completion.op() == Op::Return
            && completion.register_a() == undefined.result_register();
        shaped.then(|| ForwardedThisMethod {
            method: (method.imm(), method.c()),
            apply: (apply.imm(), apply.c()),
            call_pc: FORWARDED_THIS_METHOD_CALL,
            call_result: call.a(),
        })
    }

    /// How an activation answers every read of the `arguments` binding from the values it
    /// was passed, if it can: each read loads it into the last argument of the
    /// `CallForwardingArguments` right after, reads its length or an element
    /// (`ArgumentsLength`, `ArgumentGet`), or, for `Passes`, loads the object as a value
    /// where no read of the binding follows (`arguments_passed_last`). Nothing else can
    /// reach the binding: no closure, environment clone, dynamic scope access or promoted
    /// register, and no suspension. An object it later materializes must equal one made on
    /// entry: unmapped, or mapped onto no parameters.
    pub(crate) fn kept_arguments(&self) -> Option<KeptArguments> {
        let slot = self.arguments_slot?;
        let instructions = self.instructions().collect::<Vec<_>>();
        // A mapped object's elements alias the parameters; its `length` does not, so a body
        // that reads only `arguments.length` cannot observe the mapping.
        let reads_only_length = instructions.iter().all(|instruction| {
            instruction.op() == Op::ArgumentsLength
                || !instruction.addressed_local_slots().any(|addressed| addressed == slot)
        });
        if self.is_generator
            || self.is_async
            || (self.arguments_alias_parameters() && self.params != 0 && !reads_only_length)
            || self.local_registers.iter().any(|entry| entry.local == slot)
            || self.has_dynamic_scope_access()
        {
            return None;
        }
        let mut forwarded = false;
        let mut passes = SmallVec::<[usize; 2]>::new();
        for (pc, instruction) in instructions.iter().enumerate() {
            if matches!(instruction.op(), Op::MakeClosure | Op::CloneEnv) {
                return None;
            }
            if !instruction
                .addressed_local_slots()
                .any(|addressed| addressed == slot)
            {
                continue;
            }
            let loads = matches!(instruction.op(), Op::LoadLocal | Op::LoadLocalPlain);
            let feeds_forwarding_call = loads
                && instructions.get(pc + 1).is_some_and(|next| {
                    next.op() == Op::CallForwardingArguments
                        && next.call_window().last() == Some(instruction.result_register())
                });
            let reads_kept_values =
                matches!(instruction.op(), Op::ArgumentsLength | Op::ArgumentGet);
            if feeds_forwarding_call || reads_kept_values {
                forwarded = true;
            } else if loads {
                passes.push(pc);
            } else {
                return None;
            }
        }
        if passes.is_empty() {
            return forwarded.then_some(KeptArguments::Forwards);
        }
        self.arguments_passed_last(&instructions, slot, &passes).then_some(KeptArguments::Passes)
    }

    /// Whether no read of the `arguments` binding in local `slot`, `passes` among them, can
    /// follow any of `passes`: the object a pass makes is the binding's last read, so no
    /// later read needs it. A handler's edges are not traced, so a body with one answers
    /// `false`.
    fn arguments_passed_last(&self, instructions: &[WideInstruction], slot: u16, passes: &[usize]) -> bool {
        if !self.handlers.is_empty() {
            return false;
        }
        let mut reached = vec![false; instructions.len()];
        let mut work = Vec::new();
        for &pass in passes {
            crate::bytecode::for_each_instruction_successor(pass, instructions[pass], |next| work.push(next));
        }
        while let Some(pc) = work.pop() {
            let Some(instruction) = instructions.get(pc) else {
                continue;
            };
            if std::mem::replace(&mut reached[pc], true) {
                continue;
            }
            if instruction.addressed_local_slots().any(|addressed| addressed == slot) {
                return false;
            }
            crate::bytecode::for_each_instruction_successor(pc, *instruction, |next| work.push(next));
        }
        true
    }

    /// Whether the function itself can reach bindings by name at run time: it was created
    /// inside a `with` scope, records by-name lexical sites, resolves names dynamically, or
    /// calls direct eval (also from a parameter scope).
    /// Derived once per function: lane views, capture layouts and name resolution each ask,
    /// and the instruction scan had cost 1.3% of the axios npm scenario's instructions.
    pub(crate) fn has_dynamic_scope_access(&self) -> bool {
        *self.dynamic_scope_access.get_or_init(|| self.derive_dynamic_scope_access())
    }

    fn derive_dynamic_scope_access(&self) -> bool {
        self.inherited_with_scope
            || !self.binding_sites.is_empty()
            || self.instructions().any(WideInstruction::resolves_dynamic_scope)
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
    /// `program` holds this function; an enclosing function whose record it has not read yet
    /// is read here.
    pub(crate) fn names_resolve_statically(&self, program: &ResidualProgram) -> bool {
        *self.static_name_scope.get_or_init(|| self.derive_names_resolve_statically(program))
    }

    fn derive_names_resolve_statically(&self, program: &ResidualProgram) -> bool {
        let atoms = &program.atoms;
        !self.has_dynamic_scope_access()
            && !self.contains_with_statement(|atom| atom < atoms.len() && &atoms[atom] == WITH_ENTER_BINDING)
            && self.parent.is_none_or(|parent| {
                program
                    .function_get(parent as usize)
                    .is_some_and(|parent| parent.names_resolve_statically(program))
            })
    }

    /// The memoized `plain_local_slots` projection for this residual's atoms.
    #[inline(always)]
    pub(crate) fn plain_local(&self, program: &ResidualProgram, slot: usize) -> bool {
        self.plain_locals
            .get_or_init(|| {
                self.plain_local_slots(&program.superinstructions)
                    .into_boxed_slice()
            })
            .get(slot)
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn plain_local_slot_is_safe(&self, slot: usize) -> bool {
        self.plain_local_slot_ineligibility(slot).is_none()
    }

    pub(crate) fn plain_local_slot_ineligibility(
        &self,
        slot: usize,
    ) -> Option<PlainLocalSlotIneligibility> {
        if self.local_atoms.get(slot).is_none() {
            return Some(PlainLocalSlotIneligibility::MissingAtom);
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
            && self.arguments_alias_parameters()
        {
            return Some(PlainLocalSlotIneligibility::MappedArgument);
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
    pub(crate) const ALL: [Self; 4] = [Self::Mutable, Self::Immutable, Self::FunctionName, Self::PrivateName];

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

/// One step of a nested literal of constants (`MakeLiteralTemplate`), run in order over a
/// stack of values: a constant pushes its value, and an array or an object replaces the
/// element or property values on top of the stack with a fresh container of them. A
/// template's steps leave exactly one value, the literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiteralStep {
    Constant(u32),
    /// An array of the given number of elements.
    Array(u32),
    /// An object of an object site's keys, one value each.
    Object(u32),
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

/// Where a handler stores the value thrown to it: a frame local, or the register late
/// frame-register promotion moved that local into (`compile::frame_registers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThrownValueSlot {
    Local(u16),
    Register(Register),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Handler {
    pub start: u32,
    pub end: u32,
    pub target: u32,
    pub slot: Option<ThrownValueSlot>,
    pub return_target: Option<u32>,
    pub return_slot: Option<u16>,
    pub with_depth: u16,
}

/// The argument count of a forwarding `apply` call: the receiver and `arguments`.
pub(crate) const FORWARDED_APPLY_ARGUMENTS: u16 = 2;

/// The record index of a `ForwardedThisMethod` body's forwarding call: after the reads of
/// the method and `apply` and the loads of `this` and `arguments`.
pub(crate) const FORWARDED_THIS_METHOD_CALL: u32 = 4;

/// A constructor body that forwards its construction to a method of `this`
/// (`Function::forwarded_this_method`): the atom and cache site of each read, and the
/// forwarding call's record index and result register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ForwardedThisMethod {
    pub(crate) method: (Atom, u16),
    pub(crate) apply: (Atom, u16),
    pub(crate) call_pc: u32,
    pub(crate) call_result: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct FieldBase(pub(crate) u16);

impl FieldBase {
    pub(crate) const THIS: Self = Self(u16::MAX);
    pub(crate) const NESTED: u16 = u16::MAX - 1;
    const LOCAL_FLAG: u16 = 0x4000;

    pub(crate) fn register(value: Register) -> Self {
        debug_assert!(value <= REGISTER_MASK);
        Self(value)
    }

    pub(crate) fn register_index(self) -> Option<Register> {
        (self.0 <= REGISTER_MASK).then_some(self.0)
    }

    pub(crate) fn local(value: u16) -> Option<Self> {
        (value <= REGISTER_MASK).then_some(Self(Self::LOCAL_FLAG | value))
    }

    pub(crate) fn local_index(self) -> Option<u16> {
        (self.0 & Self::LOCAL_FLAG != 0
            && self.0 & !(Self::LOCAL_FLAG | REGISTER_MASK) == 0)
            .then_some(self.0 & REGISTER_MASK)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldPart { First, Second, Sink }

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
        self.sources[match part { FieldPart::First => 0, FieldPart::Second => 1, FieldPart::Sink => 2 }]
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
    pub(crate) const PAYLOAD_MASK: u16 = (1 << Self::TAG_SHIFT) - 1;

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

/// The source text a program retains for diagnostics and `Function.prototype.toString`: its
/// own copy, static host text (an embedded library) the compiled source wrapped, or the text
/// its compile-cache entry keeps after its encoding (`binary::append_kept_source`), read in
/// place from the host's mapping.
#[derive(Clone, Debug)]
pub(crate) enum SourceText {
    Shared(std::sync::Arc<str>),
    Static(&'static str),
    Kept(KeptText),
}

/// Text inside kept bytes: valid UTF-8, checked when made (`KeptText::new`).
#[derive(Clone)]
pub(crate) struct KeptText {
    bytes: KeptBytes,
    range: std::ops::Range<usize>,
}

impl KeptText {
    /// The text `range` of `bytes` holds, if it is UTF-8.
    pub(crate) fn new(bytes: KeptBytes, range: std::ops::Range<usize>) -> Option<Self> {
        std::str::from_utf8((*bytes).as_ref().get(range.clone())?).ok()?;
        Some(Self { bytes, range })
    }
}

impl std::ops::Deref for KeptText {
    type Target = str;
    fn deref(&self) -> &str {
        // SAFETY: `new` checked that the range holds UTF-8, and kept bytes never change.
        unsafe { std::str::from_utf8_unchecked(&(*self.bytes).as_ref()[self.range.clone()]) }
    }
}

impl std::fmt::Debug for KeptText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("KeptText").field("len", &self.range.len()).finish()
    }
}

/// The first `len` bytes of kept bytes: a compile-cache entry's encoding, without the source
/// text after it.
struct KeptPrefix {
    bytes: KeptBytes,
    len: usize,
}

impl AsRef<[u8]> for KeptPrefix {
    fn as_ref(&self) -> &[u8] {
        &(*self.bytes).as_ref()[..self.len]
    }
}

/// A compile-cache entry split into its encoding and the source text
/// `binary::append_kept_source` kept after it, when it keeps `source_len` bytes of text whose
/// checksum holds; the entry itself otherwise.
pub(crate) fn split_kept_source(entry: KeptBytes, source_len: usize) -> Result<(KeptBytes, SourceText), KeptBytes> {
    let Some(range) = binary::kept_source_range((*entry).as_ref()).filter(|range| range.len() == source_len) else {
        return Err(entry);
    };
    let Some(text) = KeptText::new(entry.clone(), range.clone()) else {
        return Err(entry);
    };
    let encoding: KeptBytes = std::rc::Rc::new(KeptPrefix { bytes: entry, len: range.start });
    Ok((encoding, SourceText::Kept(text)))
}

pub(crate) use binary::append_kept_source;

impl std::ops::Deref for SourceText {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Self::Shared(text) => text,
            Self::Static(text) => text,
            Self::Kept(text) => text,
        }
    }
}

/// Compiler source snapshot and wrapper origin, retained for diagnostics.
#[derive(Clone, Debug)]
pub(crate) struct ProgramSource {
    pub(crate) text: SourceText,
    /// Bytes of compiled source before `text` that the program keeps no copy of: a host
    /// wrapper's prefix. With any, `text` starts at the wrapper origin, compiled line
    /// `line_offset + 1` and UTF-16 column `column_offset`.
    pub(crate) base: u32,
    pub(crate) line_offset: u32,
    pub(crate) column_offset: u32,
    /// `source_line_starts(text)`, indexed on the first line read: error source positions
    /// read a line per throw, which a scan from the top would make linear in the source.
    /// Shared, so a copy of the source (an error's captured call sites) copies no index.
    pub(crate) line_starts: std::cell::OnceCell<std::sync::Arc<[u32]>>,
}

/// A static item at a fixed place in the executable image, from which a snapshot measures the
/// embedded text a program retains (`SourceText::Static`): within one linked image the offset
/// between two statics is fixed, and a snapshot is read only by the build that wrote it.
static SNAPSHOT_TEXT_ANCHOR: u8 = 0;

/// The bytes at each end of embedded text that a snapshot checks before using the text.
const SNAPSHOT_TEXT_EDGE: usize = 32;

impl SourceText {
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        match self {
            // A snapshot outlives the entry's mapping, so it keeps its own copy of kept text.
            Self::Shared(_) | Self::Kept(_) => {
                writer.u8(0);
                writer.str(self);
            }
            Self::Static(text) => {
                writer.u8(1);
                let anchor = std::ptr::addr_of!(SNAPSHOT_TEXT_ANCHOR) as usize as i64;
                writer.u64((text.as_ptr() as usize as i64).wrapping_sub(anchor) as u64);
                writer.len(text.len());
                let edge = SNAPSHOT_TEXT_EDGE.min(text.len());
                writer.bytes(&text.as_bytes()[..edge]);
                writer.bytes(&text.as_bytes()[text.len() - edge..]);
            }
        }
    }

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        Ok(match reader.u8()? {
            0 => Self::Shared(std::sync::Arc::from(reader.str()?)),
            1 => {
                let offset = reader.u64()? as i64;
                let len = reader.len()?;
                let head = reader.bytes()?;
                let tail = reader.bytes()?;
                let anchor = std::ptr::addr_of!(SNAPSHOT_TEXT_ANCHOR) as usize as i64;
                let address = anchor.wrapping_add(offset) as usize as *const u8;
                // SAFETY: the snapshot was written by this build (its header names the runtime
                // ABI and the host keys it by the executable's identity), whose image holds the
                // embedded text at this offset from the anchor; its edges are checked below.
                let bytes = unsafe { std::slice::from_raw_parts(address, len) };
                if &bytes[..head.len()] != head || &bytes[len - tail.len()..] != tail {
                    return Err(crate::snapshot::SnapshotError("snapshot embedded text moved".into()));
                }
                // SAFETY: the text was a `&'static str` of this image when written.
                Self::Static(unsafe { std::str::from_utf8_unchecked(bytes) })
            }
            _ => return Err(crate::snapshot::SnapshotError("invalid snapshot source text".into())),
        })
    }
}

impl ProgramSource {
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        // `line_starts` is a memo of `text`, made again on the first line read.
        let Self { text, base, line_offset, column_offset, line_starts: _ } = self;
        text.write_snapshot(writer);
        writer.u32(*base);
        writer.u32(*line_offset);
        writer.u32(*column_offset);
    }

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        Ok(Self {
            text: SourceText::read_snapshot(reader)?,
            base: reader.u32()?,
            line_offset: reader.u32()?,
            column_offset: reader.u32()?,
            line_starts: std::cell::OnceCell::new(),
        })
    }
}

/// Starts of JavaScript source lines, including an empty line after a final terminator.
pub(crate) fn source_line_starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    const LINE_SEPARATOR: &[u8] = "\u{2028}".as_bytes();
    const PARAGRAPH_SEPARATOR: &[u8] = "\u{2029}".as_bytes();
    // Scanned as bytes: every terminator but LS and PS is ASCII, and those two share their
    // UTF-8 lead byte, so only the three candidate bytes need a closer look.
    let bytes = text.as_bytes();
    std::iter::once(0).chain(
        memchr::memchr3_iter(b'\n', b'\r', LINE_SEPARATOR[0], bytes).filter_map(move |index| {
            let width = match bytes[index] {
                b'\n' => 1,
                // CR LF ends one line, after the LF.
                b'\r' if bytes.get(index + 1) != Some(&b'\n') => 1,
                b'\r' => return None,
                _ if bytes[index..].starts_with(LINE_SEPARATOR)
                    || bytes[index..].starts_with(PARAGRAPH_SEPARATOR) =>
                {
                    LINE_SEPARATOR.len()
                }
                _ => return None,
            };
            Some(index + width)
        }),
    )
}

impl ProgramSource {
    /// The retained text of compiled source bytes `start..end`, if it holds them.
    pub(crate) fn slice(&self, start: u32, end: u32) -> Option<&str> {
        let start = start.checked_sub(self.base)? as usize;
        let end = end.checked_sub(self.base)? as usize;
        self.text.get(start..end)
    }

    pub(crate) fn source_line(&self, line: u32) -> Option<&str> {
        // Text that starts at the wrapper origin holds no wrapper lines or columns to skip.
        let (line_offset, column_offset) = if self.base == 0 {
            (self.line_offset, self.column_offset)
        } else {
            (0, 0)
        };
        let compiled_line = line.checked_add(line_offset)?.checked_sub(1)? as usize;
        let starts = self.line_starts.get_or_init(|| {
            source_line_starts(&self.text)
                .map(|start| u32::try_from(start).unwrap_or(u32::MAX))
                .collect()
        });
        let start = *starts.get(compiled_line)? as usize;
        let end = starts.get(compiled_line + 1).map_or(self.text.len(), |&end| end as usize);
        let text = self.text.get(start..end)?
            .trim_end_matches(['\r', '\n', '\u{2028}', '\u{2029}']);
        if line != 1 {
            return Some(text);
        }
        let mut offset = 0;
        for (byte, character) in text.char_indices() {
            if offset == column_offset {
                return Some(&text[byte..]);
            }
            offset += character.len_utf16() as u32;
        }
        (offset == column_offset).then_some("")
    }
}

/// The sizes of a program's inline-cache site tables (`ResidualProgram::cache_site_counts`):
/// its field, method and object literal sites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CacheSiteCounts {
    pub(crate) fields: u16,
    pub(crate) methods: usize,
    pub(crate) objects: usize,
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
    pub(crate) functions: Vec<FunctionSlot>,
    pub(crate) cache_sites: u16,
    pub(crate) method_sites: Vec<MethodSite>,
    pub(crate) method_arguments: Vec<Register>,
    pub(crate) field_sites: Vec<FieldSite>,
    pub(crate) object_sites: Vec<ObjectSite>,
    pub(crate) regexp_literal_sites: Vec<RegExpLiteralSite>,
    pub(crate) literal_templates: Vec<Box<[LiteralStep]>>,
    pub(crate) superinstructions: Vec<Superinstruction>,
    /// How its functions can be compiled again from source, when they can.
    pub(crate) replay: Option<std::rc::Rc<crate::compile::replay::ProgramReplay>>,
    /// The records a decoded program keeps encoded until their functions' first read.
    pub(crate) encoded_records: Option<std::rc::Rc<EncodedRecords>>,
    /// Whether the program is known to be valid (`ValidationProof`).
    pub(crate) validated: ValidationProof,
    /// The index of the `undefined` constant, which every compiled program holds and an
    /// unlowered record's lowering reads, derived on first use.
    pub(crate) undefined_constant: std::cell::OnceCell<u32>,
    /// What each function's closures read of the environments they close over, when the
    /// compiler derived it; without it a closure keeps its whole scope chain.
    pub(crate) closure_reads: Option<std::rc::Rc<ClosureReads>>,
}

/// The encoded records of a decoded program's deferred functions (`binary::decode_program`):
/// each decodes on its function's first read, as a released record compiles again. A program
/// loaded from the compile cache runs a small part of its functions, and decoding every
/// record had been most of its load.
pub(crate) struct EncodedRecords {
    /// The program's whole encoding, as the host handed it over: its records stay in place,
    /// in pages that a mapped file keeps out of memory until a record is read.
    pub(crate) bytes: KeptBytes,
    /// A compact encoding's atom ids, by table position (`binary::BinaryReader`).
    pub(crate) atoms: Option<std::rc::Rc<[u32]>>,
    /// Per function, its record in `bytes`, when it is deferred.
    pub(crate) ranges: Box<[Option<DeferredRecord>]>,
}

impl std::fmt::Debug for EncodedRecords {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("EncodedRecords").field("ranges", &self.ranges.len()).finish_non_exhaustive()
    }
}

/// Where a deferred record lies in its program's encoding, and its checksum, which decoding
/// it checks: the load checked only the encoding's body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DeferredRecord {
    pub(crate) start: u32,
    pub(crate) length: u32,
    pub(crate) checksum: u64,
}

/// Bytes a host kept for a compiled program (`Host::kept_compiled_program`), which the program
/// decoded from them keeps for as long as it lives.
pub type KeptBytes = std::rc::Rc<dyn AsRef<[u8]>>;

/// That a program is valid without validating it: this build's compiler output, compiled and
/// validated or decoded from its checksummed form by `Runtime::compile_entry`, which hands it
/// to its embedder unchanged. A clone, which its owner may change, starts unproven.
#[derive(Debug, Default)]
pub(crate) struct ValidationProof(std::cell::Cell<bool>);

impl ValidationProof {
    pub(crate) fn holds(&self) -> bool {
        self.0.get()
    }

    pub(crate) fn establish(&self) {
        self.0.set(true);
    }
}

impl Clone for ValidationProof {
    fn clone(&self) -> Self {
        Self::default()
    }
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
    /// The record of function `index`, compiled again from source on its first read when
    /// the program released it.
    #[inline(always)]
    pub(crate) fn function(&self, index: usize) -> &Function {
        let slot = &self.functions[index];
        match slot.record() {
            Some(function) => function,
            None => self.recompile_function(index),
        }
    }

    /// `function`, for an index that may lie outside the table.
    #[inline(always)]
    pub(crate) fn function_get(&self, index: usize) -> Option<&Function> {
        (index < self.functions.len()).then(|| self.function(index))
    }

    #[cold]
    #[inline(never)]
    fn recompile_function(&self, index: usize) -> &Function {
        self.functions[index].record.get_or_init(|| {
            if let Some(unlowered) = self.functions[index].unlowered.borrow_mut().take() {
                let function = self.lower_unlowered(unlowered);
                function.decoded.prime_loop(&function.code, &function.wide);
                return std::rc::Rc::new(function);
            }
            crate::compile::compile_phase_boundary(crate::compile::CompilePhase::FirstCall);
            let deferred = self
                .encoded_records
                .as_deref()
                .and_then(|records| binary::decode_deferred_function(records, index));
            let function = match deferred {
                Some(decoded) => decoded.unwrap_or_else(|error| {
                    panic!("function {index} of {} does not decode: {error}", self.source_name)
                }),
                None => crate::compile::replay::recompile(self, index as u32)
                    .unwrap_or_else(|error| {
                        panic!("function {index} of {} does not compile again: {error}", self.source_name)
                    })
                    .function,
            };
            // As `ResidualProgram::prime_loop_decoding` primes an installed program's records.
            function.decoded.prime_loop(&function.code, &function.wide);
            std::rc::Rc::new(function)
        })
    }

    /// An unlowered record lowered against the finished program's tables, as its load
    /// would have lowered it (`compile::lower_settled`), with its register roots.
    #[cold]
    #[inline(never)]
    fn lower_unlowered(&self, unlowered: std::rc::Rc<Function>) -> Function {
        let mut function = std::rc::Rc::unwrap_or_clone(unlowered);
        let undefined_constant = *self.undefined_constant.get_or_init(|| {
            let index = self
                .constants
                .iter()
                .position(|constant| matches!(constant, Constant::Undefined))
                .expect("a program with an unlowered record holds the `undefined` constant");
            u32::try_from(index).expect("constant index fits u32")
        });
        crate::compile::lower_settled(
            &mut function,
            &crate::compile::LoweringTables {
                methods: self,
                field_sites: &self.field_sites,
                superinstructions: &self.superinstructions,
                specialized: self.specialized,
                undefined_constant,
            },
            |atom| self.atoms.get(atom),
        );
        function.register_roots =
            crate::compile::liveness::register_roots(&function, self, &self.field_sites, &self.superinstructions);
        function.shrink_to_fit();
        function
    }

    /// Compiles every released record again.
    pub(crate) fn compile_released(&self) {
        for index in 0..self.functions.len() {
            self.function(index);
        }
    }

    /// Drops the record of every function the program can compile again from its source,
    /// which then compiles on its first read. A program whose source keeps only part of
    /// its compiled text, as host builtins do, keeps its records.
    pub(crate) fn release_replayable_records(&mut self) {
        let Some(replay) = self.replay.clone() else {
            return;
        };
        if self.source.as_ref().is_none_or(|source| source.base != 0) {
            return;
        }
        for &id in replay.replayable() {
            self.functions[id as usize].release();
        }
    }

    /// The registers live at `pc` in `function`, when liveness covers it.
    pub(crate) fn register_root_mask(&self, function: u32, pc: usize) -> Option<crate::compile::liveness::RegisterSet> {
        self.register_root_masks(function)?.get(pc)
    }

    /// `function`'s live register masks by pc, one past its last instruction included, when
    /// liveness covers it.
    pub(crate) fn register_root_masks(&self, function: u32) -> Option<&crate::compile::liveness::RegisterMasks> {
        self.shared_register_root_masks(function).map(|masks| &**masks)
    }

    /// `register_root_masks`, as the shared derivation the loop tier's regions hold.
    pub(crate) fn shared_register_root_masks(
        &self,
        function: u32,
    ) -> Option<&std::rc::Rc<crate::compile::liveness::RegisterMasks>> {
        let function = self.function_get(function as usize)?;
        if !function.register_roots.covered {
            return None;
        }
        let masks = function
            .register_roots
            .masks
            .get_or_init(|| {
                crate::compile::liveness::analyze(
                    function,
                    self,
                    &self.field_sites,
                    &self.superinstructions,
                )
                .map(|masks| std::rc::Rc::new(crate::compile::liveness::RegisterMasks::new(masks)))
                .expect("a covered function's liveness is derivable")
            });
        Some(masks)
    }

    pub(crate) fn source_position(&self, function: u32, pc: u32) -> Option<SourcePosition> {
        let position = self.function_get(function as usize)?.source_positions.at(pc)?;
        self.adjust_source_position(position)
    }

    pub(crate) fn adjust_source_position(&self, mut position: SourcePosition) -> Option<SourcePosition> {
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

    pub const FORMAT_VERSION: u8 = 109;
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

    /// What root `slot` of this program holds (`RootSlotRule`), from its declaration.
    pub(crate) fn root_slot_rule(&self, slot: usize) -> RootSlotRule {
        let Some(root) = self.functions.first() else {
            return RootSlotRule::Binding;
        };
        let declaration = root.slot_declaration(slot);
        if declaration.global_var && !self.root_variables_are_local() {
            RootSlotRule::Projection
        } else if declaration.global_lexical && self.kind == ProgramKind::Script {
            RootSlotRule::GlobalLexical
        } else {
            RootSlotRule::Binding
        }
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

    /// The compile-cache form of this program (`binary::encode_compact_program`) and the atom
    /// ids its table names, in order.
    pub(crate) fn encode_compact(&self) -> (Vec<u8>, Vec<u32>) {
        binary::encode_compact_program(self)
    }

    /// This program as an application entry's cached form: its own atom ids, its source and a
    /// checksum (`binary::encode_trusted_program`). Every released record compiles first.
    pub(crate) fn encode_trusted(&self) -> Vec<u8> {
        binary::encode_trusted_program(self)
    }

    /// Decodes `encode_trusted`'s bytes into a program proven valid: this build's compiler
    /// output, which the host keys by build and whose checksum held.
    /// `decode_trusted`, with the program's atoms named by `intern`'s ids.
    pub(crate) fn decode_trusted_interned(bytes: KeptBytes, intern: &mut dyn FnMut(&str) -> u32) -> Result<Self, String> {
        binary::decode_trusted_program_interned(bytes, intern)
    }

    pub(crate) fn decode_trusted(bytes: KeptBytes) -> Result<Self, String> {
        let program = binary::decode_trusted_program(bytes)?;
        program.validated.establish();
        Ok(program)
    }

    /// This program as a snapshot keeps it (`crate::snapshot`): its own atom ids, which the
    /// snapshot's atom table keeps, and its source written apart (`SourceText`).
    pub(crate) fn encode_snapshot(&self) -> Vec<u8> {
        binary::encode_snapshot_program(self)
    }

    /// `encode_snapshot`'s bytes, sharing `atoms` (the restored VM's) and with `text` as the
    /// source; valid as the compiler output it was when written.
    /// The sizes of the program's inline-cache site tables, which a VM lays out per program.
    pub(crate) fn cache_site_counts(&self) -> CacheSiteCounts {
        CacheSiteCounts {
            fields: self.cache_sites,
            methods: self.method_sites.len(),
            objects: self.object_sites.len(),
        }
    }

    /// Primes the loop decoding of every function record the program holds; a released
    /// record primes as it compiles again (`ResidualProgram::function`).
    pub(crate) fn prime_loop_decoding(&self) {
        for function in self.functions.iter().filter_map(FunctionSlot::record) {
            function.decoded.prime_loop(&function.code, &function.wide);
        }
    }

    /// Checks `encode_snapshot`'s bytes as `decode_snapshot` would before decoding them.
    pub(crate) fn verify_snapshot(bytes: &[u8]) -> Result<(), String> {
        binary::verify_snapshot_program(bytes)
    }

    pub(crate) fn decode_snapshot(
        bytes: KeptBytes,
        text: Option<SourceText>,
        atoms: &std::rc::Rc<AtomSequence>,
    ) -> Result<Self, String> {
        let (mut program, atom_count) = binary::decode_snapshot_program(bytes, text)?;
        if atom_count > atoms.len() {
            return Err("snapshot program names atoms its VM lacks".into());
        }
        program.atoms = AtomTable::shared(atoms.clone(), atom_count);
        program.validated.establish();
        Ok(program)
    }

    /// Decodes `encode_compact`'s bytes with `intern`'s ids and `text` as the source; the
    /// caller sets the atom table and validates.
    pub(crate) fn decode_compact(
        bytes: KeptBytes,
        intern: &mut dyn FnMut(&str) -> u32,
        text: SourceText,
    ) -> Result<Self, String> {
        binary::decode_compact_program(bytes, intern, text)
    }
}
mod binary;
mod closure_reads;
pub(crate) use closure_reads::{ClosureReads, EnvironmentDemand, FunctionReadFacts};
pub(crate) use binary::{decode_static_module_analysis, encode_static_module_analysis};
mod disassemble;
#[cfg(test)]
mod tests;
mod validate;

/// A store of an initializing body (`Function::initializer_stores`): the field `atom`, at
/// cache site `site`, takes parameter `parameter` as passed, or the Number `fallback` where
/// the parameter is falsy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InitializerStore {
    pub(crate) atom: u32,
    pub(crate) site: u16,
    pub(crate) parameter: u16,
    pub(crate) fallback: Option<f64>,
}
