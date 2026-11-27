//! The direct call row's frame (`call_rows.rs`, `call_direct_*`): the call protocol's hooks
//! (`staged::invoke::CallFrame`) over a site's `CallRecord`, so the row runs the staged call
//! (`invoke::invoke`, its `enter_direct`) as the interpreter and the u-jit do. What the record
//! fixes is the plan (`call_entry`): the callee's identity checked against the record's word, or
//! folded where a Stable fact holds it (`CHECKED` false); a sloppy callee's global from the record;
//! an arrow's `this` as the deleted marker its body never reads (a bound `this`, as the lane's frame
//! plans a lexical callee). The hooks the record cannot fix are the row ABI's:
//!
//! - the arguments: a leading argument may be a pinned register's value (`call_words`, read from the
//!   places the planner named; the rest from the caller's window);
//! - admission (`call_room`): the shared `admit_native` (one validity compare, one room check) plus
//!   the plain depth bound;
//! - entry (`call_enter`): the callee's plain header, its body in the body table, the lane's new
//!   tops, then its row entry or lane body entry, a `call reg` only a `call_*` row may make;
//! - settle (`call_settle`): the pop and the result register's write.
//!
//! A callee that ended with any other status is routed after the step (`ended`): not entered pops
//! and refuses, an exception pops, counts and raises, and a leave keeps the frames for the lane.

use crate::codes::{DOUBLE_PINS, LAST_INT_PIN, MEM, PIN, PINF};
use crate::heap_rows::{payload, set_context_word, word_at};
use crate::layout::call_record::{CalleeRecord, CallRecord, LaneStatus, PLACED_ARGUMENTS, REGISTER_BYTES};
use crate::layout::{CELL_BODY_AT, CELL_COUNTERS_AT, CELL_ENTRY_AT, CELL_NUMBERS_AT, CELL_PROVEN_AT, CELL_RECOVERY_AT, CELL_ROWS_AT,
    CELL_USES_THIS_AT, CONTEXT_BODY, CONTEXT_CAPTURE, CONTEXT_ENV, CONTEXT_THIS, FRAME_CONTEXT_WORDS, JS_STEP_ACTIVATION_COUNT,
    JS_STEP_BODY_TABLE, JS_STEP_LANE_VALUES_END, JS_STEP_NATIVE_TABLE, JS_STEP_POP_OBSERVED_HEADER, JS_STEP_RESIDUAL, JS_STEP_VM,
    PLAIN_DEPTH_LIMIT, RAISED_RUN, RECOVERY_ACTIVATIONS_AT, RECOVERY_VALUES_AT};
use crate::places::Places;
use crate::stage::StencilStage;
use crate::bytecode::Register;
use crate::staged::frame::{PLAIN_DEPTH_AT, PLAIN_REGISTERS_AT};
use crate::staged::invoke::{Entered, Room};
use crate::staged::plan::{Bound, WordList};
use crate::staged::region::Region;
use crate::staged::{call::admit_native, protocol, value, word_offset, Arguments, CallEntry, CallFrame, CalleeKind, Callee, Concrete,
    Constant, DirectPlan, Frame, Identity, Invocation, Receiver, Stage};
use crate::typed_ir::Representation;
use crate::Value;
use core::mem::{offset_of, transmute};

/// Shift from a word count to bytes.
const WORD_SHIFT: u32 = REGISTER_BYTES.trailing_zeros();
/// The last double register (xmm7, d7) a value may name: the double pins follow `PINF`.
const LAST_DOUBLE_PIN: u8 = PINF + DOUBLE_PINS - 1;

/// How a direct row binds the receiver: as it is, sloppy (`protocol::bind_this`), or none (an arrow).
pub(crate) const STRICT: u8 = 0;
pub(crate) const SLOPPY: u8 = 1;
pub(crate) const LEXICAL: u8 = 2;

/// A lane body's return: its status and result word in (eax, rdx).
#[repr(C)]
#[derive(Clone, Copy)]
struct BodyReturn {
    status: u32,
    value: u64,
}

