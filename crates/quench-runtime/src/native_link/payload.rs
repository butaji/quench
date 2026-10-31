//! Remove offline symbol metadata only after the full unstripped link audit.
//! Compare actual runtime bytes, rather than relying on absent symbol names.

use object::{Object, ObjectSection, ObjectSegment};
use serde::Serialize;
use std::{fs, path::Path, process::Command};

const MACHO_LINKEDIT_SEGMENT: &[u8] = b"__LINKEDIT";

#[derive(Serialize)]
pub struct PayloadReduction {
    pub unstripped_bytes: u64,
    pub binary_bytes: u64,
    pub symbols_before: usize,
    pub symbols_after: usize,
    pub preserved_runtime_sections: usize,
    pub preserved_loader_records: usize,
}

pub fn remove_offline_symbols(executable: &Path) -> Result<PayloadReduction, String> {
    let before = fs::read(executable).map_err(|error| error.to_string())?;
    // Retain the exported interface for runtime-loaded code. Plain Mach-O
    // `strip` rewrites its export trie, even when code/data bytes stay intact.
    super::link::checked_output(Command::new("strip").args(["-S", "-x"]).arg(executable), "native symbol removal")?;
    let after = fs::read(executable).map_err(|error| error.to_string())?;
    verify_runtime_payload(&before, &after)
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeSection<'a> {
    segment: Option<Vec<u8>>,
    name: &'a [u8],
    address: u64,
    size: u64,
    align: u64,
    flags: object::SectionFlags,
    data: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeSegment {
    name: Option<Vec<u8>>,
    address: u64,
    size: Option<u64>,
    align: u64,
    flags: object::SegmentFlags,
}

#[derive(Debug, PartialEq, Eq)]
enum LoaderRecord {
    Rebase, Bind, WeakBind, LazyBind, Export, FunctionStarts, DataInCode, ChainedFixups,
}

type LoaderPayload<'a> = Vec<(LoaderRecord, &'a [u8])>;

fn runtime_sections<'a>(file: &'a object::File<'a>) -> Result<Vec<RuntimeSection<'a>>, String> {
    file.sections().filter_map(|section| {
        let included = match section.flags() {
            object::SectionFlags::Elf { sh_flags } => sh_flags & u64::from(object::elf::SHF_ALLOC) != 0,
            object::SectionFlags::MachO { flags } => flags & object::macho::S_ATTR_DEBUG == 0,
            _ => return Some(Err("native payload verification requires ELF or Mach-O sections".into())),
        };
        included.then(|| Ok(RuntimeSection {
            segment: section.segment_name_bytes().map_err(|error| error.to_string())?.map(<[u8]>::to_vec),
            name: section.name_bytes().map_err(|error| error.to_string())?,
            address: section.address(), size: section.size(), align: section.align(), flags: section.flags(),
            data: section.data().map_err(|error| error.to_string())?,
        }))
    }).collect()
}

fn runtime_segments(file: &object::File<'_>) -> Result<Vec<RuntimeSegment>, String> {
    file.segments().map(|segment| {
        let name = segment.name_bytes().map_err(|error| error.to_string())?;
        let linkedit = name == Some(MACHO_LINKEDIT_SEGMENT);
        let permissions = segment.permissions();
        if linkedit && (permissions.executable() || permissions.writable()) {
            return Err("native link-edit metadata segment is executable or writable".into());
        }
        Ok(RuntimeSegment { name: name.map(<[u8]>::to_vec), address: segment.address(), size: (!linkedit).then(|| segment.size()),
            align: segment.align(), flags: segment.flags() })
    }).collect()
}

