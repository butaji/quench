//! C layouts and constants of the Node-API ABI (`js_native_api_types.h`, `node_api_types.h`),
//! plus the environment-level queries that need no JavaScript.

use super::{api, out, Env};
use std::ffi::{c_char, c_void, CStr};

/// `napi_value`: a 1-based handle index; null means "no value".
pub(crate) type NapiValue = *mut c_void;
pub(crate) const NULL_VALUE: NapiValue = std::ptr::null_mut();
/// `NAPI_AUTO_LENGTH`: the string is NUL-terminated.
pub(crate) const AUTO_LENGTH: usize = usize::MAX;

/// `napi_status`, in header order; the discriminants are the ABI values. Every value is
/// named, including those only other Node-API implementations produce.
#[repr(C)]
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Ok,
    InvalidArg,
    ObjectExpected,
    StringExpected,
    NameExpected,
    FunctionExpected,
    NumberExpected,
    BooleanExpected,
    ArrayExpected,
    GenericFailure,
    PendingException,
    Cancelled,
    EscapeCalledTwice,
    HandleScopeMismatch,
    CallbackScopeMismatch,
    QueueFull,
    Closing,
    BigintExpected,
    DateExpected,
    ArraybufferExpected,
    DetachableArraybufferExpected,
    WouldDeadlock,
    NoExternalBuffersAllowed,
    CannotRunJs,
}

impl Status {
    /// Node's `error_messages` table (js_native_api_v8.cc), indexed by status.
    fn message(self) -> &'static CStr {
        match self {
            Status::Ok => c"",
            Status::InvalidArg => c"Invalid argument",
            Status::ObjectExpected => c"An object was expected",
            Status::StringExpected => c"A string was expected",
            Status::NameExpected => c"A string or symbol was expected",
            Status::FunctionExpected => c"A function was expected",
            Status::NumberExpected => c"A number was expected",
            Status::BooleanExpected => c"A boolean was expected",
            Status::ArrayExpected => c"An array was expected",
            Status::GenericFailure => c"Unknown failure",
            Status::PendingException => c"An exception is pending",
            Status::Cancelled => c"The async work item was cancelled",
            Status::EscapeCalledTwice => c"napi_escape_handle already called on scope",
            Status::HandleScopeMismatch => c"Invalid handle scope usage",
            Status::CallbackScopeMismatch => c"Invalid callback scope usage",
            Status::QueueFull => c"Thread-safe function queue is full",
            Status::Closing => c"Thread-safe function handle is closing",
            Status::BigintExpected => c"A bigint was expected",
            Status::DateExpected => c"A date was expected",
            Status::ArraybufferExpected => c"An arraybuffer was expected",
            Status::DetachableArraybufferExpected => c"A detachable arraybuffer was expected",
            Status::WouldDeadlock => c"Main thread would deadlock",
            Status::NoExternalBuffersAllowed => c"External buffers are not allowed",
            Status::CannotRunJs => c"Cannot run JavaScript",
        }
    }
}

/// `napi_extended_error_info`.
#[repr(C)]
pub(crate) struct ExtendedErrorInfo {
    pub error_message: *const c_char,
    pub engine_reserved: *mut c_void,
    pub engine_error_code: u32,
    pub error_code: Status,
}

impl Default for ExtendedErrorInfo {
    fn default() -> Self {
        Self {
            error_message: std::ptr::null(),
            engine_reserved: std::ptr::null_mut(),
            engine_error_code: 0,
            error_code: Status::Ok,
        }
    }
}

pub(crate) type Callback = Option<unsafe extern "C" fn(*mut Env, *mut c_void) -> NapiValue>;
pub(crate) type Finalize = Option<unsafe extern "C" fn(*mut Env, *mut c_void, *mut c_void)>;
pub(crate) type CleanupHook = unsafe extern "C" fn(*mut c_void);
pub(crate) type RegisterFunction = unsafe extern "C" fn(*mut Env, NapiValue) -> NapiValue;
pub(crate) type ApiVersionFunction = unsafe extern "C" fn() -> i32;

/// `napi_property_attributes` bits.
pub(crate) const WRITABLE: i32 = 1 << 0;
pub(crate) const ENUMERABLE: i32 = 1 << 1;
pub(crate) const CONFIGURABLE: i32 = 1 << 2;
pub(crate) const STATIC: i32 = 1 << 10;

/// `napi_property_descriptor`.
#[repr(C)]
pub(crate) struct PropertyDescriptor {
    pub utf8name: *const c_char,
    pub name: NapiValue,
    pub method: Callback,
    pub getter: Callback,
    pub setter: Callback,
    pub value: NapiValue,
    pub attributes: i32,
    pub data: *mut c_void,
}

