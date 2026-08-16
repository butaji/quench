use hmac::{Hmac, Mac};
use oxc_resolver::{ResolveOptions, Resolver};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sha3::{
    digest::{ExtendableOutput, Update as XofUpdate, XofReader},
    Shake128, Shake256,
};
use std::path::{Path, PathBuf};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use quench_runtime::{
    ops::{HostCapabilityKind, HostCapabilityRef, RealmId},
    value::{ArrayBufferData, Uint8ArrayData, Value},
    vm::{Host, VmContext, VmError},
};

thread_local! {
    static NODE_PROCESS_ENV: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PROCESS_TITLE: RefCell<String> = RefCell::new("quench-node".into());
    static NODE_PATH_MODULE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_UTIL_TYPES: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PROCESS_MODULE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PROCESS_WARNING_LISTENERS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static NODE_EXPERIMENTAL_WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static NODE_DNS_SERVERS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static NODE_STREAM_PROMISES: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PENDING_DGRAM_CALLBACKS: RefCell<Vec<(Value, Value)>> = const { RefCell::new(Vec::new()) };
    static NODE_TIMERS_PROMISES: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_TIMER_COUNTS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
    static NODE_ASSERT_MODULE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_OS_HOME_ERROR: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_OS_BINDING: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_PRIORITY: Cell<i32> = const { Cell::new(0) };
    static VM_SCRIPT_RUNS: Cell<u32> = const { Cell::new(0) };
    static VM_COMPILE_CONTEXT_EXTENSION: Cell<bool> = const { Cell::new(false) };
    static VM_COMPILE_PARSING_CONTEXT: RefCell<Option<Value>> = const { RefCell::new(None) };
    static VM_COMPILE_RETURN_VALUE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static VM_SCRIPT_CACHE_SOURCE: RefCell<Option<String>> = const { RefCell::new(None) };
    static BUFFER_INSPECT_MAX_BYTES: Cell<f64> = const { Cell::new(f64::INFINITY) };
    static NODE_DH_PRIVATE_SET: Cell<bool> = const { Cell::new(false) };
    static NODE_KEY_SOURCE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_PRIVATE_KEY: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_PUBLIC_KEY: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_GROUP_CONSTRUCTOR: RefCell<Option<Value>> = const { RefCell::new(None) };
    static NODE_DH_GENERATED_KEY: RefCell<Option<Value>> = const { RefCell::new(None) };
}

// Capability numbers are an internal NodeHost registry. Keep them named so
// the runtime boundary carries intent instead of opaque numeric protocol IDs.
struct CapabilityName;
include!("js_runtime_capabilities.rs");

include!("js_runtime_host_types.rs");

include!("js_runtime_construct_stream.rs");
include!("js_runtime_construct_c.rs");
include!("js_runtime_construct_b.rs");
include!("js_runtime_construct_a.rs");
include!("js_runtime_dispatch_misc_e.rs");
include!("js_runtime_dispatch_misc_d.rs");
include!("js_runtime_dispatch_misc_c.rs");
include!("js_runtime_dispatch_misc_b.rs");
include!("js_runtime_dispatch_misc_a.rs");
include!("js_runtime_dispatch_url.rs");
include!("js_runtime_dispatch_crypto_c.rs");
include!("js_runtime_dispatch_crypto_b.rs");
include!("js_runtime_dispatch_crypto_a.rs");
include!("js_runtime_dispatch_buffer.rs");
include!("js_runtime_dispatch_core.rs");

fn is_util_resolver(id: u16) -> bool {
    id >= CapabilityName::UtilResolverFirst
        && !matches!(
            id,
            CapabilityName::CryptoHashOn
                | CapabilityName::CryptoHashWrite
                | CapabilityName::CryptoHashEnd
                | CapabilityName::CryptoHashUpdate
                | CapabilityName::CryptoHashDigest
        )
}

