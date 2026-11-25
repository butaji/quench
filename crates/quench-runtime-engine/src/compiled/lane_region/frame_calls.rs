//! Stage-generic callouts for a lane frame: the interrupt/collection safepoint, the
//! element store helper and a literal allocation's completion.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::EmitFrame;
use super::super::{emit_js_guard, emit_js_payload_word, emit_js_record_base, emit_lane_helper_call, Emit, EmitProof,
    JsStepDispatch, LaneHelper};
use super::{BindingTime, Fact, Proof, Projection, RegisterFact, Representation, Type};
use quench_runtime::bytecode::{Register, RegisterWindow};
use quench_runtime::staged::{Allocation, InlineEmpty};
use cranelift_codegen::ir;
use quench_runtime::Value;
use cranelift_codegen::ir::types;
use quench_runtime::staged::machine::{Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use super::super::object_slots::emit;
use crate::compiled::regions;

/// A fault-polling body records its poll instruction: the poll page's fault takes it at a set flag. A body that polls
/// by compare runs the staged back-edge poll instead (`staged::poll::back_edge_poll`, `Lowering::interrupt_check`).
pub(crate) fn safepoint(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, proof: EmitProof) -> Option<()> {
    safepoint_parts(stage.builder, frame.polls, proof)
}

/// A Number return: the register in the int32 or binary64 view its facts favor, jumped to a
/// block that `Lowering::finish_number_returns` completes with the joined format.
pub(crate) fn number_return(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, register: Register) -> Option<()> {
    let super::Edges::ValueGuarded { miss } = frame.edges else { return None };
    let fact = frame.state.values[usize::from(register)].value;
    let value = if fact.ty.is_subtype(Type::INT32) || fact.representation == Representation::Int32 {
        super::register_int_value(stage.builder, frame.registers, frame.state, register, miss).word()
    } else {
        super::register_number_value(stage.builder, frame.registers, frame.state, register, miss).word()
    };
    let fact = frame.state.values[usize::from(register)].value;
    let machine = match fact.representation {
        Representation::Int32 => types::I32,
        Representation::Float64 => types::F64,
        Representation::Int64 | Representation::Tagged => unreachable!("Number return has an int32 or binary64 view"),
    };
    let block = stage.builder.create_block();
    stage.builder.append_block_param(block, machine);
    stage.goto(block, &[value]);
    frame.number_returns.push(super::super::NumberReturn { block, fact });
    Some(())
}

/// The safepoint emission without a frame: the entry poll-latch fill has no register state.
pub(crate) fn safepoint_parts(builder: &mut cranelift_frontend::FunctionBuilder<'_>,
    polls: &mut smallvec::SmallVec<[cranelift_codegen::ir::Inst; super::super::INLINE_EFFECT_EDGES]>,
    proof: EmitProof) -> Option<()> {
    let EmitProof::Safepoint { interrupt } = proof else { return None };
    let requested = emit(builder).raw_atomic_load(Lane::I8, regions::dispatch(), interrupt);
    let cranelift_codegen::ir::ValueDef::Result(inst, _) = builder.func.dfg.value_def(requested) else { return None };
    polls.push(inst);
    Some(())
}

/// The element store's host helper: its nonzero answer means the store landed.
pub(crate) fn index_set_helper(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>,
    base: cranelift_codegen::ir::Value, key: cranelift_codegen::ir::Value,
    value: cranelift_codegen::ir::Value, proof: EmitProof) -> Option<()> {
    let EmitProof::IndexSetHelper { miss } = proof else { return None };
    index_set(frame, stage, (base, key, value), miss);
    Some(())
}

/// The element store's host helper over the tagged `base`, `key` and `value`: its nonzero answer
/// means the store landed, else the body leaves at `miss`.
pub(crate) fn index_set(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>,
    (base, key, value): (ir::Value, ir::Value, ir::Value), miss: ir::Block) {
    let payload = frame.payload;
    let address = frame.helpers.target(&mut stage.builder, LaneHelper::IndexSet);
    let stored = emit_lane_helper_call(stage.builder, payload, address, &[base, key, value]);
    let hit = stage.raw_compare_imm(Relation::NotEqual, stored, 0);
    emit_js_guard(stage.builder, hit, miss);
}

/// A literal allocation's DELETED answer (a due collection) leaves at `miss`; otherwise the
/// literal is the destination's tagged value.
#[inline(always)]
pub(crate) fn finish_allocation(stage: &mut Emit<'_, '_>, frame: &mut EmitFrame<'_>, destination: Register,
    value: ir::Value, miss: ir::Block) {
    quench_runtime::staged::protocol::guard_received(stage, value, miss);
    <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value,
        RegisterFact::definition(Type::ANY, Representation::Tagged));
}

