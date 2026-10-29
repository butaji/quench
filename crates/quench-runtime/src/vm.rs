mod compiled;
mod js_compiled;
mod jit_backend;
mod shape_transitions;
mod snapshot;
mod startup_intrinsics;
mod wasm;
pub use compiled::ExecutionMode;
mod wasm_exception;
mod wasm_gc;
mod wasm_host;
mod wasm_js;
pub(crate) use wasm_js::{js_tag_cell, js_tag_type};
mod wasm_table;
use crate::Value;
use crate::bytecode::{
    Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, LEXICAL_THIS_BINDING,
    NEW_TARGET_BINDING, Op, Operand, Register, ResidualProgram, WideInstruction,
};
#[cfg(feature = "profile-memory")]
use crate::heap::CellKind;
use crate::heap::{
    CallSiteLocation, CallSiteName, CallSiteRecord, Cell, FunctionKind, FunctionRecord, GcTraceKind, IteratorRecord, Heap, IteratorConsumer,
    IteratorHelper, IteratorKind, Native, Object, ProtoLink, ProxyKind, RootId, StackData, TypedArrayKind,
    WeakHandle,
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

/// A fresh seed for `Math.random`'s xorshift state, from the process's random hash keys (which
/// the standard library draws from the OS): each VM, like each Node process, has its own
/// sequence. The state must not be zero, which xorshift never leaves.
fn fresh_random_seed() -> u64 {
    use std::hash::BuildHasher as _;
    std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now()) | 1
}
pub(super) const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
pub(super) const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;
pub(super) const ROOT_FUNCTION_ID: u32 = 0;
/// The values a call roots besides its arguments: the callee and the receiver.
const CALL_ROOTS_BESIDE_ARGUMENTS: usize = 2;

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
    /// The iterator protocol's step method.
    next: Atom,
    /// Canonical key for named function-info lookup; paired with length_atom.
    name: Atom,
    /// The empty string, an anonymous function's initial name.
    empty: Atom,
    /// An iterator result's keys, in its literal order.
    value: Atom,
    done: Atom,
    /// The iterator protocol's close method.
    iterator_return: Atom,
    /// A property descriptor object's other keys (`value` above), which
    /// `Object.getOwnPropertyDescriptor` writes on every call.
    writable: Atom,
    enumerable: Atom,
    configurable: Atom,
    get: Atom,
    set: Atom,
    /// The internal keys a bound function's environment holds its parts under
    /// (`Vm::bound_function_parts`).
    bound_target: Atom,
    bound_this: Atom,
    bound_args: Atom,
    /// `String.prototype.charCodeAt`, whose intactness a host's string scan checks
    /// (`Vm::embedding_string_char_code_at_intact`).
    char_code_at: Atom,
}
pub(crate) mod activation;
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
mod call_printer;
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
#[cfg(feature = "profile-lane-exits")]
mod lane_exits;
mod embedding;
mod provenance;
mod inspection;
mod environment;
mod equality;
mod error;
mod eval;
mod fdlibm_math;
mod field_cache;
mod site_table;
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
mod lazy_data_property;
mod local_time;
mod property_definition;
use group_by::GroupByKind;
use property_definition::PropertyDefinitionKind;
mod iterators;
mod json;
mod method_cache;
mod plain_equality;
mod module;
mod number;
mod object_template;
mod object;
mod shape_memo;
mod symbol_write_proof;
mod symbol_load_proof;
mod private_site_home;
mod enumerable_key_memo;
mod name_atom_memo;
mod object_assignment;
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
use call_arguments::CallArguments;
use object::AtomClass;
pub(crate) use program_store::ProgramId;
pub(crate) use regexp::RegExpFlags;
use program_store::{ModuleImport, ProgramStore};
use promise::PromiseRuntime;
use property_key::PropertyKey;
mod operations;
pub(crate) use operations::number_is_truthy;
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
mod value_serializer;
mod owned_clone_native;
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
pub use value_serializer::{
    write_double, write_raw_bytes, write_uint32, write_uint64, HostObjectRead, SerializedValue, SharedWireInput, ValueDeserializer,
    ValueSerializer, WireInput, WireOutput, WIRE_FORMAT_VERSION,
};
pub(crate) use value_serializer::{ReadHooks, WriteHooks};
use wtf16::JsString;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod closure_retention_tests;
#[derive(Clone)]
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
    /// Skip one function-entry tier attempt after a native continuation stopped
    /// on an already-executed backedge to PC zero.
    pub(super) js_jit_skip_entry_once: bool,
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
    /// Opt-in interpreter time for a synchronous JIT-diagnostic activation.
    /// Excludes compile work, native execution, suspended and exceptional exits.
    js_jit_interpreter_started: Option<std::time::Instant>,
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

/// Capture walks `Vm::walked_capture_slots` remembers, by start environment and depth: enough
/// for the closures of a few nested scopes reading their outer bindings in turn.
const CAPTURE_WALKS: usize = 64;
/// Closure templates `Vm::closure_in_realm` remembers, one per function id modulo the count:
/// the nested functions a closure-heavy region makes again on each call of its parent. A
/// conflict overwrites the entry, which only costs the displaced function's next closure
/// the lookups the template saves.
const CLOSURE_TEMPLATES: usize = 64;
const _: () = assert!(CLOSURE_TEMPLATES.is_power_of_two());
const _: () = assert!(CAPTURE_WALKS.is_power_of_two());

/// A remembered capture walk: `depth` layers out from `cache.env`, as `CaptureCache` holds
/// the innermost one.
#[derive(Clone, Copy, Default)]
pub(super) struct CaptureWalk {
    depth: u16,
    cache: CaptureCache,
}

