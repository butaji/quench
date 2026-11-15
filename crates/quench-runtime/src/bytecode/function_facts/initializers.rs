//! The stores a class initializer makes, and the forwarded constructions and argument passing a function's code is shaped as.

use super::super::*;

impl Function {
    /// The stores of a body whose whole effect is storing parameters and primitive constants
    /// into fields of `this`, then returning `undefined`, in order; `None` for any other body.
    /// A constant store is the `SetThisField` of a temporary a primitive constant's
    /// `LoadConst` wrote, which writes nothing else. A parameter store's value is a parameter as passed (`SetThisField` of
    /// the parameter's register), or a Number
    /// constant in its place where the parameter is falsy: `this.f = (p ? p : k)` (a
    /// `JumpFalse` of the parameter over its `Move` into a temporary, else the constant's
    /// `LoadConst` into it, then the temporary's `SetThisField`) or `if (!p) p = k` before
    /// the parameter's stores (a `JumpTrue` of the parameter over the constant's `LoadConst`
    /// into it). The body completes with the return of an undefined constant or of a
    /// register nothing writes. In a plain activation, where parameter `k` lives in
    /// register `k` (`CallPlan::registers_only`), these stores are its whole effect.
    pub(super) fn derive_initializer_stores(&self, constants: &[Constant]) -> Option<Box<[InitializerStore]>> {
        if !self.handlers.is_empty() {
            return None;
        }
        let instructions = self.instructions().collect::<Vec<_>>();
        let number = |load: &WideInstruction| match constants.get(load.constant_index()) {
            Some(Constant::Number(number)) if load.op() == Op::LoadConst => Some(*number),
            _ => None,
        };
        let parameter = |register: Register| (register < self.params).then_some(register);
        let mut fallbacks: SmallVec<[Option<f64>; 4]> = smallvec::smallvec![None; usize::from(self.params)];
        let mut stores = Vec::new();
        let mut temporaries = initializer_stores::Temporaries::default();
        let mut pc = 0;
        let store = |instruction: &WideInstruction| instruction.stores_to_this().then_some(());
        loop {
            let at = |offset: usize| instructions.get(pc + offset);
            let target = |offset: usize| (pc + offset) as u32;
            match at(0)? {
                // `if (!p) p = k`, before any store of `p`.
                jump if jump.op() == Op::JumpTrue
                    && let Some(p) = parameter(jump.register_a())
                    && jump.jump_target() == target(2)
                    && let Some(load) = at(1)
                    && load.result_register() == p
                    && let Some(k) = number(load)
                    && fallbacks[usize::from(p)].is_none()
                    && !stores.iter().any(|stored: &InitializerStore| stored.parameter() == Some(p)) =>
                {
                    fallbacks[usize::from(p)] = Some(k);
                    pc += 2;
                }
                // `this.f = (p ? p : k)`.
                jump if jump.op() == Op::JumpFalse
                    && let Some(p) = parameter(jump.register_a())
                    && jump.jump_target() == target(3)
                    && let (Some(copy), Some(skip), Some(load), Some(set)) = (at(1), at(2), at(3), at(4))
                    && copy.op() == Op::Move
                    && copy.register_b() == p
                    && copy.result_register() >= self.params
                    && skip.op() == Op::Jump
                    && skip.jump_target() == target(4)
                    && load.result_register() == copy.result_register()
                    && let Some(k) = number(load)
                    && store(set).is_some()
                    && set.register_a() == copy.result_register()
                    && fallbacks[usize::from(p)].is_none() =>
                {
                    stores.push(InitializerStore {
                        atom: set.imm(),
                        site: set.c(),
                        value: InitializerValue::Parameter { parameter: p, fallback: Some(k) },
                    });
                    pc += 5;
                }
                set if store(set).is_some() && let Some(p) = parameter(set.register_a()) => {
                    stores.push(InitializerStore {
                        atom: set.imm(),
                        site: set.c(),
                        value: InitializerValue::Parameter { parameter: p, fallback: fallbacks[usize::from(p)] },
                    });
                    pc += 1;
                }
                // `this.f = k` or `this.f = p op k` of a temporary (`Temporaries::value`).
                set if store(set).is_some() && let Some(value) = temporaries.value(set.register_a()) => {
                    stores.push(InitializerStore { atom: set.imm(), site: set.c(), value });
                    pc += 1;
                }
                _ if let Some(length) = temporaries.consume(&instructions, pc, (self.params, &fallbacks, constants), &mut stores) => pc += length,
                _ => break,
            }
        }
        let undefined_load = |load: &WideInstruction| {
            load.op() == Op::LoadConst && matches!(constants.get(load.constant_index()), Some(Constant::Undefined))
        };
        let completes = match &instructions[pc..] {
            [completion] => {
                (undefined_load(completion) && completion.returns_from_frame())
                    || (completion.op() == Op::Return
                        && completion.register_a() >= self.params
                        && match temporaries.value(completion.register_a()) {
                            Some(value) => value == InitializerValue::Constant(InitializerConstant::Undefined),
                            None => self.initial_register == InitialRegister::Undefined,
                        })
            }
            [load, completion] => {
                undefined_load(load)
                    && !load.returns_from_frame()
                    && completion.op() == Op::Return
                    && completion.register_a() == load.result_register()
            }
            _ => false,
        };
        // A defaulted parameter none of the stores reads is a store the model leaves out.
        let all_read = fallbacks.iter().enumerate().all(|(p, fallback)| {
            fallback.is_none() || stores.iter().any(|stored: &InitializerStore| stored.parameter().map(usize::from) == Some(p))
        });
        (completes && all_read && !stores.is_empty()).then(|| stores.into_boxed_slice())
    }

