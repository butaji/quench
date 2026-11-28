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

/// Per block leader that a later PC enters (a loop's header, or a rotated body a latch enters), its forward edges and
/// the last PC that enters it (`Lowering::carry_typed_views`). The entry `start` has edges no instruction names: none.
fn loop_carries(instructions: &[WideInstruction], leaders: &BitSet, start: usize) -> BTreeMap<usize, (usize, usize)> {
    let mut edges: BTreeMap<usize, (usize, Option<usize>)> = BTreeMap::new();
    let mut add = |pc: usize, target: usize| {
        if !leaders.contains(target) || target == start { return; }
        let entry = edges.entry(target).or_insert((0, None));
        if target > pc { entry.0 += 1 } else { entry.1 = Some(entry.1.map_or(pc, |last: usize| last.max(pc))) }
    };
    for (pc, instruction) in instructions.iter().enumerate() {
        quench_runtime::bytecode::for_each_instruction_successor(pc, *instruction, |target| add(pc, target));
    }
    // A latch whose loop's header is a test repeats it (`edges::rotated_bodies`): its edges are the test's, from the latch.
    for (latch, header) in quench_runtime::bytecode::back_edges_of(instructions.iter().copied()) {
        let Some(&test) = instructions.get(header) else { continue };
        if !quench_runtime::staged::meaning::means(test, quench_runtime::staged::meaning::Meaning::ConditionalJump) { continue; }
        quench_runtime::bytecode::for_each_instruction_successor(header, test, |target| add(latch, target));
    }
    edges.into_iter().filter_map(|(pc, (forward, last))| Some((pc, (forward, last?)))).collect()
}

/// A typed array view an access proved (`Proofs::typed`): the register holding the base, its native kind, the view's elements and
/// length, whether it was proved for stores (which also answers reads), and whether an instruction that may change a view
/// ran since (`kept`): a kept view is the same view once the code holds views (`Lowering::typed_view_of`).
#[derive(Clone, Copy, PartialEq)]
struct TypedProof { register: Register, kind: quench_runtime::tier_surface::TypedArrayKind, values: ir::Value, length: ir::Value, writable: bool, kept: bool }
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

/// Semantic proof and active machine view; tagged bits may also be current.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegisterFact {
    value: VerifiedFact,
    tagged: TaggedView,
}

/// A current tagged view is either an entry/join variable or a dominating SSA value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaggedView { Unavailable, Variable, Value(ir::Value) }

impl TaggedView {
    fn current(self) -> bool { self != Self::Unavailable }
}

impl RegisterFact {
    fn admit(value: Fact) -> Option<Self> {
        Some(Self { value: value.verified()?, tagged: if value.representation == Representation::Tagged {
            TaggedView::Variable
        } else { TaggedView::Unavailable } })
    }

    fn definition(ty: Type, representation: Representation) -> Self {
        Self::admit(Fact { ty, representation, binding: BindingTime::Dynamic,
            proof: Proof::Definition, projection: Projection::Exact })
            .expect("semantic operation's result type matches its representation")
    }

    /// A definition held in the narrowest representation its type takes (`repr`).
    fn narrowest(ty: Type) -> Self { Self::definition(ty, repr(ty)) }

    fn canonical() -> Self {
        Self::admit(Fact::tagged(BindingTime::Location, Proof::CanonicalValue))
            .expect("canonical tagged value")
    }
}

struct Registers {
    pub tagged: Vec<Variable>,
    /// Each register's int32, 64-bit integer and float64 views, declared at their first use: most
    /// registers never take one, and the SSA builder's tables grow with variables times blocks. A
    /// view read before any definition reads zero, as only a state that defined it reads it.
    pub int: Vec<Option<Variable>>,
    pub long: Vec<Option<Variable>>,
    pub float: Vec<Option<Variable>>,
}

/// The compile-time state of the instruction being lowered.
#[derive(Clone)]
struct State {
    values: Vec<RegisterFact>,
    proofs: LocalProofs,
    /// What the jump just stepped proved of a register along each of its edges
    /// (`Frame::refine`), applied to the successors' states (`State::narrow`) once it branches.
    refinements: smallvec::SmallVec<[(quench_runtime::staged::Edge, Register, Type); INLINE_EDGE_REFINEMENTS]>,
    /// Receivers kept as their field values (`virtual_objects`), each live in its register.
    virtuals: smallvec::SmallVec<[virtual_objects::VirtualObject; 1]>,
    /// Array literals kept as their element values (`virtual_objects::VirtualArray`), each live in its register.
    virtual_arrays: smallvec::SmallVec<[virtual_objects::VirtualArray; 1]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FieldValue {
    /// The resolved tagged word of the receiver the read named: a fact of the object, so every register holding it shares it.
    owner: ir::Value,
    atom: Atom,
    value: ir::Value,
    ty: Type,
}

/// Available object and value proofs saved at control-flow edges. Register types
/// belong to the shared dataflow analysis and need no duplicate snapshots.
#[derive(Clone, Default)]
struct LocalProofs {
    /// Direct-object proofs by the resolved tagged word they prove, until an effect that may change the object
    /// retires them. A register's redefinition names a new word, so it retires nothing. The flag is whether the
    /// proof was a read's (`readable`, which may name a function's object).
    headers: smallvec::SmallVec<[(ir::Value, ObjectHeader, bool); INLINE_FIELD_BINDINGS]>,
    /// The record each proved value names (`ObjectRecord`), by the tagged word (resolved), and whether
    /// the proof was a read's (`readable`, which may name a function's object): a fact of the value, which
    /// no effect changes (records stay put in a run, and the reservation's base is the run's), so it
    /// outlives the header's shape proof across calls and holds for every register that holds the word
    /// (a copy, an inlined frame's `this`). A later header of any of them reads its shape through it, with
    /// no family test and no record lookup: the untagged view of a proven object (R2, facts on values).
    records: smallvec::SmallVec<[(ir::Value, ObjectRecord, bool); INLINE_FIELD_BINDINGS]>,
    /// The complete own-slot projection of an object, by its resolved tagged word and exact shape: a fact of the
    /// object, which any register holding the word shares (`keep_layout`).
    layouts: BTreeMap<(ir::Value, u32), CheckedObject>,
    /// The projection of the realm's global object a held slot's access proved (`call_out::global_layout`),
    /// until an effect that may move the property arena retires it, as `layouts`.
    global: Option<(CheckedObject, usize)>,
    /// The exact shape of an object, by its resolved tagged word, that the object had when a proof
    /// (or definition) of it made the shape its type, and which a call keeps (`kept_shapes.rs`): the
    /// code holds it again at a use where the VM holds the shape stable.
    kept_shapes: BTreeMap<ir::Value, u32>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
    /// Immutable reads; receiver definitions and memory effects retire these facts.
    fields: smallvec::SmallVec<[FieldValue; INLINE_FIELD_BINDINGS]>,
    /// Pure ToInt32 results, separate from each source value's semantic type.
    int32_projections: smallvec::SmallVec<[(ir::Value, quench_runtime::staged::Int32<ir::Value>); INLINE_NUMBER_PROJECTIONS]>,
    /// Number views of tagged values a test proved (`frame_operands::register_number_value`): a
    /// second register defined as the same value (`p.x * p.x`) takes the double without a test.
    numbers: smallvec::SmallVec<[(ir::Value, quench_runtime::staged::Float64<ir::Value>); INLINE_NUMBER_PROJECTIONS]>,
    /// Dense array views by the tagged base they prove, until an instruction that could change an array's storage (`retains_array_views`).
    arrays: smallvec::SmallVec<[(ir::Value, super::DenseArrayView); INLINE_ARRAY_VIEWS]>,
    /// Typed array views by the tagged base and the observed kind they prove (`row_or_typed`'s typed-only row), retired
    /// as `arrays` are: a later access of the same base and kind takes the view without proving it again.
    typed: smallvec::SmallVec<[TypedProof; INLINE_ARRAY_VIEWS]>,
    /// The dense capture views (values, length) a frameless admission of an inlined callee gave, by the
    /// callee's value: its frame's reads take them, all dominated by the one admission (`Lowering::admitted_captures`).
    admitted_captures: smallvec::SmallVec<[(ir::Value, (ir::Value, ir::Value)); INLINE_CAPTURE_ADMISSIONS]>,
    /// Whether this block read the code's invalidation word since the last helper that may change a Stable fact's subject
    /// (run JavaScript, write a prototype, name a method): every Stable fact the code folded holds until such a helper (`Lowering::hold_stable`).
    stable: bool,
    /// Whether this block proved the realm its global reads were compiled against active (`Lowering::hold_global`).
    realm: bool,
    /// Whether this block proved the run's stores specialized (`JsStepDispatch::stores_specialized`, a
    /// constant of the run), so a store of an unnamed atom tests only the invalidation word (`Lowering::online_store_held`).
    specialized: bool,
    /// Registers whose lane-frame slot holds their current tagged value (`Lowering::home_written`): a
    /// definition retires one, so a call writes back only what changed since the last write-back.
    homed: BitSet,
    /// Resolved words of holders this unit allocated (object or array literals) with no safepoint since (4.7 (b)):
    /// a store into one needs no barrier log. A safepoint is an allocation, a call, or a step that may allocate or run guest
    /// code (`Lowering::retire_fresh`); a join keeps the holders every arrival holds (`keep_shared_proofs`, `join_states`).
    fresh: smallvec::SmallVec<[ir::Value; INLINE_FRESH_HOLDERS]>,
}

/// A block usually holds a few fresh holders at once (a constructor's receiver, a literal's temporaries).
const INLINE_FRESH_HOLDERS: usize = 4;

impl LocalProofs {
    /// Keeps the typed views past an instruction that may change one: the next access takes a kept view only
    /// where the code holds views (`Lowering::typed_view_of`), else proves it again.
    pub(crate) fn keep_typed_views(&mut self) {
        for view in &mut self.typed { view.kept = true; }
    }

    /// Notes that tagged `bits` read as the double `number`: the most recent views win a full
    /// list (a loop body that proves many Numbers keeps the ones its next reads name).
    pub(crate) fn note_number(&mut self, bits: ir::Value, number: quench_runtime::staged::Float64<ir::Value>) {
        if self.numbers.len() >= 4 * INLINE_NUMBER_PROJECTIONS { self.numbers.remove(0); }
        self.numbers.push((bits, number));
    }

    /// Notes the dense capture view `callee`'s admission gave; the oldest view goes once the list is full.
    pub(crate) fn note_admitted_captures(&mut self, callee: ir::Value, view: (ir::Value, ir::Value)) {
        if self.admitted_captures.len() >= INLINE_CAPTURE_ADMISSIONS { self.admitted_captures.remove(0); }
        self.admitted_captures.push((callee, view));
    }

    /// An admitted existing own-slot write changes only this name. Retire it for every receiver, since distinct receiver values may alias.
    fn write_field(&mut self, atom: Atom) {
        self.fields.retain(|field| field.atom != atom);
    }

    /// Keeps the homes `other` also holds, for a state both arrivals reach.
    fn meet_homed(&mut self, other: &BitSet) {
        let dropped: Vec<usize> = self.homed.iter().filter(|register| !other.contains(*register)).collect();
        for register in dropped { self.homed.remove(register); }
    }
}

impl State {
    /// Takes back the tagged views `before` held where one path made its own (`TaggedView::Value`, a word
    /// it boxed or read): that word is defined in the path's blocks, which a sibling path or the join past
    /// both does not see. The next read derives the view again where it is made (`frame::register_tagged`).
    fn drop_path_views(&mut self, before: &State) {
        for (value, before) in self.values.iter_mut().zip(&before.values) {
            if matches!(value.tagged, TaggedView::Value(_)) && value.tagged != before.tagged {
                value.tagged = before.tagged;
            }
        }
    }

