//! Lane handlers of a Wasm instance's state: its bindings, linear memory, SIMD values and
//! globals.
use super::*;

lane_handler! {
    /// Load an instance binding; loading the function's memory register
    /// resolves the memory view.
    pub(super) fn lane_instance_binding<MEMORY: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let env = vm.frames[unsafe { (*cx).frame }].env;
        let Some(value) = vm.heap.environment_slot(env, i.imm() as usize) else {
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
        let Some(effective) = address.checked_add(offset) else {
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
        let Some((kernel, lane)) = crate::wasm::simd::SimdKernel::of(i.imm()) else {
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
        let value = match (kernel.apply)(left, right, lane) {
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
        let value = vm.encode_wasm_value(crate::WasmValue::V128(crate::wasm::simd::shuffle(left, right, indices)));
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    pub(super) fn lane_global_get<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal { value, .. }) = unsafe { vm(cx) }.heap.get(r.get(i.b)) else {
            return exit(ip);
        };
        r.set(i.a, *value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    pub(super) fn lane_global_set<>(cx, r, ip, i, view) {
        let Some(Cell::WasmGlobal {
            value,
            mutable: true,
            ..
        }) = unsafe { vm(cx) }.heap.get_mut(r.get(i.b))
        else {
            return exit(ip);
        };
        *value = r.get(i.a);
        next!(cx, r, following(ip), view)
    }
}
