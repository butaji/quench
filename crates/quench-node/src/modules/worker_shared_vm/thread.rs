//! Worker threads: each runs a complete Node environment (its own `NodeHost`, VM and event
//! loop) on a dedicated thread, joined to its parent by a message channel. A shared
//! control block carries the lifecycle back: online, termination requests, and the exit
//! code with any fatal exception.

use super::ports::{Endpoint, Message};
use crate::host::NodeHost;
use crate::shared_run::{EntryGoal, SharedCompletion, SharedInput};
use quench_runtime::{NativeContext, RootId, RootedError, SerializedValue, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Node's exit code for a terminated worker.
pub(crate) const TERMINATED_EXIT_CODE: i32 = 1;
/// Node's exit code for a fatal exception.
const FATAL_EXIT_CODE: i32 = 1;
/// Node's exit code when the entry's top-level await never settles.
const UNSETTLED_TOP_LEVEL_AWAIT_EXIT_CODE: i32 = 13;
/// The `process.argv[1]` Node reports for an `eval` worker.
const EVAL_ENTRY_NAME: &str = "[worker eval]";

/// Thread ids: the main thread is 0; workers count from 1 in creation order.
static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

/// A worker's fatal exception: serialized for its parent, or only its description when
/// the thrown value cannot be serialized or no VM was running.
pub(crate) enum Failure {
    Thrown(SerializedValue),
    Message(String),
}

struct Exit {
    pub(crate) code: i32,
    pub(crate) error: Option<Failure>,
}

#[derive(Default)]
struct ControlState {
    online: bool,
    /// Bytes the worker wrote to stdout (`false`) or stderr (`true`), not yet delivered.
    output: std::collections::VecDeque<(bool, Vec<u8>)>,
    failure: Option<Failure>,
    exit: Option<Exit>,
}

/// The lifecycle a worker thread shares with its parent.
#[derive(Default)]
pub(crate) struct WorkerControl {
    state: Mutex<ControlState>,
    terminate: AtomicBool,
}

impl WorkerControl {
    fn lock(&self) -> MutexGuard<'_, ControlState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn set_online(&self) {
        self.lock().online = true;
    }

    pub(crate) fn is_online(&self) -> bool {
        self.lock().online
    }

    /// The next output chunk, or the exit once all output has been delivered.
    pub(crate) fn take_output(&self) -> Option<(bool, Vec<u8>)> {
        self.lock().output.pop_front()
    }

    /// The fatal exception of an exited worker, delivered before (and apart from) its exit.
    pub(crate) fn take_error(&self) -> Option<Failure> {
        let mut state = self.lock();
        if !state.output.is_empty() {
            return None;
        }
        state.exit.as_mut().and_then(|exit| exit.error.take())
    }

    /// The exit code, once all output and the fatal exception have been delivered.
    pub(crate) fn take_exit(&self) -> Option<i32> {
        let mut state = self.lock();
        if !state.output.is_empty() || state.exit.as_ref().is_some_and(|exit| exit.error.is_some()) {
            return None;
        }
        state.exit.take().map(|exit| exit.code)
    }

    pub(crate) fn write_output(&self, stderr: bool, bytes: Vec<u8>) {
        self.lock().output.push_back((stderr, bytes));
    }

    pub(crate) fn request_termination(&self) {
        self.terminate.store(true, Ordering::Release);
    }

    pub(crate) fn terminating(&self) -> bool {
        self.terminate.load(Ordering::Acquire)
    }

    fn record_failure(&self, failure: Failure) {
        self.lock().failure.get_or_insert(failure);
    }

    fn finish(&self, outcome: Result<SharedCompletion, String>) {
        let mut state = self.lock();
        let failure = state.failure.take();
        let (code, error) = match outcome {
            Ok(SharedCompletion::Completed) => (0, None),
            Ok(SharedCompletion::GuestExit { code }) => (code, None),
            Ok(SharedCompletion::PendingTopLevelAwait) => (UNSETTLED_TOP_LEVEL_AWAIT_EXIT_CODE, None),
            Err(message) => (FATAL_EXIT_CODE, Some(failure.unwrap_or(Failure::Message(message)))),
        };
        // A worker that came up ran JavaScript; one that failed earlier still reports online
        // first, as Node emits 'online' before 'error' and 'exit'.
        state.online = true;
        state.exit = Some(Exit { code, error });
    }
}

