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
    let (mut imports, helpers) = import_symbols(image, isa, &mut functions)?;
    let first = u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
    let bases = base_declarations(image, first)?;
    // The helper stubs follow every entry and base body, so the entry numbering `base_declarations`
    // computes is unchanged.
    let lanes = u32::try_from(image.functions.iter().filter(|function| function.lane.is_some()).count())
        .map_err(|_| "native symbol count exceeds u32")?;
    let helper_base = first.checked_add(u32::try_from(image.functions.len()).map_err(|_| "native symbol count exceeds u32")?)
        .and_then(|after| after.checked_add(lanes)).ok_or("native symbol count overflow")?;
    for (at, helper) in helpers.iter().enumerate() {
        imports.insert((crate::vm::compiled::lane_helper_key(*helper).0, crate::vm::compiled::lane_helper_key(*helper).1), helper_base + at as u32);
    }
    let targets = Targets { image, imports: &imports, bases: &bases, conv: isa.default_call_conv() };
    let mut definitions = image.functions.iter().map(|function| entry_definition(namespace, function, &targets)).collect::<Result<Vec<_>, String>>()?;
    definitions.extend(image.functions.iter().filter(|function| function.lane.is_some())
        .map(|function| base_definition(namespace, function, &targets)).collect::<Result<Vec<_>, String>>()?);
    functions.extend(definitions);
    if !helpers.is_empty() {
        let table = *imports.get(&(crate::vm::compiled::lane_helper_table_key().0, crate::vm::compiled::lane_helper_table_key().1))
            .ok_or("an image names a helper but no helper table")?;
        // Named per module: each object defines its own stub (nothing outside references it -- the
        // code reaches it by declaration index), so two objects never collide on one name.
        for helper in &helpers {
            functions.push(helper_stub(format!("{namespace}_lane_helper_{helper}"), *helper, table, isa));
        }
    }
    let data = vec![recipe_data(namespace, image), entry_table(isa, namespace, image.functions.len(), first)?];
    Ok(NativeJsModule::of(functions, data, ENTRY_TABLE_RECORD))
}

/// Every code an image's function holds: its base body, and its lane body where it has one.
fn codes(function: &AotFunction) -> impl Iterator<Item = &RelocatableCode> {
    std::iter::once(&function.base.code).chain(function.lane.as_ref().map(|lane| &lane.code))
}

/// Declares the semantic-entry symbols the code names, after `functions`' existing declarations,
/// and returns their declaration indices and the helpers the code names. A helper is not an
/// import: the module defines one stub per helper over the exported table (`helper_stub`), because
/// the executable cannot export a text symbol per helper of a build it does not know.
fn import_symbols(image: &AotImage, isa: &dyn TargetIsa, functions: &mut Vec<NativeFunction>)
    -> Result<(BTreeMap<(u32, u32), u32>, Vec<u32>), String> {
    let mut imports = BTreeMap::new();
    let mut helpers: Vec<u32> = Vec::new();
    for reloc in image.functions.iter().flat_map(codes).flat_map(|code| code.relocations.iter()) {
        let key = (reloc.namespace, reloc.key);
        match SymbolTable::decode(&image.labels, key.0, key.1) {
            Some(Symbol::Helper(helper)) => {
                if !helpers.contains(&helper) { helpers.push(helper); }
                continue;
            }
            _ => {}
        }
        if imports.contains_key(&key) {
            continue;
        }
        let import = match SymbolTable::decode(&image.labels, key.0, key.1) {
            Some(Symbol::HelperTable) => NativeFunction::Import {
                name: crate::NATIVE_JS_LANE_HELPER_TABLE_SYMBOL.to_owned(),
                signature: ir::Signature::new(isa.default_call_conv()),
            },
            Some(Symbol::SemanticEntry(op)) => operation_import(isa, op),
            Some(Symbol::BasePc { .. }) => continue,
            Some(symbol) => return Err(format!("{symbol:?} has no object form yet")),
            None => return Err("an AOT image names an unknown symbol".into()),
        };
        imports.insert(key, u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?);
        functions.push(import);
    }
    // A helper needs the table; an image that names helpers before the table does gets it here.
    if !helpers.is_empty() && !imports.contains_key(&(crate::vm::compiled::lane_helper_table_key().0, crate::vm::compiled::lane_helper_table_key().1)) {
        let key = (crate::vm::compiled::lane_helper_table_key().0, crate::vm::compiled::lane_helper_table_key().1);
        imports.insert(key, u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?);
        functions.push(NativeFunction::Import {
            name: crate::NATIVE_JS_LANE_HELPER_TABLE_SYMBOL.to_owned(),
            signature: ir::Signature::new(isa.default_call_conv()),
        });
    }
    Ok((imports, helpers))
}

/// Helper `helper`'s stub: a branch to the table's entry, so a stitched row's call reaches the
/// helper through one exported symbol. Built as Cranelift IR rather than bytes, so the target
/// backend emits the reference to the table with the relocations Mach-O accepts from a text
/// section (an absolute word there is illegal, and the table lives in data).
///
/// The stub forwards its own parameter registers unchanged, which is the row ABI a helper is
/// entered with: it reads none of them, it only tails into the helper's entry.
#[cfg(target_arch = "aarch64")]
fn helper_stub(name: String, helper: u32, table: u32, isa: &dyn TargetIsa) -> NativeFunction {
    use cranelift_codegen::ir::InstBuilder as _;
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
    let mut signature = ir::Signature::new(isa.default_call_conv());
    for _ in 0..STUB_PARAMETERS { signature.params.push(ir::AbiParam::new(cranelift_codegen::ir::types::I64)); }
    signature.returns.push(ir::AbiParam::new(cranelift_codegen::ir::types::I64));
    let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature.clone());
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let symbol = builder.func.declare_imported_user_function(ir::UserExternalName::new(NATIVE_FUNCTION_NAMESPACE, table));
    let table_value = builder.func.create_global_value(ir::GlobalValueData::Symbol {
        name: ir::ExternalName::user(symbol), offset: ir::immediates::Imm64::new(0), colocated: false, tls: false });
    let base = builder.ins().symbol_value(cranelift_codegen::ir::types::I64, table_value);
    let entry_address = builder.ins().load(cranelift_codegen::ir::types::I64, ir::MemFlagsData::trusted(), base, (helper * 8) as i32);
    let call = builder.import_signature(signature);
    let arguments: Vec<_> = builder.block_params(entry).to_vec();
    // A call and return, not a sibling call: AArch64's register allocator cannot satisfy a tail
    // call of this shape (`return_call_indirect` fails to allocate a minimal bundle).
    let call = builder.ins().call_indirect(call, entry_address, &arguments);
    let results: Vec<_> = builder.inst_results(call).to_vec();
    builder.ins().return_(&results);
    builder.finalize(isa.frontend_config());
    NativeFunction::Definition { name, function }
}

/// The word registers a helper's entry may read: the row ABI's context, residual and operands.
#[cfg(target_arch = "aarch64")]
const STUB_PARAMETERS: usize = 5;

#[cfg(not(target_arch = "aarch64"))]
fn helper_stub(name: String, _helper: u32, _table: u32, isa: &dyn TargetIsa) -> NativeFunction {
    NativeFunction::Import { name, signature: ir::Signature::new(isa.default_call_conv()) }
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
        local: false,
        relocations,
    }))
}

impl Targets<'_> {
    /// The declaration and addend of symbol `(namespace, key)` under `addend`.
    fn of(&self, namespace: u32, key: u32, addend: i64) -> Result<(u32, i64), String> {
        match SymbolTable::decode(&self.image.labels, namespace, key) {
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
