//! Stage-generic callouts for a lane frame: the interrupt/collection safepoint, the
//! element store helper and a literal allocation's completion.

use super::EmitFrame;
use super::super::{emit_js_guard, emit_lane_helper_call, Emit, EmitProof, LaneHelper};
use super::{BindingTime, Fact, Proof, Projection, RegisterFact, Representation, Type};
use crate::bytecode::Register;
use cranelift_codegen::ir;
use crate::Value;
use cranelift_codegen::ir::{types, InstBuilder, MemFlagsData, condcodes::IntCC};

/// A fault-polling body records its poll instruction; any other leaves at the miss edge when
/// the flag is set.
pub(super) fn safepoint(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, proof: EmitProof) -> Option<()> {
    safepoint_parts(stage.builder, frame.polls, proof)
}

/// A depth-zero capture of the resolved run: the staged dense read, whose marker or bounds
/// miss leaves at `miss`.
pub(super) fn capture(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, depth: u16, slot: u16, miss: ir::Block) -> Option<ir::Value> {
    let (values, len) = frame.captures.filter(|_| depth == 0)?;
    crate::staged::environment::dense_capture(stage, values, len, slot, miss)
}

/// A Number return: the register in the int32 or binary64 view its facts favor, jumped to a
/// block that `Lowering::finish_number_returns` completes with the joined format.
pub(super) fn number_return(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, register: Register) -> Option<()> {
    let super::Edges::ValueGuarded { miss } = frame.edges else { return None };
    let fact = frame.state.values[usize::from(register)].value;
    let value = if fact.ty.is_subtype(Type::INT32) || fact.representation == Representation::Int32 {
        super::register_int_value(stage.builder, frame.registers, frame.state, register, miss)
    } else {
        super::register_number_value(stage.builder, frame.registers, frame.state, register, miss)
    };
    let fact = frame.state.values[usize::from(register)].value;
    let machine = match fact.representation {
        Representation::Int32 => types::I32,
        Representation::Float64 => types::F64,
        Representation::Tagged => unreachable!("Number return has an unboxed view"),
    };
    let block = stage.builder.create_block();
    stage.builder.append_block_param(block, machine);
    stage.builder.ins().jump(block, &[value.into()]);
    frame.number_returns.push(super::super::NumberReturn { block, fact });
    Some(())
}

/// The safepoint emission without a frame: the entry poll-latch fill has no register state.
pub(in super::super) fn safepoint_parts(builder: &mut cranelift_frontend::FunctionBuilder<'_>,
    polls: &mut smallvec::SmallVec<[cranelift_codegen::ir::Inst; super::super::INLINE_EFFECT_EDGES]>,
    proof: EmitProof) -> Option<()> {
    let EmitProof::Safepoint { interrupt, miss, fault_poll } = proof else { return None };
    let requested = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), interrupt);
    if fault_poll {
        let cranelift_codegen::ir::ValueDef::Result(inst, _) = builder.func.dfg.value_def(requested) else { return None };
        polls.push(inst);
    } else {
        let miss = miss?;
        let requested = builder.ins().uextend(types::I32, requested);
        let clear = builder.ins().icmp_imm_u(IntCC::Equal, requested, 0);
        emit_js_guard(builder, clear, miss);
    }
    Some(())
}

/// The element store's host helper: its nonzero answer means the store landed.
pub(super) fn index_set_helper(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>,
    base: cranelift_codegen::ir::Value, key: cranelift_codegen::ir::Value,
    value: cranelift_codegen::ir::Value, proof: EmitProof) -> Option<()> {
    let EmitProof::IndexSetHelper { miss } = proof else { return None };
    let payload = frame.payload;
    let address = frame.helpers.address(&mut stage.builder, LaneHelper::IndexSet);
    let stored = emit_lane_helper_call(stage.builder, payload, address, &[base, key, value]);
    let hit = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
    emit_js_guard(stage.builder, hit, miss);
    Some(())
}

/// A literal allocation's DELETED answer (a due collection) leaves at `miss`; otherwise the
/// literal is the destination's tagged value.
#[inline(always)]
pub(super) fn finish_allocation(stage: &mut Emit<'_, '_>, frame: &mut EmitFrame<'_>, destination: Register,
    value: ir::Value, miss: ir::Block) {
    let made = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
    super::super::emit_js_guard(stage.builder, made, miss);
    <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value,
        RegisterFact::definition(Type::ANY, Representation::Tagged));
}

/// The VM's field-store hit (`LaneHelper::FieldSet`): its false answer leaves at the miss edge.
pub(super) fn field_store_miss(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, object: ir::Value,
    atom: u32, site: u16, value: ir::Value, proof: EmitProof) -> Option<()> {
    let EmitProof::FieldStoreMiss { miss } = proof else { return None };
    let payload = frame.payload;
    let atom = atom_word(frame, stage, atom);
    let site = stage.builder.ins().iconst(types::I64, i64::from(site));
    let set = frame.helpers.address(&mut stage.builder, LaneHelper::FieldSet);
    let stored = emit_lane_helper_call(stage.builder, payload, set, &[object, atom, site, value]);
    let ok = stage.builder.ins().icmp_imm_u(IntCC::NotEqual, stored, 0);
    emit_js_guard(stage.builder, ok, miss);
    Some(())
}

