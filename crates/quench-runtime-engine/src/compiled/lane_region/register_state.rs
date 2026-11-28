use super::*;

/// A typed array view an access proved (`Proofs::typed`): the register holding the base, its native kind, the view's elements and
/// length, whether it was proved for stores (which also answers reads), and whether an instruction that may change a view
/// ran since (`kept`): a kept view is the same view once the code holds views (`Lowering::typed_view_of`).
#[derive(Clone, Copy, PartialEq)]
pub(super) struct TypedProof { register: Register, kind: quench_runtime::tier_surface::TypedArrayKind, values: ir::Value, length: ir::Value, writable: bool, kept: bool }
/// Semantic proof and active machine view; tagged bits may also be current.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegisterFact {
    pub(super) value: VerifiedFact,
    pub(super) tagged: TaggedView,
}

/// A current tagged view is either an entry/join variable or a dominating SSA value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TaggedView { Unavailable, Variable, Value(ir::Value) }

impl TaggedView {
    pub(super) fn current(self) -> bool { self != Self::Unavailable }
}

impl RegisterFact {
    pub(super) fn admit(value: Fact) -> Option<Self> {
        Some(Self { value: value.verified()?, tagged: if value.representation == Representation::Tagged {
            TaggedView::Variable
        } else { TaggedView::Unavailable } })
    }

    pub(super) fn definition(ty: Type, representation: Representation) -> Self {
        Self::admit(Fact { ty, representation, binding: BindingTime::Dynamic,
            proof: Proof::Definition, projection: Projection::Exact })
            .expect("semantic operation's result type matches its representation")
    }

    /// A definition held in the narrowest representation its type takes (`repr`).
    pub(super) fn narrowest(ty: Type) -> Self { Self::definition(ty, repr(ty)) }

    pub(super) fn canonical() -> Self {
        Self::admit(Fact::tagged(BindingTime::Location, Proof::CanonicalValue))
            .expect("canonical tagged value")
    }
}

