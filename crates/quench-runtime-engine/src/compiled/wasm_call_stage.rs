//! The Wasm tier's calls: the lowering of each `Call` step (a direct call linked at placement,
//! a callout, an indirect call through the VM boundary) and the analysis of its callee and
//! arguments. An indirect target's checks (`staged::wasm_call::indirect_target`) and its call
//! are emitted over the table's mirror (`wasm_table_stage`).
//!
//! A tail call (`is_tail_call`: a call whose destination is flagged as a return) is a true tail
//! call when its callee is compiled natively and returns the same result as this function: a
//! `return_call` (here, the known call's activation, `KnownCall::call_hand_over`) or a `return_call_indirect` (the native fast path of
//! `wasm_table_stage`). Bodies use `BODY_CALL_CONV` (`Tail`) so the convention allows it. The
//! other shapes (a callout, a VM call-out, a foreign instance, a result of another type) are a
//! call that writes the result, followed by the return of that result (`return_if_tail`), as
//! before. The b-jit's stitched tail calls still grow the native stack (`wasm_stitch_tail`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::wasm_instance_stage::value_kind;
use super::{
    Analysis, AnalysisState, BODY_CALL_CONV, Emitter, Kind, Plan, Slot, function_name,
    internal_signature,
};
use quench_runtime::WasmSignature;
use quench_runtime::bytecode::{Function, WideInstruction};
use quench_runtime::staged::machine::{MachineAccess, MachineStage, MachineTail};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_call::Call;
use quench_runtime::staged::wasm_step::Step;
use cranelift_codegen::ir;
use quench_runtime::bytecode::Register;
use quench_runtime::staged::plan::{CallEntry, CalleeKind, DirectPlan, Identity};
use quench_runtime::staged::wasm_invoke::step_known_call_of;
use quench_runtime::staged::{CallFrame, Callee, Constant, Entered, Frame, Invocation, NoPlans, PropertyFrame, Room,
    Stage, Step as StagedStep};
use super::object_slots::Emit;

/// Whether `instruction` is a Wasm call whose destination is flagged as a return: a tail call,
/// after which the function returns (nothing follows it on its path).
pub(crate) fn is_tail_call(instruction: WideInstruction) -> bool {
    instruction.result_flags() & quench_runtime::bytecode::RETURN_REGISTER != 0
        && matches!(Step::of(instruction.op(), instruction.imm()), Some(Step::Call(_)))
}

impl Emitter<'_, '_> {
    /// Lowers a call step.
    pub(crate) fn call(&mut self, pc: usize, instruction: WideInstruction, call: Call) {
        match call {
            Call::Known => self.call_known(instruction),
            Call::IndirectTarget => self.indirect_target(pc, instruction),
            // A function reference is known statically per call site (`Slot::Import`).
            Call::RefFunc => {}
            Call::Reference => match self.plan.operand(pc) {
                Slot::Import(_) => {
                    self.callout(pc, instruction);
                    self.return_if_tail(instruction);
                }
                Slot::IndirectReference { .. } => self.call_reference(pc, instruction),
                _ => unreachable!("planned calls hold a supported callable"),
            },
        }
    }

    /// Ends a tail call that wrote its result to its destination: the function returns it.
    /// A tail call that was a true tail call already left (`KnownCall::call_hand_over`, `call_reference`).
    pub(crate) fn return_if_tail(&mut self, instruction: WideInstruction) {
        if is_tail_call(instruction) {
            self.return_register(instruction.result_register());
        }
    }

