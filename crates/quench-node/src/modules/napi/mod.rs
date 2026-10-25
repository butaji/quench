//! Node-API: native addons loaded with `process.dlopen` call the `napi_*` C ABI that this
//! executable exports (Node 26 `js_native_api.h`/`node_api.h` are the ABI authority).
//!
//! Every value an addon holds is a `napi_value` handle: an index into its environment's
//! handle table naming a root of the shared VM. Handles live in the scope of the call that
//! made them, exactly like V8 local handles: the entry trampolines (module registration,
//! function callbacks, finalizers, async completions) open a scope and close it on return,
//! and `napi_open_handle_scope` nests further scopes inside one call. Calls into JavaScript
//! use the `NativeContext` of the innermost entry, published per thread for the C ABI.
//!
//! JavaScript exceptions raised while an addon runs become the environment's pending
//! exception; the trampoline rethrows it when the addon returns, as Node does.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::{
    cell::{Cell, RefCell},
    ffi::{c_char, c_void, CStr, CString},
    sync::Arc,
};

mod abi;
mod errors;
mod functions;
mod lifecycle;
mod memory;
mod objects;
mod values;
mod work;

pub(crate) use abi::*;

pub(crate) fn wake_fd() -> Option<std::os::fd::RawFd> {
    work::wake_fd()
}

/// The calling thread's wake signal for cross-thread completions outside N-API.
pub(crate) fn thread_wake() -> std::io::Result<std::sync::Arc<work::WakeSignal>> {
    work::thread_wake()
}

/// Notify a retained wake signal after a completion is visible to its thread.
pub(crate) fn notify_wake(signal: &work::WakeSignal) {
    work::notify_wake(signal);
}

pub(crate) fn drain_wake() {
    work::drain_wake();
}

type Context<'a> = NativeContext<'a, NodeHost>;

/// The environment an addon receives as `napi_env`. One is created per loaded addon module;
/// its address stays fixed for the process lifetime because native code retains it.
pub(crate) struct Env {
    /// Live `napi_value` handles; handle `n` names `handles[n - 1]`.
    handles: RefCell<Vec<RootId>>,
    /// A thrown value waiting for the trampoline to rethrow; retained, not scoped.
    pending_exception: Cell<Option<RootId>>,
    last_error: RefCell<ExtendedErrorInfo>,
    /// The Node-API version the module was built for; selects version-gated behavior.
    module_api_version: i32,
    /// The addon's location as a `file:` URL (`node_api_get_module_file_name`).
    file_url: CString,
    /// Private maps from objects to their wrap records and type tags (retained WeakMaps).
    wraps: RootId,
    type_tags: RootId,
    /// Objects constructed by a class constructor, mapped to its record (V8 signatures).
    template_instances: RootId,
    /// Startup `WeakMap.prototype.set`/`delete`, so guest edits never reach these maps.
    weak_map_set: RootId,
    weak_map_delete: RootId,
    instance_data: Cell<Option<lifecycle::Finalizer>>,
    cleanup_hooks: RefCell<Vec<(CleanupHook, usize)>>,
    /// Bytes addons report holding outside the heap (`napi_adjust_external_memory`).
    external_memory: Cell<i64>,
    /// Weakly held targets whose finalizers run once the collector clears them.
    finalizers: RefCell<Vec<lifecycle::WeakFinalizer>>,
    /// Work completed off the VM thread, delivered by the event loop.
    events: Arc<work::EventQueue>,
    /// Outstanding work that keeps the event loop alive.
    pending_async_work: Cell<usize>,
    threadsafe_functions: RefCell<Vec<*mut work::ThreadsafeFunction>>,
}

thread_local! {
    /// The innermost active entry's context; null outside an addon call.
    static CURRENT: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    /// Environments created on this VM thread, polled by the event loop.
    static ENVS: RefCell<Vec<*mut Env>> = const { RefCell::new(Vec::new()) };
    /// A module registered by `napi_module_register` while `dlopen` runs its constructors.
    static REGISTERED: Cell<Option<abi::RegisterFunction>> = const { Cell::new(None) };
}

