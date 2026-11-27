//! Cranelift-backed compiled tiers for the shared VM.
//!
//! In Ershov's terms the lane's per-instruction semantics have two readings.
//! The lane handlers interpret them; this module is their generating
//! extension: for each instruction it emits the same meaning as Cranelift IR,
//! with every operand the residual already fixes (register numbers,
//! immediates, constants, jump targets, callees) folded in. Running it over a
//! function's residual bytecode is Futamura's first projection, the
//! interpreter specialized to that function; what specialization removes is
//! exactly what the program fixed: dispatch, operand decoding, value tags
//! (i32 and i64 stay unboxed), the register file (registers become SSA
//! variables) and the per-run memory-view resolution.
//!
//! Each operator keeps one semantic authority. Integer operators come from
//! the integer, float and conversion selectors of `quench_runtime::wasm` (an i32
//! operator is its i64 namesake at the narrower width), memory accesses from
//! `MemoryLoad`/`MemoryStore`, traps from `WasmTrap`. A function compiles only
//! when every instruction it can reach has a compiled denotation and every
//! function it calls compiles; anything else stays interpreted, so the mode
//! never changes behavior, only speed. Compiled code never allocates or
//! collects itself and holds no heap roots: values cross its boundary in
//! their Wasm encoding, a trap leaves as a status that becomes the
//! interpreter's `WasmTrap`, and an imported call is a callout through which
//! the VM makes the call exactly as the interpreter would (`import_callout`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use tier_policy::Tiers;
use crate::dispatch_fast::{frame_word, set_frame_word, Activation};
use quench_runtime::staged::frame::{CONTEXT_BODY, CONTEXT_THIS};
use crate::engine_prelude::*;
use quench_runtime::bytecode::{Traits,
    ControlFlowLayout, FieldLookup, Function, Operand, OperandKind, ResidualProgram,
    WideInstruction,
};
use quench_runtime::staged::wasm_call::Call as WasmCall;
use quench_runtime::staged::wasm_flow::Flow;
use quench_runtime::staged::wasm_step::Step;
use quench_runtime::staged::machine::{MachineAccess as _, MachineFlow as _, MachineScalar as _, MachineStage};
use quench_runtime::staged::Stage as _;
use quench_runtime::staged::wasm::{Width, integer_binary};
use quench_runtime::staged::wasm_operation::{Form, Operand as Right, WasmOperation};
use quench_runtime::staged::ConcreteWasm;
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, MemFlagsData, Signature, UserFuncName, types};
#[cfg(test)]
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_frontend::{FunctionBuilder, Variable};
#[cfg(test)]
use cranelift_frontend::FunctionBuilderContext;
use cranelift_codegen::entity::{SecondaryMap, packed_option::PackedOption};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use quench_runtime::staged::js_step_head::JsStepHead;

/// The staged operation a record decodes to, with no constants or sites resolved: what a
/// tier's analysis asks of a record instead of naming its opcode (`Operation::decode`).
#[inline]
pub(crate) fn decoded(instruction: impl std::borrow::Borrow<quench_runtime::bytecode::WideInstruction>) -> Option<quench_runtime::staged::Operation<(), ()>> {
    quench_runtime::staged::Operation::decode(*instruction.borrow(), quench_runtime::staged::DecodeContext::EMPTY)
}

/// The staged operation a record makes apart from how its result completes the frame (its result
/// flags), with no constants or sites resolved (`Operation::decode_completed`): what a tier that
/// completes the result itself asks instead of naming the opcode.
#[inline]
pub(crate) fn decoded_completed(instruction: impl std::borrow::Borrow<quench_runtime::bytecode::WideInstruction>) -> Option<quench_runtime::staged::Operation<(), ()>> {
    quench_runtime::staged::Operation::decode_completed(*instruction.borrow(), quench_runtime::staged::DecodeContext::EMPTY).map(|(operation, _)| operation)
}

/// The staged operation a record performs, whatever completion its result flags ask (a result
/// that also returns from the frame or writes `this`): what an analysis asks of a record when the
/// question is the operation, not how the frame completes. `decoded` declines a flagged record,
/// whose row a tier makes through its completion.
#[inline]
pub(crate) fn performed(instruction: impl std::borrow::Borrow<quench_runtime::bytecode::WideInstruction>) -> Option<quench_runtime::staged::Operation<(), ()>> {
    let instruction = *instruction.borrow();
    let plain = if instruction.op().field_layout(quench_runtime::bytecode::InstructionField::A) == quench_runtime::bytecode::FieldLayout::ResultRegister {
        quench_runtime::bytecode::WideInstruction::new(instruction.op(), instruction.a() & quench_runtime::bytecode::REGISTER_MASK,
            instruction.b(), instruction.c(), instruction.imm())
    } else { instruction };
    decoded(plain)
}
/// What an instruction means to the compiled tiers, classified from the shared decode (`staged/meaning_compiled.rs`).
#[path = "compiled/meaning_compiled.rs"]
pub(crate) mod meaning_compiled;
#[path = "compiled/slow_paths.rs"] pub(crate) mod slow_paths;

/// The Number unary operation a record denotes with its operator (`NumberUnary::from_js_instruction`
/// asked of the record's opcode), so a tier passes the record and names no opcode.
#[inline]
pub(crate) fn number_unary(instruction: WideInstruction, operator: u32) -> Option<operations::NumberUnary> {
    operations::NumberUnary::from_js_instruction(instruction.op(), operator)
}

/// The signed int32 bitwise operation a record denotes with its operator (`NumberBitwise::from_js_instruction`
/// asked of the record's opcode).
#[inline]
pub(crate) fn number_bitwise(instruction: WideInstruction, operator: u32) -> Option<operations::NumberBitwise> {
    operations::NumberBitwise::from_js_instruction(instruction.op(), operator)
}

/// The Number arithmetic operation a record denotes with its operator (`NumberArithmetic::from_js_instruction`
/// asked of the record's opcode).
#[inline]
pub(crate) fn number_arithmetic(instruction: WideInstruction, operator: u32) -> Option<operations::NumberArithmetic> {
    operations::NumberArithmetic::from_js_instruction(instruction.op(), operator)
}

#[path = "compiled/osr_loop.rs"]
pub(crate) mod osr_loop;
pub(crate) mod call_signature;
pub(crate) mod exit_protocol;
pub(crate) mod fallback_sites;
pub(crate) mod entry_protocol;
pub(crate) mod region_lend;
pub(crate) use exit_protocol::{ExitFact, ExitTable, RegionRun};
pub mod osr_compile; pub(crate) mod osr_lane; pub(crate) mod base_region; pub(crate) mod base_body; pub(crate) use base_body::image_body; pub(crate) mod base_rows; pub(crate) mod stencil_run; use stencil_run::Crossing;
pub(crate) use osr_compile::{CallCode, OsrCompile, PendingOsrRegion, RegionCalls};
#[cfg(feature = "jit")]
pub(crate) use lane_compile::FactTables;
pub(crate) use lane_facts::bind_callees as bind_region_callees;
pub(crate) use lane_region::ElementViews;
pub(crate) use lane_region::{inlines_in_loop, inlines_in_loop_call, native_method_call};
use quench_runtime::typed_ir;

#[path = "compiled/facts.rs"]
pub(crate) mod facts;
pub use quench_runtime::vm::tier_policy;
#[cfg(test)]
pub(crate) mod stable_fact_tests;
#[path = "compiled/lane_region.rs"]
pub(crate) mod lane_region;
pub(crate) use lane_region::relocs::RelocKind;
pub(crate) mod declared_checks;
#[path = "compiled/emit_machine.rs"]
pub(crate) mod emit_machine;
#[path = "compiled/lane_unit.rs"]
pub(crate) mod lane_unit;
pub(crate) mod admission;
#[cfg(all(test, feature = "aot"))]
#[path = "compiled/lane_unit_polymorphic_tests.rs"]
pub(crate) mod lane_unit_polymorphic_tests;
#[path = "compiled/lane_sites.rs"]
pub(crate) mod lane_sites;
#[path = "compiled/lane_allocations.rs"] pub(crate) mod lane_allocations;
use lane_allocations::{binary_in_lane, collection_lookup_in_lane, concat_pair_in_lane, flatten_string_in_lane, scalar_native_in_lane, new_array_in_lane, new_closure_in_lane, new_concat_in_lane, new_const_array_in_lane, compiled_new_object, new_object_literal_in_lane, new_object_pair_in_lane, lane_new_regexp, new_template_in_lane};
#[path = "compiled/lane_compile.rs"]
pub(crate) mod lane_compile;
#[path = "compiled/lane_method.rs"] pub(crate) mod lane_method;
#[path = "compiled/site_posterior.rs"]
pub(crate) mod site_posterior;
#[path = "compiled/entry_cell.rs"] pub(crate) mod entry_cell;
#[path = "compiled/numeric_image.rs"] pub(crate) mod numeric_image; #[cfg(test)] pub(crate) use numeric_image::compile_js_typed_numeric_region;
pub(crate) use numeric_image::NumericImage;
#[path = "compiled/ahead_images.rs"] pub(crate) mod ahead_images; pub use ahead_images::AheadImages; pub(crate) use ahead_images::closed_lane_image;
pub use ahead_images::{AHEAD_BUILD_ID, AHEAD_BUILD_ID_PREFIX, ahead_program_fingerprint};
#[path = "compiled/lane_body_abi.rs"] pub(crate) mod lane_body_abi; use lane_body_abi::{BodyEntry, BodyReturn};
#[path = "compiled/lane_facts.rs"]
pub(crate) mod lane_facts;
#[path = "compiled/tier_model.rs"] pub(crate) mod tier_model;
#[path = "compiled/loop_model.rs"] pub(crate) mod loop_model;
#[path = "compiled/loop_tier_up.rs"] pub(crate) mod loop_tier_up;
pub(crate) use lane_compile::{LaneBodyOutcome, LaneBodyWork, PendingLaneMethod};
#[path = "compiled/static_loops.rs"]
pub(crate) mod static_loops;
#[path = "compiled/wasm_callable.rs"]
pub(crate) mod wasm_callable;
pub(crate) mod wasm_growth;
#[path = "compiled/wasm_stage.rs"]
pub(crate) mod wasm_stage;
pub(crate) mod wasm_instance_stage;
pub(crate) mod instance_context;
pub(crate) mod indirect_profile;
pub use instance_context::{InstanceContext, InstanceRecord, NativeRoots};
#[cfg(test)]
pub(crate) mod wasm_global_tests;
pub(crate) mod wasm_memory_stage;
pub(crate) mod wasm_flow_stage;
pub(crate) mod wasm_call_stage;
pub(crate) mod wasm_table_stage;
pub(crate) mod wasm_table_ops_stage;
pub(crate) mod wasm_gc_stage;
pub(crate) mod wasm_gc_allocate;
use self::wasm_gc_allocate::allocating_reads;
pub(crate) mod wasm_exception_stage;
pub(crate) mod wasm_catch;
pub(crate) mod plan_report; pub(crate) mod body_views;
pub(crate) mod wasm_gc_callout;
pub(crate) mod wasm_instance_switch;
pub(crate) mod wasm_roots;
pub(crate) mod wasm_simd_stage;
// The one stitcher every b-jit family places its rows through.
pub(crate) mod stitcher;
// `--b-jit` enters a module's functions through their stitched rows (`stitched_entry`).
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) mod wasm_stitch;
pub(crate) mod wasm_specializer;
// An AOT image's Wasm functions: their stitched rows, planned from the module's declarations.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) mod wasm_image;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) mod wasm_world;
use wasm_specializer::{WasmResidual, WasmSpecializer, WasmTierChoice};
use quench_runtime::vm::specializer::Specializer as _;
/// The b-jit's Wasm codes equal the runtime's values (compile-time checks only).
pub(crate) mod wasm_codes_check;
pub(crate) mod wasm_call_vm;
pub(crate) use lane_unit::{CalleeBinding, CallTarget, InlinedCall};
#[cfg(test)]
pub(crate) use lane_unit::static_call_targets;
/// `emit` with its guards exempt from the forced-deopt switch: the status dispatch of a call
/// protocol is no speculation (`forced_miss::exempt`).
#[inline(always)]
fn unforced<T>(emit: impl FnOnce() -> T) -> T {
    #[cfg(test)]
    return forced_miss::exempt(emit);
    #[cfg(not(test))]
    emit()
}
/// The forced-deopt switch (R26): a test arms the Nth guard `emit_js_guard` emits on this
/// thread, which then always leaves through its exit; tests compile on the running thread.
#[cfg(test)]
pub(crate) mod forced_miss {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<(Option<usize>, usize)> = const { Cell::new((None, 0)) };
        static EXEMPT: Cell<bool> = const { Cell::new(false) };
    }
    /// Runs `emit`, whose guards are not deopt points: they dispatch on a status the code
    /// already holds, which no speculation could make fail, so they are neither counted nor forced.
    pub(crate) fn exempt<T>(emit: impl FnOnce() -> T) -> T {
        let outer = EXEMPT.replace(true);
        let result = emit();
        EXEMPT.set(outer);
        result
    }
    /// Counts guards from zero; `Some(n)` forces the nth, `None` only counts.
    pub(crate) fn arm(forced: Option<usize>) { STATE.with(|state| state.set((forced, 0))); }
    /// Guards emitted since `arm`.
    pub(crate) fn emitted() -> usize { STATE.with(|state| state.get().1) }
    pub(crate) fn next() -> bool {
        if EXEMPT.get() { return false; }
        STATE.with(|state| {
            let (forced, count) = state.get();
            state.set((forced, count + 1));
            forced == Some(count)
        })
    }
}
/// The successor a guard-shaped branch takes when its condition holds: the forced-deopt switch
/// makes a test-armed guard take `fail` instead. Sites call it in place of `ok`.
#[inline(always)]
fn forced_or(ok: ir::Block, fail: ir::Block) -> ir::Block {
    #[cfg(test)]
    if forced_miss::next() { return fail; }
    #[cfg(not(test))]
    let _ = fail;
    ok
}
pub(crate) mod object_slots;
pub(crate) mod regions;
pub(crate) use object_slots::NativeFieldLoad;
pub(crate) mod residuals;
pub(crate) mod code_heap;
pub(crate) use code_heap::CodeMemory;
#[path = "compiled/direct_call.rs"]
pub(crate) mod direct_call;
pub(crate) mod direct_callee;
pub(crate) use direct_callee::{frames_window, window_width, DirectCallee, DirectCandidates};
#[path = "compiled/lane_storage.rs"]
pub(crate) mod lane_storage;
pub(crate) mod lane_body_run;
pub(crate) use lane_body_run::{BodyRun, CALLEE_ACTIVATION_PENDING, NO_PENDING_ACTIVATIONS};
#[path = "compiled/general_elements.rs"]
pub(crate) mod general_elements;
#[path = "compiled/frameless.rs"]
pub(crate) mod frameless;
#[path = "compiled/constructions.rs"]
pub(crate) mod constructions;
pub(crate) mod function_binding;
use function_binding::{admitted_function_binding_in_lane, function_binding_in_lane};
pub(crate) mod general_store;
use direct_call::direct_call;
#[cfg(all(target_arch = "aarch64", any(target_os = "linux", target_os = "macos")))]
pub(crate) mod guard_aarch64;
pub(crate) mod eager;
pub(crate) mod frozen;
#[cfg(feature = "native-execution")]
pub(crate) mod semantic_stop;
pub(crate) use frozen::ClosedFrozen;
pub(crate) use eager::compile_eager_native;
#[cfg(unix)]
#[path = "compiled/fault_handler.rs"]
pub(crate) mod fault_handler;
#[cfg(unix)]
pub use fault_handler::NativeFaultHandler;
pub(crate) use osr_loop::{
    CALLEE_CAPTURE, THIS_CAPTURE,
    FieldReceiver, FieldUse, CalleeSource, InlineCallee, InlineFrame, InlineGuard, LoopEntry, LoopRegion,
    MethodCalls, RegionAnalysisFailure, RegionRejection,
};

pub(crate) use quench_runtime::staged::Domain as NumberRelationDomain;

/// The most records a native function body covers.
const MAX_NATIVE_RECORDS: usize = quench_bjit::call_record::GENERAL_DISPATCH_BLOCKS as usize;
/// Return statuses encode the returning record's PC without colliding with the shared
/// entry's small stop, resume, or trap statuses.
const RETURN_STATUS_BASE: u32 = quench_bjit::call_record::GENERAL_RETURN_STATUS_BASE;
const _: () = assert!(MAX_NATIVE_RECORDS as u32 == quench_bjit::call_record::GENERAL_DISPATCH_BLOCKS);

const _: () = assert!(RETURN_STATUS_BASE > TERMINATED_STATUS as u32);

/// Compute ToBoolean for values whose truthiness is encoded without a heap
/// lookup. Strings, BigInts, and objects take the VM path at the same PC.
fn emit_js_tagged_truthiness(builder: &mut FunctionBuilder<'_>, bits: ir::Value, fallback: ir::Block) -> ir::Value {
    let mut stage = object_slots::emit(builder);
    let (undecided, truthy) = quench_runtime::staged::value::tagged_truthiness(&mut stage, bits);
    let decided = stage.not(undecided);
    stage.guard(decided, fallback);
    truthy
}

/// Decode an already-proved Number without repeating its semantic type check.
fn emit_known_number(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    quench_runtime::staged::value::known_number_f64(&mut object_slots::emit(builder), bits)
}

// The boxes `emit_js_box_number` made while a lowering records them (`record_boxed_numbers`):
// each tagged result with the binary64 word it boxed, which dominates it. A lowering that reads
// the Number back takes that word (`State::proofs.numbers`) instead of unboxing.
thread_local! {
    static BOXED_NUMBERS: std::cell::RefCell<Option<Vec<(ir::Value, quench_runtime::staged::Float64<ir::Value>)>>> = const { std::cell::RefCell::new(None) };
}

/// Starts recording the boxes `emit_js_box_number` makes, until `take_boxed_numbers`.
pub(crate) fn record_boxed_numbers() {
    BOXED_NUMBERS.with(|boxed| *boxed.borrow_mut() = Some(Vec::new()));
}

/// The boxes made since `record_boxed_numbers`, which stops recording.
pub(crate) fn take_boxed_numbers() -> Vec<(ir::Value, quench_runtime::staged::Float64<ir::Value>)> {
    BOXED_NUMBERS.with(|boxed| boxed.borrow_mut().take().unwrap_or_default())
}

/// Encode a Number result with `Value::number`'s integer, double and NaN
/// representations. A double with a fraction is its own bits, after one compare
/// with its truncation; a whole, signed-zero or NaN result takes the cold encoding.
pub(crate) fn emit_js_box_number(builder: &mut FunctionBuilder<'_>, proof: quench_runtime::staged::Float64<ir::Value>) -> ir::Value {
    let number = proof.word();
    let recording = BOXED_NUMBERS.with(|boxed| boxed.borrow().is_some());
    if !recording { return emit_js_box_number_word(builder, number); }
    // A recorded box is pure (no branch), so a lowering that reads the Number back leaves it
    // unread and it folds away.
    let result = emit_js_box_number_pure(builder, number);
    BOXED_NUMBERS.with(|boxed| if let Some(boxed) = boxed.borrow_mut().as_mut() { boxed.push((result, proof)); });
    result
}

/// `emit_js_box_number` with no branch: the integer, double and NaN encodings, selected.
fn emit_js_box_number_pure(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> ir::Value {
    quench_runtime::staged::value::number_word(&mut object_slots::emit(builder), number)
}

/// The int32 a double converts to exactly, and whether it does (`staged::value::exact_int32`).
fn emit_js_exact_int32(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> (ir::Value, ir::Value) {
    quench_runtime::staged::value::exact_int32(&mut object_slots::emit(builder), number)
}

fn emit_js_box_number_word(builder: &mut FunctionBuilder<'_>, number: ir::Value) -> ir::Value {
    let (cold, join) = (builder.create_block(), builder.create_block());
    builder.set_cold_block(cold);
    let result = builder.append_block_param(join, types::I64);
    let mut stage = object_slots::emit(builder);
    let (whole_or_nan, bits) = quench_runtime::staged::value::number_bits_unless_whole(&mut stage, number);
    stage.branch_if(whole_or_nan, (cold, &[]), (join, &[bits]));
    stage.builder.switch_to_block(cold);
    stage.builder.seal_block(cold);
    let encoded = quench_runtime::staged::value::number_word(&mut stage, number);
    stage.goto(join, &[encoded]);
    builder.switch_to_block(join);
    builder.seal_block(join);
    result
}

/// The ToInt32 of `number`, natively below 2^63 in magnitude; beyond, infinities and NaN
/// leave through `exit` (`EXACT_I64_TRUNCATION_LIMIT`).
pub(crate) fn emit_number_to_int32(builder: &mut FunctionBuilder<'_>, number: ir::Value, exit: ir::Block) -> ir::Value {
    // A double converted from an int32 is that int32's exact value, so its ToInt32 is the
    // int32 itself: no round trip through the double, no range guard.
    let number = builder.func.dfg.resolve_aliases(number);
    if let Some(def) = builder.func.dfg.value_def(number).inst()
        && builder.func.dfg.insts[def].opcode() == ir::Opcode::FcvtFromSint
        && let [integer] = builder.func.dfg.inst_args(def)
        && builder.func.dfg.value_type(*integer) == types::I32
    {
        return *integer;
    }
    let mut stage = object_slots::emit(builder);
    let (int32, exact) = quench_runtime::staged::value::truncate_int32(&mut stage, number);
    stage.guard(exact, exit);
    int32
}

/// Continue native code only when one guard holds. All failures share the exact-PC
/// VM fallback emitted after the direct path.
fn emit_js_guard(
    builder: &mut FunctionBuilder<'_>,
    condition: ir::Value,
    fallback: ir::Block,
) {
    let passed = builder.create_block();
    // A test arms one guard of a compile to fail every time: both successors leave.
    #[cfg(test)]
    let taken = if forced_miss::next() { fallback } else { passed };
    #[cfg(not(test))]
    let taken = passed;
    object_slots::emit(builder).branch_if(condition, (taken, &[]), (fallback, &[]));
    builder.switch_to_block(passed);
    builder.seal_block(passed);
}

/// The array record reservation's base in the dispatch payload: fixed until the arrays change.
fn emit_array_record_base(stage: &mut object_slots::Emit<'_, '_>, payload: ir::Value) -> ir::Value {
    use quench_runtime::staged::machine::Lane;
    stage.raw_load(Lane::I64, MemFlagsData::trusted(), payload,
        std::mem::offset_of!(JsStepDispatch, head.array_record_base) as i32)
}

/// The `length` of the direct array `base`, tagged, as an int32: its dense elements' count,
/// which an array without extras has as its length (no arguments object, no sparse elements,
/// `Object::mark_sparse_elements`). Any other base, or a length past int32, leaves through
/// `fallback`.
pub(crate) fn emit_array_length_read(
    builder: &mut FunctionBuilder<'_>,
    payload: ir::Value,
    base: ir::Value,
    fallback: ir::Block,
) -> ir::Value {
    use quench_runtime::staged::machine::{Lane, Unary};
    let mut stage = object_slots::emit(builder);
    let record_base = emit_array_record_base(&mut stage, payload);
    let length = quench_runtime::staged::elements::array_length(&mut stage, record_base, base, fallback)
        .expect("the emit stage continues past its guards");
    stage.raw_unary(Unary::Narrow(Lane::I32), length)
}

/// A dense own array's elements as one access proved them: the backing's first value and
/// its length. Holds until something changes the array's storage (`ArrayElements`): only
/// element reads and in-place replacements may run in between.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DenseArrayView {
    pub(crate) values: ir::Value,
    /// The backing's `u32` length, as `i32`.
    pub(crate) length: ir::Value,
    /// Whether a store may replace a present element in place: the array owns its backing
    /// alone, is not frozen and has no extras (indexed descriptors, an arguments map or a
    /// prototype watch), as `Vm::try_replace_record_element` requires.
    pub(crate) writable: bool,
    /// Whether its backing counts no values (`emit_dense_array_uncounted` ran on the edge
    /// that proved the view): a store through it then writes the element alone.
    pub(crate) uncounted: bool,
    /// The double flag of a backing the staged row's proof admitted double
    /// (`staged::elements::DenseView::double`); `None` where the view proved it wide.
    pub(crate) double: Option<ir::Value>,
}

/// The emission proofs a staged value operation carries: a dense array view with the cold
/// block a hole or miss continues at.
#[derive(Clone, Copy)]
pub(crate) enum EmitProof {
    /// `in_bounds`: the index is proved below the view's length, so only a hole leaves at `absent`.
    Index { view: DenseArrayView, absent: ir::Block, in_bounds: bool },
    Allocation { miss: ir::Block },
    Safepoint { interrupt: ir::Value }, IndexSetHelper { miss: ir::Block }, GlobalRead { miss: Option<ir::Block>, binding: quench_runtime::staged::GlobalBinding, slot: Option<(quench_runtime::vm::global_facts::GlobalSlot, quench_runtime::staged::ShapeFact)> }, FieldStoreMiss { miss: ir::Block },
}

/// The view of the dense own array `base` (tagged); `writable` also proves in-place
/// replacement. Any other base leaves through `fallback`, with nothing read.
pub(crate) fn emit_dense_array_view(
    builder: &mut FunctionBuilder<'_>,
    payload: ir::Value,
    base: ir::Value,
    writable: bool,
    fallback: ir::Block,
) -> DenseArrayView {
    use quench_runtime::staged::machine::{Lane, Unary};
    let mut stage = object_slots::emit(builder);
    let record_base = emit_array_record_base(&mut stage, payload);
    let backing = quench_runtime::staged::elements::array_backing(&mut stage, record_base, base, fallback)
        .expect("the emit stage continues past its guards");
    let quench_runtime::staged::elements::Backing { values, length, .. } = backing;
    if writable {
        quench_runtime::staged::elements::writable_backing(&mut stage, backing, fallback)
            .expect("the emit stage continues past its guards");
    }
    // The view's length is an `i32`, the width an int32 index has.
    let length = stage.raw_unary(Unary::Narrow(Lane::I32), length);
    DenseArrayView { values, length, writable, uncounted: false, double: None }
}

/// The backing's first value and length of the array `base` (tagged) an earlier view
/// proved, after a helper that may have grown its backing: the array, its record and its
/// writability stay what the view proved, so only the backing is read again. The helper may
/// also have given the array extras (a sparse store's facts), which then hold the backing:
/// `staged::elements::array_header` reads it from either place.
pub(crate) fn emit_dense_array_reload(builder: &mut FunctionBuilder<'_>, payload: ir::Value, base: ir::Value) -> (ir::Value, ir::Value) {
    use quench_runtime::staged::machine::{Lane, Unary};
    let mut stage = object_slots::emit(builder);
    let record_base = emit_array_record_base(&mut stage, payload);
    let record = quench_runtime::staged::elements::array_record(&mut stage, record_base, base);
    let quench_runtime::staged::elements::Backing { values, length, .. } = quench_runtime::staged::elements::record_backing(&mut stage, record);
    // A helper may have changed the backing's storage form in place (narrow or double kind,
    // a flagged length word): the view then reads as empty, so every access through it
    // misses its bounds test and leaves.
    let encoded = stage.ugt_imm(length, u64::from(!quench_runtime::tier_surface::ENCODED_LEN));
    let empty = stage.constant(0);
    let length = stage.select(encoded, empty, length);
    (values, stage.raw_unary(Unary::Narrow(Lane::I32), length))
}

/// The raw doubles of the array `base` (tagged) whose backing is double and holds no hole
/// (`staged::elements::double_view`): their first word and their 64-bit count, as a typed view
/// holds them; `writable` also proves in-place replacement. Any other base or backing leaves
/// through `fallback`, with nothing read.
pub(crate) fn emit_double_array_view(builder: &mut FunctionBuilder<'_>, payload: ir::Value, base: ir::Value,
    writable: bool, fallback: ir::Block) -> (ir::Value, ir::Value) {
    let mut stage = object_slots::emit(builder);
    let record_base = emit_array_record_base(&mut stage, payload);
    quench_runtime::staged::elements::double_view(&mut stage, record_base, base, writable, fallback)
        .expect("the emit stage continues past its guards")
}

/// `emit_dense_array_reload` of a double view (`emit_double_array_view`): the backing read again,
/// empty once it is no longer double (`staged::elements::reloaded_double_view`).
pub(crate) fn emit_double_array_reload(builder: &mut FunctionBuilder<'_>, payload: ir::Value, base: ir::Value) -> (ir::Value, ir::Value) {
    let mut stage = object_slots::emit(builder);
    let record_base = emit_array_record_base(&mut stage, payload);
    let record = quench_runtime::staged::elements::array_record(&mut stage, record_base, base);
    quench_runtime::staged::elements::reloaded_double_view(&mut stage, record)
}

/// The address of `view`'s present element `index` (an int32) and its value; an index past
/// the elements (a negative one among them, compared unsigned) or a hole leaves through
/// `fallback`.
pub(crate) fn emit_dense_element(
    builder: &mut FunctionBuilder<'_>,
    view: DenseArrayView,
    index: ir::Value,
    fallback: ir::Block,
    in_bounds: bool,
) -> (ir::Value, ir::Value) {
    debug_assert!(view.double.is_none(), "a raw element access needs a view proved wide");
    let mut stage = object_slots::emit(builder);
    let element = if in_bounds {
        quench_runtime::staged::elements::present_element(&mut stage, view.values, index, fallback)
    } else {
        quench_runtime::staged::elements::dense_element(&mut stage, view.values, view.length, index, fallback)
    };
    element.expect("the emit stage continues past its guards")
}

/// Replace the present element at `address`, whose value was `old`, with the non-heap
/// `value` through a writable view, as `ArrayElements::replace_through` does: the backing's
/// count of non-int32 values (`OTHERS_FROM_VALUES`) follows the write, and an unknown count
/// stays unknown. A non-heap value needs no generational log (`RecordSpace::store_logs`).
pub(crate) fn emit_dense_element_replace(
    builder: &mut FunctionBuilder<'_>,
    view: DenseArrayView,
    address: ir::Value,
    old: ir::Value,
    value: ir::Value,
) {
    debug_assert!(view.writable, "an in-place replacement needs a writable view");
    let mut stage = object_slots::Emit { builder, property_values: None, gate: None, payload: None, facts: Default::default() };
    if view.uncounted {
        quench_runtime::staged::elements::replace_uncounted_element(&mut stage, address, value);
    } else {
        quench_runtime::staged::elements::replace_element(&mut stage, view.values, address, old, value);
    }
}

/// Mark a writable view's backing as holding uncounted other values (`OTHERS_UNKNOWN`), so
/// stores through it need not keep the count (`ArrayElements::replace_through`), as
/// `osr_loop` does for the arrays a region writes; the count stays unknown until the heap
/// counts the values again.
pub(crate) fn emit_dense_array_uncounted(builder: &mut FunctionBuilder<'_>, view: DenseArrayView) {
    debug_assert!(view.writable, "only a writable view's backing is this array's own");
    quench_runtime::staged::elements::uncount_backing(&mut object_slots::emit(builder), view.values);
}

/// Flat field input bound once for a no-GC, layout-preserving region. Values
/// may change; the field table, arena and record reservation cannot.
#[derive(Clone, Copy)]
struct FieldSlotBinding {
    receiver: ir::Value,
    holder: ir::Value,
    slot: FieldSlot,
    records: ir::Value,
    arena: FieldArena,
    writes: Option<quench_runtime::typed_ir::Type>,
    /// Whether the run's entry admitted `writes` for the slot (`Lowering::entry_facts`): a
    /// store through the binding guards it, so an unadmitted slot leaves only its stores.
    admitted: Option<ir::Value>,
}

/// A bound slot moves invariant address arithmetic off the receiver-load chain.
/// The remaining length is zero when the slot lies outside the arena; forming
/// an integer base grants no permission to dereference it.
#[derive(Clone, Copy)]
enum FieldArena {
    Unbound { values: ir::Value, length: ir::Value },
    BoundSlot { base: ir::Value, remaining: ir::Value },
}

impl FieldArena {
    fn bind(self, builder: &mut FunctionBuilder<'_>, slot: ir::Value) -> Self {
        let Self::Unbound { values, length } = self else { return self; };
        let (base, remaining) = quench_runtime::staged::object::bind_arena_slot(&mut object_slots::emit(builder), values, length, slot);
        Self::BoundSlot { base, remaining }
    }

    fn address(self, builder: &mut FunctionBuilder<'_>, storage: ir::Value,
        slot: FieldSlot, fallback: ir::Block) -> ir::Value
    {
        let (values, length, position) = match self {
            Self::Unbound { values, length } => {
                let slot = slot.read(builder);
                (values, length, object_slots::emit(builder).add(storage, slot))
            }
            Self::BoundSlot { base, remaining } => (base, remaining, storage),
        };
        quench_runtime::staged::object::arena_slot_address(&mut object_slots::emit(builder), values, length, position, fallback)
            .expect("the emit stage continues past its guards")
    }
}

/// A region binds its slot once; a single operation decodes it only after
/// storage admission so failed packed/shape probes do no slot work.
#[derive(Clone, Copy)]
enum FieldSlot {
    Row(ir::Value),
    Bound(ir::Value),
}

impl FieldSlot {
    fn read(self, builder: &mut FunctionBuilder<'_>) -> ir::Value {
        match self {
            Self::Bound(value) => value,
            Self::Row(row) => quench_runtime::staged::entry::entry_slot(&mut object_slots::emit(builder), row),
        }
    }
}

fn emit_js_field_binding(builder: &mut FunctionBuilder<'_>, payload: ir::Value,
    site: u16, miss: ir::Block, table: Option<object_slots::CheckedObjectTable>) -> FieldSlotBinding
{
    use quench_runtime::staged::machine::{Binary, Lane, Relation};
    use quench_runtime::vm::NATIVE_FIELD_CACHE_LAYOUT as CACHE;
    let entries = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.field_entries));
    let records = table.map(|table| table.base()).unwrap_or_else(||
        emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.object_record_base)));
    let (values, length) = emit_js_property_values(builder, payload);
    let mut stage = object_slots::emit(builder);
    // ObjectHeader owns record-base admission, including reused region proofs.
    for base in [entries, values] {
        let present = stage.raw_compare_imm(Relation::NotEqual, base, 0);
        stage.guard(present, miss);
    }
    let entry = stage.add_imm(entries, (usize::from(site) * CACHE.entry_size) as u64);
    let mut word = |offset: usize, complement: u32| {
        let stored = stage.raw_load(Lane::I32, MemFlagsData::trusted(), entry, offset as i32);
        let complement = stage.raw_const(Lane::I32, i64::from(complement));
        stage.raw_binary(Binary::Xor, stored, complement)
    };
    let receiver = word(CACHE.receiver_offset, CACHE.receiver_complement);
    let holder = word(CACHE.holder_offset, CACHE.holder_complement);
    FieldSlotBinding { receiver, holder, slot: FieldSlot::Row(entry), records,
        arena: FieldArena::Unbound { values, length }, writes: None, admitted: None }
}

// `packed_slot`, when requested, receives a proved own-slot address holding a narrow
// u32. The ordinary return addresses a full tagged Value in arena or holder extras. Neither
// projection survives an operation that can change storage or collect the object.
fn emit_js_cached_field_slot(
    builder: &mut FunctionBuilder<'_>,
    payload: ir::Value,
    receiver: ir::Value,
    site: u16,
    allow_holder: bool,
    fallback: ir::Block,
    binding: Option<FieldSlotBinding>,
    header: Option<object_slots::ObjectHeader>,
    packed_slot: Option<ir::Block>,
) -> (ir::Value, ir::Value) {
    use quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT as OBJECT;
    use quench_runtime::staged::machine::{Binary, Lane, Relation};
    use quench_runtime::staged::object::{extras_pointer, has_extras, inline_storage, object_record_of, plain_payload, proto_link, proto_object_record_id};
    use quench_runtime::staged::region::Region;
    use quench_runtime::vm::NATIVE_FIELD_CACHE_LAYOUT as CACHE;

    let base = receiver;
    let record_id = quench_runtime::staged::value::direct_object_id(&mut object_slots::emit(builder), base);

    let binding = binding.unwrap_or_else(|| emit_js_field_binding(builder, payload, site, fallback, None));
    let FieldSlotBinding { holder, receiver: receiver_shape, slot, records, arena, .. } = binding;
    let own = object_slots::emit(builder).raw_compare_imm(Relation::Equal, holder, i64::from(CACHE.own_holder));
    if !allow_holder {
        emit_js_guard(builder, own, fallback);
    }

    let object_slots::ObjectHeader { record, shape, inline, address: object, .. } = header
        .unwrap_or_else(|| {
            let class_base = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.class_base));
            object_slots::ObjectHeader::prove(builder, base, records, class_base, None, fallback)
        });
    // The exact shape includes dictionary storage, which cannot match an own-data row.
    let same_shape = object_slots::emit(builder).raw_compare(Relation::Equal, shape, receiver_shape);
    emit_js_guard(builder, same_shape, fallback);
    // A holder read uses only the receiver's shape and prototype link. Its own
    // property representation is unrelated to the holder's slot address.
    let own_storage = |builder: &mut FunctionBuilder<'_>| {
        if let Some(packed_slot) = packed_slot {
            let packed = builder.create_block();
            let arena = builder.create_block();
            object_slots::emit(builder).branch_if(inline, (packed, &[]), (arena, &[]));
            builder.switch_to_block(packed);
            let mut stage = object_slots::emit(builder);
            let storage = stage.raw_load(Lane::I64, MemFlagsData::trusted(), object, OBJECT.payload_offset as i32);
            let plain = plain_payload(&mut stage, storage);
            stage.guard(plain, fallback);
            let slot = slot.read(stage.builder);
            let fits = stage.raw_compare_imm(Relation::UnsignedLess, slot, OBJECT.inline_property_count as i64);
            stage.guard(fits, fallback);
            let stride = stage.raw_const(Lane::I64, size_of::<u32>() as i64);
            let offset = stage.raw_binary(Binary::Mul, slot, stride);
            let address = record.offset_address(stage.builder, offset);
            let payload_offset = stage.raw_const(Lane::I64, OBJECT.payload_offset as i64);
            let address = stage.raw_binary(Binary::Add, address, payload_offset);
            stage.goto(packed_slot, &[address]);
            builder.seal_block(packed);
            builder.seal_block(arena);
            builder.switch_to_block(arena);
        }
        let mut stage = object_slots::emit(builder);
        if packed_slot.is_none() {
            let arena_storage = stage.raw_compare_imm(Relation::Equal, inline, 0);
            stage.guard(arena_storage, fallback);
        }
        // Plain storage has the arena start alone in the payload's lower half.
        let storage = stage.raw_load(Lane::I64, MemFlagsData::trusted(), object, OBJECT.payload_offset as i32);
        let half = stage.raw_const(Lane::I64, i64::from(u32::BITS));
        let upper = stage.raw_binary(Binary::ShiftRightUnsigned, storage, half);
        let plain = stage.raw_compare_imm(Relation::Equal, upper, 0);
        stage.guard(plain, fallback);
        storage
    };
    let address = if allow_holder {
        let own_block = builder.create_block();
        let holder_block = builder.create_block();
        let join = builder.create_block();
        builder.append_block_param(join, types::I64);
        object_slots::emit(builder).branch_if(own, (own_block, &[]), (holder_block, &[]));
        builder.switch_to_block(own_block);
        let storage = own_storage(builder);
        let address = arena.address(builder, storage, slot, fallback);
        object_slots::emit(builder).goto(join, &[address]);

        builder.switch_to_block(holder_block);
        let mut stage = object_slots::emit(builder);
        let link = proto_link(&mut stage, object);
        // The link names the prototype's stub granule (R4-B2b, option B): the record is reached by the hop.
        let granule = proto_object_record_id(&mut stage, link, fallback).expect("an emitted guard continues");
        let prototype = object_record_of(&mut stage, records, granule);
        let proto_shape = object_slots::exact_shape(stage.builder, MemFlagsData::trusted(), prototype);
        let same_holder = stage.raw_compare(Relation::Equal, proto_shape, holder);
        stage.guard(same_holder, fallback);
        let inline = inline_storage(&mut stage, prototype);
        let proto_storage = stage.raw_load(Lane::I64, MemFlagsData::trusted(), prototype, OBJECT.payload_offset as i32);
        let extras = has_extras(&mut stage, proto_storage);
        stage.guard(extras, fallback);
        let extras_pointer = extras_pointer(&mut stage, proto_storage);
        let present = stage.raw_compare_imm(Relation::NotEqual, extras_pointer, 0);
        stage.guard(present, fallback);
        let displaced = stage.builder.create_block();
        let in_arena = stage.builder.create_block();
        stage.branch_if(inline, (displaced, &[]), (in_arena, &[]));
        stage.builder.switch_to_block(displaced);
        let index = slot.read(stage.builder);
        let fits = stage.raw_compare_imm(Relation::UnsignedLess, index, OBJECT.inline_property_count as i64);
        stage.guard(fits, fallback);
        // Exact holder shape admits the data slot. Extras hold full Values;
        // keep this projection inside the same no-GC access as the live prototype.
        let stride = stage.raw_const(Lane::I64, OBJECT.property_value_size as i64);
        let offset = stage.raw_binary(Binary::Mul, index, stride);
        let address = stage.raw_binary(Binary::Add, extras_pointer, offset);
        let values_at = stage.raw_const(Lane::I64, OBJECT.extras_inline_values_offset as i64);
        let address = stage.raw_binary(Binary::Add, address, values_at);
        stage.goto(join, &[address]);
        stage.builder.switch_to_block(in_arena);
        let start = stage.load_u32(Region::Payload.mutable(), extras_pointer, OBJECT.extras_arena_start_offset as i32);
        let address = arena.address(stage.builder, start, slot, fallback);
        stage.goto(join, &[address]);
        builder.seal_block(own_block);
        builder.seal_block(holder_block);
        builder.seal_block(join);
        builder.switch_to_block(join);
        builder.block_params(join)[0]
    } else {
        let storage = own_storage(builder);
        arena.address(builder, storage, slot, fallback)
    };
    (address, record_id)
}

/// A word of the running dispatch's payload (`JsStepDispatch`).
fn emit_js_payload_word(builder: &mut FunctionBuilder<'_>, payload: ir::Value, offset: usize) -> ir::Value {
    use quench_runtime::staged::region::Region;
    let offset = i32::try_from(offset).expect("JS dispatch offset fits i32");
    object_slots::emit(builder).load_u64(Region::Dispatch.mutable(), payload, offset)
}

/// The lane helper the dispatch payload holds `offset` bytes in, as a call target (`EngineEntry::LaneHelper`).
fn emit_js_payload_target(builder: &mut FunctionBuilder<'_>, payload: ir::Value, offset: usize) -> quench_runtime::staged::machine::CallTarget<ir::Value> {
    use quench_runtime::staged::{machine::MachineAccess, region::Region};
    let offset = i32::try_from(offset).expect("JS dispatch offset fits i32");
    object_slots::emit(builder).entry_target(crate::compiled::slow_paths::EngineEntry::LaneHelper, regions::flags(Region::Dispatch.mutable()), payload, offset)
}

/// A header word (`staged::frame`'s `CONTEXT_*`, counted down from register 0) of the window at `window`.
fn emit_frame_word(builder: &mut FunctionBuilder<'_>, window: ir::Value, index: usize) -> ir::Value {
    use quench_runtime::staged::region::Region;
    let offset = -i32::try_from(8 * index).expect("frame context offset fits i32");
    object_slots::emit(builder).load_u64(Region::Frame.mutable(), window, offset)
}

/// A payload word fixed for a whole native run (`JsStepDispatch::vm`, `residual`): a nested
/// run that rebinds it restores it before its caller's code continues
/// (`Vm::run_nested_lane_method`), so the caller's code reads it once, and every helper call
/// shares that read instead of loading it again after each call.
fn emit_js_payload_fixed_word(builder: &mut FunctionBuilder<'_>, payload: ir::Value, offset: usize) -> ir::Value {
    use quench_runtime::staged::region::Region;
    let offset = i32::try_from(offset).expect("JS dispatch offset fits i32");
    object_slots::emit(builder).load_u64(Region::Dispatch.readonly(), payload, offset)
}

/// A record reservation's base in the dispatch payload (`JsStepDispatch::object_record_base`,
/// `function_record_base`): fixed for a whole native run, so the load merges and hoists.
fn emit_js_record_base(builder: &mut FunctionBuilder<'_>, payload: ir::Value, offset: usize) -> ir::Value {
    use quench_runtime::staged::region::Region;
    let offset = i32::try_from(offset).expect("JS dispatch offset fits i32");
    object_slots::emit(builder).load_u64(Region::RecordBases.readonly(), payload, offset)
}

fn emit_js_property_values(builder: &mut FunctionBuilder<'_>, payload: ir::Value) -> (ir::Value, ir::Value) {
    use quench_runtime::staged::{machine::Lane, region::Region};
    let mut stage = object_slots::emit(builder);
    let header = stage.raw_load(Lane::I64, invariant(), payload,
        std::mem::offset_of!(JsStepDispatch, head.property_buffer) as i32);
    let layout = quench_runtime::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT;
    let values = stage.load_u64(Region::HeapMeta.mutable(), header, layout.base as i32);
    let length = stage.load_u64(Region::HeapMeta.mutable(), header, layout.length as i32);
    (values, length)
}

/// Call a lane helper (`LaneHelpers`) with the payload's VM and residual first.
fn emit_lane_call(builder: &mut FunctionBuilder<'_>, helper: quench_runtime::staged::machine::CallTarget<ir::Value>, arguments: &[ir::Value]) -> ir::Value {
    object_slots::emit(builder).call_words(helper, arguments)
}

fn emit_lane_helper_call(
    builder: &mut FunctionBuilder<'_>,
    payload: ir::Value,
    helper: quench_runtime::staged::machine::CallTarget<ir::Value>,
    arguments: &[ir::Value],
) -> ir::Value {
    let vm = emit_js_payload_fixed_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.vm));
    let residual = emit_js_payload_fixed_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.residual));
    // All current helpers fit inline; larger future ABIs spill without a limit.
    let mut values = smallvec::SmallVec::<[ir::Value; 8]>::from_slice(&[vm, residual]);
    values.extend_from_slice(arguments);
    emit_lane_call(builder, helper, &values)
}

/// The mark list a store into record space `space` reads its owner's mark blocks from.
fn store_mark_list(space: quench_runtime::staged::StagedRecordSpace) -> quench_runtime::staged::barrier::MarkList {
    use quench_runtime::staged::barrier::MarkList;
    match space {
        quench_runtime::staged::StagedRecordSpace::Object => MarkList {
            blocks: std::mem::offset_of!(JsStepDispatch, head.object_mark_blocks),
            count: std::mem::offset_of!(JsStepDispatch, head.object_mark_block_count),
        },
        quench_runtime::staged::StagedRecordSpace::Function => MarkList {
            blocks: std::mem::offset_of!(JsStepDispatch, head.function_mark_blocks),
            count: std::mem::offset_of!(JsStepDispatch, head.function_mark_block_count),
        },
    }
}

/// What a lane body knows of its receiver from entry on: the record, exact shape and
/// storage mode found at entry, which only an adding store changes, after which the body reads
/// them again (`Lowering::reload_receiver`); and the property arena's base, which an
/// allocating helper may move, after which the body reloads it
/// (`Lowering::reload_property_arena`).
struct LaneReceiver {
    object: Variable,
    shape: Variable,
    inline: Variable,
    properties: Variable,
}

/// Encode a proved tagged int32; widening remains the shared store's responsibility.
fn emit_narrow_integer_write(builder: &mut FunctionBuilder<'_>, value: ir::Value,
    fallback: ir::Block, range: Option<typed_ir::IntRange>) -> ir::Value {
    use quench_runtime::staged::narrow::{integer_code, integer_fits};
    let mut stage = object_slots::emit(builder);
    if range.is_some_and(|range| typed_ir::IntRange::packed().contains(range)) {
        #[cfg(test)] PACKED_RANGE_PROOFS.with(|count| count.set(count.get() + 1));
    } else {
        let fits = integer_fits(&mut stage, value);
        stage.guard(fits, fallback);
    }
    integer_code(&mut stage, value)
}

/// The linked data symbol `symbol` as a global value of `builder`'s function.
fn linked_data_symbol(builder: &mut FunctionBuilder<'_>, symbol: u32) -> ir::GlobalValue {
    let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
        crate::native_object::NATIVE_DATA_NAMESPACE, symbol));
    builder.create_global_value(ir::GlobalValueData::Symbol {
        name: ir::ExternalName::user(name), offset: 0.into(), colocated: false, tls: false,
    })
}

/// The narrow decode's upper-word table in a live compile: this process's own table.
fn process_narrow_table(builder: &mut FunctionBuilder<'_>) -> ir::Value {
    object_slots::emit(builder).constant(quench_runtime::value::NarrowLayout::upper_words() as u64)
}

/// The narrow decode's upper-word table of a statically linked image: the object's copy.
fn linked_narrow_table(builder: &mut FunctionBuilder<'_>, symbol: u32) -> ir::Value {
    let global = linked_data_symbol(builder, symbol);
    object_slots::emit(builder).symbol_address(global)
}

/// `staged::object::decode_narrow_through`'s residual for the 32-bit narrow word `narrow` and the
/// table `address` it decodes through, passed to `join`.
fn emit_narrow_read(builder: &mut FunctionBuilder<'_>, narrow: ir::Value,
    address: ir::Value, join: ir::Block)
{
    use quench_runtime::staged::machine::{Lane, Unary};
    let table = address;
    let mut stage = object_slots::emit(builder);
    let narrow = stage.raw_unary(Unary::ZeroExtend(Lane::I64), narrow);
    let encoded = quench_runtime::staged::object::decode_narrow_through(&mut stage, narrow, table);
    stage.goto(join, &[encoded]);
}

/// Establish a lane body's `LaneReceiver` once, at entry: `NO_FIELD_RECEIVER` as its shape
/// where the receiver is no direct object or the heap bases are absent, which no folded
/// field matches.
/// `trusted`: the receiver fact holds (`dispatch_fast::receiver_trusted`), so `this` is a direct
/// object the binding checked once, and only the tables are tested.
fn emit_lane_receiver(builder: &mut FunctionBuilder<'_>, payload: ir::Value, this: ir::Value, trusted: bool) -> LaneReceiver {
    use quench_runtime::staged::machine::{Binary, Lane, Relation, Unary};
    let receiver = LaneReceiver {
        object: builder.declare_var(types::I64),
        shape: builder.declare_var(types::I32),
        inline: builder.declare_var(types::I8),
        properties: builder.declare_var(types::I64),
    };
    let records = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.object_record_base));
    let class_base = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.class_base));
    let (properties, _) = emit_js_property_values(builder, payload);
    builder.def_var(receiver.properties, properties);
    let (found, done) = (builder.create_block(), builder.create_block());
    // The payload is the stage's, so the record hop's class base reads the dispatch head (`Table::ClassSpace`).
    let mut stage = object_slots::Emit { builder, property_values: None, gate: None, payload: Some(payload), facts: Default::default() };
    let none = stage.raw_const(Lane::I32, i64::from(quench_runtime::vm::NO_FIELD_RECEIVER));
    let null = stage.raw_const(Lane::I64, 0);
    let outside = stage.raw_const(Lane::I8, 0);
    stage.builder.def_var(receiver.shape, none);
    stage.builder.def_var(receiver.inline, outside);
    stage.builder.def_var(receiver.object, null);
    // The direct object's granule must name a cell of the class space; its cell's header then names the record.
    let granule = stage.raw_unary(Unary::Narrow(Lane::I32), this);
    let granule = stage.raw_unary(Unary::ZeroExtend(Lane::I64), granule);
    let in_range = stage.raw_compare_imm(Relation::UnsignedLess, granule, i64::from(quench_runtime::staged::layout_offsets::CLASS_SPACE_GRANULES));
    let has_records = stage.raw_compare_imm(Relation::NotEqual, records, 0);
    let has_classes = stage.raw_compare_imm(Relation::NotEqual, class_base, 0);
    let has_properties = stage.raw_compare_imm(Relation::NotEqual, properties, 0);
    let ok = if trusted { in_range } else {
        let object = quench_runtime::staged::value::is_direct_object(&mut stage, this);
        stage.raw_binary(Binary::And, object, in_range)
    };
    let ok = stage.raw_binary(Binary::And, ok, has_records);
    let ok = stage.raw_binary(Binary::And, ok, has_classes);
    let ok = stage.raw_binary(Binary::And, ok, has_properties);
    stage.branch_if(ok, (found, &[]), (done, &[]));
    stage.builder.switch_to_block(found);
    let address = quench_runtime::staged::object::object_record_of(&mut stage, records, this);
    // The exact shape carries the dictionary bit, so a dictionary never matches.
    let shape = object_slots::exact_shape(stage.builder, MemFlagsData::trusted(), address);
    let inline = quench_runtime::staged::object::inline_storage(&mut stage, address);
    stage.builder.def_var(receiver.object, address);
    stage.builder.def_var(receiver.shape, shape);
    stage.builder.def_var(receiver.inline, inline);
    stage.goto(done, &[]);
    builder.switch_to_block(done);
    receiver
}

/// The host-specific semantic helpers a lane body calls where its native path does not
/// apply: the VM's own field-cache read and its logged own-field store, each the rule the
/// lane's handlers run (`Vm::cached_field_hit`, `Vm::primary_own_field_store_hit`).
macro_rules! lane_helper_inventory {
    ($($(#[$attr:meta])* $name:ident => $address:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum LaneHelper { $($(#[$attr])* $name),+ }
        impl LaneHelper {
            pub(crate) const COUNT: usize = [$(stringify!($name)),+].len();
            /// Each helper's name, by its index (`addresses`).
            const NAMES: [&'static str; Self::COUNT] = [$(stringify!($name)),+];
            /// The name of helper number `index` (`Symbol::Helper`), as an image's demand lists it.
            pub(crate) fn name(index: usize) -> Option<&'static str> { Self::NAMES.get(index).copied() }
            pub(crate) fn addresses() -> &'static [*const (); Self::COUNT] {
                &[$($address as *const ()),+]
            }
        }
        // A lane helper is a declared call target: by this process's address (a slow path), read
        // from a bound table, or by its linked symbol (`LaneHelpers::target`).
        impl quench_runtime::staged::machine::slow_path_seal::Declared for LaneHelper {}
        impl quench_runtime::staged::machine::EntrySource for LaneHelper {}
        impl quench_runtime::staged::machine::SlowPath for LaneHelper {
            #[inline]
            fn address(self) -> usize { Self::addresses()[self as usize] as usize }
        }

        /// Defines the symbol `quench_lane_helper_<Name>` of every lane helper: a tail
        /// jump to the helper, so a call through it is the helper's own (every register and the
        /// stack as the caller left them). These are the symbols an ahead image's object imports
        /// (`aot_image::object`, `lane_helper_symbol`). An executable's export macro instantiates
        /// this once (`export_native_js_operations!`); each trampoline lies in a section of its own,
        /// so the function itself runs nothing and the linker keeps only the ones an image names.
        #[doc(hidden)]
        #[inline(never)]
        // The labels are the exported symbols themselves, defined once: the one instantiation is
        // the executable's, never inlined.
        #[allow(named_asm_labels)]
        pub fn define_lane_helper_symbols() {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            // SAFETY: the template only lays out code in another section; it runs no instruction here.
            unsafe {
                core::arch::asm!(
                    $(concat!(".pushsection .text.quench_lane_helper_", stringify!($name), ",\"ax\",@progbits\n.globl quench_lane_helper_", stringify!($name), "\n.p2align 4\nquench_lane_helper_", stringify!($name), ":\njmp {", stringify!($name), "}\n.popsection"),)+
                    ".pushsection .text.quench_lane_helper_ahead_Step,\"ax\",@progbits\n.globl quench_lane_helper_ahead_Step\n.p2align 4\nquench_lane_helper_ahead_Step:\njmp {ahead_step}\n.popsection",
                    $($name = sym $address),+,
                    ahead_step = sym crate::dispatch_fast::ahead_general_step,
                    options(nomem, nostack, preserves_flags),
                );
            }
            #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
            // SAFETY: as above.
            unsafe {
                core::arch::asm!(
                    $(concat!(".pushsection .text.quench_lane_helper_", stringify!($name), ",\"ax\",@progbits\n.globl quench_lane_helper_", stringify!($name), "\n.p2align 4\nquench_lane_helper_", stringify!($name), ":\nb {", stringify!($name), "}\n.popsection"),)+
                    ".pushsection .text.quench_lane_helper_ahead_Step,\"ax\",@progbits\n.globl quench_lane_helper_ahead_Step\n.p2align 4\nquench_lane_helper_ahead_Step:\nb {ahead_step}\n.popsection",
                    $($name = sym $address),+,
                    ahead_step = sym crate::dispatch_fast::ahead_general_step,
                    options(nomem, nostack, preserves_flags),
                );
            }
            // Mach-O has no `.pushsection`; the trampolines are laid out in this function's own
            // section, which never runs (the instance exists for its symbols alone).
            #[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
            // SAFETY: as above. `.subsections_via_symbols` makes each trampoline its own
            // subsection, so `ld64 -dead_strip` keeps only the helpers an image names.
            unsafe {
                core::arch::asm!(
                    $(concat!(".globl _quench_lane_helper_", stringify!($name), "\n.p2align 2\n_quench_lane_helper_", stringify!($name), ":\nb {", stringify!($name), "}"),)+
                    ".globl _quench_lane_helper_ahead_Step\n.p2align 2\n_quench_lane_helper_ahead_Step:\nb {ahead_step}",
                    ".subsections_via_symbols",
                    $($name = sym $address),+,
                    ahead_step = sym crate::dispatch_fast::ahead_general_step,
                    options(nomem, nostack, preserves_flags),
                );
            }
            #[cfg(all(target_vendor = "apple", target_arch = "x86_64"))]
            // SAFETY: as above. `.subsections_via_symbols` makes each trampoline its own
            // subsection, so `ld64 -dead_strip` keeps only the helpers an image names.
            unsafe {
                core::arch::asm!(
                    $(concat!(".globl _quench_lane_helper_", stringify!($name), "\n.p2align 4\n_quench_lane_helper_", stringify!($name), ":\njmp {", stringify!($name), "}"),)+
                    ".globl _quench_lane_helper_ahead_Step\n.p2align 4\n_quench_lane_helper_ahead_Step:\njmp {ahead_step}",
                    ".subsections_via_symbols",
                    $($name = sym $address),+,
                    ahead_step = sym crate::dispatch_fast::ahead_general_step,
                    options(nomem, nostack, preserves_flags),
                );
            }
        }
    };
}

/// The symbol an ahead image's object names lane helper `index` by (`define_lane_helper_symbols`): the ahead entry of
/// `Step` (`ahead_general_step`, over the payload), else the helper's own.
pub(crate) fn lane_helper_symbol(index: usize) -> Option<String> {
    let name = LaneHelper::NAMES.get(index)?;
    Some(if index == LaneHelper::Step as usize { format!("quench_lane_helper_ahead_{name}") } else { format!("quench_lane_helper_{name}") })
}

/// The address an ahead image's helper `index` is in this process, as `lane_helper_symbol` names it: `ahead_general_step` for
/// `Step`, else the helper's own address.
pub(crate) fn ahead_helper_address(index: usize) -> Option<usize> {
    if index == LaneHelper::Step as usize { return Some(crate::dispatch_fast::ahead_general_step as *const () as usize); }
    LaneHelper::addresses().get(index).map(|address| *address as usize)
}
lane_helper_inventory! {
    FunctionCapture => function_capture_read_in_lane,
    /// A leave's boxing of an unboxed double (`box_number_in_lane`): out of line, as leaves are cold.
    BoxNumber => box_number_in_lane,
    FieldGet => field_get_in_lane,
    /// The count of the entry function's `arguments` (`body_arguments_length_in_lane`).
    ArgumentsLength => crate::dispatch_fast::body_arguments_length_in_lane,
    /// One kept `arguments` element (`body_argument_get_in_lane`).
    ArgumentGet => crate::dispatch_fast::body_argument_get_in_lane,
    FieldSet => field_set_in_lane,
    Call => crate::dispatch_fast::body_call_helper,
    /// A loop region's call (`dispatch_fast::region_call_in_lane`).
    RegionCall => crate::dispatch_fast::region_call_in_lane,
    IndexSet => index_set_in_lane,
    /// Room for a loop's append to a dense array (`array_append_room_in_lane`).
    AppendRoom => array_append_room_in_lane,
    /// Whether no prototype of an array holds an index (`prototypes_hold_no_index`), for a loop's
    /// element search (`Lowering::element_search`).
    PrototypesHoldNoIndex => prototypes_hold_no_index_in_lane,
    FunctionBinding => function_binding_in_lane,
    InterruptOwner => interrupt_owner_in_lane,
    GlobalGet => global_get_in_lane,
    FieldWriteAdmission => field_write_admission_in_lane,
    CallBudget => call_budget_in_lane,
    AdmittedFunctionBinding => admitted_function_binding_in_lane,
    /// An outer capture's read (`dispatch_fast::body_capture_in_lane`).
    CaptureGet => crate::dispatch_fast::body_capture_in_lane,
    /// A capture's store (`dispatch_fast::body_capture_store_in_lane`).
    CaptureSet => crate::dispatch_fast::body_capture_store_in_lane,
    /// Strict equality of two distinct strings or BigInts (`strict_equal_cells_in_lane`).
    StrictEqual => strict_equal_cells_in_lane,
    /// `instanceof` by its steady case (`instance_of_in_lane`).
    InstanceOf => instance_of_in_lane,
    /// A fused `typeof` test of a heap value (`typeof_is_in_lane`).
    TypeofIs => typeof_is_in_lane,
    /// The `typeof` name index of any value (`typeof_index_in_lane`).
    TypeofIndex => typeof_index_in_lane,
    /// A planned construction (`construct_in_lane`).
    Construct => constructions::construct_in_lane,
    ConstructReceiver => constructions::construct_receiver_in_lane,
    /// Whether a construct plan holds (`dispatch_fast::construct_plan_holds`).
    ConstructPlanHolds => constructions::construct_plan_holds_in_lane,
    /// A construction of the body's own, by the call protocol (`dispatch_fast::body_construct_helper`).
    ConstructCall => crate::dispatch_fast::body_construct_helper,
    /// A body's uncaught `throw`, stashed for its native callers and the lane (`compiled_raise`).
    Raise => compiled_raise,
    /// The exception a callee left in `Vm::lane_raised`, for a handler of the caller's own (`take_raised_in_lane`).
    TakeRaised => take_raised_in_lane,
    /// An object literal's and an array literal's allocation.
    NewObject => compiled_new_object,
    NewArray => new_array_in_lane,
    /// An array literal of program constants (`new_const_array_in_lane`).
    NewConstArray => new_const_array_in_lane,
    /// A two-property object literal's allocation (`new_object_pair_in_lane`).
    NewObjectPair => new_object_pair_in_lane,
    Binary => binary_in_lane,
    /// A scalar native's call on its in-place case (`lane_allocations::scalar_native_in_lane`).
    ScalarNative => scalar_native_in_lane,
    /// A store that may add its property (`field_add_in_lane`).
    FieldAdd => field_add_in_lane,
    /// The slow arm of a loop's hoisted typed array view proof (`typed_array_view_in_lane`).
    TypedArrayView => typed_array_view_in_lane,
    /// `+` of strings (`dispatch_fast::body_string_concat`), named by the b-jit call planner
    /// (`base_region::call_plan`), which the whole-function planner will consume.
    #[allow(dead_code)]
    StringConcat => crate::dispatch_fast::body_string_concat,
    /// A VM operation a staged row calls out for (`dispatch_fast::vm_operation_step`).
    VmStep => crate::dispatch_fast::vm_operation_step,
    /// A fixed-shape object the record defines, made by the lane's allocator (`dispatch_fast::lane_allocate_step`).
    AllocateStep => crate::dispatch_fast::lane_allocate_step,
    /// An instruction whose general step runs in place (`dispatch_fast::body_general_step`), which the b-jit's
    /// helper steps and stepped calls make (`base_region::steps`).
    Step => crate::dispatch_fast::body_general_step,
    /// `Step` for an instruction the planner decoded (`CallRow::HelperPacked`): a covered family runs it directly
    /// (`dispatch_fast::lane_body_step_packed`).
    StepPacked => crate::dispatch_fast::lane_body_step_packed,
    /// ToBoolean of a value its cell decides (`truthy_in_lane`).
    Truthy => truthy_in_lane,
    /// The array storage proofs a shorter array falsifies (`retire_array_proofs_in_lane`).
    RetireArrayProofs => retire_array_proofs_in_lane,
    /// A native header Rust observed (spilled or linked) pops where Rust keeps it (`pop_observed_header_in_lane`).
    PopObservedHeader => pop_observed_header_in_lane,
    /// An object literal over a register window (`new_object_literal_in_lane`).
    NewObjectLiteral => new_object_literal_in_lane,
    /// A closure over the frame's environment (`new_closure_in_lane`).
    NewClosure => new_closure_in_lane,
    /// A literal template's value (`new_template_in_lane`).
    NewTemplate => new_template_in_lane,
    /// A string concatenation of pure parts (`new_concat_in_lane`).
    NewConcat => new_concat_in_lane,
    /// `+` of two operands that become strings without guest code (`concat_pair_in_lane`).
    ConcatPair => concat_pair_in_lane,
    /// A rope's flat form as a new string of its own, for a loop's entry (`flatten_string_in_lane`).
    FlattenString => flatten_string_in_lane,
    /// A regexp literal's fresh object of its site's template (`new_regexp_in_lane`).
    NewRegExp => lane_new_regexp,
    /// A decision of two words the call-out carries, which needs no VM record
    /// (`dispatch_fast::decide_comparison`).
    Decide => crate::dispatch_fast::decide_comparison,
    /// A handler's entry in the frame, for the exception it catches (`dispatch_fast::handler_edge`).
    HandlerEdge => crate::dispatch_fast::handler_edge,
    /// The runtime's Number remainder leaf (`RuntimeSlowPath::NumberRemainder`), which a body naming
    /// its helpers by symbol calls by this one (`slow_path_symbol`).
    NumberRemainder => quench_runtime::vm::operations::number_remainder,
    /// A local, capture or name access's step over the running window or frame (`dispatch_fast::owner_step`): the owner
    /// step alone, which a helper row or slow arm of such an instruction makes in place of the general step.
    OwnerStep => crate::dispatch_fast::owner_step,
    /// An operator or comparison jump over registers and constants (`base_body::value_step`): the VM's operator on their
    /// values, in place of the general step.
    ValueStep => base_body::value_step,
    AllocationStep => base_body::allocation_step,
    ThrowStep => base_body::throw_step,
    CallOutStep => base_body::call_out_step,
    /// A collection lookup a call runs by its callee's identity (`lane_allocations::collection_lookup_in_lane`).
    CollectionLookup => collection_lookup_in_lane,
}

thread_local! {
    /// While a body names its helpers by symbol (`LaneHelpers::names_symbols`): whether it folded a slow
    /// path no helper names, an address of this process its code must not hold.
    static NAMING_SYMBOLS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// A lowering whose code names its helpers by symbol: the slow paths it calls become helper symbols
/// (`slow_path_symbol`), and one that has no helper fails it (`folded`).
pub(crate) struct NamingSymbols(Option<bool>);

impl NamingSymbols {
    pub(crate) fn enter(helpers: LaneHelpers) -> Self {
        Self(NAMING_SYMBOLS.with(|naming| naming.replace(helpers.names_symbols().then_some(false))))
    }
    /// Whether the lowering folded a slow path's process address.
    pub(crate) fn folded(&self) -> bool { NAMING_SYMBOLS.with(|naming| naming.get() == Some(true)) }
}

impl Drop for NamingSymbols {
    fn drop(&mut self) { NAMING_SYMBOLS.with(|naming| naming.set(self.0)); }
}

/// The helper symbol a body naming its helpers by symbol calls slow path `address` by, or `None` where it
/// takes the address itself (a live body, or a slow path no helper names, which fails the lowering).
pub(crate) fn slow_path_symbol(func: &mut ir::Function, address: usize) -> Option<ir::GlobalValue> {
    NAMING_SYMBOLS.with(|naming| {
        naming.get()?;
        let helper = LaneHelper::addresses().iter().position(|helper| *helper as usize == address);
        if helper.is_none() { naming.set(Some(true)); }
        Some(quench_runtime::vm::jit_backend::declare_symbol(func, RelocKind::Helper as u32, helper? as u32))
    })
}

unsafe extern "C" fn interrupt_owner_in_lane(context: u64) -> u64 {
    // SAFETY: the running entry binds this owner for its whole native call.
    let context = unsafe { &*(context as *const CallContext) };
    u64::from(unsafe { context.interrupt_owner.as_ref() }.is_some_and(|owner| owner.exclusive_and_clear()))
}

#[cfg(test)]
thread_local! {
    static FIELD_WRITE_ADMISSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FIELD_HELPER_CALLS: std::cell::Cell<[usize; 2]> = const { std::cell::Cell::new([0; 2]) };
    static FUNCTION_BINDING_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CALL_BUDGET_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FIELD_VALUE_FORWARDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FIELD_FORWARD_SITE: std::cell::Cell<Option<(u16, usize)>> = const { std::cell::Cell::new(None) };
    static LOOP_INT32_READ_SITE: std::cell::Cell<Option<(u16, usize)>> = const { std::cell::Cell::new(None) };
    static LOOP_INT32_PHIS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LOOP_SLOT_STORES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PACKED_RANGE_PROOFS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PACKED_RANGE_SITE: std::cell::Cell<Option<(u16, usize)>> = const { std::cell::Cell::new(None) };
    static INT32_PROJECTION_REUSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// An object's atom table is the data symbol after its narrow table.
pub(crate) const NATIVE_ATOM_TABLE_OFFSET: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneHelpers {
    /// A frameless Number region: no VM helper capability, helpers named by symbol.
    Numeric,
    Process(&'static [*const (); LaneHelper::COUNT]),
    /// The compile target "ahead": every run-time address `RelocKind` names is a symbol (`Process`, "live", embeds them).
    Ahead,
    Linked(u32, u32),
    Bound(ir::Value, u32),
}
/// The process helper table a JIT publication binds.
pub(crate) fn process_lane_helpers() -> LaneHelpers {
    LaneHelpers::Process(LaneHelper::addresses())
}

impl LaneHelpers {
    /// Whether this is a frameless Number region, which has no VM helper capability.
    fn is_numeric(self) -> bool {
        matches!(self, Self::Numeric)
    }

    /// Whether helper addresses are symbols a loader resolves, not addresses of this process.
    pub(crate) fn links(self) -> bool {
        !matches!(self, Self::Process(_))
    }

    /// The word of program atom `atom`: its id now for a live lowering; for a linking one the
    /// symbol of an ahead image, or the object's atom table entry (`NATIVE_ATOM_TABLE_OFFSET`
    /// symbols after its narrow table) the loader's namespace check proves to hold the id.
    pub(crate) fn atom_word(self, builder: &mut FunctionBuilder<'_>, atom: u32) -> ir::Value {
        match self {
            Self::Process(_) => object_slots::emit(builder).constant(u64::from(atom)),
            Self::Numeric => lane_region::atom_symbol(builder, atom),
            // An ahead image's atom is a word beside its code (`Symbol::Atom`), which the loader
            // fills with the running VM's id: an image's atom numbers are its build's, not this
            // VM's, and an `adrp` can reach a word but not an immediate.
            Self::Ahead => {
                use quench_runtime::staged::machine::{Lane, Unary};
                let base = lane_region::atom_symbol(builder, atom);
                let mut stage = object_slots::emit(builder);
                let word = stage.raw_load(Lane::I32, invariant(), base, 0);
                stage.raw_unary(Unary::ZeroExtend(Lane::I64), word)
            }
            Self::Linked(_, narrow) | Self::Bound(_, narrow) => {
                use quench_runtime::staged::machine::{Lane, Unary};
                let table = linked_data_symbol(builder, narrow + NATIVE_ATOM_TABLE_OFFSET);
                let mut stage = object_slots::emit(builder);
                let base = stage.symbol_address(table);
                let word = stage.raw_load(Lane::I32, invariant(), base, (atom as usize * std::mem::size_of::<u32>()) as i32);
                stage.raw_unary(Unary::ZeroExtend(Lane::I64), word)
            }
        }
    }

    /// `helper` as a call target: its symbol, or the table's entry (a symbol per use in live lane
    /// bodies costs raytrace compile Ir +17.5%, run-only +0.6%).
    fn target(self, builder: &mut FunctionBuilder<'_>, helper: LaneHelper) -> quench_runtime::staged::machine::CallTarget<ir::Value> {
        if self.names_symbols() {
            let symbol = quench_runtime::vm::jit_backend::declare_symbol(builder.func, RelocKind::Helper as u32, helper as u32);
            return object_slots::emit(builder).symbol_target(helper, symbol);
        }
        let mut stage = object_slots::emit(builder);
        match self.table_address(helper) {
            HelperAddress::Embedded => stage.slow_path_target(helper),
            HelperAddress::Loaded { table, offset } => stage.entry_target(helper, invariant(), table, offset),
        }
    }

    /// Whether helpers are symbols a loader resolves, not table entries.
    pub(crate) fn names_symbols(self) -> bool {
        matches!(self, Self::Numeric | Self::Ahead)
    }

    /// Where the address of `helper` in this process's table is: embedded, or loaded from the bound one.
    fn table_address(self, helper: LaneHelper) -> HelperAddress {
        match self {
            Self::Numeric => unreachable!("a frameless Number region names its helper by symbol"),
            Self::Ahead => unreachable!("an ahead region names its helper by symbol"),
            Self::Process(_) => HelperAddress::Embedded,
            Self::Bound(table, _) => HelperAddress::Loaded {
                table, offset: (helper as usize * std::mem::size_of::<usize>()) as i32 },
            Self::Linked(..) => unreachable!("linked helpers bind at region entry"),
        }
    }
}

/// Where a helper's address is: a constant, or an entry of the table a region bound.
#[derive(Clone, Copy)]
enum HelperAddress {
    Embedded,
    Loaded { table: ir::Value, offset: i32 },
}

/// Immutable host-specific helper addresses, resolved by the running image.
/// The table and every code pointer have process lifetime; obtaining it has no effects.
#[cfg(feature = "native-execution")]
pub fn native_js_lane_helpers() -> *const () {
    LaneHelper::addresses().as_ptr().cast()
}

pub(crate) fn native_lane_helpers_signature(isa: &dyn cranelift_codegen::isa::TargetIsa) -> ir::Signature {
    call_signature::word_signature(isa.default_call_conv(), 0, 1, isa.pointer_type())
}

#[derive(Clone, Copy)]
struct NullishComparison {
    source: Register,
    expected: Value,
    strict: bool,
    negated: bool,
}

/// Resolve the constant operand and equality denotation before execution.
fn nullish_comparison(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction,
    register_type: impl Fn(Register) -> typed_ir::Type) -> Option<NullishComparison> {
    if !meaning_compiled::is_jump_binary_false(instruction) { return None; }
    let nullish = |operand: Operand| {
        let ty = facts::operand_type(constants, operand, &register_type);
        if ty == typed_ir::Type::language(quench_runtime::ValueType::Null) { Some(Value::NULL) }
        else if ty == typed_ir::Type::language(quench_runtime::ValueType::Undefined) { Some(Value::UNDEFINED) }
        else { None }
    };
    let (left, right) = (instruction.operand_b(), instruction.operand_c());
    let (expected, source) = nullish(left).zip(right.register_index())
        .or_else(|| nullish(right).zip(left.register_index()))?;
    let (strict, negated) = quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field())?;
    Some(NullishComparison { source, expected, strict, negated })
}

/// What the online division may fold as Stable facts of one VM, and where the code learns
/// that one changed (`typed_ir::BindingTime::Stable`).
#[derive(Clone, Copy)]
pub(crate) struct OnlineFacts<'a> {
    /// The names method caches hold now.
    pub(crate) method_cache_atoms: &'a quench_runtime::vm::method_cache::AtomBits,
    /// The address of the code's invalidation word (`code_dependencies::Invalidation`).
    pub(crate) invalidation: usize,
    /// The program's constants as the VM made and roots them (`Vm::active_constant`).
    pub(crate) constants: &'a [Value],
    /// What the VM proved of a constructor's `instanceof` (`Vm::instanceof_fact`), by its identity.
    pub(crate) instanceofs: &'a dyn Fn(Value) -> Option<quench_runtime::code_dependencies::InstanceofFact>,
    /// Whether code may hold an object's exact shape past a call (`Vm::shape_is_stable`).
    pub(crate) shape_stable: &'a dyn Fn(u32) -> bool,
    /// The naming a cache site recorded for a `SetFunctionName`'s atom and the word of the name's string
    /// (`Vm::native_function_name`), whose staged row names a function; `None` where none may be held.
    pub(crate) names: Option<&'a dyn Fn(u16, Atom) -> Option<(quench_runtime::staged::add_plan::Transition, u64)>>,
    /// The own member of a constant-marked object that the VM holds for `atom` (`Vm::own_constant_of`):
    /// a read of it folds under an identity guard and a dependency on the object's exact shape.
    pub(crate) own_constant: &'a dyn Fn(Atom) -> Option<OwnConstant>,
    /// The root constant a capture read at a PC takes (`FactTables::root_constant`): a written-once root
    /// binding's function, with the program whose root it is; `None` where none may be held.
    pub(crate) root_constant: &'a dyn Fn(usize) -> Option<(quench_runtime::vm::program_store::ProgramId, Value)>,
}

pub(crate) use quench_runtime::vm::engine_types::OwnConstant;

/// A construct plan whose receiver is its fields alone (`ConstructPlanKind::Passed`): its
/// callee, its receiver's exact shape and, per field in slot order, the field's name and where
/// its value comes from. A body that reads such a receiver's fields and lets it escape nowhere
/// keeps the values and allocates the receiver only at a leave.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScalarConstructPlan {
    pub(crate) callee: PlanCallee,
    /// The receiver's shape: its id, or for an image's plan (`PlanCallee::Image`) its
    /// `Symbol::Shape` key, which the loader interns.
    pub(crate) shape: u32,
    pub(crate) fields: smallvec::SmallVec<[(Atom, ScalarField); 4]>,
}

/// The callee a `ScalarConstructPlan` constructs by, which its code guards by identity: a closure
/// the VM made, or in an ahead image the `Symbol::HeapValue` key of the closure the closed world
/// proves the site constructs (`HeapValueRecipe::ClosureOf`), which the loader resolves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlanCallee {
    Live(Value),
    Image(u32),
}

impl PlanCallee {
    /// The closure, where the VM made it.
    pub(crate) fn live(self) -> Option<Value> {
        match self { Self::Live(callee) => Some(callee), Self::Image(_) => None }
    }
}

/// Where a field of a `ScalarConstructPlan` takes its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ScalarField {
    /// The argument of this parameter (`undefined` past the passed ones).
    Parameter(u16),
    /// This constant, whatever is passed.
    Constant(Value),
}

impl OnlineFacts<'_> {
    /// Whether a method cache names `atom` now.
    pub(crate) fn names(&self, atom: Atom) -> bool {
        self.method_cache_atoms.contains(atom as usize)
    }
}

/// Whether `atom` of `program` names `length`, which a direct array answers from its record.
/// The program's own name decides, so code compiled for no particular VM (a static image's)
/// reads it as natively as a VM's own compile does.
pub(crate) fn names_length(program: &ResidualProgram, atom: Atom) -> bool {
    program.atoms.get(atom as usize) == Some("length")
}

/// Whether `instruction` is a plain read of `length`, which its lowering answers for any base
/// (`Lowering::array_length`), so the base needs no object record.
pub(crate) fn reads_length(program: &ResidualProgram, instruction: WideInstruction) -> bool {
    // Only a field op reads a `length`: `field_lookup` proves that of its reader.
    instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::FieldLookup
        && !instruction.writes_current_this() && !instruction.returns_from_frame()
        && matches!(instruction.field_lookup(), Some(FieldLookup::Atom { atom, .. }) if names_length(program, atom))
}

/// The facts of the VM a lowering specializes to, fixed when the VM is built.
#[derive(Clone, Copy)]
pub(crate) struct VmFacts<'a> {
    /// Whether the host provides the IsHTMLDDA object (`CapabilityId::IsHTMLDDA`): without
    /// it, loosely only `null` and `undefined` equal `null`.
    pub(crate) html_dda: bool,
    /// Whether leaves record the guard they took (`JsStepDispatch::exit_line`).
    pub(crate) diagnostics: bool,
    /// The thresholds the compiling VM's tier controllers read: a Static fact of the code it makes.
    pub(crate) tiers: &'static Tiers,
    /// The Stable facts the code may fold under a code dependency: `None` for code that takes
    /// no dependency on this VM (`Division::Offline`).
    pub(crate) online: Option<OnlineFacts<'a>>,
    /// Whether those Stable facts come from a closed whole-program image rather than from a
    /// running VM's feedback: `Division::Frozen` instead of `Division::Online`.
    pub(crate) frozen: bool,
    /// Whole-program prototype values by atom, each guarded at its read site.
    pub(crate) frozen_fields: Option<&'a quench_runtime::dense_map::DenseMap<Value>>,
    /// The image's constant block, materialized after realm initialization.
    pub(crate) frozen_constants: Option<usize>,
    /// The scalar entry a lane body's direct calls of its own function call (`lane_region::scalar`).
    pub(crate) scalar: Option<lane_region::scalar::ScalarAbi>,
    /// The names method caches hold now, where offline code stores in place under a dependency on
    /// the atoms it stores staying unnamed (`OsrFacts::stable`): a loop region compiled for a
    /// specialized run, which a cache naming one retires (`MethodCacheAtoms::name`).
    pub(crate) held_atoms: Option<HeldAtoms<'a>>,
    /// The construct plan a construction record of a function holds now, by function and PC
    /// (`ScalarConstructPlan`): what code may keep as its field values while the object
    /// escapes nowhere (`lane_region::virtual_objects`). The code tests that the plan still
    /// holds where it would construct, so it takes no dependency on it.
    pub(crate) construct_plans: Option<&'a dyn Fn(u32, u32) -> Option<ScalarConstructPlan>>,
    /// Where an ahead lane body's leaves continue, in its base body (`lane_region::AheadLeaves`);
    /// `None` where a leave returns its exit word.
    pub(crate) ahead: Option<lane_region::AheadLeaves<'a>>,
    /// The whole-program facts of the program a lane body lowers (`compile::aot_facts`, the program
    /// store's one solve): its call targets admit the callees the unit inlines (`Lowering::plan_call`),
    /// its Number registers are entry types (`int_inputs::solved_entry_types`), and its register cells
    /// narrow the lowering's facts (`lane_region::narrow_to_cells`).
    pub(crate) facts: Option<&'a quench_runtime::compile::aot_facts::AotFacts>,
    /// Whether a lane body's entry is its proven entry (`lane_region::LoweredLane::proven`): its parameters' entry guards
    /// are taken as proved by the caller, so the lowering states them as held rather than testing them.
    pub(crate) proven_entry: bool,
}

/// What offline code holds of the atoms it stores (`VmFacts::held_atoms`): the names method caches
/// hold now, and the word its publication sets when a cache names one it stores
/// (`code_dependencies::Invalidation`), which a run that calls reads after each call.
#[derive(Clone, Copy)]
pub(crate) struct HeldAtoms<'a> {
    pub(crate) names: &'a quench_runtime::vm::method_cache::AtomBits,
    pub(crate) invalidation: usize,
}

impl VmFacts<'static> {
    /// The facts of no particular VM, for code shared across VMs, as a static image's is:
    /// an IsHTMLDDA object possible, no diagnostics, no known `call`, and no Stable facts.
    pub(crate) const STATIC: Self = Self { html_dda: true, diagnostics: false, tiers: tier_policy::STATIC_ROWS, online: None, frozen: false,
        frozen_fields: None, frozen_constants: None, scalar: None, held_atoms: None, construct_plans: None, ahead: None, facts: None, proven_entry: false };
}

#[quench_macros::ext(CompiledVm)]
impl Vm {
    pub(crate) fn vm_facts(&self) -> VmFacts<'static> {
        let html_dda = self.host.globals().iter().any(|global| global.capability == quench_runtime::host::CapabilityId::IsHTMLDDA);
        VmFacts { html_dda, diagnostics: admission::diagnosing(self), tiers: self.programs.tiers(), online: None, frozen: false,
            frozen_fields: None, frozen_constants: None, scalar: None, held_atoms: None, construct_plans: None, ahead: None, facts: None, proven_entry: false }
    }
}

pub use quench_runtime::vm::engine_types::ExecutionMode;


/// A value's width inside compiled code. Each register has an integer and a
/// float view per width (`View`); a write defines both, so the analysis
/// tracks only the width and a reinterpretation is free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    I32,
    I64,
    /// A reference (funcref, externref, anyref, GC references). Its Wasm bits are its Value bits,
    /// held in a 64-bit word like an `I64`; the analysis keeps it apart so only reference
    /// operations read it, and the frame roots its registers across calls (`wasm_roots`).
    Reference,
}

impl Kind {
    #[inline]
    fn of(ty: quench_runtime::WasmType) -> Option<Self> {
        match ty {
            quench_runtime::WasmType::I32 | quench_runtime::WasmType::F32 => Some(Self::I32),
            quench_runtime::WasmType::I64 | quench_runtime::WasmType::F64 => Some(Self::I64),
            quench_runtime::WasmType::Reference { .. } => Some(Self::Reference),
            _ => None,
        }
    }

    fn ir(self) -> ir::Type {
        match self {
            Self::I32 => types::I32,
            Self::I64 | Self::Reference => types::I64,
        }
    }

    /// The Wasm type of the register's integer view: a reference's bits live in the `I64` view.
    fn integer(self) -> quench_runtime::WasmType {
        match self {
            Self::I32 => quench_runtime::WasmType::I32,
            Self::I64 | Self::Reference => quench_runtime::WasmType::I64,
        }
    }

    fn lane(self) -> quench_runtime::staged::machine::Lane {
        use quench_runtime::staged::machine::Lane;
        match self {
            Self::I32 => Lane::I32,
            Self::I64 | Self::Reference => Lane::I64,
        }
    }
}

/// The Value of a Wasm word `bits` of type `ty`, as the callout ABI carries it: an i32 from its
/// low word, a 64-bit number boxed where its payload does not fit, and a reference as its Value
/// bits (`WasmValue` has no reference word; `encode_wasm_value` gives the reference's Value).
fn wasm_word_value(vm: &mut Vm, ty: quench_runtime::WasmType, bits: u64) -> Value {
    match Kind::of(ty) {
        Some(Kind::Reference) => Value::from_bits(bits),
        Some(Kind::I32) => vm.encode_wasm_value(quench_runtime::WasmValue::from_bits(ty, u64::from(bits as u32))),
        _ => vm.encode_wasm_value(quench_runtime::WasmValue::from_bits(ty, bits)),
    }
}

/// The views of a register's value, integer and float per width, in the
/// order of a register's variables.
const VIEW_TYPES: [quench_runtime::WasmType; 4] = [
    quench_runtime::WasmType::I32,
    quench_runtime::WasmType::I64,
    quench_runtime::WasmType::F32,
    quench_runtime::WasmType::F64,
];

/// The variable index of a register's view as Wasm type `ty`.
fn view(ty: quench_runtime::WasmType) -> usize {
    VIEW_TYPES
        .iter()
        .position(|candidate| *candidate == ty)
        .expect("compiled values are numeric scalars")
}

/// The machine type of a register's view. Unsigned address offsets stay extended
/// across loop parameters: Cranelift elides extension of x64 arithmetic results,
/// but not loop parameters. Signed arithmetic stays i32 and widens at consumers,
/// so loop-carried values do not require a sign extension on every definition.
fn view_ir(ty: quench_runtime::WasmType, extension: Int32Extension) -> ir::Type {
    match ty {
        quench_runtime::WasmType::I32 if extension == Int32Extension::Zero => types::I64,
        _ => scalar_ir(ty),
    }
}

/// The machine type of a scalar Wasm type.
fn scalar_ir(ty: quench_runtime::WasmType) -> ir::Type {
    match ty {
        quench_runtime::WasmType::I32 => types::I32,
        quench_runtime::WasmType::I64 => types::I64,
        quench_runtime::WasmType::F32 => types::F32,
        quench_runtime::WasmType::F64 => types::F64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// The type of the other view of the same bits.
fn counterpart(ty: quench_runtime::WasmType) -> quench_runtime::WasmType {
    match ty {
        quench_runtime::WasmType::I32 => quench_runtime::WasmType::F32,
        quench_runtime::WasmType::I64 => quench_runtime::WasmType::F64,
        quench_runtime::WasmType::F32 => quench_runtime::WasmType::I32,
        quench_runtime::WasmType::F64 => quench_runtime::WasmType::I64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// What a register holds at an instruction, as the analysis derives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Value(Kind),
    /// An unsigned memory32 index plus a validated memory32 offset, without wrapping.
    /// Moves preserve this proof; arithmetic and mixed joins discard it.
    Memory32Address,
    /// A 128-bit SIMD value. Only SIMD instructions, moves and vector accesses read it, so
    /// it never meets a call, a return or a scalar view (`Emitter::vector`).
    Vector,
    /// An instance binding: the environment slot it was loaded from.
    Binding(u32),
    /// The function's memory binding; only direct accesses read it.
    Memory,
    /// A reference to imported function `index`; only a call reads it.
    Import(u32),
    /// An indirect table call, with its table binding and expected type.
    IndirectReference {
        signature: u32,
        table: u32,
    },
    /// Paths disagree; no compiled instruction may read it.
    Conflict,
}

impl Slot {
    const DEFAULT: Self = Self::Value(Kind::I32);

    fn join(self, other: Self) -> Self {
        match (self, other) {
            (a, b) if a == b => a,
            (Self::Memory32Address, Self::Value(Kind::I64))
            | (Self::Value(Kind::I64), Self::Memory32Address) => Self::Value(Kind::I64),
            _ => Self::Conflict,
        }
    }
}

/// Register facts at one bytecode position. Most Wasm temporaries have the
/// default i32 representation, so retaining a full `registers`-sized vector
/// at every reachable instruction wastes memory. This stores only deviations
/// from that default; joins apply the same `Slot` lattice to every register.
#[derive(Clone, Debug, PartialEq)]
struct AnalysisState {
    slots: Vec<(u16, Slot)>,
}

impl AnalysisState {
    fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn get(&self, register: u16, registers: u16) -> Option<Slot> {
        if register >= registers {
            return None;
        }
        Some(
            self.slots
                .binary_search_by_key(&register, |(index, _)| *index)
                .map(|index| self.slots[index].1)
                .unwrap_or(Slot::DEFAULT),
        )
    }

    fn set(&mut self, register: u16, registers: u16, slot: Slot) -> Option<()> {
        if register >= registers {
            return None;
        }
        match self
            .slots
            .binary_search_by_key(&register, |(index, _)| *index)
        {
            Ok(index) if slot == Slot::DEFAULT => {
                self.slots.remove(index);
            }
            Ok(index) => self.slots[index].1 = slot,
            Err(index) if slot != Slot::DEFAULT => {
                self.slots.insert(index, (register, slot));
            }
            Err(_) => {}
        }
        Some(())
    }

    fn join_from(&mut self, incoming: &Self) -> bool {
        let mut joined = Vec::with_capacity(self.slots.len() + incoming.slots.len());
        let (mut left, mut right) = (0, 0);
        while left < self.slots.len() || right < incoming.slots.len() {
            let left_index = self.slots.get(left).map(|(index, _)| *index);
            let right_index = incoming.slots.get(right).map(|(index, _)| *index);
            let register = match (left_index, right_index) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => unreachable!(),
            };
            let current = if left_index == Some(register) {
                let slot = self.slots[left].1;
                left += 1;
                slot
            } else {
                Slot::DEFAULT
            };
            let other = if right_index == Some(register) {
                let slot = incoming.slots[right].1;
                right += 1;
                slot
            } else {
                Slot::DEFAULT
            };
            let slot = current.join(other);
            if slot != Slot::DEFAULT {
                joined.push((register, slot));
            }
        }
        let changed = joined != self.slots;
        self.slots = joined;
        changed
    }
}

/// Traps compiled code can raise, by status (index + 1); status 0 is a
/// normal return.
use quench_runtime::wasm::trap::COMPILED_TRAPS as TRAPS;

use crate::compiled::tier_policy::{MAX_JIT_ROOT_FUNCTIONS, MAX_JIT_ROOT_BYTECODES_PER_FUNCTION, MAX_JIT_ROOT_CODE_BYTES, MAX_JIT_PROFILED_INDIRECT_TARGETS, THREAD_BUDGET, UNROLL_BUDGET, MAX_UNROLL_FACTOR, WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT, JS_TIER_LANE_ENTRY_RECORDS};

fn jit_root_override() -> Result<Option<usize>, ()> {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return Ok(mode.root);
    }
    quench_runtime::trace::Dev::WasmRoot.value().map(|root| root.parse::<usize>().map_err(|_| ())).transpose()
}

/// Whether Wasm modules stay selective for good, compiling only their hot roots
/// (`WasmModuleCompile::NEVER`): an A/B arm against the default, which compiles a long runner
/// whole.
pub(crate) fn jit_wasm_selective_enabled() -> bool {
    quench_runtime::trace::Dev::WasmSelective.on()
}

fn jit_wasm_root_only_enabled() -> bool {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return mode.root_only;
    }
    quench_runtime::trace::Dev::WasmRootOnly.on()
}

fn jit_wasm_root_first_enabled() -> bool {
    #[cfg(test)]
    if let Some(mode) = WASM_JIT_MODE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return mode.root_first;
    }
    quench_runtime::trace::Dev::WasmRootFirst.on()
}

#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct WasmJitModeTestOverride {
    root_only: bool,
    root_first: bool,
    root: Option<usize>,
}

#[cfg(test)]
thread_local! {
    static WASM_JIT_MODE_TEST_OVERRIDE: std::cell::Cell<Option<WasmJitModeTestOverride>> =
        const { std::cell::Cell::new(None) };
    static WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Whether the eager artifact compiles functions holding `unreachable` (C asserts and aborts,
/// which most Emscripten functions hold): the trap lowers to a return of the trap status. A test
/// may reject them to keep such a function for the tier.
fn eager_unreachable_admitted() -> bool {
    #[cfg(test)]
    if WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST.with(std::cell::Cell::get) {
        return false;
    }
    true
}

#[cfg(test)]
pub(crate) fn with_eager_unreachable_rejected_for_test<R>(run: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST.with(|rejected| rejected.set(self.0));
        }
    }
    let previous =
        WASM_JIT_EAGER_UNREACHABLE_REJECTED_FOR_TEST.with(|rejected| rejected.replace(true));
    let _restore = Restore(previous);
    run()
}

#[cfg(test)]
pub(crate) fn with_wasm_jit_mode_for_test<R>(
    root_only: bool,
    root_first: bool,
    root: Option<usize>,
    run: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<WasmJitModeTestOverride>);

    impl Drop for Restore {
        fn drop(&mut self) {
            WASM_JIT_MODE_TEST_OVERRIDE.with(|override_mode| override_mode.set(self.0));
        }
    }

    let previous = WASM_JIT_MODE_TEST_OVERRIDE.with(|override_mode| {
        override_mode.replace(Some(WasmJitModeTestOverride {
            root_only,
            root_first,
            root,
        }))
    });
    let _restore = Restore(previous);
    run()
}

fn jit_root_opt_level(root: Option<usize>, root_only: bool) -> Result<&'static str, ()> {
    if root.is_none() {
        return Ok("speed");
    }
    parse_jit_root_opt_level(root_only, quench_runtime::trace::Dev::WasmRootOptLevel.value())
}

fn parse_jit_root_opt_level(root_only: bool, level: Option<&str>) -> Result<&'static str, ()> {
    let Some(level) = level else {
        return Ok(if root_only { "none" } else { "speed" });
    };
    match level {
        "none" => Ok("none"),
        "speed" => Ok("speed"),
        "speed_and_size" => Ok("speed_and_size"),
        _ => Err(()),
    }
}

/// Linux process RSS and high-water mark, in KiB, for opt-in JIT diagnostics.
#[cfg(target_os = "linux")]
fn jit_memory_snapshot() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let mut rss = None;
    let mut high_water = None;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            rss = value.split_whitespace().next()?.parse().ok();
        } else if let Some(value) = line.strip_prefix("VmHWM:") {
            high_water = value.split_whitespace().next()?.parse().ok();
        }
        if rss.is_some() && high_water.is_some() {
            break;
        }
    }
    Some((rss?, high_water?))
}

#[cfg(not(target_os = "linux"))]
fn jit_memory_snapshot() -> Option<(u64, u64)> {
    None
}

/// The status of a callout that raised: the VM's error (or panic) waits in
/// the context.
const RAISED_STATUS: i64 = TRAPS.len() as i64 + 1;
const TERMINATED_STATUS: i64 = quench_bjit::call_record::TERMINATED_STATUS as i64;
const _: () = assert!(TERMINATED_STATUS == RAISED_STATUS + 1, "the stencils' terminated status follows the raised one");

fn trap_status(trap: quench_runtime::WasmTrap) -> i64 {
    i64::from(trap.compiled_status().expect("compiled code raises only listed traps"))
}

/// Stack compiled code may use below its entry before a call traps as
/// exhausted; the interpreter's own guards bound the remainder.
const COMPILED_STACK_BUDGET: usize = 512 * 1024;

pub(crate) use quench_runtime::vm::engine_types::CallContext;

use quench_runtime::vm::engine_types::CallContextFields;




#[quench_macros::ext(CallContextOps1)]
impl CallContext {
    fn native_adapter(
        adapter_payload: *mut (),
        interrupt: *const std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) -> Self {
        let mut context = Self {
            head: CallContextFields {
                stack_limit: 0,
                interrupt: std::ptr::null(),
                table: std::ptr::null(),
                vm: std::ptr::null_mut(),
                env: Value::NULL,
                entry_stack: 0,
                guard_start: 0,
                guard_end: 0,
                globals: std::ptr::null(),
                table_words: std::ptr::null(),
                callout: 0,
                call_reference: 0,
                compiled: std::ptr::null(),
                program: ProgramId::MAIN,
                import_callouts: std::ptr::null_mut(),
                known_callouts: std::ptr::null_mut(),
                reference_callouts: std::ptr::null_mut(),
                memory: Value::NULL,
                memory_length: 0,
                raised: None,
                call_reference_diagnostic: 0,
                reference_call_sites: std::ptr::null(),
                reference_call_site_count: 0,
                poll_start: 0,
                poll_end: 0,
                emscripten_memcpy_fast_path_hits: std::ptr::null_mut(),
                import_callout_counts: std::ptr::null(),
                import_callout_count: 0,
                adapter_payload: std::ptr::null_mut(),
            },
            interrupt_owner: std::ptr::null(),
            instance: std::ptr::null(),
            native_roots: std::ptr::null_mut(),
            memory_grow: 0,
            wasm_gc_records: 0,
        };
        context.bind_native_adapter(adapter_payload, interrupt, poll_page);
        context
    }

    fn bind_native_adapter(
        &mut self, payload: *mut (), interrupt: *const std::sync::atomic::AtomicBool,
        poll_page: Option<(usize, usize)>,
    ) {
        self.stack_limit = native_stack_limit();
        self.interrupt = interrupt;
        self.adapter_payload = payload;
        self.bind_poll_page(poll_page);
    }

    /// The poll page whose fault ends the entry as terminated (`guard_fault`), or none.
    fn bind_poll_page(&mut self, poll_page: Option<(usize, usize)>) {
        self.poll_start = poll_page.map_or(0, |(start, _)| start);
        self.poll_end = poll_page.map_or(0, |(start, bytes)| start + bytes);
    }

    /// The context of code that reads only the interrupt state: the flag, and the VM's
    /// poll page, whose fault ends the call as terminated (`guard_fault`), where the entry
    /// runs through the fault-resuming trampoline (`native_poll_page`).
    fn interrupt_only(interrupt: &quench_runtime::execution_interrupt::ExecutionInterrupt) -> Self {
        let poll = native_poll_page(interrupt);
        Self {
            head: CallContextFields {
                stack_limit: 0,
                interrupt: interrupt.attention_flag(),
                table: std::ptr::null(),
                vm: std::ptr::null_mut(),
                env: Value::NULL,
                entry_stack: 0,
                guard_start: 0,
                guard_end: 0,
                globals: std::ptr::null(),
                table_words: std::ptr::null(),
                callout: 0,
                call_reference: 0,
                compiled: std::ptr::null(),
                program: ProgramId::MAIN,
                import_callouts: std::ptr::null_mut(),
                known_callouts: std::ptr::null_mut(),
                reference_callouts: std::ptr::null_mut(),
                memory: Value::NULL,
                memory_length: 0,
                raised: None,
                call_reference_diagnostic: 0,
                reference_call_sites: std::ptr::null(),
                reference_call_site_count: 0,
                poll_start: poll.map_or(0, |(start, _)| start),
                poll_end: poll.map_or(0, |(start, bytes)| start + bytes),
                emscripten_memcpy_fast_path_hits: std::ptr::null_mut(),
                import_callout_counts: std::ptr::null(),
                import_callout_count: 0,
                adapter_payload: std::ptr::null_mut(),
            },
            interrupt_owner: interrupt,
            instance: std::ptr::null(),
            native_roots: std::ptr::null_mut(),
            memory_grow: 0,
            wasm_gc_records: 0,
        }
    }

    /// A nested native entry shares the caller's instance and callout state,
    /// but needs its own guard-resume stack pointer and raised completion, as a
    /// nested `run_compiled` invocation would. Its stack limit is never below
    /// the caller's: recursion through the callout (a runaway `call_indirect`)
    /// spends the outermost entry's budget and traps as exhausted, rather than
    /// taking a fresh budget at each level until the native stack overflows.
    fn nested_entry(&self) -> Self {
        let marker = 0u8;
        let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
        Self {
            head: CallContextFields {
                stack_limit: self
                    .stack_limit
                    .max(stack_pointer.saturating_sub(COMPILED_STACK_BUDGET)),
                interrupt: self.interrupt,
                table: self.table,
                vm: self.vm,
                env: self.env,
                entry_stack: 0,
                guard_start: self.guard_start,
                guard_end: self.guard_end,
                globals: self.globals,
                table_words: self.table_words,
                callout: self.callout,
                call_reference: self.call_reference,
                compiled: self.compiled,
                program: self.program,
                import_callouts: self.import_callouts,
                known_callouts: self.known_callouts,
                reference_callouts: self.reference_callouts,
                memory: self.memory,
                memory_length: self.memory_length,
                raised: None,
                call_reference_diagnostic: self.call_reference_diagnostic,
                reference_call_sites: self.reference_call_sites,
                reference_call_site_count: self.reference_call_site_count,
                poll_start: self.poll_start,
                poll_end: self.poll_end,
                emscripten_memcpy_fast_path_hits: self.emscripten_memcpy_fast_path_hits,
                import_callout_counts: self.import_callout_counts,
                import_callout_count: self.import_callout_count,
                adapter_payload: std::ptr::null_mut(),
            },
            interrupt_owner: self.interrupt_owner,
            instance: self.instance,
            native_roots: self.native_roots,
            memory_grow: self.memory_grow,
            wasm_gc_records: self.wasm_gc_records,
        }
    }
}

pub(crate) use quench_runtime::vm::engine_types::Raised;

const CONTEXT_INTERRUPT: i32 = std::mem::offset_of!(CallContext, head.interrupt) as i32;
const CONTEXT_STACK_LIMIT: i32 = std::mem::offset_of!(CallContext, head.stack_limit) as i32;
const CONTEXT_TABLE: i32 = std::mem::offset_of!(CallContext, head.table) as i32;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const CONTEXT_ENTRY_STACK: u8 = std::mem::offset_of!(CallContext, head.entry_stack) as u8;
const CONTEXT_GLOBALS: i32 = std::mem::offset_of!(CallContext, head.globals) as i32;
const CONTEXT_TABLE_WORDS: i32 = std::mem::offset_of!(CallContext, head.table_words) as i32;
const CONTEXT_CALLOUT: i32 = std::mem::offset_of!(CallContext, head.callout) as i32;
const CONTEXT_CALL_REFERENCE: i32 = std::mem::offset_of!(CallContext, head.call_reference) as i32;
const CONTEXT_MEMORY_LENGTH: i32 = std::mem::offset_of!(CallContext, head.memory_length) as i32;
const CONTEXT_MEMORY_GROW: i32 = std::mem::offset_of!(CallContext, memory_grow) as i32;
const CONTEXT_CALL_REFERENCE_DIAGNOSTIC: i32 =
    std::mem::offset_of!(CallContext, head.call_reference_diagnostic) as i32;
const CONTEXT_INSTANCE: i32 = std::mem::offset_of!(CallContext, instance) as i32;
const CONTEXT_NATIVE_ROOTS: i32 = std::mem::offset_of!(CallContext, native_roots) as i32;
/// The callout's signature: (context, function index, arguments, result) ->
/// status; arguments and result are Wasm bits, one word each.
type Callout = unsafe extern "C" fn(*mut CallContext, u64, *const u64, *mut u64) -> i64;
type ReferenceCallout =
    unsafe extern "C" fn(*mut CallContext, u64, u64, u64, u64, *const u64, *mut u64) -> i64;
type DiagnosticReferenceCallout = ReferenceCallout;

/// Fill an already bounds-checked range in the instance's direct memory.
/// The compiler checks against the exact current length before calling this
/// leaf, and no VM call can move the guarded memory during the operation.
unsafe extern "C" fn compiled_memory_fill(address: u64, byte: u64, length: u64) -> i64 {
    // Wasm32 operands are zero-extended before this call; they fit every
    // supported host's address size.
    unsafe {
        std::ptr::write_bytes(address as usize as *mut u8, byte as u8, length as usize);
    }
    0
}

/// The machine type compiled code holds a Wasm vector in (`Slot::Vector`).
const VECTOR_IR: ir::Type = types::I8X16;

/// The words of a SIMD kernel callout (`compiled_simd_kernel`), by position: each operand
/// and the result take two, low then high; a scalar sits in the low word.
const SIMD_KERNEL_LEFT: usize = 0;
const SIMD_KERNEL_RIGHT: usize = 2;
const SIMD_KERNEL_RESULT: usize = 4;
const SIMD_KERNEL_WORDS: usize = 6;

/// Applies the SIMD operator `selector` names to the operand words at `words` and writes
/// its result there (`SIMD_KERNEL_RESULT`): the kernel the interpreter runs, so compiled
/// code needs no lowering of its own for an operator.
unsafe extern "C" fn compiled_simd_kernel(selector: u64, words: *mut u64) {
    let (kernel, lane) =
        quench_runtime::wasm::simd::SimdKernel::of(selector as u32).expect("planned SIMD operator");
    // SAFETY: the emitter passes its slot of SIMD_KERNEL_WORDS words.
    let words = unsafe { std::slice::from_raw_parts_mut(words, SIMD_KERNEL_WORDS) };
    let operand = |at: usize| u128::from(words[at]) | (u128::from(words[at + 1]) << u64::BITS);
    let result = kernel.apply_bits(operand(SIMD_KERNEL_LEFT), operand(SIMD_KERNEL_RIGHT), lane);
    words[SIMD_KERNEL_RESULT] = result as u64;
    words[SIMD_KERNEL_RESULT + 1] = (result >> u64::BITS) as u64;
}

/// `memory.copy` within one memory, whose ranges the caller checked: overlapping ranges copy
/// as if through a temporary buffer.
unsafe extern "C" fn compiled_memory_copy(destination: u64, source: u64, length: u64) -> i64 {
    // Wasm32 operands are zero-extended before this call; they fit every supported host's
    // address size.
    unsafe {
        std::ptr::copy(
            source as usize as *const u8,
            destination as usize as *mut u8,
            length as usize,
        );
    }
    0
}

fn note_compiled_callout(counter: *mut u64) {
    if !counter.is_null() {
        // SAFETY: only diagnostic-enabled calls provide this pointer, and it
        // points to a counter owned by the suspended native invocation.
        unsafe {
            *counter = (*counter).saturating_add(1);
        }
    }
}

unsafe fn note_import_callout_target(
    counters: *const std::cell::Cell<u64>,
    counter_count: usize,
    function: u64,
) {
    let Ok(index) = usize::try_from(function) else {
        return;
    };
    if counters.is_null() || index >= counter_count {
        return;
    }
    // SAFETY: diagnostic contexts point at `counter_count` initialized Cells
    // owned by their CompiledProgram, which remains live across the callout.
    let counter = unsafe { &*counters.add(index) };
    counter.set(counter.get().saturating_add(1));
}



use quench_runtime::vm::engine_types::ReferenceCallSiteCounts;

use quench_runtime::vm::engine_types::ReferenceCallTargetCounts;

pub(crate) use quench_runtime::vm::engine_types::ReferenceCallSiteDiagnostics;

fn note_reference_call_site(
    context: &CallContext,
    site: u64,
    native: bool,
    same_artifact_direct: bool,
    target_function: Option<usize>,
    target_same_artifact_native: bool,
) {
    let Ok(site) = usize::try_from(site) else {
        return;
    };
    if context.reference_call_sites.is_null() || site >= context.reference_call_site_count {
        return;
    }
    // SAFETY: diagnostic call sites are a stable vector owned by the compiled
    // program for the full native invocation, and generated ids are assigned
    // from that vector at compile time.
    let counts = unsafe { &*context.reference_call_sites.add(site) };
    let mut updated = counts.counts.get();
    updated.total = updated.total.saturating_add(1);
    if native {
        updated.native = updated.native.saturating_add(1);
    } else {
        updated.fallback = updated.fallback.saturating_add(1);
    }
    if same_artifact_direct {
        updated.same_artifact_direct = updated.same_artifact_direct.saturating_add(1);
    }
    counts.counts.set(updated);
    if let Some(function) = target_function {
        let mut targets = counts.targets.borrow_mut();
        let target_index = if let Some(target_index) = targets
            .iter()
            .position(|target| target.function == function)
        {
            target_index
        } else {
            targets.push(ReferenceCallTargetCounts {
                function,
                ..ReferenceCallTargetCounts::default()
            });
            targets.len() - 1
        };
        let target = &mut targets[target_index];
        target.total = target.total.saturating_add(1);
        if native {
            target.native = target.native.saturating_add(1);
        }
        if native && target_same_artifact_native {
            target.same_artifact_native = target.same_artifact_native.saturating_add(1);
        }
    }
}

fn refresh_memory_length(context: &mut CallContext, vm: &Vm) {
    if context.memory != Value::UNDEFINED
        && let Some((_, length)) = crate::dispatch_fast::direct_memory(vm, context.memory)
    {
        context.memory_length = length as u64;
    }
}

/// Publishes the Wasm GC record range's base (`CallContext::wasm_gc_records`) after a callout that may
/// have reserved it: the base is fixed once reserved, so zero means the range is not yet made.
pub(crate) fn refresh_wasm_gc_records(context: &mut CallContext, vm: &Vm) {
    context.wasm_gc_records = vm.heap_reads().wasm_gc_record_base().map_or(0, |base| base as usize);
}

fn finish_callout(
    context: &mut CallContext,
    called: Result<Result<(), JsError>, Box<dyn std::any::Any + Send>>,
) -> i64 {
    match called {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            context.raised = Some(Raised::Error(error));
            RAISED_STATUS
        }
        Err(panic) => {
            context.raised = Some(Raised::Panic(panic));
            RAISED_STATUS
        }
    }
}

/// Resolve, check, and invoke an indirect table target in one VM callout.
unsafe extern "C" fn reference_callout(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // A stable call-site id is present even without diagnostics so the VM can
    // find the per-site monomorphic cache without a hash lookup.
    unsafe {
        reference_callout_inner::<false>(
            context,
            table_slot,
            index,
            signature_index,
            site,
            args,
            out,
        )
    }
}

/// Diagnostic-only reference callout. Its extra site id is emitted only when
/// Wasm JIT diagnostics were enabled at compilation time.
unsafe extern "C" fn diagnostic_reference_callout(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    unsafe {
        reference_callout_inner::<true>(
            context,
            table_slot,
            index,
            signature_index,
            site,
            args,
            out,
        )
    }
}

/// Builds the validation error of a failed Wasm callout check, out of line.
#[cold]
#[inline(never)]
fn wasm_callout_error(message: &'static str) -> JsError {
    JsError::validation(message.into())
}

/// Calls an indirect target with no direct native entry and stores its scalar result at `out`.
#[cold]
#[inline(never)]
unsafe fn reference_callout_fallback<const DIAGNOSTICS: bool>(
    vm: &mut Vm,
    program: ProgramId,
    caller: *const CompiledProgram,
    callee: Value,
    same_program_function: Option<(ProgramId, u32, Value)>,
    params: &[quench_runtime::WasmType],
    result_type: Option<quench_runtime::WasmType>,
    reference_target_function: Option<usize>,
    args: *const u64,
    out: *mut u64,
    native_target_hit: &mut bool,
    target_same_artifact_native: &mut bool,
) -> Result<(), JsError> {
    let mut values: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
    for (offset, ty) in params.iter().enumerate() {
        Kind::of(*ty).ok_or_else(|| JsError::validation("unsupported Wasm call parameter".into()))?;
        // SAFETY: the generated call frame stores one word per parameter.
        let bits = unsafe { *args.add(offset) };
        values.push(wasm_word_value(vm, *ty, bits));
    }
    let compiled_result = same_program_function.and_then(|(target_program, function, env)| {
        vm.run_compiled(target_program, env, function, &values)
    });
    *native_target_hit = compiled_result.is_some();
    if DIAGNOSTICS
        && *native_target_hit
        && let Some(function) = reference_target_function
    {
        let target_compiled = compiled_reference_target(vm, program, function, caller);
        *target_same_artifact_native = !caller.is_null()
            && target_compiled
                .as_ref()
                .is_some_and(|compiled| std::ptr::eq(caller, Rc::as_ptr(compiled)));
    }
    let result = match compiled_result {
        Some(result) => result?,
        None => {
            let code = vm
                .programs
                .get(program)
                .ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
            let receiver = vm
                .programs
                .constant(program, VOID_RESULT_CONSTANT)
                .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
            vm.call_value(&code, callee, receiver, &values)?
        }
    };
    let bits = wasm_call_vm::result_bits(vm, program, result, result_type)?;
    // SAFETY: `out` points into the result word emitted for this helper.
    unsafe { *out = bits };
    Ok(())
}

/// Shared implementation, monomorphized so ordinary callouts have no
/// diagnostic counter branch or extra site lookup.
unsafe fn reference_callout_inner<const DIAGNOSTICS: bool>(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: generated code passes the context, table binding, table index,
    // signature index, argument words, and output storage for this call.
    let context = unsafe { &mut *context };
    note_compiled_callout(context.reference_callouts);
    let vm_pointer = context.vm.cast::<Vm>();
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let program = context.program;
    let mut native_target_hit = false;
    let mut same_artifact_direct_hit = false;
    let mut target_same_artifact_native = false;
    let mut reference_target_function = None;
    let mut direct_target_root = None;
    let mut direct_status = None;
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (callee, params, result_type, same_program_function, direct_entry) = {
            // Do all heap/program inspection before entering native code. The
            // VM is re-entered through callouts, so no `&mut Vm` may live
            // across that boundary.
            let vm = unsafe { &mut *vm_pointer };
            let table_slot = u32::try_from(table_slot)
                .map_err(|_| wasm_callout_error("invalid Wasm table binding"))?;
            let signature_index = u32::try_from(signature_index)
                .map_err(|_| wasm_callout_error("invalid Wasm signature index"))?;
            let table = vm
                .heap_reads()
                .environment_slot(context.env, table_slot as usize)
                .ok_or_else(|| wasm_callout_error("invalid Wasm table binding"))?;
            let callee = vm.wasm_indirect_target(program, table, index, signature_index)?;
            let (params, result_type) = {
                let signature = vm
                    .programs
                    .wasm_signatures(program)
                    .and_then(|signatures| signatures.type_signature(signature_index))
                    .ok_or_else(|| wasm_callout_error("invalid Wasm signature index"))?;
                (
                    signature
                        .params
                        .iter()
                        .copied()
                        .collect::<smallvec::SmallVec<[quench_runtime::WasmType; 8]>>(),
                    signature.results.first().copied(),
                )
            };
            let same_program_function = match vm.function_record(callee) {
                Some(FunctionRecord {
                    kind: quench_runtime::tier_surface::FunctionKind::User(target_program, function),
                    env,
                    ..
                }) if *target_program == program => Some((*target_program, *function, *env)),
                _ => None,
            };
            let same_instance_function = same_program_function
                .filter(|(_, _, target_env)| *target_env == context.env)
                .map(|(_, function, _)| function);
            if let Some(function) = same_instance_function
                && !context.compiled.is_null()
                && let Ok(site) = usize::try_from(site)
            {
                // SAFETY: the running artifact outlives its own call.
                unsafe { &*context.compiled }.note_indirect_target(site, function);
            }
            let direct_entry = same_instance_function.and_then(|function| {
                let compiled =
                    compiled_reference_target(vm, program, function as usize, context.compiled)?;
                direct_reference_entry(
                    vm,
                    compiled,
                    function as usize,
                    (context.env, program),
                    context.memory,
                    Some((&params, result_type)),
                )
            });
            (callee, params, result_type, same_program_function, direct_entry)
        };
        if DIAGNOSTICS {
            reference_target_function = direct_entry.as_ref().map(|entry| entry.1).or_else(|| {
                same_program_function
                    .as_ref()
                    .map(|(_, function, _)| *function as usize)
            });
        }
        if let Some((
            compiled,
            function,
            entry,
            (base, length),
            record,
        )) = direct_entry
        {
            // A re-entrant import can mutate the table and collect while this
            // body is running. Keep its function cell alive until the call
            // returns.
            let callee_root = unsafe { &mut *vm_pointer }.hold_root(callee);
            direct_target_root = Some(callee_root);
            compiled.note_direct_reference_entry(function);
            let mut nested = context.nested_entry();
            // SAFETY: the record belongs to the environment the target runs in.
            nested.enter(unsafe { &*record });
            // SAFETY: the table/signature resolver ran first; same program,
            // exact instance environment, and exact scalar signature were
            // checked above. The caller's raw argument/result buffers have
            // the Wasm ABI shape expected by this compiled entry, and `compiled`
            // keeps its executable mapping alive for the duration of the call.
            native_target_hit = true;
            if DIAGNOSTICS {
                same_artifact_direct_hit = !context.compiled.is_null()
                    && std::ptr::eq(context.compiled, Rc::as_ptr(&compiled));
                target_same_artifact_native = same_artifact_direct_hit;
            }
            // SAFETY: as the direct path's call; the nested context outlives the call.
            let status = unsafe {
                enter_wasm(
                    &mut nested,
                    base,
                    length,
                    args,
                    out,
                    entry,
                )
            };
            if status == RAISED_STATUS as u32 {
                match nested.raised.take() {
                    Some(Raised::Error(error)) => return Err(error),
                    Some(Raised::Panic(panic)) => std::panic::resume_unwind(panic),
                    None => {
                        return Err(wasm_callout_error("compiled indirect callee raised without an error"));
                    }
                }
            }
            direct_status = Some(status);
            return Ok(());
        }

        let vm = unsafe { &mut *vm_pointer };
        // SAFETY: the generated call passes one argument word per parameter of `params` and
        // a result word for `result_type`, the shape the fallback reads and writes.
        unsafe {
            reference_callout_fallback::<DIAGNOSTICS>(
                vm,
                program,
                context.compiled,
                callee,
                same_program_function,
                &params,
                result_type,
                reference_target_function,
                args,
                out,
                &mut native_target_hit,
                &mut target_same_artifact_native,
            )
        }?;
        Ok(())
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    let vm = unsafe { &mut *vm_pointer };
    if let Some(root) = direct_target_root {
        vm.drop_root(root);
    }
    if DIAGNOSTICS {
        note_reference_call_site(
            context,
            site,
            native_target_hit,
            same_artifact_direct_hit,
            reference_target_function,
            target_same_artifact_native,
        );
    }
    refresh_memory_length(context, vm);
    refresh_wasm_gc_records(context, vm);
    direct_status.map_or_else(|| finish_callout(context, called), i64::from)
}

/// Make imported call `function` for compiled code, as the interpreter's
/// `Call` does.
unsafe extern "C" fn import_callout(
    context: *mut CallContext,
    function: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose
    // VM is suspended in that call.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm>() };
    let defined = vm
        .programs
        .wasm_signatures(context.program)
        .and_then(|signatures| signatures.defined_index(function as u32))
        .is_some();
    note_compiled_callout(if defined {
        context.known_callouts
    } else {
        context.import_callouts
    });
    if !defined && !context.import_callout_counts.is_null() {
        // SAFETY: this pointer is non-null only when the owning compiled
        // program enabled diagnostic counters for the full native call.
        unsafe {
            note_import_callout_target(
                context.import_callout_counts,
                context.import_callout_count,
                function,
            );
        }
    }
    // The VM's own code runs outside compiled code: no guard fault is ours.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let program = context.program;
    let env = context.env;
    let current_memory = context.memory;
    let memcpy_fast_path_hits = context.emscripten_memcpy_fast_path_hits;
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: compiled code passes one word per parameter and a result
        // word.
        unsafe {
            vm.call_import(
                program,
                env,
                function as u32,
                current_memory,
                memcpy_fast_path_hits,
                args,
                out,
            )
        }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    refresh_memory_length(context, vm);
    refresh_wasm_gc_records(context, vm);
    finish_callout(context, called)
}

use quench_runtime::vm::engine_types::WasmEntry;

/// The native-entry ABI of compiled Wasm functions: (context, memory base or locals, memory
/// length or local count, arguments or registers, result word) -> status.
pub(crate) fn native_entry_signature(
    pointer: ir::Type,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    call_signature::typed_signature(call_conv, &[pointer, types::I64, types::I64, pointer, pointer], &[types::I32])
}

pub(crate) use quench_runtime::vm::engine_types::NativeNumberResult;

enum CompiledArgs<'a> {
    Values(&'a [Value]),
    RegisterWindow {
        caller: usize,
        base: usize,
        count: usize,
    },
}

/// Function `index`'s entry stitched from the b-jit's rows, under `--b-jit` where it has rows
/// for every instruction: the `WasmEntry` ABI the compiled entry has. Its World is the shared one
/// (`Analysis::with_world`), naming this process's cells and callouts.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn stitched_entry(
    analysis: &Analysis<'_>,
    index: usize,
    (plans, cells, callable): (&[Option<Plan>], &[std::cell::Cell<usize>], &[bool]),
    memory_slot: Option<u32>,
    (full_plan, sites): (&Plan, &[(usize, usize)]),
) -> Option<Vec<u8>> {
    let (vm, program, env) = (analysis.vm, analysis.program, analysis.env);
    // The indirect call row calls the non-diagnostic reference callout.
    if !admission::wasm_module_routes_to_stencils() {
        return None;
    }
    // The program's image may hold the rows already (`wasm_image::linked`).
    if let Some(rows) = wasm_image::linked(analysis, index, memory_slot, (cells, callable)) {
        return Some(rows);
    }
    let function = analysis.code.function_get(index)?;
    // A callee has an entry cell while it is `callable` (it has a plan and its stitch succeeded);
    // the cell holds the entry once placed. A callee that is not callable is called through the
    // VM (`CALL_VM`), so its callers stitch all the same.
    let callee = |callee: usize| callable.get(callee).copied()?.then(|| cells[callee].as_ptr() as u64);
    let canonical = |index: u32| vm.programs.wasm_signatures(program)?.type_canonical(index).map(|canonical| canonical.word());
    let indirect = |pc: usize| wasm_stitch::indirect::site(full_plan, sites, &canonical, pc);
    // A growth leaves the base in place only for a reserved (guarded) memory.
    let memory_stable = memory_slot.is_none_or(|slot| {
        vm.heap_reads().environment_slot(env, slot as usize).and_then(|binding| guarded_memory(vm, binding)).is_some()
    });
    let naming = wasm_world::Naming { constant: &|_| true, callee: &callee, indirect: &indirect, memory_stable, callouts: wasm_world::Callouts::live() };
    let returns = plans[index].as_ref()?.result.is_some();
    analysis.with_world(function, full_plan, memory_slot, naming, |world| wasm_stitch::stitch(function, usize::from(function.params), returns, world))
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn stitched_entry(_: &Analysis<'_>, _: usize, _: (&[Option<Plan>], &[std::cell::Cell<usize>], &[bool]), _: Option<u32>, _: (&Plan, &[(usize, usize)])) -> Option<Vec<u8>> {
    None
}

/// The `CompiledProgram::positions` entry of a function an artifact does not hold.
const NOT_COMPILED: u32 = u32::MAX;

pub(crate) use quench_runtime::vm::engine_types::CompiledFunction;

type DirectReferenceEntry = (
    Rc<CompiledProgram>,
    usize,
    WasmEntry,
    (u64, u64),
    *const InstanceContext,
);

/// Build the per-invocation activation for a same-instance compiled indirect
/// target. Even a monomorphic cache hit comes through here so memory, guards,
/// and mutable global addresses always reflect the current instance state.
fn direct_reference_entry(
    vm: &mut Vm,
    compiled: Rc<CompiledProgram>,
    function: usize,
    (env, program): (Value, ProgramId),
    context_memory: Value,
    signature: Option<(&[quench_runtime::WasmType], Option<quench_runtime::WasmType>)>,
) -> Option<DirectReferenceEntry> {
    let native = compiled.function(function)?;
    if signature.is_some_and(|(params, result)| {
        native.params.as_slice() != params || native.result != result
    }) {
        return None;
    }
    let entry = native.entry;
    if let Some(slot) = compiled.memory_slot
        && vm.heap_reads().environment_slot(env, slot as usize)? != context_memory
    {
        return None;
    }
    let (record, memory) = vm.publish_wasm_instance(&compiled, env, program)?;
    Some((compiled, function, entry, memory, record))
}

/// Pick a compiled artifact for an indirect target, preferring the artifact
/// that is already executing when it contains the target. This is the case
/// after profile-guided selective promotion closes over a hot indirect callee.
fn compiled_reference_target(
    vm: &Vm,
    program: ProgramId,
    function: usize,
    caller: *const CompiledProgram,
) -> Option<Rc<CompiledProgram>> {
    let current_artifact = if caller.is_null() {
        None
    } else {
        vm.programs
            .compiled(program)
            .flatten()
            .filter(|compiled| std::ptr::eq(Rc::as_ptr(compiled), caller))
            .filter(|compiled| compiled.has_function(function))
            .or_else(|| {
                u32::try_from(function).ok().and_then(|function| {
                    vm.programs.wasm_tier_for_artifact_containing(
                        program,
                        function as usize,
                        caller,
                    )
                })
            })
    };
    current_artifact
        .or_else(|| {
            vm.programs
                .compiled(program)
                .flatten()
                .filter(|compiled| compiled.has_function(function))
        })
        .or_else(|| {
            u32::try_from(function)
                .ok()
                .and_then(|function| vm.programs.wasm_tier_containing(program, function))
        })
}

/// A Wasm program's compile (`compile_wasm_program`), the route `jit_routes` installs.
pub(crate) type WasmCompile = fn(&Vm, ProgramId, Value, bool, Option<usize>, bool, admission::CompiledUnit) -> Option<CompiledProgram>;
pub(crate) use quench_runtime::vm::engine_types::CompiledProgram;

use quench_runtime::vm::engine_types::WasmJitDiagnostics;



fn jit_diagnostics_enabled() -> bool {
    quench_runtime::trace::Channel::WasmJit.on()
}

#[quench_macros::ext(CompiledProgramOps1)]
impl CompiledProgram {
    #[cfg(test)]
    pub(crate) fn emscripten_memcpy_fast_path_hits(&self) -> u64 {
        self.diagnostics
            .as_ref()
            .map_or(0, |stats| stats.emscripten_memcpy_fast_path_hits.get())
    }

    fn note_direct_reference_entry(&self, function: usize) {
        self.note_native_entry(function);
        #[cfg(test)]
        if let Some(function) = self.function(function) {
            function
                .direct_reference_entries
                .set(function.direct_reference_entries.get().saturating_add(1));
        }
    }

    fn note_native_entry(&self, function: usize) {
        if let Some(stats) = &self.diagnostics
            && let Some(entries) = stats.native_entry_requests.get(function)
        {
            entries.set(entries.get().saturating_add(1));
        }
        #[cfg(test)]
        if let Some(function) = self.function(function) {
            function
                .native_entries
                .set(function.native_entries.get().saturating_add(1));
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_call_site_counts(&self) -> Option<Vec<(usize, usize, u64, u64, u64)>> {
        Some(
            self.diagnostics
                .as_ref()?
                .reference_call_sites
                .iter()
                .map(|site| {
                    let counts = site.counts.get();
                    (
                        site.function,
                        site.pc,
                        counts.total,
                        counts.native,
                        counts.fallback,
                    )
                })
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn reference_call_target_counts(
        &self,
    ) -> Option<Vec<(usize, usize, usize, u64, u64, u64)>> {
        Some(
            self.diagnostics
                .as_ref()?
                .reference_call_sites
                .iter()
                .flat_map(|site| {
                    site.targets
                        .borrow()
                        .iter()
                        .map(|target| {
                            (
                                site.function,
                                site.pc,
                                target.function,
                                target.total,
                                target.native,
                                target.same_artifact_native,
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn same_artifact_direct_reference_hits(&self) -> Option<u64> {
        Some(
            self.diagnostics
                .as_ref()?
                .reference_call_sites
                .iter()
                .map(|site| site.counts.get().same_artifact_direct)
                .fold(0u64, u64::saturating_add),
        )
    }

    #[cfg(test)]
    pub(crate) fn native_entries(&self, function: usize) -> Option<u64> {
        self.function(function).map(|function| function.native_entries.get())
    }

    #[cfg(test)]
    pub(crate) fn direct_reference_entries(&self, function: usize) -> Option<u64> {
        self.function(function).map(|function| function.direct_reference_entries.get())
    }
}

/// With `QUENCH_TRACE=perf-map`, names each function's code (`kind-index`) in `/tmp/perf-PID.map`
/// and writes the code to `/tmp/quench-jit-PID-BASE.bin` for disassembly at `BASE` (diagnostics).
fn write_perf_map(kind: &str, base: usize, bytes: &[u8], spans: &[(usize, usize, usize)]) {
    if !quench_runtime::trace::Channel::PerfMap.on() {
        return;
    }
    use std::io::Write;
    let pid = std::process::id();
    if let Ok(mut map) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("/tmp/perf-{pid}.map"))
    {
        for &(offset, len, index) in spans {
            let _ = writeln!(map, "{:x} {len:x} {kind}-{index}", base + offset);
        }
    }
    let _ = std::fs::write(format!("/tmp/quench-jit-{pid}-{base:x}.bin"), bytes);
}

/// Counts a lane-ABI region's run of `region` that ended in `run`, `lent` the payload, in the VM's tier events.
fn note_osr_run(events: &quench_runtime::vm::tier_events::TierEvents, run: &RegionRun, region: &LoopRegion, lent: bool) {
    use quench_runtime::vm::tier_events::Event;
    let inlined = !run.nested && matches!(run.exit, LaneExit::Inlined { .. });
    for (event, happened) in [(Event::OsrLent, lent), (Event::OsrExited, run.left(region)), (Event::OsrInlined, inlined)] {
        if happened { events.record(event); }
    }
}

pub(crate) use quench_runtime::vm::engine_types::CompiledOsrRegion;

pub(crate) use quench_runtime::vm::engine_types::EntryStamp;

#[quench_macros::ext(EntryStampOps1)]
impl EntryStamp {
    /// The stamp of the activation an entry runs, its capture view not yet bound (`captures`).
    fn of(vm: &Vm, (program, function, frame): (ProgramId, u32, usize), lane_context: usize, registers: *mut Value,
        specialized: bool) -> Self {
        let (frame_record, locals) = vm.frames.get(frame)
            .map_or((0, 0), |record| (std::ptr::from_ref(record).addr(), record.locals.as_ptr().addr()));
        Self { vm: std::ptr::from_ref(vm).addr(), program, function, frame, frame_record, locals, lane_context,
            registers: registers.addr(), specialized, captures: (0, 0) }
    }

    /// Whether `other` is the activation `self` was bound for: the same VM, program and function,
    /// frame (by index, record and locals), lane context and register file.
    fn binds(&self, other: &Self) -> bool {
        self.vm == other.vm && self.program == other.program && self.function == other.function
            && self.frame == other.frame && self.frame_record == other.frame_record && self.locals == other.locals
            && self.lane_context == other.lane_context && self.registers == other.registers
    }
}

/// The capture view a run of frame `frame` binds (`JsStepDispatch::refresh_capture_view`), as the
/// key a stamp compares: the slots' address (0 where the view is none) and the native layout
/// epoch, which moves at every collection. Equal keys bind the same view: a collection that
/// retires or moves the frame's slots changes one of the two.
fn captures_key(vm: &Vm, frame: usize) -> (usize, u64) {
    let slots = vm.current_capture_slots(frame).filter(|(_, root)| root.is_none()).map_or(0, |(slots, _)| slots.addr());
    (slots, vm.heap_reads().native_layout_epoch())
}


#[quench_macros::ext(CompiledOsrRegionOps1)]
impl CompiledOsrRegion {
    /// Whether a run can begin at `pc`: the region's header, or a PC with a resume stub.
    pub(crate) fn can_run_from(&self, region: &LoopRegion, pc: u32) -> bool {
        pc == region.header || self.resumes.stub(pc).is_some()
    }

    /// The native entry and its `captures` argument that begin a run at `from` (`can_run_from`):
    /// the region's entry at its header; otherwise the resume thunk, given the stub's address.
    fn start_at(&self, region: &LoopRegion, from: u32) -> (BodyEntry, u64) {
        let Some(stub) = self.resumes.stub(from).filter(|_| from != region.header) else { return (self.entry, 0) };
        let base = self.code.pointer();
        // SAFETY: the thunk is a routine of the code at `base`, of the lane body ABI.
        let thunk = unsafe { std::mem::transmute::<*mut u8, BodyEntry>(base.add(self.resumes.thunk())) };
        (thunk, base as u64 + stub as u64)
    }

    /// Whether a run builds an activation over the lane's VM frame; a bare run reads the registers alone.
    pub(crate) fn needs_frame(&self) -> bool { self.crossing != Crossing::Bare }

    /// Runs the region on `registers` with `entry`'s record, through the lane body ABI
    /// (`enter_body`), from its header or, where `can_run_from`, from the instruction at `from`;
    /// `lane` the lane context and whether the lane may make the activations of direct calls.
    ///
    /// # Safety
    /// `registers` is the running activation's register file, at least `Function::registers`
    /// long; `entry` was built for this region from the current values of its arrays and
    /// captures. Capacity helpers may refresh array backing words without collecting;
    /// `vm` and `interrupt` belong to
    /// this lane activation, which excludes other mutable VM access while native code runs.
    pub(crate) unsafe fn run(
        &self,
        registers: *mut quench_runtime::value::Value,
        entry: &mut LoopEntry,
        region: &LoopRegion,
        vm: *mut Vm,
        program: ProgramId,
        function: u32,
        frame: usize,
        interrupt: *const quench_runtime::execution_interrupt::ExecutionInterrupt,
        lane: (usize, bool),
        from: u32,
    ) -> RegionRun {
        let start = self.start_at(region, from);
        // SAFETY: the caller's contract covers `vm`.
        unsafe { (*vm).programs.tier_events().record(quench_runtime::vm::tier_events::Event::OsrRun) };
        // SAFETY: the caller's contract; the region runs the same lane entry ABI.
        unsafe { if self.crossing == Crossing::Bare { stencil_run::run_bare(start, registers, entry, interrupt) }
            else { self.run_lane(start, registers, entry, region, vm, program, function, frame, interrupt, lane) } }
    }

    /// Runs a loop compiled through the lane region specializer: the shared lane entry ABI,
    /// with the activation's dispatch as the payload and the register file as its arguments.
    ///
    /// # Safety
    /// As `run`: `registers` is the activation's live register file, and `vm`/`interrupt`
    /// belong to this lane activation.
    unsafe fn run_lane(
        &self,
        (code, resume): (BodyEntry, u64),
        registers: *mut quench_runtime::value::Value,
        entry: &mut LoopEntry,
        region: &LoopRegion,
        vm: *mut Vm,
        program: ProgramId,
        function: u32,
        frame: usize,
        interrupt: *const quench_runtime::execution_interrupt::ExecutionInterrupt,
        (lane_context, direct_calls): (usize, bool),
    ) -> RegionRun {
        let lend = self.calls.enters_callees().then_some(direct_calls);
        let calls = *self.region_calls.get_or_init(|| osr_lane::region_calls(region));
        // The dense capture copy is the region's own, taken for this run and put back after it
        // (`dense`), so an entry refills it in place.
        let mut captures = self.dense.take();
        let parked = self.parked.take().filter(|_| self.crossing == Crossing::General);
        // The region's calls, named to the lane's helpers for the run (`rebind_region_call`).
        // SAFETY: `lane_context` is the running lane's.
        let outer_calls = unsafe { std::mem::replace(&mut (*(lane_context as *mut quench_runtime::vm::engine_types::LaneContext)).region_calls, std::ptr::from_ref(&self.calls).addr()) };
        // SAFETY: the caller's contract covers `vm`, `registers`, `interrupt` and the context.
        let outcome = unsafe {
            match parked {
                // The region's parked dispatch: an entry of the activation it was last fully bound for
                // rebinds only what a run changes (`rewarm_region`); any other rebinds in full.
                Some(mut dispatch) => {
                    let vm = &mut *vm;
                    let bound = match self.stamp.get() {
                        Some(stamp) if stamp.binds(&EntryStamp::of(&*vm, (program, function, frame), lane_context, registers, stamp.specialized)) => Some(stamp),
                        _ => None,
                    };
                    let stamp = match bound {
                        Some(stamp) => {
                            dispatch.rewarm_region(vm, stamp.specialized);
                            let view = captures_key(vm, frame);
                            if view != stamp.captures { dispatch.refresh_capture_view(vm); }
                            EntryStamp { captures: view, ..stamp }
                        }
                        None => {
                            let Some(residual) = vm.programs.get(program) else { return RegionRun::own(LaneExit::Terminated) };
                            dispatch.rebind_region(vm, program, &residual, frame, registers);
                            osr_lane::bind_frame(&mut dispatch, &residual, vm, (program, function, lane_context), registers, true);
                            EntryStamp { captures: captures_key(vm, frame), ..EntryStamp::of(vm, (program, function, frame), lane_context, registers, residual.specialized) }
                        }
                    };
                    osr_lane::bind_loop(&mut dispatch, region, entry, lane_context);
                    osr_lane::bind_captures(region, entry, lane_context, registers, calls, &mut captures);
                    self.stamp.set(Some(stamp));
                    let run = osr_lane::enter(&mut dispatch, vm, program, (code, resume), registers, interrupt, (lane_context, lend));
                    self.parked.set(Some(dispatch));
                    run
                }
                // A run entered while another of the region's runs holds the parked dispatch, or the
                // first runs, builds its own activation. The first run parks an idle dispatch for the
                // runs after it.
                None => JsStepDispatch::with_function(&mut *vm, program, (frame, function), 0, false, self.crossing == Crossing::General, |dispatch, vm| {
                    dispatch.bind_native_storage(std::ptr::from_mut(vm));
                    osr_lane::bind_loop(dispatch, region, entry, lane_context);
                    osr_lane::bind_captures(region, entry, lane_context, registers, calls, &mut captures);
                    // `with_function` admitted the program; it holds it for the run.
                    let Some(residual) = vm.programs.get(program) else { return RegionRun::own(LaneExit::Terminated) };
                    osr_lane::bind_frame(dispatch, &residual, vm, (program, function, lane_context), registers, false);
                    let run = osr_lane::enter(dispatch, vm, program, (code, resume), registers, interrupt, (lane_context, lend));
                    if self.crossing == Crossing::General && !self.made.replace(true) {
                        self.parked.set(Some(Box::new(JsStepDispatch::idle())));
                    }
                    run
                }).unwrap_or(RegionRun::own(LaneExit::Terminated)),
            }
        };
        self.dense.set(captures);
        // SAFETY: as above.
        unsafe { (*(lane_context as *mut quench_runtime::vm::engine_types::LaneContext)).region_calls = outer_calls };
        // SAFETY: as above; the activation that held `vm` has returned.
        note_osr_run(unsafe { (*vm).programs.tier_events() }, &outcome, region, self.calls.enters_callees());
        outcome
    }

}

/// Analyze a loop candidate whose calls run inlined (`osr_loop::inline_region`), each
/// callee from `resolve` of the cached field its function is read from.
#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_inlining_osr_region<'a>(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[quench_runtime::value::Value],
    sites: &[quench_runtime::bytecode::FieldSite],
    live: &quench_runtime::compile::liveness::RegisterMasks,
    header: u32,
    backedge: u32,
    resolve: &mut dyn FnMut(&[FieldUse], CalleeSource) -> Option<InlineCallee<'a>>,
) -> Option<LoopRegion> {
    osr_loop::inline_region(function, decoded, constants, sites, live, header, backedge, resolve)
}

/// Analyze a loop candidate through the shared region compiler's OSR mode.
/// Analyze a loop candidate and preserve the function PC that blocked admission.
pub(crate) fn analyze_osr_region_detailed(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[quench_runtime::value::Value],
    sites: &[quench_runtime::bytecode::FieldSite],
    live: Option<&std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    header: u32,
    backedge: u32,
    method_calls: MethodCalls,
    native_call: &dyn Fn(usize) -> bool,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    osr_loop::region(function, decoded, constants, sites, live, header, backedge, method_calls, native_call)
}


/// Whether the loop tier compiles regions under `policy` (`admission::region_unit_under`), for the tests that probe it.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn loop_tier_compiles(policy: quench_runtime::MemoryPolicy) -> bool {
    admission::region_unit_under(policy).is_some()
}

/// Compile one admitted OSR region and publish it through the shared executable-memory path.
/// The process's peak resident set in KiB, for `QUENCH_TRACE=jit-peak`.
fn process_peak_rss_kib() -> i64 {
    // SAFETY: `getrusage` writes into the struct this call owns.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: as above; RUSAGE_SELF names this process.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return 0;
    }
    // Darwin counts bytes, Linux KiB.
    #[cfg(target_vendor = "apple")]
    {
        usage.ru_maxrss / 1024
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        usage.ru_maxrss
    }
}

/// The directory the `jit-dump` trace channel names: each loop region and body
/// compiled writes its machine code and its CLIF there (`dump_native_code`), for attaching
/// instruction-level profiles to native code.
fn native_dump_directory() -> Option<&'static std::path::Path> {
    quench_runtime::trace::Channel::JitDump.value().map(std::path::Path::new)
}

/// What a dumped piece of native code was compiled for: a loop region, named by its header's
/// bytecode position, or a function's body, named by its function and, where it has one, the
/// byte offset of its scalar entry (the per-call path the C-parity gate measures).
#[derive(Clone, Copy)]
pub(crate) enum DumpSite {
    Loop { header: u32 },
    Body { function: u32, scalar: Option<usize> },
}

impl std::fmt::Display for DumpSite {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Loop { header } => write!(out, "loop header={header}"),
            Self::Body { function, scalar: None } => write!(out, "body function={function}"),
            Self::Body { function, scalar: Some(at) } => write!(out, "body function={function} scalar={at}"),
        }
    }
}

/// Writes compiled code's executable mapping as `code-<n>.bin` and its CLIF as `code-<n>.clif`
/// into `directory`, numbered in compilation order, and reports what it was compiled for and
/// where the mapping lives on stderr. A failed write leaves the run as it is.
fn dump_native_code(directory: &std::path::Path, site: DumpSite, code: &CodeMemory, clif: &str) {
    // SAFETY: the mapping is `code.length` readable bytes while `code` lives.
    let bytes = unsafe { std::slice::from_raw_parts(code.pointer(), code.length()) };
    dump_native_bytes(directory, site, bytes, Some(code.pointer()), clif);
}

/// Publication is diagnostic data, not a reason to allocate executable memory.
fn dump_native_bytes(directory: &std::path::Path, site: DumpSite, bytes: &[u8], address: Option<*mut u8>, clif: &str) {
    static COMPILED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let number = COMPILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = std::fs::write(directory.join(format!("code-{number:03}.bin")), bytes);
    let _ = std::fs::write(directory.join(format!("code-{number:03}.clif")), clif);
    match address {
        Some(address) => eprintln!("QUENCH_JS_JIT_DUMP code-{number:03} {site} address={address:p} bytes={}", bytes.len()),
        None => eprintln!("QUENCH_JS_JIT_DUMP code-{number:03} {site} unpublished bytes={}", bytes.len()),
    }
}

/// Publishes compiled `code` of a region lowered with `body_signature_in_lane` to executable
/// memory, and its entry.
fn publish_body(code: &[u8]) -> Option<(CodeMemory, BodyEntry)> {
    let memory = publish_code(code)?;
    // SAFETY: callers lower the region with `body_signature_in_lane`.
    let entry = unsafe { std::mem::transmute::<*mut u8, BodyEntry>(memory.pointer()) };
    Some((memory, entry))
}

/// `code` in executable memory, named for the profiler.
fn publish_code(code: &[u8]) -> Option<CodeMemory> {
    let memory = CodeMemory::new(code)?;
    write_perf_map("js-native", memory.pointer() as usize, code, &[(0, code.len(), memory.pointer() as usize)]);
    Some(memory)
}

/// Eager publication has no code-cache admission budget: all requested bodies
/// must compile or the whole program fails. Owners release code and operands together.
use quench_runtime::vm::jit_backend::NativeCompileRejection;

/// Publication policy for relocation-free JIT entries. Compilation itself keeps
/// symbolic references for consumers that link functions before publication.
fn compile_native_function<T>(
    function: ir::Function,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    max_code_bytes: usize,
    publish: impl FnOnce(&[u8]) -> Option<T>,
) -> Result<T, NativeCompileRejection> {
    quench_runtime::vm::jit_backend::compile_function(function, isa, Some(max_code_bytes), |compiled, _| {
        if !compiled.buffer.relocs().is_empty() {
            return Err(NativeCompileRejection::Relocations);
        }
        publish(compiled.code_buffer()).ok_or(NativeCompileRejection::Publish)
    })
}

pub(crate) use quench_runtime::vm::engine_types::CompiledJsFunction;

pub(crate) use quench_runtime::vm::engine_types::JsNativeStep;

use quench_runtime::vm::engine_types::JsStepDispatch;




pub(crate) use quench_runtime::vm::engine_types::LaneMethod;

#[quench_macros::ext(LaneMethodOps1)]
impl LaneMethod {
    pub(crate) fn frame_reach(&self) -> usize { self.frame_reach }
    #[inline]
    pub(crate) fn recovery_space(&self) -> crate::dispatch_fast::LaneRecoverySpace { self.recovery_space }
    /// The registers of the function it runs, which an inlined callee's follow.
    pub(crate) fn caller_registers(&self) -> u16 {
        self.caller_registers
    }

    /// Whether its inlined frames' registers are in the activation's register file past the
    /// caller's (`LaneMethod::framed`).
    pub(crate) fn framed(&self) -> bool {
        self.framed
    }

    /// Its type summary, if its lowering made one and its entry cell has not withdrawn it (`lane_region::TypeSummary`).
    pub(crate) fn summary(&self) -> Option<&lane_region::TypeSummary> {
        if self.withdrawn.get() { None } else { self.summary.as_ref() }
    }

    /// The entry types its proven entry takes its parameters at, the types a direct caller proves its arguments have
    /// before it calls the proven entry (`call_frames`). Read through this one accessor, which the summary of the
    /// body's parameter types (`TypeSummary`) may come to back.
    pub(crate) fn entry_param_types(&self) -> Option<&[quench_runtime::typed_ir::Type]> {
        self.proven.as_ref().map(|(_, params)| &params[..])
    }

    /// Whether an entry, which costs a few hundred instructions the lane's own run of a
    /// short leaf never pays, has work to amortize it: a loop, an inlined call, or length
    /// (`Tiers::amortizing_instructions`). A short leaf runs in the lane, or inlined.
    pub(crate) fn amortizes_entry(&self) -> bool {
        self.amortizes
    }

    /// Whether a direct caller may call it (`LaneMethod::closed`).
    pub(crate) fn calls_only_natively(&self) -> bool {
        self.closed
    }
}

pub(crate) use quench_runtime::vm::engine_types::PublishedLaneMethod;

pub(crate) use quench_runtime::vm::engine_types::LaneExit;

/// The status of a lane body whose activation completes: its result word holds the value,
/// and `JsStepDispatch::returned_pc` the record that completes it (`LaneExit::Returned`). It
/// sits below the return statuses and above every small status.
/// The lane body's completion and exception statuses, defined once in `quench_bjit::call_record::LaneStatus`.
/// `Returned`: the activation completes, its result word holds the value and `JsStepDispatch::returned_pc`
/// names the completing record. `Threw`: the exception is in `Vm::lane_raised` and `returned_pc` names the
/// record that raised it (`LaneExit::Threw`), which native callers unwind through.
pub(crate) use quench_bjit::call_record::{LANE_BODY_RETURNED, LANE_BODY_THREW};

/// What a lane body's native call (`dispatch_fast::body_call_helper`) did: completed, with its
/// result in the call's register; missed before any effect, so the body leaves at the call;
/// or left the lane in the callee's activation it made.
/// Termination is terminal: the outer native body must not reconstruct or replay a call.
pub(crate) use quench_bjit::call_record::{
    LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_HOLDS, LANE_CALL_LEFT, LANE_CALL_MISSED, LANE_CALL_RAISED, LANE_CALL_TERMINATED,
};

#[quench_macros::ext(LaneExitOps1)]
impl LaneExit {
    /// The caller PC a leave or completion is at, with a completion's value written to the
    /// register its record names, where a test reads a `Return`'s result.
    #[cfg(test)]
    pub(crate) fn settled(self, registers: &mut [Value], instructions: &[quench_runtime::bytecode::WideInstruction]) -> Option<usize> {
        if let Self::Returned { pc, value } = self {
            registers[usize::from(instructions[pc].register_a() & quench_runtime::bytecode::REGISTER_MASK)] = value;
        }
        self.caller_pc()
    }
    #[cfg(test)]
    pub(crate) fn caller_pc(self) -> Option<usize> {
        match self { Self::At(pc) | Self::Returned { pc, .. } | Self::Threw { pc } => Some(pc), Self::Entry | Self::Stub => Some(0), Self::Inlined { .. } | Self::Callee | Self::Terminated => None }
    }
    /// Where the activation resumes: an entry leave at its first record.
    #[inline]
    pub(crate) fn resumed(self) -> Self {
        if self.entered_nothing() { Self::At(0) } else { self }
    }
}

/// A lane body's callout. Every unsupported instruction and failed guard leaves native
/// code here, with nothing of that instruction done; the lane executes it next.
unsafe extern "C" fn method_exit_in_lane(_context: u64, pc: u64, result: u64, _unused: u64) -> i64 {
    // SAFETY: the body passes the entry's result word, which `run_lane_method` owns.
    unsafe { (result as *mut u64).write(pc) };
    i64::from(NATIVE_JS_STOPPED)
}

/// A lane body's field read where its own-slot path does not apply: the VM's cache probe
/// (`Vm::cached_field_hit`), which reads prototype holders too; `DELETED` on any miss, which
/// leaves native code for the lane's full read.
unsafe extern "C" fn field_get_in_lane(vm: u64, residual: u64, receiver: Value, read: u64) -> u64 {
    #[cfg(test)] FIELD_HELPER_CALLS.with(|counts| { let mut values = counts.get(); values[0] += 1; counts.set(values); });
    // SAFETY: the payload holds the running VM and residual, live and unborrowed while the
    // body runs; the probe reads caches and never allocates.
    unsafe { cached_field_read(vm, residual, receiver, read) }
}

/// The VM's cached read of the field `read` names (its atom above its cache site): an array's
/// `length`, a proved inherited entry, else the site's caches; `Value::DELETED` for a miss.
///
/// # Safety
/// `vm` and `residual` are the running VM and residual, live and unborrowed for the call.
unsafe fn cached_field_read(vm: u64, residual: u64, receiver: Value, read: u64) -> u64 {
    let (atom, site) = ((read >> u16::BITS) as Atom, read as u16);
    // SAFETY: as the caller promises; the probe reads caches and never allocates.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram)) };
    // An array's `length`, an own data property no shape holds, answers first: a loop's
    // condition reads it every iteration (`Vm::array_length_field`).
    if let Some(length) = vm.array_length_field(receiver, atom) {
        return length.to_bits();
    }
    // A string primitive reads as the lane's does: its `length`, else `String.prototype`.
    if let Some(value) = vm.cached_string_field_hit(residual, receiver, atom, site) { return value.to_bits(); }
    // A typed array's geometry, which no field cache records, as the lane's (`LaneFrame::cached_read`).
    if vm.atom_class(atom).contains(quench_runtime::vm::object::AtomClass::VIRTUAL_FIELD)
        && let Some(value) = vm.typed_array_geometry_read(receiver, atom) { return value.to_bits(); }
    // A read native code has no descriptor for is mostly an inherited method's: its proved
    // entry answers without a walk (`Vm::proved_inherited_field_hit`), before the other tiers.
    if let Some(value) = vm.proved_inherited_field_hit(residual, receiver, site) {
        return value.to_bits();
    }
    // A miss reads a data property as the lane's does (`Vm::field_miss_data_value`).
    match vm.compiled_field_hit(residual, receiver, site) {
        quench_runtime::vm::object::FieldCacheProbe::Hit(value) => value.to_bits(),
        quench_runtime::vm::object::FieldCacheProbe::OrdinaryMiss => vm.field_miss_data_value(receiver, atom, vm.field_cache_index(site)).unwrap_or(Value::DELETED).to_bits(),
        _ => Value::DELETED.to_bits(),
    }
}

/// ToBoolean of `bits`, a value whose cell decides it (`Vm::truthy`): a read, which neither
/// allocates nor runs guest code.
unsafe extern "C" fn truthy_in_lane(vm: u64, _residual: u64, bits: Value) -> u64 {
    // SAFETY: the payload holds the running VM, live and unborrowed while the body runs.
    u64::from(unsafe { &*(vm as *const Vm) }.heap_cell_truthy(bits))
}

/// Admit a bound slot's write domain before any guest effect or raw store.
unsafe extern "C" fn field_write_admission_in_lane(vm: u64, _residual: u64, shape: u64, slot: u64, domain: u64) -> u64 {
    #[cfg(test)] FIELD_WRITE_ADMISSIONS.with(|count| count.set(count.get() + 1));
    let raw_double = domain & RAW_DOUBLE_WRITES != 0;
    let Some(domain) = quench_runtime::typed_ir::SlotType::ALL.get((domain & !RAW_DOUBLE_WRITES) as usize) else { return 0; };
    // SAFETY: the shared native entry owns the VM; admission neither allocates nor reenters.
    let vm = unsafe { &mut *(vm as *mut Vm) };
    u64::from(vm.admit_slot_writes_as(shape as u32, slot as usize, domain.ty(), raw_double))
}

/// The bit of a write admission's domain word (`field_write_admission_in_lane`) that says the
/// writer stores a slot of raw doubles as raw doubles (`StoreArm::raw_double`).
pub(crate) const RAW_DOUBLE_WRITES: u64 = 1 << 16;

/// A logged own-field store that preserves every native storage projection.
/// Migration or observed type widening declines before writing, leaving the full
/// store to the lane at its original PC after retiring dependent native entries.
unsafe extern "C" fn field_set_in_lane(vm: u64, residual: u64, receiver: Value, atom: u64, site: u64, value: Value) -> u64 {
    #[cfg(test)] FIELD_HELPER_CALLS.with(|counts| { let mut values = counts.get(); values[1] += 1; counts.set(values); });
    // SAFETY: as `field_get_in_lane`; a hit stores into an existing slot and allocates nothing.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram)) };
    #[cfg(feature = "profile-native-ops")]
    let storage = {
        use quench_runtime::vm::native_op_profile::StoreStorage;
        match receiver.direct_object_index() {
            None => StoreStorage::NonRecord,
            Some(id) => {
                let object = vm.heap_reads().object_record_by_id(vm.heap_reads().record_id(id));
                if object.plain_storage_start().is_some() { StoreStorage::Arena }
                else if object.properties().has_inline_property_storage() { StoreStorage::Inline }
                else { StoreStorage::OtherRecord }
            }
        }
    };
    // A receiver of another shape the site met stores through its polymorphic entries, as
    // long as the store keeps the record's layout, which the caller's proofs rely on.
    let (receiver, value, layout) = (receiver, value, quench_runtime::tier_surface::PropertyStoreLayout::PreserveNative);
    // Only the primary entry's hit proves its slot's representation (`general_store`).
    let primary = vm.primary_own_field_store_hit::<true>(residual, receiver, atom as Atom, value, site as u16, layout);
    if primary { general_store::record_store_class(vm, site as u16, value); }
    let hit = primary
        || vm.polymorphic_own_field_store_hit(residual, receiver, atom as Atom, value, site as u16, layout).is_some();
    #[cfg(feature = "profile-native-ops")]
    quench_runtime::vm::native_op_profile::record_store(storage, hit);
    u64::from(hit)
}

/// Available guest activations, stable until guest reentry. Closed native regions
/// bind this at entry; regions with calls read it at each binding edge.
unsafe extern "C" fn call_budget_in_lane(_vm: u64, _residual: u64) -> u64 {
    #[cfg(test)] CALL_BUDGET_READS.with(|count| count.set(count.get() + 1));
    quench_runtime::stack::GuestCallGuard::remaining() as u64
}

/// The word naming the record a lane body's `throw` raised at: its function above the record.
pub(crate) fn raise_site(function: u32, record: u32) -> u64 {
    u64::from(function) << u32::BITS | u64::from(record)
}

/// The function and record `raise_site` names.
pub(crate) fn raise_site_parts(site: u64) -> (u32, u32) {
    ((site >> u32::BITS) as u32, site as u32)
}

/// A lane body's `throw` of `value` that no handler of its function takes: the exception in
/// `Vm::lane_raised`, with where it was thrown, for native callers and then the lane to find.
/// The interpreter's `Throw` makes the same error; its source location is captured only if it
/// escapes native code (`dispatch_fast::raised_exit_in_lane`).
unsafe extern "C" fn compiled_raise(vm: u64, _residual: u64, value: Value, site: u64) -> u64 {
    // SAFETY: as `field_get_in_lane`; the message of a value reads no guest code.
    let vm = unsafe { &mut *(vm as *mut Vm) };
    let message = vm.detail_string(value);
    vm.lane_completion.raise(JsError::thrown(value, message));
    vm.lane_raise_site = site;
    0
}

/// The exception a callee's body raised (`compiled_raise`), taken from `Vm::lane_raised` for the
/// handler of the caller that catches it.
unsafe extern "C" fn take_raised_in_lane(vm: u64, _residual: u64) -> u64 {
    // SAFETY: as `field_get_in_lane`.
    let vm = unsafe { &mut *(vm as *mut Vm) };
    let error = vm.lane_completion.take_raised().expect("a raised call left its exception");
    error.thrown_value().expect("a body raises a thrown value").to_bits()
}

/// A lane body's global name read (`Vm::global_read_in_lane`): its value, or `Value::DELETED` for one the lane's own read would not take.
unsafe extern "C" fn global_get_in_lane(vm: u64, residual: u64, atom: u64, site: u64) -> u64 {
    // SAFETY: as `field_get_in_lane`; the read runs no guest code and allocates nothing.
    let (vm, residual) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram)) };
    vm.global_read_in_lane(residual, atom as Atom, site as u16).unwrap_or(Value::DELETED).to_bits()
}

/// A lane body's strict equality of two distinct heap values of one content-compared family
/// (`Vm::heap_values_strictly_equal`): 1 for equal strings or BigInts, else 0.
unsafe extern "C" fn strict_equal_cells_in_lane(vm: u64, _residual: u64, left: Value, right: Value) -> u64 {
    // SAFETY: as `field_get_in_lane`; the comparison reads the heap and allocates nothing.
    let vm = unsafe { &*(vm as *const Vm) };
    u64::from(vm.heap_values_strictly_equal(left, right))
}

/// A lane body's `value instanceof constructor`, decided as the interpreter's own record
/// decides it (`lane_instanceof_value`): the constructor's proof (`Vm::record_instanceof`),
/// else the ordinary walk (`Vm::ordinary_instanceof`), which calls nothing. 1 or 0, or
/// `Value::DELETED` where only the full algorithm decides, which leaves the body at the test.
unsafe extern "C" fn instance_of_in_lane(vm: u64, _residual: u64, value: Value, constructor: Value) -> u64 {
    // SAFETY: as `field_get_in_lane`; both reads walk the heap, call nothing and allocate nothing
    // (the walk only fills the constructor-layout memo).
    let vm = unsafe { &mut *(vm as *mut Vm) };
    vm.record_instanceof(value, constructor)
        .or_else(|| vm.ordinary_instanceof(value, constructor))
        .map_or_else(|| Value::DELETED.to_bits(), u64::from)
}

/// A lane body's fused `typeof` test of a heap value (`Vm::typeof_test_value`): 1 where its
/// name is the one at `index` of `bytecode::TYPEOF_NAMES`, else 0.
/// The tagged Number of the double `bits` (`Value::number`): a leave's boxing of an unboxed
/// register, called from its cold block instead of the inline encoding's two blocks and
/// fifteen instructions per register.
unsafe extern "C" fn box_number_in_lane(_vm: u64, _residual: u64, bits: u64) -> u64 {
    Value::number(f64::from_bits(bits)).to_bits()
}

unsafe extern "C" fn typeof_is_in_lane(vm: u64, _residual: u64, value: Value, index: u64) -> u64 {
    // SAFETY: as `field_get_in_lane`; the test reads the value's cell and allocates nothing.
    let vm = unsafe { &*(vm as *const Vm) };
    u64::from(vm.typeof_test_value(value, index as usize, false) == Value::TRUE)
}

/// The index in `bytecode::TYPEOF_NAMES` of `typeof value` (`Vm::typeof_index_value`): a heap value's by its
/// cell. Reads the cell and allocates nothing.
unsafe extern "C" fn typeof_index_in_lane(vm: u64, _residual: u64, value: Value) -> u64 {
    // SAFETY: as `typeof_is_in_lane`.
    let vm = unsafe { &*(vm as *const Vm) };
    vm.typeof_index_value(value).as_int().map_or(0, |index| index as u64)
}

/// A lane body's store of `value` to the field the packed `write` names (its atom above its
/// cache site) of `receiver`, as the lane's own store at an adding site: into a slot the
/// object has (`Vm::primary_own_field_store_hit`, or a polymorphic site's
/// `Vm::polymorphic_own_field_store_hit`), else the cached add of the property
/// (`Vm::primary_field_add_hit`), which may change the object's shape and move the
/// property arena, whose base and length the dispatch reads again. Zero where neither hits,
/// or a collection is due, which the body leaves for.
unsafe extern "C" fn field_add_in_lane(vm: u64, residual: u64, payload: u64, receiver: Value, write: u64, value: Value) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running one.
    let (vm, residual, dispatch) = unsafe {
        (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch))
    };
    let (atom, site) = ((write >> u16::BITS) as Atom, write as u16);
    if vm.primary_own_field_store_hit::<true>(residual, receiver, atom, value, site, quench_runtime::tier_surface::PropertyStoreLayout::Preserve) {
        return 1;
    }
    if vm.collection_due() {
        return 0;
    }
    // A site past one shape stores through its megamorphic own entries, which may widen the
    // object's storage; the body retires its proofs after this store either way.
    if vm.polymorphic_own_field_store_hit(residual, receiver, atom, value, site, quench_runtime::tier_surface::PropertyStoreLayout::MayWiden).is_none()
        && !vm.primary_field_add_hit(residual, receiver, atom, value, site) {
        return 0;
    }
    crate::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    1
}

/// The slow arm of a loop's hoisted typed array view proof (`Vm::publish_typed_view`): publishes
/// the typed array's elements in its record, which the proof then reads; zero where the loop
/// cannot address them in place, which leaves at the loop's header. The count is below 2^31
/// (`staged::typed_elements::TypedView`): a longer array leaves too.
unsafe extern "C" fn typed_array_view_in_lane(vm: u64, _residual: u64, object: Value) -> u64 {
    // SAFETY: as `field_get_in_lane`; the publication reads cells and allocates nothing.
    let vm = unsafe { &mut *(vm as *mut Vm) };
    u64::from(vm.publish_typed_view(object))
}

/// A lane body's element store: the lane's own in-place replacement of a record array's
/// element (`Vm::try_replace_record_element`) or fill of a hole it may take
/// (`Vm::try_add_record_element`), whose miss changes nothing and leaves the body at the store.
unsafe extern "C" fn index_set_in_lane(vm: u64, _residual: u64, object: Value, key: Value, value: Value) -> u64 {
    // SAFETY: as `field_get_in_lane`; a replacement allocates nothing.
    let vm = unsafe { &mut *(vm as *mut Vm) };
    // Collection debt leaves before the store, as the verified lowering's Grow exit does:
    // the lane performs the store and the VM collects at its rooted safepoint.
    if vm.collection_due() {
        return 0;
    }
    u64::from(vm.try_replace_record_element(object, key, value) || vm.try_add_record_element(object, key, value))
}

/// Retires the running lane's array storage proofs (`retire_array_storage_proofs`), which a
/// native `pop` calls once it has shortened an array, as a heap change does.
/// The return of an emitted call whose native header a nested entry to emitted code spilled
/// (`LaneStack::spill_plain`): the header is the innermost record now, and pops as one.
unsafe extern "C" fn pop_observed_header_in_lane(vm: u64, _residual: u64) -> u64 {
    // SAFETY: as `field_get_in_lane`.
    let vm = unsafe { &mut *(vm as *mut Vm) };
    vm.lane_stack.pop_observed_header();
    0
}

unsafe extern "C" fn retire_array_proofs_in_lane(_vm: u64, _residual: u64) -> u64 {
    quench_runtime::tier_surface::retire_array_storage_proofs();
    0
}

/// A lane loop's room for appending to the dense array `object` (`Vm::reserve_native_array_append`):
/// the room its backing holds, or zero, which leaves the append to `index_set_in_lane`.
unsafe extern "C" fn array_append_room_in_lane(vm: u64, _residual: u64, object: Value) -> u64 {
    // SAFETY: as `field_get_in_lane`; a growth moves only this array's backing.
    let vm = unsafe { &mut *(vm as *mut Vm) };
    vm.reserve_native_array_append(object).map_or(0, |room| room as u64)
}

/// 1 where no prototype of the array `object` holds an index (`Vm::prototypes_hold_no_index`), else 0.
unsafe extern "C" fn prototypes_hold_no_index_in_lane(vm: u64, _residual: u64, object: Value) -> u64 {
    // SAFETY: as `array_append_room_in_lane`; the check reads the heap and writes nothing.
    let vm = unsafe { &*(vm as *const Vm) };
    u64::from(vm.prototypes_hold_no_index(object))
}

#[quench_macros::ext(CompiledVm2)]
impl Vm {
    /// Room for one more element of `array` (a dense array, `dense_array_length`, whose
    /// backing it owns alone and no prototype of which holds an index), so that OrdinarySet
    /// of the index at its length defines a plain element in place: the backing grows when
    /// full, storing, calling and collecting nothing. The answer, the room the backing holds,
    /// bounds the lengths an `i32` index reaches (`MAX_INDEXED_LENGTH`). `None` for any other
    /// array, and where collection is due, before or after a growth: the store leaves for
    /// the VM, which collects at its safepoint.
    fn reserve_native_array_append(&mut self, array: Value) -> Option<usize> {
        if self.collection_due() { return None; }
        let length = self.dense_array_length(array)?;
        if length >= osr_loop::MAX_INDEXED_LENGTH || !self.prototypes_hold_no_index(array) { return None; }
        // A Number store logs nothing.
        let elements = self.record_array_store_mut_unlogged(array, Value::integer(0))?.elements_mut();
        if elements.owner_count() != 1 { return None; }
        // A double backing keeps its kind (`ArrayElements::reserve_double`).
        let (before, room) = if elements.is_double() {
            let before = elements.capacity();
            elements.reserve_double(1);
            (before, elements.capacity())
        } else {
            let elements = quench_runtime::vm::index::mutable_array_elements(elements);
            let before = elements.capacity();
            elements.reserve(1);
            (before, elements.capacity())
        };
        self.charge_payload_growth((room - before) * size_of::<Value>());
        (!self.collection_due()).then_some(room.min(osr_loop::MAX_INDEXED_LENGTH))
    }
}

/// Admit a depth-zero closure capture without walking, allocating or invoking
/// guest code. Owned slots and plain root projections use the lane's shared reads;
/// accessors, uninitialized bindings and other projections decline at the read.
unsafe extern "C" fn function_capture_read_in_lane(vm: u64, callee: Value, slot: u64) -> u64 {
    // SAFETY: the lane owns this VM while generated code runs; no collection or
    // reentry occurs while it holds the closure's environment slots.
    let vm = unsafe { &*(vm as *const Vm) };
    let read = || {
        let env = vm.function_record(callee)?.env;
        let slot = u16::try_from(slot).ok()?;
        let (slots, root) = vm.environment_capture_slots(env, false, 0)?;
        vm.owned_capture(slots, root, slot)
            .or_else(|| vm.plain_projected_root_binding(env, slot))
    };
    read().unwrap_or(Value::DELETED).to_bits()
}

#[quench_macros::ext(CompiledVm3)]
impl Vm {
    /// The value a global read of `atom` through cache `site` takes as the lane's own read
    /// does (`dispatch_fast::global_name_refresh_miss`): its global lexical binding's, else
    /// the global object's own data property or cached field; none for a binding in its TDZ or a miss.
    pub(crate) fn global_read_in_lane(&mut self, code: &ResidualProgram, atom: Atom, site: u16) -> Option<Value> {
        match self.global_name_read(code, atom, site) {
            quench_runtime::vm::field_cache::GlobalNameRead::Value(value, _) => Some(value),
            quench_runtime::vm::field_cache::GlobalNameRead::Uninitialized | quench_runtime::vm::field_cache::GlobalNameRead::Miss => None,
        }
    }

    /// The field cache sites `unit` reads or writes, each with the exact shapes and slots its
    /// entries prove now: a read's own or prototype-held (`Vm::native_field_read_descriptors`),
    /// a store's own (`Vm::native_field_load_descriptors`); a nested lookup names one site
    /// per read of its chain, and its sink's store.
    /// The field cache sites `instruction` reads (`true`) or stores through (`false`).
    /// Counts a run of a lane body that left at the instruction `pc` of `function`, on each
    /// field site it has: the leave path's one write of a site's instability.
    pub(crate) fn note_field_leave(&mut self, program: &ResidualProgram, function: u32, pc: usize) {
        let Some(instruction) = program.function_get(function as usize).and_then(|body| body.instructions().nth(pc)) else { return };
        for (site, _) in Self::instruction_field_sites(program, &instruction) {
            self.record_field_leave(self.field_cache_index(site));
        }
    }

    fn instruction_field_sites(program: &ResidualProgram, instruction: &WideInstruction) -> Vec<(u16, bool)> {
        quench_runtime::staged::Operation::<u64, ()>::field_cache_sites(*instruction, &program.field_sites)
    }

    /// The proved shapes and slots of `unit`'s field sites, but those of the instructions at
    /// `generic`, where a previous body left on its runs: their proofs did not hold for the
    /// values they met (a function's own field, read through a plain object's proof), so
    /// they take the VM's cached read, which answers them (`lane_region::field_probe`).
    /// `view` is the lane view of `function`, whose own instructions' records hold the own-field
    /// hints (`LaneView::own_field_hint`); each own read's hint becomes its loads' `formed` layout
    /// id, which a load names only where it is the load's shape (`lane_region::field_plans`). An
    /// inlined instruction's record belongs to another body's view, so it takes no hint.
    fn unit_fields_in_lane(&self, program: &ResidualProgram, unit: &lane_unit::Unit, generic: &[usize],
        view: Option<&crate::dispatch_fast::LaneView>) -> Vec<Option<Vec<NativeFieldLoad>>> {
        let (generic, functions): (Vec<u16>, _) = (generic.iter().flat_map(|pc| Self::instruction_field_sites(program, &unit.instructions[*pc]))
            .map(|(site, _)| site).collect(), self.heap_reads().function_shapes());
        let sites = unit.instructions
            .iter()
            .enumerate()
            .flat_map(|(pc, instruction)| Self::instruction_field_sites(program, instruction).into_iter().map(move |(site, read)| (pc, site, read)))
            .filter(|(_, site, _)| !generic.contains(site))
            .map(|(pc, site, read)| {
                let formed = match (read, view, unit.origins.get(pc)) {
                    (true, Some(view), Some(lane_unit::Origin::Caller(caller))) => view.own_field_hint(*caller as usize),
                    _ => None,
                };
                let loads = if read { self.native_field_read_descriptors(site) } else { self.native_field_load_descriptors(site) };
                (site, loads.into_iter().map(|load| NativeFieldLoad { function_shape: functions.contains(load.shape as usize), formed, ..load }).collect::<Vec<_>>())
            })
            .filter(|(site, shapes)| !shapes.is_empty() && site_posterior::SiteEvidence::of(self.programs.tiers(), self.field_site_leaves(self.field_cache_index(*site)), shapes.len()).speculates())
            .map(|(site, shapes)| (usize::from(site), shapes));
        lane_region::flat_table(sites)
    }

    /// Run a lane body on an activation's `registers`; the PC the lane continues at. A
    /// native `Return` leaves at its own record, whose register already holds the result.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_lane_method(
        &mut self,
        method: &LaneMethod,
        direct_calls: bool,
        program: ProgramId,
        residual: &ResidualProgram,
        registers: *mut Value,
        receiver: Value,
        captures: *const usize,
        lane_context: *mut (),
    ) -> LaneExit {
        self.run_lane_body(&BodyRun {
            method, direct_calls, program, residual, registers, receiver, captures, lane_context,
            pending: NO_PENDING_ACTIVATIONS, bases_stand: false,
        })
    }

    /// Points the lane frame's field cache entries at the table's current mapping, where the
    /// frame is this VM's: its payload word is what generated code reads, and it was bound
    /// before the table moved (`Vm::field_caches_moved`).
    pub(crate) fn rebind_lane_field_entries(&self) {
        // SAFETY: the frame is reached only through this pointer, on its thread; the table
        // moves in the VM's own code, never inside a payload access.
        let frame = unsafe { &mut *LANE_FRAME.with(std::cell::UnsafeCell::get) };
        if let Some(frame) = frame.as_mut().filter(|frame| frame.dispatch.vm == std::ptr::from_ref(self).addr()) {
            frame.dispatch.field_entries = self.program_field_entries(frame.dispatch.program_id).unwrap_or(0);
        }
    }

    /// `run_lane_method` from inside a running body (`dispatch_fast::body_call_helper`): the
    /// caller's body continues with the per-entry words it entered with. Its callee runs in
    /// the same program (`crosses_in_lane`) and allocates nothing, so the program's bases stand.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_nested_lane_method(
        &mut self,
        method: &LaneMethod,
        program: ProgramId,
        residual: &ResidualProgram,
        registers: *mut Value,
        receiver: Value,
        captures: *const usize,
        lane_context: *mut (),
    ) -> LaneExit {
        let frame = frame_in_lane();
        // SAFETY: as `run_lane_method`; the caller's entry made the frame.
        let saved = unsafe { LaneEntry::of(&(*frame).dispatch) };
        // SAFETY: as above.
        let admits_frames = unsafe { (*frame).admits_frames };
        let moved_from = self.cache_epochs.field_entries;
        // Reentrant entries lend the same TLS context. Preserve the outer
        // recovery landing and its binding before the inner entry overwrites them.
        let binding = unsafe {
            let context = &(*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
        };
        // The callee runs in the caller's program and its caller's helper allocated nothing since
        // the caller's body last checked the bases (`BodyRun::bases_stand`).
        let exit = self.run_lane_body(&BodyRun {
            method, direct_calls: true, program, residual, registers, receiver, captures, lane_context,
            // An entry from outside any body (`saved.residual == 0`) checks them as any entry does.
            pending: NO_PENDING_ACTIVATIONS, bases_stand: saved.residual != 0,
        });
        // SAFETY: as above.
        unsafe {
            // An entry from outside any body (the interpreter's call) found the run's words
            // unbound: they stay bound to the run it made, so the next such entry of the same run
            // binds none of them again (`BodyRunContext::bind_run`); the per-entry words go back.
            let saved = if saved.residual == 0 && saved.field_entries == 0 { saved.keeping_run_of(&(*frame).dispatch) } else { saved };
            saved.install(&mut (*frame).dispatch);
            (*frame).admits_frames = admits_frames;
            if self.cache_epochs.field_entries != moved_from { self.rebind_lane_field_entries(); }
            let context = &mut (*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner) = binding;
        }
        exit
    }

    /// Copies the registers that follow the first `from` of the activation at `registers`, where a
    /// stitched body kept its inlined callees' (`LaneMethod::framed`), into the scratch window a
    /// leave inside one reads (`scratch_in_lane`), before the activations it makes overwrite them.
    pub(crate) fn adopt_window_in_lane(registers: *const Value, from: usize, width: usize) {
        let frame = frame_in_lane();
        // SAFETY: as `stack_words_in_lane`; the body's admission proved the room past the
        // activation's registers (`LaneRecoverySpace`), and no lane body runs while the lane reads it.
        unsafe {
            for (at, slot) in (*frame).scratch.iter_mut().take(width).enumerate() {
                *slot = registers.add(from + at).read();
            }
        }
    }

    /// Reloads the heap bases a running body's rows read through the payload
    /// (`JsStepDispatch::refresh_object_bases`): a helper that allocated or ran guest code may
    /// have moved the record tables or the barrier's mark blocks. Every call, construct and step
    /// helper ends with it, so a row after one reads current bases and checks none.
    /// Binds the running body's direct-call stack words again (`stack_entry_in_lane`) after a helper's call that
    /// completed: the call's frames pushed and popped through the general loop (`LaneStack::frames_changed`), which
    /// zeroed the plain depth limit, and without a new binding every later direct call of the run would refuse to
    /// the helper row too. Only for a run whose body names direct calls (`admits_frames`), whether or not it entered
    /// as a leaf: the helper's call materialized the leaf (`materialize_running_leaf_in_lane`).
    pub(crate) fn rearm_direct_calls(&mut self) {
        let lane = frame_in_lane();
        // SAFETY: as below.
        if !unsafe { (*lane).admits_frames } || self.lane_stack.plain_depth_limit.get() != 0 { return; }
        let entry = self.stack_entry_in_lane(SHADOW_FRAMES);
        // SAFETY: the frame lives for the thread; the running body's helper is the payload's only user.
        unsafe { entry.bind(&mut (*lane).dispatch) };
    }

    pub(crate) fn refresh_body_bases(&mut self) {
        let lane = frame_in_lane();
        // SAFETY: the frame lives for the thread; the running body's helper is the payload's only user.
        unsafe { (*lane).dispatch.refresh_object_bases(self) };
    }

    /// Publishes `receiver`, a `this` the running body's frame has just bound (`InitializeThis`), in the
    /// window's header the body's rows read: the frame's own and the header's are written by the one step.
    pub(crate) fn publish_body_receiver(&self, receiver: Value) {
        let frame = frame_in_lane();
        // SAFETY: as `refresh_body_bases`; the running body's window is the dispatch's registers.
        unsafe { set_frame_word((*frame).dispatch.native_registers, CONTEXT_THIS, receiver.to_bits()) }
    }

    /// Reloads the heap bases of the payload of the native loop region the lane entered, whose address the
    /// lane context holds (`dispatch_fast::region_payload_in_lane`; zero where none runs), as
    /// `refresh_body_bases` does the body frame's, and the lane stack's room the call may have moved
    /// (`region_lend::bind_stacks`, `direct_calls` as the region was entered with).
    ///
    /// The stacks are rebound only where no plain frame stands above the region: the region's entry spilled
    /// every one below it (`region_lend::lend`), so a plain frame here is a direct call's still running (a
    /// stencil call row's push, whose pop restores the plain depth, window and body top it found). Binding
    /// spills it into a record that pop never removes: each such call that allocated left one record more,
    /// until the walk over them outgrew the activation limit (ai-astar under `--b-jit`).
    pub(crate) fn refresh_region_bases(&mut self, payload: usize, direct_calls: bool) {
        // SAFETY: a region's payload lives for its run, which the helper that calls this is made from.
        if let Some(payload) = unsafe { (payload as *mut JsStepDispatch).as_mut() } {
            payload.refresh_object_bases(self);
            if !self.lane_stack.has_plain_frames() {
                region_lend::bind_stacks(self, payload, direct_calls);
            }
        }
    }

    /// Hands `write` each of the `count` registers an inlined callee its lane body left
    /// inside held, as the body wrote them to the scratch window (`BodyRunContext::scratch`).
    pub(crate) fn scratch_in_lane(start: usize, count: usize, mut write: impl FnMut(u16, Value)) {
        LANE_FRAME.with(|frame| {
            // SAFETY: as in `run_lane_method`; no lane body runs while the lane reads it.
            let Some(frame) = (unsafe { &*frame.get() }).as_ref() else { return };
            for (register, value) in frame.scratch[start..start + count].iter().enumerate() {
                write(register as u16, *value);
            }
        });
    }
}

/// Direct calls a run nests natively: past them, a call takes the lane's helper path. The
/// machine stack each costs a frame; the bound keeps a native recursion within it.
const SHADOW_FRAMES: usize = 64;

/// The stride of the native frames a run pushes: an activation record, whose first words they are.
pub(crate) const FRAME_BYTES: usize = std::mem::size_of::<Activation>();

/// The words of `JsStepDispatch` one lane body entry owns (`Vm::run_lane_method`).
#[derive(Clone, Copy)]
struct LaneEntry {
    program_id: ProgramId,
    field_entries: usize,
    lane_context: usize,
    native_registers: *mut Value,
    /// The activation's local slots and their count (`dispatch_fast::body_locals`), which a
    /// body's local rows read through the payload.
    native_locals: (*mut Value, usize),
    /// The storage of its environment-owned locals (`JsStepDispatch::environment_storage`).
    environment_storage: u64,
    residual: usize,
    /// The end of the frames the run may nest, which a nested run narrows and its caller regains.
    frames_end: usize,
}

impl LaneEntry {
    #[inline]
    fn of(dispatch: &JsStepDispatch) -> Self {
        Self {
            program_id: dispatch.program_id,
            field_entries: dispatch.field_entries,
            lane_context: dispatch.lane_context,
            native_registers: dispatch.native_registers,
            native_locals: (dispatch.native_locals, dispatch.local_count),
            environment_storage: dispatch.environment_storage,
            residual: dispatch.residual,
            frames_end: dispatch.lane_frames_end,
        }
    }

    /// This entry's per-entry words with the run words `dispatch` holds now: its program, field
    /// cache entries and residual.
    fn keeping_run_of(self, dispatch: &JsStepDispatch) -> Self {
        Self { program_id: dispatch.program_id, field_entries: dispatch.field_entries, residual: dispatch.residual, ..self }
    }

    #[inline]
    fn install(self, dispatch: &mut JsStepDispatch) {
        dispatch.program_id = self.program_id;
        dispatch.field_entries = self.field_entries;
        dispatch.lane_context = self.lane_context;
        dispatch.native_registers = self.native_registers;
        (dispatch.native_locals, dispatch.local_count) = self.native_locals;
        dispatch.environment_storage = self.environment_storage;
        dispatch.residual = self.residual;
        dispatch.lane_frames_end = self.frames_end;
    }
}

/// The words of the dispatch payload and context that stand for every entry of one lane run: its VM, program, residual,
/// store specialization and lane stack (its values' top word's address), and what follows from them, the field cache
/// entries and the stack's storage addresses (`BodyRunContext::bind_run`). Per entry it writes its lane context
/// (compared only after it is written), its registers and the stack's room.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LaneRun {
    vm: usize,
    program_id: ProgramId,
    lane_context: usize,
    residual: usize,
    stores_specialized: usize,
    stack: usize,
}

impl LaneRun {
    #[inline]
    fn of(dispatch: &JsStepDispatch) -> Self {
        Self { vm: dispatch.vm, program_id: dispatch.program_id, lane_context: dispatch.lane_context, residual: dispatch.residual, stores_specialized: dispatch.stores_specialized, stack: dispatch.lane_values_top }
    }
}

/// The context and payload every lane body on this thread runs with
/// (`Vm::run_lane_method`). Thread facts are set once; each entry writes its own.
struct BodyRunContext {
    context: CallContext,
    dispatch: JsStepDispatch,
    /// The registers of an inlined callee its lane body left inside.
    scratch: Vec<Value>,
    /// Whether the running body's entry admitted direct-call frames (`bind_body_entry`): a helper's completed call
    /// binds them again for it (`Vm::rearm_direct_calls`). A nested entry keeps its caller's (`run_nested_lane_method`).
    admits_frames: bool,
}

impl BodyRunContext {
    /// Binds the payload and context to `run`'s words, written once per run: a nested entry of the same run
    /// (`Vm::run_nested_lane_method`) and each next entry of the lane's calls find them standing. `field_entries` derives
    /// the program's field cache entries, which stand until the table moves (`Vm::rebind_lane_field_entries`). The run's VM
    /// is bound after it (`JsStepDispatch::bind_lane_storage`), so a VM that is not the last entry's makes a new run. The
    /// native stack's limit is anchored where the run binds: the lane run's frame is at one depth for all its entries (its
    /// calls are activations, not native frames), a deeper run (a native callback's) has its own context and so binds its
    /// own, and the nested entries of one run (`Vm::run_nested_lane_method`) spend the run's budget. The interrupt and the
    /// payload, whose address is the frame's, bind once per interrupt owner. The lane stack's storage addresses (`storage`)
    /// bind with the run: no reallocation moves them.
    /// Whether the entry bound a different run than the last one's.
    #[inline(always)]
    fn bind_run(&mut self, run: LaneRun, interrupt: &quench_runtime::ExecutionInterrupt, field_entries: impl FnOnce() -> usize, storage: impl FnOnce() -> crate::dispatch_fast::LaneStackStorage) -> bool {
        let owner = std::ptr::from_ref(interrupt);
        // The lane context is the entry's own activation, written by every entry; the other
        // words stand for the whole run.
        self.dispatch.lane_context = run.lane_context;
        let rebound = LaneRun::of(&self.dispatch) != run;
        if rebound {
            self.dispatch.field_entries = field_entries();
            self.context.stack_limit = native_stack_limit();
            self.dispatch.program_id = run.program_id;
            self.dispatch.residual = run.residual;
            self.dispatch.stores_specialized = run.stores_specialized;
            storage().bind(&mut self.dispatch);
        }
        if self.context.interrupt_owner != owner {
            self.context.interrupt = interrupt.atomic_flag() as *const _;
            self.context.interrupt_owner = owner;
            self.context.adapter_payload = (&raw mut self.dispatch).cast();
        }
        rebound
    }

    fn new() -> Self {
        let mut context = CallContext::native_adapter(std::ptr::null_mut(), std::ptr::null(), None);
        context.callout = method_exit_in_lane as unsafe extern "C" fn(u64, u64, u64, u64) -> i64 as usize;
        Self {
            context,
            dispatch: JsStepDispatch::idle(),
            scratch: Vec::new(),
            admits_frames: false,
        }
    }
}

thread_local! {
    static LANE_FRAME: std::cell::UnsafeCell<Option<BodyRunContext>> = const { std::cell::UnsafeCell::new(None) };
}

/// This thread's lane frame, made at its first lane body run.
/// The lowering line of the guard the lane body that just ran left at, reset; zero where it
/// recorded none (`VmFacts::diagnostics`).
pub(crate) fn take_lane_exit_line() -> u64 {
    // SAFETY: as `lane_frame`.
    unsafe { std::mem::take(&mut (*frame_in_lane()).dispatch.exit_line) }
}

#[inline]
/// Whether a lane body runs on this thread now: its entry's words are the thread's lane frame's
/// (`LaneEntry::residual`), which another body's entry must keep (`Vm::run_nested_lane_method`).
pub(crate) fn body_running_in_lane() -> bool {
    let frame = frame_in_lane();
    // SAFETY: as `frame_in_lane`; the words are read and dropped here.
    unsafe { LaneEntry::of(&(*frame).dispatch) }.residual != 0
}

fn frame_in_lane() -> *mut BodyRunContext {
    let frame = LANE_FRAME.with(std::cell::UnsafeCell::get);
    // SAFETY: the frame is reached only through this pointer, on its thread, and no
    // reference into it outlives the statement that makes it.
    unsafe { (*frame).get_or_insert_with(BodyRunContext::new) as *mut BodyRunContext }
}


#[quench_macros::ext(JsStepDispatchOps1)]
impl JsStepDispatch {
    /// Construct and consume one owned activation in the entry's scope. The
    /// large dispatch does not cross an optional aggregate-return boundary.
    #[cfg(feature = "native-execution")]
    #[inline(always)]
    fn with<R>(vm: &mut Vm, program_id: ProgramId, frame: usize, start_pc: usize,
        allow_inline_calls: bool, views: bool, run: impl FnOnce(&mut Self, &mut Vm) -> R) -> Option<R> {
        let function = vm.frames[frame].function;
        Self::with_function(vm, program_id, (frame, function), start_pc, allow_inline_calls, views, run)
    }

    /// `with` for `function` of `program_id`, which runs above VM frame `frame`: a frameless
    /// activation's function is not its frame's, and may not be of the frame's program.
    /// The program is held as a local for the run: the payload borrows it (`residual`), and
    /// holding it keeps its record from unique-owner release while the dispatch runs. Reentrant
    /// entries own separate payloads, never a VM-local ID in TLS.
    #[inline(always)]
    fn with_function<R>(vm: &mut Vm, program_id: ProgramId, (frame, function): (usize, u32), start_pc: usize,
        allow_inline_calls: bool, views: bool, run: impl FnOnce(&mut Self, &mut Vm) -> R) -> Option<R> {
        let program = vm.programs.get(program_id)?;
        let body = program.function_get(function as usize)? as *const _;
        let vm_word = vm as *mut Vm as usize;
        let activation = &mut vm.frames[frame];
        let mut dispatch = JsStepDispatch {
            head: JsStepHead {
                native_locals: activation.locals.as_mut_ptr(),
                local_count: activation.locals.len(),
                vm: vm_word,
                residual: Rc::as_ptr(&program) as usize,
                ..JsStepDispatch::idle().head
            },
            program_id, frame, allow_inline_calls, start_pc,
            native_registers: activation.registers.as_mut_ptr(),
            register_count: activation.registers.len(),
            body,
            ..JsStepDispatch::idle()
        };
        // The prologue already coerced `this` (`Vm::call_this_value`); a frame that has not
        // initialized it leaves the receiver `DELETED`, at which a body's `LoadThis` leaves.
        let frame = &vm.frames[frame];
        let this = if frame.fixed_this { frame.this } else { Value::DELETED };
        // SAFETY: the activation's register file is live for this run, and its header is its own.
        unsafe { set_frame_word(dispatch.native_registers, CONTEXT_THIS, this.to_bits()) };
        if views { dispatch.refresh_heap_bases(vm, &program); }
        Some(run(&mut dispatch, vm))
    }

    /// The payload of no activation: no frame, registers, locals or VM, every base word unbound.
    fn idle() -> Self {
        Self {
            head: JsStepHead {
                loop_entry: 0,
                array_record_base: 0, collection_record_base: 0, string_record_base: 0, object_record_base: 0, function_record_base: 0,
                class_base: 0,
                object_mark_blocks: 0, object_mark_block_count: 0, function_mark_blocks: 0, function_mark_block_count: 0,
                property_buffer: 0, field_entries: 0, native_locals: std::ptr::null_mut(), local_count: 0,
                epoch: 0, method_cache_atoms: 0, method_cache_atom_words: 0, stores_specialized: 0, store_watch: 0,
                empty_string: Value::DELETED.to_bits(),
                environment_storage: quench_runtime::tier_surface::absent_environment_storage(),
                array_mark_blocks: 0, array_mark_block_count: 0,
                typed_view_directory: 0, typed_view_pages: 0, buffer_relocations: 0,
                vm: 0, residual: 0, lane_context: 0, returned_pc: 0,
                lane_values_top: 0, lane_values_end: 0, lane_activations_top: 0, lane_frames_end: 0,
                stub_cursor: 0, object_cursor: 0, empty_object_layout: 0, empty_object_payload: 0, empty_object_proto: 0,
                array_cursor: 0, empty_array_layout: 0, empty_array_payload: 0, empty_array_proto: 0,
                ascii_unit_strings: 0,
                armed_array_proofs: quench_runtime::tier_surface::armed_proofs_address(),
                array_chain_index_free_address: quench_runtime::tier_surface::index_free_chain_link_address(),
                plain_depth: 0, plain_depth_limit: 0, body_table: 0, activation_count: 0, body_top: 0, native_table: 0,
                pop_observed_header: pop_observed_header_in_lane as *const () as usize,
            },
            program_id: ProgramId::MAIN, frame: usize::MAX, allow_inline_calls: false, start_pc: 0,
            #[cfg(feature = "native-execution")]
            stop_pc: JS_NO_STOP_PC,
            native_registers: std::ptr::null_mut(), register_count: 0,
            fixed_this_slot: std::ptr::null(),
            call_cache: 0, layout_seen: u64::MAX, bases_vm: 0, frameless: 0, call_targets: 0, element_get: 0,
            body: std::ptr::null(), inlined: 0, exit_line: 0,
            outcome: None,
        }
    }

    /// The VM stays in place throughout this entry. Derive the owner address
    /// from the same full-VM pointer as semantic callouts, without creating a
    /// shared borrow that later mutable callouts would invalidate. Rebind on
    /// every entry, including reuse of the thread-local lane frame by another VM.
    fn bind_native_storage(&mut self, vm: *mut Vm) {
        let offset = Vm::PROPERTY_BUFFER_OFFSET;
        self.property_buffer = vm.cast::<u8>().wrapping_add(offset) as usize;
        self.frameless = frameless::general_frameless_admit as *const () as usize;
        // SAFETY: the VM outlives this entry; the eval flags hold through it (`function_matches_in_lane`).
        let vm_ref = unsafe { &*vm };
        self.bind_run_storage(vm_ref);
        self.element_get = general_elements::general_element_get as *const () as usize;
    }

    /// Object loads, stores and barriers share these bases in lane and general code.
    /// Allocation or collection between native entries may move any of them.
    /// The bases native code reads, as `vm` holds them now. They stand while this VM's record
    /// layout (`Heap::native_layout_epoch`: collections, record blocks, mark blocks), method
    /// cache atom words, typed view directory and slot tracking are as last read, and the
    /// lazily made empty string and unit strings exist: then one pass of compares replaces the
    /// rereads every allocating, call or step helper ends with.
    #[inline]
    pub(crate) fn refresh_object_bases(&mut self, vm: &Vm) {
        let layout = vm.heap_reads().native_layout_epoch();
        let atoms = vm.method_cache_atoms.words();
        let (directory, pages) = vm.heap_reads().typed_view_directory();
        if self.bases_vm == (vm as *const Vm).addr() && self.layout_seen == layout
            && self.method_cache_atoms == atoms.as_ptr() as usize && self.method_cache_atom_words == atoms.len()
            && self.typed_view_directory == directory.addr() && self.typed_view_pages == pages
            && self.store_watch == vm.heap_reads().store_watch_word()
            && self.empty_string != Value::DELETED.to_bits() && self.ascii_unit_strings != 0
            && self.empty_object_proto == vm.realm_object_prototype(vm.realm.globals).to_bits()
            && self.empty_array_proto == vm.array_prototype_for_realm(vm.realm.globals).to_bits()
            && self.collection_record_base == vm.heap_reads().collection_record_base().map_or(0, |base| base as usize)
        {
            return;
        }
        self.reread_object_bases(vm);
        (self.layout_seen, self.bases_vm) = (layout, (vm as *const Vm).addr());
    }

    #[cold]
    #[inline(never)]
    fn reread_object_bases(&mut self, vm: &Vm) {
        // The empty object's template is dropped with the bases: the next empty allocation's helper
        // makes an object again, which captures the template anew (`capture_empty_object`).
        self.empty_object_proto = vm.realm_object_prototype(vm.realm.globals).to_bits();
        (self.empty_object_layout, self.empty_object_payload) = (0, 0);
        self.stub_cursor = vm.heap_reads().native_stub_cursor();
        self.object_cursor = vm.heap_reads().native_object_cursor();
        // The empty array's template is dropped with the bases, as the object's is (`capture_empty_array`).
        self.empty_array_proto = vm.array_prototype_for_realm(vm.realm.globals).to_bits();
        (self.empty_array_layout, self.empty_array_payload) = (0, 0);
        self.array_cursor = vm.heap_reads().native_array_cursor();
        self.object_record_base = vm.heap_reads().object_record_base().map_or(0, |base| base as usize);
        self.class_base = vm.heap_reads().class_space_base();
        self.function_record_base = vm.heap_reads().function_record_base().map_or(0, |base| base as usize);
        self.array_record_base = vm.heap_reads().array_record_base().map_or(0, |base| base as usize);
        self.collection_record_base = vm.heap_reads().collection_record_base().map_or(0, |base| base as usize);
        self.string_record_base = vm.heap_reads().string_record_base().map_or(0, |base| base as usize);
        self.ascii_unit_strings = vm.heap_reads().ascii_unit_strings_base().map_or(0, |base| base as usize);
        let (directory, pages) = vm.heap_reads().typed_view_directory();
        (self.typed_view_directory, self.typed_view_pages) = (directory.addr(), pages);
        self.buffer_relocations = quench_runtime::tier_surface::buffer_relocations_address().addr();
        self.epoch = vm.heap_reads().collection_epoch();
        let (blocks, count) = vm.heap_reads().native_object_mark_blocks();
        (self.object_mark_blocks, self.object_mark_block_count) = (blocks as usize, count);
        let (blocks, count) = vm.heap_reads().native_function_mark_blocks();
        (self.function_mark_blocks, self.function_mark_block_count) = (blocks as usize, count);
        let (blocks, count) = vm.heap_reads().native_array_mark_blocks();
        (self.array_mark_blocks, self.array_mark_block_count) = (blocks as usize, count);
        self.empty_string = vm.empty_string_word().to_bits();
        self.store_watch = vm.heap_reads().store_watch_word();
        let atoms = vm.method_cache_atoms.words();
        (self.method_cache_atoms, self.method_cache_atom_words) = (atoms.as_ptr() as usize, atoms.len());
    }

    /// Records `object`, the empty object `Vm::object` just made for the current realm's prototype,
    /// as the template an inline empty allocation copies: its two record words and the prototype
    /// they were made for. The words depend on the prototype only (`Object::new`). Nothing is
    /// captured where allocation is profiled per record, which the inline path would skip.
    fn capture_empty_object(&mut self, vm: &Vm, object: Value) {
        if cfg!(any(feature = "profile-memory", feature = "profile-aggregate")) {
            return;
        }
        let Some(record) = vm.heap_reads().object_record(object) else { return };
        let (payload, layout) = record.record().stored_words();
        self.empty_object_proto = vm.realm_object_prototype(vm.realm.globals).to_bits();
        (self.empty_object_payload, self.empty_object_layout) = (payload, u64::from(layout));
    }

    /// Records `array`, the zero-length array `Vm::hole_array(0)` just made for the current realm's
    /// Array prototype, as the template an inline empty array allocation copies. Its elements are
    /// the static empty header, so the record's two words depend on the prototype only
    /// (`ArrayObject::fresh`), as an empty object's do. Not captured under per-record profiling.
    fn capture_empty_array(&mut self, vm: &Vm, array: Value) {
        if cfg!(any(feature = "profile-memory", feature = "profile-aggregate")) {
            return;
        }
        let Some(record) = vm.heap_reads().object_ref(array) else { return };
        let (payload, layout) = record.stored_words();
        self.empty_array_proto = vm.array_prototype_for_realm(vm.realm.globals).to_bits();
        (self.empty_array_payload, self.empty_array_layout) = (payload, u64::from(layout));
    }

    /// Readies a region's parked dispatch (`CompiledOsrRegion::parked`) for a run of `program`
    /// above VM frame `frame`: the words `JsStepDispatch::with_function` and
    /// `bind_native_storage` bind, the run state the last run left reset to `idle`'s, and the
    /// bases refreshed (`refresh_heap_bases`), which reread only what moved since the last run.
    /// The words the run's binds set (`osr_lane`) are theirs.
    fn rebind_region(&mut self, vm: &mut Vm, program_id: ProgramId, program: &ResidualProgram, frame: usize, registers: *mut Value) {
        self.bind_lane_storage(std::ptr::from_mut(vm));
        (self.program_id, self.frame) = (program_id, frame);
        // The window the capture view reads and writes is the run's own, which the parked dispatch
        // last ran in only where the activation is the same; set before the bases are refreshed.
        self.native_registers = registers;
        self.reset_region_run();
        self.refresh_heap_bases(vm, program);
    }

    /// The run state a region's run leaves, reset to `idle`'s.
    #[inline(always)]
    fn reset_region_run(&mut self) {
        self.set_captures_word(0);
        (self.inlined, self.exit_line, self.returned_pc) = (0, 0, 0);
        (self.lane_values_top, self.lane_values_end, self.lane_activations_top, self.lane_frames_end) = (0, 0, 0, 0);
        self.environment_storage = quench_runtime::tier_surface::absent_environment_storage();
        self.outcome = None;
    }

    /// Readies a region's parked dispatch for another run of the activation it was last fully
    /// bound for (`EntryStamp`): its program, frame, register, local and body words, `this` slot
    /// and frame capture view stand (the run binds its captures itself, `osr_lane::bind_captures`).
    /// What the VM may have changed since is bound again: its run storage, the run state, the
    /// bases (a compare-only pass while nothing moved), the program's
    /// field entries (a moved cache table moves them) and the store specialization.
    #[inline]
    fn rewarm_region(&mut self, vm: &mut Vm, program_specialized: bool) {
        self.bind_run_storage(vm);
        self.reset_region_run();
        self.refresh_object_bases(vm);
        vm.frameless.bind(self);
        self.field_entries = vm.program_field_entries(self.program_id).unwrap_or(0);
        self.stores_specialized = usize::from(vm.specialized && program_specialized);
    }

    /// Refresh the additional frame and array views read by native code.
    fn refresh_heap_bases(&mut self, vm: &Vm, program: &ResidualProgram) {
        self.refresh_object_bases(vm);
        self.fixed_this_slot = vm.frames.get(self.frame)
            .filter(|frame| frame.fixed_this)
            .map_or(std::ptr::null(), |frame| &frame.this as *const Value);
        self.refresh_capture_view(vm);
        vm.frameless.bind(self);
        self.field_entries = vm.program_field_entries(self.program_id).unwrap_or(0);
        self.stores_specialized = usize::from(vm.specialized && program.specialized);
    }

    /// The window's captures word (`CONTEXT_BODY`): the frame's dense capture slots, which generated reads
    /// index (their address, count, and whether they admit every direct store), held in the window's header.
    fn captures_word(&self) -> u64 {
        // SAFETY: `native_registers` is the bound window, whose header is its own.
        unsafe { frame_word(self.native_registers, CONTEXT_BODY) }
    }

    fn set_captures_word(&self, word: u64) {
        // SAFETY: as `captures_word`; the dispatch owns the header of the window it runs in.
        unsafe { set_frame_word(self.native_registers, CONTEXT_BODY, word) }
    }

    /// The frame's dense capture slots, which generated reads index (the window's captures word): none
    /// while its slot memo is missing or stale, or its storage is not dense, and a read then
    /// leaves at its record.
    fn refresh_capture_view(&mut self, vm: &Vm) {
        let held = self.captures_word();
        let (values, len) = vm.frames.get(self.frame)
            .and_then(|_| vm.current_capture_slots(self.frame))
            .and_then(|(slots, root)| {
                if root.is_some() { return None; }
                // SAFETY: the frame's current memo owns these slots through this
                // collection epoch. Only dense, non-projected storage is admitted.
                unsafe { &*slots }.native_dense_view()
            })
            .unwrap_or((std::ptr::null(), 0));
        // The stores bit holds only while the slots are the ones it was admitted for.
        let stores = quench_bjit::shared_words::capture_stores(held) != 0 && values.addr() as u64 == quench_bjit::shared_words::capture_values(held);
        self.set_captures_word(entry_cell::capture_word(values, len, stores));
    }

    /// Binds the frame's capture slots once per activation. Only a lane entry makes the slot
    /// memo, so an AOT activation walks to its environment here (`Vm::cached_direct_capture`
    /// keeps the walk) and views the slots; later refreshes keep the view while the memo is
    /// current, and a collection that retires it leaves reads to the per-PC fallback.
    #[cfg(feature = "native-execution")]
    fn bind_captures(&mut self, vm: &mut Vm) {
        vm.cached_direct_capture(self.frame, 0, 0);
        self.refresh_capture_view(vm);
    }
}

#[cfg(feature = "native-execution")]
fn js_native_frame_storage_unchanged(frame: &Frame, dispatch: &JsStepDispatch) -> bool {
    frame.registers.len() == dispatch.register_count
        && frame.registers.as_ptr() == dispatch.native_registers
        && frame.locals.len() == dispatch.local_count
        && frame.locals.as_ptr() == dispatch.native_locals
}

#[cfg(feature = "native-execution")]
enum JsCalloutStep {
    Continue(usize),
    Stop(JsNativeStep),
}

/// Dynamic dispatch selects its opcode from the instruction, not a specialization.
#[cfg(feature = "native-execution")]
const DYNAMIC_JS_OPCODE: usize = meaning_compiled::OPCODE_COUNT;

#[cfg(feature = "native-execution")]
unsafe extern "C" fn js_step_callout(
    context: *mut CallContext,
    function: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    // SAFETY: the native entry owns the context and result storage.
    unsafe { js_semantic_callout::<false, DYNAMIC_JS_OPCODE>(context, function, arguments, result) }
}

use quench_runtime::vm::engine_types::StaticJsOperation;

pub use quench_runtime::vm::engine_types::NativeJsOperationEntry;

/// Resolve a compiler opcode identity to its derived semantic helper. The opcode
/// inventory generates this mapping; linked entries never dispatch an opcode.
#[cfg(feature = "native-execution")]
pub fn native_js_operation_entry_for(opcode: usize) -> Option<NativeJsOperationEntry> {
    Op::from_index(opcode).map(Op::native_semantic_entry)
}

pub(crate) fn native_js_operation_signature(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Signature {
    let pointer = isa.pointer_type();
    call_signature::typed_signature(isa.default_call_conv(), &[pointer, types::I64, pointer, pointer], &[types::I64])
}

pub(crate) use quench_bjit::call_record::NATIVE_JS_STOPPED;
pub(crate) const NATIVE_JS_INVALID_PC: u32 = 2;
#[cfg(feature = "native-execution")]
const JS_NO_STOP_PC: usize = usize::MAX;

#[cfg(feature = "profile-native-ops")]
fn native_operation_fallback_kind(vm: &Vm, frame: usize, instruction: WideInstruction) -> &'static str {
    if meaning_compiled::is_jump_binary_false(instruction) {
        let Some((strict, _))= quench_runtime::vm::operations::equality_operator(instruction.binary_operator_field()) else {
            return "branch-relation";
        };
        let registers = &vm.frames[frame].registers;
        let constants = vm.programs.constants(vm.frames[frame].program);
        let value = |operand: Operand| match operand.kind() {
            Some(OperandKind::Register) => registers.get(usize::from(operand.payload())).copied(),
            Some(OperandKind::Constant) => constants.get(usize::from(operand.payload())).copied(),
            _ => None,
        };
        let (Some(left), Some(right)) = (value(instruction.operand_b()), value(instruction.operand_c())) else {
            return "branch-computed-operand";
        };
        return if left.is_null() || left.is_undefined() || right.is_null() || right.is_undefined() {
            if strict { "branch-strict-nullish" } else { "branch-loose-nullish" }
        } else if left.as_bool().is_some() || right.as_bool().is_some() {
            if strict { "branch-strict-boolean" } else { "branch-loose-boolean" }
        } else if left.direct_object_index().is_some() || right.direct_object_index().is_some() {
            "branch-direct-object"
        } else {
            "branch-other-equality"
        };
    }
    // merge: `is_capture_read` does not exist on the staged Operation; match the decoded LoadCapture instead.
    if matches!(quench_runtime::staged::Operation::<u64, ()>::of(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) {
        if instruction.capture_depth() != 0 {
            return "capture-outer-depth";
        }
        let Some((slots, root)) = vm.current_capture_slots(frame) else {
            return "capture-cache-miss";
        };
        // SAFETY: current_capture_slots checked the frame and collection epoch.
        let slots = unsafe { &*slots };
        return match slots.state(usize::from(instruction.capture_slot())) {
            Some(quench_runtime::heap::EnvironmentSlotState::Owned(value)) if !value.is_deleted() => {
                if vm.owned_capture(slots, root, instruction.capture_slot()).is_none() {
                    "capture-root-projection"
                } else if root.is_some() {
                    "capture-root-owned"
                } else {
                    "capture-function-owned"
                }
            }
            Some(quench_runtime::heap::EnvironmentSlotState::Owned(_)) => "capture-tdz",
            Some(quench_runtime::heap::EnvironmentSlotState::Shared(_)) => "capture-shared",
            None => "capture-slot-miss",
        };
    }
    if !meaning_compiled::is_get_field(instruction) {
        return "other";
    }
    if vm.frames[frame].function == 0 {
        return "root";
    }
    if instruction.writes_current_this() || instruction.numeric_local_target().is_some()
        || !(instruction.result_flags() == 0
            || (instruction.returns_from_frame() && instruction.result_flags() == RETURN_REGISTER))
    {
        return "result-flags";
    }
    let Some(FieldLookup::Atom { base, cache_site, .. }) = instruction.field_lookup() else {
        return "nested";
    };
    let receiver = if base == FieldBase::THIS {
        if !vm.frames[frame].fixed_this { return "uninitialized-this"; }
        vm.frames[frame].this
    } else if let Some(register) = base.register_index() {
        let Some(value) = vm.frames[frame].registers.get(usize::from(register)) else { return "invalid-register"; };
        *value
    } else {
        return "unsupported-base";
    };
    if receiver.direct_object_index().is_none() {
        return "non-direct-object";
    }
    let Some(object) = vm.heap_reads().object_ref(receiver) else { return "missing-object"; };
    if object.record().inline_properties().is_some() {
        "inline-storage"
    } else if object.record().plain_storage_start().is_none() {
        "extras-storage"
    } else {
        if vm.active_program != vm.frames[frame].program {
            return "inactive-program";
        }
        let cache = vm.field_cache_entry(cache_site);
        if cache.holder != NO_FIELD_HOLDER {
            if cache.receiver != object.exact_shape() {
                "prototype-receiver-shape-miss"
            } else if let Some(holder) = object.proto_link().object_granule().and_then(|granule| vm.heap_reads().object_record(quench_runtime::Value::direct_object(granule))) {
                if holder.exact_shape() != cache.holder {
                    "prototype-holder-shape-miss"
                } else if holder.record().inline_properties().is_some() {
                    "prototype-holder-inline"
                } else if holder.record().extras_storage_start().is_some() {
                    "prototype-holder-extras"
                } else if holder.plain_storage_start().is_some() {
                    "prototype-holder-plain"
                } else {
                    "prototype-holder-other-storage"
                }
            } else {
                "prototype-non-record"
            }
        } else if cache.receiver != object.exact_shape() {
            "own-shape-miss"
        } else {
            "own-shape-match"
        }
    }
}

pub(crate) fn native_js_start_pc(builder: &mut FunctionBuilder<'_>, context: ir::Value) -> ir::Value {
    use quench_runtime::staged::machine::Lane;
    let pointer = emit_machine::of_in_lane(builder.func.signature.params[0].value_type);
    let mut stage = object_slots::emit(builder);
    let payload = stage.raw_load(pointer, MemFlagsData::trusted(), context,
        std::mem::offset_of!(CallContext, head.adapter_payload) as i32);
    stage.raw_load(Lane::I64, MemFlagsData::trusted(), payload,
        std::mem::offset_of!(JsStepDispatch, start_pc) as i32)
}

/// Execute one compile-time-selected semantic operation and return its exact
/// continuation. This entry never batches subsequent bytecodes.
///
/// # Safety
/// `context` must be a live shared native-entry context for `H`, with its
/// frame, program and adapter payload rooted through this call. `result` must
/// point to a writable result word. `arguments` must point to an aligned,
/// live `NativeJsOperands` from this compiler build, matching the instruction
/// at the supplied PC. Operand structure is resolved before native execution.
#[cfg(feature = "native-execution")]
pub unsafe extern "C" fn native_js_operation_entry<const OPCODE: usize>(
    context: *mut std::ffi::c_void,
    pc: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    #[cfg(feature = "profile-native-ops")]
    quench_runtime::vm::native_op_profile::record(OPCODE);
    // SAFETY: forwarded from this entry's native ABI contract. Invalid opcode
    // constants are reported inside the shared exception boundary.
    unsafe { js_semantic_callout::<true, OPCODE>(context.cast(), pc, arguments, result) }
}

/// Whether the callout's step pushed a frame: it runs in the program its push made active, so
/// the program that was running when the code was entered is not made active again over it.
#[cfg(feature = "native-execution")]
fn pushed_a_frame<E>(called: &Result<Result<JsCalloutStep, JsError>, E>) -> bool {
    matches!(called, Ok(Ok(JsCalloutStep::Stop(JsNativeStep {
        result: Ok(StepResult::PushFrame { .. } | StepResult::ResumeFrames(_)), ..
    }))))
}

#[cfg(feature = "native-execution")]
unsafe fn js_semantic_callout<const SINGLE_OPERATION: bool, const OPCODE: usize>(
    context: *mut CallContext,
    function: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    // The opcode resolves its operation statically; the boundary around the step is the same for
    // every opcode, so one body per host runs it (`js_semantic_callout_with`): one copy per opcode was
    // 290 instances of ~6 KB that every edit to a helper it inlines compiled again.
    let operation = const {
        if SINGLE_OPERATION {
            match Op::from_index(OPCODE) {
                Some(opcode) => Some(StaticJsOperation {
                    opcode,
                    execute: Vm::resolve_step_operation(opcode),
                }),
                None => None,
            }
        } else {
            None
        }
    };
    // SAFETY: forwarded from this callout's contract.
    unsafe { js_semantic_callout_with::<SINGLE_OPERATION>(operation, context, function, arguments, result) }
}

/// `js_semantic_callout`'s body, with the operation its opcode resolved (`None` for the dynamic entry).
#[cfg(feature = "native-execution")]
#[inline(never)]
pub unsafe fn js_semantic_callout_with<const SINGLE_OPERATION: bool>(
    operation: Option<StaticJsOperation>,
    context: *mut CallContext,
    function: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    // The shared trampoline marks its active context while native code runs.
    // Clear it while Rust semantics and any host or guest helper run, so a
    // fault there is never mistaken for a generated-code fault.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    // SAFETY: the compiled entry's owner keeps this context and adapter payload
    // alive through the callout.
    let context_ref = unsafe { &mut *context };
    // The program whose code called (a function of another program than the one running makes
    // its constants the active ones for the step), restored after it.
    let entered_from = std::cell::Cell::new(None);
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let pc = usize::try_from(function)
            .map_err(|_| JsError::validation("native JS PC does not fit usize".into()))?;
        let dispatch = unsafe { &mut *context_ref.adapter_payload.cast::<JsStepDispatch>() };
        let vm = unsafe { &mut *context_ref.vm.cast::<Vm>() };
        if vm.active_program != dispatch.program_id {
            entered_from.set(Some(vm.active_program));
            vm.set_active_program(dispatch.program_id);
        }
        // SAFETY: the owning activation (`JsStepDispatch::with`) admits both pointers once and retains the shared
        // program until native execution (including reentry) finishes; release needs it unique.
        let program = unsafe { &*(dispatch.residual as *const ResidualProgram) };
        let body = unsafe { &*dispatch.body };
        #[cfg(feature = "native-execution")]
        if SINGLE_OPERATION && dispatch.stop_pc == pc {
            let frame = &mut vm.frames[dispatch.frame];
            frame.pc = pc;
            frame.pc_is_resume = true;
            return Ok(JsCalloutStep::Stop(JsNativeStep {
                instruction_pc: pc, next_pc: pc, result: Ok(StepResult::Continue),
            }));
        }
        let packed = *body.code.get(pc).ok_or_else(|| JsError::validation("native JS PC is outside function".into()))?;
        let instruction = if SINGLE_OPERATION {
            let selected = operation.as_ref().ok_or_else(||
                JsError::validation("invalid native semantic opcode".into()))?;
            // Linked operands are already resolved. Admission needs only the
            // source opcode, not a second decoding of every operand field.
            let source_opcode = body.op_of(packed);
            if selected.opcode != source_opcode {
                return Err(JsError::validation("native semantic opcode does not match instruction".into()));
            }
            if arguments.is_null() {
                return Err(JsError::validation("native semantic operands are missing".into()));
            }
            #[cfg(feature = "profile-native-ops")]
            {
                let source_instruction = body.widened(packed);
                quench_runtime::vm::native_op_profile::record_site(
                    dispatch.program_id.raw(), vm.frames[dispatch.frame].function, pc, source_instruction,
                    body.name.or(body.inferred_name)
                        .and_then(|atom| program.atoms.get(atom as usize)),
                    || native_operation_fallback_kind(vm, dispatch.frame, source_instruction),
                );
            }
            // SAFETY: linked code supplies resolved operands under the entry's
            // ABI contract; the source opcode check precedes all guest effects.
            unsafe { &*arguments.cast::<quench_runtime::vm::native_operands::NativeJsOperands>() }
                .instruction(selected.opcode)
        } else {
            body.widened(packed)
        };
        let next_pc = pc.checked_add(1).ok_or_else(|| JsError::validation("native JS PC overflow".into()))?;

        let frame = &mut vm.frames[dispatch.frame];
        // Entry admits these homes, and every effectful continuation validates
        // them before native code resumes. No frame-storage mutation occurs
        // between that edge and this callout; polling here duplicates its proof.
        frame.pc = pc;
        frame.pc_is_resume = false;
        if let Err(error) = vm.check_execution_interrupt() {
            return Ok(JsCalloutStep::Stop(JsNativeStep {
                instruction_pc: pc,
                next_pc,
                result: Err(error),
            }));
        }

        if !SINGLE_OPERATION && quench_runtime::staged::Operation::<u64, ()>::is_field_read(instruction)
            && (instruction.result_flags() == 0
                || instruction.result_flags() == quench_runtime::bytecode::RETURN_REGISTER)
            && !instruction.writes_current_this()
            && (instruction.returns_from_frame() || next_pc < body.code.len())
            && let Some(FieldLookup::Atom {
                base,
                cache_site,
                ..
            }) = instruction.field_lookup()
            && let Some(receiver_register) = base.register_index()
            && let Some(receiver) = vm.frames[dispatch.frame]
                .registers
                .get(usize::from(receiver_register))
                .copied()
        {
            let probe = vm.cached_field_hit(program, receiver, cache_site);
            if admission::diagnosing(vm) {
                vm.programs.record_js_jit_field_cache_probe(
                    dispatch.program_id,
                    vm.frames[dispatch.frame].function,
                    cache_site,
                    matches!(&probe, quench_runtime::vm::object::FieldCacheProbe::Hit(_)),
                );
            }
            if let quench_runtime::vm::object::FieldCacheProbe::Hit(value) = probe {
                let frame = &mut vm.frames[dispatch.frame];
                if instruction.returns_from_frame() {
                    frame.pc = pc;
                    frame.pc_is_resume = false;
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: pc,
                        next_pc,
                        result: Ok(quench_runtime::vm::StepResult::Return(value)),
                    }));
                }
                let destination = frame
                    .registers
                    .get_mut(usize::from(instruction.result_register()))
                    .ok_or_else(|| {
                        JsError::validation("native JS field result register is invalid".into())
                    })?;
                *destination = value;
                frame.pc = next_pc;
                frame.pc_is_resume = true;
                return Ok(JsCalloutStep::Continue(next_pc));
            }
        }

        // merge: HEAD's js field store is a named store through a register receiver (`base: Some`).
        if !SINGLE_OPERATION && matches!(quench_runtime::staged::Operation::<u64, ()>::of(instruction),
            Some(quench_runtime::staged::Operation::SetField { base: Some(_), .. }))
            && instruction.result_flags() == 0
            && !instruction.returns_from_frame()
            && next_pc < body.code.len()
            && let (Some(receiver), Some(value)) = (
                vm.frames[dispatch.frame]
                    .registers
                    .get(usize::from(instruction.register_b()))
                    .copied(),
                vm.frames[dispatch.frame]
                    .registers
                    .get(usize::from(instruction.register_a()))
                    .copied(),
            )
        {
            let hit = vm.cached_field_store_hit(
                program,
                receiver,
                instruction.atom_index(),
                value,
                instruction.cache_site_index(),
            );
            if admission::diagnosing(vm) {
                vm.programs.record_js_jit_field_store_cache_probe(
                    dispatch.program_id,
                    vm.frames[dispatch.frame].function,
                    hit,
                );
            }
            if hit {
                let frame = &mut vm.frames[dispatch.frame];
                frame.pc = next_pc;
                frame.pc_is_resume = true;
                return Ok(JsCalloutStep::Continue(next_pc));
            }
        }

        let frame_depth = vm.frames.len();
        let frame_function = vm.frames[dispatch.frame].function;
        let mut current_pc = pc;
        let mut current_instruction = instruction;
        let mut next = next_pc;
        loop {
            let layout = current_instruction.op().control_flow_layout();
            let is_call = meaning_compiled::is_ordinary_call(current_instruction);
            let diagnostics_enabled = is_call && admission::diagnosing(vm);
            let direct_call_target = if diagnostics_enabled && is_call {
                vm.frames[dispatch.frame]
                    .registers
                    .get(usize::from(current_instruction.register_b()))
                    .copied()
                    .and_then(|callee| vm.function_record(callee))
                    .and_then(|record| match &record.kind {
                        quench_runtime::tier_surface::FunctionKind::User(program, function) => {
                            Some((program.raw(), *function))
                        }
                        _ => None,
                    })
            } else {
                None
            };
            let capture = if matches!(quench_runtime::staged::Operation::<u64, ()>::of(current_instruction),
                Some(quench_runtime::staged::Operation::LoadCapture { .. })) {
                vm.cached_direct_capture(
                    dispatch.frame,
                    current_instruction.capture_depth(),
                    current_instruction.capture_slot(),
                )
            } else {
                None
            };
            // Static AOT helpers and JIT dispatch offer the same guarded frame
            // transition before falling back to the call's semantic operation.
            let in_loop_call = if dispatch.allow_inline_calls && is_call {
                vm.try_inline_user_call(program, dispatch.frame, current_instruction, next)
            } else {
                None
            };
            // A pushed call is the shared frame transition however it then completes.
            let pushed = diagnostics_enabled && matches!(in_loop_call, Some(Ok(quench_runtime::vm::StepResult::PushFrame { .. })));
            let in_loop_call = in_loop_call.map(|step| vm.complete_pushed_call(program, dispatch.frame, current_instruction, step));
            let step = if let Some(value) = capture {
                let destination = vm.frames[dispatch.frame]
                    .registers
                    .get_mut(usize::from(current_instruction.result_register()))
                    .ok_or_else(|| JsError::validation(
                        "native JS capture result register is invalid".into(),
                    ))?;
                *destination = value;
                Ok(quench_runtime::vm::StepResult::Continue)
            } else if let Some(step) = in_loop_call {
                step
            } else if SINGLE_OPERATION {
                (operation.as_ref().expect("validated semantic operation").execute)(
                    vm, program, dispatch.frame, current_instruction, &mut next,
                    dispatch.allow_inline_calls,
                )
            } else {
                (Vm::resolve_step_operation(current_instruction.op()))(
                    vm,
                    program,
                    dispatch.frame,
                    current_instruction,
                    &mut next,
                    dispatch.allow_inline_calls,
                )
            };
            // A construction the operation pushed completes as a pushed call does.
            let step = if meaning_compiled::is_construct(current_instruction) && dispatch.allow_inline_calls {
                vm.complete_pushed_call(program, dispatch.frame, current_instruction, step)
            } else { step };
            if is_call && diagnostics_enabled {
                let outcome = match &step {
                    _ if pushed => quench_runtime::vm::program_store::JsJitCallOutcome::PushFrame,
                    Ok(quench_runtime::vm::StepResult::Continue) => {
                        quench_runtime::vm::program_store::JsJitCallOutcome::Continue
                    }
                    Ok(quench_runtime::vm::StepResult::TailCall) => {
                        quench_runtime::vm::program_store::JsJitCallOutcome::TailCall
                    }
                    Ok(quench_runtime::vm::StepResult::PushFrame { .. } | quench_runtime::vm::StepResult::ResumeFrames(_)) => {
                        quench_runtime::vm::program_store::JsJitCallOutcome::PushFrame
                    }
                    Ok(quench_runtime::vm::StepResult::Return(_)) => {
                        quench_runtime::vm::program_store::JsJitCallOutcome::Return
                    }
                    Ok(quench_runtime::vm::StepResult::Suspend(quench_runtime::vm::Suspended::Await { .. })) => {
                        quench_runtime::vm::program_store::JsJitCallOutcome::Await
                    }
                    Ok(quench_runtime::vm::StepResult::Suspend(quench_runtime::vm::Suspended::Yield { .. })) => {
                        quench_runtime::vm::program_store::JsJitCallOutcome::Yield
                    }
                    Err(_) => quench_runtime::vm::program_store::JsJitCallOutcome::Error,
                };
                vm.programs.record_js_jit_call_site(
                    dispatch.program_id,
                    frame_function,
                    current_pc,
                    current_instruction.op(),
                    direct_call_target,
                    outcome,
                );
            }
            match step {
                Ok(quench_runtime::vm::StepResult::Continue) => {
                    if next >= body.code.len() {
                        vm.frames[dispatch.frame].pc = current_pc;
                        vm.frames[dispatch.frame].pc_is_resume = false;
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Err(JsError::validation(
                                "native JS continuation is outside its function".into(),
                            )),
                        }));
                    }
                    let frame = &mut vm.frames[dispatch.frame];
                    frame.pc = next;
                    frame.pc_is_resume = true;
                    if !js_native_frame_storage_unchanged(frame, dispatch) {
                        // The operation ran once. Return the transition to the
                        // frame owner at its already-computed successor.
                        frame.js_jit_skip_entry_once = next == 0;
                        vm.active_registers = frame.registers.as_mut_ptr();
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Ok(quench_runtime::vm::StepResult::Continue),
                        }));
                    }
                    if vm.frames.len() != frame_depth
                        || vm.frames[dispatch.frame].function != frame_function
                    {
                        // Do not carry a native continuation across a call or
                        // return that changed the active VM frame.
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Ok(quench_runtime::vm::StepResult::Continue),
                        }));
                    }

                    let fallthrough = layout == ControlFlowLayout::Fallthrough
                        && next == current_pc.saturating_add(1);
                    if SINGLE_OPERATION || !fallthrough {
                        // Only generated code consumes these raw views. A stopped
                        // dispatch discards them, and a batched VM operation will
                        // refresh once at the boundary before native execution.
                        dispatch.refresh_heap_bases(vm, program);
                        return Ok(JsCalloutStep::Continue(next));
                    }

                    current_pc = next;
                    let Some(packed) = body.code.get(current_pc).copied() else {
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: current_pc,
                            result: Err(JsError::validation(
                                "native JS continuation is outside function".into(),
                            )),
                        }));
                    };
                    current_instruction =
                        body.widened(packed);
                    next = current_pc
                        .checked_add(1)
                        .ok_or_else(|| JsError::validation("native JS PC overflow".into()))?;
                    let frame = &mut vm.frames[dispatch.frame];
                    frame.pc = current_pc;
                    frame.pc_is_resume = false;
                    if let Err(error) = vm.check_execution_interrupt() {
                        return Ok(JsCalloutStep::Stop(JsNativeStep {
                            instruction_pc: current_pc,
                            next_pc: next,
                            result: Err(error),
                        }));
                    }
                }
                result => {
                    return Ok(JsCalloutStep::Stop(JsNativeStep {
                        instruction_pc: current_pc,
                        next_pc: next,
                        result,
                    }));
                }
            }
        }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    if let Some(previous) = entered_from.get().filter(|_| !pushed_a_frame(&called)) {
        // SAFETY: the context's VM outlives the callout.
        unsafe { &mut *context_ref.vm.cast::<Vm>() }.set_active_program(previous);
    }

    match called {
        Ok(Ok(JsCalloutStep::Continue(next))) => {
            // SAFETY: generated code supplies a valid result word.
            unsafe { result.write(next as u64) };
            0
        }
        Ok(Ok(JsCalloutStep::Stop(step))) => {
            // SAFETY: the adapter payload is unique to this native entry.
            unsafe { stopped::<SINGLE_OPERATION>(context_ref, step, result) }
        }
        Ok(Err(error)) => {
            let pc = usize::try_from(function).unwrap_or(0);
            let step = JsNativeStep { instruction_pc: pc, next_pc: pc.saturating_add(1), result: Err(error) };
            // SAFETY: as above.
            unsafe { stopped::<SINGLE_OPERATION>(context_ref, step, result) }
        }
        Err(panic) => {
            context_ref.raised = Some(Raised::Panic(panic));
            RAISED_STATUS
        }
    }
}

/// A callout's stop: a semantic entry's (`SINGLE_OPERATION`, run by an image body's generic row) in
/// the body's own protocol (`semantic_stop::settle`); the dynamic step's as its step in the payload.
///
/// # Safety
/// The context's VM and adapter payload are live and unborrowed; `result` is writable.
#[cfg(feature = "native-execution")]
unsafe fn stopped<const SINGLE_OPERATION: bool>(context: &mut CallContext, step: JsNativeStep, result: *mut u64) -> i64 {
    // SAFETY: as the caller promises.
    let dispatch = unsafe { &mut *context.adapter_payload.cast::<JsStepDispatch>() };
    let (status, word) = if SINGLE_OPERATION {
        semantic_stop::settle(unsafe { &mut *context.vm.cast::<Vm>() }, dispatch, step)
    } else {
        dispatch.outcome = Some(step);
        (i64::from(NATIVE_JS_STOPPED), 0)
    };
    unsafe { result.write(word) };
    status
}

#[quench_macros::ext(CompiledJsFunctionOps1)]
impl CompiledJsFunction {
    pub(crate) fn from_artifact(artifact: crate::js_compiled::JsJitArtifact) -> Option<Self> {
        let code = CodeMemory::new(&artifact.code)?;
        Some(Self {
            mapped_bytes: code.length(),
            // SAFETY: a numeric region emits the lane body ABI (`body_signature_in_lane`).
            entry: unsafe { std::mem::transmute::<*mut u8, BodyEntry>(code.pointer()) },
            _code: code,
            result: artifact.result,
            params: artifact.params,
            native_entries: std::cell::Cell::new(0),
        })
    }

    pub(crate) fn params(&self) -> usize {
        self.params
    }

    /// The numeric body's result on `arguments`; `None` for too few arguments, an argument
    /// that is no Number, or an interrupted run.
    pub(crate) fn call(
        &self,
        arguments: &[Value],
        interrupt: &quench_runtime::ExecutionInterrupt,
        poll_page: Option<(usize, usize)>,
    ) -> Option<Value> {
        let arguments = arguments.get(..self.params)?;
        let BodyReturn { status, value } = run_numeric_entry(self.entry, arguments.as_ptr().cast(), interrupt, poll_page);
        #[cfg(test)]
        self.native_entries.set(self.native_entries.get().saturating_add(1));
        (status == 0).then_some(())?;
        Some(match self.result {
            NativeNumberResult::Int32 => Value::integer(value as u32 as i32),
            NativeNumberResult::Float64 => Value::number(f64::from_bits(value)),
        })
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(crate) fn native_entries(&self) -> u64 {
        self.native_entries.get()
    }
}

/// A function's derived compilation facts.
pub(crate) struct Plan {
    params: Vec<quench_runtime::WasmType>,
    result: Option<quench_runtime::WasmType>,
    /// Per instruction, `None` when unreachable, else the denotation's operand
    /// fact: `c` for vector memory accesses (effective address), `b` otherwise
    /// (a moved value's kind, a global's binding, an import reference). The
    /// full register states exist only while planning.
    operands: Vec<Option<Slot>>,
    /// Per instruction, the second binding slot of a table copy or segment init
    /// (`TableOp::bindings`), `None` where the step names none.
    seconds: Vec<Option<Slot>>,
    /// Per instruction, the registers its state holds as references: the ones a call there holds
    /// in the native roots (`wasm_roots`). Only a register that is a reference on every path is
    /// listed, so a number's bits never are.
    references: Vec<Vec<u16>>,
    /// Per instruction, the operand registers of an allocating GC step (`allocating_reads`) with
    /// their kinds, which the allocation callout converts; empty elsewhere.
    gc_reads: Vec<Vec<(u16, Kind)>>,
    /// The frame's registers: a local is the register past them (`local_register`).
    registers: u16,
    /// Per instruction, the catch a raise there enters (`wasm_catch::catch_frame`).
    catches: Vec<Option<wasm_catch::Catch>>,
}

impl Plan {
    /// The plan as its callers see it: only the function being compiled
    /// holds its per-instruction facts.
    fn interface(&self) -> Self {
        Self {
            params: self.params.clone(),
            result: self.result,
            operands: Vec::new(),
            seconds: Vec::new(),
            references: Vec::new(),
            gc_reads: Vec::new(),
            registers: self.registers,
            catches: Vec::new(),
        }
    }

    fn reachable(&self, pc: usize) -> bool {
        self.operands[pc].is_some()
    }

    /// The selected operand fact of reachable instruction `pc`.
    fn operand(&self, pc: usize) -> Slot {
        self.operands[pc].expect("reachable")
    }
}

/// The plan's register-state analysis: one block per instruction, whose input is the state before
/// it. A raise at a pc with a catch also flows into the catch's target, in the entry state
/// `wasm_catch::raise_entry` gives. A rejection records its reason and fails the plan.
struct PlanFlow<'a, 'b> {
    analysis: &'a Analysis<'b>,
    index: usize,
    function: &'a Function,
    instructions: &'a [WideInstruction],
    callable: &'a quench_runtime::bitset::BitSet,
    memory_register: Option<u16>,
    memory_slot: &'a mut Option<u32>,
    result: Option<quench_runtime::WasmType>,
    entry: AnalysisState,
    catches: &'a [Option<wasm_catch::Catch>],
    bound: u16,
    /// Per pc, the entry state of its catch, set by the pc's transfer.
    raised: Vec<Option<AnalysisState>>,
    /// The edges of pc `p` are `edge_targets[edge_starts[p]..edge_starts[p + 1]]`: its ordinary
    /// successors in range, then its catch target.
    edge_starts: Vec<usize>,
    edge_targets: Vec<usize>,
    /// Whether a rejection or an unsupported raise stopped the analysis.
    failed: bool,
}

impl PlanFlow<'_, '_> {
    /// Records the rejection `reason` of this function and fails the plan.
    fn fail(&mut self, reason: impl FnOnce() -> String) {
        self.analysis.reject(self.index, reason);
        self.failed = true;
    }
}

impl quench_runtime::dataflow::Dataflow for PlanFlow<'_, '_> {
    type State = AnalysisState;

    fn blocks(&self) -> usize {
        self.instructions.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.edge_targets[self.edge_starts[block]..self.edge_starts[block + 1]]
    }

    fn entry_state(&self, block: usize) -> Option<AnalysisState> {
        (block == 0).then(|| self.entry.clone())
    }

    fn join(&self, into: &mut AnalysisState, incoming: &AnalysisState) -> bool {
        into.join_from(incoming)
    }

    fn transfer(&mut self, block: usize, state: &AnalysisState) -> AnalysisState {
        let mut out = state.clone();
        if self.failed {
            return out;
        }
        let instruction = self.instructions[block];
        if let Some(callee) = Step::of(instruction.op(), instruction.imm())
            .and_then(|step| step.callee(instruction))
            && !self.callable.contains(callee)
            && !self.analysis.experimental_root_compilation
        {
            self.fail(|| format!("uncompiled-callee:{callee}@{block}"));
            return out;
        }
        if let Some(catch) = self.catches[block] {
            match wasm_catch::raise_entry(catch, state, self.bound) {
                Some(entered) => self.raised[block] = Some(entered),
                None => {
                    self.failed = true;
                    return out;
                }
            }
        }
        let op = instruction.op();
        if self
            .analysis
            .transfer(
                self.function,
                instruction,
                &mut out,
                self.callable,
                (self.memory_register, &mut *self.memory_slot),
                self.result,
            )
            .is_none()
        {
            self.fail(|| format!("unsupported-instruction:{op:?}@{block}"));
        }
        out
    }

    fn transfer_edge_at(&self, block: usize, index: usize, out: &AnalysisState) -> AnalysisState {
        match &self.raised[block] {
            Some(entered) if self.edge_starts[block] + index + 1 == self.edge_starts[block + 1] => {
                entered.clone()
            }
            _ => out.clone(),
        }
    }
}

/// The analysis context shared by every function of a program.
struct Analysis<'a> {
    vm: &'a Vm,
    program: ProgramId,
    code: &'a ResidualProgram,
    env: Value,
    /// Whether imported calls may compile: a callee may grow memory, which
    /// moves an unguarded memory's bytes.
    callouts: bool,
    /// `unreachable` is enabled only for a bounded, explicitly selected root
    /// experiment until its compile-time memory cost is measured.
    experimental_root_compilation: bool,
    /// A root-only function may call out only while callouts are safe for its
    /// current memory view. A VM callee can grow unguarded memory.
    root_only: bool,
    /// Rejection reasons exist only for explicit diagnostic runs.
    compile_rejections: Option<&'a std::cell::RefCell<Vec<Option<String>>>>,
    /// Whether bodies unroll their innermost loops (`program_unrolls`), decided once the
    /// plans have read every record and before any body is built.
    unrolls: bool,
    /// The defined functions that may grow the memory (`wasm_growth`); empty until the module's
    /// memory is known, and a function past its bound is taken to grow it.
    grows_memory: quench_runtime::bitset::BitSet,
    /// The module's binding types by environment slot where the plan has no instance (an image's), which
    /// the plan then reads in place of `env`'s cells (`binding`).
    declared: Option<&'a [quench_runtime::WasmBindingType]>,
}

impl<'a> Analysis<'a> {
    /// The plan of program `program` (`code`) over the instance environment `env`: callouts compile, no
    /// root experiment, no diagnostics, no unrolling, growth unknown, bindings read from `env`.
    fn new(vm: &'a Vm, program: ProgramId, code: &'a ResidualProgram, env: Value) -> Self {
        Self {
            vm, program, code, env, callouts: true, experimental_root_compilation: false, root_only: false,
            compile_rejections: None, unrolls: false, grows_memory: quench_runtime::bitset::BitSet::default(), declared: None,
        }
    }
}

impl Analysis<'_> {
    fn reject(&self, index: usize, reason: impl FnOnce() -> String) {
        let Some(rejections) = self.compile_rejections else {
            return;
        };
        let mut rejections = rejections.borrow_mut();
        if let Some(rejection) = rejections.get_mut(index)
            && rejection.is_none()
        {
            let reason = reason();
            *rejection = Some(reason.clone());
            eprintln!("QUENCH_WASM_JIT_REJECTION {index} {reason}");
        }
    }

    fn constant(&self, index: usize) -> Option<Value> {
        self.vm.programs.constant(self.program, index)
    }

    /// The machine kind of a constant: `ref.null`'s null as a reference (`Kind::Reference`, its
    /// Value bits), i32-encoded scalars, boxed 64-bit scalars, and any other constant (the void
    /// result) as an inert i32.
    fn constant_kind(&self, value: Value) -> Kind {
        if value.is_null() {
            return Kind::Reference;
        }
        match self.vm.decode_bits64(value) {
            Some(_) => Kind::I64,
            None => Kind::I32,
        }
    }

    /// The slot a constant defines: a vector's, or its machine kind's (`constant_kind`).
    fn constant_slot(&self, value: Value) -> Slot {
        if self.vm.heap_reads().v128(value).is_some() {
            Slot::Vector
        } else {
            Slot::Value(self.constant_kind(value))
        }
    }

    /// A vector constant's bits.
    fn constant_vector(&self, value: Value) -> Option<u128> {
        self.vm.heap_reads().v128(value).map(u128::from_le_bytes)
    }

    /// Check `register` names the function's memory for a vector access: the memory
    /// register, or a register an instance binding loaded the same direct memory into
    /// (lowering binds the memory afresh for each such access), which then is the
    /// function's compiled memory (`memory_slot`).
    fn vector_memory(
        &self,
        function: &Function,
        state: &AnalysisState,
        register: u16,
        memory_register: Option<u16>,
        memory_slot: &mut Option<u32>,
    ) -> Option<()> {
        match state.get(register, function.registers)? {
            Slot::Memory if Some(register) == memory_register => {}
            Slot::Binding(slot)
                if self.binds_direct_memory(slot) && memory_slot.is_none_or(|known| known == slot) =>
            {
                *memory_slot = Some(slot);
            }
            _ => return None,
        }
        memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot)).then_some(())
    }

    /// The byte offset a vector access adds to its address (`WasmMemoryAddress`).
    fn memory_offset(&self, index: usize) -> Option<u64> {
        let offset = self.vm.decode_bits64(self.constant(index)?)?;
        u32::try_from(offset).ok().map(u64::from)
    }

    /// A shuffle's lane selectors: sixteen byte indices into its two operands.
    fn shuffle_lanes(&self, index: usize) -> Option<[u8; quench_runtime::wasm::V128_BYTES]> {
        self.vm.heap_reads().v128(self.constant(index)?)
    }

    fn constant_bits(&self, value: Value) -> i64 {
        if value.is_null() {
            return value.to_bits() as i64;
        }
        match self.vm.decode_bits64(value) {
            Some(bits) => bits as i64,
            None => i64::from(value.wasm_bits32() as i32),
        }
    }

    /// What environment slot `slot` binds: the module's declared type where the plan has no instance
    /// (`declared`, an image's), else the instance's cell.
    fn binding(&self, slot: u32) -> quench_runtime::WasmBindingType {
        use quench_runtime::WasmBindingType as Binding;
        if let Some(declared) = self.declared { return declared.get(slot as usize).copied().unwrap_or(Binding::Other); }
        let heap = self.vm.heap_reads();
        match heap.environment_slot(self.env, slot as usize).and_then(|binding| heap.get(binding)) {
            Some(Cell::WasmGlobal { ty, .. }) => Binding::Global(*ty),
            Some(Cell::WasmMemory { bytes, header, .. }) => Binding::Memory { ty: header.ty, direct: bytes.is_direct() },
            Some(Cell::WasmTable { table64, .. }) => Binding::Table { table64: *table64 },
            _ => Binding::Other,
        }
    }

    /// The type of the global bound at environment slot `slot`.
    fn global_type(&self, slot: u32) -> Option<quench_runtime::WasmType> {
        match self.binding(slot) { quench_runtime::WasmBindingType::Global(ty) => Some(ty), _ => None }
    }

    /// Whether `slot` binds a memory compiled code can view (`MemoryStorage::is_direct`).
    fn binds_direct_memory(&self, slot: u32) -> bool {
        matches!(self.binding(slot), quench_runtime::WasmBindingType::Memory { direct: true, .. })
    }

    /// Whether a bound memory has the representation supported by native
    /// linear-memory operations: direct memory32 with ordinary Wasm pages.
    fn supports_compiled_memory(&self, slot: u32) -> bool {
        matches!(self.binding(slot), quench_runtime::WasmBindingType::Memory { ty, direct: true }
            if !ty.memory64 && ty.page_size() as usize == (1usize << quench_runtime::wasm::memory::WASM_PAGE_BITS))
    }

    fn binds_table32(&self, slot: u32) -> bool {
        matches!(self.binding(slot), quench_runtime::WasmBindingType::Table { table64: false })
    }

    fn signature(&self, index: u32) -> Option<&quench_runtime::WasmSignature> {
        self.vm
            .programs
            .wasm_function_signature(self.program, index)
    }

    /// The signature of `function` when it is imported.
    fn import_signature(&self, function: u32) -> Option<&quench_runtime::WasmSignature> {
        let signatures = self.vm.programs.wasm_signatures(self.program)?;
        signatures
            .defined_index(function)
            .is_none()
            .then(|| signatures.get(function as usize))?
    }

    fn type_signature(&self, index: u32) -> Option<&quench_runtime::WasmSignature> {
        self.vm
            .programs
            .wasm_signatures(self.program)?
            .type_signature(index)
    }

    /// The function's plan, assuming every function in `callable` compiles.
    fn plan(&self, index: usize, callable: &quench_runtime::bitset::BitSet, memory_slot: &mut Option<u32>) -> Option<Plan> {
        let diagnose = self.compile_rejections.is_some();
        let memory_before = diagnose.then(jit_memory_snapshot).flatten();
        let function = &self.code.function(index);
        let Some(signature) = self.signature(index as u32) else {
            self.reject(index, || "missing-signature".into());
            return None;
        };
        if function.locals != 0 && self.code.kind != quench_runtime::bytecode::ProgramKind::Wasm {
            self.reject(index, || format!("frame-locals:{}", function.locals));
            return None;
        }
        if signature.results.len() > 1 {
            self.reject(index, || {
                format!("multi-value-results:{}", signature.results.len())
            });
            return None;
        }
        let params = signature.params.clone();
        let result = signature.results.first().copied();
        if let Some(ty) = params
            .iter()
            .chain(result.iter())
            .find(|ty| Kind::of(**ty).is_none())
        {
            self.reject(index, || format!("unsupported-signature-type:{ty:?}"));
            return None;
        }
        let Some(parameter_base) = function.parameter_registers.map(usize::from) else {
            self.reject(index, || "missing-parameter-register-base".into());
            return None;
        };
        let registers = usize::from(function.registers);
        if parameter_base + params.len() > registers {
            self.reject(index, || "parameters-outside-register-window".into());
            return None;
        }
        let memory_register = crate::dispatch_fast::memory_register(function);
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| function.widened(*packed))
            .collect();
        let mut entry = AnalysisState::new();
        for (offset, ty) in params.iter().enumerate() {
            entry.set(
                u16::try_from(parameter_base + offset).ok()?,
                function.registers,
                Slot::Value(Kind::of(*ty)?),
            )?;
        }
        let (catches, bound) = wasm_catch::catch_frame(function)?;
        let mut edge_starts = Vec::with_capacity(instructions.len() + 1);
        let mut edge_targets = Vec::with_capacity(instructions.len());
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            edge_starts.push(edge_targets.len());
            quench_runtime::bytecode::for_each_instruction_successor(pc, instruction, |successor| {
                if successor < instructions.len() {
                    edge_targets.push(successor);
                }
            });
            edge_targets.extend(catches[pc].map(|catch| catch.target));
        }
        edge_starts.push(edge_targets.len());
        let mut flow = PlanFlow {
            analysis: self,
            index,
            function: *function,
            instructions: &instructions,
            callable,
            memory_register,
            memory_slot,
            result,
            entry,
            catches: &catches,
            bound,
            raised: vec![None; instructions.len()],
            edge_starts,
            edge_targets,
            failed: false,
        };
        let states = quench_runtime::dataflow::solve(&mut flow);
        if flow.failed {
            return None;
        }
        let operands = states
            .iter()
            .zip(&instructions)
            .map(|(state, instruction)| {
                let state = state.as_ref()?;
                let operand = Step::of(instruction.op(), instruction.imm())
                    .map_or(instruction.b(), |step| step.operand(*instruction, function.registers));
                Some(state.get(operand, bound).unwrap_or(Slot::Conflict))
            })
            .collect();
        let seconds: Vec<Option<Slot>> = states
            .iter()
            .zip(&instructions)
            .map(|(state, instruction)| {
                let state = state.as_ref()?;
                let register = Step::of(instruction.op(), instruction.imm())?.second_binding(*instruction)?;
                state.get(register, function.registers)
            })
            .collect();
        let references: Vec<Vec<u16>> = states
            .iter()
            .map(|state| {
                state.as_ref().map_or_else(Vec::new, |state| {
                    state
                        .slots
                        .iter()
                        .filter(|(_, slot)| *slot == Slot::Value(Kind::Reference))
                        .map(|(register, _)| *register)
                        .collect()
                })
            })
            .collect();
        let gc_reads: Vec<Vec<(u16, Kind)>> = states
            .iter()
            .zip(&instructions)
            .map(|(state, instruction)| {
                let (Some(state), Some(reads)) = (state.as_ref(), allocating_reads(*instruction).or_else(|| wasm_exception_stage::reads(*instruction))) else {
                    return Vec::new();
                };
                // Every operand is checked as a value at analysis, so none is dropped here.
                reads
                    .into_iter()
                    .map(|register| match state.get(register, function.registers)? {
                        Slot::Value(kind) => Some((register, kind)),
                        Slot::Memory32Address => Some((register, Kind::I64)),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_default()
            })
            .collect();
        if diagnose {
            plan_report::report(index, memory_before, instructions.len(), registers, &states);
        }
        Some(Plan {
            params,
            result,
            operands,
            seconds,
            references,
            gc_reads,
            registers: function.registers,
            catches,
        })
    }

    /// Check `instruction` has a compiled denotation in `state`, and apply its
    /// register writes. `memory` is the function's memory register and the slot it binds.
    fn transfer(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        callable: &quench_runtime::bitset::BitSet,
        memory: (Option<u16>, &mut Option<u32>),
        result: Option<quench_runtime::WasmType>,
    ) -> Option<()> {
        let step = Step::of(instruction.op(), instruction.imm())?;
        admitted(function, instruction, step, memory.0)?;
        let (memory_register, memory_slot) = memory;
        match step {
            Step::Numeric(operation) => {
                let value = |state: &AnalysisState, register: u16| {
                    wasm_instance_stage::value_kind(state.get(register, function.registers)?)
                };
                wasm_stage::transfer(state, function.registers, instruction, operation, &value)
            }
            Step::Instance(step) => {
                self.transfer_instance(function, instruction, state, step, (memory_register, memory_slot))
            }
            Step::Memory(step) => {
                self.transfer_memory(function, instruction, state, step, (memory_register, *memory_slot))
            }
            Step::Table(op) => {
                self.transfer_table(function, instruction, state, op, (memory_register, *memory_slot))
            }
            Step::Gc(op) => self.transfer_gc(function, instruction, state, op),
            Step::Exception(op) => self.transfer_exception(function, instruction, state, op),
            Step::Flow(flow) => self.transfer_flow(function, instruction, state, flow, result),
            Step::Call(call) => self.transfer_call(function, instruction, state, call, callable),
            Step::Simd(step) => {
                self.transfer_simd(function, instruction, state, step, (memory_register, memory_slot))
            }
            Step::Atomic(_) => None,
        }
    }
}

/// Checks the lane fields of `instruction` fit, only a return leaves the frame, and the memory
/// register is read only by the steps that name a memory.
fn admitted(
    function: &Function,
    instruction: WideInstruction,
    step: Step,
    memory_register: Option<u16>,
) -> Option<()> {
    let fits = crate::dispatch_fast::fields_fit_in_lane(instruction, function);
    let leaves = instruction.returns_from_frame() && !step.returns();
    let reads = memory_register
        .is_some_and(|register| !step.reads_memory() && reads_register(instruction, register, step));
    (fits && !leaves && !reads).then_some(())
}

/// Whether `instruction` reads `register` through a field `step` does not leave unread.
fn reads_register(instruction: WideInstruction, register: u16, step: Step) -> bool {
    let op = instruction.op();
    quench_runtime::bytecode::InstructionField::ALL.iter().any(|field| {
        op.field_layout(*field).reads_register()
            && instruction.field_value(*field) == register
            && !step.leaves_unread(*field)
    })
}

/// Disjoint state compiled code accesses: an access to one never aliases an
/// access to another, so loads of one survive stores to the other.
#[derive(Clone, Copy, Debug)]
enum Region {
    /// The instance's linear memory.
    LinearMemory,
    /// Global values, which live in heap cells.
    Globals,
}

impl Region {
    const ALL: [Self; 2] = [Self::LinearMemory, Self::Globals];
    const COUNT: usize = Self::ALL.len();
}

/// Flags of a load of context state fixed for the whole call (the stack
/// limit, the callout, global addresses): it may be reused and hoisted.
fn invariant() -> MemFlagsData {
    MemFlagsData::trusted().with_readonly().with_can_move()
}

#[cfg(test)]
thread_local! {
    /// The CLIF of each Wasm body this thread built, by function index, for tests of what the
    /// tier emits.
    static BUILT_BODIES: std::cell::RefCell<Vec<(usize, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The memory facts a compiled call passes after the context: the base
/// unless the program fixes it as a constant, and the length unless the
/// guard bounds accesses. One program's functions share them.
#[derive(Clone, Copy)]
struct MemoryArguments {
    base: bool,
    length: bool,
}

impl MemoryArguments {
    #[inline]
    fn of(fixed_base: Option<i32>, guarded: bool) -> Self {
        Self {
            base: fixed_base.is_none(),
            length: !guarded,
        }
    }

    /// The passed ones of `base` and `length`, in parameter order.
    fn select(self, base: ir::Value, length: ir::Value) -> impl Iterator<Item = ir::Value> {
        [self.base.then_some(base), self.length.then_some(length)]
            .into_iter()
            .flatten()
    }

    fn count(self) -> usize {
        usize::from(self.base) + usize::from(self.length)
    }
}

/// The calling convention of compiled Wasm bodies and of the calls between them. `Tail` makes a
/// flagged call a true tail call (`return_call` / `return_call_indirect`, which Cranelift allows
/// only between functions of one convention that supports tail calls). The entry trampoline
/// (`build_entry`) keeps the default convention and calls its body with this one; the callouts
/// into the VM keep the default, as they call Rust. With `unwind_info` on (the u-jit's setting),
/// `Tail` saves the SystemV callee-saved registers, so a SystemV caller of a body is unaffected.
const BODY_CALL_CONV: cranelift_codegen::isa::CallConv = cranelift_codegen::isa::CallConv::Tail;

/// The internal signature of a compiled function: the context, the memory
/// arguments, then the Wasm parameters.
fn internal_signature(
    memory: MemoryArguments,
    plan_params: &[quench_runtime::WasmType],
    result: Option<quench_runtime::WasmType>,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    let params = (0..1 + memory.count())
        .map(|_| types::I64)
        .chain(plan_params.iter().map(|ty| Kind::of(*ty).expect("planned").ir()))
        .collect::<Vec<_>>();
    let returns = std::iter::once(types::I64)
        .chain(result.map(|ty| Kind::of(ty).expect("planned").ir()))
        .collect::<Vec<_>>();
    call_signature::typed_signature(call_conv, &params, &returns)
}

/// Where a threaded copy continues once its path is done.
#[derive(Clone, Copy)]
enum ThreadEnd {
    /// The branch table's case for the constant.
    Case(usize),
    /// The successor the constant decided a branch to.
    Exit(usize),
    /// The table at the path's end, run for real: a jump that knows no
    /// selector gets its own copy of the dispatch (`dispatch_path`), so each
    /// case's back edge predicts its own successor.
    Dispatch,
}

/// Bytecodes up to which a compiled Wasm program unrolls its innermost loops. Every function
/// compiles eagerly, and each copy is compiled whether its loop runs or not: in programs of
/// 17K to 70K bytecodes the copies cost more compile time than they saved (TSF's first
/// iteration +6%, gcc-loops +2.5%, HashSet +1.2% per iteration from spills), so only smaller
/// programs pay for them.
const UNROLL_PROGRAM_BYTECODES: usize = 16_384;

/// Whether `program` is small enough to unroll its innermost loops (`UNROLL_PROGRAM_BYTECODES`).
fn program_unrolls(program: &ResidualProgram) -> bool {
    // A function whose record has not been read yet (compiled again or decoded on first
    // read) is not counted.
    program
        .functions
        .iter()
        .filter_map(quench_runtime::bytecode::FunctionSlot::record)
        .map(|function| function.code.len())
        .sum::<usize>()
        <= UNROLL_PROGRAM_BYTECODES
}

/// An innermost loop unrolled at its latch: the back edge enters the next of `factor - 1`
/// copies of the body, and the last copy's back edge returns to `header` with the loop's
/// interrupt check (`Emitter::backedge`).
#[derive(Clone, Copy)]
struct UnrolledLoop {
    header: usize,
    factor: usize,
}

/// Whether `op` may sit in an unrolled body: no call, and no operation that calls out to the
/// VM or ends the body other than by a branch out of the loop.
fn unrollable_op(op: Op) -> bool {
    !op.traits().has(quench_runtime::bytecode::Traits::UNROLL_EXCLUDED)
}

/// The innermost loops to unroll, by latch: a back edge from `latch` to `header` whose body
/// `header..=latch` runs straight through but for branches that leave the loop, has no other
/// entry, and fits the budget at least twice.
fn unrollable_loops(instructions: &[WideInstruction], plan: &Plan) -> Vec<Option<UnrolledLoop>> {
    let mut entered = vec![false; instructions.len() + 1];
    for (pc, instruction) in instructions.iter().enumerate() {
        if !plan.reachable(pc) {
            continue;
        }
        match instruction.op().control_flow_layout() {
            ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                entered[instruction.jump_target() as usize] = true;
            }
            ControlFlowLayout::BranchTable => {
                for entry in quench_runtime::bytecode::branch_table_entries(pc, *instruction) {
                    entered[instructions[entry].jump_target() as usize] = true;
                }
            }
            _ => {}
        }
    }
    let mut loops = vec![None; instructions.len()];
    for (latch, header) in quench_runtime::bytecode::back_edges_of(instructions.iter().copied()) {
        if !plan.reachable(latch) {
            continue;
        }
        let length = latch - header + 1;
        let factor = (UNROLL_BUDGET / length).min(MAX_UNROLL_FACTOR);
        if factor < 2 {
            continue;
        }
        let straight = (header..latch).all(|pc| {
            let body = instructions[pc];
            plan.reachable(pc)
                && unrollable_op(body.op())
                && !wasm_call_stage::is_tail_call(body)
                && match body.op().control_flow_layout() {
                    ControlFlowLayout::Fallthrough => true,
                    ControlFlowLayout::ConditionalJump => body.jump_target() as usize > latch,
                    _ => false,
                }
        });
        if straight && !entered[header + 1..=latch].iter().any(|&entered| entered) {
            loops[latch] = Some(UnrolledLoop { header, factor });
        }
    }
    loops
}

/// A copy of the instructions from a jump target to a branch table, for a
/// jump that leaves the table's selector holding a known constant: along
/// the copy the selector cannot change, so branches on it are decided and
/// the table becomes a direct jump (jump threading; a state machine's
/// dispatch on the state it just set disappears). The copy repeats each
/// instruction at its own PC and exits only to block leaders.
struct Thread {
    selector: u16,
    value: i32,
    path: Vec<usize>,
    end: ThreadEnd,
}

/// Whether `instruction` may write `register`; windows and other register
/// groups count as writing every register.
fn writes_register(instruction: WideInstruction, register: u16) -> bool {
    use quench_runtime::bytecode::{FieldLayout, InstructionField};
    InstructionField::ALL.iter().any(|field| {
        let operand = match field {
            InstructionField::A => instruction.a(),
            InstructionField::B => instruction.b(),
            InstructionField::C => instruction.c(),
        };
        match instruction.op().field_layout(*field) {
            FieldLayout::ResultRegister
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => operand == register,
            FieldLayout::RegisterWindowBase
            | FieldLayout::RegisterCount
            | FieldLayout::ConstructArguments
            | FieldLayout::NumericLocalTarget => true,
            _ => false,
        }
    })
}

/// Whether conditional jump `instruction` is taken when `register` holds
/// `value`, if that alone decides it.
fn decided_branch(instruction: WideInstruction, register: u16, value: i32) -> Option<bool> {
    if instruction.a() != register {
        return None;
    }
    match WasmOperation::of(instruction.op(), instruction.imm())? {
        WasmOperation::Test { nonzero } => Some((value != 0) == nonzero),
        WasmOperation::Binary {
            operator,
            width: Width::I32,
            form: Form::BranchImmediate,
        } => {
            let Right::Constant(right) = Form::BranchImmediate.right(instruction) else {
                return None;
            };
            let (left, right) = (
                ConcreteWasm.constant(Width::I32, i64::from(value)),
                ConcreteWasm.constant(Width::I32, right),
            );
            let holds = integer_binary(&mut ConcreteWasm, operator, Width::I32, left, right);
            holds.ok().map(|holds| holds != 0)
        }
        _ => None,
    }
}

/// The threaded copy from `start` for `selector` holding `value`, when one
/// reaches a branch table on the selector, or a branch the value decides,
/// within the budget.
fn thread_path(
    instructions: &[WideInstruction],
    plan: &Plan,
    start: usize,
    selector: u16,
    value: i32,
) -> Option<Thread> {
    fn walk(
        instructions: &[WideInstruction],
        plan: &Plan,
        pc: usize,
        selector: u16,
        value: i32,
        path: &mut Vec<usize>,
    ) -> Option<ThreadEnd> {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        // A tail call ends its function: a copy cannot continue past it.
        if writes_register(instruction, selector) || wasm_call_stage::is_tail_call(instruction) {
            return None;
        }
        path.push(pc);
        let end = match instruction.op().control_flow_layout() {
            ControlFlowLayout::BranchTable if instruction.a() == selector => {
                let entries: Vec<usize> = quench_runtime::bytecode::branch_table_entries(pc, instruction)
                    .map(|entry| instructions[entry].jump_target() as usize)
                    .collect();
                let (default, cases) = entries.split_last()?;
                let case = usize::try_from(value as u32).ok();
                Some(ThreadEnd::Case(
                    *case.and_then(|case| cases.get(case)).unwrap_or(default),
                ))
            }
            ControlFlowLayout::Fallthrough => {
                walk(instructions, plan, pc + 1, selector, value, path)
            }
            ControlFlowLayout::Jump => walk(
                instructions,
                plan,
                instruction.jump_target() as usize,
                selector,
                value,
                path,
            ),
            ControlFlowLayout::ConditionalJump => {
                let target = instruction.jump_target() as usize;
                match decided_branch(instruction, selector, value) {
                    Some(taken) => {
                        let next = if taken { target } else { pc + 1 };
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            Some(ThreadEnd::Exit(next))
                        })
                    }
                    None => [target, pc + 1].into_iter().find_map(|next| {
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            None
                        })
                    }),
                }
            }
            _ => None,
        };
        if end.is_none() {
            path.pop();
        }
        end
    }
    let mut path = Vec::new();
    let end = walk(instructions, plan, start, selector, value, &mut path)?;
    Some(Thread {
        selector,
        value,
        path,
        end,
    })
}

/// The straight-line copy from jump target `start` to the branch table it
/// reaches, within the threading budget; the table is the path's last
/// instruction.
fn dispatch_path(instructions: &[WideInstruction], plan: &Plan, start: usize) -> Option<Thread> {
    let mut path = Vec::new();
    let mut pc = start;
    loop {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        // A tail call ends its function: a copy cannot continue past it.
        if wasm_call_stage::is_tail_call(instruction) {
            return None;
        }
        path.push(pc);
        pc = match instruction.op().control_flow_layout() {
            ControlFlowLayout::Fallthrough => pc + 1,
            ControlFlowLayout::Jump => instruction.jump_target() as usize,
            ControlFlowLayout::BranchTable => {
                // The copy repeats every edge of the table, each a jump with its own moves.
                let edges = quench_runtime::bytecode::branch_table_entries(pc, instruction).count();
                if path.len() - 1 + edges > THREAD_BUDGET {
                    return None;
                }
                return Some(Thread {
                    selector: instruction.a(),
                    value: 0,
                    path,
                    end: ThreadEnd::Dispatch,
                });
            }
            _ => return None,
        };
    }
}

/// Whether a Wasm function is a leaf: every reachable step is arithmetic, control flow, a
/// register, constant or global, or a plain memory access, so it calls no function and re-enters
/// no VM, and its frame is small (`LEAF_REGISTERS`). Its stack use is bounded by that frame and
/// its running time by its loops' back edges, which poll themselves.
fn is_leaf(instructions: &[WideInstruction], plan: &Plan, registers: u16) -> bool {
    use quench_runtime::staged::wasm_memory::Memory;
    /// The most registers a leaf's frame holds: a few KiB, well inside the stack budget's margin.
    const LEAF_REGISTERS: u16 = 256;
    registers <= LEAF_REGISTERS
        && instructions.iter().enumerate().all(|(pc, instruction)| {
            !plan.reachable(pc)
                || matches!(
                    Step::of(instruction.op(), instruction.imm()),
                    Some(
                        Step::Numeric(_)
                            | Step::Flow(_)
                            | Step::Instance(_)
                            | Step::Memory(Memory::Load(_) | Memory::Store(_) | Memory::Size)
                    )
                )
        })
}

/// Bind the activation's stable interrupt capability. The pointer is invariant;
/// the byte it addresses remains atomic and must be read at every safepoint.
fn native_interrupt_pointer(builder: &mut FunctionBuilder<'_>, context: ir::Value, fault_poll: bool) -> ir::Value {
    let offset = if fault_poll { std::mem::offset_of!(CallContext, head.poll_start) as i32 }
        else { CONTEXT_INTERRUPT };
    object_slots::emit(builder).raw_load(quench_runtime::staged::machine::Lane::I64, invariant(), context, offset)
}

/// The common native region core: builder, call context, interrupt check and its
/// termination block. Frontends keep their own state representation and exit semantics.
pub(crate) struct RegionCore<'b> {
    builder: FunctionBuilder<'b>,
    context: ir::Value,
    /// The VM's poll page when every entry runs through the fault-resuming trampoline.
    poll: Option<ir::Value>,
    /// Where a flag check that finds termination requested goes; the frontend fills it with
    /// the terminated status (`TERMINATED_STATUS`).
    termination_block: Option<ir::Block>,
}

impl<'b> RegionCore<'b> {
    fn new(mut builder: FunctionBuilder<'b>, context: ir::Value, poll: Option<i64>) -> Self {
        let poll = poll.map(|_| native_interrupt_pointer(&mut builder, context, true));
        Self {
            builder,
            context,
            poll,
            termination_block: None,
        }
    }

    fn into_builder(self) -> FunctionBuilder<'b> {
        self.builder
    }

    /// Checks for a termination request: a read of the poll page, which faults once one is
    /// made and resumes as the terminated status (`guard_fault`), else the shared back-edge poll
    /// (`staged::poll::back_edge_poll`) of the context's flag, which leaves to `termination_block`.
    fn interrupt_safepoint(&mut self) {
        if let Some(poll) = self.poll {
            // The read faults once termination is requested, and the fault resumes as the
            // termination (`guard_fault`). Atomic, so it is neither merged nor dropped.
            self.raw_atomic_load(quench_runtime::staged::machine::Lane::I8, MemFlagsData::trusted(), poll);
            return;
        }
        let context = self.context;
        let flag = native_interrupt_pointer(&mut self.builder, context, false);
        let termination = *self.termination_block.get_or_insert_with(|| self.builder.create_block());
        // The poll is no speculation: the forced-deopt switch leaves it alone (`unforced`).
        unforced(|| quench_runtime::staged::poll::back_edge_poll(&mut object_slots::emit(&mut self.builder), flag, termination)).expect("a guard always emits");
    }
}

impl<'b> Deref for RegionCore<'b> {
    type Target = FunctionBuilder<'b>;

    fn deref(&self) -> &Self::Target {
        &self.builder
    }
}

impl<'b> DerefMut for RegionCore<'b> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.builder
    }
}

/// Scalar Wasm and speculative JS numeric register views layered on the common region core.
///
/// A register has one SSA variable per view type (`VIEW_TYPES`). Writing a value defines its
/// own view and the other view of the same bits (`counterpart`); a view nothing reads costs
/// nothing, since its conversion is dead. A Wasm value is its scalar; a JavaScript Number a
/// region holds unboxed is an `i32` or an `f64` view.
pub(crate) struct Body<'b> {
    core: RegionCore<'b>,
    /// Per register, its views, declared on first use.
    variables: Vec<Option<[Variable; VIEW_TYPES.len()]>>,
    /// Whether i32 views retain an unsigned address offset or a signed scalar.
    int32_extension: Int32Extension,
    /// Each i32 read from a register's view, by the extended value it was read from.
    extended: SecondaryMap<ir::Value, PackedOption<ir::Value>>,
}

/// The interpretation of a register's i32 view and its widening at consumers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Int32Extension {
    /// A Wasm i32 is an unsigned address offset.
    Zero,
    /// A JS Number held as an i32 stays narrow; an element access sign-extends it.
    Sign,
}

impl<'b> Body<'b> {
    fn new(builder: FunctionBuilder<'b>, context: ir::Value, poll: Option<i64>, int32_extension: Int32Extension) -> Self {
        Self {
            core: RegionCore::new(builder, context, poll),
            variables: Vec::new(),
            int32_extension,
            extended: SecondaryMap::new(),
        }
    }

    fn into_builder(self) -> FunctionBuilder<'b> {
        self.core.into_builder()
    }

    /// The register's views.
    fn views(&mut self, register: u16) -> [Variable; VIEW_TYPES.len()] {
        let index = usize::from(register);
        if self.variables.len() <= index {
            self.variables.resize(index + 1, None);
        }
        if let Some(views) = self.variables[index] {
            return views;
        }
        let extension = self.int32_extension;
        let views = VIEW_TYPES.map(|ty| self.builder.declare_var(view_ir(ty, extension)));
        self.variables[index] = Some(views);
        views
    }

    /// The register's value viewed as `ty`.
    fn read(&mut self, register: u16, ty: quench_runtime::WasmType) -> ir::Value {
        use quench_runtime::staged::machine::{Lane, Unary};
        let variable = self.views(register)[view(ty)];
        let stored = self.builder.use_var(variable);
        if ty != quench_runtime::WasmType::I32 || self.int32_extension == Int32Extension::Sign {
            return stored;
        }
        let value = self.raw_unary(Unary::Narrow(Lane::I32), stored);
        self.extended[value] = stored.into();
        value
    }

    /// `value`, an i32, extended to 64 bits as `extension` does: the i32 view it was read
    /// from, if the view holds it so.
    fn extend(&mut self, value: ir::Value, extension: Int32Extension) -> ir::Value {
        use quench_runtime::staged::machine::{Lane, Unary};
        match self.extended[value].expand() {
            Some(wide) if extension == self.int32_extension => wide,
            _ => match extension {
                Int32Extension::Zero => self.raw_unary(Unary::ZeroExtend(Lane::I64), value),
                Int32Extension::Sign => self.raw_unary(Unary::SignExtend(Lane::I64), value),
            },
        }
    }

    /// `value`, an i32, zero-extended to 64 bits.
    fn zero_extend(&mut self, value: ir::Value) -> ir::Value {
        self.extend(value, Int32Extension::Zero)
    }

    /// The stored form of `value` in the register's view as `ty` (`view_ir`).
    fn view_value(&mut self, ty: quench_runtime::WasmType, value: ir::Value) -> ir::Value {
        match ty {
            quench_runtime::WasmType::I32 if self.int32_extension == Int32Extension::Zero => self.zero_extend(value),
            _ => value,
        }
    }

    /// Define the register as `value` of type `ty`, in both views of its bits.
    fn write(&mut self, register: u16, ty: quench_runtime::WasmType, value: ir::Value) {
        use quench_runtime::staged::machine::Unary;
        let other = counterpart(ty);
        let converted = self.raw_unary(Unary::Bitcast(emit_machine::of_in_lane(scalar_ir(other))), value);
        let stored = self.view_value(ty, value);
        let converted = self.view_value(other, converted);
        let variables = self.views(register);
        self.builder.def_var(variables[view(ty)], stored);
        self.builder.def_var(variables[view(other)], converted);
    }

    fn interrupt_safepoint(&mut self) {
        self.core.interrupt_safepoint();
    }
}

impl<'b> Deref for Body<'b> {
    type Target = RegionCore<'b>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl<'b> DerefMut for Body<'b> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

pub(crate) struct Emitter<'a, 'b> {
    guarded: bool,
    /// Per `Region`, its alias region in this function.
    regions: [ir::AliasRegion; Region::COUNT],
    analysis: &'a Analysis<'a>,
    body: Body<'b>,
    plan: &'a Plan,
    blocks: Vec<Option<ir::Block>>,
    /// Dense ids for the compiled artifact's actual indirect call sites.
    reference_site_ids: &'a [(usize, usize)],
    /// Selects the diagnostics ABI at compile time; both ABIs carry site ids.
    diagnostics_enabled: bool,
    base: ir::Value,
    /// The memory's exact length for checked accesses and bulk operations.
    length: Option<ir::Value>,
    /// Whether this function may grow the memory (`wasm_growth`): if not, `length` holds for its
    /// whole activation even where callouts may refresh the context's length.
    grows: bool,
    memory: MemoryArguments,
    trap_blocks: [Option<ir::Block>; TRAPS.len()],
    /// Per callee, its imported name in this function.
    callees: Vec<Option<ir::FuncRef>>,
    callout_signature: Option<ir::SigRef>,
    plans: &'a [Option<Plan>],
    call_conv: cranelift_codegen::isa::CallConv,
    instructions: &'a [WideInstruction],
    /// Registers some branch table dispatches on.
    selectors: Vec<u16>,
    /// Selectors known to hold an i32 constant since the current block began.
    known: Vec<(u16, i32)>,
    /// Threaded copies by (target, selector, value); `None` where none fits.
    threads: Vec<((usize, u16, i32), Option<ir::Block>)>,
    /// Copies to emit once the function body is done.
    pending: Vec<(ir::Block, Thread)>,
    /// While a copy emits a branch, the block standing in for one target.
    redirect: Option<(usize, ir::Block)>,
    /// Whether jumps may enter threaded copies (not from within one).
    threading: bool,
    /// The innermost loops to unroll, by latch (`unrollable_loops`).
    unrolled: Vec<Option<UnrolledLoop>>,
    /// While a latch of an unrolled loop is emitted: its header and the copy its back edge
    /// enters instead, or `None` for the last copy, whose back edge returns to the header and
    /// checks for an interrupt.
    backedge: Option<(usize, Option<ir::Block>)>,
    /// Copies of an unrolled body to emit right after its latch: the block, the loop, and the
    /// copy its back edge enters.
    pending_copies: Vec<(ir::Block, UnrolledLoop, usize, Option<ir::Block>)>,
    /// Per register, its 128-bit SIMD variable (`Slot::Vector`), declared at its first
    /// vector definition; scalar views never hold a vector.
    vectors: Vec<Option<Variable>>,
    /// The words a SIMD kernel callout reads its operands from and writes its result to
    /// (`compiled_simd_kernel`), made at the first callout.
    simd_words: Option<ir::StackSlot>,
    /// The table an indirect target is being emitted for (`WasmTable`).
    indirect: Option<wasm_table_stage::IndirectSite>,
    /// The row an indirect target just checked, which the call that follows reads again.
    checked_row: Option<wasm_table_stage::CheckedRow>,
    /// The reference registers the instruction being emitted holds (`Plan::references`): the
    /// roots a call it makes holds (`wasm_roots`).
    roots: &'a [u16],
    /// The instruction being emitted: a raised status of its calls takes its catch (`Plan::catches`).
    pc: usize,
}

impl Emitter<'_, '_> {
    /// The flags of an access to `region`.
    fn flags(&self, region: Region) -> MemFlagsData {
        MemFlagsData::trusted()
            .with_endianness(ir::Endianness::Little)
            .with_alias_region(Some(self.regions[region as usize]))
    }

    fn read(&mut self, register: u16, kind: Kind) -> ir::Value {
        self.read_typed(register, kind.integer())
    }

    fn write(&mut self, register: u16, kind: Kind, value: ir::Value) {
        self.write_typed(register, kind.integer(), value);
    }

    /// The register's value viewed as `ty`.
    fn read_typed(&mut self, register: u16, ty: quench_runtime::WasmType) -> ir::Value {
        self.body.read(register, ty)
    }

    fn zero_extend(&mut self, value: ir::Value) -> ir::Value {
        self.body.zero_extend(value)
    }

    /// Define the register as `value` of type `ty` (`Body::write`), which ends what a branch
    /// table selector was known to hold.
    fn write_typed(&mut self, register: u16, ty: quench_runtime::WasmType, value: ir::Value) {
        self.known.retain(|(known, _)| *known != register);
        self.body.write(register, ty, value);
    }

    /// Define the register as the constant `bits` of width `kind`.
    fn write_constant(&mut self, register: u16, kind: Kind, bits: i64) {
        use quench_runtime::staged::machine::Lane;
        self.known.retain(|(known, _)| *known != register);
        if kind == Kind::I32 && self.selectors.contains(&register) {
            self.known.push((register, bits as i32));
        }
        let (integer, float) = match kind {
            // The i32 view holds the constant zero-extended (`view_ir`).
            Kind::I32 => (self.raw_const(Lane::I64, i64::from(bits as u32)), self.raw_const(Lane::F32, bits)),
            Kind::I64 | Kind::Reference => (self.raw_const(Lane::I64, bits), self.raw_const(Lane::F64, bits)),
        };
        let float_type = counterpart(kind.integer());
        let variables = self.body.views(register);
        self.body.core.builder
            .def_var(variables[view(kind.integer())], integer);
        self.body.core.builder.def_var(variables[view(float_type)], float);
    }

    /// The kind of the `b` register of instruction `pc`.
    fn operand_kind(&self, pc: usize) -> Kind {
        match self.plan.operand(pc) {
            Slot::Value(kind) => kind,
            Slot::Memory32Address => Kind::I64,
            _ => Kind::I32,
        }
    }

    fn block(&mut self, pc: usize) -> ir::Block {
        *self.blocks[pc].get_or_insert_with(|| self.body.core.builder.create_block())
    }

    fn trap_block(&mut self, trap: quench_runtime::WasmTrap) -> ir::Block {
        let index = (trap_status(trap) - 1) as usize;
        if let Some(block) = self.trap_blocks[index] {
            return block;
        }
        let block = self.body.core.builder.create_block();
        self.body.core.builder.set_cold_block(block);
        self.trap_blocks[index] = Some(block);
        block
    }

    /// Branch to the trap when `condition` is nonzero; continue otherwise.
    fn trap_if(&mut self, condition: ir::Value, trap: quench_runtime::WasmTrap) {
        let trap = self.trap_block(trap);
        let next = self.body.core.builder.create_block();
        self.branch_if(condition, (trap, &[]), (next, &[]));
        self.body.core.builder.switch_to_block(next);
        self.body.core.builder.seal_block(next);
    }

    fn interrupt_safepoint(&mut self) {
        self.body.interrupt_safepoint();
    }

    fn finish_traps(&mut self) {
        use quench_runtime::staged::machine::Lane;
        if let Some(block) = self.body.termination_block {
            self.body.core.builder.switch_to_block(block);
            let status = self.raw_const(Lane::I64, TERMINATED_STATUS);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.raw_const(kind.lane(), 0));
            }
            self.leave(&values);
        }
        for (index, block) in self.trap_blocks.into_iter().enumerate() {
            let Some(block) = block else { continue };
            self.body.core.builder.switch_to_block(block);
            let status = self.raw_const(Lane::I64, index as i64 + 1);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.raw_const(kind.lane(), 0));
            }
            self.leave(&values);
        }
    }

    /// The address of the native slot of the global bound at `slot`: the instance's global area
    /// holds it, indexed by the binding.
    fn global_address(&mut self, slot: u32) -> ir::Value {
        use quench_runtime::staged::machine::Lane;
        let context = self.body.core.context;
        let table = self.raw_load(Lane::I64, invariant(), context, CONTEXT_GLOBALS);
        self.raw_load(Lane::I64, invariant(), table, (slot as usize * size_of::<*mut u64>()) as i32)
    }

    /// The binding the `b` register of instruction `pc` holds.
    fn binding_slot(&self, pc: usize) -> u32 {
        match self.plan.operand(pc) {
            Slot::Binding(slot) => slot,
            _ => unreachable!("planned global accesses read a binding"),
        }
    }

    /// A nonzero status from a call (a trap or a raised callout) unwinds
    /// this activation too.
    fn propagate(&mut self, status: ir::Value) {
        let unwound = self.body.core.builder.create_block();
        self.body.core.builder.set_cold_block(unwound);
        let next = self.body.core.builder.create_block();
        self.branch_if(status, (unwound, &[]), (next, &[]));
        self.body.core.builder.switch_to_block(unwound);
        self.body.core.builder.seal_block(unwound);
        match self.plan.catches[self.pc] {
            Some(catch) => self.catch_raised(status, catch),
            None => self.leave_raised(status),
        }
        self.body.core.builder.switch_to_block(next);
        self.body.core.builder.seal_block(next);
    }

    /// An imported call: arguments go to the VM in a stack array, through
    /// the context's callout.
    fn callout(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::Import(function) = self.plan.operand(pc) else {
            unreachable!("planned imported calls read an import reference");
        };
        let signature = self
            .analysis
            .import_signature(function)
            .expect("planned")
            .clone();
        self.callout_function(function, &signature, instruction);
    }

    /// A statically known Wasm callee that did not pass native planning still
    /// crosses the ordinary VM boundary. Its body can run interpreted (and
    /// can itself enter any compatible compiled function).
    fn callout_known_fallback(&mut self, function: u32, instruction: WideInstruction) {
        let signature = self
            .analysis
            .signature(function)
            .expect("known signature")
            .clone();
        // `CallKnown` stores the defined-function-local index, while the
        // ordinary VM callout and function-reference lookup use Wasm's
        // absolute function index (imports first).
        let absolute_function = self
            .analysis
            .vm
            .programs
            .wasm_signatures(self.analysis.program)
            .and_then(|signatures| {
                u32::try_from(signatures.imports().len())
                    .ok()?
                    .checked_add(function)
            })
            .expect("validated known function index");
        self.callout_function(absolute_function, &signature, instruction);
    }

    fn callout_function(
        &mut self,
        function: u32,
        signature: &quench_runtime::WasmSignature,
        instruction: WideInstruction,
    ) {
        use quench_runtime::staged::machine::{Lane, Unary};
        let word = types::I64.bytes();
        let signature = signature.clone();
        let words = signature.params.len() + 1;
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.raw_unary(Unary::ZeroExtend(Lane::I64), value),
                Kind::I64 | Kind::Reference => value,
            };
            let address = self.slot_address(slot, (offset as u32 * word) as i32);
            self.raw_store(Lane::I64, MemFlagsData::trusted(), (value, false), address, 0);
        }
        let result_offset = (signature.params.len() as u32 * word) as i32;
        let args = self.slot_address(slot, 0);
        let out = self.slot_address(slot, result_offset);
        let callout_signature = match self.callout_signature {
            Some(signature) => signature,
            None => {
                let callout = call_signature::word_signature(self.call_conv, 4, 1, types::I64);
                let callout = self.body.core.builder.import_signature(callout);
                self.callout_signature = Some(callout);
                callout
            }
        };
        let context = self.body.core.context;
        let target = self.entry_target(crate::compiled::slow_paths::EngineEntry::WasmCallOut, invariant(), context, CONTEXT_CALLOUT);
        let function = self.raw_const(Lane::I64, i64::from(function));
        let roots = self.root();
        let status = self.call_signature(callout_signature, target, &[context, function, args, out])[0];
        self.unroot(roots);
        self.propagate(status);
        let destination = instruction.result_register();
        match signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self.raw_load(Lane::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.raw_unary(Unary::Narrow(Lane::I32), bits),
                    Kind::I64 | Kind::Reference => bits,
                };
                self.write(destination, kind, value);
            }
            None => self.write_constant(destination, Kind::I32, 0),
        }
    }

    /// Emit a jump, polling only for the entry transfer or a loop backedge.
    /// Every bytecode control-flow cycle has a backedge, so acyclic edges do
    /// not need an atomic load to preserve cancellation responsiveness.
    fn jump(&mut self, target: usize, interrupt_check: bool) {
        if interrupt_check {
            self.interrupt_safepoint();
        }
        let mut block = self.edge(target);
        if self.threading
            && Some(block) == self.blocks[target]
            && let Some(thread) = dispatch_path(self.instructions, self.plan, target)
        {
            block = self.body.core.builder.create_block();
            self.pending.push((block, thread));
        }
        self.goto(block, &[]);
    }

    /// A conditional transfer from `pc`. It checks for an interrupt when
    /// either edge leads to `pc` or before: a loop repeats through one of
    /// them (`JumpI32Zero` passes its targets swapped, so either may be the
    /// one behind). The check sits before the branch so the backedge stays
    /// direct: routing it through a block of its own measured 10% more
    /// CoreMark instructions. A loop latch (a taken target behind the
    /// fallthrough) leaves the loop through a block of its own, so values
    /// only the exit passes (merge arguments) are computed after the loop,
    /// not on every iteration.
    fn branch(&mut self, pc: usize, condition: ir::Value, target: usize, fallthrough: usize) {
        if self.polls(pc, target) || self.polls(pc, fallthrough) {
            self.interrupt_safepoint();
        }
        let taken = self.edge(target);
        let next = self.edge(fallthrough);
        if target >= fallthrough {
            self.branch_if(condition, (taken, &[]), (next, &[]));
            return;
        }
        let exit = self.body.core.builder.create_block();
        self.branch_if(condition, (taken, &[]), (exit, &[]));
        self.body.core.builder.switch_to_block(exit);
        self.goto(next, &[]);
    }

    /// Whether an edge from `pc` to `target` checks for an interrupt: a back edge does, except
    /// one into a copy of an unrolled body, whose last copy's back edge checks for all.
    fn polls(&self, pc: usize, target: usize) -> bool {
        target <= pc && !matches!(self.backedge, Some((header, Some(_))) if header == target)
    }

    /// The block a jump from here to `target` enters: a threaded copy when
    /// a known selector decides a branch table ahead, else the target's.
    fn edge(&mut self, target: usize) -> ir::Block {
        if let Some((redirected, block)) = self.redirect
            && redirected == target
        {
            return block;
        }
        if let Some((header, Some(copy))) = self.backedge
            && header == target
        {
            return copy;
        }
        if self.threading {
            for (selector, value) in self.known.clone() {
                if let Some(block) = self.thread(target, selector, value) {
                    return block;
                }
            }
        }
        self.block(target)
    }

    fn thread(&mut self, target: usize, selector: u16, value: i32) -> Option<ir::Block> {
        let key = (target, selector, value);
        if let Some((_, block)) = self.threads.iter().find(|(known, _)| *known == key) {
            return *block;
        }
        let thread = thread_path(self.instructions, self.plan, target, selector, value);
        let block = thread.map(|thread| {
            let block = self.body.core.builder.create_block();
            self.pending.push((block, thread));
            block
        });
        self.threads.push((key, block));
        block
    }

    /// Emit the threaded copy `thread` into `block`.
    /// Emit the latch at `pc`; at an unrolled loop's latch the back edge enters the first copy
    /// of the body, queued with the rest (`pending_copies`).
    fn latch(&mut self, function: &Function, pc: usize, instruction: WideInstruction) -> bool {
        let Some(unrolled) = self.unrolled[pc] else {
            return self.instruction(function, pc, instruction);
        };
        let copies = (1..unrolled.factor)
            .map(|_| self.body.core.builder.create_block())
            .collect::<Vec<_>>();
        for (index, &copy) in copies.iter().enumerate() {
            let next = copies.get(index + 1).copied();
            self.pending_copies.push((copy, unrolled, pc, next));
        }
        self.backedge = Some((unrolled.header, copies.first().copied()));
        let open = self.instruction(function, pc, instruction);
        self.backedge = None;
        open
    }

    /// Emit a copy of the body of the unrolled loop with latch `latch` into `block`: branches
    /// out of the loop leave through the original blocks, and the back edge enters `next`, or
    /// the header from the last copy.
    fn emit_unrolled_copy(
        &mut self,
        function: &Function,
        block: ir::Block,
        unrolled: UnrolledLoop,
        latch: usize,
        next: Option<ir::Block>,
    ) {
        self.body.core.builder.switch_to_block(block);
        self.known.clear();
        for pc in unrolled.header..latch {
            let instruction = self.instructions[pc];
            if instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump {
                let continuation = self.body.core.builder.create_block();
                self.redirect = Some((pc + 1, continuation));
                self.instruction(function, pc, instruction);
                self.redirect = None;
                self.body.core.builder.switch_to_block(continuation);
                self.known.clear();
            } else {
                self.instruction(function, pc, instruction);
            }
        }
        self.backedge = Some((unrolled.header, next));
        self.instruction(function, latch, self.instructions[latch]);
        self.backedge = None;
    }

    fn emit_thread(&mut self, function: &Function, block: ir::Block, thread: &Thread) {
        self.body.core.builder.switch_to_block(block);
        self.interrupt_safepoint();
        for (index, &pc) in thread.path.iter().enumerate() {
            let instruction = self.instructions[pc];
            match instruction.op().control_flow_layout() {
                // A dispatch copy ends in its table, run for real.
                ControlFlowLayout::BranchTable if matches!(thread.end, ThreadEnd::Dispatch) => {
                    self.instruction(function, pc, instruction);
                }
                // The path continues at the jump's target, or the table's
                // case: nothing to emit.
                ControlFlowLayout::Jump | ControlFlowLayout::BranchTable => {}
                ControlFlowLayout::ConditionalJump => {
                    if decided_branch(instruction, thread.selector, thread.value).is_some() {
                        continue;
                    }
                    let next = thread.path[index + 1];
                    let continuation = self.body.core.builder.create_block();
                    self.redirect = Some((next, continuation));
                    self.instruction(function, pc, instruction);
                    self.redirect = None;
                    self.body.core.builder.switch_to_block(continuation);
                }
                _ => {
                    self.instruction(function, pc, instruction);
                }
            }
        }
        let (ThreadEnd::Case(target) | ThreadEnd::Exit(target)) = thread.end else {
            return;
        };
        let target = self.block(target);
        self.goto(target, &[]);
    }

    /// Emit the instruction at `pc`; returns whether control falls through.
    fn instruction(
        &mut self,
        function: &Function,
        pc: usize,
        instruction: WideInstruction,
    ) -> bool {
        let step = Step::of(instruction.op(), instruction.imm())
            .expect("planned instructions have a denotation");
        let plan = self.plan;
        self.roots = &plan.references[pc];
        self.pc = pc;
        quench_runtime::staged::wasm_step::dispatch(&mut UjitTier { emitter: self, function }, pc, instruction, step)
    }
}

/// The u-jit as a Wasm tier (`staged::wasm_step::WasmTier`): each family emits its staged
/// rules over the compiled frame and answers whether control falls through.
struct UjitTier<'t, 'e, 'f> {
    emitter: &'t mut Emitter<'e, 'f>,
    function: &'t Function,
}

impl quench_runtime::staged::wasm_step::WasmTier for UjitTier<'_, '_, '_> {
    type Out = bool;

    fn numeric(&mut self, pc: usize, instruction: WideInstruction, operation: quench_runtime::staged::wasm_operation::WasmOperation) -> bool {
        self.emitter.operation(pc, instruction, operation)
    }

    fn instance(&mut self, pc: usize, instruction: WideInstruction, step: quench_runtime::staged::wasm_instance::Instance) -> bool {
        self.emitter.instance(pc, instruction, step);
        true
    }

    fn memory(&mut self, _pc: usize, instruction: WideInstruction, step: quench_runtime::staged::wasm_memory::Memory) -> bool {
        self.emitter.memory(instruction, step);
        true
    }

    fn flow(&mut self, pc: usize, instruction: WideInstruction, flow: quench_runtime::staged::wasm_flow::Flow) -> bool {
        self.emitter.flow(self.function, pc, instruction, flow)
    }

    fn call(&mut self, pc: usize, instruction: WideInstruction, call: quench_runtime::staged::wasm_call::Call) -> bool {
        self.emitter.call(pc, instruction, call);
        // A tail call returns from the function with its result: nothing follows it.
        !wasm_call_stage::is_tail_call(instruction)
    }

    fn table(&mut self, pc: usize, instruction: WideInstruction, op: quench_runtime::staged::wasm_table_ops::TableOp) -> bool {
        self.emitter.table_op(pc, instruction, op);
        true
    }

    fn simd(&mut self, pc: usize, instruction: WideInstruction, step: quench_runtime::staged::wasm_simd::Simd) -> bool {
        self.emitter.simd(pc, instruction, step);
        true
    }

    fn gc(&mut self, pc: usize, instruction: WideInstruction, op: quench_runtime::staged::wasm_gc::GcOp) -> bool {
        self.emitter.gc(pc, instruction, op);
        true
    }

    fn exception(&mut self, pc: usize, instruction: WideInstruction, op: quench_runtime::staged::wasm_exception::Exception) -> bool {
        self.emitter.exception_op(pc, instruction, op)
    }

    fn atomic(&mut self, _pc: usize, _instruction: WideInstruction, _op: quench_runtime::staged::wasm_atomic::Atomic) -> bool {
        unreachable!("the analysis rejects atomic steps")
    }
}

/// The machine code of a built function and its calls to other compiled functions; `None`
/// when Cranelift refuses it or its code needs a relocation other than such a call. It reads
/// only the function and the target, so any thread may run it.
fn compile_built(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    func: ir::Function,
) -> Option<(Vec<u8>, Vec<Call>)> {
    quench_runtime::vm::jit_backend::compile_function(func, isa, None, |compiled, function| {
        let code = compiled.code_buffer().to_vec();
        let names = function.params.user_named_funcs();
        let calls = compiled.buffer
            .relocs()
            .iter()
            .map(|relocation| {
                let cranelift_codegen::RelocTarget::ExternalName(ir::ExternalName::User(
                    name,
                )) = &relocation.target
                else {
                    return None;
                };
                let name = names.get(*name)?;
                (name.namespace == FUNCTION_NAMESPACE).then_some(Call {
                    offset: relocation.offset as usize,
                    kind: relocation.kind,
                    addend: relocation.addend,
                    callee: name.index as usize,
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(NativeCompileRejection::Relocations)?;
        Ok((code, calls))
    })
    .ok()
}

/// Built functions queued for each compile worker: one compiling and one ready keeps every
/// worker busy while the building thread stays ahead, and bounds the Cranelift functions
/// alive at once (each holds its whole body's IR) to a few per worker.
const BUILT_FUNCTIONS_QUEUED_PER_WORKER: usize = 2;

/// A function's compiled body with its calls, and its compiled entry (`build_entry`).
type CompiledBody = (Vec<u8>, Vec<Call>, Vec<u8>);

/// The compile result of the function at an index, with its instruction count, which leaves
/// the in-flight bound when the result arrives.
type CompiledBuilt = (usize, usize, Option<CompiledBody>);

/// A built function's body and entry, compiled (`compile_built`, `compile_entry`).
fn compile_body_and_entry(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    func: ir::Function,
    entry: ir::Function,
) -> Option<CompiledBody> {
    let (code, calls) = compile_built(isa, func)?;
    Some((code, calls, compile_entry(isa, entry)?))
}

/// Each of `count` functions' compiled body, by index, from the built functions `build`
/// yields in any order: the building stays on this thread, which owns the VM, and the
/// compiling, which needs only the function and the target, runs on one worker per
/// available CPU, holding at most `policy`'s `WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT`. `None`
/// when any function fails to build or compile, as the sequential artifact fails.
fn compile_built_functions(
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    count: usize,
    policy: quench_runtime::MemoryPolicy,
    build: impl Iterator<Item = Option<(usize, ir::Function, ir::Function)>>,
) -> Option<Vec<Option<CompiledBody>>> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(count);
    let mut bodies: Vec<Option<CompiledBody>> = std::iter::repeat_with(|| None).take(count).collect();
    if workers <= 1 {
        for built in build {
            let (index, func, entry) = built?;
            bodies[index] = Some(compile_body_and_entry(isa, func, entry)?);
        }
        return Some(bodies);
    }
    let bound = WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT[policy as usize];
    let failed = std::sync::atomic::AtomicBool::new(false);
    let (work, queue) =
        std::sync::mpsc::sync_channel::<(usize, ir::Function, ir::Function)>(workers * BUILT_FUNCTIONS_QUEUED_PER_WORKER);
    let queue = std::sync::Mutex::new(queue);
    let (done, results) = std::sync::mpsc::channel::<CompiledBuilt>();
    let mut place = |(index, _, body): CompiledBuilt| bodies[index] = body;
    let built_all = std::thread::scope(|scope| {
        for _ in 0..workers {
            let (queue, done, failed) = (&queue, done.clone(), &failed);
            scope.spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|queue| queue.recv().ok());
                    let Some((index, func, entry)) = next else { break };
                    let instructions = func.dfg.num_insts();
                    let body = if failed.load(std::sync::atomic::Ordering::Relaxed) {
                        None
                    } else {
                        compile_body_and_entry(isa, func, entry)
                    };
                    if body.is_none() {
                        failed.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    let _ = done.send((index, instructions, body));
                }
            });
        }
        drop(done);
        let mut built_all = true;
        // The instructions and the count of the functions sent and not yet compiled.
        let (mut in_flight, mut pending) = (0, 0);
        for built in build {
            let Some((index, func, entry)) = built else {
                built_all = false;
                break;
            };
            let instructions = func.dfg.num_insts();
            while pending > 0 && bound.is_some_and(|bound| in_flight + instructions > bound) {
                let Ok(result) = results.recv() else { break };
                in_flight -= result.1;
                pending -= 1;
                place(result);
            }
            if failed.load(std::sync::atomic::Ordering::Relaxed) || work.send((index, func, entry)).is_err() {
                break;
            }
            in_flight += instructions;
            pending += 1;
        }
        drop(work);
        built_all
    });
    if !built_all || failed.into_inner() {
        return None;
    }
    results.into_iter().for_each(place);
    Some(bodies)
}

impl<'a> Analysis<'a> {
    /// The Cranelift function of `index` with `plan`, ready for `compile_built`; `None` when
    /// a construct has no lowering.
    fn build_function(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        plans: &[Option<Plan>],
        guarded: bool,
        fixed_base: Option<i32>,
        reference_site_ids: &'a [(usize, usize)],
        diagnostics_enabled: bool,
    ) -> Option<ir::Function> {
        let function = &self.code.function(index);
        let call_conv = isa.default_call_conv();
        let memory = MemoryArguments::of(fixed_base, guarded);
        let mut func = ir::Function::with_name_signature(
            UserFuncName::User(function_name(index)),
            internal_signature(memory, &plan.params, plan.result, BODY_CALL_CONV),
        );
        use quench_runtime::staged::machine::{Lane, Relation};
        let mut builder_context = quench_runtime::vm::jit_backend::builder::take_context();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let context = parameters[0];
        let mut passed = parameters[1..].iter().copied();
        // A constant base folds into each access's addressing mode and
        // frees the register a parameter would occupy.
        let base = match fixed_base {
            Some(fixed) => object_slots::emit(&mut builder).raw_const(quench_runtime::staged::machine::Lane::I64, i64::from(fixed)),
            None => passed.next().expect("a passed base"),
        };
        let length = memory
            .length
            .then(|| passed.next().expect("a passed length"));
        let grows = index >= self.grows_memory.len() || self.grows_memory.contains(index);
        let wasm_parameters = &parameters[1 + memory.count()..];
        let regions = Region::ALL.map(|region| {
            builder.func.dfg.alias_regions.insert(ir::AliasRegionData {
                user_id: region as u32,
                description: format!("{region:?}").into(),
            })
        });
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| function.widened(*packed))
            .collect();
        let mut selectors: Vec<u16> = instructions
            .iter()
            .enumerate()
            .filter(|(pc, instruction)| {
                plan.reachable(*pc) && matches!(
                    Step::of(instruction.op(), instruction.imm()),
                    Some(Step::Flow(Flow::Table))
                )
            })
            .map(|(_, instruction)| instruction.a())
            .collect();
        selectors.sort_unstable();
        selectors.dedup();
        let mut emitter = Emitter {
            guarded,
            regions,
            analysis: self,
            body: Body::new(builder, context, poll_address(&self.vm.execution_interrupt), Int32Extension::Zero),
            plan,
            blocks: vec![None; function.code.len()],
            reference_site_ids,
            diagnostics_enabled,
            base,
            length,
            grows,
            memory,
            trap_blocks: [None; TRAPS.len()],
            callees: vec![None; plans.len()],
            callout_signature: None,
            plans,
            call_conv,
            instructions: &instructions,
            selectors,
            known: Vec::new(),
            threads: Vec::new(),
            pending: Vec::new(),
            redirect: None,
            threading: true,
            unrolled: if self.unrolls {
                unrollable_loops(&instructions, plan)
            } else {
                vec![None; instructions.len()]
            },
            backedge: None,
            pending_copies: Vec::new(),
            vectors: Vec::new(),
            simd_words: None,
            indirect: None,
            checked_row: None,
            roots: &[],
            pc: 0,
        };
        // Registers start at i32 zero, as an activation's register file does, where an entry may
        // read them before writing (`entry_fill`), and the locals. Any other register is written
        // before a read on every path, so it needs no zero.
        let zeroed = crate::dispatch_fast::entry_fill(self.code, function);
        for register in (0..zeroed).chain(function.registers..function.registers.saturating_add(function.locals)) {
            emitter.write_constant(register, Kind::I32, 0);
            emitter.write_constant(register, Kind::I64, 0);
        }
        let parameter_base = function.parameter_registers.expect("planned");
        for (offset, ty) in plan.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            emitter.write(
                parameter_base + offset as u16,
                kind,
                wasm_parameters[offset],
            );
        }
        // Each activation that calls checks the stack it may still use and polls for an
        // interrupt: unbounded depth and unbounded time both need a call (or a back edge, which
        // polls itself). A leaf (`is_leaf`) runs bounded on one small frame, under the margin its
        // caller's check left, so it enters with neither.
        let leaf = is_leaf(&instructions, plan, function.registers);
        if !leaf {
            let stack_pointer = emitter.stack_pointer();
            let limit = emitter.raw_load(Lane::I64, invariant(), context, CONTEXT_STACK_LIMIT);
            let exhausted = emitter.raw_compare(Relation::UnsignedLess, stack_pointer, limit);
            emitter.trap_if(exhausted, quench_runtime::WasmTrap::CallStackExhausted);
        }
        emitter.known.clear();
        emitter.jump(0, !leaf);
        // Every jump target starts a block, created before emission so a
        // backward jump finds its target's block already placed.
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                continue;
            }
            let targets = match instruction.op().control_flow_layout() {
                ControlFlowLayout::BranchTable => {
                    quench_runtime::bytecode::branch_table_entries(pc, instruction)
                        .map(|entry| instructions[entry].jump_target() as usize)
                        .collect()
                }
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    vec![instruction.jump_target() as usize, pc + 1]
                }
                _ => vec![],
            };
            for target in targets {
                if target < instructions.len() && plan.reachable(target) {
                    emitter.block(target);
                }
            }
            if let Some(catch) = plan.catches[pc] { emitter.block(catch.target); }
        }
        let mut open = false;
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                open = false;
                continue;
            }
            let is_leader = emitter.blocks[pc].is_some() || !open;
            if is_leader {
                emitter.known.clear();
                let block = emitter.block(pc);
                if open {
                    emitter.goto(block, &[]);
                }
                emitter.body.core.builder.switch_to_block(block);
            }
            open = emitter.latch(function, pc, instruction);
            // Copies of an unrolled body follow its latch in the layout, so a loop that runs a few
            // iterations stays in one stretch of code.
            for (block, unrolled, latch, next) in std::mem::take(&mut emitter.pending_copies) {
                emitter.emit_unrolled_copy(function, block, unrolled, latch, next);
            }
            // A later jump may target the next instruction; give it a block
            // now so falling into it stays explicit.
            if open && emitter.blocks.get(pc + 1).is_some_and(Option::is_some) {
                let next = emitter.block(pc + 1);
                emitter.goto(next, &[]);
                open = false;
            }
        }
        if open {
            return None;
        }
        emitter.threading = false;
        for (block, thread) in std::mem::take(&mut emitter.pending) {
            emitter.emit_thread(function, block, &thread);
        }
        emitter.finish_traps();
        emitter.body.core.builder.seal_all_blocks();
        emitter.body.into_builder().finalize(isa.frontend_config());
        quench_runtime::vm::jit_backend::builder::keep_context(builder_context);
        #[cfg(test)]
        BUILT_BODIES.with(|built| built.borrow_mut().push((index, func.display().to_string())));
        Some(func)
    }

    /// The C entry of `index`: unpack arguments, call through the table and
    /// store the result.
    /// The native entry of function `index` with `plan`: the trampoline a VM call enters,
    /// which loads the arguments and calls the body through the table. Built here and
    /// compiled beside the body (`compile_built_functions`), as it reads nothing but these.
    fn build_entry(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        memory: MemoryArguments,
    ) -> ir::Function {
        use quench_runtime::staged::machine::{Lane, Unary};
        let call_conv = isa.default_call_conv();
        let signature = native_entry_signature(isa.pointer_type(), call_conv);
        let mut func = ir::Function::with_name_signature(
            UserFuncName::user(ENTRY_NAMESPACE, index as u32),
            signature,
        );
        let mut builder_context = quench_runtime::vm::jit_backend::builder::take_context();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let (block, parameters) = quench_runtime::vm::jit_backend::builder::enter(&mut builder);
        builder.seal_block(block);
        let (context, base, length, args, out) = (
            parameters[0],
            parameters[1],
            parameters[2],
            parameters[3],
            parameters[4],
        );
        let target_signature = builder.import_signature(internal_signature(
            memory,
            &plan.params,
            plan.result,
            BODY_CALL_CONV,
        ));
        let wide_context = builder.func.dfg.value_type(context) == types::I64;
        let mut stage = object_slots::emit(&mut builder);
        let context = if wide_context {
            context
        } else {
            stage.raw_unary(Unary::ZeroExtend(Lane::I64), context)
        };
        let mut call_args = vec![context];
        call_args.extend(memory.select(base, length));
        for (offset, ty) in plan.params.iter().enumerate() {
            let raw = stage.raw_load(Lane::I64, MemFlagsData::trusted(), args, (offset * 8) as i32);
            call_args.push(match Kind::of(*ty).expect("planned") {
                Kind::I32 => stage.raw_unary(Unary::Narrow(Lane::I32), raw),
                Kind::I64 | Kind::Reference => raw,
            });
        }
        let table = stage.raw_load(Lane::I64, MemFlagsData::trusted(), context, CONTEXT_TABLE);
        let target = stage.entry_target(crate::compiled::slow_paths::EngineEntry::WasmFunctionEntry, MemFlagsData::trusted(), table, (index * 8) as i32);
        let results = stage.call_signature(target_signature, target, &call_args);
        if let Some(ty) = plan.result {
            let value = match Kind::of(ty).expect("planned") {
                Kind::I32 => stage.raw_unary(Unary::ZeroExtend(Lane::I64), results[1]),
                Kind::I64 | Kind::Reference => results[1],
            };
            stage.raw_store(Lane::I64, MemFlagsData::trusted(), (value, false), out, 0);
        }
        let status = stage.raw_unary(Unary::Narrow(Lane::I32), results[0]);
        stage.leave(&[status]);
        builder.finalize(isa.frontend_config());
        quench_runtime::vm::jit_backend::builder::keep_context(builder_context);
        func
    }
}

/// The compiled code of an entry `build_entry` built, which calls only through the table.
fn compile_entry(isa: &dyn cranelift_codegen::isa::TargetIsa, func: ir::Function) -> Option<Vec<u8>> {
    let mut context = cranelift_codegen::Context::for_function(func);
    let compiled = context.compile(isa, &mut cranelift_codegen::control::ControlPlane::default()).ok()?;
    if !compiled.buffer.relocs().is_empty() {
        return None;
    }
    Some(compiled.code_buffer().to_vec())
}

/// A memory base an x86-64 addressing mode can carry as its signed 32-bit
/// displacement.
fn displacement_base(base: *mut u8) -> Option<i32> {
    i32::try_from(base as usize).ok()
}

/// The guard reservation of a direct memory binding.
fn guarded_memory(vm: &Vm, binding: Value) -> Option<(*mut u8, usize)> {
    crate::dispatch_fast::direct_memory_and_guard(vm, binding)?.1
}

thread_local! {
    /// The context of the compiled call running on this thread, which a
    /// guard fault resumes from.
    static ACTIVE_CONTEXT: std::cell::Cell<*mut CallContext> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Enters compiled code: (context, base, length, arguments, result, entry)
/// -> status. It saves the caller's callee-saved registers, records its
/// stack pointer in the context, and calls `entry`; a guard fault resumes at
/// its landing, which returns the out-of-bounds status through the same
/// epilogue.
type Trampoline =
    unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64, WasmEntry) -> u32;

/// The trampoline and its landing, once the fault handler is installed.
struct GuardSupport {
    _code: Option<CodeMemory>,
    trampoline: Trampoline,
    #[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
    landing: usize,
    /// Where a fault on the poll page resumes: the terminated status.
    #[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
    termination: usize,
}

/// The host ABI can recover owned guard/poll faults through a native entry.
const FAULT_POLL_SUPPORTED: bool = cfg!(any(
    all(target_arch = "x86_64", target_os = "linux"),
    all(target_arch = "aarch64", target_os = "linux"),
    all(target_arch = "aarch64", target_os = "macos"),
));

// SAFETY: immutable executable code, shared by every thread.
unsafe impl Send for GuardSupport {}
unsafe impl Sync for GuardSupport {}

/// The status a guard fault on linear memory returns (`WasmTrap::OutOfBoundsMemory`), which the
/// trampoline's landing loads; set once before the fault handler is installed (`guard_support`).
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
static GUARD_OUT_OF_BOUNDS_STATUS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// The x86-64 System V guard trampoline, assembled by the toolchain (no hand-typed machine code):
// it saves the callee-saved registers, publishes the entry stack in the context, and calls the
// compiled entry in `r9`. A guard fault resumes at `quench_guard_landing` (the out-of-bounds status)
// or `quench_guard_termination` (the terminated status), which both return through the epilogue.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
std::arch::global_asm!(
    ".pushsection .text.quench_guard_trampoline,\"ax\",@progbits",
    ".p2align 4",
    ".globl quench_guard_trampoline",
    ".hidden quench_guard_trampoline",
    ".globl quench_guard_landing",
    ".hidden quench_guard_landing",
    ".globl quench_guard_termination",
    ".hidden quench_guard_termination",
    "quench_guard_trampoline:",
    "push rbp",
    "mov rbp, rsp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Keeps the call aligned.
    "sub rsp, 8",
    "mov qword ptr [rdi + {entry_stack}], rsp",
    "call r9",
    "2:",
    "add rsp, 8",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
    "quench_guard_landing:",
    "mov eax, dword ptr [rip + {out_of_bounds}]",
    "jmp 2b",
    "quench_guard_termination:",
    "mov eax, {terminated}",
    "jmp 2b",
    "int3",
    ".popsection",
    entry_stack = const std::mem::offset_of!(CallContext, head.entry_stack),
    out_of_bounds = sym GUARD_OUT_OF_BOUNDS_STATUS,
    terminated = const TERMINATED_STATUS as u32,
);

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
unsafe extern "C" {
    fn quench_guard_trampoline();
    fn quench_guard_landing();
    fn quench_guard_termination();
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
static PREVIOUS_SEGV: std::sync::OnceLock<libc::sigaction> = std::sync::OnceLock::new();

/// Publish forwarding before a handler can run; revoke partial installations.
#[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
fn install_guard_signals(signals: &[(i32, &std::sync::OnceLock<libc::sigaction>)],
    handler: usize, flags: i32) -> Option<()> {
    for (index, (signal, saved)) in signals.iter().enumerate() {
        // SAFETY: sigaction supplies a complete old disposition before we publish
        // ours. Installation owns this one-time process lifecycle transition.
        let installed = unsafe {
            let mut previous: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(*signal, std::ptr::null(), &mut previous) != 0
                || saved.set(previous).is_err() { false }
            else {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = handler;
                action.sa_flags = flags;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(*signal, &action, std::ptr::null_mut()) == 0
            }
        };
        if !installed {
            for (signal, saved) in &signals[..index] {
                // SAFETY: each earlier entry was installed above; restore its owner.
                if unsafe { libc::sigaction(*signal, saved.get().unwrap(), std::ptr::null_mut()) } != 0 {
                    std::process::abort();
                }
            }
            return None;
        }
    }
    Some(())
}

#[cfg(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
unsafe fn forward_guard_fault(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void,
    previous: &libc::sigaction) {
    // SAFETY: forwarding the original signal ABI to its published disposition.
    unsafe {
        match previous.sa_sigaction {
            libc::SIG_IGN => {},
            libc::SIG_DFL => {
                libc::sigaction(signal, previous, std::ptr::null_mut());
                libc::raise(signal);
            }
            handler if previous.sa_flags & libc::SA_SIGINFO != 0 => {
                std::mem::transmute::<usize, extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void)>(handler)(signal, info, context);
            }
            handler => { std::mem::transmute::<usize, extern "C" fn(i32)>(handler)(signal); }
        }
    }
}

/// The guard support of this host, installing the fault handler on first
/// use; `None` where guard faults cannot be resumed.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn guard_support() -> Option<&'static GuardSupport> {
    static SUPPORT: std::sync::OnceLock<Option<GuardSupport>> = std::sync::OnceLock::new();
    SUPPORT
        .get_or_init(|| {
            GUARD_OUT_OF_BOUNDS_STATUS.store(trap_status(quench_runtime::WasmTrap::OutOfBoundsMemory) as u32,
                std::sync::atomic::Ordering::Release);
            // SAFETY: the assembled trampoline implements `Trampoline` for this ABI.
            let trampoline = unsafe { std::mem::transmute::<unsafe extern "C" fn(), Trampoline>(quench_guard_trampoline) };
            let landing = quench_guard_landing as *const () as usize;
            let termination = quench_guard_termination as *const () as usize;
            install_guard_signals(&[(libc::SIGSEGV, &PREVIOUS_SEGV)],
                guard_fault as *const () as usize, libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_NODEFER)?;
            Some(GuardSupport {
                _code: None,
                trampoline,
                landing,
                termination,
            })
        })
        .as_ref()
}

#[cfg(any(all(target_arch = "aarch64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos")))]
fn guard_support() -> Option<&'static GuardSupport> { guard_aarch64::support() }

#[cfg(not(any(all(target_arch = "x86_64", target_os = "linux"), all(target_arch = "aarch64", target_os = "linux"), all(target_arch = "aarch64", target_os = "macos"))))]
fn guard_support() -> Option<&'static GuardSupport> {
    None
}

fn guard_trampoline() -> Option<Trampoline> {
    guard_support().map(|support| support.trampoline)
}

/// Where compiled code polls for termination: the VM's poll page when every entry runs
/// through the fault-resuming trampoline, else nowhere, and code checks the context's
/// interrupt flag.
fn poll_address(interrupt: &quench_runtime::execution_interrupt::ExecutionInterrupt) -> Option<i64> {
    guard_support()
        .and_then(|_| interrupt.poll_page())
        .map(|(start, _)| start as i64)
}

/// Runs compiled Wasm `entry` through the host trampoline, which can return from guard or
/// poll faults.
///
/// # Safety
/// `entry` is compiled code for the `WasmEntry` signature, and `context`, `arguments` and
/// `out` are what it reads and writes for the call's duration.
unsafe fn enter_wasm(
    context: &mut CallContext,
    base: u64,
    length: u64,
    arguments: *const u64,
    out: *mut u64,
    entry: WasmEntry,
) -> u32 {
    // SAFETY: as the caller promises; the trampoline saves the caller's registers and records
    // where a fault resumes.
    let context: *mut CallContext = context;
    unsafe { enter_compiled(context, |trampoline| match trampoline {
        Some(trampoline) => trampoline(context, base, length, arguments, out, entry),
        None => entry(context, base, length, arguments, out),
    }) }
}

/// The one way into compiled code, whatever its ABI (`enter_wasm`, `lane_body_abi::enter_body`):
/// `call` runs the entry through the fault-resuming trampoline where there is one, with
/// `context` the active one, and a fault's unreturned native frames give back the roots they
/// held (`NativeRoots`).
///
/// # Safety
/// `call` must enter compiled code of the ABI it types the trampoline with, under `context`.
pub(crate) unsafe fn enter_compiled<R>(context: *mut CallContext, call: impl FnOnce(Option<Trampoline>) -> R) -> R {
    // SAFETY: the caller's context outlives the call.
    let roots = unsafe { (*context).native_roots };
    // SAFETY: a context with roots is a Wasm entry's, whose VM outlives the call.
    let depth = (!roots.is_null()).then(|| unsafe { (*roots).depth() });
    let returned = match guard_trampoline() {
        Some(trampoline) => {
            let outer = ACTIVE_CONTEXT.with(|active| active.replace(context));
            let returned = call(Some(trampoline));
            ACTIVE_CONTEXT.with(|active| active.set(outer));
            returned
        }
        None => call(None),
    };
    if let Some(depth) = depth {
        // SAFETY: as above.
        unsafe { (*roots).set_depth(depth) };
    }
    returned
}

/// The poll page compiled code faults on to stop for `interrupt`: only where the guard
/// trampoline can resume the fault.
fn native_poll_page(interrupt: &quench_runtime::ExecutionInterrupt) -> Option<(usize, usize)> {
    guard_trampoline()?;
    interrupt.poll_page()
}

/// The stack limit a native entry starting at the current stack pointer runs under.
fn native_stack_limit() -> usize {
    let marker = 0u8;
    let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
    stack_pointer.saturating_sub(COMPILED_STACK_BUDGET)
}

/// Runs numeric body `entry` on `arguments`, the window whose first slots are the call's
/// Values, under `interrupt`; the body proves each a Number once.
fn run_numeric_entry(
    entry: BodyEntry,
    arguments: *const u64,
    interrupt: &quench_runtime::ExecutionInterrupt,
    poll_page: Option<(usize, usize)>,
) -> BodyReturn {
    let flag = poll_page.map_or_else(
        || std::ptr::from_ref(interrupt.attention_flag()),
        |(start, _)| start as *const std::sync::atomic::AtomicBool,
    );
    let mut context = CallContext::native_adapter(std::ptr::null_mut(), flag, poll_page);
    context.interrupt_owner = interrupt;
    let descriptor = entry_protocol::EntryDescriptor { kind: entry_protocol::EntryKind::Numeric, receiver: 0, captures: 0, registers: arguments, entry };
    // SAFETY: the region reads only its parameter slots, which `arguments` holds for the
    // call, and its ABI does not inspect the adapter payload.
    let returned = unsafe { entry_protocol::enter(&mut context, &descriptor) };
    entry_protocol::numeric_result(&descriptor, returned)
}

/// A fault inside the running compiled call's guard reservation resumes at
/// the trampoline's landing with the trampoline's saved stack; anything else
/// belongs to the previous handler.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
extern "C" fn guard_fault(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
    // SAFETY: supplied by the signal ABI.
    if unsafe { resume_guard_fault(info, context) } { return; }
    let Some(previous) = PREVIOUS_SEGV.get() else { std::process::abort(); };
    unsafe { forward_guard_fault(signal, info, context, previous); }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
unsafe fn resume_guard_fault(info: *mut libc::siginfo_t, context: *mut libc::c_void) -> bool {
    let active = ACTIVE_CONTEXT.with(std::cell::Cell::get);
    // SAFETY: the kernel passes valid signal information and context.
    unsafe {
        let address = (*info).si_addr() as usize;
        if !active.is_null()
            && let Some(support) = guard_support()
        {
            let landing = if (*active).head.guard_start <= address && address < (*active).head.guard_end {
                Some(support.landing)
            } else if (*active).head.poll_start <= address && address < (*active).head.poll_end {
                Some(support.termination)
            } else {
                None
            };
            if let Some(landing) = landing {
                let context = &mut *context.cast::<libc::ucontext_t>();
                context.uc_mcontext.gregs[libc::REG_RSP as usize] = (*active).head.entry_stack as i64;
                context.uc_mcontext.gregs[libc::REG_RIP as usize] = landing as i64;
                return true;
            }
        }
    }
    false
}

/// The name namespace of compiled function bodies; their C entries use
/// `ENTRY_NAMESPACE`.
const FUNCTION_NAMESPACE: u32 = 0;
const ENTRY_NAMESPACE: u32 = 1;

fn function_name(index: usize) -> ir::UserExternalName {
    ir::UserExternalName::new(FUNCTION_NAMESPACE, index as u32)
}

/// A direct call in a function body, linked once bodies are placed.
struct Call {
    /// The relocated field, from the start of the body.
    offset: usize,
    kind: cranelift_codegen::binemit::Reloc,
    addend: i64,
    callee: usize,
}

/// Point `call` in the body at `body` to the callee placed at `callee`; all
/// are offsets in one buffer, so the relative displacement is final.
fn link(bytes: &mut [u8], body: usize, call: &Call, callee: usize) -> Option<()> {
    use cranelift_codegen::binemit::Reloc;
    let site = body + call.offset;
    let displacement = callee as i64 - site as i64 + call.addend;
    match call.kind {
        Reloc::X86CallPCRel4 => {
            let field = i32::try_from(displacement).ok()?;
            bytes
                .get_mut(site..site + size_of::<i32>())?
                .copy_from_slice(&field.to_le_bytes());
        }
        Reloc::Arm64Call => {
            /// A BL immediate: a signed word displacement in the low bits.
            const IMMEDIATE_BITS: u32 = 26;
            const WORD_SHIFT: u32 = 2;
            let words = displacement >> WORD_SHIFT;
            let range = 1i64 << (IMMEDIATE_BITS - 1);
            if displacement & ((1 << WORD_SHIFT) - 1) != 0 || !(-range..range).contains(&words) {
                return None;
            }
            let field = bytes.get_mut(site..site + size_of::<u32>())?;
            let mask = (1u32 << IMMEDIATE_BITS) - 1;
            let instruction = u32::from_le_bytes(field.try_into().ok()?);
            let instruction = (instruction & !mask) | (words as u32 & mask);
            field.copy_from_slice(&instruction.to_le_bytes());
        }
        _ => return None,
    }
    Some(())
}

/// Code alignment for each compiled function.
const FUNCTION_ALIGNMENT: usize = 64;

#[quench_macros::ext(CompiledVm4)]
impl Vm {
    pub(crate) fn set_execution_mode(&mut self, mode: ExecutionMode) {
        self.execution_mode = mode;
        self.lane_tiers_run = None;
        #[cfg(feature = "jit")]
        crate::regexp_native::enable(mode != ExecutionMode::Interpreted, (self.programs.tiers().regexp_stitch_runs(), self.programs.tiers().regexp_native_runs));
        self.set_narrow_runs(mode == ExecutionMode::Interpreted);
    }

    /// Lets hot functions reach lane bodies by the tier gate (`Runtime` runs for speed).
    pub(crate) fn enable_lane_methods(&mut self) { self.programs.enable_js_lane_methods(); }

    /// Whether the process enrolled the lane-body tier (a mode selection, not realm state).
    #[cfg(test)]
    pub(crate) fn methods_enrolled_in_lane(&self) -> bool { admission::lane_bodies_enrolled(self) }

    /// Whether the runtime compiles Wasm modules whole at their first entry.
    pub(crate) fn wasm_compile_at_first_entry(&self) -> bool {
        self.programs.wasm_modules_compile_at_first_entry()
    }

    /// The unit of a Wasm program's module compile at `root` (`UnitKind::WasmModule`); `None` where
    /// the program is missing, not Wasm, or a root-only compile names no root.
    pub(crate) fn wasm_module_unit(&self, program: ProgramId, root: Option<usize>, root_only: bool) -> Option<admission::CompiledUnit> {
        let code = self.programs.get(program)?;
        let facts = admission::UnitFacts { wasm_program: code.kind == quench_runtime::bytecode::ProgramKind::Wasm, root_ok: !(root_only && root.is_none()), ..Default::default() };
        admission::specialize(admission::UnitRequest { kind: admission::UnitKind::WasmModule, facts }).compiled()
    }

    /// Compile each Wasm module whole at its first entry (`Runtime::compile_wasm_eagerly`), as
    /// `unit` (`UnitKind::WasmEager`) says.
    pub(crate) fn compile_wasm_eagerly(&mut self, unit: admission::CompiledUnit) {
        // An execution-only build (no `jit`) compiles no Wasm module at run time.
        #[cfg(feature = "jit")]
        {
            let _ = unit;
            self.programs.compile_wasm_modules_at_first_entry();
        }
        #[cfg(not(feature = "jit"))]
        let _ = unit;
    }

    pub(crate) fn native_execution_rejections(&self) -> u64 {
        self.native_execution_rejections
    }

    pub(crate) fn record_native_execution_rejection(&mut self) {
        self.native_execution_rejections = self.native_execution_rejections.saturating_add(1);
    }

    pub(crate) fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// A compiled tier's poll page is usable only when the shared entry can
    /// resume the protected-page fault back into the interpreter.
    pub(crate) fn compiled_poll_page(&self) -> Option<(usize, usize)> {
        native_poll_page(&self.execution_interrupt)
    }

    /// Whether any compiled tier may run (`admission::vm_may_run` of every tier), decided once per execution
    /// mode and memory policy rather than at every lane entry.
    #[inline]
    pub(crate) fn tiers_run_in_lanes(&mut self) -> bool {
        match self.lane_tiers_run {
            Some(run) => run,
            None => *self.lane_tiers_run.insert(admission::vm_may_run(self, admission::Tier::ANY)),
        }
    }

    /// Whether the entry paths claim `function`: a numeric candidate that pays for a native
    /// entry, or a body a lane body may take.
    pub(crate) fn js_tier_may_claim(
        &mut self,
        program: &ResidualProgram,
        id: ProgramId,
        function: u32,
    ) -> bool {
        // A function of a program with a loaded AOT image is claimed at its first entry, whatever its size and mode: its
        // image body is compiled already, so there is no compile to amortize, and an execution-only build has nothing else
        // to run it with.
        if self.images_run_in_lane() && self.programs.ahead_images(id, program).is_some() {
            return true;
        }
        // The Cranelift function tiers claim, and so do lane bodies, which `--b-jit` stitches from stencils (`UnitKind::Claim`).
        if admission::specialize(admission::UnitRequest { kind: admission::UnitKind::Claim, facts: admission::UnitFacts {
            cranelift_allowed: admission::vm_may_run(self, admission::Tier::CRANELIFT),
            base_lane_allowed: admission::lane_bodies_enrolled(self) && admission::vm_may_run(self, &[admission::Tier::Base]),
            wasm_program: program.kind == quench_runtime::bytecode::ProgramKind::Wasm, ..Default::default() } }).compiled().is_none() {
            return false;
        }
        let Some(facts) = self.programs.js_entry_facts(id, function) else {
            return false;
        };
        let state = self.programs.js_tier(id, function);
        // Lane bodies claim only bodies that can amortize an entry (`amortizes_entry`): a claim
        // costs a short call-free one its leaf calls. A function the numeric tier cannot take
        // is still theirs to claim.
        let lane_claims = admission::lane_bodies_enrolled(self)
            && program.function_get(function as usize).is_some_and(|body| body_may_amortize_in_lane(self.programs.tiers(), body));
        if quench_runtime::vm::tier_machine::js_tier_state(state.as_ref()) == quench_runtime::vm::tier_machine::TierState::Declined {
            return lane_claims;
        }
        let candidate = facts.candidate && program.function_get(function as usize).is_some_and(pays_native_entry);
        // Inlining may erase a short body's calls; a call-free candidate still has
        // to pay for native entry under the same cost policy.
        candidate || lane_claims
    }

    /// The JavaScript tier at the entry of a frameless lane activation of `function`, as
    /// `js_tier_frame_entry` is at a frame's: with the native result, the PC of the
    /// instruction the activation continues at, its register and the value to write there;
    /// else `None`, and the activation runs from its entry. `parameters` are the
    /// activation's parameter registers.
    pub(crate) fn js_tier_activation_entry(
        &mut self,
        program: &ResidualProgram,
        id: ProgramId,
        function: u32,
        parameters: &[Value],
    ) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
        #[cfg(feature = "jit")]
        { self.js_tier_activation_entry_compiled(program, id, function, parameters) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (program, id, function, parameters);
            None
        }
    }

    /// An execution-only build (no `jit`) compiles no function at an activation's entry.
    #[cfg(feature = "jit")]
    fn js_tier_activation_entry_compiled(
        &mut self,
        program: &ResidualProgram,
        id: ProgramId,
        function: u32,
        parameters: &[Value],
    ) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
        // A compiled body that reads only its parameters runs as a direct call does
        // (`js_jit_compiled_call`); the activation completes at its `Return` with the result.
        if let Some(value) = self.js_jit_compiled_call(program, (id, function), false, parameters) {
            let body = program.function_get(function as usize)?;
            let last_pc = body.code.len().checked_sub(1)?;
            let terminal = body.code.get(last_pc)?.as_wide();
            if meaning_compiled::is_return(terminal) {
                return Some((last_pc, terminal.register_a(), value));
            }
        }
        let entry = TierEntry {
            program: id,
            function,
            parameters: parameters.iter().copied().collect(),
            facts: self.programs.js_entry_facts(id, function)?,
        };
        let entered = self.js_jit_at_entry(program, &entry);
        if entered.is_none() {
            self.programs.record_js_jit_fallback_entry(id, function);
        }
        entered
    }

    /// The callee's depth-0 capture under the same environment binding as the lane.
    pub(crate) fn environment_capture(&mut self, env: Value, slot: u16) -> Option<Value> {
        // A frameless activation's function has no environment of its own
        // (`load_capture_walk_miss`).
        let (slots, root) = self.walked_capture_slots(env, false, 0)?;
        // SAFETY: as `Vm::walked_capture_slots`; nothing collects before the read.
        self.owned_capture(unsafe { &*slots }, root, slot)
    }

    /// Runs the body a native caller's call pushed (`push_in_loop_call`) to its completion on
    /// the native stack, so the caller continues past the call natively instead of stopping
    /// for the loop to run the callee and entering again after it; a `call`'s returning from
    /// the caller's frame returns its value. A callee of another program, or one past the
    /// native stack budget, stays the loop's push.
    #[cfg(feature = "native-execution")]
    fn complete_pushed_call(&mut self, program: &ResidualProgram, caller: usize, call: WideInstruction,
        step: Result<quench_runtime::vm::StepResult, JsError>) -> Result<quench_runtime::vm::StepResult, JsError> {
        // A construction yields its receiver unless the body returns an object, as the loop's
        // return of a pushed constructor does (`CallCompletion::Constructor`).
        let (destination, call_guard, constructed) = match step {
            Ok(quench_runtime::vm::StepResult::PushFrame { destination, call_guard, completion: quench_runtime::vm::CallCompletion::Value }) =>
                (destination, call_guard, None),
            Ok(quench_runtime::vm::StepResult::PushFrame { destination, call_guard, completion: quench_runtime::vm::CallCompletion::Constructor(this) }) =>
                (destination, call_guard, Some(this)),
            step => return step,
        };
        let callee = self.frames.len() - 1;
        let stack = (self.frames[callee].program == self.frames[caller].program)
            .then(quench_runtime::stack::StackGuard::enter_optional).flatten();
        let Some(_stack) = stack else {
            let completion = constructed.map_or(quench_runtime::vm::CallCompletion::Value, quench_runtime::vm::CallCompletion::Constructor);
            return Ok(quench_runtime::vm::StepResult::PushFrame { destination, call_guard, completion });
        };
        let result = self.run_pushed_body(program, callee);
        // A construction yields the activation's own receiver, which a derived constructor's
        // `super()` binds in the body.
        let outcome = self.complete_user_frame(result, constructed.is_some());
        drop(call_guard);
        self.finish_pushed_call(caller, destination, call, outcome?)
    }

    /// Runs the pushed body in `callee`: a linked body that returns completes the call without a
    /// loop; any other transition it made is the loop's to continue, as its own entry of the
    /// body would.
    #[cfg(feature = "native-execution")]
    fn run_pushed_body(&mut self, program: &ResidualProgram, callee: usize) -> Result<quench_runtime::vm::FrameOutcome, JsError> {
        let caller_registers = self.active_registers;
        self.active_registers = self.frames[callee].registers.as_mut_ptr();
        let result = self.run_frame_general(program, callee);
        self.active_registers = caller_registers;
        result
    }

    /// The completed body's value as the pushing call's result: a construction yields the
    /// body's object or else its receiver; a `call` returning from the caller's frame returns
    /// it, any other writes the call's destination register.
    #[cfg(feature = "native-execution")]
    fn finish_pushed_call(&mut self, caller: usize, destination: Register, call: WideInstruction,
        outcome: quench_runtime::vm::FrameOutcome) -> Result<quench_runtime::vm::StepResult, JsError> {
        let outcome = match outcome {
            quench_runtime::vm::FrameOutcome::ConstructComplete { value, this } =>
                quench_runtime::vm::FrameOutcome::Complete(Value::from_bits(quench_runtime::staged::call_construct::concrete_construct_value(value.to_bits(), this.to_bits(), self.is_object_like(value)))),
            outcome => outcome,
        };
        match outcome {
            quench_runtime::vm::FrameOutcome::Complete(value) if call.returns_from_frame() => Ok(quench_runtime::vm::StepResult::Return(value)),
            quench_runtime::vm::FrameOutcome::Complete(value) => {
                self.frames[caller].registers[usize::from(destination)] = value;
                Ok(quench_runtime::vm::StepResult::Continue)
            }
            _ => Err(JsError::validation("an in-loop body completed without its value".into())),
        }
    }

    /// Entry obligations for a selected no-frame callee, before its first effect.
    #[inline(always)]
    fn enter_guarded_call(&self) -> Option<quench_runtime::stack::GuestCallGuard> {
        let guard = quench_runtime::stack::GuestCallGuard::enter().ok()?;
        (!self.execution_interrupt.atomic_flag().load(std::sync::atomic::Ordering::Relaxed)).then_some(guard)
    }

    /// The JavaScript tier at the entry of `frame`, an initialized frame at PC zero, made
    /// wherever a frame is entered: the general loop, and the lane's in-place frame calls
    /// (`frame_call_in_lane`). With the native result written to its return register, the PC
    /// of the `Return` that completes the frame; else `None`, the entry counted as an
    /// interpreter fallback, and the untouched frame runs from its entry.
    pub(crate) fn js_tier_frame_entry(&mut self, program: &ResidualProgram, frame: usize) -> Option<usize> {
        #[cfg(feature = "jit")]
        { self.js_tier_frame_entry_compiled(program, frame) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (program, frame);
            None
        }
    }

    /// An execution-only build (no `jit`) compiles no function at an entry.
    #[cfg(feature = "jit")]
    fn js_tier_frame_entry_compiled(&mut self, program: &ResidualProgram, frame: usize) -> Option<usize> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        if self.frames[frame].js_jit_skip_entry_once {
            self.frames[frame].js_jit_skip_entry_once = false;
            return None;
        }
        let function = self.frames[frame].function;
        let program_id = self.frames[frame].program;
        // Most entries are of functions no tier path can take.
        let facts = self.programs.js_entry_facts(program_id, function)?;
        let at_entry = self.frames[frame].pc == 0;
        if !facts.candidate && at_entry {
            self.programs.observe_unclaimed_entry(program, program_id, function, &self.frames[frame].registers);
        }
        let entry = self.frames.get(frame).filter(|_| facts.candidate && at_entry).and_then(|record| {
            let params = program.function_get(record.function as usize)?.params;
            Some(TierEntry {
                program: record.program,
                function: record.function,
                parameters: record.registers.iter().copied().take(usize::from(params)).collect(),
                facts: Rc::clone(&facts),
            })
        });
        let compiled_entry = entry.as_ref().and_then(|entry| self.js_jit_at_entry(program, entry));
        if let Some((return_pc, return_register, value)) = compiled_entry {
            // The frame's own register file: a lane frame call reaches here before
            // `active_registers` names the frame (`enter_script_frame_in_lane`).
            self.frames[frame].registers[usize::from(return_register)] = value;
            return Some(return_pc);
        }
        self.programs.record_js_jit_fallback_entry(program_id, function);
        None
    }

    /// Try the per-function JavaScript tier at an already-initialized frame's
    /// entry. A guard miss leaves the frame untouched so the interpreter can
    /// run from the same bytecode PC.
    pub(crate) fn js_jit_at_entry(
        &mut self,
        program: &ResidualProgram,
        entry: &TierEntry,
    ) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
        // The Cranelift tiers' gate (`UnitKind::Entry`) comes before any entry is counted.
        let Some(unit) = admission::specialize(admission::UnitRequest { kind: admission::UnitKind::Entry,
            facts: admission::UnitFacts { cranelift_allowed: admission::vm_may_run(self, admission::Tier::CRANELIFT), ..Default::default() } }).compiled() else { return None };
        if program.kind == quench_runtime::bytecode::ProgramKind::Wasm {
            return None;
        }
        let (program_id, function_id) = (entry.program, entry.function);
        let function = program.function_get(function_id as usize)?;
        let facts = &*entry.facts;
        #[cfg(test)]
        if !self.programs.js_entry_specializations_enabled() { return None; }
        // Most entries are of functions no entry path can claim.
        if !facts.candidate {
            self.programs.observe_unclaimed_entry(program, program_id, function_id, &entry.parameters);
            return None;
        }
        let tier = self.programs.js_tier(program_id, function_id);
        if matches!(
            tier.as_ref(),
            Some(quench_runtime::vm::program_store::JsTierState::Unsupported | quench_runtime::vm::program_store::JsTierState::Rejected { .. })
        ) {
            self.programs.observe_unclaimed_entry(program, program_id, function_id, &entry.parameters);
            return None;
        }
        let params = usize::from(function.params);
        let arguments = entry.parameters.get(..params)?;
        let numeric = arguments.iter().all(|value| value.as_number().is_some());
        self.programs.record_js_jit_entry(program_id, function_id, numeric, function.params);
        let compiled = match tier {
            Some(quench_runtime::vm::program_store::JsTierState::Compiled(compiled)) => {
                if !numeric {
                    return None;
                }
                compiled
            }
            Some(
                quench_runtime::vm::program_store::JsTierState::Unsupported
                | quench_runtime::vm::program_store::JsTierState::Rejected { .. },
            ) => return None,
            Some(quench_runtime::vm::program_store::JsTierState::Hot { .. })
            | Some(quench_runtime::vm::program_store::JsTierState::PromotionPending)
            | None => {
                if !self.programs.note_js_function_entry(program_id, function_id, numeric, numeric) || !numeric {
                    return None;
                }
                self.js_tier_compile(unit, program_id, function_id)?
            }
        };
        if compiled.params() != params {
            return None;
        }
        self.programs.record_js_jit_native_entry(program_id, function_id);
        let poll_page = self.compiled_poll_page();
        let value = compiled.call(arguments, &self.execution_interrupt, poll_page)?;
        let last_pc = function.code.len().checked_sub(1)?;
        let terminal = function.code.get(last_pc)?.as_wide();
        (meaning_compiled::is_return(terminal)).then_some((last_pc, terminal.register_a(), value))
    }

    /// Compiles and installs `function`'s numeric body (`js_compiled::compile`); `None` where
    /// the specializer declines it or the tier does not install it.
    fn js_tier_compile(&mut self, unit: admission::CompiledUnit, program_id: ProgramId, function_id: u32) -> Option<Rc<CompiledJsFunction>> {
        if !cfg!(feature = "jit") {
            return None; // no run-time compile without the JIT: only image bodies run natively
        }
        let residual = self.programs.get(program_id)?;
        let key = (program_id, function_id);
        let pending = self.js_tier_pending.remove(&key).or_else(|| crate::js_compiled::start(&residual, function_id, unit));
        let image = match pending.map(|mut pending| (pending.poll(), pending)) {
            Some((std::task::Poll::Pending, pending)) => {
                self.js_tier_pending.insert(key, pending);
                return None;
            }
            Some((std::task::Poll::Ready(image), _)) => image.flatten(),
            None => None,
        };
        self.host.before_native_compilation();
        let peak_before = quench_runtime::trace::Channel::JitPeak.on().then(process_peak_rss_kib);
        let artifact = image.and_then(|image| image.load());
        if let Some(before) = peak_before {
            let after = process_peak_rss_kib();
            eprintln!(
                "QUENCH_JS_JIT_PEAK function={function_id} succeeded={} peak_before_kib={before} peak_after_kib={after}",
                artifact.is_some(),
            );
        }
        self.host.after_native_compilation();
        let artifact_failed = artifact.is_none();
        let compiled = artifact.and_then(CompiledJsFunction::from_artifact).map(Rc::new);
        let compile_rejection = Self::js_tier_compile_rejection(&residual, function_id, artifact_failed, compiled.is_none());
        let installed = self.programs.install_js_tier(program_id, function_id, compiled.clone());
        if admission::diagnosing(self) {
            let rejection = if !installed && compiled.is_some() { Some("shared-code-cache-budget-exhausted") } else { compile_rejection };
            self.record_js_tier_compile(program_id, function_id, compiled.as_deref().filter(|_| installed), rejection);
        }
        installed.then_some(())?;
        compiled
    }

    /// Why a compile installed nothing: the specializer declined the candidate
    /// (`artifact_failed`), or executable memory could not be allocated for its artifact.
    fn js_tier_compile_rejection(residual: &ResidualProgram, function_id: u32, artifact_failed: bool,
        unallocated: bool) -> Option<&'static str> {
        if artifact_failed {
            Some(crate::js_compiled::candidate_rejection_reason(residual, function_id))
        } else {
            unallocated.then_some("executable-memory-allocation-failed")
        }
    }

    /// Records a compile for the diagnostics: the installed code's size, and why nothing was
    /// installed.
    fn record_js_tier_compile(&mut self, program_id: ProgramId, function_id: u32,
        installed: Option<&CompiledJsFunction>, rejection: Option<&'static str>) {
        self.programs.record_js_jit_compile(
            program_id,
            function_id,
            installed.map_or(0, CompiledJsFunction::mapped_bytes),
            installed.is_some(),
            rejection,
        );
    }

    /// The JavaScript tier's direct entry of a plain call of a same-program user function,
    /// before any callee frame is pushed: the general loop's in-loop and known calls and the
    /// lane's frame calls (`frame_call_in_lane`) make it. Only an already-promoted function
    /// enters; its native subset is pure and has a guarded Number ABI. A call that returns
    /// its callee's completion (`returns`) completes its caller, so it and every guard miss
    /// stay on the ordinary frame path.
    pub(crate) fn js_jit_compiled_call(
        &mut self,
        program: &ResidualProgram,
        (program_id, function_id): (ProgramId, u32),
        returns: bool,
        arguments: &[Value],
    ) -> Option<Value> {
        if returns
            || !admission::vm_may_run(self, admission::Tier::ANY)
            || program_id != self.active_program
            || program.kind == quench_runtime::bytecode::ProgramKind::Wasm
        {
            return None;
        }
        let diagnostics = admission::diagnosing(self);
        let (programs, interrupt) = (&mut self.programs, &self.execution_interrupt);
        let function = program.function_get(function_id as usize)?;
        let native_result = {
            let compiled = match programs.js_tier_ref(program_id, function_id)? {
                quench_runtime::vm::program_store::JsTierState::Compiled(compiled) => compiled.as_ref(),
                _ => return None,
            };
            let params = usize::from(function.params);
            if compiled.params() != params {
                return None;
            }
            direct_call(compiled, arguments, interrupt)
        };
        if diagnostics {
            programs.record_js_jit_entry(program_id, function_id, true, function.params);
            programs.record_js_jit_native_entry(program_id, function_id);
        }
        native_result
    }

    /// The direct run of `function`'s compiled body a frameless activation keeps
    /// (`LaneView::tier_direct`): where its body reads only its parameters
    /// (`CompiledJsFunction::direct_callable`) and ends in a `Return`, which its result
    /// completes.
    pub(crate) fn direct_tier_entry(
        &self,
        program: &ResidualProgram,
        id: ProgramId,
        function: u32,
    ) -> Option<DirectTierEntry> {
        let quench_runtime::vm::program_store::JsTierState::Compiled(compiled) = self.programs.js_tier_ref(id, function)? else {
            return None;
        };
        let body = program.function_get(function as usize)?;
        let params = usize::from(body.params);
        (compiled.params() == params).then_some(())?;
        let return_pc = body.code.len().checked_sub(1)?;
        let terminal = body.code.get(return_pc)?.as_wide();
        (meaning_compiled::is_return(terminal)).then_some(())?;
        Some(DirectTierEntry {
            compiled: Rc::downgrade(compiled),
            return_pc,
            return_register: terminal.register_a(),
            function: (id, function),
            params: body.params,
        })
    }

    /// Runs `entry` on a frameless activation's `parameters`, as `js_jit_compiled_call` runs
    /// the body: the `Return` to continue at, its register and the result; `None` where an
    /// argument is no Number the body's guards admit, or the tier retired the artifact.
    pub(crate) fn run_direct_tier_entry(
        &mut self,
        entry: &DirectTierEntry,
        parameters: &[Value],
    ) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
        let compiled = entry.compiled.upgrade()?;
        let value = direct_call(&compiled, parameters, &self.execution_interrupt)?;
        let (program, function) = entry.function;
        self.programs.record_js_jit_entry(program, function, true, entry.params);
        self.programs.record_js_jit_native_entry(program, function);
        Some((entry.return_pc, entry.return_register, value))
    }

    /// Compile every function of `program` that has a compiled denotation,
    /// with `env` as the instance whose bindings fix global types and memory.
    fn compile_program(&self, program: ProgramId, env: Value, unit: admission::CompiledUnit) -> Option<CompiledProgram> {
        self.compile_program_with_diagnostics(program, env, jit_diagnostics_enabled(), unit)
    }

    fn compile_program_with_diagnostics(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
        unit: admission::CompiledUnit,
    ) -> Option<CompiledProgram> {
        let root = jit_root_override().ok()?;
        self.compile_program_with_diagnostics_and_root(program, env, diagnostics_enabled, root, unit)
    }

    fn compile_program_with_diagnostics_and_root(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
        root: Option<usize>,
        unit: admission::CompiledUnit,
    ) -> Option<CompiledProgram> {
        self.compile_program_with_diagnostics_and_root_mode(
            program,
            env,
            diagnostics_enabled,
            root,
            false,
            unit,
        )
    }

    /// Compile one explicitly selected Wasm function without its static
    /// `CallKnown` closure. Unselected callees use the existing VM callout
    /// path. This is an opt-in diagnostic tier; normal root compilation keeps
    /// its current closure behavior.
    fn compile_program_with_diagnostics_and_root_mode(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
        root: Option<usize>,
        root_only: bool,
        unit: admission::CompiledUnit,
    ) -> Option<CompiledProgram> {
        // Without the JIT's routes (`jit_routes`) the program runs in the interpreter.
        (crate::jit_routes::jit()?.wasm)(self, program, env, diagnostics_enabled, root, root_only, unit)
    }

    /// `compile_program_with_diagnostics_and_root_mode`'s route, which `jit_routes` installs.
    fn compile_wasm_program(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
        root: Option<usize>,
        root_only: bool,
        unit: admission::CompiledUnit,
    ) -> Option<CompiledProgram> {
        let _ = unit;
        let code = self.programs.get(program)?;
        let compile_rejections =
            diagnostics_enabled.then(|| std::cell::RefCell::new(vec![None; code.functions.len()]));
        let count = code.functions.len();
        let allowed = if let Some(root) = root.filter(|_| root_only) {
            let mut selected = vec![false; count];
            let Some(function) = code.function_get(root) else {
                if diagnostics_enabled {
                    eprintln!("QUENCH_WASM_JIT_ROOT_REJECTION root={root} invalid-function={root}");
                }
                return None;
            };
            if function.code.len() > MAX_JIT_ROOT_BYTECODES_PER_FUNCTION {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} function={root} bytecodes={} limit={MAX_JIT_ROOT_BYTECODES_PER_FUNCTION}",
                        function.code.len()
                    );
                }
                return None;
            }
            selected[root] = true;
            selected
        } else if let Some(root) = root {
            let mut selected = vec![false; count];
            let mut pending = vec![root];
            while let Some(index) = pending.pop() {
                let Some(selected_function) = selected.get_mut(index) else {
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_ROOT_REJECTION root={root} invalid-function={index}"
                        );
                    }
                    return None;
                };
                if *selected_function {
                    continue;
                }
                let function = code.function_get(index)?;
                if function.code.len() > MAX_JIT_ROOT_BYTECODES_PER_FUNCTION {
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_ROOT_REJECTION root={root} function={index} bytecodes={} limit={MAX_JIT_ROOT_BYTECODES_PER_FUNCTION}",
                            function.code.len()
                        );
                    }
                    return None;
                }
                *selected_function = true;
                for packed in &function.code {
                    let instruction = function.widened(*packed);
                    if let Some(callee) = Step::of(instruction.op(), instruction.imm())
                        .and_then(|step| step.callee(instruction))
                    {
                        pending.push(callee);
                    }
                }
            }
            // A selective tier normally follows only statically known calls.
            // Before this promotion, however, the eager artifact has already
            // observed hot `call_indirect` targets. Add a bounded set of those
            // callees from call sites inside this root's static closure; the
            // generated call still keeps the dynamic resolver as its miss
            // path.
            if let Some(baseline) = self.programs.compiled(program).flatten() {
                let profiled_targets =
                    baseline.profiled_indirect_targets(self, program, env, &selected);
                for target in profiled_targets {
                    if target >= count || selected[target] {
                        continue;
                    }
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_PROFILED_INDIRECT_TARGET root={root} function={target}"
                        );
                    }
                    pending.push(target);
                }
                while let Some(index) = pending.pop() {
                    let Some(selected_function) = selected.get_mut(index) else {
                        if diagnostics_enabled {
                            eprintln!(
                                "QUENCH_WASM_JIT_ROOT_REJECTION root={root} invalid-function={index}"
                            );
                        }
                        return None;
                    };
                    if *selected_function {
                        continue;
                    }
                    let function = code.function_get(index)?;
                    if function.code.len() > MAX_JIT_ROOT_BYTECODES_PER_FUNCTION {
                        if diagnostics_enabled {
                            eprintln!(
                                "QUENCH_WASM_JIT_ROOT_REJECTION root={root} function={index} bytecodes={} limit={MAX_JIT_ROOT_BYTECODES_PER_FUNCTION}",
                                function.code.len()
                            );
                        }
                        return None;
                    }
                    *selected_function = true;
                    for packed in &function.code {
                        let instruction =
                            function.widened(*packed);
                        if let Some(callee) = Step::of(instruction.op(), instruction.imm())
                            .and_then(|step| step.callee(instruction))
                        {
                            pending.push(callee);
                        }
                    }
                }
            }
            let selected_count = selected.iter().filter(|selected| **selected).count();
            if selected_count > MAX_JIT_ROOT_FUNCTIONS {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} functions={selected_count} limit={MAX_JIT_ROOT_FUNCTIONS}"
                    );
                }
                return None;
            }
            selected
        } else {
            vec![true; count]
        };
        let mut analysis = Analysis {
            experimental_root_compilation: root.is_some(),
            root_only,
            compile_rejections: compile_rejections.as_ref(),
            ..Analysis::new(self, program, &code, env)
        };
        // The callable set is the greatest fixpoint: assume every function
        // compiles, drop those that cannot, until nothing changes.
        // Each callable function's full plan, which its compilation reads, and the
        // interfaces its callers read.
        let mut full_plans: Vec<Option<Plan>>;
        let mut plans: Vec<Option<Plan>>;
        let mut memory_slot;
        let mut guarded;
        loop {
            (full_plans, memory_slot) = analysis.settle_callable(&quench_runtime::bitset::BitSet::from_fn(count, |index| allowed[index]));
            plans = full_plans.iter().map(|plan| plan.as_ref().map(Plan::interface)).collect();
            // Accesses may rely on the guard when the memory has one and
            // this host resumes guard faults.
            let stable_memory = memory_slot.is_some_and(|slot| {
                self.heap_reads()
                    .environment_slot(env, slot as usize)
                    .and_then(|binding| guarded_memory(self, binding))
                    .is_some()
            });
            guarded = guard_trampoline().is_some() && stable_memory;
            // Callouts need a stable base, not fault-based bounds checks.
            // Hosts without a fault trampoline retain explicit bounds checks.
            if !analysis.callouts || stable_memory || memory_slot.is_none() {
                break;
            }
            analysis.callouts = false;
        }
        let memory_shared = memory_slot.is_some_and(|slot| {
            self.heap_reads()
                .environment_slot(env, slot as usize)
                .and_then(|binding| self.heap_reads().get(binding))
                .is_some_and(|cell| matches!(cell, Cell::WasmMemory { bytes, .. } if matches!(**bytes, quench_runtime::wasm::memory::MemoryStorage::Shared(_))))
        });
        analysis.grows_memory = wasm_growth::growing_functions(&code, count, memory_shared);
        if plans.iter().all(Option::is_none) {
            return None;
        }
        if root.is_some_and(|root| plans.get(root).is_none_or(Option::is_none)) {
            return None;
        }
        let fixed_base = guarded
            .then(|| {
                let binding = self.heap_reads().environment_slot(env, memory_slot? as usize)?;
                displacement_base(guarded_memory(self, binding)?.0)
            })
            .flatten();
        // Under `--b-jit` no Cranelift code is generated, so no ISA is created.
        let base_only = admission::wasm_module_routes_to_stencils();
        let isa = if base_only {
            None
        } else {
            let opt_level = jit_root_opt_level(root, root_only).ok()?;
            if diagnostics_enabled {
                eprintln!("QUENCH_WASM_JIT_OPT_LEVEL {opt_level}");
            }
            Some(quench_runtime::vm::jit_backend::target_isa_at(opt_level)?)
        };
        let mut reference_site_ids = vec![Vec::new(); count];
        let mut reference_call_sites = Vec::new();
        let mut reference_cache_sites = Vec::new();
        let mut reference_cache_count = 0usize;
        // One pass over each callable function's full plan finds what every body's compilation
        // reads first: its indirect reference sites.
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let full_plan = full_plans[index].as_ref()?;
            let function = &code.function(index);
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = function.widened(*packed);
                if Step::of(instruction.op(), instruction.imm()) != Some(Step::Call(WasmCall::Reference)) {
                    continue;
                }
                let Slot::IndirectReference { table, .. } = full_plan.operand(pc) else {
                    continue;
                };
                let site_id = reference_cache_count;
                reference_cache_count += 1;
                reference_site_ids[index].push((pc, site_id));
                reference_cache_sites.push((index, pc, table));
                if diagnostics_enabled {
                    reference_call_sites.push(ReferenceCallSiteDiagnostics {
                        function: index,
                        pc,
                        counts: std::cell::Cell::new(ReferenceCallSiteCounts::default()),
                        targets: std::cell::RefCell::new(Vec::new()),
                    });
                }
            }
        }
        let mut bytes = Vec::new();
        let mut offsets = vec![None; count];
        let mut spans = Vec::new();
        let mut entries = vec![None; count];
        // Stitched rows call a function through its cell, filled once the code is placed.
        let entry_cells: Box<[std::cell::Cell<usize>]> = (0..count).map(|_| std::cell::Cell::new(0)).collect();
        let mut calls = Vec::new();
        let place = |bytes: &mut Vec<u8>, code: Vec<u8>| {
            bytes.resize(bytes.len().next_multiple_of(FUNCTION_ALIGNMENT), 0);
            let offset = bytes.len();
            bytes.extend_from_slice(&code);
            offset
        };
        let projected_code_len = |current: usize, appended: usize| {
            current
                .checked_add(FUNCTION_ALIGNMENT - 1)
                .map(|aligned| aligned / FUNCTION_ALIGNMENT * FUNCTION_ALIGNMENT)?
                .checked_add(appended)
        };
        // Largest first: the longest compile starts at once and the rest fill around it, so
        // the slowest function does not finish last on one worker (bodies are placed by index).
        // Without an ISA (`--b-jit`) no body or Cranelift entry exists: `bodies` stays empty.
        let mut bodies = Vec::new();
        if let Some(isa) = isa.as_deref() {
            analysis.unrolls = program_unrolls(&code);
            let mut build_order: Vec<usize> = (0..count).filter(|&index| plans[index].is_some()).collect();
            build_order.sort_by_key(|&index| std::cmp::Reverse(code.function(index).code.len()));
            let build = build_order.into_iter().map(|index| {
                let full_plan = full_plans[index].as_ref()?;
                let body_memory_before = diagnostics_enabled.then(jit_memory_snapshot).flatten();
                let admitted = admission::specialize(admission::UnitRequest { kind: admission::UnitKind::Wasm, facts: Default::default() }).compiled()?;
                let built = WasmSpecializer {
                    index,
                    plans: &plans,
                    full_plan,
                    reference_site_ids: &reference_site_ids[index],
                    admitted,
                    tier: WasmTierChoice::Optimizing { analysis: &analysis, isa, guarded, fixed_base, diagnostics_enabled },
                }
                .specialize(&code);
                if diagnostics_enabled {
                    let memory_after = jit_memory_snapshot();
                    eprintln!(
                        "QUENCH_WASM_JIT_BODY_MEMORY function={index} rss_kib_before={:?} rss_kib_after={:?} hwm_kib_before={:?} hwm_kib_after={:?}",
                        body_memory_before.map(|memory| memory.0),
                        memory_after.map(|memory| memory.0),
                        body_memory_before.map(|memory| memory.1),
                        memory_after.map(|memory| memory.1),
                    );
                }
                let Ok(WasmResidual::Compiled(built, entry)) = built else { return None };
                Some((index, built, entry))
            });
            bodies = compile_built_functions(isa, count, self.memory_policy(), build)?;
        }
        // Under `--b-jit` each function's entry is its stitched rows. A callee whose stitch fails
        // has no entry cell, so it leaves `callable` and its callers call it through the VM
        // (`CALL_VM`) instead of failing. Only a movable memory (`memory_stable` false) refuses
        // that call, so there a caller still fails with its callee; the set only shrinks.
        let mut stitched: Vec<Option<Vec<u8>>> = if base_only {
            let mut callable: Vec<bool> = plans.iter().map(Option::is_some).collect();
            loop {
                let codes: Vec<Option<Vec<u8>>> = (0..count)
                    .map(|index| {
                        if !callable[index] {
                            return None;
                        }
                        let full_plan = full_plans[index].as_ref()?;
                        let admitted = admission::specialize(admission::UnitRequest { kind: admission::UnitKind::Wasm, facts: Default::default() }).compiled()?;
                        let residual = WasmSpecializer {
                            index,
                            plans: &plans,
                            full_plan,
                            reference_site_ids: &reference_site_ids[index],
                            admitted,
                            tier: WasmTierChoice::Baseline {
                                analysis: &analysis,
                                memory_slot,
                                entry_cells: &entry_cells,
                                callable: &callable,
                            },
                        }
                        .specialize(&code);
                        match residual.ok()? {
                            WasmResidual::Rows(rows) => Some(rows),
                            WasmResidual::Compiled(..) => None,
                        }
                    })
                    .collect();
                let kept: Vec<bool> = codes.iter().map(Option::is_some).collect();
                if kept == callable {
                    break codes;
                }
                callable = kept;
            }
        } else {
            Vec::new()
        };
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            if base_only {
                // `--b-jit`: the stitched rows are the entry, and the table names them as the
                // body too (the same `WasmEntry` ABI, so indirect calls enter them directly).
                // A function whose stitch failed has no native entry: it runs in the interpreter.
                let Some(stitched) = stitched[index].take() else {
                    continue;
                };
                let stitched_projected_len = projected_code_len(bytes.len(), stitched.len())?;
                if let Some(root) = root
                    && stitched_projected_len > MAX_JIT_ROOT_CODE_BYTES
                {
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_ROOT_REJECTION root={root} native-code-bytes={stitched_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                        );
                    }
                    return None;
                }
                let stitched_len = stitched.len();
                let offset = place(&mut bytes, stitched);
                offsets[index] = Some(offset);
                spans.push((offset, stitched_len, index));
                entries[index] = Some(offset);
                continue;
            }
            let (body, function_calls, entry) = bodies[index].take()?;
            let body_projected_len = projected_code_len(bytes.len(), body.len())?;
            if let Some(root) = root
                && body_projected_len > MAX_JIT_ROOT_CODE_BYTES
            {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} native-code-bytes={body_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                    );
                }
                return None;
            }
            let body_len = body.len();
            let offset = place(&mut bytes, body);
            offsets[index] = Some(offset);
            spans.push((offset, body_len, index));
            calls.extend(function_calls.into_iter().map(|call| (offset, call)));
            let entry_projected_len = projected_code_len(bytes.len(), entry.len())?;
            if let Some(root) = root
                && entry_projected_len > MAX_JIT_ROOT_CODE_BYTES
            {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} native-code-bytes={entry_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                    );
                }
                return None;
            }
            entries[index] = Some(place(&mut bytes, entry));
        }
        for (offset, call) in calls {
            link(&mut bytes, offset, &call, offsets[call.callee]?)?;
        }
        let memory = CodeMemory::new(&bytes)?;
        let mapped_bytes = memory.length();
        let base = memory.pointer() as usize;
        write_perf_map("wasm-function", base, &bytes, &spans);
        for (cell, entry) in entry_cells.iter().zip(&entries) {
            cell.set(entry.map_or(0, |entry| base + entry));
        }
        let table: Box<[usize]> = offsets
            .iter()
            .map(|offset| offset.map_or(0, |offset| base + offset))
            .collect();
        let mut positions = vec![NOT_COMPILED; count].into_boxed_slice();
        let functions: Box<[CompiledFunction]> = plans
            .into_iter()
            .zip(entries)
            .enumerate()
            .filter_map(|(index, (plan, entry))| Some((index, plan?, entry?)))
            .enumerate()
            .map(|(position, (index, plan, entry))| {
                positions[index] = position as u32;
                // SAFETY: the entry was compiled with the `WasmEntry` signature in
                // the host's default calling convention.
                let entry = unsafe { std::mem::transmute::<usize, WasmEntry>(base + entry) };
                CompiledFunction {
                    entry,
                    params: plan.params,
                    result: plan.result,
                    native_entries: std::cell::Cell::new(0),
                    direct_reference_entries: std::cell::Cell::new(0),
                }
            })
            .collect();
        let compiled_functions = functions.len();
        let diagnostics = diagnostics_enabled.then(|| {
            let import_callout_names = self
                .programs
                .wasm_signatures(program)
                .map(|signatures| {
                    signatures
                        .imports()
                        .iter()
                        .enumerate()
                        .map(|(function, import)| {
                            (
                                u32::try_from(function).expect("Wasm function indices fit in u32"),
                                import.name.index,
                                import.name.module.clone(),
                                import.name.name.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let import_callout_counts = import_callout_names
                .iter()
                .map(|_| std::cell::Cell::new(0))
                .collect();
            WasmJitDiagnostics {
                program: program.raw(),
                total_functions: count,
                compiled_functions,
                code_bytes: bytes.len(),
                mapped_bytes,
                native_entry_requests: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
                interpreter_fallback_entries: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
                compiled_to_vm_callouts: std::cell::Cell::new(0),
                compiled_import_callouts: std::cell::Cell::new(0),
                compiled_known_callouts: std::cell::Cell::new(0),
                compiled_reference_callouts: std::cell::Cell::new(0),
                emscripten_memcpy_fast_path_hits: std::cell::Cell::new(0),
                import_callout_names,
                import_callout_counts,
                reference_call_sites,
            }
        });
        Some(CompiledProgram {
            _code: memory,
            table,
            _entry_cells: entry_cells,
            functions,
            positions,
            memory_slot,
            guarded,
            fixed_base,
            diagnostics,
            indirect_hits: indirect_profile::SiteHits::new(reference_cache_count),
            reference_cache_sites,
        })
    }

    #[cfg(test)]
    pub(crate) fn install_compiled_diagnostics_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
    ) -> bool {
        let Some(compiled) = jit_root_override().ok().and_then(|root| self.wasm_module_unit(program, root, false)).and_then(|unit| self.compile_program_with_diagnostics(program, env, true, unit))
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    #[cfg(test)]
    pub(crate) fn install_compiled_root_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
        root: usize,
    ) -> bool {
        let Some(compiled) = self.wasm_module_unit(program, Some(root), false).and_then(|unit| self.compile_program_with_diagnostics_and_root(program, env, true, Some(root), unit))
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    #[cfg(test)]
    pub(crate) fn install_compiled_root_only_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
        root: usize,
    ) -> bool {
        let Some(compiled) = self.wasm_module_unit(program, Some(root), true).and_then(|unit| self.compile_program_with_diagnostics_and_root_mode(program, env, true, Some(root), true, unit))
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    /// The native code a known call of function `id` from frame `caller` runs, offered as the
    /// general loop's native entry (`wasm_activation::enter_native`) asks: the code a tier holds,
    /// after the tiers' entry counters and compiles this entry makes (`offer_compiled_in`).
    /// `None` where no tier runs it: the interpreter takes the call.
    pub(crate) fn offer_compiled(&mut self, caller: usize, id: u32) -> Option<Rc<CompiledProgram>> {
        #[cfg(feature = "jit")]
        { self.offer_compiled_compiled(caller, id) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (caller, id);
            None
        }
    }

    #[cfg(feature = "jit")]
    fn offer_compiled_compiled(&mut self, caller: usize, id: u32) -> Option<Rc<CompiledProgram>> {
        let (program, env) = {
            let frame = self.frames.get(caller)?;
            (frame.program, frame.env)
        };
        self.offer_compiled_in(program, env, id)
    }

    /// Runs the code `offer_compiled` offered for function `id`, with arguments from frame
    /// `caller`'s register `window`. `None` leaves the call to the interpreter.
    pub(crate) fn run_offered(
        &mut self,
        compiled: Rc<CompiledProgram>,
        caller: usize,
        id: u32,
        window: quench_runtime::bytecode::RegisterWindow,
    ) -> Option<Result<Value, JsError>> {
        #[cfg(feature = "jit")]
        { self.run_offered_compiled(compiled, caller, id, window) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (compiled, caller, id, window);
            None
        }
    }

    #[cfg(feature = "jit")]
    fn run_offered_compiled(
        &mut self,
        compiled: Rc<CompiledProgram>,
        caller: usize,
        id: u32,
        window: quench_runtime::bytecode::RegisterWindow,
    ) -> Option<Result<Value, JsError>> {
        let (program, env) = {
            let frame = self.frames.get(caller)?;
            (frame.program, frame.env)
        };
        let args = CompiledArgs::RegisterWindow { caller, base: usize::from(window.base), count: usize::from(window.count) };
        self.run_offered_in(compiled, program, env, id, args)
    }

    /// Runs the code the tiers offered for function `id` of `program` (`wasm_activation::offer_entry`)
    /// in instance `env`, with encoded Wasm `args`. `None` leaves the call to the interpreter.
    pub(crate) fn run_offered_values(
        &mut self,
        compiled: Rc<CompiledProgram>,
        program: ProgramId,
        env: Value,
        id: u32,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        #[cfg(feature = "jit")]
        { self.run_offered_in(compiled, program, env, id, CompiledArgs::Values(args)) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (compiled, program, env, id, args);
            None
        }
    }

    /// Run function `id` of `program` in instance `env` as native code, with
    /// encoded Wasm `args`. `None` leaves the call to the interpreter, which
    /// is then exactly where the call would otherwise have gone.
    pub(crate) fn run_compiled(
        &mut self,
        program: ProgramId,
        env: Value,
        id: u32,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        #[cfg(feature = "jit")]
        { self.run_compiled_inner(program, env, id, CompiledArgs::Values(args)) }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (program, env, id, args);
            None
        }
    }

    /// Convert an owned argument window or a public Wasm value slice into the
    /// raw scalar words expected by a compiled entry. The register borrow is
    /// confined to this conversion and ends before native code can call back
    /// into the VM.
    fn compiled_raw_args(
        &self,
        args: CompiledArgs<'_>,
        params: &[quench_runtime::WasmType],
    ) -> Option<smallvec::SmallVec<[u64; 8]>> {
        let values = match args {
            CompiledArgs::Values(values) => values,
            CompiledArgs::RegisterWindow {
                caller,
                base,
                count,
            } => self
                .frames
                .get(caller)?
                .registers
                .get(base..base.checked_add(count)?)?,
        };
        if values.len() != params.len() {
            return None;
        }
        let mut raw = smallvec::SmallVec::new();
        for (value, ty) in values.iter().zip(params) {
            raw.push(match Kind::of(*ty)? {
                Kind::I32 => u64::from(value.wasm_bits32()),
                Kind::I64 => self.decode_bits64(*value)?,
                Kind::Reference => value.to_bits(),
            });
        }
        Some(raw)
    }

    fn run_compiled_inner(
        &mut self,
        program: ProgramId,
        env: Value,
        id: u32,
        args: CompiledArgs<'_>,
    ) -> Option<Result<Value, JsError>> {
        let compiled = self.offer_compiled_in(program, env, id)?;
        self.run_offered_in(compiled, program, env, id, args)
    }

    /// The tiers' answer to an entry of function `id` of `program` in instance `env`: the code
    /// that runs it natively, after the entry's counters and the compiles it makes due (the
    /// program's first decision, a selective module's whole compile, a hot root's tier). `None`
    /// where no tier holds it, which is noted as the function's interpreter fallback.
    #[inline(always)]
    fn offer_compiled_in(&mut self, program: ProgramId, env: Value, id: u32) -> Option<Rc<CompiledProgram>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        // The common entry: the program's whole compile holds the function, which no counter,
        // escalation or tier changes (the decisions below all start from a missing artifact).
        if let Some(Some(baseline)) = self.programs.compiled(program)
            && baseline.has_function(id as usize)
        {
            return Some(baseline);
        }
        self.offer_compiled_cold(program, env, id)
    }

    /// `offer_compiled_in` of a function the program's whole compile does not hold.
    #[cold]
    #[inline(never)]
    fn offer_compiled_cold(&mut self, program: ProgramId, env: Value, id: u32) -> Option<Rc<CompiledProgram>> {
        if !cfg!(feature = "jit") || self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        let wasm_root_only = jit_wasm_root_only_enabled()
            && self
                .programs
                .residual(program)
                .is_some_and(|code| code.kind == quench_runtime::bytecode::ProgramKind::Wasm);
        let wasm_root_first = jit_wasm_root_first_enabled()
            && self
                .programs
                .residual(program)
                .is_some_and(|code| code.kind == quench_runtime::bytecode::ProgramKind::Wasm);
        let _decided = match self.programs.compiled(program) {
            Some(compiled) => compiled,
            None => {
                if self
                    .programs
                    .residual(program)
                    .is_some_and(|code| code.kind == quench_runtime::bytecode::ProgramKind::Wasm)
                {
                    self.host.before_native_compilation();
                }
                if wasm_root_only || wasm_root_first {
                    // This explicit diagnostic mode compiles only the
                    // function named by QUENCH_DEV=wasm-root=N. Root-only mode
                    // keeps its callees in the VM; root-first mode allows a
                    // hot callee to enter the existing bounded tier below.
                    let diagnose = jit_diagnostics_enabled();
                    let root = jit_root_override().ok().flatten();
                    let compiled = root
                        .and_then(|root| {
                            self.wasm_module_unit(program, Some(root), true).and_then(|unit| self.compile_program_with_diagnostics_and_root_mode(program, env, diagnose, Some(root), true, unit))
                        })
                        .map(Rc::new);
                    if diagnose && compiled.is_none() {
                        let mode_field = if wasm_root_first {
                            "\"root_first\":true"
                        } else {
                            "\"root_only\":true"
                        };
                        eprintln!(
                            "QUENCH_WASM_JIT_STATS {{\"program\":{},{},\"compile_failed\":true,\"root\":{}}}",
                            program.raw(),
                            mode_field,
                            root.map_or_else(|| "null".to_owned(), |root| root.to_string()),
                        );
                    }
                    self.programs.set_compiled(program, compiled);
                    self.programs.settle_wasm_module_tier(program);
                    self.programs.compiled(program).flatten()
                } else if self
                    .programs
                    .residual(program)
                    .is_some_and(|code| code.kind == quench_runtime::bytecode::ProgramKind::Wasm)
                    && self.programs.wasm_module_starts_interpreted(program)
                {
                    // A Wasm module starts in the interpreter and compiles whole once it runs
                    // long (`WasmModuleTier`); a small one compiles whole below, at once.
                    self.programs.set_compiled(program, None);
                    None
                } else {
                    let diagnose = jit_diagnostics_enabled()
                        && self
                            .programs
                            .get(program)
                            .is_some_and(|code| code.kind == quench_runtime::bytecode::ProgramKind::Wasm);
                    let compiled = jit_root_override().ok().and_then(|root| self.wasm_module_unit(program, root, false)).and_then(|unit| self.compile_program(program, env, unit)).map(Rc::new);
                    if diagnose && compiled.is_none() {
                        let total_functions = self
                            .programs
                            .get(program)
                            .map_or(0, |code| code.functions.len());
                        eprintln!(
                            "QUENCH_WASM_JIT_STATS {{\"program\":{},\"compile_attempts\":1,\"compile_failed\":true,\"total_functions\":{}}}",
                            program.raw(),
                            total_functions,
                        );
                    }
                    self.programs.set_compiled(program, compiled);
                    self.programs.settle_wasm_module_tier(program);
                    self.programs.compiled(program).flatten()
                }
            }
        };
        // The program's first entry decided its whole compile above; the entry's counters and the
        // compiles they make due are the staged rule both interpreters run.
        let mut tiers = GeneralTiers { vm: self, program, env, root_only: wasm_root_only, root_first: wasm_root_first };
        match quench_runtime::staged::wasm_activation::offer_entry(&mut tiers, id) {
            quench_runtime::staged::wasm_activation::EntryOffer::Held(code) => Some(code),
            _ => None,
        }
    }

    /// Makes the claimed whole compile of selective Wasm module `program` (`WasmModuleTier`).
    fn escalate_wasm_module(&mut self, program: ProgramId, env: Value) {
        // The module's code is about to run natively, so the interpreter's views of it go
        // first, and the compile reuses their memory, unless a frame of it is on the stack,
        // whose lane run may hold their records, or a lane activation deferred beside this
        // call (`runs_beside_activations`), a frame not on it. An interpreted run derives
        // them again.
        if !self.deferred_activations_run(program) && !self.frames.iter().any(|frame| frame.program == program) {
            self.programs.release_lane_views(program);
            // Their records' addresses name private-name sites' entries (`PrivateSiteHomes`).
            self.private_site_homes.clear();
        }
        let diagnose = jit_diagnostics_enabled();
        let compiled = jit_root_override().ok().and_then(|root| self.wasm_module_unit(program, root, false)).and_then(|unit| self.compile_program(program, env, unit)).map(Rc::new);
        if diagnose {
            let bytecodes: usize = self.programs.residual(program).map_or(0, |code| {
                (0..code.functions.len())
                    .filter_map(|index| code.function_get(index))
                    .map(|function| function.code.len())
                    .sum()
            });
            eprintln!(
                "QUENCH_WASM_JIT_STATS {{\"program\":{},\"module_escalated\":true,\"compiled\":{},\"bytecodes\":{}}}",
                program.raw(),
                compiled.is_some(),
                bytecodes,
            );
        }
        self.programs.set_compiled(program, compiled);
    }

    /// Makes the root compile of function `id` of Wasm module `program` and installs it; whether
    /// the tier keeps it.
    fn compile_wasm_root(&mut self, program: ProgramId, env: Value, id: u32) -> bool {
        #[cfg(feature = "jit")]
        {
            let tier = self.wasm_module_unit(program, Some(id as usize), false).and_then(|unit| self.compile_program_with_diagnostics_and_root(program, env, jit_diagnostics_enabled(), Some(id as usize), unit)).map(Rc::new);
            tier.is_some_and(|tier| self.programs.install_wasm_tier(program, id, tier))
        }
        #[cfg(not(feature = "jit"))]
        {
            let _ = (program, env, id);
            false
        }
    }

    /// Runs `compiled`, the code `offer_compiled_in` offered for function `id` of `program`, in
    /// instance `env` with `args`. `None` leaves the call to the interpreter.
    fn run_offered_in(
        &mut self,
        compiled: Rc<CompiledProgram>,
        program: ProgramId,
        env: Value,
        id: u32,
        args: CompiledArgs<'_>,
    ) -> Option<Result<Value, JsError>> {
        let result = (|| {
            let function = compiled.function(id as usize)?;
            let (record, (base, length)) = self.publish_wasm_instance(&compiled, env, program)?;
            let raw = self.compiled_raw_args(args, &function.params)?;
            let marker = 0u8;
            let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
            let mut import_callouts = 0u64;
            let mut known_callouts = 0u64;
            let mut reference_callouts = 0u64;
            let mut emscripten_memcpy_fast_path_hits = 0u64;
            let (
                import_callouts_ptr,
                known_callouts_ptr,
                reference_callouts_ptr,
                emscripten_memcpy_fast_path_hits_ptr,
            ): (*mut u64, *mut u64, *mut u64, *mut u64) = if compiled.diagnostics.is_some() {
                (
                    &mut import_callouts as *mut u64,
                    &mut known_callouts as *mut u64,
                    &mut reference_callouts as *mut u64,
                    &mut emscripten_memcpy_fast_path_hits as *mut u64,
                )
            } else {
                (
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            let poll = self.execution_interrupt.poll_page();
            let mut context = CallContext {
                head: CallContextFields {
                    stack_limit: stack_pointer.saturating_sub(COMPILED_STACK_BUDGET),
                    interrupt: self.execution_interrupt.attention_flag(),
                    table: std::ptr::null(),
                    vm: (self as *mut Self).cast(),
                    env,
                    entry_stack: 0,
                    guard_start: 0,
                    guard_end: 0,
                    globals: std::ptr::null(),
                    table_words: std::ptr::null(),
                    callout: import_callout as Callout as usize,
                    call_reference: reference_callout as ReferenceCallout as usize,
                    compiled: std::ptr::null(),
                    program,
                    import_callouts: import_callouts_ptr,
                    known_callouts: known_callouts_ptr,
                    reference_callouts: reference_callouts_ptr,
                    memory: Value::UNDEFINED,
                    memory_length: 0,
                    raised: None,
                    call_reference_diagnostic: 0,
                    reference_call_sites: std::ptr::null(),
                    reference_call_site_count: 0,
                    poll_start: poll.map_or(0, |(start, _)| start),
                    poll_end: poll.map_or(0, |(start, bytes)| start + bytes),
                    emscripten_memcpy_fast_path_hits: emscripten_memcpy_fast_path_hits_ptr,
                    import_callout_counts: std::ptr::null(),
                    import_callout_count: 0,
                    adapter_payload: std::ptr::null_mut(),
                },
                interrupt_owner: std::ptr::null(),
                instance: std::ptr::null(),
                native_roots: &mut *self.native_roots,
                memory_grow: wasm_memory_stage::memory_grow_callout as *const () as usize,
                wasm_gc_records: self.heap_reads().wasm_gc_record_base().map_or(0, |base| base as usize),
            };
            // SAFETY: the record belongs to the environment the call runs in.
            context.enter(unsafe { &*record });
            let mut out = 0u64;
            compiled.note_native_entry(id as usize);
            // SAFETY: the entry is compiled code for this function's
            // signature; the context, arguments and result outlive the call,
            // and the memory view stays valid because compiled code never
            // grows memory.
            let status = unsafe {
                enter_wasm(
                    &mut context,
                    base,
                    length,
                    raw.as_ptr(),
                    &mut out,
                    function.entry,
                )
            };
            if let Some(stats) = &compiled.diagnostics {
                stats.compiled_to_vm_callouts.set(
                    stats
                        .compiled_to_vm_callouts
                        .get()
                        .saturating_add(import_callouts)
                        .saturating_add(known_callouts)
                        .saturating_add(reference_callouts),
                );
                stats.compiled_import_callouts.set(
                    stats
                        .compiled_import_callouts
                        .get()
                        .saturating_add(import_callouts),
                );
                stats.compiled_known_callouts.set(
                    stats
                        .compiled_known_callouts
                        .get()
                        .saturating_add(known_callouts),
                );
                stats.compiled_reference_callouts.set(
                    stats
                        .compiled_reference_callouts
                        .get()
                        .saturating_add(reference_callouts),
                );
                stats.emscripten_memcpy_fast_path_hits.set(
                    stats
                        .emscripten_memcpy_fast_path_hits
                        .get()
                        .saturating_add(emscripten_memcpy_fast_path_hits),
                );
            }
            if i64::from(status) == TERMINATED_STATUS {
                return Some(Err(JsError::execution_terminated()));
            }
            if i64::from(status) == RAISED_STATUS {
                return match context.raised.take()? {
                    Raised::Error(error) => Some(Err(error)),
                    Raised::Panic(panic) => std::panic::resume_unwind(panic),
                };
            }
            if status != 0 {
                let trap = TRAPS[status as usize - 1];
                return Some(Err(JsError::wasm_trap_error(trap)));
            }
            Some(Ok(match function.result {
                Some(ty) => match Kind::of(ty)? {
                    Kind::I32 => Value::integer(out as u32 as i32),
                    Kind::I64 => self.encode_bits64(out),
                    Kind::Reference => Value::from_bits(out),
                },
                None => self.programs.constant(program, VOID_RESULT_CONSTANT)?,
            }))
        })();
        if let Some(stats) = &compiled.diagnostics
            && result.is_none()
            && let Some(fallbacks) = stats.interpreter_fallback_entries.get(id as usize)
        {
            fallbacks.set(fallbacks.get().saturating_add(1));
        }
        result
    }
}

/// The general loop's tiers for one Wasm entry (`wasm_activation::offer_entry`): it makes the
/// compiles an entry makes due in place. `root_only` and `root_first` are the diagnostic root modes.
struct GeneralTiers<'a> {
    vm: &'a mut Vm,
    program: ProgramId,
    env: Value,
    root_only: bool,
    root_first: bool,
}

impl quench_runtime::staged::wasm_activation::WasmEntryTiers for GeneralTiers<'_> {
    type Code = Rc<CompiledProgram>;

    fn whole(&mut self, function: u32) -> Option<Rc<CompiledProgram>> {
        self.vm.programs.compiled(self.program).flatten().filter(|compiled| compiled.has_function(function as usize))
    }

    fn note_module_work(&mut self) -> bool {
        !self.root_only && !self.root_first && matches!(self.vm.programs.compiled(self.program), Some(None))
            && self.vm.programs.note_wasm_module_entry(self.program)
    }

    fn compile_whole(&mut self) -> quench_runtime::staged::wasm_activation::Compile {
        use quench_runtime::staged::wasm_activation::Compile;
        if !self.vm.programs.claim_wasm_module_escalation(self.program) {
            return Compile::Refused;
        }
        self.vm.escalate_wasm_module(self.program, self.env);
        Compile::Made
    }

    fn root(&mut self, function: u32) -> Option<Rc<CompiledProgram>> {
        self.vm.programs.wasm_tier_containing(self.program, function)
    }

    /// Root-first mode earns further tiering only after its selected root produced an artifact:
    /// where admission failed, the workload stays in the interpreter instead of silently becoming
    /// ordinary selective tiering.
    fn root_admissible(&mut self, function: u32) -> bool {
        !self.root_only
            && (!self.root_first || self.vm.programs.compiled(self.program).flatten().is_some())
            && self.vm.programs.wasm_root_admissible(self.program, function)
    }

    fn note_root_entry(&mut self, function: u32) -> bool {
        self.vm.programs.note_wasm_root_entry(self.program, function, false)
    }

    fn compile_root(&mut self, function: u32) -> quench_runtime::staged::wasm_activation::Compile {
        use quench_runtime::staged::wasm_activation::Compile;
        if self.vm.compile_wasm_root(self.program, self.env, function) { Compile::Made } else { Compile::Refused }
    }

    fn note_fallback(&mut self, function: u32) {
        self.vm.programs.has_compiled_function_or_note_fallback(self.program, function as usize);
    }
}

#[quench_macros::ext(CompiledVm5)]
impl Vm {
    /// Call imported `function` of `program` in instance `env` with the Wasm
    /// bits at `args`, storing its result's bits at `out`.
    ///
    /// # Safety
    /// `args` holds a word per parameter of the import and `out` one word.
    unsafe fn call_import(
        &mut self,
        program: ProgramId,
        env: Value,
        function: u32,
        current_memory: Value,
        memcpy_fast_path_hits: *mut u64,
        args: *const u64,
        out: *mut u64,
    ) -> Result<(), JsError> {
        let code = self
            .programs
            .get(program)
            .ok_or_else(|| JsError::validation("missing compiled Wasm program".into()))?;
        let (params, result_type) = {
            let signature = self
                .programs
                .wasm_signatures(program)
                .and_then(|signatures| signatures.get(function as usize))
                .ok_or_else(|| JsError::validation("missing Wasm import signature".into()))?;
            (
                signature
                    .params
                    .iter()
                    .copied()
                    .collect::<smallvec::SmallVec<[quench_runtime::WasmType; 8]>>(),
                signature.results.first().copied(),
            )
        };
        let active_program = self.replace_active_program(program);
        let callee = self.wasm_function_reference(&code, function, env);
        self.set_active_program(active_program);
        let callee = callee?;
        let receiver = self
            .programs
            .constant(program, VOID_RESULT_CONSTANT)
            .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
        let defined = self
            .programs
            .wasm_signatures(program)
            .and_then(|signatures| signatures.defined_index(function));
        let result = {
            // Allocation never collects, so the encoded arguments stay live
            // until the ordinary callout roots them.
            let mut values: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
            for (offset, ty) in params.iter().enumerate() {
                // SAFETY: the caller passes a word per parameter.
                let bits = unsafe { *args.add(offset) };
                values.push(wasm_word_value(self, *ty, bits));
            }
            if let Some(defined) = defined {
                let active_program = self.replace_active_program(program);
                let compiled = self.run_compiled(program, env, defined, &values);
                self.set_active_program(active_program);
                if let Some(compiled) = compiled {
                    let result = compiled?;
                    let bits = wasm_call_vm::result_bits(self, program, result, result_type)?;
                    // SAFETY: the caller passes a result word.
                    unsafe { *out = bits };
                    return Ok(());
                }
            }
            let js_import = self
                .host_import_js_target(callee)
                .filter(|(_, _, signature)| values.len() == signature.params.len());
            if let Some((target, memcpy_capture, signature)) = js_import {
                self.call_compiled_js_wasm_import(
                    program,
                    &code,
                    callee,
                    target,
                    memcpy_capture,
                    &signature,
                    receiver,
                    &values,
                    current_memory,
                    memcpy_fast_path_hits,
                )?
            } else {
                self.call_value(&code, callee, receiver, &values)?
            }
        };
        if result_type.is_some() {
            let bits = wasm_call_vm::result_bits(self, program, result, result_type)?;
            // SAFETY: the caller passes a result word.
            unsafe { *out = bits };
        }
        Ok(())
    }
}

/// The constant a void Wasm function returns (see the Wasm lowering).
const VOID_RESULT_CONSTANT: usize = 1;

/// Whether a lane body for `body` could amortize an entry (`lane_facts::amortizes_entry`): a
/// loop, a call (which a plan inlines or binds), or length. A short call-free function never
/// gets a body of its own, so claiming it for the tier would only cost it its leaf calls.
pub(crate) fn body_may_amortize_in_lane(tiers: &Tiers, body: &quench_runtime::bytecode::Function) -> bool {
    body_may_amortize(tiers, body, tier_policy::body_tier_in_lane())
}

/// `body_may_amortize_in_lane` for the body of `tier`, whose length that tier must reach.
pub(crate) fn body_may_amortize(tiers: &Tiers, body: &quench_runtime::bytecode::Function, tier: crate::compiled::admission::Tier) -> bool {
    body.loops() || body.code.len() >= tiers.amortizing_instructions(tier)
        || body.code.iter().any(|packed| quench_runtime::staged::Operation::<u64, ()>::is_plain_call(body.widened(*packed)))
}

/// Whether native entries of `body` can pay: a loop-free body shorter than an entry's
/// overhead (`JS_TIER_LANE_ENTRY_RECORDS`) runs cheaper in the interpreter at each entry.
pub(crate) fn pays_native_entry(body: &quench_runtime::bytecode::Function) -> bool {
    body.code.len() >= JS_TIER_LANE_ENTRY_RECORDS || body.loops()
}

pub(crate) use quench_runtime::vm::engine_types::DirectTierEntry;

#[quench_macros::ext(DirectTierEntryOps1)]
impl DirectTierEntry {
    /// Whether the tier retired the artifact.
    pub(crate) fn retired(&self) -> bool {
        self.compiled.strong_count() == 0
    }
}

/// What the JavaScript tier reads of an activation it enters at PC zero: a VM frame's
/// (`Vm::js_tier_frame_entry`) or a frameless lane activation's
/// (`Vm::js_tier_activation_entry`). Either way a native result continues the same
/// activation, so the tier never decides how a function is called.
pub(crate) struct TierEntry {
    program: ProgramId,
    function: u32,
    /// The activation's parameter registers.
    parameters: smallvec::SmallVec<[Value; 8]>,
    /// The function's entry facts, looked up once per entry (`ProgramStore::js_entry_facts`).
    facts: Rc<crate::js_compiled::JsEntryFacts>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_narrow_store_encoding_matches_shared_values() {
        use quench_runtime::value::NarrowLayout as N;
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.params.push(AbiParam::new(types::I64));
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let value = builder.block_params(entry)[0];
        let mut stage = object_slots::Emit { builder: &mut builder, property_values: None, gate: None, payload: None, facts: Default::default() };
        let encoded = quench_runtime::staged::narrow::narrow_form(&mut stage, value, typed_ir::Type::ANY);
        builder.ins().return_(&[encoded]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let memory = compile_native_function(function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the generated function accepts and returns a single u64.
        let encode: unsafe extern "C" fn(u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
        let limit = 1_i32 << (N::INTEGER_BITS - 1);
        let mut values = vec![Value::NULL, Value::UNDEFINED, Value::TRUE, Value::DELETED,
            Value::heap(0), Value::direct_string(0), Value::number(-0.0), Value::number(1.5), Value::number(f64::NAN)];
        values.extend([i32::MIN, -limit - 1, -limit, -1, 0, 1, limit - 1, limit, i32::MAX].map(Value::integer));
        values.extend([0, 1, (1 << N::INDEX_BITS) - 1, 1 << N::INDEX_BITS, u32::MAX].map(Value::direct_object));
        for value in values {
            let expected = value.to_narrow().map_or(u64::MAX, u64::from);
            assert_eq!(unsafe { encode(value.to_bits()) }, expected, "{value:?}");
        }
    }

    #[test]
    fn native_narrow_decoder_matches_value_bits() {
        use quench_runtime::value::NarrowLayout as N;
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.params.push(AbiParam::new(types::I32));
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let narrow = builder.block_params(entry)[0];
        let done = builder.create_block();
        let result = builder.append_block_param(done, types::I64);
        let table = process_narrow_table(&mut builder);
        emit_narrow_read(&mut builder, narrow, table, done);
        builder.switch_to_block(done);
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let memory = compile_native_function(function, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the function above accepts one u32 and returns its decoded u64.
        let decode: unsafe extern "C" fn(u32) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
        let indices = [0, 1, (1 << N::INTEGER_BITS) - 1, u32::MAX];
        for code in 0..=N::CODE_MASK {
            for index in indices {
                let narrow = (index << N::INDEX_SHIFT) | (code << N::CODE_SHIFT);
                assert_eq!(unsafe { decode(narrow) }, Value::from_narrow(narrow).to_bits());
            }
        }
        let half = 1_i32 << (N::INTEGER_BITS - 1);
        for integer in [i32::MIN, -half, -half + 1, -1, 0, 1, half - 1, half, i32::MAX] {
            let narrow = ((integer as u32) << N::CODE_SHIFT) | N::INTEGER_FLAG;
            assert_eq!(unsafe { decode(narrow) }, Value::from_narrow(narrow).to_bits());
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_logical_right_shift_selection_preserves_all_integer_widths() {
        // A64 Logical (shifted register): fixed class bits, N=0; shift kind
        // occupies bits23:22. The test checks actual emitted instructions.
        const LOGICAL_CLASS_MASK: u32 = 0x1f20_0000;
        const LOGICAL_CLASS: u32 = 0x0a00_0000;
        const SHIFT_KIND_OFFSET: u32 = 22;
        const SHIFT_KIND_MASK: u32 = 0b11 << SHIFT_KIND_OFFSET;
        const LSR_KIND: u32 = 0b01 << SHIFT_KIND_OFFSET;
        const ASR_KIND: u32 = 0b10 << SHIFT_KIND_OFFSET;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLSH");
        use quench_runtime::vm::operations::NumberBitwise;
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            for operation in [NumberBitwise::And, NumberBitwise::Or, NumberBitwise::Xor] { for signed in [false, true] { for reversed in [false, true] {
                for count in [0_u64, 1, 17, 31, 32, 63, 64, u64::MAX] {
                    let mut signature = ir::Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64), AbiParam::new(types::I64)]);
                    signature.returns.push(AbiParam::new(types::I64));
                    let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let block = builder.create_block();
                    builder.append_block_params_for_function_params(block);
                    builder.switch_to_block(block);
                    let args = builder.block_params(block).to_vec();
                    let left = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                    let right = if ty == types::I64 { args[1] } else { builder.ins().ireduce(ty, args[1]) };
                    let shifted = if signed { builder.ins().sshr_imm_u(right, count as i64) }
                        else { builder.ins().ushr_imm_u(right, count as i64) };
                    let (left, right) = if reversed { (shifted, left) } else { (left, shifted) };
                    let result = match operation {
                        NumberBitwise::And => builder.ins().band(left, right),
                        NumberBitwise::Or => builder.ins().bor(left, right),
                        NumberBitwise::Xor => builder.ins().bxor(left, right),
                        _ => unreachable!("logical operation matrix"),
                    };
                    let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                    builder.ins().return_(&[result]);
                    builder.seal_all_blocks();
                    builder.finalize(isa.frontend_config());
                    let mut compiler = cranelift_codegen::Context::for_function(function);
                    let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
                    let bytes = compiled.buffer.data();
                    if width >= u32::BITS && count == 17 {
                        let kind = if signed { ASR_KIND } else { LSR_KIND };
                        assert!(bytes.as_chunks::<{ std::mem::size_of::<u32>() }>().0.iter().any(|bytes| {
                            let word = u32::from_le_bytes(*bytes);
                            word & LOGICAL_CLASS_MASK == LOGICAL_CLASS && word & SHIFT_KIND_MASK == kind
                        }), "right shift must fold: {ty} operation{operation:?} signed{signed} reversed{reversed}");
                    }
                    let memory = CodeMemory::new(bytes).unwrap();
                    // SAFETY: this verified native function has exactly two
                    // u64 inputs and one u64 result under the host C ABI.
                    let entry = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                    for left in [0, 1, u64::MAX, 1_u64 << (width - 1)] {
                        for right in [0, 1, u64::MAX, 1_u64 << (width - 1)] {
                            let shift = count as u32 & (width - 1);
                            let right = right & mask;
                            let shifted = if signed {
                                let widened = ((right << (u64::BITS - width)) as i64) >> (u64::BITS - width);
                                (widened >> shift) as u64 & mask
                            } else { right >> shift };
                            let expected = match operation { NumberBitwise::And => left & shifted, NumberBitwise::Or => left | shifted,
                                NumberBitwise::Xor => left ^ shifted, _ => unreachable!("logical operation matrix") } & mask;
                            assert_eq!(unsafe { entry(left, right) }, expected,
                                "{ty} operation{operation:?} signed{signed} reversed{reversed} shift{count}");
                        }
                    }
                }
            }}}
        }
    }

    #[cfg(target_arch = "aarch64")]
    fn assert_aarch64_cfg_edges(code: &cranelift_codegen::CompiledCode) {
        // A64 direct branch encodings; this fixture has no calls, jump tables
        // or literal islands. Compare the metadata with actual branch targets.
        const B_MASK: u32 = 0xfc00_0000;
        const B: u32 = 0x1400_0000;
        const B_COND_MASK: u32 = 0xff00_0010;
        const B_COND: u32 = 0x5400_0000;
        const CB_MASK: u32 = 0x7e00_0000;
        const CB: u32 = 0x3400_0000;
        const RET_MASK: u32 = 0xffff_fc1f;
        const RET: u32 = 0xd65f_0000;
        const RETAA: u32 = 0xd65f_0bff;
        const RETAB: u32 = 0xd65f_0fff;
        const BRANCH_BITS: u32 = 26;
        const CONDITIONAL_BITS: u32 = 19;
        const CONDITIONAL_SHIFT: u32 = 5;
        let bytes = code.buffer.data();
        let mut actual = Vec::new();
        for (index, &start) in code.bb_starts.iter().enumerate() {
            let end = code.bb_starts.get(index + 1).copied().unwrap_or(bytes.len() as u32);
            let mut terminal = false;
            for pc in (start..end).step_by(std::mem::size_of::<u32>()) {
                let word = u32::from_le_bytes(bytes[pc as usize..pc as usize + std::mem::size_of::<u32>()].try_into().unwrap());
                let branch = if word & B_MASK == B { terminal = true; Some((word, BRANCH_BITS)) }
                    else if word & B_COND_MASK == B_COND || word & CB_MASK == CB {
                        Some((word >> CONDITIONAL_SHIFT, CONDITIONAL_BITS))
                    } else { None };
                if let Some((immediate, bits)) = branch {
                    let signed = ((immediate << (u32::BITS - bits)) as i32) >> (u32::BITS - bits);
                    actual.push((start, (i64::from(pc) + i64::from(signed) * std::mem::size_of::<u32>() as i64) as u32));
                }
                terminal |= word & RET_MASK == RET || word == RETAA || word == RETAB;
                if terminal { break; }
            }
            if !terminal && end < bytes.len() as u32 { actual.push((start, end)); }
        }
        actual.sort_unstable(); actual.dedup();
        let mut recorded = code.bb_edges.clone();
        recorded.sort_unstable(); recorded.dedup();
        assert_eq!(recorded, actual, "machine CFG must follow physical blocks: starts{:?}, bytes{:x?}, {:?}",
            code.bb_starts, bytes.as_chunks::<{ std::mem::size_of::<u32>() }>().0.iter().map(|word| u32::from_le_bytes(*word)).collect::<Vec<_>>(), code.vcode);
    }

    #[test]
    fn private_store_commits_preserve_overlapping_bytes_and_zero_trips() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 2]);
        let mut function = ir::Function::with_name_signature(UserFuncName::testcase("private_store_commit"), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block(); let body = builder.create_block(); let exit = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        let count = builder.append_block_param(body, types::I64);
        builder.switch_to_block(entry);
        let args = builder.block_params(entry).to_vec();
        builder.ins().brif(args[1], body, &[args[1].into()], exit, &[]);
        builder.switch_to_block(body);
        let word = builder.ins().ireduce(types::I32, count);
        let first = builder.ins().store(MemFlagsData::trusted(), word, args[0], 0);
        const BYTE_XOR: u8 = 7;
        const BYTE_OFFSET: usize = 1;
        let byte = builder.ins().ireduce(types::I8, count);
        let byte = builder.ins().bxor_imm_u(byte, i64::from(BYTE_XOR));
        let second = builder.ins().store(MemFlagsData::trusted(), byte, args[0], BYTE_OFFSET as i32);
        builder.func.private_stores.extend([first, second]);
        let next = builder.ins().iadd_imm_s(count, -1);
        builder.ins().brif(next, body, &[next.into()], exit, &[]);
        builder.switch_to_block(exit); builder.ins().return_(&[]);
        builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
        let mut compiler = cranelift_codegen::Context::for_function(function);
        let code = compiler.compile(&*isa, &mut Default::default()).unwrap();
        let memory = CodeMemory::new(code.buffer.data()).unwrap();
        compiler.compute_cfg(); compiler.compute_domtree(); compiler.compute_loop_analysis();
        assert!(!compiler.func.layout.blocks().filter(|block| compiler.loop_analysis.innermost_loop(*block).is_some())
            .flat_map(|block| compiler.func.layout.block_insts(block))
            .any(|inst| compiler.func.dfg.insts[inst].opcode().can_store()), "{}", compiler.func);
        // SAFETY: the verified signature receives an aligned, live word and a count.
        let run: unsafe extern "C" fn(*mut u64, u64) = unsafe { std::mem::transmute(memory.pointer()) };
        const SENTINEL: u64 = 0x9182_7364_5546_3728;
        for count in [0, 1, 2, 17, 257] {
            let mut actual = SENTINEL; let mut expected = SENTINEL.to_ne_bytes();
            if count != 0 {
                expected[..size_of::<u32>()].copy_from_slice(&1_u32.to_ne_bytes());
                expected[BYTE_OFFSET] = 1 ^ BYTE_XOR;
            }
            unsafe { run(&mut actual, count); }
            assert_eq!(actual.to_ne_bytes(), expected, "count={count}");
        }
    }

    #[test]
    fn conditional_xor_masks_preserve_integer_words_and_bit_positions() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let word = u64::MAX >> (u64::BITS - ty.bits());
            let sign = 1_u64 << (ty.bits() - 1);
            for tested in (0..ty.bits()).map(|bit| 1_u64 << bit).chain([0, word]) { for invert in [false, true] {
              for comparison in [None, Some(IntCC::Equal), Some(IntCC::NotEqual)] { for factored in [false, true] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.extend([AbiParam::new(types::I64); 3]);
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("conditional_xor"), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let args = builder.block_params(entry).to_vec();
                let values: Vec<_> = args.into_iter().map(|value| if ty == types::I64 { value }
                    else { builder.ins().ireduce(ty, value) }).collect();
                let condition = builder.ins().band_imm_u(values[0], tested as i64);
                let condition = comparison.map_or(condition, |cc| builder.ins().icmp_imm_u(cc, condition, 0));
                let result = if factored {
                    let zero = builder.ins().iconst(ty, 0);
                    let selected = if invert { builder.ins().select(condition, zero, values[2]) }
                        else { builder.ins().select(condition, values[2], zero) };
                    builder.ins().bxor(values[1], selected)
                } else {
                    let changed = builder.ins().bxor(values[1], values[2]);
                    if invert { builder.ins().select(condition, values[1], changed) }
                    else { builder.ins().select(condition, changed, values[1]) }
                };
                let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                if tested.is_power_of_two() {
                    assert!(!compiler.func.layout.blocks().flat_map(|block| compiler.func.layout.block_insts(block))
                        .any(|inst| compiler.func.dfg.insts[inst].opcode() == ir::Opcode::Select), "{ty} mask{tested} invert{invert}: {}", compiler.func);
                }
                let memory = CodeMemory::new(compiler.compiled_code().unwrap().buffer.data()).unwrap();
                // SAFETY: verified native ABI accepts and returns machine words; mapping stays live.
                let run: unsafe extern "C" fn(u64, u64, u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
                let sources: Vec<_> = if ty == types::I8 { (0..=word).collect() }
                    else { vec![0, 1, sign, word, tested, word ^ tested] };
                let deltas: Vec<_> = if ty == types::I8 { (0..=word).collect() }
                    else { vec![0, 1, sign, word, tested, word ^ tested] };
                for source in sources { for base in [0, 1, sign, word] { for &delta in &deltas {
                    let expected = base ^ if (source & tested != 0) ^ invert ^ (comparison == Some(IntCC::Equal)) { delta } else { 0 };
                    assert_eq!(unsafe { run(source, base, delta) }, expected, "{ty} mask{tested} invert{invert} factored{factored} source{source} base{base} delta{delta}");
                }}}
            }}}}
        }
    }

    #[test]
    fn signed_shift_pairs_preserve_width_masking_and_shared_inputs() {
        use cranelift_codegen::settings::Configurable;
        let mut flags = cranelift_codegen::settings::builder();
        // Exercise instruction selection directly, including widths for which
        // the middle-end can otherwise replace the pair with an IR extension.
        flags.set("opt_level", "none").unwrap();
        flags.set("enable_verifier", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            for amount in 0..width { for delta in [0, 1] { for shared in [false, true] { for masked in [false, true] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.push(AbiParam::new(types::I64));
                signature.params.push(AbiParam::new(types::I64));
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("signed_shift_pair"), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let input = builder.block_params(entry)[0];
                let filter = builder.block_params(entry)[1];
                let filter = if ty == types::I64 { filter } else { builder.ins().ireduce(ty, filter) };
                let input = if ty == types::I64 { input } else { builder.ins().ireduce(ty, input) };
                let left = builder.ins().ishl_imm_u(input, i64::from(amount) - i64::from(width));
                let right = builder.ins().sshr_imm_u(left, i64::from(amount + width + delta));
                let result = if shared { builder.ins().bxor(left, right) } else { right };
                let result = if masked { builder.ins().band(result, filter) } else { result };
                let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                let code = compiler.compiled_code().unwrap().buffer.data();
                #[cfg(target_arch = "aarch64")]
                if ty == types::I64 && amount != 0 && delta == 0 && !shared {
                    // A64 Bitfield class, ignoring sf/opc and operand fields.
                    const BITFIELD_CLASS_MASK: u32 = 0x1f80_0000;
                    const BITFIELD_CLASS: u32 = 0x1300_0000;
                    const SIGNED_BITFIELD_MASK: u32 = 0x7f80_0000;
                    let bitfields = code.chunks_exact(std::mem::size_of::<u32>())
                        .filter(|word| u32::from_le_bytes((*word).try_into().unwrap()) & BITFIELD_CLASS_MASK == BITFIELD_CLASS).count();
                    assert_eq!(bitfields, 1, "one signed extract replaces both shifts, amount={amount}");
                    assert!(code.chunks_exact(std::mem::size_of::<u32>()).any(|word|
                        u32::from_le_bytes(word.try_into().unwrap()) & SIGNED_BITFIELD_MASK == BITFIELD_CLASS),
                        "logical consumers must retain the signed extract, amount={amount} masked={masked}");
                }
                let memory = CodeMemory::new(code).unwrap();
                // SAFETY: verified two-word argument/one-word result ABI and live code.
                let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                let retained_sign = 1_u64 << (width - amount - 1);
                for value in (0_u64..=255).chain([retained_sign, retained_sign - 1, mask, mask >> 1, !mask, u64::MAX]) { for filter in [0, 1, mask, !mask] {
                    let left = value.wrapping_shl(amount) & mask;
                    let signed = (left << (u64::BITS - width)) as i64 >> (u64::BITS - width);
                    let right = (signed >> ((amount + delta) & (width - 1))) as u64 & mask;
                    let expected = if shared { left ^ right } else { right };
                    let expected = if masked { expected & filter } else { expected };
                    assert_eq!(unsafe { run(value, filter) }, expected, "{ty} shift={amount} delta={delta} shared={shared} masked={masked} value={value}");
                }}
            }}}}
        }
    }

    #[test]
    fn masked_integer_conditions_preserve_width_polarity_and_other_uses() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let word_mask = u64::MAX >> (u64::BITS - ty.bits());
            let sign_bit = 1_u64 << (ty.bits() - 1);
            for mask in [0, 1, sign_bit, sign_bit | 1, word_mask, 0x55 & word_mask] {
                for condition in [None, Some(IntCC::Equal), Some(IntCC::NotEqual)] {
                    let mut signature = Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64); 3]);
                    signature.returns.push(AbiParam::new(types::I64));
                    let mut function = ir::Function::with_name_signature(UserFuncName::testcase("masked_condition"), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let entry = builder.create_block();
                    builder.append_block_params_for_function_params(entry);
                    builder.switch_to_block(entry);
                    let args = builder.block_params(entry).to_vec();
                    let source = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                    let masked = builder.ins().band_imm_u(source, mask as i64);
                    let predicate = match condition {
                        None => masked,
                        Some(condition) => builder.ins().icmp_imm_u(condition, masked, 0),
                    };
                    let selected = builder.ins().select(predicate, args[1], args[2]);
                    // Keep a second consumer: folding the predicate must not remove its value.
                    let masked = if ty == types::I64 { masked } else { builder.ins().uextend(types::I64, masked) };
                    let result = builder.ins().bxor(selected, masked);
                    builder.ins().return_(&[result]);
                    builder.seal_all_blocks();
                    builder.finalize(isa.frontend_config());
                    let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                    let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, u64) -> u64>(code.pointer()) };
                    for source in (0..=255).chain([sign_bit, sign_bit - 1, word_mask, !word_mask, u64::MAX]) {
                        let masked = source & mask;
                        let choose_first = if condition == Some(IntCC::Equal) { masked == 0 } else { masked != 0 };
                        for (first, second) in [(0, u64::MAX), (sign_bit, 1)] {
                            let expected = (if choose_first { first } else { second }) ^ masked;
                            assert_eq!(unsafe { run(source, first, second) }, expected,
                                "{ty} source={source:x} mask={mask:x} condition={condition:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rotated_countdown_preserves_empty_loops_and_early_exit_values() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
          for observe_counter in [false, true] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::testcase("rotated_counter"), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let body = builder.create_block();
            let advance = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(header, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let mut words = Vec::new();
            for value in args { words.push(if ty == types::I64 { value } else { builder.ins().ireduce(ty, value) }); }
            let zero = builder.ins().iconst(ty, 0);
            builder.ins().jump(header, &[words[0].into(), zero.into()]);
            builder.switch_to_block(header);
            let counter = builder.block_params(header)[0];
            let checksum = builder.block_params(header)[1];
            let more = builder.ins().icmp(IntCC::SignedLessThan, counter, words[1]);
            builder.ins().brif(more, body, &[], exit, &[]);
            builder.switch_to_block(body);
            let stopped = builder.ins().icmp(IntCC::Equal, if observe_counter { counter } else { checksum }, words[2]);
            builder.ins().brif(stopped, exit, &[], advance, &[]);
            builder.switch_to_block(advance);
            let updated = if observe_counter { builder.ins().bxor(checksum, counter) }
                else { builder.ins().iadd_imm_u(checksum, 1) };
            let next = builder.ins().iadd_imm_u(counter, 1);
            builder.ins().jump(header, &[next.into(), updated.into()]);
            builder.switch_to_block(exit);
            let result = builder.ins().iadd(counter, checksum);
            let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            if !observe_counter {
                let converted = quench_runtime::vm::jit_backend::compile_function_reading(function.clone(), &*isa, None, false, |_, native| {
                    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
                    let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
                    let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
                    loops.compute(native, &cfg, &dom);
                    Ok(native.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some())
                        .flat_map(|block| native.layout.block_insts(block))
                        .any(|inst| native.dfg.insts[inst].opcode() == ir::Opcode::Isub))
                }).unwrap();
                assert!(converted, "{ty}: rotated loop must use shared countdown lowering");
            }
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, u64) -> u64>(code.pointer()) };
            let sign_bit = 1_u64 << (ty.bits() - 1);
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            let signed = |value:u64| ((value << (u64::BITS - ty.bits())) as i64) >> (u64::BITS - ty.bits());
            for start in [0, 1, sign_bit - 1, sign_bit, sign_bit + 1, mask] {
                for bound in [0, 1, sign_bit - 1, sign_bit, mask] {
                    for distance in [0, 1, 17, mask.min(257)] {
                        let stop = if observe_counter { start.wrapping_add(distance) & mask } else { distance & mask };
                        let (mut counter, mut checksum) = (start, 0);
                        while signed(counter) < signed(bound) && (if observe_counter { counter } else { checksum }) != stop {
                            checksum = if observe_counter { checksum ^ counter } else { checksum.wrapping_add(1) & mask };
                            counter = counter.wrapping_add(1) & mask;
                        }
                        assert_eq!(unsafe { run(start, bound, stop) }, counter.wrapping_add(checksum) & mask,
                            "{ty}: start={start:x} bound={bound:x} stop={stop:x}");
                    }
                }
            }
          }
        }
    }

    #[test]
    fn countdown_induction_preserves_words_checkpoints_and_labels() {
        use cranelift_codegen::settings::Configurable;
        use std::sync::atomic::AtomicU8;
        const MAX_EXECUTED_TRIPS: u128 = 257;
        const PREVIOUS_LABEL: u32 = 0;
        const NEXT_LABEL: u32 = 1;
        const PREVIOUS_USE: u32 = 101;
        const NEXT_USE: u32 = 102;
        let mut flags = cranelift_codegen::settings::builder();
        flags.set("opt_level", "speed").unwrap();
        flags.set("enable_verifier", "true").unwrap();
        flags.set("machine_code_cfg_info", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        let mut executions = 0;
        let mut optimized = 0;
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            let min = if width == i64::BITS { i64::MIN } else { -(1_i64 << (width - 1)) };
            let max = if width == i64::BITS { i64::MAX } else { (1_i64 << (width - 1)) - 1 };
            let starts: Vec<_> = if ty == types::I8 { (min..=max).collect() }
                else { vec![min, min + 1, -7, -1, 0, 1, max - 17, max - 1] };
            for step in [1_i64, -1] { for &start in &starts {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.extend([AbiParam::new(types::I64); 3]);
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("countdown_words"), signature);
                function.collect_debug_info();
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                let header = builder.create_block();
                let comparison = builder.create_block();
                let backedge = builder.create_block();
                let empty = builder.create_block();
                let done = builder.create_block();
                let interrupted = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.append_block_param(header, ty);
                builder.switch_to_block(entry);
                builder.set_srcloc(ir::SourceLoc::new(0));
                let args = builder.block_params(entry).to_vec();
                let bound = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                let initial = builder.ins().iconst(ty, start);
                let condition = if step > 0 { IntCC::SignedLessThan } else { IntCC::SignedGreaterThan };
                let enters = builder.ins().icmp(condition, initial, bound);
                builder.ins().brif(enters, header, &[initial.into()], empty, &[]);
                builder.switch_to_block(header);
                let previous = builder.block_params(header)[0];
                builder.set_val_label(previous, ir::ValueLabel::from_u32(PREVIOUS_LABEL));
                let next = builder.ins().iadd_imm_u(previous, step);
                builder.set_val_label(next, ir::ValueLabel::from_u32(NEXT_LABEL));
                let requested = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[1]);
                builder.ins().brif(requested, interrupted, &[], comparison, &[]);
                builder.switch_to_block(comparison);
                let continues = builder.ins().icmp(condition, next, bound);
                builder.ins().brif(continues, backedge, &[], done, &[]);
                builder.switch_to_block(backedge);
                builder.ins().jump(header, &[next.into()]);
                let widen = |builder: &mut FunctionBuilder<'_>, value| if ty == types::I64 { value }
                    else { builder.ins().uextend(types::I64, value) };
                for block in [interrupted, done] {
                    builder.switch_to_block(block);
                    builder.set_srcloc(ir::SourceLoc::new(PREVIOUS_USE));
                    let old = widen(&mut builder, previous);
                    builder.ins().store(MemFlagsData::trusted(), old, args[2], 0);
                    builder.set_srcloc(ir::SourceLoc::new(NEXT_USE));
                    let new = widen(&mut builder, next);
                    builder.ins().store(MemFlagsData::trusted(), new, args[2], std::mem::size_of::<u64>() as i32);
                    builder.set_srcloc(ir::SourceLoc::new(0));
                    builder.ins().return_(&[new]);
                }
                builder.switch_to_block(empty);
                let initial = widen(&mut builder, initial);
                builder.ins().store(MemFlagsData::trusted(), initial, args[2], 0);
                builder.ins().store(MemFlagsData::trusted(), initial, args[2], std::mem::size_of::<u64>() as i32);
                builder.ins().return_(&[initial]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                let code = compiler.compiled_code().unwrap();
                let memory = CodeMemory::new(code.buffer.data()).unwrap();
                for (label, location) in [(PREVIOUS_LABEL, PREVIOUS_USE), (NEXT_LABEL, NEXT_USE)] {
                    let uses: Vec<_> = code.buffer.get_srclocs_sorted().iter().filter(|source|
                        source.loc == ir::MaybeRelSourceLoc::abs(ir::SourceLoc::new(location)) && source.start < source.end).collect();
                    if uses.is_empty() { continue; }
                    let ranges = code.value_labels_ranges.get(&ir::ValueLabel::from_u32(label)).unwrap_or_else(||
                        panic!("{ty} start{start} missing label{label}: {:?}\n{}", compiler.func.dfg.values_labels, compiler.func));
                    for source in uses {
                        assert!(ranges.iter().any(|range| range.start <= source.start && source.start < range.end),
                            "{ty} start{start} label{label} at{}: {ranges:?}", source.start);
                    }
                }
                let native = &compiler.func;
                let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
                let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
                let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
                loops.compute(native, &cfg, &dom);
                let hot: Vec<_> = native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
                    .flat_map(|block| native.layout.block_insts(block)).collect();
                let countdown = hot.iter().filter(|inst| native.dfg.insts[**inst].opcode() == ir::Opcode::Isub).count();
                if start == 0 || (step > 0 && start == min) { assert_eq!(countdown, 1, "{native}"); }
                if countdown != 0 {
                    assert_eq!(countdown, 1, "{native}");
                    assert!(!hot.iter().any(|inst| matches!(native.dfg.insts[*inst], ir::InstructionData::IntCompare {
                        cond: IntCC::SignedLessThan | IntCC::SignedGreaterThanOrEqual
                            | IntCC::SignedGreaterThan | IntCC::SignedLessThanOrEqual, .. })), "{native}");
                    optimized += 1;
                }
                // SAFETY: the verified ABI takes a word, a live atomic byte,
                // and two writable word cells, returning one word.
                let run = unsafe { std::mem::transmute::<*mut u8,
                    unsafe extern "C" fn(u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
                let bounds: Vec<_> = if ty == types::I8 { (min..=max).collect() } else {
                    [min, start.saturating_sub(17), start.saturating_sub(2), start.saturating_sub(1), start, start.saturating_add(1), start.saturating_add(2),
                        start.saturating_add(17), max].into_iter().filter(|bound| *bound >= min && *bound <= max).collect()
                };
                for bound in bounds { for requested in [0_u8, 1] {
                    let distance = ((i128::from(bound) - i128::from(start)) * i128::from(step)).max(0) as u128;
                    let enters = distance != 0;
                    if requested == 0 && distance > MAX_EXECUTED_TRIPS { continue; }
                    let expected = if !enters { start } else if requested != 0 { start + step } else { bound };
                    let previous = if !enters || requested != 0 { start } else { bound - step };
                    let flag = AtomicU8::new(requested);
                    let mut cells = [0_u64; 2];
                    assert_eq!(unsafe { run(bound as u64 & mask, &flag, cells.as_mut_ptr()) }, expected as u64 & mask,
                        "{ty} start{start} bound{bound} step{step} request{requested}");
                    assert_eq!(cells, [previous as u64 & mask, expected as u64 & mask]);
                    executions += 1;
                }}
            }}
        }
        assert!(executions >= (1_usize << u8::BITS) * (1_usize << u8::BITS) * 4);
        assert!(optimized >= 3 * 4, "each width must optimize both directions from zero and increasing from signed minimum");
    }

    #[test]
    fn loop_carried_updates_reuse_the_comparison_value() {
        use std::sync::atomic::AtomicU8;
        use cranelift_codegen::settings::Configurable;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLCU");
        const BOUND_LABEL_INDEX: u32 = 0;
        let bound_label = ir::ValueLabel::from_u32(BOUND_LABEL_INDEX);
        const PROBE_SOURCE_OFFSET: u32 = 42;
        let probe_location = ir::SourceLoc::new(PROBE_SOURCE_OFFSET);
        let mut flags = cranelift_codegen::settings::builder();
        flags.set("opt_level", "speed").unwrap();
        flags.set("machine_code_cfg_info", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        for ty in [types::I32, types::I64] { for step in [1_i64, -1, 7, -7] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 4]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
            function.collect_debug_info();
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let comparison = builder.create_block();
            let backedge = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(exit, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let narrow = |builder: &mut FunctionBuilder<'_>, value| if ty == types::I32 {
                builder.ins().ireduce(ty, value)
            } else { value };
            let start = narrow(&mut builder, args[0]);
            let bound = narrow(&mut builder, args[1]);
            builder.set_srcloc(ir::SourceLoc::new(0));
            builder.set_val_label(bound, bound_label);
            let empty = builder.ins().icmp(IntCC::Equal, start, bound);
            builder.ins().brif(empty, exit, &[start.into()], header, &[start.into()]);
            builder.switch_to_block(header);
            let previous = builder.block_params(header)[0];
            let next = builder.ins().iadd_imm_u(previous, step);
            let stop = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[2]);
            builder.ins().brif(stop, exit, &[next.into()], comparison, &[]);
            builder.switch_to_block(comparison);
            let done = builder.ins().icmp(IntCC::Equal, next, bound);
            builder.ins().brif(done, exit, &[next.into()], backedge, &[]);
            builder.switch_to_block(backedge);
            builder.set_srcloc(probe_location);
            let observed = if ty == types::I32 { builder.ins().uextend(types::I64, bound) } else { bound };
            builder.ins().store(MemFlagsData::trusted(), observed, args[3], 0);
            builder.set_srcloc(ir::SourceLoc::new(0));
            builder.ins().jump(header, &[next.into()]);
            builder.switch_to_block(exit);
            let result = builder.block_params(exit)[0];
            let result = if ty == types::I32 { builder.ins().uextend(types::I64, result) } else { result };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let mut compiler = cranelift_codegen::Context::for_function(function);
            compiler.set_disasm(true);
            let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
            let locations = compiled.value_labels_ranges.get(&bound_label).expect("live bound label");
            assert!(!locations.is_empty());
            assert!(locations.windows(2).all(|pair| pair[0].end <= pair[1].start));
            let probes: Vec<_> = compiled.buffer.get_srclocs_sorted().iter()
                .filter(|source| source.loc == ir::MaybeRelSourceLoc::abs(probe_location) && source.start < source.end).collect();
            assert!(!probes.is_empty(), "the relocated backedge must retain its probe");
            for probe in probes {
                assert!(locations.iter().any(|range| range.start <= probe.start && probe.end <= range.end),
                    "bound debug location must cover its relocated use: {}..{}, {locations:?}", probe.start, probe.end);
            }
            assert!(compiled.bb_starts.windows(2).all(|pair| pair[0] < pair[1]));
            #[cfg(target_arch = "aarch64")]
            assert_aarch64_cfg_edges(compiled);
            let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
            let native = &compiler.func;
            let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
            let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
            let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
            loops.compute(native, &cfg, &dom);
            assert_eq!(loops.loops().count(), 1);
            let updates = native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
                .flat_map(|block| native.layout.block_insts(block))
                .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Iadd).count();
            assert_eq!(updates, 1, "one update serves comparison and backedge: {ty} step{step}\n{native}");
            // SAFETY: the verified function has four word inputs and one word
            // output; the pointers name a live atomic byte and writable probe.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            for seed in [0_u64, 1, u64::MAX, i32::MAX as u64, i64::MAX as u64] {
                for trips in [0_u64, 1, 2, 17, 257] { for requested in [0_u8, 1] {
                    let seed = seed & mask;
                    let bound = seed.wrapping_add(trips.wrapping_mul(step as u64)) & mask;
                    let flag = AtomicU8::new(requested);
                    let expected = if trips == 0 { seed } else if requested != 0 {
                        seed.wrapping_add(step as u64) & mask
                    } else { bound };
                    let mut probe = 0;
                    assert_eq!(unsafe { run(seed, bound, &flag, &mut probe) }, expected,
                        "{ty} step{step} seed{seed} trips{trips} stop{requested}");
                    assert_eq!(probe, if trips > 1 && requested == 0 { bound } else { 0 });
                }}
            }
        }}
    }

    #[test]
    fn loop_carried_updates_share_branch_store_and_backedge_placement() {
        use std::sync::atomic::AtomicU8;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLSP");
        const FEEDBACK: u64 = 0xD008;
        const STORED_MASK: u64 = 0x55;
        const UPDATE_LABEL: u32 = 0;
        const STORE_SOURCE_OFFSET: u32 = 42;
        let update_label = ir::ValueLabel::from_u32(UPDATE_LABEL);
        let store_location = ir::SourceLoc::new(STORE_SOURCE_OFFSET);
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for ty in [types::I32, types::I64] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 5]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
            function.collect_debug_info();
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let body = builder.create_block();
            let odd = builder.create_block();
            let even = builder.create_block();
            let join = builder.create_block();
            let left = builder.create_block();
            let right = builder.create_block();
            let latch = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(header, types::I64);
            builder.append_block_param(join, ty);
            builder.append_block_param(join, ty);
            builder.append_block_param(exit, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let seed = if ty == types::I32 { builder.ins().ireduce(ty, args[0]) } else { args[0] };
            builder.ins().brif(args[1], header, &[seed.into(), args[1].into()], exit, &[seed.into()]);
            builder.switch_to_block(header);
            let previous = builder.block_params(header)[0];
            let count = builder.block_params(header)[1];
            let stop = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[4]);
            builder.ins().brif(stop, exit, &[previous.into()], body, &[]);
            builder.switch_to_block(body);
            let shifted = builder.ins().ushr_imm_u(previous, 1);
            let low = builder.ins().band_imm_u(previous, 1);
            builder.ins().brif(low, odd, &[], even, &[]);
            builder.switch_to_block(odd);
            let feedback = builder.ins().iconst(ty, FEEDBACK as i64);
            builder.ins().jump(join, &[shifted.into(), feedback.into()]);
            builder.switch_to_block(even);
            let zero = builder.ins().iconst(ty, 0);
            builder.ins().jump(join, &[shifted.into(), zero.into()]);
            builder.switch_to_block(join);
            let operands = [builder.block_params(join)[0], builder.block_params(join)[1]];
            let next = builder.ins().bxor(operands[0], operands[1]);
            builder.set_val_label(next, update_label);
            let wide = if ty == types::I32 { builder.ins().uextend(types::I64, next) } else { next };
            builder.ins().brif(args[2], left, &[], right, &[]);
            builder.switch_to_block(left);
            builder.set_srcloc(store_location);
            builder.ins().store(MemFlagsData::trusted(), wide, args[3], 0);
            builder.ins().jump(latch, &[]);
            builder.switch_to_block(right);
            let encoded = builder.ins().bxor_imm_u(wide, STORED_MASK as i64);
            builder.ins().store(MemFlagsData::trusted(), encoded, args[3], core::mem::size_of::<u64>() as i32);
            builder.ins().jump(latch, &[]);
            builder.switch_to_block(latch);
            builder.set_srcloc(ir::SourceLoc::new(0));
            let remaining = builder.ins().iadd_imm_s(count, -1);
            builder.ins().brif(remaining, header, &[next.into(), remaining.into()], exit, &[next.into()]);
            builder.switch_to_block(exit);
            let result = builder.block_params(exit)[0];
            let result = if ty == types::I32 { builder.ins().uextend(types::I64, result) } else { result };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let mut compiler = cranelift_codegen::Context::for_function(function);
            let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
            let locations = compiled.value_labels_ranges.get(&update_label).expect("live update label");
            let probes: Vec<_> = compiled.buffer.get_srclocs_sorted().iter()
                .filter(|source| source.loc == ir::MaybeRelSourceLoc::abs(store_location) && source.start < source.end).collect();
            assert!(!probes.is_empty());
            assert!(probes.iter().all(|probe| locations.iter().any(|range|
                range.start <= probe.start && probe.end <= range.end)), "update label covers both storage arms");
            let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
            let native = &compiler.func;
            let shifts = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
                .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Ushr).count();
            assert_eq!(shifts, 1, "one producer serves both stores and the next iteration: {ty}\n{native}");
            // SAFETY: the verified ABI takes three words and live pointers to
            // two writable words and an atomic stop byte, returning a word.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64, u64, *mut u64, *const AtomicU8) -> u64>(memory.pointer()) };
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            for seed in [0, 1, 123456789, u64::MAX, i64::MIN as u64] {
                for trips in [0, 1, 2, 17, 257] { for arm in [0, 1] { for requested in [0, 1] {
                    let seed = seed & mask;
                    let flag = AtomicU8::new(requested);
                    let mut cells = [0_u64; 2];
                    let expected = (0..if requested == 0 { trips } else { 0 }).fold(seed, |value, _|
                        (value >> 1) ^ if value & 1 != 0 { FEEDBACK } else { 0 });
                    assert_eq!(unsafe { run(seed, trips, arm, cells.as_mut_ptr(), &flag) }, expected);
                    let expected_cells = if requested != 0 || trips == 0 { [0, 0] }
                        else if arm != 0 { [expected, 0] } else { [0, expected ^ STORED_MASK] };
                    assert_eq!(cells, expected_cells, "{ty} seed{seed} trips{trips} arm{arm} stop{requested}");
                }}}
            }
        }
    }

    #[test]
    fn loop_constants_share_conditional_entries_and_preserve_exact_exits() {
        use std::sync::atomic::AtomicU8;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLCP");
        const FEEDBACK: u64 = 0xD008;
        const UNTOUCHED: u64 = 0x55;
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for ty in [types::I32, types::I64] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 5]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let choose = builder.create_block();
            let headers = [builder.create_block(), builder.create_block()];
            let bodies = [builder.create_block(), builder.create_block()];
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            for header in headers {
                builder.append_block_param(header, ty);
                builder.append_block_param(header, types::I64);
            }
            builder.append_block_param(exit, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let seed = if ty == types::I32 { builder.ins().ireduce(ty, args[0]) } else { args[0] };
            let empty = builder.ins().icmp_imm_s(IntCC::SignedLessThanOrEqual, args[1], 0);
            builder.ins().brif(empty, exit, &[seed.into()], choose, &[]);
            builder.switch_to_block(choose);
            builder.ins().brif(args[2], headers[0], &[seed.into(), args[1].into()], headers[1], &[seed.into(), args[1].into()]);
            for arm in 0..headers.len() {
                builder.switch_to_block(headers[arm]);
                let previous = builder.block_params(headers[arm])[0];
                let count = builder.block_params(headers[arm])[1];
                let stop = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[3]);
                builder.ins().brif(stop, exit, &[previous.into()], bodies[arm], &[]);
                builder.switch_to_block(bodies[arm]);
                let feedback = builder.ins().iconst(ty, FEEDBACK as i64);
                let next = if arm == 0 { builder.ins().bxor(previous, feedback) }
                    else { builder.ins().iadd(previous, feedback) };
                let observed = if ty == types::I32 { builder.ins().uextend(types::I64, next) } else { next };
                builder.ins().store(MemFlagsData::trusted(), observed, args[4], 0);
                let remaining = builder.ins().iadd_imm_s(count, -1);
                builder.ins().brif(remaining, headers[arm], &[next.into(), remaining.into()], exit, &[next.into()]);
            }
            builder.switch_to_block(exit);
            let result = builder.block_params(exit)[0];
            let result = if ty == types::I32 { builder.ins().uextend(types::I64, result) } else { result };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let mut compiler = cranelift_codegen::Context::for_function(function);
            let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
            let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
            cranelift_codegen::verify_function(&compiler.func, &*isa).unwrap();
            compiler.flowgraph();
            compiler.compute_loop_analysis();
            let native = &compiler.func;
            assert_eq!(compiler.loop_analysis.loops().count(), 2, "distinct recurrences retain both versions: {native}");
            let live_literals: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block).map(move |inst| (block, inst)))
                .filter_map(|(block, inst)| {
                    if !matches!(native.dfg.insts[inst], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                        if imm.bits() as u64 == FEEDBACK) { return None; }
                    let value = native.dfg.first_result(inst);
                    let uses: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block).map(move |inst| (block, inst)))
                        .filter(|(_, user)| native.dfg.inst_values(*user).any(|arg| native.dfg.resolve_aliases(arg) == value))
                        .map(|(block, _)| block).collect();
                    (!uses.is_empty()).then_some((block, uses))
                }).collect();
            assert_eq!(live_literals.len(), 1, "one literal serves both loop versions: {native}");
            let (parent, uses) = &live_literals[0];
            assert!(compiler.loop_analysis.innermost_loop(*parent).is_none());
            for lp in compiler.loop_analysis.loops() {
                let header = compiler.loop_analysis.loop_header(lp);
                assert_eq!(compiler.domtree.idom(header), Some(*parent));
                assert!(uses.iter().any(|block| compiler.loop_analysis.is_in_loop(*block, lp)), "shared constant reaches {lp}");
            }
            // SAFETY: the verified ABI receives three scalar words and live
            // pointers to an atomic byte and writable word; it returns one word.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, i64, u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            for seed in [0_u64, 1, i32::MAX as u64, i64::MAX as u64, u64::MAX] {
                for trips in [-1_i64, 0, 1, 2, 17, 257] { for arm in [0, 1] { for requested in [0, 1] {
                    let flag = AtomicU8::new(requested);
                    let mut observed = UNTOUCHED;
                    let actual_trips = if requested == 0 { trips.max(0) } else { 0 };
                    let expected = (0..actual_trips).fold(seed & mask, |value, _|
                        (if arm != 0 { value ^ FEEDBACK } else { value.wrapping_add(FEEDBACK) }) & mask);
                    assert_eq!(unsafe { run(seed, trips, arm, &flag, &mut observed) }, expected,
                        "{ty}, seed={seed}, trips={trips}, arm={arm}, stop={requested}");
                    assert_eq!(observed, if actual_trips == 0 { UNTOUCHED } else { expected });
                } } }
            }
        }
    }

    #[test]
    fn slot_widening_unlinks_compiled_readers_without_releasing_active_code() {
        let program = quench_runtime::Engine::specialize(
            "var receiver={edgeValue:1}; function read(){var n=0;for(var i=0;i<5;i++)n=(n+receiver.edgeValue)|0;return n;} read();",
            "slot-reader-retirement.js").unwrap();
        let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        vm.programs.enable_js_lane_methods();
        let function = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "read")).unwrap();
        let method = vm.compile_lane_method(&program, function as u32,
            &|_, _| Vec::new(), &|_| (0, 0), Vec::new(), &[]).unwrap();
        assert!(!method.dependencies.slots.is_empty(), "the actual field read must subscribe");
        let published = { let (mut facts, programs) = vm.fact_watch(); programs.publish_lane_method(method, &mut facts) }.unwrap();
        let active = published.lease().unwrap();
        let weak = Rc::downgrade(&active);
        let receiver_atom = vm.intern_atom("receiver");
        let receiver = vm.own_property(vm.realm.globals, receiver_atom).unwrap();
        let atom = vm.intern_atom("edgeValue");
        let shape = vm.heap_reads().object_ref(receiver).unwrap().shape();
        let slot = vm.shape_slot(shape, atom).unwrap();
        vm.store_property_slot(receiver, slot, Value::integer(2));
        assert!(published.lease().is_some());
        vm.store_property_slot(receiver, slot, Value::number(0.5));
        assert!(published.lease().is_none(), "widening retires future entry at the write");
        assert_eq!(vm.heap_reads().property_get_ref(vm.heap_reads().object_ref(receiver).unwrap(), slot), Some(Value::number(0.5)));
        assert!(weak.upgrade().is_some(), "retirement preserves the active code lease");
        drop(active);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn numeric_result_signature_joins_typed_returns_and_preserves_float_values() {
        let integer = "function choose(n,x){n=n|0;x=x|0;if(n<0)return x|0;for(let i=0;i<n;i=(i+1)|0)x=(x+1)|0;return x|0;}";
        let mixed = "function choose(n,x){n=n|0;if(n<0)return x|0;return x*1;}";
        for typescript in [false, true] {
            for (source, format) in [(integer, NativeNumberResult::Int32), (mixed, NativeNumberResult::Float64)] {
                let source = if typescript { source.replace("(n,x)", "(n:number,x:number):number") } else { source.to_owned() };
                let program = if typescript { quench_runtime::Engine::specialize_typescript(&source, "result-signature.ts") }
                    else { quench_runtime::Engine::specialize(&source, "result-signature.js") }.unwrap();
                let artifact = compile_js_typed_numeric_region(&program, 1).expect("shared typed return region");
                assert_eq!(artifact.result, format);
                let compiled = CompiledJsFunction::from_artifact(artifact).unwrap();
                let interrupt = quench_runtime::ExecutionInterrupt::default();
                for n in [-1, 0, 3] { for x in [i32::MIN, -1, 0, i32::MAX] {
                    let expected = if format == NativeNumberResult::Int32 && n >= 0 { x.wrapping_add(n) } else { x };
                    let result = direct_call(&compiled, &[Value::integer(n), Value::integer(x)], &interrupt);
                    assert!(matches!(result, Some(value) if value.to_bits() == Value::integer(expected).to_bits()),
                        "{source}:n={n},x={x}");
                }}
                if format == NativeNumberResult::Float64 {
                    for x in [-0.0, 1.5, f64::MIN_POSITIVE, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                        let result = direct_call(&compiled, &[Value::integer(1), Value::number(x)], &interrupt);
                        assert!(matches!(result, Some(value) if value.to_bits() == Value::number(x).to_bits()),
                            "mixed result preserves {x:?}");
                    }
                }
                for value in [Value::UNDEFINED, Value::TRUE, Value::NULL] {
                    assert!(direct_call(&compiled, &[value, Value::integer(1)], &interrupt).is_none(), "TS annotations are not input proofs");
                }
                interrupt.request_termination();
                assert!(direct_call(&compiled, &[Value::integer(1), Value::integer(1)], &interrupt).is_none());
            }
        }
    }

    #[test]
    fn native_widening_edges_retire_readers_before_guest_store() {
        #[derive(Clone, Copy)]
        enum Edge { Store, Domain, Pointer }
        for edge in [Edge::Store, Edge::Domain, Edge::Pointer] {
            let program = quench_runtime::Engine::specialize(
                "var receiver={edgeValue:1,a:2,b:3}; function read(){var n=0;for(var i=0;i<5;i++)n=(n+receiver.edgeValue)|0;return n;} function write(x){receiver.edgeValue=x;} function make(n){return function store(){this.edgeValue=n;};} var setter=make(0.5);setter.call(receiver);write(1);read();",
                "native-widening-edge.js").unwrap();
            let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
            quench_runtime::test_support::execute(&mut vm, &program).unwrap();
            vm.programs.enable_js_lane_methods();
            let reader = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "read")).unwrap();
            let writer = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "write")).unwrap();
            // merge: HEAD found the register-receiver store (`base: Some`, not strict).
            let store = program.function(writer).instructions().find(|i| matches!(quench_runtime::staged::Operation::<u64, ()>::of(*i),
                Some(quench_runtime::staged::Operation::SetField { base: Some(_), strict: false, .. }))).unwrap();
            let method = vm.compile_lane_method(&program, reader as u32,
                &|_, _| Vec::new(), &|_| (0, 0), Vec::new(), &[]).unwrap();
            assert!(!method.dependencies.slots.is_empty());
            let published = { let (mut facts, programs) = vm.fact_watch(); programs.publish_lane_method(method, &mut facts) }.unwrap();
            let active = published.lease().unwrap();
            let atom = vm.intern_atom("receiver");
            let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
            let shape = vm.heap_reads().object_ref(receiver).unwrap().shape();
            let slot = vm.shape_slot(shape, store.atom_index()).unwrap();
            let residual = (&program as *const ResidualProgram).addr() as u64;
            let machine = (&mut vm as *mut Vm).addr() as u64;
            // Each edge uses the same live VM/program and exact slot as a native
            // region; rejection must precede the guest store or raw pointer escape.
            let admitted = unsafe {
                match edge {
                    Edge::Domain => field_write_admission_in_lane(machine, residual,
                        shape as u64, slot as u64, quench_runtime::typed_ir::SlotType::from_type(quench_runtime::typed_ir::Type::NUMBER) as u64),
                    Edge::Store => field_set_in_lane(machine, residual, receiver,
                        store.atom_index() as u64, store.cache_site_index() as u64, Value::number(0.5)),
                    Edge::Pointer => u64::from(vm.cached_field_store_pointer(&program, receiver,
                        store.atom_index(), store.cache_site_index(), quench_runtime::typed_ir::Type::NUMBER).is_some()),
                }
            };
            assert_eq!(admitted, 0, "widening must exit before mutation");
            assert!(published.lease().is_none());
            assert_eq!(vm.heap.slot_type(shape, slot), quench_runtime::typed_ir::Type::NUMBER);
            assert_eq!(vm.heap_reads().property_get_ref(vm.heap_reads().object_ref(receiver).unwrap(), slot), Some(Value::integer(1)));
            vm.store_property_slot(receiver, slot, Value::number(0.5));
            assert!(vm.admit_slot_writes_as(shape, slot, quench_runtime::typed_ir::Type::NUMBER, false),
                "the wider domain is admitted on the next entry");
            assert_eq!(vm.heap.unrepresented_slots(), 0);
            drop(active);
        }
    }

    #[test]
    fn jit_loop_slots_preserve_widening_and_reentry() {
        let source = concat!("run({count:4096,value:0},1);\n",
            include_str!("../../quench-runtime/tests/fixtures/loop-slot-lifetime.js"), "\nthrow 42;");
        for typescript in [false, true] {
            let program = if typescript { quench_runtime::Engine::specialize_typescript(source, "jit-loop-slots.cts") }
                else { quench_runtime::Engine::specialize(source, "jit-loop-slots.cjs") }.unwrap();
            let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::Compiled);
            let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{error}");
        }
    }

    #[test]
    fn bound_field_arena_preserves_slot_bounds() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for bound in [false, true] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 4]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut native, &mut context);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let p = builder.block_params(entry).to_vec();
            let arena = FieldArena::Unbound { values: p[0], length: p[1] };
            let arena = if bound { arena.bind(&mut builder, p[3]) } else { arena };
            let miss = builder.create_block();
            let address = arena.address(&mut builder, p[2], FieldSlot::Bound(p[3]), miss);
            builder.ins().return_(&[address]);
            builder.switch_to_block(miss);
            let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
            builder.ins().return_(&[deleted]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: this function performs integer address arithmetic only and
            // never dereferences the synthetic arena or returned address.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64, u64, u64) -> u64>(memory.pointer()) };
            let width = quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.property_value_size as u64;
            let base = width;
            for (length, storage, slot) in [
                (0, 0, 0), (1, 0, 0), (1, 0, 1), (1, 0, 2),
                (3, 1, 1), (3, 2, 1),
                (u64::from(u32::MAX), u64::from(u32::MAX), 0),
                (u64::from(u32::MAX) + u64::from(u16::MAX) + 1,
                    u64::from(u32::MAX), u64::from(u16::MAX)),
            ] {
                let expected = if storage + slot < length { base + (storage + slot) * width }
                    else { Value::DELETED.to_bits() };
                assert_eq!(unsafe { run(base, length, storage, slot) }, expected,
                    "bound={bound}, length={length}, storage={storage}, slot={slot}");
            }
        }
    }

    #[test]
    fn nullable_slot_observations_keep_checks_across_execution_tiers() {
        quench_runtime::test_support::on_compile_stack(|| {
            let source = include_str!("../../quench-runtime/tests/fixtures/nullable-slot-writes.js");
            let program = quench_runtime::Engine::specialize(source, "nullable-slot-writes.js").unwrap();
            let native = quench_runtime::NativeJsProgram::compile_eager(program.clone()).unwrap();
            for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled, ExecutionMode::NativeOnly] {
                let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
                vm.programs.enable_js_lane_methods();
                vm.set_execution_mode(mode);
                if mode == ExecutionMode::NativeOnly { vm.execute_native_program(&native, true).unwrap(); }
                else { quench_runtime::test_support::execute(&mut vm, &program).unwrap(); }
                let atom = vm.lookup_atom("answer").unwrap();
                // Local Node oracle: 3000 warm writes, 12 widening cases, 64 later writes.
                assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::integer(3076)));
            }
        });
    }

    #[test]
    fn native_holder_read_does_not_require_receiver_arena_storage() {
        for (source, inline_holder) in [
            ("function read(o){return o.method;} function Box(){} Box.prototype.method=42; Box.prototype.pad=0; globalThis.box=new Box(); box.own=1; read(box);", false),
            ("function read(o){return o.method;} function Box(){} Box.prototype.method=42; globalThis.box=new Box(); box.own=1; read(box);", true),
        ] {
        let program = quench_runtime::Engine::specialize(source, "packed-holder.js").unwrap();
        let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let atom = vm.lookup_atom("box").unwrap();
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        assert!(vm.heap_reads().object_ref(receiver).unwrap().record().inline_properties().is_some());
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let function = program.functions.iter().position(|function|
            function.header().name.is_some_and(|name| &program.atoms[name as usize] == "read")).unwrap();
        let site = program.function(function).instructions().find_map(|instruction| match instruction.field_lookup() {
            Some(FieldLookup::Atom { cache_site, .. }) => Some(cache_site), _ => None,
        }).unwrap();
        let cache = vm.field_cache_entry(site);
        assert_ne!(cache.holder, quench_runtime::vm::NO_FIELD_HOLDER);
        assert_eq!(vm.record_field_entry_hit(cache, receiver), Some(Value::number(42.0)));
        let prototype = vm.heap_reads().object_ref(receiver).unwrap().proto();
        let holder = vm.heap_reads().object_ref(prototype).unwrap();
        assert_eq!(holder.exact_shape(), cache.holder);
        assert_eq!(holder.inline_properties().is_some(), inline_holder);
        assert_eq!(holder.extras_storage_start().is_some(), !inline_holder);

        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 4]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let mut binding = FieldSlotBinding {
            receiver: builder.ins().iconst(types::I32, i64::from(cache.receiver)),
            holder: builder.ins().iconst(types::I32, i64::from(cache.holder)),
            slot: FieldSlot::Bound(builder.ins().iconst(types::I64, i64::from(cache.slot))),
            records: parameters[0],
            arena: FieldArena::Unbound { values: parameters[1], length: parameters[2] },
            writes: None, admitted: None,
        };
        let slot = binding.slot.read(&mut builder);
        binding.arena = binding.arena.bind(&mut builder, slot);
        let miss = builder.create_block();
        let unused = builder.ins().iconst(types::I64, 0);
        let (address, _) = emit_js_cached_field_slot(&mut builder, unused,
            parameters[3], site, true, miss, Some(binding), None, None);
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
        builder.ins().return_(&[value]);
        builder.switch_to_block(miss);
        let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
        builder.ins().return_(&[deleted]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the four inputs name this VM's live reservation, arena and
        // receiver. No guest call or collection occurs while the code runs.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(usize, *const Value, usize, u64) -> u64>(memory.pointer()) };
        let (values, length) = vm.heap_reads().native_property_values();
        let records = vm.heap_reads().object_record_base().unwrap() as usize;
        assert_eq!(Value::from_bits(unsafe { run(records, values, length, receiver.to_bits()) }), Value::number(42.0));
        // A receiver that has not proved the cached shape cannot read this slot.
        assert_eq!(unsafe { run(records, values, length, Value::NULL.to_bits()) }, Value::DELETED.to_bits());
        assert_eq!(unsafe { run(records, values, length, vm.realm.globals.to_bits()) }, Value::DELETED.to_bits());
        }
    }

    #[test]
    fn activation_admits_only_authoritative_this_and_dense_function_captures() {
        let mut vm = semantic_vm("throw 7;");
        let receiver = vm.object();
        vm.frames[0].this = receiver;
        vm.frames[0].fixed_this = false;
        JsStepDispatch::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, _| {
            assert!(activation.fixed_this_slot.is_null());
        }).unwrap();
        vm.frames[0].fixed_this = true;
        let slots = quench_runtime::tier_surface::EnvironmentSlots::from(vec![Value::number(7.0)]);
        vm.frames[0].capture_cache = quench_runtime::vm::CaptureCache {
            env: vm.frames[0].env, captured: vm.frames[0].captured,
            epoch: vm.cache_epochs.gc, slots: &slots, root: None,
        };
        JsStepDispatch::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, vm| {
            assert_eq!(activation.fixed_this_slot, &vm.frames[0].this as *const Value);
            assert!(quench_bjit::shared_words::capture_values(activation.captures_word()) != 0);
        }).unwrap();
        vm.frames[0].capture_cache.root = Some(Some(ProgramId::MAIN.raw()));
        JsStepDispatch::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, _| {
            assert_eq!(quench_bjit::shared_words::capture_values(activation.captures_word()), 0);
        }).unwrap();
    }

    #[test]
    fn typed_native_loops_preserve_stores_in_native_only_and_unspecialized_execution() {
        let source = "function tail(node) { while(node.next!==null) { node.mark=node.mark+1; node=node.next; } return node; } const end={next:null,mark:0}; const head={next:end,mark:0}; for(let i=0;i<40;i++) { if(tail(head)!==end) throw 'identity'; } throw head.mark;";
        for specialized in [false, true] {
            let mut program = quench_runtime::Engine::specialize(source, "native-loop-store.js").unwrap();
            program.specialized = specialized;
            let native = quench_runtime::NativeJsProgram::compile_eager(program).unwrap();
            let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(40.0)));
        }
    }


    #[test]
    #[allow(dangerous_implicit_autorefs)] // the test reads and restores the frame header through its raw pointer
    fn nested_lane_entry_restores_outer_fault_binding() {
        if !FAULT_POLL_SUPPORTED { return; }
        let program = quench_runtime::Engine::specialize(
            "function count(n){n=n|0;let sum=0;for(let i=0;i<n;i=(i+1)|0)sum=(sum+1)|0;return sum;}",
            "nested-poll-binding.js").unwrap();
        let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
        let method = vm.compile_lane_method(&program, 1, &|_, _| Vec::new(),
            &|_| (0, 0), Vec::new(), &[]).expect("polling lane body");
        assert!(method.fault_poll);
        let frame = frame_in_lane();
        // The real outer landing must survive an inner trampoline overwriting it.
        // This sentinel is never entered: the inner entry installs its own landing.
        let outer_landing = (&program as *const ResidualProgram).addr();
        let original_landing = unsafe { std::mem::replace(&mut (*frame).context.entry_stack, outer_landing) };
        // These outer program inputs are never dereferenced by the nested body:
        // entry must replace them, then restore them before returning to the caller.
        let original_program = unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) };
        let outer_program = (ProgramId::from_raw(1), (&program as *const ResidualProgram).addr());
        unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) = outer_program; }
        let before = unsafe {
            let context = &(*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
        };
        let mut registers = vec![Value::number(7.0); usize::from(method.caller_registers)];
        let result = vm.run_nested_lane_method(&method, ProgramId::MAIN, &program,
            registers.as_mut_ptr(), Value::DELETED, std::ptr::null(), std::ptr::null_mut());
        assert!(matches!(result, LaneExit::Returned { value, .. } if value == Value::number(7.0)));
        let after = unsafe {
            let context = &(*frame).context;
            (context.stack_limit, context.entry_stack, context.interrupt,
                context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
        };
        assert_eq!(before, after, "outer native recovery binding is borrowed, not replaced");
        assert_eq!(unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) }, outer_program,
            "the nested entry must restore the outer program's field table");
        unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) = original_program; }
        unsafe { (*frame).context.entry_stack = original_landing; }

        unsafe extern "C" fn outer(context: *mut CallContext, _: u64, _: u64,
            arguments: *const u64, out: *mut u64) -> u32 {
            // Only borrowed raw inputs live across the poll: recovery skips no destructor.
            let inputs = unsafe { std::slice::from_raw_parts(arguments, 4) };
            let vm = unsafe { &mut *(inputs[0] as *mut Vm) };
            let method = unsafe { &*(inputs[1] as *const LaneMethod) };
            let program = unsafe { &*(inputs[2] as *const ResidualProgram) };
            let registers = inputs[3] as *mut Value;
            let result = vm.run_nested_lane_method(method, ProgramId::MAIN, program,
                registers, Value::DELETED, std::ptr::null(), std::ptr::null_mut());
            let LaneExit::Returned { value, .. } = result else { return NATIVE_JS_STOPPED; };
            unsafe { out.write(value.to_bits()); }
            vm.execution_interrupt.request_termination();
            // The restored outer landing must own this fault, after the inner landing died.
            let poll = unsafe { (*context).poll_start as *const std::sync::atomic::AtomicBool };
            unsafe { (*poll).load(std::sync::atomic::Ordering::Acquire); }
            0
        }
        let inputs = [&mut vm as *mut _ as u64, &method as *const _ as u64,
            &program as *const _ as u64, registers.as_mut_ptr() as u64];
        let page = native_poll_page(&vm.execution_interrupt).expect("owned poll page");
        let context = unsafe { &mut (*frame).context };
        context.bind_native_adapter(context.adapter_payload,
            vm.execution_interrupt.attention_flag(), Some(page));
        let mut out = 0;
        let status = unsafe { enter_wasm(context, 0, 0, inputs.as_ptr(), &mut out, outer) };
        assert_eq!(i64::from(status), TERMINATED_STATUS);
        assert_eq!(Value::from_bits(out), Value::number(7.0));
    }

    #[test]
    fn numeric_jit_shared_entry_observes_live_termination() {
        use std::{sync::mpsc, thread, time::Duration};
        const TIMEOUT: Duration = Duration::from_secs(5);
        fn started(context: &mut quench_runtime::NativeContext<'_>,
            _: quench_runtime::RootId, _: &[quench_runtime::RootId]) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
            context.host_as::<quench_runtime::test_host::TestHost>().state::<mpsc::Sender<()>>().send(()).unwrap();
            Ok(context.undefined())
        }
        let (ready, entered) = mpsc::channel::<()>();
        let (send_handle, receive_handle) = mpsc::channel();
        let (complete, finished) = mpsc::channel();
        let worker = thread::Builder::new().stack_size(quench_runtime::WORKER_STACK_SIZE).spawn(move || {
            let program = quench_runtime::Engine::specialize(r#"
                function spin(n){let x=1;for(let i=0;i<n;i=(i+1)|0)x=x^(x<<13);return x;}
                for(let warm=0;warm<64;warm++)spin(1);
                started();spin(Infinity);
            "#, "numeric-jit-interrupt.js").unwrap();
            let host = quench_runtime::test_host::TestHost::default()
                .with_natives(quench_runtime::host_functions![global "started" (0) => started]).with_state(Rc::new(ready));
            let mut vm = quench_runtime::test_support::vm(host);
            vm.set_execution_mode(ExecutionMode::Compiled);
            send_handle.send(vm.execution_interrupt.clone()).unwrap();
            let budget = quench_runtime::stack::GuestCallGuard::remaining();
            let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
            assert!(error.is_execution_terminated(), "{error}");
            assert_eq!(quench_runtime::stack::GuestCallGuard::remaining(), budget);
            let Some(quench_runtime::vm::program_store::JsTierState::Compiled(native)) =
                vm.programs.js_tier(ProgramId::MAIN, 1) else { panic!("numeric function must compile"); };
            #[cfg(not(quench_profiled))]
            assert!(native.native_entries() > 0);
            complete.send(()).unwrap();
        }).unwrap();
        let handle = receive_handle.recv_timeout(TIMEOUT).unwrap();
        entered.recv_timeout(TIMEOUT).unwrap();
        thread::sleep(Duration::from_millis(20));
        handle.request_termination();
        finished.recv_timeout(TIMEOUT).expect("shared numeric JIT loop must observe live cancellation");
        worker.join().unwrap();
    }

    #[test]
    fn scalar_core_binds_poll_pointer_before_loop_but_keeps_atomic_reads() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
        let mut native = ir::Function::with_name_signature(UserFuncName::testcase("bound_poll"), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        let header = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let activation = builder.block_params(entry)[0];
        // The old address establishes fault-poll availability, not executable identity.
        let interrupt = quench_runtime::ExecutionInterrupt::default();
        let Some(address) = poll_address(&interrupt) else { return };
        let mut core = RegionCore::new(builder, activation, Some(address));
        core.ins().jump(header, &[]);
        core.switch_to_block(header);
        core.interrupt_safepoint();
        core.interrupt_safepoint();
        core.ins().jump(header, &[]);
        core.seal_all_blocks();
        core.into_builder().finalize(isa.frontend_config());
        cranelift_codegen::verify_function(&native, &*isa).unwrap();
        let owners: Vec<_> = native.layout.block_insts(entry).filter(|&inst| matches!(native.dfg.insts[inst], ir::InstructionData::Load { arg, offset, .. }
                if arg == activation && i32::from(offset) == std::mem::offset_of!(CallContext, head.poll_start) as i32)).map(|inst| native.dfg.inst_results(inst)[0]).collect();
        assert_eq!(owners.len(), 1, "bind the activation's pointer once before the loop");
        let polls: Vec<_> = native.layout.block_insts(header).filter(|&inst|
            native.dfg.insts[inst].opcode() == ir::Opcode::AtomicLoad).collect();
        assert_eq!(polls.len(), 2, "poll reads must remain separate observable operations");
        for inst in polls { assert_eq!(native.dfg.inst_args(inst), &[owners[0]]); }
    }

    pub(crate) fn semantic_vm(source: &str) -> Vm {
        let program = quench_runtime::Engine::specialize(source, "semantic-entry.js").unwrap();
        semantic_program(program)
    }

    fn semantic_program(program: ResidualProgram) -> Vm {
        let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
        vm.initialize_for_snapshot(&program).unwrap();
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let body = program.function(0);
        let mut frame = Box::new(Frame::empty(ProgramId::MAIN, 0));
        frame.registers.resize(usize::from(body.registers), Value::UNDEFINED);
        frame.locals.resize(usize::from(body.locals), Value::UNDEFINED);
        frame.initialized_registers = frame.registers.len();
        vm.frames.push(frame);
        vm.active_registers = vm.frames[0].registers.as_mut_ptr();
        vm
    }

    fn semantic_instruction(vm: &Vm, opcode: Op) -> (usize, WideInstruction) {
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let body = program.function(0);
        body.code.iter().copied().enumerate().find_map(|(pc, packed)| {
            let instruction = body.widened(packed);
            (instruction.op() == opcode).then_some((pc, instruction))
        }).expect("test operation exists")
    }

    fn invoke_semantic(
        vm: &mut Vm, pc: usize,
        entry: unsafe extern "C" fn(*mut std::ffi::c_void, u64, *const u64, *mut u64) -> i64,
    ) -> (i64, u64, Option<JsNativeStep>) {
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let body = program.function(0);
        let instruction = body.widened(body.code[pc]);
        let operands = quench_runtime::NativeJsOperands::new(
            instruction.a(), instruction.b(), instruction.c(), instruction.imm(),
        );
        with_semantic_context(vm, pc, |context, result| {
            // SAFETY: the context owner roots the frame; operands and result live through this call.
            unsafe { entry(context.cast(), pc as u64, operands.as_ptr(), result) }
        })
    }

    fn with_semantic_context(
        vm: &mut Vm, pc: usize,
        run: impl FnOnce(*mut CallContext, *mut u64) -> i64,
    ) -> (i64, u64, Option<JsNativeStep>) {
        JsStepDispatch::with(vm, ProgramId::MAIN, 0, pc, false, true, |activation, vm| {
            let dispatch = &mut *activation;
            let mut context = CallContext::native_adapter(
                (dispatch as *mut JsStepDispatch).cast(),
                vm.execution_interrupt.attention_flag(), None,
            );
            let vm = vm as *mut Vm;
            context.vm = vm.cast();
            dispatch.bind_native_storage(vm);
            context.program = ProgramId::MAIN;
            let mut result = 0;
            let status = run(&mut context, &mut result);
            (status, result, dispatch.outcome.take())
        }).unwrap()
    }

    /// Publishes compiled `code` of a function lowered with `native_entry_signature` to
    /// executable memory, and its entry.
    #[test]
    fn native_object_table_binding_tracks_heap_lifetime() {
        let source = "globalThis.tableProbe=0; throw 7;";
        let mut vm = semantic_vm(source);
        let original = vm.heap_reads().object_record_base().unwrap() as usize;
        let probe = vm.lookup_atom("tableProbe").unwrap();
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let assert_binding = |vm: &mut Vm, expected| {
            JsStepDispatch::with(vm, ProgramId::MAIN, 0, 0, false, true, |activation, vm| {
                assert_ne!(expected, 0);
                assert_eq!(activation.object_record_base, expected);
                assert_eq!(vm.heap_reads().object_record_base().unwrap() as usize, expected);
            }).unwrap();
        };
        assert_binding(&mut vm, original);
        // A normal semantic store may allocate/widen property storage; neither
        // it nor a full collection replaces the record-table reservation.
        vm.set_property(vm.realm.globals, probe, Value::number(1.5)).unwrap();
        vm.collect_now(&program);
        assert_binding(&mut vm, original);
        assert_eq!(vm.own_property(vm.realm.globals, probe), Some(Value::number(1.5)));

        let mut other = semantic_vm(source);
        let other_base = other.heap_reads().object_record_base().unwrap() as usize;
        // Reentry in another VM admits that VM's table, without changing the
        // outer activation's admitted base or retaining it for later entries.
        JsStepDispatch::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, _| {
            assert_binding(&mut other, other_base);
            assert_eq!(activation.object_record_base, original);
        }).unwrap();

        // Realm initialization destroys the old heap reservation. Rebuild the
        // caller frame as semantic_program does, then admit the current table;
        // allocators may reuse the address, so pointer inequality is no proof.
        vm.initialize_for_snapshot(&program).unwrap();
        let current = vm.programs.get(ProgramId::MAIN).unwrap();
        let body = current.function(0);
        let mut frame = Box::new(Frame::empty(ProgramId::MAIN, 0));
        frame.registers.resize(usize::from(body.registers), Value::UNDEFINED);
        frame.locals.resize(usize::from(body.locals), Value::UNDEFINED);
        frame.initialized_registers = frame.registers.len();
        vm.frames.push(frame);
        vm.active_registers = vm.frames[0].registers.as_mut_ptr();
        let reset_base = vm.heap_reads().object_record_base().unwrap() as usize;
        assert_binding(&mut vm, reset_base);
        assert_binding(&mut other, other_base);
    }

    #[test]
    fn native_activation_owns_metadata_without_retaining_it_between_entries() {
        let vm_source = "throw 7;";
        let mut first_vm = semantic_vm(vm_source);
        let mut second_vm = semantic_vm(vm_source);
        let weak = JsStepDispatch::with(&mut first_vm, ProgramId::MAIN, 0, 0, false, true, |first, first_vm| {
            let first_program = first_vm.programs.get(ProgramId::MAIN).unwrap();
            JsStepDispatch::with(&mut second_vm, ProgramId::MAIN, 0, 0, false, true, |second, second_vm| {
                let second_program = second_vm.programs.get(ProgramId::MAIN).unwrap();
                assert_eq!(first.program_id, second.program_id);
                assert!(!Rc::ptr_eq(&first_program, &second_program));
                assert_eq!(first.residual, Rc::as_ptr(&first_program) as usize);
                assert_eq!(first.body, first_program.function(0) as *const _);
                assert_ne!(first.body, second.body);
            }).unwrap();
            let mut shared = first_program.clone();
            assert!(Rc::get_mut(&mut shared).is_none(), "admitted records cannot be uniquely released");
            Rc::downgrade(&first_program)
        }).unwrap();
        // Only the VM owns the program after the native scope returns.
        assert_eq!(weak.strong_count(), 1);
        drop(first_vm);
        assert!(weak.upgrade().is_none());
    }

    #[cfg(feature = "native-execution")]
    #[test]
    fn native_only_rejects_unlinked_roots_before_guest_effects() {
        let program = quench_runtime::Engine::specialize("globalThis.effect = 1;", "unlinked.js").unwrap();
        let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
        assert!(error.to_string().contains("native guest code required"));
        let atom = vm.lookup_atom("effect").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), None);
    }

    #[test]
    fn static_semantic_entry_executes_one_store_and_returns_to_native_control() {
        let mut vm = semantic_vm("globalThis.x = 1; globalThis.y = 2;");
        let (pc, instruction) = semantic_instruction(&vm, Op::SetField);
        vm.frames[0].registers[usize::from(instruction.register_b())] = vm.realm.globals;
        let value = Value::number(1.0);
        vm.frames[0].registers[usize::from(instruction.register_a())] = value;
        let (status, next, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<{ Op::SetField as usize }>);
        assert_eq!(status, 0);
        assert_eq!(next, (pc + 1) as u64);
        assert!(outcome.is_none());
        assert_eq!(vm.own_property(vm.realm.globals, instruction.atom_index()), Some(value));
        let y = vm.lookup_atom("y").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, y), None);
    }

    #[test]
    fn static_semantic_entry_rejects_mismatched_or_invalid_opcodes_before_effects() {
        let mut vm = semantic_vm("globalThis.x = 1;");
        let (pc, _) = semantic_instruction(&vm, Op::SetField);
        let before = vm.frames[0].registers.clone();
        for entry in [
            native_js_operation_entry::<{ Op::LoadConst as usize }>,
            native_js_operation_entry::<{ Op::COUNT }>,
            native_js_operation_entry::<{ usize::MAX }>,
        ] {
            let (status, _, outcome) = invoke_semantic(&mut vm, pc, entry);
            assert_eq!(status, i64::from(LANE_BODY_THREW), "a semantic stop is the body's own exception");
            assert!(outcome.is_none());
            assert!(vm.lane_completion.take_raised().is_some());
            assert_eq!(vm.frames[0].registers[..], before[..]);
        }
    }

    #[test]
    fn static_semantic_entry_admits_wide_opcodes_without_decoding_source_operands() {
        let mut program = quench_runtime::Engine::specialize("throw 7;", "wide-semantic-entry.js").unwrap();
        let body = &mut program.functions[0];
        let pc = body.instructions().position(|instruction| quench_runtime::staged::Operation::<u64, ()>::is_throw(instruction)).unwrap();
        let instruction = body.widened(body.code[pc]);
        let wide_index = body.wide.len();
        body.wide.push(instruction);
        body.code[pc] = quench_runtime::bytecode::Instr::wide(wide_index).unwrap();
        let mut vm = semantic_program(program);
        vm.frames[0].registers[usize::from(instruction.register_a())] = Value::number(7.0);
        let (status, _, _) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<{ Op::SetField as usize }>);
        assert_eq!(status, i64::from(LANE_BODY_THREW));
        assert!(vm.lane_completion.take_raised().is_some());
        assert_eq!(vm.frames[0].registers[usize::from(instruction.register_a())], Value::number(7.0));
        let (status, _, _) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<{ Op::Throw as usize }>);
        assert_eq!(status, i64::from(LANE_BODY_THREW));
        assert_eq!(raise_site_parts(vm.lane_raise_site).1 as usize, pc);
        let error = vm.lane_completion.take_raised().expect("wide Throw must stop with its original value");
        assert_eq!(error.thrown_value(), Some(Value::number(7.0)));
    }

    #[test]
    fn static_semantic_entry_preserves_thrown_value_and_exact_pc() {
        let mut vm = semantic_vm("throw 7;");
        let (pc, instruction) = semantic_instruction(&vm, Op::Throw);
        let value = Value::number(7.0);
        vm.frames[0].registers[usize::from(instruction.register_a())] = value;
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<{ Op::Throw as usize }>);
        assert_eq!(status, i64::from(LANE_BODY_THREW));
        assert!(outcome.is_none());
        assert_eq!(raise_site_parts(vm.lane_raise_site).1 as usize, pc);
        let error = vm.lane_completion.take_raised().expect("semantic throw must be abrupt");
        assert_eq!(error.thrown_value(), Some(value));
    }

    #[test]
    fn static_semantic_entry_checks_termination_before_guest_effects() {
        let mut vm = semantic_vm("globalThis.x = 1;");
        let (pc, instruction) = semantic_instruction(&vm, Op::SetField);
        vm.frames[0].registers[usize::from(instruction.register_b())] = vm.realm.globals;
        vm.frames[0].registers[usize::from(instruction.register_a())] = Value::number(1.0);
        vm.execution_interrupt.request_termination();
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<{ Op::SetField as usize }>);
        assert_eq!(status, i64::from(quench_bjit::call_record::TERMINATED_STATUS), "terminated entry must stop");
        assert!(outcome.is_none());
        assert_eq!(vm.own_property(vm.realm.globals, instruction.atom_index()), None);
    }

    #[test]
    fn per_import_callout_counts_use_function_indices_and_escape_json_names() {
        let names = vec![
            (0, 2, "env".to_owned(), "copy\"fast".to_owned()),
            (1, 5, "host\\api".to_owned(), "line\nfeed".to_owned()),
        ];
        let counters = [std::cell::Cell::new(0), std::cell::Cell::new(0)];
        // SAFETY: the pointer covers both initialized counters for their
        // entire use below.
        unsafe {
            note_import_callout_target(counters.as_ptr(), counters.len(), 1);
            note_import_callout_target(counters.as_ptr(), counters.len(), 1);
            note_import_callout_target(counters.as_ptr(), counters.len(), 99);
        }
        assert_eq!(counters[0].get(), 0);
        assert_eq!(counters[1].get(), 2);
        assert_eq!(
            quench_runtime::vm::engine_types::format_import_callout_targets(&names, &counters),
            "{\"function\":1,\"import_index\":5,\"module\":\"host\\\\api\",\"name\":\"line\\nfeed\",\"count\":2}"
        );
        assert_eq!(quench_runtime::vm::engine_types::json_string("\u{1}\t"), "\"\\u0001\\t\"");
    }

    #[test]
    #[cfg(all(target_arch = "x86_64", unix))]
    fn native_checked_signed_multiply_preserves_product_and_overflow() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for ty in [types::I16, types::I32, types::I64] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);
            let args = builder.block_params(block).to_vec();
            let left = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
            let right = if ty == types::I64 { args[1] } else { builder.ins().ireduce(ty, args[1]) };
            let (product, overflow) = builder.ins().smul_overflow(left, right);
            let product = if ty == types::I64 { product } else { builder.ins().sextend(types::I64, product) };
            let overflow = builder.ins().uextend(types::I64, overflow);
            builder.ins().store(MemFlagsData::trusted(), product, args[2], 0);
            builder.ins().store(MemFlagsData::trusted(), overflow, args[2], std::mem::size_of::<i64>() as i32);
            builder.ins().return_(&[]);
            builder.finalize(isa.frontend_config());
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: the generated signature is (i64, i64, pointer) -> void; its only
            // memory accesses write the two i64 slots supplied below. Code stays live.
            let multiply = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(i64, i64, *mut i64)>(code.pointer()) };
            let bits = ty.bits();
            let min = -(1i128 << (bits - 1));
            let max = (1i128 << (bits - 1)) - 1;
            let cases = [min, min + 1, -46341, -2, -1, 0, 1, 2, 46341, max - 1, max];
            for left in cases.into_iter().filter(|value| (min..=max).contains(value)) {
                for right in cases.into_iter().filter(|value| (min..=max).contains(value)) {
                    let exact = left * right;
                    let wrapped = (exact << (128 - bits)) >> (128 - bits);
                    let mut actual = [0i64; 2];
                    unsafe { multiply(left as i64, right as i64, actual.as_mut_ptr()) };
                    assert_eq!(actual, [wrapped as i64, i64::from(!(min..=max).contains(&exact))], "{ty}: {left} * {right}");
                }
            }
        }
    }

    #[test]
    #[cfg(all(target_arch = "x86_64", unix))]
    fn native_vector_rotations_preserve_near_matches_and_unaligned_accesses() {
        #[derive(Clone, Copy, Debug)]
        enum Shape { Rotate, DistinctInputs, SignedRight, NonComplementaryShift }
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        // A baseline x86-64 target has no optional SSSE3: it must retain
        // the existing shifts/OR lowering while producing the same values.
        let baseline = cranelift_codegen::isa::lookup(isa.triple().clone())
            .unwrap().finish(isa.flags().clone()).unwrap();
        for isa in [isa, baseline] {
        let cases = [0, 1, 2, u32::MAX as u64, 1 << 32, 1 << 63, i64::MAX as u64, u64::MAX, 0x0123_4567_89ab_cdef, 0xaaaa_5555_aaaa_5555];
        for rotate in 0..u64::BITS {
            for reversed in [false, true] {
                for shape in [Shape::Rotate, Shape::DistinctInputs, Shape::SignedRight, Shape::NonComplementaryShift] {
                    let mut signature = Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64); 3]);
                    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let block = builder.create_block();
                    builder.append_block_params_for_function_params(block);
                    builder.switch_to_block(block);
                    builder.seal_block(block);
                    let args = builder.block_params(block).to_vec();
                    let memory = MemFlagsData::new().with_notrap();
                    let cast = MemFlagsData::new().with_endianness(ir::Endianness::Little);
                    let first = builder.ins().load(types::I8X16, memory, args[0], 0);
                    let first = builder.ins().bitcast(types::I64X2, cast, first);
                    let second = if matches!(shape, Shape::DistinctInputs) {
                        let value = builder.ins().load(types::I8X16, memory, args[1], 0);
                        builder.ins().bitcast(types::I64X2, cast, value)
                    } else { first };
                    let left_amount = u64::BITS - rotate - u32::from(matches!(shape, Shape::NonComplementaryShift));
                    let left_count = builder.ins().iconst(types::I32, i64::from(left_amount));
                    let right_count = builder.ins().iconst(types::I32, i64::from(rotate));
                    let left = builder.ins().ishl(first, left_count);
                    let right = if matches!(shape, Shape::SignedRight) {
                        builder.ins().sshr(second, right_count)
                    } else { builder.ins().ushr(second, right_count) };
                    let left = builder.ins().bitcast(types::I8X16, cast, left);
                    let right = builder.ins().bitcast(types::I8X16, cast, right);
                    let value = if reversed { builder.ins().bor(right, left) } else { builder.ins().bor(left, right) };
                    builder.ins().store(memory, value, args[2], 0);
                    builder.ins().return_(&[]);
                    builder.finalize(isa.frontend_config());
                    let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                    // SAFETY: generated code accesses exactly sixteen bytes at
                    // each pointer, promises no alignment, and stays live here.
                    let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const u8, *const u8, *mut u8)>(code.pointer()) };
                    for offset in [0, 1, 7] {
                        for (index, input) in cases.into_iter().enumerate() {
                            let left = [input, cases[(index + 3) % cases.len()]];
                            let right = if matches!(shape, Shape::DistinctInputs) { [!left[1], !left[0]] } else { left };
                            let mut first = [0u8; 32];
                            let mut second = [0u8; 32];
                            let mut output = [0xa5u8; 32];
                            for lane in 0..2 {
                                let start = offset + lane * std::mem::size_of::<u64>();
                                first[start..start + 8].copy_from_slice(&left[lane].to_le_bytes());
                                second[start..start + 8].copy_from_slice(&right[lane].to_le_bytes());
                            }
                            unsafe { run(first.as_ptr().add(offset), second.as_ptr().add(offset), output.as_mut_ptr().add(offset)) };
                            for lane in 0..2 {
                                let right = if matches!(shape, Shape::SignedRight) { (right[lane] as i64).wrapping_shr(rotate) as u64 } else { right[lane].wrapping_shr(rotate) };
                                let expected = left[lane].wrapping_shl(left_amount) | right;
                                let start = offset + lane * std::mem::size_of::<u64>();
                                assert_eq!(&output[start..start + 8], &expected.to_le_bytes(), "{shape:?}, rotate={rotate}, reversed={reversed}, offset={offset}");
                            }
                            assert!(output[..offset].iter().chain(output[offset + 16..].iter()).all(|byte| *byte == 0xa5));
                        }
                    }
                }
            }
        }
        }
    }

    #[test]
    fn native_alias_regions_preserve_overlapping_untagged_accesses() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        for tagged_load in [false, true] {
            for tagged_store in [false, true] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.push(AbiParam::new(types::I64));
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
                let region = function.dfg.alias_regions.insert(ir::AliasRegionData {
                    user_id: 0,
                    description: "overlapping test accesses".into(),
                });
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let block = builder.create_block();
                builder.append_block_params_for_function_params(block);
                builder.switch_to_block(block);
                builder.seal_block(block);
                let address = builder.block_params(block)[0];
                let load_flags = MemFlagsData::trusted().with_alias_region(tagged_load.then_some(region));
                let store_flags = MemFlagsData::trusted().with_alias_region(tagged_store.then_some(region));
                let first = builder.ins().load(types::I64, load_flags, address, 0);
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().store(store_flags, zero, address, 0);
                let second = builder.ins().load(types::I64, load_flags, address, 0);
                let result = builder.ins().iadd(first, second);
                builder.ins().return_(&[result]);
                builder.finalize(isa.frontend_config());
                let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                // SAFETY: the generated function reads and writes one live u64,
                // and its executable mapping remains alive throughout each call.
                let run = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*mut u64) -> u64>(code.pointer())
                };
                for input in [0, 1, 42, i64::MAX as u64, u64::MAX] {
                    let mut memory = input;
                    assert_eq!(unsafe { run(&mut memory) }, input, "load tagged={tagged_load}, store tagged={tagged_store}");
                    assert_eq!(memory, 0);
                }
            }
        }
    }

    #[test]
    fn native_unsigned_widening_multiply_preserves_lane_products() {
        let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
        // Exhaust both single-input shuffle encodings and all lane selectors,
        // including the unaffected cases, against scalar unsigned products.
        for shuffle in std::iter::once(None).chain(
            (0..=u8::MAX).flat_map(|mask| [Some((mask, false)), Some((mask, true))]),
        ) {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);
            let args = builder.block_params(block).to_vec();
            let mut input = |address| {
                let value = builder.ins().load(types::I8X16, MemFlagsData::trusted(), address, 0);
                let value = if let Some((selector, rhs)) = shuffle {
                    let lanes: Vec<u8> = (0..16).map(|byte| {
                        ((selector >> ((byte / 4) * 2)) & 3) * 4 + byte % 4 + if rhs { 16 } else { 0 }
                    }).collect();
                    let mask = builder.func.dfg.immediates.push(ir::ConstantData::from(lanes));
                    let poison = builder.func.dfg.constants.insert(ir::ConstantData::from(vec![0x5a; 16]));
                    let poison = builder.ins().vconst(types::I8X16, poison);
                    let (left, right) = if rhs { (poison, value) } else { (value, poison) };
                    builder.ins().shuffle(left, right, mask)
                } else { value };
                builder.ins().bitcast(types::I32X4, MemFlagsData::new().with_endianness(ir::Endianness::Little), value)
            };
            let left = input(args[0]);
            let right = input(args[1]);
            let left = builder.ins().uwiden_low(left);
            let right = builder.ins().uwiden_low(right);
            let product = builder.ins().imul(left, right);
            builder.ins().store(MemFlagsData::trusted(), product, args[2], 0);
            builder.ins().return_(&[]);
            builder.finalize(isa.frontend_config());
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: generated code reads two four-u32 inputs and writes two u64 outputs.
            // All arrays below outlive the call, as does the executable code mapping.
            let multiply = unsafe {
                std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const u32, *const u32, *mut u64)>(code.pointer())
            };
            let cases = [0, 1, 2, 0xffff, 0x10000, 0x7fff_ffff, 0x8000_0000, u32::MAX];
            for a in cases {
                for b in cases {
                    // Different lane values and poisoned unused lanes expose lane selection
                    // errors as well as accidental signed extension or truncation.
                    let left = [a, !b, u32::MAX, 0];
                    let right = [b, !a, 0, u32::MAX];
                    let mut actual = [0u64; 2];
                    unsafe { multiply(left.as_ptr(), right.as_ptr(), actual.as_mut_ptr()) };
                    let indices = shuffle.map_or([0, 1], |(selector, _)| [usize::from(selector & 3), usize::from((selector >> 2) & 3)]);
                    let expected = indices.map(|index| u64::from(left[index]) * u64::from(right[index]));
                    assert_eq!(actual, expected, "shuffle {shuffle:?}, inputs {left:?} * {right:?}");
                }
            }
        }
    }

    #[test]
    fn root_only_jit_defaults_to_none_and_accepts_explicit_opt_level() {
        assert_eq!(parse_jit_root_opt_level(true, None), Ok("none"));
        assert_eq!(parse_jit_root_opt_level(false, None), Ok("speed"));
        assert_eq!(parse_jit_root_opt_level(true, Some("none")), Ok("none"));
        assert_eq!(parse_jit_root_opt_level(true, Some("speed")), Ok("speed"));
        assert_eq!(
            parse_jit_root_opt_level(true, Some("speed_and_size")),
            Ok("speed_and_size")
        );
        assert_eq!(parse_jit_root_opt_level(true, Some("invalid")), Err(()));
    }

    #[test]
    fn sparse_analysis_states_preserve_register_joins() {
        assert_eq!(Slot::Memory32Address.join(Slot::Memory32Address), Slot::Memory32Address);
        assert_eq!(Slot::Memory32Address.join(Slot::Value(Kind::I64)), Slot::Value(Kind::I64));
        assert_eq!(Slot::Value(Kind::I64).join(Slot::Memory32Address), Slot::Value(Kind::I64));
        assert_eq!(Slot::Memory32Address.join(Slot::DEFAULT), Slot::Conflict);
        const FACTS: [Slot; 9] = [
            Slot::DEFAULT,
            Slot::Value(Kind::I64),
            Slot::Memory32Address,
            Slot::Vector,
            Slot::Binding(7),
            Slot::Memory,
            Slot::Import(9),
            Slot::IndirectReference {
                signature: 11,
                table: 13,
            },
            Slot::Conflict,
        ];
        let states = FACTS
            .iter()
            .flat_map(|first| {
                FACTS.iter().flat_map(move |second| {
                    FACTS.iter().map(move |third| vec![*first, *second, *third])
                })
            })
            .collect::<Vec<_>>();
        let sparse = |dense: &[Slot]| {
            let registers = u16::try_from(dense.len()).expect("small test state");
            let mut state = AnalysisState::new();
            for (index, slot) in dense.iter().copied().enumerate() {
                state
                    .set(
                        u16::try_from(index).expect("small test register"),
                        registers,
                        slot,
                    )
                    .expect("register in range");
            }
            state
        };

        for left in &states {
            for right in &states {
                let mut expected = left.clone();
                let mut changed = false;
                for (slot, incoming) in expected.iter_mut().zip(right) {
                    let joined = slot.join(*incoming);
                    changed |= joined != *slot;
                    *slot = joined;
                }

                let mut actual = sparse(left);
                assert_eq!(actual.join_from(&sparse(right)), changed);
                for (index, expected) in expected.iter().copied().enumerate() {
                    assert_eq!(actual.get(index as u16, 3), Some(expected));
                }
            }
        }

        // Once a loop backedge turns a fact into Conflict, later iterations
        // must not make it precise again.
        let mut loop_state = AnalysisState::new();
        let mut incoming = AnalysisState::new();
        incoming
            .set(0, 1, Slot::Value(Kind::I64))
            .expect("register in range");
        assert!(loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert!(!loop_state.join_from(&AnalysisState::new()));
        assert!(!loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert_eq!(loop_state.get(1, 1), None);

        loop_state
            .set(0, 1, Slot::DEFAULT)
            .expect("register in range");
        assert_eq!(loop_state.get(0, 1), Some(Slot::DEFAULT));
    }
}

/// The lane body compile a core record holds type-erased (`LaneBody::Compiling`, `LoopBody::Compiling`).
#[inline]
pub(crate) fn pending_lane(pending: Box<dyn std::any::Any>) -> Box<PendingLaneMethod> {
    pending.downcast().unwrap_or_else(|_| unreachable!("a lane body compile is a `PendingLaneMethod`"))
}

/// `pending_lane`, borrowed.
#[inline]
pub(crate) fn pending_lane_ref(pending: &dyn std::any::Any) -> &PendingLaneMethod {
    pending.downcast_ref().unwrap_or_else(|| unreachable!("a lane body compile is a `PendingLaneMethod`"))
}

/// The loop region compile a core record holds type-erased (`LoopTier::pending`).
#[inline]
pub(crate) fn pending_osr(pending: Box<dyn std::any::Any>) -> Box<PendingOsrRegion> {
    pending.downcast().unwrap_or_else(|_| unreachable!("a loop region compile is a `PendingOsrRegion`"))
}

/// `pending_osr`, borrowed.
#[inline]
pub(crate) fn pending_osr_ref(pending: &dyn std::any::Any) -> &PendingOsrRegion {
    pending.downcast_ref().unwrap_or_else(|| unreachable!("a loop region compile is a `PendingOsrRegion`"))
}
