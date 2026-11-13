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
use crate::vm::compiled::regions;
use super::*;
use super::super::specializer::Specializer;
use crate::typed_ir::{BindingTime, Fact, Projection, Proof, Reentry, Representation, Type, VerifiedFact};
use super::object_slots::{CheckedObject, CheckedObjectTable, Emit, ObjectHeader, ObjectRecord, ObjectTable};
use crate::typed_ir::Division;
use crate::staged::typed_elements::{NativeKind, ViewKind};
use crate::staged::typed_view::{prove_typed_view, TypedRecords, ViewAccess};
use std::collections::{BTreeMap, BTreeSet};
use cranelift_codegen::bitset::CompoundBitSet;
use cranelift_codegen::entity::EntityRef;

#[path = "lane_region/packed_ranges.rs"]
mod packed_ranges;
#[path = "lane_region/int_ranges.rs"]
mod int_ranges;
#[cfg(all(test, feature = "aot"))]
#[path = "lane_region/int_ranges_tests.rs"]
mod int_ranges_tests;
#[path = "lane_region/global_reads.rs"]
mod global_reads;
#[path = "lane_region/native_calls.rs"] mod native_calls;
#[path = "lane_region/builtin_calls.rs"] mod builtin_calls;
pub(in crate::vm::compiled) use native_calls::{emit_float_extreme, emit_float_unary, emit_int_multiply};
#[path = "lane_region/stable.rs"]
mod stable;
#[path = "lane_region/polls.rs"]
mod polls;
#[path = "lane_region/function_names.rs"] mod function_names;
#[path = "lane_region/literals.rs"] mod literals;
#[path = "lane_region/allocations.rs"] mod allocations;
#[path = "lane_region/edges.rs"] mod edges;
#[path = "lane_region/demand.rs"] mod demand;
#[path = "lane_region/exits.rs"] mod exits;
#[cfg(all(test, feature = "aot"))] #[path = "lane_region/exit_tests.rs"] mod exit_tests;
#[cfg(all(test, feature = "aot"))] #[path = "lane_region/home_tests.rs"] mod home_tests;
#[path = "lane_region/catches.rs"] mod catches;
use catches::CallEdges;
#[path = "lane_region/store_domains.rs"] mod store_domains;
pub(in crate::vm) use store_domains::store_domains;
#[path = "lane_region/captures.rs"] mod captures;
#[path = "lane_region/compare_jumps.rs"] mod compare_jumps;
#[path = "lane_region/relocs.rs"] pub(super) mod relocs;
use relocs::RelocKind; pub(super) use relocs::{helper_symbol, numeric_symbol};
use captures::RootedRead;
#[path = "lane_region/constructions.rs"] mod constructions;
#[path = "lane_region/dense_elements.rs"] mod dense_elements;
#[path = "lane_region/dense_appends.rs"] mod dense_appends;
#[path = "lane_region/bindings.rs"] mod bindings;
#[path = "lane_region/frame.rs"] mod frame;
#[path = "lane_region/frame_fields.rs"] mod frame_fields;
#[path = "lane_region/frame_elements.rs"] mod frame_elements;
#[path = "lane_region/frame_stores.rs"] mod frame_stores;
#[path = "lane_region/field_plans.rs"] mod field_plans;
#[path = "lane_region/headers.rs"] mod headers;
#[path = "lane_region/hoisted_reads.rs"] mod hoisted_reads;
#[path = "lane_region/field_reads.rs"] mod field_reads;
#[path = "lane_region/number_domains.rs"] mod number_domains;
#[path = "lane_region/loop_arrays.rs"] mod loop_arrays;
#[path = "lane_region/call_frames.rs"] mod call_frames;
use call_frames::{CallSite, Leave, ReturnSite, Site};
#[path = "lane_region/call_outs.rs"] mod call_outs;
#[path = "lane_region/jumps.rs"] mod jumps;
#[path = "lane_region/call_rows.rs"] mod call_rows;
#[path = "lane_region/register_rows.rs"] mod register_rows;
#[path = "lane_region/element_rows.rs"] mod element_rows;
#[path = "lane_region/fixed_arrays.rs"] mod fixed_arrays;
#[cfg(test)] #[path = "lane_region/element_tests.rs"] mod element_tests;
use loop_arrays::{ArraySource, LoopArray};
#[cfg(feature = "aot")] #[path = "lane_region/bound_this.rs"] mod bound_this;
use frame::EmitFrame;
use frame_fields::{FieldFacts, HeldRead, StableHold};
use field_plans::{with_read_plan, LanePlans, LinkInput};
use crate::staged::property::{StoreArm, StoreJoin, StorePlan};
use polls::specialize_closed_polls;
#[cfg(test)]
use polls::retain_closed_polls;
pub(crate) use stable::StableFacts;
use crate::typed_ir::IntRange;

pub(super) const LANE_REGION_NAMESPACE: u32 = u32::from_be_bytes(*b"QLRG");
/// Inline scratch capacity for the small field-site sets typical of native units;
/// larger units spill normally, without a semantic or compilation limit.
const INLINE_FIELD_BINDINGS: usize = 4;
/// Common small effect frontiers stay inline; larger control-flow graphs spill normally.
const INLINE_EFFECT_EDGES: usize = 4;
/// Most blocks project only a few numbers; larger blocks spill without a limit.
const INLINE_NUMBER_PROJECTIONS: usize = 4;
/// A loop body usually indexes only a few arrays; more spill without a limit.
const INLINE_ARRAY_VIEWS: usize = 4;
/// A header and a short latch usually need only a few guarded inputs.
const INLINE_LOOP_INPUTS: usize = 4;
/// A jump's step proves a fact of one tested register, along one or both of its edges.
const INLINE_EDGE_REFINEMENTS: usize = 2;
/// Common native-entry failure status; a pure scalar miss restarts its call.
const NUMBER_ENTRY_MISS: i64 = 1;
/// The machine bytes an inlined instruction of a lane body costs at most, with its guards and
/// exits: what a unit's instruction budget (`lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS`) is
/// derived from its code-size budget by.
pub(super) const LANE_BODY_CODE_BYTES_PER_INSTRUCTION: usize = 128;
/// The machine bytes an IR instruction of a lane body costs at most, measured over the V8-v7
/// bodies (6.7 to 7.3), which a plan's size is judged by before it compiles.
pub(super) const LANE_BODY_CODE_BYTES_PER_IR: usize = 8;
/// The executable memory a lane body may take, a resource limit: the code and compile time a
/// hot loop's inlined call graph (Richards' ~900 instructions) costs. A larger body is planned
/// again with less inlined (`Vm::compile_lane_method`).
pub(super) const LANE_BODY_MAX_CODE_BYTES: usize = 115 * 1024;

/// Semantic proof and active machine view; tagged bits may also be current.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RegisterFact {
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

    fn canonical() -> Self {
        Self::admit(Fact::tagged(BindingTime::Location, Proof::CanonicalValue))
            .expect("canonical tagged value")
    }
}

struct Registers {
    tagged: Vec<Variable>,
    /// Each register's int32 and float64 views, declared at their first use: most registers
    /// never take one, and the SSA builder's tables grow with variables times blocks. A view
    /// read before any definition reads zero, as only a state that defined it reads it.
    int: Vec<Option<Variable>>,
    float: Vec<Option<Variable>>,
}

/// The compile-time state of the instruction being lowered.
#[derive(Clone)]
struct State {
    values: Vec<RegisterFact>,
    proofs: LocalProofs,
    /// What the jump just stepped proved of a register along each of its edges
    /// (`Frame::refine`), applied to the successors' states (`State::narrow`) once it branches.
    refinements: smallvec::SmallVec<[(crate::staged::Edge, Register, Type); INLINE_EDGE_REFINEMENTS]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FieldValue {
    /// Receiver home until its next definition (`None` is the fixed `this`).
    owner: Option<Register>,
    atom: Atom,
    value: ir::Value,
    ty: Type,
}

/// Available object and value proofs saved at control-flow edges. Register types
/// belong to the shared dataflow analysis and need no duplicate snapshots.
#[derive(Clone, Default)]
struct LocalProofs {
    /// Direct-object proofs by owner, until an effect that may change the object retires
    /// them: a register's, and in the final entry fixed `this`'s.
    headers: Vec<Option<ObjectHeader>>,
    /// The owners of `headers` whose proof a read made, which may name a function's record
    /// (`ObjectHeader::prove_readable`): a store proves them again as objects.
    readable: CompoundBitSet,
    /// A fixed receiver (`None`) or register's complete own-slot projection.
    layouts: BTreeMap<(Option<Register>, u32), CheckedObject>,
    /// Registers holding the receiver, whose facts the entry proved.
    receivers: BTreeSet<Register>,
    /// Immutable reads; receiver definitions and memory effects retire these facts.
    fields: smallvec::SmallVec<[FieldValue; INLINE_FIELD_BINDINGS]>,
    /// Pure ToInt32 results, separate from each source value's semantic type.
    int32_projections: smallvec::SmallVec<[(ir::Value, ir::Value); INLINE_NUMBER_PROJECTIONS]>,
    /// Dense array views by the tagged base they prove, until an instruction that could change an array's storage (`retains_array_views`).
    arrays: smallvec::SmallVec<[(ir::Value, super::DenseArrayView); INLINE_ARRAY_VIEWS]>,
    /// Whether this block read the code's invalidation word since the last helper that may change a Stable fact's subject
    /// (run JavaScript, write a prototype, name a method): every Stable fact the code folded holds until such a helper (`Lowering::hold_stable`).
    stable: bool,
    /// Whether this block proved the realm its global reads were compiled against active (`Lowering::hold_global`).
    realm: bool,
    /// Registers whose lane-frame slot holds their current tagged value (`Lowering::home_written`): a
    /// definition retires one, so a call writes back only what changed since the last write-back.
    homed: CompoundBitSet,
}

impl LocalProofs {
    fn set_readable(&mut self, owner: usize, readable: bool) {
        if readable { self.readable.insert(owner); } else { self.readable.remove(owner); }
    }

    /// An admitted existing own-slot write changes only this name. Retire it for every receiver, since distinct receiver values may alias.
    fn write_field(&mut self, atom: Atom) {
        self.fields.retain(|field| field.atom != atom);
    }

    /// Keeps the homes `other` also holds, for a state both arrivals reach.
    fn meet_homed(&mut self, other: &CompoundBitSet) {
        let dropped: Vec<usize> = self.homed.iter().filter(|register| !other.contains(*register)).collect();
        for register in dropped { self.homed.remove(register); }
    }
}

impl State {
    fn canonical(registers: usize) -> Self {
        Self { values: vec![RegisterFact::canonical(); registers], refinements: smallvec::SmallVec::new(), proofs: LocalProofs {
            headers: vec![None; registers + 1], readable: CompoundBitSet::with_capacity(registers + 1),
            layouts: BTreeMap::new(), receivers: BTreeSet::new(),
            fields: smallvec::SmallVec::new(), int32_projections: smallvec::SmallVec::new(), stable: false, realm: false,
            arrays: smallvec::SmallVec::new(), homed: CompoundBitSet::new() } }
    }

    /// The facts a jump proved along `edge` (`refinements`), each narrowing its register's type for
    /// the edge's successor.
    fn narrow(&mut self, edge: crate::staged::Edge, refinements: &[(crate::staged::Edge, Register, Type)]) {
        for &(_, register, ty) in refinements.iter().filter(|(on, ..)| *on == edge) {
            let fact = self.values[usize::from(register)];
            if fact.value.representation == Representation::Tagged && ty != fact.value.ty && ty.is_subtype(fact.value.ty) {
                self.values[usize::from(register)] = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(ty, Representation::Tagged) };
            }
        }
    }

    /// The `headers` index of an owner: a register, or fixed `this` (`None`).
    fn header_index(&self, owner: Option<Register>) -> usize {
        owner.map_or(self.values.len(), usize::from)
    }

    /// Native reentry retires shape/storage and function-body admission proofs;
    /// object records and language families stay.
    fn call_effect(&mut self) {
        self.proofs.fields.clear();
        self.proofs.int32_projections.clear();
        self.proofs.arrays.clear();
        self.proofs.headers.fill(None);
        self.proofs.readable.clear();
        self.proofs.layouts.clear();
        self.proofs.stable = false;
        for fact in &mut self.values {
            let unshaped = fact.value.ty.without_object_shape();
            if unshaped != fact.value.ty {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(unshaped, fact.value.representation) };
            } else if fact.value.ty.body_index().is_some() {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(Type::language(crate::ValueType::Function), fact.value.representation) };
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiverRecovery { Object, ObjectOrNullish }

impl ReceiverRecovery {
    fn for_body(body: &crate::bytecode::Function) -> Self {
        if body.instructions().all(|instruction| {
            !instruction.writes_current_this()
                && !instruction.op().is_load_this()
                && !instruction.stores_to_this()
                && (!instruction.op().is_get_field() || matches!(instruction.field_lookup(),
                    Some(FieldLookup::Atom { base, .. }) if base != crate::bytecode::FieldBase::THIS))
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
    this: Variable,
    /// Flat per-site immutable slot inputs; no per-operation probe.
    fields: smallvec::SmallVec<[(u16, FieldSlotBinding); INLINE_FIELD_BINDINGS]>,
    /// Function-bound numeric calls whose invariant closure was admitted here.
    calls: CompoundBitSet,
    /// Inlined frames whose invariant receiver needs no boxing on recovery.
    call_receivers: CompoundBitSet,
}

/// A lane body lowered for the code generator, which may run it on another thread.
pub(super) struct LoweredLane {
    pub(super) region: LoweredRegion,
    pub(super) clif: Option<String>,
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
}

#[derive(Clone, Copy)]
struct InvariantRead {
    pc: usize,
    /// The canonical caller home supplying a field receiver at loop entry.
    owner: Option<Register>,
    /// Whether a root-held binding it reads stays put: its loop stores no global object field.
    rooted: bool,
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
}

pub(super) struct LoweredRegion {
    pub(super) number_result: Option<NativeNumberResult>,
    pub(super) function: ir::Function,
    pub(super) native: usize,
    pub(super) fault_poll: bool,
    /// Whether a loop of the region hoists an array view, so a typed variant may differ.
    pub(super) hoists_arrays: bool,
    pub(super) stable: StableFacts,
}

/// A guard's failure block and the state it leaves from: the registers' facts, and which frame slots
/// already held their value (`LocalProofs::homed`), at the guard.
struct GuardExit {
    block: ir::Block,
    values: Vec<RegisterFact>,
    homed: CompoundBitSet,
    pc: u32,
    line: u32,
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

struct Lowering<'a, 'f> {
    builder: FunctionBuilder<'f>,
    program: &'a ResidualProgram,
    unit: &'a super::lane_unit::Unit,
    /// The function's parameter count: an entry guard on a parameter declines the region
    /// when a call's value does not match the compiled entry fact.
    params: usize,
    /// The lowered function, which an exception it raises names as its source (`Lowering::throw`).
    function_id: u32,
    helpers: LaneHelpers,
    /// The proved shapes and slots of each field cache site the unit reads or writes.
    fields: &'a FieldSites,
    /// An OSR region's field reads answered from the entry record, by unit PC.
    entry_fields: &'a EntryWords,
    /// The element reads observed to hold Numbers (`RegionSpecializer::number_reads`).
    observed: super::facts::Observed<'a>,
    /// The registers observed to hold int32s at entry (`RegionSpecializer::int_entries`).
    int_entries: &'a [(Register, IntRange)],
    /// The capture slots observed to hold int32s (`RegionSpecializer::int_captures`).
    int_captures: &'a [u16],
    /// The by-name loads that read the global scope (`Vm::lane_global_reads`).
    global_reads: &'a crate::vm::global_facts::GlobalReads,
    /// The VM's facts: its `length` name, whose read of a direct array is its record's
    /// (`array_length`), and whether an IsHTMLDDA object can exist (`emit_nullish_branch`).
    vm: VmFacts<'a>,
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
    /// The shared leave of a failed input proof (`LaneExit::Entry`), made at its first guard
    /// and filled with the other exits.
    entry_exit: Option<ir::Block>,
    /// The calls, by unit PC, that call a callee's compiled entry (`DirectCallee`).
    direct: &'a BTreeMap<usize, *const DirectCallee>,
    /// Per inlined frame, its callee's dense capture slots (values and count), fixed by the
    /// callee's identity, which its call guards; null where they are not dense.
    frame_captures: &'a [(usize, usize)],
    context: ir::Value,
    /// An entry-bound poll page, or the ordinary atomic termination flag.
    interrupt: ir::Value,
    fault_poll: bool,
    /// Callback completion edges can retire the native entry's interrupt ownership proof.
    reentries: smallvec::SmallVec<[ir::Block; INLINE_EFFECT_EDGES]>,
    polls: smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>,
    /// Already-conforming backedges meet before their common fault poll.
    poll_latches: super::SecondaryMap<ir::Block, Option<ir::Block>>,
    shared_headers: CompoundBitSet,
    payload: ir::Value,
    registers_base: ir::Value,
    /// The running body's receiver and capture words its caller passes (`entry_cell::capture_word`).
    entry_words: (ir::Value, ir::Value),
    registers: Registers,
    written: CompoundBitSet,
    /// Source operations already consumed by an exact, unobservable scalar pair.
    consumed: CompoundBitSet,
    blocks: Vec<Option<ir::Block>>,
    /// Per resume PC, its leave blocks, one per set of registers stored (`exits.rs`).
    exits: Vec<Vec<exits::ExitBlock>>,
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
    /// The calls that run inline as Math operations (`native_calls::intrinsic_calls`).
    intrinsics: native_calls::Intrinsics,
    hoisted: BTreeMap<usize, LoopRead>,
    /// Loop-invariant array bases whose views their loop's entry proves (`loop_arrays`).
    loop_arrays: Vec<LoopArray>,
    /// Whether no operation of the unit may move an array's backing or the array record table
    /// (`arrays_stay_put`), so the record table's base is read-only throughout.
    arrays_fixed: bool,
    /// Each hoisted base's tagged value and view, by register, across its loop's iterations.
    array_vars: BTreeMap<ArraySource, ArrayVariables>,
    views: ElementViews,
    /// Successful write-through stores defining a loop-header field's SSA variable.
    loop_fields: Vec<LoopField>,
    loop_stores: BTreeMap<usize, LoopStore>,
    /// The unit's integer ranges, computed once before lowering (`int_ranges::analyze`): the
    /// arithmetic that cannot overflow and the hoisted element accesses within their array.
    ranges: int_ranges::IntRanges,
    /// The registers whose int32 range the entry guards once and the ranges pass takes as
    /// the entry state (`Lowering::entering_ranges`).
    entering: Vec<(Register, IntRange)>,
    /// The PC being lowered, whose edges to a header enter its loop or close it.
    at: usize,
    /// Per block leader, its forward edges (`forward_edges`), and the states the lowered
    /// ones arrived with: a block they all reach keeps the local proofs they share.
    forward: Vec<Option<usize>>,
    arrived: BTreeMap<usize, Vec<LocalProofs>>,
    /// The block leaders an entry or a lowered edge enters: a block none does is not
    /// lowered, its code reached only past a leave.
    entered: CompoundBitSet,
    /// The block leaders sealed when lowered, which only edges lowered before enter
    /// (`edges::leader_edges`): a variable read in one resolves without a parameter.
    sealable: CompoundBitSet,
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
    catches: BTreeMap<usize, catches::Catch>,
    /// The block the call-outs' statuses of a leave in a callee, or a termination, reach with the
    /// status, made at the first and filled at the region's end (`call_outs`).
    call_out_stop: Option<ir::Block>,
}

enum Liveness<'a> {
    Unit(Option<std::borrow::Cow<'a, crate::compile::liveness::RegisterMasks>>),
    Frames {
        caller: Option<&'a crate::compile::liveness::RegisterMasks>,
        frames: Vec<Option<&'a crate::compile::liveness::RegisterMasks>>,
    },
}

/// How much of the unit must run natively: a prefix (a lane body, which leaves to the lane
/// anywhere), or all of it (a numeric region).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Coverage {
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
    pub(super) fn entry(self) -> usize {
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

/// How a run ends. Every kind runs the lane body ABI (`lane_body_signature`): it takes its
/// inputs in the register window and returns its status and result word in registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Completion {
    /// A status and a PC (`LaneExit`): a lane body, or a loop region.
    Body,
    /// Pure, restartable scalar entry: borrowed inputs in the window's first slots, proved
    /// Numbers once at entry, and one Number result word; a miss restarts the original call.
    Number,
}

impl Completion {
    /// Whether a run completes with a status and a PC (a lane body or a loop region).
    pub(super) fn status_and_pc(self) -> bool { self == Self::Body }
}

/// Which arrays a region's loops index in place (`LoopArray`): dense arrays' elements, or
/// typed arrays' (`Vm::lane_typed_array_elements`). A region assumes one, proved on each
/// loop's entering edge, so no access chooses between them; a static image publishes a
/// typed variant beside the dense one, which its dispatcher tries next at the header, and a
/// loop site compiles the one its entry found (`LoopTier::views`). A typed view carries the
/// kind the site observed, which the proof compares once and the accesses fold; `None` where
/// the entries showed several kinds (or none was observed), the accesses dispatching on the
/// record's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(in crate::vm) enum ElementViews {
    #[default]
    Dense,
    Typed(Option<crate::heap::TypedArrayKind>),
}

impl ElementViews {
    /// The views of arrays viewed both as `self` and as `other`: the same dense views, or typed
    /// views of the kind both name and of no kind where they differ; `None` for a mix.
    pub(in crate::vm) fn common(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Typed(left), Self::Typed(right)) => Some(Self::Typed(if left == right { left } else { None })),
            _ => (self == other).then_some(self),
        }
    }

    /// The kind a typed view's site observed.
    fn observed_kind(self) -> Option<crate::heap::TypedArrayKind> {
        match self {
            Self::Typed(kind) => kind,
            Self::Dense => None,
        }
    }
}

/// No heap, environment or guest-call effect may precede scalar-call recovery.
fn numeric_unit(program: &ResidualProgram, unit: &super::lane_unit::Unit) -> bool {
    unit.frames.is_empty() && numeric_instructions(program, &unit.instructions)
}

pub(super) fn restartable_numeric_function(program: &ResidualProgram, function: &crate::bytecode::Function,
    instructions: &[WideInstruction]) -> bool {
    super::super::js_compiled::function_candidate(function, program) && numeric_instructions(program, instructions)
}

fn numeric_instructions(program: &ResidualProgram, instructions: &[WideInstruction]) -> bool {
    instructions.iter().all(|instruction| {
        if instruction.writes_current_this()
            || (instruction.result_flags() != 0 && !instruction.returns_from_frame()) { return false; }
        match instruction.op() {
            opcode if opcode.is_load_const() => program.constants.get(instruction.constant_index())
                .and_then(super::facts::immediate).is_some(),
            _ if instruction.op().traits().has(crate::bytecode::Traits::SCALAR_REGION) => true,
            _ => instruction.op().js_denotation() == JsDenotation::NumericBinary
                && instruction.numeric_local_target().is_none(),
        }
    })
}

fn register_offset(register: Register) -> i32 {
    i32::from(register) * std::mem::size_of::<u64>() as i32
}

pub(super) fn int_tagged(builder: &mut FunctionBuilder<'_>, int: ir::Value) -> ir::Value {
    let payload = builder.ins().uextend(types::I64, int);
    builder.ins().bor_imm_u(payload, crate::value::ValueLayout::INT_TAG as i64)
}

pub(super) fn is_int_tagged(builder: &mut FunctionBuilder<'_>, bits: ir::Value) -> ir::Value {
    let shift = i64::from(crate::value::ValueLayout::TAG_SHIFT);
    let tag = builder.ins().ushr_imm_u(bits, shift);
    builder.ins().icmp_imm_u(IntCC::Equal, tag, (crate::value::ValueLayout::INT_TAG >> shift) as i64)
}

/// The registers an instruction writes, by its operand layouts.
pub(super) fn writes(instruction: WideInstruction) -> Vec<Register> {
    instruction.written_registers().collect()
}

impl<'a, 'f> Lowering<'a, 'f> {
    fn ins(&mut self) -> cranelift_frontend::FuncInstBuilder<'_, 'f> {
        self.builder.ins()
    }

    fn word_const(&mut self, bits: i64) -> ir::Value { self.int_const(types::I64, bits) }
    fn int_const(&mut self, ty: ir::Type, bits: i64) -> ir::Value { self.ins().iconst(ty, bits) }
    fn word_cmp_imm(&mut self, cc: IntCC, a: ir::Value, imm: i64) -> ir::Value { self.ins().icmp_imm_u(cc, a, imm) }
    /// The low 32 bits of a word.
    fn narrow_word(&mut self, word: ir::Value) -> ir::Value { self.ins().ireduce(types::I32, word) }
    /// A word to the lane's register file at `base` plus `offset` (alias region `Frame`).
    fn frame_store(&mut self, word: ir::Value, base: ir::Value, offset: i32) { self.ins().store(regions::frame(), word, base, offset); }

    fn tagged(&mut self, state: &mut State, register: Register) -> ir::Value {
        frame::register_tagged(&mut self.builder, &mut self.registers, state, register)
    }

    fn define(&mut self, state: &mut State, register: Register, value: ir::Value, fact: RegisterFact) {
        frame::register_define(&mut self.builder, &mut self.registers, &mut self.written, state,
            register, value, fact);
    }

    fn set_tagged(&mut self, state: &mut State, register: Register, bits: ir::Value) {
        self.define(state, register, bits, RegisterFact::definition(Type::ANY, Representation::Tagged));
    }

    /// The tagged Boolean a predicate `holds` selects.
    fn boolean(&mut self, holds: ir::Value) -> ir::Value {
        let (yes, no) = (self.ins().iconst(types::I64, Value::TRUE.to_bits() as i64), self.ins().iconst(types::I64, Value::FALSE.to_bits() as i64));
        self.ins().select(holds, yes, no)
    }

    fn set_boolean(&mut self, state: &mut State, register: Register, boolean: ir::Value) {
        self.define(state, register, boolean, RegisterFact::definition(Type::language(crate::ValueType::Boolean), Representation::Tagged));
    }

    fn set_int(&mut self, state: &mut State, register: Register, int: ir::Value) {
        self.define(state, register, int, RegisterFact::definition(Type::INT32, Representation::Int32));
    }

    fn set_number(&mut self, state: &mut State, register: Register, number: ir::Value) {
        self.define(state, register, number, RegisterFact::definition(Type::NUMBER, Representation::Float64));
    }

    /// Prove an exact int32 view once; a Number already in a float register
    /// needs no canonical encoding or tag test to establish that projection.
    fn int(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        frame::register_int_value(&mut self.builder, &mut self.registers, state, register, otherwise)
    }

    /// Prove Number only for a new untrusted value; later uses read its unboxed view.
    fn number(&mut self, state: &mut State, register: Register, otherwise: ir::Block) -> ir::Value {
        frame::register_number_value(&mut self.builder, &mut self.registers, state, register, otherwise)
    }

    fn canonicalize(&mut self, state: &mut State, pc: usize) {
        for register in 0..state.values.len() {
            if self.is_live(pc, register as Register) && !state.values[register].tagged.current() {
                self.tagged(state, register as Register);
            }
        }
    }

    /// `leave` naming the lowering line that decided the leave, which a diagnostics run reports
    /// (`JsStepDispatch::exit_line`).
    #[track_caller]
    fn exit(&mut self, state: &mut State, pc: u32) {
        if self.vm.diagnostics && self.completion != Completion::Number {
            let line = self.builder.ins().iconst(types::I64, i64::from(std::panic::Location::caller().line()));
            self.builder.ins().store(regions::dispatch(), line, self.payload, std::mem::offset_of!(JsStepDispatch, exit_line) as i32);
        }
        self.leave(state, pc);
    }

    /// Leave at `pc` with the interpreter's state: every written register back in the lane's file, but those it already holds.
    fn leave(&mut self, state: &mut State, pc: u32) {
        if self.completion == Completion::Number {
            self.return_at(0);
            return;
        }
        // A pure callee miss restarts its call. A nested marker names the
        // parent's call PC, so recovery keeps any already-performed parent effects.
        let mut pc = pc as usize;
        while let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc).copied() {
            if self.unit.frames[frame].recovery != super::lane_unit::CallRecovery::RestartCall { break; }
            pc = self.unit.markers[frame];
        }
        let stored = self.stored_at(state, pc);
        self.materialize_inlined(state, pc);
        let block = self.exit_block(pc, stored);
        self.ins().jump(block, &[]);
    }

    /// A guard's failure block: it leaves at `pc` from the state where the guard sits. It is
    /// filled after the region, since the current block is still open.
    #[track_caller]
    fn guard_exit(&mut self, state: &State, pc: u32) -> ir::Block {
        if self.proving_entry {
            let builder = &mut self.builder;
            return *self.entry_exit.get_or_insert_with(|| { let block = builder.create_block(); builder.set_cold_block(block); block });
        }
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.pending.push(GuardExit { block, values: state.values.clone(), homed: state.proofs.homed.clone(),
            pc, line: std::panic::Location::caller().line() });
        block
    }

    /// Lowers into `block` from here, sealed where every edge into it is lowered.
    fn switch_to(&mut self, block: ir::Block, sealed: bool) {
        self.builder.switch_to_block(block);
        if sealed { self.builder.seal_block(block); }
    }

    fn continue_at(&mut self, state: &mut State, pc: usize) {
        if !self.coverage.lowered(pc) {
            self.exit(state, pc as u32);
            return;
        }
        // A block before this one that no edge entered was not lowered.
        let lowered = pc > self.at || self.entered.contains(pc);
        match self.blocks.get(pc).copied().flatten().filter(|_| lowered) {
            // An edge the forward analysis did not see reaches a sealed block: decline.
            Some(_) if pc <= self.at && self.sealable.contains(pc) => self.valid = false,
            Some(block) => {
                self.entered.insert(pc);
                self.finishes |= pc <= self.at;
                if self.at < pc { self.hoist_loop_entry(state, pc); }
                self.conform(state, pc);
                if self.at < pc { self.arrive(state, pc); }
                self.ins().jump(block, &[]);
            }
            None => self.exit(state, pc as u32),
        }
    }

    /// Record `state` arriving at block leader `pc` by one of its forward edges.
    fn arrive(&mut self, state: &State, pc: usize) {
        if self.forward.get(pc).copied().flatten().is_some() {
            self.arrived.entry(pc).or_default().push(state.proofs.clone());
        }
    }

    /// The local proofs every forward edge into `pc` arrived with, as the same values, for
    /// a block no other edge enters: each was made in a block over all of them, so over this.
    fn keep_shared_proofs(&mut self, state: &mut State, pc: usize) {
        let Some(edges) = self.forward.get(pc).copied().flatten() else { return };
        let Some(arrived) = self.arrived.remove(&pc) else { return };
        let Some((first, rest)) = arrived.split_first().filter(|_| arrived.len() == edges) else { return };
        for (index, header) in state.proofs.headers.iter_mut().enumerate() {
            *header = first.headers[index].filter(|proof| rest.iter()
                .all(|other| other.headers[index] == Some(*proof)));
        }
        state.proofs.readable.clear();
        for arrived in &arrived {
            for owner in arrived.readable.iter() { state.proofs.readable.insert(owner); }
        }
        state.proofs.layouts = first.layouts.iter().filter(|(key, projection)| rest.iter()
            .all(|other| other.layouts.get(key) == Some(projection))).map(|(key, projection)| (*key, *projection)).collect();
        state.proofs.receivers = first.receivers.iter().filter(|register| rest.iter()
            .all(|other| other.receivers.contains(register))).copied().collect();
        state.proofs.fields = first.fields.iter().copied().filter(|field| rest.iter()
            .all(|other| other.fields.contains(field))).collect();
        state.proofs.int32_projections = first.int32_projections.iter().copied().filter(|projection| rest.iter()
            .all(|other| other.int32_projections.contains(projection))).collect();
        state.proofs.arrays = first.arrays.iter().copied().filter(|view| rest.iter()
            .all(|other| other.arrays.contains(view))).collect();
        state.proofs.stable = first.stable && rest.iter().all(|other| other.stable);
        state.proofs.realm = first.realm && rest.iter().all(|other| other.realm);
        // A block a rotated latch also enters has an edge `rest` does not hold: only a sealable block keeps the homes.
        if self.sealable.contains(pc) {
            state.proofs.homed.clone_from(&first.homed);
            for other in rest { state.proofs.meet_homed(&other.homed); }
        }
    }

    /// Conform the machine view and discharge the target's semantic requirements.
    fn conform(&mut self, state: &mut State, target: usize) {
        let Some(types) = self.facts.at(target).map(<[Type]>::to_vec) else {
            self.canonicalize(state, target);
            return;
        };
        for (index, ty) in types.iter().copied().enumerate() {
            let register = index as Register;
            if !self.is_live(target, register) { continue; }
            // A parameter's entry fact varies per call: a mismatched value declines the
            // region (the lane runs the header) instead of exiting native code.
            let saved = self.proving_entry;
            if target == self.coverage.entry() && index < self.params { self.proving_entry = true; }
            let proved = self.conform_register(state, target, register, ty);
            self.proving_entry = saved;
            if !proved { return; }
        }
    }

    /// One live register conformed to `target`'s fact `ty`; whether the edge still reaches it.
    fn conform_register(&mut self, state: &mut State, target: usize, register: Register, ty: Type) -> bool {
        let index = usize::from(register);
        let held = state.values[index].value.representation;
        match number_domains::target_representation(ty) {
            Representation::Int32 if held != Representation::Int32 => {
                let exit = self.guard_exit(state, target as u32);
                self.int(state, register, exit);
            }
            Representation::Float64 if held != Representation::Float64 => {
                let exit = self.guard_exit(state, target as u32);
                let number = self.number(state, register, exit);
                let variable = self.float_var(index);
                self.builder.def_var(variable, number);
            }
            Representation::Tagged => { self.tagged(state, register); }
            Representation::Int32 | Representation::Float64 => {}
        }
        self.conform_object(state, target, register, ty);
        // A fact this edge does not prove and cannot guard leaves at the target, exactly:
        // the edge loses native code, never the body (AGENTS.md, check once).
        if self.proves(state, register, ty, target) { return true; }
        let exit = self.guard_exit(state, target as u32);
        self.builder.ins().jump(exit, &[]);
        let unreached = self.builder.create_block();
        self.builder.switch_to_block(unreached);
        false
    }

    /// An object fact of the target the edge has not proved: the direct-object family by one
    /// guard, an exact shape guarded on this edge with the owner's header read once, so the
    /// edge keeps native code.
    fn conform_object(&mut self, state: &mut State, target: usize, register: Register, ty: Type) {
        let index = usize::from(register);
        if ty == Type::DIRECT_OBJECT_FAMILY && !state.values[index].value.ty.is_direct_object_family() {
            let exit = self.guard_exit(state, target as u32);
            let bits = self.tagged(state, register);
            let object = crate::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), bits);
            emit_js_guard(&mut self.builder, object, exit);
            self.refine_object_family(state, bits);
        }
        if let Some(shape) = ty.object_shape() && !state.values[index].value.ty.is_subtype(ty) {
            let exit = self.guard_exit(state, target as u32);
            let header = self.object_header(state, Some(register), false, exit);
            headers::prove_header_shape(&mut self.builder, state, Some(register), header.shape, shape, exit);
            state.values[index] = RegisterFact { tagged: state.values[index].tagged,
                ..RegisterFact::definition(Type::direct_object(shape), Representation::Tagged) };
        }
    }

