//! The callee admission cache of native code. An admitted user function's entry (`CallEntry`)
//! is cached by callee value for the collection epoch it was admitted in (`general_frameless_admit`,
//! `admission.rs`): a lane reads an inlined callee's dense capture slots from it
//! (`emit_admitted_captures`), and each closure of an admitted function shares the function's entry.
use super::{emit_js_payload_word, emit_lane_helper_call, JsStepDispatch};
use crate::vm::compiled::regions;
use crate::Value;
use cranelift_codegen::ir::{self, types};
use super::object_slots::emit;
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use cranelift_frontend::FunctionBuilder;

#[path = "frameless/admission.rs"] mod admission;
pub(super) use admission::general_frameless_admit;

const CACHE_ENTRIES: usize = 256;
/// `CallEntry::meta`'s fields above its function: the register count, then the parameter count.
const META_REGISTERS: u32 = u32::BITS;
const META_PARAMS: u32 = META_REGISTERS + u16::BITS;
/// `CallEntry::this`: the receiver as passed (a strict body), or no `this` slot (an arrow's
/// is lexical). Any other value is the global object a sloppy body binds for `undefined`
/// or `null`.
const THIS_RECEIVER: u64 = 0;
const THIS_NONE: u64 = 1;

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

/// The per-VM storage of the callee admission cache.
#[derive(Default)]
pub(crate) struct Frameless {
    /// The cache, made when native code first binds it (`bind`): a VM that runs no native body keeps
    /// none, and one that does commits only the pages its admissions reach (`zeroed_boxed_slice`).
    cache: std::sync::OnceLock<Box<[CallEntry]>>,
    /// Each function's last admission, by function index, with its program and closure
    /// environment: a fresh closure of it shares all but its callee and captures (`admit`).
    functions: Vec<(u32, Value, CallEntry)>,
}

/// A boxed slice of `len` zeroed elements. The allocation is zeroed by the allocator, which
/// backs a large one with fresh pages it commits by first touch: the cache pays resident
/// memory only for the entries a run reaches, not for all of them at binding.
///
/// # Safety
/// All-zero bytes are a valid `T`.
unsafe fn zeroed_boxed_slice<T>(len: usize) -> Box<[T]> {
    let layout = std::alloc::Layout::array::<T>(len).expect("frameless cache layout");
    // SAFETY: the layout is nonzero-sized, as the cache holds entries.
    let base = unsafe { std::alloc::alloc_zeroed(layout) }.cast::<T>();
    if base.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    // SAFETY: `len` zeroed elements the box owns, freed by `Box`'s deallocation with the same
    // layout; zeroed bytes are valid per the caller's contract.
    unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(base, len)) }
}

impl Frameless {
    fn cache(&self) -> &[CallEntry] {
        self.cache.get_or_init(|| {
            // SAFETY: `CallEntry` is a struct of `u64` words, whose all-zero bytes are its default.
            unsafe { zeroed_boxed_slice(CACHE_ENTRIES) }
        })
    }

    fn cache_mut(&mut self) -> &mut [CallEntry] {
        self.cache();
        self.cache.get_mut().expect("made above")
    }

    /// Binds a general entry's payload to the call cache its native code reads admissions from.
    pub(super) fn bind(&self, dispatch: &mut JsStepDispatch) {
        dispatch.call_cache = self.cache().as_ptr() as usize;
    }

    /// Forgets every admission (`Vm::clear_call_targets`).
    pub(in crate::vm) fn clear(&mut self) {
        if let Some(cache) = self.cache.get_mut() {
            cache.fill(CallEntry::default());
        }
        self.functions.clear();
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
