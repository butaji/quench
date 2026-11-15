//! GC operations use the common heap and activation registers.
use wasmparser::{FieldType, StorageType};

mod field;
pub(crate) use field::GcField;
mod kinds;
pub(crate) use kinds::{ArraySegmentInput, ArraySegmentKind, GcFieldAccess, StructConstruction, storage_subtype};

use super::*;
impl Lowering<'_> {
    fn array_constructor(
        &mut self,
        op: Op,
        index: u32,
        fixed: Option<u32>,
    ) -> Result<(), Diagnostic> {
        let field = self
            .signatures
            .declarations
            .array_field(index)
            .ok_or_else(|| {
                Diagnostic::unsupported(self.name, "unsupported Wasm array declaration")
            })?;
        if let StorageType::Val(ty) = field.element_type
            && (self
                .signatures
                .declarations
                .callable_value_type(ty)
                .is_none()
                || op == Op::WasmArrayNewDefault && !ty.is_defaultable())
            {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "unsupported Wasm array element",
                ));
            }
        if self.path == Reachability::Live {
            let (base, extra) = match fixed {
                Some(count) => {
                    let count = u16::try_from(count).map_err(|_| {
                        Diagnostic::unsupported(
                            self.name,
                            "Wasm fixed array window exceeds register layout",
                        )
                    })?;
                    let base = self
                        .depth
                        .checked_sub(count)
                        .filter(|&base| base >= self.control_base())
                        .ok_or_else(|| {
                            Diagnostic::unsupported(self.name, "missing Wasm fixed array elements")
                        })?;
                    self.depth = base;
                    (base, count)
                }
                None => {
                    let count = self.pop()?;
                    if op == Op::WasmArrayNew {
                        (self.pop()?, count)
                    } else {
                        (count, 0)
                    }
                }
            };
            let result = self.push()?;
            self.emit(op, result, base, extra, index)?;
        }
        Ok(())
    }

    pub(super) fn gc_operator(&mut self, operator: &Operator<'_>) -> Result<bool, Diagnostic> {
        let array_access = match *operator {
            Operator::ArrayGet { array_type_index } => {
                Some((GcFieldAccess::Read, array_type_index))
            }
            Operator::ArrayGetS { array_type_index } => {
                Some((GcFieldAccess::ReadSigned, array_type_index))
            }
            Operator::ArrayGetU { array_type_index } => {
                Some((GcFieldAccess::ReadUnsigned, array_type_index))
            }
            Operator::ArraySet { array_type_index } => {
                Some((GcFieldAccess::Write, array_type_index))
            }
            _ => None,
        };
        if let Some((access, index)) = array_access {
            let field = self
                .signatures
                .declarations
                .array_field(index)
                .filter(|field| access.supports(*field))
                .ok_or_else(|| {
                    Diagnostic::unsupported(self.name, "invalid Wasm array element access")
                })?;
            if let StorageType::Val(ty) = field.element_type
                && self
                    .signatures
                    .declarations
                    .callable_value_type(ty)
                    .is_none()
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "unsupported Wasm array element",
                    ));
                }
            if self.path == Reachability::Live {
                let value = if access == GcFieldAccess::Write {
                    Some(self.pop()?)
                } else {
                    None
                };
                let element = self.pop()?;
                let reference = self.pop()?;
                let (a, b, c) = match value {
                    Some(value) => (reference, element, value),
                    None => (self.push()?, reference, element),
                };
                self.emit(access.array_op(), a, b, c, GcField::new(0, field.element_type).expect("index 0 fits").tag())?;
            }
            return Ok(true);
        }
        match *operator {
            Operator::ArrayNewData {
                array_type_index,
                array_data_index,
            } => {
                self.array_segment(
                    ArraySegmentKind::Data,
                    true,
                    array_type_index,
                    array_data_index,
                )?;
                return Ok(true);
            }
            Operator::ArrayNewElem {
                array_type_index,
                array_elem_index,
            } => {
                self.array_segment(
                    ArraySegmentKind::Element,
                    true,
                    array_type_index,
                    array_elem_index,
                )?;
                return Ok(true);
            }
            Operator::ArrayInitData {
                array_type_index,
                array_data_index,
            } => {
                self.array_segment(
                    ArraySegmentKind::Data,
                    false,
                    array_type_index,
                    array_data_index,
                )?;
                return Ok(true);
            }
            Operator::ArrayInitElem {
                array_type_index,
                array_elem_index,
            } => {
                self.array_segment(
                    ArraySegmentKind::Element,
                    false,
                    array_type_index,
                    array_elem_index,
                )?;
                return Ok(true);
            }
            Operator::ArrayFill { array_type_index } => {
                self.array_bulk(array_type_index, None)?;
                return Ok(true);
            }
            Operator::ArrayCopy {
                array_type_index_dst,
                array_type_index_src,
            } => {
                self.array_bulk(array_type_index_dst, Some(array_type_index_src))?;
                return Ok(true);
            }
            Operator::ArrayNew { array_type_index } => {
                return self
                    .array_constructor(Op::WasmArrayNew, array_type_index, None)
                    .map(|()| true);
            }
            Operator::ArrayNewDefault { array_type_index } => {
                return self
                    .array_constructor(Op::WasmArrayNewDefault, array_type_index, None)
                    .map(|()| true);
            }
            Operator::ArrayNewFixed {
                array_type_index,
                array_size,
            } => {
                return self
                    .array_constructor(Op::WasmArrayNewFixed, array_type_index, Some(array_size))
                    .map(|()| true);
            }
            Operator::ArrayLen => {
                if self.path == Reachability::Live {
                    let input = self.pop()?;
                    let result = self.push()?;
                    self.emit(Op::WasmArrayLen, result, input, 0, 0)?;
                }
                return Ok(true);
            }
            _ => {}
        }
        let field = match *operator {
            Operator::StructGet {
                struct_type_index,
                field_index,
            } => Some((GcFieldAccess::Read, struct_type_index, field_index)),
            Operator::StructGetS {
                struct_type_index,
                field_index,
            } => Some((GcFieldAccess::ReadSigned, struct_type_index, field_index)),
            Operator::StructGetU {
                struct_type_index,
                field_index,
            } => Some((GcFieldAccess::ReadUnsigned, struct_type_index, field_index)),
            Operator::StructSet {
                struct_type_index,
                field_index,
            } => Some((GcFieldAccess::Write, struct_type_index, field_index)),
            _ => None,
        };
        if let Some((access, index, field)) = field {
            return self.struct_field(access, index, field).map(|()| true);
        }
        if let Operator::RefGetDesc { type_index } = *operator {
            if self
                .signatures
                .declarations
                .descriptor_type(type_index)
                .is_none()
            {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "Wasm type has no descriptor",
                ));
            }
            if self.path == Reachability::Live {
                let source = self.pop()?;
                let result = self.push()?;
                self.emit(Op::WasmRefGetDesc, result, source, 0, type_index)?;
            }
            return Ok(true);
        }
        let Some((index, mode)) = StructConstruction::from_operator(operator) else {
            return Ok(false);
        };
        if mode.described()
            != self
                .signatures
                .declarations
                .descriptor_type(index)
                .is_some()
        {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm struct constructor descriptor mismatch",
            ));
        }
        let default = mode.defaulted();
        let fields = self
            .signatures
            .declarations
            .struct_fields(index)
            .ok_or_else(|| {
                Diagnostic::unsupported(self.name, "unsupported Wasm struct declaration")
            })?;
        for field in fields {
            if let wasmparser::StorageType::Val(ty) = field.element_type
                && (self
                    .signatures
                    .declarations
                    .callable_value_type(ty)
                    .is_none()
                    || default && !ty.is_defaultable())
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "unsupported Wasm struct field",
                    ));
                }
        }
        if self.path == Reachability::Live {
            let inputs = if default {
                0
            } else {
                u16::try_from(fields.len()).map_err(|_| {
                    Diagnostic::unsupported(
                        self.name,
                        "Wasm struct input window exceeds register layout",
                    )
                })?
            };
            let inputs = inputs
                .checked_add(u16::from(mode.described()))
                .ok_or_else(|| {
                    Diagnostic::unsupported(
                        self.name,
                        "Wasm struct input window exceeds register layout",
                    )
                })?;
            let base = self
                .depth
                .checked_sub(inputs)
                .filter(|&base| base >= self.control_base())
                .ok_or_else(|| Diagnostic::unsupported(self.name, "missing Wasm struct fields"))?;
            self.depth = base;
            let result = self.push()?;
            self.emit(
                mode.op(),
                result,
                if mode == StructConstruction::Default {
                    0
                } else {
                    base
                },
                inputs,
                index,
            )?;
        }
        Ok(true)
    }
    fn array_segment(
        &mut self,
        kind: ArraySegmentKind,
        construct: bool,
        index: u32,
        segment: u32,
    ) -> Result<(), Diagnostic> {
        let field = self
            .signatures
            .declarations
            .array_field(index)
            .filter(|field| (construct || field.mutable) && kind.accepts(field.element_type))
            .ok_or_else(|| {
                Diagnostic::unsupported(self.name, "invalid Wasm array segment storage")
            })?;
        let slot = match kind {
            ArraySegmentKind::Data => self.data_slot(segment)?,
            ArraySegmentKind::Element => {
                let slot = self.element_slot(segment)?;
                let StorageType::Val(wasmparser::ValType::Ref(target)) = field.element_type else {
                    unreachable!("reference storage")
                };
                if !self.signatures.declarations.reference_subtype(
                    self.elements[segment as usize].element_type,
                    &self.signatures.declarations,
                    target,
                ) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "incompatible Wasm array element segment",
                    ));
                }
                slot
            }
        };
        if self.path == Reachability::Live {
            let count = self.pop()?;
            let input = self.pop()?;
            if construct {
                // Offset/count are existing contiguous stack slots; append the rooted segment binding.
                self.depth = count + 1;
                self.instance_binding(usize::from(slot))?;
                self.depth = input;
                let result = self.push()?;
                self.emit(
                    kind.new_op(),
                    result,
                    input,
                    ArraySegmentInput::COUNT,
                    index,
                )?;
            } else {
                let output = self.pop()?;
                let array = self.pop()?;
                let depth = self.depth;
                self.depth = count + 1;
                let segment = self.instance_binding(usize::from(slot))?;
                self.emit(
                    kind.init_op(),
                    array,
                    segment,
                    count,
                    crate::bytecode::ImmediateLayout::register_pair_immediate(output, input),
                )?;
                self.depth = depth;
            }
        }
        Ok(())
    }

    fn array_bulk(&mut self, destination: u32, source: Option<u32>) -> Result<(), Diagnostic> {
        let owner = &self.signatures.declarations;
        let destination = owner
            .array_field(destination)
            .filter(|field| field.mutable)
            .ok_or_else(|| {
                Diagnostic::unsupported(self.name, "invalid Wasm array bulk destination")
            })?;
        if let Some(source) = source {
            let source = owner.array_field(source).ok_or_else(|| {
                Diagnostic::unsupported(self.name, "invalid Wasm array bulk source")
            })?;
            if !storage_subtype(source.element_type, owner, destination.element_type, owner) {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "incompatible Wasm array copy storage",
                ));
            }
        } else if let StorageType::Val(ty) = destination.element_type
            && owner.callable_value_type(ty).is_none() {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "unsupported Wasm array fill storage",
                ));
            }
        if self.path == Reachability::Live {
            let count = self.pop()?;
            let (input, source_value) = if source.is_some() {
                let input = self.pop()?;
                (Some(input), self.pop()?)
            } else {
                (None, self.pop()?)
            };
            let output = self.pop()?;
            let destination = self.pop()?;
            self.emit(
                if source.is_some() {
                    Op::WasmArrayCopy
                } else {
                    Op::WasmArrayFill
                },
                destination,
                source_value,
                count,
                crate::bytecode::ImmediateLayout::register_pair_immediate(
                    output,
                    input.unwrap_or(output),
                ),
            )?;
        }
        Ok(())
    }

    fn struct_field(
        &mut self,
        access: GcFieldAccess,
        index: u32,
        field: u32,
    ) -> Result<(), Diagnostic> {
        let declaration = self
            .signatures
            .declarations
            .struct_fields(index)
            .and_then(|fields| fields.get(field as usize))
            .copied()
            .filter(|field| access.supports(*field))
            .ok_or_else(|| {
                Diagnostic::unsupported(self.name, "invalid Wasm struct field access")
            })?;
        if let StorageType::Val(ty) = declaration.element_type
            && self
                .signatures
                .declarations
                .callable_value_type(ty)
                .is_none()
            {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "unsupported Wasm struct field",
                ));
            }
        if self.path == Reachability::Live {
            let value = if access == GcFieldAccess::Write {
                Some(self.pop()?)
            } else {
                None
            };
            let reference = self.pop()?;
            let (a, b) = match value {
                Some(value) => (reference, value),
                None => (self.push()?, reference),
            };
            // Subtype prefixes keep packed types, so the storage holds for every live object.
            let field = GcField::new(field, declaration.element_type).ok_or_else(|| Diagnostic::unsupported(self.name, "invalid Wasm struct field"))?;
            self.emit(access.op(), a, b, 0, field.tag())?;
        }
        Ok(())
    }
}
