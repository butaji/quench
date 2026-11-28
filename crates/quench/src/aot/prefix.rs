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
//!
//! One gate serves both hosts. The first input a watched run reads ends that run
//! (`quench_runtime::clock::watch_inputs`), and nothing past it is kept: the Node gate's `refuse`
//! and `stop` end a run at a host operation before it runs, and a core probe (`Probe`,
//! `core_rest::probe`) ends its run at the input the runtime notes, and makes no output of its own.
//! What a build keeps is the state before its first input: a core probe's input native captures the
//! frames that called it, and the run ends with them as its `Captured` exit (`vm::exit::Exit`).
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The host operations a build-time run may make: they read and write only the realm and
/// the host's own state, write the process's output (`processStreamWrite`), which the run
/// records (`record`), or answer whether a stream is a terminal as the run assumes
/// (`processStreamTerminal`, `answer`). Every other operation is a dynamic input.
const STATIC_OPERATIONS: &[&str] = &[
    // `require` and its builtin views load the program's own modules and builtins, which
    // a strict graph names statically: the sources the image is compiled from.
    "asyncTickInvoke",
    "bufferFrom",
    "builtinExportValues",
    "consoleGlobalInitialize",
    "consoleWrite",
    "consoleWritePlain",
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
static STATE: Mutex<State> = Mutex::new(State { effects: Vec::new(), refusal: None, answers: Vec::new(), choice: None, rest: None, held: (Vec::new(), Vec::new(), Vec::new()) });

struct State {
    effects: Vec<(i32, Vec<u8>)>,
    refusal: Option<String>,
    /// The answers this run assumes to the questions it may ask (`answer`).
    answers: Vec<(Question, bool)>,
    /// A question the run asked with no answer assumed.
    choice: Option<Question>,
    /// The runtime as the run left it at its first wait for input (`reach_wait`).
    rest: Option<Vec<u8>>,
    /// The host operations the saved runtime holds (`reach_wait`).
    held: HeldOperations,
}

/// A yes-or-no dynamic input: a run per answer specializes it, and the image asks it as it
/// starts to choose among them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Question {
    /// Whether the descriptor is a terminal.
    Terminal(i32),
    /// Whether the environment variable is set to the value.
    Environment(String, String),
}

impl Question {
    /// The question's answer in this process, as an image asks it as it starts.
    pub(crate) fn holds(&self) -> bool {
        match self {
            // SAFETY: `isatty` reads only the descriptor number.
            Self::Terminal(fd) => unsafe { libc::isatty(*fd) == 1 },
            Self::Environment(name, value) => std::env::var_os(name).is_some_and(|set| set == value.as_str()),
        }
    }

    /// The question as its key: `tty:FD` or `env:NAME=VALUE`.
    pub(crate) fn key(&self) -> String {
        match self {
            Self::Terminal(fd) => format!("tty:{fd}"),
            Self::Environment(name, value) => format!("env:{name}={value}"),
        }
    }

    pub(crate) fn of_key(key: &str) -> Result<Self, String> {
        let malformed = || format!("malformed build-time question {key}");
        match key.split_once(':').ok_or_else(malformed)? {
            ("tty", fd) => Ok(Self::Terminal(fd.parse().map_err(|_| malformed())?)),
            ("env", binding) => {
                let (name, value) = binding.split_once('=').ok_or_else(malformed)?;
                Ok(Self::Environment(name.to_owned(), value.to_owned()))
            }
            _ => Err(malformed()),
        }
    }
}

/// The host operations a saved runtime's host functions run, and apart, per set of globals no
/// loaded code names, those only the global object holds for them
/// (`quench_runtime::Runtime::held_host_operations`); then the opcodes its programs run
/// (`quench_runtime::Runtime::held_opcodes`).
pub(crate) type HeldOperations = (Vec<u32>, Vec<(Vec<String>, Vec<u32>)>, Vec<u16>);

/// What a build-time run left: its effects in order; a question it asked with no answer
/// assumed, which a run per answer specializes; or a dynamic input it read, named. A captured
/// caller chain is the run's own exit (`vm::exit::Exit::Captured`), not a residual.
pub(crate) enum Residual {
    Effects(Vec<(i32, Vec<u8>)>),
    /// The run's effects up to its first wait for input, the runtime there as a snapshot, and
    /// the host operations its host functions run.
    Waiting(Vec<(i32, Vec<u8>)>, Vec<u8>, HeldOperations),
    Question(Question),
    Dynamic(String),
}

