//! The interpreter's register frame for staged rows: the running frame's registers and local
//! slots, read and written in place, and the VM's own answers to the rows it leaves to the VM
//! (captures, name reads and writes, `typeof`, suspensions, completions). A row the frame does not
//! decide is `Step::Unresolved`, and the general path takes it under the VM's own rules.

use crate::bytecode::{Operand, Register, ResidualProgram, WideInstruction};
use crate::staged::protocol_literals::LiteralStep;
use crate::staged::value::TypeofKind;
use crate::staged::{Allocation, CallOut, Concrete, Constant, Edge, NumericFrame, Operation, Step};
use crate::staged::Frame as _;
use crate::staged::CallFrame as _;
use crate::staged::invoke::CallForm;
use crate::vm::call_out::{CallSite, Loaded};
use crate::vm::var_owner::VarOwner;
use crate::vm::{StepResult, Suspended, Vm};
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
pub(in crate::vm) struct InterpreterFrame<'a> {
    /// The running VM (`InterpreterFrame::step_row`). The row runner holds no other reference into
    /// the VM while the row runs, so the frame's calls are its only access to it.
    vm: *mut Vm,
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
    /// Whether a read's getter may run as an in-place activation (`Vm::load_field_step`): the caller's
    /// `allow_inline_calls`, which `step` passes down.
    allow_inline_calls: bool,
    /// The activation a read's getter pushed, which the row stops the run on (`step_row`).
    pushed: Option<StepResult>,
}

impl<'a> InterpreterFrame<'a> {
    /// The frame over the running activation `frame` of the VM `vm`, its registers and locals read and written in
    /// place; `pc` is the resume PC the row's effects are made at.
    #[inline(always)]
    fn over(vm: *mut Vm, program: &'a ResidualProgram, frame: usize, pc: usize, allow_inline_calls: bool) -> Self {
        // SAFETY: `vm` is the running VM, live for the row. The register and local pointers are the
        // running activation's, which stay allocated for the activation's run.
        let (registers, locals, locals_len, function) = unsafe {
            let running = &mut (&mut *vm).frames[frame];
            (running.registers.as_mut_ptr(), running.locals.as_mut_ptr(), running.locals.len(), running.function)
        };
        let site = CallSite { owner: VarOwner::Frame(frame), function, resume: pc };
        Self {
            vm,
            program,
            site,
            registers,
            locals,
            locals_len,
            raised: None,
            stashed: Value::UNDEFINED,
            resume: None,
            allow_inline_calls,
            pushed: None,
        }
    }

    /// The general call `form` of the call-family record `instruction` at resume PC `pc` on frame `frame`, run
    /// after the record's staged row declined (`CallFrame::call_general`). Its completion is `Done` where the
    /// call completed its result, else the step the VM's call ends in (`Stop`).
    #[inline(always)]
    pub(in crate::vm) fn call_row(vm: *mut Vm, program: &'a ResidualProgram, frame: usize, pc: usize,
        allow_inline_calls: bool, instruction: WideInstruction, form: CallForm) -> RowRun {
        let mut interpreted = Self::over(vm, program, frame, pc, allow_inline_calls);
        let step = interpreted.call_general(form, instruction, pc);
        if let Some(error) = interpreted.raised.take() {
            return RowRun::Stop(Err(error));
        }
        if let Some(pushed) = interpreted.pushed.take() {
            return RowRun::Stop(Ok(pushed));
        }
        match step {
            Some(_) => RowRun::Done(Edge::Following),
            None => RowRun::Declined,
        }
    }