pub(super) struct Registers {
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
pub(super) struct State {
    pub(super) values: Vec<RegisterFact>,
    pub(super) proofs: LocalProofs,
    /// What the jump just stepped proved of a register along each of its edges
    /// (`Frame::refine`), applied to the successors' states (`State::narrow`) once it branches.
    pub(super) refinements: smallvec::SmallVec<[(quench_runtime::staged::Edge, Register, Type); INLINE_EDGE_REFINEMENTS]>,
    /// Receivers kept as their field values (`virtual_objects`), each live in its register.
    pub(super) virtuals: smallvec::SmallVec<[virtual_objects::VirtualObject; 1]>,
    /// Array literals kept as their element values (`virtual_objects::VirtualArray`), each live in its register.
    pub(super) virtual_arrays: smallvec::SmallVec<[virtual_objects::VirtualArray; 1]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FieldValue {
    /// The resolved tagged word of the receiver the read named: a fact of the object, so every register holding it shares it.
    pub(super) owner: ir::Value,
    pub(super) atom: Atom,
    pub(super) value: ir::Value,
    pub(super) ty: Type,
}

/// Available object and value proofs saved at control-flow edges. Register types
/// belong to the shared dataflow analysis and need no duplicate snapshots.
#[derive(Clone, Default)]
pub(super) struct LocalProofs {
    /// Direct-object proofs by the resolved tagged word they prove, until an effect that may change the object
    /// retires them. A register's redefinition names a new word, so it retires nothing. The flag is whether the
    /// proof was a read's (`readable`, which may name a function's object).
    pub(super) headers: smallvec::SmallVec<[(ir::Value, ObjectHeader, bool); INLINE_FIELD_BINDINGS]>,
    /// The record each proved value names (`ObjectRecord`), by the tagged word (resolved), and whether
    /// the proof was a read's (`readable`, which may name a function's object): a fact of the value, which
    /// no effect changes (records stay put in a run, and the reservation's base is the run's), so it
    /// outlives the header's shape proof across calls and holds for every register that holds the word
    /// (a copy, an inlined frame's `this`). A later header of any of them reads its shape through it, with
    /// no family test and no record lookup: the untagged view of a proven object (R2, facts on values).
    pub(super) records: smallvec::SmallVec<[(ir::Value, ObjectRecord, bool); INLINE_FIELD_BINDINGS]>,
    /// The complete own-slot projection of an object, by its resolved tagged word and exact shape: a fact of the
    /// object, which any register holding the word shares (`keep_layout`).
    pub(super) layouts: BTreeMap<(ir::Value, u32), CheckedObject>,
    /// The projection of the realm's global object a held slot's access proved (`call_out::global_layout`),
    /// until an effect that may move the property arena retires it, as `layouts`.
    pub(super) global: Option<(CheckedObject, usize)>,
    /// The exact shape of an object, by its resolved tagged word, that the object had when a proof
    /// (or definition) of it made the shape its type, and which a call keeps (`kept_shapes.rs`): the
    /// code holds it again at a use where the VM holds the shape stable.
    pub(super) kept_shapes: BTreeMap<ir::Value, u32>,
    /// Registers holding the receiver, whose facts the entry proved.
    pub(super) receivers: BTreeSet<Register>,
    /// Immutable reads; receiver definitions and memory effects retire these facts.
    pub(super) fields: smallvec::SmallVec<[FieldValue; INLINE_FIELD_BINDINGS]>,
    /// Pure ToInt32 results, separate from each source value's semantic type.
    pub(super) int32_projections: smallvec::SmallVec<[(ir::Value, quench_runtime::staged::Int32<ir::Value>); INLINE_NUMBER_PROJECTIONS]>,
    /// Number views of tagged values a test proved (`frame_operands::register_number_value`): a
    /// second register defined as the same value (`p.x * p.x`) takes the double without a test.
    pub(super) numbers: smallvec::SmallVec<[(ir::Value, quench_runtime::staged::Float64<ir::Value>); INLINE_NUMBER_PROJECTIONS]>,
    /// Dense array views by the tagged base they prove, until an instruction that could change an array's storage (`retains_array_views`).
    pub(super) arrays: smallvec::SmallVec<[(ir::Value, super::super::DenseArrayView); INLINE_ARRAY_VIEWS]>,
    /// Typed array views by the tagged base and the observed kind they prove (`row_or_typed`'s typed-only row), retired
    /// as `arrays` are: a later access of the same base and kind takes the view without proving it again.
    pub(super) typed: smallvec::SmallVec<[TypedProof; INLINE_ARRAY_VIEWS]>,
    /// The dense capture views (values, length) a frameless admission of an inlined callee gave, by the
    /// callee's value: its frame's reads take them, all dominated by the one admission (`Lowering::admitted_captures`).
    pub(super) admitted_captures: smallvec::SmallVec<[(ir::Value, (ir::Value, ir::Value)); INLINE_CAPTURE_ADMISSIONS]>,
    /// Whether this block read the code's invalidation word since the last helper that may change a Stable fact's subject
    /// (run JavaScript, write a prototype, name a method): every Stable fact the code folded holds until such a helper (`Lowering::hold_stable`).
    pub(super) stable: bool,
    /// Whether this block proved the realm its global reads were compiled against active (`Lowering::hold_global`).
    pub(super) realm: bool,
    /// Whether this block proved the run's stores specialized (`JsStepDispatch::stores_specialized`, a
    /// constant of the run), so a store of an unnamed atom tests only the invalidation word (`Lowering::online_store_held`).
    pub(super) specialized: bool,
    /// Registers whose lane-frame slot holds their current tagged value (`Lowering::home_written`): a
    /// definition retires one, so a call writes back only what changed since the last write-back.
    pub(super) homed: BitSet,
    /// Resolved words of holders this unit allocated (object or array literals) with no safepoint since (4.7 (b)):
    /// a store into one needs no barrier log. A safepoint is an allocation, a call, or a step that may allocate or run guest
    /// code (`Lowering::retire_fresh`); a join keeps the holders every arrival holds (`keep_shared_proofs`, `join_states`).
    pub(super) fresh: smallvec::SmallVec<[ir::Value; INLINE_FRESH_HOLDERS]>,
}

/// A block usually holds a few fresh holders at once (a constructor's receiver, a literal's temporaries).
pub(super) const INLINE_FRESH_HOLDERS: usize = 4;

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
    pub(super) fn write_field(&mut self, atom: Atom) {
        self.fields.retain(|field| field.atom != atom);
    }

    /// Keeps the homes `other` also holds, for a state both arrivals reach.
    pub(super) fn meet_homed(&mut self, other: &BitSet) {
        let dropped: Vec<usize> = self.homed.iter().filter(|register| !other.contains(*register)).collect();
        for register in dropped { self.homed.remove(register); }
    }
}

impl State {
    /// Takes back the tagged views `before` held where one path made its own (`TaggedView::Value`, a word
    /// it boxed or read): that word is defined in the path's blocks, which a sibling path or the join past
    /// both does not see. The next read derives the view again where it is made (`frame::register_tagged`).
    pub(super) fn drop_path_views(&mut self, before: &State) {
        for (value, before) in self.values.iter_mut().zip(&before.values) {
            if matches!(value.tagged, TaggedView::Value(_)) && value.tagged != before.tagged {
                value.tagged = before.tagged;
            }
        }
    }

