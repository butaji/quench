//! The lane frame's property callouts (`staged::PropertyFrame`): what the property rows read
//! back of the lowering's proofs of an object (its record header, its shape, its layout), the
//! reads a loop or an OSR entry already holds, the code dependencies a folded prototype value
//! takes and the VM's own probe of a site (`frame_stores`: the store callouts). The rows decide
//! what a read is; this frame owns the facts it is proved under, kept in `State` beside the
//! registers'.

use super::*;
use crate::vm::compiled::regions;
use super::field_plans::LanePlans;
use super::frame::calls;
use crate::staged::ic::Reached;
use crate::staged::property::Owner;
use crate::staged::PropertyFrame;
use crate::staged::Layout;

/// What the lowering holds of the instruction's field access, beside the register frame.
pub(super) struct FieldFacts<'a> {
    /// The plans the instruction's sites proved (`field_plans::with_read_plan`).
    pub(super) plans: &'a LanePlans<'a>,
    /// The region's entry proofs: the receiver's record, its shape, the bound field slots.
    pub(super) entry: Option<&'a Entry>,
    /// Whether the unit makes native calls, after which no read folds the receiver's entry facts.
    pub(super) native_calls: bool,
    /// The values the access already has from a loop's entry or an OSR region's record.
    pub(super) held: HeldRead,
    /// Where a folded prototype value takes its code dependency.
    pub(super) stable: StableHold<'a>,
}

/// What an instruction's field access already holds: the reads its value may come from, and for
/// a store the slot its loop projected and the run's admission of the atom.
#[derive(Clone, Copy, Default)]
pub(super) struct HeldRead {
    /// A read the loop made once on its entry edge.
    pub(super) hoisted: Option<LoopRead>,
    /// The word of the OSR region's entry record that holds the field's value.
    pub(super) entry_word: Option<u32>,
    /// The value an earlier read of the first field proved (`LocalProofs::fields`).
    pub(super) known: Option<ir::Value>,
    /// The slot a loop's entry projected for the store at this access (`StorePlan::Entry`).
    pub(super) slot: Option<LoopStore>,
    /// The test that no method cache names the atom a store writes: the entry's where the run
    /// calls nothing, made at the store where it does.
    pub(super) admitted: Option<ir::Value>,
}

/// The Stable facts a lowering takes, and where it takes them.
pub(super) struct StableHold<'a> {
    /// Exact shapes of the prototypes whose values the code holds (`StableUses::holders`).
    pub(super) holders: &'a mut BTreeSet<u32>,
    /// The loops that hold every Stable fact (`StableUses::stable_loops`).
    pub(super) loops: &'a [(usize, usize)],
    /// The facts of the VM the code depends on; none for code shared across VMs.
    pub(super) online: Option<OnlineFacts<'a>>,
    /// The unit PC being lowered, which a loop that holds every Stable fact contains.
    pub(super) at: usize,
}

impl<'a> EmitFrame<'a> {
    /// The region's entry proofs, where the lowering has them.
    pub(super) fn entry(&self) -> Option<&'a Entry> { self.fields.as_ref()?.entry }

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
    pub(super) fn proof_key(&self, owner: Owner) -> Option<Option<Register>> {
        match owner {
            Owner::Temporary => None,
            Owner::This => Some(None),
            Owner::Register(register) => Some((!self.is_receiver(owner)).then_some(register)),
        }
    }

    /// The proofs of the probed fast path do not dominate the probe's join: past it only what
    /// held before the read does.
    fn retire_fast_proofs(&mut self, owner: Owner, before: &State) {
        self.state.proofs.headers.clone_from(&before.proofs.headers);
        self.state.proofs.readable.clone_from(&before.proofs.readable);
        self.state.proofs.layouts.retain(|key, _| before.proofs.layouts.contains_key(key));
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
        Some(stage.builder.use_var(self.this?))
    }

