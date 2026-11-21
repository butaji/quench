use smallvec::SmallVec;

mod internal_bindings;
mod module_records;
mod declared_types;
pub(crate) use declared_types::{
    Declared, DeclaredKind, DeclaredLocal, DeclaredLocalMember, DeclaredMember, DeclaredParameter, DeclaredParameterClass, DeclaredSignature,
    DeclaredTypes, EnumConstant, SavedTypeName,
};
pub use module_records::{ModuleImportBinding, ModuleImportName, ModuleRequest, ModuleRequestPhase};
pub(crate) use module_records::{ModuleImportNameKind, ModuleLinkPlan, ModuleReexport, ModuleReexportKind};
pub(crate) use internal_bindings::*;
pub(crate) const LEXICAL_THIS_BINDING: &str = "\0quench:lexical-this";
pub(crate) const NEW_TARGET_BINDING: &str = "\0quench:new-target";

pub type Atom = u32;
pub type Register = u16;

mod slot_records;
pub(crate) use slot_records::{DeferredRecord, ReceiverPath};
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
pub(crate) use field_store::{FieldStore, StoreReceiver};
mod call_plan;
pub(crate) use call_plan::{CALL_PLAN_FLAG_COUNT, CallPlan};
mod instruction;
mod register_list;
mod name_bindings;
pub(crate) use name_bindings::{CapturedNames, NameBindings};
mod numeric_ops; mod op_classes; mod op_scan; mod typeof_names; mod unary_operators; mod rest_alias;
pub(crate) use unary_operators::*;
mod source_positions;
pub(crate) use source_positions::{SourcePositions, SourcePositionsBuilder};
mod function_caches;
pub(crate) use function_caches::{FunctionCaches, LazyFlag};
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
pub(crate) use control_flow::{ControlEdge, Successors, branch_table_entries, entered_only_by_fallthrough, for_each_instruction_successor, may_repeat, reachable_from};
pub use instruction::Instr;
pub(crate) use instruction::WideInstruction;
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
/// The realm index a global lexical declaration's slot holds while the active realm binds no
/// binding of its name there (`GlobalDeclarations::slots`).
pub(crate) const NO_REALM_BINDING: u16 = u16::MAX;
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
    /// Boxed so that the rare wide payloads do not size every constant of the program table
    /// (a `BigInt` is 32 bytes inline; boxed, the enum is 24 bytes).
    StringUnits(Box<[u16]>),
    BigInt(Box<num_bigint::BigInt>),
    WasmBits64(u64),
    WasmV128([u8; crate::wasm::V128_BYTES]),
    Boolean(bool),
    Null,
    Undefined,
}
const _: () = assert!(size_of::<Constant>() == 24);
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
    /// The payload type a Wasm exception field read carries (`staged::wasm_exception::PayloadType`).
    WasmPayloadType,
    /// A struct read's field's static payload offset, a 16-bit byte offset carried in the operand
    /// (`wasm::gc::static_offset`); not a register.
    StaticOffset,
}

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
            | Self::SignedConstant
            | Self::WasmPayloadType
            | Self::StaticOffset => BindingTime::Static,
        }
    }
}

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
            | Self::GlobalLexicalIndex
            | Self::IntCaseMinimum
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
    /// The lowest case value of a dense int switch (`IntCaseIndex`), an `i32` as its bits.
    IntCaseMinimum,
    AtomIndex,
    LocalSlot,
    GlobalLexicalIndex,
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
    #[inline(always)]
    const fn field(self, field: InstructionField) -> FieldLayout {
        match field {
            InstructionField::A => self.a,
            InstructionField::B => self.b,
            InstructionField::C => self.c,
        }
    }
}

impl ResultLayout {
    #[inline(always)]
    pub(crate) const fn allowed_flags(self) -> Register {
        match self {
            Self::Register => NO_RESULT_FLAGS,
            Self::RegisterReadWrite => NO_RESULT_FLAGS,
            Self::Returnable => RETURN_REGISTER,
            Self::ReturnableAndThis => RETURN_REGISTER | SET_THIS_REGISTER,
            Self::NumericReturnable => RETURN_REGISTER | NUMERIC_LOCAL_TARGET,
        }
    }

    pub(crate) const fn allows_return(self) -> bool {
        matches!(
            self,
            Self::Returnable | Self::ReturnableAndThis | Self::NumericReturnable
        )
    }

    pub(crate) const fn allows_this_write(self) -> bool {
        matches!(self, Self::ReturnableAndThis)
    }

    pub(crate) const fn reads_result_register(self) -> bool {
        matches!(self, Self::RegisterReadWrite)
    }

    #[inline(always)]
    pub(crate) const fn allows_numeric_local(self) -> bool {
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

    #[inline(always)]
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
    /// Lane call records carry no eval flags (`call_fits_in_lane`), so the flag reuses the
    /// direct-eval bit; a callee whose result is unobservable may skip building it.
    pub(crate) const LANE_CALL_RESULT_UNUSED_FLAG: u32 = Self::DIRECT_EVAL_FLAG;

    pub(crate) const fn call_result_used_in_lane(imm: u32) -> bool {
        imm & Self::LANE_CALL_RESULT_UNUSED_FLAG == 0
    }

    /// A lane `Call` record whose result only the `Await` right after it reads
    /// (`derive_lane_view`). Lane call records carry no eval flags, so the flag reuses the
    /// parameter-eval bit; a settling callee there needs no promise for a primitive result.
    pub(crate) const LANE_CALL_RESULT_AWAITED_FLAG: u32 = Self::PARAMETER_EVAL_FLAG;

    pub(crate) const fn call_result_awaited_in_lane(imm: u32) -> bool {
        imm & Self::LANE_CALL_RESULT_AWAITED_FLAG != 0
    }

    #[inline(always)]
    pub(crate) const fn direct_eval(imm: u32) -> bool {
        imm & Self::DIRECT_EVAL_FLAG != 0
    }

    #[inline(always)]
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

    #[inline]
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


mod op_catalog; mod op_roles; mod denotation; mod opcode_table;
pub(crate) use op_catalog::{Class, Decoder, GenericStep, JsDenotation, OwnedStep, Traits, ValueStep, OPCODE_COUNT, opcode_name};
pub(crate) use op_roles::{CALL_INVOKE, ELEMENT_STORE, JUMP_IF_FALSE, OBJECT_FIELD_STORE, OpSet, REGISTER_MOVE, RETURN_OP, negated_jump, op_table, row_ops, strict_variant, the_op};

macro_rules! opcodes {
    ($d:tt; $($name:ident => $effect:expr; layout $layout:ident $(; meaning $immediate_role:ident)? $(; flow $flow:ident)? $(; js $js_denotation:ident)? $(; class $class:ident)? $(; staged $decoder:ident)? $(; step $step:ident)? $(; owned $owned:ident)? $(; value $value:ident)? $(; traits $($trait:ident)+)?, @ $result:ident, @ fields($a:ident, $b:ident, $c:ident)),+ $(,)?) => {
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
            #[inline(always)] pub(crate) const fn class(self) -> Class { Self::CLASSES[self as usize] }
            const TRAITS: [Traits; Self::COUNT] = [$(opcodes!(@traits $($($trait)+)?)),+];
            #[inline(always)] pub(crate) const fn traits(self) -> Traits { Self::TRAITS[self as usize] }
            const DECODERS: [Option<Decoder>; Self::COUNT] = [$(opcodes!(@staged $($decoder)?)),+];
            #[inline(always)] pub(crate) const fn staged(self) -> Option<Decoder> { Self::DECODERS[self as usize] }
            const GENERIC_STEPS: [Option<GenericStep>; Self::COUNT] = [$(opcodes!(@step $($step)?)),+];
            #[inline(always)] pub(crate) const fn generic_step(self) -> Option<GenericStep> { Self::GENERIC_STEPS[self as usize] }
            const OWNED_STEPS: [Option<OwnedStep>; Self::COUNT] = [$(opcodes!(@owned $($owned)?)),+];
            #[inline(always)] pub(crate) const fn owned_step(self) -> Option<OwnedStep> { Self::OWNED_STEPS[self as usize] }
            const VALUE_STEPS: [Option<ValueStep>; Self::COUNT] = [$(opcodes!(@value $($value)?)),+];
            #[inline(always)] pub(crate) const fn value_step(self) -> Option<ValueStep> { Self::VALUE_STEPS[self as usize] }
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

            #[inline(always)] pub(crate) const fn effect(self) -> Effect {
                Self::EFFECTS[self as usize]
            }

            #[inline(always)] pub(crate) const fn immediate_layout(self) -> ImmediateLayout {
                Self::IMMEDIATE_LAYOUTS[self as usize]
            }

            #[inline(always)]
            pub(crate) const fn result_layout(self) -> ResultLayout {
                Self::RESULT_LAYOUTS[self as usize]
            }

            #[inline(always)] pub(crate) const fn control_flow_layout(self) -> ControlFlowLayout {
                Self::CONTROL_FLOW_LAYOUTS[self as usize]
            }

            #[inline(always)] pub(crate) const fn field_layout(self, field: InstructionField) -> FieldLayout {
                Self::OPERAND_LAYOUTS[self as usize].field(field)
            }

            #[inline(always)] pub(crate) const fn immediate_role(self) -> ImmediateRole {
                Self::IMMEDIATE_ROLES[self as usize]
            }

            #[inline(always)] pub(crate) const fn js_denotation(self) -> JsDenotation {
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

/// The catalog's representative opcode of `$class`: the first entry whose class is `$class`. Each
/// class below names one row family, so tiers install and compare rows through these constants
/// and never spell an opcode.
macro_rules! representative {
    ($($name:ident => $class:ident),+ $(,)?) => {$(
        pub(crate) const $name: Op = {
            let mut index = 0;
            loop {
                match Op::from_index(index) {
                    Some(op) if matches!(op.class(), Class::$class) => break op,
                    Some(_) => index += 1,
                    None => panic!("the catalog names a row of this class"),
                }
            }
        };
    )+};
}

representative! {
    FIELD_READ => GetField,
    CALL_ROW => Call,
    CONSTRUCT_ROW => Construct,
    RETURN_ROW => Return,
    UNARY_ROW => Unary,
    BINARY_ROW => Binary,
    WASM_F64_UNARY_ROW => WasmFloatUnary64,
    PRIVATE_FIELD_DEFINE => DefinePrivateField,
    PRIVATE_NAME_MARK => MarkPrivateName,
    PRIVATE_CHECK => CheckPrivate,
    PRIVATE_IN => PrivateIn,
    PROPERTY_RECORD_DEFINE => DefinePropertyRecord,
    FIELD_DEFINE => DefineField,
    COPY_DATA_PROPERTIES => CopyDataProperties,
    CALL_ARRAY => CallArray,
    CALL_SPREAD => CallSpread,
    DELETE_ROW => Delete,
    FORWARDED_CALL => CallForwardingArguments,
    OBJECT_PAIR => MakeObject2,
    OBJECT_LITERAL_ROW => MakeObjectLiteral,
    SUPER_CONST_ARRAY_OBJECT => SuperConstArrayObject2,
    TIER_PROLOGUE => TierPrologue,
    BODY_ENTRY => BodyEntry,
}

const READ_THROW: Effect = Effect::READS_HEAP.union(Effect::THROWS);
const WRITE_THROW: Effect = Effect::WRITES_HEAP.union(Effect::THROWS);
const CALL_EFFECT: Effect = READ_THROW.union(Effect::WRITES_HEAP);

