//! The Wasm tier's indirect calls: the primitives of `WasmTable` as Cranelift code over the
//! table's mirror, which the VM publishes as words (`CallContext::table_words`), and the call
//! through a row's function. The target's checks are the staged rule
//! (`staged::wasm_call::indirect_target`), emitted: the bound, the null id and the id compare
//! trap in the interpreter's order; an id that is not the expected one is the VM's to relate.
//! A call goes through the row natively where the row names a function this artifact compiled,
//! in this instance or, switching the context to the instance's record
//! (`wasm_instance_switch`), in another; every other target (a host function, code not
//! compiled, a memory the call cannot be made across) goes through the VM's callout.

use super::wasm_stage::infallible;
use super::{
    Analysis, CONTEXT_CALL_REFERENCE, CONTEXT_CALL_REFERENCE_DIAGNOSTIC, CONTEXT_INSTANCE,
    CONTEXT_TABLE, CONTEXT_TABLE_WORDS, CallContext, Emitter, Host, Kind,
    Slot, Vm, internal_signature, invariant,
};
use crate::bytecode::WideInstruction;
use crate::heap::{TableTarget, TableWords};
use crate::staged::machine::MachineStage;
use crate::staged::wasm::Width;
use crate::staged::wasm_call::{WasmTable, indirect_target};
use crate::wasm::CanonicalSignature;
use crate::wasm::integer::I64BinaryOperator as Binary;
use cranelift_codegen::ir::{self, BlockArg, InstBuilder, MemFlagsData, condcodes::IntCC, types};

/// The table an indirect target is being emitted for: the address of its published words, its
/// binding and the type the call expects.
#[derive(Clone, Copy)]
pub(super) struct IndirectSite {
    words: ir::Value,
    table: u32,
    signature_index: u32,
}

/// Whether the element at `index` of the table at binding `table` satisfies the type
/// `expected` (a type index of the program) although its id is not the expected one: a subtype,
/// or a host function whose signature the VM compares. A reference the VM cannot compare does
/// not satisfy it.
unsafe extern "C" fn compiled_table_accepts<H: Host>(
    context: *mut CallContext,
    table: u64,
    index: u64,
    expected: u64,
) -> u64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let context = unsafe { &*context };
    let vm = unsafe { &*context.vm.cast::<Vm<H>>() };
    let binding = vm.heap.environment_slot(context.env, table as usize);
    let reference = binding.and_then(|table| vm.wasm_table_get(table, index).ok());
    let pool = vm.programs.wasm_signatures(context.program);
    let accepted = reference
        .zip(pool)
        .and_then(|(reference, pool)| vm.wasm_callable_matches(reference, pool, expected as u32, false));
    u64::from(accepted.unwrap_or(false))
}

impl<H: Host> Emitter<'_, '_, H> {
    fn site(&self) -> IndirectSite {
        self.indirect.expect("an indirect target is being emitted")
    }

    /// The address of the published words of the table at binding `table`: the instance's table
    /// area holds them, indexed by the binding.
    fn table_words(&mut self, table: u32) -> ir::Value {
        let builder = &mut self.body.core.builder;
        let array = builder.ins().load(types::I64, invariant(), self.body.core.context, CONTEXT_TABLE_WORDS);
        let offset = (table as usize * size_of::<*const TableWords>()) as i32;
        builder.ins().load(types::I64, invariant(), array, offset)
    }

    /// The address of the row of element `index`.
    fn table_row(&mut self, words: ir::Value, index: ir::Value) -> ir::Value {
        let builder = &mut self.body.core.builder;
        let base = builder.ins().load(types::I64, MemFlagsData::trusted(), words, TableWords::BASE);
        let offset = builder.ins().imul_imm_u(index, TableTarget::SIZE);
        builder.ins().iadd(base, offset)
    }

    /// Lowers the target of an indirect call: the staged rule over the table, whose result,
    /// the row of the element, the call that follows reads.
    pub(super) fn indirect_target(&mut self, pc: usize, instruction: WideInstruction) {
        let table = self.binding_slot(pc);
        let signature_index = instruction.imm();
        let expected = self.analysis.type_canonical(signature_index).expect("planned");
        let words = self.table_words(table);
        self.indirect = Some(IndirectSite { words, table, signature_index });
        let index = self.read(instruction.c(), Kind::I32);
        let index = self.body.core.builder.ins().uextend(types::I64, index);
        let row = infallible(indirect_target(self, index, expected));
        self.indirect = None;
        self.write(instruction.a(), Kind::I64, row);
    }
}

