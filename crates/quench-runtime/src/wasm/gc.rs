//! GC operations use the common heap and activation registers.
use crate::Value;
use wasmparser::{FieldType, StorageType};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GcFieldAccess {
    Read,
    ReadSigned,
    ReadUnsigned,
    Write,
}

impl GcFieldAccess {
    const OPS: &[(Self, Op, Op)] = &[
        (Self::Read, Op::WasmStructGet, Op::WasmArrayGet),
        (Self::ReadSigned, Op::WasmStructGetS, Op::WasmArrayGetS),
        (Self::ReadUnsigned, Op::WasmStructGetU, Op::WasmArrayGetU),
        (Self::Write, Op::WasmStructSet, Op::WasmArraySet),
    ];

    pub(crate) fn op(self) -> Op {
        Self::OPS
            .iter()
            .find(|(access, _, _)| *access == self)
            .unwrap()
            .1
    }

    fn array_op(self) -> Op {
        Self::OPS
            .iter()
            .find(|(access, _, _)| *access == self)
            .unwrap()
            .2
    }

    pub(crate) fn from_op(op: Op) -> Option<Self> {
        Self::OPS
            .iter()
            .find(|(_, structure, array)| *structure == op || *array == op)
            .map(|(access, _, _)| *access)
    }

    pub(crate) fn supports(self, field: FieldType) -> bool {
        match self {
            Self::Write => field.mutable,
            Self::Read => matches!(field.element_type, StorageType::Val(_)),
            Self::ReadSigned | Self::ReadUnsigned => {
                matches!(field.element_type, StorageType::I8 | StorageType::I16)
            }
        }
    }