    /// A direct call of a function the program defines, through the one call step
    /// (`wasm_invoke::step_known_call_of`) over the call's native frame (`KnownCall`): a tail call
    /// of a callee returning this function's result is the frame's activation (`return_call`), any
    /// other call enters the callee and settles its status. A callee that did not compile is its
    /// callout.
    fn call_known(&mut self, instruction: WideInstruction) {
        let function = instruction.known_function_index();
        let callee = usize::from(function);
        let Some(plan) = self.plans[callee].as_ref() else {
            self.callout_known_fallback(callee as u32, instruction);
            self.return_if_tail(instruction);
            return;
        };
        let replaces = is_tail_call(instruction) && plan.result == self.plan.result;
        let arguments = self.known_arguments(instruction, plan);
        let target = self.known_target(callee, plan);
        // A tail call leaves the frame's references to the callee, which roots its own before its
        // calls; a call that returns keeps them rooted around it.
        let roots = if replaces { self.interrupt_safepoint(); None } else { self.root() };
        let window = instruction.call_window();
        let destination = instruction.result_register();
        let mut frame = KnownCall { target, arguments, replaces, settled: None };
        let builder = &mut self.body.core.builder;
        let miss = builder.create_block();
        let step = step_known_call_of(&mut super::object_slots::emit(builder), &mut frame, miss,
            (function, (window.base, window.count), destination, replaces));
        let value = match step {
            StagedStep::Control(()) => return,
            StagedStep::Value(value) => value,
            StagedStep::Unresolved => unreachable!("a compiled callee's call is direct"),
        };
        let settled = frame.settled.expect("a returned call settled");
        self.settle_known(settled, roots, (plan.result, destination, value));
        self.return_if_tail(instruction);
    }

    /// Settles a known call that returned to this function: the step left the returned path's
    /// block open and empty, so the unfinished path is filled first, any status but returned (a
    /// trap or a raised callout) unwinding this activation too, then the returned path releases the
    /// roots and writes the callee's result (`0` for a callee with none) to `destination`.
    fn settle_known(&mut self, (status, unfinished): (ir::Value, ir::Block), roots: Option<ir::Value>,
        (result, destination, value): (Option<quench_runtime::WasmType>, u16, ir::Value)) {
        let returned = self.body.core.builder.current_block().expect("the returned path");
        self.body.core.builder.switch_to_block(unfinished);
        self.body.core.builder.seal_block(unfinished);
        self.unroot(roots);
        match self.plan.catches[self.pc] {
            Some(catch) => self.catch_raised(status, catch),
            None => self.leave_raised(status),
        }
        self.body.core.builder.switch_to_block(returned);
        self.unroot(roots);
        match result {
            Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), value),
            None => {
                let zero = self.constant(Width::I32, 0);
                self.write(destination, Kind::I32, zero);
            }
        }
    }

    /// The context, the memory arguments, then the call window.
    fn known_arguments(&mut self, instruction: WideInstruction, plan: &Plan) -> Vec<ir::Value> {
        let window = instruction.call_window();
        let mut arguments = vec![self.body.core.context];
        if self.memory.base {
            arguments.push(self.base);
        }
        arguments.extend(self.passed_length());
        for (offset, ty) in plan.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            arguments.push(self.read(window.base + offset as u16, kind));
        }
        arguments
    }

    /// The callee's imported name in this function. Compiled functions share one code buffer:
    /// a direct call, linked at placement (`link`).
    fn known_target(&mut self, callee: usize, plan: &Plan) -> ir::FuncRef {
        if let Some(target) = self.callees[callee] {
            return target;
        }
        let signature = internal_signature(self.memory, &plan.params, plan.result, BODY_CALL_CONV);
        let signature = self.body.core.builder.import_signature(signature);
        let name = self
            .body
            .core
            .builder
            .func
            .declare_imported_user_function(function_name(callee));
        let target = self.body.core.builder.import_function(ir::ExtFuncData {
            name: ir::ExternalName::User(name),
            signature,
            colocated: true,
            patchable: false,
        });
        self.callees[callee] = Some(target);
        target
    }
}

