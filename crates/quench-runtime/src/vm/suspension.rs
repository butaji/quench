//! The general step's suspensions (`Await`, `Yield`, `YieldStar`), run as their staged rows
//! (`staged::suspend`, `staged::delegation`) on the interpreter's frame: the frame answers each
//! call-out with the VM's own operation and turns the row's suspension into the step result the
//! general loop suspends the activation with.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use super::*;
use super::call_out::{CallSite, HostControl, HostFrame};
use crate::bytecode::WideInstruction;
use crate::staged::{CallFrame, Concrete, DelegationStep, SuspendEffect, Operation, Step, Suspending};

impl Vm {
    /// The suspension row `i` at `site`, by its staged row on the interpreter's stage: the step
    /// result that suspends the activation, or `StepResult::Continue` where a `yield*` delegate
    /// completed. A suspension that resumes at this instruction (a `yield*` step) moves `pc` back
    /// to it. An error the VM raised is thrown.
    pub(super) fn staged_suspend(&mut self, p: &ResidualProgram, site: CallSite, i: WideInstruction, pc: &mut usize)
        -> Result<StepResult, JsError> {
        let row = Operation::<u64, ()>::decode(i, crate::staged::DecodeContext::of(p)).expect("a suspension decodes");
        let mut stage = Concrete::over_vm(self);
        let mut frame = HostFrame { vm: self, program: p, site, raised: None };
        match crate::staged::step(&mut stage, &mut frame, row) {
            Step::Value(_) => Ok(StepResult::Continue),
            Step::Control(HostControl::Suspend(suspending)) => Ok(suspended_step(suspending, pc)),
            Step::Control(HostControl::Edge(_)) => unreachable!("a suspension row takes no edge"),
            Step::Unresolved => Err(frame.raised.expect("a suspension left to the VM raised")),
        }
    }

    /// The `next` method of the delegate `iterator` of a `yield*`: read from an async-from-sync
    /// iterator's source, else from the iterator, and refused where it is not callable.
    fn delegate_next_method(&mut self, p: &ResidualProgram, iterator: Value) -> Result<Value, JsError> {
        let receiver = match self.heap.iterator(iterator) {
            Some(IteratorRecord { source, kind: IteratorKind::AsyncFromSync, .. }) => *source,
            _ => iterator,
        };
        let atom = self.runtime_atoms.next;
        let method = self.get_property(p, receiver, atom)?;
        if !self.is_function(method) {
            return Err(self.type_error(p, "iterator next method is not callable".into()));
        }
        Ok(method)
    }

    /// The value a step of `yield*`'s delegation defines (`DelegationStep`), by the VM's own
    /// operation.
    fn delegation_step(&mut self, p: &ResidualProgram, step: DelegationStep<u64>) -> Result<Value, JsError> {
        let value = Value::from_bits;
        match step {
            DelegationStep::Iterator { source, asynchronous: true } => self.get_async_iterator(p, value(source)),
            DelegationStep::Iterator { source, asynchronous: false } => self.get_iterator(p, value(source)),
            DelegationStep::NextMethod { iterator } => self.delegate_next_method(p, value(iterator)),
            DelegationStep::Next { iterator, method, input } =>
                self.iterator_next_with_cached_method(p, value(iterator), value(method), &[value(input)]),
            DelegationStep::Done { result } => {
                let atom = self.runtime_atoms.done;
                self.get_property(p, value(result), atom)
            }
            DelegationStep::Value { result } => {
                let atom = self.runtime_atoms.value;
                self.get_property(p, value(result), atom)
            }
            DelegationStep::Truthy { value: word } => Ok(Value::boolean(self.truthy(value(word)))),
        }
    }
}

/// The step result of the row's suspension `suspending`, with `pc` moved back to this
/// instruction where the activation resumes at it.
fn suspended_step(suspending: Suspending<u64>, pc: &mut usize) -> StepResult {
    if suspending.again {
        *pc -= 1;
    }
    let (value, destination) = (Value::from_bits(suspending.value), suspending.destination);
    StepResult::Suspend(match suspending.effect {
        SuspendEffect::Await => Suspended::Await { value, destination },
        SuspendEffect::Yield => Suspended::Yield { value, destination, delegated_result: suspending.delegated.map(Value::from_bits) },
    })
}

impl HostFrame<'_> {
    /// The value of `answer`, or `None` with its error kept for the step's caller to throw.
    fn answered(&mut self, answer: Result<Value, JsError>) -> Option<u64> {
        match answer {
            Ok(value) => Some(value.to_bits()),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }
}

impl CallFrame<Concrete> for HostFrame<'_> {
    /// The suspension as the frame's control, which `Vm::staged_suspend` makes the step result.
    fn suspend(&mut self, _stage: &mut Concrete, suspending: Suspending<u64>) -> Option<HostControl> {
        Some(HostControl::Suspend(suspending))
    }

    fn asynchronous(&self) -> Option<bool> {
        Some(self.program.function(self.site.function as usize).is_async)
    }

    fn object_test(&mut self, _stage: &mut Concrete, value: u64) -> Option<bool> {
        Some(self.vm.is_object_like(Value::from_bits(value)))
    }

    fn promise_resolve(&mut self, _stage: &mut Concrete, value: u64) -> Option<u64> {
        let resolved = self.vm.promise_for_value(self.program, Value::from_bits(value));
        self.answered(resolved)
    }

    fn delegation(&mut self, _stage: &mut Concrete, step: DelegationStep<u64>) -> Option<(u64, ())> {
        let answer = self.vm.delegation_step(self.program, step);
        self.answered(answer).map(|word| (word, ()))
    }
}
