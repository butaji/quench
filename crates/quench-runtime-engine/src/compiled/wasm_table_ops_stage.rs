//! The Wasm table, element-segment and data-segment operations of the u-jit, and the one
//! semantics they share with the interpreter. A table or segment operand is a binding
//! (`Slot::Binding`): the callout receives its environment slot, never a value. Every index,
//! length and delta is a 32-bit word, zero-extended to the callout; a reference is its bits, an
//! `I64` word, as `reference_callout` passes them. Each operation is one VM callout that runs
//! `Vm::wasm_table_op`, the operation the interpreter's arms run through `Vm::wasm_table_step`,
//! so the two tiers share the rule (`staged::wasm_table_ops`).

use super::wasm_instance_stage::value_kind;
use super::{Analysis, AnalysisState, CallContext, Emitter, Kind, Slot, Vm, finish_callout};
use quench_runtime::bytecode::{Function, WideInstruction};
use quench_runtime::staged::machine::{MachineAccess, MachineStage};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_table_ops::TableOp;
use quench_runtime::wasm::conversion::ScalarConversionOperator as Conversion;
use cranelift_codegen::ir::{self, MemFlagsData};

/// The bytes of one word of the callout's result.
const WORD: u32 = size_of::<u64>() as u32;

/// The environment slot of a binding word `slot` names, or the undefined value when the
/// environment holds no binding there (which the analysis never admits).
fn bound(vm: &Vm, environment: super::Value, slot: u64) -> super::Value {
    vm.heap
        .environment_slot(environment, slot as usize)
        .unwrap_or(super::Value::UNDEFINED)
}

/// The VM's table or segment operation of `request`: the operation's tag in the low byte, its
/// first binding's environment slot in bits 8..24 and its second's in bits 24..40 (or the
/// segment a drop names, in the first). The words `a`, `b` and `c` are the operation's
/// `TableOp::words`, `value` its reference (`TableOp::value`), and its result word is written
/// to `out`. A trap raised by the operation propagates through `finish_callout`.
pub(crate) unsafe extern "C" fn table_op_callout(
    context: *mut CallContext,
    request: u64,
    a: u64,
    b: u64,
    c: u64,
    value: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm>() };
    let environment = context.env;
    let memory = context.memory;
    let first = (request >> 8) & 0xffff;
    let second = (request >> 24) & 0xffff;
    let result: Result<u64, super::JsError> = match TableOp::from_tag(request & 0xff) {
        None => Err(super::JsError::validation("invalid Wasm table operation".into())),
        Some(TableOp::SegmentDrop) => {
            vm.wasm_segment_drop(environment, first as usize);
            Ok(0)
        }
        Some(op) => {
            let tables = match op {
                TableOp::MemoryInit => [memory, bound(vm, environment, first)],
                TableOp::Copy | TableOp::Init => {
                    [bound(vm, environment, first), bound(vm, environment, second)]
                }
                _ => [bound(vm, environment, first), super::Value::UNDEFINED],
            };
            vm.wasm_table_op(op, tables, [a, b, c], super::Value::from_bits(value))
        }
    };
    match result {
        Ok(word) => {
            // SAFETY: the emitted code gives `out` a stack word of its own for this call.
            unsafe { *out = word };
            0
        }
        Err(error) => finish_callout(context, Ok(Err(error))),
    }
}

#[quench_macros::ext(CompiledWasmTableOpsStageVm)]
impl Vm {
    /// The operation `op` over decoded operands: the one semantics the interpreter and the u-jit's
    /// callout both run. `tables` are the table (or, for `memory.init`, the memory) and the
    /// second table or segment, `words` the operation's `TableOp::words` (zero-extended indices),
    /// `value` its reference. Returns the result word: a get's reference bits, a size, a grow's
    /// size before (`u64::MAX` where it cannot grow), else zero. Checks run before any write.
    pub(crate) fn wasm_table_op(
        &mut self,
        op: TableOp,
        tables: [super::Value; 2],
        words: [u64; 3],
        value: super::Value,
    ) -> Result<u64, super::JsError> {
        let [table, source] = tables;
        let [first, second, third] = words;
        Ok(match op {
            TableOp::Size => self.wasm_table_elements(table)?.len() as u64,
            TableOp::Get => self.wasm_table_get(table, first)?.to_bits(),
            TableOp::Set => {
                self.wasm_table_fill(table, first, value, 1)?;
                0
            }
            TableOp::Grow => self.wasm_table_grow(table, value, first)?.unwrap_or(u64::MAX),
            TableOp::Fill => {
                self.wasm_table_fill(table, first, value, second)?;
                0
            }
            TableOp::Copy => {
                self.wasm_table_copy(table, source, first, second, third)?;
                0
            }
            TableOp::Init => {
                self.wasm_table_init(table, source, first, second, third)?;
                0
            }
            TableOp::MemoryInit => {
                let source = (source != super::Value::UNDEFINED).then_some(source);
                self.wasm_copy_bytes(table, source, first, second, third)?;
                0
            }
            TableOp::SegmentDrop => 0,
        })
    }

