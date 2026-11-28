pub(in crate::native_link) mod audit;
use audit::audit;

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
    pub footprint: super::NativeImageFootprint,
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
    /// The engine layer's library (`quench_runtime_engine`), which names the build (`check_build`).
    engine: Option<PathBuf>,
    /// The layer facade (`quench-layers`), which a core image names (`extern crate quench_layers`)
    /// so its `extern "Rust"` engine, intl and stencil symbols link.
    layers: Option<PathBuf>,
    /// The allocator (`quench-alloc`), which a core image names as its global allocator.
    alloc: Option<PathBuf>,
    /// The standard library a minimal static support built itself (`-Zbuild-std`, `aot/cli.rs`), which the driver
    /// links in place of the sysroot's; `None` where the support links the sysroot's.
    built_std: Option<PathBuf>,
    native_paths: BTreeSet<PathBuf>,
    optimization: String,
    rust_lto: bool,
    support: NativeImageSupport,
    host_semantic_exports: bool,
    /// The host's executable link arguments (kind, function order), applied to each image.
    executable_link_args: &'static [&'static str],
}

const NATIVE_SDK_FEATURE: &str = "native-sdk";

/// The linker script fragment that drops a self-contained image's unwind and landing-pad
/// tables (`.eh_frame`, `.gcc_except_table`), inserted into the linker's default layout.
#[cfg(target_os = "linux")]
const DISCARD_UNWIND_TABLES: &str =
    "SECTIONS { /DISCARD/ : { *(.eh_frame) *(.eh_frame.*) *(.gcc_except_table) *(.gcc_except_table.*) } } INSERT AFTER .text;\n";

