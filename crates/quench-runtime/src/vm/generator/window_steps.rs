//! A generator the lane runs as an activation over its suspended frame's register file
//! (`FrameRecord`): the record keeps the frame while the activation runs, so a resume
//! makes no `Box<Frame>` and copies no register, and a yield suspends the frame in place.
use super::*;
use super::super::activation::FrameRecord;

impl<H: Host> Vm<H> {
    /// The frame of `generator` the lane resumes as an activation over its register file, which
    /// `code`, the running program's, lays out as its registers or with an environment window
    /// after them (`window_layout`, the `bool`): a suspended generator of this realm and
    /// program that is not running or done.
    pub(in crate::vm) fn generator_window_frame(&self, generator: Value, code: &ResidualProgram) -> Option<(&FrameRecord, bool)> {
        let IteratorRecord { ext, .. } = self.heap.iterator(generator)?;
        let record = ext.generator.as_deref()?;
        let frame = &*record.suspended_continuation()?.frame;
        let environment = self.window_layout(frame, code)?;
        (record.realm == self.realm.globals).then_some((frame, environment))
    }

    /// Whether the lane resumes `frame`, suspended by a function of `code`, over its register
    /// file, and whether its window carries an environment after the registers: a frame of the
    /// active program whose function lays out as its registers or with an environment window
    /// (`CallPlan::suspended_registers`, `CallPlan::suspended_environment`).
    pub(in crate::vm) fn window_layout(&self, frame: &FrameRecord, code: &ResidualProgram) -> Option<bool> {
        let plan = code.function_get(frame.function as usize)?.call_plan(code);
        (frame.program == self.active_program && (plan.suspended_registers || plan.suspended_environment))
            .then_some(plan.suspended_environment)
    }

    /// The receiver a window over `frame`'s record runs with, resumed under the environment
    /// `parent`: the frame's own, or an arrow's lexical `this`, which a `super()` since the frame
    /// suspended may have initialized. `None` where it is still uninitialized, which only the
    /// general path's dead-zone check raises from.
    pub(in crate::vm) fn window_this(&self, body: &crate::bytecode::Function, this: Value, parent: Value) -> Option<Value> {
        if !body.is_arrow {
            return Some(this);
        }
        match self.captured_lexical_this(parent) {
            Some(lexical) => (!lexical.is_deleted()).then_some(lexical),
            None => Some(this),
        }
    }

    /// Starts a windowed run of `generator`, which `generator_window_frame` admitted: the
    /// frame stays in its record, `sent` is the value the suspending `yield` evaluates to, and
    /// the native activation roots the generator while the run lasts. The frame's address, which
    /// its box keeps until the run ends.
    pub(in crate::vm) fn resume_generator_window(
        &mut self,
        method: Value,
        generator: Value,
        sent: Value,
        depth: usize,
    ) -> Option<*mut FrameRecord> {
        let (frame, resume_register) = self.generator_record_mut(generator)?.resume_windowed()?;
        // SAFETY: the record owns the box, which nothing moves or drops until the run ends.
        unsafe { &mut *frame }.receive(resume_register, sent);
        // The frames the lane's activations become sit below the generator's.
        self.realm.promise.active_native.push(super::super::activation::NativeActivation {
            callable: method,
            receiver: generator,
            frame_depth: self.frames.len() + depth,
            boundary: super::super::activation::NativeCallBoundary::Opaque,
        });
        Some(frame)
    }

    /// Ends the windowed run of `generator` at a `yield` that resumes at `pc`, whose value goes to
    /// `destination` when it resumes: the frame is suspended where it stands, its dead registers
    /// cleared, and takes back the iterators its loops pushed on the lane stack for the
    /// activation at `depth`.
    pub(in crate::vm) fn suspend_generator_window(
        &mut self,
        generator: Value,
        pc: usize,
        destination: crate::bytecode::Register,
        value: Value,
        depth: usize,
    ) {
        self.realm.promise.active_native.pop();
        let mut frame = self
            .generator_record_mut(generator)
            .and_then(GeneratorRecord::take_windowed)
            .expect("a windowed yield belongs to a windowed generator");
        self.park_frame(&mut frame, pc, depth);
        let continuation = Continuation { frame, completion: Completion::Yield(value), resume_register: Some(destination), promise: Value::UNDEFINED };
        if let Some(record) = self.generator_record_mut(generator) {
            record.suspend(continuation);
        }
    }

