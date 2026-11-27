#[cfg(feature = "native-execution")]
pub mod native_operands;
pub mod jit_backend;
pub mod tier_policy;
pub mod tier_queries;
pub mod engine_types; pub mod fact_edge;
pub mod lazy_table;
pub mod builtin_table;
use builtin_table::{BuiltinFamilies, BuiltinTable};
pub mod execution_table;
use execution_table::ExecutionTable;
pub mod engine_calls;
pub(crate) use engine_calls::EngineVm;
pub mod engine_calls2;
pub(crate) use engine_calls2::*;
#[cfg(feature = "native-execution")]
pub use engine_calls::{define_lane_helper_symbols, native_js_lane_helpers, native_js_operation_entry, native_js_operation_entry_for};
pub mod intl_core;
pub mod layer_hooks;
pub mod realm_table;
pub mod register_file;
pub mod lane_completion; pub(crate) use lane_completion::LaneCompletion;
pub mod shape_lookup; mod string_wrapper; pub mod accessor_cache; mod holder_hints; use holder_hints::PrototypeHints;
#[cfg(feature = "native-execution")]
pub use native_operands::NativeJsOperands;
pub mod specializer;
pub mod specialize;
#[cfg(unix)]
pub use engine_types::NativeFaultHandler;
#[cfg(feature = "profile-native-ops")]
pub mod native_op_profile;
pub use jit_backend::{IsaBaseline, NativeTarget};
pub mod tier_machine; pub mod tier_events;
pub mod shape_stability;
pub mod switches;
pub mod shape_transitions;
pub mod snapshot;
pub mod startup_intrinsics;
pub mod wasm;
pub use engine_types::{AheadImages, ExecutionMode};
pub(crate) use engine_types::InstanceRecord;
pub use date::local_timezone_changed;
pub mod wasm_exception;
pub mod wasm_gc;
pub mod wasm_global;
pub mod wasm_host;
pub mod wasm_js;
pub(crate) use wasm_exception::{js_tag_cell, js_tag_type};
pub mod wasm_indirect;
pub mod wasm_table;
use crate::Value;
use crate::bytecode::{Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, LEXICAL_THIS_BINDING,
    NEW_TARGET_BINDING, Op, Operand, ProgramRecord, Register, ResidualProgram, WideInstruction};
use crate::staged::add_plan::{AddSite, Transition, concrete_add_property, concrete_add_property_held, concrete_set_function_name};
#[cfg(feature = "profile-memory")]
use crate::heap::CellKind;
use crate::heap::{
    CallSiteLocation, CallSiteName, CallSiteRecord, Cell, CollectionStorage, FunctionKind, FunctionRecord, GcTraceKind, IteratorRecord, Heap, IteratorConsumer,
    IteratorHelper, IteratorKind, Native, Object, ProtoLink, ProxyKind, RootId, StackData, TypedArrayKind,
    WeakHandle,
};
use crate::host::{CapabilityId, Host, HostContext};
use crate::profile::{CallSource, Profile, TerminalCall};
use crate::value::number_to_u32;
pub use activation::{CallCompletion, CallContext, ConstructionTarget};
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
    if let Some(seed) = crate::clock::fixed_random_seed() {
        return seed;
    }
    std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now()) | 1
}
pub(super) use crate::units::MAX_SAFE_INTEGER;
pub(crate) use crate::bytecode::MAX_ARRAY_LENGTH;
pub const ROOT_FUNCTION_ID: u32 = 0;
/// The values a call roots besides its arguments: the callee and the receiver.
const CALL_ROOTS_BESIDE_ARGUMENTS: usize = 2;
/// The arguments a prepared callback's call keeps rooted in its loop's reused slots
/// (`Vm::with_prepared_callback`): the most an array iteration native passes, a reducer's
/// four. A call passing more roots them as `call_value` does.
const PREPARED_ARGUMENT_ROOTS: usize = 4;

#[derive(Clone, Copy, Default)]
pub struct RuntimeAtoms {
    pub lexical_this: Atom,
    pub new_target: Atom,
    /// The RegExp members the builtin fast paths check on every call.
    pub regexp_exec: Atom,
    pub regexp_flags: Atom,
    pub last_index: Atom,
    /// `regexp::REGEXP_FLAG_ACCESSORS` names, in that order.
    pub regexp_flag_accessors: [Atom; regexp::REGEXP_FLAG_ACCESSORS.len()],
    /// Read by every promise resolution and species lookup.
    pub then: Atom,
    pub constructor: Atom,
    /// The ToPrimitive methods, read by every ToNumber and ToString of an object.
    pub value_of: Atom,
    pub to_string: Atom,
    /// The iterator protocol's step method.
    pub next: Atom,
    /// Canonical key for named function-info lookup; paired with length_atom.
    pub name: Atom,
    /// The empty string, an anonymous function's initial name.
    pub empty: Atom,
    /// An iterator result's keys, in its literal order.
    pub value: Atom,
    pub done: Atom,
    /// The iterator protocol's close method.
    pub iterator_return: Atom,
    /// A property descriptor object's other keys (`value` above), which
    /// `Object.getOwnPropertyDescriptor` writes on every call.
    pub writable: Atom, enumerable: Atom, configurable: Atom,
    pub get: Atom, pub set: Atom,
    /// The internal keys a bound function's environment holds its parts under
    /// (`Vm::bound_function_parts`).
    pub bound_target: Atom,
    pub bound_this: Atom,
    pub bound_args: Atom,
    /// The names of a Proxy handler's traps (`proxy::ProxyTrap`).
    pub proxy_traps: [Atom; proxy::PROXY_TRAP_COUNT],
}
pub mod activation;
pub mod agent;
pub mod atom_set;
pub mod pending_job;
use pending_job::PendingJob;
use atom_set::{AtomSet, IndexAtoms};
pub mod arguments;
pub mod array;
pub mod array_buffer;
pub mod array_builtins;
pub mod array_copy;
pub mod array_dense;
pub mod inplace;
pub mod array_flatten;
pub mod array_group;
pub mod array_indexed;
pub mod array_view;
pub mod array_modern;
pub mod array_fresh;
pub mod atom_keys;
pub mod atomics;
pub mod bigint;
pub mod boolean;
pub mod builtins;
pub mod native_rows;
pub mod call_arguments;
pub mod call_printer;
pub(crate) use call_printer::{freeze_call_pcs, freeze_call_sites};
pub mod call_out;
pub mod call_out_calls;
pub mod call_out_literals;
pub mod call_out_classes;
pub mod call_out_values;
pub mod call_out_owned;
pub mod call_out_wasm;
pub mod suspension;
pub mod coercion;
pub mod collection_intake;
pub mod collections;
pub mod construction;
pub mod data_view;
pub mod date;
pub mod dispatch;
pub mod lane_values; pub mod lane_activations;
pub(crate) use engine_types::{copy_arguments, fill_window as fill_registers};
pub mod dispatch_frame; mod tier0_promotion;
pub mod dispatch_wasm;
pub mod dynamic_strings;
#[cfg(feature = "profile-lane-exits")]
pub mod lane_exits;
pub mod embedding; mod embedding_values; pub use embedding_values::Detach;
pub mod provenance;
pub mod preshaped_receiver;
pub mod inspection;
pub mod caller_view;
pub mod environment; pub mod name_scope;
pub mod owner_step; pub mod var_owner;
pub mod root_bindings;
pub mod handler_entry;
pub mod equality; pub mod error; mod eval;
pub mod field_cache;
pub mod field_stubs;
pub mod in_presence;
pub mod instanceof_walk;
pub mod site_table;
pub mod finalization;
pub mod first_call_append; mod first_call_chunk;
pub mod function;
pub mod function_cache;
pub mod gc;
pub mod generator;
pub mod global_facts;
pub mod group_by;
pub mod host_function;
pub mod index;
pub mod iterator_list;
pub mod lazy_data_property;
pub mod local_time;
pub mod property_definition;
use group_by::GroupByKind;
use property_definition::PropertyDefinitionKind;
pub mod intact_fuses;
pub mod iterator_proofs;
pub mod iterators;
pub mod json;
pub mod method_cache;
pub mod plain_equality;
pub mod module;
pub mod number;
mod number_natives;
pub mod number_parse;
pub mod object_template;
pub mod object;
pub mod shape_memo;
pub mod cache_epochs;
pub mod symbol_write_proof;
pub mod symbol_load_proof;
pub mod private_site_home;
pub mod private_slots;
pub mod enumerable_key_memo;
pub mod name_atom_memo;
mod byte_census;
pub mod object_assignment;
pub mod object_array;
pub mod object_builtins;
pub mod object_descriptors;
pub mod object_get;
pub mod object_integrity;
pub mod object_keys;
pub mod object_static;
pub mod object_symbols;
pub mod object_shape_adopt;
#[cfg(test)] mod lane_loop_tests;
#[cfg(test)] mod record_slot_tests;
#[cfg(test)] mod lane_native_callback_tests;
#[cfg(test)] mod lane_entry_tests;
#[cfg(test)] mod call_out_tests;
#[cfg(test)] mod object_tests; #[cfg(test)] mod shape_attribute_tests;
#[cfg(test)] mod native_frame_tests;
#[cfg(test)] mod catch_tests; #[cfg(test)] mod shape_stability_tests;
pub mod property_key;
use call_arguments::CallArguments;
use object::AtomClass;
pub(crate) use json::json_literal_length;
pub(crate) use object_static::array_index;
pub use program_store::ProgramId;
pub(crate) use regexp::RegExpFlags;
use program_store::{ModuleImport, ProgramStore};
use promise::PromiseRuntime;
pub use property_key::PropertyKey;
use root_bindings::RootBinding;
use var_owner::{EvalCaller, VarOwner};
pub mod operations;
pub mod native_table;
pub mod reachable_realm;
mod native_effects;
mod reachable_protocol;
#[cfg(all(test, feature = "bench-primitives"))]
pub mod primitive_bench;
pub mod primitives;
pub mod program_store;
pub mod promise;
pub mod host_frames;
pub mod promise_aggregate;
pub mod promise_async;
pub mod promise_jobs;
pub mod promise_state;
pub mod proxy; use proxy::ProxyTrap;
pub mod reflect;
pub mod regexp;
pub mod shadow_realm;
pub mod sort;
pub mod string;
pub mod string_cache;
pub mod string_extra;
pub mod value_serializer;
pub mod owned_clone_native;
pub mod symbol;
pub mod well_known_symbols;
pub use well_known_symbols::{WellKnownSymbol, WellKnownSymbols};
pub mod type_predicates;
pub mod typed_array;
pub mod typed_array_access;
pub mod typed_array_base64;
pub mod typed_array_construct;
pub mod typed_array_install;
pub mod vm_init;
pub mod wtf16;
pub use error::{CompileFailureKind, JsError};
pub use value_serializer::{
    write_double, write_raw_bytes, write_uint32, write_uint64, HostObjectRead, SerializedValue, SharedWireInput, ValueDeserializer,
    ValueSerializer, WireInput, WireOutput, WIRE_FORMAT_VERSION,
};
pub(crate) use value_serializer::{ReadHooks, WriteHooks};
use wtf16::JsString;
#[cfg(test)] mod tests; #[cfg(test)] mod lane_semantics_tests;
#[cfg(test)] mod closure_retention_tests;
#[cfg(test)] mod storage_tests; #[cfg(test)] mod name_scope_tests;

/// A native's callback prepared for a loop of calls (`Vm::with_prepared_callback`): the
/// callee and receiver every call passes, and how its calls run (`CallbackProof`).
pub(super) struct PreparedCallback {
    callee: Value,
    this: Value,
    proof: CallbackProof,
}

/// The life of a prepared callback's proof, one transition per loop: its first call runs as
/// `call_value` runs it, so a loop of one call pays for no proof; the second proves what
/// the loop's calls share once, in the lane that called the native (`Lane`) or not at all
/// (`General`), and every later call reads that proof. Held in place for one loop, so the
/// lane proof's size costs no allocation.
#[allow(clippy::large_enum_variant)]
enum CallbackProof {
    First,
    Second,
    /// The lane proof, and the call roots from `roots` on that keep the callee, the receiver
    /// and, in the `PREPARED_ARGUMENT_ROOTS` after them, each call's arguments.
    Lane { roots: usize, lane: engine_types::PreparedLaneCallback },
    General,
}

#[quench_macros::test_visible]
#[derive(Clone)]
pub struct Frame {
    // Actual callable identity is distinct from the code ID and captured environment.
    pub context: CallContext,
    /// Leading actual arguments that legacy `f.arguments` reflects live from the parameter
    /// storage; `original_arguments` holds the arguments after them.
    reflected_parameters: u16,
    pub original_arguments: Vec<Value>,
    pub program: ProgramId,
    pub function: u32,
    pub pc: usize,
    /// Whether pc is a saved continuation after a call rather than the executing operation.
    pub pc_is_resume: bool,
    /// Skip one function-entry tier attempt after a native continuation stopped
    /// on an already-executed backedge to PC zero.
    pub js_jit_skip_entry_once: bool,
    source_field: Option<(usize, crate::bytecode::FieldPart)>,
    // The current operation's lexical projection, independent of the PC
    // published for GC roots or saved as the continuation after a call.
    binding_site_pc: Option<u32>,
    pub env: Value,
    pub this: Value,
    pub locals: Vec<Value>,
    dynamic_bindings: Vec<(Atom, Value)>,
    pub captured: bool,
    pub registers: register_file::RegisterFile,
    capture_cache: CaptureCache,
    /// How many leading register slots hold values, whatever the current length: below it a
    /// root-mapped activation can take the file without rewriting it.
    initialized_registers: usize,
    pub active_iterators: Vec<ActiveIterator>,
    // Empty while active; owns the transferred scope stack while detached.
    pub with_objects: Vec<Value>,
    pub with_base: usize,
    /// The frame pushed its own `this` binding with a live value. Only `super()` rebinds
    /// `this`, and only in derived constructors, whose binding starts deleted; so for these
    /// frames `this` is authoritative and the binding lookup can be skipped.
    pub fixed_this: bool,
}
/// A frame's innermost capture slots, valid for `env` until the collection after `epoch`
/// (`Vm::cached_direct_capture`).
#[quench_macros::test_visible]
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

impl CaptureCache {
    const EMPTY: Self = Self { env: Value::NULL, captured: false, epoch: 0, slots: std::ptr::null(), root: None };
}

impl Default for CaptureCache {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// Capture walks `Vm::walked_capture_slots` remembers, by start environment and depth: enough
/// for the closures of a few nested scopes reading their outer bindings in turn.
const CAPTURE_WALKS: usize = 64;
/// Closure templates `Vm::closure_in_realm` remembers, one per function id modulo the count:
/// the nested functions a closure-heavy region makes again on each call of its parent. A
/// conflict overwrites the entry, which costs the displaced function's next closure the
/// lookups the template saves (about 1.6K Ir) and a template recorded again. At 64 the
/// TypeScript compiler's closures took that path 882K times in one run.
const CLOSURE_TEMPLATES: usize = 1024;
const _: () = assert!(CLOSURE_TEMPLATES.is_power_of_two());
const _: () = assert!(CAPTURE_WALKS.is_power_of_two());

/// The entries a `LazyTable` answers with before its first write, shared by every VM that never
/// fills it.
/// A remembered capture walk: `depth` layers out from `cache.env`, as `CaptureCache` holds
/// the innermost one.
#[derive(Clone, Copy, Default)]
pub(super) struct CaptureWalk {
    depth: u16,
    cache: CaptureCache,
}

impl CaptureWalk {
    const EMPTY: Self = Self { depth: 0, cache: CaptureCache::EMPTY };
}

impl Frame {
    fn source_instruction_pc(&self) -> usize {
        if self.pc_is_resume { self.pc.saturating_sub(1) } else { self.pc }
    }