/// The address of a stack run holding the tagged words of `registers`, which an allocator
/// reads as the parts of a literal or a concatenation.
fn register_words(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, registers: impl ExactSizeIterator<Item = Register>) -> ir::Value {
    let word = types::I64.bytes();
    let slot = stage.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
        word * (registers.len() as u32).max(1), word.trailing_zeros() as u8));
    for (index, register) in registers.enumerate() {
        let value = <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::tagged(frame, stage, register);
        stage.slot_store(slot, Lane::I64, value, (word as usize * index) as i32);
    }
    stage.slot_address(slot, 0)
}

/// The parts address and count an allocator takes for `window` (`register_words`).
fn window_arguments(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, window: RegisterWindow) -> [ir::Value; 2] {
    [register_words(frame, stage, window.base..window.base + window.count), stage.raw_const(Lane::I64, i64::from(window.count))]
}

/// An allocation row by the lane's own allocator of it (`LaneHelper::NewObject`, `NewArray`, `NewRegExp`, `NewConstArray`,
/// `NewObjectPair`, `NewObjectLiteral`, `NewClosure`, `NewTemplate`, `NewConcat`): each leaves for a due collection first, and a
/// DELETED answer leaves at the proof's miss edge.
pub(crate) fn allocate_value(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    allocation: Allocation, proof: EmitProof) -> Option<ir::Value> {
    match allocation.inline_empty() {
        Some(InlineEmpty::Object) => return allocate_template(frame, stage, destination, EMPTY_OBJECT, LaneHelper::NewObject, &[], proof),
        Some(InlineEmpty::Array) => {
            let arguments = [stage.raw_const(Lane::I64, 0), stage.raw_const(Lane::I64, 0)];
            return allocate_template(frame, stage, destination, EMPTY_ARRAY, LaneHelper::NewArray, &arguments, proof);
        }
        None => {}
    }
    let (helper, arguments) = match allocation {
        Allocation::Empty | Allocation::Array { length: 0, .. } => unreachable!("an empty allocation is inline"),
        // `numbers` only sizes the backing a Number-only literal starts with; a plain hole array is as exact (`Allocation::Array`).
        Allocation::Array { length, .. } =>
            (LaneHelper::NewArray, vec![stage.raw_const(Lane::I64, i64::from(length)), stage.raw_const(Lane::I64, 0)]),
        Allocation::ArrayOf { elements } => {
            let registers = elements.registers();
            let words = register_words(frame, stage, registers.iter().copied());
            (LaneHelper::NewArray, vec![stage.raw_const(Lane::I64, registers.len() as i64), words])
        }
        Allocation::RegExp { site, callee } => {
            // A callee-argument site passes its call's callee as the register holds it; any other
            // passes `DELETED`, which is no String method, so the allocator makes a fresh object.
            let callee = match callee {
                Some(callee) => <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::tagged(frame, stage, callee),
                None => stage.raw_const(Lane::I64, Value::DELETED.to_bits() as i64),
            };
            (LaneHelper::NewRegExp, vec![stage.raw_const(Lane::I64, i64::from(site)), callee])
        }
        Allocation::ConstArray { constant, count } =>
            (LaneHelper::NewConstArray, vec![stage.raw_const(Lane::I64, i64::from(constant)), stage.raw_const(Lane::I64, i64::from(count))]),
        Allocation::ObjectPair { site, first, second } => {
            let first = <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::tagged(frame, stage, first);
            let second = <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::tagged(frame, stage, second);
            (LaneHelper::NewObjectPair, vec![stage.raw_const(Lane::I64, i64::from(site)), first, second])
        }
        Allocation::ObjectLiteral { site, window } => {
            let [parts, count] = window_arguments(frame, stage, window);
            (LaneHelper::NewObjectLiteral, vec![stage.raw_const(Lane::I64, i64::from(site)), parts, count])
        }
        Allocation::Closure { function } => (LaneHelper::NewClosure, vec![stage.raw_const(Lane::I64, i64::from(function))]),
        Allocation::Template { template } => (LaneHelper::NewTemplate, vec![stage.raw_const(Lane::I64, i64::from(template))]),
        Allocation::Concat { window } => {
            (LaneHelper::NewConcat, window_arguments(frame, stage, window).to_vec())
        }
    };
    frame.allocate(stage, helper, destination, &arguments, proof)
}

