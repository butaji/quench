//! The lane frame's property callouts (`staged::PropertyFrame`): what the property rows read
//! back of the lowering's proofs of an object (its record header, its shape, its layout), the
//! reads a loop or an OSR entry already holds, the code dependencies a folded prototype value
//! takes and the VM's own probe of a site (`frame_stores`: the store callouts). The rows decide
//! what a read is; this frame owns the facts it is proved under, kept in `State` beside the
//! registers'.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::flow::FlowStage;
use quench_runtime::staged::{Region, ShapeFact, Shaped, Stage};
use super::field_plans::LanePlans;
use super::frame::calls;
use quench_runtime::staged::ic::Reached;
use quench_runtime::staged::property::Owner;
use quench_runtime::staged::PropertyFrame;
use quench_runtime::staged::Layout;

/// What the lowering holds of the instruction's field access, beside the register frame.
pub(crate) struct FieldFacts<'a> {
    /// The plans the instruction's sites proved (`field_plans::with_read_plan`).
    pub(crate) plans: &'a LanePlans<'a>,
    /// The region's entry proofs: the receiver's record, its shape, the bound field slots.
    pub(crate) entry: Option<&'a Entry>,
    /// Whether the unit makes native calls, after which no read folds the receiver's entry facts.
    pub(crate) native_calls: bool,
    /// The values the access already has from a loop's entry or an OSR region's record.
    pub(crate) held: HeldRead,
    /// Where a folded prototype value takes its code dependency.
    pub(crate) stable: StableHold<'a>,
    /// The alias region of the access's (class, atom) key, where the solver proves its receiver's class and
    /// the function declared one (`regions::declare_field_regions`); `None` keeps its values in `Region::ObjectSlots`.
    pub(crate) property: Option<cranelift_codegen::ir::AliasRegion>,
}

/// What an instruction's field access already holds: the reads its value may come from, and for
/// a store the slot its loop projected and the run's admission of the atom.
#[derive(Clone, Copy, Default)]
pub(crate) struct HeldRead {
    /// A read the loop made once on its entry edge.
    pub(crate) hoisted: Option<LoopRead>,
    /// The word of the OSR region's entry record that holds the field's value.
    pub(crate) entry_word: Option<u32>,
    /// The value an earlier read of the first field proved (`LocalProofs::fields`).
    pub(crate) known: Option<ir::Value>,
    /// The slot a loop's entry projected for the store at this access (`StorePlan::Entry`).
    pub(crate) slot: Option<LoopStore>,
    /// The test that no method cache names the atom a store writes: the entry's where the run
    /// calls nothing, made at the store where it does.
    pub(crate) admitted: Option<ir::Value>,
    /// `admitted` in its two parts where the store tests both: its run's stores specialize, and
    /// no method cache names the atom; an in-place arm takes the second only for a store that
    /// may call a cache's attention (`store::admit_named`).
    pub(crate) split: Option<(ir::Value, ir::Value)>,
    /// Whether a store the in-place arms miss leaves instead of calling the VM's store
    /// (`Lowering::store_leaves`).
    pub(crate) leaves: bool,
}

/// The Stable facts a lowering takes, and where it takes them.
pub(crate) struct StableHold<'a> {
    /// Exact shapes of the prototypes whose values the code holds (`StableUses::holders`).
    pub(crate) holders: &'a mut quench_runtime::bitset::BitSet,
    /// Exact shapes of the objects whose shape the code holds past a call (`StableUses::shapes`).
    pub(crate) shapes: &'a mut quench_runtime::staged::HeldShapes,
    /// Whether the code holds the typed views it proved past a call (`StableUses::views`).
    pub(crate) views: &'a mut quench_runtime::staged::HeldViews,
    /// The loops that hold every Stable fact (`StableUses::stable_loops`).
    pub(crate) loops: &'a [(usize, usize)],
    /// The facts of the VM the code depends on; none for code shared across VMs.
    pub(crate) online: Option<OnlineFacts<'a>>,
    /// The unit PC being lowered, which a loop that holds every Stable fact contains.
    pub(crate) at: usize,
}

impl<'a> EmitFrame<'a> {
    /// The region's entry proofs, where the lowering has them.
    pub(crate) fn entry(&self) -> Option<&'a Entry> { self.fields.as_ref()?.entry }

