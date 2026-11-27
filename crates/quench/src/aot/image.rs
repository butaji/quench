//! Image staging: the sources an image compiles, the host services and bootstrap it links, the
//! runs it saves at their first wait for input, and the driver its executable is linked from.

use super::cli::{BOOTSTRAP_WORKER_OPTION, WAIT_WORKER_OPTION};
use super::{CompiledSources, DependencyGraph, NativeSupport, SourceManifest};
use quench_runtime::native_link::image_dump::{self, Dump, UNDUMPED};
use quench_runtime::native_link::{NativeImageInputs, NativeLinkAudit};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

/// Stage the Node bootstrap's source units for a Node image; a core image runs no bootstrap.
pub(super) fn capture_bootstrap(core: bool, graph: &DependencyGraph, manifest: &mut SourceManifest) -> Result<(), String> {
    if !core {
        manifest.extend(SourceManifest::capture_for_graph(graph)?)?;
    }
    Ok(())
}

/// Compile the image's sources: the Node loader's empty entry plus its graph units, or a core
/// image's whole source as the entry itself.
pub(super) fn compile_sources(
    core: bool, graph: &DependencyGraph, entry: &Path, manifest: SourceManifest,
) -> Result<CompiledSources, String> {
    if !core {
        return manifest.compile();
    }
    let file = graph.files.get(entry).ok_or("core entry is not in the module graph")?;
    let source = crate::host::shared_vm::commonjs::source_text(&file.bytes);
    manifest.compile_closed(&source, file.kind)
}

