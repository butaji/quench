//! The module an AOT image or a Cranelift program publishes: its native functions and data, the
//! entry table, and the already-compiled code (`NativeCode`) an image relocates by declaration.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{NativeData, NativeFunction};
use quench_runtime::bytecode::{ProgramKind, ResidualProgram};
use cranelift_codegen::{binemit::Reloc, ir};
use cranelift_module::{FuncId, Module, ModuleReloc, ModuleRelocTarget};
use cranelift_object::ObjectModule;

/// Code already compiled position-independent, with its relocations to module symbols.
pub struct NativeCode {
    pub name: String,
    pub signature: ir::Signature,
    pub code: Vec<u8>,
    pub alignment: u64,
    pub relocations: Vec<NativeReloc>,
}

/// One relocation of `NativeCode`: the field at `offset` takes the address of declaration
/// `index` in `namespace` (`NATIVE_FUNCTION_NAMESPACE` or `NATIVE_DATA_NAMESPACE`) plus `addend`.
pub struct NativeReloc {
    pub offset: u32,
    pub kind: Reloc,
    pub namespace: u32,
    pub index: u32,
    pub addend: i64,
}

/// The link inputs of one program: its functions and data, and which data record is its entry table.
pub struct NativeJsModule {
    pub functions: Vec<NativeFunction>,
    pub data: Vec<NativeData>,
    entry_table: usize,
}

impl NativeJsModule {
    pub(crate) fn of(functions: Vec<NativeFunction>, data: Vec<NativeData>, entry_table: usize) -> Self {
        Self { functions, data, entry_table }
    }

    /// Linker symbol for the ABI header followed by function-ID-ordered entries.
    pub fn entry_table_symbol(&self) -> &str {
        &self.data[self.entry_table].name
    }
}

/// Lowers every function of `program` as one AOT image for the default target.
pub fn lower_native_js_module(program: &ResidualProgram, namespace: &str) -> Result<NativeJsModule, String> {
    lower_native_js_module_for(program, namespace, &quench_runtime::vm::jit_backend::NativeTarget::default())
}

/// Lowers every function of `program` as one AOT image for `target` and emits it as a module
/// whose symbols the system linker resolves (`aot_image::object`).
pub fn lower_native_js_module_for(program: &ResidualProgram, namespace: &str, target: &quench_runtime::vm::jit_backend::NativeTarget) -> Result<NativeJsModule, String> {
    if program.kind == ProgramKind::Wasm {
        return Err("JavaScript native graph requires a JS or TS program".into());
    }
    // Every function's lowered record, which the facts and the image read (`function_get`) in
    // place of a loop-free function's tier-0 copy. No frame runs during an ahead compile.
    for index in 0..program.function_count() {
        program.lowered_function(index);
    }
    let facts = quench_runtime::compile::aot_facts::analyze(program);
    let image = crate::aot_image::lower(program, &facts, target)?;
    crate::aot_image::object::emit(&image, namespace)
}

/// Defines `code` as the already-compiled body of `id`, its relocations named by declaration.
pub(crate) fn define_code(module: &mut ObjectModule, id: FuncId, code: NativeCode) -> Result<(), String> {
    let relocations = code.relocations.iter().map(|reloc| ModuleReloc {
        kind: reloc.kind,
        offset: reloc.offset,
        name: ModuleRelocTarget::User { namespace: reloc.namespace, index: reloc.index },
        addend: reloc.addend,
    }).collect::<Vec<_>>();
    module.define_function_bytes(id, code.alignment, &code.code, &relocations)
        .map_err(|error| format!("{}: {error}", code.name))
}
