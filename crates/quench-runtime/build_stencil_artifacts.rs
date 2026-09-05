use std::{env, fs, path::{Path, PathBuf}, process::Command};

use object::read::{Object, ObjectSection, ObjectSymbol};
use object::SectionKind;
use object::SymbolSection;

use super::RegionDeclaration;

const HEADER: &str = "/// Rust object artifacts generated at build time.\n";

pub(crate) fn generate(out_dir: &Path, declarations: &[RegionDeclaration]) {
    let target = env::var("TARGET").unwrap_or_default();
    let generated = if env::var_os("QUENCH_GENERATE_STENCIL_OBJECTS").is_some()
        && supports_target(&target)
    {
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
    assert_single_text_section(&file);
    reject_unwind_or_tls_sections(&file);
    assert_no_relocations(&file, "assembly verifier");
    let mut sections = file.sections().filter(|section| {
        section
            .name()
            .ok()
            .is_some_and(|name| name == ".text" || name == "__text")
    });
    let section = sections.next().expect("Rust assembly text section");
    assert!(sections.next().is_none(), "assembly verifier found multiple text sections");
    let bytes = section.uncompressed_data().expect("read Rust assembly text");
    assert!(!bytes.is_empty() && bytes.len() % 4 == 0, "assembly verifier found invalid instruction bounds");
    for word in expected {
        let needle = word.to_le_bytes();
        assert!(bytes.chunks_exact(4).any(|chunk| chunk == needle), "assembly word {word:08x} missing from extracted text");
    }
}

pub(crate) fn verify_symbols(path: &Path, names: &[&str]) {
    let data = fs::read(path).expect("read Rust assembly object");
    let file = object::File::parse(&*data).expect("parse Rust assembly object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_single_text_section(&file);
    reject_unwind_or_tls_sections(&file);
    assert_no_relocations(&file, "assembly verifier");
    let mut sections = file.sections().filter(|section| {
        section
            .name()
            .ok()
            .is_some_and(|name| name == ".text" || name == "__text")
    });
    let section = sections.next().expect("Rust assembly text section");
    assert!(sections.next().is_none(), "assembly verifier found multiple text sections");
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
        assert!(offset < section.size() as usize, "assembly symbol is outside text");
        previous = offset;
    }
}

fn empty_artifacts() -> String {
    format!(
        "{HEADER}pub struct BuildStencilArtifact {{ pub name: &'static str, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub bytes: &'static [u8], pub stencil: crate::stencil_fact::Stencil }}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[];\n"
    )
}

fn extract_objects(declarations: &[RegionDeclaration]) -> String {
    let target = env::var("TARGET").expect("TARGET for stencil object generation");
    let compiler = rustc_path();
    let flags = ["--crate-type=lib", "--emit=obj", "-Copt-level=2", "-Cpanic=abort", "-Coverflow-checks=off", "--edition=2021"];
    let fingerprint = fingerprint(&target, &compiler, &flags, declarations);
    let root = unique_directory();
    let mut constants = Vec::new();
    let mut rows = Vec::new();
    for declaration in declarations {
        if !declaration.holes.is_empty() || !declaration.aarch64_holes.is_empty() {
            continue;
        }
        let Some(recipe) = super::rust_leaf_recipe(declaration.operations) else {
            continue;
        };
        let artifact = compile_one(&root.path, &target, &compiler, &flags, declaration, recipe);
        let (constant, row) = render_artifact(declaration.name, &target, &compiler, &fingerprint, &artifact);
        constants.push(constant);
        rows.push(row);
    }
    assert!(!rows.is_empty(), "no extractable Rust stencil declarations");
    let generated = format!(
        "{HEADER}pub struct BuildStencilArtifact {{ pub name: &'static str, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub bytes: &'static [u8], pub stencil: crate::stencil_fact::Stencil }}\n{}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[\n{}\n];\n",
        constants.join("\n"),
        rows.join("\n")
    );
    generated
}

fn rust_source(name: &str, recipe: super::RustLeafRecipe) -> String {
    format!(
        "#![no_std]\n#[no_mangle]\n#[inline(never)]\npub extern \"C\" fn q_{}({}) -> f64 {{ {} }}\n",
        name,
        recipe.parameters(),
        recipe.expression()
    )
}

fn compile_one(
    root: &Path,
    target: &str,
    compiler: &str,
    flags: &[&str],
    declaration: &RegionDeclaration,
    recipe: super::RustLeafRecipe,
) -> Vec<u8> {
    let source = root.join(format!("{}.rs", declaration.name));
    let object = root.join(format!("{}.o", declaration.name));
    fs::write(&source, rust_source(declaration.name, recipe)).expect("write Rust stencil source");
    let mut command = Command::new(compiler);
    command.args(["--target", target]).args(flags).args([source.to_str().unwrap(), "-o", object.to_str().unwrap()]);
    run(&mut command, "compile Rust stencil object");
    parse_object(&object, &format!("q_{}", declaration.name))
}

fn parse_object(path: &Path, name: &str) -> Vec<u8> {
    let data = fs::read(path).expect("read Rust stencil object");
    let file = object::File::parse(&*data).expect("parse Rust stencil object");
    assert_target_architecture(&file, env::var("TARGET").ok().as_deref());
    assert_single_text_section(&file);
    reject_unwind_or_tls_sections(&file);
    assert_no_relocations(&file, "Rust stencil");
    for symbol in file.symbols() {
        if matches!(symbol.section(), SymbolSection::Undefined) {
            panic!("Rust stencil has undeclared external symbol {:?}", symbol.name());
        }
    }
    let mut sections = file.sections().filter(|section| {
        section
            .name()
            .ok()
            .is_some_and(|name| name == ".text" || name == "__text")
    });
    let section = sections.next().expect("Rust stencil text section");
    assert!(sections.next().is_none(), "Rust stencil has multiple text sections");
    let section_index = section.index();
    let bytes = section.uncompressed_data().expect("read Rust stencil text");
    assert_eq!(section.size() as usize, bytes.len(), "Rust stencil text size is ambiguous");
    assert_eq!(section.align() % 4, 0, "Rust stencil text alignment is invalid");
    let symbols = file.symbols().filter(|symbol| symbol.section_index() == Some(section_index)).filter(|symbol| symbol.name().ok().is_some_and(|value| value.trim_start_matches('_') == name)).collect::<Vec<_>>();
    assert_eq!(symbols.len(), 1, "Rust stencil entry symbol must be unique");
    assert_no_other_global_text_symbols(&file, section_index, name);
    let symbol = &symbols[0];
    let offset = symbol.address().checked_sub(section.address()).expect("stencil symbol precedes text");
    let size = symbol.size();
    assert!(offset == 0, "Rust stencil entry is not at section start");
    assert!(size == 0 || size as usize == bytes.len(), "Rust stencil has ambiguous symbol bounds");
    let output = bytes.to_vec();
    assert!(!output.is_empty() && output.len() % 4 == 0, "Rust stencil has invalid instruction bounds");
    output
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
        assert_eq!(name, entry_name, "Rust stencil has another global text symbol {name:?}");
    }
}

