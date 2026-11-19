//! The object linker: an `AotImage` as a `NativeJsModule` for the system linker. Each function's
//! entry is its lane body where it has one, else its base body; a lane's base PC leaves name the
//! base body as a definition of its own. Helpers and semantic entries are imports the linker binds;
//! a code relocation names its target by declaration index, with a PC leave's offset as addend.
//! Symbols whose value is only known in a VM (atoms, shapes, heap values, cells) have no object
//! form yet and are rejected here rather than linked to a wrong address.
//!
//! Owner: one pipeline.

use super::{AotFunction, AotImage, base::NO_ENTRY, file, symbols::{Symbol, SymbolTable}};
use crate::bytecode::opcode_name;
use crate::vm::compiled::native_js_operation_signature;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;
use crate::vm::native_object::{NATIVE_FUNCTION_NAMESPACE, NativeCode, NativeData, NativeFunction, NativeFunctionAddress, NativeJsModule, NativeReloc};
use cranelift_codegen::{ir, isa::{CallConv, TargetIsa}};
use std::collections::BTreeMap;

/// The system linker's name of lane helper number `helper`, which the runtime binds.
const HELPER_SYMBOL_PREFIX: &str = "quench_lane_helper_";
/// Every body starts at this alignment in the object.
const CODE_ALIGNMENT: u64 = 16;
/// The declaration index of the ABI header import, the object's first symbol.
const ABI_DECLARATION: u32 = 0;
/// The entry table's data record, after the recipe record.
const ENTRY_TABLE_RECORD: usize = 1;

/// The declarations an image's code names, and where its definitions are.
struct Targets<'a> {
    image: &'a AotImage,
    /// The declaration index of each helper and semantic-entry symbol, by `(namespace, key)`.
    imports: &'a BTreeMap<(u32, u32), u32>,
    /// The declaration index of each function's base body, by function.
    bases: &'a [u32],
    conv: CallConv,
}

/// The object of `image`, its symbols named under `namespace`.
pub(crate) fn emit(image: &AotImage, namespace: &str) -> Result<NativeJsModule, String> {
    let isa = crate::vm::jit_backend::target_isa_for("speed", crate::vm::jit_backend::CodePublication::RelocatableObject)
        .ok_or("native target unavailable")?;
    let isa = &*isa;
    let mut functions = vec![NativeFunction::Import {
        name: crate::NATIVE_JS_ABI_SYMBOL.into(),
        signature: ir::Signature::new(isa.default_call_conv()),
    }];
    let imports = import_symbols(image, isa, &mut functions)?;
    let first = u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
    let bases = base_declarations(image, first)?;
    let targets = Targets { image, imports: &imports, bases: &bases, conv: isa.default_call_conv() };
    let mut definitions = image.functions.iter().map(|function| entry_definition(namespace, function, &targets)).collect::<Result<Vec<_>, String>>()?;
    definitions.extend(image.functions.iter().filter(|function| function.lane.is_some())
        .map(|function| base_definition(namespace, function, &targets)).collect::<Result<Vec<_>, String>>()?);
    functions.extend(definitions);
    let data = vec![recipe_data(namespace, image), entry_table(isa, namespace, image.functions.len(), first)?];
    Ok(NativeJsModule::of(functions, data, ENTRY_TABLE_RECORD))
}

/// Every code an image's function holds: its base body, and its lane body where it has one.
fn codes(function: &AotFunction) -> impl Iterator<Item = &RelocatableCode> {
    std::iter::once(&function.base.code).chain(function.lane.as_ref().map(|lane| &lane.code))
}

/// Declares the helper and semantic-entry symbols the code names, after `functions`' existing
/// declarations, and returns their declaration indices.
fn import_symbols(image: &AotImage, isa: &dyn TargetIsa, functions: &mut Vec<NativeFunction>) -> Result<BTreeMap<(u32, u32), u32>, String> {
    let mut imports = BTreeMap::new();
    for reloc in image.functions.iter().flat_map(codes).flat_map(|code| code.relocations.iter()) {
        let key = (reloc.namespace, reloc.key);
        if imports.contains_key(&key) {
            continue;
        }
        let import = match SymbolTable::decode(key.0, key.1) {
            Some(Symbol::Helper(helper)) => helper_import(isa, helper),
            Some(Symbol::SemanticEntry(op)) => operation_import(isa, op),
            Some(Symbol::BasePc { .. }) => continue,
            Some(symbol) => return Err(format!("{symbol:?} has no object form yet")),
            None => return Err("an AOT image names an unknown symbol".into()),
        };
        imports.insert(key, u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?);
        functions.push(import);
    }
    Ok(imports)
}

fn helper_import(isa: &dyn TargetIsa, helper: u32) -> NativeFunction {
    NativeFunction::Import { name: format!("{HELPER_SYMBOL_PREFIX}{helper}"), signature: ir::Signature::new(isa.default_call_conv()) }
}

