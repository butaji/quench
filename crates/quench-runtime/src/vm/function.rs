use super::*;

/// The forwarding layers `Vm::forwarded_call` peels from one call: an uncurried builtin is
/// two (its bound `call`, then `call`); more is left to the general call path, which needs
/// no bound since each layer there is a native call of its own.
const CALL_FORWARDING_DEPTH: usize = 4;

/// SetFunctionLength's value for a bound function of a target whose `length` is
/// `target_length`, past `bound_count` bound values: 0 for a non-number.
fn bound_length(target_length: Value, bound_count: f64) -> f64 {
    match target_length.as_number() {
        None => 0.0,
        Some(target_length) if target_length.is_infinite() => target_length.max(0.0),
        Some(target_length) => (target_length.trunc() - bound_count).max(0.0),
    }
}

/// A bound function's bound arguments, then the call's, filled once on the native stack: no
/// heap allocation per call. The values are rooted as the callee's call roots them.
pub(super) fn bound_then(bound: &[Value], args: &[Value]) -> smallvec::SmallVec<[Value; 8]> {
    let mut combined = smallvec::SmallVec::<[Value; 8]>::with_capacity(bound.len() + args.len());
    combined.extend_from_slice(bound);
    combined.extend_from_slice(args);
    combined
}

/// A bound function's own `length` and `name`: read-only, not enumerable, configurable.
const BOUND_FUNCTION_NAMED_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    writable: false,
    enumerable: false,
    configurable: true,
    accessor: false,
    getter: None,
    setter: None,
};

impl Vm {
    pub(super) fn set_function_source(
        &mut self,
        function: Value,
        source: &str,
    ) -> Result<(), JsError> {
        let source_value = self.heap.alloc_string(source.into());
        self.set_function_source_value(function, source_value)
    }

    pub(super) fn set_function_source_value(
        &mut self,
        function: Value,
        source: Value,
    ) -> Result<(), JsError> {
        let source_atom = self.intern_atom("\0quench:function-source");
        self.set_property(function, source_atom, source)
    }

    /// The source text `Function.prototype.toString` reports for `function`: one recorded for
    /// it (a dynamic function's synthesized source), otherwise its definition's range of the
    /// program source, made a string on first request and shared by every closure of it.
    pub fn function_source_value(&mut self, function: Value) -> Option<Value> {
        let source_atom = self.intern_atom("\0quench:function-source");
        if let Some(source) = self.own_property(function, source_atom) {
            return Some(source);
        }
        if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = self.heap.function(function)
            && *native != Native::FunctionBoundCall
            && let Some(name) = self.original_function_name(function)
        {
            let source = native_function_source(self.atom_name(name));
            return Some(self.heap.alloc_string(source.as_str().into()));
        }
        let Some(FunctionRecord {
            kind: FunctionKind::User(program, id),
            ..
        }) = self.heap.function(function)
        else {
            return None;
        };
        let (program, id) = (*program, *id);
        if let Some(source) = self.programs.function_source(program, id) {
            return Some(source);
        }
        let residual = self.programs.residual(program)?;
        let span = residual.functions.get(id as usize)?.header().source_span?;
        let text = residual.source.as_ref()?.slice(span.start, span.end)?;
        let source = self.heap.alloc_string(text.into());
        self.programs
            .cache_function_source(program, id, source)
            .or(Some(source))
    }

    pub(super) fn function_caller_is_restricted(&self, function: Value) -> bool {
        match self.heap.function(function) {
            Some(FunctionRecord {
                kind: FunctionKind::User(program_id, id),
                ..
            }) => self.programs.get(*program_id).is_some_and(|program| {
                program
                    .function_get(*id as usize)
                    .is_some_and(|function| function.has_restricted_legacy_caller_access())
            }),
            Some(FunctionRecord {
                kind: FunctionKind::Native(_),
                ..
            }) => true,
            _ => false,
        }
    }

