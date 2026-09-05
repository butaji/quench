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
    "pub struct BuildStencilArtifact { pub name: &'static str, pub key: crate::stencil_fact::RegionKey, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub abi: crate::stencil_select::RegionAbi, pub entry: u16, pub external_entries: &'static [u16], pub has_fallthrough: bool, pub executable: bool, pub template_calls_helper: bool, pub bytes: &'static [u8], pub stencil: crate::stencil_fact::Stencil, pub fallthrough: Option<crate::stencil_fact::Stencil>, pub fallthrough_entry: u16 }"
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
    let fingerprint = fingerprint(&target, &compiler, &flags, declarations);
    let root = unique_directory();
    let mut constants = Vec::new();
    let mut rows = Vec::new();
    for declaration in declarations {
        let Some(recipe) = super::rust_leaf_recipe(declaration) else {
            continue;
        };
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
        } else {
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
            }
        };
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
        };
    }
    let head = compile_assembly_fragment(
        root,
        target,
        compiler,
        flags,
        rustflags,
        "fallthrough_head",
        aarch64_head_source(),
        declaration.aarch64_holes,
        Some("q_fallthrough_tail"),
    );
    let tail = compile_assembly_fragment(
        root,
        target,
        compiler,
        flags,
        rustflags,
        "fallthrough_tail",
        aarch64_tail_source(),
        &[],
        None,
    );
    ExtractedObject {
        bytes: head,
        fallthrough: Some(tail),
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
    expected_relocations: &[(u16, usize, &'static str)],
    target_symbol: Option<&str>,
) -> Vec<u8> {
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
        target_symbol,
    )
}

fn aarch64_head_source() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_fallthrough_head\nq_fallthrough_head:\n  fadd d0, d0, d1\n  b q_fallthrough_tail\nq_fallthrough_head_end:\n\"#);\n"
}

fn aarch64_tail_source() -> &'static str {
    "#![no_std]\nuse core::arch::global_asm;\nglobal_asm!(r#\"\n.text\n.p2align 2\n.globl q_fallthrough_tail\nq_fallthrough_tail:\n  ret\nq_fallthrough_tail_end:\n\"#);\n"
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
    expected_relocations: &[(u16, usize, &'static str)],
    target_symbol: Option<&str>,
) -> Vec<u8> {
    let data = fs::read(path).expect("read Rust fragment object");
    let file = object::File::parse(&*data).expect("parse Rust fragment object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_target_format(&file, env::var("TARGET").ok().as_deref());
    reject_unwind_or_tls_sections(&file);
    validate_fragment_relocations(&file, expected_relocations, target_symbol, "Rust fragment");
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
    output
}

fn validate_fragment_relocations(
    file: &object::File<'_>,
    expected: &[(u16, usize, &'static str)],
    target_symbol: Option<&str>,
    context: &str,
) {
    if let object::File::MachO64(macho) = file {
        let mut relocation_count = 0;
        for section in macho.sections() {
            for relocation in section
                .macho_relocations()
                .expect("read Mach-O fragment relocations")
            {
                let info = relocation.info(Endianness::Little);
                let expected_target = target_symbol.expect("declared Mach-O relocation target");
                let (expected_offset, _, expected_kind) = expected
                    .first()
                    .copied()
                    .expect("declared Mach-O relocation hole");
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
                assert_eq!(info.r_address, u32::from(expected_offset));
                assert_eq!(target, expected_target);
                assert_eq!(info.r_type, object::macho::ARM64_RELOC_BRANCH26);
                assert!(info.r_pcrel && info.r_length == 2);
                assert_eq!(expected_kind, "Branch26");
                relocation_count += 1;
            }
        }
        assert_eq!(
            relocation_count,
            expected.len(),
            "{context} relocation count mismatch"
        );
        return;
    }
    let text_index = file
        .sections()
        .find(|section| section.kind() == SectionKind::Text)
        .map(|section| section.index());
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
        let Some((expected_offset, _width, expected_kind)) = expected
            .iter()
            .find(|(expected_offset, _, _)| u64::from(*expected_offset) == offset)
        else {
            panic!("{context} relocation at {offset:#x} is undeclared")
        };
        assert_eq!(Some(target_name.as_str()), target_symbol);
        assert_eq!(*expected_kind, "Branch26");
        assert_eq!(u64::from(*expected_offset), offset);
        assert_eq!(relocation.kind(), RelocationKind::PltRelative);
        assert_eq!(relocation.encoding(), RelocationEncoding::AArch64Call);
        assert_eq!(relocation.size(), 26);
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
        "    BuildStencilArtifact {{ name: {name:?}, key: CANONICAL_{identifier}_KEY, target: {target:?}, compiler: {compiler:?}, fingerprint: {fingerprint:?}, abi: {}, entry: {}, external_entries: &[{}], has_fallthrough: {}, executable: true, template_calls_helper: {}, bytes: BYTES_{identifier}, stencil: crate::stencil_fact::Stencil {{ bytes: BYTES_{identifier}, holes: {} }}, fallthrough: {}, fallthrough_entry: {} }},",
        super::abi_expr(declaration),
        declaration.entry,
        entries,
        declaration.name == "fallthrough",
        super::target_template_calls_helper(declaration),
        holes_expr(declaration, target, extracted.fallthrough.is_some()),
        extracted.fallthrough.as_ref().map_or("None".to_owned(), |_| {
            format!("Some(crate::stencil_fact::Stencil {{ bytes: FALLTHROUGH_{identifier}, holes: &[] }})")
        }),
        fallthrough_offset(declaration, target),
    );
    (constant, row)
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
                .unwrap_or_default();
            format!(
                "{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{source}",
                item.name,
                item.abi,
                item.operations,
                item.x86_bytes,
                item.aarch64_bytes,
                item.holes,
                item.aarch64_holes,
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{target}\n{version}\n{features}\n{rustflags}\n{flags:?}\n{schema}\nrust-leaf-recipes-v1\nabi-v3").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv64-{hash:016x}")
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
