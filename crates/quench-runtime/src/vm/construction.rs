use super::*;
#[path = "array_construction.rs"]
mod array_construction;
#[cfg(test)]
#[path = "derived_construct_tests.rs"]
mod derived_construct_tests;
mod closure_scope;
mod shortcut;

/// The shapes an ordinary closure's own properties step through as `Vm::closure_in_realm`
/// defines them, each with its final attributes: `length`, then `name`, then for a
/// constructor `prototype`, paired with its prototype object's shape once `constructor` is
/// defined. Recorded from the first such closure built property by property; a later one
/// is allocated with its values under them (`Vm::finished_closure_object`).
#[derive(Clone, Copy)]
pub(super) struct ClosureShapes {
    length: u32,
    name: u32,
    prototype: Option<(u32, u32)>,
}

/// A `super(...)` pushed in place (`Vm::inline_super_construct_target`): the parent's program,
/// function and environment, the `new.target` it constructs with, the `this` its frame starts
/// with (the receiver made from `new.target` for a base parent, `undefined` for a derived one,
/// which binds its own), and the completion of its pending call.
pub(super) struct SuperConstruction {
    pub target: (ProgramId, u32, Value),
    pub new_target: Value,
    pub this: Value,
    pub completion: CallCompletion,
}

/// What a recorded ordinary closure of one nested function in one realm is made of, apart
/// from its environment (`Vm::closure_in_realm`): its own properties' values under their
/// named shape, its `[[Prototype]]`, its kind, and for a constructor its prototype object's
/// parent and the shape `constructor` gives it.
#[derive(Clone, Copy)]
pub(super) struct ClosureTemplate {
    program: ProgramId,
    function: u32,
    realm: Value,
    function_prototype: Value,
    /// The closure object's headers (`InlineObjectHeader`) under its named shape: inline, for
    /// own properties that fit its inline slots (`finished_closure_fits`), else over a run.
    header: crate::heap::InlineObjectHeader,
    run_header: crate::heap::InlineObjectHeader,
    length: Value,
    name: Value,
    kind: FunctionKind,
    /// The prototype object's header: its parent, under the shape `constructor` gives it.
    prototype: Option<crate::heap::InlineObjectHeader>,
    /// What the closures read of the activation that makes them (`MakerReads`): an
    /// environment activation makes those reading nothing of its own environment over
    /// another (`Vm::maker_free_closure`).
    maker_reads: crate::bytecode::MakerReads,
    /// Whether the function's closures are call-only where the lane makes them
    /// (`Value::known_closure`), which no template makes.
    call_only: bool,
    /// The environment of no slots that `MakerReads::Outer` closures of one maker share: the
    /// maker's parent and it. Cleared at each collection (`forget_outer_environments`).
    outer_environment: Option<(Value, Value)>,
}


/// The closure template slot of a program's function: functions of different programs share
/// ids, so the program is mixed in by a multiplicative hash (the 32-bit golden ratio).
#[inline]
fn closure_template_index(program: ProgramId, function: u32) -> usize {
    const PROGRAM_MIX: u32 = 0x9E37_79B9;
    (function ^ program.raw().wrapping_mul(PROGRAM_MIX)) as usize & (CLOSURE_TEMPLATES - 1)
}

/// A closure's own `length` and `name`, which the recorded shapes add first; its `prototype`,
/// when it has one, follows them.
const FINISHED_CLOSURE_NAMED_SLOTS: usize = 2;

/// A closure's own properties: `length` and `name`, then its `prototype` when it has one.
const FINISHED_CLOSURE_SLOTS: usize = FINISHED_CLOSURE_NAMED_SLOTS + 1;

/// Whether a closure's own properties fit its inline slots.
#[inline]
fn finished_closure_fits(prototype: Option<Value>) -> bool {
    FINISHED_CLOSURE_NAMED_SLOTS + usize::from(prototype.is_some())
        <= crate::value_vec::INLINE_PROPERTY_COUNT
}

/// The inline slots of a closure whose own properties fit them (`finished_closure_fits`).
fn finished_closure_slots(
    named: &[Value; FINISHED_CLOSURE_NAMED_SLOTS],
    prototype: Option<Value>,
) -> [Value; crate::value_vec::INLINE_PROPERTY_COUNT] {
    debug_assert!(finished_closure_fits(prototype));
    let mut slots = [Value::UNDEFINED; crate::value_vec::INLINE_PROPERTY_COUNT];
    slots[..FINISHED_CLOSURE_NAMED_SLOTS].copy_from_slice(named);
    if let (Some(prototype), Some(slot)) = (prototype, slots.get_mut(FINISHED_CLOSURE_NAMED_SLOTS)) {
        *slot = prototype;
    }
    slots
}

impl Vm {
    /// The creation realm of a function record: its override, else the main realm.
    #[inline(always)]
    pub fn record_realm(&self, record: &FunctionRecord) -> Value {
        record.realm_override().unwrap_or(self.main_realm)
    }

    /// `record` as a function created in `realm`: only a realm other than the main one (and an
    /// explicit `Value::NULL`) is stored, as an override.
    pub(super) fn in_realm_record(&mut self, mut record: FunctionRecord, realm: Value) -> FunctionRecord {
        if realm != self.main_realm {
            self.heap.set_function_realm_override(&mut record, realm);
        }
        record
    }

    /// Allocates `record` as a function created in `realm` (`in_realm_record`).
    pub(super) fn alloc_function_in(&mut self, record: FunctionRecord, realm: Value) -> Value {
        let record = self.in_realm_record(record, realm);
        self.heap.alloc_function(record)
    }