    /// `function_caller`, handed out: a caller whose `prototype` was deferred
    /// (`Function::prototype_unobservable`) gets it first, as its creation would have made
    /// it, so the value is the ordinary function it stands for.
    pub(super) fn function_caller_value(&mut self, function: Value) -> Result<Value, JsError> {
        // Legacy reflection reads the frame stack (`Vm::materialize_deferred_lane`).
        self.materialize_deferred_lane();
        let mut caller = self.function_caller(function);
        if caller.as_known_closure().is_some() {
            caller = self.known_closure_function(caller)?;
        }
        let Some(record) = self.heap.function(caller) else {
            return Ok(caller);
        };
        let FunctionKind::User(program, id) = record.kind else {
            return Ok(caller);
        };
        let realm = self.record_realm(record);
        let deferred = self
            .programs
            .get(program)
            .is_some_and(|code| code.functions.get(id as usize).is_some_and(|slot| slot.header().prototype_unobservable && !slot.header().is_arrow));
        let atom = self.intern_atom("prototype");
        if deferred && self.own_property(caller, atom).is_none() {
            let parent = self.realm_object_prototype(realm);
            let prototype = self.heap.alloc_object(Self::empty_object(parent));
            self.define_instance_prototype(caller, atom, prototype, false, false)?;
        }
        Ok(caller)
    }

    /// The function object a call-only closure (`Value::known_closure`) stands for, made when
    /// legacy reflection hands it out: each frame running it names the object from then on,
    /// so further reads in those calls return the same one. A closure of another program than
    /// the running one is not handed out.
    fn known_closure_function(&mut self, known: Value) -> Result<Value, JsError> {
        let Some((id, env)) = known.as_known_closure() else {
            return Ok(known);
        };
        let program = self.active_program;
        let Some(code) = self.programs.get(program) else {
            return Ok(Value::NULL);
        };
        if !self.frames.iter().any(|frame| frame.context.callable() == Some(known) && frame.program == program) {
            return Ok(Value::NULL);
        }
        let function = self.closure(&code, id, env)?;
        for frame in self.frames.iter_mut().filter(|frame| frame.context.callable() == Some(known)) {
            frame.context = CallContext::Function(function);
        }
        Ok(function)
    }

    fn function_caller(&self, function: Value) -> Value {
        let Some(mut index) = self
            .frames
            .iter()
            .rposition(|frame| frame.context.callable() == Some(function))
        else {
            return Value::NULL;
        };
        loop {
            if self.host_frame_hides_caller(index) {
                return Value::NULL;
            }
            let Some(parent) = index.checked_sub(1) else {
                return Value::NULL;
            };
            index = parent;
            match self.frames[index].context {
                CallContext::DirectEval(_) => continue,
                CallContext::Internal | CallContext::IndirectEval(_) => return Value::NULL,
                CallContext::Function(caller) | CallContext::Constructor(caller) => {
                    // A call-only closure's function is a plain sloppy one of its frame's program
                    // (`Function::prototype_unobservable`).
                    if let Some((id, _)) = caller.as_known_closure() {
                        let visible = self.programs.get(self.frames[index].program).is_some_and(|program| {
                            program.function_get(id as usize).is_some_and(|metadata| !metadata.strict)
                        });
                        return if visible { caller } else { Value::NULL };
                    }
                    let Some(record) = self.heap.function(caller) else {
                        return Value::NULL;
                    };
                    let FunctionKind::User(program_id, id) = &record.kind else {
                        return Value::NULL;
                    };
                    let visible = self.record_realm(record) == self.realm.globals
                        && self.programs.get(*program_id).is_some_and(|program| {
                            program.function_get(*id as usize).is_some_and(|metadata| {
                                !metadata.strict && !metadata.is_async && !metadata.is_generator
                            })
                        });
                    return if visible { caller } else { Value::NULL };
                }
            }
        }
    }

