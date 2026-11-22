//! The Wasm tier's calls: the lowering of each `Call` step (a direct call linked at placement,
//! a callout, an indirect call through the VM boundary) and the analysis of its callee and
//! arguments. An indirect target's checks (`staged::wasm_call::indirect_target`) and its call
//! are emitted over the table's mirror (`wasm_table_stage`).
//!
//! A tail call (`is_tail_call`: a call whose destination is flagged as a return) is a true tail
//! call when its callee is compiled natively and returns the same result as this function: a
//! `return_call` (here, `tail_known`) or a `return_call_indirect` (the native fast path of
//! `wasm_table_stage`). Bodies use `BODY_CALL_CONV` (`Tail`) so the convention allows it. The
//! other shapes (a callout, a VM call-out, a foreign instance, a result of another type) are a
//! call that writes the result, followed by the return of that result (`return_if_tail`), as
//! before. The b-jit's stitched tail calls still grow the native stack (`wasm_stitch_tail`).

use cranelift_codegen::ir::InstBuilder;
use super::wasm_instance_stage::value_kind;
use super::{
    Analysis, AnalysisState, BODY_CALL_CONV, Emitter, Kind, Plan, Slot, function_name,
    internal_signature,
};
use quench_runtime::WasmSignature;
use quench_runtime::bytecode::{Function, WideInstruction};
use quench_runtime::staged::machine::{MachineAccess, MachineStage};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_call::Call;
use quench_runtime::staged::wasm_step::Step;
use cranelift_codegen::ir;

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
    /// A tail call that was a true tail call already left (`tail_known`, `call_reference`).
    pub(crate) fn return_if_tail(&mut self, instruction: WideInstruction) {
        if is_tail_call(instruction) {
            self.return_register(instruction.result_register());
        }
    }

    /// A direct call of a function the program defines, or its callout where it did not compile.
    fn call_known(&mut self, instruction: WideInstruction) {
        let callee = usize::from(instruction.known_function_index());
        let Some(plan) = self.plans[callee].as_ref() else {
            self.callout_known_fallback(callee as u32, instruction);
            self.return_if_tail(instruction);
            return;
        };
        if is_tail_call(instruction) && plan.result == self.plan.result {
            self.tail_known(callee, plan, instruction);
            return;
        }
        let arguments = self.known_arguments(instruction, plan);
        let target = self.known_target(callee, plan);
        let roots = self.root();
        let results = self.call_function(target, &arguments);
        self.unroot(roots);
        self.propagate(results[0]);
        let destination = instruction.result_register();
        match plan.result {
            Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), results[1]),
            None => {
                let zero = self.constant(Width::I32, 0);
                self.write(destination, Kind::I32, zero);
            }
        }
        self.return_if_tail(instruction);
    }

    /// A tail call of compiled `callee`, which returns the same result: a `return_call` with the
    /// arguments the ordinary call passes. The frame is replaced, so the callee's status and
    /// result are this function's, and the call returns nothing here. No roots are pushed: a
    /// root surrounds only a call that returns, and a tail call leaves the frame's references to
    /// the callee, which roots its own before its calls.
    fn tail_known(&mut self, callee: usize, plan: &Plan, instruction: WideInstruction) {
        let arguments = self.known_arguments(instruction, plan);
        let target = self.known_target(callee, plan);
        self.interrupt_safepoint();
        self.body.core.builder.ins().return_call(target, &arguments);
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
        let rooted = self.experimental_root_compilation && !(self.root_only && !self.callouts);
        (compiles || rooted).then_some(())?;
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
