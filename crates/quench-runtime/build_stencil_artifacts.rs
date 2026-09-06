use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use object::endian::Endianness;
use object::read::{Object, ObjectSection, ObjectSymbol};
use object::SymbolSection;
use object::{BinaryFormat, RelocationEncoding, RelocationKind, RelocationTarget, SectionKind};

use super::RegionDeclaration;

const HEADER: &str = "/// Rust object artifacts generated at build time.\n";

pub(crate) fn generate(out_dir: &Path, declarations: &[RegionDeclaration]) {
    let target = env::var("TARGET").unwrap_or_default();
    let generation_enabled =
        env::var_os("QUENCH_GENERATE_STENCIL_OBJECTS").is_some() && supports_target(&target);
    if generation_enabled {
        println!("cargo:rustc-cfg=quench_generated_stencil_artifacts");
    }
    let generated = if generation_enabled {
        extract_objects(declarations)
    } else {
        empty_artifacts()
    };
    fs::write(out_dir.join("stencil_artifacts.rs"), generated)
        .expect("write generated stencil artifacts");
}

fn supports_target(target: &str) -> bool {
    target.starts_with("aarch64") || target.starts_with("x86_64")
}

struct OwnedDirectory {
    path: PathBuf,
}

struct ExtractedObject {
    bytes: Vec<u8>,
    fallthrough: Option<Vec<u8>>,
    relocations: Vec<ExtractedRelocation>,
}

#[derive(Clone, Copy)]
struct ExtractedRelocation {
    offset: u16,
    kind: &'static str,
    target: &'static str,
    addend: i64,
}

#[derive(Clone, Copy)]
struct ExpectedRelocation {
    section: SectionKind,
    offset: u16,
    width: usize,
    kind: &'static str,
    target: &'static str,
    addend: i64,
}

fn expected_relocation_index(
    expected: &[ExpectedRelocation],
    consumed: &[bool],
    section: SectionKind,
    offset: u64,
    kind: &str,
    target: &str,
) -> Option<usize> {
    expected.iter().enumerate().find_map(|(index, item)| {
        (!consumed[index]
            && item.section == section
            && u64::from(item.offset) == offset
            && item.kind == kind
            && item.target == target)
            .then_some(index)
    })
}

struct ParsedFragment {
    bytes: Vec<u8>,
    relocations: Vec<ExtractedRelocation>,
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        let Ok(entries) = fs::read_dir(&self.path) else {
            return;
        };
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
        let _ = fs::remove_dir(&self.path);
    }
}

