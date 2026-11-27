//! A generator the lane runs as an activation over its suspended frame's register file
//! (`FrameRecord`): the record keeps the frame while the activation runs, so a resume
//! makes no `Box<Frame>` and copies no register, and a yield suspends the frame in place.
use super::*;
use super::super::activation::FrameRecord;
use super::super::activation::frame_record::ParkedNative;

impl Vm {
    /// Whether the lane resumes `frame`, suspended by a function of `code`, over its register
    /// file, and whether its window carries an environment after the registers: a frame of the
    /// active program whose function lays out as its registers or with an environment window
    /// (`CallPlan::suspended_registers`, `CallPlan::suspended_environment`).
    pub fn window_layout(&self, frame: &FrameRecord, code: &ResidualProgram) -> Option<bool> {
        let plan = code.function_get(frame.function as usize)?.call_plan(code);
        (frame.program == self.active_program && (plan.suspended_registers || plan.suspended_environment))
            .then_some(plan.suspended_environment)
    }

    /// The receiver a window over `frame`'s record runs with, resumed under the environment
    /// `parent`: the frame's own, or an arrow's lexical `this`, which a `super()` since the frame
    /// suspended may have initialized. `None` where it is still uninitialized, which only the
    /// general path's dead-zone check raises from.
    pub fn window_this(&self, body: &crate::bytecode::Function, this: Value, parent: Value) -> Option<Value> {
        if !body.is_arrow {
            return Some(this);
        }
        match self.captured_lexical_this(parent) {
            Some(lexical) => (!lexical.is_deleted()).then_some(lexical),
            None => Some(this),
        }
    }

    /// Starts a windowed run of `generator`, which `lane_resume_generator` admitted with its
    /// suspended frame and the register that takes `sent` (`GeneratorRecord::suspended_frame`):
    /// the frame stays in its record, `sent` is the value the suspending `yield` evaluates to,
    /// and the native activation roots the generator while the run lasts.
    /// `record` is `generator`'s, which its cell keeps boxed while the generator lives.
    #[inline(always)]
    pub fn resume_generator_window(
        &mut self,
        (generator, record): (Value, *mut GeneratorRecord),
        suspended: (*mut FrameRecord, Option<u16>),
        method: Value,
        sent: Value,
        depth: usize,
    ) {
        // SAFETY: the caller's; nothing since its lookup freed the generator or ran its frame.
        unsafe {
            (*record).resume_suspended_window();
            (*suspended.0).resuming_generator = record;
        }
        self.enter_windowed(suspended, sent);
        // The frames the lane's activations become sit below the generator's.
        self.push_host_frame(method, generator, self.frames.len() + depth, super::super::activation::NativeCallBoundary::Opaque);
    }

    /// Enters the suspended frame a windowed run resumes, which its owner handed over running
    /// (`resume_windowed`) with the register that takes `sent`: its dead registers settle and the
    /// register receives the value. Every windowed resumption, a generator step's and a job's
    /// (`resume_job_window`), enters its frame here. The frame's address.
    #[inline(always)]
    pub(in crate::vm) fn enter_windowed(&mut self, (frame, resume_register): (*mut FrameRecord, Option<u16>), sent: Value) -> *mut FrameRecord {
        let epoch = self.heap.collection_epoch();
        // SAFETY: the owner keeps the box, which nothing moves or drops until the run ends.
        let record = unsafe { &mut *frame };
        record.settle_registers(epoch);
        record.receive(resume_register, sent);
        frame
    }

    /// Ends the windowed run of `generator` over `frame`, its suspended frame's record (the
    /// activation's, `ScriptActivation::record`), at a `yield` that resumes at `pc`, whose
    /// value goes to `destination` when it resumes: the frame is suspended where it stands, its
    /// live registers noted, and takes back the iterators its loops pushed on the lane stack for
    /// the activation at `depth`.
    #[inline(always)]
    pub fn suspend_generator_window(
        &mut self,
        (generator, frame): (Value, *mut FrameRecord),
        pc: usize,
        destination: crate::bytecode::Register,
        value: Value,
        depth: usize,
    ) {
        self.pop_host_frame();
        // SAFETY: the generator's record keeps the frame its activation runs over boxed, and
        // parking reads the lane stack, not the record.
        let frame = unsafe { &mut *frame };
        self.park_frame(frame, pc, depth);
        let record = frame.resuming_generator;
        debug_assert!(self.generator_record_mut(generator).is_some_and(|found| std::ptr::eq(found, record)));
        // SAFETY: the record that resumed the window (`resume_generator_window`), boxed in the
        // generator's cell, which the native activation popped above rooted for the run.
        unsafe { (*record).park_windowed(Completion::Yield(value), Some(destination), false) };
    }

