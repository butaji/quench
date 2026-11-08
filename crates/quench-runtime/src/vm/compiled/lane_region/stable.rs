//! Stable facts in a lane region: what its code folds under code dependencies, and the
//! invalidation points where it learns that one changed.

use super::*;

/// The Stable facts a region's code folded (`typed_ir::BindingTime::Stable`), each the subject
/// of a code dependency its publication takes (`ProgramStore::publish_lane_method`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StableFacts {
    /// Exact shapes of prototypes whose values it holds (`heap::subscribe_prototype_values`).
    pub(crate) holders: Box<[u32]>,
    /// Atoms it stores without a method-cache test (`MethodCacheAtoms::readers`).
    pub(crate) atoms: Box<[Atom]>,
    /// Global cells whose values it holds (`heap::subscribe_global_value`).
    pub(crate) globals: Box<[crate::heap::GlobalFact]>,
}

/// The Stable facts a lowering folded so far.
#[derive(Default)]
pub(super) struct StableUses {
    /// Holder shapes whose prototypes' values the code took as constants.
    holders: BTreeSet<u32>,
    /// Atoms it stores directly because no method cache named them.
    atoms: BTreeSet<Atom>,
    /// Global cells whose values the code took as constants.
    globals: BTreeSet<crate::heap::GlobalFact>,
    /// Loops, by header and backedge, entered only through forward edges that proved the realm
    /// (`Lowering::hoist_stable`): their blocks hold the proof.
    realm_loops: Vec<(usize, usize)>,
    /// Loops, likewise, entered only through forward edges that read the invalidation word, whose
    /// bodies change no Stable fact's subject (`keeps_stable_facts`): their blocks hold every
    /// Stable fact, which a change made elsewhere retires before the loop is entered again.
    stable_loops: Vec<(usize, usize)>,
    /// The PCs a run of the body starts at.
    pub(super) starts: Vec<usize>,
}

impl StableUses {
    pub(super) fn take(&mut self) -> StableFacts {
        StableFacts { holders: std::mem::take(&mut self.holders).into_iter().collect(),
            atoms: std::mem::take(&mut self.atoms).into_iter().collect(),
            globals: std::mem::take(&mut self.globals).into_iter().collect() }
    }
}

impl Lowering<'_, '_> {
    /// The division this lowering specializes under: online where it may take code
    /// dependencies on the VM (`VmFacts::online`), offline for code shared across VMs.
    pub(super) fn division(&self) -> Division {
        if self.vm.online.is_some() { Division::Online } else { Division::Offline }
    }

    /// The code's invalidation word, as it is now (`code_dependencies::Invalidation`).
    fn invalidation_word(&mut self, online: OnlineFacts<'_>) -> ir::Value {
        let address = self.ins().iconst(types::I64, online.invalidation as i64);
        self.ins().load(types::I64, MemFlagsData::trusted(), address, 0)
    }

    /// Holds every Stable fact the code folded from here to the block's next helper that may
    /// change one: an invalidation point, which reads the word once and leaves through `exit`
    /// where a change retired the code (Execution laws 2: the change triggers, a use checks
    /// nothing).
    pub(super) fn hold_stable(&mut self, state: &mut State, exit: ir::Block) {
        if state.proofs.stable || self.stable.stable_loops.iter().any(|(header, end)| (*header..=*end).contains(&self.at)) {
            return;
        }
        self.read_invalidation(state, exit);
    }

    /// The invalidation point of `hold_stable`, made here.
    pub(super) fn read_invalidation(&mut self, state: &mut State, exit: ir::Block) {
        let online = self.vm.online.expect("only the online division folds Stable facts");
        let invalidated = self.invalidation_word(online);
        let held = self.ins().icmp_imm_u(IntCC::Equal, invalidated, 0);
        emit_js_guard(&mut self.builder, held, exit);
        state.proofs.stable = true;
    }


    /// Holds the values of prototypes of exact shape `holder` (`hold_stable`).
    pub(super) fn hold_holder(&mut self, state: &mut State, exit: ir::Block, holder: u32) {
        self.hold_stable(state, exit);
        self.stable.holders.insert(holder);
    }

