//! Asynchronous Node-API: async work on the shared pool, thread-safe functions, promises
//! and the event-loop source that delivers their completions on the VM thread.
//!
//! Other threads never touch the VM. They append `Event`s to their environment's queue;
//! the event loop's `poll` drains them through the `napiDrain` operation, one addon entry
//! per event, so each completion runs with its own handle scope and exception routing.

use super::abi::{node_api, Finalize};
use super::functions::{call, callable};
use super::lifecycle::{collected, finalize, Finalizer};
use super::{api, enter, js_api, out, Context, Env, NapiValue, Status, Thrown, ENVS, NULL_VALUE};
use crate::host::NodeHost;
use quench_runtime::{RootId, RootedError};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
#[cfg(not(target_os = "linux"))]
use std::io::{Read, Write};
use std::os::fd::RawFd;
#[cfg(not(target_os = "linux"))]
use std::os::fd::AsRawFd;
#[cfg(not(target_os = "linux"))]
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};

type Execute = unsafe extern "C" fn(*mut Env, *mut c_void);
type Complete = unsafe extern "C" fn(*mut Env, Status, *mut c_void);
type CallJs = unsafe extern "C" fn(*mut Env, NapiValue, *mut c_void, *mut c_void);

/// Work delivered to the VM thread.
pub(crate) enum Event {
    WorkComplete(*mut AsyncWork),
    Threadsafe(*mut ThreadsafeFunction),
    Finalize(Finalizer),
    Uncaught(RootId),
}
// SAFETY: events carry addresses owned by their environment; only the VM thread
// dereferences them, after the producing thread has published its writes via the lock.
unsafe impl Send for Event {}

type Events = Arc<EventQueue>;

pub(super) struct EventQueue {
    pub(super) queue: Mutex<VecDeque<Event>>,
    wake: Arc<WakeSignal>,
}

pub(crate) struct WakeSignal {
    #[cfg(target_os = "linux")]
    fd: RawFd,
    #[cfg(not(target_os = "linux"))]
    reader: Mutex<UnixStream>,
    #[cfg(not(target_os = "linux"))]
    writer: Mutex<UnixStream>,
}