impl<H: Host> WasmTable for Emitter<'_, '_, H> {
    fn table_range_exceeds(&mut self, index: ir::Value, extent: ir::Value) -> ir::Value {
        let end = self.operate(Binary::Add, Width::I64, index, extent);
        let words = self.site().words;
        let length = self.body.core.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            words,
            TableWords::LENGTH,
        );
        self.operate(Binary::GreaterUnsigned, Width::I64, end, length)
    }

    fn element_signature(&mut self, index: ir::Value) -> ir::Value {
        let row = self.table_row(self.site().words, index);
        let builder = &mut self.body.core.builder;
        let id = builder.ins().load(types::I32, MemFlagsData::trusted(), row, TableTarget::SIGNATURE);
        builder.ins().uextend(types::I64, id)
    }

    /// The reference of an element, for the call that follows, is its row: the table keeps it.
    fn table_element(&mut self, index: ir::Value) -> ir::Value {
        index
    }

    /// An equal id accepts; any other is the VM's to relate.
    fn signature_accepts(
        &mut self,
        reference: ir::Value,
        signature: ir::Value,
        expected: ir::Value,
    ) -> ir::Value {
        let equal = self.operate(Binary::Equal, Width::I64, signature, expected);
        let site = self.site();
        let (table, expected_type) = (i64::from(site.table), i64::from(site.signature_index));
        let context = self.body.core.context;
        let builder = &mut self.body.core.builder;
        let (relate, join) = (builder.create_block(), builder.create_block());
        builder.append_block_param(join, types::I32);
        builder.ins().brif(equal, join, &[BlockArg::Value(equal)], relate, &[]);
        builder.set_cold_block(relate);
        builder.switch_to_block(relate);
        builder.seal_block(relate);
        let table = builder.ins().iconst(types::I64, table);
        let expected_type = builder.ins().iconst(types::I64, expected_type);
        let target = compiled_table_accepts::<H> as *const () as usize;
        let related = self.call_leaf(target, &[context, table, reference, expected_type]);
        let builder = &mut self.body.core.builder;
        let related = builder.ins().ireduce(types::I32, related);
        builder.ins().jump(join, &[BlockArg::Value(related)]);
        builder.switch_to_block(join);
        builder.seal_block(join);
        builder.block_params(join)[0]
    }
}

