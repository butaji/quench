use super::activation::{Completion, Continuation, GeneratorRecord};
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
            object: Self::empty_object(self.iterator_proto),
            source: Value::NULL,
            kind: IteratorKind::Generator,
            index: 0,
        });
        if let Some(Cell::Iterator { source, .. }) = self.heap.get_mut(generator) {
            *source = generator;
        }
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

    pub(super) fn generator_next(
        &mut self,
        p: &ResidualProgram,
        generator: Value,
        args: &[Value],
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
        };
        if let Some(register) = continuation.resume_register {
            if register as usize >= frame.registers.len() {
                return Err(JsError("invalid generator resume register".into()));
            }
            frame.registers[register as usize] = args.first().copied().unwrap_or(Value::UNDEFINED);
        }
        self.frames.push(frame);
        let result = self.run_frame_general(p, self.frames.len() - 1);
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
}
