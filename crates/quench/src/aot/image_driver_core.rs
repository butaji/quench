//! The pure-ECMAScript image driver: the same compiled program and image tables as the Node
//! driver, executed on the runtime's own host. It links `quench_runtime` alone, so the
//! executable carries no Node host and no operation table. It starts by mapping the dump its
//! build made (`image_dump::adopt`): the realm and the entry at rest, with no installation and
//! no decoding. Its build dumps that state (`start_state`) before the image is published.
#![no_main]

use quench_runtime::native_link::image_dump;
use quench_runtime::tier_surface::Native;
use quench_runtime::{ExecutionMode, NativeJsProgram, RealmRoots, Runtime, SystemHost};
use std::collections::BTreeSet;

// Every runtime layer (engine, intl, stencils) links through this facade: the core calls their
// `extern "Rust"` symbols, so a binary that uses the core must name the crate.
extern crate quench_layers;

/// The runtime's size-classed small-object allocator (`quench_alloc`), which the Node driver
/// also installs (`quench::GLOBAL_ALLOCATOR`).
#[cfg(unix)]
#[global_allocator]
static GLOBAL_ALLOCATOR: quench_alloc::QuenchAlloc = quench_alloc::QuenchAlloc::new();

include!(env!("QUENCH_NATIVE_CATALOG"));

/// The state an image starts from: its runtime at rest and the entry it runs.
type Rest = (Runtime<SystemHost>, NativeJsProgram);

/// Serve small requests from Quench's allocator where the build chose it (`ALLOCATOR_SERVES_SMALL`,
/// written into the catalog at compile time), as the Node host's `allocator_policy` does.
fn apply_allocator_policy() {
    #[cfg(unix)]
    if ALLOCATOR_SERVES_SMALL {
        quench_alloc::QuenchAlloc::enable();
    }
}

/// The state the image starts from: the dump its build made, mapped back where it was made. A
/// build that dumps its state (`image_dump::requested_wait`) writes it here and ends. An image
/// without a dump, or whose addresses are taken, builds the state it would have mapped.
fn start_state() -> Result<Box<Rest>, String> {
    if let Some(root) = image_dump::adopt(0) {
        // SAFETY: the dump's root is the `Rest` its build leaked, mapped back at its address.
        return Ok(unsafe { Box::from_raw(root.cast::<Rest>()) });
    }
    // SAFETY: the compiler emits the catalog, metadata and objects together.
    let program = unsafe { bind_entry() }?;
    let mut runtime = Runtime::new_static_with_builtins(SystemHost, &BUILTINS, &EXECUTION);
    runtime.set_execution_mode(ExecutionMode::NativeOnly);
    runtime.set_memory_policy(MEMORY_POLICY);
    runtime.install_native_at_rest(&program).map_err(|error| uncaught(&error))?;
    let mut rest = Box::new((runtime, program));
    if image_dump::requested_wait().is_some() {
        // The dump keeps the realm the closed world reaches: the closure of the program over the
        // global names the build recorded and the natives this image keeps (`close`).
        let globals: BTreeSet<Box<str>> = CLOSED_GLOBALS.iter().map(|name| Box::from(*name)).collect();
        let natives: BTreeSet<Native> = BUILTINS.natives.unwrap_or(&[]).iter().copied().collect();
        let (runtime, program) = &mut *rest;
        runtime
            .collect_closed_world(program.residual(), RealmRoots { globals: &globals, natives: &natives })
            .map_err(|error| uncaught(&error))?;
        image_dump::write(0, Box::into_raw(rest).cast());
    }
    Ok(rest)
}

fn run() -> Result<u8, String> {
    apply_allocator_policy();
    let mut rest = start_state()?;
    let (runtime, program) = &mut *rest;
    runtime.execute_restored(program.residual()).map_err(|error| uncaught(&error))?;
    runtime.run_host_jobs(program.residual()).map_err(|error| uncaught(&error))?;
    runtime.finish_deferred_execution(program.residual()).map_err(|error| uncaught(&error))?;
    if runtime.module_evaluation_pending(program.residual()).map_err(|error| uncaught(&error))? {
        return Ok(quench_runtime::UNSETTLED_TOP_LEVEL_AWAIT_EXIT);
    }
    Ok(0)
}

/// An uncaught guest exception ends the process with status 1 and the error on stderr, as the
/// Node host's uncaught path does.
fn uncaught(error: &quench_runtime::JsError) -> String {
    format!("Uncaught {error}")
}

/// The process's start. `#![no_main]` leaves out std's runtime start: the program runs on a
/// fixed-stack worker (`image_dump::run_on_fixed_stack`), so a dump's thread-locals lie at the
/// address they had, with no std stack-overflow handler. Its buffered standard output is
/// flushed by `exit_process`, which then ends the process with `_exit`.
#[unsafe(no_mangle)]
extern "C" fn main(_: std::ffi::c_int, _: *const *const std::ffi::c_char) -> std::ffi::c_int {
    quench_runtime::host::begin_process();
    image_dump::begin();
    let status = image_dump::run_on_fixed_stack(|| match run() {
        Ok(code) => code,
        Err(error) => {
            quench_runtime::host::flush_stdout();
            eprintln!("{error}");
            1
        }
    });
    quench_runtime::host::exit_process(status.unwrap_or(1))
}
