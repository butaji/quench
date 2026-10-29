use super::typed_array_install::TYPED_ARRAY_INSTALLS;
use super::*;

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
    shape: u32,
    length: Value,
    name: Value,
    kind: FunctionKind,
    prototype: Option<(Value, u32)>,
}

/// The closure template slot of a program's function: functions of different programs share
/// ids, so the program is mixed in by a multiplicative hash (the 32-bit golden ratio).
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

impl<H: Host> Vm<H> {
    pub(super) fn function_realm(
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
            (Some(FunctionRecord { realm, .. }), _) => Origin::Own(*realm),
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

    pub(super) fn set_function_name(
        &mut self,
        p: &ResidualProgram,
        function: Value,
        name: Atom,
    ) -> Result<(), JsError> {
        if (name as usize) >= p.atoms.len() {
            return Err(JsError("function name atom is invalid".into()));
        }
        // A program's atom ids are the VM's, so the name's atom is known without interning.
        let value = self
            .inferred_function_name_slot(function)
            .map(|slot| (slot, self.atom_string(name)));
        if let Some((slot, value)) = value {
            self.heap.property_set(function, slot, value);
            self.capture_function_name_for_atom(function, name);
        }
        Ok(())
    }

    /// The string value spelling `atom`, shared for program atoms and the anonymous name.
    /// Other dynamic atoms grow with guest input, so their strings stay collectable.
    pub(super) fn atom_string(&mut self, atom: Atom) -> Value {
        let keep = (atom as usize) < self.atom_text.len() || atom == self.runtime_atoms.empty;
        self.atom_string_kept(atom, keep)
    }

    /// `atom`'s string, kept for the next request when `keep` holds. Program atoms are kept
    /// always; a dynamic atom is kept only where its kind bounds how many there can be, as a
    /// function's name is.
    fn atom_string_kept(&mut self, atom: Atom, keep: bool) -> Value {
        if keep {
            if let Some(&value) = self.atom_strings.get(&atom) {
                return value;
            }
            let value = self.heap.alloc_string(self.atom_value(atom));
            self.atom_strings.insert(atom, value);
            return value;
        }
        if let Some(&value) = self.atom_strings.get(&atom) {
            return value;
        }
        let slot = atom as usize & (super::ATOM_STRING_CACHE_SIZE - 1);
        if let Some(recent) = &self.recent_atom_strings
            && recent[slot].0 == atom
        {
            return recent[slot].1;
        }
        let value = self.heap.alloc_string(self.atom_value(atom));
        self.recent_atom_strings
            .get_or_insert_with(|| {
                Box::new([(super::NO_RECENT_ATOM, Value::UNDEFINED); super::ATOM_STRING_CACHE_SIZE])
            })[slot] = (atom, value);
        value
    }

    pub(super) fn set_function_name_key(&mut self, function: Value, key: Value, prefix: u32) {
        let name = match self.heap.get(key) {
            _ if let Some(value) = self.heap.string(key)  => value.to_string(),
            Some(Cell::Symbol(description)) => description
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

    pub(super) fn is_constructable(&self, p: &ResidualProgram, value: Value) -> bool {
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
                        .is_some_and(|target| self.is_constructable(p, target))
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
                        | Native::WasmGlobal
                        | Native::WasmCompileError
                        | Native::WasmLinkError
                        | Native::WasmRuntimeError
                ),
        }
    }

    pub(super) fn closure(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
    ) -> Result<Value, JsError> {
        self.closure_in_realm(p, id, env, self.realm.globals)
    }

    /// The template recorded for closures of the active program's function `id` in `realm`.
    #[inline(always)]
    fn recorded_closure_template(&self, id: u32, realm: Value) -> Option<ClosureTemplate> {
        self.closure_templates[closure_template_index(self.active_program, id)].filter(|template| {
            template.function == id && template.program == self.active_program && template.realm == realm
        })
    }

    /// A closure's object allocated finished: its own `length`, `name` and `prototype` under
    /// `shape`, as the recorded adds would leave them, inline where they fit, else in the
    /// property arena. Inline: a closure's making is mostly this, and its object returns
    /// through no temporary.
    #[inline(always)]
    pub(super) fn finished_closure_object(
        &mut self,
        function_prototype: Value,
        shape: u32,
        named: [Value; FINISHED_CLOSURE_NAMED_SLOTS],
        prototype: Option<Value>,
    ) -> crate::heap::Object {
        if finished_closure_fits(prototype) {
            return crate::heap::Object::with_property_storage(
                function_prototype,
                crate::value_vec::ValueVec::inline_property_storage(shape),
                finished_closure_slots(&named, prototype),
            );
        }
        let [length, name] = named;
        let values: [Value; FINISHED_CLOSURE_SLOTS] = [length, name, prototype.unwrap_or(Value::UNDEFINED)];
        let count = FINISHED_CLOSURE_NAMED_SLOTS + usize::from(prototype.is_some());
        let (storage, inline) = self.heap.initial_object_storage(shape, &values[..count], false);
        crate::heap::Object::with_property_storage(function_prototype, storage, inline)
    }

    /// A closure of `template`'s function over `env`. The template's prototypes were flagged
    /// as prototypes by the closure it was recorded from, which needs no marking again.
    fn closure_of_template(&mut self, template: ClosureTemplate, env: Value) -> Value {
        let prototype = template
            .prototype
            .map(|(parent, _)| self.heap.alloc_object_under_flagged_prototype(Self::empty_object(parent)));
        let object = self.finished_closure_object(
            template.function_prototype,
            template.shape,
            [template.length, template.name],
            prototype,
        );
        let function = self.heap.alloc_function_under_flagged_prototype(FunctionRecord {
            object,
            realm: template.realm,
            kind: template.kind,
            env,
        });
        if let (Some(prototype), Some((_, constructor))) = (prototype, template.prototype) {
            self.heap.property_add(prototype, function, constructor);
        }
        function
    }

    pub(super) fn closure_in_realm(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        mut env: Value,
        realm: Value,
    ) -> Result<Value, JsError> {
        // No `with` was ever entered, so no closure captures a `with` object
        // (`Vm::with_scope_entered`): a recorded closure needs none of the scope work below.
        if !self.with_scope_entered
            && let Some(template) = self.recorded_closure_template(id, realm)
        {
            return Ok(self.closure_of_template(template, env));
        }
        let function_plan = p.functions[id as usize].header();
        let with_objects = if env.is_null() {
            Vec::new()
        } else {
            let inherited = self.captured_with_objects_for_function(env, function_plan.inherited_with_scope, p.kind);
            let active = self.frames.last().map_or(&[][..], |frame| {
                &self.with_stack[frame.with_base.min(self.with_stack.len())..]
            });
            active
                .strip_prefix(inherited.as_slice())
                .unwrap_or(active)
                .to_vec()
        };
        let template_index = closure_template_index(self.active_program, id);
        let plain_scope = with_objects.is_empty();
        if plain_scope && let Some(template) = self.recorded_closure_template(id, realm) {
            return Ok(self.closure_of_template(template, env));
        }
        if !with_objects.is_empty() {
            env = self.heap.alloc(Cell::Environment {
                parent: env,
                function: u32::MAX,
                slots: Vec::<Value>::new().into_boxed_slice().into(),
                scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                    program: None,
                    root_eval_scope: false,
                    binding_site_pc: None,
                    dynamic_bindings: Vec::new().into(),
                    with_objects: with_objects.into_boxed_slice(),
                }),
            });
        }
        let has_instance_prototype = (function_plan.constructible || function_plan.is_generator)
            && !function_plan.prototype_unobservable;
        let prototype = if has_instance_prototype {
            let generator_prototype_parent = if function_plan.is_generator {
                let prototypes = &self.realm.intrinsics.iterator_prototypes[&realm];
                if function_plan.is_async {
                    prototypes.async_generator
                } else {
                    prototypes.generator
                }
            } else {
                self.realm_object_prototype(realm)
            };
            Some(
                self.heap
                    .alloc(Cell::Object(Self::empty_object(generator_prototype_parent))),
            )
        } else {
            None
        };
        let intrinsic = match (function_plan.is_async, function_plan.is_generator) {
            (true, true) => Native::AsyncGeneratorFunction,
            (false, true) => Native::GeneratorFunction,
            (true, false) => Native::AsyncFunction,
            (false, false) => Native::Function,
        };
        let function_object_prototype = self.realm.intrinsics.builtin_prototypes[&(realm, intrinsic)];
        let plan_name = function_plan.name.or(function_plan.inferred_name);
        // Names are the compiled functions' own, so keeping each one's string is bounded by
        // the code loaded, while every closure of a module function would otherwise make one.
        let name_value = self.atom_string_kept(plan_name.unwrap_or(self.runtime_atoms.empty), true);
        let length_value = Value::number(function_plan.length as f64);
        // An ordinary closure's own properties and its prototype's `constructor` take the
        // shapes the first one recorded; every other closure defines them one by one.
        let ordinary = !function_plan.is_async
            && !function_plan.is_generator
            && !function_plan.is_class_constructor;
        let shapes_key = usize::from(prototype.is_some());
        let recorded_shapes = self.closure_shapes[shapes_key].filter(|_| ordinary);
        // A recorded closure is allocated finished (`finished_closure_object`): its `length`,
        // `name` and `prototype` as the adds would leave them, under the shape they and its
        // original name would reach.
        let finished = recorded_shapes.map(|shapes| match (prototype, shapes.prototype) {
            (Some(prototype), Some((with_prototype, _))) => (with_prototype, Some(prototype)),
            _ => (shapes.name, None),
        });
        let mut named_shape = None;
        let object = match finished {
            Some((shape, own_prototype)) => {
                let named =
                    self.function_name_shape(shape, plan_name.unwrap_or(self.runtime_atoms.empty));
                named_shape = Some(named);
                self.finished_closure_object(function_object_prototype, named, [length_value, name_value], own_prototype)
            }
            None => Self::empty_object(function_object_prototype),
        };
        let kind = FunctionKind::User(self.active_program, id);
        let function = self.heap.alloc_function(FunctionRecord {
            object,
            realm,
            kind,
            env,
        });
        // Only closures of a program's top level are ever found again by plan
        // (`cached_functions`): a module root's hoisted functions and Wasm functions, which
        // have no parent. A nested closure is never looked up, so it is not recorded.
        if function_plan
            .parent
            .is_none_or(|parent| parent == super::ROOT_FUNCTION_ID)
        {
            let program = self.active_program;
            self.function_values.entry((program, id)).or_default().push(
                self.heap
                    .weak_handle(function)
                    .expect("new closure is live"),
            );
        }
        if let Some(shapes) = recorded_shapes {
            let constructor = match (prototype, shapes.prototype) {
                (Some(prototype), Some((_, constructor))) => {
                    self.heap.property_add(prototype, function, constructor);
                    Some(constructor)
                }
                _ => None,
            };
            // A nested function's closures are found by none of its plans, so a template
            // makes the next ones the same way; a top-level one is recorded above. A prototype
            // the recorded shapes do not hold leaves the template unmade.
            if plain_scope
                && function_plan.parent.is_some_and(|parent| parent != super::ROOT_FUNCTION_ID)
                && prototype.is_some() == constructor.is_some()
                && let Some(shape) = named_shape
            {
                self.closure_templates[template_index] = Some(ClosureTemplate {
                    program: self.active_program,
                    function: id,
                    realm,
                    function_prototype: function_object_prototype,
                    shape,
                    length: length_value,
                    name: name_value,
                    kind,
                    prototype: constructor.map(|constructor| (self.realm_object_prototype(realm), constructor)),
                });
            }
            return Ok(function);
        }
        let mut recorded = ClosureShapes {
            length: 0,
            name: 0,
            prototype: None,
        };
        let length = self.length_atom;
        self.set_property_with_attributes(
            function,
            length,
            length_value,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        recorded.length = self.closure_shape(function);
        let name = self.intern_atom("name");
        self.set_property_with_attributes(
            function,
            name,
            name_value,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        recorded.name = self.closure_shape(function);
        let prototype_atom = self.known_prototype_atom();
        if let Some(prototype) = prototype
            && let Some(atom) = prototype_atom
        {
            self.define_instance_prototype(
                function,
                atom,
                prototype,
                function_plan.is_class_constructor,
                function_plan.is_generator,
            )?;
        }
        if !function_plan.is_generator
            && let Some(prototype) = prototype
        {
            if prototype_atom.is_none() {
                self.set_builtin_value_named(prototype, "constructor", function)?;
            }
            recorded.prototype =
                Some((self.closure_shape(function), self.closure_shape(prototype)));
        }
        let shared = |shape: u32| !self.shape_is_dictionary(shape);
        if ordinary
            && (prototype.is_none() || prototype_atom.is_some())
            && shared(recorded.length)
            && shared(recorded.name)
            && recorded
                .prototype
                .is_none_or(|(function, prototype)| shared(function) && shared(prototype))
        {
            self.closure_shapes[shapes_key] = Some(recorded);
        }
        self.capture_fresh_closure_name(function, plan_name);
        Ok(function)
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
    pub(super) fn realm_object_prototype(&self, realm: Value) -> Value {
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

    pub(super) fn construct_value(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_value_with_new_target(p, callee, callee, args)
    }

    /// Whether `callee` is a native whose construction may run beside a lane's frameless
    /// activations (`DeferredLaneCall`): any but a host function, a bound function and an
    /// error constructor, which captures the stack its frames make.
    pub(super) fn constructs_beside_activations(&self, callee: Value) -> bool {
        matches!(
            self.heap.function(callee),
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. })
                if !matches!(native, Native::HostFunction | Native::FunctionBoundCall)
                    && !native.is_error_constructor()
        )
    }

    pub(super) fn construct_value_with_new_target(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        new_target: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        // Guest code observes the frame stack; a native's construction reaches it only through
        // the calls, errors and collections that make deferred activations frames first.
        if !self.constructs_beside_activations(callee) {
            self.materialize_deferred_lane();
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
                if !vm.is_constructable(p, callee) {
                    return Err(vm.type_error(p, super::call_printer::NOT_CONSTRUCTOR_MESSAGE.into()));
                }
                if !vm.is_constructable(p, new_target) {
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
                    vm.heap.alloc(Cell::Object(object))
                };
                let previous_target = vm.construct_target;
                vm.construct_target = Some(ConstructionTarget { callee, new_target });
                let result = if derived_constructor {
                    vm.call_user_for_construct(p, callee, args)
                        .map(|(value, this)| (value, Some(this)))
                } else {
                    vm.call_value(p, callee, object, args)
                        .map(|value| (value, None))
                };
                vm.construct_target = previous_target;
                let (result, this) = result?;
                if vm.is_object_like(result) {
                    return Ok(result);
                }
                if derived_constructor {
                    if result.is_undefined() {
                        let this = this.unwrap_or(Value::DELETED);
                        return if this.is_deleted() {
                            Err(vm.reference_error(
                                p,
                                "Must call super constructor before returning from derived constructor"
                                    .into(),
                            ))
                        } else {
                            Ok(this)
                        };
                    }
                    return Err(vm.type_error(
                        p,
                        "derived constructor may only return an object or undefined".into(),
                    ));
                }
                Ok(object)
            },
        )
    }

