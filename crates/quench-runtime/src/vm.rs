mod wasm;
mod wasm_exception;
mod wasm_gc;
mod wasm_host;
mod wasm_table;
use crate::Value;
use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, LEXICAL_THIS_BINDING,
    NEW_TARGET_BINDING, Op, Operand, Register, ResidualProgram, WideInstruction,
};
use crate::heap::{
    CallSiteRecord, Cell, FunctionKind, Heap, IteratorConsumer, IteratorHelper, IteratorKind,
    Native, Object, ProxyKind, RootId, StackData, TypedArrayKind, WeakHandle,
};
use crate::host::{CapabilityId, Host, HostContext};
use crate::profile::Profile;
use crate::value::number_to_u32;
use crate::value_vec::ValueVec;
use activation::CallContext;
use atomics::Test262AgentState;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::OnceCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

const DEFAULT_RANDOM_SEED: u64 = 0x4d59_5df4_d0f3_3173;
pub(super) const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
pub(super) const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;
pub(super) const ROOT_FUNCTION_ID: u32 = 0;

#[derive(Clone, Copy, Default)]
struct RuntimeAtoms {
    lexical_this: Atom,
    new_target: Atom,
}
pub(crate) mod activation;
mod activation_lifecycle;
mod agent;
mod arguments;
mod array;
mod array_buffer;
mod array_builtins;
mod array_copy;
mod array_flatten;
mod array_group;
mod array_indexed;
mod array_modern;
mod atom_keys;
mod atomics;
mod bigint;
mod boolean;
mod builtins;
mod call_arguments;
mod coercion;
mod collections;
mod construction;
mod data_view;
mod date;
mod dispatch;
mod dispatch_frame;
mod dispatch_numeric;
mod dynamic_strings;
mod embedding;
mod environment;
mod equality;
mod error;
mod eval;
mod field_cache;
mod finalization;
mod function;
mod function_cache;
mod gc;
mod generator;
mod group_by;
mod host_function;
mod index;
mod intl_collator;
mod intl_datetime;
mod intl_datetime_parts;
mod intl_display_names;
mod intl_duration_format;
mod intl_list_format;
mod intl_namespace;
mod intl_number;
mod intl_plural_rules;
mod intl_relative;
mod intl_segmenter;
mod iterator_list;
mod local_time;
mod property_definition;
use group_by::GroupByKind;
use property_definition::PropertyDefinitionKind;
mod iterators;
mod json;
mod method_cache;
mod module;
mod number;
mod numeric_site;
mod object;
mod object_array;
mod object_builtins;
mod object_descriptors;
mod object_get;
mod object_integrity;
mod object_keys;
mod object_static;
mod object_symbols;
#[cfg(test)]
mod object_tests;
mod property_key;
use activation::{Continuation, SuspendedEntry};
use call_arguments::CallArguments;
use numeric_site::NumericSite;
use program_store::{ModuleImport, ProgramId, ProgramStore};
use promise::PromiseRuntime;
use property_key::PropertyKey;
mod operations;
mod primitives;
mod profile_edges;
pub(crate) mod program_store;
mod promise;
mod promise_aggregate;
mod promise_async;
mod promise_jobs;
mod promise_state;
mod proxy;
mod reflect;
mod regexp;
mod shadow_realm;
mod sort;
mod string;
mod string_cache;
mod string_extra;
mod structured_clone;
mod superinstruction;
mod symbol;
mod temporal;
mod temporal_date;
mod temporal_date_arithmetic;
mod temporal_date_difference;
mod temporal_date_parse;
mod temporal_date_projection;
mod temporal_date_time;
mod temporal_date_time_difference;
mod temporal_instant;
mod temporal_instant_difference;
mod temporal_instant_format;
mod temporal_instant_round;
mod temporal_plain_date_time_conversion;
mod temporal_plain_time;
mod temporal_zoned_date_time;
mod type_predicates;
mod typed_array;
mod typed_array_access;
mod typed_array_base64;
mod typed_array_construct;
mod typed_array_install;
mod vm_init;
pub(crate) mod wtf16;
pub use error::JsError;
use wtf16::JsString;
#[cfg(test)]
mod tests;
pub(super) struct Frame {
    // Actual callable identity is distinct from the code ID and captured environment.
    context: CallContext,
    original_arguments: Vec<Value>,
    program: ProgramId,
    function: u32,
    pc: usize,
    // The current operation's lexical projection, independent of the PC
    // published for GC roots or saved as the continuation after a call.
    binding_site_pc: Option<u32>,
    env: Value,
    this: Value,
    locals: Vec<Value>,
    dynamic_bindings: Vec<(Atom, Value)>,
    captured: bool,
    registers: Vec<Value>,
    active_iterators: Vec<ActiveIterator>,
    // Empty while active; owns the transferred scope stack while detached.
    with_objects: Vec<Value>,
    with_base: usize,
}
impl Frame {
    fn prepare_registers(&mut self, register_count: usize) {
        self.registers.resize(register_count, Value::UNDEFINED);
        self.registers.fill(Value::UNDEFINED);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveIterator {
    pub(crate) iterator: u16,
    pub(crate) done: u16,
}
struct PendingJob {
    callback: Value,
    this: Value,
    args: Vec<Value>,
}
struct Realm {
    globals: Value,
    global_lexical_declarations: FxHashSet<Atom>,
    global_lexical_bindings: FxHashMap<Atom, Value>,
    immutable_global_lexical_bindings: FxHashSet<Atom>,
    global_lexical_states: FxHashMap<Value, GlobalLexicalState>,
    intrinsics: RealmIntrinsics,
    jobs: Vec<PendingJob>,
    template_objects: FxHashMap<(ProgramId, u32, u32), Value>,
    promise: PromiseRuntime,
}
#[derive(Default)]
struct RealmIntrinsics {
    iterator_prototypes: FxHashMap<Value, IteratorRealmPrototypes>,
    builtin_prototypes: FxHashMap<(Value, Native), Value>,
    promise_constructors: FxHashMap<Value, Value>,
    regexp_intrinsics: FxHashMap<Value, regexp::RegExpIntrinsics>,
    intl_number_format_constructors: FxHashMap<Value, Value>,
    intl_number_format_prototypes: FxHashMap<Value, Value>,
    intl_number_format_fallback_symbols: FxHashMap<Value, Value>,
    intl_collator_constructors: FxHashMap<Value, Value>,
    intl_collator_prototypes: FxHashMap<Value, Value>,
    intl_plural_rules_prototypes: FxHashMap<Value, Value>,
    intl_datetime_format_constructors: FxHashMap<Value, Value>,
    intl_datetime_format_prototypes: FxHashMap<Value, Value>,
    intl_datetime_format_fallback_symbols: FxHashMap<Value, Value>,
    intl_display_names_constructors: FxHashMap<Value, Value>,
    intl_display_names_prototypes: FxHashMap<Value, Value>,
    intl_duration_format_constructors: FxHashMap<Value, Value>,
    intl_duration_format_prototypes: FxHashMap<Value, Value>,
    intl_list_format_constructors: FxHashMap<Value, Value>,
    intl_list_format_prototypes: FxHashMap<Value, Value>,
    intl_relative_time_format_prototypes: FxHashMap<Value, Value>,
    intl_segmenter_prototypes: FxHashMap<Value, Value>,
    intl_segment_iterator_prototypes: FxHashMap<Value, Value>,
    intl_segments_prototypes: FxHashMap<Value, Value>,
    intl_locale_prototypes: FxHashMap<Value, Value>,
}
#[derive(Default)]
struct GlobalLexicalState {
    declarations: FxHashSet<Atom>,
    bindings: FxHashMap<Atom, Value>,
    immutable_bindings: FxHashSet<Atom>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct FieldCache {
    receiver: u32,
    slot: u16,
}
const EMPTY_CACHE: FieldCache = FieldCache {
    receiver: u32::MAX,
    slot: 0,
};
const NO_MEGAMORPHIC_FIELD: u32 = u32::MAX;
const FIELD_MEGAMORPHIC_INLINE: usize = 4;
// Bound each site's overflow to 256 recorded shapes; unseen shapes keep the generic fallback.
const FIELD_MEGAMORPHIC_LIMIT: usize = 256;
struct FieldCacheSet {
    len: u8,
    entries: [FieldCache; FIELD_MEGAMORPHIC_INLINE],
    overflow: Option<Box<FxHashMap<u32, FieldCache>>>,
}
#[derive(Clone, Copy)]
struct MethodCache {
    shape: u32,
    atom: Atom,
    proto: Value,
    callee: Value,
    guard: MethodGuard,
    target: Option<CallTarget>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct MethodGuard {
    owner: Value,
    owner_shape: u32,
    slot: u16,
    depth: u16,
}
const METHOD_MEGAMORPHIC_LIMIT: usize = 8;
struct MethodCacheSet {
    site: usize,
    len: u8,
    entries: [MethodCache; METHOD_MEGAMORPHIC_LIMIT],
}
#[derive(Clone, Copy, Default)]
struct ProgramCacheLayout {
    field_base: usize,
    method_base: usize,
    object_base: usize,
}
const STRING_CONCAT_CACHE_SIZE: usize = 64;
#[derive(Clone, Copy)]
struct StringConcatCache {
    left: Value,
    right: Value,
    result: Value,
}
#[derive(Clone, Copy)]
enum ShapeTransition {
    Root,
    Add {
        key: property_key::PropertyKey,
        slot: u32,
    },
    Delete {
        key: property_key::PropertyKey,
        slot: u32,
    },
    Vacant,
    Descriptor {
        slot: u32,
        attributes: PropertyAttributes,
    },
    Dictionary {
        trigger: DictionaryTrigger,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DictionaryTrigger {
    PropertyCount,
    DeletionPattern,
    PrototypeUse,
}
impl DictionaryTrigger {
    #[cfg(feature = "profile-aggregate")]
    pub(crate) const COUNT: usize = 3;

    #[cfg(feature = "profile-aggregate")]
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::PropertyCount => 0,
            Self::DeletionPattern => 1,
            Self::PrototypeUse => 2,
        }
    }
}
struct ShapeLookupIndex {
    entries: Vec<(property_key::PropertyKey, u32)>,
    slots: FxHashMap<property_key::PropertyKey, u32>,
    attributes: FxHashMap<u32, PropertyAttributes>,
}
#[cfg(feature = "profile-memory")]
impl ShapeLookupIndex {
    fn payload_capacity_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<(property_key::PropertyKey, u32)>()
            + self.slots.capacity() * size_of::<(property_key::PropertyKey, u32)>()
            + self.attributes.capacity() * size_of::<(u32, PropertyAttributes)>()
    }
}
struct Shape {
    parent: Option<u32>,
    transition: ShapeTransition,
    storage_len: usize,
    dictionary_trigger: Option<DictionaryTrigger>,
    lookup_index: OnceCell<Box<ShapeLookupIndex>>,
}
impl Shape {
    fn root() -> Self {
        Self::child(None, ShapeTransition::Root, 0, None)
    }

    fn child(
        parent: Option<u32>,
        transition: ShapeTransition,
        storage_len: usize,
        dictionary_trigger: Option<DictionaryTrigger>,
    ) -> Self {
        Self {
            parent,
            transition,
            storage_len,
            dictionary_trigger,
            lookup_index: OnceCell::new(),
        }
    }
}
const EMPTY_STRING_CONCAT_CACHE: StringConcatCache = StringConcatCache {
    left: Value::UNDEFINED,
    right: Value::UNDEFINED,
    result: Value::UNDEFINED,
};
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallTarget {
    User(ProgramId, u32, Value),
    NumericUser(ProgramId, u32, Value),
    Native(Native),
}
pub(super) enum StepResult {
    Continue,
    TailCall,
    Return(Value),
    Await {
        value: Value,
        destination: Register,
    },
    Yield {
        value: Value,
        destination: Register,
        delegated_result: Option<Value>,
    },
}
pub(super) enum FrameOutcome {
    Complete(Value),
    ConstructComplete {
        value: Value,
        this: Value,
    },
    ParameterInitializationComplete,
    Await {
        value: Value,
        destination: Register,
        frame: Option<Frame>,
    },
    Yield {
        value: Value,
        destination: Register,
        delegated_result: Option<Value>,
        frame: Option<Frame>,
    },
}
#[cfg(feature = "profile-aggregate")]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MethodCacheKey {
    site: usize,
    shape: u32,
    proto: Value,
}
#[cfg(feature = "profile-aggregate")]
#[derive(Clone, Copy)]
struct InvalidatedMethod {
    target: CallTarget,
    reason: u8,
}
const EMPTY_METHOD_GUARD: MethodGuard = MethodGuard {
    owner: Value::NULL,
    owner_shape: u32::MAX,
    slot: 0,
    depth: 0,
};
const EMPTY_METHOD_CACHE: MethodCache = MethodCache {
    shape: u32::MAX,
    atom: u32::MAX,
    proto: Value::UNDEFINED,
    callee: Value::NULL,
    guard: EMPTY_METHOD_GUARD,
    target: None,
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct PropertyAttributes {
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
    pub accessor: bool,
    pub getter: Option<Value>,
    pub setter: Option<Value>,
}

impl PropertyAttributes {
    /// CanDeclareGlobalFunction accepts a configurable property, or an
    /// enumerable, writable data property when it cannot be reconfigured.
    fn permits_global_function_declaration(self) -> bool {
        self.configurable || (!self.accessor && self.writable && self.enumerable)
    }
}

const DEFAULT_PROPERTY_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    writable: true,
    enumerable: true,
    configurable: true,
    accessor: false,
    getter: None,
    setter: None,
};
pub(crate) struct Vm<H> {
    pub(crate) host: H,
    specialized: bool,
    heap: Heap,
    realm: Realm,
    object_proto: Value,
    function_proto: Value,
    array_proto: Value,
    string_proto: Value,
    array_buffer_proto: Value,
    shared_array_buffer_proto: Value,
    array_iterator_proto: Value,
    typed_array_proto: Value,
    uint8_array_proto: Value,
    uint8_clamped_array_proto: Value,
    uint16_array_proto: Value,
    uint32_array_proto: Value,
    int8_array_proto: Value,
    int16_array_proto: Value,
    int32_array_proto: Value,
    bigint64_array_proto: Value,
    biguint64_array_proto: Value,
    float16_array_proto: Value,
    float32_array_proto: Value,
    float64_array_proto: Value,
    data_view_proto: Value,
    map_proto: Value,
    set_proto: Value,
    shadow_realm_proto: Value,
    map_iterator_proto: Value,
    set_iterator_proto: Value,
    weak_map_proto: Value,
    weak_set_proto: Value,
    weak_ref_proto: Value,
    finalization_registry_proto: Value,
    iterator_proto: Value,
    string_iterator_proto: Value,
    regexp_string_iterator_proto: Value,
    generator_proto: Value,
    iterator_helper_proto: Value,
    wrap_for_valid_iterator_proto: Value,
    async_iterator_proto: Value,
    async_generator_proto: Value,
    async_from_sync_iterator_proto: Value,
    regexp_proto: Value,
    temporal_plain_date_proto: Value,
    temporal_plain_time_proto: Value,
    temporal_plain_month_day_proto: Value,
    temporal_plain_year_month_proto: Value,
    natives: Vec<(Native, Value)>,
    frames: Vec<Frame>,
    frame_pool: Vec<Frame>,
    active_call_roots: Vec<Value>,
    with_stack: Vec<Value>,
    suspended: Vec<SuspendedEntry>,
    suspended_free: Vec<u32>,
    test262_agent: Test262AgentState,
    programs: ProgramStore,
    program_cache_layouts: Vec<ProgramCacheLayout>,
    active_program: ProgramId,
    profile: Profile,
    numeric_sites: FxHashMap<(u32, u32), NumericSite>,
    shapes: Vec<Shape>,
    transitions: FxHashMap<(u32, property_key::PropertyKey), u32>,
    atom_text: AtomTable,
    atoms: FxHashMap<u64, Atom>,
    atom_collisions: FxHashMap<u64, Vec<Atom>>,
    dynamic_atoms: Vec<JsString>,
    dynamic_strings: Option<Box<FxHashMap<u64, Value>>>,
    symbol_registry: FxHashMap<String, Value>,
    well_known_symbols: FxHashMap<String, Value>,
    string_concats: Option<Box<[StringConcatCache]>>,
    field_caches: Vec<FieldCache>,
    megamorphic_field_indices: Vec<u32>,
    megamorphic_fields: Vec<FieldCacheSet>,
    length_atom: Atom,
    size_atom: Atom,
    byte_length_atom: Atom,
    byte_offset_atom: Atom,
    buffer_atom: Atom,
    to_fixed_atom: Atom,
    to_precision_atom: Atom,
    runtime_atoms: RuntimeAtoms,
    method_caches: Vec<[MethodCache; 2]>,
    megamorphic_methods: Vec<MethodCacheSet>,
    #[cfg(feature = "profile-aggregate")]
    invalidated_methods: FxHashMap<MethodCacheKey, InvalidatedMethod>,
    object_shapes: Vec<u32>,
    descriptors: FxHashMap<(Value, property_key::PropertyKey), PropertyAttributes>,
    // Weak identities; captured environments remain authoritative in function cells.
    function_values: FxHashMap<(ProgramId, u32), Vec<WeakHandle>>,
    direct_eval: bool,
    direct_eval_var_program: Option<ProgramId>,
    parameter_eval: bool,
    eval_script_context: bool,
    deferred_dependency_batch: bool,
    construct_target: Option<Value>,
    random_state: u64,
}

#[derive(Clone, Copy)]
struct IteratorRealmPrototypes {
    helper: Value,
    wrapper: Value,
    generator: Value,
    async_generator: Value,
}
impl<H: Host> Vm<H> {
    pub(super) fn switch_realm_global(&mut self, global: Value) -> Value {
        let previous = self.realm.globals;
        if previous == global {
            return previous;
        }
        self.realm.global_lexical_states.insert(
            previous,
            GlobalLexicalState {
                declarations: std::mem::take(&mut self.realm.global_lexical_declarations),
                bindings: std::mem::take(&mut self.realm.global_lexical_bindings),
                immutable_bindings: std::mem::take(
                    &mut self.realm.immutable_global_lexical_bindings,
                ),
            },
        );
        let state = self
            .realm
            .global_lexical_states
            .remove(&global)
            .unwrap_or_default();
        self.realm.globals = global;
        self.realm.global_lexical_declarations = state.declarations;
        self.realm.global_lexical_bindings = state.bindings;
        self.realm.immutable_global_lexical_bindings = state.immutable_bindings;
        previous
    }

    pub(crate) fn root(&mut self, value: Value) -> RootId {
        self.heap.root(value)
    }
    pub(crate) fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.heap.update_root(root, value)
    }
    pub(crate) fn root_value(&self, root: RootId) -> Option<Value> {
        self.heap.root_value(root)
    }
    pub(crate) fn enqueue_job(&mut self, callback: Value, args: Vec<Value>) {
        self.realm.jobs.push(PendingJob {
            callback,
            this: Value::UNDEFINED,
            args,
        });
    }
    pub(crate) fn release_root(&mut self, root: RootId) -> bool {
        self.heap.release_root(root)
    }
    pub(super) fn instantiate_global_declarations(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        if program.is_module() {
            return Ok(());
        }
        let Some(root) = program.functions.first() else {
            return Ok(());
        };
        for atom in root.global_lexical_atoms.iter().copied() {
            if self.realm.global_lexical_declarations.contains(&atom) {
                return self
                    .syntax_error_result(program, "global lexical declaration already exists")
                    .map(|_| ());
            }
            let key = PropertyKey::string(atom);
            if self.own_property(self.realm.globals, atom).is_some()
                && self
                    .property_attributes(self.realm.globals, key)
                    .is_some_and(|attributes| !attributes.configurable)
            {
                return self
                    .syntax_error_result(
                        program,
                        "global lexical declaration conflicts with restricted property",
                    )
                    .map(|_| ());
            }
            if self.eval_script_context {
                self.realm
                    .global_lexical_bindings
                    .insert(atom, Value::DELETED);
            }
        }
        self.realm
            .global_lexical_declarations
            .extend(root.global_lexical_atoms.iter().copied());
        if self.eval_script_context {
            self.realm.immutable_global_lexical_bindings.extend(
                root.global_immutable_atoms
                    .iter()
                    .copied()
                    .filter(|atom| root.global_lexical_atoms.contains(atom)),
            );
        }
        if root
            .global_var_atoms
            .iter()
            .any(|atom| self.realm.global_lexical_declarations.contains(atom))
        {
            return self
                .syntax_error_result(
                    program,
                    "global var declaration conflicts with lexical binding",
                )
                .map(|_| ());
        }
        for atom in root.global_function_atoms.iter().copied() {
            let key = PropertyKey::string(atom);
            let Some(attributes) = self.property_attributes(self.realm.globals, key) else {
                if self
                    .object_data(self.realm.globals)
                    .is_some_and(|object| !object.is_extensible())
                {
                    return Err(self.type_error(program, "cannot declare global function".into()));
                }
                self.set_property(self.realm.globals, atom, Value::UNDEFINED)?;
                self.set_property_attributes(
                    self.realm.globals,
                    key,
                    PropertyAttributes {
                        writable: true,
                        enumerable: true,
                        configurable: false,
                        accessor: false,
                        getter: None,
                        setter: None,
                    },
                );
                continue;
            };
            if attributes.configurable {
                self.set_property_attributes(
                    self.realm.globals,
                    key,
                    PropertyAttributes {
                        writable: true,
                        enumerable: true,
                        configurable: false,
                        accessor: false,
                        getter: None,
                        setter: None,
                    },
                );
            } else if !attributes.permits_global_function_declaration() {
                return Err(self.type_error(program, "cannot declare global function".into()));
            }
        }
        for atom in root.global_var_atoms.iter().copied() {
            if self.own_property(self.realm.globals, atom).is_some() {
                continue;
            }
            if self
                .object_data(self.realm.globals)
                .is_some_and(|object| !object.is_extensible())
            {
                return Err(self.type_error(program, "cannot declare global var".into()));
            }
            self.set_property(self.realm.globals, atom, Value::UNDEFINED)?;
            self.set_property_attributes(
                self.realm.globals,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: true,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        Ok(())
    }
    pub(super) fn mirror_global_lexical_binding(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
        slot: usize,
        value: Value,
    ) {
        if !self.eval_script_context {
            return;
        }
        let Some(current) = self.frames.get(frame) else {
            return;
        };
        if current.function != ROOT_FUNCTION_ID {
            return;
        }
        let Some(atom) = program.functions[ROOT_FUNCTION_ID as usize]
            .local_atoms
            .get(slot)
            .copied()
        else {
            return;
        };
        if program.functions[ROOT_FUNCTION_ID as usize]
            .global_lexical_atoms
            .contains(&atom)
        {
            self.realm.global_lexical_bindings.insert(atom, value);
        }
    }
    pub(super) fn persist_global_lexical_bindings(
        &mut self,
        program: &ResidualProgram,
        frame: &Frame,
    ) {
        if !self.eval_script_context || frame.function != ROOT_FUNCTION_ID {
            return;
        }
        let metadata = &program.functions[ROOT_FUNCTION_ID as usize];
        let bindings = metadata
            .global_lexical_atoms
            .iter()
            .map(|atom| {
                let value = metadata
                    .local_atoms
                    .iter()
                    .position(|candidate| candidate == atom)
                    .and_then(|slot| {
                        if frame.captured {
                            self.heap.environment_slot(frame.env, slot)
                        } else {
                            frame.locals.get(slot).copied()
                        }
                    })
                    .unwrap_or(Value::DELETED);
                (
                    *atom,
                    if value.is_deleted() {
                        Value::UNDEFINED
                    } else {
                        value
                    },
                )
            })
            .collect::<Vec<_>>();
        self.realm.global_lexical_bindings.extend(bindings);
    }
    pub(crate) fn execute(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        self.execute_with_job_drain(program, true)
    }

    pub(crate) fn execute_deferred_jobs(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<Value, JsError> {
        self.execute_with_job_drain(program, false)
    }

    fn execute_with_job_drain(
        &mut self,
        program: &ResidualProgram,
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        if program.functions.is_empty() {
            return Err(JsError::validation("program has no entry function".into()));
        }
        self.initialize(program)?;
        if program.is_module() {
            self.instantiate_main_module(program)?;
            self.evaluate_program_module_requests(program)?;
        }
        self.instantiate_global_declarations(program)?;
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            self.report_memory("initialized");
        }
        let root = self.closure(program, 0, Value::NULL)?;
        let this = if program.is_module() {
            Value::UNDEFINED
        } else {
            self.realm.globals
        };
        let result = self.call_value(program, root, this, &[]);
        self.finish_main_module(program, &result)?;
        if drain_jobs {
            self.advance_dynamic_import_jobs(program, true)?;
            let jobs = self.drain_jobs(program);
            self.report_execution(program);
            jobs?;
        }
        result
    }

    pub(crate) fn finish_deferred_execution(&mut self, program: &ResidualProgram) {
        self.report_execution(program);
    }

    fn report_execution(&mut self, program: &ResidualProgram) {
        self.profile.report(&self.heap, program);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            self.report_memory("complete");
        }
    }
    #[cfg(feature = "profile-memory")]
    fn report_memory(&self, phase: &str) {
        let (
            slots,
            slot_bytes,
            free_bytes,
            cell_bytes,
            property_values,
            property_capacity,
            property_free,
        ) = self.heap.memory_stats();
        let shape_bytes = self.shapes.capacity() * size_of::<Shape>();
        let shape_lookup_index_payload_bytes = self
            .shapes
            .iter()
            .filter_map(|shape| shape.lookup_index.get())
            .map(|index| index.payload_capacity_bytes())
            .sum::<usize>();
        let max_shape_width = self
            .shapes
            .iter()
            .map(|shape| shape.storage_len)
            .max()
            .unwrap_or(0);
        let cell_counts = self.heap.cell_counts();
        let live_payload_bytes = self.heap.live_payload_bytes();
        let (live_property_values, live_property_capacity) = self.heap.live_property_stats();
        let (array_elements, array_capacity) = self.heap.array_element_stats();
        let megamorphic_field_entries: usize =
            self.megamorphic_fields.iter().map(FieldCacheSet::len).sum();
        let max_megamorphic_field_entries = self
            .megamorphic_fields
            .iter()
            .map(FieldCacheSet::len)
            .max()
            .unwrap_or(0);
        let frame_bytes: usize = self
            .frames
            .iter()
            .chain(&self.frame_pool)
            .map(|frame| {
                frame.locals.capacity() * size_of::<Value>()
                    + frame.registers.capacity() * size_of::<Value>()
            })
            .sum();
        eprintln!(
            "{{\"kind\":\"quench-memory\",\"phase\":\"{phase}\",\"heap_slots\":{slots},\"slot_bytes\":{slot_bytes},\"free_bytes\":{free_bytes},\"cell_bytes\":{cell_bytes},\"cell_counts\":{cell_counts:?},\"property_values\":{property_values},\"property_capacity\":{property_capacity},\"property_free_ranges\":{property_free},\"live_property_values\":{live_property_values},\"live_property_capacity\":{live_property_capacity},\"array_elements\":{array_elements},\"array_capacity\":{array_capacity},\"shapes\":{},\"shape_capacity\":{},\"max_shape_width\":{max_shape_width},\"shape_bytes\":{shape_bytes},\"shape_lookup_index_payload_bytes\":{shape_lookup_index_payload_bytes},\"transitions\":{},\"transition_bytes\":{},\"frame_bytes\":{frame_bytes},\"field_cache_bytes\":{},\"megamorphic_field_sites\":{},\"megamorphic_field_entries\":{megamorphic_field_entries},\"max_megamorphic_field_entries\":{max_megamorphic_field_entries},\"method_cache_bytes\":{},\"megamorphic_method_sites\":{}}}",
            self.shapes.len(),
            self.shapes.capacity(),
            self.transitions.len(),
            self.transitions.capacity() * size_of::<((u32, property_key::PropertyKey), u32)>(),
            self.field_caches.capacity() * size_of::<FieldCache>()
                + self.megamorphic_field_indices.capacity() * size_of::<u32>(),
            self.megamorphic_fields.len(),
            self.method_caches.capacity() * size_of::<[MethodCache; 2]>(),
            self.megamorphic_methods.len(),
        );
        self.heap
            .memory_profile()
            .report(phase, cell_counts, live_payload_bytes);
        crate::report_allocator_memory(phase);
    }
    fn initialize(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.initialize_shared(&Rc::new(program.clone()))
    }
    fn initialize_shared(&mut self, program: &Rc<ResidualProgram>) -> Result<(), JsError> {
        self.specialized = program.specialized;
        self.heap.reset();
        self.natives.clear();
        self.frames.clear();
        self.frame_pool.clear();
        self.realm.jobs.clear();
        self.realm.global_lexical_declarations.clear();
        self.realm.global_lexical_bindings.clear();
        self.realm.immutable_global_lexical_bindings.clear();
        self.with_stack.clear();
        self.suspended.clear();
        self.suspended_free.clear();
        self.direct_eval = false;
        self.parameter_eval = false;
        self.eval_script_context = false;
        self.construct_target = None;
        self.realm.promise = Default::default();
        self.numeric_sites.clear();
        self.shapes.truncate(1);
        self.transitions.clear();
        self.atom_text = program.atoms.clone();
        self.atoms.clear();
        self.atom_collisions.clear();
        self.dynamic_atoms.clear();
        self.dynamic_strings = None;
        self.symbol_registry.clear();
        self.well_known_symbols.clear();
        self.string_concats = None;
        let atom_text = self.atom_text.clone();
        for (id, name) in atom_text.iter().enumerate() {
            self.index_atom(Self::atom_hash_str(name), id as Atom);
        }
        let lexical_this = self.intern_atom(LEXICAL_THIS_BINDING);
        let new_target = self.intern_atom(NEW_TARGET_BINDING);
        self.runtime_atoms = RuntimeAtoms {
            lexical_this,
            new_target,
        };
        self.field_caches = vec![EMPTY_CACHE; program.cache_sites as usize];
        self.megamorphic_field_indices = vec![NO_MEGAMORPHIC_FIELD; program.cache_sites as usize];
        self.megamorphic_fields.clear();
        self.length_atom = self.intern_atom("length");
        self.size_atom = self.intern_atom("size");
        self.byte_length_atom = self.intern_atom("byteLength");
        self.byte_offset_atom = self.intern_atom("byteOffset");
        self.buffer_atom = self.intern_atom("buffer");
        self.to_fixed_atom = self.intern_atom("toFixed");
        self.to_precision_atom = self.intern_atom("toPrecision");
        self.method_caches = vec![[EMPTY_METHOD_CACHE; 2]; program.method_sites.len()];
        self.megamorphic_methods.clear();
        self.object_shapes = vec![u32::MAX; program.object_sites.len()];
        self.descriptors.clear();
        self.function_values.clear();
        self.programs.reset(program.clone());
        self.program_cache_layouts.clear();
        self.program_cache_layouts
            .push(ProgramCacheLayout::default());
        self.active_program = ProgramId::MAIN;
        self.typed_array_proto = Value::NULL;
        self.finalization_registry_proto = Value::NULL;
        self.temporal_plain_date_proto = Value::NULL;
        self.temporal_plain_time_proto = Value::NULL;
        self.temporal_plain_month_day_proto = Value::NULL;
        self.temporal_plain_year_month_proto = Value::NULL;
        self.random_state = DEFAULT_RANDOM_SEED;
        self.realm.globals = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        self.materialize_program_constants(ProgramId::MAIN, program);
        self.install_builtins(program)?;
        self.initialize_host(program)
    }
    fn materialize_constant(&mut self, constant: &Constant) -> Value {
        match constant {
            Constant::Number(v) => Value::number(*v),
            Constant::WasmBits64(bits) => self.heap.alloc(Cell::WasmBits64(*bits)),
            Constant::WasmV128(bits) => self.heap.alloc(Cell::WasmV128(*bits)),
            Constant::String(v) => self.heap.alloc(Cell::String(v.clone().into())),
            Constant::StringUnits(v) => self.heap.alloc(Cell::String(JsString::from_units(v))),
            Constant::BigInt(v) => self.heap.alloc(Cell::BigInt(v.clone())),
            Constant::Boolean(true) => Value::TRUE,
            Constant::Boolean(false) => Value::FALSE,
            Constant::Null => Value::NULL,
            Constant::Undefined => Value::UNDEFINED,
        }
    }
    fn materialize_program_constants(&mut self, id: ProgramId, program: &ResidualProgram) {
        let mut constants = Vec::with_capacity(program.constants.len());
        for constant in &program.constants {
            let value = self.materialize_constant(constant);
            constants.push(value);
        }
        self.programs.set_constants(id, constants);
    }
    pub(super) fn store_module_program(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.intern_program_atoms(&program);
        let id = self.programs.insert_module(program)?;
        let residual = self.programs.get(id)?;
        self.append_program_cache_layout(id, &residual);
        self.materialize_program_constants(id, &residual);
        Some(id)
    }
    pub(super) fn store_dynamic_program(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.intern_program_atoms(&program);
        let id = self.programs.insert(program)?;
        let residual = self.programs.get(id)?;
        self.append_program_cache_layout(id, &residual);
        self.materialize_program_constants(id, &residual);
        Some(id)
    }
    fn append_program_cache_layout(&mut self, id: ProgramId, program: &ResidualProgram) {
        debug_assert_eq!(id.raw() as usize, self.program_cache_layouts.len());
        self.program_cache_layouts.push(ProgramCacheLayout {
            field_base: self.field_caches.len(),
            method_base: self.method_caches.len(),
            object_base: self.object_shapes.len(),
        });
        self.field_caches.resize(
            self.field_caches.len() + usize::from(program.cache_sites),
            EMPTY_CACHE,
        );
        self.megamorphic_field_indices.resize(
            self.megamorphic_field_indices.len() + usize::from(program.cache_sites),
            NO_MEGAMORPHIC_FIELD,
        );
        self.method_caches.resize(
            self.method_caches.len() + program.method_sites.len(),
            [EMPTY_METHOD_CACHE; 2],
        );
        self.object_shapes.resize(
            self.object_shapes.len() + program.object_sites.len(),
            u32::MAX,
        );
    }
    fn active_cache_layout(&self) -> ProgramCacheLayout {
        self.program_cache_layouts[self.active_program.raw() as usize]
    }
    pub(super) fn field_cache_index(&self, site: u16) -> usize {
        self.active_cache_layout().field_base + usize::from(site)
    }
    pub(super) fn method_cache_index(&self, site: usize) -> usize {
        self.active_cache_layout().method_base + site
    }
    pub(super) fn object_cache_index(&self, site: usize) -> usize {
        self.active_cache_layout().object_base + site
    }
    fn intern_program_atoms(&mut self, program: &ResidualProgram) {
        let first_new_atom = self.atom_text.len() + self.dynamic_atoms.len();
        for (index, name) in program.atoms.iter().enumerate().skip(first_new_atom) {
            let atom = self.intern_atom(name);
            debug_assert_eq!(atom as usize, index);
        }
    }
    fn call_value(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.call_value_with_target(p, callee, this, args, None)
    }

    fn call_value_with_target(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        this: Value,
        args: &[Value],
        target: Option<CallTarget>,
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(callee)
                .chain(std::iter::once(this))
                .chain(args.iter().copied()),
            |vm| vm.call_value_with_target_inner(p, callee, this, args, target),
        )
    }

    fn call_value_from_frame(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.call_value_with_target_inner(p, callee, this, args, None)
    }

    fn call_value_with_target_from_frame(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        this: Value,
        args: &[Value],
        target: Option<CallTarget>,
    ) -> Result<Value, JsError> {
        // The receiver and arguments remain in the paused caller frame, whose
        // root map is published at this call instruction. A method getter can
        // produce a callee that is not present in that frame, so root it here.
        self.with_call_roots([callee], |vm| {
            vm.call_value_with_target_inner(p, callee, this, args, target)
        })
    }

    fn call_value_with_target_inner(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        this: Value,
        args: &[Value],
        target: Option<CallTarget>,
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(callee), Some(Cell::Proxy { .. })) {
            return self.proxy_call(p, callee, this, args);
        }
        let target = match target {
            Some(target) => target,
            None => self
                .call_target(callee)
                .map_err(|error| self.type_error(p, error.to_string()))?,
        };
        match target {
            CallTarget::Native(native) => {
                self.profile.call_target(0, args.len());
                self.call_native_guarded(p, native, this, args, callee)
            }
            CallTarget::User(program_id, id, env)
            | CallTarget::NumericUser(program_id, id, env) => {
                let target_kind = if matches!(target, CallTarget::User(..)) {
                    1
                } else {
                    2
                };
                self.profile.call_target(target_kind, args.len());
                let program = self.programs.get(program_id).ok_or_else(|| {
                    self.type_error(p, "function belongs to an unavailable program".into())
                })?;
                let active_program = std::mem::replace(&mut self.active_program, program_id);
                let realm = match self.heap.get(callee) {
                    Some(Cell::Function { realm, .. }) => *realm,
                    _ => self.realm.globals,
                };
                let current_global = std::mem::replace(&mut self.realm.globals, realm);
                let result = if program
                    .functions
                    .get(id as usize)
                    .is_some_and(|function| function.is_class_constructor)
                    && self.construct_target.is_none()
                {
                    Err(self.type_error(p, "class constructor cannot be called without new".into()))
                } else {
                    self.call_user_maybe_async(
                        &program,
                        id,
                        env,
                        this,
                        args,
                        CallContext::user_function(id, callee),
                    )
                };
                self.active_program = active_program;
                self.realm.globals = current_global;
                result
            }
        }
    }