    pub(super) fn function_arguments(&mut self, function: Value) -> Result<Value, JsError> {
        self.materialize_deferred_lane();
        let Some(frame) = self
            .frames
            .iter()
            .rfind(|frame| frame.context.callable() == Some(function))
        else {
            return Ok(Value::NULL);
        };
        let program = self.programs.get(frame.program);
        let metadata = program
            .as_deref()
            .and_then(|program| program.function_get(frame.function as usize));
        let expose_callee = metadata.is_some_and(|metadata| metadata.simple_parameters);
        let reflected = (0..frame.reflected_parameters).map(|parameter| {
            let metadata = metadata.expect("reflected parameters belong to a live function");
            match metadata
                .local_registers
                .iter()
                .find(|entry| entry.local == parameter)
            {
                Some(entry) => frame.registers[usize::from(entry.register)],
                None => frame.locals[usize::from(parameter)],
            }
        });
        let values = reflected
            .chain(frame.original_arguments.iter().copied())
            .collect::<Vec<_>>();
        let arguments = self.heap.alloc_array(crate::heap::ArrayObject::new(
                Self::empty_object(self.realm_object_prototype(self.realm.globals)),
                crate::heap::ArrayElements::from_slice(&values),
            ));
        self.with_call_roots([arguments], |vm| {
            vm.initialize_arguments_object(arguments, Some(function), &values, expose_callee)?;
            Ok(arguments)
        })
    }

