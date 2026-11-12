//! Exceptions inside compiled code. A `throw`, and a call whose callee raised, that a handler
//! of the same function covers is the catch's jump in native code: the thrown value goes to the
//! handler's binding and control to the handler's block, which is what
//! `Vm::exception_handler_target` does for a thrown value, so the interpreter never re-executes
//! the `throw` to find the handler. One no handler of the function covers leaves the body as
//! `LANE_BODY_THREW`, the exception in `Vm::lane_raised`, which native callers unwind through
//! the same way and the lane raises where it resumes: no frame is made for the exception.
use super::*;
use crate::bytecode::{Function, Handler, ThrownValueSlot};
use super::super::lane_unit::{Origin, Unit};

/// Where a throw continues: the unit PC of its handler's block, and how the entry binds the
/// thrown value.
#[derive(Clone, Copy, Debug)]
pub(super) struct Catch {
    pub(super) target: usize,
    pub(super) bind: Bind,
}

/// What a handler's entry settles, which native code does as `Vm::exception_handler_target` does.
#[derive(Clone, Copy, Debug)]
pub(super) enum Bind {
    /// A catch that binds nothing.
    Nothing,
    /// A binding promoted to a register (`ThrownValueSlot::Register`).
    Register(Register),
    /// A frame local no environment owns, stored where the interpreter stores it.
    Local(u16),
    /// A local the frame's environment owns, which only the frame's own helper settles
    /// (`dispatch_fast::lane_handler_edge`): a captured frame gives it a fresh environment per
    /// entry.
    Frame,
}

/// The edges out of a native call: its helper path, its completion, and the callee's exception.
#[derive(Clone, Copy)]
pub(super) struct CallEdges {
    pub(super) helper: ir::Block,
    pub(super) completed: ir::Block,
    pub(super) raised: ir::Block,
}

/// Whether `unit` runs `pc` as the function's own record `pc`: the only record whose handler
/// table is the caller's, with its PCs the unit's.
fn is_own_record(unit: &Unit, pc: usize) -> bool {
    unit.origins.get(pc) == Some(&Origin::Caller(pc as u32))
}

/// Whether an exception of `function` unwinds through native code alone: a generator, an async
/// function and a dynamic scope have state only the interpreter's unwinding settles.
fn unwinds_natively(function: &Function) -> bool {
    !(function.is_generator || function.is_async || function.has_dynamic_scope_access())
}

/// Whether `instruction` raises an exception native code can route: a plain `throw` of a
/// register of the unit, or a plain call or construction, whose callee may throw.
fn raising_instruction(instruction: WideInstruction, registers: Register) -> bool {
    let class = instruction.op().class();
    instruction.result_flags() == 0 && if class.eq(Class::Throw) {
        instruction.register_a() < registers
    } else {
        class.eq(Class::Call) || class.eq(Class::Construct)
    }
}

/// The register and the frame local a handler's slot names, each none where it names the
/// other or nothing; `None` for a register outside the unit. A `finally` is served as a
/// catch-all: the thrown value goes to its error register, its exceptional copy of the
/// finalizer runs, and the `Throw` that ends it re-raises (`Lowering::throw`). Its
/// `return_target` is only where a generator resumes a `return`, which an exception never takes.
fn slot_parts(unit: &Unit, handler: Handler) -> Option<(Option<Register>, Option<u16>)> {
    match handler.slot {
        None => Some((None, None)),
        Some(ThrownValueSlot::Register(register)) => (register < unit.registers).then_some((Some(register), None)),
        Some(ThrownValueSlot::Local(slot)) => Some((None, Some(slot))),
    }
}

/// What admits a handler's entry into native code: the lowered function, its unit, and how its
/// run completes.
struct Admission<'a> {
    program: &'a ResidualProgram,
    function: &'a Function,
    unit: &'a Unit,
    completion: Completion,
}

