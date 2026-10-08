//! Table operators use ordinary VM references and captured instance bindings.

use super::{Lowering, Reachability, WasmImportName, WasmReferenceInitializer};
use crate::{
    Diagnostic,
    bytecode::{ImmediateLayout, Op, Register},
};
use wasmparser::{Operator, RefType, TableType};

/// A table's limits/type and its single evaluated-per-instance initializer.
#[derive(Clone)]
pub struct WasmTable {
    pub ty: TableType,
    pub initializer: WasmTableInitializer,
}

/// Allocation/initialization and import binding are distinct instance transitions.
#[derive(Clone)]
pub enum WasmTableInitializer {
    Reference(WasmReferenceInitializer),
    Import(WasmImportName),
}

impl From<WasmReferenceInitializer> for WasmTableInitializer {
    fn from(value: WasmReferenceInitializer) -> Self {
        Self::Reference(value)
    }
}

impl From<TableType> for WasmTable {
    fn from(ty: TableType) -> Self {
        Self {
            ty,
            initializer: WasmReferenceInitializer::Null.into(),
        }
    }
}

pub(crate) const TABLE32_MAX_ELEMENTS: u64 = u32::MAX as u64;

pub(crate) fn table_index_limit(table64: bool) -> u64 {
    if table64 {
        u64::MAX
    } else {
        TABLE32_MAX_ELEMENTS
    }
}

pub(crate) fn supported_table(table: &TableType, types: &super::WasmTypes) -> bool {
    !table.shared
        && types
            .callable_value_type(wasmparser::ValType::Ref(table.element_type))
            .is_some()
        && table.initial <= table_index_limit(table.table64)
        && table.maximum.is_none_or(|maximum| {
            maximum >= table.initial && maximum <= table_index_limit(table.table64)
        })
}

