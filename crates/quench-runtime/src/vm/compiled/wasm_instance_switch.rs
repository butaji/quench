//! A native call into another instance: the call context takes the target's record
//! (`instance_context::SWITCHED`), the body of the target's code table runs, and the context's
//! own words return, exactly as in a call within one instance but for the context. The target's
//! environment is held in the VM's native roots for the call (`NativeRoots`), as no frame or
//! table need keep it alive while it runs; the call is made natively only where the target's
//! bodies take the arguments the caller passes (`InstanceContext::abi`) and the roots have room.

use super::instance_context::{InstanceContext, NATIVE_ROOT_CAPACITY, NativeRoots, SWITCHED};
use super::{CONTEXT_NATIVE_ROOTS, Emitter, Host, invariant};
use crate::staged::machine::{MachineAccess, MachineFlow, MachineStage};
use crate::staged::wasm::Width;
use crate::wasm::integer::I64BinaryOperator as Binary;
use cranelift_codegen::ir::{self, MemFlagsData, types};

/// The width of a switched field's machine type.
fn field_width(ty: ir::Type) -> Width {
    if ty == types::I32 { Width::I32 } else { Width::I64 }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The call of the function at index `entry` of the instance at `record`, a row's other
    /// instance: its exit jumps to `done`; an instance this artifact cannot call across, or one
    /// whose code table has no body for the function, leaves for `slow`.
    pub(super) fn foreign_call(
        &mut self,
        record: ir::Value,
        entry: ir::Value,
        signature: &crate::WasmSignature,
        arguments: &[ir::Value],
        (slow, done): (ir::Block, ir::Block),
    ) {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let abi = self.constant(Width::I64, self.memory.word() as i64);
        let published = self.load_word(Width::I64, flags, record, InstanceContext::ABI);
        let same_abi = self.operate(Binary::Equal, Width::I64, published, abi);
        let roots = self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        let depth = self.load_word(Width::I64, flags, roots, NativeRoots::DEPTH);
        let capacity = self.constant(Width::I64, NATIVE_ROOT_CAPACITY as i64);
        let room = self.operate(Binary::LessUnsigned, Width::I64, depth, capacity);
        let usable = self.operate(Binary::And, Width::I32, same_abi, room);
        let lookup = self.body.core.builder.create_block();
        self.branch_if(usable, (lookup, &[]), (slow, &[]));
        self.enter(lookup);
        let table = self.load_word(Width::I64, flags, record, InstanceContext::TABLE);
        let body = self.entry_body(table, entry, slow);
        let (status, result) = self.call_in_instance(record, body, signature, arguments);
        self.goto_exit(done, status, result);
    }

    /// The call of `body` with the context switched to the instance at `record` and restored
    /// after it: the callee's memory is the record's, and its instance stays rooted throughout.
    fn call_in_instance(
        &mut self,
        record: ir::Value,
        body: ir::Value,
        signature: &crate::WasmSignature,
        arguments: &[ir::Value],
    ) -> (ir::Value, Option<ir::Value>) {
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
        let length = self.load_word(Width::I64, flags, record, InstanceContext::MEMORY_LENGTH);
        let depth = self.push_root(record);
        let length = self.memory.length.then_some(length);
        let exit = self.call_body(body, signature, arguments, (base, length));
        self.pop_root(depth);
        for ((at, _, _), word) in SWITCHED.iter().zip(saved) {
            self.store_word(flags, word, context, *at);
        }
        exit
    }

    /// Holds the environment of the instance at `record` in the native roots: the depth before.
    fn push_root(&mut self, record: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let roots = self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        let depth = self.load_word(Width::I64, flags, roots, NativeRoots::DEPTH);
        let environment = self.load_word(Width::I64, flags, record, InstanceContext::ENV);
        let size = self.constant(Width::I64, size_of::<crate::Value>() as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, depth, size);
        let slot = self.operate(Binary::Add, Width::I64, roots, offset);
        self.store_word(flags, environment, slot, NativeRoots::SLOTS);
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