    /// A function's own data `prototype`, read without the generic [[Get]]. Functions are
    /// ordinary objects for this key, so an own data slot is exactly the [[Get]] result; any
    /// other receiver or an accessor takes the generic path.
    /// `new callee(...)` that the general loop may run as an in-loop frame push: an ordinary
    /// (non-class, non-derived, field-free) user constructor of the executing program and realm,
    /// whose own data `prototype` is an object. Returns the callee's code and environment
    /// together with the freshly allocated receiver; any other case keeps `construct_value`.
    pub(super) fn inline_construct_target(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        callee: Value,
    ) -> Option<(ProgramId, u32, Value, Value)> {
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
        let Some(FunctionRecord {
            kind: FunctionKind::User(program_id, id),
            env,
            realm,
            ..
        }) = self.heap.function(callee)
        else {
            return None;
        };
        let (program_id, id, env) = (*program_id, *id, *env);
        if *realm != self.realm.globals || id == super::ROOT_FUNCTION_ID {
            return None;
        }
        // A constructor of another program is pushed in its own, which the pushing loop
        // makes active until the return restores the caller's (`StepResult::PushFrame`).
        let body = if program_id == caller.program && program_id == self.active_program {
            p.function_get(id as usize)?
        } else {
            let owner = self.programs.residual(program_id)?;
            if owner.kind == crate::bytecode::ProgramKind::Wasm {
                return None;
            }
            owner.function_get(id as usize)?
        };
        if !Self::inline_constructible(body) {
            return None;
        }
        Some((program_id, id, env, self.inline_construct_receiver(callee)?))
    }

