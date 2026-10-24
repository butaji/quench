//! Per-environment native workers: bounded workers, unbounded owned FIFO.
//! No guest roots or runtime callbacks may cross this boundary.
use std::{
    collections::VecDeque,
    ffi::{c_char, c_int, CString},
    io,
    sync::{Arc, Condvar, Mutex},
    thread,
};
const LIBUV_DEFAULT_WORKERS: usize = 4;
const LIBUV_MAX_WORKERS: usize = 1024;
const LIBUV_ENV_BUFFER_BYTES: usize = 16;
const LIBUV_STACK_BYTES: usize = 8 * 1024 * 1024;
type Task = Box<dyn FnOnce() + Send + 'static>;
#[derive(Default)]
struct Queue {
    stopped: bool,
    tasks: VecDeque<Task>,
}
struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}
pub(crate) struct Pool {
    shared: Arc<Shared>,
    workers: Vec<thread::JoinHandle<()>>,
    configured: Option<usize>,
    started: bool,
}
fn worker_count(value: Option<&[u8]>) -> usize {
    let Some(value) = value.filter(|v| v.len() < LIBUV_ENV_BUFFER_BYTES) else {
        return LIBUV_DEFAULT_WORKERS;
    };
    let Ok(value) = CString::new(value) else {
        return LIBUV_DEFAULT_WORKERS;
    };
    unsafe extern "C" {
        fn atoi(value: *const c_char) -> c_int;
    }
    // Same host libc atoi conversion and unsigned assignment as libuv.
    let count = unsafe { atoi(value.as_ptr()) } as u32 as usize;
    count.clamp(1, LIBUV_MAX_WORKERS)
}
fn configured_workers() -> usize {
    // libuv reads process environment at the first work submission.
    let value = std::env::var_os("UV_THREADPOOL_SIZE");
    #[cfg(unix)]
    let count = {
        use std::os::unix::ffi::OsStrExt;
        worker_count(value.as_deref().map(|v| v.as_bytes()))
    };
    #[cfg(not(unix))]
    let count = worker_count(value.as_ref().and_then(|v| v.to_str()).map(str::as_bytes));
    count
}
impl Default for Pool {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                ready: Condvar::new(),
            }),
            workers: Vec::new(),
            configured: None,
            started: false,
        }
    }
}
impl Pool {
    #[cfg(test)]
    fn with_workers(configured: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                ready: Condvar::new(),
            }),
            workers: Vec::new(),
            configured: Some(configured),
            started: false,
        }
    }
    fn start(&mut self) -> io::Result<()> {
        if self.started {
            return Ok(());
        }
        if self.shared.queue.lock().unwrap().stopped {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "native pool stopped",
            ));
        }
        let configured = *self.configured.get_or_insert_with(configured_workers);
        for index in 0..configured {
            let shared = self.shared.clone();
            match thread::Builder::new()
                .name(format!("quench-native-{index}"))
                .stack_size(LIBUV_STACK_BYTES)
                .spawn(move || {
                    loop {
                        let task = {
                            let mut queue = shared.queue.lock().unwrap();
                            while !queue.stopped && queue.tasks.is_empty() {
                                queue = shared.ready.wait(queue).unwrap();
                            }
                            if queue.stopped {
                                break;
                            }
                            queue.tasks.pop_front().expect("ready task")
                        };
                        // One native task panic must not silently destroy pool capacity.
                        // Channel disconnect reports failure to its host-side request.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                    }
                }) {
                Ok(worker) => self.workers.push(worker),
                Err(error) => {
                    self.shutdown();
                    return Err(error);
                }
            }
        }
        self.started = true;
        Ok(())
    }
    pub(crate) fn submit(&mut self, task: impl FnOnce() + Send + 'static) -> io::Result<()> {
        self.start()?;
        let mut queue = self.shared.queue.lock().unwrap();
        if queue.stopped {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "native pool stopped",
            ));
        }
        queue.tasks.push_back(Box::new(task));
        self.shared.ready.notify_one();
        Ok(())
    }
    pub(crate) fn shutdown(&mut self) {
        let pending = {
            let mut queue = self.shared.queue.lock().unwrap();
            queue.stopped = true;
            std::mem::take(&mut queue.tasks)
        };
        // Drop closures outside the lock: their native destructors may re-enter.
        drop(pending);
        self.shared.ready.notify_all();
        // Native OS calls cannot always be cancelled. Do not block process.exit.
        // Detached active workers own Shared and retire after their current task.
        for worker in self.workers.drain(..) {
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.shutdown()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };
    #[test]
    fn config_matches_libuv() {
        for (value, want) in [
            (None, 4),
            (Some("0"), 1),
            (Some("-1"), 1024),
            (Some("3junk"), 3),
            (Some("junk"), 1),
            (Some("1025"), 1024),
            (Some("1234567890123456"), 4),
        ] {
            assert_eq!(worker_count(value.map(str::as_bytes)), want)
        }
    }
    #[test]
    fn bounded_workers_unbounded_queue_and_no_losses() {
        let mut pool = Pool::with_workers(2);
        let count = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        for i in 0..5000 {
            let (count, peak, gate, tx) = (count.clone(), peak.clone(), gate.clone(), tx.clone());
            pool.submit(move || {
                let active = count.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(active, Ordering::SeqCst);
                let (ready, wake) = (&gate.0, &gate.1);
                let mut ready = ready.lock().unwrap();
                while !*ready {
                    ready = wake.wait(ready).unwrap()
                }
                drop(ready);
                count.fetch_sub(1, Ordering::SeqCst);
                tx.send(i).unwrap();
            })
            .unwrap();
        }
        assert_eq!(pool.workers.len(), 2);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        let mut received = (0..5000)
            .map(|_| rx.recv_timeout(Duration::from_secs(10)).unwrap())
            .collect::<Vec<_>>();
        received.sort_unstable();
        assert_eq!(received, (0..5000).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) <= 2);
    }
    #[test]
    fn task_panic_does_not_lose_worker() {
        let mut pool = Pool::with_workers(1);
        pool.submit(|| panic!("intentional native task panic"))
            .unwrap();
        let (tx, rx) = mpsc::channel();
        pool.submit(move || tx.send(7).unwrap()).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), 7)
    }
    #[test]
    fn shutdown_drops_queued_and_rejects_further_work() {
        struct Dropped(Arc<AtomicUsize>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let mut pool = Pool::with_workers(1);
        let (gate_tx, gate_rx) = mpsc::channel();
        let (start_tx, start_rx) = mpsc::channel();
        pool.submit(move || {
            start_tx.send(()).unwrap();
            let _ = gate_rx.recv();
        })
        .unwrap();
        start_rx.recv().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        for _ in 0..100 {
            let guard = Dropped(drops.clone());
            pool.submit(move || drop(guard)).unwrap()
        }
        pool.shutdown();
        assert_eq!(drops.load(Ordering::SeqCst), 100);
        assert!(pool.submit(|| {}).is_err());
        gate_tx.send(()).unwrap();
    }
}
