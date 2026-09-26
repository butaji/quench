use crate::Value;
use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, Op, Operand, REGISTER_MASK,
    RETURN_REGISTER, Register, ResidualProgram, WideInstruction,
};
use crate::heap::{Cell, FunctionKind, Heap, IteratorKind, Native, Object, RootId, TypedArrayKind};
use crate::host::{CapabilityId, Host, HostContext};
use crate::profile::Profile;
use crate::value::number_to_u32;
use crate::value_vec::ValueVec;
use atomics::Test262AgentState;
use rustc_hash::{FxHashMap, FxHashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

const DEFAULT_RANDOM_SEED: u64 = 0x4d59_5df4_d0f3_3173;
pub(super) const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
pub(super) const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;
pub(super) const ROOT_FUNCTION_ID: u32 = 0;
pub(crate) mod activation;
mod activation_lifecycle;
mod agent;
mod arguments;
mod array;
mod array_buffer;
mod array_builtins;
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
mod environment;
mod equality;
mod error;
mod eval;
mod field_cache;
mod finalization;
mod function;
mod gc;
mod generator;
mod index;
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
mod string;
mod string_cache;
mod string_extra;
mod superinstruction;
mod symbol;
mod type_predicates;
mod typed_array;
mod typed_array_access;
mod typed_array_construct;
mod typed_array_float;
mod typed_array_install;
mod typed_array_signed;
mod typed_array_uint16;
mod vm_init;
pub(crate) mod wtf16;
pub use error::JsError;
use wtf16::JsString;
#[cfg(test)]
mod tests;
pub(super) struct Frame {
    program: ProgramId,
    function: u32,
    pc: usize,
    env: Value,
    this: Value,
    locals: Vec<Value>,
    dynamic_bindings: Vec<(Atom, Value)>,
    captured: bool,
    registers: Vec<Value>,
    active_iterators: Vec<ActiveIterator>,
    with_base: usize,
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
    jobs: Vec<PendingJob>,
    template_objects: FxHashMap<(ProgramId, u32, u32), Value>,
}
enum NumericArguments<'a> {
    Values(&'a [Value]),
    Registers {
        frame: usize,
        values: &'a [Register],
    },
}
#[derive(Clone, Copy)]
struct FieldCache {
    receiver: u32,
    atom: Atom,
    owner: Value,
    owner_shape: u32,
    slot: u16,
    depth: u16,
}
const EMPTY_CACHE: FieldCache = FieldCache {
    receiver: u32::MAX,
    atom: u32::MAX,
    owner: Value::NULL,
    owner_shape: u32::MAX,
    slot: 0,
    depth: 0,
};
const NO_MEGAMORPHIC_FIELD: u32 = u32::MAX;
const FIELD_MEGAMORPHIC_INLINE: usize = 4;
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
    target: Option<CallTarget>,
}
const METHOD_MEGAMORPHIC_LIMIT: usize = 8;
struct MethodCacheSet {
    site: u16,
    len: u8,
    entries: [MethodCache; METHOD_MEGAMORPHIC_LIMIT],
}
const STRING_CONCAT_CACHE_SIZE: usize = 64;
#[derive(Clone, Copy)]
struct StringConcatCache {
    left: Value,
    right: Value,
    result: Value,
}
#[derive(Clone)]
struct Shape {
    keys: Vec<property_key::PropertyKey>,
    slots: FxHashMap<property_key::PropertyKey, u32>,
    descriptors: Vec<PropertyAttributes>,
    storage_len: usize,
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
const EMPTY_METHOD_CACHE: MethodCache = MethodCache {
    shape: u32::MAX,
    atom: u32::MAX,
    proto: Value::UNDEFINED,
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
const DEFAULT_PROPERTY_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    writable: true,
    enumerable: true,
    configurable: true,
    accessor: false,
    getter: None,
    setter: None,
};
pub struct Vm<H> {
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
    uint8_array_proto: Value,
    uint8_clamped_array_proto: Value,
    uint16_array_proto: Value,
    uint32_array_proto: Value,
    int8_array_proto: Value,
    int16_array_proto: Value,
    int32_array_proto: Value,
    bigint64_array_proto: Value,
    biguint64_array_proto: Value,
    float32_array_proto: Value,
    float64_array_proto: Value,
    data_view_proto: Value,
    map_proto: Value,
    set_proto: Value,
    weak_map_proto: Value,
    weak_set_proto: Value,
    weak_ref_proto: Value,
    finalization_registry_proto: Value,
    iterator_proto: Value,
    generator_proto: Value,
    async_iterator_proto: Value,
    async_generator_proto: Value,
    async_from_sync_iterator_proto: Value,
    regexp_proto: Value,
    natives: Vec<(Native, Value)>,
    frames: Vec<Frame>,
    frame_pool: Vec<Frame>,
    with_stack: Vec<Value>,
    suspended: Vec<SuspendedEntry>,
    suspended_free: Vec<u32>,
    promise: PromiseRuntime,
    test262_agent: Test262AgentState,
    programs: ProgramStore,
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
    method_caches: Vec<[MethodCache; 2]>,
    megamorphic_methods: Vec<MethodCacheSet>,
    #[cfg(feature = "profile-aggregate")]
    invalidated_methods: FxHashMap<MethodCacheKey, InvalidatedMethod>,
    object_shapes: Vec<u32>,
    descriptors: FxHashMap<(Value, property_key::PropertyKey), PropertyAttributes>,
    // Closure identity cache is indexed by function id; each function keeps
    // the small set of captured environments it has materialized.
    function_values: FxHashMap<(ProgramId, u32), Vec<(Value, Value)>>,
    direct_eval: bool,
    parameter_eval: bool,
    eval_script_context: bool,
    deferred_dependency_batch: bool,
    construct_target: Option<Value>,
    random_state: u64,
}
impl<H: Host> Vm<H> {
    pub fn root(&mut self, value: Value) -> RootId {
        self.heap.root(value)
    }
    pub fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.heap.update_root(root, value)
    }
    pub fn root_value(&self, root: RootId) -> Option<Value> {
        self.heap.root_value(root)
    }
    pub(crate) fn enqueue_job(&mut self, callback: Value, args: Vec<Value>) {
        self.realm.jobs.push(PendingJob {
            callback,
            this: Value::UNDEFINED,
            args,
        });
    }
    pub fn release_root(&mut self, root: RootId) -> bool {
        self.heap.release_root(root)
    }
    pub(super) fn instantiate_global_declarations(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        if program.module {
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
            } else if attributes.accessor || !attributes.writable || !attributes.enumerable {
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
                            match self.heap.get(frame.env) {
                                Some(Cell::Environment { slots, .. }) => slots.get(slot).copied(),
                                _ => None,
                            }
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
    pub fn execute(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        self.initialize(program)?;
        if program.module {
            self.instantiate_main_module(program)?;
            self.evaluate_program_module_requests(program)?;
        }
        self.instantiate_global_declarations(program)?;
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            self.report_memory("initialized");
        }
        let root = self.closure(program, 0, Value::NULL)?;
        let this = if program.module {
            Value::UNDEFINED
        } else {
            self.realm.globals
        };
        let result = self.call_value(program, root, this, &[]);
        self.finish_main_module(program, &result)?;
        self.advance_dynamic_import_jobs(program, true)?;
        let jobs = self.drain_jobs(program);
        self.profile.report(&self.heap, program);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            self.report_memory("complete");
        }
        jobs?;
        result
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
        let shape_bytes: usize = self
            .shapes
            .iter()
            .map(|shape| shape.keys.capacity() * size_of::<property_key::PropertyKey>())
            .sum();
        let max_shape_width = self
            .shapes
            .iter()
            .map(|shape| shape.keys.len())
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
            "{{\"kind\":\"rqj-memory\",\"phase\":\"{phase}\",\"heap_slots\":{slots},\"slot_bytes\":{slot_bytes},\"free_bytes\":{free_bytes},\"cell_bytes\":{cell_bytes},\"cell_counts\":{cell_counts:?},\"property_values\":{property_values},\"property_capacity\":{property_capacity},\"property_free_ranges\":{property_free},\"live_property_values\":{live_property_values},\"live_property_capacity\":{live_property_capacity},\"array_elements\":{array_elements},\"array_capacity\":{array_capacity},\"shapes\":{},\"max_shape_width\":{max_shape_width},\"shape_bytes\":{shape_bytes},\"transitions\":{},\"transition_bytes\":{},\"frame_bytes\":{frame_bytes},\"field_cache_bytes\":{},\"megamorphic_field_sites\":{},\"megamorphic_field_entries\":{megamorphic_field_entries},\"max_megamorphic_field_entries\":{max_megamorphic_field_entries},\"method_cache_bytes\":{},\"megamorphic_method_sites\":{}}}",
            self.shapes.len(),
            self.transitions.len(),
            self.transitions.capacity() * (size_of::<(u32, Atom)>() + size_of::<u32>()),
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
        self.promise = Default::default();
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
            self.index_atom(Self::atom_hash(name), id as Atom);
        }
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
        self.programs.reset(program);
        self.active_program = ProgramId::MAIN;
        self.finalization_registry_proto = Value::NULL;
        self.random_state = DEFAULT_RANDOM_SEED;
        self.realm.globals = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        self.materialize_program_constants(ProgramId::MAIN, program);
        self.install_builtins(program)
    }
    fn materialize_program_constants(&mut self, id: ProgramId, program: &ResidualProgram) {
        let mut constants = Vec::with_capacity(program.constants.len());
        for constant in &program.constants {
            let value = match constant {
                Constant::Number(v) => Value::number(*v),
                Constant::String(v) => self.heap.alloc(Cell::String(v.clone().into())),
                Constant::StringUnits(v) => self.heap.alloc(Cell::String(JsString::from_units(v))),
                Constant::BigInt(v) => self.heap.alloc(Cell::BigInt(v.clone())),
                Constant::Boolean(true) => Value::TRUE,
                Constant::Boolean(false) => Value::FALSE,
                Constant::Null => Value::NULL,
                Constant::Undefined => Value::UNDEFINED,
            };
            constants.push(value);
        }
        self.programs.set_constants(id, constants);
    }
    pub(super) fn store_module_program(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.intern_program_atoms(&program);
        let id = self.programs.insert_module(program)?;
        let residual = self.programs.get(id)?;
        self.materialize_program_constants(id, &residual);
        Some(id)
    }
    pub(super) fn store_dynamic_program(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.intern_program_atoms(&program);
        let id = self.programs.insert(program)?;
        let residual = self.programs.get(id)?;
        self.materialize_program_constants(id, &residual);
        Some(id)
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
        if matches!(self.heap.get(callee), Some(Cell::Proxy { .. })) {
            return self.proxy_call(p, callee, this, args);
        }
        let target = self
            .call_target(callee)
            .map_err(|error| self.type_error(p, error.to_string()))?;
        match target {
            CallTarget::Native(native) => {
                self.profile.call_target(0, args.len());
                if native == Native::ProxyRevoke {
                    return self.proxy_revoke(callee);
                }
                self.call_native_guarded(p, native, this, args, callee)
            }
            CallTarget::User(program_id, id, env) => {
                self.profile.call_target(1, args.len());
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
                    self.call_user_maybe_async(&program, id, env, this, args)
                };
                self.active_program = active_program;
                self.realm.globals = current_global;
                result
            }
            CallTarget::NumericUser(program_id, id, env) => {
                self.profile.call_target(2, args.len());
                let program = self.programs.get(program_id).ok_or_else(|| {
                    self.type_error(p, "function belongs to an unavailable program".into())
                })?;
                let active_program = std::mem::replace(&mut self.active_program, program_id);
                let realm = match self.heap.get(callee) {
                    Some(Cell::Function { realm, .. }) => *realm,
                    _ => self.realm.globals,
                };
                let current_global = std::mem::replace(&mut self.realm.globals, realm);
                let result =
                    self.call_user_numeric(&program, id, env, this, NumericArguments::Values(args));
                self.active_program = active_program;
                self.realm.globals = current_global;
                result
            }
        }
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
        let outcome = self.call_user_construct_frame(&program, id, env, args);
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