    fn canonical(registers: usize) -> Self {
        Self { values: vec![RegisterFact::canonical(); registers], refinements: smallvec::SmallVec::new(), virtuals: smallvec::SmallVec::new(),
            virtual_arrays: smallvec::SmallVec::new(), proofs: LocalProofs {
            headers: smallvec::SmallVec::new(), records: smallvec::SmallVec::new(),
            layouts: BTreeMap::new(), global: None, kept_shapes: BTreeMap::new(), receivers: BTreeSet::new(),
            fields: smallvec::SmallVec::new(), int32_projections: smallvec::SmallVec::new(), numbers: smallvec::SmallVec::new(), stable: false, realm: false, specialized: false,
            arrays: smallvec::SmallVec::new(), typed: smallvec::SmallVec::new(), admitted_captures: smallvec::SmallVec::new(), homed: BitSet::new(),
            fresh: smallvec::SmallVec::new() } }
    }

    /// The facts a jump proved along `edge` (`refinements`), each narrowing its register's type for
    /// the edge's successor.
    fn narrow(&mut self, edge: quench_runtime::staged::Edge, refinements: &[(quench_runtime::staged::Edge, Register, Type)]) {
        for &(_, register, ty) in refinements.iter().filter(|(on, ..)| *on == edge) {
            let fact = self.values[usize::from(register)];
            if fact.value.representation == Representation::Tagged && ty != fact.value.ty && ty.is_subtype(fact.value.ty) {
                self.values[usize::from(register)] = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(ty, Representation::Tagged) };
                // The word this refinement names, where the register holds one without a builder (`kept_shapes`).
                if let (Some(shape), TaggedView::Value(word)) = (ty.object_shape(), fact.tagged) {
                    self.proofs.kept_shapes.insert(word, shape);
                }
            }
        }
    }

    /// Native reentry retires shape/storage and function-body admission proofs;
    /// object records and language families stay.
    fn call_effect(&mut self) {
        self.proofs.fresh.clear();
        self.proofs.fields.clear();
        self.proofs.arrays.clear();
        self.proofs.keep_typed_views();
        // Guest code may move a callee's captures or start another admission epoch: the next read probes again.
        self.proofs.admitted_captures.clear();
        self.proofs.headers.clear();
        self.proofs.layouts.clear();
        self.proofs.global = None;
        self.proofs.stable = false;
        for fact in &mut self.values {
            let unshaped = fact.value.ty.without_object_shape();
            if unshaped != fact.value.ty {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(unshaped, fact.value.representation) };
            } else if fact.value.ty.body_index().is_some() {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(Type::language(quench_runtime::ValueType::Function), fact.value.representation) };
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiverRecovery { Object, ObjectOrNullish }

impl ReceiverRecovery {
    fn for_body(body: &quench_runtime::bytecode::Function) -> Self {
        if body.instructions().all(|instruction| {
            !instruction.writes_current_this()
                && !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::LoadThis { .. }))
                && !instruction.stores_to_this()
                && (!matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) || matches!(instruction.field_lookup(),
                    Some(FieldLookup::Atom { base, .. }) if base != quench_runtime::bytecode::FieldBase::THIS))
        }) { Self::ObjectOrNullish } else { Self::Object }
    }
}

/// Region-wide facts proved at entry.
struct Entry {
    receiver: LaneReceiver,
    /// This heap's non-null record reservation, fixed for the native run.
    object_table: Option<CheckedObjectTable>,
    /// Entry receiver certificates: record locators of `this` (the final entry) and of the
    /// caller registers no unit instruction writes. Records stay put in a run, which
    /// collects nothing; shapes are derived per use.
    records: Vec<Option<ObjectRecord>>,
    /// Per folded receiver shape, whether the receiver has it.
    receiver_shapes: BTreeMap<u32, Variable>,
    /// Per stored atom, whether the run admits direct stores of it.
    store_atoms: BTreeMap<Atom, Variable>,
    /// The running activation's dense capture slots (`EnvironmentSlots::native_dense_view`).
    capture_values: Variable,
    capture_len: Variable,
    /// Whether those slots admit every direct store (the captures word's `CAPTURE_STORES_BIT`).
    capture_stores: Variable,
    /// The word of the storage the activation's environment-owned locals live in (`JsStepDispatch::environment_storage`),
    /// read once at entry and again after an operation that makes an environment (`Lowering::reload_environment`).
    environment: Variable,
    this: Variable,
    /// Flat per-site immutable slot inputs; no per-operation probe.
    fields: smallvec::SmallVec<[(u16, FieldSlotBinding); INLINE_FIELD_BINDINGS]>,
    /// Function-bound numeric calls whose invariant closure was admitted here.
    calls: BitSet,
    /// Inlined frames whose invariant receiver needs no boxing on recovery.
    call_receivers: BitSet,
}

/// A lane body lowered for the code generator, which may run it on another thread.
#[derive(Default)]
pub(crate) struct LoweredLane {
    pub(crate) region: LoweredRegion,
    pub(crate) clif: Option<String>,
    /// The function's scalar entry (`scalar`), which its direct calls of itself call: compiled beside the body.
    pub(crate) scalar: Option<ir::Function>,
    /// The body's proven entry (`ProvenEntry`): the same body past its parameter guards, and the entry types those guards
    /// test (`LoweredRegion::entry_types`). Its code must be the body's dependencies (`LoweredRegion::stable`).
    pub(crate) proven: Option<(ir::Function, Box<[Type]>)>,
}

/// One typed numeric return edge, materialized after joining all return facts.
struct NumberReturn {
    block: ir::Block,
    fact: VerifiedFact,
}

#[derive(Clone, Copy)]
struct LoopRead {
    value: Variable,
    ty: Type,
    representation: Representation,
    /// The read's actual entry proof, valid while the qualified loop cannot
    /// replace this receiver or change its shape. Never inferred from feedback.
    receiver: Option<(Register, Type)>,
    header: Option<ObjectHeader>,
    /// The word an induction value (`representation` `Int64`) advances by at each step of its counter.
    step: Option<Variable>,
}

#[derive(Clone, Copy)]
struct InvariantRead {
    pc: usize,
    /// The canonical caller home supplying a field receiver at loop entry.
    owner: Option<Register>,
    /// Whether a root-held binding it reads stays put: its loop stores no global object field.
    rooted: bool,
    /// The caller register a function-bound inlined capture read takes its closure from, where the
    /// loop neither writes it nor reads the callee again: the closure it holds at entry is the one
    /// every iteration reads from, so the read is hoisted to the entry edge (`entry_callee`).
    callee: Option<Register>,
}

/// A hoisted view: its base, its elements' address and count, and in a typed region the
/// array's `TypedArrayKind` discriminant.
#[derive(Clone, Copy)]
struct ArrayVariables {
    bits: Variable,
    values: Variable,
    /// The element count: a dense backing's `u32` as `i32`, a typed array's as `i64`.
    length: Variable,
    kind: Variable,
    /// A dense array's room for appends (`dense_appends`) as `i32`: zero until a reserve proves it.
    room: Variable,
}

/// A STORE-site projection admitted on entry to a storage-preserving loop.
/// The record is physical receiver identity, not merely an equal shape.
#[derive(Clone, Copy)]
struct LoopStore {
    header: ObjectHeader,
    address: ir::Value,
    packed_range: Option<IntRange>,
    /// The domain the entry admitted for the store (`Type::NUMBER` where the slot keeps raw
    /// doubles, which the carried Number stores as is; `Type::INT32` otherwise).
    domain: Type,
}

pub(crate) struct LoweredRegion {
    pub number_result: Option<NativeNumberResult>,
    pub function: ir::Function,
    pub native: usize,
    pub fault_poll: bool,
    /// Whether a loop of the region hoists an array view, so a typed variant may differ.
    pub hoists_arrays: bool,
    pub stable: StableFacts,
    /// The entry types of the function's parameters, as a proven entry's lowering holds them (`VmFacts::proven_entry`):
    /// the types its entry guards test, which a caller must prove each argument has. Empty elsewhere.
    pub entry_types: Box<[Type]>,
    /// The body's type summary (`TypeSummary`), where it is a whole function's body.
    pub summary: Option<TypeSummary>,
    /// The leaves the code takes, with the values each stores (`ExitTable::of_unit`, the emitter's own store sets).
    pub exits: ExitTable,
}

pub(crate) use quench_runtime::vm::engine_types::TypeSummary;

impl Default for LoweredRegion {
    /// A region that lowered nothing (`LoweredLane`'s default): an empty function.
    fn default() -> Self {
        Self { number_result: None, function: ir::Function::new(), native: 0, fault_poll: false, hoists_arrays: false, stable: StableFacts::default(), summary: None, entry_types: Box::default(), exits: ExitTable::default() }
    }
}

/// A guard's failure block and the state it leaves from: the registers' facts, and which frame slots
/// already held their value (`LocalProofs::homed`), at the guard.
struct GuardExit {
    block: ir::Block,
    values: Vec<RegisterFact>,
    homed: BitSet,
    virtuals: smallvec::SmallVec<[virtual_objects::VirtualObject; 1]>,
    virtual_arrays: smallvec::SmallVec<[virtual_objects::VirtualArray; 1]>,
    pc: u32,
    line: u32,
    /// The fact the guard tests (`guard_exit_for`); `Unclassified` where it names none.
    fact: ExitFact,
}

/// What a step's frame needs beyond its edges: the plans and held read of a named field access,
/// what a slow case needs of the lowering (`sites`: a call-out's record, a capture access's reach,
/// an `instanceof`'s prototype link).
#[derive(Default)]
struct StepWith<'a> {
    access: Option<(&'a LanePlans<'a>, HeldRead)>,
    sites: frame::Sites<'a>,
    /// The call or return a step of the call family runs for (`call_frames`).
    site: Option<call_frames::Site<'a>>,
}

