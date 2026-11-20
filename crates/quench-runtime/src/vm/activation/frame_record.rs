//! The heap record of a frame: everything its activation needs to run again, owned in one
//! place instead of spread over a pooled `Frame` and its continuation. A suspended function's
//! (a generator, an async function) holds it while suspended and a lane activation runs over its
//! register file; an ordinary function's activation keeps its scope chain on the lane stack and
//! has no record, a frame being what holds its dynamic bindings.

use super::CallContext;
use super::frame_cold::FrameCold;
use super::suspension_live::SuspensionLive;
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
    /// The reflected parameters and kept arguments, dynamic bindings, `with` objects and
    /// binding site, when the frame holds any (`FrameCold`).
    pub(super) cold: Option<Box<FrameCold>>,
    pub(in crate::vm) program: ProgramId,
    pub(in crate::vm) function: u32,
    pub(in crate::vm) pc: usize,
    pub(in crate::vm) env: Value,
    pub(in crate::vm) locals: Vec<Value>,
    pub(in crate::vm) captured: bool,
    /// The register file, with the frame context words below register 0 that a lane window
    /// over it writes (`register_file::FRAME_CONTEXT`). Only its values change while
    /// suspended, never its length, so `WINDOW` stays valid.
    pub(in crate::vm) registers: RegisterFile,
    pub(in crate::vm) active_iterators: Vec<ActiveIterator>,
    /// Whether the registers the last suspension found live (`suspension_live`) are the only
    /// ones the GC traces (`live_registers`): set while suspended, until the frame runs again
    /// (`settle_registers`). The others keep what they held, cleared only on resume after a
    /// collection. A flag beside the set, so a suspension at the pc of the last one writes no
    /// copy of it, and a resume reads none.
    traces_live: bool,
    /// The heap's collection epoch when the frame suspended (`Heap::collection_epoch`).
    suspended_epoch: u64,
    /// The last suspension's pc and the registers live there, when liveness covers the function
    /// (`Vm::park_frame`, `suspend_registers`).
    suspension_live: Option<SuspensionLive>,
    /// What its windowed resumes derive once (`generator_windows::window_plan`).
    pub(in crate::vm) window_plan: Option<crate::vm::dispatch_fast::WindowPlan>,
    /// Skip one function-entry tier attempt when the frame runs again (`Frame::js_jit_skip_entry_once`).
    pub(in crate::vm) js_jit_skip_entry_once: bool,
    /// An environment window follows the registers in the file's spare capacity while a lane
    /// activation runs the frame (`open_environment_window`): the locals and environment
    /// live there until the run ends (`close_environment_window`).
    environment_window: bool,
    /// The record of the generator whose lane window runs the frame, set as the window resumes
    /// it (`Vm::resume_generator_window`) and read as it yields (`Vm::suspend_generator_window`):
    /// a running generator is rooted, and its record boxed, so its yield needs no lookup. Null
    /// where no generator's window ran the frame; stale once the run ends, and read by none then.
    pub(in crate::vm) resuming_generator: *mut super::GeneratorRecord,
    /// The frame's own `this` is authoritative (`Frame::fixed_this`), as its call decided
    /// (`Vm::initialize_activation_bindings`): an activation that pushed no lexical `this`
    /// binding has no other place to read it from when it runs again.
    fixed_this: bool,
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
    /// registers, as a call lays a fresh one (`activate_environment_in_lane`): the environment (the
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

    /// A record at rest over `registers`, with nothing a run derived yet. Its fields are spelled
    /// here alone: the constructors and `clone` set what they carry over, through struct update.
    #[inline(always)]
    fn at_rest(program: ProgramId, function: u32, registers: RegisterFile) -> Self {
        Self {
            receiver: Value::UNDEFINED.to_bits(),
            context: CallContext::Internal,
            cold: None,
            program,
            function,
            pc: 0,
            env: Value::NULL,
            locals: Vec::new(),
            captured: false,
            registers,
            active_iterators: Vec::new(),
            traces_live: false,
            suspension_live: None,
            suspended_epoch: 0,
            window_plan: None,
            js_jit_skip_entry_once: false,
            environment_window: false,
            resuming_generator: std::ptr::null_mut(),
            fixed_this: false,
        }
    }

    /// The record of an ordinary activation of `function` of `program` that runs a direct eval
    /// (`lane_eval`): its closure `callee`, its receiver and the environment its closure
    /// captured, with no registers or locals of its own until the window is pulled.
    pub(in crate::vm) fn for_eval(program: ProgramId, function: u32, callee: Value, this: Value, env: Value) -> Box<Self> {
        Box::new(Self {
            receiver: this.to_bits(),
            context: CallContext::Function(callee),
            env,
            ..Self::at_rest(program, function, RegisterFile::new())
        })
    }

    /// Makes `this` the receiver an eval's code reads (`CallerView::this`) and returns the
    /// one it replaces, which the caller puts back when the eval ends.
    pub(in crate::vm) fn replace_this(&mut self, this: Value) -> Value {
        Value::from_bits(std::mem::replace(&mut self.receiver, this.to_bits()))
    }

    /// The suspension of a frame that ran in a lane activation over its register file: it resumes
    /// at `pc`, in the collection epoch `epoch`, with the registers live there that
    /// `suspension_live` already holds for `pc` (`Vm::park_frame`). The iterators its loops
    /// pushed and the `with` objects its scope chain held while it ran come back first.
    #[inline(always)]
    pub(in crate::vm) fn suspend_at(&mut self, pc: usize, epoch: u64) {
        debug_assert!(self.suspension_live.as_ref().is_none_or(|live| live.at(pc)));
        self.pc = pc;
        (self.traces_live, self.suspended_epoch) = (true, epoch);
    }

    /// The registers live at the suspension at `pc` (`None` where liveness does not cover the
    /// function) and the collection epoch it happened in: the dead ones keep their values, which
    /// no collection traces (`extend_roots`).
    pub(in crate::vm) fn suspend_registers(&mut self, (live, epoch): (Option<RegisterSet>, u64)) {
        SuspensionLive::store(&mut self.suspension_live, self.pc, live);
        (self.traces_live, self.suspended_epoch) = (true, epoch);
    }

    /// The registers the GC traces alone while the frame is suspended, when liveness covers
    /// its function; `None` traces every register.
    #[inline(always)]
    pub(in crate::vm) fn live_registers(&self) -> Option<RegisterSet> {
        if !self.traces_live {
            return None;
        }
        self.suspension_live.as_ref().and_then(SuspensionLive::live)
    }

    /// Whether the last suspension, whose live registers the record keeps, was at `pc`.
    #[inline(always)]
    pub(in crate::vm) fn suspended_live_at(&self, pc: usize) -> bool {
        self.suspension_live.as_ref().is_some_and(|live| live.at(pc))
    }

    /// Keeps the registers live at a suspension at `pc` (`None`: liveness does not cover the
    /// function) for each next suspension there (`Vm::park_frame`).
    pub(in crate::vm) fn note_suspension_live(&mut self, pc: usize, live: Option<RegisterSet>) {
        SuspensionLive::store(&mut self.suspension_live, pc, live);
    }

    /// The frame runs again, where every register is traced: the registers dead at its
    /// suspension are cleared when a collection ran since (epoch `epoch`), as the cells they
    /// named may be gone; with none, each still names a cell.
    #[inline(always)]
    pub(in crate::vm) fn settle_registers(&mut self, epoch: u64) {
        if self.suspended_epoch != epoch && self.traces_live {
            self.clear_dead_registers_after_collection();
        }
        self.traces_live = false;
    }

    /// `settle_registers` after a collection: the registers outside the suspension's live set
    /// cleared.
    #[inline(never)]
    fn clear_dead_registers_after_collection(&mut self) {
        if let Some(live) = self.suspension_live.as_ref().and_then(SuspensionLive::live) {
            clear_dead_registers(&mut self.registers, live);
        }
    }

    /// `frame`'s suspended state, moved: its register file and buffers keep their storage, but
    /// for the rarely held ones that are empty (`FrameCold`), which are freed. What a run derives
    /// for itself (the operation's field and binding site, the capture cache,
    /// the `with` base) is not kept.
    pub(in crate::vm) fn capture(frame: Box<Frame>) -> Box<Self> {
        let Frame {
            context, reflected_parameters, original_arguments, program, function, pc, env, this,
            locals, dynamic_bindings, captured, registers, active_iterators, with_objects,
            pc_is_resume: _, source_field: _, binding_site_pc: _,
            initialized_registers: _, capture_cache: _, with_base: _, js_jit_interpreter_started: _,
            fixed_this, js_jit_skip_entry_once,
        } = *frame;
        let cold = FrameCold { reflected_parameters, original_arguments, dynamic_bindings, with_objects, binding_site_pc: None }.boxed();
        Box::new(Self {
            receiver: this.to_bits(), context, cold, pc, env, locals, captured, active_iterators, js_jit_skip_entry_once, fixed_this,
            ..Self::at_rest(program, function, registers)
        })
    }

    /// The suspended frame, ready to push: it continues after its suspending instruction,
    /// every register holds a value, and the state a run derives for itself starts anew.
    pub(in crate::vm) fn into_frame(mut self: Box<Self>, with_base: usize) -> Box<Frame> {
        #[cfg(test)]
        note_frame_made();
        self.settle_registers(u64::MAX);
        self.into_buffers(with_base)
    }

    /// The finished activation's buffers as a frame for the pool (`Vm::pool_finished_frame`):
    /// no run reads its values again.
    pub(in crate::vm) fn into_buffers(self: Box<Self>, with_base: usize) -> Box<Frame> {
        let this = self.this();
        let Self {
            receiver: _, context, cold, program, function, pc, env, locals, captured, registers, active_iterators,
            traces_live: _, suspended_epoch: _, suspension_live: _, window_plan: _, js_jit_skip_entry_once, environment_window: _,
            resuming_generator: _, fixed_this,
        } = *self;
        let FrameCold { reflected_parameters, original_arguments, dynamic_bindings, with_objects, binding_site_pc: _ } = FrameCold::unbox(cold);
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
            fixed_this,
        })
    }

    /// Its edges, appended to `out`. Locals and registers contribute only their heap values,
    /// and registers only the live ones where the suspension knows them (`live_registers`):
    /// each other one would cost the mark a push and a pop to find it names no cell.
    pub(in crate::vm) fn extend_roots(&self, out: &mut Vec<Value>) {
        let cold = self.cold();
        out.extend(self.context.callee());
        out.extend_from_slice(&cold.original_arguments);
        out.extend_from_slice(&cold.with_objects);
        out.extend([self.env, self.this()]);
        Value::extend_heap_values(out, &self.locals);
        out.extend(cold.dynamic_bindings.iter().map(|(_, value)| *value));
        match self.live_registers() {
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
            cold: self.cold.as_deref().and_then(|cold| FrameCold { binding_site_pc: None, ..cold.clone() }.boxed()),
            pc: self.pc,
            env: self.env,
            locals: self.locals.clone(),
            captured: self.captured,
            active_iterators: self.active_iterators.clone(),
            traces_live: self.traces_live,
            suspension_live: self.suspension_live.clone(),
            suspended_epoch: self.suspended_epoch,
            js_jit_skip_entry_once: self.js_jit_skip_entry_once,
            fixed_this: self.fixed_this,
            // A clone runs its own window and plan, so these start unset.
            ..Self::at_rest(self.program, self.function, self.registers.clone())
        }
    }
}