    pub fn function_realm(
        &mut self,
        p: &ResidualProgram,
        function: Value,
    ) -> Result<Value, JsError> {
        // Only the fields that decide the realm are copied out; the cell itself stays put.
        enum Origin {
            Bound(Value),
            Own(Value),
            Revoked,
            Proxied(Value),
            Current,
        }
        let origin = match (self.heap.function(function), self.heap.get(function)) {
            (
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::FunctionBoundCall),
                    env,
                    ..
                }),
                _,
            ) => Origin::Bound(*env),
            (Some(record), _) => Origin::Own(self.record_realm(record)),
            (None, Some(Cell::Proxy { handler, .. })) if handler.is_null() => Origin::Revoked,
            (None, Some(Cell::Proxy { target, .. })) => Origin::Proxied(*target),
            _ => Origin::Current,
        };
        match origin {
            Origin::Bound(env) => {
                let target_atom = self.runtime_atoms.bound_target;
                let target = self
                    .own_property(env, target_atom)
                    .unwrap_or(Value::UNDEFINED);
                self.function_realm(p, target)
            }
            Origin::Own(realm) => Ok(realm),
            Origin::Revoked => Err(self.type_error(p, "cannot access a revoked proxy".into())),
            Origin::Proxied(target) => self.function_realm(p, target),
            Origin::Current => Ok(self.realm.globals),
        }
    }

    pub(super) fn string_constructor_argument(&mut self, args: &[Value]) -> Value {
        args.first().copied().unwrap_or_else(|| {
            self.heap
                .alloc_string(super::wtf16::JsString::from_str(""))
        })
    }

    pub(super) fn string_constructor_value(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let argument = self.string_constructor_argument(args);
        let text = self.coerce_js_string(p, argument)?;
        Ok(self.heap.alloc_string(text))
    }

    /// SetFunctionName of `function` to the program atom `name` at cache `site`: the staged naming
    /// row along the edge the site recorded (`named_by_transition`), else the general naming, which
    /// records the edge it made for the next one.
    pub fn set_function_name(
        &mut self,
        p: &ResidualProgram,
        function: Value,
        (name, site): (Atom, u16),
    ) -> Result<(), JsError> {
        if (name as usize) >= p.atoms.len() {
            return Err(JsError("function name atom is invalid".into()));
        }
        if self.named_by_transition(function, name, site) {
            return Ok(());
        }
        // A program's atom ids are the VM's, so the name's atom is known without interning.
        let before = self.object_data(function).map(|data| (data.shape(), data.layout_id()));
        let value = self
            .inferred_function_name_slot(function)
            .map(|slot| (slot, self.atom_string(name)));
        if let (Some((slot, value)), Some(before)) = (value, before) {
            self.heap.property_set(function, slot, value);
            self.capture_function_name_for_atom(function, name);
            self.record_function_name_edge(site, function, name, (before, slot));
        }
        Ok(())
    }

    /// The word of the empty string every anonymous closure starts with as its name
    /// (`closure_in_realm`), `DELETED` before any exists, which no name slot holds.
    pub fn empty_string_word(&self) -> Value {
        self.heap.atom_string(self.runtime_atoms.empty, true).unwrap_or(Value::DELETED)
    }

    /// The string value spelling `atom`, shared for program atoms and the anonymous name.
    /// Other dynamic atoms grow with guest input, so their strings stay collectable.
    pub fn atom_string(&mut self, atom: Atom) -> Value {
        let keep = (atom as usize) < self.atom_text.len() || atom == self.runtime_atoms.empty;
        self.atom_string_kept(atom, keep)
    }

    /// `atom`'s string: the one the heap records as spelling it (`Heap::atom_string`), or a
    /// new one registered there. A kept string is a root; program atoms are kept always, a
    /// dynamic atom only where its kind bounds how many there can be, as a function's name
    /// is. Any other atom's string lives as long as something else holds it.
    fn atom_string_kept(&mut self, atom: Atom, keep: bool) -> Value {
        if let Some(value) = self.heap.atom_string(atom, keep) {
            return value;
        }
        let value = self.heap.alloc_string(self.atom_value(atom));
        self.heap.register_string_atom(value, atom, keep);
        value
    }

    pub fn set_function_name_key(&mut self, function: Value, key: Value, prefix: u32) {
        // A string key is its own name: strings are immutable, so the closure shares the key's
        // record instead of copying its text into a new one for every closure made with it.
        if prefix == crate::bytecode::FUNCTION_NAME_PREFIX_NONE && self.heap.string(key).is_some() {
            if let Some(slot) = self.inferred_function_name_slot(function) {
                self.heap.property_set(function, slot, key);
                self.capture_function_name(function, key);
            }
            return;
        }
        let name = match self.heap.get(key) {
            _ if let Some(value) = self.heap.string(key)  => value.to_string(),
            _ if let Some(description) = self.heap.symbol_data(key) => description
                .as_ref()
                .map_or_else(String::new, |description| format!("[{}]", description)),
            _ => return,
        };
        let name = match prefix {
            crate::bytecode::FUNCTION_NAME_PREFIX_GETTER => format!("get {name}"),
            crate::bytecode::FUNCTION_NAME_PREFIX_SETTER => format!("set {name}"),
            _ => name,
        };
        self.set_function_name_text(function, name);
    }

    pub(super) fn set_function_name_text(&mut self, function: Value, text: String) {
        let Some(slot) = self.inferred_function_name_slot(function) else {
            return;
        };
        let value = self.heap.alloc_string(text.into());
        self.heap.property_set(function, slot, value);
        self.capture_function_name(function, value);
    }

    /// The slot of a function's own `name` while it still holds the empty name an anonymous
    /// function definition starts with, which SetFunctionName replaces; `None` for anything
    /// else, which keeps its name.
    fn inferred_function_name_slot(&self, function: Value) -> Option<usize> {
        if !self.heap.is_function_value(function) {
            return None;
        }
        let name_atom = self.runtime_atoms.name;
        let current = self.own_property(function, name_atom).unwrap_or(Value::UNDEFINED);
        if !self.heap.string(current).is_some_and(|value| value.units().is_empty()) {
            return None;
        }
        self.shape_slot(self.object_data(function)?.shape(), name_atom)
    }

    pub(super) fn is_constructable(&self, value: Value) -> bool {
        let Some(FunctionRecord { kind, env, .. }) = self.heap.function(value) else {
            return matches!(
                self.heap.get(value),
                Some(Cell::Proxy {
                    kind: ProxyKind::Constructor,
                    ..
                })
            );
        };
        match kind {
                FunctionKind::Native(Native::HostFunction) => self.host_function_constructible(*env),
                FunctionKind::User(program_id, id) => {
                    self.programs.get(*program_id).is_some_and(|program| {
                        program.functions.get(*id as usize).is_some_and(|function| {
                            let header = function.header();
                            header.constructible && !header.is_async && !header.is_generator
                        })
                    })
                }
                FunctionKind::Native(Native::FunctionBoundCall) => {
                    self.own_property(*env, self.runtime_atoms.bound_target)
                        .is_some_and(|target| self.is_constructable(target))
                }
                FunctionKind::Native(native) => matches!(
                    native,
                    Native::Function
                        | Native::CallSiteConstructor
                        | Native::AbstractModuleSource
                        | Native::AsyncFunction
                        | Native::GeneratorFunction
                        | Native::AsyncGeneratorFunction
                        | Native::Iterator
                        | Native::Object
                        | Native::Proxy
                        | Native::Array
                        | Native::TypedArray
                        | Native::ArrayBuffer
                        | Native::SharedArrayBuffer
                        | Native::Uint8Array
                        | Native::Uint8ClampedArray
                        | Native::Uint16Array
                        | Native::Uint32Array
                        | Native::Int8Array
                        | Native::Int16Array
                        | Native::Int32Array
                        | Native::BigInt64Array
                        | Native::BigUint64Array
                        | Native::Float16Array
                        | Native::Float32Array
                        | Native::Float64Array
                        | Native::DataView
                        | Native::Map
                        | Native::Set
                        | Native::ShadowRealm
                        | Native::WeakMap
                        | Native::WeakSet
                        | Native::WeakRef
                        | Native::FinalizationRegistry
                        | Native::DisposableStack
                        | Native::AsyncDisposableStack
                        | Native::Date
                        | Native::BigInt
                        | Native::Error
                        | Native::AggregateError
                        | Native::SuppressedError
                        | Native::EvalError
                        | Native::RangeError
                        | Native::ReferenceError
                        | Native::SyntaxError
                        | Native::TypeError
                        | Native::RealmTypeError
                        | Native::URIError
                        | Native::RegExp
                        | Native::String
                        | Native::Boolean
                        | Native::Number
                        | Native::IntlNumberFormat
                        | Native::IntlCollator
                        | Native::IntlPluralRules
                        | Native::IntlDateTimeFormat
                        | Native::IntlDisplayNames
                        | Native::IntlDurationFormat
                        | Native::IntlListFormat
                        | Native::IntlLocale
                        | Native::IntlRelativeTimeFormat
                        | Native::IntlSegmenter
                        | Native::Promise
                        | Native::Symbol
                        | Native::TemporalDuration
                        | Native::TemporalPlainTime
                        | Native::TemporalInstant
                        | Native::TemporalPlainDate
                        | Native::TemporalPlainDateTime
                        | Native::TemporalZonedDateTime
                        | Native::TemporalPlainMonthDay
                        | Native::TemporalPlainYearMonth
                        | Native::WasmModule
                        | Native::WasmInstance
                        | Native::WasmMemory
                        | Native::WasmTable
                        | Native::WasmGlobal | Native::WasmTag
                        | Native::WasmCompileError
                        | Native::WasmLinkError
                        | Native::WasmRuntimeError
                ),
        }
    }

    pub fn closure(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
    ) -> Result<Value, JsError> {
        self.closure_in_realm(p, id, env, self.realm.globals)
    }

    /// The recorded template a lane closure of the active program's function `id` is made
    /// from in the running realm: none under a `with` scope, nor for a call-only closure
    /// (`Value::known_closure`), whose function has none.
    #[inline(always)]
    pub fn closure_template_in_lane(&self, id: u32) -> Option<ClosureTemplate> {
        if self.with_scope_entered {
            return None;
        }
        self.recorded_closure_template(id, self.realm.globals).filter(|template| !template.call_only)
    }

    /// The environment a closure of `template` made by an activation of the active program's
    /// function `maker` under `parent` takes when it reads nothing of the maker's own
    /// (`ClosureTemplate::maker_reads`): `parent` itself, or for one reading further out an
    /// environment of `maker` of no slots under it, shared by the maker's activations until a
    /// collection. `None` for a closure that reads the maker's environment.
    #[inline(always)]
    pub fn maker_free_environment(&mut self, template: &ClosureTemplate, parent: Value, maker: u32) -> Option<Value> {
        match template.maker_reads {
            crate::bytecode::MakerReads::Maker => None,
            crate::bytecode::MakerReads::Nothing => Some(parent),
            crate::bytecode::MakerReads::Outer => Some(match template.outer_environment {
                Some((held, env)) if held == parent => env,
                _ => {
                    let env = self.outer_environment(parent, maker);
                    let index = closure_template_index(template.program, template.function);
                    if let Some(slot) = self.closure_templates[index].as_mut() {
                        slot.outer_environment = Some((parent, env));
                    }
                    env
                }
            }),
        }
    }

    /// An environment of the active program's function `maker` under `parent` holding none
    /// of its bindings: one slot, never read, so its storage links to its parent's as an
    /// activation's does (`EnvironmentSlots::link`).
    #[cold]
    fn outer_environment(&mut self, parent: Value, maker: u32) -> Value {
        let slots = crate::heap::EnvironmentSlots::from_values(std::iter::once(Value::DELETED));
        let scope = crate::heap::EnvironmentScopeRef::of_program(self.active_program.raw());
        let (environment, storage) = self.heap.alloc_environment(parent, maker, slots, scope);
        if let Some(crate::heap::EnvironmentRecord { function: owner, slots: parent_slots, .. }) = self.heap.environment(parent)
            && *owner != u32::MAX
            && *owner != super::ROOT_FUNCTION_ID
            && let Some(parent_storage) = parent_slots.dense_storage()
            // SAFETY: the storage `alloc_environment` returned stays put while it lives.
            && let Some(own) = unsafe { &*storage }.dense_storage()
        {
            // SAFETY: both storages are live dense ones, the parent's outliving its child's
            // link until the next collection, whose epoch retires it.
            unsafe { crate::heap::EnvironmentSlots::link(own, parent_storage, self.heap.collection_epoch()) };
        }
        environment
    }

    /// Forgets the outer environments the templates hold (`ClosureTemplate::outer_environment`):
    /// at a collection, which they do not root.
    pub(super) fn forget_outer_environments(&mut self) {
        for template in self.closure_templates.iter_mut().flatten() {
            template.outer_environment = None;
        }
    }

    /// The template recorded for closures of the active program's function `id` in `realm`.
    #[inline(always)]
    fn recorded_closure_template(&self, id: u32, realm: Value) -> Option<ClosureTemplate> {
        self.closure_templates[closure_template_index(self.active_program, id)].filter(|template| {
            template.function == id && template.program == self.active_program && template.realm == realm
        })
    }

    /// A closure's object allocated finished: its own `length`, `name` and `prototype` under
    /// `shape`, as the recorded adds would leave them, inline where they fit and have narrow
    /// forms, else in the property arena. Inline: a closure's making is mostly this, and its
    /// object returns through no temporary. A wide value takes the arena rather than boxing
    /// `ObjectExtras` (`with_property_storage` has no arena of its own to widen into).
    #[inline(always)]
    pub(super) fn finished_closure_object(
        &mut self,
        function_prototype: Value,
        shape: u32,
        named: [Value; FINISHED_CLOSURE_NAMED_SLOTS],
        prototype: Option<Value>,
    ) -> crate::heap::Object {
        if finished_closure_fits(prototype) {
            let slots = finished_closure_slots(&named, prototype);
            if crate::heap::Object::inline_values_narrow(&slots) {
                return crate::heap::Object::with_property_storage(
                    function_prototype,
                    crate::value_vec::ValueVec::inline_property_storage(shape),
                    slots,
                );
            }
        }
        let [length, name] = named;
        let values: [Value; FINISHED_CLOSURE_SLOTS] = [length, name, prototype.unwrap_or(Value::UNDEFINED)];
        let count = FINISHED_CLOSURE_NAMED_SLOTS + usize::from(prototype.is_some());
        let (storage, inline) = self.heap.initial_object_storage(shape, &values[..count], false);
        crate::heap::Object::with_property_storage(function_prototype, storage, inline)
    }

    /// A closure of `template`'s function over `env`. The template's prototypes were flagged
    /// as prototypes by the closure it was recorded from, which needs no marking again.
    pub fn closure_of_template(&mut self, template: ClosureTemplate, env: Value) -> Value {
        // Both objects are allocated finished from headers derived once: the prototype under
        // the shape its `constructor` add would reach, its slot filled once the closure exists.
        // A closure whose record is claimed first is its prototype's `constructor` from the
        // start; any other fills that slot once it exists.
        let claimed = template.prototype.and_then(|_| self.heap.claim_function_record());
        let mut unfilled = false;
        let prototype = template.prototype.map(|header| {
            claimed
                .and_then(|function| self.heap.alloc_object_under_flagged_prototype_with_first(header, function))
                .unwrap_or_else(|| {
                    unfilled = true;
                    self.heap.alloc_unfilled_object_under_flagged_prototype(header)
                })
        });
        let named = [template.length, template.name];
        let object = if finished_closure_fits(prototype) {
            crate::heap::Object::from_inline_header(template.header, finished_closure_slots(&named, prototype))
        } else {
            let [length, name] = named;
            let values = [length, name, prototype.unwrap_or(Value::UNDEFINED), Value::UNDEFINED];
            let len = FINISHED_CLOSURE_NAMED_SLOTS + usize::from(prototype.is_some());
            self.heap.finished_run_object(template.run_header, &values, len)
        };
        let object = object.unwrap_or_else(|| {
            self.finished_closure_object(template.function_prototype, template.header.shape(), named, prototype)
        });
        let record = self.in_realm_record(FunctionRecord { object, kind: template.kind, env }, template.realm);
        let function = match claimed {
            Some(function) => {
                self.heap.fill_claimed_function_record(function, record);
                function
            }
            None => self.heap.alloc_function_under_flagged_prototype(record),
        };
        if unfilled && let Some(prototype) = prototype {
            self.heap.fill_new_object_slot(prototype, 0, function);
        }
        function
    }

    /// A closure of function `id` over `env`, in the scope chain of the innermost VM frame.
    /// A closure that takes no environment (`ResidualProgram::closure_takes_no_environment`)
    /// is made over `Value::NULL`.
    pub(super) fn closure_in_realm(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        realm: Value,
    ) -> Result<Value, JsError> {
        // A root closure starts a run of its program's root (`Vm::note_root_run`).
        if id == super::ROOT_FUNCTION_ID {
            self.note_root_run();
        }
        // No `with` was ever entered, so no closure captures a `with` object
        // (`Vm::with_scope_entered`): a recorded closure needs none of the scope work below.
        if !self.with_scope_entered
            && let Some(template) = self.recorded_closure_template(id, realm)
        {
            return Ok(self.closure_of_template(template, env));
        }
        let chain = self.frames.last().map_or_else(Vec::new, |frame| {
            self.with_stack[frame.with_base.min(self.with_stack.len())..].to_vec()
        });
        self.closure_over_scope(p, id, env, realm, &chain)
    }

    /// Notes that the active program's root starts another run (its root closure is made); from
    /// its second, `root_slot_written_once` answers for no slot.
    pub(in crate::vm) fn note_root_run(&self) {
        self.programs.note_root_run(self.active_program);
    }

    /// Whether root `slot` of `program` is written once while the root has run at most once
    /// (`ProgramStore::root_slot_written_once`): a read that found it initialized may keep the
    /// value for every later read.
    pub fn root_slot_written_once(&self, program: ProgramId, slot: u16) -> bool {
        self.programs.root_slot_written_once(program, slot)
    }

    /// `function`'s own `prototype` property holding `prototype`, as a function's creation
    /// defines it: read-only for a class constructor. A non-generator's `prototype` gets
    /// `constructor` naming `function` back.
    pub(super) fn define_instance_prototype(
        &mut self,
        function: Value,
        atom: Atom,
        prototype: Value,
        class_constructor: bool,
        generator: bool,
    ) -> Result<(), JsError> {
        self.set_property_with_attributes(
            function,
            atom,
            prototype,
            PropertyAttributes {
                writable: !class_constructor,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        if !generator {
            self.set_builtin_value_named(prototype, "constructor", function)?;
        }
        Ok(())
    }

    /// The shape of an object `closure_in_realm` just made.
    fn closure_shape(&self, object: Value) -> u32 {
        self.object_data(object)
            .expect("a new closure or prototype is an object")
            .shape()
    }

    /// `realm`'s `Object.prototype`: the VM field for the active realm, which follows it
    /// (`REALM_FOLLOWING_PROTOTYPES`), and the intrinsics table for any other.
    pub fn realm_object_prototype(&self, realm: Value) -> Value {
        if realm == self.realm.globals {
            return self.object_proto;
        }
        self.realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, Native::Object))
            .copied()
            .unwrap_or(self.object_proto)
    }

    /// `realm`'s `Function.prototype`, read as `realm_object_prototype` reads its object one.
    pub(super) fn realm_function_prototype(&self, realm: Value) -> Value {
        if realm == self.realm.globals {
            return self.function_proto;
        }
        self.realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, Native::Function))
            .copied()
            .unwrap_or(self.function_proto)
    }

    pub fn construct_value(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_value_with_new_target(p, callee, callee, args)
    }

    /// Whether `callee` is a native whose construction may run beside a lane's frameless
    /// activations (`DeferredLaneCall`): any but a host function and a bound function. An
    /// error constructor's stack capture reads the activations from the lane's frame walk
    /// (`Vm::lane_stack_sites`, frame-switches.md §5), or makes them frames where the walk
    /// declines, as every other observation point does.
    pub fn constructs_beside_activations(&self, callee: Value) -> bool {
        matches!(
            self.heap.function(callee),
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. })
                if !matches!(native, Native::HostFunction | Native::FunctionBoundCall)
        )
    }

    pub fn construct_value_with_new_target(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        new_target: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if let Some(constructed) = self.construction_shortcut(p, callee, new_target, args) {
            return constructed;
        }
        self.with_call_roots(
            std::iter::once(callee)
                .chain(std::iter::once(new_target))
                .chain(args.iter().copied()),
            |vm| {
                let _stack = vm.enter_stack()?;
                if matches!(vm.heap.get(callee), Some(Cell::Proxy { .. })) {
                    return vm.proxy_construct(p, callee, new_target, args);
                }
                if !vm.is_constructable(callee) {
                    return Err(vm.type_error(p, super::call_printer::NOT_CONSTRUCTOR_MESSAGE.into()));
                }
                if !vm.is_constructable(new_target) {
                    return Err(vm.type_error(p, "newTarget is not a constructor".into()));
                }
                let (kind, derived_constructor) = match vm.heap.function(callee) {
                    Some(FunctionRecord { kind, .. }) => {
                        let derived = match kind {
                            FunctionKind::User(program_id, id) => {
                                vm.programs.get(*program_id).is_some_and(|program| {
                                    program
                                        .function_get(*id as usize)
                                        .is_some_and(|function| function.derived_constructor)
                                })
                            }
                            _ => false,
                        };
                        (*kind, derived)
                    }
                    _ => unreachable!("IsConstructor accepted a non-function target"),
                };
                if let FunctionKind::User(program_id, id) =
                    kind
                {
                    let Some(program) = vm.programs.get(program_id) else {
                        return Err(
                            vm.type_error(p, "function belongs to an unavailable program".into())
                        );
                    };
                    let Some(function) = program.function_get(id as usize) else {
                        return Err(vm.type_error(p, "function index is outside its program".into()));
                    };
                    if !function.constructible {
                        return Err(vm.type_error(p, super::call_printer::NOT_CONSTRUCTOR_MESSAGE.into()));
                    }
                    if function.is_async {
                        return Err(JsError("async function is not a constructor".into()));
                    }
                    if function.is_generator {
                        return Err(JsError("generator function is not a constructor".into()));
                    }
                    if vm
                        .known_prototype_atom()
                        .is_some_and(|atom| vm.own_property(callee, atom).is_none())
                    {
                        return Err(JsError("arrow function is not a constructor".into()));
                    }
                }
                if let FunctionKind::Native(Native::FunctionBoundCall) = kind {
                    let env = match vm.heap.function(callee) {
                        Some(FunctionRecord { env, .. }) => *env,
                        _ => return Err(vm.type_error(p, "invalid bound function".into())),
                    };
                    let target_atom = vm.runtime_atoms.bound_target;
                    let args_atom = vm.runtime_atoms.bound_args;
                    let target = vm
                        .own_property(env, target_atom)
                        .ok_or_else(|| vm.type_error(p, "invalid bound function".into()))?;
                    let bound_args = vm
                        .own_property(env, args_atom)
                        .and_then(|value| {
                            vm.heap
                                .array_elements(value)
                                .map(|elements| elements.to_vec())
                        })
                        .unwrap_or_default();
                    let mut arguments = bound_args;
                    arguments.extend_from_slice(args);
                    let new_target = if new_target == callee {
                        target
                    } else {
                        new_target
                    };
                    return vm.construct_value_with_new_target(p, target, new_target, &arguments);
                }
                if let FunctionKind::Native(native) = kind
                    && native != Native::HostFunction
                {
                    let realm = vm.function_realm(p, callee)?;
                    let previous_global = vm.switch_realm_global(realm);
                    let result = vm.construct_native_with_new_target(p, native, args, new_target);
                    vm.switch_realm_global(previous_global);
                    let result = result?;
                    let result = if vm.intl_constructor_prototypes(native).is_none()
                        && !native.constructs_from_new_target()
                        && native != Native::Proxy
                        && !(native == Native::Object
                            && new_target == vm.native_value(Native::Object)
                            && args
                                .first()
                                .is_some_and(|value| !value.is_null() && !value.is_undefined()))
                    {
                        vm.set_constructed_prototype(p, result, new_target, native)?
                    } else {
                        result
                    };
                    // Object(value) can preserve an existing object or box a
                    // primitive through that primitive's own creation path.
                    let preserves_origin = native == Native::Object && callee == new_target
                        && args.first().is_some_and(|value| !value.is_null() && !value.is_undefined());
                    if native != Native::Proxy && !preserves_origin {
                        vm.capture_constructed_value(result, callee, callee == new_target);
                    }
                    return Ok(result);
                }
                let object = if derived_constructor {
                    Value::UNDEFINED
                } else {
                    let proto = vm.prototype_from_constructor(p, new_target)?;
                    let mut object = Self::empty_object(proto);
                    vm.capture_constructed_name(&mut object, callee, callee == new_target);
                    let start = object.shape();
                    let object = vm.heap.alloc_object(object);
                    // `callee`'s body runs next on it: its constant layout and the storage
                    // `new_target`'s receivers reach (`preshaped_receiver`).
                    vm.preshape_receiver(object, start, callee, new_target);
                    object
                };
                let previous_target = vm.construct_target;
                vm.set_construct_target(Some(ConstructionTarget { callee, new_target }));
                let result = if derived_constructor {
                    vm.call_user_for_construct(p, callee, args)
                        .map(|(value, this)| (value, Some(this)))
                } else {
                    vm.call_value(p, callee, object, args)
                        .map(|value| (value, None))
                };
                vm.set_construct_target(previous_target);
                let (result, this) = result?;
                if vm.is_object_like(result) {
                    return Ok(result);
                }
                if derived_constructor {
                    return vm.derived_construct_value(p, result, this.unwrap_or(Value::DELETED));
                }
                Ok(object)
            },
        )
    }

    /// What [[Construct]] of a derived constructor whose body completed with `value` yields,
    /// its frame's `this` binding being `this` (`Value::DELETED` while uninitialized)
    /// (ECMA-262 10.2.2 steps 10-12): an object it returns, else its `this` binding for
    /// `undefined`, which `super(...)` must have initialized; any other value is a TypeError.
    /// The recursive construction (`construct_value_with_new_target`) and the pending call of
    /// one pushed in place (`CallCompletion::DerivedConstructor`) both complete through it.
    pub(super) fn derived_construct_value(&mut self, p: &ResidualProgram, value: Value, this: Value) -> Result<Value, JsError> {
        if self.is_object_like(value) {
            return Ok(value);
        }
        if !value.is_undefined() {
            return Err(self.type_error(p, "derived constructor may only return an object or undefined".into()));
        }
        if this.is_deleted() {
            return Err(self.reference_error(p, "Must call super constructor before returning from derived constructor".into()));
        }
        Ok(this)
    }

    /// Whether `derived_construct_value` of `value` and `this` completes without an error, so a
    /// lane may finish the construction itself (`lane_return_script_frame`).
    pub fn derived_construct_completes(&self, value: Value, this: Value) -> bool {
        self.is_object_like(value) || value.is_undefined() && !this.is_deleted()
    }

    /// A function's own data `prototype`, read without the generic [[Get]]. Functions are
    /// ordinary objects for this key, so an own data slot is exactly the [[Get]] result; any
    /// other receiver or an accessor takes the generic path.
    /// `new callee(...)` that the general loop may run as an in-loop frame push: an ordinary
    /// (non-class, non-derived, field-free) user constructor of the executing program and realm,
    /// whose own data `prototype` is an object. Returns the callee's code and environment
    /// together with the freshly allocated receiver; any other case keeps `construct_value`.
    pub fn inline_construct_target(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        callee: Value,
    ) -> Option<(ProgramId, u32, Value, Value)> {
        let (program_id, id, env) = self.in_loop_constructor(p, frame, callee, super::ConstructKind::Base)?;
        Some((program_id, id, env, self.inline_construct_receiver(callee)?))
    }

    /// `new callee(...)` of a derived class constructor that the loop may push in place, as
    /// `inline_construct_target` admits a base one: its code and environment. It allocates no
    /// receiver: the body binds `this` through `super(...)` (`InitializeThis`), and its pending
    /// call yields that binding or the object the body returns (`CallCompletion::DerivedConstructor`).
    pub fn inline_derived_construct_target(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        callee: Value,
    ) -> Option<(ProgramId, u32, Value)> {
        self.in_loop_constructor(p, frame, callee, super::ConstructKind::Derived)
    }

    /// `super(...)` at frame `frame` of `parent`, a user constructor the loop may push in place
    /// with the running constructor's `new.target` (`construct_super_value`): a base parent
    /// (`inline_constructible`) over the receiver OrdinaryCreateFromConstructor makes from
    /// `new.target`'s own data `prototype` (`inline_construct_template_by`), completing as
    /// `CallCompletion::Constructor`; a derived parent (`inline_derived_constructible`), which
    /// binds its own, as `CallCompletion::DerivedConstructor`. Both the general loop
    /// (`Vm::try_inline_construct`) and a lane running the frame (`derived_frames`) push it. Any other parent
    /// (a native, a bound function, a proxy, a class with instance fields at its root, a
    /// `new.target` whose `prototype` is not an own data object) keeps `construct_super_value`.
    pub fn inline_super_construct_target(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        parent: Value,
    ) -> Option<SuperConstruction> {
        let new_target_atom = self.runtime_atoms.new_target;
        let new_target = self
            .dynamic_binding(self.frame_caller(frame), new_target_atom)
            .filter(|value| !value.is_undefined() && self.is_constructable(*value))?;
        if let Some(target) = self.in_loop_constructor(p, frame, parent, super::ConstructKind::Derived) {
            return Some(SuperConstruction { target, new_target, this: Value::UNDEFINED, completion: CallCompletion::DerivedConstructor });
        }
        let target = self.in_loop_constructor(p, frame, parent, super::ConstructKind::Base)?;
        let object = self.inline_construct_template_by(new_target, parent)?;
        let this = self.heap.alloc_object(object);
        Some(SuperConstruction { target, new_target, this, completion: CallCompletion::Constructor(this) })
    }

    /// The values `super(...)` applies from `array` (`ConstructArguments::Array`: an implicit
    /// derived constructor's rest list, or a spread) where reading them runs no guest code: a
    /// dense array's elements, as CreateListFromArrayLike reads them (`dense_apply_arguments`).
    /// `None` leaves the construction to `construct_super_value`, which reads them in full and
    /// rejects a non-array as the general step does.
    pub fn dense_super_arguments(&self, array: Value) -> Option<Vec<Value>> {
        self.heap.array(array)?;
        self.dense_apply_arguments(array)
    }

    /// The checks `inline_construct_target` makes of the running frame, and the program, function
    /// and environment of `callee` when its cached target (`in_loop_entry`) has the construct
    /// kind `kind`. The frame checks run per construction; the callee's classification is filled
    /// once per cache entry, with the same owner, root and wasm rules the call path applies.
    fn in_loop_constructor(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        callee: Value,
        kind: super::ConstructKind,
    ) -> Option<(ProgramId, u32, Value)> {
        let caller = &self.frames[frame];
        if frame + 1 != self.frames.len()
            || caller.function == super::ROOT_FUNCTION_ID
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || self.construct_target.is_some()
            || self.direct_eval
            || self.parameter_eval
            || p.function_get(caller.function as usize)?.dispatch != DispatchClass::General
        {
            return None;
        }
        // A constructor of another program is pushed in its own, which the pushing loop
        // makes active until the return restores the caller's (`StepResult::PushFrame`).
        let entry = self.in_loop_entry(p, callee)?;
        (entry.construct == kind).then_some((entry.program, entry.function, entry.env))
    }

    /// The `new` classification of `function`, which the call target cache stores with the entry
    /// it fills for the function: `Base` for `inline_constructible`, `Derived` for
    /// `inline_derived_constructible`, otherwise `No`.
    pub(super) fn construct_kind(function: &crate::bytecode::Function) -> super::ConstructKind {
        if Self::inline_constructible(function) {
            super::ConstructKind::Base
        } else if Self::inline_derived_constructible(function) {
            super::ConstructKind::Derived
        } else {
            super::ConstructKind::No
        }
    }

    /// A body whose [[Construct]] the dispatch loop may push in place: a constructible
    /// function that binds its receiver itself. A base class constructor's [[Construct]] is
    /// an ordinary one (OrdinaryCreateFromConstructor, then the body, whose instance fields
    /// are its own code); a derived constructor binds `this` through `super()` and runs its
    /// instance initializer there, so it is pushed with its own completion
    /// (`inline_derived_constructible`).
    pub fn inline_constructible(function: &crate::bytecode::Function) -> bool {
        function.dispatch == DispatchClass::General
            && function.constructible
            && !function.is_async
            && !function.is_generator
            && !function.derived_constructor
            && !function.class_initializer
            && function.instance_initializer().is_none()
    }

    /// A derived class constructor whose [[Construct]] the dispatch loop may push in place: its
    /// frame starts with `this` uninitialized (`activate_general_frame`), its `super(...)`
    /// binds it and runs its instance initializer (`InitializeThis`), and its pending call
    /// applies the derived return rules (`CallCompletion::DerivedConstructor`).
    pub fn inline_derived_constructible(function: &crate::bytecode::Function) -> bool {
        function.dispatch == DispatchClass::General
            && function.constructible
            && !function.is_async
            && !function.is_generator
            && function.derived_constructor
            && !function.class_initializer
    }

    /// The receiver an in-place [[Construct]] of `callee` allocates: an ordinary object
    /// whose prototype is the callee's own data `prototype`, when that is an object, in the
    /// callee's constant layout (`preshaped_receiver`), as every caller runs its body next.
    /// A construct plan's own allocations take the empty template (`inline_construct_template`):
    /// a plan records the stores its body adds from it.
    pub fn inline_construct_receiver(&mut self, callee: Value) -> Option<Value> {
        let object = self.inline_construct_template(callee)?;
        Some(self.alloc_construct_receiver(object, callee))
    }

    /// The object `inline_construct_receiver` allocates, before it is allocated.
    pub fn inline_construct_template(&mut self, callee: Value) -> Option<Object> {
        self.inline_construct_template_by(callee, callee)
    }

    /// The receiver [[Construct]] of `constructor` makes with `new_target` before its body
    /// runs (OrdinaryCreateFromConstructor): of `new_target`'s own data `prototype`, named by
    /// `constructor` as the general path names it (`capture_constructed_name`).
    pub fn inline_construct_template_by(&mut self, new_target: Value, constructor: Value) -> Option<Object> {
        let prototype_atom = self.prototype_atom();
        let prototype = self.own_function_prototype_data(new_target, prototype_atom)?;
        self.object_data(prototype)?;
        let mut object = Self::empty_object(prototype);
        self.capture_constructed_name(&mut object, constructor, constructor == new_target);
        Some(object)
    }

    /// `Get(constructor, "prototype")` of GetPrototypeFromConstructor: an own data property
    /// read in place (`own_function_prototype_data`), which runs no guest code, and any
    /// other through [[Get]].
    pub fn constructor_prototype_value(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.prototype_atom();
        match self.own_function_prototype_data(constructor, prototype_atom) {
            Some(prototype) => Ok(prototype),
            None => self.get_property(p, constructor, prototype_atom),
        }
    }

    pub(super) fn own_function_prototype_data(
        &self,
        constructor: Value,
        prototype_atom: Atom,
    ) -> Option<Value> {
        debug_assert_eq!(self.known_prototype_atom(), Some(prototype_atom));
        let object = &self.heap.function(constructor)?.object;
        let shape = object.shape();
        let key = PropertyKey::string(prototype_atom);
        if self.shape_is_dictionary(shape) {
            return self.heap.property_get(object, self.prototype_data_slot(constructor, shape, key)?);
        }
        let memo = &self.shapes[shape as usize];
        let slot = match memo.prototype_slot() {
            PROTOTYPE_SLOT_UNKNOWN => {
                let slot = self
                    .prototype_data_slot(constructor, shape, key)
                    .map_or(PROTOTYPE_SLOT_NONE, |slot| u32::try_from(slot).unwrap_or(PROTOTYPE_SLOT_UNKNOWN));
                if slot == PROTOTYPE_SLOT_UNKNOWN {
                    // A slot past every memo: read it unmemoized.
                    return self.heap.property_get(object, self.prototype_data_slot(constructor, shape, key)?);
                }
                memo.set_prototype_slot(slot);
                slot
            }
            slot => slot,
        };
        if slot == PROTOTYPE_SLOT_NONE {
            return None;
        }
        self.heap.property_get(object, slot as usize)
    }

    /// The slot of `constructor`'s own `prototype` data property `key` in its `shape`; `None`
    /// for an accessor or no such property. Out of line and of plain values: a construct the
    /// lane inlines this lookup into passes it no address of its stack.
    #[inline(never)]
    fn prototype_data_slot(&self, constructor: Value, shape: u32, key: PropertyKey) -> Option<usize> {
        self.property_attributes(constructor, key)
            .filter(|attributes| !attributes.accessor)
            .and_then(|_| self.property_shape_slot(shape, key))
    }

    /// `constructor`'s own shape and the slot of its own data `prototype` there, for a shape
    /// that fixes both (not a dictionary) once `own_function_prototype_data` has found it.
    pub fn function_prototype_layout(&self, constructor: Value) -> Option<(u32, usize)> {
        let shape = self.heap.function(constructor)?.object.shape();
        if self.shape_is_dictionary(shape) {
            return None;
        }
        // A shape whose `prototype` slot is not yet memoized learns it here, as the first
        // construction of the shape would (`own_function_prototype_data`), so a plain function's
        // first plan is recorded as a class's is.
        if self.shapes[shape as usize].prototype_slot() == PROTOTYPE_SLOT_UNKNOWN
            && let Some(prototype_atom) = self.known_prototype_atom()
        {
            let _ = self.own_function_prototype_data(constructor, prototype_atom);
        }
        match self.shapes[shape as usize].prototype_slot() {
            PROTOTYPE_SLOT_UNKNOWN | PROTOTYPE_SLOT_NONE => None,
            slot => Some((shape, slot as usize)),
        }
    }

    fn prototype_from_constructor(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.prototype_atom();
        let prototype = match self.own_function_prototype_data(constructor, prototype_atom) {
            Some(prototype) => prototype,
            None => self.get_property(p, constructor, prototype_atom)?,
        };
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let has_function_realm = self.is_function(constructor)
            || matches!(self.heap.get(constructor), Some(Cell::Proxy { .. }));
        let realm = if has_function_realm {
            self.function_realm(p, constructor)?
        } else {
            return Ok(self.object_proto);
        };
        Ok(self.realm_object_prototype(realm))
    }

    /// `realm`'s `Array.prototype`. The active realm's is the field that follows it
    /// (`Vm::realm_prototype_field`), read without the table.
    #[inline(always)]
    pub fn array_prototype_for_realm(&self, realm: Value) -> Value {
        if realm == self.realm.globals {
            return self.array_proto;
        }
        self.array_prototype_in_table(realm)
    }

    #[inline(never)]
    fn array_prototype_in_table(&self, realm: Value) -> Value {
        self.realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, Native::Array))
            .copied()
            .unwrap_or(self.array_proto)
    }

    pub(super) fn regexp_prototype_from_new_target(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.constructor_prototype_value(p, new_target)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = self.function_realm(p, new_target)?;
        Ok(self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&realm)
            .map(|intrinsics| intrinsics.prototype)
            .unwrap_or(self.regexp_proto))
    }

    pub(super) fn set_constructed_prototype(
        &mut self,
        p: &ResidualProgram,
        result: Value,
        new_target: Value,
        native: Native,
    ) -> Result<Value, JsError> {
        let Some(current) = self.object_data(result).map(Object::proto) else {
            return Ok(result);
        };
        // GetPrototypeFromConstructor reading an own data `prototype` runs no guest code, and
        // [[SetPrototypeOf]] to the prototype the object already has changes nothing. A
        // DataView still checks its buffer afterwards.
        if native != Native::DataView
            && let Some(prototype_atom) = self.known_prototype_atom()
            && self.own_function_prototype_data(new_target, prototype_atom) == Some(current)
        {
            return Ok(result);
        }
        let result_root = self.heap.root(result);
        let target_root = self.heap.root(new_target);
        let outcome = self
            .set_constructed_prototype_rooted(p, result_root, target_root, native)
            .map(|()| {
                self.heap
                    .root_value(result_root)
                    .expect("constructed object root remains live")
            });
        self.heap.release_root(target_root);
        self.heap.release_root(result_root);
        outcome
    }

    fn set_constructed_prototype_rooted(
        &mut self,
        p: &ResidualProgram,
        result_root: crate::heap::RootId,
        target_root: crate::heap::RootId,
        native: Native,
    ) -> Result<(), JsError> {
        let new_target = self.heap.root_value(target_root).unwrap();
        let Some(prototype) = self.native_constructor_prototype(p, new_target, native)? else {
            return Ok(());
        };
        let result = self
            .heap
            .root_value(result_root)
            .expect("constructed object root remains live");
        self.object_set_prototype_of(p, result, prototype)?;
        if native == Native::DataView
            && let Some((buffer, offset, length)) = self.data_view_view(result)
        {
            if self.array_buffer_detached(buffer) {
                return Err(self.type_error(p, "Cannot use a detached ArrayBuffer".into()));
            }
            if self.array_buffer_out_of_bounds(buffer, offset, length) {
                return Err(self.range_error(p, "Invalid DataView byte length".into()));
            }
        }
        Ok(())
    }

    pub(super) fn native_constructor_prototype(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
        native: Native,
    ) -> Result<Option<Value>, JsError> {
        let prototype_atom = self.prototype_atom();
        let prototype = self.constructor_prototype_value(p, new_target)?;
        let prototype = if prototype.is_null() || self.object_data(prototype).is_none() {
            let Some(intrinsic) = TypedArrayKind::of_constructor(native).map(TypedArrayKind::name)
                .or_else(|| match native {
                    Native::Object => Some("Object"),
                    Native::Function => Some("Function"),
                    Native::AsyncFunction => Some("AsyncFunction"),
                    Native::GeneratorFunction => Some("GeneratorFunction"),
                    Native::AsyncGeneratorFunction => Some("AsyncGeneratorFunction"),
                    Native::RegExp => Some("RegExp"),
                    Native::Number => Some("Number"),
                    Native::String => Some("String"),
                    Native::Iterator => Some("Iterator"),
                    Native::Boolean => Some("Boolean"),
                    Native::DataView => Some("DataView"),
                    Native::Date => Some("Date"),
                    Native::Promise => Some("Promise"),
                    Native::Error => Some("Error"),
                    Native::AggregateError => Some("AggregateError"),
                    Native::SuppressedError => Some("SuppressedError"),
                    Native::EvalError => Some("EvalError"),
                    Native::RangeError => Some("RangeError"),
                    Native::ReferenceError => Some("ReferenceError"),
                    Native::SyntaxError => Some("SyntaxError"),
                    Native::TypeError | Native::RealmTypeError => Some("TypeError"),
                    Native::URIError => Some("URIError"),
                    native => super::wasm_js::WASM_ERRORS
                        .iter()
                        .find_map(|(name, error)| (*error == native).then_some(*name)),
                })
            else {
                return Ok(None);
            };
            let realm = self.function_realm(p, new_target)?;
            let prototype = if native == Native::RegExp {
                self.realm
                    .intrinsics
                    .regexp_intrinsics
                    .get(&realm)
                    .map(|intrinsics| intrinsics.prototype)
                    .unwrap_or(self.regexp_proto)
            } else if let Some(prototype) = self
                .realm
                .intrinsics
                .builtin_prototypes
                .get(&(realm, native))
                .copied()
            {
                prototype
            } else {
                let constructor_atom = self.intern_atom(intrinsic);
                let constructor = self.get_property(p, realm, constructor_atom)?;
                self.get_property(p, constructor, prototype_atom)?
            };
            if self.object_data(prototype).is_none() {
                return Ok(None);
            }
            prototype
        } else {
            prototype
        };
        Ok(Some(prototype))
    }

    pub(super) fn construct_super_value(
        &mut self,
        p: &ResidualProgram,
        superclass: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_constructable(superclass) {
            return Err(self.type_error(p, "superclass is not a constructor".into()));
        }
        let new_target_atom = self.runtime_atoms.new_target;
        let new_target = self
            .frames
            .len()
            .checked_sub(1)
            .and_then(|frame| self.dynamic_binding(self.frame_caller(frame), new_target_atom))
            .filter(|value| !value.is_undefined())
            .ok_or_else(|| {
                self.reference_error(p, "new.target is unavailable for super()".into())
            })?;
        self.construct_value_with_new_target(p, superclass, new_target, args)
    }

    pub(super) fn construct_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let new_target = self.native_value(native);
        self.construct_native_with_new_target(p, native, args, new_target)
    }

    fn construct_native_with_new_target(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        if let Some(kind) = TypedArrayKind::of_constructor(native) {
            let name = kind.name();
            return self.construct_typed_array_with_new_target(
                p,
                args,
                kind,
                name,
                Some(new_target),
            );
        }
        match native {
            Native::CallSiteConstructor => self.call_site_constructor(p),
            Native::AbstractModuleSource => {
                Err(self.type_error(p, "AbstractModuleSource cannot be constructed".into()))
            }
            Native::Function
            | Native::AsyncFunction
            | Native::GeneratorFunction
            | Native::AsyncGeneratorFunction => self.function_native(p, native, args),
            Native::Iterator if new_target == self.native_value(Native::Iterator) => Err(self
                .type_error(
                    p,
                    "Iterator constructor cannot be called or constructed".into(),
                )),
            Native::Iterator => Ok(self.object()),
            Native::Object => {
                if new_target != self.native_value(Native::Object) {
                    return Ok(self.object());
                }
                if let Some(value) = args.first().copied() {
                    if self.object_data(value).is_some() {
                        return Ok(value);
                    }
                    if !value.is_null() && !value.is_undefined() {
                        return self.box_object(value);
                    }
                }
                Ok(self.object())
            }
            Native::Proxy => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                let handler = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if !self.is_object_like(target) {
                    return Err(self.type_error(p, "Proxy target must be an object".into()));
                }
                if !self.is_object_like(handler) {
                    return Err(self.type_error(p, "Proxy handler must be an object".into()));
                }
                let kind = if self.is_constructable(target) {
                    ProxyKind::Constructor
                } else if self.is_function(target) {
                    ProxyKind::Callable
                } else {
                    ProxyKind::Object
                };
                Ok(self.heap.alloc(Cell::Proxy {
                    object: Box::new(crate::heap::Object::proxy(self.object_proto)),
                    kind,
                    target,
                    handler,
                }))
            }
            Native::Array => self.construct_array_native(p, args, new_target),
            Native::TypedArray => {
                Err(self.type_error(p, "TypedArray is an abstract constructor".into()))
            }
            Native::ArrayBuffer | Native::SharedArrayBuffer => {
                self.construct_buffer_native(p, native, args, new_target)
            }
            Native::DataView => self.construct_data_view_native(p, args),
            Native::BigInt => Err(self.type_error(p, "BigInt cannot be called with new".into())),
            Native::Map | Native::Set => {
                self.construct_collection_native(p, native, args, new_target)
            }
            Native::ShadowRealm => self.construct_shadow_realm(p, new_target),
            native if super::wasm_js::is_wasm_js_constructor(native) => {
                self.construct_wasm_js_native(p, native, args)
            }
            Native::WeakMap | Native::WeakSet => {
                self.construct_weak_collection_native(p, native, args, new_target)
            }
            Native::WeakRef => self.construct_weak_ref_native(p, args, new_target),
            Native::FinalizationRegistry => {
                self.construct_finalization_registry_native(p, args, new_target)
            }
            Native::DisposableStack | Native::AsyncDisposableStack => {
                self.construct_disposable_stack_native(p, native, new_target)
            }
            Native::Promise => self.construct_promise(p, args),
            Native::RegExp => self.construct_regexp_native(p, args, Some(new_target)),
            Native::Date => self.date_construct_native(p, args, new_target),
            Native::IntlNumberFormat
            | Native::IntlCollator
            | Native::IntlPluralRules
            | Native::IntlDateTimeFormat
            | Native::IntlDisplayNames
            | Native::IntlDurationFormat
            | Native::IntlListFormat
            | Native::IntlLocale
            | Native::IntlRelativeTimeFormat
            | Native::IntlSegmenter
            | Native::TemporalDuration
            | Native::TemporalPlainTime
            | Native::TemporalPlainDate
            | Native::TemporalPlainDateTime
            | Native::TemporalPlainMonthDay
            | Native::TemporalPlainYearMonth
            | Native::TemporalZonedDateTime
            | Native::TemporalInstant => super::layer_hooks::intl_temporal_construct(self, p, native, args, new_target),
            Native::AggregateError => self.construct_aggregate_error(p, args, new_target),
            Native::Symbol => Err(self.type_error(p, "Symbol is not a constructor".into())),
            Native::Error
            | Native::SuppressedError
            | Native::EvalError
            | Native::RangeError
            | Native::ReferenceError
            | Native::SyntaxError
            | Native::TypeError
            | Native::URIError
            | Native::RealmTypeError
            | Native::WasmCompileError
            | Native::WasmLinkError
            | Native::WasmRuntimeError => {
                self.construct_error_with_new_target(p, native, args, new_target)
            }
            Native::String => {
                let value = self.string_constructor_value(p, args)?;
                self.box_primitive_object(value)
            }
            Native::Number => {
                let argument = args.first().copied().unwrap_or_else(|| Value::number(0.0));
                let primitive = if self.is_object_like(argument) {
                    self.to_primitive(p, argument, "number")?
                } else {
                    argument
                };
                let number = match self.heap.get(primitive) {
                    Some(Cell::BigInt(value)) => value.to_f64(),
                    _ => self.to_number(p, primitive)?,
                };
                let value = Value::number(number);
                self.box_primitive_object(value)
            }
            Native::Boolean => {
                let value = if args
                    .first()
                    .copied()
                    .is_some_and(|value| self.truthy(value))
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                };
                self.box_primitive_object(value)
            }
            _ => Err(JsError("native is not constructible".into())),
        }
    }
}