pub(crate) struct Lowering<'a, 'f> {
    builder: FunctionBuilder<'f>,
    program: &'a ResidualProgram,
    unit: &'a super::lane_unit::Unit,
    /// The alias region each (class, atom) named property is keyed by (`declare_fields`); empty
    /// unless every property access of the unit is keyed.
    field_regions: BTreeMap<regions::FieldKey, cranelift_codegen::ir::AliasRegion>,
    /// The function's parameter count: an entry guard on a parameter declines the region
    /// when a call's value does not match the compiled entry fact.
    params: usize,
    /// The lowered function, which an exception it raises names as its source (`Lowering::throw`).
    function_id: u32,
    /// The entry function's annotated locals: the register each lives in and the language type its
    /// annotation seeds (`declared::declared_local_types`).
    declared_definitions: Vec<(Register, Type)>,
    /// The entry function's declared `this` type, which its `LoadThis` checks where it enters the destination.
    declared_this: Option<Type>,
    /// The entry function's unwritten rest parameter register and its declared element type (`declared::declared_rest_read`).
    declared_rest: Option<(Register, Type)>,
    /// The entry function's unwritten non-rest parameter registers, each with its parameter index
    /// (`declared::declared_member_reads`), whose named reads the declared member types check.
    declared_members: Vec<(Register, u16)>,
    /// The entry function's signature key (its source span start), which `declared_members` are typed by.
    declared_member_start: Option<u32>,
    helpers: LaneHelpers,
    /// The proved shapes and slots of each field cache site the unit reads or writes.
    fields: &'a FieldSites,
    /// The typed array kinds the body's element records took (`body_views::typed_reads`): the kind's view each access proves.
    kinds: &'a [super::body_views::ObservedKind],
    /// The by-name loads that read the global scope (`Vm::global_reads_in_lane`).
    global_reads: &'a quench_runtime::vm::global_facts::GlobalReads,
    /// The VM's facts: its `length` name, whose read of a direct array is its record's
    /// (`array_length`), and whether an IsHTMLDDA object can exist (`emit_nullish_branch`).
    vm: VmFacts<'a>,
    /// The call plans of the lowered function and of its inlined callees (`bindings::CallPlans`): each call
    /// site's arms and miss decide its admission.
    plans: bindings::CallPlans<'a>,
    /// The Stable facts the code folded, each a code dependency of its publication.
    stable: stable::StableUses,
    /// The unit PCs a previous body left at (`Vm::compile_lane_method`): a construction there
    /// runs by the call protocol (`Lowering::construct`).
    generic: &'a [usize],
    /// Whether the entry is proving a lane body's inputs: its guards share `entry_exit`.
    proving_entry: bool,
    /// Whether a previous body left at the instruction being lowered (`generic`): its reads probe the VM's caches.
    left_here: bool,
    /// The view of the loop's hoisted dense array the next element row reads or writes through, which
    /// its entering edge proved once (`EmitFrame::held`).
    held_view: Option<frame_elements::Held>,
    /// Whether the held view's read leaves at a hole (`EmitFrame::hole_leaves`).
    hole_leaves: bool,
    /// The shared leave of a failed input proof (`LaneExit::Entry`), made at its first guard
    /// and filled with the other exits.
    entry_exit: Option<ir::Block>,
    direct: &'a DirectSites,
    /// Per inlined frame, its callee's dense capture slots (values and count), fixed by the
    /// callee's identity, which its call guards; null where they are not dense.
    frame_captures: &'a [(usize, usize)],
    context: ir::Value,
    /// An entry-bound poll page, or the ordinary atomic termination flag.
    interrupt: ir::Value,
    fault_poll: bool,
    /// A loop region's completed iterations, flushed to its entry record where it leaves (`trips.rs`).
    trips: Option<Variable>,
    /// Callback completion edges can retire the native entry's interrupt ownership proof.
    reentries: smallvec::SmallVec<[ir::Block; INLINE_EFFECT_EDGES]>,
    polls: smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>,
    /// Already-conforming backedges meet before their common fault poll.
    poll_latches: super::SecondaryMap<ir::Block, Option<ir::Block>>,
    shared_headers: BitSet,
    payload: ir::Value,
    registers_base: ir::Value,
    /// The running body's receiver and capture words its caller passes (`entry_cell::capture_word`).
    entry_words: (ir::Value, ir::Value),
    callee_slots: Vec<(u16, ir::Value)>,
    registers: Registers,
    written: BitSet,
    /// Source operations already consumed by an exact, unobservable scalar pair.
    consumed: BitSet,
    blocks: Vec<Option<ir::Block>>,
    /// Per resume PC, its leave blocks, one per set of registers stored (`exits.rs`).
    exits: Vec<Vec<exits::ExitBlock>>,
    /// The leave blocks shared by every caller-frame PC that stores the same registers, which
    /// take the exit word as their parameter (`Lowering::shared_exit_block`).
    shared_exits: Vec<exits::ExitBlock>,
    /// Each block leader's register use demand (`demand::analyze`).
    demands: demand::Demands,
    entry: Option<Entry>,
    native: usize,
    /// Whether this entry's caller loop reaches its native latch.
    backedge: bool,
    /// Guard failure blocks to fill once the region is lowered (`guard_exit`).
    // Recovery boxes register values; object/layout proofs authorize forward execution
    // only and must not be retained in every guard's snapshot. With the lowering line that
    // made each guard, which a diagnostics build records.
    pending: Vec<GuardExit>,
    /// Whether the unit makes native calls (`Lowering::call`): a callee may change `this`,
    /// so no read or store folds the receiver facts its entry proved.
    native_calls: bool,
    /// Entry-bound guest depth for a region whose helpers cannot reenter.
    call_budget: Option<ir::Value>,
    /// The registers' types at each block leader (`facts::analyze`).
    facts: super::facts::Facts,
    /// The unit's register liveness by unit PC (`Unit::register_masks`), where the unit
    /// fits one register set; otherwise the caller's by its PCs and per inlined frame its
    /// callee's, projected per query (`Lowering::is_live`). A function without masks keeps
    /// every register live.
    live: Liveness<'a>,
    valid: bool,
    coverage: Coverage,
    completion: Completion,
    /// One return stays inline; additional returns spill without a region limit.
    number_returns: smallvec::SmallVec<[NumberReturn; 1]>,
    /// Per loop header, the reads its loop repeats unchanged (`invariant_reads`), made once
    /// on the loop's one entry edge; each read's PC names the value it made there and
    /// whether its int tag was tested.
    hoists: BTreeMap<usize, Vec<InvariantRead>>,
    /// The unit's block leaders (`leaders`), made once for the first loop whose reads hoist.
    unit_leaders: std::cell::OnceCell<std::rc::Rc<BitSet>>,
    /// The calls that run inline as Math operations (`native_calls::intrinsic_calls`).
    intrinsics: native_calls::Intrinsics,
    hoisted: BTreeMap<usize, LoopRead>,
    /// The scalar native calls a loop's entry computed once, by PC (`native_calls::hoist_scalar_calls`).
    scalar_values: BTreeMap<usize, native_calls::HoistedScalar>,
    /// Per dense array a loop searches, whether its prototypes hold no index, proved on the loop's entry
    /// (`hoist_arrays`) and unset by an invalidation (`forget_append_rooms`).
    prototype_clear: BTreeMap<Register, Variable>,
    /// The hoisted dense arrays, by loop header and source, whose entering edge guarded every element
    /// an int32 (`int_only_array`, `hoist_arrays`): their reads take the element's word as it is.
    int_only: BTreeSet<(usize, Register)>,
    /// The loops whose entering edge tests the construct plan of each callee their kept
    /// constructions build by, once for the loop (`virtual_objects::hoist_constructions`).
    construct_ready: Vec<virtual_objects::ConstructReady>,
    /// Loop-invariant array bases whose views their loop's entry proves (`loop_arrays`).
    loop_arrays: Vec<LoopArray>,
    /// Loop-invariant strings whose flat facts their loop's entry proves (`loop_strings`).
    loop_strings: Vec<LoopString>,
    /// The variables each held string's facts live in, by register (`loop_strings`).
    string_vars: BTreeMap<Register, StringVariables>,
    /// The unit a string element read left for the comparison at the next PC, which compares it
    /// as a unit and never makes the one-character string (`Lowering::unit_compare`).
    pending_unit: Option<PendingUnit>,
    /// The shapes loops proved of their invariant receivers on their entering edges (`loop_receivers`).
    loop_shapes: Vec<loop_receivers::LoopShape>,
    /// Whether no operation of the unit may move an array's backing or the array record table
    /// (`arrays_stay_put`), so the record table's base is read-only throughout.
    arrays_fixed: bool,
    /// Each hoisted base's tagged value and view, by register, across its loop's iterations.
    array_vars: BTreeMap<Register, ArrayVariables>,
    views: ElementViews,
    /// Successful write-through stores defining a loop-header field's SSA variable.
    loop_fields: Vec<LoopField>,
    loop_stores: BTreeMap<usize, LoopStore>,
    /// The unit's integer proofs, computed once before lowering (`Lowering::prove_ranges`): the
    /// arithmetic that cannot overflow and the hoisted element accesses within their array.
    ranges: int_ranges::IntRanges,
    /// The PC being lowered, whose edges to a header enter its loop or close it.
    at: usize,
    /// Per block leader, its forward edges (`forward_edges`), and the states the lowered
    /// ones arrived with: a block they all reach keeps the local proofs they share.
    forward: Vec<Option<usize>>,
    arrived: BTreeMap<usize, Vec<LocalProofs>>,
    /// Per forward-only join, the kept receivers its arrivals carried (`virtual_objects::JoinedVirtuals`).
    joined_virtuals: BTreeMap<usize, virtual_objects::JoinedVirtuals>,
    /// Per block leader a later PC also enters (a loop's header, a rotated body), its forward edge count and its last
    /// entering PC, and the typed views its forward edges arrived with (`carry_typed_views`).
    loop_carry: BTreeMap<usize, (usize, usize)>,
    carried_typed: BTreeMap<usize, Vec<smallvec::SmallVec<[TypedProof; INLINE_ARRAY_VIEWS]>>>,
    /// The block leaders an entry or a lowered edge enters: a block none does is not
    /// lowered, its code reached only past a leave.
    entered: BitSet,
    /// The block leaders sealed when lowered, which only edges lowered before enter
    /// (`edges::leader_edges`): a variable read in one resolves without a parameter.
    sealable: BitSet,
    /// Whether a run can do anything but leave: complete natively or run a loop natively. A
    /// body that can do neither declines (`lower`).
    finishes: bool,
    /// The one epilogue every stop shares: it returns its status parameter.
    leave: ir::Block,
    /// The stop at a record: it writes its result word parameter and leaves stopped.
    stop: ir::Block,
    /// The block the call-outs' statuses of a leave in a callee, or a termination, reach with the
    /// status, made at the first and filled at the region's end (`call_outs`).
    call_out_stop: Option<ir::Block>,
    /// The throws native code catches itself, by unit PC (`catches::catch_edges`).
    catches: catches::Catches,
    /// The same throws as the analysis's exception edges (`facts::Raise`), ascending by PC.
    raises: Vec<super::facts::Raise>,
}

enum Liveness<'a> {
    Unit(Option<std::borrow::Cow<'a, quench_runtime::compile::liveness::RegisterMasks>>),
    Frames {
        caller: Option<&'a quench_runtime::compile::liveness::RegisterMasks>,
        frames: Vec<Option<&'a quench_runtime::compile::liveness::RegisterMasks>>,
    },
}

/// How much of the unit must run natively: a prefix (a lane body, which leaves to the lane
/// anywhere), or all of it (a numeric region).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Coverage {
    Prefix,
    Full,
    /// One loop of the function's unit, from its header through its backward jump. Only
    /// instructions in the loop lower; an edge out of it leaves at the exact PC. The
    /// backedge is a native branch to the header with the tier's poll.
    Loop { header: usize, backedge: usize, poll: bool },
    /// A caller-origin entry into the original unit CFG. Opaque operations
    /// leave at their exact origin; earlier loops and effects are not replayed.
    PrefixFrom(usize),
}

impl Coverage {
    pub(crate) fn entry(self) -> usize {
        match self { Self::Loop { header, .. } => header, Self::PrefixFrom(pc) => pc, _ => 0 }
    }
    fn full(self) -> bool { self == Self::Full }
    fn selected_entry(self) -> bool {
        matches!(self, Self::PrefixFrom(_) | Self::Loop { .. })
    }
    /// The PC after the region this coverage selects: where a path that falls off its end
    /// leaves. A loop leaves after its backward jump.
    fn exit_pc(self, unit_len: usize) -> usize {
        match self {
            Self::Loop { backedge, .. } => backedge.saturating_add(1).min(unit_len),
            _ => unit_len,
        }
    }

    /// Whether instruction `pc` is part of the region this coverage selects.
    fn lowered(self, pc: usize) -> bool {
        match self {
            Self::Loop { header, backedge, .. } => (header..=backedge).contains(&pc),
            _ => true,
        }
    }
}

