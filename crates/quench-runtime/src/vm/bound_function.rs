//! Bound functions and forwarded calls: how `Function.prototype.bind` makes a bound function,
//! how a bound function's call and a forwarding call reach their target, and the parts a bound
//! function reads back.

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

/// A bound function's own `length` and `name`: read-only, not enumerable, configurable.
const BOUND_FUNCTION_NAMED_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    writable: false,
    enumerable: false,
    configurable: true,
    accessor: false,
    getter: None,
    setter: None,
};

/// The parts a bound function is made of: its target, the bound `this` and arguments array,
/// the prototype it inherits, and the `length` and `name` it reports, with the atom that name
/// was interned as.
struct BoundParts {
    target: Value,
    this: Value,
    args: Value,
    prototype: Value,
    length: f64,
    name: Value,
    name_atom: Atom,
}

/// The arguments a forwarded call passes on (`Vm::forwarded_call`).
pub(super) trait ForwardedArgs {
    /// Removes the first argument, `undefined` when there is none.
    fn shift(&mut self) -> Value;
    /// Puts `bound` before the arguments; `false` when this list cannot hold them.
    fn prepend(&mut self, bound: &[Value]) -> bool;
}

impl ForwardedArgs for &[Value] {
    fn shift(&mut self) -> Value {
        let first = self.first().copied().unwrap_or(Value::UNDEFINED);
        *self = self.get(1..).unwrap_or_default();
        first
    }
    fn prepend(&mut self, bound: &[Value]) -> bool {
        bound.is_empty()
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

impl<H: Host> Vm<H> {
    pub(super) fn bind_function(
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
        let bound_args = self.alloc_bound_arguments(args);
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
            let function = self.heap.alloc_function(FunctionRecord { object, realm, kind: FunctionKind::Native(Native::FunctionBoundCall), env });
            self.capture_function_name_for_atom(function, name_atom);
            return Ok(function);
        }
        self.bind_function_unshaped(BoundParts { target, this, args: bound_args, prototype, length, name, name_atom })
    }

    /// The bound arguments array of `args` past the target: a fresh array of their values.
    fn alloc_bound_arguments(&mut self, args: &[Value]) -> Value {
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                args.get(1..).unwrap_or_default().to_vec(),
            ),
        })
    }

    /// A bound function made with its own environment and properties, which records the shapes
    /// the next bound functions reuse (`bound_function_shape`).
    fn bind_function_unshaped(&mut self, parts: BoundParts) -> Result<Value, JsError> {
        let BoundParts { target, this, args, prototype, length, name, name_atom } = parts;
        let env = self.object();
        self.set_property(env, self.runtime_atoms.bound_target, target)?;
        self.set_property(env, self.runtime_atoms.bound_this, this)?;
        self.set_property(env, self.runtime_atoms.bound_args, args)?;
        let function = self.native_with_env(Native::FunctionBoundCall, env);
        self.object_data_mut(function).expect("bound function has object storage").set_proto(prototype);
        let length_atom = self.length_atom;
        self.set_property_with_attributes(function, length_atom, Value::number(length), BOUND_FUNCTION_NAMED_ATTRIBUTES)?;
        self.set_property_with_attributes(function, self.runtime_atoms.name, name, BOUND_FUNCTION_NAMED_ATTRIBUTES)?;
        self.capture_function_name_for_atom(function, name_atom);
        let shape_of = |vm: &Self, object: Value| vm.object_data(object).map(crate::heap::Object::shape).filter(|&shape| !vm.shape_is_dictionary(shape));
        self.bound_function_shape = shape_of(self, function).zip(shape_of(self, env));
        Ok(function)
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
        let mut combined = bound_args.to_vec();
        combined.extend_from_slice(args);
        self.call_value(p, target, receiver, &combined)
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
    pub(super) fn forwarded_call<A: ForwardedArgs>(
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
    /// (`lane_function_call`): `f.call(thisArg, ...args)`, a bound function without bound arguments.
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
    /// The slots of a bound function's target, `this` and arguments in an environment of
    /// `shape`, when the shape holds them as plain data: recorded for the next environment of
    /// the shape.
    fn bound_parts_slots(&self, shape: u32) -> Option<[u16; 3]> {
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

    pub(super) fn bound_function_parts(&self, env: Value) -> Option<(Value, Value, std::borrow::Cow<'_, [Value]>)> {
        let data = self.object_data(env)?;
        let shape = data.shape();
        // The environment's three slots, read through the layout its shape keeps for every
        // bound function (`bound_parts_layout`), found by name once per shape.
        let slots = match self.bound_parts_layout.get() {
            Some((layout, slots)) if layout == shape => Some(slots),
            _ => self.bound_parts_slots(shape),
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
        let arguments = arguments
            .and_then(|list| self.heap.array_elements(list))
            .map_or(std::borrow::Cow::Borrowed(&[][..]), |elements| elements.as_slice());
        Some((target, receiver, arguments))
    }
}
