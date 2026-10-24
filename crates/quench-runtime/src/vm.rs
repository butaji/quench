mod compiled;
mod shape_transitions;
mod startup_intrinsics;
mod wasm;
pub use compiled::ExecutionMode;
mod wasm_exception;
mod wasm_gc;
mod wasm_host;
mod wasm_js;
mod wasm_table;
use crate::Value;
use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, LEXICAL_THIS_BINDING,
    NEW_TARGET_BINDING, Op, Operand, Register, ResidualProgram, WideInstruction,
};
#[cfg(feature = "profile-memory")]
use crate::heap::CellKind;
use crate::heap::{
    CallSiteLocation, CallSiteName, CallSiteRecord, Cell, FunctionKind, GcTraceKind, Heap,
    IteratorConsumer, IteratorHelper, IteratorKind, Native, Object, ProxyKind, RootId, StackData,
    TypedArrayKind, WeakHandle,
};
use crate::host::{CapabilityId, Host, HostContext};
use crate::profile::Profile;
use crate::value::number_to_u32;
use activation::{CallContext, ConstructionTarget};
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
    /// The RegExp members the builtin fast paths check on every call.
    regexp_exec: Atom,
    regexp_flags: Atom,
    last_index: Atom,
    /// `regexp::REGEXP_FLAG_ACCESSORS` names, in that order.
    regexp_flag_accessors: [Atom; regexp::REGEXP_FLAG_ACCESSORS.len()],
    /// Read by every promise resolution and species lookup.
    then: Atom,
    constructor: Atom,
    /// Canonical key for named function-info lookup; paired with length_atom.
    name: Atom,
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
mod dispatch_fast;
mod dispatch_frame;
mod dispatch_wasm;
mod dynamic_strings;
mod embedding;
mod environment;
mod equality;
mod error;
mod eval;
mod fdlibm_math;
mod field_cache;
mod finalization;
mod function;
mod function_cache;
mod gc;
mod generator;
mod group_by;
mod host_function;
mod index;
mod inspection;
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
mod lazy_data_property;
mod local_time;
mod property_definition;
mod provenance;
use group_by::GroupByKind;
use property_definition::PropertyDefinitionKind;
mod iterators;
mod json;
mod method_cache;
mod module;
mod number;
mod object;
mod object_array;
mod object_assignment;
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
use object::AtomClass;
pub(crate) use program_store::ProgramId;
use program_store::{ModuleImport, ProgramStore};
use promise::PromiseRuntime;
use property_key::PropertyKey;
mod operations;
mod owned_clone_native;
mod primitives;
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
    /// Leading actual arguments that legacy `f.arguments` reflects live from the parameter
    /// storage; `original_arguments` holds the arguments after them.
    reflected_parameters: u16,
    original_arguments: Vec<Value>,
    program: ProgramId,
    function: u32,
    pc: usize,
    /// Whether pc is a saved continuation after a call rather than the executing operation.
    pc_is_resume: bool,
    source_field: Option<(usize, crate::bytecode::FieldPart)>,
    // The current operation's lexical projection, independent of the PC
    // published for GC roots or saved as the continuation after a call.
    binding_site_pc: Option<u32>,
    env: Value,
    this: Value,
    locals: Vec<Value>,
    dynamic_bindings: Vec<(Atom, Value)>,
    captured: bool,
    registers: Vec<Value>,
    capture_cache: CaptureCache,
    /// How many leading register slots hold values, whatever the current length: below it a
    /// root-mapped activation can take the file without rewriting it.
    initialized_registers: usize,
    active_iterators: Vec<ActiveIterator>,
    // Empty while active; owns the transferred scope stack while detached.
    with_objects: Vec<Value>,
    with_base: usize,
    /// The frame pushed its own `this` binding with a live value. Only `super()` rebinds
    /// `this`, and only in derived constructors, whose binding starts deleted; so for these
    /// frames `this` is authoritative and the binding lookup can be skipped.
    fixed_this: bool,
}
/// A frame's innermost capture slots, valid for `env` until the collection after `epoch`
/// (`Vm::cached_direct_capture`).
#[derive(Clone, Copy)]
pub(super) struct CaptureCache {
    env: Value,
    /// Whether `env` is the frame's own environment, which the walk steps past; a
    /// pooled frame can later run a closure whose parent is that same environment.
    captured: bool,
    epoch: u64,
    slots: *const crate::heap::EnvironmentSlots,
    /// The program of a root environment, whose projected slots are read elsewhere
    /// (`owned_capture`); `None` for a function's environment.
    root: Option<Option<u32>>,
}

impl Default for CaptureCache {
    fn default() -> Self {
        Self {
            env: Value::NULL,
            captured: false,
            epoch: 0,
            slots: std::ptr::null(),
            root: None,
        }
    }
}

impl Frame {
    fn source_instruction_pc(&self) -> usize {
        if self.pc_is_resume {
            self.pc.saturating_sub(1)
        } else {
            self.pc
        }
    }

    fn empty(program: ProgramId, with_base: usize) -> Self {
        Self {
            context: CallContext::Internal,
            reflected_parameters: 0,
            original_arguments: vec![],
            program,
            function: 0,
            pc: 0,
            pc_is_resume: true,
            source_field: None,
            binding_site_pc: None,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            dynamic_bindings: vec![],
            captured: false,
            registers: vec![],
            initialized_registers: 0,
            capture_cache: CaptureCache::default(),
            active_iterators: vec![],
            with_objects: Vec::new(),
            with_base,
            fixed_this: false,
        }
    }

    /// Arguments for a body that keeps its parameters in registers.
    #[inline(always)]
    fn bind_register_parameters(&mut self, function: &crate::bytecode::Function, args: &[Value]) {
        if let Some(base) = function.parameter_registers {
            for offset in 0..function.params {
                self.registers[usize::from(base + offset)] = args
                    .get(usize::from(offset))
                    .copied()
                    .unwrap_or(Value::UNDEFINED);
            }
        }
    }

    /// A Wasm activation's registers: the parameters where the function
    /// keeps them, every other register its default, each written once.
    #[inline(always)]
    fn prepare_wasm_registers(
        &mut self,
        function: &crate::bytecode::Function,
        arguments: &[Value],
    ) {
        let initial = function.initial_register.value();
        let count = usize::from(function.registers);
        let (base, parameters) = match function.parameter_registers {
            Some(base) => (
                usize::from(base),
                &arguments[..arguments.len().min(usize::from(function.params))],
            ),
            None => (0, &[][..]),
        };
        assert!(
            base + parameters.len() <= count,
            "parameters fit the register file"
        );
        self.registers.clear();
        self.registers.reserve(count);
        let registers = self.registers.as_mut_ptr();
        // SAFETY: capacity holds `count` values; every slot below `count` is
        // written before the length covers it, parameters in their window.
        unsafe {
            for index in 0..count {
                registers.add(index).write(initial);
            }
            // A few values: an inline copy, not a library call.
            for (offset, value) in parameters.iter().enumerate() {
                registers.add(base + offset).write(*value);
            }
            self.registers.set_len(count);
        }
        self.initialized_registers = self.initialized_registers.max(count);
    }

    #[inline(always)]
    fn prepare_registers(&mut self, count: u16, initial: crate::bytecode::InitialRegister) {
        self.registers.clear();
        self.registers.resize(usize::from(count), initial.value());
        self.initialized_registers = self.initialized_registers.max(usize::from(count));
    }