impl Analysis<'_> {
    /// Checks a call step has a compiled denotation in `state` and applies its register
    /// writes; `callable` holds the functions assumed to compile.
    pub(crate) fn transfer_call(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        call: Call,
        callable: &quench_runtime::bitset::BitSet,
    ) -> Option<()> {
        if call.needs_callouts() && !self.callouts {
            return None;
        }
        let registers = function.registers;
        match call {
            Call::IndirectTarget => self.transfer_indirect_target(function, instruction, state),
            Call::RefFunc => {
                self.import_signature(instruction.imm())?;
                state.set(instruction.a(), registers, Slot::Import(instruction.imm()))
            }
            Call::Known => self.transfer_known(function, instruction, state, callable),
            Call::Reference => {
                let signature = match state.get(instruction.b(), registers)? {
                    Slot::Import(import) => self.import_signature(import)?,
                    Slot::IndirectReference { signature, .. } if self.callouts => {
                        self.type_signature(signature)?
                    }
                    _ => return None,
                };
                (signature.results.len() <= 1).then_some(())?;
                self.transfer_window(function, instruction, state, signature)
            }
        }
    }

    /// The reference of an indirect call: a 32-bit index into a table binding, checked against
    /// a signature the program declares.
    fn transfer_indirect_target(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
    ) -> Option<()> {
        let registers = function.registers;
        let Slot::Binding(table) = state.get(instruction.b(), registers)? else {
            return None;
        };
        self.binds_table32(table).then_some(())?;
        (value_kind(state.get(instruction.c(), registers)?)? == Kind::I32).then_some(())?;
        self.type_signature(instruction.imm())?;
        self.type_canonical(instruction.imm())?;
        let reference = Slot::IndirectReference {
            signature: instruction.imm(),
            table,
        };
        state.set(instruction.a(), registers, reference)
    }

    /// A direct call: the callee compiles, or a callout stands in for it.
    fn transfer_known(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        callable: &quench_runtime::bitset::BitSet,
    ) -> Option<()> {
        let callee = u32::from(instruction.known_function_index());
        let compiles = callable.contains(callee as usize);
        let signature = self.signature(callee)?;
        // The VM callout ABI currently carries at most one result: do not admit a residual
        // fallback that would silently discard the remaining Wasm results.
        (compiles || signature.results.len() <= 1).then_some(())?;
        self.transfer_window(function, instruction, state, signature)
    }

    /// The call window holds the signature's parameters; the result register takes its result.
    fn transfer_window(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        signature: &WasmSignature,
    ) -> Option<()> {
        let window = instruction.call_window();
        (usize::from(window.count) == signature.params.len()).then_some(())?;
        for (offset, ty) in signature.params.iter().enumerate() {
            let register = window.base + offset as u16;
            (value_kind(state.get(register, function.registers)?)? == Kind::of(*ty)?)
                .then_some(())?;
        }
        let kind = match signature.results.first() {
            Some(ty) => Kind::of(*ty)?,
            None => Kind::I32,
        };
        state.set(
            instruction.result_register(),
            function.registers,
            Slot::Value(kind),
        )
    }
}

/// A known call's native frame for the one call step: the callee's linked name and the arguments
/// the body convention passes (`known_arguments`). The callee's identity and receiver are folded,
/// its arguments are passed in registers (no window is filled), and the call settles by status
/// (`complete_call`), whose unfinished path the emitter fills with the unwinding (`call_known`).
struct KnownCall {
    target: ir::FuncRef,
    arguments: Vec<ir::Value>,
    /// Whether the call replaces this activation (`return_call`).
    replaces: bool,
    /// The settled call's status and the block a status other than returned takes.
    settled: Option<(ir::Value, ir::Block)>,
}