pub(crate) fn verify_words(path: &Path, expected: &[u32]) {
    let data = fs::read(path).expect("read Rust assembly object");
    let file = object::File::parse(&*data).expect("parse Rust assembly object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_target_format(&file, env::var("TARGET").ok().as_deref());
    reject_unwind_or_tls_sections(&file);
    validate_relocations(&file, "assembly verifier");
    let mut sections = file.sections().filter(|section| {
        section
            .name()
            .ok()
            .is_some_and(|name| name == ".text" || name == "__text")
    });
    let section = sections.next().expect("Rust assembly text section");
    assert!(
        sections.next().is_none(),
        "assembly verifier found multiple text sections"
    );
    let bytes = section
        .uncompressed_data()
        .expect("read Rust assembly text");
    assert!(
        !bytes.is_empty() && bytes.len() % 4 == 0,
        "assembly verifier found invalid instruction bounds"
    );
    for word in expected {
        let needle = word.to_le_bytes();
        assert!(
            bytes.chunks_exact(4).any(|chunk| chunk == needle),
            "assembly word {word:08x} missing from extracted text"
        );
    }
}

pub(crate) fn verify_symbols(path: &Path, names: &[&str]) {
    let data = fs::read(path).expect("read Rust assembly object");
    let file = object::File::parse(&*data).expect("parse Rust assembly object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_target_format(&file, env::var("TARGET").ok().as_deref());
    assert_single_text_section(&file);
    reject_unwind_or_tls_sections(&file);
    validate_relocations(&file, "assembly verifier");
    let mut sections = file.sections().filter(|section| {
        section
            .name()
            .ok()
            .is_some_and(|name| name == ".text" || name == "__text")
    });
    let section = sections.next().expect("Rust assembly text section");
    assert!(
        sections.next().is_none(),
        "assembly verifier found multiple text sections"
    );
    let section_index = section.index();
    let mut previous = 0;
    for name in names {
        let symbols = file
            .symbols()
            .filter(|symbol| symbol.section_index() == Some(section_index))
            .filter(|symbol| {
                symbol
                    .name()
                    .ok()
                    .is_some_and(|value| value.trim_start_matches('_') == *name)
            })
            .collect::<Vec<_>>();
        assert_eq!(symbols.len(), 1, "assembly symbol {name} must be unique");
        let offset = symbols[0]
            .address()
            .checked_sub(section.address())
            .expect("assembly symbol precedes text") as usize;
        assert!(offset >= previous, "assembly symbols are out of order");
        assert!(
            offset < section.size() as usize,
            "assembly symbol is outside text"
        );
        previous = offset;
    }
}

fn empty_artifacts() -> String {
    format!(
        "{HEADER}{}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[];\n",
        artifact_schema()
    )
}

fn artifact_schema() -> &'static str {
    "#[derive(Clone, Copy, Debug)] pub struct BuildStencilArtifact { pub name: &'static str, pub artifact_id: &'static str, pub key: crate::stencil_fact::RegionKey, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub abi: crate::stencil_select::RegionAbi, pub entry: u16, pub external_entries: &'static [u16], pub has_fallthrough: bool, pub executable: bool, pub template_calls_helper: bool, pub bytes: &'static [u8], pub data: &'static [u8], pub relocations: &'static [crate::stencil_select::PhysicalRelocation], pub stencil: crate::stencil_fact::Stencil, pub fallthrough: Option<crate::stencil_fact::Stencil>, pub fallthrough_entry: u16 }"
}

fn extract_objects(declarations: &[RegionDeclaration]) -> String {
    let target = env::var("TARGET").expect("TARGET for stencil object generation");
    let compiler = rustc_path();
    let flags = [
        "--crate-type=lib",
        "--emit=obj",
        "-Copt-level=2",
        "-Cpanic=abort",
        "-Coverflow-checks=off",
        "--edition=2021",
    ];
    let rustflags = effective_rustflags();
    let build_fingerprint = fingerprint(&target, &compiler, &flags, declarations);
    let root = unique_directory();
    let mut constants = Vec::new();
    let mut rows = Vec::new();
    for declaration in declarations {
        // A generated whole-function recipe owns its arguments directly, so
        // it does not need the canonical byte-template holes (AddConst is the
        // first example). Unsupported hole-bearing recipes remain skipped
        // until a declared relocation plan exists.
        let extracted = if declaration.name == "fallthrough" {
            if !target.starts_with("aarch64") {
                continue;
            }
            compile_fragment_pair(
                &root.path,
                &target,
                &compiler,
                &flags,
                &rustflags,
                declaration,
            )
        } else if declaration.name == "array_numeric_loop" {
            if !target.starts_with("aarch64") {
                continue;
            }
            ExtractedObject {
                bytes: compile_assembly_fragment(
                    &root.path,
                    &target,
                    &compiler,
                    &flags,
                    &rustflags,
                    "array_numeric_loop",
                    super::build_stencil_templates::aarch64_array_loop(),
                    &[],
                )
                .bytes,
                fallthrough: None,
                relocations: Vec::new(),
            }
        } else if declaration.name == "prototype_property" {
            if !target.starts_with("aarch64") {
                continue;
            }
            ExtractedObject {
                bytes: compile_assembly_fragment(
                    &root.path,
                    &target,
                    &compiler,
                    &flags,
                    &rustflags,
                    "prototype_property",
                    super::build_stencil_templates::aarch64_prototype_property(),
                    &[],
                )
                .bytes,
                fallthrough: None,
                relocations: Vec::new(),
            }
        } else {
            let Some(recipe) = super::rust_leaf_recipe(declaration) else {
                continue;
            };
            ExtractedObject {
                bytes: compile_one(
                    &root.path,
                    &target,
                    &compiler,
                    &flags,
                    &rustflags,
                    declaration,
                    recipe,
                ),
                fallthrough: None,
                relocations: Vec::new(),
            }
        };
        let fingerprint =
            artifact_fingerprint(declaration, &target, &build_fingerprint, &extracted);
        let (constant, row) =
            render_artifact(declaration, &target, &compiler, &fingerprint, &extracted);
        constants.push(constant);
        rows.push(row);
    }
    assert!(!rows.is_empty(), "no extractable Rust stencil declarations");
    let generated = format!(
        "{HEADER}{}\n{}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[\n{}\n];\n",
        artifact_schema(),
        constants.join("\n"),
        rows.join("\n")
    );
    generated
}

fn rust_source(name: &str, recipe: super::RustLeafRecipe) -> String {
    assert_valid_symbol_name(name);
    format!(
        "#![no_std]\n#[no_mangle]\n#[inline(never)]\npub extern \"C\" fn q_{}({}) -> f64 {{ {} }}\n",
        name,
        recipe.parameters(),
        recipe.expression()
    )
}

fn assert_valid_symbol_name(name: &str) {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    assert!(valid_start, "invalid Rust stencil symbol name {name:?}");
    assert!(
        chars.all(|character| character == '_' || character.is_ascii_alphanumeric()),
        "invalid Rust stencil symbol name {name:?}"
    );
}

fn compile_one(
    root: &Path,
    target: &str,
    compiler: &str,
    flags: &[&str],
    rustflags: &[String],
    declaration: &RegionDeclaration,
    recipe: super::RustLeafRecipe,
) -> Vec<u8> {
    let source = root.join(format!("{}.rs", declaration.name));
    let object = root.join(format!("{}.o", declaration.name));
    fs::write(&source, rust_source(declaration.name, recipe)).expect("write Rust stencil source");
    let mut command = Command::new(compiler);
    command
        .args(["--target", target])
        .args(flags)
        .args(rustflags)
        .args([source.to_str().unwrap(), "-o", object.to_str().unwrap()]);
    run(&mut command, "compile Rust stencil object");
    parse_object(&object, &format!("q_{}", declaration.name))
}

fn compile_fragment_pair(
    root: &Path,
    target: &str,
    compiler: &str,
    flags: &[&str],
    rustflags: &[String],
    declaration: &RegionDeclaration,
) -> ExtractedObject {
    if !target.starts_with("aarch64") {
        return ExtractedObject {
            bytes: Vec::new(),
            fallthrough: None,
            relocations: Vec::new(),
        };
    }
    let expected = declaration
        .aarch64_holes
        .iter()
        .map(|(offset, width, kind)| ExpectedRelocation {
            section: SectionKind::Text,
            offset: *offset,
            width: *width,
            kind,
            target: "q_fallthrough_tail",
            addend: 0,
        })
        .collect::<Vec<_>>();
    let head = compile_assembly_fragment(
        root,
        target,
        compiler,
        flags,
        rustflags,
        "fallthrough_head",
        super::build_stencil_templates::aarch64_head(),
        &expected,
    );
    let tail = compile_assembly_fragment(
        root,
        target,
        compiler,
        flags,
        rustflags,
        "fallthrough_tail",
        super::build_stencil_templates::aarch64_tail(),
        &[],
    );
    ExtractedObject {
        bytes: head.bytes,
        fallthrough: Some(tail.bytes),
        relocations: head.relocations,
    }
}

fn compile_assembly_fragment(
    root: &Path,
    target: &str,
    compiler: &str,
    flags: &[&str],
    rustflags: &[String],
    name: &str,
    source_text: &str,
    expected_relocations: &[ExpectedRelocation],
) -> ParsedFragment {
    let source = root.join(format!("{name}.rs"));
    let object = root.join(format!("{name}.o"));
    fs::write(&source, source_text).expect("write Rust assembly fragment");
    let mut command = Command::new(compiler);
    command
        .args(["--target", target])
        .args(flags)
        .args(rustflags)
        .args([source.to_str().unwrap(), "-o", object.to_str().unwrap()]);
    run(&mut command, "compile Rust assembly fragment");
    parse_object_range(
        &object,
        &format!("q_{name}"),
        &format!("q_{name}_end"),
        expected_relocations,
    )
}

fn parse_object(path: &Path, name: &str) -> Vec<u8> {
    let data = fs::read(path).expect("read Rust stencil object");
    let file = object::File::parse(&*data).expect("parse Rust stencil object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_target_format(&file, env::var("TARGET").ok().as_deref());
    assert_single_text_section(&file);
    reject_unwind_or_tls_sections(&file);
    validate_relocations(&file, "Rust stencil");
    for symbol in file.symbols() {
        if matches!(symbol.section(), SymbolSection::Undefined) {
            panic!(
                "Rust stencil has undeclared external symbol {:?}",
                symbol.name()
            );
        }
    }
    let symbol = file
        .symbols()
        .find(|symbol| {
            symbol
                .name()
                .ok()
                .is_some_and(|value| value.trim_start_matches('_') == name)
        })
        .expect("Rust stencil entry symbol must exist");
    let section_index = symbol.section_index().expect("stencil symbol section");
    let section = file
        .sections()
        .find(|section| section.index() == section_index)
        .expect("Rust stencil symbol section must exist");
    assert_eq!(
        section.kind(),
        SectionKind::Text,
        "Rust stencil entry must point into executable text"
    );
    let bytes = section.uncompressed_data().expect("read Rust stencil text");
    assert_eq!(
        section.size() as usize,
        bytes.len(),
        "Rust stencil text size is ambiguous"
    );
    if matches!(file.architecture(), object::Architecture::Aarch64) {
        assert_eq!(section.align() % 4, 0, "AArch64 text alignment is invalid");
    }
    let symbols = file
        .symbols()
        .filter(|symbol| symbol.section_index() == Some(section_index))
        .filter(|symbol| {
            symbol
                .name()
                .ok()
                .is_some_and(|value| value.trim_start_matches('_') == name)
        })
        .collect::<Vec<_>>();
    assert_eq!(symbols.len(), 1, "Rust stencil entry symbol must be unique");
    assert_no_other_global_text_symbols(&file, section_index, name);
    let symbol = &symbols[0];
    let offset = symbol
        .address()
        .checked_sub(section.address())
        .expect("stencil symbol precedes text");
    let size = symbol.size();
    assert!(offset == 0, "Rust stencil entry is not at section start");
    assert!(
        size == 0 || size as usize == bytes.len(),
        "Rust stencil has ambiguous symbol bounds"
    );
    let output = bytes.to_vec();
    assert!(
        !output.is_empty(),
        "Rust stencil has empty instruction bounds"
    );
    if matches!(file.architecture(), object::Architecture::Aarch64) {
        assert_eq!(
            output.len() % 4,
            0,
            "AArch64 instruction bounds are invalid"
        );
    }
    output
}

fn parse_object_range(
    path: &Path,
    name: &str,
    end_name: &str,
    expected_relocations: &[ExpectedRelocation],
) -> ParsedFragment {
    let data = fs::read(path).expect("read Rust fragment object");
    let file = object::File::parse(&*data).expect("parse Rust fragment object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_target_format(&file, env::var("TARGET").ok().as_deref());
    reject_unwind_or_tls_sections(&file);
    let relocations = validate_fragment_relocations(&file, expected_relocations, "Rust fragment");
    let (start_section, start_address) = find_text_symbol(&file, name);
    let (end_section, end_address) = find_text_symbol(&file, end_name);
    assert_eq!(start_section, end_section, "fragment bounds cross sections");
    let section_index = start_section.expect("fragment start section");
    let section = file
        .sections()
        .find(|section| section.index() == section_index)
        .expect("fragment text section");
    assert_eq!(
        section.kind(),
        SectionKind::Text,
        "fragment is not executable text"
    );
    let start_offset = start_address
        .checked_sub(section.address())
        .expect("fragment start precedes section") as usize;
    let end_offset = end_address
        .checked_sub(section.address())
        .expect("fragment end precedes section") as usize;
    assert!(
        start_offset < end_offset,
        "fragment bounds are empty or reversed"
    );
    let bytes = section.uncompressed_data().expect("read fragment text");
    assert!(end_offset <= bytes.len(), "fragment end exceeds section");
    assert_eq!(start_offset, 0, "fragment entry is not at section start");
    let output = bytes[start_offset..end_offset].to_vec();
    assert!(!output.is_empty(), "fragment has empty instruction bounds");
    if matches!(file.architecture(), object::Architecture::Aarch64) {
        assert_eq!(output.len() % 4, 0, "AArch64 fragment bounds are invalid");
    }
    assert_no_other_global_text_symbols(&file, section_index, name);
    ParsedFragment {
        bytes: output,
        relocations,
    }
}

fn validate_fragment_relocations(
    file: &object::File<'_>,
    expected: &[ExpectedRelocation],
    context: &str,
) -> Vec<ExtractedRelocation> {
    let mut records = Vec::new();
    if let object::File::MachO64(macho) = file {
        let mut relocation_count = 0;
        let mut consumed = vec![false; expected.len()];
        for section in macho.sections() {
            let text_section = section.kind() == SectionKind::Text;
            for relocation in section
                .macho_relocations()
                .expect("read Mach-O fragment relocations")
            {
                assert!(
                    text_section,
                    "Mach-O fragment relocation targets non-text data"
                );
                let info = relocation.info(Endianness::Little);
                assert!(
                    info.r_address <= u32::from(u16::MAX),
                    "Mach-O relocation offset is outside the declared range"
                );
                let target = macho
                    .symbol_by_index(object::SymbolIndex(info.r_symbolnum as usize))
                    .expect("Mach-O fragment relocation symbol")
                    .name()
                    .expect("Mach-O fragment relocation name")
                    .trim_start_matches('_');
                assert!(
                    info.r_extern,
                    "Mach-O fragment relocation must name a symbol"
                );
                let expected_index = expected_relocation_index(
                    expected,
                    &consumed,
                    section.kind(),
                    u64::from(info.r_address),
                    "Branch26",
                    target,
                )
                .expect("undeclared or duplicate Mach-O fragment relocation");
                let item = expected[expected_index];
                assert_eq!(item.addend, 0, "Mach-O branch addend must be zero");
                assert_eq!(item.width, 4, "Mach-O branch hole width must be four bytes");
                assert_eq!(info.r_type, object::macho::ARM64_RELOC_BRANCH26);
                assert!(info.r_pcrel && info.r_length == 2);
                let bytes = section
                    .uncompressed_data()
                    .expect("read Mach-O relocation section");
                let start = usize::try_from(info.r_address).expect("nonnegative relocation offset");
                let instruction = bytes
                    .get(start..start + 4)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .expect("Mach-O branch relocation lies within text");
                assert_eq!(
                    instruction & 0x03ff_ffff,
                    0,
                    "Mach-O Branch26 implicit addend must be zero"
                );
                consumed[expected_index] = true;
                records.push(ExtractedRelocation {
                    offset: item.offset,
                    kind: item.kind,
                    target: item.target,
                    addend: item.addend,
                });
                relocation_count += 1;
            }
        }
        assert_eq!(
            relocation_count,
            expected.len(),
            "{context} relocation count mismatch"
        );
        assert!(
            consumed.into_iter().all(|matched| matched),
            "{context} missing relocation"
        );
        return records;
    }
    let text_index = file
        .sections()
        .find(|section| section.kind() == SectionKind::Text)
        .map(|section| section.index());
    let mut consumed = vec![false; expected.len()];
    for (relocation_section, offset, relocation) in file.sections().flat_map(|section| {
        section
            .relocations()
            .map(move |(offset, relocation)| (section.index(), offset, relocation))
    }) {
        let Some(text_section) = text_index else {
            panic!("{context} relocation has no text section")
        };
        assert_eq!(
            relocation_section, text_section,
            "fragment relocation section mismatch"
        );
        let target_name = match relocation.target() {
            RelocationTarget::Symbol(index) => file
                .symbol_by_index(index)
                .expect("fragment relocation symbol")
                .name()
                .expect("fragment relocation symbol name")
                .trim_start_matches('_')
                .to_owned(),
            _ => panic!("{context} relocation does not name a declared symbol"),
        };
        let expected_index = expected_relocation_index(
            expected,
            &consumed,
            SectionKind::Text,
            offset,
            "Branch26",
            &target_name,
        )
        .unwrap_or_else(|| panic!("{context} relocation at {offset:#x} is undeclared"));
        let item = expected[expected_index];
        consumed[expected_index] = true;
        assert_eq!(item.kind, "Branch26");
        assert_eq!(item.width, 4, "branch hole width must be four bytes");
        assert_eq!(item.addend, relocation.addend());
        assert_eq!(u64::from(item.offset), offset);
        assert_eq!(relocation.kind(), RelocationKind::PltRelative);
        assert_eq!(relocation.encoding(), RelocationEncoding::AArch64Call);
        assert_eq!(relocation.size(), 26);
        records.push(ExtractedRelocation {
            offset: item.offset,
            kind: item.kind,
            target: item.target,
            addend: item.addend,
        });
    }
    let relocation_count = file
        .sections()
        .map(|section| section.relocations().count())
        .sum::<usize>();
    assert_eq!(
        relocation_count,
        expected.len(),
        "{context} relocation count mismatch"
    );
    assert!(
        consumed.into_iter().all(|matched| matched),
        "{context} missing relocation"
    );
    records
}

fn find_text_symbol<'data>(
    file: &object::File<'data>,
    name: &str,
) -> (Option<object::SectionIndex>, u64) {
    let mut symbols = file
        .symbols()
        .filter(|symbol| {
            symbol
                .name()
                .ok()
                .is_some_and(|value| value.trim_start_matches('_') == name)
        })
        .filter(|symbol| symbol.section_index().is_some())
        .collect::<Vec<_>>();
    assert_eq!(symbols.len(), 1, "fragment symbol {name} must be unique");
    let symbol = symbols.pop().expect("fragment symbol exists");
    (symbol.section_index(), symbol.address())
}

fn assert_no_other_global_text_symbols<'data>(
    file: &object::File<'data>,
    section_index: object::SectionIndex,
    entry_name: &str,
) {
    for symbol in file.symbols() {
        if symbol.section_index() != Some(section_index) || !symbol.is_global() {
            continue;
        }
        let name = symbol.name().unwrap_or_default().trim_start_matches('_');
        assert_eq!(
            name, entry_name,
            "Rust stencil has another global text symbol {name:?}"
        );
    }
}

fn assert_single_text_section<'data>(file: &object::File<'data>) {
    let text_sections = file
        .sections()
        .filter(|section| section.kind() == SectionKind::Text)
        .count();
    assert_eq!(
        text_sections, 1,
        "Rust stencil must have one executable text section"
    );
}