    /// A body whose [[Construct]] the dispatch loop may push in place: a constructible
    /// function that binds its receiver itself. A base class constructor's [[Construct]] is
    /// an ordinary one (OrdinaryCreateFromConstructor, then the body, whose instance fields
    /// are its own code); a derived constructor binds `this` through `super()` and runs its
    /// instance initializer there, so it keeps the native path.
    pub(super) fn inline_constructible(function: &crate::bytecode::Function) -> bool {
        function.dispatch == DispatchClass::General
            && function.constructible
            && !function.is_async
            && !function.is_generator
            && !function.derived_constructor
            && !function.class_initializer
            && !function.parameter_eval_arguments_error
            && function.instance_initializer.is_none()
    }

    /// The receiver an in-place [[Construct]] of `callee` allocates: an ordinary object
    /// whose prototype is the callee's own data `prototype`, when that is an object.
    pub(super) fn inline_construct_receiver(&mut self, callee: Value) -> Option<Value> {
        let object = self.inline_construct_template(callee)?;
        Some(self.heap.alloc_object(object))
    }

    /// The object `inline_construct_receiver` allocates, before it is allocated.
    pub(super) fn inline_construct_template(&mut self, callee: Value) -> Option<Object> {
        self.inline_construct_template_by(callee, callee)
    }

