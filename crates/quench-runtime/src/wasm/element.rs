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
    pub items: WasmElementItems,
}

/// A segment's items: a function-index list, as toolchains emit for their indirect-call
/// tables (dotnet-aot-wasm: 33K items), is held at 4 bytes an item.
#[derive(Clone)]
pub enum WasmElementItems {
    Functions(Rc<[u32]>),
    Initializers(Rc<[WasmReferenceInitializer]>),
}

impl WasmElementItems {
    pub fn len(&self) -> usize {
        match self {
            Self::Functions(functions) => functions.len(),
            Self::Initializers(items) => items.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = std::borrow::Cow<'_, WasmReferenceInitializer>> {
        let (functions, items) = match self {
            Self::Functions(functions) => (&functions[..], &[][..]),
            Self::Initializers(items) => (&[][..], &items[..]),
        };
        functions
            .iter()
            .map(|function| std::borrow::Cow::Owned(WasmReferenceInitializer::Function(*function)))
            .chain(items.iter().map(std::borrow::Cow::Borrowed))
    }
}

impl From<Vec<WasmReferenceInitializer>> for WasmElementItems {
    fn from(items: Vec<WasmReferenceInitializer>) -> Self {
        let functions: Option<Rc<[u32]>> = items
            .iter()
            .map(|item| match item {
                WasmReferenceInitializer::Function(function) => Some(*function),
                _ => None,
            })
            .collect();
        match functions {
            Some(functions) => Self::Functions(functions),
            None => Self::Initializers(items.into()),
        }
    }
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
        self.emit(Op::WasmSegmentDrop, 0, 0, 0, u32::from(slot))?;
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