    /// Whether `owner`'s object is the entry's receiver, whose record, shape and storage kind
    /// the entry proved: `this`, or a register holding it, in a run that calls nothing.
    fn is_receiver(&self, owner: Owner) -> bool {
        let Some(facts) = &self.fields else { return false };
        !facts.native_calls && match owner {
            Owner::This => true,
            Owner::Register(register) => self.state.proofs.receivers.contains(&register),
            Owner::Temporary => false,
        }
    }

    /// The key of `owner`'s proofs in `State` (`None` is the fixed `this`'s); a receiver's
    /// proofs are shared by every register that holds it, and a temporary keeps none.
    pub(crate) fn proof_key(&self, owner: Owner) -> Option<Option<Register>> {
        match owner {
            Owner::Temporary => None,
            Owner::This => Some(None),
            Owner::Register(register) => Some((!self.is_receiver(owner)).then_some(register)),
        }
    }

    /// The resolved word `owner`'s proofs in `State` are keyed by (`headers`): a receiver's is `this`'s, as its
    /// proofs are shared; a temporary has none.
    fn proof_word(&mut self, builder: &mut FunctionBuilder<'_>, owner: Owner) -> Option<ir::Value> {
        match self.proof_key(owner)? {
            Some(register) => Some(headers::register_word(builder, self.registers, self.state, register)),
            None => Some(headers::this_word(builder, self.this?)),
        }
    }

    /// The other registers that hold `register`'s tagged value, which is one SSA value: copies a
    /// `Move` made (`register_copy`), not yet redefined. Receivers keep their own proofs.
    fn copies(&self, register: Register) -> Vec<Register> {
        let TaggedView::Value(value) = self.state.values[usize::from(register)].tagged else { return Vec::new() };
        (0..self.state.values.len()).filter(|&other| other != usize::from(register)
            && self.state.values[other].tagged == TaggedView::Value(value))
            .filter_map(|other| Register::try_from(other).ok())
            .filter(|&other| !self.is_receiver(Owner::Register(other))).collect()
    }

    /// The proofs of the probed fast path do not dominate the probe's join: past it only what
    /// held before the read does.
    fn retire_fast_proofs(&mut self, owner: Owner, before: &State) {
        self.state.proofs.headers.clone_from(&before.proofs.headers);
        self.state.proofs.records.clone_from(&before.proofs.records);
        self.state.proofs.layouts.retain(|key, _| before.proofs.layouts.contains_key(key));
        if self.state.proofs.global != before.proofs.global { self.state.proofs.global = None; }
        if let Some(Some(register)) = self.proof_key(owner) {
            self.state.values[usize::from(register)] = before.values[usize::from(register)];
        }
    }
}

impl<'a, 'b, 'c> PropertyFrame<Emit<'b, 'c>> for EmitFrame<'a> {
    type Plans = &'a LanePlans<'a>;

    fn plans(&self) -> &'a LanePlans<'a> { self.fields.as_ref().map_or(&LanePlans::NONE, |facts| facts.plans) }

    /// The semantic type the lowering's state holds of the register: a static fact.
    fn value_fact(&self, register: Register) -> Type { self.state.values[usize::from(register)].value.ty }

    fn object_records(&mut self, stage: &mut Emit<'b, 'c>) -> Option<ir::Value> {
        let entry = self.entry().or(self.table_entry)?;
        Some(headers::object_table_base(stage.builder, entry, self.payload))
    }

    /// The entry's `this`, which a body that reads fields has proved bound.
    fn field_this(&mut self, stage: &mut Emit<'b, 'c>, _miss: ir::Block) -> Option<ir::Value> {
        Some(headers::this_word(stage.builder, self.this?))
    }

