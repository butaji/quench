//! Wasm GC operations the lane makes in place with the interpreter's own step for them
//! (`Vm::wasm_gc_value_step`), rather than leaving for the general loop at each.
use super::*;
use super::super::dispatch_wasm::{wasm_gc_allocates, wasm_gc_field_op, wasm_gc_reads_instance};
use super::super::wasm_gc::GcFieldIndex;
use super::super::wasm_gc::stage::HeapStage;
use crate::staged::wasm_gc::{Aggregate, GcStep, array_copy, array_fill, array_length, field_read, field_write, non_null};
use crate::wasm::gc::{GcField, GcFieldAccess};

/// The `wasm_gc_value_op` numbered `OP`.
const fn gc_op<const OP: u16>() -> Op {
    Op::from_index(OP as usize).expect("a Wasm GC opcode")
}

/// A struct or array field access of the GC step `step`: its operands decoded from `i`'s registers
/// (`Vm::wasm_field_operands`), then the staged `field_write` or `field_read` over the heap. `Some`
/// of a read's value (`None` for a write); `None` for a miss or trap, which takes the general path.
#[inline(always)]
fn lane_gc_field<H: Host>(
    vm: &mut Vm<H>,
    step: GcStep,
    i: WideInstruction,
    read: impl Fn(&Vm<H>, u16) -> Value,
) -> Option<Option<Value>> {
    let operands = vm.wasm_field_operands(step, i, read).ok()?;
    let (aggregate, index) = match operands.location {
        GcFieldIndex::Struct(index) => (Aggregate::Struct, index),
        GcFieldIndex::Array(index) => (Aggregate::Array, index),
    };
    let (reference, index) = (operands.reference.to_bits(), u64::from(index));
    let mut stage = HeapStage { vm };
    match operands.value {
        Some(value) => {
            field_write(&mut stage, aggregate, reference, index, operands.field, value.to_bits()).ok().map(|()| None)
        }
        None => {
            let word = field_read(&mut stage, aggregate, reference, index, operands.field, operands.access).ok()?;
            Some(Some(match operands.access {
                GcFieldAccess::Read => Value::from_bits(word),
                _ => Value::integer(word as i32),
            }))
        }
    }
}

/// `array.fill` and `array.copy` (`GcStep::ArrayFill`, `ArrayCopy`) as the staged rules run them
/// over the heap, their operands decoded by `Vm::wasm_array_bulk_operands`.
#[inline(always)]
fn lane_gc_array_bulk<H: Host>(
    vm: &mut Vm<H>,
    step: GcStep,
    i: WideInstruction,
    read: impl Fn(&Vm<H>, u16) -> Value,
) -> Result<Option<Value>, crate::vm::JsError> {
    let copy = step == GcStep::ArrayCopy;
    let ops = vm.wasm_array_bulk_operands(i, read, copy)?;
    if copy {
        array_copy(&mut HeapStage { vm }, ops.destination.to_bits(), ops.output, ops.source.to_bits(), ops.input, ops.count)?;
    } else {
        array_fill(&mut HeapStage { vm }, ops.destination.to_bits(), ops.output, ops.source.to_bits(), ops.count)?;
    }
    Ok(None)
}

/// The non-field GC steps the lane runs as the staged rules over the heap: `ref.as_non_null`,
/// `array.len`, `array.fill` and `array.copy`. `None` for the others, which the interpreter's step
/// makes (`Vm::wasm_gc_value_step`).
#[inline(always)]
fn lane_gc_rule<H: Host>(
    vm: &mut Vm<H>,
    step: GcStep,
    i: WideInstruction,
    read: impl Fn(&Vm<H>, u16) -> Value + Copy,
) -> Option<Result<Option<Value>, crate::vm::JsError>> {
    match step {
        GcStep::AsNonNull => {
            let check = crate::wasm::reference::NonNullCheck::from_tag(i.imm()).expect("validated non-null check");
            let reference = read(vm, i.register_b()).to_bits();
            Some(non_null(&mut HeapStage { vm }, reference, check.trap()).map(|word| Some(Value::from_bits(word))))
        }
        GcStep::ArrayLen => {
            let reference = read(vm, i.register_b()).to_bits();
            Some(array_length(&mut HeapStage { vm }, reference).map(|length| Some(Value::integer(length as i32))))
        }
        GcStep::ArrayFill | GcStep::ArrayCopy => Some(lane_gc_array_bulk(vm, step, i, read)),
        _ => None,
    }
}