impl NativeSupport {
    pub fn from_cargo_report(report: &str, host_crate: &str, support: NativeImageSupport) -> Result<Self, String> {
        let mut host = None;
        let mut runtime = None;
        let mut engine = None;
        let mut layers = None;
        let mut alloc = None;
        let mut built_std = None;
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
                    // The runtime always depends on Cranelift (AOT code generation is unconditional, and
                    // Cargo unifies its architecture features across the workspace), so a static image's
                    // support may carry the code generator in its rlibs; the link's symbol audit
                    // (`audit`, `BACKEND_COMPILER_SYMBOLS`) checks that none of it reaches the image.
                    let _ = features;
                    // A dynamic-source image links the standard build's artifacts (default features:
                    // `bundled-sources`, `jit`); a static image's support excludes both, and its link's
                    // symbol audit checks that the compiler is dropped.
                    if name == "std" {
                        built_std = artifact["filenames"].as_array().into_iter().flatten().filter_map(Value::as_str)
                            .find(|file| file.ends_with(".rlib")).map(PathBuf::from);
                    }
                    if let Some(library) = layer_library(&artifact, name)? {
                        match name {
                            "quench_runtime_engine" => engine = Some(library),
                            "quench_layers" => layers = Some(library),
                            _ => alloc = Some(library),
                        }
                    }
                    if name == host_crate || name == "quench_runtime" {
                        if !features.iter().any(|feature| feature == "native-execution")
                            || (!support.allows_dynamic_source() && features.iter().any(|feature| {
                                matches!(feature.as_str(), Some("aot" | "bundled-sources" | "jit"))
                            }))
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
                        let file = exact_dependency(Path::new(file), name, files)?;
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
            engine,
            layers,
            alloc,
            built_std,
            native_paths,
            optimization: optimization.ok_or("Cargo report has no support optimization level")?, executable_link_args: &[],
        })
    }

    /// Fails unless the engine library is the build this compiler is (`AHEAD_BUILD_ID_MARK`, which
    /// names the engine and the core under it): an image this compiler lowers runs only on the
    /// build it was lowered for, so a library rebuilt from a changed tree would link an executable
    /// that refuses its own images.
    pub fn check_build(&self) -> Result<(), String> {
        unsafe extern "Rust" {
            safe fn quench_engine_build_id() -> u64;
        }
        let library = self.engine.as_ref().ok_or("Cargo report has no engine support library")?;
        let bytes = std::fs::read(library).map_err(|error| format!("{}: {error}", library.display()))?;
        let prefix = crate::vm::AHEAD_BUILD_ID_PREFIX;
        let id = bytes.windows(32).find(|window| window.starts_with(prefix) && window[16..].iter().all(u8::is_ascii_hexdigit))
            .map(|window| String::from_utf8_lossy(&window[16..]).into_owned())
            .ok_or_else(|| format!("{} names no quench build", library.display()))?;
        let own = format!("{:016x}", quench_engine_build_id());
        if id != own {
            return Err(format!("the engine library {} is build {id}, but this compiler is build {own}: the tree changed \
                after this compiler was built; rebuild it (cargo build -p quench) and compile again", library.display()));
        }
        Ok(())
    }

    /// Link images as the host binary links: no dynamic relocations dirtying pages at startup,
    /// and the trained order packing the functions a run executes.
    pub fn with_executable_link_args(mut self, arguments: &'static [&'static str]) -> Self { self.executable_link_args = arguments; self }

    pub fn report(&self) -> Value {
        serde_json::json!({ "host_library": self.host, "host_crate": self.host_crate, "runtime_library": self.runtime,
            "optimization_level": self.optimization, "native_search_paths": self.native_paths,
            "driver_panic_strategy": "abort", "rust_lto": self.rust_lto,
            "dynamic_source": self.support.allows_dynamic_source(), "host_semantic_exports": self.host_semantic_exports, "executable_link_args": self.executable_link_args })
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
        self.link_audited_with(inputs, driver, executable, true)
    }

    /// Link a core image, which runs on `quench_runtime`'s own host: the driver's `--extern`s
    /// name the runtime alone, so no Node host library and none of its dependencies can reach
    /// the executable.
    pub fn link_audited_core(
        &self, inputs: &NativeImageInputs, driver: &Path, executable: &Path,
    ) -> Result<NativeLinkAudit, String> {
        self.link_audited_with(inputs, driver, executable, false)
    }

    fn link_audited_with(
        &self, inputs: &NativeImageInputs, driver: &Path, executable: &Path, node_host: bool,
    ) -> Result<NativeLinkAudit, String> {
        let mut command = self.base_command(driver)?;
        self.add_externs(&mut command, node_host)?;
        command.arg("-o").arg(executable).env("QUENCH_NATIVE_CATALOG", inputs.catalog());
        self.apply_link_options(&mut command, inputs, executable)?;
        self.add_objects(&mut command, inputs, executable);
        let interface = super::exports::configure(&mut command, inputs, executable)?;
        checked_link(&mut command)?;
        let symbols = audit(inputs, executable, self.support)?;
        let footprint = super::footprint::read(executable, &symbols)?;
        let dynamic_interface = interface.verify(executable)?;
        Ok(NativeLinkAudit { dynamic_interface, footprint })
    }

    /// `rustc` with the compiler-owned crate identity and the profile's optimization.
    fn base_command(&self, driver: &Path) -> Result<Command, String> {
        let mut command = Command::new("rustc"); // The rustc that built the rlibs: their checkout's pin.
        command.current_dir(self.host.parent().ok_or("support library has no parent")?)
            .args(["--edition=2024", "--crate-name=quench_native_image", "-C", "debuginfo=0", "-C", "panic=abort"])
            .arg("-C").arg(format!("opt-level={}", self.optimization))
            // `QUENCH_IMAGE_RUSTC_ARGS` (whitespace-separated) adds driver options, such as
            // `-C target-feature=+crt-static`, to measure link variants of an image.
            .args(std::env::var("QUENCH_IMAGE_RUSTC_ARGS").unwrap_or_default().split_whitespace())
            .arg(driver);
        Ok(command)
    }

    /// The runtime, and for a Node image its host; a core image names the shared layer facade
    /// instead, so the engine, intl and stencil symbols of its `extern "Rust"` calls link.
    fn add_externs(&self, command: &mut Command, node_host: bool) -> Result<(), String> {
        let parent = |path: &Path, what: &str| path.parent().ok_or_else(|| format!("{what} has no parent")).map(Path::to_path_buf);
        command.arg("--extern").arg(format!("quench_runtime={}", self.runtime.display()))
            .arg("-L").arg(format!("dependency={}", parent(&self.runtime, "runtime library")?.display()));
        // A minimal static support built its own standard library (`-Zbuild-std`, `aot/cli.rs`), with the
        // `immediate-abort` panic strategy: the driver links that `std`, not the sysroot's, under the same strategy.
        if let Some(std) = &self.built_std {
            command.env("RUSTC_BOOTSTRAP", "1")
                .args(["-Z", "unstable-options", "-C", "panic=immediate-abort"])
                .arg("--extern").arg(format!("noprelude:std={}", std.display()));
        }
        if node_host {
            command.arg("--extern").arg(format!("{}={}", self.host_crate, self.host.display()))
                .arg("-L").arg(format!("dependency={}", parent(&self.host, "support library")?.display()));
        } else {
            let layers = self.layers.as_ref().ok_or("Cargo report has no layer support library")?;
            command.arg("--extern").arg(format!("quench_layers={}", layers.display()))
                .arg("-L").arg(format!("dependency={}", parent(layers, "layer library")?.display()));
            // The runtime's own allocator: a core image's global allocator.
            let alloc = self.alloc.as_ref().ok_or("Cargo report has no allocator support library")?;
            command.arg("--extern").arg(format!("quench_alloc={}", alloc.display()))
                .arg("-L").arg(format!("dependency={}", parent(alloc, "allocator library")?.display()));
        }
        Ok(())
    }

    /// Dynamic-source, bootstrap, LTO and platform link flags, then the support's native paths
    /// and the host's own executable link arguments.
    fn apply_link_options(&self, command: &mut Command, inputs: &NativeImageInputs, executable: &Path) -> Result<(), String> {
        if self.support.allows_dynamic_source() {
            command.args(["--cfg", "quench_native_dynamic_source"]);
        }
        if let Some(bootstrap) = &inputs.bootstrap {
            command.args(["--cfg", "quench_native_bootstrap"]).env("QUENCH_NATIVE_BOOTSTRAP", bootstrap);
        }
        if self.rust_lto {
            command.args(["-C", "lto=fat"]);
        }
        self.apply_platform_flags(command, inputs, executable)?;
        for path in &self.native_paths {
            command.arg("-L").arg(path);
        }
        command.args(self.executable_link_args.iter().flat_map(|argument| ["-C".to_owned(), format!("link-arg={argument}")]));
        Ok(())
    }

    /// The platform's link flags: static C runtime and unwind hooks on Linux, chained fixups and
    /// dead dylib stripping on macOS.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    fn apply_platform_flags(&self, command: &mut Command, inputs: &NativeImageInputs, executable: &Path) -> Result<(), String> {
        // A self-contained image links glibc statically and position-dependent.
        #[cfg(target_os = "linux")]
        if inputs.is_self_contained() {
            command.args(["-C", "target-feature=+crt-static", "-C", "relocation-model=static"]);
            // The static C runtime's start code registers `.eh_frame` with the unwinder when
            // these weak hooks resolve, and the first unwind then classifies every FDE (8.6M
            // instructions in a readline REPL image). Null hooks leave the unwinder on
            // `PT_GNU_EH_FRAME`'s sorted table, as the shipping static build does (`link_layout.rs`).
            for hook in ["__register_frame_info", "__deregister_frame_info"] {
                command.arg("-C").arg(format!("link-arg=-Wl,--defsym={hook}=0"));
            }
            // An image aborts on panic, so nothing in it unwinds: its unwind tables and
            // landing-pad tables (5% of a readline REPL image) are discarded at link time.
            let mut script = executable.as_os_str().to_owned();
            script.push(".discard-unwind.ld");
            let script = PathBuf::from(script);
            std::fs::write(&script, DISCARD_UNWIND_TABLES).map_err(|error| format!("{}: {error}", script.display()))?;
            command.arg("-C").arg("link-arg=-Wl,--no-eh-frame-hdr")
                .arg("-C").arg(format!("link-arg=-Wl,-T,{}", script.display()));
            apply_static_layout_flags(command);
        }
        // Apple Silicon's macOS floor already supports chained fixups. Request them explicitly:
        // rustc's default deployment target otherwise selects classic relocations, dirtying every
        // pointer-bearing constant page at launch. Newer kernels can apply chains on page-in
        // instead. Leave the deployment target and older x86-64 platform behavior unchanged.
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        command.args(["-C", "link-arg=-Wl,-fixup_chains"]);
        // Record only the shared libraries the image references: the support archives' native
        // dependencies (`openssl-sys` names `-lssl -lcrypto` for every link) otherwise become
        // load commands even when the program can reach no host operation that uses them.
        #[cfg(target_os = "macos")]
        command.args(["-C", "link-arg=-Wl,-dead_strip_dylibs"]);
        #[cfg(target_os = "linux")]
        command.args(["-C", "link-arg=-Wl,--as-needed"]);
        Ok(())
    }

    /// The image objects, the linker's map, and `QUENCH_EXTRA_LINK_ARGS` diagnostics.
    fn add_objects(&self, command: &mut Command, inputs: &NativeImageInputs, executable: &Path) {
        for object in inputs.objects() {
            command.arg("-C").arg(format!("link-arg={}", object.display()));
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let map = {
            let mut path = executable.as_os_str().to_owned();
            path.push(".link-map");
            PathBuf::from(path)
        };
        // The linker's map (ld64 spells the option `-map`, GNU-style linkers `-Map`).
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        command.args(["-C", "link-arg=-Xlinker", "-C", if cfg!(target_os = "macos") { "link-arg=-map" } else { "link-arg=-Map" },
            "-C", "link-arg=-Xlinker", "-C"]).arg(format!("link-arg={}", map.display()));
        // `QUENCH_EXTRA_LINK_ARGS` (whitespace-separated linker driver arguments, such as
        // `-Wl,--why-live=SYMBOL`) diagnoses what keeps code in an image: in every link, or in
        // the `QUENCH_EXTRA_LINK_ROUND`th alone, counted from 1, of a compile that links again
        // until the operations it keeps settle.
        static LINKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let link = LINKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let round = std::env::var("QUENCH_EXTRA_LINK_ROUND").ok().and_then(|round| round.parse::<usize>().ok());
        if let Some(extra) = std::env::var_os("QUENCH_EXTRA_LINK_ARGS").filter(|_| round.is_none_or(|round| round == link)) {
            for argument in extra.to_string_lossy().split_whitespace() {
                command.arg("-C").arg(format!("link-arg={argument}"));
            }
        }
    }
}