    /// The receiver [[Construct]] of `constructor` makes with `new_target` before its body
    /// runs (OrdinaryCreateFromConstructor): of `new_target`'s own data `prototype`, named by
    /// `constructor` as the general path names it (`capture_constructed_name`).
    pub(super) fn inline_construct_template_by(&mut self, new_target: Value, constructor: Value) -> Option<Object> {
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
    pub(super) fn constructor_prototype_value(
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
        let memo = &self.shapes[shape as usize].prototype_slot;
        let slot = match memo.get() {
            PROTOTYPE_SLOT_UNKNOWN => {
                let slot = self
                    .prototype_data_slot(constructor, shape, key)
                    .map_or(PROTOTYPE_SLOT_NONE, |slot| slot as u32);
                memo.set(slot);
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
    /// lane inlines this lookup into passes it no address of its stack, which would keep its
    /// dispatch from being a sibling call (`tools/check-lane-tail-calls.sh`).
    #[inline(never)]
    fn prototype_data_slot(&self, constructor: Value, shape: u32, key: PropertyKey) -> Option<usize> {
        self.property_attributes(constructor, key)
            .filter(|attributes| !attributes.accessor)
            .and_then(|_| self.property_shape_slot(shape, key))
    }

    /// `constructor`'s own shape and the slot of its own data `prototype` there, for a shape
    /// that fixes both (not a dictionary) once `own_function_prototype_data` has found it.
    pub(super) fn function_prototype_layout(&self, constructor: Value) -> Option<(u32, usize)> {
        let shape = self.heap.function(constructor)?.object.shape();
        if self.shape_is_dictionary(shape) {
            return None;
        }
        match self.shapes[shape as usize].prototype_slot.get() {
            PROTOTYPE_SLOT_UNKNOWN | PROTOTYPE_SLOT_NONE => None,
            slot => Some((shape, slot as usize)),
        }
    }

    /// `constructor`'s own data `prototype` while its own shape is still the one `layout`
    /// names (`function_prototype_layout`), which fixes the property and its slot.
    #[inline(always)]
    pub(super) fn function_prototype_in_layout(
        &self,
        constructor: Value,
        (shape, slot): (u32, usize),
    ) -> Option<Value> {
        let Some(FunctionRecord { object, .. }) = self.heap.function(constructor) else {
            return None;
        };
        if object.shape() != shape {
            return None;
        }
        // SAFETY: the matching non-dictionary shape (`function_prototype_layout`) holds `slot`.
        unsafe { self.heap.property_get_shaped(object, slot) }.filter(|value| !value.is_deleted())
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

    fn array_prototype_from_new_target(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.constructor_prototype_value(p, new_target)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = self.function_realm(p, new_target)?;
        Ok(self.array_prototype_for_realm(realm))
    }

    pub(super) fn array_prototype_for_realm(&self, realm: Value) -> Value {
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
            let Some(intrinsic) = TYPED_ARRAY_INSTALLS
                .iter()
                .find_map(|(_, candidate, name)| (*candidate == native).then_some(*name))
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
        if !self.is_constructable(p, superclass) {
            return Err(self.type_error(p, "superclass is not a constructor".into()));
        }
        let new_target_atom = self.runtime_atoms.new_target;
        let new_target = self
            .frames
            .len()
            .checked_sub(1)
            .and_then(|frame| self.dynamic_binding(frame, new_target_atom))
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
        if let Some(&(kind, _, name)) = TYPED_ARRAY_INSTALLS
            .iter()
            .find(|(_, candidate, _)| *candidate == native)
        {
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
                let kind = if self.is_constructable(p, target) {
                    ProxyKind::Constructor
                } else if self.is_function(target) {
                    ProxyKind::Callable
                } else {
                    ProxyKind::Object
                };
                Ok(self.heap.alloc(Cell::Proxy {
                    object: Box::new(Self::empty_object(self.object_proto)),
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
            Native::Date => self.date_construct_native(p, args),
            Native::IntlNumberFormat => self.intl_number_format_construct(p, args, new_target),
            Native::IntlCollator => self.intl_collator_construct(p, args, new_target),
            Native::IntlPluralRules => self.intl_plural_rules_construct(p, args, new_target),
            Native::IntlDateTimeFormat => self.intl_date_time_format_construct(p, args, new_target),
            Native::IntlDisplayNames => self.intl_display_names_construct(p, args, new_target),
            Native::IntlDurationFormat => self.intl_duration_format_construct(p, args, new_target),
            Native::IntlListFormat => self.intl_list_format_construct(p, args, new_target),
            Native::IntlLocale => self.intl_namespace_construct(p, native, args, new_target),
            Native::IntlRelativeTimeFormat => {
                self.intl_relative_time_format_construct(p, args, new_target)
            }
            Native::IntlSegmenter => self.intl_segmenter_construct(p, args, new_target),
            Native::TemporalDuration => self.temporal_duration_construct(p, args),
            Native::TemporalPlainTime => self.temporal_plain_time_construct(p, args),
            Native::TemporalPlainDate => self.temporal_plain_date_construct(p, args, new_target),
            Native::TemporalPlainDateTime => {
                self.temporal_plain_date_time_construct(p, args, new_target)
            }
            Native::TemporalPlainMonthDay | Native::TemporalPlainYearMonth => {
                self.temporal_calendar_projection_construct(p, native, args, new_target)
            }
            Native::TemporalZonedDateTime => {
                self.temporal_zoned_date_time_construct(p, args, new_target)
            }
            Native::TemporalInstant => self.temporal_instant_construct(p, args, new_target),
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
                let argument = args.first().copied().unwrap_or(Value::number(0.0));
                let primitive = if self.is_object_like(argument) {
                    self.to_primitive(p, argument, "number")?
                } else {
                    argument
                };
                let number = match self.heap.get(primitive) {
                    Some(Cell::BigInt(value)) => crate::bigint::to_f64(value),
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

    /// `new Array(length)` of a Number `length` with `prototype` (ECMA-262 23.1.1.1 step 5):
    /// an array of that length whose elements are all absent; `None` for a length that is no
    /// array length, which throws a RangeError. Out of line: the lane makes it too
    /// (`lane_native_construct`), whose frame then holds none of its temporaries.
    #[inline(never)]
    pub(super) fn array_of_length(&mut self, prototype: Value, length: f64) -> Option<Value> {
        if !length.is_finite() || length < 0.0 || length.fract() != 0.0 || length > f64::from(u32::MAX) {
            return None;
        }
        let array = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(prototype), Vec::new()),
        });
        self.heap.sparse_set_length(array, length as usize);
        Some(array)
    }

    fn construct_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.array_prototype_from_new_target(p, new_target)?;
        if let [length] = args
            && let Some(length) = length.as_number()
        {
            return match self.array_of_length(prototype, length) {
                Some(array) => Ok(array),
                None => Err(self.range_error(p, "Invalid array length".into())),
            };
        }

        Ok(self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(prototype),
                args.to_vec(),
            ),
        }))
    }
}