/// How a run ends. Every kind runs the lane body ABI (`body_signature_in_lane`): it takes its
/// inputs in the register window and returns its status and result word in registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Completion {
    /// A status and a PC (`LaneExit`): a lane body, or a loop region.
    Body,
    /// Pure, restartable scalar entry: borrowed inputs in the window's first slots, proved
    /// Numbers once at entry, and one Number result word; a miss restarts the original call.
    Number,
    /// A numeric function's entry that takes its numbers in registers (`scalar`): the same
    /// results and misses as `Number`, for a call whose arguments are proved those numbers.
    Scalar(scalar::ScalarAbi),
}

impl Completion {
    /// Whether a run completes with a status and a PC (a lane body or a loop region).
    pub(crate) fn status_and_pc(self) -> bool { self == Self::Body }
}

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

pub(crate) fn restartable_numeric_function(program: &ResidualProgram, function: &quench_runtime::bytecode::Function,
    instructions: impl IntoIterator<Item = WideInstruction>) -> bool {
    crate::js_compiled::function_candidate(function, program) && numeric_instructions(program, instructions)
}

fn numeric_instructions(program: &ResidualProgram, instructions: impl IntoIterator<Item = WideInstruction>) -> bool {
    instructions.into_iter().all(|instruction| numeric_instruction(program, instruction))
}

fn numeric_instruction(program: &ResidualProgram, instruction: WideInstruction) -> bool {
    if instruction.writes_current_this()
        || (instruction.result_flags() != 0 && !instruction.returns_from_frame()) { return false; }
    match () {
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => program.constants.get(instruction.constant_index())
            .and_then(super::facts::immediate).is_some(),
        _ if instruction.op().traits().has(quench_runtime::bytecode::Traits::SCALAR_REGION) => true,
        _ => quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary)
            && instruction.numeric_local_target().is_none(),
    }
}

fn register_offset(register: Register) -> i32 {
    i32::from(register) * std::mem::size_of::<u64>() as i32
}

pub(crate) fn int_tagged(builder: &mut FunctionBuilder<'_>, int: ir::Value) -> ir::Value {
    let mut stage = emit(builder);
    let payload = stage.raw_unary(Unary::ZeroExtend(Lane::I64), int);
    quench_runtime::staged::value::int_word(&mut stage, payload)
}

pub(crate) fn is_int_tagged(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    quench_runtime::staged::value::is_int(&mut emit(builder), bits)
}

/// The tagged Boolean a predicate `holds` selects: the boxed `true` where it holds, else the boxed `false`.
pub(crate) fn boolean_word(builder: &mut FunctionBuilder<'_>, holds: ir::Value) -> ir::Value {
    let yes = builder.ins().iconst(types::I64, Value::TRUE.to_bits() as i64);
    let no = builder.ins().iconst(types::I64, Value::FALSE.to_bits() as i64);
    builder.ins().select(holds, yes, no)
}

/// A block marked cold before any code is lowered into it, for the exits and slow paths the hot path leaves to.
pub(crate) fn cold_block(builder: &mut FunctionBuilder<'_>) -> ir::Block {
    let block = builder.create_block();
    builder.set_cold_block(block);
    block
}

/// The registers an instruction writes, by its operand layouts.
pub(crate) fn writes(instruction: WideInstruction) -> super::facts::Writes {
    instruction.written_registers().collect()
}

impl<'f> Builds<'f> for Lowering<'_, 'f> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'f> {
        &mut self.builder
    }
}

/// A string element read whose unit its next instruction compares with a one-unit constant
/// (`Lowering::unit_compare`): the register the read would define, the unit it read, and the
/// constant unit and sense of the comparison.
#[derive(Clone, Copy)]
struct PendingUnit {
    pc: usize,
    register: Register,
    unit: ir::Value,
    constant: u16,
    negated: bool,
}

/// The register whose value `register` copies at the comparison at `pc`: following the moves and
/// nops back to `header` within one block, so copies share the comparison's SSA value.
fn move_root(instructions: &[WideInstruction], leaders: &BitSet, (header, pc): (usize, usize), mut register: Register) -> Register {
    for at in (header..pc).rev() {
        if at + 1 < pc && leaders.contains(at + 1) { break; }
        let instruction = instructions[at];
        match () {
            _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_flags() == 0 => {
                if instruction.register_a() == register { register = instruction.register_b(); }
            }
            _ if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Nop)) => {},
            _ => break,
        }
    }
    register
}

/// Whether `instruction` can change no heap value natively: anything else it would do (a
/// non-Number operand, a missed cache) leaves the body first.
fn reads_only(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> bool {
    reads_slot(constants, instruction).unwrap_or_else(|| match () {
        // ToNumeric of a Number is the Number; any other operand leaves (`Lowering::instruction`). A
        // dense element is read natively, any other leaving (`emit_dense_element_read`); a null or
        // undefined operand leaves before any effect (`staged::require_object_coercible`).
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::ToNumeric { .. }))) || matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) || matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::RequireObjectCoercible { .. })) => !instruction.returns_from_frame(),
        // A Number operator of a Number; any other operand leaves before any effect (`Lowering::unary`).
        // A dense switch's case slot and its branch table read one frame register; an operand that
        // is no int leaves before any effect (`Lowering::int_case_index`, `Lowering::branch_table`).
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::IntCase { .. }))) => instruction.result_flags() == 0,
        _ if instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::BranchTable => instruction.result_flags() == 0,
        _ if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::UnaryOperator
            && crate::compiled::number_unary(instruction, instruction.unary_operator()).is_some() =>
            instruction.result_flags() == 0 && !instruction.writes_current_this(),
        _ => quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && !instruction.returns_from_frame(),
    })
}

/// Whether `instruction` reads only a frame slot, a constant, a capture or a field, where it is
/// one; `None` for the other instructions.
fn reads_slot(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> Option<bool> {
    Some(match () {
        _ if instruction.op().traits().has(Traits::FRAME_ONLY) => true,
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) => constants.get(instruction.constant_index()).and_then(super::facts::immediate).is_some(),
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::IncDec) => instruction.numeric_local_target().is_none(),
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) => instruction.result_flags() == 0 && instruction.capture_depth() == 0,
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => !instruction.writes_current_this() && !instruction.returns_from_frame(),
        _ => return None,
    })
}

/// Whether `instruction` is a store natively: of its atom, into a slot the object already
/// has, so it changes no shape, layout or other property; anything else leaves first.
fn stores_own(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) && !instruction.returns_from_frame()
}

/// Whether `instruction` is an element store natively: into a record array, whose elements
/// no named field read, capture or object shape observes (`Lowering::element_write`); anything
/// else leaves first.
fn stores_element(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) && !instruction.returns_from_frame()
}

/// Whether an array view (`LocalProofs::arrays`) survives `instruction`: it reads no heap
/// value but an element, or replaces one in place (`Lowering::element_write` retires the views
/// itself on its general path), so no array's storage changes.
fn retains_array_views(constants: &[quench_runtime::bytecode::Constant], instruction: WideInstruction) -> bool {
    match () {
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. } | quench_runtime::staged::Operation::LoadCapture { .. })) => false,
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) => true,
        _ => reads_only(constants, instruction),
    }
}

/// Whether a loop may hold `instruction` natively: a read, an own-field or element store, or a
/// call lowered as an intrinsic the loop holds (`native_calls::holds_in_loop`).
fn stays_native(constants: &[Constant], instruction: WideInstruction, intrinsic: bool) -> bool {
    intrinsic || reads_only(constants, instruction) || stores_own(instruction) || stores_element(instruction)
}

/// Whether an allocation makes a holder of named or indexed properties (an object or array literal), whose stores may be
/// fresh (`LocalProofs::fresh`). A closure, regexp or template is no such holder here.
fn makes_holder(allocation: quench_runtime::staged::Allocation) -> bool {
    use quench_runtime::staged::Allocation;
    matches!(allocation, Allocation::Empty | Allocation::Array { .. } | Allocation::ConstArray { .. } | Allocation::ArrayOf { .. } | Allocation::ObjectPair { .. } | Allocation::ObjectLiteral { .. })
}

/// Loops with one external entry and no interior entry, whose native operations
/// only read or replace existing own fields and array elements. Misses leave before guest effects.
/// A sole native entry at the header can supply the same entry proofs.
fn local_loops(unit: &super::lane_unit::Unit, constants: &[quench_runtime::bytecode::Constant], start: usize,
    intrinsics: &native_calls::Intrinsics, global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> Vec<(usize, usize)> {
    let instructions = &unit.instructions;
    let target = |instruction: &WideInstruction| quench_runtime::bytecode::jump_edge(*instruction);
    let mut loops = Vec::new();
    for (back, header) in quench_runtime::bytecode::back_edges_of(instructions.iter().copied()) {
        let entry_header = start == header;
        let body = &instructions[header..=back];
        let entries = usize::from(header > 0 && quench_runtime::bytecode::falls_into_next(header - 1, instructions[header - 1]))
            + instructions[..header].iter().filter(|candidate| target(candidate) == Some(header)).count();
        let side = instructions.iter().enumerate().any(|(pc, candidate)| !(header..=back).contains(&pc)
            && target(candidate).is_some_and(|to| (header + 1..=back).contains(&to) || (to == header && pc > back)));
        if (entries != 1 && !entry_header) || side
            || (header + 1..=back).contains(&start)
            || !body.iter().enumerate().all(|(offset, candidate)| {
                let pc = header + offset;
                stays_native(constants, *candidate, native_calls::holds_in_loop(intrinsics, pc) || native_calls::scalar_name_read(global_reads, pc))
            }) { continue }
        loops.push((header, back));
    }
    loops
}

fn invariant_reads(program: &ResidualProgram, unit: &super::lane_unit::Unit, loops: &[(usize, usize)],
    intrinsics: &native_calls::Intrinsics, quiet_store: &dyn Fn(usize) -> bool,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads) -> BTreeMap<usize, Vec<InvariantRead>> {
    let mut hoists = BTreeMap::new();
    let boundaries = leaders(&unit.instructions);
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let stored: BTreeSet<Atom> = body.iter().filter(|candidate| stores_own(**candidate))
            .map(|candidate| candidate.atom_index()).collect();
        // An element store, an append or a pop may change its array's `length`; a store
        // `quiet_store` proves through a typed view changes none.
        let resizes = body.iter().enumerate().any(|(offset, candidate)| {
            let pc = header + offset;
            (stores_element(*candidate) && !quiet_store(pc)) || native_calls::resizes(intrinsics, pc)
        });
        let mut reads: Vec<InvariantRead> = body.iter().enumerate().filter_map(|(offset, candidate)| {
            let pc = header + offset;
            if matches!(crate::compiled::performed(candidate), Some(quench_runtime::staged::Operation::LoadCapture { .. }))
                && matches!(unit.origins[pc], super::lane_unit::Origin::Caller(_)) {
                return Some(InvariantRead { pc, owner: None, rooted: stored.is_empty(), callee: None });
            }
            if !matches!(crate::compiled::performed(candidate), Some(quench_runtime::staged::Operation::GetField { .. })) { return None; }
            // A native method's read folds to the function its holder holds (`fold_native_member`): its receiver,
            // a string primitive for `charCodeAt`, is no object whose field the entry would hoist.
            if matches!(global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) { return None; }
            match candidate.field_lookup() {
            Some(FieldLookup::Atom { atom, base, .. }) if !stored.contains(&atom) && !(resizes && names_length(program, atom)) => {
                let owner = invariant_field_owner(unit, header, pc, base.register_index()?, &written, &boundaries)?;
                Some(InvariantRead { pc, owner: Some(owner), rooted: false, callee: None })
            }
            _ => None,
        }}).collect();
        // An inlined capture belongs to the actual closure admitted at its
        // marker, not merely to a function body or an observed environment.
        // Its invariant callee read must run first on the entry edge.
        let captures: Vec<_> = body.iter().enumerate().filter_map(|(offset, instruction)| {
            let pc = header + offset;
            let super::lane_unit::Origin::Inlined { frame, .. } = unit.origins[pc] else { return None };
            if !matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) { return None; }
            let read = invariant_callee_read(unit, reads.iter().map(|read| read.pc), frame, &boundaries).is_some();
            // A function-bound callee the loop neither writes nor reads again is held at entry.
            let callee = (!read).then(|| entry_callee(unit, frame, &written, body)).flatten();
            (read || callee.is_some()).then_some(InvariantRead { pc, owner: None, rooted: false, callee })
        }).collect();
        reads.extend(captures);
        if !reads.is_empty() { hoists.insert(header, reads); }
    }
    hoists
}