/// A lane body's entry (`BodyEntry`): (context, receiver, capture word, registers).
type BodyEntry = unsafe extern "C" fn(usize, u64, u64, usize) -> BodyReturn;
/// A b-jit body's row entry (`entry_rows`, `EntryCell::rows`): (window, context, receiver, captures)
/// -> (word, status), in the row ABI, which saves no callee-saved register.
type RowsEntry = unsafe extern "rust-preserve-none" fn(usize, usize, u64, u64) -> RowsReturn;
#[repr(C)]
struct RowsReturn {
    value: u64,
    status: u64,
}

/// How a direct call ended.
pub(crate) enum Ended {
    /// Refused before any effect, or the callee's cell held the stub: the helper takes the call.
    Refused,
    /// The callee completed; its word is stored in the call's register.
    Returned,
    /// The callee ended by an exception: the caller raises it at the call.
    Raised,
    /// The callee left: its (status, word), which the caller leaves with.
    Left(u32, u64),
}

/// What admission found: the callee's cell, the lane stack's values' top where its window goes, the
/// window's size, and the plain depth and innermost window a pop restores.
#[derive(Clone, Copy)]
struct Admitted {
    cell: usize,
    top: usize,
    room: usize,
    /// The registers past the arguments the push starts `undefined` (`window_fill`): the rest are
    /// written before any read, as the interpreter's own call leaves them.
    fill: usize,
    /// The plain depth and innermost window before the push.
    depth: u64,
    registers: u64,
    /// The dispatch payload, whose VM and spilled-header helper a pop reads.
    payload: usize,
    frames: u64,
    /// The addresses of the lane's state words, read once at admission: every move writes through them.
    tops: protocol::LaneTops<u64>,
}

/// The frame of the direct row for `ARGC` arguments, receiver mode `MODE`, identity check `CHECKED`
/// and argument places `PLACED`: the caller's places, its call record, and what admission and the
/// callee's entry left for the row.
pub(crate) struct DirectFrame<'p, const ARGC: usize, const MODE: u8, const CHECKED: bool, const PLACED: bool> {
    /// The caller's places, a local of the row's own: their pinned registers are read at an index the record names,
    /// which would keep the rest of this frame in memory were they its field.
    places: &'p mut Places,
    record: usize,
    /// The words of the arguments read from their places (`PLACED` rows only).
    words: [u64; ARGC],
    admitted: Option<Admitted>,
    returned: Option<BodyReturn>,
}

impl<'p, const ARGC: usize, const MODE: u8, const CHECKED: bool, const PLACED: bool> DirectFrame<'p, ARGC, MODE, CHECKED, PLACED> {
    /// The frame of the row with the caller's `places` for the call record at `record`.
    #[inline(always)]
    pub(crate) fn new(places: &'p mut Places, record: usize) -> Self {
        Self { places, record, words: [0; ARGC], admitted: None, returned: None }
    }

    #[inline(always)]
    fn field(&self, at: usize) -> u64 {
        word_at(self.record, at)
    }

    /// The callee record the site names (`CallRecord::callee_record`): the callee's words are read through it.
    #[inline(always)]
    fn callee(&self) -> usize {
        word_at(self.record, offset_of!(CallRecord, callee_record)) as usize
    }

    /// A word of the callee record at `at` (an `offset_of!(CalleeRecord, _)`).
    #[inline(always)]
    fn callee_field(&self, at: usize) -> u64 {
        word_at(self.callee(), at)
    }

    /// The register index the record names at `at`, read as the frame's accesses are (`Region::Frame`: a call
    /// record lives on the lane stack).
    #[inline(always)]
    fn register(&self, at: usize) -> Register {
        StencilStage::from_hole(0).load_u16(Region::Frame.mutable(), self.record as u64, at as i32) as Register
    }