    pub(crate) fn read(self, storage: StorageType, value: Value) -> Option<Value> {
        Some(match (self, storage) {
            (Self::Read, StorageType::Val(_)) => value,
            (Self::ReadSigned, StorageType::I8) => Value::integer(i32::from(value.as_int()? as i8)),
            (Self::ReadSigned, StorageType::I16) => {
                Value::integer(i32::from(value.as_int()? as i16))
            }
            (Self::ReadUnsigned, StorageType::I8 | StorageType::I16) => value,
            _ => return None,
        })
    }
}
pub(crate) fn storage_subtype(
    source: StorageType,
    source_owner: &super::WasmTypes,
    destination: StorageType,
    destination_owner: &super::WasmTypes,
) -> bool {
    match (source, destination) {
        (StorageType::I8, StorageType::I8) | (StorageType::I16, StorageType::I16) => true,
        (StorageType::Val(source), StorageType::Val(destination)) => {
            match (
                source_owner.callable_value_type(source),
                destination_owner.callable_value_type(destination),
            ) {
                (Some(source), Some(destination)) => {
                    source_owner.value_subtype(source, destination_owner, destination)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArraySegmentKind {
    Data,
    Element,
}

impl ArraySegmentKind {
    const OPS: &[(Self, Op, Op)] = &[
        (Self::Data, Op::WasmArrayNewData, Op::WasmArrayInitData),
        (Self::Element, Op::WasmArrayNewElem, Op::WasmArrayInitElem),
    ];

    pub(crate) fn new_op(self) -> Op {
        Self::OPS
            .iter()
            .find(|(kind, _, _)| *kind == self)
            .unwrap()
            .1
    }
    fn init_op(self) -> Op {
        Self::OPS
            .iter()
            .find(|(kind, _, _)| *kind == self)
            .unwrap()
            .2
    }
    pub(crate) fn from_op(op: Op) -> Option<Self> {
        Self::OPS
            .iter()
            .find(|(_, constructor, initializer)| *constructor == op || *initializer == op)
            .map(|(kind, _, _)| *kind)
    }
    pub(crate) fn accepts(self, storage: StorageType) -> bool {
        match self {
            Self::Data => Self::data_load(storage).is_some(),
            Self::Element => matches!(storage, StorageType::Val(wasmparser::ValType::Ref(_))),
        }
    }
    pub(crate) fn data_load(storage: StorageType) -> Option<super::memory::MemoryLoad> {
        use super::memory::MemoryLoad;
        Some(match storage {
            StorageType::I8 => MemoryLoad::I32Load8U,
            StorageType::I16 => MemoryLoad::I32Load16U,
            StorageType::Val(wasmparser::ValType::I32) => MemoryLoad::I32Load,
            StorageType::Val(wasmparser::ValType::I64) => MemoryLoad::I64Load,
            StorageType::Val(wasmparser::ValType::F32) => MemoryLoad::F32Load,
            StorageType::Val(wasmparser::ValType::F64) => MemoryLoad::F64Load,
            StorageType::Val(wasmparser::ValType::V128) => MemoryLoad::V128Load,
            _ => return None,
        })
    }
}

/// The complete constructor operand/root window; the type index remains immediate.
#[repr(u16)]
pub(crate) enum ArraySegmentInput {
    Offset,
    Count,
    Segment,
}
impl ArraySegmentInput {
    pub(crate) const COUNT: u16 = Self::Segment as u16 + 1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StructConstruction {
    New,
    Default,
    Described,
    DefaultDescribed,
}
impl StructConstruction {
    const OPS: &[(Self, Op)] = &[
        (Self::New, Op::WasmStructNew),
        (Self::Default, Op::WasmStructNewDefault),
        (Self::Described, Op::WasmStructNewDesc),
        (Self::DefaultDescribed, Op::WasmStructNewDefaultDesc),
    ];
    pub(crate) fn from_op(op: Op) -> Option<Self> {
        Self::OPS
            .iter()
            .find(|(_, candidate)| *candidate == op)
            .map(|(mode, _)| *mode)
    }
    pub(crate) fn op(self) -> Op {
        Self::OPS.iter().find(|(mode, _)| *mode == self).unwrap().1
    }
    pub(crate) fn defaulted(self) -> bool {
        matches!(self, Self::Default | Self::DefaultDescribed)
    }
    pub(crate) fn described(self) -> bool {
        matches!(self, Self::Described | Self::DefaultDescribed)
    }
    pub(crate) fn from_operator(operator: &Operator<'_>) -> Option<(u32, Self)> {
        Some(match *operator {
            Operator::StructNew { struct_type_index } => (struct_type_index, Self::New),
            Operator::StructNewDefault { struct_type_index } => (struct_type_index, Self::Default),
            Operator::StructNewDesc { struct_type_index } => (struct_type_index, Self::Described),
            Operator::StructNewDefaultDesc { struct_type_index } => {
                (struct_type_index, Self::DefaultDescribed)
            }
            _ => return None,
        })
    }
}

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
        if let StorageType::Val(ty) = field.element_type {
            if self
                .signatures
                .declarations
                .callable_value_type(ty)
                .is_none()
                || op == Op::WasmArrayNewDefault && !ty.is_defaultable()
            {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "unsupported Wasm array element",
                ));
            }
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
            if let StorageType::Val(ty) = field.element_type {
                if self
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
                self.emit(access.array_op(), a, b, c, 0)?;
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
            if let wasmparser::StorageType::Val(ty) = field.element_type {
                if self
                    .signatures
                    .declarations
                    .callable_value_type(ty)
                    .is_none()
                    || default && !ty.is_defaultable()
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "unsupported Wasm struct field",
                    ));
                }
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
        } else if let StorageType::Val(ty) = destination.element_type {
            if owner.callable_value_type(ty).is_none() {
                return Err(Diagnostic::unsupported(
                    self.name,
                    "unsupported Wasm array fill storage",
                ));
            }
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
        if let StorageType::Val(ty) = declaration.element_type {
            if self
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
            // Validation resolves the static type. The live object owns field layout;
            // subtype prefixes preserve packed types and mutable-field invariance.
            self.emit(access.op(), a, b, 0, field)?;
        }
        Ok(())
    }
}
