//! A native call into another instance: the call context takes the target's record
//! (`instance_context::SWITCHED`), the body of the target's code table runs, and the context's
//! own words return, exactly as in a call within one instance but for the context. The target's
//! environment is held in the VM's native roots for the call (`NativeRoots`), as no frame or
//! table need keep it alive while it runs; the call is made natively only where the target's
//! bodies take the arguments the caller passes (`InstanceContext::abi`) and the roots have room.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::instance_context::{InstanceContext, NativeRoots, SWITCHED};
use super::{CONTEXT_NATIVE_ROOTS, CallContext, Emitter, Vm, invariant, refresh_memory_length};
use quench_runtime::staged::machine::{MachineAccess, MachineFlow, MachineStage};
use quench_runtime::staged::wasm::Width;
use quench_runtime::wasm::integer::I64BinaryOperator as Binary;
use cranelift_codegen::ir::{self, MemFlagsData, types};

/// The width of a switched field's machine type.
fn field_width(ty: ir::Type) -> Width {
    if ty == types::I32 { Width::I32 } else { Width::I64 }
}

impl Emitter<'_, '_> {
    /// The call of the function at index `entry` of the instance at `record`, a row's other
    /// instance: its status jumps to `done`; an instance this artifact cannot call across, or one
    /// whose code table has no body for the function, leaves for `slow`.
    pub(crate) fn foreign_call(
        &mut self,
        record: ir::Value,
        entry: ir::Value,
        signature: &quench_runtime::WasmSignature,
        buffers: (ir::Value, ir::Value),
        (slow, done): (ir::Block, ir::Block),
    ) {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let abi = self.constant(Width::I64, self.memory.word() as i64);
        let published = self.load_word(Width::I64, flags, record, InstanceContext::ABI);
        let same_abi = self.operate(Binary::Equal, Width::I64, published, abi);
        let roots = self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        let depth = self.load_word(Width::I64, flags, roots, NativeRoots::DEPTH);
        let capacity = self.load_word(Width::I64, flags, roots, NativeRoots::CAPACITY);
        let room = self.operate(Binary::LessUnsigned, Width::I64, depth, capacity);
        let usable = self.operate(Binary::And, Width::I32, same_abi, room);
        let lookup = self.body.core.builder.create_block();
        self.branch_if(usable, (lookup, &[]), (slow, &[]));
        self.enter(lookup);
        let table = self.load_word(Width::I64, flags, record, InstanceContext::TABLE);
        let body = self.entry_body(table, entry, slow);
        let status = self.call_in_instance(record, body, signature, buffers);
        self.goto(done, &[status]);
    }

    /// The call of `body` with the context switched to the instance at `record` and restored
    /// after it: the callee's memory is the record's, and its instance stays rooted throughout.
    fn call_in_instance(
        &mut self,
        record: ir::Value,
        body: ir::Value,
        signature: &quench_runtime::WasmSignature,
        buffers: (ir::Value, ir::Value),
    ) -> ir::Value {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let mut saved = Vec::with_capacity(SWITCHED.len());
        for (at, _, ty) in SWITCHED {
            saved.push(self.load_word(field_width(*ty), flags, context, *at));
        }
        for (at, from, ty) in SWITCHED {
            let word = self.load_word(field_width(*ty), flags, record, *from);
            self.store_word(flags, word, context, *at);
        }
        let base = self.load_word(Width::I64, flags, record, InstanceContext::MEMORY_BASE);
        // The record's length is the one its code was last published with: the memory may have
        // grown since (TOD-1790), so the length is read from the memory itself on the way in,
        // and again on the way out, as the callee may have grown a memory the caller shares.
        let leaf = switched_memory_length as *const () as usize;
        let length = self.call_leaf(leaf, &[context]);
        let depth = self.push_root(record);
        let length = self.memory.length.then_some(length);
        let status = self.call_body(body, signature, buffers, (base, length));
        self.pop_root(depth);
        for ((at, _, _), word) in SWITCHED.iter().zip(saved) {
            self.store_word(flags, word, context, *at);
        }
        self.call_leaf(leaf, &[context]);
        status
    }

    /// Holds the environment of the instance at `record` in the native roots: the depth before.
    fn push_root(&mut self, record: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let roots = self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        let depth = self.load_word(Width::I64, flags, roots, NativeRoots::DEPTH);
        let environment = self.load_word(Width::I64, flags, record, InstanceContext::ENV);
        let slots = self.load_word(Width::I64, flags, roots, NativeRoots::SLOTS);
        let size = self.constant(Width::I64, size_of::<quench_runtime::Value>() as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, depth, size);
        let slot = self.operate(Binary::Add, Width::I64, slots, offset);
        self.store_word(flags, environment, slot, 0);
        let one = self.constant(Width::I64, 1);
        let deeper = self.operate(Binary::Add, Width::I64, depth, one);
        self.store_word(flags, deeper, roots, NativeRoots::DEPTH);
        depth
    }

    /// Releases the roots held since the depth `depth`.
    fn pop_root(&mut self, depth: ir::Value) {
        let context = self.body.core.context;
        let roots = self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        self.store_word(MemFlagsData::trusted(), depth, roots, NativeRoots::DEPTH);
    }
}

/// The length of the memory of the instance the context runs now, read from the memory and
/// written to the context, which every access after it reads (`WasmMemory::memory_length`).
unsafe extern "C" fn switched_memory_length(context: *mut CallContext) -> u64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose VM is suspended in
    // that call.
    let context = unsafe { &mut *context };
    let vm = unsafe { &*context.vm.cast::<Vm>() };
    refresh_memory_length(context, vm);
    context.memory_length
}
