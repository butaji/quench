use super::{variant, NativeSupport, SourceManifest};
use quench_runtime::{NativeImageSupport, NativeTarget};
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

/// The compile worker option that runs a native entry's residual program at build time to its
/// first wait for input and saves the run there (`build_rest`).
const WAIT_WORKER_OPTION: &str = "--wait-worker";
#[cfg(unix)]
const PRIVATE_BUILD_DIRECTORY_MODE: u32 = 0o700;

const USAGE: &str = "quench compile ENTRY -o BINARY [--report FILE] [--profile experiment|release|iteration] [--sdk-report FILE] [--strict-aot] [--dynamic-source|--static-source] [--target-cpu x86-64-v2|x86-64-v3|armv8|armv8.2|native] [--target TRIPLE]";
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
        if !matches!(profile.as_str(), "experiment" | "release" | "iteration") {
            return Err("compile profile must be experiment, release or iteration".into());
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
            Work::Wait { program, output, assumptions } => build_rest(&program, &output, &assumptions),
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
    // The host operations the startup heap holds, from which the image's table starts.
    let held: (Vec<u32>, Vec<(Vec<String>, Vec<u32>)>) = runtime.held_host_operations();
    fs::write(output.with_extension("operations"), serde_json::to_vec(&held).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    fs::write(output, bytes).map_err(|error| error.to_string())
}

/// The worker: runs the native entry's residual `program` with the build-time gate armed
/// (`aot::prefix`), under `assumptions` (`QUESTION=0|1`), to its first wait for input, and writes the run saved there to `output` and
/// the writes it made before to `output` with the extension `effects` (JSON pairs of descriptor
/// and bytes). Fails when the run does not reach such a wait, having read a dynamic input.
fn build_rest(program: &Path, output: &Path, assumptions: &[String]) -> Result<(), String> {
    if !crate::host::startup_snapshot::native_bootstrap_environment_is_neutral() {
        return Err("native wait worker received process-specific configuration".into());
    }
    let program = quench_runtime::ResidualProgram::read_binary(program)?;
    super::prefix::begin(super::strict::parse_assumptions(assumptions)?);
    let host = crate::NodeHost::new(vec!["quench".into()]).with_exec_argv(Vec::new())?;
    let mut runtime = crate::shared_run::EntryRuntime::new(quench_runtime::Runtime::new(host), crate::shared_run::Teardown::Free);
    let execution = runtime.execute_deferred_jobs(&program);
    let completion = crate::shared_run::finish_shared_execution(&mut runtime, &program, execution);
    match super::prefix::finish() {
        super::prefix::Residual::Waiting(effects, rest, held) => {
            fs::write(output, rest).map_err(|error| error.to_string())?;
            fs::write(output.with_extension("operations"), serde_json::to_vec(&held).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
            fs::write(output.with_extension("effects"), serde_json::to_vec(&effects).map_err(|error| error.to_string())?).map_err(|error| error.to_string())
        }
        super::prefix::Residual::Dynamic(input) => Err(format!("the native entry's build-time run read {input} before its first wait")),
        super::prefix::Residual::Question(question) => Err(format!("the native entry's build-time run asked {} before its first wait", question.key())),
        super::prefix::Residual::Effects(_) => Err(format!("the native entry's build-time run did not wait for input ({:?})", completion.err())),
    }
}

/// A run saved at its first wait for input: the answers it assumed, the writes it made before
/// the wait, and the saved runtime's file.
struct SavedWait {
    answers: super::strict::Answers,
    effects: Vec<(i32, Vec<u8>)>,
    rest: PathBuf,
    /// The host operations the saved runtime holds.
    held: super::prefix::HeldOperations,
}

/// Saves, per answer set of `runs`, the native entry's run at its first wait (`--wait-worker`);
/// none when any run cannot be saved, which `report` names.
fn prepare_saved_waits(runs: &[super::strict::Answers], work: &Path, native_entry: &Path, report: &mut Value) -> Vec<SavedWait> {
    let mut saved = Vec::new();
    for (index, answers) in runs.iter().enumerate() {
        let rest = work.join(format!("rest-{index}.bin"));
        let prepared = std::env::current_exe().map_err(|error| error.to_string()).and_then(|compiler| {
            let mut worker = Command::new(compiler);
            worker.args(["compile", WAIT_WORKER_OPTION]).arg(native_entry).arg(&rest)
                .args(answers.iter().map(|(question, answer)| format!("{}={}", question.key(), u8::from(*answer))));
            crate::host::startup_snapshot::neutralize_native_bootstrap_environment(&mut worker);
            quench_runtime::native_link::checked_output(&mut worker, "native wait preparation")?;
            let read = |extension: &str| fs::read(rest.with_extension(extension)).map_err(|error| error.to_string());
            let effects = serde_json::from_slice(&read("effects")?).map_err(|error| error.to_string())?;
            let held = serde_json::from_slice(&read("operations")?).map_err(|error| error.to_string())?;
            Ok((effects, held))
        });
        match prepared {
            Ok((effects, held)) => saved.push(SavedWait { answers: answers.clone(), effects, rest, held }),
            Err(error) => {
                report["resume"] = json!({"status": "unavailable", "diagnostic": error});
                return Vec::new();
            }
        }
    }
    if !saved.is_empty() {
        report["resume"] = json!({"status": "prepared", "runs": saved.iter().map(|wait| json!({
            "answers": wait.answers.iter().map(|(question, answer)| (question.key(), *answer)).collect::<Vec<_>>(),
            "writes": wait.effects.len(), "bytes": fs::metadata(&wait.rest).map_or(0, |metadata| metadata.len()),
        })).collect::<Vec<_>>()});
    }
    saved
}

/// The catalog's `RESUMES_AT_REST` (`image_driver`): each saved run, which an image whose
/// answers hold as it starts resumes.
fn resumes_at_rest(saved: &[SavedWait]) -> String {
    let runs = saved.iter().map(|wait| {
        let answers = wait.answers.iter().map(|(question, answer)| format!("({:?}, {answer})", question.key())).collect::<Vec<_>>().join(", ");
        let writes = wait.effects.iter().map(|(descriptor, bytes)| format!("({descriptor}, &{bytes:?})")).collect::<Vec<_>>().join(", ");
        format!("quench::aot::SavedWait {{ answers: &[{answers}], effects: &[{writes}], rest: include_bytes!({:?}) }}", wait.rest.display().to_string())
    }).collect::<Vec<_>>().join(", ");
    // The runs answer every yes-or-no input the build-time evaluation asked, each both ways
    // (`strict::specialize`), so one always holds: an image with saved runs links no other start.
    // An image that only resumes them installs lane handlers for the opcodes their programs
    // hold, any other running on the general path; one that also runs its entry, every one.
    let mut opcodes: Vec<u16> = saved.iter().flat_map(|wait| wait.held.2.iter().copied()).collect();
    opcodes.sort_unstable();
    opcodes.dedup();
    let group = crate::LANE_ROW_GROUP;
    let rows = opcodes.iter().map(|op| format!("&quench::lane_rows::g{}::R{}", op / group, op % group)).collect::<Vec<_>>().join(", ");
    let lanes = if saved.is_empty() { "quench::install_all_lane_rows()".to_owned() } else { format!("quench::install_lane_rows(&[{rows}])") };
    format!("static RESUMES_AT_REST: &[quench::aot::SavedWait] = &[{runs}];\nconst RESUME_ONLY: bool = {};\nfn install_lanes() {{ {lanes} }}\n", !saved.is_empty())
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
    manifest.extend(SourceManifest::capture_for_graph(&graph)?)?;
    report["source_units"] = json!(manifest.units().len());
    report["archived_module_sources"] = json!(manifest.assets().modules.len());
    report["static_import_resolutions"] = json!(manifest.assets().imports.len());
    report["sdk_source_views"] = json!(manifest.assets().views.len());
    let compiled = manifest.compile()?;
    report["native_code_units"] = json!(compiled.units.len());
    report["deferred_syntax_errors"] = json!(compiled.syntax_errors.len());
    let images = work.0.join("images");
    let inputs = compiled.emit(&images, &options.target)?;
    // A program that loads no native addon and nothing at run time needs no dynamic interface.
    let inputs = if graph.requirements.iter().any(|requirement| requirement.is_dynamic_load() || matches!(requirement, super::GraphRequirement::NativeAddon { .. })) {
        inputs
    } else {
        inputs.self_contained()
    };
    report["self_contained"] = json!(inputs.is_self_contained());
    let native_entry = images.join(format!("{}.bin", super::compiler::NATIVE_ENTRY_UNIT));
    // A program whose build-time runs wait for input before reading any resumes the run whose
    // answers hold as it starts: no entry run and no installation, so no startup snapshot.
    let saved_waits = prepare_saved_waits(&waits, &work.0, &native_entry, &mut report);
    fs::OpenOptions::new().append(true).open(inputs.catalog())
        .and_then(|mut catalog| catalog.write_all(format!("{}const MAKES_CODE: bool = {};\nconst RUNS_WASM: bool = {};\n", resumes_at_rest(&saved_waits), makes_code(&graph), runs_wasm(&graph)).as_bytes())).map_err(|error| error.to_string())?;
    let (inputs, startup_held) = if saved_waits.is_empty() { bootstrap_inputs(&graph, &work.0, &native_entry, inputs, &mut report)? } else { (inputs, None) };
    // The heaps the image starts from: each saved run's, else the startup snapshot's.
    let heaps: Vec<(Vec<u32>, Vec<(Vec<String>, Vec<u32>)>)> = match startup_held {
        Some(held) => vec![held],
        None => saved_waits.iter().map(|wait| (wait.held.0.clone(), wait.held.1.clone())).collect(),
    };
    // A strict image whose entry reads a process input runs it from the startup snapshot alone.
    fs::OpenOptions::new().append(true).open(inputs.catalog())
        .and_then(|mut catalog| writeln!(catalog, "const RESTORED: bool = {};", options.strict && saved_waits.is_empty() && !heaps.is_empty())).map_err(|error| error.to_string())?;
    // An image that only resumes saved runs starts from the operations their heaps hold, and
    // the builders of the builtins its program may name only as it runs (`require`, a lazy
    // global's module): every one the graph names.
    let held: Option<std::collections::BTreeSet<u32>> = (!heaps.is_empty()).then(|| {
        // What the global object holds only for globals no loaded code names (a pending lazy
        // global's initializer, an accessor's getter) runs only when code reads one of those
        // names: code the program holds names it, code it loads may, and so may a computed read
        // of the global object, which code reaches by an alias's name or a sloppy call's `this`.
        let names_any = loads_code(&graph) || graph.script_reads_this
            || graph.builtins.iter().any(|name| name.trim_start_matches("node:") == "vm")
            || super::graph::GLOBAL_OBJECT_NAMES.iter().any(|alias| graph.global_names.contains(*alias));
        // A compression stream builds `zlib` by name (`commonjs::zlib_module_named`): kept where
        // the program can reach the class, by its global or `stream/web`.
        let compresses = names_any || ["CompressionStream", "DecompressionStream"].iter().any(|name| graph.global_names.contains(*name))
            || graph.builtins.iter().chain(&graph.esm_builtins).any(|name| name.trim_start_matches("node:") == "stream/web");
        let named = graph.snapshot_builtins().into_iter().chain(crate::modules::util_lazy_shared_vm::lazy_global_modules().map(str::to_owned))
            .chain(compresses.then(|| "zlib".to_owned()));
        let builders = named.filter_map(|name| crate::host::shared_vm::commonjs::builtin_operation_of(&name)).map(|operation| operation.0);
        report["unnamed_globals"] = json!(heaps.iter().flat_map(|heap| &heap.1).map(|(names, operations)| json!({
            "names": names, "operations": operations.len(),
            "kept": names_any || names.iter().any(|name| graph.global_names.contains(name)),
        })).collect::<Vec<_>>());
        let lazy = heaps.iter().flat_map(|heap| &heap.1)
            .filter(|(names, _)| names_any || names.iter().any(|name| graph.global_names.contains(name)))
            .flat_map(|(_, operations)| operations.iter().copied());
        // The internal bindings it may load as it runs: those it names, or every one where it
        // reads a binding loader otherwise.
        let bindings: Vec<_> = if graph.dynamic_bindings {
            crate::host::shared_vm::commonjs::binding_operations().map(|operation| operation.0).collect()
        } else {
            graph.bindings.iter().filter_map(|name| crate::modules::trace_shared_vm::binding_operation(name)).map(|operation| operation.0).collect()
        };
        heaps.iter().flat_map(|heap| heap.0.iter().copied()).chain(lazy).chain(builders).chain(bindings).collect()
    });
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
                cargo.args(["--no-default-features", "--features", "native-execution"]);
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
    let driver = work.0.join("driver.rs");
    fs::write(&driver, include_str!("image_driver.rs")).map_err(|error| error.to_string())?;
    let staged = work.0.join("native-image");
    let audit = link_with_reached_operations(&support, &inputs, &driver, &staged, &graph, held, &mut report)?;
    report["semantic_exports"] = json!(audit.semantic_exports());
    report["dynamic_interface"] = json!(audit.dynamic_interface);
    report["footprint_before_symbol_removal"] = json!(audit.footprint);
    // `QUENCH_KEEP_SYMBOLS` keeps the symbol table, so a profiler or debugger names the runtime's functions.
    if std::env::var_os("QUENCH_KEEP_SYMBOLS").is_none() {
        let payload = quench_runtime::native_link::remove_offline_symbols(&staged)?;
        report["symbol_audit_stage"] = json!("before_symbol_removal");
        report["binary_bytes"] = json!(payload.binary_bytes);
        report["payload_reduction"] = json!(payload);
    }
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

/// Links the image with the host operations it reaches. Its table starts from `held`, the
/// operations the saved runtimes' heaps hold, and gains each operation the linked code marks
/// as one it makes (`shared_vm::MarkedOperation`), linking again until the marks name nothing
/// new: a closure the linker computes over the code it keeps. Code names an operation only
/// by its mark, never by a name it computes, so the closure is whole. Without saved runs, or
/// for a program that loads code as it runs, the table keeps every operation.
fn link_with_reached_operations(
    support: &NativeSupport, inputs: &quench_runtime::native_link::NativeImageInputs, driver: &Path, staged: &Path,
    graph: &super::graph::DependencyGraph, held: Option<std::collections::BTreeSet<u32>>, report: &mut Value,
) -> Result<quench_runtime::native_link::NativeLinkAudit, String> {
    let catalog = fs::read_to_string(inputs.catalog()).map_err(|error| error.to_string())?;
    let mut kept = held.filter(|_| !loads_code(graph));
    report["held_host_operations"] = json!(kept.as_ref().map(std::collections::BTreeSet::len));
    let mut links = 0;
    // The operations each link's marks added, by name.
    let mut rounds: Vec<Vec<&'static str>> = Vec::new();
    loop {
        let (table, unlinked) = operation_table(graph, kept.as_ref());
        fs::write(inputs.catalog(), format!("{catalog}{table}")).map_err(|error| error.to_string())?;
        let audit = support.link_audited(inputs, driver, staged)?;
        links += 1;
        let Some(operations) = &mut kept else {
            report["unlinked_host_operations"] = json!(unlinked);
            report["operation_links"] = json!(links);
            return Ok(audit);
        };
        let marks = quench_runtime::native_link::marked_words(staged, "QUENCH_OPERATION_MARK")?;
        let added: Vec<u32> = marks.into_iter().filter(|mark| !operations.contains(mark)).collect();
        rounds.push(added.iter().map(|&operation| crate::host::shared_vm::commonjs::operation_name(operation)).collect::<Vec<_>>());
        report["operation_rounds"] = json!(rounds);
        operations.extend(added);
        if rounds.last().is_some_and(Vec::is_empty) {
            report["unlinked_host_operations"] = json!(unlinked);
            report["operation_links"] = json!(links);
            return Ok(audit);
        }
    }
}

/// Whether the program may load code as it runs, which may name any module.
fn loads_code(graph: &super::graph::DependencyGraph) -> bool {
    graph.requirements.iter().any(|requirement| requirement.is_dynamic_load() || matches!(requirement, super::GraphRequirement::NativeAddon { .. }))
        || !graph.dynamic_eval_sites.is_empty() || !graph.dynamic_function_sites.is_empty()
}

/// Whether the program can make code as it runs, so its image links the source frontend
/// (`quench::host::enable_dynamic_source`): it loads code (`loads_code`), evaluates an
/// `import()`, names eval, `Function` or `Reflect`, imports a module that compiles source, or
/// reads the global object by a computed name. An image of any other program refuses code
/// generation from strings, as Node does under `--disallow-code-generation-from-strings`.
fn makes_code(graph: &super::graph::DependencyGraph) -> bool {
    const COMPILING_MODULES: &[&str] = &["vm", "repl", "module", "worker_threads", "inspector"];
    const CODE_NAMES: &[&str] = &["eval", "Function", "Reflect", "WebAssembly", "Worker", "ShadowRealm"];
    loads_code(graph) || graph.imports_at_run_time || graph.script_reads_this
        || graph.builtins.iter().chain(&graph.esm_builtins).any(|name| COMPILING_MODULES.contains(&name.trim_start_matches("node:")))
        || CODE_NAMES.iter().chain(super::graph::GLOBAL_OBJECT_NAMES).any(|name| graph.global_names.contains(*name))
}

/// Whether the program can reach WebAssembly, so its image links the Wasm frontend
/// (`quench::host::enable_wasm`): it can make code (`makes_code`, which a `WebAssembly` name
/// implies) or imports a `.wasm` module.
fn runs_wasm(graph: &super::graph::DependencyGraph) -> bool {
    makes_code(graph) || graph.requirements.iter().any(|requirement| matches!(requirement, super::GraphRequirement::Wasm { .. }))
}

/// The modules whose host operations an image links only when its program imports them by
/// name: each holds a heavy dependency (`node:sqlite` the SQL engine).
const IMPORTED_OPERATION_FAMILIES: &[(&str, &str)] = &[("sqlite", "sqlite_shared_vm")];

/// The image's host operation table (`NodeHost::with_operations`), with the number of
/// operations it leaves unlinked: the operations `kept`, or else every operation except those
/// of a family whose module the program cannot import. A program that loads anything at run
/// time keeps them all.
fn operation_table(graph: &super::graph::DependencyGraph, kept: Option<&std::collections::BTreeSet<u32>>) -> (String, usize) {
    let dynamic = loads_code(graph) || !graph.dynamic_reflective_sites.is_empty();
    let builtins = graph.snapshot_builtins();
    let excluded: Vec<&str> = IMPORTED_OPERATION_FAMILIES.iter()
        .filter(|(builtin, _)| !dynamic && !builtins.iter().any(|name| name == builtin))
        .map(|(_, module)| *module).collect();
    let paths = crate::host::OPERATION_CALLBACK_PATHS;
    let mut unlinked = 0;
    let entries = paths.iter().enumerate().map(|(index, path)| {
        let linked = match kept {
            Some(kept) => kept.contains(&(index as u32)),
            None => !excluded.iter().any(|module| path.contains(module)),
        };
        if linked {
            format!("quench::host::OPERATION_CALLBACKS[{index}]")
        } else {
            unlinked += 1;
            "quench::host::unlinked_operation".to_owned()
        }
    }).collect::<Vec<_>>().join(", ");
    let table = format!("static OPERATIONS: [quench_runtime::HostCallback; {n}] = [{entries}];\n\
        const _: () = assert!(quench::host::OPERATION_CALLBACKS.len() == {n}, \"native image host operation table mismatch\");\n", n = paths.len());
    (table, unlinked)
}

/// The image's inputs with its startup snapshot (`--bootstrap-worker`), or without one when the
/// snapshot cannot be prepared; the catalog gains the start's constants either way.
/// Also the host operations the snapshot's heap holds (`build_bootstrap`).
fn bootstrap_inputs(graph: &super::graph::DependencyGraph, work: &Path, native_entry: &Path, inputs: quench_runtime::native_link::NativeImageInputs, report: &mut Value)
    -> Result<(quench_runtime::native_link::NativeImageInputs, Option<(Vec<u32>, Vec<(Vec<String>, Vec<u32>)>)>), String> {
    let bootstrap = work.join("bootstrap.bin");
    let mut worker = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    worker.args(["compile", BOOTSTRAP_WORKER_OPTION])
        .arg(native_entry)
        .arg(&bootstrap)
        .args(graph.snapshot_builtins());
    crate::host::startup_snapshot::neutralize_native_bootstrap_environment(&mut worker);
    Ok(match quench_runtime::native_link::checked_output(&mut worker, "native bootstrap preparation") {
        Ok(_) => {
            let mut catalog = fs::OpenOptions::new().append(true).open(inputs.catalog()).map_err(|error| error.to_string())?;
            writeln!(catalog, "const _: () = assert!(quench::aot::BOOTSTRAP_ABI == {:#x}, \"native bootstrap host ABI mismatch\");", super::BOOTSTRAP_ABI)
                .map_err(|error| error.to_string())?;
            report["bootstrap"] = json!({"status": "prepared", "builtins": graph.snapshot_builtins(), "bytes": fs::metadata(&bootstrap).map_err(|error| error.to_string())?.len(),
                "configuration": "neutral", "namespace": "native entry atom IDs preserved", "host_abi": format!("{:#x}", super::BOOTSTRAP_ABI)});
            let held = serde_json::from_slice(&fs::read(bootstrap.with_extension("operations")).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
            (inputs.with_bootstrap(bootstrap), Some(held))
        },
        Err(error) => {
            report["bootstrap"] = json!({"status": "unavailable", "diagnostic": error});
            (inputs, None)
        },
    })
}
