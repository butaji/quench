use super::{variant, NativeSupport, SourceManifest};
use quench_runtime::{NativeImageSupport, NativeTarget};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
};

const COMPILE_REPORT_VERSION: u64 = 1;
pub(super) const BOOTSTRAP_WORKER_OPTION: &str = "--bootstrap-worker";

/// The compile worker option that runs a native entry's residual program at build time to its
/// first wait for input and saves the run there (`build_rest`).
pub(super) const WAIT_WORKER_OPTION: &str = "--wait-worker";
#[cfg(unix)]
const PRIVATE_BUILD_DIRECTORY_MODE: u32 = 0o700;

const USAGE: &str = "quench compile ENTRY -o BINARY [--report FILE] [--profile experiment|release|iteration|aot-compact] [--sdk-report FILE] [--strict-aot] [--dynamic-source|--static-source] [--target-cpu x86-64-v2|x86-64-v3|armv8|armv8.2|native] [--target TRIPLE]";
const RETAINED_SUPPORT: &[&str] = &[
    "unpruned Wasm frontend dependencies",
    "unpruned Node host support",
];

#[derive(Clone, Copy)]
enum SourceSupport { Automatic, Dynamic, Static }

struct Options {
    entry: PathBuf,
    output: PathBuf,
    report: Option<PathBuf>,
    sdk_report: Option<PathBuf>,
    profile: String,
    strict: bool,
    support: SourceSupport,
    target: NativeTarget,
}

