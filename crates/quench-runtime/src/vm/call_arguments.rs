use crate::Value;
use smallvec::SmallVec;

const INLINE_ARGUMENT_CAPACITY: usize = 8;

pub(super) struct CallArguments {
    values: SmallVec<[Value; INLINE_ARGUMENT_CAPACITY]>,
}

impl CallArguments {
    pub(super) fn from_values<I>(values: I) -> Self
    where
        I: IntoIterator<Item = Value>,
    {
        let mut arguments = SmallVec::new();
        arguments.extend(values);
        Self { values: arguments }
    }

    pub(super) fn from_slice(values: &[Value]) -> Self {
        Self {
            values: SmallVec::from_slice(values),
        }
    }

    pub(super) fn as_slice(&self) -> &[Value] {
        &self.values
    }
}
