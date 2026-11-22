//! Lane handlers of a Wasm instance's state: its memory bindings, linear memory and globals, and
//! the heap callbacks of its vector rows. A non-memory instance binding and the SIMD operators run
//! as their staged rows (`Decoder::WasmState`, `Decoder::WasmVector`).
use super::*;
use quench_runtime::staged::wasm_instance::{Storage, binding_get, global_get, global_set};
use quench_runtime::staged::wasm_machine::ConcreteInstance;
use quench_runtime::staged::wasm_memory_bytes::{read_memory, write_memory};
use quench_runtime::staged::wasm_simd_rules::{shuffle_bits, simd_operand};
use quench_runtime::vm::wasm_global::InstanceBindings;

lane_handler! {
    /// Load the memory binding of the function's memory register and install its memory view:
    /// the row answers the value, but the carried view and the lane context's memory are this
    /// handler's to update, so the memory binding keeps its own body.
    pub(crate) fn lane_instance_binding_memory<>(cx, r, ip, i, view) {
        let env = instance_env_in_lane(cx);
        let (known, storage) = unsafe { (*cx).instance_slots };
        let slot = i.imm();
        // SAFETY: `storage` is the dense storage of `known`, found this run, which no
        // collection has run since (`after_nested_collection_in_lane` drops it).
        let cached = if known == env && slot <= u32::from(u16::MAX) {
            unsafe { quench_runtime::heap::EnvironmentSlots::read_storage(storage, slot as u16) }
        } else {
            None
        };
        let Some(value) = cached.or_else(|| lane_instance_slot_miss(cx, env, slot)) else {
            return exit(ip);
        };
        r.set(i.a, value);
        let memory = resolve_memory(cx, value);
        unsafe { (*cx).memory = memory };
        let view = Carried { memory, ..view };
        next!(cx, r, following(ip), view)
    }
}

/// The global bound in `binding` as its register encoding (`Vm::wasm_global_load`, the staged
/// rule), for the instance-state row of the lane context `context` (`WasmGlobals`). Anything but a
/// global binding answers `None`, which leaves the row to its general step.
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn wasm_global_load(context: *mut (), binding: Value) -> Option<Value> {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    vm.wasm_global_load(binding).ok()
}

lane_handler! {
    /// A generic Wasm memory load (`quench_runtime::wasm::memory::MemoryLoad`), as vectors and the
    /// accesses no direct op names take: in place when it reads in bounds; a trap, or an
    /// operand of another representation, takes the general path, which raises it.
    pub(crate) fn lane_wasm_memory_load<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(operator) = quench_runtime::wasm::memory::MemoryLoad::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Ok(address) = vm.wasm_i64_operand(r.get(i.c)) else {
            return exit(ip);
        };
        let loaded = match vm.wasm_memory_bytes(r.get(i.b)) {
            Ok(bytes) => read_memory(&bytes, operator, address as u64),
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
    /// A generic Wasm memory store (`quench_runtime::wasm::memory::MemoryStore`), in place as
    /// `lane_wasm_memory_load` reads: the staged `write_memory` over the memory's bytes. A store
    /// checks its bounds before writing, so one that traps leaves memory as it was for the general
    /// path to raise. The memory's lock is released before the next record runs.
    pub(crate) fn lane_wasm_memory_store<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(operator) = quench_runtime::wasm::memory::MemoryStore::from_tag(i.imm()) else {
            return exit(ip);
        };
        let Ok(address) = vm.wasm_i64_operand(r.get(i.c)) else {
            return exit(ip);
        };
        let Ok(value) = vm.decode_wasm_value(r.get(i.a), operator.value_type()) else {
            return exit(ip);
        };
        let stored = match vm.heap.get(r.get(i.b)) {
            Some(Cell::WasmMemory { bytes, .. }) => write_memory(&mut bytes.lock(), operator, address as u64, value),
            _ => return exit(ip),
        };
        if stored.is_err() {
            return exit(ip);
        }
        next!(cx, r, following(ip), view)
    }
}