impl<H: Host> Analysis<'_, H> {
    /// The canonical id of the type `index` of the program.
    pub(super) fn type_canonical(&self, index: u32) -> Option<CanonicalSignature> {
        self.vm
            .programs
            .wasm_signatures(self.program)?
            .type_canonical(index)
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// Lowers a call of the row an indirect target produced: natively through the function the
    /// row names where it has one, else through the VM's callout.
    pub(super) fn call_reference(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::IndirectReference { signature: signature_index, table } = self.plan.operand(pc)
        else {
            unreachable!("planned indirect calls retain their table binding");
        };
        let signature = self.analysis.type_signature(signature_index).expect("planned").clone();
        let expected = self.analysis.type_canonical(signature_index).expect("planned");
        let (args, out) = self.spill_arguments(instruction, &signature);
        let row = self.read(instruction.b(), Kind::I64);
        let words = self.table_words(table);
        let builder = &mut self.body.core.builder;
        let (slow, done) = (builder.create_block(), builder.create_block());
        builder.append_block_param(done, types::I64);
        self.native_calls(words, row, expected, &signature, (args, out), (slow, done));
        let builder = &mut self.body.core.builder;
        builder.switch_to_block(slow);
        builder.set_cold_block(slow);
        builder.seal_block(slow);
        let status = self.call_through_vm(pc, table, signature_index, row, (args, out));
        let builder = &mut self.body.core.builder;
        builder.ins().jump(done, &[BlockArg::Value(status)]);
        builder.switch_to_block(done);
        builder.seal_block(done);
        let status = builder.block_params(done)[0];
        self.propagate(status);
        self.take_result(instruction, &signature, out);
    }

    /// The native calls of the row: when it holds the `expected` id, a call of its function in
    /// this instance (the running context's) or in another (the instance's record), each jumping
    /// to `done` with its status; any other row, or a function without a body, leaves for `slow`.
    fn native_calls(
        &mut self,
        words: ir::Value,
        row: ir::Value,
        expected: CanonicalSignature,
        signature: &crate::WasmSignature,
        buffers: (ir::Value, ir::Value),
        (slow, done): (ir::Block, ir::Block),
    ) {
        let address = self.table_row(words, row);
        let context = self.body.core.context;
        let builder = &mut self.body.core.builder;
        let flags = MemFlagsData::trusted();
        let id = builder.ins().load(types::I32, flags, address, TableTarget::SIGNATURE);
        let same_signature = builder.ins().icmp_imm_u(IntCC::Equal, id, expected.word() as i64);
        let (resolve, local, foreign) = (builder.create_block(), builder.create_block(), builder.create_block());
        builder.ins().brif(same_signature, resolve, &[], slow, &[]);
        builder.switch_to_block(resolve);
        builder.seal_block(resolve);
        let instance = builder.ins().load(types::I64, flags, address, TableTarget::INSTANCE);
        let entry = builder.ins().load(types::I32, flags, address, TableTarget::ENTRY);
        let entry = builder.ins().uextend(types::I64, entry);
        let running = builder.ins().load(types::I64, invariant(), context, CONTEXT_INSTANCE);
        let mine = builder.ins().icmp(IntCC::Equal, instance, running);
        builder.ins().brif(mine, local, &[], foreign, &[]);
        builder.switch_to_block(local);
        builder.seal_block(local);
        self.local_call(entry, signature, buffers, (slow, done));
        let builder = &mut self.body.core.builder;
        builder.switch_to_block(foreign);
        builder.seal_block(foreign);
        self.foreign_call(instance, entry, signature, buffers, (slow, done));
    }

    /// The call of the function at index `entry` of the running artifact's code table, in the
    /// running context: its status jumps to `done`.
    fn local_call(
        &mut self,
        entry: ir::Value,
        signature: &crate::WasmSignature,
        buffers: (ir::Value, ir::Value),
        (slow, done): (ir::Block, ir::Block),
    ) {
        let context = self.body.core.context;
        let table = self.body.core.builder.ins().load(types::I64, invariant(), context, CONTEXT_TABLE);
        let body = self.entry_body(table, entry, slow);
        let status = self.call_body(body, signature, buffers, (self.base, self.length));
        self.body.core.builder.ins().jump(done, &[BlockArg::Value(status)]);
    }

    /// The body at index `entry` of the code table at `table`; a function without one leaves
    /// for `slow`.
    pub(super) fn entry_body(&mut self, table: ir::Value, entry: ir::Value, slow: ir::Block) -> ir::Value {
        let builder = &mut self.body.core.builder;
        let offset = builder.ins().imul_imm_u(entry, size_of::<usize>() as i64);
        let slot = builder.ins().iadd(table, offset);
        let body = builder.ins().load(types::I64, MemFlagsData::trusted(), slot, 0);
        let compiled = builder.ins().icmp_imm_u(IntCC::NotEqual, body, 0);
        let enter = builder.create_block();
        builder.ins().brif(compiled, enter, &[], slow, &[]);
        builder.switch_to_block(enter);
        builder.seal_block(enter);
        body
    }

    /// The call of `body` with the arguments the call spilled and the memory arguments the
    /// callee takes: its status, the result stored through `out`.
    pub(super) fn call_body(
        &mut self,
        body: ir::Value,
        signature: &crate::WasmSignature,
        (args, out): (ir::Value, ir::Value),
        (base, length): (ir::Value, Option<ir::Value>),
    ) -> ir::Value {
        let mut arguments = vec![self.body.core.context];
        if self.memory.base {
            arguments.push(base);
        }
        arguments.extend(length);
        for (offset, ty) in signature.params.iter().enumerate() {
            let raw = self.load_word_at(args, offset);
            arguments.push(match Kind::of(*ty).expect("planned") {
                Kind::I32 => self.body.core.builder.ins().ireduce(types::I32, raw),
                Kind::I64 => raw,
            });
        }
        let callee = internal_signature(self.memory, &signature.params, signature.results.first().copied(), self.call_conv);
        let callee = self.body.core.builder.import_signature(callee);
        let call = self.body.core.builder.ins().call_indirect(callee, body, &arguments);
        let results = self.body.core.builder.inst_results(call).to_vec();
        if let Some(result) = signature.results.first() {
            let value = match Kind::of(*result).expect("planned") {
                Kind::I32 => self.body.core.builder.ins().uextend(types::I64, results[1]),
                Kind::I64 => results[1],
            };
            self.body.core.builder.ins().store(MemFlagsData::trusted(), value, out, 0);
        }
        results[0]
    }
}