fn assert_single_text_section<'data>(file: &object::File<'data>) {
    let text_sections = file
        .sections()
        .filter(|section| section.kind() == SectionKind::Text)
        .count();
    assert_eq!(text_sections, 1, "Rust stencil must have one executable text section");
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
        assert_eq!(file.architecture(), expected, "Rust stencil target architecture mismatch");
    }
}

fn reject_unwind_or_tls_sections<'data>(file: &object::File<'data>) {
    for section in file.sections() {
        let name = section.name().unwrap_or_default();
        assert!(!matches!(name, ".eh_frame" | "__eh_frame" | ".gcc_except_table"),
            "Rust leaf carries an unsupported unwind section {name}");
        assert!(!matches!(section.kind(), SectionKind::Tls | SectionKind::UninitializedTls | SectionKind::TlsVariables),
            "Rust leaf carries unsupported TLS data");
    }
}

fn assert_no_relocations<'data>(file: &object::File<'data>, context: &str) {
    for section in file.sections() {
        if let Some((offset, relocation)) = section.relocations().next() {
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
    name: &str,
    target: &str,
    compiler: &str,
    fingerprint: &str,
    bytes: &[u8],
) -> (String, String) {
    let code = bytes.iter().map(|byte| format!("0x{byte:02x}")).collect::<Vec<_>>().join(", ");
    let identifier = name.to_ascii_uppercase();
    let constant = format!("const BYTES_{identifier}: &[u8] = &[{code}];");
    let row = format!("    BuildStencilArtifact {{ name: {name:?}, target: {target:?}, compiler: {compiler:?}, fingerprint: {fingerprint:?}, bytes: BYTES_{identifier}, stencil: crate::stencil_fact::Stencil {{ bytes: BYTES_{identifier}, holes: &[] }} }},");
    (constant, row)
}

fn fingerprint(target: &str, compiler: &str, flags: &[&str], declarations: &[RegionDeclaration]) -> String {
    let version = command_output(Command::new(compiler).arg("-vV"), "read rustc identity");
    let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let rustflags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let schema = declarations
        .iter()
        .map(|item| {
            format!(
                "{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
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
    env::var_os("QUENCH_RUSTC").or_else(|| env::var_os("RUSTC")).map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|| "rustc".to_owned())
}

fn unique_directory() -> OwnedDirectory {
    let base = env::var_os("OUT_DIR").map(PathBuf::from).expect("OUT_DIR for Rust stencil artifacts");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    for attempt in 0..8u8 {
        let directory = base.join(format!("stencil-objects-{stamp}-{}-{attempt}", std::process::id()));
        if fs::create_dir(&directory).is_ok() {
            return OwnedDirectory { path: directory };
        }
    }
    panic!("cannot create unique Rust stencil object directory")
}

fn run(command: &mut Command, description: &str) {
    let status = command.status().unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}

fn command_output(command: &mut Command, description: &str) -> String {
    let output = command.output().unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(output.status.success(), "{description} exited with {}", output.status);
    String::from_utf8(output.stdout).expect("Rust tool output is UTF-8")
}
