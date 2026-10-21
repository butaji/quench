//! A module's imports and exports as its binary declares them, read without validating or
//! lowering it: what a host needs to link the module before it compiles it.

use wasmparser::{ExternalKind, Parser, Payload, TypeRef};

/// An import or export's kind, as the JS API names it (`WebAssembly.Module.imports`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternKind {
    Function,
    Table,
    Memory,
    Global,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub kind: ExternKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub kind: ExternKind,
}

/// The import and export sections' entries, in declaration order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Interface {
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

/// The interface of `bytes`, or `None` when the module does not parse; whether it validates
/// is its compilation's question.
pub fn interface(bytes: &[u8]) -> Option<Interface> {
    let mut interface = Interface::default();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.ok()? {
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import.ok()?;
                    let kind = match import.ty {
                        TypeRef::Func(_) | TypeRef::FuncExact(_) => ExternKind::Function,
                        TypeRef::Table(_) => ExternKind::Table,
                        TypeRef::Memory(_) => ExternKind::Memory,
                        TypeRef::Global(_) => ExternKind::Global,
                        TypeRef::Tag(_) => ExternKind::Tag,
                    };
                    interface.imports.push(Import { module: import.module.to_owned(), name: import.name.to_owned(), kind });
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.ok()?;
                    let kind = match export.kind {
                        ExternalKind::Func | ExternalKind::FuncExact => ExternKind::Function,
                        ExternalKind::Table => ExternKind::Table,
                        ExternalKind::Memory => ExternKind::Memory,
                        ExternalKind::Global => ExternKind::Global,
                        ExternalKind::Tag => ExternKind::Tag,
                    };
                    interface.exports.push(Export { name: export.name.to_owned(), kind });
                }
            }
            _ => {}
        }
    }
    Some(interface)
}