    pub fn bind_function(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_function(target) {
            return Err(JsError(super::call_printer::NOT_CALLABLE_MESSAGE.into()));
        }
        // BoundFunctionCreate's [[GetPrototypeOf]], then SetFunctionLength's and SetFunctionName's
        // reads of the target, which may run guest code; the bound function made after them
        // observes nothing they could.
        let target_root = self.heap.root(target);
        let metadata = self.bound_function_metadata(p, target_root, args.len().saturating_sub(1) as f64);
        let target = self.heap.root_value(target_root).expect("bound target root remains live");
        self.heap.release_root(target_root);
        let (prototype, length, name) = metadata?;
        let this = args.first().copied().unwrap_or(Value::UNDEFINED);
        let bound_args = self.heap.alloc_array(crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                args.get(1..).unwrap_or_default().to_vec(),
            ));
        // `name` is "bound " and the target's name, interned as a builtin's name is.
        let name_atom = self.intern_atom(&format!("bound {name}"));
        let name = self.heap.alloc_string(self.atom_value(name_atom));
        // Every bound function's own `length` and `name`, and its environment's three values,
        // reach the shapes the first one's adds recorded: the next ones are made finished there.
        if let Some((shape, env_shape)) = self.bound_function_shape {
            let (storage, inline) = self.heap.initial_object_storage(env_shape, &[target, this, bound_args], false);
            let object_prototype = self.realm_object_prototype(self.realm.globals);
            let env = self.heap.alloc_object(crate::heap::Object::with_property_storage(object_prototype, storage, inline));
            let realm = self.realm.globals;
            let object = self.finished_closure_object(prototype, shape, [Value::number(length), name], None);
            let function = self.alloc_function_in(FunctionRecord { object, kind: FunctionKind::Native(Native::FunctionBoundCall), env }, realm);
            self.capture_function_name_for_atom(function, name_atom);
            return Ok(function);
        }
        let env = self.object();
        self.set_property(env, self.runtime_atoms.bound_target, target)?;
        self.set_property(env, self.runtime_atoms.bound_this, this)?;
        self.set_property(env, self.runtime_atoms.bound_args, bound_args)?;
        let function = self.native_with_env(Native::FunctionBoundCall, env);
        self.heap.object_mut(function).expect("bound function has object storage").set_proto(prototype);
        let length_atom = self.length_atom;
        self.set_property_with_attributes(function, length_atom, Value::number(length), BOUND_FUNCTION_NAMED_ATTRIBUTES)?;
        self.set_property_with_attributes(function, self.runtime_atoms.name, name, BOUND_FUNCTION_NAMED_ATTRIBUTES)?;
        self.capture_function_name_for_atom(function, name_atom);
        let shape_of = |vm: &Self, object: Value| vm.heap.object(object).map(|object| object.shape()).filter(|&shape| !vm.shape_is_dictionary(shape));
        self.bound_function_shape = shape_of(self, function).zip(shape_of(self, env));
        Ok(function)
    }

    /// The own data property `atom` of an ordinary object whose shape holds every own
    /// property's attributes and no getter (`Vm::plain_function_length`'s test): its value,
    /// read observing nothing. `None` for an accessor, an absent name or any other object.
    pub fn plain_own_data(&self, object: Value, atom: Atom) -> Option<Value> {
        let data = self.heap.object(object)?;
        // record-only: side descriptors
        if data.has_side_descriptors() || self.shapes[data.shape() as usize].facts.slot_getters() {
            return None;
        }
        let slot = self.shape_slot(data.shape(), atom)?;
        self.heap.property_get_ref(data, slot).filter(|value| !self.is_accessor_pair(data.shape(), *value))
    }

    /// What a bound function of the target `target_root` holds is read from it (ECMA-262
    /// 10.4.1.3, 20.2.3.2): its prototype, its own `length` less the `bound_count` values
    /// bound (0 for none, or a non-number), and its `name` (empty for a non-string).
    fn bound_function_metadata(
        &mut self,
        p: &ResidualProgram,
        target_root: RootId,
        bound_count: f64,
    ) -> Result<(Value, f64, String), JsError> {
        let target = self.heap.root_value(target_root).expect("bound target root remains live");
        let prototype = self.object_get_prototype_of(p, target)?;
        let prototype_root = self.heap.root(prototype);
        let outcome = self.bound_function_length_and_name(p, target_root, bound_count);
        self.heap.release_root(prototype_root);
        let (length, name) = outcome?;
        Ok((prototype, length, name))
    }

    fn bound_function_length_and_name(&mut self, p: &ResidualProgram, target_root: RootId, bound_count: f64) -> Result<(f64, String), JsError> {
        let target = self.heap.root_value(target_root).expect("bound target root remains live");
        // A target holding both as own data properties, as a function holds them unless
        // redefined, answers HasOwnProperty and both reads from its shape, running nothing.
        if let Some(length) = self.plain_own_data(target, self.length_atom)
            && let Some(name) = self.plain_own_data(target, self.runtime_atoms.name)
        {
            return Ok((bound_length(length, bound_count), self.heap.string(name).map(ToString::to_string).unwrap_or_default()));
        }
        let length_atom = self.length_atom;
        let length_key = self.heap.alloc_string(self.atom_value(length_atom));
        // HasOwnProperty(Target, "length"): [[GetOwnProperty]], a Proxy's through its trap.
        let length = if self.own_property_enumerable(p, target, length_key)?.is_some() {
            let target = self.heap.root_value(target_root).expect("bound target root remains live");
            bound_length(self.get_property(p, target, length_atom)?, bound_count)
        } else {
            0.0
        };
        let target = self.heap.root_value(target_root).expect("bound target root remains live");
        let name_atom = self.runtime_atoms.name;
        let target_name = self.get_property(p, target, name_atom)?;
        let name = self.heap.string(target_name).map(ToString::to_string).unwrap_or_default();
        Ok((length, name))
    }

    pub(super) fn call_bound_function(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let env = self
            .active_native_env()
            .ok_or_else(|| JsError("invalid bound function".into()))?;
        let (target, receiver, bound_args) = self
            .bound_function_parts(env)
            .ok_or_else(|| JsError("invalid bound function".into()))?;
        if bound_args.is_empty() {
            // `Function.prototype.call` bound to a function, as Node's primordials uncurry every
            // method: the call is that function's, with the first argument as its receiver.
            if matches!(
                self.heap.function(target),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionCall), .. })
            ) {
                let this = args.first().copied().unwrap_or(Value::UNDEFINED);
                return self.call_value(p, receiver, this, args.get(1..).unwrap_or_default());
            }
            return self.call_value(p, target, receiver, args);
        }
        self.call_value(p, target, receiver, &bound_then(&bound_args, args))
    }

    /// The call a call of `callee` with `receiver` and `args` makes without an observable
    /// step of its own: a bound function without bound arguments calls its target with its
    /// bound `this` and the same arguments (ECMA-262 10.4.1.1), and `Function.prototype.call`
    /// calls its receiver with the first argument as `this` and the rest (20.2.3.3), so an
    /// uncurried builtin (`bind.bind(call)(f)`) calls `f`. `None` unless at least one such
    /// layer was peeled within `CALL_FORWARDING_DEPTH`; the target may still be anything,
    /// which the caller checks before making the call itself, and otherwise makes the
    /// original call.
    /// With `args` the caller owns (`SmallVec`), as an applied list's copied elements, a
    /// bound function's bound arguments lead the values it passes on (10.4.1.1); borrowed
    /// arguments (`&[Value]`) leave such a call to the original call.
    pub fn forwarded_call<A: ForwardedArgs>(
        &self,
        mut callee: Value,
        mut receiver: Value,
        mut args: A,
    ) -> Option<Forwarded<A>> {
        let mut through_call = false;
        for depth in 0..CALL_FORWARDING_DEPTH {
            match self.heap.function(callee) {
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall), env, .. }) => {
                    let (target, this, bound) = self.bound_function_parts(*env)?;
                    if !args.prepend(&bound) {
                        return None;
                    }
                    (callee, receiver) = (target, this);
                }
                Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionCall), .. }) => {
                    through_call |= depth == 0;
                    callee = receiver;
                    receiver = args.shift();
                }
                record => {
                    let kind = record.map(|record| record.kind);
                    return (depth != 0).then_some(Forwarded { callee, receiver, args, kind, through_call });
                }
            }
        }
        None
    }

    /// A plain call forwarding without an observable step of its own (`forwarded_call`) to a
    /// user function the loop pushes in place, with the forwarded receiver and arguments
    /// (`function_call_miss`): `f.call(thisArg, ...args)`, a bound function without bound arguments.
    #[inline(never)]
    pub(super) fn try_forwarded_user_call(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, pc: usize,
        callee: Value) -> Option<Result<StepResult, JsError>> {
        // Only a call passing a register window forwards its values (not `CallArray`'s array).
        if !matches!(i.op().immediate_layout(), crate::bytecode::ImmediateLayout::CallWindowWithEvalFlags) { return None; }
        let window: *const [Value] = self.register_window(f, i.call_window());
        // SAFETY: the window lies in the caller frame, which callee setup only reads.
        let forwarded = self.forwarded_call(callee, self.read(f, i.register_c()), unsafe { &*window })?;
        let target = self.in_loop_call_target(p, forwarded.callee)?;
        self.push_in_loop_call(p, f, pc, target, forwarded.callee, forwarded.receiver, forwarded.args, i.result_register())
    }

    /// A bound function's target, bound `this` and bound arguments, read from the
    /// environment object `Function.prototype.bind` made (`env`); `None` for any other.
    pub(super) fn bound_function_parts(&self, env: Value) -> Option<(Value, Value, std::borrow::Cow<'_, [Value]>)> {
        let (target, receiver, list) = self.bound_function_words(env)?;
        let arguments = Some(list)
            .filter(|list| !list.is_undefined())
            .and_then(|list| self.heap.array_elements(list))
            .map_or(std::borrow::Cow::Borrowed(&[][..]), |elements| elements.as_slice());
        Some((target, receiver, arguments))
    }

    /// `bound_function_parts` as the three words the environment holds: the target, the bound
    /// `this` and the list of bound arguments (`undefined` for none), which `bind` made and no
    /// script can reach, so its elements never change. A call site may keep the words for a
    /// bound function it calls again (`LaneStack::bound_call`).
    pub fn bound_function_words(&self, env: Value) -> Option<(Value, Value, Value)> {
        // record-only: flat slot reads below take the record
        let data = self.heap.object(env)?.record_only()?;
        let shape = data.shape();
        // The environment's three slots, read through the layout its shape keeps for every
        // bound function (`bound_parts_layout`), found by name once per shape.
        let slots = match self.bound_parts_layout.get() {
            Some((layout, slots)) if layout == shape => Some(slots),
            _ => {
                let atoms = self.runtime_atoms;
                let slots = [atoms.bound_target, atoms.bound_this, atoms.bound_args]
                    .map(|atom| self.shape_slot(shape, atom).and_then(|slot| u16::try_from(slot).ok()));
                match slots {
                    [Some(target), Some(this), Some(args)]
                        if !self.shape_may_have_accessors(shape) && !self.shape_is_dictionary(shape) =>
                    {
                        self.bound_parts_layout.set(Some((shape, [target, this, args])));
                        Some([target, this, args])
                    }
                    _ => None,
                }
            }
        };
        let (target, receiver, arguments) = match slots {
            // SAFETY: the environment's shape is the layout's, outside dictionary storage, which
            // holds the three slots; nothing deletes a bound function's internal properties.
            Some([target, this, args]) => unsafe {
                (
                    self.heap.property_get_flat(data, usize::from(target)),
                    self.heap.property_get_flat(data, usize::from(this)),
                    Some(self.heap.property_get_flat(data, usize::from(args))).filter(|list| !list.is_undefined()),
                )
            },
            None => (
                self.own_property(env, self.runtime_atoms.bound_target)?,
                self.own_property(env, self.runtime_atoms.bound_this).unwrap_or(Value::UNDEFINED),
                self.own_property(env, self.runtime_atoms.bound_args),
            ),
        };
        Some((target, receiver, arguments.unwrap_or(Value::UNDEFINED)))
    }
}

