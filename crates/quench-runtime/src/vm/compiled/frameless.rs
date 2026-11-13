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
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

#[path = "frameless/admission.rs"] mod admission;
pub(super) use admission::general_frameless_admit;

/// Values of the windows of all frameless activations one native entry may hold.
const WINDOW_VALUES: usize = 1 << 16;
const SHADOW_RECORDS: usize = 1 << 12;
const CACHE_ENTRIES: usize = 256;
/// Recoveries of one function's frameless activations in one collection epoch after which
/// its calls keep frames for the rest of the epoch.
const MATERIALIZATION_LIMIT: u32 = 16;
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
    call_pc: u64,
    this: Value,
    callee: Value,
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
    windows: Box<[Value]>,
    shadow: Box<[ShadowRecord]>,
    cache: Box<[CallEntry]>,
    /// Each function's last admission, by function index, with its program and closure
    /// environment: a fresh closure of it shares all but its callee and captures (`admit`).
    functions: Vec<(u32, Value, CallEntry)>,
    /// The leaves of each function's frameless activations in the epoch they count in
    /// (`MATERIALIZATION_LIMIT`).
    materializations: rustc_hash::FxHashMap<(u32, u32), (u32, u64)>,
}

impl Default for Frameless {
    fn default() -> Self {
        let empty = ShadowRecord { call_pc: 0, this: Value::UNDEFINED, callee: Value::UNDEFINED };
        Self {
            windows: vec![Value::UNDEFINED; WINDOW_VALUES].into_boxed_slice(),
            shadow: vec![empty; SHADOW_RECORDS].into_boxed_slice(),
            cache: vec![CallEntry::default(); CACHE_ENTRIES].into_boxed_slice(),
            functions: Vec::new(),
            materializations: rustc_hash::FxHashMap::default(),
        }
    }
}

impl Frameless {
    /// Binds a general entry's payload to empty stacks: a native entry starts with no
    /// frameless activation live (`leave`).
    pub(super) fn bind(&self, dispatch: &mut JsStepDispatch) {
        let windows = self.windows.as_ptr_range();
        let shadow = self.shadow.as_ptr_range();
        (dispatch.native_top, dispatch.native_end) = (windows.start as usize, windows.end as usize);
        (dispatch.shadow_top, dispatch.shadow_end) = (shadow.start as usize, shadow.end as usize);
        dispatch.call_cache = self.cache.as_ptr() as usize;
    }

    /// Whether the payload runs inside a frameless activation.
    #[cfg(feature = "native-execution")]
    pub(super) fn inside(&self, dispatch: &JsStepDispatch) -> bool {
        dispatch.shadow_top != self.shadow.as_ptr() as usize
    }

    /// Forgets every admission (`Vm::clear_call_targets`).
    pub(in crate::vm) fn clear(&mut self) {
        self.cache.fill(CallEntry::default());
        self.functions.clear();
    }

    #[cfg(feature = "native-execution")]
    fn live(&self, dispatch: &JsStepDispatch) -> &[ShadowRecord] {
        let count = (dispatch.shadow_top - self.shadow.as_ptr() as usize) / size_of::<ShadowRecord>();
        &self.shadow[..count]
    }
}

/// The dense capture slots (values, length) of `callee`'s admission (`emit_entry`), which a
/// frameless activation of it reads; `declined` where the payload binds no admissions (a lane
/// run's) or `callee` has none.
pub(super) fn emit_admitted_captures(builder: &mut FunctionBuilder<'_>, payload: ir::Value, callee: ir::Value,
    declined: ir::Block) -> (ir::Value, ir::Value) {
    let cache = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, call_cache));
    let bound = builder.create_block();
    builder.ins().brif(cache, super::forced_or(bound, declined), &[], declined, &[]);
    builder.switch_to_block(bound);
    let entry = emit_entry(builder, payload, callee, declined);
    let word = |field: usize| i32::try_from(field).expect("entry field offset fits i32");
    (builder.ins().load(types::I64, MemFlagsData::trusted(), entry, word(std::mem::offset_of!(CallEntry, capture_values))),
        builder.ins().load(types::I64, MemFlagsData::trusted(), entry, word(std::mem::offset_of!(CallEntry, capture_len))))
}

/// The cache entry admitting `callee` in this epoch, asking `general_frameless_admit` on a
/// miss; `framed` where it declines.
fn emit_entry(builder: &mut FunctionBuilder<'_>, payload: ir::Value, callee: ir::Value, framed: ir::Block) -> ir::Value {
    let cache = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, call_cache));
    let slot = builder.ins().band_imm_u(callee, (CACHE_ENTRIES - 1) as i64);
    let offset = builder.ins().imul_imm_u(slot, size_of::<CallEntry>() as i64);
    let cached = builder.ins().iadd(cache, offset);
    let named = builder.ins().load(types::I64, MemFlagsData::trusted(), cached, std::mem::offset_of!(CallEntry, callee) as i32);
    let epoch = builder.ins().load(types::I64, MemFlagsData::trusted(), cached, std::mem::offset_of!(CallEntry, epoch) as i32);
    let current = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, epoch));
    let same = builder.ins().icmp(IntCC::Equal, named, callee);
    let fresh = builder.ins().icmp(IntCC::Equal, epoch, current);
    let hit = builder.ins().band(same, fresh);
    let (found, admit) = (builder.create_block(), builder.create_block());
    let entry = builder.append_block_param(found, types::I64);
    builder.ins().brif(hit, found, &[ir::BlockArg::Value(cached)], admit, &[]);
    builder.switch_to_block(admit);
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, frameless));
    let admitted = emit_lane_helper_call(builder, payload, helper, &[payload, callee]);
    let some = builder.ins().icmp_imm_u(IntCC::NotEqual, admitted, 0);
    builder.ins().brif(some, found, &[ir::BlockArg::Value(admitted)], framed, &[]);
    builder.switch_to_block(found);
    entry
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
    let mut window = vm.frameless.windows.as_ptr();
    let records: Vec<_> = live.iter().zip(continues).map(|(record, source_pc)| {
        let Some(crate::heap::FunctionKind::User(_, function)) = vm.heap.function(record.callee).map(|callee| callee.kind)
            else { unreachable!("a frameless callee is an admitted user function") };
        let registers = window;
        window = window.wrapping_add(usize::from(program.function(function as usize).registers));
        ChainFrame { function, call_pc: record.call_pc as u32, source_pc, registers, this: record.this, callee: record.callee }
    }).collect();
    if let Some(last) = records.last() {
        let epoch = vm.heap.collection_epoch();
        let (leaves, counted) = vm.frameless.materializations.entry((dispatch.program_id.raw(), last.function)).or_default();
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
    pub(super) function: u32, pub(super) call_pc: u32, pub(super) source_pc: u32, pub(super) registers: *const Value,
    pub(super) this: Value, pub(super) callee: Value,
}