/// Stage the host services, saved runs and bootstrap an image links: a core image links none
/// (its start state is dumped once it is linked, `core_rest`); a Node image takes the operation
/// table its program can reach and a snapshot of the builtins it names. Returns the image inputs,
/// the operations an image that starts from a saved heap keeps, and its saved runs
/// (`dump_saved_waits`). A `strict` image whose entry reads a process input runs it from the
/// startup snapshot alone (`RESTORED`).
pub(super) fn stage_image(
    core: bool,
    strict: bool,
    graph: &DependencyGraph,
    work: &Path,
    native_entry: &Path,
    inputs: NativeImageInputs,
    waits: &[super::strict::Answers],
    report: &mut Value,
) -> Result<(NativeImageInputs, Option<BTreeSet<u32>>, Vec<SavedWait>), String> {
    if core {
        report["unlinked_host_operations"] = json!(0);
        core_memory_policy(&inputs.catalog())?;
        return Ok((inputs, None, Vec::new()));
    }
    // A program whose build-time runs wait for input before reading any resumes the run whose
    // answers hold as it starts: no entry run and no installation, so no startup snapshot.
    let saved = prepare_saved_waits(waits, work, native_entry, report);
    let (inputs, startup_held) = if saved.is_empty() { bootstrap_inputs(graph, work, native_entry, inputs, report)? } else { (inputs, None) };
    // A strict image whose entry reads a process input runs it from the startup snapshot alone.
    let restored = strict && startup_held.is_some();
    // The heaps the image starts from: each saved run's, else the startup snapshot's.
    let heaps: Vec<super::prefix::HeldOperations> = match startup_held {
        Some(held) => vec![held],
        None => saved.iter().map(|wait| wait.held.clone()).collect(),
    };
    let lane_opcodes = (!saved.is_empty()).then(|| {
        let mut opcodes: Vec<u16> = heaps.iter().flat_map(|heap| heap.2.iter().copied()).collect();
        opcodes.sort_unstable();
        opcodes.dedup();
        opcodes
    });
    fs::OpenOptions::new().append(true).open(inputs.catalog())
        .and_then(|mut catalog| catalog.write_all(format!("{}const MAKES_CODE: bool = {};\nconst RUNS_WASM: bool = {};\nconst RESTORED: bool = {restored};\nconst REACHES_INTL: bool = {};\n",
            resumes_at_rest(&saved, lane_opcodes.as_deref()), makes_code(graph), runs_wasm(graph), NODE_REACHES_INTL).as_bytes()))
        .map_err(|error| error.to_string())?;
    // An image that starts from a saved heap (a saved run's, or the startup snapshot's) starts
    // from the operations that heap holds, and the builders of the builtins its program may
    // name only as it runs (`require`, a lazy global's module): every one the graph names.
    let held = (!heaps.is_empty()).then(|| {
        // What the global object holds only for globals no loaded code names (a pending lazy
        // global's initializer, an accessor's getter) runs only when code reads one of those
        // names: code the program holds names it, code it loads may, and so may a computed read
        // of the global object, which code reaches by an alias's name or a sloppy call's `this`.
        let names_any = super::host_operations::loads_code(graph) || graph.script_reads_this
            || graph.builtins.iter().any(|name| name.trim_start_matches("node:") == "vm")
            || super::graph::GLOBAL_OBJECT_NAMES.iter().any(|alias| graph.global_names.contains(*alias));
        // A compression stream builds `zlib` by name (`commonjs::zlib_module_named`): kept where
        // the program can reach the class, by its global or `stream/web`.
        let compresses = names_any || ["CompressionStream", "DecompressionStream"].iter().any(|name| graph.global_names.contains(*name))
            || graph.builtins.iter().chain(&graph.esm_builtins).any(|name| name.trim_start_matches("node:") == "stream/web");
        let named = graph.snapshot_builtins().into_iter().chain(crate::modules::util_lazy_shared_vm::lazy_global_modules().map(str::to_owned))
            .chain(compresses.then(|| "zlib".to_owned()))
            .chain(crate::host::shared_vm::commonjs::replaceable_lazy_global_modules()
                .filter(|(name, _)| names_any || graph.global_names.contains(*name) || crate::host::shared_vm::HOST_READ_GLOBALS.contains(name))
                .map(|(_, module)| module.to_owned()));
        let builders = named.filter_map(|name| crate::host::shared_vm::commonjs::builtin_operation_of(&name)).map(|operation| operation.0);
        // A group is read where the program names one of its globals, or where host code reads
        // one by name as it runs (`shared_vm::HOST_READ_GLOBALS`).
        let kept = |names: &Vec<String>| names_any || names.iter().any(|name| {
            graph.global_names.contains(name) || crate::host::shared_vm::HOST_READ_GLOBALS.contains(&name.as_str())
        });
        report["unnamed_globals"] = json!(heaps.iter().flat_map(|heap| &heap.1).map(|(names, operations)| json!({
            "names": names, "operations": operations.len(), "kept": kept(names),
        })).collect::<Vec<_>>());
        let lazy = heaps.iter().flat_map(|heap| &heap.1)
            .filter(|(names, _)| kept(names))
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
    Ok((inputs, held, saved))
}

/// Writes a core image's memory policy into its catalog: the policy the build selects (the
/// memory-first experiment, read here at compile time), and whether the image's allocator serves
/// small requests (the throughput policy, unless the memory mix names the system allocator). The
/// image reads these constants and no environment at run time.
fn core_memory_policy(catalog: &Path) -> Result<(), String> {
    let memory_first = quench_runtime::trace::Dev::MemoryFirst.on();
    let policy = if memory_first { "MemoryFirst" } else { "Throughput" };
    let allocator = !memory_first && !quench_runtime::memory_mix_has("alloc");
    fs::OpenOptions::new().append(true).open(catalog)
        .and_then(|mut file| file.write_all(format!("const MEMORY_POLICY: quench_runtime::MemoryPolicy = quench_runtime::MemoryPolicy::{policy};\nconst ALLOCATOR_SERVES_SMALL: bool = {allocator};\n").as_bytes()))
        .map_err(|error| error.to_string())
}

/// The worker: runs the native entry's residual `program` with the build-time gate armed
/// (`aot::prefix`), under `assumptions` (`QUESTION=0|1`), to its first wait for input, and writes
/// the run saved there to `output` and the writes it made before to `output` with the extension
/// `effects` (JSON pairs of descriptor and bytes). Fails when the run does not reach such a wait,
/// having read a dynamic input.
pub(super) fn build_rest(program: &Path, output: &Path, assumptions: &[String]) -> Result<(), String> {
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
pub(super) struct SavedWait {
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
fn resumes_at_rest(saved: &[SavedWait], lane_opcodes: Option<&[u16]>) -> String {
    let runs = saved.iter().map(|wait| {
        let answers = wait.answers.iter().map(|(question, answer)| format!("({:?}, {answer})", question.key())).collect::<Vec<_>>().join(", ");
        let writes = wait.effects.iter().map(|(descriptor, bytes)| format!("({descriptor}, &{bytes:?})")).collect::<Vec<_>>().join(", ");
        format!("quench::aot::SavedWait {{ answers: &[{answers}], effects: &[{writes}], rest: include_bytes!({:?}) }}", wait.rest.display().to_string())
    }).collect::<Vec<_>>().join(", ");
    // The runs answer every yes-or-no input the build-time evaluation asked, each both ways
    // (`strict::specialize`), so one always holds: an image with saved runs links no other start.
    // An image that only resumes them installs lane handlers for `lane_opcodes`, those its
    // heaps' programs hold, any other running on the general path; one that runs its entry,
    // every one.
    let group = crate::LANE_ROW_GROUP;
    let lanes = match lane_opcodes {
        Some(opcodes) => {
            let rows = opcodes.iter().map(|op| format!("&quench::lane_rows::g{}::R{}", op / group, op % group)).collect::<Vec<_>>().join(", ");
            format!("quench::install_lane_rows(&[{rows}])")
        }
        None => "quench::install_all_lane_rows()".to_owned(),
    };
    format!("static RESUMES_AT_REST: &[quench::aot::SavedWait] = &[{runs}];\nconst RESUME_ONLY: bool = {};\nfn install_lanes() {{ {lanes} }}\n", !saved.is_empty())
}

/// The statics and thread-locals a Node image's dump never carries beyond the runtime's
/// (`image_dump::UNDUMPED`): the host process's own state, which the environment sets.
const HOST_UNDUMPED: &[&[&str]] = &[
    &["quench", "shared_run"], &["quench", "allocator_policy"], &["quench", "host", "resident_pages"],
    &["quench", "host", "process_start"], &["quench", "modules", "process_signals_state"],
];

/// Dumps each saved run of the linked Node `image` (`image_dump::dump_image`), each under its
/// answers' environment and named by the environment (`DUMP_VARIABLE`); none where any cannot be
/// dumped, which `report` names.
pub(super) fn dump_saved_waits(image: &Path, saved: &[SavedWait], work: &Path, report: &mut Value) -> Vec<Dump> {
    let answers: Vec<Vec<(String, String)>> = saved.iter().map(|wait| wait.answers.iter().filter_map(|(question, answer)| match question {
        super::prefix::Question::Environment(name, value) if *answer => Some((name.clone(), value.clone())),
        _ => None,
    }).collect()).collect();
    let undumped: Vec<&[&str]> = UNDUMPED.iter().chain(HOST_UNDUMPED.iter()).copied().collect();
    let prepare = |run: &mut Command, index: usize, path: &Path| {
        crate::host::startup_snapshot::neutralize_native_bootstrap_environment(run);
        for name in super::ADOPTION_VARIABLES {
            run.env_remove(name);
        }
        for (name, value) in &answers[index] {
            run.env(name, value);
        }
        run.env(image_dump::DUMP_VARIABLE, format!("{index}:{}", path.display()));
    };
    match image_dump::dump_image(image, work, saved.len(), &undumped, &prepare) {
        Ok(dumps) => {
            report["dump"] = json!({"status": "prepared", "runs": dumps.iter().map(|(dump, raw)| json!({"raw_bytes": raw, "page_bytes": dump.page_bytes()})).collect::<Vec<_>>()});
            dumps.into_iter().map(|(dump, _)| dump).collect()
        }
        Err(error) => {
            report["dump"] = json!({"status": "unavailable", "diagnostic": error});
            Vec::new()
        }
    }
}

/// The image's inputs with its startup snapshot (`--bootstrap-worker`), or without one when the
/// snapshot cannot be prepared; the catalog gains the start's constants either way.
/// Also the host operations the snapshot's heap holds (`build_bootstrap`).
fn bootstrap_inputs(graph: &DependencyGraph, work: &Path, native_entry: &Path, inputs: NativeImageInputs, report: &mut Value)
    -> Result<(NativeImageInputs, Option<super::prefix::HeldOperations>), String> {
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
                "configuration": "neutral", "namespace": "native entry atom IDs preserved", "host_abi": format!("{:#x}", super::BOOTSTRAP_ABI),
                // The code the startup heap's programs carry, which runs on the interpreter.
                "heap_code": fs::read(bootstrap.with_extension("code")).ok().and_then(|code| serde_json::from_slice::<Value>(&code).ok())});
            let held = serde_json::from_slice(&fs::read(bootstrap.with_extension("operations")).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
            (inputs.with_bootstrap(bootstrap), Some(held))
        },
        Err(error) => {
            report["bootstrap"] = json!({"status": "unavailable", "diagnostic": error});
            (inputs, None)
        },
    })
}

