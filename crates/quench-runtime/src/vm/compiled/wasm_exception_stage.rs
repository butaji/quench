//! The exception operations of the u-jit's Wasm functions (`Step::Exception`): an exception's
//! construction, a catch's tag test, and `throw_ref`, each a VM callout that runs the staged rules
//! (`staged::wasm_exception`) on the VM's heap, so the tiers share the checks and their traps.
//!
//! The callouts' ABI follows `wasm_gc_allocate`'s: the operands are the frame's Wasm bits, converted
//! at the callout's boundary (`operand_value`), and the status is 0 when the operation ran, else the
//! status a raised error leaves with (`finish_callout`), which `propagate` carries out.
//!
//! A try region is compiled natively: a raised status at a pc a handler covers (`catch_sites`) takes
//! the handler's edge, not the activation's exit (`catch_raised`). The thrown value is the error the
//! call left raised, made by the catch callout (`catches`); a trap or a termination is never caught,
//! and a `throw_ref` raises through the same edge. The handler's entry state binds that value to
//! its frame local (`Catch::entry`). What is not lowered: a `payload` read. Its result's kind is the
//! caught tag's parameter type, which the bytecode does not name, so the analysis rejects it.
//!
//! No frame roots anything across the construction: an allocation only makes a collection due, and
//! a collection runs at an interpreter safepoint, which the callout does not reach.

use super::wasm_gc_allocate::operand_value;
use super::wasm_instance_stage::value_kind;
use super::{Analysis, AnalysisState, CallContext, Emitter, Host, JsError, Kind, RAISED_STATUS, Raised, Slot, Value, Vm, finish_callout};
use crate::bytecode::{Function, Handler, ThrownValueSlot, WideInstruction};
use crate::staged::machine::{MachineAccess as _, MachineFlow as _, MachineScalar as _, MachineStage as _};
use crate::staged::wasm::Width;
use crate::staged::wasm_exception::Exception;
use crate::staged::wasm_instance::local_register;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::tag::ExceptionInput;
use cranelift_codegen::ir::{self, MemFlagsData};

/// The bytes of one word of an operand block or a result.
const WORD: usize = size_of::<u64>();

/// The value operands an exception construction reads: its payload registers of its window. The
/// tag is the window's first register, which holds the tag binding (`Slot::Binding`), not a value.
/// `None` for any other operation.
pub(super) fn reads(instruction: WideInstruction) -> Option<Vec<u16>> {
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
            Exception::ThrowRef => holds_reference(state, registers, instruction.a()),
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

/// The catch a handler makes: the pc its thrown value goes to, and the register that holds it (the
/// frame local the handler binds, `Function::handler_at`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Catch {
    pub(super) target: usize,
    pub(super) local: u16,
}

impl Catch {
    /// The state a handler is entered in from `state`, which holds the thrown value as a reference
    /// in its local (`bound` is the frame's registers and locals).
    pub(super) fn entry(self, mut state: AnalysisState, bound: u16) -> Option<AnalysisState> {
        state.set(self.local, bound, Slot::Value(Kind::Reference))?;
        Some(state)
    }
}

/// The catch of each pc of `function`, or `None` where a handler is not one a Wasm function lowers:
/// a binding that is not a frame local, a `finally` return, or a target past the code.
pub(super) fn catch_sites(function: &Function) -> Option<Vec<Option<Catch>>> {
    (0..function.code.len())
        .map(|pc| match function.handler_at(u32::try_from(pc).ok()?) {
            None => Some(None),
            Some(handler) => catch_of(function, handler).map(Some),
        })
        .collect()
}

/// The catch of a handler of a Wasm function.
fn catch_of(function: &Function, handler: Handler) -> Option<Catch> {
    let Some(ThrownValueSlot::Local(slot)) = handler.slot else {
        return None;
    };
    let target = usize::try_from(handler.target).ok().filter(|target| *target < function.code.len())?;
    (handler.return_target.is_none() && handler.with_depth == 0).then_some(())?;
    Some(Catch { target, local: local_register(function.registers, u32::from(slot)) })
}

/// Whether a handler catches `error`: a Wasm exception, or a JavaScript exception, which `catch_all`
/// and `WebAssembly.JSTag` catch. A trap, a link failure and a termination are never caught.
fn catches(error: &JsError) -> bool {
    !error.is_execution_terminated() && (error.wasm_exception().is_some() || error.is_javascript_exception())
}

/// The catch of a raised `status` (`exception_catch_callout`'s contract, see `Emitter::catch_raised`):
/// the error a handler catches leaves the context and its thrown value goes to `out`, and the
/// status is 0. Otherwise `status` returns unchanged and the error stays raised.
unsafe extern "C" fn exception_catch_callout<H: Host>(context: *mut CallContext, status: i64, out: *mut u64) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    let caught = status == RAISED_STATUS && matches!(&context.raised, Some(Raised::Error(error)) if catches(error));
    if !caught {
        return status;
    }
    let Some(program) = vm.programs.get(context.program) else {
        return status;
    };
    let Some(Raised::Error(error)) = context.raised.take() else {
        return status;
    };
    let value = vm.thrown_value_for(&program, error);
    // SAFETY: the emitted code gives `out` a word of its own for this call.
    unsafe { *out = value.to_bits() };
    0
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

    /// `throw_ref`: the callout always raises, so its status takes the raised path: a covering
    /// handler catches it, else the activation leaves with it.
    fn exception_throw(&mut self, instruction: WideInstruction) {
        let exception = self.read(instruction.a(), Kind::Reference);
        let context = self.body.core.context;
        let target = exception_throw_callout::<H> as *const () as usize;
        let status = self.call_leaf(target, &[context, exception]);
        self.propagate(status);
        self.leave_raised(status);
    }

    /// The catch of a raised `status` by `catch`: where the handler catches the error the call left
    /// raised, the error leaves the context and its thrown value binds to the handler's local, which
    /// control enters. Otherwise the status leaves the activation (`leave_raised`).
    pub(super) fn catch_raised(&mut self, status: ir::Value, catch: Catch) {
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            WORD as u32,
            WORD.trailing_zeros() as u8,
        ));
        let context = self.body.core.context;
        let out = self.slot_address(slot, 0);
        let target = exception_catch_callout::<H> as *const () as usize;
        let caught = self.call_leaf(target, &[context, status, out]);
        let uncaught = self.body.core.builder.create_block();
        let handled = self.body.core.builder.create_block();
        self.branch_if(caught, (uncaught, &[]), (handled, &[]));
        self.body.core.builder.switch_to_block(uncaught);
        self.body.core.builder.seal_block(uncaught);
        self.leave_raised(caught);
        self.body.core.builder.switch_to_block(handled);
        self.body.core.builder.seal_block(handled);
        let bits = self.load_word(Width::I64, MemFlagsData::trusted(), out, 0);
        self.write(catch.local, Kind::Reference, bits);
        self.jump(catch.target, false);
    }
}