opcodes!($;
    Nop => Effect::PURE; layout Scalar; meaning Unused; js Noop; class Nop; staged Flow; traits LEAF_VALUE SCALAR_REGION FRAME_ONLY NO_OP NO_STEP, @ Register, @ fields(Unused, Unused, Unused),
    CloneEnv => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning EnvironmentCloneIndex; class CloneEnv; staged Names; step Body; traits NAME_OPERATION ENVIRONMENT_CLONE ENVIRONMENT_CAPTURE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Unused, Unused, Unused),
    Wide => Effect::PURE; layout Scalar; meaning WideInstructionIndex; traits NO_STEP, @ Register, @ fields(WideIndexChunk, WideIndexChunk, WideIndexChunk),
    LoadConst => Effect::PURE; layout Scalar; meaning ConstantIndex; js LoadConst; class LoadConst; staged Flow; step Body; traits LEAF_VALUE INLINABLE PURE_SETUP ACTIVATION_FREE SCALAR_REGION LIFETIME_SAFE REMOVABLE_WHEN_DEAD CONSTANT_LOAD MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadLocal => Effect::PURE; layout Scalar; meaning LocalSlot; class LoadLocal; staged Local; step Body; owned LoadLocal; traits ROW_HANDLER ACTIVATION_FREE FRAME_LOCAL_LOAD RELOCATABLE_LOCAL LOCAL_BINDING_LOAD FRAME_SLOT_READ, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocal => Effect::PURE; layout Scalar; meaning LocalSlot; class StoreLocal; staged Local; step Body; owned StoreLocal; traits ROW_HANDLER ACTIVATION_FREE FRAME_LOCAL_STORE LOCAL_BINDING_STORE RELOCATABLE_LOCAL LOCAL_SLOT_STORE, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    StoreVarBinding => CALL_EFFECT; layout Scalar; meaning LocalSlot; class StoreVarBinding; step Body; owned StoreVarBinding; traits NAME_OPERATION BINDING_FORM MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    LoadEnvLocal => Effect::READS_HEAP; layout Scalar; meaning LocalSlot; class LoadEnvLocal; staged Names; step Body; owned LoadLocal; traits ENV_LOCAL_LOAD ENV_LOCAL_ACCESS ROW_HANDLER OWNER_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreEnvLocal => Effect::WRITES_HEAP; layout Scalar; meaning LocalSlot; class StoreEnvLocal; staged Names; step Body; owned StoreEnvLocal; traits ENV_LOCAL_STORE ENV_LOCAL_ACCESS LOCAL_SLOT_STORE ROW_HANDLER OWNER_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    LoadCapture => Effect::READS_HEAP; layout CaptureDepthAndSlot; js CapturedValue; class LoadCapture; staged Value; step Body; owned LoadCapture; traits ROW_HANDLER BOXED_RESULT INLINABLE CAPTURE_LOAD CAPTURE_ACCESS MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreCapture => Effect::WRITES_HEAP; layout CaptureDepthAndSlot; class StoreCapture; staged Value; step Body; owned StoreCapture; traits ROW_HANDLER CAPTURE_STORE CAPTURE_ACCESS MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    LoadName => READ_THROW; layout Scalar; meaning AtomIndex; class LoadName; staged Proved; step Body; owned LoadName; traits BOXED_RESULT UNIT_REWRITTEN NAME_LEAF NAME_OPERATION GLOBAL_NAME_ACCESS NAME_LOAD BY_NAME_ACCESS ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, CacheSiteIndex),
    LoadNameCall => READ_THROW; layout Scalar; meaning AtomIndex; class LoadNameCall; staged Proved; step Body; owned LoadNameCall; traits NAME_OPERATION CALL_LINK GLOBAL_NAME_ACCESS NAME_LOAD BY_NAME_ACCESS ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, WriteRegister, CacheSiteIndex),
    LoadNameTypeof => Effect::READS_HEAP; layout Scalar; meaning AtomIndex; class LoadNameTypeof; staged Names; step Body; owned LoadNameTypeof; traits NAME_OPERATION GLOBAL_NAME_ACCESS NAME_LOAD BY_NAME_ACCESS DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, CacheSiteIndex),
    ResolveName => READ_THROW; layout Scalar; meaning AtomIndex; staged Names; step Body; traits NAME_OPERATION BINDING_FORM BY_NAME_ACCESS MAY_ALLOCATE, @ Register, @ fields(ResultRegister, BooleanFlag, CacheSiteIndex),
    LoadResolvedName => READ_THROW; layout Scalar; meaning AtomIndex; staged Names; step Body; traits NAME_OPERATION BINDING_FORM MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, BooleanFlag),
    DeleteName => READ_THROW; layout Scalar; meaning AtomIndex; staged Names; step Body; traits NAME_OPERATION BINDING_FORM BY_NAME_ACCESS MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    StoreName => WRITE_THROW; layout Scalar; meaning AtomIndex; class StoreName; staged Names; step Body; owned StoreName; traits NAME_OPERATION GLOBAL_NAME_ACCESS NAME_STORE BY_NAME_ACCESS ROW_HANDLER OWNER_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, BooleanFlag, CacheSiteIndex),
    // `imm` is the declaration index of a script-top-level lexical (`global_declarations.lexical`); each
    // falls back to the by-name operation of its atom when the binding is not yet initialized.
    LoadGlobalLexical => READ_THROW; layout Scalar; meaning GlobalLexicalIndex; step Body; owned LoadGlobalLexical; traits BOXED_RESULT UNIT_REWRITTEN MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, CacheSiteIndex),
    StoreGlobalLexical => WRITE_THROW; layout Scalar; meaning GlobalLexicalIndex; step Body; owned StoreGlobalLexical; traits MAY_ALLOCATE, @ Register, @ fields(Register, BooleanFlag, CacheSiteIndex),
    StoreResolvedName => WRITE_THROW; layout Scalar; meaning AtomIndex; staged Names; step Body; traits NAME_OPERATION BINDING_FORM MAY_ALLOCATE, @ Register, @ fields(Register, Register, BooleanFlag),
    LoadThis => READ_THROW; layout Scalar; meaning Unused; class LoadThis; staged Value; traits UNIT_REWRITTEN THIS_LEAF ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadCallee => Effect::PURE; layout Scalar; meaning Unused; class LoadCallee; staged Value; step Body; traits DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadImportMeta => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; staged Names; step Body; traits MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeClosure => CALL_EFFECT; layout Scalar; meaning ClosureFunctionIndex; class MakeClosure; staged Literal; step Body; traits MAKES_CLOSURE ENVIRONMENT_CAPTURE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeArray => CALL_EFFECT; layout Scalar; meaning ArrayLength; class MakeArray; staged Literal; step Body; traits ARRAY_MAKE DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, BooleanFlag, Unused),
    MakeConstArray => CALL_EFFECT; layout Scalar; meaning ConstantIndex; class MakeConstArray; staged Literal; step Body; traits CONST_ARRAY_OBJECT2_HEAD DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, ElementCount, Unused),
    // `a` = the position among the `b` string constants from `imm` of the first strictly equal
    // to register `c`, or `b` when none is: a string `switch`'s case selector, which a
    // `WasmBranchTable` follows.
    StringCaseIndex => Effect::PURE; layout Scalar; meaning ConstantIndex; class StringCaseIndex; staged Lane; step Body; traits ROW_HANDLER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, ElementCount, Register),
    // `a` = the slot among the `b` dense int labels from `imm` the discriminant `c` names, when it is an
    // integral double in the run, else `b`: an int `switch`'s jump table index.
    IntCaseIndex => Effect::PURE; layout Scalar; meaning IntCaseMinimum; staged Generic, @ Register, @ fields(ResultRegister, ElementCount, Register),
    MakeLiteralTemplate => CALL_EFFECT; layout Scalar; meaning LiteralTemplateIndex; staged Literal; step Body; traits DISTANT_PRODUCER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeObject => CALL_EFFECT; layout Scalar; meaning Unused; js ObjectAllocation; class MakeObject; staged Literal; step Body; traits OBJECT_MAKE DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    MakeObject2 => CALL_EFFECT; layout Scalar; meaning ObjectSiteIndex; class MakeObject2; staged Literal; step Body; traits RETURNING_OBJECT FRAME_RETURNABLE CONST_ARRAY_OBJECT2_TAIL PAIR_LITERAL_MAKE ROW_HANDLER MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, Register),
    MakeObjectLiteral => CALL_EFFECT; layout Scalar; meaning ObjectSiteIndex; class MakeObjectLiteral; staged Literal; step Body; traits RETURNING_OBJECT_LITERAL FRAME_RETURNABLE WINDOW_LITERAL_MAKE DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    SuperConstArrayObject2 => CALL_EFFECT; layout Scalar; meaning SuperinstructionIndex; class SuperConstArrayObject2; staged Literal; step Body; traits RETURNING_LITERAL FRAME_RETURNABLE FUSED_CONST_ARRAY_OBJECT2 ROW_HANDLER MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Unused, Unused),
    GetIterator => READ_THROW; layout Scalar; meaning Unused; class GetIterator; staged Names; step LaneResult; traits ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    GetAsyncIterator => READ_THROW; layout Scalar; meaning Unused; staged Names; step Body; traits ROW_HANDLER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    CreateRegExpLiteral => CALL_EFFECT; layout Scalar; meaning RegExpLiteralSiteIndex; class CreateRegExpLiteral; staged Literal; step Body; traits DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, OptionalReadRegister, Unused),
    IteratorClose => READ_THROW; layout Scalar; meaning Unused; class IteratorClose; staged Names; step LaneResult; traits ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    // ArrayAccumulation past a literal's static prefix: `length` elisions, then the optional
    // value (iterated when the flag marks a spread), appended at the array's length.
    AppendArray => CALL_EFFECT; layout Scalar; meaning ArrayLength; class AppendArray; staged Names; step LaneResult; traits ROW_HANDLER MAY_ALLOCATE ARRAY_APPEND, @ Register, @ fields(Register, OptionalReadRegister, BooleanFlag),
    RequireObjectCoercible => READ_THROW; layout Scalar; meaning Unused; class RequireObjectCoercible; staged Value; traits ROW_HANDLER INLINABLE PURE_SETUP LIFETIME_SAFE MAY_ALLOCATE INDEXED_RECEIVER, @ Register, @ fields(Unused, Register, Unused),
    RequireIteratorResult => READ_THROW; layout Scalar; meaning Unused; class RequireIteratorResult; staged Value; step Lane; traits ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(BooleanFlag, Register, Unused),
    // Reads the frame's `this` binding and continues or throws: a lane step in place
    // (`lane_general_step`) where the lane runs the derived constructor's frame.
    SuperCallCheck => READ_THROW; layout Scalar; meaning Unused; staged Names; step Lane; traits NAME_OPERATION CALL_LINK MAY_ALLOCATE BINDING_FORM SUPER_CALL_CHECK, @ Register, @ fields(Unused, Unused, Unused),
    IteratorCleanupPush => Effect::CONTROL; layout Scalar; meaning Unused; class IteratorCleanupPush; staged Names; step Lane; traits ROW_HANDLER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Register, Unused),
    IteratorCleanupPop => Effect::CONTROL; layout Scalar; meaning Unused; class IteratorCleanupPop; staged Names; step Lane; traits ROW_HANDLER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Unused, Unused, Unused),
    // `a` = the next key of the for-in state `b` (`Vm::for_in_next`) that is still an enumerable property; to `imm` once none is left.
    ForInNext => CALL_EFFECT; layout Scalar; meaning JumpTarget; flow ConditionalJump; class ForInNext; staged Names; step Lane; traits LANE_LOOP_EXIT ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    // `a` = the for-in state over `b` (`Vm::for_in_start`): its key snapshot, at the first key.
    ForInStart => CALL_EFFECT; layout Scalar; meaning Unused; class ForInStart; staged Names; step LaneResult; traits ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    GetEnumerated => READ_THROW; layout RegisterPair; staged Names; step Lane; traits BOXED_RESULT ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    SetFunctionName => Effect::WRITES_HEAP; layout Scalar; meaning AtomIndex; class SetFunctionName; staged Names; step Lane; traits ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    SetFunctionNameKey => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning FunctionNamePrefix; staged Names; step Lane; traits FUNCTION_NAME_KEY_SET ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Unused),
    InitializeTdz => Effect::PURE; layout Scalar; meaning LocalSlot; class InitializeTdz; staged Local; step Body; owned InitializeTdz; traits TDZ_INITIALIZE ROW_HANDLER, @ Register, @ fields(Unused, Unused, Unused),
    Await => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning Unused; class Await; staged Flow; traits SUSPENDS SUSPENSION_POINT MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    Yield => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning Unused; class Yield; staged Flow; traits SUSPENDS SUSPENSION_POINT MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    YieldStar => READ_THROW.union(Effect::CONTROL); layout RegisterPair; class YieldStar; staged Flow; traits SUSPENSION_POINT SUSPENDS MAY_ALLOCATE, @ RegisterReadWrite, @ fields(ResultRegister, Register, ReadWriteRegister),
    GetField => READ_THROW; layout Scalar; meaning FieldLookup; js GetField; class GetField; staged Property; step Body; traits BOXED_RESULT FIELD_LEAF RETURNING_FIELD INLINABLE PURE_SETUP FRAME_RETURNABLE ROW_ONLY MAY_ALLOCATE, @ ReturnableAndThis, @ fields(ResultRegister, FieldBase, FieldLookupCacheSiteIndex),
    GetIndex => READ_THROW; layout Scalar; meaning Unused; class GetIndex; staged Element; step Body; traits BOXED_RESULT INLINABLE PURE_SETUP ELEMENT_ACCESS LIFETIME_SAFE ELEMENT_LOAD INDEXED_RECEIVER DISTANT_PRODUCER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, NumericIndexOperand, NumericIndexOperand),
    ToPropertyKey => READ_THROW; layout Scalar; meaning Unused; class ToPropertyKey; staged Generic; step LaneResult; traits PROPERTY_KEY_CONVERT DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    ToNumeric => READ_THROW; layout Scalar; meaning Unused; class ToNumeric; staged Value; traits ROW_HANDLER INLINABLE PURE_SETUP LIFETIME_SAFE DISTANT_PRODUCER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    CopyDataProperties => CALL_EFFECT; layout Scalar; meaning Unused; class CopyDataProperties; staged Names; step Lane; traits MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    MarkPrivateName => WRITE_THROW; layout Scalar; meaning AtomIndex; class MarkPrivateName; staged Names; step Lane; traits PRIVATE_NAME_MARK INSTANCE_ELEMENT_DEFINE MAY_ALLOCATE, @ Register, @ fields(Unused, Register, Register),
    DefinePrivateField => WRITE_THROW; layout Scalar; meaning AtomIndex; class DefinePrivateField; staged Names; step Lane; traits PRIVATE_FIELD_DEFINE INSTANCE_ELEMENT_DEFINE MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    SetField => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldStore; staged Property; step Body; traits INLINABLE SLOPPY_STORE OBJECT_FIELD_STORE NAMED_FIELD_WRITE ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Register, CacheSiteIndex),
    DefineComputedField => WRITE_THROW; layout Scalar; meaning Unused; staged Names; step Lane; traits COMPUTED_FIELD_DEFINE INSTANCE_ELEMENT_DEFINE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    SetThisField => WRITE_THROW; layout Scalar; meaning AtomIndex; staged Property; step Body; traits INLINABLE THIS_FIELD_SINK SLOPPY_STORE THIS_FIELD_STORE ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    SetIndex => WRITE_THROW; layout Scalar; meaning BooleanFlag; class SetIndex; staged Element; step Body; traits INLINABLE ELEMENT_ACCESS LIFETIME_SAFE ELEMENT_STORE INDEXED_RECEIVER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    DefineArrayElement => WRITE_THROW; layout Scalar; meaning ArrayIndex; class DefineArrayElement; staged Literal; step Lane; traits ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Unused),
    Binary => READ_THROW; layout Scalar; meaning BinaryOperator; js NumericBinary; class Binary; staged Generic; step Body; value Binary; traits ARITHMETIC_BINARY BINARY_RESULT LEAF_BINARY RETURNING_BINARY INLINABLE PURE_SETUP ACTIVATION_FREE LIFETIME_SAFE CONST_ARRAY_OBJECT2_OPERAND DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    IncDec => READ_THROW; layout Scalar; meaning BooleanFlag; class IncDec; staged Value; traits ROW_HANDLER LEAF_VALUE INLINABLE PURE_SETUP SCALAR_REGION LIFETIME_SAFE INCREMENT_UPDATE ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    Unary => READ_THROW; layout Scalar; meaning UnaryOperator; js NumericUnary; class Unary; staged Value; step Body; value Unary; traits LEAF_VALUE RETURNING_UNARY INLINABLE PURE_SETUP ACTIVATION_FREE FRAME_RETURNABLE LIFETIME_SAFE ROW_ONLY ROW_HANDLER MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, Unused),
    // `a` = ToString of each register of the window `b` of `c` registers, concatenated in
    // order: a template literal's string (ECMA-262 13.15.5.1), made at once.
    StringConcat => READ_THROW; layout Scalar; meaning Unused; js StringConcatenation; staged Literal; step Body; traits STRING_CONCAT DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    Delete => READ_THROW; layout Scalar; meaning BooleanFlag; class Delete; staged Property; step Body; traits MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    CheckPrivate => READ_THROW; layout Scalar; meaning AtomIndex; class CheckPrivate; staged Names; step Lane; traits MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    PrivateIn => READ_THROW; layout Scalar; meaning AtomIndex; class PrivateIn; staged Names; step Lane; traits DISTANT_PRODUCER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    Move => Effect::PURE; layout Scalar; meaning Unused; js Move; class Move; staged Flow; traits LEAF_VALUE INLINABLE PURE_SETUP ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE REGISTER_COPY REMOVABLE_WHEN_DEAD REGISTER_ROW, @ Register, @ fields(ResultRegister, Register, Unused),
    Call => CALL_EFFECT; layout CallWindowWithEvalFlags; flow Call; class Call; staged Call; traits UNIT_REWRITTEN INLINABLE FRAME_RETURNABLE CALL_LINK PLAIN_CALL CALL_INVOKE ORDINARY_CALL MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallArray => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call; class CallArray; staged Names; traits CALL_LINK ORDINARY_CALL MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallSpread => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call; class CallSpread; staged Names; traits CALL_LINK SPREAD_CALL MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, Register),
    CallKnown => CALL_EFFECT; layout CallWindow; flow Call; class CallKnown; staged Call; traits CALL_LINK MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, FunctionIndex, Unused),
    CallMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call; class CallMethod; staged Call; traits CALL_LINK MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, Unused),
    CallThisMethod => CALL_EFFECT; layout Scalar; meaning MethodSiteIndex; flow Call; staged Call; traits CALL_LINK MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Unused, Unused),
    Construct => CALL_EFFECT; layout ConstructCountAndFlags; flow Call; class Construct; staged Call; traits UNIT_REWRITTEN FRAME_RETURNABLE CALL_LINK CONSTRUCTION MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, ConstructArguments),
    Jump => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow Jump; js Jump; class Jump; staged Flow; traits LEAF_JUMP INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE UNCONDITIONAL_JUMP REGISTER_ROW, @ Register, @ fields(Unused, Unused, Unused),
    JumpFalse => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class JumpFalse; staged Flow; traits LEAF_JUMP INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE JUMPS_ON_FALSE REGISTER_ROW, @ Register, @ fields(Register, Unused, Unused),
    JumpBinaryFalse => READ_THROW.union(Effect::CONTROL); layout Scalar; meaning JumpTarget; flow ConditionalJump; class JumpBinaryFalse; staged Generic; step Body; value CompareJump; traits LEAF_BINARY INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE FUSED_BINARY_JUMP ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(BinaryOperator, Operand, Operand),
    Return => Effect::CONTROL; layout Scalar; meaning Unused; flow Terminal; js Return; class Return; staged Flow; traits UNIT_REWRITTEN LEAF_VALUE INLINABLE ACTIVATION_FREE SCALAR_REGION RETURN_VALUE RETURN_FAMILY ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    Throw => Effect::THROWS.union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal; class Throw; staged Flow; step Body; traits UNIT_REWRITTEN RAISES ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    NumericAdd => READ_THROW; layout Scalar; meaning AdditionOperator; js NumericBinary; class NumericAdd; staged Generic; traits ARITHMETIC_BINARY LEAF_BINARY INLINABLE PURE_SETUP ACTIVATION_FREE LIFETIME_SAFE DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    NumericMultiply => READ_THROW; layout Scalar; meaning MultiplicationOperator; js NumericBinary; class NumericMultiply; staged Generic; traits ARITHMETIC_BINARY LEAF_BINARY INLINABLE PURE_SETUP ACTIVATION_FREE LIFETIME_SAFE DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ NumericReturnable, @ fields(ResultRegister, Operand, Operand),
    InitializeThis => CALL_EFFECT.union(Effect::CONTROL); layout Scalar; meaning Unused; staged Names; step Body; traits BINDS_THIS NAME_OPERATION BINDING_FORM MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    CacheTemplateObject => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning TemplateSiteIndex; staged Names; step Body; traits TEMPLATE_CACHE_STORE ROW_HANDLER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    LoadCachedTemplateObject => Effect::READS_HEAP; layout Scalar; meaning TemplateSiteIndex; staged Names; step Body; traits TEMPLATE_CACHE_LOAD DISTANT_PRODUCER ROW_HANDLER ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    DefineField => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldDefinition; class DefineField; staged Names; step Lane; traits FIELD_DEFINE INSTANCE_ELEMENT_DEFINE MAY_ALLOCATE, @ Register, @ fields(Register, Register, CacheSiteIndex),
    ValidateClassHeritage => READ_THROW; layout Scalar; meaning Unused; staged Names; step Body; traits CLASS_HERITAGE_CHECK MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    SetFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex; js FieldStore; staged Property; step Body; traits STRICT_STORE NAMED_FIELD_WRITE ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Register, CacheSiteIndex),
    SetThisFieldStrict => WRITE_THROW; layout Scalar; meaning AtomIndex; staged Property; step Body; traits STRICT_STORE THIS_FIELD_STORE ROW_ONLY MAY_ALLOCATE, @ Register, @ fields(Register, Unused, CacheSiteIndex),
    // The general loop's fast arms dispatch on these with the opcodes above, so they share
    // one dense range of opcode numbers.
    LoadLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot; js PlainLocalLoad; class LoadLocalPlain; staged Local; traits REGISTER_ROW ROW_HANDLER ACTIVATION_FREE FRAME_LOCAL_LOAD PLAIN_LOCAL_LOAD, @ Register, @ fields(ResultRegister, NumericLocalTarget, NumericLocalStoreMarker),
    StoreLocalPlain => Effect::PURE; layout Scalar; meaning LocalSlot; js PlainLocalStore; class StoreLocalPlain; staged Local; traits REGISTER_ROW ROW_HANDLER ACTIVATION_FREE FRAME_LOCAL_STORE PLAIN_LOCAL_STORE, @ Register, @ fields(Register, OptionalRegister, BooleanFlag),
    JumpTrue => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class JumpTrue; staged Flow; traits REGISTER_ROW LEAF_JUMP INLINABLE ACTIVATION_FREE SCALAR_REGION FRAME_ONLY LIFETIME_SAFE JUMPS_ON_TRUE, @ Register, @ fields(Register, Unused, Unused),
    // `Call` whose last window argument the instruction before it loaded from the activation's
    // `arguments` binding (`CallPlan::forwards_arguments`).
    CallForwardingArguments => CALL_EFFECT; layout CallWindowWithEvalFlags; flow Call; class CallForwardingArguments; staged Call; traits FRAME_RETURNABLE CALL_LINK MAY_ALLOCATE, @ Returnable, @ fields(ResultRegister, Register, Register),
    WasmI32Add => Effect::PURE; layout Scalar; meaning Unused; class WasmI32AddRegister; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Subtract => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Multiply => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32DivideSigned => Effect::THROWS; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32DivideUnsigned => Effect::THROWS; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RemainderSigned => Effect::THROWS; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RemainderUnsigned => Effect::THROWS; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32And => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Or => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Xor => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32ShiftLeft => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32ShiftRightSigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32ShiftRightUnsigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RotateLeft => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32RotateRight => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Equal => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32NotEqual => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessSigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessUnsigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterSigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterUnsigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessEqualSigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32LessEqualUnsigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterEqualSigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32GreaterEqualUnsigned => Effect::PURE; layout Scalar; meaning Unused; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32AddImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; class WasmI32AddImmediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32SubtractImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; class WasmI32SubtractImmediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32MultiplyImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32DivideSignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32DivideUnsignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RemainderSignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RemainderUnsignedImmediate => Effect::THROWS; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32AndImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32OrImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32XorImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32ShiftLeftImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32ShiftRightSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32ShiftRightUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RotateLeftImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32RotateRightImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32EqualImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32NotEqualImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessEqualSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32LessEqualUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterEqualSignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI32GreaterEqualUnsignedImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    // `(b >>> c) & imm`: a bit-field extraction fused from its two operators.
    WasmI32ShiftRightUnsignedAndImmediate => Effect::PURE; layout Scalar; meaning WasmI32Immediate; class WasmI32ShiftRightUnsignedAnd; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    // `a = b + c`, then jump when `a` is nonzero: a counted loop's step and test.
    WasmI32AddImmediateJumpNonZero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmI32AddJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    WasmInstanceBinding => Effect::READS_HEAP; layout Scalar; meaning WasmInstanceSlot; class WasmInstanceBinding; staged WasmState; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmFillRegisters => Effect::PURE; layout Scalar; meaning ConstantIndex; class WasmFillRegisters; staged WasmState; traits WASM_OPCODE ROW_HANDLER REGISTER_FILL, @ Register, @ fields(Unused, RegisterWindowBase, RegisterCount),
    WasmSelect => Effect::PURE; layout RegisterPair; class WasmSelect; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER WASM_SELECT, @ Register, @ fields(ResultRegister, Register, Register),
    WasmJumpI32Zero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32Zero; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Unused, Unused),
    WasmJumpI32NonZero => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32NonZero; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Unused, Unused),
    WasmBranchTable => Effect::CONTROL; layout Scalar; meaning BranchTableCases; flow BranchTable; class WasmBranchTable; staged WasmJump; traits ROW_ONLY WASM_OPCODE ROW_HANDLER, @ Register, @ fields(Register, Unused, Unused),
    WasmJumpI32Equal => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32NotEqual => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessEqualSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32LessEqualUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterEqualSigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32GreaterEqualUnsigned => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, Register, Unused),
    WasmJumpI32EqualImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32EqualImmediate; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32NotEqualImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; class WasmJumpI32NotEqualImmediate; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessEqualSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32LessEqualUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterEqualSignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmJumpI32GreaterEqualUnsignedImmediate => Effect::CONTROL; layout Scalar; meaning JumpTarget; flow ConditionalJump; staged WasmJump; traits WASM_OPCODE, @ Register, @ fields(Register, SignedConstant, Unused),
    WasmI32Unary => Effect::PURE; layout Scalar; meaning WasmI32UnaryOperator; class WasmI32Unary; staged WasmInt; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI64Binary => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmI64BinaryOperator; class WasmI64Binary; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE I64_BINARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64BinaryImmediate => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmI64BinaryOperator; class WasmI64BinaryImmediate; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE I64_BINARY_IMMEDIATE_FAMILY, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    WasmI64Unary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmI64UnaryOperator; class WasmI64Unary; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE I64_UNARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI64BinaryRaw => Effect::THROWS; layout Scalar; meaning WasmI64BinaryOperator; class WasmI64BinaryRaw; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE RAW_FORM I64_BINARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64BinaryImmediateRaw => Effect::THROWS; layout Scalar; meaning WasmI64BinaryOperator; class WasmI64BinaryImmediateRaw; staged WasmInt; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE RAW_FORM I64_BINARY_IMMEDIATE_FAMILY, @ Register, @ fields(ResultRegister, Register, SignedConstant),
    WasmI64UnaryRaw => Effect::PURE; layout Scalar; meaning WasmI64UnaryOperator; class WasmI64UnaryRaw; staged WasmInt; traits WASM_OPCODE ROW_HANDLER RAW_FORM I64_UNARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmF64BinaryRaw => Effect::PURE; layout Scalar; meaning WasmF64BinaryOperator; class WasmFloatBinary64Raw; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER RAW_FORM F64_BINARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF64UnaryRaw => Effect::PURE; layout Scalar; meaning WasmF64UnaryOperator; class WasmFloatUnary64Raw; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER RAW_FORM F64_UNARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmReturn64 => Effect::CONTROL; layout Scalar; meaning Unused; flow Terminal; class WasmReturn64; staged Flow; traits WASM_OPCODE RAW_FORM RETURN_FAMILY, @ Register, @ fields(Register, Unused, Unused),
    WasmScalarConvert => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmScalarConversionOperator; class WasmScalarConvert; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmF32Binary => Effect::PURE; layout Scalar; meaning WasmF32BinaryOperator; class WasmFloatBinary32; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF32Unary => Effect::PURE; layout Scalar; meaning WasmF32UnaryOperator; class WasmFloatUnary32; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmF64Binary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmF64BinaryOperator; class WasmFloatBinary64; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE F64_BINARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF64Unary => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmF64UnaryOperator; class WasmFloatUnary64; staged WasmFloat; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE F64_UNARY_FAMILY, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableInit => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; class WasmTableInit; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmIndirectTarget => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmSignatureIndex; class WasmIndirectTarget; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmGlobalGet => READ_THROW; layout Scalar; meaning Unused; class WasmGlobalGet; staged WasmState; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmGlobalSet => WRITE_THROW; layout Scalar; meaning Unused; class WasmGlobalSet; staged WasmState; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Unused),
    WasmRefFunc => CALL_EFFECT; layout Scalar; meaning WasmFunctionIndex; class WasmRefFunc; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmRefIsNull => Effect::PURE; layout Scalar; meaning Unused; staged Names; traits WASM_OPCODE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableSize => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; class WasmTableSize; staged Names; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmTableGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused; class WasmTableGet; staged Names; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmTableSet => Effect::WRITES_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused; class WasmTableSet; staged Names; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmTableGrow => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout RegisterPair; class WasmTableGrow; staged Names; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmTableFill => Effect::WRITES_HEAP.union(Effect::THROWS); layout RegisterPair; class WasmTableFill; staged Names; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmTableCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; class WasmTableCopy; staged Names; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmMemoryInit => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; class WasmMemoryInit; staged Names; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmMemoryCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; class WasmMemoryCopy; traits WASM_OPCODE WASM_MEMORY UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmMemoryFill => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; class WasmMemoryFill; traits WASM_OPCODE WASM_MEMORY UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmMemoryAddress => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning ConstantIndex; class WasmMemoryAddress; staged WasmMemory; traits WASM_OPCODE WASM_MEMORY ROW_HANDLER MAY_ALLOCATE WASM_MEMORY_OFFSET_CONSTANT, @ Register, @ fields(ResultRegister, Register, Register),
    WasmMemoryLoad => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryLoadOperator; class WasmMemoryLoad; staged WasmMemory; traits WASM_OPCODE WASM_MEMORY ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmMemoryStore => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryStoreOperator; class WasmMemoryStore; staged WasmMemory; traits WASM_OPCODE WASM_MEMORY ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmI32Load => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF32Load => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmF64Load => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load8S => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load8S; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load8U => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load8U; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load16S => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load16S; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Load16U => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; class WasmI32Load16U; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load8S => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load8U => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load16S => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load16U => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load32S => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI64Load32U => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmI32Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmI64Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmF32Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmF64Store => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmI32Store8 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmI32Store16 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmI64Store8 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmI64Store16 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmI64Store32 => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmMemoryOffset; staged WasmMemory; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmMemorySize => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; class WasmMemorySize; traits WASM_OPCODE WASM_MEMORY MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmMemoryGrow => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; class WasmMemoryGrow; traits WASM_OPCODE UNROLL_EXCLUDED MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmUnreachable => Effect::THROWS.union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal; class WasmUnreachable; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Unused, Unused, Unused),
    WasmSimd => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning WasmSimdOperator; class WasmSimd; staged WasmVector; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmSimdShuffle => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning ConstantIndex; class WasmSimdShuffle; staged WasmVector; traits WASM_OPCODE ROW_HANDLER MAY_ALLOCATE WASM_SHUFFLE_CONSTANT, @ Register, @ fields(ResultRegister, Register, Register),
    DefinePropertyRecord => CALL_EFFECT; layout Scalar; meaning PropertyDefinitionMode; class DefinePropertyRecord; staged Names; traits MAY_ALLOCATE PROPERTY_RECORD_DEFINE, @ Register, @ fields(Register, Register, Register),
    WasmRefAsNonNull => Effect::THROWS; layout Scalar; meaning WasmNonNullCheck; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmI31 => Effect::THROWS; layout Scalar; meaning WasmI31Operator; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefTest => Effect::READS_HEAP; layout Scalar; meaning WasmReferenceTarget; class WasmRefTest; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefCast => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget; class WasmRefCast; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmRefEq => Effect::PURE; layout Scalar; meaning Unused; traits WASM_OPCODE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmStructNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmStructNewDefault => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    WasmStructGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; traits WASM_OPCODE MAY_ALLOCATE STRUCT_GET_FAMILY, @ Register, @ fields(ResultRegister, Register, StaticOffset),
    WasmStructGetS => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, StaticOffset),
    WasmStructGetU => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, StaticOffset),
    WasmStructSet => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcField; traits WASM_OPCODE MAY_ALLOCATE STRUCT_SET_FAMILY, @ Register, @ fields(Register, Register, Unused),
    WasmStructGetRaw => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; traits WASM_OPCODE MAY_ALLOCATE RAW_FORM STRUCT_GET_FAMILY, @ Register, @ fields(ResultRegister, Register, StaticOffset),
    WasmStructSetRaw => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcField; traits WASM_OPCODE MAY_ALLOCATE RAW_FORM STRUCT_SET_FAMILY, @ Register, @ fields(Register, Register, Unused),

    WasmArrayNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNew; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayNewDefault => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewDefault; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmArrayNewFixed => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewFixed; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayLen => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),

    WasmArrayGet => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; staged Names; traits WASM_OPCODE MAY_ALLOCATE ARRAY_GET_FAMILY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayGetS => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArrayGetU => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArraySet => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcField; staged Names; traits WASM_OPCODE MAY_ALLOCATE ARRAY_SET_FAMILY, @ Register, @ fields(Register, Register, Register),
    WasmArrayGetRaw => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcField; staged Names; traits WASM_OPCODE MAY_ALLOCATE RAW_FORM ARRAY_GET_FAMILY, @ Register, @ fields(ResultRegister, Register, Register),
    WasmArraySetRaw => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcField; staged Names; traits WASM_OPCODE MAY_ALLOCATE RAW_FORM ARRAY_SET_FAMILY, @ Register, @ fields(Register, Register, Register),
    WasmExternalConversion => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmExternalConversion; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmArrayFill => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmArrayCopy => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmArrayNewData => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewData; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayNewElem => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmArrayNewElem; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmArrayInitData => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),
    WasmArrayInitElem => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout RegisterPair; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Register, Register, Register),

    WasmExceptionNew => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning Unused; staged Names; traits WASM_OPCODE MAY_ALLOCATE WASM_EXCEPTION_WINDOW, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmExceptionMatch => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning Unused; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmExceptionPayload => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmExceptionFieldIndex; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmThrowRef => Effect::READS_HEAP.union(Effect::THROWS).union(Effect::CONTROL); layout Scalar; meaning Unused; flow Terminal; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    WasmStructNewDesc => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmStructNewDefaultDesc => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmRefGetDesc => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmGcTypeIndex; class WasmRefGetDesc; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    WasmDescriptorTest => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget; class WasmDescriptorTest; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    WasmDescriptorCast => Effect::READS_HEAP.union(Effect::THROWS); layout Scalar; meaning WasmReferenceTarget; class WasmDescriptorCast; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),

    WasmAtomicAccess => Effect::READS_HEAP.union(Effect::WRITES_HEAP).union(Effect::THROWS); layout Scalar; meaning WasmAtomicOperator; staged Names; traits WASM_OPCODE MAY_ALLOCATE WASM_ATOMIC_WINDOW, @ Register, @ fields(ResultRegister, RegisterWindowBase, RegisterCount),
    WasmAtomicFence => Effect::READS_HEAP.union(Effect::WRITES_HEAP); layout Scalar; meaning Unused; staged Names; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Unused, Unused, Unused),
    SetFunctionDebugName => Effect::WRITES_HEAP; layout Scalar; meaning AtomIndex; staged Names; step Lane; traits MAY_ALLOCATE, @ Register, @ fields(Register, Unused, Unused),
    // `arguments.length` and `arguments[key]` of the `arguments` binding in the local slot:
    // an unmaterialized binding answers from the values its activation kept
    // (`Function::kept_arguments`); any other value is read as `GetField` and
    // `GetIndex` read it.
    ArgumentsLength => READ_THROW; layout Scalar; meaning LocalSlot; class ArgumentsLength; staged Names; traits ARGUMENTS_READ FRAME_SLOT_READ DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Unused, Unused),
    ArgumentGet => READ_THROW; layout Scalar; meaning LocalSlot; class ArgumentGet; staged Names; traits ARGUMENTS_READ FRAME_SLOT_READ DISTANT_PRODUCER ROW_HANDLER MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Unused),
    // `rest.length` and `rest[key]` of a rest parameter read nowhere else (`Function::rest_view`):
    // the values its activation kept past its first `imm` parameters, with no array made.
    // A key other than an index those values hold makes the rest array and reads it.
    // The rest register (`b` of `RestLength`, `c` of `RestGet`) holds `undefined` where the
    // entry kept the values, else the array an entry made, which they then read.
    RestLength => Effect::READS_HEAP; layout Scalar; meaning ArrayLength; class RestLength; staged Names; step Lane; traits ROW_HANDLER MAY_ALLOCATE REST_LENGTH_READ, @ Register, @ fields(ResultRegister, Register, Unused),
    RestGet => READ_THROW; layout Scalar; meaning ArrayLength; class RestGet; staged Names; step Lane; traits BOXED_RESULT ROW_HANDLER MAY_ALLOCATE REST_INDEX_READ, @ Register, @ fields(ResultRegister, Register, Register),
    // `CallSpread` of a rest parameter `compile::rest_reads` keeps as a view: its window holds
    // the rest register, `undefined` where the entry kept the values passed, which it spreads.
    CallRest => CALL_EFFECT; layout SingleArgumentCallWindowWithEvalFlags; flow Call; class CallRest; staged Names; traits CALL_LINK MAY_ALLOCATE REST_SPREAD_CALL, @ Returnable, @ fields(ResultRegister, Register, Register),
    // A first-call stub's body: calls its chunk with its arguments and `new.target` (`Vm::call_first_chunk`).
    CallFirstChunk => CALL_EFFECT; layout Scalar; meaning Unused; staged Call; traits FIRST_CALL_CHUNK MAY_ALLOCATE, @ Register, @ fields(ResultRegister, Register, Register),
    // `a` = whether `b`, a base class constructor's fresh receiver, already holds the run of
    // leading constant fields (`ConstantField`) whose defines follow the `JumpTrue` of `a` that
    // skips them: its exact shape is the one the run's first `DefineField` (cache site `c`) found
    // already holding its name, as only a receiver made in the run's layout does
    // (`vm::preshaped_receiver`, `Vm::constant_fields_held`). It reads one cache entry and a
    // shape, and raises nothing; not taking the skip is always exact too, as the defines then
    // write each value over itself (`compile::class::emit_instance_fields`).
    ConstantFieldsHeld => Effect::READS_HEAP; layout Scalar; meaning Unused; staged Names; step LaneResult; traits ROW_HANDLER, @ Register, @ fields(ResultRegister, Register, CacheSiteIndex),
    WasmSegmentDrop => Effect::WRITES_HEAP; layout Scalar; meaning WasmInstanceSlot; class WasmSegmentDrop; traits WASM_OPCODE MAY_ALLOCATE, @ Register, @ fields(Unused, Unused, Unused),
    // Synthetic records only: the lane view's prologue records, which no bytecode holds. A frameless activation
    // of a function the JavaScript tier may claim starts at `TierPrologue` (`imm` the function, `a` its parameter
    // count); a specialized body's prologue is `BodyEntry` (`imm` the function, `b` whether it binds `this`).
    // Their staged rows are `Operation::Prologue` and `Operation::BodyEntry` (`staged::prologue`).
    TierPrologue => CALL_EFFECT; layout Scalar; meaning Unused; class TierPrologue; staged Lane; traits MAY_ALLOCATE, @ Register, @ fields(Unused, Unused, Unused),
    BodyEntry => CALL_EFFECT; layout Scalar; meaning Unused; class BodyEntry; staged Lane; traits MAY_ALLOCATE, @ Register, @ fields(Unused, Unused, Unused),
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
        if self.entries.get() == 0 && back_edges_of(code.iter().map(|&packed| widen(packed, wide))).next().is_some() {
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

/// The back edges of a sequence of fixed-width instructions, in source order: each jump whose
/// target is its own or an earlier instruction, as (source pc, target pc). The one definition
/// of a loop's back edge; handler edges are not jumps and are not listed here.
pub(crate) fn back_edges_of(instructions: impl Iterator<Item = WideInstruction>) -> impl Iterator<Item = (usize, usize)> {
    instructions.enumerate().filter_map(|(pc, instruction)| {
        instruction.op().control_flow_layout().jumps()
            .then(|| instruction.jump_target() as usize)
            .filter(|&target| target <= pc)
            .map(|target| (pc, target))
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
    /// The realm index each lexical's binding holds, parallel to `lexical`. The declaration
    /// index a `LoadGlobalLexical` or `StoreGlobalLexical` op carries names it. Mapped at each
    /// instantiation (`Vm::map_global_lexical_slots`) and refreshed on a by-name fallback; a
    /// stale index is caught by the atom check of the read and costs only the by-name path.
    pub(crate) slots: Box<[std::cell::Cell<u16>]>,
}

impl GlobalDeclarations {
    /// The declarations, with no lexical mapped to a realm binding yet.
    pub fn new(
        lexical: Vec<Atom>,
        var: Vec<Atom>,
        function: Vec<Atom>,
        annex_b_var: Vec<Atom>,
        immutable: Vec<Atom>,
    ) -> Self {
        let slots: Box<[std::cell::Cell<u16>]> = lexical.iter().map(|_| std::cell::Cell::new(NO_REALM_BINDING)).collect();
        Self { lexical, var, function, annex_b_var, immutable, slots }
    }

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
    /// A base class constructor's leading constant fields (`ConstantField`): the public fields
    /// its first instructions define, in order, each with the primitive its initializer is;
    /// empty for every other function.
    pub(crate) constant_fields: crate::heap::ThinBoxSlice<ConstantField>,
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
    /// `function f(...rest)` compiled with no parameters, `rest` naming its `arguments` binding
    /// (`compile::rest_alias`): the binding materializes as the rest array, not an arguments object.
    pub rest_alias: bool,
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
    /// The local slots of the function declarations this nested function's body declares at its top level, which
    /// no store writes (`capture_layout::inputs`): a closure that captures one holds its identity fixed.
    pub(crate) declared_functions: crate::heap::ThinBoxSlice<u16>,
    /// The depth-0 capture slots of this function whose value is a never-written declaration of its parent
    /// (`declared_functions` of the parent): a body that calls one by its identity checks it once at entry.
    pub(crate) declared_captures: crate::heap::ThinBoxSlice<u16>,
    /// Binding slots assigned stable frame registers by lowering.
    pub(crate) local_registers: crate::heap::ThinBoxSlice<LocalRegister>,
    /// The function was created while an enclosing `with` scope was active,
    /// so unresolved names in descendants can still observe its outer locals.
    pub(crate) inherited_with_scope: bool,
    pub(crate) lexical_atoms: crate::heap::ThinBoxSlice<Atom>,
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
    /// The `plain_local_slots` words of the final code: stored by `compile::lower_settled` from its
    /// proof, else derived on the first read (`plain_local`). The binary codec writes them for a
    /// lowered record. Read on every interpreter local operand, so it stays inline rather than
    /// behind `caches`.
    pub(crate) plain_locals: std::cell::OnceCell<crate::heap::ThinBoxSlice<u64>>,
    /// The lazily derived booleans (`LazyFlag`), each a known bit and a value bit; never serialized.
    pub(crate) lazy_flags: std::cell::Cell<u32>,
    /// The caches a call or a collection fills, boxed by the first fill (`caches`); never serialized.
    pub(crate) caches: std::cell::OnceCell<Box<FunctionCaches>>,
    /// Lazily derived `call_plan`; never serialized.
    /// `CallPlan::to_bits`, packed: the plan's flags are one word, not a byte each.
    pub(crate) call_plan: std::cell::OnceCell<u16>,
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
    /// The parts few functions set (`FunctionRare`), behind one pointer: `None` for most.
    pub(crate) rare: Option<Box<FunctionRare>>,
}

/// The parts of a function record that few functions set, boxed together so each other
/// function pays one pointer for all of them (`Function::rare`).
#[derive(Clone, Debug, Default)]
pub(crate) struct FunctionRare {
    /// The language types (`typed_ir::Type::language` bits) each parameter's TypeScript
    /// annotation declares, by argument index; empty where none declares one. A seed, not a
    /// proof: a tier that relies on one checks it once where the argument enters
    /// (`compile::typescript::declared_parameters`).
    pub(crate) declared_parameters: crate::heap::ThinBoxSlice<u16>,
    /// What a Script or eval body declares in the global scope; `None` for every other
    /// function, which declares nothing there.
    pub(crate) global_declarations: Option<Box<GlobalDeclarations>>,
    /// Instance-element initialization run when this derived constructor binds `this`.
    pub(crate) instance_initializer: Option<u32>,
}

impl FunctionRare {
    /// The rare parts, boxed only when one is set.
    pub(crate) fn boxed(self) -> Option<Box<Self>> {
        let empty = self.declared_parameters.is_empty()
            && self.global_declarations.is_none()
            && self.instance_initializer.is_none();
        (!empty).then(|| Box::new(self))
    }
}

// Every compiled function keeps one record for the program's life (a fastify server holds
// about 4,400), so each word here is a per-function cost of an idle process: lists fixed
// once compiled sit behind thin slices. `code`, `wide` and `source_positions` stay as the
// compiler builds them: its passes rewrite them, and its rewrite loop measured slower when
// their layout changed.
// `declared_parameters` (8 bytes) carries TypeScript parameter types into the fact store.
// `constant_fields` (8 bytes) carries a base class's constant fields to its receiver's allocation.
const _: () = assert!(size_of::<Function>() <= 384);

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

    /// `template`, a stub record its program's stubs of one kind share, as this header's own:
    /// those stubs differ in nothing else (`compile::first_call::template`).
    fn own(self, mut template: Function, name_bindings: NameBindings) -> Function {
        template.name_bindings = name_bindings;
        template.parent = self.parent;
        template.name = self.name;
        template.inferred_name = self.inferred_name;
        template.source_span = self.source_span;
        template.length = self.length;
        template.is_arrow = self.is_arrow;
        template.constructible = self.constructible;
        template
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
const _: () = assert!(size_of::<FunctionSlot>() == 56);

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
    unlowered: UnlowerCell,
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
    /// A first-call stub's: the record its program's stubs of one kind share, which the
    /// slot's header and the stub's own name bindings, which share its scopes, make its own
    /// (`FunctionHeader::own`).
    Template(Box<(std::rc::Rc<Function>, NameBindings)>),
}

impl Unlowered {
    fn function(&self, header: FunctionHeader) -> Function {
        match self {
            Self::Record(function) => Function::clone(function),
            Self::Packed(packed) => binary::unpack_unlowered(packed, &header),
            Self::Template(template) => header.own(Function::clone(&template.0), template.1.clone()),
        }
    }

    fn into_function(self, header: FunctionHeader) -> Function {
        match self {
            Self::Record(function) => std::rc::Rc::unwrap_or_clone(function),
            Self::Packed(packed) => binary::unpack_unlowered(&packed, &header),
            Self::Template(template) => header.own(Function::clone(&template.0), template.1),
        }
    }
}

/// A slot's `Unlowered`, held in a `Cell` so that a slot shared by its program is changed through
/// `&self` without a borrow flag: `with` takes the record out for the read and puts it back.
struct UnlowerCell(std::cell::Cell<Option<Unlowered>>);

impl UnlowerCell {
    fn new(unlowered: Option<Unlowered>) -> Self {
        Self(std::cell::Cell::new(unlowered))
    }

    /// `read` over the record, which stays in the slot.
    fn with<R>(&self, read: impl FnOnce(Option<&Unlowered>) -> R) -> R {
        let unlowered = self.0.take();
        let result = read(unlowered.as_ref());
        self.0.set(unlowered);
        result
    }

    fn set(&self, unlowered: Option<Unlowered>) {
        self.0.set(unlowered);
    }

    fn take(&self) -> Option<Unlowered> {
        self.0.take()
    }

    fn is_none(&self) -> bool {
        self.with(|unlowered| unlowered.is_none())
    }

    fn get_mut(&mut self) -> &mut Option<Unlowered> {
        self.0.get_mut()
    }
}

impl Clone for UnlowerCell {
    fn clone(&self) -> Self {
        Self::new(self.with(|unlowered| unlowered.cloned()))
    }
}

impl std::fmt::Debug for UnlowerCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|unlowered| std::fmt::Debug::fmt(&unlowered, f))
    }
}