    fn with_call_roots<R>(
        &mut self,
        values: impl IntoIterator<Item = Value>,
        call: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let start = self.active_call_roots.len();
        self.active_call_roots.extend(values);
        let result = call(self);
        self.active_call_roots.truncate(start);
        result
    }

    pub(super) fn call_user_for_construct(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        args: &[Value],
    ) -> Result<(Value, Value), JsError> {
        let (program_id, id, env, realm) = match self.heap.get(callee) {
            Some(Cell::Function {
                kind: FunctionKind::User(program_id, id) | FunctionKind::NumericUser(program_id, id),
                env,
                realm,
                ..
            }) => (*program_id, *id, *env, *realm),
            _ => return Err(self.type_error(p, "constructor is not a user function".into())),
        };
        let program = self.programs.get(program_id).ok_or_else(|| {
            self.type_error(p, "function belongs to an unavailable program".into())
        })?;
        let previous_program = std::mem::replace(&mut self.active_program, program_id);
        let previous_global = std::mem::replace(&mut self.realm.globals, realm);
        let outcome = self.call_user_construct_frame(
            &program,
            id,
            env,
            args,
            CallContext::user_function(id, callee),
        );
        self.active_program = previous_program;
        self.realm.globals = previous_global;
        match outcome? {
            FrameOutcome::ConstructComplete { value, this } => Ok((value, this)),
            _ => Err(JsError(
                "constructor activation did not complete synchronously".into(),
            )),
        }
    }
}
