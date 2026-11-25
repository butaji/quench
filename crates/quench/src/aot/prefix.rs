//! Strict AOT's first stage of partial evaluation: the entry runs at build time, against the
//! build-time realm, until it reads its first dynamic input. What it computes from static
//! inputs alone (its sources, the realm, constants) is the same in every later run, so the
//! image keeps only its effects. A run that reads no dynamic input at all leaves a residual
//! program of effects alone: the bytes it wrote to stdout and stderr, in order, and its exit
//! status, which an image replays with no engine.
//!
//! A dynamic input is anything a later run may see differently: a host operation outside
//! `STATIC_OPERATIONS` (files, the terminal, timers, the network, child processes, ...), which
//! the gate refuses before it runs, so a build never makes the program's effects; the process
//! identity and environment (`process.argv`, `process.env`, ...), which stay lazy and are
//! host operations too; and the clock, the local time zone and `Math.random`, which the
//! runtime notes (`quench_runtime::clock::note_input`).
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The host operations a build-time run may make: they read and write only the realm and
/// the host's own state, or write the process's output (`processStreamWrite`), which the
/// run records (`record`). Every other operation is a dynamic input.
const STATIC_OPERATIONS: &[&str] = &[
    // `require` and its builtin views load the program's own modules and builtins, which
    // a strict graph names statically: the sources the image is compiled from.
    "builtinExportValues",
    "consoleGlobalInitialize",
    "consoleWrite",
    "emitterProtoEmit",
    "moduleParentSet",
    "moduleRunMain",
    "processEmitExitEvent",
    // The exit status, which the image exits with.
    "processExit",
    "processExitCodeGet",
    "processExitCodeSet",
    "processGlobalGet",
    "processGlobalSet",
    "processStreamWrite",
    "reallyExit",
    "require",
];

/// Whether this process evaluates an entry at build time (`begin`).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the entry's own code runs (`arm`): the host's start-up before it is the realm's.
static ARMED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State { effects: Vec::new(), refusal: None });

struct State {
    effects: Vec<(i32, Vec<u8>)>,
    refusal: Option<String>,
}

/// What a build-time run left: its effects in order, or the first dynamic input it read.
pub(crate) enum Residual {
    Effects(Vec<(i32, Vec<u8>)>),
    Dynamic(String),
}

/// Makes this process's entry run a build-time evaluation.
pub(crate) fn begin() {
    ACTIVE.store(true, Ordering::Relaxed);
}

#[inline]
pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

#[inline]
pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// The entry's own code starts: from here every host operation passes the gate, and the
/// runtime notes the inputs it reads. The process identity and environment must still be
/// unread: start-up that read them would have handed the entry this process's values.
pub(crate) fn arm(context: &mut NativeContext<'_>) -> Result<(), RootedError> {
    if !active() {
        return Ok(());
    }
    let process = crate::modules::process_shared_vm::process_root(context)?;
    for name in ["env", "argv"] {
        if !context.is_pending_lazy_data_property_rooted(process, name)? {
            refuse(format!("process.{name} (read during start-up)"));
        }
    }
    quench_runtime::clock::watch_inputs();
    ARMED.store(true, Ordering::Relaxed);
    Ok(())
}

/// Whether the gate lets host operation `name` run.
pub(crate) fn admits(name: &str) -> bool {
    STATIC_OPERATIONS.contains(&name)
}

/// Notes the first dynamic input of the run.
pub(crate) fn refuse(what: String) {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).refusal.get_or_insert(what);
}

/// The callback the gate runs in place of a refused operation: it ends the run, which a
/// guest `try` cannot catch, before the operation has any effect.
pub(crate) fn refused(context: &mut NativeContext<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    context.execution_interrupt_handle().request_termination();
    context.check_execution_interrupt()?;
    Err(RootedError::host("build-time evaluation refused a dynamic input"))
}

/// Records a write of `bytes` to the process's descriptor `fd`, in place of making it.
pub(crate) fn record(fd: i32, bytes: Vec<u8>) {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).effects.push((fd, bytes));
}

/// What the run left once it ended.
pub(crate) fn finish() -> Residual {
    let mut state = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(refusal) = state.refusal.take() {
        return Residual::Dynamic(refusal);
    }
    if let Some(input) = quench_runtime::clock::first_input() {
        return Residual::Dynamic(input.to_owned());
    }
    Residual::Effects(std::mem::take(&mut state.effects))
}