fn assert_target_architecture<'data>(file: &object::File<'data>, target: Option<&str>) {
    let expected = target.map(|triple| {
        if triple.starts_with("aarch64") {
            object::Architecture::Aarch64
        } else if triple.starts_with("x86_64") {
            object::Architecture::X86_64
        } else {
            panic!("unsupported stencil artifact target {triple}");
        }
    });
    if let Some(expected) = expected {
        assert_eq!(
            file.architecture(),
            expected,
            "Rust stencil target architecture mismatch"
        );
    }
}

fn assert_target_format<'data>(file: &object::File<'data>, target: Option<&str>) {
    let Some(target) = target else { return };
    let expected = if target.contains("-apple-") {
        BinaryFormat::MachO
    } else if target.contains("-windows-") {
        BinaryFormat::Coff
    } else {
        BinaryFormat::Elf
    };
    assert_eq!(
        file.format(),
        expected,
        "Rust stencil object format does not match target {target}"
    );
}

fn reject_unwind_or_tls_sections<'data>(file: &object::File<'data>) {
    for section in file.sections() {
        let name = section.name().unwrap_or_default();
        assert!(
            !matches!(name, ".eh_frame" | "__eh_frame" | ".gcc_except_table"),
            "Rust leaf carries an unsupported unwind section {name}"
        );
        assert!(
            !matches!(
                section.kind(),
                SectionKind::Tls | SectionKind::UninitializedTls | SectionKind::TlsVariables
            ),
            "Rust leaf carries unsupported TLS data"
        );
    }
}

