use smallvec::SmallVec;

mod internal_bindings;
mod module_records;
pub use module_records::{ModuleImportBinding, ModuleImportName, ModuleRequest, ModuleRequestPhase};
pub(crate) use module_records::{ModuleImportNameKind, ModuleLinkPlan, ModuleReexport, ModuleReexportKind};
pub(crate) use internal_bindings::*;
pub(crate) const LEXICAL_THIS_BINDING: &str = "\0quench:lexical-this";
pub(crate) const NEW_TARGET_BINDING: &str = "\0quench:new-target";

pub type Atom = u32;
pub type Register = u16;

mod source_text_snapshot; pub(crate) mod append_vec;
mod atoms;
mod control_flow;
pub(crate) mod initialization;
mod initializer_stores;
mod super_initializer;
pub(crate) use initializer_stores::{InitializerConstant, InitializerStore, InitializerValue, NestedConstruct, NESTED_ARGUMENTS, NESTED_CHAIN};
mod environment_clone;
pub use environment_clone::{EnvironmentClone, EnvironmentClones};
pub(crate) use environment_clone::Resets;
mod field_store;
pub(crate) use field_store::{FieldStore, StoreReceiver, field_store_op};
mod call_plan;
pub(crate) use call_plan::{CALL_PLAN_FLAG_COUNT, CallPlan};
mod instruction;
mod register_list;
mod name_bindings;
pub(crate) use name_bindings::{CapturedNames, NameBindings};
mod numeric_ops; mod op_classes; mod op_scan; mod unary_operators;
pub(crate) use unary_operators::*;
mod source_positions;
pub(crate) use source_positions::SourcePositions;
pub(crate) use atoms::{AtomSequence, AtomTable};

/// Infix of the compiler's hidden atom for a named function expression's own binding:
/// `{name}{SELF_BINDING_MARKER}{function id}`.
pub(crate) const SELF_BINDING_MARKER: &str = "\0quench:self-binding:";
/// Prefix of the compiler's hidden local for a block-scoped binding renamed apart from a
/// same-named binding of its function: `{BLOCK_BINDING_MARKER}{name}`.
pub(crate) const BLOCK_BINDING_MARKER: &str = "\0quench:block-binding:";
/// Separates the count `hidden_local` appends to a repeated hidden name; no source name has it.
pub(crate) const HIDDEN_LOCAL_SUFFIX: char = '\0';
/// The name a hidden renamed block binding has in source; other names are unchanged.
pub(crate) fn source_binding_name(name: &str) -> &str {
    name.strip_prefix(BLOCK_BINDING_MARKER).map_or(name, |name| name.split(HIDDEN_LOCAL_SUFFIX).next().unwrap_or(name))
}
/// Whether a local named `name` must live in its function's environment whatever its
/// captures: other hidden bindings (lexical `this`, `new.target`, home objects, private
/// names, ...) are reached by name, not only through capture slots. A renamed block binding
/// and a hidden argument slot (`HIDDEN_PARAMETER_PREFIX`) are reached only through their slots.
pub(crate) fn local_name_requires_environment(name: &str) -> bool {
    name.starts_with('\0') && !name.starts_with(BLOCK_BINDING_MARKER) && !name.starts_with(HIDDEN_PARAMETER_PREFIX)
}
/// The prefix of a class's home-object binding, which private brand checks and `super`
/// resolve by name through the enclosing environments.
pub(crate) const HOME_BINDING_PREFIX: &str = "\0quench:home:";
/// Whether the runtime finds local `name` by name from outside its own activation, so a
/// closure that never captures it may still read it: a home object (`HOME_BINDING_PREFIX`),
/// or a compiler binding its function's environment declares (`declares_environment_atom`),
/// which by-name loads of compiler bindings resolve (asked lazily: it scans the atoms).
pub(crate) fn local_name_read_by_name(name: &str, declared_in_environment: impl FnOnce() -> bool) -> bool {
    name.starts_with(HOME_BINDING_PREFIX)
        || (local_name_requires_environment(name) && declared_in_environment())
}
pub(crate) use control_flow::{ControlEdge, Successors, branch_table_entries, for_each_instruction_successor, reachable_from};
pub use instruction::Instr;
pub(crate) use instruction::{ConstructArguments, RegisterWindow, WideInstruction};
pub(crate) use register_list::{ARRAY_LITERAL_WINDOW, RegisterList};
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
pub(crate) use crate::units::MAX_ARRAY_LENGTH;
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

#[cfg(feature = "aot")]
pub(crate) use crate::typed_ir::BindingTime;

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

    #[cfg(feature = "aot")]
    /// An operand field is a location whose encoding may instead carry a
    /// constant; the constant is static, which the instruction decides.
    pub(crate) const fn binding_time(self) -> BindingTime {
        match self {
            Self::ResultRegister
            | Self::Register
            | Self::WriteRegister
            | Self::ReadWriteRegister
            | Self::OptionalRegister
            | Self::OptionalReadRegister
            | Self::NumericLocalTarget
            | Self::RegisterWindowBase
            | Self::ConstructArguments
            | Self::FieldBase
            | Self::Operand
            | Self::NumericIndexOperand => BindingTime::Location,
            Self::CacheSiteIndex | Self::FieldLookupCacheSiteIndex => BindingTime::Site,
            Self::Unused
            | Self::NumericLocalStoreMarker
            | Self::FunctionIndex
            | Self::ElementCount
            | Self::RegisterCount
            | Self::WideIndexChunk
            | Self::BooleanFlag
            | Self::BinaryOperator
            | Self::SignedConstant => BindingTime::Static,
        }
    }
}

#[cfg(feature = "aot")]
impl ImmediateRole {
    /// A layout-encoded immediate packs register windows or capture slots
    /// unless it is a plain scalar.
    pub(crate) const fn binding_time(self, layout: ImmediateLayout) -> BindingTime {
        match self {
            Self::LayoutEncoded => match layout {
                ImmediateLayout::Scalar => BindingTime::Static,
                ImmediateLayout::CaptureDepthAndSlot
                | ImmediateLayout::CallWindow
                | ImmediateLayout::CallWindowWithEvalFlags
                | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags
                | ImmediateLayout::ConstructCountAndFlags
                | ImmediateLayout::RegisterPair => BindingTime::Location,
            },
            Self::LocalSlot | Self::WasmInstanceSlot => BindingTime::Location,
            Self::TemplateSiteIndex
            | Self::RegExpLiteralSiteIndex
            | Self::MethodSiteIndex
            | Self::ObjectSiteIndex
            | Self::FieldLookup => BindingTime::Site,
            Self::Unused
            | Self::ConstantIndex
            | Self::EnvironmentCloneIndex
            | Self::ClosureFunctionIndex
            | Self::ArrayLength
            | Self::AtomIndex
            | Self::FunctionNamePrefix
            | Self::PropertyDefinitionMode
            | Self::BooleanFlag
            | Self::ArrayIndex
            | Self::BinaryOperator
            | Self::AdditionOperator
            | Self::MultiplicationOperator
            | Self::UnaryOperator
            | Self::WasmSignatureIndex
            | Self::WasmFunctionIndex
            | Self::WasmSimdOperator
            | Self::WasmMemoryLoadOperator
            | Self::WasmMemoryStoreOperator
            | Self::WasmMemoryOffset
            | Self::WasmAtomicOperator
            | Self::WasmI32Immediate
            | Self::WasmI32UnaryOperator
            | Self::WasmI31Operator
            | Self::WasmExternalConversion
            | Self::WasmReferenceTarget
            | Self::WasmNonNullCheck
            | Self::WasmGcTypeIndex
            | Self::WasmGcField
            | Self::WasmExceptionFieldIndex
            | Self::WasmI64BinaryOperator
            | Self::WasmI64UnaryOperator
            | Self::WasmScalarConversionOperator
            | Self::WasmF32BinaryOperator
            | Self::WasmF32UnaryOperator
            | Self::WasmF64BinaryOperator
            | Self::WasmF64UnaryOperator
            | Self::LiteralTemplateIndex
            | Self::JumpTarget
            | Self::BranchTableCases
            | Self::SuperinstructionIndex
            | Self::WideInstructionIndex => BindingTime::Static,
        }
    }
}

#[cfg(feature = "aot")]
impl Op {
    /// The binding time of each input, in field order `a`, `b`, `c`, then the
    /// immediate: Ershov's division of this instruction.
    pub(crate) const fn input_binding_times(self) -> [BindingTime; 4] {
        [
            self.field_layout(InstructionField::A).binding_time(),
            self.field_layout(InstructionField::B).binding_time(),
            self.field_layout(InstructionField::C).binding_time(),
            self.immediate_role().binding_time(self.immediate_layout()),
        ]
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
    WasmGcField,
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

    /// A lane `Call` record whose result only the `Await` right after it reads
    /// (`derive_lane_view`). Lane call records carry no eval flags, so the flag reuses the
    /// parameter-eval bit; a settling callee there needs no promise for a primitive result.
    pub(crate) const LANE_CALL_RESULT_AWAITED_FLAG: u32 = Self::PARAMETER_EVAL_FLAG;

    pub(crate) const fn lane_call_result_awaited(imm: u32) -> bool {
        imm & Self::LANE_CALL_RESULT_AWAITED_FLAG != 0
    }

    pub(crate) const fn direct_eval(imm: u32) -> bool {
        imm & Self::DIRECT_EVAL_FLAG != 0
    }

    pub(crate) const fn parameter_eval(imm: u32) -> bool {
        imm & Self::PARAMETER_EVAL_FLAG != 0
    }

    #[inline] pub(crate) const fn call_window_base(imm: u32) -> Register {
        ((imm & Self::CALL_BASE_MASK) >> Self::CALL_BASE_SHIFT) as Register
    }

