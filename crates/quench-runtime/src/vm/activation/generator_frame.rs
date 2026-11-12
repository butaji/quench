//! The heap record of a suspended frame: everything its activation needs to run again,
//! owned in one place instead of spread over a pooled `Frame` and its continuation.

use super::CallContext;
use crate::bytecode::Atom;
use crate::compile::liveness::RegisterSet;
use crate::snapshot::{Reader, Snap, SnapshotResult, Writer};
use crate::vm::compiled::ShadowFrame;
use crate::vm::program_store::ProgramId;
use crate::vm::{ActiveIterator, CaptureCache, Frame};
use crate::Value;

/// A suspended frame. The prefix is the one emitted code writes for a frame (`CALLEE`,
/// `THIS`, `BODY`, `WINDOW`, `staged::frame`); its `WINDOW` word points into `registers`,
/// whose buffer a move of the record leaves in place and a clone makes anew
/// (`bind_window`). The receiver lives in the prefix alone (`this`); the program is the
/// one the frame resumes in, and the realm the owner's (`GeneratorRecord::realm`, the await
/// state's).
pub(crate) struct GeneratorFrame {
    prefix: ShadowFrame,
    pub(in crate::vm) context: CallContext,
    pub(in crate::vm) reflected_parameters: u16,
    pub(in crate::vm) original_arguments: Vec<Value>,
    pub(in crate::vm) program: ProgramId,
    pub(in crate::vm) function: u32,
    pub(in crate::vm) pc: usize,
    pub(in crate::vm) env: Value,
    pub(in crate::vm) locals: Vec<Value>,
    pub(in crate::vm) dynamic_bindings: Vec<(Atom, Value)>,
    pub(in crate::vm) captured: bool,
    /// The register file. Only its values change while suspended, never its length, so
    /// `WINDOW` stays valid.
    pub(in crate::vm) registers: Vec<Value>,
    pub(in crate::vm) active_iterators: Vec<ActiveIterator>,
    /// The scope stack the frame detached when it suspended.
    pub(in crate::vm) with_objects: Vec<Value>,
    /// The registers live at the suspension, when liveness covers the function and every
    /// other register was cleared there (`Vm::suspend_activation`): only these can name a
    /// cell, so the GC traces them alone. `None` traces every register.
    pub(in crate::vm) live_registers: Option<RegisterSet>,
}

impl GeneratorFrame {
    /// Binds `WINDOW` to the register file's first register.
    fn bind_window(&mut self) {
        self.prefix.window = self.registers.as_mut_ptr().addr();
    }

    /// The suspended receiver.
    pub(in crate::vm) fn this(&self) -> Value {
        Value::from_bits(self.prefix.this)
    }

    /// `frame`'s suspended state, moved: its register file and buffers keep their storage. What
    /// a run derives for itself (the operation's field and binding site, the capture cache,
    /// a fixed `this`, the `with` base) is not kept.
    pub(super) fn capture(frame: Box<Frame>) -> Box<Self> {
        let Frame {
            context, reflected_parameters, original_arguments, program, function, pc, env, this,
            locals, dynamic_bindings, captured, registers, active_iterators, with_objects,
            pc_is_resume: _, js_jit_skip_entry_once: _, source_field: _, binding_site_pc: _,
            initialized_registers: _, capture_cache: _, with_base: _, js_jit_interpreter_started: _,
            fixed_this: _,
        } = *frame;
        let prefix = ShadowFrame { this: this.to_bits(), ..ShadowFrame::EMPTY };
        let mut record = Self {
            prefix, context, reflected_parameters, original_arguments, program, function, pc, env,
            locals, dynamic_bindings, captured, registers, active_iterators, with_objects,
            live_registers: None,
        };
        record.bind_window();
        Box::new(record)
    }