/// Validate every relocation, including local references.  Isolated Rust
/// leaves currently declare no holes, so any relocation is rejected rather
/// than silently dropping a local literal/helper reference. Composable
/// templates will pass a catalog-derived allowlist here when their relocation
/// records are represented in `Stencil` metadata.
fn validate_relocations<'data>(file: &object::File<'data>, context: &str) {
    for section in file.sections() {
        for (offset, relocation) in section.relocations() {
            panic!(
                "{context} contains unsupported relocation at {offset:#x}: kind={:?} encoding={:?} size={}",
                relocation.kind(),
                relocation.encoding(),
                relocation.size()
            );
        }
    }
}

fn render_artifact(
    declaration: &RegionDeclaration,
    target: &str,
    compiler: &str,
    fingerprint: &str,
    extracted: &ExtractedObject,
) -> (String, String) {
    let name = declaration.name;
    let code = extracted
        .bytes
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    let identifier = name.to_ascii_uppercase();
    let constant = format!(
        "const BYTES_{identifier}: &[u8] = &[{code}];{}",
        extracted
            .fallthrough
            .as_ref()
            .map_or_else(String::new, |tail| {
                let code = tail
                    .iter()
                    .map(|byte| format!("0x{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("\nconst FALLTHROUGH_{identifier}: &[u8] = &[{code}];")
            })
    );
    let entries = declaration
        .external_entries
        .iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let row = format!(
        "    BuildStencilArtifact {{ name: {name:?}, artifact_id: {artifact_id:?}, key: CANONICAL_{identifier}_KEY, target: {target:?}, compiler: {compiler:?}, fingerprint: {fingerprint:?}, abi: {}, entry: {}, external_entries: &[{}], has_fallthrough: {}, executable: true, template_calls_helper: {}, bytes: BYTES_{identifier}, data: &[], relocations: {}, stencil: crate::stencil_fact::Stencil {{ bytes: BYTES_{identifier}, holes: {} }}, fallthrough: {}, fallthrough_entry: {} }},",
        super::abi_expr(declaration),
        declaration.entry,
        entries,
        declaration.name == "fallthrough",
        super::target_template_calls_helper(declaration),
        relocation_expr(extracted),
        holes_expr(declaration, target, extracted.fallthrough.is_some()),
        extracted.fallthrough.as_ref().map_or("None".to_owned(), |_| {
            format!("Some(crate::stencil_fact::Stencil {{ bytes: FALLTHROUGH_{identifier}, holes: &[] }})")
        }),
        fallthrough_offset(declaration, target),
        artifact_id = format!("{name}@{fingerprint}"),
    );
    (constant, row)
}

fn relocation_expr(extracted: &ExtractedObject) -> String {
    let entries = extracted
        .relocations
        .iter()
        .map(|relocation| {
            format!(
                "crate::stencil_select::PhysicalRelocation {{ offset: {}, kind: crate::stencil_fact::HoleKind::{}, target: {:?}, addend: {} }}",
                relocation.offset, relocation.kind, relocation.target, relocation.addend
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{entries}]")
}

fn fallthrough_offset(declaration: &RegionDeclaration, target: &str) -> u16 {
    let holes = if target.starts_with("aarch64") {
        declaration.aarch64_holes
    } else {
        declaration.holes
    };
    holes
        .iter()
        .find(|(_, _, kind)| *kind == "Branch26" || *kind == "Rel32")
        .map_or(0, |(offset, _, _)| *offset)
}

fn holes_expr(declaration: &RegionDeclaration, target: &str, composable: bool) -> String {
    if !composable {
        return "&[]".to_owned();
    }
    let holes = if target.starts_with("aarch64") {
        declaration.aarch64_holes
    } else {
        declaration.holes
    };
    let values = holes
        .iter()
        .map(|(offset, _width, kind)| {
            format!(
                "crate::stencil_fact::Hole {{ offset: {offset}, kind: crate::stencil_fact::HoleKind::{kind} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{values}]")
}

fn fingerprint(
    target: &str,
    compiler: &str,
    flags: &[&str],
    declarations: &[RegionDeclaration],
) -> String {
    let version = command_output(Command::new(compiler).arg("-vV"), "read rustc identity");
    let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let rustflags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let schema = declarations
        .iter()
        .map(|item| {
            let source = super::rust_leaf_recipe(item)
                .map(|recipe| rust_source(item.name, recipe))
                .or_else(|| match item.name {
                    "fallthrough" => Some(
                        super::build_stencil_templates::aarch64_head().to_owned()
                            + super::build_stencil_templates::aarch64_tail(),
                    ),
                    "array_numeric_loop" => {
                        Some(super::build_stencil_templates::aarch64_array_loop().to_owned())
                    }
                    "prototype_property" => Some(
                        super::build_stencil_templates::aarch64_prototype_property().to_owned(),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            format!(
                "{name}:{abi:?}:{ops:?}:{x86:?}:{arm:?}:{portable:?}:{holes:?}:{arm_holes:?}:{entry}:{external:?}:{source}",
                name = item.name,
                abi = item.abi,
                ops = item.operations,
                x86 = item.x86_bytes,
                arm = item.aarch64_bytes,
                portable = item.portable_bytes,
                holes = item.holes,
                arm_holes = item.aarch64_holes,
                entry = item.entry,
                external = item.external_entries,
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!(
        "{target}\n{version}\n{features}\n{rustflags}\n{flags:?}\n{schema}\nphysical-abi-v3\nobject-extractor-v2"
    )
    .bytes()
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv64-{hash:016x}")
}

fn artifact_fingerprint(
    declaration: &RegionDeclaration,
    target: &str,
    build_fingerprint: &str,
    extracted: &ExtractedObject,
) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    hash_bytes(&mut hash, build_fingerprint.as_bytes());
    hash_bytes(&mut hash, target.as_bytes());
    hash_bytes(&mut hash, format!("{:?}", declaration.abi).as_bytes());
    hash_bytes(&mut hash, &declaration.entry.to_le_bytes());
    for entry in declaration.external_entries {
        hash_bytes(&mut hash, &entry.to_le_bytes());
    }
    hash_bytes(
        &mut hash,
        &[u8::from(super::target_template_calls_helper(declaration))],
    );
    hash_bytes(&mut hash, &extracted.bytes);
    if let Some(fallthrough) = &extracted.fallthrough {
        hash_bytes(&mut hash, fallthrough);
    }
    for relocation in &extracted.relocations {
        hash_bytes(&mut hash, &relocation.offset.to_le_bytes());
        hash_bytes(&mut hash, relocation.kind.as_bytes());
        hash_bytes(&mut hash, relocation.target.as_bytes());
        hash_bytes(&mut hash, &relocation.addend.to_le_bytes());
    }
    format!("fnv64-{hash:016x}")
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn rustc_path() -> String {
    env::var_os("QUENCH_RUSTC")
        .or_else(|| env::var_os("RUSTC"))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rustc".to_owned())
}

fn effective_rustflags() -> Vec<String> {
    env::var("CARGO_ENCODED_RUSTFLAGS")
        .unwrap_or_default()
        .split('\u{1f}')
        .filter(|flag| !flag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn unique_directory() -> OwnedDirectory {
    let base = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .expect("OUT_DIR for Rust stencil artifacts");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    for attempt in 0..8u8 {
        let directory = base.join(format!(
            "stencil-objects-{stamp}-{}-{attempt}",
            std::process::id()
        ));
        if fs::create_dir(&directory).is_ok() {
            return OwnedDirectory { path: directory };
        }
    }
    panic!("cannot create unique Rust stencil object directory")
}

fn run(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}

fn command_output(command: &mut Command, description: &str) -> String {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(
        output.status.success(),
        "{description} exited with {}",
        output.status
    );
    String::from_utf8(output.stdout).expect("Rust tool output is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(entries: &'static [u32]) -> RegionDeclaration {
        RegionDeclaration {
            name: "identity_probe",
            operations: &["Add", "Return"],
            abi: super::super::DeclAbi::Scalar,
            x86_bytes: &[1],
            aarch64_bytes: &[2],
            portable_bytes: &[3],
            holes: &[],
            aarch64_holes: &[],
            entry: 0,
            external_entries: entries,
        }
    }

    fn extracted(bytes: &[u8]) -> ExtractedObject {
        ExtractedObject {
            bytes: bytes.to_vec(),
            fallthrough: None,
            relocations: Vec::new(),
        }
    }

    fn expected(offset: u16, target: &'static str) -> ExpectedRelocation {
        ExpectedRelocation {
            section: SectionKind::Text,
            offset,
            width: 4,
            kind: "Branch26",
            target,
            addend: 0,
        }
    }

    #[test]
    fn relocation_identity_matching_is_order_independent() {
        let records = [expected(12, "tail"), expected(4, "helper")];
        let mut consumed = [false; 2];
        let second = expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            4,
            "Branch26",
            "helper",
        )
        .expect("second record");
        consumed[second] = true;
        let first = expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            12,
            "Branch26",
            "tail",
        )
        .expect("first record");
        assert_eq!(first, 0);
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            4,
            "Branch26",
            "helper",
        )
        .is_none());
    }

    #[test]
    fn relocation_identity_matching_rejects_wrong_target_or_kind() {
        let records = [expected(8, "tail")];
        let consumed = [false];
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            8,
            "Branch26",
            "other",
        )
        .is_none());
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Text,
            8,
            "Page21",
            "tail",
        )
        .is_none());
        assert!(expected_relocation_index(
            &records,
            &consumed,
            SectionKind::Data,
            8,
            "Branch26",
            "tail",
        )
        .is_none());
    }

    #[test]
    fn artifact_identity_covers_entries_and_extracted_payload() {
        static ENTRY_ZERO: [u32; 1] = [0];
        static ENTRY_ZERO_FOUR: [u32; 2] = [0, 4];
        let base = declaration(&ENTRY_ZERO);
        let changed_entries = declaration(&ENTRY_ZERO_FOUR);
        let identity = artifact_fingerprint(&base, "aarch64-test", "build", &extracted(&[1, 2]));
        let entry_identity = artifact_fingerprint(
            &changed_entries,
            "aarch64-test",
            "build",
            &extracted(&[1, 2]),
        );
        let byte_identity =
            artifact_fingerprint(&base, "aarch64-test", "build", &extracted(&[1, 3]));
        assert_ne!(identity, entry_identity);
        assert_ne!(identity, byte_identity);
    }
}
