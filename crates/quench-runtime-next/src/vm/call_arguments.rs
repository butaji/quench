use crate::Value;

pub(super) struct CallArguments {
    inline: [Value; 8],
    overflow: Option<Vec<Value>>,
    len: usize,
}

impl CallArguments {
    pub(super) fn from_values<I>(values: I) -> Self
    where
        I: IntoIterator<Item = Value>,
    {
        let mut arguments = Self {
            inline: [Value::UNDEFINED; 8],
            overflow: None,
            len: 0,
        };
        for value in values {
            if arguments.len < arguments.inline.len() && arguments.overflow.is_none() {
                arguments.inline[arguments.len] = value;
            } else {
                let overflow = arguments
                    .overflow
                    .get_or_insert_with(|| arguments.inline[..arguments.len].to_vec());
                overflow.push(value);
            }
            arguments.len += 1;
        }
        arguments
    }

    pub(super) fn as_slice(&self) -> &[Value] {
        match &self.overflow {
            Some(values) => values,
            None => &self.inline[..self.len],
        }
    }
}
