//! The Wasm tier's indirect calls: the primitives of `WasmTable` as Cranelift code over the
//! table's mirror, which the VM publishes as words (`CallContext::table_words`), and the call
//! through a row's function. The target's checks are the staged rule
//! (`staged::wasm_call::indirect_target`), emitted: the bound, the null id and the id compare
//! trap in the interpreter's order; an id that is not the expected one is the VM's to relate.
//! A call goes through the row natively where the row names a function this artifact compiled,
//! in this instance or, switching the context to the instance's record
//! (`wasm_instance_switch`), in another; every other target (a host function, code not
//! compiled, a memory the call cannot be made across) goes through the VM's callout.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::wasm_stage::infallible;
use super::wasm_call_stage::is_tail_call;
use super::{
    Analysis, BODY_CALL_CONV, CONTEXT_CALL_REFERENCE,
    CONTEXT_INSTANCE, CONTEXT_TABLE_WORDS, CallContext, Emitter, Kind,
    Slot, Vm, internal_signature, invariant,
};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::tier_surface::{TableTarget, TableWords};
use quench_runtime::staged::machine::{MachineAccess, MachineFlow, MachineStage, MachineTail};
use quench_runtime::staged::wasm::Width;
use quench_runtime::wasm::conversion::ScalarConversionOperator as Conversion;
use quench_runtime::staged::wasm_call::{WasmTable, indirect_target};
use quench_runtime::wasm::CanonicalSignature;
use quench_runtime::wasm::integer::I64BinaryOperator as Binary;
use cranelift_codegen::ir::{self, MemFlagsData, types};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The table an indirect target is being emitted for: the address of its published words, its
/// binding and the type the call expects.
#[derive(Clone, Copy)]
pub(crate) struct IndirectSite {
    words: ir::Value,
    table: u32,
    signature_index: u32,
}

/// The row an indirect target checked: its element index (the target's value), the row's
/// address and its signature id, each defined before the checks' branches. A call emitted in
/// the block the target ended in (only moves of its arguments between: anything that could
/// change the table also branches) reads them instead of loading them again.
#[derive(Clone, Copy)]
pub(crate) struct CheckedRow {
    block: Option<ir::Block>,
    index: ir::Value,
    address: ir::Value,
    id: ir::Value,
}

/// Whether the element at `index` of the table at binding `table` satisfies the type
/// `expected` (a type index of the program) although its id is not the expected one: a subtype,
/// or a host function whose signature the VM compares. A reference the VM cannot compare does
/// not satisfy it.
pub(super) unsafe extern "C" fn compiled_table_accepts(
    context: *mut CallContext,
    table: u64,
    index: u64,
    expected: u64,
) -> u64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let context = unsafe { &*context };
    let vm = unsafe { &*context.vm.cast::<Vm>() };
    let binding = vm.heap_reads().environment_slot(context.env, table as usize);
    let reference = binding.and_then(|table| vm.wasm_table_get(table, index).ok());
    let pool = vm.programs.wasm_signatures(context.program);
    let accepted = reference
        .zip(pool)
        .and_then(|(reference, pool)| vm.wasm_callable_matches(reference, pool, expected as u32, false));
    u64::from(accepted.unwrap_or(false))
}

impl Emitter<'_, '_> {
    fn site(&self) -> IndirectSite {
        self.indirect.expect("an indirect target is being emitted")
    }

    /// Makes `block` the current one: the code that follows is its own.
    pub(crate) fn enter(&mut self, block: ir::Block) {
        self.body.core.builder.switch_to_block(block);
        self.body.core.builder.seal_block(block);
    }

    /// The address of the published words of the table at binding `table`: the instance's table
    /// area holds them, indexed by the binding.
    ///
    /// Loaded at each use, not as invariant: an invariant load is hoisted out of the loop and
    /// then lives across every call in it, taking a callee-saved register (or a spill) from the
    /// loop's own values. Two loads from the context, which stays in a register, cost no more
    /// than the spill's reload.
    fn table_words(&mut self, table: u32) -> ir::Value {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let array = self.load_word(Width::I64, flags, context, CONTEXT_TABLE_WORDS);
        let offset = (table as usize * size_of::<*const TableWords>()) as i32;
        self.load_word(Width::I64, flags, array, offset)
    }