    /// The call the record makes: its callee register, receiver register and window of `ARGC` arguments.
    #[inline(always)]
    pub(crate) fn site(&mut self) -> Invocation {
        if PLACED {
            self.words = self.argument_words();
        }
        Invocation {
            callee: Callee::Register(self.register(offset_of!(CallRecord, callee_register))),
            receiver: Receiver::Register(self.register(offset_of!(CallRecord, this_register))),
            arguments: Arguments::Window { first: self.register(offset_of!(CallRecord, args_register)), count: ARGC as u16 },
            result: self.register(offset_of!(CallRecord, result_register)),
            returns: false,
            forwards: false,
        }
    }

    /// The tagged words of the `ARGC` arguments, each from the place the planner named for it
    /// (`CallRecord::argument_places`): a pinned int's or double's canonical word, else the caller's
    /// window slot. Only the leading `PLACED_ARGUMENTS` may be pinned.
    #[inline(always)]
    fn argument_words(&self) -> [u64; ARGC] {
        let s = &self.places.s;
        let mut stage = StencilStage::from_hole(0);
        let window = self.register(offset_of!(CallRecord, args_register));
        let places = (self.record + offset_of!(CallRecord, argument_places)) as *const u8;
        let mut words = [0u64; ARGC];
        for (k, word) in words.iter_mut().enumerate() {
            // SAFETY: the record's places are the planner's, for the live frame.
            let place = if k < PLACED_ARGUMENTS { unsafe { places.add(k).read() } } else { MEM };
            *word = match place {
                PIN..=LAST_INT_PIN => value::int_word(&mut stage, s.p[usize::from(place - PIN)] as u32 as u64),
                PINF..=LAST_DOUBLE_PIN => stage.box_number(crate::staged::Float64::by_fact(s.f[usize::from(place - PINF)].to_bits(), crate::staged::NumberFact::float64_home())),
                _ => stage.load_u64(Region::Frame.mutable(), s.frame as u64, word_offset(window + k as u16)),
            };
        }
        words
    }

    /// Pops the frame the call pushed: the plain depth, innermost window and body top it found, and the values' top.
    /// Where Rust observed the frame, the activation pops where Rust keeps it (`LaneStack::pop_observed_header`), as the
    /// u-jit's `pop_native` pops it: a nested entry to emitted code under the callee (`Vm::stack_words_in_lane`) made its
    /// native header a record (`LaneStack::spill_plain`), leaving no plain header, or a helper linked it
    /// (`native_frames::link_header`), which cleared its NATIVE flag and gave it a side record that pops with it.
    #[inline(always)]
    fn pop(a: &Admitted) {
        let mut stage = StencilStage::from_hole(0);
        protocol::pop_native_frame(&mut stage, &a.tops, a.top as u64, a.frames, Some((a.depth, a.registers)), #[inline(always)] |_| {
            core::hint::cold_path();
            // SAFETY: the payload's helper is `pop_observed_header_in_lane`, which takes the VM and residual words.
            unsafe {
                let helper: unsafe extern "C" fn(u64, u64) -> u64 = transmute(word_at(a.payload, JS_STEP_POP_OBSERVED_HEADER) as usize);
                helper(word_at(a.payload, JS_STEP_VM), word_at(a.payload, JS_STEP_RESIDUAL));
            }
        });
    }

    /// The row entry a direct call enters: the callee's proven-arguments entry where every Number guard it skips (the cell's
    /// `numbers`) is one the call's proven arguments (`CallRecord::proven_args`) cover, else its row entry; 0 where it has none.
    #[inline(always)]
    fn row_entry(&self, cell: usize) -> usize {
        let proven = word_at(cell, CELL_PROVEN_AT) as usize;
        let covered = word_at(cell, CELL_NUMBERS_AT) & !self.field(offset_of!(CallRecord, proven_args)) == 0;
        if covered && proven != 0 { proven } else { word_at(cell, CELL_ROWS_AT) as usize }
    }

    /// Runs the pushed callee: its row entry (the row ABI) where it has one, else its lane body entry.
    #[inline(always)]
    fn run_body(&self, a: &Admitted, this: u64, captures: u64) -> BodyReturn {
        let rows = self.row_entry(a.cell);
        let ctx = self.places.s.ctx;
        // SAFETY: the cell's entries are the callee's published body, entered with the window `admit` proved.
        unsafe {
            if rows != 0 {
                let entry: RowsEntry = transmute(rows);
                let RowsReturn { value, status } = entry(a.top, ctx, this, captures);
                BodyReturn { status: status as u32, value }
            } else {
                let entry: BodyEntry = transmute(word_at(a.cell, CELL_ENTRY_AT) as usize);
                entry(ctx, this, captures, a.top)
            }
        }
    }

    /// How the call ended, after the staged call (`returned`: whether it completed): a callee that ended with
    /// any other status is routed here. Not entered pops and refuses; an exception pops, is counted in the
    /// callee's cell and raises in the caller; any other status keeps the frames (the lane links them).
    #[inline(always)]
    pub(crate) fn ended(&self, returned: bool) -> Ended {
        if returned {
            return Ended::Returned;
        }
        let (Some(a), Some(body)) = (self.admitted, self.returned) else { return Ended::Refused };
        match LaneStatus::of(body.status) {
            Some(LaneStatus::NotEntered) => {
                Self::pop(&a);
                Ended::Refused
            }
            Some(LaneStatus::Threw) => {
                Self::pop(&a);
                let counters = word_at(a.cell, CELL_COUNTERS_AT);
                StencilStage::from_hole(0).store_u64(Region::EntryCell.mutable(), a.cell as u64, CELL_COUNTERS_AT as i32, counters.wrapping_add(RAISED_RUN));
                Ended::Raised
            }
            _ => Ended::Left(body.status, body.value),
        }
    }
}

