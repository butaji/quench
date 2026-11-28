mod abi;
mod closure_tables;
use closure_tables::{read_closure_tables, write_closure_tables};
use super::rest_alias::{REST_ALIAS_BIT, UNALIASED_ARGUMENTS_BIT};
use super::{
    AtomTable, BindingSite, Constant, DerivedFact, DispatchClass, FactTables, EvalBinding, EvalBindingDeclaration,
    EvalBindingLocation, FieldBase, FieldSite, Function, FunctionSlot, Handler, InitialRegister, Instr,
    LexicalBindingKind, LocalRegister, MethodSite, ModuleImportBinding, ModuleImportName,
    ModuleImportNameKind, ModuleLinkPlan, ModuleReexport, ModuleReexportKind, ModuleRequest,
    LiteralStep, ModuleRequestPhase, ObjectSite, Op, RegExpLiteralSite, SourceOffset, Superinstruction,
    WideInstruction,
};

/// The encoded kinds of `Handler::slot`: none, a frame local, or a register.
const THROWN_VALUE_DISCARDED: u8 = 0;
/// A function entry's flag for a deferred record encoded unlowered, its rewrites run (flag 0 is
/// the record in place, 1 a deferred record lowered).
const DEFERRED_UNLOWERED: u8 = 2;
/// A function entry's flag for a deferred record encoded unlowered and unrewritten: its rewrites
/// wait for its first read (`RewriteRanges::deferred`), which settles them before it lowers.
const DEFERRED_UNREWRITTEN: u8 = 3;
const THROWN_VALUE_IN_LOCAL: u8 = 1;
const THROWN_VALUE_IN_REGISTER: u8 = 2;

/// Encodes a function without an `undefined` register.
const NO_UNDEFINED_REGISTER: u16 = u16::MAX;
const RESIDUAL_MAGIC: &[u8; 8] = &[
    b'Q',
    b'U',
    b'E',
    b'N',
    b'C',
    b'H',
    RESIDUAL_MAGIC_TERMINATOR,
    super::ResidualProgram::FORMAT_VERSION,
];
const RESIDUAL_MAGIC_TERMINATOR: u8 = 0;
// Serialized absence of register parameters; no register file reaches it.
const NO_PARAMETER_REGISTERS: u16 = u16::MAX;
const WASM_BITS64_TAG: u8 = 8;
const WASM_V128_TAG: u8 = 9;
const EVAL_BINDING_LOCAL: u8 = 0;
const EVAL_BINDING_CAPTURE: u8 = 1;
const OPTIONAL_STRING_NONE: u8 = 0;
const OPTIONAL_STRING_SOME: u8 = 1;
/// A compact encoding's source text, which its reader supplies (`encode_compact_program`).
const OPTIONAL_STRING_ELIDED: u8 = 2;
const MODULE_LINK_PLAN_NONE: u8 = 0;
const MODULE_LINK_PLAN_SOME: u8 = 1;
const LITERAL_STEP_CONSTANT: u8 = 0;
const LITERAL_STEP_ARRAY: u8 = 1;
const LITERAL_STEP_OBJECT: u8 = 2;
const LITERAL_STEP_INT: u8 = 3;
const LITERAL_STEP_JSON: u8 = 4;

fn read_eval_binding(input: &mut BinaryReader<'_>) -> Result<EvalBinding, String> {
    Ok(EvalBinding {
        atom: input.atom()?,
        location: match input.u8()? {
            EVAL_BINDING_LOCAL => EvalBindingLocation::Local(input.u16()?),
            EVAL_BINDING_CAPTURE => EvalBindingLocation::Capture {
                depth: input.u16()?,
                slot: input.u16()?,
            },
            _ => return Err("invalid eval binding location".into()),
        },
        with_depth: input.u16()?,
        kind: LexicalBindingKind::from_binary_tag(input.u8()?)?,
        declaration: EvalBindingDeclaration::from_binary_tag(input.u8()?)?,
    })
}

fn write_eval_binding(out: &mut BinaryWriter, binding: &EvalBinding) {
    out.atom(binding.atom);
    match binding.location {
        EvalBindingLocation::Local(slot) => {
            out.u8(EVAL_BINDING_LOCAL);
            out.u16(slot);
        }
        EvalBindingLocation::Capture { depth, slot } => {
            out.u8(EVAL_BINDING_CAPTURE);
            out.u16(depth);
            out.u16(slot);
        }
    }
    out.u16(binding.with_depth);
    out.u8(binding.kind as u8);
    out.u8(binding.declaration as u8);
}