    pub(super) fn canonical(registers: usize) -> Self {
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
    pub(super) fn narrow(&mut self, edge: quench_runtime::staged::Edge, refinements: &[(quench_runtime::staged::Edge, Register, Type)]) {
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
    pub(super) fn call_effect(&mut self) {
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
pub(super) enum ReceiverRecovery { Object, ObjectOrNullish }

impl ReceiverRecovery {
    pub(super) fn for_body(body: &quench_runtime::bytecode::Function) -> Self {
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
pub(super) struct Entry {
    pub(super) receiver: LaneReceiver,
    /// Entry receiver certificates: record locators of `this` (the final entry) and of the
    /// caller registers no unit instruction writes. Records stay put in a run, which
    /// collects nothing; shapes are derived per use.
    pub(super) records: Vec<Option<ObjectRecord>>,
    /// Per folded receiver shape, whether the receiver has it.
    pub(super) receiver_shapes: BTreeMap<u32, Variable>,
    /// Per stored atom, whether the run admits direct stores of it.
    pub(super) store_atoms: BTreeMap<Atom, Variable>,
    /// The running activation's dense capture slots (`EnvironmentSlots::native_dense_view`).
    pub(super) capture_values: Variable,
    pub(super) capture_len: Variable,
    /// Whether those slots admit every direct store (the captures word's `CAPTURE_STORES_BIT`).
    pub(super) capture_stores: Variable,
    /// The word of the storage the activation's environment-owned locals live in (`JsStepDispatch::environment_storage`),
    /// read once at entry and again after an operation that makes an environment (`Lowering::reload_environment`).
    pub(super) environment: Variable,
    pub(super) this: Variable,
    /// Flat per-site immutable slot inputs; no per-operation probe.
    pub(super) fields: smallvec::SmallVec<[(u16, FieldSlotBinding); INLINE_FIELD_BINDINGS]>,
    /// Function-bound numeric calls whose invariant closure was admitted here.
    pub(super) calls: BitSet,
    /// Inlined frames whose invariant receiver needs no boxing on recovery.
    pub(super) call_receivers: BitSet,
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
pub(super) struct NumberReturn {
    pub(super) block: ir::Block,
    pub(super) fact: VerifiedFact,
}

#[derive(Clone, Copy)]
pub(super) struct LoopRead {
    pub(super) value: Variable,
    pub(super) ty: Type,
    pub(super) representation: Representation,
    /// The read's actual entry proof, valid while the qualified loop cannot
    /// replace this receiver or change its shape. Never inferred from feedback.
    pub(super) receiver: Option<(Register, Type)>,
    pub(super) header: Option<ObjectHeader>,
    /// The word an induction value (`representation` `Int64`) advances by at each step of its counter.
    pub(super) step: Option<Variable>,
}

#[derive(Clone, Copy)]
pub(super) struct InvariantRead {
    pub(super) pc: usize,
    /// The canonical caller home supplying a field receiver at loop entry.
    pub(super) owner: Option<Register>,
    /// Whether a root-held binding it reads stays put: its loop stores no global object field.
    pub(super) rooted: bool,
    /// The caller register a function-bound inlined capture read takes its closure from, where the
    /// loop neither writes it nor reads the callee again: the closure it holds at entry is the one
    /// every iteration reads from, so the read is hoisted to the entry edge (`entry_callee`).
    pub(super) callee: Option<Register>,
}

/// A hoisted view: its base, its elements' address and count, and in a typed region the
/// array's `TypedArrayKind` discriminant.
#[derive(Clone, Copy)]
pub(super) struct ArrayVariables {
    pub(super) bits: Variable,
    pub(super) values: Variable,
    /// The element count: a dense backing's `u32` as `i32`, a typed array's as `i64`.
    pub(super) length: Variable,
    pub(super) kind: Variable,
    /// A dense array's room for appends (`dense_appends`) as `i32`: zero until a reserve proves it.
    pub(super) room: Variable,
}

/// A STORE-site projection admitted on entry to a storage-preserving loop.
/// The record is physical receiver identity, not merely an equal shape.
#[derive(Clone, Copy)]
pub(super) struct LoopStore {
    pub(super) header: ObjectHeader,
    pub(super) address: ir::Value,
    pub(super) packed_range: Option<IntRange>,
    /// The domain the entry admitted for the store (`Type::NUMBER` where the slot keeps raw
    /// doubles, which the carried Number stores as is; `Type::INT32` otherwise).
    pub(super) domain: Type,
}

