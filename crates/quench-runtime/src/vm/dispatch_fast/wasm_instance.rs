//! Lane handlers of a Wasm instance's state: its memory bindings, linear memory and globals, and
//! the heap callbacks of its vector rows. A non-memory instance binding and the SIMD operators run
//! as their staged rows (`Decoder::WasmState`, `Decoder::WasmVector`).
use super::*;
// merge: kept HEAD imports the body still uses (scalar_address, ConcreteWasm); the SIMD imports are dropped, since theirs runs SIMD operators as staged rows and nothing here names them.
use crate::staged::wasm_memory::scalar_address;
use crate::staged::ConcreteWasm;

lane_handler! {
    /// Load the memory binding of the function's memory register and install its memory view:
    /// the row answers the value, but the carried view and the lane context's memory are this
    /// handler's to update, so the memory binding keeps its own body.
    pub(super) fn lane_instance_binding_memory<>(cx, r, ip, i, view) {
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
        let Some(value) = cached.or_else(|| lane_instance_slot_miss(cx, env, slot as usize)) else {
            return exit(ip);
        };
        r.set(i.a, value);
        let memory = resolve_memory(cx, value);
        unsafe { (*cx).memory = memory };
        let view = Carried { memory, ..view };
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

/// The global bound in `binding` as its register encoding (`Vm::wasm_global_load`, the staged
/// rule), for the instance-state row of the lane context `context` (`WasmGlobals`). Anything but a
/// global binding answers `None`, which leaves the row to its general step.
///
/// # Safety
/// `context` is the running lane's context.
pub(super) unsafe fn wasm_global_load<H: Host>(context: *mut (), binding: Value) -> Option<Value> {
    let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
    vm.wasm_global_load(binding).ok()
}

/// Store `value` to the mutable global bound in `binding` (`Vm::wasm_global_store`, the staged
/// rule), for the lane context `context`; `false` for a binding that is no mutable global, which the
/// row leaves to the general step that raises.
///
/// # Safety
/// `context` is the running lane's context.
pub(super) unsafe fn wasm_global_store<H: Host>(context: *mut (), binding: Value, value: Value) -> bool {
    let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
    vm.wasm_global_store(binding, value).is_ok()
}

/// The value of the running instance's slot `slot` (`WasmInstanceBinding`) for the instance-state
/// row of the lane context `context`: the environment's dense storage this run holds, else the
/// heap's read (`lane_instance_slot_miss`). `None` for a slot that holds no binding.
///
/// # Safety
/// `context` is the running lane's context.
pub(super) unsafe fn wasm_instance_slot<H: Host>(context: *mut (), slot: u32) -> Option<Value> {
    let cx = context.cast::<LaneContext<H>>();
    let env = lane_instance_env(cx);
    let (known, storage) = unsafe { (*cx).instance_slots };
    // SAFETY: `storage` is the dense storage of `known`, found this run, which no collection has run
    // since (`lane_after_nested_collection` drops it).
    let cached = if known == env && slot <= u32::from(u16::MAX) {
        unsafe { crate::heap::EnvironmentSlots::read_storage(storage, slot as u16) }
    } else {
        None
    };
    cached.or_else(|| lane_instance_slot_miss(cx, env, slot as usize))
}

/// The 16 bytes of the vector cell `value` (`Heap::v128`) for the vector rows of the lane context
/// `context`; `None` where `value` is no vector.
///
/// # Safety
/// `context` is the running lane's context.
pub(super) unsafe fn wasm_vector_read<H: Host>(context: *mut (), value: Value) -> Option<[u8; 16]> {
    let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
    vm.heap.v128(value)
}

/// A fresh vector cell holding `bytes` for the vector rows of the lane context `context`
/// (`Heap::alloc_v128`), noting the allocation as the hand handlers did.
///
/// # Safety
/// `context` is the running lane's context.
pub(super) unsafe fn wasm_vector_alloc<H: Host>(context: *mut (), bytes: [u8; 16]) -> Option<Value> {
    let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
    let value = vm.heap.alloc_v128(bytes);
    note_allocation(vm);
    Some(value)
}

/// `lane_instance_binding_memory`'s first read of `env` this run (or of a slot its storage does not
/// hold as a plain value): the heap's read, after which the run keeps `env`'s dense storage.
#[cold]
#[inline(never)]
fn lane_instance_slot_miss<H: Host>(cx: *mut LaneContext<H>, env: Value, slot: usize) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if let Some(crate::heap::EnvironmentRecord { slots, .. }) = vm.heap.environment(env)
        && let Some(storage) = slots.dense_storage()
    {
        unsafe { (*cx).instance_slots = (env, storage) };
    }
    vm.wasm_binding(env, slot)
}
