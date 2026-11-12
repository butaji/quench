//! The code generator's own thread: compiles run beside the program, which polls for
//! their code and runs on in the lane meanwhile, as V8 and JSC compile their optimizing
//! tiers. A synchronous compile stalled the program for all of it: Richards' 19K-instruction
//! lane body took 300 ms the first time and each of its four recompiles 60-90 ms, which made
//! its first `--jit` iteration six times slower than the interpreter's (x86_64, 2026-11-07).

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::task::Poll;

type Work = Box<dyn FnOnce() + Send>;

/// Work handed to the code generator's thread, whose result arrives later.
pub(in crate::vm) struct Pending<T> {
    result: Receiver<T>,
}

impl<T> Pending<T> {
    /// The result once the work has run; `Ready(None)` if the thread is gone.
    pub(in crate::vm) fn poll(&self) -> Poll<Option<T>> {
        match self.result.try_recv() {
            Ok(result) => Poll::Ready(Some(result)),
            Err(TryRecvError::Empty) => Poll::Pending,
            Err(TryRecvError::Disconnected) => Poll::Ready(None),
        }
    }
}

/// Runs `work` on the code generator's thread, started on the first call.
pub(in crate::vm) fn run_in_background<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
    static QUEUE: std::sync::OnceLock<Option<Sender<Work>>> = std::sync::OnceLock::new();
    let (deliver, result) = std::sync::mpsc::sync_channel(1);
    let job: Work = Box::new(move || {
        // The program may have dropped its pending work; nothing waits for the result then.
        let _ = deliver.send(work());
    });
    // Tests read what a compile published right after the entry that started it, and a
    // fixed-work measurement (`QUENCH_DEV=sync-compile`) sees the code a run installs.
    if cfg!(test) || crate::trace::Dev::SyncCompile.on() {
        job();
        return Pending { result };
    }
    let queue = QUEUE.get_or_init(|| {
        let (queue, jobs) = std::sync::mpsc::channel::<Work>();
        std::thread::Builder::new()
            .name("quench-codegen".into())
            .spawn(move || serve(&jobs))
            .ok()
            .map(|_| queue)
    });
    // Without its thread, the work runs now: the caller polls a result that is ready.
    match queue {
        Some(queue) => {
            if let Err(std::sync::mpsc::SendError(job)) = queue.send(job) {
                job();
            }
        }
        None => job(),
    }
    Pending { result }
}

/// The code generator's loop. Its large scratch lives in its own arena (`quench_alloc::adopt_scratch_thread`),
/// not glibc's, and goes back once per idle period, when the queue has drained: a release after each compile
/// made ~2,300 `madvise` calls (and `malloc_trim` more) in V8-v7 earley-boyer, each a TLB shootdown that
/// interrupted the program's thread (x86_64, 2026-11-12).
fn serve(jobs: &Receiver<Work>) {
    quench_alloc::adopt_scratch_thread();
    loop {
        match jobs.try_recv() {
            Ok(job) => job(),
            Err(TryRecvError::Empty) => {
                release_compile_scratch();
                match jobs.recv() {
                    Ok(job) => job(),
                    Err(_) => return,
                }
            }
            Err(TryRecvError::Disconnected) => return,
        }
    }
}

/// Returns the code generator's idle pages to the system, and the code zone's batched writer-view retirements.
fn release_compile_scratch() {
    quench_alloc::release_empty_pages();
    quench_alloc::release_scratch();
    crate::vm::compiled::flush_retired_writers();
}