    fn entry_state(&mut self, pc: usize, registers: usize) -> State {
        let mut state = State::canonical(registers);
        self.home_entry_registers(&mut state, pc);
        let Some(types) = self.facts.at(pc) else { return state };
        for (index, ty) in types.iter().copied().enumerate() {
            if !self.is_live(pc, index as Register) { continue; }
            let Some(fact) = self.entry_fact(pc, ty) else { self.valid = false; return state; };
            state.values[index] = fact;
        }
        state
    }

    /// The live caller registers a once-entered region entry loaded from their frame slots.
    fn home_entry_registers(&self, state: &mut State, pc: usize) {
        if pc == self.coverage.entry() && self.completion != Completion::Number
            && edges::entered_once(&self.unit.instructions, pc, |at| self.coverage.lowered(at)) {
            for register in (0..self.unit.caller_registers).filter(|register| self.is_live(pc, *register)) {
                state.proofs.homed.insert(usize::from(register));
            }
        }
    }

    /// The fact a register of type `ty` has at `pc`: guarded at the region entry, a definition after.
    fn entry_fact(&self, pc: usize, ty: Type) -> Option<RegisterFact> {
        let entry = pc == self.coverage.entry();
        RegisterFact::admit(Fact { ty, representation: number_domains::target_representation(ty),
            binding: if entry { BindingTime::Location } else { BindingTime::Dynamic },
            proof: if !entry { Proof::Definition }
                else if ty == Type::ANY { Proof::CanonicalValue } else { Proof::EntryGuard },
            projection: Projection::Exact,
        })
    }

    /// Whether unit `register` is live at unit `pc`. Without unit masks: the frame that owns
    /// it is in the chain of frames `pc` runs in, and its function's masks keep it live at
    /// the frame's own PC there (the PC itself for the innermost, its call's for each frame
    /// around it).
    fn is_live(&self, pc: usize, register: Register) -> bool {
        let (caller, frames) = match &self.live {
            Liveness::Unit(masks) => return masks.as_deref().is_none_or(|live| live.is_live(pc, register)),
            Liveness::Frames { caller, frames } => (caller, frames),
        };
        let (mut frame, mut at) = match self.unit.origins.get(pc).copied() {
            Some(super::lane_unit::Origin::Inlined { frame, pc }) => (Some(frame), pc),
            Some(super::lane_unit::Origin::Caller(pc)) => (None, pc),
            None => (None, self.unit.end),
        };
        while let Some(index) = frame {
            let inlined = self.unit.frames[index];
            if (inlined.base..inlined.base + inlined.registers).contains(&register) {
                return frames[index].is_none_or(|live| live.is_live(at as usize, register - inlined.base));
            }
            (frame, at) = (inlined.parent, inlined.call);
        }
        register < self.unit.caller_registers && caller.is_none_or(|live| live.is_live(at as usize, register))
    }

    fn operand_number(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        frame::operand_number(&mut self.builder, &mut self.registers, state, &self.program.constants,
            operand, otherwise)
    }

    /// An operand's int32 value, statically where it is a constant int32.
    fn operand_int(&mut self, state: &mut State, operand: Operand, otherwise: ir::Block) -> Option<ir::Value> {
        frame::operand_int(&mut self.builder, &mut self.registers, state, &self.program.constants,
            operand, otherwise)
    }

    /// An operand's tagged bits: its register's, or its immediate constant's.
    fn operand_tagged(&mut self, state: &mut State, operand: Operand) -> Option<ir::Value> {
        match operand.kind()? {
            OperandKind::Register => Some(self.tagged(state, operand.register_index()?)),
            OperandKind::Constant => {
                let (value, _) = self.constant_value(usize::from(operand.payload()))?;
                Some(self.identity_constant(value))
            }
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    fn operand_supported(&self, operand: Operand) -> bool {
        match operand.kind() {
            Some(OperandKind::Register) => operand.register_index().is_some_and(|r| r < self.unit.registers),
            Some(OperandKind::Constant) => self
                .program
                .constants
                .get(usize::from(operand.payload()))
                .and_then(super::facts::immediate)
                .is_some(),
            _ => false,
        }
    }
}

/// The PCs that begin basic blocks: the entry, every jump target and every instruction
/// after a jump.
pub(super) fn leaders(instructions: &[WideInstruction]) -> CompoundBitSet {
    let mut leaders = CompoundBitSet::with_capacity(instructions.len());
    if !instructions.is_empty() { leaders.insert(0); }
    for (pc, instruction) in instructions.iter().enumerate() {
        if instruction.op().control_flow_layout().jumps() {
            if let Some(target) = usize::try_from(instruction.jump_target()).ok().filter(|t| *t < instructions.len()) {
                leaders.insert(target);
            }
            if pc + 1 < instructions.len() { leaders.insert(pc + 1); }
        }
    }
    leaders
}

/// Whether `instruction` reads a field as an int32 (`Lowering::get_field`): a lookup with no
/// sink or completion whose last read's slots have only held int32 values, its value's tag
/// tested at the read.
fn int32_read(program: &ResidualProgram, fields: &FieldSites, instruction: WideInstruction) -> bool {
    if !instruction.op().class().eq(Class::GetField) || instruction.returns_from_frame() || instruction.writes_current_this() {
        return false;
    }
    let last = match instruction.field_lookup() {
        Some(FieldLookup::Atom { cache_site, .. }) => cache_site,
        Some(FieldLookup::Site(index)) => match program.field_sites.get(index) {
            Some(site) if site.sink.is_none() => site.second.unwrap_or(site.first).1,
            _ => return false,
        },
        None => return false,
    };
    site_loads(fields, last).is_some_and(|shapes| !shapes.is_empty()
        && shapes.iter().all(|field| field.ty == Type::INT32))
}

/// Whether `instruction` can change no heap value natively: anything else it would do (a
/// non-Number operand, a missed cache) leaves the body first.
fn reads_only(constants: &[crate::bytecode::Constant], instruction: WideInstruction) -> bool {
    match instruction.op() {
        _ if instruction.op().traits().has(Traits::FRAME_ONLY) => true,
        opcode if opcode.is_load_const() => constants.get(instruction.constant_index()).and_then(super::facts::immediate).is_some(),
        opcode if opcode.is_inc_dec() => instruction.numeric_local_target().is_none(),
        opcode if opcode.is_load_capture() => instruction.result_flags() == 0 && instruction.capture_depth() == 0,
        opcode if opcode.is_get_field() => !instruction.writes_current_this() && !instruction.returns_from_frame(),
        // ToNumeric of a Number is the Number; any other operand leaves (`Lowering::instruction`). A
        // dense element is read natively, any other leaving (`emit_dense_element_read`); a null or
        // undefined operand leaves before any effect (`staged::require_object_coercible`).
        opcode if opcode.is_to_numeric() || opcode.is_get_index() || opcode.is_require_object_coercible() => !instruction.returns_from_frame(),
        _ => instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary && !instruction.returns_from_frame(),
    }
}

/// Whether `instruction` is a store natively: of its atom, into a slot the object already
/// has, so it changes no shape, layout or other property; anything else leaves first.
fn stores_own(instruction: WideInstruction) -> bool {
    instruction.op().is_field_store() && !instruction.returns_from_frame()
}

/// Whether `instruction` is an element store natively: into a record array, whose elements
/// no named field read, capture or object shape observes (`Lowering::element_write`); anything
/// else leaves first.
fn stores_element(instruction: WideInstruction) -> bool {
    instruction.op().is_set_index() && !instruction.returns_from_frame()
}

/// Whether an array view (`LocalProofs::arrays`) survives `instruction`: it reads no heap
/// value but an element, or replaces one in place (`Lowering::element_write` retires the views
/// itself on its general path), so no array's storage changes.
fn retains_array_views(constants: &[crate::bytecode::Constant], instruction: WideInstruction) -> bool {
    match instruction.op() {
        opcode if opcode.is_get_field() || opcode.is_load_capture() => false,
        opcode if opcode.is_set_index() => true,
        _ => reads_only(constants, instruction),
    }
}

/// Whether a loop may hold `instruction` natively: a read, an own-field or element store, or a
/// call lowered as an intrinsic that changes no array (`native_calls::keeps_arrays`).
fn stays_native(constants: &[Constant], instruction: WideInstruction, intrinsic: bool) -> bool {
    intrinsic || reads_only(constants, instruction) || stores_own(instruction) || stores_element(instruction)
}

/// Loops with one external entry and no interior entry, whose native operations
/// only read or replace existing own fields and array elements. Misses leave before guest effects.
/// A sole native entry at the header can supply the same entry proofs.
fn local_loops(unit: &super::lane_unit::Unit, constants: &[crate::bytecode::Constant], start: usize,
    intrinsics: &native_calls::Intrinsics) -> Vec<(usize, usize)> {
    let instructions = &unit.instructions;
    let target = |instruction: &WideInstruction| instruction.op().control_flow_layout().jumps().then(|| instruction.jump_target() as usize);
    let falls = |instruction: &WideInstruction| instruction.op().control_flow_layout().falls_through();
    let mut loops = Vec::new();
    for (back, instruction) in instructions.iter().enumerate() {
        let Some(header) = target(instruction).filter(|header| *header <= back) else { continue };
        let entry_header = start == header;
        let body = &instructions[header..=back];
        let entries = usize::from(header > 0 && falls(&instructions[header - 1]))
            + instructions[..header].iter().filter(|candidate| target(candidate) == Some(header)).count();
        let side = instructions.iter().enumerate().any(|(pc, candidate)| !(header..=back).contains(&pc)
            && target(candidate).is_some_and(|to| (header + 1..=back).contains(&to) || (to == header && pc > back)));
        if (entries != 1 && !entry_header) || side
            || (header + 1..=back).contains(&start)
            || !body.iter().enumerate().all(|(offset, candidate)| stays_native(constants, *candidate, native_calls::keeps_arrays(intrinsics, header + offset))) { continue }
        loops.push((header, back));
    }
    loops
}

fn invariant_reads(program: &ResidualProgram, unit: &super::lane_unit::Unit, loops: &[(usize, usize)]) -> BTreeMap<usize, Vec<InvariantRead>> {
    let mut hoists = BTreeMap::new();
    let boundaries = leaders(&unit.instructions);
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let stored: BTreeSet<Atom> = body.iter().filter(|candidate| stores_own(**candidate))
            .map(|candidate| candidate.atom_index()).collect();
        // An element store may append, which changes its array's `length`.
        let resizes = body.iter().any(|candidate| stores_element(*candidate));
        let mut reads: Vec<InvariantRead> = body.iter().enumerate().filter_map(|(offset, candidate)| {
            let pc = header + offset;
            if candidate.op().is_load_capture()
                && matches!(unit.origins[pc], super::lane_unit::Origin::Caller(_)) {
                return Some(InvariantRead { pc, owner: None, rooted: stored.is_empty() });
            }
            if !candidate.op().is_get_field() { return None; }
            match candidate.field_lookup() {
            Some(FieldLookup::Atom { atom, base, .. }) if !stored.contains(&atom) && !(resizes && names_length(program, atom)) => {
                let owner = invariant_field_owner(unit, header, pc, base.register_index()?, &written, &boundaries)?;
                Some(InvariantRead { pc, owner: Some(owner), rooted: false })
            }
            _ => None,
        }}).collect();
        // An inlined capture belongs to the actual closure admitted at its
        // marker, not merely to a function body or an observed environment.
        // Its invariant callee read must run first on the entry edge.
        let captures: Vec<_> = body.iter().enumerate().filter_map(|(offset, instruction)| {
            let pc = header + offset;
            let super::lane_unit::Origin::Inlined { frame, .. } = unit.origins[pc] else { return None };
            (instruction.op().is_load_capture()
                && invariant_callee_read(unit, reads.iter().map(|read| read.pc), frame, &boundaries).is_some())
                .then_some(InvariantRead { pc, owner: None, rooted: false })
        }).collect();
        reads.extend(captures);
        if !reads.is_empty() { hoists.insert(header, reads); }
    }
    hoists
}

/// Resolve only block-local receiver copies to a caller home unchanged by the
/// whole loop. A branch or implicit frame initialization ends this proof.
fn invariant_field_owner(unit: &super::lane_unit::Unit, header: usize, pc: usize,
    mut owner: Register, written: &BTreeSet<Register>, boundaries: &CompoundBitSet) -> Option<Register> {
    for at in (header..pc).rev() {
        if owner < unit.caller_registers && !written.contains(&owner) { return Some(owner); }
        let instruction = unit.instructions[at];
        if boundaries.contains(at + 1) || unit.markers.contains(&at)
            || instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return None; }
        if writes(instruction).contains(&owner) {
            if !instruction.op().is_move() || instruction.result_register() != owner { return None; }
            owner = instruction.register_b();
        }
    }
    (owner < unit.caller_registers && !written.contains(&owner)).then_some(owner)
}

