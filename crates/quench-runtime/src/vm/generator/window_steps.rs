//! A generator the lane runs as an activation over its suspended frame's register file
//! (`GeneratorFrame`): the record keeps the frame while the activation runs, so a resume
//! makes no `Box<Frame>` and copies no register, and a yield suspends the frame in place.
use super::*;
use super::super::activation::GeneratorFrame;

impl<H: Host> Vm<H> {
    /// The frame of `generator` the lane resumes as an activation over its register file, which
    /// `code`, the running program's, lays out as its registers (`CallPlan::generator_registers_only`):
    /// a suspended generator of this realm and program that is not running or done.
    pub(in crate::vm) fn generator_window_frame(&self, generator: Value, code: &ResidualProgram) -> Option<&GeneratorFrame> {
        let IteratorRecord { ext, .. } = self.heap.iterator(generator)?;
        let record = ext.generator.as_deref()?;
        let frame = &*record.suspended_continuation()?.frame;
        (record.realm == self.realm.globals
            && frame.program == self.active_program
            && code.function_get(frame.function as usize)?.call_plan(code).generator_registers_only)
            .then_some(frame)
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
    ) -> Option<*mut GeneratorFrame> {
        let (frame, resume_register) = self.generator_record_mut(generator)?.resume_windowed()?;
        // SAFETY: the record owns the box, which nothing moves or drops until the run ends.
        if let Some(register) = resume_register
            && let Some(slot) = unsafe { &mut *frame }.registers.get_mut(usize::from(register))
        {
            *slot = sent;
        }
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
        let (program, function) = (frame.program, frame.function);
        let live_registers = self.release_dead_registers(program, function, pc, &mut frame.registers);
        frame.suspend_at(pc, live_registers, self.lane_stack.drain_iterators(depth));
        let continuation = Continuation { frame, completion: Completion::Yield(value), resume_register: Some(destination), promise: Value::UNDEFINED };
        if let Some(record) = self.generator_record_mut(generator) {
            record.suspend(continuation);
        }
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
    ) {
        let frame = self
            .generator_record_mut(generator)
            .and_then(GeneratorRecord::take_windowed)
            .expect("a windowed generator's activation materializes");
        let mut frame = frame.into_frame(self.with_stack.len());
        frame.active_iterators.extend(iterators);
        self.activate_frame(&mut frame);
        self.frames.push(frame);
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
mod tests {
    use crate::test_host::TestHost;
    use crate::vm::{activation::frames_made, Vm};
    use crate::Engine;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What running `source` prints, and how many frames its resumes made.
    fn run(source: &str) -> (Vec<String>, usize) {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = Engine::specialize(source, "generator-windows.js").unwrap();
        let before = frames_made();
        vm.execute(&program).unwrap();
        let made = frames_made() - before;
        let lines = output.borrow().clone();
        (lines, made)
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

    #[test]
    fn only_generators_laid_out_as_their_registers_run_in_windows() {
        let program = Engine::specialize(
            "function* flat(n) { for (let i = 0; i < n; i++) yield i; }
             function* closing(n) { for (let i = 0; i < n; i++) yield () => i; }
             function* reading() { yield arguments.length; }
             async function* asynchronous(n) { for (let i = 0; i < n; i++) yield i; }",
            "generator-plans.js",
        )
        .unwrap();
        let windowed: Vec<bool> = program
            .functions
            .iter()
            .filter(|function| function.is_generator)
            .map(|function| function.call_plan(&program).generator_registers_only)
            .collect();
        assert_eq!(windowed, [true, false, false, false]);
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
