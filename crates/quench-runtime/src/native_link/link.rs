use super::NativeImageInputs;
use crate::NativeImageSupport;
use serde_json::Value;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// One validated link, before offline symbol removal.
#[derive(Serialize)]
pub struct NativeLinkAudit {
    pub dynamic_interface: super::exports::DynamicInterface,
}

impl NativeLinkAudit {
    /// The semantic helper interface is a projection of the validated exports.
    pub fn semantic_exports(&self) -> BTreeSet<String> {
        self.dynamic_interface.linked.iter().filter(|name| name.starts_with(crate::NATIVE_JS_OPERATION_PREFIX)).cloned().collect()
    }
}

/// Cargo's exact selected support artifacts and their native search paths.
pub struct NativeSupport {
    host: PathBuf,
    host_crate: String,
    runtime: PathBuf,
    native_paths: BTreeSet<PathBuf>,
    optimization: String,
    rust_lto: bool,
    support: NativeImageSupport,
    host_semantic_exports: bool,
}

const NATIVE_SDK_FEATURE: &str = "native-sdk";

impl NativeSupport {
    pub fn from_cargo_report(report: &str, host_crate: &str, support: NativeImageSupport) -> Result<Self, String> {
        let mut host = None;
        let mut runtime = None;
        let mut native_paths = BTreeSet::new();
        let mut optimization = None;
        let mut completed = false;
        let mut host_semantic_exports = false;
        for line in report.lines() {
            let artifact: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
            match artifact["reason"].as_str() {
                Some("compiler-artifact") => {
                    let name = artifact["target"]["name"]
                        .as_str()
                        .ok_or("Cargo artifact has no target name")?;
                    let features = artifact["features"]
                        .as_array()
                        .ok_or("Cargo artifact has no feature list")?;
                    if matches!(
                        name,
                        "cranelift_object" | "cranelift_module"
                    ) || (name == "cranelift_native" && !support.allows_dynamic_source()) {
                        return Err(format!(
                            "native support includes compiler dependency {name}"
                        ));
                    }
                    if !support.allows_dynamic_source() && name == "cranelift_codegen"
                        && features.iter().any(|feature| {
                            matches!(
                                feature.as_str(),
                                Some(
                                    "host-arch"
                                        | "all-arch"
                                        | "all-native-arch"
                                        | "x86"
                                        | "arm64"
                                        | "riscv64"
                                        | "s390x"
                                        | "pulley"
                                )
                            )
                        })
                    {
                        return Err("native support includes an architecture code generator".into());
                    }
                    if name == host_crate || name == "quench_runtime" {
                        if !features.iter().any(|feature| feature == "native-execution")
                            || features.iter().any(|feature| {
                                matches!(feature.as_str(), Some("aot" | "bundled-sources"))
                                    || (!support.allows_dynamic_source() && feature == "jit")
                            })
                        {
                            return Err(format!("{name} is not valid {support:?} native support"));
                        }
                        if support.allows_dynamic_source() && !features.iter().any(|feature| feature == "jit") {
                            return Err(format!("{name} has no JIT support for dynamic source"));
                        }
                        let files = artifact["filenames"]
                            .as_array()
                            .ok_or("Cargo artifact has no files")?;
                        let file = files
                            .iter()
                            .filter_map(Value::as_str)
                            .find(|file| {
                                Path::new(file)
                                    .extension()
                                    .is_some_and(|extension| extension == "rlib")
                            })
                            .ok_or_else(|| format!("Cargo did not emit {name}'s rlib"))?;
                        let file = exact_dependency(Path::new(file), name)?;
                        if name == host_crate {
                            host_semantic_exports = features.iter().any(|feature|feature == NATIVE_SDK_FEATURE);
                            if host.replace(file).is_some() {
                                return Err("ambiguous host support artifacts".into());
                            }
                            optimization = Some(
                                artifact["profile"]["opt_level"]
                                    .as_str()
                                    .ok_or("Cargo artifact has no optimization level")?
                                    .to_owned(),
                            );
                        } else if runtime.replace(file).is_some() {
                            return Err("ambiguous runtime support artifacts".into());
                        }
                    }
                }
                Some("build-finished") => {
                    if artifact["success"] != true {
                        return Err("support build did not complete successfully".into());
                    }
                    completed = true;
                }
                Some("build-script-executed") => {
                    for path in artifact["linked_paths"]
                        .as_array()
                        .ok_or("Cargo build script has no native search paths")?
                    {
                        native_paths.insert(PathBuf::from(
                            path.as_str().ok_or("invalid Cargo native search path")?,
                        ));
                    }
                }
                _ => {}
            }
        }
        if !completed {
            return Err("Cargo support report is incomplete".into());
        }
        let host = host.ok_or_else(|| format!("Cargo report has no {host_crate} support library"))?;
        let rust_lto = requires_rust_lto(&host)?;
        Ok(Self {
            host,
            host_crate: host_crate.to_owned(),
            support,
            rust_lto,
            host_semantic_exports,
            runtime: runtime.ok_or("Cargo report has no runtime support library")?,
            native_paths,
            optimization: optimization.ok_or("Cargo report has no support optimization level")?,
        })
    }