    #[inline] pub(crate) const fn argument_count(imm: u32) -> u16 {
        (imm & Self::ARGUMENT_COUNT_MASK) as u16
    }

    #[inline] pub(crate) const fn capture_depth(imm: u32) -> u16 {
        (imm >> Self::PAIR_SECOND_SHIFT) as u16
    }

    #[inline] pub(crate) const fn capture_slot(imm: u32) -> u16 {
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


mod op_catalog; mod denotation; mod opcode_table;
pub(crate) use op_catalog::{Class, Decoder, GenericStep, JsDenotation, OwnedStep, Traits, ValueStep, OPCODE_COUNT, opcode_name};

macro_rules! opcodes {
    ($d:tt; $($name:ident => $effect:expr; layout $layout:ident $(; meaning $immediate_role:ident)? $(; flow $flow:ident)? $(; js $js_denotation:ident)? $(; class $class:ident)? $(; staged $decoder:ident)? $(; step $step:ident)? $(; owned $owned:ident)? $(; value $value:ident)? $(; traits $($trait:ident)+)? , @ $result:ident, @ fields($a:ident, $b:ident, $c:ident)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u16)]
        #[quench_macros::native_operation_exports(19, "quench_js_op_")]
        pub enum Op { $($name),+ }

        /// The catalog's opcode indices, generated here so tiers never spell an opcode name.
        #[allow(non_upper_case_globals, dead_code)]
        pub(crate) mod index {
            $(pub(crate) const $name: u16 = super::Op::$name as u16;)+
        }


        #[allow(dead_code)]
        impl Op {
            pub const COUNT: usize = [$(stringify!($name)),+].len();
            pub const NAMES: [&'static str; Self::COUNT] = [$(stringify!($name)),+];
            const CLASSES: [Class; Self::COUNT] = [$(opcodes!(@class $($class)?)),+];
            pub(crate) const fn class(self) -> Class { Self::CLASSES[self as usize] }
            const TRAITS: [Traits; Self::COUNT] = [$(opcodes!(@traits $($($trait)+)?)),+];
            pub(crate) const fn traits(self) -> Traits { Self::TRAITS[self as usize] }
            const DECODERS: [Option<Decoder>; Self::COUNT] = [$(opcodes!(@staged $($decoder)?)),+];
            pub(crate) const fn staged(self) -> Option<Decoder> { Self::DECODERS[self as usize] }
            const GENERIC_STEPS: [Option<GenericStep>; Self::COUNT] = [$(opcodes!(@step $($step)?)),+];
            pub(crate) const fn generic_step(self) -> Option<GenericStep> { Self::GENERIC_STEPS[self as usize] }
            const OWNED_STEPS: [Option<OwnedStep>; Self::COUNT] = [$(opcodes!(@owned $($owned)?)),+];
            pub(crate) const fn owned_step(self) -> Option<OwnedStep> { Self::OWNED_STEPS[self as usize] }
            const VALUE_STEPS: [Option<ValueStep>; Self::COUNT] = [$(opcodes!(@value $($value)?)),+];
            pub(crate) const fn value_step(self) -> Option<ValueStep> { Self::VALUE_STEPS[self as usize] }
            #[cfg(feature = "native-execution")]
            pub(crate) fn native_semantic_entry<H: crate::Host>(self) -> crate::NativeJsOperationEntry {
                match self {
                    $(Self::$name => crate::native_js_operation_entry::<H, { Self::$name as usize }>),+
                }
            }
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

            #[inline(always)]
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
    (@class $class:ident) => { Class::$class };
    (@class) => { Class::Other };
    (@staged $decoder:ident) => { Some(Decoder::$decoder) };
    (@staged) => { None };
    (@step $step:ident) => { Some(GenericStep::$step) };
    (@step) => { None };
    (@owned $owned:ident) => { Some(OwnedStep::$owned) };
    (@owned) => { None };
    (@value $value:ident) => { Some(ValueStep::$value) };
    (@value) => { None };
    (@traits $($trait:ident)*) => { Traits::NONE $(.union(Traits::$trait))* };
}

const READ_THROW: Effect = Effect::READS_HEAP.union(Effect::THROWS);
const WRITE_THROW: Effect = Effect::WRITES_HEAP.union(Effect::THROWS);
const CALL_EFFECT: Effect = READ_THROW.union(Effect::WRITES_HEAP);

opcodes!($;
    Nop => Effect::PURE; layout Scalar; meaning Unused; js Noop; class Nop; traits LEAF_VALUE SCALAR_REGION FRAME_ONLY, @ Register, @ fields(Unused, Unused, Unused),
    CloneEnv => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning EnvironmentCloneIndex; class CloneEnv; step Body; traits NAME_OPERATION, @ Register, @ fields(Unused, Unused, Unused),
    Wide => Effect::PURE; layout Scalar; meaning WideInstructionIndex, @ Register, @ fields(WideIndexChunk, WideIndexChunk, WideIndexChunk),
    LoadConst => Effect::PURE; layout Scalar; meaning ConstantIndex; js LoadConst; class LoadConst; staged Flow; step Body; traits LEAF_VALUE INLINABLE PURE_SETUP ACTIVATION_FREE SCALAR_REGION LIFETIME_SAFE, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadLocal => Effect::PURE; layout Scalar; meaning LocalSlot; class LoadLocal; step Body; owned LoadLocal; traits ACTIVATION_FREE, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocal => Effect::PURE; layout Scalar; meaning LocalSlot; class StoreLocal; step Body; owned StoreLocal; traits ACTIVATION_FREE, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    StoreVarBinding => CALL_EFFECT; layout Scalar; meaning LocalSlot; class StoreVarBinding; step Body; owned StoreVarBinding; traits NAME_OPERATION, @ Register, @ fields(Register, Unused, Unused),
    LoadEnvLocal => Effect::READS_HEAP; layout Scalar; meaning LocalSlot; class LoadEnvLocal; staged Names; step Body; owned LoadLocal, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreEnvLocal => Effect::WRITES_HEAP; layout Scalar; meaning LocalSlot; class StoreEnvLocal; staged Names; step Body; owned StoreEnvLocal, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    LoadCapture => Effect::READS_HEAP; layout CaptureDepthAndSlot; js CapturedValue; class LoadCapture; staged Value; step Body; owned LoadCapture; traits BOXED_RESULT INLINABLE, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreCapture => Effect::WRITES_HEAP; layout CaptureDepthAndSlot; class StoreCapture; staged Value; step Body; owned StoreCapture, @ Register, @ fields(Register, Unused, Unused),
    LoadName => READ_THROW; layout Scalar; meaning AtomIndex; class LoadName; staged Proved; step Body; owned LoadName; traits BOXED_RESULT UNIT_REWRITTEN NAME_LEAF NAME_OPERATION GLOBAL_NAME_ACCESS, @ Register, @ fields(ResultRegister, Unused, CacheSiteIndex),
    LoadNameCall => READ_THROW; layout Scalar; meaning AtomIndex; class LoadNameCall; staged Proved; step Body; owned LoadNameCall; traits NAME_OPERATION CALL_LINK GLOBAL_NAME_ACCESS, @ Register, @ fields(ResultRegister, WriteRegister, CacheSiteIndex),
    LoadNameTypeof => Effect::READS_HEAP; layout Scalar; meaning AtomIndex; class LoadNameTypeof; staged Names; step Body; owned LoadNameTypeof; traits NAME_OPERATION GLOBAL_NAME_ACCESS, @ Register, @ fields(ResultRegister, Unused, CacheSiteIndex),
    ResolveName => READ_THROW; layout Scalar; meaning AtomIndex; step Body; traits NAME_OPERATION, @ Register, @ fields(ResultRegister, BooleanFlag, CacheSiteIndex),
    LoadResolvedName => READ_THROW; layout Scalar; meaning AtomIndex; step Body; traits NAME_OPERATION, @ Register, @ fields(ResultRegister, Register, BooleanFlag),
    DeleteName => READ_THROW; layout Scalar; meaning AtomIndex; step Body; traits NAME_OPERATION, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreName => WRITE_THROW; layout Scalar; meaning AtomIndex; class StoreName; staged Names; step Body; owned StoreName; traits NAME_OPERATION GLOBAL_NAME_ACCESS, @ Register, @ fields(Register, BooleanFlag, CacheSiteIndex),
    StoreResolvedName => WRITE_THROW; layout Scalar; meaning AtomIndex; step Body; traits NAME_OPERATION, @ Register, @ fields(Register, Register, BooleanFlag),
    LoadThis => READ_THROW; layout Scalar; meaning Unused; class LoadThis; staged Value; traits UNIT_REWRITTEN THIS_LEAF, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadCallee => Effect::PURE; layout Scalar; meaning Unused; class LoadCallee; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadImportMeta => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeClosure => CALL_EFFECT; layout Scalar; meaning ClosureFunctionIndex; class MakeClosure; step Body; traits MAKES_CLOSURE, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeArray => CALL_EFFECT; layout Scalar; meaning ArrayLength; class MakeArray; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeConstArray => CALL_EFFECT; layout Scalar; meaning ConstantIndex; class MakeConstArray; step Body, @ Register, @ fields(ResultRegister, ElementCount, Unused),
    // `a` = the position among the `b` string constants from `imm` of the first strictly equal
    // to register `c`, or `b` when none is: a string `switch`'s case selector, which a
    // `WasmBranchTable` follows.
    StringCaseIndex => Effect::PURE; layout Scalar; meaning ConstantIndex; class StringCaseIndex; step Body, @ Register, @ fields(ResultRegister, ElementCount, Register),
    MakeLiteralTemplate => CALL_EFFECT; layout Scalar; meaning LiteralTemplateIndex; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeObject => CALL_EFFECT; layout Scalar; meaning Unused; js ObjectAllocation; class MakeObject; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeObject2 => CALL_EFFECT; layout Scalar; meaning ObjectSiteIndex; class MakeObject2; step Body; traits RETURNING_OBJECT FRAME_RETURNABLE, @ Returnable, @ fields(ResultRegister, Register, Register),
    MakeObjectLiteral => CALL_EFFECT; layout Scalar; meaning ObjectSiteIndex; class MakeObjectLiteral; step Body; traits RETURNING_OBJECT_LITERAL FRAME_RETURNABLE, @ Returnable, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    SuperConstArrayObject2 => CALL_EFFECT; layout Scalar; meaning SuperinstructionIndex; class SuperConstArrayObject2; step Body; traits RETURNING_LITERAL FRAME_RETURNABLE, @ Returnable, @ fields(ResultRegister, Unused, Unused),
    GetIterator => READ_THROW; layout Scalar; meaning Unused; class GetIterator; staged Names; step LaneResult, @ Register, @ fields(ResultRegister, Register, Unused),
    GetAsyncIterator => READ_THROW; layout Scalar; meaning Unused; step Body, @ Register, @ fields(ResultRegister, Register, Unused),
    CreateRegExpLiteral => CALL_EFFECT; layout Scalar; meaning RegExpLiteralSiteIndex; class CreateRegExpLiteral; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    IteratorClose => READ_THROW; layout Scalar; meaning Unused; class IteratorClose; staged Names; step LaneResult, @ Register, @ fields(ResultRegister, Register, Unused),
    // ArrayAccumulation past a literal's static prefix: `length` elisions, then the optional
    // value (iterated when the flag marks a spread), appended at the array's length.
    AppendArray => CALL_EFFECT; layout Scalar; meaning ArrayLength; class AppendArray; staged Names; step LaneResult, @ Register, @ fields(Register, OptionalReadRegister, BooleanFlag),
    RequireObjectCoercible => READ_THROW; layout Scalar; meaning Unused; class RequireObjectCoercible; staged Value; traits INLINABLE PURE_SETUP LIFETIME_SAFE, @ Register, @ fields(Unused, Register, Unused),
    RequireIteratorResult => READ_THROW; layout Scalar; meaning Unused; class RequireIteratorResult; step Lane, @ Register, @ fields(Unused, Register, Unused),
    SuperCallCheck => READ_THROW; layout Scalar; meaning Unused; step Body; traits NAME_OPERATION CALL_LINK, @ Register, @ fields(Unused, Unused, Unused),
    IteratorCleanupPush => Effect::CONTROL; layout Scalar; meaning Unused; class IteratorCleanupPush; step Lane, @ Register, @ fields(Register, Register, Unused),
    IteratorCleanupPop => Effect::CONTROL; layout Scalar; meaning Unused; class IteratorCleanupPop; step Lane, @ Register, @ fields(Unused, Unused, Unused),
    // `a` = the next key of the for-in state `b` (`Vm::for_in_next`) that is still an enumerable property; to `imm` once none is left.
    ForInNext => CALL_EFFECT; layout Scalar; meaning JumpTarget; flow ConditionalJump; class ForInNext; staged Names; step Lane; traits LANE_LOOP_EXIT, @ Register, @ fields(ResultRegister, Register, Unused),
    // `a` = the for-in state over `b` (`Vm::for_in_start`): its key snapshot, at the first key.
    ForInStart => CALL_EFFECT; layout Scalar; meaning Unused; class ForInStart; staged Names; step LaneResult, @ Register, @ fields(ResultRegister, Register, Unused),
    SetFunctionName => Effect::WRITES_HEAP; layout Scalar; meaning AtomIndex; class SetFunctionName; step Lane, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    SetFunctionNameKey => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning FunctionNamePrefix; step Lane, @ Register, @ fields(Register, Register, Unused),
    InitializeTdz => Effect::PURE; layout Scalar; meaning LocalSlot; class InitializeTdz; step Body, @ Register, @ fields(Unused, Unused, Unused),
    Await => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning Unused; class Await, @ Register, @ fields(ResultRegister, Register, Unused),
    Yield => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning Unused; class Yield, @ Register, @ fields(ResultRegister, Register, Unused),
    YieldStar => READ_THROW.union(Effect::CONTROL); layout RegisterPair; class YieldStar, @ RegisterReadWrite, @ fields(ResultRegister, Register, ReadWriteRegister),
    GetField => READ_THROW; layout Scalar; meaning FieldLookup; js GetField; class GetField; staged Property; step Body; traits BOXED_RESULT FIELD_LEAF RETURNING_FIELD INLINABLE PURE_SETUP FRAME_RETURNABLE, @ ReturnableAndThis, @ fields(ResultRegister, FieldBase, FieldLookupCacheSiteIndex),
    GetIndex => READ_THROW; layout Scalar; meaning Unused; class GetIndex; staged Element; step Body; traits BOXED_RESULT INLINABLE PURE_SETUP ELEMENT_ACCESS LIFETIME_SAFE, @ Register, @ fields(ResultRegister, NumericIndexOperand, NumericIndexOperand),
    ToPropertyKey => READ_THROW; layout Scalar; meaning Unused; class ToPropertyKey; staged Generic; step LaneResult, @ Register, @ fields(ResultRegister, Register, Unused),
    ToNumeric => READ_THROW; layout Scalar; meaning Unused; class ToNumeric; staged Value; traits INLINABLE PURE_SETUP LIFETIME_SAFE, @ Register, @ fields(ResultRegister, Register, Unused),
    CopyDataProperties => CALL_EFFECT; layout Scalar; meaning Unused; class CopyDataProperties; step Lane, @ Register, @ fields(Register, Register, Register),
    MarkPrivateName => WRITE_THROW; layout Scalar; meaning AtomIndex; class MarkPrivateName; step Lane, @ Register, @ fields(Unused, Register, Register),
    DefinePrivateField => WRITE_THROW; layout Scalar; meaning AtomIndex; class DefinePrivateField; step Lane, @ Register, @ fields(Register, Register, Register),
    SetField => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldStore; staged Property; step Body; traits INLINABLE, @ Register, @ fields(Register, Register, CacheSiteIndex),
    DefineComputedField => WRITE_THROW; layout Scalar; meaning Unused; step Lane, @ Register, @ fields(Register, Register, Register),
    SetThisField => WRITE_THROW; layout Scalar; meaning AtomIndex; staged Property; step Body; traits INLINABLE, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    SetIndex => WRITE_THROW; layout Scalar; meaning BooleanFlag; class SetIndex; staged Element; step Body; traits INLINABLE ELEMENT_ACCESS LIFETIME_SAFE, @ Register, @ fields(Register, Register, Register),
    DefineArrayElement => WRITE_THROW; layout Scalar; meaning ArrayIndex; class DefineArrayElement; step Lane, @ Register, @ fields(Register, Register, Unused),
    Binary => READ_THROW; layout Scalar; meaning BinaryOperator; js NumericBinary; class Binary; staged Generic; step Body; value Binary; traits LEAF_BINARY RETURNING_BINARY INLINABLE PURE_SETUP ACTIVATION_FREE LIFETIME_SAFE, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    IncDec => READ_THROW; layout Scalar; meaning BooleanFlag; class IncDec; staged Value; traits LEAF_VALUE INLINABLE PURE_SETUP SCALAR_REGION LIFETIME_SAFE, @ Register, @ fields(ResultRegister, Register, Unused),
    Unary => READ_THROW; layout Scalar; meaning UnaryOperator; js NumericUnary; class Unary; staged Value; step Body; value Unary; traits LEAF_VALUE RETURNING_UNARY INLINABLE PURE_SETUP ACTIVATION_FREE FRAME_RETURNABLE LIFETIME_SAFE, @ Returnable, @ fields(ResultRegister, Register, Unused),
    // `a` = ToString of each register of the window `b` of `c` registers, concatenated in
    // order: a template literal's string (ECMA-262 13.15.5.1), made at once.
    StringConcat => READ_THROW; layout Scalar; meaning Unused; js StringConcatenation; step Body, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    Delete => READ_THROW; layout Scalar; meaning BooleanFlag; class Delete; staged Property; step Body, @ Register, @ fields(ResultRegister, Register, Register),
    CheckPrivate => READ_THROW; layout Scalar; meaning AtomIndex; class CheckPrivate; step Lane, @ Register, @ fields(Register, Unused, Unused),
    PrivateIn => READ_THROW; layout Scalar; meaning AtomIndex; class PrivateIn; step Lane, @ Register, @ fields(ResultRegister, Register, Unused),
    Move => Effect::PURE; layout Scalar; meaning Unused; js Move; class Move; staged Flow; traits LEAF_VALUE INLINABLE PURE_SETUP ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE, @ Register, @ fields(ResultRegister, Register, Unused),
    Call => CALL_EFFECT; layout CallWindowWithEvalFlags; flow Call; class Call; staged Call; traits UNIT_REWRITTEN INLINABLE FRAME_RETURNABLE CALL_LINK, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallArray => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call; class CallArray; staged Names; traits CALL_LINK, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallSpread => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call; class CallSpread; staged Names; traits CALL_LINK, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallKnown => CALL_EFFECT; layout CallWindow; flow Call; class CallKnown; staged Call; traits CALL_LINK, @ Returnable, @ fields(ResultRegister, FunctionIndex, Unused),
    CallMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call; class CallMethod; staged Call; traits CALL_LINK, @ Returnable, @ fields(ResultRegister, Register, Unused),
    CallThisMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call; staged Call; traits CALL_LINK, @ Returnable, @ fields(ResultRegister, Unused, Unused),
    Construct => CALL_EFFECT; layout ConstructCountAndFlags; flow Call; class Construct; staged Call; traits UNIT_REWRITTEN FRAME_RETURNABLE CALL_LINK, @ Returnable, @ fields(ResultRegister, Register, ConstructArguments),
    Jump => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow Jump; js Jump; class Jump; staged Flow; traits LEAF_JUMP INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE, @ Register, @ fields(Unused, Unused, Unused),
    JumpFalse => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class JumpFalse; staged Flow; traits LEAF_JUMP INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE, @ Register, @ fields(Register, Unused, Unused),
    JumpBinaryFalse => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning JumpTarget; flow ConditionalJump; class JumpBinaryFalse; staged Generic; step Body; value CompareJump; traits LEAF_BINARY INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE, @ Register, @ fields(BinaryOperator, Operand, Operand),
    Return => Effect::CONTROL; layout Scalar; meaning Unused; flow Terminal; js Return; class Return; staged Flow; traits UNIT_REWRITTEN LEAF_VALUE INLINABLE ACTIVATION_FREE SCALAR_REGION, @ Register, @ fields(Register, Unused, Unused),
    Throw => Effect::THROWS.union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal; class Throw; step Body; traits UNIT_REWRITTEN RAISES, @ Register, @ fields(Register, Unused, Unused),
    NumericAdd => READ_THROW; layout Scalar; meaning AdditionOperator; js NumericBinary; class NumericAdd; traits LEAF_BINARY INLINABLE PURE_SETUP ACTIVATION_FREE LIFETIME_SAFE, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    NumericMultiply => READ_THROW; layout Scalar; meaning MultiplicationOperator; js NumericBinary; class NumericMultiply; traits LEAF_BINARY INLINABLE PURE_SETUP ACTIVATION_FREE LIFETIME_SAFE, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    InitializeThis => CALL_EFFECT.union(Effect::CONTROL); layout Scalar; meaning Unused; step Body; traits BINDS_THIS NAME_OPERATION, @ Register, @ fields(Register, Unused, Unused),
    CacheTemplateObject => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning TemplateSiteIndex; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadCachedTemplateObject => Effect::READS_HEAP; layout Scalar; meaning TemplateSiteIndex; step Body, @ Register, @ fields(ResultRegister, Unused, Unused),
    DefineField => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldDefinition; class DefineField; step Lane, @ Register, @ fields(Register, Register, CacheSiteIndex),
    ValidateClassHeritage => READ_THROW; layout Scalar; meaning Unused; step Body, @ Register, @ fields(Register, Unused, Unused),
    SetFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldStore; staged Property; step Body, @ Register, @ fields(Register, Register, CacheSiteIndex),
    SetThisFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex; staged Property; step Body, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    // The general loop's fast arms dispatch on these with the opcodes above, so they share
    // one dense range of opcode numbers.
    LoadLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot; js PlainLocalLoad; class LoadLocalPlain; staged Local; traits ACTIVATION_FREE, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot; js PlainLocalStore; class StoreLocalPlain; staged Local; traits ACTIVATION_FREE, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    JumpTrue => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class JumpTrue; staged Flow; traits LEAF_JUMP INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE, @ Register, @ fields(Register, Unused, Unused),
    // `Call` whose last window argument the instruction before it loaded from the activation's
    // `arguments` binding (`CallPlan::forwards_arguments`).
    CallForwardingArguments => CALL_EFFECT; layout CallWindowWithEvalFlags; flow Call; class CallForwardingArguments; staged Call; traits FRAME_RETURNABLE CALL_LINK, @ Returnable, @ fields(ResultRegister, Register, Register),
    WasmI32Add => Effect::PURE; layout Scalar; meaning Unused; class WasmI32AddRegister, @ Register, @ fields(ResultRegister, Register, Register),
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
    WasmI32AddImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; class WasmI32AddImmediate, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32SubtractImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; class WasmI32SubtractImmediate, @ Register, @ fields(ResultRegister, Register, Unused),
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
    WasmI32ShiftRightUnsignedAndImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; class WasmI32ShiftRightUnsignedAnd, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    // `a = b + c`, then jump when `a` is nonzero: a counted loop's step and test.
    WasmI32AddImmediateJumpNonZero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmI32AddJump, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    WasmInstanceBinding => Effect::READS_HEAP; layout Scalar; meaning WasmInstanceSlot; class WasmInstanceBinding, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmFillRegisters => Effect::PURE; layout Scalar; meaning ConstantIndex; class WasmFillRegisters, @ Register, @ fields(Unused, RegisterWindowBase, RegisterCount),
    WasmSelect => Effect::PURE; layout RegisterPair; class WasmSelect, @ Register, @ fields(ResultRegister, Register, Register),
    WasmJumpI32Zero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32Zero, @ Register, @ fields(Register, Unused, Unused),
    WasmJumpI32NonZero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32NonZero, @ Register, @ fields(Register, Unused, Unused),
    WasmBranchTable => Effect::CONTROL; layout Scalar; meaning BranchTableCases; flow BranchTable; class WasmBranchTable, @ Register, @ fields(Register, Unused, Unused),
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
    WasmJumpI32EqualImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32EqualImmediate, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32NotEqualImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32NotEqualImmediate, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessEqualSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessEqualUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterEqualSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterEqualUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmI32Unary => Effect::PURE; layout Scalar; meaning WasmI32UnaryOperator; class WasmI32Unary, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI64Binary => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmI64BinaryOperator; class WasmI64Binary, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Unary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmI64UnaryOperator; class WasmI64Unary, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmScalarConvert => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmScalarConversionOperator; class WasmScalarConvert, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmF32Binary => Effect::PURE; layout Scalar; meaning WasmF32BinaryOperator; class WasmFloatBinary32, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF32Unary => Effect::PURE; layout Scalar; meaning WasmF32UnaryOperator; class WasmFloatUnary32, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmF64Binary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmF64BinaryOperator; class WasmFloatBinary64, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF64Unary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmF64UnaryOperator; class WasmFloatUnary64, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableInit => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; traits UNROLL_EXCLUDED, @ Register, @ fields(Register, Register, Register),
    WasmIndirectTarget => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmSignatureIndex; class WasmIndirectTarget; traits UNROLL_EXCLUDED, @ Register, @ fields(ResultRegister, Register, Register),
    WasmGlobalGet => READ_THROW; layout Scalar; meaning Unused; class WasmGlobalGet, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmGlobalSet => WRITE_THROW; layout Scalar; meaning Unused; class WasmGlobalSet, @ Register, @ fields(Register, Register, Unused),
    WasmRefFunc => CALL_EFFECT; layout Scalar; meaning WasmFunctionIndex; class WasmRefFunc, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmRefIsNull => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableSize => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; traits UNROLL_EXCLUDED, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused; traits UNROLL_EXCLUDED, @ Register, @ fields(ResultRegister, Register, Register),
    WasmTableSet => Effect::WRITES_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused; traits UNROLL_EXCLUDED, @ Register, @ fields(Register, Register, Register),
    WasmTableGrow => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout RegisterPair; traits UNROLL_EXCLUDED, @ Register, @ fields(ResultRegister, Register, Register),
    WasmTableFill => Effect::WRITES_HEAP.union(Effect::THROWS); layout RegisterPair; traits UNROLL_EXCLUDED, @ Register, @ fields(Register, Register, Register),
    WasmTableCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; traits UNROLL_EXCLUDED, @ Register, @ fields(Register, Register, Register),
    WasmMemoryInit => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; traits UNROLL_EXCLUDED, @ Register, @ fields(Register, Register, Register),
    WasmMemoryCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; class WasmMemoryCopy; traits WASM_MEMORY UNROLL_EXCLUDED, @ Register, @ fields(Register, Register, Register),
    WasmMemoryFill => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; class WasmMemoryFill; traits WASM_MEMORY UNROLL_EXCLUDED, @ Register, @ fields(Register, Register, Register),
    WasmMemoryAddress => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning ConstantIndex; class WasmMemoryAddress; traits WASM_MEMORY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmMemoryLoad => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryLoadOperator; class WasmMemoryLoad; traits WASM_MEMORY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmMemoryStore => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryStoreOperator; class WasmMemoryStore; traits WASM_MEMORY, @ Register, @ fields(Register, Register, Register),
    WasmI32Load => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF32Load => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF64Load => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load8S => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load8S, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load8U => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load8U, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load16S => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load16S, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load16U => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load16U, @ Register, @ fields(ResultRegister, Register, Register),
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
    WasmMemorySize => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; class WasmMemorySize; traits WASM_MEMORY, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmMemoryGrow => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; class WasmMemoryGrow; traits UNROLL_EXCLUDED, @ Register, @ fields(ResultRegister, Register, Register),
    WasmUnreachable => Effect::THROWS.union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal; class WasmUnreachable, @ Register, @ fields(Unused, Unused, Unused),
    WasmSimd => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmSimdOperator; class WasmSimd, @ Register, @ fields(ResultRegister, Register, Register),
    WasmSimdShuffle => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning ConstantIndex; class WasmSimdShuffle, @ Register, @ fields(ResultRegister, Register, Register),
    DefinePropertyRecord => CALL_EFFECT; layout Scalar; meaning PropertyDefinitionMode; class DefinePropertyRecord, @ Register, @ fields(Register, Register, Register),
    WasmRefAsNonNull => Effect::THROWS; layout Scalar; meaning WasmNonNullCheck, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI31 => Effect::THROWS; layout Scalar; meaning WasmI31Operator, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefTest => Effect::READS_HEAP; layout Scalar; meaning WasmReferenceTarget; class WasmRefTest, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefCast => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget; class WasmRefCast, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefEq => Effect::PURE; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmStructNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmStructNewDefault => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmStructGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmStructGetS => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmStructGetU => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmStructSet => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(Register, Register, Unused),

    WasmArrayNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNew, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayNewDefault => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewDefault, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmArrayNewFixed => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewFixed, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayLen => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Unused),

    WasmArrayGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayGetS => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayGetU => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArraySet => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcField, @ Register, @ fields(Register, Register, Register),
    WasmExternalConversion => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmExternalConversion, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmArrayFill => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmArrayCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmArrayNewData => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewData, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayNewElem => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewElem, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayInitData => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),
    WasmArrayInitElem => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair, @ Register, @ fields(Register, Register, Register),

    WasmExceptionNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmExceptionMatch => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
    WasmExceptionPayload => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmExceptionFieldIndex, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmThrowRef => Effect::READS_HEAP.union(Effect::THROWS).union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal, @ Register, @ fields(Register, Unused, Unused),
    WasmStructNewDesc => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmStructNewDefaultDesc => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmRefGetDesc => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmRefGetDesc, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmDescriptorTest => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget; class WasmDescriptorTest, @ Register, @ fields(ResultRegister, Register, Register),
    WasmDescriptorCast => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget; class WasmDescriptorCast, @ Register, @ fields(ResultRegister, Register, Register),

    WasmAtomicAccess => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmAtomicOperator, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmAtomicFence => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused, @ Register, @ fields(Unused, Unused, Unused),
    SetFunctionDebugName => Effect::WRITES_HEAP; layout Scalar; meaning AtomIndex; step Lane, @ Register, @ fields(Register, Unused, Unused),
    // `arguments.length` and `arguments[key]` of the `arguments` binding in the local slot:
    // an unmaterialized binding answers from the values its activation kept
    // (`Function::kept_arguments`); any other value is read as `GetField` and
    // `GetIndex` read it.
    ArgumentsLength => READ_THROW; layout Scalar; meaning LocalSlot; class ArgumentsLength; staged Names, @ Register, @ fields(ResultRegister, Unused, Unused),
    ArgumentGet => READ_THROW; layout Scalar; meaning LocalSlot; class ArgumentGet, @ Register, @ fields(ResultRegister, Register, Unused),
    // A first-call stub's body: calls its chunk with its arguments and `new.target` (`Vm::call_first_chunk`).
    CallFirstChunk => CALL_EFFECT; layout Scalar; meaning Unused, @ Register, @ fields(ResultRegister, Register, Register),
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
    #[inline(always)] pub(crate) fn on_entry(
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
        instruction.op().control_flow_layout().jumps() && instruction.jump_target() as usize <= pc
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
/// Made only for a function with a list longer than `LOCAL_SLOT_SCAN`: a short list is
/// scanned, where a box of sets per called function held 9.6 MB of jsdom-d3-startup's peak.
#[derive(Clone, Debug, Default)]
pub(crate) struct DeclaredAtomSets {
    global_lexical: rustc_hash::FxHashSet<Atom>,
    global_var: rustc_hash::FxHashSet<Atom>,
    global_immutable: rustc_hash::FxHashSet<Atom>,
    environment: rustc_hash::FxHashSet<Atom>,
    /// Per local slot: whether its name is a global var or global lexical declaration.
    slots: Box<[SlotDeclaration]>,
    /// Each local name's first slot, for a function with more than `LOCAL_SLOT_SCAN` locals
    /// (a script root declaring thousands of globals); a smaller one scans its names.
    local_slots: Option<rustc_hash::FxHashMap<Atom, u32>>,
}

/// Locals a name's slot is found among by a scan (`Function::local_slot`).
const LOCAL_SLOT_SCAN: usize = 32;

/// Whether a local slot's name is among `global_var_atoms` and `global_lexical_atoms`.
/// What a root environment slot holds for the reads through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootSlotRule {
    /// The binding itself.
    Binding,
    /// A Script's global lexical binding of this name: the slot itself until the realm
    /// retains the name, the realm's record from then on.
    GlobalLexical(Atom),
    /// A global var of this name: a property of the global object, which reads go through.
    GlobalVar(Atom),
    /// A module import, which reads go through.
    Import,
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
    /// A parameter's direct `eval` declares `arguments`, which the call raises a SyntaxError for
    /// (`Vm::step`'s `Call`); the function otherwise activates as any other.
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
    pub(crate) selective_capture_slots: Option<crate::heap::ThinBoxSlice<u16>>,
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
    pub environment_clones: EnvironmentClones,
    pub code: Vec<Instr>,
    pub(crate) wide: Vec<WideInstruction>,
    pub registers: u16,
    pub(crate) dispatch: DispatchClass,
    /// Fixed-width view of `code`, derived once the function proves hot.
    pub(crate) decoded: HotDecoding,
    /// Lazily derived `plain_local_slots`; never serialized.
    pub(crate) plain_locals: std::cell::OnceCell<crate::heap::ThinBoxSlice<u64>>,
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
    /// `CallPlan::to_bits`, packed: the plan's flags are one word, not a byte each.
    pub(crate) call_plan: std::cell::OnceCell<u16>,
    /// Lazily derived `initializer_stores`; never serialized.
    pub(crate) initializer: std::cell::OnceCell<Option<crate::heap::ThinBoxSlice<InitializerStore>>>,
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

// Every compiled function keeps one record for the program's life (a fastify server holds
// about 4,400), so each word here is a per-function cost of an idle process: lists fixed
// once compiled sit behind thin slices. `code`, `wide` and `source_positions` stay as the
// compiler builds them: its passes rewrite them, and its rewrite loop measured slower when
// their layout changed.
const _: () = assert!(size_of::<Function>() == 368);

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

/// A slot's `FunctionHeader` in 24 bytes, not 48: each optional word is stored with
/// `u32::MAX` for none (no program reaches that many functions, atoms or source bytes), and
/// the seven flags share one byte. `FunctionSlot::header` unpacks it, field by field where
/// the caller reads one.
#[derive(Clone, Copy, Debug)]
struct PackedHeader {
    parent: u32,
    name: u32,
    inferred_name: u32,
    span_start: u32,
    span_end: u32,
    length: u16,
    dispatch: DispatchClass,
    flags: u8,
}

const _: () = assert!(size_of::<PackedHeader>() == 24);
const _: () = assert!(size_of::<FunctionSlot>() == 64);

const NO_WORD: u32 = u32::MAX;

impl PackedHeader {
    const ARROW: u8 = 1 << 0;
    const ASYNC: u8 = 1 << 1;
    const GENERATOR: u8 = 1 << 2;
    const CLASS_CONSTRUCTOR: u8 = 1 << 3;
    const CONSTRUCTIBLE: u8 = 1 << 4;
    const PROTOTYPE_UNOBSERVABLE: u8 = 1 << 5;
    const INHERITED_WITH_SCOPE: u8 = 1 << 6;

    fn word(value: Option<u32>) -> u32 {
        let word = value.unwrap_or(NO_WORD);
        assert!(value.is_none() || word != NO_WORD, "a header word reaches u32::MAX");
        word
    }

    #[inline(always)]
    fn option(word: u32) -> Option<u32> {
        (word != NO_WORD).then_some(word)
    }

    fn pack(header: &FunctionHeader) -> Self {
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        let (span_start, span_end) = match header.source_span {
            Some(span) => (Self::word(Some(span.start)), span.end),
            None => (NO_WORD, 0),
        };
        Self {
            parent: Self::word(header.parent),
            name: Self::word(header.name),
            inferred_name: Self::word(header.inferred_name),
            span_start,
            span_end,
            length: header.length,
            dispatch: header.dispatch,
            flags: flag(header.is_arrow, Self::ARROW)
                | flag(header.is_async, Self::ASYNC)
                | flag(header.is_generator, Self::GENERATOR)
                | flag(header.is_class_constructor, Self::CLASS_CONSTRUCTOR)
                | flag(header.constructible, Self::CONSTRUCTIBLE)
                | flag(header.prototype_unobservable, Self::PROTOTYPE_UNOBSERVABLE)
                | flag(header.inherited_with_scope, Self::INHERITED_WITH_SCOPE),
        }
    }

    #[inline(always)]
    fn unpack(self) -> FunctionHeader {
        let flag = |bit: u8| self.flags & bit != 0;
        FunctionHeader {
            parent: Self::option(self.parent),
            name: Self::option(self.name),
            inferred_name: Self::option(self.inferred_name),
            source_span: Self::option(self.span_start).map(|start| SourceSpan { start, end: self.span_end }),
            length: self.length,
            dispatch: self.dispatch,
            is_arrow: flag(Self::ARROW),
            is_async: flag(Self::ASYNC),
            is_generator: flag(Self::GENERATOR),
            is_class_constructor: flag(Self::CLASS_CONSTRUCTOR),
            constructible: flag(Self::CONSTRUCTIBLE),
            prototype_unobservable: flag(Self::PROTOTYPE_UNOBSERVABLE),
            inherited_with_scope: flag(Self::INHERITED_WITH_SCOPE),
        }
    }
}

/// What a record built from its program (`ResidualProgram::built_record`) is for: kept as the
/// function's record, which its first call makes, or read once and dropped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RecordUse {
    Kept,
    Transient,
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
    header: PackedHeader,
    record: std::cell::OnceCell<std::rc::Rc<Function>>,
    unlowered: std::cell::RefCell<Option<Unlowered>>,
    /// The unlowered record its first runs execute until it lowers (`tier0`).
    tier0: tier0::Tier0Cell,
}

