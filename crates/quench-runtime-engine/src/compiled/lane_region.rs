//! The lane region specializer: Futamura's first projection of the interpreter onto one
//! JavaScript function, run on the function's frameless lane activations under Ershov's
//! online division (Linear TOD-1232).
//!
//! What the division makes static is folded, and each dynamic fact is proved once:
//!
//! - Every bytecode register is an SSA value in machine registers. The register file is
//!   read at entry and written back only where the region leaves.
//! - Shared semantic Type/Fact proofs select unboxed i32/f64 views. Read-only numeric
//!   inputs are proved once at region entry; definitions and CFG edges carry the proof.
//!   Tagged bits are rebuilt only where a tagged consumer or recovery needs them.
//! - Region-wide facts are proved at entry: the receiver's record, shape and storage, the
//!   shapes the receiver sites fold, the method-cache state stores need, and capture reads.
//! - Field sites fold their cache entries (shape and slot) and constants fold to immediates.
//!
//! The region leaves at the exact PC of any instruction it does not run, before any of
//! that instruction's effects, with the register file holding the interpreter's state; the
//! lane resumes there. Nothing in a run allocates, collects or calls guest code.
#[allow(unused_imports)]
use quench_runtime::staged::AllocationAllocationExt;
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationNamesExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::compiled::regions;
use super::*;
use quench_runtime::vm::specializer::Specializer;
use quench_runtime::typed_ir::{repr, BindingTime, Fact, Projection, Proof, Reentry, Representation, Type, VerifiedFact};
use super::object_slots::{emit, CheckedObject, CheckedObjectTable, Emit, ObjectHeader, ObjectRecord, ObjectTable};
use quench_runtime::typed_ir::Division;
use quench_runtime::staged::typed_elements::{NativeKind, ViewKind};
use quench_runtime::staged::typed_view::{prove_typed_view, ViewAccess};
use std::collections::{BTreeMap, BTreeSet};
/// The direct callees of a body's call sites, by unit PC.
/// The direct callees of each call site, by PC, in the order a chain of identity tests takes them
/// (`Lowering::polymorphic_call`); one for a site that is not polymorphic.
pub(crate) type DirectSites = BTreeMap<usize, Vec<*const DirectCallee>>;
use quench_runtime::dataflow::BitSet;
pub(crate) use quench_runtime::bytecode::instruction_leaders as leaders;
use cranelift_codegen::entity::EntityRef;

