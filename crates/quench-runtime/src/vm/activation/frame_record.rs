//! The heap record of a frame: everything its activation needs to run again, owned in one
//! place instead of spread over a pooled `Frame` and its continuation. A suspended function's
//! (a generator, an async function) holds it while suspended and a lane activation runs over its
//! register file; an ordinary function's activation keeps its scope chain on the lane stack and
//! has no record, a frame being what holds its dynamic bindings.

use super::CallContext;
use crate::bytecode::Atom;
use crate::compile::liveness::RegisterSet;
use crate::snapshot::{Reader, Snap, SnapshotResult, Writer};
use crate::heap::EnvironmentSlots;
use crate::vm::dispatch_fast::{WINDOW_ENVIRONMENT, WINDOW_LOCALS};
use crate::vm::program_store::ProgramId;
use crate::vm::register_file::RegisterFile;
use crate::vm::{ActiveIterator, CaptureCache, Frame};
use crate::Value;

/// A frame's record. Its window is its register file's first register (`WINDOW`, `staged::frame`),
/// so it names no address of its own; the receiver lives in the record alone (`receiver`,
/// `THIS`). The program is the one the frame resumes in, and the realm the owner's
/// (`GeneratorRecord::realm`, the await state's).
pub(crate) struct FrameRecord {
    /// The receiver, as value bits (the frame prefix's THIS word, `staged::frame`).
    receiver: u64,
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
    /// The register file, with the frame context words below register 0 that a lane window
    /// over it writes (`register_file::FRAME_CONTEXT`). Only its values change while
    /// suspended, never its length, so `WINDOW` stays valid.
    pub(in crate::vm) registers: RegisterFile,
    pub(in crate::vm) active_iterators: Vec<ActiveIterator>,
    /// The scope stack the frame detached when it suspended: the `with` objects of its
    /// scope chain, those its closure captured and those it entered, outermost first. While a
    /// lane activation runs the frame, the chain is the lane stack's (`LaneStack::with_objects`).
    pub(in crate::vm) with_objects: Vec<Value>,
    /// The registers live at the suspension, when liveness covers the function: only these
    /// can name a cell, so the GC traces them alone. `None` traces every register. The others
    /// keep what they held, cleared only on resume after a collection (`settle_registers`).
    pub(in crate::vm) live_registers: Option<RegisterSet>,
    /// The heap's collection epoch when the frame suspended (`Heap::collection_epoch`).
    suspended_epoch: u64,
    /// What its windowed resumes derive once (`generator_windows::window_plan`).
    pub(in crate::vm) window_plan: Option<crate::vm::dispatch_fast::WindowPlan>,
    /// Skip one function-entry tier attempt when the frame runs again (`Frame::js_jit_skip_entry_once`).
    pub(in crate::vm) js_jit_skip_entry_once: bool,
    /// An environment window follows the registers in the file's spare capacity while a lane
    /// activation runs the frame (`open_environment_window`): the locals and environment
    /// live there until the run ends (`close_environment_window`).
    environment_window: bool,
    /// The resume pc of the binding site the direct eval the activation is running was called
    /// at (`CallerView::binding_site`).
    pub(in crate::vm) binding_site_pc: Option<u32>,
}