    /// Suspends the frame of a windowed run at `pc`: the environment window closed, its dead
    /// registers cleared, and the iterators and `with` objects the activation at `depth` holds
    /// taken back.
    pub(in crate::vm) fn park_frame(&mut self, frame: &mut FrameRecord, pc: usize, depth: usize) {
        frame.close_environment_window();
        let live_registers = self.release_dead_registers(frame.program, frame.function, pc, &mut frame.registers);
        let with_objects = self.lane_stack.drain_with_objects(depth);
        frame.suspend_at(pc, live_registers, self.lane_stack.drain_iterators(depth), with_objects.into_iter());
    }

    /// Ends the windowed run of `generator` at its return: it completes, dropping its frame.
    pub(in crate::vm) fn finish_generator_window(&mut self, generator: Value) {
        self.realm.promise.active_native.pop();
        if let Some(record) = self.generator_record_mut(generator) {
            record.complete();
        }
    }

    /// The frame of `generator`'s windowed run, for the general loop to continue in where the
    /// lane's activations become frames: pushed with the iterators its loops pushed since the
    /// lane resumed it (`iterators`).
    pub(in crate::vm) fn push_generator_window_frame(
        &mut self,
        generator: Value,
        iterators: impl Iterator<Item = ActiveIterator>,
        with_objects: impl Iterator<Item = Value>,
    ) {
        let frame = self
            .generator_record_mut(generator)
            .and_then(GeneratorRecord::take_windowed)
            .expect("a windowed generator's activation materializes");
        self.push_resumed_frame(frame, iterators, with_objects);
    }

    /// The frame of a windowed run's record, made a VM frame on the frame stack with the
    /// iterators the lane's loops pushed and the `with` objects its scope chain holds since the
    /// lane resumed it.
    pub(in crate::vm) fn push_resumed_frame(
        &mut self,
        mut frame: Box<FrameRecord>,
        iterators: impl Iterator<Item = ActiveIterator>,
        with_objects: impl Iterator<Item = Value>,
    ) {
        frame.close_environment_window();
        frame.with_objects.extend(with_objects);
        let mut frame = frame.into_frame(self.with_stack.len());
        frame.active_iterators.extend(iterators);
        self.activate_frame(&mut frame);
        self.frames.push(frame);
    }

    /// The `value` and `done` of `result` where it is an iterator result this realm made
    /// (`iterator_result`) whose fields are still its own data slots, `done` a Boolean: a
    /// delegate step's result, which no code has seen. `None` for any other object, whose
    /// fields `[[Get]]` reads.
    pub(in crate::vm) fn own_iterator_result_parts(&self, result: Value) -> Option<(Value, bool)> {
        let (prototype, header) = self.iterator_result_header?;
        let object = self.heap.object_record(result)?;
        if object.shape() != header.shape() || object.proto() != prototype {
            return None;
        }
        let value = self.heap.property_get(object, crate::vm::iterators::ITERATOR_RESULT_VALUE_SLOT)?;
        Some((value, self.heap.property_get(object, crate::vm::iterators::ITERATOR_RESULT_DONE_SLOT)?.as_bool()?))
    }

