//! Derive the native image's dynamic interface from its ABI owners. Private Rust
//! implementation symbols remain available to the pre-publication link audit.

use object::{Object, ObjectSymbol};
use serde::Serialize;
use std::{collections::BTreeSet, fs, path::Path, process::Command};
use super::NativeImageInputs;

const C_MAIN_SYMBOL: &str = "main";
#[cfg(target_os = "macos")]
const MACHO_HEADER_SYMBOL: &str = "_mh_execute_header";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPolicy {
    RestrictedMachO,
    RequiredElf,
}

#[derive(Serialize)]
pub struct DynamicInterface {
    pub policy: ExportPolicy,
    pub required: BTreeSet<String>,
    pub linked: BTreeSet<String>,
}

pub(super) struct ExportPlan {
    policy: ExportPolicy,
    required: BTreeSet<String>,
}

fn required_exports(inputs: &NativeImageInputs) -> Result<BTreeSet<String>, String> {
    let mut names = inputs.host_exports.clone();
    names.extend(fs::read_to_string(inputs.required_exports()).map_err(|error| error.to_string())?.lines().map(str::to_owned));
    names.insert(C_MAIN_SYMBOL.into());
    #[cfg(target_os = "macos")]
    names.insert(MACHO_HEADER_SYMBOL.into());
    for name in &names {
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Err(format!("invalid native ABI symbol: {name:?}"));
        }
    }
    Ok(names)
}

pub(super) fn configure(command: &mut Command, inputs: &NativeImageInputs, executable: &Path) -> Result<ExportPlan, String> {
    let required = required_exports(inputs)?;
    let mut path = executable.as_os_str().to_owned(); path.push(".exports");
    #[cfg(target_os = "macos")]
    let (policy, option, contents) = (ExportPolicy::RestrictedMachO, "-exported_symbols_list",
        required.iter().map(|name| format!("_{name}\n")).collect::<String>());
    #[cfg(target_os = "linux")]
    let (policy, option, contents) = (ExportPolicy::RequiredElf, "--dynamic-list",
        format!("{{\n{}\n}};\n", required.iter().map(|name| format!("  {name};")).collect::<Vec<_>>().join("\n")));
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err("native export interfaces support Mach-O and ELF targets".into());
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        fs::write(&path, contents).map_err(|error| error.to_string())?;
        command.args(["-C", "link-arg=-Xlinker", "-C"]).arg(format!("link-arg={option}"))
            .args(["-C", "link-arg=-Xlinker", "-C"]).arg(format!("link-arg={}", Path::new(&path).display()));
        Ok(ExportPlan { policy, required })
    }
}

impl ExportPlan {
    pub(super) fn verify(self, executable: &Path) -> Result<DynamicInterface, String> {
        let bytes = fs::read(executable).map_err(|error| error.to_string())?;
        let linked = dynamic_exports(&bytes)?;
        let missing: Vec<_> = self.required.difference(&linked).collect();
        let extra: Vec<_> = linked.difference(&self.required).collect();
        if !missing.is_empty() || matches!(self.policy, ExportPolicy::RestrictedMachO) && !extra.is_empty() {
            return Err(format!("native dynamic interface mismatch: missing {missing:?}; extra {extra:?}"));
        }
        Ok(DynamicInterface { policy: self.policy, required: self.required, linked })
    }
}

fn macho_exports<'a, Mach: object::read::macho::MachHeader>(
    file: &object::read::macho::MachOFile<'a, Mach>,
) -> Result<BTreeSet<String>, String> {
    let endian = file.endian();
    let mut names = BTreeSet::new();
    let mut commands = file.macho_load_commands().map_err(|error| error.to_string())?;
    while let Some(command) = commands.next().map_err(|error| error.to_string())? {
        let info = if command.cmd() == object::macho::LC_DYLD_EXPORTS_TRIE {
            *command.data::<object::macho::LinkeditDataCommand<Mach::Endian>>().map_err(|error| error.to_string())?
        } else if let Some(info) = command.dyld_info().map_err(|error| error.to_string())? {
            // Adapt the older LC_DYLD_INFO range to object's export-trie reader.
            object::macho::LinkeditDataCommand {
                cmd: object::U32::new(endian, object::macho::LC_DYLD_EXPORTS_TRIE),
                cmdsize: object::U32::new(endian, size_of::<object::macho::LinkeditDataCommand<Mach::Endian>>() as u32),
                dataoff: info.export_off, datasize: info.export_size,
            }
        } else { continue; };
        if info.datasize.get(endian) == 0 { continue; }
        for export in info.exports_trie(endian, file.data()).map_err(|error| error.to_string())? {
            let export = export.map_err(|error| error.to_string())?;
            let name = std::str::from_utf8(export.name()).map_err(|error| error.to_string())?;
            names.insert(name.strip_prefix('_').ok_or("Mach-O C ABI export has no leading underscore")?.into());
        }
    }
    Ok(names)
}

pub(super) fn dynamic_exports(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let file = object::File::parse(bytes).map_err(|error| error.to_string())?;
    match &file {
        object::File::MachO32(file) => macho_exports(file),
        object::File::MachO64(file) => macho_exports(file),
        object::File::Elf32(_) | object::File::Elf64(_) => file.dynamic_symbols().filter(|symbol| symbol.is_definition())
            .map(|symbol| symbol.name().map(str::to_owned).map_err(|error| error.to_string())).collect(),
        _ => Err("native dynamic export audit requires Mach-O or ELF".into()),
    }
}

// The audit's test reads a Mach-O executable.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn dynamic_interface_rejects_missing_and_unexpected_exports() {
        let executable = std::env::current_exe().unwrap();
        let names = dynamic_exports(&fs::read(&executable).unwrap()).unwrap();
        assert!(names.len() > 1);
        let mut missing = names.clone();
        missing.insert("quench_missing_test_abi".into());
        let error = ExportPlan { policy: ExportPolicy::RestrictedMachO, required: missing }.verify(&executable).err().unwrap();
        assert!(error.contains("quench_missing_test_abi"), "{error}");
        let mut incomplete = names.clone();
        incomplete.pop_first();
        let error = ExportPlan { policy: ExportPolicy::RestrictedMachO, required: incomplete }.verify(&executable).err().unwrap();
        assert!(error.contains("extra"), "{error}");
        assert!(ExportPlan { policy: ExportPolicy::RestrictedMachO, required: names }.verify(&executable).is_ok());
    }
}