    /// Parks the frame of the windowed run `record` holds, in place, at `pc`, with what it
    /// receives next going to `destination` and the iterators the activation at `depth`
    /// pushed taken back: suspended at `completion`, or busy (`busy`) where an async
    /// generator awaits.
    pub(in crate::vm) fn park_windowed_record(
        &mut self,
        record: *mut GeneratorRecord,
        (pc, destination, depth): (usize, crate::bytecode::Register, usize),
        completion: Completion,
        busy: bool,
    ) {
        // SAFETY: the generator's cell keeps its record boxed, and parking the frame frees no cell.
        let record = unsafe { &mut *record };
        let frame = record.windowed_frame_mut().expect("a windowed stop belongs to a windowed generator");
        // SAFETY: the record keeps the frame boxed; parking reads the lane stack, not the record.
        self.park_frame(unsafe { &mut *std::ptr::from_mut(frame) }, pc, depth);
        record.park_windowed(completion, Some(destination), busy);
    }

    /// The generator record of `generator`. A record is boxed once at creation and freed only by
    /// a collection, so the last one found is reused while no collection ran since
    /// (`LaneStack::last_generator`): a step looks its generator up several times, and a
    /// delegation alternates between two.
    #[inline(always)]
    pub fn generator_record_mut(
        &mut self,
        generator: Value,
    ) -> Option<&mut GeneratorRecord> {
        let (last, epoch, record) = self.lane_stack.last_generator[0];
        if last == generator && epoch == self.heap.collection_epoch() {
            // SAFETY: the record is boxed, never replaced, and freed only by a collection.
            return Some(unsafe { &mut *record });
        }
        self.looked_up_generator_record(generator)
    }

    /// `generator_record_mut` of a generator other than the last one found: the one before it,
    /// or looked up in the heap, made the last.
    #[inline(never)]
    fn looked_up_generator_record(&mut self, generator: Value) -> Option<&mut GeneratorRecord> {
        let epoch = self.heap.collection_epoch();
        let [last, previous] = self.lane_stack.last_generator;
        let record: *mut GeneratorRecord = if (previous.0, previous.1) == (generator, epoch) {
            previous.2
        } else {
            match self.heap.iterator_mut(generator) {
                Some(IteratorRecord { ext, .. }) => std::ptr::from_mut(ext.generator.as_deref_mut()?),
                _ => return None,
            }
        };
        self.lane_stack.last_generator = [(generator, epoch, record), last];
        // SAFETY: just found in the live heap.
        Some(unsafe { &mut *record })
    }

    /// Suspends the frame of a windowed run at `pc`: the environment window closed, its dead
    /// registers noted, and the iterators and `with` objects the activation at `depth` holds
    /// taken back.
    #[inline(always)]
    pub(in crate::vm) fn park_frame(&mut self, frame: &mut FrameRecord, pc: usize, depth: usize) {
        frame.close_environment_window();
        if !frame.suspended_live_at(pc) {
            self.note_suspension_registers(frame, pc);
        }
        self.lane_stack.take_with_objects(depth, frame);
        self.lane_stack.take_iterators(depth, &mut frame.active_iterators);
        frame.suspend_at(pc, self.heap.collection_epoch());
    }

    /// `suspension_registers` of `frame` at `pc`, kept on the record (`suspension_live`) for
    /// each next suspension there: a generator or async body suspends at the same few points
    /// over and over, and its function's code stays while the frame lives.
    #[inline(never)]
    fn note_suspension_registers(&self, frame: &mut FrameRecord, pc: usize) {
        let (live, _) = self.suspension_registers(frame.program, frame.function, pc);
        frame.note_suspension_live(pc, live);
    }

    /// Ends the windowed run of `generator` at its return: it completes, dropping its frame.
    pub fn finish_generator_window(&mut self, generator: Value) {
        self.pop_host_frame();
        if let Some(record) = self.generator_record_mut(generator) {
            record.complete();
        }
    }

    /// The frame of `generator`'s windowed run, for the general loop to continue in where the
    /// lane's activations become frames: pushed with the iterators its loops pushed since the
    /// lane resumed it (`iterators`).
    pub fn push_generator_window_frame(
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
        if let Some(caller) = frame.split_caller() {
            self.push_resumed_frame(caller, std::iter::empty(), std::iter::empty());
        }
        frame.close_environment_window();
        let mut frame = frame.into_frame(self.with_stack.len());
        frame.with_objects.extend(with_objects);
        frame.active_iterators.extend(iterators);
        self.activate_frame(&mut frame);
        self.frames.push(frame);
    }

