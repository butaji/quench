//! The code generator's own thread: compiles run beside the program, which polls for
//! their code and runs on in the lane meanwhile, as V8 and JSC compile their optimizing
//! tiers. A synchronous compile stalled the program for all of it: Richards' 19K-instruction
//! lane body took 300 ms the first time and each of its four recompiles 60-90 ms, which made
//! its first `--jit` iteration six times slower than the interpreter's (x86_64, 2026-11-07).

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::task::Poll;

type Work = Box<dyn FnOnce() + Send>;

/// Work handed to the code generator's thread, whose result arrives later.
pub struct Pending<T> {
    result: Receiver<T>,
}

impl<T> Pending<T> {
    /// The result once the work has run; `Ready(None)` if the thread is gone.
    pub fn poll(&self) -> Poll<Option<T>> {
        match self.result.try_recv() {
            Ok(result) => Poll::Ready(Some(result)),
            Err(TryRecvError::Empty) => Poll::Pending,
            Err(TryRecvError::Disconnected) => Poll::Ready(None),
        }
    }
}

/// Signals that work handed to the code generator's thread is done, so what it borrowed may go
/// (`Done::wait`): the program keeps the data a job reads by pointer until then.
pub(in crate::vm) struct Done(Receiver<()>);

impl Done {
    /// Waits for the work to finish; at once where it has (or never will: its thread is gone).
    pub(in crate::vm) fn wait(&self) {
        let _ = self.0.recv();
    }
}

impl Drop for Done {
    fn drop(&mut self) {
        self.wait();
    }
}

/// Runs `work` on the code generator's thread as `run_in_background` does, with a `Done` whose
/// drop waits for it: for work that reads data the program owns by pointer.
pub(in crate::vm) fn run_in_background_guarded<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> (Pending<T>, Done) {
    let (finished, done) = std::sync::mpsc::sync_channel(1);
    let pending = run_in_background(move || {
        let result = work();
        let _ = finished.send(());
        result
    });
    (pending, Done(done))
}

/// A value moved to the code generator's thread and back, which is `Send` though its type is
/// not (a lowering's job and result): it shares no reference count with a value the program's
/// thread keeps (a job's program is a detached copy, `ResidualProgram::detached`; the direct
/// callees it names it reads by pointer and never clones), and one thread at a time uses it.
pub(in crate::vm) struct AssertSend<T>(pub(in crate::vm) T);

// SAFETY: as the type's comment says: no part of it is counted by a value of another thread, and
// the pointers it holds name data the program's thread keeps alive and leaves unchanged until
// the work is done (`OffThread`'s guard).
unsafe impl<T> Send for AssertSend<T> {}

/// The one shape every compile driver takes (a lane body, a loop region, an image): its work,
/// lowering and code generation, runs on the code generator's thread from owned inputs, and the
/// program polls for the result between runs. Where the work reads data the program owns by
/// pointer (a plan's direct callees), dropping the driver waits for it (`guarded`).
pub struct OffThread<T> {
    /// The work's end, which a drop waits for. First, so it drops before what the work reads.
    done: Option<Done>,
    result: Pending<AssertSend<T>>,
}

impl<T: 'static> OffThread<T> {
    /// `work` on the code generator's thread, reading nothing the program owns.
    pub fn start<W: 'static>(work: W, run: fn(W) -> T) -> Self {
        let work = AssertSend(work);
        Self { done: None, result: run_in_background(move || { let held = work; let AssertSend(work) = held; AssertSend(run(work)) }) }
    }

    /// `work` on the code generator's thread, which reads data the program keeps alive until
    /// it ends: dropping the driver waits for it.
    pub fn guarded<W: 'static>(work: W, run: fn(W) -> T) -> Self {
        let work = AssertSend(work);
        let (result, done) = run_in_background_guarded(move || { let held = work; let AssertSend(work) = held; AssertSend(run(work)) });
        Self { done: Some(done), result }
    }

    /// A result already in hand (a tier that builds its code at once), polled as any other.
    pub fn ready(result: T) -> Self {
        Self::start(result, std::convert::identity)
    }

    /// The result once the work has run; `Ready(None)` if its thread is gone. Once ready,
    /// what the work read may go.
    pub fn poll(&mut self) -> Poll<Option<T>> {
        let result = self.result.poll();
        if result.is_ready() {
            self.done = None;
        }
        result.map(|result| result.map(|AssertSend(result)| result))
    }
}

/// The stack the code generator's thread holds. The lane lowering's Cranelift pipeline used more
/// than a spawned thread's default 2 MiB on this host (the lane tests overflowed there), and the
/// generator's own scratch lives in its arena (`quench_alloc::adopt_scratch_thread`), so this is
/// the lowering's frames, not its data.
const CODEGEN_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Runs `work` on the code generator's thread, started on the first call.
pub fn run_in_background<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
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
            .stack_size(CODEGEN_STACK_BYTES)
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
    // Every free block of a page or more, not only those of 64 KiB: the small classes a kept code vector pins held
    // ~38 MB between compiles on the data-structure suite (TOD-2292); this runs once per idle period.
    quench_alloc::release_scratch_phase();
    crate::vm::engine_calls::flush_retired_writers();
}
