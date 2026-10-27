//! The fast lane runs straight-line instructions whose whole effect is a
//! register write, a Wasm linear-memory access or a jump. No frame state is
//! published. The lane stops before any instruction that would collect, trap,
//! throw, call or touch a shared memory, and the general path executes that
//! instruction with its full semantics, so lane execution is always an exact
//! prefix of general execution.
//!
//! The lane runs a function's *lane view*: one [`LaneInstruction`] per
//! bytecode instruction, at the same index, derived once from the bytecode.
//! A record names its opcode's handler and carries the instruction's decoded
//! fields, so handlers neither decode packed fields nor look up a table.
//! Each handler ends by dispatching the next record's handler itself
//! (threaded dispatch), so each opcode predicts its successor at its own
//! branch. Where the build forms sibling calls (`quench_lane_tail_calls`), that
//! dispatch is a jump and a run of handlers shares one stack frame; otherwise
//! every handler returns to the lane loop. A taken safepoint jump compares the
//! stack pointer with the run's stack floor: should a build not form a sibling
//! call, the stack reaches the floor and the jump returns to the lane loop,
//! which unwinds it; a due collection raises the floor so that the next
//! safepoint jump leaves the lane.

use super::dispatch_frame::PendingGeneralCall;
use crate::heap::InlineObjectHeader;
use crate::value_vec::INLINE_PROPERTY_COUNT;
use super::*;
use crate::bytecode::{
    FieldLayout, Function, ImmediateLayout, ImmediateRole, InstructionField, WideInstruction,
};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{
    F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator,
};
use crate::wasm::integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator,
};
use crate::wasm::memory::{MemoryLoad, MemoryStorage, MemoryStore};

/// One instruction of a lane view.
#[repr(C)]
pub(super) struct LaneInstruction {
    /// The opcode's [`Handler`] for the VM that built the view.
    handler: *const (),
    operand: LaneOperand,
    a: u16,
    b: u16,
    c: u16,
    /// A call record's hint of the script call target cache entry that answered it last
    /// (`Vm::linked_script_call_entry`), which the record's next call tests first; `NO_CALL_LINK`
    /// for none. It fills the record's padding, so every record keeps its size.
    call_link: std::cell::Cell<u16>,
}

/// A `LaneInstruction::call_link` that names no entry: no cache holds this many.
const NO_CALL_LINK: u16 = u16::MAX;
const _: () = assert!(std::mem::size_of::<LaneInstruction>() == 3 * std::mem::size_of::<u64>());

/// The immediate, resolved by its role: a jump target becomes the byte
/// distance from the record, a constant index becomes the constant.
#[derive(Clone, Copy)]
union LaneOperand {
    imm: u32,
    /// A `GetField` record's immediate and the receiver shape of its own-field hint
    /// (`LaneInstruction::field_hint`).
    field: FieldOperand,
    jump: isize,
    constant: Value,
    /// A loop's backward jump in compiled execution: its site (`LoopSite`), which holds the
    /// jump's distance.
    site: *const LoopSite,
}

/// A `GetField` record's operand: its immediate, and the exact receiver shape its own-field
/// hint names, `NO_FIELD_RECEIVER` for none. The hint's slot is the record's `call_link`,
/// which no field record reads otherwise (`holds_no_call_link`).
#[derive(Clone, Copy)]
#[repr(C)]
struct FieldOperand {
    imm: u32,
    shape: u32,
}

impl LaneInstruction {
    /// The own-field hint of a `GetField` record: an exact receiver shape and the slot of its
    /// own data field, as the site's primary entry proved it (`lane_get_field_own_refresh`).
    /// Between shape compactions, which clear every hint (`LaneView::clear_field_hints`), a
    /// shape's slots never change, so the pair holds whatever the entry holds now.
    #[inline(always)]
    fn field_hint(&self) -> super::FieldCache {
        // SAFETY: the view writes `field` for every `GetField` record, the only records whose
        // handlers read the hint.
        super::FieldCache::own(unsafe { self.operand.field.shape }, self.call_link.get())
    }

    #[inline(always)]
    fn imm(&self) -> u32 {
        // SAFETY: the view writes `imm` for every role other than a jump
        // target or constant index, and only those handlers read it.
        unsafe { self.operand.imm }
    }

    #[inline(always)]
    fn jump(&self) -> isize {
        // SAFETY: the view writes `jump` for every jump-target immediate.
        unsafe { self.operand.jump }
    }

    #[inline(always)]
    fn constant(&self) -> Value {
        // SAFETY: the view writes `constant` for every constant index.
        unsafe { self.operand.constant }
    }

    #[inline(always)]
    fn loop_site(&self) -> &LoopSite {
        // SAFETY: `install_loop_sites` writes `site` for the records it gives
        // `lane_loop_jump`, the only handler that reads it; the sites live in the view.
        unsafe { &*self.operand.site }
    }
}

/// A loop the shared compiler may run natively: the region its backward jump
/// closes, the jump's distance to the header record, and the tier's state for it.
pub(super) struct LoopSite {
    /// The bytecode function whose lane view owns this site. A lane activation may be
    /// frameless, so the VM frame's function is not necessarily the one taking this edge.
    function: u32,
    /// Observe backedge work for the JS function tier when OSR rejected this loop. These
    /// sites never attempt native loop code; the shared function compiler owns promotion.
    promotion_only: bool,
    /// First analysis failure, when admission did not produce a region.
    analysis_failure: Option<super::compiled::RegionAnalysisFailure>,
    /// Static effectful calls that leave native code at their exact PC.
    call_boundary_pcs: Option<Box<[u32]>>,
    /// The admitted region; a loop whose calls run inlined makes its region once it is hot
    /// (`lane_inlining_region`), when its callees are known.
    region: std::cell::OnceCell<super::compiled::LoopRegion>,
    /// The loop's header and backward jump PCs.
    header: u32,
    backedge: u32,
    jump: isize,
    /// The header record's own handler while the site holds that record
    /// (`lane_loop_header`), which enters the loop's native code wherever control arrives at
    /// the header; null otherwise.
    header_handler: std::cell::Cell<*const ()>,
    /// Whether the site may hold its header record (`lane_loop_hold_header`): one that is
    /// no call record, whose `call_link` names a call target, and the header of no other
    /// site. The record's `call_link` then names the site.
    hookable: bool,
    /// Backward jumps taken before the region compiles, up to `LOOP_TIER_THRESHOLD`.
    taken: std::cell::Cell<u32>,
    /// Native runs that left at an instruction the general path executes.
    exits: std::cell::Cell<u32>,
    /// Entries declined since the last that ran native code.
    declines: std::cell::Cell<u32>,
    /// Whether this VM requested OSR diagnostics when its lane view was derived.
    diagnostics: bool,
    lane_backedge_count: std::cell::Cell<u64>,
    native_entry_attempts: std::cell::Cell<u64>,
    native_region_runs: std::cell::Cell<u64>,
    entry_guard_misses: std::cell::Cell<u64>,
    native_exits: std::cell::Cell<u64>,
    widenings: std::cell::Cell<u64>,
    compile_attempts: std::cell::Cell<u64>,
    compile_successes: std::cell::Cell<u64>,
    tier: std::cell::RefCell<LoopTier>,
}

impl LoopSite {
    fn is_admitted(&self) -> bool {
        !self.promotion_only
            && self.analysis_failure.is_none_or(|failure| {
                failure.reason == super::compiled::RegionRejection::UnsupportedInstruction(Op::Call)
            })
    }
}

/// A loop site's compilations: the current code, if any, and the values earlier code found
/// were no `i32`, which the next compilation holds as `f64`s.
#[derive(Default)]
struct LoopTier {
    code: Option<super::compiled::CompiledOsrRegion>,
    widened: std::collections::BTreeSet<super::compiled::Widening>,
    compilations: u32,
    /// A raw field load embeds a shape ID, which collection may remap. Such code is only
    /// valid under the epoch in which its field descriptors were observed.
    native_field_epoch: Option<u64>,
}

impl LoopTier {
    /// Whether the site has native code or may still compile it. A site whose compilations
    /// ran out, or under a policy that compiles no loop regions, takes its backward jump in
    /// the interpreter without reading an entry: reading one at every iteration had cost
    /// V8-v7 Crypto `--jit` a third of its instructions under memory-first.
    fn may_run(&self, policy: crate::MemoryPolicy) -> bool {
        self.code.is_some()
            || (self.compilations < LOOP_TIER_MAX_COMPILATIONS && super::compiled::loop_tier_compiles(policy))
    }
}

/// Backward jumps a loop takes in the interpreter before it compiles: enough that loops
/// which run briefly never pay the compilation.
pub(super) const LOOP_TIER_THRESHOLD: u32 = 1000;
/// Native runs that may leave at a general-path instruction before the site stops entering
/// native code: a loop whose values keep leaving the subset is the interpreter's.
const LOOP_TIER_MAX_EXITS: u32 = 1000;
/// Entries a site's guards may decline in a row before it stops trying: a field whose cache
/// never hits at entry (an array's `length`) made V8-v7 Crypto's sum loop decline at each of
/// its iterations.
const LOOP_TIER_MAX_DECLINES: u32 = 64;
/// Elements of an array read to guess whether it holds `i32`s; a wrong guess costs one
/// compilation.
const LOOP_TIER_ELEMENT_SAMPLE: usize = 8;
/// Compilations a site may make: each after the first widens a value the last one held as
/// an `i32`, and a region has few such values.
const LOOP_TIER_MAX_COMPILATIONS: u32 = 8;

type Ip = *const LaneInstruction;

/// A function's lane view: one record per instruction, and the one register
/// its direct memory accesses go through. Accesses through any other
/// register, and lane instructions other than the binding load that would
/// write this one, take the general path, so the memory view tracks the
/// register exactly.
pub(super) struct LaneView {
    /// A record's handler may change while the lane runs (`requicken`); its other fields do not.
    records: Box<[std::cell::UnsafeCell<LaneInstruction>]>,
    /// Per instruction, whether the general loop enters the lane there: the
    /// straight-line lane run from it is long enough to repay the lane's
    /// entry cost, and its record does not read the accumulator. Kept apart
    /// from the records, which only the lane reads.
    entries: Box<[bool]>,
    memory_register: Option<u16>,
    /// How a lane call activates the function without a frame; `None`
    /// when its activation needs a frame (locals outside registers).
    activation: Option<ActivationShape>,
    /// The facts of its body a frameless call target records (`TargetBody`), found on its
    /// first resolution: every closure of the function shares them.
    target_body: std::cell::OnceCell<TargetBody>,
    /// The window of an environment activation, when its local and closure records also run
    /// in one (`specialize_environment_window`).
    environment_window: Option<EnvironmentWindow>,
    /// The loops whose backward jumps run `lane_loop_jump`. The view owns them for the
    /// records that point into them, which are their only readers.
    loops: Box<[LoopSite]>,
    /// The record an activation starts at (`LaneView::start`): its first, or `prologue`
    /// while the JavaScript tier may claim the function (`LaneView::claim_for_tier`).
    start: std::cell::Cell<Ip>,
    /// The tier prologue (`lane_tier_prologue`), outside the records so the function's PCs
    /// stay its records'; written when the tier may claim the function.
    prologue: std::cell::UnsafeCell<LaneInstruction>,
    /// The prologue's direct run of the compiled body, once the tier compiled it
    /// (`Vm::direct_tier_entry`).
    tier_direct: std::cell::Cell<Option<super::compiled::DirectTierEntry>>,
    plain_backward_jump_sites: usize,
    osr_rejections: Option<Box<[(super::compiled::RegionRejection, usize)]>>,
    /// The `GetField` records past the loop headers its sites hold, whose own-field hints a
    /// shape compaction clears.
    field_records: Box<[u32]>,
}

#[derive(Debug, Default)]
pub(super) struct OsrDiagnostics {
    pub(super) plain_backward_jump_sites: usize,
    pub(super) admitted_sites: usize,
    pub(super) promotion_only_sites: usize,
    pub(super) compile_attempts: u64,
    pub(super) compile_successes: u64,
    pub(super) currently_compiled_sites: usize,
    pub(super) native_entry_attempts: u64,
    pub(super) native_region_runs: u64,
    pub(super) entry_guard_misses: u64,
    pub(super) exits: u64,
    pub(super) widenings: u64,
    pub(super) analysis_rejections: Vec<(super::compiled::RegionRejection, usize)>,
    pub(super) sites: Vec<OsrSiteDiagnostics>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OsrSiteStatus {
    Admitted,
    CallBoundary,
    PromotionOnly,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OsrSiteDiagnostics {
    pub(super) header_pc: u32,
    pub(super) backedge_pc: u32,
    pub(super) status: OsrSiteStatus,
    pub(super) failure: Option<super::compiled::RegionAnalysisFailure>,
    pub(super) admitted: bool,
    pub(super) deferred_call: bool,
    pub(super) tier_threshold_reached: bool,
    pub(super) lane_backedge_count: u64,
    pub(super) analysis_rejection: Option<super::compiled::RegionRejection>,
    pub(super) call_boundary_pcs: Vec<u32>,
    pub(super) compile_attempts: u64,
    pub(super) compile_successes: u64,
    pub(super) native_entry_attempts: u64,
    pub(super) native_region_runs: u64,
    pub(super) entry_guard_misses: u64,
    pub(super) exits: u64,
    pub(super) widenings: u64,
}

impl LaneView {
    /// OSR tier coverage and outcomes, for opt-in JIT diagnostics.
    pub(super) fn osr_diagnostics(&self) -> OsrDiagnostics {
        let mut sites = self.loops.iter().map(|site| OsrSiteDiagnostics {
            header_pc: site.header,
            backedge_pc: site.backedge,
            status: if site.promotion_only {
                OsrSiteStatus::PromotionOnly
            } else if site.analysis_failure.is_some_and(|failure| {
                failure.reason == super::compiled::RegionRejection::UnsupportedInstruction(Op::Call)
            }) {
                OsrSiteStatus::CallBoundary
            } else if site.analysis_failure.is_some() {
                OsrSiteStatus::Rejected
            } else {
                OsrSiteStatus::Admitted
            },
            failure: site.analysis_failure,
            call_boundary_pcs: site.call_boundary_pcs.as_deref().unwrap_or_default().to_vec(),
            admitted: site.is_admitted(),
            deferred_call: site.call_boundary_pcs.as_deref().is_some_and(|pcs| !pcs.is_empty())
                || site.analysis_failure.is_some_and(|failure| {
                    failure.reason == super::compiled::RegionRejection::UnsupportedInstruction(Op::Call)
                }),
            tier_threshold_reached: site.taken.get() >= LOOP_TIER_THRESHOLD,
            lane_backedge_count: site.lane_backedge_count.get(),
            analysis_rejection: site.analysis_failure.map(|failure| failure.reason),
            compile_attempts: u64::from(site.compile_attempts.get()),
            compile_successes: u64::from(site.compile_successes.get()),
            native_entry_attempts: site.native_entry_attempts.get(),
            native_region_runs: site.native_region_runs.get(),
            entry_guard_misses: site.entry_guard_misses.get(),
            exits: u64::from(site.native_exits.get()),
            widenings: u64::from(site.widenings.get()),
        }).collect::<Vec<_>>();
        sites.sort_unstable_by_key(|site| (site.header_pc, site.backedge_pc));
        OsrDiagnostics {
            plain_backward_jump_sites: self.plain_backward_jump_sites,
            admitted_sites: self.loops.iter().filter(|site| site.is_admitted()).count(),
            promotion_only_sites: self.loops.iter().filter(|site| site.promotion_only).count(),
            compile_attempts: self.loops.iter().map(|site| site.compile_attempts.get()).sum(),
            compile_successes: self.loops.iter().map(|site| site.compile_successes.get()).sum(),
            currently_compiled_sites: self
                .loops
                .iter()
                .filter(|site| site.tier.borrow().code.is_some())
                .count(),
            native_entry_attempts: self
                .loops
                .iter()
                .map(|site| site.native_entry_attempts.get())
                .sum(),
            native_region_runs: self
                .loops
                .iter()
                .map(|site| site.native_region_runs.get())
                .sum(),
            entry_guard_misses: self
                .loops
                .iter()
                .map(|site| site.entry_guard_misses.get())
                .sum(),
            exits: self.loops.iter().map(|site| site.native_exits.get()).sum(),
            widenings: self.loops.iter().map(|site| site.widenings.get()).sum(),
            analysis_rejections: self
                .osr_rejections
                .as_deref()
                .unwrap_or_default()
                .to_vec(),
            sites,
        }
    }
}

/// An environment activation's window past its registers (`lane_activate_environment`):
/// its environment, a pointer to the environment's slot storage (`WINDOW_SLOTS`), then its
/// `locals`; bit `k` of `captured` is set when local `k` lives in the environment
/// (`Function::local_slot_uses_environment`).
#[derive(Clone, Copy)]
struct EnvironmentWindow {
    locals: u16,
    captured: u64,
}

/// An environment window's entries after its registers: its environment, `DELETED` until
/// one is made (`window_environment`); the address of the environment's slot storage, which
/// never moves while it lives, held in a `Value`'s bits; the `EnvironmentWindow::captured`
/// mask, likewise; the words of the slot storage the window holds in place
/// (`EnvironmentSlots::lay_out_in_place`) over its locals, which follow. Until a closure or a
/// frame needs the environment, every slot lives among the locals and the storage address
/// names that in-place storage, so a captured slot reads the same way before and after.
const WINDOW_ENVIRONMENT: u16 = 0;
const WINDOW_SLOTS: u16 = 1;
const WINDOW_CAPTURED: u16 = 2;
const WINDOW_STORAGE: u16 = 3;
const WINDOW_LOCALS: u16 = WINDOW_STORAGE + crate::heap::IN_PLACE_SLOTS_WORDS as u16;

impl EnvironmentWindow {
    fn captured(self, slot: usize) -> bool {
        self.captured & 1 << slot != 0
    }
}

/// What a function's body fixes about a frameless call target of it
/// (`Vm::frameless_call_target`): its entry apart from a kept `arguments`, and whether `new`
/// may activate it or run it as its stores alone.
#[derive(Clone, Copy)]
struct TargetBody {
    entry: CallEntry,
    constructs: bool,
    initializes: bool,
}

/// Registers a lane call writes as one fixed block: a window fills whole blocks, whose slots
/// past the window lie in spare stack capacity, so no store depends on the window's size.
const ACTIVATION_FILL_BLOCK: usize = 8;

/// `ACTIVATION_FILL_BLOCK` for a JavaScript activation, whose bodies keep more values in
/// registers: on V8-v7, most called functions have 9 to 16.
const SCRIPT_FILL_BLOCK: usize = 2 * ACTIVATION_FILL_BLOCK;

/// The lane-stack room `fill_window::<BLOCK>` writes for a window of `count` registers: whole
/// blocks, at least one.
const fn fill_room<const BLOCK: usize>(count: usize) -> usize {
    let room = count.next_multiple_of(BLOCK);
    if room < BLOCK { BLOCK } else { room }
}

/// The values of a call window to copy as one fixed-size move each; a longer window copies
/// value by value.
const FIXED_ARGUMENT_COPIES: u16 = 4;

/// Copies the `count` values at `source` to `destination`: a window of at most
/// `FIXED_ARGUMENT_COPIES` values as one move of its size, which needs no loop.
///
/// # Safety
/// Both ranges hold `count` values and do not overlap.
#[inline(always)]
unsafe fn copy_arguments(destination: *mut Value, source: *const Value, count: u16) {
    /// One move of `N` values.
    #[inline(always)]
    unsafe fn copy<const N: usize>(destination: *mut Value, source: *const Value) {
        // SAFETY: the caller's contract, for `N` values.
        unsafe { destination.cast::<[Value; N]>().write_unaligned(source.cast::<[Value; N]>().read_unaligned()) }
    }
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    // SAFETY: the function's contract.
    unsafe {
        // Compares, not a table, so the common windows of a value or two take a branch each.
        if count <= 2 {
            if count == 2 {
                copy::<2>(destination, source);
            } else if count == 1 {
                copy::<1>(destination, source);
            }
        } else if count <= FIXED_ARGUMENT_COPIES {
            if count == FIXED_ARGUMENT_COPIES {
                copy::<4>(destination, source);
            } else {
                copy::<3>(destination, source);
            }
        } else {
            std::ptr::copy_nonoverlapping(source, destination, usize::from(count));
        }
    }
}

/// Writes `initial` to the `count` registers at `registers`, as whole blocks of `BLOCK`: the
/// first unconditionally, since most windows fit it.
///
/// # Safety
/// `registers` has room for `fill_room::<BLOCK>(count)` values, none of them live.
#[inline(always)]
unsafe fn fill_window<const BLOCK: usize>(registers: *mut Value, count: usize, initial: Value) {
    // SAFETY: the room holds at least one block.
    unsafe { registers.cast::<[Value; BLOCK]>().write_unaligned([initial; BLOCK]) };
    for block in 1..count.div_ceil(BLOCK) {
        // SAFETY: the block ends within the room the caller provides.
        unsafe {
            registers
                .add(block * BLOCK)
                .cast::<[Value; BLOCK]>()
                .write_unaligned([initial; BLOCK]);
        }
    }
}

/// A function's register window as a lane activation starts it: every
/// register its initial value, then the parameters at their base.
#[derive(Clone, Copy)]
struct ActivationShape {
    registers: u16,
    parameters: u16,
    params: u16,
    initial: Value,
}

impl LaneView {
    /// Forgets every own-field hint of its records (`LaneInstruction::field_hint`): a shape
    /// compaction renumbers the shapes they name.
    pub(super) fn clear_field_hints(&self) {
        for &pc in &self.field_records {
            let record = std::cell::UnsafeCell::raw_get(&self.records[pc as usize]);
            // SAFETY: no handler runs during a collection, and each listed record is a
            // `GetField` one, whose operand is `field`, and no loop's header.
            unsafe {
                (*record).operand.field.shape = super::NO_FIELD_RECEIVER;
                (*record).call_link.set(NO_CALL_LINK);
            }
        }
    }

    /// How many of its records hold an own-field hint. Profiled builds derive no lane views,
    /// so only the unprofiled test reads it.
    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn held_field_hints(&self) -> usize {
        self.field_records
            .iter()
            // SAFETY: each listed record is a `GetField` one, whose operand is `field`.
            .filter(|&&pc| unsafe { (*self.records[pc as usize].get()).operand.field.shape } != super::NO_FIELD_RECEIVER)
            .count()
    }

    /// The address range its records span (`lane_exits::register_view`, `lane_header_site`).
    pub(super) fn record_range(&self) -> (usize, usize) {
        let start = self.records().addr();
        (start, start + self.records.len() * std::mem::size_of::<LaneInstruction>())
    }

    pub(super) fn records(&self) -> Ip {
        std::cell::UnsafeCell::raw_get(self.records.as_ptr()).cast_const()
    }

    /// The record a frameless activation starts at: the first, or the tier prologue.
    #[inline(always)]
    fn start(&self) -> Ip {
        self.start.get()
    }

    /// Starts the function's frameless activations at the tier prologue, which enters the
    /// JavaScript tier with its `params` parameters and, where it `binds_this`, its receiver
    /// (`lane_tier_prologue`). The prologue's fields hold those: `imm` the function, `a` its
    /// parameter count, `b` whether it binds its own `this`.
    fn claim_for_tier(&self, handler: Handler, function: u32, params: u16, binds_this: bool) {
        let prologue = self.prologue.get();
        // SAFETY: the lane runs on the view's thread, and nothing reads the prologue while
        // the start does not name it.
        unsafe {
            *prologue = LaneInstruction {
                handler: handler as *const (),
                operand: LaneOperand { imm: function },
                a: params,
                b: u16::from(binds_this),
                c: 0,
                call_link: std::cell::Cell::new(NO_CALL_LINK),
            };
        }
        self.start.set(prologue.cast_const());
    }

    /// Starts activations at the first record again, once the tier declines the function.
    fn release_from_tier(&self) {
        self.start.set(self.records());
    }
}

/// Switches the record at `ip` to `handler` for the rest of its view's life: a one-way step of
/// the record's state machine, taken when an execution of the record shows the handler it has
/// does not fit the site. Every handler a record may switch between computes the same result.
#[inline(always)]
fn requicken<H: Host>(ip: Ip, handler: Handler) {
    // SAFETY: `ip` comes from `LaneView::records`, whose records live in `UnsafeCell`s; the
    // lane runs on the view's thread, and no reference to the handler field is live. A
    // header record a loop site holds keeps `lane_loop_header`, and the site the handler.
    unsafe {
        if (*ip).handler == lane_loop_header::<H> as Handler as *const ()
            && let Some(site) = lane_header_site::<H>(lane_context::<H>(), ip)
        {
            (*site).header_handler.set(handler as *const ());
            return;
        }
        (*ip.cast_mut()).handler = handler as *const ();
    }
}

/// The active frame's registers. Lane instructions never resize registers or
/// change the frame stack, so the base stays valid for the whole lane run.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Registers(*mut Value);

impl Registers {
    #[inline(always)]
    fn get(self, register: u16) -> Value {
        // SAFETY: the view admits only register fields below
        // Function.registers, and frame setup sizes the register file to it.
        unsafe { *self.0.add(usize::from(register)) }
    }

    #[inline(always)]
    fn set(self, register: u16, value: Value) {
        // SAFETY: as for `get`; no reference into the register file is live.
        unsafe { *self.0.add(usize::from(register)) = value }
    }

    #[inline(always)]
    fn i32(self, register: u16) -> i32 {
        self.get(register).wasm_bits32() as i32
    }
}

/// The unshared memory bytes behind the active function's memory register.
/// The view is resolved whenever that register can change under the lane:
/// at lane entry, when a lane call or return switches functions, and when
/// the binding instruction loads the register; no other lane instruction
/// writes it (see `LaneView`). The lane never collects, calls out or grows
/// memory, so the bytes stay alive, unborrowed and unresized at a stable
/// address. A memory the lane cannot borrow resolves to the empty view, on
/// which every access leaves the lane.
/// A carried value a record may leave unset: the accumulator after a record that produces
/// none, which only the record a producer falls through to reads, and a JavaScript view's
/// memory reach, which no JavaScript record reads. An unset slot costs its dispatch no
/// instruction; a reader gets one only where the view proves it was set.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Slot<T: Copy>(std::mem::MaybeUninit<T>);

impl<T: Copy> Slot<T> {
    #[inline(always)]
    const fn of(value: T) -> Self {
        Self(std::mem::MaybeUninit::new(value))
    }

    #[inline(always)]
    const fn unset() -> Self {
        Self(std::mem::MaybeUninit::uninit())
    }

    /// SAFETY: the slot was set (`Slot::of`) on the way here.
    #[inline(always)]
    unsafe fn get(self) -> T {
        unsafe { self.0.assume_init() }
    }
}

#[derive(Clone, Copy)]
struct MemoryView {
    bytes: *mut u8,
    /// The memory's length less the widest direct access: an access
    /// starting at or below it is in bounds whatever its width, so the hot
    /// path compares the address alone. Negative when the memory is
    /// narrower than that access. Unset in a JavaScript view (`MemoryView::script`).
    reach: Slot<isize>,
}

/// The widest access a lane record makes directly.
const WIDEST_DIRECT_ACCESS: usize = size_of::<u64>();

impl MemoryView {
    const NONE: Self = Self::new(std::ptr::null_mut(), 0);

    /// A JavaScript run's view. JavaScript binds no memory (`frame_memory_view`), so the
    /// register a Wasm view's bytes take carries the run's context instead, which
    /// `script_handler!` handlers read without the thread-local load. Its reach is unset: no
    /// JavaScript record accesses memory.
    #[inline(always)]
    fn script<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self {
            bytes: cx.cast(),
            reach: Slot::unset(),
        }
    }

    const fn new(bytes: *mut u8, len: usize) -> Self {
        Self {
            bytes,
            reach: Slot::of(len as isize - WIDEST_DIRECT_ACCESS as isize),
        }
    }

    #[inline(always)]
    fn len(self) -> usize {
        // SAFETY: only a Wasm view's memory is measured, whose reach is set.
        (unsafe { self.reach.get() } + WIDEST_DIRECT_ACCESS as isize) as usize
    }

    /// Direct load `load` at `address`, when the access lies within the
    /// reach; `None` leaves it to the general path, which also decides the
    /// last bytes before the memory's end exactly.
    ///
    /// SAFETY: the view must have been resolved during the current lane run.
    #[inline(always)]
    unsafe fn read(self, load: MemoryLoad, address: u64) -> Option<crate::WasmValue> {
        // Addresses are at most 33 bits, so the signed comparison is exact.
        if (address as i64) > unsafe { self.reach.get() } as i64 || load.width() > WIDEST_DIRECT_ACCESS {
            // Off the hot path: the access falls through to its dispatch.
            std::hint::cold_path();
            return None;
        }
        // SAFETY: the access ends at or before the memory's end.
        Some(unsafe { load.read_at(self.bytes.add(address as usize)) })
    }

    /// Direct store `store` of `value` at `address`, as for `read`.
    ///
    /// SAFETY: the view must have been resolved during the current lane run.
    #[inline(always)]
    unsafe fn write(self, store: MemoryStore, address: u64, value: crate::WasmValue) -> Option<()> {
        // Addresses are at most 33 bits, so the signed comparison is exact.
        if (address as i64) > unsafe { self.reach.get() } as i64 || store.width() > WIDEST_DIRECT_ACCESS {
            std::hint::cold_path();
            return None;
        }
        // SAFETY: the access ends at or before the memory's end.
        unsafe { store.write_at(self.bytes.add(address as usize), value) };
        Some(())
    }
}

/// The state handlers carry from one to the next in machine registers.
#[derive(Clone, Copy)]
struct Carried {
    memory: MemoryView,
    integers: crate::value::IntegerEncoding,
    /// The value an accumulator producer just wrote to its result register.
    /// A record reads it only where the view proved it equals that register; unset after
    /// any other record.
    acc: Slot<Value>,
}

impl Carried {
    /// What a JavaScript record that produces no accumulator carries on: the run's view
    /// (`MemoryView::script`), tagged integers, and no accumulator, which only the record
    /// after a producer reads. Passing this instead of the incoming state frees the
    /// registers the rest of that state would occupy across the record.
    #[inline(always)]
    fn script<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self {
            memory: MemoryView::script(cx),
            integers: crate::value::IntegerEncoding::TAG,
            acc: Slot::unset(),
        }
    }
}

/// An activation a lane call made without a VM frame. Its registers are
/// a window of the lane stack; it returns to the record after `call` in its
/// caller, whose registers, view and memory it keeps for the return.
struct Activation {
    function: u32,
    call: Ip,
    caller_base: Ip,
    caller_registers: Registers,
    /// The lane-stack index of this activation's first register.
    window: usize,
    /// Requested Wasm register payload held by this activation, or zero for scripts.
    frame_storage_bytes: usize,
    /// The program its caller runs, which its return makes the lane's again
    /// (`lane_enter_program`); its own is the next activation's caller program, or the
    /// lane's for the innermost.
    caller_program: ProgramId,
    kind: ActivationKind,
}

/// What an activation keeps beyond its registers, by the kind of function it runs.
enum ActivationKind {
    Script(ScriptActivation),
    /// A Wasm activation keeps its caller's memory for the return.
    Wasm(MemoryView),
}

impl Activation {
    /// The frame state of a JavaScript activation; `None` for Wasm.
    fn script(&self) -> Option<&ScriptActivation> {
        match &self.kind {
            ActivationKind::Script(script) => Some(script),
            ActivationKind::Wasm(_) => None,
        }
    }

    /// The memory view its caller runs with: a JavaScript activation's caller is JavaScript,
    /// whose view carries the context (`MemoryView::script`).
    fn caller_memory<H: Host>(&self, cx: *mut LaneContext<H>) -> MemoryView {
        match self.kind {
            ActivationKind::Script(_) => MemoryView::script(cx),
            ActivationKind::Wasm(memory) => memory,
        }
    }
}

/// What a JavaScript activation's frame would hold beyond its registers
/// (`Vm::activate_plain_frame`), and the caller's lane state its return restores. Its own
/// receiver and environment live in the context while it runs (`LaneContext::this`, `env`)
/// and in the next script activation's caller state beneath that (`ActivationState::of`):
/// one fact, held once.
struct ScriptActivation {
    callee: Value,
    /// Arguments passed, all in parameter registers.
    argc: u16,
    caller_this: Value,
    caller_env: Value,
    caller_capture: *const crate::heap::EnvironmentSlots,
    /// What its return yields to the call.
    completion: Completion,
    /// The `argc` values passed follow the registers in the window, for a forwarded
    /// `arguments` (`ScriptCallTarget::forwards`), for the construction a skipped
    /// forwarding constructor passed on (`Completion::Forwarder`), or for values passed past
    /// the parameters.
    forwards: bool,
    /// Its environment and frame locals follow its registers (`CallEntry::Environment`).
    environment: bool,
}

/// A JavaScript activation's own receiver and environment, which it does not store
/// (`ScriptActivation`).
#[derive(Clone, Copy)]
struct ActivationState {
    this: Value,
    env: Value,
    /// The program it runs.
    program: ProgramId,
}

impl ActivationState {
    /// Each activation's state, innermost from the context, each other's from the caller
    /// state the next script activation above it saved. A Wasm activation changes neither
    /// receiver nor environment; every activation names its caller's program.
    fn of<H: Host>(activations: &[Activation], cx: *mut LaneContext<H>) -> Vec<Self> {
        let mut current = unsafe { Self { this: (*cx).this, env: (*cx).env, program: (*cx).program } };
        let mut states = vec![current; activations.len()];
        for (state, activation) in states.iter_mut().zip(activations).rev() {
            *state = current;
            if let Some(script) = activation.script() {
                current.this = script.caller_this;
                current.env = script.caller_env;
            }
            current.program = activation.caller_program;
        }
        states
    }
}

/// What a JavaScript activation's return yields to its call: one byte, so a plain return
/// tests it once.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Completion {
    /// The returned value.
    Plain,
    /// The returned value, from an activation of another program than its caller's, whose
    /// return makes the caller's program the lane's again (`lane_enter_program`).
    Cross,
    /// A `new` made it: `this` unless it returns an object.
    Constructs,
    /// A `new` of another program's constructor: `Constructs`, whose return makes the
    /// caller's program the lane's again as `Cross` does.
    CrossConstructs,
    /// It runs the method a forwarding constructor's body would have called
    /// (`ScriptCallTarget::forwarded_method`), whose frame `LaneStack::forwarders` keeps the
    /// last entry of: the construction yields `this`, the receiver, whatever it returns.
    Forwarder,
    /// A native's call of JavaScript run frameless from the lane that called the native
    /// (`Vm::lane_native_callback`): its return ends that run with the value, which the
    /// native receives. Made a frame, it is the entry frame of the general loop that finishes
    /// it, which no pending call names.
    ReturnsToNative,
    /// A plain call of a non-suspending async function of the caller's program
    /// (`CallEntry::Settles`): its return resolves the promise the call made, which
    /// `LaneStack::settling` keeps, and the call yields that promise. Made a frame, its
    /// pending call settles it (`PendingGeneralCall::settles`).
    SettlesPromise,
}

impl Completion {
    /// The completion of an activation a `new` made or not, of the caller's program or not,
    /// of a target whose return settles its call's promise or not. Only a plain call of the
    /// caller's program settles one (`lane_script_call_settles`).
    fn of(constructs: bool, cross: bool, settles: bool) -> Self {
        debug_assert!(!settles || !constructs && !cross, "only a plain call of its own program settles");
        match (constructs, cross, settles) {
            (false, false, true) => Self::SettlesPromise,
            (false, false, false) => Self::Plain,
            (false, true, _) => Self::Cross,
            (true, false, _) => Self::Constructs,
            (true, true, _) => Self::CrossConstructs,
        }
    }

    /// Whether a `new` made the activation, whose completion substitutes its receiver for a
    /// non-object result.
    fn constructs(self) -> bool {
        matches!(self, Self::Constructs | Self::CrossConstructs)
    }

    /// Whether the activation's return makes the caller's program the lane's again.
    fn crosses(self) -> bool {
        matches!(self, Self::Cross | Self::CrossConstructs)
    }
}

/// A forwarding constructor whose activation a construction skipped
/// (`lane_forwarded_construction`): what its frame holds when activations become frames,
/// pending at its forwarding call.
#[derive(Clone, Copy)]
struct SkippedForwarder {
    function: u32,
    callee: Value,
    env: Value,
    method: crate::bytecode::ForwardedThisMethod,
}

/// Where a lane activation's arguments come from: a window of the caller's registers, or
/// values a forwarding activation or frame kept (`f.apply(this, arguments)`).
#[derive(Clone, Copy)]
enum ActivationArguments {
    Window(u16),
    Kept(*const Value, usize),
}

/// A user function a lane `Call` may activate without a frame, valid as the general loop's
/// `InLoopCallTarget` is: for `callee` while the collection epoch and realm it was resolved
/// under hold, which its `generation` names. A target serves calls from `program`, the
/// callee's own, or, held apart (`LaneStack::cross_targets`), from any other program, whose
/// calls it only activates, switching the lane to its program (`lane_enter_program`).
#[derive(Clone, Copy)]
pub(super) struct ScriptCallTarget {
    callee: Value,
    /// The `LaneStack::generation` it was resolved in.
    generation: ScriptGeneration,
    program: ProgramId,
    function: u32,
    env: Value,
    view: *const LaneView,
    registers: u16,
    /// The lane-stack room its plain activation fills (`fill_room::<SCRIPT_FILL_BLOCK>` of
    /// `registers`), derived once.
    room: u16,
    params: u16,
    /// How a call binds its `this` (`ThisBinding`).
    this_binding: ThisBinding,
    /// An arrow's own `this` (`ThisBinding::Lexical`), found in its environment when it was
    /// resolved; unused for any other binding.
    lexical_this: Value,
    /// `new` may activate it too (`Vm::inline_constructible`).
    constructs: bool,
    /// `new` may run its body as the stores alone (`Function::only_initializes_this_fields`).
    initializes: bool,
    /// How a plain call enters it.
    entry: CallEntry,
    /// The slots its depth-0 captures read (`LaneContext::capture`), or null where the walk
    /// does not end at a function environment or its storage is sparse. The storage of a live environment never
    /// moves, and the generation retires the target at the next collection.
    capture: *const crate::heap::EnvironmentSlots,
    /// The `LaneStack::forwarded_methods` index of a constructor body forwarding to a method
    /// of `this` (`Function::forwarded_this_method`), which a construction may activate
    /// directly, or `NO_FORWARDED_METHOD`. An index, not the record, keeps every target's
    /// cache entry its size.
    forwarded_method: u16,
}

/// How a plain lane call enters a frameless target (`ScriptCallTarget::entry`), one byte so the
/// call tests it once. The kinds exclude each other: a body that is one read keeps nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum CallEntry {
    /// Its activation holds its registers alone (`lane_activate`).
    Activates,
    /// It keeps the values it was passed for a forwarded `arguments`
    /// (`CallPlan::forwards_arguments`, `lane_activate_keeping`).
    Keeps,
    /// It keeps them, and its rest parameter, in the register after the others, starts as
    /// the array of those past the others (`lane_activate_keeping`).
    Gathers,
    /// Its body is one returning `GetField` (`returning_kind` of its first instruction),
    /// whose hit a call answers without activating it (`lane_call_read`).
    Reads,
    /// It needs a VM frame (`CallPlan::registers_only` fails, or its view has no
    /// activation): a call takes `lane_frame_call`. Only the cache and the call's own
    /// out-of-line form see such a target (`lane_script_target` leaves it out).
    Frame,
    /// Its body reads a field of `this` or a parameter, then an element of that by a
    /// parameter, and returns it (`element_read_body`), which a call answers likewise.
    ReadsElement,
    /// Its body returns a call of a method of a field of `this` or a parameter with its
    /// parameters in order (`forwarding_read_body`), which a call answers where that method
    /// is a `Reads` or `ReadsElement` target whose read hits.
    ForwardsRead,
    /// Its body returns one of its captures (`capture_read_body`), as a bundler's export
    /// getter `() => binding` compiles, which a call answers by reading the capture.
    ReadsCapture,
    /// Its body returns its `this` (`this_read_body`), as `get self() { return this; }`
    /// compiles, which a call answers with the receiver it binds.
    ReadsThis,
    /// It keeps frame locals and makes closures (`CallPlan::environment_activation`): its
    /// activation's window holds its environment and locals after its registers
    /// (`lane_activate_environment`). Only a plain call activates it.
    Environment,
    /// An async function whose body never suspends, laid out as its registers
    /// (`CallPlan::settles_registers_only`): only a plain call of its own program activates
    /// it (`lane_script_call_settles`), whose promise its return settles
    /// (`Completion::SettlesPromise`).
    Settles,
}

impl CallEntry {
    /// An entry another program's call activates frameless (`Vm::cross_call_entry`): a plain
    /// or keeping activation, which `lane_activate_keeping` enters across programs.
    #[inline(always)]
    fn activates_across(self) -> bool {
        matches!(self, Self::Activates | Self::Keeps | Self::Gathers)
    }

    /// A body a call answers by reading alone (`lane_call_leaf_read`).
    #[inline(always)]
    fn reads(self) -> bool {
        matches!(self, Self::Reads | Self::ReadsElement | Self::ReadsCapture | Self::ReadsThis)
    }
}

/// Whether `body` is `GetField` of `this` or a parameter, `GetField` of a method of that,
/// moves of its parameters in order into a call window, and the call of that method on the
/// first field returning its completion (`CallEntry::ForwardsRead`), as `at(i) { return
/// this.items.at(i); }` compiles.
fn forwarding_read_body(body: &Function) -> bool {
    let count = usize::from(body.params);
    let Some(code) = body.code.get(..count + 3) else {
        return false;
    };
    let decoded: Vec<WideInstruction> =
        code.iter().map(|packed| decoded_instruction(body, *packed)).collect();
    let (field, method, call) = (decoded[0], decoded[1], decoded[count + 2]);
    let base = crate::bytecode::FieldBase(field.b());
    let window = ImmediateLayout::call_window_base(call.imm());
    field.op() == Op::GetField
        && !field.returns_from_frame()
        && base.0 != crate::bytecode::FieldBase::NESTED
        && base.local_index().is_none()
        && base.register_index().is_none_or(|register| register < body.params)
        && method.op() == Op::GetField
        && !method.returns_from_frame()
        && crate::bytecode::FieldBase(method.b()).register_index() == Some(field.a())
        && decoded[2..count + 2].iter().zip(0..body.params).all(|(moved, parameter)| {
            moved.op() == Op::Move && moved.b() == parameter && moved.a() == window + parameter
        })
        && call.op() == Op::Call
        && call.a() & crate::bytecode::RETURN_REGISTER != 0
        && call.b() == method.a()
        && call.c() == field.a()
        && ImmediateLayout::argument_count(call.imm()) == body.params
}

/// Whether `body` is `GetField` of `this` or a parameter, `GetIndex` of its result by a
/// parameter, and `Return` of that (`CallEntry::ReadsElement`), as `at(i) { return
/// this.items[i]; }` compiles.
fn element_read_body(body: &Function) -> bool {
    let [field, element, done] = match body.code.get(..3) {
        Some(&[field, element, done]) => [field, element, done].map(|packed| decoded_instruction(body, packed)),
        _ => return false,
    };
    let base = crate::bytecode::FieldBase(field.b());
    field.op() == Op::GetField
        && !field.returns_from_frame()
        && base.0 != crate::bytecode::FieldBase::NESTED
        && base.local_index().is_none()
        && base.register_index().is_none_or(|register| register < body.params)
        && element.op() == Op::GetIndex
        && element.b() == field.a()
        && element.c() < body.params
        && done.op() == Op::Return
        && done.a() == element.a()
}

/// Whether `body` is `LoadCapture` and `Return` of it (`CallEntry::ReadsCapture`), as
/// `() => binding` compiles.
fn capture_read_body(body: &Function) -> bool {
    let [load, done] = match body.code.get(..2) {
        Some(&[load, done]) => [load, done].map(|packed| decoded_instruction(body, packed)),
        _ => return false,
    };
    load.op() == Op::LoadCapture
        && !load.returns_from_frame()
        && done.op() == Op::Return
        && done.a() == load.a()
}

/// Whether `body` is `LoadThis` and `Return` of it (`CallEntry::ReadsThis`): a function's own
/// `this`, which neither an arrow's lexical one nor a constructor's binding complicates.
fn this_read_body(body: &Function) -> bool {
    let [load, done] = match body.code.get(..2) {
        Some(&[load, done]) => [load, done].map(|packed| decoded_instruction(body, packed)),
        _ => return false,
    };
    !body.is_arrow
        && !body.is_class_constructor
        && load.op() == Op::LoadThis
        && !load.returns_from_frame()
        && done.op() == Op::Return
        && done.a() == load.a()
}

impl ScriptCallTarget {
    /// It keeps the values it was passed for a forwarded `arguments`.
    #[inline(always)]
    fn forwards(&self) -> bool {
        matches!(self.entry, CallEntry::Keeps | CallEntry::Gathers)
    }

    /// Its activation's return settles its call's promise (`Completion::SettlesPromise`).
    #[inline(always)]
    fn settles(&self) -> bool {
        self.entry == CallEntry::Settles
    }
}

/// How a call of a `ScriptCallTarget` binds `this`, one byte so the entry stays one cache
/// line: the receiver as is (strict), the receiver boxed or the global for nullish
/// (sloppy), or for an arrow its environment's `this` (`ScriptCallTarget::lexical_this`),
/// or the caller's receiver when its environment binds none. An arrow whose binding is
/// still uninitialized (before `super()`) is never resolved, and an initialized binding
/// never changes.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum ThisBinding {
    Sloppy,
    Strict,
    Lexical,
    CallerReceiver,
}

/// A lane run's generation (`LaneStack::generation`), narrow so a cache entry stays one
/// line; its wrap clears every entry, so no stale one can match a reused number.
type ScriptGeneration = u32;

/// The rest values a gathering activation collects inline before its array is made.
const GATHERED_INLINE_VALUES: usize = 8;

/// The size a script call cache entry keeps, one cache line, which each call reads.
const SCRIPT_CALL_TARGET_BYTES: usize = 64;
const _: () = assert!(std::mem::size_of::<ScriptCallTarget>() <= SCRIPT_CALL_TARGET_BYTES);

/// A `ScriptCallTarget::forwarded_method` that names no record. A program with more distinct
/// forwarding bodies than the index spans leaves the rest unrecorded.
const NO_FORWARDED_METHOD: u16 = u16::MAX;

const EMPTY_SCRIPT_CALL_TARGET: ScriptCallTarget = ScriptCallTarget {
    callee: Value::UNDEFINED,
    generation: NO_SCRIPT_GENERATION,
    program: ProgramId::MAIN,
    function: 0,
    env: Value::UNDEFINED,
    view: std::ptr::null(),
    registers: 0,
    room: 0,
    params: 0,
    this_binding: ThisBinding::Sloppy,
    lexical_this: Value::UNDEFINED,
    constructs: false,
    initializes: false,
    entry: CallEntry::Activates,
    capture: std::ptr::null(),
    forwarded_method: NO_FORWARDED_METHOD,
};

/// Entries of the script call cache, in sets of `SCRIPT_TARGET_WAYS` that a hash of the
/// callee's heap index picks (`ScriptTargets::set`), at most, under each memory policy
/// (`MemoryPolicy` order): Express's router calls a matcher closure per layer, some 400
/// distinct callees a request, which 256 entries thrashed (Callgrind: 2.4% of a ping request
/// re-resolving). 1024 still met conflicts in their two-way sets, re-resolving 34 callees a
/// body request (~380 instructions each); 4096 (256 KiB per table, reached only by a cache
/// that keeps displacing live targets) cut express body and ping 1.3% (Callgrind, per
/// request). Memory-first stops at 1024: a long run that displaces a few targets at a time
/// reaches the larger size too (EarleyBoyer, both tables: +220 KiB anonymous peak).
const SCRIPT_CALL_TARGETS: [usize; crate::MemoryPolicy::ALL.len()] = [4096, 1024];

/// The largest script call cache `policy` allows.
fn script_call_targets(policy: crate::MemoryPolicy) -> usize {
    SCRIPT_CALL_TARGETS[policy as usize]
}
/// Entries a script call cache starts with: a program that calls few distinct callees keeps
/// 16 KiB per table resident instead of 64 (V8-v7 max RSS, both tables: about 100 KiB per
/// fixture). A cache grows to `SCRIPT_CALL_TARGETS` by this factor each time it has
/// displaced more live targets than it holds.
const SCRIPT_CALL_TARGETS_INITIAL: usize = 256;
const SCRIPT_CALL_TARGETS_GROWTH: usize = 4;
const _: () = {
    assert!(SCRIPT_CALL_TARGETS_INITIAL.is_power_of_two() && SCRIPT_CALL_TARGETS_GROWTH.is_power_of_two());
    let mut policy = 0;
    while policy < SCRIPT_CALL_TARGETS.len() {
        let most = SCRIPT_CALL_TARGETS[policy];
        assert!(most.is_power_of_two() && SCRIPT_CALL_TARGETS_INITIAL <= most && most <= NO_CALL_LINK as usize);
        policy += 1;
    }
};
/// Entries per set: the one most recently resolved first, then the one before it, so two
/// callees whose indices share a set both stay resolved.
const SCRIPT_TARGET_WAYS: usize = 2;
const _: () = assert!(SCRIPT_TARGET_WAYS == 2);

/// A script call cache (`LaneStack::script_targets`, `cross_targets`): sets of
/// `SCRIPT_TARGET_WAYS` entries, growing from `SCRIPT_CALL_TARGETS_INITIAL` entries.
/// Entry pointers (`Vm::script_call_entry`) are not held across a resolution, which is
/// where it grows.
struct ScriptTargets {
    entries: Box<[ScriptCallTarget]>,
    /// Live targets displaced from their set since it last grew.
    displaced: usize,
    /// The hash shift of its sets (`shape_memo::direct_mapped_shift`), derived with its size.
    set_shift: u32,
}

impl ScriptTargets {
    fn with_len(len: usize) -> Self {
        Self {
            entries: vec![EMPTY_SCRIPT_CALL_TARGET; len].into_boxed_slice(),
            displaced: 0,
            set_shift: super::shape_memo::direct_mapped_shift(len / SCRIPT_TARGET_WAYS),
        }
    }

    /// The first entry of `callee`'s set, when it names a heap cell; the set's other entry
    /// follows it (`| 1`). The set is the shared direct-mapped index of the heap index
    /// (`shape_memo::direct_mapped_index`): a family's closures take indices at a fixed
    /// stride, which a multiplicative hash of the index alone crowds into a few sets.
    #[inline(always)]
    fn set(&self, callee: Value) -> Option<usize> {
        Some(super::shape_memo::direct_mapped_index_by_shift(callee.heap_index()?, self.set_shift) * SCRIPT_TARGET_WAYS)
    }

    /// Record `target` first in its set, the entry it displaces moving second; a cache that
    /// has displaced more live targets than it holds grows, empty, before recording it, up to
    /// `most` entries.
    fn remember(&mut self, target: ScriptCallTarget, most: usize) {
        let Some(set) = self.set(target.callee) else {
            return;
        };
        if self.entries[set].callee != target.callee {
            if self.entries[set | 1].generation == target.generation {
                self.displaced += 1;
            }
            self.entries[set | 1] = self.entries[set];
        }
        self.entries[set] = target;
        if self.displaced > self.entries.len() && self.entries.len() < most {
            *self = Self::with_len(self.entries.len() * SCRIPT_CALL_TARGETS_GROWTH);
            self.remember(target, most);
        }
    }

    fn clear(&mut self) {
        self.entries.fill(EMPTY_SCRIPT_CALL_TARGET);
        self.displaced = 0;
    }
}

impl std::ops::Index<usize> for ScriptTargets {
    type Output = ScriptCallTarget;
    #[inline(always)]
    fn index(&self, index: usize) -> &ScriptCallTarget {
        &self.entries[index]
    }
}

/// Entries of the function target memo (`LaneStack::function_targets`), one per function
/// id modulo the count: a closure first called finds its function's target derived when a
/// closure of the same function was resolved before, as each call of a closure-making
/// function makes a new one. A conflict overwrites the entry, which only costs the next
/// closure of the displaced function a full resolution.
const FUNCTION_TARGETS: usize = 64;
const _: () = assert!(FUNCTION_TARGETS.is_power_of_two());

/// What a constructor's initializer body (`ScriptCallTarget::initializes`) made of a receiver
/// whose adds all hit: the callee's receiver template under `prototype`, which the callee and
/// that prototype alone determine, ends at `shape`, its fields the arguments for
/// `parameters[..fields]` in order, a defaulted one's fallback in place of a falsy argument.
/// While the prototype layout keeps `epoch` (`prototype_layout_epoch`), which every
/// collection advances, those adds would hit again with the same transitions, so the object
/// can be allocated finished.
#[derive(Clone, Copy)]
struct ConstructPlan {
    callee: Value,
    epoch: u64,
    /// The callee's own layout that fixes where its `prototype` lives
    /// (`Vm::function_prototype_layout`), which a construct reads it through.
    callee_layout: (u32, usize),
    prototype: Value,
    shape: u32,
    /// The receiver's header (`ConstructPlan::derive_header`): of inline storage when its
    /// fields fit the inline slots, else of an arena run.
    header: InlineObjectHeader,
    kind: ConstructPlanKind,
    fields: u8,
    parameters: [u16; CONSTRUCT_PLAN_FIELDS],
    /// Fields whose store defaults a falsy argument (`InitializerStore::fallback`), one bit
    /// per field index, each taking `fallbacks` at its index then.
    defaulted: u8,
    fallbacks: [Value; CONSTRUCT_PLAN_FIELDS],
    /// For a forwarding constructor's plan, where its body's reads resolved
    /// (`lane_forwarded_construction`), which the plan's own proof keeps. Its fields, if any, are
    /// those its forwarded initializer stores (`lane_record_forwarded_fields`).
    forwarded: Option<ForwardedResolution>,
}

/// Which constructions allocate from a `ConstructPlan`, and how they compute its fields.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConstructPlanKind {
    /// An initializer constructor's plan (`lane_initialize_fields`) whose fields are the
    /// arguments as passed.
    Passed,
    /// An initializer constructor's plan with defaulted fields (`ConstructPlan::defaulted`).
    Defaulted,
    /// A forwarding constructor's plan (`lane_planned_receiver`), which holds only with its
    /// forwarding (`lane_forwarded_fields`).
    Forwarding,
}

/// Where a forwarding constructor's reads of `this.<method>` and its `apply` resolved under
/// a construct plan (`ConstructPlan::forwarded`). While the plan holds, the prototype layout
/// epoch is unchanged, so the plan's prototype keeps the method's data slot and the method's
/// prototype keeps `apply`'s, and the receiver template lacks the method. The values are
/// read again on every construction, and a method whose own shape or prototype differs
/// from the one recorded, which could shadow `apply`, resolves afresh.
#[derive(Clone, Copy)]
struct ForwardedResolution {
    method_slot: u16,
    method_shape: u32,
    apply_holder: Value,
    apply_slot: u16,
    /// The forwarded method's function whose initializer stores the plan's fields record
    /// (`lane_record_forwarded_fields`), when it has fields: the method this resolution
    /// reads must still be of it.
    initializer: Option<(ProgramId, u32)>,
}

/// Fields a `ConstructPlan` names; an initializer storing more keeps running its adds.
const CONSTRUCT_PLAN_FIELDS: usize = 4;
// A plan's values fill a receiver's inline slots (`ConstructPlan::allocate`).
const _: () = assert!(INLINE_PROPERTY_COUNT <= CONSTRUCT_PLAN_FIELDS);
// `ConstructPlan::defaulted` holds a bit per field.
const _: () = assert!(CONSTRUCT_PLAN_FIELDS <= u8::BITS as usize);

/// Entries of the construct plan cache, indexed by the callee's heap index.
const CONSTRUCT_PLANS: usize = 64;
const _: () = assert!(CONSTRUCT_PLANS.is_power_of_two());

/// A `ConstructPlan::epoch` no epoch reaches: the entry plans nothing.
const NO_CONSTRUCT_PLAN_EPOCH: u64 = u64::MAX;

impl ConstructPlan {
    /// This plan, holding from the current prototype layout epoch, with its receiver
    /// header derived from its prototype and shape.
    fn finish(self) -> Self {
        Self {
            epoch: crate::heap::prototype_layout_epoch(),
            ..self.derive_header()
        }
    }

    /// This plan with the header its receiver starts with, of its prototype and shape: inline
    /// storage when its fields fit the inline slots, else an arena run.
    fn derive_header(self) -> Self {
        let header = if usize::from(self.fields) <= INLINE_PROPERTY_COUNT {
            InlineObjectHeader::of(self.prototype, self.shape)
        } else {
            InlineObjectHeader::of_run(self.prototype, self.shape)
        };
        Self { header, ..self }
    }

    /// This plan's field values in order, `undefined` past them: the argument each field's
    /// parameter names (`argument`), or its fallback for a defaulted field passed a falsy
    /// value, as its store computes it.
    fn values<H: Host>(&self, vm: &Vm<H>, argument: impl Fn(u16) -> Value) -> [Value; CONSTRUCT_PLAN_FIELDS] {
        let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
        for (field, value) in values.iter_mut().enumerate().take(usize::from(self.fields)) {
            let fallback = (self.defaulted & (1 << field) != 0).then_some(self.fallbacks[field]);
            *value = initializer_value(vm, argument(self.parameters[field]), fallback);
        }
        values
    }

    /// The receiver this plan allocates with its fields `field(k)` in order and `undefined`
    /// past them, which is how unused inline slots start: gathered into the receiver's inline
    /// slots where they fit, else into the arena run's values.
    #[inline(always)]
    fn allocate(&self, heap: &mut crate::heap::Heap, field: impl Fn(usize) -> Value) -> Value {
        let fields = usize::from(self.fields);
        if fields <= INLINE_PROPERTY_COUNT {
            let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
            for (index, slot) in inline.iter_mut().enumerate().take(fields) {
                *slot = field(index);
            }
            return heap.alloc_object_from_header(self.header, self.prototype, inline);
        }
        let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
        for (index, value) in values.iter_mut().enumerate().take(fields) {
            *value = field(index);
        }
        heap.alloc_object_from_run_header(self.header, self.prototype, &values[..fields])
    }

    /// This plan extended by the field `store` adds, `None` once it names all it can.
    fn with_field(mut self, store: &crate::bytecode::InitializerStore) -> Option<Self> {
        let field = usize::from(self.fields);
        *self.parameters.get_mut(field)? = store.parameter;
        if let Some(fallback) = store.fallback {
            self.kind = ConstructPlanKind::Defaulted;
            self.defaulted |= 1 << field;
            self.fallbacks[field] = Value::number(fallback);
        }
        self.fields += 1;
        Some(self)
    }
}

const EMPTY_CONSTRUCT_PLAN: ConstructPlan = ConstructPlan {
    callee: Value::UNDEFINED,
    epoch: NO_CONSTRUCT_PLAN_EPOCH,
    callee_layout: (0, 0),
    prototype: Value::UNDEFINED,
    shape: 0,
    header: InlineObjectHeader::EMPTY,
    kind: ConstructPlanKind::Passed,
    fields: 0,
    parameters: [0; CONSTRUCT_PLAN_FIELDS],
    defaulted: 0,
    fallbacks: [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS],
    forwarded: None,
};

/// Activations a lane runs without VM frames, innermost last, over the
/// bottom VM frame of the lane run. They exist only while the lane runs:
/// every exit makes them VM frames with pending calls, exactly as frame
/// pushes would have, so the general loop, traps and collection see the
/// usual frame stack. The register storage grows only between activations,
/// so register windows stay put while any exists.
pub(super) struct LaneStack {
    values: Vec<Value>,
    activations: Vec<Activation>,
    /// Wasm activation payload represented by `values` before lane materialization.
    wasm_storage_bytes: usize,
    /// Resolved targets of lane `Call` records (`Vm::script_call_entry`) of callees of the
    /// running program.
    script_targets: ScriptTargets,
    /// Resolved targets of callees of another program than the calling one, which those
    /// calls only activate (`Vm::resolve_script_call_target`): one per callee serves every
    /// other program, apart from the target its own program's calls use.
    cross_targets: ScriptTargets,
    /// The last target resolved for a closure of each function (`FUNCTION_TARGETS`), under
    /// its generation and program: every field but the closure's own (`callee`, `env`,
    /// `capture`, an arrow's `lexical_this`) depends on the function alone.
    function_targets: Box<[ScriptCallTarget; FUNCTION_TARGETS]>,
    /// `function_targets` of other programs' callees (`cross_targets`), whose closures a
    /// program's code makes afresh as often as its own.
    cross_function_targets: Box<[ScriptCallTarget; FUNCTION_TARGETS]>,
    /// Constructors' plans (`lane_initialize_fields`, `lane_planned_receiver`).
    construct_plans: Box<[ConstructPlan; CONSTRUCT_PLANS]>,
    /// The reads call sites answered (`ReadMemo`), one per set of sites (`read_memo_index`).
    read_memos: Box<[ReadMemo; READ_MEMOS]>,
    /// The forwarding constructors whose activations constructions skipped, one per
    /// `Completion::Forwarder` activation, innermost last.
    forwarders: Vec<SkippedForwarder>,
    /// The rooted promises of the `Completion::SettlesPromise` activations, innermost last:
    /// each activation's return resolves its own, which its frame's pending call takes when
    /// it materializes (`PendingGeneralCall::settles`).
    settling: Vec<crate::heap::RootId>,
    /// The iterators the activations' `for-of` loops would close, each with the index of
    /// the activation whose loop pushed it (`IteratorCleanupPush`), innermost last: a
    /// frame's `active_iterators`, which the activation's frame takes when it materializes.
    /// A completion drops its activation's entries, as a frame's pop drops its list.
    iterators: Vec<(usize, super::ActiveIterator)>,
    /// The forwarding constructor bodies call targets name (`ScriptCallTarget::forwarded_method`),
    /// each once: a program's bodies are fixed, so the table only grows with distinct ones.
    forwarded_methods: Vec<crate::bytecode::ForwardedThisMethod>,
    /// The activation count at which a JavaScript lane call leaves for
    /// `lane_script_call_frame`: the activations' capacity, the guest calls the thread still
    /// admits (the activations hold no guards; their frames take them when they materialize,
    /// which `lane_call_native` does before any guest code runs), or none while a collection
    /// is due (`note_allocation`). One comparison stands for the three.
    activation_limit: std::cell::Cell<usize>,
    /// Names the collection epoch and realm of the running lane, which no lane instruction
    /// changes: a target resolved under another generation is stale. Lane runs move between
    /// programs far more often than either changes, so each target names its program instead.
    generation: ScriptGeneration,
    /// The epoch and realm `generation` names.
    generation_key: (u64, Value),
}

/// A generation no lane runs under: an empty target entry never matches.
const NO_SCRIPT_GENERATION: ScriptGeneration = 0;

impl Default for LaneStack {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            activations: Vec::new(),
            wasm_storage_bytes: 0,
            script_targets: ScriptTargets::with_len(SCRIPT_CALL_TARGETS_INITIAL),
            cross_targets: ScriptTargets::with_len(SCRIPT_CALL_TARGETS_INITIAL),
            function_targets: Box::new([EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS]),
            cross_function_targets: Box::new([EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS]),
            construct_plans: Box::new([EMPTY_CONSTRUCT_PLAN; CONSTRUCT_PLANS]),
            read_memos: Box::new([EMPTY_READ_MEMO; READ_MEMOS]),
            forwarders: Vec::new(),
            settling: Vec::new(),
            iterators: Vec::new(),
            forwarded_methods: Vec::new(),
            activation_limit: std::cell::Cell::new(0),
            generation: NO_SCRIPT_GENERATION,
            generation_key: (0, Value::UNDEFINED),
        }
    }
}

impl LaneStack {
    /// Records a target of the running program's callee (`script_targets`), the cache
    /// growing up to what `policy` allows.
    fn remember_script_target(&mut self, target: ScriptCallTarget, policy: crate::MemoryPolicy) {
        self.script_targets.remember(target, script_call_targets(policy));
    }

    /// `remember_script_target` of another program's callee (`cross_targets`).
    fn remember_cross_target(&mut self, target: ScriptCallTarget, policy: crate::MemoryPolicy) {
        self.cross_targets.remember(target, script_call_targets(policy));
    }

    /// Forget resolved call targets, as the general loop's call target cache is reset.
    pub(super) fn clear_script_targets(&mut self) {
        self.script_targets.clear();
        self.cross_targets.clear();
        *self.function_targets = [EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS];
        *self.cross_function_targets = [EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS];
        *self.read_memos = [EMPTY_READ_MEMO; READ_MEMOS];
    }

    /// The target `function_targets` holds for a closure of `function` of `program` in this
    /// generation, or `cross_function_targets` for calls from another program, whose closure
    /// fields the caller rebinds.
    fn function_target(&self, program: ProgramId, function: u32, cross: bool) -> Option<ScriptCallTarget> {
        let targets = if cross { &self.cross_function_targets } else { &self.function_targets };
        let known = targets[function as usize & (FUNCTION_TARGETS - 1)];
        (known.generation == self.generation && known.program == program && known.function == function)
            .then_some(known)
    }

    /// Record `target` in its set and as its function's (`function_targets`), or a `cross`
    /// one in the other programs' tables.
    fn remember_function_target(&mut self, target: ScriptCallTarget, cross: bool, policy: crate::MemoryPolicy) {
        let index = target.function as usize & (FUNCTION_TARGETS - 1);
        if cross {
            self.cross_function_targets[index] = target;
            self.remember_cross_target(target, policy);
        } else {
            self.function_targets[index] = target;
            self.remember_script_target(target, policy);
        }
    }

    /// The index of `method` in `forwarded_methods`, recording it when new, or
    /// `NO_FORWARDED_METHOD` once the index space is spent.
    fn forwarded_method_index(&mut self, method: crate::bytecode::ForwardedThisMethod) -> u16 {
        if let Some(index) = self.forwarded_methods.iter().position(|known| *known == method) {
            return index as u16;
        }
        match u16::try_from(self.forwarded_methods.len()) {
            Ok(index) if index != NO_FORWARDED_METHOD => {
                self.forwarded_methods.push(method);
                index
            }
            _ => NO_FORWARDED_METHOD,
        }
    }

    /// Push `activation` onto the stack's reserved capacity, without `Vec::push`'s growth.
    ///
    /// # Safety
    /// The caller checked that the activations' capacity exceeds their length.
    #[inline(always)]
    unsafe fn push_activation(&mut self, activation: Activation) {
        let len = self.activations.len();
        let frame_storage_bytes = activation.frame_storage_bytes;
        debug_assert!(len < self.activations.capacity(), "lane activations stay within capacity");
        // SAFETY: the slot at `len` lies within capacity and is uninitialized; the length
        // covers it only once written.
        unsafe {
            self.activations.as_mut_ptr().add(len).write(activation);
            self.activations.set_len(len + 1);
        }
        self.wasm_storage_bytes += frame_storage_bytes;
    }

    #[inline(always)]
    fn pop_activation(&mut self) -> Option<Activation> {
        let activation = self.activations.pop()?;
        self.wasm_storage_bytes -= activation.frame_storage_bytes;
        self.drop_iterators_from(self.activations.len());
        Some(activation)
    }

    /// Drops the iterator entries of the activations from index `depth` on, which completed.
    #[inline(always)]
    fn drop_iterators_from(&mut self, depth: usize) {
        if !self.iterators.is_empty() {
            self.drop_iterators_held_from(depth);
        }
    }

    #[cold]
    #[inline(never)]
    fn drop_iterators_held_from(&mut self, depth: usize) {
        self.iterators.retain(|(owner, _)| *owner < depth);
    }

    /// The generation of a lane run under `key`: the current one while the key holds, else
    /// a new one, which retires every target resolved before.
    /// Derive `activation_limit` from the activations' capacity, the guest calls the
    /// thread admits beyond the frames and activations already counted, and a due
    /// collection.
    fn refresh_activation_limit(&self) {
        let limit = if lane_floor().load(std::sync::atomic::Ordering::Relaxed) == COLLECTION_DUE_FLOOR {
            0
        } else {
            let admitted = self.activations.len() + crate::stack::GuestCallGuard::remaining();
            self.activations.capacity().min(admitted)
        };
        self.activation_limit.set(limit);
    }

    fn enter_generation(&mut self, key: (u64, Value)) {
        if self.generation == NO_SCRIPT_GENERATION || self.generation_key != key {
            self.generation = self.generation.wrapping_add(1);
            if self.generation == NO_SCRIPT_GENERATION {
                // A wrapped number could name entries of a generation long past.
                self.clear_script_targets();
                self.generation = NO_SCRIPT_GENERATION + 1;
            }
            self.generation_key = key;
        }
    }
}

/// A lane native call made beside the lane's activations (`Vm::runs_beside_activations`):
/// the lane's context and the call's record, whose frames the first observer of the frame
/// stack makes (`Vm::materialize_deferred_lane`).
pub(super) struct DeferredLaneCall {
    /// The lane's `LaneContext`, of the VM's host.
    cx: *mut (),
    call_pc: usize,
}

/// A native a lane called from its running frame after its activations became frames
/// (`lane_call_native`): the lane's `LaneContext`, the call's record and the frame count the
/// native runs at. A JavaScript call the native makes while all three still hold runs as an
/// activation of that lane (`Vm::lane_native_callback`).
#[derive(Clone, Copy)]
pub(super) struct LaneNativeCall {
    cx: *mut (),
    call: Ip,
    frames: usize,
}

/// Lane state that handlers read rarely; the hot state travels in handler
/// arguments so it stays in machine registers across dispatches. Its fields keep their
/// declared order: a plain call saves and restores `this`, `env` and `capture` together
/// (`lane_push_passed`), so they stay adjacent, and the Wasm tier's rarely read counters
/// come last.
#[repr(C)]
struct LaneContext<H: Host> {
    vm: *mut Vm<H>,
    /// The executing program, which every inline call stays within.
    code: *const ResidualProgram,
    program: ProgramId,
    /// The active frame, its registers and the view of its function.
    frame: usize,
    registers: Registers,
    base: Ip,
    /// The general loop's pending inline calls, which lane calls extend and
    /// lane returns complete exactly as the general loop would.
    pending: *mut Vec<PendingGeneralCall>,
    /// The general loop's executing program, which a materialized call into another
    /// program sets aside in its pending call as the loop's own push does.
    executing: *mut super::dispatch_frame::ExecutingProgram,
    /// The program's lane views by function, which a call reads when its
    /// callee's view is derived.
    views: *const [super::program_store::LaneViewCell],
    /// The program's constants, which constant operands index. A program's lane views are
    /// derived from its constants and replaced with them, so they hold for the run.
    constants: *const [Value],
    /// Lane calls and returns manage activations: the interpreter runs a
    /// Wasm program in a loop that allows inline calls. A compiled callee
    /// runs from the general path, which raises its traps.
    inline_calls: bool,
    /// The view handlers start from when the lane loop dispatches.
    memory: MemoryView,
    /// The last memory binding resolved in this lane run and its view. No
    /// lane instruction grows memory or runs host code, so a binding's view
    /// holds for the whole run; calls and returns within one instance reuse
    /// it instead of resolving the binding again.
    resolved: (Value, MemoryView),
    /// JavaScript calls may activate their callees without frames: the general loop
    /// would push them in place (`Vm::try_inline_user_call`).
    script_calls: bool,
    /// `new` may activate its callee without a frame too: the run's frame is an ordinary
    /// body, from which the general loop pushes constructions (`Vm::inline_construct_target`).
    script_constructs: bool,
    /// The running activation's `this`, or `DELETED` when the frame's binding decides
    /// it (`Vm::current_this`).
    this: Value,
    /// The running JavaScript activation's environment, or `DELETED` for a VM frame.
    env: Value,
    /// The innermost capture slots of the running function, once a capture read found
    /// them dense (`EnvironmentSlots::dense`); null until then and for sparse storage, which
    /// the walk reads. No collection runs in the lane, so they stay in place. Tagged with
    /// `CAPTURE_STORES` where a store to any of them is the whole write (`capture_stores`).
    capture: *const crate::heap::EnvironmentSlots,
    /// The last outer capture walk an activation read through (`lane_load_outer_capture`).
    /// One walk measured better than one per depth parity on closure-heavy code.
    outer: OuterWalk,
    /// The storage of the environment a frame run's captured locals live in, once a local
    /// access found it (`lane_frame_storage`).
    frame_storage: FrameStorage,
    /// Backward jumps left before the running Wasm function is credited with an
    /// interpreter entry's worth of loop work (`WASM_JIT_BACKEDGES_PER_ENTRY`), or `None`
    /// when this run's code earns none (`Vm::lane_credits_backedges`). A crediting run keeps
    /// its floor raised, so every backward jump reaches `lane_safepoint`, which counts: the
    /// handlers stay as they are.
    backedges_left: Option<u32>,
    /// The accumulator the lane loop dispatches its next record with: the one a safepoint
    /// jump carried to its target (`lane_safepoint`), else none.
    resumed_acc: Slot<Value>,
}

/// The slot storage of environment `env`, which frame `frame` held when a lane local access
/// looked it up; null for none. A live environment's storage never moves, and no
/// collection runs in the lane, so it holds while that frame runs with that environment.
#[derive(Clone, Copy)]
struct FrameStorage {
    frame: usize,
    env: Value,
    slots: *mut crate::heap::EnvironmentSlots,
}

const NO_FRAME_STORAGE: FrameStorage =
    FrameStorage { frame: usize::MAX, env: Value::DELETED, slots: std::ptr::null_mut() };

/// The storage of the running frame's captured environment (`LaneContext::frame_storage`),
/// for a frame run of a function other than the root whose frame holds one; `None` leaves
/// the access to the heap.
#[inline(always)]
fn lane_frame_storage<H: Host>(cx: *mut LaneContext<H>) -> Option<&'static mut crate::heap::EnvironmentSlots> {
    let vm = unsafe { vm(cx) };
    let index = unsafe { (*cx).frame };
    let frame = &vm.frames[index];
    if !frame.captured || frame.function == super::ROOT_FUNCTION_ID {
        return None;
    }
    let known = unsafe { (*cx).frame_storage };
    let slots = if known.frame == index && known.env == frame.env {
        known.slots
    } else {
        lane_find_frame_storage(cx, index, frame.env)
    };
    // SAFETY: as `FrameStorage`; the lane holds no other reference to the storage.
    unsafe { slots.as_mut() }
}

/// `lane_frame_storage` where its cache already holds the running frame's, without the
/// lookup, and the environment it belongs to, whose barrier a store takes.
#[inline(always)]
fn lane_known_frame_storage<H: Host>(
    cx: *mut LaneContext<H>,
) -> Option<(&'static mut crate::heap::EnvironmentSlots, Value)> {
    let known = unsafe { (*cx).frame_storage };
    let vm = unsafe { vm(cx) };
    if known.frame != unsafe { (*cx).frame } || known.slots.is_null() || vm.frames[known.frame].env != known.env {
        return None;
    }
    // SAFETY: as `FrameStorage`; the lane holds no other reference to the storage.
    Some((unsafe { &mut *known.slots }, known.env))
}

/// `lane_frame_storage` past its cache: the environment's storage, recorded.
#[cold]
#[inline(never)]
fn lane_find_frame_storage<H: Host>(
    cx: *mut LaneContext<H>,
    frame: usize,
    env: Value,
) -> *mut crate::heap::EnvironmentSlots {
    let vm = unsafe { vm(cx) };
    let slots = match vm.heap.get_mut(env) {
        Some(Cell::Environment { slots, .. }) => std::ptr::from_mut(slots),
        _ => std::ptr::null_mut(),
    };
    unsafe { (*cx).frame_storage = FrameStorage { frame, env, slots } };
    slots
}

/// A walk from environment `env`, `DELETED` for none, `depth` layers out to the dense slots
/// `slots`. An environment's chain is fixed and its storage stays put while no collection
/// runs, so the walk holds while the running activation's environment is `env`; a re-entry
/// after the general path, which may collect, forgets it.
#[derive(Clone, Copy)]
struct OuterWalk {
    env: Value,
    depth: u16,
    slots: *const crate::heap::EnvironmentSlots,
}

const NO_OUTER_WALK: OuterWalk = OuterWalk { env: Value::DELETED, depth: 0, slots: std::ptr::null() };

/// The tag of a `LaneContext::capture` whose every dense slot a store may write directly:
/// its environment admits every direct store (`Vm::admits_every_direct_store`) and is in the
/// remembered set already, so a write needs no barrier until the next collection, which
/// retires the target that tagged it.
const CAPTURE_STORES: usize = 1;
const _: () = assert!(std::mem::align_of::<crate::heap::EnvironmentSlots>() > CAPTURE_STORES);

/// The slots a `LaneContext::capture` reads, its tag cleared.
#[inline(always)]
fn capture_reads(capture: *const crate::heap::EnvironmentSlots) -> *const crate::heap::EnvironmentSlots {
    capture.map_addr(|address| address & !CAPTURE_STORES)
}

/// The slots a `LaneContext::capture` tagged `CAPTURE_STORES` lets a store write.
#[inline(always)]
fn capture_stores(capture: *const crate::heap::EnvironmentSlots) -> Option<*mut crate::heap::EnvironmentSlots> {
    (capture.addr() & CAPTURE_STORES != 0).then(|| capture_reads(capture).cast_mut())
}

/// A lane handler returns the next record for the lane loop, tagged with
/// [`EXIT_TAG`] when the general path must execute it. The carried state is
/// passed as scalars so every argument stays in a register; the context,
/// which hot handlers do not touch, is reached through [`LANE_CONTEXT`].
type Handler = fn(Registers, Ip, *mut u8, Slot<isize>, crate::value::IntegerEncoding, Slot<Value>) -> Ip;

thread_local! {
    /// The context of this thread's running lane. A lane run never calls
    /// out, so runs nest only through the general path, and each run
    /// restores the outer context when it returns.
    static LANE_CONTEXT: std::cell::Cell<*mut ()> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

thread_local! {
    /// The running lane's stack floor (`ExecutionInterrupt::lane_floor` of its VM): a taken
    /// safepoint jump below it returns to the lane loop. The maximum address marks a due
    /// collection or a termination request. A safepoint only reads it, so loops carry no
    /// dependence through it.
    static LANE_FLOOR: std::cell::Cell<*const std::sync::atomic::AtomicUsize> =
        const { std::cell::Cell::new(std::ptr::null()) };
}

/// The running lane's floor.
#[inline(always)]
fn lane_floor() -> &'static std::sync::atomic::AtomicUsize {
    // SAFETY: set for the whole of a lane run to its VM's interrupt state, which the VM
    // holds; only read while a lane runs.
    unsafe { &*LANE_FLOOR.with(std::cell::Cell::get) }
}

/// Stack a lane run may use below its entry before a safepoint jump returns
/// to the lane loop; only reachable should a build not form sibling calls.
const LANE_STACK_SLACK: usize = 64 * 1024;

/// The floor that sends the next safepoint jump through the slow path.
const COLLECTION_DUE_FLOOR: usize = crate::execution_interrupt::LANE_FLOOR_RAISED;

/// The current stack address.
#[cfg(quench_lane_tail_calls)]
#[inline(always)]
fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reads the stack pointer register; no memory, stack or flags.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    // SAFETY: as above.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    sp
}

/// Without sibling calls every handler returns to the lane loop, so a run's
/// stack cannot grow; this address lies above every stack floor and below
/// the due-collection floor.
#[cfg(not(quench_lane_tail_calls))]
#[inline(always)]
fn stack_pointer() -> usize {
    COLLECTION_DUE_FLOOR - 1
}

/// Whether a taken safepoint jump takes the slow path.
#[inline(always)]
fn below_floor() -> bool {
    stack_pointer() < lane_floor().load(std::sync::atomic::Ordering::Relaxed)
}

/// After a lane allocation, a due collection raises the floor.
#[inline(always)]
fn note_allocation<H: Host>(vm: &Vm<H>) {
    if vm.heap.should_collect() {
        lane_floor().store(COLLECTION_DUE_FLOOR, std::sync::atomic::Ordering::Relaxed);
        vm.lane_stack.activation_limit.set(0);
    }
}

/// The running activation's `this`; `None` where the general path decides it (a
/// derived constructor before `super()`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn lane_this<H: Host>(cx: *mut LaneContext<H>) -> Option<Value> {
    let this = unsafe { (*cx).this };
    if !this.is_deleted() {
        return Some(this);
    }
    unsafe { vm(cx).current_this((*cx).frame) }
}

/// The running lane's context.
#[inline(always)]
fn lane_context<H: Host>() -> *mut LaneContext<H> {
    LANE_CONTEXT.with(std::cell::Cell::get).cast()
}

/// Records are word aligned, so the low address bit is free to mark a lane
/// exit.
const EXIT_TAG: usize = 1;
const _: () = assert!(std::mem::align_of::<LaneInstruction>() > EXIT_TAG);

/// The general path executes the instruction at `ip`.
#[inline(always)]
fn exit(ip: Ip) -> Ip {
    ip.map_addr(|address| address | EXIT_TAG)
}

/// SAFETY: the context outlives the lane run and no other reference to the
/// VM is live while a handler runs.
#[inline(always)]
unsafe fn vm<'a, H: Host>(cx: *mut LaneContext<H>) -> &'a mut Vm<H> {
    unsafe { &mut *(*cx).vm }
}

/// Sample the identity and body a script call site targets. Diagnostic builds use this to
/// determine whether a per-site target memo is worth implementing; production builds compile
/// every call to this helper away.
#[cfg(feature = "profile-lane-exits")]
#[inline(always)]
fn profile_script_call_target<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    callee: Value,
    target: &ScriptCallTarget,
) {
    let vm = unsafe { vm(cx) };
    let caller = vm
        .lane_stack
        .activations
        .last()
        .map_or(vm.frames[unsafe { (*cx).frame }].function, |activation| {
            activation.function
        });
    let pc = unsafe { ip.offset_from((*cx).base) } as u32;
    super::lane_exits::call_target(
        unsafe { (*cx).program.raw() },
        caller,
        pc,
        callee,
        target.function,
    );
}

/// The handler of the record at `ip`.
#[inline(always)]
fn handler<H: Host>(ip: Ip) -> Handler {
    #[cfg(feature = "profile-lane-exits")]
    super::lane_exits::count_record(ip.addr(), std::mem::size_of::<LaneInstruction>());
    // SAFETY: a view is only run by the VM type whose handlers built it, and
    // every reachable record is initialized.
    unsafe { std::mem::transmute::<*const (), Handler>((*ip).handler) }
}

/// Continue with the handler of the record at `ip`.
#[cfg(quench_lane_tail_calls)]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let ip: Ip = $ip;
        let view: Carried = $view;
        let _ = $cx;
        return handler::<H>(ip)(
            $r,
            ip,
            view.memory.bytes,
            view.memory.reach,
            view.integers,
            view.acc,
        );
    }};
}

#[cfg(not(quench_lane_tail_calls))]
macro_rules! next {
    ($cx:expr, $r:expr, $ip:expr, $view:expr) => {{
        let ip: Ip = $ip;
        let _ = ($cx, $r, $view);
        return ip;
    }};
}

/// Where the lane continues once an activation completes (`lane_complete_activation`): after
/// the call in its caller, with these registers and view, or out of the lane at a frame's
/// call record that returns the completion (`lane_frame_return`).
enum Resumed {
    At(Registers, Ip, Carried),
    Exit(Ip),
}

/// Runs `handler` for the record at `ip` as a sibling call: the calling handler's fast path
/// then keeps no state across a slow path that needs more registers.
macro_rules! sibling {
    ($handler:expr, $r:expr, $ip:expr, $view:expr) => {{
        let view: Carried = $view;
        return $handler(
            $r,
            $ip,
            view.memory.bytes,
            view.memory.reach,
            view.integers,
            view.acc,
        );
    }};
}

/// Continue as `lane_complete_activation` resumed.
macro_rules! resume {
    ($cx:expr, $resumed:expr) => {{
        match $resumed {
            Resumed::At(registers, next, view) => next!($cx, registers, next, view),
            Resumed::Exit(next) => return next,
        }
    }};
}

/// Define a handler with the shared signature. `$i` is the record and
/// `$view` the current memory view.
macro_rules! lane_handler {
    ($(#[$meta:meta])* fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        lane_handler!(@define bytes [lane_context::<H>()] $(#[$meta])* fn $name<$($param: $ty),*>($cx, $r, $ip, $i, $view) $body);
    };
    (@define $bytes:ident [$context:expr] $(#[$meta:meta])* fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        $(#[$meta])*
        #[allow(unused_variables)]
        fn $name<H: Host, $(const $param: $ty),*>(
            $r: Registers,
            $ip: Ip,
            $bytes: *mut u8,
            reach: Slot<isize>,
            integers: crate::value::IntegerEncoding,
            acc: Slot<Value>,
        ) -> Ip {
            let $cx: *mut LaneContext<H> = $context;
            let $view = Carried {
                memory: MemoryView { bytes: $bytes, reach },
                integers,
                acc,
            };
            // SAFETY: the lane only dispatches records of the running view.
            let $i = unsafe { &*$ip };
            $body
        }
    };
}

/// `lane_handler!` for a handler only JavaScript views dispatch: the run's context comes
/// from the carried view (`MemoryView::script`) rather than the thread-local load.
macro_rules! script_handler {
    ($(#[$meta:meta])* fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        lane_handler!(@define bytes [{
            let cx: *mut LaneContext<H> = bytes.cast();
            debug_assert_eq!(cx, lane_context::<H>(), "a JavaScript view carries its context");
            cx
        }] $(#[$meta])* fn $name<$($param: $ty),*>($cx, $r, $ip, $i, $view) $body);
    };
}

/// Which field of a record reads the accumulator: none, or the field the
/// constant names. The view selects a reading instantiation only where the
/// field's register is the previous record's result register and control
/// reaches the record only by falling through from it.
const ACC_NONE: u8 = 0;
const ACC_A: u8 = 1;
const ACC_B: u8 = 2;
const ACC_C: u8 = 3;
/// The first register of the immediate's register pair: a select's
/// condition.
const ACC_PAIR: u8 = 4;
/// Accumulator positions a record may read: none, A, B, C and the pair.
const ACC_POSITIONS: usize = 5;

impl InstructionField {
    const fn accumulator(self) -> u8 {
        match self {
            Self::A => ACC_A,
            Self::B => ACC_B,
            Self::C => ACC_C,
        }
    }
}

/// The value of register field `FIELD` of a record whose accumulator field
/// is `ACC`.
#[inline(always)]
fn read<const ACC: u8, const FIELD: u8>(r: Registers, register: u16, view: Carried) -> Value {
    if ACC == FIELD {
        // SAFETY: the view selects an accumulator-reading record only after the producer
        // that set it.
        unsafe { view.acc.get() }
    } else {
        r.get(register)
    }
}

#[inline(always)]
fn read_i32<const ACC: u8, const FIELD: u8>(r: Registers, register: u16, view: Carried) -> i32 {
    read::<ACC, FIELD>(r, register, view).wasm_bits32() as i32
}

/// Write an accumulator producer's result and continue with it carried.
macro_rules! produce {
    ($cx:expr, $r:expr, $ip:expr, $register:expr, $value:expr, $view:expr) => {{
        let value: Value = $value;
        $r.set($register, value);
        next!(
            $cx,
            $r,
            following($ip),
            Carried {
                acc: Slot::of(value),
                ..$view
            }
        )
    }};
}

/// The record after `ip`.
#[inline(always)]
fn following(ip: Ip) -> Ip {
    // SAFETY: validation ends every function in a terminator, so a lane
    // instruction that falls through has a successor.
    unsafe { ip.add(1) }
}

/// The opcode a handler instantiation serves.
const fn opcode(index: u16) -> Op {
    match Op::from_index(index as usize) {
        Some(op) => op,
        None => panic!("lane handler for an opcode outside the opcode table"),
    }
}

lane_handler! {
    /// Not a lane instruction: the general path executes it.
    fn lane_exit<>(cx, r, ip, i, view) {
        exit(ip)
    }
}

lane_handler! {
    fn lane_move<ACC: u8>(cx, r, ip, i, view) {
        produce!(cx, r, ip, i.a, read::<ACC, ACC_B>(r, i.b, view), view)
    }
}

lane_handler! {
    fn lane_load_const<>(cx, r, ip, i, view) {
        produce!(cx, r, ip, i.a, i.constant(), view)
    }
}

lane_handler! {
    fn lane_load_local_plain<>(cx, r, ip, i, view) {
        // SAFETY: validated bytecode bounds the slot by Function.locals, and
        // frame setup sizes locals to that count.
        let value = unsafe { vm(cx).read_validated_local((*cx).frame, i.imm() as usize) };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// `LoadLocal` and `LoadEnvLocal` of an initialized binding a VM frame owns
    /// (`Vm::initialized_frame_binding`); any other read takes the general path.
    fn lane_load_local<>(cx, r, ip, i, view) {
        let value = if unsafe { (*cx).env }.is_deleted()
            && i.a & crate::bytecode::NUMERIC_LOCAL_TARGET == 0
        {
            // SAFETY: the context's program outlives the lane run.
            (unsafe { vm(cx) }).initialized_frame_binding(
                unsafe { &*(*cx).code },
                unsafe { (*cx).frame },
                i.imm() as usize,
            )
        } else {
            None
        };
        let Some(value) = value else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

/// The value a frame-backed run reads from `slot` of frame `frame`'s function: the frame's
/// environment slot when `environment` and the environment is captured, else its local.
/// `None` for a root body's frame or an uninitialized binding.
#[inline(always)]
fn frame_window_local<H: Host>(vm: &Vm<H>, frame: usize, slot: usize, environment: bool) -> Option<Value> {
    let frame = &vm.frames[frame];
    if frame.function == super::ROOT_FUNCTION_ID {
        return None;
    }
    let value = if environment && frame.captured {
        vm.heap.environment_slot(frame.env, slot)?
    } else {
        *frame.locals.get(slot)?
    };
    (!value.is_deleted()).then_some(value)
}

script_handler! {
    /// A local load of a function an environment activation may run
    /// (`specialize_environment_window`): in a frame's run, the frame's binding, as
    /// `lane_load_frame_local`; in an environment activation, its environment's slot
    /// (`ENVIRONMENT`) or its window's local. An uninitialized binding or a numeric local
    /// takes the general path. `lane_load_window_local` answers the common case first.
    #[inline(never)]
    fn lane_load_window_local_general<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let value = if i.a & crate::bytecode::NUMERIC_LOCAL_TARGET != 0 {
            None
        } else if unsafe { (*cx).env }.is_deleted() {
            match (if ENVIRONMENT { lane_frame_storage(cx) } else { None }).and_then(|storage| storage.dense()) {
                Some(dense) => dense.dense_plain(slot).or_else(|| {
                    let vm = unsafe { vm(cx) };
                    vm.heap.environment_slot(vm.frames[unsafe { (*cx).frame }].env, slot)
                }),
                None => frame_window_local(unsafe { vm(cx) }, unsafe { (*cx).frame }, slot, ENVIRONMENT),
            }
        } else if ENVIRONMENT {
            // SAFETY: an environment window holds its environment's slot storage, which
            // never moves while the environment lives (`lane_activate_environment`).
            let storage = unsafe {
                &*(std::mem::transmute::<Value, u64>(r.get(registers + WINDOW_SLOTS))
                    as *const crate::heap::EnvironmentSlots)
            };
            match storage.dense().and_then(|dense| dense.dense_plain(slot)) {
                Some(value) => Some(value),
                None => unsafe { vm(cx) }.heap.environment_slot(r.get(registers + WINDOW_ENVIRONMENT), slot),
            }
        } else {
            Some(r.get(registers + WINDOW_LOCALS + slot as u16))
        };
        let Some(value) = value.filter(|value| !value.is_deleted()) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_load_window_local_general`'s store: to an initialized (or initializing)
    /// binding. `lane_store_window_local` answers the common case first.
    #[inline(never)]
    fn lane_store_window_local_general<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let value = r.get(i.a);
        let vm = unsafe { vm(cx) };
        let binding = if unsafe { (*cx).env }.is_deleted() {
            let storage = if ENVIRONMENT { lane_frame_storage(cx) } else { None };
            let frame = &mut vm.frames[unsafe { (*cx).frame }];
            if frame.function == super::ROOT_FUNCTION_ID {
                None
            } else if ENVIRONMENT && frame.captured {
                // A write to a slot through its storage takes the environment's barrier first.
                vm.heap.remember_cell(frame.env);
                match storage.and_then(|storage| storage.owned_mut(slot)) {
                    Some(binding) => Some(binding),
                    None => vm.heap.environment_slot_mut(frame.env, slot),
                }
            } else {
                frame.locals.get_mut(slot)
            }
        } else if ENVIRONMENT {
            let environment = r.get(registers + WINDOW_ENVIRONMENT);
            // A write to a slot through its storage takes the environment's barrier first.
            vm.heap.remember_cell(environment);
            // SAFETY: as `lane_load_window_local`'s read of the window's storage.
            let storage = unsafe {
                &mut *(std::mem::transmute::<Value, u64>(r.get(registers + WINDOW_SLOTS))
                    as *mut crate::heap::EnvironmentSlots)
            };
            match storage.owned_mut(slot) {
                Some(binding) => Some(binding),
                None => vm.heap.environment_slot_mut(environment, slot),
            }
        } else {
            // SAFETY: an environment activation's window holds its locals after its
            // environment (`lane_activate_environment`).
            Some(unsafe { &mut *r.0.add(usize::from(registers + WINDOW_LOCALS) + slot) })
        };
        match binding {
            Some(binding) if i.c != 0 || !binding.is_deleted() => *binding = value,
            _ => return exit(ip),
        }
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_load_window_local`'s `InitializeTdz`: opens the binding's dead zone.
    fn lane_initialize_window_tdz<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        let vm = unsafe { vm(cx) };
        let binding = if unsafe { (*cx).env }.is_deleted() {
            // SAFETY: the context's program outlives the lane run.
            vm.frame_binding_mut(unsafe { &*(*cx).code }, unsafe { (*cx).frame }, slot)
        } else if ENVIRONMENT {
            // SAFETY: as `lane_load_window_local`'s read of the window's storage; a marker
            // needs no barrier.
            let storage = unsafe {
                &mut *(std::mem::transmute::<Value, u64>(r.get(registers + WINDOW_SLOTS))
                    as *mut crate::heap::EnvironmentSlots)
            };
            match storage.owned_mut(slot) {
                Some(binding) => Some(binding),
                None => vm.heap.environment_slot_mut(r.get(registers + WINDOW_ENVIRONMENT), slot),
            }
        } else {
            // SAFETY: as `lane_store_window_local`.
            Some(unsafe { &mut *r.0.add(usize::from(registers + WINDOW_LOCALS) + slot) })
        };
        let Some(binding) = binding else {
            return exit(ip);
        };
        *binding = Value::DELETED;
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `MakeClosure` of a function an environment activation may run: in a frame's run, as
    /// `lane_make_closure`; in an environment activation, over its environment.
    fn lane_make_window_closure<>(cx, r, ip, i, view) {
        let (function, registers) = environment_window_fields(i);
        let value = if unsafe { (*cx).env }.is_deleted() {
            lane_closure_value_of(cx, function as u32)
        } else {
            lane_window_closure_value(cx, r, function as u32, registers)
        };
        let Some(value) = value else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// `lane_make_window_closure`'s closure of `function` over the running environment
/// activation's environment, whose window starts at register `registers`. Out of line, as
/// `lane_new_object`: making the closure takes stack temporaries.
#[inline(never)]
fn lane_window_closure_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    function: u32,
    registers: u16,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let owner = vm.lane_stack.activations.last().map(|activation| activation.function);
    // SAFETY: the running environment activation's window, whose function is the
    // innermost activation's and whose environment's parent is the context's.
    let environment = owner.map(|owner| unsafe {
        window_environment(vm, &*(*cx).code, r.0.add(usize::from(registers)), owner, (*cx).env, (*cx).program, (*cx).this)
    });
    // SAFETY: the context's program outlives the lane run.
    let value = environment.and_then(|environment| vm.closure(unsafe { &*(*cx).code }, function, environment).ok());
    note_allocation(vm);
    value
}

lane_handler! {
    /// `StoreLocal` and `StoreEnvLocal` to a binding a VM frame owns that
    /// `Vm::store_initialized_frame_binding` completes; any other store takes the general
    /// path. A `StoreEnvLocal` of a function that resolves names at run time, which may
    /// write a `with` object's binding instead, has no record (`derive_lane_view`).
    fn lane_store_local<>(cx, r, ip, i, view) {
        let value = r.get(i.a);
        // SAFETY: the context's program outlives the lane run.
        let stored = unsafe { (*cx).env }.is_deleted()
            && (unsafe { vm(cx) }).store_initialized_frame_binding(
                unsafe { &*(*cx).code },
                unsafe { (*cx).frame },
                i.imm() as usize,
                value,
                i.c != 0,
            );
        if !stored {
            return exit(ip);
        }
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}

/// The value of a register or constant operand; other operand kinds take
/// the general path.
#[inline(always)]
fn lane_operand_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    encoded: u16,
) -> Option<Value> {
    let operand = crate::bytecode::Operand(encoded);
    match operand.kind()? {
        crate::bytecode::OperandKind::Register => Some(r.get(operand.payload())),
        // SAFETY: the run's program keeps its constants (`LaneContext::constants`).
        crate::bytecode::OperandKind::Constant => unsafe { &*(*cx).constants }
            .get(usize::from(operand.payload()))
            .copied(),
        crate::bytecode::OperandKind::Local => lane_plain_local_operand(cx, operand.payload()),
        _ => None,
    }
}

/// A local operand of the running function that is a plain slot, which the general loop
/// reads as `LoadLocalPlain` does (`Vm::operand_value`): generators and other functions that
/// keep frame locals compare and compute with them in place, and an environment
/// activation reads its window's local (`lane_load_window_local`). Any other frameless
/// activation has no locals; any other local is the general path's.
#[inline(never)]
fn lane_plain_local_operand<H: Host>(cx: *mut LaneContext<H>, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(activation) = vm.lane_stack.activations.last() {
        activation.script().filter(|script| script.environment)?;
        let body = code.function_get(activation.function as usize)?;
        if !body.plain_local(code, usize::from(slot)) {
            return None;
        }
        // SAFETY: the running function's registers, which its window's locals follow.
        let value = unsafe { (*cx).registers }.get(body.registers + WINDOW_LOCALS + slot);
        return (!value.is_deleted()).then_some(value);
    }
    // SAFETY: its frame is the running one.
    let frame = unsafe { (*cx).frame };
    let frame = vm.frames.get(frame)?;
    let slot = usize::from(slot);
    code.functions
        .get(frame.function as usize)?
        .plain_local(code, slot)
        .then(|| frame.locals.get(slot).copied())
        .flatten()
}

script_handler! {
    /// `IteratorCleanupPush` in the running VM frame, as the general loop runs it: the frame's
    /// iterator to close if the loop exits abruptly. A frameless activation keeps it on the
    /// lane stack (`LaneStack::iterators`), which its frame takes if it materializes.
    fn lane_iterator_cleanup_push<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let iterator = super::ActiveIterator { iterator: i.a, done: i.b };
        if let Some(depth) = vm.lane_stack.activations.len().checked_sub(1) {
            vm.lane_stack.iterators.push((depth, iterator));
        } else {
            let frame = unsafe { (*cx).frame };
            vm.frames[frame].active_iterators.push(iterator);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `IteratorCleanupPop` in the running VM frame (`lane_iterator_cleanup_push`).
    fn lane_iterator_cleanup_pop<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let popped = match vm.lane_stack.activations.len().checked_sub(1) {
            // The running activation's own entry, the last.
            Some(depth) => vm.lane_stack.iterators.pop_if(|(owner, _)| *owner == depth).is_some(),
            None => {
                let frame = unsafe { (*cx).frame };
                vm.frames[frame].active_iterators.pop().is_some()
            }
        };
        if !popped {
            return exit(ip);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

lane_handler! {
    /// `DefineArrayElement` into the literal's dense storage (`Vm::define_literal_element`);
    /// any other definition takes the general path.
    fn lane_define_array_element<>(cx, r, ip, i, view) {
        if !(unsafe { vm(cx) }).define_literal_element(r.get(i.b), i.imm() as usize, r.get(i.a)) {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `RequireIteratorResult` of an object, which passes; any other value takes the general
    /// path, which throws.
    fn lane_require_iterator_result<>(cx, r, ip, i, view) {
        let result = r.get(i.b);
        if !result.is_record() && !(unsafe { vm(cx) }).is_object_like(result) {
            return exit(ip);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

lane_handler! {
    fn lane_store_local_plain<>(cx, r, ip, i, view) {
        let value = r.get(i.a);
        // SAFETY: validated bytecode bounds the slot by Function.locals.
        unsafe { vm(cx).write_validated_local((*cx).frame, i.imm() as usize, value) };
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `InitializeTdz`: opens the binding's dead zone wherever the frame keeps it, as the
    /// general path does (`Vm::frame_binding_mut`).
    fn lane_initialize_tdz<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let Some(binding) = (unsafe { vm(cx) }).frame_binding_mut(
            unsafe { &*(*cx).code },
            unsafe { (*cx).frame },
            i.imm() as usize,
        ) else {
            return exit(ip);
        };
        *binding = Value::DELETED;
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `CloneEnv` in the lane's own frame, as the general path makes it
    /// (`Vm::clone_frame_environment`): nothing to copy while the frame holds no captured
    /// environment, else the frame's environment replaced by its copy (a `for (let ...)`
    /// iteration's, which a closure in the body captures), after which the run's capture
    /// caches name the old environment and are dropped. An activation's environment is its
    /// window's, so its clone takes the general path.
    fn lane_clone_env<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !vm.lane_stack.activations.is_empty() {
            return exit(ip);
        }
        let frame = unsafe { (*cx).frame };
        if vm.frames[frame].captured {
            // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
            // running function's view.
            let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
            let body = code.function(vm.frames[frame].function as usize);
            let Some(plan) = body.environment_clones.get(decoded_instruction(body, body.code[pc]).environment_clone_index())
            else {
                return exit(ip);
            };
            // It checks the environment before it allocates the copy, so an error leaves the
            // frame for the general path to raise it from.
            if vm.clone_frame_environment(frame, &plan.fresh, &plan.reset).is_err() {
                return exit(ip);
            }
            note_allocation(vm);
            unsafe {
                (*cx).capture = std::ptr::null();
                (*cx).outer = NO_OUTER_WALK;
                (*cx).frame_storage = NO_FRAME_STORAGE;
            }
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// Numbers and BigInts convert to themselves; other values take the general path.
    fn lane_to_numeric<>(cx, r, ip, i, view) {
        let value = r.get(i.b);
        if value.as_number().is_none() && !matches!(unsafe { vm(cx) }.heap.get(value), Some(Cell::BigInt(_))) {
            return exit(ip);
        }
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `MakeConstArray`: the new array of the running program's constant elements, as the
    /// general loop makes it (`Vm::const_array_value`).
    #[inline(never)]
    fn lane_make_const_array<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let function = match vm.lane_stack.activations.last() {
            Some(activation) => activation.function,
            None => vm.frames[unsafe { (*cx).frame }].function,
        };
        // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
        // running function's view.
        let (code, pc, program) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize, (*cx).program) };
        let body = code.function(function as usize);
        let Some(value) = vm.const_array_value(program, decoded_instruction(body, body.code[pc])) else {
            return exit(ip);
        };
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `RequireObjectCoercible`: any value but `null` and `undefined` passes; the general
    /// path throws for those.
    fn lane_require_object_coercible<>(cx, r, ip, i, view) {
        let value = r.get(i.b);
        if value.is_null() || value.is_undefined() {
            return exit(ip);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

lane_handler! {
    /// An instruction whose general step only continues or throws (`lane_steps_in_place`),
    /// run in place by that step when the lane runs the frame's own code: the frame's
    /// registers are the lane's, and its PC is published as the general loop publishes it.
    /// A throw is stashed in `Vm::lane_raised` for the general path to raise at this
    /// instruction. Within a frameless activation, which has no frame, the general path runs
    /// it.
    #[inline(never)]
    fn lane_general_step<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !vm.lane_stack.activations.is_empty() {
            return lane_step_beside_activations(cx, r, ip);
        }
        // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
        // active view, which is the running frame's.
        let (code, frame, pc) = unsafe {
            (&*(*cx).code, (*cx).frame, ip.offset_from((*cx).base) as usize)
        };
        let function = code.function(vm.frames[frame].function as usize);
        let instruction = decoded_instruction(function, function.code[pc]);
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = false;
        let mut next = pc + 1;
        match vm.step(code, frame, instruction, &mut next, false) {
            Ok(StepResult::Continue) if next == pc + 1 => {}
            Ok(_) => unreachable!("an in-place step only continues"),
            Err(error) => {
                vm.lane_raised = Some(error);
                return exit(ip);
            }
        }
        // The step may allocate, which can make a collection due.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// `lane_general_step` within a frameless activation: a value step (`Vm::value_step`) of
/// the activation's registers, made beside the activations as a native runs beside them
/// (`DeferredLaneCall`), so guest code it reaches makes them frames first; its result lands
/// in the running function's registers then. Any other step takes the general path.
#[cold]
#[inline(never)]
fn lane_step_beside_activations<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip) -> Ip {
    let vm = unsafe { vm(cx) };
    let Some(function) = vm.lane_stack.activations.last().map(|activation| activation.function) else {
        return exit(ip);
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(function as usize);
    let instruction = decoded_instruction(body, body.code[pc]);
    if !matches!(instruction.op(), Op::ToPropertyKey | Op::GetIterator | Op::IteratorClose | Op::AppendArray) {
        return exit(ip);
    }
    let [a, b] = super::dispatch::value_step_operands(instruction).map(|register| register.map(|register| r.get(register)));
    let Some(value) = lane_in_place(cx, ip, |vm, code| vm.value_step(code, instruction, a, b)) else {
        return exit(ip);
    };
    // Guest code the step ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    if let Some(value) = value {
        r.set(instruction.result_register(), value);
    }
    next!(cx, r, following(ip), Carried::script(cx))
}

/// Make `operation` of the record at `ip` from the lane, as its general step would: in a
/// frame's own run with the frame's PC published (`lane_general_step`), or beside frameless
/// activations as a native beside them runs (`DeferredLaneCall`), so guest code it reaches
/// makes them frames first. A throw is stashed in `Vm::lane_raised`, raised from those
/// frames, and `None` is returned for the general path to raise it. The running function's
/// registers are `(*cx).registers` afterwards.
#[inline(always)]
fn lane_in_place<H: Host, T>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    operation: impl FnOnce(&mut Vm<H>, &ResidualProgram) -> Result<T, JsError>,
) -> Option<T> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // running function's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let beside = !vm.lane_stack.activations.is_empty();
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc: pc });
    } else {
        let frame = unsafe { (*cx).frame };
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = false;
    }
    let result = operation(vm, code);
    // The operation may allocate, which can make a collection due.
    note_allocation(vm);
    let unobserved = beside && vm.lane_deferred.take().is_some();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc: pc });
                vm.materialize_deferred_lane();
            }
            vm.lane_raised = Some(error);
            None
        }
    }
}

/// Instructions whose general step reads and writes only the frame, its registers and heap
/// values, runs guest code only through nested calls, and continues to the next instruction
/// or throws: no jump, call, return, `this` binding or environment change the lane caches.
const fn lane_steps_in_place(op: Op) -> bool {
    matches!(
        op,
        Op::DefineField
            | Op::DefineComputedField
            | Op::DefinePrivateField
            | Op::CheckPrivate
            | Op::PrivateIn
            | Op::MarkPrivateName
            | Op::SetFunctionName
            | Op::SetFunctionNameKey
            | Op::SetFunctionDebugName
            | Op::DefineArrayElement
            | Op::AppendArray
            | Op::ToPropertyKey
            | Op::CopyDataProperties
            | Op::GetIterator
            | Op::RequireIteratorResult
            | Op::IteratorClose
            | Op::IteratorCleanupPush
            | Op::IteratorCleanupPop
    )
}

script_handler! {
    /// `IncDec` of a Number (`number_increment`); a BigInt takes `lane_inc_dec_bigint` and any
    /// other value the general path.
    /// The view derives the direction from the record (`specialize_updates`).
    fn lane_inc_dec<DECREMENT: bool>(cx, r, ip, i, view) {
        let Some(value) = super::dispatch::number_increment(r.get(i.b), DECREMENT) else {
            sibling!(lane_inc_dec_bigint::<H, DECREMENT>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_inc_dec` of a value other than a Number: a BigInt's update (`lane_bigint_increment`);
    /// any other value takes the general path. Out of line, so the Number update forms its
    /// dispatch as a sibling call whatever the BigInt update inlines.
    #[inline(never)]
    fn lane_inc_dec_bigint<DECREMENT: bool>(cx, r, ip, i, view) {
        let Some(value) = lane_bigint_increment::<H, DECREMENT>(cx, r.get(i.b)) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The update of a BigInt (`Vm::bigint_increment`, which allocates its result); `None` for
/// any other value. Out of line: its arithmetic keeps stack temporaries a handler inlining
/// it would hold across its dispatch, which then could not be a sibling call.
#[inline(never)]
fn lane_bigint_increment<H: Host, const DECREMENT: bool>(cx: *mut LaneContext<H>, value: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let next = vm.bigint_increment(value, DECREMENT)?;
    note_allocation(vm);
    Some(next)
}

script_handler! {
    /// `IncDec` of a Number whose result the next record, a register `GetIndex`
    /// (`lane_get_index_registers`), reads as its index, the `a[++i]` of compiled loops
    /// (`specialize_updated_index_reads`): the update, then that read of a record array's
    /// dense element, as one record. A read the pair's fast path leaves runs the `GetIndex`
    /// record itself; another value takes the general path at the update.
    fn lane_update_get_index<DECREMENT: bool>(cx, r, ip, i, view) {
        let Some(index) = super::dispatch::number_increment(r.get(i.b), DECREMENT) else {
            return exit(ip);
        };
        r.set(i.a, index);
        // SAFETY: the pair is two records, the second never the view's last.
        let read = unsafe { &*following(ip) };
        let Some(value) = (unsafe { vm(cx) }).direct_dense_element(r.get(read.b), index) else {
            next!(cx, r, following(ip), Carried::script(cx))
        };
        r.set(read.a, value);
        next!(cx, r, following(following(ip)), Carried::script(cx))
    }
}

script_handler! {
    /// A postfix update of a Number or BigInt (`lane_bigint_increment`): `ToNumeric` into `a` and the
    /// `IncDec` of its result into `c` that follows it (`specialize_updates`). Other values
    /// take the general path at the conversion.
    fn lane_postfix_update<DECREMENT: bool>(cx, r, ip, i, view) {
        let value = r.get(i.b);
        let Some(updated) = super::dispatch::number_increment(value, DECREMENT)
            .or_else(|| lane_bigint_increment::<H, DECREMENT>(cx, value))
        else {
            return exit(ip);
        };
        r.set(i.a, value);
        r.set(i.c, updated);
        next!(cx, r, following(following(ip)), Carried::script(cx))
    }
}

script_handler! {
    /// Numeric operands on the integer fast path; the regional site profile
    /// records the execution exactly as the general path does.
    fn lane_binary<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_binary_value(cx, r, i) }) else {
            sibling!(lane_binary_general::<H>, r, ip, Carried::script(cx))
        };
        if i.a & crate::bytecode::NUMERIC_LOCAL_TARGET != 0 {
            let local = usize::from(i.a & crate::bytecode::REGISTER_MASK);
            // SAFETY: the view admits a numeric local target only below
            // Function.locals.
            unsafe { vm(cx).write_validated_local((*cx).frame, local, value) };
        } else {
            r.set(i.a, value);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_binary` of operands its value declines: the general operation made in place
    /// (`lane_in_place`), which may reach guest code (a proxy's `has`, a
    /// `Symbol.hasInstance`, a `valueOf` or `toString`) or throw. A numeric local target takes
    /// the general path.
    #[inline(never)]
    fn lane_binary_general<>(cx, r, ip, i, view) {
        let operator = i.imm();
        if i.a & crate::bytecode::NUMERIC_LOCAL_TARGET != 0 {
            return exit(ip);
        }
        let (Some(left), Some(right)) = (lane_operand_value(cx, r, i.b), lane_operand_value(cx, r, i.c))
        else {
            return exit(ip);
        };
        let Some(value) = lane_in_place(cx, ip, |vm, code| vm.binary(code, operator, left, right)) else {
            return exit(ip);
        };
        // Guest code the operator ran made the activations frames, whose registers these are.
        let r = unsafe { (*cx).registers };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// A `Binary` record's value where it runs no guest code: Number arithmetic and
/// relations, then identity equality and an ordinary `instanceof`.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
unsafe fn lane_binary_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let left = lane_operand_value(cx, r, i.b)?;
    let right = lane_operand_value(cx, r, i.c)?;
    let operator = i.imm();
    match unsafe { vm(cx) }.number_binary(operator, left, right) {
        Some(value) => Some(value),
        None if operator == super::operations::ADDITION => lane_concatenation(cx, left, right)
            .or_else(|| lane_bigint_value(cx, operator, left, right)),
        // A comparison, or `instanceof` (`lane_relation_other`), decides from the operands;
        // any other operator past Numbers may be a BigInt's.
        None if operator <= super::operations::LAST_COMPARISON_OPERATOR || operator == INSTANCEOF_OPERATOR => {
            lane_relation::<H>(operator, left, right)
                .map(|holds| if holds { Value::TRUE } else { Value::FALSE })
        }
        None => lane_bigint_value(cx, operator, left, right),
    }
}

/// `lane_binary_value` of a returning `Binary`, which keeps its operands encoded: two
/// integer registers decide it in place (`integer_binary`), as the operator-specialized
/// records decide theirs; any other operands take the general value.
///
/// # Safety
/// As for `lane_binary_value`.
#[inline(always)]
unsafe fn lane_returning_binary_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let register = |encoded: u16| {
        let operand = crate::bytecode::Operand(encoded);
        (operand.kind() == Some(crate::bytecode::OperandKind::Register)).then(|| r.get(operand.payload()))
    };
    if let (Some(left), Some(right)) = (register(i.b), register(i.c))
        && let Some((left, right)) = Value::int_pair(left, right)
        && let Some(value) = super::operations::integer_binary(i.imm(), left, right)
    {
        return Some(value);
    }
    unsafe { lane_binary_value(cx, r, i) }
}

/// A BigInt operator of two BigInts that raises nothing (`Vm::bigint_binary_value`), which
/// allocates the result; `None` leaves the operation, and any error, to the general path.
#[inline(never)]
fn lane_bigint_value<H: Host>(
    cx: *mut LaneContext<H>,
    operator: u32,
    left: Value,
    right: Value,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let value = vm.bigint_binary_value(operator, left, right)?;
    note_allocation(vm);
    Some(value)
}

/// `+` of operands that become strings without guest code (`Vm::primitive_concatenation`),
/// which allocates the result; `None` leaves any other `+` to the general path. Out of line,
/// so the numeric handlers that fall back to it keep their code.
#[inline(never)]
fn lane_concatenation<H: Host>(cx: *mut LaneContext<H>, left: Value, right: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.primitive_concatenation(unsafe { &*(*cx).code }, left, right)?;
    note_allocation(vm);
    Some(value)
}

/// A relation the lane decides without running guest code: Number comparison, identity
/// decided from the operand words, then `lane_relation_other`. Matches rather than
/// `Option::or_else` closures, which capture the operands by reference and, left out of
/// line, would cost the calling handler its tail call to the next record.
#[inline(always)]
fn lane_relation<H: Host>(operator: u32, left: Value, right: Value) -> Option<bool> {
    if let Some(holds) = super::operations::number_relation(operator, left, right) {
        return Some(holds);
    }
    if let Some(holds) = Vm::<H>::word_identity_equality(operator, left, right) {
        return Some(holds);
    }
    lane_relation_other::<H>(operator, left, right)
}

/// Return `value` from the running function at the record `ip`, as `Return` would: the
/// innermost activation completes (`lane_complete_activation`); a script frame the lane
/// pushed returns in place (`lane_return_script_frame`); any other VM frame returns the value
/// from this record (`lane_frame_return`).
macro_rules! lane_return_value {
    ($cx:expr, $ip:expr, $value:expr) => {{
        let (cx, ip, value): (*mut LaneContext<H>, Ip, Value) = ($cx, $ip, $value);
        if let Some(resumed) = lane_complete_plain_activation(cx, value) {
            resume!(cx, resumed)
        }
        return lane_return_value_general(cx, ip, value);
    }};
}

/// `lane_return_value!` past the common return (`lane_complete_plain_activation`): any
/// other activation's completion, or the frame's return. Out of line, so the returning
/// handlers keep their registers.
#[inline(never)]
fn lane_return_value_general<H: Host>(cx: *mut LaneContext<H>, ip: Ip, value: Value) -> Ip {
    // As `lane_return_general`: a run of returning calls, then another program's activation.
    if let Some(resumed) = lane_complete_plain_chain(cx, value) {
        resume!(cx, resumed)
    }
    if let Some(resumed) = lane_complete_cross_activation(cx, value) {
        resume!(cx, resumed)
    }
    if let Some(resumed) = lane_complete_activation(cx, ip, value) {
        resume!(cx, resumed)
    }
    if !unsafe { (*cx).inline_calls } && unsafe { (*cx).script_calls } {
        return lane_return_script_frame(cx, ip, value, false);
    }
    lane_frame_return(unsafe { vm(cx) }, ip, value)
}


script_handler! {
    /// `lane_load_window_local_general` in an environment activation reading an initialized
    /// non-numeric binding its window or in-place storage holds densely, saving no
    /// registers; any other load takes the general handler.
    fn lane_load_window_local<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        // A frame run whose environment storage a local access found (`lane_frame_storage`):
        // the one read this record makes inline; every other takes the window's out of line,
        // so this one keeps few registers.
        if ENVIRONMENT {
            let (slot, _) = environment_window_fields(i);
            if i.a & crate::bytecode::NUMERIC_LOCAL_TARGET == 0
                && unsafe { (*cx).env }.is_deleted()
                && let Some((storage, _)) = lane_known_frame_storage(cx)
                && let Some(value) = storage.dense().and_then(|dense| dense.dense_plain(slot))
            {
                r.set(i.a, value);
                next!(cx, r, following(ip), view)
            }
            sibling!(lane_load_window_local_window::<H, true>, r, ip, view)
        }
        let (slot, registers) = environment_window_fields(i);
        if i.a & crate::bytecode::NUMERIC_LOCAL_TARGET == 0
            && !unsafe { (*cx).env }.is_deleted()
            && let Some(value) = Some(r.get(registers + WINDOW_LOCALS + slot as u16)).filter(|value| !value.is_deleted())
        {
            r.set(i.a, value);
            next!(cx, r, following(ip), view)
        }
        sibling!(lane_load_window_local_general::<H, false>, r, ip, view)
    }
}

script_handler! {
    /// `lane_load_window_local` of a binding its window or in-place storage holds densely;
    /// any other load takes the general handler.
    #[inline(never)]
    fn lane_load_window_local_window<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        if i.a & crate::bytecode::NUMERIC_LOCAL_TARGET == 0 && !unsafe { (*cx).env }.is_deleted() {
            let value = if ENVIRONMENT {
                // SAFETY: as `lane_load_window_local_general`'s read of the window's storage.
                let storage = unsafe {
                    &*(std::mem::transmute::<Value, u64>(r.get(registers + WINDOW_SLOTS))
                        as *const crate::heap::EnvironmentSlots)
                };
                storage.dense().and_then(|dense| dense.dense_plain(slot))
            } else {
                Some(r.get(registers + WINDOW_LOCALS + slot as u16)).filter(|value| !value.is_deleted())
            };
            if let Some(value) = value {
                r.set(i.a, value);
                next!(cx, r, following(ip), view)
            }
        }
        sibling!(lane_load_window_local_general::<H, ENVIRONMENT>, r, ip, view)
    }
}

script_handler! {
    /// `lane_store_window_local_general` in an environment activation writing an
    /// initialized binding its window or in-place storage holds densely, saving no
    /// registers; any other store takes the general handler. The write needs no barrier:
    /// an activation's environment is made during its lane run (`window_environment`), in a
    /// slot no collection has marked since, and no collection runs in the lane, so it is
    /// young.
    fn lane_store_window_local<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, _) = environment_window_fields(i);
        // A frame run's store to an initialized binding of its environment's storage, after
        // the environment's barrier.
        if ENVIRONMENT
            && i.c == 0
            && unsafe { (*cx).env }.is_deleted()
            && let Some((storage, environment)) = lane_known_frame_storage(cx)
            && let Some(binding) = storage.dense_mut().and_then(|dense| dense.dense_plain_mut(slot))
        {
            let value = r.get(i.a);
            *binding = value;
            unsafe { vm(cx) }.heap.remember_cell(environment);
            if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
                r.set(register, value);
            }
            next!(cx, r, following(ip), view)
        }
        if ENVIRONMENT {
            sibling!(lane_store_window_local_window::<H, true>, r, ip, view)
        }
        sibling!(lane_store_window_local_window::<H, false>, r, ip, view)
    }
}

script_handler! {
    /// `lane_store_window_local` of a binding its window or in-place storage holds densely,
    /// out of line so the frame run's store keeps few registers; any other store takes the
    /// general handler.
    #[inline(never)]
    fn lane_store_window_local_window<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, registers) = environment_window_fields(i);
        if !unsafe { (*cx).env }.is_deleted() && i.c == 0 {
            let binding = if ENVIRONMENT {
                // SAFETY: as `lane_load_window_local_general`'s read of the window's storage.
                let storage = unsafe {
                    &mut *(std::mem::transmute::<Value, u64>(r.get(registers + WINDOW_SLOTS))
                        as *mut crate::heap::EnvironmentSlots)
                };
                storage.dense_mut().and_then(|dense| dense.dense_plain_mut(slot))
            } else {
                // SAFETY: an environment activation's window holds its locals after its
                // entries (`lane_activate_environment`).
                Some(unsafe { &mut *r.0.add(usize::from(registers + WINDOW_LOCALS) + slot) })
                    .filter(|binding| !binding.is_deleted())
            };
            if let Some(binding) = binding {
                debug_assert!(!unsafe { vm(cx) }.heap.is_old(r.get(registers + WINDOW_ENVIRONMENT)));
                let value = r.get(i.a);
                *binding = value;
                if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
                    r.set(register, value);
                }
                next!(cx, r, following(ip), view)
            }
        }
        sibling!(lane_store_window_local_general::<H, ENVIRONMENT>, r, ip, view)
    }
}

lane_handler! {
    /// `lane_load_local` of a slot whose binding facts the view fixed
    /// (`specialize_frame_locals`): no mapped `arguments` mirrors it, and it moves into the
    /// frame's environment once that is captured exactly when `ENVIRONMENT`. A root body's
    /// frame, an uninitialized binding or a numeric local takes the general path.
    fn lane_load_frame_local<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        if !unsafe { (*cx).env }.is_deleted() || i.a & crate::bytecode::NUMERIC_LOCAL_TARGET != 0 {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let frame = &vm.frames[unsafe { (*cx).frame }];
        if ENVIRONMENT && frame.captured {
            sibling!(lane_load_frame_environment_local::<H>, r, ip, view)
        }
        let value = (frame.function != super::ROOT_FUNCTION_ID)
            .then(|| frame.locals.get(i.imm() as usize).copied())
            .flatten();
        let Some(value) = value.filter(|value| !value.is_deleted()) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// `lane_load_frame_local` of a slot the frame's captured environment holds; out of line,
    /// so the frame-local read saves no registers.
    #[inline(never)]
    fn lane_load_frame_environment_local<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let frame = &vm.frames[unsafe { (*cx).frame }];
        let value = (frame.function != super::ROOT_FUNCTION_ID)
            .then(|| vm.heap.environment_slot(frame.env, i.imm() as usize))
            .flatten();
        let Some(value) = value.filter(|value| !value.is_deleted()) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// `lane_store_local` of a slot `lane_load_frame_local`'s facts hold for, which is no
    /// self binding either: the store to an initialized (or initializing) binding.
    fn lane_store_frame_local<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        if !unsafe { (*cx).env }.is_deleted() {
            return exit(ip);
        }
        let value = r.get(i.a);
        let vm = unsafe { vm(cx) };
        let frame = &mut vm.frames[unsafe { (*cx).frame }];
        if ENVIRONMENT && frame.captured {
            sibling!(lane_store_frame_environment_local::<H>, r, ip, view)
        }
        let binding = (frame.function != super::ROOT_FUNCTION_ID)
            .then(|| frame.locals.get_mut(i.imm() as usize))
            .flatten();
        match binding {
            Some(binding) if i.c != 0 || !binding.is_deleted() => *binding = value,
            _ => return exit(ip),
        }
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// `lane_store_frame_local` to a slot the frame's captured environment holds; out of
    /// line, as `lane_load_frame_environment_local`.
    #[inline(never)]
    fn lane_store_frame_environment_local<>(cx, r, ip, i, view) {
        let value = r.get(i.a);
        let vm = unsafe { vm(cx) };
        let frame = &vm.frames[unsafe { (*cx).frame }];
        let binding = (frame.function != super::ROOT_FUNCTION_ID)
            .then(|| vm.heap.environment_slot_mut(frame.env, i.imm() as usize))
            .flatten();
        match binding {
            Some(binding) if i.c != 0 || !binding.is_deleted() => *binding = value,
            _ => return exit(ip),
        }
        if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
            r.set(register, value);
        }
        next!(cx, r, following(ip), view)
    }
}


script_handler! {
    /// `this` where the lane holds it; the frame's `this` takes `lane_load_frame_this`,
    /// so this handler makes no call and saves no registers.
    fn lane_load_this<>(cx, r, ip, i, view) {
        let this = unsafe { (*cx).this };
        if this.is_deleted() {
            sibling!(lane_load_frame_this::<H>, r, ip, view)
        }
        r.set(i.a, this);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `LoadThis` of the frame's `this` (`lane_this`).
    #[inline(never)]
    fn lane_load_frame_this<>(cx, r, ip, i, view) {
        // An uninitialized `this` throws on the general path.
        let Some(value) = (unsafe { lane_this(cx) }) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}


script_handler! {
    /// `Binary` of operator `OP` on two registers, its operator and operand kinds resolved
    /// when the view was derived (`specialize_register_operands`): the fields hold the
    /// operand registers themselves. Integers take the fast path here, which saves no
    /// registers; the first other operand switches the record to `lane_operator_numbers`.
    fn lane_operator<OP: u32>(cx, r, ip, i, view) {
        let Some(value) = super::operations::integer_operator::<OP>(r.get(i.b), r.get(i.c)) else {
            requicken::<H>(ip, lane_operator_numbers::<H, OP>);
            sibling!(lane_operator_numbers::<H, OP>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `Binary` of a comparison (`LAST_COMPARISON_OPERATOR`) on a register and a register
    /// (`CONSTANT` false: the operator stays in the immediate) or a constant (`CONSTANT`
    /// true: the operator moves to `c`), decoded when the view was derived
    /// (`specialize_register_operands`): the boolean of Number comparison of two integers
    /// (`integer_relation`). Any other pair takes `lane_relation_value_other`.
    fn lane_relation_value<CONSTANT: bool>(cx, r, ip, i, view) {
        let (operator, left, right) = lane_relation_operands::<CONSTANT>(r, i);
        let Some(holds) = super::operations::integer_relation(operator, left, right) else {
            sibling!(lane_relation_value_other::<H, CONSTANT>, r, ip, Carried::script(cx))
        };
        r.set(i.a, if holds { Value::TRUE } else { Value::FALSE });
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_relation_value` of operands other than two integers: the relations the lane
    /// decides without guest code (`lane_relation`); any other takes the general path. Out
    /// of line, so the integer case keeps few values live.
    #[inline(never)]
    fn lane_relation_value_other<CONSTANT: bool>(cx, r, ip, i, view) {
        let (operator, left, right) = lane_relation_operands::<CONSTANT>(r, i);
        let Some(holds) = lane_relation::<H>(operator, left, right) else {
            return exit(ip);
        };
        r.set(i.a, if holds { Value::TRUE } else { Value::FALSE });
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// A `lane_relation_value` record's operator and operands.
#[inline(always)]
fn lane_relation_operands<const CONSTANT: bool>(
    r: Registers,
    i: &LaneInstruction,
) -> (u32, Value, Value) {
    if CONSTANT {
        (u32::from(i.c), r.get(i.b), i.constant())
    } else {
        (i.imm(), r.get(i.b), r.get(i.c))
    }
}

script_handler! {
    /// `lane_operator` at a site that has met operands other than two integers: any Numbers,
    /// through their double values. Other operands take `lane_operator_other`, out of line,
    /// so this handler saves no registers for them.
    #[inline(never)]
    fn lane_operator_numbers<OP: u32>(cx, r, ip, i, view) {
        let Some(value) = super::operations::number_value_operator::<OP>(r.get(i.b), r.get(i.c))
        else {
            sibling!(lane_operator_other::<H, OP>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_operator_numbers` of operands not both Numbers: a concatenation of strings or
    /// BigInt arithmetic in place; anything else takes the general path.
    #[inline(never)]
    fn lane_operator_other<OP: u32>(cx, r, ip, i, view) {
        let (left, right) = (r.get(i.b), r.get(i.c));
        let value = if OP == super::operations::ADDITION {
            lane_concatenation(cx, left, right).or_else(|| lane_bigint_value(cx, OP, left, right))
        } else {
            lane_bigint_value(cx, OP, left, right)
        };
        let Some(value) = value else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_operator` whose right operand is a constant, carried in the record.
    fn lane_operator_constant<OP: u32>(cx, r, ip, i, view) {
        let Some(value) = super::operations::integer_operator::<OP>(r.get(i.b), i.constant())
        else {
            requicken::<H>(ip, lane_operator_constant_numbers::<H, OP>);
            sibling!(lane_operator_constant_numbers::<H, OP>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_operator_numbers` with a constant right operand; a concatenation takes
    /// `lane_operator_constant_concatenation`, out of line.
    #[inline(never)]
    fn lane_operator_constant_numbers<OP: u32>(cx, r, ip, i, view) {
        let Some(value) = super::operations::number_value_operator::<OP>(r.get(i.b), i.constant())
        else {
            if OP == super::operations::ADDITION {
                sibling!(lane_operator_constant_concatenation::<H>, r, ip, Carried::script(cx))
            }
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_operator_constant_numbers` of an addition that concatenates.
    #[inline(never)]
    fn lane_operator_constant_concatenation<>(cx, r, ip, i, view) {
        let Some(value) = lane_concatenation(cx, r.get(i.b), i.constant()) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// `plain_keyed_get` at lane site `ip`, recording the own slot a symbol key's proof gives
/// for the site's next read (`Vm::site_symbol_own_read`). Out of line: the site's hits skip it.
#[inline(never)]
fn lane_site_keyed_get<H: Host>(vm: &mut Vm<H>, ip: Ip, base: Value, key: Value) -> Option<Value> {
    let value = vm.plain_keyed_get(base, key)?;
    vm.record_site_symbol_read(ip.addr(), base, key);
    Some(value)
}

/// A string's element in the lane (`Vm::string_element`), which allocates its one-unit string.
#[inline(never)]
fn lane_string_element<H: Host>(vm: &mut Vm<H>, base: Value, key: Value) -> Option<Value> {
    let value = vm.string_element(base, key)?;
    note_allocation(vm);
    Some(value)
}

lane_handler! {
    /// A present dense array element; any other read takes the general path.
    fn lane_get_index<>(cx, r, ip, i, view) {
        let (Some(base), Some(key)) =
            (lane_operand_value(cx, r, i.b), lane_operand_value(cx, r, i.c))
        else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        let Some(value) = vm
            .dense_array_element(base, key)
            .or_else(|| vm.absent_array_element(base, key))
            .or_else(|| vm.plain_keyed_get(base, key))
            .or_else(|| vm.plain_typed_element(base, key))
            .or_else(|| lane_string_element(vm, base, key))
            .or_else(|| vm.string_index_element(base, key))
        else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_get_index` with register base and key, decoded when the view was derived
    /// (`specialize_register_operands`).
    /// An array whose object is a record runs here, saving no registers; any other base takes
    /// `lane_get_index_slot`.
    fn lane_get_index_registers<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { vm(cx) }).direct_dense_element(r.get(i.b), r.get(i.c)) else {
            sibling!(lane_get_index_slot::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_index_registers` past a record array.
    #[inline(never)]
    fn lane_get_index_slot<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (base, key) = (r.get(i.b), r.get(i.c));
        // A symbol-keyed read this site made of the same shape and key reads its own slot.
        if let Some(value) = vm.site_symbol_own_read(ip.addr(), base, key) {
            r.set(i.a, value);
            next!(cx, r, following(ip), Carried::script(cx))
        }
        let Some(value) = lane_index_slot_element(vm, ip, base, key) else {
            sibling!(lane_get_index_getter::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_index_slot` of an element no data read answers: an index getter's
    /// activation (`lane_index_getter_activation`), else the general path. Apart, since the
    /// activation's result is a stack temporary of the handler that makes it.
    #[inline(never)]
    fn lane_get_index_getter<>(cx, r, ip, i, view) {
        let (base, key) = (r.get(i.b), r.get(i.c));
        let vm = unsafe { vm(cx) };
        if !vm.lane_stack.activations.is_empty() && vm.proved_symbol_getter(base, key).is_none() {
            return lane_get_index_beside_activations(cx, ip, i, base, key);
        }
        match lane_index_getter_activation(cx, r, ip, base, key) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(next) => next,
        }
    }
}

/// A `GetIndex` of `base[key]` past the lane's reads within a frameless activation: the
/// general read made in place (`lane_in_place`), as `lane_get_field_beside_activations`
/// makes a field's, so a getter, proxy or throw makes the activations frames only when it
/// runs.
#[cold]
#[inline(never)]
fn lane_get_index_beside_activations<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    i: &LaneInstruction,
    base: Value,
    key: Value,
) -> Ip {
    let Some(value) = lane_in_place(cx, ip, |vm, code| vm.get_index(code, base, key)) else {
        return exit(ip);
    };
    // Guest code the read ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    r.set(i.a, value);
    next!(cx, r, following(ip), Carried::script(cx))
}

/// `base[key]` of a data element `lane_get_index_slot` reads past a record array: a dense
/// or absent array element, a keyed read this site recorded, a typed array's or a string's
/// element. Out of line, as `lane_set_index_slot_store`.
#[inline(never)]
fn lane_index_slot_element<H: Host>(vm: &mut Vm<H>, ip: Ip, base: Value, key: Value) -> Option<Value> {
    vm.dense_array_element(base, key)
        .or_else(|| vm.absent_array_element(base, key))
        .or_else(|| lane_site_keyed_get(vm, ip, base, key))
        .or_else(|| vm.plain_typed_element(base, key))
        .or_else(|| lane_string_element(vm, base, key))
        .or_else(|| vm.string_index_element(base, key))
}

script_handler! {
    /// `delete base[key]` of a string key on an ordinary object, which runs no guest code:
    /// the deletion's `true`. A refused deletion (a non-configurable property) has changed
    /// nothing and takes the general path, which strict code makes an error; so does any
    /// other base or key.
    #[inline(never)]
    fn lane_delete<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let (base, key) = (r.get(i.b), r.get(i.c));
        let plain = base.direct_object_index().is_some()
            && vm.heap.string(key).is_some()
            && vm
                .object_data(base)
                .is_some_and(|data| !data.is_module_namespace() && !data.is_arguments_object());
        if !plain || !vm.object_delete_property(code, &[base, key]).is_ok_and(|deleted| deleted == Value::TRUE) {
            return exit(ip);
        }
        r.set(i.a, Value::TRUE);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// A lane `GetIndex` of a symbol key whose proof records an inherited getter
/// (`Vm::proved_symbol_getter`): the getter runs as a frameless activation of the read's
/// record, as `lane_getter_activation` runs a field's, with the base as receiver. `Err`
/// carries the record to continue at: the general path for any other read.
#[inline(never)]
fn lane_index_getter_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    base: Value,
    key: Value,
) -> Result<(Registers, Ip), Ip> {
    if !unsafe { (*cx).script_calls } {
        return Err(exit(ip));
    }
    let vm = unsafe { vm(cx) };
    let Some(getter) = vm.proved_symbol_getter(base, key) else {
        return Err(exit(ip));
    };
    lane_activate_getter(cx, r, ip, getter, base)
}

script_handler! {
    /// Replacing a present dense element; any other write takes the general path.
    /// An array whose object is a record runs here, saving no registers; any other object
    /// takes `lane_set_index_slot`.
    fn lane_set_index<>(cx, r, ip, i, view) {
        let (object, key) = (r.get(i.b), r.get(i.c));
        if !(unsafe { vm(cx) }).try_replace_record_element(object, key, r.get(i.a)) {
            sibling!(lane_set_index_slot::<H>, r, ip, Carried::script(cx))
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_set_index` past a record array, appending to a dense array at its length,
    /// filling one of its holes, a Number stored into a typed array
    /// (`Vm::typed_array_set_number`), or a plain symbol-, string- or integer-keyed write. A
    /// symbol write repeats the step its site recorded for the receiver's shape first
    /// (`Vm::site_symbol_set`).
    #[inline(never)]
    fn lane_set_index_slot<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !lane_set_index_slot_store(vm, ip, r.get(i.b), r.get(i.c), r.get(i.a)) {
            if vm.lane_stack.activations.is_empty() {
                return exit(ip);
            }
            return lane_set_index_beside_activations(cx, r, ip, i);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// `lane_set_index_slot` past the lane's stores within a frameless activation: the general
/// store made in place (`lane_in_place`), strict as its record or function is.
#[cold]
#[inline(never)]
fn lane_set_index_beside_activations<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Ip {
    let vm = unsafe { vm(cx) };
    let Some(function) = vm.lane_stack.activations.last().map(|activation| activation.function) else {
        return exit(ip);
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(function as usize);
    let instruction = decoded_instruction(body, body.code[pc]);
    if instruction.op() != Op::SetIndex {
        return exit(ip);
    }
    let strict = body.strict || instruction.boolean_flag().unwrap_or(false);
    let (object, key, value) = (r.get(i.b), r.get(i.c), r.get(i.a));
    if lane_in_place(cx, ip, |vm, code| vm.set_index_mode(code, object, key, value, strict)).is_none() {
        return exit(ip);
    }
    // Guest code the store ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    next!(cx, r, following(ip), Carried::script(cx))
}

/// `lane_set_index_slot`'s store of `value` at `object[key]` for the record at `ip`; `false`
/// stores nothing. Out of line, as every store a lane handler makes through calls whose
/// stack temporaries escape: the handler's frame then holds none across its dispatch, which
/// forms as a sibling call (`tools/check-lane-tail-calls.sh`).
#[inline(never)]
fn lane_set_index_slot_store<H: Host>(vm: &mut Vm<H>, ip: Ip, object: Value, key: Value, value: Value) -> bool {
    if !vm.site_symbol_set(ip.addr(), object, key, value)
        && !vm.try_replace_dense_element(object, key, value)
        && !vm.try_append_dense_element(object, key, value)
        && !vm.try_fill_dense_hole(object, key, value)
        && !vm.try_site_symbol_set(ip.addr(), object, key, value)
        && !vm.try_host_private_symbol_set(object, key, value)
        && !lane_typed_array_store(ip, vm, object, key, value)
        && !lane_string_key_store(ip, vm, object, key, value)
        && !key
            .as_int()
            .and_then(|index| usize::try_from(index).ok())
            .is_some_and(|index| vm.set_plain_index_property(object, index, value).is_some())
    {
        return false;
    }
    // Appending an element or adding a property can allocate.
    note_allocation(vm);
    true
}

/// A store of a number at an index of a typed array, which none of the array and symbol
/// paths before it serve: the site then tries it first (`lane_set_index_typed`).
fn lane_typed_array_store<H: Host>(ip: Ip, vm: &mut Vm<H>, object: Value, key: Value, value: Value) -> bool {
    let stored = vm
        .array_index_key(key)
        .is_some_and(|index| vm.typed_array_set_number(object, index as usize, value));
    if stored {
        requicken::<H>(ip, lane_set_index_typed::<H>);
    }
    stored
}

/// A plain string-keyed write (`Vm::try_plain_string_key_set`), which every array, symbol
/// and typed-array path before it declines: the site then tries it first
/// (`lane_set_index_string`).
fn lane_string_key_store<H: Host>(ip: Ip, vm: &mut Vm<H>, object: Value, key: Value, value: Value) -> bool {
    let stored = vm.try_plain_string_key_set(object, key, value);
    if stored {
        requicken::<H>(ip, lane_set_index_string::<H>);
    }
    stored
}

script_handler! {
    /// `lane_set_index_slot` at a site that made a plain string-keyed write: that write is
    /// the whole store, as the generic order reaches it once every array, symbol and typed
    /// path declines a string key; any other store takes the generic order.
    fn lane_set_index_string<>(cx, r, ip, i, view) {
        let (object, key, value) = (r.get(i.b), r.get(i.c), r.get(i.a));
        let vm = unsafe { vm(cx) };
        if !vm.try_plain_string_key_set(object, key, value) {
            sibling!(lane_set_index_slot::<H>, r, ip, view)
        }
        // Adding a property can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_set_index_slot` at a site that stored into a typed array: a number stored at an
    /// index of one is the whole store, as the generic order reaches it once every array and
    /// symbol path declines a typed array; any other store takes the generic order.
    fn lane_set_index_typed<>(cx, r, ip, i, view) {
        let (object, key, value) = (r.get(i.b), r.get(i.c), r.get(i.a));
        let vm = unsafe { vm(cx) };
        let stored = vm
            .array_index_key(key)
            .is_some_and(|index| vm.typed_array_set_number(object, index as usize, value));
        if !stored {
            sibling!(lane_set_index_slot::<H>, r, ip, view)
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A capture the frame's environment owns directly; any other read takes the general path.
    /// A read through the frame's current capture cache runs here, saving no registers; any
    /// other takes `lane_load_capture_walk`.
    fn lane_load_capture<>(cx, r, ip, i, view) {
        let imm = i.imm();
        // Deeper reads take `lane_load_outer_capture` (`specialize_outer_captures`).
        debug_assert_eq!(ImmediateLayout::capture_depth(imm), 0);
        let slots = capture_reads(unsafe { (*cx).capture });
        let value = if !slots.is_null() {
            // SAFETY: the running function's dense capture slots, found this run
            // (`LaneContext::capture`).
            (unsafe { &*slots }).dense_plain(usize::from(ImmediateLayout::capture_slot(imm)))
        } else {
            None
        };
        let Some(value) = value else {
            sibling!(lane_load_capture_walk::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_load_capture` of a capture past the running function's own scope
    /// (`specialize_outer_captures`). An activation's read through the walk it read last
    /// (`LaneContext::outer`) runs here, saving no registers; a frame's read takes
    /// `lane_load_frame_outer_capture`, any other `lane_load_outer_capture_walk`.
    fn lane_load_outer_capture<>(cx, r, ip, i, view) {
        let imm = i.imm();
        let env = unsafe { (*cx).env };
        if env.is_deleted() {
            sibling!(lane_load_frame_outer_capture::<H>, r, ip, view)
        }
        let depth = ImmediateLayout::capture_depth(imm);
        let walk = unsafe { (*cx).outer };
        let value = if walk.env == env && walk.depth == depth {
            // SAFETY: as `OuterWalk`.
            unsafe { &*walk.slots }.dense().and_then(|slots| slots.dense_plain(usize::from(ImmediateLayout::capture_slot(imm))))
        } else {
            None
        };
        let Some(value) = value else {
            sibling!(lane_load_outer_capture_walk::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_load_outer_capture` in an activation through another walk: the walk from its
    /// environment this lane run or an earlier one remembered, or walked now and remembered
    /// (`Vm::walked_capture_slots`), ending at a function's dense slot, which the context
    /// then reads through (`LaneContext::outer`); any other read takes
    /// `lane_load_capture_walk`.
    #[inline(never)]
    fn lane_load_outer_capture_walk<>(cx, r, ip, i, view) {
        let imm = i.imm();
        let vm = unsafe { vm(cx) };
        let (env, depth) = (unsafe { (*cx).env }, ImmediateLayout::capture_depth(imm));
        let walked = vm.walked_capture_slots(env, false, depth);
        // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
        let value = walked.and_then(|(slots, root)| {
            let value = root
                .is_none()
                .then(|| unsafe { &*slots }.dense()?.dense_plain(usize::from(ImmediateLayout::capture_slot(imm))))
                .flatten()?;
            unsafe { (*cx).outer = OuterWalk { env, depth, slots } };
            Some(value)
        });
        let Some(value) = value else {
            sibling!(lane_load_capture_walk::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_load_outer_capture` in a frame's run: the walk remembered from the frame's
    /// environment, or the walk.
    #[inline(never)]
    fn lane_load_frame_outer_capture<>(cx, r, ip, i, view) {
        let imm = i.imm();
        let vm = unsafe { vm(cx) };
        let frame = &vm.frames[unsafe { (*cx).frame }];
        let depth = ImmediateLayout::capture_depth(imm);
        let Some(value) =
            vm.remembered_capture(frame.env, frame.captured, depth, ImmediateLayout::capture_slot(imm))
        else {
            sibling!(lane_load_capture_walk::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

/// A capture of a root environment's global var or global lexical
/// (`Vm::plain_projected_root_binding`), which a script's functions read for its top-level
/// `var`s, and in an eval script's context its `let`s and `const`s: the capture's environment
/// as the general path finds it, from the frame or from the activation's environment.
#[inline(never)]
fn lane_projected_capture<H: Host>(cx: *mut LaneContext<H>, depth: u16, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).env };
    let env = if env.is_deleted() {
        vm.capture_env(unsafe { (*cx).frame }, depth)?
    } else {
        vm.environment_capture_env(env, false, depth)?
    };
    vm.plain_projected_root_binding(env, slot)
}

/// A frame run's innermost capture slots for `LaneContext::capture`, when the frame's
/// cache holds them dense: tagged `CAPTURE_STORES` when their environment admits every
/// store, whose barrier this takes once, as `Vm::target_capture` tags an activation's. The
/// first store of a frame run asks (`lane_store_capture`).
#[inline(never)]
fn lane_frame_capture<H: Host>(cx: *mut LaneContext<H>) -> Option<*const crate::heap::EnvironmentSlots> {
    let vm = unsafe { vm(cx) };
    let frame = unsafe { (*cx).frame };
    let Some((slots, None)) = vm.current_capture_slots(frame) else {
        return None;
    };
    // SAFETY: the frame's cache names live slots this collection; none runs in the lane.
    let dense = std::ptr::from_ref(unsafe { &*slots }.dense()?);
    // The cache's slots are those of the frame's depth-0 capture environment.
    let env = vm.capture_env(frame, 0)?;
    // SAFETY: the context's program outlives the lane run.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    if vm.admits_every_direct_store(code, program, env) {
        vm.heap.remember_cell(env);
        return Some(dense.map_addr(|address| address | CAPTURE_STORES));
    }
    Some(dense)
}

script_handler! {
    /// `lane_load_capture` before the running function's slots are known: the walk from
    /// the frame's cache, or from the activation's environment, which keeps them.
    #[inline(never)]
    fn lane_load_capture_walk<>(cx, r, ip, i, view) {
        let imm = i.imm();
        let depth = ImmediateLayout::capture_depth(imm);
        let slot = ImmediateLayout::capture_slot(imm);
        let vm = unsafe { vm(cx) };
        let env = unsafe { (*cx).env };
        let value = if env.is_deleted() {
            let frame = unsafe { (*cx).frame };
            let value = vm.cached_direct_capture(frame, depth, slot);
            if depth == 0
                && let Some((slots, None)) = vm.current_capture_slots(frame)
                && let Some(slots) = unsafe { &*slots }.dense()
            {
                unsafe { (*cx).capture = slots };
            }
            value
        } else {
            // A frameless activation's function has no environment of its own.
            vm.walked_capture_slots(env, false, depth).and_then(|(slots, root)| {
                // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
                let slots = unsafe { &*slots };
                if depth == 0
                    && root.is_none()
                    && let Some(dense) = slots.dense()
                {
                    unsafe { (*cx).capture = dense };
                }
                vm.owned_capture(slots, root, slot)
            })
        };
        #[cfg(feature = "profile-lane-exits")]
        {
            let code = unsafe { &*(*cx).code };
            let function = vm
                .lane_stack
                .activations
                .last()
                .map_or(vm.frames[unsafe { (*cx).frame }].function, |activation| activation.function);
            let body = code.function_get(function as usize);
            let shape = vm.current_capture_slots(unsafe { (*cx).frame }).map(|(slots, root)| {
                (root.is_some(), unsafe { &*slots }.dense().is_some(), unsafe { &*slots }.len())
            });
            super::lane_exits::record_detail(format!(
                "capture walk in {} {} span={:?} depth={depth} frame_run={} (root, dense)={shape:?}",
                vm.programs.source_name(unsafe { (*cx).program }).unwrap_or_default(),
                body.and_then(|body| body.name).map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                body.and_then(|body| body.source_span).map(|span| span.start),
                env.is_deleted(),
            ));
        }
        let Some(value) = value.or_else(|| lane_projected_capture(cx, depth, slot)) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `Unary` of `UNARY_TYPEOF_INDEX`, chosen when the view was derived
    /// (`specialize_register_operands`): the index of `typeof`'s name, which runs no guest code.
    fn lane_typeof_index<>(cx, r, ip, i, view) {
        r.set(i.a, unsafe { vm(cx) }.typeof_index_value(r.get(i.b)));
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `Unary`: logical not and `typeof` of any value, and the numeric operators of a
    /// Number, run no guest code; any other operand takes the general path.
    fn lane_unary<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_unary_value(cx, r, i) }) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// A `Unary` record's value where it runs no guest code and throws nothing: logical not and
/// `typeof` of any value, and every other operator of a Number, String, Boolean, null or
/// undefined, whose conversions are pure.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
unsafe fn lane_unary_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let operator = i.imm();
    let operand = r.get(i.b);
    let vm = unsafe { vm(cx) };
    if operator == super::coercion::LOGICAL_NOT {
        return Some(vm.logical_not(operand));
    }
    if let Some((index, negated)) = crate::bytecode::typeof_test(operator) {
        return Some(vm.typeof_test_value(operand, index, negated));
    }
    if operator == crate::bytecode::UNARY_TYPEOF_INDEX {
        return Some(vm.typeof_index_value(operand));
    }
    if operator == super::coercion::TYPEOF {
        let name = vm.typeof_value(operand);
        // Interning the type's name can allocate.
        note_allocation(vm);
        return Some(name);
    }
    if operand.as_number().is_some() && super::coercion::numeric_unary(operator) {
        // SAFETY: the context's program outlives the lane run.
        return vm.unary(unsafe { &*(*cx).code }, operator, operand).ok();
    }
    let pure_primitive = operand.as_bool().is_some()
        || operand.is_null()
        || operand.is_undefined()
        || operand.as_number().is_some()
        || vm.is_string(operand);
    if !pure_primitive {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let value = vm.unary(unsafe { &*(*cx).code }, operator, operand).ok()?;
    // Converting to a string can allocate.
    note_allocation(vm);
    Some(value)
}

script_handler! {
    /// `StoreCapture` that writes an initialized slot directly
    /// (`Vm::direct_store_environment`); any other store takes the general path.
    fn lane_store_capture<>(cx, r, ip, i, view) {
        let imm = i.imm();
        let depth = ImmediateLayout::capture_depth(imm);
        if depth == 0
            && let Some(slots) = capture_stores(unsafe { (*cx).capture })
            // SAFETY: the running function's dense capture slots, which admit this write
            // (`CAPTURE_STORES`); no collection runs in the lane.
            && let Some(binding) = unsafe { &mut *slots }.dense_plain_mut(usize::from(ImmediateLayout::capture_slot(imm)))
        {
            *binding = r.get(i.a);
            next!(cx, r, following(ip), view)
        }
        let vm = unsafe { vm(cx) };
        let env = unsafe { (*cx).env };
        // A frame run's first depth-0 store finds its slots, tagged when they admit every
        // store (reads leave them untagged: most frame runs store none), then stores through
        // them.
        if depth == 0
            && env.is_deleted()
            && let Some(capture) = lane_frame_capture(cx)
        {
            unsafe { (*cx).capture = capture };
            if let Some(slots) = capture_stores(capture)
                // SAFETY: as for the stores above.
                && let Some(binding) = unsafe { &mut *slots }.dense_plain_mut(usize::from(ImmediateLayout::capture_slot(imm)))
            {
                *binding = r.get(i.a);
                next!(cx, r, following(ip), view)
            }
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let program = unsafe { (*cx).program };
        // A frameless activation's function has no environment of its own: its depth-0
        // capture is the environment it runs in, stored with one lookup.
        if depth == 0
            && !env.is_deleted()
            && vm.direct_store_own_environment(code, program, env, ImmediateLayout::capture_slot(imm), r.get(i.a))
        {
            next!(cx, r, following(ip), view)
        }
        let target = if env.is_deleted() {
            vm.capture_env(unsafe { (*cx).frame }, depth)
        } else {
            vm.environment_capture_env(env, false, depth)
        };
        let stored = target.is_some_and(|target| {
            vm.direct_store_environment(code, program, target, ImmediateLayout::capture_slot(imm), r.get(i.a))
        });
        if !stored {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `CreateRegExpLiteral` (`Vm::regexp_literal`), which allocates and runs no guest code.
    fn lane_regexp_literal<>(cx, r, ip, i, view) {
        let Some(value) = lane_regexp_literal_value(cx, i) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `MakeClosure` in a frame's own lane run (`lane_closure_value`).
    fn lane_make_closure<>(cx, r, ip, i, view) {
        let Some(value) = lane_closure_value(cx, i) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The closure a `MakeClosure` record makes as the general loop does, in a frame's own lane
/// run of a function with no binding sites outside a module's root: its environment is the
/// frame's, promoted once, and making it allocates and runs no guest code. Any other
/// closure takes the general path; a promotion made first is idempotent.
#[inline(never)]
fn lane_closure_value<H: Host>(cx: *mut LaneContext<H>, i: &LaneInstruction) -> Option<Value> {
    lane_closure_value_of(cx, i.imm())
}

/// `lane_closure_value` of function `id`.
#[inline(never)]
fn lane_closure_value_of<H: Host>(cx: *mut LaneContext<H>, id: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !vm.lane_stack.activations.is_empty() {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame) };
    let function = vm.frames[frame].function;
    if function == super::ROOT_FUNCTION_ID
        || !code.function_get(function as usize)?.binding_sites.is_empty()
    {
        return None;
    }
    let env = vm.promote_frame_environment(frame);
    // Promotion moves the frame's captured locals into the environment, so a capture
    // slot pointer found before it is not reused.
    unsafe { (*cx).capture = std::ptr::null() };
    let value = vm.closure(code, id, env).ok();
    note_allocation(vm);
    value
}

/// The object a `CreateRegExpLiteral` record makes in the lane: its site belongs to the
/// running program, the frame's or an activation's (`LaneContext::program`). A pattern that
/// fails to compile raises at its instruction through `Vm::lane_raised` in a frame's own
/// run; inside a lane activation, `None` leaves it to the general path, which raises the
/// same error from the site's cached record.
#[inline(never)]
fn lane_regexp_literal_value<H: Host>(
    cx: *mut LaneContext<H>,
    i: &LaneInstruction,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    let result = vm.regexp_literal_of(code, program, i.imm() as usize);
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            if vm.lane_stack.activations.is_empty() {
                vm.lane_raised = Some(error);
            }
            None
        }
    }
}

script_handler! {
    /// `MakeObject2`: a two-property literal allocates and runs no guest code.
    fn lane_make_object2<>(cx, r, ip, i, view) {
        let value = lane_object_pair(cx, r, i);
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `MakeObjectLiteral`: an ordinary object of the literal site's shape holding the
    /// record's window of registers, as the general loop makes it.
    fn lane_make_object_literal<>(cx, r, ip, i, view) {
        let value = lane_object_literal_value(cx, r, i);
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The object a `MakeObjectLiteral` record makes: its site's shape over the record's window
/// of property values (`Vm::object_site_shape`).
#[inline(always)]
fn lane_object_literal_value<H: Host>(cx: *mut LaneContext<H>, r: Registers, i: &LaneInstruction) -> Value {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let shape = vm.object_site_shape(unsafe { &*(*cx).code }, i.imm() as usize);
    // SAFETY: `lane_fields_fit` admitted the window inside the register file, and no
    // other reference into it is live while the object is allocated.
    let properties = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.b)), usize::from(i.c)) };
    let value = vm.heap.alloc_object_with_properties(vm.object_proto, shape, properties);
    note_allocation(vm);
    value
}

script_handler! {
    /// `MakeObject`: an empty ordinary object (`Vm::object`), as the general loop makes it.
    fn lane_make_object<>(cx, r, ip, i, view) {
        r.set(i.a, lane_new_object(unsafe { vm(cx) }));
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

lane_handler! {
    /// `MakeArray`: a fresh array of the record's length in holes (`Vm::hole_array`).
    fn lane_make_array<>(cx, r, ip, i, view) {
        r.set(i.a, lane_new_hole_array(unsafe { vm(cx) }, i.imm() as usize));
        next!(cx, r, following(ip), view)
    }
}

/// `lane_make_object`'s object. Out of line: an allocation whose record space is full falls
/// back to a heap slot, which takes the new object by its stack address; a handler holding
/// that temporary could not dispatch as a sibling call (`tools/check-lane-tail-calls.sh`).
#[inline(never)]
fn lane_new_object<H: Host>(vm: &mut Vm<H>) -> Value {
    let value = vm.object();
    note_allocation(vm);
    value
}

/// `lane_make_array`'s array of `length` holes, out of line as `lane_new_object`.
#[inline(never)]
fn lane_new_hole_array<H: Host>(vm: &mut Vm<H>, length: usize) -> Value {
    let value = vm.hole_array(length);
    note_allocation(vm);
    value
}

/// The object a `MakeObject2` record makes (`Vm::object_pair`).
#[inline(always)]
fn lane_object_pair<H: Host>(cx: *mut LaneContext<H>, r: Registers, i: &LaneInstruction) -> Value {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.object_pair(unsafe { &*(*cx).code }, i.imm() as usize, r.get(i.b), r.get(i.c));
    note_allocation(vm);
    value
}

script_handler! {
    /// `SuperConstArrayObject2` whose two fused operations see primitives only: its parts
    /// as `Vm::execute_const_array_object2` runs them; any other operand takes the
    /// general path, which runs the whole record again.
    fn lane_const_array_object2<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_literal_value(cx, r, i) }) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// A `SuperConstArrayObject2` record's object: a copy of the constant array, the two
/// fused `Binary` results, and the literal holding them, with each part's result in its
/// register as the general path leaves it. `None` before any guest code could run: an
/// operation on an object, or one that throws.
///
/// Out of line: its general binary result returns through a stack slot, which would cost a
/// handler inlining it the tail call to the next record.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(never)]
unsafe fn lane_literal_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let parts = &code.superinstructions.get(i.imm() as usize)?.wide;
    let [array, first, second, object] = [parts[0], parts[1], parts[2], parts[3]];
    let start = array.constant_index();
    let elements = vm.programs.const_array(
        unsafe { (*cx).program },
        start,
        usize::from(array.element_count()),
    )?;
    let array_value = vm.heap.alloc(Cell::Array {
        object: crate::heap::ArrayObject::new(Vm::<H>::empty_object(vm.array_proto), elements),
    });
    r.set(array.result_register(), array_value);
    for part in [first, second] {
        let left = lane_operand_value(cx, r, part.operand_b().0)?;
        let right = lane_operand_value(cx, r, part.operand_c().0)?;
        if vm.is_object_like(left) || vm.is_object_like(right) {
            return None;
        }
        let value = vm.binary(code, part.binary_operator(), left, right).ok()?;
        r.set(part.result_register(), value);
    }
    let value = vm.object_pair(
        code,
        object.object_site_index(),
        r.get(object.register_b()),
        r.get(object.register_c()),
    );
    note_allocation(vm);
    Some(value)
}

/// The object a non-nested field record's base names: a register, or `this` where the lane
/// knows it. `None` for a nested site, whose record names no single base.
///
/// # Safety
/// As for `lane_field_value`.
#[inline(always)]
unsafe fn lane_single_field_base<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    base: u16,
) -> Option<Value> {
    if crate::bytecode::FieldBase(base).local_index().is_some() {
        return None;
    }
    if base == crate::bytecode::FieldBase::NESTED {
        return None;
    }
    match crate::bytecode::FieldBase(base).register_index() {
        Some(register) => Some(r.get(register)),
        None => unsafe { lane_fixed_this(cx) },
    }
}

/// `this` where the lane holds it or the frame fixed it; `None` for a lexical `this`, which
/// the general lookup (`lane_this`) resolves with a call. Without that call, a handler's
/// fast path keeps its values in registers no callee preserves.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn lane_fixed_this<H: Host>(cx: *mut LaneContext<H>) -> Option<Value> {
    let this = unsafe { (*cx).this };
    if !this.is_deleted() {
        return Some(this);
    }
    let frame = (unsafe { vm(cx) }).frames.get(unsafe { (*cx).frame })?;
    frame.fixed_this.then_some(frame.this)
}

/// `this` as the context holds it, for a record whose cached hit answers only an object:
/// `DELETED` where the frame's binding decides it (`LaneContext::this`), which no cache entry
/// answers, so the record declines to its general form, which resolves the binding
/// (`lane_fixed_this`, `lane_this`). One load, with no test of its own.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn lane_held_this<H: Host>(cx: *mut LaneContext<H>) -> Value {
    unsafe { (*cx).this }
}

/// The base of a single-base record whose handler was chosen for it (`single_base_handler`):
/// `this` when `THIS`, else the register the field names.
///
/// # Safety
/// As for `lane_single_field_base`; the record's base is `this` exactly when `THIS`.
#[inline(always)]
unsafe fn lane_known_field_base<H: Host, const THIS: bool>(
    cx: *mut LaneContext<H>,
    r: Registers,
    base: u16,
) -> Option<Value> {
    if THIS {
        debug_assert_eq!(base, crate::bytecode::FieldBase::THIS.0);
        // Every caller reads a cached object field of it, which declines `DELETED`.
        Some(unsafe { lane_held_this(cx) })
    } else {
        debug_assert!(base < crate::bytecode::FieldBase::NESTED);
        Some(r.get(base))
    }
}

/// The instantiation of a single-base read handler that a record of base `base` takes: the
/// one for `this` or the one for a register. A nested record, which names no single base,
/// takes `nested`.
fn single_base_handler(base: u16, this: Handler, register: Handler, nested: Handler) -> Handler {
    match base {
        base if base == crate::bytecode::FieldBase::THIS.0 => this,
        base if crate::bytecode::FieldBase(base).register_index().is_some() => register,
        // A nested record names no single base, and a local base (`FieldBase::local`) is no
        // register the single-base handlers could read.
        _ => nested,
    }
}

/// Whether a field record's base is one the single-base handlers read
/// (`lane_known_field_base`): `this` or a register, not a nested chain or a local slot.
fn is_single_field_base(base: u16) -> bool {
    base == crate::bytecode::FieldBase::THIS.0 || crate::bytecode::FieldBase(base).register_index().is_some()
}

script_handler! {
    /// `GetField` of a register or `this`: the site's primary own entry inline. Any other
    /// read goes through `lane_get_field_miss`, which may move the record to a fitting
    /// handler.
    fn lane_get_field_own<THIS: bool>(cx, r, ip, i, view) {
        // The record's own hint first (`LaneInstruction::field_hint`), which reads no entry.
        let Some(value) = (unsafe { lane_known_field_base::<H, THIS>(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).record_field_entry_hit(i.field_hint(), object))
        else {
            sibling!(lane_get_field_own_refresh::<H, THIS>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_own` past its record's hint: the site's primary own entry, whose hit
    /// becomes the hint; any other read goes through `lane_get_field_miss`.
    #[inline(never)]
    fn lane_get_field_own_refresh<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let Some(value) = (unsafe { lane_known_field_base::<H, THIS>(cx, r, i.b) })
            .and_then(|object| vm.primary_own_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        // A held loop header's `call_link` names its site (`lane_header_site`), and such a
        // record keeps no hint: an empty hint's link is `NO_CALL_LINK`, a held one's shape a
        // recorded one.
        let header = unsafe { (*ip).operand.field.shape } == super::NO_FIELD_RECEIVER && i.call_link.get() != NO_CALL_LINK;
        if !header {
            // SAFETY: as `requicken`; the hit proved the entry an own one of the receiver's
            // shape.
            let entry = vm.field_cache_entry(i.c);
            unsafe { (*ip.cast_mut()).operand.field.shape = entry.receiver };
            i.call_link.set(entry.slot);
        }
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `GetField` of a register or `this` at a site past one shape
    /// (`Vm::field_site_polymorphic`): its megamorphic entries; any other read takes
    /// `lane_get_field`.
    fn lane_get_field_polymorphic<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { lane_known_field_base::<H, THIS>(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).inline_polymorphic_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `GetField` of a register or `this` at a site whose primary entry names a prototype
    /// holder (`Vm::field_site_holder`), a method read: that entry inline; any other read
    /// takes `lane_get_field_miss`, which moves a site gone polymorphic.
    fn lane_get_field_holder<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { lane_known_field_base::<H, THIS>(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).primary_record_holder_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field_holder_any::<H, THIS>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_holder` past a record prototype (`Vm::primary_holder_field_hit`), out
    /// of line so the record read makes no call: any other read takes `lane_get_field_miss`.
    #[inline(never)]
    fn lane_get_field_holder_any<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { lane_known_field_base::<H, THIS>(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).primary_holder_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_own` for a receiver in a heap cell rather than the object records
    /// (`Vm::cell_own_field_hit`): a function's own field, as `Strength.REQUIRED`. A miss
    /// moves the record on as `lane_get_field_miss` decides.
    fn lane_get_field_own_cell<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { lane_single_field_base(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).cell_own_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `GetField` of a string primitive, which `String.prototype` answers through the
    /// site's cache (`Vm::string_prototype_field_hit`), a method read as `s.substring`, or
    /// its chain's data walk for an absent name, at a site `Vm::string_field_site_cacheable`
    /// admits. A miss moves the record on as `lane_get_field_miss` decides.
    fn lane_get_field_string<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // The site checks (`Vm::string_field_site_cacheable`) ran when the handler was
        // installed, so a hit reads the site's `String.prototype` entry directly; any other
        // read is `Vm::cached_string_field_hit`'s, whose miss records the site's entry.
        let Some(value) = (unsafe { lane_single_field_base(cx, r, i.b) })
            .filter(|object| vm.heap.is_string(*object))
            .and_then(|object| {
                vm.string_prototype_field_hit(i.c)
                    .or_else(|| vm.cached_string_field_hit(unsafe { &*(*cx).code }, object, i.imm(), i.c))
            })
        else {
            sibling!(lane_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_string` of a register base whose `String.prototype` entry answers
    /// (`Vm::string_prototype_field_hit`): one tag test and the entry's shape before the
    /// slot. Any other read takes `lane_get_field_string` out of line.
    fn lane_get_field_string_register<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let base = r.get(i.b);
        let Some(value) = vm.heap.is_string(base).then(|| vm.string_prototype_field_hit(i.c)).flatten() else {
            sibling!(lane_get_field_string::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_holder` for a receiver in a heap cell rather than the object records
    /// (`Vm::cell_holder_field_hit`): a method read from a function, as `f.apply`. A miss
    /// moves the record on as `lane_get_field_miss` decides.
    fn lane_get_field_holder_cell<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { lane_single_field_base(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).cell_holder_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_holder_cell` of a register base: the base read from its register, and
    /// a miss taken out of line by `lane_get_field_holder_cell`, which moves the record on.
    fn lane_get_field_holder_cell_register<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { vm(cx) }).cell_holder_field_hit(code, r.get(i.b), i.c) else {
            sibling!(lane_get_field_holder_cell::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `GetField` of a register or `this` at a site with an inherited entry
    /// (`Vm::field_site_inherited`), a method read beyond the immediate prototype: that entry
    /// inline while its proof holds (`Vm::proved_inherited_field_hit`); any other read takes
    /// `lane_get_field_inherited_walk`.
    fn lane_get_field_inherited<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { lane_known_field_base::<H, THIS>(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).proved_inherited_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field_inherited_walk::<H, THIS>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_inherited` past its proof: the entry's chain walk, which proves it
    /// again (`Vm::inherited_field_hit`); any other read takes `lane_get_field_miss`.
    #[inline(never)]
    fn lane_get_field_inherited_walk<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = (unsafe { lane_known_field_base::<H, THIS>(cx, r, i.b) })
            .and_then(|object| (unsafe { vm(cx) }).inherited_field_hit(code, object, i.c))
        else {
            sibling!(lane_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A `GetField`'s first run: a read of `name` or `length` can enter prototype lookup
    /// mode (`Vm::reads_function_info`), which only `lane_get_field`'s general probe
    /// prepares, so such a site keeps it; any other takes `lane_get_field_own`.
    #[inline(never)]
    fn lane_get_field_first<>(cx, r, ip, i, view) {
        if crate::bytecode::FieldBase(i.b).local_index().is_some() {
            sibling!(lane_get_field::<H>, r, ip, Carried::script(cx))
        }
        let vm = unsafe { vm(cx) };
        if vm.is_private_name(i.imm()) && i.b != crate::bytecode::FieldBase::NESTED {
            requicken::<H>(ip, lane_get_private_field::<H>);
            sibling!(lane_get_private_field::<H>, r, ip, Carried::script(cx))
        }
        if vm.reads_function_info(i.imm()) {
            // A register's `length` is most often an array's, which no function information
            // can answer.
            let handler: Handler = if i.imm() == vm.length_atom
                && crate::bytecode::FieldBase(i.b).register_index().is_some()
                && i.b != crate::bytecode::FieldBase::NESTED
            {
                lane_get_length::<H>
            } else {
                lane_get_field::<H>
            };
            requicken::<H>(ip, handler);
            sibling!(handler, r, ip, Carried::script(cx))
        }
        let own = single_base_handler(
            i.b,
            lane_get_field_own::<H, true>,
            lane_get_field_own::<H, false>,
            lane_get_field_miss::<H>,
        );
        requicken::<H>(ip, own);
        sibling!(own, r, ip, Carried::script(cx))
    }
}

script_handler! {
    /// `GetField` of `length` from a register (`lane_get_field_first`): an array's own
    /// `length` (`Vm::array_length_field`, which `lane_get_field` answers for an array
    /// after a probe that cannot hit) inline; any other receiver takes `lane_get_field`.
    fn lane_get_length<>(cx, r, ip, i, view) {
        let Some(length) = (unsafe { vm(cx) }).own_array_length(r.get(i.b)) else {
            sibling!(lane_get_length_other::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, Value::length(length));
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_length` of a receiver other than an array, which the general cached read
    /// does not answer: a string primitive's length (`Vm::string_length`) here, and any
    /// other receiver past `lane_get_length_arguments`.
    #[inline(never)]
    fn lane_get_length_other<>(cx, r, ip, i, view) {
        let Some(length) = (unsafe { vm(cx) }).string_length(r.get(i.b)) else {
            sibling!(lane_get_length_arguments::<H>, r, ip, Carried::script(cx))
        };
        // A site that reads a string's length mostly reads strings, as a scan over one does.
        requicken::<H>(ip, lane_get_length_string::<H>);
        r.set(i.a, Value::length(length));
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_length` at a site that read a string's length (`lane_get_length_other`):
    /// a string's length here; any other receiver, an array's included, takes
    /// `lane_get_length` out of line, which keeps this one's registers to the string.
    fn lane_get_length_string<>(cx, r, ip, i, view) {
        let Some(length) = (unsafe { vm(cx) }).string_length(r.get(i.b)) else {
            sibling!(lane_get_length::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, Value::length(length));
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_length_other` of a receiver other than a string: an arguments object's own
    /// data `length` in its creation layout (`Vm::arguments_object_length`), or a user
    /// function's own data `length` (`Vm::plain_function_length`), as a middleware stack
    /// reads a handler's arity. Any other receiver takes `lane_get_field`.
    #[inline(never)]
    fn lane_get_length_arguments<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let receiver = r.get(i.b);
        let Some(length) = vm.arguments_object_length(receiver).or_else(|| vm.plain_function_length(receiver)) else {
            sibling!(lane_get_field::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, length);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A miss of a single-entry read (`lane_get_field_own`, `_holder`, `_inherited`): a nested record moves to
    /// `lane_get_field`, one whose site went polymorphic to `lane_get_field_polymorphic`,
    /// one whose primary entry names a prototype holder to `lane_get_field_holder`, one with
    /// an inherited entry to `lane_get_field_inherited`; the read itself takes
    /// `lane_get_field`.
    #[inline(never)]
    fn lane_get_field_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let base = unsafe { lane_single_field_base(cx, r, i.b) };
        let string_base = base.is_some_and(|base| {
            (unsafe { vm(cx) }).heap.is_string(base)
        });
        let handler: Option<Handler> = match lane_field_site_tier(unsafe { vm(cx) }, code, i) {
            // A string primitive reads `String.prototype`, whatever tier its entries name,
            // where the site admits that read; any other read of one is the general read's.
            _ if string_base && (unsafe { vm(cx) }).string_field_site_cacheable(code, i.imm()) => {
                Some(single_base_handler(
                    i.b,
                    lane_get_field_string::<H>,
                    lane_get_field_string_register::<H>,
                    lane_get_field_string::<H>,
                ))
            }
            _ if string_base => Some(lane_get_field::<H>),
            FieldSiteTier::Nested => Some(lane_get_field_nested::<H>),
            FieldSiteTier::General => Some(lane_get_field::<H>),
            FieldSiteTier::Getter => Some(lane_get_field_getter::<H>),
            FieldSiteTier::Polymorphic => Some(single_base_handler(
                i.b,
                lane_get_field_polymorphic::<H, true>,
                lane_get_field_polymorphic::<H, false>,
                lane_get_field::<H>,
            )),
            // A receiver outside the object records, such as a function, takes the cell lookup.
            FieldSiteTier::Holder
                if base.is_some_and(|object| object.direct_object_index().is_none()) =>
            {
                Some(if crate::bytecode::FieldBase(i.b).register_index().is_some() {
                    lane_get_field_holder_cell_register::<H>
                } else {
                    lane_get_field_holder_cell::<H>
                })
            }
            FieldSiteTier::Holder => Some(single_base_handler(
                i.b,
                lane_get_field_holder::<H, true>,
                lane_get_field_holder::<H, false>,
                lane_get_field::<H>,
            )),
            FieldSiteTier::Inherited => Some(single_base_handler(
                i.b,
                lane_get_field_inherited::<H, true>,
                lane_get_field_inherited::<H, false>,
                lane_get_field::<H>,
            )),
            FieldSiteTier::Own
                if base.is_some_and(|object| object.direct_object_index().is_none()) =>
            {
                Some(lane_get_field_own_cell::<H>)
            }
            FieldSiteTier::Own => None,
        };
        if let Some(handler) = handler {
            requicken::<H>(ip, handler);
        }
        sibling!(lane_get_field::<H>, r, ip, Carried::script(cx))
    }
}

/// The cache tier a single-read `GetField` site's entries call for, to which a miss of its
/// single-entry handler moves the record (`lane_get_field_miss`, `lane_returning_field_miss`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldSiteTier {
    /// A nested read whose steps hit primary own entries, or an array's own `length`
    /// (`lane_nested_own_value`).
    Nested,
    /// A nested read with another step, which only the general cached read makes.
    General,
    /// A read that met an accessor the site cached (`Vm::field_site_getter`).
    Getter,
    /// Past one shape (`Vm::field_site_polymorphic`).
    Polymorphic,
    /// A primary entry naming a prototype holder (`Vm::field_site_holder`).
    Holder,
    /// An entry beyond the immediate prototype (`Vm::field_site_inherited`).
    Inherited,
    /// The primary own entry, which the record already reads.
    Own,
}

fn lane_field_site_tier<H: Host>(
    vm: &Vm<H>,
    code: &ResidualProgram,
    i: &LaneInstruction,
) -> FieldSiteTier {
    if i.b == crate::bytecode::FieldBase::NESTED {
        // A step that reads `name` or `length` may prepare prototype lookup
        // (`Vm::reads_function_info`); only an array's own `length` needs no preparing.
        let nested_own = code.field_sites.get(i.imm() as usize).is_some_and(|site| {
            !vm.reads_function_info(site.first.0)
                && site.second.is_none_or(|(atom, _)| {
                    atom == vm.length_atom || !vm.reads_function_info(atom)
                })
        });
        if nested_own { FieldSiteTier::Nested } else { FieldSiteTier::General }
    } else if vm.field_site_getter(i.c) {
        FieldSiteTier::Getter
    } else if vm.field_site_polymorphic(i.c) {
        FieldSiteTier::Polymorphic
    } else if vm.field_site_holder(i.c) {
        FieldSiteTier::Holder
    } else if vm.field_site_inherited(i.c) {
        FieldSiteTier::Inherited
    } else {
        FieldSiteTier::Own
    }
}

/// A nested `GetField`'s value (`this.a.b`, `r.a.b`) where each step hits its site's primary
/// own entry (`Vm::primary_own_field_hit`), or the second is an array's own `length`
/// (`Vm::array_length_field`); `None` leaves the read to the general cached read. The site
/// reads neither `name` nor another `length` (`lane_field_site_tier`).
///
/// # Safety
/// `cx` is the running lane's context and `i` a nested record of it.
#[inline(always)]
unsafe fn lane_nested_own_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run; a nested record names its site.
    let code = unsafe { &*(*cx).code };
    let site = unsafe { code.field_sites.get_unchecked(i.imm() as usize) };
    let vm = unsafe { vm(cx) };
    let base = match site.base.register_index() {
        Some(register) => r.get(register),
        None => unsafe { lane_fixed_this(cx) }?,
    };
    nested_own_field_hit(vm, code, site, base)
}

/// A nested site's read of `base` where both steps hit primary own entries
/// (`lane_nested_own_value`), which runs no guest code.
#[inline(always)]
fn nested_own_field_hit<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    site: &crate::bytecode::FieldSite,
    base: Value,
) -> Option<Value> {
    let first = vm.primary_own_field_hit(code, base, site.first.1)?;
    match site.second {
        None => Some(first),
        Some((atom, _)) if atom == vm.length_atom => vm.array_length_field(first, atom),
        Some((_, cache)) => vm.primary_own_field_hit(code, first, cache),
    }
}

script_handler! {
    /// A nested `GetField` whose steps hit primary own entries (`lane_nested_own_value`);
    /// any other read takes `lane_get_field`.
    fn lane_get_field_nested<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_nested_own_value(cx, r, i) }) else {
            sibling!(lane_get_field::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A field read that hits its cache; any other read takes the general
    /// path, which repeats the side-effect-free lookup.
    #[inline(never)]
    fn lane_get_field<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_field_value(cx, r, i) }) else {
            if lane_field_read_beside_activations(cx, r, i).is_some() {
                sibling!(lane_get_field_beside_activations::<H>, r, ip, Carried::script(cx))
            }
            return match lane_getter_activation(cx, r, ip, i) {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                Err(next) => next,
            };
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field` at a site whose reads met a getter (`FieldSiteTier::Getter`): the
    /// getter activation first, whose cache check fails without effect, so a data read the
    /// cached probes answer stays `lane_get_field`'s. An activation without lane-stack room
    /// makes the activations frames first (`lane_script_call_frame`), so the read goes on with
    /// the running function's registers read again (the rollup npm scenario's
    /// `childNodeKeys[this.type]` was written to the emptied lane stack, not its frame).
    #[inline(never)]
    fn lane_get_field_getter<>(cx, r, ip, i, view) {
        match lane_getter_activation(cx, r, ip, i) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(_) => {
                let r = unsafe { (*cx).registers };
                sibling!(lane_get_field::<H>, r, ip, Carried::script(cx))
            }
        }
    }
}

/// The base of a `GetField` of a register or `this` that a frameless activation makes in
/// place (`lane_get_field_beside_activations`): a public name whose site caches no getter,
/// which `lane_getter_activation` would run as an activation. `None` for a frame's run, a
/// nested or local base, a private name or such a getter.
#[inline(never)]
fn lane_field_read_beside_activations<H: Host>(cx: *mut LaneContext<H>, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.activations.is_empty() || i.b == crate::bytecode::FieldBase::NESTED || vm.is_private_name(i.imm()) {
        return None;
    }
    let base = unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) }?;
    vm.cached_field_getter(base, i.imm(), vm.field_cache_index(i.c)).is_none().then_some(base)
}

script_handler! {
    /// `lane_get_field` past its caches within a frameless activation
    /// (`lane_field_read_beside_activations`): the general read made in place
    /// (`lane_in_place`), so an uncached getter, a proxy or a throw makes the activations
    /// frames only when it runs, instead of leaving the lane, which makes them all frames.
    #[inline(never)]
    fn lane_get_field_beside_activations<>(cx, r, ip, i, view) {
        let Some(base) = lane_field_read_beside_activations(cx, r, i) else {
            return exit(ip);
        };
        let (atom, site) = (i.imm(), i.c);
        let Some(value) = lane_in_place(cx, ip, |vm, code| vm.get_field_cached(code, base, atom, site)) else {
            return exit(ip);
        };
        // Guest code the read ran made the activations frames, whose registers these are.
        let r = unsafe { (*cx).registers };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// A lane `GetField` whose read meets the accessor its site cached
/// (`Vm::cached_field_getter`): the getter runs as a frameless activation of the read's
/// record, as `lane_script_call` runs a call, with the base as receiver and no arguments,
/// and its completion is the read's value. `Err` carries the record to continue at: the
/// general path for any other read.
#[inline(never)]
fn lane_getter_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Ip> {
    if !unsafe { (*cx).script_calls } {
        #[cfg(feature = "profile-lane-exits")]
        super::lane_exits::record_detail("getter decline script_calls".into());
        return Err(exit(ip));
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The object the getter's read is of and its site: a nested site's second field is read
    // of the value its first, cached read gives.
    let read = if i.b == crate::bytecode::FieldBase::NESTED {
        code.field_sites.get(i.imm() as usize).copied().and_then(|site| {
            let base = unsafe { lane_field_base(cx, r, site.base) }?;
            match site.second {
                Some((atom, cache)) => {
                    Some((lane_cached_field(cx, code, base, site.first.0, site.first.1)?, atom, cache))
                }
                None => Some((base, site.first.0, site.first.1)),
            }
        })
    } else {
        unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) }.map(|base| (base, i.imm(), i.c))
    };
    let vm = unsafe { vm(cx) };
    #[cfg(feature = "profile-lane-exits")]
    if let Some((base, atom, cache)) = read
        && vm.cached_field_getter(base, atom, vm.field_cache_index(cache)).is_none()
    {
        super::lane_exits::record_detail(format!(
            "getter decline uncached {} recorded={}",
            &code.atoms[atom as usize],
            vm.field_site_getter(cache)
        ));
    } else if read.is_none() {
        super::lane_exits::record_detail("getter decline base".into());
    }
    let Some((base, getter)) = read.and_then(|(base, atom, cache)| {
        Some((base, vm.cached_field_getter(base, atom, vm.field_cache_index(cache))?))
    }) else {
        return Err(exit(ip));
    };
    lane_activate_getter(cx, r, ip, getter, base)
}

/// The activation of `getter` with receiver `base` and no arguments for the read record at
/// `ip`, whose completion is the read's value: a frameless target of the running program, or
/// another program's plain activation (`Vm::cross_call_entry`), which runs in its program.
/// `Err` carries the record to continue at: the general path for any other getter.
fn lane_activate_getter<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    getter: Value,
    base: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    let no_arguments = ActivationArguments::Kept(std::ptr::NonNull::dangling().as_ptr(), 0);
    if let Some(target) = lane_script_target(cx, getter).filter(|target| !target.forwards()) {
        let Some(this) = lane_call_this(vm, &target, base) else {
            return Err(exit(ip));
        };
        return lane_activate_keeping::<H, false>(cx, r, ip, &target, no_arguments, this, false, None);
    }
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let Some(target) = vm
        .cross_call_entry(getter)
        .map(|target| unsafe { *target })
        .filter(|target| target.entry == CallEntry::Activates && !lane_constructing(vm, getter))
    else {
        #[cfg(feature = "profile-lane-exits")]
        super::lane_exits::record_detail(format!(
            "getter decline target cross={:?} constructing={}",
            vm.cross_call_entry(getter).map(|target| unsafe { (*target).entry } as u8),
            lane_constructing(vm, getter)
        ));
        return Err(exit(ip));
    };
    let Some(this) = lane_call_this(vm, &target, base) else {
        return Err(exit(ip));
    };
    lane_activate_keeping::<H, true>(cx, r, ip, &target, no_arguments, this, false, None)
}

/// A `GetField` record's value where caches answer it (`lane_cached_field`): one read of a
/// register or `this`, or a nested site's chain of two. `None` leaves the read to the
/// general path.
///
/// Out of line: the generic reads (`lane_get_field`, `lane_get_field_sink`, a returning
/// `GetField`) run it on their slower paths, so one copy serves them all instead of each
/// carrying its own probe (DeltaBlue, PGO build: ~40 instruction-cache lines of warm code).
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(never)]
unsafe fn lane_field_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if i.b == crate::bytecode::FieldBase::NESTED {
        let site = *code.field_sites.get(i.imm() as usize)?;
        let base = unsafe { lane_field_base(cx, r, site.base) }?;
        let value = lane_cached_field(cx, code, base, site.first.0, site.first.1)?;
        return match site.second {
            Some((atom, cache)) => lane_cached_field(cx, code, value, atom, cache),
            None => Some(value),
        };
    }
    let base = unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) }?;
    lane_cached_field(cx, code, base, i.imm(), i.c)
}

/// The object a field base names: a register, or `this` where the lane knows it.
///
/// # Safety
/// As for `lane_field_value`.
#[inline(always)]
unsafe fn lane_field_base<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    base: crate::bytecode::FieldBase,
) -> Option<Value> {
    if let Some(slot) = base.local_index() {
        return lane_plain_local_operand(cx, slot);
    }
    match base.register_index() {
        Some(register) => Some(r.get(register)),
        None => unsafe { lane_this(cx) },
    }
}

/// A field read the caches answer: the general loop's probe (`cached_field_hit`, own,
/// megamorphic and holder entries, or an array's `length`), then the data lookup its slower
/// paths make before any that runs guest code (`Vm::cached_field_polymorphic_hit`).
#[inline(always)]
fn lane_cached_field<H: Host>(
    cx: *mut LaneContext<H>,
    code: &ResidualProgram,
    object: Value,
    atom: crate::bytecode::Atom,
    site: u16,
) -> Option<Value> {
    let probe = {
        let vm = unsafe { vm(cx) };
        // A typed array's geometry, which no field cache records (`AtomClass::VIRTUAL_FIELD`).
        if vm.atom_class(atom).contains(super::object::AtomClass::VIRTUAL_FIELD)
            && let Some(value) = vm.typed_array_geometry_read(object, atom)
        {
            return Some(value);
        }
        vm.prepare_named_function_info_lookup(object, atom);
        vm.cached_field_hit(code, object, site)
    };
    match probe {
        super::object::FieldCacheProbe::Hit(value) => Some(value),
        // An accessor's entry answers before the data lookups, which would walk the chain to
        // reach the same accessor and decline it.
        probe => unsafe { lane_getter_read(cx, object, atom, site) }.or_else(|| {
            // SAFETY: the context outlives the lane run.
            lane_cached_field_miss(unsafe { vm(cx) }, code, object, atom, site, probe, unsafe { (*cx).env })
        }),
    }
}

/// A read whose site caches an accessor (`Vm::cached_field_getter`) whose getter is a pure
/// native read (`Vm::pure_native_getter_value`) or a frameless read target, answered as the
/// getter's call would return (`lane_call_read`): `[[Get]]` calls it on the base with no
/// arguments, and neither runs guest code. `None` leaves the read to the general path, which
/// calls the getter.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(never)]
unsafe fn lane_getter_read<H: Host>(
    cx: *mut LaneContext<H>,
    object: Value,
    atom: crate::bytecode::Atom,
    site: u16,
) -> Option<Value> {
    let getter = {
        let vm = unsafe { vm(cx) };
        vm.cached_field_getter(object, atom, vm.field_cache_index(site))?
    };
    if let Some(value) = (unsafe { vm(cx) }).pure_native_getter_value(getter, object) {
        return Some(value);
    }
    let target = lane_script_target(cx, getter)?;
    if target.entry == CallEntry::Reads {
        // A getter only the general path has called has not run its read in the lane yet: it
        // takes the kind its first run would.
        // SAFETY: as `lane_call_read`; the context's program outlives the lane run.
        let (entry, code) = unsafe { ((*target.view).records(), &*(*cx).code) };
        if unsafe { (*entry).handler } == lane_returning_field_first::<H> as *const () {
            requicken::<H>(entry, returning_field_first_kind(unsafe { vm(cx) }, code, unsafe { &*entry }));
        }
    } else if !target.entry.reads() && target.entry != CallEntry::ForwardsRead {
        return None;
    }
    let this = lane_direct_call_this(unsafe { vm(cx) }, &target, object)?;
    unsafe { lane_call_read(cx, &target, ReadArguments::NONE, this) }
}

/// `lane_cached_field` past its probe. Out of line so that no handler inlining the probe
/// keeps the lookups' stack locals, which would cost it the tail call to the next record.
#[inline(never)]
fn lane_cached_field_miss<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    object: Value,
    atom: crate::bytecode::Atom,
    site: u16,
    probe: super::object::FieldCacheProbe,
    environment: Value,
) -> Option<Value> {
    // A private field reads from the object's private elements before any cache, as the
    // general loop's `GetField` does. Its home follows the running function and the
    // environment its frame would capture from: the lane's frame, or a frameless activation's
    // function and environment, whose frame would hold that environment as its closure's.
    if vm.is_private_name(atom) {
        return match vm.lane_stack.activations.last() {
            None => vm.private_field_value(code, object, atom),
            Some(activation) => {
                let function = activation.script().map(|_| activation.function)?;
                vm.memoized_private_field_value(code, environment, function, object, atom)
            }
        };
    }
    match probe {
        super::object::FieldCacheProbe::Hit(value) => Some(value),
        // The order `Vm::get_field_cached` takes: an array's `length` before the lookups.
        super::object::FieldCacheProbe::OrdinaryMiss => vm
            .array_length_field(object, atom)
            .or_else(|| vm.cached_field_polymorphic_hit(code, object, atom, site)),
        super::object::FieldCacheProbe::Ineligible => vm
            .array_length_field(object, atom)
            .or_else(|| vm.cached_string_field_hit(code, object, atom, site)),
    }
    // A collection's `size` or a buffer view's field, which no cache records
    // (`AtomClass::VIRTUAL_FIELD`).
    .or_else(|| vm.pure_virtual_field(object, atom))
}

script_handler! {
    /// A sinking `GetField`'s first run: a site that reads `name` or `length`
    /// (`Vm::reads_function_info`) keeps `lane_get_field_sink`, whose reads prepare them;
    /// any other takes `lane_get_field_sink_own`.
    #[inline(never)]
    fn lane_get_field_sink_first<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let own = i.b == crate::bytecode::FieldBase::NESTED
            && code.field_sites.get(i.imm() as usize).is_some_and(|site| {
                !vm.reads_function_info(site.first.0)
                    && site.second.is_none_or(|(atom, _)| !vm.reads_function_info(atom))
            });
        if own {
            requicken::<H>(ip, lane_get_field_sink_own::<H>);
            sibling!(lane_get_field_sink_own::<H>, r, ip, Carried::script(cx))
        }
        requicken::<H>(ip, lane_get_field_sink::<H>);
        sibling!(lane_get_field_sink::<H>, r, ip, Carried::script(cx))
    }
}

script_handler! {
    /// `lane_get_field_sink` where every step hits its site's primary own entry: the
    /// nested read (`Vm::primary_own_field_hit`), then the store to `this`
    /// (`Vm::primary_own_field_store_hit`) where the write barrier logs nothing. Any other
    /// run takes `lane_get_field_sink`, which repeats the side-effect-free reads.
    fn lane_get_field_sink_own<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        // SAFETY: `lane_get_field_sink_first` installs this handler only on a nested site.
        let site = unsafe { code.field_sites.get_unchecked(i.imm() as usize) };
        let base = match site.base.register_index() {
            Some(register) => Some(r.get(register)),
            None => Some(unsafe { lane_held_this(cx) }),
        };
        let value = base
            .and_then(|base| vm.primary_own_field_hit(code, base, site.first.1))
            .and_then(|value| match site.second {
                Some((_, cache)) => vm.primary_own_field_hit(code, value, cache),
                None => Some(value),
            });
        let stored = match (value, site.sink, unsafe { lane_held_this(cx) }) {
            (Some(value), Some((atom, cache)), this) => {
                vm.primary_own_field_store_hit::<false>(code, this, atom, value, cache)
            }
            _ => false,
        };
        if !stored {
            sibling!(lane_get_field_sink::<H>, r, ip, Carried::script(cx))
        }
        // SAFETY: a store happened only with a value.
        r.set(i.a & crate::bytecode::REGISTER_MASK, unsafe { value.unwrap_unchecked() });
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A nested `GetField` that also stores its value to a field of `this` (`this.a =
    /// this.b.c`): the read as `lane_get_field` makes it, then the cached write of
    /// `SetThisField`. A read or write the caches do not answer takes the general path,
    /// which repeats the side-effect-free read.
    fn lane_get_field_sink<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let (Some(value), Some(this), Some(sink)) = (
            unsafe { lane_field_value(cx, r, i) },
            unsafe { lane_this(cx) },
            code.field_sites.get(i.imm() as usize).and_then(|site| site.sink),
        ) else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        if !vm.cached_field_write_hit(code, this, sink.0, value, sink.1) {
            return exit(ip);
        }
        // Adding a property can allocate.
        note_allocation(vm);
        r.set(i.a & crate::bytecode::REGISTER_MASK, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// Store through the field cache; any other store takes the general path.
#[inline(always)]
fn lane_set_field<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    object: Value,
    i: &LaneInstruction,
) -> Ip {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    // A private field's store writes the object's private element, as the general loop's
    // `SetField` does, before any cache; the general loop's cached write otherwise: the store
    // inline, the add out of line.
    let private = vm.is_private_name(i.imm());
    let stored = if private {
        lane_private_field_store(cx, ip, code, object, i.imm(), r.get(i.a))
    } else {
        vm.cached_field_write_hit(code, object, i.imm(), r.get(i.a), i.c)
    };
    if !stored {
        if private {
            return exit(ip);
        }
        return lane_set_field_in_place(cx, r, ip, object, i);
    }
    // Adding a property can allocate.
    note_allocation(vm);
    next!(cx, r, following(ip), Carried::script(cx))
}

/// `lane_set_field` of a public name past its caches: the general store made in place
/// (`lane_in_place`), in the frame's own run or beside frameless activations, strict as its
/// record is. A store that adds a property to receivers of many shapes (zod's schema
/// initializers in the playwright npm scenario: 51K of its 79K lane exits) otherwise left the
/// lane at every store.
#[cold]
#[inline(never)]
fn lane_set_field_in_place<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    object: Value,
    i: &LaneInstruction,
) -> Ip {
    let vm = unsafe { vm(cx) };
    let function = match vm.lane_stack.activations.last() {
        Some(activation) => activation.function,
        None => vm.frames[unsafe { (*cx).frame }].function,
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(function as usize);
    let strict = match decoded_instruction(body, body.code[pc]).op() {
        Op::SetFieldStrict | Op::SetThisFieldStrict => true,
        Op::SetField | Op::SetThisField => false,
        _ => return exit(ip),
    };
    let (atom, value, site) = (i.imm(), r.get(i.a), i.c);
    if lane_in_place(cx, ip, |vm, code| vm.set_field_cached(code, object, atom, value, site, strict)).is_none() {
        return exit(ip);
    }
    // Guest code the store ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    next!(cx, r, following(ip), Carried::script(cx))
}

/// The brand of private name `atom` at the record `ip`, whose home follows the running
/// function and the environment its frame would capture from: the lane's frame, or a
/// frameless activation's function and closure environment (`Vm::site_private_brand`).
#[inline(never)]
fn lane_private_brand<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    code: &ResidualProgram,
    atom: crate::bytecode::Atom,
) -> Option<crate::heap::PrivateBrand> {
    let (environment, function, frame) = lane_private_site(cx)?;
    unsafe { vm(cx) }.site_private_brand(code, ip.addr(), environment, function, frame, atom)
}

/// What a lane read of a private name of an object resolves to (`lane_get_private_field`).
enum PrivateRead {
    Value(Value),
    /// A private accessor's getter, called with the object.
    Getter(Value, Value),
}

/// The running function's private-name scope: the environment its frame, or its activation,
/// captures from, the function, and whether it runs in a frame (`Vm::site_private_brand`).
#[inline(always)]
fn lane_private_site<H: Host>(cx: *mut LaneContext<H>) -> Option<(Value, u32, bool)> {
    let vm = unsafe { vm(cx) };
    Some(match vm.lane_stack.activations.last() {
        None => {
            // The running frame is the top one, whose home `Vm::private_name_home` finds.
            let frame = vm.frames.len().checked_sub(1)?;
            (vm.captured_parent_environment(frame), vm.frames[frame].function, true)
        }
        // SAFETY: the context outlives the lane run.
        Some(activation) => (unsafe { (*cx).env }, activation.script().map(|_| activation.function)?, false),
    })
}

/// The store of private field `atom` of `object` at the record `ip`, under its brand there
/// (`lane_private_brand`). `false` leaves the store, a private accessor's or method's
/// included, to the general path.
#[inline(never)]
fn lane_private_field_store<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    code: &ResidualProgram,
    object: Value,
    atom: crate::bytecode::Atom,
    value: Value,
) -> bool {
    let Some(brand) = lane_private_brand(cx, ip, code, atom) else {
        return false;
    };
    (unsafe { vm(cx) }).object_data_mut(object).is_some_and(|data| data.set_private_field(brand, value))
}

/// Leave the lane at `ip` after its instruction failed with `error` before any effect: a frame
/// run raises it there (`Vm::lane_raised`); an activation leaves the instruction to the general
/// path, which runs it again and raises the same error.
#[inline(never)]
fn lane_raise<H: Host>(cx: *mut LaneContext<H>, ip: Ip, error: JsError) -> Ip {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.activations.is_empty() {
        vm.lane_raised = Some(error);
    }
    exit(ip)
}

script_handler! {
    /// `DefinePropertyRecord` (`Vm::define_property_or_throw`): it can run a Proxy trap, so
    /// publish the current PC and materialize frameless activations around the general semantic
    /// operation. Ordinary definitions stay in the lane; abrupt completion resumes at this
    /// exact instruction through `lane_in_place`.
    #[inline(never)]
    fn lane_define_property_record<>(cx, r, ip, i, view) {
        let Some(mode) = crate::bytecode::PropertyDefinitionMode::from_word(i.imm()) else {
            return lane_raise(
                cx,
                ip,
                JsError::validation("invalid property definition mode".into()),
            );
        };
        let target = r.get(i.b);
        let key = r.get(i.c);
        let descriptor = super::object_descriptors::PropertyDescriptorRecord::for_definition(
            mode,
            r.get(i.a),
        );
        let result = lane_in_place(cx, ip, |vm, code| {
            vm.define_property_or_throw(code, target, key, descriptor)
        });
        if result.is_none() {
            return exit(ip);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `DefineField` (`Vm::define_field_cached`), as the general step defines it. A Proxy
    /// receiver's define runs its trap, which a frameless activation leaves to the general path.
    #[inline(never)]
    fn lane_define_field<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let object = r.get(i.b);
        if !vm.lane_stack.activations.is_empty() && vm.proxy_parts(object).is_some() {
            return exit(ip);
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        if let Err(error) = vm.define_field_cached(code, object, i.imm(), r.get(i.a), i.c) {
            return lane_raise(cx, ip, error);
        }
        // Adding a property can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `DefinePrivateField` (`Vm::initialize_private_field`) of the object in `b` under the home
    /// in `c`, which runs no guest code.
    #[inline(never)]
    fn lane_define_private_field<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        if let Err(error) = vm.initialize_private_field(code, r.get(i.b), r.get(i.c), i.imm(), r.get(i.a)) {
            return lane_raise(cx, ip, error);
        }
        // Adding a private element can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `CheckPrivate` of an object holding the private field under the name's brand at this
    /// record (`lane_private_brand`), which proves the check; any other check takes
    /// `lane_general_step`.
    #[inline(never)]
    fn lane_check_private<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let present = lane_private_brand(cx, ip, code, i.imm()).is_some_and(|brand| {
            (unsafe { vm(cx) }).object_data(r.get(i.a)).is_some_and(|data| data.private_field(brand).is_some())
        });
        if !present {
            sibling!(lane_general_step::<H>, r, ip, Carried::script(cx))
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `CopyDataProperties` (an object spread or rest) that a plain copy answers
    /// (`Vm::copy_plain_data_properties`), which runs no guest code and cannot throw, so it
    /// runs beside activations; any other copy takes `lane_general_step`.
    #[inline(never)]
    fn lane_copy_data_properties<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !vm.copy_plain_data_properties(r.get(i.a), r.get(i.b), r.get(i.c)) {
            sibling!(lane_general_step::<H>, r, ip, Carried::script(cx))
        }
        // The copy can allocate property storage.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `PrivateIn` (`#x in object`) of an object with its own data (`Vm::object_data`): whether
    /// it carries the name's brand at this record (`lane_private_brand`), a field's or a
    /// method's. Any other operand, which may raise, takes `lane_general_step`.
    #[inline(never)]
    fn lane_private_in<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let carries = lane_private_brand(cx, ip, code, i.imm()).and_then(|brand| {
            (unsafe { vm(cx) }).object_data(r.get(i.b)).map(|data| data.has_private_name(brand))
        });
        let Some(carries) = carries else {
            sibling!(lane_general_step::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, Value::boolean(carries));
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `GetField` of a private name (`Vm::is_private_name`) from a register or `this`, which
    /// no field cache records (`lane_get_field_first`): the object's private element under
    /// the name's brand at this record (`lane_private_brand`). Any other read, a private
    /// method's or accessor's included, takes `lane_get_field`.
    #[inline(never)]
    fn lane_get_private_field<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let read = unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) }.and_then(|object| {
            let brand = lane_private_brand(cx, ip, code, i.imm())?;
            let data = (unsafe { vm(cx) }).object_data(object)?;
            match data.private_field(brand) {
                Some(value) => Some(PrivateRead::Value(value)),
                // A method or accessor of a brand the object carries (`Vm::site_private_element`).
                None if data.has_private_name(brand) => {
                    match (unsafe { vm(cx) }).site_private_element(ip.addr(), brand, object)? {
                        super::private_site_home::PrivateElement::Method(value) => Some(PrivateRead::Value(value)),
                        super::private_site_home::PrivateElement::Getter(getter) => Some(PrivateRead::Getter(object, getter)),
                    }
                }
                None => None,
            }
        });
        match read {
            Some(PrivateRead::Value(value)) => {
                r.set(i.a, value);
                next!(cx, r, following(ip), Carried::script(cx))
            }
            // The getter runs as the read's activation, as a cached accessor's does.
            Some(PrivateRead::Getter(object, getter)) => match lane_activate_getter(cx, r, ip, getter, object) {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                Err(next) => next,
            },
            None => sibling!(lane_get_field::<H>, r, ip, Carried::script(cx)),
        }
    }
}

script_handler! {
    /// `SetField` or `SetFieldStrict` at a site whose primary entry names an own data field
    /// (`Vm::primary_own_field_store_hit`): that store inline where the write barrier logs
    /// nothing; any other takes `lane_set_field_logged`.
    fn lane_set_object_field_own<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        if !unsafe { vm(cx) }.primary_own_field_store_hit::<false>(code, r.get(i.b), i.imm(), r.get(i.a), i.c) {
            sibling!(lane_set_field_logged::<H, false>, r, ip, Carried::script(cx))
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// An own-field store (`lane_set_object_field_own`, `lane_set_this_field_own`) the write
    /// barrier logs, which may grow its log: the store with the barrier, out of line so the
    /// inline store makes no call. Any other takes `lane_set_field_miss`.
    #[inline(never)]
    fn lane_set_field_logged<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let object = if THIS { unsafe { lane_fixed_this(cx) } } else { Some(r.get(i.b)) };
        let stored = object.is_some_and(|object| {
            unsafe { vm(cx) }.primary_own_field_store_hit::<true>(code, object, i.imm(), r.get(i.a), i.c)
        });
        if !stored {
            sibling!(lane_set_field_miss::<H, THIS>, r, ip, Carried::script(cx))
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_set_object_field_own` for `SetThisField` and `SetThisFieldStrict`.
    fn lane_set_this_field_own<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let this = unsafe { lane_held_this(cx) };
        let stored = unsafe { vm(cx) }.primary_own_field_store_hit::<false>(code, this, i.imm(), r.get(i.a), i.c);
        if !stored {
            sibling!(lane_set_field_logged::<H, true>, r, ip, Carried::script(cx))
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A miss of an own-field store (`lane_set_object_field_own`, `lane_set_this_field_own`):
    /// a site that adds properties or went polymorphic moves to the general cached write
    /// (`lane_set_object_field`, `lane_set_this_field`), which the store itself takes.
    #[inline(never)]
    fn lane_set_field_miss<THIS: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let polymorphic = vm.field_site_polymorphic(i.c);
        let adds = vm.field_site_adds(i.c);
        // A receiver outside the object records (a RegExp's or a function's cell) never hits
        // the own-record store, so its site takes the general cached write from now on.
        let cell_receiver = !THIS && r.get(i.b).direct_object_index().is_none() && vm.is_object_like(r.get(i.b));
        // A private field's store takes the private element path (`lane_set_field`).
        let handler: Option<Handler> = if vm.is_private_name(i.imm()) {
            Some(if THIS { lane_set_this_field::<H> } else { lane_set_object_field::<H> })
        } else if adds && !polymorphic && vm.field_add_atom_eligible(i.imm()) {
            Some(if THIS { lane_set_this_field_add::<H> } else { lane_set_object_field_add::<H> })
        } else if adds || polymorphic || cell_receiver {
            Some(if THIS { lane_set_this_field::<H> } else { lane_set_object_field::<H> })
        } else {
            None
        };
        if let Some(handler) = handler {
            requicken::<H>(ip, handler);
        }
        if THIS {
            sibling!(lane_set_this_field::<H>, r, ip, Carried::script(cx))
        }
        sibling!(lane_set_object_field::<H>, r, ip, Carried::script(cx))
    }
}

/// The cached add of an add record (`Vm::primary_field_add_hit`) of `r[i.a]` to `object`.
/// Out of line: the add keeps locals whose addresses its callees take, and a handler with
/// such locals cannot dispatch its successor as a sibling call.
#[inline(never)]
fn lane_field_add<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    object: Value,
    i: &LaneInstruction,
) -> bool {
    // SAFETY: the record's registers belong to the running activation, which the add
    // neither moves nor frees.
    let value = unsafe { (*lane_context::<H>()).registers }.get(i.a);
    vm.primary_field_add_hit(code, object, i.imm(), value, i.c)
}

script_handler! {
    /// `SetField` or `SetFieldStrict` at a site that adds a property to records of one shape
    /// (`lane_set_field_miss`): the proved add inline (`Vm::proved_field_add_hit`), else the
    /// cached add alone, out of line (`lane_set_field_add_any`).
    fn lane_set_object_field_add<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !vm.proved_field_add_hit(r.get(i.b), i.imm(), r.get(i.a), i.c) {
            sibling!(lane_set_field_add_any::<H, false>, r, ip, Carried::script(cx))
        }
        // Adding a property can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_set_object_field_add` for `SetThisField` and `SetThisFieldStrict`.
    fn lane_set_this_field_add<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let added = unsafe { lane_fixed_this(cx) }.is_some_and(|this| vm.proved_field_add_hit(this, i.imm(), r.get(i.a), i.c));
        if !added {
            sibling!(lane_set_field_add_any::<H, true>, r, ip, Carried::script(cx))
        }
        // Adding a property can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// An add record's add past a proved one (`lane_set_object_field_add`,
    /// `lane_set_this_field_add`): the cached add (`Vm::primary_field_add_hit`), which may
    /// prove the chain or move storage. Any other write moves the record to the general
    /// cached write (`lane_set_object_field`, `lane_set_this_field`), which makes it.
    #[inline(never)]
    fn lane_set_field_add_any<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let object = if THIS { unsafe { lane_fixed_this(cx) } } else { Some(r.get(i.b)) };
        if !object.is_some_and(|object| lane_field_add(vm, code, object, i)) {
            let general: Handler = if THIS { lane_set_this_field::<H> } else { lane_set_object_field::<H> };
            requicken::<H>(ip, general);
            if THIS {
                sibling!(lane_set_this_field::<H>, r, ip, Carried::script(cx))
            }
            sibling!(lane_set_object_field::<H>, r, ip, Carried::script(cx))
        }
        // Adding a property can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `SetField` and `SetFieldStrict`.
    fn lane_set_object_field<>(cx, r, ip, i, view) {
        lane_set_field(cx, r, ip, r.get(i.b), i)
    }
}

script_handler! {
    /// `SetThisField` and `SetThisFieldStrict`.
    fn lane_set_this_field<>(cx, r, ip, i, view) {
        let Some(this) = (unsafe { lane_this(cx) }) else {
            return exit(ip);
        };
        lane_set_field(cx, r, ip, this, i)
    }
}

script_handler! {
    /// Number comparisons, as on the general path's fast path.
    fn lane_jump_binary_false<BACKWARD: bool>(cx, r, ip, i, view) {
        let (Some(left), Some(right)) =
            (lane_operand_value(cx, r, i.b), lane_operand_value(cx, r, i.c))
        else {
            return exit(ip);
        };
        let Some(holds) = lane_relation::<H>(u32::from(i.a), left, right) else {
            return exit(ip);
        };
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The `instanceof` operator as a `Binary` immediate or a test's operator field.
const INSTANCEOF_OPERATOR: u32 = oxc_ast::ast::BinaryOperator::Instanceof as u32;

script_handler! {
    /// `Binary` `instanceof` of two registers, decoded when the view was derived
    /// (`specialize_register_operands`): the ordinary prototype-chain walk
    /// (`Vm::ordinary_instanceof`), which runs no guest code. Any other case, which the full
    /// algorithm decides, takes the general path.
    fn lane_instanceof_value<>(cx, r, ip, i, view) {
        let Some(holds) = (unsafe { vm(cx) }).steady_instanceof(r.get(i.b), r.get(i.c)) else {
            sibling!(lane_instanceof_value_walk::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, if holds { Value::TRUE } else { Value::FALSE });
        // `instanceof` is a JavaScript record, which carries the script state.
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_instanceof_value` run with the `JumpTrue` of its result that follows it, the
    /// `!(x instanceof C)` test of compiled code (`specialize_negated_instanceof`): the
    /// record carries the jump's distance from itself, and the result register is written
    /// as the pair would. A case the steady test leaves runs the pair unfused, from the walk.
    fn lane_instanceof_jump_true<BACKWARD: bool>(cx, r, ip, i, view) {
        let Some(holds) = (unsafe { vm(cx) }).steady_instanceof(r.get(i.b), r.get(i.c)) else {
            sibling!(lane_instanceof_value_walk::<H>, r, ip, Carried::script(cx))
        };
        if holds {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), Some((i.a, Value::TRUE)));
        }
        r.set(i.a, Value::FALSE);
        // SAFETY: the fused pair is two records, the second never the view's last.
        next!(cx, r, unsafe { ip.add(2) }, Carried::script(cx))
    }
}

script_handler! {
    /// `lane_instanceof_value` that its steady case leaves (`Vm::steady_instanceof`): the
    /// whole ordinary walk, out of line so the steady case calls nothing.
    #[inline(never)]
    fn lane_instanceof_value_walk<>(cx, r, ip, i, view) {
        let Some(holds) = (unsafe { vm(cx) }).ordinary_instanceof(r.get(i.b), r.get(i.c)) else {
            return exit(ip);
        };
        r.set(i.a, if holds { Value::TRUE } else { Value::FALSE });
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_binary_false` of `instanceof` with register operands, decoded when the view
    /// was derived (`specialize_register_operands`), as `lane_instanceof_value` decides it.
    fn lane_jump_instanceof_false<BACKWARD: bool>(cx, r, ip, i, view) {
        let Some(holds) = (unsafe { vm(cx) }).steady_instanceof(r.get(i.b), r.get(i.c)) else {
            sibling!(lane_jump_instanceof_false_walk::<H, BACKWARD>, r, ip, Carried::script(cx))
        };
        // `instanceof` is a JavaScript record, which carries the script state.
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_instanceof_false` that its steady case leaves: the whole ordinary walk, out
    /// of line so the steady case calls nothing.
    #[inline(never)]
    fn lane_jump_instanceof_false_walk<BACKWARD: bool>(cx, r, ip, i, view) {
        let Some(holds) = (unsafe { vm(cx) }).ordinary_instanceof(r.get(i.b), r.get(i.c)) else {
            return exit(ip);
        };
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_binary_false` with register operands, decoded when the view was derived
    /// (`specialize_register_operands`): Number comparison of two integers
    /// (`integer_relation`); any other pair takes `lane_jump_binary_false_registers_other`.
    fn lane_jump_binary_false_registers<BACKWARD: bool, OP: u32>(cx, r, ip, i, view) {
        let (left, right) = (r.get(i.b), r.get(i.c));
        let Some(holds) = super::operations::integer_relation(relation_operator::<OP>(i), left, right) else {
            sibling!(lane_jump_binary_false_registers_other::<H, BACKWARD>, r, ip, Carried::script(cx))
        };
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A plain local load followed by a register/register comparison branch. The load still
    /// writes its destination before the branch reads either operand, preserving aliases and
    /// values live beyond the branch while skipping one threaded dispatch.
    fn lane_load_local_jump_binary_false_registers<BACKWARD: bool>(cx, r, ip, i, view) {
        // SAFETY: the view installs this handler only for an adjacent supported branch record.
        let branch_ip = unsafe { ip.add(1) };
        let branch = unsafe { &*branch_ip };
        // SAFETY: the function's validated bytecode bounds the local slot, and the lane keeps
        // its frame installed for the whole run.
        let value = unsafe { vm(cx).read_validated_local((*cx).frame, i.imm() as usize) };
        r.set(i.a, value);
        let (left, right) = (r.get(branch.b), r.get(branch.c));
        let Some(holds) = super::operations::integer_relation(u32::from(branch.a), left, right)
        else {
            sibling!(
                lane_jump_binary_false_registers_other::<H, BACKWARD>,
                r,
                branch_ip,
                Carried::script(cx)
            )
        };
        if !holds {
            return lane_jump(cx, r, branch_ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(branch_ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_binary_false_registers` of operands other than two integers: the relations
    /// the lane decides without guest code (`lane_relation`); any other takes the general
    /// path. Out of line, so the integer case keeps few values live.
    #[inline(never)]
    fn lane_jump_binary_false_registers_other<BACKWARD: bool>(cx, r, ip, i, view) {
        let Some(holds) = lane_relation::<H>(u32::from(i.a), r.get(i.b), r.get(i.c)) else {
            return exit(ip);
        };
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_binary_false` of a register and an integer constant small enough for the
    /// record's `c` field, both decoded when the view was derived
    /// (`specialize_register_operands`): Number comparison of an integer register
    /// (`integer_relation`); any other takes `lane_jump_binary_false_small_other`.
    fn lane_jump_binary_false_small<BACKWARD: bool, OP: u32>(cx, r, ip, i, view) {
        let right = Value::integer(i32::from(i.c as i16));
        let Some(holds) = super::operations::integer_relation(relation_operator::<OP>(i), r.get(i.b), right) else {
            sibling!(lane_jump_binary_false_small_other::<H, BACKWARD>, r, ip, view)
        };
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, view, None);
        }
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `lane_jump_binary_false_small` of an operator or register value other than an integer
    /// comparison: a Number comparison (`number_relation`), else the truth of a Number
    /// operator's result (`if (x & MASK)`) or another relation (`lane_relation_other`). Out of
    /// line, so the integer case keeps few values live and makes no call.
    #[inline(never)]
    fn lane_jump_binary_false_small_other<BACKWARD: bool>(cx, r, ip, i, view) {
        let right = Value::integer(i32::from(i.c as i16));
        let Some(holds) = super::operations::number_relation(u32::from(i.a), r.get(i.b), right)
            .or_else(|| lane_relation_other::<H>(u32::from(i.a), r.get(i.b), right))
        else {
            return exit(ip);
        };
        if !holds {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_binary_false` of a register under strict (in)equality against a heap
    /// constant (a string or BigInt), both decoded when the view was derived
    /// (`specialize_register_operands`): equal exactly when identical or of equal contents
    /// (`Vm::strict_equal`), with no Number or `instanceof` test on the way.
    fn lane_jump_strict_constant_false<BACKWARD: bool>(cx, r, ip, i, view) {
        let value = r.get(i.b);
        // SAFETY: the run's program keeps its constants (`LaneContext::constants`), and the
        // view was derived with `c` indexing one.
        let constant = unsafe { *(&*(*cx).constants).get_unchecked(usize::from(i.c)) };
        let negated = u32::from(i.a) == STRICT_INEQUALITY_OPERATOR;
        let equal = strictly_equal_constant(unsafe { vm(cx) }, value, constant);
        if equal == negated {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The strict operators `lane_jump_strict_constant_false` decides.
const STRICT_INEQUALITY_OPERATOR: u32 = oxc_ast::ast::BinaryOperator::StrictInequality as u32;

/// Whether `value` is strictly equal to `constant`, a constant that is no Number: a boolean,
/// null or undefined is equal only to itself, a heap constant (a string or BigInt) also to
/// a value of equal contents (`Vm::strict_equal`), where two `typeof` names are equal only
/// when identical (`Vm::typeof_name`).
#[inline(always)]
fn strictly_equal_constant<H: Host>(vm: &Vm<H>, value: Value, constant: Value) -> bool {
    value == constant
        || (constant.is_heap()
            && value.is_heap()
            && !(vm.is_typeof_name(value) && vm.is_typeof_name(constant))
            && vm.strict_equal(value, constant))
}

lane_handler! {
    /// `lane_relation_value` under strict (in)equality against a constant that is no Number,
    /// carried in the record with the operator in `c` (`strictly_equal_constant`).
    fn lane_strict_constant_value<>(cx, r, ip, i, view) {
        let equal = strictly_equal_constant(unsafe { vm(cx) }, r.get(i.b), i.constant());
        let holds = equal != (u32::from(i.c) == STRICT_INEQUALITY_OPERATOR);
        r.set(i.a, if holds { Value::TRUE } else { Value::FALSE });
        next!(cx, r, following(ip), view)
    }
}

/// The nullish test of a `lane_jump_nullish_false` instantiation (`NULLISH`): strict
/// identity with `null` or with `undefined`, or loose equality with either.
const NULLISH_NULL: u16 = 0;
const NULLISH_UNDEFINED: u16 = 1;
const NULLISH_LOOSE: u16 = 2;

script_handler! {
    /// `lane_jump_binary_false` of a register under an equality operator against a `null` or
    /// `undefined` constant, decoded when the view was derived
    /// (`specialize_register_operands`) into the instantiation's negation and `NULLISH` test:
    /// strict equality is identity with the constant; loose equality holds for `null` and
    /// `undefined`, and among other values only for [[IsHTMLDDA]] objects
    /// (`Vm::identity_equality`).
    fn lane_jump_nullish_false<BACKWARD: bool, NEGATED: bool, NULLISH: u16>(cx, r, ip, i, view) {
        let value = r.get(i.b);
        let equal = match NULLISH {
            NULLISH_NULL => value == Value::NULL,
            NULLISH_UNDEFINED => value == Value::UNDEFINED,
            // A record is an ordinary object: never `null`, `undefined` or [[IsHTMLDDA]].
            _ if value.is_heap() => {
                if !value.is_record() {
                    sibling!(lane_jump_nullish_cell::<H, BACKWARD, NEGATED>, r, ip, Carried::script(cx))
                }
                false
            }
            _ => value.is_null() || value.is_undefined(),
        };
        if equal == NEGATED {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_binary_false` of a register under strict equality against a boolean
    /// constant (`specialize_register_operands`), the `x !== false` truthiness test of
    /// compiled code: a boolean strictly equals only itself, which has one encoding, so the
    /// test is the register's identity with the instantiation's constant (`TRUE`).
    fn lane_jump_boolean_false<BACKWARD: bool, NEGATED: bool, TRUE: bool>(cx, r, ip, i, view) {
        let equal = r.get(i.b) == if TRUE { Value::TRUE } else { Value::FALSE };
        if equal == NEGATED {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_binary_false` of two registers under `===` (`!==` where `NEGATED`)
    /// (`specialize_register_operands`): strict equality runs no guest code, so the lane
    /// decides every pair (`Vm::strict_equal`) without the general relation chain. Values
    /// outside the heap compare as numbers or by identity, and a heap value equals only
    /// itself among them, inline; two distinct heap values, which may be equal strings or
    /// BigInts, take `lane_jump_strict_equal_cells`.
    fn lane_jump_strict_equal_false<BACKWARD: bool, NEGATED: bool>(cx, r, ip, i, view) {
        let (left, right) = (r.get(i.b), r.get(i.c));
        let equal = match (left.as_number(), right.as_number()) {
            (Some(left), Some(right)) => left == right,
            _ if left == right => true,
            _ if left.is_heap() && right.is_heap() => {
                sibling!(lane_jump_strict_equal_cells::<H, BACKWARD, NEGATED>, r, ip, Carried::script(cx))
            }
            _ => false,
        };
        if equal == NEGATED {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_strict_equal_false` of two distinct heap values (`Vm::strict_equal`); out
    /// of line, so the common tests make no call.
    #[inline(never)]
    fn lane_jump_strict_equal_cells<BACKWARD: bool, NEGATED: bool>(cx, r, ip, i, view) {
        if unsafe { vm(cx) }.strict_equal(r.get(i.b), r.get(i.c)) == NEGATED {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_jump_nullish_false`'s loose test of a heap value outside the records, which only
    /// an [[IsHTMLDDA]] object passes; out of line, so the common tests make no call.
    #[inline(never)]
    fn lane_jump_nullish_cell<BACKWARD: bool, NEGATED: bool>(cx, r, ip, i, view) {
        if unsafe { vm(cx) }.is_html_dda(r.get(i.b)) == NEGATED {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The instantiation of an equality jump-test handler for its direction and negation, the
/// const-generic prefix the view decodes from the record (`specialize_register_operands`).
macro_rules! equality_test_handler {
    ($handler:ident::<$host:ident $(, $rest:tt)*>, $backward:expr, $negated:expr) => {
        match ($backward, $negated) {
            (false, false) => $handler::<$host, false, false $(, $rest)*> as Handler,
            (false, true) => $handler::<$host, false, true $(, $rest)*> as Handler,
            (true, false) => $handler::<$host, true, false $(, $rest)*> as Handler,
            (true, true) => $handler::<$host, true, true $(, $rest)*> as Handler,
        }
    };
}

/// The tests the general loop decides past Number comparison without running guest code:
/// identity equality, an ordinary `instanceof`, BigInt order, and the truth of a Number
/// operator's result.
/// Out of line, so the comparison handlers keep their Number path's shape.
#[cold]
#[inline(never)]
fn lane_relation_other<H: Host>(operator: u32, left: Value, right: Value) -> Option<bool> {
    let vm = unsafe { vm(lane_context::<H>()) };
    // A nullish value is unequal to every Number under both loose and strict equality.
    // The Number/nullish pair cannot invoke conversion hooks or `document.all`; keep this
    // check in the shared cold fallback so it does not enlarge each hot comparison handler.
    if operator <= STRICT_INEQUALITY_OPERATOR
        && ((left.is_null() || left.is_undefined()) && right.as_number().is_some()
            || (right.is_null() || right.is_undefined()) && left.as_number().is_some())
    {
        return Some(matches!(operator, 1 | STRICT_INEQUALITY_OPERATOR));
    }
    vm.instanceof_truthy(operator, left, right)
        .or_else(|| vm.plain_in(operator, left, right))
        .or_else(|| vm.identity_equality(operator, left, right))
        .or_else(|| vm.string_relation(operator, left, right))
        // A test of an arithmetic or bitwise result (`if (flags & MASK)`): the Number
        // operator's value, whose ToBoolean runs no guest code.
        .or_else(|| vm.bigint_relation(operator, left, right))
        .or_else(|| vm.number_binary(operator, left, right)?.truthiness())
}

/// A taken jump. Backward jumps, the only ones that close a loop, are
/// safepoints: they leave the lane when a collection is due; forward jumps
/// dispatch directly. Every jump's direction is fixed when the view is
/// derived. Leaving hands the whole instruction to the general path, so
/// `commit`, the register write the instruction makes before its jump,
/// applies only when the lane takes the jump itself.
#[inline(always)]
fn lane_jump<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    backward: bool,
    view: Carried,
    commit: Option<(u16, Value)>,
) -> Ip {
    let jump = unsafe { (*ip).jump() };
    // SAFETY: the view records the distance to a validated jump target.
    let target = unsafe { ip.byte_offset(jump) };
    if backward && below_floor() {
        // Scalar arguments only, so the slow path stays a sibling call.
        return match commit {
            None => lane_safepoint::<H>(ip, target, view.acc),
            Some((register, value)) => lane_safepoint_writing::<H>(r, ip, target, view.acc, register, value),
        };
    }
    if let Some((register, value)) = commit {
        r.set(register, value);
    }
    next!(cx, r, target, view)
}

/// Count one backward jump of a crediting run (`LaneContext::backedges_left`); at the budget's
/// end, credit the running Wasm function with one interpreter entry for the loop work
/// (`ProgramStore::note_wasm_tier_backedges`) and refill it. A function entered once and
/// looping hot (JetStream's tsf and zlib) then promotes at its next entry, as one called often
/// does.
fn lane_count_backedge<H: Host>(cx: *mut LaneContext<H>) {
    // SAFETY: called from `lane_safepoint` with the run's context.
    let Some(left) = (unsafe { (*cx).backedges_left }) else {
        return;
    };
    if let Some(left) = left.checked_sub(1) {
        unsafe { (*cx).backedges_left = Some(left) };
        return;
    }
    let machine = unsafe { vm(cx) };
    let program = unsafe { (*cx).program };
    let function = machine
        .lane_stack
        .activations
        .last()
        .map_or(machine.frames[unsafe { (*cx).frame }].function, |activation| activation.function);
    machine.programs.note_wasm_tier_backedges(program, function);
    unsafe { (*cx).backedges_left = Some(super::program_store::WASM_JIT_BACKEDGES_PER_ENTRY) };
}

lane_handler! {
    fn lane_jump_always<BACKWARD: bool>(cx, r, ip, i, view) {
        lane_jump(cx, r, ip, BACKWARD, view, None)
    }
}

script_handler! {
    /// A loop's backward jump in compiled execution (`LoopSite`): the jump's safepoint, then
    /// the loop's native code once the jump is hot and the region compiled. Native code
    /// resumes the lane where control leaves the region, or leaves to the general path at an
    /// instruction outside its subset; a run its entry guards decline takes the jump.
    fn lane_loop_jump<>(cx, r, ip, i, view) {
        let site = i.loop_site();
        if site.diagnostics {
            site.lane_backedge_count
                .set(site.lane_backedge_count.get().saturating_add(1));
        }
        // SAFETY: the site records the distance to a validated jump target.
        let header = unsafe { ip.byte_offset(site.jump) };
        // SAFETY: the region carries its validated header and latch PCs, and
        // every instruction record remains contiguous in this lane view.
        let at = |pc: u32| unsafe { header.offset(pc as isize - site.header as isize) };
        if below_floor() {
            return lane_safepoint::<H>(ip, header, Slot::unset());
        }
        // Keep the ordinary interpreter and JIT-off lane free of tier metadata
        // lookups. Only the opt-in shared JS baseline observes loop work here.
        let runtime = unsafe { vm(cx) };
        if runtime.js_tier_runs()
            && runtime.programs.js_jit_general_dispatch_enabled()
            && unsafe { &*(*cx).code }.kind != crate::bytecode::ProgramKind::Wasm
        {
            let (program_id, code) = unsafe {
                let context = &*cx;
                (context.program, &*context.code)
            };
            let function = site.function;
            let instruction_count = code
                .function_get(function as usize)
                .map_or(0, |body| body.code.len());
            if runtime.programs.note_js_general_dispatch_backedge(
                program_id,
                function,
                // Only the taken backedge is guaranteed to have executed;
                // branches and `continue` can skip the rest of the region.
                1,
                instruction_count,
            ) {
                // The shared whole-function CFG starts at this exact loop header
                // after run_fast_lane materializes any frameless lane activations.
                return exit(header);
            }
        }
        if site.promotion_only {
            next!(cx, r, header, Carried::script(cx))
        }
        if site.taken.get() < LOOP_TIER_THRESHOLD {
            site.taken.set(site.taken.get() + 1);
            next!(cx, r, header, Carried::script(cx))
        }
        if site.analysis_failure.is_some_and(|failure| {
            failure.reason != super::compiled::RegionRejection::UnsupportedInstruction(Op::Call)
        }) {
            lane_loop_retire::<H>(cx, ip, site);
            next!(cx, r, header, Carried::script(cx))
        }
        // SAFETY: the lane's VM is live, and nothing borrows it across the read.
        let layout_epoch = crate::heap::prototype_layout_epoch();
        {
            let mut tier = site.tier.borrow_mut();
            if tier.native_field_epoch.is_some_and(|epoch| epoch != layout_epoch) {
                tier.code = None;
                tier.native_field_epoch = None;
            }
        }
        if site.exits.get() >= LOOP_TIER_MAX_EXITS
            || site.declines.get() >= LOOP_TIER_MAX_DECLINES
            || !site.tier.borrow().may_run(unsafe { vm(cx) }.heap.memory_policy())
        {
            lane_loop_retire::<H>(cx, ip, site);
            next!(cx, r, header, Carried::script(cx))
        }
        let region = match site.region.get() {
            Some(region) => region,
            None => match lane_inlining_region(cx, r, site) {
                Some(region) => site.region.get_or_init(|| region),
                None => {
                    // The calls do not run inlined: the loop stays the interpreter's.
                    site.exits.set(LOOP_TIER_MAX_EXITS);
                    lane_loop_retire::<H>(cx, ip, site);
                    next!(cx, r, header, Carried::script(cx))
                }
            },
        };
        match lane_loop_native::<H>(cx, r, ip, header, site, region) {
            LoopAttempt::Declined => lane_at_header!(cx, r, header, site),
            LoopAttempt::At(next) => next!(cx, r, next, Carried::script(cx)),
            LoopAttempt::Leave(next) => next,
        }
    }
}

/// The header record's own handler at `header`, as the interpreter runs it: the one `site`
/// holds while it holds the record (`lane_loop_header`), which the backward jump thus does
/// not try again.
macro_rules! lane_at_header {
    ($cx:expr, $r:expr, $header:expr, $site:expr) => {{
        let held = $site.header_handler.get();
        if held.is_null() {
            next!($cx, $r, $header, Carried::script($cx))
        }
        // SAFETY: the site holds the record's handler, which has the shared signature.
        let handler: Handler = unsafe { std::mem::transmute::<*const (), Handler>(held) };
        sibling!(handler, $r, $header, Carried::script($cx))
    }};
}
use lane_at_header;

/// What a loop site's native attempt leaves the lane to do.
enum LoopAttempt {
    /// Nothing ran natively: the header record runs as the interpreter's.
    Declined,
    /// Continue at this record.
    At(Ip),
    /// Return this from the handler: an exit, or a safepoint's continuation.
    Leave(Ip),
}

/// Runs `site`'s region on `r` from its `header` record, reached at `record` (the backward
/// jump, or the header itself): the entry guards, then native code, which resumes the lane
/// where control leaves the region or leaves to the general path. Once the region has code,
/// the site holds its header record, so a loop entered from outside runs natively from its
/// first iteration.
#[inline(always)]
fn lane_loop_native<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    record: Ip,
    header: Ip,
    site: &LoopSite,
    region: &super::compiled::LoopRegion,
) -> LoopAttempt {
    // SAFETY: the region carries its validated header and latch PCs, and every instruction
    // record remains contiguous in this lane view.
    let at = |pc: u32| unsafe { header.offset(pc as isize - site.header as isize) };
    // The record is filled where it lives, so its words are neither cleared nor copied.
    let mut entry = super::compiled::LoopEntry::new();
    if lane_loop_entry(cx, r, region, &mut entry).is_none() {
        site.declines.set(site.declines.get() + 1);
        return LoopAttempt::Declined;
    }
    // The tier's state is borrowed only here, so the dispatch below holds no borrow.
    let continuation = lane_loop_run(cx, r, site, region, &mut entry);
    site.declines.set(if matches!(continuation, LoopContinuation::Jump) { site.declines.get() + 1 } else { 0 });
    lane_loop_hold_header::<H>(header, site);
    match continuation {
        LoopContinuation::Jump => LoopAttempt::Declined,
        LoopContinuation::Resume(pc) => LoopAttempt::At(at(pc)),
        LoopContinuation::Exit(pc) => LoopAttempt::Leave(match region.inlined_exit(pc) {
            Some((frame, callee_pc)) => exit(lane_inlined_activation(cx, r, region, &entry, *frame, at(frame.call), pc, callee_pc)),
            None => exit(at(pc)),
        }),
        LoopContinuation::Safepoint => LoopAttempt::Leave(lane_safepoint::<H>(record, header, Slot::unset())),
    }
}

/// Gives `site` its `header` record (`LoopSite::hookable`) once its region has code: the
/// record runs `lane_loop_header`, and the site keeps the record's own handler.
fn lane_loop_hold_header<H: Host>(header: Ip, site: &LoopSite) {
    if !site.hookable || !site.header_handler.get().is_null() || site.tier.borrow().code.is_none() {
        return;
    }
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view.
    unsafe {
        site.header_handler.set((*header).handler);
        (*header.cast_mut()).handler = lane_loop_header::<H> as Handler as *const ();
    }
}

/// The site whose header record is at `ip` (`LoopSite::hookable`), in the view that holds
/// the record: the running activation's, else another of the program's.
unsafe fn lane_header_site<H: Host>(cx: *mut LaneContext<H>, ip: Ip) -> Option<*const LoopSite> {
    // SAFETY: the context's VM and views are live while the lane runs.
    let (machine, views) = unsafe { (vm(cx), &*(*cx).views) };
    let function = machine
        .lane_stack
        .activations
        .last()
        .map_or(machine.frames[unsafe { (*cx).frame }].function, |activation| activation.function);
    let holds = |view: &&LaneView| {
        let (start, end) = view.record_range();
        (start..end).contains(&ip.addr())
    };
    let view = views
        .get(function as usize)
        .and_then(super::program_store::derived_lane_view)
        .filter(holds)
        .or_else(|| views.iter().filter_map(super::program_store::derived_lane_view).find(holds))?;
    // SAFETY: a record of the view; a held header's `call_link` names its site.
    view.loops.get(usize::from(unsafe { (*ip).call_link.get() })).map(std::ptr::from_ref)
}

script_handler! {
    /// A loop's header record while its site holds it (`lane_loop_hold_header`): arriving at
    /// the header enters the loop's native code as its backward jump does, and otherwise
    /// runs the record's own handler.
    fn lane_loop_header<>(cx, r, ip, i, view) {
        // SAFETY: a record runs this handler only while a site holds it.
        let site = unsafe { &*lane_header_site::<H>(cx, ip).expect("a held header names its site") };
        if !below_floor()
            && site.exits.get() < LOOP_TIER_MAX_EXITS
            && site.declines.get() < LOOP_TIER_MAX_DECLINES
            && let Some(region) = site.region.get()
        {
            match lane_loop_native::<H>(cx, r, ip, ip, site, region) {
                LoopAttempt::Declined => {}
                LoopAttempt::At(next) => next!(cx, r, next, Carried::script(cx)),
                LoopAttempt::Leave(next) => return next,
            }
        }
        lane_at_header!(cx, r, ip, site)
    }
}

/// Gives a loop site that will not enter native code again its plain backward jump back,
/// so each iteration takes the jump without the site's checks. The shared JS baseline
/// observes every backward jump of a site (`note_js_general_dispatch_backedge`), so while
/// it runs the site stays.
fn lane_loop_retire<H: Host>(cx: *mut LaneContext<H>, ip: Ip, site: &LoopSite) {
    // SAFETY: the lane's VM is live, and nothing borrows it across the read.
    if unsafe { vm(cx) }.programs.js_jit_general_dispatch_enabled() {
        return;
    }
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view, and
    // the jump reads its distance from the operand, which the site kept (`LoopSite::jump`).
    // A header record the site holds gets its own handler back.
    unsafe {
        let held = site.header_handler.replace(std::ptr::null());
        if !held.is_null() {
            (*ip.byte_offset(site.jump).cast_mut()).handler = held;
        }
        (*ip.cast_mut()).operand = LaneOperand { jump: site.jump };
    }
    requicken::<H>(ip, lane_jump_always::<H, true>);
}

/// Where the lane goes after a loop site's native attempt.
enum LoopContinuation {
    /// Take the backward jump in the interpreter.
    Jump,
    Resume(u32),
    /// The general path executes the instruction at this PC.
    Exit(u32),
    /// The backward jump's safepoint ends the execution termination stopped.
    Safepoint,
}

/// Compiles `site`'s region if it has no current code, then runs it on `r` with `entry`. A
/// representation an outcome shows wrong is widened, and the code dropped for the next
/// compilation.
#[inline(never)]
fn lane_loop_run<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    site: &LoopSite,
    region: &super::compiled::LoopRegion,
    entry: &mut super::compiled::LoopEntry,
) -> LoopContinuation {
    use super::compiled::LoopOutcome;
    let mut tier = site.tier.borrow_mut();
    let tier = &mut *tier;
    if tier.code.is_none() {
        // SAFETY: the lane's VM is live, and nothing borrows it across the read.
        if !tier.may_run(unsafe { vm(cx) }.heap.memory_policy()) {
            return LoopContinuation::Jump;
        }
        tier.compilations += 1;
        if site.diagnostics {
            site.compile_attempts.set(site.compile_attempts.get().saturating_add(1));
        }
        // SAFETY: the lane's VM is live, and nothing borrows it across the call.
        unsafe { vm(cx) }.host.before_native_compilation();
        let captures = entry.captures(region.captures.len());
        let fields: Vec<Value> = (0..region.fields.len()).map(|index| entry.field_value(region, index)).collect();
        let machine = unsafe { vm(cx) };
        let (properties_base, properties_len) = machine.heap.native_property_values();
        let native_heap_available =
            machine.heap.object_record_base().is_some() && !properties_base.is_null() && properties_len > 0;
        let native_field_loads = region
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                (native_heap_available && region.inlining.is_none() && region.is_dynamic_field(index))
                    .then(|| machine.native_field_load_descriptor(field.cache_site))
                    .flatten()
                    .map(|(shape, slot)| super::compiled::NativeFieldLoad {
                        shape,
                        slot,
                        epoch: crate::heap::prototype_layout_epoch(),
                    })
            })
            .collect::<Vec<_>>();
        let heap = &machine.heap;
        // A field's array is the value its entry word holds, not yet the register's.
        let array_value = |register| match region.arrays.iter().find(|array| array.register == register) {
            Some(super::compiled::ArrayUse { field: Some(index), .. }) => entry.field_value(region, *index),
            _ => r.get(register),
        };
        let int32_elements = |register| {
            heap.record_array(array_value(register)).is_some_and(|array| {
                array.elements().iter().take(LOOP_TIER_ELEMENT_SAMPLE).all(|value| value.as_int().is_some())
            })
        };
        // An inlined callee's register is no register of the running activation: it holds
        // nothing at entry.
        let caller_registers = region.inlining.as_ref().map_or(Register::MAX, |inlining| inlining.caller_registers);
        let observation = super::compiled::Observation {
            register: &|register| if register < caller_registers { r.get(register) } else { Value::UNDEFINED },
            int32_elements: &int32_elements,
            captures: &captures,
            fields: &fields,
            native_field_loads: &native_field_loads,
            widened: &tier.widened,
        };
        let track_backedges = unsafe { vm(cx) }
            .programs
            .js_jit_general_dispatch_enabled();
        tier.code =
            super::compiled::compile_osr_region(
                region,
                &observation,
                &unsafe { vm(cx) }.execution_interrupt,
                heap.memory_policy(),
                track_backedges,
            );
        tier.native_field_epoch = tier.code.as_ref().and_then(|_| {
            native_field_loads
                .iter()
                .flatten()
                .next()
                .map(|load| load.epoch)
        });
        // SAFETY: as above; the observation's borrow of the heap has ended.
        unsafe { vm(cx) }.host.after_native_compilation();
        if site.diagnostics && tier.code.is_some() {
            site.compile_successes
                .set(site.compile_successes.get().saturating_add(1));
        }
        if tier.code.is_none() {
            tier.compilations = LOOP_TIER_MAX_COMPILATIONS;
            return LoopContinuation::Jump;
        }
    }
    let code = tier.code.as_ref().expect("compiled above");
    // The native field probe may access the VM only through its synchronous, cache-only
    // callback. Pass raw pointers so no Rust borrow of the VM overlaps that callback.
    let (machine, program, interrupt) = unsafe {
        let context = &*cx;
        (
            context.vm,
            context.program,
            std::ptr::addr_of!((*context.vm).execution_interrupt),
        )
    };
    // SAFETY: `r` and `machine` are the running lane activation's storage and VM; the
    // callback executes no guest code, allocates nothing, and retains no VM references.
    let outcome = unsafe {
        code.run::<H>(r.0, entry, machine, program, site.function, (*cx).frame, interrupt)
    };
    if site.diagnostics {
        site.native_entry_attempts
            .set(site.native_entry_attempts.get().saturating_add(1));
        match outcome {
            LoopOutcome::NotEntered(widening) => {
                if std::env::var_os("QUENCH_JS_JIT_TRACE_OSR_REJECT").is_some() {
                    eprintln!(
                        "QUENCH_JS_JIT_OSR_ENTRY_MISS function={} header={} widening={widening:?}",
                        site.function,
                        site.header,
                    );
                }
                site.entry_guard_misses
                    .set(site.entry_guard_misses.get().saturating_add(1));
            }
            LoopOutcome::Resume(_) => {
                site.native_region_runs
                    .set(site.native_region_runs.get().saturating_add(1));
            }
            LoopOutcome::Exit(_) => {
                site.native_region_runs
                    .set(site.native_region_runs.get().saturating_add(1));
                site.native_exits.set(site.native_exits.get().saturating_add(1));
            }
            LoopOutcome::Widen(_) => {
                site.native_region_runs
                    .set(site.native_region_runs.get().saturating_add(1));
                site.widenings.set(site.widenings.get().saturating_add(1));
            }
            LoopOutcome::Terminated => {}
        }
    }
    let general_dispatch_due = {
        let runtime = unsafe { vm(cx) };
        if runtime.programs.js_jit_general_dispatch_enabled() {
            let backedges = usize::try_from(entry.backedge_count()).unwrap_or(usize::MAX);
            // Every completed iteration executed at least its own backedge. The
            // loop body's static span is not a sound work estimate when branches
            // or `continue` paths skip instructions.
            let work = backedges;
            if work > 0 {
                let context = unsafe { &*cx };
                let function = site.function;
                let instruction_count = unsafe { &*context.code }
                    .function_get(function as usize)
                    .map_or(0, |body| body.code.len());
                runtime.programs.note_js_general_dispatch_backedge(
                    context.program,
                    function,
                    work,
                    instruction_count,
                )
            } else {
                false
            }
        } else {
            false
        }
    };
    if general_dispatch_due {
        match outcome {
            LoopOutcome::Resume(pc) => return LoopContinuation::Exit(pc),
            LoopOutcome::Exit(pc) => {
                site.exits.set(site.exits.get().saturating_add(1));
                return LoopContinuation::Exit(pc);
            }
            LoopOutcome::Widen(pc) => {
                if let Some(widening) = code.widening(pc) {
                    tier.widened.insert(widening);
                }
                tier.code = None;
                return LoopContinuation::Exit(pc);
            }
            LoopOutcome::NotEntered(_) | LoopOutcome::Terminated => {}
        }
    }
    match outcome {
        LoopOutcome::NotEntered(widening) => {
            if let Some(widening) = widening {
                tier.widened.insert(widening);
                tier.code = None;
            }
            LoopContinuation::Jump
        }
        LoopOutcome::Resume(pc) => LoopContinuation::Resume(pc),
        LoopOutcome::Exit(pc) => {
            site.exits.set(site.exits.get() + 1);
            LoopContinuation::Exit(pc)
        }
        LoopOutcome::Widen(pc) => {
            if let Some(widening) = code.widening(pc) {
                tier.widened.insert(widening);
            }
            tier.code = None;
            LoopContinuation::Exit(pc)
        }
        LoopOutcome::Terminated => LoopContinuation::Safepoint,
    }
}

/// The entry record of `site`'s native code from the running activation, when its arrays
/// and captures pass the region's guards: each written array a plain record array whose
/// elements a write replaces in place (`Vm::try_replace_record_element`), each read one a
/// record array (`Vm::direct_dense_element`), each capture a plain value of the running
/// function's dense capture slots (`lane_load_capture`).
#[inline(never)]
fn lane_loop_entry<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    region: &super::compiled::LoopRegion,
    entry: &mut super::compiled::LoopEntry,
) -> Option<()> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the running program outlives this lane's OSR entry setup.
    let code = unsafe { &*(*cx).code };
    if !region.captures.is_empty() {
        let slots = capture_reads(unsafe { (*cx).capture });
        if slots.is_null() {
            return None;
        }
        for slot in &region.captures {
            // SAFETY: the running function's dense capture slots (`LaneContext::capture`).
            entry.push_capture((unsafe { &*slots }).dense_plain(usize::from(*slot))?);
        }
    }
    // These regions cannot run calls or field writes, and their element stores define or
    // replace own data elements, which changes no shape slot. Thus a hit on an ordinary
    // data field stays valid for the whole native run. Any miss, accessor, private name, or
    // non-Number exits before native code can alter a register. A field read through
    // another's value follows it.
    for field in &region.fields {
        let value = lane_region_field(cx, r, field, |index| entry.field_value(region, index))?;
        entry.push_field(value, field.tagged)?;
    }
    for array in &region.arrays {
        let value = match array.field {
            None => r.get(array.register),
            Some(index) => entry.field_value(region, index),
        };
        if array.written {
            // A store below the length or an append at it, into a dense array no prototype
            // claims an index of, defines or replaces an own data element and nothing else
            // (`append_dense_element`, `fill_dense_hole`).
            if vm.dense_array_length(value).is_none() || !vm.prototypes_hold_no_index(value) {
                return None;
            }
            // A Number store logs nothing.
            let object = vm.heap.record_array_store_mut_unlogged(value, Value::integer(0))?;
            let elements = object.elements_mut();
            if elements.owner_count() != 1 {
                return None;
            }
            let elements = super::index::mutable_array_elements(elements);
            entry.push_array(elements.as_mut_ptr(), elements.len(), elements.capacity())?;
        } else {
            let elements = vm.heap.record_array(value)?.elements().as_slice();
            entry.push_array(elements.as_ptr().cast_mut(), elements.len(), elements.len())?;
        }
    }
    // A raw field slot stays valid for this native run because OSR regions make no calls or
    // allocations, the receiver is unchanged and rooted in its register, and these stores
    // write only Number values (which need no collector barrier). Every miss exits before the
    // store through the interpreter's exact SetField PC.
    // SAFETY: the runtime cache probe proves a primary own-data slot. `osr_loop::admit_region`
    // admits only Number values and rejects receiver rewrites and same-name field reads.
    for field in &region.field_store_sites {
        let object = r.get(field.receiver);
        let pointer = unsafe {
            vm.cached_field_store_pointer(code, object, field.atom, field.cache_site)
        }?;
        entry.push_field_store(pointer);
    }
    if region.has_dynamic_field_reads() && region.inlining.is_none() {
        let object_base = vm.heap.object_record_base().unwrap_or(std::ptr::null());
        let (properties_base, properties_len) = vm.heap.native_property_values();
        entry.push_native_field_heap(object_base, properties_base, properties_len);
    }
    if let Some(inlining) = &region.inlining {
        // Leaving at an inlined instruction makes its callee's activation as the call would:
        // each call must still be one the lane activates in place (`lane_inlined_activation`).
        for frame in &inlining.frames {
            lane_inlined_target(cx, lane_inlined_receiver(r, region, entry, frame), frame)?;
        }
        entry.push_scratch();
    }
    Some(())
}

/// A cached data field's value as the region's entry reads it: a hit of the field's cache
/// on its receiver, for no private name. `earlier` gives the value of the earlier field a
/// receiver may be (`FieldReceiver::Field`).
fn lane_region_field<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    field: &super::compiled::FieldUse,
    earlier: impl FnOnce(usize) -> Value,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if vm.is_private_name(field.atom) {
        return None;
    }
    let receiver = match field.receiver {
        super::compiled::FieldReceiver::This => unsafe { lane_held_this(cx) },
        super::compiled::FieldReceiver::Register(register) => r.get(register),
        super::compiled::FieldReceiver::Field(index) => earlier(index),
    };
    match vm.cached_field_hit(code, receiver, field.cache_site) {
        super::object::FieldCacheProbe::Hit(value) => Some(value),
        _ => None,
    }
}

/// The call target of an inlined `frame`'s callee, when `lane_script_call` would activate it
/// in place now, binding the frame's receiver as its `this`: the target the call caches, an
/// activation of the running program with room on the lane stack.
fn lane_inlined_target<H: Host>(
    cx: *mut LaneContext<H>,
    receiver: Value,
    frame: &super::compiled::InlineFrame,
) -> Option<*const ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    let target_pointer = vm.script_call_entry(frame.callee)?;
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
    let target = unsafe { &*target_pointer };
    (target.entry == CallEntry::Activates
        && target.program == unsafe { (*cx).program }
        && unsafe { (*cx).script_calls }
        && !vm.with_scope_entered
        && lane_activation_room(vm, target)
        && lane_call_this(vm, target, receiver) == Some(receiver))
    .then_some(target_pointer)
}

/// The receiver of an inlined `frame`'s call: the field value a register the region writes
/// holds (`LoopRegion::held_field`), else the register's.
fn lane_inlined_receiver(
    r: Registers,
    region: &super::compiled::LoopRegion,
    entry: &super::compiled::LoopEntry,
    frame: &super::compiled::InlineFrame,
) -> Value {
    match region.held_field(frame.receiver) {
        Some(index) => entry.field_value(region, index),
        None => r.get(frame.receiver),
    }
}

/// The region of `site`'s loop with its calls inlined, each callee the function the cached
/// field it is read from holds now, whose call the lane activates in place.
#[inline(never)]
fn lane_inlining_region<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    site: &LoopSite,
) -> Option<super::compiled::LoopRegion> {
    let machine = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let program = unsafe { (*cx).program };
    let function_index = machine
        .lane_stack
        .activations
        .last()
        .map_or(machine.frames[unsafe { (*cx).frame }].function, |activation| activation.function);
    let function = code.function_get(function_index as usize)?;
    let decoded: Vec<WideInstruction> = function.instructions().collect();
    let live = code.register_root_masks(function_index)?;
    let constants = machine.programs.constants(program).to_vec();
    // The callee is the last of the fields its read reads through, each read from the
    // registers and the fields before it as they are now.
    let mut resolve = |chain: &[super::compiled::FieldUse]| -> Option<super::compiled::InlineCallee<'_>> {
        let mut values = Vec::with_capacity(chain.len());
        for field in chain {
            let value = lane_region_field(cx, r, field, |index| values[index])?;
            values.push(value);
        }
        let callee = *values.last()?;
        let vm = unsafe { vm(cx) };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
        let target = unsafe { &*vm.script_call_entry(callee)? };
        (target.entry == CallEntry::Activates && target.program == program).then_some(())?;
        let body = code.function_get(target.function as usize)?;
        Some(super::compiled::InlineCallee {
            callee,
            params: body.params,
            registers: body.registers,
            instructions: body.instructions().collect(),
            live: code.register_root_masks(target.function)?,
        })
    };
    super::compiled::analyze_inlining_osr_region(
        function,
        &decoded,
        &constants,
        &code.field_sites,
        live,
        site.header,
        site.backedge,
        &mut resolve,
    )
}

/// The record native code leaving at the inlined instruction `callee_pc` of `frame` (exit
/// PC `pc`) continues at: the callee's activation the call at `call` makes
/// (`lane_push_activation`), its registers those native code held, a parameter bound to a
/// caller register that register's value, and any other `undefined`, which nothing reads
/// before writing. The entry checked the call activates in place (`lane_inlined_target`).
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn lane_inlined_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    region: &super::compiled::LoopRegion,
    entry: &super::compiled::LoopEntry,
    frame: super::compiled::InlineFrame,
    call: Ip,
    pc: u32,
    callee_pc: u32,
) -> Ip {
    let receiver = lane_inlined_receiver(r, region, entry, &frame);
    let target = lane_inlined_target(cx, receiver, &frame).expect("the entry checked the inlined call");
    // SAFETY: as `Vm::script_call_entry`; `call` is the call's record in the running view.
    let (target, arguments) =
        unsafe { (&*target, ImmediateLayout::call_window_base((*call).imm())) };
    let (registers, _) =
        lane_push_activation::<H, false, false>(cx, r, call, target, arguments, receiver, false);
    let inlining = region.inlining.as_ref().expect("an inlining region");
    for register in 0..frame.registers {
        let value = match frame.bound.get(usize::from(register)).copied().flatten() {
            Some(source) => r.get(source),
            None if region.wrote_back(pc, frame.base + register) => {
                // SAFETY: native code wrote back the registers live where it left.
                unsafe { entry.scratch_value(inlining, frame.base + register) }
            }
            None => Value::UNDEFINED,
        };
        registers.set(register, value);
    }
    // SAFETY: the activation's view holds a record for each of its function's PCs.
    unsafe { (*cx).base.add(callee_pc as usize) }
}

script_handler! {
    /// `JumpFalse` of a value that decides its own truthiness (`Value::truthiness`); a heap
    /// value whose cell decides it takes `lane_jump_test_cell`.
    fn lane_jump_false<BACKWARD: bool>(cx, r, ip, i, view) {
        let Some(truthy) = r.get(i.a).truthiness() else {
            sibling!(lane_jump_test_cell::<H, BACKWARD, false>, r, ip, Carried::script(cx))
        };
        if !truthy {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `JumpTrue` as `lane_jump_false`: ToBoolean runs no guest code.
    fn lane_jump_true<BACKWARD: bool>(cx, r, ip, i, view) {
        let Some(truthy) = r.get(i.a).truthiness() else {
            sibling!(lane_jump_test_cell::<H, BACKWARD, true>, r, ip, Carried::script(cx))
        };
        if truthy {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `JumpFalse` (`WHEN` false) or `JumpTrue` (`WHEN` true) of a heap value whose cell
    /// decides its truthiness; out of line, so the common tests make no call.
    #[inline(never)]
    fn lane_jump_test_cell<BACKWARD: bool, WHEN: bool>(cx, r, ip, i, view) {
        if unsafe { vm(cx) }.heap_cell_truthy(r.get(i.a)) == WHEN {
            return lane_jump(cx, r, ip, BACKWARD, Carried::script(cx), None);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

lane_handler! {
    /// Take the selected entry's jump; the view admits a table only when
    /// every entry is a lane jump.
    fn lane_branch_table<BACKWARD: bool>(cx, r, ip, i, view) {
        let case = r.get(i.a).wasm_bits32().min(i.imm());
        // SAFETY: validation places `imm + 1` entries after the table.
        let entry = unsafe { ip.add(1 + case as usize) };
        lane_jump(cx, r, entry, BACKWARD, view, None)
    }
}

lane_handler! {
    fn lane_bit_field<ACC: u8>(cx, r, ip, i, view) {
        let value = crate::wasm::integer::shift_right_unsigned_and(
            read_i32::<ACC, ACC_B>(r, i.b, view),
            i32::from(i.c as i16),
            i.imm() as i32,
        );
        produce!(cx, r, ip, i.a, view.integers.encode(value), view)
    }
}

lane_handler! {
    /// A counted loop's step and test.
    fn lane_add_jump_nonzero<BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let sum = read_i32::<ACC, ACC_B>(r, i.b, view).wrapping_add(i32::from(i.c as i16));
        if sum != 0 {
            return lane_jump(cx, r, ip, BACKWARD, view, Some((i.a, view.integers.encode(sum))));
        }
        // Falling through, the step is an accumulator producer.
        produce!(cx, r, ip, i.a, view.integers.encode(sum), view)
    }
}

lane_handler! {
    /// `WasmJumpI32Zero` and `WasmJumpI32NonZero`. With `CARRY`, the taken
    /// jump carries register `c` (a field the instruction leaves unused) as
    /// the accumulator, for a target that reads it there.
    fn lane_jump_i32_zero<WHEN_ZERO: bool, BACKWARD: bool, ACC: u8, CARRY: bool>(cx, r, ip, i, view) {
        if (read_i32::<ACC, ACC_A>(r, i.a, view) == 0) == WHEN_ZERO {
            let view = if CARRY { Carried { acc: Slot::of(r.get(i.c)), ..view } } else { view };
            return lane_jump(cx, r, ip, BACKWARD, view, None);
        }
        next!(cx, r, following(ip), view)
    }
}

/// The zero-test jumps, indexed by whether they jump on zero, direction and
/// accumulator position; `CARRY` as in `lane_jump_i32_zero`.
struct ZeroJumps<H, const CARRY: bool>(std::marker::PhantomData<H>);

impl<H: Host, const CARRY: bool> ZeroJumps<H, CARRY> {
    const HANDLERS: [[[Handler; ACC_POSITIONS]; 2]; 2] = {
        macro_rules! positions {
            ($zero:literal, $backward:literal) => {
                [
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_NONE, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_A, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_B, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_C, CARRY>,
                    lane_jump_i32_zero::<H, $zero, $backward, ACC_PAIR, CARRY>,
                ]
            };
        }
        [
            [positions!(false, false), positions!(false, true)],
            [positions!(true, false), positions!(true, true)],
        ]
    };
}

lane_handler! {
    /// Fused i32 comparison and branch, against a register or a constant.
    fn lane_compare_jump<OP: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let (comparison, immediate) = const {
            match I32BinaryOperator::from_jump_op(opcode(OP)) {
                Some(comparison) => (comparison, false),
                None => match I32BinaryOperator::from_immediate_jump_op(opcode(OP)) {
                    Some(comparison) => (comparison, true),
                    None => panic!("not an i32 comparison jump"),
                },
            }
        };
        let right = if immediate {
            i32::from(i.b as i16)
        } else {
            read_i32::<ACC, ACC_B>(r, i.b, view)
        };
        let Ok(taken) = comparison.evaluate(read_i32::<ACC, ACC_A>(r, i.a, view), right) else {
            return exit(ip);
        };
        if taken != 0 {
            return lane_jump(cx, r, ip, BACKWARD, view, None);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// Load an instance binding; loading the function's memory register
    /// resolves the memory view.
    fn lane_instance_binding<MEMORY: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let env = vm.frames[unsafe { (*cx).frame }].env;
        let Some(value) = vm.heap.environment_slot(env, i.imm() as usize) else {
            return exit(ip);
        };
        r.set(i.a, value);
        let view = if MEMORY {
            let memory = resolve_memory(cx, value);
            unsafe { (*cx).memory = memory };
            Carried { memory, ..view }
        } else {
            view
        };
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A Wasm effective address: the index operand in the memory's index type plus the
    /// record's constant offset, as an i64. An overflow, which traps, or an operand of another
    /// representation takes the general path.
    fn lane_wasm_memory_address<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (Ok(address), Some(offset)) = (
            vm.wasm_memory_index(r.get(i.c), r.get(i.b)),
            vm.decode_bits64(i.constant()),
        ) else {
            return exit(ip);
        };
        let Some(effective) = address.checked_add(offset) else {
            return exit(ip);
        };
        let value = vm.encode_bits64(effective);
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A generic Wasm memory load (`crate::wasm::memory::MemoryLoad`), as vectors and the
    /// accesses no direct op names take: in place when it reads in bounds; a trap, or an
    /// operand of another representation, takes the general path, which raises it.
    fn lane_wasm_memory_load<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(operator) = crate::wasm::memory::MemoryLoad::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Ok(address) = vm.wasm_i64_operand(r.get(i.c)) else {
            return exit(ip);
        };
        let loaded = match vm.wasm_memory_bytes(r.get(i.b)) {
            Ok(bytes) => operator.read(&bytes, address as u64),
            Err(_) => return exit(ip),
        };
        let Ok(value) = loaded else {
            return exit(ip);
        };
        let value = vm.encode_wasm_value(value);
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A generic Wasm memory store (`crate::wasm::memory::MemoryStore`), in place as
    /// `lane_wasm_memory_load` reads; a store checks its bounds before writing, so one that
    /// traps leaves memory as it was for the general path to raise.
    fn lane_wasm_memory_store<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(operator) = crate::wasm::memory::MemoryStore::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Ok(address) = vm.wasm_i64_operand(r.get(i.c)) else {
            return exit(ip);
        };
        if vm.wasm_memory_store(r.get(i.b), address as u64, operator, r.get(i.a)).is_err() {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

/// A SIMD operand of static type `ty`: a vector or an i32 read from its slot directly, any
/// other scalar as `Vm::decode_wasm_value` reads it. `None` for another representation.
#[inline(always)]
fn lane_simd_operand<H: Host>(vm: &Vm<H>, value: Value, ty: crate::WasmType) -> Option<crate::WasmValue> {
    match ty {
        crate::WasmType::V128 => Some(crate::WasmValue::V128(u128::from_le_bytes(vm.heap.v128(value)?))),
        crate::WasmType::I32 => Some(crate::WasmValue::I32(value.as_int()?)),
        _ => vm.decode_wasm_value(value, ty).ok(),
    }
}

lane_handler! {
    /// A Wasm SIMD operator (`crate::wasm::simd::SimdOperator`), which neither traps nor runs
    /// guest code: its operands decoded by their static types and its result encoded, a
    /// vector in a fresh cell (`Vm::encode_wasm_value`). An operand of another
    /// representation takes the general path, which raises its validation error.
    fn lane_wasm_simd<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some((kernel, lane)) = crate::wasm::simd::SimdKernel::of(i.imm()) else {
            return exit(ip);
        };
        let Some(left) = lane_simd_operand(vm, r.get(i.b), kernel.left) else {
            return exit(ip);
        };
        let right = match kernel.right {
            None => None,
            Some(ty) => match lane_simd_operand(vm, r.get(i.c), ty) {
                Some(right) => Some(right),
                None => return exit(ip),
            },
        };
        let value = match (kernel.apply)(left, right, lane) {
            crate::WasmValue::V128(bits) => vm.heap.alloc_v128(bits.to_le_bytes()),
            value => vm.encode_wasm_value(value),
        };
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// `i8x16.shuffle` of two vectors by the record's constant lane indices, as
    /// `lane_wasm_simd` runs an operator.
    fn lane_wasm_simd_shuffle<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(indices) = vm.heap.v128(i.constant()) else {
            return exit(ip);
        };
        let (Some(left), Some(right)) = (vm.heap.v128(r.get(i.b)), vm.heap.v128(r.get(i.c))) else {
            return exit(ip);
        };
        let (left, right) = (u128::from_le_bytes(left), u128::from_le_bytes(right));
        let value = vm.encode_wasm_value(crate::WasmValue::V128(crate::wasm::simd::shuffle(left, right, indices)));
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_global_get<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal { value, .. }) = unsafe { vm(cx) }.heap.get(r.get(i.b)) else {
            return exit(ip);
        };
        r.set(i.a, *value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_global_set<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal {
            value,
            mutable: true,
            ..
        }) = unsafe { vm(cx) }.heap.get_mut(r.get(i.b))
        else {
            return exit(ip);
        };
        *value = r.get(i.a);
        next!(cx, r, following(ip), view)
    }
}

/// `if condition { chosen } else { other }` of two loaded values, picked after both load, so
/// neither load waits on the condition; the compiler would otherwise load once from a picked
/// register address, a load the condition delays.
#[inline(always)]
fn select_loaded(condition: bool, chosen: Value, other: Value) -> Value {
    #[cfg(target_arch = "x86_64")]
    {
        let mut picked = other.to_bits();
        // SAFETY: a register-only conditional move; no memory or stack.
        unsafe {
            std::arch::asm!(
                "test {condition:e}, {condition:e}",
                "cmovnz {picked}, {chosen}",
                condition = in(reg) u32::from(condition),
                chosen = in(reg) chosen.to_bits(),
                picked = inout(reg) picked,
                options(pure, nomem, nostack),
            );
        }
        Value::from_bits(picked)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        std::hint::select_unpredictable(condition, chosen, other)
    }
}

lane_handler! {
    fn lane_select<ACC: u8>(cx, r, ip, i, view) {
        let (condition, _) = ImmediateLayout::register_pair(i.imm());
        let condition = if ACC == ACC_PAIR {
            // SAFETY: as for `read`.
            unsafe { view.acc.get() }.wasm_bits32() as i32
        } else {
            r.i32(condition)
        };
        let value = select_loaded(condition != 0, r.get(i.b), r.get(i.c));
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    fn lane_fill_registers<>(cx, r, ip, i, view) {
        let value = i.constant();
        for offset in 0..i.c {
            r.set(i.b + offset, value);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    fn lane_i32_unary<ACC: u8>(cx, r, ip, i, view) {
        let Some(operator) = I32UnaryOperator::from_tag(i.imm()) else {
            return exit(ip);
        };
        let crate::WasmValue::I32(value) = operator.apply(read_i32::<ACC, ACC_B>(r, i.b, view)) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, view.integers.encode(value), view)
    }
}

lane_handler! {
    /// An i32 operator over two registers or a register and a constant; a
    /// trap leaves the lane.
    fn lane_i32_binary<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let (operator, immediate) = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => (operator, false),
                None => match I32BinaryOperator::from_immediate_op(opcode(OP)) {
                    Some(operator) => (operator, true),
                    None => panic!("not an i32 binary operator"),
                },
            }
        };
        let right = if immediate {
            i.imm() as i32
        } else {
            read_i32::<ACC, ACC_C>(r, i.c, view)
        };
        let Ok(value) = operator.evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), right) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, view.integers.encode(value), view)
    }
}

/// A scalar of static Wasm type `ty` read from a slot value.
#[inline(always)]
fn lane_scalar(value: Value, ty: crate::WasmType) -> Option<crate::WasmValue> {
    Some(match ty {
        crate::WasmType::I32 => crate::WasmValue::I32(value.as_int()?),
        crate::WasmType::F32 => crate::WasmValue::F32(value.as_int()? as u32),
        crate::WasmType::I64 => crate::WasmValue::I64(value.bits64()? as i64),
        crate::WasmType::F64 => crate::WasmValue::F64(value.bits64()?),
        _ => return None,
    })
}

/// A numeric scalar as a slot value.
#[inline(always)]
fn lane_encode(view: Carried, value: crate::WasmValue) -> Option<Value> {
    Some(match value {
        crate::WasmValue::I32(value) => view.integers.encode(value),
        crate::WasmValue::F32(bits) => view.integers.encode(bits as i32),
        crate::WasmValue::I64(value) => Value::from_bits64(value as u64)?,
        crate::WasmValue::F64(bits) => Value::from_bits64(bits)?,
        _ => return None,
    })
}

lane_handler! {
    /// An f32 or f64 operator: each width's selector applies its own
    /// semantics (`crate::wasm::float`).
    fn lane_float_binary<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let left = read::<ACC, ACC_B>(r, i.b, view);
        let right = read::<ACC, ACC_C>(r, i.c, view);
        let value = if const { matches!(opcode(OP), Op::WasmF64Binary) } {
            let (Some(left), Some(right), Some(operator)) = (
                left.bits64(),
                right.bits64(),
                F64BinaryOperator::from_tag(i.imm()),
            ) else {
                return exit(ip);
            };
            operator.apply(f64::from_bits(left), f64::from_bits(right))
        } else {
            let (Some(left), Some(right), Some(operator)) = (
                left.as_int(),
                right.as_int(),
                F32BinaryOperator::from_tag(i.imm()),
            ) else {
                return exit(ip);
            };
            operator.apply(f32::from_bits(left as u32), f32::from_bits(right as u32))
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

/// The float operators a lane record runs as one hardware operation or
/// comparison, by index: the operator is part of the handler, not a
/// selector it decodes.
const FLOAT_ADD: u8 = 0;
const FLOAT_SUBTRACT: u8 = 1;
const FLOAT_MULTIPLY: u8 = 2;
const FLOAT_DIVIDE: u8 = 3;
/// Comparisons, whose result is an i32 truth value.
const FLOAT_EQUAL: u8 = 4;
const FLOAT_NOT_EQUAL: u8 = 5;
const FLOAT_LESS: u8 = 6;
const FLOAT_GREATER: u8 = 7;
const FLOAT_LESS_EQUAL: u8 = 8;
const FLOAT_GREATER_EQUAL: u8 = 9;
const FLOAT_ARITHMETIC: usize = 10;

/// The arithmetic index of a float binary selector, when it has one.
fn float_arithmetic(op: Op, selector: u32) -> Option<u8> {
    Some(match op {
        Op::WasmF64Binary => match F64BinaryOperator::from_tag(selector)? {
            F64BinaryOperator::Add => FLOAT_ADD,
            F64BinaryOperator::Subtract => FLOAT_SUBTRACT,
            F64BinaryOperator::Multiply => FLOAT_MULTIPLY,
            F64BinaryOperator::Divide => FLOAT_DIVIDE,
            F64BinaryOperator::Equal => FLOAT_EQUAL,
            F64BinaryOperator::NotEqual => FLOAT_NOT_EQUAL,
            F64BinaryOperator::Less => FLOAT_LESS,
            F64BinaryOperator::Greater => FLOAT_GREATER,
            F64BinaryOperator::LessEqual => FLOAT_LESS_EQUAL,
            F64BinaryOperator::GreaterEqual => FLOAT_GREATER_EQUAL,
            _ => return None,
        },
        Op::WasmF32Binary => match F32BinaryOperator::from_tag(selector)? {
            F32BinaryOperator::Add => FLOAT_ADD,
            F32BinaryOperator::Subtract => FLOAT_SUBTRACT,
            F32BinaryOperator::Multiply => FLOAT_MULTIPLY,
            F32BinaryOperator::Divide => FLOAT_DIVIDE,
            F32BinaryOperator::Equal => FLOAT_EQUAL,
            F32BinaryOperator::NotEqual => FLOAT_NOT_EQUAL,
            F32BinaryOperator::Less => FLOAT_LESS,
            F32BinaryOperator::Greater => FLOAT_GREATER,
            F32BinaryOperator::LessEqual => FLOAT_LESS_EQUAL,
            F32BinaryOperator::GreaterEqual => FLOAT_GREATER_EQUAL,
            _ => return None,
        },
        _ => return None,
    })
}

lane_handler! {
    /// Float arithmetic `ARITH` of width f64 (`WIDE`) or f32, with the
    /// operators' own semantics (`crate::wasm::float`): the hardware result.
    fn lane_float_arithmetic<WIDE: bool, ARITH: u8, ACC: u8>(cx, r, ip, i, view) {
        let left = read::<ACC, ACC_B>(r, i.b, view);
        let right = read::<ACC, ACC_C>(r, i.c, view);
        /// The arithmetic result, or a comparison's truth value.
        enum Outcome<F> {
            Float(F),
            Truth(bool),
        }
        macro_rules! apply {
            ($left:expr, $right:expr) => {{
                let (left, right) = ($left, $right);
                match ARITH {
                    FLOAT_ADD => Outcome::Float(left + right),
                    FLOAT_SUBTRACT => Outcome::Float(left - right),
                    FLOAT_MULTIPLY => Outcome::Float(left * right),
                    FLOAT_DIVIDE => Outcome::Float(left / right),
                    FLOAT_EQUAL => Outcome::Truth(left == right),
                    FLOAT_NOT_EQUAL => Outcome::Truth(left != right),
                    FLOAT_LESS => Outcome::Truth(left < right),
                    FLOAT_GREATER => Outcome::Truth(left > right),
                    FLOAT_LESS_EQUAL => Outcome::Truth(left <= right),
                    _ => Outcome::Truth(left >= right),
                }
            }};
        }
        // Registers hold their static Wasm types: an f32 slot an encoded
        // i32, an f64 slot its bits in place or a box whose bits read as a
        // NaN, which these operators treat as any other NaN.
        let value = if WIDE {
            let left = f64::from_bits(left.float_bits64());
            let right = f64::from_bits(right.float_bits64());
            match apply!(left, right) {
                Outcome::Float(result) => Value::from_float_bits64(result.to_bits()),
                Outcome::Truth(truth) => view.integers.encode(i32::from(truth)),
            }
        } else {
            let left = f32::from_bits(left.wasm_bits32());
            let right = f32::from_bits(right.wasm_bits32());
            match apply!(left, right) {
                Outcome::Float(result) => view.integers.encode(result.to_bits() as i32),
                Outcome::Truth(truth) => view.integers.encode(i32::from(truth)),
            }
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

/// The f64 operator `ARITH` over two operands, as `lane_float_arithmetic`
/// applies it: the hardware result, or a comparison's truth value.
#[inline(always)]
fn f64_operation(arith: u8, left: f64, right: f64) -> Result<f64, bool> {
    match arith {
        FLOAT_ADD => Ok(left + right),
        FLOAT_SUBTRACT => Ok(left - right),
        FLOAT_MULTIPLY => Ok(left * right),
        FLOAT_DIVIDE => Ok(left / right),
        FLOAT_EQUAL => Err(left == right),
        FLOAT_NOT_EQUAL => Err(left != right),
        FLOAT_LESS => Err(left < right),
        FLOAT_GREATER => Err(left > right),
        FLOAT_LESS_EQUAL => Err(left <= right),
        _ => Err(left >= right),
    }
}

/// Which operand of the second fused float record the first one's result
/// is: its left (`b`) or right (`c`) register.
const FUSED_LEFT: bool = true;

lane_handler! {
    /// An f64 arithmetic `FIRST` whose result the next record, f64 operator
    /// `SECOND`, takes as its left (`LEFT`) or right operand: both records
    /// run here, each result written as its own record writes it.
    fn lane_f64_pair<FIRST: u8, SECOND: u8, LEFT: bool, ACC: u8>(cx, r, ip, i, view) {
        let first = match f64_operation(
            FIRST,
            f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64()),
            f64::from_bits(read::<ACC, ACC_C>(r, i.c, view).float_bits64()),
        ) {
            Ok(result) => result,
            Err(_) => unreachable!("a fused first operator is arithmetic"),
        };
        r.set(i.a, Value::from_float_bits64(first.to_bits()));
        let next = following(ip);
        // SAFETY: the view fuses a float record only with a following one.
        let second = unsafe { &*next };
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(second.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(second.b).float_bits64()), first)
        };
        let value = match f64_operation(SECOND, left, right) {
            Ok(result) => Value::from_float_bits64(result.to_bits()),
            Err(truth) => view.integers.encode(i32::from(truth)),
        };
        produce!(cx, r, next, second.a, value, view)
    }
}

lane_handler! {
    /// An f64 load whose value the next record, f64 operator `SECOND`,
    /// takes as its left (`LEFT`) or right operand: both records run here.
    /// Bits that need a box leave the lane at the load.
    fn lane_f64_load_pair<SECOND: u8, LEFT: bool>(cx, r, ip, i, view) {
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::F64(bits)) = (unsafe { view.memory.read(MemoryLoad::F64Load, lane_address::<ACC_NONE>(r, i, view)) })
        else {
            return exit(ip);
        };
        let Some(loaded) = Value::from_bits64(bits) else {
            return exit(ip);
        };
        r.set(i.a, loaded);
        let next = following(ip);
        // SAFETY: the view fuses a load only with a following record.
        let second = unsafe { &*next };
        let first = f64::from_bits(bits);
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(second.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(second.b).float_bits64()), first)
        };
        let value = match f64_operation(SECOND, left, right) {
            Ok(result) => Value::from_float_bits64(result.to_bits()),
            Err(truth) => view.integers.encode(i32::from(truth)),
        };
        produce!(cx, r, next, second.a, value, view)
    }
}

lane_handler! {
    /// An f64 load, the f64 arithmetic `ARITH` taking the loaded value as
    /// its left (`LEFT`) or right operand, and the f64 store writing the
    /// arithmetic's result (`mem[x] += v`): three records run here, each
    /// result written as its own record writes it. An access the general
    /// path must run exits at its record.
    fn lane_f64_update<ARITH: u8, LEFT: bool>(cx, r, ip, i, view) {
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::F64(bits)) = (unsafe {
            view.memory.read(MemoryLoad::F64Load, lane_address::<ACC_NONE>(r, i, view))
        }) else {
            return exit(ip);
        };
        let Some(loaded) = Value::from_bits64(bits) else {
            return exit(ip);
        };
        r.set(i.a, loaded);
        let operator = following(ip);
        // SAFETY: the view fuses a load only with a following operator and
        // store.
        let o = unsafe { &*operator };
        let first = f64::from_bits(bits);
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(o.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(o.b).float_bits64()), first)
        };
        let Ok(result) = f64_operation(ARITH, left, right) else {
            unreachable!("a fused update's operator is arithmetic");
        };
        let value = Value::from_float_bits64(result.to_bits());
        r.set(o.a, value);
        let store = following(operator);
        // SAFETY: as above.
        let access = unsafe { &*store };
        // SAFETY: as for `lane_store`.
        if unsafe {
            view.memory.write(
                MemoryStore::F64Store,
                lane_address::<ACC_NONE>(r, access, view),
                crate::WasmValue::F64(value.float_bits64()),
            )
        }
        .is_none()
        {
            return exit(store);
        }
        next!(cx, r, following(store), view)
    }
}

lane_handler! {
    /// An f64 arithmetic `ARITH` whose result the next record, an f64
    /// store, writes to memory: both records run here. A store the general
    /// path must run exits at its record, the result already written.
    fn lane_f64_store_pair<ARITH: u8, ACC: u8>(cx, r, ip, i, view) {
        let result = match f64_operation(
            ARITH,
            f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64()),
            f64::from_bits(read::<ACC, ACC_C>(r, i.c, view).float_bits64()),
        ) {
            Ok(result) => result,
            Err(_) => unreachable!("a fused store's operator is arithmetic"),
        };
        let value = Value::from_float_bits64(result.to_bits());
        r.set(i.a, value);
        let next = following(ip);
        // SAFETY: the view fuses an operator only with a following store.
        let store = unsafe { &*next };
        let bits = value.float_bits64();
        // SAFETY: as for `lane_store`.
        if unsafe {
            view.memory.write(
                MemoryStore::F64Store,
                lane_address::<ACC_NONE>(r, store, view),
                crate::WasmValue::F64(bits),
            )
        }
        .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The accumulator fields a fused first float record may read: none, its
/// left operand, or its right operand.
const FUSED_ACC_FIELDS: [u8; 3] = [ACC_NONE, ACC_B, ACC_C];

macro_rules! float_store_pair_handlers {
    ($host:ty, $arith:expr) => {
        [
            lane_f64_store_pair::<$host, $arith, ACC_NONE> as Handler,
            lane_f64_store_pair::<$host, $arith, ACC_B> as Handler,
            lane_f64_store_pair::<$host, $arith, ACC_C> as Handler,
        ]
    };
}

macro_rules! float_load_pair_handlers {
    ($host:ty; $($second:expr),+) => {
        [$([
            lane_f64_load_pair::<$host, $second, false> as Handler,
            lane_f64_load_pair::<$host, $second, FUSED_LEFT> as Handler,
        ]),+]
    };
}

struct FloatPairs<H>(std::marker::PhantomData<H>);

macro_rules! float_pair_handlers {
    ($host:ty) => {
        [
            float_pair_handlers!($host, FLOAT_ADD),
            float_pair_handlers!($host, FLOAT_SUBTRACT),
            float_pair_handlers!($host, FLOAT_MULTIPLY),
            float_pair_handlers!($host, FLOAT_DIVIDE),
        ]
    };
    ($host:ty, $first:expr) => {
        [
            float_pair_handlers!($host, $first, FLOAT_ADD),
            float_pair_handlers!($host, $first, FLOAT_SUBTRACT),
            float_pair_handlers!($host, $first, FLOAT_MULTIPLY),
            float_pair_handlers!($host, $first, FLOAT_DIVIDE),
            float_pair_handlers!($host, $first, FLOAT_EQUAL),
            float_pair_handlers!($host, $first, FLOAT_NOT_EQUAL),
            float_pair_handlers!($host, $first, FLOAT_LESS),
            float_pair_handlers!($host, $first, FLOAT_GREATER),
            float_pair_handlers!($host, $first, FLOAT_LESS_EQUAL),
            float_pair_handlers!($host, $first, FLOAT_GREATER_EQUAL),
        ]
    };
    ($host:ty, $first:expr, $second:expr) => {
        [
            float_pair_handlers!($host, $first, $second, false),
            float_pair_handlers!($host, $first, $second, FUSED_LEFT),
        ]
    };
    ($host:ty, $first:expr, $second:expr, $left:expr) => {
        [
            lane_f64_pair::<$host, $first, $second, $left, ACC_NONE> as Handler,
            lane_f64_pair::<$host, $first, $second, $left, ACC_B> as Handler,
            lane_f64_pair::<$host, $first, $second, $left, ACC_C> as Handler,
        ]
    };
}

/// The f64 arithmetic operators a fused pair can begin with: add,
/// subtract, multiply and divide, whose results are floats.
const FLOAT_PAIR_FIRSTS: usize = 4;

impl<H: Host> FloatPairs<H> {
    /// By first operator, second operator, whether the first result is the
    /// second's left operand, and the first's accumulator field.
    const HANDLERS: [[[[Handler; FUSED_ACC_FIELDS.len()]; 2]; FLOAT_ARITHMETIC];
        FLOAT_PAIR_FIRSTS] = float_pair_handlers!(H);
    /// An f64 load fused with the operator taking its value, by second
    /// operator and whether the value is its left operand.
    /// An f64 arithmetic fused with the store of its result, by operator.
    /// By operator and the operator's accumulator field (none, `b`, `c`).
    const STORES: [[Handler; FUSED_ACC_FIELDS.len()]; FLOAT_PAIR_FIRSTS] = [
        float_store_pair_handlers!(H, FLOAT_ADD),
        float_store_pair_handlers!(H, FLOAT_SUBTRACT),
        float_store_pair_handlers!(H, FLOAT_MULTIPLY),
        float_store_pair_handlers!(H, FLOAT_DIVIDE),
    ];
    /// An f64 load, arithmetic and store of its result, by operator and
    /// whether the loaded value is the left operand.
    const UPDATES: [[Handler; 2]; FLOAT_PAIR_FIRSTS] = [
        [
            lane_f64_update::<H, FLOAT_ADD, false>,
            lane_f64_update::<H, FLOAT_ADD, FUSED_LEFT>,
        ],
        [
            lane_f64_update::<H, FLOAT_SUBTRACT, false>,
            lane_f64_update::<H, FLOAT_SUBTRACT, FUSED_LEFT>,
        ],
        [
            lane_f64_update::<H, FLOAT_MULTIPLY, false>,
            lane_f64_update::<H, FLOAT_MULTIPLY, FUSED_LEFT>,
        ],
        [
            lane_f64_update::<H, FLOAT_DIVIDE, false>,
            lane_f64_update::<H, FLOAT_DIVIDE, FUSED_LEFT>,
        ],
    ];
    const LOADS: [[Handler; 2]; FLOAT_ARITHMETIC] = float_load_pair_handlers!(H;
        FLOAT_ADD, FLOAT_SUBTRACT, FLOAT_MULTIPLY, FLOAT_DIVIDE, FLOAT_EQUAL,
        FLOAT_NOT_EQUAL, FLOAT_LESS, FLOAT_GREATER, FLOAT_LESS_EQUAL, FLOAT_GREATER_EQUAL);
}

struct FloatArithmetic<H>(std::marker::PhantomData<H>);

macro_rules! float_arithmetic_handlers {
    ($host:ty, $wide:expr) => {
        [
            float_arithmetic_handlers!($host, $wide, FLOAT_ADD),
            float_arithmetic_handlers!($host, $wide, FLOAT_SUBTRACT),
            float_arithmetic_handlers!($host, $wide, FLOAT_MULTIPLY),
            float_arithmetic_handlers!($host, $wide, FLOAT_DIVIDE),
            float_arithmetic_handlers!($host, $wide, FLOAT_EQUAL),
            float_arithmetic_handlers!($host, $wide, FLOAT_NOT_EQUAL),
            float_arithmetic_handlers!($host, $wide, FLOAT_LESS),
            float_arithmetic_handlers!($host, $wide, FLOAT_GREATER),
            float_arithmetic_handlers!($host, $wide, FLOAT_LESS_EQUAL),
            float_arithmetic_handlers!($host, $wide, FLOAT_GREATER_EQUAL),
        ]
    };
    ($host:ty, $wide:expr, $arith:expr) => {
        [
            lane_float_arithmetic::<$host, $wide, $arith, ACC_NONE> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_A> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_B> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_C> as Handler,
            lane_float_arithmetic::<$host, $wide, $arith, ACC_PAIR> as Handler,
        ]
    };
}

impl<H: Host> FloatArithmetic<H> {
    /// By width (f32, then f64), arithmetic index and accumulator field.
    const HANDLERS: [[[Handler; ACC_POSITIONS]; FLOAT_ARITHMETIC]; 2] = [
        float_arithmetic_handlers!(H, false),
        float_arithmetic_handlers!(H, true),
    ];
}

lane_handler! {
    /// f64 square root: an arithmetic operator, so a NaN operand matters
    /// only as a NaN (see `float_bits64`), and its result is held in place.
    fn lane_f64_sqrt<ACC: u8>(cx, r, ip, i, view) {
        let input = f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64());
        produce!(cx, r, ip, i.a, Value::from_float_bits64(input.sqrt().to_bits()), view)
    }
}

lane_handler! {
    fn lane_float_unary<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let input = read::<ACC, ACC_B>(r, i.b, view);
        let value = if const { matches!(opcode(OP), Op::WasmF64Unary) } {
            let (Some(input), Some(operator)) =
                (input.bits64(), F64UnaryOperator::from_tag(i.imm()))
            else {
                return exit(ip);
            };
            operator.apply(f64::from_bits(input))
        } else {
            let (Some(input), Some(operator)) =
                (input.as_int(), F32UnaryOperator::from_tag(i.imm()))
            else {
                return exit(ip);
            };
            operator.apply(f32::from_bits(input as u32))
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    /// An i64 operator; a trap leaves the lane, where the general loop
    /// raises it.
    fn lane_i64_binary<ACC: u8>(cx, r, ip, i, view) {
        let (Some(left), Some(right), Some(operator)) = (
            read::<ACC, ACC_B>(r, i.b, view).bits64(),
            read::<ACC, ACC_C>(r, i.c, view).bits64(),
            I64BinaryOperator::from_tag(i.imm()),
        ) else {
            return exit(ip);
        };
        let Ok(value) = operator.apply(left as i64, right as i64) else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    fn lane_i64_unary<ACC: u8>(cx, r, ip, i, view) {
        let (Some(input), Some(operator)) = (
            read::<ACC, ACC_B>(r, i.b, view).bits64(),
            I64UnaryOperator::from_tag(i.imm()),
        ) else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, operator.apply(input as i64)) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    /// A scalar conversion; a trapping truncation leaves the lane.
    fn lane_scalar_convert<ACC: u8>(cx, r, ip, i, view) {
        let Some(operator) = ScalarConversionOperator::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Some(input) = lane_scalar(read::<ACC, ACC_B>(r, i.b, view), operator.source_type())
        else {
            return exit(ip);
        };
        let Ok(value) = operator.apply(input) else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

/// The memory32 effective address of a direct access through the function's
/// memory register.
#[inline(always)]
fn lane_address<const ACC: u8>(r: Registers, i: &LaneInstruction, view: Carried) -> u64 {
    u64::from(read::<ACC, ACC_C>(r, i.c, view).wasm_bits32()) + u64::from(i.imm())
}

/// The view of the unshared memory `binding` names, or the empty view.
/// The bytes and length of the unshared memory `binding` names, when the
/// lane or compiled code can access it directly.
pub(super) fn unshared_memory<H: Host>(vm: &Vm<H>, binding: Value) -> Option<(*mut u8, usize)> {
    let view = memory_view(vm, binding);
    (!view.bytes.is_null()).then_some((view.bytes, view.len()))
}

fn memory_view<H: Host>(vm: &Vm<H>, binding: Value) -> MemoryView {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(binding) else {
        return MemoryView::NONE;
    };
    let MemoryStorage::Unshared(state) = &**bytes else {
        return MemoryView::NONE;
    };
    // An outstanding borrow belongs to a suspended host access; the general
    // path reports it exactly.
    let Ok(mut state) = state.try_borrow_mut() else {
        return MemoryView::NONE;
    };
    MemoryView::new(state.bytes.as_mut_ptr(), state.bytes.len())
}

/// The view of memory binding `binding`, reusing the run's last resolution.
#[inline(always)]
fn resolve_memory<H: Host>(cx: *mut LaneContext<H>, binding: Value) -> MemoryView {
    // SAFETY: handlers run with exclusive access to their context.
    let (resolved, view) = unsafe { (*cx).resolved };
    if resolved == binding {
        return view;
    }
    let view = memory_view(unsafe { vm(cx) }, binding);
    unsafe { (*cx).resolved = (binding, view) };
    view
}

/// The view behind `view`'s memory register in `registers`.
fn frame_memory_view<H: Host>(
    cx: *mut LaneContext<H>,
    view: &LaneView,
    registers: Registers,
) -> MemoryView {
    // A lane run is all JavaScript or all Wasm (`LaneContext::script_calls`, `inline_calls`).
    // SAFETY: the context's program outlives the lane run.
    if unsafe { (*(*cx).code).kind } != crate::bytecode::ProgramKind::Wasm {
        return MemoryView::script(cx);
    }
    view.memory_register.map_or(MemoryView::NONE, |register| {
        resolve_memory(cx, registers.get(register))
    })
}

/// A taken safepoint jump below the stack floor: leave the lane when a
/// collection is due, otherwise dispatch the target from the lane loop,
/// which unwinds the stack, with the accumulator `acc` the jump carries
/// (`LaneContext::resumed_acc`): a loop head whose every edge carries a
/// register reads it there. Out of line and reached by a sibling call, so a
/// safepoint's fast path neither loads the context nor saves registers.
#[cold]
#[inline(never)]
fn lane_safepoint<H: Host>(ip: Ip, target: Ip, acc: Slot<Value>) -> Ip {
    let cx = lane_context::<H>();
    // SAFETY: called only while a lane runs, from one of its handlers.
    if unsafe { vm(cx) }.heap.should_collect() || unsafe { vm(cx) }.execution_interrupt.is_termination_requested() {
        return exit(ip);
    }
    lane_count_backedge::<H>(cx);
    // SAFETY: as above; the lane loop takes it before dispatching `target`.
    unsafe { (*cx).resumed_acc = acc };
    target
}

/// `lane_safepoint` for a jump that writes `register` when taken.
#[cold]
#[inline(never)]
fn lane_safepoint_writing<H: Host>(
    r: Registers,
    ip: Ip,
    target: Ip,
    acc: Slot<Value>,
    register: u16,
    value: Value,
) -> Ip {
    let next = lane_safepoint::<H>(ip, target, acc);
    if next == target {
        r.set(register, value);
    }
    next
}

// The lane holds 64-bit payloads only in place (`Value::from_bits64`): a
// boxed operand or a result that needs a box leaves the lane, and the
// general loop, which boxes, runs the instruction. The handlers then make
// no calls and keep their sibling-call shape.

lane_handler! {
    fn lane_load<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        // SAFETY: the view admits direct accesses only through the memory
        // register, whose bytes `view` holds; see MemoryView.
        let Some(value) = (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    /// An i32 shift left by a constant, reading its operand through field
    /// `ACC`, whose result the next record, an i32 add, takes as its left
    /// (`LEFT`) or right operand: a scaled index. Both records run here,
    /// each result written as its own record writes it.
    fn lane_shift_add<ACC: u8, LEFT: bool>(cx, r, ip, i, view) {
        let Ok(shifted) = I32BinaryOperator::ShiftLeft
            .evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), i.imm() as i32)
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(shifted));
        let next = following(ip);
        // SAFETY: the view fuses a shift only with a following add record.
        let add = unsafe { &*next };
        let other = r.i32(if LEFT { add.c } else { add.b });
        let (left, right) = if LEFT { (shifted, other) } else { (other, shifted) };
        let Ok(sum) = I32BinaryOperator::Add.evaluate(left, right) else {
            return exit(next);
        };
        produce!(cx, r, next, add.a, view.integers.encode(sum), view)
    }
}

lane_handler! {
    /// A direct i32 load `OP` whose value the next record, a branch table,
    /// selects on: both records run here, the value written as the load
    /// writes it. With `ADD`, the record is a plain add of a constant ahead
    /// of the load (`switch (*pc++)`), and the three run here, each result
    /// written as its own record writes it. A load the general path must
    /// run exits at its record.
    fn lane_load_table<OP: u16, ACC: u8, ADD: bool>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        // With `ADD` and `ACC_C`, the accumulator holds the register both
        // the add and the load read.
        let ip = if ADD {
            let base = read_i32::<ACC, ACC_C>(r, i.b, view);
            r.set(i.a, view.integers.encode(base.wrapping_add(i.imm() as i32)));
            following(ip)
        } else {
            ip
        };
        // SAFETY: the view fuses an add only with a following load record.
        let i = unsafe { &*ip };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(selector)) =
            (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(selector));
        let table = following(ip);
        // SAFETY: the view fuses a load only with a following table record.
        let case = (selector as u32).min(unsafe { (*table).imm() });
        // SAFETY: validation places `imm + 1` entries after the table.
        let entry = unsafe { table.add(1 + case as usize) };
        lane_jump(cx, r, entry, false, view, None)
    }
}

lane_handler! {
    /// An i32 add of a constant whose sum the next record, direct load `OP`,
    /// reads as its address: both records run here, the sum written as the
    /// add writes it. A load the general path must run exits at its record.
    fn lane_add_load<OP: u16>(cx, r, ip, i, view) {
        let load = const {
            match MemoryLoad::from_direct_op(opcode(OP)) {
                Some(load) => load,
                None => panic!("not a direct load"),
            }
        };
        let sum = r.i32(i.b).wrapping_add(i.imm() as i32);
        r.set(i.a, view.integers.encode(sum));
        let next = following(ip);
        // SAFETY: the view fuses an add only with a following load record.
        let access = unsafe { &*next };
        let address = u64::from(sum as u32) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, address) }) else {
            return exit(next);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(next);
        };
        produce!(cx, r, next, access.a, value, view)
    }
}

lane_handler! {
    /// A jump absorbing the move (`ADD` false) or i32 add of the constant
    /// `c` (`ADD` true) at its target: the record holds the target's fields
    /// and jumps past it, carrying the result as the target would.
    fn lane_jump_through<ADD: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let value = if ADD {
            view.integers.encode(r.i32(i.b).wrapping_add(i32::from(i.c as i16)))
        } else {
            r.get(i.b)
        };
        // A safepoint exit resumes at the jump, which runs the target
        // again, so the result is written only when the jump is taken.
        lane_jump(cx, r, ip, BACKWARD, Carried { acc: Slot::of(value), ..view }, Some((i.a, value)))
    }
}

lane_handler! {
    /// Two moves in a row (a parallel copy resolved in order): both
    /// records run here, the second's value carried as its record carries
    /// it. With `JUMP`, the unconditional jump after them runs here too, in
    /// direction `BACKWARD`.
    fn lane_move_pair<JUMP: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        r.set(i.a, r.get(i.b));
        let next = following(ip);
        // SAFETY: the view fuses a move only with a following move, and
        // with `JUMP` a jump after it.
        let second = unsafe { &*next };
        let value = r.get(second.b);
        if JUMP {
            r.set(second.a, value);
            let view = Carried { acc: Slot::of(value), ..view };
            return lane_jump(cx, r, following(next), BACKWARD, view, None);
        }
        produce!(cx, r, next, second.a, value, view)
    }
}

lane_handler! {
    /// A move, reading its source through field `ACC`, whose next record is a zero-test jump
    /// (`lane_jump_i32_zero` without a carried register), as at a loop's latch: both records
    /// run here, the move's value carried as its own record carries it. The jump reads its
    /// operand from its register, which the move has written if it names it.
    fn lane_move_jump_i32_zero<WHEN_ZERO: bool, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let value = read::<ACC, ACC_B>(r, i.b, view);
        r.set(i.a, value);
        let view = Carried { acc: Slot::of(value), ..view };
        let next = following(ip);
        // SAFETY: the view fuses a move only with a following zero-test jump.
        let jump = unsafe { &*next };
        if (r.i32(jump.a) == 0) == WHEN_ZERO {
            return lane_jump(cx, r, next, BACKWARD, view, None);
        }
        next!(cx, r, following(next), view)
    }
}

/// The moves fused with a following zero-test jump, indexed by whether the jump is taken on
/// zero, its direction, and whether the move reads the accumulator.
struct MoveZeroJumps<H>(std::marker::PhantomData<H>);

impl<H: Host> MoveZeroJumps<H> {
    const HANDLERS: [[[Handler; 2]; 2]; 2] = {
        macro_rules! moves {
            ($zero:literal, $backward:literal) => {
                [
                    lane_move_jump_i32_zero::<H, $zero, $backward, ACC_NONE>,
                    lane_move_jump_i32_zero::<H, $zero, $backward, ACC_B>,
                ]
            };
        }
        [[moves!(false, false), moves!(false, true)], [moves!(true, false), moves!(true, true)]]
    };
}

lane_handler! {
    /// A move (`ADD` false) or an i32 add of a constant (`ADD` true) whose
    /// next record is an unconditional jump, as at a loop latch: both
    /// records run here, the result written as its own record writes it.
    fn lane_then_jump<ADD: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let value = if ADD {
            view.integers.encode(r.i32(i.b).wrapping_add(i.imm() as i32))
        } else {
            r.get(i.b)
        };
        r.set(i.a, value);
        lane_jump(cx, r, following(ip), BACKWARD, Carried { acc: Slot::of(value), ..view }, None)
    }
}

/// The most records before a record that one fused record can run through:
/// an f64 load, arithmetic and store (`FloatPairs::UPDATES`) start two
/// records ahead of the last they run.
const FUSION_REACH: usize = 2;

/// The handlers fusing a move or an add of a constant with a jump, indexed
/// by whether the instruction adds and by the jump's direction.
struct JumpFusions<H>(std::marker::PhantomData<H>);

impl<H: Host> JumpFusions<H> {
    /// A jump absorbing the instruction at its target.
    const THROUGH: [[Handler; 2]; 2] = [
        [
            lane_jump_through::<H, false, false>,
            lane_jump_through::<H, false, true>,
        ],
        [
            lane_jump_through::<H, true, false>,
            lane_jump_through::<H, true, true>,
        ],
    ];
    /// The instruction followed by its next record's jump.
    const THEN: [[Handler; 2]; 2] = [
        [
            lane_then_jump::<H, false, false>,
            lane_then_jump::<H, false, true>,
        ],
        [
            lane_then_jump::<H, true, false>,
            lane_then_jump::<H, true, true>,
        ],
    ];
}

/// The scaled-index handlers, indexed by the shift's operand field (none,
/// `b`) and by whether the add takes the shifted value on its left.
struct ScaledIndex<H>(std::marker::PhantomData<H>);

impl<H: Host> ScaledIndex<H> {
    const HANDLERS: [[Handler; 2]; 2] = [
        [
            lane_shift_add::<H, ACC_NONE, false>,
            lane_shift_add::<H, ACC_NONE, true>,
        ],
        [
            lane_shift_add::<H, ACC_B, false>,
            lane_shift_add::<H, ACC_B, true>,
        ],
    ];
}

lane_handler! {
    /// An i32 add of a constant, the f64 load reading its sum, and the f64
    /// operator `SECOND` taking the loaded value as its left (`LEFT`) or
    /// right operand: three records run here, each result written as its
    /// own record writes it. Bits that need a box leave at the load.
    fn lane_add_load_f64<SECOND: u8, LEFT: bool>(cx, r, ip, i, view) {
        let sum = r.i32(i.b).wrapping_add(i.imm() as i32);
        r.set(i.a, view.integers.encode(sum));
        let load = following(ip);
        // SAFETY: the view fuses an add only with a following load and
        // float operator.
        let access = unsafe { &*load };
        let address = u64::from(sum as u32) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::F64(bits)) =
            (unsafe { view.memory.read(MemoryLoad::F64Load, address) })
        else {
            return exit(load);
        };
        let Some(loaded) = Value::from_bits64(bits) else {
            return exit(load);
        };
        r.set(access.a, loaded);
        let next = following(load);
        let second = unsafe { &*next };
        let first = f64::from_bits(bits);
        let (left, right) = if LEFT {
            (first, f64::from_bits(r.get(second.c).float_bits64()))
        } else {
            (f64::from_bits(r.get(second.b).float_bits64()), first)
        };
        let value = match f64_operation(SECOND, left, right) {
            Ok(result) => Value::from_float_bits64(result.to_bits()),
            Err(truth) => view.integers.encode(i32::from(truth)),
        };
        produce!(cx, r, next, second.a, value, view)
    }
}

macro_rules! add_load_f64_handlers {
    ($host:ty; $($second:expr),+) => {
        [$([
            lane_add_load_f64::<$host, $second, false> as Handler,
            lane_add_load_f64::<$host, $second, FUSED_LEFT> as Handler,
        ]),+]
    };
}

struct AddLoadF64<H>(std::marker::PhantomData<H>);

impl<H: Host> AddLoadF64<H> {
    /// By the float operator and whether the loaded value is its left
    /// operand.
    const HANDLERS: [[Handler; 2]; FLOAT_ARITHMETIC] = add_load_f64_handlers!(H;
        FLOAT_ADD, FLOAT_SUBTRACT, FLOAT_MULTIPLY, FLOAT_DIVIDE, FLOAT_EQUAL,
        FLOAT_NOT_EQUAL, FLOAT_LESS, FLOAT_GREATER, FLOAT_LESS_EQUAL, FLOAT_GREATER_EQUAL);
}

/// The fused add-and-load handler for direct load `op`.
/// The fused handler for direct load `load`, with address field `acc`, and
/// direct store `store` writing its value: copies of one value type.
fn load_store_handler<H: Host>(load: Op, store: Op, acc: u8) -> Option<Handler> {
    macro_rules! copies {
        ($(($load:ident, $store:ident)),+) => {
            match (load, store, acc) {
                $(
                    (Op::$load, Op::$store, ACC_NONE) => Some(lane_load_store::<H, { Op::$load as u16 }, { Op::$store as u16 }, ACC_NONE> as Handler),
                    (Op::$load, Op::$store, ACC_C) => Some(lane_load_store::<H, { Op::$load as u16 }, { Op::$store as u16 }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    copies!(
        (WasmI32Load, WasmI32Store),
        (WasmI32Load8U, WasmI32Store8),
        (WasmI64Load, WasmI64Store)
    )
}

/// The fused handler for i32 load `op`, with address field `acc`, the
/// branch table selecting on its value, and with `add` the plain add of a
/// constant ahead of the load.
fn load_table_handler<H: Host>(op: Op, acc: u8, add: bool) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op, acc, add) {
                $(
                    (Op::$load, ACC_NONE, false) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_NONE, false> as Handler),
                    (Op::$load, ACC_C, false) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_C, false> as Handler),
                    (Op::$load, ACC_NONE, true) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_NONE, true> as Handler),
                    (Op::$load, ACC_C, true) => Some(lane_load_table::<H, { Op::$load as u16 }, ACC_C, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    loads!(
        WasmI32Load,
        WasmI32Load8S,
        WasmI32Load8U,
        WasmI32Load16S,
        WasmI32Load16U
    )
}

const fn add_load_handler<H: Host>(op: Op) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match op {
                $(Op::$load => Some(lane_add_load::<H, { Op::$load as u16 }> as Handler),)+
                _ => None,
            }
        };
    }
    loads!(
        WasmI32Load,
        WasmI64Load,
        WasmF32Load,
        WasmF64Load,
        WasmI32Load8S,
        WasmI32Load8U,
        WasmI32Load16S,
        WasmI32Load16U,
        WasmI64Load8S,
        WasmI64Load8U,
        WasmI64Load16S,
        WasmI64Load16U,
        WasmI64Load32S,
        WasmI64Load32U
    )
}

lane_handler! {
    /// A direct load `LOAD`, with address field `ACC`, whose value the next
    /// record, direct store `STORE`, writes: both records run here, the value
    /// written to its register as the load writes it. An access the general
    /// path must run exits at its record.
    fn lane_load_store<LOAD: u16, STORE: u16, ACC: u8>(cx, r, ip, i, view) {
        let (load, store) = const {
            match (
                MemoryLoad::from_direct_op(opcode(LOAD)),
                MemoryStore::from_direct_op(opcode(STORE)),
            ) {
                (Some(load), Some(store)) => (load, store),
                _ => panic!("not a direct load and store"),
            }
        };
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        let Some(encoded) = lane_encode(view, value) else {
            return exit(ip);
        };
        r.set(i.a, encoded);
        let next = following(ip);
        // SAFETY: the view fuses a load only with a following store record.
        let access = unsafe { &*next };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, access, view), value) }
            .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The direct load an instantiation serves.
const fn direct_load(op: u16) -> MemoryLoad {
    match MemoryLoad::from_direct_op(opcode(op)) {
        Some(load) => load,
        None => panic!("not a direct load"),
    }
}

lane_handler! {
    /// An i32 load, with address field `ACC`, whose value the next record, direct i32 load
    /// `SECOND`, takes as its address (a pointer chase, `p->field->member`): both records run
    /// here, each value written as its own record writes it. An access the general path must
    /// run exits at its record.
    fn lane_load_load<SECOND: u16, ACC: u8>(cx, r, ip, i, view) {
        let first = const { direct_load(Op::WasmI32Load as u16) };
        let second = const { direct_load(SECOND) };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(pointer)) =
            (unsafe { view.memory.read(first, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(pointer));
        let next = following(ip);
        // SAFETY: the view fuses a load only with a following load reading its value.
        let access = unsafe { &*next };
        let address = u64::from(pointer as u32) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(second, address) }) else {
            return exit(next);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(next);
        };
        produce!(cx, r, next, access.a, value, view)
    }
}

lane_handler! {
    /// A direct i32 load `OP`, with address field `ACC`, whose value the next record, a
    /// zero-test jump without a carried register, tests (`p = p->next; while (p)`): both
    /// records run here, the value carried as the load carries it.
    fn lane_load_jump_i32_zero<OP: u16, ACC: u8, WHEN_ZERO: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let load = const { direct_load(OP) };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(value)) =
            (unsafe { view.memory.read(load, lane_address::<ACC>(r, i, view)) })
        else {
            return exit(ip);
        };
        let encoded = view.integers.encode(value);
        r.set(i.a, encoded);
        let view = Carried { acc: Slot::of(encoded), ..view };
        let next = following(ip);
        if (value == 0) == WHEN_ZERO {
            return lane_jump(cx, r, next, BACKWARD, view, None);
        }
        next!(cx, r, following(next), view)
    }
}

lane_handler! {
    /// A move, reading its source through field `ACC`, whose moved value the next record,
    /// direct i32 load `OP`, takes as its address: both records run here, each value written
    /// as its own record writes it.
    fn lane_move_load<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let load = const { direct_load(OP) };
        let moved = read::<ACC, ACC_B>(r, i.b, view);
        r.set(i.a, moved);
        let next = following(ip);
        // SAFETY: the view fuses a move only with a following load reading its value.
        let access = unsafe { &*next };
        let address = u64::from(moved.wasm_bits32()) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, address) }) else {
            return exit(next);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(next);
        };
        produce!(cx, r, next, access.a, value, view)
    }
}

lane_handler! {
    /// A direct store `OP`, reading through field `ACC`, followed by a move: both records run
    /// here, the moved value carried as the move carries it.
    fn lane_store_move<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let Some(value) = stored_value(store, read::<ACC, ACC_A>(r, i.a, view)) else {
            return exit(ip);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC>(r, i, view), value) }
            .is_none()
        {
            return exit(ip);
        }
        let next = following(ip);
        // SAFETY: the view fuses a store only with a following move.
        let moved = unsafe { &*next };
        produce!(cx, r, next, moved.a, r.get(moved.b), view)
    }
}

lane_handler! {
    /// Two direct i32 loads `LOAD` and the i32 register operator `OP` combining their values
    /// (`a[i] * b[j]`), the first value as its left operand when `FIRST_LEFT`: three records
    /// run here, the loads issued together, each value written as its own record writes it.
    /// An access the general path must run exits at its record.
    fn lane_load_pair_binary<LOAD: u16, OP: u16, FIRST_LEFT: bool>(cx, r, ip, i, view) {
        let load = const { direct_load(LOAD) };
        let operator = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => operator,
                None => panic!("not an i32 register operator"),
            }
        };
        let second_ip = following(ip);
        // SAFETY: the view fuses a load only with a following load and operator.
        let second_record = unsafe { &*second_ip };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(first)) =
            (unsafe { view.memory.read(load, lane_address::<ACC_NONE>(r, i, view)) })
        else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(first));
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(second)) =
            (unsafe { view.memory.read(load, lane_address::<ACC_NONE>(r, second_record, view)) })
        else {
            return exit(second_ip);
        };
        r.set(second_record.a, view.integers.encode(second));
        let combine = following(second_ip);
        // SAFETY: as above.
        let o = unsafe { &*combine };
        let (left, right) = if FIRST_LEFT { (first, second) } else { (second, first) };
        let Ok(result) = operator.evaluate(left, right) else {
            return exit(combine);
        };
        produce!(cx, r, combine, o.a, view.integers.encode(result), view)
    }
}

/// The load-pair handler for i32 load `load` and register operator `op`.
fn load_pair_binary_handler<H: Host>(load: Op, op: Op, first_left: bool) -> Option<Handler> {
    macro_rules! pairs {
        ($(($load:ident, $op:ident)),+) => {
            match (load, op, first_left) {
                $(
                    (Op::$load, Op::$op, false) => Some(lane_load_pair_binary::<H, { Op::$load as u16 }, { Op::$op as u16 }, false> as Handler),
                    (Op::$load, Op::$op, true) => Some(lane_load_pair_binary::<H, { Op::$load as u16 }, { Op::$op as u16 }, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    pairs!(
        (WasmI32Load, WasmI32Add),
        (WasmI32Load, WasmI32Multiply),
        (WasmI32Load16S, WasmI32Multiply),
        (WasmI32Load16U, WasmI32Multiply),
        (WasmI32Load8U, WasmI32Add)
    )
}

lane_handler! {
    /// An i32 add or subtract of a constant (`FIRST`), reading through field `ACC`, the and of
    /// a constant over its result, and the immediate comparison jump `JUMP` testing that
    /// (`(c - '0') & 0xff <= 9`): three records run here, each result written as its own
    /// record writes it.
    fn lane_offset_mask_jump<FIRST: u16, JUMP: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let offset = const {
            match I32BinaryOperator::from_immediate_op(opcode(FIRST)) {
                Some(operator) => operator,
                None => panic!("not an i32 immediate operator"),
            }
        };
        let comparison = const {
            match I32BinaryOperator::from_immediate_jump_op(opcode(JUMP)) {
                Some(comparison) => comparison,
                None => panic!("not an i32 immediate comparison jump"),
            }
        };
        let Ok(shifted) = offset.evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), i.imm() as i32) else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(shifted));
        let mask_ip = following(ip);
        // SAFETY: the view fuses an offset only with a following mask and comparison jump.
        let mask = unsafe { &*mask_ip };
        let masked = shifted & (mask.imm() as i32);
        let encoded = view.integers.encode(masked);
        r.set(mask.a, encoded);
        let view = Carried { acc: Slot::of(encoded), ..view };
        let jump_ip = following(mask_ip);
        // SAFETY: as above.
        let jump = unsafe { &*jump_ip };
        let Ok(taken) = comparison.evaluate(masked, i32::from(jump.b as i16)) else {
            return exit(jump_ip);
        };
        if taken != 0 {
            return lane_jump(cx, r, jump_ip, BACKWARD, view, None);
        }
        next!(cx, r, following(jump_ip), view)
    }
}

/// The offset-mask-test handler for offset operator `first`, immediate comparison jump `jump`.
fn offset_mask_jump_handler<H: Host>(first: Op, jump: Op, backward: bool, acc: u8) -> Option<Handler> {
    macro_rules! jumps {
        ($first:ident; $($jump:ident),+) => {
            match (jump, backward, acc) {
                $(
                    (Op::$jump, false, ACC_NONE) => Some(lane_offset_mask_jump::<H, { Op::$first as u16 }, { Op::$jump as u16 }, false, ACC_NONE> as Handler),
                    (Op::$jump, true, ACC_NONE) => Some(lane_offset_mask_jump::<H, { Op::$first as u16 }, { Op::$jump as u16 }, true, ACC_NONE> as Handler),
                    (Op::$jump, false, ACC_B) => Some(lane_offset_mask_jump::<H, { Op::$first as u16 }, { Op::$jump as u16 }, false, ACC_B> as Handler),
                    (Op::$jump, true, ACC_B) => Some(lane_offset_mask_jump::<H, { Op::$first as u16 }, { Op::$jump as u16 }, true, ACC_B> as Handler),
                )+
                _ => None,
            }
        };
    }
    macro_rules! tested {
        ($first:ident) => {
            jumps!(
                $first;
                WasmJumpI32EqualImmediate,
                WasmJumpI32NotEqualImmediate,
                WasmJumpI32LessUnsignedImmediate,
                WasmJumpI32LessEqualUnsignedImmediate,
                WasmJumpI32GreaterUnsignedImmediate,
                WasmJumpI32GreaterEqualUnsignedImmediate
            )
        };
    }
    match first {
        Op::WasmI32AddImmediate => tested!(WasmI32AddImmediate),
        Op::WasmI32SubtractImmediate => tested!(WasmI32SubtractImmediate),
        _ => None,
    }
}

lane_handler! {
    /// A bit-field extraction, reading its operand through field `ACC`, and a following i32
    /// operator `SECOND` (another extraction, or an operator of registers or a constant): both
    /// records run here, the extraction written before the operator reads its operands.
    fn lane_bit_field_then<SECOND: u16, ACC: u8>(cx, r, ip, i, view) {
        let (operator, immediate) = const {
            match opcode(SECOND) {
                Op::WasmI32ShiftRightUnsignedAndImmediate => (None, true),
                op => match I32BinaryOperator::from_register_op(op) {
                    Some(operator) => (Some(operator), false),
                    None => match I32BinaryOperator::from_immediate_op(op) {
                        Some(operator) => (Some(operator), true),
                        None => panic!("not an i32 operator"),
                    },
                },
            }
        };
        let field = crate::wasm::integer::shift_right_unsigned_and(
            read_i32::<ACC, ACC_B>(r, i.b, view),
            i32::from(i.c as i16),
            i.imm() as i32,
        );
        r.set(i.a, view.integers.encode(field));
        let second_ip = following(ip);
        // SAFETY: the view fuses an extraction only with a following i32 operator.
        let second = unsafe { &*second_ip };
        let value = match operator {
            None => crate::wasm::integer::shift_right_unsigned_and(
                r.i32(second.b),
                i32::from(second.c as i16),
                second.imm() as i32,
            ),
            Some(operator) => {
                let right = if immediate { second.imm() as i32 } else { r.i32(second.c) };
                let Ok(value) = operator.evaluate(r.i32(second.b), right) else {
                    return exit(second_ip);
                };
                value
            }
        };
        produce!(cx, r, second_ip, second.a, view.integers.encode(value), view)
    }
}

/// The extraction-then-operator handler for i32 operator `second`, the extraction read
/// through field `acc`.
fn bit_field_then_handler<H: Host>(second: Op, acc: u8) -> Option<Handler> {
    macro_rules! seconds {
        ($($second:ident),+) => {
            match (second, acc) {
                $(
                    (Op::$second, ACC_NONE) => Some(lane_bit_field_then::<H, { Op::$second as u16 }, ACC_NONE> as Handler),
                    (Op::$second, ACC_B) => Some(lane_bit_field_then::<H, { Op::$second as u16 }, ACC_B> as Handler),
                )+
                _ => None,
            }
        };
    }
    seconds!(WasmI32ShiftRightUnsignedAndImmediate, WasmI32Multiply, WasmI32XorImmediate)
}

lane_handler! {
    /// An immediate comparison jump `JUMP`, reading its operand through field `ACC`, that
    /// falls through to direct i32 load `LOAD` (`if c == k goto L; v = *p`): both records run
    /// here, the load only when the jump falls through.
    fn lane_compare_jump_load<JUMP: u16, LOAD: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let comparison = const {
            match I32BinaryOperator::from_immediate_jump_op(opcode(JUMP)) {
                Some(comparison) => comparison,
                None => panic!("not an i32 immediate comparison jump"),
            }
        };
        let load = const { direct_load(LOAD) };
        let Ok(taken) = comparison.evaluate(read_i32::<ACC, ACC_A>(r, i.a, view), i32::from(i.b as i16)) else {
            return exit(ip);
        };
        if taken != 0 {
            return lane_jump(cx, r, ip, BACKWARD, view, None);
        }
        let load_ip = following(ip);
        // SAFETY: the view fuses a comparison jump only with a following plain load.
        let access = unsafe { &*load_ip };
        let address = u64::from(r.get(access.c).wasm_bits32()) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, address) }) else {
            return exit(load_ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(load_ip);
        };
        produce!(cx, r, load_ip, access.a, value, view)
    }
}

/// The compare-jump-load handler for immediate comparison jump `jump` and i32 load `load`.
fn compare_jump_load_handler<H: Host>(jump: Op, load: Op, backward: bool, acc: u8) -> Option<Handler> {
    macro_rules! accs {
        ($jump:ident, $load:ident) => {
            match (backward, acc) {
                (false, ACC_NONE) => Some(lane_compare_jump_load::<H, { Op::$jump as u16 }, { Op::$load as u16 }, false, ACC_NONE> as Handler),
                (true, ACC_NONE) => Some(lane_compare_jump_load::<H, { Op::$jump as u16 }, { Op::$load as u16 }, true, ACC_NONE> as Handler),
                (false, ACC_A) => Some(lane_compare_jump_load::<H, { Op::$jump as u16 }, { Op::$load as u16 }, false, ACC_A> as Handler),
                (true, ACC_A) => Some(lane_compare_jump_load::<H, { Op::$jump as u16 }, { Op::$load as u16 }, true, ACC_A> as Handler),
                _ => None,
            }
        };
    }
    macro_rules! loads {
        ($jump:ident) => {
            match load {
                Op::WasmI32Load => accs!($jump, WasmI32Load),
                Op::WasmI32Load8U => accs!($jump, WasmI32Load8U),
                _ => None,
            }
        };
    }
    match jump {
        Op::WasmJumpI32EqualImmediate => loads!(WasmJumpI32EqualImmediate),
        Op::WasmJumpI32NotEqualImmediate => loads!(WasmJumpI32NotEqualImmediate),
        _ => None,
    }
}

lane_handler! {
    /// Two i32 adds (`FIRST`, reading through field `ACC`, then `SECOND`), each of a register
    /// or a constant, or an add and a counted loop's step and test (`SECOND` is
    /// `WasmI32AddImmediateJumpNonZero`): a strided loop's pointer and counter updates. The
    /// first result is written before the second reads its operands, as two records do.
    fn lane_add_pair<FIRST: u16, SECOND: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let first_immediate = const {
            match opcode(FIRST) {
                Op::WasmI32Add => false,
                Op::WasmI32AddImmediate => true,
                _ => panic!("not an i32 add"),
            }
        };
        let right = if first_immediate { i.imm() as i32 } else { read_i32::<ACC, ACC_C>(r, i.c, view) };
        let first = view.integers.encode(read_i32::<ACC, ACC_B>(r, i.b, view).wrapping_add(right));
        r.set(i.a, first);
        let second_ip = following(ip);
        // SAFETY: the view fuses an add only with a following add or loop step.
        let second = unsafe { &*second_ip };
        let view = Carried { acc: Slot::of(first), ..view };
        match const { opcode(SECOND) } {
            Op::WasmI32AddImmediateJumpNonZero => {
                let sum = r.i32(second.b).wrapping_add(i32::from(second.c as i16));
                let encoded = view.integers.encode(sum);
                if sum != 0 {
                    return lane_jump(cx, r, second_ip, BACKWARD, view, Some((second.a, encoded)));
                }
                produce!(cx, r, second_ip, second.a, encoded, view)
            }
            Op::WasmI32AddImmediate => {
                let sum = r.i32(second.b).wrapping_add(second.imm() as i32);
                produce!(cx, r, second_ip, second.a, view.integers.encode(sum), view)
            }
            _ => {
                let sum = r.i32(second.b).wrapping_add(r.i32(second.c));
                produce!(cx, r, second_ip, second.a, view.integers.encode(sum), view)
            }
        }
    }
}

/// The add-pair handler for add `first` read through field `acc`, then `second`.
fn add_pair_handler<H: Host>(first: Op, second: Op, backward: bool, acc: u8) -> Option<Handler> {
    macro_rules! accs {
        ($first:ident, $second:ident, $backward:literal) => {
            match acc {
                ACC_NONE => Some(lane_add_pair::<H, { Op::$first as u16 }, { Op::$second as u16 }, $backward, ACC_NONE> as Handler),
                ACC_B => Some(lane_add_pair::<H, { Op::$first as u16 }, { Op::$second as u16 }, $backward, ACC_B> as Handler),
                ACC_C => Some(lane_add_pair::<H, { Op::$first as u16 }, { Op::$second as u16 }, $backward, ACC_C> as Handler),
                _ => None,
            }
        };
    }
    macro_rules! seconds {
        ($first:ident) => {
            match (second, backward) {
                (Op::WasmI32Add, _) => accs!($first, WasmI32Add, false),
                (Op::WasmI32AddImmediate, _) => accs!($first, WasmI32AddImmediate, false),
                (Op::WasmI32AddImmediateJumpNonZero, false) => accs!($first, WasmI32AddImmediateJumpNonZero, false),
                (Op::WasmI32AddImmediateJumpNonZero, true) => accs!($first, WasmI32AddImmediateJumpNonZero, true),
                _ => None,
            }
        };
    }
    match first {
        Op::WasmI32Add => seconds!(WasmI32Add),
        Op::WasmI32AddImmediate => seconds!(WasmI32AddImmediate),
        _ => None,
    }
}

lane_handler! {
    /// An i32 operator of a constant (`FIRST`) and the register comparison jump `JUMP` reading
    /// its result as either operand (`(x & mask) == y`): two records run here, the result
    /// written as its own record writes it.
    fn lane_immediate_compare_jump<FIRST: u16, JUMP: u16, BACKWARD: bool>(cx, r, ip, i, view) {
        let operator = const {
            match I32BinaryOperator::from_immediate_op(opcode(FIRST)) {
                Some(operator) => operator,
                None => panic!("not an i32 immediate operator"),
            }
        };
        let comparison = const {
            match I32BinaryOperator::from_jump_op(opcode(JUMP)) {
                Some(comparison) => comparison,
                None => panic!("not an i32 register comparison jump"),
            }
        };
        let Ok(result) = operator.evaluate(r.i32(i.b), i.imm() as i32) else {
            return exit(ip);
        };
        let encoded = view.integers.encode(result);
        r.set(i.a, encoded);
        let view = Carried { acc: Slot::of(encoded), ..view };
        let jump_ip = following(ip);
        // SAFETY: the view fuses an immediate operator only with a following comparison jump.
        let jump = unsafe { &*jump_ip };
        let operand = |register: u16| if register == i.a { result } else { r.i32(register) };
        let Ok(taken) = comparison.evaluate(operand(jump.a), operand(jump.b)) else {
            return exit(jump_ip);
        };
        if taken != 0 {
            return lane_jump(cx, r, jump_ip, BACKWARD, view, None);
        }
        next!(cx, r, following(jump_ip), view)
    }
}

/// The immediate-operator comparison-jump handler for operator `first`, register comparison
/// jump `jump`.
fn immediate_compare_jump_handler<H: Host>(first: Op, jump: Op, backward: bool) -> Option<Handler> {
    macro_rules! jumps {
        ($first:ident; $($jump:ident),+) => {
            match (jump, backward) {
                $(
                    (Op::$jump, false) => Some(lane_immediate_compare_jump::<H, { Op::$first as u16 }, { Op::$jump as u16 }, false> as Handler),
                    (Op::$jump, true) => Some(lane_immediate_compare_jump::<H, { Op::$first as u16 }, { Op::$jump as u16 }, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    macro_rules! tested {
        ($first:ident) => {
            jumps!($first; WasmJumpI32Equal, WasmJumpI32NotEqual)
        };
    }
    // Masked tag tests; each further pairing is another family of handler copies.
    match first {
        Op::WasmI32AndImmediate => tested!(WasmI32AndImmediate),
        _ => None,
    }
}

lane_handler! {
    /// An i32 operator `OP` (reading through field `ACC`; a comparison, typically), a constant
    /// load and a select choosing among their results and other registers (`cur = cur > clip
    /// ? 0 : cur`): three records run here, each result written as its own record writes it,
    /// and the select reads the two results as computed rather than back from registers.
    fn lane_operator_constant_select<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let (operator, immediate) = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => (operator, false),
                None => match I32BinaryOperator::from_immediate_op(opcode(OP)) {
                    Some(operator) => (operator, true),
                    None => panic!("not an i32 binary operator"),
                },
            }
        };
        let right = if immediate { i.imm() as i32 } else { read_i32::<ACC, ACC_C>(r, i.c, view) };
        let Ok(result) = operator.evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), right) else {
            return exit(ip);
        };
        let result = view.integers.encode(result);
        r.set(i.a, result);
        let constant_ip = following(ip);
        // SAFETY: the view fuses an operator only with a following constant and select.
        let load = unsafe { &*constant_ip };
        let constant = load.constant();
        r.set(load.a, constant);
        let select_ip = following(constant_ip);
        // SAFETY: as above.
        let select = unsafe { &*select_ip };
        // The constant's record runs second, so it wins a register both write.
        let value_of = |register: u16| {
            if register == load.a {
                constant
            } else if register == i.a {
                result
            } else {
                r.get(register)
            }
        };
        let (condition, _) = ImmediateLayout::register_pair(select.imm());
        let value = select_loaded(
            value_of(condition).wasm_bits32() != 0,
            value_of(select.b),
            value_of(select.c),
        );
        produce!(cx, r, select_ip, select.a, value, view)
    }
}

/// The operator-constant-select handler for i32 operator `op` reading through field `acc`.
fn operator_constant_select_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! operators {
        ($($op:ident),+) => {
            match (op, acc) {
                $(
                    (Op::$op, ACC_NONE) => Some(lane_operator_constant_select::<H, { Op::$op as u16 }, ACC_NONE> as Handler),
                    (Op::$op, ACC_B) => Some(lane_operator_constant_select::<H, { Op::$op as u16 }, ACC_B> as Handler),
                    (Op::$op, ACC_C) => Some(lane_operator_constant_select::<H, { Op::$op as u16 }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    operators!(
        WasmI32GreaterSigned,
        WasmI32GreaterUnsigned,
        WasmI32LessSigned,
        WasmI32LessUnsigned,
        WasmI32Equal,
        WasmI32NotEqual,
        WasmI32GreaterEqualSigned,
        WasmI32LessEqualSigned
    )
}

/// The i32 loads the load fusions below serve.
macro_rules! i32_loads {
    ($m:ident) => {
        $m!(WasmI32Load, WasmI32Load8S, WasmI32Load8U, WasmI32Load16S, WasmI32Load16U)
    };
}


/// The pointer-chase handler whose second load is `op`.
fn load_load_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op, acc) {
                $(
                    (Op::$load, ACC_NONE) => Some(lane_load_load::<H, { Op::$load as u16 }, ACC_NONE> as Handler),
                    (Op::$load, ACC_C) => Some(lane_load_load::<H, { Op::$load as u16 }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    i32_loads!(loads)
}

/// The load-and-test handler for i32 load `op` with address field `acc`, jumping on zero
/// (`zero`) in direction `backward`.
fn load_jump_handler<H: Host>(op: Op, acc: u8, zero: bool, backward: bool) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op, acc, zero, backward) {
                $(
                    (Op::$load, ACC_NONE, false, false) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_NONE, false, false> as Handler),
                    (Op::$load, ACC_NONE, false, true) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_NONE, false, true> as Handler),
                    (Op::$load, ACC_NONE, true, false) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_NONE, true, false> as Handler),
                    (Op::$load, ACC_NONE, true, true) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_NONE, true, true> as Handler),
                    (Op::$load, ACC_C, false, false) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_C, false, false> as Handler),
                    (Op::$load, ACC_C, false, true) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_C, false, true> as Handler),
                    (Op::$load, ACC_C, true, false) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_C, true, false> as Handler),
                    (Op::$load, ACC_C, true, true) => Some(lane_load_jump_i32_zero::<H, { Op::$load as u16 }, ACC_C, true, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    i32_loads!(loads)
}

/// The move-then-load handler for i32 load `op`, the move reading through field `acc`.
fn move_load_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! loads {
        ($($load:ident),+) => {
            match (op, acc) {
                $(
                    (Op::$load, ACC_NONE) => Some(lane_move_load::<H, { Op::$load as u16 }, ACC_NONE> as Handler),
                    (Op::$load, ACC_B) => Some(lane_move_load::<H, { Op::$load as u16 }, ACC_B> as Handler),
                )+
                _ => None,
            }
        };
    }
    i32_loads!(loads)
}

lane_handler! {
    /// A plain direct store `OP`, a move and a zero-test jump (`WHEN_ZERO`, `BACKWARD`) testing
    /// the register the record before them carries as the accumulator, which the taken jump
    /// carries on to its target (a loop latch whose head reads the register there): three
    /// records run here, the moved value carried as the move carries it when falling through.
    fn lane_store_move_jump_i32_zero<OP: u16, WHEN_ZERO: bool, BACKWARD: bool>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        // SAFETY: the view selects this record only after the producer that set it.
        let tested = unsafe { view.acc.get() };
        let Some(value) = stored_value(store, r.get(i.a)) else {
            return exit(ip);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, i, view), value) }
            .is_none()
        {
            return exit(ip);
        }
        let move_ip = following(ip);
        // SAFETY: the view fuses a store only with a following move and zero-test jump.
        let moved = unsafe { &*move_ip };
        let value = r.get(moved.b);
        r.set(moved.a, value);
        let jump_ip = following(move_ip);
        if (tested.wasm_bits32() == 0) == WHEN_ZERO {
            return lane_jump(cx, r, jump_ip, BACKWARD, Carried { acc: Slot::of(tested), ..view }, None);
        }
        next!(cx, r, following(jump_ip), Carried { acc: Slot::of(value), ..view })
    }
}

/// The store-move-latch handler for direct store `op`.
fn store_move_jump_handler<H: Host>(op: Op, zero: bool, backward: bool) -> Option<Handler> {
    macro_rules! stores {
        ($($store:ident),+) => {
            match (op, zero, backward) {
                $(
                    (Op::$store, false, false) => Some(lane_store_move_jump_i32_zero::<H, { Op::$store as u16 }, false, false> as Handler),
                    (Op::$store, false, true) => Some(lane_store_move_jump_i32_zero::<H, { Op::$store as u16 }, false, true> as Handler),
                    (Op::$store, true, false) => Some(lane_store_move_jump_i32_zero::<H, { Op::$store as u16 }, true, false> as Handler),
                    (Op::$store, true, true) => Some(lane_store_move_jump_i32_zero::<H, { Op::$store as u16 }, true, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    stores!(WasmI32Store, WasmI32Store8, WasmI32Store16)
}

/// The store-then-move handler for direct store `op` reading through field `acc`.
fn store_move_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! stores {
        ($($store:ident),+) => {
            match (op, acc) {
                $(
                    (Op::$store, ACC_NONE) => Some(lane_store_move::<H, { Op::$store as u16 }, ACC_NONE> as Handler),
                    (Op::$store, ACC_A) => Some(lane_store_move::<H, { Op::$store as u16 }, ACC_A> as Handler),
                    (Op::$store, ACC_C) => Some(lane_store_move::<H, { Op::$store as u16 }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    stores!(WasmI32Store, WasmI32Store8, WasmI32Store16)
}

/// The scalar direct store `store` writes for slot value `value`: registers
/// hold their static Wasm types, and a value whose bits need a box has none.
#[inline(always)]
fn stored_value(store: MemoryStore, value: Value) -> Option<crate::WasmValue> {
    Some(match store.value_type() {
        crate::WasmType::I32 => crate::WasmValue::I32(value.wasm_bits32() as i32),
        crate::WasmType::F32 => crate::WasmValue::F32(value.as_int()? as u32),
        crate::WasmType::I64 => crate::WasmValue::I64(value.bits64()? as i64),
        crate::WasmType::F64 => crate::WasmValue::F64(value.bits64()?),
        _ => return None,
    })
}

lane_handler! {
    fn lane_store<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let Some(value) = stored_value(store, read::<ACC, ACC_A>(r, i.a, view)) else {
            return exit(ip);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC>(r, i, view), value) }
            .is_none()
        {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// An i32 load, with address field `ACC`, the i32 operator `OP` taking
    /// the loaded value as its left (`LEFT`) or right operand, and the i32
    /// store writing the operator's result (`mem[x] op= v`): three records
    /// run here, each result written as its own record writes it. An access
    /// the general path must run, or a trapping operator, exits at its
    /// record.
    fn lane_i32_update<OP: u16, LEFT: bool, ACC: u8>(cx, r, ip, i, view) {
        let operator = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => operator,
                None => panic!("not an i32 register operator"),
            }
        };
        // SAFETY: as for `lane_load`.
        let Some(crate::WasmValue::I32(loaded)) = (unsafe {
            view.memory.read(MemoryLoad::I32Load, lane_address::<ACC>(r, i, view))
        }) else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(loaded));
        let combine = following(ip);
        // SAFETY: the view fuses a load only with a following operator and
        // store.
        let o = unsafe { &*combine };
        let (left, right) = if LEFT { (loaded, r.i32(o.c)) } else { (r.i32(o.b), loaded) };
        let Ok(result) = operator.evaluate(left, right) else {
            return exit(combine);
        };
        r.set(o.a, view.integers.encode(result));
        let store = following(combine);
        // SAFETY: as above.
        let access = unsafe { &*store };
        // SAFETY: as for `lane_store`.
        if unsafe {
            view.memory.write(
                MemoryStore::I32Store,
                lane_address::<ACC_NONE>(r, access, view),
                crate::WasmValue::I32(result),
            )
        }
        .is_none()
        {
            return exit(store);
        }
        next!(cx, r, following(store), view)
    }
}

/// The i32 update handler for register operator `op`, loaded value side
/// `left` and load address field `acc`.
fn i32_update_handler<H: Host>(op: Op, left: bool, acc: u8) -> Option<Handler> {
    macro_rules! updates {
        ($($op:ident),+) => {
            match (op, left, acc) {
                $(
                    (Op::$op, false, ACC_NONE) => Some(lane_i32_update::<H, { Op::$op as u16 }, false, ACC_NONE> as Handler),
                    (Op::$op, true, ACC_NONE) => Some(lane_i32_update::<H, { Op::$op as u16 }, true, ACC_NONE> as Handler),
                    (Op::$op, false, ACC_C) => Some(lane_i32_update::<H, { Op::$op as u16 }, false, ACC_C> as Handler),
                    (Op::$op, true, ACC_C) => Some(lane_i32_update::<H, { Op::$op as u16 }, true, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    updates!(
        WasmI32Add,
        WasmI32Subtract,
        WasmI32Multiply,
        WasmI32And,
        WasmI32Or,
        WasmI32Xor,
        WasmI32ShiftLeft,
        WasmI32ShiftRightSigned,
        WasmI32ShiftRightUnsigned,
        WasmI32Equal,
        WasmI32NotEqual,
        WasmI32LessSigned,
        WasmI32LessUnsigned
    )
}

lane_handler! {
    /// A constant that the next record, direct store `OP`, writes to
    /// memory: both records run here, the constant written to its register
    /// as its own record writes it.
    fn lane_constant_store<OP: u16>(cx, r, ip, i, view) {
        let store = const {
            match MemoryStore::from_direct_op(opcode(OP)) {
                Some(store) => store,
                None => panic!("not a direct store"),
            }
        };
        let constant = i.constant();
        r.set(i.a, constant);
        let next = following(ip);
        // SAFETY: the view fuses a constant only with a following store.
        let access = unsafe { &*next };
        let Some(value) = stored_value(store, constant) else {
            return exit(next);
        };
        // SAFETY: as for `lane_load`.
        if unsafe { view.memory.write(store, lane_address::<ACC_NONE>(r, access, view), value) }
            .is_none()
        {
            return exit(next);
        }
        next!(cx, r, following(next), view)
    }
}

/// The constant-store handler for direct store `op`.
const fn constant_store_handler<H: Host>(op: Op) -> Option<Handler> {
    macro_rules! stores {
        ($($store:ident),+) => {
            match op {
                $(Op::$store => Some(lane_constant_store::<H, { Op::$store as u16 }> as Handler),)+
                _ => None,
            }
        };
    }
    stores!(
        WasmI32Store,
        WasmI64Store,
        WasmF32Store,
        WasmF64Store,
        WasmI32Store8,
        WasmI32Store16,
        WasmI64Store8,
        WasmI64Store16,
        WasmI64Store32
    )
}

/// Continue in `frame` at `pc`: the record the frame's function resumes at.
#[inline(always)]
fn lane_enter_frame<H: Host>(cx: *mut LaneContext<H>, frame: usize, pc: usize) -> Ip {
    let (r, ip, view) = lane_frame_state(cx, frame, pc);
    next!(cx, r, ip, view)
}

/// Make `frame` the lane's running function, at the record of `pc`: its registers, that
/// record and the state its records start from.
#[inline(always)]
fn lane_frame_state<H: Host>(
    cx: *mut LaneContext<H>,
    frame: usize,
    pc: usize,
) -> (Registers, Ip, Carried) {
    let vm = unsafe { vm(cx) };
    let function = vm.frames[frame].function;
    let program = unsafe { (*cx).program };
    let diagnostics = vm.programs.reports_on_drop();
    let Some(lane_view) = vm
        .programs
        .lane_view(program, function, |code, constants, kind, live, sites| {
            derive_lane_view::<H>(
                function,
                code,
                constants,
                kind,
                live,
                sites,
                vm.js_tier_runs(),
                vm.programs.js_jit_general_dispatch_enabled(),
                diagnostics,
            )
        })
    else {
        unreachable!("an inline Wasm frame runs a function of its program");
    };
    // SAFETY: views live as long as their program entry.
    let lane_view = unsafe { &*lane_view };
    let base = lane_view.records();
    let r = Registers(vm.frames[frame].registers.as_mut_ptr());
    let view = frame_memory_view(cx, lane_view, r);
    // SAFETY: the view has one record per instruction of the function, and
    // `pc` is the frame's validated resume point.
    let ip = unsafe { base.add(pc) };
    // The general path's register access names the active frame's register file.
    vm.active_registers = r.0;
    unsafe {
        (*cx).frame = frame;
        (*cx).registers = r;
        (*cx).base = base;
        (*cx).memory = view;
        (*cx).this = vm.frame_lane_this(frame);
        (*cx).env = Value::DELETED;
        (*cx).capture = std::ptr::null();
        (*cx).outer = NO_OUTER_WALK;
    }
    let view = Carried {
        memory: view,
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    (r, ip, view)
}

impl<H: Host> Vm<H> {
    /// Whether the running fast-lane activation stack is empty. OSR adapter reads that use
    /// frame-local storage must decline while locals live in frameless lane activations.
    pub(super) fn lane_has_activations(&self) -> bool {
        !self.lane_stack.activations.is_empty()
    }

    /// Make the lane's activations VM frames, innermost last, each with the
    /// pending call its push would have recorded; the lane continues in the
    /// innermost one. The bottom caller is the lane run's VM frame.
    /// Whether a lane may call `native` (the function `callee`) beside its activations
    /// (`Native::runs_beside_activations`, `Host::host_function_runs_beside_activations`).
    fn runs_beside_activations(&self, native: Native, callee: Value) -> bool {
        if native != Native::HostFunction {
            return native.runs_beside_activations();
        }
        let Some(index) = self.heap.function(callee).and_then(|function| self.host_operation_index(function.env)) else {
            return false;
        };
        self.host_beside_activations
            .get_or_init(|| (0..self.host.functions().len()).map(|index| self.host.host_function_runs_beside_activations(index)).collect())
            .get(index as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Whether `callee` is a native that runs beside a lane's activations
    /// (`runs_beside_activations`), which a call made while they stay frameless may make
    /// without making them frames.
    pub(super) fn callee_runs_beside_activations(&self, callee: Value) -> bool {
        match self.heap.function(callee) {
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) => {
                self.runs_beside_activations(*native, callee)
            }
            _ => false,
        }
    }

    /// Make a deferred lane native call's activations frames (`DeferredLaneCall`), as
    /// `lane_call_native` would have before calling: at every point that observes the frame
    /// stack while a native runs, which guest code, a stack capture and a collection do.
    #[inline(always)]
    pub(super) fn materialize_deferred_lane(&mut self) {
        if self.lane_deferred.is_some() {
            self.materialize_deferred_lane_now();
        }
    }

    #[cold]
    #[inline(never)]
    fn materialize_deferred_lane_now(&mut self) {
        let Some(DeferredLaneCall { cx, call_pc }) = self.lane_deferred.take() else {
            return;
        };
        // SAFETY: the deferred call's lane runs, so its context lives.
        let cx = cx.cast::<LaneContext<H>>();
        let before = self.frames.len();
        self.materialize_lane_activations(cx);
        // The natives running beside the activations recorded the frame count without them;
        // their caller is now the innermost new frame (`Vm::function_caller`).
        let added = self.frames.len() - before;
        for activation in self.realm.promise.active_native.iter_mut().rev() {
            if activation.frame_depth != before {
                break;
            }
            activation.frame_depth += added;
        }
        // The call runs from the innermost activation's frame, resuming after it.
        let frame = unsafe { (*cx).frame };
        self.frames[frame].pc = call_pc + 1;
        self.frames[frame].pc_is_resume = true;
    }

    fn materialize_lane_activations(&mut self, cx: *mut LaneContext<H>) {
        if self.lane_stack.activations.is_empty() {
            return;
        }
        #[cfg(feature = "profile-lane-exits")]
        super::lane_exits::record_detail(format!("materialize depth {}", self.lane_stack.activations.len()));
        let mut activations = std::mem::take(&mut self.lane_stack.activations);
        let lane_wasm_storage_bytes = std::mem::take(&mut self.lane_stack.wasm_storage_bytes);
        debug_assert_eq!(
            lane_wasm_storage_bytes,
            activations
                .iter()
                .map(|activation| activation.frame_storage_bytes)
                .sum::<usize>()
        );
        let pending = unsafe { &mut *(*cx).pending };
        let mut caller = unsafe { (*cx).frame };
        let mut registers = std::mem::take(&mut self.lane_stack.values);
        let mut forwarders = std::mem::take(&mut self.lane_stack.forwarders).into_iter();
        let mut settling = std::mem::take(&mut self.lane_stack.settling).into_iter();
        let iterators = std::mem::take(&mut self.lane_stack.iterators);
        let states = ActivationState::of(&activations, cx);
        for (depth, (activation, state)) in activations.drain(..).zip(states).enumerate() {
            let program = state.program;
            // Its frame belongs to its own program, which its push makes active; a call
            // into another program sets the caller's aside as the general loop's push does.
            self.set_active_program(program);
            // SAFETY: the program store keeps the residual for the lane run.
            let Some(code) = self.programs.residual(program).map(std::ptr::from_ref) else {
                unreachable!("an activation runs a function of a program the store holds");
            };
            let code = unsafe { &*code };
            let caller_program = (program != activation.caller_program).then(|| {
                // SAFETY: the general loop's executing program outlives the lane run.
                let executing = unsafe { &mut *(*cx).executing };
                debug_assert_eq!(executing.id, activation.caller_program);
                let retained = self.programs.get(program);
                let saved = super::dispatch_frame::SavedProgram {
                    id: executing.id,
                    retained: std::mem::replace(&mut executing.retained, retained),
                };
                executing.id = program;
                saved
            });
            // SAFETY: a call record lies in its caller's view.
            let mut call_pc = unsafe { activation.call.offset_from(activation.caller_base) } as usize;
            let mut result = unsafe { (*activation.call).a };
            self.frames[caller].pc = call_pc + 1;
            self.frames[caller].pc_is_resume = true;
            let count = usize::from(code.function(activation.function as usize).registers);
            let window = &registers[activation.window..activation.window + count];
            let construct_this = activation
                .script()
                .and_then(|script| script.completion.constructs().then_some(state.this));
            // A native's callback is the entry frame of the loop that finishes it.
            let to_native = activation.script().is_some_and(|script| script.completion == Completion::ReturnsToNative);
            // A settling activation's frame settles its promise as the general loop's
            // in-place push of the call would (`Vm::try_settling_call`).
            let settles = activation
                .script()
                .is_some_and(|script| script.completion == Completion::SettlesPromise)
                .then(|| settling.next().expect("each settling activation roots its promise"));
            let call_guard = match activation.kind {
                ActivationKind::Script(script) => {
                    let argc = usize::from(script.argc);
                    // A forwarding activation kept the values after its registers; any other
                    // passed them all to parameter registers.
                    let passed = if script.forwards {
                        &registers[activation.window + count..activation.window + count + argc]
                    } else if script.environment {
                        // After its environment and locals (`lane_activate_environment`).
                        let kept = activation.window
                            + count
                            + usize::from(WINDOW_LOCALS)
                            + usize::from(code.function(activation.function as usize).locals);
                        &registers[kept..kept + argc]
                    } else {
                        &window[..argc]
                    };
                    if script.completion == Completion::Forwarder {
                        // The construction's skipped forwarding constructor becomes the frame
                        // it would have been, pending at its forwarding call, whose dead
                        // registers hold `undefined`; the method's frame runs inside it.
                        let forwarder = forwarders
                            .next()
                            .expect("each forwarder activation has its skipped constructor");
                        let constructor_registers = vec![
                            Value::UNDEFINED;
                            usize::from(code.function(forwarder.function as usize).registers)
                        ];
                        self.push_script_frame_with_registers(
                            code,
                            forwarder.function,
                            forwarder.env,
                            state.this,
                            CallContext::user_function(forwarder.function, forwarder.callee).constructed(),
                            passed,
                            &constructor_registers,
                        );
                        pending.push(PendingGeneralCall {
                            caller,
                            caller_cursor: None,
                            call_pc: call_pc as u32,
                            destination: result & crate::bytecode::REGISTER_MASK,
                            construct_this: Some(state.this),
                            settles: None,
                            returns: result & crate::bytecode::RETURN_REGISTER != 0,
                            call_guard: crate::stack::GuestCallGuard::enter().ok(),
                            caller_program: None,
                        });
                        caller += 1;
                        call_pc = forwarder.method.call_pc as usize;
                        result = forwarder.method.call_result;
                        self.frames[caller].pc = call_pc + 1;
                        self.frames[caller].pc_is_resume = true;
                    }
                    self.push_script_frame_with_registers(
                        code,
                        activation.function,
                        state.env,
                        state.this,
                        if script.completion.constructs() {
                            CallContext::user_function(activation.function, script.callee).constructed()
                        } else {
                            CallContext::user_function(activation.function, script.callee)
                        },
                        passed,
                        window,
                    );
                    if script.environment {
                        // The frame its promotion would have made: the environment it holds,
                        // and its locals.
                        let held = activation.window + count;
                        // SAFETY: the activation's window, intact in the taken registers.
                        let environment = unsafe {
                            window_environment(
                                self,
                                code,
                                registers.as_mut_ptr().add(held),
                                activation.function,
                                state.env,
                                program,
                                state.this,
                            )
                        };
                        let frame = self.frames.last_mut().expect("the activation pushed a frame");
                        frame.env = environment;
                        frame.captured = true;
                        let locals = held + usize::from(WINDOW_LOCALS);
                        let count = frame.locals.len();
                        // The activation never binds `arguments`: its body only reads the
                        // values passed and its parameters (`Function::reads_passed_arguments_only`),
                        // so the object the push made from the values is the binding, with
                        // each element a parameter aliases holding that parameter now.
                        let body = code.function(activation.function as usize);
                        let arguments = body.arguments_slot.map(usize::from);
                        let object = arguments.map(|slot| frame.locals[slot]);
                        frame.locals.copy_from_slice(&registers[locals..locals + count]);
                        if let (Some(slot), Some(object)) = (arguments, object) {
                            frame.locals[slot] = object;
                            if body.arguments_alias_parameters() {
                                for parameter in 0..usize::from(body.params).min(argc) {
                                    let value = registers[locals + parameter];
                                    self.store_mapped_argument(object, parameter, value);
                                }
                            }
                        }
                    }
                    // Its frame closes the iterators its loops pushed (`LaneStack::iterators`).
                    let frame = self.frames.last_mut().expect("the activation pushed a frame");
                    frame.active_iterators.extend(
                        iterators.iter().filter(|(owner, _)| *owner == depth).map(|(_, iterator)| *iterator),
                    );
                    // The activation counted within `LaneStack::activation_limit`; its frame
                    // takes the guest-call guard its push would have.
                    crate::stack::GuestCallGuard::enter().ok()
                }
                ActivationKind::Wasm(_) => {
                    // The lane reserved this payload while it was frameless; its materialized
                    // frame holds the same weighted guard until the call returns.
                    Some(self.push_wasm_frame_with_registers(
                        code,
                        caller,
                        activation.function,
                        window,
                    ))
                }
            };
            if !to_native {
                pending.push(PendingGeneralCall {
                    caller,
                    caller_cursor: None,
                    call_pc: call_pc as u32,
                    destination: result & crate::bytecode::REGISTER_MASK,
                    construct_this,
                    settles,
                    returns: result & crate::bytecode::RETURN_REGISTER != 0,
                    call_guard,
                    caller_program,
                });
            }
            caller += 1;
        }
        debug_assert!(settling.next().is_none(), "every settling root belongs to an activation");
        self.lane_stack.values = registers;
        self.lane_stack.values.clear();
        self.lane_stack.activations = activations;
        // The frames now hold the guards the activations were counted against.
        self.lane_stack.refresh_activation_limit();
        // The lane continues in the innermost frame, whose register file the general path's
        // register access names from now on (`lane_enter_frame`).
        let registers = Registers(self.frames[caller].registers.as_mut_ptr());
        self.active_registers = registers.0;
        unsafe {
            (*cx).frame = caller;
            (*cx).registers = registers;
            (*cx).this = self.frame_lane_this(caller);
            (*cx).env = Value::DELETED;
            (*cx).capture = std::ptr::null();
            (*cx).outer = NO_OUTER_WALK;
        }
    }

    /// The lane's `this` for frame `frame` (`LaneContext::this`).
    fn frame_lane_this(&self, frame: usize) -> Value {
        let frame = &self.frames[frame];
        if frame.fixed_this { frame.this } else { Value::DELETED }
    }

    /// The cached frameless target of a lane call to `callee`, if resolved, in place: its
    /// cache entry, which a call reads field by field
    /// where it uses them rather than holding a copy in registers. The cache is boxed, so
    /// the entry stays put; it holds this target until the next resolution or collection
    /// replaces it, which no activation makes.
    #[inline(always)]
    fn script_call_entry(&self, callee: Value) -> Option<*const ScriptCallTarget> {
        self.script_call_index(callee).map(|index| &self.lane_stack.script_targets[index] as *const ScriptCallTarget)
    }

    /// The index of `script_call_entry`'s entry in `LaneStack::script_targets`.
    #[inline(always)]
    fn script_call_index(&self, callee: Value) -> Option<usize> {
        let set = self.lane_stack.script_targets.set(callee)?;
        // The set's two ways, tested in turn: an array iterator here compiled to an
        // out-of-line search on every call.
        let other = set | 1;
        if self.script_call_holds(set, callee) {
            Some(set)
        } else {
            self.script_call_holds(other, callee).then_some(other)
        }
    }

    /// Whether `LaneStack::script_targets` entry `index` holds `callee`'s target for this
    /// generation and the running program.
    #[inline(always)]
    fn script_call_holds(&self, index: usize, callee: Value) -> bool {
        self.lane_stack.script_targets.entries.get(index).is_some_and(|cached| {
            cached.callee == callee
                && cached.generation == self.lane_stack.generation
                && cached.program == self.active_program
        })
    }

    /// `script_call_entry` for the call record whose hint is `link` (`LaneInstruction::call_link`):
    /// the entry it names when that holds `callee`'s target, which skips the set's hash, else
    /// the set's, which the hint names from then on. A cache that grew or another callee only
    /// costs the hash again.
    #[inline(always)]
    fn linked_script_call_entry(&self, callee: Value, link: &std::cell::Cell<u16>) -> Option<*const ScriptCallTarget> {
        let linked = usize::from(link.get());
        let index = if self.script_call_holds(linked, callee) {
            linked
        } else {
            let index = self.script_call_index(callee)?;
            // The entry count stays below `NO_CALL_LINK` (`SCRIPT_CALL_TARGETS`).
            link.set(index as u16);
            index
        };
        // SAFETY: `script_call_holds` found the entry within the cache.
        Some(unsafe { self.lane_stack.script_targets.entries.get_unchecked(index) } as *const ScriptCallTarget)
    }

    /// `script_call_entry` of a callee of another program than the running one, whose
    /// targets `LaneStack::cross_targets` holds (`lane_script_call_cross`).
    #[inline(always)]
    fn cross_call_entry(&self, callee: Value) -> Option<*const ScriptCallTarget> {
        let set = self.lane_stack.cross_targets.set(callee)?;
        let (generation, program) = (self.lane_stack.generation, self.active_program);
        let holds = |cached: &ScriptCallTarget| {
            cached.callee == callee && cached.generation == generation && cached.program != program
        };
        let targets = &self.lane_stack.cross_targets;
        [set, set | 1]
            .into_iter()
            .map(|index| &targets[index])
            .find(|cached| holds(cached))
            .map(std::ptr::from_ref)
    }

    /// Resolve and cache `callee` as a lane call target: frameless when its activation is its
    /// registers (`CallPlan::registers_only`), else a `CallEntry::Frame` entry, so its calls
    /// take a frame without resolving it again.
    #[inline(never)]
    fn resolve_script_call_target(
        &mut self,
        code: &ResidualProgram,
        callee: Value,
    ) -> Option<ScriptCallTarget> {
        callee.heap_index()?;
        let (program, function, env, numeric) = self.same_realm_script_target(callee)?;
        #[cfg(feature = "profile-lane-exits")]
        if let Some(body) = self.programs.residual(program).and_then(|code| {
            let body = code.function_get(function as usize)?;
            Some(format!(
                "resolve {} span={:?} cross={} known={}",
                body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                body.source_span.map(|span| span.start),
                program != self.active_program,
                self.lane_stack.function_target(program, function, program != self.active_program).is_some(),
            ))
        }) {
            super::lane_exits::record_detail(body);
        }
        // A callee of another program runs its own residual, which the program store keeps
        // alive with the program for the lane run; only a plain closure of a program the
        // lane admits activates across programs.
        let cross = program != self.active_program;
        let code: &ResidualProgram = if cross {
            // SAFETY: as above; no program is installed or retired while a call resolves.
            let callee_code = unsafe { &*std::ptr::from_ref(self.programs.residual(program)?) };
            if numeric || !self.lane_admits(callee_code) {
                return None;
            }
            callee_code
        } else {
            code
        };
        // The JavaScript tier enters a frameless activation at its prologue as it enters a
        // frame (`lane_tier_prologue`), so a function it may claim is cached like any other.
        if function == super::ROOT_FUNCTION_ID {
            return None;
        }
        // A numeric body's target differs from its plain closures' (below), so only plain
        // closures share their function's.
        if !numeric && let Some(known) = self.lane_stack.function_target(program, function, cross) {
            let capture = if known.view.is_null() {
                std::ptr::null()
            } else {
                self.target_capture(code, program, env)
            };
            // An arrow's `this` is its own closure's environment's, like `capture`.
            let (this_binding, lexical_this) = match known.this_binding {
                ThisBinding::Lexical | ThisBinding::CallerReceiver => match self.captured_lexical_this(env) {
                    Some(this) if this.is_deleted() => return None,
                    Some(this) => (ThisBinding::Lexical, this),
                    None => (ThisBinding::CallerReceiver, Value::UNDEFINED),
                },
                binding => (binding, Value::UNDEFINED),
            };
            let target = ScriptCallTarget { callee, env, capture, this_binding, lexical_this, ..known };
            let policy = self.heap.memory_policy();
            if cross {
                self.lane_stack.remember_cross_target(target, policy);
            } else {
                self.lane_stack.remember_script_target(target, policy);
            }
            return Some(target);
        }
        if !self.programs.lane_call_may_derive(program, function) {
            return None;
        }
        let body = code.function_get(function as usize)?;
        let plan = body.call_plan(code);
        // An arrow binds the `this` of its environment, found once here: one still
        // uninitialized (before `super()`) leaves its calls to the general path, uncached.
        let lexical_this = if body.is_arrow {
            let this = self.captured_lexical_this(env);
            if this.is_some_and(Value::is_deleted) {
                return None;
            }
            Some(this)
        } else {
            None
        };
        // A numeric body's own frame constructs nothing in place
        // (`LaneContext::script_constructs`), which its activation would inherit from the
        // caller, so only one that constructs nothing runs frameless; its other records are
        // the lane's, with or without a frame.
        // An environment body runs in an environment activation where every local and
        // closure record of its view reads one (`LaneView::environment_window`).
        let environment = !plan.registers_only
            && plan.environment_activation
            && !numeric
            && self
                .lane_view_any(program, function)
                .is_some_and(|view| unsafe { &*view }.environment_window.is_some());
        // A non-suspending async body of the running program activates for its plain calls,
        // whose promise its return settles (`CallEntry::Settles`); across programs, or under a
        // host that observes promise lifecycles, its calls take a frame.
        let settles = plan.settles_in_loop && !cross && !numeric && !self.host.observes_promise_lifecycle();
        // A non-suspending async body runs in registers only as a settling activation.
        let registers_only = plan.registers_only && (settles || !plan.settles_in_loop);
        // Across programs a construction leaves its calls a frame.
        let view = if !(registers_only || environment)
            || numeric
                && body
                    .code
                    .iter()
                    .any(|packed| decoded_instruction(body, *packed).op() == Op::Construct)
        {
            None
        } else {
            self.lane_view_any(program, function)
        };
        #[cfg(feature = "profile-lane-exits")]
        if view.is_none() {
            let closures = body.code.iter().any(|packed| decoded_instruction(body, *packed).op() == Op::MakeClosure);
            super::lane_exits::record_detail(format!(
                "frame entry {} plain={} registers_only={} env_activation={} environment={} arrow={} rest={} closures={} arguments={} handlers={} class_ctor={} derived={} async={} generator={} locals_promoted={} simple_params={} selective={} cross={} numeric={}",
                body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                plan.plain, plan.registers_only, plan.environment_activation, environment, body.is_arrow, body.rest, closures,
                body.arguments_slot.is_some(), !body.handlers.is_empty(), body.is_class_constructor, body.derived_constructor,
                body.is_async, body.is_generator, body.local_registers.len(), body.simple_parameters,
                body.selective_capture_slots.is_some(), cross, numeric,
            ));
        }
        let Some(view) = view else {
            // The function needs a frame, which the target records so its calls take one
            // without resolving it again.
            let target = ScriptCallTarget {
                callee,
                generation: self.lane_stack.generation,
                program,
                function,
                env,
                entry: CallEntry::Frame,
                ..EMPTY_SCRIPT_CALL_TARGET
            };
            self.remember_target(target, numeric, cross);
            return Some(target);
        };
        // SAFETY: the program store keeps a derived view alive with its program.
        let shape = *unsafe { &*view }.target_body.get_or_init(|| {
            let constructs = Self::inline_constructible(body);
            TargetBody {
                // A class constructor's calls throw on the general path, so they take a
                // frame; only its constructions activate frameless (`lane_script_construct`).
                entry: if body.is_class_constructor {
                    CallEntry::Frame
                } else if body.code.first().is_some_and(|packed| {
                    returning_kind(decoded_instruction(body, *packed)) == Some(RETURNING_FIELD)
                }) {
                    CallEntry::Reads
                } else if element_read_body(body) {
                    CallEntry::ReadsElement
                } else if forwarding_read_body(body) {
                    CallEntry::ForwardsRead
                } else if capture_read_body(body) {
                    CallEntry::ReadsCapture
                } else if this_read_body(body) {
                    CallEntry::ReadsThis
                } else {
                    CallEntry::Activates
                },
                constructs,
                initializes: constructs && body.only_initializes_this_fields(&code.constants),
            }
        });
        if !environment
            && !settles
            && !cross
            && !body.rest
            && !plan.keeps_arguments
            && shape.entry == CallEntry::Activates
            && self.js_tier_may_claim(code, program, function)
        {
            // SAFETY: the program store keeps a derived view alive with its program.
            unsafe { &*view }.claim_for_tier(lane_tier_prologue::<H>, function, body.params, !body.is_arrow);
        }
        let capture = self.target_capture(code, program, env);
        let target = ScriptCallTarget {
            callee,
            generation: self.lane_stack.generation,
            program,
            function,
            env,
            view,
            registers: body.registers,
            // An environment activation's window also holds its environment, its locals and
            // the values passed, at most one per parameter.
            room: u16::try_from(fill_room::<SCRIPT_FILL_BLOCK>(
                usize::from(body.registers)
                    + if environment {
                        usize::from(WINDOW_LOCALS) + usize::from(body.locals) + usize::from(body.params)
                    } else {
                        0
                    },
            ))
            .ok()?,
            params: body.params,
            this_binding: match lexical_this {
                Some(Some(_)) => ThisBinding::Lexical,
                Some(None) => ThisBinding::CallerReceiver,
                None if body.strict => ThisBinding::Strict,
                None => ThisBinding::Sloppy,
            },
            lexical_this: lexical_this.flatten().unwrap_or(Value::UNDEFINED),
            // A construction activates in place (`lane_activate`), which gathers no rest, and
            // completes across programs as a call does (`Completion::CrossConstructs`); a
            // forwarding one runs its program's keeping construction
            // (`lane_construct_keeping_activation`), which another program's caller does not.
            constructs: shape.constructs && !environment && !settles && !body.rest && !(cross && plan.keeps_arguments),
            initializes: shape.initializes && !environment && !settles && !cross && !body.rest,
            // A read target answers its call through the callee's own field sites, which
            // only its program's caches index.
            entry: if settles {
                CallEntry::Settles
            } else if body.is_class_constructor {
                shape.entry
            } else if environment {
                CallEntry::Environment
            } else if body.rest {
                CallEntry::Gathers
            } else if plan.keeps_arguments {
                CallEntry::Keeps
            } else if cross {
                CallEntry::Activates
            } else {
                shape.entry
            },
            capture,
            forwarded_method: match (plan.forwards_arguments && shape.constructs)
                .then(|| body.forwarded_this_method(&code.constants))
                .flatten()
            {
                Some(method) => self.lane_stack.forwarded_method_index(method),
                None => NO_FORWARDED_METHOD,
            },
        };
        self.remember_target(target, numeric, cross);
        Some(target)
    }

    /// Record a resolved `target`, also as its function's unless its closure is numeric; a
    /// `cross` one among the other programs' targets.
    fn remember_target(&mut self, target: ScriptCallTarget, numeric: bool, cross: bool) {
        let policy = self.heap.memory_policy();
        if numeric {
            self.lane_stack.remember_script_target(target, policy);
        } else {
            self.lane_stack.remember_function_target(target, cross, policy);
        }
    }

    /// The capture slots a frameless target whose closure has environment `env` reads
    /// (`ScriptCallTarget::capture`), tagged `CAPTURE_STORES` when they admit every store,
    /// whose barrier this takes once.
    fn target_capture(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        env: Value,
    ) -> *const crate::heap::EnvironmentSlots {
        // A frameless activation's function has no environment of its own: its depth-0
        // captures are the closure environment's dense slots (`Vm::environment_capture_slots`),
        // read here with the facts the store tag needs from the same cell.
        let Some(Cell::Environment { function, slots, scope, .. }) = self.heap.get(env) else {
            return std::ptr::null();
        };
        if *function == u32::MAX || *function == super::ROOT_FUNCTION_ID {
            return std::ptr::null();
        }
        let Some(dense) = slots.dense() else {
            return std::ptr::null();
        };
        let capture: *const crate::heap::EnvironmentSlots = dense;
        if super::environment::direct_store_owner(code, program, *function, scope)
            .is_some_and(super::environment::owner_admits_every_direct_store)
        {
            self.heap.remember_cell(env);
            return capture.map_addr(|address| address | CAPTURE_STORES);
        }
        capture
    }

    /// Whether `program` runs in the lane: Wasm always, JavaScript only where both it and the
    /// VM are specialized. A lane run keeps its program and the VM's specialization, so the
    /// lane's field caches (`Vm::primary_own_field_hit` and its kin) need not test them.
    pub(super) fn lane_admits(&self, program: &ResidualProgram) -> bool {
        program.kind == crate::bytecode::ProgramKind::Wasm || (self.specialized && program.specialized)
    }

    /// The lane view a hot loop of `function` runs with (`Vm::note_back_edge`), or null.
    pub(super) fn hot_loop_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView {
        self.lane_view_any(program, function).unwrap_or(std::ptr::null())
    }

    /// The lane view of `function`, derived on first use.
    fn lane_view_any(&self, program: ProgramId, function: u32) -> Option<*const LaneView> {
        let diagnostics = self.programs.reports_on_drop();
        self.programs.lane_view(program, function, |code, constants, kind, live, sites| {
            derive_lane_view::<H>(
                function,
                code,
                constants,
                kind,
                live,
                sites,
                self.js_tier_runs(),
                self.programs.js_jit_general_dispatch_enabled(),
                diagnostics,
            )
        })
    }
}

script_handler! {
    /// `lane_script_call` of a linked target whose entry is not a plain activation: the site
    /// calls the form for that entry directly from now on, an environment or keeping
    /// activation its own handler and any other entry the out-of-line form, which sorts them
    /// out. Out of line, as a sibling call, so `lane_script_call` makes no call.
    #[inline(never)]
    fn lane_script_call_requicken<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let entry = vm.linked_script_call_entry(r.get(i.b), &i.call_link).map(|target| {
            // SAFETY: as `Vm::script_call_entry`; nothing before this read resolves a target.
            unsafe { (*target).entry }
        });
        let handler: Handler = match entry {
            Some(CallEntry::Environment) => lane_script_call_environment::<H>,
            Some(CallEntry::Settles) => lane_script_call_settles::<H>,
            Some(CallEntry::Keeps | CallEntry::Gathers) => lane_script_call_keeping::<H>,
            Some(_) => lane_script_call_full::<H>,
            None => sibling!(lane_script_call_resolve::<H>, r, ip, Carried::script(cx)),
        };
        requicken::<H>(ip, handler);
        sibling!(handler, r, ip, Carried::script(cx))
    }
}

script_handler! {
    /// A JavaScript `Call` of a frameless target (`Vm::resolve_script_call_target`): the
    /// callee runs in the lane on a window of the lane stack, with the frame facts its
    /// push would have set held by the activation until it returns or the lane exits.
    /// Any other call takes the general path. `ARGC` is the record's argument count where
    /// the view resolved it (`script_call_handler`), so the window's copy makes no test, else
    /// `ANY_ARGC`.
    fn lane_script_call<ARGC: u16>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if lane_constructing(vm, callee) {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        }
        let Some(target) = vm.linked_script_call_entry(callee, &i.call_link) else {
            sibling!(lane_script_call_resolve::<H>, r, ip, Carried::script(cx))
        };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let target = unsafe { &*target };
        // One test: the site calls the form for its target's entry directly from now on
        // (`lane_script_call_requicken`), out of line so this handler makes no call.
        if target.entry != CallEntry::Activates {
            sibling!(lane_script_call_requicken::<H>, r, ip, Carried::script(cx))
        }
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        // A receiver whose binding needs the heap takes the out-of-line form, so this
        // handler makes no call and keeps its registers.
        let Some(this) = lane_direct_call_this(vm, target, r.get(i.c)) else {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        // Values past the parameters and a full lane stack take the out-of-line form too,
        // whose activation keeps them or grows the stack (`lane_activate`), and so does another
        // program's target (`lane_push_activation`).
        // So does a window past `FIXED_ARGUMENT_COPIES`, whose copy is a call.
        let argc = call_argument_count::<ARGC>(i);
        if argc > target.params
            || argc > FIXED_ARGUMENT_COPIES
            || !lane_activation_room(vm, target)
            || lane_crosses::<H, false>(cx, target)
        {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        }
        let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
        if (returns && !lane_returning_call_admitted(cx)) || vm.with_scope_entered {
            return exit(ip);
        }
        // SAFETY: the record's window of `r` holds its values.
        let passed = unsafe { r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))).cast_const() };
        let (registers, base) =
            lane_push_passed::<H, false, false>(cx, r, ip, target, passed, call_argument_count::<ARGC>(i), this, false);
        next!(cx, registers, base, Carried::script(cx))
    }
}

/// The `lane_script_call` argument count of a record its view did not resolve.
const ANY_ARGC: u16 = u16::MAX;

/// The argument count of the call record `i`: `ARGC`, unless that is `ANY_ARGC`.
#[inline(always)]
fn call_argument_count<const ARGC: u16>(i: &LaneInstruction) -> u16 {
    if ARGC == ANY_ARGC { ImmediateLayout::argument_count(i.imm()) } else { ARGC }
}

/// The `lane_script_call` instantiation of a call record whose immediate is `imm`: that of
/// its argument count where the copy of its window is one fixed move
/// (`FIXED_ARGUMENT_COPIES`), else the general one.
fn script_call_handler<H: Host>(imm: u32) -> Handler {
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    match ImmediateLayout::argument_count(imm) {
        0 => lane_script_call::<H, 0>,
        1 => lane_script_call::<H, 1>,
        2 => lane_script_call::<H, 2>,
        3 => lane_script_call::<H, 3>,
        4 => lane_script_call::<H, 4>,
        _ => lane_script_call::<H, ANY_ARGC>,
    }
}

/// Gives each call record the `lane_script_call` instantiation of its argument count.
fn specialize_script_calls<H: Host>(records: &mut [LaneInstruction]) {
    let general = lane_script_call::<H, ANY_ARGC> as Handler as *const ();
    for record in records.iter_mut().filter(|record| record.handler == general) {
        record.handler = script_call_handler::<H>(record.imm()) as *const ();
    }
}

/// The `this` a plain call of `target` binds for `receiver`: an arrow's lexical one
/// (`ScriptCallTarget::lexical_this`), else `call_this_value`'s without its boxing of
/// primitives, which allocates, so `None` for a sloppy callee's primitive.
#[inline(always)]
fn lane_call_this<H: Host>(vm: &Vm<H>, target: &ScriptCallTarget, receiver: Value) -> Option<Value> {
    match lane_direct_call_this(vm, target, receiver) {
        Some(this) => Some(this),
        None => vm.is_object_like(receiver).then_some(receiver),
    }
}

/// `lane_call_this` where the receiver's own bits decide it: a strict callee, an object in
/// the direct record table, or `null`/`undefined`. `None` for any other receiver, whose
/// object test reads the heap.
#[inline(always)]
fn lane_direct_call_this<H: Host>(
    vm: &Vm<H>,
    target: &ScriptCallTarget,
    receiver: Value,
) -> Option<Value> {
    match target.this_binding {
        ThisBinding::Lexical => Some(target.lexical_this),
        binding => lane_receiver_this(vm, binding, receiver),
    }
}

/// `lane_direct_call_this` of a binding other than an arrow's own `this`, which the
/// receiver alone decides; `None` for `ThisBinding::Lexical`.
#[inline(always)]
fn lane_receiver_this<H: Host>(vm: &Vm<H>, binding: ThisBinding, receiver: Value) -> Option<Value> {
    // Compares, not a table: a sloppy method's object receiver, the common call, takes the
    // first two.
    if binding != ThisBinding::Sloppy {
        // An arrow whose environment binds no `this` takes its caller's receiver.
        return (binding != ThisBinding::Lexical).then_some(receiver);
    }
    // An object receiver binds itself; a direct record of any object family is one.
    if receiver.is_direct_object_family() {
        Some(receiver)
    } else if receiver.is_null() || receiver.is_undefined() {
        Some(vm.realm.globals)
    } else {
        None
    }
}

script_handler! {
    /// `lane_script_call` out of line, so the plain call keeps its registers: a receiver
    /// whose binding reads the heap (`lane_call_this`), a forwarding target, which takes
    /// `lane_script_call_keeping`, and a target whose body is a read (`CallEntry::Reads`,
    /// `CallEntry::ReadsElement`), whose hit (`lane_call_read`) completes the
    /// call with its value as the callee's return would; a call that returns its callee's
    /// completion completes its caller in turn (`lane_return_value!`). Anything else
    /// activates the callee. A site that called such a target calls this form directly, so it
    /// makes `lane_script_call`'s own entry tests.
    #[inline(never)]
    fn lane_script_call_full<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if lane_constructing(vm, callee) {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        }
        let Some(target) = vm.linked_script_call_entry(callee, &i.call_link) else {
            sibling!(lane_script_call_resolve::<H>, r, ip, Carried::script(cx))
        };
        // SAFETY: as `Vm::script_call_entry`; a read resolves no target and collects nothing.
        let target = unsafe { &*target };
        if target.entry == CallEntry::Frame {
            sibling!(lane_frame_call::<H>, r, ip, Carried::script(cx))
        }
        if target.entry == CallEntry::Environment {
            sibling!(lane_script_call_environment::<H>, r, ip, Carried::script(cx))
        }
        if target.settles() {
            sibling!(lane_script_call_settles::<H>, r, ip, Carried::script(cx))
        }
        if target.forwards() {
            sibling!(lane_script_call_keeping::<H>, r, ip, Carried::script(cx))
        }
        // A receiver whose binding reads the heap takes the general form.
        let Some(this) = lane_direct_call_this(vm, target, r.get(i.c)) else {
            sibling!(lane_script_call_general::<H>, r, ip, Carried::script(cx))
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        let arguments = ImmediateLayout::call_window_base(i.imm());
        let argc = ImmediateLayout::argument_count(i.imm());
        let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
        let value = if target.entry == CallEntry::Activates
            || returns && !lane_returning_call_admitted(cx)
        {
            None
        } else {
            let passed = ReadArguments { registers: r, base: arguments, passed: argc };
            // SAFETY: as `lane_call_read`.
            unsafe { lane_answered_read(cx, ip, target, passed, this) }
        };
        let Some(value) = value else {
            // An activation keeping values past the parameters, a full lane stack or another
            // program's target takes the general form (`lane_activate`).
            if argc > target.params
                || !lane_activation_room(vm, target)
                || (returns && !lane_returning_call_admitted(cx))
                || vm.with_scope_entered
                || lane_crosses::<H, false>(cx, target)
            {
                sibling!(lane_script_call_general::<H>, r, ip, Carried::script(cx))
            }
            let (registers, base) = lane_push_activation::<H, false, false>(cx, r, ip, target, arguments, this, false);
            next!(cx, registers, base, Carried::script(cx))
        };
        if !returns {
            r.set(i.a, value);
            next!(cx, r, following(ip), Carried::script(cx))
        }
        lane_return_value!(cx, ip, value)
    }
}

script_handler! {
    /// `lane_script_call_full`'s activation of a call its form declines: a receiver whose
    /// binding reads the heap (`lane_call_this`), values past the parameters, which the
    /// keeping activation holds, or a full lane stack (`lane_activate`). Out of line, so the
    /// full form makes no call; it reads no target, which the full form already answered.
    #[inline(never)]
    fn lane_script_call_general<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let target = vm
            .script_call_entry(callee)
            .filter(|_| unsafe { (*cx).script_calls } && !lane_constructing(vm, callee));
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let Some(target) = target.map(|target| unsafe { &*target }).filter(|target| {
            !matches!(target.entry, CallEntry::Frame | CallEntry::Environment | CallEntry::Settles) && !target.forwards()
        }) else {
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        let Some(this) = lane_call_this(vm, target, r.get(i.c)) else {
            return exit(ip);
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        let arguments = ImmediateLayout::call_window_base(i.imm());
        match lane_activate(cx, r, ip, target, arguments, this, false) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(next) => next,
        }
    }
}

script_handler! {
    /// `lane_script_call_full` of an environment target (`CallEntry::Environment`), out of
    /// line so the other calls keep their registers: its activation holds its environment
    /// and locals (`lane_activate_environment`). Any other callee takes the full form.
    #[inline(never)]
    fn lane_script_call_environment<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let target = vm
            .script_call_entry(callee)
            .filter(|_| unsafe { (*cx).script_calls } && !lane_constructing(vm, callee));
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let Some(target) = target.map(|target| unsafe { &*target }).filter(|target| target.entry == CallEntry::Environment)
        else {
            // A site this form serves since `lane_script_call` saw an environment target
            // calls the full form once its callee differs.
            requicken::<H>(ip, lane_script_call_full::<H>);
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        let Some(this) = lane_call_this(vm, target, r.get(i.c)) else {
            return exit(ip);
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        // SAFETY: the call's window is registers of the running function.
        let passed = unsafe { lane_call_window(r, i) };
        match lane_activate_environment(cx, r, ip, target, passed, this) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(next) => next,
        }
    }
}

script_handler! {
    /// `lane_script_call_full` of a non-suspending async target (`CallEntry::Settles`), out of
    /// line so the other calls keep their registers: the call makes its promise first, as
    /// `Vm::try_settling_call` does, then activates the body, whose return settles it
    /// (`Completion::SettlesPromise`). A call passing values past the parameters, a `with`
    /// scope or a host observing promise lifecycles takes the general path, which settles the
    /// promise in a frame, and so does a full lane stack, which grows first
    /// (`lane_script_call_frame`). Any other callee takes the full form.
    #[inline(never)]
    fn lane_script_call_settles<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let target = vm
            .script_call_entry(callee)
            .filter(|_| unsafe { (*cx).script_calls } && !lane_constructing(vm, callee));
        // SAFETY: as `Vm::script_call_entry`; the target is copied before anything allocates.
        let Some(target) = target.map(|target| unsafe { *target }).filter(ScriptCallTarget::settles) else {
            requicken::<H>(ip, lane_script_call_full::<H>);
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
        if ImmediateLayout::argument_count(i.imm()) > target.params
            || (returns && !lane_returning_call_admitted(cx))
            || vm.with_scope_entered
            || lane_crosses::<H, false>(cx, &target)
            || vm.host.observes_promise_lifecycle()
        {
            return exit(ip);
        }
        if !lane_activation_room(vm, &target) {
            return lane_script_call_frame::<H>(ip);
        }
        let Some(this) = lane_call_this(vm, &target, r.get(i.c)) else {
            return exit(ip);
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, &target);
        // Without lifecycle hooks the promise's allocation runs no guest code and collects
        // nothing (`note_allocation` makes a collection due at the next frame boundary).
        let Ok(promise) = vm.promise_object() else {
            return exit(ip);
        };
        note_allocation(vm);
        let root = vm.heap.root(promise);
        vm.lane_stack.settling.push(root);
        let arguments = ImmediateLayout::call_window_base(i.imm());
        let (registers, base) = lane_push_activation::<H, false, true>(cx, r, ip, &target, arguments, this, false);
        next!(cx, registers, base, Carried::script(cx))
    }
}

script_handler! {
    /// An iteration step (`lane_iteration_step`) of this realm's array iterator `next` over a
    /// plain array (`Vm::step_present_array_value`): the step in place, with a result object
    /// only the step's `done` and `value` reads observe (`Vm::reused_iterator_result`). Any
    /// other step is the call it is.
    fn lane_iterator_step<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (method, iterator) = (r.get(i.b), r.get(i.c));
        if !vm.steps_as_array_iterator(iterator, method) {
            sibling!(lane_script_call::<H, ANY_ARGC>, r, ip, Carried::script(cx))
        }
        let Some(result) = lane_array_iteration_result(vm, iterator) else {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, result);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// `lane_iterator_step`'s step and result, out of line: the first result allocates.
#[inline(never)]
fn lane_array_iteration_result<H: Host>(vm: &mut Vm<H>, iterator: Value) -> Option<Value> {
    let step = vm.step_present_array_value(iterator)?;
    let result = vm.reused_iterator_result(step.unwrap_or(Value::UNDEFINED), step.is_none());
    note_allocation(vm);
    Some(result)
}

script_handler! {
    /// A JavaScript `Call` of another program's frameless callee (`LaneStack::cross_targets`):
    /// its activation runs in the callee's program, which its return leaves
    /// (`Completion::Cross`). A site whose callee is no such target calls `lane_script_call`
    /// again.
    #[inline(never)]
    fn lane_script_call_cross<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if lane_constructing(vm, callee) {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        }
        let Some(target) = vm.cross_call_entry(callee) else {
            requicken::<H>(ip, script_call_handler::<H>(i.imm()));
            sibling!(lane_script_call::<H, ANY_ARGC>, r, ip, Carried::script(cx))
        };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let target = unsafe { &*target };
        // A callee that needs a frame takes it as `lane_frame_call` pushes one.
        if !target.entry.activates_across() && target.entry != CallEntry::Environment {
            sibling!(lane_frame_call::<H>, r, ip, Carried::script(cx))
        }
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        let Some(this) = lane_call_this(vm, target, r.get(i.c)) else {
            return exit(ip);
        };
        let arguments = ImmediateLayout::call_window_base(i.imm());
        let activated = if target.entry == CallEntry::Environment {
            // SAFETY: the call's window is registers of the running function.
            lane_activate_environment(cx, r, ip, target, unsafe { lane_call_window(r, i) }, this)
        } else if target.forwards() {
            lane_activate_keeping::<H, true>(cx, r, ip, target, ActivationArguments::Window(arguments), this, false, None)
        } else {
            lane_activate(cx, r, ip, target, arguments, this, false)
        };
        match activated {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(next) => next,
        }
    }
}

script_handler! {
    /// `lane_script_call` of a callee its target cache does not hold: resolving it
    /// (`Vm::resolve_script_call_target`) makes a call, so it runs out of line and the call
    /// continues at `lane_script_call`, which then finds it, or at `lane_native_call`.
    #[inline(never)]
    fn lane_script_call_resolve<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if vm.cross_call_entry(callee).is_some() {
            requicken::<H>(ip, lane_script_call_cross::<H>);
            sibling!(lane_script_call_cross::<H>, r, ip, Carried::script(cx))
        }
        if let Some(target) = vm.resolve_script_call_target(code, callee) {
            if target.program == vm.active_program {
                sibling!(lane_script_call::<H, ANY_ARGC>, r, ip, Carried::script(cx))
            }
            // Another program's callee that needs a frame takes it as `lane_frame_call`
            // pushes one.
            if target.entry != CallEntry::Activates {
                sibling!(lane_frame_call::<H>, r, ip, Carried::script(cx))
            }
            requicken::<H>(ip, lane_script_call_cross::<H>);
            sibling!(lane_script_call_cross::<H>, r, ip, Carried::script(cx))
        }
        // A site calling a native takes the native handler from now on, which hands any other
        // callee back.
        match vm.heap.function(callee) {
            Some(FunctionRecord { kind: FunctionKind::Native(Native::StringCharCodeAt), .. })
                if i.a & crate::bytecode::RETURN_REGISTER == 0 =>
            {
                requicken::<H>(ip, lane_call_char_code_at::<H>);
            }
            Some(FunctionRecord { kind: FunctionKind::Native(_), .. }) => {
                requicken::<H>(ip, lane_native_call::<H>);
            }
            _ => {}
        }
        sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
    }
}

/// The by-name load a `LoadName` handler runs for (`Op::LoadName`): a `LoadNameCall` also
/// writes its call's receiver, and a `LoadNameTypeof` reads an absent global as `undefined`.
const LOAD_NAME: u8 = 0;
/// `LOAD_NAME` for `Op::LoadNameCall`.
const LOAD_NAME_CALL: u8 = 1;
/// `LOAD_NAME` for `Op::LoadNameTypeof`.
const LOAD_NAME_TYPEOF: u8 = 2;

script_handler! {
    /// A `LoadName`'s first run: a name the global object answers whenever no global
    /// lexical binding shadows it (`Vm::names_resolve_to_global_object`: a function whose
    /// unresolved names are global (`ProgramStore::unresolved_names_are_global`) and resolve
    /// statically, and a name no internal binding owns) takes `lane_load_global_name`; any
    /// other always takes the general path. `KIND` is the load's (`LOAD_NAME`): a call's also
    /// writes its receiver, `undefined` for a binding of the global object's environment.
    #[inline(never)]
    fn lane_load_name_first<KIND: u8>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        if vm.atom_class(i.imm()).contains(AtomClass::INTERNAL)
            && let Some(native) =
                crate::bytecode::intrinsic_name_native(&code.atoms[i.imm() as usize])
        {
            r.set(i.a, vm.native_value(native));
            if KIND == LOAD_NAME_CALL {
                r.set(i.b, Value::UNDEFINED);
            }
            next!(cx, r, following(ip), Carried::script(cx))
        }
        let function = vm
            .lane_stack
            .activations
            .last()
            .map_or(vm.frames[unsafe { (*cx).frame }].function, |activation| activation.function);
        // An internal name is no binding source text can declare, except the internal globals
        // the realm installs on its global object, which a static lookup reads as any other.
        // Static resolution rules out a `with` object, which the general search consults first.
        let statically = code
            .function_get(function as usize)
            .is_some_and(|function| function.names_resolve_statically(code));
        let global = statically
            && vm.programs.unresolved_names_are_global(unsafe { (*cx).program }, function)
            && (!vm.atom_class(i.imm()).contains(AtomClass::INTERNAL)
                || crate::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&vm.atom_name(i.imm())));
        if global {
            requicken::<H>(ip, lane_load_global_name::<H, KIND>);
            sibling!(lane_load_global_name::<H, KIND>, r, ip, Carried::script(cx))
        }
        if crate::bytecode::is_intrinsic_name(vm.atom_name(i.imm())) {
            requicken::<H>(ip, lane_load_intrinsic_name::<H, KIND>);
            sibling!(lane_load_intrinsic_name::<H, KIND>, r, ip, Carried::script(cx))
        }
        requicken::<H>(ip, lane_exit::<H>);
        exit(ip)
    }
}

script_handler! {
    /// A `LoadName` of an intrinsic name (`bytecode::is_intrinsic_name`), as the compiler
    /// emits for `{ __proto__: v }` or `super`: the native it stands for, which no binding
    /// can shadow and no scope affects (`Vm::intrinsic_name_value`).
    #[inline(never)]
    fn lane_load_intrinsic_name<KIND: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(value) = vm.intrinsic_name_value(i.imm()) else {
            return exit(ip);
        };
        r.set(i.a, value);
        if KIND == LOAD_NAME_CALL {
            r.set(i.b, Value::UNDEFINED);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A `LoadName` where names resolve statically to global bindings (`lane_load_name_first`):
    /// an initialized global lexical binding of the name, which the general path's search
    /// finds first there, else the global object's cached field (`Vm::cached_field_hit`), or
    /// the getter of the accessor its site cached (`Vm::cached_field_getter`), as Node defines
    /// `Buffer`, activated with the global object as receiver as a field read's is
    /// (`lane_activate_getter`). Any other read, including an uninitialized binding or a cache
    /// miss, takes the general path, which raises or caches it. A `LoadNameCall`
    /// (`LOAD_NAME_CALL`) also writes the call's receiver, `undefined` for a global binding,
    /// and `typeof` (`LOAD_NAME_TYPEOF`) reads an absent name as `undefined`.
    fn lane_load_global_name<KIND: u8>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let atom = i.imm();
        if vm.realm.global_lexical_declarations.contains(&atom) {
            let Some(value) = vm.realm.global_lexical_bindings.get(&atom).copied().filter(|value| !value.is_deleted())
            else {
                return exit(ip);
            };
            r.set(i.a, value);
            if KIND == LOAD_NAME_CALL {
                r.set(i.b, Value::UNDEFINED);
            }
            next!(cx, r, following(ip), Carried::script(cx))
        }
        let globals = vm.realm.globals;
        let super::object::FieldCacheProbe::Hit(value) = vm.cached_field_hit(code, globals, i.c) else {
            sibling!(lane_load_global_name_miss::<H, KIND>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        if KIND == LOAD_NAME_CALL {
            r.set(i.b, Value::UNDEFINED);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_load_global_name` past its lexical binding and cached global property: a
    /// global accessor's getter, activated in the lane, or a plain global property, recorded
    /// in the site's entry; any other read takes the general path. Out of line: the getter's
    /// activation and the lookups take stack temporaries, which the common read would hold
    /// across its dispatch, which then could not be a sibling call.
    #[inline(never)]
    fn lane_load_global_name_miss<KIND: u8>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let atom = i.imm();
        let globals = vm.realm.globals;
        if let Some(getter) = vm.cached_field_getter(globals, atom, vm.field_cache_index(i.c)) {
            if !unsafe { (*cx).script_calls } {
                return exit(ip);
            }
            // A binding of the global object's environment calls with an `undefined`
            // receiver.
            if KIND == LOAD_NAME_CALL {
                r.set(i.b, Value::UNDEFINED);
            }
            return match lane_activate_getter(cx, r, ip, getter, globals) {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                Err(next) => next,
            };
        }
        // A present data property records the site's entry as the general path's read
        // does (`Vm::field_miss_data_value`): the global object changes shape as Node's
        // lazy globals materialize, and a site that only walked would miss from then
        // on. Only a present one: that read answers an absent key as `[[Get]]` does,
        // where the binding raises. A global object no cache records (a dictionary
        // shape) still answers a present data property through its shape walk. An
        // absent name is `undefined` to `typeof` and raises on the general path
        // otherwise.
        let value = match vm.plain_global_lookup(atom) {
            Some(Some(value)) => vm
                .field_miss_data_value(globals, atom, vm.field_cache_index(i.c))
                .unwrap_or(value),
            Some(None) if KIND == LOAD_NAME_TYPEOF => Value::UNDEFINED,
            _ => return exit(ip),
        };
        r.set(i.a, value);
        if KIND == LOAD_NAME_CALL {
            r.set(i.b, Value::UNDEFINED);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `new Array(...values)` of an intrinsic `Array`, which allocates without running guest
    /// code: the array of the values passed, or for one Number argument the array of that
    /// length (`Vm::array_of_length`), with the callee's own `prototype`. An invalid length,
    /// and any other construction, take the general path.
    #[inline(never)]
    fn lane_native_construct<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
        match vm.heap.function(callee) {
            Some(FunctionRecord { kind: FunctionKind::User(..), .. }) => {
                if let Some(parent) = lane_super_forwarded_parent(vm, callee) {
                    return lane_construct_forwarded_parent(cx, r, ip, i, callee, parent);
                }
                return lane_frame_construct(cx, r, ip, i);
            }
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. })
                if native.constructs_from_lane() && !returns =>
            {
                let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
                // SAFETY: the construction's window is registers of the running function.
                let args = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.c)), argc) };
                let Some(value) = lane_construct_native(cx, ip, callee, callee, args) else {
                    return exit(ip);
                };
                // The construction ran from the innermost frame, whose registers are the lane's.
                let r = unsafe { (*cx).registers };
                r.set(i.a, value);
                next!(cx, r, following(ip), Carried::script(cx))
            }
            _ => {}
        }
        if (returns && vm.lane_stack.activations.is_empty())
            || !matches!(
                vm.heap.function(callee),
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::Array),
                    ..
                })
            )
        {
            return exit(ip);
        }
        let prototype_atom = vm.prototype_atom();
        let Some(prototype) = vm
            .own_function_prototype_data(callee, prototype_atom)
            .filter(|prototype| vm.object_data(*prototype).is_some())
        else {
            return exit(ip);
        };
        let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
        // SAFETY: the construction's window is registers of the running function.
        let args = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.c)), argc) };
        let array = match args {
            [length] if let Some(length) = length.as_number() => {
                // An invalid length throws its RangeError on the general path.
                let Some(array) = vm.array_of_length(prototype, length) else {
                    return exit(ip);
                };
                array
            }
            _ => lane_new_array(vm, prototype, args),
        };
        note_allocation(vm);
        if returns {
            let Some(resumed) = lane_complete_activation(cx, ip, array) else {
                unreachable!("a returning lane construction has an activation caller");
            };
            resume!(cx, resumed)
        }
        r.set(i.a, array);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The array `new Array(...args)` allocates with `prototype`. Out of line: the cell it builds
/// lives in its frame, and a handler whose frame a callee addresses cannot dispatch its
/// successor as a sibling call.
#[inline(never)]
fn lane_new_array<H: Host>(vm: &mut Vm<H>, prototype: Value, args: &[Value]) -> Value {
    vm.heap.alloc(Cell::Array {
        object: crate::heap::ArrayObject::new(Vm::<H>::empty_object(prototype), args.to_vec()),
    })
}

script_handler! {
    /// A call that forwards without an observable step of its own (`Vm::forwarded_call`):
    /// `f.call(thisArg, ...args)`, a bound function without bound arguments, or an
    /// uncurried builtin (`uncurryThis(f)(thisArg, ...args)`). A frameless target's
    /// activation is the one `lane_script_call` makes, and a native runs as
    /// `lane_native_call` runs it, with the forwarded receiver and arguments. Any other
    /// takes the general path.
    #[inline(never)]
    fn lane_function_call<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the call's window is registers of the running function, which the
        // activation or native call copies before anything can move them.
        let window = unsafe { lane_call_window(r, i) };
        let Some((callee, receiver, args)) = vm.forwarded_call(r.get(i.b), r.get(i.c), window) else {
            return exit(ip);
        };
        if let Some(FunctionRecord { kind: kind @ FunctionKind::Native(_), .. }) = vm.heap.function(callee) {
            let kind = *kind;
            if i.a & crate::bytecode::RETURN_REGISTER != 0 && !lane_returning_call_admitted(cx) {
                return exit(ip);
            }
            let value = lane_native_value(cx, ip, i, kind, callee, receiver, args);
            return lane_native_complete(cx, ip, i, value);
        }
        match lane_function_call_activation(cx, r, ip, i, callee, receiver, args) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(next) => next,
        }
    }
}

/// `lane_function_call`'s activation of the frameless `callee` with the forwarded `receiver`
/// and `args`: the callee's registers and first record, or the record to continue at. Out of line, as `lane_construct_keeping_activation`.
#[inline(never)]
fn lane_function_call_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    let Some(target) = lane_passed_target(cx, callee, args.len()) else {
        return Err(lane_forwarded_frame_call(cx, r, ip, i, callee, receiver, args));
    };
    // `call_this_value` without its boxing of primitives, which allocates.
    let Some(this) = lane_call_this(vm, &target, receiver) else {
        return Err(exit(ip));
    };
    lane_activate_passed(cx, r, ip, &target, args, this)
}

/// The frameless target a forwarded or applied call of `callee` with `passed` values
/// activates: one of the running program (`lane_script_target`), another program's plain,
/// keeping or environment activation (`cross_call_entry`), or the running program's
/// environment target, which `lane_script_target` leaves to plain calls; an environment
/// target only when it binds each value to a parameter (`lane_activate_environment`).
#[inline(never)]
fn lane_passed_target<H: Host>(cx: *mut LaneContext<H>, callee: Value, passed: usize) -> Option<ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // Matched in turn rather than chained: the combinators moved the target through copies.
    let target = match lane_script_target(cx, callee) {
        Some(target) => target,
        None => {
            // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
            let cross = (!lane_constructing(vm, callee))
                .then(|| vm.cross_call_entry(callee))
                .flatten()
                .map(|entry| unsafe { &*entry })
                .filter(|target| target.entry.activates_across() || target.entry == CallEntry::Environment);
            // SAFETY: as above.
            let own = || {
                vm.script_call_entry(callee)
                    .map(|entry| unsafe { &*entry })
                    .filter(|target| target.entry == CallEntry::Environment && target.program == vm.active_program)
            };
            match cross {
                Some(target) => *target,
                None => *own()?,
            }
        }
    };
    if target.entry == CallEntry::Environment
        && passed > usize::from(target.params)
        && !vm.programs.residual(target.program)?.function(target.function as usize).rest
    {
        return None;
    }
    Some(target)
}

/// The activation of a `lane_passed_target` with receiver `this` and the `passed` values:
/// an environment activation, or a keeping one, which keeps them and may enter another
/// program (`Completion::Cross`).
#[inline(always)]
fn lane_activate_passed<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    if target.entry == CallEntry::Environment {
        return lane_activate_environment(cx, r, ip, target, passed, this);
    }
    let arguments = ActivationArguments::Kept(passed.as_ptr(), passed.len());
    lane_activate_keeping::<H, true>(cx, r, ip, target, arguments, this, false, None)
}

/// `lane_function_call` of a forwarded `callee` with no frameless target: a user function
/// the general loop pushes in place (`Vm::in_loop_call_target`) takes its frame as
/// `lane_frame_call` makes it, with the forwarded `receiver` and `args`, the call window's
/// tail. Any other takes the general path.
#[inline(never)]
fn lane_forwarded_frame_call<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Ip {
    let skip = usize::from(ImmediateLayout::argument_count(i.imm())) - args.len();
    lane_passed_frame_call(cx, r, ip, i, callee, receiver, FrameCallArguments::Window(skip))
}

/// The frame call of `callee` with `receiver` and `passed` that `lane_forwarded_frame_call`
/// and an applied call (`lane_apply_activation`) make.
fn lane_passed_frame_call<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    passed: FrameCallArguments<'_>,
) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    if !unsafe { (*cx).script_calls } || returns && !lane_returning_call_admitted(cx) {
        return exit(ip);
    }
    let Some(target) = vm.in_loop_call_target(code, callee) else {
        #[cfg(feature = "profile-lane-exits")]
        super::lane_exits::record_detail(format!(
            "passed frame call declined kind={:?} realm_matches={} name={:?}",
            vm.heap.function(callee).map(|record| match record.kind {
                FunctionKind::Native(Native::HostFunction) => vm.host_function_name(callee).unwrap_or_default().to_string(),
                FunctionKind::Native(native) => format!("{native:?}"),
                FunctionKind::User(..) => "user".to_string(),
            }),
            vm.heap.function(callee).is_some_and(|record| record.realm == vm.realm.globals),
            vm.heap.function(callee).and_then(|record| match record.kind {
                FunctionKind::User(program, function) => vm.programs.residual(program).and_then(|code| {
                    let body = code.function_get(function as usize)?;
                    Some(format!("{} @{:?} async={} generator={} in_loop={}",
                        body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                        body.source_span.map(|span| span.start), body.is_async, body.is_generator, body.call_plan(code).in_loop))
                }),
                _ => None,
            }),
        ));
        return exit(ip);
    };
    lane_push_frame_call(cx, r, ip, i, target, callee, receiver, passed)
}

/// The values a frame call passes its callee (`lane_push_frame_call`).
#[derive(Clone, Copy)]
enum FrameCallArguments<'a> {
    /// The call window's values past its first `skip` (a forwarded call's own arguments),
    /// read again once the caller is a frame.
    Window(usize),
    /// Values the caller copied out, as an applied list's elements.
    List(&'a [Value]),
}

script_handler! {
    /// A `Call` of `Array.prototype.push` or `pop` on a dense array (`Vm::array_push_dense`,
    /// `Vm::array_pop_dense`), which completes without calling out or allocating cells, or
    /// of a string method that runs no guest code (`Vm::pure_string_method_call`), or of a
    /// native the running frame calls in place (`lane_call_native`); out of line, so script
    /// calls keep their code. Any other call takes the general path.
    #[inline(never)]
    fn lane_native_call<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // The callee's kind, read once: a call changes no function record before it runs.
        let kind = match vm.heap.function(r.get(i.b)) {
            Some(FunctionRecord {
                kind: kind @ FunctionKind::Native(_),
                ..
            }) => *kind,
            _ => sibling!(lane_frame_call::<H>, r, ip, Carried::script(cx)),
        };
        if matches!(kind, FunctionKind::Native(Native::FunctionCall | Native::FunctionBoundCall)) {
            sibling!(lane_function_call::<H>, r, ip, Carried::script(cx))
        }
        if matches!(kind, FunctionKind::Native(Native::FunctionApply | Native::ReflectApply)) {
            sibling!(lane_apply_call::<H>, r, ip, Carried::script(cx))
        }
        lane_native_call_value(cx, r, ip, i, kind)
    }
}

script_handler! {
    /// `lane_native_call` of `Function.prototype.apply` or `Reflect.apply`: the applied
    /// function's activation where its list allows one (`lane_apply_activation`), else the
    /// native's call. Apart, since the activation's result is a stack temporary of the
    /// handler that makes it, whose dispatch then is a call, not a sibling jump.
    #[inline(never)]
    fn lane_apply_call<>(cx, r, ip, i, view) {
        let Some(FunctionRecord { kind: kind @ FunctionKind::Native(native), .. }) =
            (unsafe { vm(cx) }).heap.function(r.get(i.b))
        else {
            unreachable!("lane_native_call routes only a native apply here");
        };
        let (kind, native) = (*kind, *native);
        if let Some(activation) = lane_apply_activation(cx, r, ip, i, native) {
            return match activation {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                Err(next) => next,
            };
        }
        lane_native_call_value(cx, r, ip, i, kind)
    }
}

/// The native call `lane_native_call` makes in place (`lane_native_value`), completed in its
/// result register or as its caller's return (`lane_native_complete`).
#[inline(always)]
fn lane_native_call_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    kind: FunctionKind,
) -> Ip {
    // A call that returns its result completes its caller: an activation in the lane, or
    // the frame in the general loop (`lane_frame_return`).
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    // The running function is the same activation or, once the call made activations
    // frames (`lane_call_native`), the frame it became.
    if returns && !lane_returning_call_admitted(cx) {
        return exit(ip);
    }
    // SAFETY: the call's window is registers of the running function, which the native
    // call copies before anything can move them.
    let args = unsafe { lane_call_window(r, i) };
    let value = lane_native_value(cx, ip, i, kind, r.get(i.b), r.get(i.c), args);
    lane_native_complete(cx, ip, i, value)
}

/// `f.apply(thisArg, list)` (`native` `FunctionApply`, `f` the receiver) or
/// `Reflect.apply(f, thisArg, list)` (`ReflectApply`) of a frameless `f` and a list that is a
/// dense array without holes, or absent for `apply`: `f`'s activation with the list's
/// elements, which CreateListFromArrayLike reads as its own data elements and the activation
/// copies before anything runs, as `lane_function_call_activation` activates a call's. `None`
/// for any other call, which the native makes.
#[inline(never)]
fn lane_apply_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
) -> Option<Result<(Registers, Ip), Ip>> {
    // SAFETY: the call's window is registers of the running function.
    let args = unsafe { lane_call_window(r, i) };
    let argument = |index: usize| args.get(index).copied().unwrap_or(Value::UNDEFINED);
    let (callee, receiver, list) = match native {
        Native::FunctionApply => (r.get(i.c), argument(0), argument(1)),
        _ => (argument(0), argument(1), *args.get(2)?),
    };
    let listed = !(native == Native::FunctionApply && (list.is_undefined() || list.is_null()));
    #[cfg(feature = "profile-lane-exits")]
    {
        let vm = unsafe { vm(cx) };
        super::lane_exits::record_detail(format!(
            "apply callee={:?} array={} arguments_object={} sparse={} holes={} activations={}",
            vm.heap.function(callee).map(|record| match record.kind {
                FunctionKind::User(..) => "user",
                FunctionKind::Native(_) => "native",
            }),
            vm.heap.array(list).is_some(),
            vm.heap.array(list).is_some_and(|array| array.is_arguments_object()),
            vm.heap.sparse_length(list).is_some(),
            vm.heap.array(list).is_some_and(|array| array.elements().as_slice().iter().any(|value| value.is_deleted())),
            vm.lane_stack.activations.len(),
        ));
    }
    lane_list_activation(cx, r, ip, i, callee, receiver, list, listed)
}

/// `callee`'s activation with `receiver` and the elements of `list`, a dense array without
/// holes (none when not `listed`), for an applied call or a spread call (`CallArray`), whose
/// argument array the call's spread evaluation built; `None` for any other list.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn lane_list_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    list: Value,
    listed: bool,
) -> Option<Result<(Registers, Ip), Ip>> {
    let vm = unsafe { vm(cx) };
    let dense = |vm: &Vm<H>| {
        vm.heap.array(list).is_some_and(|array| {
            !array.is_arguments_object()
                && vm.heap.sparse_length(list).is_none()
                && !array.elements().as_slice().iter().any(|value| value.is_deleted())
        })
    };
    if listed && !dense(vm) {
        return None;
    }
    // A bound function or `Function.prototype.call` forwards the list to the function it
    // names (`Vm::forwarded_call`), which a forwarded call activates; the elements are copied
    // out first, as a frame call's are.
    if matches!(
        vm.heap.function(callee),
        Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall | Native::FunctionCall), .. })
    ) {
        let elements = if listed {
            vm.heap.array_elements(list)?.as_slice().iter().copied().collect()
        } else {
            smallvec::SmallVec::<[Value; FRAME_CALL_INLINE_ARGUMENTS]>::new()
        };
        let Some((target, receiver, args)) = vm.forwarded_list_call(callee, receiver, elements) else {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::record_detail("applied forwarding declined".into());
            return None;
        };
        let args = args.as_slice();
        if !matches!(vm.heap.function(target), Some(FunctionRecord { kind: FunctionKind::User(..), .. })) {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::record_detail("applied forwarding to a native".into());
            return None;
        }
        let Some(activated) = lane_passed_target(cx, target, args.len()) else {
            return Some(Err(lane_passed_frame_call(cx, r, ip, i, target, receiver, FrameCallArguments::List(args))));
        };
        let Some(this) = lane_call_this(vm, &activated, receiver) else {
            #[cfg(feature = "profile-lane-exits")]
            super::lane_exits::record_detail("applied forwarding this declined".into());
            return Some(Err(exit(ip)));
        };
        return Some(lane_activate_passed(cx, r, ip, &activated, args, this));
    }
    let length = if listed { vm.heap.array(list).map_or(0, |array| array.elements().len()) } else { 0 };
    let Some(target) = lane_passed_target(cx, callee, length) else {
        // A user function that needs a frame takes it in place (`lane_passed_frame_call`), with
        // the elements copied out first: making the caller's
        // activations frames moves no heap array, but the callee may change the list.
        let elements = if listed {
            vm.heap.array_elements(list)?.as_slice().iter().copied().collect()
        } else {
            smallvec::SmallVec::<[Value; FRAME_CALL_INLINE_ARGUMENTS]>::new()
        };
        return Some(Err(lane_passed_frame_call(cx, r, ip, i, callee, receiver, FrameCallArguments::List(&elements))));
    };
    let Some(this) = lane_call_this(vm, &target, receiver) else {
        return Some(Err(exit(ip)));
    };
    // Read after the resolution above, which runs no guest code but may allocate.
    let elements: &[Value] = if listed { vm.heap.array_elements(list)?.as_slice() } else { &[] };
    Some(lane_activate_passed(cx, r, ip, &target, elements, this))
}

script_handler! {
    /// A call of one spread (`CallSpread`), whose window holds the iterable: an intact array
    /// (`Vm::array_iteration_intact`) of present own data elements (`present_dense_elements`)
    /// passes them, which iterating it would yield and the activation copies, as a spread call's argument array
    /// (`lane_list_activation`); any other iterable takes the general path, which builds it.
    #[inline(never)]
    fn lane_call_spread<>(cx, r, ip, i, view) {
        let list = r.get(ImmediateLayout::call_window_base(i.imm()));
        let vm = unsafe { vm(cx) };
        let length = vm.heap.array(list).map_or(0, |array| array.elements().len());
        if !vm.array_iteration_intact(list) || vm.present_dense_elements(list, 0..length).is_none() {
            return exit(ip);
        }
        match lane_list_activation(cx, r, ip, i, r.get(i.b), r.get(i.c), list, true) {
            Some(Ok((registers, base))) => next!(cx, registers, base, Carried::script(cx)),
            Some(Err(next)) => next,
            None => exit(ip),
        }
    }
}

script_handler! {
    /// A spread call (`CallArray`), whose window holds the argument array the spread built:
    /// the callee's activation with its elements, as an applied call's
    /// (`lane_list_activation`); any other call takes the general path.
    #[inline(never)]
    fn lane_call_array<>(cx, r, ip, i, view) {
        let list = r.get(ImmediateLayout::call_window_base(i.imm()));
        match lane_list_activation(cx, r, ip, i, r.get(i.b), r.get(i.c), list, true) {
            Some(Ok((registers, base))) => next!(cx, registers, base, Carried::script(cx)),
            Some(Err(next)) => next,
            None => exit(ip),
        }
    }
}

/// Values an applied frame call copies out without allocating their list.
const FRAME_CALL_INLINE_ARGUMENTS: usize = 8;



/// The values a lane `Call` record passes: its window of the running function's registers.
///
/// # Safety
/// `r` is the registers of the activation `i` belongs to; the slice is valid until they move.
#[inline(always)]
unsafe fn lane_call_window<'a>(r: Registers, i: &LaneInstruction) -> &'a [Value] {
    let window = ImmediateLayout::call_window_base(i.imm());
    let argc = ImmediateLayout::argument_count(i.imm());
    unsafe { std::slice::from_raw_parts(r.0.add(usize::from(window)), usize::from(argc)) }
}

/// The value of the lane `Call` record `i` of the native `callee`, of `kind`, with
/// `receiver` and `args`: dense `push` and `pop`, `charCodeAt` and collection lookups in
/// place, else `lane_string_or_native_call`. `None` leaves the call to the general path,
/// which raises an error the call stashed in `Vm::lane_raised`. Out of line: its calls take
/// stack temporaries, which a handler inlining them would hold across its dispatch, which
/// then could not be a sibling call.
#[inline(never)]
fn lane_native_value<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    i: &LaneInstruction,
    kind: FunctionKind,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    match kind {
        // The JIT-mode DataView lane (`js_jit_dataview_float64_enabled`); a miss takes the
        // native, which coerces and throws.
        FunctionKind::Native(native @ (Native::DataViewGetFloat64 | Native::DataViewSetFloat64))
            if vm.execution_mode == super::ExecutionMode::Compiled
                && vm.programs.js_jit_dataview_float64_enabled() =>
        {
            let value = vm.lane_data_view_float64(native, receiver, args);
            if value.is_some() {
                vm.programs.note_js_jit_dataview_float64(native == Native::DataViewSetFloat64);
            }
            value.or_else(|| lane_string_or_native_call(cx, ip, native, callee, receiver, args))
        }
        FunctionKind::Native(Native::ArrayPush) => vm.array_push_dense(receiver, args),
        FunctionKind::Native(Native::ArrayPop) => vm.array_pop_dense(receiver),
        FunctionKind::Native(Native::StringCharCodeAt) if !args.is_empty() => {
            string_char_code_at(vm, receiver, args[0])
                .or_else(|| lane_string_or_native_call(cx, ip, Native::StringCharCodeAt, callee, receiver, args))
        }
        FunctionKind::Native(
            native @ (Native::MapGet
            | Native::MapHas
            | Native::SetHas
            | Native::WeakMapGet
            | Native::WeakMapHas
            | Native::WeakSetHas),
        ) => {
            let key = args.first().copied().unwrap_or(Value::UNDEFINED);
            vm.pure_collection_lookup(native, receiver, key)
                .or_else(|| lane_string_or_native_call(cx, ip, native, callee, receiver, args))
        }
        FunctionKind::Native(Native::MapSet) if vm.map_set(receiver, args) => {
            note_allocation(vm);
            Some(receiver)
        }
        FunctionKind::Native(Native::ObjectCreate)
            if args.get(1).is_none_or(|properties| properties.is_undefined()) =>
        {
            let proto = args.first().copied().unwrap_or(Value::UNDEFINED);
            vm.object_create(proto)
                .inspect(|_| note_allocation(vm))
                .or_else(|| lane_string_or_native_call(cx, ip, Native::ObjectCreate, callee, receiver, args))
        }
        FunctionKind::Native(Native::ArrayIsArray) => vm
            .steady_is_array(args.first().copied().unwrap_or(Value::UNDEFINED))
            .map(|holds| if holds { Value::TRUE } else { Value::FALSE })
            .or_else(|| lane_string_or_native_call(cx, ip, Native::ArrayIsArray, callee, receiver, args)),
        FunctionKind::Native(
            native @ (Native::RegExpExec | Native::RegExpTest | Native::StringReplace),
        ) => lane_regexp_call(cx, ip, i, native, callee, receiver, args),
        // A generator's resumption runs its body, guest code that no in-place shortcut of
        // `lane_string_or_native_call` makes.
        FunctionKind::Native(
            native @ (Native::GeneratorNext | Native::GeneratorReturn | Native::GeneratorThrow),
        ) => lane_call_native(cx, ip, native, callee, receiver, args),
        FunctionKind::Native(native) => lane_string_or_native_call(cx, ip, native, callee, receiver, args),
        // Only natives run from the lane (`lane_call_native`).
        _ => None,
    }
}

/// `charCodeAt` of a flat string at an integer index: the unit there, or NaN outside the
/// string. `None` for any other receiver or index, which the native coerces, and for a
/// rope, which it flattens.
#[inline(always)]
fn string_char_code_at<H: Host>(vm: &Vm<H>, receiver: Value, index: Value) -> Option<Value> {
    let (units, index) = (vm.heap.string(receiver)?.existing_flat()?, index.as_int()?);
    Some(
        usize::try_from(index)
            .ok()
            .and_then(|index| units.unit(index))
            .map_or(Value::NAN, |unit| Value::integer(i32::from(unit))),
    )
}

script_handler! {
    /// A `Call` site whose callee was `String.prototype.charCodeAt` (`lane_script_call_resolve`):
    /// that call of a string at an integer index in place (`string_char_code_at`). Any other
    /// callee, receiver or index, or a call returning its result, takes `lane_native_call`.
    fn lane_call_char_code_at<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let is_char_code_at = matches!(
            vm.heap.function(r.get(i.b)),
            Some(FunctionRecord { kind: FunctionKind::Native(Native::StringCharCodeAt), .. })
        );
        // SAFETY: the call's window is registers of the running function.
        let index = unsafe { lane_call_window(r, i) }.first().copied();
        let Some(value) = index
            .filter(|_| is_char_code_at && i.a & crate::bytecode::RETURN_REGISTER == 0)
            .and_then(|index| string_char_code_at(vm, r.get(i.c), index))
        else {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The lane `Call` record `i` completed with `value`: in its result register, or returned
/// from the caller for a call that returns its result; the general path for `None`.
#[inline(always)]
fn lane_native_complete<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    i: &LaneInstruction,
    value: Option<Value>,
) -> Ip {
    let vm = unsafe { vm(cx) };
    let Some(value) = value else {
        return exit(ip);
    };
    // The call may have made the lane's activations frames, so the running function's
    // registers and caller are read again.
    if i.a & crate::bytecode::RETURN_REGISTER != 0 {
        let Some(resumed) = lane_complete_activation(cx, ip, value) else {
            // A frame of a script run retires in place, as its `Return` would.
            if unsafe { !(*cx).inline_calls && (*cx).script_calls } {
                return lane_return_script_frame(cx, ip, value, true);
            }
            return lane_frame_return(vm, ip, value);
        };
        resume!(cx, resumed)
    }
    let r = unsafe { (*cx).registers };
    r.set(i.a, value);
    next!(cx, r, following(ip), Carried::script(cx))
}

/// `RegExp.prototype.exec` or `test`, or `String.prototype.replace` of a RegExp, made in
/// place when it runs no guest code and raises nothing, so it observes no frame
/// (`Vm::steady_regexp_call`, `Vm::steady_string_replace`); any other such call goes on as
/// `lane_string_or_native_call` makes it.
#[inline(never)]
fn lane_regexp_call<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let result_used = ImmediateLayout::lane_call_result_used(i.imm());
    let steady = match (native, args) {
        (Native::StringReplace, _) => vm.steady_string_replace(code, native, receiver, args, result_used),
        (_, [subject, ..]) => vm.steady_regexp_call(code, native, receiver, *subject, result_used),
        _ => None,
    };
    let Some(result) = steady else {
        return lane_string_or_native_call(cx, ip, native, callee, receiver, args);
    };
    // The call's allocations may make a collection due, which the lane's safepoints take.
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_raised = Some(error);
            None
        }
    }
}

/// A lane native call that `lane_native_call` does not make inline: a pure string method in
/// place, else the native through `lane_call_native`.
fn lane_string_or_native_call<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(value) = vm.pure_string_method_call(code, native, receiver, args) {
        return Some(value);
    }
    if let Some(value) = lane_pure_native(vm, code, native, callee, receiver, args) {
        return Some(value);
    }
    if let Some(value) = lane_promise_native(vm, code, native, callee, receiver, args) {
        note_allocation(vm);
        return Some(value);
    }
    lane_call_native(cx, ip, native, callee, receiver, args)
}

/// A lane call of a promise native whose steps provably observe nothing, answered in place
/// so the lane's activations need not become frames (`lane_call_native`): `then` and
/// `catch` of a receiver with intrinsic species (`Vm::lane_promise_then`) and the intrinsic
/// `Promise.resolve` (`Vm::lane_promise_resolve`). A resolving function (one with an
/// environment), any other shape, or a lifecycle-observing host takes the general call.
#[inline(never)]
fn lane_promise_native<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    if !matches!(native, Native::PromiseThen | Native::PromiseCatch | Native::PromiseResolve)
        || !vm.heap.function(callee).is_some_and(|function| function.env.is_null())
    {
        return None;
    }
    match native {
        Native::PromiseThen => vm.lane_promise_then(
            code,
            receiver,
            args.first().copied().unwrap_or(Value::UNDEFINED),
            args.get(1).copied().unwrap_or(Value::UNDEFINED),
        ),
        Native::PromiseCatch => {
            // `catch` reads `then` of its receiver and calls it: only the intrinsic `then`,
            // read as plain data, is the shortcut's to make.
            let then_atom = vm.runtime_atoms.then;
            let then = vm.plain_named_load(receiver, then_atom)?;
            if !matches!(
                vm.heap.function(then),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::PromiseThen), .. })
            ) {
                return None;
            }
            vm.lane_promise_then(
                code,
                receiver,
                Value::UNDEFINED,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )
        }
        Native::PromiseResolve => vm.lane_promise_resolve(
            code,
            receiver,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        ),
        _ => unreachable!("only promise shortcut natives reach here"),
    }
}

/// A native call whose common case runs no guest code and observes no frame, answered in
/// place so the lane's activations need not become frames (`lane_call_native`): a dense
/// array's `indexOf` (`Vm::dense_index_of`), `String.fromCharCode` of Numbers, which
/// allocates its string, `Number`'s predicates, which coerce nothing
/// (`number::number_predicate`), the global `isNaN` and `isFinite` of an argument whose
/// ToNumber reads nothing (`Value::immediate_number`), `Boolean` called as a function, whose
/// ToBoolean runs no guest code, `String` called with a string or an immediate primitive,
/// `ArrayBuffer.isView`, an array iterator's step over a plain array
/// (`Vm::plain_array_iterator_next`), which allocates its result, an object's own
/// property query by a string key (`Vm::plain_own_property_query`), and a dense array's
/// `splice` (`Vm::plain_array_splice`, which allocates the removed array), `unshift`
/// (`Vm::plain_array_unshift`) and `shift` (`Vm::plain_array_shift`), for-in's per-key
/// check (`Vm::plain_for_in_key_is_enumerable`), `Object.hasOwn`, `Object.is`, `Symbol.for`
/// of a string, and the Map and Set mutators on their own collections
/// (`Vm::plain_collection_mutation`), and a DataView element get or set that raises nothing
/// (`Vm::plain_data_view_access`). `None` leaves any other call to `lane_call_native`.
#[inline(never)]
fn lane_pure_native<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let native = &native;
    let argument = args.first().copied().unwrap_or(Value::UNDEFINED);
    match native {
        Native::ArrayIndexOf => vm.dense_index_of(receiver, args),
        Native::StringFromCharCode => {
            let text = vm.from_char_code_numbers(args)?;
            note_allocation(vm);
            Some(text)
        }
        Native::ObjectPrototypeHasOwnProperty | Native::ObjectPrototypePropertyIsEnumerable => {
            vm.plain_own_property_query(code, *native, receiver, *args.first()?)
        }
        Native::ArraySplice => {
            let removed = vm.plain_array_splice(code, callee, receiver, args)?;
            note_allocation(vm);
            Some(removed)
        }
        native if super::data_view::data_view_element(*native) => {
            vm.plain_data_view_access(*native, receiver, args)
        }
        Native::ArrayUnshift => vm.plain_array_unshift(receiver, args),
        Native::ArrayShift => vm.plain_array_shift(receiver),
        Native::ForInKeyIsEnumerable => vm.plain_for_in_key_is_enumerable(code, *args.first()?, *args.get(1)?),
        Native::ObjectHasOwn => vm.plain_own_property_query(
            code,
            Native::ObjectPrototypeHasOwnProperty,
            *args.first()?,
            *args.get(1)?,
        ),
        // Pure of any arguments: SameValue.
        Native::ObjectIs => vm.call_object_native(code, Native::ObjectIs, args).ok(),
        // `super` reads (`Reflect.get` and `Reflect.getPrototypeOf` of a home object): an
        // ordinary target's data property, whose read ignores the receiver, and its prototype.
        Native::ReflectGet => {
            let target = *args.first()?;
            vm.heap.ordinary_object(target)?;
            vm.plain_keyed_get(target, *args.get(1)?)
        }
        Native::ReflectGetPrototypeOf => Some(vm.heap.ordinary_object(*args.first()?)?.proto()),
        // The registry lookup of a string key, which ToString leaves as it is.
        Native::SymbolFor if args.first().is_some_and(|key| vm.heap.is_string(*key)) => {
            let symbol = vm.call_symbol_native(code, Native::SymbolFor, args).ok()?;
            note_allocation(vm);
            Some(symbol)
        }
        native @ (Native::MapSet
        | Native::MapDelete
        | Native::MapClear
        | Native::SetAdd
        | Native::SetDelete
        | Native::SetClear) => vm.plain_collection_mutation(code, *native, receiver, args),
        Native::ArrayIteratorNext => {
            let result = vm.plain_array_iterator_next(callee, receiver)?;
            note_allocation(vm);
            Some(result)
        }
        Native::ArrayBufferIsView => Some(Value::boolean(vm.is_array_buffer_view(argument))),
        Native::Boolean => Some(Value::boolean(vm.truthy(argument))),
        // `String(value)` of a string is that string; of a primitive whose ToString reads no
        // cell, its spelling (`coercion::immediate_to_string`).
        Native::String if !args.is_empty() => {
            if vm.heap.string(argument).is_some() {
                return Some(argument);
            }
            let text = super::coercion::immediate_to_string(argument)?;
            let value = vm.heap.alloc_string(super::wtf16::JsString::from_str(&text));
            note_allocation(vm);
            Some(value)
        }
        // `Number(value)` of a primitive whose ToNumber reads no cell but a string's text, as
        // Node's `utf8Write` converts its offset and length; a BigInt, a Symbol and any object
        // take the general call.
        Native::Number if !args.is_empty() => vm.primitive_to_number(argument).map(Value::number),
        // `Number.prototype.toString` of a Number primitive in decimal, as a status code's
        // `toString()` reads; a wrapper object, another radix or a radix to coerce take the
        // general call.
        Native::NumberString
            if args.first().is_none_or(|radix| radix.is_undefined() || radix.as_number() == Some(10.0)) =>
        {
            let number = receiver.as_number()?;
            let text = vm.heap.alloc_string(super::number::number_to_decimal(number).into());
            note_allocation(vm);
            Some(text)
        }
        Native::GlobalIsNaN | Native::GlobalIsFinite => {
            super::number::global_number_predicate(*native, vm.primitive_to_number(argument)?).map(Value::boolean)
        }
        native => super::number::number_predicate(*native, argument).map(Value::boolean),
    }
}

/// A lane `Call` of a native that `Native::runs_from_lane` admits, made as the general path
/// makes it from the running frame: with the frame's resume PC published and the arguments
/// copied out of its registers, which stay rooted by the frame. Only a frame's own lane run
/// calls out, so the frame stack the native and its nested calls observe is the general
/// path's. `None` exits to the general path, which raises the error the call stashed in
/// `Vm::lane_raised` or makes a call this does not admit.
#[inline(never)]
fn lane_call_native<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !native.runs_from_lane() {
        return None;
    }
    // A unary Math function of a Number runs no guest code and observes no frame, so it
    // runs from any activation.
    if native.is_math_function()
        && let Some(value) = args.first().and_then(|value| value.as_number())
        && let Some(result) = super::number::math_unary(native, value)
    {
        return Some(Value::number(result));
    }
    // So do `Math.max` and its kin of Numbers (`math_numbers`).
    if native.is_math_function()
        && let Some(numbers) = args
        .iter()
        .map(|value| value.as_number())
        .collect::<Option<smallvec::SmallVec<[f64; super::number::MATH_INLINE_ARGUMENTS]>>>()
        && let Some(result) = super::number::math_numbers(native, &numbers)
    {
        return Some(Value::number(result));
    }
    if !unsafe { (*cx).script_calls } {
        return None;
    }
    // The values are copied before activations become frames, which moves the registers
    // they may lie in; the frames then root what the call passes.
    let arguments = super::call_arguments::CallArguments::from_slice(args);
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    // A native that observes frames only through the points `materialize_deferred_lane`
    // guards runs beside the activations; the first such point makes them frames.
    if !vm.lane_stack.activations.is_empty() && vm.runs_beside_activations(native, callee) {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc });
        let result = vm.call_native_guarded(unsafe { &*(*cx).code }, native, receiver, arguments.as_slice(), callee);
        note_allocation(vm);
        let unobserved = vm.lane_deferred.take().is_some();
        return match result {
            Ok(value) => Some(value),
            Err(error) => {
                // The general path raises it from the frames its activations become.
                if unobserved {
                    vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc });
                    vm.materialize_deferred_lane();
                }
                vm.lane_raised = Some(error);
                None
            }
        };
    }
    #[cfg(feature = "profile-lane-exits")]
    if !vm.lane_stack.activations.is_empty() {
        super::lane_exits::record_detail(match native {
            Native::HostFunction => format!("materializes for host {}", vm.host_function_name(callee).unwrap_or_default()),
            native => format!("materializes for native {native:?}"),
        });
    }
    // The callee observes the frame stack: lane activations become frames first, as leaving
    // the lane would make them, and the call runs from the innermost one.
    vm.materialize_lane_activations(cx);
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // active view, which is the running frame's.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame) };
    vm.frames[frame].pc = call_pc + 1;
    vm.frames[frame].pc_is_resume = true;
    // The native branch of `Vm::call_value_with_target_inner`, whose callee this is; its
    // calls of JavaScript run in this lane while it runs from this frame.
    let site = LaneNativeCall { cx: cx.cast(), call: ip, frames: vm.frames.len() };
    let outer = vm.lane_native.replace(site);
    let result = vm.call_native_guarded(code, native, receiver, arguments.as_slice(), callee);
    vm.lane_native = outer;
    // The native's allocations may make a collection due, which the lane's safepoints take.
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_raised = Some(error);
            None
        }
    }
}

/// A lane `new` of a native that `Native::constructs_from_lane` admits, made as the general
/// path makes it (`Vm::construct_value_with_new_target`) from the running frame, after
/// activations become frames, as `lane_call_native` makes a call. `None` exits to the general
/// path, which raises the error the construction stashed in `Vm::lane_raised` or makes one
/// this does not admit.
#[inline(never)]
fn lane_construct_native<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    callee: Value,
    new_target: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.construct_target.is_some() {
        return None;
    }
    // The values are copied before activations become frames, which moves the registers
    // they may lie in; the frames then root what the construction passes.
    let arguments = super::call_arguments::CallArguments::from_slice(args);
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // running function's view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    // A native's construction runs beside the activations, as a native call beside them
    // runs (`lane_call_native`): the first point that observes frames makes them frames.
    let beside = !vm.lane_stack.activations.is_empty() && vm.constructs_beside_activations(callee);
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc });
    } else {
        vm.materialize_lane_activations(cx);
        let frame = unsafe { (*cx).frame };
        vm.frames[frame].pc = call_pc + 1;
        vm.frames[frame].pc_is_resume = true;
    }
    // SAFETY: as above; the construction runs from the running function's program.
    let code = unsafe { &*(*cx).code };
    let result = vm.construct_value_with_new_target(code, callee, new_target, arguments.as_slice());
    // The construction's allocations may make a collection due, which the lane's safepoints
    // take.
    note_allocation(vm);
    let unobserved = beside && vm.lane_deferred.take().is_some();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            // The general path raises it from the frames the activations become.
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc });
                vm.materialize_deferred_lane();
            }
            vm.lane_raised = Some(error);
            None
        }
    }
}

/// The native parent a `new` of `callee` constructs in its place: `callee`'s own parent
/// (`[[GetPrototypeOf]]`) when its constructor only constructs it
/// (`Function::super_forwarding`) and that parent is a native the lane constructs other
/// than an error constructor, with what the constructor passes on.
#[inline(never)]
fn lane_super_forwarded_parent<H: Host>(vm: &Vm<H>, callee: Value) -> Option<(Value, crate::bytecode::SuperForwarded)> {
    let Some(FunctionRecord { kind: FunctionKind::User(program, function), .. }) = vm.heap.function(callee) else {
        return None;
    };
    let code = vm.programs.residual(*program)?;
    let forwarded = code.function_get(*function as usize)?.super_forwarding(&code.atoms, &code.constants)?;
    let parent = vm.object_data(callee)?.proto();
    match vm.heap.function(parent) {
        // An error's stack starts past its constructors' frames, which this construction
        // would not make.
        Some(FunctionRecord { kind: FunctionKind::Native(native), .. })
            if native.constructs_from_lane() && !native.is_error_constructor() =>
        {
            Some((parent, forwarded))
        }
        _ => None,
    }
}

/// `new callee(...)` of a class whose constructor only constructs its native `parent`
/// (`lane_super_forwarded_parent`): the parent's construction with what the constructor
/// passes on (every value, or its parameters with missing ones `undefined`) and `callee`
/// as `new.target`, as the body's `super(...)` makes it; its receiver is the construction's
/// value.
#[inline(never)]
fn lane_construct_forwarded_parent<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    (parent, forwarded): (Value, crate::bytecode::SuperForwarded),
) -> Ip {
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    let count = match forwarded {
        crate::bytecode::SuperForwarded::Passed => argc,
        crate::bytecode::SuperForwarded::Parameters(count) => usize::from(count),
    };
    let passed = (0..count)
        .map(|index| if index < argc { r.get(i.c + index as u16) } else { Value::UNDEFINED })
        .collect::<smallvec::SmallVec<[Value; GATHERED_INLINE_VALUES]>>();
    let value = lane_construct_native(cx, ip, parent, callee, &passed);
    lane_native_complete(cx, ip, i, value)
}

script_handler! {
    /// `new` of a frameless target that `Vm::inline_construct_target` would push: the
    /// receiver it allocates, then the activation `lane_script_call` makes, whose
    /// completion substitutes the receiver for a non-object result. Any other
    /// construction takes the general path.
    fn lane_script_construct<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if !unsafe { (*cx).script_calls && (*cx).script_constructs }
            || vm.construct_target.is_some()
        {
            return exit(ip);
        }
        // The cached target in place, as a call reads it (`Vm::script_call_entry`); an
        // uncached callee resolves (`lane_script_target`), after which the construction
        // retries.
        let entry = if lane_constructing(vm, callee) {
            None
        } else {
            vm.script_call_entry(callee).or_else(|| vm.cross_call_entry(callee))
        };
        let Some(entry) = entry else {
            if lane_script_target(cx, callee).is_some_and(|target| target.constructs) {
                sibling!(lane_script_construct::<H>, r, ip, Carried::script(cx))
            }
            sibling!(lane_native_construct::<H>, r, ip, Carried::script(cx))
        };
        // SAFETY: as `Vm::script_call_entry`; the planned construction below resolves no
        // target, and any other path copies the target first.
        let target = unsafe { &*entry };
        // A class constructor's target constructs with entry `Frame`, which its calls take.
        if !target.constructs {
            sibling!(lane_native_construct::<H>, r, ip, Carried::script(cx))
        }
        // A planned construction, the common `new C(…)` or `return new C(…)`: the receiver
        // is the finished object, and the body completed with `undefined`.
        if target.initializes
            && lane_finishes_construction(cx, i)
            && let Some(this) = lane_planned_construction(vm, callee, r, i)
        {
            // Allocating the receiver can make a collection due.
            note_allocation(vm);
            // Later runs of the site try the plan first.
            if i.a & crate::bytecode::RETURN_REGISTER == 0 {
                requicken::<H>(ip, lane_planned_construct::<H>);
                r.set(i.a, this);
                next!(cx, r, following(ip), Carried::script(cx))
            }
            requicken::<H>(ip, lane_planned_construct_returned::<H>);
            if let Some(resumed) = lane_complete_activation(cx, ip, this) {
                resume!(cx, resumed)
            }
            return lane_frame_return(vm, ip, this);
        }
        let target = *target;
        if target.forwards() {
            // The site constructs a forwarding target: later runs enter the keeping
            // construction directly, which moves the record back for any other target.
            requicken::<H>(ip, lane_script_construct_keeping::<H>);
            sibling!(lane_script_construct_keeping::<H>, r, ip, Carried::script(cx))
        }
        let built = if target.initializes {
            match lane_planned_construction(vm, callee, r, i) {
                Some(this) => Some((this, true)),
                // SAFETY: the context's program outlives the lane run.
                None => lane_initialize_fields(vm, unsafe { &*(*cx).code }, callee, target.function, r, i),
            }
        } else {
            vm.inline_construct_receiver(callee).map(|this| (this, false))
        };
        let Some((this, initialized)) = built else {
            return exit(ip);
        };
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        if initialized {
            // The body completed with `undefined`, so [[Construct]] yields the receiver.
            if i.a & crate::bytecode::RETURN_REGISTER == 0 {
                r.set(i.a, this);
                next!(cx, r, following(ip), Carried::script(cx))
            }
            if let Some(resumed) = lane_complete_activation(cx, ip, this) {
                resume!(cx, resumed)
            }
            // The frame's own construct: the general loop returns the receiver from it, unless
            // proper tail calls send it to the activation below, which leaves for that loop.
            if lane_returning_call_admitted(cx) {
                return lane_frame_return(vm, ip, this);
            }
        }
        match lane_activate(cx, r, ip, &target, i.c, this, true) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(next) => next,
        }
    }
}

script_handler! {
    /// `lane_script_construct` at a site into a register whose construction was planned
    /// (`lane_planned_construction`), which `lane_script_construct` moves it to: the plan of
    /// the callee alone, without resolving its call target. A plan exists only for a callee
    /// whose target constructs by its initializer stores, which its function fixes, and while
    /// it holds the finished object is what the activation would make. Any other construction
    /// moves the record back to `lane_script_construct`.
    fn lane_planned_construct<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !unsafe { (*cx).script_calls && (*cx).script_constructs } || vm.construct_target.is_some() {
            return exit(ip);
        }
        let Some(this) = lane_planned_construction(vm, r.get(i.b), r, i) else {
            requicken::<H>(ip, lane_script_construct::<H>);
            sibling!(lane_script_construct::<H>, r, ip, Carried::script(cx))
        };
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        r.set(i.a, this);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_planned_construct` at a site whose construction is returned (`RETURN_REGISTER`),
    /// which `lane_script_construct` moves it to: the planned receiver completes the
    /// activation that returns it, or the frame's own returning construct where
    /// `lane_finishes_construction` admits it. Any other construction moves the record back
    /// to `lane_script_construct`.
    fn lane_planned_construct_returned<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !unsafe { (*cx).script_calls && (*cx).script_constructs } || vm.construct_target.is_some() {
            return exit(ip);
        }
        let planned = lane_finishes_construction(cx, i)
            .then(|| lane_planned_construction(vm, r.get(i.b), r, i))
            .flatten();
        let Some(this) = planned else {
            requicken::<H>(ip, lane_script_construct::<H>);
            sibling!(lane_script_construct::<H>, r, ip, Carried::script(cx))
        };
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        if let Some(resumed) = lane_complete_activation(cx, ip, this) {
            resume!(cx, resumed)
        }
        lane_frame_return(vm, ip, this)
    }
}

script_handler! {
    /// `lane_script_call` of a target that keeps its passed values
    /// (`ScriptCallTarget::forwards`): the keeping activation (`lane_call_keeping_activation`).
    /// Out of line, so ordinary calls keep their code.
    #[inline(never)]
    fn lane_script_call_keeping<>(cx, r, ip, i, view) {
        match lane_call_keeping_activation(cx, r, ip, i) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(Some(next)) => next,
            // A site this form serves since `lane_script_call` saw a keeping target calls
            // the full form once its callee keeps nothing.
            Err(None) => {
                requicken::<H>(ip, lane_script_call_full::<H>);
                sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
            }
        }
    }
}

/// `lane_script_call_keeping`'s activation: the callee's registers and first record, the
/// record to continue at, or `None` for a callee whose target keeps nothing, which the full
/// form calls. Out of line, as `lane_construct_keeping_activation`.
#[inline(never)]
fn lane_call_keeping_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Option<Ip>> {
    if !unsafe { (*cx).script_calls } {
        return Err(Some(exit(ip)));
    }
    let vm = unsafe { vm(cx) };
    let Some(target) = lane_script_target(cx, r.get(i.b)) else {
        return Err(None);
    };
    if !target.forwards() {
        return Err(None);
    }
    #[cfg(feature = "profile-lane-exits")]
    profile_script_call_target(cx, ip, r.get(i.b), &target);
    let receiver = r.get(i.c);
    let Some(this) = lane_call_this(vm, &target, receiver) else {
        return Err(Some(exit(ip)));
    };
    let arguments = ActivationArguments::Window(ImmediateLayout::call_window_base(i.imm()));
    lane_activate_keeping::<H, false>(cx, r, ip, &target, arguments, this, false, None).map_err(Some)
}

script_handler! {
    /// `lane_script_construct` of a target that keeps its passed values
    /// (`ScriptCallTarget::forwards`), which `lane_script_construct` moves a site to: the
    /// keeping activation (`lane_construct_keeping_activation`). Any other construction moves
    /// the record back to `lane_script_construct`. Out of line, so ordinary constructions
    /// keep their code.
    #[inline(never)]
    fn lane_script_construct_keeping<>(cx, r, ip, i, view) {
        match lane_construct_keeping_activation(cx, r, ip, i) {
            Ok(KeepingConstruction::Activated(registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Ok(KeepingConstruction::Finished(this)) => {
                if i.a & crate::bytecode::RETURN_REGISTER == 0 {
                    r.set(i.a, this);
                    next!(cx, r, following(ip), Carried::script(cx))
                }
                if let Some(resumed) = lane_complete_activation(cx, ip, this) {
                    resume!(cx, resumed)
                }
                // The frame's own construct, which `lane_finishes_construction` admitted: the
                // general loop returns the receiver from it.
                lane_frame_return(unsafe { vm(cx) }, ip, this)
            }
            Err(Some(next)) => next,
            Err(None) => {
                requicken::<H>(ip, lane_script_construct::<H>);
                sibling!(lane_script_construct::<H>, r, ip, Carried::script(cx))
            }
        }
    }
}

/// What `lane_construct_keeping_activation` made of a construction.
enum KeepingConstruction {
    /// The callee's registers and first record.
    Activated(Registers, Ip),
    /// The construction's result, whose body ran without an activation: its site completes
    /// with it.
    Finished(Value),
}

/// Whether a construction at `i` may complete in the lane with a receiver its body finished
/// without an activation: into a register, into the activation that returns it, or from the
/// frame's own returning construct where `lane_returning_call_admitted` admits it, as
/// `lane_script_construct` completes one.
#[inline(always)]
fn lane_finishes_construction<H: Host>(cx: *mut LaneContext<H>, i: &LaneInstruction) -> bool {
    i.a & crate::bytecode::RETURN_REGISTER == 0
        || !unsafe { vm(cx) }.lane_stack.activations.is_empty()
        || lane_returning_call_admitted(cx)
}

/// `lane_script_construct_keeping`'s activation, under `lane_script_construct`'s admission:
/// the callee's registers and first record, or the receiver its body finished; the record
/// to continue at, or `None` for a construction of any target but a forwarding one. Out of line: the call target it passes
/// on lives in its frame, and a handler whose frame a callee addresses cannot dispatch its
/// successor as a sibling call.
#[inline(never)]
fn lane_construct_keeping_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<KeepingConstruction, Option<Ip>> {
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    if !unsafe { (*cx).script_calls && (*cx).script_constructs } || vm.construct_target.is_some() {
        return Err(Some(exit(ip)));
    }
    let finishes = lane_finishes_construction(cx, i);
    // A plan holding for `callee` fixes its function, so the plan alone answers before its
    // target is resolved.
    if finishes && let Some(this) = lane_forwarded_fields(vm, callee, r, i) {
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        return Ok(KeepingConstruction::Finished(this));
    }
    // Matched rather than filtered: the combinators moved the target through copies.
    let Some(target) = lane_script_target(cx, callee) else {
        return Err(None);
    };
    if !(target.constructs && target.forwards()) {
        return Err(None);
    }
    // One look at the plan serves the receiver and the forwarding: a plan the receiver's
    // allocation leaves records no resolution yet.
    let plan = holding_construct_plan(vm, callee);
    let Some(this) = lane_planned_receiver(vm, callee, plan.as_ref()) else {
        return Err(Some(exit(ip)));
    };
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    let arguments = ActivationArguments::Window(i.c);
    if let Some(&method) = vm.lane_stack.forwarded_methods.get(target.forwarded_method as usize)
        && let Some(forwarded) = match lane_resolved_forwarding(cx, callee, plan.as_ref()) {
            Some(forwarded) => Some(forwarded),
            None => lane_forwarded_construction(cx, target, method, this),
        }
    {
        // A forwarded method that only initializes `this` from its parameters, which the
        // forwarding passes the construction's arguments as: its adds alone, with neither
        // activation, where the construction completes in the lane (`lane_finishes_construction`).
        // Its body completes with `undefined`, as the forwarding body then does, so
        // [[Construct]] yields `this`.
        if forwarded.initializes && finishes {
            // SAFETY: the context's program outlives the lane run, and the forwarded target is
            // one of its functions (`lane_script_target`).
            let code = unsafe { &*(*cx).code };
            let mut fields = Some(EMPTY_CONSTRUCT_PLAN);
            let argument = lane_construct_argument(r, i);
            if lane_add_initializer_fields(vm, code, this, forwarded.function, argument, |store| {
                fields = fields.and_then(|fields| fields.with_field(store));
            }) {
                if let Some(fields) = fields {
                    lane_record_forwarded_fields(vm, callee, (forwarded.program, forwarded.function), this, &fields);
                }
                return Ok(KeepingConstruction::Finished(this));
            }
        }
        let forwarder = SkippedForwarder {
            function: target.function,
            callee,
            env: target.env,
            method,
        };
        return lane_activate_keeping::<H, false>(cx, r, ip, &forwarded, arguments, this, false, Some(forwarder))
            .map(|(registers, base)| KeepingConstruction::Activated(registers, base))
            .map_err(Some);
    }
    lane_activate_keeping::<H, false>(cx, r, ip, &target, arguments, this, true, None)
        .map(|(registers, base)| KeepingConstruction::Activated(registers, base))
        .map_err(Some)
}

/// The finished receiver of a forwarding constructor's construction under `callee`'s
/// holding plan, when it records the fields its forwarded initializer stores
/// (`lane_record_forwarded_fields`) and its forwarding still reads that initializer
/// (`resolved_forwarded_method`): the receiver that initializer's adds would finish, whose
/// body completes with `undefined`, as the forwarding body then does.
#[inline(always)]
fn lane_forwarded_fields<H: Host>(vm: &mut Vm<H>, callee: Value, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee)?;
    // The plan in place: the allocation borrows the heap beside it, not a copy.
    let plan = &vm.lane_stack.construct_plans[index];
    let (program, function) = plan.forwarded?.initializer?;
    let method = resolved_forwarded_method(vm, callee, plan)?;
    if !matches!(vm.heap.function(method), Some(FunctionRecord { kind: FunctionKind::User(p, f), .. }) if *p == program && *f == function)
    {
        return None;
    }
    let values = plan.values(vm, lane_construct_argument(r, i));
    Some(vm.lane_stack.construct_plans[index].allocate(&mut vm.heap, |field| values[field]))
}

/// Records in `callee`'s holding plan of no fields, whose forwarding resolved
/// (`ConstructPlan::forwarded`), the fields `fields` the forwarded `initializer`'s adds gave
/// `this`, which that plan's receiver started as: later constructions allocate the finished
/// object (`lane_forwarded_fields`). The adds ran no guest code, so the plan's epoch still
/// holds.
#[inline(never)]
fn lane_record_forwarded_fields<H: Host>(
    vm: &mut Vm<H>,
    callee: Value,
    initializer: (ProgramId, u32),
    this: Value,
    fields: &ConstructPlan,
) {
    let Some(index) = holding_construct_plan_index(vm, callee) else {
        return;
    };
    let Some(shape) = vm.object_data(this).map(|object| object.shape()) else {
        return;
    };
    let plan = &mut vm.lane_stack.construct_plans[index];
    let Some(resolution) = plan.forwarded.filter(|_| plan.fields == 0) else {
        return;
    };
    *plan = ConstructPlan {
        shape,
        fields: fields.fields,
        parameters: fields.parameters,
        defaulted: fields.defaulted,
        fallbacks: fields.fallbacks,
        forwarded: Some(ForwardedResolution {
            initializer: Some(initializer),
            ..resolution
        }),
        ..*plan
    }
    .derive_header();
}

/// The method a forwarding constructor's body (`ScriptCallTarget::forwarded_method`) would
/// call on the receiver `this`, which a construction may activate in its place: the body's
/// two reads, `this.<method>` and its `apply`, as cached hits that run no guest code, the
/// intrinsic `Function.prototype.apply`, and a frameless target. `None` leaves the
/// construction to the constructor's own activation, which makes the reads itself.
#[inline(never)]
fn lane_forwarded_construction<H: Host>(
    cx: *mut LaneContext<H>,
    constructor: ScriptCallTarget,
    method: crate::bytecode::ForwardedThisMethod,
    this: Value,
) -> Option<ScriptCallTarget> {
    // SAFETY: the context's program outlives the lane run, and the constructor belongs to it
    // (`Vm::resolve_script_call_target`), so the body's sites index the run's caches.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    // The tiers the lane's own reads of these sites take: a constructor every class shares
    // reads its method through the site's megamorphic entries, and `apply` lives on the
    // method's prototype, `Function.prototype`. Any other read takes the general cached one.
    // A read of `name` or `length` prepares its lookup (`Vm::reads_function_info`), which
    // only the general read does.
    let function = match (!vm.reads_function_info(method.method.0))
        .then(|| vm.polymorphic_field_hit(code, this, method.method.1))
        .flatten()
    {
        Some(function) => function,
        None => lane_cached_field(cx, code, this, method.method.0, method.method.1)?,
    };
    let apply = match (!vm.reads_function_info(method.apply.0))
        .then(|| vm.cell_holder_field_hit(code, function, method.apply.1))
        .flatten()
    {
        Some(apply) => apply,
        None => lane_cached_field(cx, code, function, method.apply.0, method.apply.1)?,
    };
    if !matches!(
        vm.heap.function(apply),
        Some(FunctionRecord {
            kind: FunctionKind::Native(Native::FunctionApply),
            ..
        })
    ) || function == constructor.callee
    {
        return None;
    }
    let forwarded = lane_script_target(cx, function)?;
    if let Some(resolution) = forwarded_resolution(vm, constructor.callee, method, function, apply) {
        let index = construct_plan_index(constructor.callee)?;
        vm.lane_stack.construct_plans[index].forwarded = Some(resolution);
    }
    Some(forwarded)
}

/// Where a forwarding constructor's reads of `this.<method>` and its `apply`, which gave
/// `function` and `apply`, resolved under `callee`'s plan of no fields while it holds
/// (`ForwardedResolution`): own data properties of the plan's prototype and of the method's
/// prototype, neither shadowed by the receiver template or the method, in shapes that fix
/// their slots. `None` records nothing.
fn forwarded_resolution<H: Host>(
    vm: &Vm<H>,
    callee: Value,
    method: crate::bytecode::ForwardedThisMethod,
    function: Value,
    apply: Value,
) -> Option<ForwardedResolution> {
    let plan = holding_construct_plan(vm, callee).filter(|plan| plan.fields == 0)?;
    let data_slot = |owner: &crate::heap::Object, atom| {
        let shape = owner.shape();
        if vm.shape_is_dictionary(shape) {
            return None;
        }
        let slot = vm.shape_slot(shape, atom)?;
        // SAFETY: the slot exists in the non-dictionary shape that names it.
        let value = unsafe { vm.heap.property_get_shaped(owner, slot) }?;
        (!value.is_deleted()).then_some((u16::try_from(slot).ok()?, value))
    };
    let data = |owner: Value, atom, slot: usize| {
        let key = crate::vm::PropertyKey::string(atom);
        vm.property_attributes(owner, key).is_some_and(|attributes| !attributes.accessor)
            && vm.object_data(owner).is_some_and(|object| vm.shape_slot(object.shape(), atom) == Some(slot))
    };
    if vm.shape_slot(plan.shape, method.method.0).is_some() {
        return None;
    }
    let prototype = vm.heap.object_record(plan.prototype)?;
    let (method_slot, found) = data_slot(prototype, method.method.0)?;
    let Some(FunctionRecord { object, .. }) = vm.heap.function(function) else {
        return None;
    };
    if found != function || vm.shape_slot(object.shape(), method.apply.0).is_some() {
        return None;
    }
    let (method_shape, apply_holder) = (object.shape(), object.proto());
    let (apply_slot, found) = data_slot(vm.heap.object(apply_holder)?, method.apply.0)?;
    (found == apply
        && data(plan.prototype, method.method.0, usize::from(method_slot))
        && data(apply_holder, method.apply.0, usize::from(apply_slot)))
    .then_some(ForwardedResolution {
        method_slot,
        method_shape,
        apply_holder,
        apply_slot,
        initializer: None,
    })
}

/// The method a forwarding constructor's construction activates, from `callee`'s plan's
/// recorded resolution (`resolved_forwarded_method`) as a frameless target, under `plan`,
/// `callee`'s holding plan. `None` resolves afresh (`lane_forwarded_construction`).
#[inline(always)]
fn lane_resolved_forwarding<H: Host>(
    cx: *mut LaneContext<H>,
    callee: Value,
    plan: Option<&ConstructPlan>,
) -> Option<ScriptCallTarget> {
    let function = resolved_forwarded_method(unsafe { vm(cx) }, callee, plan?)?;
    lane_script_target(cx, function)
}

/// The method `callee`'s forwarding reads under `plan`, its holding plan, from the plan's
/// recorded resolution (`ConstructPlan::forwarded`): the value in the recorded slot, still of
/// the recorded shape and prototype, whose `apply` is still the intrinsic, and not `callee`.
#[inline(always)]
fn resolved_forwarded_method<H: Host>(vm: &Vm<H>, callee: Value, plan: &ConstructPlan) -> Option<Value> {
    let resolution = plan.forwarded?;
    let prototype = vm.heap.object_record(plan.prototype)?;
    // SAFETY: the plan's proof keeps the prototype's shape, which holds the slot.
    let function = unsafe { vm.heap.property_get_shaped(prototype, usize::from(resolution.method_slot)) }?;
    let Some(FunctionRecord { object, .. }) = vm.heap.function(function) else {
        return None;
    };
    if object.shape() != resolution.method_shape || object.proto() != resolution.apply_holder {
        return None;
    }
    let holder = vm.heap.object(resolution.apply_holder)?;
    // SAFETY: the epoch the plan holds under keeps the holder's shape, which holds the slot.
    let apply = unsafe { vm.heap.property_get_shaped(holder, usize::from(resolution.apply_slot)) }?;
    (matches!(
        vm.heap.function(apply),
        Some(FunctionRecord {
            kind: FunctionKind::Native(Native::FunctionApply),
            ..
        })
    ) && function != callee)
        .then_some(function)
}

script_handler! {
    /// `f.apply(this, arguments)` of the running activation's forwarded `arguments`
    /// (`CallForwardingArguments`): `f` runs frameless with the values the activation, or
    /// the frame, kept, as `Vm::try_inline_call_apply` pushes it. Any other call takes the
    /// general path.
    fn lane_forward_call<>(cx, r, ip, i, view) {
        match lane_forward_activation(cx, r, ip, i) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
            Err(next) => next,
        }
    }
}

/// `lane_forward_call`'s activation: the callee's registers and first record, or the record
/// to continue at. Out of line: the call target it passes on lives in its frame, and a
/// handler whose frame a callee addresses cannot dispatch its successor as a sibling call.
#[inline(never)]
fn lane_forward_activation<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Ip> {
    let window = ImmediateLayout::call_window_base(i.imm());
    if !unsafe { (*cx).script_calls }
        || ImmediateLayout::argument_count(i.imm()) != FORWARDED_APPLY_ARGUMENTS
        || r.get(window + 1) != Value::UNMATERIALIZED_ARGUMENTS
    {
        return Err(exit(ip));
    }
    let vm = unsafe { vm(cx) };
    if !matches!(
        vm.heap.function(r.get(i.b)),
        Some(FunctionRecord {
            kind: FunctionKind::Native(Native::FunctionApply),
            ..
        })
    ) {
        return Err(exit(ip));
    }
    let Some(kept) = (unsafe { lane_kept_arguments(cx) }) else {
        return Err(exit(ip));
    };
    let Some(target) = lane_script_target(cx, r.get(i.c)) else {
        return Err(exit(ip));
    };
    let receiver = r.get(window);
    let Some(this) = lane_call_this(vm, &target, receiver) else {
        return Err(exit(ip));
    };
    lane_activate_keeping::<H, false>(cx, r, ip, &target, kept, this, false, None)
}

use crate::bytecode::FORWARDED_APPLY_ARGUMENTS;

/// The values the running function kept for its forwarded `arguments`: a forwarding
/// activation's, after its registers, or a frame's `original_arguments`.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn lane_kept_arguments<H: Host>(cx: *mut LaneContext<H>) -> Option<ActivationArguments> {
    let vm = unsafe { vm(cx) };
    if unsafe { (*cx).env }.is_deleted() {
        let frame = &vm.frames[unsafe { (*cx).frame }];
        return Some(ActivationArguments::Kept(
            frame.original_arguments.as_ptr(),
            frame.original_arguments.len(),
        ));
    }
    let activation = vm.lane_stack.activations.last()?;
    let script = activation.script().filter(|script| script.forwards)?;
    let registers = usize::from(unsafe { &*(*cx).code }.function(activation.function as usize).registers);
    // SAFETY: the activation's window holds its registers, then its kept values.
    let values = unsafe { vm.lane_stack.values.as_ptr().add(activation.window + registers) };
    Some(ActivationArguments::Kept(values, usize::from(script.argc)))
}

script_handler! {
    /// `LoadLocal` of a forwarded `arguments` binding (`CallPlan::forwards_arguments`): a
    /// frameless activation's is unmaterialized, as a frame's starts; a frame's binding is
    /// read as the general path reads it.
    fn lane_load_forwarded_arguments<>(cx, r, ip, i, view) {
        let value = if unsafe { (*cx).env }.is_deleted() {
            // SAFETY: validated bytecode bounds the slot by Function.locals.
            unsafe { vm(cx).read_validated_local((*cx).frame, i.imm() as usize) }
        } else {
            Value::UNMATERIALIZED_ARGUMENTS
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `LoadLocal` of an `arguments` binding a keeping activation makes at its last read
    /// (`KeptArguments::Passes`): the object made from the values it kept, as its frame
    /// makes one on entry, whose binding a frame's run reads.
    #[inline(never)]
    fn lane_load_kept_arguments_object<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let value = if unsafe { (*cx).env }.is_deleted() {
            // SAFETY: validated bytecode bounds the slot by Function.locals.
            unsafe { vm.read_validated_local((*cx).frame, i.imm() as usize) }
        } else {
            let Some((function, Some(script))) =
                vm.lane_stack.activations.last().map(|activation| (activation.function, activation.script()))
            else {
                return exit(ip);
            };
            // SAFETY: the context's program outlives the lane run.
            let body = unsafe { &*(*cx).code }.function(function as usize);
            // A keeping activation's values follow its registers (`lane_activate_keeping`).
            let kept = (0..script.argc)
                .map(|index| r.get(body.registers + index))
                .collect::<smallvec::SmallVec<[Value; GATHERED_INLINE_VALUES]>>();
            let context = CallContext::user_function(function, script.callee);
            let Ok(arguments) = vm.activation_arguments_object(body, context, &kept) else {
                return exit(ip);
            };
            note_allocation(vm);
            arguments
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The values the running function kept for its unmaterialized `arguments` binding, in the
/// local slot `slot` (`lane_kept_arguments`): `None` where a frame's binding was
/// materialized, which the general path then reads.
///
/// # Safety
/// `cx` is the running lane's context, and `slot` its function's validated local slot.
#[inline(always)]
unsafe fn lane_unmaterialized_arguments<H: Host>(
    cx: *mut LaneContext<H>,
    slot: usize,
) -> Option<(*const Value, usize)> {
    if unsafe { (*cx).env }.is_deleted()
        && unsafe { vm(cx).read_validated_local((*cx).frame, slot) }
            != Value::UNMATERIALIZED_ARGUMENTS
    {
        return None;
    }
    match unsafe { lane_kept_arguments(cx) }? {
        ActivationArguments::Kept(values, count) => Some((values, count)),
        ActivationArguments::Window(_) => None,
    }
}

/// The running keeping activation's kept-value count and its function, when the lane runs
/// one (`ScriptActivation::forwards`): its values follow its registers in its window, which
/// `r` names. `None` for a frame or any other activation.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn lane_keeping_activation<H: Host>(cx: *mut LaneContext<H>) -> Option<(u16, u32)> {
    if unsafe { (*cx).env }.is_deleted() {
        return None;
    }
    let activation = unsafe { vm(cx) }.lane_stack.activations.last()?;
    let script = activation.script().filter(|script| script.forwards)?;
    Some((script.argc, activation.function))
}

script_handler! {
    /// `lane_arguments_length_general` in a keeping activation: the count of the values it
    /// kept, saving no registers.
    fn lane_arguments_length<>(cx, r, ip, i, view) {
        if let Some((argc, _)) = unsafe { lane_keeping_activation(cx) } {
            r.set(i.a, Value::integer(i32::from(argc)));
            next!(cx, r, following(ip), view)
        }
        sibling!(lane_arguments_length_general::<H>, r, ip, view)
    }
}

script_handler! {
    /// `lane_argument_get_general` in a keeping activation at an integer index it kept: the
    /// value after its registers, saving no registers.
    fn lane_argument_get<>(cx, r, ip, i, view) {
        if let Some((argc, function)) = unsafe { lane_keeping_activation(cx) }
            && let Some(index) = r.get(i.b).as_int().and_then(|index| u16::try_from(index).ok()).filter(|index| *index < argc)
        {
            // SAFETY: the context's program outlives the lane run.
            let registers = unsafe { &*(*cx).code }.function(function as usize).registers;
            r.set(i.a, r.get(registers + index));
            next!(cx, r, following(ip), view)
        }
        sibling!(lane_argument_get_general::<H>, r, ip, view)
    }
}

script_handler! {
    /// `ArgumentsLength` of a binding only kept values answer (`Function::kept_arguments`):
    /// their count, as the object made on entry holds it. `lane_arguments_length` answers a
    /// keeping activation first.
    #[inline(never)]
    fn lane_arguments_length_general<>(cx, r, ip, i, view) {
        let Some((_, count)) = (unsafe { lane_unmaterialized_arguments(cx, i.imm() as usize) })
        else {
            return exit(ip);
        };
        r.set(i.a, Value::number(count as f64));
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `ArgumentGet` of such a binding at an integer index it holds: the kept value, as the
    /// object made on entry holds it. Any other key takes the general path, which
    /// materializes the object (`Vm::read_arguments_binding`). `lane_argument_get` answers a
    /// keeping activation first.
    #[inline(never)]
    fn lane_argument_get_general<>(cx, r, ip, i, view) {
        let Some((values, count)) = (unsafe { lane_unmaterialized_arguments(cx, i.imm() as usize) })
        else {
            return exit(ip);
        };
        let Some(index) = r.get(i.b).as_int().and_then(|index| usize::try_from(index).ok()).filter(|index| *index < count)
        else {
            return exit(ip);
        };
        // SAFETY: `values` holds `count` kept values, and the index is below it.
        r.set(i.a, unsafe { *values.add(index) });
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// `ArgumentsLength` (`GET` false) or `ArgumentGet` (`GET` true) of a function whose
    /// `arguments` binding is no forwarding one, in its frame's own lane run, as
    /// `Vm::read_arguments_binding` reads it: an unmaterialized binding answers its length
    /// and the indices it kept from the frame's values; a materialized object its own data
    /// `length` in its creation layout (`Vm::arguments_object_length`) and its stored
    /// elements no parameter mapping reaches (`Vm::plain_arguments_element`). Any other read,
    /// a mapped element (which aliases a parameter) among them, takes the general path.
    #[inline(never)]
    fn lane_frame_arguments<GET: bool>(cx, r, ip, i, view) {
        // An environment activation keeps the values passed after its locals
        // (`Function::reads_passed_arguments_only`); no other activation runs such a function.
        if !unsafe { (*cx).env }.is_deleted() {
            let vm = unsafe { vm(cx) };
            let Some((function, argc)) = vm.lane_stack.activations.last().and_then(|activation| {
                let script = activation.script().filter(|script| script.environment)?;
                Some((activation.function, script.argc))
            }) else {
                return exit(ip);
            };
            // SAFETY: the context's program outlives the lane run.
            let body = unsafe { &*(*cx).code }.function(function as usize);
            let value = if GET {
                let Some(index) = r.get(i.b).as_int().and_then(|index| u16::try_from(index).ok()).filter(|index| *index < argc)
                else {
                    return exit(ip);
                };
                // A parameter's element is the parameter while the mapping aliases it.
                let local = if index < body.params && body.arguments_alias_parameters() {
                    index
                } else {
                    body.locals + index
                };
                r.get(body.registers + WINDOW_LOCALS + local)
            } else {
                Value::integer(i32::from(argc))
            };
            r.set(i.a, value);
            next!(cx, r, following(ip), view)
        }
        let vm = unsafe { vm(cx) };
        let frame = unsafe { (*cx).frame };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Ok(binding) = vm.load_local_binding(code, frame, i.imm() as usize, None) else {
            return exit(ip);
        };
        let kept = &vm.frames[frame].original_arguments;
        let value = match (binding == Value::UNMATERIALIZED_ARGUMENTS, GET) {
            (true, false) => Some(Value::length(kept.len())),
            (true, true) => r
                .get(i.b)
                .as_int()
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| kept.get(index).copied()),
            (false, false) => vm.arguments_object_length(binding),
            (false, true) => r
                .get(i.b)
                .as_int()
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| vm.plain_arguments_element(binding, index)),
        };
        let Some(value) = value else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

/// The frameless target of a lane call to `callee`, resolved on first use.
/// Whether `callee` is the target of a `new` whose `new.target` the general path still
/// holds (`Vm::construct_target`): a lane activation would not bind it.
#[inline(always)]
fn lane_constructing<H: Host>(vm: &Vm<H>, callee: Value) -> bool {
    vm.construct_target.is_some_and(|target| target.callee == callee)
}

#[inline(always)]
fn lane_script_target<H: Host>(cx: *mut LaneContext<H>, callee: Value) -> Option<ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    if lane_constructing(vm, callee) {
        return None;
    }
    // The paths besides a plain call (constructions, getters, forwarded and applied calls)
    // activate callees of the running program alone. Another program's callee, once
    // resolved, is held apart (`cross_call_entry`) for the callers that activate across
    // programs; resolving it again would only find it so.
    // The cached entry is tested in place and copied once.
    let active = vm.active_program;
    let admits = move |target: &ScriptCallTarget| {
        target.program == active && !matches!(target.entry, CallEntry::Frame | CallEntry::Environment | CallEntry::Settles)
    };
    if let Some(entry) = vm.script_call_entry(callee) {
        // SAFETY: the entry is read before anything else touches the cache.
        let target = unsafe { &*entry };
        return admits(target).then(|| *target);
    }
    if vm.cross_call_entry(callee).is_some() {
        return None;
    }
    vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee).filter(admits)
}

/// Make `program` the lane's running program, as a call into or a return to one of its
/// functions makes it the general loop's executing one: the residual, lane views and
/// constants its records read, and the VM's active program, whose cache layout its sites
/// index.
#[inline(always)]
fn lane_enter_program<H: Host>(cx: *mut LaneContext<H>, program: ProgramId) {
    let vm = unsafe { vm(cx) };
    let Some((code, views, constants)) = vm.programs.lane_program(program) else {
        unreachable!("a lane target names a program the store holds");
    };
    let (code, views, constants) = (std::ptr::from_ref(code), views as *const _, std::ptr::from_ref(constants));
    vm.activate_program(program, constants);
    // SAFETY: the context outlives the lane run; the store keeps the program's residual,
    // views and constants for it.
    unsafe {
        (*cx).code = code;
        (*cx).program = program;
        (*cx).views = views;
        (*cx).constants = constants;
    }
}

/// Activate `target`, which keeps no passed values (callers route those to
/// `lane_activate_keeping`), for the call record at `ip` with the arguments from register
/// `arguments` on: the activation's registers and the callee's
/// first record, or the record to continue at when the call takes the general path.
#[inline(always)]
fn lane_activate<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let argc = ImmediateLayout::argument_count(i.imm());
    // A call that returns its callee's completion completes its caller too, which only an
    // activation's return does in the lane. A host permitting proper tail calls
    // sends strict terminal calls to the general dispatch path instead.
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    if returns && !lane_returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    // Values past the parameters stay observable (legacy `f.arguments`, a frame made of the
    // activation): the keeping activation holds them after the registers.
    if argc > target.params {
        return lane_activate_keeping::<H, true>(
            cx,
            r,
            ip,
            target,
            ActivationArguments::Window(arguments),
            this,
            constructs,
            None,
        );
    }
    if !lane_activation_room(vm, target) {
        return Err(lane_script_call_frame::<H>(ip));
    }
    Ok(lane_push_activation::<H, true, false>(cx, r, ip, target, arguments, this, constructs))
}

/// Whether the lane stack takes `target`'s activation: capacity, the guest-call budget and
/// a due collection (`LaneStack::activation_limit`).
#[inline(always)]
fn lane_activation_room<H: Host>(vm: &Vm<H>, target: &ScriptCallTarget) -> bool {
    vm.lane_stack.activations.len() < vm.lane_stack.activation_limit.get()
        && vm.lane_stack.values.capacity() - vm.lane_stack.values.len() >= usize::from(target.room)
}

script_handler! {
    /// The tier prologue a frameless activation of a function the JavaScript tier may claim
    /// starts at (`LaneView::claim_for_tier`): the tier runs the body natively where it can
    /// (`Vm::js_tier_activation_entry`), and the activation continues at the record the
    /// native result completes, with the result in its register; else at its first record.
    /// The tier enters the activation as it would a frame, so claiming a function never
    /// changes how it is called. Once the tier declines the function, its activations start
    /// at the first record again.
    #[inline(never)]
    fn lane_tier_prologue<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (function, params) = (i.imm(), usize::from(i.a));
        // SAFETY: the activation just set the context to its own function's.
        let (program, base) = unsafe { ((*cx).program, (*cx).base) };
        // SAFETY: the activation's registers hold its parameters from its first.
        let parameters = unsafe { std::slice::from_raw_parts(r.0, params) };
        // SAFETY: the program's views outlive the lane run.
        let own = unsafe { &*(*cx).views }.get(function as usize).and_then(super::program_store::derived_lane_view);
        let entered = own
            .and_then(|own| lane_tier_direct(vm, own, program, parameters))
            .or_else(|| lane_tier_enter(cx, function, i.b != 0, own, parameters));
        if let Some((pc, register, value)) = entered {
            r.set(register, value);
            // SAFETY: the tier returns a PC of the function, whose records are contiguous.
            next!(cx, r, unsafe { base.add(pc) }, Carried::script(cx))
        }
        next!(cx, r, base, Carried::script(cx))
    }
}

/// The tier prologue's run of the compiled body `own` keeps for a body that reads only its
/// parameters (`LaneView::tier_direct`): the `Return` to continue at, its register and the
/// result.
#[inline(always)]
fn lane_tier_direct<H: Host>(
    vm: &mut Vm<H>,
    own: &LaneView,
    program: ProgramId,
    parameters: &[Value],
) -> Option<(usize, crate::bytecode::Register, Value)> {
    let direct = own.tier_direct.take()?;
    let entered = vm.run_direct_tier_entry(&direct, program, parameters);
    if !direct.retired() {
        own.tier_direct.set(Some(direct));
    }
    entered
}

/// The tier prologue's entry where no direct run completes the activation
/// (`Vm::js_tier_activation_entry`). A completed entry keeps the direct run the
/// compiled body admits; once the tier declines the function, its activations start at
/// the first record again (`LaneView::release_from_tier`).
#[cold]
#[inline(never)]
fn lane_tier_enter<H: Host>(
    cx: *mut LaneContext<H>,
    function: u32,
    binds_this: bool,
    own: Option<&LaneView>,
    parameters: &[Value],
) -> Option<(usize, crate::bytecode::Register, Value)> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the activation just set the context to its own function's.
    let (code, program, this, env) = unsafe { (&*(*cx).code, (*cx).program, (*cx).this, (*cx).env) };
    let receiver = (binds_this && !this.is_deleted()).then_some(this);
    if let Some(entered) = vm.js_tier_activation_entry(code, program, function, parameters, receiver, env) {
        if let Some(own) = own
            && let Some(direct) = vm.direct_tier_entry(code, program, function)
        {
            own.tier_direct.set(Some(direct));
        }
        return Some(entered);
    }
    // The general baseline runs only from a VM frame (`Vm::js_tier_frame_entry`), so a
    // function the general tier holds gains nothing from frameless claims either.
    if let Some(own) = own
        && (vm.js_tier_holds_for_general(program, function) || !vm.js_tier_may_claim(code, program, function))
    {
        own.release_from_tier();
    }
    None
}

/// Whether activating `target` leaves the running program. Every activation tests it, so an
/// instantiation whose callers route only same-program targets to it (`MAY_CROSS` false)
/// still activates a cross-program target exactly, on its cold branch: a routing mistake
/// costs speed, not the program tables the callee runs with.
#[inline(always)]
fn lane_crosses<H: Host, const MAY_CROSS: bool>(cx: *mut LaneContext<H>, target: &ScriptCallTarget) -> bool {
    let cross = target.program != unsafe { (*cx).program };
    if !MAY_CROSS && cross {
        std::hint::cold_path();
    }
    cross
}

/// `lane_activate` past its checks: the call passes at most `target`'s parameters, the lane
/// stack has room (`lane_activation_room`), and the call may complete its caller. Pushes
/// the activation and returns its registers and the callee's first record; it makes no
/// call, so a handler activating through it keeps its registers. Without `GENERAL` its
/// callers (the fast call forms) have sent another program's target to the general form
/// (`lane_crosses`).
#[inline(always)]
fn lane_push_activation<H: Host, const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // SAFETY: `ip` is a record of the running view, whose window of `r` holds its values.
    let (passed, argc) = unsafe { (r.0.add(usize::from(arguments)).cast_const(), ImmediateLayout::argument_count((*ip).imm())) };
    lane_push_passed::<H, GENERAL, SETTLES>(cx, r, ip, target, passed, argc, this, constructs)
}

/// `lane_push_activation` of the `argc` values at `passed`, at most `target`'s parameters
/// and `FIXED_ARGUMENT_COPIES`, for the call record at `ip`; `SETTLES` for a settling
/// target's call (`Completion::SettlesPromise`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn lane_push_passed<H: Host, const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // Only `lane_script_call_settles` activates a settling target, and it says so statically,
    // so the other call forms decide the completion without reading the target's entry.
    debug_assert_eq!(SETTLES, target.settles(), "a settling target activates through its own call form");
    let vm = unsafe { vm(cx) };
    let count = usize::from(target.registers);
    let stack = &mut vm.lane_stack;
    let window = stack.values.len();
    // SAFETY: capacity holds the window; each slot is written before the length covers
    // it, and no activation's window moves. Parameter `k` lives in register `k` of a
    // plain activation (`Vm::place_plain_parameters`).
    let callee_registers = unsafe {
        let registers = stack.values.as_mut_ptr().add(window);
        fill_window::<SCRIPT_FILL_BLOCK>(registers, count, Value::UNDEFINED);
        copy_arguments(registers, passed, argc);
        stack.values.set_len(window + count);
        Registers(registers)
    };
    // Only a plain call activates another program's callee (`Vm::resolve_script_call_target`).
    debug_assert!(GENERAL || !lane_crosses::<H, true>(cx, target));
    let cross = GENERAL && lane_crosses::<H, true>(cx, target);
    let activation = Activation {
        function: target.function,
        call: ip,
        caller_base: unsafe { (*cx).base },
        caller_registers: r,
        window,
        frame_storage_bytes: 0,
        caller_program: unsafe { (*cx).program },
        kind: ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc,
            caller_this: unsafe { (*cx).this },
            caller_env: unsafe { (*cx).env },
            caller_capture: unsafe { (*cx).capture },
            completion: Completion::of(constructs, cross, SETTLES),
            forwards: false,
            environment: false,
        }),
    };
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
        (*cx).this = this;
        (*cx).env = target.env;
        (*cx).capture = target.capture;
    }
    if cross {
        lane_enter_program(cx, target.program);
    }
    // A construction runs its body from the first record; the tier enters plain calls.
    let start = if constructs { base } else { unsafe { (*target.view).start() } };
    (callee_registers, start)
}

/// The activation of a native's call of `target` with `args` from the frame whose registers
/// are `r` (`Vm::lane_native_callback`), as the lane's own call would make it from the call
/// record `call`: a plain activation when it takes at most its parameters, the values past
/// them dropped where no legacy reflection of a sloppy function can observe them, else a
/// keeping one (`lane_activate_keeping`). Its first record, or `None` where the lane would
/// not activate the call.
fn lane_push_callback<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    call: Ip,
    target: &ScriptCallTarget,
    args: &[Value],
    this: Value,
) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    let params = usize::from(target.params);
    let passed = args.len().min(params);
    let unobserved = args.len() <= params || target.this_binding != ThisBinding::Sloppy;
    if target.entry == CallEntry::Activates
        && unobserved
        && passed <= usize::from(FIXED_ARGUMENT_COPIES)
        && lane_activation_room(vm, target)
    {
        let (_, start) = lane_push_passed::<H, false, false>(cx, r, call, target, args.as_ptr(), passed as u16, this, false);
        return Some(start);
    }
    let arguments = ActivationArguments::Kept(args.as_ptr(), args.len());
    lane_activate_keeping::<H, false>(cx, r, call, target, arguments, this, false, None).ok().map(|(_, start)| start)
}

/// `lane_activate` of an environment target (`CallEntry::Environment`): its window holds
/// its registers, then its environment, made now as the frame's promotion would make it
/// (each captured slot its parameter's argument or `undefined`, every other slot owned
/// elsewhere), then its frame locals (each parameter its argument, a captured slot owned
/// by the environment, every other local `undefined`), then the values passed. The
/// environment is reachable only from the window until the lane leaves, which makes the
/// activation a frame holding it.
#[inline(never)]
fn lane_activate_environment<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    // Another program's callee enters its program and completes across (`Completion::Cross`).
    let cross = target.program != unsafe { (*cx).program };
    let Ok(argc) = u16::try_from(passed.len()) else {
        return Err(exit(ip));
    };
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    if returns && !lane_returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    let Some(code) = vm.programs.residual(target.program) else {
        return Err(exit(ip));
    };
    let body = code.function(target.function as usize);
    let rest = body.rest;
    // A body reading its `arguments` keeps every value passed past its parameters too.
    if argc > target.params && !(rest || body.call_plan(code).reads_passed_arguments) || vm.with_scope_entered {
        return Err(exit(ip));
    }
    // A rest body keeps every value passed, the room sized for its parameters and the rest.
    let extra = usize::from(argc.saturating_sub(target.params));
    if vm.lane_stack.activations.len() >= vm.lane_stack.activation_limit.get()
        || vm.lane_stack.values.capacity() - vm.lane_stack.values.len()
            < usize::from(target.room) + extra
    {
        return Err(lane_script_call_frame::<H>(ip));
    }
    // The rest array, made before the window is: the values past the other parameters,
    // which the rest parameter's local holds (`lane_activate_keeping` gathers likewise).
    let gathered = rest.then(|| {
        let fixed = usize::from(target.params) - 1;
        vm.rest_parameter_array(passed.get(fixed..).unwrap_or_default())
    });
    // SAFETY: the program store keeps a derived view alive with its program, and an
    // environment target's view has a window (`ScriptCallTarget::entry`).
    let layout = unsafe { (*target.view).environment_window.unwrap_unchecked() };
    let count = usize::from(target.registers);
    let locals = usize::from(layout.locals);
    let stack = &mut vm.lane_stack;
    let window = stack.values.len();
    let held = count + usize::from(WINDOW_LOCALS);
    debug_assert!(fill_room::<SCRIPT_FILL_BLOCK>(held + locals) <= usize::from(target.room));
    // SAFETY: capacity holds the window (`ScriptCallTarget::room`); each slot is written
    // before the length covers it, and no activation's window moves.
    let callee_registers = unsafe {
        let registers = stack.values.as_mut_ptr().add(window);
        // Every register and local starts `undefined`, filled in whole blocks within the
        // target's room, which covers them (`ScriptCallTarget::room`); the entries are then
        // written, and parameter `k` local `k` with its argument where one was passed. The
        // environment is made when one is needed.
        fill_window::<SCRIPT_FILL_BLOCK>(registers, held + locals, Value::UNDEFINED);
        let entries = registers.add(count);
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(Value::DELETED);
        entries
            .add(usize::from(WINDOW_CAPTURED))
            .write(std::mem::transmute::<u64, Value>(layout.captured));
        let storage = crate::heap::EnvironmentSlots::lay_out_in_place(entries.add(usize::from(WINDOW_STORAGE)), locals);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
        let local = registers.add(held);
        // The few values a call passes are copied in place: one pass writes each as its
        // parameter's local and, after the locals, as the value a frame keeps for legacy
        // reflection, with no library copy per run.
        let bound = usize::from(argc.min(target.params));
        for (slot, &value) in passed.iter().enumerate() {
            if slot < bound {
                local.add(slot).write(value);
            }
            local.add(locals + slot).write(value);
        }
        if let Some(gathered) = gathered {
            local.add(usize::from(target.params) - 1).write(gathered);
        }
        stack.values.set_len(window + held + locals + usize::from(argc));
        Registers(registers)
    };
    let activation = Activation {
        function: target.function,
        call: ip,
        caller_base: unsafe { (*cx).base },
        caller_registers: r,
        window,
        frame_storage_bytes: 0,
        caller_program: unsafe { (*cx).program },
        kind: ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc,
            caller_this: unsafe { (*cx).this },
            caller_env: unsafe { (*cx).env },
            caller_capture: unsafe { (*cx).capture },
            completion: Completion::of(false, cross, false),
            forwards: false,
            environment: true,
        }),
    };
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
        (*cx).this = this;
        (*cx).env = target.env;
        (*cx).capture = target.capture;
    }
    if cross {
        lane_enter_program(cx, target.program);
    }
    if vm.programs.js_jit_diagnostics_enabled() {
        vm.programs.record_js_jit_lane_entry(target.program, target.function);
    }
    Ok((callee_registers, base))
}

/// The environment of the environment window whose entries start at `entries`
/// (`WINDOW_ENVIRONMENT`): the one already made, or a new environment of `function` of
/// `program` under `parent` that takes over the slots the window held in place, its
/// captured ones from the locals, which then hold `DELETED` for them as a frame's do. The
/// window reaches the new storage from then on. The environment binds the activation's
/// `this`, where its closures' arrows find it (`Vm::captured_lexical_this`), as a frame's
/// promotion carries its bindings; an arrow's own `this` is that same lexical value.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`lane_activate_environment`).
unsafe fn window_environment<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    entries: *mut Value,
    function: u32,
    parent: Value,
    program: ProgramId,
    this: Value,
) -> Value {
    // SAFETY: as the caller guarantees.
    unsafe {
        let made = *entries.add(usize::from(WINDOW_ENVIRONMENT));
        if !made.is_deleted() {
            return made;
        }
        let captured = std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_CAPTURED)));
        let in_place = &*(std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_SLOTS)))
            as *const crate::heap::EnvironmentSlots);
        let locals = entries.add(usize::from(WINDOW_LOCALS));
        let window = EnvironmentWindow { locals: in_place.len() as u16, captured };
        let slots = crate::heap::EnvironmentSlots::from_values((0..in_place.len()).map(|slot| {
            if window.captured(slot) { *locals.add(slot) } else { Value::DELETED }
        }));
        // Each captured local, one per bit of the mask, which names no slot past the locals.
        debug_assert_eq!(captured.checked_shr(u32::from(window.locals)).unwrap_or(0), 0);
        let mut remaining = captured;
        while remaining != 0 {
            locals.add(remaining.trailing_zeros() as usize).write(Value::DELETED);
            remaining &= remaining - 1;
        }
        // A non-arrow binds its `this` where its closures' arrows find it
        // (`Vm::captured_lexical_this`), as a frame does when a closure or the body itself
        // can observe its implicit bindings (`Function::activation_bindings_observable`);
        // any other scope names only its program, which needs no allocation.
        let body = code.function(function as usize);
        let scope = if !body.is_arrow && body.activation_bindings_observable(&code.atoms, &code.functions) {
            crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: Some(program.raw()),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: vec![(vm.runtime_atoms.lexical_this, this)].into(),
                with_objects: Box::default(),
            })
        } else {
            crate::heap::EnvironmentScopeRef::of_program(program.raw())
        };
        let (environment, storage) = vm.heap.alloc_environment(parent, function, slots, scope);
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
        environment
    }
}

/// The body of [[Construct]] for a constructor that only stores its parameters into fields
/// of `this` (`ScriptCallTarget::initializes`): its receiver
/// (`Vm::inline_construct_receiver`), and whether every store was a cached add
/// (`Vm::primary_field_add_hit`), which runs no guest code, so the body completed. After a
/// miss the constructor activates on the receiver: its body repeats the stores made so far
/// as stores of the same values to the own data properties they added, which leaves the
/// object as one activation would. A body whose adds all hit leaves its `ConstructPlan`,
/// with which a later construct allocates the finished object at once
/// (`lane_planned_construction`), which comes first. Out of line, so the construct keeps
/// its tail dispatch.
#[inline(never)]
fn lane_initialize_fields<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    callee: Value,
    function: u32,
    r: Registers,
    i: &LaneInstruction,
) -> Option<(Value, bool)> {
    let argument = lane_construct_argument(r, i);
    let plan_index = construct_plan_index(callee);
    let template = vm.inline_construct_template(callee)?;
    let (prototype, source_shape) = (template.proto(), template.shape());
    let this = vm.heap.alloc_object(template);
    let mut planned = Some(ConstructPlan {
        callee,
        epoch: NO_CONSTRUCT_PLAN_EPOCH,
        callee_layout: (0, 0),
        prototype,
        shape: source_shape,
        ..EMPTY_CONSTRUCT_PLAN
    });
    let added = lane_add_initializer_fields(vm, code, this, function, argument, |store| {
        planned = planned.and_then(|plan| plan.with_field(store));
    });
    if !added {
        return Some((this, false));
    }
    if let Some(mut plan) = planned
        && let Some(index) = plan_index
        && let Some(layout) = vm.function_prototype_layout(callee)
    {
        plan.callee_layout = layout;
        plan.shape = vm.object_data(this).map_or(source_shape, |object| object.shape());
        vm.lane_stack.construct_plans[index] = plan.finish();
    }
    Some((this, true))
}

/// The adds an initializing body (`ScriptCallTarget::initializes`) of `function` makes to
/// `this`, each the parameter its store names (`argument`) as a cached add that runs no guest
/// code, passed to `each` in order. `false` at the first add the caches do not answer: the
/// adds made so far are the body's own first stores of the same values, which an activation
/// of the body then makes again as stores, leaving the object as the activation alone would.
#[inline(always)]
fn lane_add_initializer_fields<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    this: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
    mut each: impl FnMut(&crate::bytecode::InitializerStore),
) -> bool {
    let Some(stores) = code.function(function as usize).initializer_stores(&code.constants) else {
        return false;
    };
    for store in stores {
        let value = initializer_value(vm, argument(store.parameter), store.fallback.map(Value::number));
        if !vm.field_add_atom_eligible(store.atom) || !vm.primary_field_add_hit(code, this, store.atom, value, store.site) {
            return false;
        }
        each(store);
    }
    true
}

/// The value an initializer store (`InitializerStore`) gives its field from the parameter's
/// `passed` value: `fallback` in place of a falsy one where the store has it, else as passed.
#[inline(always)]
fn initializer_value<H: Host>(vm: &Vm<H>, passed: Value, fallback: Option<Value>) -> Value {
    match fallback {
        Some(fallback) if !vm.truthy(passed) => fallback,
        _ => passed,
    }
}

/// `lane_initialize_fields` while `callee`'s plan holds: the finished receiver, its fields
/// the arguments the plan's parameters name. A plan holds while its callee's layout and
/// `prototype` do: the receiver's template is derived from those alone, and the epoch keeps
/// the adds it planned free of setters. Apart from the planning, whose frame is larger.
#[inline(always)]
fn lane_planned_construction<H: Host>(
    vm: &mut Vm<H>,
    callee: Value,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee)?;
    // The plan in place: the allocation borrows the heap beside it, not a copy.
    let plan = &vm.lane_stack.construct_plans[index];
    if plan.kind != ConstructPlanKind::Passed {
        return lane_defaulted_construction(vm, index, r, i);
    }
    let argument = lane_construct_argument(r, i);
    Some(plan.allocate(&mut vm.heap, |field| argument(plan.parameters[field])))
}

/// `lane_planned_construction` under the plan at `index` with defaulted fields, whose values
/// it computes first; `None` for a forwarding constructor's plan, which holds only with its
/// forwarding (`lane_forwarded_fields`). Out of line, so plans of plain fields keep their
/// code.
#[inline(never)]
fn lane_defaulted_construction<H: Host>(vm: &mut Vm<H>, index: usize, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let plan = vm.lane_stack.construct_plans[index];
    if plan.kind == ConstructPlanKind::Forwarding {
        return None;
    }
    let values = plan.values(vm, lane_construct_argument(r, i));
    Some(plan.allocate(&mut vm.heap, |field| values[field]))
}

/// A construction's parameter `k` as a plain activation sees it: argument `k`, or undefined
/// when not passed.
#[inline(always)]
fn lane_construct_argument(r: Registers, i: &LaneInstruction) -> impl Fn(u16) -> Value + '_ {
    let argc = ImmediateLayout::argument_count(i.imm());
    move |parameter| if parameter < argc { r.get(i.c + parameter) } else { Value::UNDEFINED }
}

/// The construct plan cache entry `callee` owns, when it names a heap cell.
fn construct_plan_index(callee: Value) -> Option<usize> {
    callee.heap_index().map(|index| index as usize & (CONSTRUCT_PLANS - 1))
}

/// `callee`'s construct plan while it holds: the entry names `callee`, the prototype
/// layout keeps the plan's epoch and `callee`'s own layout still yields the planned
/// `prototype`, from which the receiver's template is derived.
#[inline(always)]
fn holding_construct_plan<H: Host>(vm: &Vm<H>, callee: Value) -> Option<ConstructPlan> {
    holding_construct_plan_index(vm, callee).map(|index| vm.lane_stack.construct_plans[index])
}

/// The construct plan cache index of `callee`'s plan while it holds (`holding_construct_plan`).
#[inline(always)]
fn holding_construct_plan_index<H: Host>(vm: &Vm<H>, callee: Value) -> Option<usize> {
    let index = construct_plan_index(callee)?;
    let plan = &vm.lane_stack.construct_plans[index];
    (plan.callee == callee
        && plan.epoch == crate::heap::prototype_layout_epoch()
        && vm.function_prototype_in_layout(callee, plan.callee_layout) == Some(plan.prototype))
    .then_some(index)
}

/// The receiver `Vm::inline_construct_receiver` allocates for a constructor that forwards its
/// arguments (`lane_script_construct_keeping`): from `callee`'s holding plan `plan` where it
/// has no fields, else from the template, which leaves that plan. `lane_script_construct` sends a
/// forwarding target here before it tests `ScriptCallTarget::initializes`, so a callee's plan
/// is only ever this one or `lane_initialize_fields`'s.
fn lane_planned_receiver<H: Host>(
    vm: &mut Vm<H>,
    callee: Value,
    plan: Option<&ConstructPlan>,
) -> Option<Value> {
    if let Some(plan) = plan.filter(|plan| plan.fields == 0) {
        return Some(plan.allocate(&mut vm.heap, |_| Value::UNDEFINED));
    }
    let template = vm.inline_construct_template(callee)?;
    let (prototype, shape) = (template.proto(), template.shape());
    let this = vm.heap.alloc_object(template);
    if let Some(index) = construct_plan_index(callee)
        && let Some(callee_layout) = vm.function_prototype_layout(callee)
    {
        vm.lane_stack.construct_plans[index] = ConstructPlan {
            callee,
            callee_layout,
            prototype,
            shape,
            kind: ConstructPlanKind::Forwarding,
            ..EMPTY_CONSTRUCT_PLAN
        }
        .finish();
    }
    Some(this)
}

/// `lane_activate` for a target that keeps its passed values, or for values a forwarding
/// activation or frame kept: the activation's registers and the callee's first record, or
/// the record to continue at when the call takes the general path. Out of line, so the
/// common activation keeps its code. Without `MAY_CROSS` its callers route the running
/// program's targets, as every path but a plain call's makes them (`lane_script_target`);
/// a cross target takes the cold branch of `lane_crosses`.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn lane_activate_keeping<H: Host, const MAY_CROSS: bool>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: ActivationArguments,
    this: Value,
    constructs: bool,
    forwarder: Option<SkippedForwarder>,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    // A plain call passed more values than its parameters activates another program's callee
    // here too (`lane_activate`).
    let cross = lane_crosses::<H, MAY_CROSS>(cx, target);
    debug_assert!(!(cross && forwarder.is_some()));
    let argc = match arguments {
        ActivationArguments::Window(_) => usize::from(ImmediateLayout::argument_count(i.imm())),
        ActivationArguments::Kept(_, count) => count,
    };
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = vm.programs.residual(target.program).and_then(|code| {
        let body = code.function_get(target.function as usize)?;
        let plan = body.call_plan(code);
        Some(format!(
            "keeping {} @{:?} entry={} argc={argc} params={} strict={} legacy={} forwards={} arguments={}",
            body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
            body.source_span.map(|span| span.start), target.entry as u8, target.params, body.strict,
            plan.legacy_reflection, plan.forwards_arguments, body.arguments_slot.is_some(),
        ))
    }) {
        super::lane_exits::record_detail(body);
    }
    let count = usize::from(target.registers);
    // A forwarding activation keeps every value passed after its registers, as does one for
    // a skipped forwarding constructor, whose frame passed them on, and one passed more values
    // than it has parameters.
    let keeps = target.forwards() || forwarder.is_some() || argc > usize::from(target.params);
    let window_size = if keeps { count + argc } else { count };
    // The rest array, made before the window is: the values past the other parameters.
    let rest = (target.entry == CallEntry::Gathers).then(|| {
        let fixed = usize::from(target.params) - 1;
        let passed = (fixed..argc).map(|index| match arguments {
            ActivationArguments::Window(base) => r.get(base + index as u16),
            // SAFETY: kept values hold `argc` values.
            ActivationArguments::Kept(values, _) => unsafe { *values.add(index) },
        });
        vm.rest_parameter_array(&passed.collect::<smallvec::SmallVec<[Value; GATHERED_INLINE_VALUES]>>())
    });
    // A call that returns its callee's completion completes its caller too, which only an
    // activation's return does in the lane. A host permitting proper tail calls
    // sends strict terminal calls to the general dispatch path instead.
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    if returns && !lane_returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    if argc > usize::from(u16::MAX)
        || vm.with_scope_entered
    {
        return Err(exit(ip));
    }
    // Capacity, the guest-call budget and a due collection (`LaneStack::activation_limit`).
    if vm.lane_stack.activations.len() >= vm.lane_stack.activation_limit.get()
        || vm.lane_stack.values.capacity() - vm.lane_stack.values.len()
            < fill_room::<ACTIVATION_FILL_BLOCK>(window_size)
    {
        return Err(lane_script_call_frame::<H>(ip));
    }
    let stack = &mut vm.lane_stack;
    let window = stack.values.len();
    // SAFETY: capacity holds the window; each slot is written before the length covers
    // it, and no activation's window moves. Parameter `k` lives in register `k` of a
    // plain activation (`Vm::place_plain_parameters`).
    let callee_registers = unsafe {
        let registers = stack.values.as_mut_ptr().add(window);
        fill_window::<ACTIVATION_FILL_BLOCK>(registers, count, Value::UNDEFINED);
        match arguments {
            ActivationArguments::Window(base) => {
                for index in 0..argc.min(usize::from(target.params)) {
                    registers.add(index).write(r.get(base + index as u16));
                }
            }
            ActivationArguments::Kept(values, _) => {
                lane_place_kept(registers, values, argc, usize::from(target.params));
            }
        }
        if keeps {
            lane_keep_arguments(registers.add(count), r, arguments, argc);
        }
        if let Some(rest) = rest {
            registers.add(usize::from(target.params) - 1).write(rest);
        }
        stack.values.set_len(window + window_size);
        Registers(registers)
    };
    let activation = Activation {
        function: target.function,
        call: ip,
        caller_base: unsafe { (*cx).base },
        caller_registers: r,
        window,
        frame_storage_bytes: 0,
        caller_program: unsafe { (*cx).program },
        kind: ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc: argc as u16,
            caller_this: unsafe { (*cx).this },
            caller_env: unsafe { (*cx).env },
            caller_capture: unsafe { (*cx).capture },
            completion: if forwarder.is_some() {
                Completion::Forwarder
            } else {
                Completion::of(constructs, cross, false)
            },
            forwards: keeps,
            environment: false,
        }),
    };
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation) };
    if let Some(forwarder) = forwarder {
        stack.forwarders.push(forwarder);
    }
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
        (*cx).this = this;
        (*cx).env = target.env;
        (*cx).capture = target.capture;
    }
    if cross {
        lane_enter_program(cx, target.program);
    }
    if vm.programs.js_jit_diagnostics_enabled() {
        vm.programs.record_js_jit_lane_entry(target.program, target.function);
    }
    let start = if constructs { base } else { unsafe { (*target.view).start() } };
    Ok((callee_registers, start))
}

/// Place kept values in a frameless activation's parameter registers.
///
/// # Safety
/// `registers` holds `params` writable registers and `values` holds `argc` values.
#[cold]
#[inline(never)]
unsafe fn lane_place_kept(registers: *mut Value, values: *const Value, argc: usize, params: usize) {
    for index in 0..argc.min(params) {
        unsafe { registers.add(index).write(*values.add(index)) };
    }
}

/// Keep a forwarding activation's passed values after its registers.
///
/// # Safety
/// `kept` holds `argc` writable slots; `arguments` names `argc` readable values.
#[cold]
#[inline(never)]
unsafe fn lane_keep_arguments(
    kept: *mut Value,
    r: Registers,
    arguments: ActivationArguments,
    argc: usize,
) {
    for index in 0..argc {
        let value = match arguments {
            ActivationArguments::Window(base) => r.get(base + index as u16),
            // SAFETY: kept values lie below the new window, or in a frame, and stay put.
            ActivationArguments::Kept(values, _) => unsafe { *values.add(index) },
        };
        unsafe { kept.add(index).write(value) };
    }
}

/// A frameless call without room on the lane stack: grow the stack, then take the
/// general path, which pushes a frame. Out of line so the activation path saves no
/// registers.
#[cold]
#[inline(never)]
fn lane_script_call_frame<H: Host>(ip: Ip) -> Ip {
    let cx = lane_context::<H>();
    let vm = unsafe { vm(cx) };
    let stack = &vm.lane_stack;
    let values_full = stack.values.capacity() - stack.values.len() < LANE_SCRIPT_WINDOW;
    let activations_full = stack.activations.len() == stack.activations.capacity();
    vm.materialize_lane_activations(cx);
    if values_full {
        let capacity = vm.lane_stack.values.capacity();
        vm.lane_stack.values.reserve((2 * capacity).max(LANE_SCRIPT_WINDOW));
    }
    if activations_full {
        let capacity = vm.lane_stack.activations.capacity();
        vm.lane_stack.activations.reserve((2 * capacity).max(1));
    }
    exit(ip)
}

/// Lane-stack room a frameless call's growth reserves at least: a common window.
const LANE_SCRIPT_WINDOW: usize = 4 * ACTIVATION_FILL_BLOCK;

lane_handler! {
    /// A Wasm call to a function of the same program, pushed as the general
    /// loop's inline call would be.
    fn lane_call_known<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).inline_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        // Keep eligible calls on the general dispatcher so they can enter
        // native code. Unsupported calls stay on the fast interpreter lane.
        if vm.execution_mode == super::ExecutionMode::Compiled
            && i.a & crate::bytecode::RETURN_REGISTER == 0
        {
            let program = unsafe { (*cx).program };
            let function = i.b as usize;
            if vm
                .programs
                .has_compiled_function_or_note_fallback(program, function)
                || vm.programs.has_wasm_tier_containing(program, function)
            {
                return exit(ip);
            }
            // `run_compiled_inner` is the one-shot promotion claimant. Count
            // lane-only entries here, but leave the threshold entry for that
            // general-path hook so no handoff token or duplicate count is
            // needed. Terminal calls stay on their tail-call path above the
            // tier gate and are deliberately not observed here.
            if vm
                .programs
                .note_wasm_tier_lane_entry(program, u32::from(i.b))
            {
                return exit(ip);
            }
        }
        // SAFETY: the program's views outlive the lane run.
        let views = unsafe { &*(*cx).views };
        let stack = &mut vm.lane_stack;
        if let Some(lane_view) = views.get(usize::from(i.b)).and_then(super::program_store::derived_lane_view)
            && let Some(shape) = lane_view.activation
            && usize::from(shape.registers)
                .saturating_mul(std::mem::size_of::<Value>())
                <= crate::stack::GuestCallGuard::remaining_storage_bytes()
                    .saturating_sub(stack.wasm_storage_bytes)
            && stack.activations.len() < stack.activations.capacity()
            && vm.frames.len() + stack.activations.len()
                < super::dispatch_frame::WASM_ACTIVATION_LIMIT
            && stack.values.capacity() - stack.values.len()
                >= fill_room::<ACTIVATION_FILL_BLOCK>(usize::from(shape.registers))
        {
            let window = stack.values.len();
            let count = usize::from(shape.registers);
            let frame_storage_bytes = count.saturating_mul(std::mem::size_of::<Value>());
            let arguments = ImmediateLayout::call_window_base(i.imm());
            let passed = ImmediateLayout::argument_count(i.imm()).min(shape.params);
            // SAFETY: capacity holds the window; each slot is written before
            // the length covers it, and no activation's window moves.
            let callee_registers = unsafe {
                let registers = stack.values.as_mut_ptr().add(window);
                fill_window::<ACTIVATION_FILL_BLOCK>(registers, count, shape.initial);
                let mut offset = 0;
                while offset < passed {
                    registers
                        .add(usize::from(shape.parameters + offset))
                        .write(r.get(arguments + offset));
                    offset += 1;
                }
                stack.values.set_len(window + count);
                Registers(registers)
            };
            // SAFETY: capacity holds the activation.
            unsafe {
                let length = stack.activations.len();
                stack.activations.as_mut_ptr().add(length).write(Activation {
                    function: u32::from(i.b),
                    call: ip,
                    caller_base: (*cx).base,
                    caller_registers: r,
                    window,
                    frame_storage_bytes,
                    caller_program: (*cx).program,
                    kind: ActivationKind::Wasm(view.memory),
                });
                stack.activations.set_len(length + 1);
                stack.wasm_storage_bytes += frame_storage_bytes;
            }
            // A fresh activation's memory register holds no memory until
            // the function's instance binding loads it.
            let base = lane_view.records();
            unsafe {
                (*cx).registers = callee_registers;
                (*cx).base = base;
                (*cx).memory = MemoryView::NONE;
            }
            let view = Carried {
                memory: MemoryView::NONE,
                integers: crate::value::IntegerEncoding::TAG,
                acc: Slot::of(Value::UNDEFINED),
            };
            next!(cx, callee_registers, base, view)
        }
        lane_call_frame::<H>(ip)
    }
}

/// A lane call without room for an activation, or to a function whose lane
/// view is not yet derived: the activations
/// become frames and the call pushes one, as the general loop would; the
/// emptied stack then grows so that later calls fit. Out of line so the
/// activation path saves no registers.
#[cold]
#[inline(never)]
fn lane_call_frame<H: Host>(ip: Ip) -> Ip {
    let cx = lane_context::<H>();
    // SAFETY: called only while a lane runs, from its call handler.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    let code = unsafe { &*(*cx).code };
    let callee = u32::from(i.b);
    let count = usize::from(code.function(usize::from(i.b)).registers);
    let window = fill_room::<ACTIVATION_FILL_BLOCK>(count);
    let stack = &vm.lane_stack;
    let values_full = stack.values.capacity() - stack.values.len() < window;
    let activations_full = stack.activations.len() == stack.activations.capacity();
    vm.materialize_lane_activations(cx);
    // Only a lack of room grows the stack, so its size follows the deepest
    // window and activation count a lane has actually needed.
    if values_full {
        let capacity = vm.lane_stack.values.capacity();
        vm.lane_stack.values.reserve((2 * capacity).max(window));
    }
    if activations_full {
        let capacity = vm.lane_stack.activations.capacity();
        vm.lane_stack.activations.reserve((2 * capacity).max(1));
    }
    let caller = unsafe { (*cx).frame };
    // SAFETY: `ip` is a record of the active view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    let window = crate::bytecode::RegisterWindow {
        base: ImmediateLayout::call_window_base(i.imm()),
        count: ImmediateLayout::argument_count(i.imm()),
    };
    let Ok(call_guard) = vm.push_wasm_frame(code, caller, callee, window) else {
        return exit(ip);
    };
    vm.frames[caller].pc = call_pc + 1;
    vm.frames[caller].pc_is_resume = true;
    unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
        caller,
        caller_cursor: None,
        call_pc: call_pc as u32,
        destination: i.a,
        construct_this: None,
        settles: None,
        returns: false,
        call_guard: Some(call_guard),
        caller_program: None,
    });
    // The pushed frame's guard narrows the activations the thread admits.
    vm.lane_stack.refresh_activation_limit();
    lane_enter_frame(cx, caller + 1, 0)
}

lane_handler! {
    /// Return from an inline call to its caller; returning from the loop's
    /// entry frame belongs to the general loop.
    fn lane_return<>(cx, r, ip, i, view) {
        if let Some(resumed) = lane_complete_plain_activation(cx, r.get(i.a)) {
            resume!(cx, resumed)
        }
        sibling!(lane_return_general::<H>, r, ip, view)
    }
}

lane_handler! {
    /// `LoadConst` of the register the next record, a `Return`, returns
    /// (`specialize_constant_returns`): the constant's return in one record, as `return;` and
    /// `return null;` compile. Any return past the common one writes the register and takes
    /// the `Return` record's own path.
    fn lane_return_constant<>(cx, r, ip, i, view) {
        let value = i.constant();
        if let Some(resumed) = lane_complete_plain_activation(cx, value) {
            resume!(cx, resumed)
        }
        r.set(i.a, value);
        sibling!(lane_return_general::<H>, r, following(ip), view)
    }
}

lane_handler! {
    /// `lane_return` past the common return (`lane_complete_plain_activation`): any other
    /// activation's completion, or the frame's return. Out of line, so `lane_return` keeps its
    /// registers.
    #[inline(never)]
    fn lane_return_general<>(cx, r, ip, i, view) {
        // A run of returning calls (`return f(...)`) first, the commonest return past the
        // plain one, then another program's activation.
        if let Some(resumed) = lane_complete_plain_chain(cx, r.get(i.a)) {
            resume!(cx, resumed)
        }
        if let Some(resumed) = lane_complete_cross_activation(cx, r.get(i.a)) {
            resume!(cx, resumed)
        }
        if let Some(resumed) = lane_complete_activation(cx, ip, r.get(i.a)) {
            resume!(cx, resumed)
        }
        lane_return_frame::<H>(r, ip)
    }
}

/// `lane_complete_activation`'s commonest completion: a JavaScript activation of the
/// running program whose call keeps its result (`Completion::Plain`, a result register). It
/// restores what that completion restores, without moving the activation out of the stack:
/// the caller's registers, records, receiver and environments, and the call's result. `None`
/// leaves every other completion to the returns past it, which take another program's
/// activation the same way (`lane_complete_cross_activation`).
#[inline(always)]
fn lane_return_plain<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    lane_return_from::<H, false>(cx, value)
}

/// `lane_complete_plain_activation` of an activation of another program than its caller's
/// (`Completion::Cross`), which enters the caller's program first, for the returns past the
/// common one (`lane_return_general`, `lane_return_value_general`), so the common return
/// carries none of the program switch.
#[inline(always)]
fn lane_complete_cross_activation<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let (registers, next) = lane_return_from::<H, true>(cx, value)?;
    Some(Resumed::At(registers, next, Carried::script(cx)))
}

/// `lane_return_plain` of the innermost activation, a script one whose call keeps its
/// result: of the caller's program (`Completion::Plain`), or, where `CROSSES`, of another
/// (`Completion::Cross`), which enters the caller's program first.
#[inline(always)]
fn lane_return_from<H: Host, const CROSSES: bool>(cx: *mut LaneContext<H>, value: Value) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let stack = &mut vm.lane_stack;
    let top = stack.activations.last()?;
    let ActivationKind::Script(script) = &top.kind else {
        return None;
    };
    if script.completion != if CROSSES { Completion::Cross } else { Completion::Plain } {
        return None;
    }
    // SAFETY: the call record belongs to the caller's live view.
    let destination = unsafe { (*top.call).a };
    if destination & crate::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let (call, caller_registers, caller_base, window) = (top.call, top.caller_registers, top.caller_base, top.window);
    let (this, env, capture) = (script.caller_this, script.caller_env, script.caller_capture);
    let caller_program = top.caller_program;
    // SAFETY: a script activation holds no drop glue and no Wasm storage
    // (`Activation::frame_storage_bytes`); its window starts within the values.
    unsafe {
        let len = stack.activations.len();
        stack.activations.set_len(len - 1);
        stack.values.set_len(window);
    }
    // The caller's memory view reads its program's records (`MemoryView::script`).
    if CROSSES {
        lane_enter_program(cx, caller_program);
    }
    unsafe {
        (*cx).registers = caller_registers;
        (*cx).base = caller_base;
        (*cx).memory = MemoryView::script(cx);
        (*cx).this = this;
        (*cx).env = env;
        (*cx).capture = capture;
    }
    caller_registers.set(destination, value);
    Some((caller_registers, following(call)))
}

/// Whether a call record that returns its callee's completion runs in the lane: from an
/// activation, which completes in turn, or from the frame, which the general loop then
/// completes with the value (`lane_frame_return`). A host permitting proper tail calls sends
/// a strict caller's terminal calls to the general dispatch path instead.
#[inline(always)]
fn lane_returning_call_admitted<H: Host>(cx: *mut LaneContext<H>) -> bool {
    let vm = unsafe { vm(cx) };
    if !vm.host.proper_tail_calls() {
        return true;
    }
    let function = match vm.lane_stack.activations.last() {
        Some(caller) => caller.function,
        None => vm.frames[unsafe { (*cx).frame }].function,
    };
    !unsafe { &*(*cx).code }.function(function as usize).strict
}

/// The frame's own call record at `call` returns `value`, its callee's completion: the
/// general loop returns it from the frame at that instruction (`Vm::lane_returned`).
#[inline(always)]
fn lane_frame_return<H: Host>(vm: &mut Vm<H>, call: Ip, value: Value) -> Ip {
    vm.lane_returned = Some(value);
    exit(call)
}

/// Complete the innermost lane activation with `value` when it is the common return
/// (`lane_return_plain`): a JavaScript activation of the running program's caller yielding
/// `value` to its call's result register. Each step loads or stores, so a handler returning
/// through it saves no registers. `None`, which changes nothing, leaves every other completion
/// to `lane_complete_activation`.
#[inline(always)]
fn lane_complete_plain_activation<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let (registers, next) = lane_return_plain(cx, value)?;
    let view = Carried {
        memory: MemoryView::script(cx),
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    Some(Resumed::At(registers, next, view))
}

/// `lane_complete_activation` of a run of JavaScript activations of one program each
/// returning the completion of the call above it (`return f(...)`), down to one whose call
/// takes `value` in its result register. Only that outermost activation's caller state is
/// restored: an inner one's would be overwritten by it. `None`, which changes nothing, leaves
/// any other completion to the activation-by-activation loop.
#[inline(always)]
fn lane_complete_plain_chain<H: Host>(cx: *mut LaneContext<H>, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let activations = &vm.lane_stack.activations;
    let mut index = activations.len();
    let outermost = loop {
        index = index.checked_sub(1)?;
        let activation = &activations[index];
        let ActivationKind::Script(script) = &activation.kind else {
            return None;
        };
        if script.completion != Completion::Plain {
            return None;
        }
        // SAFETY: the call record belongs to the caller's live view.
        if unsafe { (*activation.call).a } & crate::bytecode::RETURN_REGISTER == 0 {
            break index;
        }
    };
    // Script activations hold no Wasm storage (`LaneStack::pop_activation`), so the inner
    // ones go without its accounting.
    debug_assert!(activations[outermost..].iter().all(|activation| activation.frame_storage_bytes == 0));
    vm.lane_stack.activations.truncate(outermost + 1);
    vm.lane_stack.drop_iterators_from(outermost + 1);
    lane_complete_plain_activation(cx, value)
}

/// Restore the lane state `activation`'s caller runs with, as its return does: the lane
/// stack below its window, the caller's registers, records and memory, and a JavaScript
/// caller's receiver, environment and captures. Returns the caller's memory and the
/// returning activation's receiver, which the context held while it ran.
#[inline(always)]
fn lane_restore_caller<H: Host>(cx: *mut LaneContext<H>, activation: &Activation) -> (MemoryView, Value) {
    let vm = unsafe { vm(cx) };
    // SAFETY: an activation's window starts within the values, which hold no drop glue.
    unsafe { vm.lane_stack.values.set_len(activation.window) };
    let caller_memory = activation.caller_memory(cx);
    let this = unsafe { (*cx).this };
    unsafe {
        (*cx).registers = activation.caller_registers;
        (*cx).base = activation.caller_base;
        (*cx).memory = caller_memory;
    }
    if let ActivationKind::Script(script) = &activation.kind {
        unsafe {
            (*cx).this = script.caller_this;
            (*cx).env = script.caller_env;
            (*cx).capture = script.caller_capture;
        }
    }
    (caller_memory, this)
}

/// The caller of `activation`, restored with `memory` (`lane_restore_caller`), resumes
/// after its call with `value` in the call's result register.
#[inline(always)]
fn lane_resume_after_call(activation: &Activation, memory: MemoryView, value: Value) -> Resumed {
    let view = Carried {
        memory,
        integers: crate::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    // SAFETY: the call record belongs to the caller's live view.
    activation.caller_registers.set(unsafe { (*activation.call).a }, value);
    Resumed::At(activation.caller_registers, following(activation.call), view)
}

/// Complete the innermost lane activation with `value`, which the record at `ip` of that
/// activation returns, as a return from it does: its caller resumes after the call with the
/// value in the call's result register, or, for a call that returns its callee's
/// completion, completes in turn. `None` when no activation runs: the running function is a
/// VM frame.
#[inline(always)]
fn lane_complete_activation<H: Host>(cx: *mut LaneContext<H>, ip: Ip, mut value: Value) -> Option<Resumed> {
    if let Some(resumed) = lane_complete_plain_chain(cx, value) {
        return Some(resumed);
    }
    let vm = unsafe { vm(cx) };
    // A settling activation whose promise's resolution would observe the value returns it
    // from its frame, whose pending call resolves the promise (`lane_settled_then`); else the
    // `then` read here is the one its resolution uses.
    let mut then = Value::UNDEFINED;
    if settling(vm.lane_stack.activations.last()?) {
        let Some(read) = lane_settled_then(vm, value) else {
            return Some(Resumed::Exit(lane_frame_return(vm, ip, value)));
        };
        then = read;
    }
    let mut activation = vm.lane_stack.pop_activation()?;
    loop {
        let (caller_memory, this) = lane_restore_caller(cx, &activation);
        if let ActivationKind::Script(script) = &activation.kind {
            // A native's callback ends its run, whatever the native's own call record is.
            if script.completion == Completion::ReturnsToNative {
                vm.lane_native_returned = Some(value);
                return Some(Resumed::Exit(exit(activation.call)));
            }
            // [[Construct]] of an ordinary constructor yields its returned object,
            // otherwise the receiver it allocated. A skipped forwarding constructor returns
            // `undefined` after the method, so its construction yields the receiver.
            if script.completion.crosses() {
                lane_enter_program(cx, activation.caller_program);
            }
            if script.completion.constructs() && !vm.is_object_like(value) {
                value = this;
            }
            if script.completion == Completion::Forwarder {
                value = this;
                vm.lane_stack.forwarders.pop();
            }
            if script.completion == Completion::SettlesPromise {
                value = lane_settle_promise(cx, value, then);
            }
        }
        // SAFETY: the call record belongs to the caller's live view.
        if unsafe { (*activation.call).a } & crate::bytecode::RETURN_REGISTER == 0 {
            return Some(lane_resume_after_call(&activation, caller_memory, value));
        }
        // The call returns its callee's completion: its caller completes in turn, an
        // activation here or the frame in the general loop, which the exit hands it to.
        let in_frame = match vm.lane_stack.activations.last() {
            None => true,
            Some(caller) if settling(caller) => match lane_settled_then(vm, value) {
                Some(read) => {
                    then = read;
                    false
                }
                None => true,
            },
            Some(_) => false,
        };
        if in_frame {
            return Some(Resumed::Exit(lane_frame_return(vm, activation.call, value)));
        }
        activation = vm.lane_stack.pop_activation().expect("the caller activation is the innermost");
    }
}

/// Whether `activation` settles its call's promise (`Completion::SettlesPromise`).
#[inline(always)]
fn settling(activation: &Activation) -> bool {
    matches!(&activation.kind, ActivationKind::Script(script) if script.completion == Completion::SettlesPromise)
}

/// The `then` a settling activation completing with `value` resolves its promise with
/// (`Vm::plain_resolution_then`), read once; `None` where it settles the promise in its frame
/// instead: its resolution would read the value's `then` impurely or run a lifecycle hook,
/// either of which may run guest code. The frame's pending call resolves the promise as
/// `Vm::promise_resolve_value` does.
#[inline(always)]
fn lane_settled_then<H: Host>(vm: &mut Vm<H>, value: Value) -> Option<Value> {
    if vm.host.observes_promise_lifecycle() {
        return None;
    }
    vm.plain_resolution_then(value)
}

/// The completion of a settling activation (`Completion::SettlesPromise`) whose caller the
/// lane restored, with `value` and the `then` `lane_settled_then` read from it: its promise,
/// the innermost of `LaneStack::settling`, resolved with the value, which its call yields.
#[inline(never)]
fn lane_settle_promise<H: Host>(cx: *mut LaneContext<H>, value: Value, then: Value) -> Value {
    let vm = unsafe { vm(cx) };
    let root = vm.lane_stack.settling.pop().expect("each settling activation roots its promise");
    let promise = vm.release_settled_call_root(root);
    // SAFETY: the context's program, the caller's, outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let settled = vm.resolve_fresh_promise_plainly(code, promise, value, then);
    debug_assert!(settled, "a settling call's promise is a promise record");
    promise
}

/// The value-producing records that may return their result from the frame
/// (`lane_returns`), as `lane_returning` parameters.
const RETURNING_FIELD: u8 = 0;
const RETURNING_UNARY: u8 = 1;
const RETURNING_BINARY: u8 = 2;
const RETURNING_OBJECT: u8 = 3;
const RETURNING_LITERAL: u8 = 4;
/// A `RETURNING_FIELD` read of a register or `this` at a site that reads neither `name` nor
/// `length`: the site's primary own entry (`Vm::primary_own_field_hit`), installed by
/// `lane_returning_field_first`.
const RETURNING_FIELD_OWN: u8 = 5;
/// `RETURNING_FIELD_OWN` at a site past one shape (`Vm::polymorphic_field_hit`).
const RETURNING_FIELD_POLYMORPHIC: u8 = 6;
/// `RETURNING_FIELD_OWN` at a site whose primary entry names a prototype holder
/// (`Vm::primary_holder_field_hit`).
const RETURNING_FIELD_HOLDER: u8 = 7;
/// `RETURNING_FIELD_OWN` at a site with an inherited entry (`Vm::inherited_field_hit`).
const RETURNING_FIELD_INHERITED: u8 = 8;
/// `RETURNING_FIELD` of a nested site whose steps hit primary own entries
/// (`lane_nested_own_value`).
const RETURNING_FIELD_NESTED: u8 = 9;
/// A returning `MakeObjectLiteral` (`return { ... }`): the literal `lane_make_object_literal`
/// makes, returned from the frame.
const RETURNING_OBJECT_LITERAL: u8 = 10;

/// The cached read of a single-base returning `GetField` of `KIND` (`RETURNING_FIELD_OWN`,
/// `_POLYMORPHIC`, `_HOLDER`, `_INHERITED`) at `cache`: a hit, which runs no guest code.
#[inline(always)]
fn returning_field_hit<H: Host, const KIND: u8>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    object: Value,
    cache: u16,
) -> Option<Value> {
    match KIND {
        RETURNING_FIELD_OWN => vm.primary_own_field_hit(code, object, cache),
        RETURNING_FIELD_POLYMORPHIC => vm.inline_polymorphic_field_hit(code, object, cache),
        RETURNING_FIELD_HOLDER => vm.primary_holder_field_hit(code, object, cache),
        _ => vm.inherited_field_hit(code, object, cache),
    }
}

/// The value a frameless read target (`CallEntry::reads`, `CallEntry::ForwardsRead`)
/// returns for a call passing `arguments.get(k)` as parameter `k` (`None` where not passed) on
/// `this`, where the reads its records have quickened to hit. Answering it at the call
/// leaves nothing for an activation to observe: each hit runs no guest code and allocates
/// nothing. `None` for any other record state or a miss, which the call activates to run.
///
/// # Safety
/// `cx` is the running lane's context and `target` a target of its program.
#[inline(always)]
unsafe fn lane_call_read<H: Host>(
    cx: *mut LaneContext<H>,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    // A forwarding body's answer is its method's leaf read on the field it reads, so both
    // kinds reach one leaf read, which the call handler then holds once.
    let (target, arguments, this) = if target.entry == CallEntry::ForwardsRead {
        // SAFETY: as for `ForwardRead::of`.
        let (callee, receiver) = unsafe { ForwardRead::of::<H>(target) }?.method(vm, code, arguments, this)?;
        // SAFETY: as `Vm::script_call_entry`; a read resolves no target and collects nothing.
        let inner = unsafe { &*vm.script_call_entry(callee)? };
        if !inner.entry.reads() {
            return None;
        }
        let inner_this = lane_direct_call_this(vm, inner, receiver)?;
        // The call passes the parameters in order, so its argument `k` is parameter `k`.
        (inner, arguments.within(target.params), inner_this)
    } else {
        (target, arguments, this)
    };
    match target.entry {
        // The receiver the call binds (`lane_direct_call_this`), which is what it returns.
        CallEntry::ReadsThis => Some(this),
        // SAFETY: a capture-read target of the running program.
        CallEntry::ReadsCapture => unsafe { lane_read_capture(vm, target) },
        // SAFETY: as for `LeafRead::of`.
        _ => unsafe { LeafRead::of::<H>(target) }?.run(vm, code, arguments, this),
    }
}

/// The capture a `CallEntry::ReadsCapture` target's closure environment holds, read as its
/// frameless activation's `LoadCapture` would (`lane_load_capture_walk`); an uninitialized
/// binding misses.
///
/// # Safety
/// `target` is a `CallEntry::ReadsCapture` target of the running program, whose view holds
/// its `LoadCapture` record first.
#[inline(never)]
unsafe fn lane_read_capture<H: Host>(vm: &mut Vm<H>, target: &ScriptCallTarget) -> Option<Value> {
    // SAFETY: the program store keeps a derived view alive with its program.
    let imm = unsafe { &*(*target.view).records() }.imm();
    let (depth, slot) = (ImmediateLayout::capture_depth(imm), ImmediateLayout::capture_slot(imm));
    let slots = capture_reads(target.capture);
    if depth == 0 && !slots.is_null() {
        // SAFETY: as `ScriptCallTarget::capture`.
        return unsafe { &*slots }.dense_plain(usize::from(slot));
    }
    let (slots, root) = vm.walked_capture_slots(target.env, false, depth)?;
    // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
    vm.owned_capture(unsafe { &*slots }, root, slot)
}

/// The values a call that a read answers passed (`lane_call_read`): parameter `k` is the
/// call window's register `base + k` while `k < passed`, else unpassed. A concrete value,
/// not a closure, so the reads it feeds are one copy whatever forwarded them.
/// `lane_call_read` of the call at `site`, memoized when it answers (`lane_note_read`). Out
/// of line: the read's calls take stack temporaries, which a handler inlining them would hold
/// across its dispatch, which then could not be a sibling call.
///
/// # Safety
/// As `lane_call_read`.
#[inline(never)]
unsafe fn lane_answered_read<H: Host>(
    cx: *mut LaneContext<H>,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the caller's contract.
    let value = unsafe { lane_call_read(cx, target, arguments, this) };
    if value.is_some() {
        lane_note_read(cx, site, target, arguments, this);
    }
    value
}

#[derive(Clone, Copy)]
struct ReadArguments {
    registers: Registers,
    base: u16,
    passed: u16,
}

impl ReadArguments {
    /// A call that passes nothing, as `[[Get]]` calls a getter: `get` reads no register.
    const NONE: Self = Self { registers: Registers(std::ptr::null_mut()), base: 0, passed: 0 };

    #[inline(always)]
    fn get(self, parameter: u16) -> Option<Value> {
        (parameter < self.passed).then(|| self.registers.get(self.base + parameter))
    }

    /// The arguments a body of `params` parameters passes on in order.
    #[inline(always)]
    fn within(self, params: u16) -> Self {
        Self { passed: self.passed.min(params), ..self }
    }
}

/// The hit a field record of a read body answers with: its quickened primary entry, own or
/// held by a record prototype (`lane_get_field_own`, `lane_get_field_holder`). Each hit is
/// its cache's own validated read, whatever the record's state when it runs.
#[derive(Clone, Copy)]
enum FieldHit {
    Own(u16),
    Holder(u16),
}

impl FieldHit {
    /// The hit `record`'s quickened handler names; `None` for any other record state.
    #[inline(always)]
    fn of<H: Host>(record: &LaneInstruction) -> Option<Self> {
        let handler = record.handler;
        if handler == lane_get_field_own::<H, true> as *const ()
            || handler == lane_get_field_own::<H, false> as *const ()
        {
            Some(Self::Own(record.c))
        } else if handler == lane_get_field_holder::<H, true> as *const ()
            || handler == lane_get_field_holder::<H, false> as *const ()
        {
            Some(Self::Holder(record.c))
        } else {
            None
        }
    }

    /// The hit on `object`; `None` for a miss.
    #[inline(always)]
    fn run<H: Host>(self, vm: &mut Vm<H>, code: &ResidualProgram, object: Value) -> Option<Value> {
        match self {
            Self::Own(cache) => vm.primary_own_field_hit(code, object, cache),
            Self::Holder(cache) => vm.primary_record_holder_field_hit(code, object, cache),
        }
    }
}

/// The read a `CallEntry::reads` body answers with, by the state its records quickened to.
#[derive(Clone, Copy)]
enum LeafRead {
    /// Its returning `GetField` (`lane_returning` of `kind`) on the object `base` names: the
    /// kind's hit (`returning_field_hit`), or for a holder a record prototype's method read
    /// with no call.
    Field { base: u16, kind: u8, cache: u16 },
    /// Its nested own read (`nested_own_field_hit`) of field site `site`.
    Nested { site: u32 },
    /// Its field record's hit, then the dense element the parameter `index` names.
    Element { base: u16, field: FieldHit, index: u16 },
}

impl LeafRead {
    /// The read `target`'s records answer with now; `None` for a record state no read hits.
    ///
    /// # Safety
    /// `target` is a `CallEntry::reads` target of the running program, whose view holds its
    /// returning record first and, for an element read, its `GetIndex` record after it.
    #[inline(always)]
    unsafe fn of<H: Host>(target: &ScriptCallTarget) -> Option<Self> {
        // SAFETY: the program store keeps a derived view alive with its program.
        let entry = unsafe { &*(*target.view).records() };
        if target.entry == CallEntry::ReadsElement {
            // SAFETY: as the function's contract.
            let element = unsafe { &*(*target.view).records().add(1) };
            return Some(Self::Element { base: entry.b, field: FieldHit::of::<H>(entry)?, index: element.c });
        }
        let handler = entry.handler;
        if handler == lane_returning::<H, RETURNING_FIELD_NESTED> as *const () {
            return Some(Self::Nested { site: entry.imm() });
        }
        let kind = if handler == lane_returning::<H, RETURNING_FIELD_POLYMORPHIC> as *const () {
            RETURNING_FIELD_POLYMORPHIC
        } else if handler == lane_returning::<H, RETURNING_FIELD_OWN> as *const () {
            RETURNING_FIELD_OWN
        } else if handler == lane_returning::<H, RETURNING_FIELD_HOLDER> as *const () {
            RETURNING_FIELD_HOLDER
        } else if handler == lane_returning::<H, RETURNING_FIELD_INHERITED> as *const () {
            RETURNING_FIELD_INHERITED
        } else {
            return None;
        };
        Some(Self::Field { base: entry.b, kind, cache: entry.c })
    }

    /// The read's value for a call passing `arguments` on `this`; `None` for a miss.
    #[inline(always)]
    fn run<H: Host>(
        self,
        vm: &mut Vm<H>,
        code: &ResidualProgram,
        arguments: ReadArguments,
        this: Value,
    ) -> Option<Value> {
        let base = |base: u16| lane_read_base(crate::bytecode::FieldBase(base), arguments, this);
        match self {
            Self::Element { base: field_base, field, index } => {
                let array = field.run(vm, code, base(field_base)?)?;
                vm.direct_dense_element(array, arguments.get(index)?)
            }
            Self::Nested { site } => {
                // SAFETY: a nested record of the running program names its site.
                let site = unsafe { code.field_sites.get_unchecked(site as usize) };
                nested_own_field_hit(vm, code, site, lane_read_base(site.base, arguments, this)?)
            }
            Self::Field { base: field_base, kind, cache } => {
                let object = base(field_base)?;
                match kind {
                    RETURNING_FIELD_POLYMORPHIC => {
                        returning_field_hit::<H, RETURNING_FIELD_POLYMORPHIC>(vm, code, object, cache)
                    }
                    RETURNING_FIELD_OWN => returning_field_hit::<H, RETURNING_FIELD_OWN>(vm, code, object, cache),
                    // A record prototype's method, read with no call; any other declines the
                    // read, and the activation's own record reads it.
                    RETURNING_FIELD_HOLDER => vm.primary_record_holder_field_hit(code, object, cache),
                    _ => returning_field_hit::<H, RETURNING_FIELD_INHERITED>(vm, code, object, cache),
                }
            }
        }
    }
}

/// A `CallEntry::ForwardsRead` body's reads: the field of `this` or a parameter it reads,
/// then the method of that field's value it calls.
#[derive(Clone, Copy)]
struct ForwardRead {
    base: u16,
    field: FieldHit,
    method: FieldHit,
}

impl ForwardRead {
    /// The reads `target`'s records answer with now; `None` for a record state no read hits.
    ///
    /// # Safety
    /// `target` is a `CallEntry::ForwardsRead` target of the running program, whose view
    /// holds its method read after its field read.
    #[inline(always)]
    unsafe fn of<H: Host>(target: &ScriptCallTarget) -> Option<Self> {
        // SAFETY: the program store keeps a derived view alive with its program.
        let (field, method) = unsafe {
            let records = (*target.view).records();
            (&*records, &*records.add(1))
        };
        Some(Self { base: field.b, field: FieldHit::of::<H>(field)?, method: FieldHit::of::<H>(method)? })
    }

    /// The method the body calls and its receiver, for a call passing `arguments` on `this`.
    #[inline(always)]
    fn method<H: Host>(
        self,
        vm: &mut Vm<H>,
        code: &ResidualProgram,
        arguments: ReadArguments,
        this: Value,
    ) -> Option<(Value, Value)> {
        let base = lane_read_base(crate::bytecode::FieldBase(self.base), arguments, this)?;
        let receiver = self.field.run(vm, code, base)?;
        Some((self.method.run(vm, code, receiver)?, receiver))
    }
}

/// A read a call site's memo makes (`MemoRead`): its field steps as copies of the field
/// cache entries that answered them (`Vm::record_field_entry_hit`), so a step is one shape
/// test. A read of a polymorphic or inherited entry, which probes further, has no memo.
#[derive(Clone, Copy)]
enum MemoLeaf {
    /// The field of the object `base` names (`lane_read_base`).
    Field { base: u16, field: super::FieldCache },
    /// That field's array, then its dense element the parameter `index` names.
    Element { base: u16, field: super::FieldCache, index: u16 },
    /// That field's array, then its `length` (`Vm::array_length_field`).
    Length { base: u16, field: super::FieldCache },
    /// That field's object, then its own field `second`.
    Nested { base: u16, field: super::FieldCache, second: super::FieldCache },
}

impl MemoLeaf {
    /// The memo form of `read`, whose hits the active program's caches just answered; `None`
    /// for a polymorphic or inherited read.
    fn of<H: Host>(vm: &Vm<H>, code: &ResidualProgram, read: LeafRead) -> Option<Self> {
        let entry = |hit: FieldHit| match hit {
            FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
        };
        Some(match read {
            LeafRead::Field { base, kind: RETURNING_FIELD_OWN | RETURNING_FIELD_HOLDER, cache } => {
                Self::Field { base, field: vm.field_cache_entry(cache) }
            }
            LeafRead::Field { .. } => return None,
            LeafRead::Element { base, field, index } => Self::Element { base, field: entry(field), index },
            LeafRead::Nested { site } => {
                // SAFETY: a nested record of the running program names its site.
                let site = unsafe { code.field_sites.get_unchecked(site as usize) };
                let (base, field) = (site.base.0, vm.field_cache_entry(site.first.1));
                match site.second {
                    None => Self::Field { base, field },
                    Some((atom, _)) if atom == vm.length_atom => Self::Length { base, field },
                    Some((_, second)) => Self::Nested { base, field, second: vm.field_cache_entry(second) },
                }
            }
        })
    }

    /// `LeafRead::run` of the read this memo records.
    #[inline(always)]
    fn run<H: Host>(&self, vm: &Vm<H>, arguments: ReadArguments, this: Value) -> Option<Value> {
        let base = |base: u16| lane_read_base(crate::bytecode::FieldBase(base), arguments, this);
        match *self {
            Self::Field { base: field_base, field } => vm.record_field_entry_hit(field, base(field_base)?),
            Self::Element { base: field_base, field, index } => {
                let array = vm.record_field_entry_hit(field, base(field_base)?)?;
                vm.direct_dense_element(array, arguments.get(index)?)
            }
            Self::Length { base: field_base, field } => {
                let array = vm.record_field_entry_hit(field, base(field_base)?)?;
                vm.array_length_field(array, vm.length_atom)
            }
            Self::Nested { base: field_base, field, second } => {
                let object = vm.record_field_entry_hit(field, base(field_base)?)?;
                vm.record_field_entry_hit(second, object)
            }
        }
    }
}

/// What a call site's read target answered with (`lane_script_call_memo`), so the site's
/// next call of the same callee makes the hits alone: the target's read, or a forwarding
/// body's field and method reads and the read of the method they found. Every part depends
/// on the callee alone, whose body is fixed, and on the shapes its steps name.
#[derive(Clone, Copy)]
enum MemoRead {
    Leaf(MemoLeaf),
    Forward {
        /// The forwarding body's field read: the object `base` names, then its method.
        base: u16,
        field: super::FieldCache,
        method_field: super::FieldCache,
        /// The forwarding body's parameters, which it passes on in order.
        params: u16,
        /// The method the reads found, a `CallEntry::reads` target, and its binding.
        method: Value,
        method_this: ThisBinding,
        read: MemoLeaf,
    },
}

/// A call site's memo of its read (`MemoRead`), valid for `callee` in the generation and
/// program it was made in, as `ScriptCallTarget` is.
#[derive(Clone, Copy)]
struct ReadMemo {
    /// The call record that made it, which owns its slot (`read_memo_index`) while it holds.
    site: Ip,
    callee: Value,
    generation: ScriptGeneration,
    program: ProgramId,
    this_binding: ThisBinding,
    read: MemoRead,
}

/// Slots of the read memo (`LaneStack::read_memos`); a site whose slot another live site's
/// memo holds keeps the full call form.
const READ_MEMOS: usize = 64;
const _: () = assert!(READ_MEMOS.is_power_of_two());

const EMPTY_READ_MEMO: ReadMemo = ReadMemo {
    site: std::ptr::null(),
    callee: Value::UNDEFINED,
    generation: NO_SCRIPT_GENERATION,
    program: ProgramId::MAIN,
    this_binding: ThisBinding::Sloppy,
    read: MemoRead::Leaf(MemoLeaf::Field { base: 0, field: super::EMPTY_CACHE }),
};

/// The read memo slot of the call record `site`: the shared direct-mapped index of its
/// address (`shape_memo::direct_mapped_index`), which spreads a view's consecutive records.
#[inline(always)]
fn read_memo_index(site: Ip) -> usize {
    super::shape_memo::direct_mapped_index(site as usize, READ_MEMOS)
}

/// Records `target`'s read at the call record `site` (`lane_memoize_read`) unless a live memo
/// holds its slot: another site's, or the site's own, which then missed.
#[inline(always)]
fn lane_note_read<H: Host>(
    cx: *mut LaneContext<H>,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    let stack = unsafe { &vm(cx).lane_stack };
    let held = &stack.read_memos[read_memo_index(site)];
    if held.generation != stack.generation || held.site.is_null() {
        lane_memoize_read(cx, site, target, arguments, this);
    }
}

/// The memo of the read `target` just answered at `site` for `arguments` and `this`, and the
/// site's memo form (`lane_script_call_memo`); nothing for an arrow's own `this`, which the
/// memo does not hold, or a read whose records no longer hit.
#[cold]
#[inline(never)]
fn lane_memoize_read<H: Host>(
    cx: *mut LaneContext<H>,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    if target.this_binding == ThisBinding::Lexical {
        return;
    }
    let read = if target.entry == CallEntry::ForwardsRead {
        // SAFETY: as for `ForwardRead::of`.
        let Some(forward) = (unsafe { ForwardRead::of::<H>(target) }) else {
            return;
        };
        let Some((method, _)) = forward.method(vm, code, arguments, this) else {
            return;
        };
        // SAFETY: as `Vm::script_call_entry`; nothing here resolves a target or collects.
        let Some(inner) = vm.script_call_entry(method).map(|inner| unsafe { &*inner }) else {
            return;
        };
        if !inner.entry.reads() || inner.this_binding == ThisBinding::Lexical {
            return;
        }
        // SAFETY: as for `LeafRead::of`.
        let Some(read) = (unsafe { LeafRead::of::<H>(inner) }) else {
            return;
        };
        let entry = |hit: FieldHit| match hit {
            FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
        };
        MemoRead::Forward {
            base: forward.base,
            field: entry(forward.field),
            method_field: entry(forward.method),
            params: target.params,
            method,
            method_this: inner.this_binding,
            read: match MemoLeaf::of(vm, code, read) {
                Some(read) => read,
                None => return,
            },
        }
    } else {
        // SAFETY: as for `LeafRead::of`.
        let Some(read) = (unsafe { LeafRead::of::<H>(target) }) else {
            return;
        };
        match MemoLeaf::of(vm, code, read) {
            Some(read) => MemoRead::Leaf(read),
            None => return,
        }
    };
    vm.lane_stack.read_memos[read_memo_index(site)] = ReadMemo {
        site,
        callee: target.callee,
        generation: vm.lane_stack.generation,
        program: vm.active_program,
        this_binding: target.this_binding,
        read,
    };
    requicken::<H>(site, lane_script_call_memo::<H>);
}

/// The value the memo of the call record at `ip` answers its call with (`ReadMemo`); `None`
/// where the memo is another site's or stale, the call is another callee's or a construction's
/// (`lane_constructing`), or a hit misses.
///
/// # Safety
/// `cx` is the running lane's context and `ip` its call record `i`.
#[inline(always)]
unsafe fn lane_memo_read<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Option<Value> {
    let vm: &Vm<H> = unsafe { vm(cx) };
    let callee = r.get(i.b);
    let held = &vm.lane_stack.read_memos[read_memo_index(ip)];
    if held.site != ip
        || held.callee != callee
        || held.generation != vm.lane_stack.generation
        || held.program != vm.active_program
        || lane_constructing(vm, callee)
    {
        return None;
    }
    let this = lane_receiver_this(vm, held.this_binding, r.get(i.c))?;
    let arguments = ReadArguments {
        registers: r,
        base: ImmediateLayout::call_window_base(i.imm()),
        passed: ImmediateLayout::argument_count(i.imm()),
    };
    match &held.read {
        MemoRead::Leaf(read) => read.run(vm, arguments, this),
        MemoRead::Forward { base, field, method_field, params, method, method_this, read } => {
            let base = lane_read_base(crate::bytecode::FieldBase(*base), arguments, this)?;
            let receiver = vm.record_field_entry_hit(*field, base)?;
            if vm.record_field_entry_hit(*method_field, receiver)? != *method {
                return None;
            }
            let method_this = lane_receiver_this(vm, *method_this, receiver)?;
            // The call passes the parameters in order, so its argument `k` is parameter `k`.
            read.run(vm, arguments.within(*params), method_this)
        }
    }
}

script_handler! {
    /// `lane_script_call_full` at a site whose read call made a memo (`lane_memoize_read`): the
    /// memo's hits answer the call as the read target's would. A call the memo does not
    /// answer, or a returning call the frame does not admit (`lane_returning_call_admitted`),
    /// takes the full form, which the site then keeps.
    fn lane_script_call_memo<>(cx, r, ip, i, view) {
        let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
        let value = if !unsafe { (*cx).script_calls } || (returns && !lane_returning_call_admitted(cx)) {
            None
        } else {
            unsafe { lane_memo_read(cx, r, ip, i) }
        };
        let Some(value) = value else {
            requicken::<H>(ip, lane_script_call_full::<H>);
            sibling!(lane_script_call_full::<H>, r, ip, Carried::script(cx))
        };
        if !returns {
            r.set(i.a, value);
            next!(cx, r, following(ip), Carried::script(cx))
        }
        lane_return_value!(cx, ip, value)
    }
}

/// The object a read body's field record names: `this`, or the parameter its activation
/// would hold in that register; `None` for an unpassed one, whose `undefined` no read hits.
#[inline(always)]
fn lane_read_base(
    base: crate::bytecode::FieldBase,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    match base.register_index() {
        Some(register) => arguments.get(register),
        // A local base (`FieldBase::local`) names no activation register; the call runs.
        None if base.local_index().is_some() => None,
        None => Some(this),
    }
}

/// The `lane_returning` kind of a value record that returns its result from the frame.
fn returning_kind(instruction: WideInstruction) -> Option<u8> {
    if !instruction.returns_from_frame() {
        return None;
    }
    match instruction.op() {
        Op::GetField => Some(RETURNING_FIELD),
        Op::Unary => Some(RETURNING_UNARY),
        Op::Binary => Some(RETURNING_BINARY),
        Op::MakeObject2 => Some(RETURNING_OBJECT),
        Op::MakeObjectLiteral => Some(RETURNING_OBJECT_LITERAL),
        Op::SuperConstArrayObject2 => Some(RETURNING_LITERAL),
        _ => None,
    }
}

script_handler! {
    /// A value record that returns its result from the frame (`returning_kind`): the value
    /// as the record's own handler computes it, then the activation's completion, or the VM
    /// frame's return of it from the general loop (`lane_frame_return`). A value the
    /// handler does not compute takes the general path.
    fn lane_returning<KIND: u8>(cx, r, ip, i, view) {
        let value = unsafe {
            match KIND {
                RETURNING_FIELD_OWN
                | RETURNING_FIELD_POLYMORPHIC
                | RETURNING_FIELD_HOLDER
                | RETURNING_FIELD_INHERITED => {
                    let code = &*(*cx).code;
                    let vm = vm(cx);
                    let value = lane_single_field_base(cx, r, i.b)
                        .and_then(|object| returning_field_hit::<H, KIND>(vm, code, object, i.c));
                    let Some(value) = value else {
                        if KIND == RETURNING_FIELD_POLYMORPHIC {
                            sibling!(lane_returning::<H, RETURNING_FIELD>, r, ip, Carried::script(cx))
                        }
                        sibling!(lane_returning_field_miss::<H>, r, ip, Carried::script(cx))
                    };
                    Some(value)
                }
                RETURNING_FIELD_NESTED => {
                    let Some(value) = lane_nested_own_value(cx, r, i) else {
                        sibling!(lane_returning::<H, RETURNING_FIELD>, r, ip, Carried::script(cx))
                    };
                    Some(value)
                }
                // A read that meets its site's getter activates it, as `lane_get_field`
                // does: the record returns from the frame, so the getter's completion
                // completes this activation too (`lane_complete_activation`).
                RETURNING_FIELD => match lane_field_value(cx, r, i) {
                    Some(value) => Some(value),
                    None => {
                        return match lane_getter_activation(cx, r, ip, i) {
                            Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                            Err(next) => next,
                        };
                    }
                },
                RETURNING_UNARY => lane_unary_value(cx, r, i),
                RETURNING_BINARY => lane_returning_binary_value(cx, r, i),
                RETURNING_OBJECT => Some(lane_object_pair(cx, r, i)),
                RETURNING_OBJECT_LITERAL => Some(lane_object_literal_value(cx, r, i)),
                _ => lane_literal_value(cx, r, i),
            }
        };
        let Some(value) = value else {
            return exit(ip);
        };
        lane_return_value!(cx, ip, value)
    }
}

script_handler! {
    /// A returning `GetField`'s first run: a single read that names neither `name` nor
    /// `length` (`Vm::reads_function_info`) takes `RETURNING_FIELD_OWN`; any other keeps
    /// `RETURNING_FIELD`, whose cached read prepares them.
    #[inline(never)]
    fn lane_returning_field_first<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let handler = returning_field_first_kind(unsafe { vm(cx) }, unsafe { &*(*cx).code }, i);
        requicken::<H>(ip, handler);
        sibling!(handler, r, ip, Carried::script(cx))
    }
}

/// The kind a returning `GetField` takes on its first run (`lane_returning_field_first`),
/// which its record and site alone decide.
fn returning_field_first_kind<H: Host>(
    vm: &Vm<H>,
    code: &ResidualProgram,
    i: &LaneInstruction,
) -> Handler {
    if is_single_field_base(i.b) && !vm.reads_function_info(i.imm()) {
        lane_returning::<H, RETURNING_FIELD_OWN>
    } else if lane_field_site_tier(vm, code, i) == FieldSiteTier::Nested {
        lane_returning::<H, RETURNING_FIELD_NESTED>
    } else {
        lane_returning::<H, RETURNING_FIELD>
    }
}

script_handler! {
    /// A miss of a single-entry returning read (`RETURNING_FIELD_OWN`, `_HOLDER`,
    /// `_INHERITED`): the record moves to the kind its site's tier calls for
    /// (`lane_field_site_tier`), as `lane_get_field_miss` moves a plain read; the read itself
    /// takes `RETURNING_FIELD`.
    #[inline(never)]
    fn lane_returning_field_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        // The single-entry tiers read the object records alone; a receiver in a heap cell
        // keeps the general cached read.
        let cell_receiver = unsafe { lane_single_field_base(cx, r, i.b) }
            .is_some_and(|object| object.direct_object_index().is_none());
        let handler: Option<Handler> = match lane_field_site_tier(unsafe { vm(cx) }, code, i) {
            // A local base has no single-base kind (`is_single_field_base`).
            tier if tier != FieldSiteTier::Nested && !is_single_field_base(i.b) => {
                Some(lane_returning::<H, RETURNING_FIELD>)
            }
            FieldSiteTier::Own | FieldSiteTier::Holder | FieldSiteTier::Inherited if cell_receiver => {
                Some(lane_returning::<H, RETURNING_FIELD>)
            }
            FieldSiteTier::Nested => Some(lane_returning::<H, RETURNING_FIELD_NESTED>),
            FieldSiteTier::General | FieldSiteTier::Getter => Some(lane_returning::<H, RETURNING_FIELD>),
            FieldSiteTier::Polymorphic => Some(lane_returning::<H, RETURNING_FIELD_POLYMORPHIC>),
            FieldSiteTier::Holder => Some(lane_returning::<H, RETURNING_FIELD_HOLDER>),
            FieldSiteTier::Inherited => Some(lane_returning::<H, RETURNING_FIELD_INHERITED>),
            FieldSiteTier::Own => None,
        };
        if let Some(handler) = handler {
            requicken::<H>(ip, handler);
        }
        sibling!(lane_returning::<H, RETURNING_FIELD>, r, ip, Carried::script(cx))
    }
}

script_handler! {
    /// A JavaScript `Call` of a user function that needs a VM frame (no frameless target,
    /// `Vm::resolve_script_call_target`): the frame and pending call the general loop's
    /// in-loop call makes (`Vm::push_in_loop_call`), entered in place. Lane activations become
    /// frames first, as leaving the lane would make them (`materialize_lane_activations`).
    /// Any other call takes the general path, and the record resolves anew on its next run.
    #[inline(never)]
    fn lane_frame_call<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let callee = r.get(i.b);
        let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
        let target = vm.in_loop_call_target(code, callee);
        let admitted = |target: &(ProgramId, u32, Value)| {
            // SAFETY: the context outlives the lane run.
            let script_calls = unsafe { (*cx).script_calls };
            script_calls && (!returns || lane_returning_call_admitted(cx))
        };
        let Some(target) = target.filter(admitted) else {
            requicken::<H>(ip, script_call_handler::<H>(i.imm()));
            return exit(ip);
        };
        lane_push_frame_call(cx, r, ip, i, target, callee, r.get(i.c), FrameCallArguments::Window(0))
    }
}

/// The facts that keep function `function` of `program` out of a frameless activation, for
/// the census of frame calls and constructions.
#[cfg(feature = "profile-lane-exits")]
fn frame_entry_census<H: Host>(vm: &Vm<H>, program: ProgramId, function: u32) -> Option<String> {
    vm.programs.residual(program).and_then(|callee_code| {
        let body = callee_code.function_get(function as usize)?;
        let plan = body.call_plan(callee_code);
        let closures = body.code.iter().any(|packed| decoded_instruction(body, *packed).op() == Op::MakeClosure);
        Some(format!(
            "{} @{:?} registers_only={} env_activation={} arrow={} rest={} closures={} arguments={} handlers={} class_ctor={} async={} generator={} promoted={}/{} simple_params={} selective={} self={} plain={} in_loop={} legacy={} live={} addresses={} initial={:?} name_ops={:?} unpromoted={:?}",
            body.name.map(|atom| callee_code.atoms[atom as usize].to_string()).unwrap_or_default(),
            body.source_span.map(|span| span.start), plan.registers_only, plan.environment_activation, body.is_arrow, body.rest,
            closures, body.arguments_slot.is_some(), !body.handlers.is_empty(), body.is_class_constructor, body.is_async,
            body.is_generator, body.local_registers.len(), body.locals, body.simple_parameters, body.selective_capture_slots.is_some(),
            body.self_binding_slot.is_some(), plan.plain, plan.in_loop, plan.legacy_reflection, plan.live_parameters,
            plan.addresses_locals, body.initial_register,
            body.code.iter().map(|packed| decoded_instruction(body, *packed)).filter(|instruction| matches!(instruction.op(),
                Op::LoadName | Op::LoadNameCall | Op::LoadNameTypeof | Op::ResolveName | Op::LoadResolvedName | Op::DeleteName
                | Op::StoreName | Op::StoreResolvedName | Op::StoreVarBinding | Op::CloneEnv | Op::InitializeThis | Op::SuperCallCheck))
                .map(|instruction| format!("{:?}:{}", instruction.op(), { let atom = instruction.atom_index() as usize; if atom < callee_code.atoms.len() { callee_code.atoms[atom].replace('\0', "~") } else { "?".into() } }))
                .collect::<Vec<_>>(),
            body.local_atoms.iter().enumerate()
                .filter(|(slot, _)| !body.local_registers.iter().any(|entry| usize::from(entry.local) == *slot))
                .map(|(_, atom)| callee_code.atoms[*atom as usize].replace('\0', "~"))
                .collect::<Vec<_>>(),
        ))
    })
}

/// `lane_frame_call`'s call of `callee`, resolved to the in-loop `target`, with receiver
/// `this` and the call window's values past its first `skip` (a forwarded call's own
/// arguments, `Vm::forwarded_call`): entered directly when the JavaScript tier promoted it,
/// else pushed as the general loop's in-loop call pushes it.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn lane_push_frame_call<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: (ProgramId, u32, Value),
    callee: Value,
    this: Value,
    passed: FrameCallArguments<'_>,
) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    // A callee of another program runs in the lane only where the lane admits its program.
    if target.0 != unsafe { (*cx).program }
        && !vm.programs.residual(target.0).is_some_and(|callee_code| vm.lane_admits(callee_code))
    {
        return exit(ip);
    }
    // A callee that has not made its deferred runs takes the general path, as a lane call's
    // does (`ProgramStore::lane_call_may_derive`): entering its frame here derived its view at
    // its first call, a callee run once included (jiti npm scenario: -2.1% instructions).
    if !vm.programs.lane_call_may_derive(target.0, target.1) {
        return exit(ip);
    }
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = frame_entry_census(vm, target.0, target.1) {
        super::lane_exits::record_detail(format!("frame call {body}"));
    }
    let window = ImmediateLayout::call_window_base(i.imm());
    let argc = ImmediateLayout::argument_count(i.imm());
    // A function the JavaScript tier promoted is entered directly, as the general loop's
    // in-loop call enters it (`Vm::js_jit_compiled_call`); a guard miss pushes its frame. A
    // forwarded call is pushed, as the general loop's (`Vm::try_inline_call_apply`).
    // SAFETY: the call's window is registers of the running function, which the direct
    // call only reads.
    let arguments = match passed {
        FrameCallArguments::Window(skip) => (skip == 0).then(|| unsafe {
            std::slice::from_raw_parts(r.0.add(usize::from(window)), usize::from(argc))
        }),
        FrameCallArguments::List(list) => Some(list),
    };
    if let Some(arguments) = arguments
        && let Some(value) = vm.js_jit_compiled_call(code, (target.0, target.1), returns, arguments)
    {
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
    vm.materialize_lane_activations(cx);
    // The caller is now the innermost frame, its registers the frame's.
    let (frame, r) = unsafe { ((*cx).frame, (*cx).registers) };
    // SAFETY: the call record lies in the running view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    // SAFETY: the call's window is registers of the caller frame, which the push neither
    // moves nor frees before it copies them; a list outlives the push.
    let arguments: *const [Value] = match passed {
        FrameCallArguments::Window(skip) => unsafe {
            std::slice::from_raw_parts(r.0.add(usize::from(window) + skip), usize::from(argc) - skip)
        },
        FrameCallArguments::List(list) => list,
    };
    let destination = i.a & crate::bytecode::REGISTER_MASK;
    let pushed = vm.push_in_loop_call(
        code,
        frame,
        call_pc + 1,
        target,
        callee,
        this,
        unsafe { &*arguments },
        destination,
    );
    match pushed {
        Some(Ok(StepResult::PushFrame {
            destination,
            call_guard,
            construct_this,
            ..
        })) => {
            // A callee of another program runs in its own, which the push made active
            // and the loop's executing one sets aside as the loop's own push does.
            let caller_program = (target.0 != unsafe { (*cx).program }).then(|| {
                // SAFETY: the general loop's executing program outlives the lane run.
                let executing = unsafe { &mut *(*cx).executing };
                let saved = super::dispatch_frame::SavedProgram {
                    id: executing.id,
                    retained: std::mem::replace(&mut executing.retained, vm.programs.get(target.0)),
                };
                executing.id = target.0;
                lane_enter_program(cx, target.0);
                saved
            });
            // SAFETY: the general loop's pending calls outlive the lane run.
            unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
                caller: frame,
                caller_cursor: None,
                call_pc: call_pc as u32,
                destination,
                construct_this,
                settles: None,
                returns,
                call_guard: Some(call_guard),
                caller_program,
            });
            // The pushed frame's guard narrows the activations the thread admits.
            vm.lane_stack.refresh_activation_limit();
            // The tier observes the frame's entry as the general loop's would, and a
            // native result continues at the `Return` that completes the frame.
            let callee_frame = frame + 1;
            let handoff = vm.js_tier_needs_general_entry(
                vm.frames[callee_frame].program,
                vm.frames[callee_frame].function,
            );
            if !handoff {
                // SAFETY: the context's program, now the callee's, outlives the lane run.
                if let super::compiled::JsTierFrameEntry::ContinueAt(return_pc) = vm
                    .js_tier_frame_entry(unsafe { &*(*cx).code }, callee_frame, false)
                {
                    vm.frames[callee_frame].pc = return_pc;
                }
            }
            let callee_ip = lane_enter_script_frame(cx, callee_frame);
            if handoff {
                exit(callee_ip)
            } else {
                callee_ip
            }
        }
        // A failed push (the guest call depth) throws at the call, as the general path's.
        Some(Err(error)) => {
            vm.lane_raised = Some(error);
            exit(ip)
        }
        _ => exit(ip),
    }
}

/// `new` of a user constructor that needs a VM frame, made in place as the general loop's
/// in-loop construction makes it (`Vm::inline_construct_target`): lane activations become
/// frames, the receiver is allocated, and the constructor's frame is pushed with the pending
/// construction that substitutes the receiver for a non-object result
/// (`lane_return_script_frame`). Any other construction takes the general path.
#[inline(never)]
fn lane_frame_construct<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let callee = r.get(i.b);
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    // The checks `inline_construct_target` makes of the callee alone, before activations
    // become frames for it. A constructor of another program is pushed in its own, where the
    // lane admits it.
    let pushable = match vm.heap.function(callee) {
        Some(FunctionRecord { kind: FunctionKind::User(program, id), realm, .. }) => {
            *realm == vm.realm.globals
                && if *program == unsafe { (*cx).program } {
                    code.function_get(*id as usize).is_some_and(Vm::<H>::inline_constructible)
                } else {
                    vm.programs.residual(*program).is_some_and(|callee_code| {
                        vm.lane_admits(callee_code)
                            && callee_code
                                .function_get(*id as usize)
                                .is_some_and(Vm::<H>::inline_constructible)
                    })
                }
        }
        _ => false,
    };
    if !pushable
        || !unsafe { (*cx).script_calls && (*cx).script_constructs }
        || vm.construct_target.is_some()
        || returns && !lane_returning_call_admitted(cx)
    {
        return exit(ip);
    }
    vm.materialize_lane_activations(cx);
    // The caller is now the innermost frame, its registers the frame's.
    let (frame, r) = unsafe { ((*cx).frame, (*cx).registers) };
    let Some((program, id, env, this)) = vm.inline_construct_target(code, frame, callee) else {
        return exit(ip);
    };
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = frame_entry_census(vm, program, id) {
        super::lane_exits::record_detail(format!("frame construct {body}"));
    }
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    // SAFETY: the call record lies in the running view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    vm.frames[frame].pc = call_pc + 1;
    vm.frames[frame].pc_is_resume = true;
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    // SAFETY: the construction's window is registers of the caller frame, which the push
    // neither moves nor frees before it copies them.
    let arguments: *const [Value] = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.c)), argc) };
    vm.construct_target = Some(super::activation::ConstructionTarget { callee, new_target: callee });
    // A constructor of another program runs in its own, which the push makes active and the
    // loop's executing one sets aside, as `lane_push_frame_call` pushes a call.
    let caller_program = (program != unsafe { (*cx).program }).then(|| {
        // SAFETY: the general loop's executing program outlives the lane run.
        let executing = unsafe { &mut *(*cx).executing };
        let saved = super::dispatch_frame::SavedProgram {
            id: executing.id,
            retained: std::mem::replace(&mut executing.retained, vm.programs.get(program)),
        };
        executing.id = program;
        lane_enter_program(cx, program);
        saved
    });
    // SAFETY: the context's program is now the callee's, which outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let pushed = vm.push_general_user_frame(code, id, env, this, unsafe { &*arguments }, CallContext::user_function(id, callee));
    vm.construct_target = None;
    match pushed {
        Ok(call_guard) => {
            // SAFETY: the general loop's pending calls outlive the lane run.
            unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
                caller: frame,
                caller_cursor: None,
                call_pc: call_pc as u32,
                destination: i.a & crate::bytecode::REGISTER_MASK,
                construct_this: Some(this),
                settles: None,
                returns,
                call_guard: Some(call_guard),
                caller_program,
            });
            // The pushed frame's guard narrows the activations the thread admits.
            vm.lane_stack.refresh_activation_limit();
            let callee_frame = frame + 1;
            let handoff = vm.js_tier_needs_general_entry(
                vm.frames[callee_frame].program,
                vm.frames[callee_frame].function,
            );
            if !handoff {
                if let super::compiled::JsTierFrameEntry::ContinueAt(return_pc) =
                    vm.js_tier_frame_entry(code, callee_frame, false)
                {
                    vm.frames[callee_frame].pc = return_pc;
                }
            }
            let callee_ip = lane_enter_script_frame(cx, callee_frame);
            if handoff {
                exit(callee_ip)
            } else {
                callee_ip
            }
        }
        // A failed push (the guest call depth) throws at the construction, as the general
        // path's. The caller's program is the executing one again.
        Err(error) => {
            if let Some(saved) = caller_program {
                // SAFETY: as above.
                let executing = unsafe { &mut *(*cx).executing };
                executing.id = saved.id;
                executing.retained = saved.retained;
                lane_enter_program(cx, saved.id);
            }
            vm.lane_raised = Some(error);
            exit(ip)
        }
    }
}

/// Continue in JavaScript frame `frame` at its resume point, with the lane's per-frame
/// facts for it: `new` activates in place only from an ordinary body
/// (`LaneContext::script_constructs`).
#[inline(always)]
fn lane_enter_script_frame<H: Host>(cx: *mut LaneContext<H>, frame: usize) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let function = vm.frames[frame].function;
    unsafe {
        (*cx).script_constructs = code
            .function_get(function as usize)
            .is_some_and(|function| function.dispatch == crate::bytecode::DispatchClass::General);
    }
    lane_enter_frame(cx, frame, vm.frames[frame].pc)
}

/// The return of the running JavaScript frame with `value` to the caller frame the general
/// loop's pending call names, made in place as that loop makes it: the frame retires, an
/// in-loop `new` substitutes its receiver for a non-object result, and the caller continues
/// with the value in the call's register. A call that returns its callee's completion
/// completes its caller in turn: another pending caller here, or the loop's own frame,
/// which the general loop returns from at that call (`lane_frame_return`). A call into
/// another program returns to the caller's, as the loop's return restores it.
#[inline(never)]
fn lane_return_script_frame<H: Host>(
    cx: *mut LaneContext<H>,
    ip: Ip,
    mut value: Value,
    mut retired: bool,
) -> Ip {
    let vm = unsafe { vm(cx) };
    let pending = unsafe { &mut *(*cx).pending };
    let mut frame = unsafe { (*cx).frame };
    let mut exit_ip = ip;
    loop {
        // An async callee's promise settles on the general path (`PendingGeneralCall::settles`).
        let returns_here = pending.last().is_some_and(|call| {
            call.caller + 1 == frame && frame + 1 == vm.frames.len() && call.settles.is_none()
        });
        if !returns_here {
            // A record that has not run yet runs again on the general path; one that ran (a
            // call made, `retired`) completes the loop's own frame with the value there.
            if !retired {
                return exit(exit_ip);
            }
            return lane_frame_return(vm, exit_ip, value);
        }
        let mut call = pending.pop().expect("a pending call names the returning frame");
        let completed = vm.frames.pop().expect("the returning frame is the innermost");
        vm.finish_frame_scopes(&completed);
        vm.pool_frame(completed);
        retired = true;
        // A call into another program returns to the caller's, which the loop executes again.
        if let Some(saved) = call.caller_program.take() {
            // SAFETY: the general loop's executing program outlives the lane run.
            let executing = unsafe { &mut *(*cx).executing };
            executing.id = saved.id;
            executing.retained = saved.retained;
            lane_enter_program(cx, saved.id);
        }
        value = match call.construct_this {
            Some(this) if !vm.is_object_like(value) => this,
            _ => value,
        };
        drop(call.call_guard);
        frame = call.caller;
        vm.active_registers = vm.frames[frame].registers.as_mut_ptr();
        if !call.returns {
            vm.write(frame, call.destination, value);
            return lane_enter_script_frame(cx, frame);
        }
        // The caller completes too, at its call record.
        let (_, call_ip, _) = lane_frame_state(cx, frame, call.call_pc as usize);
        exit_ip = call_ip;
    }
}

/// Return from a frame the general loop or a frame push made. Out of line
/// so the activation return saves no registers.
#[cold]
#[inline(never)]
fn lane_return_frame<H: Host>(r: Registers, ip: Ip) -> Ip {
    let cx = lane_context::<H>();
    // SAFETY: called only while a lane runs, from its return handler.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).inline_calls } && unsafe { (*cx).script_calls } {
        return lane_return_script_frame(cx, ip, r.get(i.a), false);
    }
    let pending = unsafe { &mut *(*cx).pending };
    // An in-loop `new` substitutes its receiver for a non-object result;
    // the general path owns that [[Construct]] step.
    if !unsafe { (*cx).inline_calls }
        || pending.last().is_none_or(|call| {
            call.construct_this.is_some() || call.caller_program.is_some() || call.settles.is_some()
        })
    {
        return exit(ip);
    }
    let value = r.get(i.a);
    let Some(call) = pending.pop() else {
        return exit(ip);
    };
    vm.retire_wasm_frame();
    vm.active_registers = vm.frames[call.caller].registers.as_mut_ptr();
    vm.write(call.caller, call.destination, value);
    drop(call.call_guard);
    let pc = vm.frames[call.caller].pc;
    lane_enter_frame(cx, call.caller, pc)
}

/// A lane opcode's handler, and whether that handler is an accumulator
/// producer: whenever it falls through to the next record, it has written
/// its result register and carries the written value into that record.
#[derive(Clone, Copy)]
struct LaneHandler {
    handler: Handler,
    produces: bool,
}

impl LaneHandler {
    const fn producer(handler: Handler) -> Option<Self> {
        Some(Self {
            handler,
            produces: true,
        })
    }
}

/// The handler serving opcode `OP` whose accumulator field is `ACC`, derived
/// from the opcode's semantic family; `None` when the general path executes
/// the opcode.
const fn handler_for<H: Host, const OP: u16, const BACKWARD: bool, const ACC: u8>()
-> Option<LaneHandler> {
    let Some(op) = Op::from_index(OP as usize) else {
        return None;
    };
    if I32BinaryOperator::from_register_op(op).is_some()
        || I32BinaryOperator::from_immediate_op(op).is_some()
    {
        return LaneHandler::producer(lane_i32_binary::<H, OP, ACC>);
    }
    let handler: Handler = if I32BinaryOperator::from_jump_op(op).is_some()
        || I32BinaryOperator::from_immediate_jump_op(op).is_some()
    {
        lane_compare_jump::<H, OP, BACKWARD, ACC>
    } else if MemoryLoad::from_direct_op(op).is_some() {
        return LaneHandler::producer(lane_load::<H, OP, ACC>);
    } else if MemoryStore::from_direct_op(op).is_some() {
        lane_store::<H, OP, ACC>
    } else {
        match op {
            Op::Move => return LaneHandler::producer(lane_move::<H, ACC>),
            Op::LoadConst => return LaneHandler::producer(lane_load_const::<H>),
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                return LaneHandler::producer(lane_bit_field::<H, ACC>);
            }
            Op::WasmSelect => return LaneHandler::producer(lane_select::<H, ACC>),
            Op::WasmI32AddImmediateJumpNonZero => {
                return LaneHandler::producer(lane_add_jump_nonzero::<H, BACKWARD, ACC>);
            }
            Op::WasmI32Unary => return LaneHandler::producer(lane_i32_unary::<H, ACC>),
            Op::WasmF32Binary | Op::WasmF64Binary => {
                return LaneHandler::producer(lane_float_binary::<H, OP, ACC>);
            }
            Op::WasmF32Unary | Op::WasmF64Unary => {
                return LaneHandler::producer(lane_float_unary::<H, OP, ACC>);
            }
            Op::WasmI64Binary => return LaneHandler::producer(lane_i64_binary::<H, ACC>),
            Op::WasmI64Unary => return LaneHandler::producer(lane_i64_unary::<H, ACC>),
            Op::WasmScalarConvert => {
                return LaneHandler::producer(lane_scalar_convert::<H, ACC>);
            }
            _ => match other_handler::<H, BACKWARD, ACC>(op) {
                Some(handler) => handler,
                None => return None,
            },
        }
    };
    Some(LaneHandler {
        handler,
        produces: false,
    })
}

/// Handlers that neither produce nor read the accumulator, apart from the
/// branch tests, which read it.
const fn other_handler<H: Host, const BACKWARD: bool, const ACC: u8>(op: Op) -> Option<Handler> {
    match op {
        Op::StoreLocalPlain => Some(lane_store_local_plain::<H>),
        Op::IteratorCleanupPush => Some(lane_iterator_cleanup_push::<H>),
        Op::DefineArrayElement => Some(lane_define_array_element::<H>),
        Op::RequireIteratorResult => Some(lane_require_iterator_result::<H>),
        Op::IteratorCleanupPop => Some(lane_iterator_cleanup_pop::<H>),
        Op::StoreLocal | Op::StoreEnvLocal => Some(lane_store_local::<H>),
        Op::LoadThis => Some(lane_load_this::<H>),
        Op::InitializeTdz => Some(lane_initialize_tdz::<H>),
        Op::CloneEnv => Some(lane_clone_env::<H>),
        Op::GetField => Some(lane_get_field_first::<H>),
        Op::SetField | Op::SetFieldStrict => Some(lane_set_object_field_own::<H>),
        Op::SetThisField | Op::SetThisFieldStrict => Some(lane_set_this_field_own::<H>),
        Op::ToNumeric => Some(lane_to_numeric::<H>),
        Op::RequireObjectCoercible => Some(lane_require_object_coercible::<H>),
        Op::DefinePropertyRecord => Some(lane_define_property_record::<H>),
        Op::DefineField => Some(lane_define_field::<H>),
        Op::DefinePrivateField => Some(lane_define_private_field::<H>),
        Op::CheckPrivate => Some(lane_check_private::<H>),
        Op::PrivateIn => Some(lane_private_in::<H>),
        Op::CopyDataProperties => Some(lane_copy_data_properties::<H>),
        op if lane_steps_in_place(op) => Some(lane_general_step::<H>),
        Op::IncDec => Some(lane_inc_dec::<H, false>),
        op if matches!(op.js_denotation(), crate::bytecode::JsDenotation::NumericBinary) => {
            Some(lane_binary::<H>)
        }
        Op::GetIndex => Some(lane_get_index::<H>),
        Op::SetIndex => Some(lane_set_index::<H>),
        Op::LoadCapture => Some(lane_load_capture::<H>),
        Op::JumpBinaryFalse => Some(lane_jump_binary_false::<H, BACKWARD>),
        Op::LoadLocalPlain => Some(lane_load_local_plain::<H>),
        Op::MakeConstArray => Some(lane_make_const_array::<H>),
        Op::LoadLocal | Op::LoadEnvLocal => Some(lane_load_local::<H>),
        Op::Jump => Some(lane_jump_always::<H, BACKWARD>),
        Op::JumpFalse => Some(lane_jump_false::<H, BACKWARD>),
        Op::JumpTrue => Some(lane_jump_true::<H, BACKWARD>),
        Op::Unary => Some(lane_unary::<H>),
        Op::StoreCapture => Some(lane_store_capture::<H>),
        Op::WasmBranchTable => Some(lane_branch_table::<H, BACKWARD>),
        Op::WasmJumpI32Zero => Some(lane_jump_i32_zero::<H, true, BACKWARD, ACC, false>),
        Op::WasmJumpI32NonZero => Some(lane_jump_i32_zero::<H, false, BACKWARD, ACC, false>),
        Op::WasmInstanceBinding => Some(lane_instance_binding::<H, false>),
        Op::WasmGlobalGet => Some(lane_global_get::<H>),
        Op::WasmSimd => Some(lane_wasm_simd::<H>),
        Op::WasmMemoryLoad => Some(lane_wasm_memory_load::<H>),
        Op::WasmMemoryAddress => Some(lane_wasm_memory_address::<H>),
        Op::WasmMemoryStore => Some(lane_wasm_memory_store::<H>),
        Op::WasmSimdShuffle => Some(lane_wasm_simd_shuffle::<H>),
        Op::WasmGlobalSet => Some(lane_global_set::<H>),
        Op::WasmFillRegisters => Some(lane_fill_registers::<H>),
        Op::CallKnown => Some(lane_call_known::<H>),
        Op::Call => Some(lane_script_call::<H, ANY_ARGC>),
        Op::CallArray => Some(lane_call_array::<H>),
        Op::CallSpread => Some(lane_call_spread::<H>),
        Op::Delete => Some(lane_delete::<H>),
        Op::Construct => Some(lane_script_construct::<H>),
        Op::LoadName => Some(lane_load_name_first::<H, LOAD_NAME>),
        Op::LoadNameCall => Some(lane_load_name_first::<H, LOAD_NAME_CALL>),
        Op::LoadNameTypeof => Some(lane_load_name_first::<H, LOAD_NAME_TYPEOF>),
        Op::MakeObject2 => Some(lane_make_object2::<H>),
        Op::MakeClosure => Some(lane_make_closure::<H>),
        Op::MakeObjectLiteral => Some(lane_make_object_literal::<H>),
        Op::MakeArray => Some(lane_make_array::<H>),
        Op::MakeObject => Some(lane_make_object::<H>),
        Op::CreateRegExpLiteral => Some(lane_regexp_literal::<H>),
        Op::CallForwardingArguments => Some(lane_forward_call::<H>),
        Op::SuperConstArrayObject2 => Some(lane_const_array_object2::<H>),
        Op::Return => Some(lane_return::<H>),
        _ => None,
    }
}

/// Dense fast handlers cover the first opcode byte; later opcodes use general dispatch.
const TABLE_SLOTS: usize = 1 << u8::BITS;

/// Opcodes per generated table row.
const TABLE_ROW: u16 = 16;
const TABLE_ROWS: usize = TABLE_SLOTS / TABLE_ROW as usize;

macro_rules! table_row {
    ($h:ty, $backward:literal, $acc:expr, $row:literal) => {
        [
            handler_for::<$h, { $row * TABLE_ROW }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 1 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 2 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 3 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 4 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 5 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 6 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 7 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 8 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 9 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 10 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 11 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 12 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 13 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 14 }, $backward, { $acc }>(),
            handler_for::<$h, { $row * TABLE_ROW + 15 }, $backward, { $acc }>(),
        ]
    };
}

const fn flatten_rows(
    rows: [[Option<LaneHandler>; TABLE_ROW as usize]; TABLE_ROWS],
) -> [Option<LaneHandler>; TABLE_SLOTS] {
    let mut table = [None; TABLE_SLOTS];
    let mut slot = 0;
    while slot < TABLE_SLOTS {
        table[slot] = rows[slot / TABLE_ROW as usize][slot % TABLE_ROW as usize];
        slot += 1;
    }
    table
}

/// One handler table: the opcode's handler for one jump direction and one
/// accumulator field.
type Table = [Option<LaneHandler>; TABLE_SLOTS];

macro_rules! lane_table {
    ($h:ty, $backward:literal, $acc:expr) => {
        flatten_rows([
            table_row!($h, $backward, $acc, 0),
            table_row!($h, $backward, $acc, 1),
            table_row!($h, $backward, $acc, 2),
            table_row!($h, $backward, $acc, 3),
            table_row!($h, $backward, $acc, 4),
            table_row!($h, $backward, $acc, 5),
            table_row!($h, $backward, $acc, 6),
            table_row!($h, $backward, $acc, 7),
            table_row!($h, $backward, $acc, 8),
            table_row!($h, $backward, $acc, 9),
            table_row!($h, $backward, $acc, 10),
            table_row!($h, $backward, $acc, 11),
            table_row!($h, $backward, $acc, 12),
            table_row!($h, $backward, $acc, 13),
            table_row!($h, $backward, $acc, 14),
            table_row!($h, $backward, $acc, 15),
        ])
    };
}

struct LaneTable<H>(std::marker::PhantomData<H>);

impl<H: Host> LaneTable<H> {
    /// Tables indexed by jump direction (forward, then backward: a
    /// safepoint) and by accumulator field.
    const TABLES: [[Table; ACC_POSITIONS]; 2] = [
        [
            lane_table!(H, false, ACC_NONE),
            lane_table!(H, false, ACC_A),
            lane_table!(H, false, ACC_B),
            lane_table!(H, false, ACC_C),
            lane_table!(H, false, ACC_PAIR),
        ],
        [
            lane_table!(H, true, ACC_NONE),
            lane_table!(H, true, ACC_A),
            lane_table!(H, true, ACC_B),
            lane_table!(H, true, ACC_C),
            lane_table!(H, true, ACC_PAIR),
        ],
    ];
}

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
pub(super) fn lane_fields_fit(instruction: WideInstruction, function: &Function) -> bool {
    let registers = function.registers;
    let op = instruction.op();
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    let numeric_local = instruction.writes_numeric_local()
        && instruction.a() & crate::bytecode::RETURN_REGISTER == 0
        && instruction.result_register() < function.locals;
    let registers_fit = fields
        .iter()
        .all(|&(field, value)| match op.field_layout(field) {
            FieldLayout::ResultRegister if field == InstructionField::A && numeric_local => true,
            // A lane call reads its return flag from the field (`lane_call_fits`).
            FieldLayout::ResultRegister
                if field == InstructionField::A
                    && (matches!(
                        op,
                        Op::Call
                            | Op::CallForwardingArguments
                            | Op::Construct
                            | Op::GetField
                            | Op::Unary
                            | Op::MakeObject2
                            | Op::MakeObjectLiteral
                            | Op::SuperConstArrayObject2
                    )
                        || (op == Op::Binary && instruction.returns_from_frame())) =>
            {
                instruction.result_register() < registers
            }
            FieldLayout::FieldBase => {
                value == crate::bytecode::FieldBase::NESTED
                    || crate::bytecode::FieldBase(value)
                        .register_index()
                        .is_none_or(|register| register < registers)
            }
            FieldLayout::OptionalRegister | FieldLayout::OptionalReadRegister => value
                .checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS)
                .is_none_or(|register| register < registers),
            FieldLayout::Operand => {
                let operand = crate::bytecode::Operand(value);
                operand.kind() != Some(crate::bytecode::OperandKind::Register)
                    || operand.payload() < registers
            }
            layout => !layout.is_register_field() || value < registers,
        });
    let window_fits = !matches!(op, Op::WasmFillRegisters | Op::MakeObjectLiteral)
        || u32::from(instruction.b()) + u32::from(instruction.c()) <= u32::from(registers);
    let pair_fits = op.immediate_layout() != ImmediateLayout::RegisterPair || {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        first < registers && second < registers
    };
    registers_fit && window_fits && pair_fits
}

/// The record operand of `instruction` at `pc`, or `None` when its jump or
/// constant cannot be resolved.
fn lane_operand(
    instruction: WideInstruction,
    pc: usize,
    constants: &[Value],
) -> Option<LaneOperand> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    Some(match instruction.op().immediate_role() {
        ImmediateRole::JumpTarget => {
            let distance = isize::try_from(instruction.imm()).ok()? - isize::try_from(pc).ok()?;
            LaneOperand {
                jump: distance.checked_mul(record)?,
            }
        }
        ImmediateRole::ConstantIndex => LaneOperand {
            constant: *constants.get(instruction.imm() as usize)?,
        },
        _ if instruction.op() == Op::GetField => LaneOperand {
            field: FieldOperand { imm: instruction.imm(), shape: super::NO_FIELD_RECEIVER },
        },
        _ => LaneOperand {
            imm: instruction.imm(),
        },
    })
}

/// A `Call` runs in the lane only as the general loop pushes one in place: it evaluates no
/// direct or parameter-scope `eval`. It, `GetField`, `Unary` and `Binary` may return their
/// result from the frame (`lane_returns`, `lane_returning`); a returning record runs no
/// other result flag.
fn lane_call_fits(instruction: WideInstruction) -> bool {
    match instruction.op() {
        Op::Call | Op::CallArray | Op::CallSpread => {
            lane_returns(instruction)
                && !instruction.direct_eval()
                && !instruction.parameter_eval()
        }
        Op::CallForwardingArguments => lane_returns(instruction),
        Op::Construct => {
            lane_returns(instruction)
                && !instruction.is_super_construct()
                && !ImmediateLayout::construct_array_arguments(instruction.imm())
        }
        Op::GetField => lane_returns(instruction) || lane_sinks_field(instruction),
        Op::Unary | Op::MakeObject2 | Op::SuperConstArrayObject2 | Op::MakeObjectLiteral => {
            lane_returns(instruction)
        }
        Op::Binary if instruction.returns_from_frame() => lane_returns(instruction),
        _ => true,
    }
}

/// A `Call` record's operand, flagged when liveness (`live`, the registers live before each
/// pc) shows its result register dead after the call: nothing reads the value the callee
/// returns (`ImmediateLayout::lane_call_result_used`).
fn lane_call_result_marked(
    operand: LaneOperand,
    instruction: WideInstruction,
    function: &Function,
    pc: usize,
) -> LaneOperand {
    if instruction.op() != Op::Call || !function.register_roots.call_result_dead(pc) {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`lane_operand`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_UNUSED_FLAG }
}

/// Whether the `Call` at `pc` is an iteration step: no arguments, its result checked by the
/// `RequireIteratorResult` after it, as the compiler emits a for-of or destructuring step,
/// whose result register only the step's own `done` and `value` reads take
/// (`lane_iterator_step`).
fn lane_iteration_step(decoded: &[WideInstruction], pc: usize) -> bool {
    let call = decoded[pc];
    call.result_flags() == 0
        && ImmediateLayout::argument_count(call.imm()) == 0
        && decoded.get(pc + 1).is_some_and(|next| {
            next.op() == Op::RequireIteratorResult && next.register_b() == call.result_register()
        })
}

/// A nested `GetField` whose only result flag stores its value to a field of `this` too
/// (`lane_get_field_sink`).
fn lane_sinks_field(instruction: WideInstruction) -> bool {
    instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER
        && instruction.writes_current_this()
}

/// Whether the instruction's only result flag, if any, returns the result from the frame.
fn lane_returns(instruction: WideInstruction) -> bool {
    instruction.result_flags() & !crate::bytecode::RETURN_REGISTER == 0
}

/// The register every compiled linear-memory access of `function` goes
/// through, when there is exactly one.
pub(super) fn memory_register(function: &Function) -> Option<u16> {
    let mut registers = function
        .code
        .iter()
        .map(|packed| decoded_instruction(function, *packed))
        .filter_map(|instruction| match instruction.op() {
            op if MemoryLoad::from_direct_op(op).is_some()
                || MemoryStore::from_direct_op(op).is_some() => Some(instruction.b()),
            Op::WasmMemorySize => Some(instruction.b()),
            Op::WasmMemoryFill => Some(instruction.a()),
            _ => None,
        });
    let first = registers.next()?;
    registers.all(|register| register == first).then_some(first)
}

/// Whether a lane run of `instruction` could write `register`.
fn lane_writes(instruction: WideInstruction, register: u16) -> bool {
    let op = instruction.op();
    let result = matches!(
        op.field_layout(InstructionField::A),
        FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
    ) && instruction.a() == register;
    let optional = op.field_layout(InstructionField::B) == FieldLayout::OptionalRegister
        && instruction
            .b()
            .checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS)
            == Some(register);
    let filled = op == Op::WasmFillRegisters
        && (instruction.b()..instruction.b().saturating_add(instruction.c())).contains(&register);
    result || optional || filled
}

/// The shortest straight-line lane run the general loop enters. Entering
/// builds the lane context and memory view, which one or two lane
/// instructions do not repay: on V8-v7 fixed work, three keeps every
/// JavaScript benchmark within one percent of the general path or better
/// (one and two cost DeltaBlue three percent), and leaves Wasm loops,
/// whose runs are long, unchanged.
const LANE_ENTRY_MIN_RUN: usize = 3;

/// `LANE_ENTRY_MIN_RUN` in a function with no loop the lane runs whole: each call the
/// general loop makes into it enters the lane anew, so a run must repay a fresh lane
/// context.
const LANE_ENTRY_MIN_CALL_RUN: usize = 8;

/// Whether records may read the accumulator. Without sibling calls every
/// handler returns to the lane loop, which carries no accumulator.
const ACCUMULATOR: bool = cfg!(quench_lane_tail_calls);

/// Derive a function's lane view from its bytecode and its program's
/// constants.
fn derive_lane_view<'live, H: Host>(
    function_id: u32,
    function: &Function,
    constants: &[Value],
    kind: crate::bytecode::ProgramKind,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    sites: &[crate::bytecode::FieldSite],
    loop_tier: bool,
    general_dispatch: bool,
    diagnostics: bool,
) -> LaneView {
    // Each pass below reads instructions by PC; they are decoded once, here.
    let decoded: Vec<WideInstruction> = function.instructions().collect();
    // Every backward jump observes host termination, including allocation-free
    // Wasm loops. Collection and stack-floor exits share this safepoint.

    let tables = const { &LaneTable::<H>::TABLES };
    // Only Wasm bodies access linear memory.
    let memory_register = (kind == crate::bytecode::ProgramKind::Wasm)
        .then(|| memory_register(function))
        .flatten();
    let jump_targets = jump_targets(&decoded);
    // Wasm instructions, whose fusions the passes under `wasm` make, occur only in Wasm
    // bodies: JavaScript compiles to none of them.
    let wasm = kind == crate::bytecode::ProgramKind::Wasm;
    let resolves_names = function.has_dynamic_scope_access();
    // One function-wide fact, consulted per local load; it scans the whole body.
    let kept_arguments = std::cell::OnceCell::new();
    let mut producer: Option<u16> = None;
    // Per instruction: the general path executes it, and its record reads
    // the accumulator.
    let mut exits = Vec::with_capacity(function.code.len());
    let mut reads_accumulator = Vec::with_capacity(function.code.len());
    // Per instruction: the register its selected record carries as the
    // accumulator to the record after it.
    let mut produced = Vec::with_capacity(function.code.len());
    let mut records: Box<[LaneInstruction]> = function
        .code
        .iter()
        .enumerate()
        .map(|(pc, _)| {
            let instruction = decoded[pc];
            let op = instruction.op();
            let binds_memory =
                op == Op::WasmInstanceBinding && Some(instruction.a()) == memory_register;
            let memory_access = MemoryLoad::from_direct_op(op).is_some()
                || MemoryStore::from_direct_op(op).is_some();
            let keeps_view = if memory_access {
                Some(instruction.b()) == memory_register
            } else {
                binds_memory
                    || memory_register.is_none_or(|register| !lane_writes(instruction, register))
            };
            let operand = lane_operand(instruction, pc, constants)
                .filter(|_| {
                    keeps_view && lane_fields_fit(instruction, function) && lane_call_fits(instruction)
                })
                .map(|operand| lane_call_result_marked(operand, instruction, function, pc));
            // Control reaches this record only from the previous one when no
            // jump targets it; the general loop never enters at a record that
            // reads the accumulator.
            let carried = producer.take().filter(|_| ACCUMULATOR && !jump_targets[pc]);
            let acc = carried.map_or(ACC_NONE, |register| {
                accumulator_field(instruction, register)
            });
            // A table is backward when one of its entries jumps back.
            let backward_jump = if op == Op::WasmBranchTable {
                    crate::bytecode::branch_table_entries(pc, instruction).any(|entry| {
                        decoded[entry].imm() as usize <= pc
                    })
                } else {
                    op.immediate_role() == ImmediateRole::JumpTarget
                        && instruction.imm() as usize <= pc
                };
            let selected = operand.and(if binds_memory {
                Some(LaneHandler {
                    handler: lane_instance_binding::<H, true> as Handler,
                    produces: false,
                })
            } else if op == Op::WasmF64Unary
                && F64UnaryOperator::from_tag(instruction.imm())
                    == Some(F64UnaryOperator::SquareRoot)
            {
                tables[usize::from(backward_jump)][usize::from(acc)].get(op as usize).copied().flatten().and_then(|_| {
                    LaneHandler::producer(match acc {
                        ACC_NONE => lane_f64_sqrt::<H, ACC_NONE> as Handler,
                        ACC_A => lane_f64_sqrt::<H, ACC_A> as Handler,
                        ACC_B => lane_f64_sqrt::<H, ACC_B> as Handler,
                        ACC_C => lane_f64_sqrt::<H, ACC_C> as Handler,
                        _ => lane_f64_sqrt::<H, ACC_PAIR> as Handler,
                    })
                })
            } else if let Some(arith) = float_arithmetic(op, instruction.imm()) {
                tables[usize::from(backward_jump)][usize::from(acc)].get(op as usize).copied().flatten().and_then(|_| {
                    LaneHandler::producer(
                        FloatArithmetic::<H>::HANDLERS[usize::from(op == Op::WasmF64Binary)]
                            [usize::from(arith)][usize::from(acc)],
                    )
                })
            } else if matches!(op, Op::LoadLocal | Op::LoadLocalPlain | Op::ArgumentsLength | Op::ArgumentGet)
                && function.arguments_slot == u16::try_from(instruction.local_slot()).ok()
                && let Some(kept) = *kept_arguments.get_or_init(|| function.kept_arguments())
            {
                let forwards = decoded.get(pc + 1).is_some_and(|next| {
                    next.op() == Op::CallForwardingArguments
                        && next.call_window().last() == Some(instruction.result_register())
                });
                Some(LaneHandler {
                    handler: match op {
                        Op::ArgumentsLength => lane_arguments_length::<H> as Handler,
                        Op::ArgumentGet => lane_argument_get::<H> as Handler,
                        _ if kept == crate::bytecode::KeptArguments::Passes && !forwards => {
                            lane_load_kept_arguments_object::<H> as Handler
                        }
                        _ => lane_load_forwarded_arguments::<H> as Handler,
                    },
                    produces: false,
                })
            } else if matches!(op, Op::ArgumentsLength | Op::ArgumentGet) {
                Some(LaneHandler {
                    handler: if op == Op::ArgumentsLength {
                        lane_frame_arguments::<H, false> as Handler
                    } else {
                        lane_frame_arguments::<H, true> as Handler
                    },
                    produces: false,
                })
            } else if op == Op::Call && lane_iteration_step(&decoded, pc) {
                Some(LaneHandler {
                    handler: lane_iterator_step::<H> as Handler,
                    produces: false,
                })
            } else if op == Op::StoreEnvLocal && resolves_names {
                None
            } else if op == Op::GetField && lane_sinks_field(instruction) {
                Some(LaneHandler {
                    handler: lane_get_field_sink_first::<H> as Handler,
                    produces: false,
                })
            } else if let Some(kind) = returning_kind(instruction) {
                Some(LaneHandler {
                    handler: match kind {
                        RETURNING_FIELD => lane_returning_field_first::<H> as Handler,
                        RETURNING_UNARY => lane_returning::<H, RETURNING_UNARY> as Handler,
                        RETURNING_BINARY => lane_returning::<H, RETURNING_BINARY> as Handler,
                        RETURNING_OBJECT => lane_returning::<H, RETURNING_OBJECT> as Handler,
                        RETURNING_OBJECT_LITERAL => lane_returning::<H, RETURNING_OBJECT_LITERAL> as Handler,
                        _ => lane_returning::<H, RETURNING_LITERAL> as Handler,
                    },
                    produces: false,
                })
            } else if op == Op::Call && kind == crate::bytecode::ProgramKind::Wasm {
                // A Wasm run makes no frameless script call (`LaneContext::script_calls`), so
                // its import calls take the general path, and the JavaScript call handler,
                // which reads the context a JavaScript view carries, never runs in Wasm.
                None
            } else {
                tables[usize::from(backward_jump)][usize::from(acc)].get(op as usize).copied().flatten()
            });
            producer = selected
                .filter(|selected| selected.produces)
                .map(|_| instruction.a());
            produced.push(producer);
            exits.push(selected.is_none());
            reads_accumulator.push(selected.is_some() && acc != ACC_NONE);
            LaneInstruction {
                handler: selected.map_or(lane_exit::<H> as Handler, |selected| selected.handler)
                    as *const (),
                operand: operand.unwrap_or(LaneOperand { imm: 0 }),
                a: instruction.a(),
                b: instruction.b(),
                c: instruction.c(),
                call_link: std::cell::Cell::new(NO_CALL_LINK),
            }
        })
        .collect();
    // Each record's handler before any fusion: a record still holding it
    // runs only its own instruction.
    let selected: Vec<*const ()> = records.iter().map(|record| record.handler).collect();
    // Whether a record's handler serves `op` with accumulator field `acc`.
    let serves = |handler: *const (), op: Op, acc: u8| {
        tables[0][usize::from(acc)].get(op as usize).copied().flatten()
            .is_some_and(|served| handler == served.handler as *const ())
    };
    // The direction and accumulator field (among `accs`) of the plain handler serving `op`
    // that `handler` is, for an instruction whose handler depends on its jump's direction.
    let plain_served = |handler: *const (), op: Op, accs: &[u8]| {
        [false, true].into_iter().find_map(|backward| {
            accs.iter().copied().find(|&acc| {
                tables[usize::from(backward)][usize::from(acc)]
                    .get(op as usize)
                    .copied()
                    .flatten()
                    .is_some_and(|served| handler == served.handler as *const ())
            })
            .map(|acc| (backward, acc))
        })
    };
    if wasm {
        // A table follows its selected entry's jump, so every entry must run in
        // the lane.
        for (pc, &instruction) in decoded.iter().enumerate() {
            if instruction.op() == Op::WasmBranchTable
                && crate::bytecode::branch_table_entries(pc, instruction).any(|entry| exits[entry])
            {
                records[pc].handler = lane_exit::<H> as Handler as *const ();
                exits[pc] = true;
                reads_accumulator[pc] = false;
            }
        }
        // An add of a constant whose sum the next record loads from runs with
        // that load as one record: the load reads the sum as its carried
        // accumulator, so control reaches it only from the add.
        for pc in 0..records.len().saturating_sub(1) {
            let instruction = decoded[pc];
            let next = decoded[pc + 1];
            let plain_add = serves(records[pc].handler, Op::WasmI32AddImmediate, ACC_NONE);
            let carried_load = serves(records[pc + 1].handler, next.op(), ACC_C);
            if instruction.op() == Op::WasmI32AddImmediate
                && plain_add
                && carried_load
                && let Some(fused) = add_load_handler::<H>(next.op())
            {
                // A float operator taking the loaded f64 joins the fusion.
                let operator = (next.op() == Op::WasmF64Load && pc + 2 < records.len())
                    .then(|| decoded[pc + 2])
                    .filter(|third| third.op() == Op::WasmF64Binary)
                    .and_then(|third| {
                        let second = float_arithmetic(third.op(), third.imm())?;
                        let left = [ACC_B, ACC_C].into_iter().find(|acc| {
                            records[pc + 2].handler
                                == FloatArithmetic::<H>::HANDLERS[1][usize::from(second)]
                                    [usize::from(*acc)] as *const ()
                        })? == ACC_B;
                        Some(AddLoadF64::<H>::HANDLERS[usize::from(second)][usize::from(left)])
                    });
                records[pc].handler = operator.unwrap_or(fused) as *const ();
            }
        }
        // An i32 load whose value the next record's branch table selects on
        // runs with that table as one record.
        for pc in 0..records.len().saturating_sub(1) {
            let instruction = decoded[pc];
            let next = decoded[pc + 1];
            let field = [ACC_NONE, ACC_C]
                .into_iter()
                .find(|acc| serves(records[pc].handler, instruction.op(), *acc));
            if let Some(field) = field
                && next.a() == instruction.a()
                && serves(records[pc + 1].handler, Op::WasmBranchTable, ACC_NONE)
                && let Some(fused) = load_table_handler::<H>(instruction.op(), field, false)
            {
                records[pc].handler = fused as *const ();
                // A plain add of a constant ahead of a load reading its own
                // field joins as well.
                if pc > 0
                    && field == ACC_NONE
                    && serves(records[pc - 1].handler, Op::WasmI32AddImmediate, ACC_NONE)
                    && let Some(fused) = load_table_handler::<H>(instruction.op(), ACC_NONE, true)
                {
                    records[pc - 1].handler = fused as *const ();
                }
            }
        }
        // A shift left by a constant whose result the next record adds runs
        // with that add as one record.
        for pc in 0..records.len().saturating_sub(1) {
            let variant = |op: Op, acc: u8, at: usize| serves(records[at].handler, op, acc);
            let field = [ACC_NONE, ACC_B]
                .into_iter()
                .position(|acc| variant(Op::WasmI32ShiftLeftImmediate, acc, pc));
            let left = [ACC_C, ACC_B]
                .into_iter()
                .position(|acc| variant(Op::WasmI32Add, acc, pc + 1));
            if let (Some(field), Some(left)) = (field, left) {
                records[pc].handler = ScaledIndex::<H>::HANDLERS[field][left] as *const ();
            }
        }
        // An i32 load, the operator taking the loaded value and the store of
        // its result run as one record.
        for pc in 0..records.len().saturating_sub(2) {
            let load = decoded[pc];
            let combine = decoded[pc + 1];
            let store = decoded[pc + 2];
            if load.op() != Op::WasmI32Load || store.op() != Op::WasmI32Store {
                continue;
            }
            let field = [ACC_NONE, ACC_C]
                .into_iter()
                .find(|&acc| serves(records[pc].handler, Op::WasmI32Load, acc));
            let left = [ACC_C, ACC_B]
                .into_iter()
                .position(|acc| serves(records[pc + 1].handler, combine.op(), acc));
            if let (Some(field), Some(left)) = (field, left)
                && store.a() == combine.a()
                && serves(records[pc + 2].handler, Op::WasmI32Store, ACC_A)
                && let Some(fused) = i32_update_handler::<H>(combine.op(), left == 1, field)
            {
                records[pc].handler = fused as *const ();
            }
        }
        // A constant that the next record stores runs with that store as one
        // record.
        for pc in 0..records.len().saturating_sub(1) {
            let next = decoded[pc + 1];
            if serves(records[pc].handler, Op::LoadConst, ACC_NONE)
                && serves(records[pc + 1].handler, next.op(), ACC_A)
                && let Some(fused) = constant_store_handler::<H>(next.op())
            {
                records[pc].handler = fused as *const ();
            }
        }
        // A load whose value the next record stores runs with that store as
        // one record.
        for pc in 0..records.len().saturating_sub(1) {
            let instruction = decoded[pc];
            let next = decoded[pc + 1];
            let field = [ACC_NONE, ACC_C]
                .into_iter()
                .find(|acc| serves(records[pc].handler, instruction.op(), *acc));
            let carried_store = serves(records[pc + 1].handler, next.op(), ACC_A);
            if let Some(field) = field
                && carried_store
                && let Some(fused) = load_store_handler::<H>(instruction.op(), next.op(), field)
            {
                records[pc].handler = fused as *const ();
            }
        }
    }
    // The zero-test jump `handler` is, as (taken on zero, backward), when it carries no
    // register and reads its operand plainly or from the accumulator.
    let plain_zero_jump = |handler: *const ()| {
        let plain = ZeroJumps::<H, false>::HANDLERS;
        (0..2).flat_map(|zero| (0..2).map(move |backward| (zero, backward))).find(|&(zero, backward)| {
            [ACC_NONE, ACC_A]
                .into_iter()
                .any(|acc| handler == plain[zero][backward][usize::from(acc)] as *const ())
        })
    };
    // Each record's i32 load with its address field, if it is a plain one.
    let i32_load = |records: &[LaneInstruction], pc: usize| {
        let op = decoded[pc].op();
        [ACC_NONE, ACC_C].into_iter().find(|&acc| serves(records[pc].handler, op, acc)).filter(|_| {
            matches!(
                op,
                Op::WasmI32Load | Op::WasmI32Load8S | Op::WasmI32Load8U | Op::WasmI32Load16S | Op::WasmI32Load16U
            )
        })
    };
    if wasm {
        // An i32 add or subtract of a constant, the and of a constant over its result and an
        // immediate comparison jump testing that run as one record.
        for pc in 0..records.len().saturating_sub(2) {
            let (first, mask, jump) = (decoded[pc], decoded[pc + 1], decoded[pc + 2]);
            let Some(acc) = [ACC_NONE, ACC_B].into_iter().find(|&acc| serves(records[pc].handler, first.op(), acc))
            else {
                continue;
            };
            if !matches!(first.op(), Op::WasmI32AddImmediate | Op::WasmI32SubtractImmediate)
                || mask.op() != Op::WasmI32AndImmediate
                || mask.b() != first.a()
                || jump.a() != mask.a()
                || ![ACC_NONE, ACC_B].into_iter().any(|acc| serves(records[pc + 1].handler, Op::WasmI32AndImmediate, acc))
            {
                continue;
            }
            let Some((backward, _)) = plain_served(records[pc + 2].handler, jump.op(), &[ACC_NONE, ACC_A])
            else {
                continue;
            };
            if let Some(fused) = offset_mask_jump_handler::<H>(first.op(), jump.op(), backward, acc) {
                records[pc].handler = fused as *const ();
            }
        }
        // An i32 operator of a constant and a register comparison jump reading its result run as
        // one record.
        for pc in 0..records.len().saturating_sub(1) {
            let (first, jump) = (decoded[pc], decoded[pc + 1]);
            if !serves(records[pc].handler, first.op(), ACC_NONE)
                || I32BinaryOperator::from_jump_op(jump.op()).is_none()
                || (jump.a() != first.a() && jump.b() != first.a())
            {
                continue;
            }
            let Some((backward, _)) = plain_served(records[pc + 1].handler, jump.op(), &[ACC_NONE, ACC_A, ACC_B])
            else {
                continue;
            };
            if let Some(fused) = immediate_compare_jump_handler::<H>(first.op(), jump.op(), backward) {
                records[pc].handler = fused as *const ();
            }
        }
        // An i32 load whose value the next record's i32 load takes as its address runs with it as
        // one record; so does an i32 load whose value the next record's zero-test jump tests.
        for pc in 0..records.len().saturating_sub(1) {
            let Some(acc) = i32_load(&records, pc) else {
                continue;
            };
            let (load, next) = (decoded[pc], decoded[pc + 1]);
            let fused = if load.op() == Op::WasmI32Load
                && i32_load(&records, pc + 1).is_some()
                && next.c() == load.a()
            {
                load_load_handler::<H>(next.op(), acc)
            } else if let Some((zero, backward)) = plain_zero_jump(records[pc + 1].handler)
                && next.a() == load.a()
            {
                load_jump_handler::<H>(load.op(), acc, zero == 1, backward == 1)
            } else {
                None
            };
            if let Some(fused) = fused {
                records[pc].handler = fused as *const ();
            }
        }
        // Two plain i32 loads whose values the next record's i32 operator combines run with it
        // as one record.
        for pc in 0..records.len().saturating_sub(2) {
            let (first, second, combine) = (decoded[pc], decoded[pc + 1], decoded[pc + 2]);
            if first.op() != second.op()
                || i32_load(&records, pc) != Some(ACC_NONE)
                || i32_load(&records, pc + 1) != Some(ACC_NONE)
                || first.a() == second.a()
                || second.c() == first.a()
                || ![ACC_NONE, ACC_B, ACC_C].into_iter().any(|acc| serves(records[pc + 2].handler, combine.op(), acc))
            {
                continue;
            }
            let first_left = if (combine.b(), combine.c()) == (first.a(), second.a()) {
                true
            } else if (combine.b(), combine.c()) == (second.a(), first.a()) {
                false
            } else {
                continue;
            };
            if let Some(fused) = load_pair_binary_handler::<H>(first.op(), combine.op(), first_left) {
                records[pc].handler = fused as *const ();
            }
        }
        // An i32 operator, a constant load and a select reading them run as one record.
        for pc in 0..records.len().saturating_sub(2) {
            let (operator, constant) = (decoded[pc], decoded[pc + 1]);
            let Some(acc) = [ACC_NONE, ACC_B, ACC_C].into_iter().find(|&acc| serves(records[pc].handler, operator.op(), acc))
            else {
                continue;
            };
            if !serves(records[pc + 1].handler, Op::LoadConst, ACC_NONE)
                || ![ACC_NONE, ACC_B, ACC_C, ACC_PAIR]
                    .into_iter()
                    .any(|acc| serves(records[pc + 2].handler, Op::WasmSelect, acc))
                || constant.op() != Op::LoadConst
            {
                continue;
            }
            if let Some(fused) = operator_constant_select_handler::<H>(operator.op(), acc) {
                records[pc].handler = fused as *const ();
            }
        }
        // A plain move whose value the next record's i32 load takes as its address runs with it
        // as one record.
        for pc in 0..records.len().saturating_sub(1) {
            if serves(records[pc].handler, Op::Move, ACC_NONE)
                && i32_load(&records, pc + 1).is_some()
                && decoded[pc + 1].c() == decoded[pc].a()
                && let Some(fused) = move_load_handler::<H>(decoded[pc + 1].op(), ACC_NONE)
            {
                records[pc].handler = fused as *const ();
            }
        }
        // A plain i32 store followed by a move runs with it as one record.
        for pc in 0..records.len().saturating_sub(1) {
            let op = decoded[pc].op();
            let Some(acc) = [ACC_NONE, ACC_A, ACC_C].into_iter().find(|&acc| serves(records[pc].handler, op, acc)) else {
                continue;
            };
            if [ACC_NONE, ACC_B].into_iter().any(|acc| serves(records[pc + 1].handler, Op::Move, acc))
                && let Some(fused) = store_move_handler::<H>(op, acc)
            {
                records[pc].handler = fused as *const ();
            }
        }
    }
    // A record's plain jump, with its direction.
    let jump_direction = |handler: *const ()| {
        [false, true].into_iter().find(|&backward| {
            tables[usize::from(backward)][usize::from(ACC_NONE)][Op::Jump as usize]
                .is_some_and(|jump| handler == jump.handler as *const ())
        })
    };
    // A jump whose target is a plain move or add of a small constant runs
    // that instruction itself and jumps past it: the target's successor
    // then sees the accumulator the target would have carried. A table
    // entry stays a plain jump, since tables take the entry's jump without
    // running its record. This runs before the fusions below, which take a
    // following jump's record as a plain jump.
    let mut entry_jumps = vec![false; records.len()];
    for (pc, &instruction) in decoded.iter().enumerate() {
        if instruction.op() == Op::WasmBranchTable {
            for entry in crate::bytecode::branch_table_entries(pc, instruction) {
                entry_jumps[entry] = true;
            }
        }
    }
    let record_bytes = size_of::<LaneInstruction>() as isize;
    for (pc, &instruction) in decoded.iter().enumerate() {
        let target = instruction.imm() as usize;
        if instruction.op() != Op::Jump
            || jump_direction(records[pc].handler).is_none()
            || entry_jumps[pc]
            || target + 1 >= records.len()
        {
            continue;
        }
        let absorbed = decoded[target];
        let plain = |op: Op| absorbed.op() == op && serves(records[target].handler, op, ACC_NONE);
        let backward = target <= pc;
        // In compiled mode an OSR site's backedge must remain a standalone jump record.
        // The lane's `THROUGH` fusion would replace both its handler and jump distance.
        if loop_tier && backward {
            continue;
        }
        let (handler, c) = if plain(Op::Move) {
            (JumpFusions::<H>::THROUGH[0][usize::from(backward)], 0)
        } else if plain(Op::WasmI32AddImmediate)
            && let Ok(step) = i16::try_from(absorbed.imm() as i32)
        {
            (
                JumpFusions::<H>::THROUGH[1][usize::from(backward)],
                step as u16,
            )
        } else {
            continue;
        };
        records[pc] = LaneInstruction {
            handler: handler as *const (),
            operand: LaneOperand {
                jump: (target as isize + 1 - pc as isize) * record_bytes,
            },
            a: absorbed.a(),
            b: absorbed.b(),
            c,
            call_link: std::cell::Cell::new(NO_CALL_LINK),
        };
    }
    // Two plain moves in a row run as one record.
    let mut pc = 0;
    while pc + 1 < records.len() {
        if serves(records[pc].handler, Op::Move, ACC_NONE)
            && serves(records[pc + 1].handler, Op::Move, ACC_NONE)
        {
            // A jump after the pair joins it.
            let jump = records
                .get(pc + 2)
                .and_then(|record| jump_direction(record.handler))
                .filter(|backward| !loop_tier || !backward);
            records[pc].handler = match jump {
                None => lane_move_pair::<H, false, false> as Handler,
                Some(false) => lane_move_pair::<H, true, false>,
                Some(true) => lane_move_pair::<H, true, true>,
            } as *const ();
            pc += 2;
        } else {
            pc += 1;
        }
    }
    // A plain move or add of a constant whose next record jumps runs with
    // that jump as one record.
    for pc in 0..records.len().saturating_sub(1) {
        let plain = |op: Op| serves(records[pc].handler, op, ACC_NONE);
        let add = if plain(Op::Move) {
            false
        } else if plain(Op::WasmI32AddImmediate) {
            true
        } else {
            continue;
        };
        if let Some(backward) = jump_direction(records[pc + 1].handler) {
            // Leave a hot OSR latch's jump record visible to `install_loop_sites`.
            if loop_tier && backward {
                continue;
            }
            records[pc].handler =
                JumpFusions::<H>::THEN[usize::from(add)][usize::from(backward)] as *const ();
        }
    }
    if wasm {
        // A move whose next record is a zero-test jump that carries no register runs with that
        // jump as one record.
        for pc in 0..records.len().saturating_sub(1) {
            let Some(move_acc) = [ACC_NONE, ACC_B]
                .into_iter()
                .position(|acc| serves(records[pc].handler, Op::Move, acc))
            else {
                continue;
            };
            if let Some((zero, backward)) = plain_zero_jump(records[pc + 1].handler) {
                records[pc].handler = MoveZeroJumps::<H>::HANDLERS[zero][backward][move_acc] as *const ();
            }
        }
    }
    if wasm {
        // An f64 arithmetic record whose result the next f64 operator takes
        // through the accumulator runs with it as one record; pairs tile a
        // chain from its start.
        let mut pc = 0;
        while pc + 1 < records.len() {
            let instruction = decoded[pc];
            let next = decoded[pc + 1];
            let pair = (|| {
                if next.op() != Op::WasmF64Binary {
                    return None;
                }
                let second = float_arithmetic(next.op(), next.imm())?;
                let left_of = |records: &[LaneInstruction]| {
                    [ACC_B, ACC_C]
                        .into_iter()
                        .find(|acc| {
                            records[pc + 1].handler
                                == FloatArithmetic::<H>::HANDLERS[1][usize::from(second)]
                                    [usize::from(*acc)] as *const ()
                        })
                        .map(|acc| acc == ACC_B)
                };
                if instruction.op() == Op::WasmF64Load {
                    let plain = tables[0][usize::from(ACC_NONE)][Op::WasmF64Load as usize]?;
                    if records[pc].handler != plain.handler as *const () {
                        return None;
                    }
                    let left = left_of(&records)?;
                    // An arithmetic whose result the record after it stores
                    // (`mem[x] += v`) joins as well.
                    let updates =
                        pc + 2 < records.len() && usize::from(second) < FLOAT_PAIR_FIRSTS && {
                            let store = decoded[pc + 2];
                            store.op() == Op::WasmF64Store
                                && store.a() == next.a()
                                && [ACC_NONE, ACC_A]
                                    .into_iter()
                                    .any(|acc| serves(records[pc + 2].handler, Op::WasmF64Store, acc))
                        };
                    if updates {
                        return Some((
                            FloatPairs::<H>::UPDATES[usize::from(second)][usize::from(left)],
                            3,
                        ));
                    }
                    return Some((
                        FloatPairs::<H>::LOADS[usize::from(second)][usize::from(left)],
                        2,
                    ));
                }
                if instruction.op() != Op::WasmF64Binary {
                    return None;
                }
                let first = float_arithmetic(instruction.op(), instruction.imm())?;
                if usize::from(first) >= FLOAT_PAIR_FIRSTS {
                    return None;
                }
                let field = FUSED_ACC_FIELDS.iter().position(|acc| {
                    records[pc].handler
                        == FloatArithmetic::<H>::HANDLERS[1][usize::from(first)][usize::from(*acc)]
                            as *const ()
                })?;
                let left = left_of(&records)?;
                Some((
                    FloatPairs::<H>::HANDLERS[usize::from(first)][usize::from(second)]
                        [usize::from(left)][field],
                    2,
                ))
            })();
            // An f64 arithmetic whose result the next record stores, when only
            // the arithmetic reaches that store.
            let pair = pair.or_else(|| {
                let first = float_arithmetic(instruction.op(), instruction.imm())?;
                // The fused record writes the arithmetic's result itself, so the
                // store may read it plainly or through the accumulator.
                let store_runs = [ACC_NONE, ACC_A]
                    .into_iter()
                    .any(|acc| serves(records[pc + 1].handler, Op::WasmF64Store, acc));
                if instruction.op() != Op::WasmF64Binary
                    || usize::from(first) >= FLOAT_PAIR_FIRSTS
                    || next.op() != Op::WasmF64Store
                    || next.a() != instruction.a()
                    || jump_targets[pc + 1]
                    || !store_runs
                {
                    return None;
                }
                let field = FUSED_ACC_FIELDS.iter().position(|acc| {
                    records[pc].handler
                        == FloatArithmetic::<H>::HANDLERS[1][usize::from(first)][usize::from(*acc)]
                            as *const ()
                })?;
                Some((FloatPairs::<H>::STORES[usize::from(first)][field], 2))
            });
            match pair {
                Some((fused, span)) => {
                    records[pc].handler = fused as *const ();
                    pc += span;
                }
                None => pc += 1,
            }
        }
        // A dispatch head (an add of a constant fused with the load and table
        // after it) reads the register both take from the accumulator when every
        // edge into it carries that register there, as an interpreter's latches
        // carry its program counter.
        let unfused = |records: &[LaneInstruction], from: usize, to: usize| {
            (from..=to).all(|pc| records[pc].handler == selected[pc])
        };
        // The register the record at `pc` carries as the accumulator when it
        // falls through, if it runs only itself and nothing fused runs into it.
        let carried_through = |records: &[LaneInstruction], pc: usize| {
            unfused(records, pc.saturating_sub(FUSION_REACH), pc)
                .then_some(produced[pc])
                .flatten()
        };
        // The register an edge into `target` carries, per edge; `None` when an
        // edge carries none (or one this analysis does not follow).
        // The carrying variant of the zero-test jump `handler`, if it is one.
        let carrying_zero_jump = |handler: *const ()| {
            let plain = ZeroJumps::<H, false>::HANDLERS
                .as_flattened()
                .as_flattened();
            let carrying = ZeroJumps::<H, true>::HANDLERS.as_flattened().as_flattened();
            plain
                .iter()
                .position(|&plain| handler == plain as *const ())
                .map(|index| carrying[index])
        };
        let edges_carry = |records: &[LaneInstruction], target: usize| -> Option<u16> {
            let mut carried: Option<u16> = None;
            let mut agree = |register: Option<u16>| match (register, carried) {
                (None, _) => false,
                (Some(register), None) => {
                    carried = Some(register);
                    true
                }
                (Some(register), Some(known)) => register == known,
            };
            if target > 0 && !exits[target - 1] {
                use crate::bytecode::ControlFlowLayout as Flow;
                match decoded[target - 1]
                    .op()
                    .control_flow_layout()
                {
                    Flow::Fallthrough => {
                        if !agree(carried_through(records, target - 1)) {
                            return None;
                        }
                    }
                    Flow::Jump | Flow::BranchTable | Flow::Terminal => {}
                    // A call's return or a branch not taken carries no register.
                    Flow::Call | Flow::ConditionalJump => return None,
                }
            }
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = decoded_instruction(function, *packed);
                if instruction.op().immediate_role() != ImmediateRole::JumpTarget {
                    continue;
                }
                let jump = instruction.imm() as usize;
                let through = JumpFusions::<H>::THROUGH
                    .iter()
                    .flatten()
                    .any(|&handler| records[pc].handler == handler as *const ());
                if through {
                    // The record runs its target and lands after it.
                    if jump + 1 == target
                        && !agree(Some(decoded[jump].a()))
                    {
                        return None;
                    }
                    continue;
                }
                if jump != target {
                    continue;
                }
                // A zero-test jump can carry any register (`ZeroJumps`).
                if carrying_zero_jump(records[pc].handler).is_some() {
                    continue;
                }
                if instruction.op() != Op::Jump || entry_jumps[pc] || jump_targets[pc] || pc == 0 {
                    return None;
                }
                let then = JumpFusions::<H>::THEN
                    .iter()
                    .flatten()
                    .any(|&handler| records[pc - 1].handler == handler as *const ());
                let pair = pc >= 2
                    && [true, false].into_iter().any(|backward| {
                        records[pc - 2].handler
                            == if backward {
                                lane_move_pair::<H, true, true> as Handler
                            } else {
                                lane_move_pair::<H, true, false> as Handler
                            } as *const ()
                    });
                // Entering a pair's second move runs that move's own record.
                if pair && !then && jump_targets[pc - 1] && carried_through(records, pc - 1).is_none() {
                    return None;
                }
                let register = if then || pair {
                    Some(decoded[pc - 1].a())
                } else if jump_direction(records[pc].handler).is_some() {
                    carried_through(records, pc - 1)
                } else {
                    None
                };
                if !agree(register) {
                    return None;
                }
            }
            carried
        };
        for pc in 0..records.len().saturating_sub(1) {
            let add = decoded[pc];
            let load = decoded[pc + 1];
            let Some(head) = load_table_handler::<H>(load.op(), ACC_NONE, true) else {
                continue;
            };
            if records[pc].handler != head as *const ()
                || load.c() != add.b()
                || add.a() == add.b()
                || edges_carry(&records, pc) != Some(add.b())
            {
                continue;
            }
            if let Some(carried) = load_table_handler::<H>(load.op(), ACC_C, true) {
                records[pc].handler = carried as *const ();
                reads_accumulator[pc] = true;
                for (jump, packed) in function.code.iter().enumerate() {
                    let instruction = decoded_instruction(function, *packed);
                    if instruction.op().immediate_role() == ImmediateRole::JumpTarget
                        && instruction.imm() as usize == pc
                        && let Some(carrying) = carrying_zero_jump(records[jump].handler)
                    {
                        records[jump].handler = carrying as *const ();
                        records[jump].c = add.b();
                    }
                }
            }
        }
        // A move-and-load head whose every incoming edge carries the moved register reads it from
        // the accumulator (a pointer walk carried around its loop); a latch of store, move and
        // zero-test jump on that register, right after the load, tests and carries the register
        // the load leaves there.
        for pc in 0..records.len().saturating_sub(1) {
            let (moving, load) = (decoded[pc], decoded[pc + 1]);
            let Some(head) = move_load_handler::<H>(load.op(), ACC_NONE) else {
                continue;
            };
            if !ACCUMULATOR
                || records[pc].handler != head as *const ()
                || moving.a() == moving.b()
                || edges_carry(&records, pc) != Some(moving.b())
            {
                continue;
            }
            let Some(carried) = move_load_handler::<H>(load.op(), ACC_B) else {
                continue;
            };
            records[pc].handler = carried as *const ();
            reads_accumulator[pc] = true;
            for (jump, packed) in function.code.iter().enumerate() {
                let instruction = decoded_instruction(function, *packed);
                if instruction.op().immediate_role() != ImmediateRole::JumpTarget
                    || instruction.imm() as usize != pc
                {
                    continue;
                }
                let Some(carrying) = carrying_zero_jump(records[jump].handler) else {
                    continue;
                };
                let latch = plain_zero_jump(records[jump].handler);
                records[jump].handler = carrying as *const ();
                records[jump].c = moving.b();
                // The loop's load, store, move and latch in a row: the load leaves the walked
                // register in the accumulator for the latch to test and carry.
                if let Some((zero, backward)) = latch
                    && jump == pc + 4
                    && instruction.a() == moving.b()
                    && load.a() == moving.b()
                    && !jump_targets[pc + 1]
                    && !jump_targets[pc + 2]
                    && !jump_targets[pc + 3]
                    && decoded[pc + 3].op() == Op::Move
                    && let Some(store_op) = Some(decoded[pc + 2].op())
                    && let Some(fused) = store_move_handler::<H>(store_op, ACC_NONE)
                    && records[pc + 2].handler == fused as *const ()
                    && let Some(latch) = store_move_jump_handler::<H>(store_op, zero == 1, backward == 1)
                {
                    records[pc + 2].handler = latch as *const ();
                    reads_accumulator[pc + 2] = true;
                }
            }
        }
        // A bit-field extraction and a following i32 operator run as one record.
        for pc in 0..records.len().saturating_sub(1) {
            let second = decoded[pc + 1];
            let Some(acc) = [ACC_NONE, ACC_B]
                .into_iter()
                .find(|&acc| serves(records[pc].handler, Op::WasmI32ShiftRightUnsignedAndImmediate, acc))
            else {
                continue;
            };
            if decoded[pc].op() != Op::WasmI32ShiftRightUnsignedAndImmediate
                || ![ACC_NONE, ACC_B, ACC_C].into_iter().any(|acc| serves(records[pc + 1].handler, second.op(), acc))
            {
                continue;
            }
            if let Some(fused) = bit_field_then_handler::<H>(second.op(), acc) {
                records[pc].handler = fused as *const ();
            }
        }
        // An immediate equality jump that falls through to a plain i32 load runs with it as one
        // record (which reads the address from its register, written whether or not the
        // accumulator also holds it).
        for pc in 0..records.len().saturating_sub(1) {
            let jump = decoded[pc];
            if !matches!(jump.op(), Op::WasmJumpI32EqualImmediate | Op::WasmJumpI32NotEqualImmediate)
                || i32_load(&records, pc + 1).is_none()
            {
                continue;
            }
            let Some((backward, acc)) = plain_served(records[pc].handler, jump.op(), &[ACC_NONE, ACC_A]) else {
                continue;
            };
            if let Some(fused) = compare_jump_load_handler::<H>(jump.op(), decoded[pc + 1].op(), backward, acc) {
                records[pc].handler = fused as *const ();
            }
        }
        // Two i32 adds, or an add and a counted loop's step and test, run as one record; other
        // fusions take their records first.
        for pc in 0..records.len().saturating_sub(1) {
            let (first, second) = (decoded[pc], decoded[pc + 1]);
            let Some(acc) = [ACC_NONE, ACC_B, ACC_C].into_iter().find(|&acc| serves(records[pc].handler, first.op(), acc))
            else {
                continue;
            };
            if !matches!(first.op(), Op::WasmI32Add | Op::WasmI32AddImmediate) {
                continue;
            }
            let Some((backward, _)) = plain_served(records[pc + 1].handler, second.op(), &[ACC_NONE, ACC_B, ACC_C])
            else {
                continue;
            };
            if let Some(fused) = add_pair_handler::<H>(first.op(), second.op(), backward, acc) {
                records[pc].handler = fused as *const ();
            }
        }
        // A fused record that runs a following zero-test jump itself carries its own result when
        // the jump is taken; where the jump now carries another register to a head reading it
        // (`carrying_zero_jump`), that record runs only its own instruction again.
        for jump in 1..records.len() {
            if !ZeroJumps::<H, true>::HANDLERS
                .as_flattened()
                .as_flattened()
                .iter()
                .any(|&carrying| records[jump].handler == carrying as *const ())
            {
                continue;
            }
            let carried = records[jump].c;
            let before = jump - 1;
            let move_jump = MoveZeroJumps::<H>::HANDLERS
                .as_flattened()
                .as_flattened()
                .iter()
                .any(|&fused| records[before].handler == fused as *const ());
            let load_jump = (0..2).any(|zero| {
                (0..2).any(|backward| {
                    [ACC_NONE, ACC_C].into_iter().any(|acc| {
                        load_jump_handler::<H>(decoded[before].op(), acc, zero == 1, backward == 1)
                            .is_some_and(|fused| records[before].handler == fused as *const ())
                    })
                })
            });
            if (move_jump || load_jump) && decoded[before].a() != carried {
                records[before].handler = selected[before];
            }
        }
    }
    specialize_register_operands::<H>(&decoded, kind, constants, &mut records);
    specialize_updates::<H>(&mut records);
    specialize_updated_index_reads::<H>(&mut records);
    specialize_negated_instanceof::<H>(&mut records);
    specialize_frame_locals::<H>(function, &mut records);
    specialize_outer_captures::<H>(&mut records);
    let environment_window = specialize_environment_window::<H>(function, &decoded, &mut records);
    let (loops, osr_rejections) = if loop_tier && kind != crate::bytecode::ProgramKind::Wasm {
        install_loop_sites::<H>(
            function_id,
            function,
            &decoded,
            constants,
            sites,
            live,
            general_dispatch,
            diagnostics,
            &mut records,
        )
    } else {
        (Box::default(), Vec::new())
    };
    // The fused records are script handlers; Wasm bodies run none.
    if !wasm {
        specialize_local_branch_pairs::<H>(&mut records);
        specialize_constant_returns::<H>(&mut records);
        specialize_script_calls::<H>(&mut records);
    }
    let min_run = if closes_loop(&decoded, &exits) {
        LANE_ENTRY_MIN_RUN
    } else {
        LANE_ENTRY_MIN_CALL_RUN
    };
    // A resumed JavaScript loop body's straight tail jumps back to its head, so its run
    // continues there; a Wasm body resumes nowhere, and following its jumps only made more
    // of its PCs entries, whose lane runs a `return_call` chain left at once (TOD-19:
    // `return_call_ref.wast` 510 -> 797 ms).
    let runs = lane_runs_ahead(&decoded, &exits, min_run, !wasm);
    let entries: Box<[bool]> = runs
        .iter()
        .zip(&reads_accumulator)
        .map(|(run, reads)| *run >= min_run && !reads)
        .collect();
    let activation = function
        .parameter_registers
        .filter(|_| function.locals == 0)
        .map(|parameters| ActivationShape {
            registers: function.registers,
            parameters,
            params: function.params,
            initial: function.initial_register.value(),
        });
    let field_records = decoded
        .iter()
        .enumerate()
        .filter(|&(pc, instruction)| {
            instruction.op() == Op::GetField && !loops.iter().any(|site| site.hookable && site.header as usize == pc)
        })
        .map(|(pc, _)| pc as u32)
        .collect();
    let records: Box<[std::cell::UnsafeCell<LaneInstruction>]> =
        records.into_iter().map(std::cell::UnsafeCell::new).collect();
    let first = std::cell::UnsafeCell::raw_get(records.as_ptr()).cast_const();
    LaneView {
        records,
        entries,
        memory_register,
        activation,
        target_body: std::cell::OnceCell::new(),
        environment_window,
        loops,
        start: std::cell::Cell::new(first),
        prologue: std::cell::UnsafeCell::new(LaneInstruction {
            handler: std::ptr::null(),
            operand: LaneOperand { imm: 0 },
            a: 0,
            b: 0,
            c: 0,
            call_link: std::cell::Cell::new(NO_CALL_LINK),
        }),
        tier_direct: std::cell::Cell::new(None),
        plain_backward_jump_sites: decoded
            .iter()
            .enumerate()
            .filter(|(pc, instruction)| instruction.op() == Op::Jump && instruction.jump_target() <= *pc as u32)
            .count(),
        osr_rejections: (!osr_rejections.is_empty())
            .then(|| osr_rejections.into_boxed_slice()),
        field_records,
    }
}

/// Whether an instruction's record leaves its `call_link` unread: any but a call's, whose
/// handlers keep their target hint there (`linked_script_call_entry`).
fn holds_no_call_link(op: Op) -> bool {
    !matches!(
        op,
        Op::Call
            | Op::CallArray
            | Op::CallForwardingArguments
            | Op::CallKnown
            | Op::CallMethod
            | Op::CallSpread
            | Op::CallThisMethod
            | Op::LoadNameCall
            | Op::Construct
            | Op::SuperCallCheck
    )
}

/// Gives each backward `Jump` whose loop the shared compiler admits (`analyze_osr_region`) the
/// record that counts it and enters its native code (`lane_loop_jump`). Runs after every
/// other pass, which read a jump record's distance from its operand.
fn install_loop_sites<'live, H: Host>(
    function_id: u32,
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[crate::bytecode::FieldSite],
    live: &dyn Fn() -> Option<&'live std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    general_dispatch: bool,
    diagnostics: bool,
    records: &mut [LaneInstruction],
) -> (Box<[LoopSite]>, Vec<(super::compiled::RegionRejection, usize)>) {
    let backward = lane_jump_always::<H, true> as Handler as *const ();
    // A fused record ahead of a jump runs that jump itself, reading its offset from the jump's
    // record (`JumpFusions::THEN`, a move pair with its jump): a site there would replace the
    // offset those records read with the site (the eslint npm scenario under --jit jumped to
    // a site address read as an offset).
    let fused_into = |pc: usize| {
        let then = pc >= 1
            && JumpFusions::<H>::THEN
                .iter()
                .flatten()
                .any(|&handler| records[pc - 1].handler == handler as *const ());
        let pair = pc >= 2
            && [lane_move_pair::<H, true, false> as Handler, lane_move_pair::<H, true, true>]
                .iter()
                .any(|&handler| records[pc - 2].handler == handler as *const ());
        then || pair
    };
    // The masks are derived for the first loop site only: a body without a backward jump
    // needs none, and every view under a loop tier had derived them (npm scenarios under
    // --jit: axios -3.6%, playwright -2.5% instructions).
    let masks = std::cell::OnceCell::new();
    let live = || *masks.get_or_init(live);
    let mut rejections = Vec::new();
    let sites: Vec<(usize, LoopSite)> = (0..records.len())
        .filter(|pc| records[*pc].handler == backward && decoded[*pc].op() == Op::Jump && !fused_into(*pc))
        .filter_map(|pc| {
            let header = decoded[pc].jump_target();
            let (region, promotion_only, analysis_failure, call_boundary_pcs) = match super::compiled::analyze_osr_region_detailed(
                function,
                decoded,
                constants,
                sites,
                live(),
                header,
                pc as u32,
            ) {
                Ok(region) => {
                    let call_boundary_pcs = if diagnostics { region.call_boundary_pcs() } else { Vec::new() };
                    (std::cell::OnceCell::from(region), false, None, call_boundary_pcs)
                }
                // A call may run inlined: the site makes its region once it is hot.
                Err(failure)
                    if failure.reason == super::compiled::RegionRejection::UnsupportedInstruction(Op::Call)
                        && live().is_some() =>
                {
                    (
                        std::cell::OnceCell::new(),
                        false,
                        Some(failure),
                        diagnostics.then(|| vec![failure.pc]).unwrap_or_default(),
                    )
                }
                Err(failure) => {
                    let reason = failure.reason;
                    if diagnostics {
                        if let Some((_, count)) =
                            rejections.iter_mut().find(|(existing, _)| *existing == reason)
                        {
                            *count += 1;
                        } else {
                            rejections.push((reason, 1));
                        }
                    }
                    if !general_dispatch {
                        if !diagnostics {
                            return None;
                        }
                        (std::cell::OnceCell::new(), false, Some(failure), Vec::new())
                    } else {
                        (std::cell::OnceCell::new(), true, Some(failure), Vec::new())
                    }
                }
            };
            Some((
                pc,
                LoopSite {
                    function: function_id,
                    promotion_only,
                    analysis_failure,
                    call_boundary_pcs: diagnostics.then(|| call_boundary_pcs.into_boxed_slice()),
                    region,
                    header,
                    backedge: pc as u32,
                    jump: records[pc].jump(),
                    header_handler: std::cell::Cell::new(std::ptr::null()),
                    hookable: holds_no_call_link(decoded[header as usize].op()),
                    taken: std::cell::Cell::new(0),
                    exits: std::cell::Cell::new(0),
                    declines: std::cell::Cell::new(0),
                    diagnostics,
                    lane_backedge_count: std::cell::Cell::new(0),
                    native_entry_attempts: std::cell::Cell::new(0),
                    native_region_runs: std::cell::Cell::new(0),
                    entry_guard_misses: std::cell::Cell::new(0),
                    native_exits: std::cell::Cell::new(0),
                    widenings: std::cell::Cell::new(0),
                    compile_attempts: std::cell::Cell::new(0),
                    compile_successes: std::cell::Cell::new(0),
                    tier: std::cell::RefCell::default(),
                },
            ))
        })
        .collect();
    let mut sites = sites;
    let headers: Vec<u32> = sites.iter().map(|(_, site)| site.header).collect();
    for (_, site) in &mut sites {
        site.hookable &= headers.iter().filter(|header| **header == site.header).count() == 1
            && u16::try_from(headers.len()).is_ok_and(|count| count < NO_CALL_LINK);
    }
    let pcs: Vec<usize> = sites.iter().map(|(pc, _)| *pc).collect();
    let sites: Box<[LoopSite]> = sites.into_iter().map(|(_, site)| site).collect();
    for (index, (pc, site)) in pcs.into_iter().zip(sites.iter()).enumerate() {
        records[pc].handler = lane_loop_jump::<H> as Handler as *const ();
        records[pc].operand = LaneOperand { site };
        if site.hookable {
            records[site.header as usize].call_link.set(index as u16);
        }
    }
    (sites, rejections)
}

/// Gives each lane record whose operands are registers the handler for that operand shape,
/// with the decoded registers in its fields, so it decodes no operand kinds: `Binary` of a
/// specialized operator (`operator_handler`), also with a constant right operand carried in
/// the record, `GetIndex` and `JumpBinaryFalse`, also with a small integer constant right
/// operand carried in the record.
fn specialize_register_operands<H: Host>(
    decoded: &[WideInstruction],
    kind: crate::bytecode::ProgramKind,
    constants: &[Value],
    records: &mut [LaneInstruction],
) {
    let binary = lane_binary::<H> as Handler as *const ();
    // A Wasm view's `GetIndex` (a multi-value result's read) keeps the general handler: the
    // register form reaches its context through a JavaScript view (`script_handler!`).
    let get_index = if kind == crate::bytecode::ProgramKind::Wasm {
        std::ptr::null()
    } else {
        lane_get_index::<H> as Handler as *const ()
    };
    let forward_test = lane_jump_binary_false::<H, false> as Handler as *const ();
    let backward_test = lane_jump_binary_false::<H, true> as Handler as *const ();
    let unary = lane_unary::<H> as Handler as *const ();
    for (pc, &instruction) in decoded.iter().enumerate() {
        let record = &mut records[pc];
        if record.handler == unary {
            if instruction.imm() == crate::bytecode::UNARY_TYPEOF_INDEX {
                record.handler = lane_typeof_index::<H> as Handler as *const ();
            }
            continue;
        }
        if ![binary, get_index, forward_test, backward_test].contains(&record.handler) {
            continue;
        }
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        use crate::bytecode::OperandKind::{Constant, Register};
        let (Some(Register), Some(right_kind)) = (left.kind(), right.kind()) else {
            continue;
        };
        if record.handler == binary {
            if instruction.numeric_local_target().is_some() {
                continue;
            }
            let operator = instruction.binary_operator();
            if operator == INSTANCEOF_OPERATOR && right_kind == Register {
                record.handler = lane_instanceof_value::<H> as Handler as *const ();
                record.b = left.payload();
                record.c = right.payload();
                continue;
            }
            let compares = operator <= super::operations::LAST_COMPARISON_OPERATOR;
            match right_kind {
                Register => {
                    let handler = if compares {
                        lane_relation_value::<H, false> as Handler
                    } else {
                        let Some(handler) = operator_handler::<H, false>(operator) else {
                            continue;
                        };
                        handler
                    };
                    record.handler = handler as *const ();
                    record.c = right.payload();
                }
                Constant => {
                    let Some(constant) = constants.get(usize::from(right.payload())) else {
                        continue;
                    };
                    let strict = super::operations::equality_operator(operator)
                        .is_some_and(|(strict, _)| strict);
                    let handler = if let Some(operator) = u16::try_from(operator).ok().filter(|_| compares) {
                        // The constant takes the immediate's place, so the operator moves to `c`.
                        record.c = operator;
                        if strict && constant.as_number().is_none() {
                            lane_strict_constant_value::<H> as Handler
                        } else {
                            lane_relation_value::<H, true> as Handler
                        }
                    } else {
                        let Some(handler) = operator_handler::<H, true>(operator) else {
                            continue;
                        };
                        handler
                    };
                    record.handler = handler as *const ();
                    record.operand = LaneOperand {
                        constant: *constant,
                    };
                }
                _ => continue,
            }
            record.b = left.payload();
            continue;
        }
        let backward = record.handler == backward_test;
        if record.handler != get_index
            && u32::from(instruction.a()) == INSTANCEOF_OPERATOR
            && right_kind == Register
        {
            let handler: Handler = if backward {
                lane_jump_instanceof_false::<H, true>
            } else {
                lane_jump_instanceof_false::<H, false>
            };
            record.handler = handler as *const ();
            record.b = left.payload();
            record.c = right.payload();
            continue;
        }
        let equality = (record.handler != get_index)
            .then(|| super::operations::equality_operator(u32::from(instruction.a())))
            .flatten();
        let constant = (right_kind == Constant)
            .then(|| constants.get(usize::from(right.payload())).copied())
            .flatten();
        let nullish = equality.zip(constant).and_then(|((strict, negated), constant)| {
            let test = match constant {
                Value::NULL | Value::UNDEFINED if !strict => NULLISH_LOOSE,
                Value::NULL => NULLISH_NULL,
                Value::UNDEFINED => NULLISH_UNDEFINED,
                _ => return None,
            };
            Some(match test {
                NULLISH_NULL => equality_test_handler!(lane_jump_nullish_false::<H, NULLISH_NULL>, backward, negated),
                NULLISH_UNDEFINED => {
                    equality_test_handler!(lane_jump_nullish_false::<H, NULLISH_UNDEFINED>, backward, negated)
                }
                _ => equality_test_handler!(lane_jump_nullish_false::<H, NULLISH_LOOSE>, backward, negated),
            })
        });
        let boolean = equality.zip(constant).and_then(|((strict, negated), constant)| match constant {
            Value::TRUE if strict => Some(equality_test_handler!(lane_jump_boolean_false::<H, true>, backward, negated)),
            Value::FALSE if strict => Some(equality_test_handler!(lane_jump_boolean_false::<H, false>, backward, negated)),
            _ => None,
        });
        if let Some(handler) = nullish.or(boolean) {
            record.handler = handler as *const ();
            record.b = left.payload();
            continue;
        }
        if right_kind == Constant
            && record.handler != get_index
            && super::operations::equality_operator(u32::from(instruction.a()))
                .is_some_and(|(strict, _)| strict)
            && constants
                .get(usize::from(right.payload()))
                .is_some_and(|constant| constant.is_heap())
        {
            let handler: Handler = if backward {
                lane_jump_strict_constant_false::<H, true>
            } else {
                lane_jump_strict_constant_false::<H, false>
            };
            record.handler = handler as *const ();
            record.b = left.payload();
            record.c = right.payload();
            continue;
        }
        if right_kind == Constant
            && record.handler != get_index
            && let Some(small) = constants
                .get(usize::from(right.payload()))
                .and_then(|constant| constant.as_int())
                .and_then(|value| i16::try_from(value).ok())
        {
            let handler = relation_jump_handler::<H, true>(u32::from(instruction.a()), backward);
            record.handler = handler as *const ();
            record.b = left.payload();
            record.c = small as u16;
            continue;
        }
        if right_kind != Register {
            continue;
        }
        let handler: Handler = if record.handler == get_index {
            lane_get_index_registers::<H>
        } else if let Some((true, negated)) = equality {
            equality_test_handler!(lane_jump_strict_equal_false::<H>, backward, negated)
        } else {
            relation_jump_handler::<H, false>(u32::from(instruction.a()), backward)
        };
        record.handler = handler as *const ();
        record.b = left.payload();
        record.c = right.payload();
    }
}

/// Coalesce a plain local load with the following register/register comparison branch. The
/// second record remains independently callable for branch-table and jump-target entry.
fn specialize_local_branch_pairs<H: Host>(records: &mut [LaneInstruction]) {
    let load = lane_load_local_plain::<H> as Handler as *const ();
    for pc in 0..records.len().saturating_sub(1) {
        if records[pc].handler != load {
            continue;
        }
        // A register jump test runs its operator's instantiation (`relation_jump_handler`),
        // and the fused record reads the operator from the branch record again.
        let branch = &records[pc + 1];
        let operator = u32::from(branch.a);
        let jumps = |backward| {
            branch.handler == relation_jump_handler::<H, false>(operator, backward) as *const ()
        };
        let fused: Handler = if jumps(false) {
            lane_load_local_jump_binary_false_registers::<H, false>
        } else if jumps(true) {
            lane_load_local_jump_binary_false_registers::<H, true>
        } else {
            continue;
        };
        records[pc].handler = fused as *const ();
    }
}

/// Runs each `LoadConst` whose register the next record, a `Return`, returns as one record
/// (`lane_return_constant`). The `Return` record stays for jumps that reach it, and the
/// fused record's general return continues there.
fn specialize_constant_returns<H: Host>(records: &mut [LaneInstruction]) {
    let load = lane_load_const::<H> as Handler as *const ();
    let ret = lane_return::<H> as Handler as *const ();
    for pc in 0..records.len().saturating_sub(1) {
        if records[pc].handler == load && records[pc + 1].handler == ret && records[pc + 1].a == records[pc].a {
            records[pc].handler = lane_return_constant::<H> as Handler as *const ();
        }
    }
}

/// Gives each `IncDec` record the handler of its direction, and runs each `ToNumeric` whose
/// result the next record, an `IncDec`, reads as one record (`lane_postfix_update`). The
/// `IncDec` record stays for jumps that reach it; it reads no accumulator, and neither does
/// its successor, since it produces none.
fn specialize_updates<H: Host>(records: &mut [LaneInstruction]) {
    let to_numeric = lane_to_numeric::<H> as Handler as *const ();
    let inc_dec = lane_inc_dec::<H, false> as Handler as *const ();
    for record in records.iter_mut() {
        if record.handler == inc_dec && record.imm() != 0 {
            record.handler = lane_inc_dec::<H, true> as Handler as *const ();
        }
    }
    for pc in 0..records.len().saturating_sub(1) {
        let (conversion, update) = (&records[pc], &records[pc + 1]);
        let decrement = update.handler == lane_inc_dec::<H, true> as Handler as *const ();
        if conversion.handler != to_numeric
            || (update.handler != inc_dec && !decrement)
            || update.b != conversion.a
        {
            continue;
        }
        let handler: Handler = if decrement {
            lane_postfix_update::<H, true>
        } else {
            lane_postfix_update::<H, false>
        };
        records[pc] = LaneInstruction {
            handler: handler as *const (),
            operand: LaneOperand { imm: 0 },
            a: conversion.a,
            b: conversion.b,
            c: update.a,
            call_link: std::cell::Cell::new(NO_CALL_LINK),
        };
    }
}

/// Gives each `IncDec` whose result the next record, a register `GetIndex`, reads as its
/// index the fused update and read (`lane_update_get_index`). The `GetIndex` record stays as
/// it is, for a jump that enters it and for a read the fused record leaves to it.
fn specialize_updated_index_reads<H: Host>(records: &mut [LaneInstruction]) {
    let read = lane_get_index_registers::<H> as Handler as *const ();
    let increment = lane_inc_dec::<H, false> as Handler as *const ();
    let decrement = lane_inc_dec::<H, true> as Handler as *const ();
    for pc in 0..records.len().saturating_sub(1) {
        let (update, next) = (&records[pc], &records[pc + 1]);
        if (update.handler != increment && update.handler != decrement)
            || next.handler != read
            || next.c != update.a
        {
            continue;
        }
        let handler: Handler = if update.handler == decrement {
            lane_update_get_index::<H, true>
        } else {
            lane_update_get_index::<H, false>
        };
        records[pc].handler = handler as *const ();
    }
}

/// An environment-window record's immediate: its own (a local slot or closure function, at
/// most `ENVIRONMENT_WINDOW_FIELD`) below, the function's register count above.
const ENVIRONMENT_WINDOW_SHIFT: u32 = u16::BITS;
const ENVIRONMENT_WINDOW_FIELD: u32 = (1 << ENVIRONMENT_WINDOW_SHIFT) - 1;

/// An environment-window record's own immediate and the window's register count.
#[inline(always)]
fn environment_window_fields(i: &LaneInstruction) -> (usize, u16) {
    let imm = i.imm();
    ((imm & ENVIRONMENT_WINDOW_FIELD) as usize, (imm >> ENVIRONMENT_WINDOW_SHIFT) as u16)
}

/// Gives a function whose locals an environment activation could hold
/// (`CallPlan::environment_activation`) the local and closure records that read either a
/// frame or such an activation's window (`lane_load_window_local`), each record's
/// immediate also carrying the register count (`environment_window_fields`); the count
/// when every such record took one. Each other record keeps its handler, and a function
/// any record of which cannot keeps all of them.
fn specialize_environment_window<H: Host>(
    function: &Function,
    decoded: &[WideInstruction],
    records: &mut [LaneInstruction],
) -> Option<EnvironmentWindow> {
    if function.locals == 0
        || u32::from(function.locals) > u64::BITS
        || (function.arguments_slot.is_some() && !function.reads_passed_arguments_only())
        || !function.local_registers.is_empty()
        || function.selective_capture_slots.is_none()
    {
        return None;
    }
    let registers = function.registers;
    // The handlers derivation gives these records alone: a record any other pass gave a
    // handler, such as one carrying an accumulator to the next, keeps the function's.
    let plain: [Handler; 11] = [
        lane_load_local::<H>,
        lane_load_local_plain::<H>,
        lane_load_frame_local::<H, true>,
        lane_load_frame_local::<H, false>,
        lane_store_local::<H>,
        lane_store_local_plain::<H>,
        lane_store_frame_local::<H, true>,
        lane_store_frame_local::<H, false>,
        lane_initialize_tdz::<H>,
        lane_make_closure::<H>,
        lane_exit::<H>,
    ];
    let mut replacements = Vec::new();
    for (pc, &instruction) in decoded.iter().enumerate() {
        let op = instruction.op();
        let own = match op {
            Op::MakeClosure => instruction.imm(),
            Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::StoreLocal | Op::StoreLocalPlain
            | Op::StoreEnvLocal | Op::InitializeTdz => instruction.local_slot() as u32,
            _ => continue,
        };
        if own > ENVIRONMENT_WINDOW_FIELD
            || !plain.iter().any(|handler| *handler as *const () == records[pc].handler)
        {
            return None;
        }
        let environment = op != Op::MakeClosure && function.local_slot_uses_environment(own as usize);
        let handler: Handler = match (op, environment) {
            (Op::MakeClosure, _) => lane_make_window_closure::<H>,
            (Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal, true) => lane_load_window_local::<H, true>,
            (Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal, false) => lane_load_window_local::<H, false>,
            (Op::InitializeTdz, true) => lane_initialize_window_tdz::<H, true>,
            (Op::InitializeTdz, false) => lane_initialize_window_tdz::<H, false>,
            (_, true) => lane_store_window_local::<H, true>,
            (_, false) => lane_store_window_local::<H, false>,
        };
        replacements.push((pc, handler, own | u32::from(registers) << ENVIRONMENT_WINDOW_SHIFT));
    }
    for (pc, handler, imm) in replacements {
        records[pc].handler = handler as *const ();
        records[pc].operand = LaneOperand { imm };
    }
    Some(EnvironmentWindow {
        locals: function.locals,
        captured: (0..usize::from(function.locals))
            .filter(|slot| function.local_slot_uses_environment(*slot))
            .fold(0, |mask, slot| mask | 1 << slot),
    })
}

/// Gives each `LoadCapture` record past the function's own scope the handler that reads
/// remembered walks (`lane_load_outer_capture`); depth-0 reads keep `lane_load_capture`.
fn specialize_outer_captures<H: Host>(records: &mut [LaneInstruction]) {
    let load = lane_load_capture::<H> as Handler as *const ();
    for record in records.iter_mut() {
        if record.handler == load && ImmediateLayout::capture_depth(record.imm()) != 0 {
            record.handler = lane_load_outer_capture::<H> as Handler as *const ();
        }
    }
}

/// Gives each frame `LoadLocal` and `StoreLocal` record the handler of its slot's static
/// binding facts (`lane_load_frame_local`): a slot a mapped `arguments` mirrors, or a self
/// binding a store must leave, keeps the general handler.
fn specialize_frame_locals<H: Host>(function: &Function, records: &mut [LaneInstruction]) {
    let load = lane_load_local::<H> as Handler as *const ();
    let store = lane_store_local::<H> as Handler as *const ();
    for record in records.iter_mut() {
        let slot = record.imm() as usize;
        if (record.handler != load && record.handler != store)
            || super::arguments::mapped_arguments_slot(function, slot).is_some()
        {
            continue;
        }
        let environment = function.local_slot_uses_environment(slot);
        let handler: Handler = match (record.handler == load, environment) {
            (true, true) => lane_load_frame_local::<H, true>,
            (true, false) => lane_load_frame_local::<H, false>,
            _ if function.is_self_binding_slot(slot) => continue,
            (false, true) => lane_store_frame_local::<H, true>,
            (false, false) => lane_store_frame_local::<H, false>,
        };
        record.handler = handler as *const ();
    }
}

/// Runs each `instanceof` value record whose result the next record, a `JumpTrue`, tests as
/// one record (`lane_instanceof_jump_true`), with the jump's distance taken from the fused
/// record. The `JumpTrue` record stays for jumps that reach it.
fn specialize_negated_instanceof<H: Host>(records: &mut [LaneInstruction]) {
    let instanceof = lane_instanceof_value::<H> as Handler as *const ();
    let forward = lane_jump_true::<H, false> as Handler as *const ();
    let backward = lane_jump_true::<H, true> as Handler as *const ();
    for pc in 0..records.len().saturating_sub(1) {
        let (test, jump) = (&records[pc], &records[pc + 1]);
        if test.handler != instanceof
            || (jump.handler != forward && jump.handler != backward)
            || jump.a != test.a
        {
            continue;
        }
        let handler: Handler = if jump.handler == backward {
            lane_instanceof_jump_true::<H, true>
        } else {
            lane_instanceof_jump_true::<H, false>
        };
        let distance = jump.jump() + size_of::<LaneInstruction>() as isize;
        records[pc].handler = handler as *const ();
        records[pc].operand = LaneOperand { jump: distance };
    }
}

/// The handler of a specialized `Binary` operator, with a register or (`CONSTANT`) a constant
/// right operand.
/// A `JumpBinaryFalse` integer test handler's operator parameter that names none: the
/// handler reads the operator from the record's `a` field (`relation_operator`).
const RECORD_RELATION: u32 = u32::MAX;

/// The operator of an integer jump test (`lane_jump_binary_false_registers`,
/// `lane_jump_binary_false_small`): `OP`, fixed when the view was derived
/// (`relation_jump_handler`), else the record's.
#[inline(always)]
fn relation_operator<const OP: u32>(i: &LaneInstruction) -> u32 {
    if OP == RECORD_RELATION { u32::from(i.a) } else { OP }
}

/// The integer jump test handler of `operator` (`SMALL`: against a small constant, else two
/// registers) that branches `backward` or not: a comparison's own instantiation, which
/// decides it with no dispatch on the operator, else the one reading the record's.
fn relation_jump_handler<H: Host, const SMALL: bool>(operator: u32, backward: bool) -> Handler {
    use oxc_ast::ast::BinaryOperator as B;
    macro_rules! handler {
        ($($operator:ident = $variant:ident),*) => {{
            $(const $operator: u32 = B::$variant as u32;)*
            macro_rules! pick {
                ($op:expr) => {
                    match (SMALL, backward) {
                        (true, true) => lane_jump_binary_false_small::<H, true, { $op }> as Handler,
                        (true, false) => lane_jump_binary_false_small::<H, false, { $op }> as Handler,
                        (false, true) => lane_jump_binary_false_registers::<H, true, { $op }> as Handler,
                        (false, false) => lane_jump_binary_false_registers::<H, false, { $op }> as Handler,
                    }
                };
            }
            match operator {
                $($operator => pick!($operator),)*
                _ => pick!(RECORD_RELATION),
            }
        }};
    }
    handler!(
        EQUALITY = Equality,
        INEQUALITY = Inequality,
        STRICT_EQUALITY = StrictEquality,
        STRICT_INEQUALITY = StrictInequality,
        LESS_THAN = LessThan,
        LESS_EQUAL = LessEqualThan,
        GREATER_THAN = GreaterThan,
        GREATER_EQUAL = GreaterEqualThan
    )
}

fn operator_handler<H: Host, const CONSTANT: bool>(operator: u32) -> Option<Handler> {
    use super::operations::{
        ADDITION, BITWISE_AND, BITWISE_OR, BITWISE_XOR, DIVISION, MULTIPLICATION, REMAINDER,
        SHIFT_LEFT, SHIFT_RIGHT, SHIFT_RIGHT_ZERO_FILL, SUBTRACTION,
    };
    macro_rules! handler {
        ($($operator:ident),*) => {
            match operator {
                $($operator => Some(if CONSTANT {
                    lane_operator_constant::<H, $operator> as Handler
                } else {
                    lane_operator::<H, $operator> as Handler
                }),)*
                _ => None,
            }
        };
    }
    handler!(
        ADDITION,
        SUBTRACTION,
        MULTIPLICATION,
        DIVISION,
        REMAINDER,
        SHIFT_LEFT,
        SHIFT_RIGHT,
        SHIFT_RIGHT_ZERO_FILL,
        BITWISE_OR,
        BITWISE_XOR,
        BITWISE_AND
    )
}

/// Whether some loop of `function`, from a backward jump's target through the jump, has no
/// instruction the lane exits at.
/// Whether one activation of `function` may run an instruction more than once: it has a
/// backward jump, or a branch table, whose entries may jump back.
fn may_loop(function: &Function) -> bool {
    function.code.iter().enumerate().any(|(pc, packed)| {
        let instruction = decoded_instruction(function, *packed);
        instruction.op() == Op::WasmBranchTable
            || (instruction.op().immediate_role() == ImmediateRole::JumpTarget
                && instruction.imm() as usize <= pc)
    })
}

/// How many lane records control reaches from each PC before one that exits, counted up
/// to `cap`: where `follows_jumps`, a `Jump` continues at its target, as a resumed loop body
/// does, and any other record at the next PC. Each pass extends every count by one step, so
/// `cap` passes reach the fixed point the capped counts allow; without jumps one pass does.
fn lane_runs_ahead(decoded: &[WideInstruction], exits: &[bool], cap: usize, follows_jumps: bool) -> Vec<usize> {
    let next = |pc: usize| match decoded[pc].op() {
        Op::Jump if follows_jumps => decoded[pc].imm() as usize,
        _ => pc + 1,
    };
    let mut runs = vec![0usize; exits.len()];
    for _ in 0..cap {
        let mut changed = false;
        for pc in (0..exits.len()).rev() {
            let run = if exits[pc] {
                0
            } else {
                (1 + runs.get(next(pc)).copied().unwrap_or(0)).min(cap)
            };
            changed |= run != runs[pc];
            runs[pc] = run;
        }
        if !changed {
            break;
        }
    }
    runs
}

fn closes_loop(decoded: &[WideInstruction], exits: &[bool]) -> bool {
    decoded.iter().enumerate().any(|(pc, &instruction)| {
        let target = instruction.imm() as usize;
        instruction.op().immediate_role() == ImmediateRole::JumpTarget
            && target <= pc
            && !exits[target..=pc].contains(&true)
    })
}

/// The register field of `instruction` that reads `register`, as an
/// accumulator position; `ACC_NONE` when no register field reads it, or more
/// than one does. A register pair's first register is the last resort.
fn accumulator_field(instruction: WideInstruction, register: u16) -> u8 {
    let mut fields = InstructionField::ALL.iter().copied().filter(|&field| {
        instruction.op().field_layout(field).reads_register()
            && instruction.field_value(field) == register
    });
    match (fields.next(), fields.next()) {
        (Some(field), None) => field.accumulator(),
        (None, _)
            if instruction.op().immediate_layout() == ImmediateLayout::RegisterPair
                && ImmediateLayout::register_pair(instruction.imm()).0 == register =>
        {
            ACC_PAIR
        }
        _ => ACC_NONE,
    }
}

/// The instructions some jump of `function` targets.
fn jump_targets(instructions: &[WideInstruction]) -> Vec<bool> {
    let mut targets = vec![false; instructions.len()];
    for instruction in instructions {
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget
            && let Some(target) = targets.get_mut(instruction.imm() as usize)
        {
            *target = true;
        }
    }
    targets
}

pub(super) fn decoded_instruction(
    function: &Function,
    packed: crate::bytecode::Instr,
) -> WideInstruction {
    if packed.is_wide() {
        function.wide[packed.wide_index()]
    } else {
        packed.as_wide()
    }
}

impl<H: Host> Vm<H> {
    /// The lane view a general-loop entry of `function` runs with, or null when it runs
    /// without one (`ProgramStore::entry_lane_view`).
    pub(super) fn entry_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView {
        let diagnostics = self.programs.reports_on_drop();
        // A Wasm function that may loop gets its view at entry; a JavaScript one derives it
        // once a loop turns hot (`Vm::note_back_edge`).
        let wasm = self.programs.residual(program).is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm);
        self.programs
            .entry_lane_view(program, function, |code| wasm && may_loop(code), |code, constants, kind, live, sites| {
                derive_lane_view::<H>(
                    function,
                    code,
                    constants,
                    kind,
                    live,
                    sites,
                    self.js_tier_runs(),
                    self.programs.js_jit_general_dispatch_enabled(),
                    diagnostics,
                )
            })
            .unwrap_or(std::ptr::null())
    }

    /// Whether a lane run of `program` credits its functions' loop work: a Wasm program in
    /// compiled execution without an eager artifact (selective tiering) promotes its functions
    /// one by one, by calls and by loop work.
    fn lane_credits_backedges(&self, code: &ResidualProgram, program: ProgramId) -> bool {
        code.kind == crate::bytecode::ProgramKind::Wasm
            && self.execution_mode == super::ExecutionMode::Compiled
            && matches!(self.programs.compiled(program), Some(None))
            && self.programs.wasm_module_selective(program)
    }

    /// Whether the instruction at `pc` of `view` runs in the lane.
    #[inline(always)]
    pub(super) fn lane_runs(view: *const LaneView, pc: usize) -> bool {
        // SAFETY: a non-null view has an entry for every validated PC.
        !view.is_null() && unsafe { *(&(*view).entries).get_unchecked(pc) }
    }

    /// Run lane instructions from `pc` in `frame` and return the PC of the
    /// first one that needs the general path; lane calls and returns update
    /// `frame` and `pending` as the general loop would. Kept out of line so
    /// the general loop's code generation, which serves JavaScript, does not
    /// depend on the lane.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_fast_lane(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        view: *const LaneView,
        frame: &mut usize,
        pc: usize,
        pending: &mut Vec<PendingGeneralCall>,
        executing: &mut super::dispatch_frame::ExecutingProgram,
        inline_calls: bool,
    ) -> usize {
        // SAFETY: the cursor's view outlives the lane run.
        let view = unsafe { &*view };
        let base = view.records();
        let registers = Registers(self.frames[*frame].registers.as_mut_ptr());
        // A run that credits loop work keeps the floor raised, so its backward jumps count in
        // `lane_safepoint` (`LaneContext::backedges_left`).
        let floor = if self.heap.should_collect() || self.lane_credits_backedges(code, program) {
            COLLECTION_DUE_FLOOR
        } else {
            stack_pointer().saturating_sub(LANE_STACK_SLACK)
        };
        // The floor lives in the VM's interrupt state, where a termination request raises it;
        // entry sets it before reading the request, the request raises it after setting
        // itself, so one of the two sees the other.
        let floor_cell: *const std::sync::atomic::AtomicUsize = self.execution_interrupt.lane_floor();
        let outer_cell = LANE_FLOOR.with(|current| current.replace(floor_cell));
        // SAFETY: the VM holds its interrupt state for the whole run.
        let outer_floor = unsafe { &*floor_cell }.swap(floor, std::sync::atomic::Ordering::SeqCst);
        if self.execution_interrupt.is_termination_requested() {
            unsafe { &*floor_cell }.store(COLLECTION_DUE_FLOOR, std::sync::atomic::Ordering::SeqCst);
        }
        self.lane_stack.refresh_activation_limit();
        self.lane_stack.enter_generation((self.heap.collection_epoch(), self.realm.globals));
        let mut context = LaneContext {
            vm: self,
            code,
            program,
            frame: *frame,
            registers,
            base,
            pending,
            executing,
            views: self
                .programs
                .lane_views(program)
                .map_or(&[][..], |views| views) as *const _,
            constants: self.programs.constants(program) as *const _,
            inline_calls: inline_calls
                && code.kind == crate::bytecode::ProgramKind::Wasm,
            memory: MemoryView::NONE,
            resolved: (Value::UNDEFINED, MemoryView::NONE),
            script_calls: inline_calls
                && code.kind != crate::bytecode::ProgramKind::Wasm
                && !self.direct_eval
                && !self.parameter_eval
                && *frame + 1 == self.frames.len()
                && self.frames[*frame].function != super::ROOT_FUNCTION_ID,
            script_constructs: code
                .function_get(self.frames[*frame].function as usize)
                .is_some_and(|function| {
                    function.dispatch == crate::bytecode::DispatchClass::General
                }),
            this: self.frame_lane_this(*frame),
            resumed_acc: Slot::of(Value::UNDEFINED),
            backedges_left: self
                .lane_credits_backedges(code, program)
                .then_some(super::program_store::WASM_JIT_BACKEDGES_PER_ENTRY),
            env: Value::DELETED,
            capture: std::ptr::null(),
            outer: NO_OUTER_WALK,
            frame_storage: NO_FRAME_STORAGE,
        };
        let cx: *mut LaneContext<H> = &mut context;
        let outer = LANE_CONTEXT.with(|current| current.replace(cx.cast()));
        // SAFETY: `cx` is the only access path to the context during the run.
        unsafe { (*cx).memory = frame_memory_view(cx, view, registers) };
        // SAFETY: the view has one record per instruction, and residual
        // validation establishes every reachable PC.
        let ip = lane_dispatch(cx, unsafe { base.add(pc) });
        #[cfg(feature = "profile-lane-exits")]
        if let Some(activation) = self.lane_stack.activations.last() {
            // SAFETY: the context's program outlives the run; `ip` is a record of
            // the innermost activation's view.
            let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
            if let Some(body) = code.function_get(activation.function as usize) {
                let instruction = body.instructions().nth(pc);
                super::lane_exits::record_detail(format!(
                    "exit in activation {:?} {} {:?} {} @{:?} pc={pc}",
                    instruction,
                    self.programs.source_name(unsafe { (*cx).program }).unwrap_or_default(),
                    instruction
                        .filter(|instruction| instruction.op() == Op::GetField)
                        .and_then(|instruction| code.atoms.get(instruction.imm() as usize)),
                    body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                    body.source_span.map(|span| span.start),
                ));
            }
        }
        self.materialize_lane_activations(cx);
        LANE_CONTEXT.with(|current| current.set(outer));
        // An enclosing run of this VM keeps a raised floor raised.
        // SAFETY: as above.
        let floor = unsafe { &*floor_cell };
        if self.execution_interrupt.is_termination_requested() {
            floor.store(COLLECTION_DUE_FLOOR, std::sync::atomic::Ordering::SeqCst);
        } else {
            floor.store(outer_floor, std::sync::atomic::Ordering::Relaxed);
        }
        LANE_FLOOR.with(|current| current.set(outer_cell));
        // SAFETY: as above.
        *frame = unsafe { (*cx).frame };
        // SAFETY: handlers only return records of the active view.
        unsafe { ip.offset_from((*cx).base) as usize }
    }

    /// `Vm::call_value` of `callee` from the native a lane called (`LaneNativeCall`), run as
    /// the lane's own call of it would run: an activation of that lane (`lane_script_target`,
    /// `lane_activate_keeping`) whose completion ends the run with its value
    /// (`Completion::ReturnsToNative`). Any other exit makes the activations frames, the
    /// callback's the entry frame of the general loop that finishes the call
    /// (`Vm::run_frames_after_lane_exit`), as a general call of it would have run it. `None`
    /// leaves the call to the general path: a native not running from the lane's frame, or a
    /// callee or call no lane call would activate.
    pub(super) fn lane_native_callback(
        &mut self,
        callee: Value,
        receiver: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        let site = self.lane_native?;
        let cx: *mut LaneContext<H> = site.cx.cast();
        // The native runs from the lane's frame, the innermost, with no activation above it:
        // `lane_call_native` made them frames, and only this run makes more.
        if site.frames != self.frames.len()
            || lane_context::<H>() != cx
            || !self.lane_stack.activations.is_empty()
            || self.lane_deferred.is_some()
            || self.with_scope_entered
        {
            return None;
        }
        // SAFETY: the lane's context lives while its native runs; nothing else accesses it.
        // A due collection runs on the general path, whose safepoints take it; the lane's
        // floor, set for the native's caller, bounds this run's stack as it bounds the lane's
        // own calls (`LANE_STACK_SLACK`).
        if !unsafe { (*cx).script_calls } || self.active_program != unsafe { (*cx).program } || self.heap.should_collect() {
            return None;
        }
        // Each nested run takes native stack, as the general loop's would (`Vm::enter_stack`),
        // whose exhaustion the general path raises.
        let _stack = crate::stack::StackGuard::enter().ok()?;
        // A collection since the lane entered retires the targets it resolved.
        self.lane_stack.enter_generation((self.heap.collection_epoch(), self.realm.globals));
        let target = lane_script_target(cx, callee)?;
        let this = lane_call_this(self, &target, receiver)?;
        let registers = self.active_registers;
        // SAFETY: as above; the context holds no drop glue. A completion to the native restores
        // the state of the native's caller (`lane_restore_caller`), so only an exit, which makes
        // frames, writes the copy back.
        let saved = unsafe { std::ptr::read(cx) };
        let (code, program) = (saved.code, saved.program);
        let mut pending = self.pending_call_stacks.pop().unwrap_or_default();
        let mut executing = super::dispatch_frame::ExecutingProgram { id: program, retained: None };
        unsafe {
            (*cx).pending = &mut pending;
            (*cx).executing = &mut executing;
        }
        let Some(start) = lane_push_callback(cx, saved.registers, site.call, &target, args, this) else {
            unsafe {
                (*cx).pending = saved.pending;
                (*cx).executing = saved.executing;
            }
            self.pending_call_stacks.push(pending);
            return None;
        };
        if let Some(ActivationKind::Script(script)) =
            self.lane_stack.activations.last_mut().map(|activation| &mut activation.kind)
        {
            script.completion = Completion::ReturnsToNative;
        }
        unsafe { (*cx).memory = MemoryView::script(cx) };
        let exit = lane_dispatch(cx, start);
        let outcome = match self.lane_native_returned.take() {
            Some(value) => {
                unsafe {
                    (*cx).pending = saved.pending;
                    (*cx).executing = saved.executing;
                }
                Ok(value)
            }
            None => {
                // SAFETY: handlers only return records of the active view.
                let pc = unsafe { exit.offset_from((*cx).base) } as usize;
                self.materialize_lane_activations(cx);
                let frame = unsafe { (*cx).frame };
                debug_assert!(frame >= site.frames, "the callback's activation became a frame");
                self.frames[frame].pc = pc;
                let raised = self.lane_raised.take();
                // SAFETY: as above.
                unsafe { std::ptr::write(cx, saved) };
                // SAFETY: the lane's program outlives its run.
                let code = unsafe { &*code };
                let exited = super::dispatch_frame::LaneExit { executing, raised };
                let result = self.run_frames_after_lane_exit(code, frame, &mut pending, exited);
                debug_assert_eq!(self.frames.len(), site.frames + 1, "the loop leaves the callback's frame");
                self.complete_user_frame(code, result, false).and_then(FrameOutcome::into_value)
            }
        };
        pending.clear();
        self.pending_call_stacks.push(pending);
        self.active_registers = registers;
        if self.active_program != program {
            self.set_active_program(program);
        }
        Some(outcome)
    }
}

/// Dispatch lane records from `ip` until a handler exits; the record it exits at.
#[inline(always)]
fn lane_dispatch<H: Host>(cx: *mut LaneContext<H>, mut ip: Ip) -> Ip {
    loop {
        // SAFETY: handlers have returned, so no other access is live.
        let (view, registers, acc) = unsafe {
            let acc = std::mem::replace(&mut (*cx).resumed_acc, Slot::of(Value::UNDEFINED));
            ((*cx).memory, (*cx).registers, acc)
        };
        let next = handler::<H>(ip)(
            registers,
            ip,
            view.bytes,
            view.reach,
            crate::value::IntegerEncoding::TAG,
            acc,
        );
        let ip_next = next.map_addr(|address| address & !EXIT_TAG);
        if next.addr() & EXIT_TAG != 0 {
            return ip_next;
        }
        ip = ip_next;
    }
}
