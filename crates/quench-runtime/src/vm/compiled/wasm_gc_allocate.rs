//! The allocating GC operations of the u-jit: `struct.new*`, `array.new*`, `array.init_*`,
//! `array.fill`, `array.copy`, `ref.i31`, the extern conversions and the descriptor operations.
//! Each is a callout that runs the interpreter's own step for the operation
//! (`Vm::wasm_gc_value_step`), over the operands converted here from the frame's Wasm bits to the
//! Values the step reads, and returns the result's Wasm bits. The tiers share the rule and its
//! checks and traps.
//!
//! No frame roots anything across these callouts. An allocation only makes a collection due, and
//! a collection runs at an interpreter safepoint; a callout runs no interpreter code, so none
//! runs before the frame's next call that can re-enter the VM, and that call roots the frame's
//! references (`wasm_roots`).

use super::wasm_instance_stage::value_kind;
use super::{Analysis, AnalysisState, CallContext, Emitter, Host, JsError, Kind, ProgramId, Slot, Value, Vm, finish_callout};
use crate::bytecode::{Function, Op, WideInstruction};
use crate::staged::machine::{MachineAccess, MachineStage};
use crate::staged::wasm::Width;
use crate::staged::wasm_gc::{ArrayConstruction, GcStep};
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::gc::ArraySegmentInput;
use cranelift_codegen::ir::{self, MemFlagsData};
use crate::staged::frame::WASM_HEADER_WORDS;

/// The bytes of one word of an operand block or a result.
const WORD: usize = size_of::<u64>();

/// The bits of one field of a packed instruction: its opcode and its three registers.
const FIELD_BITS: u32 = 16;

/// The operand registers an allocating step reads, as `Vm::wasm_gc_value_step` reads them: a
/// window's registers, a pair's, and the fixed operands. `None` for an operation that is not an
/// allocating step.
pub(super) fn allocating_reads(instruction: WideInstruction) -> Option<Vec<u16>> {
    let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
    Some(match GcStep::of(instruction.op())? {
        GcStep::ArrayNew { construction: ArrayConstruction::Repeated } => vec![b, c],
        GcStep::ArrayNew { construction: ArrayConstruction::Default } | GcStep::GetDescriptor | GcStep::I31 | GcStep::ExternConversion => vec![b],
        GcStep::Test { descriptor: true, .. } => vec![b, c],
        GcStep::StructNew { default: true, descriptor: false } => Vec::new(),
        GcStep::ArrayNew { construction: ArrayConstruction::Fixed } | GcStep::StructNew { .. } => {
            let window = instruction.register_window();
            (window.base..window.base + window.count).collect()
        }
        GcStep::ArraySegment { init: false, .. } => (b..b + ArraySegmentInput::COUNT).collect(),
        GcStep::ArrayFill => {
            let (output, _) = instruction.register_pair();
            vec![a, b, c, output]
        }
        GcStep::ArrayCopy | GcStep::ArraySegment { init: true, .. } => {
            let (output, input) = instruction.register_pair();
            vec![a, b, c, output, input]
        }
        _ => return None,
    })
}

/// The kind of the word an allocating step returns; `None` for a step that returns none.
pub(super) fn allocating_result(op: Op) -> Option<Kind> {
    match GcStep::of(op) {
        Some(GcStep::ArrayFill | GcStep::ArrayCopy | GcStep::ArraySegment { init: true, .. }) => None,
        Some(GcStep::Test { cast: false, descriptor: true }) => Some(Kind::I32),
        _ => Some(Kind::Reference),
    }
}

/// The instruction as one word (its opcode and registers, which fit in 16 bits each) and its
/// immediate, which the callout unpacks.
pub(super) fn pack(instruction: WideInstruction) -> i64 {
    let fields = [instruction.op() as u64, u64::from(instruction.a()), u64::from(instruction.b()), u64::from(instruction.c())];
    fields.iter().enumerate().fold(0, |packed, (index, field)| packed | (field << (index as u32 * FIELD_BITS))) as i64
}