/// A monomorphic frame whose callee comes from an entry read through only
/// block-local copies. Other markers implicitly initialize scratch homes.
fn invariant_callee_read(unit: &super::lane_unit::Unit, reads: impl IntoIterator<Item = usize>, frame: usize,
    boundaries: &CompoundBitSet) -> Option<usize> {
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
                if !instruction.op().is_move() || instruction.result_register() != source { return false; }
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

fn successors(pc: usize, instruction: WideInstruction) -> [Option<usize>; 2] {
    let layout = instruction.op().control_flow_layout();
    [layout.falls_through().then_some(pc + 1), layout.jumps().then(|| instruction.jump_target() as usize)]
}

/// Forward must-alias analysis through inline argument copies and branch joins.
/// Only a single backedge is admitted, so every incoming state is complete when
/// visited. Unknown writes decline promotion; no runtime alias cache is needed.
fn loop_fields(program: &ResidualProgram, unit: &super::lane_unit::Unit,
    loops: &[(usize, usize)]) -> Vec<LoopField> {
    let mut fields: Vec<LoopField> = Vec::new();
    for &(header, back) in loops {
        if loops.iter().any(|&(other, end)| (other, end) != (header, back) && other <= back && header <= end)
            || (header..=back).any(|pc| successors(pc, unit.instructions[pc]).into_iter().flatten()
                .any(|target| target <= pc && (pc, target) != (back, header)))
            || unit.markers.iter().zip(&unit.frames).any(|(marker, frame)|
                (header..=back).contains(marker) && frame.alternative.is_some()) { continue; }
        let mut written: CompoundBitSet = CompoundBitSet::with_capacity(usize::from(unit.registers));
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
            if instruction.op().is_get_field() && instruction.result_flags() == 0
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
            let copied = (instruction.op().is_move()).then(|| aliases[usize::from(instruction.register_b())]).flatten();
            for register in writes(instruction) { aliases[usize::from(register)] = None; }
            if instruction.op().is_move() { aliases[usize::from(instruction.result_register())] = copied; }
            let targets = successors(pc, instruction).map(|target| target.filter(|target| *target > pc && *target <= back));
            let mut targets = targets.into_iter().flatten().peekable();
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
pub(super) struct RegionPlan {
    pub(super) coverage: Coverage,
    pub(super) completion: Completion,
    pub(super) views: ElementViews,
}

/// Futamura's first projection for one region: the same `lower` a lane body, a static typed
/// region and a numeric region run, parameterized by its `RegionPlan`. AOT and JIT both call
/// this specializer; only the plan and the `VmFacts` division differ.
/// No entry-snapshot field reads: a lane body or a static typed region reads its fields
/// from the registers and caches, not from an OSR entry record.
pub(super) static NO_ENTRY_FIELDS: [Option<u32>; 0] = [];

/// The proved loads of each field-cache site, flat and indexed by the site (`site_loads`).
pub(super) type FieldSites = [Option<Vec<NativeFieldLoad>>];
/// The entry-record word of each unit PC whose field read the record answers, flat and
/// indexed by the PC.
pub(super) type EntryWords = [Option<u32>];

/// The loads of field-cache `site`, if it has any.
pub(super) fn site_loads(fields: &FieldSites, site: u16) -> Option<&Vec<NativeFieldLoad>> {
    fields.get(usize::from(site))?.as_ref()
}

/// The flat table holding each `(key, value)` at its key, sized to the largest key.
pub(super) fn flat_table<T>(entries: impl IntoIterator<Item = (usize, T)>) -> Vec<Option<T>> {
    let entries: Vec<(usize, T)> = entries.into_iter().collect();
    let mut table: Vec<Option<T>> = Vec::new();
    table.resize_with(entries.iter().map(|(key, _)| key + 1).max().unwrap_or(0), || None);
    for (key, value) in entries { table[key] = Some(value); }
    table
}

pub(super) struct RegionSpecializer<'p> {
    pub(super) function_id: u32,
    pub(super) unit: &'p super::lane_unit::Unit,
    pub(super) helpers: LaneHelpers,
    pub(super) fields: &'p FieldSites,
    pub(super) global_reads: &'p crate::vm::global_facts::GlobalReads,
    pub(super) facts: VmFacts<'p>,
    pub(super) generic: &'p [usize],
    pub(super) direct: &'p BTreeMap<usize, *const DirectCallee>,
    pub(super) frame_captures: &'p [(usize, usize)],
    pub(super) isa: &'p dyn cranelift_codegen::isa::TargetIsa,
    /// An OSR region's field reads answered from the entry record, by unit PC and entry
    /// word (`osr_loop::field_word`).
    pub(super) entry_fields: &'p EntryWords,
    /// The unit PCs of element reads observed to hold Numbers (sorted): each leaves where its
    /// element is not one, and its result is typed a Number.
    pub(super) number_reads: &'p [super::facts::ObservedRead],
    /// The registers whose values at entry were int32s: candidates for an int32 entry contract
    /// (`refine_inductive_inputs`), kept where every definition in the loop yields an int32.
    pub(super) int_entries: &'p [(Register, IntRange)],
    /// The capture slots that held typed arrays at compilation (`Observation::typed_captures`).
    pub(super) typed_captures: &'p [u16],
    /// The capture slots that held int32s at compilation (`Observation::int_captures`).
    pub(super) int_captures: &'p [u16],
    pub(super) plan: RegionPlan,
}

impl Specializer for RegionSpecializer<'_> {
    type Residual = LoweredRegion;

    fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String> {
        lower(program, self.function_id, self.unit, self.helpers, self.fields,
            self.global_reads, self.facts, self.generic, self.direct, self.frame_captures,
            self.isa, self.entry_fields, self.number_reads, self.int_entries, self.typed_captures, self.int_captures, self.plan.coverage, self.plan.completion, self.plan.views)
            .ok_or_else(|| format!("function {} does not lower under {:?}", self.function_id, self.plan))
    }
}

/// Lowers `unit` (`lane_unit::unit`) as a lane body, whose field cache sites `fields`
/// gives the proved shapes and slots of; `None` where it does not lower.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_body(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &crate::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[(usize, usize)], entry: Option<usize>,
    number_reads: &[super::facts::ObservedRead],
) -> Option<LoweredLane> {
    let isa = super::jit_backend::target_isa()?;
    let region = RegionSpecializer { function_id, unit, helpers, fields, global_reads, facts: vm,
        generic, direct, frame_captures, isa: &*isa, entry_fields: &NO_ENTRY_FIELDS, number_reads, int_entries: &[], typed_captures: &[], int_captures: &[],
        plan: RegionPlan { coverage: entry.map_or(Coverage::Prefix, Coverage::PrefixFrom),
            completion: Completion::Body, views: ElementViews::Dense } }
        .specialize(program).ok()?;
    super::residuals::report_clif(function_id, &region.function);
    let clif = native_dump_directory().map(|_| region.function.to_string());
    Some(LoweredLane { region, clif })
}

/// The position a failed entry contract takes in a lane body's leave set
/// (`LaneExit::Entry`): past every instruction, so it names none of them. A plan with it
/// proves no input contract.
pub(super) fn entry_leave(unit: &super::lane_unit::Unit) -> usize {
    unit.instructions.len()
}

