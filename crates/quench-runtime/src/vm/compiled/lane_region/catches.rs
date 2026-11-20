//! Exceptions inside compiled code. A `throw`, and a call whose callee raised, that a handler
//! of the same function covers is the catch's jump in native code: the thrown value goes to the
//! handler's binding and control to the handler's block, which is what
//! `Vm::exception_handler_target` does for a thrown value, so the interpreter never re-executes
//! the `throw` to find the handler. One no handler of the function covers leaves the body as
//! `LANE_BODY_THREW`, the exception in `Vm::lane_raised`, which native callers unwind through
//! the same way and the lane raises where it resumes: no frame is made for the exception.
use super::*;
use super::super::object_slots::emit;
use crate::staged::{Region, Stage};
use crate::bytecode::{Function, Handler, ThrownValueSlot};
use super::super::lane_unit::{Origin, Unit};

/// Where a throw continues: the unit PC of its handler's block, and how the entry binds the
/// thrown value.
#[derive(Clone, Copy, Debug)]
pub(super) struct Catch {
    pub(super) target: usize,
    pub(super) bind: Bind,
    /// The caller's record whose handler takes the exception: the raising record itself, or,
    /// for an instruction of an inlined callee, the call that inlined it.
    pub(super) record: usize,
}

/// What a handler's entry settles, which native code does as `Vm::exception_handler_target` does.
#[derive(Clone, Copy, Debug)]
pub(super) enum Bind {
    /// A catch that binds nothing, with no scope to unwind.
    Nothing,
    /// A binding promoted to a register (`ThrownValueSlot::Register`).
    Register(Register),
    /// A frame local no environment owns, stored where the interpreter stores it.
    Local(u16),
    /// Frame state only the frame's own helper settles (`dispatch_fast::handler_edge`): the
    /// `with` scopes the exception leaves, and a local the frame's environment owns, which a
    /// captured frame gives a fresh environment per entry; the promoted register, if any.
    Frame(Option<Register>),
}

/// Whether an exception no handler of a body covers unwinds through its native callers alone: a
/// generator or async function ends in its promise or generator state, and a dynamic scope in the
/// `with` scopes of its frame, which only the lane's own unwinding settles. A loop region's
/// exception leaves to the lane, which settles any function's (`dispatch_fast::raised_exit_in_lane`).
fn unwinds_natively(function: &Function) -> bool {
    !(function.is_generator || function.is_async || function.has_dynamic_scope_access())
}

/// Whether `instruction` raises an exception native code can route: a plain `throw` of a
/// register of the unit, or a call or construction, whose callee may throw. A call that returns
/// its result (`RETURN_REGISTER`) raises as any call: its register is untouched on the raise.
fn raising_instruction(instruction: WideInstruction, registers: Register) -> bool {
    use crate::staged::meaning::{means, Meaning};
    if means(instruction, Meaning::Throw) {
        instruction.result_flags() == 0 && instruction.register_a() < registers
    } else {
        (means(instruction, Meaning::Call) || means(instruction, Meaning::Construct))
            && instruction.result_flags() & !crate::bytecode::RETURN_REGISTER == 0
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
    /// Whether the run is a loop region (`Coverage::Loop`), which runs in the innermost frame or
    /// activation of the lane, not a body's own activation.
    region: bool,
    /// Whether the function may hold `with` scopes an exception leaves (a `with` statement,
    /// an inherited scope, names resolved at run time).
    scoped: bool,
}

impl Admission<'_> {
    /// Whether the frame state a handler's entry settles is the running function's own wherever it
    /// runs: a loop region runs in the innermost frame or activation, which the helper settles
    /// (`dispatch_fast::handler_edge`), a window's included, and so does a body of any
    /// function but a registers-only one, whose calls may be native frames no activation names
    /// (`general_frameless_admit`, `DirectCallee`); a window's body is its own activation.
    fn settles_where_it_runs(&self) -> bool {
        self.region || !self.function.call_plan(self.program).registers_only
    }

    /// Whether `handler`'s entry settles frame state beyond native stores: `with` scopes the
    /// exception may leave, a local the frame's environment owns once captured, or a local a
    /// body cannot reach (only a loop region reads the activation's locals,
    /// `JsStepDispatch::native_locals`).
    fn settles_frame(&self, handler: Handler, local: Option<u16>) -> bool {
        handler.with_depth != 0 || self.scoped
            || local.is_some_and(|slot| !self.region || self.function.local_slot_uses_environment(usize::from(slot)))
    }

    /// How native code enters `handler`, `None` for one it cannot serve: frame state in a
    /// function that may run frameless.
    fn bind(&self, handler: Handler) -> Option<Bind> {
        let (register, local) = slot_parts(self.unit, handler)?;
        if self.settles_frame(handler, local) {
            return self.settles_where_it_runs().then_some(Bind::Frame(register));
        }
        Some(match (register, local) {
            (Some(register), _) => Bind::Register(register),
            (None, Some(slot)) => Bind::Local(slot),
            (None, None) => Bind::Nothing,
        })
    }
}