pub(super) fn write_program(program: &super::ResidualProgram, path: &std::path::Path, retention: super::ImageRetention) -> Result<(), String> {
    crate::vm::freeze_call_sites(program);
    let source = retained_source(SourceEncoding::Included, retention);
    let (bytes, _) = encode_program(program, AtomEncoding::Program, source, retention.stack_positions);
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

/// A compact encoding of `program` for a compile cache, with the program's atom ids in the
/// order its table names them (`decode_compact_program` maps each name to its reader's id).
/// The source text is left out: whoever decodes it supplies the same text. Its reader, this
/// build, parses call sites for a diagnostic alone (each entry's had cost mandreel 14 MB).
pub(super) fn encode_compact_program(program: &super::ResidualProgram) -> (Vec<u8>, Vec<u32>) {
    let (bytes, order) = encode_program(program, AtomEncoding::Compact(Default::default()), SourceEncoding::External, true);
    (sealed(bytes), order)
}

/// An entry program's cached form (`ResidualProgram::encode_trusted`): the lossless encoding
/// under the program's own atom ids, which seed the VM's, followed by its checksum.
pub(super) fn encode_trusted_program(program: &super::ResidualProgram) -> Vec<u8> {
    crate::vm::freeze_call_sites(program);
    sealed(encode_program(program, AtomEncoding::Program, SourceEncoding::Included, true).0)
}

/// `bytes`, an encoding, followed by the checksum of its body: everything but the region of
/// deferred records, whose own checksums the body holds.
fn sealed(mut bytes: Vec<u8>) -> Vec<u8> {
    let body = region_start(&bytes).expect("an encoding ends with its region length");
    let checksum = compact_checksum(&bytes[..body]);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes
}

/// Where the region of deferred records starts in `encoding`, which ends with its length.
pub(super) fn region_start(encoding: &[u8]) -> Result<usize, String> {
    let length_at = encoding.len().checked_sub(size_of::<u32>()).ok_or("truncated residual")?;
    let length = u32::from_le_bytes(encoding[length_at..].try_into().unwrap()) as usize;
    length_at.checked_sub(length).ok_or_else(|| "residual region exceeds its encoding".into())
}

/// `encode_trusted_program`'s bytes, decoded once their checksum holds; the caller proves the
/// program valid, as this build's compiler output.
pub(super) fn decode_trusted_program(bytes: super::KeptBytes) -> Result<super::ResidualProgram, String> {
    let encoding = sealed_encoding_length(bytes.as_ref().as_ref())?;
    decode_program(&bytes, encoding, ReadAtoms::Program, None)
}

/// `decode_trusted_program`, with each atom of the program's table named by `intern`'s id,
/// as a compact encoding's reader names them.
pub(super) fn decode_trusted_program_interned(
    bytes: super::KeptBytes,
    intern: &mut dyn FnMut(&str) -> u32,
) -> Result<super::ResidualProgram, String> {
    let encoding = sealed_encoding_length(bytes.as_ref().as_ref())?;
    decode_program(&bytes, encoding, ReadAtoms::Interned(intern), None)
}

/// The length of a sealed encoding without its checksum, when the checksum holds over its
/// body. The region of deferred records is left unread.
fn sealed_encoding_length(bytes: &[u8]) -> Result<usize, String> {
    let checksum_start = bytes.len().checked_sub(size_of::<u64>()).ok_or("truncated residual")?;
    let (encoding, checksum) = bytes.split_at(checksum_start);
    let body = region_start(encoding)?;
    if u64::from_le_bytes(checksum.try_into().unwrap()) != compact_checksum(&encoding[..body]) {
        return Err("residual checksum mismatch".into());
    }
    Ok(checksum_start)
}

/// Ends a compile-cache entry that keeps its source text after its encoding
/// (`append_kept_source`).
const KEPT_SOURCE_MAGIC: u32 = u32::from_le_bytes(*b"QSRC");

/// The bytes after a kept source: its checksum, its length and `KEPT_SOURCE_MAGIC`.
const KEPT_SOURCE_TRAILER: usize = size_of::<u64>() + size_of::<u64>() + size_of::<u32>();

/// Appends `source` to a compile-cache entry after its encoding, so a program decoded from
/// the entry reads its source text in place (`super::split_kept_source`), as the host's
/// mapping the program keeps for its records, instead of holding a copy on the heap for its
/// whole run (V8-v7 RegExp: 118 KiB at its heap's peak).
pub(crate) fn append_kept_source(entry: &mut Vec<u8>, source: &str) {
    // Sized once: the entry lives as long as the program that reads its text, and `extend`
    // would grow it by doubling, leaving up to half of the source's size in spare capacity.
    entry.reserve_exact(source.len() + KEPT_SOURCE_TRAILER);
    entry.extend_from_slice(source.as_bytes());
    entry.extend_from_slice(&compact_checksum(source.as_bytes()).to_le_bytes());
    entry.extend_from_slice(&(source.len() as u64).to_le_bytes());
    entry.extend_from_slice(&KEPT_SOURCE_MAGIC.to_le_bytes());
}

/// Where an entry's kept source lies (`append_kept_source`), when it ends with one whose
/// checksum holds.
pub(super) fn kept_source_range(entry: &[u8]) -> Option<std::ops::Range<usize>> {
    let trailer = entry.len().checked_sub(KEPT_SOURCE_TRAILER)?;
    let (checksum, rest) = entry[trailer..].split_at(size_of::<u64>());
    let (length, magic) = rest.split_at(size_of::<u64>());
    if u32::from_le_bytes(magic.try_into().ok()?) != KEPT_SOURCE_MAGIC {
        return None;
    }
    let length = usize::try_from(u64::from_le_bytes(length.try_into().ok()?)).ok()?;
    let start = trailer.checked_sub(length)?;
    (u64::from_le_bytes(checksum.try_into().ok()?) == compact_checksum(&entry[start..trailer])).then_some(start..trailer)
}

/// A cached encoding ends with this checksum of the bytes before it, which a decoder checks
/// before trusting them as the compiler output they encode (`sealed_encoding_length`), and each
/// deferred record ends its body entry with this checksum of the record.
fn compact_checksum(bytes: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write(bytes);
    hasher.finish()
}

enum SourceEncoding {
    Included,
    External,
    /// No text: the program's image keeps none (`ImageRetention::source_text` is false).
    Omitted,
}

/// `kept`, unless the image keeps no source text.
fn retained_source(kept: SourceEncoding, retention: super::ImageRetention) -> SourceEncoding {
    if retention.source_text { kept } else { SourceEncoding::Omitted }
}

fn encode_program(program: &super::ResidualProgram, atoms: AtomEncoding, source_encoding: SourceEncoding, stack_positions: bool) -> (Vec<u8>, Vec<u32>) {
    let mut out = BinaryWriter::new();
    out.bytes.extend_from_slice(RESIDUAL_MAGIC);
    out.u64(super::ResidualProgram::RUNTIME_ABI_FINGERPRINT);
    out.u8(u8::from(program.specialized));
    out.u8(program.kind.binary_tag());
    out.string(&program.source_name);
    let source = program.source.as_ref().filter(|_| !matches!(source_encoding, SourceEncoding::Omitted));
    match source {
        Some(_) if matches!(source_encoding, SourceEncoding::External) => out.u8(OPTIONAL_STRING_ELIDED),
        source => write_optional_string(&mut out, source.map(|source| source.text.as_ref())),
    }
    if let Some(source) = source {
        out.bool(source.typescript);
        out.u32(source.base);
        out.u32(source.line_offset);
        out.u32(source.column_offset);
    }
    write_call_sites(&mut out, &program.record, source.is_some());
    write_module_requests(&mut out, &program.module_requests);
    out.u32(program.module_imports.len() as u32);
    for import in &program.module_imports {
        write_request_header(&mut out, &import.source, import.phase, import.module_type.as_deref());
        match &import.imported {
            ModuleImportName::Namespace => out.u8(import.imported.binary_tag()),
            ModuleImportName::Named(name) => {
                out.u8(import.imported.binary_tag());
                out.string(name);
            }
        }
        out.string(&import.local);
    }
    write_module_link_plan(&mut out, program.module_link_plan.as_ref());
    write_declared_types(&mut out, program.declared_types.as_deref());
    let mut header = out;
    let mut out = BinaryWriter::with_atoms(atoms);
    out.stack_positions = stack_positions;
    out.bytes.reserve(program.functions.len() * ESTIMATED_RECORD_BYTES);
    out.u32(program.constants.len() as u32);
    for value in &program.constants {
        match value {
            Constant::WasmV128(bits) => {
                out.u8(WASM_V128_TAG);
                out.bytes.extend_from_slice(bits);
            }
            Constant::WasmBits64(bits) => {
                out.u8(WASM_BITS64_TAG);
                out.u64(*bits);
            }
            Constant::Number(value) => {
                out.u8(0);
                out.u64(value.to_bits());
            }
            Constant::String(value) => {
                out.u8(1);
                out.string(value);
            }
            Constant::StringUnits(value) => {
                out.u8(7);
                out.u16s(value);
            }
            Constant::BigInt(value) => {
                out.u8(6);
                out.string(&value.to_string());
            }
            Constant::Boolean(value) => {
                out.u8(if *value { 2 } else { 3 });
            }
            Constant::Null => out.u8(4),
            Constant::Undefined => out.u8(5),
        }
    }
    // Every record but the root's goes to the region after the body, which a decoder leaves
    // in place until the function's first read; the body names each by its place and checksum.
    // The records the load encoded are that region already, in the order their functions
    // finished; the others follow them.
    let mut region = Vec::new();
    let mut load_records = rustc_hash::FxHashMap::default();
    if let AtomEncoding::Compact(compact) = &mut out.atoms
        && let Some(load) = program.load_encoding.take()
    {
        let LoadEncoding { out: load, records } = *load;
        let AtomEncoding::Compact(atoms) = load.atoms else {
            unreachable!("a load encodes records under compact atom ids")
        };
        *compact = atoms;
        region = load.bytes;
        load_records = records;
    }
    // Whether names resolve statically in each function encoded so far, which reads its
    // enclosing functions' (`Function::names_resolve_statically`), each encoded before it.
    let mut static_names = Vec::with_capacity(program.functions.len());
    out.u32(program.functions.len() as u32);
    for (index, function) in program.functions.iter().enumerate() {
        let deferred = index != ROOT_FUNCTION;
        // A record still unlowered is encoded so, and lowered at its first read as the
        // program would have lowered it (`ResidualProgram::lower_unlowered`): most functions
        // of a loaded script never run, and lowering each only to encode it cost about a
        // tenth of a cold load (babel: 3.7K records, 200M instructions). A record whose rewrites
        // the load deferred is encoded unrewritten too, and its first read runs them first,
        // against the room the entry restores (`RewriteRanges::deferred`, the rewrite room below).
        let unlowered = (deferred
            && crate::DYNAMIC_EXECUTION_ENABLED
            && !load_records.contains_key(&(index as u32))
            && function.record.get().is_none())
        .then(|| function.unlowered.with(|unlowered| unlowered.map(|unlowered| unlowered.function(function.header()))))
        .flatten();
        let rewrites_pending = program.rewrite_ranges.defers(index);
        out.u8(match (deferred, unlowered.is_some(), rewrites_pending) {
            (false, _, _) => 0,
            (true, false, _) => 1,
            (true, true, false) => DEFERRED_UNLOWERED,
            (true, true, true) => DEFERRED_UNREWRITTEN,
        });
        write_function_header(&mut out, &function.header());
        if let Some(record) = unlowered {
            let enclosing = enclosing_names_static(program, &static_names, record.parent);
            let names_static = record.own_names_resolve_statically(program) && enclosing;
            out.unlowered = true;
            let span = append_to(&mut out, &mut region, |out| write_function(out, &record, &program.superinstructions, 0));
            out.unlowered = false;
            write_deferred_entry(&mut out, &region, span);
            static_names.push(names_static);
            continue;
        }
        let names_static = match load_records.get(&(index as u32)) {
            Some(range) => {
                debug_assert!(deferred, "a load never releases its root");
                let record = range.start as usize..range.end as usize;
                let enclosing = enclosing_names_static(program, &static_names, function.header().parent);
                let names_static = settle_static_name_scope(&mut region[record.clone()], enclosing);
                write_deferred_entry(&mut out, &region, record);
                names_static
            }
            None if deferred => {
                let mut names_static = false;
                let span = append_to(&mut out, &mut region, |out| {
                    names_static = write_record(program, index, &static_names, out);
                });
                write_deferred_entry(&mut out, &region, span);
                names_static
            }
            None => {
                // The root's record follows its length, which is backpatched once it is written.
                let length_at = out.bytes.len();
                out.u32(0);
                let names_static = write_record(program, index, &static_names, &mut out);
                let length = u32::try_from(out.bytes.len() - length_at - size_of::<u32>()).expect("a function record fits u32 bytes");
                out.bytes[length_at..length_at + size_of::<u32>()].copy_from_slice(&length.to_le_bytes());
                names_static
            }
        };
        static_names.push(names_static);
    }
    out.u16(program.cache_sites.get());
    out.u32(program.method_sites.len() as u32);
    for site in &program.method_sites {
        out.atom(site.atom);
        out.u16(site.cache);
        out.u32(site.argument_start);
        out.u16(site.argument_count);
        out.optional_atom_pair(site.receiver_path.get());
    }
    out.u16s(&program.method_arguments);
    out.u32(program.field_sites.len() as u32);
    for site in &program.field_sites {
        out.u16(site.base.0);
        out.atom_pair(site.first);
        out.optional_atom_pair(site.second);
        out.optional_atom_pair(site.sink);
        for source in site.source_offsets() {
            out.u8(source.is_some() as u8);
            if let Some(offset) = source {
                out.u32(offset);
            }
        }
    }
    out.u32(program.object_sites.len() as u32);
    for site in &program.object_sites {
        out.u32(site.atoms.len() as u32);
        for atom in &site.atoms {
            out.atom(*atom);
        }
    }
    out.u32(program.regexp_literal_sites.len() as u32);
    for site in &program.regexp_literal_sites {
        out.u32(site.pattern_constant);
        out.u32(site.flags_constant);
        out.bool(site.receiver);
    }
    out.u32(program.literal_templates.len() as u32);
    for template in &program.literal_templates {
        out.u32(template.len() as u32);
        for step in template {
            let (tag, operand) = match *step {
                LiteralStep::Constant(index) => (LITERAL_STEP_CONSTANT, index),
                LiteralStep::Array(count) => (LITERAL_STEP_ARRAY, count),
                LiteralStep::Object(site) => (LITERAL_STEP_OBJECT, site),
                LiteralStep::Int(value) => (LITERAL_STEP_INT, value.cast_unsigned()),
                LiteralStep::Json(constant) => (LITERAL_STEP_JSON, constant),
            };
            out.u8(tag);
            out.u32(operand);
        }
    }
    out.u32(program.superinstructions.len() as u32);
    for site in &program.superinstructions {
        for instruction in site.wide {
            out.instruction(instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm());
        }
    }
    write_closure_tables(&mut out, program);
    match program.kept_chunk_room() {
        Some(room) => {
            out.bool(true);
            for value in room.spares.into_iter().chain(room.lengths) {
                out.u32(value);
            }
            out.u16(room.site_capacity);
            out.bool(room.direct_stubs);
            out.u16(room.next_site);
            out.u32(room.name_shared.len() as u32);
            for (store, atom, site) in room.name_shared {
                out.bool(store);
                out.atom(atom);
                out.u16(site);
            }
        }
        None => out.bool(false),
    }
    // The room that deferred rewrites append their sites into: the program's table headroom at
    // the end of its load (`ResidualProgram::finish`), which a decoder restores, so a first read
    // of a deferred record appends as the load's own would have had room to. Only a program that
    // defers any keeps it; its flag says whether the decoder restores the room.
    out.bool(program.rewrite_ranges.kept_pending);
    if program.rewrite_ranges.kept_pending {
        for spare in [program.field_sites.spare(), program.superinstructions.spare()] {
            out.u32(u32::try_from(spare).expect("table headroom fits u32"));
        }
    }
    let order = match out.atoms {
        AtomEncoding::Program => {
            header.strings(&program.atoms);
            Vec::new()
        }
        AtomEncoding::Compact(compact) => {
            // The names' lengths, then their text as one run, which the reader validates as
            // UTF-8 at once instead of name by name.
            header.u32(compact.order.len() as u32);
            for &atom in &compact.order {
                header.u32(program.atoms[atom as usize].len() as u32);
            }
            for &atom in &compact.order {
                header.bytes.extend_from_slice(program.atoms[atom as usize].as_bytes());
            }
            compact.order
        }
        AtomEncoding::Shared => {
            header.u32(program.atoms.len() as u32);
            Vec::new()
        }
    };
    header.bytes.extend_from_slice(&out.bytes);
    header.bytes.extend_from_slice(&region);
    header.u32(u32::try_from(region.len()).expect("deferred records fit u32 bytes"));
    (header.bytes, order)
}

pub(super) fn read_program(path: &std::path::Path) -> Result<super::ResidualProgram, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    read_program_bytes(std::rc::Rc::new(bytes))
}

pub(super) fn read_program_bytes(bytes: super::KeptBytes) -> Result<super::ResidualProgram, String> {
    let length = bytes.as_ref().as_ref().len();
    let program = decode_program(&bytes, length, ReadAtoms::Program, None)?;
    program.validate()?;
    Ok(program)
}

/// Decodes `encode_compact_program`'s bytes, naming each atom by `intern`'s id for its text
/// and supplying `text` as the source, once their checksum shows they are what the encoder
/// wrote. The program's atom table is left empty for the caller, which owns the ids `intern`
/// returns.
pub(super) fn decode_compact_program(
    bytes: super::KeptBytes,
    intern: &mut dyn FnMut(&str) -> u32,
    text: super::SourceText,
) -> Result<super::ResidualProgram, String> {
    let encoding = sealed_encoding_length(bytes.as_ref().as_ref())?;
    decode_program(&bytes, encoding, ReadAtoms::Compact(intern), Some(text))
}

pub(super) fn encode_native_program(program: &super::ResidualProgram) -> Vec<u8> {
    crate::vm::freeze_call_sites(program);
    sealed(encode_program(program, AtomEncoding::Shared, SourceEncoding::Included, true).0)
}

/// A snapshot's encoding of `program` (`crate::snapshot`): the program's own atom ids, which
/// its VM's restored atom table keeps, and no source text, which the snapshot writes apart.
pub(super) fn encode_snapshot_program(program: &super::ResidualProgram, retention: super::ImageRetention) -> Vec<u8> {
    let source = retained_source(SourceEncoding::External, retention);
    sealed(encode_program(program, AtomEncoding::Shared, source, retention.stack_positions).0)
}

/// Checks that `bytes` are an `encode_snapshot_program` encoding whose checksum holds, as
/// decoding them would first.
pub(super) fn verify_snapshot_program(bytes: &[u8]) -> Result<(), String> {
    sealed_encoding_length(bytes).map(drop)
}

/// `encode_snapshot_program`'s bytes, with `text` as the source; the caller gives the program
/// its VM's atom table, as long as the returned count.
pub(super) fn decode_snapshot_program(
    bytes: super::KeptBytes,
    text: Option<super::SourceText>,
) -> Result<(super::ResidualProgram, usize), String> {
    let encoding = sealed_encoding_length(bytes.as_ref().as_ref())?;
    let mut atom_count = 0;
    let program = decode_program(&bytes, encoding, ReadAtoms::Shared(&mut atom_count), text)?;
    Ok((program, atom_count))
}