/// The bytes of a word of a spilled argument, a result or a status.
const WORD: usize = size_of::<u64>();

impl<H: Host> Emitter<'_, '_, H> {
    /// A stack slot of `words` words.
    fn words_slot(&mut self, words: usize) -> ir::StackSlot {
        self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            (words * WORD) as u32,
            WORD.trailing_zeros() as u8,
        ))
    }

    /// The word `offset` words from `address`.
    fn load_word_at(&mut self, address: ir::Value, offset: usize) -> ir::Value {
        let builder = &mut self.body.core.builder;
        let at = (offset * WORD) as i32;
        builder.ins().load(types::I64, MemFlagsData::trusted(), address, at)
    }

    /// Stores the call window's arguments, a word each, in a slot with one word more for the
    /// result: the addresses of the arguments and of the result.
    fn spill_arguments(
        &mut self,
        instruction: WideInstruction,
        signature: &crate::WasmSignature,
    ) -> (ir::Value, ir::Value) {
        let slot = self.words_slot(signature.params.len() + 1);
        let window = instruction.call_window();
        for (offset, ty) in signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.body.core.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address = self.body.core.builder.ins().stack_addr(types::I64, slot, (offset * WORD) as i32);
            self.body.core.builder.ins().store(MemFlagsData::trusted(), value, address, 0);
        }
        let result = (signature.params.len() * WORD) as i32;
        let args = self.body.core.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self.body.core.builder.ins().stack_addr(types::I64, slot, result);
        (args, out)
    }

    /// The call through the VM's callout, which resolves the table's element again, checks it
    /// and runs it wherever it lives: its status.
    fn call_through_vm(
        &mut self,
        pc: usize,
        table: u32,
        signature_index: u32,
        row: ir::Value,
        (args, out): (ir::Value, ir::Value),
    ) -> ir::Value {
        let site = self
            .reference_site_ids
            .binary_search_by_key(&pc, |(site_pc, _)| *site_pc)
            .ok()
            .map(|index| self.reference_site_ids[index].1)
            .expect("compiled indirect call has a cache site id");
        let offset = if self.diagnostics_enabled {
            CONTEXT_CALL_REFERENCE_DIAGNOSTIC
        } else {
            CONTEXT_CALL_REFERENCE
        };
        let context = self.body.core.context;
        let builder = &mut self.body.core.builder;
        let target = builder.ins().load(types::I64, invariant(), context, offset);
        let table = builder.ins().iconst(types::I64, i64::from(table));
        let signature_index = builder.ins().iconst(types::I64, i64::from(signature_index));
        let site = builder.ins().iconst(types::I64, site as i64);
        let words = [context, table, row, signature_index, site, args, out];
        let mut callout = ir::Signature::new(self.call_conv);
        callout.params = words.iter().map(|_| ir::AbiParam::new(types::I64)).collect();
        callout.returns.push(ir::AbiParam::new(types::I64));
        let callout = self.body.core.builder.import_signature(callout);
        let call = self.body.core.builder.ins().call_indirect(callout, target, &words);
        self.body.core.builder.inst_results(call)[0]
    }

    /// Writes the call's result, the word `out` holds, to the call's register.
    fn take_result(
        &mut self,
        instruction: WideInstruction,
        signature: &crate::WasmSignature,
        out: ir::Value,
    ) {
        let destination = instruction.result_register();
        let Some(ty) = signature.results.first() else {
            self.write_constant(destination, Kind::I32, 0);
            return;
        };
        let kind = Kind::of(*ty).expect("planned");
        let bits = self.load_word_at(out, 0);
        let value = match kind {
            Kind::I32 => self.body.core.builder.ins().ireduce(types::I32, bits),
            Kind::I64 => bits,
        };
        self.write(destination, kind, value);
    }
}