fn macho_loader_payload<'a, Mach: object::read::macho::MachHeader>(
    file: &object::read::macho::MachOFile<'a, Mach>,
) -> Result<LoaderPayload<'a>, String> {
    let endian = file.endian();
    let bytes = file.data();
    let range = |offset: u32, size: u32| -> Result<&'a [u8], String> {
        let start = usize::try_from(offset).map_err(|error| error.to_string())?;
        let end = start.checked_add(usize::try_from(size).map_err(|error| error.to_string())?)
            .ok_or("native loader payload range overflows")?;
        bytes.get(start..end).ok_or_else(|| "native loader payload is outside its image".into())
    };
    let mut payload = Vec::new();
    let mut commands = file.macho_load_commands().map_err(|error| error.to_string())?;
    while let Some(command) = commands.next().map_err(|error| error.to_string())? {
        if let Some(info) = command.dyld_info().map_err(|error| error.to_string())? {
            for (kind, offset, size) in [
                (LoaderRecord::Rebase, info.rebase_off.get(endian), info.rebase_size.get(endian)),
                (LoaderRecord::Bind, info.bind_off.get(endian), info.bind_size.get(endian)),
                (LoaderRecord::WeakBind, info.weak_bind_off.get(endian), info.weak_bind_size.get(endian)),
                (LoaderRecord::LazyBind, info.lazy_bind_off.get(endian), info.lazy_bind_size.get(endian)),
                (LoaderRecord::Export, info.export_off.get(endian), info.export_size.get(endian)),
            ] { payload.push((kind, range(offset, size)?)); }
        }
        let kind = match command.cmd() {
            object::macho::LC_DYLD_EXPORTS_TRIE => LoaderRecord::Export,
            object::macho::LC_DYLD_CHAINED_FIXUPS => LoaderRecord::ChainedFixups,
            object::macho::LC_FUNCTION_STARTS => LoaderRecord::FunctionStarts,
            object::macho::LC_DATA_IN_CODE => LoaderRecord::DataInCode,
            _ => continue,
        };
        let info: &object::macho::LinkeditDataCommand<Mach::Endian> = command.data().map_err(|error| error.to_string())?;
        payload.push((kind, range(info.dataoff.get(endian), info.datasize.get(endian))?));
    }
    Ok(payload)
}

fn loader_payload<'a>(file: &object::File<'a>) -> Result<LoaderPayload<'a>, String> {
    match file {
        object::File::MachO32(file) => macho_loader_payload(file),
        object::File::MachO64(file) => macho_loader_payload(file),
        // ELF .dynamic/.dynsym/.dynstr and relocation tables have SHF_ALLOC and
        // are already compared byte for byte by runtime_sections.
        object::File::Elf32(_) | object::File::Elf64(_) => Ok(Vec::new()),
        _ => Err("native symbol removal supports ELF and Mach-O images".into()),
    }
}

