//! GC and exception ops. Still HIR; NativeIR when proven.

use crate::hir::Reg;
use crate::layer::Layer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcStorage {
    I8,
    I16,
    Val(crate::hir::Kind),
    /// Packed as a typed ref so rec-group identity is data.
    Ref {
        type_idx: Option<u32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum GcType {
    Func {
        super_idx: Option<u32>,
        descriptor_idx: Option<u32>,
        describes_idx: Option<u32>,
        is_final: bool,
    },
    Struct {
        fields: Box<[GcStorage]>,
        super_idx: Option<u32>,
        descriptor_idx: Option<u32>,
        describes_idx: Option<u32>,
        is_final: bool,
    },
    Array {
        elem: GcStorage,
        super_idx: Option<u32>,
        descriptor_idx: Option<u32>,
        describes_idx: Option<u32>,
        is_final: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcOp {
    StructNewDefault {
        type_idx: u32,
    },
    StructNew {
        type_idx: u32,
    },
    StructNewDesc {
        type_idx: u32,
    },
    StructNewDefaultDesc {
        type_idx: u32,
    },
    StructGet {
        field: u32,
        signed: Option<bool>,
        pack: u8,
    },
    StructSet {
        field: u32,
    },
    ArrayNew {
        type_idx: u32,
    },
    ArrayNewDefault {
        type_idx: u32,
    },
    ArrayNewFixed {
        type_idx: u32,
        n: u32,
    },
    ArrayGet {
        signed: Option<bool>,
        pack: u8,
    },
    ArraySet,
    ArrayLen,
    ArrayFill,
    ArrayCopy,
    ArrayNewData {
        type_idx: u32,
        data: u32,
    },
    ArrayNewElem {
        type_idx: u32,
        elem: u32,
    },
    ArrayInitData {
        data: u32,
    },
    ArrayInitElem {
        elem: u32,
    },
    RefCast {
        nullable: bool,
        exact: bool,
        heap: crate::hir::HeapKind,
        type_idx: Option<u32>,
    },
    RefTest {
        nullable: bool,
        exact: bool,
        heap: crate::hir::HeapKind,
        type_idx: Option<u32>,
    },
    RefGetDesc,
    RefCastDesc {
        nullable: bool,
        exact: bool,
        type_idx: Option<u32>,
    },
    RefTestDesc {
        nullable: bool,
        exact: bool,
        type_idx: Option<u32>,
    },
    AnyConvertExtern,
    ExternConvertAny,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatchClause {
    pub tag: Option<u32>,
    pub with_ref: bool,
    pub target: u32,
    pub dsts: Box<[Reg]>,
}

impl GcOp {
    pub fn ir(self) -> Layer {
        Layer::Native
    }
}
