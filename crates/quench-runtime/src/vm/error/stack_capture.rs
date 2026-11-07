//! The call sites an error or `Error.captureStackTrace` records from the live frames.

use super::*;

impl<H: Host> Vm<H> {
    /// The call sites `Error.captureStackTrace` records, trimmed through `constructor_opt`
    /// when it is a function on the stack, up to `Error.stackTraceLimit`.
    pub(super) fn captured_stack_data(&mut self, constructor_opt: Value) -> StackData {
        self.materialize_deferred_lane();
        let trim_constructor =
            constructor_opt != Value::UNDEFINED && self.is_function(constructor_opt);
        let capture_limit = self.error_stack_trace_limit();
        let limit = capture_limit.unwrap_or(0);
        let mut records = Vec::with_capacity(self.frames.len().min(limit));
        let mut source = None;
        let mut source_captured = false;
        let mut native_frames = self.native_stack_frames.iter().rev().peekable();
        // V8's Error.captureStackTrace only trims through constructorOpt when
        // that function is present in the current stack. Async continuations
        // commonly resume after the requested function has left the active
        // stack (for example assert.rejects); in that case retain the full
        // stack instead of dropping every frame.
        let constructor_frame = trim_constructor
            .then(|| {
                self.frames
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(index, frame)| {
                        (frame.context.callable() == Some(constructor_opt)).then_some(index)
                    })
            })
            .flatten();
        let mut constructor_found = !trim_constructor || constructor_frame.is_none();
        let mut hide_identity = false;
        for (depth, frame) in self.frames.iter().enumerate().rev() {
            while records.len() < limit
                && native_frames
                    .peek()
                    .is_some_and(|(native_depth, _)| *native_depth > depth)
            {
                let (_, name) = native_frames.next().unwrap();
                if constructor_found {
                    records.push(CallSiteRecord {
                        location: CallSiteLocation::Native,
                        name: CallSiteName::Static(JsString::from_str(name)),
                        this_value: Value::UNDEFINED,
                        hide_identity: true,
                        is_constructor: false,
                        eval_origin: None,
                    });
                }
            }
            if records.len() >= limit {
                break;
            }
            // A Script root has no exposed callee identity, but its compiler
            // source position is still a visible stack frame. Synthetic helper
            // activations remain hidden.
            if frame.context == CallContext::Internal && frame.function != ROOT_FUNCTION_ID {
                continue;
            }
            let Some(residual) = self.programs.get(frame.program) else {
                continue;
            };
            let Some(function) = residual.function_get(frame.function as usize) else {
                continue;
            };
            hide_identity |= function.strict;
            if !constructor_found {
                constructor_found = constructor_frame == Some(depth);
                continue;
            }
            if let Some((mut record, module)) = self.source_call_site_record(frame, hide_identity) {
                if !source_captured {
                    source_captured = true;
                    // Share immutable compiler text, not a copied line per Error.
                    // Captured positions remain valid after its Script is collected.
                    source.clone_from(&residual.source);
                }
                Self::map_call_site_source(&mut self.host, &mut record, module);
                records.push(record);
            }
        }

        if constructor_found {
            for (_, name) in native_frames.take(limit.saturating_sub(records.len())) {
                records.push(CallSiteRecord {
                    location: CallSiteLocation::Native,
                    name: CallSiteName::Static(JsString::from_str(name)),
                    this_value: Value::UNDEFINED,
                    hide_identity: true,
                    is_constructor: false,
                    eval_origin: None,
                });
            }
        }

        match capture_limit {
            Some(_) => StackData::Captured { records, source },
            None => StackData::Formatted(Value::UNDEFINED),
        }
    }
}
