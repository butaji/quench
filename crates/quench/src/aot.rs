//! Linked native execution and build-time Node source inputs.

use crate::NodeHost;
use quench_runtime::{Runtime, NativeImageSupport};

pub const BOOTSTRAP_ABI: u64 = crate::host::shared_vm::BOOTSTRAP_ABI;

mod assets;
pub use assets::{EmbeddedModuleFile, EmbeddedModuleResolution, EmbeddedSourceView, EmbeddedModules, EmbeddedImportResolution, EmbeddedModuleSource, ModuleAssets, ModuleFile, PreparedModuleSource};
pub(crate) use assets::ModuleAssetState;
mod graph;
mod host_operations;
mod image;
pub use graph::{DependencyGraph, GraphRequirement};
mod variant;
mod compiler;
mod builtin_families;
pub(crate) mod prefix;
mod strict;
pub(crate) mod image_dump;
pub use compiler::{ImageHost, SourceManifest, CompiledSource, CompiledSources, DeferredSourceSyntaxError};
pub use quench_runtime::native_link::NativeImageInputs;

/// One compiler-owned source pool, shared by all embedding hosts.
#[macro_export]
macro_rules! linked_source_pool {
    ($($tokens:tt)*) => { quench_runtime::linked_source_pool! { $($tokens)* } };
}

/// Node's host selection for the shared linked-program catalog: the catalog's programs are the
/// erased host's (`ErasedHost`), which the driver wraps its `NodeHost` in.
#[macro_export]
macro_rules! linked_native_sources {
    ($($tokens:tt)*) => { quench_runtime::linked_native_sources! { host quench_runtime::ErasedHost; $($tokens)* } };
}

/// Derive a static loader archive from compiler-owned raw inputs and edges.
#[macro_export]
macro_rules! linked_module_assets {
    ($factory:ident; entry $entry:expr; files {
        $($path:literal => ($bytes:literal, $kind:ident, $prepared:expr),)*
    } resolutions {
        $(($parent:literal, $specifier:literal) => $target:literal,)*
    } views {
        $(($view_path:literal, $view_key:literal) => $view_source:literal,)*
    }) => {
        $crate::linked_module_assets! {
            values $factory; entry $entry; files {
                $($path => (bytes include_bytes!($bytes), $kind, $prepared),)*
            } resolutions {
                $(($parent, $specifier) => $target,)*
            } views {
                $(($view_path, $view_key) => source include_str!($view_source),)*
            }
        }
    };
    (values $factory:ident; entry $entry:expr; files {
        $($path:literal => (bytes $bytes:expr, $kind:ident, $prepared:expr),)*
    } resolutions {
        $(($parent:literal, $specifier:literal) => $target:literal,)*
    } views {
        $(($view_path:literal, $view_key:literal) => source $view_source:expr,)*
    } $(imports {
        $(($import_referrer:literal, $import_specifier:literal) => $import_target:literal,)*
    } modules {
        $($module_name:literal => source $module_source:expr, $(typescript $module_typescript:expr,)?)*
    })?) => {
        pub fn $factory() -> &'static $crate::aot::EmbeddedModules {
            static MODULES: $crate::aot::EmbeddedModules = $crate::aot::EmbeddedModules {
                entry: $entry,
                files: &[$($crate::aot::EmbeddedModuleFile {
                    path: $path, bytes: $bytes, kind: quench_runtime::SourceKind::$kind, prepared: $prepared,
                },)*],
                resolutions: &[$($crate::aot::EmbeddedModuleResolution { parent: $parent, specifier: $specifier, target: $target },)*],
                views: &[$($crate::aot::EmbeddedSourceView { path: $view_path, key: $view_key, source: $view_source },)*],
                imports: &[$($($crate::aot::EmbeddedImportResolution { referrer: $import_referrer, specifier: $import_specifier, target: $import_target },)*)?],
                modules: &[$($($crate::aot::EmbeddedModuleSource { name: $module_name, source: $module_source, typescript: false $(|| $module_typescript)? },)*)?],
            };
            &MODULES
        }
    };
}

/// Execute linked code with the same Node jobs, exit status and errors as the CLI.
pub fn execute_native(
    host: NodeHost, program: &quench_runtime::NativeJsProgram,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::Free, NativeImageSupport::Static, None)
}

