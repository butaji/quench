//! The object linker: an `AotImage` as a `NativeJsModule` for the system linker. Each function's
//! entry is its base body; lane bodies stay in the image file, which the VM relinks each from
//! when it installs it (`live::link_lane`). Helpers and semantic entries are imports the linker binds;
//! a code relocation names its target by declaration index, with a PC leave's offset as addend. The
//! generic rows' operand records are one data record, named with the record's offset as addend, and
//! the proven calls' records another, writable: each names no callee until the runtime binds it
//! (`bound::BoundCalls::bind`), so its direct row takes the helper meanwhile.
//! Symbols whose value is only known in a VM (atoms, shapes, heap values, cells) have no object
//! form yet and are rejected here rather than linked to a wrong address.
//!
//! Owner: one pipeline.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{AotFunction, AotImage, base::NO_ENTRY, demand::Demand, file, symbols::{Symbol, SymbolTable}};
use quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode;
use crate::native_object::{NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeCode, NativeData, NativeFunction, NativeDataAddress, NativeFunctionAddress, NativeJsModule, NativeReloc};
use cranelift_codegen::{ir, isa::{CallConv, TargetIsa}};
use quench_bjit::call_record::CallRecord;
use std::collections::BTreeMap;

/// The system linker's name of lane helper number `helper`, which the runtime binds.
/// Every body starts at this alignment in the object.
const CODE_ALIGNMENT: u64 = 16;
/// The declaration index of the ABI header import, the object's first symbol.
const ABI_DECLARATION: u32 = 0;
/// The image file's data record (`file::to_bytes`), which the executable reads its image from.
const IMAGE_RECORD: u32 = 0;
/// The entry table's data record, after the image record.
const ENTRY_TABLE_RECORD: usize = 1;
/// The proven calls' records, after the entry table.
const CALL_RECORDS_RECORD: u32 = 2;
/// The lane helpers' addresses by number (`helper_data`), after the call records.
const HELPERS_RECORD: u32 = 3;
/// The data record of the RegExp literals' searches (`regexp_data`).
const REGEXPS_RECORD: u32 = 4;
/// The bytes of one call record (`CallRecord`).
const CALL_RECORD_BYTES: i64 = std::mem::size_of::<CallRecord>() as i64;

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
pub(crate) fn emit(image: &AotImage, namespace: &str, target: &super::ImageTarget, demand: Demand) -> Result<NativeJsModule, String> {
    let isa = quench_runtime::vm::jit_backend::relocatable_isa(target)?;
    let isa = &*isa;
    let mut functions = vec![NativeFunction::Import {
        name: quench_runtime::NATIVE_JS_ABI_SYMBOL.into(),
        signature: ir::Signature::new(isa.default_call_conv()),
    }];
    let imports = import_symbols(image, isa, &mut functions)?;
    let helpers = helper_data(namespace, image, &imports);
    let first = u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
    let bases = base_declarations(image, first)?;
    let targets = Targets { image, imports: &imports, bases: &bases, conv: isa.default_call_conv() };
    let definitions = image.functions.iter().map(|function| entry_definition(namespace, function, &targets)).collect::<Result<Vec<_>, String>>()?;
    functions.extend(definitions);
    let regexps = regexp_data(namespace, image, &mut functions, targets.conv)?;
    let file = file::to_bytes(image);
    let table = entry_table(isa, namespace, (&bases, first), (file.len(), image.regexps.len()))?;
    let data = vec![image_data(namespace, file), table, call_data(namespace, image), helpers, regexps];
    Ok(NativeJsModule::of(functions, data, ENTRY_TABLE_RECORD, demand))
}

/// The code the object places for an image's function: its base body. A lane body names VM
/// symbols (atoms, heap values, the invalidation word) only a loader resolves, so the VM links
/// it from the image file when it installs it (`live::link_lane`), as it does in-process.
fn codes(function: &AotFunction) -> impl Iterator<Item = &RelocatableCode> {
    std::iter::once(&function.base.code)
}