/// The static image's section and code layout: unreferenced sections are collected, and identical
/// functions fold. Folding (`--icf`) is an lld feature, so the link uses lld when it is on the
/// path. `-z norelro` drops the RELRO page: a static non-PIE image has no dynamic relocations
/// for it to protect.
#[cfg(target_os = "linux")]
fn apply_static_layout_flags(command: &mut Command) {
    command.args(["-C", "link-arg=-Wl,--gc-sections", "-C", "link-arg=-Wl,-z,norelro"]);
    if lld_on_path() {
        command.args(["-C", "link-arg=-fuse-ld=lld", "-C", "link-arg=-Wl,--icf=all"]);
    }
}

#[cfg(target_os = "linux")]
fn lld_on_path() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("ld.lld").is_file()))
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

/// The rlib of a reported layer crate (`quench_runtime_engine`, `quench_layers`,
/// `quench_alloc`), selected by Cargo's own artifact identity; none for any other crate.
fn layer_library(artifact: &Value, name: &str) -> Result<Option<PathBuf>, String> {
    if !matches!(name, "quench_runtime_engine" | "quench_layers" | "quench_alloc") {
        return Ok(None);
    }
    let files = artifact["filenames"].as_array().ok_or("Cargo artifact has no files")?;
    let file = files.iter().filter_map(Value::as_str)
        .find(|file| Path::new(file).extension().is_some_and(|extension| extension == "rlib"));
    let Some(file) = file else { return Ok(None); };
    exact_dependency(Path::new(file), name, files).map(Some)
}