/// Execute a linked entry from canonical bootstrap state and release the runtime
/// afterward, so an embedding process can run another independent entry.
pub fn execute_native_with_bootstrap(
    host: NodeHost, program: &quench_runtime::NativeJsProgram,
    support: NativeImageSupport, bootstrap: &'static [u8],
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::Free, support, Some(bootstrap))
}

pub fn execute_native_process_entry(
    host: NodeHost, program: &quench_runtime::NativeJsProgram,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_process_entry_with_support(host, program, NativeImageSupport::Static)
}

pub fn execute_native_process_entry_with_support(
    host: NodeHost, program: &quench_runtime::NativeJsProgram, support: NativeImageSupport,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::ProcessExit, support, None)
}

pub fn execute_native_process_entry_with_bootstrap(
    host: NodeHost, program: &quench_runtime::NativeJsProgram, support: NativeImageSupport,
    bootstrap: &'static [u8],
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::ProcessExit, support, Some(bootstrap))
}

/// Runs a linked entry from the image's startup snapshot `bootstrap` in a runtime that never
/// installs its host (`Runtime::new_restored`): a strict image whose entry reads a process
/// input as it starts. As a resumed saved run does, it restores whatever configuration the
/// environment names (`NODE_OPTIONS`), which the snapshot settled.
pub fn execute_native_process_entry_restored(
    host: NodeHost, program: &quench_runtime::NativeJsProgram, support: NativeImageSupport,
    bootstrap: &'static [u8],
) -> Result<crate::shared_run::SharedCompletion, String> {
    let mut runtime = crate::shared_run::EntryRuntime::new(Runtime::new_restored(host), crate::shared_run::Teardown::ProcessExit);
    runtime.set_execution_mode(support.execution_mode());
    runtime.set_memory_policy(crate::shared_run::select_memory_policy());
    let execution = runtime.execute_native_from_snapshot(std::rc::Rc::new(bootstrap), program, |host, reader| host.read_snapshot(reader));
    finish_native(runtime, program, execution)
}

/// A run saved at its first wait for input at build time (`aot::prefix::reach_wait`), as an
/// image's catalog lists it: the answers it assumed (question keys), the writes it made before
/// the wait, and the saved runtime.
pub struct SavedWait {
    pub answers: &'static [(&'static str, bool)],
    pub effects: &'static [(i32, &'static [u8])],
    pub rest: &'static [u8],
}

/// The first of `saved` whose answers hold in this process, with its index; in a run that
/// dumps one (`image_dump`), that one.
pub fn saved_wait(saved: &'static [SavedWait]) -> Option<(usize, &'static SavedWait)> {
    if let Some(index) = image_dump::requested_wait() {
        return saved.get(index).map(|wait| (index, wait));
    }
    saved.iter().enumerate().find(|(_, wait)| wait.answers.iter().all(|&(key, answer)| prefix::Question::of_key(key).is_ok_and(|question| question.holds() == answer)))
}

/// Runs an image's process, which its driver's C `main` calls in place of std's runtime start,
/// and exits with its status. Of what std's start does, the process keeps what it relies on:
/// standard descriptors that are open (a closed one is `/dev/null`, so no file the program
/// opens takes its number) and SIGPIPE ignored, so a write to a closed pipe is an `EPIPE`
/// error as in Node. It leaves out the main thread's stack-overflow handler, whose setup reads
/// the process's whole memory map; the program runs on a thread of its own
/// (`shared_run::run_node_worker`). The exit flushes std's buffered output as std's does.
pub fn run_image_process(run: impl FnOnce() -> Result<u8, String> + Send + 'static) -> ! {
    // SAFETY: plain system calls on the standard descriptors, before any thread starts.
    unsafe {
        let mut descriptors = [0, 1, 2].map(|fd| libc::pollfd { fd, events: 0, revents: 0 });
        if libc::poll(descriptors.as_mut_ptr(), 3, 0) >= 0 {
            for descriptor in descriptors {
                if descriptor.revents & libc::POLLNVAL != 0 {
                    libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
                }
            }
        }
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    image_dump::begin();
    let status = match crate::host::process_start::begin_main().and_then(|()| crate::shared_run::run_node_worker(run)) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("quench: {error}");
            1
        }
    };
    std::process::exit(i32::from(status))
}