    #[quench_macros::test_visible]
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
            registers: register_file::RegisterFile::new(),
            initialized_registers: 0,
            capture_cache: CaptureCache::default(),
            active_iterators: vec![],
            with_objects: Vec::new(),
            with_base,
            fixed_this: false,
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
        let window_end = base + parameters.len();
        // SAFETY: capacity holds `count` values; every slot below `count` is
        // written before the length covers it. The fills cover only the slots
        // outside the parameter window, which the copy below writes.
        unsafe {
            for index in 0..base {
                registers.add(index).write(initial);
            }
            // A few values: an inline copy, not a library call.
            for (offset, value) in parameters.iter().enumerate() {
                registers.add(base + offset).write(*value);
            }
            for index in window_end..count {
                registers.add(index).write(initial);
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

    /// Gives a generator or async activation's register file room for the environment window
    /// its lane resumes lay after the registers (`open_environment_window`) in one allocation,
    /// not a reallocation at its first resume.
    #[inline(always)]
    fn reserve_suspension_window(&mut self, function: &crate::bytecode::Function) {
        if function.is_generator || function.is_async {
            self.registers.clear();
            self.registers.reserve(usize::from(function.registers) + usize::from(engine_types::WINDOW_LOCALS) + usize::from(function.locals));
        }
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
            self.reserve_suspension_window(function);
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

    /// Makes the file `registers`, a suspending activation's whole register window, sized as
    /// `prepare_registers_for` sizes it for `function` with every slot then the window's: one
    /// copy, not a fill the copy overwrites.
    #[inline(always)]
    fn adopt_registers(&mut self, function: &crate::bytecode::Function, registers: &[Value]) {
        debug_assert_eq!(registers.len(), usize::from(function.registers));
        self.js_jit_skip_entry_once = false;
        self.reserve_suspension_window(function);
        self.registers.clear();
        self.registers.extend_from_slice(registers);
        self.initialized_registers = self.initialized_registers.max(registers.len());
    }

    /// Makes `locals` `count` undefined values. A pooled frame usually has the capacity already,
    /// so this is one capacity check and the stores.
    #[inline(always)]
    fn reset_locals_undefined(&mut self, count: usize) {
        self.locals.clear();
        self.locals.resize(count, Value::UNDEFINED);
    }

    /// `place_parameters` for a plain activation, the in-place case of it: its locals already
    /// start undefined, so none is cleared, and its promoted entries, ordered by local, begin
    /// with every parameter, so validation places promoted entry `i` in register `i` and
    /// parameter `k` (for `k` below the promoted count) is written once to register `k`.
    /// Plain functions take no rest parameter and keep no parameter registers, so the general
    /// rules reduce to this copy.
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

    /// Places a general activation's parameters and promoted locals, each register written once.
    /// Parameter `k` lives in its promoted register when it has one, else in register
    /// `parameter_registers + k` when the body keeps its parameters in registers, else in its
    /// local, which `bind_activation_arguments` filled. Its value is `args[k]`, or `undefined`
    /// past the arguments; a promoted rest parameter takes the array its local holds instead. A
    /// promoted parameter's local is cleared, and every other promoted local starts `undefined`.
    /// A body has either `parameter_registers` or `local_registers`, never both: a wasm body
    /// keeps its parameters in registers and promotes nothing, and a compiled body has no
    /// parameter registers.
    fn place_parameters(&mut self, function: &crate::bytecode::Function, args: &[Value]) {
        debug_assert_eq!(self.locals.len(), usize::from(function.locals));
        debug_assert_eq!(self.registers.len(), usize::from(function.registers));
        if let Some(base) = function.parameter_registers {
            for offset in 0..function.params {
                if function.promoted_register(offset).is_none() {
                    self.registers[usize::from(base + offset)] = args
                        .get(usize::from(offset))
                        .copied()
                        .unwrap_or(Value::UNDEFINED);
                }
            }
        }
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
pub struct ActiveIterator {
    pub iterator: u16,
    pub done: u16,
}
/// A realm's global lexical bindings in insertion order. A binding is never removed while its
/// realm is active, so its index names it until the realm switches, and a read that holds the
/// index and name (`lane_load_root_lexical`) needs no hash.
type GlobalLexicalBindings = indexmap::IndexMap<Atom, Value, rustc_hash::FxBuildHasher>;

pub struct Realm {
    pub globals: Value,
    /// Every global lexical name of the realm, including every bound one
    /// (`Realm::bind_global_lexical`), so it alone answers whether a name is one: a test every
    /// global name load makes (`name_row`).
    global_lexical_declarations: AtomSet,
    pub global_lexical_bindings: GlobalLexicalBindings,
    immutable_global_lexical_bindings: FxHashSet<Atom>,
    /// Every inactive realm's global lexical state, keyed by its global, except the realm
    /// last switched away from, which `parked` holds.
    global_lexical_states: FxHashMap<Value, GlobalLexicalState>,
    /// The realm last switched away from, so switching back to it, which jobs and calls
    /// across two realms do on every turn, moves its state without hashing.
    parked: Option<ParkedRealm>,
    pub intrinsics: RealmIntrinsics,
    pub jobs: std::collections::VecDeque<PendingJob>,
    template_objects: FxHashMap<(ProgramId, u32, u32), Value>,
    pub promise: PromiseRuntime,
}
impl Realm {
    /// Binds global lexical `atom` to `value`, declaring a name bound for the first time;
    /// whether that declared it.
    #[must_use]
    fn bind_global_lexical(&mut self, atom: Atom, value: Value) -> bool {
        self.global_lexical_bindings.insert(atom, value).is_none()
            && self.global_lexical_declarations.insert(atom)
    }

    /// Whether a global lexical declaration of `atom` shadows the global object's property in
    /// this or any inactive realm.
    fn shadows_global_name(&self, atom: Atom) -> bool {
        self.global_lexical_declarations.contains(&atom)
            || self.parked.as_ref().is_some_and(|parked| parked.state.declarations.contains(&atom))
            || self.global_lexical_states.values().any(|state| state.declarations.contains(&atom))
    }
}

#[derive(Default)]
pub struct RealmIntrinsics {
    arguments_objects: FxHashMap<(Value, bool), ArgumentsObjectTemplate>,
    iterator_prototypes: realm_table::RealmTable<IteratorRealmPrototypes>,
    pub builtin_prototypes: FxHashMap<(Value, Native), Value>,
    /// Original per-realm constructors for function factory allocations.
    function_constructors: FxHashMap<(Value, Native), Value>,
    promise_constructors: realm_table::RealmTable<Value>,
    error_intrinsics: realm_table::RealmTable<ErrorRealmIntrinsics>,
    regexp_intrinsics: realm_table::RealmTable<regexp::RegExpIntrinsics>,
    pub intl_number_format_constructors: FxHashMap<Value, Value>,
    pub intl_number_format_prototypes: FxHashMap<Value, Value>,
    pub intl_number_format_fallback_symbols: FxHashMap<Value, Value>,
    pub intl_collator_constructors: FxHashMap<Value, Value>,
    pub intl_collator_prototypes: FxHashMap<Value, Value>,
    pub intl_plural_rules_prototypes: FxHashMap<Value, Value>,
    pub intl_datetime_format_constructors: FxHashMap<Value, Value>,
    pub intl_datetime_format_prototypes: FxHashMap<Value, Value>,
    pub intl_datetime_format_fallback_symbols: FxHashMap<Value, Value>,
    pub intl_display_names_constructors: FxHashMap<Value, Value>,
    pub intl_display_names_prototypes: FxHashMap<Value, Value>,
    pub intl_duration_format_constructors: FxHashMap<Value, Value>,
    pub intl_duration_format_prototypes: FxHashMap<Value, Value>,
    pub intl_list_format_constructors: FxHashMap<Value, Value>,
    pub intl_list_format_prototypes: FxHashMap<Value, Value>,
    pub intl_relative_time_format_prototypes: FxHashMap<Value, Value>,
    pub intl_segmenter_prototypes: FxHashMap<Value, Value>,
    pub intl_segment_iterator_prototypes: FxHashMap<Value, Value>,
    pub intl_segments_prototypes: FxHashMap<Value, Value>,
    pub intl_locale_prototypes: FxHashMap<Value, Value>,
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
    declarations: AtomSet,
    bindings: GlobalLexicalBindings,
    immutable_bindings: FxHashSet<Atom>,
}
/// An inactive realm with its global lexical state and the prototypes the realm-following
/// VM fields held while it was active (`REALM_FOLLOWING_PROTOTYPES`).
struct ParkedRealm {
    global: Value,
    state: GlobalLexicalState,
    prototypes: [Value; REALM_FOLLOWING_PROTOTYPES.len()],
    /// The realm's `%Promise%`, which `Vm::promise_constructor` held while it was active.
    promise_constructor: Value,
}
/// The natives whose prototypes VM fields follow from the active realm
/// (`Vm::realm_prototype_field`).
/// One `typeof` name per `bytecode::TYPEOF_NAMES` entry, indexed as `typeof_name_index` classifies values.
const TYPEOF_NAMES: usize = crate::bytecode::TYPEOF_NAMES.len();
/// Map, Set and RegExp follow too: their in-place paths and fuses test a receiver's prototype
/// against these fields, so a benchmark run in a fresh context realm missed every one.
const REALM_FOLLOWING_PROTOTYPES: [Native; 8] = [
    Native::Object, Native::Function, Native::Array, Native::String, Native::Promise,
    Native::Map, Native::Set, Native::RegExp,
];
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FieldCache {
    pub receiver: u32,
    pub holder: u32,
    pub slot: u16,
    /// The layout id of the holder (`Object::layout_id`) where the holder object was at hand when
    /// the entry was recorded, else `NO_HOLDER_LAYOUT`. It names the holder's exact shape `holder`
    /// (`layouts::auxiliary`), so a read may guard the holder's whole header in one compare.
    holder_id: u32,
}

impl FieldCache {
    /// The entry of an own data field at `slot` of receivers of exact shape `receiver`.
    #[inline(always)]
    pub const fn own(receiver: u32, slot: u16) -> Self {
        Self { receiver, holder: NO_FIELD_HOLDER, slot, holder_id: field_cache_layout::NO_HOLDER_LAYOUT }
    }
}
/// The internal keys of a bound function's environment object.
const BOUND_TARGET_KEY: &str = "\0quench:bound-target";
const BOUND_THIS_KEY: &str = "\0quench:bound-this";
const BOUND_ARGS_KEY: &str = "\0quench:bound-args";
pub const NO_FIELD_HOLDER: u32 = field_cache_layout::NO_HOLDER;
/// The holder layout id an entry holds where no holder object was at hand (`FieldCache::holder_id`).
const NO_FIELD_HOLDER_LAYOUT: u32 = field_cache_layout::NO_HOLDER_LAYOUT;
use crate::staged::field_cache_layout::{self, FieldCacheWords};
/// Where an accessor field cache's holder keeps its getter (`AccessorFieldCache::slot_accessor`):
/// in the slot itself (`ShapeAttributes::slot_getter`), in the slot's pair cell
/// (`slot_accessor_pair`), or where its attributes say (the shape's accessor table).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotAccessor {
    Getter,
    Pair,
    Attributes,
}
/// A field site's read of an accessor of a shaped receiver, its own or on a prototype: it
/// holds while the receiver keeps its shape and prototype and the chain to `holder` keeps the
/// layout it was proved under, `holder`'s `slot` attributes carrying the getter.
#[derive(Clone, Copy)]
struct AccessorFieldCache {
    receiver: u32,
    /// The receiver's layout id (`Object::layout_id`): its shape and prototype in one word,
    /// which an ordinary receiver's hit compares instead of each.
    layout: u32,
    /// The receiver's prototype, where the chain to `holder` starts.
    prototype: Value,
    /// The prototype holding the accessor, or `OWN_ACCESSOR` for the receiver itself, whose
    /// own slot each receiver of the shape fills with its own accessor.
    holder: Value,
    slot: u16,
    /// Where the holder's shape keeps the accessor at `slot` (`SlotAccessor`): the shape the
    /// entry proves (the receiver's for `OWN_ACCESSOR`, else the prototype holder's, which the
    /// cell covers) fixes it.
    slot_accessor: SlotAccessor,
    /// The validity cell of the chain from `prototype` to `holder` (`Vm::chain_cell`); for an
    /// own accessor, of no prototype.
    cell: crate::heap::ChainCell,
    /// The site's next older entry in `Vm::accessor_fields`, or `NO_ACCESSOR_FIELD`.
    next: u32,
    /// The accessor function whose call `answer` stands for, or undefined: a read site's
    /// getter (`Vm::getter_entry_read`), a store site's setter (`Vm::setter_entry_forward`).
    /// The forward holds while the holder still holds this function: its body, and so what
    /// `answer` derived from it, is the function's own.
    getter: Value,
    /// What the call of `getter` on a receiver of the entry's layout does, without a call
    /// (getters-as-calls.md, trivial accessors).
    answer: AccessorForward,
}
/// What an accessor entry's function does for a receiver of its layout
/// (`AccessorFieldCache::answer`), recognized once from its body: a field read or store at a
/// fixed slot, a constant, or the receiver itself. `None` is no forward: the call runs (a
/// store site's `None` beside its setter records that the setter's body forwards nothing).
#[derive(Clone, Copy)]
pub enum AccessorForward {
    None,
    /// `return this.f` or `return this.#f`: that read's field entry, which proves the
    /// receiver's shape itself (an own or holder entry of a public name; a private name's
    /// own slot under the getter's home).
    Field(FieldCache),
    /// `return k` of a primitive `k` that is no heap value.
    Constant(Value),
    /// `return this` of a function with its own `this` binding.
    Receiver,
    /// `this.f = v` of the setter's first parameter, then `return`: the store entry of `f`
    /// (an own data field of the receiver's exact shape) and its name.
    Store { cache: FieldCache, atom: crate::bytecode::Atom },
}
/// A cache site without an accessor entry (`Vm::accessor_field_indices`).
const NO_ACCESSOR_FIELD: u32 = object::MRU_CHAIN_END;
/// Receiver shapes one site's inherited entries remember: a method inherited from beyond the
/// immediate prototype read on instances of several subclasses (DeltaBlue's constraints)
/// reaches one site with a shape per class. A further shape reuses the oldest entry.
const INHERITED_FIELD_CACHE_WAYS: usize = 4;
// Reusing the oldest entry unlinks it from an older-than-head predecessor.
const _: () = assert!(INHERITED_FIELD_CACHE_WAYS > 1);
/// Receiver shapes one site's accessor entries remember (`Vm::record_accessor_entry`). A
/// getter on a prototype is read from instances that differ by which fields their factory
/// set, as Air's `Arg` kinds do (about ten shapes reading `isStack`); a site past its ways
/// leaves the lane at each miss and evicts an entry another shape needs next.
const ACCESSOR_FIELD_CACHE_WAYS: usize = 16;
use crate::heap::{INHERITED_FIELD_CACHE_MAX_DEPTH, InheritedFieldAnswer, InheritedFieldCache};
pub const NO_FIELD_RECEIVER: u32 = field_cache_layout::NO_RECEIVER;
/// A cache site without an inherited entry (`Vm::inherited_field_indices`).
const NO_INHERITED_FIELD: u32 = object::MRU_CHAIN_END;
// A cached add proves every prototype up to this depth. Node's stream objects (an
// `IncomingMessage` above `Readable`, `Stream`, `EventEmitter` and `Object.prototype`) add
// their fields five prototypes below the chain's end; deeper chains stay uncached. Every
// entry carries this many shapes, and recorded adds proved at most four (V8-v7, npm
// TypeScript and ESLint census, `field_add_cache_depth_counts`, 2026-10-23), so eight keeps
// the streams' five with room and halves the sixteen an inherited read keeps.
const FIELD_ADD_CACHE_MAX_PROTO_DEPTH: usize = 8;
/// Bits of a keyed add's index into `Vm::keyed_adds`.
const KEYED_ADD_INDEX_BITS: u32 = 8;
/// Keyed adds the VM remembers, direct-mapped by source shape and key: express's JSON body
/// path makes fifteen a request into header and parameter objects, whose shapes repeat.
const KEYED_ADD_ENTRIES: usize = 1 << KEYED_ADD_INDEX_BITS;
/// Source shapes one add site remembers; the oldest is evicted beyond this. Objects whose
/// optional fields are assigned in varying order reach one add site with several shapes.
const FIELD_ADD_CACHE_WAYS: usize = 4;
/// The empty entry (`field_cache_layout::EMPTY`), which a fresh cache site holds.
const EMPTY_CACHE: FieldCache = FieldCache {
    receiver: field_cache_layout::EMPTY.receiver,
    holder: field_cache_layout::EMPTY.holder,
    slot: field_cache_layout::EMPTY.slot as u16,
    holder_id: field_cache_layout::EMPTY.holder_id,
};
/// Stored as its `FieldCacheWords`, which complement the entry against `EMPTY`, so the empty entry is zero words.
impl site_table::SiteEntry for FieldCache {
    type Stored = FieldCacheWords;
    #[inline(always)]
    fn store(self) -> FieldCacheWords {
        FieldCacheWords::encode(self.receiver, self.holder, u32::from(self.slot), self.holder_id)
    }
    #[inline(always)]
    fn load(stored: FieldCacheWords) -> Self {
        let (receiver, holder, slot, holder_id) = stored.decode();
        Self { receiver, holder, slot: slot as u16, holder_id }
    }
}
// SAFETY: four `u32` words, with no padding, and every bit pattern valid.
unsafe impl site_table::StoredWords for FieldCacheWords {}
/// The layout generated code reads a field cache entry by (`staged::layout_offsets`).
pub use crate::staged::layout_offsets::NATIVE_FIELD_CACHE_LAYOUT;

/// One cached property-add transition: an extensible ordinary object of `source_shape` whose
/// prototype chain still has exactly the `prototype_depth` shapes interned at `chain`
/// (`Vm::field_add_chains`) gains `atom` at `slot` and moves to `target_shape`.
///
/// 48 bytes: proved chain cell (16), atom, source and target shapes, chain, proved prototype
/// (20), layouts (8), slot and depth (3), padded; an inline `[u32; FIELD_ADD_CACHE_MAX_PROTO_DEPTH]`
/// made it 72, though recorded adds prove at most a few prototypes and the sites of one
/// constructor's stores share one chain.
#[derive(Clone, Copy)]
struct FieldAddCache {
    atom: Atom,
    source_shape: u32,
    target_shape: u32,
    slot: u16,
    /// The layouts of the receiver that recorded the add and of the record it moves to
    /// (`Transition::layouts`), which the staged rows guard and store.
    layouts: [u32; 2],
    prototype_depth: u8,
    /// Where the chain's prototype shapes are interned (`Vm::field_add_chains`).
    chain: u32,
    /// The cell of the chain from `proved_prototype` a walk last proved (`Vm::chain_cell`), or
    /// `None`; while it holds the chain needs no walk. A change to another prototype leaves it.
    proved: Option<crate::heap::ChainCell>,
    /// The receiver's prototype link the proof holds for, compared undecoded; its death
    /// retires the cell, so it never outlives the record it names.
    proved_prototype: ProtoLink,
}
// Keep the entry compact: a monomorphic add site's set is its first entry, a length and a box.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<FieldAddCache>() == 48);
/// The prototype shapes a cached add must find again (`Vm::add_prototype_guard`), nearest
/// first, before an entry interns them as its chain.
#[derive(Clone, Copy)]
struct AddPrototypeGuard {
    depth: u8,
    shapes: [u32; FIELD_ADD_CACHE_MAX_PROTO_DEPTH],
}
impl AddPrototypeGuard {
    const EMPTY: Self = Self { depth: 0, shapes: [NO_FIELD_RECEIVER; FIELD_ADD_CACHE_MAX_PROTO_DEPTH] };
    fn shapes(&self) -> &[u32] {
        &self.shapes[..usize::from(self.depth)]
    }
}
impl FieldAddCache {
    const EMPTY: Self = Self {
        atom: u32::MAX,
        source_shape: NO_FIELD_RECEIVER,
        target_shape: NO_FIELD_RECEIVER,
        slot: 0,
        layouts: [0; 2],
        prototype_depth: 0,
        chain: 0,
        proved: None,
        proved_prototype: ProtoLink::NULL,
    };
    /// The shape transition the entry caches, as the staged add row takes it.
    #[inline(always)]
    fn transition(&self) -> Transition {
        Transition { before: self.source_shape, after: self.target_shape, slot: self.slot, layouts: self.layouts }
    }
    /// Whether the chain proof (`proved`, `proved_prototype`) holds for a receiver whose
    /// prototype `link` names: then the chain needs no walk.
    #[inline(always)]
    fn proof_holds(&self, link: ProtoLink) -> bool {
        self.proved_prototype == link && self.proved.is_some_and(crate::heap::ChainCell::holds)
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
                // Interned chains are equal exactly when their offsets and lengths are.
                entry.source_shape == cache.source_shape
                    && entry.atom == cache.atom
                    && entry.prototype_depth == cache.prototype_depth
                    && entry.chain == cache.chain
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
pub const NO_FIELD_ADD_SET: u32 = u32::MAX;
const NO_MEGAMORPHIC_FIELD: u32 = u32::MAX;
/// `object_shapes` entry of an object literal site that has recorded no shape.
const NO_OBJECT_SHAPE: u32 = u32::MAX;
/// The share of all cache sites, as a divisor, up to which an invalidation resets the
/// written sites one by one: each costs four scattered stores against a sequential fill of
/// every site's entries (Node's library wires prototypes hundreds of times at load, each an
/// invalidation that had refilled every table).
const FIELD_CACHE_SPARSE_RESET_SHARE: usize = 8;
// Bound each site's overflow to 256 recorded shapes; unseen shapes keep the generic fallback.
const FIELD_MEGAMORPHIC_LIMIT: usize = 256;
struct FieldCacheSet {
    len: u8,
    /// Whether any entry recorded an own field (`NO_FIELD_HOLDER`); a set without one names
    /// only prototype holders, so a read probes no own entry.
    holds_own: bool,
    entries: [FieldCache; crate::vm::tier_policy::POLYMORPHISM],
    overflow: Option<Box<FxHashMap<u64, FieldCache>>>,
}
#[quench_macros::test_visible]
#[derive(Clone, Copy)]
struct MethodCache {
    shape: u32,
    atom: Atom,
    proto: Value,
    callee: Value,
    guard: MethodGuard,
    target: Option<CallTarget>,
}
#[quench_macros::test_visible]
#[derive(Clone, Copy, PartialEq, Eq)]
struct MethodGuard {
    owner: Value,
    owner_shape: u32,
    slot: u16,
    depth: u16,
}
struct MethodCacheSet {
    site: usize,
    len: u8,
    entries: [MethodCache; crate::vm::tier_policy::POLYMORPHISM],
}
/// What making a program the executing one reads (`Vm::program_context`): its residual, lane
/// views, constants, cache layout and field cache entries, derived once and again only after
/// something they come from changed (`ProgramStore::context_epoch`), so a switch between
/// programs, as a cross-program call and its return make, looks nothing up and copies no
/// layout. Boxed per program (`Vm::program_contexts`), so its address holds.
pub struct ProgramContext {
    /// The `ProgramStore::context_epoch` it was derived at.
    epoch: u64,
    /// The program's residual; null for a program the store does not hold.
    pub code: *const crate::bytecode::ResidualProgram,
    /// Its lane views by function; a null slice until the program's view table exists
    /// (`Vm::program_context_in_lane`), which a program the lane never enters never makes.
    pub views: *const [program_store::LaneViewCell],
    pub constants: *const [Value],
    layout: ProgramCacheLayout,
    field_entries: *const <FieldCache as site_table::SiteEntry>::Stored,
}

impl ProgramContext {
    /// A context no epoch matches: the active one before any program is.
    fn idle() -> Self {
        Self {
            epoch: u64::MAX,
            code: std::ptr::null(),
            views: std::ptr::slice_from_raw_parts(std::ptr::null(), 0),
            constants: &[],
            layout: ProgramCacheLayout::default(),
            field_entries: std::ptr::null(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProgramCacheLayout {
    field_base: usize,
    method_base: usize,
    object_base: usize,
    /// How many sites of each kind the program's ranges hold from those bases.
    sites: crate::bytecode::CacheSiteCounts,
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
/// Entries of the in-loop call target cache, indexed by the callee's heap index.
pub const CALL_TARGET_CACHE_SIZE: usize = 64;
const _: () = assert!(CALL_TARGET_CACHE_SIZE.is_power_of_two());
/// A user function the dispatch loop may push in place (`Vm::in_loop_call_target`), or construct
/// in place (`construct`), valid for `callee` until collection, realm transition or heap reset
/// clears the cache. Its function kind, environment, realm and body are immutable during that
/// lifetime, so its classifications are computed once, when the entry is filled.
#[derive(Clone, Copy)]
pub struct InLoopCallTarget {
    pub callee: Value,
    pub program: ProgramId,
    pub function: u32,
    env: Value,
    push: InLoopPush,
    construct: ConstructKind,
}
/// How the dispatch loop pushes an in-loop callee.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InLoopPush {
    /// An ordinary body (`CallPlan::in_loop`), whose return is the call's value.
    Body,
    /// An async body (`CallPlan::settles_in_loop`, or one that may suspend), whose completion
    /// settles the promise the call returns and whose first suspension returns that promise
    /// to the caller; only a plain call pushes it.
    Settles,
    /// A callee the loop does not push as a call; its entry serves `new` (`ConstructKind`).
    Unpushed,
}
/// How `new` may push a cached callee in place (`Vm::inline_construct_target`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConstructKind {
    /// Not admitted to an in-loop construction: `new` keeps `construct_value`.
    No,
    /// A base constructor (`Vm::inline_constructible`).
    Base,
    /// A derived class constructor (`Vm::inline_derived_constructible`).
    Derived,
}
const EMPTY_CALL_TARGET: InLoopCallTarget = InLoopCallTarget {
    callee: Value::UNDEFINED,
    program: ProgramId::MAIN,
    function: 0,
    env: Value::UNDEFINED,
    push: InLoopPush::Body,
    construct: ConstructKind::No,
};
/// A user function `new` may push in place, cached beside the call targets (`Vm::in_loop_constructor`),
/// with the same lifetime: its program, function and environment, and which in-place [[Construct]]
/// its body admits (`CONSTRUCT_BASE`, `CONSTRUCT_DERIVED`).
#[derive(Clone, Copy)]
struct ConstructTarget {
    callee: Value,
    program: ProgramId,
    function: u32,
    env: Value,
    admits: u8,
}
/// A base constructor body (`Vm::inline_constructible`).
const CONSTRUCT_BASE: u8 = 1;
/// A derived constructor body (`Vm::inline_derived_constructible`).
const CONSTRUCT_DERIVED: u8 = 2;
const EMPTY_CONSTRUCT_TARGET: ConstructTarget =
    ConstructTarget { callee: Value::UNDEFINED, program: ProgramId::MAIN, function: 0, env: Value::UNDEFINED, admits: 0 };
/// A table of empty targets, which no binding matches: its function is the root, never a callee.
pub static NO_CALL_TARGETS: [InLoopCallTarget; CALL_TARGET_CACHE_SIZE] = [EMPTY_CALL_TARGET; CALL_TARGET_CACHE_SIZE];
/// The code units below which a unit is ASCII.
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
/// A property's attributes as a shape records them. An accessor keeps its functions in the
/// property's slot, its getter alone (`SLOT_GETTER`) or its getter and setter as one pair
/// cell (`SLOT_ACCESSOR_PAIR`), so its shape names no function and every object defining the
/// same keys shares it. A lone setter is rare among shapes and lives in
/// `Vm::shape_accessors`, which `accessors` indexes, so every shape stays small.
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DataAttributes {
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
}
impl Hash for DataAttributes {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // A fixed-width, injective projection of the three flags. One hash write avoids
        // mixing each bool separately; the fields remain the equality/snapshot authority.
        const WRITABLE: u8 = 1 << 0;
        const ENUMERABLE: u8 = 1 << 1;
        const CONFIGURABLE: u8 = 1 << 2;
        let flags = (u8::from(self.writable) * WRITABLE)
            | (u8::from(self.enumerable) * ENUMERABLE)
            | (u8::from(self.configurable) * CONFIGURABLE);
        state.write_u8(flags);
    }
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
/// `ShapeAttributes::accessors` of an accessor with both a getter and a setter: the slot
/// holds a `Cell::AccessorPair` of them. An object literal's `get x() {}` and `set x(v) {}` make a pair of their own on every object (Zod's
/// parse results); in the shape's accessor table each would give its object a shape of its
/// own, which no field cache could keep.
const SLOT_ACCESSOR_PAIR: u32 = u32::MAX - 2;
/// A shape-held accessor property's getter and setter.
type ShapeAccessors = (Option<Value>, Option<Value>);
impl ShapeAttributes {
    /// `attributes` for a shape, appending its accessor functions to `accessors`.
    /// A `SLOT_GETTER` or `SLOT_ACCESSOR_PAIR` record leaves storing the getter, or the pair
    /// cell, in the slot to the caller (`Vm::slot_accessor_value`).
    fn record(attributes: PropertyAttributes, accessors: &mut Vec<ShapeAccessors>) -> Self {
        let index = match (attributes.getter, attributes.setter) {
            (None, None) => NO_SHAPE_ACCESSORS,
            (Some(_), None) if attributes.accessor => SLOT_GETTER,
            (Some(_), Some(_)) if attributes.accessor => SLOT_ACCESSOR_PAIR,
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
        accessors.push(self.functions(old, || (None, None)));
        Self {
            accessors: u32::try_from(accessors.len() - 1).expect("shape accessor table exhausted"),
            ..self
        }
    }

    /// Whether the property's slot holds its getter.
    fn slot_getter(self) -> bool {
        self.accessors == SLOT_GETTER
    }

    /// Whether the property's slot holds the pair cell of its getter and setter.
    fn slot_accessor_pair(self) -> bool {
        self.accessors == SLOT_ACCESSOR_PAIR
    }

    /// Whether the property's slot holds its accessor functions rather than a value: a getter
    /// (`SLOT_GETTER`) or a pair cell (`SLOT_ACCESSOR_PAIR`).
    fn slot_accessors(self) -> bool {
        self.slot_getter() || self.slot_accessor_pair()
    }

    /// Whether `Vm::shape_accessors` holds this property's functions.
    fn table_accessors(self) -> bool {
        !matches!(self.accessors, NO_SHAPE_ACCESSORS | SLOT_GETTER | SLOT_ACCESSOR_PAIR)
    }

    /// The accessor functions; `slot` gives those the property's slot holds
    /// (`slot_accessors`, `Vm::slot_held_accessors`).
    fn functions(self, accessors: &[ShapeAccessors], slot: impl FnOnce() -> ShapeAccessors) -> ShapeAccessors {
        if self.slot_accessors() {
            return slot();
        }
        accessors.get(self.accessors as usize).copied().unwrap_or((None, None))
    }

    fn attributes(self, accessors: &[ShapeAccessors], slot: impl FnOnce() -> ShapeAccessors) -> PropertyAttributes {
        let (getter, setter) = self.functions(accessors, slot);
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
            Self::Add { key, .. } => key.gc_value().is_some(),
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
            Self::Delete { key, .. } if key.gc_value().is_none() => {
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
    /// A class cell's object departs its class on an add past its capacity or a delete
    /// (`Heap::depart_class_object`), so it leaves the class's shape for a dictionary of its own.
    ClassDeparture,
}
impl DictionaryTrigger {
    #[cfg(feature = "profile-aggregate")]
    pub(crate) const COUNT: usize = 5;

    #[cfg(feature = "profile-aggregate")]
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::PropertyCount => 0,
            Self::DeletionPattern => 1,
            Self::PrototypeUse => 2,
            Self::ModuleNamespace => 3,
            Self::ClassDeparture => 4,
        }
    }
}
struct ShapeLookupIndex {
    /// The shape's properties in definition order with their slots; deleted ones are `VACANT_SLOT`.
    entries: Vec<(property_key::PropertyKey, u32)>,
    /// Positions in `entries`, hashed by key: the pairs themselves are stored once.
    positions: hashbrown::HashTable<u32>,
    attributes: FxHashMap<u32, ShapeAttributes>,
    /// An owned dictionary's deletes and descriptor edits that made a shape (`owned_dictionary_edit`).
    edits: u8,
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
        Self { entries, positions, attributes, edits: 0 }
    }
    /// The index of a child shape, from its parent's index and the transition between
    /// them, as `derive_shape_lookup_index` would build it.
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
            ShapeTransition::Delete { key, slot } => self.remove(key, slot)?,
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
    #[inline(always)] fn slot(&self, key: property_key::PropertyKey) -> Option<u32> {
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
/// keeps it. Two bytes, so `Shape` keeps its size; bits 10 and 11 hold the shape's child count
/// (`CHILDREN_MASK`, not inherited), and the top three bits its dictionary trigger
/// (`DICTIONARY_MASK`).
#[derive(Clone, Copy, Default)]
struct ShapeFacts(u16);
impl ShapeFacts {
    /// A symbol key or a table accessor of the chain may be a GC root.
    const MAY_HAVE_GC_ROOTS: u16 = 1 << 0;
    /// Some property of the chain was a `SLOT_GETTER` accessor, so a raw slot read must check
    /// whether the slot holds a getter rather than a value. A pair cell's slot is told by its
    /// value instead (`Vm::is_accessor_pair`), under `ACCESSOR_PAIRS`, so a shape of data
    /// properties and pairs, as a logger object with a `level` pair, keeps reading raw.
    const SLOT_GETTERS: u16 = 1 << 1;
    /// Some property of the chain was made an accessor. Only a descriptor transition makes
    /// one, as an add carries data attributes, so a chain without this fact holds data
    /// properties alone and a read of any slot is its value.
    const ACCESSORS: u16 = 1 << 2;
    /// Some property of the chain was made an accessor with a setter, as every lazy data
    /// property is, rather than a getter alone: its functions in `Vm::shape_accessors`
    /// (`ShapeAttributes::table_accessors`) or a pair cell in its slot (`slot_accessor_pair`).
    const ACCESSOR_PAIRS: u16 = 1 << 3;
    /// Some property of the chain was deleted, or left a vacant slot when it was rebuilt: a
    /// deletion past `SHARED_DELETIONS` makes the object its own dictionary (`DictionaryTrigger::DeletionPattern`).
    const DELETIONS: u16 = 1 << 4;
    /// A transition introduced non-default slot attributes. Without this fact every
    /// allocated slot has the default data descriptor, including vacated slots.
    const NON_DEFAULT_ATTRIBUTES: u16 = 1 << 5;
    /// The chain recorded a function's original name (`ShapeTransition::FunctionName`): its own
    /// `name` and `length`, where it still has them, are the creation-time properties unless
    /// one of the two facts below says a redefinition changed it.
    const FUNCTION_NAMED: u16 = 1 << 6;
    /// The function's own `name` was redefined (`ShapeTransition::FunctionNameReconfigured`).
    const NAME_RECONFIGURED: u16 = 1 << 7;
    /// The function's own `length` was redefined (`ShapeTransition::FunctionLengthReconfigured`).
    const LENGTH_RECONFIGURED: u16 = 1 << 8;
    /// A transition added a property under a non-string key: a symbol or a private name (any
    /// `PropertyKey` but `String`). Read by `Vm::shape_may_have_symbol_keys`.
    const HAS_SYMBOL_KEY: u16 = 1 << 9;

    fn extended_by(self, transition: ShapeTransition) -> Self {
        let introduced = |fact: bool, bit: u16| if fact { bit } else { 0 };
        let descriptor = |test: fn(ShapeAttributes) -> bool| {
            matches!(transition, ShapeTransition::Descriptor { attributes, .. } if test(attributes))
        };
        Self(
            self.0
                | introduced(transition.introduces_gc_roots(), Self::MAY_HAVE_GC_ROOTS)
                | introduced(
                    matches!(transition, ShapeTransition::Add { attributes, .. } if attributes != DEFAULT_DATA_ATTRIBUTES)
                        || descriptor(|attributes| attributes != DEFAULT_SHAPE_ATTRIBUTES),
                    Self::NON_DEFAULT_ATTRIBUTES,
                )
                | introduced(descriptor(ShapeAttributes::slot_getter), Self::SLOT_GETTERS)
                | introduced(descriptor(|attributes| attributes.accessor), Self::ACCESSORS)
                | introduced(
                    descriptor(|attributes| attributes.table_accessors() || attributes.slot_accessor_pair()),
                    Self::ACCESSOR_PAIRS,
                )
                | introduced(matches!(transition, ShapeTransition::Delete { .. } | ShapeTransition::Vacant), Self::DELETIONS)
                | introduced(matches!(transition, ShapeTransition::FunctionName { .. }), Self::FUNCTION_NAMED)
                | introduced(matches!(transition, ShapeTransition::FunctionNameReconfigured), Self::NAME_RECONFIGURED)
                | introduced(matches!(transition, ShapeTransition::FunctionLengthReconfigured), Self::LENGTH_RECONFIGURED)
                | introduced(matches!(transition, ShapeTransition::Add { key, .. } if !matches!(key, property_key::PropertyKey::String(_))), Self::HAS_SYMBOL_KEY),
        )
    }

    /// The child count, in bits 10 and 11: none, exactly one, or `CHILDREN_MANY` for two or more,
    /// which saturates. Set per shape by `push_child` as its children are added, and never
    /// inherited: `Shape::child` clears the bits `extended_by` copies from the parent.
    const CHILDREN_SHIFT: u32 = 10;
    const CHILDREN_MASK: u16 = 0b11 << Self::CHILDREN_SHIFT;
    const CHILDREN_MANY: u16 = 2;

    /// The child count: 0, 1, or `CHILDREN_MANY`.
    #[inline(always)]
    fn children(self) -> u16 {
        (self.0 & Self::CHILDREN_MASK) >> Self::CHILDREN_SHIFT
    }

    /// These facts with `children` as the child count.
    fn with_children(self, children: u16) -> Self {
        Self((self.0 & !Self::CHILDREN_MASK) | (children << Self::CHILDREN_SHIFT))
    }

    /// The dictionary trigger, in the top three bits: zero for none, else the trigger's
    /// index plus one. Set per shape (`Shape::child`), not by `extended_by`.
    const DICTIONARY_SHIFT: u32 = 13;
    const DICTIONARY_MASK: u16 = 0b111 << Self::DICTIONARY_SHIFT;

    /// These facts with `trigger` as the dictionary trigger.
    fn with_dictionary_trigger(self, trigger: Option<DictionaryTrigger>) -> Self {
        let code: u16 = match trigger {
            None => 0,
            Some(DictionaryTrigger::PropertyCount) => 1,
            Some(DictionaryTrigger::DeletionPattern) => 2,
            Some(DictionaryTrigger::PrototypeUse) => 3,
            Some(DictionaryTrigger::ModuleNamespace) => 4,
            Some(DictionaryTrigger::ClassDeparture) => 5,
        };
        Self((self.0 & !Self::DICTIONARY_MASK) | (code << Self::DICTIONARY_SHIFT))
    }

    #[inline(always)]
    fn dictionary_trigger(self) -> Option<DictionaryTrigger> {
        match (self.0 & Self::DICTIONARY_MASK) >> Self::DICTIONARY_SHIFT {
            0 => None,
            1 => Some(DictionaryTrigger::PropertyCount),
            2 => Some(DictionaryTrigger::DeletionPattern),
            3 => Some(DictionaryTrigger::PrototypeUse),
            4 => Some(DictionaryTrigger::ModuleNamespace),
            _ => Some(DictionaryTrigger::ClassDeparture),
        }
    }

    #[inline(always)]
    fn is_dictionary(self) -> bool { self.0 & Self::DICTIONARY_MASK != 0 }

    fn accessor_pairs(self) -> bool { self.0 & Self::ACCESSOR_PAIRS != 0 }
    fn accessors(self) -> bool { self.0 & Self::ACCESSORS != 0 }
    fn default_attributes(self) -> bool { self.0 & Self::NON_DEFAULT_ATTRIBUTES == 0 }
    fn deletions(self) -> bool { self.0 & Self::DELETIONS != 0 }
    fn may_have_gc_roots(self) -> bool { self.0 & Self::MAY_HAVE_GC_ROOTS != 0 }
    fn slot_getters(self) -> bool { self.0 & Self::SLOT_GETTERS != 0 }
    fn function_named(self) -> bool { self.0 & Self::FUNCTION_NAMED != 0 }
    fn name_reconfigured(self) -> bool { self.0 & Self::NAME_RECONFIGURED != 0 }
    fn length_reconfigured(self) -> bool { self.0 & Self::LENGTH_RECONFIGURED != 0 }
    fn has_symbol_key(self) -> bool { self.0 & Self::HAS_SYMBOL_KEY != 0 }
}
/// Each shape's storage length and its parent's id and length, in table order (a parent
/// precedes its children): the table as the heap registers slot owners from it.
fn shape_layouts(shapes: &[Shape]) -> Vec<(usize, Option<(u32, usize)>)> {
    shapes.iter()
        .map(|shape| (shape.storage_len(), shape.parent().map(|parent| (parent, shapes[parent as usize].storage_len()))))
        .collect()
}

struct Shape {
    /// The shape this one extends; `NO_PARENT_SHAPE` for the root.
    parent: u32,
    transition: ShapeTransition,
    /// Property slots an object of this shape stores (slot indices are `u32`).
    storage_len: u32,
    /// The chain's facts and, in its top bits, the dictionary trigger
    /// (`ShapeFacts::dictionary_trigger`).
    facts: ShapeFacts,
    /// This immutable shape's lazily derived memos in one word (`ShapeMemo`): its own data
    /// `prototype` slot, whether its keys include an array-index name, and the long walks
    /// spent toward a lookup index.
    memo: std::cell::Cell<u16>,
    lookup_index: OnceCell<Box<ShapeLookupIndex>>,
    /// The child of this shape's latest cached transition (`NO_LAST_CHILD` before one), tried
    /// before the transition index: objects of one layout grow by the same keys in turn.
    last_child: std::cell::Cell<u32>,
}
// Every object layout and property key costs one shape; keep the record compact: 4 (parent)
// + 24 (transition) + 4 (storage_len) + 2 (facts with the dictionary trigger) + 2 (memo) + 8
// (lookup index) + 4 (last child) = 48, where separate memo and trigger fields made 56.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<Shape>() == 48);
/// `Shape::parent` of the root shape.
const NO_PARENT_SHAPE: u32 = u32::MAX;
/// `Shape::last_child` before the shape has a cached transition.
const NO_LAST_CHILD: u32 = u32::MAX;
const PROTOTYPE_SLOT_UNKNOWN: u32 = u32::MAX;
const PROTOTYPE_SLOT_NONE: u32 = u32::MAX - 1;
/// The bits of `Shape::memo`. The `prototype` slot memo takes the low ten bits: a slot below
/// `MEMO_PROTOTYPE_NONE` is memoized as itself, a slot at or past it (a function with a
/// thousand own properties before `prototype`) is never memoized and is derived again.
struct ShapeMemo;
impl ShapeMemo {
    const PROTOTYPE_MASK: u16 = (1 << 10) - 1;
    const PROTOTYPE_UNKNOWN: u16 = Self::PROTOTYPE_MASK;
    const PROTOTYPE_NONE: u16 = Self::PROTOTYPE_MASK - 1;
    const INDEX_KEYS_SHIFT: u32 = 10;
    const INDEX_KEYS_MASK: u16 = 0b11 << Self::INDEX_KEYS_SHIFT;
    const LONG_WALKS_SHIFT: u32 = 12;
    const LONG_WALKS_MAX: u8 = 15;
    const INITIAL: u16 = Self::PROTOTYPE_UNKNOWN;
}
// The long-walk count reaches `SHAPE_INDEX_LONG_WALKS` in its four bits.
const _: () = assert!(object::SHAPE_INDEX_LONG_WALKS <= ShapeMemo::LONG_WALKS_MAX);
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

    #[inline(always)] fn parent(&self) -> Option<u32> {
        (self.parent != NO_PARENT_SHAPE).then_some(self.parent)
    }

    fn storage_len(&self) -> usize {
        self.storage_len as usize
    }

    /// What made this shape and its descendants dictionaries, if anything did.
    #[inline(always)]
    fn dictionary_trigger(&self) -> Option<DictionaryTrigger> {
        self.facts.dictionary_trigger()
    }

    /// Whether this shape is a dictionary's (`dictionary_trigger`).
    #[inline(always)]
    fn is_dictionary(&self) -> bool {
        self.facts.is_dictionary()
    }

    /// The memo of this shape's own data `prototype` slot: `PROTOTYPE_SLOT_UNKNOWN` until
    /// derived, `PROTOTYPE_SLOT_NONE` when absent or an accessor.
    #[inline(always)]
    fn prototype_slot(&self) -> u32 {
        match self.memo.get() & ShapeMemo::PROTOTYPE_MASK {
            ShapeMemo::PROTOTYPE_UNKNOWN => PROTOTYPE_SLOT_UNKNOWN,
            ShapeMemo::PROTOTYPE_NONE => PROTOTYPE_SLOT_NONE,
            slot => u32::from(slot),
        }
    }

    /// Memoizes `slot` as `prototype_slot`; a slot past the memo's range stays unknown.
    fn set_prototype_slot(&self, slot: u32) {
        let bits = match slot {
            PROTOTYPE_SLOT_NONE => ShapeMemo::PROTOTYPE_NONE,
            slot if slot < u32::from(ShapeMemo::PROTOTYPE_NONE) => slot as u16,
            _ => return,
        };
        self.memo.set((self.memo.get() & !ShapeMemo::PROTOTYPE_MASK) | bits);
    }

    /// The memo of whether this shape's keys include an array-index name, once derived.
    #[inline(always)]
    fn index_keys(&self) -> Option<bool> {
        match (self.memo.get() & ShapeMemo::INDEX_KEYS_MASK) >> ShapeMemo::INDEX_KEYS_SHIFT {
            0 => None,
            1 => Some(false),
            _ => Some(true),
        }
    }

    fn set_index_keys(&self, found: bool) {
        let bits: u16 = (if found { 2u16 } else { 1u16 }) << ShapeMemo::INDEX_KEYS_SHIFT;
        self.memo.set((self.memo.get() & !ShapeMemo::INDEX_KEYS_MASK) | bits);
    }

    /// Lookups that walked at least `SHAPE_INDEX_WALK_DISTANCE` transitions from this shape
    /// before it had an index.
    #[inline(always)]
    fn long_walks(&self) -> u8 {
        (self.memo.get() >> ShapeMemo::LONG_WALKS_SHIFT) as u8
    }

    fn set_long_walks(&self, walks: u8) {
        let walks = u16::from(walks.min(ShapeMemo::LONG_WALKS_MAX));
        self.memo.set((self.memo.get() & !(0xF_u16 << ShapeMemo::LONG_WALKS_SHIFT)) | (walks << ShapeMemo::LONG_WALKS_SHIFT));
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
            facts: parent_facts
                .extended_by(transition)
                .with_children(0)
                .with_dictionary_trigger(dictionary_trigger),
            memo: std::cell::Cell::new(ShapeMemo::INITIAL),
            lookup_index: OnceCell::new(),
            last_child: std::cell::Cell::new(NO_LAST_CHILD),
        }
    }
}

/// Appends `child` to the shape table, counting it among its parent's children
/// (`ShapeFacts::children`). A parent's first child is also its `last_child`, so a walk down a
/// chain of single children (`Vm::predicted_run_capacity`) finds the child without the
/// transition index. Every shape made from a parent is appended here, so the counts are exact
/// for the table as it stands.
pub(crate) fn push_child(shapes: &mut Vec<Shape>, child: Shape) {
    let id = u32::try_from(shapes.len()).expect("object shape table exhausted");
    if let Some(parent) = child.parent() {
        let entry = &mut shapes[parent as usize];
        let children = entry.facts.children();
        if children == 0 {
            entry.last_child.set(id);
        }
        entry.facts = entry.facts.with_children((children + 1).min(ShapeFacts::CHILDREN_MANY));
    }
    shapes.push(child);
}
#[quench_macros::test_visible]
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallTarget {
    User(ProgramId, u32, Value),
    Native(Native),
}
pub enum StepResult {
    Continue,
    TailCall,
    #[cfg_attr(not(feature = "native-execution"), allow(dead_code))]
    ResumeFrames(Box<dispatch_frame::RecoveredCalls>),
    PushFrame {
        destination: Register,
        call_guard: crate::stack::GuestCallGuard,
        completion: CallCompletion,
    },
    Return(Value),
    /// A suspension point (`await`, `yield`) that leaves the general loop.
    Suspend(Suspended),
}
/// The facts of one suspension point, shared by `StepResult::Suspend` and
/// `FrameOutcome::Suspend`: the value handed out and the register that receives the resumption.
pub enum Suspended {
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
pub enum FrameOutcome {
    Complete(Value),
    ConstructComplete {
        value: Value,
        this: Value,
    },
    ParameterInitializationComplete,
    /// A suspension, with the activation's frame when the caller retains it. The frame is boxed
    /// so every `FrameOutcome` stays a few words: the common completions move it through each
    /// call return.
    Suspend(Suspended, Option<Box<Frame>>),
}

impl FrameOutcome {
    /// The value a call's completed activation yields: a return, or a constructor's.
    #[inline(always)]
    pub fn into_value(self) -> Result<Value, JsError> {
        match self {
            Self::Complete(value) | Self::ConstructComplete { value, .. } => Ok(value),
            Self::ParameterInitializationComplete => {
                Err(JsError("parameter initialization escaped a generator activation".into()))
            }
            Self::Suspend(Suspended::Await { .. }, _) => Err(JsError("await requires async continuation".into())),
            Self::Suspend(Suspended::Yield { .. }, _) => Err(JsError("yield requires generator continuation".into())),
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
#[quench_macros::test_visible]
const EMPTY_METHOD_CACHE: MethodCache = MethodCache {
    shape: u32::MAX,
    atom: u32::MAX,
    proto: Value::UNDEFINED,
    callee: Value::NULL,
    guard: EMPTY_METHOD_GUARD,
    target: None,
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PropertyAttributes {
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
#[quench_macros::test_visible]
pub struct Vm {
    /// Typed array views' element addresses by view (`typed_array_access::TypedViewCache`).
    typed_views: typed_array_access::TypedViewCache,
    /// VM-wide running-job admission for native microtask checkpoints.
    native_microtask_running: bool,
    /// The async function a microtask drain resumed whose activation the running lane run is
    /// rooted in (`Vm::resume_job`), else undefined: its `await` of a settled value continues in
    /// place when no other job is queued (`await_miss`).
    pub queue_resumed: Value,
    /// Whether the innermost running job queue has entered the host's context for unobserved
    /// Promise jobs (`Host::enter_unobserved_job`): its consecutive jobs share that entry.
    unobserved_jobs_entered: bool,
    /// The native whose fast in-place body a dispatcher tried and declined on the call that is
    /// about to reach its general native, so that native skips that body (the one-shot flag is
    /// taken at each such native's entry, and cleared at each dispatcher attempt).
    pub declined_fast: Option<Native>,
    /// The index a declined `indexOf` or `includes` dense walk stopped at (`Err(Some(index))` of
    /// `dense_search_resume`), recorded with `declined_fast` naming that native and taken with it
    /// by the general native, which resumes its steps there instead of rescanning the prefix.
    pub declined_resume: Option<usize>,
    /// The programs the running job queues return to, innermost last (`Vm::run_job_queue`):
    /// an await's job leaves its function's program active for the queue's next jobs, so a
    /// collection keeps each of these, which the queue makes active again when it ends.
    job_queue_programs: Vec<ProgramId>,
    /// Whether the innermost running job queue drains with no stopping condition
    /// (`Vm::run_job_queue`), so an await's job it runs may chain the queue's next ones in its
    /// lane run (`Vm::chain_next_job`).
    queue_chains: bool,
    /// Whether the running job is an unobserved Promise job of a queue that chains
    /// (`Vm::run_promise_job`), until the await resumption it runs takes it
    /// (`Vm::resume_queued_await`).
    chainable_job: bool,
    pub host: Box<dyn Host>,
    /// The host type's associated functions (`HostStatics::of`), the only host calls that
    /// are not methods of `host`.
    pub(crate) host_statics: crate::host::HostStatics,
    /// `Host::runs_lane`, read once: whether bodies run in the lane (`dispatch_fast`).
    pub runs_lane: bool,
    /// The intrinsic families this realm installs (`builtin_table`): `ALL` unless an image
    /// proved a family unreachable from its program.
    pub(crate) builtins: &'static BuiltinTable,
    /// The engine entries this runtime links (`execution_table`): `ALL` unless an image proved
    /// some of them unreachable from its program.
    pub(crate) execution: &'static ExecutionTable,
    pub specialized: bool,
    pub(crate) heap: Heap,
    pub realm: Realm,
    pub object_proto: Value,
    function_proto: Value,
    pub array_proto: Value,
    /// %String.prototype% of the current realm; primitive string property
    /// reads use it directly, so realm switches re-derive it.
    pub string_proto: Value,
    /// %Promise.prototype% of the current realm, which every promise allocation takes
    /// (`Vm::promise_object_with_parent`); it follows the active realm as `string_proto` does.
    promise_proto: Value,
    /// %Promise% of the current realm (`Vm::intrinsic_promise_constructor`), kept beside the
    /// intrinsics table's entry and following the active realm with `promise_proto`.
    promise_constructor: Value,
    /// %Number.prototype% of the main realm: a number primitive's field read takes it
    /// through the site's cache as a string's takes `string_proto`.
    number_proto: Value,
    array_buffer_proto: Value,
    shared_array_buffer_proto: Value,
    array_iterator_proto: Value,
    typed_array_proto: Value,
    /// Each typed-array kind's prototype in the active realm, by discriminant (`TypedArrayKind as usize`):
    /// one list that construction, installation, GC roots and the snapshot all read.
    typed_array_protos: [Value; crate::heap::TypedArrayKind::COUNT],
    pub data_view_proto: Value,
    pub map_proto: Value,
    pub set_proto: Value,
    shadow_realm_proto: Value,
    map_iterator_proto: Value,
    set_iterator_proto: Value,
    pub weak_map_proto: Value,
    pub weak_set_proto: Value,
    weak_ref_proto: Value,
    /// ECMAScript [[KeptAlive]] targets, held until the host ends a job turn.
    /// The roots `keep_alive` holds until the job ends, by the kept value's bits: one lookup per
    /// `new WeakRef`, where a scan of the list made N of them in one job cost O(N²).
    kept_alive: rustc_hash::FxHashMap<u64, RootId>,
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
    pub temporal_plain_date_proto: Value,
    pub temporal_plain_time_proto: Value,
    pub temporal_plain_month_day_proto: Value,
    pub temporal_plain_year_month_proto: Value,
    natives: builtins::NativeTable,
    /// The first realm this VM initialized: the creation realm every function record without a
    /// realm override carries (`Vm::record_realm`). Set once and kept, rooted by the GC.
    main_realm: Value,
    // Boxed: frames move between this stack, `frame_pool` and suspended generators by pointer.
    #[allow(clippy::vec_box)]
    pub frames: Vec<Box<Frame>>,
    native_stack_frames: Vec<(usize, String)>,
    /// Emptied root lists of finished native calls, reused by the next `NativeContext`.
    pub(crate) native_root_lists: Vec<Vec<RootId>>,
    /// Owners with out-of-cell edges at the previous collection, which sizes the next index.
    gc_owned_root_count: usize,
    // Boxed: pooled frames are reused as the `Box<Frame>` the stack and generators hold.
    #[allow(clippy::vec_box)]
    frame_pool: Vec<Box<Frame>>,
    /// Functions lowered from tier 0 whose unlowered copy the next full collection drops
    /// (`Vm::release_lowered_tier0`).
    /// By residual address, which the collection matches against the live programs'.
    lowered_tier0: std::cell::RefCell<Vec<(usize, u32)>>,
    /// Functions proved to be the current realm's natives (`Vm::is_current_realm_native`).
    realm_native_proofs: iterators::RealmNativeProofs,
    /// Emptied call stacks of finished general loops, one per nesting level reached: a loop
    /// takes one on entry, so running a job allocates no stack of its own.
    pub pending_call_stacks: Vec<Vec<dispatch_frame::PendingGeneralCall>>,
    /// Idle JS entry contexts, one per nesting level reached: an entry takes one and a
    /// reentrant entry under it another, so nested native calls allocate none.
    #[allow(clippy::vec_box)]
    #[cfg_attr(not(feature = "native-execution"), allow(dead_code))]
    js_native_contexts: Vec<Box<crate::vm::engine_types::CallContext>>,
    /// The environments native Wasm code holds across its calls into other instances.
    pub native_roots: Box<crate::vm::engine_types::NativeRoots>,
    /// Activations a running lane holds without frames, boxed so a nested run swaps pointers.
    pub lane_stack: Box<crate::vm::engine_types::LaneStack>,
    /// The lane stacks of the runs a nested lane run parked, outermost first (`Vm::nested_lane_run`).
    pub lane_roots: Vec<crate::vm::engine_types::ParkedLane>,
    /// Empty lane stacks a nested run returned, which the next one starts over
    /// (`Vm::nested_lane_run`): a fresh stack reserves its whole value and activation room and
    /// starts with no call target resolved.
    pub spare_lane_stacks: Vec<Box<crate::vm::engine_types::LaneStack>>,
    /// Whether Wasm functions with a compiled denotation run as native code.
    pub execution_mode: ExecutionMode,
    /// Whether any compiled tier may run under this execution mode and memory policy, decided at
    /// the first lane run after either changes (`Vm::lane_tiers_run`): none until then.
    pub lane_tiers_run: Option<bool>,
    pub native_execution_rejections: u64,
    /// Numeric function bodies whose compile runs on the code generator's thread (`Vm::js_tier_compile`).
    pub js_tier_pending: FxHashMap<(ProgramId, u32), jit_backend::OffThread<Option<crate::vm::engine_types::NumericImage>>>,
    #[cfg(feature = "native-execution")]
    pub native_program_resolver: Option<Box<engine_types::NativeProgramResolver>>,
    pub execution_interrupt: crate::ExecutionInterrupt,
    pub active_call_roots: Vec<Value>,
    /// The frame index of each root activation this Vm's entry points are running (`call_root`,
    /// `call_eval_closure`), innermost last. Global var paths walk them rather than every frame.
    active_roots: Vec<usize>,
    /// The roots of prepared callbacks' proofs (`CallbackProof::Lane`): each proof's callee,
    /// receiver and argument slots, innermost proof last. Apart from `active_call_roots`, so a
    /// native's own nested call roots (`with_call_roots`) neither drop nor displace them.
    prepared_roots: Vec<Value>,
    /// The step natives an iteration proved (`Vm::proved_step_native`), one per `StepNative`.
    step_native_proofs: [iterators::StepNativeProof; iterators::STEP_NATIVES],
    pub with_stack: Vec<Value>,
    /// The realm's intrinsics as the host bootstrap left them (`seal_startup_intrinsics`).
    startup_intrinsics: startup_intrinsics::StartupIntrinsics,
    /// Set by the first `with` entry and never cleared: `Native::WithEnter` is
    /// the only source of with objects, so until it runs no environment
    /// carries any.
    pub with_scope_entered: bool,
    /// What lane reads of names past an eval's scope rely on (`name_scope`).
    pub scope_names: name_scope::DynamicScopeNames,
    test262_agent: Test262AgentState,
    shared_async_waiters:Vec<atomics::SharedAsyncWaiter>,
    pub programs: ProgramStore,
    program_cache_layouts: Vec<ProgramCacheLayout>,
    /// The cache-site ranges of reclaimed programs, by their site counts, which the next
    /// program with the same counts takes over (`append_cache_layout`): a loop of `eval` or
    /// `new Function` makes programs alike, and each had appended ranges that never shrink.
    free_cache_layouts: FxHashMap<crate::bytecode::CacheSiteCounts, Vec<ProgramCacheLayout>>,
    /// The register file of the frame the general loop dispatches; see `run_general_loop`.
    pub active_registers: *mut Value,
    /// The staged row the general loop's fast prelude decoded for the instruction it dispatches and
    /// found the frame left: the arm that runs next takes it as declined, rather than decode and try it again.
    pending_row: Option<crate::staged::Operation<u64, ()>>,
    /// The step result a staged row stopped the general loop's run with (a return, a raise or a
    /// suspension); the fast prelude takes it as the instruction's step result.
    row_stop: Option<Result<StepResult, JsError>>,
    /// How the last lane run left its activation, for the code it exits to to take (`LaneCompletion`).
    pub lane_completion: LaneCompletion,
    /// Where a lane body's native `throw` raised the error in `lane_completion`
    /// (`compiled::raise_site`), which the lane gives it as its source location once it
    /// escapes the native frames (`dispatch_fast::raised_exit_in_lane`).
    pub lane_raise_site: u64,
    /// A lane native call running with its activations still frameless
    /// (`Native::runs_beside_activations`), which the first observer of the frame stack makes
    /// frames (`materialize_deferred_lane`).
    pub lane_deferred: Option<crate::vm::engine_types::DeferredLaneCall>,
    /// The scope `VarOwner::Window` names, read by `Vm::running_owner` while `lane_deferred` stands.
    pub running_window: Option<crate::vm::engine_types::WindowScope>,
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
    /// The native a lane call is running from its caller's frame (`dispatch_fast::call_native_in_lane`),
    /// whose calls of JavaScript run frameless in that lane (`Vm::native_callback_in_lane`).
    pub lane_native: Option<crate::vm::engine_types::LaneNativeCall>,
    /// The active program's context (`program_contexts`), kept by `set_active_program`: its
    /// cache layout and constants are one load from it.
    active_context: *const ProgramContext,
    /// Each program's context by its index (`program_context`), derived on its first use.
    program_contexts: Vec<Box<ProgramContext>>,
    /// The context active before any program is, which `active_context` names until then.
    idle_context: Box<ProgramContext>,
    /// The active program's field cache entries (`CacheSiteTable::entries_from` of its
    /// `field_base`), re-derived by `refresh_active_field_entries` whenever the active program
    /// changes or the table grows: a site's entry is then one load from it.
    active_field_entries: *const <FieldCache as site_table::SiteEntry>::Stored,
    /// The VM's invalidation counters (`cache_epochs`): the collection count caches key on, and
    /// the field cache table's moves (`CacheEpochs::field_entries`), which a run that kept a
    /// pointer into the table across code that may install a program compares.
    pub cache_epochs: cache_epochs::CacheEpochs,
    pub active_program: ProgramId,
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
    shape_attribute_memo: shape_memo::ShapeAttributeMemo,
    plain_copy_shapes: shape_memo::PlainCopyShapeMemo,
    plain_assign_shapes: shape_memo::PlainAssignShapeMemo,
    /// The shapes an empty target takes when it adopts a plain source (`adopt_source_shape`).
    adopted_shapes: shape_memo::AdoptedShapeMemo,
    /// The final shapes of host records, by their names list (`create_embedding_record`).
    embedding_record_shapes: FxHashMap<usize, u32>,
    /// The geometry accessors typed array reads resolve to (`typed_array_geometry_read`).
    typed_array_geometry: typed_array::TypedArrayGeometryProofs,
    function_name_shapes: provenance::FunctionNameShapes,
    symbol_write_proofs: symbol_write_proof::SymbolWriteProofs,
    symbol_load_proofs: symbol_load_proof::SymbolLoadProofs,
    /// Lane private-name sites' homes (`private_site_home`).
    pub private_site_homes: private_site_home::PrivateSiteHomes,
    /// The classes evaluated once and the private names they own, whose sites each meet one
    /// home for good, so a receiver shape alone proves the brand (`Vm::note_class_evaluation`).
    pub private_classes: private_site_home::PrivateClasses,
    /// An array shape with no own `@@iterator` (`Vm::array_iteration_intact`).
    array_iteration_shape: Option<u32>,
    /// A Set or Map shape with no own `@@iterator` (`Vm::intact_iteration_kind`).
    collection_iteration_shape: Option<u32>,
    array_species_proof: Option<array::ArraySpeciesProof>,
    /// An array shape no `@@isConcatSpreadable` reaches, while the cell of `Array.prototype`
    /// and `Object.prototype` holds (`Vm::concat_spreadable_absent`).
    concat_spreadable_proof: Option<(u32, crate::heap::ChainCell)>,
    for_in: object_static::ForInMemo,
    /// What `JSON.stringify`'s plain writer keeps between calls: its text buffer, which a
    /// payload's text would otherwise regrow from empty on each call, and its member plans
    /// (`json::JsonPlainCache`).
    json_plain: json::JsonPlainCache,
    species_slot_proof: std::cell::Cell<Option<object_get::SpeciesSlotProof>>,
    /// That an iterator prototype's chain lacks a name (`Vm::iterator_lacks_property`): the
    /// prototype, the name (`PropertyKey::key32`) and the cell of the chain it was proved over.
    iterator_absence_proof: std::cell::Cell<Option<(Value, u32, crate::heap::ChainCell)>>,
    /// That a realm's `%Object.prototype%` chain lacks `then` (`Vm::then_absence_proved`): the
    /// prototype and the cell of the chain it was proved over.
    then_absence_proof: Option<(Value, crate::heap::ChainCell)>,
    /// The last proof that a promise's species constructor is `%Promise%` (`promise_species_proved`).
    promise_species_proof: std::cell::RefCell<Option<promise::PromiseSpeciesProof>>,
    /// The `then` a resolution read from promises of one layout (`Vm::memoized_resolution_then`).
    promise_then_memo: std::cell::Cell<Option<promise::PromiseThenMemo>>,
    enumerable_keys: enumerable_key_memo::EnumerableKeyMemo,
    /// Each shape's own enumerable string keys as atoms, by shape id (`Vm::own_enumerable_keys_cached`).
    enumerable_key_lists: Vec<Option<std::rc::Rc<[Atom]>>>,
    /// Each shape's own enumerable string keys as string values, by shape id
    /// (`Vm::own_enumerable_key_values_cached`). Not kept by the heap: cleared at every collection.
    enumerable_key_value_lists: Vec<Option<std::rc::Rc<[Value]>>>,
    name_atom_memo: name_atom_memo::NameAtomMemo,
    atom_text: AtomTable,
    /// Every atom, hashed by its text (`atom_keys::text_hash`); equal hashes are told apart
    /// by comparing the texts, which the atom tables hold.
    atoms: hashbrown::HashTable<Atom>,
    /// The atoms that spell canonical array indices, by index: index-keyed lookups find
    /// them without formatting and hashing the decimal text.
    array_index_atoms: IndexAtoms,
    /// Every atom name in id order (`atom_text`, then the names the VM appends), shared by
    /// the dynamic and module programs whose atoms are a prefix of the VM's.
    pub atom_sequence: Rc<crate::bytecode::AtomSequence>,
    /// The strings most recently matched by a RegExp whose search decodes them, most recent
    /// first, each prepared once for repeated matching (`Vm::regexp_subject`).
    regexp_subjects: regexp::SubjectCache,
    /// The atoms concatenations of atom strings and integers spelled (`Vm::memoized_concat`).
    concat_memo: string_cache::ConcatMemo,
    regexp_matchers: regexp::RegExpMatcherCache,
    // One lazily derived class byte per atom; length tracks `atom_sequence`.
    atom_classes: Vec<std::cell::Cell<u8>>,
    /// The native each intrinsic name read so far stands for (`bytecode::intrinsic_name_native`),
    /// indexed by atom and recorded by its first read (`load_name_resolve_miss`); it lives as long
    /// as the atoms.
    pub intrinsic_natives: Vec<Option<Native>>,
    /// The entry that runs each native's call, by discriminant (`Vm::call_native_dispatch`).
    native_table: &'static native_table::NativeTable,
    /// The compiled chunk of each first-call stub, by its program and function, while both live.
    first_chunks: first_call_chunk::FirstChunks,
    streamed_stubs: first_call_chunk::StreamedStubs,
    dynamic_strings: Option<Box<FxHashMap<u64, Value>>>,
    /// The `typeof` result strings, one table indexed as `bytecode::TYPEOF_NAMES`, interned when the
    /// heap is made (`Vm::intern_typeof_names`). Rooted, so `typeof` neither hashes nor allocates, and a
    /// compiled body may name the table (`staged::value::typeof_string`).
    pub typeof_names: [Value; TYPEOF_NAMES],
    symbol_registry: FxHashMap<String, Value>,
    pub well_known_symbols: WellKnownSymbols,
    /// The slots of a bound function's target, receiver and arguments in the shape its
    /// environment object has (`Vm::bound_function_parts`): every `bind` gives its environment
    /// the three in one order, so they share it. Shape compaction renumbers shapes and empties it.
    bound_parts_layout: std::cell::Cell<Option<(u32, [u16; 3])>>,
    pub call_targets: lazy_table::LazyTable<InLoopCallTarget, CALL_TARGET_CACHE_SIZE>,
    construct_targets: lazy_table::LazyEntries<ConstructTarget, CALL_TARGET_CACHE_SIZE>,
    pub frameless: crate::vm::engine_types::CalleeAdmissions,
    pub field_caches: site_table::CacheSiteTable<FieldCache>,
    /// Field load sites whose data property was found beyond the immediate prototype, by
    /// global site index; cleared with the field caches.
    /// Per cache site, the index of its inherited entry (`Dependencies::inherited`), or
    /// `NO_INHERITED_FIELD`.
    inherited_field_indices: site_table::CacheSiteTable<u32>,
    /// Per cache site, the index of its newest accessor entry in `accessor_fields`, or
    /// `NO_ACCESSOR_FIELD`.
    accessor_field_indices: site_table::CacheSiteTable<u32>,
    /// Accessor reads (`AccessorFieldCache`), each site's linked newest first through
    /// `next`, up to `ACCESSOR_FIELD_CACHE_WAYS` receiver shapes a site, as an inherited
    /// method's site sees one shape per subclass; cleared with the field caches.
    accessor_fields: Vec<AccessorFieldCache>,
    pub field_add_indices: site_table::CacheSiteTable<u32>,
    field_add_sets: Vec<FieldAddCacheSet>,
    /// The prototype chains add entries name (`FieldAddCache::chain`), each once; cleared
    /// only with every entry (`field_add_sets` and `keyed_adds`).
    field_add_chains: crate::heap::InternedChains,
    /// The layouts and strings JSON parses share within a collection epoch.
    json_parse_cache: json::JsonParseCache,
    /// Adds a keyed `[[Set]]` of a string naming an atom made (`Vm::keyed_add_hit`): a keyed
    /// site's keys vary, so the entries are shared, indexed by source shape and atom
    /// (`keyed_add_index`); cleared with the field caches.
    keyed_adds: lazy_table::LazyEntries<FieldAddCache, KEYED_ADD_ENTRIES>,
    megamorphic_field_indices: site_table::CacheSiteTable<u32>,
    /// The cache sites whose per-site entries (`field_caches`, `megamorphic_field_indices`,
    /// `field_add_indices`, `inherited_field_indices`) were written since the last
    /// `invalidate_field_caches`, which then resets only those; `None` once they outnumber
    /// `FIELD_CACHE_SPARSE_RESET_SHARE` of the sites, and every site resets.
    field_cache_writes: Option<Vec<u32>>,
    megamorphic_fields: Vec<FieldCacheSet>,
    /// The megamorphic read stub cache (`field_stubs`): copies of the site entries that
    /// answered reads, by receiver shape and name, shared by every site; cleared with them.
    field_stubs: field_stubs::FieldStubs,
    /// The prototype-held reads polymorphic `GetField` records keep as their hints, each
    /// record its own, by index (`Vm::holder_hint_hit`). A hint names its prototype by record
    /// id, so a collection that sweeps that prototype drops the hint's value, and one that
    /// renumbers shapes every hint.
    holder_hints: PrototypeHints,
    /// Under reference counts: the shapes whose edges are pinned (`Heap::rc_pin`), and the
    /// heap's pin epoch they were pinned in.
    rc_pinned_shapes: usize,
    rc_pin_epoch: u64,
    pub length_atom: Atom,
    // Cached `prototype` atom; `NO_CACHED_ATOM` until first use after a program reset.
    prototype_atom: Atom,
    /// Non-dictionary constructor shapes verified to have no own `@@hasInstance` and a
    /// data `prototype` at the paired slot, direct-mapped by shape; derived from shapes, so
    /// field-cache invalidation clears them.
    instanceof_constructor_layouts: [Option<(u32, usize)>; INSTANCEOF_CONSTRUCTOR_LAYOUTS],
    /// What `key in object` answered of receiver shapes and keys (`in_presence`), direct-mapped; derived from
    /// shapes, so field-cache invalidation clears it.
    in_presences: [Option<in_presence::InPresence>; in_presence::IN_PRESENCES],
    /// Constructor prototypes (a subclass's parent constructor, `%TypedArray%`) proved to
    /// inherit the native `@@hasInstance` (`Vm::inherits_native_has_instance`), each with the
    /// cell of the chain its proof read (`Vm::chain_cell`): the proof marks every link of the
    /// chain a prototype, so a change to one, or its death, retires it. Direct-mapped by the
    /// holder's cell.
    instanceof_inheriting_holders: [Option<(Value, crate::heap::ChainCell)>; INSTANCEOF_CONSTRUCTOR_LAYOUTS],
    /// Per constructor, direct-mapped by its cell, the link of its `prototype` object while
    /// a watch on its values and layout holds, and the cell of the chain it inherits
    /// `@@hasInstance` through (`Vm::proved_instanceof`).
    instanceof_proofs: [(Value, crate::heap::ProtoLink, Option<(crate::heap::PrototypeValueWatch, crate::heap::ChainCell)>); INSTANCEOF_CONSTRUCTOR_LAYOUTS],
    /// The shapes an ordinary closure's own properties step through, without and with a
    /// `prototype` (`construction::ClosureShapes`); derived from shapes, so field-cache
    /// invalidation clears it.
    closure_shapes: [Option<construction::ClosureShapes>; 2],
    /// The shape a promise's resolving function's own `length` and `name` reach
    /// (`Vm::native_with_realm`); derived from shapes, so field-cache invalidation clears it.
    resolver_function_shape: Option<u32>,
    /// The shapes a bound function's own `length` and `name` and its environment's target,
    /// receiver and bound values reach (`Vm::bind_function`), the first one's adds recorded;
    /// derived from shapes, so field-cache invalidation clears it.
    bound_function_shape: Option<(u32, u32)>,
    /// The parts of a recorded closure that depend on its function and realm alone
    /// (`construction::ClosureTemplate`); derived from shapes, so field-cache invalidation
    /// clears it.
    closure_templates: lazy_table::LazyEntries<Option<construction::ClosureTemplate>, CLOSURE_TEMPLATES>,
    /// The header of an iterator result `{ value, done }` (`Vm::iterator_result`) and the
    /// prototype it was derived for; derived from shapes, so field-cache invalidation clears it.
    iterator_result_header: Option<(Value, crate::heap::InlineObjectHeader)>,
    /// The one `{ value, done }` result a lane iterator step reuses (`Vm::reused_iterator_result`),
    /// `undefined` until the first; cleared with `iterator_result_header`.
    reused_iterator_result: Value,
    /// The one `[key, value]` pair a for-of step over entries reuses when the step's value goes
    /// only to an array pattern (`Vm::iterator_item_value`), `undefined` until the first;
    /// cleared with `reused_iterator_result`.
    shared_entry_pair: Value,
    /// Whether the entry step running now may yield `shared_entry_pair`
    /// (`iterator_step_reads_miss`).
    share_entry_pair: bool,
    /// A spent await state no reaction, job or continuation names, which the next await
    /// arms instead of allocating one (`Vm::armed_await_state`); `undefined` when there is
    /// none. A root, so it outlives collections; cleared with the heap it belongs to.
    spare_await_state: Value,
    /// The shapes of a complete data and accessor property descriptor object
    /// (`Vm::property_descriptor_object`), indexed by `accessor`; derived from shapes, so
    /// field-cache invalidation clears them.
    descriptor_shapes: [Option<u32>; 2],
    /// The field slots of the descriptor shape `ToPropertyDescriptor` last read
    /// (`object_descriptors::DescriptorLayout`); derived from shapes.
    descriptor_layout: Option<object_descriptors::DescriptorLayout>,
    /// The header of a new RegExp object of the paired prototype: inline storage in the shape
    /// its own `lastIndex` gives an empty object, as the first one recorded
    /// (`Vm::regexp_from_meta`); derived from shapes, so field-cache invalidation clears it.
    regexp_object_header: Option<(Value, crate::heap::InlineObjectHeader)>,
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
    capture_walks: lazy_table::LazyEntries<CaptureWalk, CAPTURE_WALKS>,
    to_fixed_atom: Atom,
    to_precision_atom: Atom,
    pub runtime_atoms: RuntimeAtoms,
    /// `Vm::constructed_shape` answers; cleared by every collection.
    constructed_shapes: [provenance::ConstructedShape; provenance::CONSTRUCTED_SHAPE_ENTRIES],
    /// `Vm::preshape_receiver` answers: the constant or declared layout a construction
    /// allocates its receiver in; cleared by every collection and with the heap.
    preshaped_receivers: lazy_table::LazyEntries<preshaped_receiver::PreshapedReceiver, {preshaped_receiver::PRESHAPED_RECEIVER_ENTRIES}>,
    /// What constructions' receivers reach, per `new.target` (`Vm::preshape_receiver`'s slack
    /// tracking); kept across collections, which restart its sampling.
    learned_receivers: lazy_table::LazyEntries<preshaped_receiver::LearnedReceiver, {preshaped_receiver::LEARNED_RECEIVER_ENTRIES}>,
    /// The host's object templates (`object_template`), indexed by their ids.
    object_templates: Vec<object_template::ObjectTemplate>,
    method_caches: method_cache::MethodCacheTable,
    megamorphic_methods: Vec<MethodCacheSet>,
    pub method_cache_atoms: method_cache::MethodCacheAtoms,
    /// The inline method-cache sites that may hold an entry for each atom: a superset, pushed
    /// when a site first records the atom, so invalidating an atom visits only its sites.
    method_cache_sites: FxHashMap<Atom, Vec<u32>>,
    #[cfg(feature = "profile-aggregate")]
    invalidated_methods: FxHashMap<MethodCacheKey, InvalidatedMethod>,
    object_shapes: site_table::CacheSiteTable<u32>,
    descriptors: FxHashMap<(Value, property_key::PropertyKey), PropertyAttributes>,
    // Weak identities; captured environments remain authoritative in function cells.
    function_values: function_cache::FunctionValues,
    /// The hoisted function declarations of each module instance, by function id (`ModuleFunctions`).
    module_functions: function_cache::ModuleFunctions,
    /// The JavaScript wrapper of each exported Wasm closure, so every export of one
    /// function is one object. Weak: a wrapper nothing holds is made again on demand.
    wasm_exported_functions: FxHashMap<Value, WeakHandle>,
    /// Private-name home objects found by `private_name_home`, kept while their starting
    /// environment is live.
    private_homes: FxHashMap<PrivateHomeKey, Value>,
    /// The collation `String.prototype.localeCompare` resolves without locales or options
    /// (`Vm::locale_compare_collation`); the host's default locale fixes it.
    pub default_collation: Option<std::rc::Rc<intl_core::Collation>>,
    wasm_js: wasm_js::WasmJsIntrinsics,
    pub direct_eval: bool,
    /// The direct evals running above lane activations (`var_owner::EvalCaller`), innermost last.
    pub eval_callers: Vec<EvalCaller>,
    direct_eval_var_program: Option<ProgramId>,
    /// The Function-constructor programs this VM may still offer the host's compile cache
    /// (`DYNAMIC_FUNCTIONS_KEPT`).
    dynamic_functions_to_keep: u16,
    pub parameter_eval: bool,
    eval_script_context: bool,
    deferred_dependency_batch: bool,
    pub construct_target: Option<ConstructionTarget>,
    random_state: u64,
}

#[derive(Clone, Copy)]
struct IteratorRealmPrototypes {
    helper: Value,
    wrapper: Value,
    generator: Value,
    async_generator: Value,
}

impl Vm {
    /// Record a method-cache name, as a call site's first call does (tests).
    #[cfg(test)]
    pub(crate) fn name_method_atom(&mut self, atom: crate::bytecode::Atom) {
        self.method_cache_atoms.name(atom);
    }

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
        rows.sort_unstable_by_key(|row| std::cmp::Reverse(row.0));
        eprintln!("register-width: dispatches {all}, in functions over the root mask {wide}");
        for (total, registers, locals, program, function) in rows.iter().take(15) {
            eprintln!("  {total:>12} registers={registers} locals={locals} program={program} function={function}");
        }
    }

    pub(crate) fn execution_interrupt_handle(&self)->crate::ExecutionInterrupt { self.execution_interrupt.clone() }
    pub fn check_execution_interrupt(&self)->Result<(),JsError> {
        if self.execution_interrupt.is_termination_requested() { Err(JsError::execution_terminated()) } else { Ok(()) }
    }

    /// Retire weak callee bindings and frameless admissions at their lifetime-changing edge.
    fn clear_call_targets(&mut self) {
        self.call_targets.clear();
        self.construct_targets.clear();
        self.frameless.clear();
    }

    /// Makes `global`'s realm current, carrying each realm's global lexical
    /// scope with it; returns the previous realm's global.
    #[inline]
    pub fn switch_realm_global(&mut self, global: Value) -> Value {
        let previous = self.realm.globals;
        if previous != global {
            self.swap_realm_global(previous, global);
        }
        previous
    }

    #[cold]
    #[inline(never)]
    fn swap_realm_global(&mut self, previous: Value, global: Value) {
        self.clear_call_targets();
        // A native of the realm being left is not the next realm's own.
        self.realm_native_proofs.forget();
        // SAFETY: native lane bodies invoke no guest callbacks/realm switches.
        unsafe { self.programs.retire_native_calls_across_realms(); }
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
            promise_constructor: self.promise_constructor,
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
            Some(parked) => self.unpark_realm_fields(global, parked),
            None => {
                self.follow_realm_fields(global);
                self.realm.global_lexical_states.remove(&global).unwrap_or_default()
            }
        };
        self.realm.global_lexical_declarations = state.declarations;
        self.realm.global_lexical_bindings = state.bindings;
        self.realm.immutable_global_lexical_bindings = state.immutable_bindings;
    }

    /// Restores the realm-following fields `parked` held for `global`, which the switch makes
    /// active again; its global lexical state.
    ///
    /// Natives are shared by every realm and allocate from these fields, so the fields follow
    /// the active realm: a native called from a realm builds that realm's objects.
    fn unpark_realm_fields(&mut self, global: Value, parked: ParkedRealm) -> GlobalLexicalState {
        for (native, prototype) in REALM_FOLLOWING_PROTOTYPES.into_iter().zip(parked.prototypes) {
            debug_assert!(self
                .realm
                .intrinsics
                .builtin_prototypes
                .get(&(global, native))
                .is_none_or(|intrinsic| *intrinsic == prototype));
            *self.realm_prototype_field(native) = prototype;
        }
        debug_assert!(self
            .realm
            .intrinsics
            .promise_constructors
            .get(&global)
            .is_none_or(|intrinsic| *intrinsic == parked.promise_constructor));
        self.promise_constructor = parked.promise_constructor;
        parked.state
    }

    /// Points the realm-following fields at `global`'s intrinsics, read from the intrinsics
    /// table. A realm without a `Promise` yet leaves its promise fields null, as the table's
    /// absent entry read before; the other fields keep what they held.
    fn follow_realm_fields(&mut self, global: Value) {
        for native in REALM_FOLLOWING_PROTOTYPES {
            if let Some(&prototype) = self.realm.intrinsics.builtin_prototypes.get(&(global, native)) {
                *self.realm_prototype_field(native) = prototype;
            } else if native == Native::Promise {
                self.promise_proto = Value::NULL;
            }
        }
        self.promise_constructor =
            self.realm.intrinsics.promise_constructors.get(&global).copied().unwrap_or(Value::NULL);
    }

    /// Records `global`'s intrinsic `%Promise%` and `%Promise.prototype%`, which the
    /// intrinsics table just took, in the fields that follow it: the VM's while `global` is
    /// active, the parked realm's while it is parked; any other realm's are read from the
    /// table when it becomes active (`follow_realm_fields`).
    fn note_realm_promise(&mut self, global: Value, constructor: Value, prototype: Value) {
        if global == self.realm.globals {
            self.promise_constructor = constructor;
            self.promise_proto = prototype;
        } else if let Some(parked) = self.realm.parked.as_mut().filter(|parked| parked.global == global) {
            parked.promise_constructor = constructor;
            let index = REALM_FOLLOWING_PROTOTYPES.iter().position(|&native| native == Native::Promise);
            parked.prototypes[index.expect("Promise follows the active realm")] = prototype;
        }
    }

    /// The VM field holding the active realm's `native.prototype`.
    fn realm_prototype_field(&mut self, native: Native) -> &mut Value {
        match native {
            Native::Object => &mut self.object_proto,
            Native::Function => &mut self.function_proto,
            Native::Array => &mut self.array_proto,
            Native::String => &mut self.string_proto,
            Native::Promise => &mut self.promise_proto,
            Native::Map => &mut self.map_proto,
            Native::Set => &mut self.set_proto,
            Native::RegExp => &mut self.regexp_proto,
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
    pub(super) fn instantiate_global_declarations(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
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
        }
        let declared = root
            .global_lexical_atoms()
            .iter()
            .fold(false, |declared, &atom| self.realm.global_lexical_declarations.insert(atom) | declared);
        if declared {
            self.note_global_lexical_names();
        }
        self.realm.immutable_global_lexical_bindings.extend(
            root.global_immutable_atoms()
                .iter()
                .copied()
                .filter(|atom| root.global_lexical_atoms().contains(atom)),
        );
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
        // A script's lexical bindings exist from instantiation, in their temporal dead zone until the
        // declaration initializes them (`StoreGlobalLexical` with `initialize`). They are created once
        // the var conflict check passes, so a script refused for a conflicting name creates none.
        if program.kind == crate::bytecode::ProgramKind::Script {
            for atom in root.global_lexical_atoms().iter().copied() {
                self.bind_global_lexical(atom, Value::DELETED);
            }
        }
        for atom in root.global_function_atoms().iter().copied() {
            let key = PropertyKey::string(atom);
            let Some(attributes) = self.property_attributes(self.realm.globals, key) else {
                if self
                    .heap_reads().object(self.realm.globals)
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
                .heap_reads().object(self.realm.globals)
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
        self.map_global_lexical_slots(program);
        Ok(())
    }
    /// A global lexical name was declared: it may shadow a global property that a global name
    /// read's hint names (`dispatch_fast::name_row`), so every such hint is
    /// dropped (`LaneView::clear_name_hints`), and code holding one retires (`note_global_shadowing`).
    pub(super) fn note_global_lexical_names(&self) {
        self.programs.clear_global_name_hints();
        self.note_global_shadowing();
    }

    /// Whether a global name read of `atom` may keep the global object's own data property as
    /// its hint: no realm, active or not, declares a global lexical binding of it, so a realm
    /// switch keeps every hint. A later declaration drops them (`note_global_lexical_names`).
    pub fn global_name_hintable(&self, atom: Atom) -> bool {
        !self.realm.shadows_global_name(atom)
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

    #[quench_macros::test_visible]
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
        self.resume_restored()?;
        self.run_main(program, false)
    }

    /// Resumes the host of a VM restored from a run saved at rest (`snapshot_at_rest`), whose
    /// main program has already run: the run continues from the host's own sources.
    pub(crate) fn resume_restored(&mut self) -> Result<(), JsError> {
        self.check_execution_interrupt()?;
        self.resume_host()
    }

    /// Runs the main program of an installed realm: its module graph, its declarations and
    /// its body, then optionally its jobs.
    pub fn run_main(&mut self, program: &ResidualProgram, drain_jobs: bool) -> Result<Value, JsError> {
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
        let result = self.call_root(program, root, this);
        self.finish_main_module(program, &result)?;
        if drain_jobs {
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
        self.report_bytecode();
        self.report_lane_plan();
        #[cfg(feature = "profile-field-primitives")]
        self.profile.field_primitives.report();
        #[cfg(feature = "profile-memory")]
        self.name_atom_memo.report_queries();
        #[cfg(feature = "profile-allocator")]
        self.report_allocator_if_enabled("complete");
        #[cfg(feature = "profile-gc-time")]
        self.report_gc_time_if_enabled();
        #[cfg(feature = "profile-aggregate")]
        if crate::trace::Channel::AllPrograms.on() {
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
            if crate::trace::Channel::Census.value() == Some("all") {
                self.report_dispatch_by_register_width();
            }
            self.profile
                .report_object_literal_sites(ProgramId::MAIN.raw(), program);
            #[cfg(feature = "profile-memory")]
            self.report_memory_if_enabled("complete");
            return;
        }
        self.profile.report(&self.heap, ProgramId::MAIN.raw(), program);
        #[cfg(feature = "profile-memory")]
        self.report_memory_if_enabled("complete");
    }
    #[cfg(feature = "profile-allocator")]
    fn report_allocator_if_enabled(&self, phase: &str) {
        if crate::trace::Channel::Allocator.on() {
            crate::report_allocator_memory(phase);
        }
    }
    #[cfg(feature = "profile-gc-time")]
    fn report_gc_time_if_enabled(&self) {
        if !crate::trace::Channel::GcTime.on() {
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
        if crate::trace::Channel::Memory.on() {
            self.report_memory(phase);
        }
    }
    /// `QUENCH_TRACE=function-use`: how many compiled functions have been called (their call plan is
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
            + self.field_add_sets.capacity() * size_of::<FieldAddCacheSet>()
            + self.field_add_chains.bytes();
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
            self.method_caches.capacity_bytes(),
            self.megamorphic_methods.len(),
        );
        self.heap
            .memory_profile()
            .report(phase, cell_counts, live_payload_bytes, &self.heap);
        self.report_memory_snapshot(phase);
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn report_memory_snapshot(&self, phase: &str) {
        self.heap.report_record_pages(phase);
        let heap = self.heap.memory_composition();
        let page_size = switches::memory_page_size();
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
            + self.method_caches.capacity_bytes();
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
        let v128_record_reserved_bytes = heap.v128_record_reserved_bytes;
        let v128_record_used_bytes = heap.v128_record_used_bytes;
        eprintln!(
            "{{\"kind\":\"quench-environment-storage\",\"phase\":\"{phase}\",\"collection\":{},\"logical_slots\":{},\"storage_bytes\":{},\"sparse_count\":{}}}",
            self.heap.collection_count(),
            heap.environment_logical_slots,
            heap.environment_storage_bytes,
            heap.environment_sparse_count,
        );
        let accounted_runtime_bytes = heap.slot_arena_reserved_bytes
            + heap.object_record_reserved_bytes
            + heap.array_record_reserved_bytes
            + heap.function_record_reserved_bytes
            + heap.promise_record_reserved_bytes
            + heap.await_record_reserved_bytes
            + heap.iterator_record_reserved_bytes
            + heap.string_record_reserved_bytes
            + v128_record_reserved_bytes
            + heap.legacy_cell_record_reserved_bytes
            + heap.free_slot_metadata_bytes
            + property_arena_bytes
            + heap.cell_payload_bytes
            + heap.array_buffer_store_bytes
            + heap.boxed_object_header_bytes
            + heap.sparse_array_sidecar_bytes
            + vm_metadata_bytes;
        eprintln!(
            "{{\"kind\":\"quench-memory-composition\",\"phase\":\"{phase}\",\"collection\":{},\"cell_kind_names\":{:?},\"cell_counts\":{:?},\"cell_variant_names\":{:?},\"cell_variant_counts\":{:?},\"occupied_slots\":{},\"complete_slot_slabs\":{},\"fully_free_slot_slabs\":{},\"partial_slot_slabs\":{},\"fully_empty_slot_pages\":{fully_empty_slot_pages},\"slot_arena_reserved_bytes\":{},\"object_record_reserved_bytes\":{},\"object_record_reusable_ids\":{},\"object_record_allocation_scan_cursor\":{},\"array_record_reserved_bytes\":{},\"array_record_reusable_ids\":{},\"array_record_allocation_scan_cursor\":{},\"function_record_reserved_bytes\":{},\"promise_record_reserved_bytes\":{},\"await_record_reserved_bytes\":{},\"iterator_record_reserved_bytes\":{},\"string_record_reserved_bytes\":{},\"v128_record_reserved_bytes\":{v128_record_reserved_bytes},\"v128_record_used_bytes\":{v128_record_used_bytes},\"legacy_cell_record_reserved_bytes\":{},\"free_slot_metadata_bytes\":{},\"object_headers\":{},\"embedded_object_headers\":{},\"boxed_object_headers\":{},\"boxed_object_header_bytes\":{},\"object_property_counts_by_width\":{:?},\"inline_property_attribution_bytes_overlapping_slots\":{},\"object_header_size_bytes\":{},\"cell_size_bytes\":{},\"slot_size_bytes\":{},\"property_arena_values_bytes\":{},\"property_arena_metadata_bytes\":{},\"property_arena_used_values\":{},\"property_arena_capacity_values\":{},\"live_property_used_values\":{},\"live_property_capacity_values\":{},\"live_property_used_values_by_width\":{:?},\"live_property_capacity_values_by_width\":{:?},\"dense_array_elements_bytes\":{},\"dense_array_headers_bytes\":{},\"dense_array_backing_count\":{},\"sparse_array_sidecar_bytes\":{},\"string_buffers_bytes\":{},\"string_buffer_count\":{},\"other_cell_payload_bytes\":{},\"cell_payload_total_bytes\":{},\"shape_table_bytes\":{},\"shape_count\":{},\"dictionary_shape_count\":{},\"shape_lookup_bytes\":{},\"transition_table_bytes\":{},\"descriptor_side_table_bytes\":{},\"cache_side_table_bytes\":{},\"atom_side_table_bytes\":{},\"frame_bytes\":{},\"profile_instrumentation_bytes\":{},\"accounted_runtime_bytes\":{},\"accounted_with_profile_bytes\":{}}}",
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
            self.shapes.iter().filter(|shape| shape.is_dictionary()).count(),
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
    /// The host as its own type (the VM holds it erased).
    #[allow(dead_code)]
    pub(crate) fn host_as<T: Host>(&mut self) -> &mut T {
        crate::host::host_as::<T>(&mut *self.host)
    }

    #[allow(dead_code)]
    pub(crate) fn host_ref<T: Host>(&self) -> &T {
        crate::host::host_ref_as::<T>(&*self.host)
    }

    /// `Runtime::end_for_process_exit`: drops the host, and the program store when its drop
    /// reports, and leaves every other field to the operating system.
    /// # Safety
    /// The VM is never used or dropped again, so each field read here is dropped exactly once
    /// and the rest are never dropped.
    pub(crate) unsafe fn end_for_process_exit(&mut self) {
        let vm = self;
        // SAFETY: per the contract above.
        unsafe {
            if vm.programs.js_jit_diagnostics_enabled() {
                eprintln!("{}", vm.field_site_report());
            }
            crate::fact_stats::report();
            vm.report_byte_census();
            if vm.programs.reports_on_drop() {
                drop(std::ptr::read(&vm.programs));
            }
            drop(std::ptr::read(&vm.host));
        }
    }
    fn initialize(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.initialize_shared(&Rc::new(program.clone()), builtins::RealmScope::Complete)
    }
    pub fn initialize_shared(
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
        // Reset reuses environment locators and collection counters. Retire raw
        // capture views at this boundary before either identity can appear again.
        self.capture_walks.clear();
        #[cfg(feature = "profile-lane-exits")]
        eprintln!("capture-reset {:p}", self.capture_walks.as_ptr());
        // SAFETY: initialization runs outside any native body, before reuse.
        unsafe { self.programs.retire_native_calls(); }
        self.heap.reset();
        self.forget_learned_receivers();
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
        self.set_construct_target(None);
        self.realm.promise = Default::default();
        self.shapes.truncate(1);
        self.shape_constructors.clear();
        // The kept root forgets its transitions along with the index.
        self.shapes[0].last_child.set(NO_LAST_CHILD);
        self.shapes[0].facts = self.shapes[0].facts.with_children(0);
        self.transitions.clear();
        self.shape_slot_memo.clear();
        self.shape_attribute_memo.clear();
        self.plain_copy_shapes.clear();
        self.plain_assign_shapes.clear();
        self.adopted_shapes.clear();
        self.embedding_record_shapes.clear();
        self.function_name_shapes.clear();
        self.symbol_write_proofs.clear();
        self.symbol_load_proofs.clear();
        self.private_site_homes.clear();
        self.array_iteration_shape = None;
        self.collection_iteration_shape = None;
        self.array_species_proof = None;
        self.concat_spreadable_proof = None;
        self.for_in.forget_snapshot();
        self.species_slot_proof.set(None);
        self.iterator_absence_proof.set(None);
        self.then_absence_proof = None;
        self.promise_species_proof.take();
        self.promise_then_memo.take();
        self.heap.dependencies_mut().retire_all();
        self.enumerable_keys.clear(); self.enumerable_key_lists.clear(); self.enumerable_key_value_lists.clear(); self.heap.clear_string_atoms(); self.concat_memo.clear();
        // The sealed holders and their shapes belong to the discarded heap.
        self.startup_intrinsics = Default::default();
        self.atom_text = program.atoms.clone();
        self.atom_sequence = Rc::new(crate::bytecode::AtomSequence::new(self.atom_text.clone()));
        self.atoms.clear();
        self.json_parse_cache = Default::default(); self.json_plain = Default::default();
        self.name_atom_memo.clear();
        self.array_index_atoms.clear();
        self.atom_classes.clear();
        self.intrinsic_natives.clear();
        self.regexp_subjects.clear();
        self.regexp_matchers.clear();
        self.atom_classes.resize_with(self.atom_text.len(), Default::default);
        self.dynamic_strings = None;
        self.intern_typeof_names();
        self.symbol_registry.clear();
        self.well_known_symbols.clear();
        self.clear_call_targets();
        self.lane_stack.clear_script_targets();
        let atom_text = self.atom_text.clone();
        for (id, name) in atom_text.iter().enumerate() {
            self.index_atom(Self::atom_hash_str(name), id as Atom, name);
        }
        self.runtime_atoms = self.intern_runtime_atoms();
        self.field_caches = site_table::CacheSiteTable::with_empty(usize::from(program.cache_sites.capacity()));
        self.field_cache_writes = Some(Vec::new());
        program.prime_loop_decoding();
        self.heap.dependencies_mut().clear_inherited();
        self.inherited_field_indices = site_table::CacheSiteTable::with_empty(usize::from(program.cache_sites.capacity()));
        self.accessor_field_indices = site_table::CacheSiteTable::with_empty(usize::from(program.cache_sites.capacity()));
        self.accessor_fields.clear();
        self.field_add_indices = site_table::CacheSiteTable::with_empty(usize::from(program.cache_sites.capacity()));
        self.field_add_sets.clear();
        self.keyed_adds.clear();
        self.field_add_chains.clear();
        self.megamorphic_field_indices = site_table::CacheSiteTable::with_empty(usize::from(program.cache_sites.capacity()));
        self.megamorphic_fields.clear();
        self.field_stubs.clear();
        self.holder_hints.clear();
        self.length_atom = self.intern_atom("length");
        self.prototype_atom = self.intern_atom("prototype");
        self.to_fixed_atom = self.intern_atom("toFixed");
        self.to_precision_atom = self.intern_atom("toPrecision");
        self.method_caches = method_cache::MethodCacheTable::with_empty(program.method_sites.capacity());
        self.megamorphic_methods.clear();
        self.method_cache_atoms.clear();
        self.method_cache_sites.clear();
        self.object_shapes = site_table::CacheSiteTable::with_empty(program.object_sites.capacity());
        self.descriptors.clear();
        self.function_values.clear();
        self.module_functions.clear();
        self.wasm_exported_functions.clear();
        self.private_homes.clear();
        // The heap reset discards every class closure, and so every private home.
        self.private_classes.clear();
        self.programs.reset(program.clone());
        self.instanceof_constructor_layouts = [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS];
        self.instanceof_inheriting_holders = [None; INSTANCEOF_CONSTRUCTOR_LAYOUTS];
        self.instanceof_proofs = std::array::from_fn(|_| (Value::UNDEFINED, crate::heap::ProtoLink::NULL, None));
        self.in_presences = [None; in_presence::IN_PRESENCES];
        self.closure_shapes = [None; 2];
        self.resolver_function_shape = None;
        self.function_name_shapes.clear();
        self.closure_templates.clear();
        self.iterator_result_header = None;
        self.reused_iterator_result = Value::UNDEFINED;
        self.shared_entry_pair = Value::UNDEFINED;
        self.spare_await_state = Value::UNDEFINED;
        self.descriptor_shapes = [None; 2];
        self.descriptor_layout = None;
        self.regexp_object_header = None;
        self.regexp_last_index_layouts.clear();
        self.regexp_builtin_layouts.clear();
        self.regexp_symbol_layouts.clear();
        self.set_instanceof_has_instance_holder(Value::NULL);
        self.program_cache_layouts.clear();
        self.free_cache_layouts.clear();
        self.programs.bump_context_epoch();
        self.program_cache_layouts.push(ProgramCacheLayout {
            shared_uncached_field: ProgramCacheLayout::shared_uncached_field(0, program.cache_sites.capacity()),
            ..ProgramCacheLayout::default()
        });
        // The new table moved every pointer into the old one; the layouts now locate its sites.
        self.active_program = ProgramId::MAIN;
        self.field_caches_moved();
        self.typed_array_proto = Value::NULL;
        self.finalization_registry_proto = Value::NULL;
        self.temporal_plain_date_proto = Value::NULL;
        self.temporal_plain_time_proto = Value::NULL;
        self.temporal_plain_month_day_proto = Value::NULL;
        self.temporal_plain_year_month_proto = Value::NULL;
        self.random_state = fresh_random_seed();
        self.realm_native_proofs.forget();
        self.realm.globals = self
            .heap
            .alloc_object(Self::empty_object(Value::NULL));
        if self.main_realm.is_null() {
            self.main_realm = self.realm.globals;
        }
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

    /// The atoms the runtime reads by name, interned in the reset realm.
    fn intern_runtime_atoms(&mut self) -> RuntimeAtoms {
        let lexical_this = self.intern_atom(LEXICAL_THIS_BINDING);
        let new_target = self.intern_atom(NEW_TARGET_BINDING);
        let regexp_flag_accessors =
            std::array::from_fn(|index| self.intern_atom(regexp::REGEXP_FLAG_ACCESSORS[index].0));
        RuntimeAtoms {
            lexical_this,
            new_target,
            regexp_exec: self.intern_atom("exec"),
            regexp_flags: self.intern_atom("flags"),
            last_index: self.intern_atom("lastIndex"),
            regexp_flag_accessors,
            then: self.intern_atom("then"),
            constructor: self.intern_atom("constructor"),
            value_of: self.intern_atom("valueOf"),
            to_string: self.intern_atom("toString"),
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
            proxy_traps: proxy::PROXY_TRAP_NAMES.map(|name| self.intern_atom(name)),
        }
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
            Constant::BigInt(v) => self.heap.alloc(Cell::bigint((**v).clone())),
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
        self.programs.set_constants(id, constants, program.constants.capacity() - program.constants.len());
        // The dropped views' record addresses name private-name sites' entries.
        self.private_site_homes.clear();
        // Lane views carry constants, so call targets naming the old views are stale.
        self.lane_stack.clear_script_targets();
        if id == self.active_program {
            self.set_active_program(id);
        }
    }
    pub(super) fn store_module_program(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let program = self.intern_program_atoms(program);
        self.store_module_program_shared(Rc::new(program))
    }

    pub fn store_module_program_shared(&mut self, program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = self.programs.insert_module_shared(program)?;
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
        crate::vm::engine_types::AheadImages::record(&program);
        let footprint = crate::compile::program_profile::ProgramFootprint::of(&program).total(); self.heap.charge_reclaimable_program(footprint);
        let id = self.programs.insert_reclaimable_shared(program, footprint)?;
        let residual = self.programs.get(id)?;
        self.append_program_cache_layout(id, &residual);
        self.materialize_program_constants(id, &residual);
        // Default's first bodies are the program's AOT units, built now (`stage_images_at_load`).
        crate::vm::engine_calls::EngineVm::stage_images_at_load(self, id, &residual);
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
        self.programs.bump_context_epoch();
        if let Some(layout) = self.free_cache_layouts.get_mut(&sites).and_then(Vec::pop) {
            self.empty_cache_ranges(layout);
            self.program_cache_layouts.push(layout);
            return;
        }
        self.program_cache_layouts.push(ProgramCacheLayout {
            sites,
            shared_uncached_field: ProgramCacheLayout::shared_uncached_field(self.field_caches.len(), sites.fields),
            field_base: self.field_caches.len(),
            method_base: self.method_caches.len(),
            object_base: self.object_shapes.len(),
        });
        let fields = usize::from(sites.fields);
        self.field_caches.extend_empty(fields);
        self.field_caches_moved();
        self.megamorphic_field_indices.extend_empty(fields);
        self.field_add_indices.extend_empty(fields);
        self.inherited_field_indices.extend_empty(fields);
        self.accessor_field_indices.extend_empty(fields);
        self.method_caches.extend_empty(sites.methods);
        self.object_shapes.extend_empty(sites.objects);
    }

    /// Empties every cache entry in `layout`'s ranges, which a reclaimed program left, for
    /// the program taking them over.
    ///
    /// Every table's empty entry stores as zero bits, so a range empties by handing its whole
    /// pages back to the kernel (`SiteTable::empty_range`): both when its program is reclaimed
    /// (`free_cache_layouts_of`), so the dead program's recorded pages leave the resident set
    /// at once, and again on reuse, where only the sites recorded since are written.
    fn empty_cache_ranges(&mut self, layout: ProgramCacheLayout) {
        let fields = layout.field_base..layout.field_base + usize::from(layout.sites.fields);
        self.field_caches.empty_range(fields.clone());
        self.field_add_indices.empty_range(fields.clone());
        self.megamorphic_field_indices.empty_range(fields.clone());
        self.inherited_field_indices.empty_range(fields.clone());
        self.accessor_field_indices.empty_range(fields);
        self.method_caches.empty_sites(layout.method_base..layout.method_base + layout.sites.methods);
        self.object_shapes.empty_range(layout.object_base..layout.object_base + layout.sites.objects);
    }

    /// Files the cache-site ranges of reclaimed `programs` for programs with the same site
    /// counts to take over (`free_cache_layouts`).
    pub(super) fn free_cache_layouts_of(&mut self, programs: impl IntoIterator<Item = ProgramId>) {
        for program in programs {
            let Some(&layout) = self.program_cache_layouts.get(program.raw() as usize) else { continue };
            if layout.sites != crate::bytecode::CacheSiteCounts::default() {
                self.empty_cache_ranges(layout);
                self.free_cache_layouts.entry(layout.sites).or_default().push(layout);
            }
        }
    }
    /// Makes `program` the executing program and caches its layout, the projection every cache
    /// index reads. A program's layout is appended before the program can run.
    #[inline(always)]
    pub fn set_active_program(&mut self, program: ProgramId) {
        let context = self.program_context(program);
        self.activate_context(program, context);
    }

    /// `program`'s context (`ProgramContext`), derived again where something it comes from
    /// changed since.
    #[inline(always)]
    pub(super) fn program_context(&mut self, program: ProgramId) -> *const ProgramContext {
        let epoch = self.programs.context_epoch();
        match self.program_contexts.get(program.raw() as usize) {
            Some(context) if context.epoch == epoch => &**context,
            _ => self.derive_program_context(program, epoch),
        }
    }

    /// `program_context` past its context's epoch, out of line.
    #[cold]
    #[inline(never)]
    fn derive_program_context(&mut self, program: ProgramId, epoch: u64) -> *const ProgramContext {
        let index = program.raw() as usize;
        while self.program_contexts.len() <= index {
            self.program_contexts.push(Box::new(ProgramContext::idle()));
        }
        let (code, views, constants) = match self.programs.residual(program) {
            Some(code) => (
                std::ptr::from_ref(code),
                self.programs.made_lane_views(program).map_or(std::ptr::slice_from_raw_parts(std::ptr::null(), 0), std::ptr::from_ref),
                std::ptr::from_ref(self.programs.constants(program)),
            ),
            None => (std::ptr::null(), std::ptr::slice_from_raw_parts(std::ptr::null(), 0), &[][..] as *const [Value]),
        };
        let layout = self.program_cache_layouts.get(index).copied().unwrap_or_default();
        let field_entries = self.field_caches.entries_from(layout.field_base);
        let context = &mut self.program_contexts[index];
        **context = ProgramContext { epoch, code, views, constants, layout, field_entries };
        &**context
    }

    /// `program_context` of a program the lane runs, whose view table it makes if the
    /// program has none yet. The program is one the store holds.
    #[inline(always)]
    pub fn program_context_in_lane(&mut self, program: ProgramId) -> *const ProgramContext {
        let context = self.program_context(program);
        // SAFETY: a context stays in its box while the VM lives.
        if unsafe { (*context).views.is_null() } {
            return self.make_context_views(program);
        }
        context
    }

    /// `program_context_in_lane` of a program without its view table yet, out of line.
    #[cold]
    #[inline(never)]
    fn make_context_views(&mut self, program: ProgramId) -> *const ProgramContext {
        let Some((_, views, _)) = self.programs.program_in_lane(program) else {
            unreachable!("a lane target names a program the store holds");
        };
        let views = std::ptr::from_ref(views);
        let context = &mut self.program_contexts[program.raw() as usize];
        context.views = views;
        &**context
    }

    /// Sets the construction whose callee's frame is pending (`construct_target`), which
    /// refuses cached lane calls while it is (`refresh_call_refusal`).
    #[inline]
    pub fn set_construct_target(&mut self, target: Option<ConstructionTarget>) {
        self.construct_target = target;
        self.refresh_call_refusal();
    }

    /// Whether cached lane calls are refused (`crate::vm::engine_types::LaneStack::set_refused`): under a
    /// pending construction, whose calls the lane does not make from a cache.
    #[inline]
    pub(super) fn refresh_call_refusal(&mut self) {
        let refused = self.construct_target.is_some();
        self.lane_stack.set_refused(refused);
    }

    /// `set_active_program` with `program`'s context (`program_context`) in hand.
    #[inline(always)]
    pub fn activate_context(&mut self, program: ProgramId, context: *const ProgramContext) {
        self.active_program = program;
        self.lane_stack.enter_program(program);
        self.active_context = context;
        // SAFETY: a context stays in its box while the VM lives.
        self.active_field_entries = unsafe { (*context).field_entries };
    }

    /// Re-derive `active_field_entries` after the active program or the field cache table
    /// changed.
    /// The field cache entries of `program`'s sites, for generated code that reads them
    /// (`NATIVE_FIELD_CACHE_LAYOUT`); valid until the cache table grows or is replaced.
    pub fn program_field_entries(&self, program: ProgramId) -> Option<usize> {
        let layout = self.program_cache_layouts.get(program.raw() as usize)?;
        Some(self.field_caches.entries_from(layout.field_base) as usize)
    }

    pub(super) fn refresh_active_field_entries(&mut self) {
        // Every context's entries point into the table that moved.
        self.programs.bump_context_epoch();
        self.set_active_program(self.active_program);
    }

    /// The field cache table grew or was replaced: the only events that move its mapping, and so
    /// every pointer into it. The ones the VM keeps re-derive here, once, so no reader checks
    /// them: the active program's (`active_field_entries`), the lane frame's payload word
    /// generated code reads (`rebind_lane_field_entries`), and the epoch a run that kept one
    /// across a program install compares (`run_nested_lane_method`).
    fn field_caches_moved(&mut self) {
        self.cache_epochs.bump_field_entries();
        self.refresh_active_field_entries();
        self.rebind_lane_field_entries();
    }

    /// Constant `index` of the active program.
    #[inline(always)]
    pub fn active_constant(&self, index: usize) -> Option<Value> {
        // SAFETY: the active context is derived again whenever the active program or its
        // constants change, and an installed constant buffer stays put until the store is
        // reset, which reactivates the main program.
        let constants = unsafe { (*self.active_context).constants };
        debug_assert!(std::ptr::eq(constants, self.programs.constants(self.active_program)));
        unsafe { (&*constants).get(index).copied() }
    }

    /// Makes `program` the executing program and returns the one it replaces, which
    /// `restore_active_program` makes executing again. The active program's projections hold
    /// while it stays active (`active_constant`, `active_cache_layout`), so a call within the
    /// running program re-derives nothing.
    #[inline(always)]
    pub fn replace_active_program(&mut self, program: ProgramId) -> ProgramId {
        let previous = self.active_program;
        self.restore_active_program(program);
        previous
    }

    /// `set_active_program` of `previous`, the program `replace_active_program` replaced,
    /// where it is not still the executing one.
    #[inline(always)]
    pub(super) fn restore_active_program(&mut self, previous: ProgramId) {
        if self.active_program != previous {
            self.set_active_program(previous);
        }
    }

    #[inline(always)]
    fn active_cache_layout(&self) -> ProgramCacheLayout {
        // SAFETY: a context stays in its box while the VM lives.
        let layout = unsafe { (*self.active_context).layout };
        // Before any program the default context is active, with the default layout.
        debug_assert_eq!(layout, self.program_cache_layouts.get(self.active_program.raw() as usize).copied().unwrap_or_default());
        layout
    }
    #[inline(always)]
    pub fn field_cache_index(&self, site: u16) -> usize {
        self.active_cache_layout().field_base + usize::from(site)
    }
    /// Whether the active program may record cache entries at `index`: every index but its
    /// shared overflow site, if it has one.
    pub fn field_cache_records(&self, index: usize) -> bool {
        self.active_cache_layout().shared_uncached_field != Some(index)
    }
    pub(super) fn method_cache_index(&self, site: usize) -> usize {
        self.active_cache_layout().method_base + site
    }
    #[inline]
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
    pub fn call_value(
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

    /// Runs `body`, a native's loop of calls of `callee` with `this` (array iteration, a sort's
    /// comparisons), with a callback prepared for them (`Vm::call_prepared`): what holds for
    /// every call of the loop is proved once (`CallbackProof`), as JSC's `CachedCall` does.
    /// The native keeps the callee and receiver reachable, as its arguments.
    pub(super) fn with_prepared_callback<R>(
        &mut self,
        callee: Value,
        this: Value,
        body: impl FnOnce(&mut Self, &mut PreparedCallback) -> R,
    ) -> R {
        let mut prepared = PreparedCallback { callee, this, proof: CallbackProof::First };
        let result = body(self, &mut prepared);
        if let CallbackProof::Lane { roots, lane } = prepared.proof {
            self.finish_lane_callback(lane);
            self.prepared_roots.truncate(roots);
        }
        result
    }

    /// `call_value` of a prepared callback with `args`: in the lane that called the native
    /// where the loop's proof holds (`Vm::call_prepared_lane`), else the general call. The
    /// arguments stay rooted for the call, as `call_value` roots them, in the proof's slots,
    /// which hold while they are the innermost call roots.
    pub(super) fn call_prepared(
        &mut self,
        p: &ResidualProgram,
        prepared: &mut PreparedCallback,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match &mut prepared.proof {
            CallbackProof::Lane { roots, lane } => {
                let arguments = *roots + CALL_ROOTS_BESIDE_ARGUMENTS;
                // The proof's slots root the arguments while it is the innermost proof, whatever
                // call roots the native pushed since (`prepared_roots`).
                if args.len() <= PREPARED_ARGUMENT_ROOTS
                    && self.prepared_roots.len() == arguments + PREPARED_ARGUMENT_ROOTS
                {
                    let slots = &mut self.prepared_roots[arguments..arguments + PREPARED_ARGUMENT_ROOTS];
                    for (index, slot) in slots.iter_mut().enumerate() {
                        *slot = args.get(index).copied().unwrap_or(Value::UNDEFINED);
                    }
                    if let Some(result) = self.call_prepared_lane(lane, args) {
                        return result;
                    }
                }
            }
            CallbackProof::First => prepared.proof = CallbackProof::Second,
            CallbackProof::Second => {
                // Preparing can resolve the callee's target, which can allocate: the arguments
                // stay rooted across it, as `call_value` roots them.
                let start = self.active_call_roots.len();
                self.active_call_roots.extend_from_slice(args);
                let lane = self.prepare_lane_callback(prepared.callee, prepared.this);
                self.active_call_roots.truncate(start);
                prepared.proof = match lane {
                    Some(lane) => {
                        let roots = self.prepared_roots.len();
                        self.prepared_roots.extend_from_slice(&[prepared.callee, prepared.this]);
                        self.prepared_roots.extend_from_slice(&[Value::UNDEFINED; PREPARED_ARGUMENT_ROOTS]);
                        CallbackProof::Lane { roots, lane }
                    }
                    None => CallbackProof::General,
                };
                if matches!(prepared.proof, CallbackProof::Lane { .. }) {
                    return self.call_prepared(p, prepared, args);
                }
            }
            CallbackProof::General => {}
        }
        // A native a lane called runs its user callee in that lane where the lane's own call of
        // it would, as `call_value_with_target_inner` runs one, without its target resolution
        // first; the arguments stay rooted as `call_value` roots them.
        if self.lane_native.is_some()
            && matches!(self.heap.function(prepared.callee), Some(FunctionRecord { kind: FunctionKind::User(..), .. }))
        {
            let start = self.active_call_roots.len();
            self.active_call_roots.extend_from_slice(&[prepared.callee, prepared.this]);
            self.active_call_roots.extend_from_slice(args);
            let result = self.native_callback_in_lane(prepared.callee, prepared.this, args);
            self.active_call_roots.truncate(start);
            if let Some(result) = result {
                return result;
            }
        }
        self.call_value(p, prepared.callee, prepared.this, args)
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
                && let Some(result) = self.deferred_callback_in_lane(callee, this, args)
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
                // Every caller holds `args`: `call_value_with_target` roots them, and the
                // `_from_frame` calls pass the registers of a frame parked at the call, which
                // the collector scans at the call's liveness, or values their caller roots.
                self.call_native_guarded_arguments_held(p, native, this, args, callee)
            }
            CallTarget::User(program_id, id, env) => {
                // A native a lane called runs its callee in that lane where the lane's own call
                // of it would.
                if self.lane_native.is_some()
                    && let Some(result) = self.native_callback_in_lane(callee, this, args)
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
                    .map_or(self.realm.globals, |function| self.record_realm(function));
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
                self.restore_active_program(active_program);
                self.switch_realm_global(current_global);
                result
            }
        }
    }

    /// Counts collections; see `Heap::collection_epoch`.
    pub(crate) fn collection_epoch(&self) -> u64 {
        self.heap.collection_epoch()
    }

    pub fn with_call_roots<R>(
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
            Some(record) => match record.kind {
                FunctionKind::User(program_id, id) => (program_id, id, record.env, self.record_realm(record)),
                FunctionKind::Native(_) => return Err(self.type_error(p, "constructor is not a user function".into())),
            },
            None => return Err(self.type_error(p, "constructor is not a user function".into())),
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

/// The build an image belongs to, which `native_link` checks a runtime library against.
pub(crate) use engine_types::AHEAD_BUILD_ID_PREFIX;
