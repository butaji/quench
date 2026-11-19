//! The interpreter's register frame for staged rows: the running frame's registers and local
//! slots, read and written in place, and the VM's own answers to the rows it leaves to the VM
//! (captures, name reads and writes, `typeof`, suspensions, completions). A row the frame does not
//! decide is `Step::Unresolved`, and the general path takes it under the VM's own rules.

use crate::bytecode::{Operand, Register, ResidualProgram, WideInstruction};
use crate::host::Host;
use crate::staged::{CallOut, Concrete, Constant, Edge, NumericFrame, Operation, Step};
use crate::staged::Frame as _;
use crate::vm::call_out::CallSite;
use crate::vm::var_owner::VarOwner;
use crate::vm::{StepResult, Vm};
use crate::{JsError, Value};

/// The fact the interpreter keeps about a register: how its word becomes a `Value`. A staged
/// numeric row defines a binary64 result as its raw bits, which the write boxes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm) enum InterpreterFact {
    Value,
    Double,
}

/// Where the interpreter frame's row goes beyond an edge: a return or a raise (the frame's
/// completion), or a suspension of the running activation. The value a suspension carries is
/// stashed in the frame.
pub(in crate::vm) enum InterpreterControl {
    Edge(Edge),
    Return(Value),
    Throw(Value),
    Await,
    Yield,
}

/// How a staged row ends in the general loop: the edge it takes, that the arm's own path runs the
/// instruction, or the step result the VM's run stops with.
pub(in crate::vm) enum RowRun {
    Done(Edge),
    Declined,
    Stop(Result<StepResult, JsError>),
}

/// Whether the general loop may run `operation` as its staged row before its arms
/// (`InterpreterFrame::step_row`): the register, local, constant, capture, Number, compare,
/// property, name, `typeof`, `void`, completion and suspension rows the frame may answer. A row it
/// leaves runs the arm the operation's opcode has.
#[inline(always)]
pub(in crate::vm) fn runs_row(operation: Operation<u64, ()>) -> bool {
    matches!(operation,
        Operation::Move { .. } | Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. }
            | Operation::JumpIf { .. } | Operation::LoadConst { .. } | Operation::LoadCapture { .. }
            | Operation::StoreCapture { .. } | Operation::Number(_) | Operation::CompareJump { .. }
            | Operation::ElementRead { .. } | Operation::ElementWrite { .. } | Operation::GetField { .. }
            | Operation::SetField { .. } | Operation::LoadLocalChecked { .. } | Operation::StoreLocalChecked { .. }
            | Operation::LoadName { .. } | Operation::GlobalTypeof { .. } | Operation::Void { .. }
            | Operation::Return { .. } | Operation::Throw { .. } | Operation::Suspend { kind: crate::staged::Suspension::Await | crate::staged::Suspension::Yield { .. }, .. })
}

/// The running frame's register file and local slots, and the VM the row's calls answer through,
/// borrowed for one staged row.
pub(in crate::vm) struct InterpreterFrame<'a, H: Host> {
    /// The running VM (`InterpreterFrame::step_row`). The row runner holds no other reference into
    /// the VM while the row runs, so the frame's calls are its only access to it.
    vm: *mut Vm<H>,
    program: &'a ResidualProgram,
    site: CallSite,
    registers: *mut Value,
    locals: *mut Value,
    locals_len: usize,
    /// The error a VM call raised. The row answers `None` after raising, and the runner throws
    /// this rather than running the operation again on its miss path.
    raised: Option<JsError>,
    /// The value an `Await` or `Yield` suspends the run on.
    stashed: Value,
    /// The register an `Await` resumes into.
    resume: Option<Register>,
}