    /// Sizes the register file for `function`. Compiled code writes every register before
    /// reading it, so stale values from a pooled frame are unobservable to the program; they
    /// matter only to a collector that scans the whole file. A function with a liveness root
    /// map is scanned through it, so only functions without one, and generator or async
    /// activations whose suspended registers are scanned whole, are cleared.
    /// A body whose registers start at a value other than `undefined` (Wasm's i32 zero) relies
    /// on that fill, so it is always cleared.
    #[inline(always)]
    fn prepare_registers_for(&mut self, function: &crate::bytecode::Function) {
        let register_count = usize::from(function.registers);
        if function.register_root_offset == crate::bytecode::NO_REGISTER_ROOT_MAP
            || function.is_generator
            || function.is_async
            || function.initial_register != crate::bytecode::InitialRegister::Undefined
        {
            self.prepare_registers(function.registers, function.initial_register);
        } else {
            if register_count <= self.initialized_registers {
                // SAFETY: the first `initialized_registers` slots of this buffer hold values
                // (`Value` has no destructor, and shortening the file kept them in place).
                unsafe { self.registers.set_len(register_count) };
            } else {
                self.registers.resize(register_count, Value::UNDEFINED);
                self.initialized_registers = register_count;
            }
            if let Some(register) = function.undefined_register {
                // Validation bounds the register by the function's register count.
                self.registers[usize::from(register)] = Value::UNDEFINED;
            }
        }
    }

    /// Makes `locals` `count` undefined values. A pooled frame usually has the capacity already,
    /// so this is one capacity check and the stores.
    #[inline(always)]
    fn reset_locals_undefined(&mut self, count: usize) {
        self.locals.clear();
        self.locals.resize(count, Value::UNDEFINED);
    }

    /// `initialize_promoted_registers` for a plain activation, whose locals already start
    /// undefined: its promoted entries, ordered by local, begin with every parameter, and
    /// validation places promoted entry `i` in register `i`.
    #[inline(always)]
    fn place_plain_parameters(&mut self, function: &crate::bytecode::Function, args: &[Value]) {
        debug_assert_eq!(self.registers.len(), usize::from(function.registers));
        debug_assert!(
            function
                .local_registers
                .iter()
                .enumerate()
                .all(|(index, entry)| usize::from(entry.register) == index)
        );
        let promoted = function.local_registers.len();
        let supplied = usize::from(function.params).min(args.len());
        let registers = self.registers.as_mut_ptr();
        // SAFETY: residual validation bounds every promoted register, hence `promoted`, by
        // the register count the file was sized to, and `supplied` by `args.len()`.
        unsafe {
            for index in 0..supplied {
                *registers.add(index) = *args.get_unchecked(index);
            }
            for index in supplied..promoted {
                *registers.add(index) = Value::UNDEFINED;
            }
        }
    }