/// `text`, a string the encoding `encoded` holds, as a view of those kept bytes rather than a
/// copy of it: `text` lies within them, and the reader checked it is UTF-8.
fn kept_residual_text(encoded: &super::KeptBytes, text: &str) -> Result<super::SourceText, String> {
    let bytes: &[u8] = encoded.as_ref().as_ref();
    let base = bytes.as_ptr() as usize;
    let start = (text.as_ptr() as usize).checked_sub(base).ok_or("residual source text is outside its encoding")?;
    Ok(super::SourceText::Kept(super::KeptText { bytes: encoded.clone(), range: start..start + text.len() }))
}

/// How a decoder reads atoms (`AtomEncoding`).
enum ReadAtoms<'a> {
    /// The table, in the program's ids.
    Program,
    /// A compact table, whose names `intern` gives the reader's ids.
    Compact(&'a mut dyn FnMut(&str) -> u32),
    /// The program's own table, whose names `intern` gives the reader's ids.
    Interned(&'a mut dyn FnMut(&str) -> u32),
    /// Only the length of the table the program shares with its reader, written here.
    Shared(&'a mut usize),
}

/// The frozen call-site descriptions and call offsets (`write_call_sites`).
fn read_call_sites(input: &mut BinaryReader<'_>) -> Result<(std::cell::OnceCell<std::sync::Arc<[super::CallSiteDescription]>>, std::cell::OnceCell<Box<[super::CallPc]>>), String> {
    let call_sites = if input.bool()? {
        let sites = input.list(|input| Ok(super::CallSiteDescription {
            offset: input.u32()?,
            construct: input.bool()?,
            callee: input.string()?,
            expression: if input.bool()? {
                Some(super::SourceSpan { start: input.u32()?, end: input.u32()? })
            } else { None },
        }))?;
        std::cell::OnceCell::from(std::sync::Arc::from(sites))
    } else {
        Default::default()
    };
    let call_pcs = if input.bool()? {
        let pcs = input.list(|input| Ok(super::CallPc { function: input.u32()?, pc: input.u32()?, offset: input.u32()? }))?;
        std::cell::OnceCell::from(pcs.into_boxed_slice())
    } else {
        Default::default()
    };
    Ok((call_sites, call_pcs))
}

/// Writes the frozen call-site descriptions and call offsets of `record`. An expression names a
/// span of the source text, so it is written only with the text (`keep_expressions`).
fn write_call_sites(out: &mut BinaryWriter, record: &super::ProgramRecord, keep_expressions: bool) {
    out.bool(record.call_sites.get().is_some());
    if let Some(sites) = record.call_sites.get() {
        out.u32(sites.len() as u32);
        for site in sites.iter() {
            out.u32(site.offset);
            out.bool(site.construct);
            out.string(&site.callee);
            let expression = site.expression.filter(|_| keep_expressions);
            out.bool(expression.is_some());
            if let Some(span) = expression {
                out.u32(span.start);
                out.u32(span.end);
            }
        }
    }
    out.bool(record.call_pcs.get().is_some());
    if let Some(pcs) = record.call_pcs.get() {
        out.u32(pcs.len() as u32);
        for call in pcs.iter() {
            out.u32(call.function);
            out.u32(call.pc);
            out.u32(call.offset);
        }
    }
}

/// Decodes the encoding in the first `length` bytes of `encoded`, which a program with
/// deferred records keeps: their region is read only as each decodes. `elided` is the
/// source text an encoding left out.
fn decode_program(
    encoded: &super::KeptBytes,
    length: usize,
    mut read_atoms: ReadAtoms<'_>,
    elided: Option<super::SourceText>,
) -> Result<super::ResidualProgram, String> {
    let encoding = encoded.as_ref().as_ref().get(..length).ok_or("truncated residual")?;
    let region = region_start(encoding)?;
    let region_length = encoding.len() - size_of::<u32>() - region;
    let bytes = &encoding[..region];
    let mut elided = elided;
    let mut input = BinaryReader::new(bytes);
    input.magic(RESIDUAL_MAGIC)?;
    let abi = input.u64()?;
    if abi != super::ResidualProgram::RUNTIME_ABI_FINGERPRINT {
        return Err("residual runtime ABI mismatch".into());
    }
    let specialized = match input.u8()? {
        0 => false,
        1 => true,
        _ => return Err("invalid residual specialization mode".into()),
    };
    let kind = super::ProgramKind::from_binary_tag(input.u8()?)
        .ok_or_else(|| "invalid residual program kind".to_owned())?;
    let source_name = input.string()?;
    let text = match input.u8()? {
        OPTIONAL_STRING_NONE => None,
        OPTIONAL_STRING_SOME => Some(kept_residual_text(encoded, input.str_ref()?)?),
        OPTIONAL_STRING_ELIDED => Some(elided.take().ok_or("residual source text is not supplied")?),
        _ => return Err("invalid optional residual string".into()),
    };
    let source = match text {
        Some(text) => Some(super::ProgramSource {
            text,
            typescript: input.bool()?,
            base: input.u32()?,
            line_offset: input.u32()?,
            column_offset: input.u32()?,
            layout: Default::default(),
        }),
        None => None,
    };
    let (call_sites, call_pcs) = read_call_sites(&mut input)?;
    let module_requests = read_module_requests(&mut input)?;
    let module_imports = input.list(|input| {
        let ModuleRequest { source, phase, module_type } = read_module_request(input)?;
        let imported = match ModuleImportNameKind::from_binary_tag(input.u8()?)
            .ok_or_else(|| "invalid residual module import name".to_string())?
        {
            ModuleImportNameKind::Namespace => ModuleImportName::Namespace,
            ModuleImportNameKind::Named => ModuleImportName::Named(input.string()?),
        };
        let local = input.string()?;
        Ok(ModuleImportBinding {
            source,
            phase,
            module_type,
            imported,
            local,
        })
    })?;
    let module_link_plan = read_module_link_plan(&mut input)?;
    let declared_types = read_declared_types(&mut input)?;
    let atoms = match &mut read_atoms {
        ReadAtoms::Program => input.strings()?,
        ReadAtoms::Shared(count) => {
            **count = input.u32()? as usize;
            AtomTable::default()
        }
        ReadAtoms::Interned(intern) => {
            let count = input.u32()? as usize;
            let mut ids = Vec::with_capacity(count);
            for _ in 0..count {
                let len = input.u32()? as usize;
                let name = std::str::from_utf8(input.take(len)?).map_err(|_| "invalid residual string")?;
                ids.push(intern(name));
            }
            input.atoms = Some(ids.into());
            AtomTable::default()
        }
        ReadAtoms::Compact(intern) => {
            let count = input.u32()? as usize;
            let lengths = (0..count).map(|_| input.u32().map(|len| len as usize)).collect::<Result<Vec<_>, _>>()?;
            let total = lengths.iter().try_fold(0usize, |total, &len| total.checked_add(len));
            let text = std::str::from_utf8(input.take(total.ok_or("invalid residual string")?)?)
                .map_err(|_| "invalid residual string")?;
            let mut ids = Vec::with_capacity(count);
            let mut start = 0;
            for len in lengths {
                let name = text.get(start..start + len).ok_or("invalid residual string")?;
                ids.push(intern(name));
                start += len;
            }
            input.atoms = Some(ids.into());
            AtomTable::default()
        }
    };
    let constants = input.list(|input| match input.u8()? {
        WASM_BITS64_TAG => Ok(Constant::WasmBits64(input.u64()?)),
        WASM_V128_TAG => Ok(Constant::WasmV128(
            input.take(crate::wasm::V128_BYTES)?.try_into().unwrap(),
        )),
        0 => Ok(Constant::Number(f64::from_bits(crate::Value::from_f64_raw(f64::from_bits(input.u64()?)).to_bits()))),
        1 => Ok(Constant::String(input.string()?.into())),
        7 => Ok(Constant::StringUnits(input.u16s()?.into())),
        6 => Ok(Constant::BigInt(Box::new(input
            .string()?
            .parse()
            .map_err(|_| "invalid residual BigInt constant")?))),
        2 => Ok(Constant::Boolean(true)),
        3 => Ok(Constant::Boolean(false)),
        4 => Ok(Constant::Null),
        5 => Ok(Constant::Undefined),
        _ => Err("invalid residual constant".into()),
    })?;
    let function_count = input.u32()? as usize;
    let mut functions = Vec::with_capacity(function_count);
    let mut ranges = Vec::with_capacity(function_count);
    let mut deferred_rewrites = Vec::new();
    for index in 0..function_count {
        let entry = input.u8()?;
        if entry > DEFERRED_UNREWRITTEN {
            return Err("invalid residual deferred record flag".into());
        }
        if entry == DEFERRED_UNREWRITTEN {
            deferred_rewrites.push(index as u32);
        }
        let encoded_deferred = entry != 0;
        let header = read_function_header(&mut input)?;
        let length = input.u32()?;
        if encoded_deferred {
            let offset = input.u32()? as usize;
            let checksum = input.u64()?;
            if offset.checked_add(length as usize).is_none_or(|end| end > region_length) {
                return Err("deferred record outside its region".into());
            }
            let start = u32::try_from(region + offset).map_err(|_| "residual records exceed u32")?;
            let record = super::DeferredRecord::new(start, length, checksum, entry >= DEFERRED_UNLOWERED, entry == DEFERRED_UNREWRITTEN)
                .ok_or("deferred record exceeds the encoding's record length")?;
            ranges.push(Some(record));
            functions.push(FunctionSlot::released(header));
            continue;
        }
        let record = input.take(length as usize)?;
        let mut reader = input.sub_reader(record);
        let function = read_function(&mut reader, None)?;
        reader.finish()?;
        ranges.push(None);
        functions.push(FunctionSlot::compiled(function));
    }
    let encoded_records = ranges.iter().any(Option::is_some).then(|| {
        std::rc::Rc::new(super::EncodedRecords {
            bytes: encoded.clone(),
            atoms: input.atoms.clone(),
            ranges: ranges.into_boxed_slice(),
        })
    });
    let cache_sites = input.u16()?;
    let method_sites = input.list(|input| {
        Ok(MethodSite {
            atom: input.atom()?,
            cache: input.u16()?,
            argument_start: input.u32()?,
            argument_count: input.u16()?,
            receiver_path: input.optional_atom_pair()?.into(),
        })
    })?;
    let method_arguments = input.u16s()?;
    let field_sites = input.list(|input| {
        Ok(FieldSite {
            base: FieldBase(input.u16()?),
            first: input.atom_pair()?,
            second: input.optional_atom_pair()?,
            sink: input.optional_atom_pair()?,
            sources: [
                field_source_word(input)?,
                field_source_word(input)?,
                field_source_word(input)?,
            ],
        })
    })?;
    let object_sites = input.list(|input| {
        Ok(ObjectSite {
            atoms: input.list(|input| input.atom())?.into(),
        })
    })?;
    let regexp_literal_sites = input.list(|input| {
        Ok(RegExpLiteralSite {
            pattern_constant: input.u32()?,
            flags_constant: input.u32()?,
            receiver: input.bool()?,
        })
    })?;
    let literal_templates = input.list(|input| {
        Ok(input
            .list(|input| {
                let tag = input.u8()?;
                let operand = input.u32()?;
                Ok(match tag {
                    LITERAL_STEP_CONSTANT => LiteralStep::Constant(operand),
                    LITERAL_STEP_ARRAY => LiteralStep::Array(operand),
                    LITERAL_STEP_OBJECT => LiteralStep::Object(operand),
                    LITERAL_STEP_INT => LiteralStep::Int(operand.cast_signed()),
                    LITERAL_STEP_JSON => LiteralStep::Json(operand),
                    _ => return Err("invalid residual literal template step".into()),
                })
            })?
            .into_boxed_slice())
    })?;
    let superinstructions = input.list(|input| {
        let mut code = [Instr::new(Op::Nop, 0, 0, 0, 0); 4];
        for instruction in &mut code {
            let opcode = input.u16()?;
            if usize::from(opcode) >= Op::COUNT {
                return Err("invalid residual superinstruction opcode".into());
            }
            // SAFETY: same contiguous repr(u16) invariant as normal code.
            let op = unsafe { std::mem::transmute::<u16, Op>(opcode) };
            let (a, b, c) = (input.u16()?, input.u16()?, input.u16()?);
            *instruction =
                Instr::try_new(op, a, b, c, input.immediate(op, b)?)
                    .ok_or_else(|| {
                        String::from("residual superinstruction exceeds packed domain")
                    })?;
        }
        if code.map(|instruction| instruction.op())
            != [Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2]
        {
            return Err("invalid residual superinstruction pattern".into());
        }
        Ok(Superinstruction::new(code))
    })?;
    let (closure_reads, first_call_scopes) = read_closure_tables(&mut input, functions.len())?;
    let chunk_room = if input.bool()? {
        let (mut spares, mut lengths) = ([0; 9], [0; 9]);
        for value in spares.iter_mut().chain(lengths.iter_mut()) {
            *value = input.u32()?;
        }
        let (site_capacity, direct_stubs, next_site) = (input.u16()?, input.bool()?, input.u16()?);
        let name_shared = input.list(|input| Ok((input.bool()?, input.atom()?, input.u16()?)))?;
        Some(super::chunk_append::KeptChunkRoom { spares, lengths, site_capacity, direct_stubs, next_site, name_shared })
    } else {
        None
    };
    // The rewrite room of `encode_program`, present when the program defers rewrites.
    let rewrite_room = if input.bool()? { Some([input.u32()? as usize, input.u32()? as usize]) } else { None };
    input.finish()?;
    let mut program = super::ResidualProgram {
        specialized,
        kind,
        module_requests,
        module_imports,
        module_link_plan,
        declared_types,
        source_name,
        source,
        record: super::ProgramRecord { atoms, constants: constants.into(), call_sites, call_pcs },
        functions: functions.into(),
        cache_sites: cache_sites.into(),
        method_sites: method_sites.into(),
        method_arguments: method_arguments.into(),
        field_sites: field_sites.into(),
        object_sites: object_sites.into(),
        regexp_literal_sites: regexp_literal_sites.into(),
        literal_templates: literal_templates.into(),
        superinstructions: superinstructions.into(),
        rewrite_ranges: crate::compile::replay::RewriteRanges::new(rewrite_room.is_some(), deferred_rewrites),
        replay: None, first_call_scopes,
        encoded_records,
        validated: Default::default(),
        undefined_constant: Default::default(),
        load_encoding: Default::default(),
        closure_reads,
        wasm_lazy: None,
    };
    if let Some([fields, supers]) = rewrite_room {
        // The deferred rewrites append into this room, as they would have into the load's.
        program.field_sites.reserve_unshared(fields);
        program.superinstructions.reserve_unshared(supers);
    }
    if let Some(room) = &chunk_room {
        program.restore_chunk_room(room);
    }
    Ok(program)
}

/// The body's entry for a deferred record, `record` of `region`: its length, its place and its
/// checksum, which a decoder checks as it decodes the record (`decode_deferred_function`).
/// Runs `write` with `target` as `out`'s bytes, so what it writes appends to `target` where it
/// stays; returns the span it took there. A record is encoded in place, not copied from a buffer.
fn append_to(out: &mut BinaryWriter, target: &mut Vec<u8>, write: impl FnOnce(&mut BinaryWriter)) -> std::ops::Range<usize> {
    std::mem::swap(&mut out.bytes, target);
    let start = out.bytes.len();
    write(out);
    let end = out.bytes.len();
    std::mem::swap(&mut out.bytes, target);
    start..end
}

/// Writes function `index`'s record (`write_function`) at the end of `out`, and returns whether
/// its names resolve statically, given the facts of the functions encoded before it.
fn write_record(program: &super::ResidualProgram, index: usize, static_names: &[bool], out: &mut BinaryWriter) -> bool {
    program.with_record(index, |record| {
        let names_static = record.lazy_flag(super::LazyFlag::StaticNameScope, || {
            record.own_names_resolve_statically(program) && enclosing_names_static(program, static_names, record.parent)
        });
        let facts = record.derived_facts(program, names_static);
        write_function(out, record, &program.superinstructions, facts);
        names_static
    })
}

fn write_deferred_entry(out: &mut BinaryWriter, region: &[u8], record: std::ops::Range<usize>) {
    out.u32(u32::try_from(record.len()).expect("a function record fits u32 bytes"));
    out.u32(u32::try_from(record.start).expect("deferred records fit u32 bytes"));
    out.u64(compact_checksum(&region[record]));
}

/// Whether names resolve statically in a function's enclosing function `parent`
/// (`Function::names_resolve_statically`): `settled` holds the fact of each function encoded
/// so far, which an enclosing function is, as it is compiled first.
fn enclosing_names_static(program: &super::ResidualProgram, settled: &[bool], parent: Option<u32>) -> bool {
    parent.is_none_or(|parent| match settled.get(parent as usize) {
        Some(&settled) => settled,
        None => program
            .function_get(parent as usize)
            .is_some_and(|parent| parent.names_resolve_statically(program)),
    })
}

/// Settles the static-name-scope fact of a record its load encoded with the function's own
/// (`LoadEncoding::add`), which holds only where it holds in the `enclosing` functions too.
fn settle_static_name_scope(record: &mut [u8], enclosing: bool) -> bool {
    let at = record.len() - RECORD_DERIVED_FACTS_SIZE;
    let field: &mut [u8; RECORD_DERIVED_FACTS_SIZE] =
        (&mut record[at..]).try_into().expect("a record ends with its derived facts");
    let mut facts = u32::from_le_bytes(*field);
    if !enclosing {
        facts &= !DerivedFact::StaticNameScope.bit();
        *field = facts.to_le_bytes();
    }
    facts & DerivedFact::StaticNameScope.bit() != 0
}

/// The compile-cache records of the functions a compilation finishes and releases during its
/// load (`Compiler::finish_early`), each encoded as its function is finished: the program's
/// compact encoding extends their region (`encode_program`) instead of compiling every
/// released function again to write it.
pub(crate) struct LoadEncoding {
    /// The records, one after another in the order their functions finished, under compact
    /// atom ids, which the program's encoding continues.
    out: BinaryWriter,
    /// Each encoded function's record in the region.
    records: rustc_hash::FxHashMap<u32, std::ops::Range<u32>>,
}

impl Default for LoadEncoding {
    fn default() -> Self {
        Self {
            out: BinaryWriter::with_atoms(AtomEncoding::Compact(CompactAtoms::default())),
            records: Default::default(),
        }
    }
}

impl LoadEncoding {
    /// Encodes the record of finished function `id`. Whether names resolve statically in it
    /// reads its enclosing functions, which are not finished yet: the record holds the
    /// function's own fact, which the program's encoding settles.
    pub(crate) fn add(&mut self, id: u32, function: &Function, tables: &impl FactTables) {
        let start = self.out.bytes.len();
        let facts = function.derived_facts(tables, function.own_names_resolve_statically(tables));
        write_function(&mut self.out, function, tables.superinstructions(), facts);
        let range = u32::try_from(start).ok().zip(u32::try_from(self.out.bytes.len()).ok());
        let (start, end) = range.expect("encoded records fit u32 bytes");
        self.records.insert(id, start..end);
    }

    /// Names function `id`'s record by `atom`, as its load names a function it released
    /// before naming it (`FunctionTable::name_released`).
    pub(crate) fn name(&mut self, id: u32, atom: u32) {
        let Some(range) = self.records.get(&id) else {
            return;
        };
        let at = range.start as usize + RECORD_INFERRED_NAME_AT;
        let encoded = self.out.encoded_atom(atom);
        self.out.bytes[at..at + size_of::<u32>()].copy_from_slice(&encoded.to_le_bytes());
    }
}

/// A program's `LoadEncoding` until its first compact encoding takes it. A clone, which its
/// owner may change, starts without one: its encoding compiles its released records again.
#[derive(Default)]
pub(crate) struct LoadRecords(std::cell::Cell<Option<Box<LoadEncoding>>>);

impl LoadRecords {
    pub(crate) fn new(encoding: Option<LoadEncoding>) -> Self {
        Self(std::cell::Cell::new(encoding.map(Box::new)))
    }

    fn take(&self) -> Option<Box<LoadEncoding>> {
        self.0.take()
    }
}

impl Clone for LoadRecords {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for LoadRecords {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LoadRecords").finish_non_exhaustive()
    }
}

/// An unlowered record in the compact form its slot keeps until the function's first read
/// (`FunctionSlot::unlowered`): LEB128 integers under the program's own atom ids, without the
/// memos a lowered record's encoding carries. Most loaded functions never run, and this form
/// takes about half the bytes of the record.
pub(crate) fn pack_unlowered(function: &Function) -> Box<PackedUnlowered> {
    let mut out = BinaryWriter::new();
    out.packed = true;
    out.bytes.reserve(64 + 6 * (function.code.len() + function.wide.len()));
    write_function(&mut out, function, &[], 0);
    Box::new(PackedUnlowered { bytes: out.bytes.into_boxed_slice(), name_bindings: function.name_bindings.clone() })
}

/// Whether `a` and `b` pack to the same bytes: a debug check that two records agree.
pub(crate) fn packs_alike(a: &Function, b: &Function) -> bool {
    pack_unlowered(a).bytes == pack_unlowered(b).bytes
}

/// `pack_unlowered`'s record, over `header`: the slot's header, which holds the fields a packed
/// record leaves out (`write_function`).
pub(crate) fn unpack_unlowered(packed: &PackedUnlowered, header: &super::FunctionHeader) -> Function {
    let mut input = BinaryReader::new(&packed.bytes);
    input.packed = true;
    let mut function = read_function(&mut input, Some(header)).unwrap_or_else(|error| panic!("a packed unlowered record decodes: {error}"));
    function.name_bindings = packed.name_bindings.clone();
    function
}

/// An unlowered record's packed bytes and its name bindings, which share their captured scopes
/// with the records of the functions around it.
#[derive(Clone)]
pub(crate) struct PackedUnlowered {
    bytes: Box<[u8]>,
    name_bindings: super::NameBindings,
}

impl std::fmt::Debug for PackedUnlowered {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PackedUnlowered").field("bytes", &self.bytes.len()).finish_non_exhaustive()
    }
}


/// Where `write_function` writes a record's inferred name: after its parent and its name,
/// one u32 each.
const RECORD_INFERRED_NAME_AT: usize = 2 * size_of::<u32>();

/// The size of the derived facts `write_function` ends a record with.
const RECORD_DERIVED_FACTS_SIZE: usize = size_of::<u32>();

/// A base class constructor's leading constant fields (`Function::constant_fields`): their
/// count, then each field's atom and its primitive's tag, with the number's bits or the string's
/// atom after the tags that carry one.
fn write_constant_fields(out: &mut BinaryWriter, fields: &[super::ConstantField]) {
    out.u32(fields.len() as u32);
    for field in fields {
        out.atom(field.atom);
        match field.value {
            super::FieldConstant::Undefined => out.u8(CONSTANT_FIELD_UNDEFINED),
            super::FieldConstant::Null => out.u8(CONSTANT_FIELD_NULL),
            super::FieldConstant::Boolean(false) => out.u8(CONSTANT_FIELD_FALSE),
            super::FieldConstant::Boolean(true) => out.u8(CONSTANT_FIELD_TRUE),
            super::FieldConstant::Number(number) => {
                out.u8(CONSTANT_FIELD_NUMBER);
                out.u64(number.to_bits());
            }
            super::FieldConstant::String(atom) => {
                out.u8(CONSTANT_FIELD_STRING);
                out.atom(atom);
            }
        }
    }
}

fn read_constant_fields(input: &mut BinaryReader<'_>) -> Result<Vec<super::ConstantField>, String> {
    input.list(|input| {
        let atom = input.atom()?;
        let value = match input.u8()? {
            CONSTANT_FIELD_UNDEFINED => super::FieldConstant::Undefined,
            CONSTANT_FIELD_NULL => super::FieldConstant::Null,
            CONSTANT_FIELD_FALSE => super::FieldConstant::Boolean(false),
            CONSTANT_FIELD_TRUE => super::FieldConstant::Boolean(true),
            CONSTANT_FIELD_NUMBER => super::FieldConstant::Number(f64::from_bits(crate::Value::from_f64_raw(f64::from_bits(input.u64()?)).to_bits())),
            CONSTANT_FIELD_STRING => super::FieldConstant::String(input.atom()?),
            _ => return Err("invalid constant field tag".to_string()),
        };
        Ok(super::ConstantField { atom, value })
    })
}

const CONSTANT_FIELD_UNDEFINED: u8 = 0;
const CONSTANT_FIELD_NULL: u8 = 1;
const CONSTANT_FIELD_FALSE: u8 = 2;
const CONSTANT_FIELD_TRUE: u8 = 3;
const CONSTANT_FIELD_NUMBER: u8 = 4;
const CONSTANT_FIELD_STRING: u8 = 5;

/// A function's record, which `read_function` decodes, with the projection of its plain local
/// slots (`Function::plain_local_slots`) that the interpreter memoizes on first call and its
/// `derived_facts`: a decoder sets the memos instead of running the analyses again in every
/// process.
fn write_function(out: &mut BinaryWriter, function: &Function, superinstructions: &[Superinstruction], derived_facts: u32) {
        let start = out.bytes.len();
        // A packed record leaves out the fields its slot's header holds (`unpack_unlowered`).
        if !out.packed {
            out.option_u32(function.parent);
            out.option_atom(function.name);
            debug_assert!(out.bytes.len() - start == RECORD_INFERRED_NAME_AT);
            out.option_atom(function.inferred_name);
            out.u8(u8::from(function.is_arrow));
        }
        out.u16(function.self_binding_slot.unwrap_or(u16::MAX));
        if !out.packed {
            out.u8(u8::from(function.source_span.is_some()));
            if let Some(span) = function.source_span {
                out.u32(span.start);
                out.u32(span.end);
            }
        }
        out.u16(function.params);
        if !out.packed {
            out.u16(function.length);
        }
        out.u32(function.parameter_end_pc);
        out.u32(function.parameter_atoms.len() as u32);
        for atom in &function.parameter_atoms {
            out.atom(*atom);
        }
        out.u32(function.declared_parameters().len() as u32);
        for declared in function.declared_parameters() {
            out.u16(*declared);
        }
        out.u8(u8::from(function.rest));
        out.u8(u8::from(function.is_async));
        out.u8(u8::from(function.is_generator));
        out.u8(u8::from(function.is_class_constructor));
        out.u8(u8::from(function.derived_constructor));
        out.option_u32(function.instance_initializer());
        write_constant_fields(out, &function.constant_fields);
        out.option_atom(function.super_home_atom);
        out.u8(u8::from(function.constructible));
        out.u8(u8::from(function.class_initializer));
        out.u8(u8::from(function.prototype_unobservable));
        out.u8(u8::from(function.parameter_eval_arguments_error));
        out.u8(u8::from(function.unaliased_arguments) * UNALIASED_ARGUMENTS_BIT | u8::from(function.rest_alias) * REST_ALIAS_BIT);
        out.u8(u8::from(function.simple_parameters));
        out.u8(u8::from(function.strict));
        out.u16(function.arguments_slot.unwrap_or(u16::MAX));
        out.u16(function.locals);
        out.u32(function.local_atoms.len() as u32);
        for atom in &function.local_atoms {
            out.atom(*atom);
        }
        out.u32(function.local_registers.len() as u32);
        for local_register in &function.local_registers {
            out.u16(local_register.local);
            out.u16(local_register.register);
        }
        out.u32(function.environment_atoms.len() as u32);
        for atom in &function.environment_atoms {
            out.atom(*atom);
        }
        // A packed record's bindings stay beside it, shared with its scope (`PackedUnlowered`).
        let name_bindings = if out.packed { Vec::new() } else { function.name_bindings.resolved() };
        out.u32(name_bindings.len() as u32);
        for binding in &name_bindings {
            write_eval_binding(out, binding);
        }
        function.environment_clones.write(out);
        match &function.selective_capture_slots {
            Some(slots) => {
                out.u8(1);
                out.u16s(slots);
            }
            None => out.u8(0),
        }
        out.u16s(&function.declared_functions);
        out.u16s(&function.declared_captures);
        out.u8(u8::from(function.inherited_with_scope));
        out.u32(function.lexical_atoms.len() as u32);
        for atom in &function.lexical_atoms {
            out.atom(*atom);
        }
        out.u32(function.global_lexical_atoms().len() as u32);
        for atom in function.global_lexical_atoms() {
            out.atom(*atom);
        }
        out.u32(function.global_var_atoms().len() as u32);
        for atom in function.global_var_atoms() {
            out.atom(*atom);
        }
        out.u32(function.global_function_atoms().len() as u32);
        for atom in function.global_function_atoms() {
            out.atom(*atom);
        }
        out.u32(function.global_annex_b_var_atoms().len() as u32);
        for atom in function.global_annex_b_var_atoms() {
            out.atom(*atom);
        }
        out.u32(function.global_immutable_atoms().len() as u32);
        for atom in function.global_immutable_atoms() {
            out.atom(*atom);
        }
        out.u32(function.binding_sites.len() as u32);
        for site in &function.binding_sites {
            out.u32(site.resume_pc);
            out.u32(site.bindings.len() as u32);
            for binding in &site.bindings {
                write_eval_binding(out, binding);
            }
        }
        out.u16(function.registers);
        if !out.packed {
            out.u8(function.dispatch as u8);
        }
        out.u8(u8::from(function.register_roots.covered()));
        out.u16(function.undefined_register.unwrap_or(NO_UNDEFINED_REGISTER));
        out.u16(
            function
                .parameter_registers
                .unwrap_or(NO_PARAMETER_REGISTERS),
        );
        out.u8(function.initial_register as u8);
        out.u32(function.code.len() as u32);
        for instruction in &function.code {
            out.instruction(instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm());
        }
        out.u32(function.wide.len() as u32);
        for instruction in &function.wide {
            out.instruction(instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm());
        }
        out.u32(function.handlers.len() as u32);
        for handler in &function.handlers {
            out.u32(handler.start);
            out.u32(handler.end);
            out.u32(handler.target);
            let (kind, slot) = match handler.slot {
                None => (THROWN_VALUE_DISCARDED, u16::MAX),
                Some(super::ThrownValueSlot::Local(slot)) => (THROWN_VALUE_IN_LOCAL, slot),
                Some(super::ThrownValueSlot::Register(register)) => (THROWN_VALUE_IN_REGISTER, register),
            };
            out.u8(kind);
            out.u16(slot);
            out.u32(handler.return_target.unwrap_or(u32::MAX));
            out.u16(handler.return_slot.unwrap_or(u16::MAX));
            out.u16(handler.with_depth);
        }
        out.source_positions(&function.source_positions);
        // The memos below are a lowered record's, which an unlowered one derives once lowered.
        if out.packed || out.unlowered {
            return;
        }
        let computed;
        let plain: &[u64] = match function.plain_locals.get() {
            Some(words) => &words[..],
            None => {
                computed = function.plain_local_slots(superinstructions);
                computed.words()
            }
        };
        for byte in 0..usize::from(function.locals).div_ceil(u8::BITS as usize) {
            out.u8((plain[byte / 8] >> (byte % 8 * 8)) as u8);
        }
        out.u32(derived_facts);
}

/// The header a decoder keeps for a function whose record stays encoded (`FunctionHeader`).
fn write_function_header(out: &mut BinaryWriter, header: &super::FunctionHeader) {
    out.option_u32(header.parent);
    out.option_atom(header.name);
    out.option_atom(header.inferred_name);
    out.u8(u8::from(header.source_span.is_some()));
    if let Some(span) = header.source_span {
        out.u32(span.start);
        out.u32(span.end);
    }
    out.u16(header.length);
    out.u8(header.dispatch as u8);
    for flag in [
        header.is_arrow,
        header.is_async,
        header.is_generator,
        header.is_class_constructor,
        header.constructible,
        header.prototype_unobservable,
        header.inherited_with_scope,
    ] {
        out.u8(u8::from(flag));
    }
}

/// `write_function`'s record.
/// `header` is a packed record's slot header, which holds the fields `write_function` leaves out
/// of a packed record; a disk record carries them and reads with `None`.
fn read_function(input: &mut BinaryReader<'_>, header: Option<&super::FunctionHeader>) -> Result<Function, String> {
        debug_assert_eq!(header.is_some(), input.packed);
        let (parent, name, inferred_name) = match header {
            Some(header) => (header.parent, header.name, header.inferred_name),
            None => (input.option_u32()?, input.option_atom()?, input.option_atom()?),
        };
        let is_arrow = match header {
            Some(header) => header.is_arrow,
            None => input.flag("arrow function")?,
        };
        let self_binding_slot = match input.u16()? {
            u16::MAX => None,
            slot => Some(slot),
        };
        let source_span = match header {
            Some(header) => header.source_span,
            None => match input.u8()? {
                0 => None,
                _ => Some(super::SourceSpan {
                    start: input.u32()?,
                    end: input.u32()?,
                }),
            },
        };
        let params = input.u16()?;
        let length = match header {
            Some(header) => header.length,
            None => input.u16()?,
        };
        let parameter_end_pc = input.u32()?;
        let parameter_atoms = input.list(|input| input.atom())?;
        let declared_parameters = input.list(|input| input.u16())?;
        let rest = input.flag("rest")?;
        let is_async = input.flag("async function")?;
        let is_generator = input.flag("generator function")?;
        let is_class_constructor = input.flag("class constructor")?;
        let derived_constructor = input.flag("derived constructor")?;
        let instance_initializer = input.option_u32()?;
        let constant_fields = read_constant_fields(input)?;
        let super_home_atom = input.option_atom()?;
        let constructible = input.flag("constructible function")?;
        let class_initializer = input.flag("class field initializer")?;
        let prototype_unobservable = input.flag("prototype observability")?;
        let parameter_eval_arguments_error = input.flag("parameter eval")?;
        let (unaliased_arguments, rest_alias) = match input.u8()? {
            flags if flags & !(UNALIASED_ARGUMENTS_BIT | REST_ALIAS_BIT) == 0 => (flags & UNALIASED_ARGUMENTS_BIT != 0, flags & REST_ALIAS_BIT != 0),
            _ => return Err("invalid unaliased arguments flags".into()),
        };
        let simple_parameters = input.flag("simple parameters")?;
        let strict = input.flag("strict function")?;
        let arguments_slot = match input.u16()? {
            u16::MAX => None,
            slot => Some(slot),
        };
        let locals = input.u16()?;
        let local_atoms = input.list(|input| input.atom())?;
        let local_registers = input.list(|input| {
            Ok(LocalRegister {
                local: input.u16()?,
                register: input.u16()?,
            })
        })?;
        let environment_atoms = input.list(|input| input.atom())?;
        let mut name_bindings = input.list(read_eval_binding)?;
        input.restore_atom_order(&mut name_bindings);
        let environment_clones = crate::bytecode::EnvironmentClones::read(input)?;
        let selective_capture_slots = match input.u8()? {
            0 => None,
            1 => Some(input.u16s()?.into()),
            _ => return Err("invalid selective capture layout".into()),
        };
        let declared_functions = input.u16s()?.into();
        let declared_captures = input.u16s()?.into();
        let inherited_with_scope = input.flag("inherited with-scope")?;
        let lexical_atoms = input.list(|input| input.atom())?;
        let global_lexical_atoms = input.list(|input| input.atom())?;
        let global_var_atoms = input.list(|input| input.atom())?;
        let global_function_atoms = input.list(|input| input.atom())?;
        let global_annex_b_var_atoms = input.list(|input| input.atom())?;
        let global_immutable_atoms = input.list(|input| input.atom())?;
        let binding_sites = input.list(|input| {
            let resume_pc = input.u32()?;
            let mut bindings = input.list(read_eval_binding)?;
            input.restore_atom_order(&mut bindings);
            Ok(BindingSite {
                resume_pc,
                bindings,
            })
        })?;
        let registers = input.u16()?;
        let dispatch = match header {
            Some(header) => header.dispatch,
            None => input.dispatch_class()?,
        };
        let covered = input.flag("register root coverage")?;
        let register_roots = super::RegisterRoots::new(covered);
        let undefined_register =
            Some(input.u16()?).filter(|register| *register != NO_UNDEFINED_REGISTER);
        let parameter_registers = Some(input.u16()?).filter(|base| *base != NO_PARAMETER_REGISTERS);
        let initial_register = match input.u8()? {
            0 => InitialRegister::Undefined,
            1 => InitialRegister::I32Zero,
            _ => return Err("invalid residual initial register".into()),
        };
        // A narrow instruction naming an atom the reader's id for which no longer fits it is
        // widened, as the compiler widens what does not fit (`Compiler::emit`).
        let mut widened = Vec::new();
        let mut pc = 0;
        let mut code = input.list(|input| {
            let at = pc;
            pc += 1;
            let (op, a, b, c, imm) = input.instruction()?;
            if op.is_wide_marker() {
                return Instr::wide_from_fields(a, b, c, imm)
                    .ok_or_else(|| String::from("residual wide index exceeds domain"));
            }
            Ok(Instr::try_new(op, a, b, c, imm).unwrap_or_else(|| {
                widened.push((at, WideInstruction::new(op, a, b, c, imm)));
                // Replaced by the widened entry's marker below.
                Instr::new(Op::Nop, 0, 0, 0, 0)
            }))
        })?;
        let mut wide = input.list(|input| {
            let (op, a, b, c, imm) = input.instruction()?;
            if op.is_wide_marker() {
                return Err("invalid residual wide opcode".into());
            }
            Ok(WideInstruction::new(op, a, b, c, imm))
        })?;
        for (at, instruction) in widened {
            code[at] = Instr::wide(wide.len())
                .ok_or_else(|| String::from("residual wide index exceeds domain"))?;
            wide.push(instruction);
        }
        if !super::local_loads_in_bounds(&code, &wide, locals) {
            return Err("invalid residual local load".into());
        }
        let handlers = input.list(|input| {
            let start = input.u32()?;
            let end = input.u32()?;
            let target = input.u32()?;
            let kind = input.u8()?;
            let slot = input.u16()?;
            let slot = match kind {
                THROWN_VALUE_DISCARDED => None,
                THROWN_VALUE_IN_LOCAL => Some(super::ThrownValueSlot::Local(slot)),
                THROWN_VALUE_IN_REGISTER => Some(super::ThrownValueSlot::Register(slot)),
                _ => return Err("invalid residual handler slot kind".into()),
            };
            let return_target = input.u32()?;
            let return_slot = input.u16()?;
            let with_depth = input.u16()?;
            Ok(Handler {
                start,
                end,
                target,
                slot,
                return_target: (return_target != u32::MAX).then_some(return_target),
                return_slot: (return_slot != u16::MAX).then_some(return_slot),
                with_depth,
            })
        })?;
        let source_positions = input.source_positions()?;
        let packed = input.packed || input.unlowered;
        let plain_bytes = if packed { Vec::new() } else { (0..usize::from(locals).div_ceil(u8::BITS as usize)).map(|_| input.u8()).collect::<Result<Vec<_>, _>>()? };
        let plain_locals: crate::heap::ThinBoxSlice<u64> = if packed { Default::default() } else { crate::bitset::words_from_le_bytes(usize::from(locals), &plain_bytes).into() };
        let derived_facts = if packed { 0 } else { input.u32()? };
        let function = Function {
            parent,
            name,
            inferred_name,
            is_arrow,
            self_binding_slot,
            source_span,
            params,
            length,
            parameter_end_pc,
            parameter_atoms: parameter_atoms.into(),
            rare: crate::bytecode::FunctionRare {
                declared_parameters: declared_parameters.into(),
                global_declarations: super::GlobalDeclarations::new(
                    global_lexical_atoms,
                    global_var_atoms,
                    global_function_atoms,
                    global_annex_b_var_atoms,
                    global_immutable_atoms,
                )
                .boxed(),
                instance_initializer,
            }
            .boxed(),
            rest,
            is_async,
            is_generator,
            is_class_constructor,
            derived_constructor,
            constant_fields: constant_fields.into(),
            super_home_atom,
            constructible,
            prototype_unobservable,
            class_initializer,
            parameter_eval_arguments_error,
            unaliased_arguments, rest_alias,
            arguments_slot,
            simple_parameters,
            strict,
            locals,
            local_atoms: local_atoms.into(),
            local_registers: local_registers.into(),
            environment_atoms: environment_atoms.into(),
            selective_capture_slots,
            declared_functions,
            declared_captures,
            inherited_with_scope,
            environment_clones,
            name_bindings: crate::bytecode::NameBindings::flat(name_bindings),
            lexical_atoms: lexical_atoms.into(),
            binding_sites: binding_sites.into(),
            source_positions,
            code,
            wide,
            registers,
            dispatch,
            decoded: Default::default(),
            plain_locals: if packed { std::cell::OnceCell::new() } else { std::cell::OnceCell::from(plain_locals) },
            caches: Default::default(),
            lazy_flags: Default::default(),
            call_plan: Default::default(),
            handlers: handlers.into(),
            register_roots,
            undefined_register,
            parameter_registers,
            initial_register,
        };
        if !packed {
            function.seed_derived_facts(derived_facts);
        }
        Ok(function)
}

/// `write_function_header`'s header.
fn read_function_header(input: &mut BinaryReader<'_>) -> Result<super::FunctionHeader, String> {
    let parent = input.option_u32()?;
    let name = input.option_atom()?;
    let inferred_name = input.option_atom()?;
    let source_span = if input.flag("source span")? {
        Some(super::SourceSpan { start: input.u32()?, end: input.u32()? })
    } else {
        None
    };
    let length = input.u16()?;
    let dispatch = input.dispatch_class()?;
    Ok(super::FunctionHeader {
        parent,
        name,
        inferred_name,
        source_span,
        length,
        dispatch,
        is_arrow: input.flag("arrow function")?,
        is_async: input.flag("async function")?,
        is_generator: input.flag("generator function")?,
        is_class_constructor: input.flag("class constructor")?,
        constructible: input.flag("constructible function")?,
        prototype_unobservable: input.flag("prototype observability")?,
        inherited_with_scope: input.flag("inherited with-scope")?,
    })
}

/// A deferred record of `records`' program (`decode_program`), decoded for function `index`;
/// `None` when the function's record was not deferred.
pub(super) fn decode_deferred_function(
    records: &super::EncodedRecords,
    index: usize,
) -> Option<Result<Function, String>> {
    let record = (*records.ranges.get(index)?)?;
    let start = record.start as usize;
    let bytes = records.bytes.as_ref().as_ref().get(start..start + record.length())?;
    if compact_checksum(bytes) != record.checksum {
        return Some(Err("deferred record checksum mismatch".into()));
    }
    let mut reader = BinaryReader { bytes, cursor: 0, atoms: records.atoms.clone(), packed: false, unlowered: record.unlowered() };
    Some(read_function(&mut reader, None).and_then(|function| reader.finish().map(|()| function)))
}

/// The bytes of function `index`'s deferred record, when it has one.
pub(super) fn deferred_record_bytes(records: &super::EncodedRecords, index: usize) -> Option<&[u8]> {
    let record = (*records.ranges.get(index)?)?;
    let start = record.start as usize;
    records.bytes.as_ref().as_ref().get(start..start + record.length())
}

/// A lowered instruction's fixed-width record: four `u16` fields and a `u32` immediate.
const INSTRUCTION_BYTES: usize = 4 * size_of::<u16>() + size_of::<u32>();

/// The bytes a function record takes on average, which `encode_program` reserves for the body
/// up front. An estimate: a larger record grows the buffer as it always did.
const ESTIMATED_RECORD_BYTES: usize = 256;

pub(super) struct BinaryWriter {
    pub(super) bytes: Vec<u8>,
    atoms: AtomEncoding,
    /// LEB128 integers, for records kept in memory (`pack_unlowered`), not in the cache.
    packed: bool,
    /// An unlowered record (`encode_program`): without the memos a lowered one carries, which
    /// its first read derives after lowering it.
    unlowered: bool,
    /// Whether function records keep their pc-to-position tables (`ImageRetention`); without
    /// them a record's table is written empty, which reads as no positions.
    stack_positions: bool,
}

/// The function a program runs first, whose record a decoder always decodes.
const ROOT_FUNCTION: usize = 0;

/// How an encoding names atoms: by the program's own ids, or by their positions in a compact
/// table of the atoms it uses, in first-use order (`encode_compact_program`).
enum AtomEncoding {
    Program,
    Compact(CompactAtoms),
    /// The program's own ids, which name its reader's atoms too: a snapshot's programs share
    /// their VM's atom table, so only its length is written (`encode_snapshot_program`).
    Shared,
}

#[derive(Default)]
struct CompactAtoms {
    positions: rustc_hash::FxHashMap<u32, u32>,
    order: Vec<u32>,
}

#[rustfmt::skip]
impl BinaryWriter {
    pub(super) fn new() -> Self { Self::with_atoms(AtomEncoding::Program) }
    fn with_atoms(atoms: AtomEncoding) -> Self { Self { bytes: Vec::new(), atoms, packed: false, unlowered: false, stack_positions: true } }
    fn leb(&mut self, mut value: u32) { while value >= 0x80 { self.bytes.push(value as u8 | 0x80); value >>= 7; } self.bytes.push(value as u8); }
    fn atom(&mut self, atom: u32) {
        let encoded = self.encoded_atom(atom);
        self.u32(encoded);
    }
    /// The id the encoding names `atom` by.
    fn encoded_atom(&mut self, atom: u32) -> u32 {
        match &mut self.atoms {
            AtomEncoding::Program | AtomEncoding::Shared => atom,
            AtomEncoding::Compact(compact) => *compact.positions.entry(atom).or_insert_with(|| {
                compact.order.push(atom);
                compact.order.len() as u32 - 1
            }),
        }
    }
    fn option_atom(&mut self, atom: Option<u32>) { match atom { Some(atom) => self.atom(atom), None => self.u32(u32::MAX) } }
    fn atom_pair(&mut self, value: (u32, u16)) { self.atom(value.0); self.u16(value.1); }
    fn optional_atom_pair(&mut self, value: Option<(u32, u16)>) { self.u8(value.is_some() as u8); if let Some(value) = value { self.atom_pair(value); } }
    /// An instruction, whose immediate is an atom when it names one.
    #[inline]
    fn instruction(&mut self, op: Op, a: u16, b: u16, c: u16, imm: u32) {
        if self.packed {
            return self.packed_instruction(op, a, b, c, imm);
        }
        let imm = if op.immediate_names_atom(b) { self.encoded_atom(imm) } else { imm };
        let mut record = [0u8; INSTRUCTION_BYTES];
        record[0..2].copy_from_slice(&(op as u16).to_le_bytes());
        record[2..4].copy_from_slice(&a.to_le_bytes());
        record[4..6].copy_from_slice(&b.to_le_bytes());
        record[6..8].copy_from_slice(&c.to_le_bytes());
        record[8..].copy_from_slice(&imm.to_le_bytes());
        self.bytes.extend_from_slice(&record);
    }
    /// An instruction's LEB128 record, for a packed writer (`pack_unlowered`).
    fn packed_instruction(&mut self, op: Op, a: u16, b: u16, c: u16, imm: u32) {
        let imm = if op.immediate_names_atom(b) { self.encoded_atom(imm) } else { imm };
        let mut record = [0u8; 2 * 3 + 3 * 3 + 5];
        let mut at = 0;
        for value in [op as u32, u32::from(a), u32::from(b), u32::from(c), imm] {
            let mut value = value;
            while value >= 0x80 { record[at] = value as u8 | 0x80; at += 1; value >>= 7; }
            record[at] = value as u8;
            at += 1;
        }
        self.bytes.extend_from_slice(&record[..at]);
    }
    pub(super) fn u8(&mut self, value: u8) { self.bytes.push(value); }
    fn bool(&mut self, value: bool) { self.u8(u8::from(value)); }
    /// A source position table as it is coded in memory, and its final entry: decoding copies
    /// the bytes instead of coding each entry again (`BinaryReader::source_positions`).
    fn source_positions(&mut self, table: &super::SourcePositions) {
        let empty = super::SourcePositions::default();
        let (bytes, last) = if self.stack_positions { table.encoded() } else { empty.encoded() };
        self.u32(bytes.len() as u32);
        self.bytes.extend_from_slice(bytes);
        match last {
            Some(entry) => { self.u8(1); self.u32(entry.pc); self.u32(entry.offset); }
            None => self.u8(0),
        }
    }
    pub(super) fn u16(&mut self, value: u16) { if self.packed { self.leb(u32::from(value)) } else { self.bytes.extend_from_slice(&value.to_le_bytes()) } }
    pub(super) fn u32(&mut self, value: u32) { if self.packed { self.leb(value) } else { self.bytes.extend_from_slice(&value.to_le_bytes()) } }
    pub(super) fn u64(&mut self, value: u64) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn string(&mut self, value: &str) { self.u32(value.len() as u32); self.bytes.extend_from_slice(value.as_bytes()); }
    pub(super) fn strings(&mut self, values: &AtomTable) { self.u32(values.len() as u32); for value in values.iter() { self.string(value); } }
    pub(super) fn u16s(&mut self, values: &[u16]) { self.u32(values.len() as u32); for value in values { self.u16(*value); } }
    pub(super) fn option_u32(&mut self, value: Option<u32>) { self.u32(value.unwrap_or(u32::MAX)); }
}

/// A field site's source word, read as its optional offset (`FieldSite::sources`): an offset
/// of `FieldSite::NO_SOURCE` names no source, so an encoding that stores one is invalid.
fn field_source_word(input: &mut BinaryReader<'_>) -> Result<u32, String> {
    match input.optional_source_offset()? {
        None => Ok(FieldSite::NO_SOURCE),
        Some(FieldSite::NO_SOURCE) => Err("invalid field site".into()),
        Some(offset) => Ok(offset),
    }
}

pub(super) struct BinaryReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
    /// A compact encoding's atom ids, by table position; none when ids are the program's.
    atoms: Option<std::rc::Rc<[u32]>>,
    /// LEB128 integers (`BinaryWriter::packed`).
    packed: bool,
    /// An unlowered record, without a lowered one's memos (`BinaryWriter::unlowered`).
    unlowered: bool,
}

