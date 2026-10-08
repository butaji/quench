use super::*;

const DEFAULT_FLAT_DEPTH: usize = 1;
const UNBOUNDED_FLAT_DEPTH: usize = usize::MAX;

struct FlattenFrame {
    source: RootId,
    length: usize,
    index: usize,
    depth: usize,
}

impl<H: Host> Vm<H> {
    pub(super) fn array_flatten_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = self.box_object_or_type_error(p, this)?;
        let source = self.heap.root(source);
        let argument = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let receiver = (native == Native::ArrayFlatMap).then(|| {
            self.heap
                .root(args.get(1).copied().unwrap_or(Value::UNDEFINED))
        });
        let mut frames = vec![FlattenFrame {
            source,
            length: 0,
            index: 0,
            depth: DEFAULT_FLAT_DEPTH,
        }];
        let mut target = None;
        let mut mapper = None;
        let outcome = (|| {
            let source_value = self.heap.root_value(source).unwrap();
            frames[0].length = self.array_like_length(p, source_value)?;
            if native == Native::ArrayFlatMap {
                let callback = self.heap.root_value(argument).unwrap();
                if !self.is_function(callback) {
                    return Err(self.type_error(p, "flatMap callback is not callable".into()));
                }
                mapper = Some((argument, receiver.unwrap()));
            } else {
                frames[0].depth = match self.heap.root_value(argument).unwrap() {
                    Value::UNDEFINED => DEFAULT_FLAT_DEPTH,
                    value => match self.to_number(p, value)? {
                        value if value.is_nan() || value <= 0.0 => 0,
                        value if value.is_infinite() => UNBOUNDED_FLAT_DEPTH,
                        value => value.trunc() as usize,
                    },
                };
            }
            let source_value = self.heap.root_value(source).unwrap();
            let result = self.array_species_create(p, source_value, 0)?;
            let target_root = self.heap.root(result);
            target = Some(target_root);
            self.flatten_into(p, target_root, &mut frames, mapper)?;
            Ok(self.heap.root_value(target_root).unwrap())
        })();
        for frame in frames {
            self.heap.release_root(frame.source);
        }
        self.heap.release_root(argument);
        if let Some(receiver) = receiver {
            self.heap.release_root(receiver);
        }
        if let Some(target) = target {
            self.heap.release_root(target);
        }
        outcome
    }

    fn flatten_into(
        &mut self,
        p: &ResidualProgram,
        target: RootId,
        frames: &mut Vec<FlattenFrame>,
        mapper: Option<(RootId, RootId)>,
    ) -> Result<(), JsError> {
        let mut target_index = 0;
        while let Some(frame) = frames.last_mut() {
            if frame.index == frame.length {
                let frame = frames.pop().unwrap();
                self.heap.release_root(frame.source);
                continue;
            }
            let source_root = frame.source;
            let index = frame.index;
            let depth = frame.depth;
            frame.index += 1;
            let key = Value::number(index as f64);
            let source = self.heap.root_value(source_root).unwrap();
            if !self.has_property(p, source, key)? {
                continue;
            }
            let source = self.heap.root_value(source_root).unwrap();
            let mut value = self.get_index(p, source, key)?;
            if frames.len() == 1
                && let Some((callback, receiver)) = mapper
            {
                let source = self.heap.root_value(source_root).unwrap();
                let callback = self.heap.root_value(callback).unwrap();
                let receiver = self.heap.root_value(receiver).unwrap();
                value = self.call_value(p, callback, receiver, &[value, key, source])?;
            }
            let element = self.heap.root(value);
            let nested = (|| {
                let value = self.heap.root_value(element).unwrap();
                if depth > 0 && self.is_array(p, value)? {
                    let value = self.heap.root_value(element).unwrap();
                    Ok(Some(self.array_like_length(p, value)?))
                } else {
                    Ok(None)
                }
            })();
            match nested {
                Ok(Some(length)) => frames.push(FlattenFrame {
                    source: element,
                    length,
                    index: 0,
                    depth: if depth == UNBOUNDED_FLAT_DEPTH {
                        depth
                    } else {
                        depth - 1
                    },
                }),
                Ok(None) => {
                    let write = if target_index as f64 >= MAX_SAFE_INTEGER {
                        Err(self.type_error(p, "flattened array index exceeds safe integer".into()))
                    } else {
                        let target = self.heap.root_value(target).unwrap();
                        let value = self.heap.root_value(element).unwrap();
                        self.create_data_property_or_throw(p, target, target_index, value)
                    };
                    self.heap.release_root(element);
                    write?;
                    target_index += 1;
                }
                Err(error) => {
                    self.heap.release_root(element);
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}
