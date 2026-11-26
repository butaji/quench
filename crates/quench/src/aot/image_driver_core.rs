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
    let mut runtime = core_runtime();
    runtime.set_memory_policy(policy);
    phase(started, "runtime ready");
    runtime.set_native_program_resolver(move |name, parts| {
        // SAFETY: this resolver belongs to the same compiler-owned image.
        unsafe { resolve(&namespace, name, parts) }
    });
    finish(runtime, &entry, started)
}

/// The runtime a core image runs on. A source-fronted core (`Variant::CoreDynamic`) links the
/// interpreter and the source frontend but no JIT: `new_source_with_builtins` installs the source
/// routes alone, and code it makes at run time runs in the interpreter. A closed core runs only
/// its image's code and installs no source route. Both take the Intl layer the support carries.
fn core_runtime() -> Runtime<SystemHost> {
    quench_runtime::install_intl();
    #[cfg(quench_native_source)]
    let mut runtime = Runtime::new_source_with_builtins(SystemHost, &BUILTINS);
    #[cfg(not(quench_native_source))]
    let mut runtime = Runtime::new_static_with_builtins(SystemHost, &BUILTINS);
    // A body compiled ahead is hot by construction: the image's bodies run compiled, and the JIT's
    // routes are absent, so source compiled at run time stays in the interpreter.
    #[cfg(quench_native_source)]
    runtime.set_execution_mode(ExecutionMode::Compiled);
    #[cfg(not(quench_native_source))]
    runtime.set_execution_mode(ExecutionMode::NativeOnly);
    runtime
}

/// Runs the entry and the jobs it left, then reports an unsettled top-level await as the Node
/// driver does.
fn finish(
    mut runtime: Runtime<SystemHost>,
    entry: &quench_runtime::NativeJsProgram,
    started: std::time::Instant,
) -> Result<std::process::ExitCode, String> {
    let program = entry.residual();
    let executed = runtime.execute_native_deferred_jobs(entry);
    phase(started, "entry ran");
    match executed {
        Ok(()) => {
            runtime.run_host_jobs(program).map_err(|error| uncaught(&error))?;
            phase(started, "host jobs ran");
            runtime.finish_deferred_execution(program).map_err(|error| uncaught(&error))?;
            phase(started, "deferred finished");
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
    // Every recursive runtime transition is bounded by `quench_stack`'s policy, which reserves
    // `WORKER_STACK_SIZE`; the host's main thread reserves less, as the Node driver's own worker
    // records.
    let worker = std::thread::Builder::new()
        .name("quench-core".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(run);
    match worker.map(|worker| worker.join()) {
        Ok(Ok(Ok(code))) => code,
        Ok(Ok(Err(error))) => failed(&error),
        Ok(Err(_)) => failed("core image worker panicked"),
        Err(error) => failed(&error.to_string()),
    }
}

fn failed(error: &str) -> std::process::ExitCode {
    eprintln!("{error}");
    std::process::ExitCode::FAILURE
}