/// What a worker VM knows about itself; `parent` and `data` move into its
/// `worker_threads` module when that is first built.
pub(crate) struct WorkerContext {
    pub(crate) thread_id: u64,
    pub(crate) parent: Option<Arc<Endpoint>>,
    pub(crate) data: Option<Message>,
    pub(crate) control: Arc<WorkerControl>,
    /// The worker's `process.env` snapshot (its parent's, or `options.env`).
    pub(crate) env: Vec<(String, String)>,
}

/// Everything a worker thread needs from its parent.
pub(crate) struct Launch {
    pub(crate) filename: String,
    pub(crate) is_eval: bool,
    pub(crate) argv: Vec<String>,
    pub(crate) exec_path: String,
    pub(crate) exec_argv: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) data: Message,
    pub(crate) parent: Arc<Endpoint>,
}

/// Start a worker thread; returns its thread id and control block.
pub(crate) fn spawn(launch: Launch) -> Result<(u64, Arc<WorkerControl>), String> {
    let thread_id = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
    let control = Arc::new(WorkerControl::default());
    let thread_control = control.clone();
    std::thread::Builder::new()
        .name(format!("quench-worker-{thread_id}"))
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || run(thread_id, launch, thread_control))
        .map_err(|error| format!("worker thread: {error}"))?;
    Ok((thread_id, control))
}

fn run(thread_id: u64, launch: Launch, control: Arc<WorkerControl>) {
    // Process-directed signals belong to the main thread's event loop.
    #[cfg(unix)]
    {
        // SAFETY: a full mask applied to this thread only.
        unsafe {
            let mut mask = std::mem::zeroed::<libc::sigset_t>();
            libc::sigfillset(&mut mask);
            libc::pthread_sigmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut());
        }
    }
    let Launch { filename, is_eval, argv, exec_path, exec_argv, env, data, parent } = launch;
    let (input, entry) = if is_eval {
        let input = SharedInput::Eval { source: filename, goal: EntryGoal::CommonJs, exec_argv, print: false };
        (input, EVAL_ENTRY_NAME.to_owned())
    } else {
        let input = SharedInput::File { path: filename.clone().into(), exec_argv, goal: EntryGoal::Node };
        (input, filename)
    };
    let mut process_argv = vec![exec_path, entry];
    process_argv.extend(argv);
    let context = WorkerContext { thread_id, parent: Some(parent), data: Some(data), control: control.clone(), env };
    let outcome = crate::shared_run::execute_worker(input, process_argv, context);
    control.finish(outcome);
}

/// Record a worker's fatal exception for its parent before the VM reports it. Main-thread
/// VMs are unaffected.
pub(crate) fn capture_fatal(runtime: &mut quench_runtime::Runtime<NodeHost>, exception: Option<RootId>) {
    if runtime.host_mut().shared_state().borrow().messaging.context.is_none() {
        return;
    }
    let Ok(operation) = runtime.host_function(crate::host::shared_vm::operation("workerCaptureError")) else {
        return;
    };
    let receiver = runtime.root(Value::UNDEFINED);
    let argument = exception.unwrap_or(receiver);
    if let Ok(result) = runtime.call_rooted(operation, receiver, &[argument]) {
        runtime.release_root(result);
    }
    runtime.release_root(receiver);
    runtime.release_root(operation);
}

/// `workerCaptureError(exception)`: serialize a worker's fatal exception into its control.
pub(crate) fn capture_error(
    c: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let exception = args.first().copied().unwrap_or_else(|| c.undefined());
    let control = c.host_mut().shared_state().borrow().messaging.context.as_ref().map(|context| context.control.clone());
    if let Some(control) = control {
        let failure = match c.serialize_rooted(exception, &[], &[]) {
            Ok(value) => Failure::Thrown(value),
            Err(error) => {
                if let Some(thrown) = error.exception {
                    c.release_root(thrown);
                }
                Failure::Message(c.to_string(exception).unwrap_or_default())
            }
        };
        control.record_failure(failure);
    }
    Ok(c.undefined())
}