impl FunctionSlot {
    fn new(header: FunctionHeader, record: Option<Function>, unlowered: Option<Unlowered>) -> Self {
        let record = record.map_or_else(Default::default, |function| std::rc::Rc::new(function).into());
        Self { header: PackedHeader::pack(&header), record, unlowered: UnlowerCell::new(unlowered), tier0: Default::default() }
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

    /// A first-call stub's slot: `function`'s header over `template`, the record of its kind,
    /// unlowered and shared with its program's other stubs of that kind.
    pub(crate) fn unlowered_template(function: &Function, template: std::rc::Rc<Function>) -> Self {
        debug_assert!(
            binary::packs_alike(&FunctionHeader::of(function).own(Function::clone(&template), function.name_bindings.clone()), function),
            "a stub's record is its template's under its own header",
        );
        Self::new(FunctionHeader::of(function), None, Some(Unlowered::Template(Box::new((template, function.name_bindings.clone())))))
    }

    /// Whether the slot holds no record, compiled or unlowered: its first read compiles
    /// the record again or decodes it.
    #[cfg(test)]
    pub(crate) fn is_released(&self) -> bool {
        self.record.get().is_none() && self.unlowered.with(|held| held.is_none()) && self.tier0.get().is_none()
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
        self.unlowered.take();
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

/// One of a base class constructor's leading constant fields (`Function::constant_fields`,
/// `compile::class::emit_instance_fields`): a public field with a plain string key, neither
/// `__proto__` nor numeric, whose initializer is absent or a primitive constant. The leading
/// run of them is defined first thing by the constructor, before any code can observe the
/// receiver, so its receiver may be allocated already holding them (`vm::preshaped_receiver`);
/// the `DefineField`s then find each in place, and the compiler skips the run where the receiver
/// holds it (`Op::ConstantFieldsHeld`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ConstantField {
    pub(crate) atom: Atom,
    pub(crate) value: FieldConstant,
}

/// The most leading constant fields a receiver is allocated holding (`vm::preshaped_receiver`):
/// a longer run gives its first ones, and the compiler's skip over the run covers as many
/// (`compile::class::emit_instance_fields`).
pub(crate) const PRESHAPED_FIELDS: usize = 16;

/// The primitive a constant field holds; a string is named by its atom, whose string the VM
/// keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FieldConstant {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(Atom),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalRegister {
    pub(crate) local: u16,
    pub(crate) register: Register,
}

/// A source position table entry: the instruction at `pc` came from byte `offset` of the
/// compiled source text (`ProgramSource::position` maps it to a line and column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceOffset {
    pub(crate) pc: u32,
    pub(crate) offset: u32,
}