    /// The address of the row of element `index`.
    fn table_row(&mut self, words: ir::Value, index: ir::Value) -> ir::Value {
        let base = self.load_word(Width::I64, MemFlagsData::trusted(), words, TableWords::BASE);
        let size = self.constant(Width::I64, TableTarget::SIZE as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, index, size);
        self.operate(Binary::Add, Width::I64, base, offset)
    }

    /// Lowers the target of an indirect call: the staged rule over the table, whose result,
    /// the row of the element, the call that follows reads.
    pub(crate) fn indirect_target(&mut self, pc: usize, instruction: WideInstruction) {
        let table = self.binding_slot(pc);
        let signature_index = instruction.imm();
        let expected = self.analysis.type_canonical(signature_index).expect("planned");
        let words = self.table_words(table);
        self.indirect = Some(IndirectSite { words, table, signature_index });
        let index = self.read(instruction.c(), Kind::I32);
        let index = self.convert(Conversion::ExtendI32Unsigned, index);
        self.checked_row = None;
        let (_, row) = infallible(indirect_target(self, index, expected));
        self.checked_row = self.checked_row.map(|checked| CheckedRow { block: self.body.core.builder.current_block(), ..checked });
        self.indirect = None;
        self.write(instruction.a(), Kind::I64, row);
    }
}

impl WasmTable for Emitter<'_, '_> {
    fn table_range_exceeds(&mut self, index: ir::Value, extent: ir::Value) -> ir::Value {
        let end = self.operate(Binary::Add, Width::I64, index, extent);
        let words = self.site().words;
        let length = self.load_word(Width::I64, MemFlagsData::trusted(), words, TableWords::LENGTH);
        self.operate(Binary::GreaterUnsigned, Width::I64, end, length)
    }

    fn element_signature(&mut self, index: ir::Value) -> ir::Value {
        let row = self.table_row(self.site().words, index);
        let id = self.load_word(Width::I32, MemFlagsData::trusted(), row, TableTarget::SIGNATURE);
        self.checked_row = Some(CheckedRow { block: None, index, address: row, id });
        self.convert(Conversion::ExtendI32Unsigned, id)
    }

    /// The reference of an element, for the call that follows, is its row: the table keeps it.
    fn table_element(&mut self, index: ir::Value) -> ir::Value {
        index
    }

    type Row = ir::Value;

    /// The row word is the index, which the call that follows resolves through the table.
    fn element_row(&mut self, index: ir::Value) -> ir::Value {
        index
    }

    /// Reached only where the ids differ (`unless_equal`): the VM relates the element's type to
    /// the expected one and traps on a mismatch itself, so the flag is a constant and the rule's
    /// mismatch test folds away.
    fn signature_accepts(
        &mut self,
        reference: ir::Value,
        _signature: ir::Value,
        _expected: ir::Value,
    ) -> ir::Value {
        let site = self.site();
        let context = self.body.core.context;
        let table = self.constant(Width::I64, i64::from(site.table));
        let expected_type = self.constant(Width::I64, i64::from(site.signature_index));
        let target = crate::compiled::slow_paths::EngineSlowPath::TableAccepts;
        let related = self.call_leaf(target, &[context, table, reference, expected_type]);
        let related = self.convert(Conversion::WrapI64, related);
        let mismatch = self.operate_unary(quench_runtime::wasm::integer::I64UnaryOperator::EqualZero, Width::I32, related);
        infallible(self.trap_when(mismatch, quench_runtime::WasmTrap::IndirectCallTypeMismatch));
        self.constant(Width::I32, 1)
    }

    /// Equal ids continue at once: one compare and branch. The other path, cold, runs the rule's
    /// null check and the VM's relating (`signature_accepts`), which trap themselves, so both
    /// paths join accepted with no flag joined and tested again.
    fn unless_equal(
        &mut self,
        signature: ir::Value,
        expected: ir::Value,
        otherwise: impl FnOnce(&mut Self) -> Result<ir::Value, Self::Exit>,
    ) -> Result<ir::Value, Self::Exit> {
        let equal = self.operate(Binary::Equal, Width::I64, signature, expected);
        let builder = &mut self.body.core.builder;
        let (relate, join) = (builder.create_block(), builder.create_block());
        self.branch_if(equal, (join, &[]), (relate, &[]));
        self.body.core.builder.set_cold_block(relate);
        self.enter(relate);
        otherwise(self)?;
        self.goto(join, &[]);
        self.enter(join);
        Ok(self.constant(Width::I32, 1))
    }
}

