//! Wasm GC operations the lane makes in place with the interpreter's own step for them
//! (`Vm::wasm_gc_value_step`), rather than leaving for the general loop at each.
use super::*;
use quench_runtime::vm::dispatch_wasm::{wasm_gc_allocates, wasm_gc_field_op, wasm_gc_reads_instance};
use quench_runtime::vm::wasm_gc::GcFieldIndex;
use quench_runtime::vm::wasm_gc::stage::HeapStage;
use quench_runtime::staged::wasm_gc::{Aggregate, GcStep, array_copy, array_fill, array_length, field_read, field_write, non_null};
use quench_runtime::wasm::gc::{GcField, GcFieldAccess, static_offset};
use quench_runtime::staged::wasm_call::Call;

/// The running program's signature pool, cached on the lane for the program it was found for
/// (`LaneContext::signatures`). `None` where the program has none; the general path raises that.
#[inline(always)]
fn lane_signatures(cx: *mut LaneContext) -> Option<*const quench_runtime::wasm::WasmSignatures> {
    // SAFETY: the context outlives the lane run; the cache's pool is kept by the context until
    // the program changes, and no step in a lane target's check changes the program.
    unsafe {
        let program = (*cx).program;
        if let (cached, Some(pool)) = &(*cx).signatures {
            if *cached == program {
                return Some(std::rc::Rc::as_ptr(pool));
            }
        }
        let pool = vm(cx).programs.wasm_signatures_shared(program)?;
        let pointer = std::rc::Rc::as_ptr(&pool);
        (*cx).signatures = (program, Some(pool));
        Some(pointer)
    }
}

/// The lane's indirect target step (`Vm::wasm_indirect_row_with`, the interpreter's own check):
/// the reference it writes to its result register, and the row it matched, which the reference
/// call right after the target reads its callee from (`IndirectSlot`).
#[inline(always)]
fn lane_indirect_target(cx: *mut LaneContext, vm: &Vm, i: WideInstruction, ip: Ip, r: Registers) -> Option<Value> {
    // SAFETY: `lane_signatures` returned a pool the context holds for this step.
    let pool = unsafe { lane_signatures(cx)?.as_ref() };
    let (value, row) = vm.wasm_indirect_row_with(pool, r.get(i.register_b()), r.get(i.register_c()), i.imm()).ok()?;
    // SAFETY: the context outlives the lane run.
    unsafe {
        (*cx).indirect = IndirectSlot {
            // Lowering invariant: IndirectTarget is emitted only on the call_indirect path, and
            // `emit_wasm_call` then emits exactly one load_constant (the void receiver, in a register
            // above the target, so it never clobbers register a) followed by the call_through. So the
            // call is two records on. A mismatch (e.g. a fused record) is a safe miss: the reference
            // call's record, register and reference checks decide.
            next: following(following(ip)),
            register: i.result_register(),
            reference: value.to_bits(),
            entry: row.entry,
            instance: row.instance,
        };
    }
    Some(value)
}

/// The `wasm_gc_value_op` numbered `OP`.
const fn gc_op<const OP: u16>() -> Op {
    Op::from_index(OP as usize).expect("a Wasm GC opcode")
}

/// A struct or array field access of the GC step `step`: its operands decoded from `i`'s registers
/// (`Vm::wasm_field_operands`), then the staged `field_write` or `field_read` over the heap. `Some`
/// of a read's value (`None` for a write); `None` for a miss or trap, which takes the general path.
#[inline(always)]
fn lane_gc_field(
    vm: &mut Vm,
    step: GcStep,
    i: WideInstruction,
    read: impl Fn(&Vm, u16) -> Value,
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
            let bits = if operands.raw { value.to_bits() } else { stage.vm.wasm_stored_bits(operands.field, value) };
            field_write(&mut stage, aggregate, reference, index, operands.field, bits, operands.offset).ok().map(|()| None)
        }
        None => {
            let word = field_read(&mut stage, aggregate, reference, index, operands.field, operands.access, operands.offset).ok()?;
            Some(Some(if operands.raw { Value::from_bits(word) } else { stage.vm.wasm_field_value(operands.field, operands.access, word) }))
        }
    }
}