impl Admission<'_> {
    /// Whether the function runs only in a VM frame, never as a frameless activation (a
    /// registers-only body, an environment window): the frame state a handler's entry
    /// settles exists only there.
    fn frame_bound(&self) -> bool {
        let plan = self.function.call_plan(self.program);
        !(plan.registers_only || plan.environment_activation)
    }

    /// Whether `handler`'s entry settles frame state beyond native stores: a local the frame's
    /// environment owns once captured, or a local a body cannot reach (only a loop region
    /// reads the activation's locals, `JsStepDispatch::native_locals`).
    fn settles_frame(&self, local: Option<u16>) -> bool {
        local.is_some_and(|slot| self.completion == Completion::Body || self.function.local_slot_uses_environment(usize::from(slot)))
    }

    /// How native code enters `handler`, `None` for one it cannot serve: frame state in a
    /// function that may run frameless.
    fn bind(&self, handler: Handler) -> Option<Bind> {
        // A `with` scope to unwind stays with the interpreter's unwinding.
        if handler.with_depth != 0 { return None; }
        let (register, local) = slot_parts(self.unit, handler)?;
        if self.settles_frame(local) {
            return self.frame_bound().then_some(Bind::Frame);
        }
        Some(match (register, local) {
            (Some(register), _) => Bind::Register(register),
            (None, Some(slot)) => Bind::Local(slot),
            (None, None) => Bind::Nothing,
        })
    }
}

/// The throws and calls of `unit`'s function whose exceptions native code catches itself, by
/// unit PC. One whose handler is not admitted (`Admission::bind`), lies outside the lowered
/// range, or is not before its handler is not caught natively: a throw leaves at itself, which
/// the interpreter raises exactly, and a call is made by the lane (`Lowering::raises_from`).
pub(super) fn catch_edges(program: &ResidualProgram, function: &Function, unit: &Unit, coverage: Coverage, completion: Completion) -> BTreeMap<usize, Catch> {
    let mut catches = BTreeMap::new();
    if !completion.status_and_pc() || !unwinds_natively(function) || !unit.frames.is_empty() || function.handlers.is_empty() {
        return catches;
    }
    let admission = Admission { program, function, unit, completion };
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if !raising_instruction(*instruction, unit.registers)
            || !coverage.lowered(pc) || !is_own_record(unit, pc) { continue; }
        let Some(handler) = function.handler_at(pc as u32) else { continue };
        let target = handler.target as usize;
        let Some(bind) = admission.bind(handler) else { continue };
        if target > pc && target < unit.instructions.len() && coverage.lowered(target) && is_own_record(unit, target) {
            catches.insert(pc, Catch { target, bind });
        }
    }
    catches
}

impl Lowering<'_, '_> {
    /// The edges of a call about to be made: its helper path, its completion, and the callee's
    /// exception, whose block (cold) `raised_call` fills.
    pub(super) fn call_edges(&mut self) -> CallEdges {
        let (helper, completed, raised) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(raised);
        CallEdges { helper, completed, raised }
    }

    /// The record an exception raised at unit PC `pc` leaves the lowered function at, where no
    /// native handler takes it. A body's callers unwind past it, so no handler of the function
    /// may cover it (the interpreter's unwinding would run it); a loop region's only caller is
    /// the lane, which finds the handler of the record it resumes at.
    fn leaves_at(&self, pc: usize) -> Option<u32> {
        let Some(Origin::Caller(record)) = self.unit.origins.get(pc).copied() else { return None };
        let function = self.program.function(self.function_id as usize);
        let admitted = match self.completion {
            Completion::StatusAndPc => true,
            Completion::Body => function.handler_at(record).is_none(),
            Completion::SourcePc | Completion::Number => false,
        };
        (admitted && unwinds_natively(function)).then_some(record)
    }

    /// Whether native code routes an exception raised at `pc`: a handler of its own catches it
    /// or the function raises it out (`LANE_BODY_THREW`). A call native code cannot route a
    /// callee's exception from is not made natively.
    pub(super) fn raises_from(&self, pc: usize) -> bool {
        self.catches.contains_key(&pc) || self.leaves_at(pc).is_some()
    }