    /// The suspended frame, ready to push: it continues after its suspending instruction,
    /// every register holds a value, and the state a run derives for itself starts anew.
    pub(super) fn into_frame(self: Box<Self>, with_base: usize) -> Box<Frame> {
        let this = self.this();
        let Self {
            prefix: _, context, reflected_parameters, original_arguments, program, function, pc, env,
            locals, dynamic_bindings, captured, registers, active_iterators, with_objects,
            live_registers: _,
        } = *self;
        Box::new(Frame {
            context, reflected_parameters, original_arguments, program, function, pc,
            pc_is_resume: true,
            js_jit_skip_entry_once: false,
            source_field: None,
            binding_site_pc: None,
            env, this, locals, dynamic_bindings, captured,
            initialized_registers: registers.len(),
            registers,
            capture_cache: CaptureCache::default(),
            active_iterators, with_objects, with_base,
            js_jit_interpreter_started: None,
            fixed_this: false,
        })
    }

    /// Its edges, appended to `out`. Locals and registers contribute only their heap values,
    /// and registers only the live ones where the suspension knows them (`live_registers`):
    /// each other one would cost the mark a push and a pop to find it names no cell.
    pub(super) fn extend_roots(&self, out: &mut Vec<Value>) {
        out.extend(self.context.callee());
        out.extend_from_slice(&self.original_arguments);
        out.extend_from_slice(&self.with_objects);
        out.extend([self.env, self.this()]);
        Value::extend_heap_values(out, &self.locals);
        out.extend(self.dynamic_bindings.iter().map(|(_, value)| *value));
        match self.live_registers {
            Some(mut live) => {
                while live != 0 {
                    let register = live.trailing_zeros() as usize;
                    live &= live - 1;
                    if let Some(&value) = self.registers.get(register)
                        && value.is_heap()
                    {
                        out.push(value);
                    }
                }
            }
            None => Value::extend_heap_values(out, &self.registers),
        }
    }
}

impl Clone for GeneratorFrame {
    fn clone(&self) -> Self {
        let mut copy = Self {
            prefix: self.prefix,
            context: self.context,
            reflected_parameters: self.reflected_parameters,
            original_arguments: self.original_arguments.clone(),
            program: self.program,
            function: self.function,
            pc: self.pc,
            env: self.env,
            locals: self.locals.clone(),
            dynamic_bindings: self.dynamic_bindings.clone(),
            captured: self.captured,
            registers: self.registers.clone(),
            active_iterators: self.active_iterators.clone(),
            with_objects: self.with_objects.clone(),
            live_registers: self.live_registers,
        };
        copy.bind_window();
        copy
    }
}

/// A suspended frame as a snapshot records it, field by field; a restored frame traces all
/// its registers (`live_registers`) and starts its run anew on resume.
impl Snap for GeneratorFrame {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.context.snap(writer)?;
        self.reflected_parameters.snap(writer)?;
        self.original_arguments.snap(writer)?;
        self.program.snap(writer)?;
        self.function.snap(writer)?;
        self.pc.snap(writer)?;
        self.env.snap(writer)?;
        self.this().snap(writer)?;
        self.locals.snap(writer)?;
        self.dynamic_bindings.snap(writer)?;
        self.registers.snap(writer)?;
        self.active_iterators.snap(writer)?;
        self.with_objects.snap(writer)?;
        self.captured.snap(writer)
    }

    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let mut record = Self {
            context: Snap::unsnap(reader)?,
            reflected_parameters: Snap::unsnap(reader)?,
            original_arguments: Snap::unsnap(reader)?,
            program: Snap::unsnap(reader)?,
            function: Snap::unsnap(reader)?,
            pc: Snap::unsnap(reader)?,
            env: Snap::unsnap(reader)?,
            prefix: ShadowFrame { this: Value::unsnap(reader)?.to_bits(), ..ShadowFrame::EMPTY },
            locals: Snap::unsnap(reader)?,
            dynamic_bindings: Snap::unsnap(reader)?,
            registers: Snap::unsnap(reader)?,
            active_iterators: Snap::unsnap(reader)?,
            with_objects: Snap::unsnap(reader)?,
            captured: Snap::unsnap(reader)?,
            live_registers: None,
        };
        record.bind_window();
        Ok(record)
    }
}