impl<'a, H: Host> InterpreterFrame<'a, H> {
    /// `operation` run on frame `frame`'s registers and locals and the VM's own answers, where it
    /// decides it (`RowRun::Declined` where it does not). `pc` is the resume PC of `instruction`,
    /// which a `Yield` takes its destination from.
    #[inline(always)]
    pub(in crate::vm) fn step_row(vm: *mut Vm<H>, program: &'a ResidualProgram, frame: usize, pc: usize,
        instruction: WideInstruction, operation: Operation<u64, ()>) -> RowRun {
        // SAFETY: `vm` is the running VM, live for the row. The register and local pointers are the
        // running activation's, which stay allocated for the activation's run.
        let (registers, locals, locals_len, function) = unsafe {
            let running = &mut (&mut *vm).frames[frame];
            (running.registers.as_mut_ptr(), running.locals.as_mut_ptr(), running.locals.len(), running.function)
        };
        let site = CallSite { owner: VarOwner::Frame(frame), function, resume: pc };
        let mut interpreted = Self {
            vm,
            program,
            site,
            registers,
            locals,
            locals_len,
            raised: None,
            stashed: Value::UNDEFINED,
            resume: None,
        };
        let step = crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut interpreted, operation);
        if let Some(error) = interpreted.raised.take() {
            return RowRun::Stop(Err(error));
        }
        match step {
            Step::Value(_) => RowRun::Done(Edge::Following),
            Step::Unresolved => RowRun::Declined,
            Step::Control(InterpreterControl::Edge(Edge::Leave | Edge::Transfer | Edge::Entry(_))) =>
                unreachable!("the interpreter frame neither suspends the lane nor enters a branch table"),
            Step::Control(InterpreterControl::Edge(edge)) => RowRun::Done(edge),
            Step::Control(InterpreterControl::Return(value)) => RowRun::Stop(Ok(StepResult::Return(value))),
            Step::Control(InterpreterControl::Throw(value)) => RowRun::Stop(Err(Self::thrown(vm, value))),
            Step::Control(InterpreterControl::Await) => RowRun::Stop(Ok(StepResult::Await {
                value: interpreted.stashed,
                destination: interpreted.resume.expect("an await resumes into its destination"),
            })),
            Step::Control(InterpreterControl::Yield) => RowRun::Stop(Ok(StepResult::Yield {
                value: interpreted.stashed,
                destination: instruction.result_register(),
                delegated_result: None,
            })),
        }
    }

    /// The error a `Throw` of `value` raises: the primitive's own, else the object's with its detail.
    fn thrown(vm: *mut Vm<H>, value: Value) -> JsError {
        // SAFETY: the row has finished with the VM's calls; `vm` is live (`step_row`).
        let vm = unsafe { &*vm };
        JsError::thrown_primitive(value).unwrap_or_else(|| JsError::thrown(value, vm.detail_string(value)))
    }

    /// The running VM, which the frame's calls answer through.
    #[inline(always)]
    fn vm<'v>(&self) -> &'v mut Vm<H> {
        // SAFETY: `vm` is the running VM, live for the row (`step_row`), and the row holds no other
        // reference into it while its calls run.
        unsafe { &mut *self.vm }
    }

    /// The register `register` of the file.
    #[inline(always)]
    fn slot(&self, register: Register) -> *mut Value {
        // SAFETY: validated bytecode bounds every register by the frame's register count, which
        // sized the file this frame borrows.
        unsafe { self.registers.add(usize::from(register)) }
    }
}

