//! Linked native execution and build-time Node source inputs.

use crate::NodeHost;
use quench_runtime::{Runtime, NativeImageSupport};

pub const BOOTSTRAP_ABI: u64 = crate::host::shared_vm::BOOTSTRAP_ABI;

mod assets;
pub use assets::{EmbeddedModuleFile, EmbeddedModuleResolution, EmbeddedSourceView, EmbeddedModules, EmbeddedImportResolution, EmbeddedModuleSource, ModuleAssets, ModuleFile, PreparedModuleSource};
pub(crate) use assets::ModuleAssetState;
mod graph;
pub use graph::{DependencyGraph, GraphRequirement};
mod variant;
mod compiler;
pub(crate) mod prefix;
mod strict;
pub use compiler::{SourceManifest, CompiledSource, CompiledSources, DeferredSourceSyntaxError};
pub use quench_runtime::native_link::NativeImageInputs;

/// One compiler-owned source pool, shared by all embedding hosts.
#[macro_export]
macro_rules! linked_source_pool {
    ($($tokens:tt)*) => { quench_runtime::linked_source_pool! { $($tokens)* } };
}

/// Node's host selection for the shared linked-program catalog.
#[macro_export]
macro_rules! linked_native_sources {
    ($($tokens:tt)*) => { quench_runtime::linked_native_sources! { host $crate::NodeHost; $($tokens)* } };
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

/// A run saved at its first wait for input at build time (`aot::prefix::reach_wait`), as an
/// image's catalog lists it: the answers it assumed (question keys), the writes it made before
/// the wait, and the saved runtime.
pub struct SavedWait {
    pub answers: &'static [(&'static str, bool)],
    pub effects: &'static [(i32, &'static [u8])],
    pub rest: &'static [u8],
}

/// The first of `saved` whose answers hold in this process.
pub fn saved_wait(saved: &'static [SavedWait]) -> Option<&'static SavedWait> {
    saved.iter().find(|wait| wait.answers.iter().all(|&(key, answer)| prefix::Question::of_key(key).is_ok_and(|question| question.holds() == answer)))
}

/// Continues a linked entry from `wait`: its writes before the wait are made again in order,
/// then the event loop waits for the same input. The entry does not run.
pub fn resume_native_process_entry(
    host: NodeHost, program: &quench_runtime::NativeJsProgram, support: NativeImageSupport, wait: &'static SavedWait,
) -> Result<crate::shared_run::SharedCompletion, String> {
    for &(descriptor, bytes) in wait.effects {
        prefix::replay(descriptor, bytes);
    }
    let mut runtime = crate::shared_run::EntryRuntime::new(Runtime::new(host), crate::shared_run::Teardown::ProcessExit);
    runtime.set_execution_mode(support.execution_mode());
    runtime.set_memory_policy(crate::shared_run::select_memory_policy());
    let execution = runtime.resume_native_from_snapshot(std::rc::Rc::new(wait.rest), program, |host, reader| host.read_snapshot(reader));
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