    /// The iterator result of a generator step that stopped with `value`: the reused object a step
    /// whose `done` and `value` reads alone observe it shares (`reused_iterator_result`), or a
    /// fresh one.
    pub(in crate::vm) fn generator_step_result(&mut self, value: Value, done: bool, reuses_result: bool) -> Value {
        if reuses_result {
            self.reused_iterator_result(value, done)
        } else {
            self.iterator_result(value, done).expect("an iterator result allocates")
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use crate::test_host::TestHost;
    use crate::vm::{activation::frames_made, Vm};
    use crate::Engine;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What running `source` prints, and how many frames its resumes made.
    pub(in crate::vm) fn run(source: &str) -> (Vec<String>, usize) {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = Engine::specialize(source, "generator-windows.js").unwrap();
        let before = frames_made();
        vm.execute(&program).unwrap();
        let made = frames_made() - before;
        let lines = output.borrow().clone();
        (lines, made)
    }

    /// What running `source` as a module prints, and how many frames its resumes made.
    pub(in crate::vm) fn module_run(source: &str) -> (Vec<String>, usize) {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = Engine::specialize_module(source, "generator-windows.mjs").unwrap();
        let before = frames_made();
        vm.execute(&program).unwrap();
        let made = frames_made() - before;
        let lines = output.borrow().clone();
        (lines, made)
    }

    /// What running `source` prints in each execution mode, which must agree.
    pub(in crate::vm) fn agree(source: &str) -> Vec<String> {
        let outputs: Vec<Vec<String>> = [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled]
            .into_iter()
            .map(|mode| {
                let output = Rc::new(RefCell::new(Vec::new()));
                let mut vm = Vm::new(TestHost::writing_to(output.clone()));
                vm.set_execution_mode(mode);
                let program = Engine::specialize(source, "generator-windows.js").unwrap();
                vm.execute(&program).unwrap();
                let lines = output.borrow().clone();
                lines
            })
            .collect();
        assert_eq!(outputs[0], outputs[1], "compiled differs from the interpreter");
        outputs[0].clone()
    }

    fn stepping(steps: u32) -> String {
        format!(
            r#"
            function* range(n) {{ for (let i = 0; i < n; i++) yield i; }}
            function* twice(n) {{ yield* range(n); yield* range(n); }}
            let sum = 0;
            for (const x of range({steps})) sum += x;
            for (const x of twice({steps})) sum += x;
            print(String(sum));
            "#
        )
    }

    /// Per suspending function of `source`, whether its frame rides its register file, with an
    /// environment window after the registers, or stays a frame.
    pub(in crate::vm) fn layouts(source: &str) -> Vec<(bool, bool)> {
        let program = Engine::specialize(source, "suspended-plans.js").unwrap();
        program
            .compiled_functions()
            .filter(|function| function.is_generator || function.is_async)
            .map(|function| {
                let plan = function.call_plan(&program);
                (plan.suspended_registers, plan.suspended_environment)
            })
            .collect()
    }

    #[test]
    fn suspending_functions_run_in_windows_unless_they_read_what_the_lane_has_no_window_for() {
        let layouts = layouts(
            "function* flat(n) { for (let i = 0; i < n; i++) yield i; }
             function* closing(n) { for (let i = 0; i < n; i++) yield () => i; }
             function* reading() { yield arguments.length; }
             async function* asynchronous(n) { for (let i = 0; i < n; i++) { await i; yield i; } }
             async function awaiting(n) { let sum = 0; for (let i = 0; i < n; i++) sum += await i; return sum; }
             async function closing_over(n) { let sum = 0; const add = (k) => sum += k; await n; add(n); return sum; }
             async function reading_arguments() { await 0; return arguments.length; }",
        );
        let registers: Vec<bool> = layouts.iter().map(|(registers, _)| *registers).collect();
        assert_eq!(registers, [true, false, false, true, true, false, false]);
        // The `arguments` object each made on entry rides its window as a local.
        assert_eq!(layouts[2], (false, true));
        assert_eq!(layouts[6], (false, true));
        // Locals promoted to registers ride the window beside the closure it makes.
        assert_eq!(layouts[5], (false, true));
    }

    #[test]
    fn a_generator_with_a_closure_and_try_finally_resumes_without_frames() {
        // Its locals and the environment its closure captures ride a window past the record's
        // registers, and the handler is part of its bytecode, so no resume makes a `Box<Frame>`.
        let (lines, made) = run(
            r#"
            function* counting() {
                let total = 0;
                const add = (k) => total += k;
                try { for (let i = 0; i < 50; i++) { add(i); yield total; } } finally { print("closed"); }
            }
            let sum = 0;
            for (const x of counting()) sum += x;
            print(String(sum));
            "#,
        );
        assert_eq!(lines, ["closed", "20825"]);
        assert_eq!(made, 0, "a resume made a frame");
    }

    #[test]
    fn lane_resumes_of_a_generator_make_no_frame_per_step() {
        // The activation's window is the record's register file, so a step, a `yield` and a
        // `yield*` delegation allocate no `Box<Frame>`: the count does not grow with the steps.
        let (few_output, few) = run(&stepping(4));
        let (many_output, many) = run(&stepping(2000));
        assert_eq!(few_output, ["18"]);
        assert_eq!(many_output, ["5997000"]);
        assert_eq!(few, many, "frames made by resumes grew with the steps");
    }

    #[test]
    fn windowed_generators_meet_frame_resumes_and_materialization() {
        // A generator suspended by a windowed `yield` resumes as a frame (`return`, a loop's `break`),
        // and one whose activation leaves the lane (a native call, a throw) continues as a frame.
        let (lines, _) = run(
            r#"
            const out = [];
            function* g() { let i = 0; try { while (true) yield i++; } finally { out.push("closed"); } }
            for (const x of g()) { if (x === 3) break; out.push(x); }
            const it = g(); it.next(); it.next();
            out.push(JSON.stringify(it.return(7)), JSON.stringify(it.next()));
            function* h() { yield 1; Math.floor(1.5); null.x; }
            try { for (const x of h()) out.push(x); } catch (e) { out.push(e.constructor.name); }
            function* k(n) { for (let i = 0; i < n; i++) { const r = yield i; out.push(r); } }
            const sent = k(3); sent.next(); sent.next("a"); sent.next("b");
            print(out.join(" "));
            "#,
        );
        assert_eq!(lines, [r#"0 1 2 closed closed {"value":7,"done":true} {"done":true} 1 TypeError a b"#]);
    }
}
