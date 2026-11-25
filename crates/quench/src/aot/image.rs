//! Image staging: the sources an image compiles, the host services and bootstrap it links, and
//! the driver its executable is linked from.

use super::cli::BOOTSTRAP_WORKER_OPTION;
use super::{NativeSupport, SourceManifest};
use quench_runtime::native_link::{NativeImageInputs, NativeLinkAudit};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
};

/// Stage the Node bootstrap's source units for a Node image; a core image runs no bootstrap.
pub(super) fn capture_bootstrap(core: bool, graph: &super::DependencyGraph, manifest: &mut SourceManifest) -> Result<(), String> {
    if !core {
        manifest.extend(SourceManifest::capture_for_graph(graph)?)?;
    }
    Ok(())
}

/// Compile the image's sources: the Node loader's empty entry plus its graph units, or a core
/// image's whole source as the entry itself.
pub(super) fn compile_sources(
    core: bool, graph: &super::DependencyGraph, entry: &Path, manifest: SourceManifest,
) -> Result<super::CompiledSources, String> {
    if !core {
        return manifest.compile();
    }
    let file = graph.files.get(entry).ok_or("core entry is not in the module graph")?;
    let source = crate::host::shared_vm::commonjs::source_text(&file.bytes);
    manifest.compile_closed(&source, file.kind)
}

/// Stage the host services and bootstrap an image links: a core image links neither, a Node
/// image takes the operation table its program can reach and a snapshot of the builtins it
/// names.
pub(super) fn stage_image(
    core: bool,
    graph: &super::DependencyGraph,
    work: &Path,
    images: &Path,
    inputs: NativeImageInputs,
    report: &mut Value,
) -> Result<NativeImageInputs, String> {
    if core {
        report["unlinked_host_operations"] = json!(0);
        report["bootstrap"] = json!({"status": "not_required", "reason": "Variant::Core runs on the runtime's own host"});
        return Ok(inputs);
    }
    let (table, unlinked) = super::host_operations::operation_table(graph);
    fs::OpenOptions::new().append(true).open(inputs.catalog()).and_then(|mut catalog| catalog.write_all(table.as_bytes())).map_err(|error| error.to_string())?;
    report["unlinked_host_operations"] = json!(unlinked);
    prepare_bootstrap(graph, work, images, inputs, report)
}

/// Prepare the Node bootstrap snapshot and append its host-ABI assertion to the catalog.
fn prepare_bootstrap(
    graph: &super::DependencyGraph,
    work: &Path,
    images: &Path,
    inputs: NativeImageInputs,
    report: &mut Value,
) -> Result<NativeImageInputs, String> {
    let bootstrap = work.join("bootstrap.bin");
    let mut worker = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    worker.args(["compile", BOOTSTRAP_WORKER_OPTION])
        .arg(images.join(format!("{}.bin", super::compiler::NATIVE_ENTRY_UNIT)))
        .arg(&bootstrap)
        .args(graph.snapshot_builtins());
    crate::host::startup_snapshot::neutralize_native_bootstrap_environment(&mut worker);
    match quench_runtime::native_link::checked_output(&mut worker, "native bootstrap preparation") {
        Ok(_) => {
            let mut catalog = fs::OpenOptions::new().append(true).open(inputs.catalog()).map_err(|error| error.to_string())?;
            writeln!(catalog, "const _: () = assert!(quench::aot::BOOTSTRAP_ABI == {:#x}, \"native bootstrap host ABI mismatch\");", super::BOOTSTRAP_ABI)
                .map_err(|error| error.to_string())?;
            report["bootstrap"] = json!({"status": "prepared", "builtins": graph.snapshot_builtins(), "bytes": fs::metadata(&bootstrap).map_err(|error| error.to_string())?.len(),
                "configuration": "neutral", "namespace": "native entry atom IDs preserved", "host_abi": format!("{:#x}", super::BOOTSTRAP_ABI)});
            Ok(inputs.with_bootstrap(bootstrap))
        },
        Err(error) => {
            report["bootstrap"] = json!({"status": "unavailable", "diagnostic": error});
            Ok(inputs)
        },
    }
}


/// Link the staged image with the driver of its host: the Node host's loader image, or the
/// runtime's own host for a core image.
pub(super) fn link_image(
    core: bool,
    support: &NativeSupport,
    inputs: &NativeImageInputs,
    work: &Path,
    staged: &Path,
) -> Result<NativeLinkAudit, String> {
    let driver = work.join("driver.rs");
    let source = if core { include_str!("image_driver_core.rs") } else { include_str!("image_driver.rs") };
    fs::write(&driver, source).map_err(|error| error.to_string())?;
    if core { support.link_audited_core(inputs, &driver, staged) } else { support.link_audited(inputs, &driver, staged) }
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