impl Frame {
    fn source_instruction_pc(&self) -> usize {
        if self.pc_is_resume { self.pc.saturating_sub(1) } else { self.pc }
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
            js_jit_skip_entry_once: false,
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
            js_jit_interpreter_started: None,
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
        self.js_jit_skip_entry_once = false;
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
        self.js_jit_skip_entry_once = false;
        let register_count = usize::from(function.registers);
        if !function.register_roots.covered()
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
        let rest = function.rest.then(|| function.params - 1);
        for entry in &function.local_registers {
            // A promoted parameter starts as its argument, a promoted rest parameter as the
            // array its slot holds (`bind_activation_arguments`), and a promoted `var` as
            // undefined.
            let initial = if Some(entry.local) == rest {
                self.locals[usize::from(entry.local)]
            } else if entry.local < function.params {
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
    Thenable(promise::ThenableJob),
}

impl PendingJob {
    /// Pushes the values the job holds onto `roots`, which keep them alive while it runs.
    fn push_roots(&self, roots: &mut Vec<Value>) {
        match self {
            Self::Call {
                callback,
                this,
                args,
            } => {
                roots.extend([*callback, *this]);
                roots.extend_from_slice(args);
            }
            Self::Reaction(job) => roots.extend([job.realm, job.handler, job.next, job.value]),
            Self::Thenable(job) => roots.extend([job.realm, job.then, job.thenable, job.promise]),
        }
    }
}
struct Realm {
    globals: Value,
    /// Every global lexical name of the realm, including every bound one
    /// (`Realm::bind_global_lexical`), so it alone answers whether a name is one.
    global_lexical_declarations: FxHashSet<Atom>,
    global_lexical_bindings: FxHashMap<Atom, Value>,
    immutable_global_lexical_bindings: FxHashSet<Atom>,
    /// Every inactive realm's global lexical state, keyed by its global, except the realm
    /// last switched away from, which `parked` holds.
    global_lexical_states: FxHashMap<Value, GlobalLexicalState>,
    /// The realm last switched away from, so switching back to it, which jobs and calls
    /// across two realms do on every turn, moves its state without hashing.
    parked: Option<ParkedRealm>,
    intrinsics: RealmIntrinsics,
    jobs: std::collections::VecDeque<PendingJob>,
    template_objects: FxHashMap<(ProgramId, u32, u32), Value>,
    promise: PromiseRuntime,
}
impl Realm {
    /// Binds global lexical `atom` to `value`, declaring a name bound for the first time.
    fn bind_global_lexical(&mut self, atom: Atom, value: Value) {
        if self.global_lexical_bindings.insert(atom, value).is_none() {
            self.global_lexical_declarations.insert(atom);
        }
    }
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
/// An inactive realm with its global lexical state and the prototypes the realm-following
/// VM fields held while it was active (`REALM_FOLLOWING_PROTOTYPES`).
struct ParkedRealm {
    global: Value,
    state: GlobalLexicalState,
    prototypes: [Value; REALM_FOLLOWING_PROTOTYPES.len()],
}
/// The natives whose prototypes VM fields follow from the active realm
/// (`Vm::realm_prototype_field`).
/// One `typeof` name per `crate::ValueType`, indexed by its discriminant; `Function` is the
/// last type.
const TYPEOF_NAMES: usize = crate::ValueType::Function as usize + 1;
const REALM_FOLLOWING_PROTOTYPES: [Native; 4] =
    [Native::Object, Native::Function, Native::Array, Native::String];
#[derive(Clone, Copy, PartialEq, Eq)]
struct FieldCache {
    receiver: u32,
    holder: u32,
    slot: u16,
}

impl FieldCache {
    /// The entry of an own data field at `slot` of receivers of exact shape `receiver`.
    #[inline(always)]
    const fn own(receiver: u32, slot: u16) -> Self {
        Self { receiver, holder: NO_FIELD_HOLDER, slot }
    }
}
/// The internal keys of a bound function's environment object.
const BOUND_TARGET_KEY: &str = "\0quench:bound-target";
const BOUND_THIS_KEY: &str = "\0quench:bound-this";
const BOUND_ARGS_KEY: &str = "\0quench:bound-args";
const NO_FIELD_HOLDER: u32 = u32::MAX;
/// A field site's read of an accessor of a shaped receiver, its own or on a prototype: it
/// holds while the receiver keeps its shape and prototype and the chain to `holder` keeps the
/// layout it was proved under, `holder`'s `slot` attributes carrying the getter.
#[derive(Clone, Copy)]
struct AccessorFieldCache {
    receiver: u32,
    /// The receiver's prototype, where the chain to `holder` starts.
    prototype: Value,
    /// The prototype holding the accessor, or `OWN_ACCESSOR` for the receiver itself, whose
    /// own slot each receiver of the shape fills with its own accessor.
    holder: Value,
    slot: u16,
    /// Whether the holder's shape keeps the getter in the slot itself
    /// (`ShapeAttributes::slot_getter`): the shape the entry proves (the receiver's for
    /// `OWN_ACCESSOR`, else the prototype holder's, which the epoch covers) fixes it.
    getter_in_slot: bool,
    /// The `prototype_layout_epoch` the chain was proved under.
    epoch: u64,
}
/// Prototypes an inherited field load may traverse to its holder, or to the end of the chain
/// for an absent name. Node's stream objects (an `IncomingMessage` above `Readable`, `Stream`,
/// `EventEmitter` and `Object.prototype`) sit five prototypes above their instances, and the
/// Babel parser's methods up to a dozen (its parser classes and plugin mixins); deeper chains
/// stay uncached.
const INHERITED_FIELD_CACHE_MAX_DEPTH: usize = 16;
/// A field load answered `depth` prototypes above a receiver of shape `receiver`: the chain
/// still has exactly `prototype_shapes[..depth]`, and `answer` says what that chain holds.
#[derive(Clone, Copy)]
struct InheritedFieldCache {
    receiver: u32,
    atom: Atom,
    prototype_shapes: [u32; INHERITED_FIELD_CACHE_MAX_DEPTH],
    depth: u8,
    /// Whether a function may stand on the chain (`Vm::function_links_answer` of the name).
    function_links: bool,
    answer: InheritedFieldAnswer,
    /// The `prototype_layout_epoch` under which a walk last proved the chain from
    /// `proved_prototype` up to `proved_holder`, its last recorded prototype, or
    /// `NO_PROVED_EPOCH`; while both hold the chain needs no walk.
    proved_epoch: u64,
    /// The receiver's prototype link the proof holds for, compared undecoded; a
    /// collection advances the epoch, so it never outlives the record it names.
    proved_prototype: ProtoLink,
    proved_holder: Value,
    /// The site's next older entry, or `NO_INHERITED_FIELD`: a site keeps up to
    /// `INHERITED_FIELD_CACHE_WAYS` receiver shapes, newest first.
    next: u32,
}
/// Receiver shapes one site's inherited entries remember: a method inherited from beyond the
/// immediate prototype read on instances of several subclasses (DeltaBlue's constraints)
/// reaches one site with a shape per class. A further shape reuses the oldest entry.
const INHERITED_FIELD_CACHE_WAYS: usize = 4;
// Reusing the oldest entry unlinks it from an older-than-head predecessor.
const _: () = assert!(INHERITED_FIELD_CACHE_WAYS > 1);
#[derive(Clone, Copy, PartialEq, Eq)]
enum InheritedFieldAnswer {
    /// The last recorded prototype holds the data property at this slot.
    Slot(u16),
    /// No recorded shape has the name and the last prototype's prototype is null: the load
    /// is `undefined`.
    Absent,
}
const NO_FIELD_RECEIVER: u32 = u32::MAX;
/// A cache site without an inherited entry (`Vm::inherited_field_indices`).
const NO_INHERITED_FIELD: u32 = u32::MAX;
// A cached add proves every prototype up to this depth. Node's stream objects (an
// `IncomingMessage` above `Readable`, `Stream`, `EventEmitter` and `Object.prototype`) add
// their fields five prototypes below the chain's end; deeper chains stay uncached. Every
// entry carries this many shapes, and recorded adds proved at most four (V8-v7, npm
// TypeScript and ESLint census, `field_add_cache_depth_counts`, 2026-10-23), so eight keeps
// the streams' five with room and halves the sixteen an inherited read keeps.
const FIELD_ADD_CACHE_MAX_PROTO_DEPTH: usize = 8;
/// Bits of a keyed add's index into `Vm::keyed_adds`.
const KEYED_ADD_INDEX_BITS: u32 = 6;
/// Keyed adds the VM remembers, direct-mapped by source shape and key: express's JSON body
/// path makes fifteen a request into header and parameter objects, whose shapes repeat.
const KEYED_ADD_ENTRIES: usize = 1 << KEYED_ADD_INDEX_BITS;
/// Source shapes one add site remembers; the oldest is evicted beyond this. Objects whose
/// optional fields are assigned in varying order reach one add site with several shapes.
const FIELD_ADD_CACHE_WAYS: usize = 4;
const EMPTY_CACHE: FieldCache = FieldCache {
    receiver: NO_FIELD_RECEIVER,
    holder: NO_FIELD_HOLDER,
    slot: 0,
};
/// Stored as `[receiver, holder, slot]` complemented against `EMPTY_CACHE`, which stores
/// as zero words.
impl site_table::SiteEntry for FieldCache {
    type Stored = [u32; 3];
    #[inline(always)]
    fn store(self) -> [u32; 3] {
        [
            self.receiver ^ EMPTY_CACHE.receiver,
            self.holder ^ EMPTY_CACHE.holder,
            u32::from(self.slot ^ EMPTY_CACHE.slot),
        ]
    }
    #[inline(always)]
    fn load([receiver, holder, slot]: [u32; 3]) -> Self {
        Self {
            receiver: receiver ^ EMPTY_CACHE.receiver,
            holder: holder ^ EMPTY_CACHE.holder,
            slot: slot as u16 ^ EMPTY_CACHE.slot,
        }
    }
}
/// Layout facts for generated code that reads a field cache entry as `SiteEntry for
/// FieldCache` stores it: three words, each complemented against `EMPTY_CACHE`.
pub(super) struct NativeFieldCacheLayout {
    pub(super) entry_size: usize,
    pub(super) receiver_offset: usize,
    pub(super) holder_offset: usize,
    pub(super) slot_offset: usize,
    pub(super) receiver_complement: u32,
    pub(super) holder_complement: u32,
    pub(super) slot_complement: u32,
    /// The holder an own data field's entry names.
    pub(super) own_holder: u32,
}

pub(super) const NATIVE_FIELD_CACHE_LAYOUT: NativeFieldCacheLayout = NativeFieldCacheLayout {
    entry_size: std::mem::size_of::<<FieldCache as site_table::SiteEntry>::Stored>(),
    receiver_offset: 0,
    holder_offset: std::mem::size_of::<u32>(),
    slot_offset: 2 * std::mem::size_of::<u32>(),
    receiver_complement: EMPTY_CACHE.receiver,
    holder_complement: EMPTY_CACHE.holder,
    slot_complement: EMPTY_CACHE.slot as u32,
    own_holder: NO_FIELD_HOLDER,
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
    /// The `prototype_layout_epoch` under which a walk last proved the chain from
    /// `proved_prototype`, or `NO_PROVED_EPOCH`; while both hold the chain needs no walk.
    proved_epoch: u64,
    /// The receiver's prototype link the proof holds for, compared undecoded; a collection
    /// advances the epoch, so it never outlives the record it names.
    proved_prototype: ProtoLink,
}
/// A `FieldAddCache::proved_epoch` no epoch reaches: the chain is unproven.
const NO_PROVED_EPOCH: u64 = u64::MAX;
impl FieldAddCache {
    const EMPTY: Self = Self {
        atom: u32::MAX,
        source_shape: NO_FIELD_RECEIVER,
        target_shape: NO_FIELD_RECEIVER,
        slot: 0,
        prototype_depth: 0,
        prototype_shapes: [NO_FIELD_RECEIVER; FIELD_ADD_CACHE_MAX_PROTO_DEPTH],
        proved_epoch: NO_PROVED_EPOCH,
        proved_prototype: ProtoLink::NULL,
    };
    fn prototype_shapes(&self) -> &[u32] {
        &self.prototype_shapes[..usize::from(self.prototype_depth)]
    }
    /// Whether the chain proof (`proved_epoch`, `proved_prototype`) holds for a receiver
    /// whose prototype `link` names: then the chain needs no walk.
    #[inline(always)]
    fn proof_holds(&self, link: ProtoLink) -> bool {
        self.proved_epoch == crate::heap::prototype_layout_epoch() && self.proved_prototype == link
    }
}
/// The add transitions one site remembers, oldest first. The first lives in the set and the
/// rest, up to `FIELD_ADD_CACHE_WAYS`, in a box made when a second arrives: a site is nearly
/// always monomorphic (1413 of 1426 sets in npm TypeScript, every set but one in V8-v7, census
/// 2026-10-23), and four inline ways cost each site the room of four entries.
struct FieldAddCacheSet {
    len: u8,
    first: FieldAddCache,
    rest: Option<Box<[FieldAddCache; FIELD_ADD_CACHE_WAYS - 1]>>,
}
impl FieldAddCacheSet {
    const EMPTY: Self = Self {
        len: 0,
        first: FieldAddCache::EMPTY,
        rest: None,
    };
    /// The entries after the first, as many as the set holds.
    #[inline(always)]
    fn rest(&self) -> &[FieldAddCache] {
        match &self.rest {
            Some(rest) => &rest[..usize::from(self.len).saturating_sub(1)],
            None => &[],
        }
    }
    /// Every entry, oldest first.
    fn entries(&self) -> impl Iterator<Item = &FieldAddCache> {
        let first = (self.len > 0).then_some(&self.first);
        first.into_iter().chain(self.rest())
    }
    #[inline(always)]
    fn find(&self, source_shape: u32, atom: Atom) -> Option<&FieldAddCache> {
        let matches = |cache: &&FieldAddCache| cache.source_shape == source_shape && cache.atom == atom;
        if self.len > 0 && matches(&&self.first) {
            return Some(&self.first);
        }
        self.rest().iter().find(matches)
    }
    /// `find`, preferring the entry whose chain proof holds for a receiver whose prototype
    /// `link` names: receivers of one shape under different prototypes (subclasses whose
    /// constructors share their parent's stores) each keep an entry of their chain.
    #[inline(always)]
    fn find_for(&self, source_shape: u32, atom: Atom, link: ProtoLink) -> Option<&FieldAddCache> {
        let first = self.find(source_shape, atom)?;
        if first.proof_holds(link) {
            return Some(first);
        }
        let proved = |cache: &&FieldAddCache| {
            cache.source_shape == source_shape && cache.atom == atom && cache.proof_holds(link)
        };
        Some(self.rest().iter().find(proved).unwrap_or(first))
    }
    /// The indexes of the entries for receivers of `source_shape` adding `atom`, one per
    /// prototype chain the site recorded.
    fn matching(&self, source_shape: u32, atom: Atom) -> impl Iterator<Item = usize> + '_ {
        self.entries()
            .enumerate()
            .filter(move |(_, cache)| cache.source_shape == source_shape && cache.atom == atom)
            .map(|(index, _)| index)
    }
    fn entry(&self, index: usize) -> &FieldAddCache {
        if index == 0 { &self.first } else { &self.rest()[index - 1] }
    }
    fn entry_mut(&mut self, index: usize) -> &mut FieldAddCache {
        if index == 0 {
            &mut self.first
        } else {
            &mut self.rest.as_deref_mut().expect("a later entry lives in the box")[index - 1]
        }
    }
    /// Replaces the entry for the same source shape, atom and prototype chain shapes, else
    /// appends, evicting the oldest entry when the set is full.
    fn insert(&mut self, cache: FieldAddCache) {
        let mut entries = [FieldAddCache::EMPTY; FIELD_ADD_CACHE_WAYS];
        let len = usize::from(self.len);
        for (slot, entry) in entries.iter_mut().zip(self.entries()) {
            *slot = *entry;
        }
        let kept = match entries[..len]
            .iter()
            .position(|entry| {
                entry.source_shape == cache.source_shape
                    && entry.atom == cache.atom
                    && entry.prototype_shapes() == cache.prototype_shapes()
            })
        {
            Some(index) => {
                entries.copy_within(index + 1..len, index);
                len - 1
            }
            None if len == FIELD_ADD_CACHE_WAYS => {
                entries.copy_within(1..len, 0);
                len - 1
            }
            None => len,
        };
        entries[kept] = cache;
        let len = kept + 1;
        self.first = entries[0];
        if len > 1 {
            let rest = self.rest.get_or_insert_with(|| Box::new([FieldAddCache::EMPTY; FIELD_ADD_CACHE_WAYS - 1]));
            rest[..len - 1].copy_from_slice(&entries[1..len]);
        }
        self.len = len as u8;
    }
}
const NO_FIELD_ADD_SET: u32 = u32::MAX;
const NO_MEGAMORPHIC_FIELD: u32 = u32::MAX;
/// The share of all cache sites, as a divisor, up to which an invalidation resets the
/// written sites one by one: each costs four scattered stores against a sequential fill of
/// every site's entries (Node's library wires prototypes hundreds of times at load, each an
/// invalidation that had refilled every table).
const FIELD_CACHE_SPARSE_RESET_SHARE: usize = 8;
const FIELD_MEGAMORPHIC_INLINE: usize = 4;
// Bound each site's overflow to 256 recorded shapes; unseen shapes keep the generic fallback.
const FIELD_MEGAMORPHIC_LIMIT: usize = 256;
struct FieldCacheSet {
    len: u8,
    /// Whether any entry recorded an own field (`NO_FIELD_HOLDER`); a set without one names
    /// only prototype holders, so a read probes no own entry.
    holds_own: bool,
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
    fn shared_uncached_field(field_base: usize, field_sites: u16) -> Option<usize> {
        (field_sites > crate::bytecode::SHARED_UNCACHED_SITE)
            .then(|| field_base + usize::from(crate::bytecode::SHARED_UNCACHED_SITE))
    }
}
/// The well-known symbols (ECMA-262 Table 1), in their installation order on `Symbol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WellKnownSymbol {
    AsyncDispose,
    AsyncIterator,
    Dispose,
    HasInstance,
    IsConcatSpreadable,
    Iterator,
    Match,
    MatchAll,
    Metadata,
    Replace,
    Search,
    Species,
    Split,
    ToPrimitive,
    ToStringTag,
    Unscopables,
}

impl WellKnownSymbol {
    pub(crate) const ALL: [Self; 16] = [
        Self::AsyncDispose,
        Self::AsyncIterator,
        Self::Dispose,
        Self::HasInstance,
        Self::IsConcatSpreadable,
        Self::Iterator,
        Self::Match,
        Self::MatchAll,
        Self::Metadata,
        Self::Replace,
        Self::Search,
        Self::Species,
        Self::Split,
        Self::ToPrimitive,
        Self::ToStringTag,
        Self::Unscopables,
    ];

    /// The property name on `Symbol`; the description is `Symbol.` and this name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::AsyncDispose => "asyncDispose",
            Self::AsyncIterator => "asyncIterator",
            Self::Dispose => "dispose",
            Self::HasInstance => "hasInstance",
            Self::IsConcatSpreadable => "isConcatSpreadable",
            Self::Iterator => "iterator",
            Self::Match => "match",
            Self::MatchAll => "matchAll",
            Self::Metadata => "metadata",
            Self::Replace => "replace",
            Self::Search => "search",
            Self::Species => "species",
            Self::Split => "split",
            Self::ToPrimitive => "toPrimitive",
            Self::ToStringTag => "toStringTag",
            Self::Unscopables => "unscopables",
        }
    }
}

