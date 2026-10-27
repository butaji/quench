use super::*;
use crate::heap::PrivateBrand;

// Publish the current resume PC only while a binding-site consumer runs.
macro_rules! with_binding_site_pc {
    ($vm:expr, $frame:expr, $resume_pc:expr, $action:expr) => {{
        let previous = std::mem::replace(
            &mut $vm.frames[$frame].binding_site_pc,
            Some($resume_pc as u32),
        );
        let result = $action;
        $vm.frames[$frame].binding_site_pc = previous;
        result
    }};
}

impl<H: Host> Vm<H> {
    pub(super) fn module_import_value(
        &self,
        program: ProgramId,
        function: u32,
        slot: usize,
    ) -> Option<Value> {
        if function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let slot = u16::try_from(slot).ok()?;
        match self.programs.module_import(program, slot)? {
            ModuleImport::Value(value) => Some(value),
            ModuleImport::Binding(program, binding, fallback) => {
                let Some(environment) = self.programs.module_environment(program) else {
                    return Some(fallback);
                };
                let value = self
                    .heap
                    .environment_slot(environment, usize::from(binding));
                Some(match value {
                    Some(Value::DELETED) if !fallback.is_undefined() => fallback,
                    None => fallback,
                    Some(value) => value,
                })
            }
        }
    }

    /// Starts a plain or array-spread call of an eligible same-realm user function as an
    /// activation of the running dispatch loop, or returns `None` to leave it to `step`. Such a
    /// call involves no direct eval (its callee is not the eval intrinsic) and neither eval mode
    /// is set before it, so the frame push leaves both untouched.
    #[inline(always)]
    pub(super) fn try_inline_user_call(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
    ) -> Option<Result<StepResult, JsError>> {
        // A sloppy call that falls into a return of its result is pushed like any other; its
        // caller then returns the callee's completion (see `PendingGeneralCall::returns`).
        // Hosts permitting proper tail calls leave strict terminal calls to `step`. Wasm programs run in
        // the loop (as in-loop Wasm frames), and their calls keep Wasm's own activation and
        // exhaustion accounting, so they are never inlined as user calls.
        if !matches!(i.op(), Op::Call | Op::CallArray)
            || self.direct_eval
            || self.parameter_eval
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || f + 1 != self.frames.len()
            || self.frames[f].function == super::ROOT_FUNCTION_ID
            || (self.host.proper_tail_calls()
                && i.returns_from_frame()
                && p.functions[self.frames[f].function as usize].strict)
        {
            return None;
        }
        let callee = self.read(f, i.register_b());
        let caller_program = self.active_program;
        let (program_id, id, env, target_program) =
            if let Some((id, env)) = self.in_loop_call_target(p, callee) {
                (caller_program, id, env, None)
            } else {
                let (program_id, id, env) = self.same_realm_user_target(callee)?;
                if program_id == caller_program || id == super::ROOT_FUNCTION_ID {
                    return None;
                }
                let target_program = self.programs.get(program_id)?;
                if !target_program
                    .functions
                    .get(id as usize)
                    .is_some_and(|function| function.call_plan(&target_program.atoms).in_loop)
                {
                    return None;
                }
                (program_id, id, env, Some(target_program))
            };
        self.profile.call_source(0);
        let window = i.call_window();
        let this = self.read(f, i.register_c());
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        // Collection happens only at back edges, tail calls and explicit host safepoints, none
        // of which can run while a frame is pushed; afterwards the new frame itself roots the
        // callee, receiver and arguments.
        let spread_arguments;
        let arguments: &[Value] = if i.op() == Op::CallArray {
            let array = self.read(f, window.base);
            spread_arguments = match self.array_values(array) {
                Ok(arguments) => arguments,
                Err(error) => return Some(Err(error)),
            };
            spread_arguments.as_slice()
        } else {
            let arguments: *const [Value] = self.register_window(f, window);
            // SAFETY: see `push_general_user_frame`; the caller's register window outlives
            // callee setup and frame-stack growth moves only its boxed frame pointer.
            unsafe { &*arguments }
        };
        self.profile.call_target(1, arguments.len());
        let previous_program = if program_id != caller_program {
            Some(self.replace_active_program(program_id))
        } else {
            None
        };
        let call_guard = self.push_general_user_frame(
            target_program.as_deref().unwrap_or(p),
            id,
            env,
            this,
            arguments,
            CallContext::user_function(id, callee),
        );
        if call_guard.is_err()
            && let Some(previous_program) = previous_program
        {
            self.set_active_program(previous_program);
        }
        Some(call_guard.map(|call_guard| StepResult::PushFrame {
            destination: i.result_register(),
            call_guard,
            construct_this: None,
        }))
    }

    /// The function and environment of `callee` when it is a user function of the active
    /// program in the current realm that the loop may push in place: a non-root ordinary
    /// body (`CallPlan::in_loop`). Resolved targets are cached (`InLoopCallTarget`).
    #[inline(always)]
    fn in_loop_call_target(&mut self, p: &ResidualProgram, callee: Value) -> Option<(u32, Value)> {
        let index = callee.heap_index()? as usize & (super::CALL_TARGET_CACHE_SIZE - 1);
        let epoch = self.heap.collection_epoch();
        let cached = self.call_targets[index];
        if cached.callee == callee
            && cached.epoch == epoch
            && cached.realm == self.realm.globals
            && cached.program == self.active_program
        {
            return Some((cached.function, cached.env));
        }
        self.resolve_in_loop_call_target(p, callee, index, epoch)
    }

    #[inline(never)]
    fn resolve_in_loop_call_target(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        index: usize,
        epoch: u64,
    ) -> Option<(u32, Value)> {
        let (program, function, env) = self.same_realm_user_target(callee)?;
        if function == super::ROOT_FUNCTION_ID
            || program != self.active_program
            || !p
                .functions
                .get(function as usize)
                .is_some_and(|function| function.call_plan(&p.atoms).in_loop)
        {
            return None;
        }
        self.call_targets[index] = super::InLoopCallTarget {
            callee,
            epoch,
            realm: self.realm.globals,
            program,
            function,
            env,
        };
        Some((function, env))
    }

