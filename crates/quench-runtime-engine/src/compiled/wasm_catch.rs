//! The catch of a raise in the u-jit's Wasm functions: the handler each pc's raise enters
//! (`catch_frame`), the handler's entry state in the analysis (`raise_edge`), and the emitted catch
//! of a raised status (`Emitter::catch_raised`). The catch callout takes the error a call left
//! raised when a handler catches it, and binds its thrown value to the handler's frame local. A
//! trap, a link failure and a termination are never caught, as `Vm::exception_handler_target` does.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{AnalysisState, CallContext, Emitter, JsError, Kind, RAISED_STATUS, Raised, Slot, Vm};
use quench_runtime::bytecode::{Function, Handler, ThrownValueSlot};
use quench_runtime::staged::machine::{MachineAccess as _, MachineFlow as _, MachineScalar as _};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_instance::local_register;
use cranelift_codegen::ir::{self, MemFlagsData};

/// The catch a raise enters: the pc its thrown value goes to, and the register that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Catch {
    pub(crate) target: usize,
    pub(crate) local: u16,
}

impl Catch {
    /// The state a handler is entered in from `state`: its thrown value is a reference in its local.
    fn entry(self, mut state: AnalysisState, bound: u16) -> Option<AnalysisState> {
        state.set(self.local, bound, Slot::Value(Kind::Reference))?;
        Some(state)
    }
}

/// The catch of each pc of `function` and the frame's state bound (its registers and locals), or
/// `None` where a handler is not one a Wasm function lowers: a binding that is not a frame local, a
/// `finally` return, or a target past the code.
pub(crate) fn catch_frame(function: &Function) -> Option<(Vec<Option<Catch>>, u16)> {
    let bound = function.registers.checked_add(function.locals)?;
    let catches = (0..function.code.len())
        .map(|pc| match function.handler_at(u32::try_from(pc).ok()?) {
            None => Some(None),
            Some(handler) => catch_of(function, handler).map(Some),
        })
        .collect::<Option<Vec<_>>>()?;
    Some((catches, bound))
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

/// Joins the entry of a raise at a pc into its handler's state: the state `before` the raising
/// instruction, with the thrown value bound. Nothing where the pc has no catch.
pub(crate) fn raise_edge(
    states: &mut [Option<AnalysisState>],
    work: &mut Vec<usize>,
    catch: Option<Catch>,
    before: &AnalysisState,
    bound: u16,
) -> Option<()> {
    let Some(catch) = catch else {
        return Some(());
    };
    let incoming = catch.entry(before.clone(), bound)?;
    let changed = match &mut states[catch.target] {
        Some(existing) => existing.join_from(&incoming),
        target @ None => {
            *target = Some(incoming);
            true
        }
    };
    if changed {
        work.push(catch.target);
    }
    Some(())
}

/// Whether a handler catches `error`: a Wasm exception, or a JavaScript exception, which `catch_all`
/// and `WebAssembly.JSTag` catch. A trap, a link failure and a termination are never caught.
fn catches(error: &JsError) -> bool {
    !error.is_execution_terminated() && (error.wasm_exception().is_some() || error.is_javascript_exception())
}

/// The catch of a raised `status`: where a handler catches the error the call left raised, the error
/// leaves the context and its thrown value goes to `out`, and the status is 0. Otherwise `status`
/// returns unchanged and the error stays raised for the activation to leave with.
unsafe extern "C" fn exception_catch_callout(context: *mut CallContext, status: i64, out: *mut u64) -> i64 {
    // SAFETY: compiled code passes the context of its call, whose VM is suspended in it.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm>() };
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

impl Emitter<'_, '_> {
    /// Leaves the activation with the raised `status` and a zero result.
    pub(crate) fn leave_raised(&mut self, status: ir::Value) {
        let mut values = vec![status];
        if let Some(ty) = self.plan.result {
            values.push(self.raw_const(Kind::of(ty).expect("planned").lane(), 0));
        }
        self.leave(&values);
    }

    /// The catch of a raised `status` by `catch`: where the handler catches the error the call left
    /// raised, the error leaves the context and its thrown value binds to the handler's local, which
    /// control enters. Otherwise the status leaves the activation (`leave_raised`).
    pub(crate) fn catch_raised(&mut self, status: ir::Value, catch: Catch) {
        let slot = self.body.core.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            size_of::<u64>() as u32,
            size_of::<u64>().trailing_zeros() as u8,
        ));
        let context = self.body.core.context;
        let out = self.slot_address(slot, 0);
        let target = exception_catch_callout as *const () as usize;
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
