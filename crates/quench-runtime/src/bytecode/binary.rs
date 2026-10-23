use super::{
    AtomTable, BindingSite, Constant, DispatchClass, EvalBinding, EvalBindingDeclaration,
    EvalBindingLocation, FieldBase, FieldSite, Function, FunctionSlot, Handler, InitialRegister, Instr,
    LexicalBindingKind, LocalRegister, MethodSite, ModuleImportBinding, ModuleImportName,
    ModuleImportNameKind, ModuleLinkPlan, ModuleReexport, ModuleReexportKind, ModuleRequest,
    LiteralStep, ModuleRequestPhase, ObjectSite, Op, RegExpLiteralSite, SourcePosition, Superinstruction,
    WideInstruction,
};

/// The encoded kinds of `Handler::slot`: none, a frame local, or a register.
const THROWN_VALUE_DISCARDED: u8 = 0;
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

pub(super) fn write_program(
    program: &super::ResidualProgram,
    path: &std::path::Path,
) -> Result<(), String> {
    let (bytes, _) = encode_program(program, AtomEncoding::Program);
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

/// A compact encoding of `program` for a compile cache, with the program's atom ids in the
/// order its table names them (`decode_compact_program` maps each name to its reader's id).
/// The source text is left out: whoever decodes it supplies the same text.
pub(super) fn encode_compact_program(program: &super::ResidualProgram) -> (Vec<u8>, Vec<u32>) {
    let (bytes, order) = encode_program(program, AtomEncoding::Compact(Default::default()));
    (sealed(bytes), order)
}

/// An entry program's cached form (`ResidualProgram::encode_trusted`): the lossless encoding
/// under the program's own atom ids, which seed the VM's, followed by its checksum.
pub(super) fn encode_trusted_program(program: &super::ResidualProgram) -> Vec<u8> {
    sealed(encode_program(program, AtomEncoding::Program).0)
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

/// A cached encoding ends with this checksum of the bytes before it, which a decoder checks
/// before trusting them as the compiler output they encode (`sealed_encoding_length`), and each
/// deferred record ends its body entry with this checksum of the record.
fn compact_checksum(bytes: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write(bytes);
    hasher.finish()
}

fn encode_program(program: &super::ResidualProgram, atoms: AtomEncoding) -> (Vec<u8>, Vec<u32>) {
    // A compact or shared encoding leaves the source out: its reader supplies the text.
    let compact = matches!(atoms, AtomEncoding::Compact(_) | AtomEncoding::Shared);
    let mut out = BinaryWriter::new();
    out.bytes.extend_from_slice(RESIDUAL_MAGIC);
    out.u64(super::ResidualProgram::RUNTIME_ABI_FINGERPRINT);
    out.u8(u8::from(program.specialized));
    out.u8(program.kind.binary_tag());
    out.string(&program.source_name);
    match &program.source {
        Some(_) if compact => out.u8(OPTIONAL_STRING_ELIDED),
        source => write_optional_string(&mut out, source.as_ref().map(|source| source.text.as_ref())),
    }
    if let Some(source) = &program.source {
        out.u32(source.base);
        out.u32(source.line_offset);
        out.u32(source.column_offset);
    }
    write_module_requests(&mut out, &program.module_requests);
    out.u32(program.module_imports.len() as u32);
    for import in &program.module_imports {
        out.string(&import.source);
        out.u8(import.phase.binary_tag());
        write_optional_string(&mut out, import.module_type.as_deref());
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
    let mut header = out;
    let mut out = BinaryWriter::with_atoms(atoms);
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
    program.compile_released();
    // Every record but the root's goes to the region after the body, which a decoder leaves
    // in place until the function's first read; the body names each by its place and checksum.
    let mut region = Vec::new();
    out.u32(program.functions.len() as u32);
    for (index, function) in program.functions.iter().enumerate() {
        let deferred = index != ROOT_FUNCTION;
        out.u8(u8::from(deferred));
        write_function_header(&mut out, function.header());
        let body = std::mem::take(&mut out.bytes);
        write_function(&mut out, function, &program.superinstructions);
        let record = std::mem::replace(&mut out.bytes, body);
        let length = u32::try_from(record.len()).expect("a function record fits u32 bytes");
        out.u32(length);
        if deferred {
            out.u32(u32::try_from(region.len()).expect("deferred records fit u32 bytes"));
            out.u64(compact_checksum(&record));
            region.extend_from_slice(&record);
        } else {
            out.bytes.extend_from_slice(&record);
        }
    }
    out.u16(program.cache_sites);
    out.u32(program.method_sites.len() as u32);
    for site in &program.method_sites {
        out.atom(site.atom);
        out.u16(site.cache);
        out.u32(site.argument_start);
        out.u16(site.argument_count);
        out.optional_atom_pair(site.receiver_path);
    }
    out.u16s(&program.method_arguments);
    out.u32(program.field_sites.len() as u32);
    for site in &program.field_sites {
        out.u16(site.base.0);
        out.atom_pair(site.first);
        out.optional_atom_pair(site.second);
        out.optional_atom_pair(site.sink);
        for source in site.sources {
            out.u8(source.is_some() as u8);
            if let Some((line, column)) = source { out.u32(line); out.u32(column); }
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
    }
    out.u32(program.literal_templates.len() as u32);
    for template in &program.literal_templates {
        out.u32(template.len() as u32);
        for step in template {
            let (tag, operand) = match *step {
                LiteralStep::Constant(index) => (LITERAL_STEP_CONSTANT, index),
                LiteralStep::Array(count) => (LITERAL_STEP_ARRAY, count),
                LiteralStep::Object(site) => (LITERAL_STEP_OBJECT, site),
            };
            out.u8(tag);
            out.u32(operand);
        }
    }
    out.u32(program.superinstructions.len() as u32);
    for site in &program.superinstructions {
        for instruction in site.code {
            out.instruction(instruction.op(), instruction.a(), instruction.b(), instruction.c(), instruction.imm());
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
    let length = bytes.len();
    let program = decode_program(&(std::rc::Rc::new(bytes) as super::KeptBytes), length, ReadAtoms::Program, None)?;
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

/// A snapshot's encoding of `program` (`crate::snapshot`): the program's own atom ids, which
/// its VM's restored atom table keeps, and no source text, which the snapshot writes apart.
pub(super) fn encode_snapshot_program(program: &super::ResidualProgram) -> Vec<u8> {
    sealed(encode_program(program, AtomEncoding::Shared).0)
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
        OPTIONAL_STRING_SOME => Some(super::SourceText::Shared(std::sync::Arc::from(input.string()?))),
        OPTIONAL_STRING_ELIDED => Some(elided.take().ok_or("residual source text is not supplied")?),
        _ => return Err("invalid optional residual string".into()),
    };
    let source = match text {
        Some(text) => Some(super::ProgramSource {
            text,
            base: input.u32()?,
            line_offset: input.u32()?,
            column_offset: input.u32()?,
            line_starts: Default::default(),
        }),
        None => None,
    };
    let module_requests = read_module_requests(&mut input)?;
    let module_imports = input.list(|input| {
        let source = input.string()?;
        let phase = ModuleRequestPhase::from_binary_tag(input.u8()?)
            .ok_or_else(|| "invalid residual module import phase".to_string())?;
        let module_type = read_optional_string(input)?;
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
        0 => Ok(Constant::Number(f64::from_bits(input.u64()?))),
        1 => Ok(Constant::String(input.string()?)),
        7 => Ok(Constant::StringUnits(input.u16s()?)),
        6 => Ok(Constant::BigInt(input
            .string()?
            .parse()
            .map_err(|_| "invalid residual BigInt constant")?)),
        2 => Ok(Constant::Boolean(true)),
        3 => Ok(Constant::Boolean(false)),
        4 => Ok(Constant::Null),
        5 => Ok(Constant::Undefined),
        _ => Err("invalid residual constant".into()),
    })?;
    let function_count = input.u32()? as usize;
    let mut functions = Vec::with_capacity(function_count);
    let mut ranges = Vec::with_capacity(function_count);
    for _ in 0..function_count {
        let encoded_deferred = input.flag("deferred record")?;
        let header = read_function_header(&mut input)?;
        let length = input.u32()?;
        if encoded_deferred {
            let offset = input.u32()? as usize;
            let checksum = input.u64()?;
            if offset.checked_add(length as usize).is_none_or(|end| end > region_length) {
                return Err("deferred record outside its region".into());
            }
            let start = u32::try_from(region + offset).map_err(|_| "residual records exceed u32")?;
            ranges.push(Some(super::DeferredRecord { start, length, checksum }));
            functions.push(FunctionSlot::released(header));
            continue;
        }
        let record = input.take(length as usize)?;
        let mut reader = input.sub_reader(record);
        let function = read_function(&mut reader)?;
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
            receiver_path: input.optional_atom_pair()?,
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
                input.optional_source_location()?,
                input.optional_source_location()?,
                input.optional_source_location()?,
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
    input.finish()?;
    let program = super::ResidualProgram {
        specialized,
        kind,
        module_requests,
        module_imports,
        module_link_plan,
        source_name,
        source,
        atoms,
        constants,
        functions,
        cache_sites,
        method_sites,
        method_arguments,
        field_sites,
        object_sites,
        regexp_literal_sites,
        literal_templates,
        superinstructions,
        replay: None,
        encoded_records,
        validated: Default::default(),
    };
    Ok(program)
}

/// A function's record, which `read_function` decodes, with the projection of its plain local
/// slots (`Function::plain_local_slots`) that the interpreter memoizes on first call: a
/// decoder sets the memo instead of running the analysis again in every process.
fn write_function(out: &mut BinaryWriter, function: &Function, superinstructions: &[super::Superinstruction]) {
        out.option_u32(function.parent);
        out.option_atom(function.name);
        out.option_atom(function.inferred_name);
        out.u8(u8::from(function.is_arrow));
        out.u16(function.self_binding_slot.unwrap_or(u16::MAX));
        out.u8(u8::from(function.source_span.is_some()));
        if let Some(span) = function.source_span {
            out.u32(span.start);
            out.u32(span.end);
        }
        out.u16(function.params);
        out.u16(function.length);
        out.u32(function.parameter_end_pc);
        out.u32(function.parameter_atoms.len() as u32);
        for atom in &function.parameter_atoms {
            out.atom(*atom);
        }
        out.u8(u8::from(function.rest));
        out.u8(u8::from(function.is_async));
        out.u8(u8::from(function.is_generator));
        out.u8(u8::from(function.is_class_constructor));
        out.u8(u8::from(function.derived_constructor));
        out.option_u32(function.instance_initializer);
        out.option_atom(function.super_home_atom);
        out.u8(u8::from(function.constructible));
        out.u8(u8::from(function.class_initializer));
        out.u8(u8::from(function.prototype_unobservable));
        out.u8(u8::from(function.parameter_eval_arguments_error));
        out.u8(u8::from(function.unaliased_arguments));
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
        let name_bindings = function.name_bindings.resolved();
        out.u32(name_bindings.len() as u32);
        for binding in &name_bindings {
            write_eval_binding(out, binding);
        }
        out.u32(function.environment_clones.len() as u32);
        for clone in &function.environment_clones {
            for slots in [&clone.fresh, &clone.reset] {
                out.u32(slots.len() as u32);
                for slot in slots {
                    out.u16(*slot);
                }
            }
        }
        match &function.selective_capture_slots {
            Some(slots) => {
                out.u8(1);
                out.u16s(slots);
            }
            None => out.u8(0),
        }
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
        out.u8(function.dispatch as u8);
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
        out.u32(function.source_positions.iter().count() as u32);
        for position in function.source_positions.iter() {
            out.u32(position.pc);
            out.u32(position.line);
            out.u32(position.column);
        }
        let plain = match function.plain_locals.get() {
            Some(plain) => plain.to_vec(),
            None => function.plain_local_slots(superinstructions),
        };
        for byte in plain.chunks(u8::BITS as usize) {
            out.u8(byte.iter().enumerate().fold(0, |bits, (bit, &plain)| bits | (u8::from(plain) << bit)));
        }
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
fn read_function(input: &mut BinaryReader<'_>) -> Result<Function, String> {
        let parent = input.option_u32()?;
        let name = input.option_atom()?;
        let inferred_name = input.option_atom()?;
        let is_arrow = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid arrow function flag".into()),
        };
        let self_binding_slot = match input.u16()? {
            u16::MAX => None,
            slot => Some(slot),
        };
        let source_span = match input.u8()? {
            0 => None,
            _ => Some(super::SourceSpan {
                start: input.u32()?,
                end: input.u32()?,
            }),
        };
        let params = input.u16()?;
        let length = input.u16()?;
        let parameter_end_pc = input.u32()?;
        let parameter_atoms = input.list(|input| input.atom())?;
        let rest = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid residual rest flag".into()),
        };
        let is_async = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid async function flag".into()),
        };
        let is_generator = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid generator function flag".into()),
        };
        let is_class_constructor = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid class constructor flag".into()),
        };
        let derived_constructor = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid derived constructor flag".into()),
        };
        let instance_initializer = input.option_u32()?;
        let super_home_atom = input.option_atom()?;
        let constructible = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid constructible function flag".into()),
        };
        let class_initializer = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid class field initializer flag".into()),
        };
        let prototype_unobservable = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid prototype observability flag".into()),
        };
        let parameter_eval_arguments_error = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid parameter eval flag".into()),
        };
        let unaliased_arguments = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid unaliased arguments flag".into()),
        };
        let simple_parameters = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid simple parameters flag".into()),
        };
        let strict = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid strict function flag".into()),
        };
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
        let environment_clones = input.list(|input| {
            Ok(crate::bytecode::EnvironmentClone {
                fresh: input.list(|input| input.u16())?,
                reset: input.list(|input| input.u16())?,
            })
        })?;
        let selective_capture_slots = match input.u8()? {
            0 => None,
            1 => Some(input.u16s()?),
            _ => return Err("invalid selective capture layout".into()),
        };
        let inherited_with_scope = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid inherited with-scope flag".into()),
        };
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
        let dispatch = match input.u8()? {
            0 => DispatchClass::General,
            1 => DispatchClass::Numeric,
            _ => return Err("invalid residual dispatch class".into()),
        };
        let register_roots = match input.u8()? {
            0 => super::RegisterRoots::new(false),
            1 => super::RegisterRoots::new(true),
            _ => return Err("invalid residual register root coverage".into()),
        };
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
        let source_positions = input
            .list(|input| {
                Ok(SourcePosition {
                    pc: input.u32()?,
                    line: input.u32()?,
                    column: input.u32()?,
                })
            })?
            .into_iter()
            .collect();
        let plain_locals: Box<[bool]> = {
            let mut plain = Vec::with_capacity(usize::from(locals));
            for _ in 0..usize::from(locals).div_ceil(u8::BITS as usize) {
                let bits = input.u8()?;
                plain.extend((0..u8::BITS).map(|bit| bits & (1 << bit) != 0));
            }
            plain.truncate(usize::from(locals));
            plain.into()
        };
        Ok(Function {
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
            rest,
            is_async,
            is_generator,
            is_class_constructor,
            derived_constructor,
            instance_initializer,
            super_home_atom,
            constructible,
            prototype_unobservable,
            class_initializer,
            parameter_eval_arguments_error,
            unaliased_arguments,
            arguments_slot,
            simple_parameters,
            strict,
            locals,
            local_atoms: local_atoms.into(),
            local_registers: local_registers.into(),
            environment_atoms: environment_atoms.into(),
            selective_capture_slots,
            inherited_with_scope,
            environment_clones: environment_clones.into(),
            name_bindings: crate::bytecode::NameBindings::flat(name_bindings),
            lexical_atoms: lexical_atoms.into(),
            global_declarations: super::GlobalDeclarations {
                lexical: global_lexical_atoms,
                var: global_var_atoms,
                function: global_function_atoms,
                annex_b_var: global_annex_b_var_atoms,
                immutable: global_immutable_atoms,
            }
            .boxed(),
            binding_sites: binding_sites.into(),
            source_positions,
            code,
            wide,
            registers,
            dispatch,
            decoded: Default::default(),
            plain_locals: std::cell::OnceCell::from(plain_locals),
            declared_atom_sets: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            js_tier_candidate: Default::default(),
            call_plan: Default::default(),
            handlers: handlers.into(),
            register_roots,
            undefined_register,
            parameter_registers,
            initial_register,
        })
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
    let dispatch = match input.u8()? {
        0 => DispatchClass::General,
        1 => DispatchClass::Numeric,
        _ => return Err("invalid residual dispatch class".into()),
    };
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
    let bytes = records.bytes.as_ref().as_ref().get(start..start + record.length as usize)?;
    if compact_checksum(bytes) != record.checksum {
        return Some(Err("deferred record checksum mismatch".into()));
    }
    let mut reader = BinaryReader { bytes, cursor: 0, atoms: records.atoms.clone() };
    Some(read_function(&mut reader).and_then(|function| reader.finish().map(|()| function)))
}

