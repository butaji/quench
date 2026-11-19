//! Lane handlers of a Wasm instance's state: its bindings, linear memory, SIMD values and
//! globals.
use super::*;
use crate::staged::wasm_memory::scalar_address;
use crate::staged::machine::{Machine, MachineStage};
use crate::staged::wasm_instance::{WasmBindings, binding_get};
use crate::staged::wasm_simd::WasmSimd as _;
use crate::staged::wasm_simd_concrete::ConcreteSimd;
use crate::staged::ConcreteWasm;
use crate::vm::JsError;

/// The environment of a running instance, as the interpreter's stage for its binding rule: the
/// fields the heap holds in `env` (`Vm::wasm_binding`).
struct InstanceBindings<'a, H: Host> {
    vm: &'a Vm<H>,
    env: Value,
}

impl<H: Host> Machine for InstanceBindings<'_, H> {
    type Word = u64;
}

impl<H: Host> MachineStage for InstanceBindings<'_, H> {
    type Exit = JsError;

    crate::staged::wasm_concrete::scalar_operators! {}

    fn trap_when(&mut self, flag: u64, trap: crate::WasmTrap) -> Result<(), JsError> {
        if flag == 0 { Ok(()) } else { Err(JsError::wasm_trap_error(trap)) }
    }
}

impl<H: Host> WasmBindings for InstanceBindings<'_, H> {
    fn binding_word(&mut self, slot: u32) -> Option<u64> {
        self.vm.wasm_binding(self.env, slot as usize).map(|value| value.to_bits())
    }
}

