use super::program_store::ProgramId;
use super::{ActiveIterator, Atom};
use crate::Value;
use std::collections::VecDeque;

/// The only state that crosses an activation boundary. The value is kept as a
/// child of the suspension's owner and is consumed by the resumer according
/// to its completion kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Completion {
    GeneratorStart,
    Return(Value),
    // Current thrown resumes enter the shared frame error boundary directly.
    #[allow(dead_code)]
    Throw(Value),
    Yield(Value),
    Await(Value),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NativeActivation {
    pub callable: Value,
    pub frame_depth: usize,
    pub boundary: NativeCallBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeCallBoundary {
    Forward,
    Opaque,
}

/// The origin of an activation, including the identity exposed to JavaScript.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CallContext {
    Function(Value),
    DirectEval(Value),
    IndirectEval(Value),
    Internal,
}

impl CallContext {
    pub(super) fn user_function(id: u32, callable: Value) -> Self {
        if id == super::ROOT_FUNCTION_ID {
            Self::Internal
        } else {
            Self::Function(callable)
        }
    }

    pub(super) fn callee(self) -> Option<Value> {
        match self {
            Self::Function(value) | Self::DirectEval(value) | Self::IndirectEval(value) => {
                Some(value)
            }
            Self::Internal => None,
        }
    }

    pub(super) fn callable(self) -> Option<Value> {
        match self {
            Self::Function(value) => Some(value),
            Self::DirectEval(_) | Self::IndirectEval(_) | Self::Internal => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Continuation {
    pub context: CallContext,
    pub original_arguments: Vec<Value>,
    pub program: ProgramId,
    pub function: u32,
    pub pc: usize,
    pub env: Value,
    pub this: Value,
    pub locals: Vec<Value>,
    pub dynamic_bindings: Vec<(Atom, Value)>,
    pub registers: Vec<Value>,
    pub active_iterators: Vec<ActiveIterator>,
    pub with_objects: Vec<Value>,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
            context: std::mem::replace(&mut frame.context, CallContext::Internal),
            original_arguments: std::mem::take(&mut frame.original_arguments),
            program: frame.program,
            function: frame.function,
            pc: frame.pc,
            env: frame.env,
            this: frame.this,
            locals: std::mem::take(&mut frame.locals),
            dynamic_bindings: std::mem::take(&mut frame.dynamic_bindings),
            registers: std::mem::take(&mut frame.registers),
            active_iterators: std::mem::take(&mut frame.active_iterators),
            with_objects: std::mem::take(&mut frame.with_objects),
            completion,
            captured: frame.captured,
            resume_register,
            promise,
        }
    }

    pub(super) fn into_frame(self, with_base: usize) -> super::Frame {
        super::Frame {
            context: self.context,
            original_arguments: self.original_arguments,
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
            with_objects: self.with_objects,
            with_base,
        }
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.context
            .callee()
            .into_iter()
            .chain(self.original_arguments.iter().copied())
            .chain(self.with_objects.iter().copied())
            .chain(std::iter::once(self.env))
            .chain(std::iter::once(self.this))
            .chain(self.locals.iter().copied())
            .chain(self.dynamic_bindings.iter().map(|(_, value)| *value))
            .chain(self.registers.iter().copied())
            .chain(match self.completion {
                Completion::GeneratorStart => None,
                Completion::Return(value)
                | Completion::Throw(value)
                | Completion::Yield(value)
                | Completion::Await(value) => Some(value),
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
            context: CallContext::Function(Value::heap(9)),
            original_arguments: vec![Value::heap(10)],
            program: ProgramId::MAIN,
            function: 3,
            pc: 7,
            env: Value::heap(1),
            this: Value::heap(2),
            locals: vec![Value::heap(4)],
            dynamic_bindings: vec![(0, Value::heap(8))],
            registers: vec![Value::heap(5)],
            with_objects: vec![Value::heap(11)],
            active_iterators: vec![],
            completion: Completion::Await(Value::heap(6)),
            captured: false,
            resume_register: Some(1),
            promise: Value::heap(7),
        };
        assert_eq!(
            continuation.roots().collect::<Vec<_>>(),
            [
                Value::heap(9),
                Value::heap(10),
                Value::heap(11),
                Value::heap(1),
                Value::heap(2),
                Value::heap(4),
                Value::heap(8),
                Value::heap(5),
                Value::heap(6),
                Value::heap(7)
            ]
        );
        let mut frame = continuation.into_frame(0);
        assert_eq!(frame.original_arguments, [Value::heap(10)]);
        assert_eq!(frame.with_objects, [Value::heap(11)]);
        let restored = Continuation::from_frame(
            &mut frame,
            Completion::Await(Value::heap(6)),
            Some(1),
            Value::heap(7),
        );
        assert!(frame.original_arguments.is_empty());
        assert_eq!(restored.original_arguments, [Value::heap(10)]);
        assert!(frame.with_objects.is_empty());
        assert_eq!(restored.with_objects, [Value::heap(11)]);
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