    /// The `throw` at `pc`: a jump to its handler's block with the thrown value boxed once and
    /// bound as the handler's entry binds it, the edge conformed as any edge is; else, where no
    /// handler covers it, the function raises it out of native code (`LANE_BODY_THREW`) after
    /// the registers a handler of the lane's resumption reads are written back. `None` where
    /// it leaves at the throw for the interpreter to raise.
    pub(super) fn throw(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if !raising_instruction(instruction, self.unit.registers) { return None; }
        if let Some(Catch { target, bind }) = self.catches.get(&pc).copied() {
            let value = self.tagged(state, instruction.register_a());
            self.bind_caught(state, pc, bind, value);
            self.continue_at(state, target);
            return Some(false);
        }
        let record = self.leaves_at(pc)?;
        let value = self.tagged(state, instruction.register_a());
        self.canonicalize(state, pc);
        self.spill_written(pc);
        let site = self.ins().iconst(types::I64, super::super::raise_site(self.function_id, record) as i64);
        self.helper_call(LaneHelper::Raise, &[value, site]);
        self.stop_with(LANE_BODY_THREW, record, None);
        Some(false)
    }

    /// The `raised` edge of the call at unit PC `pc` (record `call_pc`), whose callee ended by
    /// an exception (`Vm::lane_raised`), the call's register untouched: the handler's block
    /// with the exception bound as `throw` binds it, the callee having changed any object; else
    /// the function raises it on (`LANE_BODY_THREW`).
    pub(super) fn raised_call(&mut self, state: &State, pc: usize, call_pc: u32, raised: ir::Block) {
        self.builder.switch_to_block(raised);
        let Some(Catch { target, bind }) = self.catches.get(&pc).copied() else {
            self.stop_with(LANE_BODY_THREW, call_pc, None);
            return;
        };
        let value = self.helper_call(LaneHelper::TakeRaised, &[]);
        let mut caught = state.clone();
        caught.call_effect();
        self.reload_property_arena();
        self.bind_caught(&mut caught, pc, bind, value);
        self.continue_at(&mut caught, target);
    }

    /// The entry of the handler that catches the exception `value` thrown at record `pc`, as
    /// `Vm::exception_handler_target` makes it: the value to its register or its frame local,
    /// or, where the frame holds state to settle, the frame's helper
    /// (`dispatch_fast::lane_handler_edge`), whose allocation may move the property arena.
    fn bind_caught(&mut self, state: &mut State, pc: usize, bind: Bind, value: ir::Value) {
        match bind {
            Bind::Nothing => {}
            Bind::Register(register) => self.set_tagged(state, register, value),
            Bind::Local(slot) => {
                let address = super::frame::local_slot_address(&mut self.builder, self.payload, slot);
                self.ins().store(MemFlagsData::trusted(), value, address, 0);
            }
            Bind::Frame => {
                let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
                let record = self.ins().iconst(types::I64, pc as i64);
                self.helper_call(LaneHelper::HandlerEdge, &[context, record, value]);
                self.allocated(state);
            }
        }
    }

    /// Continues at `raised` where a helper's `status` is `LANE_CALL_RAISED`, else in a fresh block.
    pub(super) fn branch_if_raised(&mut self, status: ir::Value, raised: ir::Block) {
        let rest = self.builder.create_block();
        let is_raised = self.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_RAISED as i64);
        self.ins().brif(is_raised, raised, &[], rest, &[]);
        self.builder.switch_to_block(rest);
    }

    /// Stops the run with `status`, and `value` as the result word where a run publishes one:
    /// `record` is the record that ends the activation (`JsStepDispatch::returned_pc`), which
    /// the lane's completion or raise resumes at.
    pub(super) fn stop_with(&mut self, status: u32, record: u32, value: Option<ir::Value>) {
        self.finishes = true;
        let record = self.ins().iconst(types::I64, i64::from(record));
        let payload = self.payload;
        self.ins().store(MemFlagsData::trusted(), record, payload,
            i32::try_from(std::mem::offset_of!(JsStepDispatch, returned_pc)).expect("dispatch offset fits i32"));
        let status = self.ins().iconst(types::I32, i64::from(status));
        self.leave_with(status, value);
    }
}
