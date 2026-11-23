//! A closure of a function over the scope chain of the activation making it: the `with` objects
//! it captures past those its environment carries, its prototype object, and its `length`,
//! `name` and `prototype` properties, defined one by one the first time and under the recorded
//! shapes after (`ClosureShapes`).
use super::*;

/// The attributes of a closure's own `length` and `name`.
const FUNCTION_METADATA_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    writable: false,
    enumerable: false,
    configurable: true,
    accessor: false,
    getter: None,
    setter: None,
};

/// What a closure of function `id` in `realm` is made of apart from its environment.
struct ClosureParts {
    id: u32,
    realm: Value,
    /// The prototype object its instances' creation reads, where it has one.
    prototype: Option<Value>,
    function_object_prototype: Value,
    length_value: Value,
    name_value: Value,
    plan_name: Option<Atom>,
    /// Neither async, a generator nor a class constructor: its properties take the recorded shapes.
    ordinary: bool,
    /// The `ClosureShapes` slot of its kind: whether it has a prototype object.
    shapes_key: usize,
}

impl Vm {
    /// A closure of function `id` over `env` made by an activation whose scope chain is `chain`,
    /// outermost first: its VM frame's (`closure_in_realm`) or a window activation's
    /// (`LaneStack::with_chain`). The closure captures the objects the activation entered,
    /// those past the ones its own environment already carries.
    pub fn closure_over_scope(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        realm: Value,
        chain: &[Value],
    ) -> Result<Value, JsError> {
        let plan = &p.functions[id as usize].header();
        let with_objects = self.closure_with_objects(p, id, env, chain);
        let plain_scope = with_objects.is_empty();
        if plain_scope && let Some(template) = self.recorded_closure_template(id, realm) {
            return Ok(self.closure_of_template(template, env));
        }
        let env = self.with_scope_environment(env, with_objects);
        let parts = self.closure_parts(plan, id, realm);
        let recorded_shapes = self.closure_shapes[parts.shapes_key].filter(|_| parts.ordinary);
        let (function, named_shape) = self.alloc_closure(&parts, env, recorded_shapes);
        // A class constructor's closure is a class evaluation, which makes the class's private
        // homes: the closure and its prototype object. No template makes one: only an ordinary
        // closure records its shapes.
        if plan.is_class_constructor {
            self.note_class_evaluation(id, function, parts.prototype);
        }
        self.record_top_level_closure(p, plan, id, function);
        let Some(shapes) = recorded_shapes else {
            self.define_closure_properties(function, plan, &parts)?;
            return Ok(function);
        };
        self.complete_recorded_closure(p, function, plan, &parts, (shapes, named_shape.filter(|_| plain_scope)));
        Ok(function)
    }

    /// The `with` objects of `chain` past those `env` already carries for function `id`.
    fn closure_with_objects(&self, p: &ResidualProgram, id: u32, env: Value, chain: &[Value]) -> Vec<Value> {
        if env.is_null() {
            return Vec::new();
        }
        let plan = p.functions[id as usize].header();
        let inherited = self.captured_with_objects_for_function(env, plan.inherited_with_scope, p.kind);
        chain.strip_prefix(inherited.as_slice()).unwrap_or(chain).to_vec()
    }