/// A slot's unlowered record: as compiled, for a function about to run (a first-call chunk's),
/// or packed (`binary::pack_unlowered`) for one a whole load left unread, which most such
/// functions stay (a 3.4 MB bundle: 12K functions, 2K ever called).
#[derive(Clone, Debug)]
enum Unlowered {
    Record(std::rc::Rc<Function>),
    Packed(Box<binary::PackedUnlowered>),
}

impl Unlowered {
    fn function(&self) -> Function {
        match self {
            Self::Record(function) => Function::clone(function),
            Self::Packed(packed) => binary::unpack_unlowered(packed),
        }
    }

    fn into_function(self) -> Function {
        match self {
            Self::Record(function) => std::rc::Rc::unwrap_or_clone(function),
            Self::Packed(packed) => binary::unpack_unlowered(&packed),
        }
    }
}

impl FunctionSlot {
    fn new(header: FunctionHeader, record: Option<Function>, unlowered: Option<Unlowered>) -> Self {
        let record = record.map_or_else(Default::default, |function| std::rc::Rc::new(function).into());
        Self { header: PackedHeader::pack(&header), record, unlowered: unlowered.into(), tier0: Default::default() }
    }

    pub(crate) fn compiled(function: Function) -> Self {
        Self::new(FunctionHeader::of(&function), Some(function), None)
    }