/// Declares the helper and semantic-entry symbols the code names, after `functions`' existing
/// declarations, and returns their declaration indices: the base bodies' every symbol, and the helpers a
/// lane body names, which the VM links it to from the helper record (`helper_data`).
fn import_symbols(image: &AotImage, isa: &dyn TargetIsa, functions: &mut Vec<NativeFunction>) -> Result<BTreeMap<(u32, u32), u32>, String> {
    let mut imports = BTreeMap::new();
    let lanes = image.functions.iter().filter_map(|function| function.lane.as_ref()).flat_map(|lane| lane.code.relocations.iter())
        .filter(|reloc| matches!(SymbolTable::decode(image, reloc.namespace, reloc.key), Some(Symbol::Helper(_))));
    for reloc in image.functions.iter().flat_map(codes).flat_map(|code| code.relocations.iter()).chain(lanes) {
        let key = (reloc.namespace, reloc.key);
        if imports.contains_key(&key) {
            continue;
        }
        let import = match SymbolTable::decode(image, key.0, key.1) {
            Some(Symbol::Helper(helper)) => helper_import(isa, helper)?,
            // The runtime exports the byte by name; the system linker resolves the word to it.
            Some(Symbol::Attention) => NativeFunction::Import { name: quench_runtime::execution_interrupt::ATTENTION_SYMBOL.into(), signature: ir::Signature::new(isa.default_call_conv()) },
            // The runtime exports the narrow decode's table by name: one table for every image.
            Some(Symbol::NarrowUpperWords) => NativeFunction::Import { name: quench_runtime::staged::value_encoding::NARROW_UPPER_WORDS_SYMBOL.into(), signature: ir::Signature::new(isa.default_call_conv()) },
            Some(Symbol::BasePc { .. } | Symbol::CallRecord { .. }) => continue,
            Some(symbol) => return Err(format!("{symbol:?} has no object form yet")),
            None => return Err("an AOT image names an unknown symbol".into()),
        };
        imports.insert(key, u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?);
        functions.push(import);
    }
    Ok(imports)
}

/// The import of lane helper `helper`, by its name (`compiled::lane_helper_symbol`), which the
/// executable defines (`define_lane_helper_symbols`).
fn helper_import(isa: &dyn TargetIsa, helper: u32) -> Result<NativeFunction, String> {
    let name = crate::compiled::lane_helper_symbol(helper as usize).ok_or("an AOT image names an unknown lane helper")?;
    Ok(NativeFunction::Import { name, signature: ir::Signature::new(isa.default_call_conv()) })
}

/// The declaration index of each function's base body, which is its entry (`codes`): `first` plus the ID.
fn base_declarations(image: &AotImage, first: u32) -> Result<Vec<u32>, String> {
    u32::try_from(image.functions.len()).map_err(|_| "native function count exceeds u32")?;
    Ok((0..image.functions.len() as u32).map(|id| first + id).collect())
}

/// Function `function`'s entry: its base body (`codes`).
fn entry_definition(namespace: &str, function: &AotFunction, targets: &Targets<'_>) -> Result<NativeFunction, String> {
    definition(format!("{namespace}_function_{}", function.id), &function.base.code, targets)
}