impl Frame<Emit<'_, '_>> for KnownCall {
    type Fact = ();
    type Control = ();
    fn fact(&self, _register: Register) {}
    fn read(&mut self, _stage: &mut Emit<'_, '_>, _register: Register) -> ir::Value {
        unreachable!("a known call passes its arguments in registers")
    }
    fn write(&mut self, _stage: &mut Emit<'_, '_>, _register: Register, _value: ir::Value, _fact: ()) {
        unreachable!("the emitter writes a known call's result")
    }
    fn this(&mut self, stage: &mut Emit<'_, '_>) -> ir::Value {
        stage.constant(quench_runtime::Value::UNDEFINED.to_bits())
    }
    fn define_this(&mut self, _stage: &mut Emit<'_, '_>, _destination: Register, _this: ir::Value) {
        unreachable!("a Wasm frame has no this")
    }
    fn constant(&mut self, stage: &mut Emit<'_, '_>, constant: Constant) -> (ir::Value, ()) {
        (stage.constant(constant.value.to_bits()), ())
    }
    fn truthiness(&mut self, _stage: &mut Emit<'_, '_>, _register: Register) -> Option<ir::Value> {
        None
    }
    fn branch(&mut self, _stage: &mut Emit<'_, '_>, _condition: ir::Value) {
        unreachable!("a call's frame takes no branch")
    }
    fn jump(&mut self, _stage: &mut Emit<'_, '_>) {
        unreachable!("a call's frame takes no jump")
    }
    fn boolean(&mut self, _stage: &mut Emit<'_, '_>, _condition: ir::Value) -> (ir::Value, ()) {
        unreachable!("a call's frame defines no boolean")
    }
}

impl PropertyFrame<Emit<'_, '_>> for KnownCall {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans {
        NoPlans
    }
}

impl CallFrame<Emit<'_, '_>> for KnownCall {
    /// The callee the record names, folded; it returns with status 0.
    fn call_entry(&self, site: &Invocation) -> CallEntry {
        match site.callee {
            Callee::Known(function) => CallEntry::Direct(DirectPlan {
                kind: CalleeKind::Script,
                identity: Identity::Folded(u64::from(function)),
                sloppy: None,
                registers: 0,
                returned: 0,
                bound: None,
            }),
            _ => CallEntry::Helper,
        }
    }

    /// No file: the arguments are values in registers, so the window the step addresses is never read
    /// (the context stands for it, which emits nothing).
    fn call_registers(&mut self, _stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        Some(self.arguments[0])
    }

    fn call_fill(&self) -> Option<u16> {
        Some(0)
    }

    /// The native stack holds the callee's frame: nothing to admit (the context stands for each word).
    fn call_room(&mut self, _stage: &mut Emit<'_, '_>, _miss: ir::Block) -> Option<Room<ir::Value>> {
        let none = self.arguments[0];
        Some(Room { cell: none, top: none, next: none, shadow: none })
    }

    /// A tail call replaces this activation with the callee's.
    fn call_hand_over(&mut self, stage: &mut Emit<'_, '_>, _this: ir::Value, _room: Room<ir::Value>) -> Option<()> {
        if !self.replaces { return None; }
        stage.tail_call(self.target, &self.arguments);
        Some(())
    }

    /// The direct call of the linked callee: its status, then its result (none for a void callee).
    fn call_enter(&mut self, stage: &mut Emit<'_, '_>, _this: ir::Value, _room: Room<ir::Value>)
        -> Option<Entered<ir::Value, ir::Block>> {
        let results = stage.call_function(self.target, &self.arguments);
        let unfinished = stage.builder.create_block();
        stage.builder.set_cold_block(unfinished);
        Some(Entered { status: results[0], word: results.get(1).copied().unwrap_or(results[0]), unfinished })
    }

    /// Records the status and its unfinished path for the emitter (`call_known`).
    fn call_settle(&mut self, _stage: &mut Emit<'_, '_>, _site: &Invocation, entered: Entered<ir::Value, ir::Block>,
        _room: Room<ir::Value>, _value: ir::Value) -> Option<()> {
        self.settled = Some((entered.status, entered.unfinished));
        Some(())
    }
}