#[path = "lane_region/known_branches.rs"]
pub(crate) mod known_branches;
#[path = "lane_region/clif_census.rs"]
pub(crate) mod clif_census;
#[path = "lane_region/packed_ranges.rs"]
pub(crate) mod packed_ranges;
#[path = "lane_region/int_ranges.rs"]
pub(crate) mod int_ranges;
#[cfg(test)]
#[path = "lane_region/int_ranges_tests.rs"]
pub(crate) mod int_ranges_tests;
#[path = "lane_region/global_reads.rs"]
pub(crate) mod global_reads;
#[path = "lane_region/native_calls.rs"] pub(crate) mod native_calls;
#[path = "lane_region/builtin_calls.rs"] pub(crate) mod builtin_calls;
#[path = "lane_region/data_view_calls.rs"] pub(crate) mod data_view_calls;
pub(crate) use native_calls::{inlines_in_loop, inlines_in_loop_call, native_method_call, IN_PLACE_CALL, LOOKUPS};
#[path = "lane_region/stable.rs"]
pub(crate) mod stable;
#[path = "lane_region/polls.rs"]
pub(crate) mod polls;
mod receiver_records;
#[path = "lane_region/scalar.rs"] pub(crate) mod scalar;
#[path = "lane_region/trips.rs"] pub(crate) mod trips;
#[path = "lane_region/function_names.rs"] pub(crate) mod function_names;
#[path = "lane_region/literals.rs"] pub(crate) mod literals;
#[path = "lane_region/allocations.rs"] pub(crate) mod allocations;
#[path = "lane_region/edges.rs"] pub(crate) mod edges;
#[path = "lane_region/demand.rs"] pub(crate) mod demand;
#[path = "lane_region/arithmetic.rs"] pub(crate) mod arithmetic;
#[path = "lane_region/index_math.rs"] pub(crate) mod index_math;
#[path = "lane_region/int_inputs.rs"] pub(crate) mod int_inputs;
#[path = "lane_region/loop_versions.rs"] pub(crate) mod loop_versions;
#[path = "lane_region/exits.rs"] pub(crate) mod exits;
#[cfg(test)] #[path = "lane_region/exit_tests.rs"] pub(crate) mod exit_tests;
#[cfg(test)] #[path = "lane_region/home_tests.rs"] pub(crate) mod home_tests;
#[path = "lane_region/catches.rs"] pub(crate) mod catches;
#[path = "lane_region/store_domains.rs"] pub(crate) mod store_domains;
pub(crate) use store_domains::store_domains;
#[path = "lane_region/arguments.rs"] pub(crate) mod arguments;
#[path = "lane_region/captures.rs"] pub(crate) mod captures;
#[path = "lane_region/compare_jumps.rs"] pub(crate) mod compare_jumps;
#[path = "lane_region/relocs.rs"] pub(crate) mod relocs;
use relocs::RelocKind; pub(crate) use relocs::{atom_symbol, numeric_symbol};
use captures::RootedRead;
#[path = "lane_region/constructions.rs"] pub(crate) mod constructions;
#[path = "lane_region/dead_stores.rs"] pub(crate) mod dead_stores;
#[path = "lane_region/virtual_objects.rs"] pub(crate) mod virtual_objects;
#[path = "lane_region/dense_elements.rs"] pub(crate) mod dense_elements;
#[path = "lane_region/dense_appends.rs"] pub(crate) mod dense_appends;
#[path = "lane_region/bindings.rs"] pub(crate) mod bindings;
#[path = "lane_region/frame.rs"] pub(crate) mod frame;
#[path = "lane_region/frame_fields.rs"] pub(crate) mod frame_fields;
#[path = "lane_region/frame_elements.rs"] pub(crate) mod frame_elements;
#[path = "lane_region/frame_stores.rs"] pub(crate) mod frame_stores;
#[path = "lane_region/field_plans.rs"] pub(crate) mod field_plans;
#[path = "lane_region/headers.rs"] pub(crate) mod headers;
#[path = "lane_region/hoisted_reads.rs"] pub(crate) mod hoisted_reads;
#[path = "lane_region/loop_hoists.rs"] pub(crate) mod loop_hoists;
#[path = "lane_region/field_reads.rs"] pub(crate) mod field_reads;
#[path = "lane_region/own_constants.rs"] pub(crate) mod own_constants;
#[path = "lane_region/number_domains.rs"] pub(crate) mod number_domains;
#[path = "lane_region/loop_arrays.rs"] pub(crate) mod loop_arrays;
#[path = "lane_region/loop_strings.rs"] pub(crate) mod loop_strings;
#[path = "lane_region/call_frames.rs"] pub(crate) mod call_frames;
#[path = "lane_region/declared.rs"] pub(crate) mod declared;
use call_frames::{CallSite, DirectSite, Leave, ReturnSite, Site};
pub(crate) use call_frames::ProvenCall;
#[path = "lane_region/call_outs.rs"] pub(crate) mod call_outs;
#[path = "lane_region/default_rows.rs"] pub(crate) mod default_rows;
#[path = "lane_region/jumps.rs"] pub(crate) mod jumps;
#[path = "lane_region/call_rows.rs"] pub(crate) mod call_rows;
#[path = "lane_region/register_rows.rs"] pub(crate) mod register_rows;
#[path = "lane_region/window_locals.rs"] pub(crate) mod window_locals;
#[path = "lane_region/element_rows.rs"] pub(crate) mod element_rows;
#[path = "lane_region/tag_proofs.rs"] pub(crate) mod tag_proofs;
#[path = "lane_region/fixed_arrays.rs"] pub(crate) mod fixed_arrays;
#[cfg(test)] #[path = "lane_region/element_tests.rs"] pub(crate) mod element_tests;
#[cfg(test)] #[path = "lane_region/loop_shape_tests.rs"] pub(crate) mod loop_shape_tests;
use loop_arrays::LoopArray;
use loop_strings::{LoopString, StringVariables};
#[path = "lane_region/bound_this.rs"] pub(crate) mod bound_this;
#[path = "lane_region/lowering_values.rs"] mod lowering_values;
#[path = "lane_region/lowering_exits.rs"] mod lowering_exits;
#[path = "lane_region/lowering_conform.rs"] mod lowering_conform;
#[path = "lane_region/truncation.rs"] mod truncation;
#[path = "lane_region/return_cells.rs"] mod return_cells;
#[path = "lane_region/entry_facts.rs"] mod entry_facts;
#[path = "lane_region/field_stores.rs"] mod field_stores;
#[path = "lane_region/instruction.rs"] mod instruction;
#[path = "lane_region/type_tests.rs"] mod type_tests;
#[path = "lane_region/operators.rs"] mod operators;
#[path = "lane_region/calls.rs"] mod calls;
#[path = "lane_region/call_targets.rs"] mod call_targets;
#[path = "lane_region/loop_refinement.rs"] mod loop_refinement;
#[path = "lane_region/hoisting.rs"] mod hoisting;
#[path = "lane_region/field_access.rs"] mod field_access;
use frame::EmitFrame;
use frame_fields::{FieldFacts, HeldRead, StableHold};
use field_plans::{with_read_plan, LanePlans, LinkInput};
use quench_runtime::staged::property::{StoreArm, StoreJoin, StorePlan};
use polls::specialize_closed_polls;
#[cfg(test)]
use polls::retain_closed_polls;
pub(crate) use stable::StableFacts;
use quench_runtime::typed_ir::IntRange;
use quench_runtime::staged::machine::{Binary, Extend, Lane, MachineAccess, MachineFlow, MachineScalar, Relation, Unary};
use super::emit_machine::Builds;

