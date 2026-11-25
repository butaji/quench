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
/// the host's own state, write the process's output (`processStreamWrite`), which the run
/// records (`record`), or answer whether a stream is a terminal as the run assumes
/// (`processStreamTerminal`, `terminal`). Every other operation is a dynamic input.
const STATIC_OPERATIONS: &[&str] = &[
    // `require` and its builtin views load the program's own modules and builtins, which
    // a strict graph names statically: the sources the image is compiled from.
    "asyncTickInvoke",
    "bufferFrom",
    "builtinExportValues",
    "consoleGlobalInitialize",
    "consoleWrite",
    "moduleParentSet",
    "moduleRunMain",
    "processEmitExitEvent",
    // The exit status, which the image exits with.
    "processExit",
    "processExitCodeGet",
    "processExitCodeSet",
    "processGlobalGet",
    "processGlobalSet",
    // process.stdin is a plain Readable whose operations keep host state: asking for input
    // (`processStdinRead`) only marks a read wanted, and the event loop's wait for it is
    // where the evaluation ends (`process_stdin_shared_vm::poll`).
    "processStdin",
    "processStdinDestroy",
    "processStdinPause",
    "processStdinRead",
    "processStdinRef",
    "processStreamTerminal",
    "processStreamWrite",
    "reallyExit",
    "require",
];

/// Whether this process evaluates an entry at build time (`begin`).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the entry's own code runs (`arm`): the host's start-up before it is the realm's.
static ARMED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State { effects: Vec::new(), refusal: None, terminals: Vec::new(), choice: None, rest: None });

struct State {
    effects: Vec<(i32, Vec<u8>)>,
    refusal: Option<String>,
    /// The answers this run assumes for whether a descriptor is a terminal (`terminal`).
    terminals: Vec<(i32, bool)>,
    /// A descriptor whose terminal answer the run read with none assumed.
    choice: Option<i32>,
    /// The runtime as the run left it at its first wait for input (`reach_wait`).
    rest: Option<Vec<u8>>,
}

/// What a build-time run left: its effects in order; a finite dynamic input it read, which a
/// run per value specializes (whether descriptor `fd` is a terminal), the image choosing among
/// them as it starts; or the first other dynamic input it read.
pub(crate) enum Residual {
    Effects(Vec<(i32, Vec<u8>)>),
    /// The run's effects up to its first wait for input, and the runtime there as a snapshot.
    Waiting(Vec<(i32, Vec<u8>)>, Vec<u8>),
    Terminal(i32),
    Dynamic(String),
}

/// Makes this process's entry run a build-time evaluation, which assumes each of `terminals`
/// (a descriptor, and whether it is a terminal).
pub(crate) fn begin(terminals: Vec<(i32, bool)>) {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).terminals = terminals;
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Whether descriptor `fd` is a terminal, as the run assumes; with no assumption, notes the
/// choice and ends the run (`stop`), which `None` asks the caller to do.
pub(crate) fn terminal(fd: i32) -> Option<bool> {
    let mut state = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let assumed = state.terminals.iter().find(|(assumed, _)| *assumed == fd).map(|(_, value)| *value);
    if assumed.is_none() {
        state.choice.get_or_insert(fd);
    }
    assumed
}

/// Ends the run, before whatever it was about to do: a guest `try` cannot catch it.
pub(crate) fn stop(context: &mut NativeContext<'_>) -> RootedError {
    context.execution_interrupt_handle().request_termination();
    match context.check_execution_interrupt() {
        Err(error) => error,
        Ok(()) => RootedError::host("build-time evaluation stopped"),
    }
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
    for name in ["stdout", "stderr"] {
        let key = context.string_rooted(name);
        let stream = context.get_property_rooted(process, key)?;
        if !context.is_pending_lazy_data_property_rooted(stream, "isTTY")? {
            refuse(format!("process.{name}.isTTY (read during start-up)"));
        }
    }
    quench_runtime::clock::watch_inputs();
    ARMED.store(true, Ordering::Relaxed);
    Ok(())
}

/// The host modules whose every operation reads and writes only the realm and the host's own
/// state, by operation name prefix: events, streams and readline (`readline_shared_vm`
/// notes its clock reads as inputs). What they reach outside the realm they reach through
/// other host operations, which pass the gate themselves.
const STATIC_MODULES: &[&str] = &["emitter", "readline", "stream"];

/// Whether the gate lets host operation `name` run.
pub(crate) fn admits(name: &str) -> bool {
    STATIC_OPERATIONS.contains(&name) || STATIC_MODULES.iter().any(|module| name.starts_with(module))
}

/// Notes the first dynamic input of the run.
pub(crate) fn refuse(what: String) {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).refusal.get_or_insert(what);
}

/// The callback the gate runs in place of a refused operation: it ends the run, which a
/// guest `try` cannot catch, before the operation has any effect.
pub(crate) fn refused(context: &mut NativeContext<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Err(stop(context))
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
    if let Some(fd) = state.choice {
        return Residual::Terminal(fd);
    }
    if let Some(input) = quench_runtime::clock::first_input() {
        return Residual::Dynamic(input.to_owned());
    }
    let effects = std::mem::take(&mut state.effects);
    match state.rest.take() {
        Some(rest) => Residual::Waiting(effects, rest),
        None => Residual::Effects(effects),
    }
}

/// The error a build-time run ends its event loop with at its first wait for input.
pub(crate) const REACHED_WAIT: &str = "build-time evaluation reached its first wait for input";

/// The event loop is about to wait for input, with nothing running and no job pending: the
/// run ends here, keeping the runtime as a snapshot (`Residual::Waiting`), which an image
/// restores to wait for the same input.
pub(crate) fn reach_wait(runtime: &mut quench_runtime::Runtime<crate::NodeHost>, program: &quench_runtime::ResidualProgram) -> String {
    match runtime.snapshot_at_rest(program, |host, writer| host.write_snapshot(writer)) {
        Ok(rest) => {
            STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).rest = Some(rest);
            REACHED_WAIT.to_owned()
        }
        Err(error) => {
            refuse(format!("a wait for input whose state cannot be saved ({error})"));
            error
        }
    }
}
