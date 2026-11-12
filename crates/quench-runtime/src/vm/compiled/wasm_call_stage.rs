//! The Wasm tier's calls: the lowering of each `Call` step (a direct call linked at placement,
//! a callout, an indirect call through the VM boundary) and the analysis of its callee and
//! arguments. An indirect target's checks (`staged::wasm_call::indirect_target`) run in the
//! interpreter's callout, so the tier keeps the table index in native code.

use super::wasm_instance_stage::value_kind;
use super::{
    Analysis, AnalysisState, Emitter, Host, Kind, Plan, Slot, function_name, internal_signature,
};
use crate::WasmSignature;
use crate::bytecode::{Function, WideInstruction};
use crate::staged::wasm_call::Call;
use cranelift_codegen::ir::{self, InstBuilder, types};

impl<H: Host> Emitter<'_, '_, H> {
    /// Lowers a call step.
    pub(super) fn call(&mut self, pc: usize, instruction: WideInstruction, call: Call) {
        match call {
            Call::Known => self.call_known(instruction),
            Call::IndirectTarget => self.indirect_target(pc, instruction),
            // A function reference is known statically per call site (`Slot::Import`).
            Call::RefFunc => {}
            Call::Reference => match self.plan.operand(pc) {
                Slot::Import(_) => self.callout(pc, instruction),
                Slot::IndirectReference { .. } => self.call_reference(pc, instruction),
                _ => unreachable!("planned calls hold a supported callable"),
            },
        }
    }

    /// A direct call of a function the program defines, or its callout where it did not compile.
    fn call_known(&mut self, instruction: WideInstruction) {
        let callee = usize::from(instruction.known_function_index());
        let Some(plan) = self.plans[callee].as_ref() else {
            self.callout_known_fallback(callee as u32, instruction);
            return;
        };
        let arguments = self.known_arguments(instruction, plan);
        let target = self.known_target(callee, plan);
        let call = self.body.core.builder.ins().call(target, &arguments);
        let results = self.body.core.builder.inst_results(call).to_vec();
        self.propagate(results[0]);
        let destination = instruction.result_register();
        match plan.result {
            Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), results[1]),
            None => {
                let zero = self.body.core.builder.ins().iconst(types::I32, 0);
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
        arguments.extend(self.length);
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
        let signature = internal_signature(self.memory, &plan.params, plan.result, self.call_conv);
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

impl<H: Host> Analysis<'_, H> {
    /// Checks a call step has a compiled denotation in `state` and applies its register
    /// writes; `callable` holds the functions assumed to compile.
    pub(super) fn transfer_call(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        call: Call,
        callable: &crate::bitset::BitSet,
    ) -> Option<()> {
        if call.needs_callouts() && !self.callouts {
            return None;
        }
        let registers = function.registers;
        match call {
            Call::IndirectTarget => {
                let Slot::Binding(table) = state.get(instruction.b(), registers)? else {
                    return None;
                };
                self.binds_table32(table).then_some(())?;
                (value_kind(state.get(instruction.c(), registers)?)? == Kind::I32).then_some(())?;
                self.type_signature(instruction.imm())?;
                let reference = Slot::IndirectReference {
                    signature: instruction.imm(),
                    table,
                };
                state.set(instruction.a(), registers, reference)
            }
            Call::RefFunc => {
                self.import_signature(instruction.imm())?;
                state.set(instruction.a(), registers, Slot::Import(instruction.imm()))
            }
            Call::Known => {
                let callee = u32::from(instruction.known_function_index());
                let compiles = callable.contains(callee as usize);
                if !compiles
                    && (!self.experimental_root_compilation || (self.root_only && !self.callouts))
                {
                    return None;
                }
                let signature = self.signature(callee)?;
                // The VM callout ABI currently carries at most one result: do not admit a
                // residual fallback that would silently discard the remaining Wasm results.
                (compiles || signature.results.len() <= 1).then_some(())?;
                self.transfer_window(function, instruction, state, signature)
            }
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