#[rustfmt::skip]
impl<'a> BinaryReader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self { Self { bytes, cursor: 0, atoms: None, packed: false, unlowered: false } }
    /// A reader of `bytes`, a part of this reader's input, under the same atom ids.
    fn sub_reader<'b>(&self, bytes: &'b [u8]) -> BinaryReader<'b> { BinaryReader { bytes, cursor: 0, atoms: self.atoms.clone(), packed: self.packed, unlowered: self.unlowered } }
    /// A one-byte boolean, named by `what` in its error.
    fn flag(&mut self, what: &str) -> Result<bool, String> {
        match self.u8()? { 0 => Ok(false), 1 => Ok(true), _ => Err(format!("invalid residual {what} flag")) }
    }
    /// A function's dispatch class, written as its discriminant.
    fn dispatch_class(&mut self) -> Result<DispatchClass, String> {
        match self.u8()? {
            byte if byte == DispatchClass::General as u8 => Ok(DispatchClass::General),
            byte if byte == DispatchClass::Numeric as u8 => Ok(DispatchClass::Numeric),
            _ => Err("invalid residual dispatch class".into()),
        }
    }
    fn source_positions(&mut self) -> Result<super::SourcePositions, String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?.to_vec();
        let last = match self.u8()? {
            0 => None,
            1 => Some(SourceOffset { pc: self.u32()?, offset: self.u32()? }),
            _ => return Err("invalid residual source position table".into()),
        };
        Ok(super::SourcePositions::from_encoded(bytes, last))
    }
    fn atom(&mut self) -> Result<u32, String> {
        let encoded = self.u32()?;
        self.atom_id(encoded)
    }
    /// The reader's id for the atom `encoded` names (`BinaryWriter::atom`).
    fn atom_id(&self, encoded: u32) -> Result<u32, String> {
        match &self.atoms {
            None => Ok(encoded),
            Some(ids) => ids.get(encoded as usize).copied().ok_or_else(|| "residual atom outside its table".into()),
        }
    }
    /// One instruction's record (`BinaryWriter::instruction`), read whole: its opcode, fields
    /// and immediate, which is the reader's atom id when it names an atom. Reading the record
    /// field by field checked its bounds five times.
    fn instruction(&mut self) -> Result<(Op, u16, u16, u16, u32), String> {
        if self.packed {
            let op = Op::from_index(usize::from(self.u16()?)).ok_or_else(|| String::from("invalid residual opcode"))?;
            let (a, b, c) = (self.u16()?, self.u16()?, self.u16()?);
            let encoded = self.u32()?;
            let imm = if op.immediate_names_atom(b) { self.atom_id(encoded)? } else { encoded };
            return Ok((op, a, b, c, imm));
        }
        const FIELD: usize = size_of::<u16>();
        let record = self.array::<INSTRUCTION_BYTES>()?;
        let field = |index: usize| u16::from_le_bytes([record[index * FIELD], record[index * FIELD + 1]]);
        let op = Op::from_index(usize::from(field(0))).ok_or_else(|| String::from("invalid residual opcode"))?;
        let (a, b, c) = (field(1), field(2), field(3));
        let encoded = u32::from_le_bytes(record[4 * FIELD..].try_into().expect("a record ends in its immediate"));
        let imm = if op.immediate_names_atom(b) { self.atom_id(encoded)? } else { encoded };
        Ok((op, a, b, c, imm))
    }
    fn option_atom(&mut self) -> Result<Option<u32>, String> {
        let encoded = self.u32()?;
        if encoded == u32::MAX { return Ok(None); }
        match &self.atoms {
            None => Ok(Some(encoded)),
            Some(ids) => ids.get(encoded as usize).copied().map(Some).ok_or_else(|| "residual atom outside its table".into()),
        }
    }
    fn atom_pair(&mut self) -> Result<(u32, u16), String> { Ok((self.atom()?, self.u16()?)) }
    /// Bindings are kept sorted by atom (`NameBindings`, `BindingSite`); ids a compact
    /// encoding maps to its reader's VM keep no order, so they are sorted again.
    fn restore_atom_order(&self, bindings: &mut [EvalBinding]) {
        if self.atoms.is_some() { EvalBinding::sort_by_atom(bindings); }
    }
    fn optional_atom_pair(&mut self) -> Result<Option<(u32, u16)>, String> {
        match self.u8()? { 0 => Ok(None), 1 => Ok(Some(self.atom_pair()?)), _ => Err("invalid residual option".into()) }
    }
    /// The immediate of an instruction of `op` with field `b`, an atom when it names one.
    fn immediate(&mut self, op: Op, b: u16) -> Result<u32, String> {
        if op.immediate_names_atom(b) { self.atom() } else { self.u32() }
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.cursor.checked_add(len).ok_or("residual overflow")?;
        let value = self.bytes.get(self.cursor..end).ok_or("truncated residual")?;
        self.cursor = end;
        Ok(value)
    }
    pub(super) fn magic(&mut self, value: &[u8]) -> Result<(), String> {
        if self.take(value.len())? == value { Ok(()) } else { Err("invalid residual header".into()) }
    }
    /// The next `N` bytes, read without a call; a truncated residual reports through `take`.
    #[inline(always)]
    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        if let Some(bytes) = self.bytes.get(self.cursor..).and_then(<[u8]>::first_chunk::<N>) {
            self.cursor += N;
            return Ok(*bytes);
        }
        self.take(N).map(|bytes| bytes.try_into().expect("take returns N bytes"))
    }
    pub(super) fn u8(&mut self) -> Result<u8, String> { Ok(self.array::<1>()?[0]) }
    fn bool(&mut self) -> Result<bool, String> {
        match self.u8()? {
            value if value == u8::from(false) => Ok(false),
            value if value == u8::from(true) => Ok(true),
            _ => Err("invalid residual boolean".into()),
        }
    }
    pub(super) fn u16(&mut self) -> Result<u16, String> {
        if self.packed { return u16::try_from(self.leb()?).map_err(|_| String::from("invalid packed u16")); }
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub(super) fn u32(&mut self) -> Result<u32, String> { if self.packed { self.leb() } else { Ok(u32::from_le_bytes(self.array()?)) } }
    #[inline(always)]
    fn leb(&mut self) -> Result<u32, String> {
        if let Some(&byte) = self.bytes.get(self.cursor) && byte < 0x80 {
            self.cursor += 1;
            return Ok(u32::from(byte));
        }
        self.leb_long()
    }
    fn leb_long(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = self.u8()?;
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 { return Ok(value); }
        }
        Err("invalid packed integer".into())
    }
    pub(super) fn u64(&mut self) -> Result<u64, String> { Ok(u64::from_le_bytes(self.array()?)) }
    pub(super) fn string(&mut self) -> Result<String, String> {
        self.str_ref().map(str::to_owned)
    }
    /// The next string, borrowed from the encoding.
    pub(super) fn str_ref(&mut self) -> Result<&'a str, String> {
        let len = self.u32()? as usize;
        std::str::from_utf8(self.take(len)?).map_err(|_| "invalid residual string".into())
    }
    pub(super) fn strings(&mut self) -> Result<AtomTable, String> {
        let count = self.u32()? as usize;
        let mut text = String::new();
        let mut ends = Vec::with_capacity(count);
        for _ in 0..count {
            let len = self.u32()? as usize;
            let value = std::str::from_utf8(self.take(len)?).map_err(|_| "invalid residual string")?;
            text.push_str(value);
            ends.push(text.len() as u32);
        }
        Ok(AtomTable::new(text, ends))
    }
    pub(super) fn u16s(&mut self) -> Result<Vec<u16>, String> { self.list(|input| input.u16()) }
    pub(super) fn option_u32(&mut self) -> Result<Option<u32>, String> {
        let value = self.u32()?;
        Ok((value != u32::MAX).then_some(value))
    }
    pub(super) fn optional_source_offset(&mut self) -> Result<Option<u32>, String> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u32()?)),
            _ => Err("invalid residual source option".into()),
        }
    }
    /// A counted list, allocated at its exact length: collecting through `Result` would
    /// lose the count and leave each list with doubling slack. Every element takes at least
    /// a byte, which bounds the allocation a corrupt count can ask for.
    pub(super) fn list<T>(&mut self, mut read: impl FnMut(&mut Self) -> Result<T, String>) -> Result<Vec<T>, String> {
        let len = self.u32()? as usize;
        let mut values = Vec::with_capacity(len.min(self.bytes.len() - self.cursor));
        for _ in 0..len {
            values.push(read(self)?);
        }
        Ok(values)
    }
    pub(super) fn finish(self) -> Result<(), String> {
        if self.cursor == self.bytes.len() { Ok(()) } else { Err("trailing residual data".into()) }
    }
}

