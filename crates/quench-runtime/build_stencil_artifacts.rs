use std::{env, fs, path::{Path, PathBuf}, process::Command};

use object::read::{Object, ObjectSection, ObjectSymbol};
use object::SymbolSection;

use super::RegionDeclaration;

const HEADER: &str = "/// Rust object artifacts generated at build time.\n";

pub(crate) fn generate(out_dir: &Path, declarations: &[RegionDeclaration]) {
    let generated = if env::var_os("QUENCH_GENERATE_STENCIL_OBJECTS").is_some() {
        extract_objects(declarations)
    } else {
        empty_artifacts()
    };
    fs::write(out_dir.join("stencil_artifacts.rs"), generated)
        .expect("write generated stencil artifacts");
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
    let mut rows = Vec::new();
    for declaration in declarations.iter().filter(|item| extractable(item)) {
        let artifact = compile_one(&root, &target, &compiler, &flags, declaration);
        rows.push(render_artifact(declaration.name, &target, &compiler, &fingerprint, &artifact));
    }
    assert!(!rows.is_empty(), "no extractable Rust stencil declarations");
    let generated = format!(
        "{HEADER}pub struct BuildStencilArtifact {{ pub name: &'static str, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub bytes: &'static [u8], pub stencil: crate::stencil_fact::Stencil }}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[\n{}\n];\n",
        rows.join("\n")
    );
    let _ = fs::remove_dir_all(root);
    generated
}

fn extractable(declaration: &RegionDeclaration) -> bool {
    declaration.holes.is_empty() && declaration.aarch64_holes.is_empty() && recipe(declaration.operations).is_some()
}

fn recipe(operations: &[&str]) -> Option<&'static str> {
    match operations {
        ["Add", "Return"] => Some("a + b"),
        ["Sub", "Return"] => Some("a - b"),
        ["Mul", "Return"] => Some("a * b"),
        ["Div", "Return"] => Some("a / b"),
        _ => None,
    }
}

fn rust_source(declaration: &RegionDeclaration) -> String {
    let body = recipe(declaration.operations).expect("extractable recipe");
    format!("#![no_std]\n#[no_mangle]\n#[inline(never)]\npub extern \"C\" fn q_{}(a: f64, b: f64) -> f64 {{ {} }}\n", declaration.name, body)
}

fn compile_one(root: &Path, target: &str, compiler: &str, flags: &[&str], declaration: &RegionDeclaration) -> Vec<u8> {
    let source = root.join(format!("{}.rs", declaration.name));
    let object = root.join(format!("{}.o", declaration.name));
    fs::write(&source, rust_source(declaration)).expect("write Rust stencil source");
    let mut command = Command::new(compiler);
    command.args(["--target", target]).args(flags).args([source.to_str().unwrap(), "-o", object.to_str().unwrap()]);
    run(&mut command, "compile Rust stencil object");
    parse_object(&object, &format!("q_{}", declaration.name))
}

fn parse_object(path: &Path, name: &str) -> Vec<u8> {
    let data = fs::read(path).expect("read Rust stencil object");
    let file = object::File::parse(&*data).expect("parse Rust stencil object");
    for symbol in file.symbols() {
        if matches!(symbol.section(), SymbolSection::Undefined) {
            panic!("Rust stencil has undeclared external symbol {:?}", symbol.name());
        }
    }
    let section = file.sections().find(|section| section.name().ok().is_some_and(|name| name == ".text" || name == "__text"))
        .expect("Rust stencil text section");
    let section_index = section.index();
    let bytes = section.uncompressed_data().expect("read Rust stencil text");
    let symbols = file.symbols().filter(|symbol| symbol.section_index() == Some(section_index)).filter(|symbol| symbol.name().ok().is_some_and(|value| value.trim_start_matches('_') == name)).collect::<Vec<_>>();
    assert_eq!(symbols.len(), 1, "Rust stencil entry symbol must be unique");
    let symbol = &symbols[0];
    let offset = symbol.address().checked_sub(section.address()).expect("stencil symbol precedes text");
    let size = symbol.size();
    assert!(offset == 0, "Rust stencil entry is not at section start");
    assert!(size == 0 || size as usize == bytes.len(), "Rust stencil has ambiguous symbol bounds");
    let output = bytes.to_vec();
    assert!(!output.is_empty() && output.len() % 4 == 0, "Rust stencil has invalid instruction bounds");
    output
}

fn render_artifact(name: &str, target: &str, compiler: &str, fingerprint: &str, bytes: &[u8]) -> String {
    let code = bytes.iter().map(|byte| format!("0x{byte:02x}")).collect::<Vec<_>>().join(", ");
    format!("    BuildStencilArtifact {{ name: {name:?}, target: {target:?}, compiler: {compiler:?}, fingerprint: {fingerprint:?}, bytes: &[{code}], stencil: crate::stencil_fact::Stencil {{ bytes: &[{code}], holes: &[] }} }},")
}

fn fingerprint(target: &str, compiler: &str, flags: &[&str], declarations: &[RegionDeclaration]) -> String {
    let version = command_output(Command::new(compiler).arg("-vV"), "read rustc identity");
    let schema = declarations.iter().map(|item| format!("{}:{:?}:{:?}", item.name, item.abi, item.operations)).collect::<Vec<_>>().join("|");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{target}\n{version}\n{flags:?}\n{schema}\nabi-v3").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv64-{hash:016x}")
}

fn rustc_path() -> String {
    env::var_os("QUENCH_RUSTC").or_else(|| env::var_os("RUSTC")).map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|| "rustc".to_owned())
}

fn unique_directory() -> PathBuf {
    let base = env::var_os("OUT_DIR").map(PathBuf::from).expect("OUT_DIR for Rust stencil artifacts");
    let directory = base.join(format!("stencil-objects-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("create Rust stencil object directory");
    directory
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