    /// Whether the body reaches its `arguments` binding only by reading its length or an
    /// element (`ArgumentsLength`, `ArgumentGet`), with no closure capturing the binding, so
    /// the values passed and the parameters answer every read: an element a parameter
    /// aliases (`arguments_alias_parameters`) is that parameter's local, which no closure
    /// captures, and any other the value passed. An environment activation then keeps those
    /// values after its locals.
    pub(crate) fn reads_passed_arguments_only(&self) -> bool {
        let Some(slot) = self.arguments_slot else {
            return false;
        };
        !self.is_generator
            && !self.is_async
            && (!self.arguments_alias_parameters()
                || (0..usize::from(self.params)).all(|parameter| !self.local_slot_uses_environment(parameter)))
            && !self.local_slot_uses_environment(usize::from(slot))
            && !self.has_dynamic_scope_access()
            && self.instructions().all(|instruction| {
                instruction.op().reads_arguments()
                    || !instruction
                        .addressed_local_slots()
                        .any(|addressed| addressed == slot)
            })
    }

    /// What a derived constructor whose whole body constructs its parent and returns the
    /// receiver passes on (`SuperForwarded`): the implicit `constructor(...args) {
    /// super(...args); }`, or `constructor(a, b) { super(a, b); }` with its parameters in
    /// order. Each reads the active function's parent (`SUPER_BASE_BINDING` of the
    /// `\0quench:super` binding) and makes the `super` construction, then binds the
    /// receiver. Without fields or private elements of its own (`instance_initializer`), its
    /// construction is the parent's with the class as `new.target`.
    pub(crate) fn super_forwarding(&self, atoms: &AtomTable, constants: &[Constant]) -> Option<SuperForwarded> {
        if !self.derived_constructor
            || !self.handlers.is_empty()
            || self.arguments_slot.is_some()
            || self.instance_initializer.is_some()
            || self.is_async
            || self.is_generator
        {
            return None;
        }
        // Each shape has a fixed number of records, so the record count rejects any other
        // body before a record decodes: the implicit constructor reads its first eight (its
        // tail is unreachable), the parameter form is exactly `FORWARDED_PARAMETERS_FRAME`
        // around its parameter loads. The rewrite may also leave the active function's copy
        // into the parent call's window as a `Move` right before the call
        // (`FORWARDED_WINDOW_COPY`).
        let shaped_records = if self.rest {
            FORWARDED_REST_RECORDS
        } else {
            FORWARDED_PARAMETERS_FRAME + usize::from(self.params)
        };
        let fits = |count: usize| {
            if self.rest {
                count >= shaped_records
            } else {
                count == shaped_records || count == shaped_records + FORWARDED_WINDOW_COPY
            }
        };
        if !fits(self.code.len()) {
            return None;
        }
        let mut records = self.instructions().take(shaped_records + FORWARDED_WINDOW_COPY).collect::<SmallVec<[WideInstruction; 16]>>();
        // The copy, dropped here: the call's window is that copy.
        let copied = records.iter().position(|record| record.op() == Op::Call).and_then(|call| {
            let moved = *records.get(call.checked_sub(1)?)?;
            (moved.op() == Op::Move && moved.result_flags() == 0).then(|| {
                records.remove(call - 1);
                (moved.register_b(), moved.result_register())
            })
        });
        // A parameter form's every record is read: one longer only by the dropped copy.
        let read = self.code.len() - usize::from(copied.is_some());
        if records.len() < shaped_records || (!self.rest && read != shaped_records) {
            return None;
        }
        records.truncate(shaped_records);
        let parent_of = |active: &WideInstruction, base: &WideInstruction, receiver: &WideInstruction, call: &WideInstruction| {
            super_initializer::calls_super_base(atoms, constants, [active, base, receiver, call], forwarded_window(copied, active))
        };
        if self.rest {
            // The implicit constructor spreads its rest list, then returns the receiver; the
            // records past its return are unreachable.
            return spreads_rest_list_to_super(&records, parent_of, self.params).then_some(SuperForwarded::Passed);
        }
        if !self.simple_parameters {
            return None;
        }
        let count = usize::from(self.params);
        let (head, tail) = records.split_at_checked(5)?;
        let (loads, tail) = tail.split_at_checked(count)?;
        let ([active, base, receiver, parent, spare], [construct, check, binds, undefined, completion]) =
            (head, tail)
        else {
            return None;
        };
        let undefined_load = |load: &WideInstruction| {
            load.op() == Op::LoadConst
                && load.result_flags() == 0
                && matches!(constants.get(load.constant_index()), Some(Constant::Undefined))
        };
        if construct.op() != Op::Construct {
            return None;
        }
        let crate::bytecode::ConstructArguments::Registers(window) = construct.construct_arguments() else {
            return None;
        };
        let forwards_parameters = usize::from(window.count) == count
            && loads.iter().enumerate().all(|(index, load)| {
                load.op() == Op::LoadLocalPlain
                    && load.result_flags() == 0
                    && load.local_slot() == index
                    && usize::from(load.result_register()) == usize::from(window.base) + index
            });
        let shaped = parent_of(active, base, receiver, parent)
            && undefined_load(spare)
            && construct.is_super_construct()
            && construct.result_flags() == 0
            && construct.b() == parent.result_register()
            && forwards_parameters
            && check.op() == Op::SuperCallCheck
            && binds.op() == Op::InitializeThis
            && binds.a() == construct.result_register()
            && undefined_load(undefined)
            && completion.op() == Op::Return
            && completion.register_a() == undefined.result_register();
        shaped.then_some(SuperForwarded::Parameters(self.params))
    }

