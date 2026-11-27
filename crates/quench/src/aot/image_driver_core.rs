//! The pure-ECMAScript image driver: the same compiled program and image tables as the Node
//! driver, executed on the runtime's own host. It links `quench_runtime` alone, so the
//! executable carries no Node host, no bootstrap snapshot and no operation table.
#![no_main]

use quench_runtime::{ExecutionMode, MemoryPolicy, NativeProgramNamespace, Runtime, SystemHost};

// Every runtime layer (engine, intl, stencils) links through this facade: the core calls their
// `extern "Rust"` symbols, so a binary that uses the core must name the crate.
extern crate quench_layers;

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

fn run() -> Result<u8, String> {
    // SAFETY: the compiler emits the catalog, metadata and objects together.
    let entry = unsafe { bind_entry() }?;
    let namespace = NativeProgramNamespace::from_entry(entry.residual())?;
    let policy = memory_policy();
    apply_allocator_policy(policy);
    let mut runtime = Runtime::new_static_with_builtins(SystemHost, &BUILTINS, &EXECUTION);
    runtime.set_execution_mode(ExecutionMode::NativeOnly);
    runtime.set_memory_policy(policy);
    runtime.set_native_program_resolver(move |name, parts| {
        // SAFETY: this resolver belongs to the same compiler-owned image.
        unsafe { resolve(&namespace, name, parts) }
    });
    let program = entry.residual();
    match runtime.execute_native_deferred_jobs(&entry) {
        Ok(()) => {
            runtime.run_host_jobs(program).map_err(|error| uncaught(&error))?;
            runtime.finish_deferred_execution(program).map_err(|error| uncaught(&error))?;
        },
        Err(error) => return Err(uncaught(&error)),
    }
    if runtime.module_evaluation_pending(program).map_err(|error| uncaught(&error))? {
        return Ok(quench_runtime::UNSETTLED_TOP_LEVEL_AWAIT_EXIT);
    }
    Ok(0)
}

/// An uncaught guest exception ends the process with status 1 and the error on stderr, as the
/// Node host's uncaught path does.
fn uncaught(error: &quench_runtime::JsError) -> String {
    format!("Uncaught {error}")
}

/// The process's start. `#![no_main]` leaves out std's runtime start: the program runs on the
/// process's main stack, with no std stack-overflow handler and no worker thread. Its buffered
/// standard output is flushed by `exit_process`, which then ends the process with `_exit`.
#[unsafe(no_mangle)]
extern "C" fn main(_: std::ffi::c_int, _: *const *const std::ffi::c_char) -> std::ffi::c_int {
    quench_runtime::host::begin_process();
    let status = match run() {
        Ok(code) => code,
        Err(error) => {
            quench_runtime::host::flush_stdout();
            eprintln!("{error}");
            1
        }
    };
    quench_runtime::host::exit_process(status)
}