impl Analysis<'_> {
    /// The canonical id of the type `index` of the program.
    pub(crate) fn type_canonical(&self, index: u32) -> Option<CanonicalSignature> {
        self.vm
            .programs
            .wasm_signatures(self.program)?
            .type_canonical(index)
    }
}

impl Emitter<'_, '_> {
    /// Lowers a call of the row an indirect target produced: natively through the function the
    /// row names where it has one, else through the VM's callout.
    pub(crate) fn call_reference(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::IndirectReference { signature: signature_index, table } = self.plan.operand(pc)
        else {
            unreachable!("planned indirect calls retain their table binding");
        };
        let signature = self.analysis.type_signature(signature_index).expect("planned").clone();
        let expected = self.analysis.type_canonical(signature_index).expect("planned");
        // The native fast path of a tail call is a `return_call_indirect`, when results agree.
        let native_tail = is_tail_call(instruction) && signature.results.first().copied() == self.plan.result;
        let row = self.read(instruction.b(), Kind::I64);
        let words = self.table_words(table);
        // The target checked in this block: its row and id, not loaded again.
        let block = self.body.core.builder.current_block();
        let checked = self.checked_row.take().filter(|checked| checked.block.is_some() && checked.block == block && checked.index == row);
        let builder = &mut self.body.core.builder;
        // `done` takes the status and the result's bits: the native call hands both on in
        // registers; only the cold paths spill the arguments and read the result from memory.
        let (slow, done) = (builder.create_block(), builder.create_block());
        builder.append_block_param(done, types::I64);
        builder.append_block_param(done, types::I64);
        self.native_calls(words, row, checked, expected, (instruction, &signature), (slow, done), native_tail);
        self.body.core.builder.set_cold_block(slow);
        self.enter(slow);
        let (args, out) = self.spill_arguments(instruction, &signature);
        let status = self.call_through_vm(pc, table, signature_index, row, (args, out));
        let bits = self.result_bits(&signature, out);
        self.goto(done, &[status, bits]);
        self.enter(done);
        let (status, bits) = (self.body.core.builder.block_params(done)[0], self.body.core.builder.block_params(done)[1]);
        self.propagate(status);
        self.take_result(instruction, &signature, bits);
        self.return_if_tail(instruction);
    }

    /// The native calls of the row: when it holds the `expected` id, a call of its function in
    /// this instance (the running context's) or in another (the instance's record), each jumping
    /// to `done` with its status; any other row, or a function without a body, leaves for `slow`.
    /// With `native_tail`, the local call is a `return_call_indirect` that does not reach `done`.
    fn native_calls(
        &mut self,
        words: ir::Value,
        row: ir::Value,
        checked: Option<CheckedRow>,
        expected: CanonicalSignature,
        (instruction, signature): (WideInstruction, &quench_runtime::WasmSignature),
        (slow, done): (ir::Block, ir::Block),
        native_tail: bool,
    ) {
        let context = self.body.core.context;
        let flags = MemFlagsData::trusted();
        let (address, id) = match checked {
            Some(checked) => (checked.address, checked.id),
            None => {
                let address = self.table_row(words, row);
                (address, self.load_word(Width::I32, flags, address, TableTarget::SIGNATURE))
            }
        };
        let wanted = self.constant(Width::I32, expected.word() as i64);
        let same_signature = self.operate(Binary::Equal, Width::I32, id, wanted);
        let builder = &mut self.body.core.builder;
        let (resolve, local, foreign) = (builder.create_block(), builder.create_block(), builder.create_block());
        self.branch_if(same_signature, (resolve, &[]), (slow, &[]));
        self.enter(resolve);
        // The row's body (one load): 0 is no compiled body, which takes the slow path.
        let body = self.entry_target(crate::compiled::slow_paths::EngineEntry::WasmTableBody, flags, address, TableTarget::BODY);
        let none = self.constant(Width::I64, 0);
        let compiled = self.operate(Binary::NotEqual, Width::I64, body.word(), none);
        let present = self.body.core.builder.create_block();
        self.branch_if(compiled, (present, &[]), (slow, &[]));
        self.enter(present);
        let instance = self.load_word(Width::I64, flags, address, TableTarget::INSTANCE);
        // At each use, like `table_words`: not a value living across the loop's calls.
        let running = self.load_word(Width::I64, flags, context, CONTEXT_INSTANCE);
        let mine = self.operate(Binary::Equal, Width::I64, instance, running);
        self.branch_if(mine, (local, &[]), (foreign, &[]));
        self.enter(local);
        let window = instruction.call_window();
        let arguments: Vec<ir::Value> = signature
            .params
            .iter()
            .enumerate()
            .map(|(offset, ty)| self.read(window.base + offset as u16, Kind::of(*ty).expect("planned")))
            .collect();
        if native_tail {
            let length = self.passed_length();
            self.tail_body_values(body, signature, &arguments, (self.base, length));
        } else {
            let length = self.passed_length();
            let (status, bits) = self.call_body_values(body, signature, &arguments, (self.base, length));
            self.goto(done, &[status, bits]);
        }
        self.body.core.builder.set_cold_block(foreign);
        self.enter(foreign);
        let entry = self.load_word(Width::I32, flags, address, TableTarget::ENTRY);
        let entry = self.convert(Conversion::ExtendI32Unsigned, entry);
        let buffers = self.spill_arguments(instruction, signature);
        let switched = self.body.core.builder.create_block();
        self.body.core.builder.append_block_param(switched, types::I64);
        self.foreign_call(instance, entry, signature, buffers, (slow, switched));
        self.body.core.builder.set_cold_block(switched);
        self.enter(switched);
        let status = self.body.core.builder.block_params(switched)[0];
        let bits = self.result_bits(signature, buffers.1);
        self.goto(done, &[status, bits]);
    }