/// A 1-based line and 1-based UTF-16 column of a source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourcePosition {
    pub(crate) line: u32,
    pub(crate) column: u32,
}

/// The name of a parameter slot whose argument a list's patterns or defaults bind apart
/// (`FunctionCompiler::parameter_name`): an argument's own slot, which no source name reaches.
pub(crate) const HIDDEN_PARAMETER_PREFIX: &str = "\0quench:param:";

/// Whether register liveness covers a function's register forms, so a frame may leave its
/// dead registers stale; the bound on the registers live at entry (`entry_fill`), derived on
/// first activation; and the per-pc live register masks the collector scans, derived when a
/// collection first finds the function on the stack. Most of a program's functions never run
/// during a collection, so their masks are never built. The loop tier's regions share them
/// (`osr_loop::LoopRegion::live`) rather than copying a function's masks for each loop.
/// Nothing here is serialized: a decoded program derives each on its first use.
#[derive(Clone, Debug)]
pub(crate) struct RegisterRoots {
    covered: bool,
    /// The registers below which every register live at entry lies (`entry_fill`), or
    /// `ALL_LIVE_AT_ENTRY` where liveness proves no bound. Filled where the lowering had the live
    /// sets, else on the first `ResidualProgram::entry_fill`.
    entry_live: std::cell::OnceCell<u16>,
    /// The masks the derivation of `entry_live` solved, kept for the first mask ask
    /// (`ResidualProgram::shared_register_root_masks`) so that ask does not solve the function
    /// again. They are the masks `RegisterMasks::of` would derive, since they come from the same
    /// solved sets at the same width. Kept only where a collection would keep them
    /// (`RegisterMasks::too_large_to_keep` is false), so a large function's table is not held
    /// before anything asks for it. Taken by that ask, so they are held once at most.
    solved: std::cell::RefCell<Option<crate::compile::liveness::RegisterMasks>>,
}