/// Resolve only block-local receiver copies to a caller home unchanged by the
/// whole loop. A branch or implicit frame initialization ends this proof.
fn invariant_field_owner(unit: &super::lane_unit::Unit, header: usize, pc: usize,
    mut owner: Register, written: &BTreeSet<Register>, boundaries: &BitSet) -> Option<Register> {
    for at in (header..pc).rev() {
        if owner < unit.caller_registers && !written.contains(&owner) { return Some(owner); }
        let instruction = unit.instructions[at];
        if boundaries.contains(at + 1) || unit.markers.contains(&at)
            || instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return None; }
        if writes(instruction).contains(&owner) {
            if !matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) || instruction.result_register() != owner { return None; }
            owner = instruction.register_b();
        }
    }
    (owner < unit.caller_registers && !written.contains(&owner)).then_some(owner)
}

/// The caller register a function-bound inlined frame's closure sits in, where the loop writes
/// it nowhere and stores no capture: the closure it holds at loop entry is every iteration's
/// callee, so its captures are read once on the entry edge (`Lowering::admitted_callee_read`).
/// `None` for an identity-bound or alternative frame, or a callee the loop may change.
fn entry_callee(unit: &super::lane_unit::Unit, frame: usize, written: &BTreeSet<Register>,
    body: &[WideInstruction]) -> Option<Register> {
    let call = unit.frames[frame];
    let marker = unit.markers[frame];
    let register = call.callee_register;
    (call.target.callee == CalleeBinding::Function && call.alternative.is_none()
        && !unit.frames.iter().any(|other| other.alternative == Some(marker))
        && register < unit.caller_registers && !written.contains(&register)
        && !body.iter().any(|instruction| matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::StoreCapture { .. }))))
        .then_some(register)
}

/// A monomorphic frame whose callee comes from an entry read through only
/// block-local copies. Other markers implicitly initialize scratch homes.
fn invariant_callee_read(unit: &super::lane_unit::Unit, reads: impl IntoIterator<Item = usize>, frame: usize,
    boundaries: &BitSet) -> Option<usize> {
    let call = unit.frames[frame];
    let marker = unit.markers[frame];
    if call.alternative.is_some() || unit.frames.iter().any(|other| other.alternative == Some(marker)) {
        return None;
    }
    reads.into_iter().find(|&pc| {
        if pc >= marker || (pc + 1..=marker).any(|at| boundaries.contains(at)) { return false; }
        let mut source = call.callee_register;
        for at in (pc + 1..marker).rev() {
            let instruction = unit.instructions[at];
            if unit.markers.contains(&at)
                || instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return false; }
            if writes(instruction).contains(&source) {
                if !matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) || instruction.result_register() != source { return false; }
                source = instruction.register_b();
            }
        }
        unit.instructions[pc].result_register() == source
    })
}

#[cfg(test)]
fn record_field_forward(cache_site: u16) {
    FIELD_VALUE_FORWARDS.with(|count| count.set(count.get() + 1));
    FIELD_FORWARD_SITE.with(|site| {
        if let Some((expected, count)) = site.get() && expected == cache_site {
            site.set(Some((expected, count + 1)));
        }
    });
}

/// One authoritative alias plan; lowering derives entry probes and typed SSA
/// homes from it. Different stable roots are not proofs of disjoint objects.
struct LoopField {
    header: usize,
    owner: Register,
    atom: Atom,
    reads: smallvec::SmallVec<[usize; INLINE_FIELD_BINDINGS]>,
    stores: smallvec::SmallVec<[usize; INLINE_FIELD_BINDINGS]>,
    /// Conditional on this field's packed/no-extras entry admission.
    store_ranges: smallvec::SmallVec<[Option<IntRange>; INLINE_FIELD_BINDINGS]>,
}

/// Forward must-alias analysis through inline argument copies and branch joins.
/// Only a single backedge is admitted, so every incoming state is complete when
/// visited. Unknown writes decline promotion; no runtime alias cache is needed.
fn loop_fields(program: &ResidualProgram, unit: &super::lane_unit::Unit,
    loops: &[(usize, usize)]) -> Vec<LoopField> {
    let mut fields: Vec<LoopField> = Vec::new();
    for &(header, back) in loops {
        if loops.iter().any(|&(other, end)| (other, end) != (header, back) && other <= back && header <= end)
            || (header..=back).any(|pc| {
                let mut back_edge = false;
                quench_runtime::bytecode::for_each_instruction_successor(pc, unit.instructions[pc], |target| back_edge |= target <= pc && (pc, target) != (back, header));
                back_edge
            })
            || unit.markers.iter().zip(&unit.frames).any(|(marker, frame)|
                (header..=back).contains(marker) && frame.alternative.is_some()) { continue; }
        let mut written: BitSet = BitSet::with_capacity(usize::from(unit.registers));
        for instruction in &unit.instructions[header..=back] {
            for register in writes(*instruction) { written.insert(usize::from(register)); }
        }
        let roots: Vec<Option<Register>> = (0..unit.registers)
            .map(|register| (register < unit.caller_registers && !written.contains(usize::from(register))).then_some(register)).collect();
        let mut incoming = vec![None; back - header + 1];
        incoming[0] = Some(roots);
        let first_field = fields.len();
        let mut stores = Vec::new();
        for pc in header..=back {
            let Some(mut aliases) = incoming[pc - header].take() else { continue };
            if let Some(frame) = unit.markers.iter().position(|marker| *marker == pc).map(|index| unit.frames[index]) {
                for register in frame.base..frame.base + frame.registers { aliases[usize::from(register)] = None; }
                for (destination, source) in frame.passed_registers(program.function(frame.target.function as usize)) {
                    aliases[usize::from(destination)] = aliases[usize::from(source)];
                }
            }
            let instruction = unit.instructions[pc];
            if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) && instruction.result_flags() == 0
                && let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
                && let Some(owner) = base.register_index().and_then(|register| aliases[usize::from(register)]) {
                if let Some(field) = fields[first_field..].iter_mut().find(|field| field.owner == owner && field.atom == atom) {
                    field.reads.push(pc);
                } else {
                    fields.push(LoopField { header, owner, atom, reads: smallvec::smallvec![pc],
                        stores: smallvec::SmallVec::new(), store_ranges: smallvec::SmallVec::new() });
                }
            }
            if stores_own(instruction) {
                let receiver = match instruction.field_store_register() {
                    Some(register) => Some(register),
                    None => match unit.origins[pc] {
                        super::lane_unit::Origin::Inlined { frame, .. } => Some(unit.frames[frame].receiver),
                        _ => None,
                    },
                };
                stores.push((receiver.and_then(|register| aliases[usize::from(register)]), instruction.atom_index(), pc));
            }
            let copied = (matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. }))).then(|| aliases[usize::from(instruction.register_b())]).flatten();
            for register in writes(instruction) { aliases[usize::from(register)] = None; }
            if matches!(super::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) { aliases[usize::from(instruction.result_register())] = copied; }
            let mut forward_targets = smallvec::SmallVec::<[usize; 2]>::new();
            quench_runtime::bytecode::for_each_instruction_successor(pc, instruction, |target| if target > pc && target <= back { forward_targets.push(target) });
            let mut targets = forward_targets.into_iter().peekable();
            while let Some(target) = targets.next() {
                // Linear control transfers ownership of its flat state; only a
                // branch needs a second copy for the second successor.
                let incoming_roots = if targets.peek().is_some() { aliases.clone() } else { std::mem::take(&mut aliases) };
                let slot = &mut incoming[target - header];
                if let Some(known) = slot {
                    for (known, root) in known.iter_mut().zip(incoming_roots) {
                        if *known != root { *known = None; }
                    }
                } else { *slot = Some(incoming_roots); }
            }
        }
        for field in &mut fields[first_field..] {
            if stores.iter().any(|(owner, atom, _)| *atom == field.atom && *owner != Some(field.owner)) { continue; }
            field.stores.extend(stores.iter().filter_map(|(_, atom, pc)| (*atom == field.atom).then_some(*pc)));
        }
        fields.retain(|field| !field.stores.is_empty());
        for field in fields.iter_mut().filter(|field| field.header == header) {
            field.store_ranges = packed_ranges::analyze(program, unit, field, back);
        }
    }
    fields
}

/// What a region specializer is asked to produce: how much of the unit must run, how its
/// runs end, and which element views it may assume. The division and the facts it folds are
/// `VmFacts`, so AOT (`Frozen`) and JIT (`Online`) are two plans over one lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegionPlan {
    pub(crate) coverage: Coverage,
    pub(crate) completion: Completion,
    pub(crate) views: ElementViews,
}

/// The proved loads of each field-cache site, flat and indexed by the site (`site_loads`).
pub(crate) type FieldSites = [Option<Vec<NativeFieldLoad>>];

/// The loads of field-cache `site`, if it has any.
pub(crate) fn site_loads(fields: &FieldSites, site: u16) -> Option<&Vec<NativeFieldLoad>> {
    fields.get(usize::from(site))?.as_ref()
}

/// The flat table holding each `(key, value)` at its key, sized to the largest key.
pub(crate) fn flat_table<T>(entries: impl IntoIterator<Item = (usize, T)>) -> Vec<Option<T>> {
    let entries: Vec<(usize, T)> = entries.into_iter().collect();
    let mut table: Vec<Option<T>> = Vec::new();
    table.resize_with(entries.iter().map(|(key, _)| key + 1).max().unwrap_or(0), || None);
    for (key, value) in entries { table[key] = Some(value); }
    table
}

/// Futamura's first projection for one region: the same `lower` a lane body, a static typed
/// region and a numeric region run, parameterized by its `RegionPlan`. AOT and JIT both call
/// this specializer; only the plan and the `VmFacts` division differ.
pub(crate) struct RegionSpecializer<'p> {
    pub(crate) function_id: u32,
    pub(crate) unit: &'p super::lane_unit::Unit,
    pub(crate) helpers: LaneHelpers,
    pub(crate) fields: &'p FieldSites,
    pub(crate) global_reads: &'p quench_runtime::vm::global_facts::GlobalReads,
    pub(crate) facts: VmFacts<'p>,
    pub(crate) generic: &'p [usize],
    pub(crate) direct: &'p DirectSites,
    pub(crate) frame_captures: &'p [(usize, usize)],
    pub(crate) isa: &'p dyn cranelift_codegen::isa::TargetIsa,
    /// The typed array kinds the body's element records took (`body_views::typed_reads`), by unit PC (sorted).
    pub(crate) kinds: &'p [super::body_views::ObservedKind],
    pub(crate) plan: RegionPlan,
    /// The gate's proof that this region's compile may run (minted by `specialize`).
    pub(crate) admitted: super::admission::CompiledUnit,
}