impl<const ARGC: usize, const MODE: u8, const CHECKED: bool, const PLACED: bool> Frame<Concrete> for DirectFrame<'_, ARGC, MODE, CHECKED, PLACED> {
    type Fact = Representation;
    type Control = ();

    #[inline(always)]
    fn fact(&self, register: Register) -> Representation {
        self.places.fact(register)
    }
    #[inline(always)]
    fn read(&mut self, stage: &mut Concrete, register: Register) -> u64 {
        self.places.read(stage, register)
    }
    #[inline(always)]
    fn write(&mut self, stage: &mut Concrete, register: Register, value: u64, fact: Representation) {
        self.places.write(stage, register, value, fact)
    }
    #[inline(always)]
    fn this(&mut self, stage: &mut Concrete) -> u64 {
        self.places.this(stage)
    }
    #[inline(always)]
    fn define_this(&mut self, stage: &mut Concrete, destination: Register, this: u64) {
        self.places.define_this(stage, destination, this)
    }
    #[inline(always)]
    fn constant(&mut self, stage: &mut Concrete, constant: Constant) -> (u64, Representation) {
        self.places.constant(stage, constant)
    }
    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        self.places.truthiness(stage, register)
    }
    #[inline(always)]
    fn branch(&mut self, stage: &mut Concrete, condition: bool) {
        self.places.branch(stage, condition)
    }
    #[inline(always)]
    fn jump(&mut self, stage: &mut Concrete) {
        self.places.jump(stage)
    }
    #[inline(always)]
    fn boolean(&mut self, stage: &mut Concrete, condition: bool) -> (u64, Representation) {
        self.places.boolean(stage, condition)
    }
}

impl<const ARGC: usize, const MODE: u8, const CHECKED: bool, const PLACED: bool> crate::staged::PropertyFrame<Concrete>
    for DirectFrame<'_, ARGC, MODE, CHECKED, PLACED> {
    type Plans = crate::staged::NoPlans;
    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }
}