    /// `f.call(this, ...args)` and `f.apply(this, list)` of the intrinsic natives invoke `f`
    /// with no observable step of their own, so when `try_inline_user_call` declines a call
    /// whose callee is one of them and `f` is a user function, `f` is pushed directly. It
    /// applies `try_inline_user_call`'s conditions to the call and to `f`; it stays out of
    /// line so ordinary user calls keep their code.
    #[inline(never)]
    pub(super) fn try_inline_call_apply(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
    ) -> Option<Result<StepResult, JsError>> {
        if !matches!(i.op(), Op::Call | Op::CallForwardingArguments)
            || self.direct_eval
            || self.parameter_eval
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || f + 1 != self.frames.len()
            || self.frames[f].function == super::ROOT_FUNCTION_ID
            || (self.host.proper_tail_calls()
                && i.returns_from_frame()
                && p.functions[self.frames[f].function as usize].strict)
        {
            return None;
        }
        let callee = self.read(f, i.register_b());
        let target = self.read(f, i.register_c());
        let window: *const [Value] = self.register_window(f, i.call_window());
        // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
        let window = unsafe { &*window };
        let this = window.first().copied().unwrap_or(Value::UNDEFINED);
        let apply_arguments;
        let arguments = match self.heap.get(callee) {
            Some(Cell::Function {
                kind: FunctionKind::Native(Native::FunctionCall),
                ..
            }) => window.get(1..).unwrap_or_default(),
            Some(Cell::Function {
                kind: FunctionKind::Native(Native::FunctionApply),
                ..
            }) => match window.get(1).copied().unwrap_or(Value::UNDEFINED) {
                // The caller's unmaterialized `arguments`: the values its frame kept.
                Value::UNMATERIALIZED_ARGUMENTS => {
                    let kept: *const [Value] = self.frames[f].original_arguments.as_slice();
                    // SAFETY: the caller's frame stays on the stack and is not mutated while
                    // the callee is set up; its buffer does not move when `frames` grows.
                    unsafe { &*kept }
                }
                list => {
                    apply_arguments = self.dense_apply_arguments(list)?;
                    apply_arguments.as_slice()
                }
            },
            _ => return None,
        };
        let (program_id, id, env) = self.same_realm_user_target(target)?;
        if id == super::ROOT_FUNCTION_ID {
            return None;
        }
        let target_program = if program_id == self.active_program {
            None
        } else {
            Some(self.programs.get(program_id)?)
        };
        let target_residual = target_program.as_deref().unwrap_or(p);
        if !target_residual
            .functions
            .get(id as usize)
            .is_some_and(|function| function.call_plan(&target_residual.atoms).in_loop)
        {
            return None;
        }
        self.profile.call_source(0);
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        self.profile.call_target(1, arguments.len());
        let previous_program = if program_id != self.active_program {
            Some(self.replace_active_program(program_id))
        } else {
            None
        };
        let call_guard = self.push_general_user_frame(
            target_residual,
            id,
            env,
            this,
            arguments,
            CallContext::user_function(id, target),
        );
        if call_guard.is_err()
            && let Some(previous_program) = previous_program
        {
            self.set_active_program(previous_program);
        }
        Some(call_guard.map(|call_guard| StepResult::PushFrame {
            destination: i.result_register(),
            call_guard,
            construct_this: None,
        }))
    }

    /// Pushes `Reflect.apply` of a same-realm user function as a general-loop frame when the
    /// argument list is a plain dense array. Exotic array-likes use the regular algorithm since
    /// reading their length/indexes can invoke guest code.
    #[inline(never)]
    pub(super) fn try_inline_reflect_apply(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
    ) -> Option<Result<StepResult, JsError>> {
        if i.op() != Op::Call
            || self.direct_eval
            || self.parameter_eval
            || p.kind == crate::bytecode::ProgramKind::Wasm
            || f + 1 != self.frames.len()
            || self.frames[f].function == super::ROOT_FUNCTION_ID
            || (self.host.proper_tail_calls()
                && i.returns_from_frame()
                && p.functions[self.frames[f].function as usize].strict)
        {
            return None;
        }
        let window: *const [Value] = self.register_window(f, i.call_window());
        // SAFETY: the caller stays active while the callee frame is initialized.
        let window = unsafe { &*window };
        let native = self.read(f, i.register_b());
        if !matches!(
            self.heap.get(native),
            Some(Cell::Function {
                kind: FunctionKind::Native(Native::ReflectApply),
                ..
            })
        ) {
            return None;
        }
        let target = window.first().copied().unwrap_or(Value::UNDEFINED);
        let this = window.get(1).copied().unwrap_or(Value::UNDEFINED);
        let list = window.get(2).copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(target)
            || (!list.is_null() && !list.is_undefined() && !self.is_object_like(list))
        {
            return None;
        }
        let arguments = self.dense_apply_arguments(list)?;
        let (program_id, id, env) = self.same_realm_user_target(target)?;
        self.profile.call_source(0);
        self.try_push_reflect_apply_target(f, i, pc, target, this, program_id, id, env, arguments)
    }

    /// Shared target validation/frame push for `Reflect.apply`, also used when a cached
    /// `Reflect.apply(...)` method call is represented by `CallMethod` rather than `Call`.
    #[inline(never)]
    pub(super) fn try_push_reflect_apply_target(
        &mut self,
        f: usize,
        i: WideInstruction,
        pc: usize,
        target: Value,
        this: Value,
        program_id: super::ProgramId,
        id: u32,
        env: Value,
        arguments: Vec<Value>,
    ) -> Option<Result<StepResult, JsError>> {
        if id == super::ROOT_FUNCTION_ID {
            return None;
        }
        let target_program = self.programs.get(program_id)?;
        if !target_program
            .functions
            .get(id as usize)
            .is_some_and(|function| function.call_plan(&target_program.atoms).in_loop)
        {
            return None;
        }

        self.profile.call_target(1, arguments.len());
        self.frames[f].pc = pc;
        self.frames[f].pc_is_resume = true;
        let previous_program = self.replace_active_program(program_id);
        let pushed = self.push_general_user_frame(
            &target_program,
            id,
            env,
            this,
            &arguments,
            CallContext::user_function(id, target),
        );
        if pushed.is_err() {
            self.set_active_program(previous_program);
        }
        Some(pushed.map(|call_guard| StepResult::PushFrame {
            destination: i.result_register(),
            call_guard,
            construct_this: None,
        }))
    }

    /// `CallForwardingArguments` whose argument is the caller's unmaterialized `arguments`:
    /// the intrinsic `apply` of an in-loop user function is pushed with the kept values;
    /// any other call first materializes the object, then proceeds as `Call` (`None`).
    #[inline(never)]
    fn forward_activation_arguments(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: usize,
        allow_inline_calls: bool,
    ) -> Option<Result<StepResult, JsError>> {
        let last = i.call_window().last()?;
        if self.read(f, last) != Value::UNMATERIALIZED_ARGUMENTS {
            return None;
        }
        if allow_inline_calls && let Some(result) = self.try_inline_call_apply(p, f, i, pc) {
            return Some(result);
        }
        match self.materialize_activation_arguments(p, f) {
            Ok(arguments) => {
                self.write(f, last, arguments);
                None
            }
            Err(error) => Some(Err(error)),
        }
    }

    /// CreateListFromArrayLike for `apply` when it reduces to a copy: null or undefined, a
    /// plain array without holes, or an arguments object whose data `length` covers a
    /// hole-free prefix, neither carrying indexed descriptors or sparse elements. `None`
    /// leaves the list to the generic algorithm, whose reads may run guest code.
    pub(super) fn dense_apply_arguments(&self, list: Value) -> Option<Vec<Value>> {
        if list.is_null() || list.is_undefined() {
            return Some(Vec::new());
        }
        let Some(object) = self.heap.array(list) else {
            return None;
        };
        if object.has_indexed_descriptors() || self.heap.sparse_length(list).is_some() {
            return None;
        }
        let elements = object.elements();
        let length = if object.is_arguments_object() {
            let attributes =
                self.property_attributes(list, PropertyKey::string(self.length_atom))?;
            if attributes.accessor {
                return None;
            }
            usize::try_from(self.own_property(list, self.length_atom)?.as_int()?).ok()?
        } else {
            elements.len()
        };
        let values = elements.get(..length)?;
        (!values.iter().any(|value| value.is_deleted())).then(|| values.to_vec())
    }

