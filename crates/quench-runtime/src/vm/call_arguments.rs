use crate::Value;
use smallvec::SmallVec;

const INLINE_ARGUMENT_CAPACITY: usize = 8;

pub struct CallArguments {
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

    /// A copy of `values`: one in place, by a loop of word copies, where they fit the inline
    /// capacity (a call's usual count), else the vector's.
    #[inline(always)]
    pub(super) fn from_slice(values: &[Value]) -> Self {
        if values.len() > INLINE_ARGUMENT_CAPACITY {
            return Self::spilled(values);
        }
        let mut buffer = [Value::UNDEFINED; INLINE_ARGUMENT_CAPACITY];
        for (slot, value) in buffer.iter_mut().zip(values) {
            *slot = *value;
        }
        Self { values: SmallVec::from_buf_and_len(buffer, values.len()) }
    }

    #[cold]
    #[inline(never)]
    fn spilled(values: &[Value]) -> Self {
        Self { values: SmallVec::from_slice(values) }
    }

    #[inline]
    pub(super) fn as_slice(&self) -> &[Value] {
        &self.values
    }
}