impl WakeSignal {
    fn new() -> std::io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: eventfd owns no pointers; the descriptor stays live with this signal.
            let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Self { fd })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let (reader, writer) = UnixStream::pair()?;
            reader.set_nonblocking(true)?;
            writer.set_nonblocking(true)?;
            Ok(Self {
                reader: Mutex::new(reader),
                writer: Mutex::new(writer),
            })
        }
    }

    pub(crate) fn notify(&self) {
        #[cfg(target_os = "linux")]
        {
            let value: u64 = 1;
            // SAFETY: `fd` is a live eventfd and `value` is its required eight-byte payload.
            loop {
                let written = unsafe {
                    libc::write(
                        self.fd,
                        (&value as *const u64).cast(),
                        std::mem::size_of::<u64>(),
                    )
                };
                if written >= 0
                    || std::io::Error::last_os_error().kind()
                        != std::io::ErrorKind::Interrupted
                {
                    break;
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            let _ = writer.write(&[1]);
        }
    }

    fn raw_fd(&self) -> RawFd {
        #[cfg(target_os = "linux")]
        {
            self.fd
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.reader
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_raw_fd()
        }
    }

    fn drain(&self) {
        #[cfg(target_os = "linux")]
        {
            let mut value = 0u64;
            loop {
                // SAFETY: `fd` is a live eventfd and `value` is an eight-byte output buffer.
                let read = unsafe {
                    libc::read(
                        self.fd,
                        (&mut value as *mut u64).cast(),
                        std::mem::size_of::<u64>(),
                    )
                };
                if read == std::mem::size_of::<u64>() as isize {
                    continue;
                }
                if read < 0
                    && std::io::Error::last_os_error().kind()
                        == std::io::ErrorKind::Interrupted
                {
                    continue;
                }
                break;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut reader = self.reader.lock().unwrap_or_else(|e| e.into_inner());
            let mut bytes = [0; 256];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for WakeSignal {
    fn drop(&mut self) {
        // SAFETY: this is the unique owner of `fd`.
        unsafe { libc::close(self.fd) };
    }
}

thread_local! {
    static WAKE_SIGNAL: RefCell<Option<Arc<WakeSignal>>> = const { RefCell::new(None) };
}

pub(crate) fn wake_signal() -> std::io::Result<Arc<WakeSignal>> {
    WAKE_SIGNAL.with(|slot| {
        if let Some(signal) = slot.borrow().as_ref() {
            return Ok(Arc::clone(signal));
        }
        let signal = Arc::new(WakeSignal::new()?);
        *slot.borrow_mut() = Some(Arc::clone(&signal));
        Ok(signal)
    })
}

pub(super) fn new_events() -> Result<Events, String> {
    let wake = wake_signal().map_err(|error| error.to_string())?;
    Ok(Arc::new(EventQueue {
        queue: Mutex::new(VecDeque::new()),
        wake,
    }))
}

pub(super) fn push(events: &Events, event: Event) {
    events
        .queue
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back(event);
    events.wake.notify();
}

pub(crate) fn wake_fd() -> Option<RawFd> {
    WAKE_SIGNAL.with(|slot| slot.borrow().as_ref().map(|signal| signal.raw_fd()))
}

pub(crate) fn drain_wake() {
    WAKE_SIGNAL.with(|slot| {
        if let Some(signal) = slot.borrow().as_ref() {
            signal.drain();
        }
    });
}

/// The lifecycle of an async work item.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkState {
    Idle,
    Queued,
    Running,
    Done,
    Cancelled,
}

pub(crate) struct AsyncWork {
    env: *mut Env,
    execute: Execute,
    complete: Option<Complete>,
    data: *mut c_void,
    state: AtomicU8,
    events: Events,
}

impl AsyncWork {
    fn transition(&self, from: WorkState, to: WorkState) -> bool {
        self.state
            .compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

struct SendWork(*mut AsyncWork);
// SAFETY: the item outlives its execution; the pool thread only runs `execute`.
unsafe impl Send for SendWork {}

/// Queue state shared with producer threads.
struct ThreadsafeState {
    queue: VecDeque<usize>,
    threads: usize,
    closing: bool,
    aborted: bool,
    /// A wake event is queued and not yet drained.
    signalled: bool,
}

pub(crate) struct ThreadsafeFunction {
    env: *mut Env,
    function: Cell<Option<RootId>>,
    context: *mut c_void,
    call_js: Option<CallJs>,
    finalize: Finalizer,
    max_queue: usize,
    state: Mutex<ThreadsafeState>,
    space: Condvar,
    referenced: Cell<bool>,
    events: Events,
}

impl ThreadsafeFunction {
    fn lock(&self) -> std::sync::MutexGuard<'_, ThreadsafeState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn signal(&self, state: &mut ThreadsafeState) {
        if !state.signalled {
            state.signalled = true;
            push(
                &self.events,
                Event::Threadsafe((self as *const Self).cast_mut()),
            );
        }
    }
}

fn threadsafe<'a>(function: *mut ThreadsafeFunction) -> Result<&'a ThreadsafeFunction, Status> {
    // SAFETY: a thread-safe function lives until its finalization on the VM thread, which
    // Node-API callers must not race (they hold a thread count until releasing it).
    unsafe { function.as_ref() }.ok_or(Status::InvalidArg)
}

/// A promise's resolving functions, retained until one settles it.
pub(crate) struct Deferred {
    resolve: RootId,
    reject: RootId,
}

/// The executor of `napi_create_promise`: records the resolving functions in its data.
pub(crate) fn promise_executor(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let slots = c.host_function_data()?;
    for (index, &function) in args.iter().take(2).enumerate() {
        let key = c.number(index as f64);
        c.set_property_rooted(slots, key, function, slots)?;
    }
    Ok(c.undefined())
}

fn settle(env: *mut Env, deferred: *mut Deferred, value: NapiValue, resolve: bool) -> Status {
    js_api(env, |env, c| {
        if deferred.is_null() {
            return Err(Status::InvalidArg);
        }
        let value = env.value(value)?;
        // SAFETY: a deferred is settled once, which ends its lifetime.
        let deferred = unsafe { Box::from_raw(deferred) };
        let function = if resolve {
            deferred.resolve
        } else {
            deferred.reject
        };
        let undefined = c.undefined();
        let result = c.call_rooted(function, undefined, &[value]);
        c.release_root(deferred.resolve);
        c.release_root(deferred.reject);
        result.napi(env, c).map(drop)
    })
}

/// Raise `error` as an uncaught exception from the event loop, as Node's
/// `napi_fatal_exception` does.
pub(super) fn report_uncaught(env: &Env, c: &mut Context<'_>, error: RootId) {
    if let Ok(error) = c.retain(error) {
        push(&env.events, Event::Uncaught(error));
    }
}

node_api! {
    fn napi_create_async_work(env: *mut Env, _resource: NapiValue, _name: NapiValue, execute: Option<Execute>, complete: Option<Complete>, data: *mut c_void, result: *mut *mut AsyncWork) -> Status {
        api(env, |env, _| {
            let execute = execute.ok_or(Status::InvalidArg)?;
            let work = AsyncWork {
                env: (env as *const Env).cast_mut(),
                execute,
                complete,
                data,
                state: AtomicU8::new(WorkState::Idle as u8),
                events: env.events.clone(),
            };
            out(result, Box::into_raw(Box::new(work)))
        })
    }

    fn napi_delete_async_work(env: *mut Env, work: *mut AsyncWork) -> Status {
        api(env, |_, _| {
            if work.is_null() {
                return Err(Status::InvalidArg);
            }
            // SAFETY: deleting ends the item's lifetime; callers delete only idle items.
            drop(unsafe { Box::from_raw(work) });
            Ok(())
        })
    }

    fn napi_queue_async_work(env: *mut Env, work: *mut AsyncWork) -> Status {
        api(env, |env, _| {
            // SAFETY: a live item created by `napi_create_async_work`.
            let item = unsafe { work.as_ref() }.ok_or(Status::InvalidArg)?;
            if !item.transition(WorkState::Idle, WorkState::Queued)
                && !item.transition(WorkState::Done, WorkState::Queued)
                && !item.transition(WorkState::Cancelled, WorkState::Queued)
            {
                return Err(Status::GenericFailure);
            }
            env.pending_async_work.set(env.pending_async_work.get() + 1);
            let job = SendWork(work);
            crate::modules::native_work::submit(move || {
                let job = job;
                // SAFETY: the item stays alive until its completion is delivered.
                let item = unsafe { &*job.0 };
                if item.transition(WorkState::Queued, WorkState::Running) {
                    unsafe { (item.execute)(item.env, item.data) };
                    item.state.store(WorkState::Done as u8, Ordering::Release);
                    push(&item.events, Event::WorkComplete(job.0));
                }
            })
            .map_err(|_| Status::GenericFailure)
        })
    }

    fn napi_cancel_async_work(env: *mut Env, work: *mut AsyncWork) -> Status {
        api(env, |_, _| {
            // SAFETY: a live item created by `napi_create_async_work`.
            let item = unsafe { work.as_ref() }.ok_or(Status::InvalidArg)?;
            if !item.transition(WorkState::Queued, WorkState::Cancelled) {
                return Err(Status::GenericFailure);
            }
            push(&item.events, Event::WorkComplete(work));
            Ok(())
        })
    }

    fn napi_create_threadsafe_function(env: *mut Env, function: NapiValue, _resource: NapiValue, _name: NapiValue, max_queue: usize, threads: usize, finalize_data: *mut c_void, finalize_callback: Finalize, context: *mut c_void, call_js: Option<CallJs>, result: *mut *mut ThreadsafeFunction) -> Status {
        api(env, |env, c| {
            if threads == 0 {
                return Err(Status::InvalidArg);
            }
            let function = if function.is_null() {
                call_js.ok_or(Status::InvalidArg)?;
                None
            } else {
                let function = callable(c, env.value(function)?)?;
                Some(c.retain(function).napi(env, c)?)
            };
            let tsfn = Box::into_raw(Box::new(ThreadsafeFunction {
                env: (env as *const Env).cast_mut(),
                function: Cell::new(function),
                context,
                call_js,
                finalize: Finalizer { callback: finalize_callback, data: finalize_data, hint: context },
                max_queue,
                state: Mutex::new(ThreadsafeState { queue: VecDeque::new(), threads, closing: false, aborted: false, signalled: false }),
                space: Condvar::new(),
                referenced: Cell::new(true),
                events: env.events.clone(),
            }));
            env.threadsafe_functions.borrow_mut().push(tsfn);
            out(result, tsfn)
        })
    }

    fn napi_get_threadsafe_function_context(function: *mut ThreadsafeFunction, result: *mut *mut c_void) -> Status {
        match threadsafe(function) {
            Ok(function) => out(result, function.context).err().unwrap_or(Status::Ok),
            Err(status) => status,
        }
    }

    fn napi_call_threadsafe_function(function: *mut ThreadsafeFunction, data: *mut c_void, blocking: i32) -> Status {
        let Ok(function) = threadsafe(function) else { return Status::InvalidArg };
        let mut state = function.lock();
        loop {
            if state.closing || state.threads == 0 {
                return Status::Closing;
            }
            if function.max_queue == 0 || state.queue.len() < function.max_queue {
                break;
            }
            if blocking == 0 {
                return Status::QueueFull;
            }
            state = function.space.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        state.queue.push_back(data as usize);
        function.signal(&mut state);
        Status::Ok
    }

    fn napi_acquire_threadsafe_function(function: *mut ThreadsafeFunction) -> Status {
        let Ok(function) = threadsafe(function) else { return Status::InvalidArg };
        let mut state = function.lock();
        if state.closing {
            return Status::Closing;
        }
        state.threads += 1;
        Status::Ok
    }

    fn napi_release_threadsafe_function(function: *mut ThreadsafeFunction, mode: i32) -> Status {
        let Ok(function) = threadsafe(function) else { return Status::InvalidArg };
        let mut state = function.lock();
        if state.threads == 0 {
            return Status::InvalidArg;
        }
        state.threads -= 1;
        // napi_tsfn_abort (1) closes the function at once and refuses further calls.
        if mode == 1 {
            state.closing = true;
            state.aborted = true;
        }
        if state.threads == 0 || state.aborted {
            function.space.notify_all();
            function.signal(&mut state);
        }
        Status::Ok
    }

    fn napi_ref_threadsafe_function(env: *mut Env, function: *mut ThreadsafeFunction) -> Status {
        api(env, |_, _| threadsafe(function).map(|function| function.referenced.set(true)))
    }

    fn napi_unref_threadsafe_function(env: *mut Env, function: *mut ThreadsafeFunction) -> Status {
        api(env, |_, _| threadsafe(function).map(|function| function.referenced.set(false)))
    }

    fn napi_create_promise(env: *mut Env, deferred: *mut *mut Deferred, promise: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            if deferred.is_null() || promise.is_null() {
                return Err(Status::InvalidArg);
            }
            let constructor = super::objects::intrinsic(env, c, &["Promise"])?;
            let slots = c.array_rooted(&[]).napi(env, c)?;
            let operation = crate::host::shared_vm::operation("napiPromiseExecutor");
            let executor = c.host_function_with_data(operation, slots).napi(env, c)?;
            let created = c.construct_rooted(constructor, constructor, &[executor]).napi(env, c)?;
            let mut functions = [None, None];
            for (index, slot) in functions.iter_mut().enumerate() {
                let key = c.number(index as f64);
                let function = c.get_property_rooted(slots, key).napi(env, c)?;
                *slot = Some(c.retain(function).napi(env, c)?);
            }
            let [Some(resolve), Some(reject)] = functions else { return Err(Status::GenericFailure) };
            out(deferred, Box::into_raw(Box::new(Deferred { resolve, reject })))?;
            out(promise, env.handle(created))
        })
    }

    fn napi_resolve_deferred(env: *mut Env, deferred: *mut Deferred, value: NapiValue) -> Status {
        settle(env, deferred, value, true)
    }

    fn napi_reject_deferred(env: *mut Env, deferred: *mut Deferred, value: NapiValue) -> Status {
        settle(env, deferred, value, false)
    }

    fn napi_is_promise(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            out(result, c.is_promise_rooted(value).napi(env, c)?)
        })
    }

    fn napi_make_callback(env: *mut Env, _context: *mut c_void, receiver: NapiValue, function: NapiValue, argc: usize, argv: *const NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| call(env, c, receiver, function, argc, argv, result))
    }

    fn napi_async_init(env: *mut Env, _resource: NapiValue, _name: NapiValue, result: *mut *mut c_void) -> Status {
        api(env, |env, _| out(result, (env as *const Env).cast_mut().cast()))
    }

    fn napi_async_destroy(env: *mut Env, _context: *mut c_void) -> Status {
        api(env, |_, _| Ok(()))
    }

    fn napi_open_callback_scope(env: *mut Env, _resource: NapiValue, _context: *mut c_void, result: *mut *mut c_void) -> Status {
        api(env, |env, _| out(result, (env as *const Env).cast_mut().cast()))
    }

    fn napi_close_callback_scope(env: *mut Env, scope: *mut c_void) -> Status {
        api(env, |_, _| if scope.is_null() { Err(Status::InvalidArg) } else { Ok(()) })
    }
}

/// Deliver one thread-safe function's queued calls; finalize it once closed and empty.
fn run_threadsafe(
    c: &mut Context<'_>,
    function: *mut ThreadsafeFunction,
) -> Result<(), RootedError> {
    // SAFETY: wake events name live functions; finalization removes them from delivery.
    let tsfn = unsafe { &*function };
    let (items, finished, aborted) = {
        let mut state = tsfn.lock();
        state.signalled = false;
        let items: Vec<usize> = state.queue.drain(..).collect();
        tsfn.space.notify_all();
        (items, state.threads == 0 || state.aborted, state.aborted)
    };
    let mut items = items.into_iter();
    if !aborted {
        for data in items.by_ref() {
            let target = tsfn.function.get();
            enter(c, tsfn.env, |env| {
                // SAFETY: the environment is live; `current` is the context `enter` published.
                let env_ref = unsafe { &*env };
                let Some(c) = super::current() else {
                    return NULL_VALUE;
                };
                let handle = target
                    .and_then(|target| c.scope_root(target))
                    .map_or(NULL_VALUE, |root| env_ref.handle(root));
                match tsfn.call_js {
                    // SAFETY: the addon's marshaller receives its context and queued data.
                    Some(call_js) => unsafe {
                        call_js(env, handle, tsfn.context, data as *mut c_void)
                    },
                    None => {
                        let undefined = env_ref.handle(c.undefined());
                        // SAFETY: calling the stored function with handles of this scope.
                        unsafe {
                            super::functions::napi_call_function(
                                env,
                                undefined,
                                handle,
                                0,
                                std::ptr::null(),
                                std::ptr::null_mut(),
                            )
                        };
                    }
                }
                NULL_VALUE
            })?;
        }
    }
    if !finished {
        return Ok(());
    }
    // Node hands undelivered items to call_js without an environment so they can be freed.
    if let Some(call_js) = tsfn.call_js {
        for data in items {
            unsafe {
                call_js(
                    std::ptr::null_mut(),
                    NULL_VALUE,
                    tsfn.context,
                    data as *mut c_void,
                )
            };
        }
    }
    // SAFETY: environments are leaked at creation.
    let env = unsafe { &*tsfn.env };
    env.threadsafe_functions
        .borrow_mut()
        .retain(|&candidate| candidate != function);
    let result = finalize(c, tsfn.env, tsfn.finalize);
    if let Some(root) = tsfn.function.take() {
        c.release_root(root);
    }
    // SAFETY: closed and removed from delivery; no producer may use it after release.
    drop(unsafe { Box::from_raw(function) });
    result
}

fn next_event() -> Option<(*mut Env, Event)> {
    ENVS.with(|envs| {
        envs.borrow().iter().find_map(|&env| {
            // SAFETY: environments are leaked at creation.
            let env_ref = unsafe { &*env };
            let event = env_ref
                .events
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_front();
            event.map(|event| (env, event))
        })
    })
}

/// `napiDrain`: deliver one pending event or collected finalizer; `true` when one ran.
pub(crate) fn drain(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if let Some((env, event)) = next_event() {
        match event {
            Event::WorkComplete(work) => {
                // SAFETY: the item is live until its completion is delivered.
                let item = unsafe { &*work };
                // SAFETY: environments are leaked at creation.
                let env_ref = unsafe { &*env };
                env_ref
                    .pending_async_work
                    .set(env_ref.pending_async_work.get().saturating_sub(1));
                let status = if item.state.load(Ordering::Acquire) == WorkState::Cancelled as u8 {
                    Status::Cancelled
                } else {
                    Status::Ok
                };
                if let Some(complete) = item.complete {
                    let data = item.data;
                    enter(c, env, |env| {
                        // SAFETY: the addon's completion receives its status and data.
                        unsafe { complete(env, status, data) };
                        NULL_VALUE
                    })?;
                }
            }
            Event::Threadsafe(function) => run_threadsafe(c, function)?,
            Event::Finalize(finalizer) => finalize(c, env, finalizer)?,
            Event::Uncaught(error) => {
                let thrown = c.throw(error);
                c.release_root(error);
                return Err(thrown);
            }
        }
        return Ok(c.boolean(true));
    }
    let envs: Vec<*mut Env> = ENVS.with(|envs| envs.borrow().clone());
    for env in envs {
        // SAFETY: environments are leaked at creation.
        let ready = collected(unsafe { &*env }, c);
        if !ready.is_empty() {
            for finalizer in ready {
                finalize(c, env, finalizer)?;
            }
            return Ok(c.boolean(true));
        }
    }
    Ok(c.boolean(false))
}

/// Whether addon work keeps the event loop alive: queued async work, referenced open
/// thread-safe functions, or undelivered events.
pub(crate) fn has_work() -> bool {
    ENVS.with(|envs| {
        envs.borrow().iter().any(|&env| {
            // SAFETY: environments are leaked at creation.
            let env = unsafe { &*env };
            env.pending_async_work.get() > 0
                || !env
                    .events
                    .queue
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty()
                || env.threadsafe_functions.borrow().iter().any(|&function| {
                    // SAFETY: listed functions are live until finalized and removed.
                    unsafe { &*function }.referenced.get()
                })
        })
    })
}

/// Event-loop source: deliver pending addon events; `true` when any ran.
pub(crate) fn poll(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    drain_wake();
    if ENVS.with(|envs| envs.borrow().is_empty()) {
        return Ok(false);
    }
    crate::modules::process_shared_vm::deliver_each(runtime, program, "napiDrain")
}