/// `NativeFunction` source text (ECMA-262 20.2.3.5) for a builtin named `name`: the name
/// stands in the text only where it is an identifier, or a `get `/`set ` accessor's or a
/// well-known symbol's identifier member.
fn native_function_source(name: &str) -> String {
    let property_name = name
        .strip_prefix("get ")
        .or_else(|| name.strip_prefix("set "))
        .unwrap_or(name);
    let symbol_member = property_name
        .strip_prefix("[Symbol.")
        .and_then(|name| name.strip_suffix(']'));
    let source_name = if quench_regexp::identifier::is_identifier_name(property_name)
        || symbol_member.is_some_and(quench_regexp::identifier::is_identifier_name)
    {
        name
    } else {
        ""
    };
    format!("function {source_name}() {{ [native code] }}")
}

/// The call a forwarding call makes (`Vm::forwarded_call`), with the facts the walk read on
/// the way, so no caller reads them again.
pub struct Forwarded<A> {
    pub callee: Value,
    pub receiver: Value,
    pub args: A,
    /// The callee's kind; `None` for a callee that is no function.
    pub kind: Option<FunctionKind>,
    /// Whether the outermost layer was `Function.prototype.call`.
    pub through_call: bool,
}

/// The arguments a forwarded call passes on (`Vm::forwarded_call`).
pub(super) trait ForwardedArgs {
    /// Removes the first argument, `undefined` when there is none.
    fn shift(&mut self) -> Value;
    /// Puts `bound` before the arguments; `false` when this list cannot hold them.
    fn prepend(&mut self, bound: &[Value]) -> bool;
}