/// Uncovered, which bounds no register live at entry.
impl Default for RegisterRoots {
    fn default() -> Self {
        Self::new(false)
    }
}

impl RegisterRoots {
    /// A function of uncovered registers bounds none live at entry; a covered one's bound is
    /// derived on its first ask.
    pub(crate) fn new(covered: bool) -> Self {
        let entry_live = if covered { std::cell::OnceCell::new() } else { std::cell::OnceCell::from(Self::ALL_LIVE_AT_ENTRY) };
        Self { covered, entry_live, solved: std::cell::RefCell::new(None) }
    }

    /// Covered, every register live at entry below `entry_live`, with the masks `solved` that
    /// derived it kept for the first mask ask (see `solved`).
    pub(crate) fn with_solved(entry_live: u16, solved: Option<crate::compile::liveness::RegisterMasks>) -> Self {
        Self { covered: true, entry_live: std::cell::OnceCell::from(entry_live), solved: std::cell::RefCell::new(solved) }
    }

    /// Keeps `masks`, which a derivation of `entry_live` solved, for the first mask ask.
    pub(crate) fn keep_solved(&self, masks: crate::compile::liveness::RegisterMasks) {
        *self.solved.borrow_mut() = Some(masks);
    }

    /// The masks kept by `keep_solved` or `with_solved`, taken: a later call finds none.
    pub(crate) fn take_solved(&self) -> Option<crate::compile::liveness::RegisterMasks> {
        self.solved.take()
    }

    /// An `entry_live` that bounds nothing.
    pub(crate) const ALL_LIVE_AT_ENTRY: u16 = u16::MAX;

    /// `entry_live`, derived by `derive` on its first ask.
    pub(crate) fn entry_live_with(&self, derive: impl FnOnce() -> u16) -> u16 {
        *self.entry_live.get_or_init(derive)
    }

    pub(crate) fn covered(&self) -> bool {
        self.covered
    }

    /// `entry_live` where it is derived already (by the lowering or a first ask), else `None`.
    pub(crate) fn entry_live_known(&self) -> Option<u16> {
        self.entry_live.get().copied()
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

/// The fixed-width form of `packed`, a wide one read through its function's `wide` table: the one
/// decoder of the packed encoding, for readers that hold the table apart from its function.
#[inline(always)]
/// Whether `function` can reach bindings by name at run time, given the instructions `scanned`
/// of its code: the one predicate behind `Function::has_dynamic_scope_access`, which the
/// validator's scan over unvalidated code shares.
pub(crate) fn dynamic_scope_access_in(
    function: &Function,
    mut scanned: impl Iterator<Item = WideInstruction>,
) -> bool {
    function.inherited_with_scope
        || !function.binding_sites.is_empty()
        || scanned.any(WideInstruction::resolves_dynamic_scope)
}

pub(crate) fn widen(packed: Instr, wide: &[WideInstruction]) -> WideInstruction {
    if packed.is_wide() { wide[packed.wide_index()] } else { packed.as_wide() }
}

impl Function {
    /// Releases the spare capacity of the tables a compilation grew, once it is finished.
    pub(crate) fn shrink_to_fit(&mut self) {
        crate::compile::trim_slack(&mut self.code);
        crate::compile::trim_slack(&mut self.wide);
    }

    /// The TypeScript parameter types, by argument index (`FunctionRare::declared_parameters`).
    pub(crate) fn declared_parameters(&self) -> &[u16] {
        match &self.rare {
            Some(rare) => &rare.declared_parameters[..],
            None => &[],
        }
    }

    /// The class constructor's instance initializer (`FunctionRare::instance_initializer`).
    pub(crate) fn instance_initializer(&self) -> Option<u32> {
        self.rare.as_deref().and_then(|rare| rare.instance_initializer)
    }

    /// Sets the TypeScript parameter types, boxing the rare parts on first use.
    pub(crate) fn set_declared_parameters(&mut self, languages: crate::heap::ThinBoxSlice<u16>) {
        self.rare.get_or_insert_with(Default::default).declared_parameters = languages;
    }

    /// Sets the instance initializer, boxing the rare parts on first use.
    pub(crate) fn set_instance_initializer(&mut self, initializer: u32) {
        self.rare.get_or_insert_with(Default::default).instance_initializer = Some(initializer);
    }

    /// Sets what a Script or eval body declares globally (`FunctionRare::global_declarations`).
    pub(crate) fn set_global_declarations(&mut self, declarations: Option<Box<GlobalDeclarations>>) {
        if declarations.is_some() || self.rare.is_some() {
            self.rare.get_or_insert_with(Default::default).global_declarations = declarations;
        }
    }

    fn global_declarations_or_none(&self) -> Option<&GlobalDeclarations> {
        self.rare.as_deref().and_then(|rare| rare.global_declarations.as_deref())
    }

    pub fn global_lexical_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.lexical)
    }

    /// Each declared global lexical with its realm index slot (`GlobalDeclarations::slots`).
    pub(crate) fn global_lexical_slots(&self) -> impl Iterator<Item = (Atom, &std::cell::Cell<u16>)> {
        self.global_declarations_or_none().into_iter().flat_map(|declarations| {
            declarations.lexical.iter().copied().zip(declarations.slots.iter())
        })
    }

