//! The direct runs of the packed step helper (`vm_steps::lane_body_step_packed`). The helper decodes the
//! instruction's staged operation (`Operation::direct_helper`) and runs its direct arm, which reads the running
//! frame and declines, before any effect, where the general step decides. A declined row runs the general step
//! (`lane_body_step`) from its PC; a completed one skips the frame bookkeeping and base refreshes, which only
//! guest code or an allocation need. The arm is the one `Vm::step` runs (`write_binary_result`), so both paths
//! share their semantics. The planner admits a row only where its site's feedback makes a decline rare
//! (`function::feedback_admits`).

use super::*;
use super::super::compiled::LANE_CALL_COMPLETED;
use super::super::field_cache::FieldCacheProbe;
use super::operand_feedback::{OperandClass, OperandKinds};
use crate::bytecode::{Atom, FieldBase, FieldLookup, Operand};
use crate::staged::{DecodeContext, Operation};

/// The staged operation the packed helper runs directly, decoded from `instruction`, where it has a direct arm.
pub(in crate::vm) fn direct_operation(instruction: WideInstruction) -> Option<Operation<(), ()>> {
    Operation::decode(instruction, DecodeContext::EMPTY).filter(|operation| operation.direct_helper())
}

/// Whether the packed helper runs `instruction` directly, so the planner packs its step (`steps::packed_words`).
pub(in crate::vm) fn covers(instruction: WideInstruction) -> bool {
    direct_operation(instruction).is_some()
}

/// Whether the site's feedback admits the direct run of the packed instruction `words` (`Step::Helper` rows
/// carry them): a binary operator whose operands are not of an unseen kind, an element access that met a dense
/// or typed element, a field access whose cache site is filled (`fields`, `Known::fields`), and the pure unary
/// queries. Other rows decline the direct run.
pub(in crate::vm) fn admits(words: [u64; 2], site: Option<OperandKinds>, fields: Option<&dyn Fn(u16) -> bool>) -> bool {
    let Ok(imm) = u32::try_from(words[1]) else {
        return false;
    };
    let Some(instruction) = WideInstruction::from_words(words[0], imm) else {
        return false;
    };
    let Some(operation) = direct_operation(instruction) else {
        return false;
    };
    match operation {
        Operation::Binary { .. } => site.is_some_and(|site| site.class() != OperandClass::Any),
        Operation::ElementRead { .. } | Operation::ElementWrite { .. } => site.is_some_and(OperandKinds::element_admitted),
        Operation::GetField { lookup: FieldLookup::Atom { cache_site, .. }, .. } => fields.is_some_and(|filled| filled(cache_site)),
        Operation::SetField { .. } => fields.is_some_and(|filled| filled(instruction.cache_site_index())),
        Operation::LogicalNot { .. } | Operation::TypeofIndex { .. } => true,
        _ => false,
    }
}

/// The direct run of the packed instruction `first` and `imm` (`WideInstruction::from_words`) in the running
/// frame: `Some(status)` where the arm completed it, `None` where the general step must run, which is every
/// case the frame form (activations, a materialized leaf), a returning record, the `this` binding, or the arm's
/// own decline takes.
///
/// # Safety
/// `cx` is the running lane's context.
pub(in crate::vm) unsafe fn direct(cx: *mut LaneContext, first: u64, imm: u64) -> Option<u64> {
    let raw = WideInstruction::from_words(first, u32::try_from(imm).ok()?)?;
    if raw.returns_from_frame() {
        return None;
    }
    let instruction = raw.without_return();
    let operation = direct_operation(instruction)?;
    // SAFETY: the running lane's context; the VM, frame and program are the ones `lane_in_place` dispatches.
    let (vm, frame, program) = unsafe { (&mut *(*cx).vm, (*cx).frame(), &*(*cx).code) };
    if !vm.lane_stack.activations().is_empty() || !unsafe { (*cx).leaf.call }.is_null() {
        return None;
    }
    if instruction.op().binds_this() || instruction.writes_current_this() {
        return None;
    }
    let done = match operation {
        Operation::Binary { left, right, operator, .. } => binary(vm, frame, instruction, left, right, operator),
        Operation::ElementRead { base, key, .. } => element_read(vm, frame, instruction, base, key),
        Operation::ElementWrite { value, base, key, .. } => element_write(vm, frame, value, base, key),
        Operation::GetField { lookup: FieldLookup::Atom { atom, base, cache_site }, .. } =>
            get_field(vm, program, frame, instruction, (atom, base, cache_site)),
        Operation::SetField { value, base: Some(base), .. } => set_field(vm, program, frame, instruction, value, base),
        Operation::LogicalNot { destination, source } => query(vm, frame, (destination, source), Vm::logical_not),
        Operation::TypeofIndex { destination, source } => query(vm, frame, (destination, source), Vm::typeof_index_value),
        _ => false,
    };
    done.then_some(LANE_CALL_COMPLETED)
}

