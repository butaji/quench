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
        };
        assert_eq!(
            continuation.roots().collect::<Vec<_>>(),
            [
                Value::heap(1),
                Value::heap(2),
                Value::heap(4),
                Value::heap(5),
                Value::heap(6)
            ]
        );
    }
}
