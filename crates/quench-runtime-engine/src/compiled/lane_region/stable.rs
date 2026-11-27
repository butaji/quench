//! Stable facts in a lane region: what its code folds under code dependencies, and the
//! invalidation points where it learns that one changed.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::machine::{Lane, MachineScalar, Relation};
use quench_runtime::staged::property::StoreJoin;
use quench_runtime::staged::{Region, ShapeFact, Stage};

pub(crate) use quench_runtime::vm::engine_types::StableFacts;

/// The Stable facts a lowering folded so far.
#[derive(Default)]
pub(crate) struct StableUses {
    /// Holder shapes whose prototypes' values the code took as constants.
    pub(crate) holders: quench_runtime::bitset::BitSet,
    /// The chains the code read those values through (`field_plans::embed_inherited`).
    pub(crate) chains: BTreeSet<quench_runtime::code_dependencies::PrototypeChain>,
    /// Exact shapes of the objects whose shape the code held past a call (`StableHold::hold_shape`).
    pub(crate) shapes: quench_runtime::staged::HeldShapes,
    /// Whether the code held the typed views it proved past a call (`StableHold::hold_views`).
    pub(crate) views: quench_runtime::staged::HeldViews,
    /// Atoms it stores directly because no method cache named them.
    atoms: BTreeSet<Atom>,
    /// Global cells whose values the code took as constants.
    globals: BTreeSet<quench_runtime::tier_surface::GlobalFact>,
    /// The heap identities the code embeds: callees, prototypes' values, constants.
    pub(crate) roots: quench_runtime::folded_roots::FoldedRoots,
    /// Global objects whose layout the code held.
    layouts: quench_runtime::staged::HeldLayouts,
    /// Global slots whose observed type the code relied on, each once.
    slots: Vec<quench_runtime::code_dependencies::SlotDependency>,
    /// Constructors whose `prototype` the code took as a constant for an `instanceof`.
    instanceofs: BTreeSet<quench_runtime::code_dependencies::InstanceofFact>,
    /// The program whose root constant the code folded, if any (`StableFacts::root_program`).
    pub(crate) root_program: Option<quench_runtime::vm::program_store::ProgramId>,
    /// Loops, by header and backedge, entered only through forward edges that proved the realm
    /// (`Lowering::hoist_stable`): their blocks hold the proof.
    realm_loops: Vec<(usize, usize)>,
    /// Loops, likewise, entered only through forward edges that read the invalidation word, whose
    /// bodies change no Stable fact's subject (`keeps_stable_facts`): their blocks hold every
    /// Stable fact, which a change made elsewhere retires before the loop is entered again.
    pub(crate) stable_loops: Vec<(usize, usize)>,
    /// Loops, likewise, whose entering edges proved the run's stores specialized (`hold_specialized`).
    specialized_loops: Vec<(usize, usize)>,
    /// The PC a run of the body starts at.
    pub(crate) start: usize,
}

impl StableUses {
    pub(crate) fn take(&mut self) -> StableFacts {
        StableFacts { holders: std::mem::take(&mut self.holders).iter().map(|holder| holder as u32).collect(),
            shapes: self.shapes.take(),
            chains: std::mem::take(&mut self.chains).into_iter().collect(),
            atoms: std::mem::take(&mut self.atoms).into_iter().collect(),
            globals: std::mem::take(&mut self.globals).into_iter().collect(), roots: std::mem::take(&mut self.roots),
            layouts: self.layouts.take(),
            slots: std::mem::take(&mut self.slots).into_iter().collect(),
            instanceofs: std::mem::take(&mut self.instanceofs).into_iter().collect(), root_program: self.root_program.take(),
            views: self.views.take() }
    }
}