    /// A function whose record its program compiles again on the function's first read.
    pub(crate) fn released(header: FunctionHeader) -> Self {
        Self::new(header, None, None)
    }

    /// A function whose load settled its dispatch class, the header's last fact, and left
    /// the rest of its lowering to its first read.
    pub(crate) fn unlowered(function: Function) -> Self {
        Self::new(FunctionHeader::of(&function), None, Some(Unlowered::Record(std::rc::Rc::new(function))))
    }

    /// An unlowered function its load leaves unread: its record kept packed until its first read.
    pub(crate) fn unlowered_packed(function: Function) -> Self {
        Self::new(FunctionHeader::of(&function), None, Some(Unlowered::Packed(binary::pack_unlowered(&function))))
    }

    /// Whether the slot holds no record, compiled or unlowered: its first read compiles
    /// the record again or decodes it.
    #[cfg(test)]
    pub(crate) fn is_released(&self) -> bool {
        self.record.get().is_none() && self.unlowered.borrow().is_none() && self.tier0.get().is_none()
    }

    #[inline(always)] pub(crate) fn header(&self) -> FunctionHeader {
        self.header.unpack()
    }

    /// The compiled record, when it exists: the lowered one, else the one its first runs
    /// execute unlowered (`tier0`).
    #[inline(always)] pub(crate) fn record(&self) -> Option<&Function> {
        self.record.get().map(|function| &**function).or_else(|| self.tier0.get().map(|tier0| tier0.record()))
    }

    /// Drops the record, which its program compiles again on the function's next read.
    fn release(&mut self) {
        self.record.take();
        self.unlowered.get_mut().take();
        self.tier0.take();
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
        // A change to a record still running unlowered makes it the compiled one.
        self.settle_tier0();
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

/// The records of the implicit `constructor(...args) { super(...args); }` that
/// `Function::super_forwarding` reads: the rest list, the parent's four, the construction,
/// the receiver binding and the return.
const FORWARDED_REST_RECORDS: usize = 8;
/// The records of `constructor(a, b) { super(a, b); }` besides its parameter loads: the
/// parent's four and a spare `undefined` load, then the construction, its check, the
/// receiver binding, an `undefined` load and the return.
const FORWARDED_PARAMETERS_FRAME: usize = 10;
/// The records a forwarding constructor may add to its shape: the rewrite's copy of the
/// active function into the parent call's window, a `Move` right before the call.
const FORWARDED_WINDOW_COPY: usize = 1;

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

/// The facts before the call plan in `Function::derived_facts`' packing, by bit position.
#[derive(Clone, Copy)]
pub(crate) enum DerivedFact {
    ActivationBindings,
    LiveParameters,
    StaticNameScope,
    DynamicScopeAccess,
}

impl DerivedFact {
    pub(crate) const fn bit(self) -> u32 {
        1 << self as u32
    }
}

const DERIVED_FACT_COUNT: u32 = DerivedFact::DynamicScopeAccess as u32 + 1;

/// The tables a function's derived facts read beyond its record (`Function::derived_facts`):
/// its program's atom names, the kinds of the functions it makes and the superinstructions.
/// A finished program answers them, and so does its compiler while it loads once the
/// function and every function nested in it are finished (`Compiler::finish_early`).
pub(crate) trait FactTables {
    /// The atom's text, or `None` for an id outside the table.
    fn atom_name(&self, atom: usize) -> Option<&str>;
    /// Whether function `function` is an arrow, or `None` for an id outside the table.
    fn function_is_arrow(&self, function: usize) -> Option<bool>;
    fn superinstructions(&self) -> &[Superinstruction];
}

impl FactTables for ResidualProgram {
    fn atom_name(&self, atom: usize) -> Option<&str> {
        self.atoms.get(atom)
    }

    fn function_is_arrow(&self, function: usize) -> Option<bool> {
        self.functions.get(function).map(|slot| slot.header().is_arrow)
    }

    fn superinstructions(&self) -> &[Superinstruction] {
        &self.superinstructions
    }
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
#[derive(Clone, Debug)]
pub(crate) struct RegisterRoots {
    covered: bool,
    /// The registers below which every register live at entry lies (`entry_fill`), or
    /// `ALL_LIVE_AT_ENTRY` where liveness proves no bound.
    entry_live: u16,
    dead_call_results: crate::heap::ThinBoxSlice<u32>,
    pub(crate) masks: std::cell::OnceCell<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
}

/// Uncovered, which bounds no register live at entry.
impl Default for RegisterRoots {
    fn default() -> Self {
        Self::new(false)
    }
}

impl RegisterRoots {
    pub(crate) fn new(covered: bool) -> Self {
        Self::with_dead_call_results(covered, Default::default())
    }

    /// `dead_call_results` in increasing pc order, with no bound on the live entry registers.
    pub(crate) fn with_dead_call_results(covered: bool, dead_call_results: crate::heap::ThinBoxSlice<u32>) -> Self {
        Self::with_entry_live(covered, Self::ALL_LIVE_AT_ENTRY, dead_call_results)
    }

    /// `with_dead_call_results`, every register live at entry below `entry_live`.
    pub(crate) fn with_entry_live(covered: bool, entry_live: u16, dead_call_results: crate::heap::ThinBoxSlice<u32>) -> Self {
        Self { covered, entry_live, dead_call_results, masks: std::cell::OnceCell::new() }
    }

    /// An `entry_live` that bounds nothing.
    pub(crate) const ALL_LIVE_AT_ENTRY: u16 = u16::MAX;

    /// The registers an activation of a function of `registers` registers starts with its
    /// initial value: those below every register live at its entry. No instruction reads
    /// another before writing it, and a collection reads a covered function's registers only
    /// where its masks prove them live, so the rest may hold any value a window held before.
    #[inline(always)]
    pub(crate) fn entry_fill(&self, registers: u16) -> u16 {
        self.entry_live.min(registers)
    }

    /// The `entry_live` bound as recorded.
    pub(crate) fn entry_live(&self) -> u16 {
        self.entry_live
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
    pub(crate) fn capacity_bytes(&self) -> usize {
        self.masks.get().map_or(0, |masks| masks.bytes())
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
    /// Canonical atom order for construction, scope projection and decoding.
    pub(crate) fn sort_by_atom(bindings: &mut [Self]) {
        bindings.sort_unstable_by_key(|binding| binding.atom);
    }
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

pub(crate) fn private_identity_label(identity: &str) -> Option<&str> {
    let identity = identity.strip_prefix("\0quench:private:")?;
    let (_, label) = identity.rsplit_once(':')?;
    (!label.is_empty()).then_some(label)
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
    /// A Number that is an int32 (`Value::integer`), held in the step: the elements of a
    /// bundle's data tables (text-encoding indexes: about 80K in JetStream web-ssr's) then
    /// neither enter nor search the constant table.
    Int(i32),
    /// An array of the given number of elements.
    Array(u32),
    /// An object of an object site's keys, one value each.
    Object(u32),
    /// The value of the JSON text at this byte of the program's source: a data script's
    /// literal (`compile::data_literal`), which the source holds in place of its tree.
    Json(u32),
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

    #[inline(always)] pub(crate) fn register_index(self) -> Option<Register> {
        (self.0 <= REGISTER_MASK).then_some(self.0)
    }

    pub(crate) fn local(value: u16) -> Option<Self> {
        (value <= REGISTER_MASK).then_some(Self(Self::LOCAL_FLAG | value))
    }

    #[inline(always)] pub(crate) fn local_index(self) -> Option<u16> {
        (self.0 & Self::LOCAL_FLAG != 0
            && self.0 & !(Self::LOCAL_FLAG | REGISTER_MASK) == 0)
            .then_some(self.0 & REGISTER_MASK)
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
    pub(crate) const PAYLOAD_MASK: u16 = (1 << Self::TAG_SHIFT) - 1;

    #[inline] pub(crate) const fn kind(self) -> Option<OperandKind> {
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

    #[inline] pub(crate) fn payload(self) -> u16 {
        self.0 & Self::PAYLOAD_MASK
    }

    #[inline(always)] pub(crate) fn register_index(self) -> Option<Register> {
        (self.kind() == Some(OperandKind::Register)).then_some(self.payload())
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
    /// The text of a guest string the program compiled from (an eval's source), shared with
    /// that string's storage.
    Local(std::rc::Rc<str>),
    Static(&'static str),
    Kept(KeptText),
    /// The text of an eval's guest string that was a rope of ASCII text: the program keeps the
    /// rope, whose pieces other strings share, and makes the text flat only when read, never
    /// inside the string (`RopeText`).
    Rope(RopeText),
}

/// A rope's text, made flat on first read into a buffer of its own. Inspector code-load
/// evaluates `function testN() { ${payload} }` per iteration: each program kept its own
/// 2 MB flat copy for the function `testN` it defines, never read again, where the rope's
/// pieces share the payload.
#[derive(Clone)]
pub(crate) struct RopeText {
    string: crate::vm::wtf16::JsString,
    flat: std::rc::Rc<std::cell::OnceCell<std::rc::Rc<str>>>,
}

impl RopeText {
    /// The text's length in bytes, which for ASCII is its units, read without making it flat.
    pub(crate) fn len_units(&self) -> usize {
        self.string.len()
    }

    /// `string`'s text kept as its rope, when it is an unflattened rope of ASCII text.
    pub(crate) fn of(string: &crate::vm::wtf16::JsString) -> Option<Self> {
        string.is_unflattened_ascii_rope().then(|| Self { string: string.clone(), flat: Default::default() })
    }
}

impl std::ops::Deref for RopeText {
    type Target = str;
    fn deref(&self) -> &str {
        self.flat.get_or_init(|| {
            let units = self.string.flat_unkept();
            std::rc::Rc::from(units.ascii_str().expect("an ASCII rope flattens to ASCII text"))
        })
    }
}

impl std::fmt::Debug for RopeText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("RopeText").field("len", &self.string.len()).field("flat", &self.flat.get().is_some()).finish()
    }
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

/// What the host does with a deferred record's bytes once they are decoded, set once by the
/// embedding: the decoded function lives on the heap, so the bytes are read again only if the
/// record is released and decoded anew, and a mapped compile-cache entry need not keep them
/// resident (Air: 94 of a 123-page entry resident at its peak, its records decoded).
static RECORD_READ: std::sync::OnceLock<fn(&[u8])> = std::sync::OnceLock::new();

/// Sets `RECORD_READ`; the first call wins.
pub fn set_record_read(action: fn(&[u8])) {
    let _ = RECORD_READ.set(action);
}

/// What the host does each time a function's record is built on its first use (lowered, decoded
/// or compiled again), set once by the embedding: new code joining a run is the work its
/// code-page policy follows (a work clock that runs without collections).
static RECORD_BUILT: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Sets `RECORD_BUILT`; the first call wins.
pub fn set_record_built(action: fn()) {
    let _ = RECORD_BUILT.set(action);
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

impl SourceText {
    /// The text's length in bytes, read without making a rope flat.
    #[allow(clippy::len_without_is_empty)]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Rope(rope) => rope.len_units(),
            text => str::len(text),
        }
    }
}

impl std::ops::Deref for SourceText {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Self::Shared(text) => text,
            Self::Local(text) => text,
            Self::Static(text) => text,
            Self::Kept(text) => text,
            Self::Rope(text) => text,
        }
    }
}

