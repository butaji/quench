//! Static element facts and the available/dropped instance transition.

use super::{Lowering, Reachability, WasmReferenceInitializer};
use crate::{
    Diagnostic,
    bytecode::{ImmediateLayout, Op},
};
use std::rc::Rc;

#[derive(Clone)]
pub enum WasmElementMode {
    Active {
        table: u32,
        offset: super::WasmConstantExpression,
    },
    Passive,
    Declared,
}

#[derive(Clone)]
pub struct WasmElement {
    pub mode: WasmElementMode,
    pub element_type: wasmparser::RefType,
    pub items: Rc<[WasmReferenceInitializer]>,
}

impl Lowering<'_> {
    pub(super) fn element_slot(&self, index: u32) -> Result<u16, Diagnostic> {
        self.elements.get(index as usize).ok_or_else(|| {
            Diagnostic::unsupported(self.name, "Wasm element index out of bounds")
        })?;
        Ok((self.globals.len()
            + self.memories.len()
            + self.data.len()
            + self.tables.len()
            + index as usize) as u16)
    }

    pub(super) fn drop_instance_binding(&mut self, slot: u16) -> Result<(), Diagnostic> {
        if self.path == Reachability::Dead {
            return Ok(());
        }
        let depth = self.depth;
        let dropped = self.push()?;
        self.emit(Op::LoadConst, dropped, 0, 0, super::VOID_RESULT_CONSTANT)?;
        self.emit(
            Op::StoreCapture,
            dropped,
            0,
            0,
            ImmediateLayout::capture_immediate(0, slot),
        )?;
        self.depth = depth;
        Ok(())
    }

    pub(super) fn element_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, Diagnostic> {
        match *operator {
            wasmparser::Operator::ElemDrop { elem_index } => {
                self.drop_instance_binding(self.element_slot(elem_index)?)?;
            }
            wasmparser::Operator::TableInit { elem_index, table } => {
                let slot = self.element_slot(elem_index)?;
                let table_type = self.table_type(table)?;
                if !self.signatures.declarations.reference_subtype(
                    self.elements[elem_index as usize].element_type,
                    &self.signatures.declarations,
                    table_type,
                ) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "incompatible Wasm element and table types",
                    ));
                }
                if self.path == Reachability::Live {
                    let length = self.pop()?;
                    let input = self.pop()?;
                    let output = self.pop()?;
                    let depth = self.depth;
                    self.depth = length + 1;
                    let table = self.table_binding(table)?;
                    let elements = self.instance_binding(usize::from(slot))?;
                    self.emit(
                        Op::WasmTableInit,
                        table,
                        elements,
                        length,
                        ImmediateLayout::register_pair_immediate(output, input),
                    )?;
                    self.depth = depth;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