    /// `env` under a scope holding `with_objects`, or `env` itself where there are none.
    fn with_scope_environment(&mut self, env: Value, with_objects: Vec<Value>) -> Value {
        if with_objects.is_empty() {
            return env;
        }
        self.heap.alloc_environment(
            env,
            u32::MAX,
            Vec::<Value>::new().into_boxed_slice().into(),
            crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: None,
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: Vec::new().into(),
                with_objects: with_objects.into_boxed_slice(),
            }),
        ).0
    }

    /// The parts of a closure of `plan`, allocating its prototype object where it has one.
    fn closure_parts(&mut self, plan: &crate::bytecode::FunctionHeader, id: u32, realm: Value) -> ClosureParts {
        let has_instance_prototype = (plan.constructible || plan.is_generator) && !plan.prototype_unobservable;
        let prototype = has_instance_prototype.then(|| self.alloc_instance_prototype(plan, realm));
        let intrinsic = Native::function_constructor(plan.is_async, plan.is_generator);
        let plan_name = plan.name.or(plan.inferred_name);
        // Names are the compiled functions' own, so keeping each one's string is bounded by
        // the code loaded, while every closure of a module function would otherwise make one.
        let name_value = self.atom_string_kept(plan_name.unwrap_or(self.runtime_atoms.empty), true);
        ClosureParts {
            id,
            realm,
            prototype,
            function_object_prototype: self.realm.intrinsics.builtin_prototypes[&(realm, intrinsic)],
            length_value: Value::number(plan.length as f64),
            name_value,
            plan_name,
            ordinary: !plan.is_async && !plan.is_generator && !plan.is_class_constructor,
            shapes_key: usize::from(prototype.is_some()),
        }
    }

    /// The object a closure of `plan` makes the prototype of its instances: under the generator
    /// prototype of its kind for a generator, else under `Object.prototype`.
    fn alloc_instance_prototype(&mut self, plan: &crate::bytecode::FunctionHeader, realm: Value) -> Value {
        let parent = if plan.is_generator {
            let prototypes = &self.realm.intrinsics.iterator_prototypes[&realm];
            if plan.is_async { prototypes.async_generator } else { prototypes.generator }
        } else {
            self.realm_object_prototype(realm)
        };
        self.heap.alloc_object(Self::empty_object(parent))
    }

    /// The closure of `parts` over `env`, and the shape its own properties are named under where
    /// `recorded_shapes` made it finished: an ordinary closure's own properties and its
    /// prototype's `constructor` take the shapes the first one recorded, and a recorded closure
    /// is allocated finished (`finished_closure_object`), its `length`, `name` and `prototype` as
    /// the adds would leave them. Every other closure defines them one by one.
    fn alloc_closure(&mut self, parts: &ClosureParts, env: Value, recorded_shapes: Option<ClosureShapes>) -> (Value, Option<u32>) {
        let finished = recorded_shapes.map(|shapes| match (parts.prototype, shapes.prototype) {
            (Some(prototype), Some((with_prototype, _))) => (with_prototype, Some(prototype)),
            _ => (shapes.name, None),
        });
        let mut named_shape = None;
        let object = match finished {
            Some((shape, own_prototype)) => {
                let named = self.function_name_shape(shape, parts.plan_name.unwrap_or(self.runtime_atoms.empty));
                named_shape = Some(named);
                self.finished_closure_object(parts.function_object_prototype, named, [parts.length_value, parts.name_value], own_prototype)
            }
            None => Self::empty_object(parts.function_object_prototype),
        };
        let kind = FunctionKind::User(self.active_program, parts.id);
        (self.alloc_function_in(FunctionRecord { object, kind, env }, parts.realm), named_shape)
    }

    /// Records `function`, a closure of a non-module program's top level, as found again by plan
    /// (`cached_functions`): Wasm functions, which have no parent, and a script's top-level
    /// functions. A module's closures are not recorded: its hoisted declarations are its instance's
    /// (`ModuleFunctions`), and any other closure is made fresh each evaluation. A nested closure
    /// is never looked up, so it is not recorded either.
    fn record_top_level_closure(&mut self, p: &ResidualProgram, plan: &crate::bytecode::FunctionHeader, id: u32, function: Value) {
        if self.records_closure(p, plan, id) {
            let handle = self.heap.weak_handle(function).expect("new closure is live");
            self.function_values.record((self.active_program, id), [handle]);
        }
    }

    /// Whether a closure of `id` must be findable by `cached_functions`: a top-level closure, or one
    /// the loaded image's value table names (`ClosureOf`).
    pub(super) fn records_closure(&self, p: &ResidualProgram, plan: &crate::bytecode::FunctionHeader, id: u32) -> bool {
        // Only a closure an image's value table names is recorded beyond the top-level ones, and
        // only while that image's bodies install (`QUENCH_DEV=ahead-bodies`): the table is read at
        // a body's link, which the experiment gates.
        let named = crate::trace::Dev::AheadBodies.on()
            && self.programs.ahead_images(self.active_program, p).is_some_and(|images| images.names_closure(id));
        !p.is_module() && (plan.parent.is_none_or(|parent| parent == crate::vm::ROOT_FUNCTION_ID) || named)
    }

    /// Adds the `constructor` of a recorded closure's prototype, and for a plain-scope closure of
    /// a nested function records its template: a nested function's closures are found by none of
    /// its plans, so a template makes the next ones the same way. A prototype the recorded shapes
    /// do not hold leaves the template unmade.
    fn complete_recorded_closure(
        &mut self,
        p: &ResidualProgram,
        function: Value,
        plan: &crate::bytecode::FunctionHeader,
        parts: &ClosureParts,
        (shapes, template_shape): (ClosureShapes, Option<u32>),
    ) {
        let constructor = match (parts.prototype, shapes.prototype) {
            (Some(prototype), Some((_, constructor))) => {
                self.heap.property_add(prototype, function, constructor);
                Some(constructor)
            }
            _ => None,
        };
        if plan.parent.is_some_and(|parent| parent != crate::vm::ROOT_FUNCTION_ID)
            && parts.prototype.is_some() == constructor.is_some()
            && let Some(shape) = template_shape
        {
            self.closure_templates[closure_template_index(self.active_program, parts.id)] = Some(ClosureTemplate {
                program: self.active_program,
                function: parts.id,
                realm: parts.realm,
                function_prototype: parts.function_object_prototype,
                header: crate::heap::InlineObjectHeader::of(parts.function_object_prototype, shape),
                run_header: crate::heap::InlineObjectHeader::of_run(parts.function_object_prototype, shape),
                length: parts.length_value,
                name: parts.name_value,
                kind: FunctionKind::User(self.active_program, parts.id),
                prototype: constructor.map(|constructor| {
                    crate::heap::InlineObjectHeader::of(self.realm_object_prototype(parts.realm), constructor)
                }),
                // A call-only closure (`Value::known_closure`) names its maker's activation.
                maker_reads: if plan.prototype_unobservable {
                    crate::bytecode::MakerReads::Maker
                } else {
                    p.closure_reads.as_deref().map_or(crate::bytecode::MakerReads::Maker, |reads| {
                        reads.maker_reads(parts.id, || crate::bytecode::arrow_chain_reads_implicit_bindings(p, parts.id))
                    })
                },
                call_only: plan.prototype_unobservable,
                outer_environment: None,
            });
        }
    }

    /// Defines `function`'s own `length`, `name` and `prototype` one by one, and records the
    /// shapes they step through for the closures that follow where they are all shared.
    fn define_closure_properties(&mut self, function: Value, plan: &crate::bytecode::FunctionHeader, parts: &ClosureParts) -> Result<(), JsError> {
        let length = self.length_atom;
        self.set_property_with_attributes(function, length, parts.length_value, FUNCTION_METADATA_ATTRIBUTES)?;
        let length_shape = self.closure_shape(function);
        let name = self.intern_atom("name");
        self.set_property_with_attributes(function, name, parts.name_value, FUNCTION_METADATA_ATTRIBUTES)?;
        let name_shape = self.closure_shape(function);
        let prototype_atom = self.known_prototype_atom();
        let prototype_shapes = self.define_closure_prototype(function, plan, parts.prototype, prototype_atom)?;
        let recorded = ClosureShapes { length: length_shape, name: name_shape, prototype: prototype_shapes };
        if parts.ordinary && (parts.prototype.is_none() || prototype_atom.is_some()) && self.closure_shapes_shared(recorded) {
            self.closure_shapes[parts.shapes_key] = Some(recorded);
        }
        self.capture_fresh_closure_name(function, parts.plan_name);
        Ok(())
    }

    /// Defines `function`'s own `prototype` as `prototype`, and for a non-generator its
    /// `constructor`; the shapes of the function and of the prototype object after, for a
    /// non-generator that has one.
    fn define_closure_prototype(
        &mut self,
        function: Value,
        plan: &crate::bytecode::FunctionHeader,
        prototype: Option<Value>,
        prototype_atom: Option<Atom>,
    ) -> Result<Option<(u32, u32)>, JsError> {
        if let Some(prototype) = prototype
            && let Some(atom) = prototype_atom
        {
            self.define_instance_prototype(function, atom, prototype, plan.is_class_constructor, plan.is_generator)?;
        }
        let Some(prototype) = prototype.filter(|_| !plan.is_generator) else {
            return Ok(None);
        };
        if prototype_atom.is_none() {
            self.set_builtin_value_named(prototype, "constructor", function)?;
        }
        Ok(Some((self.closure_shape(function), self.closure_shape(prototype))))
    }

    /// Whether none of the shapes a closure's properties stepped through is a dictionary.
    fn closure_shapes_shared(&self, recorded: ClosureShapes) -> bool {
        let shared = |shape: u32| !self.shape_is_dictionary(shape);
        shared(recorded.length)
            && shared(recorded.name)
            && recorded.prototype.is_none_or(|(function, prototype)| shared(function) && shared(prototype))
    }
}