/// The callee a failed call at a source byte offset names in its error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CallSiteDescription {
    pub(crate) offset: u32,
    pub(crate) construct: bool,
    pub(crate) callee: String,
    /// Node's first-line expression, as a range in the admitted source snapshot.
    pub(crate) expression: Option<SourceSpan>,
}

/// Compiler source snapshot and wrapper origin, retained for diagnostics.
#[derive(Clone, Debug)]
pub(crate) struct ProgramSource {
    pub(crate) text: SourceText,
    pub(crate) typescript: bool,
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
    /// Complete static call-printing facts for native images. An unset cell
    /// leaves the dynamic error-path renderer in charge; an empty table is final.
    pub(crate) call_sites: std::cell::OnceCell<std::sync::Arc<[CallSiteDescription]>>,
}

impl ProgramSource {
    /// Project admitted facts with a retained wrapper body, without reparsing it.
    #[cfg(feature = "aot")]
    pub(crate) fn retain_body(&mut self, text: SourceText, base: u32) {
        self.text = text;
        self.base = base;
        self.line_starts.take();
        if let Some(sites) = self.call_sites.take() {
            self.call_sites = std::cell::OnceCell::from(std::sync::Arc::from(sites.iter().filter_map(|site| {
                let offset = site.offset.checked_sub(base)?;
                if offset as usize >= self.text.len() { return None; }
                let expression = site.expression.and_then(|span| Some(SourceSpan {
                    start: span.start.checked_sub(base)?, end: span.end.checked_sub(base)?,
                })).filter(|span| span.end as usize <= self.text.len());
                Some(CallSiteDescription { offset, construct: site.construct, callee: site.callee.clone(), expression })
            }).collect::<Vec<_>>()));
        }
    }

    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        // `line_starts` is a memo of `text`, made again on the first line read.
        crate::vm::freeze_source_call_sites(self);
        let Self { text, typescript, base, line_offset, column_offset, line_starts: _, call_sites } = self;
        text.write_snapshot(writer);
        writer.bool(*typescript);
        writer.u32(*base);
        writer.u32(*line_offset);
        writer.u32(*column_offset);
        writer.bool(call_sites.get().is_some());
        if let Some(sites) = call_sites.get() {
            writer.len(sites.len());
            for site in sites.iter() {
                writer.u32(site.offset);
                writer.bool(site.construct);
                writer.str(&site.callee);
                writer.bool(site.expression.is_some());
                if let Some(span) = site.expression {
                    writer.u32(span.start);
                    writer.u32(span.end);
                }
            }
        }
    }

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let mut source = Self {
            text: SourceText::read_snapshot(reader)?,
            typescript: reader.bool()?,
            base: reader.u32()?,
            line_offset: reader.u32()?,
            column_offset: reader.u32()?,
            line_starts: std::cell::OnceCell::new(),
            call_sites: Default::default(),
        };
        if reader.bool()? {
            let count = reader.len()?;
            let mut sites = Vec::with_capacity(count);
            for _ in 0..count {
                sites.push(CallSiteDescription {
                    offset: reader.u32()?, construct: reader.bool()?, callee: reader.str()?.into(),
                    expression: if reader.bool()? {
                        Some(SourceSpan { start: reader.u32()?, end: reader.u32()? })
                    } else { None },
                });
            }
            source.call_sites = std::cell::OnceCell::from(std::sync::Arc::from(sites));
            source.validate_call_sites().map_err(crate::snapshot::SnapshotError)?;
        }
        Ok(source)
    }

    pub(crate) fn validate_call_sites(&self) -> Result<(), String> {
        if let Some(sites) = self.call_sites.get() {
            let mut previous = None;
            for site in sites.iter() {
                let key = (site.offset, site.construct);
                if previous.is_some_and(|old| old >= key) || !self.text.is_char_boundary(site.offset as usize) || site.callee.is_empty() {
                    return Err("invalid frozen call-site descriptions".into());
                }
                if let Some(span) = site.expression
                    && (span.start > site.offset || span.end < site.offset || span.start >= span.end
                        || !self.text.is_char_boundary(span.start as usize)
                        || !self.text.is_char_boundary(span.end as usize)
                        || self.text[span.start as usize..span.end as usize].contains(['\r', '\n', '\u{2028}', '\u{2029}']))
                    { return Err("invalid frozen call-site expression".into()); }
                previous = Some(key);
            }
        }
        Ok(())
    }

    pub(crate) fn byte_offset(&self, position: SourcePosition) -> Option<u32> {
        let line = self.source_line(position.line)?;
        let column = position.column.checked_sub(1)?;
        let line_start = line.as_ptr() as usize - self.text.as_ptr() as usize;
        let mut units = 0;
        for (byte, character) in line.char_indices() {
            if units == column { return u32::try_from(line_start + byte).ok(); }
            units += character.len_utf16() as u32;
        }
        (units == column).then(|| u32::try_from(line_start + line.len()).ok()).flatten()
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
    pub(crate) constants: append_vec::AppendVec<Constant>,
    pub(crate) functions: append_vec::AppendVec<FunctionSlot>,
    pub(crate) cache_sites: chunk_append::SiteCount,
    pub(crate) method_sites: append_vec::AppendVec<MethodSite>,
    pub(crate) method_arguments: append_vec::AppendVec<Register>,
    pub(crate) field_sites: append_vec::AppendVec<FieldSite>,
    pub(crate) object_sites: append_vec::AppendVec<ObjectSite>,
    pub(crate) regexp_literal_sites: append_vec::AppendVec<RegExpLiteralSite>,
    pub(crate) literal_templates: append_vec::AppendVec<Box<[LiteralStep]>>,
    pub(crate) superinstructions: append_vec::AppendVec<Superinstruction>,
    pub(crate) rewrite_ranges: crate::compile::replay::RewriteRanges,
    /// How its functions can be compiled again from source, when they can.
    pub(crate) replay: Option<std::rc::Rc<crate::compile::replay::ProgramReplay>>,
    /// Each nested first-call stub's scopes, by stub id, which an encoding keeps with the stubs.
    pub(crate) first_call_scopes: crate::compile::first_call::FirstCallStubs,
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
    /// The kept code of a Wasm module whose functions lower on their first calls: a
    /// released slot's record is lowered from it (`WasmLazy::lower`).
    pub(crate) wasm_lazy: Option<std::rc::Rc<crate::wasm::WasmLazy>>,
    /// The compile-cache records of the functions its load released, encoded as each was
    /// finished, until its compact encoding takes them (`binary::LoadEncoding`).
    pub(crate) load_encoding: LoadRecords,
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
        !instruction.op().addresses_local_slot() || instruction.local_slot() < usize::from(locals)
    }) && wide.iter().all(|instruction| {
        !instruction.op().addresses_local_slot() || instruction.local_slot() < usize::from(locals)
    })
}