include!("js_runtime_host_impl.rs");
include!("js_runtime_host_zlib.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_dgram.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_promises.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_fs_open.rs");
impl QuenchNodeHost {
    fn fs_dir_read(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
        asynchronous: bool,
    ) -> Result<Value, VmError> {
        let id = Self::fs_dir_id(receiver)?;
        let entry = self
            .directories
            .borrow_mut()
            .get_mut(&id)
            .and_then(|(values, index)| {
                let value = values.get(*index).cloned().unwrap_or(Value::Null);
                *index = index.saturating_add(1);
                Some(value)
            })
            .ok_or(VmError::NotCallable)?;
        if asynchronous {
            if let Some(callback) = arguments.last() {
                quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, entry])?;
            }
            Ok(Value::Undefined)
        } else {
            Ok(entry)
        }
    }

    fn fs_dir_close(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
        asynchronous: bool,
    ) -> Result<Value, VmError> {
        let id = Self::fs_dir_id(receiver)?;
        self.directories.borrow_mut().remove(&id);
        if asynchronous {
            if let Some(callback) = arguments.last() {
                quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
            }
        }
        Ok(Value::Undefined)
    }

    fn fs_close(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::Number(fd)) = arguments.first() else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "fd must be a number",
            )));
        };
        let fd = *fd as i32;
        if !self.fd_paths.borrow().contains_key(&fd) {
            return Err(VmError::Thrown(fs_error("EBADF", "bad file descriptor")));
        }
        self.fd_paths.borrow_mut().remove(&fd);
        self.fd_modes.borrow_mut().remove(&fd);
        Ok(Value::Undefined)
    }
}
include!("js_runtime_host_fs_io.rs");
impl QuenchNodeHost {}
include!("js_runtime_host_url_stream.rs");
impl QuenchNodeHost {
    fn http_call(&self, kind: HostCapabilityKind, arguments: &[Value]) -> Result<Value, VmError> {
        match kind {
            HostCapabilityKind::Custom(CapabilityName::HttpServer) => {
                self.http.borrow_mut().server_callback = arguments.first().cloned();
                Ok(Value::object(vec![
                    (
                        "listen".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpRequestOn,
                        )),
                    ),
                    (
                        "address".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpRequestEnd,
                        )),
                    ),
                    (
                        "close".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpRequestWrite,
                        )),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::HttpGet) => {
                let url = arguments
                    .first()
                    .and_then(|v| match v {
                        Value::String(s) => Some(s),
                        _ => None,
                    })
                    .ok_or(VmError::NotCallable)?;
                let callback = arguments.get(1).cloned().ok_or(VmError::NotCallable)?;
                let path = url
                    .split('/')
                    .skip(3)
                    .next()
                    .map(|p| format!("/{p}"))
                    .unwrap_or_else(|| "/".into());
                let response = response_object(500);
                let request = Value::object(vec![("url".into(), Value::String(path))]);
                let server = self
                    .http
                    .borrow()
                    .server_callback
                    .clone()
                    .ok_or(VmError::NotCallable)?;
                quench_runtime::execute::call(
                    &server,
                    &Value::Undefined,
                    &[request, response.clone()],
                )?;
                quench_runtime::execute::call(&callback, &Value::Undefined, &[response])?;
                let state = self.http.borrow();
                if let Some(data) = state.data_callback.clone() {
                    quench_runtime::execute::call(
                        &data,
                        &Value::Undefined,
                        &[Value::String(state.body.clone())],
                    )?;
                }
                if let Some(end) = state.end_callback.clone() {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpRequestOn) => {
                if let Some(callback) = arguments.last() {
                    quench_runtime::execute::call(callback, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpRequestEnd) => {
                Ok(Value::object(vec![("port".into(), Value::Number(43123.0))]))
            }
            HostCapabilityKind::Custom(CapabilityName::HttpRequestWrite) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(id) if (500..600).contains(&id) => {
                match id % 10 {
                    4 => {
                        self.http.borrow_mut().body =
                            arguments.first().map(value_to_string).unwrap_or_default()
                    }
                    5 => {
                        if matches!(arguments.first(), Some(Value::String(event)) if event == "data")
                        {
                            self.http.borrow_mut().data_callback = arguments.get(1).cloned();
                        } else if matches!(arguments.first(), Some(Value::String(event)) if event == "end")
                        {
                            self.http.borrow_mut().end_callback = arguments.get(1).cloned();
                        }
                    }
                    _ => {}
                }
                Ok(Value::Undefined)
            }
            _ => Err(VmError::NotCallable),
        }
    }
}

include!("js_runtime_helpers_host.rs");

include!("js_runtime_require_early.rs");

include!("js_runtime_require_module.rs");
include!("js_runtime_require_crypto.rs");
include!("js_runtime_require_fs.rs");
include!("js_runtime_require_stream_http.rs");
include!("js_runtime_require_url.rs");
include!("js_runtime_require_path.rs");
include!("js_runtime_path.rs");
include!("js_runtime_process_modules.rs");
include!("js_runtime_url_object.rs");
include!("js_runtime_url_pattern.rs");
include!("js_runtime_object_path.rs");
include!("js_runtime_url_legacy.rs");
include!("js_runtime_buffer_core.rs");
include!("js_runtime_buffer_methods.rs");
include!("js_runtime_string_decoder.rs");
include!("js_runtime_buffer_numeric.rs");
include!("js_runtime_internal_binding.rs");
include!("js_runtime_vm_helpers.rs");

include!("js_runtime_helpers_tail.rs");
include!("js_runtime_adapters.rs");