    /// The body at index `entry` of the code table at `table`; a function without one leaves
    /// for `slow`.
    pub(crate) fn entry_body(&mut self, table: ir::Value, entry: ir::Value, slow: ir::Block) -> quench_runtime::staged::machine::CallTarget<ir::Value> {
        let size = self.constant(Width::I64, size_of::<usize>() as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, entry, size);
        let slot = self.operate(Binary::Add, Width::I64, table, offset);
        let body = self.entry_target(crate::compiled::slow_paths::EngineEntry::WasmTableBody, MemFlagsData::trusted(), slot, 0);
        let none = self.constant(Width::I64, 0);
        let compiled = self.operate(Binary::NotEqual, Width::I64, body.word(), none);
        let enter = self.body.core.builder.create_block();
        self.branch_if(compiled, (enter, &[]), (slow, &[]));
        self.enter(enter);
        body
    }

    /// The call of `body` with the arguments the call spilled and the memory arguments the
    /// callee takes: its status, the result stored through `out`.
    pub(crate) fn call_body(
        &mut self,
        body: quench_runtime::staged::machine::CallTarget<ir::Value>,
        signature: &quench_runtime::WasmSignature,
        (args, out): (ir::Value, ir::Value),
        (base, length): (ir::Value, Option<ir::Value>),
    ) -> ir::Value {
        let values: Vec<ir::Value> = signature
            .params
            .iter()
            .enumerate()
            .map(|(offset, ty)| {
                let raw = self.load_word_at(args, offset);
                match Kind::of(*ty).expect("planned") {
                    Kind::I32 => self.convert(Conversion::WrapI64, raw),
                    Kind::I64 | Kind::Reference => raw,
                }
            })
            .collect();
        let (status, bits) = self.call_body_values(body, signature, &values, (base, length));
        if !signature.results.is_empty() {
            self.store_word(MemFlagsData::trusted(), bits, out, 0);
        }
        status
    }

    /// The call of `body` with `arguments`, each of its parameter's kind, and the memory
    /// arguments the callee takes: its status and its result as a word of bits (0 for none).
    fn call_body_values(
        &mut self,
        body: quench_runtime::staged::machine::CallTarget<ir::Value>,
        signature: &quench_runtime::WasmSignature,
        values: &[ir::Value],
        (base, length): (ir::Value, Option<ir::Value>),
    ) -> (ir::Value, ir::Value) {
        let arguments = self.body_arguments(values, (base, length));
        let callee = internal_signature(self.memory, &signature.params, signature.results.first().copied(), BODY_CALL_CONV);
        let callee = self.body.core.builder.import_signature(callee);
        let roots = self.root();
        let results = self.call_signature(callee, body, &arguments);
        self.unroot(roots);
        let bits = match signature.results.first() {
            Some(result) => match Kind::of(*result).expect("planned") {
                Kind::I32 => self.convert(Conversion::ExtendI32Unsigned, results[1]),
                Kind::I64 | Kind::Reference => results[1],
            },
            None => self.constant(Width::I64, 0),
        };
        (results[0], bits)
    }