fn definition(name: String, code: &RelocatableCode, targets: &Targets<'_>) -> Result<NativeFunction, String> {
    let relocations = code.relocations.iter().map(|reloc| {
        let (namespace, index, addend) = targets.of(reloc.namespace, reloc.key, reloc.addend)?;
        Ok(NativeReloc { offset: reloc.offset as u32, kind: reloc.kind, namespace, index, addend })
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
    /// The native namespace, declaration and addend of symbol `(namespace, key)` under `addend`.
    fn of(&self, namespace: u32, key: u32, addend: i64) -> Result<(u32, u32, i64), String> {
        match SymbolTable::decode(self.image, namespace, key) {
            Some(Symbol::BasePc { function, pc }) => {
                let offset = self.image.functions.get(function as usize)
                    .and_then(|body| body.base.pc_offsets.get(pc as usize).copied())
                    .filter(|offset| *offset != NO_ENTRY)
                    .ok_or("a lane leaves to a record no control enters")?;
                let base = *self.bases.get(function as usize).ok_or("a lane leaves to an unknown function")?;
                Ok((NATIVE_FUNCTION_NAMESPACE, base, addend + i64::from(offset)))
            }
            Some(Symbol::CallRecord { .. }) => Ok((NATIVE_DATA_NAMESPACE, CALL_RECORDS_RECORD, addend + i64::from(key) * CALL_RECORD_BYTES)),
            _ => self.imports.get(&(namespace, key)).map(|index| (NATIVE_FUNCTION_NAMESPACE, *index, addend)).ok_or_else(|| "an AOT image symbol has no object form".into()),
        }
    }
}

/// The image's file (`file::to_bytes`): the executable reads the image from it (`LinkedTable`), the
/// code in it relinked only for the lane bodies a VM publishes.
fn image_data(namespace: &str, file: Vec<u8>) -> NativeData {
    NativeData {
        name: format!("{namespace}_image"),
        bytes: file,
        alignment: std::mem::align_of::<u64>() as u64,
        writable: false,
        function_addresses: Vec::new(),
        data_addresses: Vec::new(),
    }
}

/// Each lane helper's address by number, bound by the system linker for the helpers the image names (`imports`) and 0
/// for the rest: a lane body links its helpers from it (`live::link_lane`), so the executable keeps only these.
fn helper_data(namespace: &str, image: &AotImage, imports: &BTreeMap<(u32, u32), u32>) -> NativeData {
    let pointer = std::mem::size_of::<usize>();
    let function_addresses = imports.iter().filter_map(|(&(namespace, key), &function)| match SymbolTable::decode(image, namespace, key) {
        Some(Symbol::Helper(helper)) => Some(NativeFunctionAddress { offset: helper * pointer as u32, function }),
        _ => None,
    }).collect();
    NativeData {
        name: format!("{namespace}_helpers"),
        bytes: vec![0; crate::compiled::LaneHelper::COUNT * pointer],
        alignment: pointer as u64,
        writable: false,
        function_addresses,
        data_addresses: Vec::new(),
    }
}

/// The address of each RegExp literal's search by index (`AotImage::regexps`), each defined as its code, which names no
/// symbol (`Regex::ahead_plans`): the executable registers them for the literals' first searches (`live::linked`).
fn regexp_data(namespace: &str, image: &AotImage, functions: &mut Vec<NativeFunction>, conv: CallConv) -> Result<NativeData, String> {
    let pointer = std::mem::size_of::<usize>();
    let mut function_addresses = Vec::new();
    for (index, regexp) in image.regexps.iter().enumerate() {
        let function = u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
        functions.push(NativeFunction::Code(NativeCode { name: format!("{namespace}_regexp_{index}"), signature: ir::Signature::new(conv),
            code: regexp.code.to_vec(), alignment: CODE_ALIGNMENT, relocations: Vec::new() }));
        function_addresses.push(NativeFunctionAddress { offset: u32::try_from(index * pointer).map_err(|_| "RegExp table exceeds relocation offsets")?, function });
    }
    Ok(NativeData { name: format!("{namespace}_regexps"), bytes: vec![0; image.regexps.len().max(1) * pointer], alignment: pointer as u64,
        writable: false, function_addresses, data_addresses: Vec::new() })
}

/// The proven calls' records, each its image template in host order (`CallRecord` is `repr(C)`),
/// writable for the runtime's binding.
fn call_data(namespace: &str, image: &AotImage) -> NativeData {
    NativeData {
        name: format!("{namespace}_calls"),
        bytes: image.calls.iter().flat_map(|call| record_bytes(&call.record)).collect(),
        alignment: std::mem::align_of::<CallRecord>() as u64,
        writable: true,
        function_addresses: Vec::new(),
        data_addresses: Vec::new(),
    }
}

/// The bytes of `record` as the direct row reads them: each field at its offset, the padding zero. The callee record
/// is the loader's (`BoundCalls::of`), so the template's pointer is zero.
fn record_bytes(record: &CallRecord) -> Vec<u8> {
    use std::mem::offset_of;
    let mut bytes = vec![0; std::mem::size_of::<CallRecord>()];
    let words = [(offset_of!(CallRecord, call), record.call), (offset_of!(CallRecord, caller_base), record.caller_base),
        (offset_of!(CallRecord, packed), record.packed), (offset_of!(CallRecord, proven_args), record.proven_args)];
    let registers = [(offset_of!(CallRecord, callee_register), record.callee_register), (offset_of!(CallRecord, this_register), record.this_register),
        (offset_of!(CallRecord, args_register), record.args_register), (offset_of!(CallRecord, result_register), record.result_register)];
    words.iter().for_each(|&(at, word)| bytes[at..at + std::mem::size_of::<u64>()].copy_from_slice(&word.to_ne_bytes()));
    registers.iter().for_each(|&(at, register)| bytes[at..at + std::mem::size_of::<u16>()].copy_from_slice(&register.to_ne_bytes()));
    let places = offset_of!(CallRecord, argument_places);
    bytes[places..places + record.argument_places.len()].copy_from_slice(&record.argument_places);
    bytes
}

/// The entry table (`crate::native_program::LinkedTable` reads it): the ABI header word; one
/// entry per function, each the address of its entry definition (`first` plus the ID); one base body
/// address per function (`bases`, by declaration); then the image file's address and length, the
/// call records' address, the helper record's address and count, and the RegExp record's address and count.
fn entry_table(isa: &dyn TargetIsa, namespace: &str, (bases, first): (&[u32], u32), (file_length, regexps): (usize, usize)) -> Result<NativeData, String> {
    let pointer_bytes = usize::from(isa.pointer_bytes());
    if pointer_bytes != std::mem::size_of::<usize>() {
        return Err("native image pointer width differs from host".into());
    }
    let (header, count) = (quench_runtime::NATIVE_JS_IMAGE_HEADER_WORDS, bases.len());
    let offset = |word: usize| u32::try_from(word * pointer_bytes).map_err(|_| "native entry table exceeds relocation offsets");
    let mut addresses = vec![NativeFunctionAddress { offset: 0, function: ABI_DECLARATION }];
    for id in 0..count {
        addresses.push(NativeFunctionAddress { offset: offset(header + id)?, function: first + id as u32 });
        addresses.push(NativeFunctionAddress { offset: offset(header + count + id)?, function: bases[id] });
    }
    let tail = header + 2 * count;
    let data_addresses = vec![
        NativeDataAddress { offset: offset(tail)?, data: IMAGE_RECORD, addend: 0 },
        NativeDataAddress { offset: offset(tail + 2)?, data: CALL_RECORDS_RECORD, addend: 0 },
        NativeDataAddress { offset: offset(tail + 3)?, data: HELPERS_RECORD, addend: 0 },
        NativeDataAddress { offset: offset(tail + 5)?, data: REGEXPS_RECORD, addend: 0 },
    ];
    let mut bytes = vec![0; (tail + 7) * pointer_bytes];
    bytes[(tail + 6) * pointer_bytes..(tail + 7) * pointer_bytes].copy_from_slice(&regexps.to_ne_bytes());
    bytes[(tail + 1) * pointer_bytes..(tail + 2) * pointer_bytes].copy_from_slice(&file_length.to_ne_bytes());
    bytes[(tail + 4) * pointer_bytes..(tail + 5) * pointer_bytes].copy_from_slice(&crate::compiled::LaneHelper::COUNT.to_ne_bytes());
    Ok(NativeData {
        name: format!("{namespace}_entries"),
        bytes,
        alignment: pointer_bytes as u64,
        writable: false,
        function_addresses: addresses,
        data_addresses,
    })
}
