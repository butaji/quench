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
/// activation's completion past its op (`complete_past_op_in_lane`), else the frame's return.
macro_rules! lane_return_value {
    ($cx:expr, $ip:expr, $value:expr) => { lane_return_value!($cx, $ip, $value, NOT_LEAF) };
    ($cx:expr, $ip:expr, $value:expr, $leaf:expr) => {{
        let (cx, ip, value): (*mut LaneContext, Ip, Value) = ($cx, $ip, $value);
        let window = Registers(unsafe { vm(cx) }.lane_stack.plain.registers.get());
        if let Some((registers, next)) = by_leaf!($leaf, |L| crate::dispatch_fast::lane_return_one_op::<L>(cx, window, value)) {
            next!(cx, registers, next, Carried { memory: MemoryView::script(cx), integers: quench_runtime::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) })
        }
        if let Some(resumed) = by_leaf!($leaf, |L| crate::dispatch_fast::complete_past_op_in_lane::<L>(cx, value)) { resume!(cx, resumed) }
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
use quench_runtime::staged::Form;
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
#[cfg(feature = "jit")]
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

/// The relational operators a returning constant-right `Binary` decides per operator
/// (`returning_operator_row`, its Number relation): `$pick!(OPERATOR)` of `$operator`'s, else
/// `None`; or with `all`, the array of every operator's `$pick!`. Their miss is the general step
/// (`returning_operator_miss_of` names arithmetic alone).
macro_rules! relation_operators {
    (@with $form:ident ($($args:tt)*)) => {{
        use quench_runtime::vm::operations::binary_operator as operator;
        const EQUALITY: u32 = operator::Equality;
        const INEQUALITY: u32 = operator::Inequality;
        const STRICT_EQUALITY: u32 = operator::StrictEquality;
        const STRICT_INEQUALITY: u32 = operator::StrictInequality;
        const LESS_THAN: u32 = operator::LessThan;
        const LESS_EQUAL: u32 = operator::LessEqualThan;
        const GREATER_THAN: u32 = operator::GreaterThan;
        const GREATER_EQUAL: u32 = operator::GreaterEqualThan;
        $form!($($args)*; EQUALITY, INEQUALITY, STRICT_EQUALITY, STRICT_INEQUALITY, LESS_THAN, LESS_EQUAL, GREATER_THAN,
            GREATER_EQUAL)
    }};
    (all $pick:ident) => {
        relation_operators!(@with arithmetic_all ($pick))
    };
    ($operator:expr, $pick:ident) => {
        relation_operators!(@with arithmetic_match ($operator, $pick))
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

#[cfg(feature = "jit")]
macro_rules! arithmetic_all {
    ($pick:ident; $($op:ident),*) => {
        [$($pick!($op)),*]
    };
}

use quench_runtime::vm::engine_types::LeafForm;
const NOT_LEAF: LeafForm = quench_runtime::staged::call_leaf::NOT_LEAF;
/// Neither its receiver nor its captures (`CallEntry::frameless(Frameless::Leaf)`): it runs in its caller's context.
const PLAIN_LEAF: LeafForm = quench_runtime::staged::call_leaf::PLAIN_LEAF;
/// Its captures and not its receiver (`CallEntry::frameless(Frameless::CaptureLeaf)`): the call makes the
/// environment its own and binds no receiver unless the leaf materializes.
const CAPTURE_LEAF: LeafForm = quench_runtime::staged::call_leaf::CAPTURE_LEAF;
/// Its receiver, and maybe its captures (`CallEntry::frameless(Frameless::ContextLeaf)`): the call makes the
/// receiver and the environment its own.
const CONTEXT_LEAF: LeafForm = quench_runtime::staged::call_leaf::CONTEXT_LEAF;
/// Its receiver and not its captures (`CallEntry::frameless(Frameless::ThisLeaf)`): the call binds the receiver
/// and keeps the caller's environment, which only a materialization makes the callee's.
const THIS_LEAF: LeafForm = quench_runtime::staged::call_leaf::THIS_LEAF;


pub(crate) use quench_runtime::vm::engine_types::LaneInstruction;

use quench_runtime::vm::engine_types::NOTED_SITE_CALLEES;


use quench_runtime::vm::engine_types::NO_CALL_LINK;
/// The bit of a call link the plain call proved (`script_call_prove_miss`): its target activates
/// plainly and takes the site's argument count, so `script_call_prove_miss` tests only the callee's
/// identity. The index is the link without it (`link_index`); a link without the bit is out of
/// range for `script_call_prove_miss`, which proves it first.
const PLAIN_LINK: u16 = 1 << 15;
const _: () = assert!(SCRIPT_CALL_TARGETS[0] < PLAIN_LINK as usize && SCRIPT_CALL_TARGETS[1] < PLAIN_LINK as usize);

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

}

pub(crate) use quench_runtime::vm::engine_types::LoopSite;

#[quench_macros::ext(LoopSiteOps2)]
impl LoopSite {
    pub(crate) fn function(&self) -> u32 { self.function }

    fn enters(&self) -> bool {
        self.entering.get()
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
        // An execution-only runtime never dispatches records: no leaf return runs.
        #[cfg(not(feature = "jit"))] { let _ = form; }
        #[cfg(feature = "jit")]
        {
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
                returns.into_iter().chain(arithmetic_operators!(all pick).into_iter().flatten())
                    .chain(relation_operators!(all pick).into_iter().flatten()).collect::<Vec<(Handler, Handler)>>()
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
#[cfg(feature = "jit")]
use operand_feedback::{has_operand_feedback, seed_declared_feedback};
#[cfg(feature = "jit")]
use operand_feedback::Records;
use operand_feedback::note_operands;
#[path = "body_tier_in_lane.rs"]
pub(crate) mod body_tier_in_lane;
#[path = "loop_tier.rs"]
pub(crate) mod loop_tier;
use lane_leaves::LaneLeaves;


use quench_runtime::vm::engine_types::FIXED_ARGUMENT_COPIES;

pub use quench_runtime::vm::engine_types::copy_arguments;

use quench_runtime::vm::engine_types::WINDOW_FILL_BLOCK;

pub use quench_runtime::vm::engine_types::fill_window;


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

    /// Whether a lane call of it enters its published body (`enter_cell_body_in_lane`): one whose entry pays for
    /// itself (`LaneMethod::amortizes`). A body that does not serves native callers only (`LaneView::specialized`):
    /// the lane's call runs the callee in records, which costs a tiny method less than the body's entry.
    #[inline(always)]
    fn enters_cell_body(&self) -> bool {
        let body = self.entry.body.get();
        // SAFETY: a published cell's body word names a live body (`EntryCell::publish`), zeroed when it retires.
        body != 0 && unsafe { (*(body as *const crate::compiled::LaneMethod)).amortizes }
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


    /// Whether its body specializes again at its next entry: it left too often (an ahead image's included).
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




/// The generated rows of the hot `this` sites (a `this` own read, a formed `this` store, a `this`
/// nested read's first hop) and their trusted forms (`lane_frame::trusted_this_rows`).
const TRUSTED_THIS_ROWS: [(Handler, Handler); 3] = [
    (generated_row!(53, ACC_NONE, 14), lane_frame::trusted_this_rows::generated_row::<53, ACC_NONE, 14>),
    (generated_row!(62, ACC_NONE, 42), lane_frame::trusted_this_rows::generated_row::<62, ACC_NONE, 42>),
    (generated_row!(53, ACC_NONE, 8), lane_frame::trusted_this_rows::generated_row::<53, ACC_NONE, 8>),
];

/// Whether `function`'s `this` is a direct object in every activation, by the solve's receiver cell
/// (`FactKey::Receiver`): the function's rows and code take it as one, with no tag test. A live receiver
/// (`AotFacts::live`: new code may call the function with any `this`) is never trusted, since no edge
/// retires a trusted row; a closed receiver never changes, so nothing subscribes to it.
pub(crate) fn receiver_trusted(facts: Option<&quench_runtime::compile::aot_facts::AotFacts>, function: u32) -> bool {
    let key = quench_runtime::typed_ir::FactKey::Receiver(function);
    // A cell no value reaches (`Type::NEVER`) is no fact: a function the host enters directly has none.
    facts.is_some_and(|facts| !facts.live(key) && facts.cell(key).is_some_and(|cell| cell.ty != quench_runtime::typed_ir::Type::NEVER
        && cell.ty.is_subtype(quench_runtime::typed_ir::Type::DIRECT_OBJECT_FAMILY)))
}

/// The trusted form of a row that reads `this` (`TRUSTED_THIS_ROWS`), `None` for a row with no trusted form.
pub(crate) fn trusted_this_form(handler: *const ()) -> Option<*const ()> {
    TRUSTED_THIS_ROWS.iter().find(|(plain, _)| *plain as *const () == handler).map(|(_, trusted)| *trusted as *const ())
}

/// Gives each row of a view whose receiver is trusted (`receiver_trusted`) its trusted form.
pub(crate) fn trust_this_rows(records: &mut [quench_runtime::vm::engine_types::LaneInstruction]) {
    for record in records.iter_mut() {
        if let Some(trusted) = trusted_this_form(record.handler) { record.handler = trusted; }
    }
}





/// The context words below register 0 (`staged::frame`, the one source of the frame layout).
use quench_runtime::staged::frame::{CONTEXT_BODY, CONTEXT_CALL, CONTEXT_CALLEE, CONTEXT_CALLER_BASE, CONTEXT_CALLER_REGISTERS, CONTEXT_CAPTURE, CONTEXT_ENV, CONTEXT_PACKED, CONTEXT_THIS};
const _: () = assert!(CONTEXT_BODY == quench_runtime::vm::register_file::FRAME_CONTEXT);
const _: () = assert!(CAPTURE_STORES as u64 == quench_bjit::shared_words::CAPTURE_STORES_BIT);


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
        (!matches!(self.kind, ActivationKind::Native)).then(|| unsafe { pc_in_view(self.caller_base, self.call) })
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
use call_entry::{CallEntry, Framed, Frameless};

// Recognized once per target, as it resolves; no operation runs them.
#[path = "call_body_forms.rs"]
pub(crate) mod call_body_forms;
use call_body_forms::{body_entry, leaf_body};

/// `ScriptCallTarget::forms`: `new` may activate it.
const TARGET_CONSTRUCTS: u8 = 1;
/// `ScriptCallTarget::forms`: `new` may run its body as the stores alone.
const TARGET_INITIALIZES: u8 = 2;
/// `ScriptCallTarget::forms`: a rest body (`CallEntry::framed(Framed::Gathers)`) that keeps the values it was
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
/// `ScriptCallTarget::forms`: a settling target (`CallEntry::framed(Framed::Settles)`) whose body activates
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
        self.entry == CallEntry::framed(Framed::Keeps) || self.entry == CallEntry::framed(Framed::Gathers)
    }

    /// Its activation's return settles its call's promise (`Completion::SettlesPromise`).
    #[inline(always)]
    fn settles(&self) -> bool {
        self.entry == CallEntry::framed(Framed::Settles)
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


pub(crate) use quench_runtime::vm::engine_types::PlainFrames;
pub(crate) const PLAIN_DEPTH_OFFSET: usize = std::mem::offset_of!(PlainFrames, depth);
pub(crate) const PLAIN_REGISTERS_OFFSET: usize = std::mem::offset_of!(PlainFrames, registers);
const _: () = {
    use quench_runtime::staged::frame;
    assert!(PLAIN_DEPTH_OFFSET == frame::PLAIN_DEPTH_AT && PLAIN_REGISTERS_OFFSET == frame::PLAIN_REGISTERS_AT);
    assert!(std::mem::offset_of!(PlainFrames, program) == frame::PLAIN_PROGRAM_AT);
};

/// The words of a lane run's direct calls that change per entry (`Vm::stack_entry_in_lane`): the end of
/// the frames the run may nest above the activations' top, and the end of the values' capacity, which
/// a reallocation of the values moves (`LaneStack::reserve_values`) during a run.
#[derive(Clone, Copy)]
pub(crate) struct LaneStackEntry {
    pub(crate) frames_end: usize,
    pub(crate) values_end: usize,
}

/// The addresses a lane run's direct calls push through (`Vm::stack_storage_in_lane`), bound once per run
/// (`BodyRunContext::bind_run`). Each names a field of the `LaneStack` the `Vm` owns, or a table mapped
/// once for the stack, so none moves while the run's `LaneStack` stands: a reallocation changes the
/// values' or the records' buffer, never their top words.
#[derive(Clone, Copy)]
pub(crate) struct LaneStackStorage {
    pub(crate) values_top: usize,
    pub(crate) activations_top: usize,
    /// The plain frames' words (`PlainFrames`), and the cells of the native header bound, the frames
    /// below the plain headers, and the body top (`LaneStack::plain_depth_limit` and the rest). Their
    /// values change per entry; these addresses do not.
    pub(crate) plain_depth: usize,
    pub(crate) plain_depth_limit: usize,
    pub(crate) activation_count: usize,
    pub(crate) body_table: usize,
    pub(crate) body_top: usize,
    pub(crate) native_table: usize,
}

/// The words a lane run's direct calls push onto, bound together (`Vm::stack_words_in_lane`).
#[derive(Clone, Copy)]
pub(crate) struct LaneStackWords {
    pub(crate) entry: LaneStackEntry,
    pub(crate) storage: LaneStackStorage,
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

    /// Whether a proved plain call (`plain_call_activation`) may push one more plain activation
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
    /// word every link's admission is compared with; the diagnostics bit (`Admission::DIAGNOSED`) refuses only proved links.
    #[inline(always)]
    pub(crate) fn admits_program(&self, program: ProgramId) -> bool {
        (self.admission.0 & !Admission::DIAGNOSED) >> Admission::PROGRAM_SHIFT == program.raw() as u64
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



use quench_runtime::vm::engine_types::OuterWalk;

const NO_OUTER_WALK: OuterWalk = OuterWalk { env: Value::DELETED, depth: 0, slots: std::ptr::null() };

/// The tag of a `LaneContext::capture` whose every dense slot a store may write directly:
/// its environment admits every direct store (`Vm::admits_every_direct_store`) and is in the
/// remembered set already, so a write needs no barrier until the next collection, which
/// retires the target that tagged it.
pub(crate) const CAPTURE_STORES: usize = 1;
const _: () = assert!(std::mem::align_of::<usize>() > CAPTURE_STORES);






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

/// The running lane's fuel (`LaneContext::fuel`), for the handlers that do not carry the
/// context and for `note_allocation`; null while no lane runs.
#[thread_local]
static LANE_FUEL: TlsCell<*mut u32> = TlsCell(std::cell::Cell::new(std::ptr::null_mut()));

/// Backward jumps a lane run takes between two safepoint visits: the bound on how long a
/// termination request from another thread waits (it raises the floor, which the visit reads).
const LANE_FUEL_BATCH: u32 = 4096;



/// A lane run's raised floor (`Vm::enter_lane_floor`): the VM's floor cell, and the cell and
/// floor of the run it nests in, which leaving restores.
pub(crate) struct LaneFloor {
    pub(crate) cell: *const std::sync::atomic::AtomicUsize,
    pub(crate) outer_cell: *const std::sync::atomic::AtomicUsize,
    pub(crate) outer_floor: usize,
}


/// Stack a lane run may use below its entry before a safepoint jump returns
/// to the lane loop; only reachable should a build not form sibling calls.
const LANE_STACK_SLACK: usize = 64 * 1024;

/// The floor that sends the next safepoint jump through the slow path.
const COLLECTION_DUE_FLOOR: usize = quench_runtime::execution_interrupt::LANE_FLOOR_RAISED;







/// Records are word aligned, so the low address bit is free to mark a lane
/// exit.
const EXIT_TAG: usize = 1;
const _: () = assert!(std::mem::align_of::<LaneInstruction>() > EXIT_TAG);






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

/// `lane_handler!` for a handler whose records a Wasm view also dispatches: the prologue takes
/// the view's bytes (`MemoryView::script` makes them the lane context) and the body reads the
/// thread's where its record is a Wasm one (`thread_context_row`).
macro_rules! view_handler {
    ($(#[$meta:meta])* $vis:vis fn $name:ident<$($param:ident: $ty:ty),*>($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident) $body:block) => {
        lane_handler!(@define bytes [{ bytes.cast::<LaneContext>() }] $(#[$meta])* $vis fn $name<$($param: $ty),*>($cx, $r, $ip, $i, $view) $body);
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
pub(crate) const NO_ACCUMULATOR: u16 = u16::MAX;
use quench_runtime::vm::engine_types::ACC_A;
use quench_runtime::vm::engine_types::ACC_B;
use quench_runtime::vm::engine_types::ACC_C;
/// The first register of the immediate's register pair: a select's
/// condition.
const ACC_PAIR: u8 = 4;
/// Accumulator positions a record may read: none, A, B, C and the pair.
pub(crate) const ACC_POSITIONS: usize = 5;




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


view_handler! {
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
use loop_body::LoopBody;
use lane_frame::{protocol_handler, returning_operator_row, operator_row, protocol_family, is_row, jump_row, jumps_by_row, plain_row, row_miss, row_near, row_produces, FLAG_LOOP, FLAG_ROOT, FLAG_SET, FLAG_UNREAD, FLAG_WINDOW, admits_row, name_row, FLAG_NAME_GLOBAL, FLAG_NAME_GLOBAL_SCOPED, FLAG_NAME_LEXICAL, FLAG_NAME_INTRINSIC, FLAG_NAME_NATIVE, FLAG_NAME_UNDEFINED, FLAG_SCOPE};


pub(crate) mod capture_append;
pub(crate) mod mapped_windows;
pub(crate) mod string_case_filter;










script_handler! {
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
            (&*(*cx).code, (*cx).frame(), record_pc(cx, ip) as usize)
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
        } else if !left.is_heap() && !right.is_heap() && !left.is_internal_marker() && !right.is_internal_marker() {
            // Two unequal words outside the heap (`v === undefined` of a Number, `x === null` of a
            // Boolean): only two Numbers may still be equal (an int32 and its double, `0` and `-0`);
            // any other pair differs in type or value, so no heap cell need decide it.
            Some(left.as_number().zip(right.as_number()).is_some_and(|(left, right)| left == right))
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

























script_handler! {
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
            // An array's hole (`new Array(n)` read where nothing was written): the typed and string
            // probes below only decline an array, so it is asked before them.
            .or_else(|| vm.array_hole_element(base, key))
            // A BigInt kind's element has no typed row, so its record misses here each read: asked first.
            .or_else(|| allocated_element_in_lane(vm, base, key, Vm::bigint_typed_element))
            .or_else(|| if registers { row_typed_element(ip, vm, base, key) } else { vm.plain_typed_element(base, key) })
            .or_else(|| allocated_element_in_lane(vm, base, key, Vm::string_element))
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

/// The site facts of a keyed access of a typed array's element, one per native kind
/// (`typed_elements::native_kind_at`; `requicken_typed`): the row proves the array's view for
/// the kind its site fixes (`LaneFrame::kept_view`) and reads or stores the element inline with
/// no dispatch on the kind; a miss takes the host path (`row_index_read_miss`, `row_index_write`),
/// which records the kind again.
const TYPED_KIND_SITES: [u8; quench_runtime::staged::typed_elements::NATIVE_KINDS] = {
    const FORMS: [usize; quench_runtime::staged::typed_elements::NATIVE_KINDS] = [26, 27, 28, 29, 51, 52, 53, 54, 55];
    let mut sites = [0; quench_runtime::staged::typed_elements::NATIVE_KINDS];
    let mut index = 0;
    while index < FORMS.len() {
        sites[index] = own_site(false, OWN_FORMS.len() + FORMS[index]);
        index += 1;
    }
    sites
};





const FIELD_STORE: u16 = field_store_opcode(StoreReceiver::Register, false);
const FIELD_STORE_STRICT: u16 = field_store_opcode(StoreReceiver::Register, true);
const THIS_FIELD_STORE: u16 = field_store_opcode(StoreReceiver::This, false);
const THIS_FIELD_STORE_STRICT: u16 = field_store_opcode(StoreReceiver::This, true);




/// The storage forms a store's own-field instance fixes, as `own_site` form indexes that `own_site_form`
/// decodes: the flat run (`0`) and packed inline values (`INLINE_FORM`). Their sites (41 to 44) are below
/// the returning sites and clash with no `FLAG_*` value, and no other site fact uses these indexes.
const STORE_FLAT: usize = OWN_FORMS.len() + 11;
const STORE_INLINE: usize = OWN_FORMS.len() + 12;




/// The storage forms an own-field read's hint fixes, by index: the generated row's site fact
/// (`own_site`) names one, so the row folds the layout it reads (`concrete_hinted_own_field_hit`).
/// `HINTED_FORM` reads the hint layout's own bits.
const OWN_FORMS: [u32; 9] = {
    use quench_runtime::staged::TAGGED_SLOTS;
    use quench_runtime::value_vec::{EXTRAS_FORM, INLINE_FORM, NARROW_FORM};
    [HINTED_FORM, 0, EXTRAS_FORM, INLINE_FORM, EXTRAS_FORM | INLINE_FORM, NARROW_FORM,
        TAGGED_SLOTS, EXTRAS_FORM | TAGGED_SLOTS, EXTRAS_FORM | INLINE_FORM | TAGGED_SLOTS]
};


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
/// The cell-own sites of an arena run of slot words where slots hold raw doubles (`switches::float_slots`, every JIT
/// mode): the row folds the layout's form and decodes the word as a float slot's (`decode_float_word`), with no
/// test of the switch or the hint layout's form, as an own-field site of the untagged run form does.
pub(crate) const CELL_FLOAT_RUN_THIS: u8 = own_site(true, OWN_FORMS.len() + 31);
pub(crate) const CELL_FLOAT_RUN_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 31);
/// The float cell-own sites of a run with an extras block (a function with many static members, `EXTRAS_FORM`).
pub(crate) const CELL_FLOAT_EXTRAS_THIS: u8 = own_site(true, OWN_FORMS.len() + 32);
pub(crate) const CELL_FLOAT_EXTRAS_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 32);
/// The cell-own sites of a run with an extras block where slots hold tagged words: the slot load past the layout guard.
pub(crate) const CELL_EXTRAS_THIS: u8 = own_site(true, OWN_FORMS.len() + 33);
pub(crate) const CELL_EXTRAS_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 33);

/// The site fact of a `GetField` of a local slot's row (`local_field_handler`): an own-field read of a
/// plain local, whose hint the record binds like an own read's. Own-form index 30, a
/// free one below the returning sites.
pub(crate) const LOCAL_FIELD_SITE: u8 = own_site(false, OWN_FORMS.len() + 30);
/// The site fact of a register read of one object's own constant (`IcOp::OwnConstant`): the
/// record's hint names an own-constant hint (`OWN_CONSTANT_HINT`, `Vm::own_constant_operands`).
pub(crate) const OWN_CONSTANT_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 8);

/// The site facts of a register's `length`: an array's (`IcOp::ArrayLength`) and a string's
/// (`IcOp::StringLength`); a miss takes `row_length_other`.
pub(crate) const ARRAY_LENGTH_SITE: u8 = own_site(false, OWN_FORMS.len() + 9);
pub(crate) const STRING_LENGTH_SITE: u8 = own_site(false, OWN_FORMS.len() + 10);

/// The site facts of a read the VM's geometry or length answers from the receiver's word, which no
/// record addresses (`FieldPlan::Length`, `FieldPlan::Geometry`): a register's `length` of an
/// arguments object, a user function or a typed view (`TYPED_LENGTH_SITE`, a miss to the array row),
/// and any geometry name of a typed view, DataView, Map or Set (`GEOMETRY_SITE`, a miss to the
/// general read).
pub(crate) const TYPED_LENGTH_SITE: u8 = own_site(false, OWN_FORMS.len() + 20);
pub(crate) const GEOMETRY_SITE: u8 = own_site(false, OWN_FORMS.len() + 21);

/// The site fact of a number receiver's read through a `Number.prototype` hint (`IcOp::HeldNumber`),
/// as `n.toFixed`: own-form index 14.
pub(crate) const NUMBER_SITE: u8 = own_site(false, OWN_FORMS.len() + 14);

/// The site facts of a read that met a getter whose hint returns an own field of the receiver
/// (`IcOp::GetterOwn`, `Vm::getter_hint_operands`), of `this` and of a register; a miss takes the
/// getter's refresh, which makes the hint (`row_get_field_getter_refresh`).
pub(crate) const GETTER_THIS: u8 = own_site(true, OWN_FORMS.len() + 15);
pub(crate) const GETTER_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 15);

/// The site fact of a nested read `a.b.c` of two own entries (`get_field_nested_miss`'s steps):
/// the record's hint is the first step's entry, the site's second entry the second; the row runs
/// them as a chain of two own hops (`RecordPlan::nested`), and its miss takes the hand read that
/// refreshes the hint.
pub(crate) const NESTED_SITE: u8 = own_site(false, OWN_FORMS.len() + 16);

/// The formed nested sites: `NESTED_SITE`'s row with each hop's storage form fixed, the first hop's
/// (the record's hint) and the second's (the intermediate object's), each a flat run or packed inline
/// values (`nested_hop_class`). Indexes 43 to 46 are below the returning sites (from 59) and no other
/// site fact uses them.
const NESTED_PAIR_BASE: usize = OWN_FORMS.len() + 34;
pub(crate) const NESTED_FLAT_FLAT: u8 = own_site(false, NESTED_PAIR_BASE);
pub(crate) const NESTED_FLAT_INLINE: u8 = own_site(false, NESTED_PAIR_BASE + 1);
pub(crate) const NESTED_INLINE_FLAT: u8 = own_site(false, NESTED_PAIR_BASE + 2);
pub(crate) const NESTED_INLINE_INLINE: u8 = own_site(false, NESTED_PAIR_BASE + 3);





/// The site facts of the general read (`FieldPlan::Host`): the record names no plan the row
/// runs, so its hit is the VM's cached read of the atom through the site's cache
/// (`general_field_read`, the value `lane_get_field` made), of `this` and of a register. Its miss
/// is `row_get_field_general`, the general read's activation path.
pub(crate) const GENERAL_THIS: u8 = own_site(true, OWN_FORMS.len() + 18);
pub(crate) const GENERAL_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 18);




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

/// The site facts of a tracked store installed for its slot's held type (`StorePlan::Held`,
/// `slot_store::HELD_*`), one per type a store keeps in place: a tagged slot's row stores with no
/// class test, an int slot's with one, an object slot's and an object-or-null slot's with one or two.
/// A store the slot's type refuses takes the tracked row (`TRACKED_SITE`), which stores, widens and
/// installs the row of the type the slot holds now (`tracked_store_row`). Free own-form indexes.
pub(crate) const TRACKED_TAGGED_SITE: u8 = own_site(false, OWN_FORMS.len() + 13);
pub(crate) const TRACKED_INT_SITE: u8 = own_site(false, OWN_FORMS.len() + 19);
pub(crate) const TRACKED_OBJECT_SITE: u8 = own_site(false, OWN_FORMS.len() + 43);
pub(crate) const TRACKED_OBJECT_OR_NULL_SITE: u8 = own_site(false, OWN_FORMS.len() + 49);

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





/// The site facts of the generated entry reads (`FieldPlan::Hinted`, `FieldPlan::Entry`): each
/// one hit shape of the VM's entry lookups, which the row calls by the kind it names
/// (`LaneFrame::field_entry`).
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



/// The site facts of a private name's read of `this` and of a register (`FieldPlan::Private`):
/// the record's private field at its own address (`LaneFrame::private_value`). A method, accessor
/// or missing field takes `get_private_field_miss`, the general read.
pub(crate) const PRIVATE_FIELD_THIS: u8 = own_site(true, OWN_FORMS.len() + 48);
pub(crate) const PRIVATE_FIELD_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 48);

/// The site facts of a read the VM's cache answers, of `this` and of a register (`FieldPlan::Host`):
/// the cached read of the site's atom (`LaneFrame::cached_read`). A miss takes `lane_get_field`,
/// the general read, which runs the getter and the data lookups.
pub(crate) const CACHED_FIELD_THIS: u8 = own_site(true, OWN_FORMS.len() + 42);
pub(crate) const CACHED_FIELD_REGISTER: u8 = own_site(false, OWN_FORMS.len() + 42);

/// The site facts of a returning own-field read (`RETURNING_FIELD_OWN`'s row, k 50 up: the topmost site facts): the record's own
/// hint, for `this` and a register, at each leaf form the record returns from (`LeafForm`). The
/// row completes the return with its leaf form's completion, so the leaf forms are const site
/// facts of one family: `returning_site(this, leaf)`.
const RETURNING_FORM: usize = OWN_FORMS.len() + 50;
const RETURNING_BASE: u8 = own_site(false, RETURNING_FORM);









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
                        let form = quench_runtime::tier_surface::layouts::auxiliary(layout) & (EXTRAS_FORM | INLINE_FORM | NARROW_FORM);
                        let run = form == 0;
                        Some(if run && !quench_runtime::vm::switches::float_slots() {
                            single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_RUN_THIS),
                                generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_RUN_REGISTER), general_handler(i.b))
                        } else if run {
                            single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_FLOAT_RUN_THIS),
                                generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_FLOAT_RUN_REGISTER), general_handler(i.b))
                        } else if form == EXTRAS_FORM && quench_runtime::vm::switches::float_slots() {
                            single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_FLOAT_EXTRAS_THIS),
                                generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_FLOAT_EXTRAS_REGISTER), general_handler(i.b))
                        } else if form == EXTRAS_FORM {
                            single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_EXTRAS_THIS),
                                generated_row!(FIELD_LEAF_READ, ACC_NONE, CELL_EXTRAS_REGISTER), general_handler(i.b))
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



pub(crate) use quench_runtime::vm::engine_types::FieldSiteTier;













script_handler! {
    /// A nested `GetField` whose steps hit their entries without guest code
    /// (`lane_nested_value`); any other read takes the general row (`row_get_field_general`). A miss
    /// retires a formed instance to `NESTED_SITE`, and a hit re-chooses the instance from the hops read
    /// (`requicken_nested`).
    fn get_field_nested_miss<>(cx, r, ip, i, view) {
        requicken(ip, generated_row!(FIELD_LEAF_READ, ACC_NONE, NESTED_SITE));
        let Some(value) = (unsafe { lane_nested_value(cx, r, ip, i) }) else {
            sibling!(row_get_field_general, r, ip, Carried::script_in(cx, view))
        };
        unsafe { requicken_nested(cx, r, ip, i) };
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



/// The private-name site keys of a nested read's two steps, as offsets from its record's
/// address (`LaneFrame::private_slot`): a record spans more bytes than steps, so no other
/// record's key is one of them.
const NESTED_STEPS: [usize; 2] = [1, 2];



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






/// What a lane read of a private name of an object resolves to (`get_private_field_miss`).
enum PrivateRead {
    Value(Value),
    /// A private accessor's getter, called with the object.
    Getter(Value, Value),
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










script_handler! {
    /// A loop's backward jump in compiled execution (`LoopSite`): the jump's safepoint, then
    /// the loop's lane body once the jump is hot and the body is ready (`loop_body::enter_loop_body`).
    fn loop_jump_miss<>(cx, r, ip, i, view) {
        let site = i.loop_site();
        if site.diagnostics {
            site.lane_backedge_count
                .set(site.lane_backedge_count.get().saturating_add(1));
        }
        // SAFETY: the site records the distance to a validated jump target.
        let header = unsafe { ip.byte_offset(site.jump) };
        if script_fuel_out(cx) {
            return safepoint_at(ip, header, Slot::unset());
        }
        // A refused compile (`LoopSite::refuse_warming`) is one epoch compare and a countdown: the back edge is a plain jump.
        if site.refused_now() {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        if site.interprets() {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        // The header rule (`enter_loop_body`): a ready body first.
        if let Some(attempt) = loop_body::enter_loop_body(cx, r, ip, site) {
            lane_attempt!(cx, r, attempt);
        }
        // A loop whose body is still to come keeps its backward jump, which a loop with no body
        // would otherwise retire before the body is planned.
        if crate::compiled::admission::lane_bodies_enrolled(unsafe { vm(cx) }) && site.body.borrow().pending() {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        // A loop whose calls no body inlines yet is offered a body once (`offer_body`).
        if loop_body::offer_body(cx, r, site) {
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        // A loop with no body takes its plain jump from now on.
        loop_retire_in_lane(cx, ip, site);
        next!(cx, r, header, Carried::script_in(cx, view))
    }
}

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






















pub(crate) mod wasm_instance;
#[cfg(feature = "jit")]
use wasm_instance::lane_instance_binding_memory;


script_handler! {
    /// f64 square root: an arithmetic operator, so a NaN operand matters
    /// only as a NaN (see `float_bits64`), and its result is held in place.
    fn f64_sqrt_miss<ACC: u8>(cx, r, ip, i, view) {
        let input = f64::from_bits(read::<ACC, ACC_B>(r, i.b, view).float_bits64());
        produce!(cx, r, ip, i.a, Value::from_float_bits64(input.sqrt().to_bits()), view)
    }
}








// The lane holds 64-bit payloads only in place (`Value::from_bits64`): a
// boxed operand or a result that needs a box leaves the lane, and the
// general loop, which boxes, runs the instruction. The handlers then make
// no calls.





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
            let mut call_pc = unsafe { pc_in_view(activation.caller_base(), activation.call()) };
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
            let this = self.frame_lane_this(caller);
            registers.set_this(this);
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
    pub(crate) fn frame_lane_this(&self, frame: usize) -> Value {
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
        if crate::compiled::admission::vm_may_run(self, crate::compiled::admission::Tier::ANY) {
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
    /// registers (`CallPlan::registers_only`), else a `CallEntry::framed(Framed::Frame)` entry, so its calls
    /// take a frame without resolving it again.
    #[inline(never)]
    pub(crate) fn resolve_script_call_target(&mut self, code: &ResidualProgram, callee: Value) -> Option<ScriptCallTarget> {
        self.resolve_call_target_in_lane(code, callee, false)
    }

    /// `resolve_script_call_target`, deriving the callee's view at once where `now`: an
    /// activation recovery makes (`body_inlined_activation_in_lane`) for a callee whose runs
    /// all ran inlined, so none counted toward its view (`call_derives_in_lane`).
    #[inline(never)]
    pub(crate) fn resolve_call_target_in_lane(&mut self, code: &ResidualProgram, callee: Value, now: bool) -> Option<ScriptCallTarget> {
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
        if !(if now { self.call_derives_in_lane(program, function, true) } else { self.call_may_derive_in_lane(program, function) }) { return None; }
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
        // whose promise its return settles (`CallEntry::framed(Framed::Settles)`); across programs, or under a
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
                entry: CallEntry::framed(Framed::Frame),
                ..EMPTY_SCRIPT_CALL_TARGET
            };
            self.remember_target(target, numeric, cross);
            return Some(target);
        };
        let tier_may_claim = self.js_tier_may_claim(code, program, function);
        // The callee's entry form is the one its composed signature gives (`call_plan::entry_form`), of the facts of
        // its own program; a target is a callee's fact, so the form is the same at every site that calls it. The
        // body's shape is taken within it.
        let frameless = quench_runtime::compile::call_plan::entry_form(self.programs.aot_facts(program).as_deref(), function)
            == quench_runtime::staged::effects::CallForm::Frameless;
        // SAFETY: the program store keeps a derived view alive with its program.
        let shape = *unsafe { &*view }.target_body.get_or_init(|| {
            let constructs = Self::inline_constructible(body);
            // A class constructor's calls throw on the general path, so they take a
            // frame; only its constructions activate frameless (`script_construct_miss`).
            // A frameless form whose cell holds a body that calls only natively enters it as `Frameless::Native`.
            // SAFETY: as above; the cell's body is pinned by the view while it is published.
            let native = frameless && unsafe { &*view }.cell_method().is_some_and(|method| method.calls_only_natively());
            let entry = body_entry(body, &code.constants, frameless, native);
            // SAFETY: the program store keeps a derived view alive with its program.
            unsafe { &*view }.entry.set_facts(quench_runtime::staged::call::CellFacts::of_function(body.registers, body.params, entry_fill(code, body), entry == CallEntry::framed(Framed::Activates) && !environment));
            TargetBody {
                entry,
                constructs,
                initializes: constructs && body.only_initializes_this_fields(&code.constants),
                // A leaf is a frameless entry, so only a frameless call form has one (`CallEntry::leaf`).
                leaf: (frameless && entry == CallEntry::framed(Framed::Activates))
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
            && shape.entry == CallEntry::framed(Framed::Activates)
            && tier_may_claim
            // SAFETY: as below.
            && (!crate::compiled::admission::lane_bodies_enrolled(self) || unsafe { &*view }.tier_unclaimed_in_lane())
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
        !cross || crate::compiled::admission::lane_bodies_enrolled(self)
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
        crate::compiled::admission::lane_bodies_enrolled(self) || !self.execution_mode.allows_dynamic_execution()
    }

    pub(crate) fn admits_in_lane(&self, id: ProgramId, code: &ResidualProgram) -> bool {
        self.may_run_in_lane(id, code) && self.specialized_operations_enabled(code)
    }

    /// The words a lane run's direct calls push onto (`compiled::DirectCallee`), bound together: the
    /// per-entry words (`stack_entry_in_lane`) and the storage's addresses (`stack_storage_in_lane`).
    pub(crate) fn stack_words_in_lane(&mut self, frames: usize) -> LaneStackWords {
        let entry = self.stack_entry_in_lane(frames);
        LaneStackWords { entry, storage: self.stack_storage_in_lane() }
    }

    /// The per-entry words a lane run's direct calls push onto (`stack_words_in_lane`): the end of the
    /// `frames` activations the run may nest above the activations' top, bounded by what the lane may
    /// still make of them, and the end of the values' capacity. It also binds the frame counts the
    /// run's emitted pushes check, which change at every entry.
    pub(crate) fn stack_entry_in_lane(&mut self, frames: usize) -> LaneStackEntry {
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
        // Emitted pushes keep body_top at the total count between bindings. A record the general
        // loop stepped may push an ordinary frame, which the lane's counter does not see (an image
        // body steps records it cannot run), so the counter may trail the total; it never runs ahead of it.
        debug_assert!(stack.body_top.get() <= stack.activation_count(), "body_top names at most the total frame count at binding");
        stack.body_top.set(stack.activation_count());
        stack.frames_below.set(stack.frames_below_plain());
        // A binding that admits no direct-call frames admits no native headers either: a run with a
        // pending activation (a cell body entered as the context's leaf, `enter_cell_body_in_lane`)
        // has its caller still unmaterialized, and a header pushed over it would end above the
        // activation the leaf becomes, with a stale caller (earley-boyer under --u-jit). Its calls
        // take the helper row, which materializes the leaf first (`native_frames::in_innermost_frame`).
        stack.plain_depth_limit.set(if frames != 0 { stack.activation_limit.get().min(LANE_STACK_ACTIVATIONS) } else { 0 });
        LaneStackEntry { frames_end, values_end }
    }

    /// The addresses of the lane stack's storage a lane run's direct calls push through
    /// (`LaneStackStorage`). Unlike `stack_entry_in_lane`, this changes nothing.
    pub(crate) fn stack_storage_in_lane(&self) -> LaneStackStorage {
        let stack = &self.lane_stack;
        // SAFETY: reads the records' top word's address alone; no reference into them is made.
        let records = unsafe { &*stack.activation_records.get() };
        LaneStackStorage {
            values_top: stack.values.top_word().addr(),
            activations_top: records.top_word().addr(),
            plain_depth: (&raw const stack.plain).expose_provenance(),
            plain_depth_limit: stack.plain_depth_limit.as_ptr().addr(),
            activation_count: stack.frames_below.as_ptr().addr(),
            body_table: stack.body_table.as_ptr().addr(),
            body_top: stack.body_top.as_ptr().addr(),
            native_table: stack.native_table.as_ptr().addr(),
        }
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
            crate::ahead_runner::view_in_lane(self, program, function, code, constants, kind, live, sites, image)
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
    #[cfg(feature = "jit")]
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
                let facts = self.programs.aot_facts(id);
                let planned = self.programs.planned_call_sites(id, function, planned_calls::planned_arm_admits);
                let plan = self.programs.plan_lane_view(id, function, |code, constants, kind, live, sites| {
                    derive_lane_view(function, code, constants, self.heap_reads(), kind, live, sites, crate::compiled::admission::record_plan(false), false,
                        self.programs.tiers(), facts.as_deref(), planned)
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


script_handler! {
    /// The miss of a linked target whose entry is not a plain activation: the site
    /// calls the form for that entry directly from now on, an environment or keeping
    /// activation its own handler and any other entry the out-of-line form, which sorts them
    /// out. Out of line, reached by `sibling!`, so the miss makes no call.
    #[inline(never)]
    fn script_call_requicken_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let target = vm.linked_script_call_entry(r.get(i.b), &i.call_link);
        // SAFETY: as `Vm::script_call_entry`; nothing before this read resolves a target.
        let entry = target.map(|target| unsafe { (*target).entry });
        let handler: Handler = match entry {
            Some(entry) if entry == CallEntry::framed(Framed::Environment) => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::ENVIRONMENT as u16 }>(),
            Some(entry) if entry == CallEntry::framed(Framed::Settles) => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::SETTLES as u16 }>(),
            Some(entry) if entry == CallEntry::framed(Framed::Keeps) || entry == CallEntry::framed(Framed::Gathers) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::SCRIPT_CALL_KEEPING as u16 }>(),
            Some(entry) if entry.leaf_form() != NOT_LEAF => leaf_call_handler(entry, i),
            Some(_) => call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>(),
            None => sibling!(script_call_resolve_miss, r, ip, Carried::script_in(cx, view)),
        };
        requicken(ip, handler);
        sibling!(handler, r, ip, Carried::script_in(cx, view))
    }
}



script_handler! {
    /// The miss of a call row (`script_call_row`) whose entry the row does not admit: the target the
    /// site links, which takes its entry's form where that is not a plain activation
    /// (`script_call_requicken_miss`), and the out-of-line form where the site's arguments are more than
    /// it takes. Else the entry is proved (`Admission::UNPROVEN` cleared), the link marks it (`PLAIN_LINK`),
    /// and the call runs through its row, which admits it by the entry's proof, the argument count and the
    /// room (`CallFrame::call_entry`). A returning call the run refuses (`returning_call_admitted`) takes the
    /// general path. An entry already proved, whose row declined the call, takes the full form: the row's
    /// decline is final, so the row is not entered again. A callee whose cell holds a body is proved too:
    /// the row's activation tests the cell.
    #[cold]
    #[inline(never)]
    pub(crate) fn script_call_prove_miss<ARGC: u16, PLAIN: bool>(cx, r, ip, i, view) {
        if !unsafe { (*cx).script_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        let Some(target) = vm.linked_script_call_entry(r.get(i.b), &i.call_link) else {
            sibling!(script_call_resolve_miss, r, ip, Carried::script_in(cx, view))
        };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let target = unsafe { &*target };
        if target.entry != CallEntry::framed(Framed::Activates) {
            sibling!(script_call_requicken_miss, r, ip, Carried::script_in(cx, view))
        }
        let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
        if returns && !returning_call_admitted(cx) {
            return exit(ip);
        }
        let argc = call_argument_count::<ARGC>(i);
        if argc > FIXED_ARGUMENT_COPIES || !quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(argc)) {
            sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
        }
        // The proof (`Admission::UNPROVEN`): the entry, which the link names, now admits the row's calls
        // from every site whose arguments it takes.
        let (index, resolved) = (link_index(i.call_link.get()), std::ptr::from_ref(target));
        let entry = &mut vm.lane_stack.script_targets.entries[index];
        debug_assert!(std::ptr::eq(entry, resolved), "the link names the entry it resolved");
        // An entry proved already is a site meeting another callee than its link named (a polymorphic
        // `this.task.run(p)`): the link now names this one, and the call activates in place, not
        // back through the row, which declined it.
        if entry.admission.0 & Admission::UNPROVEN == 0 {
            match activate_unproven_in_lane::<ARGC>(cx, r, ip, i, target) {
                Some(resumed) => resume!(cx, resumed),
                None => sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view)),
            }
        }
        entry.admission.0 &= !Admission::UNPROVEN;
        debug_assert!(entry.admission == vm.lane_stack.admission, "an admitted entry's word is the lane's");
        i.call_link.set(i.call_link.get() | PLAIN_LINK);
        let row = script_call_row(ARGC, PLAIN, returns);
        sibling!(row, r, ip, Carried::script_in(cx, view))
    }
}


pub(crate) mod leaf_calls;
pub(crate) mod inlined_views;
pub(crate) mod planned_calls;
use leaf_calls::{complete_leaf_in_lane, lane_enter_leaf, AdmittedWindow, materialize_running_leaf_in_lane, leaf_call_handler};



/// The `script_call_prove_miss` argument count of a record its view did not resolve.
const ANY_ARGC: u16 = u16::MAX;









script_handler! {
    /// The out-of-line form of a call row, so the plain call keeps its registers: a receiver
    /// whose binding reads the heap (`call_this_in_lane`), a forwarding target, which takes
    /// `script_call_keeping_miss`, and a target whose body is a read (`CallEntry::frameless(Frameless::Reads)`,
    /// `CallEntry::frameless(Frameless::ReadsElement)`), whose hit (`call_read_in_lane`) completes the
    /// call with its value as the callee's return would; a call that returns its callee's
    /// completion completes its caller in turn (`lane_return_value!`). Anything else
    /// activates the callee. A site that called such a target calls this form directly, so it
    /// makes the row's own entry tests.
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
        if target.entry == CallEntry::framed(Framed::Frame) {
            sibling!(frame_call_in_lane, r, ip, Carried::script_in(cx, view))
        }
        if target.entry == CallEntry::framed(Framed::Environment) {
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
        // A frameless native target runs its entry with no activation (`native_call_in_lane`); where that declines,
        // the call takes the general form below, which activates it as its records.
        let value = if target.entry == CallEntry::frameless(Frameless::Native) {
            if returns && !returning_call_admitted(cx) {
                None
            } else {
                // SAFETY: the call's window of `r` holds its `argc` values.
                let passed = unsafe { r.0.add(usize::from(arguments)).cast_const() };
                native_call_in_lane(cx, target, passed, argc, this)
            }
        } else if body_calls::activation_form(target, argc).is_some()
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
            !target.entry.needs_frame_path() && !target.forwards()
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
    /// `script_call_full_miss` of an environment target (`CallEntry::framed(Framed::Environment)`), out of
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
        if target.entry != CallEntry::framed(Framed::Environment) {
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


script_handler! {
    /// `script_call_full_miss` of a non-suspending async target (`CallEntry::framed(Framed::Settles)`), out of
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
            quench_runtime::vm::iterators::IterationStep::Call => sibling!(script_call_prove_miss::<ANY_ARGC, false>, r, ip, Carried::script_in(cx, view)),
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



script_handler! {
    /// A JavaScript `Call` of another program's frameless callee (`LaneStack::cross_targets`):
    /// its activation runs in the callee's program, which its return leaves
    /// (`Completion::Cross`). A site whose callee is no such target calls `script_call_prove_miss`
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
            sibling!(script_call_prove_miss::<ANY_ARGC, false>, r, ip, Carried::script_in(cx, view))
        };
        // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
        let target = unsafe { &*target };
        // A callee that needs a frame takes it as `frame_call_in_lane` pushes one.
        if !target.entry.activates_across() && target.entry != CallEntry::framed(Framed::Environment) {
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
        // pushes as `script_call_prove_miss`'s does, entering the callee's program
        // (`enter_window_in_lane`'s cross completion); the site calls its fixed form from now on
        // (`script_call_cross_plain_miss`).
        let argc = ImmediateLayout::argument_count(i.imm());
        if target.entry == CallEntry::framed(Framed::Activates)
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
        let activated = if target.entry == CallEntry::framed(Framed::Environment) {
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
    /// activation (`CallEntry::framed(Framed::Activates)`) of `ARGC` arguments, which no completion of its
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
        debug_assert!(target.entry == CallEntry::framed(Framed::Activates) && quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(ARGC)));
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


script_handler! {
    /// `script_call_prove_miss` of a callee its target cache does not hold: resolving it
    /// (`Vm::resolve_script_call_target`) makes a call, so it runs out of line and the call
    /// continues at `script_call_prove_miss`, which then finds it, or at `native_call_miss`.
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
                sibling!(script_call_prove_miss::<ANY_ARGC, false>, r, ip, Carried::script_in(cx, view))
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
    /// activation is the one `script_call_prove_miss` makes, and a native runs as
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
        if vm.script_call_holds(link, r.get(i.c)) && matches!(vm.function_record(r.get(i.b)), Some(FunctionRecord { kind: FunctionKind::Native(Native::FunctionCall), .. })) {
            if unsafe { (*cx).tiers_run } { note_forwarded_in_lane(vm, i, r.get(i.b), r.get(i.c)); }
            let (receiver, args) = window.split_first().map_or((Value::UNDEFINED, &[][..]), |(first, rest)| (*first, rest));
            resume!(cx, function_call_activation_in_lane(cx, r, ip, i, r.get(i.c), receiver, args))
        }
        // The bound function the lane last called (`LaneStack::bound_call`), its words read once: with bound
        // arguments, their call; without, and with tiers off, its target's activation where the site linked it.
        if let Some((bound, target, this, list)) = vm.lane_stack.bound_call && bound == r.get(i.b) {
            if !list.is_undefined() { resume!(cx, bound_arguments_call_in_lane(cx, r, ip, i)); }
            if vm.script_call_holds(link, target) && !unsafe { (*cx).tiers_run } { resume!(cx, function_call_activation_in_lane(cx, r, ip, i, target, this, window)) }
        }
        let Some(quench_runtime::vm::function::Forwarded { callee, receiver, args, kind, through_call }) = vm.forwarded_call(r.get(i.b), r.get(i.c), window) else {
            resume!(cx, bound_arguments_call_in_lane(cx, r, ip, i));
        };
        if through_call && unsafe { (*cx).tiers_run } { note_forwarded_in_lane(vm, i, r.get(i.b), callee); }
        if let Some(FunctionKind::Native(native)) = kind {
            if i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 && !returning_call_admitted(cx) { return exit(ip); }
            let value = native_value_in_lane(cx, ip, i, native, callee, receiver, args);
            resume!(cx, native_complete_in_lane(cx, ip, i, value));
        }
        if matches!(kind, Some(FunctionKind::User(..))) { remember_bound_call_in_lane(vm, r.get(i.b)); }
        resume!(cx, function_call_activation_in_lane(cx, r, ip, i, callee, receiver, args))
    }
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





pub(crate) mod named_keys;
pub(crate) mod wasm_calls;
pub(crate) mod wasm_exports; use wasm_exports::LaneExportCall;
use wasm_calls::{known_call_handler, reference_call_handler};
use named_keys::named_key_element;
pub(crate) mod in_place_natives;
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
#[cfg(feature = "jit")]
use lane_roots::LaneStart;
use lane_roots::LaneRoot;
pub(crate) mod async_requests;
pub(crate) mod generator_windows; use generator_windows::{generator_returned_in_lane, resume_generator_step_in_lane}; 
pub(crate) mod raised_windows;
pub(crate) mod array_steps;
pub(crate) mod name_probe; use name_probe::probe_handler; 
#[cfg_attr(not(feature = "jit"), allow(unreachable_code, unused_variables))] pub(crate) mod vm_steps;
pub(crate) mod family_helpers;
pub(crate) mod value_steps;
pub(crate) use vm_steps::{ahead_general_step, body_general_step, body_string_concat, decide_comparison, lane_allocate_step, lane_body_step_packed, owner_step, vm_operation_step};
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
            Native::BigIntAsIntN | Native::BigIntAsUintN =>
                self.bigint_as_n_steady(first?, args.get(1).copied()?, native == Native::BigIntAsIntN),
            Native::BigInt => self.bigint_of_safe_integer(first?),
            _ => None,
        }
    }

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














script_handler! {
    /// `new` of a frameless target that `Vm::inline_construct_target` would push: the
    /// receiver it allocates, then the activation `script_call_prove_miss` makes, whose
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
            // a strict caller's proper tail call sends it to the activation below, which leaves for that loop.
            if returning_call_admitted(cx) {
                return frame_return(vm, ip, this);
            }
        }
        let activated = if target.entry == CallEntry::framed(Framed::Environment) {
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
    /// `script_call_prove_miss` of a target that keeps its passed values
    /// (`ScriptCallTarget::forwards`): the keeping activation (`call_keeping_activation_in_lane`).
    /// Out of line, so ordinary calls keep their code.
    #[inline(never)]
    fn script_call_keeping_miss<>(cx, r, ip, i, view) {
        match call_keeping_activation_in_lane(cx, r, ip, i) {
            Ok((registers, base)) => next!(cx, registers, base, Carried::script_in(cx, view)),
            Err(Some(next)) => next,
            // A site this form serves since `script_call_prove_miss` saw a keeping target calls
            // the full form once its callee keeps nothing.
            Err(None) => {
                requicken(ip, call_row::<{ protocol_family::SCRIPT_CALL | protocol_misses::script_call::FULL as u16 }>());
                sibling!(script_call_full_miss, r, ip, Carried::script_in(cx, view))
            }
        }
    }
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


use quench_runtime::bytecode::FORWARDED_APPLY_ARGUMENTS;

















script_handler! {
    /// The tier prologue a frameless activation of a function the tier may claim starts at
    /// (`LaneView::claim_for_tier`): the activation continues where the tier's native run
    /// completes it (`Vm::js_tier_activation_entry`), else at its first record; claiming never
    /// changes how a function is called.
    #[inline(never)]
    fn tier_prologue_in_lane<>(cx, r, ip, i, view) {
        // The staged row answers the same continuation (`EntryHost::tier`); this is its miss body.
        let resumed = tier_prologue_resumed(cx, r, view, i.imm(), usize::from(i.a));
        resume!(cx, resumed)
    }
}

script_handler! {
    /// The prologue record of a function whose lane body specialized (`LaneView::specialize_lane_body`):
    /// it runs the body on the activation's registers and continues where the body left, testing
    /// nothing the specialization proved (`body_inlined_activation_in_lane`).
    #[inline(never)]
    fn body_entry_in_lane<>(cx, r, ip, i, view) {
        // The staged row answers the same continuation (`EntryHost::body`); this is its miss body.
        let resumed = body_calls::enter_published_body(cx, ip, r, view, i.imm(), i.b != 0);
        resume!(cx, resumed)
    }
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

pub(crate) use crate::compiled::direct_callee::{direct_callee_in_lane, direct_callees_in_lane};







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









/// Where the lane continues after a run of `method`, `own`'s body, on the activation whose
/// registers are `r`: the activations of the native frames it left inside first
/// (`native_frames::advance_into_frames`), then the record its leave names in the innermost, or the
/// innermost's completion; `body_left_in_lane` adds the leave as feedback (`LaneMethod::feedback`).
pub(crate) enum LaneBodyLeft {
    /// Irreversible cancellation leaves the lane without dispatching another record.
    Terminated,
    Resume(Registers, Ip, Carried),
    /// The activation's body raised an exception (`Vm::lane_raised`) at the record: the
    /// lane unwinds from it, or the call that made the activation pops it (`body_calls::body_returned_in_lane`).
    Raised(Ip),
    /// The activation completes with the value, by the record.
    Completed(Ip, Value),
}




































/// Lane-stack room a frameless call's growth reserves at least: a common window.
const LANE_SCRIPT_WINDOW: usize = 4 * ACTIVATION_FILL_BLOCK;


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
    /// The return past the common one (`complete_past_op_in_lane`): another
    /// program's activation, which a program that calls across programs returns from as
    /// often as the common return; any other completion takes `return_other_in_lane`. Out of
    /// line, so the lane's return row keeps its registers.
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
    fn export_value(&mut self, value: Value, export: Self::Export) -> Value {
        unsafe { vm(self.cx) }.wasm_export_result(value, export)
    }
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
        if target.entry == CallEntry::frameless(Frameless::ReadsElement) {
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

/// A `CallEntry::frameless(Frameless::ForwardsRead)` body's reads: the field of `this` or a parameter it reads,
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
    /// `target` is a `CallEntry::frameless(Frameless::ForwardsRead)` target of the running program, whose view
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


use quench_runtime::vm::engine_types::ReadMemo;

/// Slots of the read memo table (`LaneStack::read_memos`): it bounds the table's memory, and a
/// table that fills starts again empty, so the sites that read since make their memos again.
const READ_MEMOS: usize = 4096;





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
                    // The row compares layout ids (`form_hint`), not the entry's exact shape.
                    set_field_hint(ip, vm.form_hint(vm.field_cache_entry(i.c), object));
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













/// A lane opcode's handler, and whether that handler is an accumulator
/// producer: whenever it falls through to the next record, it has written
/// its result register and carries the written value into that record.
#[derive(Clone, Copy)]
pub(crate) struct LaneHandler {
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





/// Dense fast handlers cover every opcode, in whole table rows: a generated row
/// (`admits_row`) runs from its table entry whatever its index.
pub(crate) const TABLE_SLOTS: usize = Op::COUNT.next_multiple_of(TABLE_ROW as usize);

/// Opcodes per generated table row.
pub(crate) const TABLE_ROW: u16 = 16;
pub(crate) const TABLE_ROWS: usize = TABLE_SLOTS / TABLE_ROW as usize;
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

pub(crate) struct LaneTable;

impl LaneTable {
    /// Tables indexed by jump direction (forward, then backward: a
    /// safepoint) and by accumulator field.
    pub(crate) const TABLES: [[Table; ACC_POSITIONS]; 2] = [
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


/// The records of a destructuring step after its iteration `Call` (`binding_step_in_lane`), by
/// offset from it: the `done` read, its move to the pattern's done flag, the threaded test
/// leaving the step, and the `value` read.
const BINDING_DONE_READ: usize = 2;
const BINDING_DONE_MOVE: usize = 3;
const BINDING_DONE_TEST: usize = 4;
const BINDING_VALUE_READ: usize = 5;









#[cfg(feature = "jit")]
use crate::compiled::tier_policy::{LANE_ENTRY_MIN_RUN, LANE_ENTRY_MIN_CALL_RUN};
/// Whether records may read the accumulator. Without sibling calls every
/// handler returns to the lane loop, which carries no accumulator.
const ACCUMULATOR: bool = cfg!(quench_lane_tail_calls);



/// Derive a function's lane view from its bytecode and its program's
/// constants.
#[cfg(feature = "jit")]
use crate::jit_routes::handler_in_lane;












pub(crate) mod int_steps;






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











/// An environment-window record's immediate: its own (a local slot or closure function, at
/// most `ENVIRONMENT_WINDOW_FIELD`) below, the function's register count above.
const ENVIRONMENT_WINDOW_SHIFT: u32 = u16::BITS;
const ENVIRONMENT_WINDOW_FIELD: u32 = (1 << ENVIRONMENT_WINDOW_SHIFT) - 1;



/// The `arguments` count row the view gives its kept-arguments fact (`row_near`).
const ARGUMENTS_COUNT: u16 = quench_runtime::staged::ARGUMENTS_COUNT_ROW;

/// The jump rows the jump passes fuse and requicken to (`jump_row`).
const JUMP: u16 = quench_runtime::staged::JUMP_ROW;

/// The capture rows the capture passes fuse (`specialize_captures`, `specialize_capture_updates`,
/// `specialize_load_branch_pairs`) and whose read a fused update falls back to.
const CAPTURE_READ: u16 = quench_runtime::staged::LOAD_CAPTURE_ROW;
const CAPTURE_WRITE: u16 = quench_runtime::staged::STORE_CAPTURE_ROW;



// merge: theirs' fused capture update (`specialize_capture_updates`, `lane_update_capture_int32`)
// is not ported: it fuses `lane_operator_int32` and `lane_store_capture_load`, which ours does not
// have. Its records run unfused, with the same semantics.












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
                crate::ahead_runner::view_in_lane(self, program, function, code, constants, kind, live, sites, image)
            })
            .unwrap_or(std::ptr::null())
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
    #[cfg(feature = "jit")]
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
        unsafe { pc_in_view(context.base, ip) }
    }

    /// Raises the stack floor for a lane run of `program`: below the native stack's slack, or
    /// at once where a collection is due or the run credits loop work, so its backward jumps
    /// count in `safepoint_at` (`LaneContext::backedges_left`). The floor lives in the VM's
    /// interrupt state, where a termination request raises it; entry sets it before reading
    /// the request, the request raises it after setting itself, so one of the two sees the
    /// other.
    pub(crate) fn enter_lane_floor(&mut self, code: &ResidualProgram, program: ProgramId) -> LaneFloor {
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
                let pc = unsafe { record_pc(cx, exit) } as usize;
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
                    let pc = unsafe { record_pc(cx, exit) } as usize;
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
                    let this = self.frame_lane_this(caller);
                    frame_registers.set_this(this);
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


pub(crate) use quench_runtime::vm::engine_types::PreparedLaneCallback;

use quench_runtime::vm::engine_types::PreparedShape;

#[quench_macros::ext(PreparedShapeOps1)]
impl PreparedShape {
    /// The shape of `target`'s activations from the native whose caller's context is
    /// `caller`, for calls passing `argc` values; `None` for a gathering target, whose rest
    /// array each call makes (`activate_keeping_in_lane`).
    #[inline(always)]
    fn of(caller: &LaneContext, call: Ip, target: &ScriptCallTarget, argc: usize) -> Option<Self> {
        if target.entry == CallEntry::framed(Framed::Gathers) || argc > usize::from(u16::MAX) {
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
                let pc = unsafe { record_pc(cx, exit) } as usize;
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

mod part_0;
#[allow(unused_imports)]
pub use part_0::*;
mod part_1;
#[allow(unused_imports)]
pub use part_1::*;
mod part_2;
#[allow(unused_imports)]
pub use part_2::*;
mod part_3;
#[allow(unused_imports)]
pub use part_3::*;
mod part_4;
#[allow(unused_imports)]
pub use part_4::*;
mod part_5;
#[allow(unused_imports)]
pub use part_5::*;