impl Options {
    fn parse(arguments: Vec<String>) -> Result<Self, String> {
        let mut arguments = arguments.into_iter();
        let mut entry = None;
        let mut output = None;
        let mut report = None;
        let mut sdk_report = None;
        let mut profile = None;
        let mut strict = false;
        let mut support = None;
        let mut target_cpu = None;
        let mut target_triple = None;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "-o" | "--output" | "--report" | "--profile" | "--sdk-report" | "--target-cpu" | "--target" => {
                    let value = arguments
                        .next()
                        .ok_or_else(|| format!("missing value for {argument}"))?;
                    match argument.as_str() {
                        "-o" | "--output" => {
                            if output.replace(PathBuf::from(value)).is_some() {
                                return Err("output specified twice".into());
                            }
                        }
                        "--report" => {
                            if report.replace(PathBuf::from(value)).is_some() {
                                return Err("report specified twice".into());
                            }
                        }
                        "--sdk-report" => {
                            if sdk_report.replace(PathBuf::from(value)).is_some() {
                                return Err("SDK report specified twice".into());
                            }
                        }
                        "--profile" => {
                            if profile.replace(value).is_some() {
                                return Err("profile specified twice".into());
                            }
                        }
                        "--target-cpu" => {
                            if target_cpu.replace(value).is_some() {
                                return Err("target CPU specified twice".into());
                            }
                        }
                        "--target" => {
                            if target_triple.replace(value).is_some() {
                                return Err("target specified twice".into());
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                "--strict-aot" => strict = true,
                "--dynamic-source" | "--static-source" => {
                    let selected = if argument == "--dynamic-source" { SourceSupport::Dynamic } else { SourceSupport::Static };
                    if support.replace(selected).is_some() { return Err("source support specified more than once".into()); }
                },
                option if option.starts_with('-') => {
                    return Err(format!("unknown compile option {option}\n{USAGE}"))
                }
                _ => {
                    if entry.replace(PathBuf::from(argument)).is_some() {
                        return Err(format!("multiple entry files\n{USAGE}"));
                    }
                }
            }
        }
        let profile = profile.unwrap_or_else(|| "release".to_owned());
        if !matches!(profile.as_str(), "experiment" | "release" | "iteration" | "aot-compact") {
            return Err("compile profile must be experiment, release, iteration or aot-compact".into());
        }
        Ok(Self {
            entry: entry.ok_or(USAGE)?,
            output: output.ok_or("compile requires -o BINARY")?,
            report,
            sdk_report,
            profile,
            strict,
            support: support.unwrap_or(SourceSupport::Automatic),
            target: NativeTarget::from_names(target_cpu.as_deref(), target_triple.as_deref())?,
        })
    }
}

pub fn run(arguments: Vec<String>) -> Result<ExitCode, String> {
    if matches!(arguments.as_slice(), [option] if option == "--help" || option == "-h") {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    enum Work {
        Application(Options),
        Bootstrap { program: PathBuf, output: PathBuf, builtins: Vec<String> },
        Wait { program: PathBuf, output: PathBuf, assumptions: Vec<String> },
        Evaluation { entry: PathBuf, journal: PathBuf, assumptions: Vec<String> },
        Resumption { entry: PathBuf, rest: PathBuf, arguments: Vec<String> },
    }
    let work = match arguments.as_slice() {
        [option, program, output, builtins @ ..] if option == BOOTSTRAP_WORKER_OPTION =>
            Work::Bootstrap { program: program.into(), output: output.into(), builtins: builtins.to_vec() },
        [option, program, output, assumptions @ ..] if option == WAIT_WORKER_OPTION =>
            Work::Wait { program: program.into(), output: output.into(), assumptions: assumptions.to_vec() },
        [option, entry, journal, assumptions @ ..] if option == super::strict::EVALUATION_WORKER_OPTION =>
            Work::Evaluation { entry: entry.into(), journal: journal.into(), assumptions: assumptions.to_vec() },
        [option, entry, rest, arguments @ ..] if option == super::strict::RESUMPTION_WORKER_OPTION =>
            Work::Resumption { entry: entry.into(), rest: rest.into(), arguments: arguments.to_vec() },
        _ => Work::Application(Options::parse(arguments)?),
    };
    std::thread::Builder::new()
        .name("quench-compile".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || match work {
            Work::Application(options) => compile(options),
            Work::Bootstrap { program, output, builtins } => build_bootstrap(&program, &output, builtins),
            Work::Wait { program, output, assumptions } => super::image::build_rest(&program, &output, &assumptions),
            Work::Evaluation { entry, journal, assumptions } => super::strict::evaluate(&entry, &journal, &assumptions),
            Work::Resumption { entry, rest, arguments } => super::strict::resume(&entry, &rest, arguments),
        })
        .map_err(|error| error.to_string())?
        .join()
        .unwrap_or_else(|_| Err("compiler worker panicked".into()))
        .map(|()| ExitCode::SUCCESS)
}

fn build_bootstrap(program: &Path, output: &Path, builtins: Vec<String>) -> Result<(), String> {
    if !crate::host::startup_snapshot::native_bootstrap_environment_is_neutral() {
        return Err("native bootstrap worker received process-specific configuration".into());
    }
    let program = quench_runtime::ResidualProgram::read_binary(program)?;
    let mut host = crate::NodeHost::new(vec!["quench".into()]).with_exec_argv(Vec::new())?;
    host.building_snapshot = Some(crate::host::startup_snapshot::SnapshotKind::Bootstrap);
    host.snapshot_builtins = builtins;
    let mut runtime = quench_runtime::Runtime::new(host);
    let bytes = runtime.build_snapshot(&program, |host, writer| host.write_snapshot(writer, false))?;
    // The host operations the startup heap holds, from which the image's table starts. No
    // opcodes: the entry's functions lower as they run, to opcodes their bytecode does not name
    // yet, so an image that runs its entry installs every lane row (`resumes_at_rest`).
    let (operations, lazy) = runtime.held_host_operations();
    let held: super::prefix::HeldOperations = (operations, lazy, Vec::new());
    fs::write(output.with_extension("operations"), serde_json::to_vec(&held).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let (programs, functions, instructions) = runtime.held_code_size();
    fs::write(output.with_extension("code"), json!({"programs": programs, "functions": functions, "instructions": instructions}).to_string())
        .map_err(|error| error.to_string())?;
    fs::write(output, bytes).map_err(|error| error.to_string())
}

/// The features a static image's support is built with: `native-execution`, plus the intl layer
/// and the WebAssembly frontend only where the program reaches them. A dynamic-source support
/// links the standard build's default features instead.
fn support_features(graph: &super::DependencyGraph, support: NativeImageSupport) -> String {
    if support.allows_dynamic_source() { return "default".into(); }
    let mut features = String::from("native-execution");
    if graph.needs_intl() { features.push_str(",intl"); }
    if graph.needs_wasm() { features.push_str(",wasm"); }
    features
}

fn compile(options: Options) -> Result<(), String> {
    let entry = options
        .entry
        .canonicalize()
        .map_err(|error| format!("{}: {error}", options.entry.display()))?;
    let output = absolute_output(&options.output)?;
    if output.is_dir() {
        return Err("executable destination is a directory".into());
    }
    if output == entry || output.canonicalize().ok().as_ref() == Some(&entry) {
        return Err("compiled output cannot replace its source entry".into());
    }
    let mut manifest = SourceManifest::default();
    let graph = manifest.add_module_graph(&entry)?;
    // The explicit choice wins; otherwise the runtime's source demands decide.
    let explicit = match options.support {
        SourceSupport::Dynamic => Some(NativeImageSupport::DynamicSource),
        SourceSupport::Static => Some(NativeImageSupport::Static),
        SourceSupport::Automatic => None,
    };
    // A strict compile that reads a dynamic input falls back to full support: static support
    // still retains the interpreter, which its link audit refuses (TOD-2196).
    let variant = if options.strict { variant::Variant::Full } else { variant::select(explicit, &graph) };
    let core = variant == variant::Variant::Core;
    let support = variant.image_support();
    let report_path = options.report.as_deref().map(absolute_output).transpose()?;
    if report_path.as_ref().is_some_and(|path| path.is_dir()) {
        return Err("report destination is a directory".into());
    }
    for destination in std::iter::once(&output).chain(report_path.iter()) {
        let canonical = destination
            .canonicalize()
            .unwrap_or_else(|_| destination.to_owned());
        if graph.files.contains_key(&canonical) {
            return Err("output/report cannot replace a compilation input".into());
        }
        if options
            .sdk_report
            .as_ref()
            .and_then(|path| path.canonicalize().ok())
            .as_ref()
            == Some(&canonical)
        {
            return Err("output/report cannot replace the support artifact report".into());
        }
    }
    if report_path.as_ref().is_some_and(|path| {
        path == &output || path.canonicalize().ok() == output.canonicalize().ok() && path.exists()
    }) {
        return Err("report and executable must have different destinations".into());
    }
    let mut report = json!({
        "schema_version": COMPILE_REPORT_VERSION, "entry": entry, "output": output,
        "execution_mode": if support.allows_dynamic_source() { "native_with_dynamic_source" } else { "native_only" }, "requested_support_profile": options.profile, "support_variant": variant.name(),
        "support_features": support_features(&graph, support),
        "payload_minimal": false, "retained_support": RETAINED_SUPPORT,
        "startup_options_binding_time": "runtime",
        "source_compilation": support.allows_dynamic_source(),
        "js_source_frontend": support.allows_dynamic_source(),
        "bytecode_interpreter": support.allows_dynamic_source(),
        "jit": support.allows_dynamic_source(),
        "dynamic_eval_sites": graph.dynamic_eval_sites.iter().map(|(from, offset)| json!({"from": from, "offset": offset})).collect::<Vec<_>>(),
        "dynamic_function_sites": graph.dynamic_function_sites.iter().map(|(from, offset)| json!({"from": from, "offset": offset})).collect::<Vec<_>>(),
        "dynamic_reflective_sites": graph.dynamic_reflective_sites.iter().map(|(from, offset)| json!({"from": from, "offset": offset})).collect::<Vec<_>>(),
        "source_support_selection": match options.support { SourceSupport::Automatic => "automatic", SourceSupport::Dynamic => "dynamic", SourceSupport::Static => "static" },
        "automatic_dynamic_source_coverage": "quench_runtime::SourceDemands: eval, Function-family constructor flows, constructor/Function/Reflect references and nonliteral member keys; other forms require explicit support",
        "files": graph.files.keys().collect::<Vec<_>>(),
        "requirements": graph.requirements.iter().map(super::GraphRequirement::report).collect::<Vec<_>>(),
    });
    if graph.requirements.iter().any(|requirement| variant::blocks(variant, requirement)) {
        report["status"] = json!("unsupported_requirements");
        write_report(report_path.as_deref(), &report)?;
        return Err(format!(
            "native compilation has unresolved requirements:\n{}",
            report["requirements"]
        ));
    }
    let work = BuildDirectory::new(output.parent().ok_or("output has no parent")?)?;
    // A strict compile links no support when the program folds at build time; one that reads
    // a dynamic input links its support variant, and the report names the input.
    let mut waits = Vec::new();
    if options.strict {
        match super::strict::compile(&entry, &output, &work.0, &mut report)? {
            super::strict::Outcome::Image => {
                report["status"] = json!("compiled");
                report["support_variant"] = json!("strict");
                report["execution_mode"] = json!("replay");
                write_report(report_path.as_deref(), &report)?;
                return Ok(());
            }
            super::strict::Outcome::Waits(runs) => waits = runs,
            super::strict::Outcome::Dynamic => {}
        }
    }
    super::image::capture_bootstrap(core, &graph, &mut manifest)?;
    report["source_units"] = json!(manifest.units().len());
    report["archived_module_sources"] = json!(manifest.assets().modules.len());
    report["static_import_resolutions"] = json!(manifest.assets().imports.len());
    report["sdk_source_views"] = json!(manifest.assets().views.len());
    let compiled = super::image::compile_sources(core, &graph, &entry, manifest)?;
    if core {
        report["builtin_families"] = json!(compiled.builtin_families().bits());
    }
    report["native_code_units"] = json!(compiled.units.len());
    report["deferred_syntax_errors"] = json!(compiled.syntax_errors.len());
    let images = work.0.join("images");
    let inputs = compiled.emit(&images, &options.target, super::compiler::ImageHost::of(core))?;
    // A program that loads no native addon and nothing at run time needs no dynamic interface.
    let inputs = if graph.requirements.iter().any(|requirement| requirement.is_dynamic_load() || matches!(requirement, super::GraphRequirement::NativeAddon { .. })) {
        inputs
    } else {
        inputs.self_contained()
    };
    report["self_contained"] = json!(inputs.is_self_contained());
    let native_entry = images.join(format!("{}.bin", super::compiler::NATIVE_ENTRY_UNIT));
    let (inputs, held, saved_waits) = super::image::stage_image(core, options.strict, &graph, &work.0, &native_entry, inputs, &waits, &mut report)?;
    let support_report = match &options.sdk_report {
        Some(path) => {
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?
        }
        None => {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().map_err(|error| format!("automatic support build requires the compiler checkout: {error}; use --sdk-report with existing support artifacts"))?;
            // The `experiment` profile takes nightly flags, as the standard build does (`RUSTC_BOOTSTRAP`).
            // A dynamic-source image links the standard build's library (`cargo build -p quench`, default
            // features), which that build left fresh: this cargo run only reports it. A static image's
            // support leaves out the JIT and the bundled sources, a build of its own.
            let mut cargo = Command::new("cargo");
            cargo.current_dir(&workspace).env("RUSTC_BOOTSTRAP", "1").args(["build", "--locked", "--profile", &options.profile, "-p", "quench"]);
            if !support.allows_dynamic_source() {
                let features = support_features(&graph, support);
                cargo.args(["--no-default-features", "--features", &features]);
            }
            cargo.args(["--lib", "--message-format=json"]);
            String::from_utf8(quench_runtime::native_link::checked_output(
                &mut cargo,
                "native execution support build",
            )?)
            .map_err(|error| error.to_string())?
        }
    };
    let support = NativeSupport::from_cargo_report_with_support(&support_report, support)?;
    support.check_build()?;
    report["support_artifacts"] = support.report();
    let staged = work.0.join("native-image");
    let audit = super::image::link_image(core, &support, &inputs, &work.0, &staged, &graph, held, &mut report)?;
    report["semantic_exports"] = json!(audit.semantic_exports());
    report["dynamic_interface"] = json!(audit.dynamic_interface);
    report["footprint_before_symbol_removal"] = json!(audit.footprint);
    let dumps = super::image::dump_saved_waits(&staged, &saved_waits, &work.0, &mut report);
    // `QUENCH_KEEP_SYMBOLS` keeps the symbol table, so a profiler or debugger names the runtime's functions.
    if std::env::var_os("QUENCH_KEEP_SYMBOLS").is_none() {
        let payload = quench_runtime::native_link::remove_offline_symbols(&staged)?;
        report["symbol_audit_stage"] = json!("before_symbol_removal");
        report["binary_bytes"] = json!(payload.binary_bytes);
        report["payload_reduction"] = json!(payload);
    }
    if !dumps.is_empty() {
        super::image_dump::append(&staged, &dumps)?;
    }
    report["native_dependencies"] = json!(super::image::native_dependencies(&staged)?);
    report["status"] = json!("compiled");
    // Publish only an audited executable. A failed compile leaves existing output intact.
    let report_build = report_path
        .as_ref()
        .map(|path| BuildDirectory::new(path.parent().ok_or("report has no parent")?))
        .transpose()?;
    if let Some(directory) = &report_build {
        write_report(Some(&directory.0.join("report.json")), &report)?;
    }
    fs::rename(&staged, &output).map_err(|error| error.to_string())?;
    if let (Some(directory), Some(path)) = (&report_build, report_path) {
        if path.canonicalize().ok().as_ref() == Some(&output) {
            return Err(format!(
                "native image published at {}; report aliases the executable and was not published",
                output.display()
            ));
        }
        fs::rename(directory.0.join("report.json"), &path).map_err(|error| {
            format!(
                "native image published at {}; report publication failed: {error}",
                output.display()
            )
        })?;
    }
    eprintln!("compiled {} -> {}", entry.display(), output.display());
    Ok(())
}

fn absolute_output(path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    let parent = path.parent().ok_or("output has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    Ok(parent
        .canonicalize()
        .map_err(|error| error.to_string())?
        .join(path.file_name().ok_or("output has no filename")?))
}

fn write_report(path: Option<&Path>, report: &Value) -> Result<(), String> {
    if let Some(path) = path {
        fs::write(
            path,
            serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

struct BuildDirectory(PathBuf);
impl BuildDirectory {
    fn new(parent: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = parent.join(format!(
                ".quench-compile-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg(unix)]
            let created = {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new()
                    .mode(PRIVATE_BUILD_DIRECTORY_MODE)
                    .create(&path)
            };
            #[cfg(not(unix))]
            let created = fs::create_dir(&path);
            match created {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}
impl Drop for BuildDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_or_incomplete_compile_commands_are_rejected() {
        for args in [
            vec![],
            vec!["entry.js"],
            vec!["entry.js", "-o"],
            vec!["entry.js", "-o", "a", "-o", "b"],
            vec!["entry.js", "other.js", "-o", "a"],
            vec!["entry.js", "-o", "a", "--profile", "unknown"],
            vec!["entry.js", "-o", "a", "--static-source", "--dynamic-source"],
        ] {
            assert!(Options::parse(args.into_iter().map(str::to_owned).collect()).is_err());
        }
    }

    #[test]
    fn unresolved_graphs_do_not_execute_the_entry_or_replace_output() {
        let directory = BuildDirectory::new(&std::env::temp_dir()).unwrap();
        let entry = directory.0.join("entry.cjs");
        let output = directory.0.join("program");
        let report = directory.0.join("report.json");
        let effect = directory.0.join("effect");
        fs::write(&entry, format!("require('node:fs').writeFileSync({:?}, 'executed'); const loader = require; loader(process.argv[2]);", effect.to_string_lossy())).unwrap();
        fs::write(&output, "previous artifact").unwrap();
        let result = compile(Options {
            entry,
            output: output.clone(),
            report: Some(report.clone()),
            sdk_report: None,
            profile: "iteration".into(),
            strict: false,
            support: SourceSupport::Static,
            target: NativeTarget::default(),
        });
        assert!(result.is_err());
        assert!(!effect.exists());
        assert_eq!(fs::read_to_string(output).unwrap(), "previous artifact");
        let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(report["status"], "unsupported_requirements");
        assert_eq!(report["requirements"][0]["kind"], "dynamic_require");
    }

    #[test]
    fn report_and_output_aliases_cannot_replace_source_or_each_other() {
        let directory = BuildDirectory::new(&std::env::temp_dir()).unwrap();
        let entry = directory.0.join("entry.cjs");
        let output = directory.0.join("program");
        fs::write(&entry, "throw 'must not execute';").unwrap();
        fs::write(&output, "previous artifact").unwrap();
        for (destination, report) in [
            (entry.clone(), None),
            (output.clone(), Some(entry.clone())),
            (output.clone(), Some(output.clone())),
        ] {
            assert!(compile(Options {
                entry: entry.clone(),
                output: destination,
                report,
                sdk_report: None,
                    profile: "iteration".into(),
                strict: false,
                support: SourceSupport::Static,
                target: NativeTarget::default(),
            })
            .is_err());
        }
        assert_eq!(
            fs::read_to_string(entry).unwrap(),
            "throw 'must not execute';"
        );
        assert_eq!(fs::read_to_string(output).unwrap(), "previous artifact");
    }
}