    fn initialize_promoted_registers(
        &mut self,
        function: &crate::bytecode::Function,
        args: &[Value],
    ) {
        debug_assert_eq!(self.locals.len(), usize::from(function.locals));
        debug_assert_eq!(self.registers.len(), usize::from(function.registers));
        for entry in &function.local_registers {
            // A promoted parameter starts as its argument and a promoted `var` as undefined.
            let initial = if entry.local < function.params {
                args.get(usize::from(entry.local))
                    .copied()
                    .unwrap_or(Value::UNDEFINED)
            } else {
                Value::UNDEFINED
            };
            // SAFETY: residual validation bounds each promoted entry by the function's local and
            // register counts, which frame setup sized these buffers to.
            unsafe {
                *self.locals.get_unchecked_mut(usize::from(entry.local)) = Value::UNDEFINED;
                *self
                    .registers
                    .get_unchecked_mut(usize::from(entry.register)) = initial;
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveIterator {
    pub(crate) iterator: u16,
    pub(crate) done: u16,
}
/// A queued job: a call, or a promise reaction run without a job function to dispatch through.
enum PendingJob {
    Call {
        callback: Value,
        this: Value,
        args: Vec<Value>,
    },
    Reaction(promise::PromiseJob),
}

impl PendingJob {
    fn roots(&self) -> Vec<Value> {
        match self {
            Self::Call {
                callback,
                this,
                args,
            } => [*callback, *this]
                .into_iter()
                .chain(args.iter().copied())
                .collect(),
            Self::Reaction(job) => vec![job.realm, job.handler, job.next, job.value],
        }
    }
}
struct Realm {
    globals: Value,
    global_lexical_declarations: FxHashSet<Atom>,
    global_lexical_bindings: FxHashMap<Atom, Value>,
    immutable_global_lexical_bindings: FxHashSet<Atom>,
    global_lexical_states: FxHashMap<Value, GlobalLexicalState>,
    intrinsics: RealmIntrinsics,
    jobs: std::collections::VecDeque<PendingJob>,
    template_objects: FxHashMap<(ProgramId, u32, u32), Value>,
    promise: PromiseRuntime,
}
#[derive(Default)]
struct RealmIntrinsics {
    arguments_objects: FxHashMap<(Value, bool), ArgumentsObjectTemplate>,
    iterator_prototypes: FxHashMap<Value, IteratorRealmPrototypes>,
    builtin_prototypes: FxHashMap<(Value, Native), Value>,
    /// Original per-realm constructors for function factory allocations.
    function_constructors: FxHashMap<(Value, Native), Value>,
    promise_constructors: FxHashMap<Value, Value>,
    error_intrinsics: FxHashMap<Value, ErrorRealmIntrinsics>,
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
#[derive(Clone, Copy)]
struct ErrorRealmIntrinsics {
    host_formatter: Option<Value>,
    constructor: Value,
    stack_getter: Value,
    stack_setter: Value,
    call_site_constructor: Value,
    call_site_prototype: Value,
}
#[derive(Clone, Copy)]
struct ArgumentsObjectTemplate {
    anchor: Value,
    length_slot: usize,
    callee_slot: usize,
    iterator_slot: usize,
}
const ARGUMENTS_OBJECT_PROPERTY_COUNT: usize = 3;
/// Where a private name is resolved: the running function and the environment its
/// lookup starts from.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PrivateHomeKey {
    environment: Value,
    program: ProgramId,
    function: u32,
    name: Atom,
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
    holder: u32,
    slot: u16,
}
const NO_FIELD_HOLDER: u32 = u32::MAX;
/// Prototypes an inherited field load may traverse to its holder. DeltaBlue's constraint
/// methods sit three prototypes above their instances; deeper chains stay uncached.
const INHERITED_FIELD_CACHE_MAX_DEPTH: usize = 4;
/// A field load answered by a data property `depth` prototypes above a receiver of shape
/// `receiver`: the chain still has exactly `prototype_shapes[..depth]`, the last of which
/// holds the property at `slot`.
#[derive(Clone, Copy)]
struct InheritedFieldCache {
    receiver: u32,
    prototype_shapes: [u32; INHERITED_FIELD_CACHE_MAX_DEPTH],
    depth: u8,
    slot: u16,
}
const NO_FIELD_RECEIVER: u32 = u32::MAX;
// DeltaBlue's measured inherited writes reach three prototypes; keep the
// residual cache bounded while covering that common constructor shape.
const FIELD_ADD_CACHE_MAX_PROTO_DEPTH: usize = 4;
/// Source shapes one add site remembers; the oldest is evicted beyond this. Objects whose
/// optional fields are assigned in varying order reach one add site with several shapes.
const FIELD_ADD_CACHE_WAYS: usize = 4;
const EMPTY_CACHE: FieldCache = FieldCache {
    receiver: NO_FIELD_RECEIVER,
    holder: NO_FIELD_HOLDER,
    slot: 0,
};
/// One cached property-add transition: an extensible ordinary object of `source_shape` whose
/// prototype chain still has exactly `prototype_shapes[..prototype_depth]` gains `atom` at `slot`
/// and moves to `target_shape`.
#[derive(Clone, Copy)]
struct FieldAddCache {
    atom: Atom,
    source_shape: u32,
    target_shape: u32,
    slot: u16,
    prototype_depth: u8,
    prototype_shapes: [u32; FIELD_ADD_CACHE_MAX_PROTO_DEPTH],
}
impl FieldAddCache {
    const EMPTY: Self = Self {
        atom: u32::MAX,
        source_shape: NO_FIELD_RECEIVER,
        target_shape: NO_FIELD_RECEIVER,
        slot: 0,
        prototype_depth: 0,
        prototype_shapes: [NO_FIELD_RECEIVER; FIELD_ADD_CACHE_MAX_PROTO_DEPTH],
    };
    fn prototype_shapes(&self) -> &[u32] {
        &self.prototype_shapes[..usize::from(self.prototype_depth)]
    }
}
/// The add transitions one site remembers, oldest first.
#[derive(Clone, Copy)]
struct FieldAddCacheSet {
    len: u8,
    entries: [FieldAddCache; FIELD_ADD_CACHE_WAYS],
}
impl FieldAddCacheSet {
    const EMPTY: Self = Self {
        len: 0,
        entries: [FieldAddCache::EMPTY; FIELD_ADD_CACHE_WAYS],
    };
    #[inline(always)]
    fn find(&self, source_shape: u32, atom: Atom) -> Option<&FieldAddCache> {
        self.entries[..usize::from(self.len)]
            .iter()
            .find(|cache| cache.source_shape == source_shape && cache.atom == atom)
    }
    /// Replaces the entry for the same source shape and atom, else appends, evicting the oldest
    /// entry when the set is full.
    fn insert(&mut self, cache: FieldAddCache) {
        let len = usize::from(self.len);
        let kept = match self.entries[..len]
            .iter()
            .position(|entry| entry.source_shape == cache.source_shape && entry.atom == cache.atom)
        {
            Some(index) => {
                self.entries.copy_within(index + 1..len, index);
                len - 1
            }
            None if len == FIELD_ADD_CACHE_WAYS => {
                self.entries.copy_within(1..len, 0);
                len - 1
            }
            None => len,
        };
        self.entries[kept] = cache;
        self.len = (kept + 1) as u8;
    }
}
const NO_FIELD_ADD_SET: u32 = u32::MAX;
const NO_MEGAMORPHIC_FIELD: u32 = u32::MAX;
const FIELD_MEGAMORPHIC_INLINE: usize = 4;
// Bound each site's overflow to 256 recorded shapes; unseen shapes keep the generic fallback.
const FIELD_MEGAMORPHIC_LIMIT: usize = 256;
struct FieldCacheSet {
    len: u8,
    entries: [FieldCache; FIELD_MEGAMORPHIC_INLINE],
    overflow: Option<Box<FxHashMap<u64, FieldCache>>>,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProgramCacheLayout {
    field_base: usize,
    method_base: usize,
    object_base: usize,
    /// The field cache index of the program's shared overflow site, when its sites
    /// outnumber the 16-bit site space (`SHARED_UNCACHED_SITE`); nothing is cached there.
    shared_uncached_field: Option<usize>,
}

impl ProgramCacheLayout {
    fn shared_uncached_field(field_base: usize, program: &ResidualProgram) -> Option<usize> {
        (program.cache_sites > crate::bytecode::SHARED_UNCACHED_SITE)
            .then(|| field_base + usize::from(crate::bytecode::SHARED_UNCACHED_SITE))
    }
}
/// Entries of the in-loop call target cache, indexed by the callee's heap index.
const CALL_TARGET_CACHE_SIZE: usize = 64;
const _: () = assert!(CALL_TARGET_CACHE_SIZE.is_power_of_two());
/// A user function the dispatch loop may push in place (`Vm::in_loop_call_target`), valid
/// for `callee` in `realm` until the collection after `epoch`: until then the callee's index
/// names the same function cell, whose kind, environment and realm never change.
#[derive(Clone, Copy)]
struct InLoopCallTarget {
    callee: Value,
    epoch: u64,
    realm: Value,
    program: ProgramId,
    function: u32,
    env: Value,
}
const EMPTY_CALL_TARGET: InLoopCallTarget = InLoopCallTarget {
    callee: Value::UNDEFINED,
    epoch: 0,
    realm: Value::UNDEFINED,
    program: ProgramId::MAIN,
    function: 0,
    env: Value::UNDEFINED,
};
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
    FunctionName {
        name: Atom,
    },
    FunctionNameReconfigured,
    FunctionLengthReconfigured,
    Constructed {
        constructor: Value,
        new_target_is_base: bool,
    },
    Add {
        key: property_key::PropertyKey,
        slot: u32,
        attributes: DataAttributes,
    },
    Delete {
        key: property_key::PropertyKey,
        slot: u32,
    },
    Vacant,
    Descriptor {
        slot: u32,
        attributes: ShapeAttributes,
    },
    Dictionary {
        trigger: DictionaryTrigger,
    },
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ShapeTransitionKey {
    FunctionName(Atom),
    FunctionNameReconfigured,
    FunctionLengthReconfigured,
    Constructed(Value, bool),
    AddString(Atom, DataAttributes),
    /// A descriptor without accessor functions (`ShapeAttributes::accessors` is
    /// `NO_SHAPE_ACCESSORS`).
    Descriptor {
        slot: u32,
        attributes: ShapeAttributes,
    },
}
/// A property's attributes as a shape records them. Accessor functions are rare among
/// shapes and live in `Vm::shape_accessors`, which `accessors` indexes, so every shape stays
/// small.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ShapeAttributes {
    writable: bool,
    enumerable: bool,
    configurable: bool,
    accessor: bool,
    accessors: u32,
}
/// The attributes a data property is created with. Its `Add` transition records them, so a
/// property created non-enumerable (as builtin methods are) costs no second, `Descriptor`
/// shape.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DataAttributes {
    writable: bool,
    enumerable: bool,
    configurable: bool,
}
impl DataAttributes {
    /// `attributes` when they describe a data property.
    fn of(attributes: PropertyAttributes) -> Option<Self> {
        (!attributes.accessor).then_some(Self {
            writable: attributes.writable,
            enumerable: attributes.enumerable,
            configurable: attributes.configurable,
        })
    }

    fn shape(self) -> ShapeAttributes {
        ShapeAttributes {
            writable: self.writable,
            enumerable: self.enumerable,
            configurable: self.configurable,
            accessor: false,
            accessors: NO_SHAPE_ACCESSORS,
        }
    }
}
const DEFAULT_DATA_ATTRIBUTES: DataAttributes = DataAttributes {
    writable: true,
    enumerable: true,
    configurable: true,
};
/// `ShapeAttributes::accessors` of attributes with neither getter nor setter.
const NO_SHAPE_ACCESSORS: u32 = u32::MAX;
/// A shape-held accessor property's getter and setter.
type ShapeAccessors = (Option<Value>, Option<Value>);
impl ShapeAttributes {
    /// `attributes` for a shape, appending its accessor functions to `accessors`.
    fn record(attributes: PropertyAttributes, accessors: &mut Vec<ShapeAccessors>) -> Self {
        let index = if attributes.getter.is_none() && attributes.setter.is_none() {
            NO_SHAPE_ACCESSORS
        } else {
            accessors.push((attributes.getter, attributes.setter));
            u32::try_from(accessors.len() - 1).expect("shape accessor table exhausted")
        };
        Self {
            writable: attributes.writable,
            enumerable: attributes.enumerable,
            configurable: attributes.configurable,
            accessor: attributes.accessor,
            accessors: index,
        }
    }

    /// These attributes as a data property's, which its add transition can carry.
    fn data(self) -> Option<DataAttributes> {
        (!self.accessor && self.accessors == NO_SHAPE_ACCESSORS).then_some(DataAttributes {
            writable: self.writable,
            enumerable: self.enumerable,
            configurable: self.configurable,
        })
    }

    fn functions(self, accessors: &[ShapeAccessors]) -> ShapeAccessors {
        accessors
            .get(self.accessors as usize)
            .copied()
            .unwrap_or((None, None))
    }

    fn attributes(self, accessors: &[ShapeAccessors]) -> PropertyAttributes {
        let (getter, setter) = self.functions(accessors);
        PropertyAttributes {
            writable: self.writable,
            enumerable: self.enumerable,
            configurable: self.configurable,
            accessor: self.accessor,
            getter,
            setter,
        }
    }
}
impl ShapeTransition {
    fn introduces_gc_roots(self) -> bool {
        match self {
            Self::Add { key, .. } => key.symbol_value().is_some(),
            Self::Descriptor { attributes, .. } => attributes.accessors != NO_SHAPE_ACCESSORS,
            Self::Root
            | Self::FunctionName { .. }
            | Self::FunctionNameReconfigured
            | Self::FunctionLengthReconfigured
            | Self::Constructed { .. }
            | Self::Delete { .. }
            | Self::Vacant
            | Self::Dictionary { .. } => false,
        }
    }

    fn cache_key(self) -> Option<ShapeTransitionKey> {
        match self {
            Self::FunctionName { name } => Some(ShapeTransitionKey::FunctionName(name)),
            Self::FunctionNameReconfigured => Some(ShapeTransitionKey::FunctionNameReconfigured),
            Self::FunctionLengthReconfigured => {
                Some(ShapeTransitionKey::FunctionLengthReconfigured)
            }
            Self::Constructed {
                constructor,
                new_target_is_base,
            } => Some(ShapeTransitionKey::Constructed(
                constructor,
                new_target_is_base,
            )),
            Self::Add {
                key: PropertyKey::String(atom),
                attributes,
                ..
            } => Some(ShapeTransitionKey::AddString(atom, attributes)),
            // The transition table outlives individual properties. Never put
            // accessor values in a key, where they would become strong roots.
            Self::Descriptor { slot, attributes } if attributes.accessors == NO_SHAPE_ACCESSORS => {
                Some(ShapeTransitionKey::Descriptor { slot, attributes })
            }
            Self::Root
            | Self::Add { .. }
            | Self::Delete { .. }
            | Self::Vacant
            | Self::Descriptor { .. }
            | Self::Dictionary { .. } => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DictionaryTrigger {
    PropertyCount,
    DeletionPattern,
    PrototypeUse,
    /// A module namespace starts in dictionary mode, so no ordinary object shares its shape
    /// and a field cache hit, which only records non-dictionary shapes, proves an ordinary
    /// receiver.
    ModuleNamespace,
}
impl DictionaryTrigger {
    #[cfg(feature = "profile-aggregate")]
    pub(crate) const COUNT: usize = 4;

    #[cfg(feature = "profile-aggregate")]
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::PropertyCount => 0,
            Self::DeletionPattern => 1,
            Self::PrototypeUse => 2,
            Self::ModuleNamespace => 3,
        }
    }
}
struct ShapeLookupIndex {
    /// The shape's live properties in definition order, with their slots.
    entries: Vec<(property_key::PropertyKey, u32)>,
    /// Positions in `entries`, hashed by key: the pairs themselves are stored once.
    positions: hashbrown::HashTable<u32>,
    attributes: FxHashMap<u32, ShapeAttributes>,
}
impl ShapeLookupIndex {
    fn key_hash(key: property_key::PropertyKey) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        key.hash(&mut hasher);
        hasher.finish()
    }
    fn new(
        entries: Vec<(property_key::PropertyKey, u32)>,
        attributes: FxHashMap<u32, ShapeAttributes>,
    ) -> Self {
        let mut positions = hashbrown::HashTable::with_capacity(entries.len());
        for (position, &(key, _)) in entries.iter().enumerate() {
            let position = u32::try_from(position).expect("shape width fits u32");
            positions.insert_unique(Self::key_hash(key), position, |&position| {
                Self::key_hash(entries[position as usize].0)
            });
        }
        Self {
            entries,
            positions,
            attributes,
        }
    }
    fn slot(&self, key: property_key::PropertyKey) -> Option<u32> {
        self.positions
            .find(Self::key_hash(key), |&position| {
                self.entries[position as usize].0 == key
            })
            .map(|&position| self.entries[position as usize].1)
    }
    #[cfg(feature = "profile-memory")]
    fn payload_capacity_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<(property_key::PropertyKey, u32)>()
            + self.positions.capacity() * (size_of::<u32>() + 1)
            + self.attributes.capacity() * size_of::<(u32, ShapeAttributes)>()
    }
}
struct Shape {
    /// The shape this one extends; `NO_PARENT_SHAPE` for the root.
    parent: u32,
    transition: ShapeTransition,
    /// Property slots an object of this shape stores (slot indices are `u32`).
    storage_len: u32,
    dictionary_trigger: Option<DictionaryTrigger>,
    may_have_gc_roots: bool,
    lookup_index: OnceCell<Box<ShapeLookupIndex>>,
    /// Lookups that walked at least `SHAPE_INDEX_WALK_DISTANCE` transitions
    /// from this shape before it had an index.
    long_walks: std::cell::Cell<u8>,
    /// Memo of this immutable shape's own data `prototype` slot, which every `new F` reads:
    /// `PROTOTYPE_SLOT_UNKNOWN` until first derived, `PROTOTYPE_SLOT_NONE` when absent or an
    /// accessor. Dictionary shapes never memoize.
    prototype_slot: std::cell::Cell<u32>,
    /// Memo of whether this immutable shape's keys include an array-index name: `None` until
    /// first derived (`Vm::shape_has_index_key`). Dictionary shapes never memoize.
    index_keys: std::cell::Cell<Option<bool>>,
}
// Every object layout and property key costs one shape; keep the record compact.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<Shape>() == 48);
/// `Shape::parent` of the root shape.
const NO_PARENT_SHAPE: u32 = u32::MAX;
const PROTOTYPE_SLOT_UNKNOWN: u32 = u32::MAX;
const PROTOTYPE_SLOT_NONE: u32 = u32::MAX - 1;
/// The shape table's initial capacity, above what a builtin realm uses (about 1900 shapes).
/// A table this large is mapped by the allocator on its own, so it grows by remapping in
/// place, never leaving a copied-out table behind, and only the shapes in use are resident.
const SHAPE_TABLE_CAPACITY: usize = 4096;
impl Shape {
    fn root() -> Self {
        Self::child(None, ShapeTransition::Root, 0, None, false)
    }

    /// A shape table holding the root shape.
    fn table() -> Vec<Self> {
        let mut table = Vec::with_capacity(SHAPE_TABLE_CAPACITY);
        table.push(Self::root());
        table
    }

    fn parent(&self) -> Option<u32> {
        (self.parent != NO_PARENT_SHAPE).then_some(self.parent)
    }

    fn storage_len(&self) -> usize {
        self.storage_len as usize
    }

    fn child(
        parent: Option<u32>,
        transition: ShapeTransition,
        storage_len: usize,
        dictionary_trigger: Option<DictionaryTrigger>,
        parent_may_have_gc_roots: bool,
    ) -> Self {
        Self {
            parent: parent.unwrap_or(NO_PARENT_SHAPE),
            transition,
            storage_len: u32::try_from(storage_len).expect("object property index exceeds u32"),
            dictionary_trigger,
            may_have_gc_roots: parent_may_have_gc_roots || transition.introduces_gc_roots(),
            lookup_index: OnceCell::new(),
            long_walks: std::cell::Cell::new(0),
            prototype_slot: std::cell::Cell::new(PROTOTYPE_SLOT_UNKNOWN),
            index_keys: std::cell::Cell::new(None),
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
    PushFrame {
        destination: Register,
        call_guard: crate::stack::GuestCallGuard,
        /// For `new`: the allocated receiver, the result unless the callee returns an object.
        construct_this: Option<Value>,
    },
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

const DEFAULT_SHAPE_ATTRIBUTES: ShapeAttributes = ShapeAttributes {
    writable: true,
    enumerable: true,
    configurable: true,
    accessor: false,
    accessors: NO_SHAPE_ACCESSORS,
};
const DEFAULT_PROPERTY_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    writable: true,
    enumerable: true,
    configurable: true,
    accessor: false,
    getter: None,
    setter: None,
};
pub(crate) struct Vm<H> {
    /// VM-wide running-job admission for native microtask checkpoints.
    native_microtask_running: bool,
    pub(crate) host: H,
    specialized: bool,
    heap: Heap,
    realm: Realm,
    object_proto: Value,
    function_proto: Value,
    array_proto: Value,
    /// %String.prototype% of the current realm; primitive string property
    /// reads use it directly, so realm switches re-derive it.
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
    /// ECMAScript [[KeptAlive]] targets, held until the host ends a job turn.
    kept_alive: Vec<RootId>,
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
    /// The function values in `natives`. One native serves every realm, so it runs in the
    /// realm of its caller rather than switching to the creator's.
    shared_natives: FxHashSet<Value>,
    frames: Vec<Box<Frame>>,
    native_stack_frames: Vec<(usize, String)>,
    frame_pool: Vec<Box<Frame>>,
    /// Activations a running lane holds without frames.
    lane_stack: dispatch_fast::LaneStack,
    /// Whether Wasm functions with a compiled denotation run as native code.
    execution_mode: ExecutionMode,
    execution_interrupt: crate::ExecutionInterrupt,
    active_call_roots: Vec<Value>,
    with_stack: Vec<Value>,
    /// The realm's intrinsics as the host bootstrap left them (`seal_startup_intrinsics`).
    startup_intrinsics: startup_intrinsics::StartupIntrinsics,
    /// Set by the first `with` entry and never cleared: `Native::WithEnter` is
    /// the only source of with objects, so until it runs no environment
    /// carries any.
    with_scope_entered: bool,
    suspended: Vec<SuspendedEntry>,
    suspended_free: Vec<u32>,
    test262_agent: Test262AgentState,
    shared_async_waiters: Vec<atomics::SharedAsyncWaiter>,
    programs: ProgramStore,
    program_cache_layouts: Vec<ProgramCacheLayout>,
    /// The register file of the frame the general loop dispatches; see `run_general_loop`.
    active_registers: *mut Value,
    /// `program_cache_layouts[active_program]`, kept by `set_active_program`.
    active_layout: ProgramCacheLayout,
    /// The active program's materialized constants, kept by `set_active_program` and by
    /// constant installation. A program's constant buffer never moves once installed.
    active_constants: *const [Value],
    active_program: ProgramId,
    profile: Profile,
    shapes: Vec<Shape>,
    /// The getters and setters of shapes' accessor descriptors (`ShapeAttributes`).
    shape_accessors: Vec<ShapeAccessors>,
    transitions: shape_transitions::ShapeTransitionIndex,
    atom_text: AtomTable,
    atoms: FxHashMap<u64, Atom>,
    atom_collisions: FxHashMap<u64, Vec<Atom>>,
    /// The atoms that spell canonical array indices, by index: index-keyed lookups find
    /// them without formatting and hashing the decimal text.
    array_index_atoms: FxHashMap<u32, Atom>,
    dynamic_atoms: Vec<JsString>,
    /// Every atom name in id order (`atom_text`, then `dynamic_atoms`), shared by the dynamic
    /// and module programs whose atoms are a prefix of the VM's.
    atom_sequence: Rc<crate::bytecode::AtomSequence>,
    /// The strings most recently matched by a RegExp, most recent first, each prepared once
    /// for repeated matching (`Vm::regexp_subject`).
    regexp_subjects: Vec<Rc<quench_regexp::Subject>>,
    regexp_matchers: regexp::RegExpMatcherCache,
    // One lazily derived class byte per atom; length tracks atom_text plus dynamic_atoms.
    atom_classes: Vec<std::cell::Cell<u8>>,
    dynamic_strings: Option<Box<FxHashMap<u64, Value>>>,
    symbol_registry: FxHashMap<String, Value>,
    well_known_symbols: FxHashMap<String, Value>,
    string_concats: Option<Box<[StringConcatCache]>>,
    call_targets: Box<[InLoopCallTarget; CALL_TARGET_CACHE_SIZE]>,
    field_caches: Vec<FieldCache>,
    /// Field load sites whose data property was found beyond the immediate prototype, by
    /// global site index; cleared with the field caches.
    inherited_field_caches: FxHashMap<usize, InheritedFieldCache>,
    field_add_indices: Vec<u32>,
    field_add_sets: Vec<FieldAddCacheSet>,
    megamorphic_field_indices: Vec<u32>,
    megamorphic_fields: Vec<FieldCacheSet>,
    length_atom: Atom,
    // Cached `prototype` atom; `NO_CACHED_ATOM` until first use after a program reset.
    prototype_atom: Atom,
    /// A non-dictionary constructor shape verified to have no own `@@hasInstance` and a
    /// data `prototype` at the paired slot; derived from shapes, so field-cache invalidation
    /// clears it.
    instanceof_constructor_layout: Option<(u32, usize)>,
    /// The shapes an ordinary closure's own properties step through, without and with a
    /// `prototype` (`construction::ClosureShapes`); derived from shapes, so field-cache
    /// invalidation clears it.
    closure_shapes: [Option<construction::ClosureShapes>; 2],
    /// A RegExp receiver shape verified to hold an own writable data `lastIndex` at the paired
    /// slot; derived from shapes, so field-cache invalidation clears it.
    regexp_last_index_layout: Option<(u32, u32)>,
    /// Derived from shapes, so field-cache invalidation clears it.
    regexp_builtin_layout: Option<regexp::RegExpBuiltinLayout>,
    /// An object verified to hold the native `Function.prototype[@@hasInstance]` as a
    /// non-writable, non-configurable data property, which it then holds for its lifetime;
    /// rooted so the identity stays meaningful. `Value::NULL` when none.
    instanceof_has_instance_holder: Value,
    to_fixed_atom: Atom,
    to_precision_atom: Atom,
    runtime_atoms: RuntimeAtoms,
    /// `Vm::constructed_shape` answers; cleared by every collection.
    constructed_shapes: [provenance::ConstructedShape; provenance::CONSTRUCTED_SHAPE_ENTRIES],
    method_caches: Vec<[MethodCache; 2]>,
    megamorphic_methods: Vec<MethodCacheSet>,
    /// A filter over the atoms method caches may name: bit `atom % u64::BITS` is set when an
    /// entry for the atom is recorded and the whole filter resets when every method cache is
    /// cleared, so a clear bit proves that no method cache names the atom.
    method_cache_atoms: u64,
    #[cfg(feature = "profile-aggregate")]
    invalidated_methods: FxHashMap<MethodCacheKey, InvalidatedMethod>,
    object_shapes: Vec<u32>,
    descriptors: FxHashMap<(Value, property_key::PropertyKey), PropertyAttributes>,
    // Weak identities; captured environments remain authoritative in function cells.
    function_values: FxHashMap<(ProgramId, u32), Vec<WeakHandle>>,
    /// The JavaScript wrapper of each exported Wasm closure, so every export of one
    /// function is one object. Weak: a wrapper nothing holds is made again on demand.
    wasm_exported_functions: FxHashMap<Value, WeakHandle>,
    /// Private-name home objects found by `private_name_home`, kept while their starting
    /// environment is live.
    private_homes: FxHashMap<PrivateHomeKey, Value>,
    wasm_js: wasm_js::WasmJsIntrinsics,
    direct_eval: bool,
    direct_eval_var_program: Option<ProgramId>,
    parameter_eval: bool,
    eval_script_context: bool,
    deferred_dependency_batch: bool,
    construct_target: Option<ConstructionTarget>,
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
    pub(crate) fn execution_interrupt_handle(&self) -> crate::ExecutionInterrupt {
        self.execution_interrupt.clone()
    }
    pub(super) fn check_execution_interrupt(&self) -> Result<(), JsError> {
        if self.execution_interrupt.is_termination_requested() {
            Err(JsError::execution_terminated())
        } else {
            Ok(())
        }
    }

    /// Makes `global`'s realm current, carrying each realm's global lexical
    /// scope with it; returns the previous realm's global.
    #[inline]
    pub(super) fn switch_realm_global(&mut self, global: Value) -> Value {
        let previous = self.realm.globals;
        if previous != global {
            self.swap_realm_global(previous, global);
        }
        previous
    }

    #[cold]
    #[inline(never)]
    fn swap_realm_global(&mut self, previous: Value, global: Value) {
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
        // Natives are shared by every realm and allocate from these fields, so the fields
        // follow the active realm: a native called from a realm builds that realm's objects.
        for native in [
            Native::Object,
            Native::Function,
            Native::Array,
            Native::String,
        ] {
            if let Some(&prototype) = self
                .realm
                .intrinsics
                .builtin_prototypes
                .get(&(global, native))
            {
                *self.realm_prototype_field(native) = prototype;
            }
        }
    }

    /// The VM field holding the active realm's `native.prototype`.
    fn realm_prototype_field(&mut self, native: Native) -> &mut Value {
        match native {
            Native::Object => &mut self.object_proto,
            Native::Function => &mut self.function_proto,
            Native::Array => &mut self.array_proto,
            Native::String => &mut self.string_proto,
            _ => unreachable!("only these prototypes follow the active realm"),
        }
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
        self.realm.jobs.push_back(PendingJob::Call {
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
            self.set_property_with_attributes(
                self.realm.globals,
                atom,
                Value::UNDEFINED,
                PropertyAttributes {
                    writable: true,
                    enumerable: true,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            )?;
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
    /// Publishes a finished script root's global lexical bindings; every other activation
    /// returns without leaving the caller.
    #[inline(always)]
    pub(super) fn persist_global_lexical_bindings(
        &mut self,
        program: &ResidualProgram,
        frame: &Frame,
    ) {
        if self.eval_script_context && frame.function == ROOT_FUNCTION_ID {
            self.persist_root_global_lexical_bindings(program, frame);
        }
    }

    #[inline(never)]
    fn persist_root_global_lexical_bindings(&mut self, program: &ResidualProgram, frame: &Frame) {
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
        self.execute_with_job_drain(program, &[], true)
    }

    /// Evaluate host-supplied global scripts, then `program`, in one realm.
    pub(crate) fn execute_after_scripts(
        &mut self,
        scripts: &[(&str, &str)],
        program: &ResidualProgram,
    ) -> Result<Value, JsError> {
        self.execute_with_job_drain(program, scripts, true)
    }

    pub(crate) fn execute_deferred_jobs(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<Value, JsError> {
        self.execute_with_job_drain(program, &[], false)
    }

    fn execute_with_job_drain(
        &mut self,
        program: &ResidualProgram,
        scripts: &[(&str, &str)],
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        if program.functions.is_empty() {
            return Err(JsError::validation("program has no entry function".into()));
        }
        self.check_execution_interrupt()?;
        self.initialize(program)?;
        for (source, name) in scripts {
            self.eval_script_source(program, source, name)?;
        }
        if program.is_module() {
            self.instantiate_main_module(program)?;
            self.evaluate_program_module_requests(program)?;
        }
        self.instantiate_global_declarations(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("initialized");
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
        #[cfg(feature = "profile-gc-time")]
        self.report_gc_time_if_enabled();
        #[cfg(feature = "profile-aggregate")]
        if std::env::var_os("QUENCH_PROFILE_ALL_PROGRAMS").is_some() {
            for index in 0..self.programs.len() {
                let id = ProgramId::from_raw(index as u32);
                if let Some(residual) = self.programs.get(id) {
                    eprintln!(
                        "{{\"kind\":\"quench-profile-program\",\"program_id\":{}}}",
                        id.raw(),
                    );
                    self.profile.report(&self.heap, id.raw(), &residual);
                }
            }
            #[cfg(feature = "profile-memory")]
            self.report_memory_if_enabled("complete");
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        if self
            .profile
            .report_dispatch_census_if_enabled(ProgramId::MAIN.raw())
        {
            self.profile
                .report_object_literal_sites(ProgramId::MAIN.raw(), program);
            #[cfg(feature = "profile-memory")]
            self.report_memory_if_enabled("complete");
            return;
        }
        self.profile
            .report(&self.heap, ProgramId::MAIN.raw(), program);
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("complete");
    }
    #[cfg(feature = "profile-gc-time")]
    fn report_gc_time_if_enabled(&self) {
        if std::env::var_os("QUENCH_GC_TIME_PROFILE").is_none() {
            return;
        }
        let (_, collections, _, _, _, _) = self.heap.stats();
        let gc = self.heap.gc_profile();
        eprintln!(
            "{{\"kind\":\"quench-gc-time\",\"collections\":{collections},\"mark_nanos\":{},\"sweep_nanos\":{},\"mark_clear_nanos\":{}}}",
            gc.mark_nanos, gc.sweep_nanos, gc.mark_clear_nanos,
        );
    }
    #[cfg(feature = "profile-memory")]
    pub(crate) fn report_memory_if_enabled(&self, phase: &str) {
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            self.report_memory(phase);
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
            .map(|shape| shape.storage_len())
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
        let field_add_cache_bytes = self.field_add_indices.capacity() * size_of::<u32>()
            + self.field_add_sets.capacity() * size_of::<FieldAddCacheSet>();
        eprintln!(
            "{{\"kind\":\"quench-memory\",\"phase\":\"{phase}\",\"heap_slots\":{slots},\"slot_bytes\":{slot_bytes},\"free_bytes\":{free_bytes},\"cell_bytes\":{cell_bytes},\"cell_counts\":{cell_counts:?},\"property_values\":{property_values},\"property_capacity\":{property_capacity},\"property_free_ranges\":{property_free},\"live_property_values\":{live_property_values},\"live_property_capacity\":{live_property_capacity},\"array_elements\":{array_elements},\"array_capacity\":{array_capacity},\"shapes\":{},\"shape_capacity\":{},\"max_shape_width\":{max_shape_width},\"shape_bytes\":{shape_bytes},\"shape_lookup_index_payload_bytes\":{shape_lookup_index_payload_bytes},\"transitions\":{},\"transition_bytes\":{},\"frame_bytes\":{frame_bytes},\"field_cache_bytes\":{},\"megamorphic_field_sites\":{},\"megamorphic_field_entries\":{megamorphic_field_entries},\"max_megamorphic_field_entries\":{max_megamorphic_field_entries},\"method_cache_bytes\":{},\"megamorphic_method_sites\":{}}}",
            self.shapes.len(),
            self.shapes.capacity(),
            self.transitions.len(),
            self.transitions.bytes(),
            self.field_caches.capacity() * size_of::<FieldCache>()
                + self.megamorphic_field_indices.capacity() * size_of::<u32>()
                + field_add_cache_bytes,
            self.megamorphic_fields.len(),
            self.method_caches.capacity() * size_of::<[MethodCache; 2]>(),
            self.megamorphic_methods.len(),
        );
        self.heap
            .memory_profile()
            .report(phase, cell_counts, live_payload_bytes, &self.heap);
        self.report_memory_snapshot(phase);
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn report_memory_snapshot(&self, phase: &str) {
        let heap = self.heap.memory_composition();
        let page_size = std::env::var("QUENCH_MEMORY_PAGE_SIZE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|page_size| page_size.is_power_of_two());
        let (
            complete_slot_slabs,
            fully_free_slot_slabs,
            partial_slot_slabs,
            fully_empty_slot_pages,
        ) = self.heap.slot_slab_census(page_size);
        let fully_empty_slot_pages =
            fully_empty_slot_pages.map_or_else(|| "null".to_string(), |count| count.to_string());
        let shape_table_bytes = self.shapes.capacity() * size_of::<Shape>();
        let shape_lookup_bytes = self
            .shapes
            .iter()
            .filter_map(|shape| shape.lookup_index.get())
            .map(|index| index.payload_capacity_bytes())
            .sum::<usize>();
        let transition_table_bytes = self.transitions.bytes();
        let descriptor_side_table_bytes = self.descriptors.capacity()
            * size_of::<((Value, property_key::PropertyKey), PropertyAttributes)>();
        let cache_side_table_bytes = self.field_caches.capacity() * size_of::<FieldCache>()
            + self.megamorphic_field_indices.capacity() * size_of::<u32>()
            + self.field_add_indices.capacity() * size_of::<u32>()
            + self.field_add_sets.capacity() * size_of::<FieldAddCacheSet>()
            + self.method_caches.capacity() * size_of::<[MethodCache; 2]>();
        let atom_side_table_bytes = self.atoms.capacity() * size_of::<(u64, Atom)>()
            + self.atom_collisions.capacity() * size_of::<(u64, Vec<Atom>)>()
            + self
                .atom_collisions
                .values()
                .map(|atoms| atoms.capacity() * size_of::<Atom>())
                .sum::<usize>()
            + self.dynamic_atoms.capacity() * size_of::<JsString>()
            + self
                .dynamic_atoms
                .iter()
                .map(JsString::capacity)
                .sum::<usize>();
        let frame_bytes = self
            .frames
            .iter()
            .chain(&self.frame_pool)
            .map(|frame| {
                frame.locals.capacity() * size_of::<Value>()
                    + frame.registers.capacity() * size_of::<Value>()
                    + frame.original_arguments.capacity() * size_of::<Value>()
            })
            .sum::<usize>();
        let property_arena_bytes =
            heap.property_arena_values_bytes + heap.property_arena_metadata_bytes;
        let vm_metadata_bytes = shape_table_bytes
            + shape_lookup_bytes
            + transition_table_bytes
            + descriptor_side_table_bytes
            + cache_side_table_bytes
            + atom_side_table_bytes
            + frame_bytes;
        let accounted_runtime_bytes = heap.slot_arena_reserved_bytes
            + heap.object_record_reserved_bytes
            + heap.array_record_reserved_bytes
            + heap.legacy_cell_record_reserved_bytes
            + heap.free_slot_metadata_bytes
            + property_arena_bytes
            + heap.cell_payload_bytes
            + heap.boxed_object_header_bytes
            + heap.sparse_array_sidecar_bytes
            + vm_metadata_bytes;
        eprintln!(
            "{{\"kind\":\"quench-memory-composition\",\"phase\":\"{phase}\",\"collection\":{},\"cell_kind_names\":{:?},\"cell_counts\":{:?},\"cell_variant_names\":{:?},\"cell_variant_counts\":{:?},\"occupied_slots\":{},\"complete_slot_slabs\":{},\"fully_free_slot_slabs\":{},\"partial_slot_slabs\":{},\"fully_empty_slot_pages\":{fully_empty_slot_pages},\"slot_arena_reserved_bytes\":{},\"object_record_reserved_bytes\":{},\"object_record_reusable_ids\":{},\"object_record_allocation_scan_cursor\":{},\"array_record_reserved_bytes\":{},\"array_record_reusable_ids\":{},\"array_record_allocation_scan_cursor\":{},\"legacy_cell_record_reserved_bytes\":{},\"free_slot_metadata_bytes\":{},\"object_headers\":{},\"embedded_object_headers\":{},\"boxed_object_headers\":{},\"boxed_object_header_bytes\":{},\"object_property_counts_by_width\":{:?},\"inline_property_attribution_bytes_overlapping_slots\":{},\"object_header_size_bytes\":{},\"cell_size_bytes\":{},\"slot_size_bytes\":{},\"property_arena_values_bytes\":{},\"property_arena_metadata_bytes\":{},\"property_arena_used_values\":{},\"property_arena_capacity_values\":{},\"live_property_used_values\":{},\"live_property_capacity_values\":{},\"live_property_used_values_by_width\":{:?},\"live_property_capacity_values_by_width\":{:?},\"dense_array_elements_bytes\":{},\"dense_array_headers_bytes\":{},\"dense_array_backing_count\":{},\"sparse_array_sidecar_bytes\":{},\"string_buffers_bytes\":{},\"string_buffer_count\":{},\"other_cell_payload_bytes\":{},\"cell_payload_total_bytes\":{},\"shape_table_bytes\":{},\"shape_lookup_bytes\":{},\"transition_table_bytes\":{},\"descriptor_side_table_bytes\":{},\"cache_side_table_bytes\":{},\"atom_side_table_bytes\":{},\"frame_bytes\":{},\"profile_instrumentation_bytes\":{},\"accounted_runtime_bytes\":{},\"accounted_with_profile_bytes\":{}}}",
            self.heap.collection_count(),
            CellKind::NAMES,
            heap.cell_counts,
            heap.cell_variant_names,
            heap.cell_variant_counts,
            heap.occupied_slots,
            complete_slot_slabs,
            fully_free_slot_slabs,
            partial_slot_slabs,
            heap.slot_arena_reserved_bytes,
            heap.object_record_reserved_bytes,
            heap.object_record_reusable_ids,
            heap.object_record_allocation_scan_cursor,
            heap.array_record_reserved_bytes,
            heap.array_record_reusable_ids,
            heap.array_record_allocation_scan_cursor,
            heap.legacy_cell_record_reserved_bytes,
            heap.free_slot_metadata_bytes,
            heap.object_headers,
            heap.embedded_object_headers,
            heap.boxed_object_headers,
            heap.boxed_object_header_bytes,
            heap.object_property_counts_by_width,
            heap.inline_property_attribution_bytes,
            heap.object_header_size_bytes,
            heap.cell_size_bytes,
            heap.slot_size_bytes,
            heap.property_arena_values_bytes,
            heap.property_arena_metadata_bytes,
            heap.property_arena_used_values,
            heap.property_arena_capacity_values,
            heap.live_property_used_values,
            heap.live_property_capacity_values,
            heap.live_property_used_values_by_width,
            heap.live_property_capacity_values_by_width,
            heap.dense_array_elements_bytes,
            heap.dense_array_headers_bytes,
            heap.dense_array_backing_count,
            heap.sparse_array_sidecar_bytes,
            heap.string_buffers_bytes,
            heap.string_buffer_count,
            heap.other_cell_payload_bytes,
            heap.cell_payload_bytes,
            shape_table_bytes,
            shape_lookup_bytes,
            transition_table_bytes,
            descriptor_side_table_bytes,
            cache_side_table_bytes,
            atom_side_table_bytes,
            frame_bytes,
            heap.profile_instrumentation_bytes,
            accounted_runtime_bytes,
            accounted_runtime_bytes + heap.profile_instrumentation_bytes,
        );
        crate::report_allocator_memory(phase);
    }
    fn initialize(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.initialize_shared(&Rc::new(program.clone()), builtins::RealmScope::Complete)
    }
    fn initialize_shared(
        &mut self,
        program: &Rc<ResidualProgram>,
        scope: builtins::RealmScope,
    ) -> Result<(), JsError> {
        self.specialized = program.specialized;
        self.realm.intrinsics.arguments_objects.clear();
        // The snapshot contains heap values, shapes and atom ids from one realm.
        // A host may build a builtin before sealing the replacement realm.
        self.startup_intrinsics = Default::default();
        self.shared_async_waiters.clear();
        self.heap.reset();
        self.natives.clear();
        self.shared_natives.clear();
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
        self.shapes.truncate(1);
        self.transitions.clear();
        // The sealed holders and their shapes belong to the discarded heap.
        self.startup_intrinsics = Default::default();
        self.atom_text = program.atoms.clone();
        self.atom_sequence = Rc::new(crate::bytecode::AtomSequence::new(self.atom_text.clone()));
        self.atoms.clear();
        self.atom_collisions.clear();
        self.array_index_atoms.clear();
        self.dynamic_atoms.clear();
        self.atom_classes.clear();
        self.regexp_subjects.clear();
        self.regexp_matchers.clear();
        self.atom_classes
            .resize_with(self.atom_text.len(), Default::default);
        self.dynamic_strings = None;
        self.symbol_registry.clear();
        self.well_known_symbols.clear();
        self.string_concats = None;
        *self.call_targets = [EMPTY_CALL_TARGET; CALL_TARGET_CACHE_SIZE];
        self.lane_stack.clear_script_targets();
        let atom_text = self.atom_text.clone();
        for (id, name) in atom_text.iter().enumerate() {
            self.index_atom(Self::atom_hash_str(name), id as Atom, name);
        }
        let lexical_this = self.intern_atom(LEXICAL_THIS_BINDING);
        let new_target = self.intern_atom(NEW_TARGET_BINDING);
        let regexp_flag_accessors =
            std::array::from_fn(|index| self.intern_atom(regexp::REGEXP_FLAG_ACCESSORS[index].0));
        self.runtime_atoms = RuntimeAtoms {
            lexical_this,
            new_target,
            regexp_exec: self.intern_atom("exec"),
            regexp_flags: self.intern_atom("flags"),
            last_index: self.intern_atom("lastIndex"),
            regexp_flag_accessors,
            then: self.intern_atom("then"),
            constructor: self.intern_atom("constructor"),
            name: self.intern_atom("name"),
        };
        self.field_caches = vec![EMPTY_CACHE; program.cache_sites as usize];
        Self::prime_loop_decoding(program);
        self.inherited_field_caches.clear();
        self.field_add_indices = vec![NO_FIELD_ADD_SET; program.cache_sites as usize];
        self.field_add_sets.clear();
        self.megamorphic_field_indices = vec![NO_MEGAMORPHIC_FIELD; program.cache_sites as usize];
        self.megamorphic_fields.clear();
        self.length_atom = self.intern_atom("length");
        self.prototype_atom = self.intern_atom("prototype");
        self.to_fixed_atom = self.intern_atom("toFixed");
        self.to_precision_atom = self.intern_atom("toPrecision");
        self.method_caches = vec![[EMPTY_METHOD_CACHE; 2]; program.method_sites.len()];
        self.megamorphic_methods.clear();
        self.method_cache_atoms = 0;
        self.object_shapes = vec![u32::MAX; program.object_sites.len()];
        self.descriptors.clear();
        self.function_values.clear();
        self.wasm_exported_functions.clear();
        self.private_homes.clear();
        self.programs.reset(program.clone());
        self.instanceof_constructor_layout = None;
        self.closure_shapes = [None; 2];
        self.regexp_last_index_layout = None;
        self.regexp_builtin_layout = None;
        self.instanceof_has_instance_holder = Value::NULL;
        self.program_cache_layouts.clear();
        self.program_cache_layouts.push(ProgramCacheLayout {
            shared_uncached_field: ProgramCacheLayout::shared_uncached_field(0, program),
            ..ProgramCacheLayout::default()
        });
        self.set_active_program(ProgramId::MAIN);
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
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("vm_shell");
        self.materialize_program_constants(ProgramId::MAIN, program);
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("program_constants");
        self.install_builtins(program, scope)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("runtime_builtins");
        self.initialize_host(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("host_bootstrap");
        Ok(())
    }
    fn materialize_constant(&mut self, constant: &Constant) -> Value {
        match constant {
            Constant::Number(v) => Value::number(*v),
            Constant::WasmBits64(bits) => self.encode_bits64(*bits),
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
        if id == self.active_program {
            self.set_active_program(id);
        }
    }
    pub(super) fn store_module_program(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let program = self.intern_program_atoms(program);
        let id = self.programs.insert_module(program)?;
        let residual = self.programs.get(id)?;
        self.append_program_cache_layout(id, &residual);
        self.materialize_program_constants(id, &residual);
        Some(id)
    }
    pub(super) fn store_dynamic_program(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let program = self.intern_program_atoms(program);
        self.store_dynamic_program_shared(Rc::new(program))
    }

    fn store_dynamic_program_shared(&mut self, program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = self.programs.insert_reclaimable_shared(program)?;
        let residual = self.programs.get(id)?;
        self.append_program_cache_layout(id, &residual);
        self.materialize_program_constants(id, &residual);
        Some(id)
    }
    /// Primes every looping function of an installed `program` to expand on first entry.
    fn prime_loop_decoding(program: &ResidualProgram) {
        for function in &program.functions {
            function.decoded.prime_loop(&function.code, &function.wide);
        }
    }

    fn append_program_cache_layout(&mut self, id: ProgramId, program: &ResidualProgram) {
        debug_assert_eq!(id.raw() as usize, self.program_cache_layouts.len());
        Self::prime_loop_decoding(program);
        self.program_cache_layouts.push(ProgramCacheLayout {
            shared_uncached_field: ProgramCacheLayout::shared_uncached_field(
                self.field_caches.len(),
                program,
            ),
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
        self.field_add_indices.resize(
            self.field_add_indices.len() + usize::from(program.cache_sites),
            NO_FIELD_ADD_SET,
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
    /// Makes `program` the executing program and caches its layout, the projection every cache
    /// index reads. A program's layout is appended before the program can run.
    #[inline(always)]
    pub(super) fn set_active_program(&mut self, program: ProgramId) {
        self.active_program = program;
        self.active_layout = self.program_cache_layouts[program.raw() as usize];
        self.active_constants = self.programs.constants(program);
    }

    /// Constant `index` of the active program.
    #[inline(always)]
    pub(super) fn active_constant(&self, index: usize) -> Option<Value> {
        debug_assert!(std::ptr::eq(
            self.active_constants,
            self.programs.constants(self.active_program)
        ));
        // SAFETY: `active_constants` is re-derived whenever the active program or its constants
        // change, and an installed constant buffer stays put until the store is reset, which
        // reactivates the main program.
        unsafe { (&*self.active_constants).get(index).copied() }
    }

    #[inline(always)]
    pub(super) fn replace_active_program(&mut self, program: ProgramId) -> ProgramId {
        let previous = self.active_program;
        self.set_active_program(program);
        previous
    }

    #[inline(always)]
    fn active_cache_layout(&self) -> ProgramCacheLayout {
        debug_assert_eq!(
            self.active_layout,
            self.program_cache_layouts[self.active_program.raw() as usize]
        );
        self.active_layout
    }
    #[inline(always)]
    pub(super) fn field_cache_index(&self, site: u16) -> usize {
        self.active_cache_layout().field_base + usize::from(site)
    }
    /// Whether the active program may record cache entries at `index`: every index but its
    /// shared overflow site, if it has one.
    pub(super) fn field_cache_records(&self, index: usize) -> bool {
        self.active_layout.shared_uncached_field != Some(index)
    }
    pub(super) fn method_cache_index(&self, site: usize) -> usize {
        self.active_cache_layout().method_base + site
    }
    pub(super) fn object_cache_index(&self, site: usize) -> usize {
        self.active_cache_layout().object_base + site
    }
    /// Interns the atoms `program` adds to the VM's; its atom ids are the VM's, so it then reads
    /// every name through the shared sequence instead of keeping its own copy.
    fn intern_program_atoms(&mut self, mut program: ResidualProgram) -> ResidualProgram {
        let first_new_atom = self.atom_text.len() + self.dynamic_atoms.len();
        for (index, name) in program.atoms.iter().enumerate().skip(first_new_atom) {
            let atom = self.intern_atom(name);
            debug_assert_eq!(atom as usize, index);
        }
        debug_assert!(
            program
                .atoms
                .iter()
                .enumerate()
                .all(|(atom, name)| self.atom_name(atom as u32) == name),
            "a dynamic program's atoms are a prefix of the VM's"
        );
        program.atoms =
            crate::bytecode::AtomTable::shared(self.atom_sequence.clone(), program.atoms.len());
        program
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
                let active_program = self.replace_active_program(program_id);
                let realm = match self.heap.get(callee) {
                    Some(Cell::Function { object, .. }) => object.realm,
                    _ => self.realm.globals,
                };
                let current_global = self.switch_realm_global(realm);
                let result = if program
                    .functions
                    .get(id as usize)
                    .is_some_and(|function| function.is_class_constructor)
                    && !self
                        .construct_target
                        .is_some_and(|target| target.callee == callee)
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
                self.set_active_program(active_program);
                self.switch_realm_global(current_global);
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
                object,
                ..
            }) => (*program_id, *id, *env, object.realm),
            _ => return Err(self.type_error(p, "constructor is not a user function".into())),
        };
        let program = self.programs.get(program_id).ok_or_else(|| {
            self.type_error(p, "function belongs to an unavailable program".into())
        })?;
        let previous_program = self.replace_active_program(program_id);
        let previous_global = self.switch_realm_global(realm);
        let outcome = self.call_user_construct_frame(
            &program,
            id,
            env,
            args,
            CallContext::user_function(id, callee),
        );
        self.set_active_program(previous_program);
        self.switch_realm_global(previous_global);
        match outcome? {
            FrameOutcome::ConstructComplete { value, this } => Ok((value, this)),
            _ => Err(JsError(
                "constructor activation did not complete synchronously".into(),
            )),
        }
    }
}
