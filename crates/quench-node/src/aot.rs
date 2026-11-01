//! Linked native execution and build-time Node source inputs.

use crate::NodeHost;
use quench_runtime::{Runtime, NativeImageSupport};

mod assets;
pub use assets::{EmbeddedModuleFile, EmbeddedModuleResolution, EmbeddedSourceView, EmbeddedModules, EmbeddedImportResolution, EmbeddedModuleSource, ModuleAssets, ModuleFile, PreparedModuleSource};
pub(crate) use assets::ModuleAssetState;
#[cfg(feature = "aot")]
mod graph;
#[cfg(feature = "aot")]
pub use graph::{DependencyGraph, GraphRequirement};
#[cfg(feature = "aot")]
mod compiler;
#[cfg(feature = "aot")]
pub use compiler::{SourceManifest, CompiledSource, CompiledSources, DeferredSourceSyntaxError};
#[cfg(feature = "aot")]
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
    ($factory:ident; root $root:expr; entry $entry:expr; files {
        $($path:literal => ($bytes:literal, $kind:ident, $prepared:expr),)*
    } resolutions {
        $(($parent:literal, $specifier:literal) => $target:literal,)*
    } views {
        $(($view_path:literal, $view_key:literal) => $view_source:literal,)*
    }) => {
        $crate::linked_module_assets! {
            values $factory; root $root; entry $entry; files {
                $($path => (bytes include_bytes!($bytes), $kind, $prepared),)*
            } resolutions {
                $(($parent, $specifier) => $target,)*
            } views {
                $(($view_path, $view_key) => source include_str!($view_source),)*
            }
        }
    };
    (values $factory:ident; root $root:expr; entry $entry:expr; files {
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
                node_root: $root, entry: $entry,
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
    host: NodeHost, program: &quench_runtime::NativeJsProgram<NodeHost>,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::Free, NativeImageSupport::Static)
}

pub fn execute_native_process_entry(
    host: NodeHost, program: &quench_runtime::NativeJsProgram<NodeHost>,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_process_entry_with_support(host, program, NativeImageSupport::Static)
}

pub fn execute_native_process_entry_with_support(
    host: NodeHost, program: &quench_runtime::NativeJsProgram<NodeHost>, support: NativeImageSupport,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::ProcessExit, support)
}

fn execute_native_with_teardown(
    host: NodeHost, program: &quench_runtime::NativeJsProgram<NodeHost>, teardown: crate::shared_run::Teardown, support: NativeImageSupport,
) -> Result<crate::shared_run::SharedCompletion, String> {
    let mut runtime = crate::shared_run::EntryRuntime::new(Runtime::new(host), teardown);
    runtime.set_execution_mode(support.execution_mode());
    runtime.set_memory_policy(crate::shared_run::select_memory_policy(None)?);
    let execution = runtime.execute_native_deferred_jobs(program);
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

#[cfg(feature = "aot")]
mod link;
#[cfg(feature = "aot")]
pub use quench_runtime::native_link::{DynamicInterface, ExportPolicy};
#[cfg(feature = "aot")]
pub use link::{NativeSupport, NativeLinkAudit};
#[cfg(feature = "aot")]
pub mod cli;