impl<const ARGC: usize, const MODE: u8, const CHECKED: bool, const PLACED: bool> CallFrame<Concrete> for DirectFrame<'_, ARGC, MODE, CHECKED, PLACED> {
    /// The record's plan: a script callee whose identity and global are the record's words (`call_identity`,
    /// `call_global`; the plan's constants stand for them), checked unless a Stable fact holds the callee; an
    /// arrow's `this` bound to the deleted marker, as the lane's frame binds a lexical callee's.
    #[inline(always)]
    fn call_entry(&self, _site: &Invocation) -> CallEntry {
        let identity = if CHECKED { Identity::Guarded(0) } else { Identity::Folded(0) };
        let bound = (MODE == LEXICAL).then_some(Bound { this: Value::DELETED.to_bits(), arguments: WordList { start: 0, count: 0 } });
        CallEntry::Direct(DirectPlan { kind: CalleeKind::Script, identity, sloppy: (MODE == SLOPPY).then_some(0),
            registers: ARGC as u16, returned: LaneStatus::Returned as u32, bound })
    }

    /// The caller's register file: the frame word the row was entered with.
    #[inline(always)]
    fn call_registers(&mut self, stage: &mut Concrete) -> Option<u64> {
        self.places.call_registers(stage)
    }

    /// A strict callee's receiver, bound as it is, enters only as a direct object: the lane's rows and a body
    /// that takes its receiver as one (`dispatch_fast::receiver_trusted`), so any other takes the generic call, whose
    /// binding checks it once. A sloppy callee's binding made it one already; a lexical callee reads none.
    #[inline(always)]
    fn call_this_bound(&mut self, stage: &mut Concrete, this: u64, miss: ()) -> Option<()> {
        if MODE != STRICT { return Some(()); }
        let object = crate::staged::value::is_direct_object(stage, this);
        stage.guard(object, miss)
    }

    #[inline(always)]
    fn call_constants(&self, list: WordList) -> Option<&[u64]> {
        (list.count == 0).then_some(&[][..])
    }

    #[inline(always)]
    fn call_words(&self) -> Option<&[u64]> {
        PLACED.then_some(&self.words[..])
    }

    /// The cell's fill, at least the arguments.
    #[inline(always)]
    fn call_fill(&self) -> Option<u16> {
        let fill = self.admitted?.fill;
        Some(if fill < ARGC { ARGC as u16 } else { fill as u16 })
    }

    #[inline(always)]
    fn call_identity(&mut self, _stage: &mut Concrete) -> Option<u64> {
        Some(self.callee_field(offset_of!(CalleeRecord, callee)))
    }

    #[inline(always)]
    fn call_global(&mut self, _stage: &mut Concrete) -> Option<u64> {
        Some(self.callee_field(offset_of!(CalleeRecord, global_this)))
    }

    /// The cell's validity and room (`admit_native`, less the callee's recovery space) and the plain depth bound.
    #[inline(always)]
    fn call_room(&mut self, stage: &mut Concrete, _miss: ()) -> Option<Room<u64>> {
        let payload = payload(&self.places.s);
        let cell = self.callee_field(offset_of!(CalleeRecord, cell)) as usize;
        let tops = protocol::lane_tops(stage, payload as u64);
        let (top, end) = (word_at(tops.values_at as usize, 0), word_at(payload, JS_STEP_LANE_VALUES_END));
        let recovery = word_at(cell, CELL_RECOVERY_AT) as usize;
        let reserved = word_at(recovery, RECOVERY_VALUES_AT) + FRAME_CONTEXT_WORDS as u64;
        let (room, fill) = admit_native(stage, cell as u64, self.callee_field(offset_of!(CalleeRecord, expected)), top, end, reserved, ())?;
        let depth = word_at(tops.plain_at as usize, PLAIN_DEPTH_AT);
        let frames = word_at(recovery, RECOVERY_ACTIVATIONS_AT) + 1;
        // The payload names the limit's cell (`LaneStack::plain_depth_limit`), which Rust zeroes when it changes the frames
        // under a bound run or the run's window is a leaf's (`refuse_direct_calls_in_leaf`): the helper row calls then.
        // Comparing against the cell's address instead admitted every call, so a body entered as a leaf pushed its callee
        // over the leaf's pending activation (crypto's and deltablue's `--b-jit` failures).
        if depth + frames > word_at(word_at(payload, PLAIN_DEPTH_LIMIT) as usize, 0) {
            return None;
        }
        let registers = word_at(tops.plain_at as usize, PLAIN_REGISTERS_AT);
        let records = word_at(word_at(payload, JS_STEP_ACTIVATION_COUNT) as usize, 0);
        self.admitted = Some(Admitted { cell, top: top as usize, room: room as usize, fill: fill as usize, depth, registers,
            payload, tops, frames: records + depth });
        Some(Room { cell: cell as u64, top, next: top + ((room + FRAME_CONTEXT_WORDS as u64) << WORD_SHIFT), shadow: 0 })
    }

    /// The callee's receiver and captures word below its window, its plain header, its body in the body table and
    /// the lane's new tops; then the body runs (`run_body`).
    #[inline(always)]
    fn call_enter(&mut self, _stage: &mut Concrete, this: u64, _room: Room<u64>) -> Option<Entered<u64, ()>> {
        let a = self.admitted?;
        let file = self.places.s.frame as u64;
        let captures = self.callee_field(offset_of!(CalleeRecord, captures));
        // The receiver only where the callee's activation reads `this`: the cell's word says so, read now, so a
        // republish that changes it is seen by the next call.
        if word_at(a.cell, CELL_USES_THIS_AT) != 0 {
            set_context_word(a.top, CONTEXT_THIS, this);
        }
        set_context_word(a.top, CONTEXT_BODY, captures);
        // The callee's environment and captures pointer, which the interpreter reads from a linked frame.
        set_context_word(a.top, CONTEXT_ENV, self.callee_field(offset_of!(CalleeRecord, env)));
        set_context_word(a.top, CONTEXT_CAPTURE, self.callee_field(offset_of!(CalleeRecord, capture)));
        // The header's packed word is the site's flags and argc with the callee's function.
        let packed = self.field(offset_of!(CallRecord, packed)) | self.callee_field(offset_of!(CalleeRecord, function));
        let header = protocol::BodyHeader { call: self.field(offset_of!(CallRecord, call)), caller_registers: file,
            caller_base: self.field(offset_of!(CallRecord, caller_base)), packed,
            callee: self.callee_field(offset_of!(CalleeRecord, callee)) };
        let mut stage = StencilStage::from_hole(0);
        // Its body and descriptor at the frame's index (the descriptor links the native header where Rust observes
        // it, `native_frames::link_header`), the plain depth and window it bumps: the native frame every tier pushes.
        protocol::push_native_frame(&mut stage, &a.tops, protocol::NativeFrame { window: a.top as u64, header, body: word_at(a.cell, CELL_BODY_AT),
            descriptor: self.callee_field(offset_of!(CalleeRecord, descriptor)), index: a.frames,
            next: (a.top + ((a.room + FRAME_CONTEXT_WORDS) << WORD_SHIFT)) as u64 },
            // The tables load here, past the window's fill, so their addresses are not live across it.
            #[inline(always)] |_| protocol::NativePlaces { bodies: word_at(a.payload, JS_STEP_BODY_TABLE), descriptors: word_at(a.payload, JS_STEP_NATIVE_TABLE),
                plain_at: a.tops.plain_at, depth: a.depth });
        let body = self.run_body(&a, this, captures);
        self.returned = Some(body);
        Some(Entered { status: u64::from(body.status), word: body.value, unfinished: () })
    }

    /// The callee completed with `value`: its window and frame pop, and the word goes to the call's register.
    #[inline(always)]
    fn call_settle(&mut self, stage: &mut Concrete, site: &Invocation, _entered: Entered<u64, ()>, _room: Room<u64>, value: u64) -> Option<()> {
        Self::pop(&self.admitted?);
        stage.store_u64(Region::Frame.mutable(), self.places.s.frame as u64, word_offset(site.result), value);
        Some(())
    }
}
