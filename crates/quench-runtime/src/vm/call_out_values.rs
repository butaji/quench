//! The host's side of the by-name, environment, string and iteration-cleanup call-out rows (`staged::protocol_values`): each step runs
//! the interpreter's own helper for its opcode.
use super::*;
use crate::staged::protocol_values::ValueStep;

impl Vm {
    /// The by-name, environment, string and iteration-cleanup `step` of its operand values (`first` is the first operand register's
    /// value, `second` the second's): the value it defines, if any.
    pub(super) fn value_step(&mut self, p: &ResidualProgram, site: super::call_out::CallSite, step: ValueStep, first: Option<Value>, second: Option<Value>)
        -> Result<Option<Value>, JsError> {
        let operand = second.unwrap_or(Value::UNDEFINED);
        match step {
            ValueStep::ResolveName { atom, strict } => self.at_binding_site(site.owner, site.resume, |vm| vm.resolve_name(p, atom, strict)).map(Some),
            ValueStep::DeleteName { atom } => self.at_binding_site(site.owner, site.resume, |vm| vm.delete_name(p, atom)).map(Some),
            ValueStep::IteratorCleanupPush { .. } | ValueStep::IteratorCleanupPop => self.iterator_cleanup_step(step).map(|()| None),
            ValueStep::LoadResolvedName { atom, strict } => self.load_resolved_name(p, operand, atom, strict).map(Some),
            ValueStep::StoreResolvedName { atom, strict } => {
                let value = first.unwrap_or(Value::UNDEFINED);
                self.store_resolved_name(p, operand, atom, value, strict)?;
                Ok(None)
            }
            ValueStep::CloneEnv { plan } => {
                let f = self.own_frame();
                let clones = &p.function(self.frames[f].function as usize).environment_clones;
                let plan = &clones[plan];
                self.clone_frame_environment(f, &plan.fresh, clones.resets(plan))?;
                Ok(None)
            }
            ValueStep::GetAsyncIterator => self.get_async_iterator(p, operand).map(Some),
            ValueStep::RequireIteratorResult => {
                self.require_iterator_result(p, operand)?;
                Ok(None)
            }
            ValueStep::StringConcat { window } => {
                let f = self.own_frame();
                self.frame_string_concat(p, f, window).map(Some)
            }
            ValueStep::StringCaseIndex { start, count } => {
                let f = self.own_frame();
                let cases = self.programs.constants(self.frames[f].program)
                    .get(start..start + usize::from(count))
                    .ok_or_else(|| JsError::validation("switch cases are outside program".into()))?;
                let index = self.string_case_index(operand, cases);
                Ok(Some(Value::integer(index as i32)))
            }
        }
    }

    /// The running frame's iterator cleanup stack after an `IteratorCleanupPush` or `IteratorCleanupPop` `step`, as the
    /// interpreter's arms edit it; a pop of an empty stack is the interpreter's underflow error.
    fn iterator_cleanup_step(&mut self, step: ValueStep) -> Result<(), JsError> {
        let f = self.own_frame();
        match step {
            ValueStep::IteratorCleanupPush { iterator, done } => self.frames[f].active_iterators.push(ActiveIterator { iterator, done }),
            ValueStep::IteratorCleanupPop => {
                self.frames[f]
                    .active_iterators
                    .pop()
                    .ok_or_else(|| JsError("iterator cleanup stack underflow".into()))?;
            }
            _ => unreachable!("only the iterator cleanup steps edit the cleanup stack"),
        }
        Ok(())
    }
}