/// The realm-independent well-known symbol values, one slot per `WellKnownSymbol`, empty
/// until `Symbol` is installed.
#[derive(Clone, Default)]
pub(crate) struct WellKnownSymbols([Option<Value>; WellKnownSymbol::ALL.len()]);

impl WellKnownSymbols {
    #[inline(always)]
    pub(crate) fn get(&self, symbol: WellKnownSymbol) -> Option<Value> {
        self.0[symbol as usize]
    }
    pub(crate) fn insert(&mut self, symbol: WellKnownSymbol, value: Value) {
        self.0[symbol as usize] = Some(value);
    }
    /// The symbol whose `Symbol` property name is `name`.
    pub(crate) fn named(&self, name: &str) -> Option<Value> {
        WellKnownSymbol::ALL
            .into_iter()
            .find(|symbol| symbol.name() == name)
            .and_then(|symbol| self.get(symbol))
    }
    /// The installed symbols with their names, in `WellKnownSymbol::ALL` order.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&'static str, Value)> + '_ {
        WellKnownSymbol::ALL
            .into_iter()
            .filter_map(|symbol| self.get(symbol).map(|value| (symbol.name(), value)))
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = Value> + '_ {
        self.0.iter().flatten().copied()
    }
    pub(crate) fn clear(&mut self) {
        self.0 = [None; WellKnownSymbol::ALL.len()];
    }
}