    pub fn report(&self) -> Value {
        serde_json::json!({ "host_library": self.host, "host_crate": self.host_crate, "runtime_library": self.runtime,
            "optimization_level": self.optimization, "native_search_paths": self.native_paths,
            "driver_panic_strategy": "abort", "rust_lto": self.rust_lto,
            "dynamic_source": self.support.allows_dynamic_source(), "host_semantic_exports": self.host_semantic_exports })
    }

    /// The SDK already owns the opcode-derived C ABI helpers for its host.
    pub fn provides_semantic_exports(&self) -> bool { self.host_semantic_exports }

    pub fn link(
        &self,
        inputs: &NativeImageInputs,
        driver: &Path,
        executable: &Path,
    ) -> Result<BTreeSet<String>, String> {
        self.link_audited(inputs, driver, executable).map(|audit| audit.semantic_exports())
    }

    pub fn link_audited(
        &self, inputs: &NativeImageInputs, driver: &Path, executable: &Path,
    ) -> Result<NativeLinkAudit, String> {
        let mut command = Command::new("rustc");
        command
            .args([
                "--edition=2024",
                "--crate-name=quench_native_image",
                "-C",
                "debuginfo=0",
                "-C",
                "panic=abort",
            ])
            .arg("-C")
            .arg(format!("opt-level={}", self.optimization))
            .arg(driver)
            .arg("--extern")
            .arg(format!("{}={}", self.host_crate, self.host.display()))
            .arg("--extern")
            .arg(format!("quench_runtime={}", self.runtime.display()))
            .arg("-L")
            .arg(format!(
                "dependency={}",
                self.host
                    .parent()
                    .ok_or("support library has no parent")?
                    .display()
            ))
            .arg("-L")
            .arg(format!(
                "dependency={}",
                self.runtime
                    .parent()
                    .ok_or("runtime library has no parent")?
                    .display()
            ))
            .arg("-o")
            .arg(executable)
            .env("QUENCH_NATIVE_CATALOG", inputs.catalog());
        if self.support.allows_dynamic_source() {
            command.args(["--cfg", "quench_native_dynamic_source"]);
        }
        if self.rust_lto {
            command.args(["-C", "lto=thin"]);
        }
        for path in &self.native_paths {
            command.arg("-L").arg(path);
        }
        for object in inputs.objects() {
            command
                .arg("-C")
                .arg(format!("link-arg={}", object.display()));
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let map = {
            let mut path = executable.as_os_str().to_owned();
            path.push(".link-map");
            PathBuf::from(path)
        };
        #[cfg(target_os = "macos")]
        command
            .args([
                "-C",
                "link-arg=-Xlinker",
                "-C",
                "link-arg=-map",
                "-C",
                "link-arg=-Xlinker",
            ])
            .arg("-C")
            .arg(format!("link-arg={}", map.display()));
        #[cfg(target_os = "linux")]
        command
            .args([
                "-C",
                "link-arg=-Xlinker",
                "-C",
                "link-arg=-Map",
                "-C",
                "link-arg=-Xlinker",
            ])
            .arg("-C")
            .arg(format!("link-arg={}", map.display()));
        let interface = super::exports::configure(&mut command, inputs, executable)?;
        checked_output(&mut command, "native image link")?;
        audit(inputs, executable, self.support)?;
        let dynamic_interface = interface.verify(executable)?;
        Ok(NativeLinkAudit { dynamic_interface })
    }
}

// LLVM's raw and wrapper bitcode signatures (BitCodeFormat.html). Detect
// actual archive contents instead of inferring LTO from a profile's name.
const LLVM_RAW_BITCODE_MAGIC: [u8; size_of::<u32>()] = *b"BC\xc0\xde";
const LLVM_WRAPPED_BITCODE_MAGIC: [u8; size_of::<u32>()] = 0x0B17_C0DEu32.to_le_bytes();

fn requires_rust_lto(library: &Path) -> Result<bool, String> {
    use std::io::Read;
    let members = checked_output(
        Command::new("ar").arg("t").arg(library),
        "support archive inspection",
    )?;
    let members = String::from_utf8(members).map_err(|error| error.to_string())?;
    let member = members
        .lines()
        .find(|name| name.ends_with(".rcgu.o"))
        .ok_or("support archive has no Rust code object")?;
    let mut child = Command::new("ar")
        .arg("p")
        .arg(library)
        .arg(member)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut body = child.stdout.take().ok_or("archive reader has no output")?;
    let mut magic = [0; size_of::<u32>()];
    let header = body.read_exact(&mut magic);
    // Drain rather than allocating a potentially large bitcode member.
    let drained = std::io::copy(&mut body, &mut std::io::sink());
    let result = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "support archive inspection: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    header.map_err(|error| error.to_string())?;
    drained.map_err(|error| error.to_string())?;
    Ok(matches!(
        magic,
        LLVM_RAW_BITCODE_MAGIC | LLVM_WRAPPED_BITCODE_MAGIC
    ))
}

fn exact_dependency(file: &Path, name: &str) -> Result<PathBuf, String> {
    let parent = file.parent().ok_or("Cargo rlib has no parent")?;
    if parent.file_name().is_some_and(|name| name == "deps") {
        fs::metadata(file).map_err(|error| error.to_string())?;
        return Ok(file.to_owned());
    }
    // Cargo's top-level copy can be replaced by the producer build. Resolve it
    // immediately to the exact dependency artifact, comparing bytes, not hashes.
    let bytes = fs::read(file).map_err(|error| error.to_string())?;
    let mut matches = Vec::new();
    for entry in fs::read_dir(parent.join("deps")).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path
            .file_name()
            .is_some_and(|file| file.to_string_lossy().starts_with(&format!("lib{name}-")))
            && path
                .extension()
                .is_some_and(|extension| extension == "rlib")
            && fs::metadata(&path)
                .map_err(|error| error.to_string())?
                .len()
                == bytes.len() as u64
            && fs::read(&path).map_err(|error| error.to_string())? == bytes
        {
            matches.push(path);
        }
    }
    if matches.len() != 1 {
        return Err(format!(
            "cannot identify one exact {name} dependency artifact"
        ));
    }
    Ok(matches.remove(0))
}

