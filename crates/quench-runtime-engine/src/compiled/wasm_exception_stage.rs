//! The exception operations of the u-jit's Wasm functions (`Step::Exception`): an exception's
//! construction, a catch's tag test, and `throw_ref`, each a VM callout that runs the staged rules
//! (`staged::wasm_exception`) on the VM's heap, so the tiers share the checks and their traps.
//!
//! The callouts' ABI follows `wasm_gc_allocate`'s: the operands are the frame's Wasm bits, converted
//! at the callout's boundary (`operand_value`), and the status is 0 when the operation ran, else the
//! status a raised error leaves with (`finish_callout`), which `propagate` carries out.
//!
//! A raised status at a pc a handler covers takes the handler's edge (`wasm_catch`), not the
//! activation's exit: a throw, and a call whose callee raised. A payload read carries its payload's
//! type in its instruction (`PayloadType`), so its result's kind is known to the analysis.
//!
//! No frame roots anything across the construction: an allocation only makes a collection due, and
//! a collection runs at an interpreter safepoint, which the callout does not reach.
// Raw-pointer field access predates `dangerous_implicit_autorefs`; the sites are reads and
// writes of `Copy` fields of a live record, and converting them to `addr_of!` is its own
// change (TOD-1202). Nothing here is a new allow: the lint did not exist when they landed.
#![allow(dangerous_implicit_autorefs)]

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::wasm_gc_allocate::operand_value;
use super::wasm_instance_stage::value_kind;
use super::{Analysis, AnalysisState, CallContext, Emitter, JsError, Kind, Slot, Value, Vm, finish_callout};
use quench_runtime::bytecode::{Function, WideInstruction};
use quench_runtime::staged::machine::{MachineAccess as _, MachineStage as _};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_exception::{Exception, PayloadType};
use quench_runtime::wasm::conversion::ScalarConversionOperator as Conversion;
use quench_runtime::wasm::tag::ExceptionInput;
use cranelift_codegen::ir::{self, MemFlagsData};

/// The bytes of one word of an operand block or a result.
const WORD: usize = size_of::<u64>();

/// The value operands an exception construction reads: its payload registers of its window. The
/// tag is the window's first register, which holds the tag binding (`Slot::Binding`), not a value.
/// `None` for any other operation.
pub(crate) fn reads(instruction: WideInstruction) -> Option<Vec<u16>> {
    if Exception::of(instruction.op()) != Some(Exception::New) {
        return None;
    }
    let window = instruction.register_window();
    Some((window.base + ExceptionInput::Payload as u16..window.base + window.count).collect())
}

/// The flag that `register` holds a reference (`Kind::Reference`) in `state`.
fn holds_reference(state: &AnalysisState, registers: u16, register: u16) -> Option<()> {
    (value_kind(state.get(register, registers)?)? == Kind::Reference).then_some(())
}

impl Analysis<'_> {
    /// Checks an exception step: the operands it reads hold the kinds it needs, and its result takes
    /// the kind it returns. A payload read has no lowering (see the module docs).
    pub(crate) fn transfer_exception(
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
            Exception::Payload => {
                holds_reference(state, registers, instruction.b())?;
                let ty = PayloadType::of_code(instruction.c())?;
                state.set(instruction.a(), registers, Slot::Value(payload_kind(ty)))
            }
            Exception::ThrowRef => holds_reference(state, registers, instruction.a()),
        }
    }
}