fn verify_runtime_payload(before: &[u8], after: &[u8]) -> Result<PayloadReduction, String> {
    let original = object::File::parse(before).map_err(|error| error.to_string())?;
    let stripped = object::File::parse(after).map_err(|error| error.to_string())?;
    if (original.format(), original.architecture(), original.is_little_endian(), original.entry(), original.flags())
        != (stripped.format(), stripped.architecture(), stripped.is_little_endian(), stripped.entry(), stripped.flags()) {
        return Err("symbol removal changed the native image identity or entry".into());
    }
    let sections = runtime_sections(&original)?;
    let loader = loader_payload(&original)?;
    let after_sections = runtime_sections(&stripped)?;
    if sections.len() != after_sections.len() {
        return Err("symbol removal changed native runtime code, data or loader records: section inventory".into());
    }
    for (before, after) in sections.iter().zip(&after_sections) {
        if before != after {
            return Err(format!("symbol removal changed native runtime code, data or loader records: section {} (layout {}, bytes {})",
                String::from_utf8_lossy(before.name),
                (before.address, before.size, before.align, before.flags) == (after.address, after.size, after.align, after.flags),
                before.data == after.data));
        }
    }
    if runtime_segments(&original)? != runtime_segments(&stripped)? {
        return Err("symbol removal changed native runtime code, data or loader records: segment mappings".into());
    }
    let after_loader = loader_payload(&stripped)?;
    if loader.len() != after_loader.len() {
        return Err(format!("symbol removal changed native runtime code, data or loader records: loader inventory {:?} -> {:?}",
            loader.iter().map(|(kind, bytes)| (kind, bytes.len())).collect::<Vec<_>>(),
            after_loader.iter().map(|(kind, bytes)| (kind, bytes.len())).collect::<Vec<_>>()));
    }
    for (before, after) in loader.iter().zip(&after_loader) {
        if before != after {
            return Err(format!("symbol removal changed native runtime code, data or loader records: {:?} ({} -> {} bytes)", before.0, before.1.len(), after.1.len()));
        }
    }
    let imports = |file: &object::File<'_>| -> Result<Vec<_>, String> {
        let mut imports: Vec<_> = file.imports().map_err(|error| error.to_string())?.into_iter()
            .map(|import| (import.name().to_vec(), import.library().to_vec())).collect();
        imports.sort(); Ok(imports)
    };
    if imports(&original)? != imports(&stripped)? {
        return Err("symbol removal changed native dynamic imports".into());
    }
    let symbols_before = original.symbols().count();
    let symbols_after = stripped.symbols().count();
    if after.len() > before.len() || symbols_after > symbols_before {
        return Err(format!("symbol removal increased native image metadata ({} -> {} bytes, {symbols_before} -> {symbols_after} symbols)", before.len(), after.len()));
    }
    Ok(PayloadReduction { unstripped_bytes: before.len() as u64, binary_bytes: after.len() as u64,
        symbols_before, symbols_after, preserved_runtime_sections: sections.len(), preserved_loader_records: loader.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_payload_verification_rejects_changed_code() {
        let before = fs::read(std::env::current_exe().unwrap()).unwrap();
        let file = object::File::parse(before.as_slice()).unwrap();
        let (offset, size) = file.sections().find(|section| section.kind() == object::SectionKind::Text && section.size() != 0)
            .unwrap().file_range().unwrap();
        assert_ne!(size, 0);
        let mut changed = before.clone();
        changed[usize::try_from(offset).unwrap()] ^= 1;
        let error = verify_runtime_payload(&before, &changed).err().unwrap();
        assert!(error.contains("runtime code, data or loader records"), "{error}");
    }

    #[test]
    fn runtime_payload_verification_rejects_changed_loader_records() {
        let before = fs::read(std::env::current_exe().unwrap()).unwrap();
        let file = object::File::parse(before.as_slice()).unwrap();
        let payload = loader_payload(&file).unwrap();
        let data = payload.iter().find(|(_, bytes)| !bytes.is_empty()).map(|(_, bytes)| *bytes)
            .or_else(|| file.sections().find(|section| section.name_bytes().is_ok_and(|name| name == b".dynamic"))
                .and_then(|section| section.data().ok()));
        let Some(data) = data else {
            assert!(file.imports().unwrap().is_empty(), "a dynamically linked fixture must have loader records");
            return;
        };
        let offset = data.as_ptr() as usize - before.as_ptr() as usize;
        let mut changed = before.clone();
        changed[offset] ^= 1;
        let error = verify_runtime_payload(&before, &changed).err().unwrap();
        assert!(error.contains("runtime code, data or loader records"), "{error}");
    }

    #[test]
    fn stripping_preserves_runtime_payload_and_execution() {
        let directory = std::env::temp_dir().join(format!("quench symbol removal, {}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let source = directory.join("program.rs");
        fs::write(&source, r#"fn main() { println!("payload: {:?}", std::env::args().skip(1).collect::<Vec<_>>()); }"#).unwrap();
        let original = directory.join("original");
        let compiled = Command::new("rustc").arg(&source).args(["-C", "strip=none", "-C", "debuginfo=1", "-o"]).arg(&original).output().unwrap();
        assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
        let stripped = directory.join("program");
        fs::copy(&original, &stripped).unwrap();
        let reduction = remove_offline_symbols(&stripped).unwrap();
        assert!(reduction.binary_bytes < reduction.unstripped_bytes);
        assert!(reduction.symbols_after < reduction.symbols_before);
        assert!(reduction.preserved_runtime_sections > 0);
        let oracle = Command::new(original).args(["λ", "with spaces"]).output().unwrap();
        let actual = Command::new(&stripped).args(["λ", "with spaces"]).output().unwrap();
        assert!(actual.status.success(), "{}", String::from_utf8_lossy(&actual.stderr));
        assert_eq!(actual.status, oracle.status);
        assert_eq!(actual.stdout, oracle.stdout);
        assert_eq!(actual.stderr, oracle.stderr);
        fs::remove_dir_all(directory).unwrap();
    }
}