impl<H: Host> crate::staged::Frame<Concrete> for InterpreterFrame<'_, H> {
    type Fact = InterpreterFact;
    type Control = InterpreterControl;

    #[inline(always)]
    fn fact(&self, _register: Register) -> InterpreterFact { InterpreterFact::Value }

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        // SAFETY: as `slot`.
        unsafe { *self.slot(register) }.to_bits()
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, fact: InterpreterFact) {
        let value = match fact {
            InterpreterFact::Value => Value::from_bits(value),
            InterpreterFact::Double => Value::number(f64::from_bits(value)),
        };
        // SAFETY: as `slot`.
        unsafe { *self.slot(register) = value };
    }

    /// The interpreter's `this` is the frame's own, which the rows this frame decides never read.
    #[inline(always)]
    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }

    #[inline(always)]
    fn define_this(&mut self, stage: &mut Concrete, destination: Register, this: u64) {
        self.write(stage, destination, this, InterpreterFact::Value);
    }

    #[inline(always)]
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, InterpreterFact) {
        (constant.value.to_bits(), InterpreterFact::Value)
    }

    /// ToBoolean from the value's own word; `None` where a heap cell decides it, which the general
    /// path answers.
    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        Value::from_bits(self.read(stage, register)).truthiness()
    }

    #[inline(always)]
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> InterpreterControl {
        InterpreterControl::Edge(if condition { Edge::Target } else { Edge::Following })
    }

    #[inline(always)]
    fn jump(&mut self, _stage: &mut Concrete) -> InterpreterControl { InterpreterControl::Edge(Edge::Target) }

    #[inline(always)]
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, InterpreterFact) {
        (Value::boolean(condition).to_bits(), InterpreterFact::Value)
    }

    #[inline(always)]
    fn load_local(&mut self, _stage: &mut Concrete, slot: u16, _miss: Option<()>) -> Option<(u64, InterpreterFact)> {
        if usize::from(slot) >= self.locals_len {
            return None;
        }
        // SAFETY: `slot` is inside the frame's local slots.
        let value = unsafe { *self.locals.add(usize::from(slot)) };
        Some((value.to_bits(), InterpreterFact::Value))
    }

    #[inline(always)]
    fn store_local(&mut self, _stage: &mut Concrete, slot: u16, value: u64, _miss: Option<()>) -> bool {
        if usize::from(slot) >= self.locals_len {
            return false;
        }
        // SAFETY: `slot` is inside the frame's local slots.
        unsafe { *self.locals.add(usize::from(slot)) = Value::from_bits(value) };
        true
    }

    /// The VM's operation for a name read, a global `typeof` or a global store (`CallOut`), at the
    /// frame's site. A raise is stashed for the runner.
    #[inline(always)]
    fn vm_step(&mut self, _stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, InterpreterFact)> {
        if !matches!(operation, CallOut::LoadName { .. } | CallOut::GlobalTypeof { .. } | CallOut::GlobalWrite { .. }) {
            return None;
        }
        let (program, site) = (self.program, self.site);
        let vm = self.vm();
        let operands = operation.operands().read(&program.method_arguments, |operand| match operand.register_index() {
            Some(register) => vm.owner_register(site.owner, register),
            None => vm.active_constant(usize::from(operand.payload())).expect("a call-out's constant is in its program"),
        });
        match vm.call_out(program, site, operation, operands) {
            Ok(value) => Some((value.map_or(0, Value::to_bits), InterpreterFact::Value)),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    /// Read the capture `slot`, `depth` scopes out, by the VM's binding rules. A raise (a binding in
    /// its temporal dead zone) is stashed for the runner.
    #[inline(always)]
    fn capture_slot(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, _root: bool) -> Option<(u64, InterpreterFact)> {
        let (program, owner) = (self.program, self.site.owner);
        match self.vm().capture(program, owner, depth, slot) {
            Ok(value) => Some((value.to_bits(), InterpreterFact::Value)),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    /// Store `value` to the capture `slot`, `depth` scopes out, by the VM's binding rules.
    #[inline(always)]
    fn store_capture(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, value: u64) -> Option<()> {
        let (program, owner) = (self.program, self.site.owner);
        match self.vm().store_capture(program, owner, depth, slot, Value::from_bits(value)) {
            Ok(()) => Some(()),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    /// The frame's return of the register `register`: the completion the general loop makes of a `Return`.
    #[inline(always)]
    fn ret(&mut self, stage: &mut Concrete, register: Register) -> Option<InterpreterControl> {
        Some(InterpreterControl::Return(Value::from_bits(self.read(stage, register))))
    }

    /// `Throw` of the value in `source`: the frame's completion, which the runner raises.
    #[inline(always)]
    fn raise(&mut self, stage: &mut Concrete, source: Register) -> Option<InterpreterControl> {
        Some(InterpreterControl::Throw(Value::from_bits(self.read(stage, source))))
    }
}

impl<H: Host> crate::staged::GlobalFrame<Concrete> for InterpreterFrame<'_, H> {}

impl<H: Host> crate::staged::CallFrame<Concrete> for InterpreterFrame<'_, H> {}

impl<H: Host> crate::staged::ElementFrame<Concrete> for InterpreterFrame<'_, H> {}

impl<H: Host> crate::staged::PropertyFrame<Concrete> for InterpreterFrame<'_, H> {
    type Plans = crate::staged::NoPlans;

    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }
}

impl<H: Host> NumericFrame<Concrete> for InterpreterFrame<'_, H> {
    /// An int32 operand's word, from its register's tagged word.
    #[inline(always)]
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_int().map(|int| u64::from(int as u32))
    }

    #[inline(always)]
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_number().map(f64::to_bits)
    }

    #[inline(always)]
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }

    #[inline(always)]
    fn int_result(&mut self, _stage: &mut Concrete, word: u64) -> u64 {
        Value::integer(word as u32 as i32).to_bits()
    }

    #[inline(always)]
    fn miss(&self) {}

    #[inline(always)]
    fn int_fact(&self) -> InterpreterFact { InterpreterFact::Value }

    #[inline(always)]
    fn number_fact(&self) -> InterpreterFact { InterpreterFact::Double }
}
