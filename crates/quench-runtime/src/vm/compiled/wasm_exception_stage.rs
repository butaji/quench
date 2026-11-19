//! The exception operations of the u-jit's Wasm functions (`Step::Exception`): an exception's
//! construction, a catch's tag test, and `throw_ref`, each a VM callout that runs the staged rules
//! (`staged::wasm_exception`) on the VM's heap, so the tiers share the checks and their traps.
//!
//! The callouts' ABI follows `wasm_gc_allocate`'s: the operands are the frame's Wasm bits, converted
//! at the callout's boundary (`operand_value`), and the status is 0 when the operation ran, else the
//! status a raised error leaves with (`finish_callout`), which `propagate` carries out.
//!
//! What is not lowered: a `payload` read. Its result's kind is the caught tag's parameter type,
//! which the bytecode does not name, so the analysis rejects it. A catch also reads a frame local,
//! which a function with a handler must have, and the u-jit admits no function with frame locals
//! (`Plan`'s `frame-locals` rejection); so a catch's function stays interpreted. A `throw_ref` is
//! lowered only where no handler covers it, as the function then has none.
//!
//! No frame roots anything across the construction: an allocation only makes a collection due, and
//! a collection runs at an interpreter safepoint, which the callout does not reach.

use super::wasm_gc_allocate::operand_value;
use super::wasm_instance_stage::value_kind;
use super::{Analysis, AnalysisState, CallContext, Emitter, Host, JsError, Kind, Slot, Value, Vm, finish_callout};
use crate::bytecode::{Function, Op, WideInstruction};
use crate::staged::machine::{MachineAccess as _, MachineFlow as _, MachineScalar as _, MachineStage as _};
use crate::staged::wasm::Width;
use crate::staged::wasm_exception::Exception;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::tag::ExceptionInput;
use cranelift_codegen::ir::{self, MemFlagsData};

/// The bytes of one word of an operand block or a result.
const WORD: usize = size_of::<u64>();

/// The value operands an exception construction reads: its payload registers of its window. The
/// tag is the window's first register, which holds the tag binding (`Slot::Binding`), not a value.
/// `None` for any other operation.
pub(super) fn reads(instruction: WideInstruction) -> Option<Vec<u16>> {
    if instruction.op() != Op::WasmExceptionNew {
        return None;
    }
    let window = instruction.register_window();
    Some((window.base + ExceptionInput::Payload as u16..window.base + window.count).collect())
}

/// The flag that `register` holds a reference (`Kind::Reference`) in `state`.
fn holds_reference(state: &AnalysisState, registers: u16, register: u16) -> Option<()> {
    (value_kind(state.get(register, registers)?)? == Kind::Reference).then_some(())
}

impl<H: Host> Analysis<'_, H> {
    /// Checks an exception step: the operands it reads hold the kinds it needs, and its result takes
    /// the kind it returns. A payload read has no lowering (see the module docs).
    pub(super) fn transfer_exception(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        op: Exception,
    ) -> Option<()> {
        self.callouts.then_some(())?;
        let registers = function.registers;
        match op {
            Exception::New => {
                let window = instruction.register_window();
                (window.count >= ExceptionInput::MIN_COUNT).then_some(())?;
                matches!(state.get(window.base, registers)?, Slot::Binding(_)).then_some(())?;
                for register in reads(instruction)? {
                    value_kind(state.get(register, registers)?)?;
                }
                state.set(instruction.a(), registers, Slot::Value(Kind::Reference))
            }
            Exception::Match => {
                holds_reference(state, registers, instruction.b())?;
                holds_reference(state, registers, instruction.c())?;
                state.set(instruction.a(), registers, Slot::Value(Kind::I32))
            }
            Exception::Payload => None,
            Exception::ThrowRef => {
                // A function with a handler is not lowered: the throw would have to be caught.
                function.handlers.is_empty().then_some(())?;
                holds_reference(state, registers, instruction.a())
            }
        }
    }
}

/// The exception construction: its tag is the instance binding at environment slot `tag_slot`, read
/// from the frame's environment (as `Vm::wasm_instance_step` reads it), and its payload the operand
/// block the emitted code spilled: `count` registers, their kinds and their Wasm bits, then the
/// result word `out`.
unsafe extern "C" fn exception_new_callout<H: Host>(
    context: *mut CallContext,
    tag_slot: u64,
    operands: *const u64,
    count: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let (env, count) = unsafe { ((*context).env, count as usize) };
    let vm = unsafe { &mut *(*context).vm.cast::<Vm<H>>() };
    // SAFETY: the emitted code spilled `count` kinds and words after the `count` registers.
    let payload: Vec<Value> = (0..count)
        .map(|index| unsafe {
            let (kind, bits) = (*operands.add(count + index), *operands.add(2 * count + index));
            operand_value(vm, kind, bits)
        })
        .collect();
    let result = match vm.wasm_binding(env, tag_slot as usize) {
        Some(tag) => vm.wasm_exception_new(tag, payload),
        None => Err(JsError::validation("invalid Wasm instance binding".into())),
    };
    match result {
        Ok(value) => {
            // SAFETY: the emitted code gives `out` a word of its own for this call.
            unsafe { *out = value.to_bits() };
            0
        }
        // SAFETY: compiled code passes the context of its call.
        Err(error) => finish_callout(unsafe { &mut *context }, Ok(Err(error))),
    }
}

