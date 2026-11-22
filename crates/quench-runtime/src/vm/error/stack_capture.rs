//! The call sites an error or `Error.captureStackTrace` records from the live frames: the VM
//! frames and, while a native runs beside a lane's activations, those activations read from the
//! lane's frame walk (`Vm::lane_stack_sites`, frame-switches.md §5) without making them frames.

use super::*;

/// One frame as a stack capture reads it: what `source_site_record` and the capture's trimming
/// read of a VM frame (`FrameSite::of`), or of a lane activation as the frame its
/// materialization would push (`Vm::lane_stack_sites`).
#[derive(Clone, Copy)]
pub(in crate::vm) struct FrameSite {
    pub(in crate::vm) program: ProgramId,
    pub(in crate::vm) function: u32,
    pub(in crate::vm) context: CallContext,
    pub(in crate::vm) this: Value,
    /// The pc of the instruction it is paused at (`Frame::source_instruction_pc`).
    pub(in crate::vm) pc: usize,
    pub(in crate::vm) source_field: Option<(usize, crate::bytecode::FieldPart)>,
}

impl FrameSite {
    /// A VM frame's site.
    #[inline]
    pub(in crate::vm) fn of(frame: &Frame) -> Self {
        Self {
            program: frame.program,
            function: frame.function,
            context: frame.context,
            this: frame.this,
            pc: frame.source_instruction_pc(),
            source_field: frame.source_field,
        }
    }
}

impl Vm {
    /// The field site a frame of `function` of `program` paused at the call record `pc` reads
    /// from, where that record is a nested read (`FieldLookup::Site`) whose last read's getter
    /// is the call: the general loop's caller carries it while the getter runs
    /// (`Vm::with_field_source`). A frame a lane activation's caller became, or the walk's site
    /// of one, carries it from here, so the getter's caller reads the same position.
    pub fn called_field_source(
        &self,
        program: ProgramId,
        function: u32,
        pc: usize,
    ) -> Option<(usize, crate::bytecode::FieldPart)> {
        let code = self.programs.residual(program)?;
        let site = code.function_get(function as usize)?.nested_read_site(pc)?;
        Some((site, code.field_sites.get(site)?.last_read_part()))
    }

    /// A VM frame's site (`FrameSite::of`): a frame paused at a call record (`pc_is_resume`)
    /// that names no field site of its own takes the one its call reads from
    /// (`called_field_source`). The general loop's frames run a nested read's getter with the
    /// field site set and their pc published unresumed, so only the frames lane activations
    /// became take it here.
    pub(in crate::vm) fn frame_site(&self, frame: &Frame) -> FrameSite {
        let mut site = FrameSite::of(frame);
        if frame.pc_is_resume && site.source_field.is_none() {
            site.source_field = self.called_field_source(site.program, site.function, site.pc);
        }
        site
    }
}

impl Vm {
    /// The call sites `Error.captureStackTrace` records, trimmed through `constructor_opt`
    /// when it is a function on the stack, up to `Error.stackTraceLimit`. While a native runs
    /// beside a lane's activations, they are read from the frame walk, the root frame paused at
    /// its call of the outermost (`lane_stack_sites`); where the walk declines, they become
    /// frames first, as before.
    pub(super) fn captured_stack_data(&mut self, constructor_opt: Value) -> StackData {
        let trim_constructor =
            constructor_opt != Value::UNDEFINED && self.is_function(constructor_opt);
        let capture_limit = self.error_stack_trace_limit();
        let limit = capture_limit.unwrap_or(0);
        // No frame is read where none is recorded: the activations stay as they are.
        let lane = if limit == 0 { None } else { self.lane_stack_sites() };
        if lane.is_none() && limit != 0 {
            self.materialize_deferred_lane();
        }
        let based = self.frames.len();
        let (root_pc, lane_sites) = lane.map_or((None, Vec::new()), |lane| (lane.root_pc, lane.sites));
        // The frame at `depth`, the lane's activations above the VM frames.
        let site_at = |vm: &Self, depth: usize| -> FrameSite {
            if let Some(site) = depth.checked_sub(based).map(|above| lane_sites[above]) {
                return site;
            }
            if depth + 1 == based && let Some(pc) = root_pc {
                // The root frame, paused at its call of the outermost activation (or at the
                // deferred call), as `resume_caller_frame` / `resume_deferred_caller` resume it.
                let mut site = FrameSite::of(&vm.frames[depth]);
                site.pc = pc;
                site.source_field = site.source_field.or_else(|| vm.called_field_source(site.program, site.function, pc));
                return site;
            }
            vm.frame_site(&vm.frames[depth])
        };
        let depths = if limit == 0 { 0 } else { based + lane_sites.len() };
        let mut records = Vec::with_capacity(depths.min(limit));
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
                (0..depths)
                    .rev()
                    .find(|&depth| site_at(self, depth).context.callable() == Some(constructor_opt))
            })
            .flatten();
        let mut constructor_found = !trim_constructor || constructor_frame.is_none();
        let mut hide_identity = false;
        for depth in (0..depths).rev() {
            let frame = site_at(self, depth);
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
            if let Some((mut record, module)) = self.source_site_record(frame, hide_identity) {
                if !source_captured {
                    source_captured = true;
                    // Share immutable compiler text, not a copied line per Error.
                    // Captured positions remain valid after its Script is collected.
                    source.clone_from(&residual.source);
                }
                Self::map_call_site_source(&mut *self.host, &mut record, module);
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
