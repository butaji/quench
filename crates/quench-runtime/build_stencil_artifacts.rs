use std::{env, fs, path::Path, process::Command};

use super::RegionDeclaration;

const HEADER: &str =
    "/// Build-time object artifacts; generated, never hand-maintained.\n";

pub(crate) fn generate(out_dir: &Path, declarations: &[RegionDeclaration]) {
    let output = if env::var_os("QUENCH_GENERATE_STENCIL_OBJECTS").is_some() {
        extract_objects(declarations)
    } else {
        format!(
            "{HEADER}pub struct BuildStencilArtifact {{ pub name: &'static str, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub bytes: &'static [u8], pub stencil: crate::stencil_fact::Stencil }}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[];\n"
        )
    };
    fs::write(out_dir.join("stencil_artifacts.rs"), output)
        .expect("write generated stencil artifacts");
}

fn extract_objects(declarations: &[RegionDeclaration]) -> String {
    let target = env::var("TARGET").expect("TARGET for stencil object generation");
    let compiler = find_tool("QUENCH_CLANG", &["clang", "clang-18", "clang-17"]);
    let objcopy = find_optional_tool("QUENCH_OBJCOPY", &["llvm-objcopy", "objcopy"]);
    let nm = find_tool("QUENCH_NM", &["llvm-nm", "nm"]);
    let root = env::temp_dir().join(format!("quench-stencil-object-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create stencil object directory");
    let source = root.join("templates.c");
    let object = root.join("templates.o");
    let text = root.join("text.bin");
    let source_text = c_source(declarations);
    fs::write(&source, &source_text).expect("write stencil C templates");
    run(
        Command::new(&compiler).args([
            format!("--target={target}"),
            "-O2".to_owned(),
            "-ffreestanding".to_owned(),
            "-fno-stack-protector".to_owned(),
            "-fno-unwind-tables".to_owned(),
            "-fno-asynchronous-unwind-tables".to_owned(),
            "-fno-builtin".to_owned(),
            "-c".to_owned(),
            source.to_string_lossy().into_owned(),
            "-o".to_owned(),
            object.to_string_lossy().into_owned(),
        ]),
        "compile stencil C templates",
    );
    let undefined = output(Command::new(&nm).args(["-u", object.to_str().unwrap()]), "list undefined symbols");
    assert!(undefined.trim().is_empty(), "stencil object has undefined symbols: {undefined}");
    let bytes = extract_text(&objcopy, &object, &text);
    let symbols = output(Command::new(&nm).args(["-S", "--defined-only", object.to_str().unwrap()]), "read stencil symbols");
    let fingerprint = fingerprint(&target, &compiler, &source_text, declarations);
    let artifacts = artifacts(declarations, &symbols, &bytes, &target, &compiler, &fingerprint);
    cleanup(&root);
    artifacts
}

fn c_source(declarations: &[RegionDeclaration]) -> String {
    let mut out = String::from("#include <stdint.h>\n");
    for declaration in declarations {
        let Some(expression) = compiler_expression(declaration) else { continue };
        out.push_str(&format!(
            "__attribute__((visibility(\"hidden\"),noinline)) double q_{0}(double a,double b) {{ return {1}; }}\n",
            declaration.name, expression
        ));
    }
    out
}

fn expression(operations: &[&str]) -> Option<&'static str> {
    match operations {
        ["Add", "Return"] => Some("a+b"),
        ["Sub", "Return"] => Some("a-b"),
        ["Mul", "Return"] => Some("a*b"),
        ["Div", "Return"] => Some("a/b"),
        _ => None,
    }
}

fn compiler_expression(declaration: &RegionDeclaration) -> Option<&'static str> {
    (declaration.holes.is_empty() && declaration.aarch64_holes.is_empty())
        .then(|| expression(declaration.operations))
        .flatten()
}

fn artifacts(
    declarations: &[RegionDeclaration],
    symbols: &str,
    bytes: &[u8],
    target: &str,
    compiler: &str,
    fingerprint: &str,
) -> String {
    let mut rows = Vec::new();
    for declaration in declarations {
        if compiler_expression(declaration).is_none() { continue }
        let symbol = parse_symbol(symbols, &format!("q_{}", declaration.name), bytes.len());
        let end = symbol.offset.checked_add(symbol.size).expect("stencil symbol overflow");
        assert!(end <= bytes.len(), "stencil symbol exceeds text section");
        if target.starts_with("aarch64") {
            assert_eq!(symbol.offset % 4, 0, "AArch64 stencil is not aligned");
            assert_eq!(symbol.size % 4, 0, "AArch64 stencil has partial instruction");
        }
        let code = bytes[symbol.offset..end]
            .iter()
            .map(|byte| format!("0x{byte:02x}"))
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(format!(
            "    BuildStencilArtifact {{ name: {:?}, target: {:?}, compiler: {:?}, fingerprint: {:?}, bytes: &[{}], stencil: crate::stencil_fact::Stencil {{ bytes: &[{}], holes: &[] }} }},",
            declaration.name, target, compiler, fingerprint, code, code
        ));
    }
    format!(
        "{HEADER}pub struct BuildStencilArtifact {{ pub name: &'static str, pub target: &'static str, pub compiler: &'static str, pub fingerprint: &'static str, pub bytes: &'static [u8], pub stencil: crate::stencil_fact::Stencil }}\npub static BUILD_STENCIL_ARTIFACTS: &[BuildStencilArtifact] = &[\n{}\n];\n",
        rows.join("\n")
    )
}