    /// Resumes a chain `capture_chain` captured: its frames go back on the frame stack bottom
    /// first, each with the state its record kept. A top frame parked in a native call makes that
    /// call again through the ordinary path (`call_value`, or `construct_value_with_new_target`
    /// when the parked call is a `new`), delivers its result to the call instruction's destination
    /// and continues after it; the frames below wait for the run.
    /// No production caller yet: the build-time input capture (MEM, c5-capture-at-input.md) resumes
    /// a parked chain here.
    #[allow(dead_code)]
    pub fn resume_chain(&mut self, mut chain: Box<FrameRecord>) -> Result<(), JsError> {
        let parked = chain.parked.take();
        let (program, function) = (chain.program, chain.function);
        self.push_resumed_frame(chain, std::iter::empty(), std::iter::empty());
        let Some(parked) = parked else { return Ok(()) };
        let top = self.frames.len() - 1;
        // SAFETY: the chain names a program this VM has loaded, and a loaded program stays loaded
        // for as long as the VM runs its frames, so the reference holds across the call.
        let p: *const ResidualProgram = self.programs.residual(program).expect("a chain's program is loaded");
        let p = unsafe { &*p };
        let call_pc = parked.call_pc as usize;
        let destination = p.function(function as usize).instruction_at(call_pc).result_register();
        let value = match parked.new_target {
            Some(new_target) => self.construct_value_with_new_target(p, parked.callee, new_target, &parked.args)?,
            None => self.call_value(p, parked.callee, parked.this, &parked.args)?,
        };
        self.frames[top].pc = call_pc + 1;
        self.frames[top].pc_is_resume = true;
        if let Some(slot) = self.frames[top].registers.get_mut(usize::from(destination)) {
            *slot = value;
        }
        Ok(())
    }

    /// Captures the chain down to `base_depth` (`capture_chain`) at a native's edge, with the native
    /// call its top frame is parked in: the top frame's pc is the call instruction. `new_target` is
    /// `None` for a call and `Some` for a construction. One call, so the parked values go straight
    /// from the native's arguments into the traced record.
    #[allow(dead_code)]
    pub fn capture_chain_parked(
        &mut self,
        base_depth: usize,
        callee: Value,
        this: Value,
        args: Box<[Value]>,
        new_target: Option<Value>,
    ) -> Box<FrameRecord> {
        let call_pc = self.frames.last().expect("a native is parked in a frame").pc as u32;
        let mut top = self.capture_chain(base_depth);
        top.parked = Some(ParkedNative { callee, this, args, call_pc, new_target });
        top
    }

    /// Takes the frames from the top of the frame stack down to `base_depth` (inclusive) off it as
    /// one chain of records: the record returned is the top frame, whose `caller` is the frame it
    /// returns into, down to the base, whose `caller` is `None`. Each frame's `with` objects go
    /// with its record, as a suspension's do. `resume_chain` pushes the chain back. No production
    /// caller yet: the build-time input capture (C4b) is the first exit that needs a chain.
    #[allow(dead_code)]
    pub fn capture_chain(&mut self, base_depth: usize) -> Box<FrameRecord> {
        assert!(base_depth < self.frames.len(), "a chain captures at least its base frame");
        let mut popped = Vec::with_capacity(self.frames.len() - base_depth);
        while self.frames.len() > base_depth {
            let mut frame = self.frames.pop().expect("a frame above the base");
            frame.with_objects = self.with_stack.split_off(frame.with_base);
            popped.push(frame);
        }
        // `popped` is top first: linking from the base up gives each record its caller already built.
        let mut chain: Option<Box<FrameRecord>> = None;
        for frame in popped.into_iter().rev() {
            let mut record = FrameRecord::capture(frame);
            record.caller = chain;
            chain = Some(record);
        }
        chain.expect("the base frame is captured")
    }

    /// The `value` and `done` of `result` where it is an iterator result this realm made
    /// (`iterator_result`) whose fields are still its own data slots, `done` a Boolean: a
    /// delegate step's result, which no code has seen. `None` for any other object, whose
    /// fields `[[Get]]` reads.
    pub fn own_iterator_result_parts(&self, result: Value) -> Option<(Value, bool)> {
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
    pub fn generator_step_result(&mut self, value: Value, done: bool, reuses_result: bool) -> Value {
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

    /// A chain of three frames captured off the frame stack resumes bottom first, each frame
    /// with the function and pc it had, and the top one runs next.
    #[test]
    fn captured_chain_resumes_three_levels_bottom_first() {
        let mut vm = Vm::new(TestHost::default());
        for (function, pc) in [(1u32, 3usize), (2, 5), (3, 7)] {
            let mut frame = Box::new(crate::vm::Frame::empty(crate::vm::program_store::ProgramId::MAIN, 0));
            frame.function = function;
            frame.pc = pc;
            vm.frames.push(frame);
        }
        let chain = vm.capture_chain(0);
        assert!(vm.frames.is_empty());
        assert_eq!((chain.function, chain.pc), (3, 7));
        assert!(chain.caller.is_some());
        vm.resume_chain(chain).unwrap();
        let resumed: Vec<(u32, usize)> = vm.frames.iter().map(|frame| (frame.function, frame.pc)).collect();
        assert_eq!(resumed, [(1, 3), (2, 5), (3, 7)]);
    }

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
