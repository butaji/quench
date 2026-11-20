//! Frameless calls between native bodies. A plain call of an admitted user
//! function calls the callee's native entry directly, its registers a window on this VM's
//! own native stack (`Frameless::windows`) and its activation a shadow record
//! (`ShadowRecord`), as the lane's direct calls run (`DirectCallee`). Nothing but native code
//! sees such an activation: the first exit to the VM from inside one (an exact step, an
//! inlined callee's recovery) makes the whole chain's frames through the shared recovery
//! (`materialize_chain`) and stops every native caller, and the general loop continues in
//! the innermost frame exactly where it left. A function whose activations keep leaving
//! within one collection cycle takes the framed call again until the next
//! (`MATERIALIZATION_LIMIT`), so warm-up exits do not disable it for good.
use super::{emit_js_payload_word, emit_lane_helper_call, JsStepDispatch};
#[cfg(feature = "native-execution")]
use super::{Host, JsNativeStep, ResidualProgram, Vm};
use crate::vm::compiled::regions;
use crate::Value;
use cranelift_codegen::ir::{self, types};
use super::object_slots::emit;
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use cranelift_frontend::FunctionBuilder;

#[path = "frameless/admission.rs"] mod admission;
pub(super) use admission::general_frameless_admit;

/// Values of the windows of all frameless activations one native entry may hold.
const WINDOW_VALUES: usize = 1 << 16;
const SHADOW_RECORDS: usize = 1 << 12;
const CACHE_ENTRIES: usize = 256;
use crate::vm::compiled::tier_policy::MATERIALIZATION_LIMIT;
/// `CallEntry::meta`'s fields above its function: the register count, then the parameter count.
const META_REGISTERS: u32 = u32::BITS;
const META_PARAMS: u32 = META_REGISTERS + u16::BITS;
/// `CallEntry::this`: the receiver as passed (a strict body), or no `this` slot (an arrow's
/// is lexical). Any other value is the global object a sloppy body binds for `undefined`
/// or `null`.
const THIS_RECEIVER: u64 = 0;
const THIS_NONE: u64 = 1;

/// A frameless activation: the call in its caller that made it, the `this` its body reads
/// (`JsStepDispatch::fixed_this_slot`) and its callee, whose function it runs. Its registers
/// are the window past the records below it, each as long as its function's register count
/// (`chain`), so the record holds neither.
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct ShadowRecord {
    pub(super) call_pc: u64,
    pub(super) this: Value,
    pub(super) callee: Value,
}

/// An admitted callee, by its value, in the collection epoch it was admitted in: its native
/// entry, its closure's dense capture slots and whether they take direct writes, its function
/// (low 32 bits), register count and parameter count (the next 16 bits each), and
/// how it binds `this`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct CallEntry {
    callee: u64,
    epoch: u64,
    code: u64,
    capture_values: u64,
    capture_len: u64,
    capture_stores: u64,
    meta: u64,
    this: u64,
    /// Whether `new` may construct with it in place (`Vm::inline_constructible`).
    constructs: u64,
}

/// The per-VM storage of frameless calls. The windows are never scanned: no collection runs
/// while one is live, since every exit from native code makes their frames first.
pub(crate) struct Frameless {
    /// The windows, shadow records and call cache, made when native code first binds them
    /// (`bind`): a VM that runs no native body keeps none of them, and one that does commits
    /// only the pages its activations reach (`zeroed_boxed_slice`), not their 0.6 MiB.
    stacks: std::sync::OnceLock<FramelessStacks>,
    /// Each function's last admission, by function index, with its program and closure
    /// environment: a fresh closure of it shares all but its callee and captures (`admit`).
    functions: Vec<(u32, Value, CallEntry)>,
    /// The leaves of each function's frameless activations in the epoch they count in
    /// (`MATERIALIZATION_LIMIT`).
    materializations: rustc_hash::FxHashMap<(u32, u32), (u32, u64)>,
}

impl Default for Frameless {
    fn default() -> Self {
        Self {
            stacks: std::sync::OnceLock::new(),
            functions: Vec::new(),
            materializations: rustc_hash::FxHashMap::default(),
        }
    }
}

/// `Frameless`'s storage, which native code addresses directly: its boxes never move once made.
struct FramelessStacks {
    windows: Box<[Value]>,
    shadow: Box<[ShadowRecord]>,
    cache: Box<[CallEntry]>,
}

impl FramelessStacks {
    fn new() -> Self {
        // SAFETY: each element type is a struct of `u64` words (`Value` wraps one), for which
        // all-zero bytes are a valid value. A window is written before it is read
        // (`fill_window`), a shadow record is read only below the top a push advanced past it,
        // and an empty call entry is `CallEntry::default()`.
        unsafe {
            Self {
                windows: zeroed_boxed_slice(WINDOW_VALUES),
                shadow: zeroed_boxed_slice(SHADOW_RECORDS),
                cache: zeroed_boxed_slice(CACHE_ENTRIES),
            }
        }
    }
}

