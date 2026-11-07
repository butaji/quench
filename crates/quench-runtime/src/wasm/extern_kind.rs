//! The kind of a module's import or export, as the JS API names it.

use wasmparser::{ExternalKind, TypeRef};

/// An import or export's kind (`WebAssembly.Module.imports`, `.exports`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmExternKind {
    Function,
    Table,
    Memory,
    Global,
    Tag,
}

impl WasmExternKind {
    /// The `kind` string of a module descriptor.
    pub fn name(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Table => "table",
            Self::Memory => "memory",
            Self::Global => "global",
            Self::Tag => "tag",
        }
    }
}

impl From<ExternalKind> for WasmExternKind {
    fn from(kind: ExternalKind) -> Self {
        match kind {
            ExternalKind::Func | ExternalKind::FuncExact => Self::Function,
            ExternalKind::Table => Self::Table,
            ExternalKind::Memory => Self::Memory,
            ExternalKind::Global => Self::Global,
            ExternalKind::Tag => Self::Tag,
        }
    }
}

impl From<TypeRef> for WasmExternKind {
    fn from(ty: TypeRef) -> Self {
        match ty {
            TypeRef::Func(_) | TypeRef::FuncExact(_) => Self::Function,
            TypeRef::Table(_) => Self::Table,
            TypeRef::Memory(_) => Self::Memory,
            TypeRef::Global(_) => Self::Global,
            TypeRef::Tag(_) => Self::Tag,
        }
    }
}
