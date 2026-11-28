use super::*;

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
pub(super) struct GuardExit {
    pub(super) block: ir::Block,
    pub(super) values: Vec<RegisterFact>,
    pub(super) homed: BitSet,
    pub(super) virtuals: smallvec::SmallVec<[virtual_objects::VirtualObject; 1]>,
    pub(super) virtual_arrays: smallvec::SmallVec<[virtual_objects::VirtualArray; 1]>,
    pub(super) pc: u32,
    pub(super) line: u32,
    /// The fact the guard tests (`guard_exit_for`); `Unclassified` where it names none.
    pub(super) fact: ExitFact,
}

/// What a step's frame needs beyond its edges: the plans and held read of a named field access,
/// what a slow case needs of the lowering (`sites`: a call-out's record, a capture access's reach).
#[derive(Default)]
pub(super) struct StepWith<'a> {
    pub(super) access: Option<(&'a LanePlans<'a>, HeldRead)>,
    pub(super) sites: frame::Sites<'a>,
    /// The call or return a step of the call family runs for (`call_frames`).
    pub(super) site: Option<call_frames::Site<'a>>,
}

pub(crate) struct Lowering<'a, 'f> {
    pub(super) builder: FunctionBuilder<'f>,
    pub(super) program: &'a ResidualProgram,
    pub(super) unit: &'a super::super::lane_unit::Unit,
    /// The alias region each (class, atom) named property is keyed by (`declare_fields`); empty
    /// unless every property access of the unit is keyed.
    pub(super) field_regions: BTreeMap<regions::FieldKey, cranelift_codegen::ir::AliasRegion>,
    /// The function's parameter count: an entry guard on a parameter declines the region
    /// when a call's value does not match the compiled entry fact.
    pub(super) params: usize,
    /// The lowered function, which an exception it raises names as its source (`Lowering::throw`).
    pub(super) function_id: u32,
    /// The entry function's annotated locals: the register each lives in and the language type its
    /// annotation seeds (`declared::declared_local_types`).
    pub(super) declared_definitions: Vec<(Register, Type)>,
    /// The entry function's declared `this` type, which its `LoadThis` checks where it enters the destination.
    pub(super) declared_this: Option<Type>,
    /// The entry function's unwritten rest parameter register and its declared element type (`declared::declared_rest_read`).
    pub(super) declared_rest: Option<(Register, Type)>,
    /// The entry function's unwritten non-rest parameter registers, each with its parameter index
    /// (`declared::declared_member_reads`), whose named reads the declared member types check.
    pub(super) declared_members: Vec<(Register, u16)>,
    /// The entry function's signature key (its source span start), which `declared_members` are typed by.
    pub(super) declared_member_start: Option<u32>,
    pub(super) helpers: LaneHelpers,
    /// The proved shapes and slots of each field cache site the unit reads or writes.
    pub(super) fields: &'a FieldSites,
    /// The typed array kinds the body's element records took (`body_views::typed_reads`): the kind's view each access proves.
    pub(super) kinds: &'a [super::super::body_views::ObservedKind],
    /// The by-name loads that read the global scope (`Vm::global_reads_in_lane`).
    pub(super) global_reads: &'a quench_runtime::vm::global_facts::GlobalReads,
    /// The VM's facts: its `length` name, whose read of a direct array is its record's
    /// (`array_length`), and whether an IsHTMLDDA object can exist (`emit_nullish_branch`).
    pub(super) vm: VmFacts<'a>,
    /// The call plans of the lowered function and of its inlined callees (`bindings::CallPlans`): each call
    /// site's arms and miss decide its admission.
    pub(super) plans: bindings::CallPlans<'a>,
    /// The Stable facts the code folded, each a code dependency of its publication.
    pub(super) stable: stable::StableUses,
    /// The unit PCs a previous body left at (`Vm::compile_lane_method`): a construction there
    /// runs by the call protocol (`Lowering::construct`).
    pub(super) generic: &'a [usize],
    /// Whether the entry is proving a lane body's inputs: its guards share `entry_exit`.
    pub(super) proving_entry: bool,
    /// Whether a previous body left at the instruction being lowered (`generic`): its reads probe the VM's caches.
    pub(super) left_here: bool,
    /// The view of the loop's hoisted dense array the next element row reads or writes through, which
    /// its entering edge proved once (`EmitFrame::held`).
    pub(super) held_view: Option<frame_elements::Held>,
    /// Whether the held view's read leaves at a hole (`EmitFrame::hole_leaves`).
    pub(super) hole_leaves: bool,
    /// The shared leave of a failed input proof (`LaneExit::Entry`), made at its first guard
    /// and filled with the other exits.
    pub(super) entry_exit: Option<ir::Block>,
    pub(super) direct: &'a DirectSites,
    /// Per inlined frame, its callee's dense capture slots (values and count), fixed by the
    /// callee's identity, which its call guards; null where they are not dense.
    pub(super) frame_captures: &'a [(usize, usize)],
    pub(super) context: ir::Value,
    /// An entry-bound poll page, or the ordinary atomic termination flag.
    pub(super) interrupt: ir::Value,
    pub(super) fault_poll: bool,
    /// A loop region's completed iterations, flushed to its entry record where it leaves (`trips.rs`).
    pub(super) trips: Option<Variable>,
    /// Callback completion edges can retire the native entry's interrupt ownership proof.
    pub(super) reentries: smallvec::SmallVec<[ir::Block; INLINE_EFFECT_EDGES]>,
    pub(super) polls: smallvec::SmallVec<[ir::Inst; INLINE_EFFECT_EDGES]>,
    /// Already-conforming backedges meet before their common fault poll.
    pub(super) poll_latches: super::super::SecondaryMap<ir::Block, Option<ir::Block>>,
    pub(super) shared_headers: BitSet,
    pub(super) payload: ir::Value,
    pub(super) registers_base: ir::Value,
    /// The running body's receiver and capture words its caller passes (`entry_cell::capture_word`).
    pub(super) entry_words: (ir::Value, ir::Value),
    pub(super) callee_slots: Vec<(u16, ir::Value)>,
    pub(super) registers: Registers,
    pub(super) written: BitSet,
    /// Source operations already consumed by an exact, unobservable scalar pair.
    pub(super) consumed: BitSet,
    pub(super) blocks: Vec<Option<ir::Block>>,
    /// Per resume PC, its leave blocks, one per set of registers stored (`exits.rs`).
    pub(super) exits: Vec<Vec<exits::ExitBlock>>,
    /// The leave blocks shared by every caller-frame PC that stores the same registers, which
    /// take the exit word as their parameter (`Lowering::shared_exit_block`).
    pub(super) shared_exits: Vec<exits::ExitBlock>,
    /// Each block leader's register use demand (`demand::analyze`).
    pub(super) demands: demand::Demands,
    pub(super) entry: Option<Entry>,
    pub(super) native: usize,
    /// Whether this entry's caller loop reaches its native latch.
    pub(super) backedge: bool,
    /// Guard failure blocks to fill once the region is lowered (`guard_exit`).
    // Recovery boxes register values; object/layout proofs authorize forward execution
    // only and must not be retained in every guard's snapshot. With the lowering line that
    // made each guard.
    pub(super) pending: Vec<GuardExit>,
    /// Whether the unit makes native calls (`Lowering::call`): a callee may change `this`,
    /// so no read or store folds the receiver facts its entry proved.
    pub(super) native_calls: bool,
    /// Entry-bound guest depth for a region whose helpers cannot reenter.
    pub(super) call_budget: Option<ir::Value>,
    /// The registers' types at each block leader (`facts::analyze`).
    pub(super) facts: super::super::facts::Facts,
    /// The unit's register liveness by unit PC (`Unit::register_masks`), where the unit
    /// fits one register set; otherwise the caller's by its PCs and per inlined frame its
    /// callee's, projected per query (`Lowering::is_live`). A function without masks keeps
    /// every register live.
    pub(super) live: Liveness<'a>,
    pub(super) valid: bool,
    pub(super) coverage: Coverage,
    pub(super) completion: Completion,
    /// One return stays inline; additional returns spill without a region limit.
    pub(super) number_returns: smallvec::SmallVec<[NumberReturn; 1]>,
    /// Per loop header, the reads its loop repeats unchanged (`invariant_reads`), made once
    /// on the loop's one entry edge; each read's PC names the value it made there and
    /// whether its int tag was tested.
    pub(super) hoists: BTreeMap<usize, Vec<InvariantRead>>,
    /// The unit's block leaders (`leaders`), made once for the first loop whose reads hoist.
    pub(super) unit_leaders: std::cell::OnceCell<std::rc::Rc<BitSet>>,
    /// The calls that run inline as Math operations (`native_calls::intrinsic_calls`).
    pub(super) intrinsics: native_calls::Intrinsics,
    pub(super) hoisted: BTreeMap<usize, LoopRead>,
    /// The scalar native calls a loop's entry computed once, by PC (`native_calls::hoist_scalar_calls`).
    pub(super) scalar_values: BTreeMap<usize, native_calls::HoistedScalar>,
    /// Per dense array a loop searches, whether its prototypes hold no index, proved on the loop's entry
    /// (`hoist_arrays`) and unset by an invalidation (`forget_append_rooms`).
    pub(super) prototype_clear: BTreeMap<Register, Variable>,
    /// The hoisted dense arrays, by loop header and source, whose entering edge guarded every element
    /// an int32 (`int_only_array`, `hoist_arrays`): their reads take the element's word as it is.
    pub(super) int_only: BTreeSet<(usize, Register)>,
    /// The loops whose entering edge tests the construct plan of each callee their kept
    /// constructions build by, once for the loop (`virtual_objects::hoist_constructions`).
    pub(super) construct_ready: Vec<virtual_objects::ConstructReady>,
    /// Loop-invariant array bases whose views their loop's entry proves (`loop_arrays`).
    pub(super) loop_arrays: Vec<LoopArray>,
    /// Loop-invariant strings whose flat facts their loop's entry proves (`loop_strings`).
    pub(super) loop_strings: Vec<LoopString>,
    /// The variables each held string's facts live in, by register (`loop_strings`).
    pub(super) string_vars: BTreeMap<Register, StringVariables>,
    /// The unit a string element read left for the comparison at the next PC, which compares it
    /// as a unit and never makes the one-character string (`Lowering::unit_compare`).
    pub(super) pending_unit: Option<PendingUnit>,
    /// The shapes loops proved of their invariant receivers on their entering edges (`loop_receivers`).
    pub(super) loop_shapes: Vec<loop_receivers::LoopShape>,
    /// Whether no operation of the unit may move an array's backing or the array record table
    /// (`arrays_stay_put`), so the record table's base is read-only throughout.
    pub(super) arrays_fixed: bool,
    /// Each hoisted base's tagged value and view, by register, across its loop's iterations.
    pub(super) array_vars: BTreeMap<Register, ArrayVariables>,
    pub(super) views: ElementViews,
    /// Successful write-through stores defining a loop-header field's SSA variable.
    pub(super) loop_fields: Vec<LoopField>,
    pub(super) loop_stores: BTreeMap<usize, LoopStore>,
    /// The unit's integer proofs, computed once before lowering (`Lowering::prove_ranges`): the
    /// arithmetic that cannot overflow and the hoisted element accesses within their array.
    pub(super) ranges: int_ranges::IntRanges,
    /// The PC being lowered, whose edges to a header enter its loop or close it.
    pub(super) at: usize,
    /// Per block leader, its forward edges (`forward_edges`), and the states the lowered
    /// ones arrived with: a block they all reach keeps the local proofs they share.
    pub(super) forward: Vec<Option<usize>>,
    pub(super) arrived: BTreeMap<usize, Vec<LocalProofs>>,
    /// Per forward-only join, the kept receivers its arrivals carried (`virtual_objects::JoinedVirtuals`).
    pub(super) joined_virtuals: BTreeMap<usize, virtual_objects::JoinedVirtuals>,
    /// Per block leader a later PC also enters (a loop's header, a rotated body), its forward edge count and its last
    /// entering PC, and the typed views its forward edges arrived with (`carry_typed_views`).
    pub(super) loop_carry: BTreeMap<usize, (usize, usize)>,
    pub(super) carried_typed: BTreeMap<usize, Vec<smallvec::SmallVec<[TypedProof; INLINE_ARRAY_VIEWS]>>>,
    /// The block leaders an entry or a lowered edge enters: a block none does is not
    /// lowered, its code reached only past a leave.
    pub(super) entered: BitSet,
    /// The block leaders sealed when lowered, which only edges lowered before enter
    /// (`edges::leader_edges`): a variable read in one resolves without a parameter.
    pub(super) sealable: BitSet,
    /// Whether a run can do anything but leave: complete natively or run a loop natively. A
    /// body that can do neither declines (`lower`).
    pub(super) finishes: bool,
    /// The one epilogue every stop shares: it returns its status parameter.
    pub(super) leave: ir::Block,
    /// The stop at a record: it writes its result word parameter and leaves stopped.
    pub(super) stop: ir::Block,
    /// The block the call-outs' statuses of a leave in a callee, or a termination, reach with the
    /// status, made at the first and filled at the region's end (`call_outs`).
    pub(super) call_out_stop: Option<ir::Block>,
    /// The throws native code catches itself, by unit PC (`catches::catch_edges`).
    pub(super) catches: catches::Catches,
    /// The same throws as the analysis's exception edges (`facts::Raise`), ascending by PC.
    pub(super) raises: Vec<super::super::facts::Raise>,
}

pub(super) enum Liveness<'a> {
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
    pub(super) fn full(self) -> bool { self == Self::Full }
    pub(super) fn selected_entry(self) -> bool {
        matches!(self, Self::PrefixFrom(_) | Self::Loop { .. })
    }
    /// The PC after the region this coverage selects: where a path that falls off its end
    /// leaves. A loop leaves after its backward jump.
    pub(super) fn exit_pc(self, unit_len: usize) -> usize {
        match self {
            Self::Loop { backedge, .. } => backedge.saturating_add(1).min(unit_len),
            _ => unit_len,
        }
    }

    /// Whether instruction `pc` is part of the region this coverage selects.
    pub(super) fn lowered(self, pc: usize) -> bool {
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

