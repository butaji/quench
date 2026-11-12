//! A native call into another instance: the call context takes the target's record
//! (`instance_context::SWITCHED`), the body of the target's code table runs, and the context's
//! own words return, exactly as in a call within one instance but for the context. The target's
//! environment is held in the VM's native roots for the call (`NativeRoots`), as no frame or
//! table need keep it alive while it runs; the call is made natively only where the target's
//! bodies take the arguments the caller passes (`InstanceContext::abi`) and the roots have room.

use super::instance_context::{InstanceContext, NATIVE_ROOT_CAPACITY, NativeRoots, SWITCHED};
use super::{CONTEXT_NATIVE_ROOTS, Emitter, Host, invariant};
use cranelift_codegen::ir::{self, BlockArg, InstBuilder, MemFlagsData, condcodes::IntCC, types};

impl<H: Host> Emitter<'_, '_, H> {
    /// The call of the function at index `entry` of the instance at `record`, a row's other
    /// instance: its status jumps to `done`; an instance this artifact cannot call across, or one
    /// whose code table has no body for the function, leaves for `slow`.
    pub(super) fn foreign_call(
        &mut self,
        record: ir::Value,
        entry: ir::Value,
        signature: &crate::WasmSignature,
        buffers: (ir::Value, ir::Value),
        (slow, done): (ir::Block, ir::Block),
    ) {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let abi = self.memory.word() as i64;
        let builder = &mut self.body.core.builder;
        let published = builder.ins().load(types::I64, flags, record, InstanceContext::ABI);
        let same_abi = builder.ins().icmp_imm_u(IntCC::Equal, published, abi);
        let roots = builder.ins().load(types::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        let depth = builder.ins().load(types::I64, flags, roots, NativeRoots::DEPTH);
        let room = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, depth, NATIVE_ROOT_CAPACITY as i64);
        let usable = builder.ins().band(same_abi, room);
        let lookup = builder.create_block();
        builder.ins().brif(usable, lookup, &[], slow, &[]);
        builder.switch_to_block(lookup);
        builder.seal_block(lookup);
        let table = builder.ins().load(types::I64, flags, record, InstanceContext::TABLE);
        let body = self.entry_body(table, entry, slow);
        let status = self.call_in_instance(record, body, signature, buffers);
        self.body.core.builder.ins().jump(done, &[BlockArg::Value(status)]);
    }

    /// The call of `body` with the context switched to the instance at `record` and restored
    /// after it: the callee's memory is the record's, and its instance stays rooted throughout.
    fn call_in_instance(
        &mut self,
        record: ir::Value,
        body: ir::Value,
        signature: &crate::WasmSignature,
        buffers: (ir::Value, ir::Value),
    ) -> ir::Value {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let builder = &mut self.body.core.builder;
        let saved: Vec<ir::Value> = SWITCHED
            .iter()
            .map(|(at, _, ty)| builder.ins().load(*ty, flags, context, *at))
            .collect();
        for (at, from, ty) in SWITCHED {
            let word = builder.ins().load(*ty, flags, record, *from);
            builder.ins().store(flags, word, context, *at);
        }
        let base = builder.ins().load(types::I64, flags, record, InstanceContext::MEMORY_BASE);
        let length = builder.ins().load(types::I64, flags, record, InstanceContext::MEMORY_LENGTH);
        let depth = self.push_root(record);
        let length = self.memory.length.then_some(length);
        let status = self.call_body(body, signature, buffers, (base, length));
        self.pop_root(depth);
        let builder = &mut self.body.core.builder;
        for ((at, _, _), word) in SWITCHED.iter().zip(saved) {
            builder.ins().store(flags, word, context, *at);
        }
        status
    }

    /// Holds the environment of the instance at `record` in the native roots: the depth before.
    fn push_root(&mut self, record: ir::Value) -> ir::Value {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let builder = &mut self.body.core.builder;
        let roots = builder.ins().load(types::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        let depth = builder.ins().load(types::I64, flags, roots, NativeRoots::DEPTH);
        let environment = builder.ins().load(types::I64, flags, record, InstanceContext::ENV);
        let offset = builder.ins().imul_imm_u(depth, size_of::<crate::Value>() as i64);
        let slot = builder.ins().iadd(roots, offset);
        builder.ins().store(flags, environment, slot, NativeRoots::SLOTS);
        let deeper = builder.ins().iadd_imm_s(depth, 1);
        builder.ins().store(flags, deeper, roots, NativeRoots::DEPTH);
        depth
    }

    /// Releases the roots held since the depth `depth`.
    fn pop_root(&mut self, depth: ir::Value) {
        let context = self.body.core.context;
        let builder = &mut self.body.core.builder;
        let roots = builder.ins().load(types::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        builder.ins().store(MemFlagsData::trusted(), depth, roots, NativeRoots::DEPTH);
    }
}
