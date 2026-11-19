//! Lane handlers of a Wasm instance's state: its bindings, linear memory, SIMD values and
//! globals.
use super::*;
use crate::staged::wasm_instance::{Storage, binding_get, global_get, global_set};
use crate::staged::wasm_machine::{ConcreteBytes, ConcreteInstance};
use crate::staged::wasm_memory::{Access, load, scalar_address};
use crate::staged::wasm_memory_vector::{load_lane, load_vector};
use crate::staged::wasm_simd_rules::shuffle_bits;
use crate::staged::ConcreteWasm;
use crate::vm::wasm_global::InstanceBindings;

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

/// `MemoryLoad::read`'s staged rules over the bytes of a memory: `load` for a scalar access,
/// `load_vector` for a v128, `load_lane` for the widening and splat loads. Each checks its access
/// before it reads, so an out-of-bounds read traps and writes nothing.
#[inline(always)]
fn lane_memory_read(memory: &[u8], operator: crate::wasm::memory::MemoryLoad, address: u64) -> Result<crate::WasmValue, crate::WasmTrap> {
    let mut stage = ConcreteBytes { memory };
    match Access::of_load(operator) {
        Some(access) => load(&mut stage, access, address, 0, false).map(|word| crate::WasmValue::from_bits(access.ty, word)),
        None if matches!(operator, crate::wasm::memory::MemoryLoad::V128Load) => {
            load_vector(&mut stage, address, 0, false).map(crate::WasmValue::V128)
        }
        None => load_lane(&mut stage, operator.width(), address, 0, false).map(|word| operator.vector_of_word(word)),
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
            Ok(bytes) => lane_memory_read(&bytes, operator, address as u64),
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
        let shuffled = shuffle_bits(left, right, indices);
        let value = vm.encode_wasm_value(crate::WasmValue::V128(shuffled));
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A global's load: the staged `global_get` over the global's native slot on
    /// `ConcreteInstance`, its word encoded as a register. Anything but a global binding takes
    /// the general path.
    pub(super) fn lane_global_get<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(Cell::WasmGlobal { bits, ty, .. }) = vm.heap.get(r.get(i.b)) else {
            return exit(ip);
        };
        let (mut bits, storage) = (*bits, Storage::of(*ty));
        let mut instance = ConcreteInstance { globals: std::slice::from_mut(&mut bits), memory: &mut [] };
        let word = global_get(&mut instance, 0, storage);
        let value = match storage {
            Storage::Bits32 => Value::integer(word as u32 as i32),
            Storage::Bits64 => vm.encode_bits64(word),
            Storage::Handle => Value::from_bits(word),
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A global's store: the staged `global_set` over the mutable global's native slot. A
    /// binding that is no mutable global, or a value of another representation, takes the
    /// general path, which raises.
    pub(super) fn lane_global_set<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(Cell::WasmGlobal { ty, mutable: true, .. }) = vm.heap.get(r.get(i.b)) else {
            return exit(ip);
        };
        let (storage, value) = (Storage::of(*ty), r.get(i.a));
        let word = match storage {
            Storage::Bits32 => Some(u64::from(value.wasm_bits32())),
            Storage::Bits64 => vm.decode_bits64(value),
            Storage::Handle => Some(value.to_bits()),
        };
        let (Some(word), Some(Cell::WasmGlobal { bits, .. })) = (word, vm.heap.get_mut(r.get(i.b))) else {
            return exit(ip);
        };
        let mut instance = ConcreteInstance { globals: std::slice::from_mut(bits), memory: &mut [] };
        global_set(&mut instance, 0, storage, word);
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