/// The exception construction: its tag is the instance binding at environment slot `tag_slot`, read
/// from the frame's environment (as `Vm::wasm_instance_step` reads it), and its payload the operand
/// block the emitted code spilled: `count` registers, their kinds and their Wasm bits, then the
/// result word `out`.
unsafe extern "C" fn exception_new_callout(
    context: *mut CallContext,
    tag_slot: u64,
    operands: *const u64,
    count: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let (env, count) = unsafe { ((*context).head.env, count as usize) };
    let vm = unsafe { &mut *(*context).head.vm.cast::<Vm>() };
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
unsafe extern "C" fn exception_match_callout(
    context: *mut CallContext,
    exception: u64,
    tag: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let vm = unsafe { &mut *(*context).head.vm.cast::<Vm>() };
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

/// The kind a payload of type `ty` holds in a register: a 32-bit or a 64-bit word, or a reference.
fn payload_kind(ty: PayloadType) -> Kind {
    match ty {
        PayloadType::I32 | PayloadType::F32 => Kind::I32,
        PayloadType::I64 | PayloadType::F64 => Kind::I64,
        PayloadType::Reference => Kind::Reference,
    }
}

/// The word of payload `index` of `exception` for a payload of type `code`: a numeric payload's
/// Wasm bits (decoded from its Value), a reference's Value bits.
fn payload_word(vm: &mut Vm, exception: u64, index: u64, code: u64) -> Result<u64, JsError> {
    let ty = u16::try_from(code).ok().and_then(PayloadType::of_code).ok_or_else(|| JsError::validation("invalid Wasm payload type".into()))?;
    let value = vm.wasm_exception_payload(Value::from_bits(exception), index as u32)?;
    let Some(numeric) = ty.numeric() else {
        return Ok(value.to_bits());
    };
    Ok(match vm.decode_wasm_value(value, numeric)?.bits() {
        Some(quench_runtime::wasm::ScalarBits::Bits32(bits)) => u64::from(bits),
        Some(quench_runtime::wasm::ScalarBits::Bits64(bits)) => bits,
        _ => return Err(JsError::validation("non-scalar Wasm payload".into())),
    })
}

/// The payload read of `exception` at `index` for a payload of type `code`: its word in `out`, or
/// the status a raised error leaves with (`finish_callout`).
unsafe extern "C" fn exception_payload_callout(
    context: *mut CallContext,
    exception: u64,
    index: u64,
    code: u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let vm = unsafe { &mut *(*context).head.vm.cast::<Vm>() };
    match payload_word(vm, exception, index, code) {
        Ok(word) => {
            // SAFETY: the emitted code gives `out` a word of its own for this call.
            unsafe { *out = word };
            0
        }
        // SAFETY: compiled code passes the context of its call.
        Err(error) => finish_callout(unsafe { &mut *context }, Ok(Err(error))),
    }
}

/// `throw_ref` of `exception`: the error it raises is left in the context, and the status is the
/// raised one. It always returns that status.
unsafe extern "C" fn exception_throw_callout(context: *mut CallContext, exception: u64) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let vm = unsafe { &mut *(*context).head.vm.cast::<Vm>() };
    let error = vm.wasm_throw_ref(Value::from_bits(exception));
    // SAFETY: compiled code passes the context of its call.
    finish_callout(unsafe { &mut *context }, Ok(Err(error)))
}

impl Emitter<'_, '_> {
    /// Lowers an exception step; whether control falls through to the next instruction.
    pub(crate) fn exception_op(&mut self, pc: usize, instruction: WideInstruction, op: Exception) -> bool {
        match op {
            Exception::New => {
                self.exception_new(pc, instruction);
                true
            }
            Exception::Match => {
                self.exception_match(instruction);
                true
            }
            Exception::Payload => {
                self.exception_payload(instruction);
                true
            }
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
        let target = exception_new_callout as *const () as usize;
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
        let target = exception_match_callout as *const () as usize;
        let status = self.call_leaf(target, &[context, exception, tag, out]);
        self.propagate(status);
        let bits = self.load_word(Width::I64, MemFlagsData::trusted(), out, 0);
        let flag = self.convert(Conversion::WrapI64, bits);
        self.write(instruction.a(), Kind::I32, flag);
    }

    /// The payload read as `exception_payload_callout`: the word the payload holds, written with the
    /// kind of its type, through the raised path like any call.
    fn exception_payload(&mut self, instruction: WideInstruction) {
        let exception = self.read(instruction.b(), Kind::Reference);
        let index = self.constant(Width::I64, i64::from(instruction.imm()));
        let code = self.constant(Width::I64, i64::from(instruction.c()));
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            WORD as u32,
            WORD.trailing_zeros() as u8,
        ));
        let context = self.body.core.context;
        let out = self.slot_address(slot, 0);
        let target = exception_payload_callout as *const () as usize;
        let status = self.call_leaf(target, &[context, exception, index, code, out]);
        self.propagate(status);
        let kind = payload_kind(PayloadType::of_code(instruction.c()).expect("planned"));
        let bits = self.load_word(Width::I64, MemFlagsData::trusted(), out, 0);
        let value = match kind {
            Kind::I32 => self.convert(Conversion::WrapI64, bits),
            Kind::I64 | Kind::Reference => bits,
        };
        self.write(instruction.a(), kind, value);
    }

    /// `throw_ref`: the callout always raises, so its status takes the raised path: a covering
    /// handler catches it, else the activation leaves with it.
    fn exception_throw(&mut self, instruction: WideInstruction) {
        let exception = self.read(instruction.a(), Kind::Reference);
        let context = self.body.core.context;
        let target = exception_throw_callout as *const () as usize;
        let status = self.call_leaf(target, &[context, exception]);
        self.propagate(status);
        self.leave_raised(status);
    }
}