/// The catch's test of `exception` against `tag`: the flag in `out`.
unsafe extern "C" fn exception_match_callout<H: Host>(
    context: *mut CallContext,
    exception: u64,
    tag: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let vm = unsafe { &mut *(*context).vm.cast::<Vm<H>>() };
    match vm.wasm_exception_matches(Value::from_bits(exception), Value::from_bits(tag)) {
        Ok(matches) => {
            // SAFETY: the emitted code gives `out` a word of its own for this call.
            unsafe { *out = u64::from(matches) };
            0
        }
        // SAFETY: compiled code passes the context of its call.
        Err(error) => finish_callout(unsafe { &mut *context }, Ok(Err(error))),
    }
}

/// `throw_ref` of `exception`: the error it raises is left in the context, and the status is the
/// raised one. It always returns that status.
unsafe extern "C" fn exception_throw_callout<H: Host>(context: *mut CallContext, exception: u64) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let vm = unsafe { &mut *(*context).vm.cast::<Vm<H>>() };
    let error = vm.wasm_throw_ref(Value::from_bits(exception));
    // SAFETY: compiled code passes the context of its call.
    finish_callout(unsafe { &mut *context }, Ok(Err(error)))
}

impl<H: Host> Emitter<'_, '_, H> {
    /// Lowers an exception step; whether control falls through to the next instruction.
    pub(super) fn exception_op(&mut self, pc: usize, instruction: WideInstruction, op: Exception) -> bool {
        match op {
            Exception::New => {
                self.exception_new(pc, instruction);
                true
            }
            Exception::Match => {
                self.exception_match(instruction);
                true
            }
            Exception::Payload => unreachable!("the analysis rejects a payload read"),
            Exception::ThrowRef => {
                self.exception_throw(instruction);
                false
            }
        }
    }

    /// The construction as `exception_new_callout`: each operand's register, kind and Wasm bits are
    /// spilled in three runs, then the result word, which is written to the result register.
    fn exception_new(&mut self, pc: usize, instruction: WideInstruction) {
        let plan = self.plan;
        let spilled = &plan.gc_reads[pc];
        let count = spilled.len();
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            ((3 * count + 1) * WORD) as u32,
            WORD.trailing_zeros() as u8,
        ));
        for (index, (register, kind)) in spilled.iter().enumerate() {
            let bits = match kind {
                Kind::I32 => {
                    let value = self.read(*register, Kind::I32);
                    self.zero_extend(value)
                }
                Kind::I64 | Kind::Reference => self.read(*register, *kind),
            };
            let kind_word = self.constant(Width::I64, *kind as i64);
            let register_word = self.constant(Width::I64, i64::from(*register));
            for (word, value) in [(index, register_word), (count + index, kind_word), (2 * count + index, bits)] {
                let address = self.slot_address(slot, (word * WORD) as i32);
                self.store_word(MemFlagsData::trusted(), value, address, 0);
            }
        }
        let Slot::Binding(tag_slot) = self.plan.operand(pc) else {
            unreachable!("the analysis admits a construction whose tag is a binding");
        };
        let context = self.body.core.context;
        let tag = self.constant(Width::I64, i64::from(tag_slot));
        let operands = self.slot_address(slot, 0);
        let count_word = self.constant(Width::I64, count as i64);
        let out = self.slot_address(slot, (3 * count * WORD) as i32);
        let target = exception_new_callout::<H> as *const () as usize;
        let status = self.call_leaf(target, &[context, tag, operands, count_word, out]);
        self.propagate(status);
        let bits = self.load_word(Width::I64, MemFlagsData::trusted(), out, 0);
        self.write(instruction.a(), Kind::Reference, bits);
    }

    /// The catch's test as `exception_match_callout`: the flag is written as an i32.
    fn exception_match(&mut self, instruction: WideInstruction) {
        let exception = self.read(instruction.b(), Kind::Reference);
        let tag = self.read(instruction.c(), Kind::Reference);
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            WORD as u32,
            WORD.trailing_zeros() as u8,
        ));
        let context = self.body.core.context;
        let out = self.slot_address(slot, 0);
        let target = exception_match_callout::<H> as *const () as usize;
        let status = self.call_leaf(target, &[context, exception, tag, out]);
        self.propagate(status);
        let bits = self.load_word(Width::I64, MemFlagsData::trusted(), out, 0);
        let flag = self.convert(Conversion::WrapI64, bits);
        self.write(instruction.a(), Kind::I32, flag);
    }

    /// `throw_ref`: the callout always raises, so its status leaves the activation directly, as
    /// `propagate`'s raised path does.
    fn exception_throw(&mut self, instruction: WideInstruction) {
        let exception = self.read(instruction.a(), Kind::Reference);
        let context = self.body.core.context;
        let target = exception_throw_callout::<H> as *const () as usize;
        let status = self.call_leaf(target, &[context, exception]);
        let mut values = vec![status];
        if let Some(ty) = self.plan.result {
            values.push(self.raw_const(Kind::of(ty).expect("planned").lane(), 0));
        }
        self.leave(&values);
    }
}
