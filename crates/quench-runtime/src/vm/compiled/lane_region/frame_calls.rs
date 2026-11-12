//! Stage-generic callouts for a lane frame: the interrupt/collection safepoint, the
//! element store helper and a literal allocation's completion.

use super::EmitFrame;
use super::super::{emit_js_guard, emit_lane_helper_call, Emit, EmitProof, LaneHelper};
use super::{BindingTime, Fact, Proof, Projection, RegisterFact, Representation, Type};
use crate::bytecode::{Register, RegisterWindow};
use crate::staged::Allocation;
use cranelift_codegen::ir;
use crate::Value;
use cranelift_codegen::ir::{types, InstBuilder, MemFlagsData, condcodes::IntCC};

/// A fault-polling body records its poll instruction; any other leaves at the miss edge when
/// the flag is set.
pub(super) fn safepoint(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, proof: EmitProof) -> Option<()> {
    safepoint_parts(stage.builder, frame.polls, proof)
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

/// The address of a stack run holding the tagged words of `window`'s registers, which an
/// allocator reads as the parts of a literal or a concatenation.
fn window_words(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, window: RegisterWindow) -> ir::Value {
    let word = types::I64.bytes();
    let slot = stage.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
        word * u32::from(window.count).max(1), word.trailing_zeros() as u8));
    for part in 0..window.count {
        let value = <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::tagged(frame, stage, window.base + part);
        stage.builder.ins().stack_store(types::I64, value, slot, (word as usize * usize::from(part)) as i32);
    }
    stage.builder.ins().stack_addr(types::I64, slot, 0)
}

/// The parts address and count an allocator takes for `window` (`window_words`).
fn window_arguments(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, window: RegisterWindow) -> [ir::Value; 2] {
    [window_words(frame, stage, window), immediate(stage, i64::from(window.count))]
}

/// An allocation row by the lane's own allocator of it (`LaneHelper::NewObjectLiteral`,
/// `NewClosure`, `NewTemplate`, `NewConcat`): each leaves for a due collection first, and a
/// DELETED answer leaves at the proof's miss edge.
pub(super) fn allocate_value(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    allocation: Allocation, proof: EmitProof) -> Option<ir::Value> {
    let (helper, arguments) = match allocation {
        Allocation::ObjectLiteral { site, window } => {
            let [parts, count] = window_arguments(frame, stage, window);
            (LaneHelper::NewObjectLiteral, vec![immediate(stage, i64::from(site)), parts, count])
        }
        Allocation::Closure { function } => (LaneHelper::NewClosure, vec![immediate(stage, i64::from(function))]),
        Allocation::Template { template } => (LaneHelper::NewTemplate, vec![immediate(stage, i64::from(template))]),
        Allocation::Concat { window } => {
            (LaneHelper::NewConcat, window_arguments(frame, stage, window).to_vec())
        }
    };
    frame.allocate(stage, helper, destination, &arguments, proof)
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
fn guard_word(stage: &mut Emit<'_, '_>, word: ir::Value, cc: IntCC, value: Value, miss: ir::Block) {
    let held = stage.builder.ins().icmp_imm_u(cc, word, value.to_bits() as i64);
    emit_js_guard(stage.builder, held, miss);
}

/// The lane's own global-name read: a DELETED answer leaves at the miss edge, otherwise the
/// value is the destination's tagged word.
fn global_read(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, destination: Register,
    atom: u32, site: u16, proof: EmitProof) -> Option<()> {
    let EmitProof::GlobalRead { miss: Some(miss) } = proof else { return None };
    let value = global_word(frame, stage, atom, site);
    guard_word(stage, value, IntCC::NotEqual, Value::DELETED, miss);
    <EmitFrame<'_> as crate::staged::Frame<Emit<'_, '_>>>::write(frame, stage, destination, value,
        RegisterFact::definition(Type::ANY, Representation::Tagged));
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
    let bits = immediate(stage, value.to_bits() as i64);
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
    guard_word(stage, actual, IntCC::Equal, value, miss);
    global_held(frame, stage, destination, value, ty)
}

impl crate::staged::GlobalFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn global_constant(&mut self, stage: &mut Emit<'_, '_>, destination: Register, value: Value) -> Option<()> { global_constant(self, stage, destination, value) }
    fn global_held(&mut self, stage: &mut Emit<'_, '_>, destination: Register, value: Value, ty: Type) -> Option<()> { global_held(self, stage, destination, value, ty) }
    fn global_frozen(&mut self, stage: &mut Emit<'_, '_>, destination: Register, atom: u32, site: u16, value: Value, ty: Type, proof: EmitProof) -> Option<()> {
        global_frozen(self, stage, destination, (atom, site), (value, ty), proof)
    }
    fn global_read(&mut self, stage: &mut Emit<'_, '_>, destination: Register, atom: u32, site: u16, proof: EmitProof) -> Option<()> { global_read(self, stage, destination, atom, site, proof) }
}