impl Specializer for RegionSpecializer<'_> {
    type Residual = LoweredRegion;

    fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String> {
        lower(program, self.function_id, self.unit, self.helpers, self.fields,
            self.global_reads, self.facts, self.generic, self.direct, self.frame_captures,
            self.isa, self.kinds, self.plan.coverage, self.plan.completion, self.plan.views)
            .ok_or_else(|| format!("function {} does not lower under {:?}", self.function_id, self.plan))
    }
}

thread_local! {
    /// The first reason this thread's current lowering declined, and its PC (`usize::MAX`: none).
    static DECLINED: std::cell::Cell<Option<(&'static str, usize)>> = const { std::cell::Cell::new(None) };
    /// The opcodes this thread's current lowering leaves at because it has no lowering for them.
    static UNLOWERED: std::cell::RefCell<std::collections::BTreeSet<String>> = const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    /// Whether this thread's current lowering wrote a value whose lane disagrees with the
    /// register's representation (`register_define`): the register's variable is per representation,
    /// so the body is no well-formed lowering and is declined.
    static MISMATCH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Notes that the lowering wrote a value of the wrong representation (`register_define`).
pub(super) fn note_mismatch() {
    MISMATCH.with(|mismatch| mismatch.set(true));
}

/// Whether the current lowering wrote a representation mismatch.
fn mismatched() -> bool {
    MISMATCH.with(std::cell::Cell::get)
}

/// Notes why the lowering declines, for `QUENCH_TRACE=aot-lanes`; the first reason stands.
fn declined(why: &'static str, pc: usize) {
    DECLINED.with(|declined| if declined.get().is_none() { declined.set(Some((why, pc))); });
}

/// Lowers `unit` (`lane_unit::unit`) as a lane body, whose field cache sites `fields`
/// gives the proved shapes and slots of; `None` where it does not lower.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_body(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &DirectSites,
    frame_captures: &[(usize, usize)], entry: Option<usize>,
    kinds: &[super::body_views::ObservedKind], views: ElementViews,
    admitted: super::admission::CompiledUnit,
) -> Option<LoweredLane> {
    let isa = super::jit_backend::target_isa()?;
    DECLINED.with(|declined| declined.set(None));
    UNLOWERED.with(|unlowered| unlowered.borrow_mut().clear());
    MISMATCH.with(|mismatch| mismatch.set(false));
    let body = RegionSpecializer { function_id, unit, helpers, fields, global_reads, facts: vm,
        generic, direct, frame_captures, isa: &*isa, kinds, admitted,
        plan: RegionPlan { coverage: entry.map_or(Coverage::Prefix, Coverage::PrefixFrom),
            completion: Completion::Body, views } };
    // The scalar entry lowers first: the body calls it only where it exists.
    let abi = scalar::ScalarAbi::of(program, function_id, unit, direct, vm.facts).filter(|_| entry.is_none());
    let scalar = abi.and_then(|abi| scalar::lowered(program, RegionSpecializer { generic: &[], facts: VmFacts { scalar: Some(abi), ..vm }, plan: scalar::plan(abi), ..body }));
    let region = RegionSpecializer { facts: VmFacts { scalar: abi.filter(|_| scalar.is_some()), ..vm }, ..body }.specialize(program)
        .inspect_err(|_| if helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            let (why, pc) = DECLINED.with(|declined| declined.get()).unwrap_or(("a decline no note names", usize::MAX));
            let op = unit.instructions.get(pc).map(|instruction| format!("{:?}", instruction.op())).unwrap_or_default();
            let unlowered = UNLOWERED.with(|unlowered| unlowered.borrow().iter().cloned().collect::<Vec<_>>().join(","));
            eprintln!("aot-lanes: function {function_id}: lowering declines: {why} {op} [leaves at {unlowered}]");
        }).ok()?;
    // The proven entry lowers the same body with its parameters' entry guards taken as proved (`VmFacts::proven_entry`).
    // It is kept only where it folds exactly the body's Stable facts, which its publication takes once for both.
    let params = program.function_get(function_id as usize).is_some_and(|function| function.params > 0);
    let proven = (entry.is_none() && params).then(|| RegionSpecializer { facts: VmFacts { proven_entry: true, ..vm }, ..body }
        .specialize(program).ok()).flatten()
        .filter(|proven| proven.stable == region.stable && proven.fault_poll == region.fault_poll && proven.hoists_arrays == region.hoists_arrays
            && proven.entry_types.iter().any(|ty| *ty != Type::ANY))
        .map(|proven| (proven.function, proven.entry_types));
    super::residuals::report_clif(function_id, &region.function);
    let clif = native_dump_directory().map(|_| region.function.to_string());
    Some(LoweredLane { region, clif, scalar, proven })
}

/// What an ahead lane body reads of its image. Its leaves return their exit words, as a JIT lane's
/// do: a base body is entered only at its start (a record inside it has no resume stub to load
/// its pinned registers), and a base body leaves the same way.
#[derive(Clone, Copy)]
pub(crate) struct AheadLeaves<'a> {
    /// By unit PC, the calls the closed world proves whose records the body reads (`ProvenCall`): one, or one per
    /// function of a method read's closed set (`AotFacts::call_targets`).
    pub(crate) calls: &'a BTreeMap<usize, Vec<ProvenCall>>,
    /// By constant pool slot, the heap value key (`HeapValueRecipe::String`) of each string
    /// constant the unit loads.
    pub(crate) strings: &'a BTreeMap<usize, u32>,
}

/// The position a failed entry contract takes in a lane body's leave set
/// (`LaneExit::Entry`): past every instruction, so it names none of them. A plan with it
/// proves no input contract.
pub(crate) fn entry_leave(unit: &super::lane_unit::Unit) -> usize {
    unit.instructions.len()
}