lane_handler! {
    /// Load an instance binding; loading the function's memory register
    /// resolves the memory view.
    pub(super) fn lane_instance_binding<MEMORY: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let env = lane_instance_env(cx);
        let (known, storage) = unsafe { (*cx).instance_slots };
        let slot = i.imm();
        // SAFETY: `storage` is the dense storage of `known`, found this run, which no
        // collection has run since (`lane_after_nested_collection` drops it).
        let cached = if known == env && slot <= u32::from(u16::MAX) {
            unsafe { crate::heap::EnvironmentSlots::read_storage(storage, slot as u16) }
        } else {
            None
        };
        let Some(value) = cached.or_else(|| lane_instance_slot_miss(cx, env, slot)) else {
            return exit(ip);
        };
        r.set(i.a, value);
        let view = if MEMORY {
            let memory = resolve_memory(cx, value);
            unsafe { (*cx).memory = memory };
            Carried { memory, ..view }
        } else {
            view
        };
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A Wasm effective address: the index operand in the memory's index type plus the
    /// record's constant offset, as an i64. An overflow, which traps, or an operand of another
    /// representation takes the general path.
    pub(super) fn lane_wasm_memory_address<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (Ok(address), Some(offset)) = (
            vm.wasm_memory_index(r.get(i.c), r.get(i.b)),
            vm.decode_bits64(i.constant()),
        ) else {
            return exit(ip);
        };
        let Ok(effective) = scalar_address(&mut ConcreteWasm, address, offset) else {
            return exit(ip);
        };
        let value = vm.encode_bits64(effective);
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A generic Wasm memory load (`crate::wasm::memory::MemoryLoad`), as vectors and the
    /// accesses no direct op names take: in place when it reads in bounds; a trap, or an
    /// operand of another representation, takes the general path, which raises it.
    pub(super) fn lane_wasm_memory_load<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(operator) = crate::wasm::memory::MemoryLoad::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Ok(address) = vm.wasm_i64_operand(r.get(i.c)) else {
            return exit(ip);
        };
        let loaded = match vm.wasm_memory_bytes(r.get(i.b)) {
            Ok(bytes) => operator.read(&bytes, address as u64),
            Err(_) => return exit(ip),
        };
        let Ok(value) = loaded else {
            return exit(ip);
        };
        let value = vm.encode_wasm_value(value);
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A generic Wasm memory store (`crate::wasm::memory::MemoryStore`), in place as
    /// `lane_wasm_memory_load` reads; a store checks its bounds before writing, so one that
    /// traps leaves memory as it was for the general path to raise.
    pub(super) fn lane_wasm_memory_store<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(operator) = crate::wasm::memory::MemoryStore::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Ok(address) = vm.wasm_i64_operand(r.get(i.c)) else {
            return exit(ip);
        };
        if vm.wasm_memory_store(r.get(i.b), address as u64, operator, r.get(i.a)).is_err() {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

/// A SIMD operand of static type `ty`: a vector or an i32 read from its slot directly, any
/// other scalar as `Vm::decode_wasm_value` reads it. `None` for another representation.
#[inline(always)]
fn lane_simd_operand<H: Host>(vm: &Vm<H>, value: Value, ty: crate::WasmType) -> Option<crate::WasmValue> {
    match ty {
        crate::WasmType::V128 => Some(crate::WasmValue::V128(u128::from_le_bytes(vm.heap.v128(value)?))),
        crate::WasmType::I32 => Some(crate::WasmValue::I32(value.as_int()?)),
        _ => vm.decode_wasm_value(value, ty).ok(),
    }
}

lane_handler! {
    /// A Wasm SIMD operator (`crate::wasm::simd::SimdOperator`), which neither traps nor runs
    /// guest code: its operands decoded by their static types and its result encoded, a
    /// vector in a fresh cell (`Vm::encode_wasm_value`). An operand of another
    /// representation takes the general path, which raises its validation error.
    pub(super) fn lane_wasm_simd<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some((kernel, _)) = crate::wasm::simd::SimdKernel::of(i.imm()) else {
            return exit(ip);
        };
        let Some(left) = lane_simd_operand(vm, r.get(i.b), kernel.left) else {
            return exit(ip);
        };
        let right = match kernel.right {
            None => None,
            Some(ty) => match lane_simd_operand(vm, r.get(i.c), ty) {
                Some(right) => Some(right),
                None => return exit(ip),
            },
        };
        let value = match crate::staged::wasm_simd_concrete::execute(i.imm(), left, right) {
            crate::WasmValue::V128(bits) => vm.heap.alloc_v128(bits.to_le_bytes()),
            value => vm.encode_wasm_value(value),
        };
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}


lane_handler! {
    /// `i8x16.shuffle` of two vectors by the record's constant lane indices, as
    /// `lane_wasm_simd` runs an operator.
    pub(super) fn lane_wasm_simd_shuffle<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(indices) = vm.heap.v128(i.constant()) else {
            return exit(ip);
        };
        let (Some(left), Some(right)) = (vm.heap.v128(r.get(i.b)), vm.heap.v128(r.get(i.c))) else {
            return exit(ip);
        };
        let (left, right) = (u128::from_le_bytes(left), u128::from_le_bytes(right));
        let shuffled = ConcreteSimd.vector_shuffle(indices, left, right);
        let value = vm.encode_wasm_value(crate::WasmValue::V128(shuffled));
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A global's load (`Vm::wasm_global_load`, the staged rule): the slot's word under its
    /// type's storage. Anything but a global binding takes the general path.
    pub(super) fn lane_global_get<>(cx, r, ip, i, view) {
        let Ok(value) = unsafe { vm(cx) }.wasm_global_load(r.get(i.b)) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A global's store (`Vm::wasm_global_store`, the staged rule); a binding that is no
    /// mutable global takes the general path, which raises.
    pub(super) fn lane_global_set<>(cx, r, ip, i, view) {
        if unsafe { vm(cx) }.wasm_global_store(r.get(i.b), r.get(i.a)).is_err() {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

/// `lane_instance_binding`'s first read of `env` this run (or of a slot its storage does not
/// hold as a plain value): the heap's read, after which the run keeps `env`'s dense storage.
#[cold]
#[inline(never)]
fn lane_instance_slot_miss<H: Host>(cx: *mut LaneContext<H>, env: Value, slot: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if let Some(Cell::Environment { slots, .. }) = vm.heap.get(env)
        && let Some(storage) = slots.dense_storage()
    {
        unsafe { (*cx).instance_slots = (env, storage) };
    }
    binding_get(&mut InstanceBindings { vm, env }, slot).map(Value::from_bits)
}