/// The instruction `pack` and the immediate name.
pub(super) fn unpack(packed: u64, imm: u64) -> WideInstruction {
    let field = |index: u32| ((packed >> (index * FIELD_BITS)) & u64::from(u16::MAX)) as u16;
    let op = Op::from_index(usize::from(field(0))).expect("packed opcode");
    WideInstruction::new(op, field(1), field(2), field(3), imm as u32)
}

/// The Value of an operand the emitted code spilled: an i32 from its low word, an i64 boxed
/// where its payload does not fit, a reference as its Value bits.
pub(super) fn operand_value<H: Host>(vm: &mut Vm<H>, kind: u64, bits: u64) -> Value {
    if kind == Kind::I32 as u64 {
        Value::integer(bits as u32 as i32)
    } else if kind == Kind::I64 as u64 {
        vm.encode_bits64(bits)
    } else {
        Value::from_bits(bits)
    }
}

/// The allocating step `packed` (with immediate `imm`) over the operand block the emitted code
/// spilled: `count` registers, their kinds and their Wasm bits, then the result word `out`. The
/// step's result, if any, is its Wasm bits: a reference's Value bits, a descriptor test's flag.
unsafe extern "C" fn gc_allocate<H: Host>(
    context: *mut CallContext,
    packed: u64,
    imm: u64,
    operands: *const u64,
    count: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let (program, env) = unsafe { ((*context).head.program, (*context).head.env) };
    let instruction = unpack(packed, imm);
    let count = count as usize;
    // SAFETY: the emitted code spilled `count` registers, kinds and words, then `out`.
    let vm = unsafe { &mut *(*context).head.vm.cast::<Vm<H>>() };
    let values: Vec<(u16, Value)> = (0..count)
        .map(|index| unsafe {
            let (register, kind, bits) = (*operands.add(index), *operands.add(count + index), *operands.add(2 * count + index));
            (register as u16, operand_value(vm, kind, bits))
        })
        .collect();
    match allocating_bits(vm, program, env, instruction, &values) {
        Ok(bits) => {
            // SAFETY: the emitted code gives `out` a word of its own for this call.
            unsafe { *out = bits };
            0
        }
        // SAFETY: compiled code passes the context of its call.
        Err(error) => finish_callout(unsafe { &mut *context }, Ok(Err(error))),
    }
}

/// The interpreter's step of an allocating `instruction` over `operands` (each register's Value),
/// as its Wasm bits: a reference's Value bits, a descriptor test's flag, or 0 where the step has no
/// result. Both allocation callouts (the u-jit's `gc_allocate`, the b-jit's `allocate_frame`) run it.
pub(super) fn allocating_bits<H: Host>(
    vm: &mut Vm<H>,
    program: ProgramId,
    env: Value,
    instruction: WideInstruction,
    operands: &[(u16, Value)],
) -> Result<u64, JsError> {
    let read = |_: &Vm<H>, register: u16| {
        operands.iter().find(|(index, _)| *index == register).map_or(Value::UNDEFINED, |(_, value)| *value)
    };
    Ok(match vm.wasm_gc_value_step(program, env, instruction, read)? {
        None => 0,
        Some(value) if GcStep::of(instruction.op()) == Some(GcStep::Test { cast: false, descriptor: true }) => value.as_int().unwrap_or(0) as u32 as u64,
        Some(value) => value.to_bits(),
    })
}

