//! A module's imports and exports as its binary declares them, read without validating or
//! lowering it: what a host needs to link the module before it compiles it.

use wasmparser::{Parser, Payload};

pub use quench_runtime::WasmExternKind as ExternKind;

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
                    let kind = ExternKind::from(import.ty);
                    interface.imports.push(Import { module: import.module.to_owned(), name: import.name.to_owned(), kind });
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.ok()?;
                    let kind = ExternKind::from(export.kind);
                    interface.exports.push(Export { name: export.name.to_owned(), kind });
                }
            }
            _ => {}
        }
    }
    Some(interface)
}
