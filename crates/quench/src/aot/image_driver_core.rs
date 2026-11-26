//! The pure-ECMAScript image driver: the same compiled program and image tables as the Node
//! driver, executed on the runtime's own host. It links `quench_runtime` alone, so the
//! executable carries no Node host, no bootstrap snapshot and no operation table.

use quench_runtime::{ExecutionMode, MemoryPolicy, NativeImageSupport, NativeProgramNamespace, Runtime, SystemHost};

// Every runtime layer (engine, intl, stencils) links through this facade: the core calls their
// `extern "Rust"` symbols, so a binary that uses the core must name the crate.
extern crate quench_layers;

#[cfg(quench_native_dynamic_source)]
const SUPPORT: NativeImageSupport = NativeImageSupport::DynamicSource;
#[cfg(not(quench_native_dynamic_source))]
const SUPPORT: NativeImageSupport = NativeImageSupport::Static;
const _: () = assert!(
    SUPPORT.is_available() && (SUPPORT.allows_dynamic_source() || quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY),
    "native image SDK does not provide its selected execution support"
);

/// The runtime's size-classed small-object allocator (`quench_alloc`), which the Node driver
/// also installs (`quench::GLOBAL_ALLOCATOR`).
#[cfg(unix)]
#[global_allocator]
static GLOBAL_ALLOCATOR: quench_alloc::QuenchAlloc = quench_alloc::QuenchAlloc::new();

include!(env!("QUENCH_NATIVE_CATALOG"));

/// The policy this image runs: the default, or memory-first under the experiment the Node host
/// also reads (`QUENCH_DEV=memory-first`).
fn memory_policy() -> MemoryPolicy {
    if quench_runtime::trace::Dev::MemoryFirst.on() {
        MemoryPolicy::MemoryFirst
    } else {
        MemoryPolicy::default()
    }
}

/// Serve small requests from Quench's allocator under the throughput policy, as the Node host's
/// `allocator_policy` does; the memory-first policy keeps the system allocator.
fn apply_allocator_policy(policy: MemoryPolicy) {
    #[cfg(unix)]
    if policy == MemoryPolicy::Throughput && !quench_runtime::memory_mix_has("alloc") {
        quench_alloc::QuenchAlloc::enable();
    }
    #[cfg(not(unix))]
    let _ = policy;
}

/// Reports a startup phase's elapsed time when `QUENCH_TRACE=rss-phases` is set, as the Node
/// driver's `resident_pages::phase_peak` does; a core image has no host phase reporter.
fn phase(started: std::time::Instant, name: &str) {
    if quench_runtime::trace::Channel::RssPhases.on() {
        eprintln!("quench: core phase {name:<20} at {:>8} us", started.elapsed().as_micros());
    }
}

fn run() -> Result<std::process::ExitCode, String> {
    let started = std::time::Instant::now();
    // SAFETY: the compiler emits the catalog, metadata and objects together.
    let entry = unsafe { bind_entry() }?;
    let namespace = NativeProgramNamespace::from_entry(entry.residual())?;
    phase(started, "entry bound");
    let policy = memory_policy();
    apply_allocator_policy(policy);
    // A dynamic-source core (`Variant::CoreDynamic`) keeps the interpreter and the source
    // frontend but still runs on `SystemHost`: `Vm::new` installs every route. A closed core
    // runs only its image's code: it installs no JIT or source route and no Intl layer unless
    // the support carries it (`support_features` includes `intl` where the program reaches it).
    let mut runtime = if SUPPORT.allows_dynamic_source() {
        Runtime::new_with_builtins(SystemHost, &BUILTINS)
    } else {
        quench_runtime::install_intl();
        Runtime::new_static_with_builtins(SystemHost, &BUILTINS)
    };
    runtime.set_execution_mode(if SUPPORT.allows_dynamic_source() { ExecutionMode::Compiled } else { ExecutionMode::NativeOnly });
    runtime.set_memory_policy(policy);
    phase(started, "runtime ready");
    runtime.set_native_program_resolver(move |name, parts| {
        // SAFETY: this resolver belongs to the same compiler-owned image.
        unsafe { resolve(&namespace, name, parts) }
    });
    let program = entry.residual();
    let executed = runtime.execute_native_deferred_jobs(&entry);
    phase(started, "entry ran");
    match executed {
        Ok(()) => {
            runtime.run_host_jobs(program).map_err(|error| uncaught(&error))?;
            runtime.finish_deferred_execution(program).map_err(|error| uncaught(&error))?;
        },
        Err(error) => return Err(uncaught(&error)),
    }
    if runtime.module_evaluation_pending(program).map_err(|error| uncaught(&error))? {
        return Ok(std::process::ExitCode::from(quench_runtime::UNSETTLED_TOP_LEVEL_AWAIT_EXIT));
    }
    Ok(std::process::ExitCode::SUCCESS)
}

/// An uncaught guest exception ends the process with status 1 and the error on stderr, as the
/// Node host's uncaught path does.
fn uncaught(error: &quench_runtime::JsError) -> String {
    format!("Uncaught {error}")
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