    fn receiver_record(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, object: ir::Value, readable: bool,
        miss: ir::Block) -> Option<Reached<ir::Value, ir::Value>> {
        let entry = self.entry()?;
        if self.is_receiver(owner) {
            let receiver = &entry.receiver;
            return Some(Reached { record: stage.builder.use_var(receiver.object),
                shape: Some(stage.builder.use_var(receiver.shape)), inline: Some(stage.builder.use_var(receiver.inline)) });
        }
        let header = match owner {
            Owner::Temporary => {
                let records = headers::object_table_base(stage.builder, entry, self.payload);
                let functions = emit_js_record_base(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, function_record_base));
                ObjectHeader::prove_readable(stage.builder, object, records, functions, miss)
            }
            Owner::This => headers::object_header(stage.builder, self.registers, self.state, (entry, self.payload), None, readable, miss),
            Owner::Register(register) => headers::object_header(stage.builder, self.registers, self.state,
                (entry, self.payload), Some(register), readable, miss),
        };
        Some(Reached { record: header.address, shape: Some(header.shape), inline: Some(header.inline) })
    }

    /// The shape the owner's header holds as a constant, or its kept layout's.
    fn proven_shape(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner) -> Option<u32> {
        let key = self.proof_key(owner)?;
        headers::header_shape(stage.builder, self.state, key).or_else(||
            self.state.proofs.layouts.range((key, 0)..=(key, u32::MAX)).next().map(|((_, shape), _)| *shape))
    }

    fn prove_shape(&mut self, stage: &mut Emit<'b, 'c>, owner: Owner, shape: u32, direct: bool) {
        let Some(key) = self.proof_key(owner) else { return };
        headers::note_header_shape(stage.builder, self.state, key, shape);
        // A read that proves the receiver a direct object of its shape proves it for the block,
        // a receiver alias's too.
        if let (true, Owner::Register(register)) = (direct, owner) {
            self.state.values[usize::from(register)] = RegisterFact::definition(Type::direct_object(shape), Representation::Tagged);
        }
    }

    fn kept_layout(&self, owner: Owner, shape: u32) -> Option<Layout<ir::Value, ir::Value>> {
        let key = self.proof_key(owner)?;
        self.state.proofs.layouts.get(&(key, shape)).map(|layout| layout.layout())
    }

    fn keep_layout(&mut self, owner: Owner, shape: u32, layout: Layout<ir::Value, ir::Value>) {
        if let Some(key) = self.proof_key(owner) {
            self.state.proofs.layouts.insert((key, shape), CheckedObject::of(layout));
        }
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
        calls::field_store_miss(self, stage, object, atom, site, value, EmitProof::FieldStoreMiss { miss })
    }

    /// The run's test that no method cache names the atom stored (`HeldRead::admitted`).
    fn store_admitted(&mut self, stage: &mut Emit<'b, 'c>, miss: ir::Block) -> Option<()> {
        let admitted = self.fields.as_ref()?.held.admitted?;
        emit_js_guard(stage.builder, admitted, miss);
        Some(())
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
            Some(()) => { stage.builder.ins().jump(done, &[]); }
            None => { stage.builder.ins().jump(general, &[]); }
        }
        stage.builder.switch_to_block(general);
        slow(stage, self, miss)?;
        stage.builder.ins().jump(done, &[]);
        stage.builder.switch_to_block(done);
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
        let header = self.proof_key(owner).map(|key| headers::object_header(stage.builder, self.registers, self.state,
            (entry, self.payload), key, true, miss));
        let builder = &mut *stage.builder;
        let (slow, done, packed) = (builder.create_block(), builder.create_block(), builder.create_block());
        builder.set_cold_block(slow);
        let packed_address = builder.append_block_param(packed, types::I64);
        let result = builder.append_block_param(done, types::I64);
        let (address, _) = emit_js_cached_field_slot(builder, self.payload, object,
            site, true, slow, Some(binding), header, Some(packed));
        let value = builder.ins().load(types::I64, regions::object_slots(), address, 0);
        builder.ins().jump(done, &[ir::BlockArg::Value(value)]);
        builder.switch_to_block(packed);
        let narrow = builder.ins().load(types::I32, regions::payload(), packed_address, 0);
        let table = match self.helpers {
            #[cfg(feature = "aot")]
            LaneHelpers::Bound(_, symbol) => NarrowTable::Linked(symbol),
            _ => NarrowTable::Process,
        };
        emit_narrow_read(builder, narrow, table, done);
        builder.switch_to_block(slow);
        let value = self.field_get_host(stage, object, atom, site, miss)?;
        stage.builder.ins().jump(done, &[ir::BlockArg::Value(value)]);
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
            Some(read) => { stage.builder.ins().jump(join, &[ir::BlockArg::Value(read)]); }
            None => { stage.builder.ins().jump(generic, &[]); }
        }
        stage.builder.switch_to_block(generic);
        let mut dominating = before.clone();
        std::mem::swap(&mut *self.state, &mut dominating);
        let probed = self.field_probe(stage, owner, object, atom, site, miss);
        std::mem::swap(&mut *self.state, &mut dominating);
        stage.builder.ins().jump(join, &[ir::BlockArg::Value(probed?)]);
        stage.builder.switch_to_block(join);
        self.retire_fast_proofs(owner, &before);
        Some(value)
    }
}

/// Word `word` of the loop's entry record (`JsStepDispatch::loop_entry`).
pub(super) fn entry_record_word(builder: &mut FunctionBuilder<'_>, payload: ir::Value, word: u32) -> ir::Value {
    let entry = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, loop_entry));
    let offset = i32::try_from(usize::try_from(word).expect("entry word fits usize")
        * std::mem::size_of::<u64>()).expect("entry word offset fits i32");
    builder.ins().load(types::I64, crate::vm::compiled::regions::loop_entry(), entry, offset)
}