/// The allocating step of a stitched frame's row (`quench-bjit/stencils/wasm_rows_callout.rs`,
/// `gc_allocate`): the operands are the frame's words, not spilled ones. `descriptor` names each
/// operand as a word of `register | kind << 16` (the kinds are `Kind as u64`), `count` of them, and
/// `meta` holds the immediate in its low 32 bits. The frame's registers are at the frame header's
/// offset (`WASM_HEADER_WORDS`), as the stitcher places them.
///
/// # Safety
/// `context` is the live context of the row's call, `frame` the stitched frame whose registers
/// the descriptor names, `descriptor` holds `count` words, and `out` is a word the row owns.
pub(super) unsafe extern "C" fn allocate_frame<H: Host>(
    context: *mut CallContext,
    packed: u64,
    meta: u64,
    frame: *const u64,
    descriptor: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let (program, env) = unsafe { ((*context).head.program, (*context).head.env) };
    let instruction = unpack(packed, meta & u64::from(u32::MAX));
    let count = ((meta >> 32) & 0xff) as usize;
    // SAFETY: as above; the VM outlives the call.
    let vm = unsafe { &mut *(*context).head.vm.cast::<Vm<H>>() };
    let operands: Vec<(u16, Value)> = (0..count)
        .map(|index| unsafe {
            let entry = *descriptor.add(index);
            let register = entry as u16;
            let bits = *frame.add(WASM_HEADER_WORDS + usize::from(register));
            (register, operand_value(vm, entry >> 16, bits))
        })
        .collect();
    match allocating_bits(vm, program, env, instruction, &operands) {
        Ok(bits) => {
            // SAFETY: the row gives `out` a word of its own for this call.
            unsafe { *out = bits };
            0
        }
        // SAFETY: compiled code passes the context of its call.
        Err(error) => finish_callout(unsafe { &mut *context }, Ok(Err(error))),
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Checks an allocating step: each operand it reads holds a value, and its result takes the
    /// kind the step returns.
    pub(super) fn transfer_allocating(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
    ) -> Option<()> {
        let registers = function.registers;
        for register in allocating_reads(instruction)? {
            value_kind(state.get(register, registers)?)?;
        }
        match allocating_result(instruction.op()) {
            Some(kind) => state.set(instruction.a(), registers, Slot::Value(kind)),
            None => Some(()),
        }
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// Stores the operand block of `reads` in `slot`: each register, its kind's code and its Wasm
    /// bits, one word each per operand, in three runs (`gc_allocate` unpacks them so).
    fn spill_operands(&mut self, slot: ir::StackSlot, reads: &[(u16, Kind)]) {
        let operand_count = reads.len();
        for (index, (register, kind)) in reads.iter().enumerate() {
            let bits = match kind {
                Kind::I32 => {
                    let value = self.read(*register, Kind::I32);
                    self.zero_extend(value)
                }
                Kind::I64 | Kind::Reference => self.read(*register, *kind),
            };
            let register_word = self.constant(Width::I64, i64::from(*register));
            let kind_word = self.constant(Width::I64, *kind as i64);
            for (word, value) in [(index, register_word), (operand_count + index, kind_word), (2 * operand_count + index, bits)] {
                let address = self.slot_address(slot, (word * WORD) as i32);
                self.store_word(MemFlagsData::trusted(), value, address, 0);
            }
        }
    }

    /// Lowers an allocating step as the callout `gc_allocate`, whose status propagates. Each read
    /// register is spilled with its kind (`Plan::gc_reads`), a reference or a number's Wasm bits.
    pub(super) fn gc_allocate(&mut self, pc: usize, instruction: WideInstruction) {
        let plan = self.plan;
        let reads = &plan.gc_reads[pc];
        let operand_count = reads.len();
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            ((3 * operand_count + 1) * WORD) as u32,
            WORD.trailing_zeros() as u8,
        ));
        self.spill_operands(slot, reads);
        let context = self.body.core.context;
        let packed = self.constant(Width::I64, pack(instruction));
        let imm = self.constant(Width::I64, i64::from(instruction.imm()));
        let operands = self.slot_address(slot, 0);
        let count = self.constant(Width::I64, operand_count as i64);
        let out = self.slot_address(slot, (3 * operand_count * WORD) as i32);
        let target = gc_allocate::<H> as *const () as usize;
        let status = self.call_leaf(target, &[context, packed, imm, operands, count, out]);
        self.propagate(status);
        if let Some(kind) = allocating_result(instruction.op()) {
            let bits = self.load_word(Width::I64, MemFlagsData::trusted(), out, 0);
            let value = match kind {
                Kind::I32 => self.convert(Conversion::WrapI64, bits),
                Kind::I64 | Kind::Reference => bits,
            };
            self.write(instruction.a(), kind, value);
        }
    }
}