    /// The method a constructor body forwards its construction to, when the body is exactly
    /// `this.<method>.apply(this, arguments)` and the return of `undefined` (Prototype's
    /// `Class.create` constructor): its seven records read `this.<method>`, read `apply` from
    /// it, load `this` and the forwarded `arguments` binding into the call window, make the
    /// forwarding call, then return `undefined`. No handler covers them.
    pub(crate) fn forwarded_this_method(&self, constants: &[Constant]) -> Option<ForwardedThisMethod> {
        if !self.handlers.is_empty() {
            return None;
        }
        let arguments_slot = self.arguments_slot?;
        let records = self.instructions().collect::<SmallVec<[WideInstruction; 8]>>();
        let [method, apply, this, arguments, call, undefined, completion] = records.as_slice() else {
            return None;
        };
        let window = ImmediateLayout::call_window_base(call.imm());
        let plain_result = |record: &WideInstruction| record.result_flags() == 0;
        let shaped = method.op() == Op::GetField
            && method.b() == FieldBase::THIS.0
            && plain_result(method)
            && apply.op() == Op::GetField
            && apply.b() == method.result_register()
            && plain_result(apply)
            && this.op() == Op::LoadThis
            && plain_result(this)
            && this.result_register() == window
            && arguments.op() == Op::LoadLocalPlain
            && plain_result(arguments)
            && arguments.result_register() == window + 1
            && arguments.local_slot() == usize::from(arguments_slot)
            && call.op() == Op::CallForwardingArguments
            && call.b() == apply.result_register()
            && call.c() == method.result_register()
            && ImmediateLayout::argument_count(call.imm()) == FORWARDED_APPLY_ARGUMENTS
            && plain_result(call)
            && undefined.op() == Op::LoadConst
            && plain_result(undefined)
            && matches!(constants.get(undefined.constant_index()), Some(Constant::Undefined))
            && completion.op() == Op::Return
            && completion.register_a() == undefined.result_register();
        shaped.then(|| ForwardedThisMethod {
            method: (method.imm(), method.c()),
            apply: (apply.imm(), apply.c()),
            call_pc: FORWARDED_THIS_METHOD_CALL,
            call_result: call.a(),
        })
    }