fn exact_dependency(file: &Path, name: &str, reported_files: &[Value]) -> Result<PathBuf, String> {
    let parent = file.parent().ok_or("Cargo rlib has no parent")?;
    if parent.file_name().is_some_and(|name| name == "deps") {
        fs::metadata(file).map_err(|error| error.to_string())?;
        return Ok(file.to_owned());
    }
    // Cargo reports the hashed metadata sibling of a top-level library. Use
    // that identity, which survives another feature build replacing the alias.
    let prefix = format!("lib{name}-");
    let mut reported = reported_files.iter().filter_map(Value::as_str).map(Path::new).filter(|path| {
        path.parent().is_some_and(|parent| parent.file_name().is_some_and(|part| part == "deps"))
            && path.file_name().is_some_and(|file| file.to_string_lossy().starts_with(&prefix))
            && path.extension().is_some_and(|extension| extension == "rmeta")
    });
    if let Some(metadata) = reported.next() {
        if reported.next().is_some() {
            return Err(format!("ambiguous {name} dependency metadata"));
        }
        let library = metadata.with_extension("rlib");
        fs::metadata(&library).map_err(|error| format!("{}: {error}", library.display()))?;
        return Ok(library);
    }
    // Cargo's top-level copy can be replaced by the producer build. Resolve it
    // immediately to the exact dependency artifact, comparing bytes, not hashes.
    let bytes = fs::read(file).map_err(|error| error.to_string())?;
    let mut matches = Vec::new();
    for entry in fs::read_dir(parent.join("deps")).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path
            .file_name()
            .is_some_and(|file| file.to_string_lossy().starts_with(&prefix))
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

/// The image link's driver. A successful link's stderr reaches this process's stderr, so the
/// linker's reports (a `QUENCH_EXTRA_LINK_ARGS` `--why-live` chain) are not discarded.
fn checked_link(command: &mut Command) -> Result<(), String> {
    let output = command.output().map_err(|error| format!("native image link: {error}"))?;
    if !output.status.success() {
        return Err(format!("native image link failed ({}):\n{}", output.status, String::from_utf8_lossy(&output.stderr)));
    }
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::audit::{is_compiler_symbol, is_interpreter_symbol, retained_execution_support};

    #[test]
    fn retention_report_includes_each_mechanism_once() {
        let symbols = "T quench_runtime::compile::ast::FunctionCompiler::first\n\
            T quench_runtime::compile::ast::FunctionCompiler::second\n\
            T oxc_parser::Parser::parse\n\
            T quench_runtime::vm::compiled::js_step_callout\n\
            T cranelift_native::builder\n\
            T core::option::Option<oxc_allocator::Allocator>::unwrap";
        let retained = retained_execution_support(symbols);
        assert_eq!(retained.len(), 4);
        assert!(retained["FunctionCompiler"].ends_with("::first"));
        assert!(retained.contains_key("oxc_parser::"));
        assert!(retained.contains_key("::js_step_callout"));
        assert!(retained.contains_key("cranelift_native"));
    }

    #[test]
    fn interpreter_audit_preserves_native_frame_coordination_and_shared_validation() {
        for symbol in [
            "<quench_runtime::vm::Vm>::run_fast_lane",
            "quench_runtime::vm::dispatch_fast::derive_lane_view::<Host>",
            "quench_runtime::vm::compiled::js_step_callout::<Host>",
            "<quench_runtime::vm::Vm>::step_wasm",
            "<quench_runtime::vm::Vm>::js_tier_frame_entry",
        ] {
            assert!(is_interpreter_symbol(symbol), "{symbol}");
        }
        for symbol in [
            "<quench_runtime::vm::Vm>::run_general_loop::<false>",
            "<quench_runtime::vm::Vm>::step_operation_74",
            "quench_runtime::vm::dispatch_fast::fields_fit_in_lane",
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
            "quench::specialize_host_script",
            "<core::ptr::drop_glue<alloc::vec::Vec<u64>>>",
            "_RAND_bytes",
        ] {
            assert!(!is_compiler_symbol(symbol), "{symbol}");
        }
    }

    #[test]
    fn support_reports_reject_the_jit_and_the_full_sdk_bundle() {
        for (name, features) in [
            ("quench", vec!["native-execution", "bundled-sources"]),
            ("quench_runtime", vec!["native-execution", "jit"]),
            ("quench", vec!["native-execution", "aot"]),
        ] {
            let report = serde_json::json!({"reason": "compiler-artifact", "target": {"name": name}, "features": features}).to_string();
            let error = NativeSupport::from_cargo_report(&report, "quench", NativeImageSupport::Static).err().expect("unsupported SDK accepted");
            assert!(error.contains("native support") || error.contains("compiler dependency") || error.contains("architecture code generator"), "{error}");
        }
    }

    #[test]
    fn dynamic_source_support_requires_jit() {
        for (name, features, reason) in [
            ("quench", vec!["native-execution"], "no JIT"),
            ("quench_runtime", vec!["native-execution"], "no JIT"),
        ] {
            let report = serde_json::json!({"reason":"compiler-artifact","target":{"name":name},"features":features}).to_string();
            let error = NativeSupport::from_cargo_report(&report, "quench", NativeImageSupport::DynamicSource)
                .err().expect("incompatible hybrid SDK accepted");
            assert!(error.contains(reason), "{name}: {error}");
        }
    }

    #[test]
    fn unsuccessful_and_incomplete_support_build_reports_are_rejected() {
        assert!(NativeSupport::from_cargo_report("{\"reason\":\"build-finished\",\"success\":false}", "quench", NativeImageSupport::Static).err().unwrap().contains("successfully"));
        assert!(NativeSupport::from_cargo_report("", "quench", NativeImageSupport::Static).err().unwrap().contains("incomplete"));
    }

    #[test]
    fn support_report_keeps_the_reported_library_after_alias_replacement() {
        let root = std::env::temp_dir().join(format!("quench-sdk-artifact-{}", std::process::id()));
        let deps = root.join("deps");
        fs::create_dir_all(&deps).unwrap();
        let alias = root.join("libexample.rlib");
        let library = deps.join("libexample-selected.rlib");
        let metadata = deps.join("libexample-selected.rmeta");
        fs::write(&alias, b"different feature build").unwrap();
        fs::write(&library, b"selected feature build").unwrap();
        let files = vec![serde_json::json!(alias), serde_json::json!(metadata)];
        assert_eq!(exact_dependency(&alias, "example", &files).unwrap(), library);
        fs::remove_file(&library).unwrap();
        assert!(exact_dependency(&alias, "example", &files).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