/// Link the staged image with the driver of its host: the Node host's loader image, or the
/// runtime's own host for a core image.
pub(super) fn link_image(
    core: bool,
    support: &NativeSupport,
    inputs: &NativeImageInputs,
    work: &Path,
    staged: &Path,
    graph: &DependencyGraph,
    held: Option<BTreeSet<u32>>,
    report: &mut Value,
) -> Result<NativeLinkAudit, String> {
    let driver = work.join("driver.rs");
    let source = if core { include_str!("image_driver_core.rs") } else { include_str!("image_driver.rs") };
    fs::write(&driver, source).map_err(|error| error.to_string())?;
    if core {
        return support.link_audited_core(inputs, &driver, staged);
    }
    super::host_operations::link_with_reached_operations(support, inputs, &driver, staged, graph, held, report)
}

/// The image's recorded dynamic dependencies (`otool -L`/`readelf -d`), for the report.
pub(super) fn native_dependencies(executable: &Path) -> Result<String, String> {
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

/// Whether the program can make code as it runs, so its image links the source frontend
/// (`quench::host::enable_dynamic_source`): it loads code (`loads_code`), evaluates an
/// `import()`, names eval, `Function` or `Reflect`, imports a module that compiles source, or
/// reads the global object by a computed name. An image of any other program refuses code
/// generation from strings, as Node does under `--disallow-code-generation-from-strings`.
fn makes_code(graph: &DependencyGraph) -> bool {
    const COMPILING_MODULES: &[&str] = &["vm", "repl", "module", "worker_threads", "inspector"];
    const CODE_NAMES: &[&str] = &["eval", "Function", "Reflect", "WebAssembly", "Worker", "ShadowRealm"];
    super::host_operations::loads_code(graph) || graph.imports_at_run_time || graph.script_reads_this
        || graph.builtins.iter().chain(&graph.esm_builtins).any(|name| COMPILING_MODULES.contains(&name.trim_start_matches("node:")))
        || CODE_NAMES.iter().chain(super::graph::GLOBAL_OBJECT_NAMES).any(|name| graph.global_names.contains(*name))
}

/// Whether the program can reach WebAssembly, so its image links the Wasm frontend
/// (`quench::host::enable_wasm`): it can make code (`makes_code`, which a `WebAssembly` name
/// implies) or imports a `.wasm` module.
fn runs_wasm(graph: &DependencyGraph) -> bool {
    makes_code(graph) || graph.requirements.iter().any(|requirement| matches!(requirement, super::GraphRequirement::Wasm { .. }))
}

/// A Node image links the Intl and Temporal layer. Its realm is not closed yet (the closed world of
/// `ReachableRealm` is computed over the core realm), so the layer is kept for every Node image:
/// the locale and Intl entries are reached through names and spellings the graph cannot bound.
const NODE_REACHES_INTL: bool = true;