/// `array.fill` and `array.copy` (`GcStep::ArrayFill`, `ArrayCopy`) as the staged rules run them
/// over the heap, their operands decoded by `Vm::wasm_array_bulk_operands`.
#[inline(always)]
fn lane_gc_array_bulk(
    vm: &mut Vm,
    step: GcStep,
    i: WideInstruction,
    read: impl Fn(&Vm, u16) -> Value,
) -> Result<Option<Value>, quench_runtime::vm::JsError> {
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
fn lane_gc_rule(
    vm: &mut Vm,
    step: GcStep,
    i: WideInstruction,
    read: impl Fn(&Vm, u16) -> Value + Copy,
) -> Option<Result<Option<Value>, quench_runtime::vm::JsError>> {
    match step {
        GcStep::AsNonNull => {
            let check = quench_runtime::wasm::reference::NonNullCheck::from_tag(i.imm()).expect("validated non-null check");
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
        let indirect = const { matches!(Call::of(gc_op::<OP>()), Some(Call::IndirectTarget)) };
        let value = match rule {
            Some(Ok(value)) => value,
            Some(Err(_)) => return exit(ip),
            None if indirect => match lane_indirect_target(cx, vm, instruction, ip, r) {
                Some(value) => Some(value),
                None => return exit(ip),
            },
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
    /// or, where `ARRAY`, an array's element. Where `RAW` (a `*Raw` form, an i64 or f64 field in a
    /// raw web), the field's bits go to the register as they are: no box, no tag. A trap takes the
    /// general path, which raises it.
    #[inline(never)]
    fn lane_wasm_field_read<ARRAY: bool, RAW: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let field = GcField::from_tag(i.imm()).expect("validated field operand");
        let (aggregate, index) = if ARRAY {
            let Some(index) = r.get(i.c).as_int() else { return exit(ip) };
            (Aggregate::Array, index as u32)
        } else {
            (Aggregate::Struct, field.index())
        };
        let reference = r.get(i.b).to_bits();
        let mut stage = HeapStage { vm };
        let offset = if ARRAY { None } else { static_offset(i.c) };
        let Ok(word) = field_read(&mut stage, aggregate, reference, u64::from(index), field, GcFieldAccess::Read, offset) else {
            return exit(ip);
        };
        r.set(i.a, if RAW { Value::from_bits(word) } else { stage.vm.wasm_field_value(field, GcFieldAccess::Read, word) });
        next!(cx, r, following(ip), view)
    }
}

/// The handler of a `wasm_gc_value_op` (`lane_wasm_gc_step`), for `op` if it is one.
pub(super) const fn wasm_gc_handler(op: Op) -> Option<Handler> {
    let raw = op.traits().has(quench_runtime::bytecode::Traits::RAW_FORM);
    match (quench_runtime::staged::wasm_gc::GcStep::of(op), raw) {
        (Some(quench_runtime::staged::wasm_gc::GcStep::StructGet { signed: None }), false) => {
            return Some(lane_wasm_field_read::<false, false>);
        }
        (Some(quench_runtime::staged::wasm_gc::GcStep::StructGet { signed: None }), true) => {
            return Some(lane_wasm_field_read::<false, true>);
        }
        (Some(quench_runtime::staged::wasm_gc::GcStep::ArrayGet { signed: None }), false) => {
            return Some(lane_wasm_field_read::<true, false>);
        }
        (Some(quench_runtime::staged::wasm_gc::GcStep::ArrayGet { signed: None }), true) => {
            return Some(lane_wasm_field_read::<true, true>);
        }
        _ => {}
    }
    macro_rules! handlers {
        ($($name:ident)*) => { match op { $(Op::$name => Some(lane_wasm_gc_step::<{ Op::$name as u16 }>),)* _ => None } };
    }
    quench_runtime::wasm_gc_value_ops!(handlers)
}