/// Where an inline allocation finds its record: the cursor, the record reservation's base, the record's size and
/// tag, and the template words (`JsStepDispatch`'s offsets) of one allocation kind.
#[derive(Clone, Copy)]
struct Template {
    cursor: usize,
    records: usize,
    record_size: usize,
    tag: u64,
    layout: usize,
    words: usize,
}

/// An empty ordinary object (`Allocation::Empty`), from the object space's cursor and template.
const EMPTY_OBJECT: Template = Template {
    cursor: std::mem::offset_of!(JsStepDispatch, head.object_cursor),
    records: std::mem::offset_of!(JsStepDispatch, head.object_record_base),
    record_size: quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.record_size,
    tag: quench_runtime::value::ValueLayout::DIRECT_OBJECT_TAG,
    layout: std::mem::offset_of!(JsStepDispatch, head.empty_object_layout),
    words: std::mem::offset_of!(JsStepDispatch, head.empty_object_payload),
};

/// A zero-length array (`Allocation::Array { length: 0 }`), from the array space's cursor and template: its
/// elements are the static empty header, so its record is its two words alone (`ArrayObject::fresh`).
const EMPTY_ARRAY: Template = Template {
    cursor: std::mem::offset_of!(JsStepDispatch, head.array_cursor),
    records: std::mem::offset_of!(JsStepDispatch, head.array_record_base),
    record_size: quench_runtime::tier_surface::NATIVE_ARRAY_LAYOUT.record_size,
    tag: quench_runtime::value::ValueLayout::DIRECT_ARRAY_TAG,
    layout: std::mem::offset_of!(JsStepDispatch, head.empty_array_layout),
    words: std::mem::offset_of!(JsStepDispatch, head.empty_array_payload),
};

/// An empty allocation by the kind's cursor (`Template`). While the cursor's run has an id (`next < limit`) and the
/// kind's record words are captured (`JsStepDispatch::empty_*_layout`), the id is bumped and the record's two words
/// are stored: no call, and no occupancy write, since the run was occupied and charged when it was claimed.
/// Otherwise the lane's helper (`helper`, with `arguments`) makes it. That helper checks for a due collection
/// first, and a spent run is refilled only there, so every run is checked once before it is used.
fn allocate_template(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register, template: Template,
    helper: LaneHelper, arguments: &[ir::Value], proof: EmitProof) -> Option<ir::Value> {
    use quench_runtime::staged::bump::{bump_record, Template as Bump};
    let EmitProof::Allocation { miss } = proof else { return None };
    let payload = frame.payload;
    let cursor = emit_js_record_base(stage.builder, payload, template.cursor);
    let layout = emit_js_payload_word(stage.builder, payload, template.layout);
    let words = emit_js_payload_word(stage.builder, payload, template.words);
    let records = emit_js_record_base(stage.builder, payload, template.records);
    let slow = stage.builder.create_block();
    let joined = stage.builder.create_block();
    let made = stage.builder.append_block_param(joined, types::I64);
    let bump = Bump { records, record_size: template.record_size as u64, layout, words, tag: template.tag };
    let value = bump_record(stage, cursor, bump, slow).expect("an Emit guard always continues");
    stage.goto(joined, &[value]);

    stage.builder.switch_to_block(slow);
    stage.builder.seal_block(slow);
    let address = frame.helpers.target(&mut stage.builder, helper);
    // The allocators take the payload after the VM and residual (`compiled_new_object`, `new_array_in_lane`), as `allocate` passes it.
    let value = emit_lane_helper_call(stage.builder, payload, address, &[&[payload], arguments].concat());
    stage.goto(joined, &[value]);

    stage.builder.switch_to_block(joined);
    stage.builder.seal_block(joined);
    finish_allocation(stage, frame, destination, made, miss);
    Some(made)
}