impl StableHold<'_> {
    /// The invalidation point of a held fact at the instruction at `at`: the word read unless
    /// the block or loop holds every Stable fact already (`Lowering::hold_stable`).
    fn read_held(&self, (builder, helpers): (&mut FunctionBuilder<'_>, LaneHelpers), online: OnlineFacts<'_>,
        state: &mut State, exit: ir::Block) {
        let held = state.proofs.stable || self.loops.iter().any(|(header, end)| (*header..=*end).contains(&self.at));
        if !held { read_invalidation_word(builder, helpers, online.invalidation, state, exit); }
    }

    /// Holds the values of prototypes of exact shape `holder` from the instruction at `at`: an
    /// invalidation point unless the block or loop holds every Stable fact already
    /// (`Lowering::hold_stable`). Nothing where the code takes no dependencies on the VM.
    pub(crate) fn hold_holder(&mut self, frame: (&mut FunctionBuilder<'_>, LaneHelpers),
        state: &mut State, exit: ir::Block, holder: u32) -> Option<()> {
        let online = self.online?;
        self.read_held(frame, online, state, exit);
        self.holders.insert_growing(holder as usize);
        Some(())
    }

    /// Holds that the objects now of exact shape `shape` still have it, from the instruction at
    /// `at`: an invalidation point as `hold_holder`'s, the code retiring at the first object to
    /// leave the shape. `None` where the code takes no dependencies on the VM or the VM does not
    /// hold the shape stable (`Vm::shape_is_stable`). The fact is the dependency's: the guard
    /// on the object's shape became the hook its departure fires.
    pub(crate) fn hold_shape(&mut self, frame: (&mut FunctionBuilder<'_>, LaneHelpers),
        state: &mut State, exit: ir::Block, shape: u32) -> Option<ShapeFact> {
        let online = self.online.filter(|online| (online.shape_stable)(shape))?;
        self.read_held(frame, online, state, exit);
        Some(self.shapes.hold(shape))
    }

    /// Holds the typed views the code proved, from the instruction at `at`: an invalidation point
    /// as `hold_holder`'s, the code retiring at the first change of a live view (bytes moved,
    /// resized, detached, shared). A view proved before a call is then the same view after it
    /// (`Viewed::by_fact`), with no test. `None` where the code takes no dependencies on the VM or
    /// changes of live views keep retiring code (`heap::typed_views_stable`).
    pub(crate) fn hold_views(&mut self, (builder, helpers): (&mut FunctionBuilder<'_>, LaneHelpers),
        state: &mut State, exit: ir::Block) -> Option<quench_runtime::staged::TypedViewFact> {
        let online = self.online.filter(|_| quench_runtime::tier_surface::typed_views_stable())?;
        // Not `read_held`: a loop that holds every Stable fact may still run an instruction that changes a view
        // (`keep_typed_views`), which only this block's own read since answers.
        if !state.proofs.stable { read_invalidation_word(builder, helpers, online.invalidation, state, exit); }
        Some(self.views.hold())
    }
}

/// The live address a linking lowering's invalidation point has: none, the word is its symbol.
const LINKED_WORD: usize = 0;

/// The code's invalidation word, as it is now (`code_dependencies::Invalidation`): its address
/// (`live`) for a live lowering, a symbol the loader resolves for a linking one.
fn invalidation_word(builder: &mut FunctionBuilder<'_>, helpers: LaneHelpers, live: usize) -> ir::Value {
    let address = if helpers.links() {
        crate::symbol_words::symbol_address(builder, RelocKind::Invalidation as u32, 0)
    } else {
        emit(builder).raw_const(Lane::I64, live as i64)
    };
    emit(builder).load_u64(Region::Invalidation.mutable(), address, 0)
}

/// The invalidation point: the word read once, leaving through `exit` where a change retired
/// the code; the block then holds every Stable fact until a helper that may change one.
fn read_invalidation_word(builder: &mut FunctionBuilder<'_>, helpers: LaneHelpers, live: usize,
    state: &mut State, exit: ir::Block) {
    let invalidated = invalidation_word(builder, helpers, live);
    let held = emit(builder).raw_compare_imm(Relation::Equal, invalidated, 0);
    emit_js_guard(builder, held, exit);
    state.proofs.stable = true;
}

impl Lowering<'_, '_> {
    /// The encoded `prototype` link of the constructor in `constructor`, where the facts type it a
    /// known function and the VM proved its `instanceof` Stable: held from here to the block's next
    /// helper that may change one (`hold_stable`), under a dependency on the constructor's
    /// `prototype` slot, its shape and its `@@hasInstance` holder.
    pub(crate) fn hold_instanceof(&mut self, state: &mut State, exit: ir::Block, constructor: Operand) -> Option<u32> {
        let identity = state.values[usize::from(constructor.register_index()?)].value.ty.function_identity()?;
        let fact = (self.vm.online?.instanceofs)(identity)?;
        self.hold_stable(state, exit);
        self.stable.instanceofs.insert(fact);
        Some(fact.link)
    }

    /// The division this lowering specializes under: online where it may take code
    /// dependencies on the VM (`VmFacts::online`), offline for code shared across VMs.
    pub(crate) fn division(&self) -> Division {
        match self.vm.online {
            None => Division::Offline,
            Some(_) if self.vm.frozen => Division::Frozen,
            Some(_) => Division::Online,
        }
    }

    /// Whether `held` is `identity`, a heap value the code names: a callee, a native, a folded value.
    pub(crate) fn is_identity(&mut self, held: ir::Value, identity: Value) -> ir::Value {
        let word = self.stable.roots.embed_heap_identity(identity);
        self.raw_compare_imm(Relation::Equal, held, word as i64)
    }

    /// `value`, a heap value or an immediate, as a constant the code names.
    pub(crate) fn identity_constant(&mut self, value: Value) -> ir::Value {
        let word = self.stable.roots.embed_heap_identity(value);
        self.raw_const(Lane::I64, word as i64)
    }

    /// Holds every Stable fact the code folded from here to the block's next helper that may
    /// change one: an invalidation point, which reads the word once and leaves through `exit`
    /// where a change retired the code (Execution laws 2: the change triggers, a use checks
    /// nothing).
    pub(crate) fn hold_stable(&mut self, state: &mut State, exit: ir::Block) {
        if state.proofs.stable || self.stable.stable_loops.iter().any(|(header, end)| (*header..=*end).contains(&self.at)) {
            return;
        }
        self.read_invalidation(state, exit);
    }

    /// The invalidation point of `hold_stable`, made here: the online division's word, or an AOT
    /// image's, whose closed-world facts (`VmFacts::closed`) are its dependencies.
    pub(crate) fn read_invalidation(&mut self, state: &mut State, exit: ir::Block) {
        let closed = self.vm.closed.filter(|_| self.helpers.links()).map(|_| LINKED_WORD);
        let live = self.vm.online.map(|online| online.invalidation).or(closed).expect("only a division with Stable facts reads the word");
        read_invalidation_word(&mut self.builder, self.helpers, live, state, exit);
    }

    /// The view of the typed array in `register` of kind `kind` (for stores where `writable`) an earlier access proved
    /// (`Proofs::typed`). A view kept past an instruction that may change one (`keep_typed_views`) is the same view
    /// where the code holds views (`StableHold::hold_views`): one invalidation read answers for every kept view, else
    /// the kept views go and the access proves its view again.
    pub(crate) fn typed_view_of(&mut self, state: &mut State, (register, kind, writable): (Register, quench_runtime::tier_surface::TypedArrayKind, bool),
        exit: ir::Block) -> Option<TypedProof> {
        let proof = state.proofs.typed.iter().find(|view| view.register == register && view.kind == kind && (view.writable || !writable)).copied()?;
        if !proof.kept { return Some(proof); }
        let mut hold = StableHold { holders: &mut self.stable.holders, shapes: &mut self.stable.shapes, views: &mut self.stable.views, loops: &self.stable.stable_loops,
            online: self.vm.online, at: self.at };
        let held = hold.hold_views((&mut self.builder, self.helpers), state, exit).is_some();
        state.proofs.typed.retain_mut(|view| { view.kept &= !held; !view.kept });
        held.then_some(TypedProof { kept: false, ..proof })
    }

    /// Holds the values of prototypes of exact shape `holder` (`hold_stable`).
    pub(crate) fn hold_holder(&mut self, state: &mut State, exit: ir::Block, holder: u32) {
        let mut hold = StableHold { holders: &mut self.stable.holders, shapes: &mut self.stable.shapes, views: &mut self.stable.views, loops: &self.stable.stable_loops,
            online: self.vm.online, at: self.at };
        hold.hold_holder((&mut self.builder, self.helpers), state, exit, holder).expect("only a division with Stable facts holds a prototype");
    }

    /// Holds the value of global cell `fact` (`hold_stable`), read from the global object
    /// `global` of the realm compiled against: a realm reads its own global's cells, so a block
    /// first proves that realm active (`prove_realm`).
    pub(crate) fn hold_global(&mut self, state: &mut State, exit: ir::Block, fact: quench_runtime::tier_surface::GlobalFact,
        realm: (Value, i32)) {
        self.hold_stable(state, exit);
        self.stable.globals.insert(fact);
        self.prove_realm(state, exit, realm);
    }

    /// Holds the layout of the global object `realm` names (`hold_stable`) and, where the slot
    /// `slot` was typed when the code was compiled, that type: the code stores to the slot, or
    /// reads its `typeof`, with no check but the invalidation word and the realm proof.
    /// The layout's fact is the dependency's, which a slot read of the global object takes.
    pub(crate) fn hold_global_slot(&mut self, state: &mut State, exit: ir::Block, slot: quench_runtime::vm::global_facts::GlobalSlot) -> ShapeFact {
        self.hold_stable(state, exit);
        let fact = self.stable.layouts.hold(slot.layout);
        if let Some(dependency) = slot.dependency && !self.stable.slots.contains(&dependency) {
            self.stable.slots.push(dependency);
        }
        self.prove_realm(state, exit, (slot.global, slot.realm_offset));
        fact
    }

    /// Holds the layout of the global object `slot` names (`hold_stable`), under which a new declaration is the only change that moves the
    /// declarative record's words, and the realm active that the record belongs to: a global lexical binding is read live, at its address.
    pub(crate) fn hold_lexical(&mut self, state: &mut State, exit: ir::Block, slot: quench_runtime::vm::global_facts::LexicalSlot) {
        self.hold_stable(state, exit);
        self.stable.layouts.hold(slot.layout);
        self.prove_realm(state, exit, (slot.global, slot.realm_offset));
    }

    /// Proves the realm of global object `global` active, from the VM's field at `realm_offset`.
    /// A call returns to its caller's realm, so the proof outlives every call.
    pub(crate) fn prove_realm(&mut self, state: &mut State, exit: ir::Block, realm: (Value, i32)) {
        if state.proofs.realm || self.stable.realm_loops.iter().any(|(header, end)| (*header..=*end).contains(&self.at)) {
            return;
        }
        self.test_realm(state, exit, realm);
    }

    /// The realm test of `prove_realm`, made here.
    pub(crate) fn test_realm(&mut self, state: &mut State, exit: ir::Block, (global, realm_offset): (Value, i32)) {
        let vm = emit_js_payload_fixed_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.vm));
        let active = emit(&mut self.builder).load_u64(Region::Dispatch.mutable(), vm, realm_offset);
        let same = self.raw_compare_imm(Relation::Equal, active, global.to_bits() as i64);
        emit_js_guard(&mut self.builder, same, exit);
        state.proofs.realm = true;
    }

    /// Where no method cache names `atom` now, a store admitted while `specialized` holds:
    /// that is a Stable fact, which a cache naming it retires (`MethodCacheAtoms::name`), so the
    /// store reads the invalidation word, not the cache's names.
    pub(crate) fn store_held_unnamed(&mut self, atom: Atom, specialized: ir::Value) -> Option<ir::Value> {
        let online = self.vm.online.filter(|online| !online.names(atom))?;
        self.stable.atoms.insert(atom);
        let invalidated = invalidation_word(&mut self.builder, self.helpers, online.invalidation);
        let held = self.raw_compare_imm(Relation::Equal, invalidated, 0);
        Some(emit(&mut self.builder).all(specialized, held))
    }

    /// `store_held_unnamed` in a block: the run's stores proved specialized once per block (a
    /// constant of the run) and the invalidation word read once since the last call or once at
    /// the entry of a loop that keeps Stable facts (`hold_stable`), each leaving through `exit`
    /// where it fails, rather than each store testing both and joining the VM's store.
    pub(crate) fn online_store_held(&mut self, state: &mut State, atom: Atom, exit: ir::Block) -> Option<ir::Value> {
        self.vm.online.filter(|online| !online.names(atom))?;
        self.hold_specialized(state, exit);
        self.hold_stable(state, exit);
        self.stable.atoms.insert(atom);
        Some(self.raw_const(Lane::I8, 1))
    }

    /// The run's stores proved specialized (`stores_specialized`) where no block proof of it holds
    /// yet: a guard leaving through `exit` where the run does not. The loop's entering edge makes
    /// this proof once for a loop whose stores all hold it (`hoist_stable`).
    pub(crate) fn hold_specialized(&mut self, state: &mut State, exit: ir::Block) {
        if state.proofs.specialized || self.stable.specialized_loops.iter().any(|(header, end)| (*header..=*end).contains(&self.at)) { return; }
        let specialized = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.stores_specialized));
        let specialized = self.raw_compare_imm(Relation::NotEqual, specialized, 0);
        emit_js_guard(&mut self.builder, specialized, exit);
        state.proofs.specialized = true;
    }

    /// Whether the field store `instruction` at `pc` keeps every Stable fact in a loop: its join is
    /// the host's (no entry binding, no adding entry) and its region is a loop not widened at `pc`,
    /// so a store the in-place arms miss leaves (`store_leaves`, `field_store_host`) and runs no
    /// guest code; the store changes no fact the loop holds beyond the invalidation word it reads.
    pub(crate) fn store_keeps_facts(&self, pc: usize, instruction: WideInstruction) -> bool {
        let (_, join) = self.site_store_plan(instruction.cache_site_index(), Type::ANY);
        join == StoreJoin::Host && matches!(self.coverage, Coverage::Loop { .. }) && !self.generic.contains(&pc)
    }

    /// Where offline code holds `atom` unnamed (`VmFacts::held_atoms`), the admission of its stores
    /// in a run that makes no call: true. The code was compiled for a specialized run, a cache
    /// naming the atom retires it before its next entry, and a run that calls nothing meets no
    /// cache that could name it (`entry_store_atoms`); a run that calls tests the caches.
    pub(crate) fn loop_held_atom(&mut self, atom: Atom) -> Option<ir::Value> {
        if self.vm.online.is_some() || self.native_calls || self.helpers.links() { return None; }
        self.vm.held_atoms.filter(|held| !held.names.contains(atom as usize))?;
        self.stable.atoms.insert(atom);
        Some(self.raw_const(Lane::I8, 1))
    }

    /// `loop_held_atom` in a run that calls: a call may have named the atom, which sets the
    /// code's invalidation word, so the block reads the word once after its last call
    /// (`State::proofs.stable`) and leaves through `exit` where it is set.
    pub(crate) fn called_held_atom(&mut self, state: &mut State, atom: Atom, exit: ir::Block) -> Option<ir::Value> {
        if self.vm.online.is_some() || self.helpers.links() { return None; }
        let held = self.vm.held_atoms.filter(|held| !held.names.contains(atom as usize))?;
        self.stable.atoms.insert(atom);
        self.read_region_invalidation(state, held, exit);
        Some(self.raw_const(Lane::I8, 1))
    }

    /// Reads offline code's invalidation word (`HeldAtoms::invalidation`) once per block after
    /// its last call (`State::proofs.stable`), leaving through `exit` where it is set.
    fn read_region_invalidation(&mut self, state: &mut State, held: super::super::HeldAtoms<'_>, exit: ir::Block) {
        if state.proofs.stable { return; }
        let address = emit(&mut self.builder).raw_const(Lane::I64, held.invalidation as i64);
        let invalidated = emit(&mut self.builder).load_u64(Region::Invalidation.mutable(), address, 0);
        let unchanged = self.raw_compare_imm(Relation::Equal, invalidated, 0);
        emit_js_guard(&mut self.builder, unchanged, exit);
        state.proofs.stable = true;
    }

    /// Whether offline code holds Stable facts under its own code dependencies 
    /// (`VmFacts::held_atoms`: an OSR region, whose publication subscribes them and sets its
    /// invalidation word when one changes), as online code does under its plan's.
    pub(crate) fn region_holds_stable(&self) -> bool {
        self.vm.online.is_none() && !self.helpers.links() && self.vm.held_atoms.is_some()
    }

    /// `hold_holder` for offline code that holds Stable facts (`region_holds_stable`): the
    /// prototype's values are a dependency of its publication, read through its invalidation word.
    pub(crate) fn region_hold_holder(&mut self, state: &mut State, exit: ir::Block, holder: u32) {
        let held = self.vm.held_atoms.expect("a region that holds Stable facts");
        self.stable.holders.insert_growing(holder as usize);
        self.read_region_invalidation(state, held, exit);
    }

    /// Holds prototype `holder`'s values by whichever dependency the code takes (`hold_holder`,
    /// `region_hold_holder`); false where it takes none.
    pub(crate) fn holds_holders(&self) -> bool {
        self.division().folds(BindingTime::Stable) || self.region_holds_stable()
    }

    /// `hold_holder` or `region_hold_holder`, as the code's division allows (`holds_holders`).
    pub(crate) fn hold_holder_any(&mut self, state: &mut State, exit: ir::Block, holder: u32) {
        if self.division().folds(BindingTime::Stable) { self.hold_holder(state, exit, holder); }
        else { self.region_hold_holder(state, exit, holder); }
    }

    /// Whether a run enters loop `header..=end` only at its header, by edges that each make the
    /// proofs hoisted there: no start lies past its header (a start at it hoists on its entry
    /// edge), every edge from outside it is a forward edge to its header (`continue_at`, which
    /// hoists), and every branch table's entry is one.
    pub(crate) fn enters_only_at_header(&self, header: usize, end: usize) -> bool {
        let inside = header..=end;
        // A branch table's entries are among its successors, so an edge it makes is seen here too.
        let entered_within = self.unit.instructions.iter().enumerate().any(|(pc, instruction)|
            !inside.contains(&pc) && successors(pc, *instruction).into_iter()
                .any(|target| (target > header && target <= end) || (target == header && pc > end)));
        !entered_within && !(header < self.stable.start && self.stable.start <= end)
    }

    /// Whether `instruction` at `pc` changes no Stable fact's subject and runs no guest code: it
    /// reads natively or leaves (`reads_only`, here also where it returns from an inlined frame),
    /// or reads the global scope (`global_read`); calls, stores and constructions do not qualify.
    pub(crate) fn keeps_stable_facts(&self, pc: usize, instruction: WideInstruction) -> bool {
        match instruction.op().class() {
            _ if matches!(quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()), Some(quench_runtime::staged::Operation::LoadName { .. })) => self.global_reads.contains_key(&pc),
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) => !instruction.writes_current_this(),
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) => true,
            _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) => self.store_keeps_facts(pc, instruction),
            // A held slot's store leaves where its admission or the slot's type fails (`global_cell`, `global_write`).
            _ if self.stores_global_slot(pc, instruction) => self.division().folds(BindingTime::Stable),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Call) => super::native_calls::keeps_arrays(&self.intrinsics, pc),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) => true,
            _ => reads_only(&self.program.constants, instruction),
        }
    }

    /// Notes loop `header..=end` as holding the proofs hoisted to its entry edges.
    pub(crate) fn hold_in_loop(&mut self, header: usize, end: usize, stable: bool, realm: bool, specialized: bool) {
        if stable { self.stable.stable_loops.push((header, end)); }
        if specialized { self.stable.specialized_loops.push((header, end)); }
        if realm { self.stable.realm_loops.push((header, end)); }
    }
}
