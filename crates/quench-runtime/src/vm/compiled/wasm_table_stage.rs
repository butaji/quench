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
use super::wasm_call_stage::is_tail_call;
use super::{
    Analysis, BODY_CALL_CONV, CONTEXT_CALL_REFERENCE, CONTEXT_CALL_REFERENCE_DIAGNOSTIC,
    CONTEXT_INSTANCE, CONTEXT_TABLE_WORDS, CallContext, Emitter, Host, Kind,
    Slot, Vm, internal_signature, invariant,
};
use crate::bytecode::WideInstruction;
use crate::heap::{TableTarget, TableWords};
use crate::staged::machine::{MachineAccess, MachineFlow, MachineStage};
use crate::staged::wasm::Width;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::staged::wasm_call::{WasmTable, indirect_target};
use crate::wasm::CanonicalSignature;
use crate::wasm::integer::I64BinaryOperator as Binary;
use cranelift_codegen::ir::{self, MemFlagsData, types};

/// The table an indirect target is being emitted for: the address of its published words, its
/// binding and the type the call expects.
#[derive(Clone, Copy)]
pub(super) struct IndirectSite {
    words: ir::Value,
    table: u32,
    signature_index: u32,
}

/// The row an indirect target checked: its element index (the target's value), the row's
/// address and its signature id, each defined before the checks' branches. A call emitted in
/// the block the target ended in (only moves of its arguments between: anything that could
/// change the table also branches) reads them instead of loading them again.
#[derive(Clone, Copy)]
pub(super) struct CheckedRow {
    block: Option<ir::Block>,
    index: ir::Value,
    address: ir::Value,
    id: ir::Value,
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

    /// Makes `block` the current one: the code that follows is its own.
    pub(super) fn enter(&mut self, block: ir::Block) {
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
    pub(super) fn indirect_target(&mut self, pc: usize, instruction: WideInstruction) {
        let table = self.binding_slot(pc);
        let signature_index = instruction.imm();
        let expected = self.analysis.type_canonical(signature_index).expect("planned");
        let words = self.table_words(table);
        self.indirect = Some(IndirectSite { words, table, signature_index });
        let index = self.read(instruction.c(), Kind::I32);
        let index = self.convert(Conversion::ExtendI32Unsigned, index);
        self.checked_row = None;
        let row = infallible(indirect_target(self, index, expected));
        self.checked_row = self.checked_row.map(|checked| CheckedRow { block: self.body.core.builder.current_block(), ..checked });
        self.indirect = None;
        self.write(instruction.a(), Kind::I64, row);
    }
}

impl<H: Host> WasmTable for Emitter<'_, '_, H> {
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
        let target = compiled_table_accepts::<H> as *const () as usize;
        let related = self.call_leaf(target, &[context, table, reference, expected_type]);
        let related = self.convert(Conversion::WrapI64, related);
        let mismatch = self.operate_unary(crate::wasm::integer::I64UnaryOperator::EqualZero, Width::I32, related);
        infallible(self.trap_when(mismatch, crate::WasmTrap::IndirectCallTypeMismatch));
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
    /// row names where it has one, else through the VM's callout. The arguments reach a native
    /// callee as values; only the callout's path stores them in memory, and reads its result back.
    pub(super) fn call_reference(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::IndirectReference { signature: signature_index, table } = self.plan.operand(pc)
        else {
            unreachable!("planned indirect calls retain their table binding");
        };
        let signature = self.analysis.type_signature(signature_index).expect("planned").clone();
        let expected = self.analysis.type_canonical(signature_index).expect("planned");
        let arguments = self.call_arguments(instruction, &signature);
        let row = self.read(instruction.b(), Kind::I64);
        let words = self.table_words(table);
        let result = signature.results.first().map(|ty| Kind::of(*ty).expect("planned"));
        let builder = &mut self.body.core.builder;
        // `done` takes the status and the result's bits: the native call hands both on in
        // registers; only the cold paths spill the arguments and read the result from memory.
        let (slow, done) = (builder.create_block(), builder.create_block());
        builder.append_block_param(done, types::I64);
        if let Some(kind) = result {
            builder.append_block_param(done, kind.ir());
        }
        self.native_calls(words, row, expected, &signature, &arguments, (slow, done));
        self.body.core.builder.set_cold_block(slow);
        self.enter(slow);
        let (args, out) = self.spill_arguments(&arguments, &signature);
        let status = self.call_through_vm(pc, table, signature_index, row, (args, out));
        let mut exit = vec![status];
        if let Some(kind) = result {
            exit.push(self.slow_result(out, kind));
        }
        self.goto(done, &exit);
        self.enter(done);
        let values = self.body.core.builder.block_params(done).to_vec();
        self.propagate(values[0]);
        let destination = instruction.result_register();
        match result {
            Some(kind) => self.write(destination, kind, values[1]),
            None => self.write_constant(destination, Kind::I32, 0),
        }
    }

