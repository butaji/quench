use crate::Value;
use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, NUMERIC_LOCAL_TARGET, Op, Operand,
    REGISTER_MASK, RETURN_REGISTER, Register, ResidualProgram, SET_THIS_REGISTER,
};
use crate::heap::{Cell, FunctionKind, Heap, IteratorKind, Native, Object, RootId, TypedArrayKind};
use crate::host::Host;
use crate::profile::Profile;
use crate::value::number_to_u32;
use crate::value_vec::ValueVec;
use rustc_hash::FxHashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
mod array;
mod array_buffer;
mod array_builtins;
mod array_group;
mod array_indexed;
mod array_modern;
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
mod environment;
mod field_cache;
mod gc;
mod index;
mod iterators;
mod json;
mod method_cache;
mod number;
mod numeric_site;
mod object;
mod object_builtins;
mod object_descriptors;
mod object_get;
mod object_integrity;
mod object_static;
#[cfg(test)]
mod object_tests;
use call_arguments::CallArguments;
use numeric_site::NumericSite;
mod operations;
mod primitives;
mod profile_edges;
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
#[derive(Debug)]
pub struct JsError(ErrorMessage);
#[derive(Debug)]
struct ErrorMessage {
    payload: Box<ErrorPayload>,
}
#[derive(Debug)]
struct ErrorPayload {
    text: String,
    thrown: Option<Value>,
}
impl From<&str> for ErrorMessage {
    fn from(value: &str) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                text: value.into(),
                thrown: None,
            }),
        }
    }
}
impl From<String> for ErrorMessage {
    fn from(value: String) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                text: value,
                thrown: None,
            }),
        }
    }
}
impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.payload.text)
    }
}
impl JsError {
    pub(crate) fn thrown(value: Value, message: String) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                text: message,
                thrown: Some(value),
            }),
        })
    }
    pub(crate) fn thrown_value(&self) -> Option<Value> {
        self.0.payload.thrown
    }
    pub(crate) fn validation(message: String) -> Self {
        Self(ErrorMessage::from(format!(
            "invalid residual program: {message}"
        )))
    }
    fn into_message(self) -> String {
        self.0.payload.text.clone()
    }
}
#[cfg(test)]
mod tests;
struct Frame {
    function: u32,
    pc: usize,
    env: Value,
    this: Value,
    locals: Vec<Value>,
    captured: bool,
    registers: Vec<Value>,
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
    iterator_proto: Value,
    regexp_proto: Value,
    constants: Vec<Value>,
    const_arrays: Vec<Option<Rc<Vec<Value>>>>,
    natives: Vec<(Native, Value)>,
    frames: Vec<Frame>,
    frame_pool: Vec<Frame>,
    profile: Profile,
    numeric_sites: FxHashMap<(u32, u32), NumericSite>,
    shapes: Vec<Vec<Atom>>,
    transitions: FxHashMap<(u32, Atom), u32>,
    atom_text: AtomTable,
    atoms: FxHashMap<u64, Atom>,
    atom_collisions: FxHashMap<u64, Vec<Atom>>,
    dynamic_atoms: Vec<Rc<str>>,
    dynamic_strings: Option<Box<FxHashMap<u64, Value>>>,
    symbol_registry: FxHashMap<String, Value>,
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
    descriptors: FxHashMap<(Value, Atom), PropertyAttributes>,
    random_state: u64,
}
impl<H: Host> Vm<H> {
    pub fn root(&mut self, value: Value) -> RootId {
        self.heap.root(value)
    }
    pub fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.heap.update_root(root, value)
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
        self.profile.report(&self.heap, program);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("RQJ_MEMORY").is_some() {
            self.report_memory("complete");
        }
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
        self.numeric_sites.clear();
        self.shapes.truncate(1);
        self.transitions.clear();
        self.atom_text = program.atoms.clone();
        self.atoms.clear();
        self.atom_collisions.clear();
        self.dynamic_atoms.clear();
        self.dynamic_strings = None;
        self.symbol_registry.clear();
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
        self.random_state = 0x4d59_5df4_d0f3_3173;
        self.globals = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        for constant in &program.constants {
            let value = match constant {
                Constant::Number(v) => Value::number(*v),
                Constant::String(v) => self.heap.alloc(Cell::String(v.clone())),
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
        match self.call_target(callee)? {
            CallTarget::Native(native) => {
                self.profile.call_target(0, args.len());
                self.call_native(p, native, this, args)
            }
            CallTarget::User(id, env) => {
                self.profile.call_target(1, args.len());
                self.call_user(p, id, env, this, args)
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

    pub(super) fn intern_dynamic_string(&mut self, text: String) -> Value {
        let mut hasher = rustc_hash::FxHasher::default();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(value) = self
            .dynamic_strings
            .as_ref()
            .and_then(|strings| strings.get(&hash))
            .copied()
            && matches!(self.heap.get(value), Some(Cell::String(candidate)) if candidate == &text)
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.dynamic_string(true);
            return value;
        }
        #[cfg(feature = "profile-aggregate")]
        self.profile.dynamic_string(false);
        // A hash collision only evicts this weak canonical entry. The content
        // check above prevents it from ever changing JavaScript semantics.
        let value = self.heap.alloc(Cell::String(text));
        self.dynamic_strings
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
            .insert(hash, value);
        value
    }
}