    /// How an activation answers every read of the `arguments` binding from the values it
    /// was passed, if it can: each read loads it into the last argument of the
    /// `CallForwardingArguments` right after, reads its length or an element
    /// (`ArgumentsLength`, `ArgumentGet`), or, for `Passes`, loads the object as a value
    /// where no read of the binding follows (`arguments_passed_last`). Nothing else can
    /// reach the binding: no closure, environment clone, dynamic scope access or promoted
    /// register, and no suspension. An object it later materializes must equal one made on
    /// entry: unmapped, or mapped onto no parameters.
    pub(crate) fn kept_arguments(&self) -> Option<KeptArguments> {
        let slot = self.arguments_slot?;
        let instructions = self.instructions().collect::<Vec<_>>();
        // A mapped object's elements alias the parameters; its `length` does not, so a body
        // that reads only `arguments.length` cannot observe the mapping.
        let reads_only_length = instructions.iter().all(|instruction| {
            instruction.op() == Op::ArgumentsLength
                || !instruction.addressed_local_slots().any(|addressed| addressed == slot)
        });
        if self.is_generator
            || self.is_async
            || (self.arguments_alias_parameters() && self.params != 0 && !reads_only_length)
            || self.local_registers.iter().any(|entry| entry.local == slot)
            || self.has_dynamic_scope_access()
        {
            return None;
        }
        let mut forwarded = false;
        let mut passes = SmallVec::<[usize; 2]>::new();
        for (pc, instruction) in instructions.iter().enumerate() {
            if instruction.op().captures_environment() {
                return None;
            }
            if !instruction
                .addressed_local_slots()
                .any(|addressed| addressed == slot)
            {
                continue;
            }
            let loads = instruction.op().loads_frame_local();
            let feeds_forwarding_call = loads
                && instructions.get(pc + 1).is_some_and(|next| {
                    next.op() == Op::CallForwardingArguments
                        && next.call_window().last() == Some(instruction.result_register())
                });
            let reads_kept_values =
                instruction.op().reads_arguments();
            if feeds_forwarding_call || reads_kept_values {
                forwarded = true;
            } else if loads {
                passes.push(pc);
            } else {
                return None;
            }
        }
        if passes.is_empty() {
            return forwarded.then_some(KeptArguments::Forwards);
        }
        self.arguments_passed_last(&instructions, slot, &passes).then_some(KeptArguments::Passes)
    }

    /// Whether no read of the `arguments` binding in local `slot`, `passes` among them, can
    /// follow any of `passes`: the object a pass makes is the binding's last read, so no
    /// later read needs it. A handler's edges are not traced, so a body with one answers
    /// `false`.
    pub(super) fn arguments_passed_last(&self, instructions: &[WideInstruction], slot: u16, passes: &[usize]) -> bool {
        if !self.handlers.is_empty() {
            return false;
        }
        let mut reached = crate::bitset::BitSet::new(instructions.len());
        let mut work = Vec::new();
        for &pass in passes {
            crate::bytecode::for_each_instruction_successor(pass, instructions[pass], |next| work.push(next));
        }
        while let Some(pc) = work.pop() {
            let Some(instruction) = instructions.get(pc) else {
                continue;
            };
            if !reached.insert(pc) {
                continue;
            }
            if instruction.addressed_local_slots().any(|addressed| addressed == slot) {
                return false;
            }
            crate::bytecode::for_each_instruction_successor(pc, *instruction, |next| work.push(next));
        }
        true
    }
}

/// The register a forwarded parent call's window is read from: the active function's result,
/// or the window the dropped copy moved it into.
fn forwarded_window(copied: Option<(Register, Register)>, active: &WideInstruction) -> Option<Register> {
    match copied {
        None => Some(active.result_register()),
        Some((source, window)) => (source == active.result_register()).then_some(window),
    }
}

/// Whether the rest form's records (from its list load on) spread a rest list of `params`
/// parameters into the parent's construction, then return the receiver.
fn spreads_rest_list_to_super(
    records: &[WideInstruction],
    parent_of: impl Fn(&WideInstruction, &WideInstruction, &WideInstruction, &WideInstruction) -> bool,
    params: u16,
) -> bool {
    let [list, active, base, receiver, call, construct, binds, completion, ..] = records else {
        return false;
    };
    params == 1
        && list.op() == Op::LoadLocalPlain
        && list.local_slot() == 0
        && parent_of(active, base, receiver, call)
        && spread_super_construct(construct, call, list)
        && returns_constructed_receiver(binds, completion, construct)
}

/// Whether `construct` is the super construction of `call`'s result, its arguments the rest
/// list `list` spread.
fn spread_super_construct(construct: &WideInstruction, call: &WideInstruction, list: &WideInstruction) -> bool {
    construct.op() == Op::Construct
        && construct.is_super_construct()
        && construct.result_flags() == 0
        && construct.b() == call.result_register()
        && matches!(
            construct.construct_arguments(),
            crate::bytecode::ConstructArguments::Array(spread) if spread == list.result_register()
        )
}

/// Whether `binds` binds the receiver to the result of `construct`, and `completion` returns it.
fn returns_constructed_receiver(binds: &WideInstruction, completion: &WideInstruction, construct: &WideInstruction) -> bool {
    binds.op() == Op::InitializeThis
        && binds.a() == construct.result_register()
        && completion.op() == Op::Return
        && completion.register_a() == construct.result_register()
}