/// `napi_module`, the legacy registration record.
#[repr(C)]
pub(crate) struct Module {
    pub nm_version: i32,
    pub nm_flags: u32,
    pub nm_filename: *const c_char,
    pub nm_register_func: Option<RegisterFunction>,
    pub nm_modname: *const c_char,
    pub nm_priv: *mut c_void,
    pub reserved: [*mut c_void; 4],
}

/// `napi_node_version`.
#[repr(C)]
pub(crate) struct NodeVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub release: *const c_char,
}

/// An exported function's address, kept so the linker retains it.
pub(crate) struct ExportedSymbol(pub *const ());
// SAFETY: code addresses are immutable and never dereferenced as data.
unsafe impl Sync for ExportedSymbol {}

/// Define `#[no_mangle]` Node-API functions and the module's `EXPORTS` list from one table.
macro_rules! node_api {
    ($( fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty $body:block )*) => {
        $(
            #[no_mangle]
            #[allow(clippy::missing_safety_doc)]
            pub unsafe extern "C" fn $name($($arg: $ty),*) -> $ret $body
        )*
        pub(super) static EXPORTS: &[$crate::modules::napi::abi::ExportedSymbol] =
            &[$($crate::modules::napi::abi::ExportedSymbol($name as *const ())),*];
    };
}
pub(crate) use node_api;

/// The process-wide version record `napi_get_node_version` hands out.
struct StaticNodeVersion(NodeVersion);
// SAFETY: immutable after construction; `release` names a static string.
unsafe impl Sync for StaticNodeVersion {}
// SAFETY: as above; the record holds no thread-affine state.
unsafe impl Send for StaticNodeVersion {}

fn node_version() -> &'static NodeVersion {
    static VERSION: std::sync::OnceLock<StaticNodeVersion> = std::sync::OnceLock::new();
    &VERSION
        .get_or_init(|| {
            let mut parts = env!("QUENCH_NODE_COMPAT_VERSION")
                .split(['.', '-'])
                .map(|part| part.parse().unwrap_or(0));
            StaticNodeVersion(NodeVersion {
                major: parts.next().unwrap_or(0),
                minor: parts.next().unwrap_or(0),
                patch: parts.next().unwrap_or(0),
                release: c"node".as_ptr(),
            })
        })
        .0
}

/// Stand-in for `uv_loop_t`: addons may only compare or pass the pointer; Quench's
/// event loop exposes no libuv structure.
static EVENT_LOOP: u8 = 0;

node_api! {
    fn napi_get_last_error_info(env: *mut Env, result: *mut *const ExtendedErrorInfo) -> Status {
        // SAFETY: a non-null environment pointer names a live environment.
        let Some(env) = (unsafe { env.as_ref() }) else { return Status::InvalidArg };
        let mut info = env.last_error.borrow_mut();
        info.error_message = if info.error_code == Status::Ok {
            std::ptr::null()
        } else {
            info.error_code.message().as_ptr()
        };
        let pointer: *const ExtendedErrorInfo = &*info;
        drop(info);
        out(result, pointer).err().unwrap_or(Status::Ok)
    }

    fn napi_get_version(env: *mut Env, result: *mut u32) -> Status {
        api(env, |_, _| out(result, env!("QUENCH_NODE_API_SUPPORTED_VERSION_MAX").parse().unwrap_or(0)))
    }

    fn napi_get_node_version(env: *mut Env, result: *mut *const NodeVersion) -> Status {
        api(env, |_, _| out(result, node_version() as *const NodeVersion))
    }

    fn napi_get_uv_event_loop(env: *mut Env, result: *mut *const c_void) -> Status {
        api(env, |_, _| out(result, (&EVENT_LOOP as *const u8).cast()))
    }

    fn node_api_get_module_file_name(env: *mut Env, result: *mut *const c_char) -> Status {
        api(env, |env, _| out(result, env.file_url.as_ptr()))
    }

    fn napi_module_register(module: *mut Module) -> () {
        // SAFETY: the addon passes its static registration record.
        if let Some(register) = unsafe { module.as_ref() }.and_then(|module| module.nm_register_func) {
            super::REGISTERED.with(|registered| registered.set(Some(register)));
        }
    }

    fn napi_fatal_error(location: *const c_char, location_len: usize, message: *const c_char, message_len: usize) -> () {
        let text = |text, length| super::c_text(text, length).map(|text| text.into_owned()).unwrap_or_default();
        let (location, message) = (text(location, location_len), text(message, message_len));
        let location = if location.is_empty() { String::new() } else { format!("{location} ") };
        eprintln!("FATAL ERROR: {location}{message}");
        std::process::abort();
    }
}
