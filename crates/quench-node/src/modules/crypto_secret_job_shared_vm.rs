//! Draft native SecretKeyGenJob. Uses the configured SDK's key policy and classes.
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
use std::sync::mpsc::{self, Receiver, TryRecvError};
type Context<'a> = NativeContext<'a, NodeHost>;
const ASYNC: u32 = 0;
const SYNC: u32 = 1;
const WEBCRYPTO: u32 = 2;
const BITS_PER_BYTE: u32 = u8::BITS;
const CACHE: &str = "\0crypto:secret-job";
#[derive(Default)]
pub(crate) struct State {
    running: Vec<Running>,
    resources: Vec<Resource>,
}
struct Resource {
    identity: HostWeakRoot,
    id: u64,
}
enum Failure {
    Crypto(openssl::error::ErrorStack),
    WorkerDisconnected,
}
type Outcome = Result<Material, Failure>;
enum Delivery {
    Pending(Receiver<Outcome>),
    Ready(Outcome),
}
struct Running {
    identity: HostWeakRoot,
    result: Delivery,
}
struct Material(Vec<u8>);
impl Drop for Material {
    fn drop(&mut self) {
        // Production already links OpenSSL through openssl-sys.
        unsafe extern "C" {
            fn OPENSSL_cleanse(p: *mut std::ffi::c_void, n: usize);
        }
        unsafe { OPENSSL_cleanse(self.0.as_mut_ptr().cast(), self.0.len()) }
    }
}
fn generate(bits: u32, mode: u32) -> Outcome {
    let bytes = if mode == WEBCRYPTO {
        u64::from(bits).div_ceil(u64::from(BITS_PER_BYTE))
    } else {
        u64::from(bits) / u64::from(BITS_PER_BYTE)
    };
    let mut material = Material(vec![0; bytes as usize]);
    openssl::rand::rand_priv_bytes(&mut material.0).map_err(Failure::Crypto)?;
    if mode == WEBCRYPTO && bits % BITS_PER_BYTE != 0 {
        if let Some(last) = material.0.last_mut() {
            *last &= u8::MAX << (BITS_PER_BYTE - bits % BITS_PER_BYTE);
        }
    }
    Ok(material)
}
fn get(c: &mut Context<'_>, o: RootId, name: &str) -> Result<RootId, RootedError> {
    let name = c.string_rooted(name);
    c.get_property_rooted(o, name)
}
fn put(c: &mut Context<'_>, o: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let name = c.string_rooted(name);
    c.define_data_property_rooted(o, name, value, true, true, true)?;
    Ok(())
}
fn fail(c: &mut Context<'_>, text: &str) -> RootedError {
    match c.type_error_rooted(text) {
        Ok(e) => c.throw(e),
        Err(e) => e,
    }
}
fn uint(c: &mut Context<'_>, arg: RootId) -> Result<u32, RootedError> {
    let n = c.rooted_value(arg).and_then(|v| v.as_number());
    match n {
        Some(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => Ok(n as u32),
        _ => Err(fail(c, "Expected uint32 crypto job argument")),
    }
}
fn api(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .builtin_cache
        .get(CACHE)
        .copied()
        .ok_or_else(|| RootedError::host("Secret job binding not installed"))
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    let api = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let wm = get(c, global, "WeakMap")?;
        let map = c.construct_rooted(wm, wm, &[])?;
        let proto = get(c, wm, "prototype")?;
        let setter = get(c, proto, "set")?;
        let api = c.null_object_rooted()?;
        put(c, api, "jobs", map)?;
        put(c, api, "set", setter)?;
        for name in ["Promise", "DOMException"] {
            let ctor = get(c, global, name)?;
            put(c, api, name, ctor)?;
        }
        let reflect = get(c, global, "Reflect")?;
        let delete = get(c, reflect, "deleteProperty")?;
        put(c, api, "delete", delete)?;
        Ok(api)
    })?;
    let root = c.retain(api)?;
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(CACHE.into(), root);
    let ctor = c.host_constructor(crate::host::shared_vm::operation("cryptoSecretJobNew"))?;
    let proto = get(c, ctor, "prototype")?;
    let run = c.host_function(crate::host::shared_vm::operation("cryptoSecretJobRun"))?;
    put(c, proto, "run", run)?;
    put(c, binding, "SecretKeyGenJob", ctor)?;
    for (name, value) in [
        ("kCryptoJobAsync", ASYNC),
        ("kCryptoJobSync", SYNC),
        ("kCryptoJobWebCrypto", WEBCRYPTO),
    ] {
        let value = c.number(value as f64);
        put(c, binding, name, value)?;
    }
    Ok(())
}
fn record(c: &mut Context<'_>, job: RootId) -> Result<RootId, RootedError> {
    let api = api(c)?;
    let map = get(c, api, "jobs")?;
    let record = c.weak_map_get_rooted(map, job)?;
    if !c.is_object_rooted(record)? {
        return Err(fail(c, "Invalid SecretKeyGenJob receiver"));
    }
    Ok(record)
}
pub(crate) fn create(
    c: &mut Context<'_>,
    job: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let arg = |i: usize| args.get(i).copied().unwrap_or(missing);
    let mode = uint(c, arg(0))?;
    if mode > WEBCRYPTO {
        return Err(fail(c, "Invalid crypto job mode"));
    }
    uint(c, arg(1))?;
    let api = api(c)?;
    let record = c.null_object_rooted()?;
    for (name, i) in [
        ("mode", 0),
        ("bits", 1),
        ("algorithm", 2),
        ("usages", 3),
        ("extractable", 4),
    ] {
        put(c, record, name, arg(i))?;
    }
    let no = c.boolean(false);
    put(c, record, "started", no)?;
    let map = get(c, api, "jobs")?;
    let setter = get(c, api, "set")?;
    c.call_rooted(setter, map, &[job, record])?;
    let shared = c.host_mut().shared_state();
    let id = super::async_hooks_shared_vm::create_context(&shared);
    let id_value = c.number(id as f64);
    put(c, record, "asyncId", id_value)?;
    let identity = c.host_weak_root(job)?;
    shared
        .borrow_mut()
        .crypto_secret_jobs
        .resources
        .push(Resource { identity, id });
    // Canonical resource creation snapshots ALS stores before guest init hooks.
    // No RefCell borrow is held across the reentrant init callback.
    super::async_hooks_shared_vm::emit_init(c, job, "KEYGENREQUEST", id)?;
    Ok(job)
}
pub(crate) fn executor(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let record = c.host_function_data()?;
    put(c, record, "resolve", args[0])?;
    put(c, record, "reject", args[1])?;
    Ok(c.undefined())
}
fn encode(
    c: &mut Context<'_>,
    record: RootId,
    bytes: &[u8],
    mode: u32,
) -> Result<RootId, RootedError> {
    let algorithm = get(c, record, "algorithm")?;
    let usages = get(c, record, "usages")?;
    let extractable = get(c, record, "extractable")?;
    super::crypto_key_shared_vm::generated_secret(
        c,
        bytes,
        if mode == WEBCRYPTO {
            Some((algorithm, usages, extractable))
        } else {
            None
        },
    )
}
fn native_error(c: &mut Context<'_>, failure: Failure) -> RootedError {
    match failure {
        Failure::Crypto(stack) => super::crypto_error_shared_vm::decorated(c, stack),
        Failure::WorkerDisconnected => match c.error_rooted("Secret key worker disconnected") {
            Ok(error) => c.throw(error),
            Err(error) => error,
        },
    }
}
fn exception(c: &mut Context<'_>, error: RootedError) -> Result<RootId, RootedError> {
    if let Some(root) = error.exception {
        Ok(root)
    } else {
        c.error_rooted(&error.to_string())
    }
}
fn queue(c: &mut Context<'_>, data: RootId, completion: bool) -> Result<(), RootedError> {
    let callback = c.host_function_with_data(
        crate::host::shared_vm::operation(if completion {
            "cryptoSecretJobComplete"
        } else {
            "cryptoSecretJobPoll"
        }),
        data,
    )?;
    let shared = c.host_mut().shared_state();
    let id = shared
        .borrow_mut()
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("Crypto completion IDs exhausted"))?;
    let resource = if completion {
        let job = get(c, data, "0")?;
        let record = get(c, data, "1")?;
        let id = get(c, record, "asyncId")?;
        let id = c.to_number(id)? as u64;
        Some((job, id))
    } else {
        None
    };
    let callback = c.retain(callback)?;
    let undefined = c.undefined();
    let receiver = match c.retain(undefined) {
        Ok(root) => root,
        Err(error) => {
            c.release_root(callback);
            return Err(error);
        }
    };
    let execution = if let Some((job, id)) = resource {
        let resource = match c.retain(job) {
            Ok(root) => root,
            Err(error) => {
                c.release_root(callback);
                c.release_root(receiver);
                return Err(error);
            }
        };
        Some(super::shared_event_loop::CallbackExecution { id, resource })
    } else {
        None
    };
    shared.borrow_mut().scheduler.queue_shared_immediate(
        id,
        super::shared_event_loop::SharedImmediateOwner::Host,
        super::shared_event_loop::SharedCallback {
            execution,
            callback,
            receiver,
            args: Vec::new(),
        },
    );
    if completion {
        // The canonical scheduler now owns destruction on every return path,
        // including before-hook throws and reentrant/throwing guest callbacks.
        let (_, id) = resource.expect("completion resource");
        let index = shared
            .borrow()
            .crypto_secret_jobs
            .resources
            .iter()
            .position(|r| r.id == id);
        if let Some(index) = index {
            let resource = shared
                .borrow_mut()
                .crypto_secret_jobs
                .resources
                .remove(index);
            c.release_host_weak_root(resource.identity);
        }
    }
    Ok(())
}
pub(crate) fn run(c: &mut Context<'_>, job: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let record = record(c, job)?;
    let started = get(c, record, "started")?;
    if c.truthy_rooted(started)? {
        return Err(fail(c, "Crypto job already started"));
    }
    let mode = get(c, record, "mode")?;
    let mode = uint(c, mode)?;
    let bits = get(c, record, "bits")?;
    let bits = uint(c, bits)?;
    let yes = c.boolean(true);
    put(c, record, "started", yes)?;
    if mode == SYNC {
        let result = generate(bits, mode).map_err(|stack| native_error(c, stack));
        let result = result.and_then(|material| encode(c, record, &material.0, mode));
        let missing = c.undefined();
        return match result {
            Ok(key) => c.array_rooted(&[missing, key]),
            Err(error) => {
                let error = exception(c, error)?;
                c.array_rooted(&[error, missing])
            }
        };
    }
    let promise = if mode == WEBCRYPTO {
        let api = api(c)?;
        let ctor = get(c, api, "Promise")?;
        let executor = c.host_function_with_data(
            crate::host::shared_vm::operation("cryptoSecretJobExecutor"),
            record,
        )?;
        c.construct_rooted(ctor, ctor, &[executor])?
    } else {
        c.undefined()
    };
    // A queued native callback owns job -> WeakMap record -> algorithm/resolvers.
    // The worker receives only native values; it never touches VM roots.
    let data = c.array_rooted(&[job, record])?;
    let identity = c.host_weak_root(job)?;
    let (sender, receiver) = mpsc::channel();
    let shared = c.host_mut().shared_state();
    let worker = shared.borrow_mut().native_tasks.submit(move || {
        let result = generate(bits, mode);
        let _ = sender.send(result);
    });
    if let Err(error) = worker {
        c.release_host_weak_root(identity);
        return Err(RootedError::host(error.to_string()));
    }
    let shared = c.host_mut().shared_state();
    shared
        .borrow_mut()
        .crypto_secret_jobs
        .running
        .push(Running {
            identity,
            result: Delivery::Pending(receiver),
        });
    if let Err(error) = queue(c, data, false) {
        let entry = shared
            .borrow_mut()
            .crypto_secret_jobs
            .running
            .pop()
            .expect("just inserted");
        c.release_host_weak_root(entry.identity);
        return Err(error);
    }
    Ok(promise)
}
pub(crate) fn poll(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let job = get(c, data, "0")?;
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .crypto_secret_jobs
        .running
        .iter()
        .map(|e| e.identity)
        .collect::<Vec<_>>();
    let mut index = None;
    for (i, key) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(key) {
            if c.strict_equal_rooted(value, job)? {
                index = Some(i);
                break;
            }
        }
    }
    let Some(index) = index else {
        return Err(RootedError::host("Missing running crypto job"));
    };
    let result = {
        let state = shared.borrow();
        match &state.crypto_secret_jobs.running[index].result {
            Delivery::Pending(receiver) => receiver.try_recv(),
            Delivery::Ready(_) => {
                return Err(RootedError::host("Crypto completion already queued"));
            }
        }
    };
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(TryRecvError::Empty) => {
            queue(c, data, false)?;
            return Ok(c.undefined());
        }
        Err(TryRecvError::Disconnected) => {
            // Preserve disconnected worker failure as a host error, not empty material.
            shared.borrow_mut().crypto_secret_jobs.running[index].result =
                Delivery::Ready(Err(Failure::WorkerDisconnected));
            queue(c, data, true)?;
            return Ok(c.undefined());
        }
    };
    shared.borrow_mut().crypto_secret_jobs.running[index].result = Delivery::Ready(outcome);
    queue(c, data, true)?;
    Ok(c.undefined())
}
pub(crate) fn complete(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let job = get(c, data, "0")?;
    let record = get(c, data, "1")?;
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .crypto_secret_jobs
        .running
        .iter()
        .map(|e| e.identity)
        .collect::<Vec<_>>();
    let mut index = None;
    for (i, key) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(key) {
            if c.strict_equal_rooted(value, job)? {
                index = Some(i);
                break;
            }
        }
    }
    let index = index.ok_or_else(|| RootedError::host("Missing completed crypto job"))?;
    let entry = shared.borrow_mut().crypto_secret_jobs.running.remove(index);
    c.release_host_weak_root(entry.identity);
    let Delivery::Ready(outcome) = entry.result else {
        return Err(RootedError::host("Crypto completion before worker result"));
    };
    let mode = get(c, record, "mode")?;
    let mode = uint(c, mode)?;
    let result = match outcome {
        Ok(material) => encode(c, record, &material.0, mode),
        Err(failure) => Err(native_error(c, failure)),
    };
    let undefined = c.undefined();
    if mode == ASYNC {
        let callback = get(c, job, "ondone")?;
        let (error, key) = match result {
            Ok(key) => (undefined, key),
            Err(e) => (exception(c, e)?, undefined),
        };
        c.call_rooted(callback, job, &[error, key])?;
    } else {
        match result {
            Ok(key) => {
                // Fresh SDK InternalCryptoKey has no own then. Suppress inherited
                // then hooks during intrinsic promise resolution, as Node does.
                let then = c.string_rooted("then");
                c.define_data_property_rooted(key, then, undefined, true, false, true)?;
                let resolve = get(c, record, "resolve")?;
                let settled = c.call_rooted(resolve, undefined, &[key]);
                let api = api(c)?;
                let delete = get(c, api, "delete")?;
                let cleanup = c.call_rooted(delete, undefined, &[key, then]);
                settled?;
                cleanup?;
            }
            Err(error) => {
                let cause = exception(c, error)?;
                let api = api(c)?;
                let ctor = get(c, api, "DOMException")?;
                let options = c.null_object_rooted()?;
                let name = c.string_rooted("OperationError");
                put(c, options, "name", name)?;
                put(c, options, "cause", cause)?;
                let message =
                    c.string_rooted("The operation failed for an operation-specific reason");
                let error = c.construct_rooted(ctor, ctor, &[message, options])?;
                let reject = get(c, record, "reject")?;
                c.call_rooted(reject, undefined, &[error])?;
            }
        }
        put(c, record, "resolve", undefined)?;
        put(c, record, "reject", undefined)?;
    }
    Ok(undefined)
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .crypto_secret_jobs
        .resources
        .iter()
        .enumerate()
        .filter_map(|(i, r)| (!runtime.host_weak_root_is_live(r.identity)).then_some(i))
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let resource = shared
            .borrow_mut()
            .crypto_secret_jobs
            .resources
            .remove(index);
        runtime.release_host_weak_root(resource.identity);
        super::async_hooks_shared_vm::complete_runtime_context(runtime, resource.id);
    }
    let dead = shared
        .borrow()
        .crypto_secret_jobs
        .running
        .iter()
        .enumerate()
        .filter_map(|(i, r)| (!runtime.host_weak_root_is_live(r.identity)).then_some(i))
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let entry = shared.borrow_mut().crypto_secret_jobs.running.remove(index);
        runtime.release_host_weak_root(entry.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().crypto_secret_jobs.running);
    for entry in entries {
        runtime.release_host_weak_root(entry.identity);
    }
    let resources = std::mem::take(&mut shared.borrow_mut().crypto_secret_jobs.resources);
    for resource in resources {
        runtime.release_host_weak_root(resource.identity);
        super::async_hooks_shared_vm::clear_context(runtime, &shared, resource.id);
    }
    // Dropping receivers causes late worker results to be dropped and cleansed.
}