pub(super) struct BinaryWriter {
    pub(super) bytes: Vec<u8>,
    atoms: AtomEncoding,
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
    fn with_atoms(atoms: AtomEncoding) -> Self { Self { bytes: Vec::new(), atoms } }
    fn atom(&mut self, atom: u32) {
        let encoded = match &mut self.atoms {
            AtomEncoding::Program | AtomEncoding::Shared => atom,
            AtomEncoding::Compact(compact) => *compact.positions.entry(atom).or_insert_with(|| {
                compact.order.push(atom);
                compact.order.len() as u32 - 1
            }),
        };
        self.u32(encoded);
    }
    fn option_atom(&mut self, atom: Option<u32>) { match atom { Some(atom) => self.atom(atom), None => self.u32(u32::MAX) } }
    fn atom_pair(&mut self, value: (u32, u16)) { self.atom(value.0); self.u16(value.1); }
    fn optional_atom_pair(&mut self, value: Option<(u32, u16)>) { self.u8(value.is_some() as u8); if let Some(value) = value { self.atom_pair(value); } }
    /// An instruction, whose immediate is an atom when it names one.
    fn instruction(&mut self, op: Op, a: u16, b: u16, c: u16, imm: u32) {
        self.u16(op as u16); self.u16(a); self.u16(b); self.u16(c);
        if op.immediate_names_atom(b) { self.atom(imm) } else { self.u32(imm) }
    }
    pub(super) fn u8(&mut self, value: u8) { self.bytes.push(value); }
    pub(super) fn u16(&mut self, value: u16) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn u32(&mut self, value: u32) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn u64(&mut self, value: u64) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn string(&mut self, value: &str) { self.u32(value.len() as u32); self.bytes.extend_from_slice(value.as_bytes()); }
    pub(super) fn strings(&mut self, values: &AtomTable) { self.u32(values.len() as u32); for value in values.iter() { self.string(value); } }
    pub(super) fn u16s(&mut self, values: &[u16]) { self.u32(values.len() as u32); for value in values { self.u16(*value); } }
    pub(super) fn option_u32(&mut self, value: Option<u32>) { self.u32(value.unwrap_or(u32::MAX)); }
}