fn write_optional_string(out: &mut BinaryWriter, value: Option<&str>) {
    match value {
        Some(value) => {
            out.u8(OPTIONAL_STRING_SOME);
            out.string(value);
        }
        None => out.u8(OPTIONAL_STRING_NONE),
    }
}

fn read_optional_string(input: &mut BinaryReader<'_>) -> Result<Option<String>, String> {
    match input.u8()? {
        OPTIONAL_STRING_NONE => Ok(None),
        OPTIONAL_STRING_SOME => input.string().map(Some),
        _ => Err("invalid optional residual string".into()),
    }
}

fn write_module_requests(out: &mut BinaryWriter, requests: &[ModuleRequest]) {
    out.u32(requests.len() as u32);
    for request in requests {
        write_request_header(out, &request.source, request.phase, request.module_type.as_deref());
    }
}

/// A module request's header: its source, phase and module type, as a request and an import
/// binding both begin (`read_module_request` reads it back).
fn write_request_header(out: &mut BinaryWriter, source: &str, phase: ModuleRequestPhase, module_type: Option<&str>) {
    out.string(source);
    out.u8(phase.binary_tag());
    write_optional_string(out, module_type);
}

fn read_module_request(input: &mut BinaryReader<'_>) -> Result<ModuleRequest, String> {
    let source = input.string()?;
    let phase = ModuleRequestPhase::from_binary_tag(input.u8()?)
        .ok_or_else(|| "invalid residual module request phase".to_string())?;
    let module_type = read_optional_string(input)?;
    Ok(ModuleRequest { source, phase, module_type })
}

