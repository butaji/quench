use super::activation::{Completion, Continuation, GeneratorRecord};
use super::promise::PromiseState;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn call_generator(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        parent: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.profile.function(id as usize);
        let function = &p.functions[id as usize];
        let mut frame = self.frame_pool.pop().unwrap_or(Frame {
            function: 0,
            pc: 0,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            captured: false,
            registers: vec![],
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
                    object.arguments_map = Some(
                        (0..function.params.min(args.len() as u16)).collect(),
                    );
                }
            }
        }
        frame.function = id;
        frame.pc = 0;
        frame.env = parent;
        frame.this = this;
        frame.captured = false;
        let register_count = function.registers as usize;
        if frame.registers.capacity() < register_count {
            frame
                .registers
                .reserve_exact(register_count - frame.registers.len());
        }
        unsafe { frame.registers.set_len(register_count) };
        let generator = self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(if function.is_async {
                self.async_iterator_proto
            } else {
                self.iterator_proto
            }),
            source: Value::NULL,
            kind: if function.is_async {
                IteratorKind::AsyncGenerator
            } else {
                IteratorKind::Generator
            },
            index: 0,
        });
        if let Some(Cell::Iterator { source, .. }) = self.heap.get_mut(generator) {
            *source = generator;
        }
        let root = self.heap.root(generator);
        self.set_named(
            p,
            generator,
            "return",
            self.native_value(Native::IteratorReturn),
        )?;
        self.set_named(
            p,
            generator,
            "throw",
            self.native_value(Native::IteratorThrow),
        )?;
        self.heap.release_root(root);
        self.generators.insert(
            generator,
            GeneratorRecord {
                continuation: Some(Continuation {
                    function: frame.function,
                    pc: frame.pc,
                    env: frame.env,
                    this: frame.this,
                    locals: std::mem::take(&mut frame.locals),
                    registers: std::mem::take(&mut frame.registers),
                    completion: Completion::Yield(Value::UNDEFINED),
                    captured: frame.captured,
                    resume_register: None,
                    promise: Value::UNDEFINED,
                }),
                done: false,
                running: false,
            },
        );
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
        let promise = (kind == IteratorKind::AsyncGenerator).then(|| self.promise_object());
        self.close_generator(generator)?;
        let result = self.iterator_result(value, true)?;
        if let Some(promise) = promise {
            self.promise_resolve_value(p, promise, result)?;
            Ok(promise)
        } else {
            Ok(result)
        }
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
            self.close_generator(generator)?;
            let promise = self.promise_object();
            self.promise_settle(p, promise, PromiseState::Rejected, value)?;
            Ok(promise)
        } else {
            self.resume_generator(
                p,
                generator,
                &[],
                Some(JsError::thrown(value, "generator throw".into())),
            )
        }
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
        let Some(record) = self.generators.get_mut(&generator) else {
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
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
        initial_error: Option<JsError>,
    ) -> Result<Value, JsError> {
        let (continuation, done, running) = {
            let Some(record) = self.generators.get_mut(&generator) else {
                return Err(JsError("generator receiver is invalid".into()));
            };
            if record.running {
                return Err(JsError("generator is already running".into()));
            }
            (record.continuation.take(), record.done, record.running)
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
        if let Some(record) = self.generators.get_mut(&generator) {
            record.running = true;
        }
        let mut frame = Frame {
            function: continuation.function,
            pc: continuation.pc,
            env: continuation.env,
            this: continuation.this,
            locals: continuation.locals,
            captured: continuation.captured,
            registers: continuation.registers,
            with_base: self.with_stack.len(),
        };
        if initial_error.is_none()
            && let Some(register) = continuation.resume_register
        {
            if register as usize >= frame.registers.len() {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generators.get_mut(&generator) {
                    record.running = false;
                    record.done = true;
                }
                return Err(JsError("invalid generator resume register".into()));
            }
            frame.registers[register as usize] = args.first().copied().unwrap_or(Value::UNDEFINED);
        }
        self.frames.push(frame);
        let result = self.run_frame_general_with_error(p, self.frames.len() - 1, initial_error);
        let frame = self.frames.pop().expect("generator frame exists");
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generators.get_mut(&generator) {
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
                frame: None,
            } => {
                if let Some(record) = self.generators.get_mut(&generator) {
                    record.running = false;
                    record.continuation = Some(Continuation {
                        function: frame.function,
                        pc: frame.pc,
                        env: frame.env,
                        this: frame.this,
                        locals: frame.locals,
                        registers: frame.registers,
                        completion: Completion::Yield(value),
                        captured: frame.captured,
                        resume_register: Some(destination),
                        promise: Value::UNDEFINED,
                    });
                }
                self.iterator_result(value, false)
            }
            FrameOutcome::Complete(value) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generators.get_mut(&generator) {
                    record.running = false;
                    record.done = true;
                }
                self.iterator_result(value, true)
            }
            FrameOutcome::Await { .. } => {
                self.frame_pool.push(Self::recycle_frame(frame));
                if let Some(record) = self.generators.get_mut(&generator) {
                    record.running = false;
                    record.done = true;
                }
                Err(JsError(
                    "await is not supported in a synchronous generator".into(),
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
        let continuation = {
            let Some(record) = self.generators.get_mut(&generator) else {
                return Err(JsError("async generator receiver is invalid".into()));
            };
            if record.running {
                let error = self
                    .heap
                    .alloc(Cell::Error("async generator is already running".into()));
                self.promise_settle(p, promise, PromiseState::Rejected, error)?;
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
            continuation
        };
        let mut frame = Frame {
            function: continuation.function,
            pc: continuation.pc,
            env: continuation.env,
            this: continuation.this,
            locals: continuation.locals,
            captured: continuation.captured,
            registers: continuation.registers,
            with_base: self.with_stack.len(),
        };
        if let Some(register) = continuation.resume_register {
            if register as usize >= frame.registers.len() {
                self.fail_async_generator(
                    p,
                    generator,
                    promise,
                    JsError("invalid async generator resume register".into()),
                )?;
                return Ok(promise);
            }
            frame.registers[register as usize] = args.first().copied().unwrap_or(Value::UNDEFINED);
        }
        self.frames.push(frame);
        let result = self.run_frame_general(p, self.frames.len() - 1);
        let frame = self.frames.pop().expect("async generator frame exists");
        match result {
            Err(error) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                self.fail_async_generator(p, generator, promise, error)?;
            }
            Ok(FrameOutcome::Complete(value)) => {
                self.frame_pool.push(Self::recycle_frame(frame));
                self.finish_async_generator(p, generator, promise, value, true)?;
            }
            Ok(FrameOutcome::Yield {
                value,
                destination,
                frame: None,
            }) => self.async_generator_yield(p, generator, promise, frame, value, destination)?,
            Ok(FrameOutcome::Await {
                value,
                destination,
                frame: None,
            }) => {
                let continuation = Continuation {
                    function: frame.function,
                    pc: frame.pc,
                    env: frame.env,
                    this: frame.this,
                    locals: frame.locals,
                    registers: frame.registers,
                    completion: Completion::Await(value),
                    captured: frame.captured,
                    resume_register: Some(destination),
                    promise,
                };
                let id = self.suspend_continuation(continuation);
                self.enqueue_async_resume(p, id, promise, Some(generator), value)?;
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
    ) -> Result<(), JsError> {
        if let Some(record) = self.generators.get_mut(&generator) {
            record.running = false;
            record.continuation = Some(Continuation {
                function: frame.function,
                pc: frame.pc,
                env: frame.env,
                this: frame.this,
                locals: frame.locals,
                registers: frame.registers,
                completion: Completion::Yield(value),
                captured: frame.captured,
                resume_register: Some(destination),
                promise: Value::UNDEFINED,
            });
        }
        let result = self.iterator_result(value, false)?;
        self.promise_resolve_value(p, promise, result)
    }

    pub(super) fn finish_async_generator(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        promise: Value,
        value: Value,
        done: bool,
    ) -> Result<(), JsError> {
        if let Some(record) = self.generators.get_mut(&generator) {
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
        if let Some(record) = self.generators.get_mut(&generator) {
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