    fn receiver_record(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, object: ir::Value, readable: bool,
        miss: ir::Block) -> Option<Reached<ir::Value, ir::Value>> {
        let entry = self.entry()?;
        if self.is_receiver(owner) {
            // The entry proved a direct object's record, or none: a function `this` (a method of a
            // constructor) reads its own record here, a store of it leaves.
            let receiver = &entry.receiver;
            let (object, shape, inline) = (stage.builder.use_var(receiver.object), stage.builder.use_var(receiver.shape), stage.builder.use_var(receiver.inline));
            let present = stage.raw_compare_imm(Relation::NotEqual, object, 0);
            if !readable {
                emit_js_guard(stage.builder, present, miss);
                return Some(Reached { shape: Some(shape), inline: Some(inline), ..Reached::bare(object) });
            }
            let (other, join) = (stage.builder.create_block(), stage.builder.create_block());
            stage.builder.set_cold_block(other);
            let [record, shape_at, inline_at] = [types::I64, types::I32, types::I8].map(|ty| stage.builder.append_block_param(join, ty));
            stage.branch_if(present, (join, &[object, shape, inline]), (other, &[]));
            stage.builder.switch_to_block(other);
            let this = stage.builder.use_var(self.this?);
            let records = headers::object_table_base(stage.builder, entry, self.payload);
            let functions = emit_js_record_base(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.function_record_base));
            let class_base = headers::class_table_base(stage.builder, self.payload);
            let header = ObjectHeader::prove_readable(stage.builder, this, records, class_base, functions, miss);
            stage.goto(join, &[header.address, header.shape, header.inline]);
            stage.builder.switch_to_block(join);
            return Some(Reached { shape: Some(shape_at), inline: Some(inline_at), ..Reached::bare(record) });
        }
        let header = match owner {
            Owner::Temporary => {
                let records = headers::object_table_base(stage.builder, entry, self.payload);
                let functions = emit_js_record_base(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.function_record_base));
                let class_base = headers::class_table_base(stage.builder, self.payload);
                ObjectHeader::prove_readable(stage.builder, object, records, class_base, functions, miss)
            }
            Owner::This => self.object_header(stage.builder, entry, None, readable, miss),
            Owner::Register(register) => self.object_header(stage.builder, entry, Some(register), readable, miss),
        };
        Some(Reached { shape: Some(header.shape), inline: Some(header.inline), word: header.word, auxiliary: header.auxiliary, ..Reached::bare(header.address) })
    }

    /// The shape the owner's header holds as a constant, or its kept layout's.
    fn proven_shape(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner) -> Option<(u32, ShapeFact)> {
        let word = self.proof_word(stage.builder, owner)?;
        headers::header_shape(stage.builder, self.state, word).or_else(||
            self.state.proofs.layouts.range((word, 0)..=(word, u32::MAX)).next().map(|((_, shape), kept)| (*shape, kept.fact())))
    }

    fn prove_shape(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, (shape, proof): (u32, Shaped<ir::Value>), direct: bool) {
        let Some(word) = self.proof_word(stage.builder, owner) else { return };
        headers::note_header_shape(stage.builder, self.state, word, (shape, proof.fact()));
        // A read that proves the receiver a direct object of its shape proves it for the block,
        // for each copy of its value too (`copies`).
        if let (true, Owner::Register(register)) = (direct, owner) {
            for proved in std::iter::once(register).chain(self.copies(register)) {
                let fact = &mut self.state.values[usize::from(proved)];
                *fact = RegisterFact { tagged: fact.tagged, ..RegisterFact::definition(Type::direct_object(shape), Representation::Tagged) };
            }
            self.state.proofs.kept_shapes.insert(word, shape);
        }
    }


    fn kept_layout(&self, owner: Owner, object: ir::Value, shape: u32) -> Option<Layout<ir::Value, ir::Value>> {
        if matches!(owner, Owner::Temporary) { return None; }
        self.state.proofs.layouts.get(&(object, shape)).map(|layout| layout.layout())
    }

    fn keep_layout(&mut self, owner: Owner, object: ir::Value, (shape, fact): (u32, ShapeFact), layout: Layout<ir::Value, ir::Value>) {
        if matches!(owner, Owner::Temporary) { return; }
        self.state.proofs.layouts.insert((object, shape), CheckedObject::of(layout, fact));
    }

    fn hold_prototype(&mut self, stage: &mut Emit<'b, 'c>, shape: u32, miss: ir::Block) -> Option<()> {
        self.fields.as_mut()?.stable.hold_holder((&mut *stage.builder, self.helpers), self.state, miss, shape)
    }

    fn known_value(&mut self, _stage: &mut Emit<'b, 'c>, _owner: Owner) -> Option<ir::Value> {
        self.fields.as_ref()?.held.known
    }

    fn hoisted(&mut self, stage: &mut Emit<'b, 'c>, _lookup: FieldLookup) -> Option<ir::Value> {
        let read = self.fields.as_ref()?.held.hoisted?;
        let raw = stage.builder.use_var(read.value);
        Some(if read.representation == Representation::Int32 { int_tagged(stage.builder, raw) } else { raw })
    }

    fn entry_field(&mut self, stage: &mut Emit<'b, 'c>, _lookup: FieldLookup) -> Option<ir::Value> {
        let word = self.fields.as_ref()?.held.entry_word?;
        Some(entry_record_word(stage.builder, self.payload, word))
    }

    fn field_get_host(&mut self, stage: &mut Emit<'b, 'c>, object: ir::Value, atom: u32, site: u16,
        miss: ir::Block) -> Option<ir::Value> {
        Some(calls::field_get_host(self, stage, object, atom, site, miss))
    }

    fn field_add(&mut self, stage: &mut Emit<'b, 'c>, object: ir::Value, atom: u32, site: u16, value: ir::Value,
        miss: ir::Block) -> Option<()> {
        calls::field_add(self, stage, object, atom, site, value, miss);
        Some(())
    }

    fn field_store_host(&mut self, stage: &mut Emit<'b, 'c>, object: ir::Value, atom: u32, site: u16,
        value: ir::Value, miss: ir::Block) -> Option<()> {
        if self.fields.as_ref().is_some_and(|fields| fields.held.leaves) {
            let never = stage.ult(value, value);
            return stage.guard(never, miss);
        }
        calls::field_store_miss(self, stage, object, atom, site, value, EmitProof::FieldStoreMiss { miss })
    }

    /// The run's test that no method cache names the atom stored (`HeldRead::admitted`).
    fn store_admitted(&mut self, stage: &mut Emit<'b, 'c>, miss: ir::Block) -> Option<()> {
        let admitted = self.fields.as_ref()?.held.admitted?;
        emit_js_guard(stage.builder, admitted, miss);
        Some(())
    }

    /// `store_admitted` split where the store holds both parts (`HeldRead::split`): its run's
    /// stores specialize, tested here; that no method cache names the atom, answered.
    fn store_admission(&mut self, stage: &mut Emit<'b, 'c>, miss: ir::Block) -> Option<Option<ir::Value>> {
        let Some((specialized, free)) = self.fields.as_ref()?.held.split else {
            self.store_admitted(stage, miss)?;
            return Some(None);
        };
        emit_js_guard(stage.builder, specialized, miss);
        Some(Some(free))
    }

    fn field_store_bound(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, object: ir::Value, site: (u32, u16),
        value: (ir::Value, Type), miss: ir::Block) -> Option<()> {
        self.bound_store(stage, owner, object, site, value, miss)
    }

    fn entry_slot_store(&mut self, stage: &mut Emit<'b, 'c>, site: u16, value: ir::Value, miss: ir::Block) -> Option<()> {
        self.loop_slot_store(stage, site, value, miss)
    }

    /// The direct store, and where it declines (a shape, the barrier or the storage does not
    /// admit it) the VM's store, which logs what it must; past the join only the proofs that
    /// held before the store do.
    fn stored(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, miss: ir::Block,
        fast: impl FnOnce(&mut Emit<'b, 'c>, &mut Self, ir::Block) -> Option<()>,
        slow: impl FnOnce(&mut Emit<'b, 'c>, &mut Self, ir::Block) -> Option<()>) -> Option<()> {
        let (general, done) = (stage.builder.create_block(), stage.builder.create_block());
        stage.builder.set_cold_block(general);
        let before = self.state.clone();
        match fast(stage, self, general) {
            Some(()) => { stage.jump(done, &[]); }
            None => { stage.jump(general, &[]); }
        }
        stage.builder.switch_to_block(general);
        // The slow path is the fast one's sibling: what the fast path derived is not in its blocks.
        self.state.drop_path_views(&before);
        slow(stage, self, miss)?;
        stage.jump(done, &[]);
        stage.builder.switch_to_block(done);
        self.state.drop_path_views(&before);
        self.retire_fast_proofs(owner, &before);
        Some(())
    }

    /// The VM's own field-cache read (`Vm::cached_field_hit`), prototype holders included,
    /// through the site's bound slot first where the entry bound one; a miss leaves at `miss`.
    fn field_probe(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, object: ir::Value, atom: u32, site: u16,
        miss: ir::Block) -> Option<ir::Value> {
        let entry = self.entry();
        let bound = entry.and_then(|entry| entry.fields.iter().find_map(|(known, binding)| (*known == site).then_some(*binding)));
        let (Some(entry), Some(binding)) = (entry, bound) else {
            return self.field_get_host(stage, object, atom, site, miss);
        };
        let header = self.proof_key(owner).map(|key| self.object_header(stage.builder, entry, key, true, miss));
        let builder = &mut *stage.builder;
        let (slow, done, packed) = (builder.create_block(), builder.create_block(), builder.create_block());
        builder.set_cold_block(slow);
        let packed_address = builder.append_block_param(packed, types::I64);
        let result = builder.append_block_param(done, types::I64);
        let (address, _) = emit_js_cached_field_slot(builder, self.payload, object,
            site, true, slow, Some(binding), header, Some(packed));
        let value = emit(builder).raw_load(Lane::I64, regions::property_flags(self.fields.as_ref().and_then(|facts| facts.property)), address, 0);
        emit(builder).jump(done, &[value]);
        builder.switch_to_block(packed);
        let narrow = emit(builder).load_i32(Region::Payload.mutable(), packed_address, 0);
        let table = match self.helpers {
            LaneHelpers::Bound(_, symbol) => super::super::linked_narrow_table(builder, symbol),
            // A linking lowering names the loader's table by symbol, never this process's address.
            LaneHelpers::Ahead => crate::symbol_words::symbol_address(builder, crate::aot_image::symbols::NARROW_WORDS_NAMESPACE, 0),
            _ => super::super::process_narrow_table(builder),
        };
        emit_narrow_read(builder, narrow, table, done);
        builder.switch_to_block(slow);
        let value = self.field_get_host(stage, object, atom, site, miss)?;
        stage.jump(done, &[value]);
        stage.builder.switch_to_block(done);
        Some(result)
    }

    /// The VM's element read past a hole or an index past the backing (`frame_elements::index_get`).
    fn index_get_host(&mut self, stage: &mut Emit<'b, 'c>, base: ir::Value, key: ir::Value, miss: ir::Block) -> Option<ir::Value> {
        Some(super::frame_elements::index_get(self, stage, (base, key), miss))
    }

    /// The fast read, and for a miss of it the VM's probe, which starts from the proofs that
    /// held before the read, joined (`retire_fast_proofs`).
    fn probed(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, object: ir::Value, (atom, site): (u32, u16),
        miss: ir::Block, fast: impl FnOnce(&mut Emit<'b, 'c>, &mut Self, ir::Block) -> Option<ir::Value>) -> Option<ir::Value> {
        let (generic, join) = (stage.builder.create_block(), stage.builder.create_block());
        stage.builder.set_cold_block(generic);
        let value = stage.builder.append_block_param(join, types::I64);
        let before = self.state.clone();
        match fast(stage, self, generic) {
            Some(read) => { stage.jump(join, &[read]); }
            None => { stage.jump(generic, &[]); }
        }
        stage.builder.switch_to_block(generic);
        let mut dominating = before.clone();
        std::mem::swap(&mut *self.state, &mut dominating);
        let probed = self.field_probe(stage, owner, object, atom, site, miss);
        std::mem::swap(&mut *self.state, &mut dominating);
        stage.jump(join, &[probed?]);
        stage.builder.switch_to_block(join);
        self.state.drop_path_views(&before);
        self.retire_fast_proofs(owner, &before);
        Some(value)
    }
}

/// Word `word` of the loop's entry record (`JsStepDispatch::loop_entry`).
pub(crate) fn entry_record_word(builder: &mut FunctionBuilder<'_>, payload: ir::Value, word: u32) -> ir::Value {
    let entry = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.loop_entry));
    let offset = i32::try_from(usize::try_from(word).expect("entry word fits usize")
        * std::mem::size_of::<u64>()).expect("entry word offset fits i32");
    emit(builder).load_u64(Region::LoopEntry.readonly(), entry, offset)
}