pub fn checked_output(command: &mut Command, operation: &str) -> Result<Vec<u8>, String> {
    let output = command
        .output()
        .map_err(|error| format!("{operation}: {error}"))?;
    if !output.status.success() {
        let diagnostics = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let message: Value = serde_json::from_str(line).ok()?;
                message["message"]["rendered"].as_str().map(str::to_owned)
            })
            .collect::<String>();
        return Err(format!(
            "{operation} failed ({}):\n{}{}",
            output.status,
            diagnostics,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

const BACKEND_COMPILER_SYMBOLS: &[&str] = &[
    "cranelift_native", "cranelift_object", "cranelift_module", "target_isa_for",
];
const JS_SOURCE_COMPILER_SYMBOLS: &[&str] = &[
    "FunctionCompiler", "specialize_host_script", "specialize_with_mode",
    "specialize_module_with_atom_prefix", "specialize_eval_with_context",
    "specialize_function_constructor",
];
const INTERPRETER_DISPATCH_SYMBOLS: &[&str] = &[
    "::run_fast_lane", "::derive_lane_view", "::js_step_callout",
    "::step_wasm", "::js_tier_frame_entry",
];

fn is_interpreter_symbol(symbol: &str) -> bool {
    INTERPRETER_DISPATCH_SYMBOLS.iter().any(|name| symbol.contains(name))
}

fn is_compiler_symbol(symbol: &str) -> bool {
    BACKEND_COMPILER_SYMBOLS.iter().any(|name| symbol.contains(name))
        || ["oxc_parser::", "oxc_semantic::"].iter().any(|name| symbol.contains(name))
        || (symbol.contains("quench_runtime") && JS_SOURCE_COMPILER_SYMBOLS.iter().any(|name| symbol.contains(name)))
}

fn audit(inputs: &NativeImageInputs, executable: &Path, support: NativeImageSupport) -> Result<(), String> {
    let bytes = checked_output(Command::new("nm").arg("-C").arg(executable), "native symbol audit")?;
    let symbols = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut exports = BTreeSet::new();
    for line in symbols.lines() {
        // Demangled names omit the instantiating crate: a core generic first
        // emitted by OXC can be required without retaining any OXC operation.
        // Inspect the full line because Rust types may contain spaces.
        if !support.allows_dynamic_source() && (is_compiler_symbol(line) || is_interpreter_symbol(line)) {
            return Err(format!("native image retained compilation or interpretation support: {line}"));
        }
        let mut fields = line.split_whitespace().rev();
        let Some(symbol) = fields.next() else {
            continue;
        };
        let symbol = symbol.trim_start_matches('_');
        if fields.next() == Some("T")
            && symbol.starts_with(crate::NATIVE_JS_OPERATION_PREFIX)
        {
            exports.insert(symbol.to_owned());
        }
    }
    let expected: BTreeSet<_> = fs::read_to_string(inputs.required_exports())
        .map_err(|error| error.to_string())?
        .lines()
        .filter(|name| name.starts_with(crate::NATIVE_JS_OPERATION_PREFIX))
        .map(str::to_owned)
        .collect();
    if exports != expected {
        return Err("native semantic helper exports do not match object imports".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreter_audit_preserves_native_frame_coordination_and_shared_validation() {
        for symbol in [
            "<quench_runtime::vm::Vm<Host>>::run_fast_lane",
            "quench_runtime::vm::dispatch_fast::derive_lane_view::<Host>",
            "quench_runtime::vm::compiled::js_step_callout::<Host>",
            "<quench_runtime::vm::Vm<Host>>::step_wasm",
            "<quench_runtime::vm::Vm<Host>>::js_tier_frame_entry",
        ] {
            assert!(is_interpreter_symbol(symbol), "{symbol}");
        }
        for symbol in [
            "<quench_runtime::vm::Vm<Host>>::run_general_loop::<false>",
            "<quench_runtime::vm::Vm<Host>>::step_operation_74",
            "quench_runtime::vm::dispatch_fast::lane_fields_fit",
        ] {
            assert!(!is_interpreter_symbol(symbol), "{symbol}");
        }
    }

    #[test]
    fn compiler_audit_distinguishes_source_lowering_from_execution_analysis() {
        for symbol in [
            "quench_runtime::compile::Engine::specialize_host_script",
            "quench_runtime::compile::ast::FunctionCompiler::compile",
            "quench_runtime::compile::Engine::specialize_module_with_atom_prefix",
            "cranelift_native::target_isa_for",
            "0000000100000000 T <oxc_parser::Parser>::parse",
            "0000000100000000 T <oxc_semantic::SemanticBuilder as Trait>::build",
        ] {
            assert!(is_compiler_symbol(symbol), "{symbol}");
        }
        for symbol in [
            "quench_runtime::compile::liveness::register_roots",
            "quench_runtime::compile::regexp::validate_pattern_text",
            "quench_runtime::compile::program_profile::ProgramFootprint",
            "quench_node::specialize_host_script",
            "<core::ptr::drop_glue<alloc::vec::Vec<u64>>>",
            "_RAND_bytes",
        ] {
            assert!(!is_compiler_symbol(symbol), "{symbol}");
        }
    }

    #[test]
    fn support_reports_reject_dynamic_backends_and_the_full_sdk_bundle() {
        for (name, features) in [
            ("quench_node", vec!["native-execution", "bundled-sources"]),
            ("quench_runtime", vec!["native-execution", "jit"]),
            ("quench_node", vec!["native-execution", "aot"]),
            ("cranelift_codegen", vec!["std", "arm64"]),
            ("cranelift_native", vec![]),
        ] {
            let report = serde_json::json!({"reason": "compiler-artifact", "target": {"name": name}, "features": features}).to_string();
            let error = NativeSupport::from_cargo_report(&report, "quench_node", NativeImageSupport::Static).err().expect("unsupported SDK accepted");
            assert!(error.contains("native support") || error.contains("compiler dependency") || error.contains("architecture code generator"), "{error}");
        }
    }

    #[test]
    fn dynamic_source_support_requires_jit_and_excludes_build_time_compilers() {
        for (name, features, reason) in [
            ("quench_node", vec!["native-execution"], "no JIT"),
            ("quench_runtime", vec!["native-execution"], "no JIT"),
            ("quench_node", vec!["native-execution", "jit", "bundled-sources"], "native support"),
            ("quench_runtime", vec!["native-execution", "jit", "aot"], "native support"),
            ("cranelift_object", vec![], "compiler dependency"),
            ("cranelift_module", vec![], "compiler dependency"),
        ] {
            let report = serde_json::json!({"reason":"compiler-artifact","target":{"name":name},"features":features}).to_string();
            let error = NativeSupport::from_cargo_report(&report, "quench_node", NativeImageSupport::DynamicSource)
                .err().expect("incompatible hybrid SDK accepted");
            assert!(error.contains(reason), "{name}: {error}");
        }
    }

    #[test]
    fn unsuccessful_and_incomplete_support_build_reports_are_rejected() {
        assert!(NativeSupport::from_cargo_report("{\"reason\":\"build-finished\",\"success\":false}", "quench_node", NativeImageSupport::Static).err().unwrap().contains("successfully"));
        assert!(NativeSupport::from_cargo_report("", "quench_node", NativeImageSupport::Static).err().unwrap().contains("incomplete"));
    }
}