    /// The arguments of a body: the context, the memory arguments it takes, then `values`.
    fn body_arguments(&self, values: &[ir::Value], (base, length): (ir::Value, Option<ir::Value>)) -> Vec<ir::Value> {
        let mut arguments = vec![self.body.core.context];
        if self.memory.base {
            arguments.push(base);
        }
        arguments.extend(length);
        arguments.extend_from_slice(values);
        arguments
    }

    /// The native tail call of `body` with `values`: a `return_call_indirect` (no roots, as `tail_known`).
    fn tail_body_values(
        &mut self,
        body: quench_runtime::staged::machine::CallTarget<ir::Value>,
        signature: &quench_runtime::WasmSignature,
        values: &[ir::Value],
        (base, length): (ir::Value, Option<ir::Value>),
    ) {
        let arguments = self.body_arguments(values, (base, length));
        let callee = internal_signature(self.memory, &signature.params, signature.results.first().copied(), BODY_CALL_CONV);
        let callee = self.body.core.builder.import_signature(callee);
        self.interrupt_safepoint();
        self.tail_call_indirect(callee, body, &arguments);
    }
}

/// The bytes of a word of a spilled argument, a result or a status.
const WORD: usize = size_of::<u64>();

impl Emitter<'_, '_> {
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
        self.load_word(Width::I64, MemFlagsData::trusted(), address, (offset * WORD) as i32)
    }

    /// Stores the call window's arguments, a word each, in a slot with one word more for the
    /// result: the addresses of the arguments and of the result.
    fn spill_arguments(
        &mut self,
        instruction: WideInstruction,
        signature: &quench_runtime::WasmSignature,
    ) -> (ir::Value, ir::Value) {
        let slot = self.words_slot(signature.params.len() + 1);
        let window = instruction.call_window();
        for (offset, ty) in signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.convert(Conversion::ExtendI32Unsigned, value),
                Kind::I64 | Kind::Reference => value,
            };
            let address = self.slot_address(slot, (offset * WORD) as i32);
            self.store_word(MemFlagsData::trusted(), value, address, 0);
        }
        let result = (signature.params.len() * WORD) as i32;
        let args = self.slot_address(slot, 0);
        let out = self.slot_address(slot, result);
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
        let context = self.body.core.context;
        let target = self.entry_target(crate::compiled::slow_paths::EngineEntry::WasmTableCallOut, invariant(), context, CONTEXT_CALL_REFERENCE);
        let table = self.constant(Width::I64, i64::from(table));
        let signature_index = self.constant(Width::I64, i64::from(signature_index));
        let site = self.constant(Width::I64, site as i64);
        let roots = self.root();
        let status = self.call_words(target, &[context, table, row, signature_index, site, args, out]);
        self.unroot(roots);
        status
    }

    /// The bits of the call's result that `out` holds: 0 where the signature has none.
    fn result_bits(&mut self, signature: &quench_runtime::WasmSignature, out: ir::Value) -> ir::Value {
        if signature.results.is_empty() {
            return self.constant(Width::I64, 0);
        }
        self.load_word_at(out, 0)
    }

    /// Writes the call's result, whose word of bits is `bits`, to the call's register.
    fn take_result(
        &mut self,
        instruction: WideInstruction,
        signature: &quench_runtime::WasmSignature,
        bits: ir::Value,
    ) {
        let destination = instruction.result_register();
        let Some(ty) = signature.results.first() else {
            self.write_constant(destination, Kind::I32, 0);
            return;
        };
        let kind = Kind::of(*ty).expect("planned");
        let value = match kind {
            Kind::I32 => self.convert(Conversion::WrapI64, bits),
            Kind::I64 | Kind::Reference => bits,
        };
        self.write(destination, kind, value);
    }
}