/// The VM's field-store hit (`LaneHelper::FieldSet`): its false answer leaves at the miss edge.
pub(crate) fn field_store_miss(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, object: ir::Value,
    atom: u32, site: u16, value: ir::Value, proof: EmitProof) -> Option<()> {
    let EmitProof::FieldStoreMiss { miss } = proof else { return None };
    let payload = frame.payload;
    let atom = atom_word(frame, stage, atom);
    let site = stage.raw_const(Lane::I64, i64::from(site));
    let set = frame.helpers.target(&mut stage.builder, LaneHelper::FieldSet);
    let stored = emit_lane_helper_call(stage.builder, payload, set, &[object, atom, site, value]);
    let ok = stage.raw_compare_imm(Relation::NotEqual, stored, 0);
    emit_js_guard(stage.builder, ok, miss);
    Some(())
}

/// The word `(atom << 16) | site` the field helpers take.
fn atom_site_word(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, atom: u32, site: u16) -> ir::Value {
    if !frame.helpers.links() { return stage.raw_const(Lane::I64, (u64::from(atom) << u16::BITS | u64::from(site)) as i64); }
    let atom = atom_word(frame, stage, atom);
    crate::symbol_words::atom_site_word(stage.builder, atom, site)
}

/// The VM's cached read of `atom` of `object` through `site` (`LaneHelper::FieldGet`): a
/// DELETED answer leaves at `miss`.
pub(crate) fn field_get_host(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, object: ir::Value, atom: u32,
    site: u16, miss: ir::Block) -> ir::Value {
    let packed = atom_site_word(frame, stage, atom, site);
    let get = frame.helpers.target(&mut stage.builder, LaneHelper::FieldGet);
    let value = emit_lane_helper_call(stage.builder, frame.payload, get, &[object, packed]);
    guard_word(frame, stage, value, Relation::NotEqual, Value::DELETED, miss);
    value
}

/// The VM's own, polymorphic and adding store entries (`LaneHelper::FieldAdd`): a false
/// answer leaves at `miss`.
pub(crate) fn field_add(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, object: ir::Value, atom: u32,
    site: u16, value: ir::Value, miss: ir::Block) {
    let write = atom_site_word(frame, stage, atom, site);
    let add = frame.helpers.target(&mut stage.builder, LaneHelper::FieldAdd);
    let stored = emit_lane_helper_call(stage.builder, frame.payload, add, &[frame.payload, object, write, value]);
    let hit = stage.raw_compare_imm(Relation::NotEqual, stored, 0);
    emit_js_guard(stage.builder, hit, miss);
}

/// The lane's own global-name read word (`LaneHelper::GlobalGet`): `Value::DELETED` where it misses.
fn global_word(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, atom: u32, site: u16) -> ir::Value {
    let (atom, site) = (atom_word(frame, stage, atom), stage.raw_const(Lane::I64, i64::from(site)));
    let get = frame.helpers.target(&mut stage.builder, LaneHelper::GlobalGet);
    emit_lane_helper_call(stage.builder, frame.payload, get, &[atom, site])
}

/// Leaves at `miss` unless `word` compares `relation` to `value`.
fn guard_word(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, word: ir::Value, relation: Relation, value: Value, miss: ir::Block) {
    let held = stage.raw_compare_imm(relation, word, frame.roots.embed_heap_identity(value) as i64);
    emit_js_guard(stage.builder, held, miss);
}

/// The lane's own global-name read: a DELETED answer leaves at the miss edge, otherwise the
/// value is the destination's tagged word.
fn global_read(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    atom: u32, site: u16, ty: Type, proof: EmitProof) -> Option<()> {
    let EmitProof::GlobalRead { miss: Some(miss), .. } = proof else { return None };
    let value = global_word(frame, stage, atom, site);
    guard_word(frame, stage, value, Relation::NotEqual, Value::DELETED, miss);
    let fact = if ty == Type::ANY { RegisterFact::definition(ty, Representation::Tagged) } else {
        RegisterFact::admit(Fact { ty, representation: Representation::Tagged, binding: BindingTime::Stable,
            proof: Proof::Dependency, projection: Projection::Exact }).expect("a Stable fact's dependency proves any type")
    };
    <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value, fact);
    Some(())
}

/// The word of `atom` under the frame's helper binding.
fn atom_word(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, atom: u32) -> ir::Value {
    frame.helpers.atom_word(stage.builder, atom)
}