/// The context of the innermost entry; addons can only run JavaScript inside one.
fn current<'a>() -> Option<&'a mut Context<'static>> {
    let context = CURRENT.with(Cell::get).cast::<Context<'static>>();
    // SAFETY: `enter` publishes a context only while it is borrowed for that entry and
    // restores the previous pointer before the borrow ends.
    unsafe { context.as_mut() }
}

impl Env {
    fn new(
        c: &mut Context<'_>,
        filename: &str,
        module_api_version: i32,
    ) -> Result<*mut Env, RootedError> {
        let global = c.global_root()?;
        let weak_map = c
            .startup_data_property_rooted(global, "WeakMap")
            .ok_or_else(|| RootedError::host("WeakMap intrinsic is unavailable"))?;
        let prototype = c
            .startup_data_property_rooted(weak_map, "prototype")
            .ok_or_else(|| RootedError::host("WeakMap.prototype intrinsic is unavailable"))?;
        let method = |c: &mut Context<'_>, name| {
            c.startup_data_property_rooted(prototype, name)
                .ok_or_else(|| RootedError::host("WeakMap method intrinsic is unavailable"))
        };
        let (set, delete) = (method(c, "set")?, method(c, "delete")?);
        let wraps = c.construct_rooted(weak_map, weak_map, &[])?;
        let type_tags = c.construct_rooted(weak_map, weak_map, &[])?;
        let template_instances = c.construct_rooted(weak_map, weak_map, &[])?;
        let env = Box::new(Env {
            handles: RefCell::new(Vec::new()),
            pending_exception: Cell::new(None),
            last_error: RefCell::new(ExtendedErrorInfo::default()),
            module_api_version,
            file_url: url::Url::from_file_path(filename)
                .map(String::from)
                .ok()
                .and_then(|url| CString::new(url).ok())
                .unwrap_or_default(),
            wraps: c.retain(wraps)?,
            type_tags: c.retain(type_tags)?,
            template_instances: c.retain(template_instances)?,
            weak_map_set: c.retain(set)?,
            weak_map_delete: c.retain(delete)?,
            instance_data: Cell::new(None),
            cleanup_hooks: RefCell::new(Vec::new()),
            external_memory: Cell::new(0),
            finalizers: RefCell::new(Vec::new()),
            events: work::new_events().map_err(RootedError::host)?,
            pending_async_work: Cell::new(0),
            threadsafe_functions: RefCell::new(Vec::new()),
        });
        let env = Box::into_raw(env);
        ENVS.with(|envs| envs.borrow_mut().push(env));
        Ok(env)
    }

    /// Publish `value` as a handle in the current scope.
    fn handle(&self, value: RootId) -> NapiValue {
        let mut handles = self.handles.borrow_mut();
        handles.push(value);
        handles.len() as NapiValue
    }

    fn value(&self, handle: NapiValue) -> Result<RootId, Status> {
        let index = (handle as usize).checked_sub(1).ok_or(Status::InvalidArg)?;
        self.handles
            .borrow()
            .get(index)
            .copied()
            .ok_or(Status::InvalidArg)
    }

    /// Make `value` the pending exception, replacing an earlier one.
    fn throw_value(&self, c: &mut Context<'_>, value: RootId) -> Status {
        let retained = c.retain(value).ok();
        if let Some(previous) = self.pending_exception.replace(retained) {
            c.release_root(previous);
        }
        Status::PendingException
    }

    /// Record a JavaScript failure as the pending exception.
    fn fail(&self, c: &mut Context<'_>, error: RootedError) -> Status {
        let exception = match error.exception {
            Some(exception) => Ok(exception),
            None => c.error_rooted(&error.error.to_string()),
        };
        match exception {
            Ok(exception) => self.throw_value(c, exception),
            Err(_) => Status::GenericFailure,
        }
    }

    fn take_exception(&self) -> Option<RootId> {
        self.pending_exception.take()
    }

    /// Whether the module was built for at least Node-API `version`.
    fn module_version_at_least(&self, version: i32) -> bool {
        self.module_api_version >= version
    }
}