/// The catch of each unit PC, none where its exception is not caught natively; no entries at all
/// where nothing is.
pub(super) type Catches = Vec<Option<Catch>>;

/// The throws and calls of `unit`'s function whose exceptions native code catches itself, by
/// unit PC. One whose handler is not admitted (`Admission::bind`), lies outside the lowered
/// range, or is not before its handler is not caught natively: a throw leaves at itself, which
/// the interpreter raises exactly, and a call is made by the lane (`Lowering::raises_from`).
pub(super) fn catch_edges(program: &ResidualProgram, function: &Function, unit: &Unit, coverage: Coverage, completion: Completion) -> Catches {
    if !completion.status_and_pc() || function.handlers.is_empty() {
        return Vec::new();
    }
    let mut catches = vec![None; unit.instructions.len()];
    let admission = Admission { program, function, unit, region: matches!(coverage, Coverage::Loop { .. }), scoped: !function.own_names_resolve_statically(program) };
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        if raising_instruction(*instruction, unit.registers) && let Some(catch) = catch_at(&admission, unit, coverage, pc) {
            catches[pc] = Some(catch);
        }
    }
    catches
}

/// The exception edges of `catches` for the type analysis: each caught PC, its handler's block
/// and the register its entry binds the value to.
pub(super) fn raises(catches: &Catches) -> Vec<super::super::facts::Raise> {
    catches.iter().enumerate().filter_map(|(pc, catch)| {
        let catch = (*catch)?;
        let bound = match catch.bind {
            Bind::Register(register) | Bind::Frame(Some(register)) => Some(register),
            Bind::Nothing | Bind::Local(_) | Bind::Frame(None) => None,
        };
        Some((pc, catch.target, bound))
    }).collect()
}

/// The native catch of the raising instruction at unit PC `pc`, where its handler is admitted and
/// lies after it inside the lowered range.
fn catch_at(admission: &Admission, unit: &Unit, coverage: Coverage, pc: usize) -> Option<Catch> {
    if !coverage.lowered(pc) { return None; }
    let (record, inlined) = match unit.origins.get(pc)? {
        Origin::Caller(record) => (*record as usize, false),
        Origin::Inlined { .. } => (inlining_record(admission.program, unit, pc)?, true),
    };
    let handler = admission.function.handler_at(record as u32)?;
    let bind = admission.bind(handler)?;
    // The handler's block is the caller's record `handler.target`, at the unit PC that runs it.
    let target = unit.origins.iter().position(|origin| *origin == Origin::Caller(handler.target))?;
    // An own record's handler lies after it; an inlined callee's code follows the caller's, so
    // its handler in the caller lies before it, a block whose state the throw conforms to.
    (handler.target as usize > record && (inlined || target > pc) && coverage.lowered(target))
        .then_some(Catch { target, bind, record })
}

/// The caller's record of the call that inlined the instruction at unit PC `pc`, where the
/// exception it raises unwinds every inlined frame between them natively: no handler of an
/// inlined function covers the instruction or the call its frame makes, and none of them is a
/// generator, an async function or a function with a dynamic scope (`unwinds_natively`).
fn inlining_record(program: &ResidualProgram, unit: &Unit, pc: usize) -> Option<usize> {
    let Some(&Origin::Inlined { mut frame, mut pc }) = unit.origins.get(pc) else { return None };
    loop {
        let function = program.function(unit.frames[frame].target.function as usize);
        if function.handler_at(pc).is_some() || !unwinds_natively(function) { return None; }
        pc = unit.frames[frame].call;
        match unit.frames[frame].parent {
            Some(parent) => frame = parent,
            None => return Some(pc as usize),
        }
    }
}

