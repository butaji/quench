use super::{NativeSupport, SourceManifest};
use quench_runtime::NativeImageSupport;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
};

const COMPILE_REPORT_VERSION: u64 = 1;
const BOOTSTRAP_WORKER_OPTION: &str = "--bootstrap-worker";
#[cfg(unix)]
const PRIVATE_BUILD_DIRECTORY_MODE: u32 = 0o700;

const USAGE: &str = "quench compile ENTRY -o BINARY [--report FILE] [--profile production|iteration] [--sdk-report FILE] [--strict-aot] [--dynamic-source|--static-source]";
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
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "-o" | "--output" | "--report" | "--profile" | "--sdk-report" => {
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
        let profile = profile.unwrap_or_else(|| "production".to_owned());
        if !matches!(profile.as_str(), "production" | "iteration") {
            return Err("compile profile must be production or iteration".into());
        }
        Ok(Self {
            entry: entry.ok_or(USAGE)?,
            output: output.ok_or("compile requires -o BINARY")?,
            report,
            sdk_report,
            profile,
            strict,
            support: support.unwrap_or(SourceSupport::Automatic),
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
        Bootstrap { program: PathBuf, output: PathBuf },
    }
    let work = match arguments.as_slice() {
        [option, program, output] if option == BOOTSTRAP_WORKER_OPTION =>
            Work::Bootstrap { program: program.into(), output: output.into() },
        _ => Work::Application(Options::parse(arguments)?),
    };
    std::thread::Builder::new()
        .name("quench-compile".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || match work {
            Work::Application(options) => compile(options),
            Work::Bootstrap { program, output } => build_bootstrap(&program, &output),
        })
        .map_err(|error| error.to_string())?
        .join()
        .unwrap_or_else(|_| Err("compiler worker panicked".into()))
        .map(|()| ExitCode::SUCCESS)
}

fn build_bootstrap(program: &Path, output: &Path) -> Result<(), String> {
    if !crate::host::startup_snapshot::native_bootstrap_environment_is_neutral() {
        return Err("native bootstrap worker received process-specific configuration".into());
    }
    let program = quench_runtime::ResidualProgram::read_binary(program)?;
    let mut host = crate::NodeHost::new(vec!["quench".into()]).with_exec_argv(Vec::new())?;
    host.building_snapshot = Some(crate::host::startup_snapshot::SnapshotKind::Bootstrap);
    let mut runtime = quench_runtime::Runtime::new(host);
    let bytes = runtime.build_snapshot(&program, |host, writer| host.write_snapshot(writer))?;
    fs::write(output, bytes).map_err(|error| error.to_string())
}

fn compile(options: Options) -> Result<(), String> {
    if options.strict {
        return Err(format!(
            "strict AOT is not yet available: retained support: {}",
            RETAINED_SUPPORT.join(", ")
        ));
    }
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
    // Both eval references and known constructor flows may receive runtime
    // source. Other dynamic-source forms still require explicit support until
    // source-capability closure is complete.
    let support = match options.support {
        SourceSupport::Dynamic => NativeImageSupport::DynamicSource,
        SourceSupport::Static => NativeImageSupport::Static,
        SourceSupport::Automatic if !graph.dynamic_eval_sites.is_empty() || !graph.dynamic_function_sites.is_empty() => NativeImageSupport::DynamicSource,
        SourceSupport::Automatic => NativeImageSupport::Static,
    };
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
        "execution_mode": if support.allows_dynamic_source() { "native_with_dynamic_source" } else { "native_only" }, "requested_support_profile": options.profile,
        "payload_minimal": false, "retained_support": RETAINED_SUPPORT,
        "startup_options_binding_time": "runtime",
        "source_compilation": support.allows_dynamic_source(),
        "js_source_frontend": support.allows_dynamic_source(),
        "bytecode_interpreter": support.allows_dynamic_source(),
        "jit": support.allows_dynamic_source(),
        "dynamic_eval_sites": graph.dynamic_eval_sites.iter().map(|(from, offset)| json!({"from": from, "offset": offset})).collect::<Vec<_>>(),
        "dynamic_function_sites": graph.dynamic_function_sites.iter().map(|(from, offset)| json!({"from": from, "offset": offset})).collect::<Vec<_>>(),
        "source_support_selection": match options.support { SourceSupport::Automatic => "automatic", SourceSupport::Dynamic => "dynamic", SourceSupport::Static => "static" },
        "automatic_dynamic_source_coverage": "unshadowed eval references and discovered Function-constructor calls with nonliteral source; other forms require explicit support",
        "files": graph.files.keys().collect::<Vec<_>>(),
        "requirements": graph.requirements.iter().map(super::GraphRequirement::report).collect::<Vec<_>>(),
    });
    if !graph.requirements.is_empty() {
        report["status"] = json!("unsupported_requirements");
        write_report(report_path.as_deref(), &report)?;
        return Err(format!(
            "native compilation has unresolved requirements:\n{}",
            report["requirements"]
        ));
    }
    let work = BuildDirectory::new(output.parent().ok_or("output has no parent")?)?;
    manifest.extend(SourceManifest::capture_for_graph(&graph)?)?;
    report["source_units"] = json!(manifest.units().len());
    report["archived_module_sources"] = json!(manifest.assets().modules.len());
    report["static_import_resolutions"] = json!(manifest.assets().imports.len());
    report["sdk_source_views"] = json!(manifest.assets().views.len());
    let compiled = manifest.compile()?;
    report["native_code_units"] = json!(compiled.units.len());
    report["deferred_syntax_errors"] = json!(compiled.syntax_errors.len());
    let images = work.0.join("images");
    let inputs = compiled.emit(&images)?;
    let bootstrap = work.0.join("bootstrap.bin");
    let mut worker = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    worker.args(["compile", BOOTSTRAP_WORKER_OPTION])
        .arg(images.join(format!("{}.bin", super::compiler::NATIVE_ENTRY_UNIT)))
        .arg(&bootstrap);
    crate::host::startup_snapshot::neutralize_native_bootstrap_environment(&mut worker);
    let inputs = match quench_runtime::native_link::checked_output(&mut worker, "native bootstrap preparation") {
        Ok(_) => {
            let mut catalog = fs::OpenOptions::new().append(true).open(inputs.catalog()).map_err(|error| error.to_string())?;
            writeln!(catalog, "const _: () = assert!(quench_node::aot::BOOTSTRAP_ABI == {:#x}, \"native bootstrap host ABI mismatch\");", super::BOOTSTRAP_ABI)
                .map_err(|error| error.to_string())?;
            report["bootstrap"] = json!({"status": "prepared", "bytes": fs::metadata(&bootstrap).map_err(|error| error.to_string())?.len(),
                "configuration": "neutral", "namespace": "native entry atom IDs preserved", "host_abi": format!("{:#x}", super::BOOTSTRAP_ABI)});
            inputs.with_bootstrap(bootstrap)
        },
        Err(error) => {
            report["bootstrap"] = json!({"status": "unavailable", "diagnostic": error});
            inputs
        },
    };
    let support_report = match &options.sdk_report {
        Some(path) => {
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?
        }
        None => {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().map_err(|error| format!("automatic support build requires the compiler checkout: {error}; use --sdk-report with existing support artifacts"))?;
            let mut cargo = Command::new("cargo");
            cargo.current_dir(&workspace).args([
                "build",
                "--locked",
                "--profile",
                &options.profile,
                "-p",
                "quench-node",
                "--no-default-features",
                "--features",
                if support.allows_dynamic_source() { "native-execution,jit" } else { "native-execution" },
                "--lib",
                "--message-format=json",
            ]);
            String::from_utf8(quench_runtime::native_link::checked_output(
                &mut cargo,
                "native execution support build",
            )?)
            .map_err(|error| error.to_string())?
        }
    };
    let support = NativeSupport::from_cargo_report_with_support(&support_report, support)?;
    report["support_artifacts"] = support.report();
    let driver = work.0.join("driver.rs");
    fs::write(&driver, include_str!("image_driver.rs")).map_err(|error| error.to_string())?;
    let staged = work.0.join("native-image");
    let audit = support.link_audited(&inputs, &driver, &staged)?;
    report["semantic_exports"] = json!(audit.semantic_exports());
    report["dynamic_interface"] = json!(audit.dynamic_interface);
    report["footprint_before_symbol_removal"] = json!(audit.footprint);
    let payload = quench_runtime::native_link::remove_offline_symbols(&staged)?;
    report["symbol_audit_stage"] = json!("before_symbol_removal");
    report["binary_bytes"] = json!(payload.binary_bytes);
    report["payload_reduction"] = json!(payload);
    report["native_dependencies"] = json!(native_dependencies(&staged)?);
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

fn native_dependencies(executable: &Path) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    let output = quench_runtime::native_link::checked_output(
        Command::new("otool").arg("-L").arg(executable),
        "native dependency audit",
    )?;
    #[cfg(target_os = "linux")]
    let output = quench_runtime::native_link::checked_output(
        Command::new("readelf").arg("-d").arg(executable),
        "native dependency audit",
    )?;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = executable;
        return Err("native image dependency auditing supports macOS and Linux".into());
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    String::from_utf8(output).map_err(|error| error.to_string())
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