fn read_module_requests(input: &mut BinaryReader<'_>) -> Result<Vec<ModuleRequest>, String> {
    input.list(read_module_request)
}

/// The declared environment: none (0), or its declarations, enum constants, class fields, signatures,
/// locals, parameter members, parameter classes, parameter language sets and receiver layouts (1).
fn write_declared_types(out: &mut BinaryWriter, declared: Option<&super::DeclaredTypes>) {
    let Some(declared) = declared else {
        out.u8(0);
        return;
    };
    out.u8(1);
    out.u32(declared.declared.len() as u32);
    for entry in &declared.declared {
        out.u8(entry.kind as u8);
        out.string(entry.name.as_deref().unwrap_or(""));
        out.bool(entry.name.is_some());
        out.u32(entry.start);
        out.u32(entry.end);
    }
    out.u32(declared.enum_constants.len() as u32);
    for (enumeration, member, value) in &declared.enum_constants {
        out.string(enumeration);
        out.string(member);
        match value {
            super::EnumConstant::Number(number) => {
                out.u8(0);
                out.u64(number.to_bits());
            }
            super::EnumConstant::String(text) => {
                out.u8(1);
                out.string(text);
            }
        }
    }
    out.u32(declared.fields.len() as u32);
    for field in &declared.fields {
        out.u32(field.class_start);
        out.string(&field.name);
        out.u16(field.languages);
        out.u16(field.elements);
        out.u8(u8::from(field.is_static));
    }
    out.u32(declared.signatures.len() as u32);
    for signature in &declared.signatures {
        out.u32(signature.start);
        out.u16(signature.returns);
        out.u16(signature.rest);
        out.u16(signature.this);
    }
    out.u32(declared.locals.len() as u32);
    for local in &declared.locals {
        out.u32(local.function);
        out.string(&local.name);
        out.u16(local.languages);
        out.u16(local.elements);
    }
    out.u32(declared.members.len() as u32);
    for member in &declared.members {
        out.u32(member.function);
        out.u16(member.parameter);
        out.string(&member.name);
        out.u16(member.languages);
    }
    out.u32(declared.parameter_classes.len() as u32);
    for parameter in &declared.parameter_classes {
        out.u32(parameter.function);
        out.u16(parameter.parameter);
        out.u32(parameter.class);
    }
    out.u32(declared.parameters.len() as u32);
    for parameter in &declared.parameters {
        out.u32(parameter.function);
        out.u16(parameter.parameter);
        out.u16(parameter.languages);
    }
    write_declared_docs(out, &declared.docs);
    out.u32(declared.layouts.len() as u32);
    for layout in &declared.layouts {
        out.u32(layout.class_start);
        out.u32(layout.names.len() as u32);
        for name in &layout.names {
            out.string(name);
        }
    }
    out.u32(declared.local_members.len() as u32);
    for member in &declared.local_members {
        out.u32(member.function);
        out.string(&member.local);
        out.string(&member.member);
        out.u16(member.languages);
    }
    match &declared.type_names {
        None => out.u8(0),
        Some(names) => {
            out.u8(1);
            out.u32(names.len() as u32);
            for name in names.iter() {
                out.string(&name.name);
                out.u16(name.languages);
                out.bool(name.alias);
                out.bool(name.class.is_some());
                out.u32(name.class.unwrap_or(0));
                out.bool(name.members.is_some());
                if let Some(members) = &name.members {
                    out.u32(members.len() as u32);
                    for (member, languages) in members.iter() {
                        out.string(member);
                        out.u16(*languages);
                    }
                }
            }
        }
    }
}