impl Lowering<'_, '_> {
    /// The record an exception raised at unit PC `pc` leaves the lowered function at, where no
    /// native handler takes it. A body's callers unwind past it, so no handler of the function
    /// may cover it (the interpreter's unwinding would run it); a loop region's only caller is
    /// the lane, which finds the handler of the record it resumes at.
    fn leaves_at(&self, pc: usize) -> Option<u32> {
        let Some(Origin::Caller(record)) = self.unit.origins.get(pc).copied() else { return None };
        let function = self.program.function(self.function_id as usize);
        let admitted = match (self.completion, self.coverage) {
            (Completion::Body, Coverage::Loop { .. }) => true,
            (Completion::Body, _) => function.handler_at(record).is_none() && unwinds_natively(function),
            (Completion::Number | Completion::Scalar(_), _) => false,
        };
        admitted.then_some(record)
    }

    /// The native catch of the exception raised at unit PC `pc`.
    fn catch_of(&self, pc: usize) -> Option<Catch> {
        self.catches.get(pc).copied().flatten()
    }

    /// Whether native code routes an exception raised at `pc`: a handler of its own catches it
    /// or the function raises it out (`LANE_BODY_THREW`). A call native code cannot route a
    /// callee's exception from is not made natively.
    pub(super) fn raises_from(&self, pc: usize) -> bool {
        self.catch_of(pc).is_some() || self.leaves_at(pc).is_some()
    }

    /// The `throw` at `pc`: a jump to its handler's block with the thrown value boxed once and
    /// bound as the handler's entry binds it, the edge conformed as any edge is; else, where no
    /// handler covers it, the function raises it out of native code (`LANE_BODY_THREW`) after
    /// the registers a handler of the lane's resumption reads are written back. `None` where
    /// it leaves at the throw for the interpreter to raise.
    pub(super) fn throw(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if !raising_instruction(instruction, self.unit.registers) { return None; }
        if let Some(Catch { target, bind, record }) = self.catch_of(pc) {
            let value = self.tagged(state, instruction.register_a());
            self.bind_caught(state, record, bind, value);
            self.continue_at(state, target);
            return Some(false);
        }
        let record = self.leaves_at(pc)?;
        let value = self.tagged(state, instruction.register_a());
        self.home_written(state, pc);
        let raise_site = super::super::raise_site(self.function_id, record) as i64;
        let site = self.raw_const(Lane::I64, raise_site);
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
        let Some(Catch { target, bind, record }) = self.catch_of(pc) else {
            self.stop_with(LANE_BODY_THREW, call_pc, None);
            return;
        };
        let value = self.helper_call(LaneHelper::TakeRaised, &[]);
        let mut caught = state.clone();
        caught.call_effect();
        self.reload_property_arena();
        self.bind_caught(&mut caught, record, bind, value);
        self.continue_at(&mut caught, target);
    }

    /// The entry of the handler that catches the exception `value` thrown at record `pc`, as
    /// `Vm::exception_handler_target` makes it: the value to its register or its frame local,
    /// or, where the frame holds state to settle, the frame's helper
    /// (`dispatch_fast::handler_edge`), whose allocation may move the property arena.
    fn bind_caught(&mut self, state: &mut State, pc: usize, bind: Bind, value: ir::Value) {
        match bind {
            Bind::Nothing => {}
            Bind::Register(register) => self.set_tagged(state, register, value),
            Bind::Local(slot) => {
                let address = super::frame::local_slot_address(&mut self.builder, self.payload, slot);
                self.store_word(regions::frame(), value, address, 0);
            }
            Bind::Frame(register) => {
                let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
                let record = self.raw_const(Lane::I64, pc as i64);
                self.helper_call(LaneHelper::HandlerEdge, &[context, record, value]);
                self.allocated(state);
                if let Some(register) = register { self.set_tagged(state, register, value); }
            }
        }
    }

    /// Stops the run with `status`, and `value` as the result word where a run publishes one:
    /// `record` is the record that ends the activation (`JsStepDispatch::returned_pc`), which
    /// the lane's completion or raise resumes at.
    pub(super) fn stop_with(&mut self, status: u32, record: u32, value: Option<ir::Value>) {
        self.finishes = true;
        let record = self.raw_const(Lane::I64, i64::from(record));
        let payload = self.payload;
        emit(&mut self.builder).store_u64(Region::Dispatch.mutable(), payload,
            i32::try_from(std::mem::offset_of!(JsStepDispatch, head.returned_pc)).expect("dispatch offset fits i32"), record);
        let status = self.raw_const(Lane::I32, i64::from(status));
        self.leave_with(status, value);
    }
}