/// Build the typed residual and validate every value/CFG edge before publication.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower(
    program: &ResidualProgram,
    function_id: u32,
    unit: &super::lane_unit::Unit,
    helpers: LaneHelpers,
    fields: &FieldSites,
    global_reads: &crate::vm::global_facts::GlobalReads,
    vm: VmFacts<'_>,
    generic: &[usize],
    direct: &BTreeMap<usize, *const DirectCallee>,
    frame_captures: &[(usize, usize)],
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    entry_fields: &EntryWords,
    number_reads: &[super::facts::ObservedRead],
    int_entries: &[(Register, IntRange)],
    typed_captures: &[u16],
    int_captures: &[u16],
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
    crate::vm::dispatch_fast::lane_body_stats::lowering_function(function_id);
    let function = program.function_get(function_id as usize)?;
    let numeric = completion == Completion::Number;
    if numeric && (!super::super::js_compiled::function_candidate(function, program)
        || !numeric_unit(program, unit) || function.params > unit.caller_registers) { return None; }
    if fields.iter().flatten().flatten().any(|field| !field.valid()) { return None; }
    if unit.instructions.is_empty() || unit.instructions.len() > MAX_NATIVE_RECORDS {
        return None;
    }
    let signature = lane_body_signature(isa.pointer_type(), isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(UserFuncName::user(LANE_REGION_NAMESPACE, function_id), signature);
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    let parameters = builder.block_params(entry_block).to_vec();
    let [context, entry_this, entry_captures, registers_base] = parameters.as_slice() else { return None };
    let (context, entry_this, entry_captures, registers_base) = (*context, *entry_this, *entry_captures, *registers_base);
    let payload = if numeric { builder.ins().iconst(types::I64, 0) } else { builder.ins().load(
        types::I64,
        regions::dispatch(),
        context,
        std::mem::offset_of!(CallContext, adapter_payload) as i32,
    ) };
    let fault_poll = match coverage { Coverage::Loop { poll, .. } => poll, _ => true }
        && super::FAULT_POLL_SUPPORTED && unit.instructions.iter().enumerate().any(|(pc, instruction)|
            instruction.op().control_flow_layout().jumps()
                && instruction.jump_target() as usize <= pc);
    let mut seen_headers: CompoundBitSet = CompoundBitSet::with_capacity(unit.instructions.len());
    let mut shared_headers = CompoundBitSet::with_capacity(unit.instructions.len());
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if !instruction.op().is_jump() { continue; }
        let target = instruction.jump_target() as usize;
        if target > pc { continue; }
        if seen_headers.contains(target) { shared_headers.insert(target); }
        seen_headers.insert(target);
    }
    let interrupt = super::native_interrupt_pointer(&mut builder, context, fault_poll);
    #[cfg(feature = "aot")]
    let helpers = if let LaneHelpers::Linked(symbol, narrow_table) = helpers {
        let signature = builder.import_signature(native_lane_helpers_signature(isa));
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            super::native_object::NATIVE_FUNCTION_NAMESPACE, symbol));
        let target = builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name),
            signature, colocated: false, patchable: false });
        let call = builder.ins().call(target, &[]);
        LaneHelpers::Bound(builder.inst_results(call)[0], narrow_table)
    } else { helpers };
    let count = usize::from(unit.registers);
    let live = if unit.registers > crate::compile::liveness::ROOT_MAP_REGISTERS {
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
        float: vec![None; count],
    };
    // An alternative of a call's switch starts a block: a callee other than the one before
    // it branches there (`Lowering::inline_call`).
    let mut leaders = leaders(&unit.instructions);
    let entry_pc = coverage.entry();
    if !matches!(unit.origins.get(entry_pc), Some(super::lane_unit::Origin::Caller(_))) { return None; }
    leaders.insert(entry_pc);
    for alternative in unit.frames.iter().filter_map(|frame| frame.alternative) {
        if alternative < unit.instructions.len() { leaders.insert(alternative); }
    }
    // A handler's block is entered by the throws it catches, edges no jump names.
    let catches = catches::catch_edges(program, function, unit, coverage, completion);
    for catch in catches.values() { leaders.insert(catch.target); }
    let blocks: Vec<_> = (0..unit.instructions.len()).map(|pc| leaders.contains(pc).then(|| builder.create_block())).collect();
    let (leave, stop) = (builder.create_block(), builder.create_block());
    builder.append_block_param(leave, types::I32);
    builder.append_block_param(leave, types::I64);
    builder.append_block_param(stop, types::I64);
    builder.set_cold_block(leave);
    builder.set_cold_block(stop);
    let observed = super::facts::Observed(number_reads);
    let mut lowering = Lowering {
        builder,
        program,
        unit,
        params: usize::from(function.params),
        function_id,
        helpers,
        fields,
        entry_fields,
        observed,
        int_entries,
        int_captures,
        interrupt,
        fault_poll,
        reentries: smallvec::SmallVec::new(),
        polls: smallvec::SmallVec::new(),
        poll_latches: super::SecondaryMap::new(),
        shared_headers,
        global_reads,
        vm,
        stable: Default::default(),
        generic,
        proving_entry: false, left_here: false, held_view: None, entry_exit: None,
        direct,
        frame_captures,
        context,
        payload,
        registers_base,
        entry_words: (entry_this, entry_captures),
        registers,
        written: CompoundBitSet::with_capacity(count),
        consumed: CompoundBitSet::with_capacity(unit.instructions.len()),
        blocks,
        exits: std::iter::repeat_with(Vec::new).take(unit.instructions.len() + 1).collect(),
        demands: demand::analyze(program, unit, &leaders, &live),
        entry: None,
        native: 0,
        backedge: false,
        pending: Vec::new(),
        call_out_stop: None,
        // A static typed region's calls are guarded leaves (`guarded_call_instruction`); here any call-like one is native.
        native_calls: completion.status_and_pc()
            && unit.instructions.iter().any(|instruction| instruction.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Call),
        call_budget: None,
        facts: super::facts::Facts::default(),
        live,
        valid: true,
        coverage,
        completion,
        number_returns: smallvec::SmallVec::new(),
        hoists: BTreeMap::new(),
        intrinsics: Vec::new(),
        hoisted: BTreeMap::new(),
        loop_arrays: Vec::new(), arrays_fixed: false,
        array_vars: BTreeMap::new(),
        views,
        loop_fields: Vec::new(),
        loop_stores: BTreeMap::new(),
        ranges: int_ranges::IntRanges::new(0),
        entering: Vec::new(),
        at: 0,
        forward: Vec::new(), sealable: CompoundBitSet::new(),
        arrived: BTreeMap::new(),
        entered: CompoundBitSet::with_capacity(unit.instructions.len()),
        finishes: false,
        leave,
        stop,
        catches,
    };
    let instructions = unit.instructions.clone();
    for instruction in &instructions {
        for register in writes(*instruction).into_iter().map(usize::from).filter(|r| *r < count) {
            lowering.written.insert(register);
        }
    }
    let mut initial = vec![Type::ANY; count]; if numeric { initial[..usize::from(function.params)].fill(Type::NUMBER); }
    let heap = literals::heap_primitive_pcs(entry_pc, &initial, &instructions, &leaders, &program.constants, lowering.native_calls);
    for instruction in instructions.iter().enumerate().filter(|(pc, _)| !generic.contains(&entry_leave(unit)) && !heap.contains(*pc)).map(|(_, i)| i) {
        let (operands, required) = match instruction.op() {
            opcode if opcode.is_jump_binary_false() && nullish_comparison(&program.constants, *instruction, |_| Type::ANY).is_some() => continue,
            opcode if opcode.is_jump_binary_false() => {
                let operands = [instruction.operand_b(), instruction.operand_c()];
                if super::operations::NumberRelation::from_operator(instruction.binary_operator_field()).is_none()
                    || super::operations::equality_operator(instruction.binary_operator_field()).is_some()
                    && !operands.iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
                        && program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate)
                            .and_then(Value::as_number).is_some()) { continue; }
                let floating = operands.iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
                    && super::facts::operand_type(&program.constants, *operand, |_| Type::ANY) == Type::NUMBER);
                (operands, if floating { Type::NUMBER } else { Type::INT32 })
            }
            _ if instruction.op().js_denotation() == JsDenotation::NumericBinary => {
                let operator = instruction.binary_operator();
                let required = if super::operations::NumberBitwise::from_js_instruction(instruction.op(), operator).is_some() { Type::INT32 }
                    else if super::operations::NumberArithmetic::from_js_instruction(instruction.op(), operator).is_some() { Type::NUMBER }
                    else { continue };
                ([instruction.operand_b(), instruction.operand_c()], required)
            }
            _ => continue,
        };
        if operands.iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
            && program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate)
                .and_then(Value::as_number).is_none()) { continue; }
        for register in operands.into_iter().filter_map(Operand::register_index)
            .filter(|r| (function.parameter_registers.is_some_and(|base|
                *r >= base && *r - base < function.params)
                || function.local_registers.iter().any(|entry|
                    entry.local < function.params && entry.register == *r))
                && !numeric && !lowering.written.contains(usize::from(*r))) {
            let current = &mut initial[usize::from(register)];
            if *current != Type::INT32 { *current = required; }
        }
    }
    let own_shape = |pc: usize| {
        let instruction = instructions[pc];
        if !instruction.op().is_get_field() || instruction.result_flags() != 0 { return None; }
        let FieldLookup::Atom { base, cache_site, .. } = instruction.field_lookup()? else { return None };
        let [field] = site_loads(fields, cache_site)?.as_slice() else { return None };
        field.own_direct().then_some((base.register_index()?, field.shape))
    };
    lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &[], observed);
    lowering.intrinsics = native_calls::intrinsic_calls(unit, global_reads, |pc| direct.contains_key(&pc));
    let loops = local_loops(unit, &program.constants, entry_pc, &lowering.intrinsics);
    // One entry query of the call depth replaces each binding's helper with its cached
    // target probe (`cached_call_target`). A completed call leaves the depth it found.
    if !numeric && unit.frames.iter().any(|frame| frame.target.callee == CalleeBinding::Function) {
        let helper = lowering.helpers.address(&mut lowering.builder, LaneHelper::CallBudget);
        lowering.call_budget = Some(emit_lane_helper_call(&mut lowering.builder, lowering.payload, helper, &[]));
    }
    if coverage.selected_entry() {
        lowering.written = lowering.facts.written(&instructions, &writes, count);
        // Backedge definitions choose a numeric entry signature. Conformance
        // proves the external input once; every subsequent edge carries the
        // same verified SSA view. This is an admission requirement, not a fact
        // assumed about the source program or a TypeScript annotation.
        let mut refined = false;
        for (input, carried) in initial.iter_mut().take(usize::from(unit.caller_registers)).zip(lowering.facts.backedge()) {
            if matches!(*carried, Type::INT32 | Type::NUMBER) && *input != *carried {
                *input = *carried;
                refined = true;
            }
        }
        // An integer induction value can compare against an invariant integer
        // input directly. This selects a checked entry contract; a fractional
        // or wider bound resumes the original header with its value unchanged.
        let instruction = instructions[entry_pc];
        if !lowering.facts.backedge().is_empty() && instruction.op().is_jump_binary_false()
            && matches!(super::operations::NumberRelation::from_operator(instruction.binary_operator_field()),
                    Some(super::operations::NumberRelation::LessThan | super::operations::NumberRelation::LessEqual
                        | super::operations::NumberRelation::GreaterThan | super::operations::NumberRelation::GreaterEqual)) {
            let operands = [instruction.operand_b(), instruction.operand_c()];
            if operands.iter().any(|operand| super::facts::operand_type(&program.constants, *operand,
                |register| lowering.facts.backedge().get(usize::from(register)).copied().unwrap_or(Type::ANY)) == Type::INT32) {
                for register in operands.into_iter().filter_map(Operand::register_index)
                    .filter(|register| *register < unit.caller_registers && !lowering.written.contains(usize::from(*register))) {
                    if initial[usize::from(register)] == Type::NUMBER {
                        initial[usize::from(register)] = Type::INT32;
                        refined = true;
                    }
                }
            }
        }
        if refined {
            lowering.facts = super::facts::analyze(entry_pc, &initial, &instructions, &leaders, &program.constants, &writes, &own_shape, lowering.native_calls, &[], observed);
        }
        lowering.refine_inductive_inputs(&mut initial, &instructions, &leaders, &loops, &own_shape);
    }
    lowering.refine_loop_inputs(entry_pc, &initial, &instructions, &leaders, &loops, &own_shape);
    lowering.require_object_latches(&instructions, &leaders, &loops);
    for &pc in generic { lowering.facts.widen(pc); }
    if !numeric { lowering.entry = Some(lowering.entry_facts(&instructions, &loops, &leaders)); }
    lowering.hoists = invariant_reads(program, unit, &loops);
    lowering.loop_arrays = loop_arrays::loop_arrays(unit, &loops, &leaders,
        |header, at| lowering.hoists.get(&header).is_some_and(|reads| reads.iter().any(|read|
            read.pc == at && read.owner.is_none() && instructions[at].op().is_load_capture())),
        lowering.views, typed_captures);
    lowering.entering = lowering.entering_ranges(entry_pc);
    lowering.ranges = int_ranges::analyze(program, unit, &lowering.facts, entry_pc, &lowering.entering, generic, &|pc| lowering.dense_loop_access(pc));
    lowering.arrays_fixed = lowering.arrays_stay_put(&leaders, &writes);
    lowering.loop_fields = loop_fields(program, unit, &loops);
    for field in &lowering.loop_fields {
        lowering.hoists.entry(field.header).or_default().push(InvariantRead { pc: field.reads[0], owner: Some(field.owner), rooted: false });
    }
    // Field admissions establish the actual counterpart types used to select
    // capture views. Callee reads/bindings must precede their inlined captures.
    for reads in lowering.hoists.values_mut() {
        reads.sort_by_key(|read| (instructions[read.pc].op().is_load_capture(),
            matches!(unit.origins[read.pc], super::lane_unit::Origin::Inlined { .. })));
    }
    (lowering.forward, lowering.sealable) = edges::leader_edges(&instructions, &leaders, entry_pc);
    // No forward edge of the bytecode's enters a handler's block: its proofs are none, and it
    // seals once the region ends.
    for catch in lowering.catches.values() {
        lowering.forward[catch.target] = None;
        lowering.sealable.remove(catch.target);
    }
    lowering.entered.insert(entry_pc);
    lowering.stable.start = entry_pc;
    {
        let pc = entry_pc;
        let block = lowering.builder.create_block();
        lowering.builder.ins().jump(block, &[]);
        lowering.builder.switch_to_block(block);
        if !matches!(lowering.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let undefined = lowering.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
        for register in 0..count {
            // A numeric entry's window holds its parameters alone.
            let in_window = if numeric { register < usize::from(function.params) }
                else { register < usize::from(lowering.unit.caller_registers) && lowering.is_live(pc, register as Register) };
            let bits = if in_window {
                lowering.builder.ins().load(types::I64, regions::frame(), registers_base, register_offset(register as Register))
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
            let admitted = lowering.ins().icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, budget, depth);
            let miss = lowering.guard_exit(&state, pc as u32);
            emit_js_guard(&mut lowering.builder, admitted, miss);
        }
        if numeric {
            // Each input is proved a Number once, here; later uses read its unboxed view.
            for register in 0..function.params {
                let miss = lowering.guard_exit(&state, pc as u32);
                lowering.number(&mut state, register, miss);
            }
        }
        if fault_poll {
            // Publication lends a resumable page for this entire no-GC entry.
            // If unavailable, leave before any guest effect through ordinary recovery.
            let miss = lowering.guard_exit(&state, pc as u32);
            let available = lowering.builder.ins().icmp_imm_u(IntCC::NotEqual, interrupt, 0);
            emit_js_guard(&mut lowering.builder, available, miss);
        }
        lowering.entry_conform(&mut state, pc, coverage);
        // A region whose sole entry is this loop header has no earlier source
        // edge on which to hoist its invariant reads. Admit them on the native
        // entry edge, before the header and every possible loop effect. A failed
        // entry proof declines the region (the lane runs the header itself), not
        // an exit: the old lowering's entry admission declines the same arrays.
        // A bounded range reads its fields at their sites; hoisting them to a shared entry
        // guard would make every call leave at the entry.
        lowering.proving_entry = true;
        lowering.hoist_loop_entry(&mut state, pc);
        lowering.proving_entry = false;
        lowering.builder.ins().jump(target, &[]);
    }
    let mut state = State::canonical(count);
    let mut reachable = false;
    for (pc, instruction) in instructions.iter().copied().enumerate() {
        if !coverage.lowered(pc) { continue; }
        if let Some(block) = lowering.blocks[pc] {
            if reachable {
                lowering.hoist_loop_entry(&mut state, pc);
                lowering.conform(&mut state, pc);
                lowering.arrive(&state, pc);
                lowering.entered.insert(pc);
                lowering.builder.ins().jump(block, &[]);
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
            lowering.keep_shared_proofs(&mut state, pc);
            reachable = true;
        }
        if !reachable {
            continue;
        }
        lowering.at = pc;
        reachable = lowering.instruction(&mut state, pc, instruction);
    }
    if reachable {
        lowering.exit(&mut state, coverage.exit_pc(instructions.len()) as u32);
    }
    for (header, latch) in std::mem::take(&mut lowering.poll_latches).iter() {
        if let Some(latch) = latch {
            lowering.builder.switch_to_block(*latch);
            lowering.emit_fault_poll();
            lowering.builder.ins().jump(header, &[]);
        }
    }
    // A prefix pays for its entry with a loop, or with the calls it runs inlined.
    if matches!(coverage, Coverage::PrefixFrom(_)) && !lowering.backedge && unit.frames.is_empty() { return None; }
    for GuardExit { block, values, homed, pc, line } in lowering.take_leaves() {
        lowering.switch_to(block, true);
        let mut leaving = State { values, proofs: LocalProofs { homed, ..LocalProofs::default() }, refinements: smallvec::SmallVec::new() };
        if lowering.vm.diagnostics {
            let line = lowering.builder.ins().iconst(types::I64, i64::from(line));
            lowering.builder.ins().store(regions::dispatch(), line, lowering.payload, std::mem::offset_of!(JsStepDispatch, exit_line) as i32);
        }
        lowering.leave(&mut leaving, pc);
    }
    lowering.fill_call_out_stop();
    lowering.fill_exits();
    lowering.builder.switch_to_block(stop);
    let word = lowering.builder.block_params(stop)[0];
    let status = lowering.builder.ins().iconst(types::I32, i64::from(NATIVE_JS_STOPPED));
    lowering.leave_with(status, Some(word));
    lowering.builder.switch_to_block(leave);
    let results = lowering.builder.block_params(leave).to_vec();
    lowering.builder.ins().return_(&results);
    // A lane body is a whole function body and must complete; so must an inlined unit, whose
    // callees rejoin it. A loop or range over the function's own code completes by its exits
    // and backedge, which the plan emits where the interpreter resumes.
    let must_finish = match coverage {
        Coverage::Prefix | Coverage::PrefixFrom(_) | Coverage::Full => true,
        Coverage::Loop { .. } => !unit.frames.is_empty(),
    };
    if !lowering.valid || (completion.status_and_pc() && must_finish && !lowering.finishes) {
        return None;
    }
    let number_result = (completion == Completion::Number).then(|| lowering.finish_number_returns());
    let native_count = lowering.native;
    let hoists_arrays = !lowering.loop_arrays.is_empty();
    let mut polls = std::mem::take(&mut lowering.polls);
    let reentries = std::mem::take(&mut lowering.reentries);
    let helpers = lowering.helpers;
    let stable = lowering.stable.take();
    lowering.builder.seal_all_blocks();
    lowering.builder.finalize(isa.frontend_config());
    if fault_poll { specialize_closed_polls(&mut native, &reentries, &mut polls, helpers, context, isa); }
    Some(LoweredRegion { function: native, native: native_count, fault_poll, number_result, hoists_arrays, stable })
}

impl Lowering<'_, '_> {
    /// A bitwise operand's ToInt32 (`frame::operand_to_int32`).
    fn operand_to_int32(&mut self, state: &mut State, operand: Operand, exit: ir::Block) -> Option<ir::Value> {
        frame::operand_to_int32(&mut self.builder, &mut self.registers, state, &self.program.constants, operand, exit)
    }

    /// Whether only the result's ToInt32 is read of `register`, written at `pc`: the next
    /// instruction is its one reader, a bitwise operator, which applies ToInt32 to its operands.
    /// A sum's or difference's wrapping int32 result then serves for int32 operands, and
    /// ToInt32 of its exact double for any others. Not through a further sum: ToInt32 does not
    /// commute with adding a non-integer (trunc(-3 + 0.5) differs from trunc(-3 + 2^32 + 0.5)
    /// mod 2^32), so V8 truncates chains only of int32 inputs.
    fn truncated(&self, pc: usize, register: Register) -> bool {
        let (at, register) = self.reader(pc + 1, register);
        let Some(next) = self.unit.instructions.get(at).copied() else { return false };
        if !next.op().is_binary() || next.returns_from_frame() || next.writes_current_this() { return false; }
        let reads = [next.operand_b(), next.operand_c()].iter().filter(|operand| operand.register_index() == Some(register)).count();
        if reads == 0 || !(next.result_register() == register || !self.is_live(at + 1, register)) { return false; }
        let operator = next.binary_operator();
        if super::operations::NumberBitwise::from_js_instruction(next.op(), operator)
            .is_some_and(|bitwise| bitwise != super::operations::NumberBitwise::ShiftRightUnsigned) {
            return true;
        }
        false
    }

    /// Where `register` at `at` is next read, past an inlined completion's move and jump (`lane_unit`).
    fn reader(&self, at: usize, register: Register) -> (usize, Register) {
        let instruction = |at: usize| self.unit.instructions.get(at).copied();
        let (at, register) = match instruction(at) {
            Some(moved) if moved.op().copies_register() && moved.register_b() == register && !self.is_live(at + 1, register) => (at + 1, moved.result_register()),
            _ => (at, register),
        };
        match instruction(at) { Some(jump) if jump.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Jump => (jump.jump_target() as usize, register), _ => (at, register) }
    }

    /// `state` past a join with `incoming`: each register's facts as both paths prove them.
    /// A join no verified fact describes invalidates the region.
    fn join_states(&mut self, state: &mut State, incoming: &State, pc: usize) {
        // Only the same pre-branch SSA definition can survive both numeric arms.
        state.proofs.fields.retain(|field| incoming.proofs.fields.contains(field));
        state.proofs.int32_projections.retain(|projection| incoming.proofs.int32_projections.contains(projection));
        state.proofs.meet_homed(&incoming.proofs.homed);
        for (index, incoming) in incoming.values.iter().copied().enumerate() {
            let current = state.values[index];
            if current != incoming {
                let representation = if current.value.representation == incoming.value.representation {
                    current.value.representation
                } else {
                    self.valid &= current.tagged.current() && incoming.tagged.current();
                    Representation::Tagged
                };
                let Some(mut fact) = RegisterFact::admit(Fact {
                    ty: current.value.ty.join(incoming.value.ty), representation,
                    binding: BindingTime::Dynamic, proof: Proof::Definition,
                    projection: Projection::Exact,
                }) else { self.valid = false; return; };
                // A tagged view no later use reads is dropped (`demand`): a leave rebuilds it from the unboxed view.
                fact.tagged = if current.tagged == incoming.tagged { current.tagged }
                    else if current.tagged.current() && incoming.tagged.current()
                        && (representation == Representation::Tagged || self.demand_from(pc + 1, index).tagged()) { TaggedView::Variable }
                    else { TaggedView::Unavailable };
                state.values[index] = fact;
            }
        }
    }

    /// The ToInt32 of `number` (`emit_number_to_int32`).
    fn number_to_int32(&mut self, number: ir::Value, exit: ir::Block) -> ir::Value {
        super::emit_number_to_int32(&mut self.builder, number, exit)
    }

    /// `register`'s int32 view (`Registers::int`).
    fn int_var(&mut self, register: usize) -> Variable {
        frame::register_int(&mut self.builder, &mut self.registers, register)
    }

    /// `register`'s float64 view (`Registers::float`).
    fn float_var(&mut self, register: usize) -> Variable {
        frame::register_float(&mut self.builder, &mut self.registers, register)
    }

    /// Leaves with `word`: a lane body stops with it as its result word (`stop`); a static
    /// typed region returns it as the source PC its canonical code continues at.
    fn return_at(&mut self, word: u64) {
        match self.completion {
            Completion::Body => {
                let word = self.builder.ins().iconst(types::I64, word as i64);
                let stop = self.stop;
                self.builder.ins().jump(stop, &[word.into()]);
            }
            Completion::Number => {
                let miss = self.builder.ins().iconst(types::I32, NUMBER_ENTRY_MISS);
                let no_word = self.builder.ins().iconst(types::I64, 0);
                self.builder.ins().return_(&[miss, no_word]);
            }
        }
    }

    /// A Number return of `register`: the staged return, after the interrupt check and the
    /// type proof the lane keeps (a register not proved Number invalidates the region).
    fn return_number(&mut self, state: &mut State, register: Register, pc: u32) {
        self.finishes = true;
        if !state.values[usize::from(register)].value.ty.is_subtype(Type::NUMBER) {
            self.valid = false;
            self.return_at(0);
            return;
        }
        self.interrupt_check(state, pc);
        let exit = self.guard_exit(state, pc);
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, crate::staged::Operation::Return { source: register });
    }

    /// The semantic join selects one ABI format for every successful return.
    /// Guards and termination leave without publishing a result.
    fn finish_number_returns(&mut self) -> NativeNumberResult {
        let ty = self.number_returns.iter().fold(Type::NEVER, |ty, returned| ty.join(returned.fact.ty));
        let format = if ty != Type::NEVER && ty.is_subtype(Type::INT32) {
            NativeNumberResult::Int32
        } else { NativeNumberResult::Float64 };
        for returned in std::mem::take(&mut self.number_returns) {
            self.builder.switch_to_block(returned.block);
            let value = self.builder.block_params(returned.block)[0];
            let value = match (format, returned.fact.representation) {
                (NativeNumberResult::Int32, representation) => {
                    let integer = match representation {
                        Representation::Int32 => value,
                        Representation::Float64 => self.ins().fcvt_to_sint_sat(types::I32, value),
                        Representation::Tagged => unreachable!("typed numeric return"),
                    };
                    self.ins().uextend(types::I64, integer)
                }
                (NativeNumberResult::Float64, Representation::Int32) => {
                    let number = self.ins().fcvt_from_sint(types::F64, value);
                    self.ins().bitcast(types::I64, MemFlagsData::new(), number)
                }
                (NativeNumberResult::Float64, Representation::Float64) => self.ins().bitcast(types::I64, MemFlagsData::new(), value),
                (_, Representation::Tagged) => unreachable!("typed numeric return"),
            };
            let success = self.ins().iconst(types::I32, 0);
            self.ins().return_(&[success, value]);
        }
        format
    }

    /// The receiver bits a run starts with: a lane body's caller passes them as the entry's
    /// word, a loop region reads the payload's.
    fn entry_receiver_bits(&mut self) -> ir::Value {
        if self.body_entry() { return self.entry_words.0; }
        emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, receiver))
    }

    /// The running activation's dense capture slots (address, count): a lane body's caller
    /// packs them in the entry's word (`entry_cell::capture_word`), a loop region reads the payload's.
    fn entry_capture_words(&mut self) -> (ir::Value, ir::Value) {
        if !self.body_entry() {
            let values = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, capture_values));
            return (values, emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, capture_len)));
        }
        let word = self.entry_words.1;
        let mask = self.ins().iconst(types::I64, super::entry_cell::CAPTURE_ADDRESS_MASK);
        let values = self.ins().band(word, mask);
        (values, self.ins().ushr_imm_u(word, i64::from(super::entry_cell::CAPTURE_ADDRESS_BITS)))
    }

    /// Whether this run is a lane body, whose callers pass its receiver and captures as the
    /// entry's words; a loop region reads the payload's.
    fn body_entry(&self) -> bool { matches!(self.coverage, Coverage::Prefix | Coverage::PrefixFrom(_)) }

    /// The facts every run proves once at entry.
    fn entry_facts(&mut self, instructions: &[WideInstruction], _loops: &[(usize, usize)], _leaders: &CompoundBitSet) -> Entry {
        let this_bits = match self.helpers {
            LaneHelpers::Numeric => unreachable!("Number signatures have no receiver"),
            #[cfg(feature = "aot")]
            LaneHelpers::Bound(..) => self.fixed_this(),
            #[cfg(feature = "aot")]
            LaneHelpers::Linked(..) => unreachable!("helper table bound before entry facts"),
            _ => self.entry_receiver_bits(),
        };
        let accesses_fields = self.accesses_fields(instructions);
        let receiver = if accesses_fields { emit_lane_receiver(&mut self.builder, self.payload, this_bits) } else { self.absent_receiver() };
        let this = self.builder.declare_var(types::I64);
        self.builder.def_var(this, this_bits);
        let mut receiver_shapes = BTreeMap::new();
        for field in self.fields.iter().flatten().flatten() {
            if receiver_shapes.contains_key(&field.shape) {
                continue;
            }
            let shape = self.builder.use_var(receiver.shape);
            let same = self.builder.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(field.shape));
            let variable = self.builder.declare_var(types::I8);
            self.builder.def_var(variable, same);
            receiver_shapes.insert(field.shape, variable);
        }
        // A direct store of an atom needs a specialized run and no method cache naming it; a
        // run that makes no call changes neither (`emit_js_own_field_set`, `Lowering::site_store`).
        let mut store_atoms = BTreeMap::new();
        let stored = instructions.iter().filter_map(|instruction| match instruction.op() {
            _ if instruction.op().is_field_store() => Some(instruction.atom_index()),
            opcode if opcode.is_get_field() && instruction.writes_current_this() => match instruction.field_lookup() {
                Some(FieldLookup::Site(index)) => self.program.field_sites.get(index).and_then(|site| site.sink).map(|(atom, _)| atom),
                _ => None,
            },
            _ => None,
        }).collect::<Vec<_>>();
        for atom in stored {
            if let std::collections::btree_map::Entry::Vacant(slot) = store_atoms.entry(atom) {
                let admitted = self.emit_store_atom_admitted(atom);
                let variable = self.builder.declare_var(types::I8);
                self.builder.def_var(variable, admitted);
                slot.insert(variable);
            }
        }
        // The running activation's dense capture slots (`EnvironmentSlots::native_dense_view`),
        // fixed for the run; each read loads its slot where it runs, after any call that may
        // have stored to it.
        let capture_values = self.builder.declare_var(types::I64);
        let capture_len = self.builder.declare_var(types::I64);
        let (values, len) = self.entry_capture_words();
        self.builder.def_var(capture_values, values);
        self.builder.def_var(capture_len, len);
        let mut miss = None;
        let object_table = accesses_fields
            .then(|| {
                let base = emit_js_record_base(&mut self.builder, self.payload,
                    std::mem::offset_of!(JsStepDispatch, object_record_base));
                let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                CheckedObjectTable::prove(&mut self.builder, base, exit)
            });
        let mut calls = CompoundBitSet::with_capacity(self.unit.frames.len());
        let mut call_receivers = CompoundBitSet::with_capacity(self.unit.frames.len());
        if self.completion != Completion::Number {
            // Roots name canonical caller homes never written by this unit.
            // Local Move aliases are valid only within their basic block.
            let roots: Vec<_> = (0..self.unit.registers).map(|register|
                (register < self.unit.caller_registers && !self.written.contains(usize::from(register)))
                    .then_some(register)).collect();
            let mut aliases = roots.clone();
            let mut admitted = smallvec::SmallVec::<[(Register, super::lane_unit::CallTarget); 2]>::new();
            let mut receivers = smallvec::SmallVec::<[(Register, ReceiverRecovery); 2]>::new();
            for (pc, instruction) in instructions.iter().copied().enumerate() {
                if self.blocks[pc].is_some() { aliases.copy_from_slice(&roots); }
                if !self.admits_at_entry(pc) { continue; }
                if let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc) {
                    let call = self.unit.frames[frame];
                    let body = self.program.function(call.target.function as usize);
                    let numeric = restartable_numeric_function(self.program, body, &body.instructions().collect::<Vec<_>>());
                    if !(call.recovery == super::lane_unit::CallRecovery::RestartCall && numeric)
                        && let Some(root) = aliases[usize::from(call.receiver)] {
                        let recovery = ReceiverRecovery::for_body(body);
                        if let Some((_, required)) = receivers.iter_mut().find(|(owner, _)| *owner == root) {
                            if recovery == ReceiverRecovery::Object { *required = recovery; }
                        } else { receivers.push((root, recovery)); }
                        call_receivers.insert(frame);
                    }
                    if call.alternative.is_none()
                        && numeric
                        && let Some(root) = aliases[usize::from(call.callee_register)] {
                        let binding = (root, call.target);
                        if !admitted.contains(&binding) {
                            let callee = self.builder.ins().load(types::I64, regions::frame(),
                                self.registers_base, register_offset(root));
                            let matched = match call.target.callee {
                                CalleeBinding::Function => self.function_binding(callee, call.target.function),
                                CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => self.is_identity(callee, identity),
                            };
                            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                            emit_js_guard(&mut self.builder, matched, exit);
                            admitted.push(binding);
                        }
                        calls.insert(frame);
                    }
                }
                let root = (instruction.op().is_move())
                    .then(|| aliases[usize::from(instruction.register_b())]).flatten();
                for register in writes(instruction) { aliases[usize::from(register)] = root; }
            }
            for (root, recovery) in receivers {
                let receiver = self.builder.ins().load(types::I64, regions::frame(),
                    self.registers_base, register_offset(root));
                let matched = self.receiver_recovery(receiver, recovery);
                let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                emit_js_guard(&mut self.builder, matched, exit);
            }
        }
        #[cfg(feature = "aot")]
        let fields = {
            let mut fields = smallvec::SmallVec::new();
            if matches!(self.helpers, LaneHelpers::Bound(..)) {
                let mut uses = smallvec::SmallVec::<[(u16, bool, Type); INLINE_FIELD_BINDINGS]>::new();
                for (pc, instruction) in instructions.iter().enumerate() {
                    // An inlined call's later alternatives are entered by its switch, an edge
                    // the bytecode facts do not see: their sites bind too, or they would read
                    // through the VM's caches for the whole run.
                    if !self.facts.reached(pc) && !matches!(self.unit.origins[pc], super::lane_unit::Origin::Inlined { .. }) { continue; }
                    // A field chain names sequential sites, not alternatives for
                    // one polymorphic access. Bind each distinct column once.
                    let mut sites = smallvec::SmallVec::<[u16; 3]>::new();
                    match instruction.op() {
                        opcode if opcode.is_get_field() => match instruction.field_lookup() {
                            Some(FieldLookup::Atom { cache_site, .. }) => sites.push(cache_site),
                            Some(FieldLookup::Site(site)) => {
                                let site = self.program.field_sites[site];
                                sites.push(site.first.1);
                                if let Some((_, cache)) = site.second { sites.push(cache); }
                                if instruction.writes_current_this()
                                    && let Some((_, cache)) = site.sink { sites.push(cache); }
                            }
                            _ => {},
                        },
                        _ if instruction.op().is_field_store() => sites.push(instruction.cache_site_index()),
                        _ => {},
                    }
                    let repeated = _loops.iter().any(|&(header, back)| (header..=back).contains(&pc));
                    for site in sites {
                        let writes = if instruction.op().is_field_store() {
                            self.facts.before(pc, instruction.register_a(), instructions, _leaders,
                                &self.program.constants, &writes, self.native_calls)
                        } else if instruction.writes_current_this()
                            && matches!(instruction.field_lookup(), Some(FieldLookup::Site(index))
                                if self.program.field_sites[index].sink.is_some_and(|(_, sink)| sink == site)) {
                            Type::ANY
                        } else { Type::NEVER };
                        if let Some((_, reused, domain)) = uses.iter_mut().find(|(known, _, _)| *known == site) {
                            *reused |= repeated;
                            *domain = domain.join(writes);
                        } else { uses.push((site, repeated, writes)); }
                    }
                }
                for (site, repeated, writes) in uses {
                    let exit = *miss.get_or_insert_with(|| self.builder.create_block());
                    let mut binding = emit_js_field_binding(&mut self.builder, self.payload, site, exit, object_table);
                    let slot = binding.slot.read(&mut self.builder);
                    binding.slot = FieldSlot::Bound(slot);
                    // A slot whose writes the run does not admit leaves only at its stores
                    // (`EmitFrame::bound_store`): the rest of the run, which reads it by shape, holds.
                    if writes != Type::NEVER { binding.admitted = Some(self.field_writes_admitted(&mut binding, writes)); }
                    // The extra entry arithmetic pays for repeated receiver loads.
                    // A one-off access keeps its cheaper storage-plus-slot form.
                    if repeated {
                        binding.arena = binding.arena.bind(&mut self.builder, slot);
                    }
                    fields.push((site, binding));
                }
            }
            fields
        };
        #[cfg(not(feature = "aot"))]
        let fields = smallvec::SmallVec::new();
        self.admit_store_domains(instructions, &mut miss);
        let records = self.entry_field_records(instructions, &receiver, object_table, &mut miss);
        if let Some(miss) = miss {
            let admitted = self.builder.create_block();
            self.builder.ins().jump(admitted, &[]);
            self.builder.switch_to_block(miss);
            self.builder.set_cold_block(miss);
            // No guest operation has run and canonical registers are untouched.
            // Decline the whole run once, at its start, even for a speculative unused binding.
            let origin = |pc: usize| match self.unit.origins[pc] {
                super::lane_unit::Origin::Caller(pc) => u64::from(pc),
                super::lane_unit::Origin::Inlined { .. } => unreachable!("a run starts in the caller"),
            };
            self.return_at(origin(self.coverage.entry()));
            self.builder.switch_to_block(admitted);
        }
        Entry { receiver, object_table, records, receiver_shapes, store_atoms, capture_values, capture_len, this, fields, calls, call_receivers }
    }

    /// Whether the entry edge admits the inlined call at `pc`, a call a run may skip
    /// (`packet != null`) and so not one every run may be refused for: a body that left at
    /// its entry once admits them at their markers instead (`Lowering::inline_call`).
    fn admits_at_entry(&self, pc: usize) -> bool {
        self.facts.reached(pc) && !self.generic.contains(&entry_leave(self.unit))
    }

    /// `invariant_field_records`, unless the body left at its entry: it binds no receiver record
    /// there, the reads it hoisted ran under a branch (`packet != null`) and the run declined at
    /// its start.
    fn entry_field_records(&mut self, instructions: &[WideInstruction], receiver: &LaneReceiver,
        object_table: Option<CheckedObjectTable>, miss: &mut Option<ir::Block>) -> Vec<Option<ObjectRecord>> {
        if self.generic.contains(&entry_leave(self.unit)) { return vec![None; usize::from(self.unit.registers) + 1]; }
        self.invariant_field_records(instructions, receiver, object_table, miss)
    }

    /// Bind physical receiver facts once. The expanded unit's write set owns
    /// invariance, including renamed callee registers and synthesized returns.
    fn invariant_field_records(&mut self, instructions: &[WideInstruction], receiver: &LaneReceiver,
        object_table: Option<CheckedObjectTable>, miss: &mut Option<ir::Block>) -> Vec<Option<ObjectRecord>> {
        let count = usize::from(self.unit.registers);
        let mut owners: CompoundBitSet = CompoundBitSet::with_capacity(count + 1);
        let mut reachable = false;
        for (pc, instruction) in instructions.iter().enumerate() {
            if self.blocks[pc].is_some() { reachable = self.facts.at(pc).is_some(); }
            if !reachable { continue; }
            let owner = match instruction.op() {
                opcode if opcode.is_get_field() && !super::reads_length(self.program, *instruction) => {
                    let base = self.field_base(pc, *instruction);
                    base.and_then(|base| if base == crate::bytecode::FieldBase::THIS { Some(count) }
                        else { base.register_index().map(usize::from) }).filter(|_| self.reads_own_field(*instruction))
                }
                _ if instruction.op().is_field_store() => Some(match (instruction.field_store_register(), self.unit.origins[pc]) {
                    (Some(register), _) => usize::from(register),
                    (None, super::lane_unit::Origin::Inlined { frame, .. }) => usize::from(self.unit.frames[frame].receiver),
                    (None, _) => count,
                }),
                _ => None,
            };
            if let Some(owner) = owner.filter(|owner| *owner == count
                || (*owner < usize::from(self.unit.caller_registers) && !self.written.contains(*owner))) {
                owners.insert(owner);
            }
            if instruction.returns_from_frame() || instruction.op().control_flow_layout() == ControlFlowLayout::Terminal {
                reachable = false;
            }
        }
        let mut admitted = vec![None; count + 1];
        let mut records = None;
        for owner in owners.iter() {
            let exit = *miss.get_or_insert_with(|| self.builder.create_block());
            admitted[owner] = Some(if owner == count {
                // emit_lane_receiver already owns family/range/base admission.
                let record = self.builder.use_var(receiver.object);
                let present = self.ins().icmp_imm_u(IntCC::NotEqual, record, 0);
                emit_js_guard(&mut self.builder, present, exit);
                ObjectRecord::Address(record)
            } else {
                let records = match object_table {
                    Some(table) => ObjectTable::Checked(table),
                    None => ObjectTable::Unchecked(*records.get_or_insert_with(|| emit_js_record_base(&mut self.builder,
                        self.payload, std::mem::offset_of!(JsStepDispatch, object_record_base)))),
                };
                // An unwritten register holds its entry value at either start, in the file.
                let base = self.registers_base;
                let bits = self.ins().load(types::I64, regions::frame(), base, register_offset(owner as Register));
                ObjectRecord::prove(&mut self.builder, bits, records, None, exit)
            });
        }
        admitted
    }

    /// Admit once at the edge; later raw stores consume the compile-time domain proof.
    fn admit_field_writes(&mut self, binding: &mut FieldSlotBinding, writes: Type, exit: ir::Block) {
        if binding.writes.is_some_and(|domain| writes.is_subtype(domain)) {
            if let Some(admitted) = binding.admitted { emit_js_guard(&mut self.builder, admitted, exit); }
            return;
        }
        let admitted = self.field_writes_admitted(binding, writes);
        emit_js_guard(&mut self.builder, admitted, exit);
    }

    /// Whether the slot `binding` names admits `writes` (`LaneHelper::FieldWriteAdmission`),
    /// the domain then recorded on the binding.
    fn field_writes_admitted(&mut self, binding: &mut FieldSlotBinding, writes: Type) -> ir::Value {
        let shape = self.builder.ins().uextend(types::I64, binding.receiver);
        let slot = binding.slot.read(&mut self.builder);
        let (admitted, domain) = self.slot_writes_admitted(shape, slot, writes);
        binding.writes = Some(binding.writes.map_or(domain, |held| held.join(domain)));
        admitted
    }

    fn admit_slot_writes(&mut self, shape: ir::Value, slot: ir::Value, writes: Type, exit: ir::Block) -> Type {
        let (admitted, domain) = self.slot_writes_admitted(shape, slot, writes);
        emit_js_guard(&mut self.builder, admitted, exit);
        domain
    }

    fn slot_writes_admitted(&mut self, shape: ir::Value, slot: ir::Value, writes: Type) -> (ir::Value, Type) {
        let domain = crate::typed_ir::SlotType::from_type(writes);
        let encoded = self.builder.ins().iconst(types::I64, domain as i64);
        let helper = self.helpers.address(&mut self.builder, LaneHelper::FieldWriteAdmission);
        let admitted = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[shape, slot, encoded]);
        (self.builder.ins().icmp_imm_u(IntCC::NotEqual, admitted, 0), domain.ty())
    }

    fn emit_store_atom_admitted(&mut self, atom: Atom) -> ir::Value {
        let specialized = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, stores_specialized));
        let specialized = self.builder.ins().icmp_imm_u(IntCC::NotEqual, specialized, 0);
        if let Some(held) = self.store_held_unnamed(atom, specialized) { return held; }
        let free = self.atom_unnamed(atom);
        self.builder.ins().band(specialized, free)
    }

    /// Conform the entry's state to the facts at `pc`. A lane body's failed input proof
    /// leaves through one shared block with `LaneExit::Entry`, its specialization's feedback
    /// that the inputs need no contract; other regions leave at `pc`.
    fn entry_conform(&mut self, state: &mut State, pc: usize, coverage: Coverage) {
        self.proving_entry = matches!(coverage, Coverage::Prefix) && self.completion.status_and_pc();
        self.conform(state, pc);
        self.proving_entry = false;
        self.guard_entering_ranges(state, pc);
    }

    /// The observed int32 entries live at `entry` (a dead register's value is stale, and nothing
    /// conforms it) whose range is narrower than int32 and whose entry fact is
    /// int32: what the entry guards once (`guard_entering_ranges`) and the pass assumes.
    fn entering_ranges(&self, entry: usize) -> Vec<(Register, IntRange)> {
        let types = self.facts.at(entry);
        self.int_entries.iter().copied().filter(|&(register, range)| range != IntRange::FULL && self.is_live(entry, register)
            && types.and_then(|types| types.get(usize::from(register))).is_some_and(|ty| ty.is_subtype(Type::INT32))).collect()
    }

    /// Leaves at `pc` where an entering register is below its range's minimum: the one guard
    /// the ranges pass's entry state relies on (the range's maximum is int32's).
    fn guard_entering_ranges(&mut self, state: &mut State, pc: usize) {
        for (register, range) in self.entering.clone() {
            let exit = self.guard_exit(state, pc as u32);
            let value = self.int(state, register, exit);
            let within = self.builder.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, value, i64::from(range.min));
            emit_js_guard(&mut self.builder, within, exit);
        }
    }

    /// The guards' leaves to fill, once the entry's shared leave (`entry_conform`) is filled:
    /// it leaves with the plan's entry leave, with no register written yet to write back.
    fn take_leaves(&mut self) -> Vec<GuardExit> {
        if let Some(block) = self.entry_exit.take() {
            self.switch_to(block, true);
            self.return_entry();
        }
        std::mem::take(&mut self.pending)
    }

    /// The leave of a failed entry proof, before any instruction ran: a lane plan names it
    /// (`LaneExit::Entry`), so the activation's specialization drops the contract that
    /// failed; a Number entry misses, restarting the original call.
    fn return_entry(&mut self) {
        match self.completion {
            Completion::Body => self.return_at(super::LaneExit::Entry.word()),
            Completion::Number => self.return_at(0),
        }
    }

    /// Lower the instruction at `pc`; whether control falls through to the next.
    fn instruction(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        if self.consumed.contains(pc) { return true; }
        self.left_here = self.generic.contains(&pc);
        if self.unit.markers.contains(&pc) || !retains_array_views(&self.program.constants, instruction) {
            state.proofs.arrays.clear();
        }
        if let Some(read) = self.hoisted.get(&pc).copied() {
            self.hoisted_read(state, pc, instruction, read);
            return true;
        }
        let pc32 = pc as u32;
        let plain = instruction.result_flags() == 0;
        let ran = match instruction.op() {
            opcode if opcode.is_throw() => self.throw(state, pc, instruction),
            // A call, a construction in the caller's window and a return: the staged operations
            // the tier's facts admit (`call_rows`).
            _ if self.is_call_row(instruction) => self.call_row_step(state, pc, instruction),
            _ if self.is_call_out(instruction) => Some(self.call_out_step(state, pc, instruction)),
            _ if self.is_register_row(instruction) => Some(self.register_step(state, pc, instruction)),
            opcode if (opcode.is_load_name() || opcode.is_load_name_call()) && plain && self.global_reads.contains_key(&pc) => Some(self.global_read(state, pc, instruction)),
            opcode if opcode.is_nop() && plain => match self.unit.markers.iter().position(|marker| *marker == pc) {
                Some(frame) => Some(self.inline_call(state, pc, frame)),
                None => Some(true),
            },
            _ if plain && instruction.op().js_denotation() == JsDenotation::LoadConst => {
                let Some(constant) = self.program.constants.get(instruction.constant_index()) else { return false };
                self.load_constant(state, pc, instruction, constant)
            }
            opcode if opcode.is_return() && plain && self.completion == Completion::Number => {
                self.return_number(state, instruction.register_a(), pc32);
                Some(false)
            }
            // A capture's read or store, by its staged row.
            _ if self.is_capture_row(pc, instruction) => Some(self.capture_step(state, pc, instruction)),
            _ if instruction.op().is_get_field() && instruction.result_flags() & !(crate::bytecode::SET_THIS_REGISTER | crate::bytecode::RETURN_REGISTER) == 0 => self.get_field(state, pc, instruction),
            _ if self.is_element_row(instruction) => Some(self.element_step(state, pc, instruction)),
            _ if matches!((plain, instruction.op().field_store()), (true, Some(_))) => self.set_field(state, pc, instruction),
            _ if instruction.op().js_denotation() == JsDenotation::NumericBinary && instruction.numeric_local_target().is_none() => {
                self.operator(state, pc, instruction).or_else(|| self.binary_call_out(state, pc, instruction))
            }
            opcode if opcode.is_unary() && plain && instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX
                && instruction.register_b() < self.unit.registers => self.typeof_index(state, pc, instruction),
            opcode if opcode.is_unary() && crate::bytecode::typeof_test(instruction.unary_operator()).is_some()
                && !self.helpers.is_numeric() => self.typeof_test(state, pc, instruction),
            _ if plain && instruction.op().js_denotation() == JsDenotation::NumericUnary
                && instruction.register_b() < self.unit.registers => self.unary(state, pc, instruction),
            _ if self.allocation_row(pc, instruction) => Some(self.allocation(state, pc, instruction)),
            _ if self.is_construction(instruction) => self.construct(state, pc, instruction),
            _ if plain && self.is_jump(instruction) => return self.jump(state, pc, instruction),
            _ if plain && instruction.op().is_truthiness_jump() => {
                let target = instruction.jump_target() as usize;
                let exit = self.guard_exit(state, pc32);
                let taken_block = self.builder.create_block();
                let fall_block = self.builder.create_block();
                self.step(state, frame::Edges::Branch { target: taken_block, following: fall_block, miss: exit },
                    crate::staged::Operation::JumpIf { register: instruction.register_a(), taken: instruction.op().is_jump_true() });
                self.continue_branch(state, pc, target, taken_block, fall_block);
                return false;
            }
            _ if plain && self.is_compare_jump(instruction) => {
                if self.compare_jump(state, pc, instruction) { return false; }
                None
            }
            _ => None,
        };
        match ran {
            Some(falls) => {
                self.native += 1;
                falls
            }
            None => {
                super::dispatch_fast::lane_body_stats::unlowered(instruction.op());
                if self.coverage.full() && (self.completion == Completion::Number
                    || !((instruction.op().class().eq(Class::Return) && plain) || instruction.returns_from_frame())) {
                    self.valid = false;
                }
                self.exit(state, pc32);
                false
            }
        }
    }

    /// The end of a body whose helper neither completed nor missed: a termination stops with its
    /// status, and any other `status` is a leave in a callee.
    fn stop_by_status(&mut self, status: ir::Value) {
        let terminated = self.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_TERMINATED as i64);
        let stopped = self.builder.create_block();
        let callee = self.builder.create_block();
        self.ins().brif(terminated, stopped, &[], callee, &[]);
        self.builder.switch_to_block(stopped);
        let terminate = self.ins().iconst(types::I32, TERMINATED_STATUS);
        self.leave_with(terminate, None);
        self.builder.switch_to_block(callee);
        if let Some(raised) = raised { self.branch_if_raised(status, raised); }
        let word = self.ins().iconst(types::I64, LaneExit::Callee.word() as i64);
        let stop = self.stop;
        self.ins().jump(stop, &[word.into()]);
    }

    /// A helper's result, read from the lane's register `destination` as the register's value.
    fn reload_result(&mut self, state: &mut State, destination: Register) {
        let base = self.registers_base;
        let bits = self.ins().load(types::I64, regions::frame(), base, register_offset(destination));
        self.set_tagged(state, destination, bits);
        state.proofs.homed.insert(usize::from(destination));
    }

    /// The registers the body wrote that are live at `pc`: what a helper reads from the lane's file.
    fn spilled(&self, pc: usize) -> Vec<Register> {
        self.written.iter().map(|register| register as Register)
            .take_while(|register| *register < self.unit.caller_registers)
            .filter(|register| self.is_live(pc, *register)).collect()
    }

    /// The live registers the body wrote since their last write-back, stored to the lane's registers for a
    /// helper to read. A register whose slot is current (`LocalProofs::homed`) is neither boxed nor stored.
    fn home_written(&mut self, state: &mut State, pc: usize) {
        for register in self.spilled(pc) {
            let index = usize::from(register);
            if state.proofs.homed.contains(index) { continue; }
            let bits = self.tagged(state, register);
            let base = self.registers_base;
            self.ins().store(regions::frame(), bits, base, register_offset(register));
            state.proofs.homed.insert(index);
        }
    }

    /// A relation's predicate: `instanceof` by the prototype chain (`instance_of`),
    /// (in)equality of operands not both Numbers (`equality`), every other relation of Numbers.
    fn relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        let numbers = [left, right].iter().all(|operand| super::facts::operand_type(
            &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
            .is_subtype(Type::NUMBER));
        // A frameless Number region has no VM helper capability (`LaneHelpers::Numeric`).
        let helpers = !self.helpers.is_numeric();
        if operator == oxc_ast::ast::BinaryOperator::Instanceof as u32 {
            return helpers.then(|| self.instance_of(state, pc, left, right)).flatten();
        }
        match super::operations::equality_operator(operator) {
            Some((strict, negated)) if !numbers && helpers && self.operand_supported(left) && self.operand_supported(right) => {
                let equal = self.equality(state, pc, left, right, strict)?;
                Some(if negated { self.ins().bxor_imm_u(equal, 1) } else { equal })
            }
            _ => self.number_relation(state, pc, operator, left, right),
        }
    }

    /// A helper's call, by its address in the running image's table (`LaneHelpers::address`).
    fn helper_call(&mut self, helper: LaneHelper, arguments: &[ir::Value]) -> ir::Value {
        let address = self.helpers.address(&mut self.builder, helper);
        emit_lane_helper_call(&mut self.builder, self.payload, address, arguments)
    }

    /// The property arena's base again, which an allocation may have moved.
    fn reload_property_arena(&mut self) {
        let properties = self.entry.as_ref().expect("entry facts").receiver.properties;
        let (values, _) = emit_js_property_values(&mut self.builder, self.payload);
        self.builder.def_var(properties, values);
    }

    /// After a helper that allocated, which may move the property arena: the projections
    /// into it retire and its base reloads.
    fn allocated(&mut self, state: &mut State) {
        state.proofs.layouts.clear();
        self.reload_property_arena();
    }

    /// After a helper that may also have changed any object's shape or storage (a
    /// construction, an adding store): every proof of an object retires, as another register
    /// may name it.
    fn reshaped(&mut self, state: &mut State) {
        state.call_effect();
        self.reload_property_arena();
    }


    /// A fused `typeof value === name` (`bytecode::typeof_test`): an immediate value's tag
    /// decides it, as `Vm::typeof_test_value` would, and a heap value's cell the VM reads
    /// (`lane_typeof_is`). A returning test completes the activation with its Boolean.
    fn typeof_test(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        use crate::value::ValueLayout;
        let (index, negated) = crate::bytecode::typeof_test(instruction.unary_operator())?;
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) if instruction.result_flags() == 0 => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if self.completion.status_and_pc() => Some(record),
            _ => return None,
        };
        let value = self.tagged(state, instruction.register_b());
        // The immediate classes whose name is the tested one, by the VM's own rule.
        let named = |sample: Value| super::super::coercion::immediate_typeof_index(sample) == Some(index);
        let mut immediate = self.ins().iconst(types::I8, 0);
        if named(Value::number(0.0)) {
            let field = self.ins().ushr_imm_u(value, i64::from(ValueLayout::TAG_SHIFT));
            let offset = self.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
            let double = self.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, offset, ValueLayout::TAG_FIELD_SPAN as i64);
            let int = is_int_tagged(&mut self.builder, value);
            let number = self.ins().bor(double, int);
            immediate = self.ins().bor(immediate, number);
        }
        for sample in [Value::UNDEFINED, Value::DELETED, Value::NULL] {
            if named(sample) {
                let same = self.ins().icmp_imm_u(IntCC::Equal, value, sample.to_bits() as i64);
                immediate = self.ins().bor(immediate, same);
            }
        }
        if named(Value::TRUE) {
            let tag = self.ins().band_imm_u(value, ValueLayout::TAG_MASK as i64);
            let boolean = self.ins().icmp_imm_u(IntCC::Equal, tag, (Value::TRUE.to_bits() & ValueLayout::TAG_MASK) as i64);
            immediate = self.ins().bor(immediate, boolean);
        }
        let tag = self.ins().band_imm_u(value, ValueLayout::TAG_MASK as i64);
        let heap = self.ins().icmp_imm_u(IntCC::Equal, tag, ValueLayout::HEAP_TAG as i64);
        let (cell, join) = (self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.ins().brif(heap, cell, &[], join, &[ir::BlockArg::Value(immediate)]);
        self.builder.switch_to_block(cell);
        let tested = self.ins().iconst(types::I64, index as i64);
        let is = self.helper_call(LaneHelper::TypeofIs, &[value, tested]);
        let is = self.ins().ireduce(types::I8, is);
        self.ins().jump(join, &[ir::BlockArg::Value(is)]);
        self.builder.switch_to_block(join);
        let holds = if negated { self.ins().bxor_imm_u(holds, 1) } else { holds };
        let boolean = self.boolean(holds);
        if let Some(record) = completes {
            self.return_value(record, boolean);
            return Some(false);
        }
        self.set_boolean(state, instruction.result_register(), boolean);
        Some(true)
    }

    /// `operation` by the interpreter's semantics (`staged::step`) on the emit stage, with
    /// `edges` as its control edges. The lane specializer decides every truthiness itself, so
    /// no step is left unresolved.
    fn step(&mut self, state: &mut State, edges: frame::Edges, operation: crate::staged::Operation<ir::Value, EmitProof>)
        -> crate::staged::Step<ir::Value, ()> {
        self.step_with(state, edges, operation, StepWith::default())
    }

    /// `step` of a named field access: the frame holds the `plans` the sites proved and what
    /// the access already has (`HeldRead`), and the stage addresses the property arena.
    fn field_step(&mut self, state: &mut State, edges: frame::Edges, operation: crate::staged::Operation<ir::Value, EmitProof>,
        access: (&LanePlans<'_>, HeldRead)) -> crate::staged::Step<ir::Value, ()> {
        self.step_with(state, edges, operation, StepWith { access: Some(access), ..StepWith::default() })
    }

    /// `step` with what the operation's frame needs beyond its edges (`StepWith`).
    fn step_with(&mut self, state: &mut State, edges: frame::Edges, operation: crate::staged::Operation<ir::Value, EmitProof>,
        StepWith { access, sites, site }: StepWith<'_>) -> crate::staged::Step<ir::Value, ()> {
        self.step_at(state, edges, None, operation)
    }

    /// `step` of an operation whose slow case calls the VM (`call_out`: the record and what its
    /// helper needs).
    fn step_at(&mut self, state: &mut State, edges: frame::Edges, call_out: Option<&frame::call_out::CallOutSite>,
        operation: crate::staged::Operation<ir::Value, EmitProof>) -> crate::staged::Step<ir::Value, ()> {
        let locals = self.completion != Completion::Number;
        let property_values = access.and_then(|_| self.entry.as_ref()).map(|entry| self.builder.use_var(entry.receiver.properties));
        let gate = access.map(|_| self.payload);
        let mut stage = Emit { builder: &mut self.builder, property_values, gate };
        let fields = access.map(|(plans, held)| FieldFacts { plans, entry: self.entry.as_ref(), native_calls: self.native_calls,
            held,
            stable: StableHold { holders: &mut self.stable.holders, loops: &self.stable.stable_loops, online: self.vm.online, at: self.at } });
        let mut frame = EmitFrame { registers: &mut self.registers, written: &mut self.written,
            state, edges, constants: &self.program.constants, this: self.entry.as_ref().map(|entry| entry.this),
            payload: self.payload, helpers: self.helpers, locals, polls: &mut self.polls,
            roots: &mut self.stable.roots, number_returns: &mut self.number_returns, fields,
            call_out: sites.call_out, capture: sites.capture, html_dda: self.vm.html_dda,
            prototype: sites.prototype, table_entry: self.entry.as_ref(),
            held: self.held_view.take(), arrays_fixed: self.arrays_fixed, site };
        let step = crate::staged::step(&mut stage, &mut frame, operation);
        debug_assert!(!matches!(step, crate::staged::Step::Unresolved), "the lane specializer decides every truthiness");
        step
    }

    /// Whether a literal's allocation lowers here: plain, or the one that returns its value
    /// from a caller record, which completes the activation (`literal_done`).
    fn literal_completes(&self, instruction: WideInstruction, pc: usize) -> bool {
        self.completion.status_and_pc() && (instruction.result_flags() == 0
            || (instruction.returns_from_frame() && matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_)))))
    }

    /// A literal's allocation ran: a returning one completes its activation with the result.
    fn literal_done(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied().filter(|_| instruction.returns_from_frame()) else { return true };
        let value = self.tagged(state, instruction.result_register());
        self.return_value(record, value);
        false
    }

    fn typeof_index(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_b();
        let ty = state.values[usize::from(source)].value.ty;
        let known = [crate::ValueType::Number, crate::ValueType::Boolean, crate::ValueType::Null,
            crate::ValueType::Undefined, crate::ValueType::String, crate::ValueType::BigInt, crate::ValueType::Symbol]
            .into_iter().find(|kind| ty != Type::NEVER && ty.is_subtype(Type::language(*kind)));
        let Some(kind) = known else {
            // The staged operation leaves at the instruction for a heap value, whose cell decides its name.
            let exit = self.guard_exit(state, pc as u32);
            let operation = crate::staged::Operation::decode(instruction, crate::staged::DecodeContext::EMPTY).expect("typeof index decodes");
            self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
            return Some(true);
        };
        let index = crate::bytecode::typeof_name_index(super::super::coercion::typeof_text(kind)).expect("a language typeof name");
        let int = self.ins().iconst(types::I32, i64::try_from(index).expect("index fits i64"));
        self.define(state, instruction.result_register(), int, RegisterFact::definition(Type::INT32, Representation::Int32));
        Some(true)
    }

    /// `value instanceof constructor` by the VM's steady case (`Vm::steady_instanceof`), which
    /// calls nothing; a case only the full algorithm decides leaves at the test.
    fn instance_of(&mut self, state: &mut State, pc: u32, value: Operand, constructor: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let (value, constructor) = (self.operand_tagged(state, value)?, self.operand_tagged(state, constructor)?);
        let holds = self.helper_call(LaneHelper::InstanceOf, &[value, constructor]);
        let decided = self.ins().icmp_imm_u(IntCC::NotEqual, holds, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, decided, exit);
        Some(self.ins().ireduce(types::I8, holds))
    }

    /// `Vm::strict_equal` (`strict`) or `==` of two tagged operands. Identical bits are equal
    /// unless NaN, and two Numbers compare as Numbers. Strictly, a pair of strings or of
    /// legacy cells (BigInts) of one family compares by contents (`lane_strict_equal_cells`)
    /// and every other pair is unequal. Loosely, without conversions only: `null` and
    /// `undefined` equal each other and no object record, and two distinct object records are
    /// unequal; any other pair (a string, a Boolean, a function, which may be the IsHTMLDDA
    /// object) leaves at `pc`.
    fn equality(&mut self, state: &mut State, pc: u32, left: Operand, right: Operand, strict: bool) -> Option<ir::Value> {
        use crate::value::ValueLayout;
        let exit = self.guard_exit(state, pc);
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let [identical, distinct, numbers, others, cells, join] = std::array::from_fn(|_| self.builder.create_block());
        let equal = self.builder.append_block_param(join, types::I8);
        let same = self.ins().icmp(IntCC::Equal, left, right);
        self.ins().brif(same, identical, &[], distinct, &[]);
        self.builder.switch_to_block(identical);
        let ordered = self.ins().icmp_imm_u(IntCC::NotEqual, left, ValueLayout::CANONICAL_NAN as i64);
        self.ins().jump(join, &[ir::BlockArg::Value(ordered)]);
        self.builder.switch_to_block(distinct);
        // Distinct int-tagged bits are distinct int32 values: an int32 has one tagged form,
        // so the pair is unequal without a Number decode, which only other Numbers take.
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.ins().band(left_int, right_int);
        let (decoded, unequal_ints) = (self.builder.create_block(), self.ins().iconst(types::I8, 0));
        self.ins().brif(ints, join, &[ir::BlockArg::Value(unequal_ints)], decoded, &[]);
        self.builder.switch_to_block(decoded);
        self.builder.set_cold_block(numbers);
        let is_number = |builder: &mut FunctionBuilder<'_>, bits: ir::Value| {
            let field = builder.ins().ushr_imm_u(bits, i64::from(ValueLayout::TAG_SHIFT));
            let offset = builder.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
            let double = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, offset, ValueLayout::TAG_FIELD_SPAN as i64);
            let int = is_int_tagged(builder, bits);
            builder.ins().bor(double, int)
        };
        let (left_number, right_number) = (is_number(&mut self.builder, left), is_number(&mut self.builder, right));
        let both = self.ins().band(left_number, right_number);
        self.ins().brif(both, numbers, &[], others, &[]);
        self.builder.switch_to_block(numbers);
        let (left_value, right_value) = (emit_js_tagged_number(&mut self.builder, left, exit), emit_js_tagged_number(&mut self.builder, right, exit));
        let number_equal = self.ins().fcmp(FloatCC::Equal, left_value, right_value);
        self.ins().jump(join, &[ir::BlockArg::Value(number_equal)]);
        self.builder.switch_to_block(others);
        if !strict {
            let nullish = |builder: &mut FunctionBuilder<'_>, bits: ir::Value| {
                let null = builder.ins().icmp_imm_u(IntCC::Equal, bits, Value::NULL.to_bits() as i64);
                let undefined = builder.ins().icmp_imm_u(IntCC::Equal, bits, Value::UNDEFINED.to_bits() as i64);
                builder.ins().bor(null, undefined)
            };
            let record = |builder: &mut FunctionBuilder<'_>, bits: ir::Value| {
                let object = crate::staged::value::is_direct_object(&mut super::object_slots::emit(builder), bits);
                let array = crate::staged::value::is_direct_array(&mut super::object_slots::emit(builder), bits);
                builder.ins().bor(object, array)
            };
            let (left_nullish, right_nullish) = (nullish(&mut self.builder, left), nullish(&mut self.builder, right));
            let (left_record, right_record) = (record(&mut self.builder, left), record(&mut self.builder, right));
            // Each side nullish or an object record: decided without a conversion.
            let left_known = self.ins().bor(left_nullish, left_record);
            let right_known = self.ins().bor(right_nullish, right_record);
            let known = self.ins().band(left_known, right_known);
            emit_js_guard(&mut self.builder, known, exit);
            // Distinct bits: equal only as `null` and `undefined`.
            let nullish_pair = self.ins().band(left_nullish, right_nullish);
            self.ins().jump(join, &[ir::BlockArg::Value(nullish_pair)]);
            self.builder.switch_to_block(join);
            return Some(equal);
        }
        let family = self.ins().band_imm_u(left, ValueLayout::HEAP_FAMILY_MASK as i64);
        let right_family = self.ins().band_imm_u(right, ValueLayout::HEAP_FAMILY_MASK as i64);
        let one_family = self.ins().icmp(IntCC::Equal, family, right_family);
        let string = self.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_STRING_TAG as i64);
        let legacy = self.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::HEAP_TAG as i64);
        let contents = self.ins().bor(string, legacy);
        let compared = self.ins().band(one_family, contents);
        let unequal = self.ins().iconst(types::I8, 0);
        self.ins().brif(compared, cells, &[], join, &[ir::BlockArg::Value(unequal)]);
        self.builder.switch_to_block(cells);
        let result = self.helper_call(LaneHelper::StrictEqual, &[left, right]);
        let cells_equal = self.ins().ireduce(types::I8, result);
        self.ins().jump(join, &[ir::BlockArg::Value(cells_equal)]);
        self.builder.switch_to_block(join);
        Some(equal)
    }

    fn number_relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        let relation = super::operations::NumberRelation::from_operator(operator)?;
        if !self.operand_supported(left) || !self.operand_supported(right) || !self.numbers_supported(left, right) {
            return None;
        }
        let exit = self.guard_exit(state, pc);
        if let Some(domain) = self.number_domain(state, left, right) {
            return Some(self.number_relation_step(state, exit, relation, left, right, domain));
        }
        // Operands only their tags type compare as int32 where both tags say so, else as
        // Numbers; neither path proves a fact the other does not.
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.ins().band(left_int, right_int);
        let (int_block, number_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.ins().brif(ints, int_block, &[], number_block, &[]);
        self.builder.switch_to_block(int_block);
        let (left_bits, right_bits) = (self.ins().ireduce(types::I32, left), self.ins().ireduce(types::I32, right));
        let int_holds = emit_number_relation_operator(&mut self.builder, operator, left_bits, right_bits, NumberRelationDomain::Int32)?;
        self.ins().jump(join, &[ir::BlockArg::Value(int_holds)]);
        self.builder.switch_to_block(number_block);
        let (left, right) = (emit_js_tagged_number(&mut self.builder, left, exit), emit_js_tagged_number(&mut self.builder, right, exit));
        let number_holds = emit_number_relation_operator(&mut self.builder, operator, left, right, NumberRelationDomain::Float64)?;
        self.ins().jump(join, &[ir::BlockArg::Value(number_holds)]);
        self.builder.switch_to_block(join);
        Some(holds)
    }

    /// A Number relation of `domain`, whose operands its staged step proves.
    fn number_relation_step(&mut self, state: &mut State, miss: ir::Block,
        relation: super::operations::NumberRelation, left: Operand, right: Operand,
        domain: crate::staged::Domain) -> ir::Value {
        let operation = crate::staged::Operation::Number(crate::staged::NumberOperation::Relation { left, right, relation, domain });
        match self.step(state, frame::Edges::ValueGuarded { miss }, operation) {
            crate::staged::Step::Value(value) => value,
            _ => unreachable!("a relation produces a predicate"),
        }
    }

    /// `source`'s value into `destination`, with every fact proved of it.
    fn copy(&mut self, state: &mut State, destination: Register, source: Register) -> ir::Value {
        frame::register_copy(&mut self.builder, &mut self.registers, &mut self.written, state,
            destination, source)
    }

    /// The inlined call's receiver admitted where no dominating proof covers it: the body's own
    /// receiver recovery, a guard leaving at `exit`, but for a restartable numeric callee.
    fn guard_inlined_receiver(&mut self, state: &mut State, frame: usize, exit: ir::Block) {
        let inlined = self.unit.frames[frame];
        let receiver = self.tagged(state, inlined.receiver);
        let entry = self.entry.as_ref().expect("entry facts");
        let admitted = entry.call_receivers.contains(frame)
            || state.proofs.headers[usize::from(inlined.receiver)].is_some()
            || entry.records[usize::from(inlined.receiver)].is_some()
            || (state.proofs.receivers.contains(&inlined.receiver) && entry.records[state.values.len()].is_some());
        let callee = self.program.function(inlined.target.function as usize);
        let numeric = inlined.recovery == super::lane_unit::CallRecovery::RestartCall
            && restartable_numeric_function(self.program, callee, &callee.instructions().collect::<Vec<_>>());
        if !admitted && !numeric {
            let admitted = self.receiver_recovery(receiver, ReceiverRecovery::for_body(callee));
            emit_js_guard(&mut self.builder, admitted, exit);
        }
    }

    /// The call marker of inlined `frame` (`lane_unit::unit`): its callee is the function
    /// the call's site linked (an identity, compared) or a closure of the statically bound
    /// function (admitted by the VM's binding rule), and its receiver a direct object, which
    /// a plain call binds as `this`; a callee other than this frame's goes to the next
    /// alternative, and anything else leaves at the call, before its effects. The callee's
    /// parameters take the call's arguments and its other registers `undefined`, as its
    /// activation would (`lane_push_passed`).
    fn inline_call(&mut self, state: &mut State, pc: usize, frame: usize) -> bool {
        let inlined = self.unit.frames[frame];
        let exit = self.guard_exit(state, pc as u32);
        // An OSR region's callee comes from the entry snapshot: the caller's read is
        // resolved at compile time and dropped, and its register is reused by the loop.
        let callee = match inlined.callee_entry_word {
            Some(word) => self.loop_entry_word(word),
            None => self.tagged(state, inlined.callee_register),
        };
        let callee_index = usize::from(inlined.callee_register);
        let (same, proved) = match inlined.target.callee {
            // A dominating proof of the identity needs no compare.
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } if state.values[callee_index].value.ty.function_identity() == Some(identity)
                || self.entry.as_ref().expect("entry facts").calls.contains(frame) =>
                    (None, Some(Type::function(identity).expect("admitted callee locator"))),
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => (
                Some(self.is_identity(callee, identity)),
                Some(Type::function(identity).expect("admitted callee locator")),
            ),
            CalleeBinding::Function => (
                if state.values[callee_index].value.ty.body_index() == Some(inlined.target.function)
                    || self.entry.as_ref().expect("entry facts").calls.contains(frame) { None }
                    else { Some(self.function_binding(callee, inlined.target.function)) },
                Some(Type::function_body(inlined.target.function)),
            ),
        };
        if let Some(same) = same {
            match inlined.alternative {
                Some(next) => {
                    let (matched, other) = (self.builder.create_block(), self.builder.create_block());
                    self.ins().brif(same, matched, &[], other, &[]);
                    self.builder.switch_to_block(other);
                    let mut other_state = state.clone();
                    self.continue_at(&mut other_state, next);
                    self.builder.switch_to_block(matched);
                }
                None => emit_js_guard(&mut self.builder, same, exit),
            }
        }
        if let Some(ty) = proved {
            state.values[callee_index] = RegisterFact::admit(Fact {
                ty, representation: Representation::Tagged,
                binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact,
            }).expect("an admitted callee and its dominating check");
        }
        // A forwarded call is the call of the function it forwards to while its native is the one seen.
        if let Some((register, through)) = inlined.function_call {
            let native = self.tagged(state, register);
            let seen = self.is_identity(native, through);
            emit_js_guard(&mut self.builder, seen, exit);
        }
        let callee = self.program.function(inlined.target.function as usize);
        self.guard_inlined_receiver(state, frame, exit);
        for register in 0..inlined.registers {
            let destination = inlined.base + register;
            let undefined = self.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
            self.set_tagged(state, destination, undefined);
        }
        for (destination, source) in inlined.passed_registers(callee) {
            self.copy(state, destination, source);
        }
        true
    }

    /// Nullish receivers need no boxing only when the body ignores `this`.
    /// All other primitives retain interpreter recovery's boxing semantics.
    fn receiver_recovery(&mut self, receiver: ir::Value, recovery: ReceiverRecovery) -> ir::Value {
        let object = crate::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), receiver);
        match recovery {
            ReceiverRecovery::Object => object,
            ReceiverRecovery::ObjectOrNullish => {
                let undefined = self.ins().icmp_imm_u(IntCC::Equal, receiver, Value::UNDEFINED.to_bits() as i64);
                let null = self.ins().icmp_imm_u(IntCC::Equal, receiver, Value::NULL.to_bits() as i64);
                let nullish = self.ins().bor(undefined, null);
                self.ins().bor(object, nullish)
            }
        }
    }

    /// A call or a construction the body does not inline, by the staged call (`staged::invoke`):
    /// the body's registers written back, then a direct call of a compiled callee or the VM's
    /// call; its completion continues, a leave in the callee stops the body, and a miss leaves
    /// at `pc`. The callee may allocate, which may move the property arena, whose base and
    /// length the body reloads.
    fn call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        use crate::staged::{CallOperation, Operation};
        let decoded = Operation::<ir::Value, EmitProof>::decode(instruction, crate::staged::DecodeContext::of(self.program));
        let (Some(super::lane_unit::Origin::Caller(record)), Some(Operation::Call(CallOperation::Site(site)))) = (self.unit.origins.get(pc).copied(), decoded) else {
            self.exit(state, pc as u32);
            return false;
        };
        let missed = self.guard_exit(state, pc as u32);
        self.home_written(state, pc);
        let (completed, generic) = (self.builder.create_block(), self.builder.create_block());
        // SAFETY: the body owns the callee for as long as its code runs.
        let callee = self.direct.get(&pc).map(|callee| unsafe { &**callee });
        let call = CallSite { callee, record, context: self.context, registers: self.registers_base, completed, missed,
            stop: self.call_out_stop(), leave: self.leave_site() };
        self.step_with(state, frame::Edges::ValueGuarded { miss: generic }, Operation::Call(CallOperation::Site(site)), StepWith { site: Some(Site::Call(call)), ..StepWith::default() });
        self.reentries.push(completed);
        // A returning call's completion is its activation's: the lane completes it with the
        // result, which the call left in its register (`LaneExit::Returned`).
        if site.returns {
            self.reload_result(state, site.result);
            return self.return_register(state, record, site.result);
        }
        state.call_effect();
        self.reload_property_arena();
        self.reload_result(state, site.result);
        true
    }

    /// The activation completes with `register`, by the staged return (`Frame::ret`) at the
    /// record that completes it (`LaneExit::Returned`). Never falls through.
    fn return_register(&mut self, state: &mut State, record: u32, register: Register) -> bool {
        let site = Site::Return(ReturnSite { record, leave: self.leave_site() });
        self.step_with(state, frame::Edges::Value, crate::staged::Operation::Return { source: register }, StepWith { site: Some(site), ..StepWith::default() });
        self.finishes = true;
        false
    }

    /// The activation completes with `register`, which the record at `pc` names. Only a caller
    /// record can: an inlined callee's `Return` is the unit's move and jump.
    fn complete(&mut self, state: &mut State, pc: usize, register: Register) -> bool {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied() else {
            self.exit(state, pc as u32);
            return false;
        };
        self.native += 1;
        self.return_register(state, record, register)
    }

    /// Stops with the activation's completion: `value` in the result word, `record` the
    /// record that completes it (`LANE_BODY_RETURNED`).
    fn return_value(&mut self, record: u32, value: ir::Value) {
        self.finishes = true;
        let leave = self.leave_site();
        ReturnSite { record, leave }.complete(&mut self.builder, self.payload, value);
    }

    /// Where this run's completion goes (`Leave`).
    fn leave_site(&self) -> Leave {
        Leave { block: self.leave }
    }

    /// Leaves with `status`, and with `word` as the result word where a run publishes one.
    pub(super) fn leave_with(&mut self, status: ir::Value, word: Option<ir::Value>) {
        self.leave_site().with(&mut self.builder, status, word);
    }

    /// Count a taken execution of the region's backward edge in the loop entry record
    /// (`LoopEntry::backedge_count`), which the loop tier's pricing reads.
    /// A lane body (`Coverage::Prefix`) counts nothing.
    fn count_backedge(&mut self) {
        if !matches!(self.coverage, Coverage::Loop { .. }) { return; }
        let offset = i32::try_from(std::mem::offset_of!(super::JsStepDispatch, loop_entry))
            .expect("dispatch offset fits i32");
        let payload = self.payload;
        let entry = self.ins().load(types::I64, regions::dispatch(), payload, offset);
        let zero = self.ins().icmp_imm_u(IntCC::Equal, entry, 0);
        let (skip, count) = (self.builder.create_block(), self.builder.create_block());
        self.ins().brif(zero, skip, &[], count, &[]);
        self.builder.switch_to_block(count);
        let word = self.ins().load(types::I64, regions::miss_counter(), entry, 0);
        let next = self.ins().iadd_imm_s(word, 1);
        self.ins().store(regions::miss_counter(), next, entry, 0);
        self.ins().jump(skip, &[]);
        self.builder.switch_to_block(skip);
    }

    fn interrupt_check(&mut self, state: &mut State, pc: u32) {
        if self.fault_poll {
            let interrupt = self.interrupt;
            let proof = super::EmitProof::Safepoint { interrupt, miss: None, fault_poll: true };
            self.step(state, frame::Edges::Value, crate::staged::Operation::Safepoint { proof });
            return;
        }
        let interrupt = self.interrupt;
        let exit = self.guard_exit(state, pc);
        let proof = super::EmitProof::Safepoint { interrupt, miss: Some(exit), fault_poll: false };
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, crate::staged::Operation::Safepoint { proof });
    }

    fn emit_fault_poll(&mut self) {
        frame::calls::safepoint_parts(&mut self.builder, &mut self.polls,
            super::EmitProof::Safepoint { interrupt: self.interrupt, miss: None, fault_poll: true });
    }

    /// Joining edges must not move a guard, conversion or recovery across the
    /// interrupt check. Only already-conforming SSA views may share this latch;
    /// flag-based checks retain their exact-PC recovery on the original edge.
    fn shared_backedge(&mut self, state: &State, target: usize) -> bool {
        if !self.fault_poll || !self.shared_headers.contains(target) || !self.entered.contains(target) { return false; }
        let Some(block) = self.blocks.get(target).copied().flatten() else { return false };
        let Some(types) = self.facts.at(target) else { return false };
        if !types.iter().copied().enumerate().all(|(index, ty)| {
            if !self.is_live(target, index as Register) { return true; }
            let fact = state.values[index];
            fact.value.ty.is_subtype(ty) && if ty == Type::INT32 {
                fact.value.representation == Representation::Int32
            } else if ty == Type::NUMBER {
                fact.value.representation == Representation::Float64
            } else { fact.tagged.current() }
        }) { return false; }
        let latch = match self.poll_latches[block] {
            Some(latch) => latch,
            None => {
                let latch = self.builder.create_block();
                self.poll_latches[block] = Some(latch);
                latch
            }
        };
        self.finishes = true;
        self.ins().jump(latch, &[]);
        true
    }

    fn continue_branch(&mut self, state: &mut State, pc: usize, target: usize,
        taken_block: ir::Block, fall_block: ir::Block) {
        let back = target <= pc && target == self.coverage.entry();
        self.backedge |= back;
        if back { self.count_backedge(); }
        let refinements = std::mem::take(&mut state.refinements);
        self.switch_to(taken_block, true);
        let mut taken_state = state.clone();
        taken_state.narrow(crate::staged::Edge::Target, &refinements);
        self.continue_at(&mut taken_state, target);
        self.switch_to(fall_block, true);
        let mut fall_state = state.clone();
        fall_state.narrow(crate::staged::Edge::Following, &refinements);
        self.continue_at(&mut fall_state, pc + 1);
    }

    /// A family edge refines the value and existing SSA copies, not heap contents.
    fn refine_object_family(&mut self, state: &mut State, bits: ir::Value) {
        let bits = self.builder.func.dfg.resolve_aliases(bits);
        for fact in &mut state.values {
            let TaggedView::Value(value) = fact.tagged else { continue };
            if self.builder.func.dfg.resolve_aliases(value) == bits && !fact.value.ty.is_direct_object_family() {
                *fact = RegisterFact { tagged: fact.tagged,
                    ..RegisterFact::definition(fact.value.ty.direct_object_part().unwrap_or(Type::DIRECT_OBJECT_FAMILY), Representation::Tagged) };
            }
        }
    }

    /// Move-only latches can carry the nullish test's direct-object proof to
    /// the next field receiver. These are guarded input contracts, including
    /// the first iteration, never assumptions about values stored in a field.
    fn require_object_latches(&mut self, instructions: &[WideInstruction], leaders: &CompoundBitSet,
        loops: &[(usize, usize)]) {
        for &(header, back) in loops {
            if !instructions[back].op().is_jump()
                || self.unit.markers.iter().any(|pc| (header..=back).contains(pc)) { continue; }
            let first = instructions[header];
            if !first.op().is_get_field() || first.result_flags() != 0 { continue; }
            let Some(FieldLookup::Atom { base, .. }) = first.field_lookup() else { continue };
            let Some(receiver) = base.register_index() else { continue };
            let mut required = receiver;
            let mut inputs = smallvec::SmallVec::<[(usize, Register); INLINE_LOOP_INPUTS]>::new();
            inputs.push((header, receiver));
            let mut predicate = None;
            for pc in (header + 1..back).rev() {
                let instruction = instructions[pc];
                match instruction.op() {
                    opcode if opcode.is_move() && instruction.result_flags() == 0 => {
                        if instruction.register_a() == required { required = instruction.register_b(); }
                    }
                    opcode if opcode.is_nop() => {},
                    _ => { predicate = Some(pc); break; }
                }
                if leaders.contains(pc) { inputs.push((pc, required)); }
            }
            let Some(pc) = predicate else { continue };
            let Some(comparison) = nullish_comparison(&self.program.constants, instructions[pc], |_| Type::ANY)
                .filter(|comparison| !comparison.strict && comparison.negated
                    && instructions[pc].jump_target() as usize > back) else { continue };
            // Copies before the comparison share its SSA value as well.
            let root = |mut register| {
                for at in (header..pc).rev() {
                    if at + 1 < pc && leaders.contains(at + 1) { break; }
                    let instruction = instructions[at];
                    match instruction.op() {
                        opcode if opcode.is_move() && instruction.result_flags() == 0 => {
                            if instruction.register_a() == register { register = instruction.register_b(); }
                        }
                        opcode if opcode.is_nop() => {},
                        _ => break,
                    }
                }
                register
            };
            if root(required) != root(comparison.source) { continue; }
            if !inputs.iter().any(|&(leader, _)| leader == pc + 1) { inputs.push((pc + 1, required)); }
            if !inputs.iter().all(|&(pc, register)| self.facts.at(pc)
                .and_then(|types| types.get(usize::from(register)))
                .is_some_and(|ty| ty.is_subtype(Type::DIRECT_OBJECT_FAMILY)
                    || Type::DIRECT_OBJECT_FAMILY.is_subtype(*ty))) { continue; }
            for (pc, register) in inputs { self.facts.require(pc, register, Type::DIRECT_OBJECT_FAMILY); }
        }
    }

    /// Whether a loop definition of `register` of type `ty` makes it a candidate for an int32
    /// entry contract: the definition yields an int32, or the register held one at entry.
    fn int32_candidate(&self, register: Register, ty: Type) -> bool {
        ty == Type::INT32 || self.int_entries.iter().any(|(entry, _)| *entry == register)
    }

    /// An unchanged arm preserves a checked entry hypothesis. Every definition
    /// and backedge must preserve it too; failed hypotheses are removed before
    /// reanalysis so a dependent candidate cannot keep using their assumptions.
    fn refine_inductive_inputs(&mut self, initial: &mut [Type], instructions: &[WideInstruction],
        boundaries: &CompoundBitSet, loops: &[(usize, usize)],
        own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) {
        let header = self.coverage.entry();
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return };
        // Alternative markers add edges beyond the bytecode Nop transfer.
        // Keep their canonical lowering until this analysis models those edges.
        if self.unit.markers.iter().zip(&self.unit.frames).any(|(pc, frame)|
            (header..=back).contains(pc) && frame.alternative.is_some()) { return; }
        let mut candidates: CompoundBitSet = CompoundBitSet::with_capacity(initial.len());
        self.facts.for_each_definition(instructions, boundaries, &self.program.constants, &writes, self.native_calls,
            |pc, register, ty| {
                let index = usize::from(register);
                if (header..=back).contains(&pc) && register < self.unit.caller_registers
                    && self.is_live(header, register) && self.written.contains(index)
                    && matches!(initial[index], Type::ANY | Type::NUMBER) && self.int32_candidate(register, ty) {
                    candidates.insert(index);
                }
            });
        if let Some(facts) = super::facts::inductive_int32(initial, candidates, header..=back,
            instructions, boundaries, &self.program.constants, &writes, own_shape, self.native_calls, self.observed) {
            self.facts = facts;
        }
    }

    /// The registers a loop never writes whose every use proves them Numbers, each a Number header
    /// contract: proved once on the entering edge, so the body holds it as a double and tests it no more.
    fn number_inputs(&self, header: usize, back: usize, defined: &CompoundBitSet, candidates: &mut Vec<(super::facts::Contract, std::ops::RangeInclusive<usize>)>) {
        let Some(types) = self.facts.at(header) else { return };
        for instruction in &self.unit.instructions[header..=back] {
            for register in super::facts::proves_numbers(*instruction, types.len(), &self.program.constants, &writes, self.native_calls) {
                let index = usize::from(register);
                let contract = (header, register, Type::NUMBER);
                if !defined.contains(index) && !types[index].is_subtype(Type::NUMBER) && self.is_live(header, register)
                    && !candidates.iter().any(|(known, _)| *known == contract) {
                    candidates.push((contract, header..=back));
                }
            }
        }
    }

    /// An inner loop's counters and indices carried as int32s: a register its header joins as
    /// a Number (an index computed before the loop, say) while every definition in the loop
    /// yields an int32 takes an int32 header contract. The edge entering the loop proves it
    /// once (`conform`), instead of each use re-proving an exact int32 of a double.
    fn refine_loop_inputs(&mut self, entry: usize, initial: &[Type], instructions: &[WideInstruction],
        boundaries: &CompoundBitSet, loops: &[(usize, usize)],
        own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) {
        let mut candidates = Vec::new();
        for &(header, back) in loops.iter().filter(|(header, _)| *header != self.coverage.entry()) {
            let Some(types) = self.facts.at(header) else { continue };
            let mut defined: CompoundBitSet = CompoundBitSet::with_capacity(types.len());
            for instruction in &instructions[header..=back] {
                for register in writes(*instruction) { defined.insert(usize::from(register)); }
            }
            for (index, ty) in types.iter().enumerate() {
                if *ty == Type::NUMBER && defined.contains(index) && self.is_live(header, index as Register) {
                    candidates.push(((header, index as Register, Type::INT32), header..=back));
                }
            }
            self.number_inputs(header, back, &defined, &mut candidates);
        }
        if let Some(facts) = super::facts::loop_int32(entry, initial, candidates, instructions, boundaries,
            &self.program.constants, &writes, own_shape, self.native_calls, self.observed) {
            self.facts = facts;
        }
    }

    /// The type already proved for an operand at a use. Feedback is not enough:
    /// an admitted field read carries its actual loop-entry contract, even when
    /// the general analysis has no field feedback (as in linked AOT).
    fn int32_operand_at(&self, pc: usize, operand: Operand, boundaries: &CompoundBitSet) -> bool {
        if super::facts::operand_type(&self.program.constants, operand, |_| Type::ANY) == Type::INT32 { return true; }
        let Some(mut register) = operand.register_index() else { return false };
        if self.facts.before(pc, register, &self.unit.instructions, boundaries,
            &self.program.constants, &writes, self.native_calls) == Type::INT32 { return true; }
        for at in (0..pc).rev() {
            if boundaries.contains(at + 1) || self.unit.markers.contains(&at) { return false; }
            let instruction = self.unit.instructions[at];
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { return false; }
            if writes(instruction).contains(&register) {
                if instruction.op().is_move() && instruction.result_register() == register {
                    register = instruction.register_b();
                } else {
                    return instruction.defined_register() == Some(register)
                        && self.hoisted.get(&at).is_some_and(|read| read.ty == Type::INT32);
                }
            }
        }
        false
    }

    /// Whether the capture `instruction` reads held an int32 when this body compiled, so its
    /// entry guard takes the checked integer view (a guess the guard checks, as for `int_entries`).
    fn capture_observed_int(&self, instruction: WideInstruction) -> bool {
        instruction.capture_depth() == 0 && self.int_captures.contains(&crate::bytecode::ImmediateLayout::capture_slot(instruction.imm()))
    }

    /// A capture may choose the same checked integer view as a bitwise use, or
    /// as the already-proved integer on the other side of a Number relation.
    /// This selects an entry guard; it does not prove the capture's type.
    fn capture_int32_use(&self, pc: usize, boundaries: &CompoundBitSet) -> bool {
        let mut aliases: CompoundBitSet = CompoundBitSet::with_capacity(usize::from(self.unit.registers));
        aliases.insert(usize::from(self.unit.instructions[pc].result_register()));
        for at in pc + 1..self.unit.instructions.len() {
            if boundaries.contains(at) || self.unit.markers.contains(&at) { break; }
            let instruction = self.unit.instructions[at];
            let operator = match instruction.op() {
                opcode if opcode.is_jump_binary_false() => Some(instruction.binary_operator_field()),
                _ if instruction.op().js_denotation() == JsDenotation::NumericBinary => Some(instruction.binary_operator()),
                _ => None,
            };
            if let Some(operator) = operator {
                let operands = [instruction.operand_b(), instruction.operand_c()];
                let captured = |operand: Operand| operand.register_index()
                    .is_some_and(|register| aliases.contains(usize::from(register)));
                if operands.iter().copied().any(captured) {
                    if super::operations::NumberBitwise::from_operator(operator).is_some() { return true; }
                    if super::operations::NumberRelation::from_operator(operator).is_some()
                        && operands.iter().copied().enumerate().any(|(index, operand)| captured(operand)
                            && self.int32_operand_at(at, operands[1 - index], boundaries)) { return true; }
                }
            }
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { break; }
            let copied = instruction.op().is_move() && aliases.contains(usize::from(instruction.register_b()));
            for register in writes(instruction) { aliases.remove(usize::from(register)); }
            if copied { aliases.insert(usize::from(instruction.result_register())); }
            if aliases.is_empty() { break; }
        }
        false
    }

    /// The reads `header`'s loop repeats unchanged, made on its entry edge from `state`: a
    /// guard that fails there leaves at the header, before the loop runs.
    fn hoist_loop_entry(&mut self, state: &mut State, header: usize) {
        self.hoist_arrays(state, header, false);
        self.hoist_stable(state, header);
        self.hoist_reads(state, header);
        self.hoist_arrays(state, header, true);
    }

    /// Whether an enclosing loop's entry hoisted the capture read at `pc` already (its body holds
    /// this loop's, so the read is invariant there too): reading it again at each entry here would
    /// be a helper call inside the outer loop, spilling every value live across it.
    fn hoisted_by_enclosing_loop(&self, pc: usize, owner: Option<Register>) -> bool {
        owner.is_none() && self.unit.instructions[pc].op().is_load_capture() && self.hoisted.contains_key(&pc)
    }

    /// The hoisted read of the callee an inlined capture read belongs to, where it is the callee
    /// the frame's call binds: `Ok(None)` for a caller's own read, `Err` where the read is not
    /// admitted (its loop then reads it where it runs).
    fn admitted_callee_read(&mut self, pc: usize, reads: &[InvariantRead], boundaries: &CompoundBitSet) -> Result<Option<ir::Value>, ()> {
        let super::lane_unit::Origin::Inlined { frame, .. } = self.unit.origins[pc] else { return Ok(None) };
        let read = invariant_callee_read(self.unit, reads.iter().map(|read| read.pc), frame, boundaries)
            .and_then(|read| self.hoisted.get(&read).copied()).ok_or(())?;
        let target = self.unit.frames[frame].target;
        let admitted = match target.callee {
            CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => read.ty.function_identity() == Some(identity),
            CalleeBinding::Function => read.ty.body_index() == Some(target.function),
        };
        if admitted { Ok(Some(self.builder.use_var(read.value))) } else { Err(()) }
    }

    /// The loop's invariant captures and fields, read once on its entry edge.
    fn hoist_reads(&mut self, state: &mut State, header: usize) {
        let Some(reads) = self.hoists.get(&header).cloned() else { return };
        let boundaries = leaders(&self.unit.instructions);
        for InvariantRead { pc, owner, rooted } in reads.iter().copied() {
            let instruction = self.unit.instructions[pc];
            if self.hoisted_by_enclosing_loop(pc, owner) { continue; }
            let field = self.loop_fields.iter().position(|field| field.reads[0] == pc);
            let exit = self.guard_exit(state, header as u32);
            let (value, int32) = if instruction.op().is_load_capture() {
                let Ok(admitted_callee) = self.admitted_callee_read(pc, &reads, &boundaries) else { continue };
                let value = self.load_capture(state, pc, instruction, exit, admitted_callee, if rooted { RootedRead::OnEntry } else { RootedRead::No });
                let int32 = self.capture_int32_use(pc, &boundaries) || self.capture_observed_int(instruction);
                (value, int32)
            } else {
                let Some(FieldLookup::Atom { atom, base, cache_site }) = instruction.field_lookup() else { continue };
                let register = owner.or_else(|| base.register_index());
                // A `length` read is made once, natively where its base is a direct array.
                match register.filter(|_| super::names_length(self.program, atom)) {
                    Some(register) => {
                        let length = self.array_length(state, register, (atom, cache_site), exit);
                        (int_tagged(&mut self.builder, length), true)
                    }
                    None => {
                        let base = register.map_or(crate::bytecode::FieldBase::THIS, crate::bytecode::FieldBase::register);
                        let lookup = FieldLookup::Atom { atom, base, cache_site };
                        let Some(value) = self.field_value(state, (base, lookup), &[(atom, cache_site)], HeldRead::default(), exit) else { continue };
                        (value, int32_read(self.program, self.fields, instruction))
                    }
                }
            };
            let carried = field.is_some();
            let int32 = if carried {
                self.loop_fields[field.unwrap()].stores.iter().all(|store| {
                    self.facts.before(*store, self.unit.instructions[*store].register_a(),
                        &self.unit.instructions, &boundaries, &self.program.constants, &writes, self.native_calls) == Type::INT32
                })
            } else { int32 };
            if int32 {
                let integer = is_int_tagged(&mut self.builder, value);
                emit_js_guard(&mut self.builder, integer, exit);
            }
            let mut ty = if int32 { Type::INT32 } else { Type::ANY };
            if !int32 {
                // This is a candidate, never a proof from source syntax. Admit the
                // loaded closure on the same entry edge that already hoists its
                // immutable read. A failed admission exits before loop effects.
                let target = self.unit.markers.iter().enumerate().find_map(|(frame, marker)| {
                    let call = self.unit.frames[frame];
                    let supplies = if instruction.op().is_load_capture() {
                        // Preserve the existing caller-capture admission, including
                        // function-bound alternatives. Hoisting a callee's own
                        // captures additionally requires the monomorphic proof below.
                        let destination = instruction.result_register();
                        *marker > pc && call.callee_register == destination
                            && (call.target.callee == CalleeBinding::Function
                                || (call.alternative.is_none() && !self.unit.frames.iter()
                                    .any(|other| other.alternative == Some(*marker))))
                            && !self.unit.instructions[pc + 1..*marker].iter()
                                .any(|instruction| writes(*instruction).contains(&destination))
                    } else { invariant_callee_read(self.unit, [pc], frame, &boundaries) == Some(pc) };
                    supplies.then_some(call.target)
                });
                if let Some(target) = target {
                    let (matched, admitted) = match target.callee {
                        CalleeBinding::Function => (self.function_binding(value, target.function),
                            Type::function_body(target.function)),
                        CalleeBinding::Identity(identity) | CalleeBinding::Forwarded { callee: identity, .. } => (
                            self.is_identity(value, identity),
                            Type::function(identity).expect("admitted callee locator")),
                    };
                    emit_js_guard(&mut self.builder, matched, exit);
                    ty = admitted;
                }
            }
            let representation = if int32 { Representation::Int32 } else { Representation::Tagged };
            let raw = if representation == Representation::Int32 { self.ins().ireduce(types::I32, value) } else { value };
            let variable = self.builder.declare_var(if representation == Representation::Int32 { types::I32 } else { types::I64 });
            self.builder.def_var(variable, raw);
            let receiver = if instruction.op().is_get_field() {
                match instruction.field_lookup() {
                    Some(FieldLookup::Atom { base, .. }) => owner.or_else(|| base.register_index())
                        .map(|owner| (owner, state.values[usize::from(owner)].value.ty)),
                    _ => None,
                }
            } else { None };
            let stores: smallvec::SmallVec<[(usize, Option<FieldSlotBinding>); INLINE_FIELD_BINDINGS]> =
                field.into_iter().flat_map(|index| self.loop_fields[index].stores.iter()).filter_map(|store| {
                    if !int32 { return None; }
                    let site = self.unit.instructions[*store].cache_site_index();
                    let binding = self.entry.as_ref()?.fields.iter().find_map(|(known, binding)|
                        (*known == site).then_some(*binding));
                    Some((*store, binding))
                }).collect();
            let object_header = if !stores.is_empty() {
                receiver.map(|(owner, _)| self.object_header(state, Some(owner), false, exit))
            } else { None };
            if let (Some((owner, _)), Some(object_header)) = (receiver, object_header) {
                let object = self.tagged(state, owner);
                for (store, binding) in stores {
                    let instruction = self.unit.instructions[store];
                    let site = instruction.cache_site_index();
                    let mut binding = binding.unwrap_or_else(|| emit_js_field_binding(
                        &mut self.builder, self.payload, site, exit, None));
                    self.admit_field_writes(&mut binding, Type::INT32, exit);
                    let allowed = self.store_atom_admitted(instruction.atom_index());
                    emit_js_guard(&mut self.builder, allowed, exit);
                    let packed = self.builder.create_block();
                    let packed_address = self.builder.append_block_param(packed, types::I64);
                    let done = self.builder.create_block();
                    let address = self.builder.append_block_param(done, types::I64);
                    let (arena_address, _) = emit_js_cached_field_slot(&mut self.builder,
                        self.payload, object, site,
                        false, exit, Some(binding), Some(object_header), Some(packed));
                    self.ins().jump(done, &[ir::BlockArg::Value(arena_address)]);
                    self.builder.switch_to_block(packed);
                    self.ins().jump(done, &[ir::BlockArg::Value(packed_address)]);
                    self.builder.switch_to_block(done);
                    let packed_range = field.and_then(|index| {
                        let field = &self.loop_fields[index];
                        field.stores.iter().position(|pc| *pc == store).and_then(|index| field.store_ranges[index])
                    });
                    self.loop_stores.insert(store, LoopStore { header: object_header, address, packed_range });
                }
            }
            let read = LoopRead { value: variable, ty, representation, receiver, header: object_header };
            if let Some(index) = field {
                for pc in &self.loop_fields[index].reads { self.hoisted.insert(*pc, read); }
            } else { self.hoisted.insert(pc, read); }
        }
    }

    /// Nested sites retain callee coordinates; atom operands are already renamed by lane_unit.
    fn field_base(&self, pc: usize, instruction: WideInstruction) -> Option<crate::bytecode::FieldBase> {
        use crate::bytecode::FieldBase;
        match instruction.field_lookup()? {
            FieldLookup::Atom { base, .. } => Some(base),
            FieldLookup::Site(index) => {
                let base = self.program.field_sites.get(index)?.base;
                match self.unit.origins.get(pc)? {
                    super::lane_unit::Origin::Inlined { frame, .. } => {
                        let frame = self.unit.frames[*frame];
                        Some(FieldBase::register(if base == FieldBase::THIS { frame.receiver }
                            else { frame.base.checked_add(base.register_index()?)? }))
                    }
                    _ => Some(base),
                }
            }
        }
    }

    /// Prove the views of the arrays the loop at `header` indexes without redefining them, on
    /// its entering edge; a base that is no such array leaves at the header.
    fn hoist_arrays(&mut self, state: &mut State, header: usize, captured: bool) {
        let arrays: Vec<LoopArray> = self.loop_arrays.iter().copied()
            .filter(|array| array.header == header && matches!(array.source, ArraySource::Capture(_)) == captured).collect();
        if arrays.is_empty() { return; }
        let exit = self.guard_exit(state, header as u32);
        for array in arrays {
            let Some(bits) = self.hoisted_array_bits(state, array) else { continue };
            let variables = self.array_variables(array);
            self.builder.def_var(variables.bits, bits);
            match array.views {
                ElementViews::Dense => {
                    let view = super::emit_dense_array_view(&mut self.builder, self.payload, bits, array.writable, exit);
                    // Stores through the hoisted view then write the element alone.
                    if array.writable { super::emit_dense_array_uncounted(&mut self.builder, view); }
                    self.builder.def_var(variables.values, view.values);
                    self.builder.def_var(variables.length, view.length);
                    let unproved = self.ins().iconst(types::I32, 0);
                    self.builder.def_var(variables.room, unproved);
                }
                ElementViews::Typed(_) => self.hoist_typed_view(array, bits, variables, exit),
            }
        }
    }

    /// The tagged value of `array`'s source on the loop's entry edge: its register's, or its
    /// hoisted capture read's (none where that read was not hoisted tagged).
    fn hoisted_array_bits(&mut self, state: &mut State, array: LoopArray) -> Option<ir::Value> {
        match array.source {
            ArraySource::Register(register) => Some(self.tagged(state, register)),
            ArraySource::Capture(pc) => {
                let read = self.hoisted.get(&pc).copied().filter(|read| read.representation == Representation::Tagged)?;
                Some(self.builder.use_var(read.value))
            }
        }
    }

    /// The typed array `bits` holds, proved from its record once on the loop's entering edge
    /// (`prove_typed_view`): a proof that leaves takes the slow arm, where the VM publishes the
    /// record (`LaneHelper::TypedArrayView`) and the proof is made again, leaving at the loop's
    /// entry where it fails still.
    fn hoist_typed_view(&mut self, array: LoopArray, bits: ir::Value, variables: ArrayVariables, exit: ir::Block) {
        let access = ViewAccess { kind: array.views.observed_kind().and_then(NativeKind::new), writable: array.writable };
        let (publish, done) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(publish);
        self.prove_typed_view_into(variables, bits, access, publish);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(publish);
        self.builder.seal_block(publish);
        let published = self.helper_call(LaneHelper::TypedArrayView, &[bits]);
        let admitted = self.ins().icmp_imm_u(IntCC::NotEqual, published, 0);
        emit_js_guard(&mut self.builder, admitted, exit);
        self.prove_typed_view_into(variables, bits, access, exit);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// The proof of the typed array `bits` for `access`, leaving at `miss`, whose view the
    /// array's variables then hold.
    fn prove_typed_view_into(&mut self, variables: ArrayVariables, bits: ir::Value, access: ViewAccess, miss: ir::Block) {
        let records = TypedRecords {
            directory: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, typed_view_directory)),
            pages: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, typed_view_pages)),
            relocations: emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, buffer_relocations)),
        };
        let view = prove_typed_view(&mut super::object_slots::emit(&mut self.builder), records, bits, access, miss)
            .expect("the emit stage continues past its guards");
        self.builder.def_var(variables.values, view.values);
        self.builder.def_var(variables.length, view.length);
        if let ViewKind::Dynamic(kind) = view.kind { self.builder.def_var(variables.kind, kind); }
    }

    fn array_variables(&mut self, array: LoopArray) -> ArrayVariables {
        let builder = &mut self.builder;
        let length = if array.views == ElementViews::Dense { types::I32 } else { types::I64 };
        *self.array_vars.entry(array.source).or_insert_with(|| ArrayVariables {
            bits: builder.declare_var(types::I64),
            values: builder.declare_var(types::I64),
            length: builder.declare_var(length),
            kind: builder.declare_var(types::I64),
            room: builder.declare_var(types::I32),
        })
    }

    /// After the element store's helper, which may grow any record array's backing, through
    /// whichever register names it: every dense view hoisted around `pc` reads its backing
    /// again (`emit_dense_array_reload`), and its append room is proved again
    /// (`forget_append_rooms`). A typed region's views, which no such store moves, stay.
    fn reload_dense_arrays(&mut self, pc: usize) {
        self.forget_append_rooms(pc);
        let arrays: Vec<LoopArray> = self.loop_arrays.iter().copied()
            .filter(|array| array.views == ElementViews::Dense && (array.header..=array.back).contains(&pc)).collect();
        for array in arrays {
            let variables = self.array_variables(array);
            let bits = self.builder.use_var(variables.bits);
            let (values, length) = super::emit_dense_array_reload(&mut self.builder, self.payload, bits);
            // A grown backing may count its values again; stores through the view keep none.
            if array.writable {
                super::emit_dense_array_uncounted(&mut self.builder, super::DenseArrayView { values, length, writable: true, uncounted: false });
            }
            self.builder.def_var(variables.values, values);
            self.builder.def_var(variables.length, length);
        }
    }

    /// The hoisted view's elements' address and count.
    fn hoisted_view(&mut self, array: LoopArray) -> (ir::Value, ir::Value) {
        let variables = self.array_variables(array);
        (self.builder.use_var(variables.values), self.builder.use_var(variables.length))
    }

    /// A typed region's hoisted array kind: the site's, which its proof compared once, or the record's.
    fn hoisted_kind(&mut self, array: LoopArray) -> ViewKind<ir::Value> {
        match array.views.observed_kind().and_then(NativeKind::new) {
            Some(native) => ViewKind::Static(native),
            None => {
                let variables = self.array_variables(array);
                ViewKind::Dynamic(self.builder.use_var(variables.kind))
            }
        }
    }

    /// A Number's store through a hoisted dense array view at a site that left (`left_here`): in place,
    /// else appended inline (`dense_append`), else the VM's store. Any other hoisted store is its row.
    fn dense_element_write(&mut self, state: &mut State, array: LoopArray, pc: u32, instruction: WideInstruction,
        exit: ir::Block) {
        let index = self.int(state, instruction.register_c(), exit);
        let (values, length) = self.hoisted_view(array);
        let in_bounds = self.ranges.in_bounds(pc as usize);
        let value = self.tagged(state, instruction.register_a());
        let base = self.tagged(state, instruction.register_b());
        let key = self.tagged(state, instruction.register_c());
        let (slow, done) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(slow);
        let view = super::DenseArrayView { values, length, writable: true, uncounted: true };
        // The view's backing counts no values (`hoist_arrays`): the store writes the element.
        self.index_write(state, index, value, view, slow, in_bounds);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(slow);
        self.builder.seal_block(slow);
        self.dense_append(array, pc, index, value, base, done);
        let proof = super::EmitProof::IndexSetHelper { miss: exit };
        self.step(state, frame::Edges::ValueGuarded { miss: exit },
            crate::staged::Operation::IndexSetHelper { base, key, value, proof });
        self.reload_dense_arrays(pc as usize);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// The reads of `instruction`'s lookup, one per site, and the sink a nested read of the
    /// current `this` also stores to.
    fn field_chain(&self, instruction: WideInstruction) -> Option<(Vec<(Atom, u16)>, Option<(Atom, u16)>)> {
        Some(match instruction.field_lookup()? {
            FieldLookup::Atom { atom, cache_site, .. } => (vec![(atom, cache_site)], None),
            FieldLookup::Site(index) => {
                let site = *self.program.field_sites.get(index)?;
                let reads = [Some(site.first), site.second].into_iter().flatten().collect();
                (reads, instruction.writes_current_this().then_some(site.sink).flatten())
            }
        })
    }

    /// The owner and name of a plain read of `instruction`'s lookup (no sink, no completion):
    /// `None` where the lookup names a site the program does not have.
    fn plain_field(&self, instruction: WideInstruction, owner: Option<Register>, plain: bool) -> Option<Option<(Option<Register>, Atom)>> {
        Some(match instruction.field_lookup() {
            Some(FieldLookup::Atom { atom, .. }) if plain => Some((owner, atom)),
            Some(FieldLookup::Site(index)) if plain => Some((owner, self.program.field_sites.get(index)?.first.0)),
            _ => None,
        })
    }

    /// A named read: what the instruction does around its value (completing the activation,
    /// storing a sink, a direct array's `length`, a whole-program prototype value, the proofs
    /// the value carries), with the value itself one `GetField` row (`field_value`).
    fn get_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        // A returning read completes the activation with its value, at a caller record; an
        // inlined callee's is the unit's read, move and jump (`lane_unit`).
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) => None,
            (true, Some(super::lane_unit::Origin::Caller(record))) if self.completion.status_and_pc() => Some(record),
            (true, _) => return None,
        };
        // A nested lookup is a chain of reads from one base, each through its own site; a
        // guard that fails anywhere in it leaves at the instruction, whose reads have no effect.
        // A nested lookup that writes the current `this` stores the chain's value to `this`'s
        // sink field too, after its reads (`FieldSite::sink`).
        // An inlined frame's nested site names its base, and its sink's `this`, by the callee's
        // registers: they are the frame's (`lane_unit::Frame`).
        let frame = match self.unit.origins.get(pc) {
            Some(super::lane_unit::Origin::Inlined { frame, .. }) => Some(self.unit.frames[*frame]),
            _ => None,
        };
        let base = self.field_base(pc, instruction)?;
        let (parts, sink) = self.field_chain(instruction)?;
        let sink_owner = frame.map(|frame| frame.receiver);
        if instruction.writes_current_this() && sink.is_none() {
            return None;
        }
        let register = base.register_index();
        if register.is_some_and(|register| register >= self.unit.registers)
            || (base != crate::bytecode::FieldBase::THIS && register.is_none())
        {
            return None;
        }
        let destination = instruction.result_register();
        let lookup = instruction.field_lookup()?;
        // An OSR region's static field read answers from the entry record, as the verified
        // lowering's does: the value the field held at entry, which the loop does not write.
        if sink.is_none() && completes.is_none() && !instruction.writes_current_this()
            && let Some(Some(word)) = self.entry_fields.get(pc).copied()
        {
            let exit = self.guard_exit(state, pc as u32);
            let held = HeldRead { entry_word: Some(word), ..HeldRead::default() };
            let bits = self.field_value(state, (base, lookup), &[], held, exit)?;
            self.set_tagged(state, destination, bits);
            return Some(true);
        }
        let exit = self.guard_exit(state, pc as u32);
        let (first, rest) = parts.split_first()?;
        // A direct array's `length` is its record's, an int32.
        if let (Some(register), [], None, None) = (register, rest, sink, completes)
            && super::names_length(self.program, first.0)
        {
            let length = self.array_length(state, register, *first, exit);
            self.set_int(state, destination, length);
            return Some(true);
        }
        let field = self.plain_field(instruction, register, sink.is_none() && completes.is_none())?;
        // A whole-program prototype value: read the field once here as data, require the
        // value the closed program gives it, then fold it. A changed prototype fails the
        // guard and leaves to the shared body.
        if rest.is_empty() && sink.is_none() && completes.is_none()
            && !instruction.writes_current_this()
            && self.fold_frozen_field(state, (base, lookup), *first, destination, exit)
        {
            return Some(true);
        }
        let known = field.and_then(|(owner, atom)| state.proofs.fields.iter().copied()
            .find(|field| field.owner == owner && field.atom == atom));
        #[cfg(test)] if known.is_some() { record_field_forward(first.1); }
        let held = HeldRead { known: known.map(|field| field.value), ..HeldRead::default() };
        let value = self.field_value(state, (base, lookup), &parts, held, exit)?;
        if let Some((atom, site)) = sink {
            self.sink_store(state, sink_owner, (atom, site), value, exit)?;
        }
        if let Some(record) = completes {
            self.return_value(record, value);
            return Some(false);
        }
        self.define_field_read(state, instruction, value, (field, rest.is_empty(), known), exit);
        Some(true)
    }

    /// The value `base`'s lookup reads through `links`, the sites of its chain: one `GetField`
    /// row under the plans their caches proved, or the value `held` already has. A guard of it
    /// that fails leaves through `exit`.
    fn field_value(&mut self, state: &mut State, (base, lookup): (crate::bytecode::FieldBase, FieldLookup), links: &[(Atom, u16)],
        held: HeldRead, exit: ir::Block) -> Option<ir::Value> {
        let mut inputs: Vec<LinkInput> = links.iter().map(|link| self.link_input(*link)).collect();
        if let Some(first) = inputs.first_mut() {
            first.probed |= self.inherited_off_record(state, base, &first.loads);
        }
        let folds = self.division().folds(crate::staged::division::INHERITED_FIELD);
        if folds { field_plans::embed_inherited(&inputs, &mut self.stable.roots); }
        with_read_plan(&inputs, (folds, held.known.is_some()), |plans| {
            let edges = frame::Edges::ValueGuarded { miss: exit };
            match self.field_step(state, edges, crate::staged::Operation::GetField { base, lookup }, (plans, held)) {
                crate::staged::Step::Value(value) => Some(value),
                _ => None,
            }
        })
    }

    /// A chain link's site, with the loads its cache proved and whether its misses probe.
    fn link_input(&self, (atom, site): (Atom, u16)) -> LinkInput {
        let loads = site_loads(self.fields, site).cloned().unwrap_or_default();
        let probed = self.probes_field_misses(&loads);
        LinkInput { atom, site, loads, probed }
    }

    /// The `length` read through `read` of the value `register` holds, as an int32: a direct
    /// array's from its record (`emit_array_length_read`), a flat string's from its own
    /// (`string_length`), any other's the VM's read, which leaves through `exit` where it
    /// misses or is not an int32.
    fn array_length(&mut self, state: &mut State, register: Register, read: (Atom, u16), exit: ir::Block) -> ir::Value {
        let base = self.tagged(state, register);
        let (generic, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(generic);
        let length = self.builder.append_block_param(join, types::I32);
        let native = super::emit_array_length_read(&mut self.builder, self.payload, base, generic);
        self.ins().jump(join, &[ir::BlockArg::Value(native)]);
        self.builder.switch_to_block(generic);
        let probe = self.builder.create_block();
        self.builder.set_cold_block(probe);
        let string = self.string_length(base, probe);
        self.ins().jump(join, &[ir::BlockArg::Value(string)]);
        self.builder.switch_to_block(probe);
        let (atom, site) = read;
        let probe = crate::staged::Operation::FieldGetHost { object: base, atom, site };
        let crate::staged::Step::Value(value) = self.field_step(state, frame::Edges::ValueGuarded { miss: exit }, probe,
            (&LanePlans::NONE, HeldRead::default())) else { unreachable!("the lane's field read is a host call") };
        let integer = is_int_tagged(&mut self.builder, value);
        emit_js_guard(&mut self.builder, integer, exit);
        let int = self.ins().ireduce(types::I32, value);
        self.ins().jump(join, &[ir::BlockArg::Value(int)]);
        self.builder.switch_to_block(join);
        length
    }

    /// Whether the read `instruction` makes has an own-field entry to fold: a read of prototype
    /// holders only (a method of an array, a `Map`) proves no record at entry, which such a
    /// receiver would fail at every run, and asks the VM's entries on a guard miss instead.
    fn reads_own_field(&self, instruction: WideInstruction) -> bool {
        let Some(FieldLookup::Atom { cache_site, .. }) = instruction.field_lookup() else { return true };
        site_loads(self.fields, cache_site).is_none_or(|loads| loads.iter().any(|load| load.holder.is_none()))
    }

    /// Whether `register`, whose type no object proof covers, may hold a receiver the object
    /// table has no record of (a `Map`'s, a `Set`'s) at a read of inherited constants: its miss
    /// would leave at every entry, so it asks the VM's proved inherited entry instead.
    fn inherited_off_record(&self, state: &State, base: crate::bytecode::FieldBase, shapes: &[NativeFieldLoad]) -> bool {
        shapes.iter().any(|load| load.inherited.is_some())
            && base.register_index().is_some_and(|register| !state.proofs.receivers.contains(&register)
                && !state.values[usize::from(register)].value.ty.is_direct_object_family())
    }

    /// Whether a read of these shapes asks the VM's caches on a guard miss rather than leaves.
    fn probes_field_misses(&self, shapes: &[NativeFieldLoad]) -> bool {
        self.left_here || self.left_a_field_read() || NativeFieldLoad::probes_misses(shapes)
    }

    /// Whether a previous body left at some field read of this unit: its shapes were not all
    /// seen when it was planned, so every read now asks the VM's caches on a guard miss rather
    /// than leave, as this unit's other reads would leave one recompile at a time.
    fn left_a_field_read(&self) -> bool {
        self.generic.iter().any(|pc| self.unit.instructions.get(*pc).is_some_and(|instruction| instruction.op().is_get_field()))
    }

    /// Word `word` of the loop's entry record (`frame_fields::entry_record_word`).
    fn loop_entry_word(&mut self, word: u32) -> ir::Value {
        frame_fields::entry_record_word(&mut self.builder, self.payload, word)
    }

    /// A dominating proof of `owner`'s object (`headers::object_header`).
    fn object_header(&mut self, state: &mut State, owner: Option<Register>, read: bool, exit: ir::Block) -> ObjectHeader {
        let entry = self.entry.as_ref().expect("entry facts");
        headers::object_header(&mut self.builder, &mut self.registers, state, (entry, self.payload), owner, read, exit)
    }

    fn set_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_a();
        if source >= self.unit.registers || instruction.writes_current_this() || instruction.returns_from_frame() {
            return None;
        }
        let register = self.store_owner(pc, instruction)?;
        let carried = match self.loop_fields.iter().find(|field| field.stores.contains(&pc)) {
            Some(field) => Some(*self.hoisted.get(&field.reads[0])?),
            None => None,
        };
        let exit = self.guard_exit(state, pc as u32);
        let carried_int = carried.filter(|read| read.representation == Representation::Int32)
            .map(|_| self.int(state, source, exit));
        let ty = state.values[usize::from(source)].value.ty;
        let operation = crate::staged::Operation::SetField { value: source, base: register,
            atom: instruction.atom_index(), site: instruction.cache_site_index() };
        // A store a previous body left at may add its property (`compiled::lane_field_add`),
        // which changes the object's shape and may move the property arena: every proof of
        // an object retires, as another register may name it, and `this`'s facts are read
        // again.
        if self.generic.contains(&pc) && self.completion.status_and_pc() {
            self.store_step(state, operation, (StorePlan::Add, HeldRead::default()), exit)?;
            self.after_store(state, StoreJoin::Add);
            return Some(true);
        }
        let (atom, site) = (instruction.atom_index(), instruction.cache_site_index());
        let slot = self.projected_slot(state, pc, (register, ty));
        if slot.is_some() {
            state.proofs.write_field(atom);
            self.store_step(state, operation, (StorePlan::Entry, HeldRead { slot, ..HeldRead::default() }), exit)?;
        } else {
            self.site_store(state, operation, (atom, site, ty), exit)?;
        }
        if let Some(read) = carried {
            self.carry_store(state, source, (read, carried_int));
        }
        Some(true)
    }

    /// The loop-carried variable of the field a store writes takes the stored value: the int32
    /// view proved before the store where the carried read is unboxed, else the tagged word.
    fn carry_store(&mut self, state: &mut State, source: Register, (read, int): (LoopRead, Option<ir::Value>)) {
        let bits = int.unwrap_or_else(|| self.tagged(state, source));
        self.builder.def_var(read.value, bits);
        #[cfg(test)] if read.representation == Representation::Int32 {
            LOOP_INT32_PHIS.with(|count| count.set(count.get() + 1));
        }
    }

    /// The object `instruction` stores to: a register, or the fixed `this` for `None`; `None`
    /// for a register the unit does not have.
    fn store_owner(&self, pc: usize, instruction: WideInstruction) -> Option<Option<Register>> {
        let register = if let Some(register) = instruction.field_store_register() {
            Some(register)
        } else if let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc) {
            Some(self.unit.frames[*frame].receiver)
        } else { None };
        (!register.is_some_and(|register| register >= self.unit.registers)).then_some(register)
    }

    /// The slot a loop's entry projected for the int32 store at `pc` (`LoopStore`), where the
    /// store's object is the record the projection addresses.
    fn projected_slot(&self, state: &State, pc: usize, (register, ty): (Option<Register>, Type)) -> Option<LoopStore> {
        self.loop_stores.get(&pc).copied().filter(|slot| ty.is_subtype(Type::INT32)
            && state.proofs.headers[state.header_index(register)].is_some_and(|header| header.record == slot.header.record))
    }

    /// The named store of `operation` through its site's plan: `ty` is the stored value's type,
    /// and the site's own shapes, binding and completion decide how it writes (`site_store_plan`).
    fn site_store(&mut self, state: &mut State, operation: crate::staged::Operation<ir::Value, EmitProof>,
        (atom, site, ty): (Atom, u16, Type), exit: ir::Block) -> Option<()> {
        let (arms, join) = self.site_store_plan(site, ty);
        let plan = StorePlan::of(&arms, join);
        let admitted = (!arms.is_empty() || join == StoreJoin::Bound).then(|| self.store_atom_admitted(atom));
        state.proofs.write_field(atom);
        self.store_step(state, operation, (plan, HeldRead { admitted, ..HeldRead::default() }), exit)?;
        self.after_store(state, join);
        Some(())
    }

    /// The store of `value`, a word no register names, to the sink field `atom` of `owner`'s
    /// object (`this` for `None`) that a nested lookup writes (`FieldSite::sink`), through its
    /// site's plan. It is the only store of a value with no register, so it runs the row's word
    /// form (`Operation::StoreField`).
    fn sink_store(&mut self, state: &mut State, owner: Option<Register>, (atom, site): (Atom, u16), value: ir::Value,
        exit: ir::Block) -> Option<()> {
        let operation = crate::staged::Operation::StoreField { base: owner, value, fact: Type::ANY, atom, site };
        self.site_store(state, operation, (atom, site, Type::ANY), exit)
    }

    /// The in-place arms and the VM's completion of a store of a value of type `ty` through
    /// `site`, which the row runs (`StorePlan::of`): one arm for each of the site's own shapes
    /// whose layout the plan admits and whose slot the value's type fits or widens by a test
    /// (`StoreArm::of`); the VM's own-field hit where no arm takes the store, through the slot
    /// the entry bound for the site where there is one; and in a body that completes through
    /// the VM's polymorphic entries (`status_and_pc`) a site of several shapes ends in its
    /// adding entry.
    fn site_store_plan(&self, site: u16, ty: Type) -> (Vec<StoreArm>, StoreJoin) {
        let own: Vec<&NativeFieldLoad> = site_loads(self.fields, site).into_iter().flatten().filter(|load| load.holder.is_none()).collect();
        let bound = self.entry.as_ref().is_some_and(|entry| entry.fields.iter().any(|(known, _)| *known == site));
        let arms = own.iter().filter(|load| load.valid())
            .filter_map(|load| StoreArm::of((load.shape, load.slot), load.ty, ty)).collect();
        let join = if own.len() > 1 && self.completion.status_and_pc() { StoreJoin::Add }
            else if bound { StoreJoin::Bound } else { StoreJoin::Host };
        (arms, join)
    }

    /// What a store the VM may have completed changes: an adding entry reshapes any object, so
    /// every proof of one retires and the receiver's facts load again; the VM's own-field hit
    /// writes a field, which a prototype's Stable facts may have named.
    fn after_store(&mut self, state: &mut State, join: StoreJoin) {
        self.forget_append_rooms(self.at);
        match join {
            StoreJoin::Add => {
                self.reshaped(state);
                self.reload_receiver();
            }
            StoreJoin::Host | StoreJoin::Bound => state.proofs.stable = false,
        }
    }

    /// The store `operation` as one row step under `plan` and the `held` facts of its access.
    fn store_step(&mut self, state: &mut State, operation: crate::staged::Operation<ir::Value, EmitProof>,
        (plan, held): (StorePlan, HeldRead), exit: ir::Block) -> Option<()> {
        let plans = LanePlans { store: Some(plan), ..LanePlans::NONE };
        match self.field_step(state, frame::Edges::ValueGuarded { miss: exit }, operation, (&plans, held)) {
            crate::staged::Step::Value(_) => Some(()),
            _ => None,
        }
    }

    /// The receiver's shape and storage kind again, and the entry's tests of its shape, after
    /// a store that may have changed them (`emit_lane_receiver`, `Lowering::entry_facts`).
    fn reload_receiver(&mut self) {
        use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
        let entry = self.entry.as_ref().expect("entry facts");
        let (object_var, shape_var, inline_var) = (entry.receiver.object, entry.receiver.shape, entry.receiver.inline);
        let tests: Vec<(u32, Variable)> = entry.receiver_shapes.iter().map(|(shape, variable)| (*shape, *variable)).collect();
        let object = self.builder.use_var(object_var);
        let (found, done) = (self.builder.create_block(), self.builder.create_block());
        let present = self.ins().icmp_imm_u(IntCC::NotEqual, object, 0);
        self.ins().brif(present, found, &[], done, &[]);
        self.builder.switch_to_block(found);
        let shape = self.ins().load(types::I32, regions::shape_word(), object, OBJECT.auxiliary_offset as i32);
        let inline = super::object_slots::inline_storage(&mut self.builder, object);
        self.builder.def_var(shape_var, shape);
        self.builder.def_var(inline_var, inline);
        self.ins().jump(done, &[]);
        self.builder.switch_to_block(done);
        let shape = self.builder.use_var(shape_var);
        for (expected, variable) in tests {
            let same = self.ins().icmp_imm_u(IntCC::Equal, shape, i64::from(expected));
            self.builder.def_var(variable, same);
        }
    }

    /// Whether the run admits a direct store of `atom`. A run that calls may meet a method
    /// cache that names the atom only after a call; one that does not tests it once, at
    /// entry (`Lowering::entry_facts`).
    fn store_atom_admitted(&mut self, atom: Atom) -> ir::Value {
        if self.native_calls {
            self.emit_store_atom_admitted(atom)
        } else {
            self.builder.use_var(self.entry.as_ref().expect("entry facts").store_atoms[&atom])
        }
    }

    /// `+source` or `-source` of a Number: the shared Number unary denotation, with the
    /// source's representation. A non-Number source leaves at its exact PC for the general
    /// coercion, and an int32 negation that overflows widens and leaves.
    fn unary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if instruction.result_flags() != 0 || instruction.writes_current_this() {
            return None;
        }
        let operation = crate::vm::operations::NumberUnary::from_js_instruction(
            instruction.op(), instruction.unary_operator())?;
        let source = instruction.register_b();
        let domain = if state.values[usize::from(source)].value.ty.is_subtype(Type::INT32) {
            crate::staged::Domain::Int32
        } else {
            crate::staged::Domain::Float64
        };
        let exit = self.guard_exit(state, pc as u32);
        let operation = crate::staged::Operation::Number(crate::staged::NumberOperation::Unary {
            destination: instruction.result_register(),
            source: crate::bytecode::Operand::register(source),
            operation, domain,
        });
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
        Some(true)
    }

    fn binary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let returns = instruction.returns_from_frame();
        if instruction.writes_current_this() || (instruction.result_flags() != 0 && !returns) {
            return None;
        }
        // A returning result completes the activation from its own register, at its caller
        // record (`Lowering::complete`); an inlined callee's is the unit's binary, move and
        // jump (`lane_unit`). A jump to the function's dead terminal `Return` instead would
        // leave there: no block starts at it.
        if returns && !matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let destination = instruction.result_register();
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if !self.operand_supported(left) || !self.operand_supported(right) {
            return None;
        }
        let operator = instruction.binary_operator();
        let arithmetic = super::operations::NumberArithmetic::from_js_instruction(instruction.op(), operator);
        let bitwise = super::operations::NumberBitwise::from_js_instruction(instruction.op(), operator);
        // `%` is the shared Number remainder; the opcode's denotation is the catalog's, so the
        // meaning is not a second hand-written `Binary` arm.
        let remainder = instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
            && operator == super::operations::REMAINDER;
        // Arithmetic takes Number constants; a relation decides its own (`relation`).
        if (arithmetic.is_some() || bitwise.is_some() || remainder) && [left, right].iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
            && self.program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate).and_then(Value::as_number).is_none()) {
            return None;
        }
        let exit = self.guard_exit(state, pc as u32);
        if let Some(arithmetic) = arithmetic
            && arithmetic.int32_projection_is_wrapping() && !self.left_here
            && instruction.result_flags() == 0
            && let Some(next) = self.unit.instructions.get(pc + 1).copied()
            && next.result_flags() == 0
            && next.op().is_binary()
            && super::operations::NumberBitwise::from_js_instruction(next.op(), next.binary_operator())
                == Some(super::operations::NumberBitwise::Or)
            && next.operand_b().register_index() == Some(destination)
            && super::facts::constant_number(&self.program.constants, next.operand_c()) == Some(0.0)
            && self.blocks[pc + 1].is_none()
            && (next.result_register() == destination || !self.is_live(pc + 2, destination))
        {
            // No source edge or observer can see the intermediate Number.
            // A failed input proof resumes before the arithmetic, and the
            // consumer cannot fail once these signed-int32 inputs are proved.
            let left = self.operand_int(state, left, exit)?;
            let right = self.operand_int(state, right, exit)?;
            let bits = arithmetic.apply(&mut EmitWord32Arithmetic { builder: &mut self.builder }, left, right);
            self.set_int(state, next.result_register(), bits);
            self.consumed.insert(pc + 1);
            return Some(true);
        }
        if remainder {
            return Some(self.remainder(state, destination, [left, right], exit));
        }
        match (arithmetic, bitwise) {
            (Some(arithmetic), _) => {
                let float_operand = [left, right].iter().any(|operand| match operand.kind() {
                    Some(OperandKind::Register) => state.values[usize::from(operand.payload())].value.representation == Representation::Float64,
                    Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                        .and_then(super::facts::immediate).is_some_and(|value| !super::facts::is_int32(value)),
                    _ => false,
                }) || self.left_here;
                // Operands proved int32 stay int32, as C keeps them: an overflow, or a product's
                // negative zero, is a value no int32 holds and leaves, rather than every result
                // becoming a double that the next operation converts back.
                let proved_int = self.proved_int(state, [left, right]);
                if arithmetic == super::operations::NumberArithmetic::Divide || float_operand {
                    let operation = crate::staged::Operation::Number(crate::staged::NumberOperation::Arithmetic { destination, left, right,
                        operation: arithmetic, domain: crate::staged::Domain::Float64,
                        overflow: crate::staged::Overflow::Exit });
                    self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
                } else if proved_int {
                    // A result the unit's ranges keep inside int32, or a sum or difference only
                    // `| 0` reads (its ToInt32), is the wrapping int32 result: no overflow exit.
                    let overflow = if self.ranges.overflow_free(pc)
                        || (arithmetic.int32_projection_is_wrapping() && self.truncated(pc, destination)) {
                        crate::staged::Overflow::Wrap
                    } else {
                        crate::staged::Overflow::Exit
                    };
                    let operation = crate::staged::Operation::Number(crate::staged::NumberOperation::Arithmetic { destination, left, right,
                        operation: arithmetic, domain: crate::staged::Domain::Int32, overflow });
                    self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
                } else if arithmetic.int32_projection_is_wrapping()
                    && self.truncated(pc, destination) {
                    // Only its ToInt32 is read: int32 operands wrap, any other numbers take
                    // ToInt32 of their double result, and both join as an int32.
                    let float = self.builder.create_block();
                    self.builder.set_cold_block(float);
                    let mut float_state = state.clone();
                    let left_int = self.operand_int(state, left, float).expect("supported operand");
                    let right_int = self.operand_int(state, right, float).expect("supported operand");
                    let wrapped = if arithmetic == super::operations::NumberArithmetic::Add {
                        self.ins().iadd(left_int, right_int)
                    } else {
                        self.ins().isub(left_int, right_int)
                    };
                    let join = self.builder.create_block();
                    let int = self.builder.append_block_param(join, types::I32);
                    self.ins().jump(join, &[ir::BlockArg::Value(wrapped)]);
                    self.builder.switch_to_block(float);
                    let left_number = self.operand_number(&mut float_state, left, exit)?;
                    let right_number = self.operand_number(&mut float_state, right, exit)?;
                    let number = emit_number_arithmetic_operation(&mut self.builder, arithmetic, left_number, right_number);
                    let truncated = self.number_to_int32(number, exit);
                    self.ins().jump(join, &[ir::BlockArg::Value(truncated)]);
                    self.builder.switch_to_block(join);
                    self.join_states(state, &float_state, pc);
                    self.set_int(state, destination, int);
                } else {
                    let float = self.builder.create_block();
                    self.builder.set_cold_block(float);
                    let mut float_state = state.clone();
                    let left_int = self.operand_int(state, left, float).expect("supported operand");
                    let right_int = self.operand_int(state, right, float).expect("supported operand");
                    let (result, overflow) = arithmetic.apply(&mut EmitArithmetic::<true> { builder: &mut self.builder }, left_int, right_int);
                    let int_done = self.builder.create_block();
                    self.ins().brif(overflow, float, &[], int_done, &[]);
                    self.builder.switch_to_block(int_done);
                    if arithmetic == super::operations::NumberArithmetic::Multiply {
                        let zero = self.ins().icmp_imm_u(IntCC::Equal, result, 0);
                        let signs = self.ins().bor(left_int, right_int);
                        let negative = self.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
                        let negative_zero = self.ins().band(zero, negative);
                        emit_js_guard_not(&mut self.builder, negative_zero, float);
                    }
                    let join = self.builder.create_block();
                    let number = self.builder.append_block_param(join, types::F64);
                    let result = self.ins().fcvt_from_sint(types::F64, result);
                    self.ins().jump(join, &[ir::BlockArg::Value(result)]);
                    self.builder.switch_to_block(float);
                    let left_number = self.operand_number(&mut float_state, left, exit)?;
                    let right_number = self.operand_number(&mut float_state, right, exit)?;
                    let result = emit_number_arithmetic_operation(&mut self.builder, arithmetic, left_number, right_number);
                    self.ins().jump(join, &[ir::BlockArg::Value(result)]);
                    self.builder.switch_to_block(join);
                    self.join_states(state, &float_state, pc);
                    self.set_number(state, destination, number);
                }
            }
            (_, Some(bitwise)) => {
                let left_int = self.operand_to_int32(state, left, exit)?;
                let right_int = self.operand_to_int32(state, right, exit)?;
                let right_constant = super::facts::constant_number(&self.program.constants, right);
                if !bitwise.result_is_int32(right_constant) {
                    let number = emit_number_bitwise_operation(&mut self.builder, bitwise, left_int, right_int);
                    self.set_number(state, destination, number);
                } else {
                    let result = emit_number_bitwise_bits(&mut self.builder, bitwise, left_int, right_int);
                    self.set_int(state, destination, result);
                }
            }
            // A relation's Boolean, from the predicate a branch on it would test.
            _ => {
                let holds = self.relation(state, pc as u32, operator, left, right)?;
                let boolean = self.boolean(holds);
                self.set_boolean(state, destination, boolean);
            }
        }
        if returns && self.completion == Completion::Number {
            self.return_number(state, destination, pc as u32);
            return Some(false);
        }
        if returns {
            // The completion names the function's terminal `Return` where it has one, whose
            // register the lane reads it from, else the binary's own record.
            let terminal = self.unit.instructions.len().checked_sub(1)
                .filter(|terminal| self.unit.instructions[*terminal].op().is_return())
                .and_then(|terminal| match self.unit.origins.get(terminal) {
                    Some(super::lane_unit::Origin::Caller(record)) => Some((*record, self.unit.instructions[terminal].register_a())),
                    _ => None,
                });
            let Some(super::lane_unit::Origin::Caller(own)) = self.unit.origins.get(pc).copied() else { unreachable!("a returning binary is a caller record's") };
            let bits = self.tagged(state, destination);
            // The activation's `Return` register holds it too, as the record it names reads.
            if let Some((_, register)) = terminal {
                let base = self.registers_base;
                self.ins().store(regions::frame(), bits, base, register_offset(register));
            }
            self.return_value(terminal.map_or(own, |(record, _)| record), bits);
            self.native += 1;
            return Some(false);
        }
        Some(true)
    }
}

/// Continue only when `condition` is false; `fallback` otherwise.
fn emit_js_guard_not(builder: &mut FunctionBuilder<'_>, condition: ir::Value, fallback: ir::Block) {
    let passed = builder.create_block();
    builder.ins().brif(condition, fallback, &[], passed, &[]);
    builder.switch_to_block(passed);
}

#[cfg(test)]
#[path = "lane_region/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "lane_region/lowering_tests.rs"]
mod lowering_tests;