pub(super) struct BinaryReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
    /// A compact encoding's atom ids, by table position; none when ids are the program's.
    atoms: Option<std::rc::Rc<[u32]>>,
}

#[rustfmt::skip]
impl<'a> BinaryReader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self { Self { bytes, cursor: 0, atoms: None } }
    /// A reader of `bytes`, a part of this reader's input, under the same atom ids.
    fn sub_reader<'b>(&self, bytes: &'b [u8]) -> BinaryReader<'b> { BinaryReader { bytes, cursor: 0, atoms: self.atoms.clone() } }
    /// A one-byte boolean, named by `what` in its error.
    fn flag(&mut self, what: &str) -> Result<bool, String> {
        match self.u8()? { 0 => Ok(false), 1 => Ok(true), _ => Err(format!("invalid residual {what} flag")) }
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
        const FIELD: usize = size_of::<u16>();
        const RECORD: usize = 4 * FIELD + size_of::<u32>();
        let record = self.array::<RECORD>()?;
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
        if self.atoms.is_some() { bindings.sort_unstable_by_key(|binding| binding.atom); }
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
    pub(super) fn u16(&mut self) -> Result<u16, String> { Ok(u16::from_le_bytes(self.array()?)) }
    pub(super) fn u32(&mut self) -> Result<u32, String> { Ok(u32::from_le_bytes(self.array()?)) }
    pub(super) fn u64(&mut self) -> Result<u64, String> { Ok(u64::from_le_bytes(self.array()?)) }
    pub(super) fn string(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| "invalid residual string".into())
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
    pub(super) fn optional_source_location(&mut self) -> Result<Option<(u32, u32)>, String> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some((self.u32()?, self.u32()?))),
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
        out.string(&request.source);
        out.u8(request.phase.binary_tag());
        write_optional_string(out, request.module_type.as_deref());
    }
}

fn read_module_requests(input: &mut BinaryReader<'_>) -> Result<Vec<ModuleRequest>, String> {
    input.list(|input| {
        let source = input.string()?;
        let phase = ModuleRequestPhase::from_binary_tag(input.u8()?)
            .ok_or_else(|| "invalid residual module request phase".to_string())?;
        let module_type = read_optional_string(input)?;
        Ok(ModuleRequest { source, phase, module_type })
    })
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
