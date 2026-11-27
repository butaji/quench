//! `SetFunctionName` in the specializer: where the site recorded the naming (`Vm::native_function_name`,
//! `OnlineFacts::names`) the function in the instruction's register is named by `staged::naming`'s row
//! over the recorded edge, and a miss leaves at the instruction, where the interpreter names the function
//! and records the edge (the exact fallback). The row holds no Stable fact: the edge is an append-only
//! shape transition, the name's string a kept program atom, and what varies per function (its shape,
//! empty name, storage, the function space's store gate) the row tests.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::{Stage, add::Edge, naming::{name_form, site_name}};

impl Lowering<'_, '_> {
    /// The staged naming of the function `instruction` names, or `None` (nothing emitted, so the
    /// instruction leaves) where the site recorded none or the register is outside the unit.
    pub(crate) fn function_name(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let source = instruction.register_a();
        let names = self.vm.online?.names?;
        if source >= self.unit.registers {
            return None;
        }
        let (transition, word) = names(instruction.cache_site_index(), instruction.atom_index())?;
        let properties = self.builder.use_var(self.entry.as_ref()?.receiver.properties);
        let exit = self.guard_exit(state, pc as u32);
        let function = self.tagged(state, source);
        let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.function_record_base));
        let empty = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.empty_string));
        let mut stage = super::object_slots::Emit { builder: &mut self.builder, property_values: Some(properties), gate: Some(self.payload) , payload: None, facts: Default::default() };
        let Edge { before, after, slot } = transition.edge();
        let edge = Edge { before: stage.constant(before), after: stage.constant(after), slot: stage.constant(slot) };
        let name = stage.constant(word);
        site_name(&mut stage, records, function, (edge, empty), name_form(transition.slot), name, exit)?;
        self.reshaped(state);
        Some(true)
    }
}