/// A boxed slice of `len` zeroed elements. The allocation is zeroed by the allocator, which
/// backs a large one with fresh pages it commits by first touch: the stack pays resident
/// memory only for the entries a run reaches, not for all of them at binding.
///
/// # Safety
/// All-zero bytes are a valid `T`.
unsafe fn zeroed_boxed_slice<T>(len: usize) -> Box<[T]> {
    let layout = std::alloc::Layout::array::<T>(len).expect("frameless stack layout");
    // SAFETY: the layout is nonzero-sized, as every stack holds entries.
    let base = unsafe { std::alloc::alloc_zeroed(layout) }.cast::<T>();
    if base.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    // SAFETY: `len` zeroed elements the box owns, freed by `Box`'s deallocation with the same
    // layout; zeroed bytes are valid per the caller's contract.
    unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(base, len)) }
}

impl Frameless {
    fn stacks(&self) -> &FramelessStacks {
        self.stacks.get_or_init(FramelessStacks::new)
    }

    fn cache_mut(&mut self) -> &mut [CallEntry] {
        self.stacks();
        &mut self.stacks.get_mut().expect("made above").cache
    }

    /// Binds a general entry's payload to empty stacks: a native entry starts with no
    /// frameless activation live (`leave`).
    pub(super) fn bind(&self, dispatch: &mut JsStepDispatch) {
        let stacks = self.stacks();
        let windows = stacks.windows.as_ptr_range();
        let shadow = stacks.shadow.as_ptr_range();
        (dispatch.native_top, dispatch.native_end) = (windows.start as usize, windows.end as usize);
        (dispatch.shadow_top, dispatch.shadow_end) = (shadow.start as usize, shadow.end as usize);
        dispatch.call_cache = stacks.cache.as_ptr() as usize;
    }

    /// Whether the payload runs inside a frameless activation.
    pub(super) fn inside(&self, dispatch: &JsStepDispatch) -> bool {
        dispatch.shadow_top != self.stacks().shadow.as_ptr() as usize
    }

    /// The count of the frameless activations the payload runs inside, its innermost's depth:
    /// distinct for every one live at once in one native entry (`lane_allocations::regexp_argument_key`).
    pub(super) fn depth(&self, dispatch: &JsStepDispatch) -> usize {
        dispatch.shadow_top.saturating_sub(self.stacks().shadow.as_ptr() as usize) / size_of::<ShadowRecord>()
    }

    /// Forgets every admission (`Vm::clear_call_targets`).
    pub(in crate::vm) fn clear(&mut self) {
        if let Some(stacks) = self.stacks.get_mut() {
            stacks.cache.fill(CallEntry::default());
        }
        self.functions.clear();
    }

    #[cfg(feature = "native-execution")]
    fn live(&self, dispatch: &JsStepDispatch) -> &[ShadowRecord] {
        let shadow = &self.stacks().shadow;
        let count = (dispatch.shadow_top - shadow.as_ptr() as usize) / size_of::<ShadowRecord>();
        &shadow[..count]
    }
}

/// The dense capture slots (values, length) of `callee`'s admission (`emit_entry`), which a
/// frameless activation of it reads, from the payload's call cache word `cache`. A declined
/// admission, or a payload that binds none (a lane run's), gives an empty run, so each read
/// takes its slow case.
pub(super) fn emit_admitted_captures(builder: &mut FunctionBuilder<'_>, payload: ir::Value, callee: ir::Value,
    cache: ir::Value) -> (ir::Value, ir::Value) {
    let (bound, declined, done) = (builder.create_block(), builder.create_block(), builder.create_block());
    let values = builder.append_block_param(done, types::I64);
    let len = builder.append_block_param(done, types::I64);
    emit(builder).branch_if(cache, (super::forced_or(bound, declined), &[]), (declined, &[]));
    builder.switch_to_block(bound);
    let entry = emit_entry(builder, payload, callee, cache, declined);
    let word = |field: usize| i32::try_from(field).expect("entry field offset fits i32");
    let admitted = (emit(builder).raw_load(Lane::I64, regions::heap_meta(), entry, word(std::mem::offset_of!(CallEntry, capture_values))),
        emit(builder).raw_load(Lane::I64, regions::heap_meta(), entry, word(std::mem::offset_of!(CallEntry, capture_len))));
    emit(builder).goto(done, &[admitted.0, admitted.1]);
    builder.switch_to_block(declined);
    let none = emit(builder).raw_const(Lane::I64, 0);
    emit(builder).goto(done, &[none, none]);
    builder.switch_to_block(done);
    (values, len)
}