    /// Holds the value of global cell `fact` (`hold_stable`), read from the global object
    /// `global` of the realm compiled against: a realm reads its own global's cells, so a block
    /// first proves that realm active (`prove_realm`).
    pub(super) fn hold_global(&mut self, state: &mut State, exit: ir::Block, fact: crate::heap::GlobalFact,
        realm: (Value, i32)) {
        self.hold_stable(state, exit);
        self.stable.globals.insert(fact);
        self.prove_realm(state, exit, realm);
    }

    /// Proves the realm of global object `global` active, from the VM's field at `realm_offset`.
    /// A call returns to its caller's realm, so the proof outlives every call.
    pub(super) fn prove_realm(&mut self, state: &mut State, exit: ir::Block, realm: (Value, i32)) {
        if state.proofs.realm || self.stable.realm_loops.iter().any(|(header, end)| (*header..=*end).contains(&self.at)) {
            return;
        }
        self.test_realm(state, exit, realm);
    }

    /// The realm test of `prove_realm`, made here.
    pub(super) fn test_realm(&mut self, state: &mut State, exit: ir::Block, (global, realm_offset): (Value, i32)) {
        let vm = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, vm));
        let active = self.ins().load(types::I64, MemFlagsData::trusted(), vm, realm_offset);
        let same = self.ins().icmp_imm_u(IntCC::Equal, active, global.to_bits() as i64);
        emit_js_guard(&mut self.builder, same, exit);
        state.proofs.realm = true;
    }

    /// Where no method cache names `atom` now, a store admitted while `specialized` holds:
    /// that is a Stable fact, which a cache naming it retires (`MethodCacheAtoms::name`), so the
    /// store reads the invalidation word, not the cache's names.
    pub(super) fn store_held_unnamed(&mut self, atom: Atom, specialized: ir::Value) -> Option<ir::Value> {
        let online = self.vm.online.filter(|online| !online.names(atom))?;
        self.stable.atoms.insert(atom);
        let invalidated = self.invalidation_word(online);
        let held = self.builder.ins().icmp_imm_u(IntCC::Equal, invalidated, 0);
        Some(self.builder.ins().band(specialized, held))
    }

    /// Whether a run enters loop `header..=end` only at its header, by edges that each make the
    /// proofs hoisted there: no start lies past its header (a start at it hoists on its entry
    /// edge), every edge from outside it is a forward edge to its header (`continue_at`, which
    /// hoists), and no branch table hides an edge.
    pub(super) fn enters_only_at_header(&self, header: usize, end: usize) -> bool {
        let inside = header..=end;
        let entered_within = self.unit.instructions.iter().enumerate().any(|(pc, instruction)|
            instruction.op().control_flow_layout() == ControlFlowLayout::BranchTable
                || (!inside.contains(&pc) && successors(pc, *instruction).into_iter().flatten()
                    .any(|target| (target > header && target <= end) || (target == header && pc > end))));
        !entered_within && !self.stable.starts.iter().any(|start| *start > header && *start <= end)
    }

    /// Whether `instruction` at `pc` changes no Stable fact's subject and runs no guest code: it
    /// reads natively or leaves (`reads_only`, here also where it returns from an inlined frame),
    /// or reads the global scope (`global_read`); calls, stores and constructions do not qualify.
    pub(super) fn keeps_stable_facts(&self, pc: usize, instruction: WideInstruction) -> bool {
        match instruction.op() {
            Op::LoadName | Op::LoadNameCall => self.global_reads.contains_key(&pc),
            Op::GetField => !instruction.writes_current_this(),
            Op::GetIndex => true,
            op if op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary => true,
            _ => reads_only(&self.program.constants, instruction),
        }
    }

    /// Notes loop `header..=end` as holding the proofs hoisted to its entry edges.
    pub(super) fn hold_in_loop(&mut self, header: usize, end: usize, stable: bool, realm: bool) {
        if stable { self.stable.stable_loops.push((header, end)); }
        if realm { self.stable.realm_loops.push((header, end)); }
    }
}