#[cfg(test)]
thread_local! {
    /// The frames made on this thread from a suspension or a lane activation (`note_frame_made`):
    /// a lane resume or run that stays in the lane makes none.
    static FRAMES_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many frames resuming (`FrameRecord::into_frame`) or materializing a lane activation
/// (`Vm::push_script_frame_with_registers`) made on this thread.
#[cfg(test)]
pub(in crate::vm) fn frames_made() -> usize {
    FRAMES_MADE.get()
}

/// Counts a frame made from a suspension or a lane activation.
#[cfg(test)]
pub(in crate::vm) fn note_frame_made() {
    FRAMES_MADE.set(FRAMES_MADE.get() + 1);
}

impl FrameRecord {
    /// The suspended receiver.
    pub(in crate::vm) fn this(&self) -> Value {
        Value::from_bits(self.receiver)
    }

    /// The value the instruction the frame suspended at evaluates to, which a resume sends: it
    /// goes to `register`, where the suspension left one.
    pub(in crate::vm) fn receive(&mut self, register: Option<u16>, sent: Value) {
        if let Some(slot) = register.and_then(|register| self.registers.get_mut(usize::from(register))) {
            *slot = sent;
        }
    }

    /// Lays the environment window of a lane activation over the register file after its
    /// registers, as a call lays a fresh one (`lane_activate_environment`): the environment (the
    /// frame's own, once it made one, with `own_slots` its slot storage; else `DELETED`, the
    /// slots living among the locals in place), the slot storage's address, the `captured` mask,
    /// the in-place storage words, then the locals. The words live in the file's spare
    /// capacity, so a frame that never ran in a lane carries none of them.
    pub(in crate::vm) fn open_environment_window(&mut self, captured: u64, own_slots: Option<*const EnvironmentSlots>) {
        let count = self.registers.len();
        self.registers.reserve(usize::from(WINDOW_LOCALS) + self.locals.len() + crate::vm::dispatch_fast::INLINED_WINDOW_ROOM);
        // SAFETY: the capacity holds the entries and the locals after the registers, which
        // nothing else reads or writes while a window runs the frame.
        unsafe {
            let entries = self.registers.as_mut_ptr().add(count);
            match own_slots {
                None => crate::staged::call_environment::lay_out_entries(
                    &mut crate::staged::Concrete::new(std::ptr::null()),
                    entries.expose_provenance() as u64, captured, self.locals.len() as u16,
                ),
                Some(slots) => crate::staged::call_environment::write_entries(
                    &mut crate::staged::Concrete::new(std::ptr::null()),
                    entries.expose_provenance() as u64, self.env.to_bits(), slots.expose_provenance() as u64, captured,
                ),
            }
            entries.add(usize::from(WINDOW_LOCALS)).copy_from_nonoverlapping(self.locals.as_ptr(), self.locals.len());
        }
        self.environment_window = true;
    }

    /// Takes back what an environment window run changed: the locals, and the environment a
    /// closure made while it ran, which the frame then owns (`captured`). The frame's other
    /// fields, the parent environment included, are as it left them.
    pub(in crate::vm) fn close_environment_window(&mut self) {
        if !std::mem::take(&mut self.environment_window) {
            return;
        }
        // SAFETY: `open_environment_window` wrote the entries and the locals after the registers.
        unsafe { self.pull_window(self.registers.as_ptr().add(self.registers.len()), self.locals.len()) };
    }

    /// Takes the state of the environment window starting at `entries` (the entries
    /// `open_environment_window` lays out, or those of an ordinary activation's window): the
    /// environment a closure or a direct eval made, which the record then owns (`captured`),
    /// and the `locals` locals, which an eval's stores read and write through the record.
    ///
    /// # Safety
    /// `entries` starts a live environment window of `locals` locals.
    pub(in crate::vm) unsafe fn pull_window(&mut self, entries: *const Value, locals: usize) {
        // SAFETY: as the caller guarantees.
        unsafe {
            let environment = *entries.add(usize::from(WINDOW_ENVIRONMENT));
            if !environment.is_deleted() {
                self.env = environment;
                self.captured = true;
            }
            self.locals.resize(locals, Value::UNDEFINED);
            self.locals.as_mut_ptr().copy_from_nonoverlapping(entries.add(usize::from(WINDOW_LOCALS)), locals);
        }
    }

    /// Gives the window the locals a direct eval left in the record (`pull_window`).
    ///
    /// # Safety
    /// `entries` starts the live environment window `pull_window` read.
    pub(in crate::vm) unsafe fn push_window_locals(&self, entries: *mut Value) {
        // SAFETY: as the caller guarantees; the record holds as many locals as the window.
        unsafe { entries.add(usize::from(WINDOW_LOCALS)).copy_from_nonoverlapping(self.locals.as_ptr(), self.locals.len()) };
    }

    /// The record of an ordinary activation of `function` of `program` that runs a direct eval
    /// (`lane_eval`): its closure `callee`, its receiver and the environment its closure
    /// captured, with no registers or locals of its own until the window is pulled.
    pub(in crate::vm) fn for_eval(program: ProgramId, function: u32, callee: Value, this: Value, env: Value) -> Box<Self> {
        let record = Self {
            receiver: this.to_bits(),
            context: CallContext::Function(callee),
            reflected_parameters: 0,
            original_arguments: Vec::new(),
            program,
            function,
            pc: 0,
            env,
            locals: Vec::new(),
            dynamic_bindings: Vec::new(),
            captured: false,
            registers: RegisterFile::new(),
            active_iterators: Vec::new(),
            with_objects: Vec::new(),
            live_registers: None,
            suspended_epoch: 0,
            window_plan: None,
            js_jit_skip_entry_once: false,
            environment_window: false,
            binding_site_pc: None,
        };
        Box::new(record)
    }

    /// Makes `this` the receiver an eval's code reads (`CallerView::this`) and returns the
    /// one it replaces, which the caller puts back when the eval ends.
    pub(in crate::vm) fn replace_this(&mut self, this: Value) -> Value {
        Value::from_bits(std::mem::replace(&mut self.receiver, this.to_bits()))
    }

    /// The suspension of a frame that ran in a lane activation over its register file: it resumes
    /// at `pc`, with the registers liveness finds live there. The iterators its loops pushed and
    /// the `with` objects its scope chain held while it ran come back first (`Vm::park_frame`).
    pub(in crate::vm) fn suspend_at(&mut self, pc: usize, live: (Option<RegisterSet>, u64)) {
        self.pc = pc;
        self.suspend_registers(live);
    }

    /// The registers live at the suspension and the collection epoch it happened in: the
    /// dead ones keep their values, which no collection traces (`extend_roots`).
    pub(in crate::vm) fn suspend_registers(&mut self, (live, epoch): (Option<RegisterSet>, u64)) {
        (self.live_registers, self.suspended_epoch) = (live, epoch);
    }

    /// The frame runs again, where every register is traced: the registers dead at its
    /// suspension are cleared when a collection ran since (epoch `epoch`), as the cells they
    /// named may be gone; with none, each still names a cell.
    pub(in crate::vm) fn settle_registers(&mut self, epoch: u64) {
        if let Some(live) = self.live_registers.take()
            && self.suspended_epoch != epoch
        {
            clear_dead_registers(&mut self.registers, live);
        }
    }

    /// `frame`'s suspended state, moved: its register file and buffers keep their storage. What
    /// a run derives for itself (the operation's field and binding site, the capture cache,
    /// a fixed `this`, the `with` base) is not kept.
    pub(in crate::vm) fn capture(frame: Box<Frame>) -> Box<Self> {
        let Frame {
            context, reflected_parameters, original_arguments, program, function, pc, env, this,
            locals, dynamic_bindings, captured, registers, active_iterators, with_objects,
            pc_is_resume: _, source_field: _, binding_site_pc: _,
            initialized_registers: _, capture_cache: _, with_base: _, js_jit_interpreter_started: _,
            fixed_this: _, js_jit_skip_entry_once,
        } = *frame;
        let record = Self {
            receiver: this.to_bits(), context, reflected_parameters, original_arguments, program, function, pc, env,
            locals, dynamic_bindings, captured, registers, active_iterators, with_objects,
            live_registers: None, suspended_epoch: 0, window_plan: None, js_jit_skip_entry_once, environment_window: false,
            binding_site_pc: None,
        };
        Box::new(record)
    }

    /// The suspended frame, ready to push: it continues after its suspending instruction,
    /// every register holds a value, and the state a run derives for itself starts anew.
    pub(in crate::vm) fn into_frame(mut self: Box<Self>, with_base: usize) -> Box<Frame> {
        #[cfg(test)]
        note_frame_made();
        self.settle_registers(u64::MAX);
        let this = self.this();
        let Self {
            receiver: _, context, reflected_parameters, original_arguments, program, function, pc, env,
            locals, dynamic_bindings, captured, registers, active_iterators, with_objects,
            live_registers: _, suspended_epoch: _, window_plan: _, js_jit_skip_entry_once, environment_window: _, binding_site_pc: _,
        } = *self;
        Box::new(Frame {
            context, reflected_parameters, original_arguments, program, function, pc,
            pc_is_resume: true,
            js_jit_skip_entry_once,
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
    pub(in crate::vm) fn extend_roots(&self, out: &mut Vec<Value>) {
        out.extend(self.context.callee());
        out.extend_from_slice(&self.original_arguments);
        out.extend_from_slice(&self.with_objects);
        out.extend([self.env, self.this()]);
        Value::extend_heap_values(out, &self.locals);
        out.extend(self.dynamic_bindings.iter().map(|(_, value)| *value));
        match self.live_registers {
            Some(live) => out.extend(
                self.registers.iter().enumerate().filter(|&(register, value)| value.is_heap() && live.contains(register)).map(|(_, &value)| value),
            ),
            None => Value::extend_heap_values(out, &self.registers),
        }
    }
}

impl std::fmt::Debug for FrameRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameRecord")
            .field("program", &self.program)
            .field("function", &self.function)
            .field("pc", &self.pc)
            .finish_non_exhaustive()
    }
}

impl Clone for FrameRecord {
    fn clone(&self) -> Self {
        Self {
            receiver: self.receiver,
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
            suspended_epoch: self.suspended_epoch,
            window_plan: None,
            js_jit_skip_entry_once: self.js_jit_skip_entry_once,
            environment_window: false,
            binding_site_pc: None,
        }
    }
}

/// A suspended frame as a snapshot records it, field by field; a restored frame traces all
/// its registers (`live_registers`) and starts its run anew on resume.
impl Snap for FrameRecord {
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
        match self.live_registers {
            Some(live) => {
                let mut registers = RegisterFile::from_registers(&self.registers);
                clear_dead_registers(&mut registers, live);
                registers.snap(writer)?;
            }
            None => self.registers.snap(writer)?,
        }
        self.active_iterators.snap(writer)?;
        self.with_objects.snap(writer)?;
        self.captured.snap(writer)
    }

    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let record = Self {
            context: Snap::unsnap(reader)?,
            reflected_parameters: Snap::unsnap(reader)?,
            original_arguments: Snap::unsnap(reader)?,
            program: Snap::unsnap(reader)?,
            function: Snap::unsnap(reader)?,
            pc: Snap::unsnap(reader)?,
            env: Snap::unsnap(reader)?,
            receiver: Value::unsnap(reader)?.to_bits(),
            locals: Snap::unsnap(reader)?,
            dynamic_bindings: Snap::unsnap(reader)?,
            registers: Snap::unsnap(reader)?,
            active_iterators: Snap::unsnap(reader)?,
            with_objects: Snap::unsnap(reader)?,
            captured: Snap::unsnap(reader)?,
            live_registers: None,
            suspended_epoch: 0,
            window_plan: None,
            js_jit_skip_entry_once: false,
            environment_window: false,
            binding_site_pc: None,
        };
        Ok(record)
    }
}

/// Clears the `registers` outside `live`: each past the set's width, and the runs of dead ones
/// below it (a statement's temporaries), found a machine word of the set at a time.
pub(in crate::vm) fn clear_dead_registers(registers: &mut [Value], live: RegisterSet) {
    let covered = registers.len().min(RegisterSet::BITS as usize);
    let (low, high) = registers.split_at_mut(covered);
    high.fill(Value::UNDEFINED);
    for (word, registers) in low.chunks_mut(u64::BITS as usize).enumerate() {
        let mut dead = !live.word(word) & u64::MAX >> (u64::BITS as usize - registers.len());
        while dead != 0 {
            let start = dead.trailing_zeros();
            let end = start + (dead >> start).trailing_ones();
            registers[start as usize..end as usize].fill(Value::UNDEFINED);
            dead &= u64::MAX.checked_shl(end).unwrap_or(0);
        }
    }
}