/// Build the typed residual and validate every value/CFG edge before publication.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &quench_runtime::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &DirectSites,
    frame_captures: &[(usize, usize)],
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    kinds: &[super::body_views::ObservedKind],
    coverage: Coverage,
    completion: Completion,
    views: ElementViews,
) -> Option<LoweredRegion> {
    // A loop entry may take the rotated residual of its proven condition.
    let residual = super::static_loops::specialize(program, unit, coverage.entry())
        .map(|instructions| super::lane_unit::Unit { instructions, origins: unit.origins.clone(),
            frames: unit.frames.clone(), markers: unit.markers.clone(), caller_registers: unit.caller_registers,
            registers: unit.registers, end: unit.end });
    let unit = residual.as_ref().unwrap_or(unit);
    let naming = super::NamingSymbols::enter(helpers);
    crate::dispatch_fast::lane_body_stats::lowering_function(function_id);
    let function = program.function_get(function_id as usize)?;
    let numeric = completion.numeric();
    if numeric && (!crate::js_compiled::function_candidate(function, program)
        || !scalar::numeric_unit(program, unit, completion != Completion::Number, direct) || function.params > unit.caller_registers) { return None; }
    if fields.iter().flatten().flatten().any(|field| !field.valid()) { declined("an invalid field site", usize::MAX); return None; }
    if unit.instructions.is_empty() || unit.instructions.len() > MAX_NATIVE_RECORDS {
        declined("past the native record count", usize::MAX);
        return None;
    }
    let signature = completion.signature(isa.pointer_type(), isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(UserFuncName::user(LANE_REGION_NAMESPACE, function_id), signature);
    // Every access this lowering emits is tagged with its alias region (`regions::flags`), so the
    // function names them before anything verifies or compiles it (declaring again is a no-op).
    super::regions::declare_regions(&mut native);
    // The SSA builder's tables come back from the last lowering that finished, which
    // `finalize` left empty; one that gave up drops its context with it.
    let mut builder_context = super::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    let parameters = builder.block_params(entry_block).to_vec();
    let [context, entry_this, entry_captures, registers_base] = scalar::entry_words(&mut builder, completion, &parameters)?;
    let mut stage = emit(&mut builder);
    let payload = if numeric { stage.raw_const(Lane::I64, 0) } else {
        stage.raw_load(Lane::I64, regions::dispatch(), context, std::mem::offset_of!(CallContext, head.adapter_payload) as i32) };
    // An image body is linked away from its compile, so a fault poll would name the compiling
    // process's fault address: its back edge polls `context.interrupt` instead (no trapping check).
    let fault_poll = !matches!(helpers, LaneHelpers::Ahead)
        && match coverage { Coverage::Loop { poll, .. } => poll, _ => true }
        && super::FAULT_POLL_SUPPORTED && quench_runtime::bytecode::back_edges_of(unit.instructions.iter().copied()).next().is_some();
    let mut seen_headers: BitSet = BitSet::with_capacity(unit.instructions.len());
    let mut shared_headers = BitSet::with_capacity(unit.instructions.len());
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Jump)) { continue; }
        let target = instruction.jump_target() as usize;
        if target > pc { continue; }
        if seen_headers.contains(target) { shared_headers.insert(target); }
        seen_headers.insert(target);
    }
    let interrupt = super::native_interrupt_pointer(&mut builder, context, fault_poll);
    let helpers = if let LaneHelpers::Linked(symbol, narrow_table) = helpers {
        let signature = builder.import_signature(native_lane_helpers_signature(isa));
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            super::native_object::NATIVE_FUNCTION_NAMESPACE, symbol));
        let target = builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name),
            signature, colocated: false, patchable: false });
        LaneHelpers::Bound(emit(&mut builder).call_function(target, &[])[0], narrow_table)
    } else { helpers };
    let interrupt_owner = fault_poll.then(|| polls::interrupt_owner_call(&mut builder, helpers, context, isa));
    let count = usize::from(unit.registers);
    let live = if unit.registers > quench_runtime::compile::liveness::ROOT_MAP_REGISTERS {
        Liveness::Frames {
            caller: program.register_root_masks(function_id),
            frames: unit.frames.iter().map(|frame| program.register_root_masks(frame.target.function)).collect(),
        }
    } else {
        Liveness::Unit(unit.register_masks(program, function_id))
    };
    let registers = Registers {
        tagged: (0..count).map(|_| builder.declare_var(types::I64)).collect(),
        int: vec![None; count],
        long: vec![None; count],
        float: vec![None; count],
    };
    // An alternative of a call's switch starts a block: a callee other than the one before
    // it branches there (`Lowering::inline_call`).
    let mut leaders = leaders(&unit.instructions);
    let entry_pc = coverage.entry();
    if !matches!(unit.origins.get(entry_pc), Some(super::lane_unit::Origin::Caller(_))) { declined("the entry is not the caller's", entry_pc); return None; }
    leaders.insert(entry_pc);
    for alternative in unit.frames.iter().filter_map(|frame| frame.alternative) {
        if alternative < unit.instructions.len() { leaders.insert(alternative); }
    }
    // A handler's block is entered by the throws it catches, edges no jump names.
    let catches = catches::catch_edges(program, function, unit, coverage, completion);
    for catch in catches.iter().flatten() { leaders.insert(catch.target); }
    let blocks: Vec<_> = (0..unit.instructions.len()).map(|pc| leaders.contains(pc).then(|| builder.create_block())).collect();
    let (leave, stop) = (builder.create_block(), builder.create_block());
    builder.append_block_param(leave, types::I32);
    builder.append_block_param(leave, types::I64);
    builder.append_block_param(stop, types::I64);
    builder.set_cold_block(leave);
    builder.set_cold_block(stop);
    // The typed array kind each access of the unit's own function proves (`FunctionFacts::element_view`).
    let element_facts = vm.facts;
    let element_kind = |pc: usize| match unit.origins.get(pc) {
        Some(super::lane_unit::Origin::Caller(pc)) => element_facts?.function_facts(function_id).element_view(*pc),
        _ => None,
    };
    let mut lowering = Lowering {
        builder,
        program,
        unit,
        params: usize::from(function.params),
        function_id,
        helpers,
        fields,
        kinds,
        interrupt,
        fault_poll,
        trips: None,
        reentries: smallvec::SmallVec::new(),
        polls: smallvec::SmallVec::new(),
        poll_latches: super::SecondaryMap::new(),
        shared_headers,
        global_reads,
        plans: bindings::CallPlans::new(program, vm.facts, vm.graph),
        vm,
        stable: Default::default(),
        generic,
        proving_entry: false, left_here: false, held_view: None, hole_leaves: false, entry_exit: None,
        direct,
        frame_captures,
        context,
        payload,
        registers_base,
        entry_words: (entry_this, entry_captures),
        callee_slots: Vec::new(),
        declared_definitions: Vec::new(),
        declared_this: None,
        declared_rest: None,
        declared_members: Vec::new(),
        declared_member_start: None,
        registers,
        written: BitSet::with_capacity(count),
        consumed: BitSet::with_capacity(unit.instructions.len()),
        blocks,
        exits: std::iter::repeat_with(Vec::new).take(unit.instructions.len() + 1).collect(),
        shared_exits: Vec::new(),
        demands: demand::analyze(program, unit, &leaders, &live),
        entry: None,
        native: 0,
        backedge: false,
        pending: Vec::new(),
        call_out_stop: None,
        // A static typed region's calls are guarded leaves (`guarded_call_instruction`); here any call-like one is native.
        native_calls: completion.status_and_pc()
            && unit.instructions.iter().any(|instruction| instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Call),
        call_budget: None,
        facts: super::facts::Facts::default(),
        live,
        valid: true,
        coverage,
        completion,
        number_returns: smallvec::SmallVec::new(),
        hoists: BTreeMap::new(),
        unit_leaders: std::cell::OnceCell::new(),
        intrinsics: Vec::new(),
        hoisted: BTreeMap::new(),
        scalar_values: BTreeMap::new(),
        prototype_clear: BTreeMap::new(),
        int_only: BTreeSet::new(),
        construct_ready: Vec::new(),
        loop_arrays: Vec::new(), loop_strings: Vec::new(), string_vars: BTreeMap::new(), pending_unit: None, loop_shapes: Vec::new(), arrays_fixed: false,
        array_vars: BTreeMap::new(),
        views,
        loop_fields: Vec::new(),
        loop_stores: BTreeMap::new(),
        ranges: int_ranges::IntRanges::new(0),
        at: 0,
        field_regions: BTreeMap::new(),
        forward: Vec::new(), sealable: BitSet::new(),
        arrived: BTreeMap::new(), joined_virtuals: BTreeMap::new(), loop_carry: BTreeMap::new(), carried_typed: BTreeMap::new(),
        entered: BitSet::with_capacity(unit.instructions.len()),
        finishes: false,
        leave,
        stop,
        raises: catches::raises(&catches),
        catches,
    };
    lowering.field_regions = lowering.declare_fields();
    let instructions = unit.instructions.clone();
    for instruction in &instructions {
        for register in writes(*instruction).into_iter().map(usize::from).filter(|r| *r < count) {
            lowering.written.insert(register);
        }
    }
    let mut initial = completion.initial_types(count, usize::from(function.params));
    if let Coverage::Loop { header, backedge, .. } = coverage {
        seed_constant_entries(&instructions, program, (header, backedge), &mut initial);
    }
    if !numeric {
        if let Some(caller) = match unit.origins.get(entry_pc) { Some(super::lane_unit::Origin::Caller(pc)) => Some(*pc), _ => None } {
            int_inputs::solved_entry_types(vm.facts, function_id, function, caller, &lowering.written, &mut initial);
        }
        lowering.declared_definitions = declared::declared_local_types(program, function);
        int_inputs::closed_local_types(vm, function_id, function, &mut lowering.declared_definitions);
        lowering.declared_this = declared::declared_this(program, function);
        lowering.declared_rest = declared::declared_rest_read(program, function, &lowering.written);
        lowering.declared_members = declared::declared_member_reads(program, function, &lowering.written);
        lowering.declared_member_start = function.source_span.map(|span| span.start);
    }
    let own_shape = |pc: usize| {
        let instruction = instructions[pc];
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) || instruction.result_flags() != 0 { return None; }
        // A native method's read names its holder's function whatever its receiver (`fold_native_member`):
        // a string primitive's `charCodeAt` proves no object shape of it.
        if matches!(global_reads.get(&pc), Some(quench_runtime::vm::global_facts::GlobalRead::NativeMember { .. })) { return None; }
        let FieldLookup::Atom { base, cache_site, .. } = instruction.field_lookup()? else { return None };
        let [field] = site_loads(fields, cache_site)?.as_slice() else { return None };
        field.own_direct().then_some((base.register_index()?, field.shape))
    };
    let kind_reads = super::body_views::kind_element_types(kinds, &instructions);
    let typed_seeds = super::facts::Seeds { typed_reads: &kind_reads, ..super::facts::Seeds::typed() };
    lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &lowering.raises, typed_seeds);
    lowering.intrinsics = native_calls::intrinsic_calls(unit, global_reads, |pc| direct.contains_key(&pc));
    let loops = local_loops(unit, &program.constants, entry_pc, &lowering.intrinsics, global_reads);
    // One entry query of the call depth replaces each binding's helper with its cached
    // target probe (`cached_call_target`). A completed call leaves the depth it found.
    if !numeric && unit.frames.iter().any(|frame| frame.target.callee == CalleeBinding::Function) {
        let helper = lowering.helpers.target(&mut lowering.builder, LaneHelper::CallBudget);
        lowering.call_budget = Some(emit_lane_helper_call(&mut lowering.builder, lowering.payload, helper, &[]));
    }
    if coverage.selected_entry() {
        lowering.written = lowering.facts.written(&instructions, &writes, count);
        // Backedge definitions choose a numeric entry signature. Conformance
        // proves the external input once; every subsequent edge carries the
        // same verified SSA view. This is an admission requirement, not a fact
        // assumed about the source program (a declared parameter type is checked
        // the same way: the solve's entry cell).
        let mut refined = false;
        for (input, carried) in initial.iter_mut().take(usize::from(unit.caller_registers)).zip(lowering.facts.backedge().iter().map(|ty| ty.machine())) {
            if (carried == Type::INT32 || carried.int_range().is_some() || number_domains::is_double(carried)) && *input != carried {
                *input = carried;
                refined = true;
            }
        }
        // An integer induction value can compare against an invariant integer
        // input directly. This selects a checked entry contract; a fractional
        // or wider bound resumes the original header with its value unchanged.
        let instruction = instructions[entry_pc];
        if !lowering.facts.backedge().is_empty() && matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. }))
            && matches!(super::operations::NumberRelation::from_operator(instruction.binary_operator_field()),
                    Some(super::operations::NumberRelation::LessThan | super::operations::NumberRelation::LessEqual
                        | super::operations::NumberRelation::GreaterThan | super::operations::NumberRelation::GreaterEqual)) {
            let operands = [instruction.operand_b(), instruction.operand_c()];
            if operands.iter().any(|operand| super::facts::operand_type(&program.constants, *operand,
                |register| lowering.facts.backedge().get(usize::from(register)).copied().unwrap_or(Type::ANY)).is_int32()) {
                for register in operands.into_iter().filter_map(Operand::register_index)
                    .filter(|register| *register < unit.caller_registers && !lowering.written.contains(usize::from(*register))) {
                    if number_domains::is_double(initial[usize::from(register)]) {
                        initial[usize::from(register)] = Type::INT32;
                        refined = true;
                    }
                }
            }
        }
        if refined { lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &lowering.raises, typed_seeds); }
        lowering.refine_inductive_inputs(&mut initial, &instructions, &leaders, &loops, &own_shape);
    }
    lowering.refine_loop_inputs(entry_pc, &initial, &instructions, &leaders, &loops, &own_shape);
    lowering.require_object_latches(&instructions, &leaders, &loops);
    for &pc in generic { lowering.facts.widen(pc); }
    if !numeric { lowering.entry = Some(lowering.entry_facts(&instructions, &loops)); }
    // A preliminary run of the facts over the arrays the loops index without their captured reads
    // (`loop_arrays` before the hoists): a typed or double view's store the run proves a Number store
    // changes no length, so it does not keep a loop's `length` from its entry (`invariant_reads`).
    lowering.loop_arrays = loop_arrays::loop_arrays(unit, &loops, &leaders, lowering.views, &element_kind, &lowering.intrinsics, |pc| coverage.lowered(pc));
    let preliminary = lowering.prove_ranges(entry_pc, &initial, &instructions, &leaders, &own_shape, &loops);
    let quiet_store = |pc: usize| preliminary.numeric_store(pc) && lowering.typed_access(pc) == Some(super::facts::TypedAccess::Store);
    let hoists = invariant_reads(program, unit, &loops, &lowering.intrinsics, &quiet_store, global_reads);
    lowering.hoists = hoists;
    lowering.loop_arrays = loop_arrays::loop_arrays(unit, &loops, &leaders, lowering.views, &element_kind, &lowering.intrinsics, |pc| coverage.lowered(pc));
    let arrays = std::mem::take(&mut lowering.loop_arrays);
    lowering.loop_arrays = arrays.into_iter().filter(|array| !lowering.string_base(array)).collect();
    lowering.loop_strings = loop_strings::loop_strings(unit, &loops, &lowering.intrinsics, |pc| coverage.lowered(pc));
    lowering.ranges = lowering.prove_ranges(entry_pc, &initial, &instructions, &leaders, &own_shape, &loops);
    lowering.arrays_fixed = lowering.arrays_stay_put();
    lowering.loop_fields = loop_fields(program, unit, &loops);
    for field in &lowering.loop_fields {
        lowering.hoists.entry(field.header).or_default().push(InvariantRead { pc: field.reads[0], owner: Some(field.owner), rooted: false, callee: None });
    }
    // Field admissions establish the actual counterpart types used to select
    // capture views. Callee reads/bindings must precede their inlined captures.
    for reads in lowering.hoists.values_mut() {
        reads.sort_by_key(|read| (matches!(crate::compiled::performed(&instructions[read.pc]), Some(quench_runtime::staged::Operation::LoadCapture { .. })),
            matches!(unit.origins[read.pc], super::lane_unit::Origin::Inlined { .. })));
    }
    (lowering.forward, lowering.sealable) = edges::leader_edges(&instructions, &leaders, entry_pc);
    lowering.loop_carry = loop_carries(&instructions, &leaders, entry_pc);
    // No forward edge of the bytecode's enters a handler's block: its proofs are none, and it
    // seals once the region ends.
    for (pc, catch) in lowering.catches.iter().enumerate().filter_map(|(pc, catch)| Some((pc, (*catch)?))) {
        lowering.forward[catch.target] = None;
        lowering.sealable.remove(catch.target);
        // A throw of an inlined callee reaches its caller's handler block after the walk passed
        // it: the block is lowered when the walk reaches it, as one an edge entered.
        if catch.target < pc { lowering.entered.insert(catch.target); }
    }
    lowering.entered.insert(entry_pc);
    lowering.stable.start = entry_pc;
    let mut proven_types: Box<[Type]> = Box::default();
    {
        let pc = entry_pc;
        let block = lowering.builder.create_block();
        lowering.goto(block, &[]);
        lowering.builder.switch_to_block(block);
        lowering.begin_trips();
        if !matches!(lowering.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let undefined = lowering.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
        for register in 0..count {
            // A numeric entry's window holds its parameters alone; a scalar entry has none.
            let in_window = match completion {
                Completion::Scalar(_) => false,
                Completion::Number => register < usize::from(function.params),
                Completion::Body => register < usize::from(lowering.unit.caller_registers) && lowering.is_live(pc, register as Register),
            };
            let bits = if in_window {
                lowering.raw_load(Lane::I64, regions::frame(), registers_base, register_offset(register as Register))
            } else {
                undefined
            };
            lowering.builder.def_var(lowering.registers.tagged[register], bits);
        }
        let target = lowering.blocks.get(pc).copied().flatten()?;
        let mut state = State::canonical(count);
        if let Some(budget) = lowering.call_budget {
            // Every body edge is dominated by this admission. The closed region
            // cannot change thread call depth before leaving to materialize.
            let depth = lowering.unit.recovery_depth().max(1) as i64;
            let admitted = lowering.raw_compare_imm(Relation::UnsignedGreaterEqual, budget, depth);
            let miss = lowering.guard_exit(&state, pc as u32);
            emit_js_guard(&mut lowering.builder, admitted, miss);
        }
        if numeric { lowering.enter_numeric_parameters(&mut state, pc, &parameters); }
        if fault_poll {
            // Publication lends a resumable page for this entire no-GC entry.
            // If unavailable, leave before any guest effect through ordinary recovery.
            let miss = lowering.guard_exit(&state, pc as u32);
            let available = lowering.raw_compare_imm(Relation::NotEqual, interrupt, 0);
            emit_js_guard(&mut lowering.builder, available, miss);
        }
        // A proven entry's caller proved each argument has its parameter's entry type (`call_frames::call_target`), so the
        // guards those types make are not run: the state holds them, and the entry conform proves them with no test. A
        // shape is not held this way: its guard runs in every entry.
        if lowering.vm.proven_entry {
            let entered: Vec<Type> = lowering.facts.at(pc).map(|types| types.iter().copied().take(usize::from(function.params)).collect()).unwrap_or_default();
            for (index, ty) in entered.iter().copied().enumerate() {
                if !lowering.is_live(pc, index as Register) || ty.object_shape().is_some() { continue; }
                state.values[index] = RegisterFact { tagged: state.values[index].tagged, ..RegisterFact::definition(ty, Representation::Tagged) };
            }
            proven_types = entered.into_boxed_slice();
        }
        lowering.narrow_to_cells(&mut state, pc);
        lowering.entry_conform(&mut state, pc, coverage);
        // A region whose sole entry is this loop header has no earlier source
        // edge on which to hoist its invariant reads. Admit them on the native
        // entry edge, before the header and every possible loop effect. A failed
        // entry proof declines the region (the lane runs the header itself), not
        // an exit: the old lowering's entry admission declines the same arrays.
        // A bounded range reads its fields at their sites; hoisting them to a shared entry
        // guard would make every call leave at the entry.
        lowering.proving_entry = true;
        lowering.hoist_entry(&mut state, pc);
        lowering.proving_entry = false;
        lowering.goto(target, &[]);
    }
    let summarized = completion == Completion::Body && matches!(coverage, Coverage::Prefix);
    let mut census = clif_census::Census::new();
    let emitted = |lowering: &Lowering<'_, '_>| lowering.builder.func.dfg.num_insts();
    let mut mark = emitted(&lowering);
    let mut block_mark = lowering.builder.func.dfg.num_blocks();
    if let Some(census) = census.as_mut() { census.part(clif_census::Part::Entry, mark); }
    let mut state = State::canonical(count);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if !coverage.lowered(pc) { continue; }
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.hoist_entry(&mut state, pc);
                lowering.conform(&mut state, pc);
                lowering.carry_kept(&mut state, pc);
                lowering.arrive(&state, pc);
                lowering.entered.insert(pc);
                lowering.goto(block, &[]);
            }
            // Marker alternatives add real native edges beyond the bytecode
            // analysis's Nop fallthrough. Missing type facts select canonical
            // guarded lowering; they do not make an entered block unreachable.
            if !lowering.entered.contains(pc) {
                reachable = false;
                continue;
            }
            lowering.switch_to(block, lowering.sealable.contains(pc));
            state = lowering.entry_state(pc, count);
            lowering.join_virtuals(&mut state, pc);
            lowering.keep_shared_proofs(&mut state, pc);
            lowering.narrow_to_cells(&mut state, pc);
            lowering.carry_typed_views(&mut state, pc);
            lowering.seed_loop_shapes(&mut state, pc);
            reachable = true;
        }
        if !reachable {
            continue;
        }
        lowering.at = pc;
        if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Edges, now - mark); mark = now; block_mark = lowering.builder.func.dfg.num_blocks(); }
        reachable = lowering.instruction(&mut state, pc, instruction);
        lowering.retire_fresh(&mut state, pc, instruction);
        if let Some(census) = census.as_mut() {
            let now = emitted(&lowering);
            if census.sample(instruction.op()) {
                let insts: Vec<String> = (mark..now).map(|index| format!("{}", lowering.builder.func.dfg.display_inst(ir::Inst::new(index)))).collect();
                eprintln!("QUENCH_CLIF_SAMPLE {:?} pc={pc} n={}\n  {}", instruction.op(), now - mark, insts.join("\n  "));
            }
            census.op(instruction.op(), now - mark); mark = now;
            let blocks = lowering.builder.func.dfg.num_blocks();
            census.blocks(instruction.op(), blocks - block_mark); block_mark = blocks;
        }
    }
    if reachable {
        lowering.exit(&mut state, coverage.exit_pc(instructions.len()) as u32);
    }
    for (header, latch) in std::mem::take(&mut lowering.poll_latches).iter() {
        if let Some(latch) = latch {
            lowering.builder.switch_to_block(*latch);
            lowering.emit_fault_poll();
            lowering.goto(header, &[]);
        }
    }
    // A prefix pays for its entry with a loop, or with the calls it runs inlined.
    if matches!(coverage, Coverage::PrefixFrom(_)) && !lowering.backedge && unit.frames.is_empty() { return None; }
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Tail, now - mark); mark = now; }
    let named = lowering.named_leaves();
    for GuardExit { block, values, homed, virtuals, virtual_arrays, pc, line, .. } in lowering.take_leaves() {
        lowering.switch_to(block, true);
        if helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            eprintln!("aot-lanes: function {function_id}: a leave at {pc} (lowering line {line})");
        }
        let mut leaving = State { values, proofs: LocalProofs { homed, ..LocalProofs::default() }, refinements: smallvec::SmallVec::new(), virtuals, virtual_arrays };
        lowering.record_line(line);
        lowering.leave_at(&mut leaving, pc);
    }
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Leaves, now - mark); mark = now; }
    lowering.fill_call_out_stop();
    if lowering.fill_exits().is_none() { declined("an exit fill declines", usize::MAX); return None; }
    let exits = ExitTable::of_unit(unit, |pc, frame| lowering.inlined_stores(pc, frame), |pc| named.get(pc).copied().flatten());
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Exits, now - mark); mark = now; }
    lowering.builder.switch_to_block(stop);
    let word = lowering.builder.block_params(stop)[0];
    let status = lowering.raw_const(Lane::I32, i64::from(NATIVE_JS_STOPPED));
    lowering.leave_with(status, Some(word));
    lowering.builder.switch_to_block(leave);
    let results = lowering.builder.block_params(leave).to_vec();
    lowering.flush_trips();
    lowering.leave(&results);
    if let Some(census) = census.as_mut() { let now = emitted(&lowering); census.part(clif_census::Part::Tail, now - mark); census.report(function_id, now);
        let mut opcodes: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for index in 0..now { *opcodes.entry(format!("{:?}", lowering.builder.func.dfg.insts[ir::Inst::new(index)].opcode())).or_default() += 1; }
        let blocks = lowering.builder.func.layout.blocks().count();
        let cold = lowering.builder.func.layout.blocks().filter(|block| lowering.builder.func.layout.is_cold(*block)).count();
        eprintln!("QUENCH_CLIF_BLOCKS {blocks} {cold}");
        eprintln!("QUENCH_CLIF_OPCODES {}", opcodes.iter().map(|(name, count)| format!("{name}={count}")).collect::<Vec<_>>().join(" ")); }
    // A lane body is a whole function body and must complete; so must an inlined unit, whose
    // callees rejoin it. A loop or range over the function's own code completes by its exits
    // and backedge, which the plan emits where the interpreter resumes.
    let must_finish = match coverage {
        Coverage::Prefix | Coverage::PrefixFrom(_) | Coverage::Full => true,
        Coverage::Loop { .. } => !unit.frames.is_empty(),
    };
    if !lowering.valid || (completion.status_and_pc() && must_finish && !lowering.finishes) {
        if lowering.valid { declined("the body does not finish", usize::MAX); }
        return None;
    }
    let number_result = numeric.then(|| lowering.finish_number_returns());
    let summary = summarized.then(|| TypeSummary);
    let native_count = lowering.native;
    let hoists_arrays = !lowering.loop_arrays.is_empty();
    let mut polls = std::mem::take(&mut lowering.polls);
    let reentries = std::mem::take(&mut lowering.reentries);
    let stable = lowering.stable.take();
    let field_regions = std::mem::take(&mut lowering.field_regions);
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    super::regions::retag_field_regions(&mut native, &field_regions);
    super::jit_backend::builder::keep_context(builder_context);
    let decided = known_branches::decide(&mut native);
    if clif_census::enabled() { eprintln!("QUENCH_CLIF_DECIDED {decided}"); }
    if let Some(owner) = interrupt_owner { specialize_closed_polls(&mut native, &reentries, &mut polls, owner); }
    if naming.folded() { declined("a slow path no helper names", usize::MAX); return None; }
    Some(LoweredRegion { function: native, native: native_count, fault_poll, number_result, hoists_arrays, stable, summary, entry_types: proven_types, exits })
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