/// A suspended frame as a snapshot records it, field by field; a restored frame traces all
/// its registers (`live_registers`) and starts its run anew on resume.
impl Snap for FrameRecord {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        let cold = self.cold();
        self.context.snap(writer)?;
        cold.reflected_parameters.snap(writer)?;
        cold.original_arguments.snap(writer)?;
        self.program.snap(writer)?;
        self.function.snap(writer)?;
        self.pc.snap(writer)?;
        self.env.snap(writer)?;
        self.this().snap(writer)?;
        self.locals.snap(writer)?;
        cold.dynamic_bindings.snap(writer)?;
        match self.live_registers() {
            Some(live) => {
                let mut registers = RegisterFile::from_registers(&self.registers);
                clear_dead_registers(&mut registers, live);
                registers.snap(writer)?;
            }
            None => self.registers.snap(writer)?,
        }
        self.active_iterators.snap(writer)?;
        cold.with_objects.snap(writer)?;
        self.captured.snap(writer)
    }

    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let context = Snap::unsnap(reader)?;
        let mut cold = FrameCold { reflected_parameters: Snap::unsnap(reader)?, original_arguments: Snap::unsnap(reader)?, ..FrameCold::EMPTY };
        let (program, function, pc, env) = (Snap::unsnap(reader)?, Snap::unsnap(reader)?, Snap::unsnap(reader)?, Snap::unsnap(reader)?);
        let receiver = Value::unsnap(reader)?.to_bits();
        let locals = Snap::unsnap(reader)?;
        cold.dynamic_bindings = Snap::unsnap(reader)?;
        let (registers, active_iterators) = (Snap::unsnap(reader)?, Snap::unsnap(reader)?);
        cold.with_objects = Snap::unsnap(reader)?;
        Ok(Self {
            receiver, context, cold: cold.boxed(), pc, env, locals, active_iterators,
            captured: Snap::unsnap(reader)?,
            ..Self::at_rest(program, function, registers)
        })
    }
}

/// Appends the receiver and environment of an activation to `out`, the two context words every
/// activation holds beside its registers, which a collection traces as roots.
#[inline]
pub(in crate::vm) fn extend_this_env_roots(out: &mut Vec<Value>, this: Value, env: Value) {
    out.extend([this, env]);
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
