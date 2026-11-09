//! Generator steps made in place: a frame's `next()` call of a generator and its `yield*`
//! of a delegate generator push the generator's frame into the running loop.
use super::*;

impl<H: Host> Vm<H> {
    /// `next()` of `generator`, a suspended synchronous generator of this realm and program
    /// that `method` (`steps_as_generator`) steps, called with no arguments from frame `f`'s
    /// call record that resumes at `pc`: its frame pushed in place (`resume_generator_in_place`),
    /// the pending call completing with the step's result (`finish_in_loop_generator_step`).
    /// `None` leaves any other step, and a call returning its result from the frame (the A
    /// field `destination` says so), to the native call.
    pub(in crate::vm) fn try_in_loop_generator_step(
        &mut self,
        f: usize,
        pc: usize,
        method: Value,
        generator: Value,
        destination: crate::bytecode::Register,
        reuses_result: bool,
    ) -> Option<Result<StepResult, JsError>> {
        if destination & crate::bytecode::RETURN_REGISTER != 0 || !self.generator_resumes_in_place(generator) {
            return None;
        }
        let resumed = self.resume_generator_in_place(f, pc, method, generator, Value::UNDEFINED, reuses_result)?;
        Some(resumed.map(|(call_guard, resume)| StepResult::PushFrame {
            destination: destination & crate::bytecode::REGISTER_MASK,
            call_guard,
            completion: CallCompletion::Generator(resume),
        }))
    }

    /// A synchronous `yield*` step of `delegate`, a generator `method` steps
    /// (`steps_as_generator`) and `generator_resumes_in_place` admits, made in place from
    /// frame `f`, the innermost: `delegate`'s frame resumes with `input` and is pushed, and the
    /// step's fresh result goes to register `state`, from which the `yield*` at `pc`, which
    /// frame `f` resumes at, takes it (a value to pass on, or the delegate's completion).
    /// `None` leaves the step to the native call.
    pub(in crate::vm) fn yield_star_in_place(
        &mut self,
        f: usize,
        pc: usize,
        method: Value,
        delegate: Value,
        input: Value,
        state: crate::bytecode::Register,
    ) -> Option<Result<StepResult, JsError>> {
        if f + 1 != self.frames.len() || !self.generator_resumes_in_place(delegate) {
            return None;
        }
        let resumed = self.resume_generator_in_place(f, pc, method, delegate, input, false)?;
        Some(resumed.map(|(call_guard, resume)| StepResult::PushFrame {
            destination: state,
            call_guard,
            completion: CallCompletion::Generator(resume),
        }))
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
}
