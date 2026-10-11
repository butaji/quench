use super::*;

const WITH_ARGUMENT_COUNT: usize = 2;
const SPLICE_INSERT_ARGUMENT_START: usize = 2;

// A copy reads only retained source indices; replacements never invoke a getter.
enum ArrayCopyPlan<'a> {
    Reversed,
    Replaced {
        index: usize,
        value: Option<RootId>,
    },
    Spliced {
        start: usize,
        deleted: usize,
        inserted: &'a [RootId],
    },
}

enum ArrayCopyElement {
    Source(usize),
    Inserted(Option<RootId>),
}

impl ArrayCopyPlan<'_> {
    fn element(&self, index: usize, source_length: usize) -> ArrayCopyElement {
        match *self {
            Self::Reversed => ArrayCopyElement::Source(source_length - 1 - index),
            Self::Replaced {
                index: replaced,
                value,
            } if index == replaced => ArrayCopyElement::Inserted(value),
            Self::Replaced { .. } => ArrayCopyElement::Source(index),
            Self::Spliced { start, .. } if index < start => ArrayCopyElement::Source(index),
            Self::Spliced {
                start, inserted, ..
            } if index < start + inserted.len() => {
                ArrayCopyElement::Inserted(Some(inserted[index - start]))
            }
            Self::Spliced {
                deleted, inserted, ..
            } => ArrayCopyElement::Source(index - inserted.len() + deleted),
        }
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn array_copy_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = self.box_object_or_type_error(p, this)?;
        let source = self.heap.root(source);
        let used_arguments = match native {
            Native::ArrayToReversed => &[][..],
            Native::ArrayWith => &args[..args.len().min(WITH_ARGUMENT_COUNT)],
            Native::ArrayToSpliced => args,
            _ => unreachable!("non-copying native routed to array copy dispatch"),
        };
        let arguments: Vec<_> = used_arguments
            .iter()
            .map(|value| self.heap.root(*value))
            .collect();
        let mut target = None;
        let outcome = (|| {
            let source_value = self.heap.root_value(source).unwrap();
            let length = self.array_like_length(p, source_value)?;
            let (plan, result_length) = match native {
                Native::ArrayToReversed => (ArrayCopyPlan::Reversed, length),
                Native::ArrayWith => {
                    if length > MAX_ARRAY_LENGTH {
                        return Err(self.range_error(p, "invalid array length".into()));
                    }
                    let index = arguments
                        .first()
                        .map(|root| self.heap.root_value(*root).unwrap())
                        .unwrap_or(Value::UNDEFINED);
                    let number = self.to_number(p, index)?;
                    let index = if number.is_nan() { 0.0 } else { number.trunc() };
                    let actual_index = if index < 0.0 {
                        length as f64 + index
                    } else {
                        index
                    };
                    if actual_index < 0.0 || actual_index >= length as f64 {
                        return Err(self.range_error(p, "array index out of range".into()));
                    }
                    (
                        ArrayCopyPlan::Replaced {
                            index: actual_index as usize,
                            value: arguments.get(1).copied(),
                        },
                        length,
                    )
                }
                Native::ArrayToSpliced => {
                    let start = arguments
                        .first()
                        .copied()
                        .map(|root| {
                            let value = self.heap.root_value(root).unwrap();
                            self.array_relative_index(p, value, length)
                        })
                        .transpose()?
                        .unwrap_or(0);
                    let remaining = length - start;
                    let deleted = match arguments.get(1).copied() {
                        None if arguments.is_empty() => 0,
                        None => remaining,
                        Some(root) => {
                            let value = self.heap.root_value(root).unwrap();
                            let number = self.to_number(p, value)?;
                            if number.is_nan() || number <= 0.0 {
                                0
                            } else if number.is_infinite() {
                                remaining
                            } else {
                                (number.trunc() as usize).min(remaining)
                            }
                        }
                    };
                    let inserted = arguments
                        .get(SPLICE_INSERT_ARGUMENT_START..)
                        .unwrap_or_default();
                    let result_length = length - deleted + inserted.len();
                    if result_length as f64 > MAX_SAFE_INTEGER {
                        return Err(
                            self.type_error(p, "array-like length exceeds safe integer".into())
                        );
                    }
                    (
                        ArrayCopyPlan::Spliced {
                            start,
                            deleted,
                            inserted,
                        },
                        result_length,
                    )
                }
                _ => unreachable!("non-copying native routed to array copy dispatch"),
            };
            if result_length > MAX_ARRAY_LENGTH {
                return Err(self.range_error(p, "invalid array length".into()));
            }
            let result = self.new_array(Vec::with_capacity(result_length));
            let result_root = self.heap.root(result);
            target = Some(result_root);
            for index in 0..result_length {
                let value = match plan.element(index, length) {
                    ArrayCopyElement::Source(index) => {
                        let source = self.heap.root_value(source).unwrap();
                        self.get_index(p, source, Value::number(index as f64))?
                    }
                    ArrayCopyElement::Inserted(root) => root
                        .map(|root| self.heap.root_value(root).unwrap())
                        .unwrap_or(Value::UNDEFINED),
                };
                let result = self.heap.root_value(result_root).unwrap();
                let Some(cell @ Cell::Array { .. }) = self.heap.get_mut(result) else {
                    unreachable!("fresh copy target remains an array");
                };
                let elements = cell.array_elements_mut();
                Rc::make_mut(elements).push(value);
            }
            Ok(self.heap.root_value(result_root).unwrap())
        })();
        if let Some(target) = target {
            self.heap.release_root(target);
        }
        for argument in arguments {
            self.heap.release_root(argument);
        }
        self.heap.release_root(source);
        outcome
    }
}