    /// Drops the segment bound at environment slot `slot` of `environment`: the binding holds the
    /// undefined value afterwards, which `memory.init` reads as a dropped segment.
    pub(crate) fn wasm_segment_drop(&mut self, environment: super::Value, slot: usize) {
        if let Some(bound) = self.heap.environment_slot_mut(environment, slot) {
            *bound = super::Value::UNDEFINED;
        }
    }

    /// The interpreter's table or segment instruction `i` of frame `f`: its operands decoded as
    /// the staged rule names them (`TableOp::bindings`, `words`, `value`), then `wasm_table_op`,
    /// its result written to the result register. An index is converted by the type of the table
    /// it indexes, as the Wasm operation names it.
    pub(crate) fn wasm_table_step(
        &mut self,
        f: usize,
        i: WideInstruction,
    ) -> Result<(), super::JsError> {
        let Some(op) = TableOp::of(i.op()) else {
            return Err(super::JsError::validation("invalid Wasm table operation".into()));
        };
        if op == TableOp::SegmentDrop {
            let environment = self.frames[f].env;
            self.wasm_segment_drop(environment, i.imm() as usize);
            return Ok(());
        }
        let register = |register: Option<u16>| register.unwrap_or(0);
        let (first_register, second_register) = op.bindings(i);
        let first = self.read(f, register(first_register));
        let source = match second_register {
            Some(second) => self.read(f, second),
            None => super::Value::UNDEFINED,
        };
        // `memory.init` writes the memory in register `a` from the segment its binding names
        // (`b`): the operation takes them as (memory, segment), as the callout passes them.
        let (first, source) = if op == TableOp::MemoryInit { (self.read(f, i.a()), first) } else { (first, source) };
        let [w0, w1, w2] = op.words(i).map(|word| word.map(|word| self.read(f, word)));
        let value = op.value(i).map_or(super::Value::UNDEFINED, |value| self.read(f, value));
        let words: [u64; 3] = match op {
            TableOp::Size | TableOp::SegmentDrop => [0, 0, 0],
            TableOp::Get | TableOp::Set => {
                let index = self.wasm_table_index(first, w0.unwrap_or(first))?;
                [index, 0, 0]
            }
            TableOp::Grow => [self.wasm_table_index(first, w0.unwrap_or(first))?, 0, 0],
            TableOp::Fill => {
                let index = self.wasm_table_index(first, w0.unwrap_or(first))?;
                let length_type = self.wasm_table_index_type(first)?;
                let length = self.wasm_index_operand(w1.unwrap_or(first), length_type)?;
                [index, length, 0]
            }
            TableOp::Copy => {
                let destination = self.wasm_table_index(first, w0.unwrap_or(first))?;
                let source_index = self.wasm_table_index(source, w1.unwrap_or(first))?;
                let length_type = match (
                    self.wasm_table_index_type(first)?,
                    self.wasm_table_index_type(source)?,
                ) {
                    (quench_runtime::WasmType::I64, quench_runtime::WasmType::I64) => quench_runtime::WasmType::I64,
                    _ => quench_runtime::WasmType::I32,
                };
                let length = self.wasm_index_operand(w2.unwrap_or(first), length_type)?;
                [destination, source_index, length]
            }
            TableOp::Init => {
                let output = self.wasm_table_index(first, w0.unwrap_or(first))?;
                let input = self.wasm_index_operand(w1.unwrap_or(first), quench_runtime::WasmType::I32)?;
                let length = self.wasm_index_operand(w2.unwrap_or(first), quench_runtime::WasmType::I32)?;
                [output, input, length]
            }
            TableOp::MemoryInit => {
                let output = self.wasm_memory_index(first, w0.unwrap_or(first))?;
                let input = self.wasm_index_operand(w1.unwrap_or(first), quench_runtime::WasmType::I32)?;
                let length = self.wasm_index_operand(w2.unwrap_or(first), quench_runtime::WasmType::I32)?;
                [output, input, length]
            }
        };
        let word = self.wasm_table_op(op, [first, source], words, value)?;
        match op {
            TableOp::Get => self.write(f, i.result_register(), super::Value::from_bits(word)),
            TableOp::Size => {
                let size = self.encode_wasm_table_size(first, Some(word))?;
                self.write(f, i.result_register(), size);
            }
            TableOp::Grow => {
                let size = self.encode_wasm_table_size(first, (word != u64::MAX).then_some(word))?;
                self.write(f, i.result_register(), size);
            }
            _ => {}
        }
        Ok(())
    }
}