/// A binary operator on two operands `Vm::number_binary` answers (Numbers, no user code), written by
/// to its numeric local target or result register.
fn binary(vm: &mut Vm, frame: usize, instruction: WideInstruction, left: Operand, right: Operand, operator: u32) -> bool {
    let (Some(left), Some(right)) = (left.register_index(), right.register_index()) else {
        return false;
    };
    let (left, right) = (vm.read(frame, left), vm.read(frame, right));
    let Some(value) = vm.number_binary(operator, left, right) else {
        return false;
    };
    // A record naming a numeric local target writes the local, else its result register.
    match instruction.numeric_local_target() {
        Some(local) => vm.frames[frame].locals[local as usize] = value,
        None => vm.write(frame, instruction.result_register(), value),
    }
    true
}

/// An element read of a present dense element or a typed array's number (`element_read_kind`), written to the
/// result register as the interpreter's `step_index_get` writes it. A key that is not a register declines.
fn element_read(vm: &mut Vm, frame: usize, instruction: WideInstruction, base: Register, key: Operand) -> bool {
    let Some(key) = key.register_index() else {
        return false;
    };
    let (object, key) = (vm.read(frame, base), vm.read(frame, key));
    let Some((value, _)) = vm.element_read_kind(object, key) else {
        return false;
    };
    vm.write(frame, instruction.result_register(), value);
    true
}

/// A store to a present dense element of a plain array (`store_present_element`), the interpreter's first
/// `step_index_set` arm; any other store declines to the general path.
fn element_write(vm: &mut Vm, frame: usize, value: Register, base: Register, key: Register) -> bool {
    let (object, key, value) = (vm.read(frame, base), vm.read(frame, key), vm.read(frame, value));
    let Some(index) = key.as_int().and_then(|index| usize::try_from(index).ok()) else {
        return false;
    };
    vm.store_present_element(object, index, value)
}

/// A named read of an object's own or prototype-held data field the cache site holds (`cached_field_hit`), the
/// interpreter's `GetField` Atom arm without its getter, private-name and frame cases, which decline. A miss
/// declines before the general read, which then probes the site again.
fn get_field(vm: &mut Vm, p: &ResidualProgram, frame: usize, instruction: WideInstruction,
    (atom, base, cache_site): (Atom, FieldBase, u16)) -> bool {
    if base.local_index().is_some() || vm.is_private_name(atom) {
        return false;
    }
    let Some(register) = base.register_index() else {
        return false;
    };
    let object = vm.read(frame, register);
    if vm.cached_field_getter(object, atom, vm.field_cache_index(cache_site)).is_some() {
        return false;
    }
    vm.prepare_named_function_info_lookup(object, atom);
    let FieldCacheProbe::Hit(value) = vm.cached_field_hit(p, object, cache_site) else {
        return false;
    };
    vm.write(frame, instruction.result_register(), value);
    true
}

/// A store to an object's own data field the site's entry holds (`cached_field_store_hit`), the interpreter's
/// first `set_field_cached` arm; any other store declines to the general path.
fn set_field(vm: &mut Vm, p: &ResidualProgram, frame: usize, instruction: WideInstruction,
    value: Register, base: Register) -> bool {
    let (object, value) = (vm.read(frame, base), vm.read(frame, value));
    vm.cached_field_store_hit(p, object, instruction.atom_index(), value, instruction.cache_site_index())
}

/// A pure unary query of a register (`logical_not`, `typeof_index_value`), written to its destination.
fn query(vm: &mut Vm, frame: usize, (destination, source): (Register, Register), answer: fn(&Vm, Value) -> Value) -> bool {
    let value = answer(vm, vm.read(frame, source));
    vm.write(frame, destination, value);
    true
}
