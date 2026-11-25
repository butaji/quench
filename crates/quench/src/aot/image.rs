//! Image staging: the sources an image compiles, the host services and bootstrap it links, the
//! runs it saves at their first wait for input, and the driver its executable is linked from.

use super::cli::{BOOTSTRAP_WORKER_OPTION, WAIT_WORKER_OPTION};
use super::{CompiledSources, DependencyGraph, NativeSupport, SourceManifest};
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

/// Stage the host services, saved runs and bootstrap an image links: a core image links none,
/// a Node image takes the operation table its program can reach and a snapshot of the builtins
/// it names. Returns the image inputs and the operations a resume-only image keeps.
pub(super) fn stage_image(
    core: bool,
    graph: &DependencyGraph,
    work: &Path,
    native_entry: &Path,
    inputs: NativeImageInputs,
    waits: &[super::strict::Answers],
    report: &mut Value,
) -> Result<(NativeImageInputs, Option<BTreeSet<u32>>), String> {
    if core {
        report["unlinked_host_operations"] = json!(0);
        report["bootstrap"] = json!({"status": "not_required", "reason": "Variant::Core runs on the runtime's own host"});
        return Ok((inputs, None));
    }
    let saved = prepare_saved_waits(waits, work, native_entry, report);
    fs::OpenOptions::new().append(true).open(inputs.catalog())
        .and_then(|mut catalog| catalog.write_all(resumes_at_rest(&saved).as_bytes())).map_err(|error| error.to_string())?;
    let inputs = if saved.is_empty() { bootstrap_inputs(graph, work, native_entry, inputs, report)? } else { inputs };
    // An image that only resumes saved runs starts from the operations their heaps hold, and
    // the builders of the builtins its program may name only as it runs (`require`, a lazy
    // global's module): every one the graph names.
    let held = (!saved.is_empty()).then(|| {
        let named = graph.snapshot_builtins().into_iter()
            .chain(crate::modules::util_lazy_shared_vm::lazy_global_modules().map(str::to_owned));
        let builders = named.filter_map(|name| crate::host::shared_vm::commonjs::builtin_operation_of(&name)).map(|operation| operation.0);
        saved.iter().flat_map(|wait| wait.held.iter().copied()).chain(builders).collect()
    });
    Ok((inputs, held))
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
struct SavedWait {
    answers: super::strict::Answers,
    effects: Vec<(i32, Vec<u8>)>,
    rest: PathBuf,
    /// The host operations the saved runtime's host functions run.
    held: Vec<u32>,
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
    format!("static RESUMES_AT_REST: &[quench::aot::SavedWait] = &[{runs}];\nconst RESUME_ONLY: bool = {};\n", !saved.is_empty())
}

/// The image's inputs with its startup snapshot (`--bootstrap-worker`), or without one when the
/// snapshot cannot be prepared; the catalog gains the start's constants either way.
fn bootstrap_inputs(graph: &DependencyGraph, work: &Path, native_entry: &Path, inputs: NativeImageInputs, report: &mut Value) -> Result<NativeImageInputs, String> {
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
            inputs.with_bootstrap(bootstrap)
        },
        Err(error) => {
            report["bootstrap"] = json!({"status": "unavailable", "diagnostic": error});
            inputs
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