/// The call context, step dispatch and result words a lane body or trampoline reads and writes.
pub(crate) const DISPATCH: quench_runtime::staged::Access = quench_runtime::staged::Region::Dispatch.mutable();
pub(crate) const LANE_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QLRG");
/// Inline scratch capacity for the small field-site sets typical of native units;
/// larger units spill normally, without a semantic or compilation limit.
const INLINE_FIELD_BINDINGS: usize = 4;
/// Common small effect frontiers stay inline; larger control-flow graphs spill normally.
const INLINE_EFFECT_EDGES: usize = 4;
/// Most blocks project only a few numbers; larger blocks spill without a limit.
const INLINE_NUMBER_PROJECTIONS: usize = 4;
/// A loop body usually indexes only a few arrays; more spill without a limit.
const INLINE_ARRAY_VIEWS: usize = 4;

#[path = "lane_region/register_state.rs"]
mod register_state;
pub(crate) use register_state::*;
#[path = "lane_region/lowering_types.rs"]
mod lowering_types;
pub(crate) use lowering_types::*;
#[path = "lane_region/numeric_gate.rs"]
mod numeric_gate;
pub(crate) use numeric_gate::*;
#[path = "lane_region/move_analysis.rs"]
mod move_analysis;
pub(crate) use move_analysis::*;
#[path = "lane_region/loop_invariants.rs"]
mod loop_invariants;
pub(crate) use loop_invariants::*;
#[path = "lane_region/region_plan.rs"]
mod region_plan;
pub(crate) use region_plan::*;
#[path = "lane_region/lower_entry.rs"]
mod lower_entry;
pub(crate) use lower_entry::*;
#[path = "lane_region/lower.rs"]
mod lower;
pub(crate) use lower::*;
/// A block reads the captures of a few inlined callees; more spill without a limit.
const INLINE_CAPTURE_ADMISSIONS: usize = 4;
/// A header and a short latch usually need only a few guarded inputs.
const INLINE_LOOP_INPUTS: usize = 4;
/// A jump's step proves a fact of one tested register, along one or both of its edges.
const INLINE_EDGE_REFINEMENTS: usize = 2;
/// The sums of int32 constants a truncated chain passes through (`Lowering::truncated_through_sums`):
/// from a product under 2^52, each adds under 2^31 and the doubles stay under 2^53.
const TRUNCATION_CHAIN: usize = 4;
/// Common native-entry failure status; a pure scalar miss restarts its call.
const NUMBER_ENTRY_MISS: i64 = 1;

pub(crate) use quench_runtime::vm::engine_types::ElementViews;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

#[quench_macros::ext(ElementViewsOps1)]
impl ElementViews {
    /// The views of arrays viewed both as `self` and as `other`: the same dense views, or typed
    /// views of the kind both name and of no kind where they differ; `None` for a mix.
    pub(crate) fn common(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Typed(left), Self::Typed(right)) => Some(Self::Typed(if left == right { left } else { None })),
            _ => (self == other).then_some(self),
        }
    }

    /// The kind a typed view's site observed.
    fn observed_kind(self) -> Option<quench_runtime::tier_surface::TypedArrayKind> {
        match self {
            Self::Typed(kind) => kind,
            // A double backing's words are a `Float64Array`'s.
            Self::Double => Some(quench_runtime::tier_surface::TypedArrayKind::Float64),
            Self::Dense => None,
        }
    }
}

#[cfg(test)]
#[path = "lane_region/tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "lane_region/lowering_tests.rs"]
pub(crate) mod lowering_tests;
#[path = "lane_region/kept_shapes.rs"] pub(crate) mod kept_shapes;
#[path = "lane_region/fixed_cells.rs"] pub(crate) mod fixed_cells;
#[path = "lane_region/loop_receivers.rs"] pub(crate) mod loop_receivers;
#[cfg(test)]
#[path = "lane_region/slot_proof_tests.rs"]
pub(crate) mod slot_proof_tests;