/// Turn a JavaScript failure into the pending exception (`napi_pending_exception`).
trait Thrown<T> {
    fn napi(self, env: &Env, c: &mut Context<'_>) -> Result<T, Status>;
}

impl<T> Thrown<T> for Result<T, RootedError> {
    fn napi(self, env: &Env, c: &mut Context<'_>) -> Result<T, Status> {
        self.map_err(|error| env.fail(c, error))
    }
}

/// Run an addon entry with `c` published as the current context in a fresh handle scope.
/// `run` returns the addon's result handle; a pending exception becomes the entry's throw.
fn enter(
    c: &mut Context<'_>,
    env: *mut Env,
    run: impl FnOnce(*mut Env) -> NapiValue,
) -> Result<RootId, RootedError> {
    // SAFETY: environments are leaked at creation and never freed.
    let env_ref = unsafe { &*env };
    let mark = env_ref.handles.borrow().len();
    let previous = CURRENT.with(|current| current.replace((c as *mut Context<'_>).cast()));
    let result = run(env);
    CURRENT.with(|current| current.set(previous));
    let value = (result != NULL_VALUE)
        .then(|| env_ref.value(result).ok())
        .flatten();
    env_ref.handles.borrow_mut().truncate(mark);
    if let Some(exception) = env_ref.take_exception() {
        let error = c.throw(exception);
        c.release_root(exception);
        return Err(error);
    }
    Ok(value.unwrap_or_else(|| c.undefined()))
}

/// The status-returning shape of every exported function: validate the environment, clear
/// the last error, run `body` in the current context and record its status.
fn api(
    env: *mut Env,
    body: impl FnOnce(&Env, &mut Context<'static>) -> Result<(), Status>,
) -> Status {
    // SAFETY: a non-null `napi_env` is an environment created by `Env::new`.
    let Some(env) = (unsafe { env.as_ref() }) else {
        return Status::InvalidArg;
    };
    let status = match current() {
        Some(c) => body(env, c).err().unwrap_or(Status::Ok),
        None => Status::CannotRunJs,
    };
    env.last_error.borrow_mut().error_code = status;
    status
}

/// `api` for operations that may run JavaScript: refused while an exception is pending.
fn js_api(
    env: *mut Env,
    body: impl FnOnce(&Env, &mut Context<'static>) -> Result<(), Status>,
) -> Status {
    api(env, |env, c| {
        if env.pending_exception.get().is_some() {
            return Err(Status::PendingException);
        }
        body(env, c)
    })
}

/// Store `value` through an addon's out-parameter.
fn out<T>(target: *mut T, value: T) -> Result<(), Status> {
    // SAFETY: Node-API out-parameters are writable pointers owned by the caller.
    unsafe { target.as_mut() }
        .map(|target| *target = value)
        .ok_or(Status::InvalidArg)
}

/// The bytes of an addon string argument: `length` bytes, or up to NUL for
/// `NAPI_AUTO_LENGTH`; null is valid only for an empty string.
fn c_bytes<'a>(text: *const c_char, length: usize) -> Result<&'a [u8], Status> {
    if text.is_null() {
        return if length == 0 {
            Ok(&[])
        } else {
            Err(Status::InvalidArg)
        };
    }
    if length == AUTO_LENGTH {
        // SAFETY: NAPI_AUTO_LENGTH promises a NUL-terminated string.
        return Ok(unsafe { CStr::from_ptr(text) }.to_bytes());
    }
    if length > i32::MAX as usize {
        return Err(Status::InvalidArg);
    }
    // SAFETY: the caller supplies `length` readable bytes.
    Ok(unsafe { std::slice::from_raw_parts(text.cast::<u8>(), length) })
}

/// An addon UTF-8 string; invalid sequences become U+FFFD as in V8.
fn c_text<'a>(text: *const c_char, length: usize) -> Result<std::borrow::Cow<'a, str>, Status> {
    c_bytes(text, length).map(String::from_utf8_lossy)
}

/// The `module_api_version` an addon declares through `node_api_module_get_api_version_v1`.
fn declared_api_version(library: *mut c_void) -> i32 {
    let symbol = c"node_api_module_get_api_version_v1";
    // SAFETY: dlsym on a live handle with a NUL-terminated name.
    let address = unsafe { libc::dlsym(library, symbol.as_ptr()) };
    let default = env!("QUENCH_NODE_API_DEFAULT_MODULE_API_VERSION")
        .parse()
        .unwrap_or(0);
    if address.is_null() {
        return default;
    }
    // SAFETY: the symbol is the documented `int32_t (*)(void)` version getter.
    let version: abi::ApiVersionFunction = unsafe { std::mem::transmute(address) };
    unsafe { version() }
}

/// Whether this binary links the C runtime statically (the qualification builds).
/// An addon links the shared C runtime, and `dlopen` would load a second
/// copy beside the static one, with its own thread-local storage and heap: addon code running
/// on a thread the static runtime created reads another runtime's thread state and faults
/// (the vitest scenario's rolldown binding, 2026-10-24).
const STATIC_C_RUNTIME: bool = cfg!(target_feature = "crt-static");

/// `process.dlopen(module, filename[, flags])`: load an addon and run its registration with
/// `module.exports`, replacing the exports when registration returns another value.
pub(crate) fn dlopen(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    for symbol in EXPORTS.iter().flat_map(|symbols| symbols.iter()) {
        std::hint::black_box(symbol.0);
    }
    let module = args.first().copied().unwrap_or_else(|| c.undefined());
    let filename = match args.get(1) {
        Some(&name) => c.to_string(name)?,
        None => String::new(),
    };
    let flags = match args.get(2) {
        Some(&flags)
            if c.value_type_rooted(flags) != Some(quench_runtime::ValueType::Undefined) =>
        {
            c.to_number(flags)? as i32
        }
        _ => libc::RTLD_LAZY,
    };
    let exports_key = c.string_rooted("exports");
    let exports = c.get_property_rooted(module, exports_key)?;
    let path =
        CString::new(filename.as_str()).map_err(|_| RootedError::host("invalid addon path"))?;
    if STATIC_C_RUNTIME {
        let message = format!(
            "{filename}: native addons need a dynamically linked quench-node; this build links \
             the C runtime statically, and the addon's libc would run beside it"
        );
        return Err(errors::coded_error(c, "ERR_DLOPEN_FAILED", &message));
    }
    REGISTERED.with(|registered| registered.set(None));
    // SAFETY: loading a native library runs its constructors, which is the purpose of dlopen.
    let library = unsafe { libc::dlopen(path.as_ptr(), flags) };
    if library.is_null() {
        // SAFETY: dlerror returns the thread's last loader message or null.
        let message = unsafe { libc::dlerror() };
        let message = if message.is_null() {
            String::from("dlopen failed")
        } else {
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned()
        };
        return Err(errors::coded_error(c, "ERR_DLOPEN_FAILED", &message));
    }
    let register = REGISTERED.with(Cell::take).or_else(|| {
        // SAFETY: dlsym on the live handle; the symbol is the documented registration entry.
        let address = unsafe { libc::dlsym(library, c"napi_register_module_v1".as_ptr()) };
        (!address.is_null())
            .then(|| unsafe { std::mem::transmute::<*mut c_void, abi::RegisterFunction>(address) })
    });
    let Some(register) = register else {
        let message = format!("Module did not self-register: '{filename}'.");
        return Err(errors::coded_error(c, "ERR_DLOPEN_FAILED", &message));
    };
    let env = Env::new(c, &filename, declared_api_version(library))?;
    let returned = enter(c, env, |env| {
        // SAFETY: entering registration with a live environment and exports handle.
        let exports = unsafe { &*env }.handle(exports);
        unsafe { register(env, exports) }
    })?;
    let undefined = c.value_type_rooted(returned) == Some(quench_runtime::ValueType::Undefined);
    if !undefined && !c.strict_equal_rooted(returned, exports)? {
        c.set_property_rooted(module, exports_key, returned, module)?;
    }
    Ok(c.undefined())
}

/// The exported ABI, referenced so the linker keeps every symbol for `dlopen`ed addons.
static EXPORTS: [&[ExportedSymbol]; 8] = [
    abi::EXPORTS,
    errors::EXPORTS,
    functions::EXPORTS,
    lifecycle::EXPORTS,
    memory::EXPORTS,
    objects::EXPORTS,
    values::EXPORTS,
    work::EXPORTS,
];

pub(crate) use functions::invoke;
pub(crate) use work::{drain, has_work, poll, promise_executor};
