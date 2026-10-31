//! Linked native execution and build-time Node source inputs.

use crate::NodeHost;
use quench_runtime::Runtime;

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
pub use compiler::{SourceManifest, CompiledSource, CompiledSources, NativeImageInputs};
#[cfg(feature = "aot")]
pub(crate) use compiler::{HostSourceDemands, stage_host_sources};

/// One compiler-owned UTF-8 pool; all ranges retain their exact original text.
#[macro_export]
macro_rules! linked_source_pool {
    ($lookup:ident, $path:literal) => {
        const fn $lookup(start: usize, len: usize) -> &'static str {
            const TEXT: &str = include_str!($path);
            TEXT.split_at(start).1.split_at(len).0
        }
    };
}

/// Bind the compiler's metadata and static entry tables for NodeHost. The
/// generated invocation contains only data; this macro owns the loading rules.
#[macro_export]
macro_rules! linked_native_sources {
    (entry $entry:ident: $entry_symbol:ident => ($entry_link:literal, $entry_metadata:literal);
     resolver $resolve:ident {
         $($symbol:ident => ($link:literal, $metadata:literal, $name:literal, [$($part:expr),*] $(, source $source:expr)?),)*
     }) => {
        pub const NATIVE_SOURCE_COUNT: usize = (&[$(stringify!($symbol)),*] as &[&str]).len();
        /// # Safety
        /// Metadata, host ABI and all entry tables must be emitted together by
        /// this compiler, linked unmodified, and kept alive for the process.
        pub unsafe fn $entry() -> Result<quench_runtime::NativeJsProgram<$crate::NodeHost>, String> {
            unsafe extern "C" { #[link_name = $entry_link] static $entry_symbol: u8; }
            let bytes: quench_runtime::KeptBytes = std::rc::Rc::new(&include_bytes!($entry_metadata)[..]);
            let program = quench_runtime::ResidualProgram::from_binary_bytes(bytes)?;
            // SAFETY: the compiler emits the metadata and this linked table together.
            unsafe { quench_runtime::NativeJsProgram::from_linked_image(program, std::ptr::addr_of!($entry_symbol)) }
        }
        /// # Safety
        /// Every catalog record must meet the linked-image contract above.
        pub unsafe fn $resolve(namespace: &quench_runtime::NativeProgramNamespace, name: &str, parts: &[&str]) -> Result<Option<quench_runtime::NativeJsProgram<$crate::NodeHost>>, String> {
            $(if name == $name && parts == [$($part),*] {
                unsafe extern "C" { #[link_name = $link] static $symbol: u8; }
                let bytes: quench_runtime::KeptBytes = std::rc::Rc::new(&include_bytes!($metadata)[..]);
                let program = $crate::linked_native_sources!(@decode namespace, bytes $(, $source)?)?;
                // SAFETY: as above, including the compiler-wide atom namespace.
                return unsafe { quench_runtime::NativeJsProgram::from_linked_image(program, std::ptr::addr_of!($symbol)) }.map(Some);
            })*
            Ok(None)
        }
    };
    (@decode $namespace:ident, $bytes:ident) => { $namespace.decode($bytes) };
    (@decode $namespace:ident, $bytes:ident, $source:expr) => { $namespace.decode_with_static_source($bytes, $source) };
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
        $($module_name:literal => source $module_source:expr,)*
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
                modules: &[$($($crate::aot::EmbeddedModuleSource { name: $module_name, source: $module_source },)*)?],
            };
            &MODULES
        }
    };
}

/// Execute linked code with the same Node jobs, exit status and errors as the CLI.
pub fn execute_native(
    host: NodeHost, program: &quench_runtime::NativeJsProgram<NodeHost>,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::Free)
}

pub fn execute_native_process_entry(
    host: NodeHost, program: &quench_runtime::NativeJsProgram<NodeHost>,
) -> Result<crate::shared_run::SharedCompletion, String> {
    execute_native_with_teardown(host, program, crate::shared_run::Teardown::ProcessExit)
}

fn execute_native_with_teardown(
    host: NodeHost, program: &quench_runtime::NativeJsProgram<NodeHost>, teardown: crate::shared_run::Teardown,
) -> Result<crate::shared_run::SharedCompletion, String> {
    let mut runtime = crate::shared_run::EntryRuntime::new(Runtime::new(host), teardown);
    runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
    runtime.set_memory_policy(crate::shared_run::select_memory_policy(None)?);
    let execution = runtime.execute_native_deferred_jobs(program);
    crate::shared_run::finish_shared_execution(&mut runtime, program.residual(), execution)
}

#[cfg(feature = "aot")]
mod link;
#[cfg(feature = "aot")]
pub use link::NativeSupport;
#[cfg(feature = "aot")]
pub mod cli;