/// A SIMD operand of static type `ty`, decoded by the staged `simd_operand` rule: a vector from its
/// bytes, an i32 from its payload, any other scalar as `Vm::decode_wasm_value` reads it. `None` for
/// another representation.
#[inline(always)]
fn lane_simd_operand(vm: &Vm, value: Value, ty: quench_runtime::WasmType) -> Option<quench_runtime::WasmValue> {
    simd_operand(
        ty,
        || vm.heap.v128(value),
        || match ty {
            quench_runtime::WasmType::I32 => value.as_int().map(quench_runtime::WasmValue::I32),
            _ => vm.decode_wasm_value(value, ty).ok(),
        },
    )
}

/// Store `value` to the mutable global bound in `binding` (`Vm::wasm_global_store`, the staged
/// rule), for the lane context `context`; `false` for a binding that is no mutable global, which the
/// row leaves to the general step that raises.
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn wasm_global_store(context: *mut (), binding: Value, value: Value) -> bool {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    vm.wasm_global_store(binding, value).is_ok()
}

/// The value of the running instance's slot `slot` (`WasmInstanceBinding`) for the instance-state
/// row of the lane context `context`: the environment's dense storage this run holds, else the
/// heap's read (`lane_instance_slot_miss`). `None` for a slot that holds no binding.
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn wasm_instance_slot(context: *mut (), slot: u32) -> Option<Value> {
    let cx = context.cast::<LaneContext>();
    let env = instance_env_in_lane(cx);
    let (known, storage) = unsafe { (*cx).instance_slots };
    // SAFETY: `storage` is the dense storage of `known`, found this run, which no collection has run
    // since (`after_nested_collection_in_lane` drops it).
    let cached = if known == env && slot <= u32::from(u16::MAX) {
        unsafe { quench_runtime::heap::EnvironmentSlots::read_storage(storage, slot as u16) }
    } else {
        None
    };
    cached.or_else(|| lane_instance_slot_miss(cx, env, slot))
}

/// The 16 bytes of the vector cell `value` (`Heap::v128`) for the vector rows of the lane context
/// `context`; `None` where `value` is no vector.
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn wasm_vector_read(context: *mut (), value: Value) -> Option<[u8; 16]> {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    vm.heap.v128(value)
}

/// A fresh vector cell holding `bytes` for the vector rows of the lane context `context`
/// (`Heap::alloc_v128`), noting the allocation as the hand handlers did.
///
/// # Safety
/// `context` is the running lane's context.
pub(crate) unsafe fn wasm_vector_alloc(context: *mut (), bytes: [u8; 16]) -> Option<Value> {
    let vm = unsafe { vm(context.cast::<LaneContext>()) };
    let value = vm.heap.alloc_v128(bytes);
    note_allocation(vm);
    Some(value)
}


lane_handler! {
    /// `i8x16.shuffle` of two vectors by the record's constant lane indices, as
    /// `lane_wasm_simd` runs an operator.
    pub(crate) fn lane_wasm_simd_shuffle<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(indices) = vm.heap.v128(i.constant()) else {
            return exit(ip);
        };
        let (Some(left), Some(right)) = (vm.heap.v128(r.get(i.b)), vm.heap.v128(r.get(i.c))) else {
            return exit(ip);
        };
        let (left, right) = (u128::from_le_bytes(left), u128::from_le_bytes(right));
        let shuffled = shuffle_bits(left, right, indices);
        let value = vm.encode_wasm_value(quench_runtime::WasmValue::V128(shuffled));
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), view)
    }
}

lane_handler! {
    /// A global's load: the staged `global_get` over the global's native slot on
    /// `ConcreteInstance`, its word encoded as a register. Anything but a global binding takes
    /// the general path.
    pub(crate) fn lane_global_get<>(cx, r, ip, i, view) {
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
    pub(crate) fn lane_global_set<>(cx, r, ip, i, view) {
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
fn lane_instance_slot_miss(cx: *mut LaneContext, env: Value, slot: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if let Some(quench_runtime::heap::EnvironmentRecord { slots, .. }) = vm.heap.environment(env)
        && let Some(storage) = slots.dense_storage()
    {
        unsafe { (*cx).instance_slots = (env, storage) };
    }
    binding_get(&mut InstanceBindings { vm, env }, slot).map(Value::from_bits)
}