struct Symbol {
    offset: usize,
    size: usize,
}

fn parse_symbol(text: &str, wanted: &str, text_len: usize) -> Symbol {
    let mut entries = Vec::new();
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let Some(name) = fields.iter().find(|field| field.trim_start_matches('_') == wanted) else { continue };
        let position = fields.iter().position(|field| *field == *name).unwrap_or(0);
        let numbers = fields[..position]
            .iter()
            .filter_map(|field| u64::from_str_radix(field.trim_start_matches("0x"), 16).ok())
            .collect::<Vec<_>>();
        if numbers.len() >= 2 {
            entries.push((numbers[numbers.len() - 2] as usize, numbers[numbers.len() - 1] as usize));
        }
    }
    let Some((offset, mut size)) = entries.first().copied() else {
        panic!("missing stencil symbol {wanted} in nm output: {text}");
    };
    if size == 0 {
        let next = text.lines().filter_map(symbol_offset).filter(|value| *value > offset).min();
        size = next.unwrap_or(text_len).saturating_sub(offset);
    }
    assert!(size > 0, "stencil symbol {wanted} has no measurable size");
    Symbol { offset, size }
}

fn symbol_offset(line: &str) -> Option<usize> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    fields.first().and_then(|field| usize::from_str_radix(field, 16).ok())
}

fn fingerprint(
    target: &str,
    compiler: &str,
    source: &str,
    declarations: &[RegionDeclaration],
) -> String {
    let version = output(Command::new(compiler).arg("--version"), "read stencil compiler identity");
    let schema = declarations
        .iter()
        .map(|declaration| format!("{}:{:?}:{:?}", declaration.name, declaration.abi, declaration.operations))
        .collect::<Vec<_>>()
        .join("|");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{target}\n{version}\n{source}\n{schema}\nabi-v2").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv64-{hash:016x}")
}

fn find_tool(variable: &str, candidates: &[&str]) -> String {
    if let Some(path) = env::var_os(variable) { return path.to_string_lossy().into_owned() }
    if cfg!(target_os = "macos") {
        for candidate in candidates {
            if let Ok(output) = Command::new("xcrun").args(["--find", candidate]).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    if !path.is_empty() { return path }
                }
            }
        }
    }
    candidates
        .iter()
        .find(|candidate| Command::new(candidate).arg("--version").output().is_ok())
        .map(|candidate| (*candidate).to_owned())
        .unwrap_or_else(|| panic!("no {variable} tool found"))
}

fn find_optional_tool(variable: &str, candidates: &[&str]) -> Option<String> {
    if let Some(path) = env::var_os(variable) { return Some(path.to_string_lossy().into_owned()) }
    candidates
        .iter()
        .find(|candidate| Command::new(candidate).arg("--version").output().is_ok())
        .map(|candidate| (*candidate).to_owned())
}

fn extract_text(objcopy: &Option<String>, object: &Path, text: &Path) -> Vec<u8> {
    if let Some(objcopy) = objcopy {
        run(
            Command::new(objcopy).args([
                "--dump-section".to_owned(),
                format!(".text={}", text.to_string_lossy()),
                object.to_string_lossy().into_owned(),
            ]),
            "extract stencil text section",
        );
        return fs::read(text).expect("read extracted stencil text");
    }
    let tool = find_tool("QUENCH_OBJDUMP", &["llvm-objdump", "objdump"]);
    let args = if cfg!(target_os = "macos") {
        vec!["--macho", "--section=__text", "--full-contents", object.to_str().unwrap()]
    } else {
        vec!["-j", ".text", "-s", object.to_str().unwrap()]
    };
    let dump = output(Command::new(tool).args(args), "dump stencil text section");
    let bytes = parse_text_dump(&dump);
    fs::write(text, &bytes).expect("write extracted stencil text");
    bytes
}

fn parse_text_dump(dump: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in dump.lines() {
        let mut fields = line.split_whitespace();
        let Some(offset) = fields.next() else { continue };
        if offset.is_empty() || !offset.bytes().all(|byte| byte.is_ascii_hexdigit()) { continue }
        for field in fields {
            if field.len() < 2 || field.len() % 2 != 0 || !field.bytes().all(|byte| byte.is_ascii_hexdigit()) { break }
            for pair in field.as_bytes().chunks_exact(2) {
                bytes.push((hex(pair[0]) << 4) | hex(pair[1]));
            }
        }
    }
    assert!(!bytes.is_empty(), "objdump did not contain a .text dump: {dump}");
    bytes
}

fn hex(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("validated hexadecimal byte"),
    }
}

fn run(command: &mut Command, description: &str) {
    let status = command.status().unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}

fn output(command: &mut Command, description: &str) -> String {
    let output = command.output().unwrap_or_else(|error| panic!("{description} failed: {error}"));
    assert!(output.status.success(), "{description} exited with {}", output.status);
    String::from_utf8(output.stdout).expect("tool output is UTF-8")
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
