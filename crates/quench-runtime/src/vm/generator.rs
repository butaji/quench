use super::activation::{
    AsyncGeneratorOperation, AsyncGeneratorRequest, Completion, Continuation, GeneratorRecord, JobResume,
};
use super::promise_async::StopOwner;
use crate::heap::AwaitKind;
use super::promise::{PromiseState, ThenCapability};
use super::*;
use super::native_table::NativeFn;
mod job_windows;
mod window_steps;
#[cfg(test)]
mod arguments_tests;
#[cfg(test)]
mod async_await_tests;
#[cfg(test)]
mod eval_tests;
#[cfg(test)]
mod hidden_name_tests;
#[cfg(test)]
mod job_shape_tests;
#[cfg(test)]
mod native_step_tests;
#[cfg(test)]
mod root_tests;
#[cfg(test)]
mod iteration_environment_tests;
#[cfg(test)]
mod window_raise_tests;
#[cfg(test)]
mod with_scope_tests;

/// How a generator's resumption stopped (`Vm::resume_generator_step`).
pub(super) enum GeneratorStep {
    /// At a `yield` of the value.
    Yielded(Value),
    /// At a `yield*` passing its delegate's own result object on.
    Delegated(Value),
    /// Its body completed with the value, or it had completed before.
    Done(Value),
}

/// A generator step whose activation the lane made a frame (`Vm::push_generator_window_frame`):
/// whether the step's result is the object an iteration step reuses
/// (`Vm::reused_iterator_result`) rather than a fresh one. The generator is the receiver of
/// the step's native activation, which keeps it alive while its frame runs.
pub struct GeneratorResume {
    reuses_result: bool,
}

/// The reaction natives a `yield*` delegation hands the awaited result's `then`.
const ASYNC_GENERATOR_DELEGATE_REACTIONS: [Native; 2] =
    [Native::AsyncGeneratorDelegateFulfilled, Native::AsyncGeneratorDelegateRejected];

impl GeneratorResume {
    /// The resume of a step whose result is the reused iteration object or a fresh one.
    pub fn new(reuses_result: bool) -> Self {
        Self { reuses_result }
    }
}

/// How the frame a generator's activation became stopped running: a `yield` of a value, which
/// suspends the frame back into its generator at its resume register (or a `yield*` passing its
/// delegate's result on), or a return.
pub enum MaterializedStop {
    Yielded { destination: crate::bytecode::Register, delegated_result: Option<Value> },
    /// An async generator's frame awaiting in place (`Completion::AsyncRequest`).
    Awaited { destination: crate::bytecode::Register },
    Returned,
}

impl Vm {
    /// The result of a generator step `resume`, whose activation became a frame (the lane's
    /// activations materialize from the record, `push_generator_window_frame`) and whose popped
    /// `frame` stopped at `stop` with `value`, as `resume_generator_step` finishes it: a
    /// yield keeps the frame as the generator's continuation and a return finishes the
    /// generator, each leaving the native activation; the step's iterator result, or the
    /// delegate's own result a `yield*` passes on.
    pub fn finish_materialized_generator_step(
        &mut self,
        frame: Box<FrameRecord>,
        resume: GeneratorResume,
        value: Value,
        stop: MaterializedStop,
    ) -> Value {
        let generator = self.pop_host_frame().expect("a resumed generator's activation");
        let done = match stop {
            MaterializedStop::Yielded { destination, delegated_result } => {
                if let Some(record) = self.generator_record_mut(generator) {
                    record.suspend(Continuation::suspend(frame, Completion::Yield(value), Some(destination), Value::UNDEFINED));
                }
                if let Some(result) = delegated_result {
                    return result;
                }
                false
            }
            MaterializedStop::Awaited { .. } => unreachable!("only an async generator's frame awaits in place"),
            MaterializedStop::Returned => {
                self.pool_frame(frame.into_buffers(self.with_stack.len()));
                if let Some(record) = self.generator_record_mut(generator) {
                    record.complete();
                }
                true
            }
        };
        self.generator_step_result(value, done, resume.reuses_result)
    }