/// A loop's entry types the function's bytecode proves: a register the loop reads but never writes,
/// whose every definition before or after the loop loads a constant that is no Number (a string,
/// a boolean, `null`), has the join of those constants' types, which the entry guards once. A
/// relation reading it is then decided by the interpreter's rule (`Lowering::decided_relation`)
/// instead of being taken for a Number comparison whose entry guard always fails.
fn seed_constant_entries(instructions: &[WideInstruction], program: &ResidualProgram,
    (header, backedge): (usize, usize), initial: &mut [Type]) {
    let inside = header..=backedge;
    let written: std::collections::BTreeSet<Register> = instructions.get(inside.clone()).into_iter().flatten()
        .filter_map(|instruction| instruction.defined_register()).collect();
    let mut seeded = std::collections::BTreeMap::<Register, Option<Type>>::new();
    for (_, instruction) in instructions.iter().enumerate().filter(|(pc, _)| !inside.contains(pc)) {
        let Some(register) = instruction.defined_register() else { continue };
        if written.contains(&register) { continue; }
        // A definition by a constant-table index (the staged row of a heap constant, a string, has no
        // stage word to decode, so the instruction's layout names it); the table types it.
        let ty = (instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::ConstantIndex)
            .then(|| program.constants.get(instruction.constant_index())).flatten()
            .map(super::facts::constant_type).filter(|ty| super::facts::numberless(*ty));
        let slot = seeded.entry(register).or_insert(ty);
        *slot = slot.zip(ty).map(|(left, right)| left.join(right));
    }
    for (register, ty) in seeded {
        if let (Some(ty), Some(slot)) = (ty, initial.get_mut(usize::from(register))) && *slot == Type::ANY { *slot = ty; }
    }
}
