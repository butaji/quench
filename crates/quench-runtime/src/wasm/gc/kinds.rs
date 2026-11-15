//! The GC operation kinds: the access, segment and construction modes an operator names, with
//! their operator tables.

use super::*;
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

    /// Whether `op` reads a value field (`Read`): `Some(false)` for a struct's, `Some(true)`
    /// for an array's element, `None` for any other opcode. A lane handler selects its read by
    /// this table fact rather than naming the opcodes.
    pub(crate) const fn plain_read(op: Op) -> Option<bool> {
        let (_, structure, array) = Self::OPS[0];
        if structure as u16 == op as u16 {
            Some(false)
        } else if array as u16 == op as u16 {
            Some(true)
        } else {
            None
        }
    }

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

    /// A plain loop over `OPS`, so a constant opcode folds to its access where this inlines.
    #[inline(always)]
    pub(crate) const fn from_op(op: Op) -> Option<Self> {
        let mut at = 0;
        while at < Self::OPS.len() {
            let (access, structure, array) = Self::OPS[at];
            if structure as u16 == op as u16 || array as u16 == op as u16 {
                return Some(access);
            }
            at += 1;
        }
        None
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