fn read_declared_types(input: &mut BinaryReader<'_>) -> Result<Option<Box<super::DeclaredTypes>>, String> {
    if input.u8()? == 0 { return Ok(None); }
    let declared = input.list(|input| {
        let kind = super::DeclaredKind::from_binary_tag(input.u8()?).ok_or_else(|| "invalid declared kind".to_string())?;
        let name = input.string()?;
        let named = input.bool()?;
        Ok(super::Declared { kind, name: named.then(|| name.into_boxed_str()), start: input.u32()?, end: input.u32()? })
    })?;
    let enum_constants = input.list(|input| {
        let enumeration = input.string()?.into_boxed_str();
        let member = input.string()?.into_boxed_str();
        let value = match input.u8()? {
            0 => super::EnumConstant::Number(f64::from_bits(crate::Value::from_f64_raw(f64::from_bits(input.u64()?)).to_bits())),
            1 => super::EnumConstant::String(input.string()?.into_boxed_str()),
            _ => return Err("invalid enum constant tag".to_string()),
        };
        Ok((enumeration, member, value))
    })?;
    let fields = input.list(|input| {
        let class_start = input.u32()?;
        let name = input.string()?.into_boxed_str();
        let languages = input.u16()?;
        let elements = input.u16()?;
        let is_static = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid declared field static flag".to_string()),
        };
        Ok(super::declared_types::DeclaredField { class_start, name, languages, elements, is_static })
    })?;
    let signatures = input.list(|input| {
        Ok(super::DeclaredSignature {
            start: input.u32()?,
            returns: input.u16()?,
            rest: input.u16()?,
            this: input.u16()?,
        })
    })?;
    let locals = input.list(|input| {
        Ok(super::DeclaredLocal {
            function: input.u32()?,
            name: input.string()?.into_boxed_str(),
            languages: input.u16()?,
            elements: input.u16()?,
        })
    })?;
    let members = input.list(|input| {
        Ok(super::DeclaredMember {
            function: input.u32()?,
            parameter: input.u16()?,
            name: input.string()?.into_boxed_str(),
            languages: input.u16()?,
        })
    })?;
    let parameter_classes = input.list(|input| {
        Ok(super::DeclaredParameterClass { function: input.u32()?, parameter: input.u16()?, class: input.u32()? })
    })?;
    let parameters = input.list(|input| {
        Ok(super::DeclaredParameter { function: input.u32()?, parameter: input.u16()?, languages: input.u16()? })
    })?;
    let docs = read_declared_docs(input)?;
    let layouts = input.list(|input| {
        let class_start = input.u32()?;
        let names = input.list(|input| Ok(input.string()?.into_boxed_str()))?;
        Ok(super::declared_types::DeclaredLayout { class_start, names: names.into_boxed_slice() })
    })?;
    let local_members = input.list(|input| {
        Ok(super::DeclaredLocalMember {
            function: input.u32()?,
            local: input.string()?.into_boxed_str(),
            member: input.string()?.into_boxed_str(),
            languages: input.u16()?,
        })
    })?;
    let type_names = match input.u8()? {
        0 => None,
        1 => {
            let names = input.list(|input| {
                let name = input.string()?.into_boxed_str();
                let languages = input.u16()?;
                let alias = input.bool()?;
                let has_class = input.bool()?;
                let class = input.u32()?;
                let members = if input.bool()? {
                    Some(input.list(|input| Ok((input.string()?.into_boxed_str(), input.u16()?)))?.into_boxed_slice())
                } else {
                    None
                };
                Ok(super::SavedTypeName { name, languages, alias, class: has_class.then_some(class), members })
            })?;
            Some(names.into())
        }
        _ => return Err("invalid saved type names tag".to_string()),
    };
    Ok(Some(Box::new(super::DeclaredTypes {
        declared, enum_constants, fields, signatures, locals, members, local_members, parameter_classes, parameters, layouts, docs, type_names,
    })))
}