    /// The native calls of the row: when it holds the `expected` id, a call of its function in
    /// this instance (the running context's) or in another (the instance's record), each jumping
    /// to `done` with its exit; any other row, or a function without a body, leaves for `slow`.
    fn native_calls(
        &mut self,
        words: ir::Value,
        row: ir::Value,
        checked: Option<CheckedRow>,
        expected: CanonicalSignature,
        signature: &crate::WasmSignature,
        arguments: &[ir::Value],
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
        let body = self.load_word(Width::I64, flags, address, TableTarget::BODY);
        let none = self.constant(Width::I64, 0);
        let compiled = self.operate(Binary::NotEqual, Width::I64, body, none);
        let present = self.body.core.builder.create_block();
        self.branch_if(compiled, (present, &[]), (slow, &[]));
        self.enter(present);
        let instance = self.load_word(Width::I64, flags, address, TableTarget::INSTANCE);
        // At each use, like `table_words`: not a value living across the loop's calls.
        let running = self.load_word(Width::I64, flags, context, CONTEXT_INSTANCE);
        let mine = self.operate(Binary::Equal, Width::I64, instance, running);
        self.branch_if(mine, (local, &[]), (foreign, &[]));
        self.enter(local);
        self.local_call(entry, signature, arguments, (slow, done));
        self.enter(foreign);
        self.foreign_call(instance, entry, signature, arguments, (slow, done));
    }

    /// The call of the function at index `entry` of the running artifact's code table, in the
    /// running context: its exit jumps to `done`.
    fn local_call(
        &mut self,
        entry: ir::Value,
        signature: &crate::WasmSignature,
        arguments: &[ir::Value],
        (slow, done): (ir::Block, ir::Block),
    ) {
        let context = self.body.core.context;
        let table = self.load_word(Width::I64, invariant(), context, CONTEXT_TABLE);
        let body = self.entry_body(table, entry, slow);
        let (status, result) = self.call_body(body, signature, arguments, (self.base, self.length));
        self.goto_exit(done, status, result);
    }

    /// Leaves a native call for `done` with its status and, for a result, the result.
    pub(super) fn goto_exit(&mut self, done: ir::Block, status: ir::Value, result: Option<ir::Value>) {
        let mut values = vec![status];
        values.extend(result);
        self.goto(done, &values);
    }

    /// The body at index `entry` of the code table at `table`; a function without one leaves
    /// for `slow`.
    pub(super) fn entry_body(&mut self, table: ir::Value, entry: ir::Value, slow: ir::Block) -> ir::Value {
        let size = self.constant(Width::I64, size_of::<usize>() as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, entry, size);
        let slot = self.operate(Binary::Add, Width::I64, table, offset);
        let body = self.load_word(Width::I64, MemFlagsData::trusted(), slot, 0);
        let none = self.constant(Width::I64, 0);
        let compiled = self.operate(Binary::NotEqual, Width::I64, body, none);
        let enter = self.body.core.builder.create_block();
        self.branch_if(compiled, (enter, &[]), (slow, &[]));
        self.enter(enter);
        body
    }

    /// The call of `body` with the call's argument values and the memory arguments the callee
    /// takes, in its native ABI: its status and, for a result, the result, as it returns them.
    pub(super) fn call_body(
        &mut self,
        body: ir::Value,
        signature: &crate::WasmSignature,
        arguments: &[ir::Value],
        (base, length): (ir::Value, Option<ir::Value>),
    ) -> (ir::Value, Option<ir::Value>) {
        let mut values = vec![self.body.core.context];
        if self.memory.base {
            values.push(base);
        }
        values.extend(length);
        values.extend_from_slice(arguments);
        let callee = internal_signature(self.memory, &signature.params, signature.results.first().copied(), self.call_conv);
        let callee = self.body.core.builder.import_signature(callee);
        let results = self.call_signature(callee, body, &values);
        (results[0], results.get(1).copied())
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
        self.load_word(Width::I64, MemFlagsData::trusted(), address, (offset * WORD) as i32)
    }

    /// The values of the call window's arguments, each as its Wasm kind: what a native callee
    /// takes as it is.
    fn call_arguments(&mut self, instruction: WideInstruction, signature: &crate::WasmSignature) -> Vec<ir::Value> {
        let window = instruction.call_window();
        let mut values = Vec::with_capacity(signature.params.len());
        for (offset, ty) in signature.params.iter().enumerate() {
            values.push(self.read(window.base + offset as u16, Kind::of(*ty).expect("planned")));
        }
        values
    }

    /// Stores the arguments, a word each, in a slot with one word more for the result: the
    /// addresses of the arguments and of the result, which the VM's callout takes.
    fn spill_arguments(
        &mut self,
        arguments: &[ir::Value],
        signature: &crate::WasmSignature,
    ) -> (ir::Value, ir::Value) {
        let slot = self.words_slot(signature.params.len() + 1);
        for (offset, (value, ty)) in arguments.iter().zip(&signature.params).enumerate() {
            let value = match Kind::of(*ty).expect("planned") {
                Kind::I32 => self.convert(Conversion::ExtendI32Unsigned, *value),
                Kind::I64 => *value,
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
        let offset = if self.diagnostics_enabled {
            CONTEXT_CALL_REFERENCE_DIAGNOSTIC
        } else {
            CONTEXT_CALL_REFERENCE
        };
        let context = self.body.core.context;
        let target = self.load_word(Width::I64, invariant(), context, offset);
        let table = self.constant(Width::I64, i64::from(table));
        let signature_index = self.constant(Width::I64, i64::from(signature_index));
        let site = self.constant(Width::I64, site as i64);
        let roots = self.root();
        let status = self.call_words(target, &[context, table, row, signature_index, site, args, out]);
        self.unroot(roots);
        status
    }

    /// The result the callout wrote to `out`, as its kind.
    fn slow_result(&mut self, out: ir::Value, kind: Kind) -> ir::Value {
        let bits = self.load_word_at(out, 0);
        match kind {
            Kind::I32 => self.convert(Conversion::WrapI64, bits),
            Kind::I64 => bits,
        }
    }
}