/// Continues a linked entry from `wait`, the saved run `index`: its writes before the wait are
/// made again in order, then the event loop waits for the same input. The entry does not run.
/// The runtime at rest is mapped from the image's dump of it where it can be
/// (`image_dump::adopt`), else decoded from the saved run.
pub fn resume_native_process_entry(
    host: NodeHost, program: quench_runtime::NativeJsProgram, support: NativeImageSupport, index: usize, wait: &'static SavedWait,
) -> Result<crate::shared_run::SharedCompletion, String> {
    for &(descriptor, bytes) in wait.effects {
        prefix::replay(descriptor, bytes);
    }
    type Rest = (crate::shared_run::EntryRuntime, quench_runtime::NativeJsProgram);
    if let Some(root) = image_dump::adopt(index) {
        // SAFETY: the dump's root is the `Rest` its run leaked, mapped back at its address.
        // Kept in place: moving the runtime out of its box would copy it whole.
        let mut rest = unsafe { Box::from_raw(root.cast::<Rest>()) };
        let (runtime, entry) = &mut *rest;
        drop(program);
        runtime.host_mut().adopt_process(host);
        runtime.set_memory_policy(crate::shared_run::select_memory_policy());
        crate::host::resident_pages::phase_peak("saved wait mapped");
        let execution = runtime.resume_restored();
        crate::host::resident_pages::phase_peak("saved wait resumed");
        return crate::shared_run::finish_shared_execution(runtime, entry.residual(), execution);
    }
    let mut runtime = crate::shared_run::EntryRuntime::new(Runtime::new_at_rest(host), crate::shared_run::Teardown::ProcessExit);
    runtime.set_execution_mode(support.execution_mode());
    runtime.set_memory_policy(crate::shared_run::select_memory_policy());
    crate::host::resident_pages::phase_peak("saved wait runtime made");
    let restored = runtime.restore_native_at_rest(std::rc::Rc::new(wait.rest), &program, |host, reader| host.read_snapshot(reader));
    if restored.is_ok() && image_dump::requested_wait().is_some() {
        image_dump::write(index, Box::into_raw(Box::new((runtime, program))).cast());
    }
    let execution = restored.and_then(|()| runtime.resume_restored());
    crate::host::resident_pages::phase_peak("saved wait resumed");
    crate::shared_run::finish_shared_execution(&mut runtime, program.residual(), execution)
}

fn execute_native_with_teardown(
    host: NodeHost, program: &quench_runtime::NativeJsProgram, teardown: crate::shared_run::Teardown, support: NativeImageSupport,
    bootstrap: Option<&'static [u8]>,
) -> Result<crate::shared_run::SharedCompletion, String> {
    let bootstrap = bootstrap.filter(|_| host.accepts_native_bootstrap());
    let mut runtime = crate::shared_run::EntryRuntime::new(Runtime::new(host), teardown);
    runtime.set_execution_mode(support.execution_mode());
    runtime.set_memory_policy(crate::shared_run::select_memory_policy());
    let execution = match bootstrap {
        Some(bytes) => runtime.execute_native_from_snapshot(std::rc::Rc::new(bytes), program, |host, reader| host.read_snapshot(reader)),
        None => runtime.execute_native_deferred_jobs(program),
    };
    finish_native(runtime, program, execution)
}

/// The completion of a native entry's run, or why the image failed it.
fn finish_native(
    mut runtime: crate::shared_run::EntryRuntime, program: &quench_runtime::NativeJsProgram, execution: Result<(), quench_runtime::JsError>,
) -> Result<crate::shared_run::SharedCompletion, String> {
    let completion = crate::shared_run::finish_shared_execution(&mut runtime, program.residual(), execution)?;
    let rejected = runtime.native_execution_rejections();
    if rejected != 0 {
        return Err(format!("native AOT execution rejected {rejected} unsupported operation(s)"));
    }
    if let Some(path) = runtime.host_mut().missing_frozen_source() {
        return Err(format!("native AOT image omitted required module source: {}", path.display()));
    }
    Ok(completion)
}

/// The lane helper symbols an ahead image's objects name (`quench_lane_helper_<Name>`), defined
/// here once for the Node host: an image's driver links them from this library instead of
/// instantiating every helper, and all they reach, in its own crate (about 2 minutes per row).
#[cfg(feature = "native-execution")]
#[unsafe(export_name = "quench_lane_helper_symbols")]
pub extern "C" fn lane_helper_symbols() {
    quench_runtime::define_lane_helper_symbols()
}

mod link;
pub use quench_runtime::native_link::{DynamicInterface, ExportPolicy};
pub use link::{NativeSupport, NativeLinkAudit};
pub mod cli;
