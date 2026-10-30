//! Real host work shared by VM isolates; callbacks and guest roots stay on the VM thread.
use std::{
    ffi::CString,
    sync::{mpsc, Arc, Mutex, OnceLock},
};
type Job = Box<dyn FnOnce() + Send + 'static>;
struct Pool {
    sender: mpsc::Sender<Job>,
}
static POOL: OnceLock<Result<Pool, String>> = OnceLock::new();
const DEFAULT_WORKERS: usize = 4;
const MAX_WORKERS: usize = 1024; // libuv's documented UV_THREADPOOL_SIZE limit.
impl Pool {
    fn new() -> Result<Self, String> {
        // libuv reads this process setting when its global work pool first starts.
        let count = std::env::var_os("UV_THREADPOOL_SIZE")
            .map(|value| {
                let value =
                    CString::new(value.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
                // SAFETY: a live NUL-terminated environment value; atoi is libuv's parser.
                let count = unsafe { libc::atoi(value.as_ptr()) } as u32 as usize;
                Ok::<_, String>(count.clamp(1, MAX_WORKERS))
            })
            .transpose()?
            .unwrap_or(DEFAULT_WORKERS);
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        for i in 0..count {
            let receiver = Arc::clone(&receiver);
            std::thread::Builder::new()
                .name(format!("quench-work-{i}"))
                .spawn(move || loop {
                    let next = receiver.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok(job) = next else { break };
                    // A failed work item drops its real completion sender. Its VM owner sees
                    // disconnection; keep the remaining pool available for independent work.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(Self { sender })
    }
}
pub(crate) fn submit(job: impl FnOnce() + Send + 'static) -> Result<(), String> {
    let wake = crate::modules::napi::thread_wake().ok();
    let pool = POOL.get_or_init(Pool::new).as_ref().map_err(Clone::clone)?;
    pool.sender
        .send(Box::new(move || {
            job();
            // The completion sender published before this point; wake its event loop
            // instead of leaving it to the poll interval.
            if let Some(wake) = &wake { crate::modules::napi::notify_wake(wake); }
        }))
        .map_err(|e| e.to_string())
}