/// The JSDoc comments that type functions from outside their text (`DeclaredDoc`).
fn write_declared_docs(out: &mut BinaryWriter, docs: &[super::declared_types::DeclaredDoc]) {
    out.u32(docs.len() as u32);
    for doc in docs {
        out.u32(doc.function);
        out.u32(doc.start);
        out.u32(doc.end);
    }
}

fn read_declared_docs(input: &mut BinaryReader<'_>) -> Result<Vec<super::declared_types::DeclaredDoc>, String> {
    input.list(|input| Ok(super::declared_types::DeclaredDoc { function: input.u32()?, start: input.u32()?, end: input.u32()? }))
}

fn write_module_link_plan(out: &mut BinaryWriter, plan: Option<&ModuleLinkPlan>) {
    let Some(plan) = plan else {
        out.u8(MODULE_LINK_PLAN_NONE);
        return;
    };
    out.u8(MODULE_LINK_PLAN_SOME);
    out.u32(plan.locals.len() as u32);
    for (local, exported) in &plan.locals {
        out.string(local);
        out.string(exported);
    }
    out.u32(plan.hoisted_functions.len() as u32);
    for (binding, function) in &plan.hoisted_functions {
        out.string(binding);
        out.string(function);
    }
    out.u32(plan.reexports.len() as u32);
    for reexport in &plan.reexports {
        out.u8(reexport.kind().binary_tag());
        match reexport {
            ModuleReexport::Named { source, imported, exported } => {
                out.string(source);
                out.string(imported);
                out.string(exported);
            }
            ModuleReexport::Star { source } => out.string(source),
            ModuleReexport::Namespace { source, exported } => {
                out.string(source);
                out.string(exported);
            }
        }
    }
}

fn read_module_link_plan(input: &mut BinaryReader<'_>) -> Result<Option<ModuleLinkPlan>, String> {
    Ok(match input.u8()? {
        MODULE_LINK_PLAN_NONE => None,
        MODULE_LINK_PLAN_SOME => Some(ModuleLinkPlan {
            locals: input.list(|input| Ok((input.string()?, input.string()?)))?,
            hoisted_functions: input.list(|input| Ok((input.string()?, input.string()?)))?,
            reexports: input.list(|input| {
                let kind = ModuleReexportKind::from_binary_tag(input.u8()?)
                    .ok_or_else(|| "invalid residual module re-export kind".to_string())?;
                Ok(match kind {
                    ModuleReexportKind::Named => ModuleReexport::Named {
                        source: input.string()?,
                        imported: input.string()?,
                        exported: input.string()?,
                    },
                    ModuleReexportKind::Star => ModuleReexport::Star { source: input.string()? },
                    ModuleReexportKind::Namespace => ModuleReexport::Namespace {
                        source: input.string()?,
                        exported: input.string()?,
                    },
                })
            })?,
        }),
        _ => return Err("invalid residual module link plan tag".into()),
    })
}

/// A `StaticModuleAnalysis`'s tag: what its text says.
const ANALYSIS_UNBOUNDED: u8 = 0;
const ANALYSIS_EARLY_ERROR: u8 = 1;
const ANALYSIS_PLAN: u8 = 2;

/// A static module analysis as bytes, which the host's compile cache keeps for its text.
pub(crate) fn encode_static_module_analysis(analysis: &crate::compile::StaticModuleAnalysis) -> Vec<u8> {
    let mut out = BinaryWriter::new();
    match analysis {
        crate::compile::StaticModuleAnalysis { early_error: Some(message), .. } => {
            out.u8(ANALYSIS_EARLY_ERROR);
            out.string(message);
        }
        crate::compile::StaticModuleAnalysis { early_error: None, plan: Some(plan) } => {
            out.u8(ANALYSIS_PLAN);
            write_module_requests(&mut out, &plan.requests);
            write_module_link_plan(&mut out, plan.link_plan.as_ref());
            out.u8(u8::from(plan.has_top_level_await));
        }
        crate::compile::StaticModuleAnalysis { early_error: None, plan: None } => out.u8(ANALYSIS_UNBOUNDED),
    }
    out.bytes
}

pub(crate) fn decode_static_module_analysis(bytes: &[u8]) -> Result<crate::compile::StaticModuleAnalysis, String> {
    let mut input = BinaryReader::new(bytes);
    let analysis = match input.u8()? {
        ANALYSIS_UNBOUNDED => crate::compile::StaticModuleAnalysis { early_error: None, plan: None },
        ANALYSIS_EARLY_ERROR => crate::compile::StaticModuleAnalysis { early_error: Some(input.string()?), plan: None },
        ANALYSIS_PLAN => {
            let requests = read_module_requests(&mut input)?;
            let link_plan = read_module_link_plan(&mut input)?;
            let has_top_level_await = input.flag("top-level await")?;
            crate::compile::StaticModuleAnalysis {
                early_error: None,
                plan: Some(crate::compile::StaticModulePlan { link_plan, requests, has_top_level_await }),
            }
        }
        _ => return Err("invalid static module analysis tag".into()),
    };
    input.finish()?;
    Ok(analysis)
}
