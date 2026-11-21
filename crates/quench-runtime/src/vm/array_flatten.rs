use super::*;

const DEFAULT_FLAT_DEPTH: usize = 1;
const UNBOUNDED_FLAT_DEPTH: usize = usize::MAX;

/// One array being flattened. Its `source` stays on the call roots (`active_call_roots`)
/// while the frame is live: frames nest last in, first out, as those roots do.
struct FlattenFrame {
    source: Value,
    length: usize,
    index: usize,
    depth: usize,
}

impl Vm {
    pub(super) fn array_flatten_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let argument = args.first().copied().unwrap_or(Value::UNDEFINED);
        let receiver = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        self.with_call_roots([this, argument, receiver], |vm| {
            let source = vm.box_object_or_type_error(p, this)?;
            vm.active_call_roots.push(source);
            let length = vm.array_like_length(p, source)?;
            let mut depth = DEFAULT_FLAT_DEPTH;
            let mapper = if native == Native::ArrayFlatMap {
                if !vm.is_function(argument) {
                    return Err(vm.type_error(p, "flatMap callback is not callable".into()));
                }
                Some((argument, receiver))
            } else {
                if argument != Value::UNDEFINED {
                    depth = match vm.to_number(p, argument)? {
                        value if value.is_nan() || value <= 0.0 => 0,
                        value if value.is_infinite() => UNBOUNDED_FLAT_DEPTH,
                        value => value.trunc() as usize,
                    };
                }
                None
            };
            let target = vm.array_species_create(p, source, 0)?;
            vm.active_call_roots.push(target);
            let mut frames = vec![FlattenFrame { source, length, index: 0, depth }];
            match mapper {
                Some((callback, receiver)) => vm.with_prepared_callback(callback, receiver, |vm, prepared| {
                    vm.flatten_into(p, target, &mut frames, Some(prepared))
                })?,
                None => vm.flatten_into(p, target, &mut frames, None)?,
            }
            Ok(target)
        })
    }

    /// Runs `frames` to completion, appending leaves to `target`. Each nested frame pushes its
    /// source onto the call roots and pops it when done; the caller's `with_call_roots`
    /// drops whatever an error leaves.
    fn flatten_into(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        frames: &mut Vec<FlattenFrame>,
        mut mapper: Option<&mut PreparedCallback>,
    ) -> Result<(), JsError> {
        let mut target_index = 0;
        while let Some(frame) = frames.last_mut() {
            if frame.index == frame.length {
                frames.pop();
                if !frames.is_empty() {
                    self.active_call_roots.pop();
                }
                continue;
            }
            let (source, index, depth) = (frame.source, frame.index, frame.depth);
            frame.index += 1;
            let key = Value::number(index as f64);
            if !self.has_property(p, source, key)? {
                continue;
            }
            let mut value = self.get_index(p, source, key)?;
            if frames.len() == 1
                && let Some(prepared) = mapper.as_deref_mut()
            {
                value = self.call_prepared(p, prepared, &[value, key, source])?;
            }
            self.active_call_roots.push(value);
            if depth > 0 && self.is_array(p, value)? {
                let length = self.array_like_length(p, value)?;
                let depth = if depth == UNBOUNDED_FLAT_DEPTH { depth } else { depth - 1 };
                // The pushed root stays as the nested frame's.
                frames.push(FlattenFrame { source: value, length, index: 0, depth });
                continue;
            }
            if target_index as f64 >= MAX_SAFE_INTEGER {
                return Err(self.type_error(p, "flattened array index exceeds safe integer".into()));
            }
            self.create_data_property_or_throw(p, target, target_index, value)?;
            self.active_call_roots.pop();
            target_index += 1;
        }
        Ok(())
    }
}