impl ForwardedArgs for &[Value] {
    #[inline]
    fn shift(&mut self) -> Value {
        let first = self.first().copied().unwrap_or(Value::UNDEFINED);
        *self = self.get(1..).unwrap_or_default();
        first
    }
    fn prepend(&mut self, bound: &[Value]) -> bool {
        bound.is_empty()
    }
}

/// A forwarded call's arguments read where the caller's window holds them until a bound
/// prefix must lead them, then filled once, prefix then window, into an owned list: no copy
/// of the window before the walk, and none moved again by the prepend.
pub enum WindowArgs<'a, const N: usize> {
    Window(&'a [Value]),
    Owned(smallvec::SmallVec<[Value; N]>),
}

impl<const N: usize> WindowArgs<'_, N> {
    /// The values as an owned list, copied out of the window where they still lie there.
    pub fn into_owned(self) -> smallvec::SmallVec<[Value; N]> {
        match self {
            Self::Window(window) => smallvec::SmallVec::from_slice(window),
            Self::Owned(values) => values,
        }
    }
}

impl<const N: usize> ForwardedArgs for WindowArgs<'_, N> {
    fn shift(&mut self) -> Value {
        match self {
            Self::Window(window) => window.shift(),
            Self::Owned(values) => values.shift(),
        }
    }
    fn prepend(&mut self, bound: &[Value]) -> bool {
        if bound.is_empty() {
            return true;
        }
        match self {
            Self::Window(window) => {
                let mut values = smallvec::SmallVec::with_capacity(bound.len() + window.len());
                values.extend_from_slice(bound);
                values.extend_from_slice(window);
                *self = Self::Owned(values);
                true
            }
            Self::Owned(values) => values.prepend(bound),
        }
    }
}

impl<const N: usize> ForwardedArgs for smallvec::SmallVec<[Value; N]> {
    fn shift(&mut self) -> Value {
        if self.is_empty() { Value::UNDEFINED } else { self.remove(0) }
    }
    fn prepend(&mut self, bound: &[Value]) -> bool {
        self.insert_many(0, bound.iter().copied());
        true
    }
}