    /// A generator step `resume` whose materialized frame threw: the generator finishes, as
    /// `resume_generator_step` finishes it, and the native activation ends; the caller's
    /// unwind goes on with the error.
    pub(super) fn abandon_materialized_generator_step(&mut self, _resume: GeneratorResume) {
        let generator = self.pop_host_frame().expect("a resumed generator's activation");
        if let Some(record) = self.generator_record_mut(generator) {
            record.complete();
        }
    }
    /// Throws unless `receiver` is a generator that is not running: the receiver check of
    /// `Generator.prototype.{method}`.
    pub(super) fn check_generator_receiver(
        &mut self,
        p: &ResidualProgram,
        method: &str,
        receiver: Value,
    ) -> Result<(), JsError> {
        let is_running = match self.heap.iterator(receiver) {
            Some(IteratorRecord {
                kind: IteratorKind::Generator,
                ext,
                ..
            }) => ext.generator.as_ref().map(|record| record.is_executing()),
            _ => None,
        };
        let Some(is_running) = is_running else {
            return Err(self.type_error(
                p,
                format!("Generator.prototype.{method} called on incompatible receiver"),
            ));
        };
        if is_running {
            return Err(self.type_error(p, "Generator is already executing".into()));
        }
        Ok(())
    }

    pub fn call_generator(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Value, JsError> {
        self.profile
            .function(self.active_program.raw(), id as usize);
        let function = &p.function(id as usize);
        let mut frame = self.pooled_frame();
        // Every local starts undefined, written once, a pooled frame's old values included.
        frame.locals.clear();
        frame.locals.resize(function.locals as usize, Value::UNDEFINED);
        let fixed = usize::from(function.params) - usize::from(function.rest);
        for index in 0..fixed {
            frame.locals[index] = args.get(index).copied().unwrap_or(Value::UNDEFINED);
        }
        if function.rest {
            frame.locals[fixed] = self.heap.alloc_array(crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    args.get(fixed..).unwrap_or_default().to_vec(),
                ));
        }
        if let Some(slot) = function.self_binding_slot {
            frame.locals[usize::from(slot)] = context.callee().unwrap_or(Value::UNDEFINED);
        }
        if let Some(slot) = function.arguments_slot {
            let mapped = function.arguments_are_mapped();
            let arguments = self.heap.alloc_array(crate::heap::ArrayObject::new(
                    Self::empty_object(self.object_proto),
                    args.to_vec(),
                ));
            frame.locals[usize::from(slot)] = arguments;
            self.initialize_arguments_object(arguments, context.callee(), args, mapped)?;
            if function.arguments_alias_parameters()
                && let Some(object) = self.heap.object_mut(arguments) {
                    object.record_mut_departing().set_arguments_map((0..function.params.min(args.len() as u16)).collect());
                }
        }
        self.initialize_frame_invocation(p, &mut frame, function, context, args);
        frame.function = id;
        frame.program = self.active_program;
        frame.pc = 0;
        frame.pc_is_resume = true;
        frame.binding_site_pc = None;
        frame.env = parent;
        frame.this = self.call_this_value(this, function.strict)?;
        self.initialize_activation_bindings(&mut frame, false, Value::UNDEFINED, true);
        frame.captured = false;
        frame.with_base = self.with_stack.len();
        frame.reserve_suspension_window(function);
        // A generator's register file is filled whole, since its suspended registers are
        // scanned whole (`prepare_registers_for`), so a promoted parameter's register is
        // written by this fill and then by `place_parameters`, the one parameter pass here.
        frame.prepare_registers(function.registers, function.initial_register);
        frame.place_parameters(function, args);
        if function.parameter_end_pc != 0 {
            self.frames.push(frame);
            let result = self.run_frame_general_until::<true>(
                p,
                self.frames.len() - 1,
                Some(function.parameter_end_pc as usize),
                None,
                None,
            );
            // A captured run took this frame into its chain (`capture_chain_parked`): none is left to pop.
            if result.as_ref().is_err_and(JsError::is_captured) {
                return Err(result.err().expect("checked above"));
            }
            frame = self.frames.pop().expect("generator parameter frame exists");
            match result {
                Ok(FrameOutcome::ParameterInitializationComplete) => {}
                Ok(_) => {
                    self.pool_frame(frame);
                    return Err(JsError(
                        "generator parameter initialization suspended unexpectedly".into(),
                    ));
                }
                Err(error) => {
                    self.pool_frame(frame);
                    return Err(error);
                }
            }
        }
        frame.with_objects = self.captured_with_objects_for_function(parent, function.inherited_with_scope, p.kind);
        let function_object = context.callee();
        let realm = function_object
            .map(|function| self.function_realm(p, function))
            .transpose()?
            .unwrap_or(self.realm.globals);
        let realm_prototypes = self
            .realm
            .intrinsics
            .iterator_prototypes
            .get(&realm)
            .copied();
        let default_prototype = realm_prototypes
            .map(|prototypes| {
                if function.is_async {
                    prototypes.async_generator
                } else {
                    prototypes.generator
                }
            })
            .unwrap_or(if function.is_async {
                self.async_generator_proto
            } else {
                self.generator_proto
            });
        let prototype_atom = self.prototype_atom();
        // A generator function's own `prototype` is a non-configurable data property, read
        // through its shape's memo (`own_function_prototype_data`).
        let generator_prototype = function_object
            .and_then(|function| {
                self.own_function_prototype_data(function, prototype_atom)
                    .or_else(|| self.own_property(function, prototype_atom))
            })
            .filter(|prototype| self.heap.object(*prototype).is_some())
            .unwrap_or(default_prototype);
        let mut object = Self::empty_object(generator_prototype);
        if let Some(function) = function_object {
            self.capture_constructed_name(&mut object, function, true);
        }
        let generator = self.heap.alloc_iterator(IteratorRecord {
            object,
            source: Value::NULL,
            kind: if function.is_async {
                IteratorKind::AsyncGenerator
            } else {
                IteratorKind::Generator
            },
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt::default(),
        });
        let Some(IteratorRecord { source, ext, .. }) = self.heap.iterator_mut(generator) else {
            self.pool_frame(frame);
            return Err(JsError(
                "generator allocation lost its iterator cell".into(),
            ));
        };
        *source = generator;
        ext.generator = Some(Box::new(GeneratorRecord::new(
            Continuation::suspend(FrameRecord::capture(frame), Completion::GeneratorStart, None, Value::UNDEFINED),
            realm,
        )));
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
            let promise = self.promise_object()?;
            let running = self
                .generator_record_mut(generator)
                .ok_or_else(|| JsError("async generator receiver is invalid".into()))?
                .is_executing();
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
            && let Some((iterator, _)) = self.yield_star_iterator(generator)
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
            if record.is_executing() {
                return Err(JsError("generator is already running".into()));
            }
            record.complete_taking()
        };
        let cleanup = continuation
            .as_ref()
            .map(|continuation| self.close_suspended_iterators(p, continuation))
            .unwrap_or(Ok(()));
        cleanup?;
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
        self.with_call_roots([generator, value, promise], |vm| {
            if let Some((iterator, destination)) = vm.yield_star_iterator(generator) {
                let awaited = match vm.promise_for_value(p, value) {
                    Ok(promise) => promise,
                    Err(error) => {
                        let reason = error
                            .thrown_value()
                            .unwrap_or_else(|| vm.heap.alloc(Cell::Error(error.into_message())));
                        vm.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                        return Ok(());
                    }
                };
                let environment = vm.heap.alloc_array(crate::heap::ArrayObject::new(
                        Self::empty_object(vm.array_proto),
                        vec![
                            generator,
                            iterator,
                            Value::number(f64::from(destination)),
                        ],
                    ));
                let start =
                    vm.native_with_env(Native::AsyncGeneratorDelegateReturnStart, environment);
                let completion = vm.promise_then(p, awaited, start, Value::UNDEFINED)?;
                vm.forward_promise(p, completion, promise)?;
                return Ok(());
            }
            vm.await_async_generator_return(p, generator, value, promise)
        })
    }

    // Direct and delegated returns share awaited completion and the compiled
    // finally/loop cleanup of their original generator frame.
    fn await_async_generator_return(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        value: Value,
        promise: Value,
    ) -> Result<(), JsError> {
        self.with_call_roots([generator, value, promise], |vm| {
            if let Some(record) = vm.generator_record_mut(generator) {
                record.await_return();
            }
            let awaited = match vm.promise_for_value(p, value) {
                Ok(promise) => promise,
                Err(error) => {
                    let reason = error
                        .thrown_value()
                        .unwrap_or_else(|| vm.heap.alloc(Cell::Error(error.into_message())));
                    if let Some(record) = vm.generator_record_mut(generator) {
                        record.stop();
                    }
                    vm.async_generator_throw_ready(p, generator, reason, promise)?;
                    vm.resume_async_generator_queue(p, generator)?;
                    return Ok(());
                }
            };
            let environment = vm.heap.alloc_array(crate::heap::ArrayObject::new(
                    Self::empty_object(vm.array_proto),
                    vec![generator, promise],
                ));
            let natives = [Native::AsyncGeneratorReturnFulfilled, Native::AsyncGeneratorReturnRejected];
            vm.then_with_natives(p, awaited, natives, environment, ThenCapability::Resolving)?;
            Ok(())
        })
    }

    pub(super) fn async_generator_return_fulfilled(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let environment = self
            .active_native_env()
            .ok_or_else(|| JsError("async generator return reaction without state".into()))?;
        let Some(elements) = self.heap.array_elements(environment) else {
            return Err(JsError("async generator return state is invalid".into()));
        };
        let held_values = elements.as_slice();
        let [generator, promise] = &*held_values else {
            return Err(JsError("async generator return state is malformed".into()));
        };
        let (generator, promise) = (*generator, *promise);
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let resume_body = {
            let record = self
                .generator_record_mut(generator)
                .ok_or_else(|| JsError("async generator receiver is invalid".into()))?;
            record.stop();
            record.continuation().is_some_and(|continuation| {
                !matches!(continuation.completion, Completion::GeneratorStart)
            })
        };
        if resume_body && self.prepare_generator_return(generator, value)? {
            self.async_generator_next_with_promise(p, generator, Value::UNDEFINED, promise, None)?;
            self.resume_async_generator_queue(p, generator)?;
            return Ok(Value::UNDEFINED);
        }
        let continuation = self
            .generator_record_mut(generator)
            .and_then(|record| record.take_suspended());
        if resume_body && let Some(continuation) = continuation {
            let cleanup = self.close_suspended_iterators(p, &continuation);
            if let Err(error) = cleanup {
                self.fail_async_generator(p, generator, promise, error)?;
                self.resume_async_generator_queue(p, generator)?;
                return Ok(Value::UNDEFINED);
            }
        }
        if let Some(record) = self.generator_record_mut(generator) {
            record.complete();
        }
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
        let Some(elements) = self.heap.array_elements(environment) else {
            return Err(JsError("async generator return state is invalid".into()));
        };
        let held_values = elements.as_slice();
        let [generator, promise] = &*held_values else {
            return Err(JsError("async generator return state is malformed".into()));
        };
        let (generator, promise) = (*generator, *promise);
        if let Some(record) = self.generator_record_mut(generator) {
            record.stop();
        }
        self.async_generator_throw_ready(
            p,
            generator,
            args.first().copied().unwrap_or(Value::UNDEFINED),
            promise,
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
        let Some(elements) = self.heap.array_elements(environment) else {
            return Err(JsError("async delegate return state is invalid".into()));
        };
        let held_values = elements.as_slice();
        let [generator, iterator, destination] = &*held_values else {
            return Err(JsError("async delegate return state is malformed".into()));
        };
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(IteratorRecord {
            source,
            kind: IteratorKind::AsyncFromSync,
            ..
        }) = self.heap.iterator(*iterator)
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
        if !self.is_promise(source) {
            return Err(JsError("async generator request result is not a Promise".into()));
        }
        self.attach_promise_reaction(p, source, Value::UNDEFINED, Value::UNDEFINED, target)
    }

    pub(super) fn yield_star_iterator(&self, generator: Value) -> Option<(Value, u16)> {
        let record = match self.heap.iterator(generator) {
            Some(IteratorRecord { ext, .. }) => ext.generator.as_ref()?,
            _ => return None,
        };
        let continuation = record.continuation()?;
        let program = self.programs.get(continuation.frame.program)?;
        let function = program.function_get(continuation.frame.function as usize)?;
        let instruction = *function.code.get(continuation.frame.pc)?;
        let instruction = if instruction.is_wide() {
            function.wide.get(instruction.wide_index()).copied()?
        } else {
            instruction.as_wide()
        };
        if !instruction.op().is_yield_star() {
            return None;
        }
        let iterator = *continuation
            .frame
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
        let done_atom = self.runtime_atoms.done;
        let done = match self.get_property(p, result, done_atom) {
            Ok(done) => done,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if !self.truthy(done) {
            return Ok(result);
        }
        let value_atom = self.runtime_atoms.value;
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
        if record.is_executing() {
            return Err(JsError("generator is already running".into()));
        }
        if record.is_completed() {
            return self.iterator_result(value, true);
        }
        if self.prepare_generator_return(generator, value)? {
            return self.resume_generator(p, generator, &[], None);
        }
        let continuation = self
            .generator_record_mut(generator)
            .and_then(|record| record.continuation())
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
        generator: Value,
        value: Value,
    ) -> Result<bool, JsError> {
        let unwind = {
            let Some(record) = (match self.heap.iterator(generator) {
                Some(IteratorRecord { ext, .. }) => ext.generator.as_ref(),
                _ => None,
            }) else {
                return Err(JsError("generator receiver is invalid".into()));
            };
            let Some(continuation) = record.continuation() else {
                return Err(JsError("generator is suspended".into()));
            };
            if matches!(continuation.completion, Completion::GeneratorStart) {
                return Ok(false);
            }
            let program = self
                .programs
                .get(continuation.frame.program)
                .ok_or_else(|| JsError("generator continuation program is unavailable".into()))?;
            let Some(function) = program.function_get(continuation.frame.function as usize) else {
                return Err(JsError("generator function is invalid".into()));
            };
            let suspended_instruction = continuation.frame.pc.saturating_sub(1) as u32;
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
                        continuation.frame.captured,
                        continuation.frame.env,
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
            let Some(target_slot) = self.heap.environment_slot_mut(env, usize::from(slot)) else {
                return Err(JsError("generator return slot is out of bounds".into()));
            };
            *target_slot = value;
        } else {
            let Some(record) = self.generator_record_mut(generator) else {
                return Err(JsError("generator receiver is invalid".into()));
            };
            let Some(continuation) = record.continuation_mut() else {
                return Err(JsError("generator is suspended".into()));
            };
            let Some(target_slot) = continuation.frame.locals.get_mut(usize::from(slot)) else {
                return Err(JsError("generator return slot is out of bounds".into()));
            };
            *target_slot = value;
        }
        if let Some(continuation) = self
            .generator_record_mut(generator)
            .and_then(|record| record.continuation_mut())
        {
            continuation.frame.pc = target as usize;
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
        let promise = self.promise_object()?;
        let atom = self.intern_atom("return");
        let method = match self.get_property(p, iterator, atom) {
            Ok(method) => method,
            Err(error) => {
                self.fail_async_generator(p, generator, promise, error)?;
                return Ok(promise);
            }
        };
        if method.is_undefined() || method.is_null() {
            self.await_async_generator_return(p, generator, value, promise)?;
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
        let env = self.heap.alloc_array(crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                vec![
                    generator,
                    Value::number(f64::from(destination)),
                    Value::FALSE,
                    Value::FALSE,
                    Value::TRUE,
                ],
            ));
        self.then_with_natives(p, unwrapped, ASYNC_GENERATOR_DELEGATE_REACTIONS, env, ThenCapability::Species)
    }

    fn close_suspended_iterators(
        &mut self,
        p: &ResidualProgram,
        continuation: &Continuation,
    ) -> Result<(), JsError> {
        self.with_call_roots(continuation.roots(), |vm| {
            let mut first_error = None;
            for cleanup in continuation.frame.active_iterators.iter().rev() {
                let Some(&done) = continuation.frame.registers.get(usize::from(cleanup.done)) else {
                    first_error
                        .get_or_insert_with(|| JsError("invalid iterator cleanup state".into()));
                    continue;
                };
                if vm.truthy(done) {
                    continue;
                }
                let Some(&iterator) = continuation.frame.registers.get(usize::from(cleanup.iterator))
                else {
                    first_error
                        .get_or_insert_with(|| JsError("invalid iterator cleanup target".into()));
                    continue;
                };
                if let Err(error) = vm.iterator_close(p, iterator) {
                    first_error.get_or_insert(error);
                }
            }
            first_error.map_or(Ok(()), Err)
        })
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
            if let Some((iterator, destination)) = self.yield_star_iterator(generator) {
                self.async_generator_delegate(p, generator, iterator, destination, value, true)
            } else {
                let promise = self.promise_object()?;
                let (running, done) = self
                    .generator_record_mut(generator)
                    .map(|record| (record.is_executing(), record.is_completed()))
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
        } else if let Some((iterator, destination)) = self.yield_star_iterator(generator) {
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
            self.heap.iterator(receiver),
            Some(IteratorRecord {
                kind: IteratorKind::AsyncGenerator,
                ext,
                ..
            }) if ext.generator.is_some()
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
            let promise = self.with_call_roots([reason], |vm| vm.promise_object())?;
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
        let (receiver, adapter) = match self.heap.iterator(iterator) {
            Some(IteratorRecord {
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
                        record.complete();
                    }
                    return Err(self.type_error(p, "delegated iterator has no throw method".into()));
                }
                let promise = self.promise_object()?;
                self.await_async_generator_return(p, generator, argument, promise)?;
                return Ok(promise);
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
            let env = self.heap.alloc_array(crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    vec![
                        generator,
                        Value::number(f64::from(destination)),
                        Value::number(if throwing { 1.0 } else { 0.0 }),
                        Value::number(if adapter { 1.0 } else { 0.0 }),
                        if async_from_sync {
                            Value::TRUE
                        } else {
                            Value::FALSE
                        },
                    ],
                ));
            self.then_with_natives(p, promise, ASYNC_GENERATOR_DELEGATE_REACTIONS, env, ThenCapability::Species)
        })();
        match operation {
            Ok(promise) => Ok(promise),
            Err(error) => {
                if async_from_sync {
                    if let Some(record) = self.generator_record_mut(generator) {
                        record.complete();
                    }
                    let reason = error
                        .thrown_value()
                        .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                    let promise = self.with_call_roots([reason], |vm| vm.promise_object())?;
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
        let Some(elements) = self.heap.array_elements(env) else {
            return Err(JsError("async delegate reaction state is invalid".into()));
        };
        let held_values = elements.as_slice();
        let [generator, destination, throwing, adapter, _async_from_sync] = &*held_values
        else {
            return Err(JsError("async delegate reaction state is malformed".into()));
        };
        let (generator, destination, throwing, adapter) =
            (*generator, *destination, *throwing, *adapter);
        if !self.is_object_like(result) {
            let error = self.type_error(p, "iterator result is not an object".into());
            return self.resume_async_generator_with_throw(p, generator, error);
        }
        let done_atom = self.runtime_atoms.done;
        let done_value = match self.get_property(p, result, done_atom) {
            Ok(value) => value,
            Err(error) => return self.resume_async_generator_with_throw(p, generator, error),
        };
        let done = self.truthy(done_value);
        let value_atom = self.runtime_atoms.value;
        let value = match self.get_property(p, result, value_atom) {
            Ok(value) => value,
            Err(error) => return self.resume_async_generator_with_throw(p, generator, error),
        };
        let value = if self.truthy(adapter) {
            let resolved = self.promise_for_value(p, value)?;
            let state = self
                .promise_record(resolved)
                .cloned()
                .ok_or_else(|| JsError("async-from-sync value lost its Promise".into()))?;
            match state.state() {
                PromiseState::Fulfilled => state.result(),
                PromiseState::Rejected => {
                    return Err(JsError::thrown(
                        state.result(),
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
                && let Some(continuation) = record.continuation_mut()
            {
                continuation.frame.pc += 1;
                continuation.resume_register = None;
                if let Some(slot) = continuation
                    .frame
                    .registers
                    .get_mut(destination.as_number().unwrap_or(0.0) as usize)
                {
                    *slot = value;
                }
            }
            self.async_generator_next(p, generator, &[])
        } else {
            let promise = self.promise_object()?;
            self.await_async_generator_return(p, generator, value, promise)?;
            Ok(promise)
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
        let Some(elements) = self.heap.array_elements(env) else {
            return Err(JsError("async delegate rejection state is invalid".into()));
        };
        let generator = elements
            .first()
            
            .ok_or_else(|| JsError("async delegate rejection state is malformed".into()))?;
        let async_from_sync = elements.get(4).is_some_and(|value| self.truthy(value));
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
            record.complete();
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
            Some(record) => record.resume(),
            None => None,
        };
        let Some(mut continuation) = continuation else {
            let promise = self.with_call_roots([reason], |vm| vm.promise_object())?;
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
            return Ok(promise);
        };
        continuation.frame.pc += 1;
        continuation.resume_register = None;
        let mut roots = continuation.roots();
        roots.extend([generator, reason]);
        let (rejected, promise) = self.with_call_roots(roots, |vm| {
            let rejected = vm.promise_object()?;
            vm.with_call_roots([rejected], |vm| {
                vm.promise_settle(p, rejected, PromiseState::Rejected, reason)?;
                let promise = vm.promise_object()?;
                Ok((rejected, promise))
            })
        })?;
        self.async_generator_await(p, generator, promise, continuation, rejected)?;
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
        let done_atom = self.runtime_atoms.done;
        let done = match self.get_property(p, result, done_atom) {
            Ok(done) => self.truthy(done),
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        if !done {
            return Ok(result);
        }
        let value_atom = self.runtime_atoms.value;
        let value = match self.get_property(p, result, value_atom) {
            Ok(value) => value,
            Err(error) => return self.resume_generator(p, generator, &[], Some(error)),
        };
        let epoch = self.heap.collection_epoch(); // after the reads, which may collect
        if let Some(record) = self.generator_record_mut(generator) && let Some(continuation) = record.continuation_mut() {
            continuation.frame.pc += 1;
            continuation.resume_register = None;
            // The written register need not be live at the suspension the mask describes.
            continuation.frame.settle_registers(epoch);
            if let Some(register) = continuation.frame.registers.get_mut(usize::from(destination)) {
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
            .yield_star_iterator(generator)
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
        match self.heap.iterator(generator) {
            Some(IteratorRecord { kind, .. })
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
        if record.is_executing() {
            return Err(JsError("generator is already running".into()));
        }
        record.complete();
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
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
        initial_error: Option<JsError>,
    ) -> Result<Value, JsError> {
        match self.resume_generator_step(p, generator, args, initial_error)? {
            GeneratorStep::Yielded(value) => self.iterator_result(value, false),
            GeneratorStep::Done(value) => self.iterator_result(value, true),
            GeneratorStep::Delegated(result) => Ok(result),
        }
    }

    /// Resumes `generator` as `next`, `return` or `throw` do, and says how it stopped, so a
    /// caller that only reads the result's `value` and `done` (`Vm::generator_iteration_step`)
    /// need not allocate one. The step is the resumption every suspended activation takes
    /// (`Vm::resume_suspended`), a lane run rooted at the generator's record, in its realm.
    pub(super) fn resume_generator_step(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
        initial_error: Option<JsError>,
    ) -> Result<GeneratorStep, JsError> {
        let (realm, done, unstarted) = self.generator_resumption(generator)?;
        if done {
            return initial_error.map_or(Ok(GeneratorStep::Done(Value::UNDEFINED)), Err);
        }
        let owner = StopOwner::Generator(generator);
        if self.job_frame(owner.job()).is_none() {
            return Err(JsError("generator is suspended".into()));
        }
        if unstarted && let Some(error) = initial_error {
            return self.close_unstarted_generator(generator, error);
        }
        let how = match initial_error {
            None => JobResume::Sends(args.first().copied().unwrap_or(Value::UNDEFINED)),
            Some(error) => JobResume::Throws(error),
        };
        let previous_global = self.switch_realm_global(realm);
        let step = self.resume_suspended(p, owner, how);
        self.switch_realm_global(previous_global);
        Ok(step?.expect("a sync generator's stop is its step"))
    }

    /// What a resumption of `generator` finds: its realm, whether it has completed and whether
    /// it has not started; an error where it is no generator or is running.
    fn generator_resumption(&mut self, generator: Value) -> Result<(Value, bool, bool), JsError> {
        let Some(record) = self.generator_record_mut(generator) else {
            return Err(JsError("generator receiver is invalid".into()));
        };
        if record.is_executing() {
            return Err(JsError("generator is already running".into()));
        }
        let unstarted = record
            .suspended_continuation()
            .is_some_and(|continuation| matches!(continuation.completion, Completion::GeneratorStart));
        Ok((record.realm, record.is_completed(), unstarted))
    }

    /// An abrupt completion `error` of a generator that has not started: it closes without
    /// running, leaving no frame, and the caller raises the error.
    fn close_unstarted_generator(&mut self, generator: Value, error: JsError) -> Result<GeneratorStep, JsError> {
        let continuation = self.generator_record_mut(generator).and_then(GeneratorRecord::take_suspended);
        if let Some(continuation) = continuation {
            self.pool_frame(continuation.frame.into_frame(self.with_stack.len()));
        }
        self.close_generator(generator)?;
        Err(error)
    }

    /// The step of `generator` whose frame yielded a value, `(value, destination)`, the register its
    /// next sent value goes to: the frame is the generator's continuation, and a `yield*` passes its
    /// delegate's own result on (`Vm::settle_stop`).
    pub(super) fn park_generator_frame(
        &mut self,
        generator: Value,
        frame: Box<FrameRecord>,
        (value, destination): (Value, crate::bytecode::Register),
        delegated_result: Option<Value>,
    ) -> GeneratorStep {
        let continuation = self.suspend_activation(frame, Completion::Yield(value), Some(destination), Value::UNDEFINED);
        if let Some(record) = self.generator_record_mut(generator) {
            record.suspend(continuation);
        }
        match delegated_result {
            Some(result) => GeneratorStep::Delegated(result),
            None => GeneratorStep::Yielded(value),
        }
    }

    pub(super) fn complete_generator(&mut self, generator: Value) {
        if let Some(record) = self.generator_record_mut(generator) {
            record.complete();
        }
    }

    pub(super) fn async_generator_next(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let promise = self.promise_object()?;
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
                Some(record) if !record.is_executing() => record.next_request(),
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
            // A request that left the generator executing ends the loop at the next pop.
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
        if record.is_completed() {
            return self.promise_settle(p, promise, PromiseState::Rejected, value);
        }
        if record.is_executing() {
            record.requests.push_back(AsyncGeneratorRequest {
                operation: AsyncGeneratorOperation::Throw,
                promise,
                value,
            });
            return Ok(());
        }
        if let Some((iterator, destination)) = self.yield_star_iterator(generator) {
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

    /// The request `promise` of an async generator resumes it with `value`, or with
    /// `initial_error` thrown where it suspended: its frame stays in its record, which runs it
    /// (`Vm::resume_request`). A request of a running generator queues, and of a completed one
    /// is answered at once.
    fn async_generator_next_with_promise(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        value: Value,
        promise: Value,
        initial_error: Option<JsError>,
    ) -> Result<Value, JsError> {
        self.with_call_roots([generator, value, promise], |vm| {
            let (starting, program, realm) = {
                let Some(record) = vm.generator_record_mut(generator) else {
                    return Err(JsError("async generator receiver is invalid".into()));
                };
                if record.is_executing() {
                    record.requests.push_back(AsyncGeneratorRequest {
                        operation: AsyncGeneratorOperation::Next,
                        promise,
                        value,
                    });
                    return Ok(promise);
                }
                if record.is_completed() {
                    vm.resolve_iterator_result(p, promise, Value::UNDEFINED, true)?;
                    return Ok(promise);
                }
                let Some(continuation) = record.suspended_continuation() else {
                    return Err(JsError("async generator is suspended".into()));
                };
                (matches!(continuation.completion, Completion::GeneratorStart), continuation.program(), record.realm)
            };
            // A generator that has not started closes without running its body.
            if starting
                && let Some(error) = initial_error
            {
                vm.fail_async_generator(p, generator, promise, error)?;
                return Ok(promise);
            }
            if vm.programs.get(program).is_none() {
                let error = JsError("async generator continuation program is unavailable".into());
                vm.fail_async_generator(p, generator, promise, error)?;
                return Ok(promise);
            }
            let how = match initial_error {
                Some(error) => JobResume::Throws(error),
                None => JobResume::Sends(value),
            };
            let previous_global = vm.switch_realm_global(realm);
            let resumed = vm.resume_request(p, generator, promise, how);
            vm.switch_realm_global(previous_global);
            resumed?;
            Ok(promise)
        })
    }

    /// An async generator whose activation awaits `value` inside its body, or is resumed with a
    /// rejection through an await: its frame parks in the record, busy, and the reaction
    /// holds the generator (`AwaitKind::generator_await`).
    pub(super) fn async_generator_await(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        continuation: Continuation,
        value: Value,
    ) -> Result<(), JsError> {
        if let Some(record) = self.generator_record_mut(generator) {
            record.await_with(continuation);
        }
        self.enqueue_async_resume(p, AwaitKind::generator_await(generator), promise, value)
    }

    pub(super) fn async_generator_yield(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        frame: Box<FrameRecord>,
        value: Value,
        destination: u16,
        delegated: bool,
    ) -> Result<(), JsError> {
        let continuation = self.suspend_activation(
            frame,
            Completion::Yield(value),
            Some(destination),
            Value::UNDEFINED,
        );
        if let Some(record) = self.generator_record_mut(generator) {
            if delegated {
                record.suspend(continuation);
            } else {
                record.await_with(continuation);
            }
        }
        if delegated {
            self.resolve_iterator_result(p, promise, value, false)?;
            return Ok(());
        }
        // The yield awaits its value first; its continuation stays in the record.
        self.enqueue_async_resume(p, AwaitKind::AsyncGeneratorYield { generator }, promise, value)
    }

    pub(super) fn finish_async_generator(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if let Some(record) = self.generator_record_mut(generator) {
            record.complete();
        }
        self.resolve_iterator_result(p, promise, value, true)
    }

    pub(super) fn fail_async_generator(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        error: JsError,
    ) -> Result<(), JsError> {
        if let Some(record) = self.generator_record_mut(generator) {
            record.complete();
        }
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        self.promise_settle(p, promise, PromiseState::Rejected, reason)
    }
}

#[cfg(test)]
mod call_completion_tests {
    use super::*;

    #[test]
    fn generator_completion_moves_resume_state_once() {
        for reuses_result in [false, true] {
            let mut completion = CallCompletion::Generator(GeneratorResume { reuses_result });
            assert!(completion.take_promise().is_none());
            assert!(completion.is_generator());
            let resume = completion.take_generator().expect("generator completion");
            assert_eq!(resume.reuses_result, reuses_result);
            assert!(completion.take_generator().is_none());
            assert!(matches!(completion, CallCompletion::Value));
        }
    }
}

/// The entry that runs a synchronous generator's `next`, `return` or `throw`, or `None` when the
/// generator family does not claim `native`.
pub(super) fn entry(native: Native) -> Option<NativeFn> {
    match native {
        Native::GeneratorNext => Some(generator_next_native),
        Native::GeneratorReturn => Some(generator_return_native),
        Native::GeneratorThrow => Some(generator_throw_native),
        _ => None,
    }
}

fn generator_next_native(
    vm: &mut Vm,
    p: &ResidualProgram,
    _native: Native,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    vm.check_generator_receiver(p, "next", this)?;
    vm.generator_next(p, this, args)
}

fn generator_return_native(
    vm: &mut Vm,
    p: &ResidualProgram,
    _native: Native,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    vm.check_generator_receiver(p, "return", this)?;
    vm.generator_return(p, this, args)
}

fn generator_throw_native(
    vm: &mut Vm,
    p: &ResidualProgram,
    _native: Native,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    vm.check_generator_receiver(p, "throw", this)?;
    vm.generator_throw(p, this, args)
}
