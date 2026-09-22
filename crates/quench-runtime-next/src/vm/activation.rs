use crate::Value;

/// The only state that crosses an activation boundary. The value is kept as a
/// heap root while the continuation is suspended and is consumed by the
/// resumer according to its completion kind.
// The compiler does not emit suspension points yet; keep all four completion
// states explicit so future producers share this representation.
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
    pub function: u32,
    pub pc: usize,
    pub env: Value,
    pub this: Value,
    pub locals: Vec<Value>,
    pub registers: Vec<Value>,
    pub completion: Completion,
    pub captured: bool,
    pub resume_register: Option<u16>,
    pub promise: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct GeneratorRecord {
    pub(crate) continuation: Option<Continuation>,
    pub(crate) done: bool,
    pub(crate) running: bool,
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
    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        std::iter::once(self.env)
            .chain(std::iter::once(self.this))
            .chain(self.locals.iter().copied())
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
            function: 3,
            pc: 7,
            env: Value::heap(1),
            this: Value::heap(2),
            locals: vec![Value::heap(4)],
            registers: vec![Value::heap(5)],
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