/// `value` as the tagged immediate `destination` holds under `fact`.
fn define_immediate(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register, value: Value,
    fact: RegisterFact) -> Option<()> {
    let bits = stage.raw_const(Lane::I64, frame.roots.embed_heap_identity(value) as i64);
    <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, bits, fact);
    Some(())
}

/// An immutable global's value, defined as the immediate of its own type.
fn global_constant(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register, value: Value) -> Option<()> {
    define_immediate(frame, stage, destination, value, RegisterFact::definition(Type::immediate(value), Representation::Tagged))
}

/// A value held as a Stable fact by its dependency, of type `ty`.
fn global_held(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register, value: Value, ty: Type) -> Option<()> {
    let held = RegisterFact::admit(Fact { ty, representation: Representation::Tagged,
        binding: BindingTime::Stable, proof: Proof::Dependency, projection: Projection::Exact })
        .expect("a Stable fact's dependency proves any type");
    define_immediate(frame, stage, destination, value, held)
}

/// A global lexical binding's word, read live from its address (`staged::lexical_read`): a binding in its temporal dead zone leaves at the
/// miss edge, any other word is the destination's tagged value.
fn global_lexical(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register, address: u64, proof: EmitProof) -> Option<()> {
    let EmitProof::GlobalRead { miss: Some(miss), .. } = proof else { return None };
    let at = stage.raw_const(Lane::I64, address as i64);
    let value = quench_runtime::staged::lexical_read(stage, at, miss)?;
    <EmitFrame<'_> as quench_runtime::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value, RegisterFact::definition(Type::ANY, Representation::Tagged));
    Some(())
}

/// A whole-program value the read checks: a changed binding leaves, then the checked constant is held.
fn global_frozen(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    (atom, site): (u32, u16), (value, ty): (Value, Type), proof: EmitProof) -> Option<()> {
    let EmitProof::GlobalRead { miss: Some(miss), .. } = proof else { return None };
    let actual = global_word(frame, stage, atom, site);
    guard_word(frame, stage, actual, Relation::Equal, value, miss);
    global_held(frame, stage, destination, value, ty)
}

impl quench_runtime::staged::GlobalFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn global_binding(&mut self, proof: EmitProof) -> quench_runtime::staged::GlobalBinding {
        match proof {
            EmitProof::GlobalRead { binding, .. } => binding,
            _ => quench_runtime::staged::GlobalBinding::Dynamic,
        }
    }
    fn global_constant(&mut self, stage: &mut Emit<'_, '_>, destination: Register, value: Value) -> Option<()> { global_constant(self, stage, destination, value) }
    fn global_held(&mut self, stage: &mut Emit<'_, '_>, destination: Register, value: Value, ty: Type) -> Option<()> { global_held(self, stage, destination, value, ty) }
    fn global_frozen(&mut self, stage: &mut Emit<'_, '_>, destination: Register, atom: u32, site: u16, value: Value, ty: Type, proof: EmitProof) -> Option<()> {
        global_frozen(self, stage, destination, (atom, site), (value, ty), proof)
    }
    fn global_lexical(&mut self, stage: &mut Emit<'_, '_>, destination: Register, address: u64, proof: EmitProof) -> Option<()> { global_lexical(self, stage, destination, address, proof) }
    fn global_read(&mut self, stage: &mut Emit<'_, '_>, destination: Register, atom: u32, site: u16, ty: Type, proof: EmitProof) -> Option<()> { global_read(self, stage, destination, atom, site, ty, proof) }
    fn global_write(&mut self, stage: &mut Emit<'_, '_>, source: Register, _atom: u32, _site: u16, value: ir::Value, _initialize: bool) -> Option<()> {
        super::call_out::global_write(self, stage, source, value)
    }
}

impl EmitFrame<'_> {
    /// A literal's allocation by the lane's own `helper`, whose DELETED answer leaves at the proof's miss edge.
    pub(crate) fn allocate(&mut self, stage: &mut Emit<'_, '_>, helper: LaneHelper, destination: Register, arguments: &[ir::Value],
        proof: EmitProof) -> Option<ir::Value> {
        let EmitProof::Allocation { miss } = proof else { return None };
        let payload = self.payload;
        let address = self.helpers.target(&mut stage.builder, helper);
        let value = super::super::emit_lane_helper_call(stage.builder, payload, address, &[&[payload], arguments].concat());
        finish_allocation(stage, self, destination, value, miss);
        Some(value)
    }
}