impl Analysis<'_> {
    /// Checks a table or segment step: its table is a 32-bit table binding and its segment a
    /// binding, its words are `I32`, its reference an `I64` word, and `memory.init`'s memory is
    /// the function's memory register, which the step reads (`Step::reads_memory`). A step is
    /// admitted only with callouts, which every operation makes.
    pub(crate) fn transfer_table(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        op: TableOp,
        memory: (Option<u16>, Option<u32>),
    ) -> Option<()> {
        self.callouts.then_some(())?;
        let registers = function.registers;
        let (_, memory_slot) = memory;
        // A binding register's environment slot, which a callout can name.
        let binding = |state: &AnalysisState, register: Option<u16>| -> Option<u32> {
            match state.get(register?, registers)? {
                Slot::Binding(slot) if slot <= u32::from(u16::MAX) => Some(slot),
                _ => None,
            }
        };
        let table = |state: &AnalysisState, register: Option<u16>| -> Option<()> {
            let slot = binding(state, register)?;
            self.binds_table32(slot).then_some(())
        };
        let word = |state: &AnalysisState, register: Option<u16>| -> Option<()> {
            (value_kind(state.get(register?, registers)?)? == Kind::I32).then_some(())
        };
        let reference = |state: &AnalysisState, register: Option<u16>| -> Option<()> {
            (value_kind(state.get(register?, registers)?)? == Kind::Reference).then_some(())
        };
        let (first, second) = op.bindings(instruction);
        let words = op.words(instruction);
        match op {
            TableOp::Size => {
                table(state, first)?;
                state.set(instruction.a(), registers, Slot::Value(Kind::I32))
            }
            TableOp::Get => {
                table(state, first)?;
                word(state, words[0])?;
                state.set(instruction.a(), registers, Slot::Value(Kind::Reference))
            }
            TableOp::Set => {
                table(state, first)?;
                word(state, words[0])?;
                reference(state, op.value(instruction))
            }
            TableOp::Grow => {
                table(state, first)?;
                word(state, words[0])?;
                reference(state, op.value(instruction))?;
                state.set(instruction.a(), registers, Slot::Value(Kind::I32))
            }
            TableOp::Fill => {
                table(state, first)?;
                reference(state, op.value(instruction))?;
                word(state, words[0])?;
                word(state, words[1])
            }
            TableOp::Copy => {
                table(state, first)?;
                table(state, second)?;
                words.into_iter().try_for_each(|register| word(state, register))
            }
            TableOp::Init => {
                table(state, first)?;
                binding(state, second)?;
                words.into_iter().try_for_each(|register| word(state, register))
            }
            TableOp::MemoryInit => {
                (state.get(instruction.a(), registers) == Some(Slot::Memory)).then_some(())?;
                memory_slot?;
                binding(state, first)?;
                words.into_iter().try_for_each(|register| word(state, register))
            }
            TableOp::SegmentDrop => Some(()),
        }
    }
}

impl Emitter<'_, '_> {
    /// A 32-bit word operand, zero-extended; an absent one is zero.
    fn table_word(&mut self, register: Option<u16>) -> ir::Value {
        match register {
            Some(register) => {
                let value = self.read(register, Kind::I32);
                self.zero_extend(value)
            }
            None => self.constant(Width::I64, 0),
        }
    }

    /// Lowers a table or segment step as the VM callout `table_op_callout`, whose status
    /// propagates. The step's result (a get's reference, a size or a grow) is read back from the
    /// callout's out word.
    pub(crate) fn table_op(&mut self, pc: usize, instruction: WideInstruction, op: TableOp) {
        let first = match op {
            TableOp::SegmentDrop => instruction.imm() as u64,
            _ => match self.plan.operand(pc) {
                Slot::Binding(slot) => u64::from(slot),
                _ => unreachable!("planned table operations name a binding"),
            },
        };
        let second = match self.plan.seconds[pc] {
            Some(Slot::Binding(slot)) => u64::from(slot),
            _ => 0,
        };
        let request = op.tag() | (first << 8) | (second << 24);
        let [w0, w1, w2] = op.words(instruction);
        let a = self.table_word(w0);
        let b = self.table_word(w1);
        let c = self.table_word(w2);
        let value = match op.value(instruction) {
            Some(register) => self.read(register, Kind::Reference),
            None => self.constant(Width::I64, 0),
        };
        let context = self.body.core.context;
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            WORD,
            WORD.trailing_zeros() as u8,
        ));
        let out = self.slot_address(slot, 0);
        let request = self.constant(Width::I64, request as i64);
        let target = table_op_callout as *const () as usize;
        // The callout runs the table's operation, which may run JS (a grow's or fill's
        // conversions), so the frame's references are held across it.
        let roots = self.root();
        let status = self.call_leaf(target, &[context, request, a, b, c, value, out]);
        self.unroot(roots);
        self.propagate(status);
        let result = self.load_word(Width::I64, MemFlagsData::trusted(), out, 0);
        match op {
            TableOp::Get => self.write(instruction.a(), Kind::Reference, result),
            TableOp::Size | TableOp::Grow => {
                let wrapped = self.convert(Conversion::WrapI64, result);
                self.write(instruction.a(), Kind::I32, wrapped);
            }
            _ => {}
        }
    }
}