impl std::ops::Index<WellKnownSymbol> for WellKnownSymbols {
    type Output = Value;
    fn index(&self, symbol: WellKnownSymbol) -> &Value {
        self.0[symbol as usize]
            .as_ref()
            .expect("well-known symbols are installed with Symbol")
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
    push: InLoopPush,
}
/// How the dispatch loop pushes an in-loop callee.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InLoopPush {
    /// An ordinary body (`CallPlan::in_loop`), whose return is the call's value.
    Body,
    /// An async body that never suspends (`CallPlan::settles_in_loop`), whose completion
    /// settles the promise the call returns; only a plain call pushes it.
    Settles,
}
const EMPTY_CALL_TARGET: InLoopCallTarget = InLoopCallTarget {
    callee: Value::UNDEFINED,
    epoch: 0,
    realm: Value::UNDEFINED,
    program: ProgramId::MAIN,
    function: 0,
    env: Value::UNDEFINED,
    push: InLoopPush::Body,
};
const STRING_CONCAT_CACHE_SIZE: usize = 64;
/// Slots of `Vm::string_atoms`, a power of two.
const STRING_ATOM_CACHE_SIZE: usize = 256;
/// Slots of `Vm::recent_atom_strings`, a power of two: as many as `string_atoms` keeps the
/// other way.
const ATOM_STRING_CACHE_SIZE: usize = STRING_ATOM_CACHE_SIZE;
/// An empty `Vm::recent_atom_strings` slot: no atom has this id.
const NO_RECENT_ATOM: Atom = Atom::MAX;
/// The code units below which a unit is ASCII.
const ASCII_UNITS: usize = 0x80;
#[derive(Clone, Copy)]
struct StringConcatCache {
    left: Value,
    right: Value,
    result: Value,
}
#[derive(Clone, Copy)]
enum ShapeTransition {
    Root,
    FunctionName { name: Atom },
    FunctionNameReconfigured,
    FunctionLengthReconfigured,
    Constructed { constructor: Value, new_target_is_base: bool },
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
    /// An added data property. A symbol key names a symbol a live shape keeps alive
    /// (`append_shape_roots`); compaction rebuilds the index from live shapes only, so no
    /// entry outlives the symbol it names.
    Add(property_key::PropertyKey, DataAttributes),
    /// A descriptor whose shape names no accessor function (`ShapeAttributes::table_accessors`
    /// is false).
    Descriptor {
        slot: u32,
        attributes: ShapeAttributes,
    },
    /// A deleted property named by an atom. A deleted symbol key is in no live shape's
    /// attributes, which root the symbols an index entry names (`Add`), so its deletion is
    /// not indexed.
    Delete(property_key::PropertyKey),
}
/// A property's attributes as a shape records them. A getter-only accessor keeps its getter
/// in the property's slot (`SLOT_GETTER`), so its shape names no function and every object
/// defining the same keys shares it. Other accessor functions are rare among shapes and live
/// in `Vm::shape_accessors`, which `accessors` indexes, so every shape stays small.
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
/// The attributes of a function's own `length` and `name`.
const FUNCTION_INFO_ATTRIBUTES: DataAttributes = DataAttributes {
    writable: false,
    enumerable: false,
    configurable: true,
};
/// `ShapeAttributes::accessors` of attributes with neither getter nor setter.
const NO_SHAPE_ACCESSORS: u32 = u32::MAX;
/// `ShapeAttributes::accessors` of an accessor with a getter and no setter: the getter is the
/// property's slot value. Re-exports (`__createBinding`, `__export`, Babel's interop getters)
/// define these by the thousand, each with its own getter.
const SLOT_GETTER: u32 = u32::MAX - 1;
/// A shape-held accessor property's getter and setter.
type ShapeAccessors = (Option<Value>, Option<Value>);
impl ShapeAttributes {
    /// `attributes` for a shape, appending its accessor functions to `accessors`.
    /// A `SLOT_GETTER` record leaves storing the getter in the slot to the caller.
    fn record(attributes: PropertyAttributes, accessors: &mut Vec<ShapeAccessors>) -> Self {
        let index = match (attributes.getter, attributes.setter) {
            (None, None) => NO_SHAPE_ACCESSORS,
            (Some(_), None) if attributes.accessor => SLOT_GETTER,
            (getter, setter) => {
                accessors.push((getter, setter));
                u32::try_from(accessors.len() - 1).expect("shape accessor table exhausted")
            }
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

    /// These attributes, their table accessors recorded anew from `old` into `accessors`.
    fn rehomed(self, old: &[ShapeAccessors], accessors: &mut Vec<ShapeAccessors>) -> Self {
        if !self.table_accessors() {
            return self;
        }
        accessors.push(self.functions(old, || None));
        Self {
            accessors: u32::try_from(accessors.len() - 1).expect("shape accessor table exhausted"),
            ..self
        }
    }

    /// Whether the property's slot holds its getter.
    fn slot_getter(self) -> bool {
        self.accessors == SLOT_GETTER
    }

    /// Whether `Vm::shape_accessors` holds this property's functions.
    fn table_accessors(self) -> bool {
        !matches!(self.accessors, NO_SHAPE_ACCESSORS | SLOT_GETTER)
    }

    /// The accessor functions, given the property's slot value for a `SLOT_GETTER` record.
    fn functions(self, accessors: &[ShapeAccessors], slot_value: impl FnOnce() -> Option<Value>) -> ShapeAccessors {
        if self.slot_getter() {
            return (slot_value(), None);
        }
        accessors
            .get(self.accessors as usize)
            .copied()
            .unwrap_or((None, None))
    }

    fn attributes(self, accessors: &[ShapeAccessors], slot_value: impl FnOnce() -> Option<Value>) -> PropertyAttributes {
        let (getter, setter) = self.functions(accessors, slot_value);
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
            Self::Descriptor { attributes, .. } => attributes.table_accessors(),
            Self::Root | Self::FunctionName { .. } | Self::FunctionNameReconfigured | Self::FunctionLengthReconfigured | Self::Constructed { .. } | Self::Delete { .. } | Self::Vacant | Self::Dictionary { .. } => false,
        }
    }

    fn cache_key(self) -> Option<ShapeTransitionKey> {
        match self {
            Self::FunctionName { name } => Some(ShapeTransitionKey::FunctionName(name)),
            Self::FunctionNameReconfigured => Some(ShapeTransitionKey::FunctionNameReconfigured),
            Self::FunctionLengthReconfigured => Some(ShapeTransitionKey::FunctionLengthReconfigured),
            Self::Constructed { constructor, new_target_is_base } => Some(ShapeTransitionKey::Constructed(constructor, new_target_is_base)),
            Self::Add {
                key,
                attributes,
                ..
            } => Some(ShapeTransitionKey::Add(key, attributes)),
            // The transition table outlives individual properties. Never put
            // accessor values in a key, where they would become strong roots.
            Self::Descriptor { slot, attributes } if !attributes.table_accessors() => {
                Some(ShapeTransitionKey::Descriptor { slot, attributes })
            }
            Self::Delete { key, .. } if key.symbol_value().is_none() => {
                Some(ShapeTransitionKey::Delete(key))
            }
            Self::Root
            | Self::Delete { .. }
            | Self::Vacant
            | Self::Descriptor { .. }
            | Self::Dictionary { .. } => None,
        }
    }
}
/// Constructor layouts `instanceof` remembers (`Vm::instanceof_constructor_layouts`), and the
/// parent constructors proved to inherit the native `@@hasInstance`
/// (`Vm::instanceof_inheriting_holders`): a power of two that holds the constructors a
/// program's `instanceof` sites test (`Uint8Array`, `Buffer`, `Array`, `Map`,
/// `ReadableStream`, `Headers`, `Error`, a framework's classes) without evicting one another.
pub(crate) const INSTANCEOF_CONSTRUCTOR_LAYOUTS: usize = 16;

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
    /// The index of a child shape, from its parent's index and the transition between
    /// them, as `derive_shape_lookup_index` would build it. A deletion moves entries, so it
    /// leaves the child to derive its own index.
    fn extended_by(mut self: Box<Self>, transition: ShapeTransition) -> Option<Box<Self>> {
        match transition {
            ShapeTransition::Add {
                key,
                slot,
                attributes,
            } => {
                debug_assert!(self.slot(key).is_none(), "a shape adds only absent keys");
                let position = u32::try_from(self.entries.len()).expect("shape width fits u32");
                self.entries.push((key, slot));
                let entries = &self.entries;
                self.positions
                    .insert_unique(Self::key_hash(key), position, |&position| {
                        Self::key_hash(entries[position as usize].0)
                    });
                self.record_attributes(slot, attributes.shape());
            }
            ShapeTransition::Descriptor { slot, attributes } => {
                self.record_attributes(slot, attributes);
            }
            ShapeTransition::Delete { .. } => return None,
            ShapeTransition::FunctionName { .. }
            | ShapeTransition::FunctionNameReconfigured
            | ShapeTransition::FunctionLengthReconfigured
            | ShapeTransition::Constructed { .. }
            | ShapeTransition::Root
            | ShapeTransition::Vacant
            | ShapeTransition::Dictionary { .. } => {}
        }
        Some(self)
    }
    /// The slot's newest attributes; default ones are not stored.
    fn record_attributes(&mut self, slot: u32, attributes: ShapeAttributes) {
        if attributes == DEFAULT_SHAPE_ATTRIBUTES {
            self.attributes.remove(&slot);
        } else {
            self.attributes.insert(slot, attributes);
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
/// Facts a shape inherits from its chain: once a transition introduces one, every descendant
/// keeps it. One byte, so `Shape` keeps its size.
#[derive(Clone, Copy, Default)]
struct ShapeFacts(u8);
impl ShapeFacts {
    /// A symbol key or a table accessor of the chain may be a GC root.
    const MAY_HAVE_GC_ROOTS: u8 = 1 << 0;
    /// Some property of the chain was a `SLOT_GETTER` accessor, so a raw slot read must check
    /// whether the slot holds a getter rather than a value.
    const SLOT_GETTERS: u8 = 1 << 1;
    /// Some property of the chain was made an accessor. Only a descriptor transition makes
    /// one, as an add carries data attributes, so a chain without this fact holds data
    /// properties alone and a read of any slot is its value.
    const ACCESSORS: u8 = 1 << 2;
    /// Some property of the chain was made an accessor whose functions
    /// `Vm::shape_accessors` holds (`ShapeAttributes::table_accessors`): one with a setter, as
    /// every lazy data property is, rather than a getter alone.
    const TABLE_ACCESSORS: u8 = 1 << 3;

    fn extended_by(self, transition: ShapeTransition) -> Self {
        let introduced = |fact: bool, bit: u8| if fact { bit } else { 0 };
        let descriptor = |test: fn(ShapeAttributes) -> bool| {
            matches!(transition, ShapeTransition::Descriptor { attributes, .. } if test(attributes))
        };
        Self(
            self.0
                | introduced(transition.introduces_gc_roots(), Self::MAY_HAVE_GC_ROOTS)
                | introduced(descriptor(ShapeAttributes::slot_getter), Self::SLOT_GETTERS)
                | introduced(descriptor(|attributes| attributes.accessor), Self::ACCESSORS)
                | introduced(descriptor(ShapeAttributes::table_accessors), Self::TABLE_ACCESSORS),
        )
    }

    fn table_accessors(self) -> bool {
        self.0 & Self::TABLE_ACCESSORS != 0
    }

    fn accessors(self) -> bool {
        self.0 & Self::ACCESSORS != 0
    }

    fn may_have_gc_roots(self) -> bool {
        self.0 & Self::MAY_HAVE_GC_ROOTS != 0
    }

    fn slot_getters(self) -> bool {
        self.0 & Self::SLOT_GETTERS != 0
    }
}
struct Shape {
    /// The shape this one extends; `NO_PARENT_SHAPE` for the root.
    parent: u32,
    transition: ShapeTransition,
    /// Property slots an object of this shape stores (slot indices are `u32`).
    storage_len: u32,
    dictionary_trigger: Option<DictionaryTrigger>,
    facts: ShapeFacts,
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
    /// The child of this shape's latest cached transition (`NO_LAST_CHILD` before one), tried
    /// before the transition index: objects of one layout grow by the same keys in turn.
    last_child: std::cell::Cell<u32>,
}
// Every object layout and property key costs one shape; keep the record compact.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<Shape>() == 56);
/// `Shape::parent` of the root shape.
const NO_PARENT_SHAPE: u32 = u32::MAX;
/// `Shape::last_child` before the shape has a cached transition.
const NO_LAST_CHILD: u32 = u32::MAX;
const PROTOTYPE_SLOT_UNKNOWN: u32 = u32::MAX;
const PROTOTYPE_SLOT_NONE: u32 = u32::MAX - 1;
/// The shape table's initial capacity, above what a builtin realm uses (about 1900 shapes).
/// A table this large is mapped by the allocator on its own, so it grows by remapping in
/// place, never leaving a copied-out table behind, and only the shapes in use are resident.
const SHAPE_TABLE_CAPACITY: usize = 4096;
impl Shape {
    fn root() -> Self {
        Self::child(None, ShapeTransition::Root, 0, None, ShapeFacts::default())
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
        parent_facts: ShapeFacts,
    ) -> Self {
        Self {
            parent: parent.unwrap_or(NO_PARENT_SHAPE),
            transition,
            storage_len: u32::try_from(storage_len).expect("object property index exceeds u32"),
            dictionary_trigger,
            facts: parent_facts.extended_by(transition),
            lookup_index: OnceCell::new(),
            long_walks: std::cell::Cell::new(0),
            prototype_slot: std::cell::Cell::new(PROTOTYPE_SLOT_UNKNOWN),
            index_keys: std::cell::Cell::new(None),
            last_child: std::cell::Cell::new(NO_LAST_CHILD),
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
        /// For an async callee pushed in place: its rooted promise
        /// (`PendingGeneralCall::settles`).
        settles: Option<crate::heap::RootId>,
        /// For a generator resumed in place: its rooted generator
        /// (`PendingGeneralCall::generator`).
        generator: Option<generator::GeneratorResume>,
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
        /// Boxed so every `FrameOutcome` stays a few words: the common completions move it
        /// through each call return.
        frame: Option<Box<Frame>>,
    },
    Yield {
        value: Value,
        destination: Register,
        delegated_result: Option<Value>,
        frame: Option<Box<Frame>>,
    },
}

impl FrameOutcome {
    /// The value a call's completed activation yields: a return, or a constructor's.
    #[inline(always)]
    fn into_value(self) -> Result<Value, JsError> {
        match self {
            Self::Complete(value) | Self::ConstructComplete { value, .. } => Ok(value),
            Self::ParameterInitializationComplete => {
                Err(JsError("parameter initialization escaped a generator activation".into()))
            }
            Self::Await { .. } => Err(JsError("await requires async continuation".into())),
            Self::Yield { .. } => Err(JsError("yield requires generator continuation".into())),
        }
    }
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
    natives: builtins::NativeTable,
    /// One shared string value per program atom a function name spells (`atom_string`):
    /// strings are immutable, so every closure of a plan can hold the same name value.
    /// Only program atoms and the anonymous name are kept, so the loaded program bounds it.
    atom_strings: FxHashMap<Atom, Value>,
    frames: Vec<Box<Frame>>,
    native_stack_frames: Vec<(usize, String)>,
    /// Emptied root lists of finished native calls, reused by the next `NativeContext`.
    pub(crate) native_root_lists: Vec<Vec<RootId>>,
    /// The roots the last collection gathered, which sizes the next one's root vector.
    gc_root_count: usize,
    /// Owners with out-of-cell edges at the previous collection, which sizes the next index.
    gc_owned_root_count: usize,
    frame_pool: Vec<Box<Frame>>,
    /// Emptied call stacks of finished general loops, one per nesting level reached: a loop
    /// takes one on entry, so running a job allocates no stack of its own.
    pending_call_stacks: Vec<Vec<dispatch_frame::PendingGeneralCall>>,
    /// Activations a running lane holds without frames.
    lane_stack: dispatch_fast::LaneStack,
    /// Whether Wasm functions with a compiled denotation run as native code.
    execution_mode: ExecutionMode,
    execution_interrupt: crate::ExecutionInterrupt,
    /// Invalidates monomorphic `call_indirect` certificates after a table
    /// mutation or collection that can change target identity.
    wasm_reference_epoch: std::cell::Cell<u64>,
    active_call_roots: Vec<Value>,
    with_stack: Vec<Value>,
    /// The realm's intrinsics as the host bootstrap left them (`seal_startup_intrinsics`).
    startup_intrinsics: startup_intrinsics::StartupIntrinsics,
    /// Set by the first `with` entry and never cleared: `Native::WithEnter` is
    /// the only source of with objects, so until it runs no environment
    /// carries any.
    with_scope_entered: bool,
    test262_agent: Test262AgentState,
    shared_async_waiters:Vec<atomics::SharedAsyncWaiter>,
    programs: ProgramStore,
    program_cache_layouts: Vec<ProgramCacheLayout>,
    /// The register file of the frame the general loop dispatches; see `run_general_loop`.
    active_registers: *mut Value,
    /// The error of a call the lane made in place (`dispatch_fast::lane_call_native`), which
    /// the general loop raises at that call's instruction once the lane exits there.
    lane_raised: Option<JsError>,
    /// A lane native call running with its activations still frameless
    /// (`Native::runs_beside_activations`), which the first observer of the frame stack makes
    /// frames (`materialize_deferred_lane`).
    lane_deferred: Option<dispatch_fast::DeferredLaneCall>,
    /// The module the host resolved each (referrer, specifier) of a static request to since
    /// the last collection (`Vm::resolve_module_request`). The graph's passes (collection,
    /// pending dependencies, instantiation, evaluation) resolve each request again, and each
    /// resolution read the file and its package scope anew: 3,824 resolutions of 863
    /// requests, 19% of the date-fns npm scenario. A host must load the same module for the
    /// same (referrer, specifier) each time (ECMA-262 HostLoadImportedModule), as Node's
    /// module map does; a collection drops the sources held here.
    resolved_module_requests: rustc_hash::FxHashMap<(String, String), crate::host::ModuleSource>,
    /// The name of the module each (referrer, specifier) resolved to, kept across collections
    /// (`Vm::resolved_module_name`): binding a loaded module's imports needs only its record's
    /// name, and resolving it again after a collection read its file anew. typescript.js
    /// (9 MB) imported from an ES module was read and wrapped again at the run's peak.
    resolved_module_names: rustc_hash::FxHashMap<(String, String), String>,
    /// Per host function, `Host::host_function_runs_beside_activations`, derived on first use.
    host_beside_activations: std::cell::OnceCell<Box<[bool]>>,
    /// The completion a frame's own call record returns, made by a lane activation
    /// (`dispatch_fast::lane_frame_return`), which the general loop returns from the frame
    /// at that instruction once the lane exits there.
    lane_returned: Option<Value>,
    /// The native a lane call is running from its caller's frame (`dispatch_fast::lane_call_native`),
    /// whose calls of JavaScript run frameless in that lane (`Vm::lane_native_callback`).
    lane_native: Option<dispatch_fast::LaneNativeCall>,
    /// The completion of a native's frameless callback (`Completion::ReturnsToNative`), which
    /// its run hands to the native once the lane exits there.
    lane_native_returned: Option<Value>,
    /// `program_cache_layouts[active_program]`, kept by `set_active_program`.
    active_layout: ProgramCacheLayout,
    /// The active program's materialized constants, kept by `set_active_program` and by
    /// constant installation. A program's constant buffer never moves once installed.
    active_constants: *const [Value],
    /// The active program's field cache entries (`SiteTable::entries_from` of its
    /// `field_base`), re-derived by `refresh_active_field_entries` whenever the active program
    /// changes or the table grows: a site's entry is then one load from it.
    active_field_entries: *const <FieldCache as site_table::SiteEntry>::Stored,
    active_program: ProgramId,
    profile: Profile,
    shapes: Vec<Shape>,
    /// Each shape's constructor (`ShapeTransition::Constructed` on its chain), derived for
    /// the shapes created since the last collection: a shape's parent precedes it, so a
    /// collection extends this instead of walking every shape. Reset with the shape table.
    shape_constructors: Vec<Value>,
    /// The getters and setters of shapes' accessor descriptors (`ShapeAttributes`).
    shape_accessors: Vec<ShapeAccessors>,
    transitions: shape_transitions::ShapeTransitionIndex,
    shape_slot_memo: shape_memo::ShapeSlotMemo,
    plain_copy_shapes: shape_memo::PlainCopyShapeMemo,
    plain_assign_shapes: shape_memo::PlainAssignShapeMemo,
    /// The final shapes of host records, by their names list (`create_embedding_record`).
    embedding_record_shapes: FxHashMap<usize, u32>,
    /// The geometry accessors typed array reads resolve to (`typed_array_geometry_read`).
    typed_array_geometry: typed_array::TypedArrayGeometryProofs,
    function_name_shapes: provenance::FunctionNameShapes,
    symbol_write_proofs: symbol_write_proof::SymbolWriteProofs,
    symbol_load_proofs: symbol_load_proof::SymbolLoadProofs,
    /// Lane private-name sites' homes (`private_site_home`).
    private_site_homes: private_site_home::PrivateSiteHomes,
    array_iteration_proof: Option<iterators::ArrayIterationProof>,
    array_species_proof: Option<array::ArraySpeciesProof>,
    for_in_own_keys: Option<object_static::ForInOwnKeys>,
    species_slot_proof: std::cell::Cell<Option<object_get::SpeciesSlotProof>>,
    /// The last proof that a promise's species constructor is `%Promise%` (`promise_species_proved`).
    promise_species_proof: std::cell::Cell<Option<promise::PromiseSpeciesProof>>,
    index_free_chain_proof: std::cell::Cell<Option<object::IndexFreeChainProof>>,
    enumerable_keys: enumerable_key_memo::EnumerableKeyMemo,
    shape_attribute_memo: shape_memo::ShapeAttributeMemo,
    name_atom_memo: name_atom_memo::NameAtomMemo,
    atom_text: AtomTable,
    /// Every atom, hashed by its text (`atom_keys::text_hash`); equal hashes are told apart
    /// by comparing the texts, which the atom tables hold.
    atoms: hashbrown::HashTable<Atom>,
    /// The atoms that spell canonical array indices, by index: index-keyed lookups find
    /// them without formatting and hashing the decimal text.
    array_index_atoms: FxHashMap<u32, Atom>,
    /// Every atom name in id order (`atom_text`, then the names the VM appends), shared by
    /// the dynamic and module programs whose atoms are a prefix of the VM's.
    atom_sequence: Rc<crate::bytecode::AtomSequence>,
    /// The strings most recently matched by a RegExp whose search decodes them, most recent
    /// first, each prepared once for repeated matching (`Vm::regexp_subject`).
    regexp_subjects: Vec<Rc<quench_regexp::Subject>>,
    regexp_matchers: regexp::RegExpMatcherCache,
    // One lazily derived class byte per atom; length tracks `atom_sequence`.
    atom_classes: Vec<std::cell::Cell<u8>>,
    dynamic_strings: Option<Box<FxHashMap<u64, Value>>>,
    /// The `typeof` result string of each value type, made on first use; `undefined` until
    /// then. Rooted, so `typeof` neither hashes nor allocates once warm.
    typeof_names: [Value; TYPEOF_NAMES],
    symbol_registry: FxHashMap<String, Value>,
    well_known_symbols: WellKnownSymbols,
    string_concats: Option<Box<[StringConcatCache]>>,
    /// String values recently interned as property keys, and their atoms
    /// (`Vm::string_key_atom`); collection empties it.
    string_atoms: Option<Box<[(Value, Atom); STRING_ATOM_CACHE_SIZE]>>,
    /// The strings `Vm::atom_string` recently made for atoms whose strings it does not keep
    /// (`atom_strings`), by atom, direct-mapped; collection empties it, as `string_atoms`.
    /// A program compiled at run time spells its property names with such atoms, and
    /// `Object.keys` of its objects had made each key's string again on every call.
    recent_atom_strings: Option<Box<[(Atom, Value); ATOM_STRING_CACHE_SIZE]>>,
    /// The one-unit ASCII strings recently made (`charAt`, `String.fromCharCode`), by unit;
    /// collection empties it, as `recent_atom_strings`. A loop building a string a unit at a
    /// time (V8-v7 Crypto's hex conversion) had allocated each unit's string anew.
    ascii_unit_strings: Option<Box<[Value; ASCII_UNITS]>>,
    call_targets: Box<[InLoopCallTarget; CALL_TARGET_CACHE_SIZE]>,
    field_caches: site_table::SiteTable<FieldCache>,
    /// Field load sites whose data property was found beyond the immediate prototype, by
    /// global site index; cleared with the field caches.
    /// Per cache site, the index of its `inherited_field_caches` entry, or
    /// `NO_INHERITED_FIELD`.
    inherited_field_indices: site_table::SiteTable<u32>,
    inherited_field_caches: Vec<InheritedFieldCache>,
    /// Accessor reads by field-cache index (`AccessorFieldCache`), up to
    /// `INHERITED_FIELD_CACHE_WAYS` receiver shapes a site, oldest first, as an inherited
    /// method's site sees one shape per subclass; cleared with the field caches.
    accessor_field_caches: FxHashMap<usize, Vec<AccessorFieldCache>>,
    field_add_indices: site_table::SiteTable<u32>,
    field_add_sets: Vec<FieldAddCacheSet>,
    /// Adds a keyed `[[Set]]` of a string naming an atom made (`Vm::keyed_add_hit`): a keyed
    /// site's keys vary, so the entries are shared, indexed by source shape and atom
    /// (`keyed_add_index`); cleared with the field caches.
    keyed_adds: Box<[FieldAddCache; KEYED_ADD_ENTRIES]>,
    megamorphic_field_indices: site_table::SiteTable<u32>,
    /// The cache sites whose per-site entries (`field_caches`, `megamorphic_field_indices`,
    /// `field_add_indices`, `inherited_field_indices`) were written since the last
    /// `invalidate_field_caches`, which then resets only those; `None` once they outnumber
    /// `FIELD_CACHE_SPARSE_RESET_SHARE` of the sites, and every site resets.
    field_cache_writes: Option<Vec<u32>>,
    megamorphic_fields: Vec<FieldCacheSet>,
    length_atom: Atom,
    // Cached `prototype` atom; `NO_CACHED_ATOM` until first use after a program reset.
    prototype_atom: Atom,
    /// Non-dictionary constructor shapes verified to have no own `@@hasInstance` and a
    /// data `prototype` at the paired slot, direct-mapped by shape; derived from shapes, so
    /// field-cache invalidation clears them.
    instanceof_constructor_layouts: [Option<(u32, usize)>; INSTANCEOF_CONSTRUCTOR_LAYOUTS],
    /// Constructor prototypes (a subclass's parent constructor, `%TypedArray%`) proved to
    /// inherit the native `@@hasInstance` (`Vm::inherits_native_has_instance`), each with the
    /// prototype-layout epoch its proof holds under: the proof marks every link of the chain
    /// a prototype, so any change to one, and every collection, retires it. Direct-mapped by
    /// the holder's cell.
    instanceof_inheriting_holders: [Option<(Value, u64)>; INSTANCEOF_CONSTRUCTOR_LAYOUTS],
    /// The shapes an ordinary closure's own properties step through, without and with a
    /// `prototype` (`construction::ClosureShapes`); derived from shapes, so field-cache
    /// invalidation clears it.
    closure_shapes: [Option<construction::ClosureShapes>; 2],
    /// The shape a promise's resolving function's own `length` and `name` reach
    /// (`Vm::native_with_realm`); derived from shapes, so field-cache invalidation clears it.
    resolver_function_shape: Option<u32>,
    /// The parts of a recorded closure that depend on its function and realm alone
    /// (`construction::ClosureTemplate`); derived from shapes, so field-cache invalidation
    /// clears it.
    closure_templates: Box<[Option<construction::ClosureTemplate>; CLOSURE_TEMPLATES]>,
    /// The header of an iterator result `{ value, done }` (`Vm::iterator_result`) and the
    /// prototype it was derived for; derived from shapes, so field-cache invalidation clears it.
    iterator_result_header: Option<(Value, crate::heap::InlineObjectHeader)>,
    /// The one `{ value, done }` result a lane iterator step reuses (`Vm::reused_iterator_result`),
    /// `undefined` until the first; cleared with `iterator_result_header`.
    reused_iterator_result: Value,
    /// The shapes of a complete data and accessor property descriptor object
    /// (`Vm::property_descriptor_object`), indexed by `accessor`; derived from shapes, so
    /// field-cache invalidation clears them.
    descriptor_shapes: [Option<u32>; 2],
    /// The field slots of the descriptor shape `ToPropertyDescriptor` last read
    /// (`object_descriptors::DescriptorLayout`); derived from shapes.
    descriptor_layout: Option<object_descriptors::DescriptorLayout>,
    /// The shape a new RegExp object starts in and the one its own `lastIndex` moves it to
    /// (`Vm::regexp_from_matcher`), as the first one recorded; derived from shapes, so
    /// field-cache invalidation clears it.
    regexp_object_shapes: Option<(u32, u32)>,
    /// A RegExp receiver shape verified to hold an own writable data `lastIndex` at the paired
    /// slot; derived from shapes, so field-cache invalidation clears it.
    regexp_last_index_layouts: Vec<(u32, u32)>,
    /// Derived from shapes, so field-cache invalidation clears it.
    regexp_builtin_layouts: Vec<regexp::RegExpBuiltinLayout>,
    /// Derived from shapes, so field-cache invalidation clears it.
    regexp_symbol_layouts: Vec<regexp::RegExpSymbolLayout>,
    /// An object verified to hold the native `Function.prototype[@@hasInstance]` as a
    /// non-writable, non-configurable data property, which it then holds for its lifetime;
    /// rooted so the identity stays meaningful. `Value::NULL` when none.
    instanceof_has_instance_holder: Value,
    /// `instanceof_has_instance_holder` as a prototype link (`ProtoLink::of`), set with it,
    /// so the steady `instanceof` compares a constructor's link undecoded.
    instanceof_has_instance_link: ProtoLink,
    /// Recent capture walks (`Vm::walked_capture_slots`), direct-mapped by start environment
    /// and depth.
    capture_walks: Box<[CaptureWalk; CAPTURE_WALKS]>,
    to_fixed_atom: Atom,
    to_precision_atom: Atom,
    runtime_atoms: RuntimeAtoms,
    /// `Vm::constructed_shape` answers; cleared by every collection.
    constructed_shapes: [provenance::ConstructedShape; provenance::CONSTRUCTED_SHAPE_ENTRIES],
    /// The host's object templates (`object_template`), indexed by their ids.
    object_templates: Vec<object_template::ObjectTemplate>,
    method_caches: Vec<[MethodCache; 2]>,
    megamorphic_methods: Vec<MethodCacheSet>,
    method_cache_atoms: method_cache::MethodCacheAtoms,
    #[cfg(feature = "profile-aggregate")]
    invalidated_methods: FxHashMap<MethodCacheKey, InvalidatedMethod>,
    object_shapes: site_table::SiteTable<u32>,
    descriptors: FxHashMap<(Value, property_key::PropertyKey), PropertyAttributes>,
    // Weak identities; captured environments remain authoritative in function cells.
    function_values: FxHashMap<(ProgramId, u32), Vec<WeakHandle>>,
    /// The JavaScript wrapper of each exported Wasm closure, so every export of one
    /// function is one object. Weak: a wrapper nothing holds is made again on demand.
    wasm_exported_functions: FxHashMap<Value, WeakHandle>,
    /// Private-name home objects found by `private_name_home`, kept while their starting
    /// environment is live.
    private_homes: FxHashMap<PrivateHomeKey, Value>,
    /// The collation `String.prototype.localeCompare` resolves without locales or options
    /// (`Vm::locale_compare_collation`); the host's default locale fixes it.
    default_collation: Option<std::rc::Rc<intl_collator::Collation>>,
    wasm_js: wasm_js::WasmJsIntrinsics,
    direct_eval: bool,
    direct_eval_var_program: Option<ProgramId>,
    /// The Function-constructor programs this VM may still offer the host's compile cache
    /// (`DYNAMIC_FUNCTIONS_KEPT`).
    dynamic_functions_to_keep: u16,
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
    /// Diagnostic: general-loop dispatches by the register width of their function, since
    /// register liveness (and with it promotion and move coalescing) covers only functions
    /// whose registers fit a root mask.
    #[cfg(feature = "profile-aggregate")]
    fn report_dispatch_by_register_width(&self) {
        let mut rows = Vec::new();
        for (program, functions) in self.profile.site_counts.iter().enumerate() {
            let Some(residual) = self.programs.residual(ProgramId::from_raw(program as u32)) else {
                continue;
            };
            for (function, sites) in functions.iter().enumerate() {
                let total: u64 = sites.iter().sum();
                if total == 0 {
                    continue;
                }
                let Some(code) = residual.function_get(function) else { continue };
                rows.push((total, code.registers, code.local_atoms.len(), program, function));
            }
        }
        let all: u64 = rows.iter().map(|row| row.0).sum();
        let wide: u64 = rows
            .iter()
            .filter(|row| row.1 > crate::compile::liveness::ROOT_MAP_REGISTERS)
            .map(|row| row.0)
            .sum();
        rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        eprintln!("register-width: dispatches {all}, in functions over the root mask {wide}");
        for (total, registers, locals, program, function) in rows.iter().take(15) {
            eprintln!("  {total:>12} registers={registers} locals={locals} program={program} function={function}");
        }
    }

    pub(crate) fn execution_interrupt_handle(&self)->crate::ExecutionInterrupt { self.execution_interrupt.clone() }
    pub(super) fn check_execution_interrupt(&self)->Result<(),JsError> {
        if self.execution_interrupt.is_termination_requested() { Err(JsError::execution_terminated()) } else { Ok(()) }
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
        let leaving = ParkedRealm {
            global: previous,
            state: GlobalLexicalState {
                declarations: std::mem::take(&mut self.realm.global_lexical_declarations),
                bindings: std::mem::take(&mut self.realm.global_lexical_bindings),
                immutable_bindings: std::mem::take(
                    &mut self.realm.immutable_global_lexical_bindings,
                ),
            },
            prototypes: REALM_FOLLOWING_PROTOTYPES.map(|native| *self.realm_prototype_field(native)),
        };
        let parked = match self.realm.parked.replace(leaving) {
            Some(parked) if parked.global == global => Some(parked),
            parked => {
                if let Some(parked) = parked {
                    self.realm.global_lexical_states.insert(parked.global, parked.state);
                }
                None
            }
        };
        self.realm.globals = global;
        let state = match parked {
            Some(parked) => {
                // Natives are shared by every realm and allocate from these fields, so the
                // fields follow the active realm: a native called from a realm builds that
                // realm's objects.
                for (native, prototype) in REALM_FOLLOWING_PROTOTYPES.into_iter().zip(parked.prototypes) {
                    debug_assert!(self
                        .realm
                        .intrinsics
                        .builtin_prototypes
                        .get(&(global, native))
                        .is_none_or(|intrinsic| *intrinsic == prototype));
                    *self.realm_prototype_field(native) = prototype;
                }
                parked.state
            }
            None => {
                for native in REALM_FOLLOWING_PROTOTYPES {
                    if let Some(&prototype) = self
                        .realm
                        .intrinsics
                        .builtin_prototypes
                        .get(&(global, native))
                    {
                        *self.realm_prototype_field(native) = prototype;
                    }
                }
                self.realm.global_lexical_states.remove(&global).unwrap_or_default()
            }
        };
        self.realm.global_lexical_declarations = state.declarations;
        self.realm.global_lexical_bindings = state.bindings;
        self.realm.immutable_global_lexical_bindings = state.immutable_bindings;
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
    pub(crate) fn root_scoped(&mut self, value: Value) -> RootId {
        self.heap.root_scoped(value)
    }
    pub(crate) fn scoped_root_height(&self) -> usize {
        self.heap.scoped_root_height()
    }
    pub(crate) fn release_root_scope(&mut self, height: usize) {
        self.heap.release_root_scope(height);
    }
    pub(crate) fn release_root_scope_keeping(&mut self, height: usize, keep: &[RootId]) {
        self.heap.release_root_scope_keeping(height, keep);
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
        for atom in root.global_lexical_atoms().iter().copied() {
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
                self.realm.bind_global_lexical(atom, Value::DELETED);
            }
        }
        self.realm
            .global_lexical_declarations
            .extend(root.global_lexical_atoms().iter().copied());
        if self.eval_script_context {
            self.realm.immutable_global_lexical_bindings.extend(
                root.global_immutable_atoms()
                    .iter()
                    .copied()
                    .filter(|atom| root.global_lexical_atoms().contains(atom)),
            );
        }
        if root
            .global_var_atoms()
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
        for atom in root.global_function_atoms().iter().copied() {
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
        for atom in root.global_var_atoms().iter().copied() {
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
        let Some(atom) = program.function(ROOT_FUNCTION_ID as usize)
            .local_atoms
            .get(slot)
            .copied()
        else {
            return;
        };
        if program.function(ROOT_FUNCTION_ID as usize)
            .global_lexical_atoms()
            .contains(&atom)
        {
            self.realm.bind_global_lexical(atom, value);
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
        let metadata = &program.function(ROOT_FUNCTION_ID as usize);
        let bindings = metadata
            .global_lexical_atoms()
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
        for (atom, value) in bindings {
            self.realm.bind_global_lexical(atom, value);
        }
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
        self.run_main(program, drain_jobs)
    }

    /// Installs the realm and the host as `execute_deferred_jobs` does, without running the
    /// program: the state a startup snapshot is taken from (`write_snapshot`).
    pub(crate) fn initialize_for_snapshot(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        if program.functions.is_empty() {
            return Err(JsError::validation("program has no entry function".into()));
        }
        self.check_execution_interrupt()?;
        self.initialize(program)
    }

    /// `execute_deferred_jobs` in a VM `restore_snapshot` restored, whose main program is
    /// `program`: the host resumes its installation where the snapshot left it.
    pub(crate) fn execute_restored(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        self.resume_host()?;
        self.run_main(program, false)
    }

    /// Runs the main program of an installed realm: its module graph, its declarations and
    /// its body, then optionally its jobs.
    fn run_main(&mut self, program: &ResidualProgram, drain_jobs: bool) -> Result<Value, JsError> {
        if program.is_module() {
            self.instantiate_main_module(program)?;
            self.evaluate_program_module_requests(program)?;
        }
        self.instantiate_global_declarations(program)?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("initialized");
        #[cfg(feature = "profile-allocator")]
        self.report_allocator_if_enabled("initialized");
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
        #[cfg(feature = "profile-allocator")]
        self.report_allocator_if_enabled("complete");
        #[cfg(feature = "profile-gc-time")]
        self.report_gc_time_if_enabled();
        #[cfg(feature = "profile-aggregate")]
        if std::env::var_os("QUENCH_PROFILE_ALL_PROGRAMS").is_some() {
            for index in 0..self.programs.len() {
                let id = ProgramId::from_raw(index as u32);
                if let Some(residual) = self.programs.get(id) {
                    eprintln!(
                        "{{\"kind\":\"quench-profile-program\",\"program_id\":{},\"source_name\":{:?}}}",
                        id.raw(),
                        residual.source_name,
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
            if std::env::var_os("QUENCH_OPCODE_CENSUS").is_some_and(|census| census == "all") {
                self.report_dispatch_by_register_width();
            }
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
    #[cfg(feature = "profile-allocator")]
    fn report_allocator_if_enabled(&self, phase: &str) {
        if std::env::var_os("QUENCH_ALLOCATOR_MEMORY").is_some() {
            crate::report_allocator_memory(phase);
        }
    }
    #[cfg(feature = "profile-gc-time")]
    fn report_gc_time_if_enabled(&self) {
        if std::env::var_os("QUENCH_GC_TIME_PROFILE").is_none() {
            return;
        }
        let (_, collections, _, _, _, _) = self.heap.stats();
        let gc = self.heap.gc_profile();
        eprintln!(
            "{{\"kind\":\"quench-gc-time\",\"collections\":{collections},\"full_collections\":{},\"full_mark_nanos\":{},\"minor_mark_nanos\":{},\"mark_nanos\":{},\"sweep_nanos\":{},\"sweep_phase_nanos\":{:?},\"mark_clear_nanos\":{},\"vm_owner_edges_nanos\":{},\"vm_roots_nanos\":{},\"vm_shape_setup_nanos\":{},\"vm_after_nanos\":{}}}",
            gc.full_collections, gc.full_mark_nanos, gc.minor_mark_nanos,
            gc.mark_nanos, gc.sweep_nanos, gc.sweep_phase_nanos, gc.mark_clear_nanos, gc.vm_phase_nanos[0], gc.vm_phase_nanos[1], gc.vm_phase_nanos[2], gc.vm_phase_nanos[3],
        );
        // Sizes of the VM-side tables beside the heap: a table that grows with work done
        // rather than with live data is retention.
        let mut kinds: std::collections::BTreeMap<&'static str, usize> = Default::default();
        self.heap.for_each_live_cell(|cell| *kinds.entry(cell.profile_variant_name()).or_default() += 1);
        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        eprintln!("{{\"kind\":\"quench-live-cells\",\"top\":{:?}}}", &kinds[..kinds.len().min(8)]);
        let promise = &self.realm.promise;
        eprintln!(
            "{{\"kind\":\"quench-vm-tables\",\"live_slots\":{},\"reaction_capabilities\":{},\"jobs\":{},\"descriptors\":{},\"function_values\":{},\"frames\":{},\"active_call_roots\":{}}}",
            self.heap.live_slots(),
            promise.reaction_capabilities.len(),
            self.realm.jobs.len(),
            self.descriptors.len(),
            self.function_values.len(),
            self.frames.len(),
            self.active_call_roots.len(),
        );
    }
    #[cfg(feature = "profile-memory")]
    pub(crate) fn report_memory_if_enabled(&self, phase: &str) {
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            self.report_memory(phase);
        }
    }
    /// `QUENCH_FUNCTION_USE`: how many compiled functions have been called (their call plan is
    /// derived on first call), and the record and code bytes of those never called.
    #[cfg(feature = "profile-memory")]
    pub(crate) fn report_function_use(&self) {
        let (mut functions, mut called, mut uncalled_bytes) = (0usize, 0usize, 0usize);
        for program in self.programs.residuals() {
            for function in program.functions.iter().filter_map(crate::bytecode::FunctionSlot::record) {
                functions += 1;
                if function.call_plan.get().is_some() {
                    called += 1;
                } else {
                    uncalled_bytes += size_of::<crate::bytecode::Function>()
                        + function.code.capacity() * size_of::<crate::bytecode::Instr>()
                        + function.wide.capacity() * size_of::<crate::bytecode::WideInstruction>();
                }
            }
        }
        eprintln!(
            "{{\"kind\":\"quench-function-use\",\"functions\":{functions},\"called\":{called},\"uncalled_record_and_code_bytes\":{uncalled_bytes}}}"
        );
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
        let (
            inline_property_values,
            narrowable_inline_property_values,
            out_of_line_property_values,
            narrowable_out_of_line_property_values,
        ) = self.heap.live_property_narrow_stats();
        let (array_elements, array_capacity) = self.heap.array_element_stats();
        let (dense_array_values, narrowable_dense_array_values) =
            self.heap.dense_array_element_narrow_stats();
        let megamorphic_field_entries: usize =
            self.megamorphic_fields.iter().map(FieldCacheSet::len).sum();
        let max_megamorphic_field_entries = self
            .megamorphic_fields
            .iter()
            .map(FieldCacheSet::len)
            .max()
            .unwrap_or(0);
        let active_field_caches = self
            .field_caches
            .iter()
            .filter(|cache| cache.receiver != NO_FIELD_RECEIVER)
            .count();
        let mut field_add_cache_entries = 0usize;
        let mut field_add_cache_depth_counts = [0usize; FIELD_ADD_CACHE_MAX_PROTO_DEPTH + 1];
        for set in &self.field_add_sets {
            for cache in set.entries() {
                field_add_cache_entries += 1;
                field_add_cache_depth_counts[usize::from(cache.prototype_depth)] += 1;
            }
        }
        let mut field_add_set_occupancy = [0usize; FIELD_ADD_CACHE_WAYS + 1];
        for set in &self.field_add_sets {
            field_add_set_occupancy[usize::from(set.len)] += 1;
        }
        let frame_bytes: usize = self
            .frames
            .iter()
            .chain(&self.frame_pool)
            .map(|frame| {
                frame.locals.capacity() * size_of::<Value>()
                    + frame.registers.capacity() * size_of::<Value>()
            })
            .sum();
        let field_add_cache_bytes = self.field_add_indices.capacity_bytes()
            + self.field_add_sets.capacity() * size_of::<FieldAddCacheSet>();
        eprintln!(
            "{{\"kind\":\"quench-memory\",\"phase\":\"{phase}\",\"heap_slots\":{slots},\"slot_bytes\":{slot_bytes},\"free_bytes\":{free_bytes},\"cell_bytes\":{cell_bytes},\"cell_counts\":{cell_counts:?},\"property_values\":{property_values},\"property_capacity\":{property_capacity},\"property_free_ranges\":{property_free},\"live_property_values\":{live_property_values},\"live_property_capacity\":{live_property_capacity},\"inline_property_values\":{inline_property_values},\"narrowable_inline_property_values\":{narrowable_inline_property_values},\"out_of_line_property_values\":{out_of_line_property_values},\"narrowable_out_of_line_property_values\":{narrowable_out_of_line_property_values},\"array_elements\":{array_elements},\"array_capacity\":{array_capacity},\"dense_array_values\":{dense_array_values},\"narrowable_dense_array_values\":{narrowable_dense_array_values},\"shapes\":{},\"shape_capacity\":{},\"max_shape_width\":{max_shape_width},\"shape_bytes\":{shape_bytes},\"shape_lookup_index_payload_bytes\":{shape_lookup_index_payload_bytes},\"transitions\":{},\"transition_bytes\":{},\"frame_bytes\":{frame_bytes},\"field_cache_bytes\":{},\"field_cache_sites\":{},\"field_cache_site_capacity\":{},\"active_field_caches\":{active_field_caches},\"megamorphic_field_sites\":{},\"megamorphic_field_entries\":{megamorphic_field_entries},\"max_megamorphic_field_entries\":{max_megamorphic_field_entries},\"field_cache_data_bytes\":{},\"megamorphic_field_index_bytes\":{},\"field_add_index_bytes\":{},\"field_add_set_bytes\":{},\"field_add_sets\":{},\"field_add_cache_entries\":{field_add_cache_entries},\"field_add_cache_depth_counts\":{field_add_cache_depth_counts:?},\"field_add_set_occupancy\":{field_add_set_occupancy:?},\"method_cache_bytes\":{},\"megamorphic_method_sites\":{}}}",
            self.shapes.len(),
            self.shapes.capacity(),
            self.transitions.len(),
            self.transitions.bytes(),
            self.field_caches.capacity_bytes()
                + self.megamorphic_field_indices.capacity_bytes()
                + field_add_cache_bytes,
            self.field_caches.len(),
            self.field_caches.capacity(),
            self.megamorphic_fields.len(),
            self.field_caches.capacity_bytes(),
            self.megamorphic_field_indices.capacity_bytes(),
            self.field_add_indices.capacity_bytes(),
            self.field_add_sets.capacity() * size_of::<FieldAddCacheSet>(),
            self.field_add_sets.len(),
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
        let cache_side_table_bytes = self.field_caches.capacity_bytes()
            + self.megamorphic_field_indices.capacity_bytes()
            + self.field_add_indices.capacity_bytes()
            + self.field_add_sets.capacity() * size_of::<FieldAddCacheSet>()
            + self.method_caches.capacity() * size_of::<[MethodCache; 2]>();
        let atom_side_table_bytes = self.atoms.capacity() * (size_of::<Atom>() + 1)
            + self.atom_sequence.appended_record_bytes()
            + (0..self.atom_sequence.appended_len())
                .map(|offset| self.atom_sequence.appended(offset).capacity())
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
            + heap.function_record_reserved_bytes
            + heap.promise_record_reserved_bytes
            + heap.await_record_reserved_bytes
            + heap.iterator_record_reserved_bytes
            + heap.string_record_reserved_bytes
            + heap.legacy_cell_record_reserved_bytes
            + heap.free_slot_metadata_bytes
            + property_arena_bytes
            + heap.cell_payload_bytes
            + heap.array_buffer_store_bytes
            + heap.boxed_object_header_bytes
            + heap.sparse_array_sidecar_bytes
            + vm_metadata_bytes;
        eprintln!(
            "{{\"kind\":\"quench-memory-composition\",\"phase\":\"{phase}\",\"collection\":{},\"cell_kind_names\":{:?},\"cell_counts\":{:?},\"cell_variant_names\":{:?},\"cell_variant_counts\":{:?},\"occupied_slots\":{},\"complete_slot_slabs\":{},\"fully_free_slot_slabs\":{},\"partial_slot_slabs\":{},\"fully_empty_slot_pages\":{fully_empty_slot_pages},\"slot_arena_reserved_bytes\":{},\"object_record_reserved_bytes\":{},\"object_record_reusable_ids\":{},\"object_record_allocation_scan_cursor\":{},\"array_record_reserved_bytes\":{},\"array_record_reusable_ids\":{},\"array_record_allocation_scan_cursor\":{},\"function_record_reserved_bytes\":{},\"promise_record_reserved_bytes\":{},\"await_record_reserved_bytes\":{},\"iterator_record_reserved_bytes\":{},\"string_record_reserved_bytes\":{},\"legacy_cell_record_reserved_bytes\":{},\"free_slot_metadata_bytes\":{},\"object_headers\":{},\"embedded_object_headers\":{},\"boxed_object_headers\":{},\"boxed_object_header_bytes\":{},\"object_property_counts_by_width\":{:?},\"inline_property_attribution_bytes_overlapping_slots\":{},\"object_header_size_bytes\":{},\"cell_size_bytes\":{},\"slot_size_bytes\":{},\"property_arena_values_bytes\":{},\"property_arena_metadata_bytes\":{},\"property_arena_used_values\":{},\"property_arena_capacity_values\":{},\"live_property_used_values\":{},\"live_property_capacity_values\":{},\"live_property_used_values_by_width\":{:?},\"live_property_capacity_values_by_width\":{:?},\"dense_array_elements_bytes\":{},\"dense_array_headers_bytes\":{},\"dense_array_backing_count\":{},\"sparse_array_sidecar_bytes\":{},\"string_buffers_bytes\":{},\"string_buffer_count\":{},\"other_cell_payload_bytes\":{},\"cell_payload_total_bytes\":{},\"shape_table_bytes\":{},\"shape_count\":{},\"dictionary_shape_count\":{},\"shape_lookup_bytes\":{},\"transition_table_bytes\":{},\"descriptor_side_table_bytes\":{},\"cache_side_table_bytes\":{},\"atom_side_table_bytes\":{},\"frame_bytes\":{},\"profile_instrumentation_bytes\":{},\"accounted_runtime_bytes\":{},\"accounted_with_profile_bytes\":{}}}",
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
            heap.function_record_reserved_bytes,
            heap.promise_record_reserved_bytes,
            heap.await_record_reserved_bytes,
            heap.iterator_record_reserved_bytes,
            heap.string_record_reserved_bytes,
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
            self.shapes.len(),
            self.shapes.iter().filter(|shape| shape.dictionary_trigger.is_some()).count(),
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
    /// `Runtime::end_for_process_exit`: drops the host, and the program store when its drop
    /// reports, and leaves every other field to the operating system.
    pub(crate) fn end_for_process_exit(self) {
        let vm = std::mem::ManuallyDrop::new(self);
        // SAFETY: `vm` is never used or dropped again, so each field read here is dropped
        // exactly once and the rest are never dropped.
        unsafe {
            if vm.programs.reports_on_drop() {
                drop(std::ptr::read(&vm.programs));
            }
            drop(std::ptr::read(&vm.host));
        }
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
        // Every intrinsic names a value of the heap the reset discards; a new realm can reuse
        // a discarded realm's identity, which must not find the old one's intrinsics.
        self.realm.intrinsics = Default::default();
        // The snapshot contains heap values, shapes and atom ids from one realm.
        // A host may build a builtin before sealing the replacement realm.
        self.startup_intrinsics = Default::default();
        self.shared_async_waiters.clear();
        self.heap.reset();
        self.natives = Default::default();
        self.frames.clear();
        self.frame_pool.clear();
        self.realm.jobs.clear();
        self.realm.global_lexical_declarations.clear();
        self.realm.global_lexical_bindings.clear();
        self.realm.immutable_global_lexical_bindings.clear();
        self.with_stack.clear();
        self.direct_eval = false;
        self.parameter_eval = false;
        self.eval_script_context = false;
        self.construct_target = None;
        self.realm.promise = Default::default();
        self.shapes.truncate(1);
        self.shape_constructors.clear();
        // The kept root forgets its transitions along with the index.
        self.shapes[0].last_child.set(NO_LAST_CHILD);
        self.transitions.clear();
        self.shape_slot_memo.clear();
        self.plain_copy_shapes.clear();
        self.plain_assign_shapes.clear();
        self.embedding_record_shapes.clear();
        self.function_name_shapes.clear();
        self.symbol_write_proofs.clear();
        self.symbol_load_proofs.clear();
        self.private_site_homes.clear();
        self.array_iteration_proof = None;
        self.array_species_proof = None;
        self.for_in_own_keys = None;
        self.species_slot_proof.set(None);
        self.promise_species_proof.set(None);
        self.index_free_chain_proof.set(None);
        self.enumerable_keys.clear();
        self.shape_attribute_memo.clear();
        self.atom_strings.clear();
        // The sealed holders and their shapes belong to the discarded heap.
        self.startup_intrinsics = Default::default();
        self.atom_text = program.atoms.clone();
        self.atom_sequence = Rc::new(crate::bytecode::AtomSequence::new(self.atom_text.clone()));
        self.atoms.clear();
        self.name_atom_memo.clear();
        self.array_index_atoms.clear();
        self.atom_classes.clear();
        self.regexp_subjects.clear();
        self.regexp_matchers.clear();
        self.atom_classes
            .resize_with(self.atom_text.len(), Default::default);
        self.dynamic_strings = None;
        self.typeof_names = [Value::UNDEFINED; TYPEOF_NAMES];
        self.symbol_registry.clear();
        self.well_known_symbols.clear();
        self.string_concats = None;
        self.string_atoms = None;
        self.recent_atom_strings = None;
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
            next: self.intern_atom("next"),
            name: self.intern_atom("name"),
            empty: self.intern_atom(""),
            value: self.intern_atom("value"),
            done: self.intern_atom("done"),
            iterator_return: self.intern_atom("return"),
            writable: self.intern_atom("writable"),
            enumerable: self.intern_atom("enumerable"),
            configurable: self.intern_atom("configurable"),
            get: self.intern_atom("get"),
            set: self.intern_atom("set"),
            bound_target: self.intern_atom(BOUND_TARGET_KEY),
            bound_this: self.intern_atom(BOUND_THIS_KEY),
            bound_args: self.intern_atom(BOUND_ARGS_KEY),
            char_code_at: self.intern_atom("charCodeAt"),
        };
        self.field_caches = site_table::SiteTable::with_empty(usize::from(program.cache_sites));
        self.refresh_active_field_entries();
        self.field_cache_writes = Some(Vec::new());
        program.prime_loop_decoding();
        self.inherited_field_caches.clear();
        self.inherited_field_indices = site_table::SiteTable::with_empty(usize::from(program.cache_sites));
        self.field_add_indices = site_table::SiteTable::with_empty(usize::from(program.cache_sites));
        self.field_add_sets.clear();
        self.keyed_adds.fill(FieldAddCache::EMPTY);
        self.megamorphic_field_indices = site_table::SiteTable::with_empty(usize::from(program.cache_sites));
        self.megamorphic_fields.clear();
        self.length_atom = self.intern_atom("length");
        self.prototype_atom = self.intern_atom("prototype");
        self.to_fixed_atom = self.intern_atom("toFixed");
        self.to_precision_atom = self.intern_atom("toPrecision");
        self.method_caches = vec![[EMPTY_METHOD_CACHE; 2]; program.method_sites.len()];
        self.megamorphic_methods.clear();
        self.method_cache_atoms.clear();
        self.object_shapes = site_table::SiteTable::with_empty(program.object_sites.len());
        self.descriptors.clear();
        self.function_values.clear();
        self.wasm_exported_functions.clear();
        self.private_homes.clear();
        self.programs.reset(program.clone());
        self.instanceof_constructor_layouts = [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS];
        self.instanceof_inheriting_holders = [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS];
        self.closure_shapes = [None; 2];
        self.resolver_function_shape = None;
        self.function_name_shapes.clear();
        self.closure_templates.fill(None);
        self.iterator_result_header = None;
        self.reused_iterator_result = Value::UNDEFINED;
        self.descriptor_shapes = [None; 2];
        self.descriptor_layout = None;
        self.regexp_object_shapes = None;
        self.regexp_last_index_layouts.clear();
        self.regexp_builtin_layouts.clear();
        self.regexp_symbol_layouts.clear();
        self.set_instanceof_has_instance_holder(Value::NULL);
        self.program_cache_layouts.clear();
        self.program_cache_layouts.push(ProgramCacheLayout {
            shared_uncached_field: ProgramCacheLayout::shared_uncached_field(0, program.cache_sites),
            ..ProgramCacheLayout::default()
        });
        self.set_active_program(ProgramId::MAIN);
        self.typed_array_proto = Value::NULL;
        self.finalization_registry_proto = Value::NULL;
        self.temporal_plain_date_proto = Value::NULL;
        self.temporal_plain_time_proto = Value::NULL;
        self.temporal_plain_month_day_proto = Value::NULL;
        self.temporal_plain_year_month_proto = Value::NULL;
        self.random_state = fresh_random_seed();
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
        self.initialize_host()?;
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("host_bootstrap");
        Ok(())
    }
    fn materialize_constant(&mut self, constant: &Constant) -> Value {
        match constant {
            Constant::Number(v) => Value::number(*v),
            Constant::WasmBits64(bits) => self.encode_bits64(*bits),
            Constant::WasmV128(bits) => self.heap.alloc_v128(*bits),
            Constant::String(v) => self
                .typeof_constant(v)
                .unwrap_or_else(|| self.heap.alloc_string(JsString::from_shared_text(v))),
            Constant::StringUnits(v) => self.heap.alloc_string(JsString::from_units(v)),
            Constant::BigInt(v) => self.heap.alloc(Cell::bigint(v.clone())),
            Constant::Boolean(true) => Value::TRUE,
            Constant::Boolean(false) => Value::FALSE,
            Constant::Null => Value::NULL,
            Constant::Undefined => Value::UNDEFINED,
        }
    }
    fn materialize_program_constants(&mut self, id: ProgramId, program: &ResidualProgram) {
        let mut constants = Vec::with_capacity(program.constants.len());
        // Equal string constants share one cell, as engines intern literals: a string is an
        // immutable primitive, so which cell holds it is unobservable, and `===` between
        // literals from different sites then decides by identity before comparing text.
        let mut strings: FxHashMap<StringConstantKey<'_>, Value> = FxHashMap::default();
        for constant in &program.constants {
            let key = match constant {
                Constant::String(text) => StringConstantKey::Text(text),
                Constant::StringUnits(units) => StringConstantKey::Units(units),
                _ => {
                    constants.push(self.materialize_constant(constant));
                    continue;
                }
            };
            let value = match strings.entry(key) {
                std::collections::hash_map::Entry::Occupied(shared) => *shared.get(),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    *slot.insert(self.materialize_constant(constant))
                }
            };
            constants.push(value);
        }
        self.programs.set_constants(id, constants);
        // Lane views carry constants, so call targets naming the old views are stale.
        self.lane_stack.clear_script_targets();
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
    fn append_program_cache_layout(&mut self, id: ProgramId, program: &ResidualProgram) {
        program.prime_loop_decoding();
        self.append_cache_layout(id, program.cache_site_counts());
    }

    /// Lays out the cache tables of program `id`, with `sites` of each kind, after those of
    /// the programs before it.
    pub(super) fn append_cache_layout(&mut self, id: ProgramId, sites: crate::bytecode::CacheSiteCounts) {
        debug_assert_eq!(id.raw() as usize, self.program_cache_layouts.len());
        self.program_cache_layouts.push(ProgramCacheLayout {
            shared_uncached_field: ProgramCacheLayout::shared_uncached_field(self.field_caches.len(), sites.fields),
            field_base: self.field_caches.len(),
            method_base: self.method_caches.len(),
            object_base: self.object_shapes.len(),
        });
        let fields = usize::from(sites.fields);
        self.field_caches.extend_empty(fields);
        self.refresh_active_field_entries();
        self.megamorphic_field_indices.extend_empty(fields);
        self.field_add_indices.extend_empty(fields);
        self.inherited_field_indices.extend_empty(fields);
        self.method_caches.resize(self.method_caches.len() + sites.methods, [EMPTY_METHOD_CACHE; 2]);
        self.object_shapes.extend_empty(sites.objects);
    }
    /// Makes `program` the executing program and caches its layout, the projection every cache
    /// index reads. A program's layout is appended before the program can run.
    #[inline(always)]
    pub(super) fn set_active_program(&mut self, program: ProgramId) {
        let constants = std::ptr::from_ref(self.programs.constants(program));
        self.activate_program(program, constants);
    }

    /// `set_active_program` with `program`'s constants (`ProgramStore::constants`) in hand.
    #[inline(always)]
    pub(super) fn activate_program(&mut self, program: ProgramId, constants: *const [Value]) {
        self.active_program = program;
        self.active_layout = self.program_cache_layouts[program.raw() as usize];
        self.active_constants = constants;
        self.refresh_active_field_entries();
    }

    /// Re-derive `active_field_entries` after the active program or the field cache table
    /// changed.
    /// The field cache entries of `program`'s sites, for generated code that reads them
    /// (`NATIVE_FIELD_CACHE_LAYOUT`); valid until the cache table grows or is replaced.
    pub(super) fn program_field_entries(&self, program: ProgramId) -> Option<usize> {
        let layout = self.program_cache_layouts.get(program.raw() as usize)?;
        Some(self.field_caches.entries_from(layout.field_base) as usize)
    }

    pub(super) fn refresh_active_field_entries(&mut self) {
        self.active_field_entries = self.field_caches.entries_from(self.active_layout.field_base);
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
        // Only the names after the VM's sequence are new; the program shares the rest.
        for index in self.atom_sequence.len()..program.atoms.len() {
            let atom = self.intern_atom(&program.atoms[index]);
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
        // The roots are pushed as a slice: a chained iterator's extend cost a callback from an
        // array method about 60 instructions (`Array.prototype.map`, 1,000-element arrays).
        let start = self.active_call_roots.len();
        self.active_call_roots.reserve(args.len() + CALL_ROOTS_BESIDE_ARGUMENTS);
        self.active_call_roots.push(callee);
        self.active_call_roots.push(this);
        self.active_call_roots.extend_from_slice(args);
        let result = self.call_value_with_target_inner(p, callee, this, args, target);
        self.active_call_roots.truncate(start);
        result
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
        // Guest code observes the frame stack; a native that runs beside activations does not.
        // A script callee of such a native runs as an activation above them where it can.
        if self.lane_deferred.is_some() && !self.callee_runs_beside_activations(callee) {
            if matches!(self.heap.function(callee), Some(FunctionRecord { kind: FunctionKind::User(..), .. }))
                && let Some(result) = self.lane_deferred_callback(callee, this, args)
            {
                return result;
            }
            self.materialize_deferred_lane();
        }
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
            CallTarget::User(program_id, id, env) => {
                // A native a lane called runs its callee in that lane where the lane's own call
                // of it would.
                if self.lane_native.is_some()
                    && let Some(result) = self.lane_native_callback(callee, this, args)
                {
                    return result;
                }
                let program = self.programs.get(program_id).ok_or_else(|| {
                    self.type_error(p, "function belongs to an unavailable program".into())
                })?;
                // The profile's user targets count general and numeric bodies apart.
                let numeric = program
                    .function_get(id as usize)
                    .is_some_and(|function| function.dispatch == DispatchClass::Numeric);
                self.profile.call_target(1 + usize::from(numeric), args.len());
                let active_program = self.replace_active_program(program_id);
                let realm = self
                    .heap
                    .function(callee)
                    .map_or(self.realm.globals, |function| function.realm);
                let current_global = self.switch_realm_global(realm);
                let result = if program
                    .function_get(id as usize)
                    .is_some_and(|function| function.is_class_constructor)
                    && !self.construct_target.is_some_and(|target| target.callee == callee)
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

    /// Counts collections; see `Heap::collection_epoch`.
    pub(crate) fn collection_epoch(&self) -> u64 {
        self.heap.collection_epoch()
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
        let (program_id, id, env, realm) = match self.heap.function(callee) {
            Some(FunctionRecord {
                kind: FunctionKind::User(program_id, id),
                env,
                realm,
                ..
            }) => (*program_id, *id, *env, *realm),
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

/// A program string constant's text as written, the key under which equal constants share a
/// cell (`Vm::materialize_program_constants`).
#[derive(PartialEq, Eq, Hash)]
enum StringConstantKey<'a> {
    Text(&'a str),
    Units(&'a [u16]),
}