    /// The atom and realm index slot of the global lexical declaration `index`.
    pub(crate) fn global_lexical_declaration(&self, index: u32) -> Option<(Atom, &std::cell::Cell<u16>)> {
        let declarations = self.global_declarations_or_none()?;
        let index = index as usize;
        Some((*declarations.lexical.get(index)?, declarations.slots.get(index)?))
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

    /// The local slots of a Script's or eval's root that the realm and later code address by
    /// slot: a Script's every global declaration (`GlobalDeclarations`); a sloppy eval's var
    /// and function names, which are the global object's; none of a strict eval's, whose
    /// declarations are all its own (`ResidualProgram::root_variables_are_local`).
    pub(crate) fn global_declaration_slots(&self, eval: bool) -> impl Iterator<Item = u16> + '_ {
        // A set, once: a bundle's thousands of locals against its declarations was quadratic.
        let declared: rustc_hash::FxHashSet<Atom> = self
            .global_declarations_or_none()
            .filter(|_| !(eval && self.strict))
            .into_iter()
            .flat_map(|declarations| {
                let lexical: &[Atom] = if eval { &[] } else { &declarations.lexical };
                let immutable: &[Atom] = if eval { &[] } else { &declarations.immutable };
                lexical.iter()
                    .chain(&declarations.var)
                    .chain(&declarations.function)
                    .chain(&declarations.annex_b_var)
                    .chain(immutable)
                    .copied()
            })
            .collect();
        self.local_atoms
            .iter()
            .enumerate()
            .filter(move |(_, atom)| declared.contains(*atom))
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

    /// How many leading parameters live in registers in place, parameter `k` in register `k`,
    /// where every promoted local is such a parameter (and none is a rest parameter, whose
    /// array its frame makes): the layout a function that makes closures gets for the
    /// parameters none of them names (`Compiler::promoted_local_plan`'s `parameters_only`).
    /// An environment activation fills those registers as a call fills a plain activation's
    /// (`call_environment::EnvironmentPassed::in_registers`). 0 for any other layout.
    pub(crate) fn parameters_promoted_in_place(&self) -> u16 {
        let in_place = self.local_registers.iter().enumerate().all(|(index, entry)| {
            usize::from(entry.local) == index
                && usize::from(entry.register) == index
                && entry.local < self.params
                && !(self.rest && entry.local + 1 == self.params)
        });
        if in_place { u16::try_from(self.local_registers.len()).unwrap_or(0) } else { 0 }
    }

    /// Whether the instruction at `pc` returns `register`. A call is in tail position only
    /// when the instruction after it returns that call's own result.
    /// Whether its rest parameter is read only by `RestGet`, `RestLength` and `CallRest`
    /// (`compile::rest_reads`): an entry that keeps the values passed leaves the rest
    /// register `undefined` and makes no array.
    /// Derived once (`LazyFlag::RestView`): every frame entry of a rest function asks.
    #[inline]
    pub(crate) fn rest_view(&self) -> bool {
        self.rest && self.lazy_flag(LazyFlag::RestView, || {
            self.instructions_of(|op| matches!(op, Op::RestGet | Op::RestLength | Op::CallRest)).next().is_some()
        })
    }

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
                || instruction.op().addresses_env_local() && !selective
        }) || self.wide.iter().any(|instruction| {
            instruction.op() == Op::MakeClosure && !selective
                || instruction.op().addresses_env_local() && !selective
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
    pub(crate) fn plain_local_slots(&self, superinstructions: &[Superinstruction]) -> crate::bitset::BitSet {
        self.plain_local_slots_in(superinstructions, &initialization::FlowCell::new())
    }

    /// `plain_local_slots` over the shared control-flow blocks `flow` (`initialization::FlowCell`).
    pub(crate) fn plain_local_slots_in(&self, superinstructions: &[Superinstruction], flow: &initialization::FlowCell) -> crate::bitset::BitSet {
        let locals = usize::from(self.locals);
        if !self.plain_local_context_is_safe() {
            return crate::bitset::BitSet::new(locals);
        }
        let plain = crate::bitset::BitSet::from_fn(locals, |slot| self.plain_local_slot_is_safe(slot));
        let mut dead_zone = crate::bitset::BitSet::new(locals);
        for instruction in self.instructions_of(|op| op == Op::InitializeTdz) {
            if plain.contains(instruction.local_slot()) {
                dead_zone.insert(instruction.local_slot());
            }
        }
        let initialized =
            initialization::initialized_before_reads(self, superinstructions, &dead_zone, &Default::default(), flow);
        crate::bitset::BitSet::from_fn(locals, |slot| {
            plain.contains(slot) && (!dead_zone.contains(slot) || initialized.contains(slot))
        })
    }

    fn declared_atoms(&self) -> &DeclaredAtomSets {
        self.caches().declared_atom_sets.get_or_init(|| {
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
                local_slots: (self.local_atoms.len() > LOCAL_SLOT_SCAN).then(|| {
                    let mut slots = rustc_hash::FxHashMap::default();
                    for (slot, atom) in self.local_atoms.iter().enumerate().rev() {
                        slots.insert(*atom, slot as u32);
                    }
                    slots
                }),
            })
        })
    }

    /// The first local slot named `atom`: one probe in a function with many locals, where a
    /// scan per global write made a script of N declarations cost N^2.
    pub(crate) fn local_slot(&self, atom: Atom) -> Option<usize> {
        if self.local_atoms.len() <= LOCAL_SLOT_SCAN {
            return self.local_atoms.iter().position(|candidate| *candidate == atom);
        }
        self.declared_atoms().local_slots.as_ref()?.get(&atom).map(|slot| *slot as usize)
    }

    /// The declaration kinds of local `slot`'s name (none for a slot outside the function).
    pub(crate) fn slot_declaration(&self, slot: usize) -> SlotDeclaration {
        let (var, lexical) = (self.global_var_atoms(), self.global_lexical_atoms());
        if var.len() <= LOCAL_SLOT_SCAN && lexical.len() <= LOCAL_SLOT_SCAN {
            return self.local_atoms.get(slot).map_or_else(SlotDeclaration::default, |atom| SlotDeclaration {
                global_var: var.contains(atom),
                global_lexical: lexical.contains(atom),
            });
        }
        self.declared_atoms()
            .slots
            .get(slot)
            .copied()
            .unwrap_or_default()
    }

    /// Whether `atom` is among `global_lexical_atoms`.
    pub(crate) fn declares_global_lexical(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = self.global_lexical_atoms();
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
        self.declared_atoms().global_lexical.contains(&atom)
    }

    /// Whether `atom` is among `global_var_atoms`.
    pub(crate) fn declares_global_var(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = self.global_var_atoms();
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
        self.declared_atoms().global_var.contains(&atom)
    }

    /// Whether `atom` is among `global_immutable_atoms`.
    pub(crate) fn declares_global_immutable(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = self.global_immutable_atoms();
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
        self.declared_atoms().global_immutable.contains(&atom)
    }

    /// Whether `atom` is among `environment_atoms`.
    pub(crate) fn declares_environment_atom(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = &self.environment_atoms[..];
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
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
    pub(crate) fn activation_bindings_observable(&self, tables: &impl FactTables) -> bool {
        self.lazy_flag(LazyFlag::ActivationBindings, || self.derive_activation_bindings_observable(tables))
    }

    /// Whether the activation's own reads or the arrows it makes read its implicit bindings
    /// (`activation_bindings_observable` counts every arrow it makes): an arrow chain that reads
    /// none (`arrow_chain_reads_implicit_bindings`), as `xs.map(x => x * 2)` does, needs no
    /// `this` kept for it.
    /// `arrow_chain_reads_implicit_bindings` of this function, `id` of `code`, cached.
    pub(crate) fn arrow_reads_implicit_bindings(&self, code: &ResidualProgram, id: u32) -> bool {
        self.lazy_flag(LazyFlag::ArrowReads, || arrow_chain_reads_implicit_bindings(code, id))
    }

    pub(crate) fn maker_bindings_read(&self, code: &ResidualProgram) -> bool {
        self.lazy_flag(LazyFlag::MakerBindings, || {
            self.activation_binding_reads(code)
                || self.instructions_of(|op| op == Op::MakeClosure).any(|instruction| {
                    let id = instruction.closure_function_index();
                    code.function_is_arrow(id as usize).is_none_or(|is_arrow| is_arrow)
                        && arrow_chain_reads_implicit_bindings(code, id)
                })
        })
    }

    fn derive_activation_bindings_observable(&self, tables: &impl FactTables) -> bool {
        self.activation_binding_reads(tables)
            || self.instructions_of(|op| op == Op::MakeClosure).any(|instruction| {
                tables
                        .function_is_arrow(instruction.closure_function_index() as usize)
                        .is_none_or(|is_arrow| is_arrow)
            })
    }

    /// Whether the activation itself may read its implicit bindings through a name or scope
    /// operation, or must keep them for its kind of activation.
    fn activation_binding_reads(&self, tables: &impl FactTables) -> bool {
        self.is_class_constructor || self.binding_reads_beside_class(tables)
    }

    /// `activation_binding_reads` but for a class constructor's own kind: a base class
    /// constructor's body that reaches none of its implicit bindings (`new.target`, `super`
    /// and the like read hidden names) needs none to run frameless when constructed.
    fn binding_reads_beside_class(&self, tables: &impl FactTables) -> bool {
        self.is_async || self.is_generator || self.body_binding_reads(tables)
    }

    /// `binding_reads_beside_class` but for a suspendable kind's own: what the body itself
    /// reads. A non-suspending async body reaching none runs as an ordinary one would
    /// (`CallPlan::settles_in_loop`), its promise held by its call.
    fn body_binding_reads(&self, tables: &impl FactTables) -> bool {
        self.body_binding_reads_beside_clones(tables)
            || self.instructions_of(|op| op == Op::CloneEnv).next().is_some()
    }

    /// `body_binding_reads` but for the iteration copies of its environment (`CloneEnv`),
    /// which share the implicit bindings the environment holds rather than read them: an
    /// environment activation's window keeps those its closures may observe in the
    /// environment it makes (`window_environment`), which a copy shares as a frame's does.
    fn body_binding_reads_beside_clones(&self, tables: &impl FactTables) -> bool {
        self.evals_in_scope() || self.reads_implicit_bindings(tables)
    }

    /// Whether the function runs a direct eval, which declares `var`s in its variable
    /// environment and reads its scope by name, or records binding sites, the lexical scope
    /// its closures and evals resolve names in.
    pub(crate) fn evals_in_scope(&self) -> bool {
        self.lazy_flag(LazyFlag::EvalsInScope, || self.derive_evals_in_scope())
    }

    fn derive_evals_in_scope(&self) -> bool {
        !self.binding_sites.is_empty()
            || self.instructions_of(Op::is_call_with_eval_flags).any(|instruction| instruction.direct_eval() || instruction.parameter_eval())
    }

    /// The implicit bindings (`new.target`, `super`, the home object, a derived constructor's
    /// `this`) the body reads by a hidden name or a scope operation, which a window activation
    /// leaves to the frame it becomes: not those of a direct eval, which its record serves.
    fn reads_implicit_bindings(&self, tables: &impl FactTables) -> bool {
        self.derived_constructor
            // A field initializer or static block keeps its instance's implicit bindings; an
            // arrow inside one (also `class_initializer`) binds none of its own.
            || (self.class_initializer && !self.is_arrow)
            || self.instance_initializer().is_some()
            || self.inherited_with_scope
            || self.instructions_of(name_bindings::may_read_binding).any(|instruction| {
                name_bindings::reads_hidden_name(instruction, tables)
                    || matches!(
                        instruction.op(),
                        Op::DeleteName | Op::StoreResolvedName | Op::StoreVarBinding | Op::InitializeThis | Op::SuperCallCheck
                    )
            })
            // A direct eval beside names resolved through a scope may declare dynamic bindings, which only
            // a frame (or a suspended record) keeps: the scope's other names the window's handlers serve.
            || self.evals_in_scope() && self.instructions_of(|op| matches!(op, Op::ResolveName | Op::LoadResolvedName)).next().is_some()
    }

    /// Every instruction in fixed-width form.
    pub(crate) fn instructions(&self) -> impl Iterator<Item = WideInstruction> + '_ {
        self.code.iter().map(|&packed| self.widened(packed))
    }

    /// The instruction at `pc` in fixed-width form.
    pub(crate) fn instruction_at(&self, pc: usize) -> WideInstruction {
        self.widened(self.code[pc])
    }

    /// The field site of the nested read (`GetField` of `FieldLookup::Site`) at `pc`, if the
    /// instruction there is one: a frame paused at it calls its last read's getter.
    pub(crate) fn nested_read_site(&self, pc: usize) -> Option<usize> {
        let instruction = self.widened(*self.code.get(pc)?);
        if !matches!(instruction.op(), Op::GetField) {
            return None;
        }
        match instruction.field_lookup()? {
            FieldLookup::Site(site) => Some(site),
            FieldLookup::Atom { .. } => None,
        }
    }

    /// The opcode of the packed instruction `packed` of this function, without decoding its
    /// operands.
    #[inline(always)]
    pub(crate) fn op_of(&self, packed: Instr) -> Op {
        if packed.is_wide() { self.wide[packed.wide_index()].op() } else { packed.op() }
    }

    /// The fixed-width form of the packed instruction `packed` of this function: the one
    /// decoder of the packed encoding every reader of the code takes.
    #[inline(always)]
    pub(crate) fn widened(&self, packed: Instr) -> WideInstruction {
        widen(packed, &self.wide)
    }

    /// Whether a closure environment may hold local `slot`: the selective layout names it or,
    /// without one, the function captures its whole frame by creating closures or reaching its
    /// environment.
    pub(crate) fn local_slot_captured(&self, slot: usize) -> bool {
        match &self.selective_capture_slots {
            Some(captured) => {
                u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
            }
            None => self
                .instructions_of(|op| matches!(op, Op::MakeClosure | Op::LoadEnvLocal | Op::StoreEnvLocal))
                .next()
                .is_some(),
        }
    }

    /// Whether legacy `f.arguments` reads each passed parameter's current frame value, as V8
    /// does for parameters that live only in the frame: no code or closure uses an `arguments`
    /// binding, the parameters are simple, and each is a plain or register-promoted local.
    /// Otherwise the arguments are reflected as passed.
    pub(crate) fn reflects_live_parameters(&self, tables: &impl FactTables) -> bool {
        self.lazy_flag(LazyFlag::LiveParameters, || self.derive_reflects_live_parameters(tables))
    }

    fn derive_reflects_live_parameters(&self, tables: &impl FactTables) -> bool {
        {
            let arguments_used = self.local_atoms.iter().enumerate().any(|(slot, &atom)| {
                usize::try_from(atom).is_ok_and(|atom| tables.atom_name(atom) == Some("arguments"))
                    && (self.local_slot_captured(slot)
                        || self
                            .instructions_of(|op| op.immediate_role() == ImmediateRole::LocalSlot)
                            .any(|instruction| instruction.local_slot() == slot))
            });
            self.arguments_slot.is_none()
                && !arguments_used
                && self.simple_parameters
                && !self.rest
                && (0..self.params).all(|parameter| {
                    self.plain_local(tables, usize::from(parameter))
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

    /// The body's back edges, as (source pc, target pc), in source order (`back_edges_of`).
    pub(crate) fn back_edges(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        back_edges_of(self.instructions())
    }

    /// Whether some jump of the body targets its own or an earlier instruction: a loop.
    pub(crate) fn loops(&self) -> bool {
        self.back_edges().next().is_some()
    }

    /// Whether an activation can suspend: an `await`, or a `yield` of a generator.
    pub(crate) fn suspends(&self) -> bool {
        // By opcode alone: a wide record's opcode is in the side table.
        self.code.iter().any(|packed| {
            if packed.is_wide() { self.wide[packed.wide_index()].op().suspends() } else { packed.op().suspends() }
        })
    }

    /// Whether a suspension of the function parks its frame in a record a lane resumes from: a
    /// generator or an async function. An arrow's `this` is its environment's, which each
    /// resumption reads again (`Vm::window_this`).
    fn suspends_in_record(&self) -> bool {
        self.is_generator || self.is_async
    }

    /// Whether its locals and the environment its closures capture can live in a window past
    /// its registers: the layout an environment activation (`CallPlan::environment_activation`)
    /// and a suspended generator's window (`CallPlan::suspended_environment`) share. Each
    /// local that is not promoted is a frame local, the captured ones owned by the environment,
    /// and every instruction that addresses one is a record the window's handlers read
    /// (`window_reads_locals`); the promoted ones are registers, which the window's register
    /// file holds as it holds every register. A function's own name binding is a local too,
    /// which the entry initializes with the callee and whose stores leave the lane
    /// (`specialize_environment_window`).
    fn locals_ride_window(&self, tables: &impl FactTables) -> bool {
        self.arguments_own_their_slots(|atom| tables.atom_name(atom))
            && (self.arguments_slot.is_none() || self.reads_passed_arguments_only() || self.arguments_ride_window())
            // The implicit bindings `activation_binding_reads` names, but for clones, are a frame's:
            // a suspended function's record keeps them (and gives them to the environment its
            // window makes, `window_environment`), and the opcodes that read them
            // exit the lane to the frame it becomes.
            && (self.names_resolve_in_window() || self.holds_scope())
            && (!self.reads_implicit_bindings(tables) || self.suspends_in_record())
            && self.instructions().all(|instruction| {
                instruction.op().immediate_role() != ImmediateRole::SuperinstructionIndex
                    && self.window_reads_locals(tables, instruction)
            })
    }

    /// Whether the handlers of an environment window read every local `instruction` addresses:
    /// the local loads, stores and dead-zone openings, a field read from a plain local
    /// (`plain_local_operand`), a binary operator of a suspending function over plain
    /// locals, whose operands and numeric target the window holds (`lane_store_numeric_local`),
    /// the reads of the values
    /// passed, which the window keeps, and a function's store to a `var` binding its Annex B
    /// block function declares (`StoreVarBinding`), which is a store to the slot: only a
    /// program's root stores through the global object or an eval's record.
    fn window_reads_locals(&self, tables: &impl FactTables, instruction: WideInstruction) -> bool {
        let op = instruction.op();
        instruction.addressed_local_slots().next().is_none()
            || match op {
                Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::StoreLocal | Op::StoreLocalPlain | Op::StoreEnvLocal | Op::InitializeTdz => true,
                Op::StoreVarBinding => self.parent.is_some(),
                Op::GetField => instruction.addressed_local_slots().eq(instruction.local_field_base()),
                Op::Binary | Op::NumericAdd | Op::NumericMultiply if self.suspends_in_record() => {
                    instruction.addressed_local_slots().all(|slot| self.plain_local(tables, usize::from(slot)))
                }
                _ => op.reads_arguments(),
            }
    }

    /// Whether every name of the function resolves by slot, which an environment window answers: none
    /// goes through a `with` object, a binding site or a dynamic binding (`has_dynamic_scope_access`).
    fn names_resolve_in_window(&self) -> bool {
        !self.inherited_with_scope && self.binding_sites.is_empty() && !self.has_dynamic_scope_access()
    }

    /// Whether a function may lay out an environment window though it resolves names through `with`
    /// objects, binding sites or dynamic bindings, which its activation's scope chain and, for a
    /// function that suspends, its record carry: one that makes no iteration copy of its environment
    /// (`CloneEnv`), which would have to share them (`Vm::iteration_environment`).
    fn holds_scope(&self) -> bool {
        self.instructions_of(|op| op == Op::CloneEnv).next().is_none()
    }

    /// Whether the `arguments` object a suspending function made on entry rides its window as
    /// the local it was stored in, which the lane reads in place: a
    /// window local, or, for a binding a closure captures, its slot in the environment the
    /// window makes (`mapped_windows::ParameterHome`). A mapped object (`arguments_alias_parameters`) aliases
    /// parameters that live in the window as well, the captured ones among them, which ride the
    /// window's locals until a closure makes its environment and its slots after
    /// (`ParameterHome`): the window's parameter reads and writes go through the object
    /// whose element writes reach the binding
    /// (`LaneStack::window_parameter`).
    pub(crate) fn arguments_ride_window(&self) -> bool {
        self.arguments_slot.is_some() && self.suspends_in_record()
    }

    /// The function's `CallPlan`, derived on first use.
    #[inline(always)] pub(crate) fn call_plan(&self, tables: &impl FactTables) -> CallPlan {
        CallPlan::from_bits(u32::from(*self.call_plan.get_or_init(|| u16::try_from(self.derive_call_plan(tables).to_bits()).expect("the plan's flags fit 16 bits"))))
    }

    /// Whether an async body settles its call's promise from its activation: one that never
    /// suspends runs to completion there, and one whose suspension keeps only its registers
    /// (`suspended_registers`) leaves its caller the promise at its first `await`, its
    /// registers becoming the continuation the reaction resumes.
    fn settles_in_loop(&self, suspended_registers: bool) -> bool {
        self.is_async
            && !self.is_generator
            && !self.is_class_constructor
            && !self.derived_constructor
            && (!self.suspends() || suspended_registers)
    }

    fn derive_call_plan(&self, tables: &impl FactTables) -> CallPlan {
        {
            let promoted_parameters = self.local_registers.iter().filter(|entry| entry.local < self.params).count();
            // Laid out as its registers whatever kind of function it is: an ordinary
            // body's layout, or a non-suspending async body's (`settles_registers_only`).
            let registers_layout_of_any_kind = self.parent.is_some()
                && !self.derived_constructor
                && self.self_binding_slot.is_none()
                && !self.inherited_with_scope
                && promoted_parameters == usize::from(self.params)
                // A plain activation makes no closure, whichever bindings it would see.
                && self.instructions_of(|op| op == Op::MakeClosure).next().is_none();
            let registers_layout = registers_layout_of_any_kind && !self.is_generator && !self.is_async;
            // Plain but for an `arguments` binding.
            // An arrow's `this` is its environment's, which its activation binds
            // (`Vm::lexical_activation_this`). One that reaches `super` reads a hidden name
            // (`activation_binding_reads`) or its home as a capture, as any closure reads one.
            // Ordinary but for a rest parameter, which a frameless activation gathers
            // (`CallEntry::Gathers`) and a plain frame does not.
            let ordinary_with_rest = registers_layout && !self.activation_binding_reads(tables);
            // A base class constructor laid out as an ordinary function runs frameless when
            // constructed (`script_construct_miss`); its calls, which throw, keep a frame.
            let constructs_frameless = registers_layout
                && self.is_class_constructor
                && !self.rest
                && !self.binding_reads_beside_class(tables);
            let ordinary = ordinary_with_rest && !self.rest;
            // A non-suspending async body laid out as an ordinary one's, which keeps no
            // passed values: its activation settles its call's promise.
            let settles_with_registers = registers_layout_of_any_kind
                && !self.body_binding_reads(tables)
                && !self.rest
                && self.arguments_slot.is_none();
            // A field initializer is an ordinary method call on the instance whose
            // activation keeps its implicit bindings (`activation_binding_reads`); only the
            // derived instance initializer, which `super()` runs, is entered natively.
            let in_loop = !self.is_async
                && !self.is_generator
                && !self.is_class_constructor
                && !self.derived_constructor;
            let legacy_reflection = !self.has_restricted_legacy_caller_access();
            let live_parameters = self.reflects_live_parameters(tables);
            let kept_arguments = self.kept_arguments();
            let forwards_arguments = kept_arguments == Some(KeptArguments::Forwards);
            let addresses_locals = self.instructions().any(|instruction| {
                instruction.op().captures_environment()
                    || instruction.addressed_local_slots().next().is_some()
            });
            let reaches_only_forwarded_arguments = kept_arguments.is_some()
                && self.instructions().all(|instruction| {
                    instruction
                        .addressed_local_slots()
                        .all(|slot| Some(slot) == self.arguments_slot)
                });
            // An ordinary call places its arguments in the window's locals and fills the
            // registers with `undefined`, but for the parameters promoted in place, parameter
            // `k` in register `k` (`parameters_promoted_in_place`), which it places in their
            // registers as a plain activation's call does: a function that makes closures keeps
            // the parameters none of them names there (one-frame-one-call.md F6). Any other
            // promoted local has no window home. A suspended frame is made with both, and no
            // suspending function starts its registers apart from `undefined` (Wasm's i32 zero
            // belongs to its bodies, which never suspend).
            let environment_activation = in_loop
                && self.parent.is_some()
                && (self.local_registers.is_empty() || self.parameters_promoted_in_place() != 0)
                && self.initial_register == InitialRegister::Undefined
                && self.locals_ride_window(tables);
            let suspendable = self.suspends_in_record();
            let suspended_registers = suspendable
                && registers_layout_of_any_kind
                && !self.rest
                && self.arguments_slot.is_none()
                && !addresses_locals
                && !self.has_dynamic_scope_access();
            let settles_in_loop = self.settles_in_loop(suspended_registers);
            // Its locals and closures ride an environment window as an ordinary body's do, and so
            // does the `arguments` object it made on entry (`arguments_ride_window`); so do its
            // `with` objects and dynamic bindings, which its record and activation carry.
            // An async module's body is a program's root, which no function encloses: its slots
            // that stand for imports and global bindings are read through the activation
            // (`Vm::root_binding`).
            let suspended_environment = suspendable
                && !suspended_registers
                && (self.parent.is_some() || self.is_async)
                && self.locals_ride_window(tables);
            CallPlan {
                environment_activation,
                suspended_registers,
                suspended_environment,
                admits_direct_eval: self.evals_in_scope() && (environment_activation || suspended_environment),
                plain: ordinary && self.arguments_slot.is_none(),
                in_loop,
                settles_in_loop,
                legacy_reflection,
                live_parameters,
                addresses_locals,
                forwards_arguments,
                keeps_arguments: kept_arguments.is_some(),
                reads_passed_arguments: self.reads_passed_arguments_only(),
                registers_only: (ordinary_with_rest && in_loop || constructs_frameless || settles_in_loop && settles_with_registers)
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
    /// `names_resolve_statically` is given: the one fact that reads the enclosing functions,
    /// which a compiler finishing this function early has not finished yet
    /// (`binary::LoadEncoding`).
    pub(crate) fn derived_facts(&self, tables: &impl FactTables, names_resolve_statically: bool) -> u32 {
        let facts = [
            (DerivedFact::ActivationBindings, self.activation_bindings_observable(tables)),
            (DerivedFact::LiveParameters, self.reflects_live_parameters(tables)),
            (DerivedFact::StaticNameScope, names_resolve_statically),
            (DerivedFact::DynamicScopeAccess, self.has_dynamic_scope_access()),
        ];
        let flags = facts.iter().fold(0, |bits, &(fact, holds)| if holds { bits | fact.bit() } else { bits });
        flags | (self.call_plan(tables).to_bits() << DERIVED_FACT_COUNT)
    }

    /// Seeds the facts `derived_facts` packed; a fact already derived keeps its value.
    pub(crate) fn seed_derived_facts(&self, bits: u32) {
        let fact = |fact: DerivedFact| bits & fact.bit() != 0;
        self.seed_lazy_flag(LazyFlag::ActivationBindings, fact(DerivedFact::ActivationBindings));
        self.seed_lazy_flag(LazyFlag::LiveParameters, fact(DerivedFact::LiveParameters));
        self.seed_lazy_flag(LazyFlag::StaticNameScope, fact(DerivedFact::StaticNameScope));
        self.seed_lazy_flag(LazyFlag::DynamicScopeAccess, fact(DerivedFact::DynamicScopeAccess));
        let _ = self.call_plan.set(u16::try_from(bits >> DERIVED_FACT_COUNT).expect("the plan's flags fit 16 bits"));
    }

    /// Whether each fact `derived_facts` packs is this function's own derivation, whether it
    /// was derived or seeded (`ResidualProgram::validate`).
    pub(crate) fn derived_facts_hold(&self, program: &ResidualProgram) -> bool {
        self.activation_bindings_observable(program)
            == self.derive_activation_bindings_observable(program)
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
    #[inline(always)] pub(crate) fn initializer_stores(&self, constants: &[Constant]) -> Option<&[InitializerStore]> {
        self.caches()
            .initializer
            .get_or_init(|| self.derive_initializer_stores(constants).map(Into::into))
            .as_deref()
    }

    /// The stores of a body whose whole effect is storing parameters and primitive constants
    /// into fields of `this`, then returning `undefined`, in order; `None` for any other body.
    /// A constant store is the `SetThisField` of a temporary a primitive constant's
    /// `LoadConst` wrote, which writes nothing else. A parameter store's value is a parameter as passed (`SetThisField` of
    /// the parameter's register), or a Number
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
        let mut temporaries = initializer_stores::Temporaries::default();
        let mut pc = 0;
        let store = |instruction: &WideInstruction| instruction.stores_to_this().then_some(());
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
                    && !stores.iter().any(|stored: &InitializerStore| stored.parameter() == Some(p)) =>
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
                    stores.push(InitializerStore {
                        atom: set.imm(),
                        site: set.c(),
                        value: InitializerValue::Parameter { parameter: p, fallback: Some(k) },
                    });
                    pc += 5;
                }
                set if store(set).is_some() && let Some(p) = parameter(set.register_a()) => {
                    stores.push(InitializerStore {
                        atom: set.imm(),
                        site: set.c(),
                        value: InitializerValue::Parameter { parameter: p, fallback: fallbacks[usize::from(p)] },
                    });
                    pc += 1;
                }
                // `this.f = k` or `this.f = p op k` of a temporary (`Temporaries::value`).
                set if store(set).is_some() && let Some(value) = temporaries.value(set.register_a()) => {
                    stores.push(InitializerStore { atom: set.imm(), site: set.c(), value });
                    pc += 1;
                }
                _ if let Some(length) = temporaries.consume(&instructions, pc, (self.params, &fallbacks, constants), &mut stores) => pc += length,
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
                        && match temporaries.value(completion.register_a()) {
                            Some(value) => value == InitializerValue::Constant(InitializerConstant::Undefined),
                            None => self.initial_register == InitialRegister::Undefined,
                        })
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
            fallback.is_none() || stores.iter().any(|stored: &InitializerStore| stored.parameter().map(usize::from) == Some(p))
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
                instruction.op().reads_arguments()
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
            || self.instance_initializer().is_some()
            || self.is_async
            || self.is_generator
        {
            return None;
        }
        // Each shape has a fixed number of records, so the record count rejects any other
        // body before a record decodes: the implicit constructor reads its first eight (its
        // tail is unreachable), the parameter form is exactly `FORWARDED_PARAMETERS_FRAME`
        // around its parameter loads. The rewrite may also leave the active function's copy
        // into the parent call's window as a `Move` right before the call
        // (`FORWARDED_WINDOW_COPY`).
        let shaped_records = if self.rest {
            FORWARDED_REST_RECORDS
        } else {
            FORWARDED_PARAMETERS_FRAME + usize::from(self.params)
        };
        let fits = |count: usize| {
            if self.rest {
                count >= shaped_records
            } else {
                count == shaped_records || count == shaped_records + FORWARDED_WINDOW_COPY
            }
        };
        if !fits(self.code.len()) {
            return None;
        }
        let mut records = self.instructions().take(shaped_records + FORWARDED_WINDOW_COPY).collect::<SmallVec<[WideInstruction; 16]>>();
        // The copy, dropped here: the call's window is that copy.
        let copied = records.iter().position(|record| record.op() == Op::Call).and_then(|call| {
            let moved = *records.get(call.checked_sub(1)?)?;
            (moved.op() == Op::Move && moved.result_flags() == 0).then(|| {
                records.remove(call - 1);
                (moved.register_b(), moved.result_register())
            })
        });
        // A parameter form's every record is read: one longer only by the dropped copy.
        let read = self.code.len() - usize::from(copied.is_some());
        if records.len() < shaped_records || (!self.rest && read != shaped_records) {
            return None;
        }
        records.truncate(shaped_records);
        let window_of = |active: &WideInstruction| match copied {
            None => Some(active.result_register()),
            Some((source, window)) => (source == active.result_register()).then_some(window),
        };
        let parent_of = |active: &WideInstruction, base: &WideInstruction, receiver: &WideInstruction, call: &WideInstruction| {
            super_initializer::calls_super_base(atoms, constants, [active, base, receiver, call], window_of(active))
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
        if construct.op() != Op::Construct {
            return None;
        }
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
            if instruction.op().captures_environment() {
                return None;
            }
            if !instruction
                .addressed_local_slots()
                .any(|addressed| addressed == slot)
            {
                continue;
            }
            let loads = instruction.op().loads_frame_local();
            let feeds_forwarding_call = loads
                && instructions.get(pc + 1).is_some_and(|next| {
                    next.op() == Op::CallForwardingArguments
                        && next.call_window().last() == Some(instruction.result_register())
                });
            let reads_kept_values =
                instruction.op().reads_arguments();
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
    /// later read needs it. A handler's edges are traced too (`Successors`, its target and a
    /// `finally`'s return target from every instruction it covers, the pass included), so a
    /// read in a `catch` or `finally` that may run after a pass answers `false`; one that
    /// runs only before any pass reads the object a materialized frame made on entry, from
    /// the same kept values (one-frame-one-call.md F6 (d)).
    fn arguments_passed_last(&self, instructions: &[WideInstruction], slot: u16, passes: &[usize]) -> bool {
        let successors = (!self.handlers.is_empty()).then(|| Successors::of(self, instructions));
        let successors_of = |pc: usize, work: &mut Vec<usize>| match &successors {
            Some(successors) => work.extend(successors.at(pc).map(|(next, _)| next)),
            None => crate::bytecode::for_each_instruction_successor(pc, instructions[pc], |next| work.push(next)),
        };
        let mut reached = crate::bitset::BitSet::new(instructions.len());
        let mut work = Vec::new();
        for &pass in passes {
            successors_of(pass, &mut work);
        }
        while let Some(pc) = work.pop() {
            let Some(instruction) = instructions.get(pc) else {
                continue;
            };
            if !reached.insert(pc) {
                continue;
            }
            if instruction.addressed_local_slots().any(|addressed| addressed == slot) {
                return false;
            }
            successors_of(pc, &mut work);
        }
        true
    }

    /// Whether the function itself can reach bindings by name at run time: it was created
    /// inside a `with` scope, records by-name lexical sites, resolves names dynamically, or
    /// calls direct eval (also from a parameter scope).
    /// Derived once per function: lane views, capture layouts and name resolution each ask,
    /// and the instruction scan had cost 1.3% of the axios npm scenario's instructions.
    pub(crate) fn has_dynamic_scope_access(&self) -> bool {
        self.lazy_flag(LazyFlag::DynamicScopeAccess, || self.derive_dynamic_scope_access())
    }

    fn derive_dynamic_scope_access(&self) -> bool {
        // Only these ops resolve a dynamic scope: the scan decodes no other.
        dynamic_scope_access_in(
            self,
            self.instructions_of(|op| op == Op::ResolveName || op.is_call_with_eval_flags()),
        )
    }

    /// Whether the body enters a `with` statement, whose by-name loads may denote
    /// any enclosing function's binding. `is_with_enter` recognizes the atom id of
    /// [`WITH_ENTER_BINDING`] in the owning program's atom table.
    pub(crate) fn contains_with_statement(&self, is_with_enter: impl Fn(usize) -> bool) -> bool {
        self.instructions_of(Op::is_throwing_name_load).any(|instruction| usize::try_from(instruction.atom_index()).is_ok_and(&is_with_enter))
    }

    /// Whether neither this function nor any enclosing one has dynamic scope access or
    /// contains a `with` statement. The compiler then resolved every lexical binding the body
    /// names, so a by-name load in it can only denote a global binding.
    /// `program` holds this function; an enclosing function whose record it has not read yet
    /// is read here.
    pub(crate) fn names_resolve_statically(&self, program: &ResidualProgram) -> bool {
        self.lazy_flag(LazyFlag::StaticNameScope, || self.derive_names_resolve_statically(program))
    }

    fn derive_names_resolve_statically(&self, program: &ResidualProgram) -> bool {
        self.own_names_resolve_statically(program)
            && self.parent.is_none_or(|parent| {
                program
                    .function_get(parent as usize)
                    .is_some_and(|parent| parent.names_resolve_statically(program))
            })
    }

    /// `names_resolve_statically` but for the enclosing functions: this function itself has
    /// no dynamic scope access and contains no `with` statement.
    pub(crate) fn own_names_resolve_statically(&self, tables: &impl FactTables) -> bool {
        !self.has_dynamic_scope_access()
            && !self.contains_with_statement(|atom| tables.atom_name(atom) == Some(WITH_ENTER_BINDING))
    }

    /// The memoized `plain_local_slots` projection for this residual's atoms.
    #[inline(always)] pub(crate) fn plain_local(&self, tables: &impl FactTables, slot: usize) -> bool {
        let words =
            self.plain_locals.get_or_init(|| self.plain_local_slots(tables.superinstructions()).words().to_vec().into());
        crate::bitset::words_contain(words, slot)
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

    /// Whether the interpreter's checked local rows answer this function's slots directly: the root
    /// body's bindings are the VM's (`root`), and so are a self-binding slot and mapped arguments,
    /// whose stores the owner step writes through. A function without them keeps its slots in its frame.
    pub(crate) fn has_plain_locals(&self, root: bool) -> bool {
        !root && self.self_binding_slot.is_none() && !self.arguments_alias_parameters()
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum DispatchClass {
    /// The class a function without a numeric body takes, and a lowering's default.
    #[default]
    General,
    Numeric,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MethodSite {
    pub atom: Atom,
    pub cache: u16,
    pub argument_start: u32,
    pub argument_count: u16,
    pub receiver_path: ReceiverPath,
}

const _: () = assert!(size_of::<MethodSite>() == 20);

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
    /// The literal's only use is as the receiver of a `test` or `exec` call (`/re/.test(s)`):
    /// while those builtins are intact the object never escapes, so one object per site
    /// serves every evaluation (`Vm::regexp_literal_of`).
    pub receiver: bool,
}

/// A fused window of narrow instructions, held once in the expanded form dispatch reads (every
/// operand of every part). Its parts are never wide references, which `new` checks. The packed
/// form of the same parts was a second copy of them, and nothing reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Superinstruction {
    pub wide: [WideInstruction; 4],
}

impl Superinstruction {
    pub(crate) fn new(code: [Instr; 4]) -> Self {
        assert!(code.iter().all(|part| !part.is_wide()), "a superinstruction's parts are narrow");
        Self { wide: code.map(Instr::as_wide) }
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
    /// The compiled-source offsets the site's parts come from, each a `source_word`.
    pub(crate) sources: [u32; 3],
}
impl FieldSite {
    /// The `sources` word of a part with no compiled-source offset. It is never an offset: a
    /// source of `u32::MAX` bytes cannot be named (`source_word`).
    pub(crate) const NO_SOURCE: u32 = u32::MAX;

    /// `offset` as a `sources` word.
    pub(crate) const fn source_word(offset: Option<u32>) -> u32 {
        match offset {
            Some(offset) if offset != Self::NO_SOURCE => offset,
            _ => Self::NO_SOURCE,
        }
    }

    pub(crate) fn source_offsets(self) -> [Option<u32>; 3] {
        self.sources.map(|word| (word != Self::NO_SOURCE).then_some(word))
    }

    pub(crate) fn source(self, part: FieldPart) -> Option<u32> {
        self.source_offsets()[match part {
            FieldPart::First => 0,
            FieldPart::Second => 1,
            FieldPart::Sink => 2,
        }]
    }

    /// The part whose read comes last (the second field's where it has one): the read whose
    /// getter a call from the site's record makes (`Vm::resolve_field`).
    pub(crate) fn last_read_part(self) -> FieldPart {
        if self.second.is_some() { FieldPart::Second } else { FieldPart::First }
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
    /// The text of a host caller's owned string, which the program keeps as it is: a host that
    /// already owns its source hands the string over instead of a copy of it.
    Owned(std::rc::Rc<String>),
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
            Self::Owned(text) => text,
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

/// A source text's line starts and UTF-16 excess table: what maps a byte offset of the text to
/// its line and column. Made once per text, on the first position that asks for one.
#[derive(Debug)]
pub(crate) struct SourceLayout {
    /// The byte offset of each line start (`source_line_starts`).
    line_starts: Box<[u32]>,
    /// The byte offset of each non-ASCII character, with the running excess of its UTF-8 bytes
    /// over its UTF-16 units through it (`source_utf16_excess`).
    utf16_excess: Box<[(usize, usize)]>,
}

impl SourceLayout {
    pub(crate) fn of(text: &str) -> Self {
        Self {
            line_starts: source_line_starts(text).map(|start| u32::try_from(start).unwrap_or(u32::MAX)).collect(),
            utf16_excess: source_utf16_excess(text).into(),
        }
    }

    /// The line and UTF-16 column of byte `offset` of the text: the line holding it, and its
    /// column counted in UTF-16 units from the line start.
    pub(crate) fn line_column(&self, offset: usize) -> SourcePosition {
        let starts = &self.line_starts;
        let line_index = starts.partition_point(|start| *start as usize <= offset).saturating_sub(1);
        let line_start = starts[line_index] as usize;
        let excess_before = |offset: usize| {
            self.utf16_excess
                .partition_point(|(character, _)| *character < offset)
                .checked_sub(1)
                .map_or(0, |index| self.utf16_excess[index].1)
        };
        let column = offset - line_start - (excess_before(offset) - excess_before(line_start));
        SourcePosition {
            line: u32::try_from(line_index.saturating_add(1)).unwrap_or(u32::MAX),
            column: u32::try_from(column.saturating_add(1)).unwrap_or(u32::MAX),
        }
    }
}

/// Byte offset of each non-ASCII character of `text` with the running excess of UTF-8 bytes
/// over UTF-16 units through it.
pub(crate) fn source_utf16_excess(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut table = Vec::new();
    let mut excess = 0;
    let mut offset = 0;
    while let Some(skipped) = ascii_prefix_len(&bytes[offset..]) {
        offset += skipped;
        let character = text[offset..].chars().next().expect("a non-ASCII byte starts a character");
        excess += character.len_utf8() - character.len_utf16();
        table.push((offset, excess));
        offset += character.len_utf8();
    }
    table
}

/// The bytes of `bytes` before its first non-ASCII byte, `None` when every one is ASCII:
/// a word of bytes at a time, as source text is nearly all ASCII.
fn ascii_prefix_len(bytes: &[u8]) -> Option<usize> {
    const WORD: usize = size_of::<u64>();
    const HIGH_BITS: u64 = u64::from_ne_bytes([0x80; WORD]);
    let (words, _) = bytes.as_chunks::<WORD>();
    let mut offset = 0;
    for word in words {
        if u64::from_ne_bytes(*word) & HIGH_BITS != 0 {
            break;
        }
        offset += WORD;
    }
    bytes[offset..].iter().position(|byte| !byte.is_ascii()).map(|position| offset + position)
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
    /// The text's line and UTF-16 layout (`SourceLayout`), made on the first position or line
    /// that asks for one and shared by every copy of the source, so a copy (an error's captured
    /// call sites, a first-call chunk's program) copies no index.
    pub(crate) layout: std::cell::OnceCell<std::sync::Arc<SourceLayout>>,
    /// Complete static call-printing facts for native images. An unset cell
    /// leaves the dynamic error-path renderer in charge; an empty table is final.
    pub(crate) call_sites: std::cell::OnceCell<std::sync::Arc<[CallSiteDescription]>>,
}

impl ProgramSource {
    /// Project admitted facts with a retained wrapper body, without reparsing it.
    pub(crate) fn retain_body(&mut self, text: SourceText, base: u32) {
        self.text = text;
        self.base = base;
        self.layout.take();
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
        // `layout` is a memo of `text`, made again on the first position read.
        crate::vm::freeze_source_call_sites(self);
        let Self { text, typescript, base, line_offset, column_offset, layout: _, call_sites } = self;
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
            call_sites: Default::default(),
            layout: Default::default(),
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
    /// The line and UTF-16 layout of the kept text.
    pub(crate) fn layout(&self) -> &SourceLayout {
        self.layout.get_or_init(|| std::sync::Arc::new(SourceLayout::of(&self.text)))
    }

    /// The position of compiled-source byte `offset` (a `SourceOffset`'s), in the coordinates
    /// the program reports: a wrapper's lines and columns are removed, and a kept body that
    /// starts at its wrapper origin already reports them.
    pub(crate) fn position(&self, offset: u32) -> Option<SourcePosition> {
        let index = self.text_offset(offset)? as usize;
        let position = self.layout().line_column(index);
        if self.base != 0 {
            return Some(position);
        }
        let line = position.line.checked_sub(self.line_offset).filter(|line| *line > 0)?;
        let column = if line == 1 {
            position.column.checked_sub(self.column_offset).filter(|column| *column > 0)?
        } else {
            position.column
        };
        Some(SourcePosition { line, column })
    }

    /// The byte offset in the kept text of compiled-source byte `offset`, if the text holds it.
    pub(crate) fn text_offset(&self, offset: u32) -> Option<u32> {
        offset.checked_sub(self.base).filter(|offset| *offset as usize <= self.text.len())
    }

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
        let starts = &self.layout().line_starts;
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

impl CacheSiteCounts {
    /// Whether caches laid out for `layout` hold every site of these counts.
    pub(crate) fn fits_within(self, layout: Self) -> bool {
        self.fields <= layout.fields && self.methods <= layout.methods && self.objects <= layout.objects
    }
}

#[derive(Clone, Debug)]
pub struct ResidualProgram {
    pub(crate) specialized: bool,
    pub(crate) kind: ProgramKind,
    pub(crate) module_requests: Vec<ModuleRequest>,
    pub(crate) module_imports: Vec<ModuleImportBinding>,
    pub(crate) module_link_plan: Option<ModuleLinkPlan>,
    /// The TypeScript declared environment (`DeclaredTypes`); none for JavaScript.
    pub(crate) declared_types: Option<Box<DeclaredTypes>>,
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
            let header = self.functions[index].header();
            let slot = &self.functions[index].unlowered;
            match purpose {
                RecordUse::Kept => slot.take().map(|unlowered| unlowered.into_function(header)),
                RecordUse::Transient => slot.with(|unlowered| unlowered.map(|unlowered| unlowered.function(header))),
            }
        };
        if let Some(unlowered) = unlowered {
            if !crate::DYNAMIC_EXECUTION_ENABLED {
                panic!("native function {index} of {} was not lowered before linking", self.source_name);
            }
            let rewrites_pending = self.rewrite_ranges.defers(index);
            let function = self.lower_unlowered(index, unlowered, rewrites_pending);
            function.decoded.prime_loop(&function.code, &function.wide);
            return function;
        }
        if let Some(lazy) = &self.wasm_lazy {
            let function = lazy.lower(index, &self.constants);
            function.decoded.prime_loop(&function.code, &function.wide);
            return function;
        }
        let deferred = self.encoded_records.as_deref().and_then(|records| binary::decode_deferred_function(records, index));
        let function = match deferred {
            Some(decoded) => {
                let function = decoded.unwrap_or_else(|error| {
                    panic!("function {index} of {} does not decode: {error}", self.source_name)
                });
                let entry = self.encoded_records.as_deref().and_then(|records| records.ranges.get(index).copied().flatten());
                let function = match entry {
                    Some(record) if record.unlowered() => self.lower_unlowered(index, function, record.rewrites_pending()),
                    _ => function,
                };
                if let (Some(action), Some(bytes)) = (
                    RECORD_READ.get(),
                    self.encoded_records.as_deref().and_then(|records| binary::deferred_record_bytes(records, index)),
                ) {
                    action(bytes);
                }
                function
            }
            None => {
                if !crate::DYNAMIC_EXECUTION_ENABLED {
                    panic!("native function {index} of {} has no archived record", self.source_name);
                }
                crate::compile::replay::recompile(self, index as u32)
                .unwrap_or_else(|error| {
                    panic!("function {index} of {} does not compile again: {error}", self.source_name)
                })
                .function
            },
        };
        // As `ResidualProgram::prime_loop_decoding` primes an installed program's records.
        function.decoded.prime_loop(&function.code, &function.wide);
        function
    }

    /// An unlowered record lowered against the finished program's tables, as its load
    /// would have lowered it (`compile::lower_settled`), with its register roots. Where
    /// `rewrites_pending`, the record's rewrites run first, as the load deferred them
    /// (`compile::settle_deferred_rewrite`): the rewrites come before lowering, as the load's
    /// own order has them.
    #[cold]
    #[inline(never)]
    fn lower_unlowered(&self, index: usize, mut function: Function, rewrites_pending: bool) -> Function {
        if rewrites_pending { crate::compile::settle_deferred_rewrite(self, index as u32, &mut function); }
        let undefined_constant = *self.undefined_constant.get_or_init(|| {
            let index = self.constants.iter().position(|constant| matches!(constant, Constant::Undefined))
                .expect("a program with an unlowered record holds the `undefined` constant");
            u32::try_from(index).expect("constant index fits u32")
        });
        let solved = crate::compile::lower_settled(
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
            &mut crate::compile::CompileScratch::default(),
        );
        function.register_roots = crate::compile::liveness::register_roots_from(&function, self, &self.field_sites, &self.superinstructions, solved);
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

    /// The registers an activation of `function` starts with its initial value (`entry_fill`):
    /// those below every register live at its entry, derived from this program's tables on the
    /// first ask.
    pub(crate) fn entry_fill(&self, function: &Function) -> u16 {
        function
            .register_roots
            .entry_live_with(|| crate::compile::liveness::derive_entry_live_keeping(function, self, &self.field_sites, &self.superinstructions))
            .min(function.registers)
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
            .caches()
            .register_masks
            .get_or_init(|| {
                // The masks the lowering or the entry bound's derivation solved, when kept, else a solve.
                let masks = function.register_roots.take_solved().unwrap_or_else(|| {
                    crate::compile::liveness::RegisterMasks::of(function, self, &self.field_sites, &self.superinstructions)
                        .expect("a covered function's liveness is derivable")
                });
                std::rc::Rc::new(masks)
            });
        Some(masks)
    }

    /// The byte offset in the kept source text of the entry in effect at `pc` of `function`.
    pub(crate) fn source_offset(&self, function: u32, pc: u32) -> Option<u32> {
        let offset = self.function_get(function as usize)?.source_positions.at(pc)?;
        self.source.as_ref()?.text_offset(offset)
    }

    /// The line and column of the entry in effect at `pc` of `function`.
    pub(crate) fn source_position(&self, function: u32, pc: u32) -> Option<SourcePosition> {
        let offset = self.function_get(function as usize)?.source_positions.at(pc)?;
        self.offset_position(offset)
    }

    /// The line and column of compiled-source byte `offset`, which the kept source text maps.
    pub(crate) fn offset_position(&self, offset: u32) -> Option<SourcePosition> {
        self.source.as_ref()?.position(offset)
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
mod detached;
pub(crate) use detached::DetachedProgram;
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
pub(crate) use flow_view::used_registers;
#[cfg(test)]
mod tests;
mod validate;
mod operands;
pub(crate) use operands::{ConstructArguments, FieldBase, FieldLookup, Operand, OperandKind, RegisterWindow, REGISTER_MASK, SET_THIS_REGISTER};
mod this_reads;
pub(crate) use this_reads::activation_reads_this;
mod written_once;

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
                    | Op::StoreName | Op::StoreResolvedName | Op::StoreVarBinding
                    | Op::LoadGlobalLexical | Op::StoreGlobalLexical => true,
                    Op::MakeClosure => reads(code, instruction.closure_function_index(), depth),
                    _ => reads_this_field(code, instruction),
                }
        })
    }
    reads(code, id, 8)
}

/// Whether `instruction` reads `this` through a field base (`FieldBase::THIS`): its field
/// lookup, or a field-site operand in one of its fields. An arrow's `this.x` compiles to a
/// field read of base `this` with no `LoadThis`, and that `this` is its maker's. A field that
/// is no operand but decodes as one only answers `true`, which keeps the closure on its
/// maker's environment.
fn reads_this_field(code: &ResidualProgram, instruction: WideInstruction) -> bool {
    let site_base_is_this = |index: usize| code.field_sites.get(index).is_some_and(|site| site.base == FieldBase::THIS);
    let lookup_reads_this = instruction.op().immediate_role() == ImmediateRole::FieldLookup
        && match instruction.field_lookup() {
            Some(FieldLookup::Atom { base, .. }) => base == FieldBase::THIS,
            Some(FieldLookup::Site(index)) => site_base_is_this(index),
            None => false,
        };
    lookup_reads_this
        || [instruction.a(), instruction.b(), instruction.c()].into_iter().any(|value| {
            let operand = Operand(value);
            operand.kind() == Some(OperandKind::Field) && site_base_is_this(usize::from(operand.payload()))
        })
}
