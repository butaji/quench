use crate::Value;
use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, NUMERIC_LOCAL_TARGET, Op, Operand,
    REGISTER_MASK, RETURN_REGISTER, Register, ResidualProgram, SET_THIS_REGISTER, WideInstruction,
};
use crate::heap::{Cell, FunctionKind, Heap, IteratorKind, Native, Object, RootId, TypedArrayKind};
use crate::host::Host;
use crate::profile::Profile;
use crate::value::number_to_u32;
use crate::value_vec::ValueVec;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
mod activation;
mod activation_lifecycle;
mod array;
mod array_buffer;
mod array_builtins;
mod array_group;
mod array_indexed;
mod array_modern;
mod atom_keys;
mod atomics;
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
mod error;
mod field_cache;
mod finalization;
mod gc;
mod generator;
mod index;
mod iterators;
mod json;
mod method_cache;
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
use activation::{Continuation, GeneratorRecord, SuspendedEntry};
use call_arguments::CallArguments;
use numeric_site::NumericSite;
use promise::PromiseRuntime;
mod operations;
mod primitives;
mod profile_edges;
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
    function: u32,
    pc: usize,
    env: Value,
    this: Value,
    locals: Vec<Value>,
    captured: bool,
    registers: Vec<Value>,
}
struct PendingJob {
    callback: Value,
    this: Value,
    args: Vec<Value>,
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
    owner: Value,
    owner_shape: u32,
    slot: u16,
    depth: u16,
}
const EMPTY_CACHE: FieldCache = FieldCache {
    receiver: u32::MAX,
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
const EMPTY_STRING_CONCAT_CACHE: StringConcatCache = StringConcatCache {
    left: Value::UNDEFINED,
    right: Value::UNDEFINED,
    result: Value::UNDEFINED,
};
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallTarget {
    User(u32, Value),
    NumericUser(u32, Value),
    Native(Native),
}
pub(super) enum StepResult {
    Continue,
    Return(Value),
    Await { value: Value, destination: Register },
    Yield { value: Value, destination: Register },
}

pub(super) enum FrameOutcome {
    Complete(Value),
    Await {
        value: Value,
        destination: Register,
        frame: Option<Frame>,
    },
    Yield {
        value: Value,
        destination: Register,
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
    proto: Value::UNDEFINED,
    target: None,
};
#[derive(Clone, Copy)]
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
    host: H,
    specialized: bool,
    heap: Heap,
    globals: Value,
    object_proto: Value,
    function_proto: Value,
    array_proto: Value,
    array_buffer_proto: Value,
    uint8_array_proto: Value,
    uint8_clamped_array_proto: Value,
    uint16_array_proto: Value,
    uint32_array_proto: Value,
    int8_array_proto: Value,
    int16_array_proto: Value,
    int32_array_proto: Value,
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
    regexp_proto: Value,
    constants: Vec<Value>,
    const_arrays: Vec<Option<Rc<Vec<Value>>>>,
    natives: Vec<(Native, Value)>,
    frames: Vec<Frame>,
    frame_pool: Vec<Frame>,
    jobs: Vec<PendingJob>,
    suspended: Vec<SuspendedEntry>,
    suspended_free: Vec<u32>,
    generators: FxHashMap<Value, GeneratorRecord>,
    promise: PromiseRuntime,
    profile: Profile,
    numeric_sites: FxHashMap<(u32, u32), NumericSite>,
    shapes: Vec<Vec<Atom>>,
    shape_slots: Vec<FxHashMap<Atom, u16>>,
    transitions: FxHashMap<(u32, Atom), u32>,
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
    to_fixed_atom: Atom,
    to_precision_atom: Atom,
    primitive_atoms: [Atom; 8],
    method_caches: Vec<[MethodCache; 2]>,
    megamorphic_methods: Vec<MethodCacheSet>,
    #[cfg(feature = "profile-aggregate")]
    invalidated_methods: FxHashMap<MethodCacheKey, InvalidatedMethod>,
    object_shapes: Vec<u32>,
    descriptors: FxHashMap<(Value, property_key::PropertyKey), PropertyAttributes>,
    symbol_properties: FxHashMap<(Value, property_key::PropertyKey), Value>,
    symbol_property_order: FxHashMap<Value, Vec<property_key::PropertyKey>>,
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
        self.jobs.push(PendingJob {
            callback,
            this: Value::UNDEFINED,
            args,
        });
    }
    pub fn release_root(&mut self, root: RootId) -> bool {
        self.heap.release_root(root)
    }
    pub fn execute(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        self.initialize(program)?;
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            self.report_memory("initialized");
        }
        let root = self.closure(program, 0, Value::NULL)?;
        let result = self.call_value(program, root, self.globals, &[]);
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
            .map(|shape| shape.capacity() * size_of::<Atom>())
            .sum();
        let max_shape_width = self.shapes.iter().map(Vec::len).max().unwrap_or(0);
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
        self.constants.clear();
        self.const_arrays.clear();
        self.natives.clear();
        self.frames.clear();
        self.frame_pool.clear();
        self.jobs.clear();
        self.suspended.clear();
        self.suspended_free.clear();
        self.generators.clear();
        self.promise = Default::default();
        self.numeric_sites.clear();
        self.shapes.truncate(1);
        self.shape_slots.truncate(1);
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
        self.to_fixed_atom = self.intern_atom("toFixed");
        self.to_precision_atom = self.intern_atom("toPrecision");
        self.primitive_atoms = [
            self.intern_atom("charCodeAt"),
            self.intern_atom("charAt"),
            self.intern_atom("substring"),
            self.intern_atom("substr"),
            self.intern_atom("toString"),
            self.intern_atom("includes"),
            self.intern_atom("startsWith"),
            self.intern_atom("endsWith"),
        ];
        self.method_caches = vec![[EMPTY_METHOD_CACHE; 2]; program.method_sites.len()];
        self.megamorphic_methods.clear();
        self.object_shapes = vec![u32::MAX; program.object_sites.len()];
        self.descriptors.clear();
        self.symbol_properties.clear();
        self.symbol_property_order.clear();
        self.finalization_registry_proto = Value::NULL;
        self.random_state = 0x4d59_5df4_d0f3_3173;
        self.globals = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
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
            self.constants.push(value);
        }
        self.const_arrays.resize(self.constants.len(), None);
        self.install_builtins(program)
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
        match self.call_target(callee)? {
            CallTarget::Native(native) => {
                self.profile.call_target(0, args.len());
                if native == Native::ProxyRevoke {
                    return self.proxy_revoke(callee);
                }
                self.call_native_guarded(p, native, this, args, callee)
            }
            CallTarget::User(id, env) => {
                self.profile.call_target(1, args.len());
                self.call_user_maybe_async(p, id, env, this, args)
            }
            CallTarget::NumericUser(id, env) => {
                self.profile.call_target(2, args.len());
                self.call_user_numeric(p, id, env, this, NumericArguments::Values(args))
            }
        }
    }
    fn call_target(&self, callee: Value) -> Result<CallTarget, JsError> {
        match self.heap.get(callee) {
            Some(Cell::Function {
                kind: FunctionKind::User(id),
                env,
                ..
            }) => Ok(CallTarget::User(*id, *env)),
            Some(Cell::Function {
                kind: FunctionKind::NumericUser(id),
                env,
                ..
            }) => Ok(CallTarget::NumericUser(*id, *env)),
            Some(Cell::Function {
                kind: FunctionKind::Native(native),
                ..
            }) => Ok(CallTarget::Native(*native)),
            other => self.non_callable_target(callee, other),
        }
    }
}
