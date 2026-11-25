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
//! dispatch is `become`, a jump whatever the handler keeps on its stack, and a run of
//! handlers shares one stack frame; a helper of the default convention that a
//! handler calls returns where to continue (`Resumed`) for the handler to dispatch; otherwise
//! every handler returns to the lane loop. A taken safepoint jump spends one unit of
//! the run's fuel (`script_fuel_out`); once per batch it reaches `safepoint_at`, which
//! returns to the lane loop (unwinding the stack should a build not form a sibling call)
//! or leaves the lane where a collection is due or termination requested. A due
//! collection zeroes the fuel, so the next safepoint jump leaves; a termination request
//! from another thread raises the floor, read within one batch.
#[allow(unused_imports)]
use crate::ext_traits::*;
/// The handler of opcode `$op`'s generated row at accumulator field `$acc` and site fact `$flag`
/// (`lane_frame::generated_row_handler`), chosen at compile time so each use names one shard's instance.
/// The instance of `row_jump` with these const parameters (`sharded_handler!`).
macro_rules! row_jump {
    ($($param:tt)*) => {
        const { crate::dispatch_fast::lane_frame::row_jump_handler::<$($param)*>() }
    };
}

/// The instance of `protocol_row` with these const parameters (`sharded_handler!`).
macro_rules! protocol_row {
    ($($param:tt)*) => {
        const { crate::dispatch_fast::lane_frame::protocol_row_handler::<$($param)*>() }
    };
}

/// The instance of `returning_row` with these const parameters (`sharded_handler!`).
macro_rules! returning_row {
    ($($param:tt)*) => {
        const { crate::dispatch_fast::lane_frame::returning_row_handler::<$($param)*>() }
    };
}

/// The instance of `fused_rows` with these const parameters (`sharded_handler!`).
macro_rules! fused_rows {
    ($($param:tt)*) => {
        const { crate::dispatch_fast::lane_frame::fused::fused_rows_handler::<$($param)*>() }
    };
}

/// The instance of `chain_rows` with these const parameters (`sharded_handler!`).
macro_rules! chain_rows {
    ($($param:tt)*) => {
        const { crate::dispatch_fast::lane_frame::fused::chain_rows_handler::<$($param)*>() }
    };
}

macro_rules! generated_row {
    ($op:expr, $acc:expr, $flag:expr) => {
        const { crate::dispatch_fast::lane_frame::generated_row_handler::<{ $op }, { $acc }, { $flag }>() }
    };
}

/// `body` with `$l` the const leaf form `leaf` names (`LeafForm`, 0..=4), chosen by `if const` so a
/// const generic argument can name a leaf form a const expression computes.
macro_rules! by_leaf {
    ($leaf:expr, |$l:ident| $body:expr) => {{
        if const { $leaf == 0 } { const $l: u8 = 0; $body }
        else if const { $leaf == 1 } { const $l: u8 = 1; $body }
        else if const { $leaf == 2 } { const $l: u8 = 2; $body }
        else if const { $leaf == 3 } { const $l: u8 = 3; $body }
        else { const $l: u8 = 4; $body }
    }};
}

/// Return `value` from the record `ip` as `Return` would: one op (`lane_return_one_op`), else the
/// activation's completion, else the frame's return.
macro_rules! lane_return_value {
    ($cx:expr, $ip:expr, $value:expr) => { lane_return_value!($cx, $ip, $value, NOT_LEAF) };
    ($cx:expr, $ip:expr, $value:expr, $leaf:expr) => {{
        let (cx, ip, value): (*mut LaneContext, Ip, Value) = ($cx, $ip, $value);
        let window = Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get());
        if let Some((registers, next)) = by_leaf!($leaf, |L| crate::dispatch_fast::lane_return_one_op::<L>(cx, window, value)) {
            next!(cx, registers, next, Carried { memory: MemoryView::script(cx), integers: quench_runtime::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) })
        }
        if let Some(resumed) = by_leaf!($leaf, |L| crate::dispatch_fast::complete_plain_activation_in_lane::<L>(cx, value)) { resume!(cx, resumed) }
        resume!(cx, crate::dispatch_fast::return_value_general_in_lane(cx, ip, value));
    }};
}

/// The protocol row of a return record of opcode `$op` whose RETURN parameter code is `$code`
/// (`protocol_misses::returning_code`): the record runs its staged row, then its miss body.
macro_rules! return_row {
    ($op:expr, $code:expr) => {
        protocol_row!({ $op as u16 }, ACC_NONE, { protocol_family::RETURN | ($code as u16) })
    };
}

/// The lane handler of a returning record of opcode `$op` of kind `$kind` at leaf form `$leaf`
/// (`returning_row`): its RETURN parameter is `returning_code($kind, $leaf)`, which its miss body
/// (`returning_miss` of `$kind`) completes.
macro_rules! returning_row_of {
    ($op:expr, $kind:expr, $leaf:expr) => {
        returning_row!({ $op as u16 }, { $leaf }, { protocol_family::RETURN | (returning_code($kind, $leaf) as u16) })
    };
}

/// The lane handler of a returning constant-right `Binary` of arithmetic operator `$op` at leaf form
/// `$leaf`, whose constant is an int32 where `$int32` (`returning_operator_row`): its RETURN parameter is
/// `operator_return_code($op, $leaf)`, which its miss body (`returning_operator_miss_of`) completes.
macro_rules! returning_operator_row_of {
    ($op:expr, $leaf:expr, $int32:expr) => {
        returning_operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $op as u8 }, { $leaf }, { $int32 },
            { operator_return_code($op as u8, $leaf) }> as Handler
    };
}


use quench_runtime::staged::compare::{IN_OPERATOR, INSTANCEOF_OPERATOR};
use crate::compiled::tier_policy::Tiers;
use quench_runtime::vm::tier_events::Event;
use crate::compiled::{CALLEE_ACTIVATION_PENDING, NO_PENDING_ACTIVATIONS};
use quench_runtime::vm::dispatch_frame::PendingGeneralCall;
use quench_runtime::vm::iterators::InPlaceStep;
use smallvec::SmallVec;
use quench_runtime::tier_surface::InlineObjectHeader;
use quench_runtime::value_vec::INLINE_PROPERTY_COUNT;
use crate::engine_prelude::*;
use quench_runtime::bytecode::{RegisterList, Traits};
use quench_runtime::bytecode::{
    FieldLayout, FieldStore, Function, ImmediateLayout, ImmediateRole, InstructionField, StoreReceiver, WideInstruction,
};
use quench_runtime::wasm::float::F64UnaryOperator;
use quench_runtime::wasm::integer::I32BinaryOperator;
use quench_runtime::wasm::memory::{MemoryLoad, MemoryStore};

/// The arithmetic operators the lane decides per operator (`lane_operator`,
/// `returning_operator_miss`): `$pick!(OPERATOR)` of `$operator`'s, else `None`; or with
/// `all`, the array of every operator's `$pick!`.
macro_rules! arithmetic_operators {
    (@with $form:ident ($($args:tt)*)) => {{
        use quench_runtime::vm::operations::{
            ADDITION, BITWISE_AND, BITWISE_OR, BITWISE_XOR, DIVISION, MULTIPLICATION, REMAINDER,
            SHIFT_LEFT, SHIFT_RIGHT, SHIFT_RIGHT_ZERO_FILL, SUBTRACTION,
        };
        $form!($($args)*; ADDITION, SUBTRACTION, MULTIPLICATION, DIVISION, REMAINDER, SHIFT_LEFT,
            SHIFT_RIGHT, SHIFT_RIGHT_ZERO_FILL, BITWISE_OR, BITWISE_XOR, BITWISE_AND)
    }};
    (all $pick:ident) => {
        arithmetic_operators!(@with arithmetic_all ($pick))
    };
    ($operator:expr, $pick:ident) => {
        arithmetic_operators!(@with arithmetic_match ($operator, $pick))
    };
}

macro_rules! arithmetic_match {
    ($operator:expr, $pick:ident; $($op:ident),*) => {
        match $operator {
            $($op => Some($pick!($op)),)*
            _ => None,
        }
    };
}

macro_rules! arithmetic_all {
    ($pick:ident; $($op:ident),*) => {
        [$($pick!($op)),*]
    };
}

use quench_runtime::vm::engine_types::LeafForm;
const NOT_LEAF: LeafForm = quench_runtime::staged::call_leaf::NOT_LEAF;
/// Neither its receiver nor its captures (`CallEntry::Leaf`): it runs in its caller's context.
const PLAIN_LEAF: LeafForm = quench_runtime::staged::call_leaf::PLAIN_LEAF;
/// Its captures and not its receiver (`CallEntry::CaptureLeaf`): the call makes the
/// environment its own and binds no receiver unless the leaf materializes.
const CAPTURE_LEAF: LeafForm = quench_runtime::staged::call_leaf::CAPTURE_LEAF;
/// Its receiver, and maybe its captures (`CallEntry::ContextLeaf`): the call makes the
/// receiver and the environment its own.
const CONTEXT_LEAF: LeafForm = quench_runtime::staged::call_leaf::CONTEXT_LEAF;
/// Its receiver and not its captures (`CallEntry::ThisLeaf`): the call binds the receiver
/// and keeps the caller's environment, which only a materialization makes the callee's.
const THIS_LEAF: LeafForm = quench_runtime::staged::call_leaf::THIS_LEAF;

/// Whether a leaf of `form` runs with its receiver bound (`THIS_LEAF`, `CONTEXT_LEAF`).
#[inline(always)]
const fn leaf_binds_this(form: LeafForm) -> bool {
    quench_runtime::staged::call_leaf::binds_this(form)
}

pub(crate) use quench_runtime::vm::engine_types::LaneInstruction;

use quench_runtime::vm::engine_types::NOTED_SITE_CALLEES;

/// The `LaneStack::noted` entry of a link address and callee function.
#[inline(always)]
fn noted_slot(address: usize, function: u32) -> usize {
    ((address >> 1) ^ (function as usize).wrapping_mul(0x9E37)) & (NOTED_SITE_CALLEES - 1)
}

use quench_runtime::vm::engine_types::NO_CALL_LINK;
/// The bit of a call link the plain call proved (`script_call_prove_miss`): its target activates
/// plainly and takes the site's argument count, so `script_call_miss` tests only the callee's
/// identity. The index is the link without it (`link_index`); a link without the bit is out of
/// range for `script_call_miss`, which proves it first.
const PLAIN_LINK: u16 = 1 << 15;
const _: () = assert!(SCRIPT_CALL_TARGETS[0] < PLAIN_LINK as usize && SCRIPT_CALL_TARGETS[1] < PLAIN_LINK as usize);

/// The script target index a call link names, with or without `PLAIN_LINK`.
#[inline(always)]
fn link_index(link: u16) -> usize { usize::from(link & !PLAIN_LINK) }
const _: () = assert!(std::mem::size_of::<LaneInstruction>() == 4 * std::mem::size_of::<u64>());
const _: () = assert!(std::mem::offset_of!(LaneInstruction, a) == 8 && std::mem::offset_of!(LaneInstruction, call_link) + 2 <= 16);

use quench_runtime::vm::engine_types::LaneOperand;

use quench_runtime::vm::engine_types::FieldOperand;

use quench_runtime::vm::engine_types::CallOperand;

/// A `CallOperand::memo` that names no slot: no table holds this many.
const NO_READ_MEMO: u32 = u32::MAX;

/// The `FieldOperand::shape` of a record whose hint is an inherited entry
/// (`Vm::inherited_hint_operands`): no object's exact shape (`Object::exact_shape`, whose id
/// bits never all hold), so an own-field hint read (`LaneInstruction::field_hint`) misses it.
const INHERITED_HINT: u32 = quench_runtime::vm::NO_FIELD_RECEIVER - 1;

/// The `FieldOperand::shape` of a polymorphic read's record whose hint is a prototype-held
/// read (`Vm::holder_hint_hit`), as `INHERITED_HINT` no object's exact shape.
const HOLDER_HINT: u32 = quench_runtime::vm::NO_FIELD_RECEIVER - 2;

/// The `FieldOperand::shape` of a read's record whose hint is an own constant
/// (`Vm::own_constant_operands`), as `INHERITED_HINT` no object's exact shape.
const OWN_CONSTANT_HINT: u32 = quench_runtime::vm::NO_FIELD_RECEIVER - 3;

#[quench_macros::ext(LaneInstructionOps1)]
impl LaneInstruction {
    /// The own-field hint of a `GetField` or `SetField` record: an exact receiver shape and the
    /// slot of its own data field, as the site's primary entry proved it
    /// (`row_get_field_own_refresh`) or a polymorphic store hit it (`POLY_SITE`).
    /// Between shape compactions, which clear every hint (`LaneView::invalidate_shape_bindings`), a
    /// shape's slots never change, so the pair holds whatever the entry holds now.
    #[inline(always)]
    fn field_hint(&self) -> quench_runtime::vm::FieldCache {
        // SAFETY: the view writes `field` for every `GetField` and `SetField` record, the only
        // records whose handlers read the hint.
        quench_runtime::vm::FieldCache::own(unsafe { self.operand.field.shape }, self.call_link.get())
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

    /// The loop site of a loop's backward jump record (`set_loop_site`): its address, split over
    /// `a`, `b` and `c`, which a jump record leaves unused, so the operand keeps the jump's distance.
    #[inline(always)]
    fn loop_site(&self) -> &LoopSite {
        let address = u64::from(self.a) | u64::from(self.b) << 16 | u64::from(self.c) << 32;
        // SAFETY: `install_loop_sites` writes the address of a live site of the view, which holds this record.
        unsafe { &*(address as usize as *const LoopSite) }
    }

    /// Records `site` as this jump record's loop site; false where its address needs more than 48
    /// bits, which the install then keeps from having sites.
    fn set_loop_site(&mut self, site: &LoopSite) -> bool {
        let address = site as *const LoopSite as usize as u64;
        if address >> 48 != 0 {
            return false;
        }
        self.a = address as u16;
        self.b = (address >> 16) as u16;
        self.c = (address >> 32) as u16;
        true
    }

    #[inline(always)]
    fn vector_region(&self) -> *const vector_region::VectorRegion {
        // SAFETY: the view writes `region` for the records it gives `vector_region_row`, the
        // only handler that reads it.
        unsafe { self.operand.region }
    }
}

pub(crate) use quench_runtime::vm::engine_types::RegionDecline;



#[quench_macros::ext(LoopSiteOps1)]
impl LoopSite {
    /// Counts an attempt that entered nothing for `reason`. Under `QUENCH_TRACE=js-jit` the first
    /// of each reason on the site is printed.
    fn count_decline(&self, reason: RegionDecline) {
        let count = &self.declines[reason.index()];
        if count.get() == 0 && self.diagnostics {
            eprintln!("QUENCH_JS_JIT_REGION_DECLINE function={} header={} reason={}", self.function, self.header, reason.name());
        }
        count.set(count.get().saturating_add(1));
    }

    /// Counts an attempt that entered nothing for `reason`, and returns the interpreter's jump at the header.
    fn declined(&self, reason: RegionDecline) -> LoopContinuation {
        self.count_decline(reason);
        LoopContinuation::Jump
    }
}

pub(crate) use quench_runtime::vm::engine_types::LoopSite;

#[quench_macros::ext(LoopSiteOps2)]
impl LoopSite {
    pub(crate) fn function(&self) -> u32 { self.function }

    /// Whether native code may still run the loop: its runs have paid for their entries, its
    /// entries are not declining, and it has code or may compile it. A site without it
    /// retires at its next backward jump (`loop_retire_in_lane`).
    fn may_run_natively(&self, policy: quench_runtime::MemoryPolicy) -> bool {
        self.enters() && self.tier.borrow().may_run(self.tiers, policy)
    }

    fn enters(&self) -> bool {
        self.entering.get()
    }

    /// Whether its loop runs in a region entered from the lane: a closed one that still enters and
    /// has run natively (a region that never ran makes none of the loop's work native).
    pub(crate) fn runs_region(&self) -> bool {
        self.call_boundary_pcs.is_empty() && self.active_region().is_some() && self.enters() && self.tier.borrow().runs() > 0
    }
}

use quench_runtime::vm::engine_types::Ip;

use quench_runtime::vm::engine_types::IndirectSlot;

#[quench_macros::ext(IndirectSlotOps1)]
impl IndirectSlot {
    const NONE: Self = Self { next: std::ptr::null(), register: 0, reference: 0, entry: 0, instance: 0 };
}

pub(crate) use quench_runtime::vm::engine_types::LaneView;




#[quench_macros::ext(LaneViewOps1)]
impl LaneView {
    /// Gives each return record of a leaf's records (`leaf_body`) its leaf form (`LeafForm`),
    /// which completes a running leaf call (`complete_leaf_in_lane`), so no other function's
    /// return tests for one, and a leaf's return restores only what its call replaced.
    fn specialize_leaf_returns(&self, form: LeafForm) {
        macro_rules! forms {
            ($leaf:expr) => {{
                // A constant right operand's record keeps its operator in the operand (`specialize_returning_operator`),
                // so its operator's row decodes it; a register right operand is a `returning_row`.
                macro_rules! pick {
                    ($op:expr) => {
                        [
                            (returning_operator_row_of!($op, NOT_LEAF, true), returning_operator_row_of!($op, $leaf, true)),
                            (returning_operator_row_of!($op, NOT_LEAF, false), returning_operator_row_of!($op, $leaf, false)),
                        ]
                    };
                }
                let returns = [
                    (return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(RETURN_KIND, NOT_LEAF)) as Handler,
                        return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(RETURN_KIND, $leaf)) as Handler),
                    (return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, NOT_LEAF)) as Handler,
                        return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, $leaf)) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::UNARY_ROW, RETURNING_UNARY, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::UNARY_ROW, RETURNING_UNARY, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::BINARY_ROW, RETURNING_BINARY, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::BINARY_ROW, RETURNING_BINARY, $leaf) as Handler),
                    (row_returning_field_first::<NOT_LEAF>, row_returning_field_first::<{ $leaf }>),
                    (row_returning_field_miss::<NOT_LEAF>, row_returning_field_miss::<{ $leaf }>),
                    (generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(true, NOT_LEAF)),
                        generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(true, { $leaf }))),
                    (generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(false, NOT_LEAF)),
                        generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(false, { $leaf }))),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_OWN, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_OWN, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_POLYMORPHIC, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_POLYMORPHIC, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_HOLDER, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_HOLDER, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_INHERITED, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_INHERITED, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_NESTED, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_NESTED, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_PRIVATE, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_PRIVATE, $leaf) as Handler),
                    (returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_GETTER, NOT_LEAF) as Handler,
                        returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_GETTER, $leaf) as Handler),
                ];
                returns.into_iter().chain(arithmetic_operators!(all pick).into_iter().flatten()).collect::<Vec<(Handler, Handler)>>()
            }};
        }
        let forms = match form {
            CONTEXT_LEAF => forms!(CONTEXT_LEAF),
            CAPTURE_LEAF => forms!(CAPTURE_LEAF),
            THIS_LEAF => forms!(THIS_LEAF),
            _ => forms!(PLAIN_LEAF),
        };
        for record in self.records.iter() {
            // SAFETY: as `requicken`: the lane runs on the view's thread, and no reference to
            // the handler field is live.
            let record = unsafe { &mut *record.get() };
            if let Some((_, leaf)) = forms.iter().find(|(plain, _)| *plain as *const () == record.handler) {
                record.handler = *leaf as *const ();
            }
        }
    }
}

use quench_runtime::vm::engine_types::EnvironmentWindow;

pub(crate) use quench_runtime::vm::engine_types::WINDOW_ENVIRONMENT;
pub(crate) use quench_runtime::vm::engine_types::WINDOW_SLOTS;
pub(crate) const WINDOW_CAPTURED: u16 = 2;
pub(crate) use quench_runtime::vm::engine_types::{WINDOW_LOCALS, WINDOW_STORAGE};

use quench_runtime::vm::engine_types::TargetBody;

/// Conservative admission granule retained from block initialization. Padding stays
/// spare capacity; logical register counts own initialization and materialization.
const ACTIVATION_FILL_BLOCK: usize = 8;

/// Script activation admission groups two base granules.
const SCRIPT_FILL_BLOCK: usize = 2 * ACTIVATION_FILL_BLOCK;

pub(crate) mod closure_targets;
use closure_targets::target_entry;
pub(crate) mod window_records;
use window_records::{window_local_handler, window_record, WindowRecord};
pub(crate) mod lane_stack_growth;
pub(crate) mod lane_generations;
pub(crate) mod native_frames;
use quench_runtime::vm::lane_values::{self, HEAP_WINDOW};
use quench_runtime::vm::lane_activations::LaneActivations;
pub(crate) mod this_binding;
use this_binding::ThisBinding;
pub(crate) mod loop_fields; use loop_fields::loop_field_stores;
pub(crate) mod construct_plan; pub(crate) mod super_chain; use super_chain::super_construction; pub(crate) mod setters; pub(crate) mod proxy_traps; pub(crate) mod accessor_protocol; pub(crate) mod completions; use completions::*;
use construct_plan::{ConstructPlan, ConstructPlanKind, EMPTY_CONSTRUCT_PLAN, initialize_stores_in_lane, add_stores_in_lane, return_constructed_in_lane, planned_construction_in_lane, planned_construction, planned_inline_quick_in_lane, construct_argument_in_lane, construct_plan_index, holding_construct_plan, holding_construct_plan_index, watch_construct_plan, record_forwarded_resolution};
pub(crate) use construct_plan::{body_construction_in_lane, body_receiver_in_lane, context_constructs_in_lane, scalar_construct_plan, construct_plan_holds};
use accessor_protocol::AccessorCall;
pub(crate) mod list_calls;
pub(crate) mod loop_runs;
pub(crate) mod unary_values;
pub(crate) mod lane_capture_store; use lane_capture_store::{store_to_capture, store_to_environment};
use unary_values::{unary_value, unary_values};
use list_calls::{apply_activation_in_lane, list_activation_in_lane, spread_native_call_in_lane};

#[cfg(all(test, feature = "bench-primitives"))]
#[path = "dispatch_fast/primitive_bench/windows.rs"]
pub(crate) mod window_primitives;

#[cfg(test)]
#[path = "dispatch_fast/primitive_bench/activations.rs"]
pub(crate) mod activation_primitives;

#[cfg(test)]
#[path = "dispatch_fast/primitive_bench/commutative_reads.rs"]
pub(crate) mod commutative_read_primitives;

#[cfg(test)]
#[path = "dispatch_fast/primitive_bench/indexed_forwarding.rs"]
pub(crate) mod indexed_forwarding_primitives;

#[cfg(all(test, feature = "bench-primitives"))]
#[path = "dispatch_fast/primitive_bench/indexed_compositions.rs"]
pub(crate) mod indexed_compositions;

#[path = "lane_body_stats.rs"]
pub(crate) mod lane_body_stats;
pub(crate) mod body_calls;
pub(crate) mod body_forms;
pub(crate) mod proven_callees;
pub(crate) use body_calls::{body_call_helper, body_construct_helper};
#[path = "lane_leaves.rs"]
pub(crate) mod lane_leaves;
#[path = "operand_feedback.rs"]
pub(crate) mod operand_feedback;
use operand_feedback::{has_operand_feedback, note_operands, seed_declared_feedback, Records};
#[path = "body_tier_in_lane.rs"]
pub(crate) mod body_tier_in_lane;
#[path = "loop_tier.rs"]
pub(crate) mod loop_tier;
use loop_tier::loop_compile_in_lane;
use lane_leaves::LaneLeaves;

/// Conservative lane-stack admission room for `count` registers: whole blocks, at
/// least one. Only the logical range is initialized and made live.
const fn fill_room<const BLOCK: usize>(count: usize) -> usize {
    let room = count.next_multiple_of(BLOCK);
    if room < BLOCK { BLOCK } else { room }
}

use quench_runtime::vm::engine_types::FIXED_ARGUMENT_COPIES;

pub use quench_runtime::vm::engine_types::copy_arguments;

use quench_runtime::vm::engine_types::WINDOW_FILL_BLOCK;

pub use quench_runtime::vm::engine_types::fill_window;

/// The start of a plain activation of `count` registers, whose admission reserved whole
/// `SCRIPT_FILL_BLOCK`s past them (`ScriptCallTarget::room`), every one of the first `fill`
/// `undefined`: a `fill` within one `WINDOW_FILL_BLOCK` takes that block's fixed stores, the
/// values past it landing in registers no instruction reads before writing or in the reserved
/// room, so it makes no test of its count; a longer one fills all `count`.
///
/// # Safety
/// `registers` has writable storage for `fill_room::<SCRIPT_FILL_BLOCK>(count)` values, none
/// of them live, and `fill` covers every register the activation's function reads before
/// writing (`ResidualProgram::entry_fill`).
#[inline(always)]
unsafe fn fill_admitted_window(registers: *mut Value, fill: usize, count: usize) {
    const _: () = assert!(WINDOW_FILL_BLOCK <= SCRIPT_FILL_BLOCK);
    if fill > WINDOW_FILL_BLOCK {
        // SAFETY: the function's contract.
        return unsafe { fill_window(registers, count, Value::UNDEFINED) };
    }
    for index in 0..WINDOW_FILL_BLOCK {
        // SAFETY: the admitted room holds a whole block.
        unsafe { registers.add(index).write_unaligned(Value::UNDEFINED) };
    }
}

use quench_runtime::vm::engine_types::ActivationShape;

#[quench_macros::ext(ActivationShapeOps1)]
impl ActivationShape {
    /// The window of `function`, when its parameters have registers and it keeps no locals.
    fn of(function: &Function) -> Option<Self> {
        function.parameter_registers.filter(|_| function.locals == 0).map(|parameters| Self {
            registers: function.registers,
            parameters,
            params: function.params,
            initial: function.initial_register.value(),
            fill: entry_fill_known(function),
        })
    }
}

/// The registers of an activation of `function` that start initialized: those below its entry
/// liveness bound. A body whose frames a collection scans whole (`RegisterMasks::too_large_to_keep`)
/// keeps every register initialized, as its masks are not kept.
pub(crate) fn entry_fill(code: &quench_runtime::bytecode::ResidualProgram, function: &Function) -> u16 {
    if quench_runtime::compile::liveness::RegisterMasks::too_large_to_keep(function) {
        function.registers
    } else {
        code.entry_fill(function)
    }
}

/// `entry_fill` where only the function is at hand: its bound where derived already, else every
/// register (a fill that covers all is always sound).
fn entry_fill_known(function: &Function) -> u16 {
    match function.register_roots.entry_live_known() {
        Some(live) if !quench_runtime::compile::liveness::RegisterMasks::too_large_to_keep(function) => live.min(function.registers),
        _ => function.registers,
    }
}

pub(crate) use quench_runtime::vm::engine_types::LaneRecoverySpace;

#[quench_macros::ext(LaneRecoverySpaceOps1)]
impl LaneRecoverySpace {
    /// Frames follow their parents. Take independent maxima: the deepest
    /// chain and the largest register window need not be the same chain. Each frame's window
    /// leaves the next one's context words past its registers (`lane_values::CONTEXT`).
    pub(crate) fn inlined(frames: &[crate::compiled::InlinedCall]) -> Self {
        let mut chains = smallvec::SmallVec::<[Self; 1]>::new();
        let mut space = Self::default();
        for frame in frames {
            let parent = frame.parent.map_or_else(Self::default, |parent| chains[parent]);
            let window = fill_room::<SCRIPT_FILL_BLOCK>(usize::from(frame.registers)) + lane_values::CONTEXT;
            let chain = Self { values: parent.values + window,
                activations: parent.activations + 1 };
            space.values = space.values.max(chain.values);
            space.activations = space.activations.max(chain.activations);
            chains.push(chain);
        }
        space
    }

    /// Room for `registers` frame slots past the caller's, which a body's rows name beyond its inlined windows (its scratch).
    pub(crate) fn reaching(self, registers: usize) -> Self {
        if registers == 0 { self } else { Self { values: self.values.max(fill_room::<SCRIPT_FILL_BLOCK>(registers)), ..self } }
    }
}

use quench_runtime::vm::engine_types::LaneBody;





#[quench_macros::ext(LaneViewOps2)]
impl LaneView {
    /// The specialized frameless body, if there is one.
    #[inline]
    fn body_view_in_lane(&self) -> Option<std::rc::Rc<crate::compiled::LaneMethod>> {
        // SAFETY: this view is read on its VM's thread. Clone the code owner before
        // execution or recovery can reenter and change the view's publication.
        match unsafe { &*self.tier().tier_lane.get() } {
            LaneBody::Specialized { method, .. } => method.lease(),
            LaneBody::Unspecialized { .. } | LaneBody::Compiling { .. } | LaneBody::Declined => None,
        }
    }

    /// Whether the view's body is retired to `Declined` (`LaneBody::Declined`): a call of the view has no body to enter.
    fn declined_in_lane(&self) -> bool {
        // SAFETY: as `body_view_in_lane`; read only.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Declined)
    }

    /// The published closed body a direct caller pins (`compiled::DirectCallee::method`, `body`), which a
    /// caller of the function itself, or of one with none, leaves to the entry cell it calls through
    /// (the stub enters nothing).
    fn pinned_callee(&self, own: bool) -> (Option<std::rc::Rc<crate::compiled::PublishedLaneMethod>>, Option<std::rc::Rc<crate::compiled::LaneMethod>>) {
        let method = self.published_lane_body().filter(|_| !own);
        let body = method.as_ref().and_then(|method| method.lease()).filter(|body| body.calls_only_natively());
        if body.is_some() { (method, body) } else { (None, None) }
    }

    /// The body its entry cell holds, which every caller enters through the cell's one entry
    /// word: `None` while the cell holds the stub (no closed body is published, or it retired),
    /// whose entries take `lane_body` or the warming path. Owned as `lane_body` owns it, so a
    /// reentrant change of the publication leaves the run its code.
    fn cell_body(&self) -> Option<std::rc::Rc<crate::compiled::LaneMethod>> {
        let cell = &self.entry;
        self.entry_published().then(|| {
            let body = cell.body.get() as *const crate::compiled::LaneMethod;
            // SAFETY: a published cell's `body` is `Rc::as_ptr` of the body `pinned` keeps
            // (`EntryCell::publish`); the strong count taken here is the run's own.
            unsafe {
                std::rc::Rc::increment_strong_count(body);
                std::rc::Rc::from_raw(body)
            }
        })
    }

    /// The body its entry cell holds, borrowed: the view pins every body it published until it
    /// drops (`pinned`), and the program store keeps the view alive through any lane run, so a
    /// run needs no count of its own, and a retirement during the run only unlinks the entry.
    #[inline(always)]
    fn cell_method(&self) -> Option<&crate::compiled::LaneMethod> {
        // SAFETY: a published cell's `body` is `Rc::as_ptr` of a body `pinned` keeps (`EntryCell::publish`).
        self.entry_published().then(|| unsafe { &*(self.entry.body.get() as *const crate::compiled::LaneMethod) })
    }

    /// Whether its prologue says the body reads its receiver (`claim_for_tier`'s `binds_this`).
    #[inline(always)]
    fn binds_this(&self) -> bool {
        // SAFETY: the lane runs on the view's thread; nothing writes the prologue during a call.
        unsafe { (*self.prologue.get()).b != 0 }
    }

    /// Whether its entry cell holds a published body rather than the stub.
    #[inline(always)]
    fn entry_published(&self) -> bool {
        // The body word, not the entry word: the stub is a private function whose address the
        // linker may fold into more than one copy, so comparing entry words by identity reports
        // a withdrawn cell as published (and a published body's pointer as absent).
        self.entry.body.get() != 0
    }

    /// The address of its entry cell, which native callers name (`compiled::DirectCallee::cell`).
    pub(crate) fn entry_cell(&self) -> usize { std::ptr::from_ref(&self.entry).addr() }

    /// The publication of the specialized body, which a direct caller holds
    /// (`compiled::DirectCallee::method`).
    fn published_lane_body(&self) -> Option<std::rc::Rc<crate::compiled::PublishedLaneMethod>> {
        // SAFETY: as `lane_body`.
        match unsafe { &*self.tier().tier_lane.get() } {
            LaneBody::Specialized { method, .. } => Some(method.clone()),
            _ => None,
        }
    }

    /// A specialized body whose publication a lifecycle edge retired
    /// (`ProgramStore::retire_dead_native_calls`) warms again: its prologue takes back the
    /// tier's handler.
    fn settle_retired_body(&self) {
        // SAFETY: as `lane_body`; settling runs no lane body.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        if let LaneBody::Specialized { method, tier_prologue, .. } = body
            && !method.is_published()
        {
            // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
            unsafe { (*self.prologue.get()).handler = *tier_prologue };
            self.drop_body(body);
        }
    }


    /// Whether its body left too often and specializes again at its next entry.
    #[inline]
    pub(crate) fn respecializes(&self) -> bool {
        // SAFETY: as `lane_body`.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Unspecialized { recompiles, .. } if *recompiles > 0)
    }

    /// The own-field hint of the field record at `pc`, as the layout id (`Object::hint_shape`)
    /// of the receiver it last answered: what a specializer guards in one compare to read the
    /// record's storage form with no test (`NativeFieldLoad::formed`).
    /// `None` where the record holds no own hint, or is no hinted field record.
    pub(crate) fn own_field_hint(&self, pc: usize) -> Option<u32> {
        let pc = u32::try_from(pc).ok()?;
        if !self.field_records.contains(&pc) { return None; }
        // SAFETY: each listed record's operand is `field` (`holds_field_hint`).
        let shape = unsafe { (*self.records[pc as usize].get()).operand.field.shape };
        (shape < HOLDER_HINT).then_some(shape)
    }

    /// How many of its records hold an own-field hint. Profiled builds derive no lane views,
    /// so only the unprofiled test reads it.
    #[cfg(all(test, not(quench_profiled)))]
    pub(crate) fn held_field_hints(&self) -> usize {
        self.held_hints(&self.field_records)
    }

    /// How many of its by-name loads hold a global-object hint (`name_row`).
    #[cfg(all(test, not(quench_profiled)))]
    pub(crate) fn held_name_hints(&self) -> usize {
        self.held_hints(&self.name_records)
    }

    #[cfg(all(test, not(quench_profiled)))]
    fn held_hints(&self, pcs: &[u32]) -> usize {
        pcs.iter()
            // SAFETY: each listed record's operand is `field` (`holds_field_hint`).
            .filter(|&&pc| unsafe { (*self.records[pc as usize].get()).operand.field.shape } != quench_runtime::vm::NO_FIELD_RECEIVER)
            .count()
    }

    /// The address range its records span (`lane_exits::register_view`).
    #[cfg(feature = "profile-lane-exits")]
    pub(crate) fn record_range(&self) -> (usize, usize) {
        let start = self.records().addr();
        (start, start + self.records.len() * std::mem::size_of::<LaneInstruction>())
    }

    /// The record a frameless activation starts at: the first, or the tier prologue.
    #[inline(always)]
    fn start(&self) -> Ip {
        self.start.get()
    }

    /// Starts the function's frameless activations at the tier prologue, which enters the
    /// JavaScript tier with its `params` parameters (`tier_prologue_in_lane`). Its fields: `imm`
    /// the function, `a` its parameter count, `b` whether it binds `this` (`body_entry_in_lane`).
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
                mode: 0,
            };
        }
        self.start.set(prologue.cast_const());
    }

    /// Whether a call's resolution claims the prologue for the lane tier: while its body warms
    /// (a hot loop may start its compile before any call resolves it, `loop_retire_in_lane`)
    /// and nothing claimed it yet. A specialized body's prologue already enters it, and any
    /// other body (declined, retired, or serving native callers only) runs its function in
    /// the lane; claiming again would take the prologue back to `tier_prologue_in_lane`.
    fn tier_unclaimed_in_lane(&self) -> bool {
        self.warming() && self.start.get() != self.prologue.get().cast_const()
    }

    /// Starts activations at the tier prologue again, which `claim_for_tier` wrote.
    fn reclaim_for_tier(&self) {
        // A function no call claimed (`LaneView::claim_for_tier`) has no prologue to start at.
        // SAFETY: as `claim_for_tier`.
        if unsafe { (*self.prologue.get()).handler }.is_null() { return; }
        self.start.set(self.prologue.get().cast_const());
    }
}

/// Switches the record at `ip` to `handler` for the rest of its view's life: a one-way step of
/// the record's state machine, taken when an execution of the record shows the handler it has
/// does not fit the site. Every handler a record may switch between computes the same result.
#[inline(always)]
fn requicken(ip: Ip, handler: Handler) {
    // SAFETY: `ip` comes from `LaneView::records`, whose records live in `UnsafeCell`s; the
    // lane runs on the view's thread, and no reference to the handler field is live. A
    // header record a loop site holds keeps `loop_header_miss`, and the site the handler.
    unsafe {
        if (*ip).handler == loop_header_miss as Handler as *const () {
            let site = header_site_in_lane(ip);
            (*site).header_handler.set(handler as *const ());
            return;
        }
        if loop_kernel::requicken_kernel(ip, handler) {
            return;
        }
        (*ip.cast_mut()).handler = handler as *const ();
    }
}

/// The context words below register 0 (`staged::frame`, the one source of the frame layout).
use quench_runtime::staged::frame::{CONTEXT_BODY, CONTEXT_CALL, CONTEXT_CALLEE, CONTEXT_CALLER_BASE, CONTEXT_CALLER_REGISTERS, CONTEXT_CAPTURE, CONTEXT_ENV, CONTEXT_PACKED, CONTEXT_THIS};
const _: () = assert!(CONTEXT_BODY == quench_runtime::vm::register_file::FRAME_CONTEXT);
const _: () = assert!(CAPTURE_STORES as u64 == quench_bjit::shared_words::CAPTURE_STORES_BIT);

/// The header word `index` (a `staged::frame` `CONTEXT_*`) of the window at `registers`: the words count
/// down from register 0, so `sub(index)` words below it.
///
/// # Safety
/// `registers` is a live window, with the frame's context words below it.
#[inline(always)]
pub(crate) unsafe fn frame_word(registers: *const Value, index: usize) -> u64 {
    // SAFETY: as the caller promises.
    unsafe { *registers.cast::<u64>().sub(index) }
}

/// Writes the header word `index` of the window at `registers` (`frame_word`).
///
/// # Safety
/// As `frame_word`, and the caller owns the window's header.
#[inline(always)]
pub(crate) unsafe fn set_frame_word(registers: *mut Value, index: usize, word: u64) {
    // SAFETY: as the caller promises.
    unsafe { *registers.cast::<u64>().sub(index) = word }
}
use quench_runtime::vm::engine_types::Registers;

#[quench_macros::ext(RegistersOps1)]
impl Registers {
    #[inline(always)]
    fn set_this(self, this: Value) {
        // SAFETY: as for `this`.
        unsafe { *self.0.sub(CONTEXT_THIS) = this }
    }

    #[inline(always)]
    fn set_env(self, env: Value) {
        // SAFETY: as for `this`.
        unsafe { *self.0.sub(CONTEXT_ENV) = env }
    }

    /// The frame's captures (`ScriptCallTarget::capture`), the context word below its
    /// environment: null where none are read yet (a VM frame's, until first found).
    #[inline(always)]
    fn capture(self) -> *const usize {
        // SAFETY: as for `this`; the word holds a pointer's bits.
        unsafe { *self.0.sub(CONTEXT_CAPTURE).cast::<*const usize>() }
    }

    #[inline(always)]
    fn set_capture(self, capture: *const usize) {
        // SAFETY: as for `capture`.
        unsafe { *self.0.sub(CONTEXT_CAPTURE).cast::<*const usize>() = capture }
    }

    /// Write the plain header's call record, caller registers and caller records: a trampoline's
    /// header taking its trampoline's words as its own (`LaneStack::edit_top_side`), a moved
    /// caller window (`LaneStack::relocate_plain`).
    #[inline(always)]
    fn set_plain_caller(self, call: Ip, registers: Registers, base: Ip) {
        // SAFETY: as for `plain_caller`; the words are the header's own.
        unsafe {
            *self.0.sub(CONTEXT_CALL).cast::<Ip>() = call;
            *self.0.sub(CONTEXT_CALLER_REGISTERS).cast::<Registers>() = registers;
            *self.0.sub(CONTEXT_CALLER_BASE).cast::<Ip>() = base;
        }
    }

    /// The plain header's function, argc and callee.
    #[inline(always)]
    fn plain_callee(self) -> (u32, u16, Value) {
        // SAFETY: as for `plain_caller`.
        unsafe {
            let packed = *self.0.sub(CONTEXT_PACKED).cast::<u64>();
            // Emitted direct calls' plain frames (compiled bodies, native frames) are spilled
            // by their rows' own branch of `LaneStack::spill_plain` once those rows write them.
            // A native header (NATIVE_FLAG) is plain: it names its callee in the same word until linked.
            debug_assert!(packed & quench_runtime::staged::frame::BODY_FLAG == 0);
            (packed as u32, (packed >> quench_runtime::staged::frame::ARGC_SHIFT) as u16, *self.0.sub(CONTEXT_CALLEE))
        }
    }

    /// The register's int32, read as two dwords: the tag half compared in memory
    /// (`cmp dword [..+4], imm; jne`) and the value half loaded as it is, where a whole-word
    /// read pays a copy, a shift and a compare for the tag (V8-v7 Crypto's integer records).
    #[inline(always)]
    fn int(self, register: u16) -> Option<quench_runtime::staged::Int32<u64>> {
        // SAFETY: as for `get`; a Value is two dwords, the tag in the high one (little endian).
        let halves = unsafe { self.0.add(usize::from(register)) }.cast::<u32>();
        quench_runtime::staged::Int32::guard_halves(unsafe { halves.add(1).read() }, || unsafe { halves.read() })
    }

    #[inline(always)]
    fn i32(self, register: u16) -> i32 {
        self.get(register).wasm_bits32() as i32
    }
}

use quench_runtime::vm::engine_types::Slot;

#[quench_macros::ext(SlotOps1)]
impl<T: Copy> Slot<T> {
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

use quench_runtime::vm::engine_types::MemoryView;

/// The widest access a lane record makes directly.
use quench_runtime::vm::engine_types::WIDEST_DIRECT_ACCESS;

#[quench_macros::ext(MemoryViewOps1)]
impl MemoryView {
    const NONE: Self = Self::new(std::ptr::null_mut(), 0);

    /// A JavaScript run's view. JavaScript binds no memory (`frame_memory_view`), so the
    /// register a Wasm view's bytes take carries the run's context instead, which
    /// `script_handler!` handlers read without the thread-local load. Its reach is unset: no
    /// JavaScript record accesses memory.
    #[inline(always)]
    fn script(cx: *mut LaneContext) -> Self {
        Self {
            bytes: cx.cast(),
            reach: Slot::unset(),
        }
    }

    /// The base and length of the memory the view addresses, for a row that reads its bytes itself
    /// (`Decoder::WasmMemory`). The reach is the length less the widest access, so the length is the
    /// reach plus that width, and `NONE` is empty.
    ///
    /// SAFETY: the view must have been resolved during the current lane run.
    #[inline(always)]
    unsafe fn extent(self) -> (*mut u8, usize) {
        let length = unsafe { self.reach.get() } + WIDEST_DIRECT_ACCESS as isize;
        (self.bytes, length.max(0) as usize)
    }
}

/// Whether each opcode's field A is its result register (`FieldLayout::ResultRegister`), indexed by the
/// opcode's catalog index. Built from the catalog once, so a general step reads one entry for it instead of
/// looking up the operand layout per step (`Carried::stepped`).
const RESULT_A_OPS: [bool; quench_runtime::bytecode::Op::COUNT] = {
    let mut table = [false; quench_runtime::bytecode::Op::COUNT];
    let mut index = 0;
    while index < quench_runtime::bytecode::Op::COUNT {
        table[index] = match quench_runtime::bytecode::Op::from_index(index) {
            Some(op) => matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister),
            None => panic!("catalog index is below the opcode count"),
        };
        index += 1;
    }
    table
};

/// The state handlers carry from one to the next in machine registers.
#[derive(Clone, Copy)]
struct Carried {
    memory: MemoryView,
    integers: quench_runtime::value::IntegerEncoding,
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
    fn script(cx: *mut LaneContext) -> Self {
        Self {
            memory: MemoryView::script(cx),
            integers: quench_runtime::value::IntegerEncoding::TAG,
            acc: Slot::unset(),
        }
    }

    /// `script` with the integer encoding `incoming` carried in: the same constant
    /// (`IntegerEncoding::TAG`), passed on in the register it arrived in, so a record makes no
    /// copy of it for the next.
    #[inline(always)]
    fn script_in(cx: *mut LaneContext, incoming: Carried) -> Self {
        Self { integers: incoming.integers, ..Self::script(cx) }
    }

    /// What a record that writes its result register carries on: `incoming` with no accumulator, since
    /// no earlier producer's accumulator holds past the write. The memory view stays: a Wasm record
    /// (`WasmI32AddImmediateJumpNonZero`) runs where later records address linear memory through it.
    #[inline(always)]
    fn past_result(incoming: Carried) -> Self {
        Self { acc: Slot::unset(), ..incoming }
    }

    /// What the general step of `instruction` carries on: `script_in`, with the accumulator its
    /// result register now holds, since the record was selected as an accumulator producer
    /// (`LaneHandler::produces`) and the next record may read the accumulator for it.
    #[inline(always)]
    fn stepped(cx: *mut LaneContext, incoming: Carried, r: Registers, instruction: WideInstruction) -> Self {
        let produced = RESULT_A_OPS[instruction.op() as usize];
        let acc = if produced { Slot::of(r.get(instruction.result_register())) } else { Slot::unset() };
        Self { acc, ..Self::script_in(cx, incoming) }
    }
}

use quench_runtime::vm::engine_types::LANE_STACK_ACTIVATIONS;

pub(crate) use quench_runtime::vm::engine_types::Activation;

/// The tags of `ActivationKind`: the discriminants, which emitted code writes as the first
/// word of the kind (`NATIVE_TAG`) and the lane reads to tell a native frame from an activation.
pub(crate) use quench_bjit::shared_words::KIND_NATIVE;
pub(crate) use quench_runtime::vm::engine_types::KIND_OFFSET;
const _: () = assert!(KIND_OFFSET == quench_runtime::staged::frame::ACTIVATION_KIND_OFFSET && KIND_NATIVE == quench_runtime::staged::frame::KIND_NATIVE);
const _: () = assert!(std::mem::size_of::<Activation>() == quench_runtime::staged::frame::ACTIVATION_BYTES);
const _: () = {
    use quench_runtime::staged::frame;
    assert!(std::mem::offset_of!(Activation, callee) == frame::CALLEE);
    assert!(std::mem::offset_of!(Activation, this) == frame::THIS);
    assert!(std::mem::offset_of!(Activation, body) == frame::BODY);
    assert!(std::mem::offset_of!(Activation, window) == frame::WINDOW);
    assert!(frame::WINDOW + std::mem::size_of::<usize>() == frame::PREFIX_BYTES);
    assert!(std::mem::size_of::<Activation>().is_power_of_two());
};

use quench_runtime::vm::engine_types::ActivationKind;

const _: () = {
    assert!(KIND_OFFSET % std::mem::size_of::<u64>() == 0);
};

#[quench_macros::ext(ActivationOps1)]
impl Activation {
    /// The pc of the call record this activation was entered from, in its caller's body
    /// (`caller_base`); `None` for a native frame, which keeps no link state.
    fn caller_pc(&self) -> Option<usize> {
        // SAFETY: a linked activation's call record lies in its caller's body.
        (!matches!(self.kind, ActivationKind::Native)).then(|| unsafe { self.call.offset_from(self.caller_base) } as usize)
    }

    /// The program and noted record of a Wasm activation (`LaneStack::note_wasm_pc`); `None`
    /// for any other kind.
    pub(crate) fn wasm_site(&self) -> Option<(ProgramId, usize)> {
        match self.kind {
            ActivationKind::Wasm { program, pc, .. } | ActivationKind::WasmEntry { program, pc, .. } => Some((program, pc as usize)),
            ActivationKind::Native | ActivationKind::Script(_) => None,
        }
    }

    /// The memory view its caller runs with: a JavaScript activation's caller is JavaScript,
    /// whose view carries the context (`MemoryView::script`).
    fn caller_memory(&self, cx: *mut LaneContext) -> MemoryView {
        match self.kind {
            ActivationKind::Native | ActivationKind::Script(_) | ActivationKind::WasmEntry { .. } => MemoryView::script(cx),
            ActivationKind::Wasm { memory, .. } => memory,
        }
    }

    #[inline] fn frame_storage_bytes(&self) -> usize {
        match self.kind {
            ActivationKind::Native | ActivationKind::Script(_) => 0,
            ActivationKind::Wasm { frame_storage_bytes, .. } | ActivationKind::WasmEntry { frame_storage_bytes, .. } => frame_storage_bytes,
        }
    }
}

use quench_runtime::vm::engine_types::ScriptActivation;

/// The activation record's one access path (call protocol v2 step 3,
/// `/mnt/project-files/calls-frames/call-protocol-v2.md`): every reader and writer outside
/// this block goes through these methods, so the record's layout (and, next, where a plain
/// activation's header lives: the value stack) changes here alone.
#[quench_macros::ext(ActivationOps2)]
impl Activation {
    /// An activation of `kind` with no frame prefix: no callee, receiver or body, and no window yet.
    #[inline(always)]
    fn of_parts(function: u32, call: Ip, caller_base: Ip,
        caller_registers: Registers, caller_program: ProgramId, kind: ActivationKind) -> Self {
        Self { callee: std::ptr::null(), this: 0, body: 0, window: 0, function, call, caller_base, caller_registers, caller_program, kind }
    }
    #[inline(always)] fn kind_mut(&mut self) -> &mut ActivationKind { &mut self.kind }
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

#[quench_macros::ext(ActivationOps3)]
impl Activation {
    /// The address of this activation's first register: its prefix's WINDOW word without its
    /// mark (`HEAP_WINDOW`), which names the lane stack's values by default and any other
    /// register file (a generator's) alike.
    #[inline(always)]
    pub(crate) fn window_ptr(&self) -> *mut Value {
        (self.window & !HEAP_WINDOW) as *mut Value
    }

    /// The prefix's WINDOW word as it stands, which a return hands the lane stack's values to
    /// free the window if the stack holds it (`LaneValues::release_window`).
    #[inline(always)]
    pub(crate) fn window_word(&self) -> usize {
        self.window
    }

    /// The first `len` values from the window: its registers, and what follows them
    /// (`ScriptActivation::forwards`, `environment`).
    ///
    /// # Safety
    /// The window holds `len` initialized values, which nothing else writes while the slice lives.
    #[inline(always)]
    unsafe fn window(&self, len: usize) -> &[Value] {
        unsafe { std::slice::from_raw_parts(self.window_ptr(), len) }
    }
}

impl ActivationState {
    /// Each activation's state, innermost from the context, each other's from the caller
    /// state the next script activation above it saved. A Wasm activation changes neither
    /// receiver nor environment; every activation names its caller's program.
    fn of(activations: &[Activation], cx: *mut LaneContext) -> Vec<Self> {
        let mut current = unsafe { Self { this: Value::UNDEFINED, env: (*cx).registers.env(), program: (*cx).program } };
        let mut states = vec![current; activations.len()];
        for (state, activation) in states.iter_mut().zip(activations).rev() {
            *state = current;
            current.program = activation.caller_program();
        }
        // Each script activation's receiver is its window's context word; a Wasm activation
        // runs with its caller's, the frame's below the first.
        let first = activations.first().map(|first| first.caller_registers());
        let mut this = first.map_or(Value::UNDEFINED, Registers::this);
        let mut env = first.map_or(Value::DELETED, Registers::env);
        for (state, activation) in states.iter_mut().zip(activations) {
            if activation.script().is_some() {
                let window = Registers(activation.window_ptr());
                (this, env) = (window.this(), window.env());
            }
            (state.this, state.env) = (this, env);
        }
        states
    }
}

use quench_runtime::vm::engine_types::Completion;

pub(crate) use quench_runtime::vm::engine_types::JsImportPlan;

pub(crate) use quench_runtime::vm::engine_types::JS_IMPORT_ARGUMENTS;
pub(crate) const JS_IMPORT_I32: u8 = 0;
pub(crate) const JS_IMPORT_F32: u8 = 1;
pub(crate) const JS_IMPORT_F64: u8 = 2;

pub(crate) use quench_runtime::vm::engine_types::ToWasmCall;

#[quench_macros::ext(CompletionOps1)]
impl Completion {
    /// The completion of an activation a `new` made or not, of the caller's program or not,
    /// of a target whose return settles its call's promise or not. Only a plain call of the
    /// caller's program settles one (`script_call_settles_miss`).
    #[inline] fn of(constructs: bool, cross: bool, settles: bool) -> Self {
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
    #[inline] fn constructs(self) -> bool {
        matches!(self, Self::Constructs | Self::CrossConstructs | Self::ConstructsToBody)
    }

    /// Whether the activation runs a generator over its record's register file.
    #[inline] fn is_generator(self) -> bool {
        matches!(self, Self::Generates | Self::Delegates | Self::Steps | Self::StepReads(_))
    }

    /// Whether a generator's step yields this realm's reused iterator result, which only the
    /// step's own reads observe: a delegate's step makes it fresh, as `Steps` returns it fresh.
    #[inline] fn reuses_result(self) -> bool {
        matches!(self, Self::Generates | Self::StepReads(_))
    }

    /// Whether the activation runs over the register file of a heap record's frame, a
    /// generator's or a job's, which names its owner as its callee.
    #[inline] fn is_windowed(self) -> bool {
        self.is_generator() || matches!(self, Self::ResumesJob | Self::AsyncRequest)
    }

    /// Whether the activation's return makes the caller's program the lane's again.
    #[inline] fn crosses(self) -> bool {
        matches!(self, Self::Cross | Self::CrossConstructs)
    }

    /// Whether a header of this completion carries the completion side flag
    /// (`frame::COMPLETION_FLAG`): every completion past `Plain` and `Cross`, which a return
    /// completes by the header's side record rather than the plain pop.
    #[inline] fn needs_completion_flag(self) -> bool {
        !matches!(self, Self::Plain | Self::Cross)
    }
}

pub(crate) use quench_runtime::vm::engine_types::SkippedForwarder;

/// Where a lane activation's arguments come from: a window of the caller's registers, or
/// values a forwarding activation or frame kept (`f.apply(this, arguments)`).
#[derive(Clone, Copy)]
enum ActivationArguments {
    Window(u16),
    Kept(*const Value, usize),
}

pub(crate) use quench_runtime::vm::engine_types::ScriptCallTarget;

pub(crate) mod call_entry;
use call_entry::CallEntry;

// Recognized once per target, as it resolves; no operation runs them.
#[path = "call_body_forms.rs"]
pub(crate) mod call_body_forms;
use call_body_forms::{body_entry, leaf_body};

/// `ScriptCallTarget::forms`: `new` may activate it.
const TARGET_CONSTRUCTS: u8 = 1;
/// `ScriptCallTarget::forms`: `new` may run its body as the stores alone.
const TARGET_INITIALIZES: u8 = 2;
/// `ScriptCallTarget::forms`: a rest body (`CallEntry::Gathers`) that keeps the values it was
/// passed, for a kept `arguments` (`CallPlan::keeps_arguments`) or legacy reflection of a
/// sloppy function's; any other's rest array holds every value past its parameters.
const TARGET_GATHERS_KEEPING: u8 = 4;
/// `ScriptCallTarget::forms`: a rest body whose rest parameter only `RestGet` and
/// `RestLength` read (`Function::rest_view`): a plain call's activation keeps the values
/// passed and makes no rest array (`activate_gathering_in_lane`).
const TARGET_REST_VIEW: u8 = 8;
/// `ScriptCallTarget::forms`: an arrow that may read its maker's implicit bindings
/// (`Function::arrow_reads_implicit_bindings`), whose sibling closures read their own
/// lexical `this` (`sibling_closure_target`).
const TARGET_READS_IMPLICIT: u8 = 16;
/// `ScriptCallTarget::forms`: a settling target (`CallEntry::Settles`) whose body activates
/// over an environment window (`activate_environment_in_lane`): an async body whose locals and
/// closures ride the window (`CallPlan::suspended_environment`).
const TARGET_SETTLES_IN_ENVIRONMENT: u8 = 32;

#[quench_macros::ext(ScriptCallTargetOps1)]
impl ScriptCallTarget {
    /// The closure environment and captures pointer its activation's window holds
    /// (`CONTEXT_ENV`, `CONTEXT_CAPTURE`): a direct call row writes them (`CallRecord::env`, `capture`).
    #[inline(always)]
    pub(crate) fn context_words(&self) -> (u64, u64) {
        (self.env.to_bits(), self.capture.expose_provenance() as u64)
    }

    /// `new` may activate it too (`Vm::inline_constructible`).
    #[inline(always)]
    fn constructs(&self) -> bool {
        self.forms & TARGET_CONSTRUCTS != 0
    }

    /// `new` may run its body as the stores alone (`Function::only_initializes_this_fields`).
    #[inline(always)]
    fn initializes(&self) -> bool {
        self.forms & TARGET_INITIALIZES != 0
    }

    /// The registers its activation starts with their initial value: `fill`, exact below
    /// `u8::MAX`, else every register.
    #[inline(always)]
    fn entry_registers(&self) -> usize {
        if self.fill < u8::MAX { usize::from(self.fill) } else { usize::from(self.registers) }
    }

    /// No legacy `arguments` reflection observes its activation: a strict function's or an
    /// arrow's (`Function::has_restricted_legacy_caller_access`), which its `this` binding
    /// names. A call passing values past its parameters need not keep them, as its frame
    /// would not (`Vm::initialize_frame_invocation`); a sloppy function's keeps them.
    #[inline(always)]
    fn drops_extra_arguments(&self) -> bool {
        !matches!(self.this_binding, ThisBinding::Sloppy)
    }

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

    /// Its settling activation holds an environment window (`TARGET_SETTLES_IN_ENVIRONMENT`).
    #[inline(always)]
    fn settles_in_environment(&self) -> bool {
        self.forms & TARGET_SETTLES_IN_ENVIRONMENT != 0
    }
}

use quench_runtime::vm::engine_types::ScriptGeneration;

use quench_runtime::vm::engine_types::Admission;

#[quench_macros::ext(AdmissionOps1)]
impl Admission {
    #[inline(always)]
    const fn generation(self) -> ScriptGeneration {
        self.0 as u32
    }

    /// The generation's bits and the lane's condition bits: what a link proved for any program.
    const RUN: u64 = ScriptGeneration::MAX as u64 | Self::CONDITIONS;

    /// Whether a link made under `self` for another program admits a call in the lane whose
    /// admission is `lane`: the generation and conditions in one compare (a link carries no
    /// condition bit, so a run that calls no script functions or has a construction pending
    /// refuses it, as it refuses its own program's links), and a program other than the lane's.
    #[inline(always)]
    fn admits_across(self, lane: Admission) -> bool {
        (self.0 ^ lane.0) & Self::RUN == 0 && self.program() != lane.program()
    }

    /// Whether `self` was made in the lane's current generation and program, whatever the lane's
    /// conditions and diagnostics: what a plan built now may rely on (`direct_callee_in_lane`).
    /// Diagnostics only observe; `UNPROVEN` only gates the plain call, never a direct one (via the cell).
    #[inline(always)]
    fn current(self, lane: Admission) -> bool {
        (self.0 ^ lane.0) & !(Self::CONDITIONS | Self::DIAGNOSED | Self::UNPROVEN) == 0
    }

    /// Whether `self` was made in the lane's current generation, whatever its program.
    #[inline(always)]
    fn of_generation(self, lane: Admission) -> bool {
        self.generation() == lane.generation()
    }

    /// Whether an entry made under `self` admits a call under the lane's `lane`, proved or
    /// not: every unproved path's test (`Vm::script_call_holds`, the relinks). The proved
    /// call compares the words whole.
    #[inline(always)]
    const fn admits(self, lane: Admission) -> bool {
        self.0 & !Self::UNPROVEN == lane.0
    }
}

#[quench_macros::ext(ScriptCallTargetOps2)]
impl ScriptCallTarget {
    #[inline(always)]
    fn generation(&self) -> ScriptGeneration {
        self.admission.generation()
    }

    #[inline(always)]
    pub(crate) fn program(&self) -> ProgramId {
        self.admission.program()
    }
}

/// The rest values a gathering activation collects inline before its array is made.
const GATHERED_INLINE_VALUES: usize = 8;

/// The size a script call cache entry keeps, one cache line, which each call reads.
const SCRIPT_CALL_TARGET_BYTES: usize = 64;
const _: () = assert!(std::mem::size_of::<ScriptCallTarget>() <= SCRIPT_CALL_TARGET_BYTES);

use quench_runtime::vm::engine_types::NO_FORWARDED_METHOD;

use quench_runtime::vm::engine_types::EMPTY_SCRIPT_CALL_TARGET;

/// Entries of the script call cache, in sets of `SCRIPT_TARGET_WAYS` that a hash of the
/// callee's heap index picks (`ScriptTargets::set`), at most, under each memory policy
/// (`MemoryPolicy` order): Express's router calls a matcher closure per layer, some 400
/// distinct callees a request, which 256 entries thrashed (Callgrind: 2.4% of a ping request
/// re-resolving). 1024 still met conflicts in their two-way sets, re-resolving 34 callees a
/// body request (~380 instructions each); 4096 (256 KiB per table, reached only by a cache
/// that keeps displacing live targets) cut express body and ping 1.3% (Callgrind, per
/// request). Memory-first stops at 1024: a long run that displaces a few targets at a time
/// reaches the larger size too (EarleyBoyer, both tables: +220 KiB anonymous peak).
const SCRIPT_CALL_TARGETS: [usize; quench_runtime::MemoryPolicy::ALL.len()] = [4096, 1024];

/// The largest script call cache `policy` allows.
fn script_call_targets(policy: quench_runtime::MemoryPolicy) -> usize {
    SCRIPT_CALL_TARGETS[policy as usize]
}
use quench_runtime::vm::engine_types::SCRIPT_CALL_TARGETS_INITIAL;
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
use quench_runtime::vm::engine_types::SCRIPT_TARGET_WAYS;
const _: () = assert!(SCRIPT_TARGET_WAYS == 2);

use quench_runtime::vm::engine_types::ScriptTargets;

#[quench_macros::ext(ScriptTargetsOps1)]
impl ScriptTargets {
    /// The first entry of `callee`'s set, when it names a heap cell; the set's other entry
    /// follows it (`| 1`). The set is the shared direct-mapped index of the heap index
    /// (`shape_memo::direct_mapped_index`): a family's closures take indices at a fixed
    /// stride, which a multiplicative hash of the index alone crowds into a few sets.
    #[inline(always)]
    fn set(&self, callee: Value) -> Option<usize> {
        Some(quench_runtime::vm::shape_memo::direct_mapped_index_by_shift(callee.heap_index()?, self.set_shift) * SCRIPT_TARGET_WAYS)
    }

    /// Record `target` first in its set, the entry it displaces moving second; a cache that
    /// has displaced more live targets than it holds grows, empty, before recording it, up to
    /// `most` entries.
    fn remember(&mut self, mut target: ScriptCallTarget, most: usize) {
        // No entry is proved before a plain call's link proves it (`Admission::UNPROVEN`).
        target.admission.0 |= Admission::UNPROVEN;
        let Some(set) = self.set(target.callee) else {
            return;
        };
        if self.entries[set].callee != target.callee {
            if self.entries[set | 1].generation() == target.generation() {
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
}



use quench_runtime::vm::engine_types::FUNCTION_TARGETS;
const _: () = assert!(FUNCTION_TARGETS.is_power_of_two());

use quench_runtime::vm::engine_types::ForwardedResolution;

pub(crate) use quench_runtime::vm::engine_types::CONSTRUCT_PLAN_FIELDS;
// A plan's values fill a receiver's inline slots (`ConstructPlan::allocate`).
const _: () = assert!(INLINE_PROPERTY_COUNT <= CONSTRUCT_PLAN_FIELDS);
// `ConstructPlan::defaulted` holds a bit per field.
const _: () = assert!(CONSTRUCT_PLAN_FIELDS <= u8::BITS as usize);

use quench_runtime::vm::engine_types::CONSTRUCT_PLANS;
const _: () = assert!(CONSTRUCT_PLANS.is_power_of_two());

/// A `ConstructPlan::epoch` no epoch reaches: the entry plans nothing.

pub(crate) use quench_runtime::vm::engine_types::LaneStack;

/// Drops the entries of `held` that activations from index `depth` on own. Activations push
/// their entries innermost last, so the owners ascend and those entries are a suffix: the
/// common case, none, is one comparison.
#[inline(always)]
fn drop_owned_entries<T>(held: &mut Vec<(usize, T)>, depth: usize) {
    if held.last().is_some_and(|(owner, _)| *owner >= depth) {
        let first = held.partition_point(|(owner, _)| *owner < depth);
        held.truncate(first);
    }
}

pub(crate) use quench_runtime::vm::engine_types::PlainFrames;
pub(crate) const PLAIN_DEPTH_OFFSET: usize = std::mem::offset_of!(PlainFrames, depth);
pub(crate) const PLAIN_REGISTERS_OFFSET: usize = std::mem::offset_of!(PlainFrames, registers);
const _: () = {
    use quench_runtime::staged::frame;
    assert!(PLAIN_DEPTH_OFFSET == frame::PLAIN_DEPTH_AT && PLAIN_REGISTERS_OFFSET == frame::PLAIN_REGISTERS_AT);
    assert!(std::mem::offset_of!(PlainFrames, program) == frame::PLAIN_PROGRAM_AT);
};

/// The addresses a lane run's direct calls push onto (`Vm::stack_words_in_lane`).
#[derive(Clone, Copy)]
pub(crate) struct LaneStackWords {
    pub(crate) values_top: usize,
    pub(crate) values_end: usize,
    pub(crate) activations_top: usize,
    pub(crate) frames_end: usize,
    /// The plain frames' words (`plain_frames_word`), and the cells of the native header bound, the frames
    /// below the plain headers, and the body top (`LaneStack::plain_depth_limit` and the rest).
    pub(crate) plain_depth: usize,
    pub(crate) plain_depth_limit: usize,
    pub(crate) activation_count: usize,
    pub(crate) body_table: usize,
    pub(crate) body_top: usize,
    pub(crate) native_table: usize,
}

/// Whether emitted code may push native headers (one-frame-one-call.md F5): on by default, so a
/// u-jit direct call's `admit_stacks` finds room under `plain_depth_limit` instead of always
/// missing to the helper row. `QUENCH_DEV=no-native-frames` keeps the bound 0 (every call takes
/// the helper row) to compare against the old path.
fn native_frames_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !quench_runtime::trace::Dev::NoNativeFrames.on())
}

#[quench_macros::ext(LaneStackOps1)]
impl LaneStack {
    /// The activation records, every frame among them (call protocol v2 stage C,
    /// `/mnt/project-files/calls-frames/call-protocol-v2.md`).
    #[inline(always)]
    pub(crate) fn activations(&self) -> &LaneActivations {
        if self.has_plain_frames() { self.spill_plain(); }
        // SAFETY: only `spill_plain` writes through a shared reference, and never while a
        // reference it returned lives: plain activations are pushed through `&mut self` only.
        unsafe { &*self.activation_records.get() }
    }
    /// `activations` for writing.
    #[inline(always)]
    pub(crate) fn activations_mut(&mut self) -> &mut LaneActivations {
        if self.has_plain_frames() { self.spill_plain(); }
        // The records may change, or the values move (`reserve_values`): the proved calls'
        // bound (`PlainFrames::bound`) names neither any more.
        self.plain.bound.set(0);
        self.activation_records.get_mut()
    }

    /// Whether any frame is a plain header, innermost or sealed below a record.
    #[inline(always)]
    pub(crate) fn has_plain_frames(&self) -> bool { self.plain.depth.get() != 0 || self.sealed.get() != 0 }

    /// The innermost activation's function, read from its header where it is plain, so the
    /// read makes no record.
    #[inline(always)]
    pub(crate) fn innermost_function(&self) -> Option<u32> {
        if self.plain.depth.get() != 0 {
            return Some(Registers(self.plain.registers.get()).plain_callee().0);
        }
        // SAFETY: no plain activation remains, so the records are every activation.
        unsafe { &*self.activation_records.get() }.last().map(|activation| activation.function())
    }

    /// The address of the plain activations' words (`PlainFrames`).
    #[inline(always)]
    pub(crate) fn plain_frames_word(&self) -> usize {
        self.frames_changed();
        (&raw const self.plain).expose_provenance()
    }

    /// Records that Rust changed the frames: the emitted native push's bound is void until the next
    /// binding (`Vm::stack_words_in_lane`), so emitted calls take the helper row meanwhile.
    #[inline(always)]
    pub(crate) fn frames_changed(&self) { self.plain_depth_limit.set(0); }

    /// Make every frame a record, outermost first, as their pushes would have: the plain
    /// headers, innermost and sealed alike, each the record the frame view reads it as
    /// (`lane_frames::plain_frame`, `ActivationView::record`), side record included. Only emitted code's entry
    /// (`Vm::stack_words_in_lane`, step F5) and a deferred native's callback (whose trampoline
    /// header must be the only one) still need it: every other reader walks the frames where
    /// they are (one-frame-one-call.md F4 (3)).
    #[cold]
    #[inline(never)]
    fn spill_plain(&self) {
        // The records change: the proved calls' bound no longer holds.
        self.plain.bound.set(0);
        // Innermost first, the frames above the records floor alone (`frames_to_spill`); each
        // record keeps its side record where its completion carries a payload (`record_sided`),
        // which a header's carries exactly then.
        // Filled in place and read by reference: a spill moves a few frames, and a frame view is
        // large, so neither a returned nor a consumed vector is copied whole.
        let mut frames = frame_walk::SpilledFrames::new();
        let (floor, sides_below) = self.frames_to_spill(&mut frames);
        // SAFETY: the walk above ended; no reference into the records or side records lives.
        let (records, sides) = unsafe { (&mut *self.activation_records.get(), &mut *self.sides.get()) };
        debug_assert!(floor + frames.len() <= records.capacity(), "the activation limit counts plain activations");
        sides.truncate(sides_below);
        // SAFETY: records hold no drop glue; the activation limit keeps every frame within the
        // capacity, so each push has its slot.
        unsafe { records.set_len(floor) };
        for frame in frames.iter().rev() {
            let activation = frame.record();
            if Self::record_sided(&activation) { sides.push(Self::record_side(&activation, frame.payload())); }
            // SAFETY: as above.
            unsafe { records.push_unchecked(activation) };
        }
        self.frames_changed();
        self.plain.depth.set(0);
        // SAFETY: as above; `spill_plain` alone writes the seals through a shared reference.
        unsafe { self.clear_segments() };
    }

    /// Set `PlainFrames::bound` for plain activations pushed over the records from `floor`, the
    /// address the first one's window starts at (the values' top before its fill): `min(values
    /// end, floor + (limit - records - 1) * CONTEXT words)`, or 0 where the limit admits none.
    /// Only at plain depth 0, where `floor` is below every plain window to come; above it the
    /// bound is cleared, so the exact test decides until the depth is 0 again.
    #[inline(always)]
    fn refresh_plain_bound(&self, floor: usize) {
        let records = self.frames_below_plain();
        let allowed = self.activation_limit.get().saturating_sub(records);
        let bound = if allowed == 0 || self.plain.depth.get() != 0 {
            0
        } else {
            let word = std::mem::size_of::<Value>();
            let end = self.values.top().addr().wrapping_add(self.values.room() * word);
            floor.saturating_add((allowed - 1).saturating_mul(lane_values::CONTEXT * word)).min(end)
        };
        self.plain.bound.set(bound);
    }

    /// Whether a proved plain call (`lane_plain_activation!`) may push one more plain activation
    /// over `depth`, the plain depth it loaded, with `room` values for its window: the activation
    /// limit (`activation_count() < activation_limit`) and the values' room
    /// (`activation_room_in_lane`), the same answer. Both are one compare of the window's end
    /// against `PlainFrames::bound`, which never admits past either; where it refuses, the
    /// exact test decides (`plain_room_exact`). It makes no call on the fast path.
    #[inline(always)]
    fn plain_room(&self, depth: usize, room: u16) -> bool {
        let end = self.values.top().addr().wrapping_add(usize::from(room) * std::mem::size_of::<Value>());
        end <= self.plain.bound.get() || self.plain_room_exact(depth, room)
    }

    /// `plain_room` past its one compare: the records' count against the limit and the values'
    /// room, exactly as `activation_room_in_lane` tests them, so a recursion's `RangeError` comes at
    /// the same depth. At depth 0 it refreshes the bound for the pushes above.
    #[cold]
    #[inline(never)]
    fn plain_room_exact(&self, depth: usize, room: u16) -> bool {
        let records = self.frames_below_plain();
        if depth == 0 { self.refresh_plain_bound(self.values.top().addr()); }
        records + depth < self.activation_limit.get()
            && quench_runtime::staged::call::concrete_fits(self.values.room() as u64, u64::from(room))
    }

    /// The side-record flags of `activation` pushed as a header (one-frame-one-call.md F4,
    /// `lane_frames::Side::flags_of`): any script activation of the lane's own windows with an
    /// empty native prefix that promotes no record. `None` for every other (a native frame's,
    /// Wasm, a window over a heap register file), which stays a record.
    #[inline(always)]
    fn header_flags(activation: &Activation, window: *mut Value) -> Option<u64> {
        let ActivationKind::Script(script) = activation.kind() else { return None };
        if script.record.is_null()
            && activation.callee.is_null()
            && activation.body == 0
            && activation.this == 0
            && window.addr() & lane_values::HEAP_WINDOW == 0
        {
            Some(lane_frames::Side::flags_of(script))
        } else {
            None
        }
    }

    /// Push a native's callback of `header` over the window at `window` as a plain activation
    /// whose header names the return trampoline (`CallbackTrampoline`): the trampoline's words
    /// take the callback's own call record and caller registers, which its return and a spill
    /// read. Every native callback is pushed over no plain activation (its native observed the
    /// records, `Vm::native_callback_in_lane`), so its header is the outermost plain one and the
    /// trampoline's words are its alone.
    ///
    /// # Safety
    /// The caller admitted the activation (`concrete_admit_callback`), whose window starts at
    /// `window`, the values' top.
    #[inline(always)]
    unsafe fn push_callback_header(&mut self, header: &CallbackHeader, window: *mut Value) {
        debug_assert_eq!(self.plain.depth.get(), 0, "a native's callback runs over records alone");
        // The first plain activation over the records names their count to the proved calls
        // above it (`plain_room`), from its window up.
        self.refresh_plain_bound(window.addr());
        self.callback_return.call.set(header.call);
        self.callback_return.caller_registers.set(header.caller_registers);
        // At depth 0, which the push need not load.
        quench_runtime::staged::protocol::push_plain_at(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
            self.plain_frames_word() as u64, window.expose_provenance() as u64, header.plain, header.program, 0,
        );
    }

    /// Push an in-lane array step's callback of `header` over the window at `window` as a plain
    /// activation (`array_steps::push_array_step_in_lane`): its header names the array steps'
    /// trampoline record (`CallbackTrampoline::step_call`) and, as its caller base word, its
    /// step's index; the step's entry holds its own call record, caller registers and base. A
    /// step nests over plain activations, so unlike `push_callback_header` it may push at any
    /// depth and writes no trampoline word.
    ///
    /// # Safety
    /// The caller admitted the activation (`admits_callback`), whose window starts at
    /// `window`, the values' top, and pushed its step's entry.
    #[inline(always)]
    unsafe fn push_step_header(&mut self, header: &CallbackHeader, window: *mut Value) {
        let depth = self.plain.depth.get();
        // A first plain activation over the records names their count to the proved calls
        // above it (`plain_room`), as `push_callback_header` does; above one, the bound already
        // counts this window, which starts at the values' top and takes at least `CONTEXT` words.
        if depth == 0 { self.refresh_plain_bound(window.addr()); }
        quench_runtime::staged::protocol::push_plain_at(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
            self.plain_frames_word() as u64, window.expose_provenance() as u64, header.plain, header.program, depth as u64,
        );
    }
}




#[quench_macros::ext(LaneStackOps2)]
impl LaneStack {
    /// Records a target of the running program's callee (`script_targets`), the cache
    /// growing up to what `policy` allows.
    fn remember_script_target(&mut self, target: ScriptCallTarget, policy: quench_runtime::MemoryPolicy) {
        self.script_targets.remember(target, script_call_targets(policy));
    }

    /// `remember_script_target` of another program's callee (`cross_targets`).
    fn remember_cross_target(&mut self, target: ScriptCallTarget, policy: quench_runtime::MemoryPolicy) {
        self.cross_targets.remember(target, script_call_targets(policy));
    }

    /// The target `function_targets` holds for a closure of `function` of `program` in this
    /// generation, or `cross_function_targets` for calls from another program, whose closure
    /// fields the caller rebinds.
    fn function_target(&self, program: ProgramId, function: u32, cross: bool) -> Option<ScriptCallTarget> {
        let targets = if cross { &self.cross_function_targets } else { &self.function_targets };
        let known = targets[function as usize & (FUNCTION_TARGETS - 1)];
        // A target made under other diagnostics (`Admission::DIAGNOSED`) is admitted by no link
        // test (`Admission::admits`): resolving it again would loop, so it is made anew.
        (known.generation() == self.generation && known.program() == program && known.function == function
            && (known.admission.0 ^ self.admission.0) & Admission::DIAGNOSED == 0)
            .then_some(known)
    }

    /// Record `target` in its set and as its function's (`function_targets`), or a `cross`
    /// one in the other programs' tables.
    fn remember_function_target(&mut self, target: ScriptCallTarget, cross: bool, policy: quench_runtime::MemoryPolicy) {
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
    fn forwarded_method_index(&mut self, method: quench_runtime::bytecode::ForwardedThisMethod) -> u16 {
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
    unsafe fn push_activation(&mut self, mut activation: Activation, window: *mut Value) {
        // Every script activation of the lane's own windows writes its header below its window
        // instead of a record (one-frame-one-call.md F4), and what a record would hold past the
        // header (keeping, environment, a completion past `Plain`, a caller of another program)
        // goes to one side record its header's flags name (`lane_frames::Side`). The header
        // names its caller's program (`PlainFrames::program`), which a side record keeps too.
        if let Some(flags) = Self::header_flags(&activation, window) {
            debug_assert!(self.plain_program_holds(activation.caller_program()));
            // The first plain activation over the records names their count to the proved
            // calls above it (`plain_room`).
            // Its window is filled already, so the bound counts from the window, not the top.
            // Every header's window is the lane stack's, at least `CONTEXT` words, as the
            // bound assumes.
            if self.plain.depth.get() == 0 { self.refresh_plain_bound(window.addr()); }
            let ActivationKind::Script(script) = activation.kind() else { unreachable!() };
            use quench_runtime::staged::protocol::{push_plain, PlainHeader};
            let header = PlainHeader::new(
                activation.call().expose_provenance() as u64,
                activation.caller_registers().0.expose_provenance() as u64,
                activation.caller_base().expose_provenance() as u64,
                activation.function(), script.argc, flags, script.callee.to_bits(),
            );
            let side = lane_frames::Side::of(script, activation.caller_program(), flags);
            push_plain(
                &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
                self.plain_frames_word() as u64, window.expose_provenance() as u64, header, u64::from(activation.caller_program().raw()),
            );
            if let Some(side) = side { self.sides.get_mut().push(side); }
            return;
        }
        activation.window = window.addr();
        let frame_storage_bytes = activation.frame_storage_bytes();
        // A record over plain headers seals them where they are (one-frame-one-call.md F4 (3)):
        // no spill.
        self.seal_plain();
        let records = self.records_mut();
        debug_assert!(records.len() < records.capacity(), "lane activations stay within capacity");
        // SAFETY: the slot at `len` lies within capacity and is uninitialized; the top
        // covers it only once written.
        unsafe { records.push_unchecked(activation); }
        self.wasm_storage_bytes += frame_storage_bytes;
    }

    /// An emitted call's native header that Rust observed pops as the activation Rust keeps: the record a
    /// nested entry made of it (`spill_plain`), or the linked plain header (`native_frames::link_header`)
    /// with its side record.
    #[cold]
    pub(crate) fn pop_observed_header(&mut self) {
        let _ = self.pop_activation();
    }

    #[inline(always)]
    fn pop_activation(&mut self) -> Option<ActivationView> {
        // The innermost plain activation pops its header and side record where they are, with
        // no spill of the ones below and no record built (`lane_frames::pop_plain_frame`); it
        // holds no Wasm storage.
        if self.plain.depth.get() != 0 {
            // SAFETY: a plain activation runs, tested above.
            let returned = unsafe { self.pop_plain_frame() };
            self.drop_owned_from(self.activation_count());
            return Some(returned);
        }
        let activation = self.records_mut().pop()?;
        self.wasm_storage_bytes -= activation.frame_storage_bytes();
        let payload = self.pop_record_side(&activation);
        self.unseal_plain();
        self.drop_owned_from(self.activation_count());
        Some(ActivationView::of_record(&activation, payload))
    }


    /// Drops what the activations from index `depth` on owned, which completed: their loops'
    /// iterators, their `with` objects and eval records (a trap's check pops with its side
    /// record).
    #[inline(always)]
    fn drop_owned_from(&mut self, depth: usize) {
        if !self.iterators.is_empty() || !self.with_objects.is_empty() || !self.records.is_empty() {
            self.drop_owned_held_from(depth);
        }
    }

    #[cold]
    #[inline(never)]
    fn drop_owned_held_from(&mut self, depth: usize) {
        drop_owned_entries(&mut self.iterators, depth);
        drop_owned_entries(&mut self.with_objects, depth);
        drop_owned_entries(&mut self.records, depth);
    }

    /// Derive `activation_limit` from the activations' capacity, the guest calls the
    /// thread admits beyond the frames and activations already counted, and a due
    /// collection.
    #[inline(always)] fn refresh_activation_limit(&self) {
        let limit = if floor_in_lane().load(std::sync::atomic::Ordering::Relaxed) == COLLECTION_DUE_FLOOR {
            0
        } else {
            let admitted = self.activation_count() + quench_runtime::stack::GuestCallGuard::remaining();
            // SAFETY: reads the capacity alone.
            unsafe { (*self.activation_records.get()).capacity() }.min(admitted)
        };
        self.activation_limit.set(limit);
        // Above depth 0 this clears the bound (`refresh_plain_bound`).
        self.refresh_plain_bound(self.values.top().addr());
    }

    /// The one depth check of a push: whether `more` activations fit under `activation_limit`
    /// (capacity, the guest-call budget and a due collection, `refresh_activation_limit`).
    #[inline(always)]
    fn admits_activations(&self, more: usize) -> bool {
        self.activation_count() + more <= self.activation_limit.get()
    }

    /// The activations still admitted past `pending` more (`admits_activations`).
    #[inline(always)]
    fn activations_left(&self, pending: usize) -> usize {
        self.activation_limit.get().saturating_sub(self.activation_count() + pending)
    }

    /// Records `target` as a callee of the call record whose link is `link`, in the order
    /// first seen. The link names the latest callee, so a callee already recorded changes
    /// nothing: a polymorphic site's steady state only reads its record.
    fn note_site_callee(&self, link: &std::cell::Cell<u16>, target: crate::compiled::CallTarget) {
        let address = std::ptr::from_ref(link).addr();
        let noted = &self.noted[noted_slot(address, target.function)];
        if noted.get() == Some((address, target)) {
            return;
        }
        let mut sites = self.call_sites.borrow_mut();
        let site = sites.entry(address).or_default();
        if !site.contains(&target) {
            if site.len() == crate::compiled::tier_policy::POLYMORPHISM {
                site.remove(0);
                self.forget_noted();
            }
            site.push(target);
        }
        noted.set(Some((address, target)));
    }

    /// Notes whether the running lane run calls script functions (`LaneContext::script_calls`):
    /// while it does not, every cached call misses (`Admission::NO_SCRIPT_CALLS`).
    #[inline(always)]
    pub(crate) fn set_script_calls(&mut self, allowed: bool) {
        self.set_condition(Admission::NO_SCRIPT_CALLS, !allowed);
    }

    /// Whether the lane's admission admits script calls (`set_script_calls`).
    #[inline(always)]
    pub(crate) fn script_calls_allowed(&self) -> bool {
        self.refused & Admission::NO_SCRIPT_CALLS == 0
    }

    /// Whether the lane admits activating a script callee of `program` with no link to prove
    /// it (a getter, a native's callback, a body's call): script calls allowed, no construction
    /// pending and `program` the active one, in one compare of the admission word, the same
    /// word every link's admission is compared with.
    #[inline(always)]
    pub(crate) fn admits_program(&self, program: ProgramId) -> bool {
        self.admission.0 >> Admission::PROGRAM_SHIFT == program.raw() as u64
    }

    /// Whether any condition refuses every unlinked activation (`Admission::CONDITIONS`).
    #[inline(always)]
    fn refuses_any(&self) -> bool {
        self.refused & Admission::CONDITIONS != 0
    }

    /// Notes whether the tier's diagnostics count every entry (`Admission::DIAGNOSED`), which
    /// every lane run's start mirrors (`Vm::drive_lane`): while they do, no proved plain call
    /// runs. Changes nothing where it is already so.
    #[inline(always)]
    pub(crate) fn set_diagnosed(&mut self, diagnosed: bool) {
        if (self.refused & Admission::DIAGNOSED != 0) != diagnosed {
            self.set_condition(Admission::DIAGNOSED, diagnosed);
        }
    }

    /// Takes the proof (`Admission::UNPROVEN`) from every script call entry of `view`, whose
    /// entry cell just published a body: a proved call of it would skip the body.
    pub(crate) fn unprove_view_entries(&mut self, view: *const LaneView) {
        for entry in self.script_targets.entries.iter_mut().filter(|entry| std::ptr::eq(entry.view, view)) {
            entry.admission.0 |= Admission::UNPROVEN;
        }
    }

    /// Whether cached calls are refused (`set_refused`).
    #[inline(always)]
    fn refuses(&self) -> bool {
        self.refused & Admission::REFUSED != 0
    }

    /// The admission a script call target of `program` made now holds: this generation's, with
    /// the lane's diagnostics (`Admission::DIAGNOSED`), which every link test compares whole
    /// (`Admission::admits`); without them no link would hold under `QUENCH_TRACE=js-jit`.
    #[inline(always)]
    fn target_admission(&self, program: ProgramId) -> Admission {
        Admission(Admission::new(self.generation, program).0 | self.refused & Admission::DIAGNOSED)
    }

    /// The admission a target or memo made now holds: the lane's, without its conditions.
    #[inline(always)]
    fn current_admission(&self) -> Admission {
        Admission(self.admission.0 & !Admission::CONDITIONS)
    }
}

pub(crate) use quench_runtime::vm::engine_types::DeferredLaneCall;

use quench_runtime::vm::engine_types::NativeBoundary;

pub(crate) use quench_runtime::vm::engine_types::LaneNativeCall;

use quench_runtime::vm::engine_types::LaneBinding;

/// Drops the storage pointers a lane context resolved before a collection, as
/// `after_nested_collection_in_lane` does after an in-place operation collected: the run's
/// capture storage (whose `CAPTURE_STORES` tag let stores skip the barrier until the next
/// collection), its last outer capture walk, a frame's captured-locals storage and a Wasm
/// instance's slots. Each is found again by the path that found it first. The context's
/// stack is retired by the collector itself (`LaneStack::retire_after_collection`).
unsafe fn retire_lane_binding(context: *mut ()) {
    let cx = context.cast::<LaneContext>();
    // SAFETY: the prefix's callback was installed for this context's Host; the context is
    // live while it is the thread's running or a suspended outer one.
    unsafe {
        (*cx).registers.set_capture(std::ptr::null());
        (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
        (*cx).frame_storage = NO_FRAME_STORAGE;
        (*cx).instance_slots = (Value::DELETED, std::ptr::null());
    }
}

/// Retires the thread's running lane context after a collection ran under it: a native call
/// out of the lane (`gc()`, a host callback) or an allocation that made one due. The run
/// continues past the call with nothing it resolved under the epoch before.
pub(crate) fn retire_running_lane_context() {
    let cx = tls_lane_context();
    if cx.is_null() {
        return;
    }
    // SAFETY: every active context has the repr(C) `LaneBinding` prefix, whose callback
    // owns its actual Host type.
    unsafe { (cx.cast::<LaneBinding>().as_ref().unwrap_unchecked().retire)(cx) };
}

unsafe fn resume_lane_binding(context: *mut ()) {
    let cx = context.cast::<LaneContext>();
    // SAFETY: the prefix's callback was installed for this context's Host.
    let base = unsafe { vm(cx) }.heap_reads().object_record_base().unwrap_or(std::ptr::null());
    unsafe { (*cx).binding.object_base = base };
}

use quench_runtime::vm::engine_types::LaneContext;

use quench_runtime::vm::engine_types::LeafCall;

const _: () = {
    use quench_runtime::staged::call_leaf::{LEAF_ARGC_AT, LEAF_CALLER_BASE_AT, LEAF_CALLER_REGISTERS_AT, LEAF_CALL_AT, LEAF_FORM_AT, LEAF_TARGET_AT};
    assert!(std::mem::offset_of!(LeafCall, call) == LEAF_CALL_AT as usize);
    assert!(std::mem::offset_of!(LeafCall, caller_registers) == LEAF_CALLER_REGISTERS_AT as usize);
    assert!(std::mem::offset_of!(LeafCall, caller_base) == LEAF_CALLER_BASE_AT as usize);
    assert!(std::mem::offset_of!(LeafCall, target) == LEAF_TARGET_AT as usize);
    assert!(std::mem::offset_of!(LeafCall, argc) == LEAF_ARGC_AT as usize);
    assert!(std::mem::offset_of!(LeafCall, form) == LEAF_FORM_AT as usize);
};

const NO_LEAF_CALL: LeafCall = LeafCall {
    call: std::ptr::null(),
    form: NOT_LEAF,
    argc: 0,
    caller_registers: Registers(std::ptr::null_mut()),
    caller_base: std::ptr::null(),
    target: std::ptr::null(),
    caller_program: None,
};

use quench_runtime::vm::engine_types::FrameStorage;

const NO_FRAME_STORAGE: FrameStorage =
    FrameStorage { frame: usize::MAX, env: Value::DELETED, slots: std::ptr::null_mut() };

/// The storage of the running frame's captured environment (`LaneContext::frame_storage`),
/// for a frame run of a function other than the root whose frame holds one; `None` leaves
/// the access to the heap.
#[inline(always)]
fn frame_storage_in_lane(cx: *mut LaneContext) -> Option<&'static mut quench_runtime::tier_surface::EnvironmentSlots> {
    let vm = unsafe { vm(cx) };
    let index = unsafe { (*cx).frame() };
    let frame = &vm.frames[index];
    if !frame.captured || frame.function == quench_runtime::vm::ROOT_FUNCTION_ID {
        return None;
    }
    let known = unsafe { (*cx).frame_storage };
    let slots = if known.frame == index && known.env == frame.env {
        known.slots
    } else {
        find_frame_storage_in_lane(cx, index, frame.env)
    };
    // SAFETY: as `FrameStorage`; the lane holds no other reference to the storage.
    unsafe { slots.as_mut() }
}

/// `frame_storage_in_lane` past its cache: the environment's storage, recorded.
#[cold]
#[inline(never)]
fn find_frame_storage_in_lane(
    cx: *mut LaneContext,
    frame: usize,
    env: Value,
) -> *mut quench_runtime::tier_surface::EnvironmentSlots {
    let vm = unsafe { vm(cx) };
    let slots = match vm.environment_mut(env) {
        Some(quench_runtime::tier_surface::EnvironmentRecord { slots, .. }) => std::ptr::from_mut(slots),
        _ => std::ptr::null_mut(),
    };
    unsafe { (*cx).frame_storage = FrameStorage { frame, env, slots } };
    slots
}

use quench_runtime::vm::engine_types::OuterWalk;

const NO_OUTER_WALK: OuterWalk = OuterWalk { env: Value::DELETED, depth: 0, slots: std::ptr::null() };

/// The tag of a `LaneContext::capture` whose every dense slot a store may write directly:
/// its environment admits every direct store (`Vm::admits_every_direct_store`) and is in the
/// remembered set already, so a write needs no barrier until the next collection, which
/// retires the target that tagged it.
pub(crate) const CAPTURE_STORES: usize = 1;
const _: () = assert!(std::mem::align_of::<usize>() > CAPTURE_STORES);

/// The slots a `LaneContext::capture` reads, its tag cleared.
#[inline(always)]
pub(crate) fn capture_reads(capture: *const usize) -> *const usize {
    capture.map_addr(|address| address & !CAPTURE_STORES)
}

/// The local slots (address and count) of the VM frame the lane context `cx` runs, as the lane's handlers read
/// them (`lane_frame::staged_load_local_plain`); none where there is no context (a test's entry).
///
/// # Safety
/// `cx` is null or a live `LaneContext`.
unsafe fn frame_locals(cx: *mut ()) -> (*mut Value, usize) {
    std::ptr::NonNull::new(cx.cast::<LaneContext>()).map_or((std::ptr::null_mut(), 0), |cx| {
        // SAFETY: the caller's contract; the VM and its frame outlive the lane run.
        let (frames, root) = unsafe { (&mut vm(cx.as_ptr()).frames, (*cx.as_ptr()).root) };
        // A job's activations run in no frame (`LaneRoot::Job`).
        root.frame().map_or((std::ptr::null_mut(), 0), |frame| (frames[frame].locals.as_mut_ptr(), frames[frame].locals.len()))
    })
}

/// The slots a `LaneContext::capture` tagged `CAPTURE_STORES` lets a store write.
#[inline(always)]
pub(crate) fn capture_stores(capture: *const usize) -> Option<*const usize> {
    (capture.addr() & CAPTURE_STORES != 0).then(|| capture_reads(capture))
}

/// The dense slot view of the environment a `LaneContext::capture` names (`None` where it has
/// none), and whether that capture admits direct stores (`capture_stores`).
///
/// # Safety
/// `capture` is null or names a live environment's storage, which the caller keeps live.
#[inline(always)]
pub(crate) unsafe fn capture_view(capture: *const usize) -> (Option<(*const Value, usize)>, bool) {
    // SAFETY: as the caller promises.
    (unsafe { quench_runtime::tier_surface::EnvironmentSlots::native_storage_view(capture_reads(capture)) }, capture_stores(capture).is_some())
}

/// A lane handler returns the next record for the lane loop, tagged with
/// [`EXIT_TAG`] when the general path must execute it. The carried state is
/// passed as scalars so every argument stays in a register; the context,
/// which hot handlers do not touch, is reached through [`LANE_CONTEXT`].
///
/// Handlers use the `rust-preserve-none` convention: every register is the caller's to save,
/// and a handler's caller is only the next handler's tail jump or the lane's entry, so the hot
/// handlers save none. Under the default convention six arguments left three scratch
/// registers, and a script call saved and restored six callee-saved ones on every record
/// (V8-v7 no-JIT, static production build, pinned, 3 interleaved runs, x86_64 4 vCPU,
/// 2026-11-01: Score DeltaBlue 1936 -> 2097, NavierStokes 4638 -> 4963, RayTrace 5083 -> 5348,
/// Richards within spread).
type Handler = extern "rust-preserve-none" fn(Registers, Ip, *mut u8, Slot<isize>, quench_runtime::value::IntegerEncoding, Slot<Value>) -> Ip;

/// A plain `#[thread_local]` cell. `thread_local!` in this position-independent rlib reads
/// through a `__tls_get_addr` call that only the linker relaxes, so LLVM gave every handler
/// that reads the lane context a frame and spills; the executable's local-exec read below
/// is one load.
struct TlsCell<T: Copy>(std::cell::Cell<T>);
impl<T: Copy> TlsCell<T> {
    #[inline(always)]
    fn with<R>(&self, f: impl FnOnce(&std::cell::Cell<T>) -> R) -> R { f(&self.0) }
}
/// The context of this thread's running lane. A lane run never calls out, so runs nest
/// only through the general path, and each run restores the outer context when it returns.
#[thread_local]
static LANE_CONTEXT: TlsCell<*mut ()> = TlsCell(std::cell::Cell::new(std::ptr::null_mut()));
/// The running lane's stack floor (`ExecutionInterrupt::floor_in_lane` of its VM): a taken
/// safepoint jump below it returns to the lane loop. The maximum address marks a due
/// collection or a termination request.
#[thread_local]
static LANE_FLOOR: TlsCell<*const std::sync::atomic::AtomicUsize> = TlsCell(std::cell::Cell::new(std::ptr::null()));

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline(always)]
fn tls_lane_context() -> *mut () {
    let value: *mut ();
    // SAFETY: the executable's own TLS block; local-exec offset fixed at link time.
    unsafe { std::arch::asm!("mov {}, qword ptr fs:[{}@TPOFF]", out(reg) value, sym LANE_CONTEXT, options(pure, nostack, readonly, preserves_flags)) };
    value
}
#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
#[inline(always)]
fn tls_lane_context() -> *mut () {
    LANE_CONTEXT.0.get()
}
/// The running lane's fuel (`LaneContext::fuel`), for the handlers that do not carry the
/// context and for `note_allocation`; null while no lane runs.
#[thread_local]
static LANE_FUEL: TlsCell<*mut u32> = TlsCell(std::cell::Cell::new(std::ptr::null_mut()));
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline(always)]
fn tls_lane_fuel() -> *mut u32 {
    let value: *mut u32;
    // SAFETY: as `tls_lane_context`.
    unsafe { std::arch::asm!("mov {}, qword ptr fs:[{}@TPOFF]", out(reg) value, sym LANE_FUEL, options(pure, nostack, readonly, preserves_flags)) };
    value
}
#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
#[inline(always)]
fn tls_lane_fuel() -> *mut u32 {
    LANE_FUEL.0.get()
}

/// Backward jumps a lane run takes between two safepoint visits: the bound on how long a
/// termination request from another thread waits (it raises the floor, which the visit reads).
const LANE_FUEL_BATCH: u32 = 4096;

/// Spend one unit of the running lane's fuel; true when it was out, so the backward jump
/// takes `safepoint_at`. One decrement of a context field and one branch: the safepoint
/// test of every backward jump, in place of the stack-floor compare.
#[inline(always)]
fn script_fuel_out(cx: *mut LaneContext) -> bool {
    // SAFETY: the run's context, which only this thread touches.
    unsafe {
        let (left, out) = (*cx).fuel.overflowing_sub(1);
        (*cx).fuel = left;
        out
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline(always)]
fn tls_lane_floor() -> *const std::sync::atomic::AtomicUsize {
    let value: *const std::sync::atomic::AtomicUsize;
    // SAFETY: as `tls_lane_context`.
    unsafe { std::arch::asm!("mov {}, qword ptr fs:[{}@TPOFF]", out(reg) value, sym LANE_FLOOR, options(pure, nostack, readonly, preserves_flags)) };
    value
}
#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
#[inline(always)]
fn tls_lane_floor() -> *const std::sync::atomic::AtomicUsize {
    LANE_FLOOR.0.get()
}

/// A lane run's raised floor (`Vm::enter_lane_floor`): the VM's floor cell, and the cell and
/// floor of the run it nests in, which leaving restores.
struct LaneFloor {
    cell: *const std::sync::atomic::AtomicUsize,
    outer_cell: *const std::sync::atomic::AtomicUsize,
    outer_floor: usize,
}

/// The running lane's floor.
#[inline(always)]
fn floor_in_lane() -> &'static std::sync::atomic::AtomicUsize {
    // SAFETY: set for the whole of a lane run to its VM's interrupt state, which the VM
    // holds; only read while a lane runs.
    unsafe { &*tls_lane_floor() }
}

/// Stack a lane run may use below its entry before a safepoint jump returns
/// to the lane loop; only reachable should a build not form sibling calls.
const LANE_STACK_SLACK: usize = 64 * 1024;

/// The floor that sends the next safepoint jump through the slow path.
const COLLECTION_DUE_FLOOR: usize = quench_runtime::execution_interrupt::LANE_FLOOR_RAISED;

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

/// `below_floor` of a JavaScript record, whose context register carries the run's floor
/// (`LaneContext::floor`) without the thread-local load.
#[inline(always)]
fn script_below_floor(cx: *mut LaneContext) -> bool {
    // SAFETY: the run's floor cell, which its VM holds for the whole run.
    stack_pointer() < unsafe { &*(*cx).floor }.load(std::sync::atomic::Ordering::Relaxed)
}

/// After a lane allocation, a due collection raises the floor of the running lane, where
/// one runs: a linked region's general frame polls the heap's own counters instead.
#[inline(always)]
pub(crate) fn note_allocation(vm: &Vm) {
    if vm.collection_due() {
        let floor = tls_lane_floor();
        // SAFETY: as `lane_floor`, while a lane runs; null otherwise.
        if let Some(floor) = unsafe { floor.as_ref() } {
            floor.store(COLLECTION_DUE_FLOOR, std::sync::atomic::Ordering::Relaxed);
        }
        // SAFETY: as `fuel_out`, while a lane runs; null otherwise.
        if let Some(fuel) = unsafe { tls_lane_fuel().as_mut() } {
            *fuel = 0;
        }
        vm.lane_stack.activation_limit.set(0);
        // The proved calls' bound on the plain depth (`PlainFrames::bound`) follows the limit.
        vm.lane_stack.plain.bound.set(0);
    }
}

/// The running activation's `this`; `None` where the general path decides it (a
/// derived constructor before `super()`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn this_in_lane(cx: *mut LaneContext, r: Registers) -> Option<Value> {
    let this = r.this();
    if !this.is_deleted() {
        return Some(this);
    }
    unsafe { vm(cx).current_this((*cx).frame()) }
}

/// The running lane's context.
#[inline(always)]
fn context_in_lane() -> *mut LaneContext {
    tls_lane_context().cast()
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

/// The record a handler's result names, and whether the result leaves the lane there (`exit`).
#[inline(always)]
fn exited(ip: Ip) -> (Ip, bool) {
    (ip.map_addr(|address| address & !EXIT_TAG), ip.addr() & EXIT_TAG != 0)
}

/// SAFETY: the context outlives the lane run and no other reference to the
/// VM is live while a handler runs.
#[inline(always)]
unsafe fn vm<'a>(cx: *mut LaneContext) -> &'a mut Vm {
    unsafe { &mut *(*cx).vm }
}

/// Sample the identity and body a script call site targets. Diagnostic builds use this to
/// determine whether a per-site target memo is worth implementing; production builds compile
/// every call to this helper away.
#[cfg(feature = "profile-lane-exits")]
#[inline(always)]
fn profile_script_call_target(
    cx: *mut LaneContext,
    ip: Ip,
    callee: Value,
    target: &ScriptCallTarget,
) {
    let vm = unsafe { vm(cx) };
    let caller = vm
        .lane_stack
        .top_frame()
        .map_or_else(|| vm.frames[unsafe { (*cx).frame() }].function, |activation| {
            activation.function()
        });
    let pc = unsafe { ip.offset_from((*cx).base) } as u32;
    quench_runtime::vm::lane_exits::call_target(
        unsafe { (*cx).program.raw() },
        caller,
        pc,
        callee,
        target.function,
    );
}

/// The handler of the record at `ip`.
#[inline(always)]
fn handler(ip: Ip) -> Handler {
    #[cfg(feature = "profile-lane-exits")]
    quench_runtime::vm::lane_exits::count_record(ip.addr(), std::mem::size_of::<LaneInstruction>());
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
        become handler(ip)(
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

/// Where the lane continues once an activation completes (`complete_activation_in_lane`): after
/// the call in its caller, with these registers and view, or out of the lane at a frame's
/// call record that returns the completion (`frame_return`).
enum Resumed {
    At(Registers, Ip, Carried),
    /// The handler's result as it is: an `exit`, or a record the lane dispatches next from its
    /// context's state (a handler a raise entered, `unwound_in_place`).
    Exit(Ip),
}

/// Runs `handler` for the record at `ip` as a sibling call: the calling handler's fast path
/// then keeps no state across a slow path that needs more registers.
#[cfg(quench_lane_tail_calls)]
macro_rules! sibling {
    ($handler:expr, $r:expr, $ip:expr, $view:expr) => {{
        let view: Carried = $view;
        become $handler(
            $r,
            $ip,
            view.memory.bytes,
            view.memory.reach,
            view.integers,
            view.acc,
        );
    }};
}

#[cfg(not(quench_lane_tail_calls))]
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

impl Resumed {
    /// The record the lane loop continues at for this resumption, which it dispatches with
    /// the context's registers and view, as a build without sibling calls continues every
    /// record (`next!`): for a helper whose own result is the lane loop's.
    fn into_loop(self) -> Ip {
        match self {
            Resumed::At(_, next, _) | Resumed::Exit(next) => next,
        }
    }
}

/// Continue as `complete_activation_in_lane` resumed.
macro_rules! resume {
    ($cx:expr, $resumed:expr) => {{
        match $resumed {
            Resumed::At(registers, next, view) => next!($cx, registers, next, view),
            Resumed::Exit(next) => return next,
        }
    }};
}

/// Continue as a comparison jump's `edge` says: the target through `take_jump`, whose backward
/// poll every jump shares, or the record after it.
macro_rules! jump_on_edge {
    ($cx:expr, $r:expr, $ip:expr, $backward:expr, $edge:expr) => {
        jump_on_edge!($cx, $r, $ip, $backward, $edge, Carried::script($cx))
    };
    ($cx:expr, $r:expr, $ip:expr, $backward:expr, $edge:expr, $carried:expr) => {{
        match $edge {
            quench_runtime::staged::Edge::Target => resume!($cx, take_jump($cx, $r, $ip, $backward, $carried, None)),
            quench_runtime::staged::Edge::Following => next!($cx, $r, following($ip), $carried),
            quench_runtime::staged::Edge::Entry(_) => unreachable!("a comparison jump takes no table entry"),
            quench_runtime::staged::Edge::Leave => unreachable!("a comparison jump leaves no lane run"),
            quench_runtime::staged::Edge::Transfer => unreachable!("a comparison jump transfers no control"),
        }
    }};
}

/// Define a handler with the shared signature. `$i` is the record and
/// `$view` the current memory view.
macro_rules! lane_handler {
    ($(#[$meta:meta])* $vis:vis fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        lane_handler!(@define bytes [context_in_lane()] $(#[$meta])* $vis fn $name<$($param: $ty),*>($cx, $r, $ip, $i, $view) $body);
    };
    (@define $bytes:ident [$context:expr] $(#[$meta:meta])* $vis:vis fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        $(#[$meta])*
        #[allow(unused_variables)]
        $vis extern "rust-preserve-none" fn $name<$(const $param: $ty),*>(
            $r: Registers,
            $ip: Ip,
            $bytes: *mut u8,
            reach: Slot<isize>,
            integers: quench_runtime::value::IntegerEncoding,
            acc: Slot<Value>,
        ) -> Ip {
            let $cx: *mut LaneContext = $context;
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
    ($(#[$meta:meta])* $vis:vis fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        lane_handler!(@define bytes [{
            let cx: *mut LaneContext = bytes.cast();
            debug_assert_eq!(cx, context_in_lane(), "a JavaScript view carries its context");
            cx
        }] $(#[$meta])* $vis fn $name<$($param: $ty),*>($cx, $r, $ip, $i, $view) $body);
    };
}

/// `lane_handler!` (or `script_handler!`, `$kind`) for a handler with many const instances: rustc keeps all
/// instances of one generic function in its module's codegen unit, so the body is defined in 12 shard modules
/// (`$mods::s*`) and `$handler::<..>()` names the instance of the shard its parameters pick, as
/// `generated_row_handler` does. Only constant evaluation may call `$handler` (a compiled call would name every
/// shard's instance), so a use names it through its call-site macro (`protocol_row!` and the rest).
macro_rules! sharded_handler {
    (@mods $mods:ident [$($shard:ident)*] $item:tt) => {
        #[allow(non_snake_case)]
        pub(crate) mod $mods { $(pub(crate) mod $shard { #[allow(unused_imports)] use super::super::*; sharded_handler!(@item $item); })* }
    };
    (@item { $($item:tt)* }) => { $($item)* };
    ($kind:ident, $mods:ident, $handler:ident, [$($op:ident),+]; $(#[$meta:meta])* $vis:vis fn $name:ident<$($param:ident: $ty:ty),*>($($arg:ident),*) $body:block) => {
        sharded_handler!(@mods $mods [s0 s1 s2 s3 s4 s5 s6 s7 s8 s9 s10 s11] { $kind! { $(#[$meta])* pub(crate) fn $name<$($param: $ty),*>($($arg),*) $body } });
        $vis const fn $handler<$(const $param: $ty),*>() -> Handler {
            match crate::dispatch_fast::handler_shard(&[$($param as u64),*], crate::dispatch_fast::numeric_row(&[$($op),+])) {
            0 => $mods::s0::$name::<$($param),*>,
            1 => $mods::s1::$name::<$($param),*>,
            2 => $mods::s2::$name::<$($param),*>,
            3 => $mods::s3::$name::<$($param),*>,
            4 => $mods::s4::$name::<$($param),*>,
            5 => $mods::s5::$name::<$($param),*>,
            6 => $mods::s6::$name::<$($param),*>,
            7 => $mods::s7::$name::<$($param),*>,
            8 => $mods::s8::$name::<$($param),*>,
            9 => $mods::s9::$name::<$($param),*>,
            10 => $mods::s10::$name::<$($param),*>,
            _ => $mods::s11::$name::<$($param),*>,
            }
        }
    };
}

/// The shard (`sharded_handler!`) of a handler instance with const parameters `params`: 8 to 11 for a `numeric` row
/// (`numeric_row`), so an edit to the Number step recompiles only those units; any other keeps its shard of 0 to 7.
pub(crate) const fn handler_shard(params: &[u64], numeric: bool) -> usize {
    let mut hash = 0u64;
    let mut index = 0;
    while index < params.len() {
        hash = hash.wrapping_mul(31).wrapping_add(params[index]);
        index += 1;
    }
    if numeric { 8 + (hash % 4) as usize } else { (hash % 8) as usize }
}

/// Whether a row of opcodes `ops` runs the Number step (`staged::row_step`: Number, Arith, or Generic, whose
/// comparisons and keys convert numbers), which places its instance beside the other numeric rows
/// (`handler_shard`, `generated_row_shard`). A chain's absent link (`staged::NO_LINK`) names no opcode.
pub(crate) const fn numeric_row(ops: &[u16]) -> bool {
    use quench_runtime::staged::RowStep;
    let mut index = 0;
    while index < ops.len() {
        if let Some(op) = quench_runtime::bytecode::Op::from_index(ops[index] as usize) {
            if matches!(quench_runtime::staged::row_step(op), RowStep::Number | RowStep::Arith | RowStep::Generic) {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// `lane_handler!` for a return record of any view that has a leaf form
/// (`LaneView::specialize_leaf_returns`): a leaf's records are JavaScript, so its form takes
/// the context as `script_handler!` does, and only the others load it.
macro_rules! return_handler {
    ($(#[$meta:meta])* fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        lane_handler!(@define bytes [if LEAF == NOT_LEAF {
            context_in_lane()
        } else {
            let cx: *mut LaneContext = bytes.cast();
            debug_assert_eq!(cx, context_in_lane(), "a JavaScript view carries its context");
            cx
        }] $(#[$meta])* fn $name<$($param: $ty),*>($cx, $r, $ip, $i, $view) $body);
    };
}

/// Which field of a record reads the accumulator: none, or the field the
/// constant names. The view selects a reading instantiation only where the
/// field's register is the previous record's result register and control
/// reaches the record only by falling through from it.
const ACC_NONE: u8 = 0;
/// `LaneView::accumulator_registers` of a record that reads no accumulator.
const NO_ACCUMULATOR: u16 = u16::MAX;
use quench_runtime::vm::engine_types::ACC_A;
use quench_runtime::vm::engine_types::ACC_B;
use quench_runtime::vm::engine_types::ACC_C;
/// The first register of the immediate's register pair: a select's
/// condition.
const ACC_PAIR: u8 = 4;
/// Accumulator positions a record may read: none, A, B, C and the pair.
const ACC_POSITIONS: usize = 5;



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

lane_handler! {
    /// Not a lane instruction: the general path executes it.
    fn exit_lane<>(cx, r, ip, i, view) {
        exit(ip)
    }
}

#[path = "dispatch_fast/lane_frame.rs"]
pub(crate) mod lane_frame;
pub(crate) mod protocol_misses; use protocol_misses::*;
pub(crate) mod derived_frames;
pub(crate) mod local_fields;
use local_fields::{local_field_handler, local_field_miss};
pub(crate) mod window_bodies;
pub(crate) use window_bodies::{LocalsOf, body_locals};
pub(crate) mod lane_reads;
pub(crate) mod loop_body;
pub(crate) mod loop_resume;
use loop_body::LoopBody;
use lane_frame::{protocol_handler, returning_operator_row, operator_row, protocol_family, is_row, jump_row, jumps_by_row, plain_row, row_miss, row_near, row_produces, FLAG_LOOP, FLAG_ROOT, FLAG_SET, FLAG_UNREAD, FLAG_WINDOW, admits_row, name_row, FLAG_NAME_GLOBAL, FLAG_NAME_GLOBAL_SCOPED, FLAG_NAME_LEXICAL, FLAG_NAME_INTRINSIC, FLAG_NAME_NATIVE, FLAG_NAME_UNDEFINED, FLAG_SCOPE};


pub(crate) mod capture_append;
pub(crate) mod mapped_windows;
pub(crate) mod string_case_filter;

/// `ScopeReads::closure` of a window whose names do not resolve through its scope
/// (`FLAG_WINDOW_CLOSURE`): the closure over the running environment, as `window_closure_value_in_lane` makes it.
///
/// # Safety
/// As `lane_frame::ScopeReads`: `cx` is the running lane's context, `r` its register file.
unsafe fn window_closure_into(cx: *mut (), r: Registers, _ip: Ip, function: u32, registers: u16) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    if unsafe { (*cx).registers }.env().is_deleted() {
        return closure_value_of_in_lane(cx, function);
    }
    window_closure_value_in_lane(cx, r, function, registers)
}

/// `window_closure_into`'s closure of `function` over the running environment
/// activation's environment, whose window starts at register `registers`. Out of line, as
/// `compiled_new_object`: making the closure takes stack temporaries.
#[inline(never)]
fn window_closure_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    function: u32,
    registers: u16,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // The innermost read where it is: a plain header's eval record is its side record's
    // (one-frame-one-call.md F4 (3)).
    let activation = vm.lane_stack.top_frame()?;
    let owner = activation.function();
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let parent = unsafe { (*cx).registers }.env();
    // A module's hoisted declaration is its instance's closure, which the interpreter returns.
    if owner == quench_runtime::vm::ROOT_FUNCTION_ID && vm.is_module_function(unsafe { (*cx).program }, function) {
        return None;
    }
    // A closure that reads nothing of its environment takes none, so the window is not made.
    if code.closure_takes_no_environment(function) {
        let value = closure_in_lane(vm, code, function, Value::NULL);
        note_allocation(vm);
        return value;
    }
    // SAFETY: the running environment activation's window, whose function is the
    // innermost activation's and whose environment's parent is the context's.
    let window = |vm: &mut Vm| unsafe {
        let bindings = activation.take_record_bindings();
        window_environment(vm, code, r.0.add(usize::from(registers)), owner, parent, (*cx).program, r.this(), Some(bindings))
    };
    let value = match vm.closure_template_in_lane(function) {
        // A recorded closure is made from its template once: over another environment where
        // it reads nothing of this activation's (`Vm::maker_free_environment`), so the
        // activation needs none of its own for it, else over the activation's.
        Some(template) => {
            let env = match vm.maker_free_environment(&template, parent, owner) {
                Some(env) => env,
                None => window(vm),
            };
            Some(vm.closure_of_template(template, env))
        }
        None => {
            let environment = window(vm);
            closure_in_lane(vm, code, function, environment)
        }
    };
    note_allocation(vm);
    value
}

/// The value of a register or constant operand; other operand kinds take
/// the general path.
#[inline(always)]
fn operand_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    encoded: u16,
) -> Option<Value> {
    let operand = quench_runtime::bytecode::Operand(encoded);
    match operand.kind()? {
        quench_runtime::bytecode::OperandKind::Register => Some(r.get(operand.payload())),
        // SAFETY: the run's program keeps its constants (`LaneContext::constants`).
        quench_runtime::bytecode::OperandKind::Constant => unsafe { &*(*cx).constants }
            .get(usize::from(operand.payload()))
            .copied(),
        quench_runtime::bytecode::OperandKind::Local => plain_local_operand(cx, operand.payload()),
        _ => None,
    }
}

/// A call-out operand's value in the lane: a register's or a program constant's
/// (`operand_value_in_lane`); a call-out names only those two kinds.
#[inline(always)]
fn call_operand_in_lane(cx: *mut LaneContext, r: Registers, operand: quench_runtime::bytecode::Operand) -> Value {
    operand_value_in_lane(cx, r, operand.0).expect("a call-out operand is a register or a constant")
}

/// A local operand of the running function that is a plain slot, which the general loop
/// reads as `LoadLocalPlain` does (`Vm::operand_value`): generators and other functions that
/// keep frame locals compare and compute with them in place, and an environment
/// activation reads its window's local (`row_frame`'s `FLAG_WINDOW`). Any other frameless
/// activation has no locals; any other local is the general path's.
#[inline(never)]
fn plain_local_operand(cx: *mut LaneContext, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(activation) = vm.lane_stack.top_frame() {
        activation.script().filter(|script| script.environment)?;
        let body = code.function_get(activation.function() as usize)?;
        if !body.plain_local(code, usize::from(slot)) {
            return None;
        }
        // SAFETY: the running function's registers, which its window's locals follow.
        let value = unsafe { (*cx).registers }.get(body.registers + WINDOW_LOCALS + slot);
        return (!value.is_deleted()).then_some(value);
    }
    // SAFETY: its frame is the running one.
    let frame = unsafe { (*cx).frame() };
    let frame = vm.frames.get(frame)?;
    let slot = usize::from(slot);
    code.functions
        .get(frame.function as usize)?
        .plain_local(code, slot)
        .then(|| frame.locals.get(slot).copied())
        .flatten()
}

#[cfg(test)]
/// The integer `IncDec` of `value`, the update `dispatch::number_increment` makes of an
/// integer that does not overflow; `None` for any other value.
#[inline(always)]
pub(crate) fn integer_increment<const DECREMENT: bool>(value: Value) -> Option<i32> {
    // `Value::int_key_offset` is crate-private to quench-runtime; `as_int` is the public
    // int32 view and gives the same result for every value.
    let integer = value.as_int()?;
    if DECREMENT { integer.checked_sub(1) } else { integer.checked_add(1) }
}

/// The environment clone of plan `plan` a `CloneEnv` row makes in the lane context `cx` over its
/// register file `r` (`Vm::clone_frame_environment`, `Vm::iteration_environment`). `window` is the
/// register count of the environment window the record names, 0 where it names none. In a frame's own
/// run, nothing is copied while the frame holds no captured environment, else the frame's environment
/// is replaced by its copy (a `for (let ...)` iteration's, which a closure in the body captures), after
/// which the run's capture caches name the old environment and are dropped. In an environment
/// activation, nothing is copied while its window has made no environment; else the window's
/// environment is replaced by its iteration copy. Closures made earlier keep the old environment, whose
/// function-level slots the copy shares. Where the general path must make the clone (an error, or a
/// shape the lane does not take), it returns `false` and writes nothing.
///
/// # Safety
/// `cx` is the running lane's context and `r` its register file.
unsafe fn clone_environment_of(cx: *mut (), r: Registers, window: u16, plan: u32) -> bool {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    let env_set = !unsafe { (*cx).registers }.env().is_deleted();
    if window == 0 || !env_set {
        // A frame's own clone; an activation's environment is its window's, so none is cloned here.
        if vm.lane_stack.has_frames() {
            return false;
        }
        let frame = unsafe { (*cx).frame() };
        if vm.frames[frame].captured {
            // SAFETY: the context's program outlives the lane run.
            let code = unsafe { &*(*cx).code };
            let body = code.function(vm.frames[frame].function as usize);
            let Some(clone) = body.environment_clones.get(plan as usize) else { return false };
            // It checks the environment before it allocates the copy, so an error leaves the frame
            // for the general path to raise it from.
            if vm.clone_frame_environment(frame, &clone.fresh, body.environment_clones.resets(clone)).is_err() {
                return false;
            }
            note_allocation(vm);
            handler_edges::drop_environment_caches(cx);
        }
        return true;
    }
    // An environment activation's window, which starts `window` registers into the file.
    // SAFETY: the running environment activation's window starts at `r + window`.
    let entries = unsafe { r.0.add(usize::from(window)) };
    let source = unsafe { *entries.add(usize::from(WINDOW_ENVIRONMENT)) };
    if source.is_deleted() {
        return true;
    }
    let Some(function) = vm.lane_stack.top_frame().map(|activation| activation.function()) else {
        return false;
    };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let body = code.function(function as usize);
    let Some(clone) = body.environment_clones.get(plan as usize) else { return false };
    // It checks the environment before it allocates the copy, so an error leaves the activation for
    // the general path to raise it from.
    let Ok(environment) = vm.iteration_environment(source, unsafe { (*cx).program }, function, &clone.fresh, body.environment_clones.resets(clone))
    else {
        return false;
    };
    note_allocation(vm);
    let Some(quench_runtime::tier_surface::EnvironmentRecord { slots, .. }) = vm.heap_reads().environment(environment) else {
        unreachable!("an iteration copy is an environment");
    };
    let storage = std::ptr::from_ref(slots);
    // SAFETY: as above; the copy's slot storage never moves while it lives.
    unsafe {
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
        entries.add(usize::from(WINDOW_SLOTS)).write(std::mem::transmute::<u64, Value>(storage as u64));
    }
    true
}



lane_handler! {
    /// An instruction whose general step only continues or throws (`Op::steps_in_lane`),
    /// run in place by that step when the lane runs the frame's own code: the frame's
    /// registers are the lane's, and its PC is published as the general loop publishes it.
    /// A throw is stashed in `Vm::lane_raised` for the general path to raise at this
    /// instruction. Within a frameless activation, which has no frame, the general path runs
    /// it.
    #[inline(never)]
    fn general_step_in_lane<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // A running leaf has no activation, and the frame is its caller's: it becomes the
        // activation its call would have pushed, whose step runs beside the activations.
        materialize_running_leaf_in_lane(cx);
        if vm.lane_stack.has_frames() {
            resume!(cx, step_beside_activations_in_lane(cx, r, ip));
        }
        // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
        // active view, which is the running frame's.
        let (code, frame, pc) = unsafe {
            (&*(*cx).code, (*cx).frame(), ip.offset_from((*cx).base) as usize)
        };
        let function = code.function(vm.frames[frame].function as usize);
        let instruction = function.widened(function.code[pc]);
        // A return op itself (a raw return such as `WasmReturn64`) completes the frame, which the general path does.
        if instruction.op().traits().has(quench_runtime::bytecode::Traits::RETURN_FAMILY) {
            return exit(ip);
        }
        let returns = instruction.result_flags() & quench_runtime::bytecode::RETURN_REGISTER != 0;
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = false;
        let mut next = pc + 1;
        match vm.step(code, frame, instruction, &mut next, false) {
            Ok(StepResult::Continue) if !returns => {}
            // A record that returns its result (`RETURN_REGISTER`, a returning `a == b`): its value
            // completes the activation as a returning row's does, with no exit from the lane.
            Ok(StepResult::Return(value)) if returns => {
                if instruction.op().traits().has(Traits::MAY_ALLOCATE) {
                    note_allocation(vm);
                }
                resume!(cx, return_value_general_in_lane(cx, ip, value));
            }
            Ok(other) => unreachable!("an in-place step only continues: {:?} gave {}", instruction.op(), match other { StepResult::Return(_) => "Return", _ => "another result" }),
            Err(error) => {
                vm.lane_completion.raise(error);
                return exit(ip);
            }
        }
        // A step that may allocate or call guest code can make a collection due. An opcode without
        // `Traits::MAY_ALLOCATE` allocates nothing, so no collection can have become due by it.
        if instruction.op().traits().has(Traits::MAY_ALLOCATE) {
            note_allocation(vm);
        }
        // SAFETY: the step continues at a record of the running frame's view.
        let target = unsafe { (*cx).base.add(next) };
        if next > pc { next!(cx, r, target, Carried { memory: memory_after_step(cx, view.memory), ..Carried::stepped(cx, view, r, instruction) }) } else { exit(target) }
    }
}

script_handler! {
    /// A `Binary` row its words leave (an operator past the Number rows: `instanceof`, `in`, `**`, a
    /// string comparison): the lane's pure operation (`binary_value`) answers it in place, which
    /// runs no guest code, before the general step; the step beside activations would make them
    /// frames, and every return into them would leave the lane again.
    #[inline(never)]
    fn binary_miss_in_lane<>(cx, r, ip, i, view) {
        if let Some(value) = unsafe { binary_value(cx, r, i) } {
            note_allocation(unsafe { vm(cx) });
            r.set(i.a, value);
            next!(cx, r, following(ip), Carried { acc: Slot::of(value), ..Carried::script_in(cx, view) })
        }
        sibling!(general_step_in_lane, r, ip, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `binary_miss_in_lane` of a record whose operator is `OPERATOR`, a constant of the instance: where
    /// its operator row's Number form refused an operand pair (a string's `+`, a Boolean among Numbers),
    /// the pure operation (`binary_values`) folds to that operator's arms, with no generic row decoding
    /// the record and stepping the Number arms again first. What it leaves takes the general step.
    #[inline(never)]
    pub(crate) fn binary_operator_miss<OPERATOR: u8>(cx, r, ip, i, view) {
        debug_assert_eq!(i.imm(), u32::from(OPERATOR), "an operator row's record names its operator");
        if let (Some(left), Some(right)) = (operand_value_in_lane(cx, r, i.b), operand_value_in_lane(cx, r, i.c))
            && let Some(value) = unsafe { binary_values(cx, u32::from(OPERATOR), left, right) }
        {
            note_allocation(unsafe { vm(cx) });
            r.set(i.a, value);
            next!(cx, r, following(ip), Carried { acc: Slot::of(value), ..Carried::script_in(cx, view) })
        }
        sibling!(general_step_in_lane, r, ip, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// A plain `Binary` of `===` (`!==` where `NEGATE`): equal words are one value but a NaN, and two
    /// unequal int32 words two Numbers; any other pair is the strict equality's pure answer
    /// (`binary_values`), else the general step. No generic row decodes the operator first.
    #[inline(never)]
    pub(crate) fn strict_equality_row<NEGATE: bool>(cx, r, ip, i, view) {
        let operator = if NEGATE { oxc_ast::ast::BinaryOperator::StrictInequality } else { oxc_ast::ast::BinaryOperator::StrictEquality } as u32;
        let (Some(left), Some(right)) = (operand_value_in_lane(cx, r, i.b), operand_value_in_lane(cx, r, i.c)) else {
            sibling!(general_step_in_lane, r, ip, Carried::script_in(cx, view))
        };
        let equal = if left.to_bits() == right.to_bits() {
            (!left.as_number().is_some_and(f64::is_nan)).then_some(true)
        } else if left.as_int().is_some() && right.as_int().is_some() {
            Some(false)
        } else {
            None
        };
        let value = match equal {
            Some(equal) => Value::boolean(equal != NEGATE),
            None => match unsafe { binary_values(cx, operator, left, right) } {
                Some(value) => value,
                None => sibling!(general_step_in_lane, r, ip, Carried::script_in(cx, view)),
            },
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried { acc: Slot::of(value), ..Carried::script_in(cx, view) })
    }
}

/// The memory view the record after a general step carries: a Wasm run's frame memory, read again
/// as a call's return reads it (`complete_call`: the step may have grown it), never the script
/// view `Carried::stepped` makes. A Wasm record addressing through the script view wrote and read
/// the lane context's own bytes as linear memory (JetStream argon2-wasm: `argon2_hash` stored its
/// costs after a `v128.store`, which a general step makes, past the context, so validation read
/// zeros). `incoming` is the view the step's record ran with; one that is not the run's resolved
/// memory is none, whose accesses take the general path.
#[inline(always)]
fn memory_after_step(cx: *mut LaneContext, incoming: MemoryView) -> MemoryView {
    // SAFETY: the context's program outlives the lane run.
    if unsafe { (*(*cx).code).kind } != quench_runtime::bytecode::ProgramKind::Wasm {
        return MemoryView::script(cx);
    }
    let (binding, resolved) = unsafe { (*cx).resolved };
    let memory = if incoming.bytes == resolved.bytes {
        let fresh = memory_view(unsafe { vm(cx) }, binding);
        unsafe { (*cx).resolved = (binding, fresh) };
        fresh
    } else {
        MemoryView::NONE
    };
    unsafe { (*cx).memory = memory };
    memory
}

/// `general_step_in_lane` within a frameless activation: a value step (`Vm::call_out`) of
/// the activation's registers, made beside the activations as a native runs beside them
/// (`DeferredLaneCall`), so guest code it reaches makes them frames first; its result lands
/// in the running function's registers then. Any other step takes the general path.
#[cold]
#[inline(never)]
fn step_beside_activations_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip) -> Resumed {
    let vm = unsafe { vm(cx) };
    let Some(function) = vm.lane_stack.top_frame().map(|activation| activation.function()) else {
        return Resumed::Exit(exit(ip));
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(function as usize);
    let instruction = body.widened(body.code[pc]);
    // A record that returns its result completes its activation, which the general path does
    // (`general_step_in_lane`); no record follows it in the function.
    if !instruction.op().steps_with_result() || instruction.result_flags() & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        return Resumed::Exit(exit(ip));
    }
    let (operation, result) = quench_runtime::staged::Operation::<(), ()>::decode(instruction, quench_runtime::staged::DecodeContext::of(code))
        .and_then(|row| row.call_out()).expect("a result step is a call-out row");
    let operands = operation.operands().read(&code.method_arguments, |operand| call_operand_in_lane(cx, r, operand));
    let value = match unwound_in_place(cx, ip, |vm, code| {
        let site = quench_runtime::vm::call_out::CallSite { owner: vm.call_out_owner(operation), function, resume: pc + 1 };
        vm.call_out(code, site, operation, operands)
    }) {
        Ok(value) => value,
        Err(next) => return Resumed::Exit(next),
    };
    // Guest code the step ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    if let (Some(value), Some(result)) = (value, result) {
        r.set(result, value);
    }
    let memory = memory_after_step(cx, unsafe { (*cx).memory });
    Resumed::At(r, following(ip), Carried { memory, ..Carried::stepped(cx, Carried::script(cx), r, instruction) })
}

/// Make `operation` of the record at `ip` from the lane, as its general step would: in a
/// frame's own run with the frame's PC published (`general_step_in_lane`), or beside frameless
/// activations as a native beside them runs (`DeferredLaneCall`), so guest code it reaches
/// makes them frames first. A throw is stashed in `Vm::lane_raised`, raised from those
/// frames, and `None` is returned for the general path to raise it. The running function's
/// registers are `(*cx).registers` afterwards.
#[inline(always)]
fn in_place_in_lane<T>(
    cx: *mut LaneContext,
    ip: Ip,
    operation: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Option<T> {
    made_in_place(cx, ip, operation).map_err(|(error, made)| raised_in_place(cx, ip, error, made, false)).ok()
}

/// `in_place_in_lane` whose raise stays in the lane where the lane can unwind it
/// (frame-switches.md §5.3): `Err` carries the record the run goes on at, for the caller to
/// return as its handler's result (`Resumed::Exit` of it). The error was made as the
/// operation runs, with the record's pc published in a frame run, or deferred at the record
/// beside frameless activations (`DeferredLaneCall`), whose stack capture then read the frame
/// walk (`Vm::lane_stack_sites`); so it is the error the general path raises, and it is raised
/// and unwound at the record (`handler_edges::unwind_raised`): a handler of the running
/// function or of a plain caller takes it, else it leaves as a body's raise leaves
/// (`raised_exit_in_lane`). Where guest code or a declined walk made the activations frames
/// (`InPlaceMade::Materialized`), or the error is one the lane never unwinds
/// (`handler_edges::unwinds_in_lane`), it is raised from frames on the general path, exactly as
/// `in_place_in_lane` raises it.
#[inline(always)]
fn unwound_in_place<T>(
    cx: *mut LaneContext,
    ip: Ip,
    operation: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Result<T, Ip> {
    made_in_place(cx, ip, operation).map_err(|(error, made)| raised_in_place(cx, ip, error, made, true))
}

/// The raise of an operation made in place at the record `ip`, out of line: the record the run
/// goes on at. Where `unwind` asks and the lane can (`unwound_in_place`), the raise is unwound
/// in the lane (`handler_edges::unwind_raised`); else frameless activations nothing observed
/// are made frames, deferred at the record as the operation was, and the general path raises
/// from them (`exit`).
#[cold]
#[inline(never)]
fn raised_in_place(cx: *mut LaneContext, ip: Ip, error: JsError, made: InPlaceMade, unwind: bool) -> Ip {
    let vm = unsafe { vm(cx) };
    let unwinds = unwind && made != InPlaceMade::Materialized && handler_edges::unwinds_in_lane(&error);
    if made == InPlaceMade::Beside && !unwinds {
        // SAFETY: `ip` is a record of the running function's view.
        let pc = unsafe { ip.offset_from((*cx).base) as usize };
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc: pc, frames: vm.frames.len() });
        vm.materialize_deferred_lane();
    }
    vm.lane_completion.raise(error);
    if unwinds { handler_edges::unwind_raised(cx, ip) } else { exit(ip) }
}

/// Where an operation made in place (`made_in_place`) ran, as its raise needs to know.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InPlaceMade {
    /// In the frame's own run, its pc published at the record.
    Frame,
    /// Beside frameless activations, which nothing observed: they are still the lane's.
    Beside,
    /// Beside activations that guest code or a stack capture made frames.
    Materialized,
}

/// The operation of `in_place_in_lane`, made with its record published (frame) or deferred
/// (beside activations); its error with where it ran.
#[inline(always)]
fn made_in_place<T>(
    cx: *mut LaneContext,
    ip: Ip,
    operation: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Result<T, (JsError, InPlaceMade)> {
    materialize_running_leaf_in_lane(cx);
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // running function's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    // A Wasm activation the operation runs beside is at `pc` while it runs (`extend_wasm_roots`).
    vm.lane_stack.note_wasm_pc(pc);
    let beside = vm.lane_stack.has_frames();
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc: pc, frames: vm.frames.len() });
    } else {
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].pc = pc;
        vm.frames[frame].pc_is_resume = false;
    }
    let generation_key = (vm.cache_epochs.gc, vm.realm.globals);
    let result = operation(vm, code);
    // The operation may allocate, which can make a collection due.
    note_allocation(vm);
    if (vm.cache_epochs.gc, vm.realm.globals) != generation_key {
        after_nested_collection_in_lane(cx);
    }
    let made = if !beside {
        InPlaceMade::Frame
    } else if vm.lane_deferred.take().is_some() {
        InPlaceMade::Beside
    } else {
        InPlaceMade::Materialized
    };
    result.map_err(|error| (error, made))
}

/// A lane run continues past guest code an operation made in place (`in_place_in_lane`), and
/// that code collected or switched realms: everything the run resolved under its generation (call targets, read
/// memos, capture and frame-storage pointers) assumed no collection runs in the lane, so it
/// names cells the collection may have swept. The run takes a new generation, which retires
/// every cached target, and drops its storage pointers, as a fresh run would start.
#[cold]
#[inline(never)]
fn after_nested_collection_in_lane(cx: *mut LaneContext) {
    let vm = unsafe { vm(cx) };
    vm.lane_stack.enter_generation((vm.cache_epochs.gc, vm.realm.globals));
    // SAFETY: `cx` is the running context, of this Host.
    unsafe { retire_lane_binding(cx.cast()) };
}

#[cfg(test)]
/// A read's value may occupy either side of canonical commutative Number arithmetic.
/// Its register was already written, so reading the other register preserves aliases.
/// Ordered operations retain both original operands. A non-Number still declines through
/// the shared Number operation; its original record owns evaluation/coercion order.
#[inline(always)]
fn read_number_operands<const OP: u32>(r: Registers, operator: &LaneInstruction, other: u16, element: Value) -> (Value, Value) {
    if quench_runtime::vm::operations::number_result_is_commutative(OP) {
        (element, r.get(other))
    } else {
        (r.get(operator.b), r.get(operator.c))
    }
}

/// The index a proved element record (`specialize_proved_arrays`) reads at: a non-negative
/// integer key, as `Vm::direct_dense_element` takes one.
#[cfg(all(test, feature = "bench-primitives"))]
#[inline(always)]
pub(crate) fn proof_index(key: Value) -> Option<usize> {
    usize::try_from(key.as_int()?).ok()
}

/// A `Binary` record's value where it runs no guest code: Number arithmetic and
/// relations, then identity equality, an ordinary `instanceof` and an `in` whose key
/// and receiver (`Vm::plain_in`) need no guest code.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
unsafe fn binary_value(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let left = operand_value_in_lane(cx, r, i.b)?;
    let right = operand_value_in_lane(cx, r, i.c)?;
    unsafe { binary_values(cx, i.imm(), left, right) }
}

/// The value of `left operator right` where it runs no guest code (the pure part of a returning
/// `Binary`, which the lane frame answers a row's `Binary` or `Equality` call-out with).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn binary_values(
    cx: *mut LaneContext,
    operator: u32,
    left: Value,
    right: Value,
) -> Option<Value> {
    unsafe { binary_values_classified(cx, operator, left, right, Value::numeric_pair(left, right)) }
}

/// `binary_values` of operands whose classification `pair` is `Value::numeric_pair(left,
/// right)`, which a caller that has classified them already passes so the Number path
/// does not classify them again. The same value as `binary_values` for the same operands.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn binary_values_classified(
    cx: *mut LaneContext,
    operator: u32,
    left: Value,
    right: Value,
    pair: quench_runtime::value::NumericPair,
) -> Option<Value> {
    match unsafe { vm(cx) }.number_binary_classified(operator, left, right, pair) {
        Some(value) => Some(value),
        // `undefined`, `null` or a Boolean among Numbers, as Sunspider's `x | 0` of a missing
        // element or an emulator's `flag & mask`; a string key's `"k" + i` first.
        None if let Some(value) = concat_or_immediate(cx, operator, left, right) => Some(value),
        None if operator == quench_runtime::vm::operations::ADDITION => concat_value(cx, left, right)
            .or_else(|| bigint_value(cx, operator, left, right)),
        // A comparison, `instanceof`, or an ordinary-object `in` (`relation_other`),
        // decides from the operands; any other operator past Numbers may be a BigInt's.
        None if operator <= quench_runtime::vm::operations::LAST_COMPARISON_OPERATOR
            || operator == INSTANCEOF_OPERATOR
            || operator == IN_OPERATOR =>
        {
            relation_outcome(operator, left, right)
                .map(|holds| if holds { Value::TRUE } else { Value::FALSE })
        }
        // `**` of two Numbers (raytrace's `x ** 2`), which `number_binary` leaves.
        None if operator == oxc_ast::ast::BinaryOperator::Exponential as u32
            && let (Some(base), Some(exponent)) = (left.as_number(), right.as_number()) =>
        {
            Some(Value::number(quench_runtime::vm::operations::exponentiate(base, exponent)))
        }
        None => bigint_value(cx, operator, left, right),
    }
}

/// `+` of a string operand (`concat_value`), as a key built as `"k" + i`, before the
/// immediate operands' tests, which a string fails; else `Vm::immediate_binary`.
#[inline(always)]
fn concat_or_immediate(cx: *mut LaneContext, operator: u32, left: Value, right: Value) -> Option<Value> {
    let heap = unsafe { vm(cx) }.heap_reads();
    if operator == quench_runtime::vm::operations::ADDITION && (heap.is_string(left) || heap.is_string(right))
        && let Some(value) = concat_value(cx, left, right)
    {
        return Some(value);
    }
    unsafe { vm(cx) }.immediate_binary(operator, left, right)
}

/// The value of a returning `Binary`, which keeps its operands encoded: each operand is read
/// once (`operand_value_in_lane`, so a register, a folded constant or a plain local), after two
/// integer registers are tried as they stand. Two int32 operands decide it in place (`integer_binary`), as the operator-specialized records decide
/// theirs; the other operands are classified once (`Value::numeric_pair`) for the general value.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
unsafe fn returning_binary_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    // Two integer registers decide it without decoding the operands further.
    let register = |encoded: u16| {
        let operand = quench_runtime::bytecode::Operand(encoded);
        (operand.kind() == Some(quench_runtime::bytecode::OperandKind::Register)).then(|| r.get(operand.payload()))
    };
    if let (Some(left), Some(right)) = (register(i.b), register(i.c))
        && let Some((a, b)) = Value::int_pair(left, right)
        && let Some(value) = quench_runtime::vm::operations::integer_binary(i.imm(), a, b)
    {
        return Some(value);
    }
    let left = operand_value_in_lane(cx, r, i.b)?;
    let right = operand_value_in_lane(cx, r, i.c)?;
    // A folded constant takes the integer path a register's value does; any other pair is
    // classified once, and the Number path reuses that classification.
    if let Some((a, b)) = Value::int_pair(left, right)
        && let Some(value) = quench_runtime::vm::operations::integer_binary(i.imm(), a, b)
    {
        return Some(value);
    }
    unsafe { binary_values_classified(cx, i.imm(), left, right, Value::numeric_pair(left, right)) }
}

/// A BigInt operator of two BigInts that raises nothing (`Vm::bigint_binary_value`), which
/// allocates the result; `None` leaves the operation, and any error, to the general path.
#[inline(never)]
fn bigint_value(
    cx: *mut LaneContext,
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
fn concat_value(cx: *mut LaneContext, left: Value, right: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.primitive_concatenation(unsafe { &*(*cx).code }, left, right)?;
    note_allocation(vm);
    Some(value)
}

/// A relation the lane decides without running guest code: Number comparison, identity
/// decided from the operand words, then `relation_other`. Matches rather than
/// `Option::or_else` closures, which capture the operands by reference and, left out of
/// line, would cost the calling handler its tail call to the next record.
#[inline(always)]
fn relation_outcome(operator: u32, left: Value, right: Value) -> Option<bool> {
    // Two heap values are no Numbers: their identity decides without the Number decode.
    if left.is_heap() && right.is_heap() && let Some(holds) = Vm::word_identity_equality(operator, left, right) {
        return Some(holds);
    }
    if let Some((holds, _)) = quench_runtime::vm::operations::number_relation(operator, left, right) {
        return Some(holds);
    }
    if let Some(holds) = Vm::word_identity_equality(operator, left, right) {
        return Some(holds);
    }
    relation_other(operator, left, right)
}

/// `lane_return_value!` past the common return (`complete_plain_activation_in_lane`): any
/// other activation's completion, or the frame's return. Out of line, so the returning
/// handlers keep their registers.
#[inline(never)]
fn return_value_general_in_lane(cx: *mut LaneContext, ip: Ip, value: Value) -> Resumed {
    // Another program's activation, which a program that calls across programs returns from
    // as often as the common return, completes without the frame the other completions need.
    if let Some(resumed) = complete_native_step_in_lane(cx, value) {
        return resumed;
    }
    if let Some(resumed) = lane_complete_wasm_activation(cx, value) {
        return resumed;
    }
    if let Some(resumed) = complete_cross_activation_in_lane(cx, value) {
        return resumed;
    }
    if let Some(resumed) = complete_returning_chain_in_lane(cx, value) {
        return resumed;
    }
    return_value_other_in_lane(cx, ip, value)
}

/// The return of a native's callback activation, one per element or comparison: an array
/// callback's (`Completion::ArrayStep`) takes the step of its iteration, any other native's
/// (`Completion::ReturnsToNative`, a sort's comparator, a prepared callback) ends the run with
/// the value. Tested first among the returns past the common one, since neither is a Wasm,
/// cross-program or chained activation, so a callback loop pays none of their tests. `None`
/// for any other activation.
#[inline(always)]
fn complete_native_step_in_lane(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // Read where it is (a header's side record, a step header or a record), no spill.
    match vm.lane_stack.top_frame().and_then(|frame| frame.script()) {
        Some(script) if script.completion == Completion::ArrayStep => step_array_iteration_in_place_in_lane(cx, value),
        Some(script) if script.completion == Completion::ReturnsToNative => complete_native_callback_in_lane(cx, value),
        _ => None,
    }
}

/// `return_value_general_in_lane` of any other activation's completion, or the frame's return.
#[inline(never)]
fn return_value_other_in_lane(cx: *mut LaneContext, ip: Ip, value: Value) -> Resumed {
    if let Some(resumed) = complete_returning_in_lane(cx, ip, value) {
        return resumed;
    }
    if unsafe { (*cx).returns_script_frame } {
        // This completion already owns its value; native SSA may never have
        // written the Return register. Publish it if this frame cannot retire.
        return Resumed::Exit(return_script_frame_in_lane(cx, ip, value, true));
    }
    Resumed::Exit(frame_return(unsafe { vm(cx) }, ip, value))
}


/// The function `LoadCallee` reads (`ActivationView::callee`): the innermost script activation's, or the
/// frame's when none runs, as its activation filled its self binding (`Vm::initialize_frame_locals`).
/// `None` where the innermost activation is not a script or is windowed, which the general step reads.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn running_callee(cx: *mut ()) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    match vm.lane_stack.top_frame().map(|frame| frame.kind()) {
        // A windowed activation names the owner of its frame, which is the record's.
        Some(ActivationKind::Script(script)) if !script.completion.is_windowed() => Some(script.callee),
        Some(_) => None,
        None => Some(vm.frames[unsafe { (*cx).frame() }].context.callee().unwrap_or(Value::UNDEFINED)),
    }
}






/// `plain_keyed_get` at lane site `ip`, recording the own slot a symbol or index key's proof
/// gives for the site's next read (`Vm::site_keyed_own_read`). Out of line: the site's hits
/// skip it.
#[inline(never)]
fn site_keyed_get_in_lane(vm: &mut Vm, ip: Ip, base: Value, key: Value) -> Option<Value> {
    let value = vm.plain_keyed_get(base, key)?;
    vm.record_site_keyed_read(ip.addr(), base, key);
    Some(value)
}

/// A string's element in the lane (`Vm::string_element`), which allocates its one-unit string.
#[inline(never)]
fn string_element_in_lane(vm: &mut Vm, base: Value, key: Value) -> Option<Value> {
    let value = vm.string_element(base, key)?;
    note_allocation(vm);
    Some(value)
}

lane_handler! {
    /// A present dense array element; any other read takes the general path.
    fn get_index_miss<>(cx, r, ip, i, view) {
        let (Some(base), Some(key)) =
            (operand_value_in_lane(cx, r, i.b), operand_value_in_lane(cx, r, i.c))
        else {
            return exit(ip);
        };
        let vm = unsafe { vm(cx) };
        let key = vm.canonical_index_key(key);
        // A register-operand read (the element row's form) that found a typed element switches to
        // the typed row of its kind (`row_typed_element`), so the next read makes no protocol miss.
        let registers = i.a <= quench_runtime::bytecode::REGISTER_MASK
            && i.b <= quench_runtime::bytecode::REGISTER_MASK
            && i.c <= quench_runtime::bytecode::REGISTER_MASK;
        let Some(value) = vm
            .dense_array_element(base, key)
            .or_else(|| if registers { row_typed_element(ip, vm, base, key) } else { vm.plain_typed_element(base, key) })
            .or_else(|| string_element_in_lane(vm, base, key))
            .or_else(|| vm.absent_array_element(base, key))
            .or_else(|| vm.plain_keyed_get(base, key))
            .or_else(|| vm.keyed_array_length(base, key))
        else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

script_handler! {
    /// The element row's read past its in-place arms (a hole, a string or symbol key, a typed or
    /// string element): the VM's read at the site (`row_index_read`), else the getter's
    /// activation (`row_index_getter_miss`). Apart, so the row's fast path makes no call.
    #[inline(never)]
    fn row_index_read_miss<>(cx, r, ip, i, view) {
        let Some(value) = row_index_read(cx.cast(), ip, r.get(i.b), r.get(i.c)) else {
            sibling!(row_index_getter_miss, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The element row's refused read (`row_index_read` found nothing): an index getter's
    /// activation (`index_getter_activation_in_lane`), else the general path. Apart, since the
    /// activation's result is a stack temporary of the handler that makes it.
    #[inline(never)]
    fn row_index_getter_miss<>(cx, r, ip, i, view) {
        let (base, key) = (r.get(i.b), r.get(i.c));
        match AccessorCall::of(cx).keyed_get_trap(r, ip, base, key) {
            Some(Ok((registers, base))) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Some(Err(next)) => return next,
            None => {}
        }
        let vm = unsafe { vm(cx) };
        if vm.lane_stack.has_frames() && vm.proved_symbol_getter(base, key).is_none() {
            resume!(cx, get_index_beside_activations_in_lane(cx, ip, i, base, key));
        }
        match index_getter_activation_in_lane(cx, r, ip, base, key) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(next) => next,
        }
    }
}

/// A `GetIndex` of `base[key]` past the lane's reads within a frameless activation: the
/// general read made in place (`in_place_in_lane`), as `row_get_field_beside_activations`
/// makes a field's, so a getter, proxy or throw makes the activations frames only when it
/// runs.
#[cold]
#[inline(never)]
fn get_index_beside_activations_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    base: Value,
    key: Value,
) -> Resumed {
    let value = match unwound_in_place(cx, ip, |vm, code| vm.get_index(code, base, key)) {
        Ok(value) => value,
        Err(next) => return Resumed::Exit(next),
    };
    // Guest code the read ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    r.set(i.a, value);
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// The element row's host read (`ElementFrame`'s `index_get_host`) at site `ip` of `base[key]`
/// past the row's dense arm: a symbol-keyed read this site made of the same shape and key reads
/// its own slot; else a dense or absent array element, a keyed read this site recorded, a typed
/// array's or a string's element. Out of line, as `row_index_write`.
#[inline(never)]
fn row_index_read(context: *mut (), ip: Ip, base: Value, key: Value) -> Option<Value> {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    if let Some(value) = vm.site_keyed_own_read(ip.addr(), base, key) {
        return Some(value);
    }
    let key = vm.canonical_index_key(key);
    // A typed array answers every index read itself, never its prototype chain, and a string
    // every index inside its length, so their reads come before the ordinary array probes,
    // which would only decline them.
    vm.dense_array_element(base, key)
        .or_else(|| named_key_element(vm, base, key))
        // merge: theirs' `lane_typed_array_element` is undefined; HEAD's `row_typed_element` serves the typed read.
        .or_else(|| row_typed_element(ip, vm, base, key))
        .or_else(|| string_element_in_lane(vm, base, key))
        .or_else(|| vm.absent_array_element(base, key))
        .or_else(|| site_keyed_get_in_lane(vm, ip, base, key))
        .or_else(|| vm.keyed_array_length(base, key))
}

/// A typed array's element at an index key (`Vm::plain_typed_element`), which no dense array
/// read before it serves: the site then reads it through its typed element row
/// (`TYPED_SITE`), whose kind the record's immediate holds (`requicken_typed`).
fn row_typed_element(ip: Ip, vm: &mut Vm, base: Value, key: Value) -> Option<Value> {
    let value = vm.plain_typed_element(base, key)?;
    vm.publish_typed_view(base);
    requicken_typed(ip, true, vm.typed_array_kind_of(base));
    Some(value)
}

/// Switches the element record at `ip` to its typed row, read or store, for the element kind
/// `kind` names: the record's immediate holds the native kind's index, which the row's view
/// proof reads (`LaneFrame::kept_view`). A kind with no native element keeps the record.
fn requicken_typed(ip: Ip, read: bool, kind: Option<TypedArrayKind>) {
    let Some(index) = kind.and_then(quench_runtime::staged::typed_elements::native_kind_index) else { return };
    // SAFETY: `ip` is a live element record of the view; an element record's operand holds no
    // loop site or call target, so its immediate is free for the kind.
    unsafe { (*ip.cast_mut()).operand.imm = index };
    let handler = if read { element_typed_read_row() } else { element_typed_write_row() };
    requicken(ip, handler);
}

script_handler! {
    /// `delete base[key]` by the staged row: a string key on an ordinary object runs no guest
    /// code (`delete_value_in_lane`); any other deletion takes the general path.
    #[inline(never)]
    fn delete_miss<>(cx, r, ip, i, view) {
        if !lane_frame::staged_delete(cx, r, [i.a, i.b, i.c], i.imm() != 0) {
            return exit(ip);
        }
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// A lane `GetIndex` of a symbol key whose proof records an inherited getter
/// (`Vm::proved_symbol_getter`): the getter runs as a frameless activation of the read's
/// record, as `getter_activation_in_lane` runs a field's, with the base as receiver. `Err`
/// carries the record to continue at: the general path for any other read.
#[inline(never)]
fn index_getter_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    base: Value,
    key: Value,
) -> Result<(Registers, Ip), Ip> {
    // The getter's activation asks the admission word (`lane_activate_getter`).
    let vm = unsafe { vm(cx) };
    let Some(getter) = vm.proved_symbol_getter(base, key) else {
        return Err(exit(ip));
    };
    activate_getter_in_lane(cx, r, ip, getter, base, None)
}

script_handler! {
    /// Replacing a present dense element; any other write takes the general path.
    /// An array whose object is a record runs here, saving no registers; any other object
    /// takes `set_index_slot_miss`.
    fn set_index_miss<>(cx, r, ip, i, view) {
        let (object, key) = (r.get(i.b), r.get(i.c));
        if !(unsafe { vm(cx) }).try_replace_record_element(object, key, r.get(i.a)) {
            sibling!(set_index_slot_miss, r, ip, Carried::script_in(cx, view))
        }
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `set_index_miss` past a record array, appending to a dense array at its length,
    /// filling one of its holes, a Number stored into a typed array
    /// (`Vm::typed_array_set_number`), or a plain symbol-, string- or integer-keyed write. A
    /// symbol write repeats the step its site recorded for the receiver's shape first
    /// (`Vm::site_symbol_set`).
    #[inline(never)]
    fn set_index_slot_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !vm.lane_stack.has_frames() {
            return exit(ip);
        }
        resume!(cx, row_set_index_activation(cx, r, ip, i));
    }
}

/// `row_set_index_refused` past the lane's stores within a frameless activation: the general
/// store made in place (`in_place_in_lane`), strict as its record or function is.
#[cold]
#[inline(never)]
fn row_set_index_activation(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    let vm = unsafe { vm(cx) };
    let Some(function) = vm.lane_stack.top_frame().map(|activation| activation.function()) else {
        return Resumed::Exit(exit(ip));
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(function as usize);
    let instruction = body.widened(body.code[pc]);
    if !quench_runtime::staged::writes_element(instruction.op()) {
        return Resumed::Exit(exit(ip));
    }
    let strict = body.strict || instruction.boolean_flag().unwrap_or(false);
    let (object, key, value) = (r.get(i.b), r.get(i.c), r.get(i.a));
    if let Err(next) = unwound_in_place(cx, ip, |vm, code| vm.set_index_mode(code, object, key, value, strict)) {
        return Resumed::Exit(next);
    }
    // Guest code the store ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// The element row's host store (`ElementFrame::index_set_host`) of `value` at `object[key]` for
/// the record at `ip`: a hole, a symbol, a dense replacement or append, a typed array's number,
/// a plain string key or an index property; `false` stores nothing, which the row's miss takes.
/// Out of line, as every store a lane handler makes through calls whose stack temporaries
/// escape: the handler's frame then holds none.
#[inline(never)]
fn row_index_write(context: *mut (), ip: Ip, object: Value, key: Value, value: Value) -> bool {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    if !vm.try_fill_record_hole(object, key, value)
        && !vm.site_symbol_set(ip.addr(), object, key, value)
        && !vm.try_replace_dense_element(object, key, value)
        && !vm.try_append_dense_element(object, key, value)
        && !vm.try_fill_dense_hole(object, key, value)
        && !vm.try_site_symbol_set(ip.addr(), object, key, value)
        && !vm.try_host_private_symbol_set(object, key, value)
        && !row_typed_store(ip, vm, object, key, value)
        && !vm.try_plain_string_key_set(object, key, value)
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
/// paths before it serve: the site then stores through its typed element row (`TYPED_SITE`).
fn row_typed_store(ip: Ip, vm: &mut Vm, object: Value, key: Value, value: Value) -> bool {
    let stored = vm
        .array_index_key(key)
        .is_some_and(|index| vm.typed_array_set_number(object, index as usize, value));
    if stored {
        // The typed row proves the view the VM publishes (`LaneFrame::kept_view`): unpublished, every
        // store would miss back here.
        vm.publish_typed_view(object);
        requicken_typed(ip, false, vm.typed_array_kind_of(object));
    }
    stored
}

/// A capture of a root environment's global var or global lexical
/// (`Vm::plain_projected_root_binding`), which a script's functions read for its top-level
/// `var`s, and in an eval script's context its `let`s and `const`s: the capture's environment
/// as the general path finds it, from the frame or from the activation's environment.
#[inline(never)]
fn projected_capture(cx: *mut LaneContext, depth: u16, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).registers }.env();
    let env = if env.is_deleted() {
        vm.owner_capture_env(VarOwner::Frame(unsafe { (*cx).frame() }), depth)?
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
fn frame_capture_in_lane(cx: *mut LaneContext) -> Option<*const usize> {
    let vm = unsafe { vm(cx) };
    let frame = unsafe { (*cx).frame() };
    let Some((slots, None)) = vm.current_capture_slots(frame) else {
        return None;
    };
    // SAFETY: the frame's cache names live slots this collection; none runs in the lane.
    let dense = unsafe { &*slots }.dense_storage()?;
    // The cache's slots are those of the frame's depth-0 capture environment.
    let env = vm.owner_capture_env(VarOwner::Frame(frame), 0)?;
    // SAFETY: the context's program outlives the lane run.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    if vm.admits_every_direct_store(code, program, env) {
        vm.write_barrier(env);
        return Some(dense.map_addr(|address| address | CAPTURE_STORES));
    }
    Some(dense)
}

/// Resolve an owned or shared lane capture without guest code, refreshing the dense cache.
/// OSR entry and the LoadCapture slow lane use the same binding and TDZ guards.
#[inline(never)]
fn capture_read_value(cx: *mut LaneContext, depth: u16, slot: u16) -> Option<Value> {
    read_capture_value::<false>(cx, depth, slot)
}

/// `capture_read_value`, with `PLAIN` also a root projection's plain read
/// (`Vm::plain_capture`), for a `LoadCapture` record, which takes the value however held.
#[inline(always)]
/// Whether a capture of `depth` in the running activation's environment settles on a root binding
/// (the root `walked_capture_slots` reports), which a record's root row answers (`FLAG_ROOT`).
fn capture_settles_on_root(cx: *mut LaneContext, depth: u16) -> bool {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).registers }.env();
    !env.is_deleted() && vm.walked_capture_slots(env, false, depth).is_some_and(|(_, root)| root.is_some())
}

fn read_capture_value<const PLAIN: bool>(cx: *mut LaneContext, depth: u16, slot: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).registers }.env();

    if env.is_deleted() {
        let frame = unsafe { (*cx).frame() };
        let value = if PLAIN {
            vm.cached_plain_capture(frame, depth, slot)
        } else {
            vm.cached_direct_capture(frame, depth, slot)
        };
        if depth == 0 {
            frame_capture_at_root(cx, frame);
        }
        value
    } else {
        // A frameless activation's function has no environment of its own.
        let (slots, root) = vm.walked_capture_slots(env, false, depth)?;
        // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
        let slots = unsafe { &*slots };
        if depth == 0
            && root.is_none()
            && let Some(dense) = slots.dense_storage()
        {
            unsafe { (*cx).registers.set_capture(dense) };
        }
        if PLAIN { vm.plain_capture(slots, root, slot) } else { vm.owned_capture(slots, root, slot) }
    }
}

script_handler! {
    /// `lane_load_capture` before the running function's slots are known: the walk from
    /// the frame's cache, or from the activation's environment, which keeps them.
    #[inline(never)]
    fn load_capture_walk_miss<>(cx, r, ip, i, view) {
        let imm = i.imm();
        let depth = ImmediateLayout::capture_depth(imm);
        let slot = ImmediateLayout::capture_slot(imm);
        // The root reads' fast paths (a global var's hint, a global lexical's index, a root
        // binding slot read in place) are gone: every read takes the walk, which answers the same
        // value through `read_capture_value`. Their record requickening went with them.
        let value = read_capture_value::<true>(cx, depth, slot);
        #[cfg(feature = "profile-lane-exits")]
        {
            let vm = unsafe { vm(cx) };
            let env = unsafe { (*cx).registers }.env();
            let code = unsafe { &*(*cx).code };
            let function = running_function_in_lane(cx);
            let body = code.function_get(function as usize);
            let shape = vm.current_capture_slots(unsafe { (*cx).frame() }).map(|(slots, root)| {
                (root.is_some(), unsafe { &*slots }.dense().is_some(), unsafe { &*slots }.len())
            });
            quench_runtime::vm::lane_exits::record_detail(format!(
                "capture walk in {} {} span={:?} depth={depth} frame_run={} (root, dense)={shape:?}",
                vm.programs.source_name(unsafe { (*cx).program }).unwrap_or_default(),
                body.and_then(|body| body.name).map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                body.and_then(|body| body.source_span).map(|span| span.start),
                env.is_deleted(),
            ));
        }
        if let Some(value) = value {
            r.set(i.a, value);
            // A walk that settled on a root binding requickens the record to its root row (`FLAG_ROOT`),
            // which reads the binding through the same reads this walk takes.
            if capture_settles_on_root(cx, depth) {
                requicken(ip, generated_row!(quench_runtime::staged::LOAD_CAPTURE_ROW, ACC_NONE, FLAG_ROOT) as Handler);
            }
            next!(cx, r, following(ip), view)
        }
        // A capture of a root environment's global var or lexical: the staged row's root read.
        if !lane_frame::staged_load_capture(cx, r, view, i.a, (depth, slot), true) {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}



/// The direct write of `value` to the running function's own capture `slot` through its dense
/// slots, where they admit every store (`CAPTURE_STORES`) and the slot holds an initialized
/// binding: whether it wrote.
#[inline(always)]
fn store_own_capture(cx: *mut LaneContext, slot: u16, value: Value) -> bool {
    capture_stores(unsafe { (*cx).registers }.capture())
        // SAFETY: the running function's dense capture slots, which admit this write
        // (`CAPTURE_STORES`); no collection runs in the lane.
        .and_then(|slots| unsafe { quench_runtime::tier_surface::EnvironmentSlots::write_storage(slots, usize::from(slot), value) })
        .is_some()
}

/// The running lane activation's dense capture slots, the view a lane body's reads take
/// (`EnvironmentSlots::native_storage_view`), and whether they admit every direct store
/// (`capture_stores`): a frame run finds them as its first store does (`store_to_capture`).
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_captures(context: usize) -> Option<((*const Value, usize), bool)> {
    let cx = context as *mut LaneContext;
    unsafe {
        if capture_reads((*cx).registers.capture()).is_null() && (*cx).registers.env().is_deleted() {
            (*cx).registers.set_capture(frame_capture_in_lane(cx)?);
        }
        // SAFETY: the context's slots stay live while the lane runs, which collects nothing.
        let (view, stores) = capture_view((*cx).registers.capture());
        Some((view?, stores))
    }
}

/// The running lane activation's receiver (`this_in_lane`), which a loop region it runs reads as a
/// lane body does: a frameless activation's own, not the VM frame's beneath it; `DELETED` where
/// it has none.
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_this_in_lane(context: usize) -> Value {
    let cx = context as *mut LaneContext;
    unsafe { this_in_lane(cx, (*cx).registers) }.unwrap_or(Value::DELETED)
}

/// The object a callee-argument regexp literal record of literal `site`, whose call's callee
/// is `callee`, makes in a frame's own lane run: the general loop's
/// (`Vm::regexp_literal_argument_of`), keyed by the frame. `None` inside a lane activation,
/// which the general path makes it for, and where the site raises, which `Vm::lane_raised`
/// reports at the record.
#[inline(never)]
fn lane_regexp_argument_value(cx: *mut LaneContext, callee: Value, site: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() || !unsafe { (*cx).has_frame() } {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let (code, program, frame) = unsafe { (&*(*cx).code, (*cx).program, (*cx).frame()) };
    let result = vm.regexp_literal_argument_of(code, program, frame, site as usize, callee);
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// The closure a `MakeClosure` record makes as the general loop does, in a frame's own lane
/// run of a function with no binding sites outside a module's root: its environment is the
/// frame's, promoted once, and making it allocates and runs no guest code. Any other
/// closure takes the general path; a promotion made first is idempotent.
#[inline(never)]
fn closure_value_of_in_lane(cx: *mut LaneContext, id: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame()) };
    let function = vm.frames[frame].function;
    if function == quench_runtime::vm::ROOT_FUNCTION_ID
        || !code.function_get(function as usize)?.binding_sites.is_empty()
    {
        return None;
    }
    let env = if code.closure_takes_no_environment(id) {
        Value::NULL
    } else {
        let env = vm.promote_frame_environment(frame);
        // Promotion moves the frame's captured locals into the environment, so a capture
        // slot pointer found before it is not reused.
        unsafe { (*cx).registers.set_capture(std::ptr::null()) };
        env
    };
    let value = closure_in_lane(vm, code, id, env);
    note_allocation(vm);
    value
}

/// The closure of `function` over `env` that a lane `MakeClosure` makes: a call-only one
/// (`Function::prototype_unobservable`) as a `Value::known_closure`, with no function object,
/// while no `with` was ever entered (a `with` object could take the binding's store);
/// any other through `Vm::closure`.
#[inline(always)]
fn closure_in_lane(vm: &mut Vm, code: &ResidualProgram, function: u32, env: Value) -> Option<Value> {
    if !vm.with_scope_entered
        && code.functions.get(function as usize).is_some_and(|slot| slot.header().prototype_unobservable)
        && let Some(known) = Value::known_closure(function, env)
    {
        return Some(known);
    }
    vm.closure(code, function, env).ok()
}

/// The object a `CreateRegExpLiteral` record makes in the lane: its site belongs to the
/// running program, the frame's or an activation's (`LaneContext::program`). A pattern that
/// fails to compile raises at its instruction through `Vm::lane_raised` in a frame's own
/// run; inside a lane activation, `None` leaves it to the general path, which raises the
/// same error from the site's cached record.
#[inline(never)]
fn regexp_literal_value_in_lane(cx: *mut LaneContext, site: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    let result = vm.regexp_literal_of(code, program, site as usize);
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            if !vm.lane_stack.has_frames() {
                vm.lane_completion.raise(error);
            }
            None
        }
    }
}

/// The window of property values a `MakeObjectLiteral` record reads.
#[inline(always)]
fn literal_window_in_lane(i: &LaneInstruction) -> quench_runtime::bytecode::RegisterWindow {
    quench_runtime::bytecode::RegisterWindow { base: i.b, count: i.c }
}

/// The object a staged object literal row makes: its site's shape over the window of property
/// values (`Vm::object_site_shape`).
#[inline(always)]
fn object_literal_value_in_lane(cx: *mut LaneContext, r: Registers, site: u32,
    window: quench_runtime::bytecode::RegisterWindow) -> Value {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let shape = vm.object_site_shape(unsafe { &*(*cx).code }, site as usize);
    // SAFETY: `fields_fit_in_lane` admitted the window inside the register file, and no
    // other reference into it is live while the object is allocated.
    let properties = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(window.base)), usize::from(window.count)) };
    let value = vm.allocate_object_with_properties(vm.object_proto, shape, properties);
    note_allocation(vm);
    value
}

/// The array of the values in `elements` (`Vm::literal_array`), out of line as `compiled_new_object`.
#[inline(never)]
fn literal_array_value_in_lane(vm: &mut Vm, r: Registers, elements: RegisterList) -> Value {
    let registers = elements.registers();
    let value = vm.literal_array(registers.len(), |index| r.get(registers[index]));
    note_allocation(vm);
    value
}

/// A literal's empty object (`Allocation::Empty`). Out of line: an allocation whose record space is full falls
/// back to a heap slot, which takes the new object by its stack address, a temporary the
/// handler then does not hold.
#[inline(never)]
fn new_object_in_lane(vm: &mut Vm) -> Value {
    let value = vm.object();
    note_allocation(vm);
    value
}

/// The `MakeArray` row's array of `length` holes (`Vm::make_array`, a double backing where the
/// literal's annotation declares Number elements), out of line as `compiled_new_object`.
#[inline(never)]
fn new_hole_array_in_lane(vm: &mut Vm, length: usize, numbers: bool) -> Value {
    let value = vm.make_array(length, numbers);
    note_allocation(vm);
    value
}

/// The object of literal `site`'s shape holding registers `first` and `second` (`Vm::object_pair`).
#[inline(always)]
fn object_pair_in_lane(cx: *mut LaneContext, r: Registers, site: u32, [first, second]: [u16; 2]) -> Value {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.object_pair(unsafe { &*(*cx).code }, site as usize, r.get(first), r.get(second));
    note_allocation(vm);
    value
}

/// The four records of superinstruction `index` of the running lane's program, which a
/// `SuperConstArrayObject2` record names (`Operation::decode_literal_with`); `None` past the table.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(crate) unsafe fn super_parts_in_lane(cx: *mut LaneContext, index: usize) -> Option<[WideInstruction; 4]> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    code.superinstructions.get(index).map(|site| site.wide)
}

/// A `SuperConstArrayObject2` record's object: a copy of the constant array, the two
/// fused `Binary` results, and the literal holding them, with each part's result in its
/// register as the general path leaves it. `None` before any guest code could run: an
/// operation on an object, or one that throws. The hand body a returning pair's staged row
/// (`returning_row`) misses to (`returning_miss`); a non-returning pair's miss is the general step.
///
/// Out of line: its general binary result returns through a stack slot, which would cost a
/// handler inlining it the tail call to the next record.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(never)]
unsafe fn literal_value_in_lane(
    cx: *mut LaneContext,
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
    let array_value = vm.allocate_array(quench_runtime::tier_surface::ArrayObject::new(Vm::empty_object(vm.array_proto), elements));
    r.set(array.result_register(), array_value);
    for part in [first, second] {
        let left = operand_value_in_lane(cx, r, part.operand_b().0)?;
        let right = operand_value_in_lane(cx, r, part.operand_c().0)?;
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
/// As for `field_value_in_lane`.
#[inline(always)]
unsafe fn single_field_base_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: u16,
) -> Option<Value> {
    if quench_runtime::bytecode::FieldBase(base).local_index().is_some() {
        return None;
    }
    if base == quench_runtime::bytecode::FieldBase::NESTED {
        return None;
    }
    match quench_runtime::bytecode::FieldBase(base).register_index() {
        Some(register) => Some(r.get(register)),
        None => unsafe { fixed_this_in_lane(cx, r) },
    }
}

/// `this` where the lane holds it or the frame fixed it; `None` for a lexical `this`, which
/// the general lookup (`this_in_lane`) resolves with a call. Without that call, a handler's
/// fast path keeps its values in registers no callee preserves.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn fixed_this_in_lane(cx: *mut LaneContext, r: Registers) -> Option<Value> {
    let this = r.this();
    if !this.is_deleted() {
        return Some(this);
    }
    let frame = (unsafe { vm(cx) }).frames.get(unsafe { (*cx).frame() })?;
    frame.fixed_this.then_some(frame.this)
}

/// `this` as the context holds it, for a record whose cached hit answers only an object:
/// `DELETED` where the frame's binding decides it (`LaneContext::this`), which no cache entry
/// answers, so the record declines to its general form, which resolves the binding
/// (`fixed_this_in_lane`, `this_in_lane`). One load, with no test of its own.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn held_this_in_lane(_cx: *mut LaneContext, r: Registers) -> Value {
    r.this()
}

/// The base of a single-base record whose handler was chosen for it (`single_base_handler`):
/// `this` when `THIS`, else the register the field names.
///
/// # Safety
/// As for `single_field_base_in_lane`; the record's base is `this` exactly when `THIS`.
#[inline(always)]
unsafe fn known_field_base_in_lane<const THIS: bool>(
    cx: *mut LaneContext,
    r: Registers,
    base: u16,
) -> Option<Value> {
    if THIS {
        debug_assert_eq!(base, quench_runtime::bytecode::FieldBase::THIS.0);
        // Every caller reads a cached object field of it, which declines `DELETED`.
        Some(unsafe { held_this_in_lane(cx, r) })
    } else {
        debug_assert!(base < quench_runtime::bytecode::FieldBase::NESTED);
        Some(r.get(base))
    }
}

/// The instantiation of a single-base read handler that a record of base `base` takes: the
/// one for `this` or the one for a register. A nested record, which names no single base,
/// takes `nested`.
fn single_base_handler(base: u16, this: Handler, register: Handler, nested: Handler) -> Handler {
    match base {
        base if base == quench_runtime::bytecode::FieldBase::THIS.0 => this,
        base if quench_runtime::bytecode::FieldBase(base).register_index().is_some() => register,
        // A nested record names no single base, and a local base (`FieldBase::local`) is no
        // register the single-base handlers could read.
        _ => nested,
    }
}

/// Whether a field record's base is one the single-base handlers read
/// (`known_field_base_in_lane`): `this` or a register, not a nested chain or a local slot.
fn is_single_field_base(base: u16) -> bool {
    base == quench_runtime::bytecode::FieldBase::THIS.0 || quench_runtime::bytecode::FieldBase(base).register_index().is_some()
}

use quench_runtime::staged::HINTED_FORM;

script_handler! {
    /// the own-field row (`own_formed_handler`) past its record's hint: the site's primary own entry, whose hit
    /// becomes the hint; any other read goes through `row_get_field_miss`.
    #[inline(never)]
    fn row_get_field_own_refresh<THIS: bool, BOUND: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let Some((object, value)) = (unsafe { known_field_base_in_lane::<THIS>(cx, r, i.b) })
            .and_then(|object| own_entry_hit(vm, code, object, i.c).map(|value| (object, value)))
        else {
            sibling!(row_get_field_miss, r, ip, Carried::script_in(cx, view))
        };
        if !BOUND {
            // SAFETY: a successful direct-object lookup proves the range is reserved.
            unsafe { (*cx).binding.object_base = vm.heap_reads().object_record_base().unwrap_unchecked() };
        }
        // A register read probes for an own constant (an enum member, a namespace export): the
        // first object it meets stays unhinted, so the next read comes back here; the same
        // object again is held as the read's value under a watch on its values.
        if !THIS && !holds_loop_site(ip) && vm.probes_own_constant(ip.addr()) {
            let first = !vm.own_constant_probed(ip.addr());
            if let Some(index) = vm.probe_own_constant(ip.addr(), i.imm(), object, value, NO_CALL_LINK) {
                set_record_hint(ip, OWN_CONSTANT_HINT, index);
                requicken(ip, generated_row!(FIELD_LEAF_READ, ACC_NONE, OWN_CONSTANT_REGISTER));
                r.set(i.a, value);
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
            if first {
                r.set(i.a, value);
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
        }
        // The hit proved the entry an own one of the receiver's shape.
        let hint = vm.form_hint(vm.field_cache_entry(i.c), object);
        set_field_hint(ip, hint);
        requicken(ip, own_formed_handler::<THIS>(Some(hint.receiver)));
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// The value the primary own entry of site `site` reads in `object`, in whatever storage form
/// the object keeps it: the site row (`primary_own_field_hit`) reads arena runs and packed
/// values; an object whose values a collection narrowed (`NARROW_FORM`) is read through the
/// entry formed to its word (`form_hint`), so its record settles on the narrow handler instead
/// of missing on every read.
#[inline(always)]
fn own_entry_hit(vm: &mut Vm, code: &ResidualProgram, object: Value, site: u16) -> Option<Value> {
    if let Some(value) = vm.primary_own_field_hit(code, object, site) {
        return Some(value);
    }
    let entry = vm.field_cache_entry(site);
    let receiver = vm.heap_reads().record_object_value(object)?;
    if entry.holder != quench_runtime::vm::NO_FIELD_HOLDER || receiver.exact_shape() != entry.receiver {
        return None;
    }
    vm.own_field_hint_hit::<HINTED_FORM>(vm.form_hint(entry, object), receiver)
}

/// The opcode of the catalog's field leaf read (`Traits::FIELD_LEAF`), whose generated row
/// (`generated_row`) an own-field read's record runs once its hint is bound (`own_formed_handler`).
const FIELD_LEAF_READ: u16 = {
    let mut index = 0;
    loop {
        match Op::from_index(index) {
            Some(op) if op.traits().has(quench_runtime::bytecode::Traits::FIELD_LEAF) => break index as u16,
            Some(_) => index += 1,
            None => panic!("the catalog names a field leaf read"),
        }
    }
};

/// The site fact of a keyed access of a typed array's element (`requicken_typed`): the row proves
/// the array's view for the element kind its record's immediate holds (`LaneFrame::kept_view`),
/// and reads or stores the element inline; a miss takes the host path (`row_index_read_miss`,
/// `row_index_write`), which records the kind again.
// merge: HEAD's typed element rows are restored: the shared `requicken_typed` and `row_typed_element` use them.
pub(crate) const TYPED_SITE: u8 = own_site(false, OWN_FORMS.len() + 26);

/// The generated typed keyed read (`generated_row` of `ELEMENT_READ`, `TYPED_SITE`).
const fn element_typed_read_row() -> Handler { generated_row!(ELEMENT_READ, ACC_NONE, TYPED_SITE) }

/// The generated typed keyed write (`generated_row` of `ELEMENT_WRITE`, `TYPED_SITE`).
const fn element_typed_write_row() -> Handler { generated_row!(ELEMENT_WRITE, ACC_NONE, TYPED_SITE) }

/// The opcode of the catalog's named field store of `receiver`, strict or not (`Op::field_store`),
/// whose generated row (`generated_row`) a store record runs: its hint as a layout store.
const fn field_store_opcode(receiver: StoreReceiver, strict: bool) -> u16 {
    let mut index = 0;
    loop {
        match Op::from_index(index) {
            Some(op) => match op.field_store() {
                Some(FieldStore { receiver: found, strict: s })
                    if s == strict && matches!((found, receiver), (StoreReceiver::This, StoreReceiver::This)
                        | (StoreReceiver::Register, StoreReceiver::Register)) => break index as u16,
                _ => index += 1,
            },
            None => panic!("the catalog names every field store"),
        }
    }
}
const FIELD_STORE: u16 = field_store_opcode(StoreReceiver::Register, false);
const FIELD_STORE_STRICT: u16 = field_store_opcode(StoreReceiver::Register, true);
const THIS_FIELD_STORE: u16 = field_store_opcode(StoreReceiver::This, false);
const THIS_FIELD_STORE_STRICT: u16 = field_store_opcode(StoreReceiver::This, true);

/// The generated own-field store (`generated_row` of the store opcode) of a store record whose
/// receiver and strictness `store` names.
const fn field_store_row(store: FieldStore) -> Handler { field_store_row_at::<FLAG_UNREAD>(store) }

/// `field_store_row` at site fact `FLAG`: `ADD_SITE` is the row of a store that may add its property.
const fn field_store_row_at<const FLAG: u8>(store: FieldStore) -> Handler {
    match (store.receiver, store.strict) {
        (StoreReceiver::Register, false) => generated_row!(FIELD_STORE, ACC_NONE, FLAG_UNREAD),
        (StoreReceiver::Register, true) => generated_row!(FIELD_STORE_STRICT, ACC_NONE, FLAG_UNREAD),
        (StoreReceiver::This, false) => generated_row!(THIS_FIELD_STORE, ACC_NONE, FLAG_UNREAD),
        (StoreReceiver::This, true) => generated_row!(THIS_FIELD_STORE_STRICT, ACC_NONE, FLAG_UNREAD),
    }
}

/// The storage forms an own-field read's hint fixes, by index: the generated row's site fact
/// (`own_site`) names one, so the row folds the layout it reads (`concrete_hinted_own_field_hit`).
/// `HINTED_FORM` reads the hint layout's own bits.
const OWN_FORMS: [u32; 9] = {
    use quench_runtime::staged::TAGGED_SLOTS;
    use quench_runtime::value_vec::{EXTRAS_FORM, INLINE_FORM, NARROW_FORM};
    [HINTED_FORM, 0, EXTRAS_FORM, INLINE_FORM, EXTRAS_FORM | INLINE_FORM, NARROW_FORM,
        TAGGED_SLOTS, EXTRAS_FORM | TAGGED_SLOTS, EXTRAS_FORM | INLINE_FORM | TAGGED_SLOTS]
};

/// The site fact (`generated_row`'s `FLAG`) of an own-field read of `this` (else of a register) whose
/// hint fixes form `OWN_FORMS[form]`. Zero is no site: the row reads it as unread.
const fn own_site(this: bool, form: usize) -> u8 { 1 + (form as u8) * 2 + this as u8 }

/// The site facts of a generated read through a prototype hint (`IcOp::Held`), of `this` and of
/// a register: past every own-field site.
pub(crate) const HELD_SITE_THIS: u8 = own_site(true, OWN_FORMS.len());
pub(crate) const HELD_SITE_REGISTER: u8 = own_site(false, OWN_FORMS.len());

/// The site facts of the held read of a receiver other than a direct object (an array, function
/// or ordinary builtin, as `a.push` or `f.call`), which a held site's miss takes: its object part
/// is resolved out of line (`RecordPlan::other`).
/// One instantiation for both bases: the base is a runtime test (`runtime_base`), as these rows are cold.
pub(crate) const HELD_OTHER_THIS: u8 = own_site(false, OWN_FORMS.len() + 1);
pub(crate) const HELD_OTHER_REGISTER: u8 = HELD_OTHER_THIS;

/// The site fact of a string register's read through a `String.prototype` hint
/// (`IcOp::HeldString`), as `s.charCodeAt`.
pub(crate) const STRING_SITE: u8 = own_site(false, OWN_FORMS.len() + 2);

/// Whether site `site` is a read through a prototype hint.
pub(crate) const fn held_site(site: u8) -> bool {
    site == HELD_SITE_THIS || site == HELD_SITE_REGISTER || held_other_site(site)
}

/// The site fact of a held read at a site whose receivers live outside the object records, as
/// `a.push` or `f.call` (`FieldSiteTier::Holder` of a cell): the held-other read from the first,
/// whose miss takes the site's holder entry (`row_get_field_holder_cell_refresh`).
pub(crate) const HELD_CELL_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 3);

/// The site facts of a read through the record's inherited hint, a method beyond the immediate
/// prototype (`FieldSiteTier::Inherited`): of a direct object, then of any receiver's object
/// part, each of `this` and of a register.
pub(crate) const INHERITED_THIS: u8 = own_site(true, OWN_FORMS.len() + 4);
pub(crate) const INHERITED_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 4);
pub(crate) const INHERITED_OTHER_THIS: u8 = own_site(false, OWN_FORMS.len() + 5);
pub(crate) const INHERITED_OTHER_REGISTER: u8 = INHERITED_OTHER_THIS;

/// Whether site `site` reads `this` or a register as its record says at run time, not as a
/// site fact: a cold row kept to one instantiation for both bases.
pub(crate) const fn runtime_base(site: u8) -> bool {
    site == HELD_OTHER_THIS || site == INHERITED_OTHER_THIS || entry_site(site)
}

/// The site fact of an own read of a receiver outside the object records, as a function's static
/// field (`Strength.REQUIRED`): the record's hint is the receiver's layout id and slot
/// (`Vm::cell_own_hint`), guarded by one compare of the function's record (`IcOp::GuardLayout`);
/// one instantiation per base: the register one (`A.x`) is the common read, and a fixed base
/// saves the base test (5 Ir per read).
pub(crate) const CELL_OWN_THIS: u8 = own_site(true, OWN_FORMS.len() + 6);
pub(crate) const CELL_OWN_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 6);
/// The cell-own sites whose hint layout keeps its slots in an arena run of tagged words, where
/// float slots are off for the process (`switches::float_slots`): the row reads the slot
/// untested and undecoded, as an own-field site of that form does (`OWN_FORMS`), so the read
/// is the slot load past the layout guard.
pub(crate) const CELL_RUN_THIS: u8 = own_site(true, OWN_FORMS.len() + 7);
pub(crate) const CELL_RUN_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 7);

/// The site fact of a `GetField` of a local slot's row (`local_field_handler`): an own-field read of a
/// plain local, whose hint the record binds like an own read's. Own-form index 30, a
/// free one below the returning sites.
pub(crate) const LOCAL_FIELD_SITE: u8 = own_site(false, OWN_FORMS.len() + 30);
/// The site fact of a register read of one object's own constant (`IcOp::OwnConstant`): the
/// record's hint names an own-constant hint (`OWN_CONSTANT_HINT`, `Vm::own_constant_operands`).
pub(crate) const OWN_CONSTANT_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 8);
pub(crate) const fn own_constant_site(site: u8) -> bool { site == OWN_CONSTANT_REGISTER }

/// The site facts of a register's `length`: an array's (`IcOp::ArrayLength`) and a string's
/// (`IcOp::StringLength`); a miss takes `row_length_other`.
pub(crate) const ARRAY_LENGTH_SITE: u8 = own_site(false, OWN_FORMS.len() + 9);
pub(crate) const STRING_LENGTH_SITE: u8 = own_site(false, OWN_FORMS.len() + 10);
pub(crate) const fn length_site(site: u8) -> bool { site == ARRAY_LENGTH_SITE || site == STRING_LENGTH_SITE }

/// The site facts of a read the VM's geometry or length answers from the receiver's word, which no
/// record addresses (`FieldPlan::Length`, `FieldPlan::Geometry`): a register's `length` of an
/// arguments object, a user function or a typed view (`TYPED_LENGTH_SITE`, a miss to the array row),
/// and any geometry name of a typed view, DataView, Map or Set (`GEOMETRY_SITE`, a miss to the
/// general read).
pub(crate) const TYPED_LENGTH_SITE: u8 = own_site(false, OWN_FORMS.len() + 20);
pub(crate) const GEOMETRY_SITE: u8 = own_site(false, OWN_FORMS.len() + 21);
pub(crate) const fn host_read_site(site: u8) -> bool { site == TYPED_LENGTH_SITE || site == GEOMETRY_SITE }

/// The site fact of a number receiver's read through a `Number.prototype` hint (`IcOp::HeldNumber`),
/// as `n.toFixed`: own-form index 14.
pub(crate) const NUMBER_SITE: u8 = own_site(false, OWN_FORMS.len() + 14);

/// The site facts of a read that met a getter whose hint returns an own field of the receiver
/// (`IcOp::GetterOwn`, `Vm::getter_hint_operands`), of `this` and of a register; a miss takes the
/// getter's refresh, which makes the hint (`row_get_field_getter_refresh`).
pub(crate) const GETTER_THIS: u8 = own_site(true, OWN_FORMS.len() + 15);
pub(crate) const GETTER_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 15);
pub(crate) const fn getter_site(site: u8) -> bool { site == GETTER_THIS || site == GETTER_REGISTER }

/// The site fact of a nested read `a.b.c` of two own entries (`get_field_nested_miss`'s steps):
/// the record's hint is the first step's entry, the site's second entry the second; the row runs
/// them as a chain of two own hops (`RecordPlan::nested`), and its miss takes the hand read that
/// refreshes the hint.
pub(crate) const NESTED_SITE: u8 = own_site(false, OWN_FORMS.len() + 16);

/// The site facts of the general read (`FieldPlan::Host`): the record names no plan the row
/// runs, so its hit is the VM's cached read of the atom through the site's cache
/// (`general_field_read`, the value `lane_get_field` made), of `this` and of a register. Its miss
/// is `row_get_field_general`, the general read's activation path.
pub(crate) const GENERAL_THIS: u8 = own_site(true, OWN_FORMS.len() + 18);
pub(crate) const GENERAL_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 18);
pub(crate) const fn general_site(site: u8) -> bool { site == GENERAL_THIS || site == GENERAL_REGISTER }

/// The generated general read of a `GetField` by its base: `GENERAL_THIS` for `this`,
/// `GENERAL_REGISTER` for a register, and `row_get_field_general` for a nested or local base.
fn general_handler(base: u16) -> Handler {
    if base == quench_runtime::bytecode::FieldBase::THIS.0 {
        generated_row!(FIELD_LEAF_READ, ACC_NONE, GENERAL_THIS)
    } else if quench_runtime::bytecode::FieldBase(base).register_index().is_some() {
        generated_row!(FIELD_LEAF_READ, ACC_NONE, GENERAL_REGISTER)
    } else {
        row_get_field_general
    }
}

/// The VM's general read of `atom` of `object` through cache `site`, as the row's host path
/// (`generated_row`'s `GENERAL_*` answer): a register's hint answers first (`primary_record_holder_field_hit`,
/// `proved_inherited_field_hit`), then the caches' probe (`field_step_in_lane`), with `record` the
/// instruction's address for a private name's site. `None` leaves to `row_get_field_general`.
#[inline(always)]
pub(crate) fn general_field_read(context: *mut (), record: usize, register: bool, object: Value, atom: u32,
    site: u16) -> Option<Value> {
    let cx = context.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let quick = if register && vm.specialized_operations_enabled(code) {
        vm.primary_record_holder_field_hit(code, object, site).or_else(|| vm.proved_inherited_field_hit(code, object, site))
    } else {
        None
    };
    quick.or_else(|| field_step_in_lane(cx, record, code, object, atom, site))
}

/// Whether the nested site `i` reads two ordinary own entries, which `NESTED_SITE`'s row runs; a
/// `length` step (an array's or a string's own `length`) or a single step stays with
/// `get_field_nested_miss`.
fn nested_row_site(vm: &Vm, code: &ResidualProgram, i: &LaneInstruction) -> bool {
    code.field_sites.get(i.imm() as usize).is_some_and(|site| site.second.is_some_and(|(atom, _)| atom != vm.length_atom))
}

/// The site fact of a named store that may add its property (`StorePlan::Add`, `RecordPlan::own_add`):
/// its row runs the VM's adds, and a refused add takes the general cached write.
pub(crate) const ADD_SITE: u8 = own_site(false, OWN_FORMS.len() + 17);

/// The site fact of a named store whose own hint is a narrow run (`StorePlan::Hinted`, `narrow`):
/// the row stores the value's narrow form in place (`hinted_narrow_field_store_hit`); a refusal
/// takes the VM's own-field store, and a store whose object is no longer narrow moves back.
pub(crate) const NARROW_SITE: u8 = own_site(false, OWN_FORMS.len() + 22);

/// The site fact of a named store on a runtime whose arena keeps slot representations
/// (`Heap::tracks_slot_representations`): the row stores through the record's hint in place of
/// the untracked own store (`StorePlan::Hinted`, `hinted_primary_field_store_hit::<true>`).
pub(crate) const TRACKED_SITE: u8 = own_site(false, OWN_FORMS.len() + 23);

/// The site fact of a named store past one shape (`StorePlan::Polymorphic`): the record's hint,
/// the site's polymorphic entries, then the proved add, all inside the row; a refusal takes
/// `GENERAL_SITE`'s activation path.
pub(crate) const POLY_SITE: u8 = own_site(false, OWN_FORMS.len() + 24);

/// The site fact of a named store the general cached write completes (`StorePlan::Host`): the VM's
/// cached store or add inside the row, a private name's or a refusal's activation path after it
/// (`row_set_field_miss_path`).
pub(crate) const GENERAL_SITE: u8 = own_site(false, OWN_FORMS.len() + 25);
/// The site facts of a field read whose value is the callee of a Math call the record after it
/// makes (`native_calls::install_math_field_call`): one argument, a register second argument,
/// and a constant second argument. The row runs the call past its read (`math_row_resume`), the
/// function's identity read by the VM (`PropertyFrame::math_call`).
pub(crate) const MATH_CALL_SITE: u8 = own_site(false, OWN_FORMS.len() + 44);
pub(crate) const MATH_CALL2_SITE: u8 = own_site(false, OWN_FORMS.len() + 45);
pub(crate) const MATH_CALL2_CONST_SITE: u8 = own_site(false, OWN_FORMS.len() + 46);

/// The site fact of a nested read that stores its value to `this` (`this.a = this.b.c`), the
/// read's flag `SET_THIS_REGISTER` its row's store (`PropertyFrame::sink_store`).
pub(crate) const SINK_SITE: u8 = own_site(false, OWN_FORMS.len() + 47);

/// Whether site `site` is a field read of a Math call's callee (`MATH_CALL_SITE`, `MATH_CALL2_SITE`,
/// `MATH_CALL2_CONST_SITE`).
pub(crate) const fn math_site(site: u8) -> bool {
    site == MATH_CALL_SITE || site == MATH_CALL2_SITE || site == MATH_CALL2_CONST_SITE
}

/// The number of arguments the Math call of math site `site` passes.
pub(crate) const fn math_arity(site: u8) -> usize {
    if site == MATH_CALL_SITE { 1 } else { 2 }
}

/// Whether site `site` is a sinking read (`SINK_SITE`).
pub(crate) const fn sink_site(site: u8) -> bool { site == SINK_SITE }

/// The store `(atom, cache)` of the sinking read `i` (`SINK_SITE`): its site's sink.
#[inline(always)]
pub(crate) fn sink_operands(cx: *mut LaneContext, i: &LaneInstruction) -> Option<(quench_runtime::bytecode::Atom, u16)> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    code.field_sites.get(i.imm() as usize)?.sink
}

/// The site facts of the generated entry reads (`FieldPlan::Hinted`, `FieldPlan::Entry`): each
/// one hit shape of the VM's entry lookups, whose host call the row makes (`field_entry_read`).
/// Of `this` and of a register alike (`runtime_base`).
/// The polymorphic read: the record's own hint, then the site's inline entries (`row_polymorphic_miss` past them).
pub(crate) const POLYMORPHIC_SITE: u8 = own_site(false, OWN_FORMS.len() + 38);
/// The site's hashed entries past the inline ones (`Vm::polymorphic_field_hit`), the polymorphic read's miss.
pub(crate) const HASHED_SITE: u8 = own_site(false, OWN_FORMS.len() + 39);
/// The site's primary holder entry, of a direct object or of any receiver in a heap cell (`Vm::primary_holder_field_hit`,
/// `Vm::cell_holder_field_hit`), which holds no hint.
pub(crate) const HOLDER_SITE: u8 = own_site(false, OWN_FORMS.len() + 40);
/// The site's inherited entry walked past the immediate prototype (`Vm::inherited_field_entry_hit`), whose answer
/// becomes the record's inherited hint.
pub(crate) const INHERITED_WALK_SITE: u8 = own_site(false, OWN_FORMS.len() + 41);

/// Whether site `site` reads through a VM entry lookup (`FieldPlan::Entry`, `FieldPlan::Hinted`).
pub(crate) const fn entry_site(site: u8) -> bool {
    site == POLYMORPHIC_SITE || site == HASHED_SITE || site == HOLDER_SITE || site == INHERITED_WALK_SITE
}

/// The entry lookup a non-polymorphic entry site makes (`EntryKind`); the polymorphic site's own
/// plan is `FieldPlan::Hinted`, whose entry is `EntryKind::Polymorphic` too.
pub(crate) const fn entry_kind(site: u8) -> quench_runtime::staged::property::EntryKind {
    use quench_runtime::staged::property::EntryKind;
    if site == POLYMORPHIC_SITE { EntryKind::Polymorphic }
    else if site == HASHED_SITE { EntryKind::Hashed }
    else if site == HOLDER_SITE { EntryKind::Holder }
    else { EntryKind::Inherited }
}

/// The site facts of a private name's read of `this` and of a register (`FieldPlan::Private`):
/// the record's private field at its own address (`LaneFrame::private_value`). A method, accessor
/// or missing field takes `get_private_field_miss`, the general read.
pub(crate) const PRIVATE_FIELD_THIS: u8 = own_site(true, OWN_FORMS.len() + 48);
pub(crate) const PRIVATE_FIELD_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 48);
pub(crate) const fn private_field_site(site: u8) -> bool {
    site == PRIVATE_FIELD_THIS || site == PRIVATE_FIELD_REGISTER
}

/// The site facts of a read the VM's cache answers, of `this` and of a register (`FieldPlan::Host`):
/// the cached read of the site's atom (`LaneFrame::cached_read`). A miss takes `lane_get_field`,
/// the general read, which runs the getter and the data lookups.
pub(crate) const CACHED_FIELD_THIS: u8 = own_site(true, OWN_FORMS.len() + 42);
pub(crate) const CACHED_FIELD_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 42);
pub(crate) const fn cached_field_site(site: u8) -> bool {
    site == CACHED_FIELD_THIS || site == CACHED_FIELD_REGISTER
}

/// The site facts of a returning own-field read (`RETURNING_FIELD_OWN`'s row, k 50 up: the topmost site facts): the record's own
/// hint, for `this` and a register, at each leaf form the record returns from (`LeafForm`). The
/// row completes the return with its leaf form's completion, so the leaf forms are const site
/// facts of one family: `returning_site(this, leaf)`.
const RETURNING_FORM: usize = OWN_FORMS.len() + 50;
const RETURNING_BASE: u8 = own_site(false, RETURNING_FORM);
pub(crate) const fn returning_site(this: bool, leaf: u8) -> u8 { own_site(this, RETURNING_FORM) + 2 * leaf }
pub(crate) const fn returning_field_site(site: u8) -> bool { site >= RETURNING_BASE }
/// Saturating: a const branch for a site of another family still instantiates the call.
pub(crate) const fn returning_leaf(site: u8) -> u8 { site.saturating_sub(RETURNING_BASE) / 2 }

/// Whether `handler` is the returning own-field row of no leaf form, at `this` or a register
/// (`returning_site`): the leaf call's reads recognise it as the own read it is.
fn returning_own_row(handler: *const ()) -> bool {
    handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(true, NOT_LEAF)) as *const ()
        || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(false, NOT_LEAF)) as *const ()
}

pub(crate) const fn cell_own_site(site: u8) -> bool {
    site == CELL_OWN_THIS || site == CELL_OWN_REGISTER || site == CELL_RUN_THIS || site == CELL_RUN_REGISTER
}

/// Whether site `site` reads through the record's inherited hint; of any receiver (`other`).
pub(crate) const fn inherited_site(site: u8) -> bool {
    site == INHERITED_THIS || site == INHERITED_REGISTER || inherited_other_site(site)
}
pub(crate) const fn inherited_other_site(site: u8) -> bool { site == INHERITED_OTHER_THIS || site == INHERITED_OTHER_REGISTER }

/// Whether held site `site` reads any receiver's object part, not a direct object's alone.
pub(crate) const fn held_other_site(site: u8) -> bool {
    site == HELD_OTHER_THIS || site == HELD_OTHER_REGISTER || site == HELD_CELL_REGISTER
}

/// Whether own-field or held site `site` reads `this`.
pub(crate) const fn own_site_this(site: u8) -> bool { site.wrapping_sub(1) & 1 == 1 }

/// The storage form own-field site `site` fixes (`OWN_FORMS`); `HINTED_FORM` for none.
pub(crate) const fn own_site_form(site: u8) -> u32 {
    let index = (site.wrapping_sub(1) / 2) as usize;
    if index < OWN_FORMS.len() { OWN_FORMS[index] }
    else if index == OWN_FORMS.len() + 7 { quench_runtime::staged::TAGGED_SLOTS }
    else { HINTED_FORM }
}

/// Whether the word own-field site `site` reads is a slot word to decode (`decode_staged_word`):
/// every form but a fixed tagged one and packed or narrow values, which hold no raw double.
pub(crate) const fn own_site_decodes(site: u8) -> bool {
    if held_site(site) || site == STRING_SITE || site == NUMBER_SITE || inherited_site(site) || own_constant_site(site) || length_site(site)
        || host_read_site(site) || private_field_site(site) || cached_field_site(site) { return false; }
    let form = own_site_form(site);
    form == HINTED_FORM || (form & quench_runtime::staged::TAGGED_SLOTS == 0 && form != quench_runtime::value_vec::INLINE_FORM
        && form != quench_runtime::value_vec::NARROW_FORM)
}

/// Every generated own-field read: `$each!(site)` for each site, `this` and register alike.
macro_rules! own_sites {
    ($each:ident) => {
        [$each!(own_site(true, 0)), $each!(own_site(false, 0)), $each!(own_site(true, 1)), $each!(own_site(false, 1)),
            $each!(own_site(true, 2)), $each!(own_site(false, 2)), $each!(own_site(true, 3)), $each!(own_site(false, 3)),
            $each!(own_site(true, 4)), $each!(own_site(false, 4)), $each!(own_site(true, 5)), $each!(own_site(false, 5)),
            $each!(own_site(true, 6)), $each!(own_site(false, 6)), $each!(own_site(true, 7)), $each!(own_site(false, 7)),
            $each!(own_site(true, 8)), $each!(own_site(false, 8))]
    };
}

/// The generated own-field read (`generated_row` at an `own_site`) for the base `THIS` names and the
/// storage form the layout id `hint`'s form bits name: an arena run, extras or packed inline values,
/// whose layout it then reads untested; `None` for the form of no particular hint.
fn own_formed_handler<const THIS: bool>(hint: Option<u32>) -> Handler {
    use quench_runtime::value_vec::{EXTRAS_FORM, INLINE_FORM, NARROW_FORM};
    // A narrow run's hint names neither inline values nor extras: those tests fold away and
    // the read takes the run's halves. Packed and narrow slots never hold a raw double; the
    // other forms skip the decode where float slots are off for the process.
    let tagged = !quench_runtime::vm::switches::float_slots();
    // The hint is a layout id; its form bits are its entry's (`layouts::auxiliary`).
    let form = match hint.map(quench_runtime::tier_surface::layouts::auxiliary) {
        None => 0,
        Some(hint) if hint & NARROW_FORM != 0 => 5,
        Some(hint) => match (hint & (INLINE_FORM | EXTRAS_FORM), tagged) {
            (0, true) => 6,
            (0, false) => 1,
            (EXTRAS_FORM, true) => 7,
            (EXTRAS_FORM, false) => 2,
            (INLINE_FORM, _) => 3,
            (_, true) => 8,
            (_, false) => 4,
        },
    };
    macro_rules! row { ($site:expr) => { generated_row!(FIELD_LEAF_READ, ACC_NONE, $site) as Handler } }
    own_sites!(row)[form * 2 + usize::from(!THIS)]
}

/// Makes the own field entry `entry` the hint of the field read/store record at `ip`
/// (`LaneInstruction::field_hint`). A held loop header's `call_link` names its site
/// (`header_site_in_lane`), and such a record keeps no hint: an empty hint's link is
/// `NO_CALL_LINK`, a held one's shape a recorded one.
#[inline(always)]
fn set_field_hint(ip: Ip, entry: quench_runtime::vm::FieldCache) {
    set_record_hint(ip, entry.receiver, entry.slot);
}

/// Makes inherited entry `index` the hint of the `GetField` record at `ip`
/// (`Vm::hinted_inherited_field_hit`), where `call_link` can name it.
#[inline(always)]
fn set_inherited_hint(ip: Ip, index: u32) {
    if let Ok(index) = u16::try_from(index)
        && index != NO_CALL_LINK
    {
        set_record_hint(ip, INHERITED_HINT, index);
    }
}

/// Makes the prototype's answer `hold` keeps (`Vm::hold_holder_hint`, `Vm::hold_string_hint`)
/// the hint of the `GetField` record at `ip`, in place of the one it holds, which `hold` takes.
/// Out of line, so the reads that keep their hint keep their registers.
#[cold]
#[inline(never)]
fn set_prototype_hint(ip: Ip, hold: impl FnOnce(Option<u16>) -> Option<u16>) {
    if holds_loop_site(ip) {
        return;
    }
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let owned = (unsafe { i.operand.field.shape } == HOLDER_HINT).then(|| i.call_link.get());
    if let Some(index) = hold(owned) {
        set_record_hint(ip, HOLDER_HINT, index);
    }
}

/// Writes a field record's hint, `shape` and `link`, unless it is a held loop header
/// (`holds_loop_site`).
#[inline(always)]
fn set_record_hint(ip: Ip, shape: u32, link: u16) {
    if !holds_loop_site(ip) {
        // SAFETY: as `requicken`; `ip` is a field read/store record of a live view.
        unsafe {
            (*ip.cast_mut()).operand.field.shape = shape;
            (*ip).call_link.set(link);
        }
    }
}

/// Whether the field record at `ip` is a held loop header's, whose `call_link` names its
/// site (`header_site_in_lane`), so it keeps no hint: an empty hint's link is `NO_CALL_LINK`,
/// and a held one's shape a recorded one, `INHERITED_HINT` or `HOLDER_HINT`.
#[inline(always)]
fn holds_loop_site(ip: Ip) -> bool {
    // SAFETY: `ip` is a field read/store record of a live view.
    let i = unsafe { &*ip };
    (unsafe { i.operand.field.shape }) == quench_runtime::vm::NO_FIELD_RECEIVER && i.call_link.get() != NO_CALL_LINK
}

/// The polymorphic read of `object` by the `GetField` record at `ip` (`RETURNING_FIELD_POLYMORPHIC`):
/// its hint, the last own entry that answered it, first, then the site's inline entries, whose own
/// hit becomes the hint; with whether a prototype's entry answered, which no hint holds.
#[inline(always)]
fn polymorphic_read(vm: &mut Vm, code: &ResidualProgram, ip: Ip, object: Value) -> Option<(Value, bool)> {
    use quench_runtime::vm::object::PolymorphicHit;
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let (value, hit) = vm.hinted_polymorphic_field_hit(code, object, i.c, i.field_hint())?;
    if let PolymorphicHit::Own(entry) = hit {
        set_field_hint(ip, vm.form_hint(entry, object));
    }
    Some((value, matches!(hit, PolymorphicHit::Holder(_))))
}

/// The polymorphic read of `object` by the `GetField` record at `ip` (`FieldPlan::Hinted`,
/// `POLYMORPHIC_SITE`), past the record's own hint: the site's inline entries, own or of the
/// immediate prototype (`hinted_polymorphic_field_hit`). An own hit becomes the record's hint; a
/// prototype's hit holds the prototype hint (`hold_holder_hint`), kept while its watch is live, and
/// a record whose hint is a prototype's first tests that hint (`holder_hint_hit`).
fn polymorphic_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    use quench_runtime::vm::object::PolymorphicHit;
    // SAFETY: the context's program outlives the lane run; `ip` is a live `GetField` record.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    let i = unsafe { &*ip };
    let holds_holder = unsafe { i.operand.field.shape } == HOLDER_HINT;
    if holds_holder && let Some(value) = vm.holder_hint_hit(i.call_link.get(), i.imm(), object) {
        return Some(value);
    }
    let (value, hit) = vm.hinted_polymorphic_field_hit(code, object, i.c, i.field_hint())?;
    match hit {
        PolymorphicHit::Own(entry) => {
            if !holds_holder {
                set_field_hint(ip, vm.form_hint(entry, object));
            }
        }
        PolymorphicHit::Holder(_) => {
            // A live hint another receiver of this site answers stays: rewriting it at every
            // receiver switch re-marked and re-watched the prototype each time (V8-v7 Richards:
            // 0.71M rewrites, 5% of its run).
            if !(holds_holder && vm.holder_hint_live(i.call_link.get())) {
                set_prototype_hint(ip, |owned| {
                    if owned.is_some_and(|index| vm.holder_hint_live(index)) {
                        return None;
                    }
                    vm.hold_holder_hint(owned, i.imm(), object, value, NO_CALL_LINK)
                });
            }
        }
        PolymorphicHit::Hint => {}
    }
    Some(value)
}

/// The hashed entries of the `GetField` record at `ip` (`HASHED_SITE`): the site's hashed entries
/// past the inline ones, own or of the immediate prototype (`Vm::polymorphic_field_hit`), as an AST
/// walk reads `type` of a node of any of dozens of shapes. Any other read takes `lane_get_field`.
fn hashed_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run; `ip` is a live `GetField` record.
    let code = unsafe { &*(*cx).code };
    let i = unsafe { &*ip };
    unsafe { vm(cx) }.polymorphic_field_hit(code, object, i.c)
}

/// The site's primary holder entry of `object` (`HOLDER_SITE`): a direct object's record
/// (`Vm::primary_holder_field_hit`), else a receiver any heap cell holds, such as a function whose
/// prototype holds the method (`Vm::cell_holder_field_hit`), as `f.apply`. Holds no hint.
fn holder_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    // SAFETY: as `hashed_entry`.
    let code = unsafe { &*(*cx).code };
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    if object.direct_object_index().is_some() {
        vm.primary_holder_field_hit(code, object, i.c)
    } else {
        vm.cell_holder_field_hit(code, object, i.c)
    }
}

/// The inherited entry of `object` walked past its immediate prototype (`INHERITED_WALK_SITE`):
/// the site's entries and their chain walk, which proves one again (`Vm::inherited_field_entry_hit`);
/// the entry that answered becomes the record's inherited hint.
fn inherited_entry(cx: *mut LaneContext, ip: Ip, object: Value) -> Option<Value> {
    // SAFETY: as `hashed_entry`.
    let code = unsafe { &*(*cx).code };
    let i = unsafe { &*ip };
    let (value, entry) = unsafe { vm(cx) }.inherited_field_entry_hit(code, object, i.c)?;
    set_inherited_hint(ip, entry);
    Some(value)
}

/// The VM's entry lookup `kind` answers for `object` at the row's record `ip` (`LaneFrame::field_entry`).
pub(crate) fn field_entry_read(context: *mut (), ip: Ip, object: Value,
    kind: quench_runtime::staged::property::EntryKind) -> Option<Value> {
    use quench_runtime::staged::property::EntryKind;
    let cx = context.cast::<LaneContext>();
    match kind {
        EntryKind::Polymorphic => polymorphic_entry(cx, ip, object),
        EntryKind::Hashed => hashed_entry(cx, ip, object),
        EntryKind::Holder => holder_entry(cx, ip, object),
        EntryKind::Inherited => inherited_entry(cx, ip, object),
    }
}

script_handler! {
    /// The polymorphic row's miss (`POLYMORPHIC_SITE`): a site whose entries moved to the hash
    /// takes the hashed row (`HASHED_SITE`) from now on; every other miss takes it as well, which
    /// reads the hash and misses to `lane_get_field`.
    #[inline(never)]
    fn row_polymorphic_miss<>(cx, r, ip, i, view) {
        if unsafe { vm(cx) }.field_site_hashed(i.c) {
            requicken(ip, generated_row!(FIELD_LEAF_READ, ACC_NONE, HASHED_SITE));
        }
        sibling!(generated_row!(FIELD_LEAF_READ, ACC_NONE, HASHED_SITE), r, ip, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `lane_get_field_holder` past its hint: the site's primary holder entry
    /// (`Vm::primary_record_holder_field_hit`), whose answer becomes the hint. Any other read
    /// takes the holder row (`HOLDER_SITE`).
    #[inline(never)]
    fn row_get_field_holder_refresh<THIS: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let Some((object, value)) = (unsafe { known_field_base_in_lane::<THIS>(cx, r, i.b) })
            .and_then(|object| vm.primary_record_holder_field_hit(code, object, i.c).map(|value| (object, value)))
        else {
            sibling!(generated_row!(FIELD_LEAF_READ, ACC_NONE, HOLDER_SITE), r, ip, Carried::script_in(cx, view))
        };
        set_prototype_hint(ip, |owned| vm.hold_holder_hint(owned, i.imm(), object, value, NO_CALL_LINK));
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `GetField` of a string primitive, which `String.prototype` answers through the
    /// site's cache (`Vm::string_prototype_field_hit`), a method read as `s.substring`, or
    /// its chain's data walk for an absent name, at a site `Vm::string_field_site_cacheable`
    /// admits. A miss moves the record on as `row_get_field_miss` decides.
    fn get_field_string_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // The site checks (`Vm::string_field_site_cacheable`) ran when the handler was
        // installed, so a hit reads the site's `String.prototype` entry directly; any other
        // read is `Vm::cached_string_field_hit`'s, whose miss records the site's entry.
        let Some(value) = (unsafe { single_field_base_in_lane(cx, r, i.b) })
            .filter(|object| vm.heap_reads().is_string(*object))
            .and_then(|object| {
                vm.string_prototype_field_hit(i.c)
                    .or_else(|| vm.cached_string_field_hit(unsafe { &*(*cx).code }, object, i.imm(), i.c))
            })
        else {
            sibling!(row_get_field_miss, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The generated number read (`generated_row` at `NUMBER_SITE`) past its hint: the site's `Number.prototype`
    /// entry (`Vm::number_prototype_field_hit`), whose answer becomes the hint. Any other read
    /// takes `row_get_field_miss`, which decides the record's tier.
    #[inline(never)]
    fn row_get_field_number_refresh<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let base = r.get(i.b);
        let Some(value) = base.as_number().is_some().then(|| vm.number_prototype_field_hit(i.c)).flatten() else {
            sibling!(row_get_field_miss, r, ip, Carried::script_in(cx, view))
        };
        set_prototype_hint(ip, |owned| vm.hold_number_hint(owned, i.imm(), value, NO_CALL_LINK));
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The generated string read (`generated_row` at `STRING_SITE`) past its hint: the site's `String.prototype` entry
    /// (`Vm::string_prototype_field_hit`), whose answer becomes the hint. Any other read
    /// takes `get_field_string_miss`.
    #[inline(never)]
    fn row_get_field_string_refresh<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let base = r.get(i.b);
        let Some(value) = vm.heap_reads().is_string(base).then(|| vm.string_prototype_field_hit(i.c)).flatten() else {
            sibling!(get_field_string_miss, r, ip, Carried::script_in(cx, view))
        };
        set_prototype_hint(ip, |owned| vm.hold_string_hint(owned, i.imm(), value, NO_CALL_LINK));
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The own-constant row's miss (`OWN_CONSTANT_REGISTER`): another object, or the object was
    /// written. The site never holds an own constant again and reads as its tier decides.
    /// Out of line, so the row's hit path makes no call and saves no register.
    #[inline(never)]
    #[cold]
    fn refuse_own_constant_row<>(cx, r, ip, i, view) {
        let _ = i;
        unsafe { vm(cx) }.refuse_own_constant(ip.addr());
        sibling!(row_get_field_miss, r, ip, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The generated held read at `HELD_CELL_REGISTER` past its hint: the site's holder entry
    /// (`Vm::cell_holder_field_hit`), whose answer becomes the hint. A miss takes
    /// the holder row (`HOLDER_SITE`), which moves the record on.
    #[inline(never)]
    fn row_get_field_holder_cell_refresh<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let base = r.get(i.b);
        let Some(value) = vm.cell_holder_field_hit(code, base, i.c) else {
            sibling!(generated_row!(FIELD_LEAF_READ, ACC_NONE, HOLDER_SITE), r, ip, Carried::script_in(cx, view))
        };
        set_prototype_hint(ip, |owned| vm.hold_holder_hint(owned, i.imm(), base, value, NO_CALL_LINK));
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// A `GetField`'s first run: a read of `name` or `length` can enter prototype lookup
    /// mode (`Vm::reads_function_info`), which only `lane_get_field`'s general probe
    /// prepares, so such a site keeps it; any other takes the own-field row (`own_formed_handler`).
    #[inline(never)]
    fn row_get_field_first<>(cx, r, ip, i, view) {
        if quench_runtime::bytecode::FieldBase(i.b).local_index().is_some() {
            if let Some(handler) = local_field_handler(cx, i) {
                requicken(ip, handler);
            }
            sibling!(row_get_field_general, r, ip, Carried::script_in(cx, view))
        }
        let vm = unsafe { vm(cx) };
        if vm.is_private_name(i.imm()) && i.b != quench_runtime::bytecode::FieldBase::NESTED {
            // The private field row of the base; a local base reads through the general private read.
            let private: Handler = single_base_handler(i.b,
                generated_row!(FIELD_LEAF_READ, ACC_NONE, PRIVATE_FIELD_THIS),
                generated_row!(FIELD_LEAF_READ, ACC_NONE, PRIVATE_FIELD_REGISTER),
                protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::GET_PRIVATE_FIELD as u16 }>());
            requicken(ip, private);
            sibling!(private, r, ip, Carried::script_in(cx, view))
        }
        // `name` reads take the own-field handlers: an own hit needs the receiver's exact shape
        // to hold `name` as a data slot the general read recorded, which a function's unprepared
        // `name` never is (it is no slot until `prepare_function_info_prototypes` makes it one),
        // and every miss still reaches the general read, which prepares it. `user.name` then
        // costs an own read, not the general probe.
        if vm.reads_function_info(i.imm()) && i.imm() == vm.length_atom {
            // A register's `length` is most often an array's, which no function information
            // can answer.
            let handler: Handler = if i.imm() == vm.length_atom
                && quench_runtime::bytecode::FieldBase(i.b).register_index().is_some()
                && i.b != quench_runtime::bytecode::FieldBase::NESTED
            {
                generated_row!(FIELD_LEAF_READ, ACC_NONE, ARRAY_LENGTH_SITE)
            } else {
                general_handler(i.b)
            };
            requicken(ip, handler);
            sibling!(handler, r, ip, Carried::script_in(cx, view))
        }
        // The generated own read of the form of no particular hint, which binds the lane's
        // object records on its first miss where they are not yet bound.
        let (this, register) = (own_formed_handler::<true>(None), own_formed_handler::<false>(None));
        let own = single_base_handler(i.b, this, register, row_get_field_miss);
        requicken(ip, own);
        sibling!(own, r, ip, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The generated `length` row's miss (`ARRAY_LENGTH_SITE`, `STRING_LENGTH_SITE`): an array in a
    /// form the row does not read (`Vm::array_length_value`), then a string primitive's length
    /// (`Vm::string_length`), whose site takes the string row. Any other receiver `length_read`
    /// answers (an arguments object's or a user function's own `length`, a typed view's geometry)
    /// takes the host read row (`TYPED_LENGTH_SITE`); the rest reads `row_length_plain`.
    #[inline(never)]
    fn row_length_other<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let receiver = r.get(i.b);
        if let Some(length) = vm.array_length_value(receiver) {
            r.set(i.a, length);
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        if let Some(length) = vm.string_length(receiver) {
            // A site that reads a string's length mostly reads strings, as a scan over one does.
            requicken(ip, generated_row!(FIELD_LEAF_READ, ACC_NONE, STRING_LENGTH_SITE));
            r.set(i.a, Value::length(length));
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        if let Some(length) = length_read(vm, receiver) {
            // A site that reads such a `length` mostly reads these receivers, as a loop over a typed array does.
            requicken(ip, generated_row!(FIELD_LEAF_READ, ACC_NONE, TYPED_LENGTH_SITE));
            r.set(i.a, length);
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        sibling!(row_length_plain, r, ip, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `row_length_other`'s last read: a plain object's own `length` data slot (`{ length: n }`), as
    /// the site's primary entry recorded it; a miss takes the general read, which prepares function
    /// info.
    #[inline(never)]
    fn row_length_plain<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let receiver = r.get(i.b);
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let Some(value) = vm.primary_own_field_hit(code, receiver, i.c) else {
            sibling!(row_get_field_general, r, ip, Carried::script_in(cx, view))
        };
        // A site that read a plain object's own `length` mostly reads such objects: it takes the
        // own-field handler, whose misses still reach the general read.
        if quench_runtime::bytecode::FieldBase(i.b).register_index().is_some() && i.b != quench_runtime::bytecode::FieldBase::NESTED {
            let own = own_formed_handler::<false>(None);
            requicken(ip, own);
        }
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// A miss of a single-entry read (the own-field row, `lane_get_field_holder`, `_inherited`): a nested record moves to
    /// `lane_get_field`, one whose site went polymorphic to `POLYMORPHIC_SITE`,
    /// one whose primary entry names a prototype holder to `lane_get_field_holder`, one with
    /// an inherited entry to the generated inherited read; the read itself takes
    /// `lane_get_field`.
    #[inline(never)]
    fn row_get_field_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let base = unsafe { single_field_base_in_lane(cx, r, i.b) };
        let string_base = base.is_some_and(|base| {
            (unsafe { vm(cx) }).heap_reads().is_string(base)
        });
        if let Some(handler) = local_field_handler(cx, i) {
            requicken(ip, handler);
            sibling!(row_get_field_general, r, ip, Carried::script(cx))
        }
        let geometry_base = base.is_some_and(|base| geometry_read_in_lane(unsafe { vm(cx) }, base, i.imm()).is_some());
        let handler: Option<Handler> = match field_site_tier_in_lane(unsafe { vm(cx) }, code, i) {
            // A site that read a typed array's, DataView's, Map's or Set's geometry mostly
            // reads such views, as a loop over one does: the host read row answers it first.
            _ if geometry_base => Some(single_base_handler(
                i.b,
                general_handler(i.b),
                generated_row!(FIELD_LEAF_READ, ACC_NONE, GEOMETRY_SITE),
                general_handler(i.b),
            )),
            // A string primitive reads `String.prototype`, whatever tier its entries name,
            // where the site admits that read; any other read of one is the general read's.
            _ if string_base && (unsafe { vm(cx) }).string_field_site_cacheable(code, i.imm()) => {
                Some(single_base_handler(
                    i.b,
                    protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_FIELD_STRING as u16 }>(),
                    generated_row!(FIELD_LEAF_READ, ACC_NONE, STRING_SITE),
                    protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_FIELD_STRING as u16 }>(),
                ))
            }
            _ if string_base => Some(general_handler(i.b)),
            // A number primitive reads `Number.prototype` where the site admits that read, as the
            // string arm does; a `this` or nested base takes the general read.
            _ if base.is_some_and(|base| base.as_number().is_some())
                && (unsafe { vm(cx) }).number_field_site_cacheable(code, i.imm()) =>
            {
                Some(single_base_handler(
                    i.b,
                    general_handler(i.b),
                    generated_row!(FIELD_LEAF_READ, ACC_NONE, NUMBER_SITE),
                    general_handler(i.b),
                ))
            }
            FieldSiteTier::Nested if nested_row_site(unsafe { vm(cx) }, code, i) => Some(generated_row!(FIELD_LEAF_READ, ACC_NONE, NESTED_SITE)),
            FieldSiteTier::Nested => Some(protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_FIELD_NESTED as u16 }>()),
            FieldSiteTier::General => Some(general_handler(i.b)),
            // A getter's own-field hint is the generated read (`GETTER_THIS`, `GETTER_REGISTER`); a
            // local base, which no row names, reads through the getter's activation.
            FieldSiteTier::Getter => Some(single_base_handler(
                i.b,
                generated_row!(FIELD_LEAF_READ, ACC_NONE, GETTER_THIS),
                generated_row!(FIELD_LEAF_READ, ACC_NONE, GETTER_REGISTER),
                protocol_handler::<{ quench_runtime::bytecode::FIELD_READ as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_FIELD_GETTER as u16 }>(),
            )),
            FieldSiteTier::Polymorphic if is_single_field_base(i.b) =>
                Some(generated_row!(FIELD_LEAF_READ, ACC_NONE, POLYMORPHIC_SITE) as Handler),
            FieldSiteTier::Polymorphic => Some(general_handler(i.b)),
            // A receiver outside the object records, such as a function, takes the cell lookup.
            FieldSiteTier::Holder
                if base.is_some_and(|object| object.direct_object_index().is_none()) =>
            {
                Some(if quench_runtime::bytecode::FieldBase(i.b).register_index().is_some() {
                    generated_row!(FIELD_LEAF_READ, ACC_NONE, HELD_CELL_REGISTER)
                } else {
                    general_handler(i.b)
                })
            }
            FieldSiteTier::Holder => Some(single_base_handler(
                i.b,
                generated_row!(FIELD_LEAF_READ, ACC_NONE, HELD_SITE_THIS),
                generated_row!(FIELD_LEAF_READ, ACC_NONE, HELD_SITE_REGISTER),
                general_handler(i.b),
            )),
            FieldSiteTier::Inherited => Some(single_base_handler(
                i.b,
                generated_row!(FIELD_LEAF_READ, ACC_NONE, INHERITED_THIS),
                generated_row!(FIELD_LEAF_READ, ACC_NONE, INHERITED_REGISTER),
                general_handler(i.b),
            )),
            // A receiver outside the object records, such as a function, reads its own slot by the
            // receiver's layout id, which the record's hint holds (a loop header's keeps none).
            FieldSiteTier::Own
                if base.is_some_and(|object| object.direct_object_index().is_none()) =>
            {
                match base.and_then(|object| (unsafe { vm(cx) }).cell_own_hint(object, i.c)) {
                    Some((layout, slot)) if !holds_loop_site(ip) => {
                        set_record_hint(ip, layout, slot);
                        // A tagged arena run is a fixed form: the row folds the layout's form test and the slot decode.
                        use quench_runtime::value_vec::{EXTRAS_FORM, INLINE_FORM, NARROW_FORM};
                        let run = quench_runtime::tier_surface::layouts::auxiliary(layout) & (EXTRAS_FORM | INLINE_FORM | NARROW_FORM) == 0
                            && !quench_runtime::vm::switches::float_slots();
                        Some(if run {
                            single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_RUN_THIS),
                                generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_RUN_REGISTER), general_handler(i.b))
                        } else {
                            single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_OWN_THIS),
                                generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_OWN_REGISTER), general_handler(i.b))
                        })
                    }
                    // A site that cached nothing yet (its first read, before the general read
                    // below records an entry) keeps its record: the next miss decides by the entry,
                    // a prototype's method (`a.includes`) the held row.
                    _ if (unsafe { vm(cx) }).field_site_empty(i.c) => None,
                    // A function's own field the layout does not hold: the cached read.
                    _ => Some(single_base_handler(
                        i.b,
                        generated_row!(FIELD_LEAF_READ, ACC_NONE, CACHED_FIELD_THIS),
                        generated_row!(FIELD_LEAF_READ, ACC_NONE, CACHED_FIELD_REGISTER),
                        general_handler(i.b),
                    )),
                }
            }
            FieldSiteTier::Own => None,
        };
        if let Some(handler) = handler {
            requicken(ip, handler);
        }
        sibling!(row_get_field_general, r, ip, Carried::script_in(cx, view))
    }
}

/// The intrinsic geometry getter's value of `atom` on `base` (`Vm::typed_array_geometry_read`)
/// for a geometry name (`AtomClass::VIRTUAL_FIELD`); `None` for any other read.
#[inline(always)]
fn geometry_read_in_lane(vm: &mut Vm, base: Value, atom: quench_runtime::bytecode::Atom) -> Option<Value> {
    if !vm.atom_class(atom).contains(quench_runtime::vm::object::AtomClass::VIRTUAL_FIELD) {
        return None;
    }
    vm.typed_array_geometry_read(base, atom)
}

/// The `length` of a receiver that is no array or string (`FieldPlan::Length`): an arguments
/// object's or a user function's own data `length`, else a typed view's intrinsic geometry
/// (`Vm::typed_array_geometry_read_inline`); `None` for any other receiver.
#[inline(always)]
fn length_read(vm: &mut Vm, receiver: Value) -> Option<Value> {
    let atom = vm.length_atom;
    vm.arguments_object_length(receiver)
        .or_else(|| vm.plain_function_length(receiver))
        .or_else(|| vm.typed_array_geometry_read_inline(receiver, atom))
}

pub(crate) use quench_runtime::vm::engine_types::FieldSiteTier;

fn field_site_tier_in_lane(
    vm: &Vm,
    code: &ResidualProgram,
    i: &LaneInstruction,
) -> FieldSiteTier {
    if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        // A step that reads `name` or `length` may prepare prototype lookup
        // (`Vm::reads_function_info`); only an array's own `length` needs no preparing.
        let nested_own = code.field_sites.get(i.imm() as usize).is_some_and(|site| {
            !vm.reads_function_info(site.first.0)
                && site.second.is_none_or(|(atom, _)| {
                    atom == vm.length_atom || !vm.reads_function_info(atom)
                })
        });
        if nested_own { FieldSiteTier::Nested } else { FieldSiteTier::General }
    } else {
        // The entry tiers come from one cache index (`Vm::field_site_tier`), not one lookup per
        // predicate, since a held or inherited local read classifies its site on every read.
        vm.field_site_tier(i.c)
    }
}

/// A nested `GetField`'s value (`this.a.b`, `r.a.b`), each step one shape compare and one
/// load where it is monomorphic: the first step through the record's own-field hint
/// (`lane_nested_hinted_first`) and the second through its site's primary own entry or an
/// array's or a string's own `length` (`nested_last_step`). A second step past its primary
/// entry is read alone from the first's value (`lane_nested_second_value`), and a first step
/// past the hint takes both steps' polymorphic entries (`lane_nested_polymorphic_value`),
/// which refreshes the hint. `None` leaves the read to the general one. The site reads
/// neither `name` nor another `length` (`lane_field_site_tier`).
///
/// # Safety
/// `cx` is the running lane's context and `i` the nested record at `ip`.
#[inline(always)]
unsafe fn lane_nested_value(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run; a nested record names its site.
    let code = unsafe { &*(*cx).code };
    let site = unsafe { code.field_sites.get_unchecked(i.imm() as usize) };
    match unsafe { lane_nested_hinted_first(cx, r, i, site) } {
        Some(first) => {
            let vm = unsafe { vm(cx) };
            nested_last_step(vm, code, site, first).or_else(|| lane_nested_second_value(vm, code, site, first))
        }
        None => unsafe { nested_polymorphic_value_in_lane(cx, r, ip) },
    }
}

/// The first step of the nested read `site` of record `i`: the own field its own-field hint
/// (`LaneInstruction::field_hint`) names, which holds the receiver shape that step last met
/// however many its site has seen (a parser's `this` gains a field mid-parse); `None` for a
/// receiver of another shape.
///
/// # Safety
/// `cx` is the running lane's context and `i` a nested record of it naming `site`.
#[inline(always)]
unsafe fn lane_nested_hinted_first(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
    site: &quench_runtime::bytecode::FieldSite,
) -> Option<Value> {
    let base = match site.base.register_index() {
        Some(register) => r.get(register),
        None => unsafe { fixed_this_in_lane(cx, r) }?,
    };
    (unsafe { vm(cx) }).record_field_entry_hit(i.field_hint(), base)
}

/// The second field step of the nested read `site` from `first`, the value its hinted first
/// step read, past the step's primary own entry (`nested_last_step`): the megamorphic stub
/// cache, then the step's polymorphic entries (`Vm::megamorphic_field_hit`), as `node.left.type`
/// reads nodes of many shapes behind one receiver's field. The first step is not read again.
/// Out of line, so the hinted probe keeps its registers; `None` for a `length` step or a miss.
#[inline(never)]
fn lane_nested_second_value(
    vm: &mut Vm,
    code: &ResidualProgram,
    site: &quench_runtime::bytecode::FieldSite,
    first: Value,
) -> Option<Value> {
    match site.second {
        Some((atom, cache)) if atom != vm.length_atom => vm.megamorphic_field_hit(code, first, atom, cache),
        _ => None,
    }
}

/// `lane_nested_value` past the record's hint: each step's site's entries, the primary own
/// one, else the stub cache's and the polymorphic ones, inline or hashed
/// (`Vm::megamorphic_field_entry_hit`), as an AST walk's `node.loc.start` reads nodes of many
/// shapes, and a string's own `length`, as a tokenizer's `this.input.length` reads (`lane_cached_field`); none runs guest code. The own
/// entry that answered the first step becomes the record's hint. Out of line, so the hinted
/// probe keeps its registers; `None` leaves the read to `lane_get_field`.
///
/// # Safety
/// As for `lane_nested_value`.
#[inline(never)]
unsafe fn nested_polymorphic_value_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run; a nested record names its site.
    let code = unsafe { &*(*cx).code };
    let site = unsafe { code.field_sites.get_unchecked((*ip).imm() as usize) };
    let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
    let vm = unsafe { vm(cx) };
    let (first, own) = match vm.primary_own_field_hit(code, base, site.first.1) {
        Some(value) => (value, Some(vm.field_cache_entry(site.first.1))),
        None => match vm.megamorphic_field_entry_hit(code, base, site.first.0, site.first.1)? {
            (value, quench_runtime::vm::object::PolymorphicHit::Own(entry)) => (value, Some(entry)),
            (value, _) => (value, None),
        },
    };
    if let Some(entry) = own {
        set_field_hint(ip, entry);
    }
    match site.second {
        None => Some(first),
        Some((atom, _)) if atom == vm.length_atom => own_length(vm, first),
        Some((atom, cache)) => vm
            .primary_own_field_hit(code, first, cache)
            .or_else(|| vm.megamorphic_field_hit(code, first, atom, cache)),
    }
}

/// A nested site's read of `base` where both steps hit primary own entries
/// (`lane_nested_value`), which runs no guest code.
#[inline(always)]
pub(crate) fn nested_own_field_hit(
    vm: &mut Vm,
    code: &ResidualProgram,
    site: &quench_runtime::bytecode::FieldSite,
    base: Value,
) -> Option<Value> {
    // Two ordinary field reads are one chain row, which proves the tables once
    // (`Vm::primary_own_chain_hit`); a single read or a `length` step is `nested_last_step`'s.
    let Some((_, second)) = site.second.filter(|(atom, _)| *atom != vm.length_atom) else {
        let first = vm.primary_own_field_hit(code, base, site.first.1)?;
        return nested_last_step(vm, code, site, first);
    };
    let value = vm.primary_own_chain_hit(code, base, (site.first.1, second));
    #[cfg(feature = "profile-field-primitives")]
    pair_census(vm, code, (site.first.1, base), value.is_some());
    value
}

/// The census of a nested pair read: its attempt, whether its first step hit (a miss of the chain
/// reads the first step alone to attribute it) and whether both did.
#[cfg(feature = "profile-field-primitives")]
fn pair_census(vm: &mut Vm, code: &ResidualProgram, (first, base): (u16, Value), hit: bool) {
    use quench_runtime::profile::field_primitives::Path;
    vm.profile.field_primitives.hit(Path::NestedOwnPairAttempt);
    if hit || vm.primary_own_field_hit(code, base, first).is_some() {
        vm.profile.field_primitives.hit(Path::NestedOwnPairFirstHit);
    }
    if hit {
        vm.profile.field_primitives.hit(Path::NestedOwnPairHit);
    }
}

/// A nested site's last step from `first`, the first step's value: none, an array's or a
/// string's own `length`, or the second step's primary own entry.
#[inline(always)]
fn nested_last_step(
    vm: &mut Vm,
    code: &ResidualProgram,
    site: &quench_runtime::bytecode::FieldSite,
    first: Value,
) -> Option<Value> {
    match site.second {
        None => Some(first),
        Some((atom, _)) if atom == vm.length_atom => own_length(vm, first),
        Some((_, cache)) => {
            let value = vm.primary_own_field_hit(code, first, cache);
            #[cfg(feature = "profile-field-primitives")]
            if value.is_some() { vm.profile.field_primitives.hit(quench_runtime::profile::field_primitives::Path::NestedOwnPairHit); }
            value
        }
    }
}

/// The own `length` of an array or a string, which no prototype shadows, as a nested
/// read's last step reads it (`nested_own_field_hit`, `nested_polymorphic_value_in_lane`).
#[inline(always)]
fn own_length(vm: &Vm, object: Value) -> Option<Value> {
    // The staged row answers either by the value's tag; a form it leaves is read through the
    // string's units, or the array's record.
    vm.length_value(object).or_else(|| match vm.heap_reads().string(object) {
        Some(text) => Some(Value::length(text.len())),
        None => vm.array_length_field(object, vm.length_atom),
    })
}

script_handler! {
    /// A nested `GetField` whose steps hit their entries without guest code
    /// (`lane_nested_value`); any other read takes the general row (`row_get_field_general`).
    fn get_field_nested_miss<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_nested_value(cx, r, ip, i) }) else {
            sibling!(row_get_field_general, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// The general read's miss (`GENERAL_THIS`, `GENERAL_REGISTER`, and the nested or local base):
    /// the general read the row's host read did not answer, which repeats the side-effect-free
    /// lookup, then the activation path of a getter or a proxy.
    #[inline(never)]
    fn row_get_field_general<>(cx, r, ip, i, view) {
        // A method read whose receivers find it at different prototype depths (an override in
        // one class, the base's in another) stays here: the site's primary holder entry and its
        // proved inherited entry first, neither of which walks the chain.
        let quick = quench_runtime::bytecode::FieldBase(i.b).register_index().and_then(|register| {
            let vm = unsafe { vm(cx) };
            // SAFETY: the context's program outlives the lane run.
            let code = unsafe { &*(*cx).code };
            let object = r.get(register);
            vm.specialized_operations_enabled(code).then_some(())?;
            vm.primary_record_holder_field_hit(code, object, i.c).or_else(|| vm.proved_inherited_field_hit(code, object, i.c))
        });
        let Some(value) = quick.or_else(|| unsafe { field_value_in_lane(cx, r, i) }) else {
            if let Some(base) = field_read_beside_activations_in_lane(cx, r, i) {
                // A proxy's trap runs as an activation beside the others (`AccessorCall::read_get_trap`);
                // the in-place general read would call it across the native boundary.
                if unsafe { vm(cx) }.proxy_parts(base).is_some() {
                    match getter_activation_in_lane(cx, r, ip, i) {
                        Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
                        // A native getter that raised is not called again: the general path raises it.
                        Err(next) if (unsafe { vm(cx) }).lane_completion.is_raised() => return next,
                        Err(_) => {}
                    }
                }
                sibling!(row_get_field_beside_activations, unsafe { (*cx).registers }, ip, Carried::script_in(cx, view))
            }
            return match getter_activation_in_lane(cx, r, ip, i) {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
                Err(next) => next,
            };
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `lane_get_field` at a site whose reads met a getter (`FieldSiteTier::Getter`): the
    /// getter activation first, whose cache check fails without effect, so a data read the
    /// cached probes answer stays `lane_get_field`'s. An activation without lane-stack room
    /// makes the activations frames first (`script_call_frame_in_lane`), so the read goes on with
    /// the running function's registers read again (the rollup npm scenario's
    /// `childNodeKeys[this.type]` was written to the emptied lane stack, not its frame).
    #[inline(never)]
    fn get_field_getter_miss<>(cx, r, ip, i, view) {
        // One walk of the site's accessor entries: a trivial getter's forward, or the getter
        // to call with no second lookup (`getters::lane_getter_step`).
        let read = match getters::lane_getter_step(cx, r, ip, i) {
            getters::GetterStep::Answer(value) => {
                r.set(i.a, value);
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
            getters::GetterStep::Read(read) => read,
            getters::GetterStep::Miss => getter_activation_in_lane(cx, r, ip, i),
        };
        match read {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            // A native getter that raised is not called again: the general path raises it.
            Err(next) if (unsafe { vm(cx) }).lane_completion.is_raised() => next,
            Err(_) => sibling!(row_get_field_general, unsafe { (*cx).registers }, ip, Carried::script_in(cx, view)),
        }
    }
}

/// The base of a `GetField` of a register or `this` that a frameless activation makes in
/// place (`row_get_field_beside_activations`): a public name whose site caches no getter,
/// which `getter_activation_in_lane` would run as an activation. `None` for a frame's run, a
/// nested or local base, a private name or such a getter.
#[inline(never)]
fn field_read_beside_activations_in_lane(cx: *mut LaneContext, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !vm.lane_stack.has_frames() || i.b == quench_runtime::bytecode::FieldBase::NESTED || vm.is_private_name(i.imm()) {
        return None;
    }
    let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }?;
    vm.cached_field_getter(base, i.imm(), vm.field_cache_index(i.c)).is_none().then_some(base)
}

script_handler! {
    /// `lane_get_field` past its caches within a frameless activation
    /// (`field_read_beside_activations_in_lane`): the general read made in place
    /// (`unwound_in_place`), so an uncached getter or a proxy makes the activations frames
    /// only when it runs, instead of leaving the lane, which makes them all frames, and a
    /// throw (`undefined.x`) is unwound in the lane.
    #[inline(never)]
    fn row_get_field_beside_activations<>(cx, r, ip, i, view) {
        let Some(base) = field_read_beside_activations_in_lane(cx, r, i) else {
            return exit(ip);
        };
        let (atom, site) = (i.imm(), i.c);
        let value = match unwound_in_place(cx, ip, |vm, code| vm.get_field_cached(code, base, atom, site)) {
            Ok(value) => value,
            Err(next) => return next,
        };
        // Guest code the read ran made the activations frames, whose registers these are.
        let r = unsafe { (*cx).registers };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// A lane `GetField` whose read meets the accessor its site cached
/// (`Vm::cached_field_getter`): the getter runs as a frameless activation of the read's
/// record, as `script_call_miss` runs a call, with the base as receiver and no arguments,
/// and its completion is the read's value. `Err` carries the record to continue at: the
/// general path for any other read.
#[inline(never)]
fn getter_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Ip> {
    // No admission test here: the getter's target is found through the admission word
    // (`lane_script_target_in`, `cross_call_entry`), which refuses a run that calls no script
    // functions or has a construction pending, and a native getter asks it in `read_getter`.
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The object the getter's read is of and its site: a nested site's second field is read
    // of the value its first, cached read gives.
    let read = if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        code.field_sites.get(i.imm() as usize).copied().and_then(|site| {
            let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
            match site.second {
                Some((atom, cache)) => {
                    Some((lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, base, site.first.0, site.first.1, 0)?, atom, cache))
                }
                None => Some((base, site.first.0, site.first.1)),
            }
        })
    } else {
        unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }.map(|base| (base, i.imm(), i.c))
    };
    let vm = unsafe { vm(cx) };
    #[cfg(feature = "profile-lane-exits")]
    if let Some((base, atom, cache)) = read
        && vm.cached_field_getter(base, atom, vm.field_cache_index(cache)).is_none()
    {
        quench_runtime::vm::lane_exits::record_detail(format!(
            "getter decline uncached {} recorded={}",
            &code.atoms[atom as usize],
            vm.field_site_getter(cache)
        ));
    } else if read.is_none() {
        quench_runtime::vm::lane_exits::record_detail("getter decline base".into());
    }
    let Some((base, getter)) = read.and_then(|(base, atom, cache)| {
        Some((base, vm.cached_field_getter(base, atom, vm.field_cache_index(cache))?))
    }) else {
        return AccessorCall::of(cx).read_get_trap(r, ip, read.map(|(base, atom, _)| (base, atom)));
    };
    AccessorCall::of(cx).read_getter(r, ip, i, getter, base)
}

/// The activation of `getter` with receiver `base` and no arguments for the read record at
/// `ip`, whose completion is the read's value (`resolved`: its frameless target, which the read
/// resolved already): a frameless target of the running program, or
/// another program's plain activation (`Vm::cross_call_entry`), which runs in its program.
/// `Err` carries the record to continue at: the general path for any other getter.
fn activate_getter_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    getter: Value,
    base: Value,
    resolved: Option<ScriptCallTarget>,
) -> Result<(Registers, Ip), Ip> {
    // The getter's activation is pushed above the reading function's, and resolving its
    // target may displace the cached target a running leaf names, so a leaf's read makes the
    // leaf an activation first. A target the read resolved (`resolved`) is a copy, which
    // that leaves as it was.
    materialize_running_leaf_in_lane(cx);
    let vm = unsafe { vm(cx) };
    if let Some(target) = resolved.or_else(|| script_target_in_lane(cx, getter)).filter(|target| !target.forwards()) {
        // A getter is an ordinary call: the one call row where it is plain, else the kept activation.
        if let Some(entered) = getters::invoke_getter(cx, r, ip, &target, base) {
            return Ok(entered);
        }
        let Some(this) = call_this_in_lane(vm, &target, base) else {
            return Err(exit(ip));
        };
        return activate_kept_in_lane::<false>(cx, r, ip, &target, &[], this);
    }
    // A getter that makes closures, of this program or another, runs as the environment
    // activation its plain call makes (`activate_environment_in_lane`), with no arguments, as
    // Zod's `get error() { ... issues.map((iss) => ...) }` does.
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let environment = vm
        .script_call_entry(getter)
        .or_else(|| vm.cross_call_entry(getter))
        .map(|target| unsafe { *target })
        .filter(|target| target.entry == CallEntry::Environment && !constructing_in_lane(vm, getter));
    if let Some(target) = environment {
        let Some(this) = call_this_in_lane(vm, &target, base) else {
            return Err(exit(ip));
        };
        return activate_environment_in_lane(cx, r, ip, &target, &[], (this, false));
    }
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    let Some(target) = vm
        .cross_call_entry(getter)
        .map(|target| unsafe { *target })
        .filter(|target| target.entry.activates() && !constructing_in_lane(vm, getter))
    else {
        #[cfg(feature = "profile-lane-exits")]
        quench_runtime::vm::lane_exits::record_detail(format!(
            "getter decline target cross={:?} constructing={}",
            vm.cross_call_entry(getter).map(|target| unsafe { (*target).entry } as u8),
            constructing_in_lane(vm, getter)
        ));
        return Err(exit(ip));
    };
    let Some(this) = call_this_in_lane(vm, &target, base) else {
        return Err(exit(ip));
    };
    activate_kept_in_lane::<true>(cx, r, ip, &target, &[], this)
}

/// The private-name site keys of a nested read's two steps, as offsets from its record's
/// address (`LaneFrame::private_slot`): a record spans more bytes than steps, so no other
/// record's key is one of them.
const NESTED_STEPS: [usize; 2] = [1, 2];

/// A `GetField` record's value where caches answer it (`LaneFrame::cached_read`): one read of a
/// register or `this`, or a nested site's chain of two. `None` leaves the read to the
/// general path.
///
/// Out of line: the generic reads (`lane_get_field`, `row_get_field_sink`, a returning
/// `GetField`) run it on their slower paths, so one copy serves them all instead of each
/// carrying its own probe (DeltaBlue, PGO build: ~40 instruction-cache lines of warm code).
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(never)]
unsafe fn field_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The record's address names its private-name sites (`LaneFrame::private_slot`).
    let record = std::ptr::from_ref(i).addr();
    if i.b == quench_runtime::bytecode::FieldBase::NESTED {
        let site = *code.field_sites.get(i.imm() as usize)?;
        let base = unsafe { field_base_in_lane(cx, r, site.base) }?;
        let value = field_step_in_lane(cx, record + NESTED_STEPS[0], code, base, site.first.0, site.first.1)?;
        return match site.second {
            Some((atom, cache)) => field_step_in_lane(cx, record + NESTED_STEPS[1], code, value, atom, cache),
            None => Some(value),
        };
    }
    let base = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }?;
    field_step_in_lane(cx, record, code, base, i.imm(), i.c)
}

/// The object a field base names: a register, or `this` where the lane knows it.
///
/// # Safety
/// As for `field_value_in_lane`.
#[inline(always)]
unsafe fn field_base_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: quench_runtime::bytecode::FieldBase,
) -> Option<Value> {
    if let Some(slot) = base.local_index() {
        return plain_local_operand(cx, slot);
    }
    match base.register_index() {
        Some(register) => Some(r.get(register)),
        None => unsafe { this_in_lane(cx, r) },
    }
}

script_handler! {
    /// A nested `GetField` that also stores its value to a field of `this` (`this.a =
    /// this.b.c`): the read as `lane_get_field` makes it, then the cached write of
    /// `SetThisField`. A read or write the caches do not answer takes the general path,
    /// which repeats the side-effect-free read.
    #[inline(never)]
    fn row_get_field_sink<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let (Some(value), Some(this), Some(sink)) = (
            unsafe { field_value_in_lane(cx, r, i) },
            unsafe { this_in_lane(cx, r) },
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
        r.set(i.a & quench_runtime::bytecode::REGISTER_MASK, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// The general write of a store past its generated row (`GENERAL_SITE`): a private name's store
/// (`lane_private_field_store`), else the public name's store made in place
/// (`row_set_field_in_place`). The row's own cached write already refused it.
#[inline(never)]
fn row_set_field_miss_path(cx: *mut LaneContext, r: Registers, ip: Ip, object: Value, i: &LaneInstruction) -> Resumed {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    if vm.is_private_name(i.imm()) {
        if !lane_frame::LaneFrame::<ACC_NONE>::private_store(cx, ip.addr(), code, object, i.imm(), r.get(i.a)) {
            return Resumed::Exit(exit(ip));
        }
        // Adding a property can allocate.
        note_allocation(vm);
        return Resumed::At(r, following(ip), Carried::script(cx));
    }
    row_set_field_in_place(cx, r, ip, object, i)
}

/// `lane_set_field` of a public name past its caches: the general store made in place
/// (`in_place_in_lane`), in the frame's own run or beside frameless activations, strict as its
/// record is. A store that adds a property to receivers of many shapes (zod's schema
/// initializers in the playwright npm scenario: 51K of its 79K lane exits) otherwise left the
/// lane at every store.
#[cold]
#[inline(never)]
fn row_set_field_in_place(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    object: Value,
    i: &LaneInstruction,
) -> Resumed {
    // The running function decides the store's strictness: a running leaf's is its
    // materialized activation's.
    materialize_running_leaf_in_lane(cx);
    if let Some(resumed) = AccessorCall::of(cx).store_setter(r, ip, i, object) {
        return resumed;
    }
    let Some(strict) = store_is_strict_in_lane(cx, ip) else {
        return Resumed::Exit(exit(ip));
    };
    let (atom, value, site) = (i.imm(), r.get(i.a), i.c);
    if let Some(resumed) = AccessorCall::of(cx).store_set_trap(r, ip, object, atom, value, strict) {
        return resumed;
    }
    if let Err(next) = unwound_in_place(cx, ip, |vm, code| vm.set_field_cached(code, object, atom, value, site, strict)) {
        return Resumed::Exit(next);
    }
    // Guest code the store ran made the activations frames, whose registers these are.
    let r = unsafe { (*cx).registers };
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// Whether the named field store at the record `ip` rejects a refused write with a
/// `TypeError`, as its opcode says; `None` for any other record.
fn store_is_strict_in_lane(cx: *mut LaneContext, ip: Ip) -> Option<bool> {
    field_store_at(cx, ip).map(|store| store.strict)
}

/// The named field store at the record `ip`, as its opcode says; `None` for any other record.
fn field_store_at(cx: *mut LaneContext, ip: Ip) -> Option<FieldStore> {
    let vm = unsafe { vm(cx) };
    let function = match vm.lane_stack.top_frame() {
        Some(activation) => activation.function(),
        None => vm.frames[unsafe { (*cx).frame() }].function,
    };
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // innermost activation's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(function as usize);
    match crate::compiled::decoded(body.widened(body.code[pc])) {
        // A store to `this` has no base register (`SetThisField`).
        Some(quench_runtime::staged::Operation::SetField { base, strict, .. }) => Some(FieldStore {
            receiver: if base.is_some() { StoreReceiver::Register } else { StoreReceiver::This },
            strict,
        }),
        _ => None,
    }
}

/// The brand of private name `atom` at the record `ip`, whose home follows the running
/// function and the environment its frame would capture from: the lane's frame, or a
/// frameless activation's function and closure environment (`Vm::site_private_brand`).
#[inline(always)]
fn private_brand_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    code: &ResidualProgram,
    atom: quench_runtime::bytecode::Atom,
) -> Option<quench_runtime::tier_surface::PrivateBrand> {
    let (environment, function, frame) = private_site_in_lane(cx)?;
    unsafe { vm(cx) }.site_private_brand(code, ip.addr(), environment, function, frame, atom)
}

/// What a lane read of a private name of an object resolves to (`get_private_field_miss`).
enum PrivateRead {
    Value(Value),
    /// A private accessor's getter, called with the object.
    Getter(Value, Value),
}

/// The running function's private-name scope: the environment its frame, or its activation,
/// captures from, the function, and whether it runs in a frame (`Vm::site_private_brand`).
#[inline(always)]
fn private_site_in_lane(cx: *mut LaneContext) -> Option<(Value, u32, bool)> {
    let vm = unsafe { vm(cx) };
    Some(match vm.lane_stack.top_frame() {
        None => {
            // The running frame is the top one, whose home `Vm::private_name_home` finds.
            let frame = vm.frames.len().checked_sub(1)?;
            (vm.captured_parent_environment(vm.frame_caller(frame)), vm.frames[frame].function, true)
        }
        // SAFETY: the context outlives the lane run.
        Some(activation) => (unsafe { (*cx).registers }.env(), activation.script().map(|_| activation.function())?, false),
    })
}

/// One `GetField` step of `field_value_in_lane`: a private name's at its site, any other name's
/// through the field caches (`LaneFrame::cached_read`).
#[inline(always)]
fn lane_private_field_slot(
    cx: *mut LaneContext,
    ip: Ip,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
) -> Option<usize> {
    lane_private_field_slot_at(cx, ip.addr(), code, object, atom)
}

/// `lane_private_field_slot` at site `site`: a record's address, or an address inside it for
/// a nested read's step (`lane_field_value`), which no other record's site can be. A site
/// whose name belongs to a class evaluated once (`PrivateClasses::unique_name`) answers from
/// the receiver's shape alone, before the running function's scope is found.
#[inline(always)]
fn lane_private_field_slot_at(
    cx: *mut LaneContext,
    site: usize,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
) -> Option<usize> {
    {
        let vm = unsafe { vm(cx) };
        if vm.private_classes.unique_name(atom)
            && let Some(slot) = vm.private_site_homes.unique_field_slot(site, vm.object_data(object)?.shape())
        {
            return Some(slot);
        }
    }
    let scope = private_site_in_lane(cx)?;
    (unsafe { vm(cx) }).site_private_field_slot(code, site, scope, object, atom)
}

/// Makes the slot site `ip`'s entry recorded for an object of `shape` (`lane_private_field_slot`)
/// the hint of its `GetField` or `SetField` record (`lane_get_private_field`,
/// `lane_private_field_store`), where its private name `atom` belongs to a class evaluated
/// once: the entry recorded it under that class's home (`SiteHome::unique`), the site's for
/// good while the name stays unique, in a shape no edit in place changes, and a shape
/// compaction clears both. A record of a site that recorded none keeps its hint. A hit
/// compares the receiver's shape and reads the name's word (`PrivateClasses::unique_name`):
/// a second evaluation of the class, or eval code that sees the name, clears it, which
/// disables the hints of that name's sites alone.
#[inline(never)]
fn hint_private_field(vm: &Vm, ip: Ip, atom: quench_runtime::bytecode::Atom, shape: u32, slot: usize) {
    if vm.private_classes.unique_name(atom)
        && vm.private_site_homes.unique_field_slot(ip.addr(), shape) == Some(slot)
        && let Ok(slot) = u16::try_from(slot)
        && slot != NO_CALL_LINK
    {
        set_field_hint(ip, quench_runtime::vm::FieldCache::own(shape, slot));
    }
}

/// One `GetField` step of `lane_field_value`: a private name's at its site, any other name's
/// through the field caches (`lane_cached_field`).
#[inline(always)]
fn field_step_in_lane(
    cx: *mut LaneContext,
    site: usize,
    code: &ResidualProgram,
    object: Value,
    atom: quench_runtime::bytecode::Atom,
    cache: u16,
) -> Option<Value> {
    lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, object, atom, cache, site)
}

/// Raise at `ip` after its instruction failed with `error` before any effect, where a handler
/// of the running function or of a plain caller takes it in the lane
/// (`handler_edges::unwind_raised`). A frame run raises `error`; beside frameless activations,
/// whose frames `error`'s stack capture did not read, `again` makes the instruction's error a
/// second time beside them (`handler_edges::raise_again`, frame-switches.md #6).
#[inline(never)]
fn raise_in_lane<T>(
    cx: *mut LaneContext,
    ip: Ip,
    error: JsError,
    again: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Ip {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        drop(error);
        return handler_edges::raise_again(cx, ip, again);
    }
    vm.lane_completion.raise(error);
    handler_edges::unwind_raised(cx, ip)
}

script_handler! {
    /// `DefineField` (`Vm::define_field_cached`), as the general step defines it. A Proxy
    /// receiver's define runs its trap, which a frameless activation leaves to the general path.
    #[inline(never)]
    fn define_field_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let object = r.get(i.b);
        // An object record, which is never a proxy, takes its site's proved define inline.
        if vm.proved_field_define_hit(object, i.imm(), r.get(i.a), i.c) {
            // Adding a property can allocate.
            note_allocation(vm);
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        if vm.lane_stack.has_frames() && vm.proxy_parts(object).is_some() {
            return exit(ip);
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let function = running_function_in_lane(cx);
        if let Err(error) = vm.define_field_cached(code, object, i.imm(), r.get(i.a), i.c, function) {
            return raise_in_lane(cx, ip, error, |vm, code| vm.define_field_cached(code, object, i.imm(), r.get(i.a), i.c, function));
        }
        // Adding a property can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `DefinePrivateField` (`Vm::initialize_private_field`) of the object in `b` under the home
    /// in `c`, which runs no guest code.
    #[inline(never)]
    fn define_private_field_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (object, home) = (r.get(i.b), r.get(i.c));
        // The site's last add from this shape under this home (`PrivateSiteHomes::define_shape`).
        if let Some(data) = vm.object_data(object)
            && let Some(next) = vm.private_site_homes.define_shape(ip.addr(), home, data.shape())
            && vm.private_element_addable(data)
        {
            vm.add_property(object, r.get(i.a), next);
            note_allocation(vm);
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let before = vm.object_data(object).map(|data| data.shape());
        if let Err(error) = vm.initialize_private_field(code, object, home, i.imm(), r.get(i.a)) {
            return raise_in_lane(cx, ip, error, |vm, code| vm.initialize_private_field(code, object, home, i.imm(), r.get(i.a)));
        }
        if let (Some(before), Some(after)) = (before, vm.object_data(object).map(|data| data.shape()))
            && vm.private_shape_cacheable(before)
            && vm.private_shape_cacheable(after)
        {
            vm.private_site_homes.record_define(ip.addr(), home, before, after);
        }
        // Adding a private element can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `MarkPrivateName` (`Vm::mark_private_name`) of the object in `b` under the home in `c`,
    /// which runs no guest code: a constructor's private methods, branded in place where a
    /// general step would materialize the activations it runs beside.
    #[inline(never)]
    fn mark_private_name_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (object, home) = (r.get(i.b), r.get(i.c));
        // The site's last brand from this shape under this home (`PrivateSiteHomes::define_shape`);
        // the shape lacks the brand, so the object is not branded yet.
        if object != home
            && let Some(data) = vm.object_data(object)
            && let Some(next) = vm.private_site_homes.define_shape(ip.addr(), home, data.shape())
            && vm.private_element_addable(data)
        {
            vm.add_property(object, Value::UNDEFINED, next);
            note_allocation(vm);
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let before = vm.object_data(object).map(|data| data.shape());
        if let Err(error) = vm.mark_private_name(code, object, home, i.imm()) {
            return raise_in_lane(cx, ip, error, |vm, code| vm.mark_private_name(code, object, home, i.imm()));
        }
        if object != home
            && let (Some(before), Some(after)) = (before, vm.object_data(object).map(|data| data.shape()))
            && before != after
            && vm.private_shape_cacheable(before)
            && vm.private_shape_cacheable(after)
        {
            vm.private_site_homes.record_define(ip.addr(), home, before, after);
        }
        // Adding a private element can allocate.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// `DefinePropertyRecord` of the record at `ip`, defined as the general step defines it: a
/// class method or accessor, or an object literal accessor. An out-of-domain mode leaves the
/// instruction to the general path, a throw is raised at the record (`unwound_in_place`):
/// `Err` is the record the run goes on at. Out of line, as `row_index_write`:
/// the handler's frame then holds none of the descriptor.
#[inline(never)]
fn define_property_record_in_place_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(), Ip> {
    let mode = quench_runtime::bytecode::PropertyDefinitionMode::from_word(i.imm()).ok_or(exit(ip))?;
    let (value, target, key) = (i.a, i.b, i.c);
    unwound_in_place(cx, ip, |vm, code| {
        let (value, target, key) = {
            let r = unsafe { (*cx).registers };
            (r.get(value), r.get(target), r.get(key))
        };
        let descriptor =
            quench_runtime::vm::object_descriptors::PropertyDescriptorRecord::for_definition(mode, value);
        vm.define_property_or_throw(code, target, key, descriptor)
    })
}

script_handler! {
    /// `DefinePropertyRecord`, whose computed key's conversion or a proxy's define may run
    /// guest code. An out-of-domain mode leaves the instruction to the general path before it
    /// has any effect.
    #[inline(never)]
    fn define_property_record_miss<>(cx, r, ip, i, view) {
        if let Err(next) = define_property_record_in_place_in_lane(cx, ip, i) {
            return next;
        }
        // Guest code the definition ran made the activations frames, whose registers these are.
        let r = unsafe { (*cx).registers };
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `CheckPrivate` of an object holding the private field under the name's brand at this
    /// record (`private_brand_in_lane`), which proves the check; any other check takes
    /// `general_step_in_lane`.
    #[inline(never)]
    fn check_private_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let present = lane_frame::LaneFrame::<ACC_NONE>::private_slot(cx, ip.addr(), code, r.get(i.a), i.imm()).is_some();
        if !present {
            sibling!(general_step_in_lane, r, ip, Carried::script_in(cx, view))
        }
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `CopyDataProperties` (an object spread or rest) that a plain copy answers
    /// (`Vm::copy_plain_data_properties`), which runs no guest code and cannot throw, so it
    /// runs beside activations; any other copy takes `general_step_in_lane`.
    #[inline(never)]
    fn copy_data_properties_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !vm.copy_plain_data_properties(r.get(i.a), r.get(i.b), r.get(i.c)) {
            sibling!(general_step_in_lane, r, ip, Carried::script_in(cx, view))
        }
        // The copy can allocate property storage.
        note_allocation(vm);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `PrivateIn` (`#x in object`) of an object with its own data (`Vm::object_data`): whether
    /// it carries the name's brand at this record (`private_brand_in_lane`), a field's or a
    /// method's. Any other operand, which may raise, takes `general_step_in_lane`.
    #[inline(never)]
    fn private_in_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let object = r.get(i.b);
        let atom = i.imm();
        // A site whose name belongs to a class evaluated once answers a shape it saw carry the
        // name from the shape alone (`PrivateSiteHomes::unique_carrier`).
        let known = {
            let vm = unsafe { vm(cx) };
            vm.private_classes.unique_name(atom)
                && vm.object_data(object).is_some_and(|data| vm.private_site_homes.unique_carrier(ip.addr(), data.shape()))
        };
        let carries = if known { Some(true) } else { private_brand_in_lane(cx, ip, code, atom).and_then(|brand| {
            let vm = unsafe { vm(cx) };
            let shape = vm.object_data(object)?.shape();
            let carries = vm.has_private_name(object, brand);
            if carries && vm.private_shape_cacheable(shape) && vm.private_classes.register(atom, brand.home) {
                match vm.private_field_slot(object, brand) {
                    Some(slot) => vm.private_site_homes.record_field_slot(ip.addr(), brand.home, shape, slot, true),
                    None => vm.private_site_homes.record_brand_shape(ip.addr(), brand.home, shape),
                }
            }
            Some(carries)
        }) };
        let Some(carries) = carries else {
            sibling!(general_step_in_lane, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, Value::boolean(carries));
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `GetField` of a private name (`Vm::is_private_name`) from a register or `this`, which
    /// no field cache records (`row_get_field_first`): the object's private field under
    /// the name's brand at this record (`private_brand_in_lane`). Any other read, a private
    /// method's or accessor's included, takes `lane_get_field`.
    #[inline(never)]
    fn get_private_field_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let element_read = |element: quench_runtime::vm::private_site_home::PrivateElement, object: Value| match element {
            quench_runtime::vm::private_site_home::PrivateElement::Method(value) => PrivateRead::Value(value),
            quench_runtime::vm::private_site_home::PrivateElement::Getter(getter) => PrivateRead::Getter(object, getter),
        };
        let read = unsafe { field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b)) }.and_then(|object| {
            {
                // A site whose name belongs to a class evaluated once
                // (`PrivateClasses::unique_name`): the receiver's shape names the private key
                // with that class's home, the site's for good, so it proves the brand. The
                // record's hint holds the field's shape and slot (`hint_private_field`), the
                // site's entry a method's or accessor's brand.
                let vm = unsafe { vm(cx) };
                if vm.private_classes.unique_name(i.imm()) {
                    let data = vm.object_data(object)?;
                    let shape = data.shape();
                    let hint = i.field_hint();
                    if shape == hint.receiver {
                        return vm.heap_reads().property_get(data, usize::from(hint.slot)).map(PrivateRead::Value);
                    }
                    if let Some(element) = vm.private_site_homes.unique_element(ip.addr(), shape) {
                        return Some(element_read(element, object));
                    }
                }
            }
            if let Some(slot) = lane_private_field_slot(cx, ip, code, object, i.imm()) {
                let vm = unsafe { vm(cx) };
                let data = vm.object_data(object)?;
                hint_private_field(vm, ip, i.imm(), data.shape(), slot);
                return vm.heap_reads().property_get(data, slot).map(PrivateRead::Value);
            }
            let brand = private_brand_in_lane(cx, ip, code, i.imm())?;
            let vm = unsafe { vm(cx) };
            match vm.private_field(object, brand) {
                Some(value) => Some(PrivateRead::Value(value)),
                // A method or accessor of a brand the object carries (`Vm::site_private_element`).
                None if vm.has_private_name(object, brand) => {
                    let element = vm.site_private_element(ip.addr(), brand, object)?;
                    // The brand is the shape's (a key under the brand's home), so the site's
                    // next reads of this shape answer the element from the shape alone, where
                    // the home is the class's that owns the name.
                    if let Some(shape) = vm.object_data(object).map(|data| data.shape())
                        && vm.private_shape_cacheable(shape)
                        && vm.private_classes.register(i.imm(), brand.home)
                    {
                        vm.private_site_homes.record_brand_shape(ip.addr(), brand.home, shape);
                    }
                    Some(element_read(element, object))
                }
                None => None,
            }
        });
        match read {
            Some(PrivateRead::Value(value)) => {
                r.set(i.a, value);
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
            // The getter runs as the read's activation, as a cached accessor's does.
            Some(PrivateRead::Getter(object, getter)) => match activate_getter_in_lane(cx, r, ip, getter, object, None) {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
                Err(next) => next,
            },
            None => sibling!(row_get_field_general, r, ip, Carried::script_in(cx, view)),
        }
    }
}

script_handler! {
    /// A store's general write past the generated row (`GENERAL_SITE`, `POLY_SITE`, `ADD_SITE`):
    /// the activation path of `SetField` and its strict form, on the receiver register.
    #[inline(never)]
    fn row_set_object_field_miss<>(cx, r, ip, i, view) {
        resume!(cx, row_set_field_miss_path(cx, r, ip, r.get(i.b), i))
    }
}

script_handler! {
    /// `row_set_object_field_miss` for `SetThisField` and `SetThisFieldStrict`, on `this`.
    #[inline(never)]
    fn row_set_this_field_miss<>(cx, r, ip, i, view) {
        let Some(this) = (unsafe { this_in_lane(cx, r) }) else {
            return exit(ip);
        };
        resume!(cx, row_set_field_miss_path(cx, r, ip, this, i))
    }
}

script_handler! {
    /// A store whose site has proved neither its add nor its shape (`UNREAD`, `TRACKED_SITE`,
    /// `NARROW_SITE`) missed its row and its VM store: the site the record moves to, by what the
    /// site observed (`ADD_SITE`, `POLY_SITE`, `GENERAL_SITE`), then the general write's
    /// activation path.
    #[inline(never)]
    fn row_set_field_classify<OP: u16, THIS: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let polymorphic = vm.field_site_polymorphic(i.c);
        let adds = vm.field_site_adds(i.c);
        // A receiver outside the object records (a RegExp's or a function's cell) hits the own
        // store's logged path only at one shape, so a polymorphic site of one takes the general
        // cached write from now on.
        let cell_receiver = !THIS && r.get(i.b).direct_object_index().is_none() && vm.is_object_like(r.get(i.b));
        // A private field's store takes the private element path (`row_set_field_miss_path`).
        let handler: Option<Handler> = if vm.is_private_name(i.imm()) {
            Some(generated_row!(OP, ACC_NONE, GENERAL_SITE))
        } else if adds && !polymorphic && vm.field_add_atom_eligible(i.imm()) {
            Some(generated_row!(OP, ACC_NONE, ADD_SITE))
        } else if polymorphic && !cell_receiver {
            Some(generated_row!(OP, ACC_NONE, POLY_SITE))
        } else if adds || polymorphic {
            Some(generated_row!(OP, ACC_NONE, GENERAL_SITE))
        } else {
            None
        };
        if let Some(handler) = handler {
            requicken(ip, handler);
        }
        if THIS {
            sibling!(row_set_this_field_miss, r, ip, Carried::script_in(cx, view))
        }
        sibling!(row_set_object_field_miss, r, ip, Carried::script_in(cx, view))
    }
}

/// The edge a decided comparison takes: the jump is taken where it fails.
#[inline(always)]
const fn holds_edge(holds: bool) -> quench_runtime::staged::Edge {
    if holds { quench_runtime::staged::Edge::Following } else { quench_runtime::staged::Edge::Target }
}

script_handler! {
    /// The comparison jump's miss arm (`Miss::CompareChain`), for a pair its generated row leaves:
    /// the relations the lane decides without guest code (`relation_other`); any other takes
    /// the general path. Out of line, so the staged case keeps few values live.
    #[inline(never)]
    fn binary_false_chain_miss<BACKWARD: bool>(cx, r, ip, i, view) {
        let (Some(left), Some(right)) =
            (operand_value_in_lane(cx, r, i.b), operand_value_in_lane(cx, r, i.c))
        else {
            return exit(ip);
        };
        // SAFETY: the record is a comparison record of the running view.
        unsafe { note_operands(cx, ip, left, right) };
        let Some(holds) = relation_other(u32::from(i.a), left, right) else {
            return exit(ip);
        };
        jump_on_edge!(cx, r, ip, BACKWARD, holds_edge(holds), Carried::script_in(cx, view))
    }
}





/// The strict operators `lane_jump_strict_constant_false` decides.
const STRICT_INEQUALITY_OPERATOR: u32 = oxc_ast::ast::BinaryOperator::StrictInequality as u32;







/// The tests the general loop decides past Number comparison without running guest code:
/// identity equality, an ordinary `instanceof`, BigInt order, and the truth of a Number
/// operator's result.
/// Out of line, so the comparison handlers keep their Number path's shape.
#[cold]
#[inline(never)]
fn relation_other(operator: u32, left: Value, right: Value) -> Option<bool> {
    let vm = unsafe { vm(context_in_lane()) };
    // A nullish value is unequal to every Number under both loose and strict equality.
    // The Number/nullish pair cannot invoke conversion hooks or `document.all`; keep this
    // check in the shared cold fallback so it does not enlarge each hot comparison handler.
    if operator <= STRICT_INEQUALITY_OPERATOR
        && ((left.is_null() || left.is_undefined()) && right.as_number().is_some()
            || (right.is_null() || right.is_undefined()) && left.as_number().is_some())
    {
        return Some(matches!(operator, 1 | STRICT_INEQUALITY_OPERATOR));
    }
    // An ordering operator (`<`, `<=`, `>`, `>=`) is decided by its operand pair alone: the
    // membership, prototype and identity tests below never answer it.
    // `==` and `===` share `Equal` (`NumberRelation::from_operator`), which is no ordering: they take
    // the identity and string tests below.
    if quench_runtime::vm::operations::NumberRelation::from_operator(operator)
        .is_some_and(|relation| !matches!(relation, quench_runtime::vm::operations::NumberRelation::Equal | quench_runtime::vm::operations::NumberRelation::NotEqual))
    {
        return vm.string_relation(operator, left, right)
            .or_else(|| vm.bigint_relation(operator, left, right))
            .or_else(|| vm.primitive_comparison(operator, left, right))
            .or_else(|| vm.number_binary(operator, left, right)?.truthiness());
    }
    vm.instanceof_truthy(operator, left, right)
        .or_else(|| vm.plain_in(operator, left, right))
        .or_else(|| vm.identity_equality(operator, left, right))
        .or_else(|| vm.string_relation(operator, left, right))
        // A test of an arithmetic or bitwise result (`if (flags & MASK)`): the Number
        // operator's value, whose ToBoolean runs no guest code.
        .or_else(|| vm.bigint_relation(operator, left, right))
        // The remaining primitive pairs, whose conversions are pure: `undefined > 5`,
        // `flag == 1`, `"3" == 3`.
        .or_else(|| vm.primitive_comparison(operator, left, right))
        .or_else(|| vm.number_binary(operator, left, right)?.truthiness())
}

/// A taken jump. Backward jumps, the only ones that close a loop, are
/// safepoints: they leave the lane when a collection is due; forward jumps
/// dispatch directly. Every jump's direction is fixed when the view is
/// derived. Leaving hands the whole instruction to the general path, so
/// `commit`, the register write the instruction makes before its jump,
/// applies only when the lane takes the jump itself.
#[inline(always)]
fn take_jump(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    backward: bool,
    view: Carried,
    commit: Option<(u16, Value)>,
) -> Resumed {
    let jump = unsafe { (*ip).jump() };
    // SAFETY: the view records the distance to a validated jump target.
    let target = unsafe { ip.byte_offset(jump) };
    // The backward edge spends fuel through the run's context the handler already holds
    // (`script_fuel_out`): a decrement of `LaneContext::fuel` and a branch, with no
    // thread-local load of the counter's address (`fuel_out`).
    if backward && script_fuel_out(cx) {
        return Resumed::Exit(match commit {
            None => safepoint_at(ip, target, view.acc),
            Some((register, value)) => safepoint_writing(r, ip, target, view.acc, register, value),
        });
    }
    if let Some((register, value)) = commit {
        r.set(register, value);
    }
    Resumed::At(r, target, view)
}

/// Count one backward jump of a crediting run (`LaneContext::backedges_left`); at the budget's
/// end, credit the running Wasm function with one interpreter entry for the loop work
/// (`ProgramStore::note_wasm_tier_backedges`) and refill it. A function entered once and
/// looping hot (JetStream's tsf and zlib) then promotes at its next entry, as one called often
/// does.
fn count_backedge(cx: *mut LaneContext) {
    // SAFETY: called from `safepoint_at` with the run's context.
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
        .top_frame()
        .map_or_else(|| machine.frames[unsafe { (*cx).frame() }].function, |activation| activation.function());
    machine.programs.note_wasm_tier_backedges(program, function);
    unsafe { (*cx).backedges_left = Some(machine.programs.tiers().wasm.backedges_per_entry) };
}

script_handler! {
    /// A loop's backward jump in compiled execution (`LoopSite`): the jump's safepoint, then
    /// the loop's native code once the jump is hot and the region compiled. Native code
    /// resumes the lane where control leaves the region, or leaves to the general path at an
    /// instruction outside its subset; a run its entry guards decline takes the jump.
    fn loop_jump_miss<>(cx, r, ip, i, view) {
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
        if script_fuel_out(cx) {
            return safepoint_at(ip, header, Slot::unset());
        }
        // A refused compile (`LoopSite::refuse_warming`) is one epoch compare and a countdown: the back edge is a plain jump.
        if site.refused_now() {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        // A loop inside an admitted loop that still enters native code runs as plain jumps there: the outermost
        // such loop takes the region (`install_loop_sites`), and this one neither warms up nor enters its own.
        if site.inside_admitted_loop() {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        if site.interprets() {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        // A site backing off after declined entries runs the header as a declined entry does
        // (`LoopSite::note_decline`).
        if site.skips_entry() {
            lane_at_header!(cx, r, header, site)
        }
        // The header rule (`enter_loop_body`): a ready body first, then the region.
        if let Some(attempt) = loop_body::enter_loop_body(cx, r, ip, site) {
            lane_attempt!(cx, r, attempt);
        }
        // A loop whose body is still to come keeps its backward jump, which a loop no region
        // runs would otherwise retire before the body is planned.
        if unsafe { vm(cx) }.programs.js_jit_lane_methods_enabled() && site.body.borrow().pending() && site.region.get().is_none() {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        // A loop no region runs takes its plain jump from now on.
        if site.promotion_only {
            loop_retire_in_lane(cx, ip, site);
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        if site.analysis_failure.is_some_and(|failure| !failure.is_call_boundary()) {
            // An instruction no region runs leaves the loop to a body, which runs what it can
            // natively and leaves at the rest (`loop_body::offer_body`).
            if loop_body::offer_body(cx, r, site) { next!(cx, r, header, Carried::script_in(cx, view)) }
            loop_retire_in_lane(cx, ip, site);
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        if !site.may_run_natively(unsafe { vm(cx) }.heap_reads().memory_policy()) {
            loop_retire_in_lane(cx, ip, site);
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        let region = match site.region.get() {
            Some(region) => region,
            // The calls do not run inlined: the loop leaves at them, as at any other call.
            None => match inlining_region_in_lane(cx, r, site).or_else(|| {
                let _ = site.inline_region.set(None);
                loop_tier::method_call_region_in_lane(cx, site)
            }) {
                Some(region) => site.region.get_or_init(|| region),
                // A call no region runs (a native's, a callee the inliner cannot take) is a
                // loop body's: the body planner inlines what it knows (`native_calls`) and
                // starts at the header, so the loop runs natively without a function entry.
                None if loop_body::offer_body(cx, r, site) => next!(cx, r, header, Carried::script_in(cx, view)),
                None => {
                    site.entering.set(false);
                    loop_retire_in_lane(cx, ip, site);
                    next!(cx, r, header, Carried::script_in(cx, view))
                }
            },
        };
        let region = if !site.call_boundary_pcs.is_empty() && region.inlining.is_none() {
            site.inline_region.get_or_init(|| inlining_region_in_lane(cx, r, site).map(Box::new)).as_deref().unwrap_or(region)
        } else { region };
        match loop_native_in_lane(cx, r, ip, header, site, region) {
            LoopAttempt::Declined => lane_at_header!(cx, r, header, site),
            LoopAttempt::At(next) => next!(cx, r, next, Carried::script_in(cx, view)),
            LoopAttempt::Leave(next) => next,
            LoopAttempt::Resumed(resumed) => resume!(cx, resumed),
            LoopAttempt::Carry(registers, next, carried) => next!(cx, registers, next, carried),
        }
    }
}

/// The header record's own handler at `header`, as the interpreter runs it: the one `site`
/// holds while it holds the record (`loop_header_miss`), which the backward jump thus does
/// not try again.
macro_rules! lane_at_header {
    ($cx:expr, $r:expr, $header:expr, $site:expr) => {{
        let held = $site.header_handler.get();
        if held.is_null() || held == RETIRED_HEADER {
            next!($cx, $r, $header, Carried::script($cx))
        }
        // SAFETY: the site holds the record's handler, which has the shared signature.
        let handler: Handler = unsafe { std::mem::transmute::<*const (), Handler>(held) };
        sibling!(handler, $r, $header, Carried::script($cx))
    }};
}
use lane_at_header;

/// Continues the lane as a loop site's attempt says, or falls through where it declined.
macro_rules! lane_attempt {
    ($cx:expr, $r:expr, $attempt:expr) => {
        match $attempt {
            LoopAttempt::Declined => {}
            LoopAttempt::At(next) => next!($cx, $r, next, Carried::script($cx)),
            LoopAttempt::Leave(next) => return next,
            LoopAttempt::Resumed(resumed) => resume!($cx, resumed),
            LoopAttempt::Carry(registers, next, carried) => next!($cx, registers, next, carried),
        }
    };
}
use lane_attempt;

/// What a loop site's native attempt leaves the lane to do.
enum LoopAttempt {
    /// Nothing ran natively: the header record runs as the interpreter's.
    Declined,
    /// Continue at this record.
    At(Ip),
    /// Return this from the handler: an exit, or a safepoint's continuation.
    Leave(Ip),
    /// Continue in the activation this completion resumes.
    Resumed(Resumed),
    /// Continue at this record of these registers with this carried state.
    Carry(Registers, Ip, Carried),
}

/// `loop_native_in_lane`'s entry and run of `region` into `entry`: how it continues, or `None`
/// where its code is awaited or its entry record cannot be built (the site notes the decline).
#[inline(always)]
fn loop_run_native_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    site: &LoopSite,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
) -> Option<LoopContinuation> {
    let mut tier = site.tier.borrow_mut();
    if tier.awaits_code(unsafe { vm(cx) }, site) {
        // The code is compiling: the back edges until the next look run the header untried
        // (`skips_entry`) rather than each paying the whole entry attempt (~200 Ir) to count
        // the wait down.
        site.skipped_entries.set(tier.take_wait());
        drop(tier);
        site.count_decline(RegionDecline::Awaiting);
        return None;
    }
    drop(tier);
    let views = match loop_entry_checked_in_lane(cx, r, region, entry) {
        Ok(views) => views,
        Err(decline) => {
            site.count_decline(decline);
            site.note_decline();
            return None;
        }
    };
    // The tier's state is borrowed only here, so the dispatch after it holds no borrow.
    let continuation = loop_run_in_lane(cx, r, site, region, entry, views, site.header);
    count_continuation(cx, &continuation);
    if matches!(continuation, LoopContinuation::Jump) { site.note_decline(); }
    Some(continuation)
}

/// Runs `site`'s region on `r` from its `header` record, reached at `record` (the backward
/// jump, or the header itself): the entry guards, then native code, which resumes the lane
/// where control leaves the region or leaves to the general path. Once the region has code,
/// the site holds its header record, so a loop entered from outside runs natively from its
/// first iteration.
#[inline(always)]
fn loop_native_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    record: Ip,
    header: Ip,
    site: &LoopSite,
    region: &crate::compiled::LoopRegion,
) -> LoopAttempt {
    // SAFETY: the region carries its validated header and latch PCs, and every instruction
    // record remains contiguous in this lane view.
    let at = |pc: u32| unsafe { header.offset(pc as isize - site.header as isize) };
    if site.skips_entry() { return LoopAttempt::Declined; }
    // The record is filled where it lives, so its words are neither cleared nor copied.
    let mut entry = crate::compiled::LoopEntry::new();
    let Some(continuation) = loop_run_native_in_lane(cx, r, site, region, &mut entry) else { return LoopAttempt::Declined };
    loop_hold_header_in_lane(header, site);
    match continuation {
        LoopContinuation::Jump | LoopContinuation::Compiling => LoopAttempt::Declined,
        LoopContinuation::Resume(pc) => LoopAttempt::At(at(pc)),
        LoopContinuation::Exit(pc) if loop_body::left_for_tier_up(cx, site, pc) => LoopAttempt::At(at(site.header)),
        LoopContinuation::Exit(pc) => { count_arithmetic_leave(cx, site, pc, region.header..=region.backedge); LoopAttempt::Leave(exit(at(pc))) }
        LoopContinuation::Inlined { frame, pc } => { let frame = region.inlining.as_ref().expect("an inlining region").frames[frame];
            LoopAttempt::Leave(exit(inlined_activation_in_lane(cx, r, region, &entry, frame, at(frame.call), pc))) }
        LoopContinuation::Recovered(resumed) => loop_body::loop_attempt(cx, record, resumed),
        LoopContinuation::Safepoint => LoopAttempt::Leave(safepoint_at(record, header, Slot::unset())),
        // SAFETY: the call set the callee's activation and its resume record before the body
        // left (`body_activate_in_lane`), so the run leaves at a record of that view.
        LoopContinuation::Callee => LoopAttempt::Leave(exit(unsafe { (*cx).callee_resume })),
        LoopContinuation::Completed(pc, value) =>
            LoopAttempt::Resumed(return_value_general_in_lane(cx, at(pc), value)),
        LoopContinuation::Threw(pc) => LoopAttempt::Leave(raised_exit_in_lane(cx, at(pc))),
    }
}

/// Counts a leave at `pc` (`LoopTier::left_at`) where it is a Number operator, whose int32
/// speculation a compilation can drop, an element read, whose observed type it can drop, an
/// element store, whose append helper it can compile, a field store, whose VM store it can call
/// (`Lowering::store_leaves`), or a capture read, whose root-held binding
/// it can read by helper; a leave at any other guard (a shape, say) widens nothing.
#[cold]
fn count_arithmetic_leave(cx: *mut LaneContext, site: &LoopSite, pc: u32, region: std::ops::RangeInclusive<u32>) {
    if !region.contains(&pc) { return; }
    // SAFETY: the lane's VM is live, and nothing borrows it across the read.
    let machine = unsafe { vm(cx) };
    let counted = machine.programs.get(machine.active_program).as_deref().and_then(|program| {
        let body = program.function(site.function() as usize);
        body.code.get(pc as usize).and_then(|packed| {
            let instruction = body.widened(*packed);
            let op = instruction.op();
            crate::compiled::base_region::leave(site.function(), pc, op);
            let stores = quench_runtime::staged::stores_field(op) && matches!(op.staged(), Some(quench_runtime::bytecode::Decoder::Property)); // what decodes to `SetField` (`decode_property`), with no decode
            (quench_runtime::staged::computes_numeric_binary(op) || quench_runtime::staged::reads_element(op) || op.stores_element()
                || stores || quench_runtime::staged::miss(op) == quench_runtime::staged::Miss::WalkScopes)
                .then(|| (quench_runtime::staged::reads_element(op), stores))
        })
    });
    match counted {
        Some((_, true)) => site.tier.borrow_mut().left_store(site.tiers, pc as usize),
        Some((read, false)) => site.tier.borrow_mut().left_at(site.tiers, pc as usize, read),
        None => {}
    }
}

/// Gives `site` its `header` record (`LoopSite::hookable`) once its region has code: the
/// record runs `loop_header_miss`, and the site keeps the record's own handler.
fn loop_hold_header_in_lane(header: Ip, site: &LoopSite) {
    // A retired site (`RETIRED_HEADER`) never holds its header again: its jump record is a plain
    // jump and the header's `call_link` a field hint, so `header_site_in_lane` would read garbage.
    if !site.hookable || !site.header_handler.get().is_null() || !site.tier.borrow().is_published() {
        return;
    }
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view.
    unsafe {
        site.header_handler.set((*header).handler);
        (*header.cast_mut()).handler = loop_header_miss as Handler as *const ();
    }
}

/// The site whose header record is at `ip` (`LoopSite::hookable`): the header's `call_link`
/// is the distance, in records, to the site's backward jump, whose operand is the site
/// (`install_loop_sites`). A held header's jump still holds it: retiring the site gives the
/// header its own handler back first (`loop_retire_in_lane`).
///
/// # Safety
/// `ip` is a held header record of a live view.
#[inline(always)]
unsafe fn header_site_in_lane(ip: Ip) -> *const LoopSite {
    // SAFETY: the jump lies `call_link` records past its header in the same view.
    unsafe { (*ip.add(usize::from((*ip).call_link.get()))).loop_site() }
}

script_handler! {
    /// A loop's header record while its site holds it (`loop_hold_header_in_lane`): arriving at
    /// the header enters the loop's native code as its backward jump does, and otherwise
    /// runs the record's own handler.
    fn loop_header_miss<>(cx, r, ip, i, view) {
        // SAFETY: a record runs this handler only while a site holds it.
        let site = unsafe { &*header_site_in_lane(ip) };
        if !script_below_floor(cx)
            && let Some(attempt) = loop_body::enter_loop_body(cx, r, ip, site)
        {
            lane_attempt!(cx, r, attempt);
        }
        if !script_below_floor(cx)
            && site.enters()
            && let Some(region) = site.active_region()
        {
            match loop_native_in_lane(cx, r, ip, ip, site, region) {
                LoopAttempt::Declined => {}
                LoopAttempt::At(next) => next!(cx, r, next, Carried::script_in(cx, view)),
                LoopAttempt::Leave(next) => return next,
                LoopAttempt::Resumed(resumed) => resume!(cx, resumed),
                LoopAttempt::Carry(registers, next, carried) => next!(cx, registers, next, carried),
            }
        }
        lane_at_header!(cx, r, ip, site)
    }
}


/// Counts an entry of `function`'s lane activation toward its lane body (`own`), or, `now`,
/// specializes it at once, from the targets and callees the program's views hold now. Whether the body
/// is still warming or specialized (`LaneView::specialize_lane_body`).
fn specialize_in_lane(cx: *mut LaneContext, own: &LaneView, function: u32, now: bool, parameters: &[Value], looped: Option<(u64, u64)>) -> bool {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let diagnostics = vm.programs.js_jit_diagnostics_enabled();
    // The entry activation's array parameters, sampled: a lane body's loops read them typed (`observed_reads`).
    // Where the body comes from is the specializer's decision (`admission::route_for_lane`): an ahead image (the
    // ladder's first body, no warm-up; one that left too often `respecializes` on the JIT's route), the stencil
    // tier at once, a wait for callees still compiling, or the lane body tier.
    let base_only = crate::compiled::admission::stencil_only();
    let ahead_image = !own.respecializes() && vm.programs.ahead_images(unsafe { (*cx).program }, code).is_some();
    if crate::compiled::admission::route_for_lane(crate::compiled::admission::LaneRouteFacts { ahead_image, base_only, callees_compiling: false })
        == crate::compiled::admission::LaneRoute::Ahead
    {
        // An image body takes a claimed prologue, whichever entry reached it first: its entry reads the function there.
        if let Some(body) = code.function_get(function as usize) && unsafe { (*own.prologue.get()).handler }.is_null() { own.claim_for_tier(tier_prologue_row(), function, body.params, !body.is_arrow); }
        let published = own.entry_published();
        let installed = own.install_ahead_lane_body(body_entry_row(), || crate::compiled::admission::ahead_unit(function).and_then(|unit| vm.install_ahead_body(unit, code, function, None, &[])));
        if !published && own.entry_published() {
            unsafe { self::vm(cx) }.unprove_view_entries(own);
        }
        return installed;
    }
    let arrays = loop_tier::sampled_arrays(vm, parameters.iter().copied());
    let specialize = |work: crate::compiled::LaneBodyWork| {
        let work = match work {
            // A looping caller's trips pay for the body as its own iterations do (`looped`).
            crate::compiled::LaneBodyWork::Start { generic, shrinks, observed, looped: seen, called, caller, .. } =>
                crate::compiled::LaneBodyWork::Start { generic, shrinks, observed, looped: looped.or(seen), called, caller, arrays: &arrays },
            poll => poll,
        };
        let direct = |vm: &Vm| code.function_get(function as usize).map(|body| direct_callees_in_lane(vm, own, function, code, body)).unwrap_or_default();
        let starting = matches!(work, crate::compiled::LaneBodyWork::Start { .. });
        // A start under the lane body tier waits for callees still compiling (`body_tier_in_lane::callees_compiling`).
        let callees_compiling = starting && !base_only && body_tier_in_lane::callees_compiling(vm, own, code.function_get(function as usize));
        match crate::compiled::admission::route_for_lane(crate::compiled::admission::LaneRouteFacts { ahead_image: false, base_only, callees_compiling }) {
            // `--b-jit` stitches its body at once: it calls the callees its sites recorded by their entries and waits for none.
            crate::compiled::admission::LaneRoute::Base => {
                let crate::compiled::LaneBodyWork::Start { generic, .. } = work else { unreachable!("a stencil body never compiles in the background") };
                let unit = match vm.base_unit_or_outcome(code, function, parameters, own) { Ok(unit) => unit, Err(outcome) => return outcome };
                return vm.base_body_work(unit, code, function, generic, parameters, own, &direct);
            }
            crate::compiled::admission::LaneRoute::Deferred => return crate::compiled::LaneBodyOutcome::Deferred,
            crate::compiled::admission::LaneRoute::Ahead => unreachable!("an ahead image's program returned above"),
            crate::compiled::admission::LaneRoute::Lane => {}
        }
        let started: Option<Vec<crate::compiled::LaneExit>> = match &work { crate::compiled::LaneBodyWork::Start { generic, .. } => Some(generic.to_vec()), _ => None };
        let outcome = vm.body_work_in_lane(code, function, &|function, pc| unit_targets_in_lane(cx, function, pc), &|function, header| unit_trips_in_lane(cx, function, header), &|callee| callee_captures_in_lane(cx, callee), &direct, work, None);
        // The tier model picked b-jit for good (`tier_model::Pick::Base`): u-jit stays ahead of
        // b-jit, so a function it would not beat takes the stencil body, stitched at once.
        if let (crate::compiled::LaneBodyOutcome::Base, Some(generic)) = (&outcome, &started) {
            let unit = match vm.base_unit_or_outcome(code, function, parameters, own) { Ok(unit) => unit, Err(outcome) => return outcome };
            return vm.base_body_work(unit, code, function, generic, parameters, own, &direct);
        }
        if diagnostics && let crate::compiled::LaneBodyOutcome::Published(method) = &outcome {
            if let Some(method) = method.lease() { own.tier().body_stats.compiled(function, &method.inlined); }
        }
        outcome
    };
    let published = own.entry_published();
    let warming = if now {
        own.specialize_lane_body_now(body_entry_row(), looped.is_some(), specialize);
        own.body_view_in_lane().is_some()
    } else {
        own.specialize_lane_body(body_entry_row(), false, specialize)
    };
    // Every cell body is published here (`tier_machine::publish` from `specialized`): the
    // proved calls of the view, which skip the cell body, lose their proof.
    if !published && own.entry_published() {
        unsafe { self::vm(cx) }.unprove_view_entries(own);
    }
    warming
}

#[quench_macros::ext(DispatchFastVm)]
impl Vm {
    /// `LaneStack::unprove_view_entries` on every lane stack that may run again: the live one,
    /// the parked runs' (`nested_lane_run`) and the spares, whose entries a later run of the
    /// same generation admits.
    #[cold]
    #[inline(never)]
    fn unprove_view_entries(&mut self, view: &LaneView) {
        let view = std::ptr::from_ref(view);
        self.lane_stack.unprove_view_entries(view);
        for parked in &mut self.lane_roots { parked.stack.unprove_view_entries(view); }
        for spare in &mut self.spare_lane_stacks { spare.unprove_view_entries(view); }
    }
}

/// Gives a loop site that will not enter native code again its plain backward jump back,
/// so each iteration takes the jump without the site's checks.
fn loop_retire_in_lane(cx: *mut LaneContext, ip: Ip, site: &LoopSite) {
    // A hot loop no region runs makes its function worth a body past the entry warm-up: its
    // calls have run, so their sites have linked, and its next entry runs the loop natively.
    // A body still warming after the loop's trips asked for it is refused under the body's epoch
    // (`LoopSite::refuse_warming`): its back edges are plain jumps until the epoch moves or the
    // wait runs out, so a function whose entries do not advance costs no per-edge checks.
    // SAFETY: the program's views outlive the lane run.
    if unsafe { vm(cx) }.programs.js_jit_lane_methods_enabled()
        && let Some(own) = unsafe { &*(*cx).views }.get(site.function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)
    {
        specialize_in_lane(cx, own, site.function, true, &[], Some((site.span(), site.body_iterations())));
        // A body the model priced with the loop's work and found unpaid gains nothing from the loop asking
        // again: the loop retires to its plain jump, which costs no countdown per back edge (TOD-2062).
        if own.warming() && !own.body_unpaid() { return site.refuse_warming(own, site.tiers.loops.warmup_taken); }
    }
    // SAFETY: as `requicken`: the record lives in an `UnsafeCell` of the running view, and
    // the jump reads its distance from the operand, which the site kept (`LoopSite::jump`).
    // A header record the site holds gets its own handler back.
    let header = unsafe { ip.byte_offset(site.jump) };
    loop_release_header_in_lane(header, site);
    // A run of this site still on the stack (a native attempt that retired it) must not hold
    // the header again on its way out (`loop_hold_header_in_lane`).
    site.header_handler.set(RETIRED_HEADER);
    // A retired loop enters no native code, so the loops inside it take their own regions (`LoopSite::inside_admitted_loop`).
    site.entering.set(false);
    // No site holds the header from now on, so its record, which linked this site alone, takes
    // field hints again (`holds_loop_site`).
    if site.hookable { unsafe { (*header).call_link.set(NO_CALL_LINK) }; }
    unsafe { (*ip.cast_mut()).operand = LaneOperand { jump: site.jump }; }
    requicken(ip, jump_row::<JUMP>(true));
    // The site kept the update before the jump from fusing with it (`install_loop_sites`
    // runs before `fuse_rows`): the plain jump takes the loader's pair fusion now, with the
    // `FUSED_TARGET` record where the header's target run holds (`update_fusion`'s `target`).
    if site.backedge > site.header {
        // A header comparison that ran while the site held its loop took its formed instance (`row_jump`),
        // which no target run names: it goes back to its plain row (and its record's `mode` to the zero of an
        // unfused record), as the loader's `fuse_rows` saw it, so the back edge fuses the test again.
        // SAFETY: the header is a record of the running view, on this thread.
        if let Some(plain) = lane_frame::unformed_compare_jump(unsafe { (*header).handler }) {
            unsafe { (*header.cast_mut()).mode = 0 };
            requicken(header, plain);
        }
        // SAFETY: the record before the backedge is the loop body's, in the same view as the jump.
        let update = unsafe { ip.sub(1) };
        if let Some((fused, mode)) = update_fusion(unsafe { &*update }, unsafe { &*ip }, Some(header)) {
            // SAFETY: the record is the running view's, on this thread; its handler is requickened next.
            unsafe { (*update.cast_mut()).mode = mode };
            requicken(update, fused);
        }
    }
}

/// Restore the semantic header at the event that replaces its native owner.
/// `LoopSite::header_handler` of a retired site (`loop_retire_in_lane`): no record, so the site
/// never holds its header again.
const RETIRED_HEADER: *const () = std::ptr::without_provenance(1);

fn loop_release_header_in_lane(header: Ip, site: &LoopSite) {
    let held = site.header_handler.replace(std::ptr::null());
    if !held.is_null() && held != RETIRED_HEADER {
        // SAFETY: the validated site owns this record on the running view's thread.
        unsafe { (*header.cast_mut()).handler = held; }
    }
}

/// Where the lane goes after a loop site's native attempt.
enum LoopContinuation {
    /// Take the backward jump in the interpreter.
    Jump,
    Resume(u32),
    /// The general path executes the instruction at this PC.
    Exit(u32),
    /// The general path executes callee PC `pc` of inlined frame `frame`, in the activation
    /// its call makes (`LaneExit::Inlined`).
    Inlined { frame: usize, pc: u32 },
    /// Native code of a stitched region left inside an inlined callee, and the lane recovered the
    /// activations around it (`inlined_continuation_in_lane`).
    Recovered(LaneBodyLeft),
    /// The backward jump's safepoint ends the execution termination stopped.
    Safepoint,
    /// The region's code compiles: take the jump, which declines nothing.
    Compiling,
    /// The region completed its activation with this value at this record.
    Completed(u32, Value),
    /// The region raised the exception in `Vm::lane_raised` at this record: unwind from it.
    Threw(u32),
    /// A call activated the callee's lane body: leave the run in that activation.
    Callee,
}

/// Counts what a loop run came to: in the VM's tier events, and under `QUENCH_TRACE=js-jit` in the report's.
fn count_continuation(cx: *mut LaneContext, continuation: &LoopContinuation) {
    use crate::compiled::base_region::{count, Event as Region};
    let (region, tier) = match continuation {
        LoopContinuation::Jump | LoopContinuation::Compiling => (Region::Declined, None),
        LoopContinuation::Exit(_) | LoopContinuation::Inlined { .. } | LoopContinuation::Recovered(_) | LoopContinuation::Safepoint | LoopContinuation::Callee | LoopContinuation::Threw(_) => (Region::Left, Some(Event::OsrLeft)),
        LoopContinuation::Resume(_) | LoopContinuation::Completed(..) => (Region::Entered, Some(Event::OsrEntered)),
    };
    count(region);
    if let Some(event) = tier { unsafe { vm(cx) }.programs.tier_events().record(event); }
}

/// Compiles `site`'s region if it has no current code, for the element `views` its entry
/// proved, then runs it on `r` with `entry`. A representation an outcome shows wrong is
/// widened, and the code dropped for the next compilation.
#[inline(never)]
fn loop_run_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    site: &LoopSite,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
    views: crate::compiled::ElementViews,
    from: u32,
) -> LoopContinuation {
    use crate::compiled::{LaneExit, RegionRun};
    let mut tier = site.tier.borrow_mut();
    if !tier.select(site.tiers, views) {
        return site.declined(RegionDecline::Views);
    }
    if !tier.is_published() {
        // SAFETY: the lane's VM is live, and nothing borrows it across the read.
        if !tier.may_run(site.tiers, unsafe { vm(cx) }.heap_reads().memory_policy()) {
            return site.declined(RegionDecline::NoCompile);
        }
        let number_arrays = loop_tier::number_arrays(unsafe { vm(cx) }, r, region);
        let int_entries = loop_tier::int_entries(r, region);
        let typed_captures = loop_tier::typed_capture_slots(cx);
        let int_captures = loop_tier::int_capture_slots(cx);
        loop_compile_in_lane(cx, r, site, region, &mut tier, &number_arrays, &int_entries, &typed_captures, &int_captures);
        if !tier.is_published() {
            site.count_decline(RegionDecline::Compiling);
            return LoopContinuation::Compiling;
        }
    }
    // A b-jit region due to move to u-jit starts the u-jit compile and runs on meanwhile
    // (`LoopTier::promote`); one not yet due runs until the u-jit region would be ahead.
    if tier.promotion_due() && tier.may_run(site.tiers, unsafe { vm(cx) }.heap_reads().memory_policy()) {
        let number_arrays = loop_tier::number_arrays(unsafe { vm(cx) }, r, region);
        let int_entries = loop_tier::int_entries(r, region);
        let typed_captures = loop_tier::typed_capture_slots(cx);
        let int_captures = loop_tier::int_capture_slots(cx);
        loop_compile_in_lane(cx, r, site, region, &mut tier, &number_arrays, &int_entries, &typed_captures, &int_captures);
        if !tier.is_published() { return LoopContinuation::Compiling; }
    }
    if let Some(budget) = tier.backedge_budget(site.tiers) { entry.set_backedge_budget(budget); }
    // The code alone is held for the run (its publication is the tier's): one count, not two.
    let code = tier.code.as_ref().and_then(|publication| publication.lease()).expect("compiled above");
    drop(tier);
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
    let run = match unsafe { region_run_in_lane(cx, r, entry, (&*code, region), (site.function, from), (machine, program, interrupt)) } {
        Ok(run) => run,
        // The lane admitted no run, so the region's code entered nothing and the interpreter takes the jump.
        Err(decline) => return site.declined(decline),
    };
    if site.diagnostics {
        loop_diagnose_in_lane(cx, r, site, region, run, program);
    }
    let RegionRun { exit, nested } = run;
    if exit.entered_nothing() {
        // An entry guard missed: registers are untouched, so the backward jump runs in the interpreter.
        site.tier.borrow_mut().missed_entry(site.tiers);
        return site.declined(if matches!(exit, LaneExit::Stub) { RegionDecline::Stub } else { RegionDecline::EntryContract });
    }
    if matches!(exit, LaneExit::Terminated) { return LoopContinuation::Safepoint; }
    crate::compiled::admission::retire_after_native(unsafe { vm(cx) }, exit, || native_frames::left_native_frames(cx));
    // A run pays for its entry with the iterations it completed, wherever it leaves.
    site.note_run(entry.backedge_count());
    if nested {
        return region_left_in_lane(cx, r, site.function, exit);
    }
    match exit {
        LaneExit::At(pc) if region.holds_exit(exit) => LoopContinuation::Exit(pc as u32),
        LaneExit::At(pc) => LoopContinuation::Resume(pc as u32),
        LaneExit::Inlined { frame, pc } => inlined_continuation_in_lane(cx, r, site, (region, &code), (frame, pc)),
        // The call made the callee's activation, whose own records the lane runs next.
        LaneExit::Callee => LoopContinuation::Callee,
        LaneExit::Returned { pc, value } => LoopContinuation::Completed(pc as u32, value),
        LaneExit::Threw { pc } => LoopContinuation::Threw(pc as u32),
        LaneExit::Entry | LaneExit::Stub | LaneExit::Terminated => unreachable!("handled before the run paid"),
    }
}

/// The diagnostics of a loop site's native attempt that ended in `run`, under its `site.diagnostics`.
fn loop_diagnose_in_lane(cx: *mut LaneContext, r: Registers, site: &LoopSite, region: &crate::compiled::LoopRegion, run: crate::compiled::RegionRun, program: ProgramId) {
    use crate::compiled::LaneExit;
    let bump = |count: &std::cell::Cell<u64>| count.set(count.get().saturating_add(1));
    bump(&site.native_entry_attempts);
    if run.nested {
        bump(&site.native_region_runs);
        return;
    }
    match run.exit {
        LaneExit::Entry | LaneExit::Stub => {
            if quench_runtime::trace::Channel::OsrReject.on() {
                eprintln!("QUENCH_JS_JIT_OSR_ENTRY_MISS function={} header={}", site.function, site.header);
            }
            bump(&site.entry_guard_misses);
        }
        LaneExit::Terminated => {}
        LaneExit::Inlined { .. } => {
            bump(&site.native_region_runs);
            bump(&site.native_exits);
        }
        // A guard's exit inside the loop; the poll's leave for a published body is a handover, not one.
        LaneExit::At(pc) if region.holds_exit(run.exit) && !loop_body::left_for_tier_up(cx, site, pc as u32) => {
            bump(&site.native_region_runs);
            bump(&site.native_exits);
            loop_trace_exit_in_lane(cx, r, site, region, program, pc as u32);
        }
        _ => bump(&site.native_region_runs),
    }
}

/// Attributes a guard exit at `pc` to its recovery PC before the lane resumes, under the opt-in
/// OSR trace; ordinary execution keeps no extra table or per-site allocation.
fn loop_trace_exit_in_lane(cx: *mut LaneContext, r: Registers, site: &LoopSite, region: &crate::compiled::LoopRegion, program: ProgramId, pc: u32) {
    if quench_runtime::trace::Channel::OsrReject.on() {
        eprintln!(
            "QUENCH_JS_JIT_OSR_EXIT program={} function={} header={} pc={pc}",
            program.raw(), site.function, site.header,
        );
        // Read recovered operands only for a non-inlined activation. These
        // getters neither coerce a guest value nor allocate guest storage.
        // Raw state distinguishes index/append/capacity guards without
        // changing native exits or inventing a second guard policy.
        if region.inlining.is_none() {
            // SAFETY: the lane's program and VM outlive this synchronous trace;
            // native execution has returned and restored the canonical registers.
            let body = unsafe { &*(*cx).code }.function_get(site.function as usize);
            if let Some((body, packed)) = body.and_then(|body| body.code.get(pc as usize).map(|packed| (body, *packed))) {
                let instruction = body.widened(packed);
                if quench_runtime::staged::writes_element(instruction.op()) {
                    let key = r.get(instruction.register_c());
                    let base = r.get(instruction.register_b());
                    if let Some(array) = unsafe { vm(cx) }.heap_reads().record_array(base) {
                        eprintln!(
                            "QUENCH_JS_JIT_OSR_SET_INDEX_EXIT program={} function={} header={} pc={pc} key_bits={:016x} key_number={:?} length={} capacity={}",
                            program.raw(), site.function, site.header,
                            key.to_bits(), key.as_number(),
                            array.elements().len(), array.elements().capacity(),
                        );
                    }
                }
            }
        }
    }
}

/// Runs `code`, the region of the loop of `function` over the activation with registers `r`, from
/// its header or the record `from`, where the lane admits the run (`region_admits_in_lane`); a run it
/// does not admit enters nothing, and the `RegionDecline` names why.
///
/// # Safety
/// As `CompiledOsrRegion::run`: `machine` and `interrupt` belong to the running lane.
unsafe fn region_run_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    entry: &mut crate::compiled::LoopEntry,
    (code, region): (&crate::compiled::CompiledOsrRegion, &crate::compiled::LoopRegion),
    (function, from): (u32, u32),
    (machine, program, interrupt): (*mut Vm, ProgramId, *const quench_runtime::execution_interrupt::ExecutionInterrupt),
) -> Result<crate::compiled::RegionRun, RegionDecline> {
    // A job's frameless lane (a generator step) has no frame for the region's activation.
    if code.needs_frame() && !unsafe { &*cx }.has_frame() {
        return Err(RegionDecline::Frame);
    }
    if !region_admits_in_lane(cx, code) {
        // A run the lane lacks room for grows it and leaves at the loop's header, so the next entry admits.
        let grown = frame_holds_window_in_lane(cx, &code.calls) && room_prepare_in_lane(cx, code.calls.recovery_space());
        return Ok(crate::compiled::RegionRun::own(if grown { crate::compiled::LaneExit::At(region.header as usize) } else { crate::compiled::LaneExit::Entry }));
    }
    // SAFETY: the caller's contract.
    Ok(unsafe { code.run(r.0, entry, region, machine, program, function, (*cx).frame(), interrupt, (cx as usize, direct_calls_in_lane(cx)), from) })
}

/// Whether a run of `code`, the region of the loop of `function` the lane is in, may start: the lane
/// has the room to make the activations of the calls it inlines should it leave inside one
/// (`room_admits_in_lane`), and the activation keeps its locals apart from the window past its
/// registers, which the registers of those callees occupy (`CompiledOsrRegion::run_lane`).
#[inline(always)]
fn region_admits_in_lane(cx: *mut LaneContext, code: &crate::compiled::CompiledOsrRegion) -> bool {
    let room = code.calls.recovery_space();
    room.activations == 0 || (room_admits_in_lane(cx, room, NO_PENDING_ACTIVATIONS) && frame_holds_window_in_lane(cx, &code.calls))
}

/// Whether the running activation's register storage holds the windows of the callees `calls` inlines,
/// which start past the function's environment window (`base_region::function::inline_base`): a lane
/// activation's value stack has the room `room_admits_in_lane` proved (`CallCode::recovery_space` covers
/// the reach); a VM frame's register file, sized to its own words, needs that much capacity past its
/// registers (`INLINED_WINDOW_ROOM`, which an environment window reserves).
#[inline(always)]
fn frame_holds_window_in_lane(cx: *mut LaneContext, calls: &crate::compiled::CallCode) -> bool {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        return true;
    }
    frame_reaches_in_lane(cx, calls.caller_registers(), calls.window())
}

/// Whether the running VM frame's register file holds `reach` words past its `registers`, where code
/// writes past them (`base_region::function::inline_base`); a lane activation's storage, at the top of
/// the value stack, is proved by its room instead (`room_admits_in_lane`).
#[inline(always)]
fn frame_reaches_in_lane(cx: *mut LaneContext, registers: u16, reach: usize) -> bool {
    let vm = unsafe { vm(cx) };
    reach == 0 || vm.lane_stack.has_frames()
        || vm.frames.get(unsafe { (*cx).frame() }).is_some_and(|frame| frame.registers.capacity() >= usize::from(registers) + reach)
}


/// Whether the running activation keeps its locals in the window past its registers, as
/// `body_locals` finds them for an OSR region (`LocalsOf::Osr`): an environment activation does, and so does a VM frame whose
/// own storage does not cover the function.
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn context_locals_in_window_in_lane(context: usize) -> bool {
    let cx = context as *mut LaneContext;
    let vm = unsafe { vm(cx) };
    if let Some(activation) = vm.lane_stack.top_frame() {
        return activation.script().is_some_and(|script| script.environment);
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // A function none of whose records addresses a local (`CallPlan::addresses_locals`) keeps
    // none anywhere: its frame holds none, and neither its records nor its bodies read any.
    vm.frames.get(unsafe { (*cx).frame() }).is_some_and(|frame| code.function_get(frame.function as usize)
        .is_some_and(|body| frame.locals.len() < usize::from(body.locals) && body.call_plan(code).addresses_locals))
}

/// Where the lane goes after a region of the loop of `function` left inside the callee of a call it
/// made by its entry, with `exit`: in the innermost of the native frames the call pushed
/// (`frames_resumed_in_lane`).
fn region_left_in_lane(cx: *mut LaneContext, r: Registers, function: u32, exit: crate::compiled::LaneExit) -> LoopContinuation {
    // SAFETY: the program's views outlive the lane run, and the loop's function has the view its site was made from.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view).expect("the loop's function has its view");
    if exit == crate::compiled::LaneExit::Terminated {
        // As a body's: a fault can bypass the native exit's register snapshots.
        native_frames::discard_frames(cx);
        return LoopContinuation::Safepoint;
    }
    LoopContinuation::Recovered(frames_resumed_in_lane(cx, r, (own, None), exit))
}

/// Where the lane goes after native code left inside inlined `frame` of `region`'s callee, at its PC
/// `pc`. An inlining region of the Cranelift tier makes the callee's activation from the entry's
/// scratch window (`inlined_activation_in_lane`); a stitched region's registers are in the activation's
/// register file past its own, which the lane recovers as a lane body's (`body_inlined_activation_in_lane`).
fn inlined_continuation_in_lane(cx: *mut LaneContext, r: Registers, site: &LoopSite, (region, code): (&crate::compiled::LoopRegion, &crate::compiled::CompiledOsrRegion), (frame, pc): (usize, u32)) -> LoopContinuation {
    if region.inlining.is_some() {
        return LoopContinuation::Inlined { frame, pc };
    }
    // The exit is feedback as a top-level one is: the callee's instruction widens in the next plan, and is counted.
    let call = code.calls.inlined[frame];
    site.tier.borrow_mut().left_at(site.tiers, crate::compiled::base_region::inlined_exit_key(call.call, pc), false);
    // SAFETY: the context's program outlives the lane run.
    if let Some(body) = unsafe { &*(*cx).code }.function_get(call.target.function as usize) {
        crate::compiled::base_region::leave(call.target.function, pc, body.widened(body.code[pc as usize]).op());
    }
    // SAFETY: the context's records are the activation's, which the region ran on.
    let base = unsafe { (*cx).base };
    let (registers, resume, carried) = body_inlined_activation_in_lane(cx, r, base, InlinedRun::of_region(&code.calls), frame, pc);
    LoopContinuation::Recovered(LaneBodyLeft::Resume(registers, resume, carried))
}

/// The entry record of `site`'s native code from the running activation, and the element
/// views of its arrays (`loop_tier::loop_arrays_in_lane`), when its arrays and captures pass the
/// region's guards: each capture an initialized binding resolved without guest code by the
/// same lookup as LoadCapture (`capture_read_value`).
/// The entry record of `region` (`loop_entry_checked_in_lane`), without the reason of a decline.
fn loop_entry_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
) -> Option<crate::compiled::ElementViews> {
    loop_entry_checked_in_lane(cx, r, region, entry).ok()
}

/// `loop_entry_in_lane`, where each check that fails names its `RegionDecline`.
#[inline(never)]
fn loop_entry_checked_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &mut crate::compiled::LoopEntry,
) -> Result<crate::compiled::ElementViews, RegionDecline> {
    if !region.captures.is_empty() {
        let slots = capture_reads(unsafe { (*cx).registers }.capture());
        for slot in &region.captures {
            // The activation's `this`, for reads of `this` the region stores (`THIS_CAPTURE`).
            if *slot == crate::compiled::THIS_CAPTURE {
                entry.push_capture(unsafe { held_this_in_lane(cx, r) });
                continue;
            }
            // An inlined callee's capture reads its own environment (`Inlining::callee_captures`).
            if slot & crate::compiled::CALLEE_CAPTURE != 0 {
                let inlining = region.inlining.as_deref().ok_or(RegionDecline::CalleeCapture)?;
                let &(frame, callee_slot) = inlining.callee_captures.get(usize::from(slot & !crate::compiled::CALLEE_CAPTURE)).ok_or(RegionDecline::CalleeCapture)?;
                let callee = inlined_callee_of_guard(cx, r, inlining.frames.get(frame).ok_or(RegionDecline::CalleeCapture)?).ok_or(RegionDecline::CalleeCapture)?;
                let vm = unsafe { vm(cx) };
                // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
                let env = unsafe { &*vm.script_call_entry(callee).ok_or(RegionDecline::CalleeCapture)? }.env;
                // A top-level binding the global object projects is its property, which the
                // region keeps only while it stores no field (as the caller's own captures).
                let value = vm.environment_capture(env, callee_slot).or_else(|| {
                    (!region.writes_fields())
                        .then(|| vm.environment_capture_env(env, false, 0)
                            .and_then(|env| vm.plain_projected_root_binding(env, callee_slot)))
                        .flatten()
                }).ok_or(RegionDecline::CalleeCapture)?;
                entry.push_capture(value);
                continue;
            }
            // SAFETY: the context's dense slots remain live until the next collection.
            // A cache miss follows the same non-effectful lookup as LoadCapture.
            let value = unsafe { quench_runtime::tier_surface::EnvironmentSlots::read_storage(slots, *slot) }
                .or_else(|| capture_read_value(cx, 0, *slot))
                .or_else(|| {
                    // A projected global var aliases its global object's property. Native
                    // field stores could change it after entry, invalidating this snapshot.
                    (!region.writes_fields())
                        .then(|| projected_capture(cx, 0, *slot))
                        .flatten()
                }).ok_or(RegionDecline::Capture)?;
            entry.push_capture(value);
        }
    }
    let vm = unsafe { vm(cx) };
    // No guest call or collection occurs in a native run, and an array literal it allocates
    // is no entry value. Field writes replace guarded own slots, and element stores change
    // no shape slot. Immutable fields keep their entry values; admitted same-receiver Number
    // writes update SSA field cells. Other possible aliases are rejected by region
    // admission. Any miss, accessor, private name or non-Number declines before native
    // effects. Nested receivers follow the earlier field's value.
    for (index, field) in region.fields.iter().enumerate() {
        // A field native code reads at its instruction (`LoopRegion::is_dynamic_field`) takes
        // its entry value only to seed its representation: a receiver it does not answer for
        // now (a null child, another shape), or a value its Number representation does not
        // hold, seeds the representation's own placeholder.
        let blind = region.seeds_without_receiver(index);
        let read = (!blind).then(|| region_field_in_lane(cx, r, field, |index| entry.field_value(region, index))).flatten();
        let read_some = read.is_some();
        let value = match read {
            Some(value) if field.tagged || !region.is_dynamic_field(index) || value.as_number().is_some() => value,
            _ => (region.is_dynamic_field(index)
                    && (blind || field.tagged || vm.field_cache_entry(field.cache_site).receiver != quench_runtime::vm::NO_FIELD_RECEIVER))
                    .then(|| if field.tagged { Value::UNDEFINED } else { Value::integer(0) })
                    .ok_or(if read_some { RegionDecline::FieldNumber } else { RegionDecline::Field })?,
        };
        entry.push_field(value, field.tagged).ok_or(RegionDecline::EntryFull)?;
    }
    let views = loop_tier::loop_arrays_in_lane(vm, r, region, entry).ok_or(RegionDecline::Array)?;
    loop_field_stores(cx, r, region, entry).ok_or(RegionDecline::FieldStore)?;
    if region.reads_heap_natively() {
        let object_base = vm.heap_reads().object_record_base().unwrap_or(std::ptr::null());
        let (properties_base, properties_len) = vm.heap_reads().native_property_values();
        let array_base = vm.heap_reads().array_record_base().unwrap_or(std::ptr::null());
        entry.push_native_heap(object_base, properties_base, properties_len, array_base);
    }
    if let Some(inlining) = &region.inlining {
        // Leaving at an inlined instruction makes its callee's activation as the call would:
        // each call must still be one the lane activates in place (`inlined_activation_in_lane`).
        for frame in &inlining.frames {
            inlined_target_in_lane(cx, r, inlined_receiver_in_lane(r, region, entry, frame), frame).ok_or(RegionDecline::Inlined)?;
        }
    }
    Ok(views)
}

/// A cached data field's value as the region's entry reads it: a hit of the field's cache
/// on its receiver, or a typed array's geometry, for no private name. `earlier` gives the value of the earlier field a
/// receiver may be (`FieldReceiver::Field`).
fn region_field_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    field: &crate::compiled::FieldUse,
    earlier: impl FnOnce(usize) -> Value,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if vm.is_private_name(field.atom) { return None; }
    let receiver = match field.receiver {
        crate::compiled::FieldReceiver::This => unsafe { held_this_in_lane(cx, r) },
        crate::compiled::FieldReceiver::Register(register) => r.get(register),
        crate::compiled::FieldReceiver::Capture(slot) => capture_read_value(cx, 0, slot)?,
        crate::compiled::FieldReceiver::Field(index) => earlier(index),
    };
    // The site's primary entry first, as the lane's own reads take it (`own_formed_handler`).
    if let Some(value) = vm.record_field_entry_hit(vm.field_cache_entry(field.cache_site), receiver) {
        return Some(value);
    }
    match vm.cached_field_hit(code, receiver, field.cache_site) {
        quench_runtime::vm::object::FieldCacheProbe::Hit(value) => Some(value),
        // A site of several shapes answers from its polymorphic entries, own or of the
        // immediate prototype, as the lane's polymorphic read does (`POLYMORPHIC_SITE`).
        // A typed array's geometry is its intrinsic getter's answer, which only a call (a
        // detach, a resize, a redefinition) changes, and a call leaves the region.
        _ => vm.polymorphic_field_hit(code, receiver, field.cache_site)
            .or_else(|| vm.typed_array_geometry_read(receiver, field.atom))
            .or_else(|| (field.atom == vm.length_atom).then(|| vm.own_array_length(receiver).map(Value::length)
                // A string's length is its own, immutable: the region reads it at the instruction too.
                .or_else(|| vm.string_length_value(receiver))).flatten()),
    }
}

/// The call target of an inlined `frame`'s callee, when `script_call_miss` would activate it
/// in place now, binding the frame's receiver as its `this`: the target the call caches, an
/// activation of the running program with room on the lane stack.
fn inlined_target_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    receiver: Value,
    frame: &crate::compiled::InlineFrame,
) -> Option<*const ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    let target_pointer = vm.script_call_entry(inlined_callee_of_guard(cx, r, frame)?)?;
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
    let target = unsafe { &*target_pointer };
    (target.function == frame.function
        && target.entry.activates()
        && target.program() == unsafe { (*cx).program }
        && unsafe { (*cx).script_calls }
        && !vm.with_scope_entered
        && activation_room_in_lane(vm, target)
        && call_this_in_lane(vm, target, receiver).is_some_and(|bound| {
            bound == receiver || matches!(frame.guard, crate::compiled::InlineGuard::Capture(_) | crate::compiled::InlineGuard::Register(_))
        }))
    .then_some(target_pointer)
}

/// The callee an inlined `frame`'s call binds now: the closure its register or capture holds,
/// which the region binds by its function (a fresh closure of it on each entry is one case,
/// `osr_compile::unit_frames`), else the identity its field read guards.
fn inlined_callee_of_guard(cx: *mut LaneContext, r: Registers, frame: &crate::compiled::InlineFrame) -> Option<Value> {
    match frame.guard {
        crate::compiled::InlineGuard::Capture(slot) => capture_read_value(cx, 0, slot),
        crate::compiled::InlineGuard::Register(register) => Some(r.get(register)),
        _ => Some(frame.callee),
    }
}

/// The receiver of an inlined `frame`'s call: the field value a register the region writes
/// holds (`LoopRegion::held_field`), else the register's.
fn inlined_receiver_in_lane(
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &crate::compiled::LoopEntry,
    frame: &crate::compiled::InlineFrame,
) -> Value {
    match region.held_field(frame.receiver) {
        Some(index) => entry.field_value(region, index),
        None => r.get(frame.receiver),
    }
}

/// The region of `site`'s loop with its calls inlined, each callee the function the cached
/// field it is read from holds now, whose call the lane activates in place.
#[inline(never)]
fn inlining_region_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    site: &LoopSite,
) -> Option<crate::compiled::LoopRegion> {
    // `--b-jit` runs no inlining regions, so it does not analyze them.
    if crate::compiled::admission::stencil_only() { return None; }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let program = unsafe { (*cx).program };
    let (function_index, function, decoded, constants) = loop_tier::loop_function_in_lane(cx, code)?;
    let live = code.register_root_masks(function_index)?;
    // The callee is the last of the fields its read reads through, each read from the
    // registers and the fields before it as they are now.
    let mut resolve = |chain: &[crate::compiled::FieldUse], source: crate::compiled::CalleeSource| -> Option<crate::compiled::InlineCallee<'_>> {
        let mut values = Vec::with_capacity(chain.len());
        for field in chain {
            let value = region_field_in_lane(cx, r, field, |index| values[index])?;
            values.push(value);
        }
        let callee = match source {
            crate::compiled::CalleeSource::Capture(slot) => capture_read_value(cx, 0, slot)?,
            crate::compiled::CalleeSource::Register(register) => r.get(register),
            crate::compiled::CalleeSource::Path => *values.last()?,
        };
        let vm = unsafe { vm(cx) };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
        let target = unsafe { &*vm.script_call_entry(callee)? };
        (target.entry.activates() && target.program() == program).then_some(())?;
        let body = code.function_get(target.function as usize)?;
        Some(crate::compiled::InlineCallee {
            callee,
            function: target.function,
            params: body.params,
            registers: body.registers,
            instructions: body.instructions().collect(),
            live: code.register_root_masks(target.function)?,
        })
    };
    crate::compiled::analyze_inlining_osr_region(
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

/// The record native code leaving at the inlined instruction `callee_pc` of `frame`
/// continues at: the callee's activation the call at `call` makes (`push_activation_in_lane`),
/// a parameter bound to a caller register that register's value, each register live at
/// `callee_pc` the value native code left in the scratch window (as a lane body's leave,
/// `body_inlined_activation_in_lane`), and any other `undefined`, which nothing reads before
/// writing. The entry checked the call activates in place (`inlined_target_in_lane`).
#[inline(never)]
fn inlined_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    region: &crate::compiled::LoopRegion,
    entry: &crate::compiled::LoopEntry,
    frame: crate::compiled::InlineFrame,
    call: Ip,
    callee_pc: u32,
) -> Ip {
    let receiver = inlined_receiver_in_lane(r, region, entry, &frame);
    let target = inlined_target_in_lane(cx, r, receiver, &frame).expect("the entry checked the inlined call");
    // SAFETY: as `Vm::script_call_entry`; `call` is the call's record in the running view.
    let (target, arguments) =
        unsafe { (&*target, ImmediateLayout::call_window_base((*call).imm())) };
    let receiver = call_this_in_lane(unsafe { vm(cx) }, target, receiver)
        .expect("entry checked the receiver's effect-free binding");
    let (registers, _) =
        push_activation_in_lane::<false, false>(cx, r, call, target, arguments, receiver, false);
    let inlining = region.inlining.as_ref().expect("an inlining region");
    // The admitted target is the function the region compiled for this callee (`frame.function`,
    // fixed at analysis): the guard named the same callee, which holds one function.
    debug_assert_eq!(target.function, frame.function, "the entry's target is the inlined function");
    // SAFETY: the context's program outlives the lane run.
    let live = unsafe { &*(*cx).code }.register_root_masks(target.function)
        .expect("an inlined callee has its register roots (`inlining_region_in_lane`)");
    for register in 0..frame.registers {
        let bound = frame.bound.get(usize::from(register)).copied().flatten().map(|source| r.get(source));
        // SAFETY: native code wrote back the registers live where it left.
        let held = || live.is_live(callee_pc as usize, register).then(|| unsafe { entry.scratch_value(inlining, frame.base + register) });
        registers.set(register, bound.or_else(held).unwrap_or(Value::UNDEFINED));
    }
    // SAFETY: the activation's view holds a record for each of its function's PCs.
    unsafe { (*cx).base.add(callee_pc as usize) }
}

#[path = "vector_region.rs"]
pub(crate) mod vector_region;
pub(crate) mod wasm_instance;
use wasm_instance::lane_instance_binding_memory;


lane_handler! {
    /// f64 square root: an arithmetic operator, so a NaN operand matters
    /// only as a NaN (see `float_bits64`), and its result is held in place.
    fn f64_sqrt_miss<ACC: u8>(cx, r, ip, i, view) {
        let input = f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64());
        produce!(cx, r, ip, i.a, Value::from_float_bits64(input.sqrt().to_bits()), view)
    }
}

/// The bytes of memory binding `binding` code addresses in place (`MemoryStorage::direct_view`).
pub(crate) fn direct_memory(vm: &Vm, binding: Value) -> Option<(*mut u8, usize)> {
    direct_memory_and_guard(vm, binding).map(|(bytes, _)| bytes)
}

/// `direct_memory` with the memory's guard reservation, read together.
pub(crate) fn direct_memory_and_guard(
    vm: &Vm,
    binding: Value,
) -> Option<((*mut u8, usize), Option<(*mut u8, usize)>)> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap_reads().get(binding) else {
        return None;
    };
    bytes.direct_view()
}

fn memory_view(vm: &Vm, binding: Value) -> MemoryView {
    direct_memory(vm, binding).map_or(MemoryView::NONE, |(bytes, len)| MemoryView::new(bytes, len))
}

/// The view of memory binding `binding`, reusing the run's last resolution.
#[inline(always)]
fn resolve_memory(cx: *mut LaneContext, binding: Value) -> MemoryView {
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
fn frame_memory_view(
    cx: *mut LaneContext,
    view: &LaneView,
    registers: Registers,
) -> MemoryView {
    // A lane run is all JavaScript or all Wasm (`LaneContext::script_calls`, `inline_calls`).
    // SAFETY: the context's program outlives the lane run.
    if unsafe { (*(*cx).code).kind } != quench_runtime::bytecode::ProgramKind::Wasm {
        return MemoryView::script(cx);
    }
    view.memory_register.map_or(MemoryView::NONE, |register| {
        resolve_memory(cx, registers.get(register))
    })
}

/// A taken safepoint jump whose fuel ran out (`script_fuel_out`): leave the lane when a
/// collection is due or termination requested, otherwise refill the fuel, credit the work
/// clock and dispatch the target from the lane loop,
/// which unwinds the stack, with the accumulator `acc` the jump carries
/// (`LaneContext::resumed_acc`): a loop head whose every edge carries a
/// register reads it there. Out of line, so a
/// safepoint's fast path neither loads the context nor saves registers.
#[cold]
#[inline(never)]
fn safepoint_at(ip: Ip, target: Ip, acc: Slot<Value>) -> Ip {
    let cx = context_in_lane();
    // SAFETY: called only while a lane runs, from one of its handlers.
    if unsafe { vm(cx) }.collection_due() || unsafe { vm(cx) }.execution_interrupt.is_termination_requested() {
        // SAFETY: as above.
        unsafe { (*cx).fuel = 0 };
        return exit(ip);
    }
    // A crediting run counts every backward jump here; any other run takes a batch of
    // jumps before its next visit, and credits the work clock with the batch it spent.
    // SAFETY: as above.
    unsafe {
        if (*cx).backedges_left.is_some() {
            (*cx).fuel = 0;
        } else {
            (*cx).fuel = LANE_FUEL_BATCH;
            quench_runtime::clock::note_backedges(LANE_FUEL_BATCH);
        }
    }
    count_backedge(cx);
    // SAFETY: as above; the lane loop takes it before dispatching `target`.
    unsafe { (*cx).resumed_acc = acc };
    target
}

/// `safepoint_at` for a jump that writes `register` when taken.
#[cold]
#[inline(never)]
fn safepoint_writing(
    r: Registers,
    ip: Ip,
    target: Ip,
    acc: Slot<Value>,
    register: u16,
    value: Value,
) -> Ip {
    let next = safepoint_at(ip, target, acc);
    if next == target {
        r.set(register, value);
    }
    next
}

// The lane holds 64-bit payloads only in place (`Value::from_bits64`): a
// boxed operand or a result that needs a box leaves the lane, and the
// general loop, which boxes, runs the instruction. The handlers then make
// no calls.

/// The head record of a move pair followed by a jump (`(Move, Move, Jump)`, a `FUSED_CHAINS`
/// entry). The pair's two moves and the jump run as one record, whichever way the jump goes.
fn move_pair_jump_handler() -> Handler {
    lane_frame::chain_row::<{ quench_runtime::staged::MOVE_ROW }, { quench_runtime::staged::MOVE_ROW }, { quench_runtime::staged::JUMP_ROW }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::FUSED_PLAIN }>()
}


/// Continue in `frame` at `pc`: the record the frame's function resumes at.
#[inline(always)]
fn enter_frame_in_lane(cx: *mut LaneContext, frame: usize, pc: usize) -> Resumed {
    let (r, ip, view) = frame_state_in_lane(cx, frame, pc);
    // A frame call or return reached from an out-of-line path may nest rather than run as a
    // sibling, so a run of calls and returns with no backward jump between them grew the
    // stack until it overflowed (mandreel). Below the floor the lane loop dispatches the
    // record instead, from the frame state the context now holds.
    if script_below_floor(cx) {
        return Resumed::Exit(ip);
    }
    Resumed::At(r, ip, view)
}

/// Make `frame` the lane's running function, at the record of `pc`: its registers, that
/// record and the state its records start from.
#[inline(always)]
fn frame_state_in_lane(
    cx: *mut LaneContext,
    frame: usize,
    pc: usize,
) -> (Registers, Ip, Carried) {
    let vm = unsafe { vm(cx) };
    let function = vm.frames[frame].function;
    let program = unsafe { (*cx).program };
    // The context's table holds the frame's program's views: a derived one is read there.
    // SAFETY: the table lives as long as its program entry.
    let derived = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view);
    let view_in_lane = match derived {
        Some(view) => view,
        None => {
            if !vm.runs_lane {
                unreachable!("a host that runs no lane has no lane frame");
            }
            // The same derivation as every entry's (`derived_view`): an image's view takes on every record.
            let image = vm.programs.residual(program).is_some_and(|code| vm.programs.ahead_images(program, code).is_some());
            let Some(view_in_lane) = vm.programs.view_in_lane(program, function, |code, constants, kind, live, sites| {
                vm.derived_view(function, code, constants, kind, live, sites, image)
            }) else {
                unreachable!("an inline Wasm frame runs a function of its program");
            };
            // SAFETY: views live as long as their program entry.
            unsafe { &*view_in_lane }
        }
    };
    let base = view_in_lane.records();
    let r = Registers(vm.frames[frame].registers.as_mut_ptr());
    let view = frame_memory_view(cx, view_in_lane, r);
    // SAFETY: the view has one record per instruction of the function, and
    // `pc` is the frame's validated resume point.
    let ip = unsafe { base.add(pc) };
    // The general path's register access names the active frame's register file.
    vm.active_registers = r.0;
    unsafe {
        (*cx).root = LaneRoot::Frame(frame);
        (*cx).registers = r;
        (*cx).base = base;
        (*cx).memory = view;
        r.set_this(vm.frame_lane_this(frame));
        r.set_env(Value::DELETED);
        (*cx).registers.set_capture(std::ptr::null());
        (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
    }
    let view = Carried {
        memory: view,
        integers: quench_runtime::value::IntegerEncoding::TAG,
        acc: Slot::of(Value::UNDEFINED),
    };
    (r, ip, view)
}

#[quench_macros::ext(DispatchFastVm2)]
impl Vm {
    /// Make the lane's activations VM frames, innermost last, each with the
    /// pending call its push would have recorded; the lane continues in the
    /// innermost one. The bottom caller is the lane run's VM frame.
    /// Whether a lane may call `native` (the function `callee`) beside its activations
    /// (`Native::runs_beside_activations`, `Host::host_function_runs_beside_activations`).
    fn runs_beside_activations(&self, native: Native, callee: Value) -> bool {
        if native != Native::HostFunction {
            return native.runs_beside_activations();
        }
        let Some(index) = self.function_record(callee).and_then(|function| self.host_operation_index(function.env)) else {
            return false;
        };
        self.host.host_function_runs_beside_activations(index as usize)
    }

    /// Whether `native` called on `receiver` runs beside activations as the natives of
    /// `runs_beside_activations` do: `%IteratorPrototype%.next` of an array, string, map or set
    /// iterator, whose step reads its source alone. A generator's step resumes its frame and an
    /// iterator helper's may step one, so those take frames first.
    fn steps_beside_activations(&self, native: Native, receiver: Value) -> bool {
        use quench_runtime::tier_surface::IteratorKind;
        native == Native::IteratorNext
            && self.heap_reads().iterator(receiver).is_some_and(|iterator| {
                matches!(
                    iterator.kind,
                    IteratorKind::Array
                        | IteratorKind::ArrayKeys
                        | IteratorKind::ArrayValues
                        | IteratorKind::ArrayEntries
                        | IteratorKind::String
                        | IteratorKind::MapKeys
                        | IteratorKind::MapValues
                        | IteratorKind::MapEntries
                        | IteratorKind::SetValues
                        | IteratorKind::SetEntries
                )
            })
    }

    /// Whether the innermost lane activation runs over a suspended frame's record (a generator's
    /// or an async function's window, or one a direct eval gave a record): leaving the lane boxes
    /// that record's state as a frame, which a call waiting for its callee's view would do. The
    /// innermost is the caller deciding; a window further out only tunes when a callee lowers.
    pub(crate) fn runs_windows_in_lane(&self) -> bool {
        self.lane_stack.top_frame().and_then(|frame| frame.script()).is_some_and(|script| !script.record.is_null())
    }

    /// Whether `callee` is a native that runs beside a lane's activations
    /// (`runs_beside_activations`), which a call made while they stay frameless may make
    /// without making them frames.
    pub(crate) fn callee_runs_beside_activations(&self, callee: Value) -> bool {
        match self.function_record(callee) {
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) => {
                self.runs_beside_activations(*native, callee)
            }
            _ => false,
        }
    }

    /// Whether the activations of a lane native call deferred beside them (`lane_deferred`)
    /// run a function of `program`: the innermost runs its lane context's program, and each
    /// names its caller's, the outermost's being a frame's.
    pub(crate) fn deferred_activations_run(&self, program: ProgramId) -> bool {
        let Some(DeferredLaneCall { cx, .. }) = &self.lane_deferred else {
            return false;
        };
        // SAFETY: the deferred call's lane runs, so its context lives.
        let innermost = unsafe { (*cx.cast::<LaneContext>()).program };
        innermost == program
            || self.lane_stack.frames().any(|activation| activation.caller_program() == program)
    }

    /// Make a deferred lane native call's activations frames (`DeferredLaneCall`), as
    /// `call_native_in_lane` would have before calling: at every point that observes the frame
    /// stack while a native runs, which guest code, a stack capture and a collection do.
    #[inline(always)]
    pub(crate) fn materialize_deferred_lane(&mut self) {
        if self.lane_deferred.is_some() {
            self.materialize_deferred_lane_now();
        }
    }

    #[cold]
    #[inline(never)]
    fn materialize_deferred_lane_now(&mut self) {
        let Some(DeferredLaneCall { cx, call_pc, .. }) = self.lane_deferred.take() else {
            return;
        };
        let before = self.frames.len();
        self.materialize_lane_activations(cx.cast::<LaneContext>());
        self.resume_deferred_caller(call_pc, before);
    }

    /// The deferred call's resumption once its activations became frames above `before`.
    fn resume_deferred_caller(&mut self, call_pc: usize, before: usize) {
        // The natives running beside the activations recorded the frame count without them;
        // their caller is now the innermost new frame (`Vm::function_caller`).
        let natives = self.realm.promise.active_native.len();
        self.shift_beside_natives(natives, before, self.frames.len() - before);
        // The call runs from the innermost activation's frame, resuming after it.
        let frame = self.frames.len() - 1;
        self.frames[frame].pc = call_pc + 1;
        self.frames[frame].pc_is_resume = true;
    }

    /// The lane's exit (`dispatch_in_lane`'s exit record): a running leaf, whose records may
    /// leave the lane (`materialize_leaf_in_lane`), then every activation becomes a frame the
    /// general path runs. Nothing else materializes while a leaf runs: its records call
    /// nothing, and a getter's activation makes it first.
    fn materialize_lane_exit(&mut self, cx: *mut LaneContext) {
        materialize_running_leaf_in_lane(cx);
        self.materialize_lane_activations(cx);
    }

    fn materialize_lane_activations(&mut self, cx: *mut LaneContext) {
        debug_assert!(unsafe { (*cx).leaf.call }.is_null(), "a running leaf materializes at the lane's exit");
        self.link_natives(cx);
        if !self.lane_stack.has_frames() {
            return;
        }
        #[cfg(feature = "profile-lane-exits")]
        quench_runtime::vm::lane_exits::record_detail(format!("materialize depth {}", self.lane_stack.activation_count()));
        // Every frame taken off the stack where it is, each as the record its frame becomes reads
        // it, with what its side record carries (one-frame-one-call.md F4 (3)): no spill.
        let (activations, payloads): (Vec<Activation>, Vec<SidePayload>) = self.lane_stack.take_frames().into_iter().unzip();
        let lane_wasm_storage_bytes = std::mem::take(&mut self.lane_stack.wasm_storage_bytes);
        debug_assert_eq!(
            lane_wasm_storage_bytes,
            activations
                .iter()
                .map(Activation::frame_storage_bytes)
                .sum::<usize>()
        );
        let root = unsafe { (*cx).root };
        let mut caller = root.caller_of_first(self.frames.len());
        let mut steps = std::mem::take(&mut self.lane_stack.steps).into_iter();
        let iterators = std::mem::take(&mut self.lane_stack.iterators);
        let scopes = std::mem::take(&mut self.lane_stack.with_objects);
        // Each frame holds the environment its eval promoted; the records go with the activations.
        let _records = std::mem::take(&mut self.lane_stack.records);
        let states = ActivationState::of(&activations, cx);
        // A deferred native's caller activations below its callback take the lane's outer
        // pending calls and executing program (`NativeBoundary`).
        let boundary = self.lane_stack.native_boundary.take();
        let before = self.frames.len();
        for (depth, ((activation, payload), state)) in activations.into_iter().zip(payloads).zip(states).enumerate() {
            let below = boundary.filter(|boundary| depth < boundary.activation);
            if let Some(boundary) = boundary
                && depth == boundary.activation
            {
                self.lane_stack.boundary_frames = Some(self.frames.len() - before);
            }
            // SAFETY: the pending calls and executing program outlive the lane run.
            let pending = unsafe { &mut *below.map_or((*cx).pending, |boundary| boundary.pending) };
            let executing_program = below.map_or(unsafe { (*cx).executing }, |boundary| boundary.executing);
            let program = state.program;
            // Its frame belongs to its own program, which its push makes active; a call
            // into another program sets the caller's aside as the general loop's push does.
            self.set_active_program(program);
            // SAFETY: the program store keeps the residual for the lane run.
            let Some(code) = self.programs.residual(program).map(std::ptr::from_ref) else {
                unreachable!("an activation runs a function of a program the store holds");
            };
            let code = unsafe { &*code };
            let caller_program = (program != activation.caller_program()).then(|| {
                // SAFETY: the general loop's executing program outlives the lane run.
                let executing = unsafe { &mut *executing_program };
                debug_assert_eq!(executing.id, activation.caller_program());
                let retained = self.programs.get(program);
                let saved = quench_runtime::vm::dispatch_frame::SavedProgram {
                    id: executing.id,
                    retained: std::mem::replace(&mut executing.retained, retained),
                };
                executing.id = program;
                saved
            });
            // SAFETY: a call record lies in its caller's view.
            let mut call_pc = unsafe { activation.call().offset_from(activation.caller_base()) } as usize;
            let (mut result, resume_pc) = activation.caller_resume(call_pc);
            self.resume_caller_frame(root, depth, caller, resume_pc);
            let count = usize::from(code.function(activation.function() as usize).registers);
            // SAFETY: the activation's window holds its registers, which nothing writes until
            // its frame takes them.
            let window = unsafe { activation.window(count) };
            let completion = match activation.script().map(|script| script.completion) {
                Some(completion) if completion.constructs() => CallCompletion::Constructor(state.this),
                Some(Completion::SettlesPromise) => CallCompletion::Promise(self.settling_root(payload.settles())),
                Some(Completion::AsyncRequest) => CallCompletion::AsyncRequest(self.settling_root(payload.settles())),
                Some(Completion::ArrayStep) => {
                    let step = steps.next().expect("each array step activation has its entry");
                    CallCompletion::ArrayStep(Box::new(self.array_step_call(step.iteration, step.element, step.index)))
                }
                Some(Completion::ToWasm) => CallCompletion::ToWasm(payload.to_wasm().result),
                other => match *activation.kind() {
                    ActivationKind::WasmEntry { result, .. } => CallCompletion::WasmExport(result),
                    _ => setters::frame_completion(other, payload),
                },
            };
            // A native's callback is the entry frame of the loop that finishes it.
            let to_native = body_calls::enters_as_frame(&mut self.lane_stack, &activation, self.frames.len());
            let call_guard = match *activation.kind() {
                // A windowed activation's frame is its owner's, which takes the iterators its loops pushed.
                ActivationKind::Script(script) if script.completion.is_windowed() => {
                    let owned = iterators.iter().filter(|(owner, _)| *owner == depth).map(|(_, iterator)| *iterator);
                    let entered = scopes.iter().filter(|(owner, _)| *owner == depth).map(|(_, object)| *object);
                    self.push_window_frame(root, &script, owned, entered);
                    quench_runtime::stack::GuestCallGuard::enter().ok()
                }
                ActivationKind::Script(script) => {
                    let argc = usize::from(script.argc);
                    // A forwarding activation kept the values after its registers; any other
                    // passed them all to parameter registers.
                    let passed = if script.forwards {
                        // SAFETY: the window holds the registers, then the `argc` kept values.
                        &(unsafe { activation.window(count + argc) })[count..]
                    } else if script.environment {
                        // After its environment and locals (`activate_environment_in_lane`).
                        let kept = count
                            + usize::from(WINDOW_LOCALS)
                            + usize::from(code.function(activation.function() as usize).locals);
                        // SAFETY: the window holds them, then the `argc` kept values.
                        &(unsafe { activation.window(kept + argc) })[kept..]
                    } else {
                        &window[..argc]
                    };
                    if script.completion == Completion::Forwarder {
                        // The construction's skipped forwarding constructor becomes the frame
                        // it would have been, pending at its forwarding call, whose dead
                        // registers hold `undefined`; the method's frame runs inside it.
                        // A native frame's is its descriptor's (`NativeFrames`), which no entry holds.
                        // SAFETY: a native frame's callee outlives the run.
                        let forwarder = unsafe { activation.callee.as_ref() }
                            .map_or_else(|| payload.forwarder(), |callee| callee.forwarder)
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
                            destination: result & quench_runtime::bytecode::REGISTER_MASK,
                            completion: CallCompletion::Constructor(state.this),
                            returns: result & quench_runtime::bytecode::RETURN_REGISTER != 0,
                            call_guard: quench_runtime::stack::GuestCallGuard::enter().ok(),
                            caller_program: None,
                        });
                        caller = caller.wrapping_add(1);
                        call_pc = forwarder.method.call_pc as usize;
                        result = forwarder.method.call_result;
                        self.frames[caller].pc = call_pc + 1;
                        self.frames[caller].pc_is_resume = true;
                    }
                    self.push_script_frame_with_registers(
                        code,
                        activation.function(),
                        state.env,
                        state.this,
                        if script.completion.constructs() {
                            CallContext::user_function(activation.function(), script.callee).constructed()
                        } else {
                            CallContext::user_function(activation.function(), script.callee)
                        },
                        passed,
                        window,
                    );
                    if script.environment {
                        self.materialize_environment_frame(code, &activation, (program, state.env, state.this), argc);
                    }
                    // Its frame closes the iterators its loops pushed (`LaneStack::iterators`).
                    let frame = self.frames.last_mut().expect("the activation pushed a frame");
                    frame.active_iterators.extend(
                        iterators.iter().filter(|(owner, _)| *owner == depth).map(|(_, iterator)| *iterator),
                    );
                    // The scope chain it entered sits on the VM's scope stack above its base.
                    self.with_stack.extend(scopes.iter().filter(|(owner, _)| *owner == depth).map(|(_, object)| *object));
                    // The activation counted within `LaneStack::activation_limit`; its frame
                    // takes the guest-call guard its push would have.
                    quench_runtime::stack::GuestCallGuard::enter().ok()
                }
                ActivationKind::Native => unreachable!("native frames link before the activations become frames"),
                ActivationKind::Wasm { .. } => {
                    // The lane reserved this payload while it was frameless; its materialized
                    // frame holds the same weighted guard until the call returns.
                    let env = self.frames[caller].env;
                    Some(self.push_wasm_frame_with_registers(code, env, activation.function(), window))
                }
                // The export's frame, as `lane_wasm_export_frame` pushes it when it cannot
                // activate the callee: over the instance environment, with the pending call
                // that converts its result.
                ActivationKind::WasmEntry { instance, .. } => {
                    Some(self.push_wasm_frame_with_registers(code, instance, activation.function(), window))
                }
            };
            if !to_native {
                pending.push(PendingGeneralCall {
                    caller,
                    caller_cursor: None,
                    call_pc: call_pc as u32,
                    destination: result & quench_runtime::bytecode::REGISTER_MASK,
                    completion,
                    returns: result & quench_runtime::bytecode::RETURN_REGISTER != 0,
                    call_guard,
                    caller_program,
                });
            }
            caller = caller.wrapping_add(1);
        }
        debug_assert!(steps.next().is_none(), "every array step belongs to an activation");
        if let Some(boundary) = boundary {
            // The native's caller is now the innermost of its activations' frames, as when
            // its deferred call is observed (`materialize_deferred_lane_now`).
            let added = *self.lane_stack.boundary_frames.get_or_insert(self.frames.len() - before);
            let natives = boundary.natives.min(self.realm.promise.active_native.len());
            self.shift_beside_natives(natives, boundary.deferred.frames, added);
        }
        self.lane_stack.values.clear();
        // The frames now hold the guards the activations were counted against.
        self.lane_stack.refresh_activation_limit();
        // The lane continues in the innermost frame, whose register file the general path's
        // register access names from now on (`enter_frame_in_lane`).
        let registers = Registers(self.frames[caller].registers.as_mut_ptr());
        self.active_registers = registers.0;
        unsafe {
            (*cx).root = LaneRoot::Frame(caller);
            (*cx).registers = registers;
            registers.set_this(self.frame_lane_this(caller));
            registers.set_env(Value::DELETED);
            (*cx).instance = Value::DELETED;
            (*cx).registers.set_capture(std::ptr::null());
            (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
        }
    }

    /// Gives the frame an environment activation became its environment and locals: the frame its
    /// promotion would have made, `argc` values having been passed to `activation`.
    fn materialize_environment_frame(&mut self, code: &ResidualProgram, activation: &Activation, (program, env, this): (ProgramId, Value, Value), argc: usize) {
        // The frame its promotion would have made: the environment it holds,
        // and its locals.
        let held = usize::from(code.function(activation.function() as usize).registers);
        // SAFETY: the activation's window, intact.
        let environment = unsafe { window_environment(self, code, activation.window_ptr().add(held), activation.function(), env, program, this, None) };
        let frame = self.frames.last_mut().expect("the activation pushed a frame");
        frame.env = environment;
        frame.captured = true;
        let locals = held + usize::from(WINDOW_LOCALS);
        let count = frame.locals.len();
        // The activation never binds `arguments`: its body only reads the
        // values passed and its parameters (`Function::reads_passed_arguments_only`),
        // so the object the push made from the values is the binding, with
        // each element a parameter aliases holding that parameter now.
        let body = code.function(activation.function() as usize);
        let arguments = body.arguments_slot.map(usize::from);
        let object = arguments.map(|slot| frame.locals[slot]);
        // SAFETY: the window holds the environment's locals after the registers.
        frame.locals.copy_from_slice(&unsafe { activation.window(locals + count) }[locals..]);
        if let (Some(slot), Some(object)) = (arguments, object) {
            frame.locals[slot] = object;
            if body.arguments_alias_parameters() {
                for parameter in 0..usize::from(body.params).min(argc) {
                    // SAFETY: as the locals' read.
                    let value = unsafe { activation.window(locals + parameter + 1) }[locals + parameter];
                    self.store_mapped_argument(object, parameter, value);
                }
            }
        }
    }

    /// The lane's `this` for frame `frame` (`LaneContext::this`).
    fn frame_lane_this(&self, frame: usize) -> Value {
        let frame = &self.frames[frame];
        if frame.fixed_this {
            frame.this
        } else {
            Value::DELETED
        }
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
            cached.callee == callee && cached.admission.admits(self.lane_stack.admission)
        })
    }

    /// `script_call_entry` for the call record whose hint is `link` (`LaneInstruction::call_link`):
    /// the entry it names when that holds `callee`'s target, which skips the set's hash, else
    /// the set's, which the hint names from then on. A cache that grew or another callee only
    /// costs the hash again.
    #[inline(always)]
    fn linked_script_call_entry(&mut self, callee: Value, link: &std::cell::Cell<u16>) -> Option<*const ScriptCallTarget> {
        let linked = link_index(link.get());
        let index = if self.script_call_holds(linked, callee) {
            linked
        } else {
            let index = match self.script_call_index(callee) {
                Some(index) => index,
                None => self.relink_site_closure(callee, linked)?,
            };
            // The callee in hand joins the site's record (`LaneStack::call_sites`): the entry it
            // linked may hold another site's callee since, so only the record names its own.
            self.note_linked_callee(link, index);
            // The entry count stays below `NO_CALL_LINK` (`SCRIPT_CALL_TARGETS`).
            link.set(index as u16);
            index
        };
        // SAFETY: `script_call_holds` found the entry within the cache.
        Some(unsafe { self.lane_stack.script_targets.entries.get_unchecked(index) } as *const ScriptCallTarget)
    }

    /// `linked_script_call_entry`'s target for a closure no cache set holds: a fresh closure
    /// of the site's linked function, relinked in place, out of line, since a polymorphic
    /// site's every miss would otherwise carry it.
    #[cold]
    #[inline(never)]
    fn relink_site_closure(&mut self, callee: Value, linked: usize) -> Option<usize> {
        // The only entry creation a run with `script_calls` off could reach: refused here, so a
        // linked entry never reaches a handler while the lane admits no script calls
        // (`set_script_calls` sets `Admission::NO_SCRIPT_CALLS`, which `script_call_holds` tests).
        if !self.lane_stack.script_calls_allowed() {
            return None;
        }
        // Only a call-only closure (`Value::known_closure`) takes the known form.
        if callee.as_known_closure().is_some()
            && let Some(index) = self.relink_known_closure(callee, linked)
        {
            return Some(index);
        }
        self.relink_closure(callee, linked)
    }

    /// The site record of `link` notes entry `index`'s callee (`Vm::note_site_callee`) where a tier may
    /// run. A polymorphic site's steady state re-notes the callees it alternates between: one compare of
    /// `LaneStack::linked` finds them noted, as the interpreter's own link test costs.
    #[inline(always)]
    fn note_linked_callee(&self, link: &std::cell::Cell<u16>, index: usize) {
        if self.execution_mode != quench_runtime::vm::ExecutionMode::Compiled {
            return;
        }
        let address = std::ptr::from_ref(link).addr();
        // SAFETY: the caller resolved `index` within the cache.
        let callee = unsafe { self.lane_stack.script_targets.entries.get_unchecked(index) }.callee;
        if self.lane_stack.linked[noted_slot(address, index as u32)].get() != (address, callee) {
            self.note_linked_callee_slow(address, link, index);
        }
    }

    #[cold]
    #[inline(never)]
    fn note_linked_callee_slow(&self, address: usize, link: &std::cell::Cell<u16>, index: usize) {
        let current = &self.lane_stack.script_targets.entries[index];
        // Where no tier runs, no region reads the record: the pair is marked only.
        if self.tier_may_run(crate::compiled::admission::Tier::ANY) {
            self.note_site_callee(link, crate::compiled::CallTarget {
                callee: crate::compiled::CalleeBinding::Identity(current.callee), function: current.function });
        }
        self.lane_stack.linked[noted_slot(address, index as u32)].set((address, current.callee));
    }

    /// `script_call_entry` of a callee of another program than the running one, whose
    /// targets `LaneStack::cross_targets` holds (`script_call_cross_miss`).
    #[inline(always)]
    fn cross_call_index(&self, callee: Value) -> Option<usize> {
        let set = self.lane_stack.cross_targets.set(callee)?;
        // The set's two ways, tested in turn, as `script_call_index` does.
        if self.cross_call_holds(set, callee) {
            Some(set)
        } else {
            self.cross_call_holds(set | 1, callee).then_some(set | 1)
        }
    }

    /// Whether `LaneStack::cross_targets` entry `index` holds `callee`'s target for this
    /// generation, for a caller in another program than the callee's.
    #[inline(always)]
    fn cross_call_holds(&self, index: usize, callee: Value) -> bool {
        self.lane_stack.cross_targets.entries.get(index).is_some_and(|cached| {
            cached.callee == callee && cached.admission.admits_across(self.lane_stack.admission)
        })
    }

    /// `cross_call_entry` for the call record whose hint is `link` (`LaneInstruction::call_link`),
    /// as `linked_script_call_entry` is for the running program's targets: the entry the hint
    /// names when it holds `callee`'s target, else the set's, which the hint names from then on.
    #[inline(always)]
    fn linked_cross_call_entry(&self, callee: Value, link: &std::cell::Cell<u16>) -> Option<*const ScriptCallTarget> {
        let linked = usize::from(link.get());
        let index = if self.cross_call_holds(linked, callee) {
            linked
        } else {
            let index = self.cross_call_index(callee)?;
            // The entry count stays below `NO_CALL_LINK` (`SCRIPT_CALL_TARGETS`).
            link.set(index as u16);
            index
        };
        // SAFETY: `cross_call_holds` found the entry within the cache.
        Some(unsafe { self.lane_stack.cross_targets.entries.get_unchecked(index) } as *const ScriptCallTarget)
    }

    /// `cross_call_index`'s entry, in place.
    #[inline(always)]
    fn cross_call_entry(&self, callee: Value) -> Option<*const ScriptCallTarget> {
        self.cross_call_index(callee).map(|index| &self.lane_stack.cross_targets[index] as *const ScriptCallTarget)
    }

    /// Whether `callee` activates as `function` in place, in the active program, with an
    /// ordinary `this`: the facts an inlined frame's recovery relies on
    /// (`body_inlined_activation_in_lane`), admitted once when its body is published; a
    /// lifecycle edge retires the body before they can expire.
    pub(crate) fn inlined_target_admitted_in_lane(&mut self, code: &ResidualProgram, callee: Value, function: u32) -> bool {
        self.resolve_script_call_target(code, callee).is_some_and(|target| target.program() == self.active_program
            && target.function == function
            && target.entry.activates_registers_only()
            // An arrow that reads none of its maker's bindings has no receiver to bind.
            && (target.this_binding.binds_receiver() || (target.this_binding == ThisBinding::Lexical
                && !quench_runtime::bytecode::arrow_chain_reads_implicit_bindings(code, function))))
    }

    /// Resolve and cache `callee` as a lane call target: frameless when its activation is its
    /// registers (`CallPlan::registers_only`), else a `CallEntry::Frame` entry, so its calls
    /// take a frame without resolving it again.
    #[inline(never)]
    pub(crate) fn resolve_script_call_target(&mut self, code: &ResidualProgram, callee: Value) -> Option<ScriptCallTarget> {
        callee.heap_index()?;
        let (program, function, env) = self.user_target_in_lane(callee)?;
        // Only plain closures' functions are known (`remember_target`): a hit needs no class or admission.
        if program == self.active_program && let Some(known) = self.lane_stack.function_target(program, function, false) {
            let target = self.closure_target(code, program, callee, env, known)?;
            self.lane_stack.remember_script_target(target, self.heap_reads().memory_policy());
            return Some(target);
        }
        let numeric = self.numeric_body(program, function);
        #[cfg(feature = "profile-lane-exits")]
        if let Some(body) = self.programs.residual(program).and_then(|code| { let body = code.function_get(function as usize)?;
            Some(format!("resolve {} span={:?} cross={}", body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                body.source_span.map(|span| span.start), program != self.active_program)) }) {
            quench_runtime::vm::lane_exits::record_detail(body);
        }
        // A callee of another program runs its own residual, which the program store keeps
        // alive with the program for the lane run; a closure of a program the lane admits,
        // plain or numeric, activates across programs.
        let cross = program != self.active_program;
        let code: &ResidualProgram = if cross {
            // SAFETY: as above; no program is installed or retired while a call resolves.
            let callee_code = unsafe { &*std::ptr::from_ref(self.programs.residual(program)?) };
            if !self.admits_in_lane(program, callee_code) { return None; }
            callee_code
        } else { code };
        // The JavaScript tier enters a frameless activation at its prologue as it enters a
        // frame (`tier_prologue_in_lane`), so a function it may claim is cached like any other.
        if function == quench_runtime::vm::ROOT_FUNCTION_ID { return None; }
        // A numeric body's target differs from its plain closures' (below), so only plain
        // closures share their function's.
        if !numeric && let Some(known) = self.lane_stack.function_target(program, function, cross) {
            let target = self.closure_target(code, program, callee, env, known)?;
            let policy = self.heap_reads().memory_policy();
            if cross {
                self.lane_stack.remember_cross_target(target, policy);
            } else {
                self.lane_stack.remember_script_target(target, policy);
            }
            return Some(target);
        }
        if !self.call_may_derive_in_lane(program, function) { return None; }
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
        // A settling call is a plain call of the running program, under a host that observes no
        // promise lifecycle; an async body whose locals and closures ride a window settles from
        // an environment activation (`settles_in_environment`).
        let (may_settle, settles_in_environment) = self.settling_calls(body, &plan, cross || numeric);
        let environment = !plan.registers_only
            && (plan.environment_activation || settles_in_environment)
            && !numeric
            && self
                .view_any_in_lane(program, function)
                .is_some_and(|view| unsafe { &*view }.environment_window.is_some());
        let settles_in_environment = settles_in_environment && environment;
        // A non-suspending async body of the running program activates for its plain calls,
        // whose promise its return settles (`CallEntry::Settles`); across programs, or under a
        // host that observes promise lifecycles, its calls take a frame.
        let settles = (plan.settles_in_loop || settles_in_environment) && may_settle;
        // A non-suspending async body runs in registers only as a settling activation.
        let registers_only = plan.registers_only && (settles || !plan.settles_in_loop);
        // Across programs a construction leaves its calls a frame.
        let view = if !(registers_only || environment)
            || numeric
                && body
                    .code
                    .iter()
                    .any(|packed| quench_runtime::staged::constructs(body.widened(*packed).op()))
        {
            None
        } else {
            self.view_any_in_lane(program, function)
        };
        #[cfg(feature = "profile-lane-exits")]
        if view.is_none() {
            let closures = body.code.iter().any(|packed| body.widened(*packed).op().makes_closure());
            quench_runtime::vm::lane_exits::record_detail(format!(
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
                admission: self.lane_stack.target_admission(program),
                function,
                env,
                entry: CallEntry::Frame,
                ..EMPTY_SCRIPT_CALL_TARGET
            };
            self.remember_target(target, numeric, cross);
            return Some(target);
        };
        let tier_may_claim = self.js_tier_may_claim(code, program, function);
        // SAFETY: the program store keeps a derived view alive with its program.
        let shape = *unsafe { &*view }.target_body.get_or_init(|| {
            let constructs = Self::inline_constructible(body);
            // A class constructor's calls throw on the general path, so they take a
            // frame; only its constructions activate frameless (`script_construct_miss`).
            let entry = body_entry(body, &code.constants);
            // SAFETY: the program store keeps a derived view alive with its program.
            unsafe { &*view }.entry.set_facts(quench_runtime::staged::call::CellFacts::of_function(body.registers, body.params, entry_fill(code, body), entry == CallEntry::Activates && !environment));
            TargetBody {
                entry,
                constructs,
                initializes: constructs && body.only_initializes_this_fields(&code.constants),
                leaf: (entry == CallEntry::Activates)
                    .then(|| leaf_body(body, code, |atom| self.is_private_name(atom)))
                    .flatten()
                    // The claim is a fact of the body (`js_tier_may_claim`): it holds for the view.
                    .filter(|_| !tier_may_claim)
                    // SAFETY: the program store keeps a derived view alive with its program.
                    .inspect(|&form| unsafe { &*view }.specialize_leaf_returns(form)),
            }
        });
        // An environment activation (`environment`, whose view has a window layout) claims as any other: its body
        // reads its locals through the window (`window_bodies::body_locals`).
        if !settles
            && self.claims_across(cross)
            && !body.rest
            // A `keeps_arguments` target activates in the keeping form, whose window no body
            // run reads yet: its rows lower (`ArgumentsLength`, `ArgumentGet`), the entry does
            // not (lane-bodies claim for keeping activations).
            && !plan.keeps_arguments
            && shape.entry == CallEntry::Activates
            && tier_may_claim
            // SAFETY: as below.
            && (!self.programs.js_jit_lane_methods_enabled() || unsafe { &*view }.tier_unclaimed_in_lane())
        {
            // SAFETY: the program store keeps a derived view alive with its program.
            unsafe { &*view }.claim_for_tier(tier_prologue_row(), function, body.params, !body.is_arrow);
        }
        let capture = self.target_capture(code, program, env);
        let target = ScriptCallTarget {
            callee,
            admission: self.lane_stack.target_admission(program),
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
            lexical_this: match lexical_this {
                Some(this) => this.unwrap_or(Value::UNDEFINED),
                None if body.strict => Value::UNDEFINED,
                None => self.realm.globals,
            },
            // A construction activates in place (`activate_in_lane`), which gathers no rest, and
            // completes across programs as a call does (`Completion::CrossConstructs`); a
            // forwarding one runs its program's keeping construction
            // (`construct_keeping_activation_in_lane`), which another program's caller does not.
            // An environment activation's window has no record to hold the `new.target` an arrow reads.
            forms: u8::from(shape.constructs && !(environment && body.activation_bindings_observable(code)) && !settles && !body.rest && !(cross && plan.keeps_arguments)) * TARGET_CONSTRUCTS
                | u8::from(shape.initializes && !environment && !settles && !cross && !body.rest) * TARGET_INITIALIZES
                | u8::from(body.rest && (plan.keeps_arguments || plan.legacy_reflection || (lexical_this.is_none() && !body.strict))) * TARGET_GATHERS_KEEPING
                | u8::from(body.rest_view()) * TARGET_REST_VIEW
                | u8::from(lexical_this.is_some() && body.arrow_reads_implicit_bindings(code, function)) * TARGET_READS_IMPLICIT
                | u8::from(settles_in_environment) * TARGET_SETTLES_IN_ENVIRONMENT,
            fill: u8::try_from(code.entry_fill(body)).unwrap_or(u8::MAX),
            // A read target answers its call through the callee's own field sites, which
            // only its program's caches index.
            entry: target_entry(body, &plan, &shape, (settles, environment, cross)),
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

    /// Whether a callee of another program (where `cross`) may take the tier prologue: a lane
    /// body runs in its own program's context, which the activation enters first.
    fn claims_across(&self, cross: bool) -> bool {
        !cross || self.programs.js_jit_lane_methods_enabled()
    }

    /// Record a resolved `target`, also as its function's unless its closure is numeric; a
    /// `cross` one among the other programs' targets.
    fn remember_target(&mut self, target: ScriptCallTarget, numeric: bool, cross: bool) {
        let policy = self.heap_reads().memory_policy();
        if numeric {
            // A numeric body's target is its closure's, held per callee: another program's in
            // the cross-program set, which its callers activate across programs.
            if cross {
                self.lane_stack.remember_cross_target(target, policy);
            } else {
                self.lane_stack.remember_script_target(target, policy);
            }
        } else {
            self.lane_stack.remember_function_target(target, cross, policy);
        }
    }

    /// The capture slots a frameless target whose closure has environment `env` reads
    /// (`ScriptCallTarget::capture`), tagged `CAPTURE_STORES` when they admit every store,
    /// whose barrier this takes once.
    pub(crate) fn target_capture(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        env: Value,
    ) -> *const usize {
        // A frameless activation's function has no environment of its own: its depth-0
        // captures are the closure environment's dense slots (`Vm::environment_capture_slots`),
        // read here with the facts the store tag needs from the same cell.
        let Some(quench_runtime::tier_surface::EnvironmentRecord { function, slots, scope, .. }) = self.heap_reads().environment(env) else {
            return std::ptr::null();
        };
        // A root environment's slot may project a global var, a global lexical binding or an
        // import, which reads go through; one whose every slot is the binding (a strict
        // eval's) is read as any other.
        if *function == u32::MAX
            || (*function == quench_runtime::vm::ROOT_FUNCTION_ID && (scope.program() != Some(program.raw()) || !self.programs.root_slots_are_bindings(program)))
        {
            return std::ptr::null();
        }
        let Some(capture) = slots.dense_storage() else {
            return std::ptr::null();
        };
        if quench_runtime::vm::environment::direct_store_owner(code, program, *function, scope)
            .is_some_and(quench_runtime::vm::environment::owner_admits_every_direct_store)
        {
            self.write_barrier(env);
            return capture.map_addr(|address| address | CAPTURE_STORES);
        }
        capture
    }

    /// Whether `program` runs in the lane when dynamic execution is allowed:
    /// Wasm always, JavaScript where it and the VM are specialized. A lane run keeps its program and the VM's specialization, so the
    /// lane's field caches (`Vm::primary_own_field_hit` and its kin) need not test them.
    pub(crate) fn specialized_operations_enabled(&self, program: &ResidualProgram) -> bool {
        program.kind == quench_runtime::bytecode::ProgramKind::Wasm || (self.specialized && program.specialized)
    }

    /// Whether the lane may run program `id`: every program in a dynamic build, else one with a loaded AOT image.
    pub(crate) fn may_run_in_lane(&self, id: ProgramId, code: &ResidualProgram) -> bool {
        self.execution_mode.allows_dynamic_execution() || self.programs.ahead_images(id, code).is_some()
    }

    /// Whether an AOT image's bodies run where the lane enters them: with lane bodies, or as the only code.
    pub(crate) fn images_run_in_lane(&self) -> bool {
        self.programs.js_jit_lane_methods_enabled() || !self.execution_mode.allows_dynamic_execution()
    }

    pub(crate) fn admits_in_lane(&self, id: ProgramId, code: &ResidualProgram) -> bool {
        self.may_run_in_lane(id, code) && self.specialized_operations_enabled(code)
    }

    /// The words a lane run's direct calls push onto (`compiled::DirectCallee`): the addresses
    /// of the values' and the activations' tops, each owned here and bumped through its
    /// address by emitted code; the end of the values' capacity; and the end of the `frames`
    /// activations the run may nest above the activations' top, bounded by what the lane may
    /// still make of them.
    pub(crate) fn stack_words_in_lane(&mut self, frames: usize) -> LaneStackWords {
        let stack = &mut self.lane_stack;
        let frames = frames.min(stack.activation_limit.get().saturating_sub(stack.activation_count() + 1));
        // The one spill left for emitted code (one-frame-one-call.md F5): its direct calls push
        // native frames onto the records' top and read no header yet. A run admitting no frames
        // pushes none (its bound is the top itself, so every push refuses to the helper row, which
        // spills), so its plain frames stay where they are.
        if frames != 0 { stack.activations(); }
        // SAFETY: reads the records' top alone; no reference into them lives.
        let records = unsafe { &mut *stack.activation_records.get() };
        let frames_end = records.top_ptr().wrapping_add(frames).addr();
        // SAFETY: the values' capacity bounds the vector's own allocation.
        let values_end = unsafe { stack.values.as_mut_ptr().add(stack.values.capacity()) }.addr();
        let activations_top = records.top_word().addr();
        let words = LaneStackWords {
            values_top: stack.values.top_word().addr(),
            values_end,
            activations_top,
            frames_end,
            plain_depth: stack.plain_frames_word(),
            plain_depth_limit: stack.plain_depth_limit.as_ptr().addr(),
            activation_count: stack.frames_below.as_ptr().addr(),
            body_table: stack.body_table.as_ptr().addr(),
            body_top: stack.body_top.as_ptr().addr(),
            native_table: stack.native_table.as_ptr().addr(),
        };
        // The bound is set last: `plain_frames_word` above voids it.
        stack.frames_below.set(stack.frames_below_plain());
        // Once native headers are enabled, emitted pushes keep body_top equal to the total count between bindings.
        debug_assert!(!native_frames_enabled() || stack.body_top.get() == stack.activation_count(), "body_top names the total frame count at binding");
        stack.body_top.set(stack.activation_count());
        // A binding that admits no direct-call frames admits no native headers either: a run with a
        // pending activation (a cell body entered as the context's leaf, `enter_cell_body_in_lane`)
        // has its caller still unmaterialized, and a header pushed over it would end above the
        // activation the leaf becomes, with a stale caller (earley-boyer under --u-jit). Its calls
        // take the helper row, which materializes the leaf first (`native_frames::in_innermost_frame`).
        stack.plain_depth_limit.set(if native_frames_enabled() && frames != 0 { stack.activation_limit.get().min(LANE_STACK_ACTIVATIONS) } else { 0 });
        words
    }

    /// The lane view a hot loop of `function` runs with (`Vm::note_back_edge`), or null.
    pub(crate) fn hot_loop_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView {
        self.view_any_in_lane(program, function).unwrap_or(std::ptr::null())
    }

    /// The lane view of `function`, derived on first use.
    pub(crate) fn view_any_in_lane(&self, program: ProgramId, function: u32) -> Option<*const LaneView> {
        let code = self.programs.residual(program)?;
        if !self.runs_lane || !self.may_run_in_lane(program, code) { return None; }
        let image = self.programs.ahead_images(program, code).is_some();
        self.programs.view_in_lane(program, function, |code, constants, kind, live, sites| {
            self.derived_view(function, code, constants, kind, live, sites, image)
        })
    }
}

#[quench_macros::ext(DispatchFastVm3)]
impl Vm {
    /// Prints every loaded function's lane view, derived afresh from its bytecode (no loop
    /// tier), when `QUENCH_TRACE=bytecode=lanes` is set: per record `LANEPLAN program function pc
    /// width handler settled | instruction`, handlers as run-time addresses (symbolized against
    /// the binary's symbol table, less the anchor's bias), `settled`
    /// the handler the stored view holds after the run (`-` where no run derived one).
    pub(crate) fn report_lane_plan(&mut self) {
        if quench_runtime::trace::Channel::Bytecode.value() != Some(quench_runtime::vm::inspection::LANE_PLAN) {
            return;
        }
        // A function whose load deferred its compilation (`CallFirstChunk`) compiles its chunk
        // now, as its first call would, so the plan holds its body.
        let mut stubs = Vec::new();
        for index in 0..self.programs.len() {
            let Some(residual) = self.programs.get(ProgramId::from_raw(index as u32)) else { continue };
            residual.compile_released();
            for function in 0..residual.functions.len() as u32 {
                if residual.function_get(function as usize).is_some_and(|code| code.instructions().any(|instruction| instruction.op().traits().has(quench_runtime::bytecode::Traits::FIRST_CALL_CHUNK))) {
                    stubs.push((residual.clone(), ProgramId::from_raw(index as u32), function));
                }
            }
        }
        eprintln!("LANEPLAN-ANCHOR {:x}", plan_anchor_in_lane as fn() as usize);
        for (residual, id, function) in stubs {
            if let Ok(FunctionKind::User(chunk, body)) = self.compile_first_chunk(&residual, (id, function)) {
                eprintln!("LANEPLAN-CHUNK {} {function} {} {body}", id.raw(), chunk.raw());
            }
        }
        for index in 0..self.programs.len() {
            let id = ProgramId::from_raw(index as u32);
            let Some(residual) = self.programs.get(id) else { continue };
            residual.compile_released();
            for function in 0..residual.functions.len() as u32 {
                let Some(code) = residual.function_get(function as usize) else { continue };
                let name = code.name.map_or("<anonymous>", |atom| residual.atoms[atom as usize].as_ref());
                let plan = self.programs.plan_lane_view(id, function, |code, constants, kind, live, sites| {
                    derive_lane_view(function, code, constants, self.heap_reads(), kind, live, sites, crate::compiled::admission::record_plan(false), false, self.programs.tiers())
                });
                let Some((fresh, settled)) = plan else { continue };
                eprintln!("LANEPLAN-FUNCTION {index} {function} {name}");
                for (pc, instruction) in code.instructions().enumerate() {
                    // SAFETY: `pc` names a record of each view (both derived from `code`); no
                    // lane runs while the report reads them.
                    let handler = unsafe { (*fresh.records[pc].get()).handler };
                    let settled = settled.map_or_else(|| "-".to_owned(), |view| format!("{:x}", unsafe { (*view.records[pc].get()).handler }.addr()));
                    eprintln!("LANEPLAN {index} {function} {pc} {:x} {settled} | {}", handler.addr(), instruction.text()); // merge: HEAD-only fused_width column dropped.
                }
            }
        }
    }
}

/// A function whose run-time address the lane plan prints (`LANEPLAN-ANCHOR`): less its
/// address in the symbol table, it is the executable's load bias, PIE or not.
#[inline(never)]
fn plan_anchor_in_lane() {
    std::hint::black_box(());
}

script_handler! {
    /// `script_call_miss` of a linked target whose entry is not a plain activation: the site
    /// calls the form for that entry directly from now on, an environment or keeping
    /// activation its own handler and any other entry the out-of-line form, which sorts them
    /// out. Out of line, reached by `sibling!`, so `script_call_miss` makes no call.
    #[inline(never)]
    fn script_call_requicken_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let target = vm.linked_script_call_entry(r.get(i.b), &i.call_link);
        // SAFETY: as `Vm::script_call_entry`; nothing before this read resolves a target.
        let entry = target.map(|target| unsafe { (*target).entry });
        let handler: Handler = match entry {
            Some(CallEntry::Environment) => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::ENVIRONMENT as u16 }>(),
            Some(CallEntry::Settles) => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::SETTLES as u16 }>(),
            Some(CallEntry::Keeps | CallEntry::Gathers) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::SCRIPT_CALL_KEEPING as u16 }>(),
            Some(entry) if entry.leaf_form() != NOT_LEAF => leaf_call_handler(entry, i),
            Some(_) => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>(),
            None => sibling!(script_call_resolve_miss, r, ip, Carried::script_in(cx, view)),
        };
        requicken(ip, handler);
        sibling!(handler, r, ip, Carried::script_in(cx, view))
    }
}

/// Sets whether the lane run calls script functions (`LaneContext::script_calls`), which the
/// lane's admission mirrors (`LaneStack::set_script_calls`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn set_script_calls_in_lane(cx: *mut LaneContext, allowed: bool) {
    unsafe {
        (*cx).script_calls = allowed;
        (*cx).returns_script_frame = allowed && !(*cx).inline_calls;
        vm(cx).lane_stack.set_script_calls(allowed);
    }
}

script_handler! {
    /// A JavaScript `Call` of a frameless target (`Vm::resolve_script_call_target`): the
    /// callee runs in the lane on a window of the lane stack, with the frame facts its
    /// push would have set held by the activation until it returns or the lane exits.
    /// Any other call takes the general path. `ARGC` is the record's argument count where
    /// the view resolved it (`script_call_handler`), so the window's copy makes no test, else
    /// `ANY_ARGC`. `PLAIN` where the record's receiver is its function's `undefined`
    /// register (`Function::undefined_register`, `specialize_script_calls`), whose value the
    /// handler then knows without reading it.
    fn script_call_miss<ARGC: u16, PLAIN: bool>(cx, r, ip, i, view) {
        // A run that calls no script functions refuses every cached call
        // (`Admission::NO_SCRIPT_CALLS`), which the link's test below makes.
        debug_assert!(unsafe { (*cx).script_calls || !vm(cx).lane_stack.script_calls_allowed() });
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        // Every fact of the target the link settled is in its entry's admission word, which
        // the lane's must equal whole (link-proofs.md): the generation, the program, the
        // run's conditions (`REFUSED`: a pending construction or `with`; `NO_SCRIPT_CALLS`),
        // the tier's diagnostics (`DIAGNOSED`) and the proof (`UNPROVEN`: an entry no plain call
        // proved yet, taken back when its cell publishes). The index is the
        // link masked to the cache, so it needs no bounds test: any entry it lands on is
        // tested whole. What stays per call: the callee's identity (entries are shared by
        // every site and callee), the argument count against the entry's parameters (a
        // site's, not an entry's fact), and the stacks' room (`plain_room`).
        let entries = &vm.lane_stack.script_targets.entries;
        let index = usize::from(i.call_link.get()) & (entries.len() - 1);
        // SAFETY: the cache's length is a power of two (`SCRIPT_CALL_TARGETS_INITIAL`), so the
        // masked index lies within it; nothing below resolves a target or collects.
        let target = unsafe { &*std::ptr::from_ref(entries.get_unchecked(index)) };
        let argc = call_argument_count::<ARGC>(i);
        if target.callee != callee
            || target.admission != vm.lane_stack.admission
            || argc > FIXED_ARGUMENT_COPIES
            || !quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(argc))
        {
            sibling!(script_call_prove_miss::<ARGC, PLAIN>, r, ip, Carried::script_in(cx, view))
        }
        debug_assert!(!constructing_in_lane(vm, callee));
        // A leaf entry its own call form proved (`script_call_leaf_prove_miss`) activates as
        // `Activates` from any other call (`CallEntry::activates`).
        debug_assert!(target.entry.activates());
        debug_assert!(target.program() == unsafe { (*cx).program });
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        lane_plain_activation!(cx, vm, r, ip, i, target, ARGC, PLAIN, proven)
    }
}

script_handler! {
    /// `script_call_miss` of a site whose link is not proved: the target it links, which
    /// takes its entry's form where that is not a plain activation
    /// (`script_call_requicken_miss`) and the out-of-line form where the site's arguments are
    /// more than it takes, else the entry is proved (`Admission::UNPROVEN` cleared) and the
    /// call runs plainly, a callee whose cell holds a body too (the activation tests the cell).
    #[cold]
    #[inline(never)]
    fn script_call_prove_miss<ARGC: u16, PLAIN: bool>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let Some(target) = vm.linked_script_call_entry(r.get(i.b), &i.call_link) else {
            sibling!(script_call_resolve_miss, r, ip, Carried::script_in(cx, view))
        };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let target = unsafe { &*target };
        if target.entry != CallEntry::Activates {
            sibling!(script_call_requicken_miss, r, ip, Carried::script_in(cx, view))
        }
        let argc = call_argument_count::<ARGC>(i);
        if argc > FIXED_ARGUMENT_COPIES || !quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(argc)) {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        }
        // A callee whose cell holds a body is proved too: the plain activation tests the cell
        // on every call, proved or not (`lane_plain_activation!`), and enters the body through
        // it (`enter_cell_body_in_lane`) or, for a call completing its caller, through the
        // activation's `BodyEntry` record. Only a leaf entry's proved form skips the cell, and
        // this proves no leaf entry (`CallEntry::Activates` above).
        // The proof (`Admission::UNPROVEN`): the entry, which the link names, now admits proved
        // calls from every site whose arguments it takes (`script_call_miss`'s compare).
        let (index, resolved) = (link_index(i.call_link.get()), std::ptr::from_ref(target));
        let entry = &mut vm.lane_stack.script_targets.entries[index];
        debug_assert!(std::ptr::eq(entry, resolved), "the link names the entry it resolved");
        entry.admission.0 &= !Admission::UNPROVEN;
        debug_assert!(entry.admission == vm.lane_stack.admission, "an admitted entry's word is the lane's");
        i.call_link.set(i.call_link.get() | PLAIN_LINK);
        sibling!(script_call_miss::<ARGC, PLAIN>, r, ip, Carried::script_in(cx, view))
    }
}

/// The plain activation of `target`, an `Activates` entry, for the call record `i` at `ip`
/// whose argument count is `ARGC` (`call_argument_count`), past its target's proof
/// (`script_call_miss`, `script_call_leaf_miss`).
macro_rules! lane_plain_activation {
    ($cx:expr, $vm:expr, $r:expr, $ip:expr, $i:expr, $target:expr, $argc:ident, $plain:expr) => {
        lane_plain_activation!($cx, $vm, $r, $ip, $i, $target, $argc, $plain, false)
    };
    ($cx:expr, $vm:expr, $r:expr, $ip:expr, $i:expr, $target:expr, $argc:ident, $plain:expr, proven) => {
        lane_plain_activation!($cx, $vm, $r, $ip, $i, $target, $argc, $plain, true)
    };
    ($cx:expr, $vm:expr, $r:expr, $ip:expr, $i:expr, $target:expr, $argc:ident, $plain:expr, $proven:literal) => {{
        let (cx, vm, r, ip, i, target) = ($cx, $vm, $r, $ip, $i, $target);
        // A plain call's receiver is `undefined` by its record (`script_call_miss`'s `PLAIN`).
        // Its `this` is then its target's (`ScriptCallTarget::lexical_this`), one load.
        // A receiver whose binding needs the heap takes the out-of-line form, so this
        // handler makes no call and keeps its registers.
        let this = if $plain {
            debug_assert_eq!(direct_call_this_in_lane(vm, target, Value::UNDEFINED), Some(target.lexical_this));
            target.lexical_this
        } else {
            let Some(this) = direct_call_this_in_lane(vm, target, r.get(i.c)) else {
                sibling!(script_call_full_miss, r, ip, Carried::script(cx))
            };
            this
        };
        // Values past the parameters and a full lane stack take the out-of-line form too,
        // whose activation keeps them or grows the stack (`activate_in_lane`), and so does another
        // program's target (`push_activation_in_lane`).
        // So does a window past `FIXED_ARGUMENT_COPIES`, whose copy is a call.
        // A proved call (`script_call_miss`) tested the argument count against the entry it
        // found; the room stays per call, one compare (`LaneStack::plain_room`).
        let argc = call_argument_count::<$argc>(i);
        // The plain depth the proved push writes past (`LaneStack::plain_room`), loaded once.
        let mut depth = vm.lane_stack.plain.depth.get();
        if if $proven {
            debug_assert!(argc <= FIXED_ARGUMENT_COPIES && quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(argc)));
            debug_assert_eq!(vm.lane_stack.plain_room(depth, target.room), activation_room_in_lane(vm, target));
            !vm.lane_stack.plain_room(depth, target.room)
        } else {
            argc > FIXED_ARGUMENT_COPIES || !admits_window_in_lane(vm, target, argc)
        } {
            sibling!(script_call_full_miss, r, ip, Carried::script(cx))
        }
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        // A `with` scope refuses every cached target (`LaneStack::refuse_cached_calls`), and
        // only an admitted one reaches here.
        debug_assert!(!vm.with_scope_entered);
        if returns && !returning_call_admitted(cx) {
            return exit(ip);
        }
        // SAFETY: the record's window of `r` holds its values.
        let passed = unsafe { r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))).cast_const() };
        // A callee whose entry cell holds a body runs it before any activation exists.
        if !returns && unsafe { (*target.view).entry_published() } {
            if let Some(resumed) = enter_cell_body_in_lane(cx, r, ip, target, passed, argc, this) {
                resume!(cx, resumed)
            }
            // A declined body may have made the plain activations records (entering emitted
            // code spills them): the push writes over the depth it left, and a first plain
            // activation names the records' count to the ones above (`PlainFrames::bound`).
            depth = vm.lane_stack.plain.depth.get();
            if depth == 0 { vm.lane_stack.refresh_plain_bound(vm.lane_stack.values.top().addr()); }
        }
        debug_assert!(!target.settles(), "a settling target activates through its own call form");
        let callee_registers = fill_window_in_lane(vm, target, passed, argc);
        let (registers, base) = if $proven {
            enter_plain_window(cx, vm, r, ip, target, callee_registers, depth, argc, this)
        } else {
            enter_window_of_in_lane::<false, false, true>(cx, vm, r, ip, target, callee_registers, argc, this, false, None, None)
        };
        next!(cx, registers, base, Carried::script(cx))
    }};
}
use lane_plain_activation;

pub(crate) mod leaf_calls;
use leaf_calls::{complete_leaf_in_lane, lane_enter_leaf, AdmittedWindow, materialize_running_leaf_in_lane, leaf_call_handler};

/// Refuses the direct calls of a lane body or region about to run in `registers` where the running leaf owns them: a
/// leaf's activation is pending (`script_call_leaf_miss`, `enter_cell_body_in_lane`), and a direct row's push would put
/// the callee's frame (and, at a leaf's unmoved top, its window over the leaf's registers) below the activation the
/// leaf makes on a leave, which then resumes the wrong frame. Zeroing the plain depth limit makes every direct row
/// refuse to its helper row, which makes the leaf's activation first (`in_innermost_frame`). Called after the run's
/// stack words are bound, which set the limit.
pub(crate) fn refuse_direct_calls_in_leaf(lane_context: *mut (), registers: *mut Value) {
    let cx = lane_context.cast::<LaneContext>();
    // SAFETY: a lane body's context is the running lane's, which nothing else accesses.
    if cx.is_null() || unsafe { (*cx).leaf.call }.is_null() || unsafe { (*cx).registers.0 } != registers {
        return;
    }
    // The body runs in the leaf only where no activation owns `registers` yet: a body's call made from inside the
    // leaf pushed the callee's activation on its own window, which the context names while the leaf stays set.
    let stack = &unsafe { vm(cx) }.lane_stack;
    if stack.top_frame().is_some_and(|frame| frame.window == registers.addr()) {
        return;
    }
    stack.frames_changed();
}

/// Make the running leaf (`LaneContext::leaf`) the activation its call would have pushed
/// (`enter_window_in_lane`), before anything observes activations: their materialization
/// (`Vm::materialize_lane_exit`) at the lane's exit, a getter's
/// activation. Its registers, records and PC stay where they are, so a handler may make it
/// anywhere in its record.
#[cold]
#[inline(never)]
fn materialize_leaf_in_lane(cx: *mut LaneContext) {
    // SAFETY: the running lane's context.
    let leaf = unsafe { std::mem::replace(&mut (*cx).leaf, NO_LEAF_CALL) };
    let vm = unsafe { vm(cx) };
    // SAFETY: the leaf's target stays in its cache while it runs, and its call record
    // belongs to the caller's live view.
    let (target, call) = unsafe { (&*leaf.target, &*leaf.call) };
    let form = leaf.form;
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: the leaf's window starts at the stack's length, within its capacity
    // (`script_call_leaf_miss`).
    let window = unsafe { (*cx).registers.0 };
    unsafe { stack.values.set_top(window.add(usize::from(target.registers) + lane_values::CONTEXT)) };
    let activation = Activation::of_parts(
        target.function, leaf.call, leaf.caller_base, leaf.caller_registers, leaf.caller_program.unwrap_or(unsafe { (*cx).program }),
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc: leaf.argc,
            completion: if leaf.caller_program.is_some() { Completion::Cross } else { Completion::Plain },
            forwards: false,
            kept: 0,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the call proved room for the activation (`activation_room_in_lane`).
    unsafe { stack.push_activation(activation, window) };
    // The receiver the call proved it binds directly (`script_call_leaf_miss`), from the
    // caller's registers, which the leaf does not write, for a form that binds none yet.
    let this = (!leaf_binds_this(form)).then(|| {
        direct_call_this_in_lane(vm, target, leaf.caller_registers.get(call.c))
            .expect("the leaf's call proved its receiver's binding")
            .to_bits()
    });
    // The context words the form left to its caller, by the staged row on machine words.
    quench_runtime::staged::call_leaf::materialize_leaf_context(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()), window.expose_provenance() as u64, form, this,
        target.env.to_bits(), target.capture.expose_provenance() as u64,
    );
}

/// The `script_call_miss` argument count of a record its view did not resolve.
const ANY_ARGC: u16 = u16::MAX;

/// The argument count of the call record `i`: `ARGC`, unless that is `ANY_ARGC`.
#[inline(always)]
fn call_argument_count<const ARGC: u16>(i: &LaneInstruction) -> u16 {
    if ARGC == ANY_ARGC { ImmediateLayout::argument_count(i.imm()) } else { ARGC }
}

/// The protocol row of a `Call` record that runs the miss body `MISS` (`protocol_misses`).
const fn call_row<const MISS: u16>() -> Handler {
    protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, MISS>()
}

/// The protocol row of a `Call` record whose argument count is `argc` (`ANY_ARGC` past four),
/// not `PLAIN` (`protocol_misses::script_call`).
const fn script_call_row(argc: u16) -> Handler {
    use protocol_misses::script_call as code;
    match argc {
        0 => call_row::<{ protocol_family::SCRIPT_CALL | code::ARGC_0 as u16 }>(),
        1 => call_row::<{ protocol_family::SCRIPT_CALL | code::ARGC_1 as u16 }>(),
        2 => call_row::<{ protocol_family::SCRIPT_CALL | code::ARGC_2 as u16 }>(),
        3 => call_row::<{ protocol_family::SCRIPT_CALL | code::ARGC_3 as u16 }>(),
        4 => call_row::<{ protocol_family::SCRIPT_CALL | code::ARGC_4 as u16 }>(),
        _ => call_row::<{ protocol_family::SCRIPT_CALL | code::ARGC_ANY as u16 }>(),
    }
}

/// `script_call_row` of a record whose receiver is its function's `undefined` register, `PLAIN`.
const fn plain_script_call_row(argc: u16) -> Handler {
    use protocol_misses::script_call as code;
    match argc {
        0 => call_row::<{ protocol_family::SCRIPT_CALL | code::PLAIN_ARGC_0 as u16 }>(),
        1 => call_row::<{ protocol_family::SCRIPT_CALL | code::PLAIN_ARGC_1 as u16 }>(),
        2 => call_row::<{ protocol_family::SCRIPT_CALL | code::PLAIN_ARGC_2 as u16 }>(),
        3 => call_row::<{ protocol_family::SCRIPT_CALL | code::PLAIN_ARGC_3 as u16 }>(),
        4 => call_row::<{ protocol_family::SCRIPT_CALL | code::PLAIN_ARGC_4 as u16 }>(),
        _ => call_row::<{ protocol_family::SCRIPT_CALL | code::PLAIN_ARGC_ANY as u16 }>(),
    }
}

/// The protocol row of a call record whose immediate is `imm`: that of its argument count where
/// the copy of its window is one fixed move (`FIXED_ARGUMENT_COPIES`), else the general one.
fn script_call_handler(imm: u32) -> Handler {
    script_call_handler_of::<false>(imm)
}

/// `script_call_handler` of a record whose receiver is its function's `undefined` register
/// where `PLAIN`.
fn script_call_handler_of<const PLAIN: bool>(imm: u32) -> Handler {
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    let argc = ImmediateLayout::argument_count(imm);
    if PLAIN { plain_script_call_row(argc) } else { script_call_row(argc) }
}

/// Gives each call record the protocol row of its argument count, and of a receiver that is the
/// function's `undefined` register (`undefined`).
fn specialize_script_calls(records: &mut [LaneInstruction], undefined: Option<quench_runtime::bytecode::Register>) {
    let general = script_call_row(ANY_ARGC) as *const ();
    for record in records.iter_mut().filter(|record| record.handler == general) {
        let handler = if undefined.is_some_and(|register| u16::from(register) == record.c) {
            script_call_handler_of::<true>(record.imm())
        } else {
            script_call_handler(record.imm())
        };
        record.handler = handler as *const ();
    }
}

/// The `this` a plain call of `target` binds for `receiver`: an arrow's lexical one
/// (`ScriptCallTarget::lexical_this`), else `call_this_value`'s without its boxing of
/// primitives, which allocates, so `None` for a sloppy callee's primitive.
#[inline(always)]
fn call_this_in_lane(vm: &Vm, target: &ScriptCallTarget, receiver: Value) -> Option<Value> {
    match direct_call_this_in_lane(vm, target, receiver) {
        Some(this) => Some(this),
        None => vm.is_object_like(receiver).then_some(receiver),
    }
}

/// `call_this_in_lane` where the receiver's own bits decide it: a strict callee, an object in
/// the direct record table, or `null`/`undefined`. `None` for any other receiver, whose
/// object test reads the heap.
#[inline(always)]
fn direct_call_this_in_lane(
    vm: &Vm,
    target: &ScriptCallTarget,
    receiver: Value,
) -> Option<Value> {
    match target.this_binding {
        ThisBinding::Lexical => Some(target.lexical_this),
        binding => receiver_this_in_lane(vm, binding, receiver),
    }
}

/// `direct_call_this_in_lane` of a binding other than an arrow's own `this`, which the
/// receiver alone decides; `None` for `ThisBinding::Lexical`.
#[inline(always)]
fn receiver_this_in_lane(vm: &Vm, binding: ThisBinding, receiver: Value) -> Option<Value> {
    // Compares, not a table: a sloppy method's object receiver, the common call, takes the
    // first two.
    if binding != ThisBinding::Sloppy {
        // An arrow whose environment binds no `this` takes its caller's receiver.
        return (binding != ThisBinding::Lexical).then_some(receiver);
    }
    // The staged binding (`protocol::bind_this`): an object receiver binds itself, `undefined`
    // and `null` bind the global object, and any other receiver is the host's to box.
    quench_runtime::staged::protocol::concrete_bind_this(receiver.to_bits(), vm.realm.globals.to_bits()).map(Value::from_bits)
}

script_handler! {
    /// `script_call_miss` out of line, so the plain call keeps its registers: a receiver
    /// whose binding reads the heap (`call_this_in_lane`), a forwarding target, which takes
    /// `script_call_keeping_miss`, and a target whose body is a read (`CallEntry::Reads`,
    /// `CallEntry::ReadsElement`), whose hit (`call_read_in_lane`) completes the
    /// call with its value as the callee's return would; a call that returns its callee's
    /// completion completes its caller in turn (`lane_return_value!`). Anything else
    /// activates the callee. A site that called such a target calls this form directly, so it
    /// makes `script_call_miss`'s own entry tests.
    #[inline(never)]
    fn script_call_full_miss<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if constructing_in_lane(vm, callee) {
            sibling!(native_call_miss, r, ip, Carried::script_in(cx, view))
        }
        let Some(target) = vm.linked_script_call_entry(callee, &i.call_link) else {
            sibling!(script_call_resolve_miss, r, ip, Carried::script_in(cx, view))
        };
        // SAFETY: as `Vm::script_call_entry`; a read resolves no target and collects nothing.
        let target = unsafe { &*target };
        if target.entry == CallEntry::Frame {
            sibling!(frame_call_in_lane, r, ip, Carried::script_in(cx, view))
        }
        if target.entry == CallEntry::Environment {
            // The site calls its environment target's form directly from now on.
            requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::ENVIRONMENT as u16 }>());
            sibling!(script_call_environment_miss, r, ip, Carried::script_in(cx, view))
        }
        if target.settles() {
            sibling!(script_call_settles_miss, r, ip, Carried::script_in(cx, view))
        }
        if target.forwards() {
            sibling!(script_call_keeping_miss, r, ip, Carried::script_in(cx, view))
        }
        // A receiver whose binding reads the heap takes the general form.
        let Some(this) = direct_call_this_in_lane(vm, target, r.get(i.c)) else {
            sibling!(script_call_general_miss, r, ip, Carried::script_in(cx, view))
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        let arguments = ImmediateLayout::call_window_base(i.imm());
        let argc = ImmediateLayout::argument_count(i.imm());
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        // Past the forwards, settling, frame and environment tests above, `activation_form` holds
        // exactly when the entry activates (its plain or keeping form), so this is the same test.
        let value = if body_calls::activation_form(target, argc).is_some()
            || returns && !returning_call_admitted(cx)
        {
            None
        } else {
            let passed = ReadArguments { registers: r, base: arguments, passed: argc };
            // SAFETY: as `call_read_in_lane`.
            unsafe { answered_call_in_lane(cx, ip, target, passed, this) }
        };
        let Some(value) = value else {
            // An activation keeping values past the parameters, a full lane stack or another
            // program's target takes the general form (`activate_in_lane`). A callee that drops
            // the values past its parameters is passed those alone, as `activate_in_lane` passes them.
            if (argc > target.params && !target.drops_extra_arguments())
                || !activation_room_in_lane(vm, target)
                || (returns && !returning_call_admitted(cx))
                || vm.with_scope_entered
            {
                sibling!(script_call_general_miss, r, ip, Carried::script_in(cx, view))
            }
            // SAFETY: the call's window of `r` holds its `argc` values.
            let passed = unsafe { r.0.add(usize::from(arguments)).cast_const() };
            let (registers, base) =
                push_passed_in_lane::<false, false>(cx, r, ip, target, passed, argc.min(target.params), this, false);
            next!(cx, registers, base, Carried::script_in(cx, view))
        };
        if !returns {
            r.set(i.a, value);
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        lane_return_value!(cx, ip, value)
    }
}

script_handler! {
    /// `script_call_full_miss`'s activation of a call its form declines: a receiver whose
    /// binding reads the heap (`call_this_in_lane`), values past the parameters, which the
    /// keeping activation holds, or a full lane stack (`activate_in_lane`). Out of line, so the
    /// full form makes no call; it reads no target, which the full form already answered.
    #[inline(never)]
    fn script_call_general_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let target = vm
            .linked_script_call_entry(callee, &i.call_link)
            .filter(|_| !constructing_in_lane(vm, callee));
        // A linked target exists only while the run calls script functions (`linked_script_call_entry`).
        debug_assert!(unsafe { (*cx).script_calls } || target.is_none());
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let Some(target) = target.map(|target| unsafe { &*target }).filter(|target| {
            !matches!(target.entry, CallEntry::Frame | CallEntry::Environment | CallEntry::Settles) && !target.forwards()
        }) else {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        };
        let Some(this) = call_this_in_lane(vm, target, r.get(i.c)) else {
            return exit(ip);
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        let arguments = ImmediateLayout::call_window_base(i.imm());
        match activate_in_lane(cx, r, ip, target, arguments, this, false) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(next) => next,
        }
    }
}

script_handler! {
    /// `script_call_full_miss` of an environment target (`CallEntry::Environment`), out of
    /// line so the other calls keep their registers: its activation holds its environment
    /// and locals (`activate_environment_in_lane`). Any other callee takes the full form.
    #[inline(never)]
    fn script_call_environment_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if constructing_in_lane(vm, callee) {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        }
        // The site's linked entry, which a fresh closure of a function already resolved relinks
        // (`Vm::linked_script_call_entry`); a callee with no target yet resolves in the full form.
        // No entry exists while the run calls no script functions, so that case takes the full
        // form too, which exits (`script_call_full_miss`).
        let Some(target) = vm.linked_script_call_entry(callee, &i.call_link) else {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        };
        debug_assert!(unsafe { (*cx).script_calls });
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let target = unsafe { &*target };
        if target.entry != CallEntry::Environment {
            // A site this form serves calls the full form once its callee's target differs.
            requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>());
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        }
        let Some(this) = call_this_in_lane(vm, target, r.get(i.c)) else {
            return exit(ip);
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        // SAFETY: the call's window is registers of the running function.
        let passed = unsafe { call_window_in_lane(r, i) };
        match activate_environment_in_lane(cx, r, ip, target, passed, (this, false)) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(next) => next,
        }
    }
}

/// Whether `target`, a settling target of the running program (`CallEntry::Settles`), may
/// suspend: its first `await` leaves its caller the promise, which only a caller that keeps
/// the call's result can take (`Vm::try_settling_call`).
#[inline(never)]
fn settling_body_suspends(cx: *mut LaneContext, target: &ScriptCallTarget) -> bool {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    code.function_get(target.function as usize).is_none_or(|body| body.suspends())
}

script_handler! {
    /// `script_call_full_miss` of a non-suspending async target (`CallEntry::Settles`), out of
    /// line so the other calls keep their registers: the call makes its promise first, as
    /// `Vm::try_settling_call` does, then activates the body, whose return settles it
    /// (`Completion::SettlesPromise`). A call passing values past the parameters, a `with`
    /// scope or a host observing promise lifecycles takes the general path, which settles the
    /// promise in a frame, and so does a full lane stack, which grows first
    /// (`script_call_frame_in_lane`). Any other callee takes the full form.
    #[inline(never)]
    fn script_call_settles_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let target = vm
            .linked_script_call_entry(callee, &i.call_link)
            .filter(|_| !constructing_in_lane(vm, callee));
        // A linked target exists only while the run calls script functions (`linked_script_call_entry`).
        debug_assert!(unsafe { (*cx).script_calls } || target.is_none());
        // SAFETY: as `Vm::script_call_entry`; the target is copied before anything allocates.
        let Some(target) = target.map(|target| unsafe { *target }).filter(ScriptCallTarget::settles) else {
            requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>());
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        };
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        // A returning call of a body that may suspend takes the general path, as
        // `Vm::try_settling_call` declines it: the general loop's `Await` makes the
        // continuation only for a call whose caller keeps its result, and a caller that returns
        // the promise would otherwise suspend with it ("await requires async continuation").
        if ImmediateLayout::argument_count(i.imm()) > target.params
            || (returns && (!returning_call_admitted(cx) || settling_body_suspends(cx, &target)))
            || vm.with_scope_entered
            || vm.host.observes_promise_lifecycle()
        {
            return exit(ip);
        }
        if !activation_room_in_lane(vm, &target) {
            return script_call_frame_in_lane(ip, usize::from(target.room));
        }
        let Some(this) = call_this_in_lane(vm, &target, r.get(i.c)) else {
            return exit(ip);
        };
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, &target);
        // Without lifecycle hooks the promise's allocation runs no guest code and collects
        // nothing (`note_allocation` makes a collection due at the next frame boundary).
        let root = if ImmediateLayout::call_result_awaited_in_lane(i.imm()) {
            None
        } else {
            let Ok(promise) = vm.promise_object() else {
                return exit(ip);
            };
            note_allocation(vm);
            Some(vm.hold_root(promise))
        };
        let (registers, base) = if target.settles_in_environment() {
            // SAFETY: the call's window is registers of the running function.
            let passed = unsafe { call_window_in_lane(r, i) };
            match generator_yields::activate_settling_environment(cx, r, ip, &target, passed, this) {
                Ok(entered) => entered,
                Err(next) => {
                    // A refused activation releases the promise's root before any effect.
                    if let Some(root) = root {
                        vm.drop_root(root);
                    }
                    return next;
                }
            }
        } else {
            let arguments = ImmediateLayout::call_window_base(i.imm());
            push_activation_in_lane::<false, true>(cx, r, ip, &target, arguments, this, false)
        };
        // The promise rides in the activation's side record (one-frame-one-call.md F4 (3)).
        vm.lane_stack.attach_payload(SidePayload::Settles(root));
        next!(cx, registers, base, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// An iteration step (`iteration_step_in_lane`) of this realm's array iterator `next` over a
    /// plain array (`Vm::step_present_array_value`): the step in place, with a result object
    /// only the step's `done` and `value` reads observe (`Vm::reused_iterator_result`). Any
    /// other step is the call it is.
    fn iterator_step_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (method, iterator) = (r.get(i.b), r.get(i.c));
        match vm.iteration_step(iterator, method) {
            quench_runtime::vm::iterators::IterationStep::Generator if unsafe { (*cx).script_calls } => {
                sibling!(generator_step_miss::<0>, r, ip, Carried::script_in(cx, view))
            }
            quench_runtime::vm::iterators::IterationStep::Call => sibling!(script_call_miss::<ANY_ARGC, false>, r, ip, Carried::script_in(cx, view)),
            _ => {}
        }
        let Some(result) = array_iteration_result_in_lane(vm, iterator) else {
            sibling!(native_call_miss, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, result);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// An iteration step (`iteration_step_in_lane`) of this realm's generator `next` of a
    /// generator that is not running: the generator resumes as an activation over its record
    /// (`resume_generator_step_in_lane`), or, where its function has no window, as the native
    /// `next` runs it (`Vm::generator_iteration_step`), with a result only the step's `done`
    /// and `value` reads observe.
    /// `READS` names a for-of step whose `done` and `value` reads the generator's yields make
    /// (`Completion::StepReads`, the `ForOfTest` form `STEP_READS_EMITTED` or
    /// `STEP_READS_THREADED`), else `0`.
    #[inline(never)]
    fn generator_step_miss<READS: u8>(cx, r, ip, i, view) {
        let (method, iterator) = (r.get(i.b), r.get(i.c));
        let completion = match READS {
            STEP_READS_EMITTED => Completion::StepReads(ForOfTest::Emitted),
            STEP_READS_THREADED => Completion::StepReads(ForOfTest::Threaded),
            _ => Completion::Steps,
        };
        if let Some((registers, start)) = resume_generator_step_in_lane(cx, r, ip, i, method, iterator, completion) {
            next!(cx, registers, start, Carried::script_in(cx, view))
        }
        let value = call_from_frame_in_lane(cx, ip, |vm, code| vm.generator_iteration_step(code, method, iterator));
        resume!(cx, native_complete_in_lane(cx, ip, i, value))
    }
}

script_handler! {
    /// `iterator_step_miss` of a for-of step (`lane_for_of_step`) over a plain array or a
    /// String, Map or Set iterator (`Vm::step_in_place`): the
    /// step's `done` and `value` go to the registers its reads of `done` and `value` write,
    /// and the step goes on where they would, into the body or out of the loop. The result
    /// object only those reads observe is not written. Any other step runs as
    /// `iterator_step_miss` and the records after it.
    fn iterator_step_reads_miss<THREADED: bool>(cx, r, ip, i, view) {
        let form = if THREADED { ForOfTest::Threaded } else { ForOfTest::Emitted };
        let vm = unsafe { vm(cx) };
        let (method, iterator) = (r.get(i.b), r.get(i.c));
        // SAFETY: the view checked the step's records (`for_of_step_in_lane`).
        let (done_read, value_read) = unsafe { (&*ip.add(FOR_OF_DONE_READ), &*ip.add(form.value_read())) };
        let reads_step = call_iteration::concrete_reads_step(done_read.imm(), value_read.imm(), step_atoms_of(vm));
        let reads = call_iteration::StepReads { done: done_read.a, value: value_read.a, flag: None };
        let script_calls = unsafe { (*cx).script_calls };
        if !reads_step {
            if script_calls && vm.steps_as_generator(iterator, method) {
                sibling!(generator_step_miss::<0>, r, ip, Carried::script_in(cx, view))
            }
            sibling!(iterator_step_miss, r, ip, Carried::script_in(cx, view))
        }
        let step = match vm.step_in_place(iterator, method) {
            InPlaceStep::Element(step) => step,
            InPlaceStep::Item(step) => {
                note_allocation(vm);
                step
            }
            // A generator's step resumes it over its record, as `iterator_step_miss` makes it.
            InPlaceStep::Generator if script_calls && THREADED => {
                sibling!(generator_step_miss::<STEP_READS_THREADED>, r, ip, Carried::script_in(cx, view))
            }
            InPlaceStep::Generator if script_calls => {
                sibling!(generator_step_miss::<STEP_READS_EMITTED>, r, ip, Carried::script_in(cx, view))
            }
            _ => sibling!(iterator_step_miss, r, ip, Carried::script_in(cx, view)),
        };
        match step {
            Some(value) => {
                call_iteration::concrete_step_yielded(r.0, reads, value);
                // SAFETY: the step's records end at its `value` read, never the view's last.
                next!(cx, r, unsafe { ip.add(form.records()) }, Carried::script_in(cx, view))
            }
            None => {
                call_iteration::concrete_step_finished(r.0, reads);
                // The exit runs as its own record, a `Jump` or the threaded test of the `done`
                // just written, which the generated row runs as its own record.
                // makes that move before landing past it.
                next!(cx, r, unsafe { ip.add(form.exit()) }, Carried::script_in(cx, view))
            }
        }
    }
}

script_handler! {
    /// `iterator_step_miss` of a destructuring step (`binding_step_in_lane`) over a plain array
    /// or a String, Map or Set iterator: the step's `done` goes to the register its `done`
    /// read writes and the pattern's done flag the next record moves it to, its `value` to
    /// the register its `value` read writes, and the step goes on past them. The result
    /// object only those reads observe is not written. Any other step runs as
    /// `iterator_step_miss` and the records after it.
    fn iterator_step_binding_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (method, iterator) = (r.get(i.b), r.get(i.c));
        // SAFETY: the view checked the step's records (`binding_step_in_lane`).
        let (done_read, flag, value_read) =
            unsafe { (&*ip.add(BINDING_DONE_READ), &*ip.add(BINDING_DONE_MOVE), &*ip.add(BINDING_VALUE_READ)) };
        let reads_step = call_iteration::concrete_reads_step(done_read.imm(), value_read.imm(), step_atoms_of(vm));
        let reads = call_iteration::StepReads { done: done_read.a, value: value_read.a, flag: Some(flag.a) };
        if !reads_step {
            sibling!(iterator_step_miss, r, ip, Carried::script_in(cx, view))
        }
        let step = match vm.step_in_place(iterator, method) {
            InPlaceStep::Element(step) => step,
            InPlaceStep::Item(step) => {
                note_allocation(vm);
                step
            }
            InPlaceStep::Generator | InPlaceStep::Call => sibling!(iterator_step_miss, r, ip, Carried::script_in(cx, view)),
        };
        match step {
            Some(value) => {
                call_iteration::concrete_step_yielded(r.0, reads, value);
                // SAFETY: the step's records end at its `value` read, never the view's last.
                next!(cx, r, unsafe { ip.add(BINDING_VALUE_READ + 1) }, Carried::script_in(cx, view))
            }
            None => {
                call_iteration::concrete_step_finished(r.0, reads);
                // The threaded test of the `done` just written leaves the step.
                next!(cx, r, unsafe { ip.add(BINDING_DONE_TEST) }, Carried::script_in(cx, view))
            }
        }
    }
}

use quench_runtime::staged::call_iteration;

/// The realm's atoms an iteration step's `done` and `value` reads name
/// (`call_iteration::reads_step`).
#[inline(always)]
fn step_atoms_of(vm: &Vm) -> call_iteration::StepAtoms {
    call_iteration::StepAtoms { done: u64::from(vm.runtime_atoms.done), value: u64::from(vm.runtime_atoms.value) }
}

/// `iterator_step_miss`'s step and result, out of line: the first result allocates.
#[inline(never)]
fn array_iteration_result_in_lane(vm: &mut Vm, iterator: Value) -> Option<Value> {
    let step = match vm.step_present_array_value(iterator) {
        Some(step) => step,
        None => Some(vm.step_collection_value(iterator)?),
    };
    let result = vm.reused_iterator_result(step.unwrap_or(Value::UNDEFINED), step.is_none());
    note_allocation(vm);
    Some(result)
}

script_handler! {
    /// A JavaScript `Call` of another program's frameless callee (`LaneStack::cross_targets`):
    /// its activation runs in the callee's program, which its return leaves
    /// (`Completion::Cross`). A site whose callee is no such target calls `script_call_miss`
    /// again.
    #[inline(never)]
    fn script_call_cross_miss<>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if constructing_in_lane(vm, callee) {
            sibling!(native_call_miss, r, ip, Carried::script_in(cx, view))
        }
        let Some(target) = vm.linked_cross_call_entry(callee, &i.call_link) else {
            requicken(ip, script_call_handler(i.imm()));
            sibling!(script_call_miss::<ANY_ARGC, false>, r, ip, Carried::script_in(cx, view))
        };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let target = unsafe { &*target };
        // A callee that needs a frame takes it as `frame_call_in_lane` pushes one.
        if !target.entry.activates_across() && target.entry != CallEntry::Environment {
            #[cfg(feature = "profile-lane-exits")]
            quench_runtime::vm::lane_exits::record_detail(format!("cross call to frame entry={:?}", target.entry as u8));
            sibling!(frame_call_in_lane, r, ip, Carried::script_in(cx, view))
        }
        #[cfg(feature = "profile-lane-exits")]
        profile_script_call_target(cx, ip, callee, target);
        // A leaf runs on a window without an activation, as its own program's calls run it.
        if let Some((registers, base)) = leaf_calls::cross_leaf_call_in_lane(cx, r, ip, i, target) {
            next!(cx, registers, base, Carried::script(cx))
        }
        let Some(this) = call_this_in_lane(vm, target, r.get(i.c)) else {
            return exit(ip);
        };
        let arguments = ImmediateLayout::call_window_base(i.imm());
        // A plain activation of a window the fixed copies take, not completing its caller,
        // pushes as `script_call_miss`'s does, entering the callee's program
        // (`enter_window_in_lane`'s cross completion); the site calls its fixed form from now on
        // (`script_call_cross_plain_miss`).
        let argc = ImmediateLayout::argument_count(i.imm());
        if target.entry == CallEntry::Activates
            && i.a & quench_runtime::bytecode::RETURN_REGISTER == 0
            && argc <= FIXED_ARGUMENT_COPIES
            && admits_window_in_lane(vm, target, argc)
        {
            // The link now proves the plain form (`PLAIN_LINK`): this callee's entry activates
            // plainly and its window takes `argc`, facts fixed for the callee in this generation.
            i.call_link.set(i.call_link.get() | PLAIN_LINK);
            requicken(ip, cross_plain_handler(argc));
            // SAFETY: the record's window of `r` holds its values.
            let passed = unsafe { r.0.add(usize::from(arguments)).cast_const() };
            let (registers, base) = push_passed_in_lane::<true, false>(cx, r, ip, target, passed, argc, this, false);
            next!(cx, registers, base, Carried::script(cx))
        }
        let activated = if target.entry == CallEntry::Environment {
            // SAFETY: the call's window is registers of the running function.
            activate_environment_in_lane(cx, r, ip, target, unsafe { call_window_in_lane(r, i) }, (this, false))
        } else if target.forwards() {
            activate_keeping_in_lane::<true>(cx, r, ip, target, ActivationArguments::Window(arguments), this, false, None)
        } else {
            activate_in_lane(cx, r, ip, target, arguments, this, false)
        };
        match activated {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(next) => next,
        }
    }
}

script_handler! {
    /// `script_call_cross_miss` at a site whose last callee was another program's plain
    /// activation (`CallEntry::Activates`) of `ARGC` arguments, which no completion of its
    /// caller follows: that activation alone, with the fixed copy of its window. Any other
    /// call takes the general cross form.
    fn script_call_cross_plain_miss<ARGC: u16>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        // The link proves the call (`PLAIN_LINK`, set where the general form requickened to
        // this one): its admission (`Admission::admits_across`) that the run calls script
        // functions with no construction pending, and, for this callee in this generation, a
        // plain entry whose window takes `ARGC`. A link without the bit names no entry, so any
        // other callee or a refused run falls to the general form, which decides and relinks.
        let index = usize::from(i.call_link.get() ^ PLAIN_LINK);
        if !vm.cross_call_holds(index, callee) {
            sibling!(script_call_cross_miss, r, ip, Carried::script_in(cx, view))
        }
        // SAFETY: `cross_call_holds` found the entry within the cache; nothing below resolves a
        // target or collects.
        let target = unsafe { &*std::ptr::from_ref(vm.lane_stack.cross_targets.entries.get_unchecked(index)) };
        debug_assert!(target.entry == CallEntry::Activates && quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(ARGC)));
        // The stack's room is the one test that stays per call: it is the push's depth check.
        if !activation_room_in_lane(vm, target) {
            sibling!(script_call_cross_miss, r, ip, Carried::script_in(cx, view))
        }
        let Some(this) = direct_call_this_in_lane(vm, target, r.get(i.c)) else {
            sibling!(script_call_cross_miss, r, ip, Carried::script_in(cx, view))
        };
        // SAFETY: the record's window of `r` holds its values.
        let passed = unsafe { r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))).cast_const() };
        let (registers, base) = push_passed_in_lane::<true, false>(cx, r, ip, target, passed, ARGC, this, false);
        next!(cx, registers, base, Carried::script(cx))
    }
}

/// The `script_call_cross_plain_miss` instantiation of a window of `argc` values, at most
/// `FIXED_ARGUMENT_COPIES`.
fn cross_plain_handler(argc: u16) -> Handler {
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    match argc {
        0 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_0 as u16 }>(),
        1 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_1 as u16 }>(),
        2 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_2 as u16 }>(),
        3 => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_3 as u16 }>(),
        _ => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS_PLAIN_4 as u16 }>(),
    }
}

script_handler! {
    /// `script_call_miss` of a callee its target cache does not hold: resolving it
    /// (`Vm::resolve_script_call_target`) makes a call, so it runs out of line and the call
    /// continues at `script_call_miss`, which then finds it, or at `native_call_miss`.
    #[inline(never)]
    fn script_call_resolve_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        if vm.cross_call_entry(callee).is_some() {
            requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS as u16 }>());
            sibling!(script_call_cross_miss, r, ip, Carried::script_in(cx, view))
        }
        if let Some(target) = vm.resolve_script_call_target(code, callee) {
            if target.program() == vm.active_program {
                // Under a pending construction no cached target admits a call
                // (`LaneStack::set_refused`): the call leaves the lane, as it would. Nor in a run that
                // makes no script calls (`NO_SCRIPT_CALLS`), whose admission no target carries: the
                // miss would resolve the target again forever (zlib-wasm, tsf-wasm under `--no-jit`).
                if vm.lane_stack.refuses() || !vm.lane_stack.script_calls_allowed() {
                    return exit(ip);
                }
                sibling!(script_call_miss::<ANY_ARGC, false>, r, ip, Carried::script_in(cx, view))
            }
            // Another program's callee that needs a frame takes it as `frame_call_in_lane`
            // pushes one.
            if !target.entry.activates() {
                sibling!(frame_call_in_lane, r, ip, Carried::script_in(cx, view))
            }
            requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::CROSS as u16 }>());
            sibling!(script_call_cross_miss, r, ip, Carried::script_in(cx, view))
        }
        // A site calling a native takes the native handler from now on, which hands any other
        // callee back.
        if let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = vm.function_record(callee) {
            let native = *native;
            let handler = native_call_handler(native, i, || call_result_dead_in_lane(cx, ip, i), || regexp_calls::length_read(cx, ip, i));
            requicken(ip, handler);
            if handler as *const () != call_row::<{ protocol_family::NATIVE_CALL | protocol_misses::native_call::NATIVE as u16 }>() as *const () {
                native_calls::install_math_field_call(cx, ip, native);
            }
        }
        sibling!(native_call_miss, r, ip, Carried::script_in(cx, view))
    }
}

/// The by-name load a `LoadName` handler runs for: a `LoadNameCall` also
/// writes its call's receiver, and a `LoadNameTypeof` reads an absent global as `undefined`.
const LOAD_NAME: u8 = 0;
/// `LOAD_NAME` for `LoadNameCall`.
const LOAD_NAME_CALL: u8 = 1;
/// `LOAD_NAME` for `LoadNameTypeof`.
const LOAD_NAME_TYPEOF: u8 = 2;

/// What a by-name load's first run resolves its name to: the `name_row` instance its
/// record runs from then on, which holds the binding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NameResolution {
    /// The active realm's function for the native an internal name stands for
    /// (`Vm::intrinsic_natives`), which no binding can shadow and no scope affects.
    Native,
    /// The global `undefined`, which is non-writable and non-configurable and which no global
    /// lexical declaration may shadow (GlobalDeclarationInstantiation rejects a restricted
    /// global property): a read of it is always `undefined`.
    Undefined,
    /// A name the global object answers whenever no global lexical binding shadows it: a function
    /// whose unresolved names are global (`ProgramStore::names_are_global`) and resolve
    /// statically, and a name no internal binding owns. Static resolution rules out a `with`
    /// object, which the general search consults first.
    Global,
    /// An intrinsic name (`bytecode::is_intrinsic_name`), as the compiler emits for
    /// `{ __proto__: v }` or `super`: the native it stands for (`Vm::intrinsic_name_value`).
    Intrinsic,
    /// Any other name, which the general path resolves.
    General,
}

/// The resolution of the by-name load of `atom` in the function `cx` runs. A native an internal
/// name stands for is recorded for its reads (`Vm::intrinsic_natives`). An internal name is no
/// binding source text can declare, except the internal globals the realm installs on its global
/// object, which a static lookup reads as any other.
fn name_resolution(cx: *mut LaneContext, atom: u32) -> NameResolution {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    let internal = vm.atom_class(atom).contains(AtomClass::INTERNAL);
    if internal && let Some(native) = quench_runtime::bytecode::intrinsic_name_native(&code.atoms[atom as usize]) {
        let slot = atom as usize;
        if vm.intrinsic_natives.len() <= slot {
            vm.intrinsic_natives.resize(slot + 1, None);
        }
        vm.intrinsic_natives[slot] = Some(native);
        return NameResolution::Native;
    }
    // The running function decides the name's scope: a running leaf's is its materialized
    // activation's.
    materialize_running_leaf_in_lane(cx);
    let function = running_function_in_lane(cx);
    let global = code.function_get(function as usize).is_some_and(|function| function.names_resolve_statically(code))
        && vm.programs.names_are_global(unsafe { (*cx).program })
        && (!internal || quench_runtime::bytecode::INTERNAL_GLOBAL_BINDINGS.contains(&vm.atom_name(atom)));
    match (global, vm.atom_name(atom)) {
        (true, UNDEFINED_GLOBAL) => NameResolution::Undefined,
        (true, _) => NameResolution::Global,
        (false, name) if quench_runtime::bytecode::is_intrinsic_name(name) => NameResolution::Intrinsic,
        (false, _) => NameResolution::General,
    }
}

script_handler! {
    /// A by-name load's first run, the miss its generated row takes (`name_load_miss`): the VM's
    /// resolution of its name (`name_resolution`) fills the record, whose handler from then on
    /// is the row instance (`name_row`) holding the binding, or leaves every run to the general
    /// path. `KIND` is the load's (`LOAD_NAME`).
    #[inline(never)]
    fn load_name_resolve_miss<KIND: u8>(cx, r, ip, i, view) {
        let load = match KIND {
            LOAD_NAME_CALL => quench_runtime::staged::NameLoad::Call,
            LOAD_NAME_TYPEOF => quench_runtime::staged::NameLoad::Typeof,
            _ => quench_runtime::staged::NameLoad::Read,
        };
        let bound: Handler = match name_resolution(cx, i.imm()) {
            NameResolution::Native => name_row::<FLAG_NAME_NATIVE>(load),
            NameResolution::Undefined => name_row::<FLAG_NAME_UNDEFINED>(load),
            NameResolution::Global => name_row::<FLAG_NAME_GLOBAL>(load),
            NameResolution::Intrinsic => name_row::<FLAG_NAME_INTRINSIC>(load),
            NameResolution::General => {
                // SAFETY: the context's program outlives the lane run.
                let code = unsafe { &*(*cx).code };
                if code.function_get(running_function_in_lane(cx) as usize).is_some_and(|body| body.call_plan(code).holds_window_scope()) {
                    let scoped = name_row::<FLAG_SCOPE>(load);
                    requicken(ip, scoped);
                    sibling!(scoped, r, ip, Carried::script_in(cx, view))
                }
                // Any other name's scope is a runtime fact, which the general path's read settles
                // (`name_probe_miss`).
                if !unsafe { vm(cx) }.atom_class(i.imm()).contains(AtomClass::INTERNAL) {
                    // The probe is the row's miss at `FLAG_PROBE` (`name_probe`), which settles the record after.
                    let probing = name_row::<{ lane_frame::FLAG_PROBE }>(load);
                    requicken(ip, probing);
                    sibling!(probing, r, ip, Carried::script_in(cx, view))
                }
                requicken(ip, exit_lane);
                return exit(ip);
            }
        };
        requicken(ip, bound);
        sibling!(bound, r, ip, Carried::script_in(cx, view))
    }
}

/// The function the lane runs: its innermost activation's, or else its frame's.
#[inline(always)]
fn running_function_in_lane(cx: *mut LaneContext) -> u32 {
    let vm = unsafe { vm(cx) };
    vm.lane_stack.innermost_function().unwrap_or_else(|| vm.frames[unsafe { (*cx).frame() }].function)
}

/// The global value property `name_resolution` answers without a lookup.
const UNDEFINED_GLOBAL: &str = "undefined";

script_handler! {
    /// A `StoreName` row's miss (`Miss::StoreNameFirst`), the name's first-run resolution: an
    /// assignment to a name the global object answers whenever no global lexical binding
    /// shadows it (as `name_resolution` decides for a read) takes
    /// `store_global_name_miss`, a `var`'s initializer too, which stores as one: the
    /// declaration made the property, and a lexical binding is never hinted. The record stays
    /// on its row, which stores through the hint this arm sets. Any other store always takes
    /// the general path.
    #[inline(never)]
    fn store_name_first_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let function = running_function_in_lane(cx);
        let global = !vm.atom_class(i.imm()).contains(AtomClass::INTERNAL)
            && code.function_get(function as usize).is_some_and(|function| function.names_resolve_statically(code))
            && vm.programs.names_are_global(unsafe { (*cx).program });
        if global {
            sibling!(store_global_name_miss, r, ip, Carried::script_in(cx, view))
        }
        if code.function_get(function as usize).is_some_and(|body| body.call_plan(code).holds_window_scope()) {
            let scoped = lane_frame::scoped_store_row();
            requicken(ip, scoped);
            sibling!(scoped, r, ip, Carried::script(cx))
        }
        requicken(ip, exit_lane);
        exit(ip)
    }
}

script_handler! {
    /// The global name store past the row's hint (`global_name_store`): the global object's own
    /// writable data field, which becomes the hint where no realm declares the name lexically
    /// (`Vm::global_name_hintable`); any other store takes the general path, a shadowed name's
    /// from then on.
    #[inline(never)]
    fn store_global_name_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (atom, globals) = (i.imm(), vm.realm.globals);
        // A lexical declaration of the name is never withdrawn, so its stores always take the
        // general path.
        if !vm.global_name_hintable(atom) {
            requicken(ip, exit_lane);
            return exit(ip);
        }
        let Some(hint) = vm
            .own_data_field_hint::<true>(globals, atom)
            .filter(|hint| vm.hinted_own_field_store_hit(*hint, globals, atom, r.get(i.a)))
        else {
            return exit(ip);
        };
        set_field_hint(ip, hint);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `name_row` of a global, past its record's hint: an
    /// initialized global lexical binding of
    /// the name, which the general path's search finds first there; else the global object's
    /// own data field, which becomes the hint (`Vm::own_data_field_hint`), or its cached field
    /// (`Vm::cached_field_hit`). Any other read takes `global_name_read_miss`, or the general
    /// path for an uninitialized binding. `typeof` (`LOAD_NAME_TYPEOF`) reads an absent name as
    /// `undefined`.
    #[inline(never)]
    fn global_name_refresh_miss<KIND: u8, SCOPED: bool>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let atom = i.imm();
        // A read the general path found past a scope that could declare the name holds only
        // until some scope does (`Vm::note_dynamic_scope_name`).
        if SCOPED && vm.scope_names.declared(atom) {
            requicken(ip, exit_lane);
            return exit(ip);
        }
        let value = match vm.global_name_read(code, atom, i.c) {
            quench_runtime::vm::field_cache::GlobalNameRead::Value(value, hint) => {
                if let Some(hint) = hint {
                    set_field_hint(ip, hint);
                } else if !SCOPED
                    && let Some(index) = vm.realm.global_lexical_bindings.get_index_of(&atom)
                    && let Ok(index) = u16::try_from(index)
                    && index != NO_CALL_LINK
                {
                    // A declared name's read names its binding by index (`FLAG_NAME_LEXICAL`'s row),
                    // which the global lexical bindings never move.
                    set_field_hint(ip, quench_runtime::vm::FieldCache::own(atom, index));
                    let load = match KIND {
                        LOAD_NAME_CALL => quench_runtime::staged::NameLoad::Call,
                        LOAD_NAME_TYPEOF => quench_runtime::staged::NameLoad::Typeof,
                        _ => quench_runtime::staged::NameLoad::Read,
                    };
                    requicken(ip, name_row::<FLAG_NAME_LEXICAL>(load));
                }
                value
            }
            quench_runtime::vm::field_cache::GlobalNameRead::Uninitialized => return exit(ip),
            quench_runtime::vm::field_cache::GlobalNameRead::Miss => sibling!(global_name_read_miss::<KIND>, r, ip, Carried::script_in(cx, view)),
        };
        r.set(i.a, value);
        if KIND == LOAD_NAME_CALL {
            r.set(i.b, Value::UNDEFINED);
        }
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `global_name_refresh_miss` past its lexical binding and global property: a
    /// global accessor's getter, activated in the lane, or a plain global property, recorded
    /// in the site's entry; any other read takes the general path. Out of line: the getter's
    /// activation and the lookups take stack temporaries, which the common read then does not
    /// hold.
    #[inline(never)]
    fn global_name_read_miss<KIND: u8>(cx, r, ip, i, view) {
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
            return match activate_getter_in_lane(cx, r, ip, getter, globals, None) {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
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
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `new Array(...values)` of an intrinsic `Array`, which allocates without running guest
    /// code: the array of the values passed, or for one Number argument the array of that
    /// length (`Vm::array_of_length`), with the callee's own `prototype`. An invalid length,
    /// and any other construction, take the general path.
    #[inline(never)]
    fn native_construct_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        match vm.function_record(callee) {
            Some(FunctionRecord { kind: FunctionKind::User(..), .. }) => {
                if let Some(parent) = super_forwarded_parent_in_lane(vm, callee) {
                    resume!(cx, construct_forwarded_parent_in_lane(cx, r, ip, i, callee, parent));
                }
                if let Some(this) = super_construction(cx, r, i, callee) {
                    // Allocating the receiver can make a collection due.
                    note_allocation(vm);
                    if returns {
                        resume!(cx, return_constructed_in_lane(cx, ip, this))
                    }
                    r.set(i.a, this);
                    next!(cx, r, following(ip), Carried::script_in(cx, view))
                }
                return frame_construct_in_lane(cx, r, ip, i);
            }
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. })
                if native.constructs_from_lane() && !returns =>
            {
                let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
                // SAFETY: the construction's window is registers of the running function.
                let args = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.c)), argc) };
                let Some(value) = construct_native_in_lane(cx, ip, callee, callee, args) else {
                    return exit(ip);
                };
                // The construction ran from the innermost frame, whose registers are the lane's.
                let r = unsafe { (*cx).registers };
                r.set(i.a, value);
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
            _ => {}
        }
        if returns && !vm.lane_stack.has_frames() {
            return exit(ip);
        }
        let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
        // SAFETY: the construction's window is registers of the running function.
        let args = unsafe { std::slice::from_raw_parts(r.0.add(usize::from(i.c)), argc) };
        let Some(array) = vm.intrinsic_array_construction(callee, args) else {
            return exit(ip);
        };
        note_allocation(vm);
        if returns {
            let Some(resumed) = complete_activation_in_lane(cx, ip, array) else {
                unreachable!("a returning lane construction has an activation caller");
            };
            resume!(cx, resumed)
        }
        r.set(i.a, array);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// A call that forwards without an observable step of its own (`Vm::forwarded_call`):
    /// `f.call(thisArg, ...args)`, a bound function without bound arguments, or an
    /// uncurried builtin (`uncurryThis(f)(thisArg, ...args)`). A frameless target's
    /// activation is the one `script_call_miss` makes, and a native runs as
    /// `native_call_miss` runs it, with the forwarded receiver and arguments. Any other
    /// takes the general path.
    #[inline(never)]
    fn function_call_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the call's window is registers of the running function, which the
        // activation or native call copies before anything can move them.
        let window = unsafe { call_window_in_lane(r, i) };
        // `f.call(thisArg, ...args)` of a callee the site linked (`forwarded_target_in_lane`):
        // the forwarding is one step, so its activation needs no walk of `forwarded_call`.
        // SAFETY: a call record's operand is `call`.
        let link = unsafe { (*ip).operand.call.memo } as usize;
        if vm.script_call_holds(link, r.get(i.c))
            && matches!(vm.function_record(r.get(i.b)), Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionCall), .. }))
            && !unsafe { (*cx).tiers_run }
        {
            let (receiver, args) = window.split_first().map_or((Value::UNDEFINED, &[][..]), |(first, rest)| (*first, rest));
            resume!(cx, function_call_activation_in_lane(cx, r, ip, i, r.get(i.c), receiver, args))
        }
        // A bound function without bound arguments the lane last forwarded through, whose
        // target the site linked: its target and receiver, read once (`LaneStack::bound_call`).
        if let Some((bound, target, this, list)) = vm.lane_stack.bound_call
            && bound == r.get(i.b)
            && list.is_undefined()
            && vm.script_call_holds(link, target)
            && !unsafe { (*cx).tiers_run }
        {
            resume!(cx, function_call_activation_in_lane(cx, r, ip, i, target, this, window))
        }
        // The bound function with bound arguments the lane last called: its words are read
        // once (`LaneStack::bound_call`), so the call walks no `forwarded_call`.
        if let Some((bound, _, _, list)) = vm.lane_stack.bound_call
            && bound == r.get(i.b)
            && !list.is_undefined()
        {
            resume!(cx, bound_arguments_call_in_lane(cx, r, ip, i));
        }
        let Some(quench_runtime::vm::function::Forwarded { callee, receiver, args, kind, through_call }) = vm.forwarded_call(r.get(i.b), r.get(i.c), window) else {
            resume!(cx, bound_arguments_call_in_lane(cx, r, ip, i));
        };
        // The function a forwarded call calls is the site's callee, through its native, for a
        // body that inlines it (`lane_unit::Planner::through_call`).
        if through_call
            && unsafe { (*cx).tiers_run }
            && let Some((program, function, _, false)) = vm.same_realm_script_target(callee)
            && program == vm.active_program
        {
            vm.note_site_callee(&i.call_link, crate::compiled::CallTarget { callee: crate::compiled::CalleeBinding::Forwarded { through: r.get(i.b), callee }, function });
        }
        if let Some(FunctionKind::Native(native)) = kind {
            if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 && !returning_call_admitted(cx) {
                return exit(ip);
            }
            let value = native_value_in_lane(cx, ip, i, native, callee, receiver, args);
            resume!(cx, native_complete_in_lane(cx, ip, i, value));
        }
        if matches!(kind, Some(FunctionKind::User(..))) {
            remember_bound_call_in_lane(vm, r.get(i.b));
        }
        resume!(cx, function_call_activation_in_lane(cx, r, ip, i, callee, receiver, args))
    }
}

/// Remembers `callee` for `function_call_miss`'s bound fast paths (`LaneStack::bound_call`)
/// when it is a bound function of a user function: its words, with an empty list of bound
/// arguments kept as `undefined`, so the window alone is what such a call passes.
#[inline(never)]
fn remember_bound_call_in_lane(vm: &mut Vm, callee: Value) {
    let Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionBoundCall), env, .. }) = vm.function_record(callee) else {
        return;
    };
    let Some((target, this, list)) = vm.bound_function_words(*env) else { return };
    let list = if vm.heap_reads().array_elements(list).is_some_and(|elements| elements.len() != 0) { list } else { Value::UNDEFINED };
    if matches!(vm.function_record(target), Some(FunctionRecord { kind: FunctionKind::User(..), .. })) {
        vm.lane_stack.bound_call = Some((callee, target, this, list));
    }
}

/// `function_call_miss` of a bound function with bound arguments, which lead the values the
/// call passes (ECMA-262 10.4.1.1): the forwarded call with the values copied out
/// (`Vm::forwarded_call`), a user function's activation or frame call
/// (`FrameCallArguments::List`), else the general path.
#[inline(never)]
fn bound_arguments_call_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: as in `function_call_miss`; the values are copied before anything runs.
    let window = unsafe { call_window_in_lane(r, i) };
    // The bound function the lane last called (`LaneStack::bound_call`): its target, a user
    // function, and its receiver as remembered, and the passed values filled once, the bound
    // list's elements then the window, with no walk of its parts.
    let remembered = match vm.lane_stack.bound_call {
        Some((bound, target, this, list)) if bound == r.get(i.b) && !list.is_undefined() => {
            vm.heap_reads().array_elements(list).map(|elements| {
                let bound = elements.as_slice();
                let mut args = smallvec::SmallVec::<[Value; FRAME_CALL_INLINE_ARGUMENTS]>::with_capacity(bound.len() + window.len());
                args.extend_from_slice(&bound);
                args.extend_from_slice(window);
                (target, this, args)
            })
        }
        _ => None,
    };
    let (callee, receiver, args) = match remembered {
        Some(call) => call,
        None => {
            // The window is read in place by the walk and copied once, behind the bound
            // prefix that leads it (`WindowArgs`).
            let walked = vm.forwarded_call(r.get(i.b), r.get(i.c), quench_runtime::vm::function::WindowArgs::<FRAME_CALL_INLINE_ARGUMENTS>::Window(window));
            let Some(quench_runtime::vm::function::Forwarded { callee, receiver, args, kind, .. }) = walked else {
                return Resumed::Exit(exit(ip));
            };
            if !matches!(kind, Some(FunctionKind::User(..))) {
                return Resumed::Exit(exit(ip));
            }
            let args = args.into_owned();
            remember_bound_call_in_lane(vm, r.get(i.b));
            (callee, receiver, args)
        }
    };
    let Some(target) = passed_target_in_lane(cx, callee, args.len()) else {
        return passed_frame_call_in_lane(cx, r, ip, i, callee, receiver, FrameCallArguments::List(&args));
    };
    let Some(this) = call_this_in_lane(vm, &target, receiver) else {
        return Resumed::Exit(exit(ip));
    };
    match activate_passed_in_lane(cx, r, ip, &target, &args, this) {
        Ok((registers, base)) => Resumed::At(registers, base, Carried::script(cx)),
        Err(next) => Resumed::Exit(next),
    }
}

/// `function_call_miss`'s activation of the frameless `callee` with the forwarded `receiver`
/// and `args`: the callee's registers and first record, or the record to continue at. Out of line, as `construct_keeping_activation_in_lane`.
#[inline(never)]
fn function_call_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Resumed {
    let vm = unsafe { vm(cx) };
    let mut resolved = None;
    let Some(target) = forwarded_target_in_lane(cx, ip, callee, args.len(), &mut resolved) else {
        return Resumed::Exit(forwarded_frame_call_in_lane(cx, r, ip, i, callee, receiver, args));
    };
    // `call_this_value` without its boxing of primitives, which allocates.
    let Some(this) = call_this_in_lane(vm, target, receiver) else {
        return Resumed::Exit(exit(ip));
    };
    if let Some(resumed) = forwarded_cell_entry_in_lane(cx, r, ip, i, target, args, this) {
        return resumed;
    }
    match activate_passed_in_lane(cx, r, ip, target, args, this) {
        Ok((registers, base)) => Resumed::At(registers, base, Carried::script(cx)),
        Err(next) => Resumed::Exit(next),
    }
}

/// `passed_target_in_lane` of a forwarded call's `callee` at the call record `ip`, whose read
/// memo slot links the callee's cache entry, as an array callback method's call links its
/// callback's (`callback_target_in_lane`): the link, tested first, skips the set's hash and the
/// cross-program and environment lookups for a callee the running program activates.
#[inline(always)]
fn forwarded_target_in_lane<'a>(
    cx: *mut LaneContext,
    ip: Ip,
    callee: Value,
    passed: usize,
    resolved: &'a mut Option<ScriptCallTarget>,
) -> Option<&'a ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // SAFETY: a call record's operand is `call`.
    let link = unsafe { (*ip).operand.call.memo } as usize;
    if vm.script_call_holds(link, callee) && !constructing_in_lane(vm, callee) {
        // SAFETY: `script_call_holds` found the entry within the cache, which nothing
        // resolves into before the caller reads it; read in place, not copied.
        let target = unsafe { &*std::ptr::from_ref(vm.lane_stack.script_targets.entries.get_unchecked(link)) };
        if admits_activation_in_lane(target) {
            return Some(target);
        }
    }
    let target = resolved.insert(passed_target_in_lane(cx, callee, passed)?);
    if let Some(index) = vm.script_call_index(callee) {
        // SAFETY: as `requicken`: the record's operand is the call's, written in place.
        unsafe { (*ip.cast_mut()).operand.call.memo = index as u32 };
    }
    Some(target)
}

/// `enter_cell_body_in_lane` of a forwarded call of a plain target of the running program, passed
/// at most its parameters and not completing its caller, whose entry cell holds a body.
#[inline(always)]
fn forwarded_cell_entry_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: &ScriptCallTarget,
    args: &[Value],
    this: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let argc = u16::try_from(args.len()).ok().filter(|argc| *argc <= target.params)?;
    (target.entry == CallEntry::Activates
        && !crosses_in_lane::<true>(cx, target)
        && i.a & quench_runtime::bytecode::RETURN_REGISTER == 0
        && !vm.with_scope_entered
        && activation_room_in_lane(vm, target)
        && unsafe { (*target.view).entry_published() })
    .then_some(())?;
    enter_cell_body_in_lane(cx, r, ip, target, args.as_ptr(), argc, this)
}

/// The frameless target a forwarded or applied call of `callee` with `passed` values
/// activates: one of the running program (`script_target_in_lane`), another program's plain,
/// keeping or environment activation (`cross_call_entry`), or the running program's
/// environment target, which `script_target_in_lane` leaves to plain calls; an environment
/// target only when it binds each value to a parameter (`activate_environment_in_lane`).
#[inline(never)]
fn passed_target_in_lane(cx: *mut LaneContext, callee: Value, passed: usize) -> Option<ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // Matched in turn rather than chained: the combinators moved the target through copies.
    let target = match script_target_in_lane(cx, callee) {
        Some(target) => target,
        None => {
            // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
            let cross = (!constructing_in_lane(vm, callee))
                .then(|| vm.cross_call_entry(callee))
                .flatten()
                .map(|entry| unsafe { &*entry })
                .filter(|target| target.entry.activates_across() || target.entry == CallEntry::Environment);
            // SAFETY: as above.
            let own = || {
                vm.script_call_entry(callee)
                    .map(|entry| unsafe { &*entry })
                    .filter(|target| target.entry == CallEntry::Environment && target.program() == vm.active_program)
            };
            match cross {
                Some(target) => *target,
                None => *own()?,
            }
        }
    };
    if target.entry == CallEntry::Environment
        && passed > usize::from(target.params)
        && !vm.programs.residual(target.program())?.function(target.function as usize).rest
    {
        return None;
    }
    Some(target)
}

/// The activation of a `passed_target_in_lane` with receiver `this` and the `passed` values:
/// an environment activation, a plain call's push where the callee keeps nothing past its
/// parameters, or a keeping one, which keeps them; either may enter another program
/// (`Completion::Cross`).
#[inline(always)]
fn activate_passed_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    if target.entry == CallEntry::Environment {
        return activate_environment_in_lane(cx, r, ip, target, passed, (this, false));
    }
    activate_kept_in_lane::<true>(cx, r, ip, target, passed, this)
}

/// The activation of `target`, of the running program unless `MAY_CROSS`, for the record at
/// `ip` with receiver `this` and the `passed` values, which lie outside the caller's call
/// window (a forwarded or applied call's, a getter's none): a callee that keeps no passed
/// values (`ScriptCallTarget::forwards`) and is passed at most its parameters, or drops the
/// rest, takes the plain call's push (`activate_in_lane`); any other, a keeping activation.
#[inline(always)]
fn activate_kept_in_lane<const MAY_CROSS: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    let argc = u16::try_from(passed.len()).unwrap_or(u16::MAX);
    // SAFETY: `ip` is a record of the running view.
    let returns = unsafe { (*ip).a } & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if !target.forwards()
        && (argc <= target.params || target.drops_extra_arguments())
        && (!returns || returning_call_admitted(cx))
        && !vm.with_scope_entered
        && activation_room_in_lane(vm, target)
    {
        return Ok(push_passed_in_lane::<MAY_CROSS, false>(cx, r, ip, target, passed.as_ptr(), argc.min(target.params), this, false));
    }
    let arguments = ActivationArguments::Kept(passed.as_ptr(), passed.len());
    activate_keeping_in_lane::<MAY_CROSS>(cx, r, ip, target, arguments, this, false, None)
}

/// `function_call_miss` of a forwarded `callee` with no frameless target: a user function
/// the general loop pushes in place (`Vm::in_loop_call_target`) takes its frame as
/// `frame_call_in_lane` makes it, with the forwarded `receiver` and `args`, the call window's
/// tail. Any other takes the general path.
#[inline(never)]
fn forwarded_frame_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Ip {
    let skip = usize::from(ImmediateLayout::argument_count(i.imm())) - args.len();
    passed_frame_call_in_lane(cx, r, ip, i, callee, receiver, FrameCallArguments::Window(skip)).into_loop()
}

/// The frame call of `callee` with `receiver` and `passed` that `forwarded_frame_call_in_lane`
/// and an applied call (`apply_activation_in_lane`) make.
fn passed_frame_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    receiver: Value,
    passed: FrameCallArguments<'_>,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if !unsafe { (*cx).script_calls } || returns && !returning_call_admitted(cx) {
        return Resumed::Exit(exit(ip));
    }
    let Some(target) = vm.in_loop_call_target(code, callee) else {
        #[cfg(feature = "profile-lane-exits")]
        quench_runtime::vm::lane_exits::record_detail(format!(
            "passed frame call declined kind={:?} realm_matches={} name={:?}",
            vm.function_record(callee).map(|record| match record.kind {
                FunctionKind::Native(Native::HostFunction) => vm.host_function_name(callee).unwrap_or_default().to_string(),
                FunctionKind::Native(native) => format!("{native:?}"),
                FunctionKind::User(..) => "user".to_string(),
            }),
            vm.function_record(callee).is_some_and(|record| vm.record_realm(record) == vm.realm.globals),
            vm.function_record(callee).and_then(|record| match record.kind {
                FunctionKind::User(program, function) => vm.programs.residual(program).and_then(|code| {
                    let body = code.function_get(function as usize)?;
                    Some(format!("{} @{:?} async={} generator={} in_loop={}",
                        body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
                        body.source_span.map(|span| span.start), body.is_async, body.is_generator, body.call_plan(code).in_loop))
                }),
                _ => None,
            }),
        ));
        return Resumed::Exit(exit(ip));
    };
    push_frame_call_in_lane(cx, r, ip, i, target, callee, receiver, passed)
}

/// The values a frame call passes its callee (`push_frame_call_in_lane`).
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
    /// native the running frame calls in place (`call_native_in_lane`); out of line, so script
    /// calls keep their code. Any other call takes the general path.
    #[inline(never)]
    fn native_call_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // The callee's kind, read once: a call changes no function record before it runs.
        let native = match vm.function_record(r.get(i.b)) {
            Some(FunctionRecord {
                kind: FunctionKind::Native(native),
                ..
            }) => *native,
            _ => sibling!(frame_call_in_lane, r, ip, Carried::script_in(cx, view)),
        };
        if matches!(native, Native::FunctionCall | Native::FunctionBoundCall) {
            sibling!(function_call_miss, r, ip, Carried::script_in(cx, view))
        }
        if matches!(native, Native::FunctionApply | Native::ReflectApply) {
            sibling!(apply_call_miss, r, ip, Carried::script_in(cx, view))
        }
        if native == Native::ReflectGet {
            sibling!(reflect_get_call_handler(), r, ip, Carried::script_in(cx, view))
        }
        if native == Native::AsyncGeneratorNext
            && let Some(resumed) = async_requests::async_generator_next_in_lane(cx, r, ip, i)
        {
            resume!(cx, resumed)
        }
        if native == Native::AsyncGeneratorThrow
            && let Some(resumed) = async_requests::async_generator_throw_in_lane(cx, r, ip, i)
        {
            resume!(cx, resumed)
        }
        if native == Native::WasmExportedFunction
            && let Some(resumed) = lane_wasm_export_frame(cx, r, ip, i)
        {
            resume!(cx, resumed)
        }
        resume!(cx, native_call_value_in_lane(cx, r, ip, i, native))
    }
}

script_handler! {
    /// `native_call_miss` of `Function.prototype.apply` or `Reflect.apply`: the applied
    /// function's activation where its list allows one (`apply_activation_in_lane`), else the
    /// native's call. Apart, since the activation's result is a stack temporary of the
    /// handler that makes it, whose dispatch then is a call, not a sibling jump.
    #[inline(never)]
    fn apply_call_miss<>(cx, r, ip, i, view) {
        let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) =
            (unsafe { vm(cx) }).function_record(r.get(i.b))
        else {
            unreachable!("native_call_miss routes only a native apply here");
        };
        let native = *native;
        if let Some(activation) = apply_activation_in_lane(cx, r, ip, i, native) {
            return match activation {
                Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
                Err(next) => next,
            };
        }
        resume!(cx, native_call_value_in_lane(cx, r, ip, i, native))
    }
}

/// The native call `native_call_miss` makes in place (`native_value_in_lane`), completed in its
/// result register or as its caller's return (`native_complete_in_lane`).
#[inline(always)]
fn native_call_value_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
) -> Resumed {
    // A call that returns its result completes its caller: an activation in the lane, or
    // the frame in the general loop (`frame_return`).
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    // The running function is the same activation or, once the call made activations
    // frames (`lane_call_native`), the frame it became.
    if returns && !returning_call_admitted(cx) {
        return Resumed::Exit(exit(ip));
    }
    // A generator's `next` resumes it as an activation, which makes no frame of the caller's
    // (one-frame-one-call.md, frame switches).
    if let Some((registers, start)) = generator_windows::resume_called_generator(cx, r, ip, i, native) {
        return Resumed::At(registers, start, Carried::script(cx));
    }
    // SAFETY: the call's window is registers of the running function, which the native
    // call copies before anything can move them.
    let args = unsafe { call_window_in_lane(r, i) };
    let value = native_value_in_lane(cx, ip, i, native, r.get(i.b), r.get(i.c), args);
    native_complete_in_lane(cx, ip, i, value)
}

script_handler! {
    /// A call of one spread (`CallSpread`), whose window holds the iterable: an intact array
    /// (`Vm::array_iteration_intact`) of present own data elements (`present_dense_elements`)
    /// passes them, which iterating it would yield and the activation copies, as a spread call's argument array
    /// (`list_activation_in_lane`); any other iterable takes the general path, which builds it.
    #[inline(never)]
    fn call_spread_miss<>(cx, r, ip, i, view) {
        let list = r.get(ImmediateLayout::call_window_base(i.imm()));
        let Some(length) = (unsafe { vm(cx) }).intact_spread_length(list) else {
            if let Some(resumed) = list_calls::spread_set_native_call_in_lane(cx, ip, i, r.get(i.b), r.get(i.c), list) {
                resume!(cx, resumed)
            }
            return exit(ip);
        };
        if let Some((registers, base)) = spread_linked_call_in_lane(cx, r, ip, i, list, length) {
            next!(cx, registers, base, Carried::script_in(cx, view))
        }
        if let Some(resumed) = spread_native_call_in_lane(cx, ip, i, r.get(i.b), r.get(i.c), list) {
            resume!(cx, resumed)
        }
        match list_activation_in_lane(cx, r, ip, i, r.get(i.b), r.get(i.c), list, Some(length)) {
            Some(Ok((registers, base))) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Some(Err(next)) => next,
            None => exit(ip),
        }
    }
}

script_handler! {
    /// A spread call (`CallArray`), whose window holds the argument array the spread built:
    /// the callee's activation with its elements, as an applied call's
    /// (`list_activation_in_lane`); any other call takes the general path.
    #[inline(never)]
    fn call_array_miss<>(cx, r, ip, i, view) {
        let list = r.get(ImmediateLayout::call_window_base(i.imm()));
        let Some(length) = (unsafe { vm(cx) }).dense_list_length(list) else {
            return exit(ip);
        };
        match list_activation_in_lane(cx, r, ip, i, r.get(i.b), r.get(i.c), list, Some(length)) {
            Some(Ok((registers, base))) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Some(Err(next)) => next,
            None => exit(ip),
        }
    }
}

/// Values an applied frame call copies out without allocating their list.
const FRAME_CALL_INLINE_ARGUMENTS: usize = 8;

/// `call_spread_miss` of a callee the site links: the elements of the intact `list`, `length`
/// of them, passed as `passed_linked_call_in_lane` passes values, read in place from a wide
/// backing, decoded from a narrow one. `None` for any other call, which the general spread
/// path makes.
#[inline(always)]
fn spread_linked_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    list: Value,
    length: usize,
) -> Option<(Registers, Ip)> {
    let argc = u16::try_from(length).ok()?;
    let elements = unsafe { vm(cx) }.heap_reads().array_elements(list)?;
    let mut decoded = [Value::UNDEFINED; FRAME_CALL_INLINE_ARGUMENTS];
    let (passed, valid) = match elements.wide_values() {
        Some(values) => (values.as_ptr(), argc),
        None => {
            let mut valid = 0;
            for (slot, value) in decoded.iter_mut().zip(elements.iter().take(length)) {
                *slot = value;
                valid += 1;
            }
            (decoded.as_ptr(), valid)
        }
    };
    passed_linked_call_in_lane(cx, r, ip, i, passed, argc, valid)
}

/// A spread call of a callee the site links (`Vm::linked_script_call_entry`, as a plain
/// call's), passed the `argc` values at `passed`, of which the first `valid` may be read: a
/// rest body that keeps nothing as `activate_gathering_in_lane` activates it, else a target
/// keeping nothing (`ScriptCallTarget::forwards`) activated as a plain call is
/// (`push_passed_in_lane`), taking the values as its parameters or dropping those past them.
/// `passed` stays valid until the activation is made: no collection runs inside it. `None`
/// for any other call.
#[inline(always)]
fn passed_linked_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    passed: *const Value,
    argc: u16,
    valid: u16,
) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    if constructing_in_lane(vm, callee) {
        return None;
    }
    // SAFETY: as `Vm::script_call_entry`: read in place, not copied (64 bytes a call), since
    // nothing before the activation is made resolves a target or collects; the gathering
    // activation copies it before it allocates the rest array (`activate_gathering_in_lane`).
    let target = vm.linked_script_call_entry(callee, &i.call_link).map(|target| unsafe { &*target })?;
    if target.entry == CallEntry::Gathers && target.forms & TARGET_GATHERS_KEEPING == 0 {
        if !admits_activation_in_lane(&target) || valid < argc {
            return None;
        }
        // Refused here, not by the activation: without room it makes the lane's activations
        // frames (`script_call_frame_in_lane`), after which the caller may not go on in the lane.
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        let room = usize::from(target.room) + if target.forms & TARGET_REST_VIEW != 0 { usize::from(argc) } else { 0 };
        if (returns && !returning_call_admitted(cx))
            || vm.with_scope_entered
            || !quench_runtime::staged::call_keeping::concrete_admits_keeping(
                vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), room,
            )
        {
            return None;
        }
        let this = call_this_in_lane(vm, &target, r.get(i.c))?;
        return activate_gathering_in_lane(cx, r, ip, &target, passed, argc, this).ok();
    }
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if !admits_activation_in_lane(&target)
        || target.forwards()
        || (argc > target.params && !target.drops_extra_arguments())
        || (returns && !returning_call_admitted(cx))
        || vm.with_scope_entered
        || !activation_room_in_lane(vm, &target)
    {
        return None;
    }
    let this = call_this_in_lane(vm, &target, r.get(i.c))?;
    let argc = argc.min(target.params);
    if valid < argc {
        return None;
    }
    Some(push_passed_in_lane::<true, false>(cx, r, ip, &target, passed, argc, this, false))
}

script_handler! {
    /// `CallRest`: the spread of a rest parameter kept as a view. Its window holds the rest
    /// register; `undefined` there means the keeping activation's entry kept the values passed,
    /// which, while array iteration is intact, a linked callee takes in place
    /// (`passed_linked_call_in_lane`) with no array made. Otherwise the rest array they make is
    /// put in the window and the call is `call_spread_miss`'s, an array spread as any.
    #[inline(never)]
    fn call_rest_miss<>(cx, r, ip, i, view) {
        let window = ImmediateLayout::call_window_base(i.imm());
        if r.get(window) == Value::UNDEFINED
            && let Some((argc, function)) = unsafe { keeping_activation_in_lane(cx) }
        {
            let vm = unsafe { vm(cx) };
            let body = unsafe { &*(*cx).code }.function(function as usize);
            let fixed = body.params - 1;
            let count = argc.saturating_sub(fixed);
            // SAFETY: the keeping activation keeps its `argc` values after its registers.
            let kept = unsafe { r.0.add(usize::from(body.registers) + usize::from(fixed)).cast_const() };
            if vm.fresh_array_iteration_intact()
                && let Some((registers, base)) = passed_linked_call_in_lane(cx, r, ip, i, kept, count, count)
            {
                next!(cx, registers, base, Carried::script_in(cx, view))
            }
            let rest = vm.rest_parameter_array(unsafe { std::slice::from_raw_parts(kept, usize::from(count)) });
            r.set(window, rest);
        }
        sibling!(call_spread_miss, r, ip, view)
    }
}

/// The values a lane `Call` record passes: its window of the running function's registers.
///
/// # Safety
/// `r` is the registers of the activation `i` belongs to; the slice is valid until they move.
#[inline(always)]
unsafe fn call_window_in_lane<'a>(r: Registers, i: &LaneInstruction) -> &'a [Value] {
    unsafe { window_in_lane(r, ImmediateLayout::call_window_base(i.imm()), i) }
}

/// The `argument_count` values of the record `i` at register `window` of the running function.
///
/// # Safety
/// As `call_window_in_lane`.
#[inline(always)]
unsafe fn window_in_lane<'a>(r: Registers, window: u16, i: &LaneInstruction) -> &'a [Value] {
    unsafe { std::slice::from_raw_parts(r.0.add(usize::from(window)), usize::from(ImmediateLayout::argument_count(i.imm()))) }
}

/// The value of the lane `Call` record `i` of the native `callee`, of `kind`, with
/// `receiver` and `args`: the in-place cases every tier shares (`Vm::in_place_native_value`),
/// the lane's own, else `string_or_native_call_in_lane`. `None` leaves the call to the general path,
/// which raises an error the call stashed in `Vm::lane_raised`. Out of line: its calls take
/// stack temporaries, which a handler inlining them would hold on its common path.
#[inline(never)]
fn native_value_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // The Native argument carries the caller's function-kind proof. Each operation uses
    // its semantic fallback; the remaining natives try `string_or_native_call_in_lane`.
    match native {
        // The DataView Float64 lane, in every mode: one read of the view and one of its
        // buffer; a miss takes the native, which coerces and throws.
        Native::DataViewGetFloat64 | Native::DataViewSetFloat64 => {
            let value = if native == Native::DataViewGetFloat64 {
                vm.data_view_access_of(Native::DataViewGetFloat64, receiver, args)
            } else {
                vm.data_view_access_of(Native::DataViewSetFloat64, receiver, args)
            };
            if value.is_some() {
                if vm.execution_mode == quench_runtime::vm::ExecutionMode::Compiled {
                    vm.programs.note_js_jit_dataview_float64(native == Native::DataViewSetFloat64);
                }
                return value;
            }
        }
        Native::RegExpExec | Native::RegExpTest | Native::StringReplace | Native::StringMatch => {
            return regexp_call_in_lane(cx, ip, i, native, callee, receiver, args);
        }
        // A generator's resumption runs its body, and a Wasm export its function: guest code
        // that no in-place shortcut of `string_or_native_call_in_lane` makes.
        Native::GeneratorNext | Native::GeneratorReturn | Native::GeneratorThrow | Native::WasmExportedFunction => {
            return call_native_in_lane(cx, ip, native, callee, receiver, args);
        }
        // `with (object)` and its exit, in a frameless activation: its scope chain is the lane stack's.
        Native::WithEnter | Native::WithExit => {
            if let Some(value) = with_scopes::native_with_scope(cx, native, args) {
                return Some(value);
            }
        }
        // Neither a string method nor a pure native: the promise shortcut, else the native.
        Native::PromiseThen | Native::PromiseCatch | Native::PromiseResolve => {
            return promise_native_call_in_lane(cx, ip, native, callee, receiver, args);
        }
        _ => {}
    }
    string_or_native_call_in_lane(cx, ip, native, callee, receiver, args)
}

/// The value of a promise native call: the lane's shortcut where it makes one, else the native.
fn promise_native_call_in_lane(cx: *mut LaneContext, ip: Ip, native: Native, callee: Value, receiver: Value, args: &[Value]) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(value) = promise_native_in_lane(vm, code, native, callee, receiver, args) {
        note_allocation(vm);
        return Some(value);
    }
    call_native_in_lane(cx, ip, native, callee, receiver, args)
}

pub(crate) mod named_keys;
pub(crate) mod wasm_calls;
pub(crate) mod wasm_exports; use wasm_exports::LaneExportCall;
use wasm_calls::{known_call_handler, reference_call_handler};
use named_keys::named_key_element;
pub(crate) mod in_place_natives;
/// Whether a compiled loop may call `native` in place and go on (`in_place_natives::in_place_in_loop`).
pub(crate) const fn in_place_in_loop(native: Native) -> bool { in_place_natives::in_place_in_loop(native) }
pub(crate) mod root_bindings;
use root_bindings::frame_capture_at_root;
pub mod native_calls; pub(crate) mod regexp_calls; pub(crate) mod getters; pub(crate) mod generator_yields;
pub(crate) mod lane_roots;
pub(crate) mod lane_trace;
pub(crate) mod lane_stack_trace;
pub(crate) mod lane_frames; use lane_frames::{ActivationView, SidePayload};
pub(crate) mod frame_walk;
pub(crate) mod with_scopes;
pub mod scoped_names; pub(crate) mod window_owner; 
pub(crate) mod lane_eval;
use lane_roots::{LaneRoot, LaneStart};
pub(crate) mod async_requests;
pub(crate) mod generator_windows; use generator_windows::{generator_returned_in_lane, resume_generator_step_in_lane}; 
pub(crate) mod raised_windows;
pub(crate) mod array_steps;
pub(crate) mod name_probe; use name_probe::probe_handler; 
pub(crate) mod vm_steps;
pub(crate) mod family_helpers;
pub(crate) mod value_steps;
pub(crate) use vm_steps::{body_general_step, body_string_concat, decide_comparison, lane_allocate_step, lane_body_step_packed, vm_operation_step};
pub(crate) mod handler_edges;
pub(crate) use handler_edges::handler_edge;
use array_steps::*;
use native_calls::{math_field_call_handlers, native_call_handler, reflect_get_call_handler};
pub(crate) mod wasm_gc;

#[quench_macros::ext(DispatchFastVm4)]
impl Vm {
    /// A native call's value where its common case completes in place, without guest code
    /// or a raised error: a dense `push` or `pop`, `charCodeAt`, an ASCII `charAt`, a
    /// collection lookup, `Array.isArray`, `JSON.stringify` of plain data (a new string).
    /// `None` leaves the call to the native itself, whose own steps begin with the same cases.
    #[inline(always)]
    fn steady_native_value(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        // A new attempt clears any flag a previous attempt left for its general native.
        self.declined_fast = None;
        let first = args.first().copied();
        match native {
            Native::ArrayPush => self.array_push_dense(receiver, args),
            Native::ArrayPop => self.array_pop_dense(receiver),
            Native::StringCharCodeAt => self.string_char_code_at_core(receiver, first?),
            Native::StringCharAt => self.string_char_at_core(receiver, first?),
            Native::MapGet | Native::MapHas => {
                let found = self.pure_collection_lookup(native, receiver, first.unwrap_or(Value::UNDEFINED));
                self.fast_declined(native, found)
            }
            Native::SetHas | Native::WeakMapGet | Native::WeakMapHas
            | Native::WeakSetHas => self.pure_collection_lookup(native, receiver, first.unwrap_or(Value::UNDEFINED)),
            Native::MapSet if self.map_set(receiver, args) => Some(receiver),
            Native::ArrayIsArray => self.steady_is_array(first.unwrap_or(Value::UNDEFINED)).map(Value::boolean),
            Native::JsonStringify => {
                let text = self.json_stringify_steady(args);
                self.fast_declined(native, text)
            }
            Native::DateNow => Some(self.date_now()),
            _ => None,
        }
    }

}

/// Whether liveness proves the result register of the running function's call record `i`
/// at `ip` dead once the call completes (`liveness::register_dead` before the next PC), so
/// its value is never read: what the call writes there is unobservable.
#[cold]
fn call_result_dead_in_lane(cx: *mut LaneContext, ip: Ip, i: &LaneInstruction) -> bool {
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // A leaf makes no call, so the innermost activation, or else the frame, runs `i`.
    let function = running_function_in_lane(cx);
    // SAFETY: `ip` is a record of the running view, which starts at the context's base.
    let pc = unsafe { ip.offset_from((*cx).base) } as usize;
    code.register_root_masks(function)
        .and_then(|masks| masks.get(pc + 1))
        .is_some_and(|after| quench_runtime::compile::liveness::register_dead(after, usize::from(i.a & quench_runtime::bytecode::REGISTER_MASK)))
}

script_handler! {
    /// A `Call` site whose callee was an array's callback method (`native_call_handler`,
    /// `Native::is_array_callback`), which no in-place shortcut of `native_value_in_lane` makes:
    /// the native's call out of the lane (`call_native_out_in_lane`), whose callback calls run in
    /// this lane (`Vm::native_callback_in_lane`). Any other callee takes `native_call_miss`.
    /// Where `DISCARDS`, nothing reads the call's result, so a `forEach` of one element is its
    /// callback's plain call (`call_for_each_once_in_lane`).
    #[inline(never)]
    fn call_array_callback_miss<DISCARDS: bool>(cx, r, ip, i, view) {
        let callee = r.get(i.b);
        let native = match (unsafe { vm(cx) }).function_record(callee) {
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) if native.is_array_callback() => *native,
            _ => sibling!(native_call_miss, r, ip, Carried::script_in(cx, view)),
        };
        // SAFETY: the call's window is registers of the running function, which the native
        // call copies before anything can move them, or reads in place where nothing does
        // (`call_native_out_in_lane`).
        let args = unsafe { call_window_in_lane(r, i) };
        // One element, tested inline: a longer array takes the iteration at no further cost.
        if DISCARDS
            && native == Native::ArrayForEach
            && array_object_in_lane(unsafe { vm(cx) }, r.get(i.c)).is_some_and(|object| object.elements().len() == 1)
            && let Some((registers, start)) = call_for_each_once_in_lane(cx, ip, r.get(i.c), args)
        {
            next!(cx, registers, start, Carried::script_in(cx, view))
        }
        if let Some(resumed) = begin_array_iteration_in_lane(cx, ip, native, r.get(i.c), args) {
            resume!(cx, resumed)
        }
        let value = call_native_out_in_lane(cx, ip, native, callee, r.get(i.c), args);
        resume!(cx, native_complete_in_lane(cx, ip, i, value))
    }
}

/// Whether `callee` is a function whose behavior is the native `native`'s.
#[inline(always)]
fn callee_is_native(vm: &Vm, callee: Value, native: Native) -> bool {
    matches!(vm.function_record(callee), Some(FunctionRecord { kind: FunctionKind::Native(found), .. }) if *found == native)
}

script_handler! {
    /// A `Call` site whose callee was a Math function of Numbers (`native_call_handler`): the
    /// call of Number arguments in place (`number::math_inline_call`). Any other callee or
    /// argument takes `native_call_miss`.
    fn call_math_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let value = match vm.function_record(r.get(i.b)) {
            // SAFETY: the call's window is registers of the running function.
            Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) => {
                quench_runtime::vm::number::math_inline_call(*native, unsafe { call_window_in_lane(r, i) })
            }
            _ => None,
        };
        let Some(value) = value else {
            sibling!(native_call_miss, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// The lane `Call` record `i` completed with `value`: in its result register, or returned
/// from the caller for a call that returns its result; the general path for `None`.
#[inline(always)]
fn native_complete_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    value: Option<Value>,
) -> Resumed {
    let Some(value) = value else {
        return Resumed::Exit(exit(ip));
    };
    // The call may have made the lane's activations frames, so the running function's
    // registers and caller are read again.
    if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        let Some(resumed) = complete_activation_in_lane(cx, ip, value) else {
            // A frame of a script run retires in place, as its `Return` would.
            if unsafe { !(*cx).inline_calls && (*cx).script_calls } {
                return Resumed::Exit(return_script_frame_in_lane(cx, ip, value, true));
            }
            return Resumed::Exit(frame_return(unsafe { vm(cx) }, ip, value));
        };
        return resumed;
    }
    let r = unsafe { (*cx).registers };
    r.set(i.a, value);
    Resumed::At(r, following(ip), Carried::script(cx))
}

/// `RegExp.prototype.exec` or `test`, or `String.prototype.replace` or `match` of a RegExp, made in
/// place when it runs no guest code and raises nothing, so it observes no frame
/// (`Vm::steady_regexp_call`, `Vm::steady_string_replace`); any other such call goes on as
/// `string_or_native_call_in_lane` makes it.
#[inline(never)]
fn regexp_call_in_lane(
    cx: *mut LaneContext,
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
    let result_used = ImmediateLayout::call_result_used_in_lane(i.imm());
    let steady = match (native, args) {
        (Native::StringReplace, _) => vm.steady_string_replace(code, native, receiver, args, result_used),
        (Native::StringMatch, [search, ..]) => vm.steady_string_match(code, receiver, *search, result_used),
        (_, [subject, ..]) => vm.steady_regexp_call(code, native, receiver, *subject, result_used),
        _ => None,
    };
    let Some(result) = steady else {
        return string_or_native_call_in_lane(cx, ip, native, callee, receiver, args);
    };
    let result = vm.regexp_search_outcome(result);
    // The call's allocations may make a collection due, which the lane's safepoints take.
    note_allocation(vm);
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// A lane native call that `native_call_miss` does not make inline: a pure string method or
/// a pure native in place, else the native through `call_native_in_lane`. Inline into its two callers: a native
/// call otherwise pays this layer's own frame.
#[inline(always)]
fn string_or_native_call_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(value) = vm.in_place_native_value_inline(code, native, callee, receiver, args) {
        // An in-place case may allocate; a due collection runs at the lane's next safepoint.
        note_allocation(vm);
        return Some(value);
    }
    call_native_in_lane(cx, ip, native, callee, receiver, args)
}

/// A lane call of a promise native whose steps provably observe nothing, answered in place
/// so the lane's activations need not become frames (`call_native_in_lane`): `then` and
/// `catch` of a receiver with intrinsic species (`Vm::promise_then_in_lane`) and the intrinsic
/// `Promise.resolve` (`Vm::promise_resolve_in_lane`). A resolving function (one with an
/// environment), any other shape, or a lifecycle-observing host takes the general call.
#[inline(never)]
fn promise_native_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    if !matches!(native, Native::PromiseThen | Native::PromiseCatch | Native::PromiseResolve)
        || !vm.function_record(callee).is_some_and(|function| function.env.is_null())
    {
        return None;
    }
    match native {
        Native::PromiseThen => vm.promise_then_in_lane(
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
                vm.function_record(then),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::PromiseThen), .. })
            ) {
                return None;
            }
            vm.promise_then_in_lane(
                code,
                receiver,
                Value::UNDEFINED,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )
        }
        Native::PromiseResolve => vm.promise_resolve_in_lane(
            code,
            receiver,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        ),
        _ => unreachable!("only promise shortcut natives reach here"),
    }
}


/// A lane `Call` of a native that `Native::runs_from_lane` admits, made as the general path
/// makes it from the running frame: with the frame's resume PC published and the arguments
/// read from its registers, which the frame keeps live at the call. Only a frame's own lane run
/// calls out, so the frame stack the native and its nested calls observe is the general
/// path's. `None` exits to the general path, which raises the error the call stashed in
/// `Vm::lane_raised` or makes a call this does not admit.
#[inline(never)]
fn call_native_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    if !native.runs_from_lane() {
        return None;
    }
    call_native_out_in_lane(cx, ip, native, callee, receiver, args)
}

/// `call_native_in_lane` past its admission: the native's call out of the lane,
/// beside its activations where the native admits it, else from its frame
/// (`call_from_frame_in_lane`), as `call_array_callback_miss` calls an array's callback method,
/// which no shortcut answers.
#[inline(always)]
fn call_native_out_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } {
        return None;
    }
    // Arguments in the call's window are held by the frame parked at the call, so the native
    // roots only its receiver; any others (an `apply`'s elements) are rooted with it.
    let in_window = arguments_in_window(cx, args);
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    // Without activations the window lies in the lane's frame's register file, which nothing
    // moves or writes while the frame is parked at the call: the native reads it in place.
    if in_window && !vm.lane_stack.has_frames() && registers_are_frames(vm, cx) {
        return call_from_frame_in_lane(cx, ip, |vm, code| {
            vm.call_native_from_lane_arguments_held(code, native, receiver, args, callee)
        });
    }
    // With activations the innermost one's frame holds the window once they become frames.
    let held = in_window && vm.lane_stack.has_frames();
    // The values are copied before activations become frames, which moves the registers
    // they may lie in; the frames then hold what the call passes.
    let arguments = quench_runtime::vm::call_arguments::CallArguments::from_slice(args);
    let call = |vm: &mut Vm, code: &ResidualProgram| {
        if held {
            vm.call_native_from_lane_arguments_held(code, native, receiver, arguments.as_slice(), callee)
        } else {
            vm.call_native_from_lane(code, native, receiver, arguments.as_slice(), callee)
        }
    };
    // A native that observes frames only through the points `materialize_deferred_lane`
    // guards runs beside the activations; the first such point makes them frames, and a
    // collection takes that point before it scans (`Vm::collect_inner`).
    if vm.lane_stack.has_frames()
        && (vm.runs_beside_activations(native, callee) || vm.steps_beside_activations(native, receiver))
    {
        return call_deferred_native_in_lane(cx, call_pc, call);
    }
    #[cfg(feature = "profile-lane-exits")]
    if vm.lane_stack.has_frames() {
        quench_runtime::vm::lane_exits::record_detail(match native {
            Native::HostFunction => format!("materializes for host {}", vm.host_function_name(callee).unwrap_or_default()),
            native => format!("materializes for native {native:?}"),
        });
    }
    call_from_frame_in_lane(cx, ip, call)
}

/// Runs a native call beside the lane's activations as a deferred call (`DeferredLaneCall`),
/// whose frames the first observer makes; an error is raised for the general path as the
/// call's own frames would raise it.
#[cold]
#[inline(never)]
fn call_deferred_native_in_lane(
    cx: *mut LaneContext,
    call_pc: usize,
    call: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<Value, JsError>,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
    let result = call(vm, unsafe { &*(*cx).code });
    note_allocation(vm);
    vm.lane_stack.refresh_activation_limit();
    let unobserved = vm.lane_deferred.take().is_some();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            // The general path raises it from the frames its activations become.
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
                vm.materialize_deferred_lane();
            }
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// Whether `args` lie in the running function's registers, as its call record's window does:
/// the frame parked at the call (the lane's own, or the one the innermost activation becomes,
/// which a collection makes first) holds them live at the call's pc, where the collector scans
/// a parked frame (`Frame::source_instruction_pc`). Values a call made of others (an `apply`'s
/// array elements, a bound prefix) lie elsewhere. Only addresses are compared.
#[inline(always)]
fn arguments_in_window(cx: *mut LaneContext, args: &[Value]) -> bool {
    if args.is_empty() {
        return true;
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let count = usize::from(code.function(running_function_in_lane(cx) as usize).registers);
    let registers = unsafe { (*cx).registers.0 }.cast_const();
    let first = args.as_ptr();
    registers <= first && first.wrapping_add(args.len()) <= registers.wrapping_add(count)
}

/// Whether the lane's registers are its frame's register file, which stays where it is while
/// the frame is parked at a call.
#[inline(always)]
fn registers_are_frames(vm: &Vm, cx: *mut LaneContext) -> bool {
    // SAFETY: the lane runs, so its context lives.
    unsafe { (*cx).has_frame() && vm.frames[(*cx).frame()].registers.as_ptr() == (*cx).registers.0.cast_const() }
}

/// A native call at the lane `Call` record `ip` made from the running frame: the callee
/// observes the frame stack, so lane activations become frames first, as leaving the lane
/// would make them, and `call` runs from the innermost one. `None` leaves the error it raised
/// in `Vm::lane_raised`.
#[inline(always)]
fn call_from_frame_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    call: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<Value, JsError>,
) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    vm.materialize_lane_activations(cx);
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // active view, which is the running frame's.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame()) };
    vm.frames[frame].pc = call_pc + 1;
    vm.frames[frame].pc_is_resume = true;
    // The native branch of `Vm::call_value_with_target_inner`, whose callee this is; its
    // calls of JavaScript run in this lane while it runs from this frame.
    let site = LaneNativeCall { cx: cx.cast(), call: ip, frames: vm.frames.len() };
    let outer = vm.lane_native.replace(site);
    let result = call(vm, code);
    vm.lane_native = outer;
    // The native's allocations may make a collection due, which the lane's safepoints take.
    note_allocation(vm);
    // A host edge can change the thread's guest-call allowance. Rebind the
    // capacity certificate here, before any lane/native entry consumes it.
    vm.lane_stack.refresh_activation_limit();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// A lane `new` of a native that `Native::constructs_from_lane` admits, made as the general
/// path makes it (`Vm::construct_value_with_new_target`) from the running frame, after
/// activations become frames, as `call_native_in_lane` makes a call. `None` exits to the general
/// path, which raises the error the construction stashed in `Vm::lane_raised` or makes one
/// this does not admit.
#[inline(never)]
fn construct_native_in_lane(
    cx: *mut LaneContext,
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
    let arguments = quench_runtime::vm::call_arguments::CallArguments::from_slice(args);
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
    // running function's view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    // A native's construction runs beside the activations, as a native call beside them
    // runs (`call_native_in_lane`): the first point that observes frames makes them frames.
    let beside = vm.lane_stack.has_frames() && vm.constructs_beside_activations(callee);
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
    } else {
        vm.materialize_lane_activations(cx);
        let frame = unsafe { (*cx).frame() };
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
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
                vm.materialize_deferred_lane();
            }
            vm.lane_completion.raise(error);
            None
        }
    }
}

/// The native parent a `new` of `callee` constructs in its place: `callee`'s own parent
/// (`[[GetPrototypeOf]]`) when its constructor only constructs it
/// (`Function::super_forwarding`) and that parent is a native the lane constructs other
/// than an error constructor, with what the constructor passes on.
#[inline(never)]
fn super_forwarded_parent_in_lane(vm: &Vm, callee: Value) -> Option<(Value, quench_runtime::bytecode::SuperForwarded)> {
    // A plan with a recorded user parent (`super_construction`) is no native forwarding: its
    // construction goes straight to that plan, not decoding the constructor's body each time.
    if holding_construct_plan_index(vm, callee).is_some_and(|index| vm.lane_stack.construct_plans[index].super_parent != Value::UNDEFINED) {
        return None;
    }
    let Some(FunctionRecord { kind: FunctionKind::User(program, function), .. }) = vm.function_record(callee) else {
        return None;
    };
    let code = vm.programs.residual(*program)?;
    let body = code.function_get(*function as usize)?;
    if !body.derived_constructor {
        return None;
    }
    // The parent first: a class whose parent is no native the lane constructs, as every
    // user class hierarchy's, declines here without decoding its constructor's body
    // (`Function::super_forwarding`, ~900 Ir), which every `new` of it would otherwise pay.
    let parent = vm.object_data(callee)?.proto();
    match vm.function_record(parent) {
        // An error's stack starts past its constructors' frames, which this construction
        // would not make.
        Some(FunctionRecord { kind: FunctionKind::Native(native), .. })
            if (native.constructs_from_lane() && !native.is_error_constructor()) || *native == Native::Array =>
        {
            Some((parent, body.super_forwarding(&code.atoms, &code.constants)?))
        }
        _ => None,
    }
}

/// `new callee(...)` of a class whose constructor only constructs its native `parent`
/// (`super_forwarded_parent_in_lane`): the parent's construction with what the constructor
/// passes on (every value, or its parameters with missing ones `undefined`) and `callee`
/// as `new.target`, as the body's `super(...)` makes it; its receiver is the construction's
/// value.
#[inline(never)]
fn construct_forwarded_parent_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    callee: Value,
    (parent, forwarded): (Value, quench_runtime::bytecode::SuperForwarded),
) -> Resumed {
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    let count = match forwarded {
        quench_runtime::bytecode::SuperForwarded::Passed => argc,
        quench_runtime::bytecode::SuperForwarded::Parameters(count) => usize::from(count),
    };
    let passed = (0..count)
        .map(|index| if index < argc { r.get(i.c + index as u16) } else { Value::UNDEFINED })
        .collect::<smallvec::SmallVec<[Value; GATHERED_INLINE_VALUES]>>();
    let vm = unsafe { vm(cx) };
    // An array subclass's `super(...)`: the intrinsic's array with the class's `prototype`,
    // made in place and named as the general path names it (`capture_constructed_value`).
    if vm.construct_target.is_none()
        && matches!(vm.function_record(parent), Some(FunctionRecord { kind: FunctionKind::Native(Native::Array), .. }))
        && let Some(array) = vm.array_construction_by(callee, &passed)
    {
        vm.capture_constructed_value(array, parent, false);
        note_allocation(vm);
        return native_complete_in_lane(cx, ip, i, Some(array));
    }
    let value = construct_native_in_lane(cx, ip, parent, callee, &passed);
    native_complete_in_lane(cx, ip, i, value)
}

script_handler! {
    /// `new` of a frameless target that `Vm::inline_construct_target` would push: the
    /// receiver it allocates, then the activation `script_call_miss` makes, whose
    /// completion substitutes the receiver for a non-object result. Any other
    /// construction takes the general path.
    fn script_construct_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let callee = r.get(i.b);
        // Under a pending construction no cached target admits the construction
        // (`LaneStack::set_refused`), so the resolve-and-retry below would
        // never find one: it leaves the lane, as a refused call does.
        if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.lane_stack.refuses()) {
            return exit(ip);
        }
        // A construction inside a leaf is the activation its call would have pushed
        // (`materialize_running_leaf_in_lane`), which completes the caller as any construction
        // does; a leaf completes a planned returned one itself (`return_constructed_in_lane`).
        materialize_running_leaf_in_lane(cx);
        // The cached target in place, as a call reads it (`Vm::script_call_entry`); an
        // uncached callee resolves (`script_target_in_lane`), after which the construction
        // retries.
        let entry = if constructing_in_lane(vm, callee) {
            None
        } else {
            vm.script_call_entry(callee).or_else(|| vm.cross_call_entry(callee))
        };
        let Some(entry) = entry else {
            if script_target_in_lane(cx, callee).is_some_and(|target| target.constructs()) {
                sibling!(script_construct_miss, r, ip, Carried::script_in(cx, view))
            }
            sibling!(native_construct_miss, r, ip, Carried::script_in(cx, view))
        };
        // SAFETY: as `Vm::script_call_entry`; the planned construction below resolves no
        // target, and any other path copies the target first.
        let target = unsafe { &*entry };
        // A class constructor's target constructs with entry `Frame`, which its calls take.
        if !target.constructs() {
            sibling!(native_construct_miss, r, ip, Carried::script_in(cx, view))
        }
        // A planned construction, the common `new C(…)` or `return new C(…)`: the receiver
        // is the finished object, and the body completed with `undefined`.
        if target.initializes()
            && finishes_construction_in_lane(cx, i)
            && let Some(this) = planned_construction_in_lane(vm, callee, r, i)
        {
            // Allocating the receiver can make a collection due.
            note_allocation(vm);
            // Later runs of the site try the plan first. The site notes its callee, which a body
            // reads to keep the receiver as its field values (`scalar_construct_plan`).
            vm.note_site_callee(unsafe { &(*ip).call_link }, crate::compiled::CallTarget {
                callee: crate::compiled::CalleeBinding::Identity(callee), function: target.function });
            if i.a & quench_runtime::bytecode::RETURN_REGISTER == 0 {
                requicken(ip, protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::PLANNED_CONSTRUCT as u16 }>());
                r.set(i.a, this);
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
            requicken(ip, protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::PLANNED_CONSTRUCT_RETURNED as u16 }>());
            if let Some(resumed) = complete_activation_in_lane(cx, ip, this) {
                resume!(cx, resumed)
            }
            return frame_return(vm, ip, this);
        }
        let target = *target;
        if target.forwards() {
            // The site constructs a forwarding target: later runs enter the keeping
            // construction directly, which moves the record back for any other target.
            requicken(ip, protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::SCRIPT_CONSTRUCT_KEEPING as u16 }>());
            sibling!(script_construct_keeping_miss, r, ip, Carried::script_in(cx, view))
        }
        let built = if target.initializes() {
            match planned_construction_in_lane(vm, callee, r, i) {
                Some(this) => Some((this, true)),
                // SAFETY: the context's program outlives the lane run.
                None => initialize_fields_in_lane(vm, unsafe { &*(*cx).code }, callee, callee, target.function, construct_argument_in_lane(r, i)),
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
            if i.a & quench_runtime::bytecode::RETURN_REGISTER == 0 {
                r.set(i.a, this);
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
            if let Some(resumed) = complete_activation_in_lane(cx, ip, this) {
                resume!(cx, resumed)
            }
            // The frame's own construct: the general loop returns the receiver from it, unless
            // proper tail calls send it to the activation below, which leaves for that loop.
            if returning_call_admitted(cx) {
                return frame_return(vm, ip, this);
            }
        }
        let activated = if target.entry == CallEntry::Environment {
            // SAFETY: the construction's arguments are a window of the running function's registers.
            activate_environment_in_lane(cx, r, ip, &target, unsafe { window_in_lane(r, i.c, i) }, (this, true))
        } else {
            activate_in_lane(cx, r, ip, &target, i.c, this, true)
        };
        match activated {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(next) => next,
        }
    }
}

script_handler! {
    /// `script_construct_miss` at a site into a register whose construction was planned
    /// (`planned_construction_in_lane`), which `script_construct_miss` moves it to: the plan of
    /// the callee alone, without resolving its call target. A plan exists only for a callee
    /// whose target constructs by its initializer stores, which its function fixes, and while
    /// it holds the finished object is what the activation would make. Any other construction
    /// moves the record back to `script_construct_miss`.
    fn planned_construct_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some()) {
            return exit(ip);
        }
        let Some(this) = planned_inline_quick_in_lane(vm, r.get(i.b), r, i) else {
            sibling!(planned_construct_full_in_lane, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, this);
        // Allocating the receiver can make a collection due.
        if vm.collection_due() {
            sibling!(construct_allocation_due_in_lane, r, ip, Carried::script_in(cx, view))
        }
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `planned_construct_miss` past its quick case (`planned_inline_quick_in_lane`): any plan, and
    /// the slow allocations, out of line so the quick case calls nothing.
    #[inline(never)]
    fn planned_construct_full_in_lane<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(this) = planned_construction_in_lane(vm, r.get(i.b), r, i) else {
            requicken(ip, protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::SCRIPT_CONSTRUCT as u16 }>());
            sibling!(script_construct_miss, r, ip, Carried::script_in(cx, view))
        };
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        r.set(i.a, this);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// A construction whose receiver is already in its register and whose allocation made a
    /// collection due: notes it (`note_allocation`) and goes on, out of line.
    #[inline(never)]
    fn construct_allocation_due_in_lane<>(cx, r, ip, i, view) {
        note_allocation(unsafe { vm(cx) });
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

script_handler! {
    /// `planned_construct_miss` at a site whose construction is returned (`RETURN_REGISTER`),
    /// which `script_construct_miss` moves it to: the planned receiver completes the
    /// activation that returns it, or the frame's own returning construct where
    /// `finishes_construction_in_lane` admits it. Any other construction moves the record back
    /// to `script_construct_miss`.
    fn planned_construct_returned_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some()) {
            return exit(ip);
        }
        let planned = finishes_construction_in_lane(cx, i)
            .then(|| planned_construction_in_lane(vm, r.get(i.b), r, i))
            .flatten();
        let Some(this) = planned else {
            requicken(ip, protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::SCRIPT_CONSTRUCT as u16 }>());
            sibling!(script_construct_miss, r, ip, Carried::script_in(cx, view))
        };
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        resume!(cx, return_constructed_in_lane(cx, ip, this))
    }
}

script_handler! {
    /// `script_call_miss` of a target that keeps its passed values
    /// (`ScriptCallTarget::forwards`): the keeping activation (`call_keeping_activation_in_lane`).
    /// Out of line, so ordinary calls keep their code.
    #[inline(never)]
    fn script_call_keeping_miss<>(cx, r, ip, i, view) {
        match call_keeping_activation_in_lane(cx, r, ip, i) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(Some(next)) => next,
            // A site this form serves since `script_call_miss` saw a keeping target calls
            // the full form once its callee keeps nothing.
            Err(None) => {
                requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>());
                sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
            }
        }
    }
}

/// `script_call_keeping_miss`'s activation: the callee's registers and first record, the
/// record to continue at, or `None` for a callee whose target keeps nothing, which the full
/// form calls. Inline in its one handler, which is itself out of line.
#[inline(always)]
fn call_keeping_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<(Registers, Ip), Option<Ip>> {
    if !unsafe { (*cx).script_calls } {
        return Err(Some(exit(ip)));
    }
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    // The site's linked entry, as `script_call_environment_miss` reads it, read in place: only
    // an activation that allocates (a rest array) copies it first; any other callee takes the
    // general lookup.
    let linked = if constructing_in_lane(vm, callee) {
        None
    } else {
        // SAFETY: as `Vm::script_call_entry`; nothing before the activation resolves a target.
        vm.linked_script_call_entry(callee, &i.call_link)
            .map(|target| unsafe { &*target })
            .filter(|target| admits_activation_in_lane(target))
    };
    let resolved;
    let target: &ScriptCallTarget = match linked {
        Some(target) => target,
        None => {
            let Some(target) = script_target_in_lane(cx, callee) else {
                return Err(None);
            };
            resolved = target;
            &resolved
        }
    };
    if !target.forwards() {
        return Err(None);
    }
    #[cfg(feature = "profile-lane-exits")]
    profile_script_call_target(cx, ip, callee, &target);
    let receiver = r.get(i.c);
    let Some(this) = call_this_in_lane(vm, &target, receiver) else {
        return Err(Some(exit(ip)));
    };
    let window = ImmediateLayout::call_window_base(i.imm());
    if target.entry == CallEntry::Gathers && target.forms & TARGET_GATHERS_KEEPING == 0 {
        // SAFETY: the call's window of `r` holds its `argc` values.
        let passed = unsafe { r.0.add(usize::from(window)).cast_const() };
        return activate_gathering_in_lane(cx, r, ip, &target, passed, ImmediateLayout::argument_count(i.imm()), this).map_err(Some);
    }
    if target.entry == CallEntry::Keeps {
        return activate_keeping_window_in_lane::<false>(cx, r, ip, &target, window, this).map_err(Some);
    }
    let arguments = ActivationArguments::Window(window);
    // This activation may allocate its rest array before it reads the target: a copy.
    activate_keeping_in_lane::<false>(cx, r, ip, &{ *target }, arguments, this, false, None).map_err(Some)
}

/// The keeping activation of a target of the running program that keeps the values it was
/// passed (`CallEntry::Keeps`), called with its window at `arguments` of `r`, made as a plain
/// call's push is (`fill_window_in_lane`, `enter_window_in_lane`) with the values kept after its
/// registers, as `activate_keeping_in_lane` keeps them, and the activation marked keeping.
#[inline(always)]
fn activate_keeping_window_in_lane<const GENERAL: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if (returns && !returning_call_admitted(cx)) || vm.with_scope_entered {
        return Err(exit(ip));
    }
    let argc = ImmediateLayout::argument_count(i.imm());
    let count = usize::from(target.registers);
    let window_size = quench_runtime::staged::call_keeping::keeping_window_words(target.registers, usize::from(argc), true);
    let room = fill_room::<ACTIVATION_FILL_BLOCK>(window_size).max(usize::from(target.room));
    if !quench_runtime::staged::call_keeping::concrete_admits_keeping(
        vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), room,
    ) {
        return Err(script_call_frame_in_lane(ip, room));
    }
    // SAFETY: the call's window of `r` holds its `argc` values; capacity holds the window
    // and the kept values after it, each written before the length covers it. The registers
    // are the admitted fill's (`fill_window_in_lane`), the kept values the keeping row's.
    let callee_registers = unsafe {
        let passed = r.0.add(usize::from(arguments)).cast_const();
        let registers = fill_window_in_lane(vm, target, passed, argc.min(target.params));
        quench_runtime::staged::call_keeping::keep_passed(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()), registers.0.add(count).expose_provenance() as u64,
            passed.expose_provenance() as u64, argc,
        );
        vm.lane_stack.values.set_top(registers.0.add(window_size + lane_values::CONTEXT));
        registers
    };
    Ok(enter_window_in_lane::<GENERAL, false>(cx, r, ip, target, callee_registers, argc, this, false, None, Some(target.registers)))
}

/// The plain activation (`push_passed_in_lane`) of a rest body of the running program that keeps
/// nothing (`TARGET_GATHERS_KEEPING` clear), passed the `argc` values at `passed` (its window
/// of `r`, or the elements of the array a spread call passes): the fixed parameters copied as
/// any call's are, and the rest parameter's register the array of the values past them, read
/// in place, or `undefined` for a rest view that keeps the values after its registers.
#[inline(always)]
fn activate_gathering_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if (returns && !returning_call_admitted(cx)) || vm.with_scope_entered {
        return Err(exit(ip));
    }
    let view = target.forms & TARGET_REST_VIEW != 0;
    let room = usize::from(target.room) + if view { usize::from(argc) } else { 0 };
    if !quench_runtime::staged::call_keeping::concrete_admits_keeping(
        vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), room,
    ) {
        return Err(script_call_frame_in_lane(ip, room));
    }
    let fixed = target.params - 1;
    if view {
        // A rest view keeps every value passed after its registers, as a keeping activation
        // does, and its rest register holds `undefined` (`LaneFrame::rest_view`).
        let callee_registers = fill_window_in_lane(vm, target, passed, argc.min(fixed));
        callee_registers.set(fixed, Value::UNDEFINED);
        let count = usize::from(target.registers);
        // SAFETY: the room checked above holds the kept values past the window.
        unsafe {
            quench_runtime::staged::call_keeping::keep_passed(
                &mut quench_runtime::staged::Concrete::new(std::ptr::null()), callee_registers.0.add(count).expose_provenance() as u64,
                passed.expose_provenance() as u64, argc,
            );
            vm.lane_stack.values.set_top(callee_registers.0.add(count + usize::from(argc) + lane_values::CONTEXT));
        }
        return Ok(enter_window_in_lane::<false, false>(cx, r, ip, target, callee_registers, argc, this, false, None, Some(target.registers)));
    }
    // The entry, which callers read in place, is copied before the rest array allocates.
    let target = &{ *target };
    let rest = vm.rest_parameter_array(unsafe {
        std::slice::from_raw_parts(passed.add(usize::from(fixed.min(argc))), usize::from(argc.saturating_sub(fixed)))
    });
    let callee_registers = fill_window_in_lane(vm, target, passed, argc.min(fixed));
    callee_registers.set(fixed, rest);
    Ok(enter_window_in_lane::<false, false>(cx, r, ip, target, callee_registers, argc.min(target.params), this, false, None, None))
}

script_handler! {
    /// `script_construct_miss` of a target that keeps its passed values
    /// (`ScriptCallTarget::forwards`), which `script_construct_miss` moves a site to: the
    /// keeping activation (`construct_keeping_activation_in_lane`). Any other construction moves
    /// the record back to `script_construct_miss`. Out of line, so ordinary constructions
    /// keep their code.
    #[inline(never)]
    fn script_construct_keeping_miss<>(cx, r, ip, i, view) {
        // As `script_construct_miss`: a leaf's construction materializes the leaf's activation
        // before anything completes, which a site reaches directly once it is keeping.
        materialize_running_leaf_in_lane(cx);
        let finished = match construct_planned_fields_in_lane(cx, r, i) {
            Some(this) => Ok(KeepingConstruction::Finished(this)),
            None => construct_keeping_activation_in_lane(cx, r, ip, i),
        };
        match finished {
            Ok(KeepingConstruction::Activated(registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Ok(KeepingConstruction::Finished(this)) => {
                if i.a & quench_runtime::bytecode::RETURN_REGISTER == 0 {
                    r.set(i.a, this);
                    next!(cx, r, following(ip), Carried::script_in(cx, view))
                }
                if let Some(resumed) = complete_activation_in_lane(cx, ip, this) {
                    resume!(cx, resumed)
                }
                // The frame's own construct, which `finishes_construction_in_lane` admitted: the
                // general loop returns the receiver from it.
                frame_return(unsafe { vm(cx) }, ip, this)
            }
            Err(Some(next)) => next,
            Err(None) => {
                requicken(ip, protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::SCRIPT_CONSTRUCT as u16 }>());
                sibling!(script_construct_miss, r, ip, Carried::script_in(cx, view))
            }
        }
    }
}

/// What `construct_keeping_activation_in_lane` made of a construction.
enum KeepingConstruction {
    /// The callee's registers and first record.
    Activated(Registers, Ip),
    /// The construction's result, whose body ran without an activation: its site completes
    /// with it.
    Finished(Value),
}

/// Whether a construction at `i` may complete in the lane with a receiver its body finished
/// without an activation: into a register, into the activation that returns it, or from the
/// frame's own returning construct where `returning_call_admitted` admits it, as
/// `lane_script_construct` completes one.
#[inline(always)]
fn finishes_construction_in_lane(cx: *mut LaneContext, i: &LaneInstruction) -> bool {
    i.a & quench_runtime::bytecode::RETURN_REGISTER == 0
        || unsafe { vm(cx) }.lane_stack.has_frames()
        || returning_call_admitted(cx)
}

/// The receiver a forwarding constructor's holding plan finishes with its recorded fields
/// (`forwarded_fields_in_lane`), the first case `construct_keeping_activation_in_lane` tries, in a
/// function of its own: inlined there it ran inside that function's frame (six saved
/// registers and 1.3 KB of stack for the activation paths). `None` for any other case, which
/// that function then decides from the start.
#[inline(never)]
fn construct_planned_fields_in_lane(cx: *mut LaneContext, r: Registers, i: &LaneInstruction) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some())
        || !finishes_construction_in_lane(cx, i)
    {
        return None;
    }
    let this = forwarded_fields_in_lane(vm, r.get(i.b), construct_argument_in_lane(r, i))?;
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    Some(this)
}

/// `script_construct_keeping_miss`'s activation, under `script_construct_miss`'s admission:
/// the callee's registers and first record, or the receiver its body finished; the record
/// to continue at, or `None` for a construction of any target but a forwarding one. Out of
/// line: the call target it passes on lives in its own frame, not the handler's.
#[inline(never)]
fn construct_keeping_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
) -> Result<KeepingConstruction, Option<Ip>> {
    let vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    if !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some()) {
        return Err(Some(exit(ip)));
    }
    let finishes = finishes_construction_in_lane(cx, i);
    // A plan holding for `callee` fixes its function, so the plan alone answers before its
    // target is resolved.
    if finishes && let Some(this) = forwarded_fields_in_lane(vm, callee, construct_argument_in_lane(r, i)) {
        // Allocating the receiver can make a collection due.
        note_allocation(vm);
        return Ok(KeepingConstruction::Finished(this));
    }
    // Matched rather than filtered: the combinators moved the target through copies.
    let Some(target) = script_target_in_lane(cx, callee) else {
        return Err(None);
    };
    if !(target.constructs() && target.forwards()) {
        return Err(None);
    }
    // One look at the plan serves the receiver and the forwarding: a plan the receiver's
    // allocation leaves records no resolution yet.
    let plan = holding_construct_plan(vm, callee);
    let Some(this) = planned_receiver_in_lane(vm, callee, plan.as_ref()) else {
        return Err(Some(exit(ip)));
    };
    note_allocation(vm);
    let arguments = ActivationArguments::Window(i.c);
    if let Some(&method) = vm.lane_stack.forwarded_methods.get(target.forwarded_method as usize)
        && let Some(forwarded) = match resolved_forwarding_in_lane(cx, callee, plan.as_ref()) {
            Some(forwarded) => Some(forwarded),
            None => forwarded_construction_in_lane(cx, target, method, this),
        }
    {
        // A forwarded method that only initializes `this` from its parameters, which the
        // forwarding passes the construction's arguments as: its adds alone, with neither
        // activation, where the construction completes in the lane (`finishes_construction_in_lane`).
        // Its body completes with `undefined`, as the forwarding body then does, so
        // [[Construct]] yields `this`.
        if forwarded.initializes() && finishes {
            // SAFETY: the context's program outlives the lane run, and the forwarded target is
            // one of its functions (`script_target_in_lane`).
            let code = unsafe { &*(*cx).code };
            let argument = construct_argument_in_lane(r, i);
            if let Some(fields) = construct_plan::finish_forwarded_in_lane(vm, code, forwarded.env, this, forwarded.function, argument) {
                if let Some(fields) = fields {
                    record_forwarded_fields_in_lane(vm, callee, (forwarded.program(), forwarded.function), this, &fields);
                }
                return Ok(KeepingConstruction::Finished(this));
            }
        }
        note_construct_site(vm, i, callee, &forwarded);
        let forwarder = SkippedForwarder {
            function: target.function,
            callee,
            env: target.env,
            method,
        };
        return activate_keeping_in_lane::<false>(cx, r, ip, &forwarded, arguments, this, false, Some(forwarder))
            .map(|(registers, base)| KeepingConstruction::Activated(registers, base))
            .map_err(Some);
    }
    activate_keeping_in_lane::<false>(cx, r, ip, &target, arguments, this, true, None)
        .map(|(registers, base)| KeepingConstruction::Activated(registers, base))
        .map_err(Some)
}

/// The finished receiver of a forwarding constructor's construction under `callee`'s
/// holding plan, when it records the fields its forwarded initializer stores
/// (`record_forwarded_fields_in_lane`) and its forwarding still reads that initializer
/// (`resolved_forwarded_method`): the receiver that initializer's adds would finish, whose
/// body completes with `undefined`, as the forwarding body then does.
#[inline(always)]
fn forwarded_fields_in_lane(vm: &mut Vm, callee: Value, argument: impl Fn(u16) -> Value) -> Option<Value> {
    let index = holding_construct_plan_index(vm, callee)?;
    // The plan in place: the allocation borrows the heap beside it, not a copy.
    let plan = &vm.lane_stack.construct_plans[index];
    plan.forwarded?.initializer?;
    let mut values = [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS];
    plan.fill_values(vm, &mut values, argument)?;
    // A nested construction's field is a fresh object of its own plan; allocation never collects.
    if let Some(nested) = vm.lane_stack.construct_plans[index].nested {
        if !construct_plan::nested_root_holds(vm, &nested) { return None; }
        if nested.constructs {
            values[usize::from(nested.field)] = construct_plan::nested_construction(vm, nested.constructor, &nested.arguments)?;
        }
    }
    { let plan = vm.lane_stack.construct_plans[index]; Some(plan.allocate_values(vm, &values)) }
}

/// Records in `callee`'s holding plan of no fields, whose forwarding resolved
/// (`ConstructPlan::forwarded`), the fields `fields` the forwarded `initializer`'s adds gave
/// `this`, which that plan's receiver started as: later constructions allocate the finished
/// object (`forwarded_fields_in_lane`). The adds ran no guest code, so the plan's epoch still
/// holds.
#[inline(never)]
fn record_forwarded_fields_in_lane(
    vm: &mut Vm,
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
    *plan = plan.with_recorded(fields, shape, ForwardedResolution { initializer: Some(initializer), ..resolution });
    if let Some(nested) = fields.nested {
        watch_construct_plan(vm, index, false, nested.held());
    }
}

/// The method a forwarding constructor's body (`ScriptCallTarget::forwarded_method`) would
/// call on the receiver `this`, which a construction may activate in its place: the body's
/// two reads, `this.<method>` and its `apply`, as cached hits that run no guest code, the
/// intrinsic `Function.prototype.apply`, and a frameless target. `None` leaves the
/// construction to the constructor's own activation, which makes the reads itself.
#[inline(never)]
fn forwarded_construction_in_lane(
    cx: *mut LaneContext,
    constructor: ScriptCallTarget,
    method: quench_runtime::bytecode::ForwardedThisMethod,
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
        None => lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, this, method.method.0, method.method.1, 0)?,
    };
    let apply = match (!vm.reads_function_info(method.apply.0))
        .then(|| vm.cell_holder_field_hit(code, function, method.apply.1))
        .flatten()
    {
        Some(apply) => apply,
        None => lane_frame::LaneFrame::<ACC_NONE>::cached_read(cx, code, function, method.apply.0, method.apply.1, 0)?,
    };
    if !matches!(
        vm.function_record(apply),
        Some(FunctionRecord {
            kind: FunctionKind::Native(Native::FunctionApply),
            ..
        })
    ) || function == constructor.callee
    {
        return None;
    }
    let forwarded = script_target_in_lane(cx, function)?;
    if let Some(resolution) = forwarded_resolution(vm, constructor.callee, method, function, apply) {
        let index = construct_plan_index(constructor.callee)?;
        record_forwarded_resolution(vm, index, function, resolution);
    }
    Some(forwarded)
}

/// Where a forwarding constructor's reads of `this.<method>` and its `apply`, which gave
/// `function` and `apply`, resolved under `callee`'s plan of no fields while it holds
/// (`ForwardedResolution`): own data properties of the plan's prototype and of the method's
/// prototype, neither shadowed by the receiver template or the method, in shapes that fix
/// their slots. `None` records nothing.
fn forwarded_resolution(
    vm: &Vm,
    callee: Value,
    method: quench_runtime::bytecode::ForwardedThisMethod,
    function: Value,
    apply: Value,
) -> Option<ForwardedResolution> {
    let plan = holding_construct_plan(vm, callee).filter(|plan| plan.fields == 0)?;
    let data_slot = |owner: &quench_runtime::tier_surface::Object, atom| {
        let shape = owner.shape();
        if vm.shape_is_dictionary(shape) {
            return None;
        }
        let slot = vm.shape_slot(shape, atom)?;
        // SAFETY: the slot exists in the non-dictionary shape that names it.
        let value = unsafe { vm.heap_reads().property_get_shaped(owner, slot) }?;
        (!value.is_deleted()).then_some((u16::try_from(slot).ok()?, value))
    };
    let data = |owner: Value, atom, slot: usize| {
        let key = quench_runtime::vm::PropertyKey::string(atom);
        vm.property_attributes(owner, key).is_some_and(|attributes| !attributes.accessor)
            && vm.object_data(owner).is_some_and(|object| vm.shape_slot(object.shape(), atom) == Some(slot))
    };
    if vm.shape_slot(plan.shape, method.method.0).is_some() {
        return None;
    }
    let prototype = vm.heap_reads().object_record(plan.prototype)?;
    let (method_slot, found) = data_slot(prototype, method.method.0)?;
    let FunctionRecord { object, .. } = vm.function_record(function)?;
    if found != function || vm.shape_slot(object.shape(), method.apply.0).is_some() {
        return None;
    }
    let apply_holder = object.proto();
    let (apply_slot, found) = data_slot(vm.heap_reads().object(apply_holder)?, method.apply.0)?;
    (found == apply
        && data(plan.prototype, method.method.0, usize::from(method_slot))
        && data(apply_holder, method.apply.0, usize::from(apply_slot)))
    .then_some(ForwardedResolution {
        method: function,
        apply_holder,
        initializer: None,
    })
}

/// The method a forwarding constructor's construction activates, from `callee`'s plan's
/// recorded resolution (`resolved_forwarded_method`) as a frameless target, under `plan`,
/// `callee`'s holding plan. `None` resolves afresh (`forwarded_construction_in_lane`).
#[inline(always)]
fn resolved_forwarding_in_lane(
    cx: *mut LaneContext,
    callee: Value,
    plan: Option<&ConstructPlan>,
) -> Option<ScriptCallTarget> {
    let function = resolved_forwarded_method(callee, plan?)?;
    script_target_in_lane(cx, function)
}

/// The method `callee`'s forwarding reads under `plan`, its holding plan: the recorded
/// resolution's (`ConstructPlan::forwarded`), which the plan's dependency keeps (see
/// `ForwardedResolution`), unless it is `callee`.
#[inline(always)]
fn resolved_forwarded_method(callee: Value, plan: &ConstructPlan) -> Option<Value> {
    plan.forwarded.map(|resolution| resolution.method).filter(|&method| method != callee)
}

script_handler! {
    /// `f.apply(this, arguments)` of the running activation's forwarded `arguments`
    /// (`CallForwardingArguments`): `f` runs frameless with the values the activation, or
    /// the frame, kept, as `Vm::try_inline_call_apply` pushes it. Any other call takes the
    /// general path.
    fn forward_call_miss<>(cx, r, ip, i, view) {
        match forward_activation_in_lane(cx, r, ip, i) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(next) => next,
        }
    }
}

/// `forward_call_miss`'s activation: the callee's registers and first record, or the record
/// to continue at. Out of line: the call target it passes on lives in its own frame, not
/// the handler's.
#[inline(never)]
fn forward_activation_in_lane(
    cx: *mut LaneContext,
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
        vm.function_record(r.get(i.b)),
        Some(FunctionRecord {
            kind: FunctionKind::Native(Native::FunctionApply),
            ..
        })
    ) {
        return Err(exit(ip));
    }
    let Some(kept) = (unsafe { kept_arguments_in_lane(cx) }) else {
        return Err(exit(ip));
    };
    let Some(target) = script_target_in_lane(cx, r.get(i.c)) else {
        return Err(exit(ip));
    };
    let receiver = r.get(window);
    let Some(this) = call_this_in_lane(vm, &target, receiver) else {
        return Err(exit(ip));
    };
    activate_keeping_in_lane::<false>(cx, r, ip, &target, kept, this, false, None)
}

use quench_runtime::bytecode::FORWARDED_APPLY_ARGUMENTS;

/// The values the running function kept for its forwarded `arguments`: a forwarding
/// activation's, after its registers, or a frame's `original_arguments`.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn kept_arguments_in_lane(cx: *mut LaneContext) -> Option<ActivationArguments> {
    let vm = unsafe { vm(cx) };
    if unsafe { (*cx).registers }.env().is_deleted() {
        let frame = &vm.frames[unsafe { (*cx).frame() }];
        return Some(ActivationArguments::Kept(
            frame.original_arguments.as_ptr(),
            frame.original_arguments.len(),
        ));
    }
    let activation = vm.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| script.forwards)?;
    let registers = usize::from(unsafe { &*(*cx).code }.function(activation.function() as usize).registers);
    // SAFETY: the activation's window holds its registers, then its kept values.
    let values = unsafe { activation.window_ptr().add(registers) };
    Some(ActivationArguments::Kept(values, usize::from(script.argc)))
}

/// The values the running function kept for its unmaterialized `arguments` binding, in the
/// local slot `slot` (`kept_arguments_in_lane`): `None` where a frame's binding was
/// materialized, which the general path then reads.
///
/// # Safety
/// `cx` is the running lane's context, and `slot` its function's validated local slot.
#[inline(always)]
unsafe fn unmaterialized_arguments_in_lane(
    cx: *mut LaneContext,
    slot: usize,
) -> Option<(*const Value, usize)> {
    if unsafe { (*cx).registers }.env().is_deleted()
        && unsafe { vm(cx).read_validated_local((*cx).frame(), slot) }
            != Value::UNMATERIALIZED_ARGUMENTS
    {
        return None;
    }
    match unsafe { kept_arguments_in_lane(cx) }? {
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
unsafe fn keeping_activation_in_lane(cx: *mut LaneContext) -> Option<(u16, u32)> {
    if unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let activation = unsafe { vm(cx) }.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| script.forwards)?;
    Some((script.argc, activation.function()))
}

/// The count of the running function's `arguments` binding in local `slot`, for the
/// `ArgumentsLength` row (`Frame::arguments_length`): a keeping activation's kept-value count.
/// `KEPT`: the view proved `slot` its function's kept arguments (`Function::kept_arguments`).
/// `None` leaves the count to the row's general step, which reads the frame's or window's binding.
///
/// # Safety
/// `cx` is the running lane's context, and `slot` its function's validated local slot.
#[inline(always)]
unsafe fn arguments_count<const KEPT: bool>(cx: *mut LaneContext, slot: u16) -> Option<Value> {
    if let Some((argc, function)) = unsafe { keeping_activation_in_lane(cx) }
        // SAFETY: the context's program outlives the lane run.
        && (KEPT || unsafe { &*(*cx).code }.function(function as usize).arguments_slot == Some(slot))
    {
        return Some(Value::integer(i32::from(argc)));
    }
    None
}

/// The keeping activation's `arguments` binding a local read of its slot takes (`Frame::arguments_binding`): a
/// frame run reads its own binding as the general path does (`Vm::read_validated_local`, which materializes it);
/// a frameless activation's is the unmaterialized marker a forwarding read passes on (`OBJECT` false), or the
/// object made from the values it kept (`OBJECT` true, `Vm::activation_arguments_object`), which the keeping
/// activation's last read makes. `None` where the object cannot be made: the row leaves to its miss.
/// Subsumes the hand loads `lane_load_forwarded_arguments` and `lane_load_kept_arguments_object` of the lane.
///
/// # Safety
/// `cx` is the running lane's context, and `r` its register file.
unsafe fn kept_arguments_binding<const OBJECT: bool>(cx: *mut (), r: Registers, slot: u16) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    // The forwarding check: a frame run's registers hold the deleted environment marker.
    if unsafe { (*cx).registers }.env().is_deleted() {
        // SAFETY: validated bytecode bounds the slot by Function.locals, and the frame is the running one.
        return Some(unsafe { vm.read_validated_local((*cx).frame(), usize::from(slot)) });
    }
    if !OBJECT {
        return Some(Value::UNMATERIALIZED_ARGUMENTS);
    }
    let Some((function, Some(script))) =
        vm.lane_stack.top_frame().map(|activation| (activation.function(), activation.script()))
    else {
        return None;
    };
    // SAFETY: the context's program outlives the lane run.
    let body = unsafe { &*(*cx).code }.function(function as usize);
    // A keeping activation's values follow its registers (`activate_keeping_in_lane`).
    let kept = (0..script.argc)
        .map(|index| r.get(body.registers + index))
        .collect::<SmallVec<[Value; GATHERED_INLINE_VALUES]>>();
    let context = CallContext::user_function(function, script.callee);
    let arguments = vm.activation_arguments_object(body, context, &kept).ok()?;
    note_allocation(vm);
    Some(arguments)
}

/// The frameless target of a lane call to `callee`, resolved on first use.
/// Whether `callee` is the target of a `new` whose `new.target` the general path still
/// holds (`Vm::construct_target`): a lane activation would not bind it.
#[inline(always)]
fn constructing_in_lane(vm: &Vm, callee: Value) -> bool {
    vm.construct_target
        .is_some_and(|target| target.callee == callee)
}

#[inline(always)]
fn script_target_in_lane(cx: *mut LaneContext, callee: Value) -> Option<ScriptCallTarget> {
    script_target_in_in_lane(cx, callee, &mut None).copied()
}

/// `script_target_in_lane`'s target, read in place where the cache holds it, else resolved
/// into `resolved`. The cache's entry holds until the next resolution or collection.
#[inline(always)]
fn script_target_in_in_lane(
    cx: *mut LaneContext,
    callee: Value,
    resolved: &mut Option<ScriptCallTarget>,
) -> Option<&ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // The paths besides a plain call (constructions, getters, forwarded and applied calls)
    // activate callees of the running program alone. Another program's callee, once
    // resolved, is held apart (`cross_call_entry`) for the callers that activate across
    // programs; resolving it again would only find it so.
    // The cached entry is tested and used in place; `script_call_holds` admitted it for the
    // active program already.
    if let Some(entry) = vm.script_call_entry(callee) {
        // SAFETY: the entry is read before anything else touches the cache.
        let target = unsafe { &*entry };
        return admits_activation_in_lane(target).then_some(target);
    }
    // A cached entry or cross entry is admitted by its link's compare, which refuses under
    // any condition (no script calls, a construction pending); a resolve makes no compare, so
    // the conditions refuse it here, once, for every unlinked caller.
    if vm.lane_stack.refuses_any() || vm.cross_call_entry(callee).is_some() {
        return None;
    }
    let active = vm.active_program;
    *resolved = vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee)
        .filter(|target| target.program() == active && admits_activation_in_lane(target));
    resolved.as_ref()
}

/// The instance environment the running Wasm function reads its bindings from: the
/// export's (`LaneContext::instance`) above a JavaScript call of it, else its frame's.
#[inline(always)]
fn instance_env_in_lane(cx: *mut LaneContext) -> Value {
    // SAFETY: the context outlives the lane run.
    let instance = unsafe { (*cx).instance };
    if !instance.is_deleted() {
        return instance;
    }
    unsafe { vm(cx).frames[(*cx).frame()].env }
}

/// Make `program` the lane's running program, as a call into or a return to one of its
/// functions makes it the general loop's executing one: the residual, lane views and
/// constants its records read, and the VM's active program, whose cache layout its sites
/// index.
#[inline(always)]
fn enter_program_in_lane(cx: *mut LaneContext, program: ProgramId) {
    let vm = unsafe { vm(cx) };
    let context = vm.program_context_in_lane(program);
    vm.activate_context(program, context);
    // The innermost plain activation's caller program (`PlainFrames::program`) is the program
    // the next plain push runs under. An emitted direct call bumps the plain depth without
    // writing it, so after a crossing push (whose side record keeps its own caller program)
    // the word must name the entered program, or a plain frame pushed above it by emitted
    // code reads its caller as the crossing frame's caller program.
    vm.lane_stack.plain.program.set(program);
    // SAFETY: the context outlives the lane run; the store keeps the program's residual,
    // views and constants for it, and its context names them.
    unsafe {
        (*cx).code = (*context).code;
        (*cx).program = program;
        (*cx).views = (*context).views;
        (*cx).constants = (*context).constants;
    }
}

/// Activate `target`, which keeps no passed values (callers route those to
/// `activate_keeping_in_lane`), for the call record at `ip` with the arguments from register
/// `arguments` on: the activation's registers and the callee's
/// first record, or the record to continue at when the call takes the general path.
#[inline(always)]
fn activate_in_lane(
    cx: *mut LaneContext,
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
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    // Values past the parameters stay observable (legacy `f.arguments`, a frame made of the
    // activation): the keeping activation holds them after the registers. A callee no legacy
    // reflection observes is passed its parameters alone, as its frame would keep them.
    if argc > target.params && (!target.drops_extra_arguments() || target.forwards()) {
        // A call keeping every value it was passed, a rest body's array aside, takes the plain
        // push with the values after its registers.
        if !constructs && target.entry != CallEntry::Gathers {
            return activate_keeping_window_in_lane::<true>(cx, r, ip, target, arguments, this);
        }
        return activate_keeping_in_lane::<true>(
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
    if !activation_room_in_lane(vm, target) {
        return Err(script_call_frame_in_lane(ip, usize::from(target.room)));
    }
    // SAFETY: the call's window of `r` holds its `argc` values.
    let passed = unsafe { r.0.add(usize::from(arguments)).cast_const() };
    Ok(push_passed_in_lane::<true, false>(cx, r, ip, target, passed, argc.min(target.params), this, constructs))
}

/// Whether the lane stack takes `target`'s activation: capacity, the guest-call budget and
/// a due collection (`LaneStack::activation_limit`).
#[inline(always)]
fn activation_room_in_lane(vm: &Vm, target: &ScriptCallTarget) -> bool {
    vm.lane_stack.admits_activations(1)
        && quench_runtime::staged::call::concrete_fits(vm.lane_stack.values.room() as u64, u64::from(target.room))
}

/// Whether `target`'s parameters take `argc` arguments and the lane stack holds its activation:
/// the staged admission (`staged::call::admit_room`) that the native tiers emit, run on words.
#[inline(always)]
fn admits_window_in_lane(vm: &Vm, target: &ScriptCallTarget, argc: u16) -> bool {
    quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(argc)) && activation_room_in_lane(vm, target)
}

/// Where the lane continues after a frameless activation's tier prologue (`tier_prologue_in_lane`): the
/// JavaScript tier runs the function's compiled body where it can (`Vm::js_tier_activation_entry`), and the
/// activation continues at the record the native result completes, with the result in its register; else at
/// its first record. Shared by the prologue's staged row (`EntryHost`) and its hand body.
#[inline(always)]
fn tier_prologue_resumed(cx: *mut LaneContext, r: Registers, view: Carried, function: u32, params: usize) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the activation just set the context to its own function's.
    let base = unsafe { (*cx).base };
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view);
    // A function of a program with a loaded AOT image enters the image's body at its first entry.
    // SAFETY: the prologue is this record, written when the tier claimed the function (`claim_for_tier`).
    if let Some(own) = own && vm.images_run_in_lane() && vm.programs.ahead_images(unsafe { (*cx).program }, unsafe { &*(*cx).code }).is_some() {
        let prologue = own.prologue.get().cast_const();
        return body_calls::enter_published_body(cx, prologue, r, view, function, unsafe { (*prologue).b } != 0);
    }
    // SAFETY: the activation's registers hold its parameters from its first.
    let parameters = unsafe { std::slice::from_raw_parts(r.0, params) };
    let entered = own
        .and_then(|own| tier_direct_in_lane(vm, own, parameters))
        .or_else(|| tier_enter_in_lane(cx, function, own, parameters));
    if let Some((pc, register, value)) = entered {
        r.set(register, value);
        // SAFETY: the tier returns a PC of the function, whose records are contiguous.
        return Resumed::At(r, unsafe { base.add(pc) }, Carried::script_in(cx, view));
    }
    Resumed::At(r, base, Carried::script_in(cx, view))
}

/// The protocol row of a frameless activation's tier prologue record (`TierPrologue`): the staged prologue
/// row (`PrologueFrame::tier_prologue`), with `tier_prologue_in_lane` as the body it completes where it leaves.
const fn tier_prologue_row() -> Handler {
    protocol_row!({ quench_runtime::bytecode::TIER_PROLOGUE as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::TIER_PROLOGUE as u16 })
}

/// The protocol row of a specialized body's prologue record (`BodyEntry`): the staged prologue row
/// (`PrologueFrame::body_entry`), with `body_entry_in_lane` as the body it completes where it leaves.
const fn body_entry_row() -> Handler {
    protocol_row!({ quench_runtime::bytecode::BODY_ENTRY as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::BODY_ENTRY as u16 })
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
    fn tier_prologue_in_lane<>(cx, r, ip, i, view) {
        // The staged row answers the same continuation (`EntryHost::tier`); this is its miss body.
        let resumed = tier_prologue_resumed(cx, r, view, i.imm(), usize::from(i.a));
        resume!(cx, resumed)
    }
}

script_handler! {
    /// The prologue record of a function whose lane body specialized
    /// (`LaneView::specialize_lane_body`): it runs the body on the activation's registers and
    /// continues where the body left. What the specialization proved it does not test again:
    /// each inlined call's guards run in the body, and a leave inside one resolves its target
    /// (`body_inlined_activation_in_lane`); the entry tests only what varies per run, the room
    /// for the activation such a leave makes.
    #[inline(never)]
    fn body_entry_in_lane<>(cx, r, ip, i, view) {
        // The staged row answers the same continuation (`EntryHost::body`); this is its miss body.
        let resumed = body_calls::enter_published_body(cx, ip, r, view, i.imm(), i.b != 0);
        resume!(cx, resumed)
    }
}

/// Whether an activation of the function whose records start at `records` calls the newest
/// activation, within `RECURSION_DEPTH` calls of it less `nearer` (those the caller already
/// looked at): each activation's caller base is its caller's records.
fn activation_reentered(vm: &Vm, records: Ip, nearer: usize) -> bool {
    use crate::compiled::tier_policy::RECURSION_DEPTH;
    vm.lane_stack.activations().iter().rev().take(RECURSION_DEPTH - nearer).any(|activation| activation.caller_base() == records)
}

/// The receiver word a body is entered with: the running activation's `this` where the
/// prologue says the body reads it (`LaneView::binds_this`), `DELETED` otherwise. A frame
/// activation keeps its `this` in the frame (`this_in_lane`); an uninitialized one stays `DELETED`,
/// at which the body's `LoadThis` leaves for the general path. The activation's `this` is
/// bound already, by the call's `direct_call_this_in_lane` (sloppy coercion included).
#[inline(always)]
fn entry_receiver_in_lane(cx: *mut LaneContext, binds_this: bool) -> Value {
    // SAFETY: the context is the running activation's, or the leaf's callee.
    if binds_this { unsafe { this_in_lane(cx, (*cx).registers) }.unwrap_or(Value::DELETED) } else { Value::DELETED }
}

/// Where the lane continues after a run of `method`, `own`'s body, that ran on the
/// activation `cx` holds, with registers `r`, and ended at `exit`: a returned run completes
/// the activation (no leave to resolve, no frame to materialize), any other leave resumes
/// where it names (`body_left_in_lane`); `started` is the audit's clock for this entry, read once
/// the run's leave is resolved.
#[inline(always)]
fn body_concluded_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    own: &LaneView,
    method: &crate::compiled::LaneMethod,
    left: crate::compiled::LaneExit,
    function: u32,
) -> Resumed {
    let base = unsafe { (*cx).base };
    if let crate::compiled::LaneExit::Returned { pc, value } = left {
        record_end_in_lane(cx, own, method, left);
        // SAFETY: as `leave_counts_in_lane`; a completed run carries no warming callee.
        unsafe { (*cx).callee_warming = false };
        note_body_end_in_lane(cx, own, left, |_| {});
        // SAFETY: the record that completes the activation is one of its function's.
        return returned_in_lane(cx, unsafe { base.add(pc) }, value);
    }
    let counts = leave_counts_in_lane(cx, left);
    let (resumed, feedback) = body_left_in_lane(cx, r, own, method, left);
    // Counted once the leave is resolved: a retirement or reoptimization moves the body.
    if counts {
        note_body_end_in_lane(cx, own, feedback, |left| note_leave_in_lane(cx, function, left));
    } else {
        note_warming_leave_in_lane(cx, own);
    }
    body_resumed_in_lane(cx, ip, resumed)
}

/// Where the lane continues after a lane body's resolved leave `resumed`, which left at `ip`.
#[inline(always)]
fn body_resumed_in_lane(cx: *mut LaneContext, ip: Ip, resumed: LaneBodyLeft) -> Resumed {
    match resumed {
        LaneBodyLeft::Terminated => Resumed::Exit(exit(ip)),
        LaneBodyLeft::Resume(_, resume, _) if body_calls::left_lane(resume) => Resumed::Exit(resume),
        LaneBodyLeft::Resume(registers, resume, carried) => Resumed::At(registers, resume, carried),
        LaneBodyLeft::Raised(record) => Resumed::Exit(raised_exit_in_lane(cx, record)),
        LaneBodyLeft::Completed(record, value) => returned_in_lane(cx, record, value),
    }
}

/// `lane_return_value!` as the resumption it continues at.
#[inline(always)]
fn returned_in_lane(cx: *mut LaneContext, record: Ip, value: Value) -> Resumed {
    complete_plain_activation_in_lane::<NOT_LEAF>(cx, value).unwrap_or_else(|| return_value_general_in_lane(cx, record, value))
}

/// The tier prologue's run of the compiled body `own` keeps for a body that reads only its
/// parameters (`LaneView::tier_direct`): the `Return` to continue at, its register and the
/// result.
#[inline(always)]
fn tier_direct_in_lane(
    vm: &mut Vm,
    own: &LaneView,
    parameters: &[Value],
) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
    let direct = own.tier_direct.take()?;
    let entered = vm.run_direct_tier_entry(&direct, parameters);
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
fn tier_enter_in_lane(
    cx: *mut LaneContext,
    function: u32,
    own: Option<&LaneView>,
    parameters: &[Value],
) -> Option<(usize, quench_runtime::bytecode::Register, Value)> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the activation just set the context to its own function's.
    let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
    // A lane body runs on the activation itself, so it takes the function from the
    // frame-entering tier: the activation warms toward it in the lane, and a function whose
    // body declines runs in the lane, as it does without lane bodies.
    if vm.programs.js_jit_lane_methods_enabled()
        && let Some(own) = own
    {
        // A program with a loaded AOT image entered its body already (`tier_prologue_resumed`).
        if !specialize_in_lane(cx, own, function, false, parameters, None) {
            own.release_from_tier();
        }
        return None;
    }
    // A function only lane bodies claimed (`claim_for_tier`) has no JS-tier entry to try.
    let may_claim = vm.js_tier_may_claim(code, program, function);
    if may_claim
        && let Some(entered) = vm.js_tier_activation_entry(code, program, function, parameters)
    {
        if let Some(own) = own
            && let Some(direct) = vm.direct_tier_entry(code, program, function)
        {
            own.tier_direct.set(Some(direct));
        }
        return Some(entered);
    }
    if let Some(own) = own
        && !may_claim
    {
        own.release_from_tier();
    }
    None
}

/// The script targets the call record at `pc` of `view` linked (`call_link`), newest first:
/// its latest and its history (`LaneStack::call_sites`), each where a plain call of it binds
/// an ordinary `this` and activates in place: what a lane body inlines (`lane_unit::unit`).
fn site_targets_in_lane(vm: &Vm, view: &LaneView, pc: u32) -> Vec<crate::compiled::CallTarget> {
    // SAFETY: the view holds a record per PC; reading its link changes nothing.
    let Some(record) = view.records.get(pc as usize).map(|record| unsafe { &*record.get() }) else { return Vec::new() };
    let history = vm.lane_stack.call_sites.borrow();
    // The callees the site resolved or memoized, in the order first seen (`LaneStack::note_site_callee`).
    let recorded = history.get(&std::ptr::from_ref(&record.call_link).addr());
    // A collection since the site linked retires its entry; the callee's own entry of this
    // generation, where a call resolved it again, is the same target. A linked entry another
    // site's callee took since names no callee of this site.
    let latest = vm.lane_stack.script_targets.entries.get(link_index(record.call_link.get())).and_then(|linked| {
        if linked.admission.of_generation(vm.lane_stack.admission) {
            Some(linked)
        } else {
            // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
            vm.script_call_entry(linked.callee).map(|target| unsafe { &*target })
        }
    }).filter(|target| {
        target.program() == vm.active_program
            && target.entry.activates_registers_only()
            && target.this_binding.binds_receiver()
            && recorded.is_none_or(|recorded| recorded.iter().any(|site| site.identity() == Some(target.callee)))
    });
    match latest {
        Some(latest) => {
            let latest = crate::compiled::CallTarget {
                callee: crate::compiled::CalleeBinding::Identity(latest.callee), function: latest.function };
            std::iter::once(latest)
                .chain(recorded.into_iter().flatten().rev().copied().filter(|target| *target != latest))
                .take(crate::compiled::tier_policy::POLYMORPHISM)
                .collect()
        }
        // A site that runs only in its function's lane body, or that a read memo answers,
        // links nothing current: the callees it recorded, else the body's own inlined calls
        // (`lane_unit::InlinedCall`), are its targets.
        None => match recorded {
            Some(recorded) => recorded.iter().rev().copied().collect(),
            None => view.body_view_in_lane().map_or_else(Vec::new, |method| {
                method.inlined.iter()
                    .filter(|frame| frame.parent.is_none() && frame.call == pc)
                    .map(|frame| frame.target)
                    .collect()
            }),
        },
    }
}

/// `site_targets_in_lane` of any function of the running program, by its view.
fn unit_targets_in_lane(cx: *mut LaneContext, function: u32, pc: u32) -> Vec<crate::compiled::CallTarget> {
    // SAFETY: the program's views outlive the lane run.
    let Some(view) = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view) else {
        return Vec::new();
    };
    site_targets_in_lane(unsafe { vm(cx) }, view, pc)
}

/// The mean trips of the loop at `header` of any function of the running program per native
/// run of it (`LoopTier::mean_trip`): what its call sites are weighed by (`lane_unit::with_trips`).
fn unit_trips_in_lane(cx: *mut LaneContext, function: u32, header: u32) -> Option<u64> {
    // SAFETY: the program's views outlive the lane run.
    let view = unsafe { &*(*cx).views }.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)?;
    for site in &view.loops {
        if site.header == header {
            return site.tier.borrow().mean_trip();
        }
    }
    None
}

/// The dense capture slots of `callee`'s environment (`Vm::callee_capture_slots`).
fn callee_captures_in_lane(cx: *mut LaneContext, callee: Value) -> (usize, usize) {
    unsafe { vm(cx) }.callee_capture_slots(callee)
}

#[quench_macros::ext(DispatchFastVm5)]
impl Vm {
    /// The dense capture slots of `callee`'s environment (`EnvironmentSlots::native_dense_view`),
    /// as values and count, or none: what an inlined frame of it reads its captures from.
    pub(crate) fn callee_capture_slots(&self, callee: Value) -> (usize, usize) {
        self.script_call_entry(callee)
            // SAFETY: as `Vm::script_call_entry`; a live environment's slots never move.
            .and_then(|target| unsafe { quench_runtime::tier_surface::EnvironmentSlots::native_storage_view(capture_reads((*target).capture)) })
            .map_or((0, 0), |(values, len)| (values.addr(), len))
    }
}

/// Per PC of `body`, the callees its call site reached where the callee's own lane body
/// specialized and the lane would activate it in place, with an ordinary `this`, from a
/// plain call of at most its parameters: what a lane body calls by its compiled entry
/// (`compiled::DirectCallee`), the latest first.
fn direct_callees_in_lane(vm: &Vm, view: &LaneView, function: u32, program: &quench_runtime::bytecode::ResidualProgram, body: &quench_runtime::bytecode::Function) -> Vec<crate::compiled::DirectCandidates> {
    let tail_calls = vm.host.proper_tail_calls() && body.strict;
    body.code.iter().enumerate().map(|(pc, packed)| {
        let instruction = body.widened(*packed);
        let direct = quench_runtime::staged::calls(instruction.op()) && !instruction.direct_eval() && !instruction.parameter_eval()
            && !(tail_calls && instruction.returns_from_frame());
        let mut callees = crate::compiled::DirectCandidates::new();
        if quench_runtime::staged::construction_family(instruction.op()) {
            callees.extend(construct_callee_in_lane(vm, view, function, pc, ImmediateLayout::argument_count(instruction.imm())));
            return callees;
        }
        let Some(argc) = direct_argc(program, instruction) else { return callees };
        callees.extend(body_tier_in_lane::site_targets(vm, view, pc).filter(|_| direct)
            .filter_map(|target| direct_callee_in_lane(vm, view, function, pc, argc, target)));
        callees
    }).collect()
}

/// The arguments a call instruction passes its direct row: a method call's are its method site's count, which only the
/// site names (`None` where the program holds no such site).
fn direct_argc(program: &quench_runtime::bytecode::ResidualProgram, instruction: WideInstruction) -> Option<u16> {
    if instruction.op() as u16 == quench_runtime::staged::METHOD_CALL_ROW {
        quench_runtime::staged::DecodeContext::of(program).method_site(instruction.method_site_index()).map(|site| site.argument_count)
    } else {
        Some(ImmediateLayout::argument_count(instruction.imm()))
    }
}

/// Records the forwarded initializer the construction at `i` ran, which a body may call by
/// its entry (`construct_callee_in_lane`).
fn note_construct_site(vm: &Vm, i: &LaneInstruction, constructor: Value, forwarded: &ScriptCallTarget) {
    vm.lane_stack.note_site_callee(&i.call_link, crate::compiled::CallTarget {
        callee: crate::compiled::CalleeBinding::Forwarded { through: constructor, callee: forwarded.callee },
        function: forwarded.function,
    });
}

/// The `compiled::DirectCallee` of the construction at `pc` of `view` through a forwarding
/// constructor: the forwarded initializer the site last ran (`construct_keeping_activation_in_lane`)
/// as the callee, the constructor as the identity it guards and the activation it skips.
fn construct_callee_in_lane(vm: &Vm, view: &LaneView, function: u32, pc: usize, argc: u16) -> Option<crate::compiled::DirectCallee> {
    for site in site_targets_in_lane(vm, view, pc as u32) {
        let crate::compiled::CalleeBinding::Forwarded { through, callee } = site.callee else { continue };
        // SAFETY: as `Vm::script_call_entry`; both targets are copied before anything resolves one.
        let (constructor, initializer) = unsafe { (*vm.script_call_entry(through)?, *vm.script_call_entry(callee)?) };
        let method = *vm.lane_stack.forwarded_methods.get(constructor.forwarded_method as usize)?;
        if !constructor.forwards() || !constructor.constructs() || holding_construct_plan(vm, through).is_none_or(|plan| plan.forwarded.is_none()) {
            return None;
        }
        let mut direct = direct_callee_in_lane(vm, view, function, pc, argc, initializer)?;
        direct.callee = through;
        direct.forwarder = Some(SkippedForwarder { function: constructor.function, callee: through, env: constructor.env, method });
        return Some(direct);
    }
    None
}

/// The `compiled::DirectCallee` of `target`, the callee the call at `pc` of `view` passes
/// `argc` arguments, where the lane would activate it in place.
pub(crate) fn direct_callee_in_lane(vm: &Vm, view: &LaneView, function: u32, pc: usize, argc: u16, target: ScriptCallTarget) -> Option<crate::compiled::DirectCallee> {
    // Only BaseJIT's direct row binds an arrow's `this`; an arrow that never reads it calls as strict.
    let (sloppy, lexical) = match target.this_binding {
        ThisBinding::Sloppy => (Some(vm.realm.globals), false),
        ThisBinding::Strict => (None, false),
        ThisBinding::Lexical if crate::compiled::admission::stencil_only() => (None, true),
        ThisBinding::Lexical => vm.programs.residual(target.admission.program()).filter(|code| !quench_runtime::bytecode::activation_reads_this(code, code.function(target.function as usize))).map(|_| (None, false))?,
        _ => return None,
    };
    (target.admission.current(vm.lane_stack.admission)
        && target.entry.activates() && !target.view.is_null() && argc <= target.params).then_some(())?;
    // SAFETY: a resolved target's view outlives its program.
    let view_of = unsafe { &*target.view };
    let (method, body) = view_of.pinned_callee(target.function == function);
    // SAFETY: a live environment's slots never move (`ScriptCallTarget::capture`).
    let (capture_values, capture_len) = unsafe { quench_runtime::tier_surface::EnvironmentSlots::native_storage_view(capture_reads(target.capture)) }
        .unwrap_or((std::ptr::null(), 0));
    Some(crate::compiled::DirectCallee {
        target, method, body, call: view.records().wrapping_add(pc).addr(), caller_base: view.records().addr(), cell: view_of.entry_cell(),
        callee: target.callee, registers: target.registers, sloppy, lexical, capture_values, capture_len, capture_stores: capture_stores(target.capture).is_some(), forwarder: None,
    })
}

/// Whether a lane body's run may call bodies by their compiled entries: the lane may make
/// their activations should it leave inside one (`body_call_helper`'s own conditions).
fn direct_calls_in_lane(cx: *mut LaneContext) -> bool {
    let vm = unsafe { vm(cx) };
    let script_calls = unsafe { (*cx).script_calls };
    // A direct callee's registers follow the caller's window, where an activation that keeps its
    // locals there (`context_locals_in_window_in_lane`) has them: such a run calls through the lane.
    script_calls && !vm.with_scope_entered && vm.construct_target.is_none() && !unsafe { context_locals_in_window_in_lane(cx as usize) }
}

/// Whether a run of `own`'s specialized body may start: the one thing per run it cannot
/// prove at specialization, that the lane can make the activation of a call it inlines
/// should it leave inside one (`body_inlined_activation_in_lane`). `pending` activations are
/// the running body's own, which its call has not pushed yet (`enter_cell_body_in_lane`).
#[inline(always)]
fn body_admits_in_lane(cx: *mut LaneContext, method: &crate::compiled::LaneMethod, pending: usize) -> bool {
    room_admits_in_lane(cx, method.recovery_space(), pending)
}

/// Whether the lane has the room `room` names: the activations and values a leave inside an inlined
/// callee of native code about to run makes (`body_inlined_activation_in_lane`), past the `pending`
/// activations the running body's own call has not pushed.
#[inline(always)]
fn room_admits_in_lane(cx: *mut LaneContext, room: LaneRecoverySpace, pending: usize) -> bool {
    let vm = unsafe { vm(cx) };
    // The values past the stack's top hold the frame slots the rows write past a lane activation's
    // registers (`LaneRecoverySpace::reaching`), whether or not the code inlines an activation.
    (room.activations == 0 && (!vm.lane_stack.has_frames() || vm.lane_stack.values.room() >= room.values))
        || (vm.lane_stack.activation_limit.get().saturating_sub(vm.lane_stack.activation_count() + pending) >= room.activations
            && vm.lane_stack.values.room() >= room.values
            && unsafe { (*cx).script_calls }
            && !vm.with_scope_entered)
}

/// A compiled region's recovery demand may exceed the calls the lane has observed.
/// Materialize before growing either Vec: live lane windows borrow their storage.
/// The caller must exit immediately and resume from its original semantic record.
#[cold]
#[inline(never)]
fn body_prepare_recovery_in_lane(cx: *mut LaneContext, method: &crate::compiled::LaneMethod) -> bool {
    room_prepare_in_lane(cx, method.recovery_space())
}

/// Grows the lane stacks to the room `room` names (`room_admits_in_lane`), materializing the lane's
/// activations first; whether it grew them. The caller must exit at once and resume from its
/// original semantic record.
#[cold]
#[inline(never)]
fn room_prepare_in_lane(cx: *mut LaneContext, room: LaneRecoverySpace) -> bool {
    let vm = unsafe { vm(cx) };
    if !unsafe { (*cx).script_calls } || vm.with_scope_entered { return false; }
    let values = vm.lane_stack.values.room() < room.values;
    let activations = vm.lane_stack.frame_capacity() - vm.lane_stack.activation_count() < room.activations;
    if !values && !activations { return false; }
    vm.materialize_lane_activations(cx);
    if values { vm.lane_stack.reserve_values(room.values); }
    if activations { vm.lane_stack.records_mut().reserve(room.activations); }
    vm.lane_stack.refresh_activation_limit();
    true
}

/// The calls native code runs as their callees' rows: a lane body's (`LaneMethod`) or a loop region's
/// (`CallCode`), their frames, whether the frames' registers are in the activation's register file past
/// the caller's (a stitched body's or region's) and not in the scratch window the code fills, and the
/// registers of the function that makes them.
#[derive(Clone, Copy)]
struct InlinedRun<'a> {
    frames: &'a [crate::compiled::InlinedCall],
    framed: bool,
    caller_registers: u16,
}

impl<'a> InlinedRun<'a> {
    fn of_body(method: &'a crate::compiled::LaneMethod) -> Self {
        Self { frames: &method.inlined, framed: method.framed(), caller_registers: method.caller_registers() }
    }

    /// A BaseJIT region's: its frames' registers are in the register file.
    fn of_region(calls: &'a crate::compiled::CallCode) -> Self {
        Self { frames: &calls.inlined, framed: true, caller_registers: calls.caller_registers() }
    }
}

/// The activation native code that left inside inlined `frame` of `run`, at its callee's
/// PC `pc`, continues in: the activations the calls of the frame and the frames around it
/// make (`push_activation_in_lane`), outermost first, each holding the registers native code
/// held for it (`Vm::scratch_in_lane`); the innermost's record at `pc`. Each frame's call
/// guard proved its callee the inlined one, whose resolution is a function of it; a cache
/// that dropped its entry resolves it again.
fn body_inlined_activation_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    base: Ip,
    run: InlinedRun<'_>,
    frame: usize,
    pc: u32,
) -> (Registers, Ip, Carried) {
    let frames = run.frames;
    if run.framed {
        // Only the frames' own reach: the scratch window may be wider (another code's), past this storage.
        Vm::adopt_window_in_lane(r.0.cast_const(), usize::from(run.caller_registers), crate::compiled::window_width(frames, run.caller_registers));
    }
    let mut chain = vec![frame];
    while let Some(parent) = frames[*chain.last().expect("a chain has a frame")].parent {
        chain.push(parent);
    }
    let (mut registers, mut records) = (r, base);
    let mut view = std::ptr::null::<LaneView>();
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    for (depth, &index) in chain.iter().enumerate().rev() {
        let call = frames[index];
        let vm = unsafe { vm(cx) };
        // Cache eviction is not invalidation of an immutable callee binding: a lifecycle
        // edge retires the body first (`ProgramStore::retire_dead_native_calls`). Resolve
        // only when recovery needs the activation, rather than scanning at entry.
        let callee = call.target.identity().expect("a lane body's inlined callee has its identity");
        let target = vm.script_call_entry(callee)
            // SAFETY: copy the current cache row before any cache mutation.
            .map(|target| unsafe { *target })
            .or_else(|| vm.resolve_script_call_target(code, callee))
            .expect("an inlined callee resolves");
        // SAFETY: the record is the call's in its parent's view.
        let (target, record) = (&target, unsafe { records.add(call.call as usize) });
        let parent = call.parent_base(frames);
        let this = call_this_in_lane(vm, target, registers.get(call.receiver - parent)).expect("a direct-object receiver binds itself");
        // The frame's own window and count: a call through `call` passes past its receiver
        // (`lane_unit::InlinedCall::function_call`).
        // SAFETY: the call's window of `registers` holds its values.
        let passed = unsafe { registers.0.add(usize::from(call.window - parent)).cast_const() };
        let (callee, _) = push_passed_in_lane::<false, false>(cx, registers, record, target, passed, call.arguments, this, false);
        // The frame's registers live where it stands, the leave's PC or its inner frame's
        // call; the exit wrote only those (`lane_region`), the rest keep the activation's.
        let at = match depth.checked_sub(1) { Some(inner) => frames[chain[inner]].call, None => pc };
        let live = code.register_root_masks(call.target.function);
        let scratch = usize::from(call.base - run.caller_registers);
        Vm::scratch_in_lane(scratch, usize::from(call.registers), |register, value| {
            if live.is_none_or(|live| live.is_live(at as usize, register)) {
                callee.set(register, value);
            }
        });
        // The activation's records start at the context's base, past any tier prologue its
        // entry record is.
        (registers, records, view) = (callee, unsafe { (*cx).base }, target.view);
    }
    // SAFETY: the innermost activation's view holds a record for each of its function's PCs.
    let resume = unsafe { records.add(pc as usize) };
    (registers, resume, carried_at_in_lane(cx, view, registers, pc))
}

/// What an activation carries into the record at `pc` of its function's `view`: the script
/// state, and the accumulator where the record reads one.
fn carried_at_in_lane(cx: *mut LaneContext, view: *const LaneView, registers: Registers, pc: u32) -> Carried {
    let mut carried = Carried::script(cx);
    // SAFETY: a resolved target's view outlives the lane run.
    if let Some(&register) = unsafe { &*view }.accumulator_registers.get(pc as usize)
        && register != NO_ACCUMULATOR
    {
        carried.acc = Slot::of(registers.get(register));
    }
    carried
}

/// A lane body's call at `call_pc` of the running activation (`lane_region`'s `Call`): the
/// activation `script_call_miss`'s common form makes, then the callee's own lane body. A
/// callee whose body returns completes the call (`call_return::step_activation_return_of`), the result in the
/// call's register; one that leaves, or has no body, leaves the lane at its record in that
/// activation (`LaneContext::callee_resume`). A call the common form does not make misses,
/// before any of its effects.
///
/// # Safety
/// `context` is the running lane's context, whose activation's registers hold the caller's
/// state; nothing the call runs allocates or collects.
/// A lane body's read of a capture `packed` names (depth in its high half, slot in its low),
/// by the lane's own `LoadCapture` lookup (`read_capture_value`): its value, or
/// `Value::DELETED` for none or an uninitialized binding, which leaves the body at the read.
/// `ArgumentsLength` of a lane body's entry function (`lane_arguments_length`): the count a
/// keeping activation kept, else the count an unmaterialized binding answers. `DELETED` where
/// the lane's own handler must read the materialized object, which the body's leave reaches.
pub(crate) unsafe extern "C" fn body_arguments_length_in_lane(_vm: u64, _residual: u64, context: u64, slot: u64) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    let value = match unsafe { keeping_activation_in_lane(cx) } {
        Some((argc, _)) => Value::integer(i32::from(argc)),
        None => match unsafe { unmaterialized_arguments_in_lane(cx, slot as usize) } {
            Some((_, count)) => Value::number(count as f64),
            None => Value::DELETED,
        },
    };
    value.to_bits()
}

/// `ArgumentGet` of that binding at an integer index a keeping activation kept or an
/// unmaterialized binding holds; any other key leaves at the record.
pub(crate) unsafe extern "C" fn body_argument_get_in_lane(_vm: u64, _residual: u64, context: u64, slot: u64, index: Value) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    if let Some((argc, function)) = unsafe { keeping_activation_in_lane(cx) }
        && let Some(index) = index.as_int().and_then(|index| u16::try_from(index).ok()).filter(|index| *index < argc)
    {
        // SAFETY: the context's program outlives the lane run, and the keeping activation
        // holds its `argc` values after the function's registers (`staged::call_keeping::keep_passed`).
        let registers = unsafe { &*(*cx).code }.function(function as usize).registers;
        return unsafe { (*cx).registers }.get(registers + index).to_bits();
    }
    match unsafe { unmaterialized_arguments_in_lane(cx, slot as usize) } {
        Some((values, count)) => index
            .as_int()
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < count)
            // SAFETY: `values` holds `count` kept values, and the index is below it.
            .map_or(Value::DELETED, |index| unsafe { *values.add(index) })
            .to_bits(),
        None => Value::DELETED.to_bits(),
    }
}

pub(crate) unsafe extern "C" fn body_capture_in_lane(_vm: u64, _residual: u64, context: u64, packed: u64) -> u64 {
    if context == 0 { return Value::DELETED.to_bits(); }
    let cx = context as usize as *mut LaneContext;
    let (depth, slot) = ((packed >> u16::BITS) as u16, packed as u16);
    let value = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            let vm = unsafe { vm(cx) };
            // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
            vm.walked_capture_slots(env, false, depth).and_then(|(slots, root)| vm.plain_capture(unsafe { &*slots }, root, slot))
        }
        None => read_capture_value::<true>(cx, depth, slot),
    };
    value
        .filter(|value| value.to_bits() & quench_runtime::value::ValueLayout::TAG_MASK != quench_runtime::value::ValueLayout::INTERNAL_TAG)
        .unwrap_or(Value::DELETED)
        .to_bits()
}

/// A lane body's store of `value` to the capture `packed` names (depth in its high half,
/// slot in its low), by the lane's own stores (`store_own_capture`, else
/// `store_to_capture`): 1 where it stored, else 0, which leaves the body at the store,
/// before it.
///
/// # Safety
/// As `body_capture_in_lane`.
pub(crate) unsafe extern "C" fn body_capture_store_in_lane(_vm: u64, _residual: u64, context: u64, packed: u64, value: Value) -> u64 {
    if context == 0 { return 0; }
    let cx = context as usize as *mut LaneContext;
    let (depth, slot, value) = ((packed >> u16::BITS) as u16, packed as u16, value);
    let stored = match native_frames::innermost_callee_env(cx) {
        Some(env) => {
            // SAFETY: the context's program outlives the lane run.
            let (code, program) = unsafe { (&*(*cx).code, (*cx).program) };
            store_to_environment(unsafe { vm(cx) }, code, program, env, depth, slot, value)
        }
        None => (depth == 0 && store_own_capture(cx, slot, value)) || store_to_capture(cx, depth, slot, value),
    };
    u64::from(stored)
}

/// Whether a body's run that ended at `exit` counts toward its retirement and
/// reoptimization: a leave into a callee that only lacked a body while warming toward one
/// (`LaneContext::callee_warming`) does not, as the next run may find the callee's body. Only
/// a `Callee` leave reads the flag: the callee's own code may run nested bodies before the
/// leave resolves, whose other ends must not take it.
fn leave_counts_in_lane(cx: *mut LaneContext, exit: crate::compiled::LaneExit) -> bool {
    // SAFETY: the running lane's context, read and reset by the run that left into the callee.
    exit != crate::compiled::LaneExit::Callee || !unsafe { std::mem::take(&mut (*cx).callee_warming) }
}

/// With diagnostics, counts a leave of `own`'s body into a warming callee, which
/// `leave_counts_in_lane` keeps from its feedback, in the view's report.
fn note_warming_leave_in_lane(cx: *mut LaneContext, own: &LaneView) {
    if unsafe { vm(cx) }.programs.js_jit_diagnostics_enabled() {
        own.tier().body_stats.warming_leaves.set(own.tier().body_stats.warming_leaves.get() + 1);
    }
}

/// Where the lane continues after a run of `method`, `own`'s body, on the activation whose
/// registers are `r`: the activations of the native frames it left inside first
/// (`native_frames::advance_into_frames`), then the record its leave names in the innermost, or the
/// innermost's completion; `body_left_in_lane` adds the leave as feedback (`LaneMethod::feedback`).
enum LaneBodyLeft {
    /// Irreversible cancellation leaves the lane without dispatching another record.
    Terminated,
    Resume(Registers, Ip, Carried),
    /// The activation's body raised an exception (`Vm::lane_raised`) at the record: the
    /// lane unwinds from it, or the call that made the activation pops it (`body_calls::body_returned_in_lane`).
    Raised(Ip),
    /// The activation completes with the value, by the record.
    Completed(Ip, Value),
}



/// Counts a run of `own`'s body that ended at `exit` on its typed edge (`ExitTable::cross`), and in the diagnostics leave table.
#[inline(always)]
fn record_end_in_lane(cx: *mut LaneContext, own: &LaneView, method: &crate::compiled::LaneMethod, exit: crate::compiled::LaneExit) {
    if !matches!(exit, crate::compiled::LaneExit::Returned { .. }) && quench_runtime::trace::Channel::BodyLeave.on() {
        eprintln!("QUENCH_BODY_LEAVE function={} exit={exit:?}", running_function_in_lane(cx));
    }
    method.exits.cross(exit);
    if unsafe { vm(cx) }.programs.js_jit_diagnostics_enabled() {
        own.tier().body_stats.ended(exit, &method.exits, &method.inlined, crate::compiled::take_lane_exit_line());
    }
}

/// `LaneView::note_lane_body_end`, counting the retirement it makes in the VM's tier events.
#[inline(always)]
fn note_body_end_in_lane(cx: *mut LaneContext, own: &LaneView, exit: crate::compiled::LaneExit, left_new: impl FnOnce((Option<u32>, usize))) {
    if own.note_lane_body_end(exit, false, left_new) {
        unsafe { vm(cx) }.programs.tier_events().record(Event::Retired);
    }
}

/// Counts an entry whose body the lane could not leave inside of (`body_admits_in_lane`): in the VM's
/// tier events and, with diagnostics, the view's report.
#[cold]
fn body_refused_in_lane(vm: &Vm, own: &LaneView) {
    vm.programs.tier_events().record(Event::Refused);
    if vm.programs.js_jit_diagnostics_enabled() {
        own.tier().body_stats.refused.set(own.tier().body_stats.refused.get() + 1);
    }
}

fn body_left_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    own: &LaneView,
    method: &crate::compiled::LaneMethod,
    exit: crate::compiled::LaneExit,
) -> (LaneBodyLeft, crate::compiled::LaneExit) {
    record_end_in_lane(cx, own, method, exit);
    if exit == crate::compiled::LaneExit::Terminated {
        // A fault can bypass the native exit's register snapshots. Discard its
        // native frames instead of materializing stale homes for guest execution.
        native_frames::discard_frames(cx);
        return (LaneBodyLeft::Terminated, exit);
    }
    crate::compiled::admission::retire_after_native(unsafe { vm(cx) }, exit, || native_frames::left_native_frames(cx));
    let feedback = method.feedback(exit, unsafe { &*(*cx).code });
    (frames_resumed_in_lane(cx, r, (own, Some(method)), exit), feedback)
}

/// The activation a leave resumes in: the one the leaving body ran in for a native call's callee, else
/// the innermost frame a native-frame advance made the running one, where there is one.
fn resumed_activation_in_lane<'a>(
    cx: *mut LaneContext,
    r: Registers,
    (own, root): (&'a LaneView, Option<&'a crate::compiled::LaneMethod>),
    callee: bool,
) -> (Registers, &'a LaneView, Option<&'a crate::compiled::LaneMethod>) {
    if callee {
        return (r, own, root);
    }
    match native_frames::advance_into_frames(cx) {
        // SAFETY: the advance made the innermost frame the running activation.
        Some((own, method)) => (unsafe { (*cx).registers }, own, Some(method)),
        None => (r, own, root),
    }
}

/// Where the lane continues after native code of the activation with registers `r`, a body of
/// `root`'s or a region's, left with `exit`: in the innermost of the native frames it left inside, which
/// the lane links and continues in first (`native_frames::advance_into_frames`; a leave into a callee's
/// activation already linked them and went on in it), then at the record `exit` names there, or the
/// innermost's completion. `own` is the activation's view; `root` the body that ran, where one did,
/// whose inlined frames an `exit` that was not inside a native frame names.
fn frames_resumed_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    (own, root): (&LaneView, Option<&crate::compiled::LaneMethod>),
    exit: crate::compiled::LaneExit,
) -> LaneBodyLeft {
    let callee = matches!(exit, crate::compiled::LaneExit::Callee);
    let (r, own, method) = resumed_activation_in_lane(cx, r, (own, root), callee);
    // SAFETY: the running activation's records start at the context's base.
    let base = unsafe { (*cx).base };
    match exit.resumed() {
        // SAFETY: the record that raised is one of the activation's function.
        crate::compiled::LaneExit::Threw { pc } => LaneBodyLeft::Raised(unsafe { base.add(pc) }),
        crate::compiled::LaneExit::Terminated | crate::compiled::LaneExit::Entry | crate::compiled::LaneExit::Stub => unreachable!("termination leaves before recovery; an entry leave or an unentered call resumes at its first record"),
        crate::compiled::LaneExit::At(pc) => {
            body_resume_at_in_lane(cx, r, own, loop_body::header_for_tier_up(cx, own, pc).unwrap_or(pc))
        }
        crate::compiled::LaneExit::Inlined { frame, pc } => {
            let method = method.expect("a leave inside an inlined callee is a lane body's");
            let (registers, resume, carried) = body_inlined_activation_in_lane(cx, r, base, InlinedRun::of_body(method), frame, pc);
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // A native call made its callee's activation and left the lane there.
        crate::compiled::LaneExit::Callee => {
            let mut carried = Carried::script(cx);
            // SAFETY: `body_call_helper` set both before the body left.
            let (registers, resume) = unsafe { ((*cx).registers, (*cx).callee_resume) };
            carried.acc = Slot::of(unsafe { (*cx).callee_acc });
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        // SAFETY: the record that completes the activation is one of its function's.
        crate::compiled::LaneExit::Returned { pc, value } => LaneBodyLeft::Completed(unsafe { base.add(pc) }, value),
    }
}

/// Reloads the heap bases the running body's rows read through the payload (`Vm::refresh_body_bases`), as
/// every call, construct and step helper of the body ends: the callee's run or an allocation may have
/// moved them.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn refresh_body_bases(context: u64) {
    let cx = context as usize as *mut LaneContext;
    // SAFETY: the caller's contract.
    unsafe { vm(cx).refresh_body_bases() };
}

/// Names `payload`, the dispatch of the native loop region that runs on the activation of the lane
/// `context`, to the helpers its rows call (`region_call_in_lane`), and returns the one it replaces:
/// a region's calls run lane code that may enter regions of its own.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn region_payload_in_lane(context: usize, payload: usize) -> usize {
    // SAFETY: the caller's contract.
    std::mem::replace(unsafe { &mut (*(context as *mut LaneContext)).region_payload }, payload)
}

/// The helper a loop region's call row runs (`LaneHelper::RegionCall`): the call at `call_pc` of the
/// activation the lane context names, whose registers the region runs on, as `body_call_helper`
/// makes it, then the heap bases the region's payload holds reloaded (`LaneContext::region_payload`)
/// besides the body frame's, which the callee's run or an allocation may have moved.
///
/// # Safety
/// `context` is the running lane's, which the region entered (`region_payload_in_lane`), and `call_pc` a call record of its function.
pub(crate) unsafe extern "C" fn region_call_in_lane(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    let status = unsafe { body_calls::body_call_helper(vm, residual, context, call_pc) };
    // SAFETY: as above.
    unsafe { refresh_region_bases_of(context) };
    status
}

/// Reloads the heap bases of the native loop region the lane context `context` names (`region_payload_in_lane`), where
/// one runs: the one refresh every lane helper a region's rows call ends with (`region_call_in_lane`, `body_general_step`,
/// `vm_operation_step`), so a row after any of them reads current bases (`Vm::refresh_region_bases`). Zero payload: no-op.
///
/// # Safety
/// `context` is the running lane's.
pub(crate) unsafe fn refresh_region_bases_of(context: u64) {
    let cx = context as usize as *mut LaneContext;
    // SAFETY: the caller's contract.
    let payload = unsafe { (*cx).region_payload };
    if payload != 0 {
        unsafe { self::vm(cx).refresh_region_bases(payload, direct_calls_in_lane(cx)) };
    }
}

/// Leaves the lane at `record`, where the running activation's body raised the exception in
/// `Vm::lane_raised`: the general loop unwinds from there, from the frames frameless
/// activations become first (`in_place_in_lane`).
fn raised_exit_in_lane(cx: *mut LaneContext, record: Ip) -> Ip {
    let vm = unsafe { vm(cx) };
    let (function, throw_pc) = crate::compiled::raise_site_parts(vm.lane_raise_site);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(error) = vm.lane_completion.raised_mut() { error.capture_source_location(code, function, throw_pc); }
    if let Some(settled) = raised_windows::settle_raised_window(cx, record) {
        return settled;
    }
    if vm.lane_stack.has_frames() {
        // SAFETY: `record` is one of the running function's records.
        let call_pc = unsafe { record.offset_from((*cx).base) } as usize;
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
        vm.materialize_deferred_lane();
    }
    exit(record)
}

/// Where the lane continues after a body left at `pc` of `own`'s function on registers `r`: in
/// the loop's published BaseJIT code where it has a resume stub for `pc` (`loop_resume`),
/// else at the record, with the accumulator that record reads.
fn body_resume_at_in_lane(cx: *mut LaneContext, r: Registers, own: &LaneView, pc: usize) -> LaneBodyLeft {
    let accumulator = own.accumulator_registers.get(pc).copied().filter(|&register| register != NO_ACCUMULATOR);
    if accumulator.is_none() && let Some(left) = loop_resume::resume_loop(cx, r, own, pc) {
        return left;
    }
    let mut carried = Carried::script(cx);
    if let Some(register) = accumulator {
        carried.acc = Slot::of(r.get(register));
    }
    // SAFETY: the body leaves at a PC of the function, whose records are contiguous.
    LaneBodyLeft::Resume(r, unsafe { (*cx).base.add(pc) }, carried)
}

/// Whether activating `target` leaves the running program. A target from
/// `LaneStack::script_targets` never does: `Vm::script_call_holds` admitted it for the VM's
/// active program, which is the lane's (`enter_program_in_lane` sets both), so the fast call
/// forms take that fact as proven (`MAY_CROSS` false) and only the general activation asks.
#[inline(always)]
fn crosses_in_lane<const MAY_CROSS: bool>(cx: *mut LaneContext, target: &ScriptCallTarget) -> bool {
    MAY_CROSS && target.program() != unsafe { (*cx).program }
}

/// `activate_in_lane` past its checks: the call passes at most `target`'s parameters, the lane
/// stack has room (`activation_room_in_lane`), and the call may complete its caller. Pushes
/// the activation and returns its registers and the callee's first record; it makes no
/// call, so a handler activating through it keeps its registers. Without `GENERAL` its
/// callers (the fast call forms) have sent another program's target to the general form
/// (`crosses_in_lane`).
#[inline(always)]
fn push_activation_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // SAFETY: `ip` is a record of the running view, whose window of `r` holds its values.
    let (passed, argc) = unsafe { (r.0.add(usize::from(arguments)).cast_const(), ImmediateLayout::argument_count((*ip).imm())) };
    push_passed_in_lane::<GENERAL, SETTLES>(cx, r, ip, target, passed, argc, this, constructs)
}

/// `push_activation_in_lane` of the `argc` values at `passed`, at most `target`'s parameters
/// and `FIXED_ARGUMENT_COPIES`, for the call record at `ip`; `SETTLES` for a settling
/// target's call (`Completion::SettlesPromise`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn push_passed_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // Only `script_call_settles_miss` activates a settling target, and it says so statically,
    // so the other call forms decide the completion without reading the target's entry.
    debug_assert_eq!(SETTLES, target.settles(), "a settling target activates through its own call form");
    let vm = unsafe { vm(cx) };
    let callee_registers = fill_window_in_lane(vm, target, passed, argc);
    enter_window_of_in_lane::<GENERAL, SETTLES, true>(cx, vm, r, ip, target, callee_registers, argc, this, constructs, None, None)
}

/// A plain call of `target`, of the running program, passed the `argc` values at `passed` (at
/// most its parameters) and not completing its caller, whose entry cell holds a published body
/// (`LaneView::cell_method`): the body runs on the activation's window before the activation
/// exists. The call is a leaf the context holds (`CONTEXT_LEAF`: the caller's receiver and
/// environment are the leaf's to restore), so a body that returns completes the call with the
/// caller's registers and the next record, nothing pushed and nothing to pop; a leave, and any
/// helper that observes activations (`materialize_running_leaf_in_lane`), make the activation the
/// push would have made, in the room the run reserved for it (`CALLEE_ACTIVATION_PENDING`),
/// and resume as the body's prologue would (`body_concluded_in_lane`). `None`, which changes
/// nothing, where the body does not run: the lane cannot make the activations a leave inside
/// its inlined calls would (`body_admits_in_lane`).
#[inline(never)]
fn enter_cell_body_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let own = unsafe { &*target.view };
    let method = own.cell_method()?;
    // Most entries of a published body come through its cell: one of the audit's interpreted
    // epoch activates the call as the lane's own would (`lane_body_audit`).
    // SAFETY: the running context; its records are the caller's.
    if own.audit_entry(false, || unsafe { (*cx).base } == own.records() || activation_reentered(vm, own.records(), 1)) {
        return None;
    }
    let window = fill_window_in_lane(vm, target, passed, argc);
    if !body_admits_in_lane(cx, method, CALLEE_ACTIVATION_PENDING) {
        // SAFETY: the window starts at the stack's top, which the fill moved.
        unsafe { vm.lane_stack.values.set_top(window.0) };
        return None;
    }
    begin_cell_leaf_in_lane(cx, ip, r, target, argc, this, window);
    let run = crate::compiled::BodyRun {
        method, direct_calls: direct_calls_in_lane(cx), program: unsafe { (*cx).program }, residual: unsafe { &*(*cx).code },
        registers: window.0, receiver: entry_receiver_in_lane(cx, own.binds_this()), captures: capture_reads(target.capture), lane_context: cx.cast(),
        pending: CALLEE_ACTIVATION_PENDING, bases_stand: false };
    Some(cell_body_ended_in_lane(cx, window, own, target.function, method, vm.run_lane_body(&run)))
}

/// Makes the call of `target` the context's running leaf (`CONTEXT_LEAF`), which holds what its
/// activation would (`materialize_leaf_in_lane`): the callee's receiver, environment, captures,
/// registers and records are the context's, the caller's are the leaf's; the entry is counted.
#[inline(always)]
fn begin_cell_leaf_in_lane(cx: *mut LaneContext, ip: Ip, r: Registers, target: &ScriptCallTarget, argc: u16, this: Value, window: Registers) {
    unsafe { vm(cx) }.programs.record_js_jit_lane_entry(target.program(), target.function);
    unsafe { vm(cx) }.programs.tier_events().record(Event::CellCrossed);
    // SAFETY: the running lane's context, and the callee's view outlives its program.
    // The staged leaf row (`call_leaf::enter_leaf`) writes the record, the receiver and the
    // environment, all the callee's in `CONTEXT_LEAF`.
    unsafe {
        lane_enter_leaf(cx, leaf_calls::FilledWindow, window, ip, r, target, CONTEXT_LEAF, argc, this, None);
    }
}

/// Where the lane continues after the body `enter_cell_body_in_lane` ran on `window` ended at
/// `exit`: the caller, past the call, where the body returned and the leaf still runs; else
/// the leave, in the activation made now. A helper that pushed it resolved targets since the
/// call, so the callee is `own` and `function`, not the call's target.
#[inline(always)]
fn cell_body_ended_in_lane(
    cx: *mut LaneContext,
    window: Registers,
    own: &LaneView,
    function: u32,
    method: &crate::compiled::LaneMethod,
    exit: crate::compiled::LaneExit,
) -> Resumed {
    if let crate::compiled::LaneExit::Returned { value, .. } = exit
        && !unsafe { (*cx).leaf.call }.is_null()
    {
        // SAFETY: the window held the callee's registers; the completed call leaves the stack as it found it.
        unsafe {
            (*cx).callee_warming = false;
            vm(cx).lane_stack.values.set_top(window.0);
        }
        record_end_in_lane(cx, own, method, exit);
        note_body_end_in_lane(cx, own, exit, |_| {});
        let (registers, next) = complete_leaf_in_lane(cx, value, CONTEXT_LEAF).expect("the leaf runs until it completes or materializes");
        return Resumed::At(registers, next, Carried::script(cx));
    }
    materialize_running_leaf_in_lane(cx);
    body_concluded_in_lane(cx, window, own.start(), own, method, exit, function)
}

/// The window of `target`'s plain activation at the lane stack's top, holding the `argc`
/// values at `passed` as its parameters, which the stack's values cover from here on.
#[inline(always)]
fn fill_window_in_lane(vm: &mut Vm, target: &ScriptCallTarget, passed: *const Value, argc: u16) -> Registers {
    let count = usize::from(target.registers);
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window's admitted room (`ScriptCallTarget::room`); the
    // registers live at entry are written before the length covers them, and no activation's
    // window moves. A register past `fill` is written before any instruction reads it, and
    // a collection reads it only where liveness proves it live (`ResidualProgram::entry_fill`).
    // Parameter `k` lives in register `k` of a plain activation (`Vm::place_plain_parameters`).
    unsafe {
        let registers = stack.values.top();
        fill_admitted_window(registers, usize::from(target.fill), count);
        copy_arguments(registers, passed, argc);
        stack.values.set_top(registers.add(count + lane_values::CONTEXT));
        Registers(registers)
    }
}

/// `push_passed_in_lane` past its window, which the stack's values cover from `window` on and
/// which holds the callee's registers: the activation, and the context made the callee's.
/// A native frame's materialization enters its window here too (`native_frames::advance_into_frames`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn enter_window_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    argc: u16,
    this: Value, constructs: bool, skipped: Option<SkippedForwarder>,
    kept: Option<u16>,
) -> (Registers, Ip) {
    enter_window_of_in_lane::<GENERAL, SETTLES, true>(cx, unsafe { vm(cx) }, r, ip, target, callee_registers, argc, this, constructs, skipped, kept)
}

/// `enter_window_in_lane` with the VM its caller already holds, so the push reads the lane
/// stack through it rather than through the context again after the window's writes.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn enter_window_of_in_lane<const GENERAL: bool, const SETTLES: bool, const COUNTED: bool>(
    cx: *mut LaneContext,
    vm: &mut Vm,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    argc: u16,
    this: Value, constructs: bool, skipped: Option<SkippedForwarder>,
    // The register of the first value a keeping activation kept past its registers
    // (`ScriptActivation::kept`), which marks it forwarding; `None` for any other.
    kept: Option<u16>,
) -> (Registers, Ip) {
    // The receiver goes to its context word first, so it holds no register past the push.
    callee_registers.set_this(this);
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // Only a plain call activates another program's callee (`Vm::resolve_script_call_target`).
    let cross = crosses_in_lane::<GENERAL>(cx, target);
    // The keeping row's record words (`staged::call_keeping::KeepingRecord`).
    let record = quench_runtime::staged::call_keeping::KeepingRecord::of_kept(argc, kept);
    let activation = Activation::of_parts(
        target.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc: record.argc,
            completion: if skipped.is_some() { Completion::Forwarder } else { Completion::of(constructs, cross, SETTLES) },
            forwards: record.forwards,
            kept: record.kept,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
    if let Some(skipped) = skipped { stack.attach_payload(SidePayload::Forwarder(skipped)); }
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    callee_registers.set_env(target.env);
    callee_registers.set_capture(target.capture);
    if cross {
        enter_program_in_lane(cx, target.program());
    }
    // A proved plain call (`PLAIN_LINK`) skips this per-call count (`COUNTED`); under the tier's
    // diagnostics `enter_plain_window` counts it instead.
    if COUNTED { vm.programs.record_js_jit_lane_entry(target.program(), target.function); }
    // A construction runs its body from the first record; the tier enters plain calls.
    let start = if constructs { base } else { unsafe { (*target.view).start() } };
    (callee_registers, start)
}

/// Counts a proved call's entry, which only the tier's diagnostics make (`enter_plain_window`).
#[cold]
#[inline(never)]
fn count_proved_entry_in_lane(vm: &mut Vm, target: &ScriptCallTarget) {
    let (program, function) = (target.program(), target.function);
    vm.programs.record_js_jit_lane_entry(program, function);
}

/// `enter_window_of_in_lane` of a proved plain call (`lane_plain_activation!` past `PLAIN_LINK`):
/// the record it would build is always plain (no general form, so no crossing; no
/// construction, settling, keeping or forwarding; an empty native prefix; a lane stack
/// window), so its header goes below the window straight from the call's words
/// (`protocol::push_plain_at`), with no `Activation` value and no `LaneStack::header_flags`
/// test. `depth` is the plain depth `LaneStack::plain_room` admitted it at.
/// The tier's entry count (`record_js_jit_lane_entry`) is made here only under diagnostics.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn enter_plain_window(
    cx: *mut LaneContext,
    vm: &mut Vm,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    depth: usize,
    argc: u16,
    this: Value,
) -> (Registers, Ip) {
    use quench_runtime::staged::protocol::{push_plain_at, PlainHeader};
    callee_registers.set_this(this);
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: the running lane's context.
    let (caller_base, program) = unsafe { ((*cx).base, (*cx).program) };
    debug_assert!(depth == stack.plain.depth.get());
    debug_assert!(stack.plain_program_holds(program));
    debug_assert!(callee_registers.0.addr() & HEAP_WINDOW == 0, "a lane stack window");
    // The record `enter_window_of_in_lane` builds here keeps `argc` (`KeepingRecord::of_kept`
    // without kept values) and completes `Completion::of(false, false, false)`, `Plain`.
    let header = PlainHeader::new(
        ip.expose_provenance() as u64, r.0.expose_provenance() as u64, caller_base.expose_provenance() as u64,
        target.function, argc, 0, target.callee.to_bits(),
    );
    push_plain_at(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
        stack.plain_frames_word() as u64, callee_registers.0.expose_provenance() as u64, header, u64::from(program.raw()), depth as u64,
    );
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    callee_registers.set_env(target.env);
    callee_registers.set_capture(target.capture);
    // A proved call skips the tier entry count (`enter_window_of_in_lane`'s `COUNTED`) unless the
    // tier's diagnostics count entries: then this cold branch does, so they observe the same path.
    if vm.programs.js_jit_diagnostics_enabled() { count_proved_entry_in_lane(vm, target); }
    (callee_registers, unsafe { (*target.view).start() })
}

/// The frameless target of another program's function `callee` a native calls from the
/// lane (`Vm::native_callback_in_lane`), as a trap's (`AccessorCall::trap_target`).
fn cross_callback_target_in_lane(cx: *mut LaneContext, callee: Value) -> Option<(ScriptCallTarget, bool)> {
    AccessorCall::of(cx).trap_target(callee).filter(|&(_, crosses)| crosses)
}

/// The activation of a native's call of `target` with `args` from the lane's running
/// function (`Vm::native_callback_in_lane`), as the lane's own call would make it from the call
/// record `call`, completing to the native (`Completion::ReturnsToNative`): by its shape
/// (`PreparedShape`), or, for a gathering target, whose rest array it makes, or a call
/// without room, which grows the stack for the general path it takes, as
/// `activate_keeping_in_lane` makes it. Its first record, or `None` where the lane would not
/// activate the call.
fn push_callback_in_lane(
    cx: *mut LaneContext,
    call: Ip,
    target: &ScriptCallTarget,
    args: &[Value],
    this: Value,
    crosses: bool,
) -> Option<Ip> {
    debug_assert_eq!(crosses, crosses_in_lane::<true>(cx, target), "a native's lane callee crosses where its program is another");
    if !crosses {
        // SAFETY: the running lane's context, which nothing else accesses.
        let shape = PreparedShape::of(unsafe { &*cx }, call, target, args.len());
        if let Some(shape) = &shape
            && let Some(start) = push_prepared_in_lane(cx, shape, &shape.entry(target, this), args)
        {
            return Some(start);
        }
    }
    let (r, arguments) = (unsafe { (*cx).registers }, ActivationArguments::Kept(args.as_ptr(), args.len()));
    let (_, start) = if crosses {
        activate_keeping_in_lane::<true>(cx, r, call, target, arguments, this, false, None).ok()?
    } else {
        activate_keeping_in_lane::<false>(cx, r, call, target, arguments, this, false, None).ok()?
    };
    let vm = unsafe { vm(cx) };
    // Given its completion where it is, a header's side record or the record (no spill).
    vm.lane_stack.recomplete_top(Completion::ReturnsToNative);
    Some(start)
}

/// `activate_in_lane` of an environment target (`CallEntry::Environment`): its window holds
/// its registers, then its environment, made now as the frame's promotion would make it
/// (each captured slot its parameter's argument or `undefined`, every other slot owned
/// elsewhere), then its frame locals (each parameter its argument, a captured slot owned
/// by the environment, every other local `undefined`), then the values passed. A parameter
/// promoted in place (`EnvironmentWindow::parameter_registers`) starts in its register and
/// leaves its local `undefined`, as a frame's promoted parameter does. The
/// environment is reachable only from the window until the lane leaves, which makes the
/// activation a frame holding it.
#[inline(always)]
fn activate_environment_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    (this, constructs): (Value, bool),
) -> Result<(Registers, Ip), Ip> {
    use quench_runtime::staged::call_environment as environment_row;
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    // Another program's callee enters its program and completes across (`Completion::Cross`).
    let cross = target.program() != unsafe { (*cx).program };
    let Ok(argc) = u16::try_from(passed.len()) else {
        return Err(exit(ip));
    };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    // SAFETY: the program store keeps a derived view alive with its program, and an
    // environment target's view has a window (`ScriptCallTarget::entry`).
    let layout = unsafe { (*target.view).environment_window.unwrap_unchecked() };
    let (rest, self_binding) = (layout.rest, layout.self_binding);
    // A body reading its `arguments` keeps every value passed past its parameters too, as
    // a frame does where legacy reflection (`f.arguments`) can read them; elsewhere nothing
    // observes them, and the window keeps only those its parameters bind. Only a call
    // passing more values than the parameters reads the body's plan.
    let extra = argc > target.params && !rest && {
        let Some(code) = vm.programs.residual(target.program()) else {
            return Err(exit(ip));
        };
        let plan = code.function(target.function as usize).call_plan(code);
        if !plan.reads_passed_arguments && plan.legacy_reflection {
            return Err(exit(ip));
        }
        !plan.reads_passed_arguments
    };
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    let (passed, argc) = if extra { (&passed[..usize::from(target.params)], target.params) } else { (passed, argc) };
    // A rest body keeps every value passed, the room sized for its parameters and the rest.
    let extra = usize::from(argc.saturating_sub(target.params));
    let needed = usize::from(target.room) + extra;
    // Both stacks' room, one row (`call_environment::admit_environment`).
    if !environment_row::concrete_admit_environment(
        vm.lane_stack.activation_count() as u64,
        vm.lane_stack.activation_limit.get() as u64,
        vm.lane_stack.values.room() as u64,
        needed as u64,
    ) {
        return Err(script_call_frame_in_lane(ip, needed));
    }
    // The rest array, made before the window is: the values past the other parameters,
    // which the rest parameter's local holds (`activate_keeping_in_lane` gathers likewise).
    // A plain branch: `bool::then` with this closure stayed an out-of-line call on every
    // activation.
    let gathered = if rest {
        let fixed = usize::from(target.params) - 1;
        Some(vm.rest_parameter_array(passed.get(fixed..).unwrap_or_default()))
    } else {
        None
    };
    // The environment window, one row (`call_environment::enter_environment`): every register
    // live at entry (`ScriptCallTarget::fill`) and every local starts `undefined` within the
    // target's reserved room (`ScriptCallTarget::room`), the entries are laid out (the
    // environment is made when one is needed), parameter `k` local `k` holds its argument where
    // one was passed and every value is kept after the locals for legacy reflection, then the
    // rest array, the self binding and the frame context (`this`, `env`, `capture`).
    const _: () = assert!(
        environment_row::ENTRY_ENVIRONMENT == WINDOW_ENVIRONMENT
            && environment_row::ENTRY_SLOTS == WINDOW_SLOTS
            && environment_row::ENTRY_CAPTURED == WINDOW_CAPTURED
            && environment_row::ENTRY_STORAGE == WINDOW_STORAGE
            && environment_row::STORAGE_WORDS as usize == quench_runtime::tier_surface::IN_PLACE_SLOTS_WORDS
            && environment_row::ENTRY_WORDS == WINDOW_LOCALS
    );
    let shape = environment_row::EnvironmentShape {
        registers: target.registers,
        live: target.entry_registers() as u16,
        locals: layout.locals,
        captured: layout.captured,
    };
    let held = shape.held();
    debug_assert!(fill_room::<SCRIPT_FILL_BLOCK>(held) <= usize::from(target.room));
    let rest = if let Some(gathered) = gathered { Some((target.params - 1, gathered.to_bits())) } else { None };
    let self_binding = if let Some(slot) = self_binding { Some((slot, target.callee.to_bits())) } else { None };
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window (`ScriptCallTarget::room`); each slot is written
    // before the length covers it, and no activation's window moves.
    let callee_registers = unsafe {
        let registers = stack.values.top();
        environment_row::enter_environment(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
            registers.expose_provenance() as u64,
            shape,
            environment_row::EnvironmentPassed {
                arguments: passed.as_ptr().expose_provenance() as u64,
                count: argc,
                bound: argc.min(target.params),
                in_registers: argc.min(layout.parameter_registers),
                rest,
                self_binding,
            },
            environment_row::WindowContext {
                this: this.to_bits(),
                env: target.env.to_bits(),
                capture: target.capture.expose_provenance() as u64,
            },
        );
        stack.values.set_top(registers.add(held + usize::from(argc) + lane_values::CONTEXT));
        Registers(registers)
    };
    let activation = Activation::of_parts(
        target.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc,
            completion: Completion::of(constructs, cross, false),
            forwards: false,
            kept: 0,
            environment: true,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    // The frame context was written with the window (`call_environment::bind_context`).
    if cross {
        enter_program_in_lane(cx, target.program());
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    Ok((callee_registers, base))
}

/// The environment of the environment window whose entries start at `entries`
/// (`WINDOW_ENVIRONMENT`): the one already made, or a new environment of `function` of
/// `program` under `parent` that takes over the slots the window held in place, its
/// captured ones from the locals, which then hold `DELETED` for them as a frame's do. The
/// window reaches the new storage from then on. The environment binds the activation's
/// `this`, where its closures' arrows find it (`Vm::captured_lexical_this`), as a frame's
/// promotion carries its bindings; an arrow's own `this` is that same lexical value. The
/// environment of a window over a record also takes over the activation's dynamic `bindings`
/// (`FrameRecord::dynamic_bindings`, `Activation::take_record_bindings`), as a frame's
/// promotion takes its own (`Vm::promote_frame_environment`); any other passes none.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`activate_environment_in_lane`).
unsafe fn window_environment(
    vm: &mut Vm,
    code: &ResidualProgram,
    entries: *mut Value,
    function: u32,
    parent: Value,
    program: ProgramId,
    this: Value,
    bindings: Option<Vec<(quench_runtime::bytecode::Atom, Value)>>,
) -> Value {
    // SAFETY: as the caller guarantees.
    unsafe {
        let made = *entries.add(usize::from(WINDOW_ENVIRONMENT));
        if !made.is_deleted() {
            return made;
        }
        let slots = window_slots_taken(entries);
        // A non-arrow binds its `this` where its closures' arrows find it
        // (`Vm::captured_lexical_this`), as a frame does when a closure or the body itself
        // can observe its implicit bindings (`Function::activation_bindings_observable`);
        // any other scope names only its program, which needs no allocation.
        let mut bindings = bindings.unwrap_or_default();
        let body = code.function(function as usize);
        if !body.is_arrow && body.activation_bindings_observable(code) && body.maker_bindings_read(code) {
            bindings.insert(0, (vm.runtime_atoms.lexical_this, this));
        }
        let scope = window_scope_ref(program, bindings);
        let (environment, storage) = vm.allocate_environment(parent, function, slots, scope);
        // Its storage links to its parent's for its closures' outer captures
        // (`EnvironmentSlots::link`), as a capture walk would find the parent's.
        if let Some(quench_runtime::tier_surface::EnvironmentRecord { function: owner, slots: parent_slots, .. }) = vm.heap_reads().environment(parent)
            && *owner != u32::MAX
            && *owner != quench_runtime::vm::ROOT_FUNCTION_ID
            && let Some(parent_storage) = parent_slots.dense_storage()
            && let Some(own) = (*storage).dense_storage()
        {
            quench_runtime::tier_surface::EnvironmentSlots::link(own, parent_storage, vm.heap_reads().collection_epoch());
        }
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
        environment
    }
}

/// The slots of an environment made from the window whose entries are `entries`: the locals a
/// closure captured, each cleared in the window, which the environment owns from then on.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`activate_environment_in_lane`).
unsafe fn window_slots_taken(entries: *mut Value) -> quench_runtime::tier_surface::EnvironmentSlots {
    // SAFETY: as the caller guarantees.
    unsafe {
        let captured = std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_CAPTURED)));
        let in_place = &*(std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_SLOTS)))
            as *const quench_runtime::tier_surface::EnvironmentSlots);
        let locals = entries.add(usize::from(WINDOW_LOCALS));
        let window = EnvironmentWindow { locals: in_place.len() as u16, captured, rest: false, self_binding: None, parameter_registers: 0 };
        // SAFETY: the mask names locals of the window, which hold owned values.
        let slots = quench_runtime::tier_surface::EnvironmentSlots::from_captured(in_place.len(), window.captured, locals);
        // Each captured local, one per bit of the mask, which names no slot past the locals.
        debug_assert_eq!(captured.checked_shr(u32::from(window.locals)).unwrap_or(0), 0);
        let mut remaining = captured;
        while remaining != 0 {
            locals.add(remaining.trailing_zeros() as usize).write(Value::DELETED);
            remaining &= remaining - 1;
        }
        slots
    }
}

/// The scope of a window's environment: its program's alone, which needs no allocation, or
/// holding the dynamic `bindings` it takes over.
fn window_scope_ref(program: ProgramId, bindings: Vec<(quench_runtime::bytecode::Atom, Value)>) -> quench_runtime::tier_surface::EnvironmentScopeRef {
    if bindings.is_empty() {
        return quench_runtime::tier_surface::EnvironmentScopeRef::of_program(program.raw());
    }
    quench_runtime::tier_surface::EnvironmentScopeRef::new(quench_runtime::tier_surface::EnvironmentScope {
        program: Some(program.raw()),
        root_eval_scope: false,
        binding_site_pc: None,
        dynamic_bindings: bindings.into(),
        with_objects: Box::default(),
    })
}

/// The body of [[Construct]] for a constructor that only stores its parameters into fields
/// of `this` (`ScriptCallTarget::initializes`): its receiver
/// (`Vm::inline_construct_receiver`), and whether every store was a cached add
/// (`Vm::primary_field_add_hit`), which runs no guest code, so the body completed. After a
/// miss the constructor activates on the receiver: its body repeats the stores made so far
/// as stores of the same values to the own data properties they added, which leaves the
/// object as one activation would. A body whose adds all hit leaves its `ConstructPlan`,
/// with which a later construct allocates the finished object at once
/// (`planned_construction_in_lane`), which comes first. Out of line, so the construct keeps
/// its tail dispatch.
#[inline(never)]
fn initialize_fields_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    callee: Value,
    constructor: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
) -> Option<(Value, bool)> {
    let stores = code.function(function as usize).initializer_stores(&code.constants)?;
    let env = vm.same_realm_user_target(callee).map_or(Value::UNDEFINED, |(_, _, env)| env);
    initialize_stores_in_lane(vm, callee, constructor, env, |vm, this, nested, each| {
        add_stores_in_lane(vm, code, this, stores, argument, nested, each)
    })
}

/// The receiver `Vm::inline_construct_receiver` allocates for a constructor that forwards its
/// arguments (`script_construct_keeping_miss`): from `callee`'s holding plan `plan` where it
/// has no fields, else from the template, which leaves that plan. `script_construct_miss` sends a
/// forwarding target here before it tests `ScriptCallTarget::initializes`, so a callee's plan
/// is only ever this one or `initialize_fields_in_lane`'s.
fn planned_receiver_in_lane(
    vm: &mut Vm,
    callee: Value,
    plan: Option<&ConstructPlan>,
) -> Option<Value> {
    if let Some(plan) = plan.filter(|plan| plan.fields == 0) {
        return Some(plan.allocate(vm, |_| Value::UNDEFINED));
    }
    let template = vm.inline_construct_template(callee)?;
    let (prototype, shape) = (template.proto(), template.shape());
    let this = vm.allocate_object(template);
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
        watch_construct_plan(vm, index, true, &[callee, prototype]);
    }
    Some(this)
}

/// `activate_in_lane` for a target that keeps its passed values, or for values a forwarding
/// activation or frame kept: the activation's registers and the callee's first record, or
/// the record to continue at when the call takes the general path. Out of line, so the
/// common activation keeps its code. Without `MAY_CROSS` its callers route the running
/// program's targets, as every path but a plain call's makes them (`script_target_in_lane`),
/// so it takes that fact as proven.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn activate_keeping_in_lane<const MAY_CROSS: bool>(
    cx: *mut LaneContext,
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
    // here too (`activate_in_lane`).
    let cross = crosses_in_lane::<MAY_CROSS>(cx, target);
    debug_assert!(!(cross && forwarder.is_some()));
    let argc = match arguments {
        ActivationArguments::Window(_) => usize::from(ImmediateLayout::argument_count(i.imm())),
        ActivationArguments::Kept(_, count) => count,
    };
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = vm.programs.residual(target.program()).and_then(|code| {
        let body = code.function_get(target.function as usize)?;
        let plan = body.call_plan(code);
        Some(format!(
            "keeping {} @{:?} entry={} argc={argc} params={} strict={} legacy={} forwards={} arguments={}",
            body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
            body.source_span.map(|span| span.start), target.entry as u8, target.params, body.strict,
            plan.legacy_reflection, plan.forwards_arguments, body.arguments_slot.is_some(),
        ))
    }) {
        quench_runtime::vm::lane_exits::record_detail(body);
    }
    let count = usize::from(target.registers);
    // A forwarding activation keeps every value passed after its registers, as does one for
    // a skipped forwarding constructor, whose frame passed them on, and one passed more values
    // than it has parameters.
    // A rest body keeps nothing its rest array holds, unless a kept `arguments` or legacy
    // reflection reads them (`TARGET_GATHERS_KEEPING`).
    let gathers = target.entry == CallEntry::Gathers;
    // A rest view keeps them too, as `activate_gathering_in_lane` keeps them: its reads and
    // its frame, once materialized, take the values passed, never a rest array.
    let keeps = forwarder.is_some()
        || if gathers {
            target.forms & (TARGET_GATHERS_KEEPING | TARGET_REST_VIEW) != 0
        } else {
            target.forwards() || argc > usize::from(target.params)
        };
    let window_size = if keeps { count + argc } else { count };
    // The rest array, made before the window is: the values past the other parameters, read
    // in place from the caller's window or the kept values.
    // A rest view that keeps the values reads them in place (`LaneFrame::rest_view`).
    let rest = (gathers && !(keeps && target.forms & TARGET_REST_VIEW != 0)).then(|| {
        let fixed = usize::from(target.params) - 1;
        let passed: &[Value] = match arguments {
            // SAFETY: the window holds `argc` registers of the caller.
            ActivationArguments::Window(base) => unsafe { std::slice::from_raw_parts(r.0.add(usize::from(base)), argc) },
            // SAFETY: kept values hold `argc` values.
            ActivationArguments::Kept(values, _) => unsafe { std::slice::from_raw_parts(values, argc) },
        };
        vm.rest_parameter_array(passed.get(fixed..).unwrap_or_default())
    });
    // A call that returns its callee's completion completes its caller too, which only an
    // activation's return does in the lane. A host permitting proper tail calls
    // sends strict terminal calls to the general dispatch path instead.
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    if argc > usize::from(u16::MAX)
        || vm.with_scope_entered
    {
        return Err(exit(ip));
    }
    // Capacity, the guest-call budget and a due collection (`LaneStack::activation_limit`):
    // the keeping row's admission (`staged::call_keeping::admit_keeping`).
    let needed = fill_room::<ACTIVATION_FILL_BLOCK>(window_size);
    if !quench_runtime::staged::call_keeping::concrete_admits_keeping(
        vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), needed,
    ) {
        return Err(script_call_frame_in_lane(ip, needed));
    }
    let shape = quench_runtime::staged::call_keeping::KeepingWindow { registers: target.registers, params: target.params, keeps, gathers };
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window; each slot is written before the length covers
    // it, and no activation's window moves. Parameter `k` lives in register `k` of a
    // plain activation (`Vm::place_plain_parameters`). The passed values lie in the caller's
    // window or where they were kept, below the new window or in a frame, outside it.
    let callee_registers = unsafe {
        let registers = stack.values.top();
        let passed = match arguments {
            ActivationArguments::Window(base) => r.0.add(usize::from(base)).cast_const(),
            ActivationArguments::Kept(values, _) => values,
        };
        // The keeping row: the parameters, the kept values, and the rest register, whose
        // `undefined` in a rest view makes its reads take the kept values.
        quench_runtime::staged::call_keeping::fill_keeping(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()), registers.expose_provenance() as u64,
            passed.expose_provenance() as u64, argc as u16, shape, rest.unwrap_or(Value::UNDEFINED).to_bits(),
        );
        stack.values.set_top(registers.add(window_size + lane_values::CONTEXT));
        Registers(registers)
    };
    // An activation keeping nothing passed its parameters alone, which its window holds
    // (a rest body's rest array among them): its materialization reads them there.
    let record = quench_runtime::staged::call_keeping::KeepingRecord::of(shape, argc);
    let activation = Activation::of_parts(
        target.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc: record.argc,
            completion: if forwarder.is_some() {
                Completion::Forwarder
            } else {
                Completion::of(constructs, cross, false)
            },
            forwards: record.forwards,
            kept: record.kept,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
    if let Some(forwarder) = forwarder {
        stack.attach_payload(SidePayload::Forwarder(forwarder));
    }
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    quench_runtime::staged::call_keeping::enter_context(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()), callee_registers.0.expose_provenance() as u64,
        this.to_bits(), target.env.to_bits(), target.capture.expose_provenance() as u64,
    );
    if cross {
        enter_program_in_lane(cx, target.program());
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    let start = if constructs { base } else { unsafe { (*target.view).start() } };
    Ok((callee_registers, start))
}

/// A frameless call without room on the lane stack: grow the stack, then take the
/// general path, which pushes a frame. Out of line so the activation path saves no
/// registers.
#[cold]
#[inline(never)]
fn script_call_frame_in_lane(ip: Ip, room: usize) -> Ip {
    let cx = context_in_lane();
    let vm = unsafe { vm(cx) };
    let stack = &vm.lane_stack;
    // The callee's own room, which a function of many registers makes larger than a common
    // window: reserving less would send each of its calls at this depth here again.
    let room = room.max(LANE_SCRIPT_WINDOW);
    let full = stack.full(room);
    vm.materialize_lane_activations(cx);
    vm.lane_stack.grow(full, room);
    exit(ip)
}

/// Lane-stack room a frameless call's growth reserves at least: a common window.
const LANE_SCRIPT_WINDOW: usize = 4 * ACTIVATION_FILL_BLOCK;

/// A Wasm view's handler for `op` where it differs from a JavaScript view's: a reference call
/// enters Wasm functions and a return completes a Wasm activation; a value row (a capture
/// access, `this`) reads the context a Wasm view lacks.
fn wasm_lane_handler(instruction: WideInstruction) -> Option<Option<LaneHandler>> {
    let call = LaneHandler { handler: reference_call_handler(), produces: false };
    let ret = LaneHandler { handler: lane_return_wasm, produces: false };
    if matches!(instruction.op().staged(), Some(quench_runtime::bytecode::Decoder::Value)) {
        // A value row reads the receiver and the captures from the context, which a Wasm view lacks.
        return Some(None);
    }
    let op = instruction.op();
    match op {
        _ if quench_runtime::staged::call_family(op) => Some(Some(call)),
        _ if quench_runtime::staged::return_family(op) => Some(Some(ret)),
        _ => None,
    }
}

lane_handler! {
    /// A Wasm view's return: the Wasm activation's completion first, which a Wasm callee
    /// returns to far more often than to a script caller; any other takes
    /// `return_general_in_lane`'s order.
    fn lane_return_wasm<>(cx, r, ip, i, view) {
        if let Some(resumed) = lane_complete_wasm_activation(cx, r.get(i.a)) {
            resume!(cx, resumed)
        }
        sibling!(return_general_in_lane, r, ip, view)
    }
}

return_handler! {
    /// Return from an inline call to its caller; returning from the loop's
    /// entry frame belongs to the general loop.
    /// `LEAF` where the record is a leaf's (`LaneView::specialize_leaf_returns`).
    /// A plain activation's return (call protocol v2 stage C) is this record alone: its header
    /// below `r` names the caller, and nothing else is read (`completions::lane_return_header`, the
    /// first step of `complete_plain_activation_in_lane` for a record of no leaf). Every other
    /// return takes `lane_return_records`, so this handler makes no call.
    fn return_miss<LEAF: u8>(cx, r, ip, i, view) {
        // A leaf's return (`LEAF`) completes the running leaf the same way, one op.
        if let Some((registers, next)) = lane_return_one_op::<LEAF>(cx, r, r.get(i.a)) {
            let carried = Carried { memory: MemoryView::script(cx), integers: quench_runtime::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
            next!(cx, registers, next, carried)
        }
        sibling!(lane_return_records::<LEAF>, r, ip, view)
    }
}

/// The return of the record of form `LEAF` with `value` as one op, making no call
/// (`call_return::step_return_one_op_of`), from the running plain frame's `window`. `None`,
/// which changes nothing, for any other, which `lane_return_records` decides.
#[inline(always)]
fn lane_return_one_op<const LEAF: u8>(cx: *mut LaneContext, window: Registers, value: Value) -> Option<(Registers, Ip)> {
    quench_runtime::staged::call_return::step_return_one_op_of(&mut LaneScriptReturn::<LEAF> { cx, window }, value)
}

return_handler! {
    /// `return_miss` of an activation that is not a plain one past its header: a leaf's, a
    /// record's (`complete_plain_activation_in_lane`), else the returns past the common one.
    #[inline(never)]
    fn lane_return_records<LEAF: u8>(cx, r, ip, i, view) {
        let returned = quench_runtime::staged::call_return::step_activation_return_of(&mut LaneScriptReturn::<LEAF>::running(cx), r.get(i.a));
        if let Some(returned) = returned {
            resume!(cx, completions::resumed_script(cx, returned))
        }
        sibling!(return_general_in_lane, r, ip, view)
    }
}

return_handler! {
    /// `LoadConst` of the register the next record, a `Return`, returns
    /// (`specialize_constant_returns`): the constant's return in one record, as `return;` and
    /// `return null;` compile. Any return past the common one writes the register and takes
    /// the `Return` record's own path.
    fn return_constant_miss<LEAF: u8>(cx, r, ip, i, view) {
        let value = i.constant();
        if let Some(resumed) = complete_plain_activation_in_lane::<LEAF>(cx, value) {
            resume!(cx, resumed)
        }
        r.set(i.a, value);
        sibling!(return_general_in_lane, r, following(ip), view)
    }
}

lane_handler! {
    /// `return_miss` past the common return (`complete_plain_activation_in_lane`): another
    /// program's activation, which a program that calls across programs returns from as
    /// often as the common return; any other completion takes `return_other_in_lane`. Out of
    /// line, so `return_miss` keeps its registers.
    #[inline(never)]
    fn return_general_in_lane<>(cx, r, ip, i, view) {
        let value = r.get(i.a);
        if let Some(resumed) = complete_native_step_in_lane(cx, value) {
            resume!(cx, resumed)
        }
        if let Some(resumed) = lane_complete_wasm_activation(cx, value) {
            resume!(cx, resumed)
        }
        if let Some(resumed) = complete_cross_activation_in_lane(cx, value) {
            resume!(cx, resumed)
        }
        if let Some(resumed) = complete_returning_chain_in_lane(cx, value) {
            resume!(cx, resumed)
        }
        sibling!(return_other_in_lane, r, ip, view)
    }
}

lane_handler! {
    /// `return_general_in_lane` of any other activation's completion, or the frame's return.
    #[inline(never)]
    fn return_other_in_lane<>(cx, r, ip, i, view) {
        if let Some(resumed) = complete_returning_in_lane(cx, ip, r.get(i.a)) {
            resume!(cx, resumed)
        }
        resume!(cx, return_frame_in_lane(r, ip))
    }
}


/// The completion `return_from_in_lane` expects of the activation it returns from.
#[inline(always)]
const fn returning_completion(crosses: bool, constructs: bool, settles: bool) -> Completion {
    if crosses { Completion::Cross } else if constructs { Completion::Constructs } else if settles { Completion::SettlesPromise } else { Completion::Plain }
}

/// `call_return::step_activation_return_of` of the innermost activation, a script one whose call keeps its
/// result: of the caller's program (`Completion::Plain`), or, where `CROSSES`, of another
/// (`Completion::Cross`), which enters the caller's program first, or, where `CONSTRUCTS`, a
/// `new` of its program (`Completion::Constructs`), or, where `SETTLES`, a settling async
/// call (`Completion::SettlesPromise`), whose promise the value resolves and the call yields.
/// `INTO_BODY` admits a returning call too.
#[inline(always)]
fn return_from_in_lane<const CROSSES: bool, const INTO_BODY: bool, const CONSTRUCTS: bool, const SETTLES: bool>(cx: *mut LaneContext, mut value: Value) -> Option<(Registers, Ip)> {
    debug_assert!(unsafe { (*cx).leaf.call }.is_null(), "a leaf's own returns complete it");
    let vm = unsafe { vm(cx) };
    // A resolution that would observe the value returns it from the frame instead.
    let then = if SETTLES { settled_then_in_lane(vm, value)? } else { Value::UNDEFINED };
    // A construction yields its receiver, the activation's `this`, for a non-object result.
    // The construct family's completion row (`staged::call_construct::complete_construct`): the
    // receiver is the returning window's `this` context word, read only for a non-object.
    if CONSTRUCTS {
        let window = unsafe { (*cx).registers }.0 as u64;
        value = Value::from_bits(quench_runtime::staged::call_construct::concrete_complete_construct(window, value.to_bits(), vm.is_object_like(value)));
    }
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // A plain header's own return (`completions::lane_return_header`) ran before this one
    // (`call_return::step_activation_return_of`, `return_to_body_in_lane`): what reaches here is a side record's
    // completion, a returning call's chain or a record.
    // The innermost read where it is, a header with its side record or a record (no spill:
    // one-frame-one-call.md F4).
    let top = stack.top_frame()?;
    let expected = returning_completion(CROSSES, CONSTRUCTS, SETTLES);
    if top.completion() != Some(expected) { return None; }
    // SAFETY: the call record belongs to the caller's live view.
    let destination = unsafe { (*top.call()).a };
    if !INTO_BODY && destination & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        if CROSSES || CONSTRUCTS || SETTLES {
            return None;
        }
        return return_chain_in_lane(cx, value);
    }
    let destination = destination & quench_runtime::bytecode::REGISTER_MASK;
    let (call, caller_registers, caller_base, window) = (top.call(), top.caller_registers(), top.caller_base(), top.window_word());
    let caller_program = top.caller_program();
    // SAFETY: a script activation holds no drop glue and no Wasm storage
    // (`Activation::frame_storage_bytes`); the values hold no drop glue.
    unsafe {
        stack.discard_top_frame();
        stack.values.release_window(window);
    }
    // A return from inside a `with` leaves its scope without an exit.
    stack.drop_with_objects_from(stack.activation_count());
    // The caller's memory view reads its program's records (`MemoryView::script`), which an
    // activation of the caller's own JavaScript program left in place.
    if CROSSES {
        enter_program_in_lane(cx, caller_program);
        unsafe { (*cx).memory = MemoryView::script(cx) };
    }
    debug_assert!(unsafe { (*cx).memory.bytes } == cx.cast::<u8>(), "a JavaScript run carries its context");
    unsafe { (*cx).registers = caller_registers; (*cx).base = caller_base; }
    if SETTLES { value = settle_promise_in_lane(cx, value, then, top.payload().settles()); }
    caller_registers.set(destination, value);
    Some((caller_registers, following(call)))
}

/// `call_return::step_activation_return_of` of a plain activation that returns the completion of its call, and
/// whose callers do in turn (`return f(...)`), down to the plain activation whose call takes
/// `value`: that activation's caller resumes with it, as the run's completion one activation
/// at a time (`complete_plain_chain_in_lane`) makes it. `None`, which changes nothing, for a run
/// another completion ends; the general completions take it.
#[inline(always)]
fn return_chain_in_lane(cx: *mut LaneContext, value: Value) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // The run walked where its frames are, headers with their side records, then records: no
    // spill and no record built (one-frame-one-call.md F4). `above` counts the frames inside
    // the one whose call takes the value.
    let (outer, destination, above) = {
        let mut frames = stack.frames_innermost();
        // The innermost returns its call's completion, which the caller of `return_from_in_lane` proved.
        let innermost = frames.next()?;
        debug_assert!(innermost.completion() == Some(Completion::Plain));
        let mut above = 1;
        loop {
            let frame = frames.next()?;
            if frame.completion() != Some(Completion::Plain) {
                return None;
            }
            // SAFETY: the call record belongs to the caller's live view.
            let destination = unsafe { (*frame.call()).a };
            if destination & quench_runtime::bytecode::RETURN_REGISTER == 0 {
                break (frame, destination, above);
            }
            above += 1;
        }
    };
    let (call, caller_registers, caller_base, window) = (outer.call(), outer.caller_registers(), outer.caller_base(), outer.window_word());
    let index = stack.activation_count() - 1 - above;
    stack.drop_owned_from(index + 1);
    // SAFETY: script activations hold no drop glue or Wasm storage; the values hold no drop
    // glue. The frames from the outer one in go where they are.
    unsafe {
        for _ in 0..=above { stack.discard_top_frame(); }
        stack.values.release_window(window);
        (*cx).registers = caller_registers; (*cx).base = caller_base;
    }
    caller_registers.set(destination & quench_runtime::bytecode::REGISTER_MASK, value);
    Some((caller_registers, following(call)))
}

/// Complete the innermost lane activation with `value`, which the record at `ip` of that
/// activation returns, as a return from it does: its caller resumes after the call with the
/// value in the call's result register, or, for a call that returns its callee's
/// completion, completes in turn. `None` when no activation runs: the running function is a
/// VM frame.
#[inline(always)]
fn complete_activation_in_lane(cx: *mut LaneContext, ip: Ip, mut value: Value) -> Option<Resumed> {
    if let Some(resumed) = complete_plain_chain_in_lane(cx, value) {
        return Some(resumed);
    }
    let vm = unsafe { vm(cx) };
    // The innermost's completion read where it is, a header with its side record or a record:
    // no spill and no record built (one-frame-one-call.md F4).
    let top = vm.lane_stack.top_frame()?;
    let completion = top.completion();
    if completion == Some(Completion::ToWasm) {
        return Some(lane_complete_to_wasm(cx, ip, value));
    }
    if proxy_traps::trap_result_needs_general_path(vm, top.payload(), value) {
        return Some(Resumed::Exit(frame_return(vm, ip, value)));
    }
    // A settling activation whose promise's resolution would observe the value returns it
    // from its frame, whose pending call resolves the promise (`settled_then_in_lane`); else the
    // `then` read here is the one its resolution uses.
    let mut then = Value::UNDEFINED;
    if completion == Some(Completion::SettlesPromise) {
        let Some(read) = settled_then_in_lane(vm, value) else { return Some(Resumed::Exit(frame_return(vm, ip, value))) };
        then = read;
    }
    // Each frame popped where it is (`ActivationView`): a header's pop builds no record.
    let mut frame = vm.lane_stack.pop_activation()?;
    loop {
        let (caller_memory, this) = restore_caller_in_lane(cx, &frame);
        if let Some(completion) = frame.completion() {
            // A native's callback ends its run, whatever the native's own call record is.
            if let Some(ended) = body_calls::ends_run(cx, &frame, completion, this, value) {
                return Some(ended);
            }
            if completion == Completion::ArrayStep {
                return Some(step_array_iteration_in_lane(cx, &frame, value));
            }
            if completion.is_generator() {
                return Some(generator_returned_in_lane(cx, &frame, caller_memory, value));
            }
            if completion == Completion::AsyncRequest {
                return Some(async_requests::request_returned_in_lane(cx, &frame, caller_memory, value));
            }
            // [[Construct]] of an ordinary constructor yields its returned object,
            // otherwise the receiver it allocated. A skipped forwarding constructor returns
            // `undefined` after the method, so its construction yields the receiver.
            // A trap of another program (`AccessorCall::trap_target`) completes as a
            // trap, not as `Completion::Cross`, and leaves its program the same way.
            if completion.crosses() || frame.caller_program() != unsafe { (*cx).program } {
                enter_program_in_lane(cx, frame.caller_program());
            }
            value = setters::completed_value(vm, &frame, completion, this, value);
            if completion == Completion::Forwarder {
                value = Value::from_bits(quench_runtime::staged::call_construct::concrete_forwarder_value(value.to_bits(), this.to_bits()));
            }
            if completion == Completion::SettlesPromise {
                value = settle_promise_in_lane(cx, value, then, frame.payload().settles());
            }
        } else if let Some(result) = frame.export_result() {
            // The export's caller runs its own program again, as the run's JavaScript.
            enter_program_in_lane(cx, frame.caller_program());
            unsafe {
                (*cx).inline_calls = false;
                set_script_calls_in_lane(cx, true);
                (*cx).instance = Value::DELETED;
            }
            value = vm.wasm_export_result(value, result);
        }
        // SAFETY: the call record belongs to the caller's live view.
        if unsafe { (*frame.call()).a } & quench_runtime::bytecode::RETURN_REGISTER == 0 {
            return Some(resume_after_call_in_lane(&frame, caller_memory, value));
        }
        // The call returns its callee's completion: its caller completes in turn, an
        // activation here or the frame in the general loop, which the exit hands it to.
        if caller_returns_in_frame(vm, value, &mut then) {
            return Some(Resumed::Exit(frame_return(vm, frame.call(), value)));
        }
        frame = vm.lane_stack.pop_activation().expect("the caller activation is the innermost");
    }
}

/// The lane's primitives of a Wasm activation's and a JavaScript import's return (`WasmReturn`):
/// `ip` is the returning record.
struct LaneReturn { cx: *mut LaneContext, ip: Ip }

impl quench_runtime::staged::wasm_activation::WasmReturn for LaneReturn {
    type Resume = Resumed;
    type Popped = (ActivationView, MemoryView);
    type Value = Value;
    type Export = Option<quench_runtime::WasmType>;
    type Import = ToWasmCall;
    #[inline(always)]
    fn innermost(&mut self) -> Option<(bool, u64)> {
        let top = unsafe { vm(self.cx) }.lane_stack.top_frame()?;
        // SAFETY: the call record belongs to the caller's live view.
        Some((top.completion().is_some(), u64::from(unsafe { (*top.call()).a })))
    }
    #[inline(always)]
    fn pop(&mut self) -> Self::Popped {
        let frame = unsafe { vm(self.cx) }.lane_stack.pop_activation().expect("the returning activation is the innermost");
        let memory = restore_caller_in_lane(self.cx, &frame).0;
        (frame, memory)
    }
    fn export_result(&mut self, popped: &Self::Popped) -> Option<Self::Export> { popped.0.export_result() }
    #[inline(always)]
    fn enter_script_caller(&mut self, popped: &Self::Popped) {
        enter_program_in_lane(self.cx, popped.0.caller_program());
        unsafe { ((*self.cx).inline_calls, (*self.cx).instance) = (false, Value::DELETED); set_script_calls_in_lane(self.cx, true) };
    }
    fn export_value(&mut self, value: Value, export: Self::Export) -> Value { unsafe { vm(self.cx) }.wasm_export_result(value, export) }
    #[inline(always)]
    fn import(&mut self) -> ToWasmCall {
        unsafe { vm(self.cx) }.lane_stack.top_frame().expect("the import's activation is the innermost").payload().to_wasm()
    }
    #[inline(always)]
    fn import_value(&mut self, call: ToWasmCall, value: Value) -> Option<Value> {
        let vm = unsafe { vm(self.cx) };
        match call.result {
            None => Some(Value::UNDEFINED),
            Some(ty) => value.as_number().and_then(|number| quench_runtime::staged::wasm_activation::import_result(ty, number)).map(|wasm| vm.encode_wasm_value(wasm)),
        }
    }
    #[inline(always)]
    fn enter_wasm_caller(&mut self, call: ToWasmCall, popped: &mut Self::Popped) {
        let cx = self.cx; enter_program_in_lane(cx, popped.0.caller_program());
        let memory = call.memory.map_or(MemoryView::NONE, |binding| {
            let fresh = memory_view(unsafe { vm(cx) }, binding);
            unsafe { (*cx).resolved = (binding, fresh) };
            fresh
        });
        unsafe { ((*cx).inline_calls, (*cx).instance, (*cx).memory) = (true, call.instance, memory); set_script_calls_in_lane(cx, false) };
        popped.1 = memory;
    }
    fn return_from_frame(&mut self, value: Value) -> Resumed { Resumed::Exit(frame_return(unsafe { vm(self.cx) }, self.ip, value)) }
    #[inline(always)]
    fn resume_caller(&mut self, (frame, memory): Self::Popped, value: Value) -> Resumed { resume_after_call_in_lane(&frame, memory, value) }
}

/// The return of a JavaScript import's callee (`Completion::ToWasm`) (`step_import_return_of`).
#[inline(never)]
fn lane_complete_to_wasm(cx: *mut LaneContext, ip: Ip, value: Value) -> Resumed {
    quench_runtime::staged::wasm_activation::step_import_return_of(&mut LaneReturn { cx, ip }, value)
}

/// The return of a Wasm activation whose call keeps its result (`step_wasm_return_of`), else `None`.
#[inline(always)]
fn lane_complete_wasm_activation(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    quench_runtime::staged::wasm_activation::step_wasm_return_of(&mut LaneReturn { cx, ip: std::ptr::null() }, value)
}

/// The value-producing records that may return their result from the frame
/// (`returns_in_lane`), as `returning_miss` parameters.
const RETURNING_FIELD: u8 = 0;
const RETURNING_UNARY: u8 = 1;
const RETURNING_BINARY: u8 = 2;
const RETURNING_OBJECT: u8 = 3;
const RETURNING_LITERAL: u8 = 4;
/// A `RETURNING_FIELD` read of a register or `this` at a site that reads neither `name` nor
/// `length`: the site's primary own entry (`Vm::primary_own_field_hit`), installed by
/// `row_returning_field_first`.
const RETURNING_FIELD_OWN: u8 = 5;
/// `RETURNING_FIELD_OWN` at a site past one shape (`Vm::polymorphic_field_hit`).
const RETURNING_FIELD_POLYMORPHIC: u8 = 6;
/// `RETURNING_FIELD_OWN` at a site whose primary entry names a prototype holder
/// (`Vm::primary_holder_field_hit`).
const RETURNING_FIELD_HOLDER: u8 = 7;
/// `RETURNING_FIELD_OWN` at a site with an inherited entry (`Vm::inherited_field_hit`).
const RETURNING_FIELD_INHERITED: u8 = 8;
/// `RETURNING_FIELD` of a nested site whose steps hit primary own entries
/// (`lane_nested_value`).
const RETURNING_FIELD_NESTED: u8 = 9;
/// A returning `MakeObjectLiteral` (`return { ... }`): the literal the `MakeObjectLiteral` row
/// makes, returned from the frame.
const RETURNING_OBJECT_LITERAL: u8 = 10;
/// `RETURNING_FIELD` of a private name of a register or `this`: the site's private field
/// (`LaneFrame::private_value`); a method, accessor or missing field takes `RETURNING_FIELD`.
const RETURNING_FIELD_PRIVATE: u8 = 11;
/// `RETURNING_FIELD` at a site whose reads met a getter (`FieldSiteTier::Getter`): the getter
/// activation first, as `get_field_getter_miss` makes it, so no cached data probe precedes it.
const RETURNING_FIELD_GETTER: u8 = 12;

/// The cached read of a single-base returning `GetField` of `KIND` (`RETURNING_FIELD_OWN`,
/// `_POLYMORPHIC`, `_HOLDER`, `_INHERITED`) at `cache`: a hit, which runs no guest code.
#[inline(always)]
fn returning_field_hit<const KIND: u8>(
    vm: &mut Vm,
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
unsafe fn call_read_in_lane(
    cx: *mut LaneContext,
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
        let (callee, receiver) = unsafe { ForwardRead::of(target) }?.method(vm, code, arguments, this)?;
        // SAFETY: as `Vm::script_call_entry`; a read resolves no target and collects nothing.
        let inner = unsafe { &*vm.script_call_entry(callee)? };
        if !inner.entry.reads() {
            return None;
        }
        let inner_this = direct_call_this_in_lane(vm, inner, receiver)?;
        // The call passes the parameters in order, so its argument `k` is parameter `k`.
        (inner, arguments.within(target.params), inner_this)
    } else {
        (target, arguments, this)
    };
    match target.entry {
        // The receiver the call binds (`direct_call_this_in_lane`), which is what it returns.
        CallEntry::ReadsThis => Some(this),
        // SAFETY: a capture-read target of the running program.
        CallEntry::ReadsCapture => unsafe { read_capture_in_lane(vm, target) },
        // SAFETY: as for `LeafRead::of`.
        _ => unsafe { LeafRead::of(target) }?.run(vm, code, arguments, this),
    }
}

/// The capture a `CallEntry::ReadsCapture` target's closure environment holds, read as its
/// frameless activation's `LoadCapture` would (`load_capture_walk_miss`); an uninitialized
/// binding misses.
///
/// # Safety
/// `target` is a `CallEntry::ReadsCapture` target of the running program, whose view holds
/// its `LoadCapture` record first.
#[inline(never)]
unsafe fn read_capture_in_lane(vm: &mut Vm, target: &ScriptCallTarget) -> Option<Value> {
    // SAFETY: the program store keeps a derived view alive with its program.
    let imm = unsafe { &*(*target.view).records() }.imm();
    let (depth, slot) = (ImmediateLayout::capture_depth(imm), ImmediateLayout::capture_slot(imm));
    let slots = capture_reads(target.capture);
    if depth == 0 && !slots.is_null() {
        // SAFETY: as `ScriptCallTarget::capture`.
        return unsafe { quench_runtime::tier_surface::EnvironmentSlots::read_storage(slots, slot) };
    }
    let (slots, root) = vm.walked_capture_slots(target.env, false, depth)?;
    // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
    vm.owned_capture(unsafe { &*slots }, root, slot)
}

/// The values a call that a read answers passed (`call_read_in_lane`): parameter `k` is the
/// call window's register `base + k` while `k < passed`, else unpassed. A concrete value,
/// not a closure, so the reads it feeds are one copy whatever forwarded them.
/// `call_read_in_lane` of the call at `site`, memoized when it answers (`note_read_in_lane`). Out
/// of line: the read's calls take stack temporaries, which a handler inlining them would hold
/// on its common path.
///
/// # Safety
/// As `call_read_in_lane`.
#[inline(never)]
unsafe fn answered_call_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the caller's contract.
    let value = unsafe { read_answer_in_lane(cx, target, arguments, this) };
    // A mutating operation must never enter the pure read memo.
    if value.is_some() && target.entry != CallEntry::AppendsCapture {
        note_read_in_lane(cx, site, target, arguments, this);
    }
    value
}

/// The value `target`'s read answers a call with, or what a capture append returns, without the
/// site's memo: what a call whose record has no memo form (a method call's) is answered by.
///
/// # Safety
/// As `call_read_in_lane`.
#[inline(always)]
unsafe fn read_answer_in_lane(
    cx: *mut LaneContext,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    if target.entry == CallEntry::AppendsCapture {
        return unsafe { capture_append::run(cx, target, arguments) };
    }
    unsafe { call_read_in_lane(cx, target, arguments, this) }
}

/// Passed arguments stay in the call window; missing parameters read as absent.
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
/// held by a record prototype (`own_formed_handler`, `lane_get_field_holder`). Each hit is
/// its cache's own validated read, whatever the record's state when it runs.
#[derive(Clone, Copy)]
enum FieldHit {
    Own(u16),
    Holder(u16),
}

/// Every bound own-field read (`own_formed_handler`), of `this` and of a register.
fn own_bound_handlers() -> [*const (); 18] {
    macro_rules! row { ($site:expr) => { generated_row!(FIELD_LEAF_READ, ACC_NONE, $site) as *const () } }
    own_sites!(row)
}

impl FieldHit {
    /// The hit `record`'s quickened handler names; `None` for any other record state.
    #[inline(always)]
    fn of(record: &LaneInstruction) -> Option<Self> {
        let handler = record.handler;
        if own_bound_handlers().contains(&handler)
            || math_field_call_handlers().contains(&handler)
        {
            Some(Self::Own(record.c))
        } else if handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, HELD_SITE_THIS) as *const ()
            || handler == generated_row!(FIELD_LEAF_READ, ACC_NONE, HELD_SITE_REGISTER) as *const ()
        {
            Some(Self::Holder(record.c))
        } else {
            None
        }
    }

    /// The hit on `object`; `None` for a miss.
    #[inline(always)]
    fn run(self, vm: &mut Vm, code: &ResidualProgram, object: Value) -> Option<Value> {
        match self {
            Self::Own(cache) => vm.primary_own_field_hit(code, object, cache),
            Self::Holder(cache) => vm.primary_record_holder_field_hit(code, object, cache),
        }
    }
}

/// The read a `CallEntry::reads` body answers with, by the state its records quickened to.
#[derive(Clone, Copy)]
enum LeafRead {
    /// Its returning `GetField` (`returning_miss` of `kind`) on the object `base` names: the
    /// kind's hit (`returning_field_hit`), or for a holder a record prototype's method read
    /// with no call.
    Field { base: u16, kind: u8, cache: u16 },
    /// Its nested own read (`nested_own_field_hit`) of field site `site`.
    Nested { site: u32 },
    /// Its field record's hit, then the dense element the parameter `index` names.
    Element { base: u16, field: FieldHit, index: u16 },
    /// Its returning private field read (`RETURNING_FIELD_PRIVATE`) of `atom` at its record
    /// `site`, under the home the callee's environment `environment` finds.
    Private { base: u16, atom: quench_runtime::bytecode::Atom, site: usize, environment: Value, function: u32 },
}

impl LeafRead {
    /// The read `target`'s records answer with now; `None` for a record state no read hits.
    ///
    /// # Safety
    /// `target` is a `CallEntry::reads` target of the running program, whose view holds its
    /// returning record first and, for an element read, its `GetIndex` record after it.
    #[inline(always)]
    unsafe fn of(target: &ScriptCallTarget) -> Option<Self> {
        // SAFETY: the program store keeps a derived view alive with its program.
        let entry = unsafe { &*(*target.view).records() };
        if target.entry == CallEntry::ReadsElement {
            // SAFETY: as the function's contract.
            let element = unsafe { &*(*target.view).records().add(1) };
            return Some(Self::Element { base: entry.b, field: FieldHit::of(entry)?, index: element.c });
        }
        let handler = entry.handler;
        if handler == returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_NESTED, NOT_LEAF) as *const () {
            return Some(Self::Nested { site: entry.imm() });
        }
        if handler == returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_PRIVATE, NOT_LEAF) as *const () {
            return Some(Self::Private {
                base: entry.b,
                atom: entry.imm(),
                site: std::ptr::from_ref(entry).addr(),
                environment: target.env,
                function: target.function,
            });
        }
        let kind = if handler == returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_POLYMORPHIC, NOT_LEAF) as *const () {
            RETURNING_FIELD_POLYMORPHIC
        } else if returning_own_row(handler) || handler == returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_OWN, NOT_LEAF) as *const () {
            RETURNING_FIELD_OWN
        } else if handler == returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_HOLDER, NOT_LEAF) as *const () {
            RETURNING_FIELD_HOLDER
        } else if handler == returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_INHERITED, NOT_LEAF) as *const () {
            RETURNING_FIELD_INHERITED
        } else {
            return None;
        };
        Some(Self::Field { base: entry.b, kind, cache: entry.c })
    }

    /// The read's value for a call passing `arguments` on `this`; `None` for a miss.
    #[inline(always)]
    fn run(
        self,
        vm: &mut Vm,
        code: &ResidualProgram,
        arguments: ReadArguments,
        this: Value,
    ) -> Option<Value> {
        let base = |base: u16| read_base_in_lane(quench_runtime::bytecode::FieldBase(base), arguments, this);
        match self {
            Self::Element { base: field_base, field, index } => {
                let array = field.run(vm, code, base(field_base)?)?;
                vm.direct_dense_element(array, arguments.get(index)?)
            }
            Self::Nested { site } => {
                // SAFETY: a nested record of the running program names its site.
                let site = unsafe { code.field_sites.get_unchecked(site as usize) };
                nested_own_field_hit(vm, code, site, read_base_in_lane(site.base, arguments, this)?)
            }
            // The callee runs frameless, capturing from its closure's environment.
            Self::Private { base: field_base, atom, site, environment, function } => {
                vm.site_private_field(code, site, (environment, function, false), base(field_base)?, atom)
            }
            Self::Field { base: field_base, kind, cache } => {
                let object = base(field_base)?;
                match kind {
                    RETURNING_FIELD_POLYMORPHIC => {
                        returning_field_hit::<RETURNING_FIELD_POLYMORPHIC>(vm, code, object, cache)
                    }
                    RETURNING_FIELD_OWN => returning_field_hit::<RETURNING_FIELD_OWN>(vm, code, object, cache),
                    // A record prototype's method, read with no call; any other declines the
                    // read, and the activation's own record reads it.
                    RETURNING_FIELD_HOLDER => vm.primary_record_holder_field_hit(code, object, cache),
                    _ => returning_field_hit::<RETURNING_FIELD_INHERITED>(vm, code, object, cache),
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
    unsafe fn of(target: &ScriptCallTarget) -> Option<Self> {
        // SAFETY: the program store keeps a derived view alive with its program.
        let (field, method) = unsafe {
            let records = (*target.view).records();
            (&*records, &*records.add(1))
        };
        Some(Self { base: field.b, field: FieldHit::of(field)?, method: FieldHit::of(method)? })
    }

    /// The method the body calls and its receiver, for a call passing `arguments` on `this`.
    #[inline(always)]
    fn method(
        self,
        vm: &mut Vm,
        code: &ResidualProgram,
        arguments: ReadArguments,
        this: Value,
    ) -> Option<(Value, Value)> {
        let base = read_base_in_lane(quench_runtime::bytecode::FieldBase(self.base), arguments, this)?;
        let receiver = self.field.run(vm, code, base)?;
        Some((self.method.run(vm, code, receiver)?, receiver))
    }
}

use quench_runtime::vm::engine_types::MemoLeaf;

/// The memo shapes `script_call_memo_miss` takes as its parameter: a leaf's step
/// (`MemoLeaf::shape`), with `MEMO_FORWARD` set for a leaf a forwarding body reaches
/// (`MemoRead::Forward`), so a site's handler makes its memo's steps with no dispatch on them.
const MEMO_FIELD: u8 = 0;
const MEMO_ELEMENT: u8 = 1;
const MEMO_LENGTH: u8 = 2;
const MEMO_NESTED: u8 = 3;
const MEMO_FORWARD: u8 = 4;

#[quench_macros::ext(MemoLeafOps1)]
impl MemoLeaf {
    /// Its step's memo shape.
    fn shape(&self) -> u8 {
        match self {
            Self::Field { .. } => MEMO_FIELD,
            Self::Element { .. } => MEMO_ELEMENT,
            Self::Length { .. } => MEMO_LENGTH,
            Self::Nested { .. } => MEMO_NESTED,
        }
    }

    /// The memo form of `read`, whose hits the active program's caches just answered; `None`
    /// for a polymorphic or inherited read.
    fn of(vm: &Vm, code: &ResidualProgram, read: LeafRead) -> Option<Self> {
        let entry = |hit: FieldHit| match hit {
            FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
        };
        Some(match read {
            LeafRead::Field { base, kind: RETURNING_FIELD_OWN | RETURNING_FIELD_HOLDER, cache } => {
                Self::Field { base, field: vm.field_cache_entry(cache) }
            }
            LeafRead::Field { .. } => return None,
            // The memo holds for its callee alone, whose environment fixes the home, so the
            // site's shape and slot under it are a plain own field step.
            LeafRead::Private { base, site, environment, .. } => {
                Self::Field { base, field: vm.site_private_field_entry(site, environment)? }
            }
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

    /// This memo with its steps as `Vm::formed_entry` makes them of the objects a call passing
    /// `arguments` on `this` reads, which its steps just answered: each step then proves its
    /// shapes and storage forms by whole-word compares (`Vm::record_field_formed_hit`).
    fn formed(self, vm: &Vm, arguments: ReadArguments, this: Value) -> Self {
        let base_of = |base: u16| read_base_in_lane(quench_runtime::bytecode::FieldBase(base), arguments, this);
        let formed = |field: quench_runtime::vm::FieldCache, base: u16| base_of(base).map_or(field, |object| vm.formed_entry(field, object));
        match self {
            Self::Field { base, field } => Self::Field { base, field: formed(field, base) },
            Self::Element { base, field, index } => Self::Element { base, field: formed(field, base), index },
            Self::Length { base, field } => Self::Length { base, field: formed(field, base) },
            Self::Nested { base, field, second } => {
                let object = base_of(base).and_then(|object| vm.record_field_entry_hit(field, object));
                let second = object.map_or(second, |object| vm.formed_entry(second, object));
                Self::Nested { base, field: formed(field, base), second }
            }
        }
    }

    /// `LeafRead::run` of the read this memo records, of the step `shape` names (a constant
    /// where it inlines); `None` for a memo of another step.
    #[inline(always)]
    fn run(
        &self,
        shape: u8,
        vm: &Vm,
        arguments: ReadArguments,
        this: Value,
        this_record: Option<&quench_runtime::tier_surface::Object>,
    ) -> Option<Value> {
        match *self {
            Self::Field { base: field_base, field } if shape == MEMO_FIELD => memo_first_step(vm, field, field_base, arguments, this, this_record),
            Self::Element { base: field_base, field, index } if shape == MEMO_ELEMENT => {
                let array = memo_first_step(vm, field, field_base, arguments, this, this_record)?;
                vm.direct_dense_element(array, arguments.get(index)?)
            }
            Self::Length { base: field_base, field } if shape == MEMO_LENGTH => {
                let array = memo_first_step(vm, field, field_base, arguments, this, this_record)?;
                vm.array_length_field(array, vm.length_atom)
            }
            Self::Nested { base: field_base, field, second } if shape == MEMO_NESTED => {
                #[cfg(feature = "profile-field-primitives")]
                vm.profile.field_primitives.hit(quench_runtime::profile::field_primitives::Path::MemoPairAttempt);
                let object = memo_first_step(vm, field, field_base, arguments, this, this_record)?;
                #[cfg(feature = "profile-field-primitives")]
                vm.profile.field_primitives.hit(quench_runtime::profile::field_primitives::Path::MemoPairFirstHit);
                let value = vm.record_field_formed_hit(second, object);
                #[cfg(feature = "profile-field-primitives")]
                if value.is_some() { vm.profile.field_primitives.hit(quench_runtime::profile::field_primitives::Path::MemoPairHit); }
                value
            }
            _ => None,
        }
    }
}

/// A memo leaf's first step (`MemoLeaf::run`): `field` of the object `base` names, read
/// from `this`'s record where the caller holds it (a forwarding body's receiver, already
/// proven a direct object).
#[inline(always)]
fn memo_first_step(
    vm: &Vm,
    field: quench_runtime::vm::FieldCache,
    base: u16,
    arguments: ReadArguments,
    this: Value,
    this_record: Option<&quench_runtime::tier_surface::Object>,
) -> Option<Value> {
    match this_record {
        Some(record) if base == quench_runtime::bytecode::FieldBase::THIS.0 => vm.record_field_formed_hit_in(field, record),
        _ => vm.record_field_formed_hit(field, read_base_in_lane(quench_runtime::bytecode::FieldBase(base), arguments, this)?),
    }
}

use quench_runtime::vm::engine_types::MemoRead;

#[quench_macros::ext(MemoReadOps1)]
impl MemoRead {
    /// Its memo shape (`MEMO_FIELD` .. `MEMO_FORWARD`).
    fn shape(&self) -> u8 {
        match self {
            Self::Leaf(read) => read.shape(),
            Self::Forward { read, .. } => MEMO_FORWARD | read.shape(),
        }
    }
}

/// The `script_call_memo_miss` instantiation of memo shape `shape` (`MemoRead::shape`).
fn script_call_memo_handler(shape: u8) -> Handler {
    const _: () = assert!(MEMO_NESTED < MEMO_FORWARD);
    macro_rules! shapes {
        ($($shape:expr),*) => {
            match shape {
                $(shape if shape == $shape => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::FRAME_CALL | ($shape as u16) }>(),)*
                _ => unreachable!("a memo shape is a leaf step, forwarded or not"),
            }
        };
    }
    shapes!(
        MEMO_FIELD,
        MEMO_ELEMENT,
        MEMO_LENGTH,
        MEMO_NESTED,
        MEMO_FORWARD | MEMO_FIELD,
        MEMO_FORWARD | MEMO_ELEMENT,
        MEMO_FORWARD | MEMO_LENGTH,
        MEMO_FORWARD | MEMO_NESTED
    )
}

use quench_runtime::vm::engine_types::ReadMemo;

/// Slots of the read memo table (`LaneStack::read_memos`): it bounds the table's memory, and a
/// table that fills starts again empty, so the sites that read since make their memos again.
const READ_MEMOS: usize = 4096;

/// The memo the call record `site` made, in the slot it names (`CallOperand::memo`); `None`
/// before its first, or once the table refilled and gave the slot to another site.
#[inline(always)]
fn site_read_memo<'a>(memos: &'a [ReadMemo], site: &LaneInstruction) -> Option<&'a ReadMemo> {
    // SAFETY: a memo's site is a call record (`Op::makes_script_call`), whose operand is `call`.
    let slot = unsafe { site.operand.call.memo };
    memos.get(slot as usize).filter(|held| std::ptr::eq(held.site, site))
}

/// Records `target`'s read at the call record `site` (`memoize_read_in_lane`) unless the site's
/// own memo of this generation missed, which keeps the full form until the generation ends.
#[inline(always)]
fn note_read_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    let stack = unsafe { &vm(cx).lane_stack };
    // SAFETY: `site` is a call record of the running view.
    if site_read_memo(&stack.read_memos, unsafe { &*site }).is_none_or(|held| held.admission.generation() != stack.generation) {
        memoize_read_in_lane(cx, site, target, arguments, this);
    }
}

/// The memo of the read `target` just answered at `site` for `arguments` and `this`, and the
/// site's memo form (`script_call_memo_miss`); nothing for an arrow's own `this`, which the
/// memo does not hold, or a read whose records no longer hit.
#[cold]
#[inline(never)]
fn memoize_read_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    // The memo answers the site from now on, so no call of it links its record: its record
    // names the callee for the bodies that inline it (`site_targets_in_lane`).
    // SAFETY: `site` is a record of the running view.
    vm.note_site_callee(unsafe { &(*site).call_link }, crate::compiled::CallTarget {
        callee: crate::compiled::CalleeBinding::Identity(target.callee), function: target.function });
    if target.this_binding == ThisBinding::Lexical {
        return;
    }
    let read = if target.entry == CallEntry::ForwardsRead {
        // SAFETY: as for `ForwardRead::of`.
        let Some(forward) = (unsafe { ForwardRead::of(target) }) else {
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
        let Some(read) = (unsafe { LeafRead::of(inner) }) else {
            return;
        };
        let entry = |hit: FieldHit| match hit {
            FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
        };
        let Some(leaf) = MemoLeaf::of(vm, code, read) else {
            return;
        };
        // The steps as whole-word compares of the objects this call read (`MemoLeaf::formed`).
        let (field, method_field) = (entry(forward.field), entry(forward.method));
        let base = read_base_in_lane(quench_runtime::bytecode::FieldBase(forward.base), arguments, this);
        let receiver = base.and_then(|base| vm.record_field_entry_hit(field, base));
        let read = match receiver.and_then(|receiver| receiver_this_in_lane(vm, inner.this_binding, receiver)) {
            Some(method_this) => leaf.formed(vm, arguments.within(target.params), method_this),
            None => leaf,
        };
        MemoRead::Forward {
            base: forward.base,
            field: base.map_or(field, |base| vm.formed_entry(field, base)),
            method_field: receiver.map_or(method_field, |receiver| vm.formed_entry(method_field, receiver)),
            params: target.params,
            method,
            read,
        }
    } else {
        // SAFETY: as for `LeafRead::of`.
        let Some(read) = (unsafe { LeafRead::of(target) }) else {
            return;
        };
        match MemoLeaf::of(vm, code, read) {
            Some(read) => MemoRead::Leaf(read.formed(vm, arguments, this)),
            None => return,
        }
    };
    let handler = script_call_memo_handler(read.shape());
    let memo = ReadMemo {
        site,
        callee: target.callee,
        admission: vm.lane_stack.current_admission(),
        this_binding: target.this_binding,
        read,
    };
    let memos = &mut vm.lane_stack.read_memos;
    // SAFETY: `site` is a call record (`Op::makes_script_call`) of the running view, whose operand is
    // `call`, written as `requicken` writes it.
    unsafe {
        if site_read_memo(memos, &*site).is_some() {
            memos[(*site).operand.call.memo as usize] = memo;
        } else {
            if memos.len() == READ_MEMOS {
                memos.clear();
            }
            (*site.cast_mut()).operand.call.memo = memos.len() as u32;
            memos.push(memo);
        }
    }
    requicken(site, handler);
}

/// The value the memo of the call record `i` answers its call with (`ReadMemo`); `None`
/// where the memo is another site's or stale, the call is another callee's or a construction's
/// (`constructing_in_lane`), or a hit misses.
///
/// # Safety
/// `cx` is the running lane's context and `i` its call record.
#[inline(always)]
unsafe fn memo_read_in_lane<const SHAPE: u8>(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let vm: &Vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    let held = site_read_memo(&vm.lane_stack.read_memos, i)?;
    if held.callee != callee
        // A pending construction refuses every memo (`LaneStack::set_refused`).
        || held.admission != vm.lane_stack.admission
    {
        return None;
    }
    // A memo's steps read `this` only as a field base, which hits on a direct object alone: the
    // receiver binds itself then (`receiver_this_in_lane`), and any other receiver, whatever it
    // binds, misses the step as its binding would, so the receiver stands for `this` untested.
    if held.this_binding == ThisBinding::Lexical {
        return None;
    }
    let this = r.get(i.c);
    let arguments = ReadArguments {
        registers: r,
        base: ImmediateLayout::call_window_base(i.imm()),
        passed: ImmediateLayout::argument_count(i.imm()),
    };
    let leaf = SHAPE & !MEMO_FORWARD;
    match &held.read {
        MemoRead::Leaf(read) if SHAPE & MEMO_FORWARD == 0 => read.run(leaf, vm, arguments, this, None),
        MemoRead::Forward { base, field, method_field, params, method, read }
            if SHAPE & MEMO_FORWARD != 0 =>
        {
            let base = read_base_in_lane(quench_runtime::bytecode::FieldBase(*base), arguments, this)?;
            let receiver = vm.record_field_formed_hit(*field, base)?;
            let record = vm.heap_reads().object_record_by_id(receiver.direct_object_index()?);
            if vm.record_field_formed_hit_in(*method_field, record)? != *method {
                return None;
            }
            // A memo's method binds no lexical `this` (`memoize_read_in_lane`), so the receiver, a
            // direct object, is its `this` (`receiver_this_in_lane`), and a step on `this` reads
            // its record.
            // The call passes the parameters in order, so its argument `k` is parameter `k`.
            read.run(leaf, vm, arguments.within(*params), receiver, Some(record))
        }
        _ => None,
    }
}

script_handler! {
    /// `script_call_full_miss` at a site whose read call made a memo (`memoize_read_in_lane`): the
    /// memo's hits, of its shape `SHAPE` (`MemoRead::shape`), answer the call as the read
    /// target's would. A call the memo does not answer takes the full form, which the site
    /// then keeps; a run that admits no read call (`returning_call_admitted`) takes it
    /// once.
    fn script_call_memo_miss<SHAPE: u8>(cx, r, ip, i, view) {
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        // A run that admits no read call leaves the memo to the next.
        if !unsafe { (*cx).script_calls } || (returns && !returning_call_admitted(cx)) {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        }
        let Some(value) = (unsafe { memo_read_in_lane::<SHAPE>(cx, r, i) }) else {
            requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>());
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        };
        if !returns {
            r.set(i.a, value);
            next!(cx, r, following(ip), Carried::script_in(cx, view))
        }
        lane_return_value!(cx, ip, value)
    }
}

/// The object a read body's field record names: `this`, or the parameter its activation
/// would hold in that register; `None` for an unpassed one, whose `undefined` no read hits.
#[inline(always)]
fn read_base_in_lane(
    base: quench_runtime::bytecode::FieldBase,
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

/// The `returning_miss` kind of a value record that returns its result from the frame.
fn returning_kind(instruction: WideInstruction) -> Option<u8> {
    if !instruction.returns_from_frame() {
        return None;
    }
    let traits = instruction.op().traits();
    if traits.has(Traits::RETURNING_FIELD) { Some(RETURNING_FIELD) }
    else if traits.has(Traits::RETURNING_UNARY) { Some(RETURNING_UNARY) }
    else if traits.has(Traits::RETURNING_BINARY) { Some(RETURNING_BINARY) }
    else if traits.has(Traits::RETURNING_OBJECT) { Some(RETURNING_OBJECT) }
    else if traits.has(Traits::RETURNING_OBJECT_LITERAL) { Some(RETURNING_OBJECT_LITERAL) }
    else if traits.has(Traits::RETURNING_LITERAL) { Some(RETURNING_LITERAL) }
    else { None }
}

script_handler! {
    /// A value record that returns its result from the frame (`returning_kind`): the value
    /// as the record's own handler computes it, then the activation's completion, or the VM
    /// frame's return of it from the general loop (`frame_return`). A value the
    /// handler does not compute takes the general path.
    fn returning_miss<KIND: u8, LEAF: u8>(cx, r, ip, i, view) {
        let value = unsafe {
            match KIND {
                RETURNING_FIELD_OWN
                | RETURNING_FIELD_POLYMORPHIC
                | RETURNING_FIELD_HOLDER
                | RETURNING_FIELD_INHERITED => {
                    let code = &*(*cx).code;
                    let vm = vm(cx);
                    let value = single_field_base_in_lane(cx, r, i.b).and_then(|object| {
                        if KIND == RETURNING_FIELD_POLYMORPHIC {
                            polymorphic_read(vm, code, ip, object).map(|(value, _)| value)
                        } else {
                            returning_field_hit::<KIND>(vm, code, object, i.c)
                        }
                    });
                    let Some(value) = value else {
                        if KIND == RETURNING_FIELD_POLYMORPHIC {
                            sibling!(returning_miss::<RETURNING_FIELD, LEAF>, r, ip, Carried::script_in(cx, view))
                        }
                        sibling!(row_returning_field_miss::<LEAF>, r, ip, Carried::script_in(cx, view))
                    };
                    Some(value)
                }
                RETURNING_FIELD_PRIVATE => {
                    let code = &*(*cx).code;
                    let value = field_base_in_lane(cx, r, quench_runtime::bytecode::FieldBase(i.b))
                        .and_then(|object| lane_frame::LaneFrame::<ACC_NONE>::private_value(cx, ip.addr(), code, object, i.imm()));
                    let Some(value) = value else {
                        sibling!(returning_miss::<RETURNING_FIELD, LEAF>, r, ip, Carried::script_in(cx, view))
                    };
                    Some(value)
                }
                RETURNING_FIELD_NESTED => {
                    let Some(value) = lane_nested_value(cx, r, ip, i) else {
                        sibling!(returning_miss::<RETURNING_FIELD, LEAF>, r, ip, Carried::script_in(cx, view))
                    };
                    Some(value)
                }
                // A read that meets its site's getter activates it, as `lane_get_field`
                // does: the record returns from the frame, so the getter's completion
                // completes this activation too (`complete_activation_in_lane`).
                RETURNING_FIELD_GETTER => {
                    return match getter_activation_in_lane(cx, r, ip, i) {
                        Ok((registers, base)) => next!(cx, registers, base, Carried::script(cx)),
                        // A native getter that raised is not called again: the general path raises it.
                        Err(next) if (vm(cx)).lane_completion.is_raised() => next,
                        Err(_) => sibling!(returning_miss::<RETURNING_FIELD, LEAF>, r, ip, Carried::script(cx)),
                    };
                }
                RETURNING_FIELD => match field_value_in_lane(cx, r, i) {
                    Some(value) => Some(value),
                    None => {
                        return match getter_activation_in_lane(cx, r, ip, i) {
                            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
                            Err(next) => next,
                        };
                    }
                },
                // An operation the lane's values decline is made in place; a raise goes on
                // where `unwound_in_place` unwound it.
                RETURNING_UNARY | RETURNING_BINARY => {
                    let unary = KIND == RETURNING_UNARY;
                    let own = if unary { unary_value(cx, r, i) } else { returning_binary_value_in_lane(cx, r, i) };
                    match own.map(Ok).unwrap_or_else(|| operation_in_place(cx, ip, r, i, unary)) {
                        Ok(value) => Some(value),
                        Err(next) => return next,
                    }
                }
                RETURNING_OBJECT => Some(object_pair_in_lane(cx, r, i.imm(), [i.b, i.c])),
                RETURNING_OBJECT_LITERAL => Some(object_literal_value_in_lane(cx, r, i.imm(), literal_window_in_lane(i))),
                _ => literal_value_in_lane(cx, r, i),
            }
        };
        let Some(value) = value else {
            return exit(ip);
        };
        lane_return_value!(cx, ip, value, LEAF)
    }
}

/// The value of a `Unary` (`unary`) or `Binary` record whose operands the lane's own values
/// decline, made by the general operation in place (`unwound_in_place`): it may reach guest
/// code (an object's `ToPrimitive` hook) or throw, which is raised at the record. `Err` is the
/// record the run goes on at: the handler a throw entered, or the general path, also for an
/// operand the lane cannot read. Out of line, so the returning handlers keep their code.
#[cold]
#[inline(never)]
fn operation_in_place(cx: *mut LaneContext, ip: Ip, r: Registers, i: &LaneInstruction, unary: bool) -> Result<Value, Ip> {
    let operator = i.imm();
    if unary {
        let operand = r.get(i.b);
        return unwound_in_place(cx, ip, |vm, code| vm.unary(code, operator, operand));
    }
    let (Some(left), Some(right)) = (operand_value_in_lane(cx, r, i.b), operand_value_in_lane(cx, r, i.c)) else {
        return Err(exit(ip));
    };
    unwound_in_place(cx, ip, |vm, code| vm.binary(code, operator, left, right))
}

script_handler! {
    /// A returning `Binary` of arithmetic operator `OP` on a register and a register or, where
    /// `CONSTANT`, a constant, resolved when the view was derived
    /// (`specialize_register_operands`) as `lane_operator` resolves a non-returning one: two
    /// integers decide it here with no dispatch on the operator, any other operands take
    /// `returning_operator_other_in_lane`.
    fn returning_operator_miss<OP: u32, CONSTANT: bool, LEAF: u8>(cx, r, ip, i, view) {
        let right = if CONSTANT { i.constant() } else { r.get(i.c) };
        let Some(value) = quench_runtime::vm::operations::integer_operator::<OP>(r.get(i.b), right) else {
            sibling!(returning_operator_other_in_lane::<OP, CONSTANT, LEAF>, r, ip, Carried::script_in(cx, view))
        };
        lane_return_value!(cx, ip, value, LEAF)
    }
}

/// The protocol miss body of a returning constant-right `Binary` of operator `operator` (a
/// `BinaryOperator`'s value) at leaf form `LEAF` (`protocol_misses::returns`): `returning_operator_miss`
/// of that operator; `None` for a value no arithmetic operator has.
const fn returning_operator_miss_of<const LEAF: u8>(operator: u8) -> Option<Handler> {
    macro_rules! pick {
        ($op:expr) => {
            returning_operator_miss::<{ $op }, true, LEAF> as Handler
        };
    }
    arithmetic_operators!(operator as u32, pick)
}

script_handler! {
    /// `returning_operator_miss` of operands other than two integers: other Numbers, then the
    /// concatenation and BigInt values `returning_binary_value_in_lane` decides; any other takes the
    /// general path. Out of line, so the integer case keeps few values live.
    #[inline(never)]
    fn returning_operator_other_in_lane<OP: u32, CONSTANT: bool, LEAF: u8>(cx, r, ip, i, view) {
        let (left, right) = (r.get(i.b), if CONSTANT { i.constant() } else { r.get(i.c) });
        let value = quench_runtime::vm::operations::number_value_operator::<OP>(left, right)
            .or_else(|| (OP == quench_runtime::vm::operations::ADDITION).then(|| concat_value(cx, left, right)).flatten())
            .or_else(|| bigint_value(cx, OP, left, right));
        let Some(value) = value else {
            return exit(ip);
        };
        lane_return_value!(cx, ip, value, LEAF)
    }
}

script_handler! {
    /// A returning `GetField`'s first run: a single read that names neither `name` nor
    /// `length` (`Vm::reads_function_info`) takes `RETURNING_FIELD_OWN`; any other keeps
    /// `RETURNING_FIELD`, whose cached read prepares them. Every kind keeps the record's leaf
    /// form (`LEAF`, `LaneView::specialize_leaf_returns`).
    #[inline(never)]
    fn row_returning_field_first<LEAF: u8>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let handler = returning_field_first_kind::<LEAF>(unsafe { vm(cx) }, unsafe { &*(*cx).code }, i);
        requicken(ip, handler);
        sibling!(handler, r, ip, Carried::script_in(cx, view))
    }
}

/// The kind a returning `GetField` takes on its first run (`row_returning_field_first`),
/// which its record and site alone decide.
fn returning_field_first_kind<const LEAF: u8>(
    vm: &Vm,
    code: &ResidualProgram,
    i: &LaneInstruction,
) -> Handler {
    if i.b != quench_runtime::bytecode::FieldBase::NESTED && vm.is_private_name(i.imm()) {
        return field_return_row::<LEAF>(RETURNING_FIELD_PRIVATE);
    }
    if is_single_field_base(i.b) && !vm.reads_function_info(i.imm()) {
        by_leaf!(LEAF, |L| single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(true, L)),
            generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(false, L)),
            returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, L) as Handler))
    } else if field_site_tier_in_lane(vm, code, i) == FieldSiteTier::Nested {
        field_return_row::<LEAF>(RETURNING_FIELD_NESTED)
    } else {
        field_return_row::<LEAF>(RETURNING_FIELD)
    }
}

/// The protocol row of a returning `GetField` of `kind` (a `RETURNING_FIELD*` kind) at leaf form
/// `LEAF`: its RETURN code (`returning_code`), which `returning_miss` completes.
fn field_return_row<const LEAF: u8>(kind: u8) -> Handler {
    by_leaf!(LEAF, |L| match kind {
        RETURNING_FIELD => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, L) as Handler,
        RETURNING_FIELD_OWN => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_OWN, L) as Handler,
        RETURNING_FIELD_POLYMORPHIC => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_POLYMORPHIC, L) as Handler,
        RETURNING_FIELD_HOLDER => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_HOLDER, L) as Handler,
        RETURNING_FIELD_INHERITED => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_INHERITED, L) as Handler,
        RETURNING_FIELD_NESTED => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_NESTED, L) as Handler,
        RETURNING_FIELD_PRIVATE => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_PRIVATE, L) as Handler,
        RETURNING_FIELD_GETTER => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_GETTER, L) as Handler,
        _ => unreachable!("a returning GetField's kind is a field kind"),
    })
}

script_handler! {
    /// A miss of a single-entry returning read (`RETURNING_FIELD_OWN`, `_HOLDER`,
    /// `_INHERITED`): the record moves to the kind its site's tier calls for
    /// (`field_site_tier_in_lane`), as `row_get_field_miss` moves a plain read; the read itself
    /// takes `RETURNING_FIELD`.
    #[inline(never)]
    fn row_returning_field_miss<LEAF: u8>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        // The row's own hint is stale: the primary own entry answers as the single-entry kind did,
        // and a direct object's hit rebinds the hint the row reads.
        if let Some(object) = unsafe { single_field_base_in_lane(cx, r, i.b) } {
            let vm = unsafe { vm(cx) };
            if let Some(value) = vm.primary_own_field_hit(code, object, i.c) {
                if object.direct_object_index().is_some() {
                    set_field_hint(ip, vm.field_cache_entry(i.c));
                }
                lane_return_value!(cx, ip, value, LEAF);
            }
        }
        // The single-entry tiers read the object records alone; a receiver in a heap cell
        // keeps the general cached read.
        let cell_receiver = unsafe { single_field_base_in_lane(cx, r, i.b) }
            .is_some_and(|object| object.direct_object_index().is_none());
        let handler: Option<Handler> = match field_site_tier_in_lane(unsafe { vm(cx) }, code, i) {
            // A local base has no single-base kind (`is_single_field_base`).
            tier if tier != FieldSiteTier::Nested && !is_single_field_base(i.b) => {
                Some(field_return_row::<LEAF>(RETURNING_FIELD))
            }
            FieldSiteTier::Own | FieldSiteTier::Holder | FieldSiteTier::Inherited if cell_receiver => {
                Some(field_return_row::<LEAF>(RETURNING_FIELD))
            }
            FieldSiteTier::Nested => Some(field_return_row::<LEAF>(RETURNING_FIELD_NESTED)),
            FieldSiteTier::General => Some(field_return_row::<LEAF>(RETURNING_FIELD)),
            FieldSiteTier::Getter => Some(field_return_row::<LEAF>(RETURNING_FIELD_GETTER)),
            FieldSiteTier::Polymorphic => Some(field_return_row::<LEAF>(RETURNING_FIELD_POLYMORPHIC)),
            FieldSiteTier::Holder => Some(field_return_row::<LEAF>(RETURNING_FIELD_HOLDER)),
            FieldSiteTier::Inherited => Some(field_return_row::<LEAF>(RETURNING_FIELD_INHERITED)),
            FieldSiteTier::Own => None,
        };
        if let Some(handler) = handler {
            requicken(ip, handler);
        }
        sibling!(returning_miss::<RETURNING_FIELD, LEAF>, r, ip, Carried::script_in(cx, view))
    }
}

script_handler! {
    /// A JavaScript `Call` of a user function that needs a VM frame (no frameless target,
    /// `Vm::resolve_script_call_target`): the frame and pending call the general loop's
    /// in-loop call makes (`Vm::push_in_loop_call`), entered in place. Lane activations become
    /// frames first, as leaving the lane would make them (`materialize_lane_activations`).
    /// Any other call takes the general path, and the record resolves anew on its next run.
    #[inline(never)]
    fn frame_call_in_lane<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let callee = r.get(i.b);
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        let target = vm.in_loop_call_target(code, callee);
        let admitted = |target: &(ProgramId, u32, Value)| {
            // SAFETY: the context outlives the lane run.
            let script_calls = unsafe { (*cx).script_calls };
            script_calls && (!returns || returning_call_admitted(cx))
        };
        let Some(target) = target.filter(admitted) else {
            if let Some(resumed) = start_async_call_in_lane(cx, r, ip, i, callee)
                .or_else(|| generator_windows::start_called_generator(cx, r, ip, i, callee))
            {
                resume!(cx, resumed)
            }
            requicken(ip, script_call_handler(i.imm()));
            return exit(ip);
        };
        resume!(cx, push_frame_call_in_lane(cx, r, ip, i, target, callee, r.get(i.c), FrameCallArguments::Window(0)))
    }
}

/// `frame_call_in_lane` of an async function the general loop would push as a settling call
/// (`Vm::in_loop_settling_target`): the callee starts as a job (`Vm::start_async_call`) beside
/// the caller's activations, as a native that runs beside them does (`call_native_out_in_lane`),
/// so its first `await` suspends its window in place and the caller continues in the lane with
/// the promise. `None` leaves the call to the general path: a call that completes its caller,
/// or a callee of another program or not yet derived.
#[inline(never)]
fn start_async_call_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction, callee: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if !unsafe { (*cx).script_calls } || i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let (program, function, env) = vm.in_loop_settling_target(code, callee)?;
    if program != unsafe { (*cx).program } || !vm.call_may_derive_in_lane(program, function) {
        return None;
    }
    let window = usize::from(ImmediateLayout::call_window_base(i.imm()));
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    // SAFETY: the call's window is registers of the running function; the values are copied
    // before any activation becomes a frame.
    let arguments = quench_runtime::vm::call_arguments::CallArguments::from_slice(unsafe { std::slice::from_raw_parts(r.0.add(window), argc) });
    let this = r.get(i.c);
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    let beside = park_async_caller_in_lane(vm, cx, call_pc);
    let context = CallContext::user_function(function, callee);
    let result = vm.start_async_call(code, function, env, this, arguments.as_slice(), context);
    note_allocation(vm);
    vm.lane_stack.refresh_activation_limit();
    Some(async_call_resumed_in_lane(vm, cx, ip, i, call_pc, beside, result))
}

/// Where the lane goes once `start_async_call_in_lane`'s job has run its first step: on with the
/// promise in the call's register, or out with the error, materializing the deferred caller
/// the job left unobserved so the error has a frame to unwind.
fn async_call_resumed_in_lane(vm: &mut Vm, cx: *mut LaneContext, ip: Ip, i: &LaneInstruction, call_pc: usize, beside: bool, result: Result<Value, JsError>) -> Resumed {
    let unobserved = beside && vm.lane_deferred.take().is_some();
    match result {
        Ok(promise) => {
            // The activations may have become frames, whose registers the context now names.
            let r = unsafe { (*cx).registers };
            r.set(i.a, promise);
            Resumed::At(r, following(ip), Carried::script(cx))
        }
        Err(error) => {
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
                vm.materialize_deferred_lane();
            }
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(ip))
        }
    }
}

/// Where `start_async_call_in_lane`'s caller resumes once the callee's job settles its first
/// step: beside other activations, a deferred lane call the job materializes only if it
/// observes the caller (true); otherwise the caller's own frame, resuming past the call.
fn park_async_caller_in_lane(vm: &mut Vm, cx: *mut LaneContext, call_pc: usize) -> bool {
    let beside = vm.lane_stack.has_frames();
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
    } else {
        // SAFETY: the context outlives the lane run.
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].pc = call_pc + 1;
        vm.frames[frame].pc_is_resume = true;
    }
    beside
}

/// The facts that keep function `function` of `program` out of a frameless activation, for
/// the census of frame calls and constructions.
#[cfg(feature = "profile-lane-exits")]
fn frame_entry_census(vm: &Vm, program: ProgramId, function: u32) -> Option<String> {
    vm.programs.residual(program).and_then(|callee_code| {
        let body = callee_code.function_get(function as usize)?;
        let plan = body.call_plan(callee_code);
        let closures = body.code.iter().any(|packed| body.widened(*packed).op().makes_closure());
        Some(format!(
            "{} @{:?} registers_only={} env_activation={} arrow={} rest={} closures={} arguments={} handlers={} class_ctor={} async={} generator={} promoted={}/{} simple_params={} selective={} self={} plain={} in_loop={} legacy={} live={} addresses={} initial={:?} name_ops={:?} unpromoted={:?}",
            body.name.map(|atom| callee_code.atoms[atom as usize].to_string()).unwrap_or_default(),
            body.source_span.map(|span| span.start), plan.registers_only, plan.environment_activation, body.is_arrow, body.rest,
            closures, body.arguments_slot.is_some(), !body.handlers.is_empty(), body.is_class_constructor, body.is_async,
            body.is_generator, body.local_registers.len(), body.locals, body.simple_parameters, body.selective_capture_slots.is_some(),
            body.self_binding_slot.is_some(), plan.plain, plan.in_loop, plan.legacy_reflection, plan.live_parameters,
            plan.addresses_locals, body.initial_register,
            body.code.iter().map(|packed| body.widened(*packed)).filter(|instruction|
                quench_runtime::staged::names_binding(instruction.op()))
                .map(|instruction| format!("{:?}:{}", instruction.op(), { let atom = instruction.atom_index() as usize; if atom < callee_code.atoms.len() { callee_code.atoms[atom].replace('\0', "~") } else { "?".into() } }))
                .collect::<Vec<_>>(),
            body.local_atoms.iter().enumerate()
                .filter(|(slot, _)| !body.local_registers.iter().any(|entry| usize::from(entry.local) == *slot))
                .map(|(_, atom)| callee_code.atoms[*atom as usize].replace('\0', "~"))
                .collect::<Vec<_>>(),
        ))
    })
}

/// `frame_call_in_lane`'s call of `callee`, resolved to the in-loop `target`, with receiver
/// `this` and the call window's values past its first `skip` (a forwarded call's own
/// arguments, `Vm::forwarded_call`): entered directly when the JavaScript tier promoted it,
/// else pushed as the general loop's in-loop call pushes it.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn push_frame_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: (ProgramId, u32, Value),
    callee: Value,
    this: Value,
    passed: FrameCallArguments<'_>,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    // A callee of another program runs in the lane only where the lane admits its program.
    if target.0 != unsafe { (*cx).program }
        && !vm.programs.residual(target.0).is_some_and(|callee_code| vm.admits_in_lane(target.0, callee_code))
    {
        return Resumed::Exit(exit(ip));
    }
    // A callee that has not made its deferred runs takes the general path, as a lane call's
    // does (`ProgramStore::call_may_derive_in_lane`): entering its frame here derived its view at
    // its first call, a callee run once included (jiti npm scenario: -2.1% instructions).
    if !vm.call_may_derive_in_lane(target.0, target.1) {
        return Resumed::Exit(exit(ip));
    }
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = frame_entry_census(vm, target.0, target.1) {
        quench_runtime::vm::lane_exits::record_detail(format!("frame call {body}"));
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
        return Resumed::At(r, following(ip), Carried::script(cx));
    }
    vm.materialize_lane_activations(cx);
    // The caller is now the innermost frame, its registers the frame's.
    let (frame, r) = unsafe { ((*cx).frame(), (*cx).registers) };
    // SAFETY: the call record lies in the running view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    // SAFETY: the call's window is registers of the caller frame, which the push neither
    // moves nor frees before it copies them; a list outlives the push.
    let arguments: *const [Value] = match passed {
        FrameCallArguments::Window(skip) => unsafe {
            std::ptr::slice_from_raw_parts(r.0.add(usize::from(window) + skip), usize::from(argc) - skip)
        },
        FrameCallArguments::List(list) => list,
    };
    let destination = i.a & quench_runtime::bytecode::REGISTER_MASK;
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
            completion,
            ..
        })) => {
            // A callee of another program runs in its own, which the push made active
            // and the loop's executing one sets aside as the loop's own push does.
            let caller_program = (target.0 != unsafe { (*cx).program }).then(|| {
                // SAFETY: the general loop's executing program outlives the lane run.
                let executing = unsafe { &mut *(*cx).executing };
                let saved = quench_runtime::vm::dispatch_frame::SavedProgram {
                    id: executing.id,
                    retained: std::mem::replace(&mut executing.retained, vm.programs.get(target.0)),
                };
                executing.id = target.0;
                enter_program_in_lane(cx, target.0);
                saved
            });
            // SAFETY: the general loop's pending calls outlive the lane run.
            unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
                caller: frame,
                caller_cursor: None,
                call_pc: call_pc as u32,
                destination,
                completion,
                returns,
                call_guard: Some(call_guard),
                caller_program,
            });
            // The pushed frame's guard narrows the activations the thread admits.
            vm.lane_stack.refresh_activation_limit();
            // The tier observes the frame's entry as the general loop's would, and a
            // native result continues at the `Return` that completes the frame.
            let callee_frame = frame + 1;
            // SAFETY: the context's program, now the callee's, outlives the lane run.
            if let Some(return_pc) = vm.js_tier_frame_entry(unsafe { &*(*cx).code }, callee_frame) {
                vm.frames[callee_frame].pc = return_pc;
            }
            enter_script_frame_in_lane(cx, callee_frame)
        }
        // A failed push (the guest call depth) throws at the call, as the general path's.
        Some(Err(error)) => {
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(ip))
        }
        _ => Resumed::Exit(exit(ip)),
    }
}

/// A JavaScript `Call` of an exported Wasm function that `Vm::wasm_export_frame_call`
/// admits: its frame, over the instance environment, pushed with the pending call that
/// converts its result (`CallCompletion::WasmExport`) and entered in this lane, as a Wasm
/// call's frame is (`wasm_calls`'s `push_frame`), so the call nests no native frame and no
/// run of the general loop. Lane activations become frames first, as for any frame call.
/// `None` leaves the call to the native: a run that is not a frame's own JavaScript run
/// (`LaneContext::script_calls`), a callee native code may run, a call completing a caller
/// the lane does not complete (`returning_call_admitted`), or another program the lane
/// does not admit.
#[inline(never)]
fn lane_wasm_export_frame(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
    quench_runtime::staged::wasm_activation::step_export_call_of(&mut LaneExportCall { cx, r, ip, i: std::ptr::from_ref(i), call: None })
}


/// `new` of a user constructor that needs a VM frame, made in place as the general loop's
/// in-loop construction makes it (`Vm::inline_construct_target`): lane activations become
/// frames, the receiver is allocated, and the constructor's frame is pushed with the pending
/// construction that substitutes the receiver for a non-object result
/// (`return_script_frame_in_lane`). A derived class constructor is pushed the same way without a
/// receiver (`Vm::inline_derived_construct_target`): its frame binds `this` at `super(...)`, and
/// its pending call applies the derived return rules (`CallCompletion::DerivedConstructor`).
/// Any other construction takes the general path.
#[inline(never)]
fn frame_construct_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let callee = r.get(i.b);
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    // The checks `inline_construct_target` makes of the callee alone, before activations
    // become frames for it. A constructor of another program is pushed in its own, where the
    // lane admits it.
    // A derived class constructor is pushed too (`Vm::inline_derived_construct_target`): its
    // frame binds `this` at `super(...)`, and its pending call applies the derived return
    // rules (`CallCompletion::DerivedConstructor`), where it left the lane before.
    let admits = |body: &quench_runtime::bytecode::Function| Vm::inline_constructible(body) || Vm::inline_derived_constructible(body);
    let pushable = match vm.function_record(callee) {
        Some(record @ FunctionRecord { kind: FunctionKind::User(program, id), .. }) => {
            vm.record_realm(record) == vm.realm.globals
                && if *program == unsafe { (*cx).program } {
                    code.function_get(*id as usize).is_some_and(admits)
                } else {
                    vm.programs.residual(*program).is_some_and(|callee_code| {
                        vm.admits_in_lane(*program, callee_code)
                            && callee_code
                                .function_get(*id as usize)
                                .is_some_and(admits)
                    })
                }
        }
        _ => false,
    };
    if !pushable
        || !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some())
        || returns && !returning_call_admitted(cx)
    {
        return exit(ip);
    }
    vm.materialize_lane_activations(cx);
    // The caller is now the innermost frame, its registers the frame's.
    let (frame, r) = unsafe { ((*cx).frame(), (*cx).registers) };
    let target = match vm.inline_construct_target(code, frame, callee) {
        Some((program, id, env, this)) => Some((program, id, env, this, CallCompletion::Constructor(this))),
        None => vm
            .inline_derived_construct_target(code, frame, callee)
            .map(|(program, id, env)| (program, id, env, Value::UNDEFINED, CallCompletion::DerivedConstructor)),
    };
    let Some((program, id, env, this, completion)) = target else {
        return exit(ip);
    };
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    // SAFETY: the construction's window is registers of the caller frame.
    let arguments = std::ptr::slice_from_raw_parts(unsafe { r.0.add(usize::from(i.c)) }, argc);
    derived_frames::push_construction_frame(cx, ip, i, frame, (program, id, env), (callee, callee), (this, completion), arguments)
}

/// Continue in JavaScript frame `frame` at its resume point, with the lane's per-frame
/// facts for it: `new` activates in place only from an ordinary body
/// (`LaneContext::script_constructs`).
#[inline(always)]
fn enter_script_frame_in_lane(cx: *mut LaneContext, frame: usize) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let function = vm.frames[frame].function;
    unsafe {
        (*cx).script_constructs = code
            .function_get(function as usize)
            .is_some_and(|function| function.dispatch == quench_runtime::bytecode::DispatchClass::General);
    }
    enter_frame_in_lane(cx, frame, vm.frames[frame].pc)
}

/// The return of the running JavaScript frame with `value` to the caller frame the general
/// loop's pending call names, made in place as that loop makes it: the frame retires, an
/// in-loop `new` substitutes its receiver for a non-object result, and the caller continues
/// with the value in the call's register. A call that returns its callee's completion
/// completes its caller in turn: another pending caller here, or the loop's own frame,
/// which the general loop returns from at that call (`frame_return`). A call into
/// another program returns to the caller's, as the loop's return restores it.
#[inline(never)]
fn return_script_frame_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    mut value: Value,
    mut retired: bool,
) -> Ip {
    let vm = unsafe { vm(cx) };
    let pending = unsafe { &mut *(*cx).pending };
    let mut frame = unsafe { (*cx).frame() };
    let mut exit_ip = ip;
    loop {
        // An async callee's promise settles on the general path (`PendingGeneralCall::completion`);
        // a generator whose activation became a frame finishes here as there (`PendingGeneralCall::completion`).
        // A derived constructor's construction completes here only where it throws nothing
        // (`Vm::derived_construct_completes`); its errors are raised at the call on the general path.
        let returns_here = pending.last().is_some_and(|call| {
            lane_completes_return(cx, call, frame, retired)
                && (!matches!(call.completion, CallCompletion::DerivedConstructor)
                    || vm.derived_construct_completes(value, vm.frames[frame].this))
        });
        if !returns_here {
            // A record that has not run yet runs again on the general path; one that ran (a
            // call made, `retired`) completes the loop's own frame with the value there.
            if !retired {
                return exit(exit_ip);
            }
            return frame_return(vm, exit_ip, value);
        }
        let mut call = pending.pop().expect("a pending call names the returning frame");
        let completed = vm.frames.pop().expect("the returning frame is the innermost");
        // Checked above: the returned object, else the `this` binding `super(...)` made.
        if matches!(call.completion, CallCompletion::DerivedConstructor) && !vm.is_object_like(value) {
            value = completed.this;
        }
        // Retired as the general loop's return retires it (`Vm::retire_returned_frame`).
        let (retired_value, generator) = vm.retire_returned_frame(completed, &mut call.completion, value, None);
        value = retired_value;
        if generator {
            note_allocation(vm);
        }
        retired = true;
        // A call into another program returns to the caller's, which the loop executes again.
        if let Some(saved) = call.caller_program.take() {
            // SAFETY: the general loop's executing program outlives the lane run.
            let executing = unsafe { &mut *(*cx).executing };
            executing.id = saved.id;
            executing.retained = saved.retained;
            enter_program_in_lane(cx, saved.id);
        }
        drop(call.call_guard);
        frame = call.caller;
        // The completion may read the caller's register (`CallCompletion::Assigned`), which
        // names the register file `active_registers` holds.
        vm.active_registers = vm.frames[frame].registers.as_mut_ptr();
        value = vm.completed_call_value(&call.completion, frame, call.destination, value);
        if !call.returns {
            vm.write(frame, call.destination, value);
            return enter_script_frame_in_lane(cx, frame).into_loop();
        }
        // The caller completes too, at its call record.
        let (_, call_ip, _) = frame_state_in_lane(cx, frame, call.call_pc as usize);
        exit_ip = call_ip;
    }
}

/// Return from a frame the general loop or a frame push made. Out of line
/// so the activation return saves no registers.
#[cold]
#[inline(never)]
fn return_frame_in_lane(r: Registers, ip: Ip) -> Resumed {
    let cx = context_in_lane();
    // SAFETY: called only while a lane runs, from its return handler.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    if unsafe { (*cx).returns_script_frame } {
        return Resumed::Exit(return_script_frame_in_lane(cx, ip, r.get(i.a), false));
    }
    let pending = unsafe { &mut *(*cx).pending };
    if unsafe { (*cx).inline_calls }
        && pending.last().is_some_and(|call| {
            matches!(call.completion, CallCompletion::WasmExport(_)) && call.caller + 2 == vm.frames.len()
        })
    {
        return lane_return_wasm_export(cx, r.get(i.a));
    }
    // An in-loop `new` substitutes its receiver for a non-object result;
    // the general path owns that [[Construct]] step.
    if !unsafe { (*cx).inline_calls }
        || pending.last().is_none_or(|call| {
            !matches!(call.completion, CallCompletion::Value)
                || call.caller_program.is_some()
        })
    {
        return Resumed::Exit(exit(ip));
    }
    let value = r.get(i.a);
    let Some(call) = pending.pop() else {
        return Resumed::Exit(exit(ip));
    };
    vm.retire_wasm_frame();
    vm.active_registers = vm.frames[call.caller].registers.as_mut_ptr();
    vm.write(call.caller, call.destination, value);
    drop(call.call_guard);
    let pc = vm.frames[call.caller].pc;
    enter_frame_in_lane(cx, call.caller, pc)
}

/// The return of an exported function's frame a JavaScript call pushed
/// (`lane_wasm_export_frame`) with its encoded `value`: the frame retires, its caller's
/// program runs again as the run's JavaScript (`LaneContext::script_calls`), and the call's
/// register takes the value's JavaScript form (`CallCompletion::WasmExport`), or, for a call
/// that completes its caller, the caller returns it.
#[cold]
#[inline(never)]
fn lane_return_wasm_export(cx: *mut LaneContext, value: Value) -> Resumed {
    // The record is unused: a frame's return reads only its pending call.
    quench_runtime::staged::wasm_activation::step_export_frame_return_of(&mut LaneExportCall { cx, r: Registers(std::ptr::null_mut()), ip: std::ptr::null(), i: std::ptr::null(), call: None }, value)
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

/// The opcode of the catalog's keyed read: the element row whose record defines a result
/// register (`Decoder::Element`), whose generated row (`generated_row`) a register-keyed read runs.
pub(crate) const ELEMENT_READ: u16 = {
    let mut index = 0;
    loop {
        match Op::from_index(index) {
            Some(op) if matches!(op.staged(), Some(quench_runtime::bytecode::Decoder::Element))
                && matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister) => break index as u16,
            Some(_) => index += 1,
            None => panic!("the catalog names a keyed read"),
        }
    }
};

/// The opcode of the catalog's keyed store: the element row whose record defines no result register
/// (`Decoder::Element`), whose generated row (`generated_row`) a register-keyed store runs.
pub(crate) const ELEMENT_WRITE: u16 = {
    let mut index = 0;
    loop {
        match Op::from_index(index) {
            Some(op) if matches!(op.staged(), Some(quench_runtime::bytecode::Decoder::Element))
                && !matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister) => break index as u16,
            Some(_) => index += 1,
            None => panic!("the catalog names a keyed store"),
        }
    }
};

/// The handler serving opcode `OP` whose accumulator field is `ACC`, derived
/// from the opcode's semantic family; `None` when the general path executes
/// the opcode.
const fn handler_for<const OP: u16, const BACKWARD: bool, const ACC: u8>()
-> Option<LaneHandler> {
    let Some(op) = Op::from_index(OP as usize) else {
        return None;
    };
    // Past the first 256 opcodes only generated rows run in the lane; the rest leave it, as
    // before the table covered them, and instantiate nothing.
    if OP >= 256 && !admits_row(op) && !jumps_by_row(op) {
        return None;
    }
    if jumps_by_row(op) {
        return Some(LaneHandler { handler: row_jump!(OP, BACKWARD, FLAG_UNREAD, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }), produces: false });
    }
    if admits_row(op) {
        return Some(LaneHandler { handler: generated_row!(OP, ACC, FLAG_UNREAD), produces: row_produces(op) });
    }
    let handler: Handler = match other_handler(op) {
        Some(handler) => handler,
        None => return None,
    };
    Some(LaneHandler {
        handler,
        produces: false,
    })
}


/// The handlers of a class's private elements, none of which runs guest code: a field's
/// definition, a method's brand, and the brand checks of a read and of `#x in`.
const fn private_element_handler(opcode: Op) -> Option<Handler> {
    match opcode {
        _ if quench_runtime::staged::private_field_definition_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_FIELD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_PRIVATE_FIELD as u16 }) as Handler),
        _ if quench_runtime::staged::private_name_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_NAME_MARK as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::MARK_PRIVATE_NAME as u16 }) as Handler),
        _ if quench_runtime::staged::private_check_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_CHECK as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::CHECK_PRIVATE as u16 }) as Handler),
        _ if quench_runtime::staged::private_in_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_IN as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::PRIVATE_IN as u16 }) as Handler),
        _ => None,
    }
}

/// Handlers that neither produce nor read the accumulator, apart from the
/// branch tests, which read it.
const fn other_handler(opcode: Op) -> Option<Handler> {
    match opcode {
        // The lane's synthetic prologue records: their staged rows, which the lane's activations start at.
        quench_runtime::bytecode::TIER_PROLOGUE => Some(tier_prologue_row()),
        quench_runtime::bytecode::BODY_ENTRY => Some(body_entry_row()),
        // A raising record unwinds in the lane where a handler takes it (`handler_edges::throw_record`).
        _ if opcode.raises() => Some(handler_edges::throw_record),
        // A derived constructor's `this` binding in its own frame (`derived_frames`).
        _ if opcode.binds_this() => Some(derived_frames::bind_this_in_frame),
        _ if quench_runtime::staged::field_read_family(opcode) => Some(row_get_field_first),
        // merge: theirs' `lane_set_object_field_own` / `lane_set_this_field_own` are undefined; HEAD's `field_store_row` serves both receivers.
        _ if opcode.field_store().is_some() => match opcode.field_store() { Some(store) => Some(field_store_row(store)), None => None },
        _ if quench_runtime::staged::property_record_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PROPERTY_RECORD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_PROPERTY_RECORD as u16 }) as Handler),
        _ if quench_runtime::staged::field_definition_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::FIELD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_FIELD as u16 }) as Handler),
        _ if private_element_handler(opcode).is_some() => private_element_handler(opcode),
        _ if quench_runtime::staged::copy_properties_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::COPY_DATA_PROPERTIES as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::COPY_DATA_PROPERTIES as u16 }) as Handler),
        _ if opcode.steps_in_lane() => Some(general_step_in_lane),
        _ if quench_runtime::staged::element_read_family(opcode) => Some(protocol_row!(ELEMENT_READ, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_INDEX as u16 }) as Handler),
        _ if quench_runtime::staged::element_write_family(opcode) => Some(protocol_row!(ELEMENT_WRITE, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::SET_INDEX as u16 }) as Handler),
        _ if quench_runtime::staged::known_call_family(opcode) => Some(known_call_handler()),
        _ if quench_runtime::staged::call_family(opcode) => Some(script_call_row(ANY_ARGC)),
        _ if quench_runtime::staged::call_array_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::CALL_ARRAY as u16 }, ACC_NONE, { protocol_family::NATIVE_CALL | protocol_misses::native_call::CALL_ARRAY as u16 }) as Handler),
        _ if quench_runtime::staged::call_spread_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::CALL_SPREAD as u16 }, ACC_NONE, { protocol_family::NATIVE_CALL | protocol_misses::native_call::CALL_SPREAD as u16 }) as Handler),
        _ if quench_runtime::staged::delete_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::DELETE_ROW as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::DELETE as u16 }) as Handler),
        _ if quench_runtime::staged::construction_family(opcode) => Some(protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::SCRIPT_CONSTRUCT as u16 }>()),
        _ if quench_runtime::staged::forwarded_call_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::FORWARDED_CALL as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::FORWARD_CALL as u16 })),
        _ if quench_runtime::staged::return_family(opcode) => Some(return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(RETURN_KIND, NOT_LEAF))),
        _ if generator_yields::suspend_handler(opcode).is_some() => generator_yields::suspend_handler(opcode),
        _ => None,
    }
}

/// Dense fast handlers cover every opcode, in whole table rows: a generated row
/// (`admits_row`) runs from its table entry whatever its index.
const TABLE_SLOTS: usize = Op::COUNT.next_multiple_of(TABLE_ROW as usize);

/// Opcodes per generated table row.
const TABLE_ROW: u16 = 16;
const TABLE_ROWS: usize = TABLE_SLOTS / TABLE_ROW as usize;
// `lane_table` lists one generated row per sixteen opcodes.
const _: () = assert!(TABLE_ROWS == 19, "add a `table_row!` to `lane_table` per sixteen new opcodes");

macro_rules! table_row {
    ($backward:literal, $acc:expr, $row:literal) => {
        [
            handler_for::<{ $row * TABLE_ROW }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 1 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 2 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 3 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 4 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 5 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 6 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 7 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 8 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 9 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 10 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 11 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 12 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 13 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 14 }, $backward, { $acc }>(),
            handler_for::<{ $row * TABLE_ROW + 15 }, $backward, { $acc }>(),
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
    ($backward:literal, $acc:expr) => {
        flatten_rows([
            table_row!($backward, $acc, 0),
            table_row!($backward, $acc, 1),
            table_row!($backward, $acc, 2),
            table_row!($backward, $acc, 3),
            table_row!($backward, $acc, 4),
            table_row!($backward, $acc, 5),
            table_row!($backward, $acc, 6),
            table_row!($backward, $acc, 7),
            table_row!($backward, $acc, 8),
            table_row!($backward, $acc, 9),
            table_row!($backward, $acc, 10),
            table_row!($backward, $acc, 11),
            table_row!($backward, $acc, 12),
            table_row!($backward, $acc, 13),
            table_row!($backward, $acc, 14),
            table_row!($backward, $acc, 15),
            table_row!($backward, $acc, 16),
            table_row!($backward, $acc, 17),
            table_row!($backward, $acc, 18),
        ])
    };
}

struct LaneTable;

impl LaneTable {
    /// Tables indexed by jump direction (forward, then backward: a
    /// safepoint) and by accumulator field.
    const TABLES: [[Table; ACC_POSITIONS]; 2] = [
        [
            lane_table!(false, ACC_NONE),
            lane_table!(false, ACC_A),
            lane_table!(false, ACC_B),
            lane_table!(false, ACC_C),
            lane_table!(false, ACC_PAIR),
        ],
        [
            lane_table!(true, ACC_NONE),
            lane_table!(true, ACC_A),
            lane_table!(true, ACC_B),
            lane_table!(true, ACC_C),
            lane_table!(true, ACC_PAIR),
        ],
    ];
}

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
pub(crate) fn fields_fit_in_lane(instruction: WideInstruction, function: &Function) -> bool {
    let registers = function.registers;
    let op = instruction.op();
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    let numeric_local = instruction.writes_numeric_local()
        && instruction.a() & quench_runtime::bytecode::RETURN_REGISTER == 0
        && instruction.result_register() < function.locals;
    let registers_fit = fields
        .iter()
        .all(|&(field, value)| match op.field_layout(field) {
            FieldLayout::ResultRegister if field == InstructionField::A && numeric_local => true,
            // A lane call reads its return flag from the field (`call_fits_in_lane`), as does
            // the spread of a kept rest view (`call_rest_miss`), whose every path completes a
            // returning call.
            FieldLayout::ResultRegister
                if field == InstructionField::A
                    && (op.traits().has(quench_runtime::bytecode::Traits::FRAME_RETURNABLE)
                        || op.traits().has(quench_runtime::bytecode::Traits::REST_SPREAD_CALL)
                        || (quench_runtime::staged::computes_binary(op) && instruction.returns_from_frame())) =>
            {
                instruction.result_register() < registers
            }
            FieldLayout::FieldBase => {
                value == quench_runtime::bytecode::FieldBase::NESTED
                    || quench_runtime::bytecode::FieldBase(value)
                        .register_index()
                        .is_none_or(|register| register < registers)
            }
            FieldLayout::OptionalRegister | FieldLayout::OptionalReadRegister => value
                .checked_sub(quench_runtime::bytecode::OPTIONAL_REGISTER_BIAS)
                .is_none_or(|register| register < registers),
            FieldLayout::Operand => {
                let operand = quench_runtime::bytecode::Operand(value);
                operand.kind() != Some(quench_runtime::bytecode::OperandKind::Register)
                    || operand.payload() < registers
            }
            layout => !layout.is_register_field() || value < registers,
        });
    let window_fits = op.field_layout(InstructionField::B) != FieldLayout::RegisterWindowBase
        || u32::from(instruction.b()) + u32::from(instruction.c()) <= u32::from(registers);
    let pair_fits = op.immediate_layout() != ImmediateLayout::RegisterPair || {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        first < registers && second < registers
    };
    registers_fit && window_fits && pair_fits
}

/// The record operand of `instruction` at `pc`, or `None` when its jump or
/// constant cannot be resolved.
fn operand_in_lane(
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
        // A case selector reads its run of constants from the context by index, as a constant
        // array reads its elements' (the `MakeConstArray` row).
        ImmediateRole::ConstantIndex
            if quench_runtime::staged::selects_string_case(instruction.op()) || quench_runtime::staged::const_array_family(instruction.op()) =>
        {
            let start = instruction.imm() as usize;
            constants.get(start..start.checked_add(usize::from(instruction.b()))?)?;
            LaneOperand { imm: instruction.imm() }
        }
        ImmediateRole::ConstantIndex => LaneOperand {
            constant: *constants.get(instruction.imm() as usize)?,
        },
        _ if holds_field_hint(instruction.op()) => LaneOperand {
            field: FieldOperand { imm: instruction.imm(), shape: quench_runtime::vm::NO_FIELD_RECEIVER },
        },
        _ if instruction.op().makes_script_call() => LaneOperand {
            call: CallOperand { imm: instruction.imm(), memo: NO_READ_MEMO },
        },
        _ => LaneOperand {
            imm: instruction.imm(),
        },
    })
}

/// A `Call` runs in the lane only as the general loop pushes one in place: it evaluates no
/// direct or parameter-scope `eval`, which a window activation's call record runs apart (`lane_eval`). It, `GetField`, `Unary` and `Binary` may return their
/// result from the frame (`returns_in_lane`, `returning_miss`); a returning record runs no
/// other result flag.
fn call_fits_in_lane(instruction: WideInstruction) -> bool {
    match instruction.op() {
        _ if quench_runtime::staged::call_family(instruction.op()) || quench_runtime::staged::call_array_family(instruction.op())
            || quench_runtime::staged::call_spread_family(instruction.op()) => {
            returns_in_lane(instruction)
                && !instruction.direct_eval()
                && !instruction.parameter_eval()
        }
        _ if quench_runtime::staged::forwarded_call_family(instruction.op()) => returns_in_lane(instruction),
        // `super(...)` takes either argument form (`derived_frames::super_construct_in_frame`).
        _ if quench_runtime::staged::construction_family(instruction.op()) => {
            returns_in_lane(instruction)
                && (instruction.is_super_construct() || !ImmediateLayout::construct_array_arguments(instruction.imm()))
        }
        _ if quench_runtime::staged::field_read_family(instruction.op()) => returns_in_lane(instruction) || sinks_field_in_lane(instruction),
        _ if quench_runtime::staged::unary_family(instruction.op()) || quench_runtime::staged::object_pair_family(instruction.op())
            || quench_runtime::staged::super_const_array_family(instruction.op()) || quench_runtime::staged::object_literal_family(instruction.op()) => {
            returns_in_lane(instruction)
        }
        _ if quench_runtime::staged::binary_family(instruction.op()) && instruction.returns_from_frame() => returns_in_lane(instruction),
        _ if spreads_rest_view(instruction) => returns_in_lane(instruction),
        _ => true,
    }
}

/// Whether `instruction`'s staged row is a call spreading a rest parameter kept as a view
/// (`Passing::Rest`): the interpreter's own decode names it, so no opcode is listed here.
fn spreads_rest_view(instruction: WideInstruction) -> bool {
    matches!(quench_runtime::staged::Operation::<u64, ()>::decode_names(instruction),
        Some(quench_runtime::staged::Operation::SpreadCall { passing: quench_runtime::staged::Passing::Rest, .. }))
}

/// A `Call` record's operand, flagged when only the `Await` right after the call reads its
/// result: the await overwrites the result register, or liveness (`live`) shows it dead after
/// the await. A settling call there makes its promise only where one becomes observable
/// (`ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG`). Liveness is derived only for a call
/// followed by an `Await`, of a function whose suspension derives it anyway
/// (`Vm::suspend_activation`).
fn call_awaited_marked_in_lane<'live>(
    operand: LaneOperand,
    decoded: &[WideInstruction],
    pc: usize,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
) -> LaneOperand {
    let call = decoded[pc];
    let Some(&awaited) = decoded.get(pc + 1) else {
        return operand;
    };
    if !quench_runtime::staged::calls(call.op()) || call.result_flags() != 0 || !quench_runtime::staged::awaits(awaited.op()) {
        return operand;
    }
    let result = call.result_register();
    let feeds = awaited.register_b() == result
        && (awaited.result_register() == result
            || live()
                .and_then(|masks| masks.get(pc + 2))
                .is_some_and(|live| quench_runtime::compile::liveness::register_dead(live, usize::from(result))));
    if !feeds {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`operand_in_lane`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG }
}

/// A `Call` record's operand, flagged when liveness (`live`, the registers live before each
/// pc) shows its result register dead after the call: nothing reads the value the callee
/// returns (`ImmediateLayout::lane_call_result_used`).
fn call_result_marked_in_lane<'live>(
    operand: LaneOperand,
    instruction: WideInstruction,
    pc: usize,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
) -> LaneOperand {
    if !quench_runtime::staged::calls(instruction.op()) {
        return operand;
    }
    let Some(masks) = live() else {
        return operand;
    };
    if !quench_runtime::compile::liveness::call_result_dead(instruction, pc, masks) {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`operand_in_lane`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_UNUSED_FLAG }
}

/// Whether the `Call` at `pc` is an iteration step: no arguments, its result checked by the
/// `RequireIteratorResult` after it, as the compiler emits a for-of or destructuring step,
/// whose result register only the step's own `done` and `value` reads take
/// (`iterator_step_miss`).
fn iteration_step_in_lane(decoded: &[WideInstruction], pc: usize) -> bool {
    let call = decoded[pc];
    call.result_flags() == 0
        && ImmediateLayout::argument_count(call.imm()) == 0
        && decoded.get(pc + 1).is_some_and(|next| {
            quench_runtime::staged::requires_iterator_result(next.op()) && next.register_b() == call.result_register()
        })
}

/// The records of a for-of step after its iteration `Call` (`for_of_step_in_lane`), by offset
/// from it: the `done` read of the step's result, then its test.
/// The for-of step's `RequireIteratorResult`, whose otherwise unused `a` the compiler sets
/// when the step's value goes only to an array pattern (`FunctionCompiler::for_iterable`).
const FOR_OF_REQUIRE_RESULT: usize = 1;
const FOR_OF_DONE_READ: usize = 2;
const FOR_OF_DONE_TEST: usize = 3;

/// How a for-of step's records test its `done` read (`for_of_step_in_lane`). As `for_of` emits
/// it, a `JumpFalse` into the body jumps over the `Jump` that leaves the loop; the rewrite
/// that threads a jump over a jump makes them one `JumpTrue` that leaves it.
/// `generator_step_miss`'s `READS` for a for-of step of each `ForOfTest` form.
const STEP_READS_EMITTED: u8 = 1;
const STEP_READS_THREADED: u8 = 2;

use quench_runtime::vm::engine_types::ForOfTest;

#[quench_macros::ext(ForOfTestOps1)]
impl ForOfTest {
    /// The offset of the record that leaves the loop: the `Jump` after the test, or the
    /// threaded test itself.
    const fn exit(self) -> usize {
        match self {
            Self::Emitted => FOR_OF_DONE_TEST + 1,
            Self::Threaded => FOR_OF_DONE_TEST,
        }
    }

    /// The offset of the `value` read, which follows the record that leaves the loop.
    const fn value_read(self) -> usize {
        self.exit() + 1
    }

    /// The records the step spans, its `Call` through its `value` read.
    const fn records(self) -> usize {
        self.value_read() + 1
    }
}

/// How the iteration step at `pc` (`iteration_step_in_lane`) begins a for-of step: after its
/// `RequireIteratorResult`, a plain read of the result into a register, a test of that
/// register (`ForOfTest`), and a second plain read of the result where the body begins.
/// `iterator_step_reads_miss` then makes the reads from the step itself; their atoms it
/// tests as it runs.
fn for_of_step_in_lane(decoded: &[WideInstruction], pc: usize) -> Option<ForOfTest> {
    let result = decoded[pc].result_register();
    let record = |offset: usize| decoded.get(pc + offset).copied();
    let reads_result = |offset: usize| {
        record(offset).is_some_and(|read| quench_runtime::staged::reads_named_field(read.op()) && read.result_flags() == 0 && read.b() == result)
    };
    record(FOR_OF_REQUIRE_RESULT).filter(|require| quench_runtime::staged::meaning_fast::require_iterator_result_operand(*require).is_some())?;
    let done = record(FOR_OF_DONE_READ).filter(|_| reads_result(FOR_OF_DONE_READ))?.result_register();
    let test = record(FOR_OF_DONE_TEST).filter(|test| test.a() == done)?;
    let form = match quench_runtime::staged::truth_jump(test.op())? {
        false => ForOfTest::Emitted,
        true => ForOfTest::Threaded,
    };
    let exit = record(form.exit())?;
    let leaves = (if form == ForOfTest::Emitted { quench_runtime::staged::jump_family(exit.op()) } else { quench_runtime::staged::truth_jump(exit.op()) == Some(true) })
        && exit.jump_target() as usize > pc + form.value_read();
    let enters = form == ForOfTest::Threaded || test.jump_target() as usize == pc + form.value_read();
    (leaves && enters && reads_result(form.value_read())).then_some(form)
}

/// The records of a destructuring step after its iteration `Call` (`binding_step_in_lane`), by
/// offset from it: the `done` read, its move to the pattern's done flag, the threaded test
/// leaving the step, and the `value` read.
const BINDING_DONE_READ: usize = 2;
const BINDING_DONE_MOVE: usize = 3;
const BINDING_DONE_TEST: usize = 4;
const BINDING_VALUE_READ: usize = 5;

/// Whether the iteration step at `pc` (`iteration_step_in_lane`) is a destructuring step as
/// `binding_iterator_step` emits it once its test is threaded: a plain read of the result's
/// `done`, a move of it to another register, a `JumpTrue` of it past the step, and a plain
/// read of the result's `value`. `iterator_step_binding_miss` then makes the reads and the
/// move from the step itself; their atoms it tests as it runs.
fn binding_step_in_lane(decoded: &[WideInstruction], pc: usize) -> bool {
    let result = decoded[pc].result_register();
    let record = |offset: usize| decoded.get(pc + offset).copied();
    let reads_result = |offset: usize| {
        record(offset).is_some_and(|read| quench_runtime::staged::reads_named_field(read.op()) && read.result_flags() == 0 && read.b() == result)
    };
    let (Some(done), Some(flag), Some(test), Some(value)) =
        (record(BINDING_DONE_READ), record(BINDING_DONE_MOVE), record(BINDING_DONE_TEST), record(BINDING_VALUE_READ))
    else {
        return false;
    };
    let done = done.result_register();
    reads_result(BINDING_DONE_READ)
        && reads_result(BINDING_VALUE_READ)
        && quench_runtime::staged::copies_register(flag.op())
        && flag.result_flags() == 0
        && flag.b() == done
        && flag.a() != done
        && flag.a() != result
        && quench_runtime::staged::truth_jump(test.op()) == Some(true)
        && test.a() == done
        && test.jump_target() as usize > pc + BINDING_VALUE_READ
        && value.result_register() != done
        && value.result_register() != flag.a()
        && pc + BINDING_VALUE_READ + 1 < decoded.len()
}

/// A nested `GetField` whose only result flag stores its value to a field of `this` too
/// (`row_get_field_sink`).
fn sinks_field_in_lane(instruction: WideInstruction) -> bool {
    instruction.result_flags() == quench_runtime::bytecode::SET_THIS_REGISTER
        && instruction.writes_current_this()
}

/// Whether the instruction's only result flag, if any, returns the result from the frame.
fn returns_in_lane(instruction: WideInstruction) -> bool {
    instruction.result_flags() & !quench_runtime::bytecode::RETURN_REGISTER == 0
}

/// The register every compiled linear-memory access of `function` goes
/// through, when there is exactly one: the direct accesses', or in a body with none, its
/// vector accesses' (`WasmMemoryAddress`) when the prologue binds it, as lowering binds a
/// memory32 memory once per call.
pub(crate) fn memory_register(function: &Function) -> Option<u16> {
    let instructions = || function.code.iter().map(|packed| function.widened(*packed));
    let direct = instructions().filter_map(quench_runtime::staged::wasm_memory::bound_memory_register);
    single_register(direct).or_else(|| {
        let vector = single_register(
            instructions().filter(|instruction| quench_runtime::staged::wasm_memory_address(instruction.op())).map(|instruction| instruction.c()),
        )?;
        instructions()
            .take_while(|instruction| {
                quench_runtime::staged::wasm_setup_family(instruction.op())
            })
            .any(|instruction| quench_runtime::staged::wasm_instance_binding(instruction.op()) && instruction.a() == vector)
            .then_some(vector)
    })
}

/// The one register `registers` names, when they name exactly one.
fn single_register(mut registers: impl Iterator<Item = u16>) -> Option<u16> {
    let first = registers.next()?;
    registers.all(|register| register == first).then_some(first)
}

/// Whether a lane run of `instruction` could write `register`.
fn writes_in_lane(instruction: WideInstruction, register: u16) -> bool {
    let op = instruction.op();
    let result = matches!(
        op.field_layout(InstructionField::A),
        FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
    ) && instruction.a() == register;
    let optional = op.field_layout(InstructionField::B) == FieldLayout::OptionalRegister
        && instruction
            .b()
            .checked_sub(quench_runtime::bytecode::OPTIONAL_REGISTER_BIAS)
            == Some(register);
    let filled = quench_runtime::staged::wasm_fill_registers(op)
        && (instruction.b()..instruction.b().saturating_add(instruction.c())).contains(&register);
    result || optional || filled
}

use crate::compiled::tier_policy::{LANE_ENTRY_MIN_RUN, LANE_ENTRY_MIN_CALL_RUN};
/// Whether records may read the accumulator. Without sibling calls every
/// handler returns to the lane loop, which carries no accumulator.
const ACCUMULATOR: bool = cfg!(quench_lane_tail_calls);

/// The handler of a returning record of `kind` (`returning_kind`) in a view that has no leaf form:
/// each kind is one opcode's (its traits name it), so the record is that opcode's `returning_row`.
fn returning_handler(kind: u8) -> Handler {
    match kind {
        RETURNING_FIELD => row_returning_field_first::<NOT_LEAF>,
        RETURNING_UNARY => returning_row_of!(quench_runtime::bytecode::UNARY_ROW, RETURNING_UNARY, NOT_LEAF) as Handler,
        RETURNING_BINARY => returning_row_of!(quench_runtime::bytecode::BINARY_ROW, RETURNING_BINARY, NOT_LEAF) as Handler,
        RETURNING_OBJECT => returning_row_of!(quench_runtime::bytecode::OBJECT_PAIR, RETURNING_OBJECT, NOT_LEAF) as Handler,
        RETURNING_OBJECT_LITERAL => returning_row_of!(quench_runtime::bytecode::OBJECT_LITERAL_ROW, RETURNING_OBJECT_LITERAL, NOT_LEAF) as Handler,
        _ => returning_row_of!(quench_runtime::bytecode::SUPER_CONST_ARRAY_OBJECT, RETURNING_LITERAL, NOT_LEAF) as Handler,
    }
}

/// Whether local `slot` of `function` is a plain frame local: no environment holds it, no mapped
/// `arguments` aliases it and it is no self binding. A checked local row reads and writes such a slot
/// in its frame's locals (`lane_frame::row_frame`); any other slot takes its hand handler.
fn plain_frame_local(function: &Function, slot: usize) -> bool {
    !function.local_slot_uses_environment(slot)
        && quench_runtime::vm::arguments::mapped_arguments_slot(function, slot).is_none()
        && !function.is_self_binding_slot(slot)
}

/// Derive a function's lane view from its bytecode and its program's
/// constants.
fn derive_lane_view<'live>(
    function_id: u32,
    function: &Function,
    constants: &[Value],
    heap: quench_runtime::vm::tier_queries::HeapReads<'_>,
    kind: quench_runtime::bytecode::ProgramKind,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    sites: &[quench_runtime::bytecode::FieldSite],
    plan: crate::compiled::admission::RecordPlan,
    diagnostics: bool,
    tiers: &'static Tiers,
) -> LaneView {
    let loop_tier = plan.loop_sites;
    // Each pass below reads instructions by PC; they are decoded once, here.
    let decoded: Vec<WideInstruction> = function.instructions().collect();
    // Every backward jump observes host termination, including allocation-free
    // Wasm loops. Collection and stack-floor exits share this safepoint.

    let tables = const { &LaneTable::TABLES };
    // Only Wasm bodies access linear memory.
    let memory_register = (kind == quench_runtime::bytecode::ProgramKind::Wasm)
        .then(|| memory_register(function))
        .flatten();
    let entries = entered_records(&decoded, function.parameter_end_pc);
    // Wasm instructions, whose fusions the passes under `wasm` make, occur only in Wasm
    // bodies: JavaScript compiles to none of them.
    let wasm = kind == quench_runtime::bytecode::ProgramKind::Wasm;
    let resolves_names = function.has_dynamic_scope_access();
    // One function-wide fact, consulted per local load; it scans the whole body.
    let kept_arguments = std::cell::OnceCell::new();
    let mut producer: Option<u16> = None;
    // Per instruction: the general path executes it, and its record reads
    // the accumulator.
    let mut exits = Vec::with_capacity(function.code.len());
    let mut reads_accumulator = Vec::with_capacity(function.code.len());
    // Per instruction: the register whose value a record reading the accumulator reads
    // there, `NO_ACCUMULATOR` for a record that reads none.
    let mut accumulator_registers = Vec::with_capacity(function.code.len());
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
                quench_runtime::staged::wasm_instance_binding(op) && Some(instruction.a()) == memory_register;
            let memory_access = MemoryLoad::from_direct_op(op).is_some()
                || MemoryStore::from_direct_op(op).is_some();
            let keeps_view = if memory_access {
                Some(instruction.b()) == memory_register
            } else {
                binds_memory
                    || memory_register.is_none_or(|register| !writes_in_lane(instruction, register))
            };
            let operand = operand_in_lane(instruction, pc, constants)
                .filter(|_| {
                    keeps_view && fields_fit_in_lane(instruction, function) && call_fits_in_lane(instruction)
                })
                .map(|operand| call_result_marked_in_lane(operand, instruction, pc, live))
                .map(|operand| call_awaited_marked_in_lane(operand, &decoded, pc, live));
            // Control reaches this record only from the previous one when nothing enters it
            // (`entered_records`); the general loop never enters at a record that reads the
            // accumulator.
            let carried = producer.take().filter(|_| ACCUMULATOR && !entries.contains(pc));
            let acc = carried.map_or(ACC_NONE, |register| {
                accumulator_field(instruction, register)
            });
            // A table is backward when one of its entries jumps back.
            let backward_jump = if quench_runtime::staged::wasm_branch_table(op) {
                    quench_runtime::bytecode::branch_table_entries(pc, instruction).any(|entry| {
                        decoded[entry].imm() as usize <= pc
                    })
                } else {
                    op.immediate_role() == ImmediateRole::JumpTarget
                        && instruction.imm() as usize <= pc
                };
            // A memory binding installs its view whether or not the record has an operand.
            let selected = (binds_memory || operand.is_some()).then_some(()).and_then(|_| if binds_memory {
                Some(LaneHandler {
                    handler: lane_instance_binding_memory as Handler,
                    produces: false,
                })
            } else if quench_runtime::staged::wasm_f64_unary(op)
                && F64UnaryOperator::from_tag(instruction.imm())
                    == Some(F64UnaryOperator::SquareRoot)
            {
                tables[usize::from(backward_jump)][usize::from(acc)].get(op as usize).copied().flatten()
                    .and_then(|_| {
                        LaneHandler::producer(match acc {
                            ACC_NONE => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_NONE, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_NONE as u16 }) as Handler,
                            ACC_A => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_A, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_A as u16 }) as Handler,
                            ACC_B => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_B, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_B as u16 }) as Handler,
                            ACC_C => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_C, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_C as u16 }) as Handler,
                            _ => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_PAIR, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_PAIR as u16 }) as Handler,
                        })
                    })
            } else if admits_row(op) && op.reads_arguments()
                && function.arguments_slot == u16::try_from(instruction.local_slot()).ok()
                && kept_arguments.get_or_init(|| function.kept_arguments()).is_some()
            {
                // The count of the function's kept arguments: its row with that fact (`row_near`).
                let row = tables[usize::from(backward_jump)][usize::from(acc)].get(op as usize).copied().flatten();
                row.map(|row| LaneHandler { handler: row_near::<ARGUMENTS_COUNT>(row.handler as *const ()).unwrap_or(row.handler), ..row })
            } else if quench_runtime::staged::local_read_family(op)
                && function.arguments_slot == u16::try_from(instruction.local_slot()).ok()
                && let Some(kept) = *kept_arguments.get_or_init(|| function.kept_arguments())
            {
                // The binding a keeping activation may leave unmaterialized: its local read's row answers it
                // (`Frame::arguments_binding`). The object is made where no forwarding call follows the read
                // and the activation keeps its values to its last read (`KeptArguments::Passes`); else the marker.
                let forwards = decoded.get(pc + 1).is_some_and(|next| {
                    quench_runtime::staged::forwarded_call_family(next.op()) && next.call_window().last() == Some(instruction.result_register())
                });
                let object = kept == quench_runtime::bytecode::KeptArguments::Passes && !forwards;
                tables[usize::from(backward_jump)][usize::from(acc)].get(op as usize).copied().flatten()
                    .and_then(|row| lane_frame::kept_row(row.handler as *const (), object)
                        .map(|handler| LaneHandler { handler, ..row }))
            } else if quench_runtime::staged::calls(op) && iteration_step_in_lane(&decoded, pc) {
                Some(LaneHandler {
                    handler: match for_of_step_in_lane(&decoded, pc) {
                        Some(ForOfTest::Emitted) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::READS_EMITTED as u16 }>(),
                        Some(ForOfTest::Threaded) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::READS_THREADED as u16 }>(),
                        None if binding_step_in_lane(&decoded, pc) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::BINDING as u16 }>(),
                        None => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::STEP as u16 }>(),
                    },
                    produces: false,
                })
            } else if spreads_rest_view(instruction) {
                Some(LaneHandler { handler: call_rest_miss as Handler, produces: false })
            } else if quench_runtime::staged::writes_environment_slot(op) && resolves_names {
                None
            } else if quench_runtime::staged::checked_local(op)
                && !plain_frame_local(function, instruction.local_slot())
            {
                // A slot an environment, a mapped `arguments` or the self binding holds keeps its hand
                // handler; `specialize_frame_locals` then gives it the handler of its binding facts.
                Some(LaneHandler { handler: lane_frame::checked_local_hand(op) as Handler, produces: false })
            } else if quench_runtime::staged::reads_named_field(op) && sinks_field_in_lane(instruction) {
                Some(LaneHandler {
                    handler: generated_row!(FIELD_LEAF_READ, ACC_NONE, SINK_SITE) as Handler,
                    produces: false,
                })
            } else if op.immediate_layout() == ImmediateLayout::ConstructCountAndFlags && ImmediateLayout::super_construct(instruction.imm()) {
                // `super(...)`: the parent pushed in place in the frame the lane runs (`derived_frames`).
                Some(LaneHandler { handler: derived_frames::super_construct_in_frame as Handler, produces: false })
            } else if let Some(kind) = returning_kind(instruction) {
                Some(LaneHandler { handler: returning_handler(kind), produces: false })
            } else if let Some(handler) = wasm.then(|| wasm_lane_handler(instruction)).flatten() {
                handler
            } else {
                tables[usize::from(backward_jump)][usize::from(acc)]
                    .get(op as usize)
                    .copied()
                    .flatten()
            });
            // A generated row reads its record's `a` as its result register (`LaneFields::wide`);
            // a record whose `a` carries result flags (a numeric local target) runs the general
            // step, which decodes them, so no row tests for them.
            let row = tables[usize::from(backward_jump)][usize::from(acc)].get(op as usize).copied().flatten();
            let selected = selected.map(|chosen| {
                let flagged = admits_row(op)
                    && matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister)
                    && instruction.a() & !quench_runtime::bytecode::REGISTER_MASK != 0;
                if flagged && row.is_some_and(|row| row.handler as *const () == chosen.handler as *const ()) {
                    LaneHandler { handler: general_step_in_lane, ..chosen }
                } else {
                    chosen
                }
            });
            producer = selected
                .filter(|selected| selected.produces)
                .map(|_| instruction.a());
            produced.push(producer);
            exits.push(selected.is_none());
            reads_accumulator.push(selected.is_some() && acc != ACC_NONE);
            accumulator_registers.push(
                carried.filter(|_| selected.is_some() && acc != ACC_NONE).unwrap_or(NO_ACCUMULATOR),
            );
            let handler = selected.map_or(exit_lane as Handler, |selected| selected.handler) as *const ();
            LaneInstruction {
                handler,
                operand: operand.unwrap_or(LaneOperand { imm: 0 }),
                a: instruction.a(),
                b: instruction.b(),
                // A kept argument read finds the values after the function's registers, whose
                // count its unused field carries (`ARGUMENT_GET_ROW`).
                c: if op as u16 == quench_runtime::staged::ARGUMENT_GET_ROW {
                    function.registers as u16
                } else {
                    instruction.c()
                },
                call_link: std::cell::Cell::new(NO_CALL_LINK),
                mode: 0,
            }
        })
        .collect();
    // Each record's handler before any fusion: a record still holding it
    // runs only its own instruction.
    let mut selected: Vec<*const ()> = records.iter().map(|record| record.handler).collect();
    // Whether a record's handler serves `op` with accumulator field `acc`.
    let serves = |handler: *const (), op: u16, acc: u8| {
        tables[0][usize::from(acc)]
            .get(usize::from(op))
            .copied()
            .flatten()
            .is_some_and(|served| handler == served.handler as *const ())
    };
    // The direction and accumulator field (among `accs`) of the plain handler serving `op`
    // that `handler` is, for an instruction whose handler depends on its jump's direction.
    let plain_served = |handler: *const (), op: u16, accs: &[u8]| {
        [false, true].into_iter().find_map(|backward| {
            accs.iter().copied().find(|&acc| {
                tables[usize::from(backward)][usize::from(acc)]
                    .get(usize::from(op))
                    .copied()
                    .flatten()
                    .is_some_and(|served| handler == served.handler as *const ())
            })
            .map(|acc| (backward, acc))
        })
    };
    // Runs of vector instructions execute as one record each (`vector_region`): placed
    // before the fusions below, which then leave each region's first record alone. Only a
    // body with SIMD operators derives the liveness they read.
    let vectors = wasm
        && decoded.iter().any(|instruction| quench_runtime::staged::wasm_simd(instruction.op()) || quench_runtime::staged::wasm_simd_shuffle(instruction.op()));
    let vector_regions: Box<[vector_region::VectorRegion]> = match vectors.then(live).flatten() {
        Some(live) => vector_region::vector_regions(
            &decoded,
            constants,
            heap,
            memory_register,
            live,
            &entries,
            &exits,
            &produced,
        ),
        None => Vec::new(),
    }
    .into_boxed_slice();
    for region in &vector_regions {
        records[region.first()].handler = vector_region::region_handler() as *const ();
        records[region.first()].operand = LaneOperand { region };
    }
    if wasm {
        // A table follows its selected entry's jump, so every entry must run in
        // the lane.
        for (pc, &instruction) in decoded.iter().enumerate() {
            if quench_runtime::staged::wasm_branch_table(instruction.op())
                && quench_runtime::bytecode::branch_table_entries(pc, instruction).any(|entry| exits[entry])
            {
                records[pc].handler = exit_lane as Handler as *const ();
                exits[pc] = true;
                reads_accumulator[pc] = false;
                accumulator_registers[pc] = NO_ACCUMULATOR;
            }
        }
    }
    // A record's plain jump, with its direction.
    let jump_direction = |handler: *const ()| {
        [false, true].into_iter().find(|&backward| {
            tables[usize::from(backward)][usize::from(ACC_NONE)][quench_runtime::staged::JUMP_ROW as usize]
                .is_some_and(|jump| handler == jump.handler as *const ())
        })
    };
    let _entry_jumps = branch_table_entry_records(&decoded);
    // A plain move or add of a constant whose next record jumps fuses with that jump
    // (`staged::FUSED_ROWS`, `fuse_rows`), except at a hot OSR latch: its jump record stays
    // visible to `install_loop_sites`, which runs before the fusion pass.
    let _then_fuses = |records: &[LaneInstruction], pc: usize| {
        pc + 1 < records.len()
            && (serves(records[pc].handler, quench_runtime::staged::MOVE_ROW, ACC_NONE) || serves(records[pc].handler, quench_runtime::staged::WASM_I32_ADD_IMMEDIATE_ROW, ACC_NONE))
            && jump_direction(records[pc + 1].handler).is_some_and(|backward| !(loop_tier && backward))
    };
    specialize_returning_binaries(&decoded, constants, &mut records);
    specialize_frame_locals(function, &mut records);
    specialize_captures(&mut records);
    // A Wasm view's records take no JavaScript context (`script_handler!`): it has no environment windows.
    let environment_window = (kind != quench_runtime::bytecode::ProgramKind::Wasm)
        .then(|| specialize_environment_window(function, &decoded, &mut records)).flatten();
    let (loops, osr_rejections) = if loop_tier && kind != quench_runtime::bytecode::ProgramKind::Wasm {
        install_loop_sites(
            function_id,
            function,
            &decoded,
            constants,
            sites,
            live,
            (diagnostics, tiers),
            &mut records,
        )
    } else {
        Default::default()
    };
    // The fused records are script handlers; Wasm bodies run none.
    if !wasm {
        specialize_constant_returns(&mut records);
        specialize_script_calls(&mut records, function.undefined_register);
        specialize_proved_arrays(&decoded, &mut records);
    }
    // Loop kernels take their test records last, keeping each record's final handler.
    // Every mode runs them: a loop a kernel takes keeps no tier site (`loop_sites`), since the
    // compiled tiers project the interpreter's fastest form of the loop, its kernel.
    let loop_kernels = if !wasm {
        loop_kernel::install_loop_kernels(&decoded, constants, function, &exits, &mut records, live().map(|masks| &**masks))
    } else {
        Box::default()
    };
    // Adjacent generated rows the staged pair list names run as one generated record.
    // Wasm records fuse too (a bit-field extraction then its i32 operator).
    // A plain `Binary` record with no accumulator field runs its operator's row rather than fusing
    // (`operator_row`): its operator and operand tests fold, which a fused pair's or chain's run-time
    // operator does not. Records that read the accumulator wait for the pass at the end, past the
    // accumulator hazard check, which recognizes only generated rows.
    // The int32 kinds of the registers (`int_kinds`), which pick the operator records' proven forms.
    // A loop kernel or vector region writes its registers from its own code, under no kind.
    let int_kinds = (!wasm && loop_kernels.is_empty() && vector_regions.is_empty())
        .then(|| int_kinds::analyze(&decoded, constants, live().map(|masks| &**masks), usize::from(function.registers),
            resolves_names, plan.speculates))
        .flatten();
    let mut int_speculating = Vec::new();
    if !wasm {
        let before: Vec<*const ()> = records.iter().map(|record| record.handler).collect();
        specialize_plain_operators(&decoded, constants, &mut records, true, int_kinds.as_ref(), &mut int_speculating);
        for ((record, selected), before) in records.iter().zip(selected.iter_mut()).zip(before) {
            if record.handler != before {
                *selected = record.handler;
            }
        }
    }
    let mut fused = lane_frame::fuse_chains(&mut records, &decoded, live().map(|masks| &**masks), function.handlers.is_empty());
    fuse_target_backedges(&mut records, loop_tier);
    fused.extend(lane_frame::fuse_rows(&mut records, &decoded, live().map(|masks| &**masks), loop_tier, function.handlers.is_empty()));
    if let Some(kinds) = int_kinds.as_ref() {
        prove_loop_steps(&decoded, &mut records, kinds, &mut int_speculating);
    }
    // A record reads the accumulator its predecessor produced only when control reaches it
    // from that predecessor running its own handler. A fusion replaces a record's handler, or
    // runs records ahead of it and lands past them, without carrying what the replaced or
    // skipped producers wrote (a fused postfix update skips its `IncDec` row), so a plain
    // accumulator reader within reach of a changed record reads its operand from the register.
    // A fused record that ends at the reader's predecessor publishes that predecessor's carry-out
    // (its last link's `Carried`, `fused_rows`, `chain_rows`), so it does not count as a change
    // there, as long as the link publishes the register the reader takes. A change that reports no
    // span (`fuse_target_backedges`) counts as one.
    if !wasm {
        const FUSED_REACH: usize = 6;
        let changed: Vec<bool> = records.iter().zip(&selected).map(|(record, &selected)| record.handler != selected).collect();
        let mut fused_last: Vec<Option<usize>> = vec![None; records.len()];
        for (head, last) in fused {
            fused_last[head] = Some(last);
        }
        for pc in 1..records.len() {
            let publishes = produced[pc - 1].is_some() && lane_frame::row_produces(decoded[pc - 1].op());
            // Whether the change at `head` can leave the reader at `pc` without the carry it expects.
            let hazard = |head: usize| match fused_last[head] {
                Some(last) if last + 1 < pc => false,
                Some(last) if last + 1 == pc => !publishes,
                _ => true,
            };
            if !(pc.saturating_sub(FUSED_REACH)..pc).any(|head| changed[head] && hazard(head)) {
                continue;
            }
            let op = decoded[pc].op() as u16;
            let Some((backward, _)) = plain_served(records[pc].handler, op, &[ACC_A, ACC_B, ACC_C, ACC_PAIR]) else { continue };
            if let Some(plain) = tables[usize::from(backward)][usize::from(ACC_NONE)].get(usize::from(op)).copied().flatten() {
                records[pc].handler = plain.handler as *const ();
            }
        }
    }
    // Last, so the fusions above and the accumulator check see each record's generated row.
    if !wasm {
        specialize_plain_operators(&decoded, constants, &mut records, false, None, &mut int_speculating);
        specialize_register_operands(&mut records);
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
    let runs = runs_ahead_in_lane(&decoded, &exits, min_run, !wasm);
    let mut entries = vec![0u64; runs.len().div_ceil(u64::BITS as usize)];
    for (pc, (run, reads)) in runs.iter().zip(&reads_accumulator).enumerate() {
        if *run >= min_run && !reads {
            entries[pc / u64::BITS as usize] |= 1 << (pc % u64::BITS as usize);
        }
    }
    let entries = entries.into_boxed_slice();
    let returns_from_entry = reads_accumulator.first() == Some(&false) && entry_run_returns(&decoded, &exits);
    let activation = ActivationShape::of(function);
    let hinted_records = |names: bool| -> Box<[u32]> {
        decoded
            .iter()
            .enumerate()
            .filter(|&(pc, instruction)| {
                holds_field_hint(instruction.op())
                    && is_global_name_access(instruction.op()) == names
                    && !loops.iter().any(|site| site.hookable && site.header as usize == pc)
            })
            .map(|(pc, _)| pc as u32)
            .collect()
    };
    let (field_records, name_records) = (hinted_records(false), hinted_records(true));
    let string_case_filters = string_case_filter::specialize(heap, constants, &decoded, &mut records);
    let records = Records::new(records, decoded.iter().fold(false, |seen, instruction| seen || has_operand_feedback(instruction.op())));
    seed_declared_feedback(function, &records);
    let first = records.base();
    // The first loop closing at a backedge owns it, as a front-to-back search would find it.
    let mut loop_at_backedge: Vec<(u32, u32)> = loops.iter().enumerate().map(|(index, site)| (site.backedge, index as u32)).collect();
    loop_at_backedge.sort_unstable();
    loop_at_backedge.dedup_by_key(|&mut (backedge, _)| backedge);
    LaneView {
        records,
        entries,
        tiers,
        memory_register,
        activation,
        target_body: std::cell::OnceCell::new(),
        _string_case_filters: string_case_filters,
        environment_window,
        returns_from_entry,
        loops,
        loop_at_backedge: loop_at_backedge.into_boxed_slice(),
        vector_regions,
        _loop_kernels: loop_kernels.into_iter().map(|kernel| kernel as Box<dyn std::any::Any>).collect(),
        start: std::cell::Cell::new(first),
        prologue: std::cell::UnsafeCell::new(LaneInstruction {
            handler: std::ptr::null(),
            operand: LaneOperand { imm: 0 },
            a: 0,
            b: 0,
            c: 0,
            call_link: std::cell::Cell::new(NO_CALL_LINK),
            mode: 0,
        }),
        tier_direct: std::cell::Cell::new(None),
        tier: std::cell::OnceCell::new(),
        entry: crate::compiled::entry_cell::EntryCell::new(),
        accumulator_registers: accumulator_registers.into_boxed_slice(),
        plain_backward_jump_sites: decoded
            .iter()
            .enumerate()
            .filter(|(pc, instruction)| quench_runtime::staged::jumps(instruction.op()) && instruction.jump_target() <= *pc as u32)
            .count(),
        osr_rejections: (!osr_rejections.is_empty())
            .then(|| osr_rejections.into_boxed_slice()),
        field_records,
        name_records,
        int_speculating: int_speculating.into_boxed_slice(),
    }
}

/// Whether a record holds an own-field hint (`LaneInstruction::field_hint`) in a `field`
/// operand: every field read or store's of its receiver, and a global name access's or top-level
/// `var` capture read's of the global object (`name_row`, `lane_load_root_var`).
fn holds_field_hint(op: Op) -> bool {
    quench_runtime::staged::stores_field(op) || quench_runtime::staged::reads_named_field(op) || quench_runtime::staged::reads_capture(op) || is_global_name_access(op)
}

/// Whether an instruction is a by-name load (`LOAD_NAME`) or store, whose hint a global
/// lexical declaration clears (`LaneView::clear_name_hints`).
fn is_global_name_access(op: Op) -> bool {
    op.traits().has(quench_runtime::bytecode::Traits::GLOBAL_NAME_ACCESS)
}

/// Whether an instruction's record leaves its `call_link` unread: any but a call's, whose
/// handlers keep their target hint there (`linked_script_call_entry`).
fn holds_no_call_link(op: Op) -> bool {
    !op.traits().has(quench_runtime::bytecode::Traits::CALL_LINK)
}

/// The handler of a loop site's backward jump: the jump row in the loop's flag (`FLAG_LOOP`), which
/// takes the jump while the site warms up and leaves any other to `loop_jump_miss`.
fn loop_jump_row() -> Handler {
    row_jump!(JUMP, true, FLAG_LOOP, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }) as Handler
}

/// Gives each backward `Jump` whose loop the shared compiler admits (`analyze_osr_region`) the
/// record that counts it and enters its native code (`loop_jump_row`). Runs after every
/// other pass, which read a jump record's distance from its operand.
fn install_loop_sites<'live>(
    function_id: u32,
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[quench_runtime::bytecode::FieldSite],
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    (diagnostics, tiers): (bool, &'static Tiers),
    records: &mut [LaneInstruction],
) -> (Box<[LoopSite]>, Vec<(crate::compiled::RegionRejection, usize)>) {
    let backward = jump_row::<JUMP>(true) as *const ();
    let table_entries = branch_table_entry_records(decoded);
    // A fused record ahead of a jump runs that jump itself, reading its offset from the jump's
    // record: a site there would replace the offset those records read with the site (the eslint
    // npm scenario under --jit jumped to a site address read as an offset). The sites are backward
    // jumps, and a move or add never fuses into a hot OSR latch (`then_fuses`), so only a move
    // chain can sit ahead of one.
    let fused_into = |pc: usize| {
        let pair = pc >= 2
            && records[pc - 2].handler == move_pair_jump_handler() as *const ();
        // A branch table takes its entry's jump offset the same way (`row_jump`, `Edge::Entry`):
        // babel-minify-wtb's generator `switch` under --jit jumped to a site read as an offset.
        pair || table_entries.contains(pc)
    };
    // The masks are derived for the first loop site only: a body without a backward jump
    // needs none, and every view under a loop tier had derived them (npm scenarios under
    // --jit: axios -3.6%, playwright -2.5% instructions).
    let masks = std::cell::OnceCell::new();
    let live = || *masks.get_or_init(live);
    let mut rejections = Vec::new();
    let sites: Vec<(usize, LoopSite)> = (0..records.len())
        .filter(|pc| records[*pc].handler == backward && matches!(quench_runtime::staged::Operation::<(), ()>::decode(decoded[*pc], quench_runtime::staged::DecodeContext::EMPTY), Some(quench_runtime::staged::Operation::Jump)) && !fused_into(*pc))
        .map(|pc| {
            let header = decoded[pc].jump_target();
            let (region, promotion_only, analysis_failure, call_boundary_pcs) = match crate::compiled::analyze_osr_region_detailed(
                function,
                decoded,
                constants,
                sites,
                live(),
                header,
                pc as u32, crate::compiled::MethodCalls::Inliner, &|_| false,
            ) {
                Ok(region) => {
                    let call_boundary_pcs = region.call_boundary_pcs();
                    (std::cell::OnceCell::from(region), false, None, call_boundary_pcs)
                }
                // A call may run inlined: the site makes its region once it is hot.
                Err(failure)
                    if failure.is_call_boundary() && live().is_some() =>
                {
                    (
                        std::cell::OnceCell::new(),
                        false,
                        Some(failure),
                        vec![failure.pc],
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
                    // A loop the region analysis rejects keeps its site, diagnostics or not: its
                    // function's lane body may run it (`loop_retire_in_lane`), and the
                    // site retires to the plain jump where none does. A diagnostics run tiers
                    // as any other.
                    (std::cell::OnceCell::new(), false, Some(failure), Vec::new())
                }
            };
            (
                pc,
                LoopSite {
                    function: function_id,
                    promotion_only,
                    analysis_failure,
                    call_boundary_pcs: call_boundary_pcs.into_boxed_slice(),
                    region,
                    inline_region: std::cell::OnceCell::new(),
                    header,
                    backedge: pc as u32,
                    jump: records[pc].jump(),
                    header_handler: std::cell::Cell::new(std::ptr::null()),
                    hookable: holds_no_call_link(decoded[header as usize].op()),
                    taken: std::cell::Cell::new(0),
                    needed: std::cell::Cell::new(tiers.loops.warmup_taken),
                    entering: std::cell::Cell::new(true),
                    enclosing: std::cell::OnceCell::new(),
                    declines_in_row: std::cell::Cell::new(0),
                    declines: Default::default(),
                    skipped_entries: std::cell::Cell::new(0),
                    diagnostics,
                    tiers,
                    lane_backedge_count: std::cell::Cell::new(0),
                    native_entry_attempts: std::cell::Cell::new(0),
                    native_region_runs: std::cell::Cell::new(0),
                    tier_ups: std::cell::Cell::new(0),
                    body_runs: std::cell::Cell::new(0),
                    entry_guard_misses: std::cell::Cell::new(0),
                    native_exits: std::cell::Cell::new(0),
                    widenings: std::cell::Cell::new(0),
                    compile_attempts: std::cell::Cell::new(0),
                    compile_successes: std::cell::Cell::new(0),
                    tier: std::cell::RefCell::default(), body: std::cell::RefCell::new(LoopBody::initial(tiers)), body_seen: std::cell::Cell::new(0), body_wait: std::cell::Cell::new(0), body_offered: std::cell::Cell::new(false),
                    refused_epoch: std::cell::Cell::new(loop_runs::NO_REFUSAL), refused_view: std::cell::Cell::new(std::ptr::null()), refused_wait: std::cell::Cell::new(0), refused_span: std::cell::Cell::new(0), enclosed: std::cell::Cell::new(true),
                },
            )
        })
        .collect();
    let mut sites = sites;
    // The tiers run a loop the interpreter's kernel takes slower than the kernel does (`Recurrence::beats_rows`): in
    // every mode that loop keeps no site, so its kernel runs it (`install_loop_kernels`).
    sites.retain(|(pc, site)| !loop_kernel::LoopKernel::beats_rows(decoded, constants, function, (site.header as usize, *pc), live().map(|masks| &**masks)));
    let headers: Vec<u32> = sites.iter().map(|(_, site)| site.header).collect();
    for (pc, site) in &mut sites {
        // The header names its site by the distance to the site's jump (`header_site_in_lane`).
        site.hookable &= headers.iter().filter(|header| **header == site.header).count() == 1
            && u16::try_from(*pc - site.header as usize).is_ok_and(|distance| distance < NO_CALL_LINK);
    }
    let pcs: Vec<usize> = sites.iter().map(|(pc, _)| *pc).collect();
    let sites: Box<[LoopSite]> = sites.into_iter().map(|(_, site)| site).collect();
    // The loops each site lies inside that the region analysis admits: such a loop takes the region, and the
    // site runs as plain jumps inside it (`LoopSite::inside_admitted_loop`), so the outermost admitted loop
    // is the one entered per run, not an inner loop re-entered per outer iteration.
    for (index, site) in sites.iter().enumerate() {
        let enclosing: Box<[*const LoopSite]> = sites.iter().enumerate()
            .filter(|(other, outer)| *other != index && outer.header <= site.header && site.backedge <= outer.backedge && outer.is_admitted())
            .map(|(_, outer)| std::ptr::from_ref(outer))
            .collect();
        let _ = site.enclosing.set(enclosing);
    }
    for (pc, site) in pcs.into_iter().zip(sites.iter()) {
        records[pc].handler = loop_jump_row() as *const ();
        let placed = records[pc].set_loop_site(site);
        debug_assert!(placed, "the site's address fits 48 bits");
        if site.hookable {
            records[site.header as usize].call_link.set((pc - site.header as usize) as u16);
        }
    }
    (sites, rejections)
}



/// Gives each `Binary` record that a `Return` consumes its returning form
/// (`specialize_returning_operator`, the calls thread's). Every other record keeps the handler
/// its generated row or jump row gave it: operand kinds and operators are the rows' own decode
/// (`Binary`, `JumpBinaryFalse`, `Unary`), not handler choices made here.
fn specialize_returning_binaries(decoded: &[WideInstruction], constants: &[Value], records: &mut [LaneInstruction]) {
    let returning_binary = returning_row_of!(quench_runtime::bytecode::BINARY_ROW, RETURNING_BINARY, NOT_LEAF) as *const ();
    for (pc, &instruction) in decoded.iter().enumerate() {
        if records[pc].handler == returning_binary {
            specialize_returning_operator(instruction, constants, &mut records[pc]);
        }
    }
}

/// Whether `instruction` is a plain `Binary` of an arithmetic operator (`arithmetic_operators!`), which
/// `specialize_plain_operators` gives its operator's row.
fn arithmetic_binary(instruction: WideInstruction) -> bool {
    macro_rules! pick { ($op:expr) => { () }; }
    quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Binary) && instruction.result_flags() == 0
        && arithmetic_operators!(instruction.binary_operator(), pick).is_some()
}

/// Gives each plain `Binary` record of an arithmetic operator that still runs its generated row, at
/// any accumulator field, that operator's `operator_row`: the operator becomes a parameter of the
/// instantiation, so the row neither decodes nor dispatches on it (179 against 64 instructions for an
/// int32 add). Its operand kinds and any int32 constant operand pick the form (`OPERANDS_*`). Its misses go back to the
/// generated row, so every operand pair stays decided by the one staged row.
///
/// Where the registers' int32 kinds (`kinds`) prove every register operand of a record with no
/// accumulator field an int32, it takes the proven form (`OPERANDS_PROVEN`, `OPERANDS_PROVEN_INT32`),
/// which reads them with no tag test; one that reads a speculative register is listed in
/// `speculating` with its checked form (`LaneView::retire_int_speculation`).
fn specialize_plain_operators(decoded: &[WideInstruction], constants: &[Value], records: &mut [LaneInstruction], accumulator_free: bool,
    kinds: Option<&int_kinds::IntKinds>, speculating: &mut Vec<(u32, *const (), *const ())>) {
    use quench_runtime::bytecode::OperandKind::{Constant, Register};
    use lane_frame::{OPERANDS_ANY, OPERANDS_INT32, OPERANDS_PROVEN, OPERANDS_PROVEN_INT32, OPERANDS_REGISTERS, OPERANDS_REGISTER_CONSTANT, OPERANDS_REGISTER_INT32};
    macro_rules! row_at {
        ($acc:expr) => {
            generated_row!(quench_runtime::staged::BINARY_ROW, $acc, FLAG_UNREAD) as Handler as *const ()
        };
    }
    let plain = [row_at!(ACC_NONE), row_at!(ACC_B), row_at!(ACC_C)];
    for (pc, &instruction) in decoded.iter().enumerate() {
        let Some(acc) = plain.iter().position(|row| *row == records[pc].handler) else { continue };
        if accumulator_free && acc != 0 {
            continue;
        }
        if instruction.result_flags() != 0 || instruction.numeric_local_target().is_some() {
            continue;
        }
        let int32_constant = |operand: quench_runtime::bytecode::Operand| operand.kind() == Some(Constant)
            && constants.get(usize::from(operand.payload())).is_some_and(|constant| constant.as_int().is_some());
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        let form = match (left.kind(), right.kind()) {
            (Some(Register), Some(Register)) => OPERANDS_REGISTERS,
            (Some(Register), Some(Constant)) if int32_constant(right) => OPERANDS_REGISTER_INT32,
            (Some(Register), Some(Constant)) => OPERANDS_REGISTER_CONSTANT,
            _ if int32_constant(left) || int32_constant(right) => OPERANDS_INT32,
            _ => OPERANDS_ANY,
        };
        // The int32 form's constant, folded into the record (`operator_row` reads it from `mode`, which
        // no fused record has here: fusion runs after this pass and assigns `mode` with its handler).
        if form == OPERANDS_REGISTER_INT32 && let Some(constant) = constants.get(usize::from(right.payload())).and_then(|constant| constant.as_int()) {
            records[pc].mode = u64::from(constant as u32);
        }
        macro_rules! at_form {
            ($acc:expr, $op:expr) => {
                match form {
                    OPERANDS_REGISTERS => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_REGISTERS> as Handler,
                    OPERANDS_REGISTER_INT32 => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_REGISTER_INT32> as Handler,
                    OPERANDS_REGISTER_CONSTANT => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_REGISTER_CONSTANT> as Handler,
                    OPERANDS_INT32 => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_INT32> as Handler,
                    _ => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_ANY> as Handler,
                }
            };
        }
        macro_rules! pick {
            ($op:expr) => {
                match acc {
                    0 => at_form!(ACC_NONE, $op),
                    1 => at_form!(ACC_B, $op),
                    _ => at_form!(ACC_C, $op),
                }
            };
        }
        if let Some(handler) = arithmetic_operators!(instruction.binary_operator(), pick) {
            records[pc].handler = handler as *const ();
            // The proven form, where the kinds prove each register operand an int32.
            let proven = |operand: quench_runtime::bytecode::Operand| operand.register_index().is_some_and(|register| kinds.is_some_and(|kinds| kinds.proven(register)));
            let speculative = |operand: quench_runtime::bytecode::Operand| operand.register_index().is_some_and(|register| kinds.is_some_and(|kinds| kinds.of(register) == int_kinds::Kind::Speculative));
            let proven_form = match form {
                OPERANDS_REGISTERS if acc == 0 && proven(left) && proven(right) => Some(OPERANDS_PROVEN),
                OPERANDS_REGISTER_INT32 if acc == 0 && proven(left) => Some(OPERANDS_PROVEN_INT32),
                _ => None,
            };
            macro_rules! proven_at {
                ($op:expr) => {
                    if proven_form == Some(OPERANDS_PROVEN) {
                        operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, ACC_NONE, { $op as u8 }, OPERANDS_PROVEN> as Handler
                    } else {
                        operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, ACC_NONE, { $op as u8 }, OPERANDS_PROVEN_INT32> as Handler
                    }
                };
            }
            if proven_form.is_some() && let Some(proven_handler) = arithmetic_operators!(instruction.binary_operator(), proven_at) {
                if speculative(left) || speculative(right) {
                    speculating.push((pc as u32, proven_handler as *const (), handler as *const ()));
                }
                records[pc].handler = proven_handler as *const ();
            }
        } else if instruction.binary_operator() == oxc_ast::ast::BinaryOperator::StrictEquality as u32 {
            records[pc].handler = strict_equality_row::<false> as Handler as *const ();
        } else if instruction.binary_operator() == oxc_ast::ast::BinaryOperator::StrictInequality as u32 {
            records[pc].handler = strict_equality_row::<true> as Handler as *const ();
        }
    }
}

/// Runs each `LoadConst` whose register the next record, a `Return`, returns as one record
/// (`return_constant_miss`). The `Return` record stays for jumps that reach it, and the
/// fused record's general return continues there.
fn specialize_constant_returns(records: &mut [LaneInstruction]) {
    let load = generated_row!(quench_runtime::staged::LOAD_CONST_ROW, ACC_NONE, FLAG_UNREAD) as Handler as *const ();
    let ret = return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(RETURN_KIND, NOT_LEAF)) as *const ();
    for pc in 0..records.len().saturating_sub(1) {
        if records[pc].handler == load && records[pc + 1].handler == ret && records[pc + 1].a == records[pc].a {
            records[pc].handler = return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, NOT_LEAF)) as *const ();
        }
    }
}

pub(crate) mod loop_kernel;
pub(crate) mod int_kinds;

/// The proved row of the element record running `handler` (`specialize_proved_arrays`): the row of
/// the same opcode with `FLAG_SET`, which reads or replaces its base's element through the proof of
/// the array's storage (`proof_span`) and runs its own full check where that declines.
fn proved_row(handler: *const ()) -> Option<Handler> {
    if plain_row::<ELEMENT_READ>(handler) {
        return Some(generated_row!(ELEMENT_READ, ACC_NONE, FLAG_SET));
    }
    // A store keeps its generic record (`set_index_miss`) until a proof names it: the keyed store
    // has no row of its own in the lane, so this is the one place a store's row is installed.
    if handler == protocol_row!(ELEMENT_WRITE, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::SET_INDEX as u16 }) as Handler as *const () {
        return Some(generated_row!(ELEMENT_WRITE, ACC_NONE, FLAG_SET));
    }
    None
}

/// The proved element read row (`proved_row` of `ELEMENT_READ`), which the primitive bench counts.
#[cfg(test)]
pub(crate) fn proved_element_read_row() -> *const () { generated_row!(ELEMENT_READ, ACC_NONE, FLAG_SET) as Handler as *const () }

/// The proved element write row (`proved_row` of `ELEMENT_WRITE`), which the primitive bench counts.
#[cfg(test)]
pub(crate) fn proved_element_write_row() -> *const () { generated_row!(ELEMENT_WRITE, ACC_NONE, FLAG_SET) as Handler as *const () }

/// The span of the array a proved element row (`proved_row`) reads or stores through the proof of its
/// base register's slot (`ArrayStorageProofs::read_span`, `write_span`), proving the array first where
/// the slot holds another. `None` where the array takes the row's own check instead.
///
/// # Safety
/// `context` is the running lane context of `H`, live for the row.
unsafe fn proof_span(context: *mut u8, register: u16, base: Value, writable: bool)
    -> Option<(std::ptr::NonNull<Value>, usize)> {
    let cx = context.cast::<LaneContext>();
    // SAFETY: the context's proofs and its VM are distinct, both live for the run.
    let (proofs, vm) = unsafe { (&mut (*cx).proofs, vm(cx)) };
    if !proofs.holds(register, base) && !vm.prove_array_storage(proofs, register, base) {
        return None;
    }
    if writable { proofs.write_span(register, base) } else { proofs.read_span(register, base) }
}

/// Gives each element read and store whose register base its innermost loop does not
/// redefine the proved row (`proved_row`), which reaches the element through the
/// run's proof of the array's storage (`ArrayStorageProofs`) instead of decoding the array:
/// the loop proves the array once, at its first access, and each access after checks only
/// that its slot still proves the base. A base whose proof slot another such base of the
/// same loop takes keeps its record, as their accesses would evict each other's proofs.
/// V8-v7 NavierStokes, whose `lin_solve` row loop reads four elements and stores one each
/// iteration: 795.3 -> 717.4 instructions per element stored, Callgrind, no-JIT static
/// production build, compile cache and startup snapshot off, x86_64 4 vCPU, 2026-11-01;
/// Crypto -4.7% instructions at fixed iterations, the other six within +0.06%.
///
/// The proofs alone decide soundness, whatever control flow does: a base the loop
/// redefines through a definition `defined_register` does not report only misses its
/// proof, and its record takes the generic one. Aggregate profiles count each generic
/// store, so they keep the generic records.
fn specialize_proved_arrays(decoded: &[WideInstruction], records: &mut [LaneInstruction]) {
    if cfg!(feature = "profile-aggregate") {
        return;
    }
    let loops: Vec<std::ops::RangeInclusive<usize>> = decoded
        .iter()
        .enumerate()
        .filter(|(pc, instruction)| {
            instruction.op().immediate_role() == ImmediateRole::JumpTarget && instruction.imm() as usize <= *pc
        })
        .map(|(pc, instruction)| instruction.imm() as usize..=pc)
        .collect();
    // Each candidate: its record, proved row, base register and innermost loop.
    let candidates: Vec<(usize, Handler, u16, usize)> = (0..records.len())
        .filter_map(|pc| {
            let proved = proved_row(records[pc].handler)?;
            let base = records[pc].b;
            let (index, _) = loops
                .iter()
                .enumerate()
                .filter(|(_, body)| body.contains(&pc))
                .min_by_key(|(_, body)| body.end() - body.start())?;
            decoded[loops[index].clone()]
                .iter()
                .all(|instruction| instruction.defined_register() != Some(base))
                .then_some((pc, proved, base, index))
        })
        .collect();
    for &(pc, proved, base, index) in &candidates {
        let slot = quench_runtime::tier_surface::ArrayStorageProofs::slot(base);
        let shared = candidates.iter().any(|&(_, _, other, other_index)| {
            other_index == index && other != base && quench_runtime::tier_surface::ArrayStorageProofs::slot(other) == slot
        });
        if !shared {
            records[pc].handler = proved as *const ();
        }
    }
}

/// The `FUSED_ROWS` pairs whose second row is `Jump`, by their index in that table: a row followed
/// by a backward jump, which `FUSED_TARGET` may fuse with the target run at the jump's target.
/// `(StoreLocalPlain, Jump)`, `(Move, Jump)` and `(IncDec, Jump)` are the pairs (the check names each one's second row);
/// not `(WasmI32AddImmediate, Jump)`: only a Wasm body has that add, and a target run's rows are JavaScript rows.
const BACK_EDGE_PAIRS: [usize; 3] = [31, 32, 37];
const _: () = {
    let mut k = 0;
    while k < BACK_EDGE_PAIRS.len() {
        assert!(quench_runtime::staged::FUSED_ROWS[BACK_EDGE_PAIRS[k]].1 == quench_runtime::staged::JUMP_ROW, "a BACK_EDGE_PAIRS entry is not a Jump pair");
        k += 1;
    }
};

/// One `FUSED_TARGET` form of a back edge: whether its target run holds (`holds(target, after)`, where
/// `target` is the target record's handler and `after(k)` the handler of the record `k` places after it,
/// read only by a form with a prefix row), and the handler the back edge's record takes for it.
type TargetForm = (fn(*const (), &dyn Fn(usize) -> *const ()) -> bool, Handler);

/// Whether the target run holds: `PREFIX` (when there is one) at the target, `COPY` (when there is one)
/// after it, then `TEST`. With no prefix, `TEST` is the target record itself.
fn target_run_holds<const PREFIX: u16, const COPY: u16, const TEST: u16>(target: *const (), after: &dyn Fn(usize) -> *const ()) -> bool {
    if const { PREFIX == quench_runtime::staged::NO_LINK } {
        lane_frame::plain_row::<TEST>(target)
    } else if const { COPY == quench_runtime::staged::NO_LINK } {
        lane_frame::plain_row::<PREFIX>(target) && lane_frame::plain_row::<TEST>(after(1))
    } else {
        lane_frame::plain_row::<PREFIX>(target) && lane_frame::plain_row::<COPY>(after(1)) && lane_frame::plain_row::<TEST>(after(2))
    }
}

/// The `FUSED_TARGET` form of the pair `(A, B)`, a back edge whose target run is `PREFIX` then `TEST`:
/// the fused record `fused_rows` makes with that run.
fn pair_target_form<const A: u16, const B: u16, const PREFIX: u16, const TEST: u16>() -> TargetForm {
    (
        target_run_holds::<PREFIX, { quench_runtime::staged::NO_LINK }, TEST> as fn(*const (), &dyn Fn(usize) -> *const ()) -> bool,
        // `B` is a jump whose direction is its distance (`fused_rows` reads `BACKWARD` only for a branch table).
        fused_rows!(A, B, false, { quench_runtime::staged::FUSED_TARGET }, TEST, PREFIX, 0) as Handler,
    )
}

/// The `(IncDec, Jump)` back edge whose target is its loop's test alone, `fused_rows` with the test's
/// comparison fixed (`CMP`, `lane_frame::target_comparison`).
const fn inc_dec_test_row<const CMP: u8>() -> Handler {
    fused_rows!({ quench_runtime::staged::INC_DEC_ROW }, { quench_runtime::staged::JUMP_ROW }, false, { quench_runtime::staged::FUSED_TARGET },
        { quench_runtime::staged::COMPARE_JUMP_ROW }, { quench_runtime::staged::NO_LINK }, CMP) as Handler
}

/// The `(IncDec, Jump)` back edges whose step and test the registers' int32 kinds prove
/// (`int_kinds`): `i++` in place of a register of an int32 kind, then a fixed register-register
/// comparison whose left operand is that register. Each takes its `PROVEN_STEP` form, which reads
/// the counter with no tag test and gives the test its new value as held; one over a speculative
/// counter is listed in `speculating` with its checked form (`LaneView::retire_int_speculation`).
fn prove_loop_steps(decoded: &[WideInstruction], records: &mut [LaneInstruction], kinds: &int_kinds::IntKinds,
    speculating: &mut Vec<(u32, *const (), *const ())>) {
    macro_rules! proven_row {
        ($($code:literal),*) => {
            |code: u8| -> Option<(Handler, Handler)> {
                match code {
                    $($code => Some((inc_dec_test_row::<$code>(), inc_dec_test_row::<{ $code | lane_frame::PROVEN_STEP }>())),)*
                    _ => None,
                }
            }
        };
    }
    // The register-register comparisons' codes (`target_comparison`: odd codes; even ones name a constant).
    let row_of = proven_row!(1, 3, 5, 7, 9, 11);
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    for head in 0..records.len().saturating_sub(1) {
        let Some(code) = (1..=12u8).step_by(2).find(|&code| row_of(code).is_some_and(|(checked, _)| records[head].handler == checked as *const ())) else {
            continue;
        };
        let Some((checked, proven)) = row_of(code) else { continue };
        let step = decoded[head];
        if !quench_runtime::staged::meaning::means(step, quench_runtime::staged::meaning::Meaning::IncDec) || step.numeric_local_target().is_some() || step.boolean_flag() != Some(false)
            || step.result_flags() != 0 || step.result_register() != step.register_b() {
            continue;
        }
        let counter = step.register_b();
        let kind = kinds.of(counter);
        if kind == int_kinds::Kind::Tagged {
            continue;
        }
        let jump = head + 1;
        let Ok(target) = usize::try_from(jump as isize + records[jump].jump() / record) else { continue };
        let Some(&test) = decoded.get(target) else { continue };
        if !quench_runtime::staged::meaning::means(test, quench_runtime::staged::meaning::Meaning::CompareJump) || Operand(records[target].b).register_index() != Some(counter) {
            continue;
        }
        if kind == int_kinds::Kind::Speculative {
            speculating.push((head as u32, proven as *const (), checked as *const ()));
        }
        records[head].handler = proven as *const ();
    }
}

/// `handler`, a back edge's `FUSED_TARGET` record, with its test's comparison fixed where it is the
/// `(IncDec, Jump)` pair's run of the test `test` alone and that test is one `target_comparison`
/// fixes: the loop's step then switches on no operator. Any other handler is itself.
fn fixed_comparison(handler: Handler, test: &LaneInstruction) -> Handler {
    if handler as *const () != inc_dec_test_row::<0>() as *const () { return handler }
    match lane_frame::target_comparison(test) {
        1 => inc_dec_test_row::<1>(), 2 => inc_dec_test_row::<2>(), 3 => inc_dec_test_row::<3>(),
        4 => inc_dec_test_row::<4>(), 5 => inc_dec_test_row::<5>(), 6 => inc_dec_test_row::<6>(),
        7 => inc_dec_test_row::<7>(), 8 => inc_dec_test_row::<8>(), 9 => inc_dec_test_row::<9>(),
        10 => inc_dec_test_row::<10>(), 11 => inc_dec_test_row::<11>(), 12 => inc_dec_test_row::<12>(),
        _ => handler,
    }
}

/// The `FUSED_TARGET` form of a lone backward jump whose target run is `PREFIX` then `TEST`: the jump's
/// record `row_jump` with that run (`target_run`).
fn lone_target_form<const PREFIX: u16, const TEST: u16>() -> TargetForm {
    (
        target_run_holds::<PREFIX, { quench_runtime::staged::NO_LINK }, TEST> as fn(*const (), &dyn Fn(usize) -> *const ()) -> bool,
        row_jump!(JUMP, true, FLAG_UNREAD, PREFIX, { quench_runtime::staged::NO_LINK }, TEST) as Handler,
    )
}

/// The `FUSED_TARGET` form of a lone backward jump whose target run is `PREFIX`, a copy (`FUSED_TARGET_COPY_ROWS`)
/// and `TEST`: the jump's record `row_jump` with that run, its copy slot the copy (`target_run`).
fn lone_copy_target_form<const PREFIX: u16, const TEST: u16>() -> TargetForm {
    (
        target_run_holds::<PREFIX, { quench_runtime::staged::FUSED_TARGET_COPY_ROWS[0] }, TEST> as fn(*const (), &dyn Fn(usize) -> *const ()) -> bool,
        row_jump!(JUMP, true, FLAG_UNREAD, PREFIX, { quench_runtime::staged::FUSED_TARGET_COPY_ROWS[0] }, TEST) as Handler,
    )
}

/// The twelve forms of a back edge's target run, in the order: no prefix, then each
/// `FUSED_TARGET_PREFIX_ROWS` row; each with each `FUSED_TARGET_ROWS` test. `$form` takes the
/// leading const arguments (a pair's `A` and `B`), then `PREFIX` and `TEST`.
macro_rules! target_forms {
    ($form:ident $(, $arg:tt)*) => {
        [
            $form::<$($arg,)* { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
            $form::<$($arg,)* { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
            $form::<$($arg,)* { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
            $form::<$($arg,)* { quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
        ]
    };
}

/// The `FUSED_TARGET` forms of the pair `(A, B)`'s back edge.
fn pair_target_forms<const A: u16, const B: u16>() -> [TargetForm; 12] {
    target_forms!(pair_target_form, A, B)
}

/// The `FUSED_TARGET` forms of a lone backward jump.
fn lone_target_forms() -> [TargetForm; 12] {
    target_forms!(lone_target_form)
}

/// The nine `FUSED_TARGET` forms of a lone back jump's copy run: each `FUSED_TARGET_PREFIX_ROWS` row, then
/// the copy, then each `FUSED_TARGET_ROWS` test.
fn lone_copy_target_forms() -> [TargetForm; 9] {
    [
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
    ]
}

/// The handler of the first form of `forms` whose target run holds at `target` (`after` as `target_run_holds`).
fn target_run_handler(forms: &[TargetForm], target: *const (), after: &dyn Fn(usize) -> *const ()) -> Option<Handler> {
    forms.iter().find(|(holds, _)| holds(target, after)).map(|(_, handler)| *handler)
}

/// The handler of a lone back jump's target run at `target`: the first of `lone` that holds, else, where the
/// record after the target is a copy (`FUSED_TARGET_COPY_ROWS`) whose destination the test after it names
/// (`a`, `b` or `c`), the first of `lone_copy` that holds. The copy's register write is the copy's own row's.
fn lone_target_handler(lone: &[TargetForm], lone_copy: &[TargetForm], records: &[LaneInstruction], target: usize, after: &dyn Fn(usize) -> *const ()) -> Option<Handler> {
    let target_handler = records.get(target)?.handler;
    if let Some(handler) = target_run_handler(lone, target_handler, after) {
        return Some(handler);
    }
    let copied = match (records.get(target + 1), records.get(target + 2)) {
        (Some(copy), Some(test)) => [test.a, test.b, test.c].contains(&copy.a),
        _ => false,
    };
    if !copied {
        return None;
    }
    target_run_handler(lone_copy, target_handler, after)
}

/// Gives each backward jump whose target run holds (a test, or a variable read or an update then a test,
/// at the jump's target: `FUSED_TARGET_ROWS`, `FUSED_TARGET_PREFIX_ROWS`) the `FUSED_TARGET` record, in
/// one of two places, the target keeping its own row for every other entry:
/// - a jump after one of `BACK_EDGE_PAIRS`' pairs: the pair's record becomes the pair, the jump and the
///   target run in one dispatch (`pair_target_forms`);
/// - a jump with no pair before it: the jump's own record runs the target run (`lone_target_forms`).
/// Runs after `fuse_chains` and before `fuse_rows`, so the target records are still their own rows, and
/// a lone jump is not rewritten where `fuse_rows` would fuse the record before it with the jump.
///
/// Subsumes `lane_inc_dec_loop` and `lane_inc_dec_small_loop` (an update pair into a register or constant
/// test), `lane_inc_dec_load_loop` (an update pair into a loaded bound, then its test), `lane_jump_back_nullish`
/// and `specialize_nullish_back_edges` (a lone back jump into a nullish test), and `lane_jump_back_update_small`
/// (an update then its test, and its copy-then-test form: a lone back jump whose target is an update, a `Move`
/// the test after it reads, then the test; `lone_copy_target_forms`).
fn fuse_target_backedges(records: &mut [LaneInstruction], osr_loop: bool) {
    macro_rules! pair {
        ($pair:literal) => {
            (
                (|handler| plain_row::<{ quench_runtime::staged::FUSED_ROWS[$pair].0 }>(handler)) as fn(*const ()) -> bool,
                pair_target_forms::<{ quench_runtime::staged::FUSED_ROWS[$pair].0 }, { quench_runtime::staged::FUSED_ROWS[$pair].1 }>(),
            )
        };
    }
    let pairs = [pair!(31), pair!(32), pair!(37)];
    let lone = lone_target_forms();
    let lone_copy = lone_copy_target_forms();
    let back = jump_row::<JUMP>(true) as *const ();
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    for jump in 0..records.len() {
        if records[jump].handler != back {
            continue;
        }
        // The jump's target: its distance is in bytes from the jump record, as `take_jump` reads it.
        let Ok(target) = usize::try_from(jump as isize + records[jump].jump() / record) else {
            continue;
        };
        let Some(target_handler) = records.get(target).map(|record_at| record_at.handler) else {
            continue;
        };
        let after = |k: usize| records.get(target + k).map_or(std::ptr::null(), |record_at| record_at.handler);
        let Some(head) = jump.checked_sub(1) else {
            // No record before the jump: a lone back jump.
            if let Some(handler) = lone_target_handler(&lone, &lone_copy, records, target, &after) {
                records[jump].mode = target_test_word(|k| records.get(target + k));
                records[jump].handler = handler as *const ();
            }
            continue;
        };
        let first = records[head].handler;
        if let Some((_, forms)) = pairs.iter().find(|(first_row, _)| first_row(first)) {
            // A move at a hot OSR latch keeps its own record, as `fuse_rows` keeps it.
            let latch = osr_loop && plain_row::<{ quench_runtime::staged::MOVE_ROW }>(first);
            if !latch {
                if let Some(handler) = target_run_handler(forms, target_handler, &after) {
                    let handler = fixed_comparison(handler, &records[target]);
                    let word = target_word(&records[jump]) | target_test_word(|k| records.get(target + k));
                    records[head].handler = handler as *const ();
                    records[head].mode = word;
                }
            }
            continue;
        }
        // The window pair (`fuse_rows`' `window_pairs`) whose first row is a window local's stores before a jump: leave its jump.
        if lane_frame::is_window_row::<{ quench_runtime::staged::FUSED_ROWS[31].0 }>(first) {
            continue;
        }
        if let Some(handler) = lone_target_handler(&lone, &lone_copy, records, target, &after) {
            records[jump].mode = target_test_word(|k| records.get(target + k));
            records[jump].handler = handler as *const ();
        }
    }
}

/// The target test code of a back edge's record `mode` (`lane_frame::TARGET_TEST_SHIFT`): the code of the run's first record (`run(k)`) past its prefix and copy rows where that is a plain comparison jump.
fn target_test_word<'a>(run: impl Fn(usize) -> Option<&'a LaneInstruction>) -> u64 {
    use quench_runtime::staged::{COMPARE_JUMP_ROW as TEST, FUSED_TARGET_COPY_ROWS as C, FUSED_TARGET_PREFIX_ROWS as P};
    let passes = |h| plain_row::<{ P[0] }>(h) || plain_row::<{ P[1] }>(h) || plain_row::<{ P[2] }>(h) || plain_row::<{ C[0] }>(h);
    let test = (0..3).map_while(&run).find(|record| !passes(record.handler)).filter(|record| plain_row::<TEST>(record.handler));
    test.map_or(0, |record| u64::from(lane_frame::target_comparison(record)) << lane_frame::TARGET_TEST_SHIFT)
}

/// The handler of the `IncDec` record `update` that the record `jump` follows, where `jump` is an
/// unconditional jump: the generated pair of `IncDec` and `Jump` (`fused_row`, the `staged::FUSED_ROWS`
/// entry of that pair, `FUSED_PLAIN`). `None` for any other pair. A backward jump whose target run holds
/// at `target` (a test, or a variable read or an update then a test) takes the pair's `FUSED_TARGET`
/// record instead (`pair_target_forms`), so the target run runs in the same dispatch. The loader's
/// `fuse_rows` pass and a loop site that retires (`loop_retire_in_lane`) fuse by it, so a declined loop runs
/// what the interpreter's own does. The target record keeps its own row.
///
/// Returns the handler and the record's `mode` word (`target_word`), which the caller writes with it.
fn update_fusion(update: &LaneInstruction, jump: &LaneInstruction, target: Option<Ip>) -> Option<(Handler, u64)> {
    update_direction(update)?;
    let backward = match jump.handler {
        plain if plain == jump_row::<JUMP>(true) as *const () => true,
        plain if plain == jump_row::<JUMP>(false) as *const () => false,
        _ => return None,
    };
    if let (true, Some(target)) = (backward, target) {
        let forms = pair_target_forms::<{ quench_runtime::staged::INC_DEC_ROW }, { quench_runtime::staged::JUMP_ROW }>();
        // SAFETY: a backward jump's target is a validated record of the running view.
        let target_handler = unsafe { (*target).handler };
        // A target run's rows fall through to the records after it, which validation makes records of the view
        // (a copy is a value row, so the record after it is one too).
        let after = |k: usize| unsafe { (*(0..k).fold(target, |at, _| following(at))).handler };
        if let Some(handler) = target_run_handler(&forms, target_handler, &after) {
            // SAFETY: as the read of `target_handler`.
            let handler = fixed_comparison(handler, unsafe { &*target });
            // SAFETY: as `after`: the run's rows fall through to records of the view.
            let test = target_test_word(|k| Some(unsafe { &*(0..k).fold(target, |at, _| following(at)) }));
            return Some((handler, target_word(jump) | test));
        }
    }
    let pair = lane_frame::fused_row::<{ quench_runtime::staged::INC_DEC_ROW }, { quench_runtime::staged::JUMP_ROW }, { quench_runtime::staged::FUSED_PLAIN }>(backward);
    Some((pair, target_word(jump)))
}

/// The `mode` word of a `FUSED_ROWS` or `FUSED_TARGET` pair record whose second row is the jump `jump`:
/// its direction (`MODE_BACKWARD` where the jump's distance is not positive, as `fused_rows` reads it).
fn target_word(jump: &LaneInstruction) -> u64 {
    if jump.jump() <= 0 { quench_runtime::staged::MODE_BACKWARD } else { 0 }
}

/// Whether `record`, an `IncDec` running its row (`generated_row`), decrements (its immediate flag);
/// `None` for any other record.
fn update_direction(record: &LaneInstruction) -> Option<bool> {
    is_row::<{ quench_runtime::staged::INC_DEC_ROW }>(record.handler).then(|| record.imm() != 0)
}

/// An environment-window record's immediate: its own (a local slot or closure function, at
/// most `ENVIRONMENT_WINDOW_FIELD`) below, the function's register count above.
const ENVIRONMENT_WINDOW_SHIFT: u32 = u16::BITS;
const ENVIRONMENT_WINDOW_FIELD: u32 = (1 << ENVIRONMENT_WINDOW_SHIFT) - 1;

/// An environment-window record's own immediate and the window's register count.
#[inline(always)]
fn environment_window_fields(i: &LaneInstruction) -> (usize, u16) {
    let imm = i.imm();
    (usize::from(quench_runtime::staged::window_slot(imm)), quench_runtime::staged::window_base(imm))
}

/// Gives a function whose locals an environment activation or a suspended frame's window could
/// hold (`CallPlan::environment_activation`, `CallPlan::suspended_environment`) the records that read
/// its locals or make its closures a window's: each record's immediate carries the window's register
/// count above its slot (`environment_window_fields`). A plain slot's local record takes its generated
/// row with `FLAG_WINDOW` (`window_records::window_local_handler`), which reads the window's locals in an
/// activation's run; any other local record takes its miss. Returns the window every record names.
fn specialize_environment_window(
    function: &Function,
    decoded: &[WideInstruction],
    records: &mut [LaneInstruction],
) -> Option<EnvironmentWindow> {
    if u32::from(function.locals) > u64::BITS
        || (function.arguments_slot.is_some() && !function.reads_passed_arguments_only() && !function.arguments_ride_window())
    {
        return None;
    }
    // A function that resolves names at run time stores and makes closures through the scope
    // its activation carries (`scoped_names`).
    let scoped = function.has_dynamic_scope_access();
    let registers = function.registers;
    let mut replacements = Vec::new();
    for (pc, &instruction) in decoded.iter().enumerate() {
        match window_record(function, instruction) {
            WindowRecord::Keeps => {}
            WindowRecord::Eval => replacements.push((pc, Some(lane_eval::direct_eval_row_handler()), instruction.imm())),
            WindowRecord::Local(own) => {
                if own > ENVIRONMENT_WINDOW_FIELD {
                    return None;
                }
                let handler = window_local_handler(function, instruction.op(), own as usize, scoped);
                replacements.push((pc, handler, own | u32::from(registers) << ENVIRONMENT_WINDOW_SHIFT));
            }
        }
    }
    for (pc, handler, imm) in replacements {
        if let Some(handler) = handler {
            records[pc].handler = handler as *const ();
        }
        records[pc].operand = LaneOperand { imm };
    }
    Some(EnvironmentWindow {
        locals: function.locals,
        captured: (0..usize::from(function.locals))
            .filter(|slot| function.local_slot_uses_environment(*slot))
            .fold(0, |mask, slot| mask | 1 << slot),
        rest: function.rest,
        self_binding: function.self_binding_slot,
        parameter_registers: function.parameters_promoted_in_place(),
    })
}

/// The `arguments` count row the view gives its kept-arguments fact (`row_near`).
const ARGUMENTS_COUNT: u16 = quench_runtime::staged::ARGUMENTS_COUNT_ROW;

/// The jump rows the jump passes fuse and requicken to (`jump_row`).
const JUMP: u16 = quench_runtime::staged::JUMP_ROW;

/// The capture rows the capture passes fuse (`specialize_captures`, `specialize_capture_updates`,
/// `specialize_load_branch_pairs`) and whose read a fused update falls back to.
const CAPTURE_READ: u16 = quench_runtime::staged::LOAD_CAPTURE_ROW;
const CAPTURE_WRITE: u16 = quench_runtime::staged::STORE_CAPTURE_ROW;

/// Gives each capture record its handler by the capture's depth, which the bytecode fixes. A
/// record past the function's own scope keeps its row: an outer `LoadCapture` reads through the
/// lane's remembered walks (`Reads::of_captures`), an outer `StoreCapture` takes the general step.
/// A depth-0 record is its row read as
/// the zero depth it is (`row_near`).
fn specialize_captures(records: &mut [LaneInstruction]) {
    for pc in 0..records.len() {
        let record = &records[pc];
        let own = ImmediateLayout::capture_depth(record.imm()) == 0;
        let handler: Handler = if !own {
            continue;
        } else if let Some(near) = row_near::<CAPTURE_READ>(record.handler).or_else(|| row_near::<CAPTURE_WRITE>(record.handler)) {
            // Its own scope's capture: the row reads its depth as the zero it is.
            near
        } else {
            continue;
        };
        records[pc].handler = handler as *const ();
    }
}

/// Gives each plain two-operand arithmetic record whose operands are both registers its row at
/// `FLAG_REGISTERS`, which reads them without testing their tags. Runs after the fusion passes, which
/// find the rows by their plain handlers.
fn specialize_register_operands(records: &mut [LaneInstruction]) {
    for record in records.iter_mut() {
        if !lane_frame::register_operands(record) {
            continue;
        }
        let row = lane_frame::registers_row::<{ quench_runtime::staged::BINARY_ROW }>(record.handler)
            .or_else(|| lane_frame::registers_row::<{ quench_runtime::staged::NUMERIC_ADD_ROW }>(record.handler))
            .or_else(|| lane_frame::registers_row::<{ quench_runtime::staged::NUMERIC_MULTIPLY_ROW }>(record.handler));
        if let Some(row) = row {
            record.handler = row as *const ();
        }
    }
}

// merge: theirs' fused capture update (`specialize_capture_updates`, `lane_update_capture_int32`)
// is not ported: it fuses `lane_operator_int32` and `lane_store_capture_load`, which ours does not
// have. Its records run unfused, with the same semantics.

/// Gives each frame `LoadLocal` record the handler of its slot's static binding facts
/// (`LOCAL_LOAD_ROW`): a slot a mapped `arguments` mirrors, or a self binding, keeps the
/// general handler. A `StoreLocal` keeps the row for every slot: its binding is resolved there
/// (the row's frame, else the general step), which also covers a captured environment's slot.
fn specialize_frame_locals(function: &Function, records: &mut [LaneInstruction]) {
    for record in records.iter_mut() {
        let slot = record.imm() as usize;
        let load = is_row::<{ quench_runtime::staged::LOAD_ENV_LOCAL_ROW }>(record.handler);
        let store = is_row::<{ quench_runtime::staged::STORE_ENV_LOCAL_ROW }>(record.handler);
        if (!load && !store) || quench_runtime::vm::arguments::mapped_arguments_slot(function, slot).is_some() {
            continue;
        }
        // A slot the frame's captured environment holds is read by its `Names` row; one it does not hold is the frame's own local, which the generated
        // LoadLocal and StoreLocal rows read and write.
        let environment = function.local_slot_uses_environment(slot);
        let handler: Handler = match (load, environment) {
            // A captured slot's load keeps its environment row (`LoadEnvLocal`'s `Names` row).
            (true, true) => continue,
            (true, false) => generated_row!(quench_runtime::staged::LOCAL_LOAD_ROW, ACC_NONE, FLAG_UNREAD),
            _ if function.is_self_binding_slot(slot) => continue,
            // An environment slot's store keeps its row, which misses to the general store.
            (false, true) => continue,
            (false, false) => generated_row!(quench_runtime::staged::LOCAL_STORE_ROW, ACC_NONE, FLAG_UNREAD),
        };
        record.handler = handler as *const ();
    }
}



/// Gives a returning `Binary` record of an arithmetic operator on a register and a constant its
/// operator's `returning_operator_row`, its fields resolved as `specialize_register_operands`
/// resolves a non-returning one's. A record whose right operand is a register, and any other,
/// keeps its `returning_row`: a constant operand replaces the record's operator field (the
/// operand is one union with it), so the row's operator is its static parameter instead.
fn specialize_returning_operator(instruction: WideInstruction, constants: &[Value], record: &mut LaneInstruction) {
    use quench_runtime::bytecode::OperandKind::{Constant, Register};
    let (left, right) = (instruction.operand_b(), instruction.operand_c());
    if left.kind() != Some(Register) || instruction.numeric_local_target().is_some() {
        return;
    }
    let constant = match right.kind() {
        Some(Constant) => match constants.get(usize::from(right.payload())) {
            Some(constant) => *constant,
            None => return,
        },
        _ => return,
    };
    let int32 = constant.as_int().is_some();
    macro_rules! pick {
        ($op:expr) => {
            if int32 { returning_operator_row_of!($op, NOT_LEAF, true) } else { returning_operator_row_of!($op, NOT_LEAF, false) }
        };
    }
    let Some(handler) = arithmetic_operators!(instruction.binary_operator(), pick) else {
        return;
    };
    record.handler = handler as *const ();
    record.b = left.payload();
    record.operand = LaneOperand { constant };
}



/// Whether some loop of `function`, from a backward jump's target through the jump, has no
/// instruction the lane exits at.
/// Whether one activation of `function` may run an instruction more than once: it has a
/// backward jump, or a branch table, whose entries may jump back.
fn may_loop(function: &Function) -> bool {
    function.code.iter().enumerate().any(|(pc, packed)| {
        let instruction = function.widened(*packed);
        quench_runtime::staged::wasm_branch_table(instruction.op())
            || (instruction.op().immediate_role() == ImmediateRole::JumpTarget
                && instruction.imm() as usize <= pc)
    })
}

/// How many lane records control reaches from each PC before one that exits, counted up
/// to `cap`: where `follows_jumps`, a `Jump` continues at its target, as a resumed loop body
/// does, and any other record at the next PC. Each pass extends every count by one step, so
/// `cap` passes reach the fixed point the capped counts allow; without jumps one pass does.
/// An `Await` counts as `cap`: a resumed async frame's lane run that stops there suspends
/// without the general loop (`Vm::stop_awaits_in_lane`), so a short run to it repays its entry.
fn runs_ahead_in_lane(decoded: &[WideInstruction], exits: &[bool], cap: usize, follows_jumps: bool) -> Vec<usize> {
    let next = |pc: usize| match decoded[pc].op() {
        _ if follows_jumps && quench_runtime::staged::jump_family(decoded[pc].op()) => decoded[pc].imm() as usize,
        _ => pc + 1,
    };
    let mut runs = vec![0usize; exits.len()];
    for _ in 0..cap {
        let mut changed = false;
        for pc in (0..exits.len()).rev() {
            let run = if quench_runtime::staged::awaits(decoded[pc].op()) {
                cap
            } else if exits[pc] {
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

/// Whether the straight-line run from the first instruction, through its unconditional jumps,
/// reaches a `Return` before any record the lane exits at.
fn entry_run_returns(decoded: &[WideInstruction], exits: &[bool]) -> bool {
    let mut pc = 0;
    for _ in 0..decoded.len() {
        if exits.get(pc).is_none_or(|exits| *exits) {
            return false;
        }
        match decoded[pc].op() {
            _ if quench_runtime::staged::return_family(decoded[pc].op()) => return true, _ if decoded[pc].returns_from_frame() => return true, // fused with its return
            _ if quench_runtime::staged::jump_family(decoded[pc].op()) => pc = decoded[pc].imm() as usize,
            _ => pc += 1,
        }
    }
    false
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

/// Two inline words cover 128 instruction PCs without a heap allocation;
/// larger functions spill to contiguous words without limiting their size.
const INLINE_INSTRUCTION_WORDS: usize = 2;
const INSTRUCTION_WORD_BITS: usize = u64::BITS as usize;

struct InstructionSet(SmallVec<[u64; INLINE_INSTRUCTION_WORDS]>);

impl InstructionSet {
    fn new(count: usize) -> Self {
        Self(std::iter::repeat_n(0, count.div_ceil(INSTRUCTION_WORD_BITS)).collect())
    }

    fn insert(&mut self, pc: usize) {
        self.0[pc / INSTRUCTION_WORD_BITS] |= 1 << (pc % INSTRUCTION_WORD_BITS);
    }

    fn contains(&self, pc: usize) -> bool {
        self.0[pc / INSTRUCTION_WORD_BITS] & (1 << (pc % INSTRUCTION_WORD_BITS)) != 0
    }
}

/// The records of every branch table's entries, whose jump offsets the table reads.
fn branch_table_entry_records(instructions: &[WideInstruction]) -> InstructionSet {
    let mut entries = InstructionSet::new(instructions.len());
    for (pc, &instruction) in instructions.iter().enumerate() {
        if quench_runtime::staged::wasm_branch_table(instruction.op()) {
            for entry in quench_runtime::bytecode::branch_table_entries(pc, instruction) {
                entries.insert(entry);
            }
        }
    }
    entries
}

/// The records control enters other than from the record before: jump targets, and the end of
/// a generator's parameter initialization, where its first resumption starts.
fn entered_records(instructions: &[WideInstruction], parameter_end_pc: u32) -> InstructionSet {
    let mut targets = InstructionSet::new(instructions.len());
    for instruction in instructions {
        let target = instruction.imm() as usize;
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget
            && target < instructions.len()
        {
            targets.insert(target);
        }
    }
    if parameter_end_pc != 0 {
        targets.insert(parameter_end_pc as usize);
    }
    targets
}

#[quench_macros::ext(DispatchFastVm6)]
impl Vm {
    /// The lane view a general-loop entry of `function` runs with, or null when it runs
    /// without one (`ProgramStore::entry_lane_view`).
    pub(crate) fn entry_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView {
        let Some(code) = self.programs.residual(program) else { return std::ptr::null() };
        if !self.runs_lane || !self.may_run_in_lane(program, code) { return std::ptr::null(); }
        // A Wasm function that may loop, or one of a program with an AOT image, gets its view at entry; any
        // other derives it once a loop turns hot (`Vm::note_back_edge`).
        let wasm = code.kind == quench_runtime::bytecode::ProgramKind::Wasm;
        let image = self.programs.ahead_images(program, code).is_some();
        self.programs
            .entry_lane_view(program, function, |code| image || wasm && may_loop(code), |code, constants, kind, live, sites| {
                self.derived_view(function, code, constants, kind, live, sites, image)
            })
            .unwrap_or(std::ptr::null())
    }

    /// `function`'s view of `code`, derived with this VM's facts.
    #[allow(clippy::too_many_arguments)]
    fn derived_view<'a>(&self, function: u32, code: &quench_runtime::bytecode::Function, constants: &[Value], kind: quench_runtime::bytecode::ProgramKind,
        live: &dyn Fn() -> Option<&'a std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>, sites: &[quench_runtime::bytecode::FieldSite], image: bool) -> LaneView {
        let mut view = derive_lane_view(function, code, constants, self.heap_reads(), kind, live, sites,
            crate::compiled::admission::record_plan(self.tier_may_run(crate::compiled::admission::Tier::ANY)),
            self.programs.reports_on_drop(), self.programs.tiers());
        // Where an image's bodies are the only code, the lane takes on every record they leave at.
        if image && !self.execution_mode.allows_dynamic_execution() { view.admit_every_record(); }
        view
    }

    /// Whether a lane run of `program` credits its functions' loop work: a Wasm program in
    /// compiled execution without an eager artifact (selective tiering) promotes its functions
    /// one by one, by calls and by loop work.
    fn credits_backedges_in_lane(&self, code: &ResidualProgram, program: ProgramId) -> bool {
        code.kind == quench_runtime::bytecode::ProgramKind::Wasm
            && self.execution_mode == quench_runtime::vm::ExecutionMode::Compiled
            && matches!(self.programs.compiled(program), Some(None))
            && self.programs.wasm_module_selective(program)
    }

    /// Whether the instruction at `pc` of `view` runs in the lane.
    #[inline(always)]
    pub(crate) fn runs_in_lane(view: *const LaneView, pc: usize) -> bool {
        // SAFETY: a non-null view has an entry for every validated PC.
        !view.is_null()
            && unsafe {
                let entries: &[u64] = &(*view).entries;
                *entries.get_unchecked(pc / u64::BITS as usize)
                    & (1 << (pc % u64::BITS as usize)) != 0
            }
    }

    /// Whether a call from a native enters `view` at its first instruction: where the general
    /// loop would, or where the lane runs the call whole (`LaneView::returns_from_entry`).
    #[inline(always)]
    pub(crate) fn runs_call_in_lane(view: *const LaneView) -> bool {
        // SAFETY: a non-null view lives with its program.
        Self::runs_in_lane(view, 0) || !view.is_null() && unsafe { (*view).returns_from_entry }
    }

    /// Run lane instructions from `pc` in `frame` and return the PC of the
    /// first one that needs the general path; lane calls and returns update
    /// `frame` and `pending` as the general loop would. Kept out of line so
    /// the general loop's code generation, which serves JavaScript, does not
    /// depend on the lane.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_fast_lane(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        view: *const LaneView,
        frame: &mut usize,
        pc: usize,
        pending: &mut Vec<PendingGeneralCall>,
        executing: &mut quench_runtime::vm::dispatch_frame::ExecutingProgram,
        inline_calls: bool,
    ) -> usize {
        // SAFETY: the cursor's view outlives the lane run.
        let view = unsafe { &*view };
        let base = view.records();
        let registers = Registers(self.frames[*frame].registers.as_mut_ptr());
        let entered = self.enter_lane_floor(code, program);
        let start = LaneStart {
            code,
            program,
            root: LaneRoot::Frame(*frame),
            registers,
            base,
            this: self.frame_lane_this(*frame),
            pending,
            executing,
            inline_calls,
            script_calls: inline_calls
                && code.kind != quench_runtime::bytecode::ProgramKind::Wasm
                && *frame + 1 == self.frames.len(),
            script_constructs: code
                .function_get(self.frames[*frame].function as usize)
                .is_some_and(|function| {
                    function.dispatch == quench_runtime::bytecode::DispatchClass::General
                }),
            floor: entered.cell,
        };
        let mut context = self.context_for_start(start);
        let function = self.frames[*frame].function;
        let ip = self.drive_lane(&mut context, entered, |cx| {
            // SAFETY: `cx` is the only access path to the context during the run. A frame at its first
            // instruction enters its published body as an activation does.
            unsafe { (*cx).memory = frame_memory_view(cx, view, registers) };
            if pc == 0 && let Some(start) = body_calls::enter_frame_body(cx, function) { return start; }
            // SAFETY: the view has one record per instruction, and residual
            // validation establishes every reachable PC.
            unsafe { base.add(pc) }
        });
        *frame = context.frame();
        // SAFETY: handlers only return records of the active view.
        unsafe { ip.offset_from(context.base) as usize }
    }

    /// Raises the stack floor for a lane run of `program`: below the native stack's slack, or
    /// at once where a collection is due or the run credits loop work, so its backward jumps
    /// count in `safepoint_at` (`LaneContext::backedges_left`). The floor lives in the VM's
    /// interrupt state, where a termination request raises it; entry sets it before reading
    /// the request, the request raises it after setting itself, so one of the two sees the
    /// other.
    fn enter_lane_floor(&mut self, code: &ResidualProgram, program: ProgramId) -> LaneFloor {
        let floor = if self.collection_due() || self.credits_backedges_in_lane(code, program) {
            COLLECTION_DUE_FLOOR
        } else {
            stack_pointer().saturating_sub(LANE_STACK_SLACK)
        };
        let cell: *const std::sync::atomic::AtomicUsize = self.execution_interrupt.floor_in_lane();
        let outer_cell = LANE_FLOOR.with(|current| current.replace(cell));
        // SAFETY: the VM holds its interrupt state for the whole run.
        let outer_floor = unsafe { &*cell }.swap(floor, std::sync::atomic::Ordering::SeqCst);
        if self.execution_interrupt.is_termination_requested() {
            unsafe { &*cell }.store(COLLECTION_DUE_FLOOR, std::sync::atomic::Ordering::SeqCst);
        }
        LaneFloor { cell, outer_cell, outer_floor }
    }

    /// Restores the floor `entered` raised: an enclosing run of this VM keeps a raised floor
    /// raised, and a termination request raises it for good.
    fn leave_lane_floor(&self, entered: LaneFloor) {
        // SAFETY: the VM holds its interrupt state for the whole run.
        let floor = unsafe { &*entered.cell };
        if self.execution_interrupt.is_termination_requested() {
            floor.store(COLLECTION_DUE_FLOOR, std::sync::atomic::Ordering::SeqCst);
        } else {
            floor.store(entered.outer_floor, std::sync::atomic::Ordering::Relaxed);
        }
        LANE_FLOOR.with(|current| current.set(entered.outer_cell));
    }

    /// `Vm::call_value` of `callee` from the native a lane called (`LaneNativeCall`), run as
    /// the lane's own call of it would run: an activation of that lane (`script_target_in_lane`,
    /// `activate_keeping_in_lane`) whose completion ends the run with its value
    /// (`Completion::ReturnsToNative`). Any other exit makes the activations frames, the
    /// callback's the entry frame of the general loop that finishes the call
    /// (`Vm::run_frames_after_lane_exit`), as a general call of it would have run it. `None`
    /// leaves the call to the general path: a native not running from the lane's frame, or a
    /// callee or call no lane call would activate.
    pub(crate) fn native_callback_in_lane(
        &mut self,
        callee: Value,
        receiver: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if !self.execution_mode.allows_dynamic_execution() { return None; }
        let site = self.lane_native?;
        let cx: *mut LaneContext = site.cx.cast();
        // The native runs from the lane's frame, the innermost, with no activation above it:
        // `call_native_in_lane` made them frames, and only this run makes more.
        if site.frames != self.frames.len()
            || context_in_lane() != cx
            || self.lane_stack.activation_count() != 0
            || self.lane_deferred.is_some()
            || self.with_scope_entered
        {
            return None;
        }
        // SAFETY: the lane's context lives while its native runs; nothing else accesses it.
        // A due collection runs on the general path, whose safepoints take it; the lane's
        // floor, set for the native's caller, bounds this run's stack as it bounds the lane's
        // own calls (`LANE_STACK_SLACK`).
        if !self.lane_stack.admits_program(unsafe { (*cx).program }) || self.collection_due() {
            return None;
        }
        // Each nested run takes native stack, as the general loop's would (`Vm::enter_stack`),
        // whose exhaustion the general path raises.
        let _stack = quench_runtime::stack::StackGuard::enter().ok()?;
        // A collection since the lane entered retires the targets it resolved.
        self.lane_stack.enter_generation((self.cache_epochs.gc, self.realm.globals));
        // Another program's callee (a library's function a benchmark's native call reaches)
        // runs as that program's activation, whose completion leaves the program to re-enter.
        let (target, crosses) = match script_target_in_lane(cx, callee) {
            Some(target) => (target, false),
            None => cross_callback_target_in_lane(cx, callee)?,
        };
        let this = call_this_in_lane(self, &target, receiver)?;
        let registers = self.active_registers;
        // SAFETY: as above; the context holds no drop glue. A completion to the native restores
        // the state of the native's caller (`restore_caller_in_lane`), so only an exit, which makes
        // frames, writes the copy back.
        let saved = unsafe { std::ptr::read(cx) };
        let (code, program) = (saved.code, saved.program);
        let mut pending = self.pending_call_stacks.pop().unwrap_or_default();
        let mut executing = quench_runtime::vm::dispatch_frame::ExecutingProgram { id: program, retained: None };
        unsafe {
            (*cx).pending = &mut pending;
            (*cx).executing = &mut executing;
        }
        let Some(start) = push_callback_in_lane(cx, site.call, &target, args, this, crosses) else {
            unsafe {
                (*cx).pending = saved.pending;
                (*cx).executing = saved.executing;
            }
            self.pending_call_stacks.push(pending);
            return None;
        };
        unsafe { (*cx).memory = MemoryView::script(cx) };
        let exit = dispatch_in_lane(cx, start);
        let outcome = match self.lane_completion.take_native_returned() {
            Some(value) => {
                unsafe {
                    (*cx).pending = saved.pending;
                    (*cx).executing = saved.executing;
                }
                if crosses {
                    enter_program_in_lane(cx, program);
                }
                Ok(value)
            }
            None => {
                // SAFETY: handlers only return records of the active view.
                let pc = unsafe { exit.offset_from((*cx).base) } as usize;
                // The callback's frame runs in its own program, which the general loop
                // finishes it in.
                let code = if crosses {
                    self.programs.residual(target.program()).map_or(code, std::ptr::from_ref)
                } else {
                    code
                };
                self.materialize_lane_exit(cx);
                let frame = unsafe { (*cx).frame() };
                debug_assert!(frame >= site.frames, "the callback's activation became a frame");
                self.frames[frame].pc = pc;
                let raised = self.lane_completion.take_raised();
                // SAFETY: as above.
                unsafe { restore_context_in_lane(cx, saved) };
                // SAFETY: the lane's program outlives its run.
                let code = unsafe { &*code };
                let exited = quench_runtime::vm::dispatch_frame::LaneExit { executing, raised };
                let result = self.run_frames_after_lane_exit(code, frame, &mut pending, exited);
                debug_assert_eq!(self.frames.len(), site.frames + 1, "the loop leaves the callback's frame");
                self.complete_user_frame(result, false).and_then(FrameOutcome::into_value)
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

    /// `Vm::call_value` of `callee` from a native running beside the lane's activations
    /// (`DeferredLaneCall`), run as an activation above them whose completion ends the run
    /// with its value (`Completion::ReturnsToNative`), so they stay activations and the call
    /// stays deferred. A materialization while it runs makes the activations below it the
    /// native caller's frames, with the lane's outer pending calls (`NativeBoundary`); an exit
    /// finishes the callback's frames in the general loop, as `native_callback_in_lane`'s does,
    /// and the native then returns to its caller's frame, as an observed deferred call does.
    /// `None` leaves the call to the general path: another lane, program or frame count, a
    /// callee no lane call would activate, or a boundary already in place.
    pub(crate) fn deferred_callback_in_lane(
        &mut self,
        callee: Value,
        receiver: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if !self.execution_mode.allows_dynamic_execution() { return None; }
        let deferred = self.lane_deferred?;
        let cx: *mut LaneContext = deferred.cx.cast();
        if deferred.frames != self.frames.len()
            || context_in_lane() != cx
            // Spills on purpose: the callback's trampoline header must be the only one, so a
            // header naming the trampoline already becomes a record (one-frame-one-call.md F2c).
            || self.lane_stack.activations().is_empty()
            || self.lane_stack.native_boundary.is_some()
            || self.with_scope_entered
        {
            return None;
        }
        // SAFETY: as `lane_native_callback`'s.
        if !self.lane_stack.admits_program(unsafe { (*cx).program }) || self.collection_due() {
            return None;
        }
        let _stack = quench_runtime::stack::StackGuard::enter().ok()?;
        self.lane_stack.enter_generation((self.cache_epochs.gc, self.realm.globals));
        let target = script_target_in_lane(cx, callee)?;
        // A callback of another program would enter it from the native's caller's view.
        if target.program() != unsafe { (*cx).program } {
            return None;
        }
        let this = call_this_in_lane(self, &target, receiver)?;
        let registers = self.active_registers;
        // SAFETY: as `native_callback_in_lane`'s; the copy is the innermost caller activation's
        // state, which an in-lane completion restores and an exit rebuilds as a frame's.
        let saved = unsafe { std::ptr::read(cx) };
        let (code, program) = (saved.code, saved.program);
        // SAFETY: the deferred call's record lies in the caller activation's view.
        let call = unsafe { saved.base.add(deferred.call_pc) };
        let lower = self.lane_stack.activation_count();
        let Some(start) = push_callback_in_lane(cx, call, &target, args, this, false) else {
            // A push that found no room made the activations frames, as the native's
            // observation of the frame stack would have.
            if self.frames.len() != deferred.frames {
                self.lane_deferred = None;
                self.resume_deferred_caller(deferred.call_pc, deferred.frames);
            }
            return None;
        };
        // The callback's activation is made: a materialization from now on gives the
        // caller's activations the outer pending calls and executing program.
        let mut pending = self.pending_call_stacks.pop().unwrap_or_default();
        let mut executing = quench_runtime::vm::dispatch_frame::ExecutingProgram { id: program, retained: None };
        unsafe {
            (*cx).pending = &mut pending;
            (*cx).executing = &mut executing;
        }
        self.lane_stack.native_boundary =
            Some(NativeBoundary {
                activation: lower,
                pending: saved.pending,
                executing: saved.executing,
                deferred,
                natives: self.realm.promise.active_native.len(),
            });
        // A callback of a native an earlier callback's frames run nests its own boundary.
        let outer_boundary_frames = self.lane_stack.boundary_frames.take();
        // A native the callback calls defers in its own turn, with its own window.
        self.lane_deferred = None; let window = self.running_window.take();
        unsafe { (*cx).memory = MemoryView::script(cx) };
        let exit = dispatch_in_lane(cx, start);
        let returned = self.lane_completion.take_native_returned();
        let mut boundary_frames = std::mem::replace(&mut self.lane_stack.boundary_frames, outer_boundary_frames);
        let outcome = match (returned, boundary_frames) {
            // Completed in the lane above its caller's activations: the call stays deferred.
            (Some(value), None) => {
                self.lane_stack.native_boundary = None;
                unsafe {
                    (*cx).pending = saved.pending;
                    (*cx).executing = saved.executing;
                }
                self.lane_deferred = Some(deferred); self.running_window = window;
                Ok(value)
            }
            (returned, _) => {
                let exit_pc = returned.is_none().then(|| {
                    // SAFETY: handlers only return records of the active view.
                    let pc = unsafe { exit.offset_from((*cx).base) } as usize;
                    self.lane_stack.boundary_frames = None;
                    self.materialize_lane_exit(cx);
                    if let Some(made) = std::mem::replace(&mut self.lane_stack.boundary_frames, outer_boundary_frames) {
                        boundary_frames = Some(made);
                    }
                    let frame = unsafe { (*cx).frame() };
                    self.frames[frame].pc = pc;
                    frame
                });
                // The caller's activations are frames: the lane continues in the innermost,
                // the native's caller, once the callback's frames finish.
                let caller = deferred.frames
                    + boundary_frames.expect("the caller's activations became frames above the native's")
                    - 1;
                self.lane_stack.native_boundary = None;
                // SAFETY: as above; the copy is restored before the general loop runs, as
                // `native_callback_in_lane`'s is, then made the caller frame's.
                unsafe {
                    restore_context_in_lane(cx, saved);
                    let frame_registers = Registers(self.frames[caller].registers.as_mut_ptr());
                    (*cx).root = LaneRoot::Frame(caller);
                    (*cx).registers = frame_registers;
                    frame_registers.set_this(self.frame_lane_this(caller));
                    frame_registers.set_env(Value::DELETED);
                    (*cx).registers.set_capture(std::ptr::null());
                    (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
                    (*cx).memory = MemoryView::script(cx);
                }
                let result = match (returned, exit_pc) {
                    (Some(value), _) => Ok(value),
                    (None, Some(frame)) => {
                        let raised = self.lane_completion.take_raised();
                        // SAFETY: the lane's program outlives its run.
                        let code = unsafe { &*code };
                        let exited = quench_runtime::vm::dispatch_frame::LaneExit { executing, raised };
                        let result = self.run_frames_after_lane_exit(code, frame, &mut pending, exited);
                        self.complete_user_frame(result, false).and_then(FrameOutcome::into_value)
                    }
                    (None, None) => unreachable!("an exit records its frame"),
                };
                pending.clear();
                self.pending_call_stacks.push(std::mem::take(&mut pending));
                self.active_registers = self.frames[caller].registers.as_mut_ptr();
                if self.active_program != program {
                    self.set_active_program(program);
                }
                return Some(result);
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

/// Restores the copy `saved` a native's callback took of the lane's context before it ran
/// (`Vm::native_callback_in_lane`). The copy's array storage proofs predate whatever the
/// callback changed, so they retire.
///
/// # Safety
/// `cx` is the running lane's context, which nothing else accesses.
unsafe fn restore_context_in_lane(cx: *mut LaneContext, saved: LaneContext) {
    unsafe {
        std::ptr::write(cx, saved);
        (*cx).proofs.retire();
    }
}

pub(crate) use quench_runtime::vm::engine_types::PreparedLaneCallback;

use quench_runtime::vm::engine_types::PreparedShape;

#[quench_macros::ext(PreparedShapeOps1)]
impl PreparedShape {
    /// The shape of `target`'s activations from the native whose caller's context is
    /// `caller`, for calls passing `argc` values; `None` for a gathering target, whose rest
    /// array each call makes (`activate_keeping_in_lane`).
    #[inline(always)]
    fn of(caller: &LaneContext, call: Ip, target: &ScriptCallTarget, argc: usize) -> Option<Self> {
        if target.entry == CallEntry::Gathers || argc > usize::from(u16::MAX) {
            return None;
        }
        let (params, registers) = (usize::from(target.params), usize::from(target.registers));
        let keeps = target.forwards() || argc > params && !target.drops_extra_arguments();
        let size = if keeps { registers + argc } else { registers };
        let activation = Activation::of_parts(
            target.function, call, caller.base, caller.registers, caller.program,
            ActivationKind::Script(ScriptActivation {
                callee: target.callee,
                argc: if keeps { argc } else { argc.min(params) } as u16,
                completion: Completion::ReturnsToNative,
                forwards: keeps,
                kept: if keeps { target.registers } else { 0 },
                environment: false,
                record: std::ptr::null_mut(),
            }),
        );
        let room = usize::from(target.room).max(fill_room::<ACTIVATION_FILL_BLOCK>(size));
        // A callback that keeps nothing is pushed as a plain activation returning through the
        // trampoline; one that keeps its values needs the record's `kept`.
        let header = (!keeps).then(|| {
            // SAFETY: the lane's VM, which outlives its context; the trampoline is the lane stack's.
            let (trampoline, result) = unsafe { (*caller.vm).lane_stack.callback_return.bind() };
            CallbackHeader {
                plain: quench_runtime::staged::protocol::PlainHeader::new(
                    trampoline, result, caller.base.expose_provenance() as u64,
                    target.function, argc.min(params) as u16, 0, target.callee.to_bits(),
                ),
                program: u64::from(caller.program.raw()),
                call,
                caller_registers: caller.registers,
            }
        });
        Some(Self { argc, activation, header, registers, passed: argc.min(params), keeps, size, room })
    }
}

use quench_runtime::vm::engine_types::CallbackHeader;


use quench_runtime::vm::engine_types::CallbackTrampoline;

#[quench_macros::ext(CallbackTrampolineOps1)]
impl CallbackTrampoline {
    /// Binds the handler of the VM `H` (`callback_trampoline_returned`) to its records, and answers
    /// the words a callback's header names: its call record and its result window. The call
    /// record is never dispatched; it holds the handler too.
    fn bind(&mut self) -> (u64, u64) {
        let handler = callback_trampoline_returned as Handler as *const ();
        for record in &mut self.records {
            record.handler = handler;
        }
        // The array steps' pair, bound with it: a step's header is its shape's with the call
        // record replaced (`step_call`).
        let stepping = array_step_returned as Handler as *const ();
        for record in &mut self.steps {
            record.handler = stepping;
        }
        (self.records.as_ptr().expose_provenance() as u64, self.result.as_ptr().expose_provenance() as u64)
    }

    /// The call record word an array step's header names (`steps[0]`), whose `following` is the
    /// stepping record. Bound once `bind` ran for a shape, which every step's shape did.
    #[inline(always)]
    fn step_call(&self) -> u64 {
        self.steps.as_ptr().expose_provenance() as u64
    }
}

script_handler! {
    /// `records[1]` of the return trampoline (`CallbackTrampoline`): the callback's plain return
    /// wrote its value to the trampoline's result register and restored its caller's records
    /// and the values' top. The native's caller's registers are the context's again and the run
    /// ends with the value, which the native receives, as `complete_native_callback_in_lane` ends a
    /// callback's record.
    fn callback_trampoline_returned<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let value = r.get(i.a);
        let stack = &mut vm.lane_stack;
        let (call, caller_registers) = (stack.callback_return.call.get(), stack.callback_return.caller_registers.get());
        // SAFETY: the running lane's context, which nothing else accesses.
        unsafe { (*cx).registers = caller_registers };
        // What the callback's loops and scopes owned completed with it.
        stack.drop_owned_from(stack.activation_count());
        vm.lane_completion.set(LaneCompletion::NativeReturned(value));
        exit(call)
    }
}

use quench_runtime::vm::engine_types::PreparedEntry;

#[quench_macros::ext(PreparedEntryOps1)]
impl PreparedEntry {
    /// The window layout the callback family's rows fill (`staged::call_callback::CallbackWindow`).
    #[inline(always)]
    fn window(&self) -> quench_runtime::staged::call_callback::CallbackWindow {
        quench_runtime::staged::call_callback::CallbackWindow {
            registers: self.registers as u16, passed: self.passed, argc: self.argc, keeps: self.keeps,
        }
    }

    /// The context words its callee runs with (`staged::call_callback::CallbackContext`).
    #[inline(always)]
    fn context(&self) -> quench_runtime::staged::call_callback::CallbackContext<u64> {
        quench_runtime::staged::call_callback::CallbackContext {
            this: self.this.to_bits(), env: self.env.to_bits(), capture: self.capture.expose_provenance() as u64,
        }
    }
}

#[quench_macros::ext(PreparedShapeOps2)]
impl PreparedShape {
    /// The entry of its activations of `target` with `this`.
    #[inline(always)]
    fn entry(&self, target: &ScriptCallTarget, this: Value) -> PreparedEntry {
        PreparedEntry {
            view: target.view,
            env: target.env,
            capture: target.capture,
            this,
            registers: self.registers,
            passed: self.passed as u16,
            argc: self.argc as u16,
            keeps: self.keeps,
            size: self.size,
            program: target.program(),
            function: target.function,
        }
    }
}

/// The activation of a native's callback of `shape`, for a call with `args` (`entry`): the
/// window filled, the activation pushed (its plain header where the shape has one, which
/// returns through the trampoline, else its record) and the context made the callee's.
/// `None`, which changes nothing, where the lane stack lacks room.
#[inline(always)]
fn push_prepared_in_lane(
    cx: *mut LaneContext,
    shape: &PreparedShape,
    entry: &PreparedEntry,
    args: &[Value],
) -> Option<Ip> {
    let window = match &shape.header {
        Some(header) => admit_callback_header(cx, header, shape.room)?,
        None => admit_prepared_in_lane(cx, &shape.activation, shape.room)?,
    };
    // SAFETY: the admission pushed the activation over the window; `args` holds
    // `entry.argc` values.
    Some(unsafe { enter_prepared_in_lane(cx, entry, args, window) })
}

/// Whether the lane stack admits one more activation with `room` values: the callback
/// family's admission row (`staged::call_callback::admit_callback`).
#[inline(always)]
fn admits_callback(stack: &LaneStack, room: usize) -> bool {
    quench_runtime::staged::call_callback::concrete_admit_callback(
        stack.activation_count() as u64, stack.activation_limit.get() as u64, stack.values.room() as u64, room as u64,
    )
}

/// `push_prepared_in_lane`'s admission of a record: `activation` pushed over a window at the lane
/// stack's top with `room`, whose address it answers for the entry to fill. `None`, which
/// changes nothing, where the lane stack lacks room.
#[inline(always)]
fn admit_prepared_in_lane(cx: *mut LaneContext, activation: &Activation, room: usize) -> Option<*mut Value> {
    let stack = &mut unsafe { vm(cx) }.lane_stack;
    if !admits_callback(stack, room) {
        return None;
    }
    // SAFETY: as `push_callback_in_lane`'s.
    let at = stack.values.top();
    // A header with its side record (`Completion::ReturnsToNative` or `ArrayStep`, and the
    // values it keeps), as every script activation of the lane's windows is
    // (one-frame-one-call.md F4 (3)).
    unsafe { stack.push_activation(*activation, at) };
    Some(at)
}

/// `admit_prepared_in_lane` of a callback's plain header (`LaneStack::push_callback_header`).
#[inline(always)]
fn admit_callback_header(cx: *mut LaneContext, header: &CallbackHeader, room: usize) -> Option<*mut Value> {
    let stack = &mut unsafe { vm(cx) }.lane_stack;
    if !admits_callback(stack, room) {
        return None;
    }
    // SAFETY: admitted above; the window starts at the values' top.
    let at = stack.values.top();
    unsafe { stack.push_callback_header(header, at) };
    Some(at)
}

/// The activation `entry` enters, whose window starts at `registers`,
/// for a call with `args`: the window filled and the context made the callee's. Its first
/// record.
///
/// # Safety
/// The lane stack's capacity holds the window, and `args` holds `entry.argc` values.
#[inline(always)]
unsafe fn enter_prepared_in_lane(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    args: &[Value],
    registers: *mut Value,
) -> Ip {
    // The callback family's window row (`staged::call_callback::fill_callback`).
    quench_runtime::staged::call_callback::fill_callback(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
        registers.expose_provenance() as u64, args.as_ptr().expose_provenance() as u64, entry.window(),
    );
    unsafe { enter_filled_in_lane(cx, entry, registers) }
}

/// `enter_prepared_in_lane` of a call passing `values`, written from where the caller holds
/// them (an array step's element, index and array) rather than copied from memory.
///
/// # Safety
/// As `enter_prepared_in_lane`'s, for `entry.argc == N`.
#[inline(always)]
unsafe fn enter_prepared_values_in_lane<const N: usize>(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    values: [Value; N],
    registers: *mut Value,
) -> Ip {
    unsafe {
        let registers = fill_prepared_values_in_lane::<N>(entry, values, registers);
        enter_filled_in_lane(cx, entry, registers)
    }
}

/// `enter_prepared_values_in_lane` of the activation that returned from the window it holds,
/// at the lane stack's top, entered again for its next call (`step_array_iteration_in_place_in_lane`):
/// its return left the context its own and the stack's length past its window, so only the
/// window is filled again.
///
/// # Safety
/// As `enter_prepared_values_in_lane`'s, for the innermost activation, whose window `entry`
/// entered at `registers` and whose return the context still holds.
#[inline(always)]
unsafe fn reenter_prepared_values_in_lane<const N: usize>(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    values: [Value; N],
    registers: *mut Value,
) -> Ip {
    unsafe {
        let registers = fill_prepared_values_in_lane::<N>(entry, values, registers);
        // A capture walk inside the returned run may have cached the closure's dense slots in the
        // window's capture word (`capture_read_value`): the same environment's, which the entry
        // passes as it passed it before.
        Registers(registers).set_capture(entry.capture);
        debug_assert!(
            (*cx).registers.0 == registers
                && (*cx).base == (*entry.view).records()
                && Registers(registers).this() == entry.this
                && Registers(registers).env() == entry.env
                && Registers(registers).capture() == entry.capture
                && vm(cx).lane_stack.top_frame().is_some_and(|frame| frame.window_ptr() == registers),
            "a returning activation's context is its own"
        );
        let vm = vm(cx);
        vm.programs.record_js_jit_lane_entry(entry.program, entry.function);
        // SAFETY: the program store keeps a derived view alive with its program.
        (*entry.view).start()
    }
}

/// The window of an activation of `entry` at `registers` filled for a call
/// passing `values` (`enter_prepared_values_in_lane`): its registers.
///
/// # Safety
/// As `enter_prepared_values_in_lane`'s.
#[inline(always)]
unsafe fn fill_prepared_values_in_lane<const N: usize>(
    entry: &PreparedEntry,
    values: [Value; N],
    registers: *mut Value,
) -> *mut Value {
    // The callback family's window row of held words (`staged::call_callback::fill_callback_words`).
    quench_runtime::staged::call_callback::fill_callback_words(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()),
        registers.expose_provenance() as u64, values.map(Value::to_bits), entry.window(),
    );
    registers
}

/// The activation `entry` enters whose window, at `registers`, is filled: the context made the callee's. Its first record.
///
/// # Safety
/// As `enter_prepared_in_lane`'s; the window's values are written.
#[inline(always)]
unsafe fn enter_filled_in_lane(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    registers: *mut Value,
) -> Ip {
    let vm = unsafe { vm(cx) };
    unsafe { vm.lane_stack.values.set_top(registers.add(entry.size + lane_values::CONTEXT)) };
    install_entry_in_lane(cx, entry, registers);
    vm.programs.record_js_jit_lane_entry(entry.program, entry.function);
    // SAFETY: the program store keeps a derived view alive with its program.
    unsafe { (*entry.view).start() }
}

/// The context made `entry`'s callee's, running over the window at `registers`, wherever it lies.
#[inline(always)]
fn install_entry_in_lane(cx: *mut LaneContext, entry: &PreparedEntry, registers: *mut Value) {
    // SAFETY: the program store keeps a derived view alive with its program.
    unsafe {
        (*cx).registers = Registers(registers);
        (*cx).base = (*entry.view).records();
    }
    // The callback family's context row (`staged::call_callback::install_callback`).
    quench_runtime::staged::call_callback::install_callback(
        &mut quench_runtime::staged::Concrete::new(std::ptr::null()), registers.expose_provenance() as u64, entry.context(),
    );
}

/// The activation of a suspended frame (`generator_windows`) enters its window at `registers`,
/// a register file of its own that no lane stack holds, at the record of `pc`, where the frame
/// resumes: the context made the callee's.
///
/// # Safety
/// `registers` holds the callee's registers, which nothing else writes while it runs, and `pc`
/// is a record of its view.
#[inline(always)]
unsafe fn enter_suspended_in_lane(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    registers: *mut Value,
    pc: usize,
) -> Ip {
    install_entry_in_lane(cx, entry, registers);
    // SAFETY: the program store keeps a derived view alive with its program.
    unsafe { (*entry.view).records().add(pc) }
}

#[quench_macros::ext(DispatchFastVm7)]
impl Vm {
    /// `native_callback_in_lane`'s tests and resolution, made once for a native's loop of calls
    /// of `callee` with `receiver`: `None` where none of them would run in the lane.
    pub(crate) fn prepare_lane_callback(&mut self, callee: Value, receiver: Value) -> Option<PreparedLaneCallback> {
        if !self.execution_mode.allows_dynamic_execution() || self.lane_deferred.is_some() {
            return None;
        }
        let site = self.lane_native?;
        let cx: *mut LaneContext = site.cx.cast();
        if site.frames != self.frames.len()
            || context_in_lane() != cx
            || self.lane_stack.activation_count() != 0
            || self.with_scope_entered
            // SAFETY: the lane's context lives while its native runs.
            || !self.lane_stack.admits_program(unsafe { (*cx).program })
        {
            return None;
        }
        let stack = quench_runtime::stack::StackGuard::enter().ok()?;
        let epoch = self.cache_epochs.gc;
        self.lane_stack.enter_generation((epoch, self.realm.globals));
        let target = script_target_in_lane(cx, callee)?;
        let this = call_this_in_lane(self, &target, receiver)?;
        Some(PreparedLaneCallback {
            cx,
            site,
            receiver,
            target,
            this,
            epoch,
            // SAFETY: as `native_callback_in_lane`'s copy; the native's caller resumes with it.
            caller: unsafe { std::ptr::read(cx) },
            shape: None,
            pending: self.pending_call_stacks.pop().unwrap_or_default(),
            // SAFETY: as above.
            executing: quench_runtime::vm::dispatch_frame::ExecutingProgram { id: unsafe { (*cx).program }, retained: None },
            _stack: stack,
        })
    }

    /// Ends a prepared loop: the context names its caller's pending calls and executing
    /// program again (`Vm::call_prepared_lane` left the loop's named), and its pending-call
    /// stack returns to the pool.
    pub(crate) fn finish_lane_callback(&mut self, prepared: PreparedLaneCallback) {
        // SAFETY: the lane's context lives while its native, which ends the loop, runs.
        unsafe {
            (*prepared.cx).pending = prepared.caller.pending;
            (*prepared.cx).executing = prepared.caller.executing;
        }
        let PreparedLaneCallback { mut pending, .. } = prepared;
        pending.clear();
        self.pending_call_stacks.push(pending);
    }

    /// `native_callback_in_lane` of the prepared callee with `args`, past what the loop proved:
    /// it tests the frame depth, the lane's activations and program, a due collection and
    /// the collection epoch, which the native's steps between calls can change. `None`
    /// leaves the call to the general path.
    pub(crate) fn call_prepared_lane(
        &mut self,
        prepared: &mut PreparedLaneCallback,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        let cx = prepared.cx;
        let site = prepared.site;
        // SAFETY: the lane's context lives while its native runs.
        let program = unsafe { (*cx).program };
        if site.frames != self.frames.len()
            || self.lane_stack.activation_count() != 0
            || self.lane_deferred.is_some()
            || self.with_scope_entered
            || self.active_program != program
            || self.collection_due()
        {
            return None;
        }
        if self.cache_epochs.gc != prepared.epoch {
            prepared.epoch = self.cache_epochs.gc;
            self.lane_stack.enter_generation((prepared.epoch, self.realm.globals));
            let callee = prepared.target.callee;
            prepared.target = script_target_in_lane(cx, callee)?;
            prepared.this = call_this_in_lane(self, &prepared.target, prepared.receiver)?;
            prepared.shape = None;
        }
        if prepared.shape.as_ref().is_none_or(|shape| shape.argc != args.len()) {
            prepared.shape = PreparedShape::of(&prepared.caller, site.call, &prepared.target, args.len());
        }
        let registers = self.active_registers;
        // Named past the call until the loop ends (`finish_lane_callback`) or an exit restores
        // the caller's: between calls no run of this lane reads them, and a nested one restores them.
        // SAFETY: as `lane_native_callback`'s.
        unsafe {
            (*cx).pending = &mut prepared.pending;
            (*cx).executing = &mut prepared.executing;
        }
        let pushed = match &prepared.shape {
            Some(shape) => push_prepared_in_lane(cx, shape, &shape.entry(&prepared.target, prepared.this), args),
            None => None,
        };
        // A match, not `Option::or_else`: the closure's captured words were spilled and its
        // out-of-line call made on every call of the loop, though the prepared push nearly
        // always succeeds.
        let start = match pushed {
            Some(start) => start,
            None => push_callback_in_lane(cx, site.call, &prepared.target, args, prepared.this, false)?,
        };
        unsafe { (*cx).memory = MemoryView::script(cx) };
        let exit = dispatch_in_lane(cx, start);
        let outcome = match self.lane_completion.take_native_returned() {
            Some(value) => {
                debug_assert!(prepared.executing.retained.is_none(), "a completed callback's run retains no program");
                Ok(value)
            }
            None => {
                // SAFETY: handlers only return records of the active view.
                let pc = unsafe { exit.offset_from((*cx).base) } as usize;
                self.materialize_lane_exit(cx);
                let frame = unsafe { (*cx).frame() };
                debug_assert!(frame >= site.frames, "the callback's activation became a frame");
                self.frames[frame].pc = pc;
                let raised = self.lane_completion.take_raised();
                // SAFETY: as `native_callback_in_lane`'s; the copy holds no drop glue.
                unsafe { restore_context_in_lane(cx, std::ptr::read(&prepared.caller)) };
                // SAFETY: the lane's program outlives its run.
                let code = unsafe { &*prepared.caller.code };
                // The program the run left in: a frame call the lane made into another program
                // switched it (`push_frame_call_in_lane`), and the general loop resumes there.
                let executing = quench_runtime::vm::dispatch_frame::ExecutingProgram {
                    id: std::mem::replace(&mut prepared.executing.id, program),
                    retained: prepared.executing.retained.take(),
                };
                let exited = quench_runtime::vm::dispatch_frame::LaneExit { executing, raised };
                let result = self.run_frames_after_lane_exit(code, frame, &mut prepared.pending, exited);
                debug_assert_eq!(self.frames.len(), site.frames + 1, "the loop leaves the callback's frame");
                prepared.pending.clear();
                self.complete_user_frame(result, false).and_then(FrameOutcome::into_value)
            }
        };
        self.active_registers = registers;
        if self.active_program != program {
            self.set_active_program(program);
        }
        Some(outcome)
    }
}

pub(crate) mod lane_dispatch; use lane_dispatch::dispatch_in_lane;

#[cfg(test)]
pub(crate) mod instruction_set_tests;
#[cfg(test)]
pub(crate) mod call_site_history_tests;
#[cfg(test)]
pub(crate) mod update_fusion_tests;
pub(crate) mod on_demand_metadata;
use on_demand_metadata::{lane_completes_return, record_late_call};

#[cfg(test)]
pub(crate) mod numeric_pair_tests;

#[cfg(all(test, feature = "profile-field-primitives"))]
pub(crate) mod field_pair_census_tests;

#[cfg(test)]
pub(crate) mod leaf_call_tests;
#[cfg(test)]
pub(crate) mod retirement_tests;

/// Counts a leave of a lane body at a place its body had not named before on the field sites of
/// the instruction there (`Vm::note_field_leave`): a place counts once per body.
fn note_leave_in_lane(cx: *mut LaneContext, function: u32, (inlined, pc): (Option<u32>, usize)) {
    let function = inlined.unwrap_or(function);
    // SAFETY: the running lane's VM and program.
    unsafe { vm(cx).note_field_leave(&*(*cx).code, function, pc) };
}
