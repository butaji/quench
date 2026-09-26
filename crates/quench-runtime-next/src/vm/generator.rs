use super::activation::{
    AsyncGeneratorOperation, AsyncGeneratorRequest, Completion, Continuation, GeneratorRecord,
};
use super::promise::{PromiseReaction, PromiseState};
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn generator_prototype_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let is_running = match self.heap.get(receiver) {
            Some(Cell::Iterator {
                kind: IteratorKind::Generator,
                generator: Some(record),
                ..
            }) => Some(record.running),
            _ => None,
        };
        let Some(is_running) = is_running else {
            let name = match native {
                Native::GeneratorNext => "next",
                Native::GeneratorReturn => "return",
                Native::GeneratorThrow => "throw",
                _ => unreachable!(),
            };
            return Err(self.type_error(
                p,
                format!("Generator.prototype.{name} called on incompatible receiver"),
            ));
        };
        if is_running {
            return Err(self.type_error(p, "Generator is already executing".into()));
        }
        match native {
            Native::GeneratorNext => self.generator_next(p, receiver, args),
            Native::GeneratorReturn => self.generator_return(p, receiver, args),
            Native::GeneratorThrow => self.generator_throw(p, receiver, args),
            _ => unreachable!(),
        }
    }

    pub(super) fn generator_record_mut(
        &mut self,
        generator: Value,
    ) -> Option<&mut GeneratorRecord> {
        match self.heap.get_mut(generator) {
            Some(Cell::Iterator {
                generator: Some(record),
                ..
            }) => Some(record.as_mut()),
            _ => None,
        }
    }

    pub(super) fn call_generator(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.profile.function(id as usize);
        let parameter_eval_arguments_error =
            p.functions[id as usize].parameter_eval_arguments_error;
        if parameter_eval_arguments_error {
            return self.syntax_error_result(
                p,
                "arguments binding is not allowed in generator parameters",
            );
        }
        let function = &p.functions[id as usize];
        let mut frame = self.frame_pool.pop().unwrap_or(Frame {
            program: self.active_program,
            function: 0,
            pc: 0,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            dynamic_bindings: vec![],
            captured: false,
            registers: vec![],
            active_iterators: vec![],
            with_base: self.with_stack.len(),
        });
        frame
            .locals
            .resize(function.locals as usize, Value::UNDEFINED);
        frame.locals[function.params as usize..].fill(Value::UNDEFINED);
        let fixed = usize::from(function.params) - usize::from(function.rest);
        for index in 0..fixed {
            frame.locals[index] = args.get(index).copied().unwrap_or(Value::UNDEFINED);
        }
        if function.rest {
            frame.locals[fixed] = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(args.get(fixed..).unwrap_or_default().to_vec()),
            });
        }
        if let Some(slot) = function
            .local_atoms
            .iter()
            .position(|atom| self.atom_name(*atom).contains("\0rqj:self-binding:"))
        {
            frame.locals[slot] = self.function_values[&(self.active_program, id)]
                .iter()
                .rev()
                .find_map(|(closure_env, value)| (*closure_env == parent).then_some(*value))
                .unwrap_or(Value::UNDEFINED);
        }
        if let Some(encoded_slot) = function.arguments_slot {
            let mapped = encoded_slot & crate::bytecode::MAPPED_ARGUMENTS_BIT != 0;
            let slot = encoded_slot & !crate::bytecode::MAPPED_ARGUMENTS_BIT;
            let arguments = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.object_proto),
                elements: Rc::new(args.to_vec()),
            });
            frame.locals[usize::from(slot)] = arguments;
            self.initialize_arguments_object(p, arguments, id, parent, args, mapped)?;
            if mapped {
                if let Some(object) = self.object_data_mut(arguments) {
                    object.arguments_map =
                        Some((0..function.params.min(args.len() as u16)).collect());
                }
            }
        }
        frame.function = id;
        frame.program = self.active_program;
        frame.pc = 0;
        frame.env = parent;
        frame.this = self.call_this_value(this, function.strict)?;
        frame.captured = false;
        frame.with_base = self.with_stack.len();
        let register_count = function.registers as usize;
        if frame.registers.capacity() < register_count {
            frame
                .registers
                .reserve_exact(register_count - frame.registers.len());
        }
        unsafe { frame.registers.set_len(register_count) };
        if function.parameter_end_pc != 0 {
            self.frames.push(frame);
            let result = self.run_frame_general_until(
                p,
                self.frames.len() - 1,
                Some(function.parameter_end_pc as usize),
                None,
            );
            frame = self.frames.pop().expect("generator parameter frame exists");
            match result {
                Ok(FrameOutcome::ParameterInitializationComplete) => {}
                Ok(_) => {
                    self.frame_pool.push(Self::recycle_frame(frame));
                    return Err(JsError(
                        "generator parameter initialization suspended unexpectedly".into(),
                    ));
                }
                Err(error) => {
                    self.frame_pool.push(Self::recycle_frame(frame));
                    return Err(error);
                }
            }
        }
        let default_prototype = if function.is_async {
            self.async_generator_proto
        } else {
            self.generator_proto
        };
        let function_object = self
            .function_values
            .get(&(self.active_program, id))
            .and_then(|values| {
                values
                    .iter()
                    .find(|(closure_env, _)| *closure_env == parent)
                    .map(|(_, function)| *function)
            });
        let realm = self.realm.globals;
        let prototype_atom = self.intern_atom("prototype");
        let generator_prototype = function_object
            .and_then(|function| self.own_property(function, prototype_atom))
            .filter(|prototype| self.object_data(*prototype).is_some())
            .unwrap_or(default_prototype);
        let generator = self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(generator_prototype),
            source: Value::NULL,
            next_method: None,
            helper: None,
            helper_running: false,
            kind: if function.is_async {
                IteratorKind::AsyncGenerator
            } else {
                IteratorKind::Generator
            },
            index: 0,
            done: false,
            generator: None,
        });
        if let Some(Cell::Iterator { source, .. }) = self.heap.get_mut(generator) {
            *source = generator;
        }
        if let Some(Cell::Iterator {
            generator: slot, ..
        }) = self.heap.get_mut(generator)
        {
            *slot = Some(Box::new(GeneratorRecord {
                continuation: Some(Continuation {
                    program: frame.program,
                    function: frame.function,
                    pc: frame.pc,
                    env: frame.env,
                    this: frame.this,
                    locals: std::mem::take(&mut frame.locals),
                    registers: std::mem::take(&mut frame.registers),
                    active_iterators: std::mem::take(&mut frame.active_iterators),
                    completion: Completion::Yield(Value::UNDEFINED),
                    captured: frame.captured,
                    resume_register: None,
                    promise: Value::UNDEFINED,
                }),
                realm,
                done: false,
                running: false,
                requests: std::collections::VecDeque::new(),
            }));
        } else {
            return Err(JsError(
                "generator allocation lost its iterator cell".into(),
            ));
        }
        self.frame_pool.push(Self::recycle_frame(frame));
        Ok(generator)
    }

    pub(super) fn generator_return(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let kind = self.generator_kind(generator)?;
        if kind == IteratorKind::AsyncGenerator {
            let promise = self.promise_object();
            let running = self
                .generator_record_mut(generator)
                .ok_or_else(|| JsError("async generator receiver is invalid".into()))?
                .running;
            if running {
                self.generator_record_mut(generator)
                    .expect("async generator record exists")
                    .requests
                    .push_back(AsyncGeneratorRequest {
                        operation: AsyncGeneratorOperation::Return,
                        promise,
                        value,
                    });
                return Ok(promise);
            }
            self.async_generator_return_ready(p, generator, value, promise)?;
            return Ok(promise);
        }
        if kind == IteratorKind::Generator
            && let Some((iterator, _)) = self.yield_star_iterator(p, generator)
        {
            return self.return_from_yield_star(p, generator, iterator, value);
        }
        if kind == IteratorKind::Generator {
            return self.complete_generator_return(p, generator, value);
        }
        let continuation = {
            let Some(record) = self.generator_record_mut(generator) else {
                return Err(JsError("generator receiver is invalid".into()));
            };
            if record.running {
                return Err(JsError("generator is already running".into()));
            }
            if record.done {
                None
            } else {
                record.done = true;
                record.continuation.take()
            }
        };
        let cleanup = continuation
            .as_ref()
            .map(|continuation| self.close_suspended_iterators(p, continuation))
            .unwrap_or(Ok(()));
        if let Err(error) = cleanup {
            return Err(error);
        }
        let result = self.iterator_result(value, true)?;
        Ok(result)
    }

    fn async_generator_return_ready(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        value: Value,
        promise: Value,
    ) -> Result<(), JsError> {
        if let Some((iterator, destination)) = self.yield_star_iterator(p, generator) {
            let awaited = match self.promise_for_value(p, value) {
                Ok(promise) => promise,
                Err(error) => {
                    let reason = error
                        .thrown_value()
                        .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                    self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                    return Ok(());
                }
            };
            let environment = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(vec![
                    generator,
                    iterator,
                    Value::number(f64::from(destination)),
                ]),
            });
            let start =
                self.native_with_env(Native::AsyncGeneratorDelegateReturnStart, environment);
            let completion = self.promise_then(p, awaited, start, Value::UNDEFINED)?;
            self.forward_promise(p, completion, promise)?;
            return Ok(());
        }
        if self.promise.records.contains_key(&value)
            && let Err(error) = self.promise_for_value(p, value)
        {
            let reason = error
                .thrown_value()
                .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
            self.async_generator_throw_ready(p, generator, reason, promise)?;
            return Ok(());
        }
        if self.prepare_generator_return(p, generator, value)? {
            self.async_generator_next_with_promise(p, generator, Value::UNDEFINED, promise, None)?;
            return Ok(());
        }
        let (continuation, was_done) = {
            let Some(record) = self.generator_record_mut(generator) else {
                return Err(JsError("async generator receiver is invalid".into()));
            };
            if record.running {
                return Err(JsError("async generator is already running".into()));
            }
            let was_done = record.done;
            record.running = true;
            (record.continuation.take(), was_done)
        };
        if let Some(continuation) = continuation
            && let Err(error) = self.close_suspended_iterators(p, &continuation)
        {
            let reason = error
                .thrown_value()
                .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
            if let Some(record) = self.generator_record_mut(generator) {
                record.running = false;
                record.done = true;
            }
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            return Ok(());
        }
        let awaited = match self.promise_for_value(p, value) {
            Ok(promise) => promise,
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                if was_done {
                    if let Some(record) = self.generator_record_mut(generator) {
                        record.running = false;
                    }
                    self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                    self.resume_async_generator_queue(p, generator)?;
                } else {
                    self.async_generator_throw_ready(p, generator, reason, promise)?;
                }
                return Ok(());
            }
        };
        let environment = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![generator, promise]),
        });
        let fulfilled = self.native_with_env(Native::AsyncGeneratorReturnFulfilled, environment);
        let rejected = self.native_with_env(Native::AsyncGeneratorReturnRejected, environment);
        self.promise_then(p, awaited, fulfilled, rejected)?;
        Ok(())
    }

    pub(super) fn async_generator_return_fulfilled(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let environment = self
            .active_native_env()
            .ok_or_else(|| JsError("async generator return reaction without state".into()))?;
        let Some(Cell::Array { elements, .. }) = self.heap.get(environment) else {
            return Err(JsError("async generator return state is invalid".into()));
        };
        let [generator, promise] = elements.as_slice() else {
            return Err(JsError("async generator return state is malformed".into()));
        };
        let (generator, promise) = (*generator, *promise);
        if let Some(record) = self.generator_record_mut(generator) {
            record.running = false;
            record.done = true;
            record.continuation = None;
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let result = self.iterator_result(value, true)?;
        self.promise_settle(p, promise, PromiseState::Fulfilled, result)?;
        self.resume_async_generator_queue(p, generator)?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn async_generator_return_rejected(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let environment = self
            .active_native_env()
            .ok_or_else(|| JsError("async generator return rejection without state".into()))?;
        let Some(Cell::Array { elements, .. }) = self.heap.get(environment) else {
            return Err(JsError("async generator return state is invalid".into()));
        };
        let [generator, promise] = elements.as_slice() else {
            return Err(JsError("async generator return state is malformed".into()));
        };
        let (generator, promise) = (*generator, *promise);
        if let Some(record) = self.generator_record_mut(generator) {
            record.running = false;
            record.done = true;
            record.continuation = None;
        }
        self.promise_settle(
            p,
            promise,
            PromiseState::Rejected,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        )?;
        self.resume_async_generator_queue(p, generator)?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn async_generator_delegate_return_start(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let environment = self
            .active_native_env()
            .ok_or_else(|| JsError("async delegate return without state".into()))?;
        let Some(Cell::Array { elements, .. }) = self.heap.get(environment) else {
            return Err(JsError("async delegate return state is invalid".into()));
        };
        let [generator, iterator, destination] = elements.as_slice() else {
            return Err(JsError("async delegate return state is malformed".into()));
        };
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::Iterator {
            source,
            kind: IteratorKind::AsyncFromSync,
            ..
        }) = self.heap.get(*iterator)
        {
            return self.return_from_async_from_sync(
                p,
                *generator,
                *iterator,
                *source,
                destination.as_number().unwrap_or(0.0) as u16,
                value,
            );
        }
        self.async_generator_delegate(
            p,
            *generator,
            *iterator,
            destination.as_number().unwrap_or(0.0) as u16,
            value,
            false,
        )
    }

    fn forward_promise(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        target: Value,
    ) -> Result<(), JsError> {
        let record = self
            .promise
            .records
            .get(&source)
            .cloned()
            .ok_or_else(|| JsError("async generator request result is not a Promise".into()))?;
        let reaction = PromiseReaction {
            on_fulfilled: Value::UNDEFINED,
            on_rejected: Value::UNDEFINED,
            next: target,
        };
        if record.state == PromiseState::Pending {
            self.promise
                .records
                .get_mut(&source)
                .expect("source Promise record exists")
                .reactions
                .push(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, record.state, record.result);
        }
        Ok(())
    }

    fn yield_star_iterator(&self, p: &ResidualProgram, generator: Value) -> Option<(Value, u16)> {
        let record = match self.heap.get(generator) {
            Some(Cell::Iterator {
                generator: Some(record),
                ..
            }) => record,
            _ => return None,
        };
        let continuation = record.continuation.as_ref()?;
        let function = p.functions.get(continuation.function as usize)?;
        let instruction = *function.code.get(continuation.pc)?;
        let instruction = if instruction.is_wide() {
            function.wide.get(instruction.wide_index()).copied()?
        } else {
            instruction.as_wide()
        };
        if instruction.op() != Op::YieldStar {
            return None;
        }
        let iterator = *continuation
            .registers
            .get(usize::from(instruction.register_c()))?;
        (!iterator.is_undefined()).then_some((iterator, instruction.result_register()))
    }

    fn return_from_yield_star(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        iterator: Value,
        value: Value,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom("return");
        let method = match self.get_property(p, iterator, atom) {
            Ok(method) => method,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if method.is_undefined() || method.is_null() {
            return self.complete_generator_return(p, generator, value);
        }
        if !self.is_function(method) {
            let error = self.type_error(p, "iterator return method is not callable".into());
            return self.resume_generator(p, generator, &[], Some(error));
        }
        let result = match self.call_value(p, method, iterator, &[value]) {
            Ok(result) => result,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if !self.is_object_like(result) {
            let error = self.type_error(p, "iterator return result is not an object".into());
            return self.resume_generator(p, generator, &[], Some(error));
        }
        let done_atom = self.intern_atom("done");
        let done = match self.get_property(p, result, done_atom) {
            Ok(done) => done,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if !self.truthy(done) {
            return Ok(result);
        }
        let value_atom = self.intern_atom("value");
        let value = match self.get_property(p, result, value_atom) {
            Ok(value) => value,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        self.complete_generator_return(p, generator, value)
    }

    fn complete_generator_return(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        value: Value,
    ) -> Result<Value, JsError> {
        let Some(record) = self.generator_record_mut(generator) else {
            return Err(JsError("generator receiver is invalid".into()));
        };
        if record.running {
            return Err(JsError("generator is already running".into()));
        }
        if record.done {
            return self.iterator_result(value, true);
        }
        if self.prepare_generator_return(p, generator, value)? {
            return self.resume_generator(p, generator, &[], None);
        }
        let continuation = self
            .generator_record_mut(generator)
            .and_then(|record| record.continuation.as_ref())
            .cloned();
        let cleanup = continuation
            .as_ref()
            .map(|continuation| self.close_suspended_iterators(p, continuation))
            .unwrap_or(Ok(()));
        self.close_generator(generator)?;
        cleanup?;
        self.iterator_result(value, true)
    }

    fn prepare_generator_return(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        value: Value,
    ) -> Result<bool, JsError> {
        let unwind = {
            let Some(record) = self.generator_record_mut(generator) else {
                return Err(JsError("generator receiver is invalid".into()));
            };
            let Some(continuation) = record.continuation.as_ref() else {
                return Err(JsError("generator is suspended".into()));
            };
            let Some(function) = p.functions.get(continuation.function as usize) else {
                return Err(JsError("generator function is invalid".into()));
            };
            let suspended_instruction = continuation.pc.saturating_sub(1) as u32;
            function
                .handlers
                .iter()
                .filter(|handler| {
                    suspended_instruction >= handler.start && suspended_instruction < handler.end
                })
                .filter_map(|handler| {
                    Some((
                        handler.return_target?,
                        handler.return_slot?,
                        continuation.captured,
                        continuation.env,
                        handler.end - handler.start,
                    ))
                })
                .min_by_key(|(_, _, _, _, width)| *width)
                .map(|(target, slot, captured, env, _)| (target, slot, captured, env))
        };
        let Some((target, slot, captured, env)) = unwind else {
            return Ok(false);
        };

        if captured {
            let Some(Cell::Environment { slots, .. }) = self.heap.get_mut(env) else {
                return Err(JsError("invalid generator return environment".into()));
            };
            let Some(target_slot) = slots.get_mut(usize::from(slot)) else {
                return Err(JsError("generator return slot is out of bounds".into()));
            };
            *target_slot = value;
        } else {
            let Some(record) = self.generator_record_mut(generator) else {
                return Err(JsError("generator receiver is invalid".into()));
            };
            let Some(continuation) = record.continuation.as_mut() else {
                return Err(JsError("generator is suspended".into()));
            };
            let Some(target_slot) = continuation.locals.get_mut(usize::from(slot)) else {
                return Err(JsError("generator return slot is out of bounds".into()));
            };
            *target_slot = value;
        }
        if let Some(continuation) = self
            .generator_record_mut(generator)
            .and_then(|record| record.continuation.as_mut())
        {
            continuation.pc = target as usize;
            continuation.resume_register = None;
            continuation.completion = Completion::Return(value);
        }
        Ok(true)
    }

    fn return_from_async_from_sync(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        adapter: Value,
        iterator: Value,
        destination: u16,
        value: Value,
    ) -> Result<Value, JsError> {
        let promise = self.promise_object();
        let atom = self.intern_atom("return");
        let method = match self.get_property(p, iterator, atom) {
            Ok(method) => method,
            Err(error) => {
                self.fail_async_generator(p, generator, promise, error)?;
                return Ok(promise);
            }
        };
        if method.is_undefined() || method.is_null() {
            self.close_generator(generator)?;
            let result = self.iterator_result(value, true)?;
            self.promise_resolve_value(p, promise, result)?;
            return Ok(promise);
        }
        if !self.is_function(method) {
            let error = self.type_error(p, "iterator return method is not callable".into());
            self.fail_async_generator(p, generator, promise, error)?;
            return Ok(promise);
        }
        let result = match self.call_value(p, method, iterator, &[value]) {
            Ok(result) => result,
            Err(error) => {
                self.fail_async_generator(p, generator, promise, error)?;
                return Ok(promise);
            }
        };
        if !self.is_object_like(result) {
            let error = self.type_error(p, "iterator return result is not an object".into());
            self.fail_async_generator(p, generator, promise, error)?;
            return Ok(promise);
        }
        let unwrapped = self.async_from_sync_result(p, adapter, result, false)?;
        let env = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![
                generator,
                Value::number(f64::from(destination)),
                Value::FALSE,
                Value::FALSE,
                Value::TRUE,
            ]),
        });
        let fulfilled = self.native_with_env(Native::AsyncGeneratorDelegateFulfilled, env);
        let rejected = self.native_with_env(Native::AsyncGeneratorDelegateRejected, env);
        self.promise_then(p, unwrapped, fulfilled, rejected)
    }

    fn close_suspended_iterators(
        &mut self,
        p: &ResidualProgram,
        continuation: &Continuation,
    ) -> Result<(), JsError> {
        let mut first_error = None;
        for cleanup in continuation.active_iterators.iter().rev() {
            let Some(&done) = continuation.registers.get(usize::from(cleanup.done)) else {
                first_error.get_or_insert_with(|| JsError("invalid iterator cleanup state".into()));
                continue;
            };
            if self.truthy(done) {
                continue;
            }
            let Some(&iterator) = continuation.registers.get(usize::from(cleanup.iterator)) else {
                first_error
                    .get_or_insert_with(|| JsError("invalid iterator cleanup target".into()));
                continue;
            };
            if let Err(error) = self.iterator_close(p, iterator) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(super) fn generator_throw(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let kind = self.generator_kind(generator)?;
        if kind == IteratorKind::AsyncGenerator {
            if let Some((iterator, destination)) = self.yield_star_iterator(p, generator) {
                self.async_generator_delegate(p, generator, iterator, destination, value, true)
            } else {
                let promise = self.promise_object();
                let (running, done) = self
                    .generator_record_mut(generator)
                    .map(|record| (record.running, record.done))
                    .ok_or_else(|| JsError("async generator receiver is invalid".into()))?;
                if running {
                    self.generator_record_mut(generator)
                        .expect("async generator record exists")
                        .requests
                        .push_back(AsyncGeneratorRequest {
                            operation: AsyncGeneratorOperation::Throw,
                            promise,
                            value,
                        });
                } else if done {
                    self.promise_settle(p, promise, PromiseState::Rejected, value)?;
                } else {
                    self.async_generator_throw_ready(p, generator, value, promise)?;
                }
                Ok(promise)
            }
        } else if let Some((iterator, destination)) = self.yield_star_iterator(p, generator) {
            self.throw_into_yield_star(p, generator, iterator, destination, value)
        } else {
            self.resume_generator(
                p,
                generator,
                &[],
                Some(JsError::thrown(value, "generator throw".into())),
            )
        }
    }

    pub(super) fn async_generator_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !matches!(
            self.heap.get(receiver),
            Some(Cell::Iterator {
                kind: IteratorKind::AsyncGenerator,
                generator: Some(_),
                ..
            })
        ) {
            let name = match native {
                Native::AsyncGeneratorNext => "next",
                Native::AsyncGeneratorReturn => "return",
                Native::AsyncGeneratorThrow => "throw",
                _ => unreachable!(),
            };
            let error = self.type_error(
                p,
                format!("AsyncGenerator.{name} called on incompatible receiver"),
            );
            let reason = error
                .thrown_value()
                .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
            let promise = self.promise_object();
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            return Ok(promise);
        }
        match native {
            Native::AsyncGeneratorNext => self.async_generator_next(p, receiver, args),
            Native::AsyncGeneratorReturn => self.generator_return(p, receiver, args),
            Native::AsyncGeneratorThrow => self.generator_throw(p, receiver, args),
            _ => Err(JsError("invalid async generator method".into())),
        }
    }

    fn async_generator_delegate(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        iterator: Value,
        destination: u16,
        argument: Value,
        throwing: bool,
    ) -> Result<Value, JsError> {
        let (receiver, adapter) = match self.heap.get(iterator) {
            Some(Cell::Iterator {
                source,
                kind: IteratorKind::AsyncFromSync,
                ..
            }) => (*source, true),
            _ => (iterator, false),
        };
        let async_from_sync = adapter;
        let name = if throwing { "throw" } else { "return" };
        let atom = self.intern_atom(name);
        let operation = (|| {
            let method = self.get_property(p, receiver, atom)?;
            if method.is_undefined() || method.is_null() {
                if throwing {
                    let return_atom = self.intern_atom("return");
                    let return_method = self.get_property(p, receiver, return_atom)?;
                    if !return_method.is_undefined() && !return_method.is_null() {
                        if !self.is_function(return_method) {
                            return Err(
                                self.type_error(p, "iterator return method is not callable".into())
                            );
                        }
                        let _ = self.call_value(p, return_method, receiver, &[])?;
                    }
                    if let Some(record) = self.generator_record_mut(generator) {
                        record.running = false;
                        record.done = true;
                        record.continuation = None;
                    }
                    return Err(self.type_error(p, "delegated iterator has no throw method".into()));
                }
                if let Some(record) = self.generator_record_mut(generator) {
                    record.running = false;
                    record.done = true;
                    record.continuation = None;
                }
                let awaited = self.promise_for_value(p, argument)?;
                return self.promise_then(
                    p,
                    awaited,
                    self.native_value(Native::AsyncGeneratorReturnResult),
                    Value::UNDEFINED,
                );
            }
            if !self.is_function(method) {
                return Err(self.type_error(p, "delegated iterator method is not callable".into()));
            }
            let result = self.call_value(p, method, receiver, &[argument])?;
            // Async-from-sync iteration awaits both the synchronous iterator
            // result and its `value` before async-generator delegation sees it.
            // Reuse the same adapter operation as ordinary `for await` steps.
            let (result, adapter) = if adapter {
                (
                    self.async_from_sync_result(p, iterator, result, throwing)?,
                    false,
                )
            } else {
                (result, false)
            };
            let promise = self.promise_for_value(p, result)?;
            let env = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(vec![
                    generator,
                    Value::number(f64::from(destination)),
                    Value::number(if throwing { 1.0 } else { 0.0 }),
                    Value::number(if adapter { 1.0 } else { 0.0 }),
                    if async_from_sync {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    },
                ]),
            });
            let fulfilled = self.native_with_env(Native::AsyncGeneratorDelegateFulfilled, env);
            let rejected = self.native_with_env(Native::AsyncGeneratorDelegateRejected, env);
            self.promise_then(p, promise, fulfilled, rejected)
        })();
        match operation {
            Ok(promise) => Ok(promise),
            Err(error) => {
                if async_from_sync {
                    if let Some(record) = self.generator_record_mut(generator) {
                        record.running = false;
                        record.done = true;
                        record.continuation = None;
                    }
                    let promise = self.promise_object();
                    let reason = error
                        .thrown_value()
                        .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                    self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                    Ok(promise)
                } else {
                    self.resume_async_generator_with_throw(p, generator, error)
                }
            }
        }
    }

    pub(super) fn async_generator_delegate_fulfilled(
        &mut self,
        p: &ResidualProgram,
        _this: Value,
        result: Value,
    ) -> Result<Value, JsError> {
        let env = self
            .active_native_env()
            .ok_or_else(|| JsError("async delegate reaction without state".into()))?;
        let Some(Cell::Array { elements, .. }) = self.heap.get(env) else {
            return Err(JsError("async delegate reaction state is invalid".into()));
        };
        let [generator, destination, throwing, adapter, _async_from_sync] = elements.as_slice()
        else {
            return Err(JsError("async delegate reaction state is malformed".into()));
        };
        let (generator, destination, throwing, adapter) =
            (*generator, *destination, *throwing, *adapter);
        if !self.is_object_like(result) {
            let error = self.type_error(p, "iterator result is not an object".into());
            return self.resume_async_generator_with_throw(p, generator, error);
        }
        let done_atom = self.intern_atom("done");
        let done_value = match self.get_property(p, result, done_atom) {
            Ok(value) => value,
            Err(error) => return self.resume_async_generator_with_throw(p, generator, error),
        };
        let done = self.truthy(done_value);
        let value_atom = self.intern_atom("value");
        let value = match self.get_property(p, result, value_atom) {
            Ok(value) => value,
            Err(error) => return self.resume_async_generator_with_throw(p, generator, error),
        };
        let value = if self.truthy(adapter) {
            let resolved = self.promise_for_value(p, value)?;
            let state = self
                .promise
                .records
                .get(&resolved)
                .cloned()
                .ok_or_else(|| JsError("async-from-sync value lost its Promise".into()))?;
            match state.state {
                PromiseState::Fulfilled => state.result,
                PromiseState::Rejected => {
                    return Err(JsError::thrown(
                        state.result,
                        "async-from-sync value rejected".into(),
                    ));
                }
                PromiseState::Pending => value,
            }
        } else {
            value
        };
        if !done {
            return self.iterator_result(value, false);
        }
        if self.truthy(throwing) {
            if let Some(record) = self.generator_record_mut(generator)
                && let Some(continuation) = record.continuation.as_mut()
            {
                continuation.pc += 1;
                continuation.resume_register = None;
                if let Some(slot) = continuation
                    .registers
                    .get_mut(destination.as_number().unwrap_or(0.0) as usize)
                {
                    *slot = value;
                }
            }
            self.async_generator_next(p, generator, &[])
        } else {
            self.close_generator(generator)?;
            self.iterator_result(value, true)
        }
    }

    pub(super) fn async_generator_delegate_rejected(
        &mut self,
        p: &ResidualProgram,
        _this: Value,
        reason: Value,
    ) -> Result<Value, JsError> {
        let env = self
            .active_native_env()
            .ok_or_else(|| JsError("async delegate rejection without state".into()))?;
        let Some(Cell::Array { elements, .. }) = self.heap.get(env) else {
            return Err(JsError("async delegate rejection state is invalid".into()));
        };
        let generator = elements
            .first()
            .copied()
            .ok_or_else(|| JsError("async delegate rejection state is malformed".into()))?;
        let async_from_sync = elements.get(4).is_some_and(|value| self.truthy(*value));
        let error = JsError::thrown(reason, "delegated async iterator rejected".into());
        if async_from_sync {
            self.fail_async_generator_delegate(generator, error)
        } else {
            self.resume_async_generator_with_throw(p, generator, error)
        }
    }

    fn fail_async_generator_delegate(
        &mut self,
        generator: Value,
        error: JsError,
    ) -> Result<Value, JsError> {
        if let Some(record) = self.generator_record_mut(generator) {
            record.running = false;
            record.done = true;
            record.continuation = None;
        }
        Err(error)
    }

    fn resume_async_generator_with_throw(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        error: JsError,
    ) -> Result<Value, JsError> {
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        let continuation = match self.generator_record_mut(generator) {
            Some(record) => {
                record.running = true;
                record.done = false;
                record.continuation.take()
            }
            None => None,
        };
        let Some(mut continuation) = continuation else {
            let promise = self.promise_object();
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            return Ok(promise);
        };
        continuation.pc += 1;
        continuation.resume_register = None;
        let rejected = self.promise_object();
        self.promise_settle(p, rejected, PromiseState::Rejected, reason)?;
        let promise = self.promise_object();
        let id = self.suspend_continuation(continuation);
        self.enqueue_async_resume(p, id, promise, Some(generator), rejected, false)?;
        Ok(promise)
    }

    fn throw_into_yield_star(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        iterator: Value,
        destination: u16,
        value: Value,
    ) -> Result<Value, JsError> {
        let throw_atom = self.intern_atom("throw");
        let method = match self.get_property(p, iterator, throw_atom) {
            Ok(method) => method,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if method.is_undefined() || method.is_null() {
            return self.throw_into_outer_generator(p, generator, value);
        }
        if !self.is_function(method) {
            let error = self.type_error(p, "iterator throw method is not callable".into());
            return self.resume_generator(p, generator, &[], Some(error));
        }
        let result = match self.call_value(p, method, iterator, &[value]) {
            Ok(result) => result,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if !self.is_object_like(result) {
            let error = self.type_error(p, "iterator throw result is not an object".into());
            return self.resume_generator(p, generator, &[], Some(error));
        }
        let done_atom = self.intern_atom("done");
        let done = match self.get_property(p, result, done_atom) {
            Ok(done) => self.truthy(done),
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if !done {
            return Ok(result);
        }
        let value_atom = self.intern_atom("value");
        let value = match self.get_property(p, result, value_atom) {
            Ok(value) => value,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if let Some(record) = self.generator_record_mut(generator)
            && let Some(continuation) = record.continuation.as_mut()
        {
            continuation.pc += 1;
            continuation.resume_register = None;
            if let Some(register) = continuation.registers.get_mut(usize::from(destination)) {
                *register = value;
            }
        }
        self.resume_generator(p, generator, &[], None)
    }

    fn throw_into_outer_generator(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        _value: Value,
    ) -> Result<Value, JsError> {
        let return_atom = self.intern_atom("return");
        let iterator = self
            .yield_star_iterator(p, generator)
            .map(|(iterator, _)| iterator)
            .unwrap_or(Value::UNDEFINED);
        let method = match self.get_property(p, iterator, return_atom) {
            Ok(method) => method,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if !method.is_undefined() && !method.is_null() {
            if !self.is_function(method) {
                let error = self.type_error(p, "iterator return method is not callable".into());
                return self.resume_generator(p, generator, &[], Some(error));
            }
            let result = match self.call_value(p, method, iterator, &[]) {
                Ok(result) => result,
                Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
            };
            if !self.is_object_like(result) {
                let error = self.type_error(p, "iterator return result is not an object".into());
                return self.resume_generator(p, generator, &[], Some(error));
            }
        }
        let error = self.type_error(p, "delegated iterator has no throw method".into());
        self.resume_generator(p, generator, &[], Some(error))
    }

    fn generator_kind(&self, generator: Value) -> Result<IteratorKind, JsError> {
        match self.heap.get(generator) {
            Some(Cell::Iterator { kind, .. })
                if matches!(kind, IteratorKind::Generator | IteratorKind::AsyncGenerator) =>
            {
                Ok(*kind)
            }
            _ => Err(JsError("generator receiver is invalid".into())),
        }
    }

    fn close_generator(&mut self, generator: Value) -> Result<(), JsError> {
        let Some(record) = self.generator_record_mut(generator) else {
            return Err(JsError("generator receiver is invalid".into()));
        };
        if record.running {
            return Err(JsError("generator is already running".into()));
        }
        record.done = true;
        record.continuation = None;
        Ok(())
    }

    pub(super) fn generator_next(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.resume_generator(p, generator, args, None)
    }

    fn resume_generator(
        &mut self,
        _p: &ResidualProgram,
        generator: Value,
        args: &[Value],
        initial_error: Option<JsError>,
    ) -> Result<Value, JsError> {
        let (continuation, realm, done, running) = {
            let Some(record) = self.generator_record_mut(generator) else {
                return Err(JsError("generator receiver is invalid".into()));
            };
            if record.running {
                return Err(JsError("generator is already running".into()));
            }
            (
                record.continuation.take(),
                record.realm,
                record.done,
                record.running,
            )
        };
        if running {
            return Err(JsError("generator is already running".into()));
        }
        if done {
            if let Some(error) = initial_error {
                return Err(error);
            }
            return self.iterator_result(Value::UNDEFINED, true);
        }
        let continuation = continuation.ok_or_else(|| JsError("generator is suspended".into()))?;
        let execution_program = self
            .programs
            .get(continuation.program)
            .ok_or_else(|| JsError("generator continuation program is unavailable".into()))?;
        if let Some(record) = self.generator_record_mut(generator) {
            record.running = true;
        }
        let mut frame = Frame {
            program: continuation.program,
            function: continuation.function,
            pc: continuation.pc,
            env: continuation.env,
            this: continuation.this,
            locals: continuation.locals,
            dynamic_bindings: vec![],
            captured: continuation.captured,
            registers: continuation.registers,
            active_iterators: continuation.active_iterators,
            with_base: self.with_stack.len(),
        };
        if initial_error.is_none()
            && let Some(register) = continuation.resume_register
        {
            if register as usize >= frame.registers.len() {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generator_record_mut(generator) {
                    record.running = false;
                    record.done = true;
                }
                return Err(JsError("invalid generator resume register".into()));
            }
            frame.registers[register as usize] = args.first().copied().unwrap_or(Value::UNDEFINED);
        }
        let previous_program = std::mem::replace(&mut self.active_program, continuation.program);
        let previous_global = std::mem::replace(&mut self.realm.globals, realm);
        self.frames.push(frame);
        let result = self.run_frame_general_with_error(
            &execution_program,
            self.frames.len() - 1,
            initial_error,
        );
        let frame = self.frames.pop().expect("generator frame exists");
        self.active_program = previous_program;
        self.realm.globals = previous_global;
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generator_record_mut(generator) {
                    record.running = false;
                    record.done = true;
                }
                return Err(error);
            }
        };
        match outcome {
            FrameOutcome::Yield {
                value,
                destination,
                delegated_result,
                frame: None,
            } => {
                if let Some(record) = self.generator_record_mut(generator) {
                    record.running = false;
                    record.continuation = Some(Continuation {
                        program: frame.program,
                        function: frame.function,
                        pc: frame.pc,
                        env: frame.env,
                        this: frame.this,
                        locals: frame.locals,
                        registers: frame.registers,
                        active_iterators: frame.active_iterators,
                        completion: Completion::Yield(value),
                        captured: frame.captured,
                        resume_register: Some(destination),
                        promise: Value::UNDEFINED,
                    });
                }
                match delegated_result {
                    Some(result) => Ok(result),
                    None => self.iterator_result(value, false),
                }
            }
            FrameOutcome::Complete(value) | FrameOutcome::ConstructComplete { value, .. } => {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generator_record_mut(generator) {
                    record.running = false;
                    record.done = true;
                }
                self.iterator_result(value, true)
            }
            FrameOutcome::Await { .. } => {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generator_record_mut(generator) {
                    record.running = false;
                    record.done = true;
                }
                Err(JsError(
                    "await is not supported in a synchronous generator".into(),
                ))
            }
            FrameOutcome::ParameterInitializationComplete => {
                self.frame_pool.push(Self::recycle_frame(frame));
                Err(JsError(
                    "unexpected generator parameter initialization boundary".into(),
                ))
            }
            FrameOutcome::Yield {
                frame: Some(frame), ..
            } => {
                self.frame_pool.push(Self::recycle_frame(frame));
                Err(JsError("generator frame retained unexpectedly".into()))
            }
        }
    }

    pub(super) fn async_generator_next(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let promise = self.promise_object();
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.async_generator_next_with_promise(p, generator, value, promise, None)?;
        self.resume_async_generator_queue(p, generator)?;
        Ok(promise)
    }

    pub(super) fn resume_async_generator_queue(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
    ) -> Result<(), JsError> {
        loop {
            let request = match self.generator_record_mut(generator) {
                Some(record) if !record.running => record.requests.pop_front(),
                _ => None,
            };
            let Some(request) = request else {
                return Ok(());
            };
            match request.operation {
                AsyncGeneratorOperation::Next => {
                    self.async_generator_next_with_promise(
                        p,
                        generator,
                        request.value,
                        request.promise,
                        None,
                    )?;
                }
                AsyncGeneratorOperation::Return => {
                    self.async_generator_return_ready(
                        p,
                        generator,
                        request.value,
                        request.promise,
                    )?;
                }
                AsyncGeneratorOperation::Throw => {
                    self.async_generator_throw_ready(p, generator, request.value, request.promise)?;
                }
            }
            if self
                .generator_record_mut(generator)
                .is_some_and(|record| record.running)
            {
                return Ok(());
            }
        }
    }

    fn async_generator_throw_ready(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        value: Value,
        promise: Value,
    ) -> Result<(), JsError> {
        let Some(record) = self.generator_record_mut(generator) else {
            return Err(JsError("async generator receiver is invalid".into()));
        };
        if record.done {
            return self.promise_settle(p, promise, PromiseState::Rejected, value);
        }
        if record.running {
            record.requests.push_back(AsyncGeneratorRequest {
                operation: AsyncGeneratorOperation::Throw,
                promise,
                value,
            });
            return Ok(());
        }
        if let Some((iterator, destination)) = self.yield_star_iterator(p, generator) {
            let completion =
                self.async_generator_delegate(p, generator, iterator, destination, value, true)?;
            return self.forward_promise(p, completion, promise);
        }
        self.async_generator_next_with_promise(
            p,
            generator,
            Value::UNDEFINED,
            promise,
            Some(JsError::thrown(value, "async generator throw".into())),
        )?;
        Ok(())
    }

    fn async_generator_next_with_promise(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        value: Value,
        promise: Value,
        initial_error: Option<JsError>,
    ) -> Result<Value, JsError> {
        let (continuation, realm) = {
            let Some(record) = self.generator_record_mut(generator) else {
                return Err(JsError("async generator receiver is invalid".into()));
            };
            if record.running {
                record.requests.push_back(AsyncGeneratorRequest {
                    operation: AsyncGeneratorOperation::Next,
                    promise,
                    value,
                });
                return Ok(promise);
            }
            if record.done {
                let result = self.iterator_result(Value::UNDEFINED, true)?;
                self.promise_resolve_value(p, promise, result)?;
                return Ok(promise);
            }
            let Some(continuation) = record.continuation.take() else {
                record.running = false;
                return Err(JsError("async generator is suspended".into()));
            };
            record.running = true;
            (continuation, record.realm)
        };
        let execution_program = match self.programs.get(continuation.program) {
            Some(program) => program,
            None => {
                self.fail_async_generator(
                    p,
                    generator,
                    promise,
                    JsError("async generator continuation program is unavailable".into()),
                )?;
                return Ok(promise);
            }
        };
        let mut frame = Frame {
            program: continuation.program,
            function: continuation.function,
            pc: continuation.pc,
            env: continuation.env,
            this: continuation.this,
            locals: continuation.locals,
            dynamic_bindings: vec![],
            captured: continuation.captured,
            registers: continuation.registers,
            active_iterators: continuation.active_iterators,
            with_base: self.with_stack.len(),
        };
        if initial_error.is_none()
            && let Some(register) = continuation.resume_register
        {
            if register as usize >= frame.registers.len() {
                self.fail_async_generator(
                    p,
                    generator,
                    promise,
                    JsError("invalid async generator resume register".into()),
                )?;
                return Ok(promise);
            }
            frame.registers[register as usize] = value;
        }
        let previous_program = std::mem::replace(&mut self.active_program, continuation.program);
        let previous_global = std::mem::replace(&mut self.realm.globals, realm);
        self.frames.push(frame);
        let result = self.run_frame_general_with_error(
            &execution_program,
            self.frames.len() - 1,
            initial_error,
        );
        let frame = self.frames.pop().expect("async generator frame exists");
        self.active_program = previous_program;
        self.realm.globals = previous_global;
        match result {
            Err(error) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                self.fail_async_generator(p, generator, promise, error)?;
            }
            Ok(FrameOutcome::Complete(value) | FrameOutcome::ConstructComplete { value, .. }) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                self.finish_async_generator(p, generator, promise, value, true)?;
            }
            Ok(FrameOutcome::Yield {
                value,
                destination,
                delegated_result,
                frame: None,
            }) => self.async_generator_yield(
                p,
                generator,
                promise,
                frame,
                value,
                destination,
                delegated_result.is_some(),
            )?,
            Ok(FrameOutcome::Await {
                value,
                destination,
                frame: None,
            }) => {
                let continuation = Continuation {
                    program: frame.program,
                    function: frame.function,
                    pc: frame.pc,
                    env: frame.env,
                    this: frame.this,
                    locals: frame.locals,
                    registers: frame.registers,
                    active_iterators: frame.active_iterators,
                    completion: Completion::Await(value),
                    captured: frame.captured,
                    resume_register: Some(destination),
                    promise,
                };
                let id = self.suspend_continuation(continuation);
                self.enqueue_async_resume(p, id, promise, Some(generator), value, false)?;
            }
            Ok(_) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                self.fail_async_generator(
                    p,
                    generator,
                    promise,
                    JsError("async generator frame retained unexpectedly".into()),
                )?;
            }
        }
        Ok(promise)
    }

    pub(super) fn async_generator_yield(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        frame: Frame,
        value: Value,
        destination: u16,
        delegated: bool,
    ) -> Result<(), JsError> {
        let continuation = Continuation {
            program: frame.program,
            function: frame.function,
            pc: frame.pc,
            env: frame.env,
            this: frame.this,
            locals: frame.locals,
            registers: frame.registers,
            active_iterators: frame.active_iterators,
            completion: Completion::Yield(value),
            captured: frame.captured,
            resume_register: Some(destination),
            promise: Value::UNDEFINED,
        };
        if let Some(record) = self.generator_record_mut(generator) {
            record.running = !delegated;
            record.continuation = Some(continuation.clone());
        }
        if delegated {
            let result = self.iterator_result(value, false)?;
            self.promise_resolve_value(p, promise, result)?;
            return Ok(());
        }
        let awaited = self.promise_for_value(p, value)?;
        let id = self.suspend_continuation(continuation);
        self.enqueue_async_resume(p, id, promise, Some(generator), awaited, true)
    }

    pub(super) fn finish_async_generator(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        value: Value,
        done: bool,
    ) -> Result<(), JsError> {
        if let Some(record) = self.generator_record_mut(generator) {
            record.running = false;
            record.done = done;
            record.continuation = None;
        }
        let result = self.iterator_result(value, done)?;
        self.promise_resolve_value(p, promise, result)
    }

    pub(super) fn fail_async_generator(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        error: JsError,
    ) -> Result<(), JsError> {
        if let Some(record) = self.generator_record_mut(generator) {
            record.running = false;
            record.done = true;
            record.continuation = None;
        }
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        self.promise_settle(p, promise, PromiseState::Rejected, reason)
    }
}