/// The lane's own global-name read word (`LaneHelper::GlobalGet`): `Value::DELETED` where it misses.
fn global_word(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, atom: u32, site: u16) -> ir::Value {
    let (atom, site) = (atom_word(frame, stage, atom), immediate(stage, i64::from(site)));
    let get = frame.helpers.address(&mut stage.builder, LaneHelper::GlobalGet);
    emit_lane_helper_call(stage.builder, frame.payload, get, &[atom, site])
}

/// Leaves at `miss` unless `word` compares `cc` to `value`.
fn guard_word(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, word: ir::Value, cc: IntCC, value: Value, miss: ir::Block) {
    let held = stage.builder.ins().icmp_imm_u(cc, word, frame.roots.embed_heap_identity(value) as i64);
    emit_js_guard(stage.builder, held, miss);
}

/// The lane's own global-name read: a DELETED answer leaves at the miss edge, otherwise the
/// value is the destination's tagged word.
fn global_read(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    atom: u32, site: u16, ty: Type, proof: EmitProof) -> Option<()> {
    let EmitProof::GlobalRead { miss: Some(miss) } = proof else { return None };
    let value = global_word(frame, stage, atom, site);
    guard_word(frame, stage, value, IntCC::NotEqual, Value::DELETED, miss);
    let fact = if ty == Type::ANY { RegisterFact::definition(ty, Representation::Tagged) } else {
        RegisterFact::admit(Fact { ty, representation: Representation::Tagged, binding: BindingTime::Stable,
            proof: Proof::Dependency, projection: Projection::Exact }).expect("a Stable fact's dependency proves any type")
    };
    <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value, fact);
    Some(())
}

/// The 64-bit immediate `bits`.
fn immediate(stage: &mut Emit<'_, '_>, bits: i64) -> ir::Value { stage.builder.ins().iconst(types::I64, bits) }

/// The word of `atom`: its id now for a live lowering, its symbol for a linking one (`RelocKind::Atom`).
fn atom_word(frame: &EmitFrame<'_>, stage: &mut Emit<'_, '_>, atom: u32) -> ir::Value {
    if !frame.helpers.links() { return immediate(stage, i64::from(atom)); }
    crate::vm::jit_backend::symbol_address(&mut stage.builder, types::I64, super::relocs::RelocKind::Atom as u32, atom)
}

/// `value` as the tagged immediate `destination` holds under `fact`.
fn define_immediate(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register, value: Value,
    fact: RegisterFact) -> Option<()> {
    let bits = immediate(stage, frame.roots.embed_heap_identity(value) as i64);
    <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, bits, fact);
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

/// A whole-program value the read checks: a changed binding leaves, then the checked constant is held.
fn global_frozen(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    (atom, site): (u32, u16), (value, ty): (Value, Type), proof: EmitProof) -> Option<()> {
    let EmitProof::GlobalRead { miss: Some(miss) } = proof else { return None };
    let actual = global_word(frame, stage, atom, site);
    guard_word(frame, stage, actual, IntCC::Equal, value, miss);
    global_held(frame, stage, destination, value, ty)
}

impl crate::staged::GlobalFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn global_constant(&mut self, stage: &mut Emit<'_, '_>, destination: Register, value: Value) -> Option<()> { global_constant(self, stage, destination, value) }
    fn global_held(&mut self, stage: &mut Emit<'_, '_>, destination: Register, value: Value, ty: Type) -> Option<()> { global_held(self, stage, destination, value, ty) }
    fn global_frozen(&mut self, stage: &mut Emit<'_, '_>, destination: Register, atom: u32, site: u16, value: Value, ty: Type, proof: EmitProof) -> Option<()> {
        global_frozen(self, stage, destination, (atom, site), (value, ty), proof)
    }
    fn global_read(&mut self, stage: &mut Emit<'_, '_>, destination: Register, atom: u32, site: u16, ty: Type, proof: EmitProof) -> Option<()> { global_read(self, stage, destination, atom, site, ty, proof) }
    fn global_write(&mut self, stage: &mut Emit<'_, '_>, source: Register, _atom: u32, _site: u16, value: ir::Value, _initialize: bool) -> Option<()> {
        super::call_out::global_write(self, stage, source, value)
    }
    fn global_typeof(&mut self, stage: &mut Emit<'_, '_>, destination: Register, _atom: u32, _site: u16) -> Option<()> {
        super::call_out::global_typeof(self, stage, destination)
    }
}

impl EmitFrame<'_> {
    /// A literal's allocation by the lane's own `helper`, whose DELETED answer leaves at the proof's miss edge.
    pub(super) fn allocate(&mut self, stage: &mut Emit<'_, '_>, helper: LaneHelper, destination: Register, arguments: &[ir::Value],
        proof: EmitProof) -> Option<ir::Value> {
        let EmitProof::Allocation { miss } = proof else { return None };
        let payload = self.payload;
        let address = self.helpers.address(&mut stage.builder, helper);
        let value = super::super::emit_lane_helper_call(stage.builder, payload, address, &[&[payload], arguments].concat());
        finish_allocation(stage, self, destination, value, miss);
        Some(value)
    }
}