lane_handler! {
    /// The `wasm_gc_value_op` numbered `OP` of the running function's registers, made by the
    /// interpreter's step; a trap, which writes nothing, takes the general path to raise it.
    /// One handler per opcode: the record holds the instruction's operands, so the step needs
    /// no decode, and the opcode is a constant its step folds.
    #[inline(never)]
    fn lane_wasm_gc_step<OP: u16>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let op = const { gc_op::<OP>() };
        // SAFETY: the context outlives the lane run.
        let program = unsafe { (*cx).program };
        let env = if const { wasm_gc_reads_instance(gc_op::<OP>()) } { instance_env_in_lane(cx) } else { Value::UNDEFINED };
        let instruction = WideInstruction::new(op, i.a, i.b, i.c, i.imm());
        // A field access allocates nothing and reads no instance state: a miss, which traps,
        // takes the general path, which raises it.
        if const { wasm_gc_field_op(gc_op::<OP>()) } {
            let Some(step) = GcStep::of(op) else { return exit(ip) };
            let Some(value) = lane_gc_field(vm, step, instruction, |_, register| r.get(register)) else {
                return exit(ip);
            };
            if let Some(value) = value {
                r.set(instruction.result_register(), value);
            }
            next!(cx, r, following(ip), view)
        }
        let rule = match GcStep::of(op) {
            Some(step) => lane_gc_rule(vm, step, instruction, |_, register| r.get(register)),
            None => None,
        };
        let value = match rule {
            Some(Ok(value)) => value,
            Some(Err(_)) => return exit(ip),
            None => {
                let Ok(value) = vm.wasm_gc_value_step(program, env, instruction, |_, register| r.get(register)) else {
                    return exit(ip);
                };
                value
            }
        };
        if let Some(value) = value {
            r.set(instruction.result_register(), value);
        }
        // An allocating step can make a collection due.
        if const { wasm_gc_allocates(gc_op::<OP>()) } {
            note_allocation(vm);
        }
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A plain read of a value field (`GcStep::StructGet` or `ArrayGet` with no sign): the interpreter's own access
    /// (`staged::wasm_gc::field_read`) with the record's operands read in place, a struct's field
    /// or, where `ARRAY`, an array's element. A trap takes the general path, which raises it.
    #[inline(never)]
    fn lane_wasm_field_read<ARRAY: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let field = GcField::from_tag(i.imm()).expect("validated field operand");
        let (aggregate, index) = if ARRAY {
            let Some(index) = r.get(i.c).as_int() else { return exit(ip) };
            (Aggregate::Array, index as u32)
        } else {
            (Aggregate::Struct, field.index())
        };
        let reference = r.get(i.b).to_bits();
        let Ok(word) = field_read(&mut HeapStage { vm }, aggregate, reference, u64::from(index), field, GcFieldAccess::Read) else {
            return exit(ip);
        };
        r.set(i.a, Value::from_bits(word));
        next!(cx, r, following(ip), view)
    }
}

/// The handler of a `wasm_gc_value_op` (`lane_wasm_gc_step`), for `op` if it is one.
pub(super) const fn wasm_gc_handler<H: Host>(op: Op) -> Option<Handler> {
    match crate::staged::wasm_gc::GcStep::of(op) {
        Some(crate::staged::wasm_gc::GcStep::StructGet { signed: None }) => {
            return Some(lane_wasm_field_read::<H, false>);
        }
        Some(crate::staged::wasm_gc::GcStep::ArrayGet { signed: None }) => {
            return Some(lane_wasm_field_read::<H, true>);
        }
        _ => {}
    }
    macro_rules! handlers {
        ($($name:ident)*) => { match op { $(Op::$name => Some(lane_wasm_gc_step::<H, { Op::$name as u16 }>),)* _ => None } };
    }
    super::super::dispatch_wasm::wasm_gc_value_ops!(handlers)
}