    /// `operation` run on frame `frame`'s registers and locals and the VM's own answers, where it
    /// decides it (`RowRun::Declined` where it does not). `pc` is the resume PC of `instruction`,
    /// which a `Yield` takes its destination from.
    #[inline(always)]
    pub(in crate::vm) fn step_row(vm: *mut Vm, program: &'a ResidualProgram, frame: usize, pc: usize,
        allow_inline_calls: bool, instruction: WideInstruction, operation: Operation<u64, ()>) -> RowRun {
        let mut interpreted = Self::over(vm, program, frame, pc, allow_inline_calls);
        let step = crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut interpreted, operation);
        if let Some(error) = interpreted.raised.take() {
            return RowRun::Stop(Err(error));
        }
        if let Some(pushed) = interpreted.pushed.take() {
            return RowRun::Stop(Ok(pushed));
        }
        match step {
            Step::Value(_) => RowRun::Done(Edge::Following),
            Step::Unresolved => RowRun::Declined,
            Step::Control(InterpreterControl::Edge(Edge::Leave | Edge::Transfer)) =>
                unreachable!("the interpreter frame neither suspends the lane nor transfers it"),
            Step::Control(InterpreterControl::Edge(edge)) => RowRun::Done(edge),
            Step::Control(InterpreterControl::Return(value)) => RowRun::Stop(Ok(StepResult::Return(value))),
            Step::Control(InterpreterControl::Throw(value)) => RowRun::Stop(Err(Self::thrown(vm, value))),
            Step::Control(InterpreterControl::Await) => RowRun::Stop(Ok(StepResult::Suspend(Suspended::Await {
                value: interpreted.stashed,
                destination: interpreted.resume.expect("an await resumes into its destination"),
            }))),
            Step::Control(InterpreterControl::Yield) => RowRun::Stop(Ok(StepResult::Suspend(Suspended::Yield {
                value: interpreted.stashed,
                destination: instruction.result_register(),
                delegated_result: None,
            }))),
        }
    }

    /// The error a `Throw` of `value` raises: the primitive's own, else the object's with its detail.
    fn thrown(vm: *mut Vm, value: Value) -> JsError {
        // SAFETY: the row has finished with the VM's calls; `vm` is live (`step_row`).
        let vm = unsafe { &*vm };
        vm.thrown_error(value)
    }

    /// The running VM, which the frame's calls answer through.
    #[inline(always)]
    fn vm<'v>(&self) -> &'v mut Vm {
        // SAFETY: `vm` is the running VM, live for the row (`step_row`), and the row holds no other
        // reference into it while its calls run.
        unsafe { &mut *self.vm }
    }

    /// The value of a read the VM made, or the activation its getter pushed, which the run stops on (`pushed`).
    fn loaded(&mut self, loaded: Result<Loaded, JsError>) -> Option<(u64, InterpreterFact)> {
        match loaded {
            Ok(Loaded::Value(value)) => Some((value.to_bits(), InterpreterFact::Value)),
            Ok(Loaded::Pushed(step)) => {
                self.pushed = Some(step);
                None
            }
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    /// The register `register` of the file.
    #[inline(always)]
    fn slot(&self, register: Register) -> *mut Value {
        // SAFETY: validated bytecode bounds every register by the frame's register count, which
        // sized the file this frame borrows.
        unsafe { self.registers.add(usize::from(register)) }
    }
}

impl crate::staged::Frame<Concrete> for InterpreterFrame<'_> {
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

    /// The running activation's bound `this`; an unbound one raises, which the runner throws.
    #[inline(always)]
    fn this_value(&mut self, _stage: &mut Concrete) -> Option<(u64, InterpreterFact)> {
        let (program, site) = (self.program, self.site);
        let vm = self.vm();
        let frame = vm.owner_frame(site.owner)?;
        match vm.checked_this_binding(program, frame) {
            Ok(this) => Some((this.to_bits(), InterpreterFact::Value)),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    #[inline(always)]
    fn callee_value(&mut self, _stage: &mut Concrete) -> Option<(u64, InterpreterFact)> {
        let site = self.site;
        let vm = self.vm();
        let frame = vm.owner_frame(site.owner)?;
        Some((vm.frames[frame].context.callee().unwrap_or(Value::UNDEFINED).to_bits(), InterpreterFact::Value))
    }

    #[inline(always)]
    fn define_this(&mut self, stage: &mut Concrete, destination: Register, this: u64) {
        self.write(stage, destination, this, InterpreterFact::Value);
    }

    #[inline(always)]
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, InterpreterFact) {
        (constant.value.to_bits(), InterpreterFact::Value)
    }

    /// ToBoolean of the register's value: an immediate's by its tag, a heap value's by its cell (`Vm::truthy`,
    /// which runs no guest code).
    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        let value = Value::from_bits(self.read(stage, register));
        Some(value.truthiness().unwrap_or_else(|| self.vm().truthy(value)))
    }

    /// The `typeof` kind of the heap value `word`, by its cell (`Vm::typeof_kind`).
    fn type_of(&mut self, _stage: &mut Concrete, word: u64) -> Option<TypeofKind> {
        let index = crate::vm::coercion::typeof_name_index(self.vm().typeof_kind(Value::from_bits(word)));
        Some(TypeofKind::of_index(index as u64))
    }

    /// The interned `typeof` result string of the name index `name` (`Vm::typeof_names`).
    fn typeof_string(&mut self, _stage: &mut Concrete, name: u64) -> Option<(u64, InterpreterFact)> {
        Some((self.vm().typeof_names[name as usize].to_bits(), InterpreterFact::Value))
    }

    #[inline(always)]
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> InterpreterControl {
        InterpreterControl::Edge(if condition { Edge::Target } else { Edge::Following })
    }

    #[inline(always)]
    fn jump(&mut self, _stage: &mut Concrete) -> InterpreterControl { InterpreterControl::Edge(Edge::Target) }

    /// The entry a branch table's selector picks, the last one past the cases (`branch_table_case`): `step_row`
    /// leaves the table at it as `Edge::Entry`, and `run_row` takes it.
    #[inline(always)]
    fn branch_table(&mut self, _stage: &mut Concrete, index: u64, entries: u32) -> Option<InterpreterControl> {
        Some(InterpreterControl::Edge(Edge::Entry((index as u32).min(entries - 1))))
    }

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

    /// The VM's operation for a name read, a global `typeof` or a store, a for-in step, an async
    /// iterator and a template object (`CallOut`), at the frame's site. A raise is stashed for the runner.
    #[inline(always)]
    fn vm_step(&mut self, _stage: &mut Concrete, operation: CallOut, result: Option<Register>) -> Option<(u64, InterpreterFact)> {
        // A named read at the running frame: its getter may run in place, so the read is the VM's `load_field_step`.
        if let CallOut::LoadField { lookup } = operation {
            let VarOwner::Frame(frame) = self.site.owner else { return None };
            let (program, site, allow) = (self.program, self.site, self.allow_inline_calls);
            let loaded = self.vm().load_field_step(program, frame, site.resume, lookup, result, allow);
            return self.loaded(loaded);
        }
        // An indexed read: the same, for a proved symbol key's getter.
        if let CallOut::LoadElement { base, key } = operation {
            let VarOwner::Frame(frame) = self.site.owner else { return None };
            let (program, site, allow) = (self.program, self.site, self.allow_inline_calls);
            let vm = self.vm();
            let value = |operand: Operand| match operand.register_index() {
                Some(register) => vm.owner_register(site.owner, register),
                None => vm.active_constant(usize::from(operand.payload())).expect("a call-out's constant is in its program"),
            };
            let operands = (value(base), value(key));
            let loaded = vm.load_element_step(program, frame, site.resume, operands, result, allow);
            return self.loaded(loaded);
        }
        if !matches!(operation,
            CallOut::LoadName { .. } | CallOut::GlobalTypeof { .. } | CallOut::GlobalWrite { .. } | CallOut::ForInNext { .. }
                | CallOut::Binary { .. } | CallOut::Equality { .. } | CallOut::PropertyKey { .. } | CallOut::Decide { .. }
                | CallOut::DecideOperand { .. } | CallOut::ToNumeric { .. } | CallOut::IncDec { .. } | CallOut::Unary { .. }
                | CallOut::StoreField { .. } | CallOut::StoreElement { .. }
                | CallOut::Protocol { step: crate::staged::Protocol::Value(crate::staged::protocol_values::ValueStep::GetAsyncIterator)
                    | crate::staged::Protocol::Literal(crate::staged::protocol_literals::LiteralStep::CacheTemplate { .. }
                        | crate::staged::protocol_literals::LiteralStep::LoadTemplate { .. }), .. })
        {
            return None;
        }
        let (program, site) = (self.program, self.site);
        let vm = self.vm();
        let mut failure = None;
        let operands = operation.operands().read(&program.method_arguments, |operand| match vm.call_out_operand(program, site.owner, operand) {
            Ok(value) => value,
            Err(error) => {
                failure.get_or_insert(error);
                Value::UNDEFINED
            }
        });
        if let Some(error) = failure {
            self.raised = Some(error);
            return None;
        }
        // A template object is the cache entry of the frame the row runs in, so it is keyed by that frame
        // rather than the VM's innermost one (`call_out` makes the step over its own frame).
        if let CallOut::Protocol { step: crate::staged::Protocol::Literal(step @ (LiteralStep::CacheTemplate { .. } | LiteralStep::LoadTemplate { .. })), .. } = operation {
            let VarOwner::Frame(frame) = site.owner else { return None };
            let (program, function) = (vm.frames[frame].program, vm.frames[frame].function);
            let value = match step {
                LiteralStep::CacheTemplate { site } => {
                    let first = operands.fixed[0].expect("CacheTemplateObject names its template object");
                    *vm.realm.template_objects.entry((program, function, site)).or_insert(first)
                }
                LiteralStep::LoadTemplate { site } => {
                    vm.realm.template_objects.get(&(program, function, site)).copied().unwrap_or(Value::UNDEFINED)
                }
                _ => unreachable!("{step:?} is no template object step"),
            };
            return Some((value.to_bits(), InterpreterFact::Value));
        }
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

    /// `StringCaseIndex`: the case the VM's table selects for `source` among the `count` program
    /// constants from `start` (`Vm::string_case_index`). Constants outside the program raise.
    #[inline(always)]
    fn case_index(&mut self, stage: &mut Concrete, source: Register, start: u32, count: u16) -> Option<(u64, InterpreterFact)> {
        let VarOwner::Frame(frame) = self.site.owner else { return None };
        let value = Value::from_bits(self.read(stage, source));
        let vm = self.vm();
        let start = start as usize;
        let cases = vm.programs.constants(vm.frames[frame].program).get(start..start + usize::from(count));
        let Some(cases) = cases else {
            self.raised = Some(JsError::validation("switch cases are outside program".into()));
            return None;
        };
        let index = vm.string_case_index(value, cases);
        Some((Value::integer(index as i32).to_bits(), InterpreterFact::Value))
    }

    /// `RequireIteratorResult`'s object test: the VM's own check, which raises the iterator-result TypeError.
    #[inline(always)]
    fn object_like(&mut self, _stage: &mut Concrete, word: u64) -> Option<()> {
        let program = self.program;
        let vm = self.vm();
        match vm.require_iterator_result(program, Value::from_bits(word)) {
            Ok(()) => Some(()),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    /// `RequireObjectCoercible`'s miss: the VM's own check, which raises for a `null` or `undefined` word.
    #[inline(always)]
    fn object_coercion_failed(&mut self, _stage: &mut Concrete, word: u64) {
        let program = self.program;
        let vm = self.vm();
        if let Err(error) = vm.require_object_coercible(program, Value::from_bits(word)) {
            self.raised = Some(error);
        }
    }

    /// `SetFunctionName` of `function` to the program atom `name` at cache `site`: the VM's naming.
    #[inline(always)]
    fn set_function_name(&mut self, _stage: &mut Concrete, function: u64, name: u32, site: u16) -> Option<()> {
        let program = self.program;
        let vm = self.vm();
        match vm.set_function_name(program, Value::from_bits(function), (name, site)) {
            Ok(()) => Some(()),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    /// An array literal's element definition: the VM's definition into the fresh hole (`define_array_literal_element`).
    #[inline(always)]
    fn define_element(&mut self, _stage: &mut Concrete, array: u64, index: u32, value: u64) -> Option<()> {
        let program = self.program;
        let vm = self.vm();
        match vm.define_array_literal_element(program, Value::from_bits(array), index as usize, Value::from_bits(value)) {
            Ok(()) => Some(()),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }

    /// `IteratorCleanupPush`: the running frame's cleanup stack takes `iterator` with its `done` register.
    #[inline(always)]
    fn iterator_push(&mut self, _stage: &mut Concrete, iterator: Register, done: Register) -> Option<()> {
        let VarOwner::Frame(frame) = self.site.owner else { return None };
        self.vm().frames[frame].active_iterators.push(crate::vm::ActiveIterator { iterator, done });
        Some(())
    }

    /// `IteratorCleanupPop`: the running frame's innermost cleanup entry is removed. An empty stack is
    /// the interpreter's underflow error, which the runner throws.
    #[inline(always)]
    fn iterator_pop(&mut self, _stage: &mut Concrete) -> Option<()> {
        let VarOwner::Frame(frame) = self.site.owner else { return None };
        if self.vm().frames[frame].active_iterators.pop().is_some() {
            return Some(());
        }
        self.raised = Some(JsError("iterator cleanup stack underflow".into()));
        None
    }

    /// The heap value `allocation` makes, defined into `destination`: the VM's own allocation of each literal,
    /// closure, regexp, template and concatenation (`Operation::Allocate`). A raise is stashed for the runner.
    #[inline(always)]
    fn allocate_value(&mut self, stage: &mut Concrete, destination: Register, allocation: Allocation, _proof: ()) -> Option<u64> {
        let (program, call_site) = (self.program, self.site);
        let VarOwner::Frame(frame) = call_site.owner else { return None };
        let owner = VarOwner::Frame(frame);
        let vm = self.vm();
        let program_id = vm.frames[frame].program;
        let made: Result<Value, JsError> = match allocation {
            Allocation::Empty => Ok(vm.object()),
            Allocation::Array { length, numbers: false } => Ok(vm.hole_array(length as usize)),
            // A Number-element array: its element kind is the allocation's (`Vm::make_array`).
            Allocation::Array { length, numbers: true } => Ok(vm.make_array(length as usize, true)),
            // A regexp literal that is a call's first argument: the site's shared object while the callee
            // is the realm's String method (`Vm::regexp_literal_argument_of`), else a fresh one.
            Allocation::RegExp { site, callee: Some(callee) } => {
                let callee = vm.read(frame, callee);
                vm.regexp_literal_argument_of(program, program_id, frame, site as usize, callee)
            }
            // An array literal's elements are defined after it (`DefineElement`), so `decode_literal` never makes it whole.
            Allocation::ArrayOf { .. } => return None,
            Allocation::RegExp { site, callee: None } => vm.regexp_literal(program, frame, site as usize),
            Allocation::ConstArray { constant, count } => vm
                .const_array_value(program_id, constant as usize, usize::from(count))
                .ok_or_else(|| JsError::validation("constant array is outside program".into())),
            Allocation::ObjectPair { site, first, second } => {
                let (first, second) = (vm.read(frame, first), vm.read(frame, second));
                Ok(vm.object_pair(program, site as usize, first, second))
            }
            Allocation::ObjectLiteral { site, window } => {
                let shape = vm.object_site_shape(program, site as usize);
                let prototype = vm.object_proto;
                let start = usize::from(window.base);
                let end = start + usize::from(window.count);
                let properties = &vm.frames[frame].registers[start..end];
                Ok(vm.heap.alloc_object_with_properties(prototype, shape, properties))
            }
            Allocation::Closure { function } => {
                // The closure's environment is captured at the instruction's resume PC, as the binding site.
                let previous = vm.replace_binding_site(owner, Some(call_site.resume as u32));
                let environment = vm.capture_binding_environment(owner);
                vm.replace_binding_site(owner, previous);
                environment.and_then(|env| vm.closure_over_environment(program, owner, function, env))
            }
            Allocation::Template { template } => vm.literal_template(program, frame, template as usize),
            Allocation::Concat { window } => vm.frame_string_concat(program, frame, window),
        };
        match made {
            Ok(value) => {
                self.write(stage, destination, value.to_bits(), InterpreterFact::Value);
                Some(value.to_bits())
            }
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }
}

impl crate::staged::GlobalFrame<Concrete> for InterpreterFrame<'_> {}

impl crate::staged::CallFrame<Concrete> for InterpreterFrame<'_> {
    /// The call's general form, run by the VM on the running activation (`Vm::call_form_step`). Its
    /// completion is a result the frame defines (`Step::Value`, the row's edge follows), or the step
    /// the call ends in (`pushed`, the activation or tail call the run stops on, or a return); a raise
    /// is stashed for the runner.
    #[inline(always)]
    fn call_general(&mut self, form: CallForm, instruction: WideInstruction, pc: usize) -> Option<Step<u64, InterpreterControl>> {
        let VarOwner::Frame(frame) = self.site.owner else { return None };
        let (program, allow) = (self.program, self.allow_inline_calls);
        match self.vm().call_form_step(program, frame, form, instruction, pc, allow) {
            Ok(StepResult::Continue) => Some(Step::Value(0)),
            Ok(result) => {
                self.pushed = Some(result);
                Some(Step::Value(0))
            }
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }
}

impl crate::staged::ElementFrame<Concrete> for InterpreterFrame<'_> {}

impl crate::staged::PropertyFrame<Concrete> for InterpreterFrame<'_> {
    type Plans = crate::staged::NoPlans;

    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }
}

impl NumericFrame<Concrete> for InterpreterFrame<'_> {
    /// An int32 operand's word, from its register's tagged word.
    #[inline(always)]
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<crate::staged::Int32<u64>> {
        let word = self.operand_word(stage, operand)?;
        crate::staged::Int32::checked(Value::from_bits(word))
    }

    #[inline(always)]
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<crate::staged::Float64<u64>> {
        let word = self.operand_word(stage, operand)?;
        crate::staged::Float64::checked(Value::from_bits(word))
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

    /// The interpreter's registers hold tagged words, so a Number is defined as its boxed word.
    #[inline(always)]
    fn tagged_number_fact(&self) -> Option<InterpreterFact> { Some(InterpreterFact::Value) }

    #[inline(always)]
    fn number_fact(&self) -> InterpreterFact { InterpreterFact::Double }

    /// The interpreter takes the VM's general operator in place: a Number row its words leave is the VM's.
    #[inline(always)]
    fn general_operators(&self) -> bool { true }

    /// The interpreter takes the VM's named and keyed stores in place, where a store row's plan leaves them.
    #[inline(always)]
    fn general_stores(&self) -> bool { true }

    /// The interpreter takes the VM's named and indexed reads in place, where a read row's plan leaves them.
    #[inline(always)]
    fn general_loads(&self) -> bool { true }

    /// The local `local` takes `word` as `fact` boxes it: an int32 word as its value, a binary64 word as a Number.
    #[inline(always)]
    fn store_numeric_local(&mut self, _stage: &mut Concrete, local: Register, word: u64, fact: InterpreterFact) -> bool {
        if usize::from(local) >= self.locals_len {
            return false;
        }
        let value = match fact {
            InterpreterFact::Value => Value::from_bits(word),
            InterpreterFact::Double => Value::number(f64::from_bits(word)),
        };
        // SAFETY: `local` is inside the frame's local slots.
        unsafe { *self.locals.add(usize::from(local)) = value };
        true
    }
}