    #[inline(always)]
    pub(super) fn step(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
        allow_inline_calls: bool,
    ) -> Result<StepResult, JsError> {
        match i.op() {
            Op::Nop => {}
            Op::CloneEnv => {
                let plan = &p.functions[self.frames[f].function as usize].environment_clones
                    [i.environment_clone_index()];
                self.clone_frame_environment(f, &plan.fresh, &plan.reset)?;
            }
            Op::Wide => unreachable!("validated dispatch cannot contain nested wide instruction"),
            Op::LoadConst => {
                debug_assert_eq!(self.frames[f].program, self.active_program);
                let value = self.active_constant(i.constant_index()).ok_or_else(|| {
                    JsError::validation("constant index is outside program".into())
                })?;
                self.write(f, i.result_register(), value);
            }
            Op::CreateRegExpLiteral => {
                let value = self.regexp_literal(p, f, i.regexp_literal_site_index())?;
                self.write(f, i.result_register(), value);
            }
            Op::LoadLocalPlain => {
                // SAFETY: validated bytecode bounds the slot by Function.locals,
                // and frame setup sizes locals to that count.
                let value = unsafe { self.read_validated_local(f, i.local_slot()) };
                self.write(f, i.result_register(), value);
            }
            Op::LoadLocal | Op::LoadEnvLocal => {
                let value = self.load_local_binding(p, f, i.local_slot(), None)?;
                self.write(f, i.result_register(), value);
            }
            Op::StoreVarBinding => {
                self.store_var_binding(p, f, i.local_slot(), self.read(f, i.register_a()))?;
            }
            Op::StoreLocal => {
                let value = self.read(f, i.register_a());
                let slot = i.local_slot();
                let function = &p.functions[self.frames[f].function as usize];
                if function.is_self_binding_slot(slot) {
                    if function.strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    if let Some(register) = i.optional_register_b() {
                        self.write(f, register, value);
                    }
                    return Ok(StepResult::Continue);
                }
                self.check_local_assignment_initialized(
                    p,
                    f,
                    slot,
                    i.boolean_field(crate::bytecode::InstructionField::C)
                        .unwrap_or(false),
                )?;
                if self.frames[f].function == 0 {
                    if function
                        .local_atoms
                        .get(slot)
                        .is_some_and(|atom| function.global_immutable_atoms.contains(atom))
                        && i.boolean_field(crate::bytecode::InstructionField::C) == Some(false)
                    {
                        return Err(self.type_error(p, "assignment to constant binding".into()));
                    }
                }
                *self
                    .frame_binding_mut(p, f, slot)
                    .ok_or_else(|| JsError("invalid local slot".into()))? = value;
                self.mirror_global_lexical_binding(p, f, slot, value);
                self.mapped_argument_store(p, f, slot, value);
                if let Some(register) = i.optional_register_b() {
                    self.write(f, register, value);
                }
            }
            Op::StoreLocalPlain => {
                let value = self.read(f, i.register_a());
                // SAFETY: validated bytecode bounds the slot by Function.locals,
                // and frame setup sizes locals to that count.
                unsafe { self.write_validated_local(f, i.local_slot(), value) };
                if let Some(register) = i.optional_register_b() {
                    self.write(f, register, value);
                }
            }
            Op::StoreEnvLocal => {
                let value = self.read(f, i.register_a());
                let slot = i.local_slot();
                let function = &p.functions[self.frames[f].function as usize];
                let atom = function.local_atoms.get(slot).copied();
                if let Some(atom) = atom
                    && self.store_with_binding(p, f, atom, value, function.strict)?
                {
                    if let Some(register) = i.optional_register_b() {
                        self.write(f, register, value);
                    }
                    return Ok(StepResult::Continue);
                }
                self.check_local_assignment_initialized(
                    p,
                    f,
                    slot,
                    i.boolean_field(crate::bytecode::InstructionField::C)
                        .unwrap_or(false),
                )?;
                if let Some(atom) = atom
                    && function.global_immutable_atoms.contains(&atom)
                    && !i
                        .boolean_field(crate::bytecode::InstructionField::C)
                        .unwrap_or(false)
                {
                    return Err(self.type_error(p, "assignment to constant binding".into()));
                }
                if function.is_self_binding_slot(slot) {
                    if function.strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    return Ok(StepResult::Continue);
                }
                let global_var = self.root_global_var_atom(
                    p,
                    self.frames[f].program,
                    self.frames[f].function,
                    slot,
                );
                if let Some(atom) = global_var {
                    let written = self.set_property_with_receiver(
                        p,
                        self.realm.globals,
                        atom,
                        value,
                        self.realm.globals,
                    )?;
                    if !written && p.functions[self.frames[f].function as usize].strict {
                        return Err(
                            self.type_error(p, "cannot assign to read-only global binding".into())
                        );
                    }
                    if let Some(register) = i.optional_register_b() {
                        self.write(f, register, value);
                    }
                    return Ok(StepResult::Continue);
                }
                if self.eval_script_context
                    && let Some(atom) =
                        self.root_global_lexical_atom(p, self.frames[f].function, slot)
                {
                    self.realm.global_lexical_bindings.insert(atom, value);
                }
                *self
                    .frame_binding_mut(p, f, slot)
                    .ok_or_else(|| JsError("invalid local environment".into()))? = value;
                self.mapped_argument_store(p, f, slot, value);
                if let Some(register) = i.optional_register_b() {
                    self.write(f, register, value);
                }
            }
            Op::LoadCapture => {
                let v = self.capture(p, f, i.capture_depth(), i.capture_slot())?;
                self.write(f, i.result_register(), v);
            }
            Op::StoreCapture => self.store_capture(
                p,
                f,
                i.capture_depth(),
                i.capture_slot(),
                self.read(f, i.register_a()),
            )?,
            Op::LoadName => {
                let v = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.load_name(p, i.atom_index(), Some(i.cache_site_index()))
                )?;
                self.write(f, i.result_register(), v);
            }
            Op::LoadNameCall => {
                let (callee, this) = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.load_name_call(p, i.atom_index(), Some(i.cache_site_index()), false)
                )?;
                self.write(f, i.result_register(), callee);
                self.write(f, i.register_b(), this);
            }
            Op::LoadNameTypeof => {
                let v = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.load_name_typeof(p, i.atom_index(), i.cache_site_index())
                )?;
                self.write(f, i.result_register(), v);
            }
            Op::StoreName => {
                let value = self.read(f, i.register_a());
                with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.store_name(
                        p,
                        i.atom_index(),
                        value,
                        i.cache_site_index(),
                        i.boolean_field(crate::bytecode::InstructionField::B)
                            .expect("validated initialization flag"),
                    )
                )?;
            }
            Op::LoadThis => {
                let this = self.checked_this_binding(p, f)?;
                self.write(f, i.result_register(), this);
            }
            Op::LoadImportMeta => {
                let program = self.frames[f].program;
                let import_meta = if let Some(value) = self.programs.import_meta(program) {
                    value
                } else {
                    let value = self
                        .heap
                        .alloc(Cell::Object(Self::empty_object(Value::NULL)));
                    if let Some(source_name) = self.programs.source_name(program).map(str::to_owned)
                    {
                        let url = self.host.import_meta_url(&source_name);
                        let url = self.heap.alloc(Cell::String(url.into()));
                        let url_atom = self.intern_atom("url");
                        self.set_property(value, url_atom, url)?;
                        if let Some(filename) = self.host.import_meta_filename(&source_name) {
                            let filename_value =
                                self.heap.alloc(Cell::String(filename.clone().into()));
                            let filename_atom = self.intern_atom("filename");
                            self.set_property(value, filename_atom, filename_value)?;
                            let dirname = std::path::Path::new(&filename)
                                .parent()
                                .and_then(|path| path.to_str());
                            if let Some(dirname) = dirname {
                                let dirname_value =
                                    self.heap.alloc(Cell::String(dirname.to_owned().into()));
                                let dirname_atom = self.intern_atom("dirname");
                                self.set_property(value, dirname_atom, dirname_value)?;
                            }
                        }
                        {
                            let mut context = crate::NativeContext::new(self);
                            let import_meta = context.scoped_value(value);
                            H::initialize_import_meta(
                                &mut context,
                                &source_name,
                                import_meta,
                            )
                            .map_err(|error| context.finish_error(error))?;
                        }
                    }
                    self.programs.set_import_meta(program, value);
                    value
                };
                self.write(f, i.result_register(), import_meta);
            }
            Op::InitializeThis => {
                let value = self.read(f, i.register_a());
                self.initialize_this_binding(p, f, value)?;
            }
            Op::CacheTemplateObject => {
                let key = (
                    self.frames[f].program,
                    self.frames[f].function,
                    i.template_site_index(),
                );
                let value = if let Some(value) = self.realm.template_objects.get(&key).copied() {
                    value
                } else {
                    let value = self.read(f, i.register_a());
                    self.realm.template_objects.insert(key, value);
                    value
                };
                self.write(f, i.register_a(), value);
            }
            Op::LoadCachedTemplateObject => {
                let key = (
                    self.frames[f].program,
                    self.frames[f].function,
                    i.template_site_index(),
                );
                let value = self
                    .realm
                    .template_objects
                    .get(&key)
                    .copied()
                    .unwrap_or(Value::UNDEFINED);
                self.write(f, i.result_register(), value);
            }
            Op::MakeClosure => {
                let env = with_binding_site_pc!(self, f, *pc, self.capture_binding_environment(f))?;
                let module_root = p.is_module()
                    && self.frames[f].function == super::ROOT_FUNCTION_ID
                    && self.programs.module_environment(self.frames[f].program) == Some(env);
                let v = if module_root {
                    let cached = self
                        .cached_functions_in_environment(
                            self.frames[f].program,
                            i.closure_function_index(),
                            env,
                        )
                        .next();
                    match cached {
                        Some(function) => function,
                        None => self.closure(p, i.closure_function_index(), env)?,
                    }
                } else {
                    self.closure(p, i.closure_function_index(), env)?
                };
                self.write(f, i.result_register(), v);
            }
            Op::MakeObject => {
                let v = self.object();
                self.write(f, i.result_register(), v);
            }
            Op::MakeObject2 => {
                let v = self.object_pair(
                    p,
                    i.object_site_index(),
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                );
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::MakeObjectLiteral => {
                let site = i.object_site_index();
                let shape = self.object_site_shape(p, site);
                let window = i.register_window();
                let start = usize::from(window.base);
                let end = start + usize::from(window.count);
                let prototype = self.object_proto;
                let properties = &self.frames[f].registers[start..end];
                let object = self
                    .heap
                    .alloc_object_with_properties(prototype, shape, properties);
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(object));
                }
                self.write(f, i.result_register(), object);
            }
            Op::SuperConstArrayObject2 => {
                if let Some(value) = self.execute_const_array_object2(p, f, i)? {
                    return Ok(StepResult::Return(value));
                }
            }
            Op::MakeArray => {
                let v = self.heap.alloc(Cell::Array {
                    object: crate::heap::ArrayObject::new(
                        Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                        Rc::new(vec![Value::DELETED; i.array_length()]),
                    ),
                });
                self.write(f, i.result_register(), v);
            }
            Op::MakeConstArray => {
                let start = i.constant_index();
                let end = start + i.element_count() as usize;
                let elements = self
                    .programs
                    .const_array(self.frames[f].program, start, end - start)
                    .ok_or_else(|| {
                        JsError::validation("constant array is outside program".into())
                    })?;
                let v = self.heap.alloc(Cell::Array {
                    object: crate::heap::ArrayObject::new(
                        Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                        elements,
                    ),
                });
                self.write(f, i.result_register(), v);
            }
            Op::GetField => {
                let lookup = i.field_lookup().ok_or_else(|| {
                    JsError::validation("invalid nested field lookup encoding".into())
                })?;
                let v = match lookup {
                    crate::bytecode::FieldLookup::Site(site) => self.resolve_field(p, f, site)?,
                    crate::bytecode::FieldLookup::Atom {
                        atom,
                        base,
                        cache_site,
                    } => {
                        let base = self.resolve_field_base(p, f, base)?;
                        self.get_field_cached(p, base, atom, cache_site)?
                    }
                };
                if i.writes_current_this() {
                    let crate::bytecode::FieldLookup::Site(site) = lookup else {
                        unreachable!("field sink is only attached to nested field lookup")
                    };
                    let sink = p.field_sites[site].sink.expect("fused field sink");
                    self.with_field_source(f, site, crate::bytecode::FieldPart::Sink, |vm| {
                        vm.set_field_cached(
                            p,
                            vm.frames[f].this,
                            sink.0,
                            v,
                            sink.1,
                            p.functions[vm.frames[f].function as usize].strict,
                        )
                    })?;
                }
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::GetIndex => {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_dispatch(false, false);
                let base = self.resolve_operand(p, f, i.operand_b())?;
                let key = self.resolve_operand(p, f, i.operand_c())?;
                let v = self.get_index(p, base, key)?;
                self.write(f, i.result_register(), v);
            }
            Op::CheckPrivate => {
                let object = self.read(f, i.register_a());
                let atom = i.atom_index();
                self.check_private_brand(p, object, atom)?;
                if self.private_field_value(p, object, atom).is_none()
                    && self.own_property(object, atom).is_none()
                    && self.property_accessor(object, atom).is_none()
                {
                    return Err(
                        self.type_error(p, "private member is not present on this object".into())
                    );
                }
            }
            Op::PrivateIn => {
                let object = self.read(f, i.register_b());
                if !self.is_object_like(object) {
                    return Err(
                        self.type_error(p, "right-hand side of 'in' is not an object".into())
                    );
                }
                let result = self.has_private_brand(p, object, i.atom_index());
                self.write(
                    f,
                    i.result_register(),
                    if result { Value::TRUE } else { Value::FALSE },
                );
            }
            Op::MarkPrivateName => {
                let object = self.read(f, i.register_b());
                let home = self.read(f, i.register_c());
                let atom = i.atom_index();
                let brand = PrivateBrand { home, name: atom };
                if object != home {
                    let already_branded = self
                        .object_data(object)
                        .is_some_and(|object| object.has_private_name(brand));
                    if already_branded {
                        return Err(self.type_error(
                            p,
                            "Cannot add private method to an object that already has it".into(),
                        ));
                    }
                }
                if let Some(object) = self.object_data_mut(object) {
                    object.add_private_name(brand);
                }
            }
            Op::ResolveName => {
                let strict = i
                    .boolean_field(crate::bytecode::InstructionField::B)
                    .ok_or_else(|| JsError::validation("invalid resolve-name flag".into()))?;
                let value = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.resolve_name(p, i.atom_index(), strict)
                )?;
                self.write(f, i.result_register(), value);
            }
            Op::LoadResolvedName => {
                let strict = i
                    .boolean_field(crate::bytecode::InstructionField::C)
                    .ok_or_else(|| JsError::validation("invalid resolved-name flag".into()))?;
                let object = self.read(f, i.register_b());
                let value = self.load_resolved_name(p, object, i.atom_index(), strict)?;
                self.write(f, i.result_register(), value);
            }
            Op::ValidateClassHeritage => {
                let heritage = self.read(f, i.register_a());
                if !heritage.is_null() && !self.is_constructable(p, heritage) {
                    return Err(self
                        .type_error(p, "Class extends value is not a constructor or null".into()));
                }
            }
            Op::DeleteName => {
                let value =
                    with_binding_site_pc!(self, f, *pc, self.delete_name(p, i.atom_index()))?;
                self.write(f, i.result_register(), value);
            }
            Op::StoreResolvedName => {
                let strict = i
                    .boolean_field(crate::bytecode::InstructionField::C)
                    .ok_or_else(|| JsError::validation("invalid resolved-name flag".into()))?;
                let object = self.read(f, i.register_b());
                let atom = i.atom_index();
                self.store_resolved_name(p, object, atom, self.read(f, i.register_a()), strict)?;
            }
            Op::ToPropertyKey => {
                let value = self.to_property_key(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::ToNumeric => {
                let value = self.to_numeric(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::CopyDataProperties => self.copy_data_properties(
                p,
                self.read(f, i.register_a()),
                self.read(f, i.register_b()),
                self.read(f, i.register_c()),
            )?,
            Op::GetIterator => {
                let value = self.get_iterator(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::AppendArray => {
                let array = self.read(f, i.register_a());
                self.append_array_literal_holes(array, i.array_length())?;
                if let Some(register) = i.optional_register_b() {
                    let value = self.read(f, register);
                    let spread = i
                        .boolean_field(crate::bytecode::InstructionField::C)
                        .expect("validated boolean field");
                    if spread {
                        self.spread_into_array(p, array, value)?;
                    } else {
                        self.append_array_literal_elements(p, array, [value])?;
                    }
                }
            }
            Op::RequireObjectCoercible => {
                self.require_object_coercible(p, self.read(f, i.register_b()))?;
            }
            Op::RequireIteratorResult => {
                if !self.is_object_like(self.read(f, i.register_b())) {
                    return Err(self.type_error(p, "iterator next result is not an object".into()));
                }
            }
            Op::SuperCallCheck => self.check_super_call(p)?,
            Op::IteratorClose => {
                let result = self.iterator_close(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), result);
            }
            Op::IteratorCleanupPush => self.frames[f].active_iterators.push(ActiveIterator {
                iterator: i.register_a(),
                done: i.register_b(),
            }),
            Op::IteratorCleanupPop => {
                self.frames[f]
                    .active_iterators
                    .pop()
                    .ok_or_else(|| JsError("iterator cleanup stack underflow".into()))?;
            }
            Op::SetFunctionDebugName => {
                self.set_function_debug_name(self.read(f, i.register_a()), i.atom_index());
            }
            Op::SetFunctionName => {
                self.set_function_name(p, self.read(f, i.register_a()), i.atom_index())?;
            }
            Op::SetFunctionNameKey => {
                self.set_function_name_key(
                    self.read(f, i.register_a()),
                    self.read(f, i.register_b()),
                    i.function_name_prefix(),
                );
            }
            Op::InitializeTdz => {
                let slot = i.local_slot();
                *self
                    .frame_binding_mut(p, f, slot)
                    .ok_or_else(|| JsError("invalid local slot".into()))? = Value::DELETED;
            }
            Op::GetAsyncIterator => {
                let value = self.get_async_iterator(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::Await => {
                // Promise conversion is part of this instruction, so abrupt
                // completion reaches the active frame's catch/finally handlers.
                // A primitive needs none: the promise PromiseResolve would wrap it in is
                // already fulfilled and unreachable, so the resume reads the value itself
                // (`enqueue_async_resume`).
                let awaited = self.read(f, i.register_b());
                let value = if self.is_object_like(awaited) {
                    self.promise_for_value(p, awaited)?
                } else {
                    awaited
                };
                return Ok(StepResult::Await {
                    value,
                    destination: i.result_register(),
                });
            }
            Op::Yield => {
                let value = self.read(f, i.register_b());
                let value = if p.functions[self.frames[f].function as usize].is_async {
                    self.promise_for_value(p, value)?
                } else {
                    value
                };
                return Ok(StepResult::Yield {
                    value,
                    destination: i.result_register(),
                    delegated_result: None,
                });
            }
            Op::YieldStar => {
                let iterator = self.read(f, i.register_c());
                let iterator = if iterator.is_undefined() {
                    let source = self.read(f, i.register_b());
                    let asynchronous = p.functions[self.frames[f].function as usize].is_async;
                    let iterator = if asynchronous {
                        self.get_async_iterator(p, source)?
                    } else {
                        self.get_iterator(p, source)?
                    };
                    self.write(f, i.register_c(), iterator);
                    iterator
                } else {
                    iterator
                };
                let (state_register, next_method_register) = i.register_pair();
                let next_method = self.read(f, next_method_register);
                let next_method = if next_method.is_undefined() {
                    let receiver = match self.heap.get(iterator) {
                        Some(Cell::Iterator {
                            source,
                            kind: IteratorKind::AsyncFromSync,
                            ..
                        }) => *source,
                        _ => iterator,
                    };
                    let atom = self.intern_atom("next");
                    let method = self.get_property(p, receiver, atom)?;
                    if !self.is_function(method) {
                        return Err(
                            self.type_error(p, "iterator next method is not callable".into())
                        );
                    }
                    self.write(f, next_method_register, method);
                    method
                } else {
                    next_method
                };
                let awaited_result = self.read(f, state_register);
                let asynchronous = p.functions[self.frames[f].function as usize].is_async;
                let result = if asynchronous && !awaited_result.is_undefined() {
                    self.write(f, state_register, Value::UNDEFINED);
                    awaited_result
                } else {
                    let input = self.read(f, i.result_register());
                    let result =
                        self.iterator_next_with_cached_method(p, iterator, next_method, &[input])?;
                    if asynchronous {
                        *pc -= 1;
                        let value = self.promise_for_value(p, result)?;
                        return Ok(StepResult::Await {
                            value,
                            destination: state_register,
                        });
                    }
                    result
                };
                let done_atom = self.intern_atom("done");
                let done = self.get_property(p, result, done_atom)?;
                if self.truthy(done) {
                    let value_atom = self.intern_atom("value");
                    let value = self.get_property(p, result, value_atom)?;
                    self.write(f, i.result_register(), value);
                } else {
                    *pc -= 1;
                    if asynchronous {
                        self.write(f, state_register, Value::UNDEFINED);
                    }
                    let value = if asynchronous {
                        let value_atom = self.intern_atom("value");
                        self.get_property(p, result, value_atom)?
                    } else {
                        Value::UNDEFINED
                    };
                    return Ok(StepResult::Yield {
                        value,
                        destination: i.result_register(),
                        delegated_result: Some(result),
                    });
                }
            }
            Op::SetField | Op::SetFieldStrict => {
                let object = self.read(f, i.register_b());
                let value = self.read(f, i.register_a());
                self.set_field_cached(
                    p,
                    object,
                    i.atom_index(),
                    value,
                    i.cache_site_index(),
                    i.op() == Op::SetFieldStrict,
                )?;
            }
            Op::DefinePropertyRecord => {
                let mode = i.property_definition_mode().ok_or_else(|| {
                    JsError::validation("invalid property definition mode".into())
                })?;
                let target = self.read(f, i.register_b());
                let key = self.read(f, i.register_c());
                let descriptor =
                    super::object_descriptors::PropertyDescriptorRecord::for_definition(
                        mode,
                        self.read(f, i.register_a()),
                    );
                self.define_property_or_throw(p, target, key, descriptor)?;
            }
            Op::DefinePrivateField => {
                self.initialize_private_field(
                    p,
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                    i.atom_index(),
                    self.read(f, i.register_a()),
                )?;
            }
            Op::DefineField => {
                let object = self.read(f, i.register_b());
                let value = self.read(f, i.register_a());
                self.define_class_field(p, object, PropertyKey::string(i.atom_index()), value)?;
            }
            Op::DefineComputedField => {
                let object = self.read(f, i.register_b());
                let key = self.read(f, i.register_c());
                let key = self.to_property_key(p, key)?;
                let key = match self.heap.get(key).cloned() {
                    Some(Cell::String(text)) => PropertyKey::string(self.intern_js_atom(&text)),
                    Some(Cell::Symbol(_)) => PropertyKey::symbol(key),
                    _ => return Err(JsError::validation("invalid class field key".into())),
                };
                self.define_class_field(p, object, key, self.read(f, i.register_a()))?;
            }
            Op::SetThisField | Op::SetThisFieldStrict => {
                let this = self.checked_this_binding(p, f)?;
                self.set_field_cached(
                    p,
                    this,
                    i.atom_index(),
                    self.read(f, i.register_a()),
                    i.cache_site_index(),
                    i.op() == Op::SetThisFieldStrict,
                )?;
            }
            Op::SetIndex => {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_dispatch(true, false);
                self.set_index_mode(
                    p,
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                    self.read(f, i.register_a()),
                    p.functions[self.frames[f].function as usize].strict
                        || i.boolean_flag().expect("validated boolean immediate"),
                )?
            }
            Op::DefineArrayElement => self.define_array_literal_element(
                p,
                self.read(f, i.register_b()),
                i.array_index() as usize,
                self.read(f, i.register_a()),
            )?,
            Op::Move => self.write(f, i.result_register(), self.read(f, i.register_b())),
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                let operator = i.binary_operator();
                let left_operand = i.operand_b();
                let right_operand = i.operand_c();
                self.profile
                    .binary(operator as usize, left_operand.0, right_operand.0);
                let left = self.resolve_operand(p, f, left_operand)?;
                let right = self.resolve_operand(p, f, right_operand)?;
                #[cfg(feature = "profile-aggregate")]
                {
                    let integer = self.numeric_integer_binary(operator, left, right).is_some();
                    self.profile.regional_binary(
                        self.frames[f].program.raw(),
                        self.frames[f].function,
                        (*pc - 1) as u32,
                        integer,
                    );
                    if i.op() != Op::Binary {
                        self.profile.numeric_binary_path(
                            operator as usize,
                            integer,
                            left.as_int().is_some() && right.as_int().is_some(),
                        );
                    }
                }
                let v = match self.number_binary(operator, left, right) {
                    Some(value) => value,
                    None => self.binary(p, operator, left, right)?,
                };
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                if let Some(local) = i.numeric_local_target() {
                    self.frames[f].locals[local as usize] = v;
                    self.profile
                        .virtual_opcode(self.frames[f].program.raw(), Op::StoreLocalPlain as usize);
                } else {
                    self.write(f, i.result_register(), v);
                }
            }
            Op::IncDec => {
                let input = self.read(f, i.register_b());
                let is_decrement = i.boolean_flag().expect("validated boolean immediate");
                // ToNumeric, then Number::add or BigInt::add of one (ECMA-262 13.4).
                let value = match number_increment(input, is_decrement) {
                    Some(value) => value,
                    None => {
                        let numeric = self.to_numeric(p, input)?;
                        match number_increment(numeric, is_decrement) {
                            Some(value) => value,
                            None => self.numeric_update(p, numeric, is_decrement)?,
                        }
                    }
                };
                self.write(f, i.result_register(), value);
            }
            Op::Unary => {
                let operator = i.unary_operator();
                let operand = self.read(f, i.register_b());
                let v = if operator == super::coercion::LOGICAL_NOT {
                    self.logical_not(operand)
                } else {
                    self.unary(p, operator, operand)?
                };
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::Delete => {
                let result = self.delete_reference_property(
                    p,
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                )?;
                if !self.truthy(result)
                    && (p.functions[self.frames[f].function as usize].strict
                        || i.boolean_flag().expect("validated boolean immediate"))
                {
                    return Err(self.type_error(p, "Cannot delete property in strict mode".into()));
                }
                self.write(f, i.result_register(), result);
            }
            Op::Jump => {
                *pc = i.jump_target() as usize;
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = true;
                self.maybe_collect(p);
            }
            Op::JumpFalse => {
                let value = self.read(f, i.register_a());
                let truthy = self.truthy(value);
                #[cfg(feature = "profile-aggregate")]
                self.profile
                    .branch_value(value.profile_kind() as usize, truthy);
                if !truthy {
                    *pc = i.jump_target() as usize;
                }
            }
            Op::JumpTrue => {
                let value = self.read(f, i.register_a());
                let truthy = self.truthy(value);
                #[cfg(feature = "profile-aggregate")]
                self.profile
                    .branch_value(value.profile_kind() as usize, truthy);
                if truthy {
                    *pc = i.jump_target() as usize;
                }
            }
            Op::JumpBinaryFalse => {
                let operator = i.binary_operator_field();
                let left_operand = i.operand_b();
                let right_operand = i.operand_c();
                self.profile
                    .binary(operator as usize, left_operand.0, right_operand.0);
                let left = self.resolve_operand(p, f, left_operand)?;
                let right = self.resolve_operand(p, f, right_operand)?;
                if !self.binary_truthy(p, operator, left, right)? {
                    *pc = i.jump_target() as usize;
                }
            }
            Op::Call | Op::CallArray | Op::CallForwardingArguments => {
                if i.op() == Op::CallForwardingArguments
                    && let Some(result) =
                        self.forward_activation_arguments(p, f, i, *pc, allow_inline_calls)
                {
                    return result;
                }
                self.profile.call_source(0);
                let window = i.call_window();
                // A spread argument array is read before any call state changes, since
                // reading it can fail; register windows are copied only on the paths that
                // need an owned argument list.
                let spread_arguments = if i.op() == Op::CallArray {
                    let array = self.read(f, window.base);
                    Some(CallArguments::from_values(self.array_values(array)?))
                } else {
                    None
                };
                let this = self.read(f, i.register_c());
                let callee = self.read(f, i.register_b());
                let caller = &mut *self.frames[f];
                caller.pc = *pc;
                caller.pc_is_resume = true;
                debug_assert_eq!(caller.program, self.active_program);
                let direct_eval = i.direct_eval()
                    && matches!(
                        self.heap.get(callee),
                        Some(Cell::Function {
                            kind: FunctionKind::Native(Native::Eval),
                            object,
                            ..
                        }) if object.realm == self.realm.globals
                    );
                let previous_binding_site_pc = if direct_eval {
                    Some(std::mem::replace(
                        &mut self.frames[f].binding_site_pc,
                        Some(*pc as u32),
                    ))
                } else {
                    None
                };
                let parameter_eval = direct_eval && i.parameter_eval();
                let previous_direct_eval = self.direct_eval;
                let previous_parameter_eval = self.parameter_eval;
                let previous_this = (direct_eval && !this.is_undefined())
                    .then(|| std::mem::replace(&mut self.frames[f].this, this));
                // The dispatch loop offers every plain call to `try_inline_user_call` first.
                // Compilation flags every call that falls into a return of its result.
                let terminal = i.returns_from_frame();
                self.direct_eval = direct_eval;
                self.parameter_eval = parameter_eval;
                let arguments = spread_arguments
                    .unwrap_or_else(|| CallArguments::from_slice(self.register_window(f, window)));
                let args = arguments.as_slice();
                if p.kind == crate::bytecode::ProgramKind::Wasm
                    && i.returns_from_frame()
                    && let Some(CallTarget::User(program_id, id, env)) =
                        self.call_target(callee).ok()
                {
                    let program = self.programs.get(program_id).ok_or_else(|| {
                        JsError::validation("missing Wasm tail-call program".into())
                    })?;
                    let realm = match self.heap.get(callee) {
                        Some(Cell::Function { object, .. }) => object.realm,
                        _ => self.realm.globals,
                    };
                    let result = self.with_call_roots(
                        [callee, this].into_iter().chain(args.iter().copied()),
                        |vm| {
                            vm.set_active_program(program_id);
                            vm.switch_realm_global(realm);
                            vm.prepare_user_tail(
                                &program,
                                f,
                                id,
                                env,
                                this,
                                args,
                                CallContext::Function(callee),
                            )
                        },
                    );
                    self.direct_eval = previous_direct_eval;
                    self.parameter_eval = previous_parameter_eval;
                    result?;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                if terminal
                    && self.host.proper_tail_calls()
                    && p.kind != crate::bytecode::ProgramKind::Wasm
                    && p.functions[self.frames[f].function as usize]
                        .permits_js_tail_call_at(pc.saturating_sub(1) as u32)
                    && !matches!(self.heap.get(callee), Some(Cell::Proxy { .. }))
                    && let Some(CallTarget::User(program_id, id, env)) =
                        self.call_target(callee).ok()
                    && program_id == self.frames[f].program
                    && !p.functions[id as usize].is_async
                    && !p.functions[id as usize].is_generator
                {
                    self.with_call_roots(
                        [callee, this].into_iter().chain(args.iter().copied()),
                        |vm| {
                            vm.prepare_user_tail(
                                p,
                                f,
                                id,
                                env,
                                this,
                                args,
                                CallContext::Function(callee),
                            )
                        },
                    )?;
                    self.direct_eval = previous_direct_eval;
                    self.parameter_eval = previous_parameter_eval;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                let discarded_wasm_frame =
                    p.kind == crate::bytecode::ProgramKind::Wasm && i.returns_from_frame();
                if discarded_wasm_frame {
                    // Native/host calls need argument roots, not the discarded guest activation.
                    self.with_stack.truncate(self.frames[f].with_base);
                    let frame = &mut self.frames[f];
                    frame.context = CallContext::Internal;
                    frame.reflected_parameters = 0;
                    frame.original_arguments.clear();
                    frame.locals.fill(Value::UNDEFINED);
                    frame.registers.fill(Value::UNDEFINED);
                    frame.dynamic_bindings.clear();
                    frame.active_iterators.clear();
                    frame.env = Value::NULL;
                    frame.this = Value::UNDEFINED;
                    frame.fixed_this = false;
                    frame.captured = false;
                }
                if allow_inline_calls
                    && !direct_eval
                    && let Some(result) = self.try_inline_call_apply(p, f, i, *pc)
                {
                    self.direct_eval = previous_direct_eval;
                    self.parameter_eval = previous_parameter_eval;
                    return result;
                }
                let called = if discarded_wasm_frame {
                    self.call_value(p, callee, this, args)
                } else {
                    self.call_value_from_frame(p, callee, this, args)
                };
                if let Some(binding_site_pc) = previous_binding_site_pc {
                    self.frames[f].binding_site_pc = binding_site_pc;
                }
                let value = match called {
                    Ok(value) => value,
                    Err(error) => {
                        if let Some(previous_this) = previous_this {
                            self.frames[f].this = previous_this;
                        }
                        self.direct_eval = previous_direct_eval;
                        self.parameter_eval = previous_parameter_eval;
                        return Err(error);
                    }
                };
                if let Some(previous_this) = previous_this {
                    self.frames[f].this = previous_this;
                }
                self.direct_eval = previous_direct_eval;
                self.parameter_eval = previous_parameter_eval;
                if i.returns_from_frame() {
                    self.profile.terminal_call(0);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::CallKnown
                if allow_inline_calls
                    && p.kind == crate::bytecode::ProgramKind::Wasm
                    && !i.returns_from_frame() =>
            {
                self.profile.call_source(1);
                if let Some(result) =
                    self.call_compiled(f, u32::from(i.known_function_index()), i.call_window())
                {
                    let value = result?;
                    self.write(f, i.result_register(), value);
                    return Ok(StepResult::Continue);
                }
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = true;
                let call_guard = self.push_wasm_frame(
                    p,
                    f,
                    u32::from(i.known_function_index()),
                    i.call_window(),
                )?;
                return Ok(StepResult::PushFrame {
                    destination: i.result_register(),
                    call_guard,
                    construct_this: None,
                });
            }
            Op::CallKnown => {
                self.profile.call_source(1);
                let window = i.call_window();
                let function_index = i.known_function_index();
                let arguments = CallArguments::from_slice(self.register_window(f, window));
                let args = arguments.as_slice();
                let parent = self.capture_env(f, 0).unwrap_or(self.frames[f].env);
                self.profile.call_target(1, usize::from(window.count));
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = true;
                let terminal = i.returns_from_frame();
                if ((p.kind == crate::bytecode::ProgramKind::Wasm && i.returns_from_frame())
                    || (p.kind != crate::bytecode::ProgramKind::Wasm
                        && self.host.proper_tail_calls()
                        && terminal
                        && p.functions[self.frames[f].function as usize]
                            .permits_js_tail_call_at(pc.saturating_sub(1) as u32)))
                    && !p.functions[function_index as usize].is_async
                    && !p.functions[function_index as usize].is_generator
                {
                    self.prepare_user_tail(
                        p,
                        f,
                        u32::from(function_index),
                        parent,
                        Value::UNDEFINED,
                        args,
                        CallContext::Internal,
                    )?;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                let value = self.call_user_maybe_async(
                    p,
                    u32::from(function_index),
                    parent,
                    Value::UNDEFINED,
                    args,
                    CallContext::Internal,
                )?;
                if i.returns_from_frame() {
                    self.profile.terminal_call(0);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::CallMethod => {
                self.profile.call_source(2);
                let this = self.read(f, i.register_b());
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = true;
                let value = self.call_method_site_safe(p, f, i.method_site_index(), this)?;
                if i.returns_from_frame() {
                    self.profile.terminal_call(1);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::CallThisMethod => {
                self.profile.call_source(3);
                let method_site = i.method_site_index();
                let path = p.method_sites[method_site].receiver_path;
                let this = if let Some((atom, cache)) = path {
                    self.get_field_cached(p, self.frames[f].this, atom, cache)?
                } else {
                    self.frames[f].this
                };
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = true;
                let value = self.call_method_site_safe(p, f, method_site, this)?;
                if i.returns_from_frame() {
                    self.profile.terminal_call(2);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::Construct => {
                self.profile.call_source(4);
                // A construction whose result is returned is pushed too; its caller returns
                // the constructed value (see `PendingGeneralCall::returns`).
                if allow_inline_calls
                    && !i.is_super_construct()
                    && let crate::bytecode::ConstructArguments::Registers(window) =
                        i.construct_arguments()
                {
                    let callee = self.read(f, i.register_b());
                    if let Some((id, env, this)) = self.inline_construct_target(p, f, callee) {
                        let arguments: *const [Value] = self.register_window(f, window);
                        self.frames[f].pc = *pc;
                        self.frames[f].pc_is_resume = true;
                        self.construct_target = Some(ConstructionTarget {
                            callee,
                            new_target: callee,
                        });
                        // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
                        let pushed = self.push_general_user_frame(
                            p,
                            id,
                            env,
                            this,
                            unsafe { &*arguments },
                            CallContext::user_function(id, callee),
                        );
                        self.construct_target = None;
                        return Ok(StepResult::PushFrame {
                            destination: i.result_register(),
                            call_guard: pushed?,
                            construct_this: Some(this),
                        });
                    }
                }
                let args = match i.construct_arguments() {
                    crate::bytecode::ConstructArguments::Array(register) => {
                        let array = self.read(f, register);
                        if !self.heap.array(array).is_some() {
                            return Err(JsError("constructor arguments are not an array".into()));
                        }
                        self.call_argument_list(p, array, false)?
                    }
                    crate::bytecode::ConstructArguments::Registers(window) => {
                        self.register_window(f, window).to_vec()
                    }
                };
                self.frames[f].pc = *pc;
                self.frames[f].pc_is_resume = true;
                let callee = self.read(f, i.register_b());
                let v = if i.is_super_construct() {
                    self.construct_super_value(p, callee, &args)?
                } else {
                    self.construct_value(p, callee, &args)?
                };
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::Return => return Ok(StepResult::Return(self.read(f, i.register_a()))),
            Op::Throw => {
                let value = self.read(f, i.register_a());
                let message = self.detail_string(value);
                return Err(JsError::thrown(value, message));
            }
            _ => return self.step_wasm(p, f, i, pc),
        }
        Ok(StepResult::Continue)
    }

    #[inline(always)]
    pub(super) fn resolve_operand(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        operand: Operand,
    ) -> Result<Value, JsError> {
        let tag = operand.tag();
        self.profile.operand(tag as usize);
        match operand.kind() {
            Some(crate::bytecode::OperandKind::Register) => {
                Ok(self.read(frame, operand.payload() as Register))
            }
            Some(crate::bytecode::OperandKind::Constant) => {
                debug_assert_eq!(self.frames[frame].program, self.active_program);
                self.active_constant(operand.payload() as usize)
                    .ok_or_else(|| {
                        JsError::validation("constant operand is outside program".into())
                    })
            }
            Some(crate::bytecode::OperandKind::Field) => {
                self.resolve_field(p, frame, usize::from(operand.payload()))
            }
            Some(crate::bytecode::OperandKind::Local) => {
                let slot = operand.payload() as usize;
                // A plain slot is exactly what `LoadLocalPlain` reads.
                if p.functions[self.frames[frame].function as usize].plain_local(&p.atoms, slot) {
                    return Ok(self.frames[frame].locals[slot]);
                }
                self.load_local_binding(p, frame, slot, None)
            }
            None => unreachable!("two-bit operand tag"),
        }
    }
    #[inline(always)]
    fn with_field_source<T>(
        &mut self,
        frame: usize,
        site: usize,
        part: crate::bytecode::FieldPart,
        run: impl FnOnce(&mut Self) -> Result<T, JsError>,
    ) -> Result<T, JsError> {
        let previous = self.frames[frame].source_field.replace((site, part));
        let result = run(self);
        self.frames[frame].source_field = previous;
        result
    }

    pub(super) fn resolve_field(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site: usize,
    ) -> Result<Value, JsError> {
        let site_id = site;
        let site = p.field_sites[site_id];
        let base = self.resolve_field_base(p, frame, site.base)?;
        let value =
            self.with_field_source(frame, site_id, crate::bytecode::FieldPart::First, |vm| {
                vm.get_field_cached(p, base, site.first.0, site.first.1)
            })?;
        match site.second {
            Some((atom, cache)) => {
                self.with_field_source(frame, site_id, crate::bytecode::FieldPart::Second, |vm| {
                    vm.get_field_cached(p, value, atom, cache)
                })
            }
            None => Ok(value),
        }
    }
}

/// The `++`/`--` step: one, negated for a decrement.
fn increment_delta(is_decrement: bool) -> f64 {
    if is_decrement { -1.0 } else { 1.0 }
}

/// `++`/`--` of a Number operand, staying an integer while the result fits; `None` for any
/// other operand, whose conversion may run guest code or allocate.
#[inline(always)]
pub(super) fn number_increment(input: Value, is_decrement: bool) -> Option<Value> {
    if let Some(integer) = input.as_int() {
        let next = if is_decrement {
            integer.checked_sub(1)
        } else {
            integer.checked_add(1)
        };
        return Some(next.map_or_else(
            || Value::number(f64::from(integer) + increment_delta(is_decrement)),
            Value::integer,
        ));
    }
    input
        .as_number()
        .map(|number| Value::number(number + increment_delta(is_decrement)))
}