impl ResidualProgram {
    /// Original compiler text used by diagnostics and Function.prototype.toString.
    pub fn source_text(&self) -> Option<&str> {
        self.source.as_ref().map(|source| source.text.as_ref())
    }

    /// The source from compiled byte `start` on, where a data script's literal starts
    /// (`LiteralStep::Json`); `None` when the program keeps no text there.
    pub(crate) fn data_literal_text(&self, start: u32) -> Option<&str> {
        let source = self.source.as_ref()?;
        let text: &str = source.text.as_ref();
        text.get(start.checked_sub(source.base)? as usize..)
    }

    /// Resolve source-derived native error descriptions before publication.
    #[cfg(feature = "aot")]
    pub fn freeze_native_diagnostics(&self) {
        crate::vm::freeze_call_sites(self);
    }
    /// The record of function `index`, compiled again from source on its first read when
    /// the program released it.
    #[inline(always)] pub(crate) fn function(&self, index: usize) -> &Function {
        let slot = &self.functions[index];
        match slot.record() {
            Some(function) => function,
            None => self.recompile_function(index),
        }
    }

    /// `function`, for an index that may lie outside the table.
    #[inline(always)] pub(crate) fn function_get(&self, index: usize) -> Option<&Function> {
        (index < self.functions.len()).then(|| self.function(index))
    }

    #[cold]
    #[inline(never)]
    fn recompile_function(&self, index: usize) -> &Function {
        let lowered = || &**self.functions[index].record.get_or_init(|| std::rc::Rc::new(self.built_record(index, RecordUse::Kept)));
        self.tier0_record(index).unwrap_or_else(lowered)
    }

    /// `write` applied to `index`'s record: the record itself when it is compiled, else one
    /// built for this call alone and dropped after it. Encoding a program reads every
    /// record once; compiling its released ones all again for that held them all at once
    /// (typescript.js: 21.7K functions, +38 MB at a cold compile's peak).
    pub(crate) fn with_record<R>(&self, index: usize, write: impl FnOnce(&Function) -> R) -> R {
        // A record running unlowered (`tier0`) is not the lowered one encoding reads.
        match self.functions[index].record.get() {
            Some(function) => write(function),
            None => write(&self.built_record(index, RecordUse::Transient)),
        }
    }

    /// `index`'s record, from its unlowered form, its encoded entry or its source; the host
    /// counts each as work (`set_record_built`).
    fn built_record(&self, index: usize, purpose: RecordUse) -> Function {
        let function = self.build_record(index, purpose);
        if let Some(action) = RECORD_BUILT.get() {
            action();
        }
        function
    }

    fn build_record(&self, index: usize, purpose: RecordUse) -> Function {
        let unlowered = {
            let mut unlowered = self.functions[index].unlowered.borrow_mut();
            match purpose {
                RecordUse::Kept => unlowered.take().map(Unlowered::into_function),
                RecordUse::Transient => unlowered.as_ref().map(Unlowered::function),
            }
        };
        if let Some(unlowered) = unlowered {
            if !crate::DYNAMIC_EXECUTION_ENABLED {
                panic!("native function {index} of {} was not lowered before linking", self.source_name);
            }
            let function = self.lower_unlowered(index, unlowered);
            function.decoded.prime_loop(&function.code, &function.wide);
            return function;
        }
        if let Some(lazy) = &self.wasm_lazy {
            let function = lazy.lower(index, &self.constants);
            function.decoded.prime_loop(&function.code, &function.wide);
            return function;
        }
        let function = self.decode_archived_record(index).unwrap_or_else(|| self.recompile_record(index));
        // As `ResidualProgram::prime_loop_decoding` primes an installed program's records.
        function.decoded.prime_loop(&function.code, &function.wide);
        function
    }

    /// The archived record of function `index`, decoded, or `None` when the program holds none.
    fn decode_archived_record(&self, index: usize) -> Option<Function> {
        let deferred = self.encoded_records.as_deref().and_then(|records| binary::decode_deferred_function(records, index))?;
        let function = deferred.unwrap_or_else(|error| {
            panic!("function {index} of {} does not decode: {error}", self.source_name)
        });
        if let (Some(action), Some(bytes)) = (
            RECORD_READ.get(),
            self.encoded_records.as_deref().and_then(|records| binary::deferred_record_bytes(records, index)),
        ) {
            action(bytes);
        }
        Some(function)
    }

    /// Compiles function `index` again from its source, for a program that archived no record of it.
    fn recompile_record(&self, index: usize) -> Function {
        if !crate::DYNAMIC_EXECUTION_ENABLED {
            panic!("native function {index} of {} has no archived record", self.source_name);
        }
        crate::compile::replay::recompile(self, index as u32)
            .unwrap_or_else(|error| {
                panic!("function {index} of {} does not compile again: {error}", self.source_name)
            })
            .function
    }

    /// An unlowered record lowered against the finished program's tables, as its load
    /// would have lowered it (`compile::lower_settled`), with its register roots.
    #[cold]
    #[inline(never)]
    fn lower_unlowered(&self, index: usize, mut function: Function) -> Function {
        if self.rewrite_ranges.kept_pending { crate::compile::settle_deferred_rewrite(self, index as u32, &mut function); }
        let undefined_constant = *self.undefined_constant.get_or_init(|| {
            let index = self.constants.iter().position(|constant| matches!(constant, Constant::Undefined))
                .expect("a program with an unlowered record holds the `undefined` constant");
            u32::try_from(index).expect("constant index fits u32")
        });
        crate::compile::lower_settled(
            &mut function,
            &crate::compile::LoweringTables {
                object_sites: &self.object_sites,
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
    #[allow(dead_code)]
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
                crate::compile::liveness::RegisterMasks::of(function, self, &self.field_sites, &self.superinstructions)
                    .map(std::rc::Rc::new)
                    .expect("a covered function's liveness is derivable")
            });
        Some(masks)
    }

    pub(crate) fn source_position(&self, function: u32, pc: u32) -> Option<SourcePosition> {
        let position = self.function_get(function as usize)?.source_positions.at(pc)?;
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

    /// Whether the entry uses module semantics, including an import-only bootstrap.
    pub fn is_module(&self) -> bool {
        self.kind == ProgramKind::Module
    }

    /// Declared module requests in source order, before execution. Hosts can
    /// resolve and compile a static graph from the compiler's syntax result.
    pub fn module_requests(&self) -> &[ModuleRequest] {
        &self.module_requests
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
            RootSlotRule::GlobalVar(root.local_atoms[slot])
        } else if declaration.global_lexical && self.kind == ProgramKind::Script {
            RootSlotRule::GlobalLexical(root.local_atoms[slot])
        } else {
            RootSlotRule::Binding
        }
    }

    /// Every function, compiled where its slot is not yet (tests).
    #[cfg(test)]
    pub(crate) fn compiled_functions(&self) -> impl Iterator<Item = &Function> {
        (0..self.function_count()).map(|index| self.function(index))
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

    /// Read the same validated metadata from an embedded native image asset.
    pub fn from_binary_bytes(bytes: KeptBytes) -> Result<Self, String> {
        binary::read_program_bytes(bytes)
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
    /// The sizes the program's inline-cache site tables may reach, which a VM lays out per
    /// program: the room a program that appends chunks reserved is laid out with it.
    pub(crate) fn cache_site_counts(&self) -> CacheSiteCounts {
        CacheSiteCounts {
            fields: self.cache_sites.capacity(),
            methods: self.method_sites.capacity(),
            objects: self.object_sites.capacity(),
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
mod tier0;
#[cfg(feature = "native-execution")]
mod native_metadata;
#[cfg(feature = "native-execution")]
pub use native_metadata::NativeProgramNamespace;
mod chunk_append;
pub(crate) use chunk_append::{SiteCount, ChunkDelta, TableLengths};
mod closure_reads;
pub(crate) use closure_reads::{ClosureReads, EnvironmentDemand, FunctionReadFacts, MakerReads};
pub(crate) use binary::{decode_static_module_analysis, encode_static_module_analysis, LoadEncoding, LoadRecords};
mod disassemble;
mod flow_view;
pub use flow_view::{FlowFunction, FlowInstruction};
#[cfg(test)]
mod tests;
mod validate;
mod function_facts;

/// How many levels of arrows `arrow_chain_reads_implicit_bindings` follows.
const ARROW_CHAIN_DEPTH: u8 = 8;

/// Whether function `id` of `code`, an arrow, or an arrow it makes through arrows alone,
/// reads an implicit binding of the activation it would resolve in: `this`, `arguments`,
/// `new.target`, a home object or any by-name binding (bounded at eight levels, past which
/// it answers `true`). A non-arrow binds its own.
pub(crate) fn arrow_chain_reads_implicit_bindings(code: &ResidualProgram, id: u32) -> bool {
    fn reads(code: &ResidualProgram, id: u32, depth: u8) -> bool {
        let Some(depth) = depth.checked_sub(1) else { return true };
        let Some(function) = code.function_get(id as usize) else { return true };
        if !function.is_arrow {
            return false;
        }
        function.instructions().any(|instruction| {
            let op = instruction.op();
            op.immediate_role() == ImmediateRole::SuperinstructionIndex
                || op.reads_arguments()
                || match op {
                    Op::LoadThis | Op::InitializeThis | Op::SuperCallCheck | Op::LoadCallee | Op::LoadName
                    | Op::LoadNameCall | Op::LoadNameTypeof | Op::ResolveName | Op::LoadResolvedName | Op::DeleteName
                    | Op::StoreName | Op::StoreResolvedName | Op::StoreVarBinding => true,
                    Op::MakeClosure => reads(code, instruction.closure_function_index(), depth),
                    _ => false,
                }
        })
    }
    reads(code, id, ARROW_CHAIN_DEPTH)
}
