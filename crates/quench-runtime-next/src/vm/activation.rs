use super::program_store::ProgramId;
use super::{ActiveIterator, Atom};
use crate::Value;
use std::collections::VecDeque;

/// The only state that crosses an activation boundary. The value is kept as a
/// heap root while the continuation is suspended and is consumed by the
/// resumer according to its completion kind.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Completion {
    Return(Value),
    Throw(Value),
    Yield(Value),
    Await(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Continuation {
    pub program: ProgramId,
    pub function: u32,
    pub pc: usize,
    pub env: Value,
    pub this: Value,
    pub locals: Vec<Value>,
    pub dynamic_bindings: Vec<(Atom, Value)>,
    pub registers: Vec<Value>,
    pub active_iterators: Vec<ActiveIterator>,
    pub completion: Completion,
    pub captured: bool,
    pub resume_register: Option<u16>,
    pub promise: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct GeneratorRecord {
    pub(crate) continuation: Option<Continuation>,
    pub(crate) realm: Value,
    pub(crate) done: bool,
    pub(crate) running: bool,
    pub(crate) requests: VecDeque<AsyncGeneratorRequest>,
}

impl GeneratorRecord {
    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        std::iter::once(self.realm)
            .chain(self.continuation.iter().flat_map(Continuation::roots))
            .chain(
                self.requests
                    .iter()
                    .flat_map(|request| [request.promise, request.value]),
            )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncGeneratorOperation {
    Next,
    Return,
    Throw,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AsyncGeneratorRequest {
    pub(crate) operation: AsyncGeneratorOperation,
    pub(crate) promise: Value,
    pub(crate) value: Value,
}

/// A generation-checked slot for a suspended activation. Resumption consumes
/// the slot, so a stale host/compiler token cannot resume a replacement frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContinuationId {
    pub(crate) slot: u32,
    pub(crate) generation: u32,
}

pub(crate) struct SuspendedEntry {
    pub(crate) generation: u32,
    pub(crate) continuation: Option<Continuation>,
}

impl Continuation {
    pub(super) fn from_frame(
        frame: &mut super::Frame,
        completion: Completion,
        resume_register: Option<u16>,
        promise: Value,
    ) -> Self {
        Self {
            program: frame.program,
            function: frame.function,
            pc: frame.pc,
            env: frame.env,
            this: frame.this,
            locals: std::mem::take(&mut frame.locals),
            dynamic_bindings: std::mem::take(&mut frame.dynamic_bindings),
            registers: std::mem::take(&mut frame.registers),
            active_iterators: std::mem::take(&mut frame.active_iterators),
            completion,
            captured: frame.captured,
            resume_register,
            promise,
        }
    }

    pub(super) fn into_frame(self, with_base: usize) -> super::Frame {
        super::Frame {
            program: self.program,
            function: self.function,
            pc: self.pc,
            binding_site_pc: None,
            env: self.env,
            this: self.this,
            locals: self.locals,
            dynamic_bindings: self.dynamic_bindings,
            captured: self.captured,
            registers: self.registers,
            active_iterators: self.active_iterators,
            with_base,
        }
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        std::iter::once(self.env)
            .chain(std::iter::once(self.this))
            .chain(self.locals.iter().copied())
            .chain(self.dynamic_bindings.iter().map(|(_, value)| *value))
            .chain(self.registers.iter().copied())
            .chain(match self.completion {
                Completion::Return(value)
                | Completion::Throw(value)
                | Completion::Yield(value)
                | Completion::Await(value) => std::iter::once(value),
            })
            .chain(std::iter::once(self.promise))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_roots_include_frame_and_completion_values() {
        let continuation = Continuation {
            program: ProgramId::MAIN,
            function: 3,
            pc: 7,
            env: Value::heap(1),
            this: Value::heap(2),
            locals: vec![Value::heap(4)],
            dynamic_bindings: vec![(0, Value::heap(8))],
            registers: vec![Value::heap(5)],
            active_iterators: vec![],
            completion: Completion::Await(Value::heap(6)),
            captured: false,
            resume_register: Some(1),
            promise: Value::heap(7),
        };
        assert_eq!(
            continuation.roots().collect::<Vec<_>>(),
            [
                Value::heap(1),
                Value::heap(2),
                Value::heap(4),
                Value::heap(8),
                Value::heap(5),
                Value::heap(6),
                Value::heap(7)
            ]
        );
    }

    #[test]
    fn continuation_slots_use_distinct_generation_tokens() {
        let first = ContinuationId {
            slot: 2,
            generation: 1,
        };
        let second = ContinuationId {
            slot: 2,
            generation: 2,
        };
        assert_ne!(first, second);
    }
}