fn operation_import(isa: &dyn TargetIsa, op: u16) -> NativeFunction {
    NativeFunction::Import {
        name: format!("{}{}", crate::NATIVE_JS_OPERATION_PREFIX, opcode_name(op as usize)),
        signature: native_js_operation_signature(isa),
    }
}

/// The declaration index of each function's base body. Entries come first, one per function
/// (`first` plus the ID); a function with a lane body has its base body after them.
fn base_declarations(image: &AotImage, first: u32) -> Result<Vec<u32>, String> {
    let count = u32::try_from(image.functions.len()).map_err(|_| "native function count exceeds u32")?;
    let mut next = first.checked_add(count).ok_or("native symbol count overflow")?;
    image.functions.iter().enumerate().map(|(id, function)| -> Result<u32, String> {
        if function.lane.is_none() {
            return Ok(first + id as u32);
        }
        let at = next;
        next = next.checked_add(1).ok_or("native symbol count overflow")?;
        Ok(at)
    }).collect()
}

/// Function `function`'s entry: its lane body where it has one, else its base body.
fn entry_definition(namespace: &str, function: &AotFunction, targets: &Targets<'_>) -> Result<NativeFunction, String> {
    let code = function.lane.as_ref().map_or(&function.base.code, |lane| &lane.code);
    definition(format!("{namespace}_function_{}", function.id), code, targets)
}

/// Function `function`'s base body as a definition of its own, where a lane body is its entry.
fn base_definition(namespace: &str, function: &AotFunction, targets: &Targets<'_>) -> Result<NativeFunction, String> {
    definition(format!("{namespace}_function_{}_base", function.id), &function.base.code, targets)
}

fn definition(name: String, code: &RelocatableCode, targets: &Targets<'_>) -> Result<NativeFunction, String> {
    let relocations = code.relocations.iter().map(|reloc| {
        let (index, addend) = targets.of(reloc.namespace, reloc.key, reloc.addend)?;
        Ok(NativeReloc { offset: reloc.offset as u32, kind: reloc.kind, namespace: NATIVE_FUNCTION_NAMESPACE, index, addend })
    }).collect::<Result<Vec<_>, String>>()?;
    Ok(NativeFunction::Code(NativeCode {
        name,
        signature: ir::Signature::new(targets.conv),
        code: code.code.to_vec(),
        alignment: CODE_ALIGNMENT,
        relocations,
    }))
}

impl Targets<'_> {
    /// The declaration and addend of symbol `(namespace, key)` under `addend`.
    fn of(&self, namespace: u32, key: u32, addend: i64) -> Result<(u32, i64), String> {
        match SymbolTable::decode(namespace, key) {
            Some(Symbol::BasePc { function, pc }) => {
                let offset = self.image.functions.get(function as usize)
                    .and_then(|body| body.base.pc_offsets.get(pc as usize).copied())
                    .filter(|offset| *offset != NO_ENTRY)
                    .ok_or("a lane leaves to a record no control enters")?;
                let base = *self.bases.get(function as usize).ok_or("a lane leaves to an unknown function")?;
                Ok((base, addend + i64::from(offset)))
            }
            _ => self.imports.get(&(namespace, key)).map(|index| (*index, addend)).ok_or_else(|| "an AOT image symbol has no object form".into()),
        }
    }
}

/// The recipe tables and atom texts, as the loader reads them.
fn recipe_data(namespace: &str, image: &AotImage) -> NativeData {
    NativeData {
        name: format!("{namespace}_recipes"),
        bytes: file::recipe_bytes(image),
        alignment: std::mem::align_of::<u32>() as u64,
        writable: false,
        function_addresses: Vec::new(),
    }
}

/// The entry table: the ABI header word, then one entry per function, each the address of
/// its entry definition (`first` plus the ID).
fn entry_table(isa: &dyn TargetIsa, namespace: &str, count: usize, first: u32) -> Result<NativeData, String> {
    let pointer_bytes = usize::from(isa.pointer_bytes());
    if pointer_bytes != std::mem::size_of::<usize>() {
        return Err("native image pointer width differs from host".into());
    }
    let header = crate::NATIVE_JS_IMAGE_HEADER_WORDS;
    let mut addresses = vec![NativeFunctionAddress { offset: 0, function: ABI_DECLARATION }];
    for id in 0..count {
        let offset = u32::try_from((id + header) * pointer_bytes).map_err(|_| "native entry table exceeds relocation offsets")?;
        addresses.push(NativeFunctionAddress { offset, function: first + id as u32 });
    }
    Ok(NativeData {
        name: format!("{namespace}_entries"),
        bytes: vec![0; (count + header) * pointer_bytes],
        alignment: pointer_bytes as u64,
        writable: false,
        function_addresses: addresses,
    })
}