impl Lowering<'_> {
    pub(super) fn table_binding(&mut self, index: u32) -> Result<Register, Diagnostic> {
        self.table_type(index)?;
        self.instance_binding(
            self.globals.len() + self.memories.len() + self.data.len() + index as usize,
        )
    }

    pub(super) fn table_type(&self, index: u32) -> Result<RefType, Diagnostic> {
        self.tables
            .get(index as usize)
            .map(|table| table.ty.element_type)
            .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm table index out of bounds"))
    }

    pub(super) fn table_operator(&mut self, operator: &Operator<'_>) -> Result<bool, Diagnostic> {
        match *operator {
            Operator::CallIndirect { type_index, .. }
            | Operator::ReturnCallIndirect { type_index, .. }
            | Operator::CallRef { type_index }
            | Operator::ReturnCallRef { type_index } => {
                let table = match *operator {
                    Operator::CallIndirect { table_index, .. }
                    | Operator::ReturnCallIndirect { table_index, .. } => {
                        if self
                            .signatures
                            .declarations
                            .callable_value_type(wasmparser::ValType::Ref(
                                self.table_type(table_index)?,
                            ))
                            .is_none_or(|ty| {
                                matches!(
                                    ty,
                                    super::WasmType::Reference {
                                        kind: super::WasmReferenceKind::External,
                                        ..
                                    }
                                )
                            })
                        {
                            return Err(Diagnostic::unsupported(
                                self.name,
                                "unsupported indirect-call table type",
                            ));
                        }
                        Some(table_index)
                    }
                    _ => None,
                };
                let signature = self
                    .signatures
                    .intern(self.name, super::WasmCallableType::Declared(type_index))?;
                let shape = self.signatures.type_signature(signature).unwrap();
                let params = u16::try_from(shape.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many reference-call parameters")
                })?;
                let results = u16::try_from(shape.results.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many reference-call results")
                })?;
                if self.path == Reachability::Live {
                    let target = self.pop()?;
                    if let Some(table_index) = table {
                        let depth = self.depth;
                        self.depth = target + 1;
                        let table = self.table_binding(table_index)?;
                        self.emit(Op::WasmIndirectTarget, target, table, target, signature)?;
                        self.depth = depth;
                    } else {
                        // Validation proves the callable type; preserve the original function/capture.
                        self.emit(
                            Op::WasmRefAsNonNull,
                            target,
                            target,
                            0,
                            super::reference::NonNullCheck::Function as u32,
                        )?;
                    }
                    let tail = matches!(
                        operator,
                        Operator::ReturnCallIndirect { .. } | Operator::ReturnCallRef { .. }
                    );
                    self.emit_wasm_call(
                        params,
                        results,
                        super::WasmCallTarget::Reference(target),
                        tail,
                    )?;
                }
            }
            Operator::RefNull { .. } => {
                if self.path == Reachability::Live {
                    let result = self.push()?;
                    // Heap type is a validation fact; the runtime null value is uniform.
                    let constant = self.append_constant(crate::bytecode::Constant::Null)?;
                    self.emit(Op::LoadConst, result, 0, 0, constant)?;
                }
            }
            Operator::RefFunc { function_index } => {
                self.signatures
                    .get(function_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm function reference out of bounds")
                    })?;
                if self.path == Reachability::Live {
                    let result = self.push()?;
                    self.emit(Op::WasmRefFunc, result, 0, 0, function_index)?;
                }
            }
            Operator::RefIsNull | Operator::RefAsNonNull => {
                if self.path == Reachability::Live {
                    let input = self.pop()?;
                    let result = self.push()?;
                    let (op, immediate) = match operator {
                        Operator::RefIsNull => (Op::WasmRefIsNull, 0),
                        _ => (
                            Op::WasmRefAsNonNull,
                            super::reference::NonNullCheck::Reference as u32,
                        ),
                    };
                    self.emit(op, result, input, 0, immediate)?;
                }
            }
            Operator::TableGet { table } | Operator::TableSet { table } => {
                self.table_type(table)?;
                if self.path == Reachability::Live {
                    let write = matches!(operator, Operator::TableSet { .. });
                    let value = if write { Some(self.pop()?) } else { None };
                    let index = self.pop()?;
                    let depth = self.depth;
                    self.depth = index + if write { 2 } else { 1 };
                    let table = self.table_binding(table)?;
                    self.emit(
                        if write {
                            Op::WasmTableSet
                        } else {
                            Op::WasmTableGet
                        },
                        value.unwrap_or(index),
                        table,
                        index,
                        0,
                    )?;
                    self.depth = depth;
                    if !write {
                        self.push()?;
                    }
                }
            }
            Operator::TableSize { table } | Operator::TableGrow { table } => {
                self.table_type(table)?;
                if self.path == Reachability::Live {
                    let grow = matches!(operator, Operator::TableGrow { .. });
                    let operands = if grow {
                        Some((self.pop()?, self.pop()?))
                    } else {
                        None
                    };
                    let result = self.push()?;
                    let depth = self.depth;
                    if let Some((delta, _)) = operands {
                        self.depth = delta + 1;
                    }
                    let table = self.table_binding(table)?;
                    match operands {
                        Some((delta, initial)) => self.emit(
                            Op::WasmTableGrow,
                            result,
                            table,
                            initial,
                            ImmediateLayout::register_pair_immediate(delta, delta),
                        )?,
                        None => self.emit(Op::WasmTableSize, result, table, 0, 0)?,
                    }
                    self.depth = depth;
                }
            }
            Operator::TableFill { table }
            | Operator::TableCopy {
                dst_table: table, ..
            } => {
                let ty = self.table_type(table)?;
                let source = if let Operator::TableCopy { src_table, .. } = *operator {
                    let source_type = self.table_type(src_table)?;
                    if !self.signatures.declarations.reference_subtype(
                        source_type,
                        &self.signatures.declarations,
                        ty,
                    ) {
                        return Err(Diagnostic::unsupported(
                            self.name,
                            "incompatible Wasm table copy types",
                        ));
                    }
                    Some(src_table)
                } else {
                    None
                };
                if self.path == Reachability::Live {
                    let length = self.pop()?;
                    let input = self.pop()?;
                    let output = self.pop()?;
                    let depth = self.depth;
                    self.depth = length + 1;
                    let table = self.table_binding(table)?;
                    let source = match source {
                        Some(index) => self.table_binding(index)?,
                        None => input,
                    };
                    let copy = matches!(operator, Operator::TableCopy { .. });
                    self.emit(
                        if copy {
                            Op::WasmTableCopy
                        } else {
                            Op::WasmTableFill
                        },
                        table,
                        source,
                        length,
                        ImmediateLayout::register_pair_immediate(
                            output,
                            if copy { input } else { output },
                        ),
                    )?;
                    self.depth = depth;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