/// Makes this process's entry run a build-time evaluation, which assumes each of `answers`.
pub(crate) fn begin(answers: Vec<(Question, bool)>) {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).answers = answers;
    ACTIVE.store(true, Ordering::Relaxed);
}

/// The answer to `question` the run assumes; with none, notes the question and asks the
/// caller to end the run (`stop`) with `None`.
pub(crate) fn answer(question: Question) -> Option<bool> {
    let mut state = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let assumed = state.answers.iter().find(|(assumed, _)| *assumed == question).map(|(_, value)| *value);
    if assumed.is_none() {
        state.choice.get_or_insert(question);
    }
    assumed
}

/// Whether `process.env` still holds this process's environment unread: a question about
/// a variable then asks about the environment the image runs in.
pub(crate) fn environment_unread(context: &mut NativeContext<'_>) -> Result<bool, RootedError> {
    let process = crate::modules::process_shared_vm::process_root(context)?;
    context.is_pending_lazy_data_property_rooted(process, "env")
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
    quench_runtime::clock::watch_inputs(context.execution_interrupt_handle());
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

/// The dynamic input host operation `name` reads, as a diagnostic names it.
pub(crate) fn input_of(name: &str) -> String {
    match name {
        "processIdentityInitialize" => "the process identity (process.argv, execPath, pid)".into(),
        "processEnvInitialize" => "the environment (process.env)".into(),
        _ => format!("the host operation {name}"),
    }
}

/// Notes a dynamic input the run reads by a refusal: the first refusal is the one named.
pub(crate) fn refuse(what: String) {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).refusal.get_or_insert(what);
}

/// The callback the gate runs in place of a refused operation: it ends the run, which a
/// guest `try` cannot catch, before the operation has any effect.
pub(crate) fn refused(context: &mut NativeContext<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Err(stop(context))
}

/// Records a write of `bytes` to the process's descriptor `fd`, in place of making it.
/// Makes again a write a build-time run recorded (`record`), as a strict image's replay makes it:
/// a closed or failing descriptor ends it, as Node's stdio writes ignore such a failure.
pub(crate) fn replay(fd: i32, bytes: &[u8]) {
    let mut written = 0;
    while written < bytes.len() {
        // SAFETY: the range lies inside `bytes`.
        let count = unsafe { libc::write(fd, bytes[written..].as_ptr().cast(), bytes.len() - written) };
        if count <= 0 {
            return;
        }
        written += count as usize;
    }
}

pub(crate) fn record(fd: i32, bytes: Vec<u8>) {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).effects.push((fd, bytes));
}

/// What the run left once it ended.
pub(crate) fn finish() -> Residual {
    let mut state = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(refusal) = state.refusal.take() {
        return Residual::Dynamic(refusal);
    }
    if let Some(question) = state.choice.take() {
        return Residual::Question(question);
    }
    let effects = std::mem::take(&mut state.effects);
    match state.rest.take() {
        Some(rest) => Residual::Waiting(effects, rest, std::mem::take(&mut state.held)),
        None => Residual::Effects(effects),
    }
}

/// The error a build-time run ends its event loop with at its first wait for input.
pub(crate) const REACHED_WAIT: &str = "build-time evaluation reached its first wait for input";

/// The event loop is about to wait for input, with nothing running and no job pending: the
/// run ends here, keeping the runtime as a snapshot (`Residual::Waiting`), which an image
/// restores to wait for the same input.
pub(crate) fn reach_wait(runtime: &mut quench_runtime::Runtime<crate::NodeHost>, program: &quench_runtime::ResidualProgram) -> String {
    match runtime.snapshot_at_rest(program, |host, writer| host.write_snapshot(writer, true)) {
        Ok(rest) => {
            let mut state = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.rest = Some(rest);
            let (operations, lazy) = runtime.held_host_operations();
            state.held = (operations, lazy, runtime.held_opcodes());
            REACHED_WAIT.to_owned()
        }
        Err(error) => {
            refuse(format!("a wait for input whose state cannot be saved ({error})"));
            error
        }
    }
}

/// The core half of the gate: the host a core build-time probe runs its entry on
/// (`core_rest::probe`). An output is an input the runtime notes before it is made
/// (`HostContext::invoke`), and the gate's handler ends the run there, so the probe makes no
/// output: `write_line` and `write_err` discard what they are given. A clock read is the
/// same kind of input, and the run ends at it too.
pub(crate) struct Probe;

impl quench_runtime::Host for Probe {
    fn write_line(&mut self, _: &str) {}

    fn write_err(&mut self, _: &str) {}

    fn clock_millis(&mut self) -> f64 {
        quench_runtime::clock::unix_millis()
    }
}
