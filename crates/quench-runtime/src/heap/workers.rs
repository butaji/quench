//! Parked helper threads for the collector's parallel phases.
//!
//! Spawning and joining helpers for every marking and sweeping phase costs more than the
//! phase's own start; the helpers instead wait on a condition variable between phases. A phase
//! borrows the heap only for the duration of `run`, which returns after every helper finished.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// A phase body, called with the worker index: 0 on the collecting thread, then each helper.
type Phase<'a> = dyn Fn(usize) + Sync + 'a;

struct State {
    /// The running phase with its lifetime erased; `run` keeps the borrow alive until every
    /// helper has returned from it.
    phase: Option<*const Phase<'static>>,
    generation: u64,
    running: usize,
    panicked: bool,
    shutdown: bool,
}

// SAFETY: the phase pointer is only dereferenced while `run` holds the referent alive, and the
// referent is `Sync`.
unsafe impl Send for State {}

struct Shared {
    state: Mutex<State>,
    start: Condvar,
    finished: Condvar,
}

pub(super) struct GcWorkers {
    shared: Arc<Shared>,
    helpers: Vec<JoinHandle<()>>,
}

impl GcWorkers {
    pub(super) fn new(helpers: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                phase: None,
                generation: 0,
                running: 0,
                panicked: false,
                shutdown: false,
            }),
            start: Condvar::new(),
            finished: Condvar::new(),
        });
        let helpers = (1..=helpers)
            .map(|index| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name("quench-gc".into())
                    .spawn(move || helper(&shared, index))
                    .expect("collector helper thread")
            })
            .collect();
        Self { shared, helpers }
    }

    /// Workers a phase runs on, the collecting thread included.
    pub(super) fn workers(&self) -> usize {
        self.helpers.len() + 1
    }

    /// Runs `phase` on every worker and returns each one's result, by worker index.
    pub(super) fn run<R: Send>(&self, phase: impl Fn(usize) -> R + Sync) -> Vec<R> {
        let results: Vec<Mutex<Option<R>>> =
            (0..self.workers()).map(|_| Mutex::new(None)).collect();
        let body = |index: usize| {
            let result = phase(index);
            *results[index].lock().unwrap() = Some(result);
        };
        let body: &Phase<'_> = &body;
        {
            let mut state = self.shared.state.lock().unwrap();
            // SAFETY: only the lifetime is erased; this call waits below until no helper uses
            // the phase any more.
            state.phase = Some(unsafe {
                std::mem::transmute::<*const Phase<'_>, *const Phase<'static>>(body)
            });
            state.generation += 1;
            state.running = self.helpers.len();
            self.shared.start.notify_all();
        }
        body(0);
        let mut state = self.shared.state.lock().unwrap();
        while state.running != 0 {
            state = self.shared.finished.wait(state).unwrap();
        }
        state.phase = None;
        let panicked = std::mem::take(&mut state.panicked);
        drop(state);
        assert!(!panicked, "collector helper panicked");
        results
            .into_iter()
            .map(|slot| {
                slot.into_inner()
                    .unwrap()
                    .expect("every worker ran the phase")
            })
            .collect()
    }
}

fn helper(shared: &Shared, index: usize) {
    let mut seen = 0;
    loop {
        let phase = {
            let mut state = shared.state.lock().unwrap();
            while state.generation == seen && !state.shutdown {
                state = shared.start.wait(state).unwrap();
            }
            if state.shutdown {
                return;
            }
            seen = state.generation;
            state.phase.expect("a started phase")
        };
        // SAFETY: `run` keeps the phase alive until `running` drops to zero below.
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*phase)(index) }));
        let mut state = shared.state.lock().unwrap();
        state.panicked |= outcome.is_err();
        state.running -= 1;
        if state.running == 0 {
            shared.finished.notify_one();
        }
    }
}

impl Drop for GcWorkers {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().shutdown = true;
        self.shared.start.notify_all();
        for helper in self.helpers.drain(..) {
            let _ = helper.join();
        }
    }
}