/// The cache entry admitting `callee` in this epoch, asking `general_frameless_admit` on a
/// miss, in the call cache word `cache`; `framed` where it declines.
fn emit_entry(builder: &mut FunctionBuilder<'_>, payload: ir::Value, callee: ir::Value, cache: ir::Value,
    framed: ir::Block) -> ir::Value {
    let slot = emit(builder).raw_binary_imm(Binary::And, callee, (CACHE_ENTRIES - 1) as i64);
    let offset = emit(builder).raw_binary_imm(Binary::Mul, slot, size_of::<CallEntry>() as i64);
    let cached = emit(builder).raw_binary(Binary::Add, cache, offset);
    let named = emit(builder).raw_load(Lane::I64, regions::heap_meta(), cached, std::mem::offset_of!(CallEntry, callee) as i32);
    let epoch = emit(builder).raw_load(Lane::I64, regions::heap_meta(), cached, std::mem::offset_of!(CallEntry, epoch) as i32);
    let current = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.epoch));
    let same = emit(builder).raw_compare(Relation::Equal, named, callee);
    let fresh = emit(builder).raw_compare(Relation::Equal, epoch, current);
    let hit = emit(builder).raw_binary(Binary::And, same, fresh);
    let (found, admit) = (builder.create_block(), builder.create_block());
    let entry = builder.append_block_param(found, types::I64);
    emit(builder).branch_if(hit, (found, &[cached]), (admit, &[]));
    builder.switch_to_block(admit);
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, frameless));
    let admitted = emit_lane_helper_call(builder, payload, helper, &[payload, callee]);
    let some = emit(builder).raw_compare_imm(Relation::NotEqual, admitted, 0);
    emit(builder).branch_if(some, (found, &[admitted]), (framed, &[]));
    builder.switch_to_block(found);
    entry
}

/// The function a frameless callee runs, read from its heap object: the callee is an admitted user function
/// (`general_frameless_admit`), so its heap kind names it. Not `admission::admitted_function`, which finds an
/// admission by function index, not by callee value.
#[cfg(feature = "native-execution")]
pub(super) fn callee_function<H: Host>(vm: &Vm<H>, callee: Value) -> u32 {
    let Some(crate::heap::FunctionKind::User(_, function)) = vm.heap.function(callee).map(|callee| callee.kind)
        else { unreachable!("a frameless callee is an admitted user function") };
    function
}

/// The chain of the payload's frameless activations, outermost first, followed by `inlined`:
/// each continues at its callee's call, the innermost at `pc`. Counts the leave against the
/// innermost frameless function (`MATERIALIZATION_LIMIT`).
#[cfg(feature = "native-execution")]
pub(super) fn chain<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &JsStepDispatch, pc: u32,
    inlined: impl Iterator<Item = ChainFrame>) -> Vec<ChainFrame> {
    let live = vm.frameless.live(dispatch);
    let continues = live.iter().skip(1).map(|record| record.call_pc as u32).chain([pc]);
    // An admitted callee is a user function of the entry's program; its window follows (`emit_enter`).
    let mut window = vm.frameless.stacks().windows.as_ptr();
    let records: Vec<_> = live.iter().zip(continues).map(|(record, source_pc)| {
        let registers = window;
        window = window.wrapping_add(usize::from(program.function(callee_function(vm, record.callee) as usize).registers));
        ChainFrame { source_pc, registers, link: *record }
    }).collect();
    if let Some(last) = records.last() {
        let last_function = callee_function(vm, last.link.callee);
        let epoch = vm.cache_epochs.gc;
        let (leaves, counted) = vm.frameless.materializations.entry((dispatch.program_id.raw(), last_function)).or_default();
        if *counted != epoch { (*leaves, *counted) = (0, epoch); }
        *leaves += 1;
        if *leaves == MATERIALIZATION_LIMIT {
            // Calls of it keep frames for this epoch: no cached admission names it.
            vm.frameless.clear();
        }
    }
    records.into_iter().chain(inlined).collect()
}

/// An exact step inside a frameless activation runs in its frame: the step that made the
/// chain's frames, which the general loop continues from (`StepResult::ResumeFrames`).
#[cfg(feature = "native-execution")]
pub(super) fn leave<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &mut JsStepDispatch, pc: usize) -> Option<JsNativeStep> {
    if !vm.frameless.inside(dispatch) { return None; }
    let chain = chain(vm, program, dispatch, pc as u32, std::iter::empty());
    super::materialize_chain(vm, program, dispatch, &chain);
    dispatch.outcome.take()
}

/// One native activation the VM makes a frame for (`materialize_chain`): its function, the
/// call in its caller, the PC it continues at, its registers, and the receiver a construction
/// allocated for it (`Value::DELETED` for a call), and its callee where its instruction names
/// none in a register (a getter's read; else `Value::DELETED`).
#[derive(Clone, Copy)]
#[cfg(feature = "native-execution")]
pub(in crate::vm) struct ChainFrame {
    pub(super) source_pc: u32, pub(super) registers: *const Value,
    /// The link's call record (its PC, where a header holds the call's `Ip`), receiver and callee:
    /// the `ShadowRecord` the native caller wrote, which the header words of the link repeat
    /// (`frame::CONTEXT_CALL`, `CONTEXT_THIS`, `CONTEXT_CALLEE`). Taken whole, not field by field.
    pub(super) link: ShadowRecord,
}
