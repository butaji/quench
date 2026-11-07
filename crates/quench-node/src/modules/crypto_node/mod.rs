//! Node's `crypto` module and the WebCrypto globals, over OpenSSL.
//!
//! Every stateful object (a hash, cipher, signer, key, Diffie-Hellman party or certificate)
//! is a guest object carrying one host-private slot: the index of its native state in the
//! host's handle table. The table owns the OpenSSL contexts; a weak root per entry lets the
//! idle sweep release the state of objects the collector reclaimed.

pub(crate) mod cipher;
pub(crate) mod clone;
pub(crate) mod dh;
pub(crate) mod errors;
pub(crate) mod hash;
pub(crate) mod kdf;
pub(crate) mod keys;
pub(crate) mod random;
pub(crate) mod sign;
pub(crate) mod web;
pub(crate) mod x509;

use crate::host::NodeHost;
use errors::CryptoError;
use quench_runtime::{BuiltinBrand, DescriptorFields, HostWeakRoot, NativeContext, RootId, RootedError, Runtime};

pub(crate) use crate::modules::stream_shared_vm::{arg, attempt, call_method, get, inherit, is_null, is_nullish, is_undefined, put};

pub(crate) type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) type Outcome = Result<RootId, RootedError>;

/// The module's constructors, kept so instances made natively share them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Hash,
    Hmac,
    Cipheriv,
    Decipheriv,
    Sign,
    Verify,
    KeyObject,
    SecretKeyObject,
    PublicKeyObject,
    PrivateKeyObject,
    Ecdh,
    DiffieHellman,
    DiffieHellmanGroup,
    X509Certificate,
    Certificate,
    CryptoKey,
    Crypto,
    SubtleCrypto,
}

impl Class {
    const COUNT: usize = 18;
}

/// The module's fixed roots, retained for the VM's lifetime.
#[derive(Clone, Copy)]
pub(crate) struct Intrinsics {
    /// The private symbol each stateful object keys its handle with.
    handle: RootId,
    /// `kKeyType`: the symbol a KeyObject shows its type under.
    pub(crate) key_type: RootId,
    classes: [RootId; Class::COUNT],
    module: RootId,
    /// The `Crypto` instance `globalThis.crypto` and `crypto.webcrypto` are.
    webcrypto: RootId,
}

/// The native state behind one guest object.
pub(crate) enum Native {
    Hash(hash::Digesting),
    Cipher(Box<cipher::CipherState>),
    Signer(sign::Signer),
    Key(std::sync::Arc<keys::Key>),
    Ecdh(dh::Ecdh),
    DiffieHellman(dh::DiffieHellman),
    Certificate(x509::Certificate),
    CryptoKey(Box<web::CryptoKeyState>),
}

struct Handle {
    owner: HostWeakRoot,
    native: Native,
}

/// The handle table and the module's intrinsics.
#[derive(Default)]
pub(crate) struct State {
    handles: Vec<Option<Handle>>,
    vacant: Vec<usize>,
    /// Live handles after the last sweep: the table sweeps when it doubles past this.
    live_after_sweep: usize,
    intrinsics: Option<Intrinsics>,
    pub(crate) digests: hash::DigestCache,
    /// The experimental WebCrypto algorithms already warned about.
    pub(crate) experimental_warned: Vec<web::Algorithm>,
}

/// The table sweeps no earlier than this many live handles.
const SWEEP_FLOOR: usize = 64;

impl State {
    fn live(&self) -> usize {
        self.handles.len() - self.vacant.len()
    }

    fn release(&mut self, index: usize) -> Option<HostWeakRoot> {
        let handle = self.handles.get_mut(index)?.take()?;
        self.vacant.push(index);
        Some(handle.owner)
    }

    fn dead(&self, is_live: impl Fn(HostWeakRoot) -> bool) -> Vec<usize> {
        let entries = self.handles.iter().enumerate();
        entries.filter_map(|(index, handle)| handle.as_ref().filter(|handle| !is_live(handle.owner)).map(|_| index)).collect()
    }

    pub(crate) fn write_snapshot(&self, _: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        let quiescent = self.live() == 0 && self.intrinsics.is_none();
        crate::host::startup_snapshot::require_quiescent(quiescent, "crypto state")
    }

    pub(crate) fn read_snapshot(&mut self, _: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        Ok(())
    }
}

fn state(c: &mut Context<'_>) -> std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>> {
    c.host_mut().shared_state()
}

pub(crate) fn intrinsics(c: &mut Context<'_>) -> Result<Intrinsics, RootedError> {
    if state(c).borrow().crypto.intrinsics.is_none() {
        module(c)?;
    }
    state(c).borrow().crypto.intrinsics.ok_or_else(|| RootedError::host("crypto module is not initialized"))
}

pub(crate) fn class(c: &mut Context<'_>, class: Class) -> Outcome {
    Ok(intrinsics(c)?.classes[class as usize])
}

/// Gives `object` its native state.
pub(crate) fn attach(c: &mut Context<'_>, object: RootId, native: Native) -> Result<(), RootedError> {
    let symbol = intrinsics(c)?.handle;
    let shared = state(c);
    let sweep = {
        let table = &shared.borrow().crypto;
        table.live() >= SWEEP_FLOOR.max(table.live_after_sweep * 2)
    };
    if sweep {
        let dead = shared.borrow().crypto.dead(|owner| c.host_weak_root_is_live(owner));
        for index in dead {
            let owner = shared.borrow_mut().crypto.release(index);
            if let Some(owner) = owner {
                c.release_host_weak_root(owner);
            }
        }
        let live = shared.borrow().crypto.live();
        shared.borrow_mut().crypto.live_after_sweep = live;
    }
    let owner = c.host_weak_root(object)?;
    let index = {
        let table = &mut shared.borrow_mut().crypto;
        let handle = Some(Handle { owner, native });
        match table.vacant.pop() {
            Some(index) => {
                table.handles[index] = handle;
                index
            }
            None => {
                table.handles.push(handle);
                table.handles.len() - 1
            }
        }
    };
    let index = c.number(index as f64);
    c.define_data_property_rooted(object, symbol, index, false, false, false)?;
    Ok(())
}

/// The handle index `object` carries, if it is one of the module's stateful objects.
fn handle_index(c: &mut Context<'_>, object: RootId) -> Result<Option<usize>, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(None);
    }
    let symbol = intrinsics(c)?.handle;
    let Some(fields) = c.own_property_fields_rooted(object, symbol)? else {
        return Ok(None);
    };
    let index = fields.value.and_then(|value| c.rooted_value(value)).and_then(|value| value.as_number());
    Ok(index.map(|index| index as usize))
}

/// Runs `f` over `object`'s native state; `None` when it has none.
pub(crate) fn with_native<R>(
    c: &mut Context<'_>,
    object: RootId,
    f: impl FnOnce(&mut Native) -> R,
) -> Result<Option<R>, RootedError> {
    let Some(index) = handle_index(c, object)? else {
        return Ok(None);
    };
    let shared = state(c);
    let mut table = shared.borrow_mut();
    Ok(table.crypto.handles.get_mut(index).and_then(Option::as_mut).map(|handle| f(&mut handle.native)))
}

/// Releases the state of objects the collector reclaimed (the idle hook).
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared.borrow().crypto.dead(|owner| runtime.host_weak_root_is_live(owner));
    for index in dead {
        let owner = shared.borrow_mut().crypto.release(index);
        if let Some(owner) = owner {
            runtime.release_host_weak_root(owner);
        }
    }
}

/// Releases every handle at shutdown.
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let handles = std::mem::take(&mut shared.borrow_mut().crypto.handles);
    shared.borrow_mut().crypto.vacant.clear();
    for handle in handles.into_iter().flatten() {
        runtime.release_host_weak_root(handle.owner);
    }
}

// ---- Argument conversion -------------------------------------------------------------

/// Node's formatted lists of the types an argument accepts.
pub(crate) mod accepts {
    pub(crate) const DATA: &str = "of type string or an instance of Buffer, TypedArray, or DataView";
    pub(crate) const BINARY_LIKE: &str =
        "of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView";
    pub(crate) const BUFFER_SOURCE: &str = "an instance of ArrayBuffer, Buffer, TypedArray, or DataView";
    pub(crate) const KEY_LIKE: &str =
        "of type string or an instance of ArrayBuffer, Buffer, TypedArray, DataView, KeyObject, or CryptoKey";
    pub(crate) const VIEW: &str = "an instance of Buffer, TypedArray, or DataView";
}

pub(crate) fn is_string(c: &Context<'_>, value: RootId) -> bool {
    crate::modules::stream_shared_vm::is_string(c, value)
}

pub(crate) fn is_view(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)
}

pub(crate) fn is_any_buffer(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    c.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)
}

/// The bytes of an ArrayBuffer, SharedArrayBuffer or view.
pub(crate) fn buffer_bytes(c: &Context<'_>, value: RootId) -> Option<Vec<u8>> {
    c.with_buffer_bytes_rooted(value, <[u8]>::to_vec)
}

/// A string's bytes as `Buffer.from(string, encoding)` makes them; `'buffer'` means UTF-8.
pub(crate) fn string_bytes(c: &mut Context<'_>, value: RootId, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    let encoding = match c.string_text(encoding)? {
        Some(name) if name == "buffer" => None,
        _ => Some(encoding),
    };
    crate::modules::buffer_shared_vm::encode_bytes(c, value, encoding)
}

/// Node's `getArrayBufferOrView`: a string (under `encoding`), an ArrayBuffer or a view.
pub(crate) fn binary_like(c: &mut Context<'_>, value: RootId, name: &str, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    if is_string(c, value) {
        return string_bytes(c, value, encoding);
    }
    match buffer_bytes(c, value) {
        Some(bytes) => Ok(bytes),
        None => Err(errors::invalid_type(c, value, name, accepts::BINARY_LIKE)),
    }
}

/// Node's `validateEncoding`: hex input must have an even length.
pub(crate) fn validate_encoding(c: &mut Context<'_>, data: RootId, encoding: RootId) -> Result<(), RootedError> {
    let Some(name) = c.string_text(encoding)? else {
        return Ok(());
    };
    let length = c.string_code_units(data)?.map_or(0, |units| units.len());
    if canonical_encoding(&name) == Some("hex") && length % 2 != 0 {
        let message = format!("is invalid for data of length {length}");
        return Err(errors::invalid_value(c, encoding, "encoding", &message));
    }
    Ok(())
}

pub(crate) fn canonical_encoding(name: &str) -> Option<&'static str> {
    crate::modules::buffer_codec::canonical_encoding(name)
}

/// The `update(data, encoding)` input shared by Hash, Hmac, Sign and Verify: a string under
/// `encoding`, or a view's bytes.
pub(crate) fn update_data(c: &mut Context<'_>, data: RootId, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    if is_string(c, data) {
        validate_encoding(c, data, encoding)?;
        let units = c.string_code_units(data)?.unwrap_or_default();
        let canonical = match c.string_text(encoding)? {
            Some(name) => canonical_encoding(&name).unwrap_or("utf8"),
            None => "utf8",
        };
        return Ok(crate::modules::buffer_codec::encode_units(&units, canonical));
    }
    if !is_view(c, data)? {
        return Err(errors::invalid_type(c, data, "data", accepts::DATA));
    }
    Ok(buffer_bytes(c, data).unwrap_or_default())
}

/// Bytes as the native layer returns them: a Buffer, or a string under a known encoding.
pub(crate) fn encode_output(c: &mut Context<'_>, bytes: Vec<u8>, encoding: RootId) -> Outcome {
    let canonical = match c.string_text(encoding)? {
        Some(name) => canonical_encoding(&name),
        None => None,
    };
    match canonical {
        Some(canonical) => {
            let units = crate::modules::buffer_codec::decode_units(&bytes, canonical);
            Ok(c.string_units_rooted(&units))
        }
        None => buffer(c, bytes),
    }
}

/// The object tables OpenSSL registers algorithm names in.
#[derive(Clone, Copy)]
pub(crate) enum Registry {
    Digests,
    Ciphers,
}

type NameCallback<T> = Option<unsafe extern "C" fn(*const T, *const std::ffi::c_char, *const std::ffi::c_char, *mut std::ffi::c_void)>;

unsafe extern "C" {
    fn EVP_MD_do_all_sorted(callback: NameCallback<openssl_sys::EVP_MD>, argument: *mut std::ffi::c_void);
    fn EVP_CIPHER_do_all_sorted(callback: NameCallback<openssl_sys::EVP_CIPHER>, argument: *mut std::ffi::c_void);
}

unsafe extern "C" fn collect_name<T>(_: *const T, from: *const std::ffi::c_char, _: *const std::ffi::c_char, names: *mut std::ffi::c_void) {
    if from.is_null() {
        return;
    }
    // SAFETY: `names` is the vector `registered_names` passed; `from` is a NUL-terminated name.
    let names = unsafe { &mut *names.cast::<Vec<String>>() };
    names.push(unsafe { std::ffi::CStr::from_ptr(from) }.to_string_lossy().into_owned());
}

/// The names a registry holds that `available` accepts, sorted and unique
/// (`getHashes()`, `getCiphers()`).
pub(crate) fn registered_names(registry: Registry, available: impl Fn(&str) -> bool) -> Vec<String> {
    openssl::init();
    let mut names = Vec::<String>::new();
    let argument = (&mut names as *mut Vec<String>).cast();
    // SAFETY: the callback only appends to `names`, which outlives the walk.
    unsafe {
        match registry {
            Registry::Digests => EVP_MD_do_all_sorted(Some(collect_name), argument),
            Registry::Ciphers => EVP_CIPHER_do_all_sorted(Some(collect_name), argument),
        }
    }
    names.retain(|name| available(name));
    names.sort_unstable();
    names.dedup();
    names
}

pub(crate) fn string_array(c: &mut Context<'_>, values: &[String]) -> Outcome {
    let values: Vec<RootId> = values.iter().map(|value| c.string_rooted(value)).collect();
    c.array_rooted(&values)
}

pub(crate) fn buffer(c: &mut Context<'_>, bytes: Vec<u8>) -> Outcome {
    crate::modules::buffer_shared_vm::from_vec(c, bytes)
}

pub(crate) fn array_buffer(c: &mut Context<'_>, bytes: Vec<u8>) -> Outcome {
    let view = c.uint8_array_from_rooted(bytes)?;
    get(c, view, "buffer")
}

/// `validateString`.
pub(crate) fn string_argument(c: &mut Context<'_>, value: RootId, name: &str) -> Result<String, RootedError> {
    match c.string_text(value)? {
        Some(text) => Ok(text),
        None => Err(errors::invalid_type(c, value, name, "of type string")),
    }
}

/// `validateFunction`.
pub(crate) fn callback_argument(c: &mut Context<'_>, value: RootId, name: &str) -> Result<RootId, RootedError> {
    if c.is_callable_rooted(value)? {
        Ok(value)
    } else {
        Err(errors::invalid_type(c, value, name, "of type function"))
    }
}

/// `validateObject` (null and arrays rejected, functions accepted only where Node does).
pub(crate) fn object_argument(c: &mut Context<'_>, value: RootId, name: &str) -> Result<RootId, RootedError> {
    if c.is_object_rooted(value)? && !c.is_array_rooted(value)? {
        Ok(value)
    } else {
        Err(errors::invalid_type(c, value, name, "of type object"))
    }
}

/// `validateInt32`/`validateUint32`/`validateInteger` with Node's bounds.
pub(crate) fn integer_argument(c: &mut Context<'_>, value: RootId, name: &str, minimum: f64, maximum: f64) -> Result<f64, RootedError> {
    let maximum = c.number(maximum);
    crate::modules::util_shared_vm::validate_integer(c, value, name, minimum, maximum)
}

/// A member of an options object; `undefined` when the options are nullish.
pub(crate) fn option(c: &mut Context<'_>, options: RootId, name: &str) -> Outcome {
    if is_nullish(c, options) || !c.is_object_rooted(options)? {
        return Ok(c.undefined());
    }
    get(c, options, name)
}

/// How an asynchronous operation reports its completion.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Completion {
    /// A thread-pool job's: from the event loop, `callback(null, ...values)` on success.
    Job,
    /// A job reporting success with `callback(undefined, ...values)` (`checkPrime`,
    /// `generatePrime`).
    JobUndefinedError,
    /// `randomInt`'s, from its entropy cache: the next tick, `callback(undefined, value)`.
    NextTick,
}

/// An asynchronous operation's completion: `callback(error)`, or success with `values`.
pub(crate) fn complete_as(c: &mut Context<'_>, callback: RootId, result: Result<Vec<RootId>, RootedError>, completion: Completion) -> Outcome {
    let mut args = Vec::new();
    match result {
        Ok(values) => {
            args.push(if completion == Completion::Job { c.null() } else { c.undefined() });
            args.extend(values);
        }
        Err(error) => args.push(errors::caught(error)?),
    }
    let undefined = c.undefined();
    if completion == Completion::NextTick {
        crate::modules::shared_event_loop::queue_next_tick(c, callback, undefined, &args)?;
    } else {
        args.insert(0, callback);
        crate::modules::timers_shared_vm::set_immediate(c, undefined, &args)?;
    }
    Ok(undefined)
}

pub(crate) fn complete(c: &mut Context<'_>, callback: RootId, result: Result<Vec<RootId>, RootedError>) -> Outcome {
    complete_as(c, callback, result, Completion::Job)
}

/// A promise settled with `result`.
pub(crate) fn settled(c: &mut Context<'_>, result: Outcome) -> Outcome {
    let promise = c.pending_promise_rooted()?;
    match result {
        Ok(value) => c.resolve_promise_rooted(promise, value)?,
        Err(error) => {
            let reason = errors::caught(error)?;
            c.reject_promise_rooted(promise, reason)?;
        }
    }
    Ok(promise)
}

// ---- Class construction --------------------------------------------------------------

/// How a class member is installed: assigned on a prototype (enumerable), or declared in a
/// class body (non-enumerable).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Members {
    Assigned,
    Declared,
}

pub(crate) fn define_methods(
    c: &mut Context<'_>,
    target: RootId,
    methods: &[(&str, &str)],
    members: Members,
) -> Result<(), RootedError> {
    for &(name, operation) in methods {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(target, key, function, true, members == Members::Assigned, true)?;
    }
    Ok(())
}

pub(crate) fn define_getters(
    c: &mut Context<'_>,
    target: RootId,
    getters: &[(&str, &str)],
    members: Members,
) -> Result<(), RootedError> {
    for &(name, operation) in getters {
        let getter = c.host_function_named(crate::host::shared_vm::operation(operation), &format!("get {name}"))?;
        let key = c.string_rooted(name);
        let fields = DescriptorFields {
            get: Some(getter),
            set: Some(c.undefined()),
            enumerable: Some(members == Members::Assigned),
            configurable: Some(true),
            ..DescriptorFields::default()
        };
        c.define_property_fields_rooted(target, key, fields)?;
    }
    Ok(())
}

/// A constructor whose prototype inherits from `parent`'s (and the constructor from it).
pub(crate) fn subclass(c: &mut Context<'_>, operation: &str, name: &str, parent: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation(operation), name)?;
    inherit(c, constructor, parent)?;
    Ok(constructor)
}

/// `Symbol.toStringTag` on a prototype, as WebIDL classes define it.
pub(crate) fn to_string_tag(c: &mut Context<'_>, prototype: RootId, tag: &str) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = get(c, symbol, "toStringTag")?;
    let value = c.string_rooted(tag);
    c.define_data_property_rooted(prototype, key, value, false, false, true)?;
    Ok(())
}

/// The receiver as a constructor sees it: `this` under `new`, else a fresh instance (the
/// legacy factories `Hash(...)` and friends allow calls without `new`).
pub(crate) fn construct_if_called(c: &mut Context<'_>, this: RootId, class_id: Class, args: &[RootId]) -> Result<Option<RootId>, RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(None);
    }
    let constructor = class(c, class_id)?;
    if c.is_object_rooted(this)? && c.instance_of_rooted(this, constructor)? {
        return Ok(None);
    }
    c.construct_rooted(constructor, constructor, args).map(Some)
}

/// The `stream` module's class `name` (`Transform`, `Writable`).
fn stream_class(c: &mut Context<'_>, name: &str) -> Outcome {
    let stream = crate::modules::stream_shared_vm::module(c)?;
    get(c, stream, name)
}

// ---- The module ----------------------------------------------------------------------

/// The `crypto` module's functions: `(export name, host operation, function name)`.
const FUNCTIONS: &[(&str, &str, &str)] = &[
    ("checkPrime", "cryptoCheckPrime", "checkPrime"),
    ("checkPrimeSync", "cryptoCheckPrimeSync", "checkPrimeSync"),
    ("createCipheriv", "cryptoCreateCipheriv", "createCipheriv"),
    ("createDecipheriv", "cryptoCreateDecipheriv", "createDecipheriv"),
    ("createDiffieHellman", "cryptoCreateDiffieHellman", "createDiffieHellman"),
    ("createDiffieHellmanGroup", "cryptoCreateDiffieHellmanGroup", "createDiffieHellmanGroup"),
    ("createECDH", "cryptoCreateEcdh", "createECDH"),
    ("createHash", "cryptoCreateHash", "createHash"),
    ("createHmac", "cryptoCreateHmac", "createHmac"),
    ("createPrivateKey", "cryptoCreatePrivateKey", "createPrivateKey"),
    ("createPublicKey", "cryptoCreatePublicKey", "createPublicKey"),
    ("createSecretKey", "cryptoCreateSecretKey", "createSecretKey"),
    ("createSign", "cryptoCreateSign", "createSign"),
    ("createVerify", "cryptoCreateVerify", "createVerify"),
    ("diffieHellman", "cryptoDiffieHellmanStateless", "diffieHellman"),
    ("generatePrime", "cryptoGeneratePrime", "generatePrime"),
    ("generatePrimeSync", "cryptoGeneratePrimeSync", "generatePrimeSync"),
    ("getCiphers", "cryptoGetCiphers", ""),
    ("getCipherInfo", "cryptoGetCipherInfo", "getCipherInfo"),
    ("getCurves", "cryptoGetCurves", ""),
    ("getDiffieHellman", "cryptoCreateDiffieHellmanGroup", "createDiffieHellmanGroup"),
    ("getHashes", "cryptoGetHashes", ""),
    ("hkdf", "cryptoHkdfAsync", "hkdf"),
    ("hkdfSync", "cryptoHkdfSync", "hkdfSync"),
    ("pbkdf2", "cryptoPbkdf2Async", "pbkdf2"),
    ("pbkdf2Sync", "cryptoPbkdf2Sync", "pbkdf2Sync"),
    ("generateKeyPair", "cryptoGenerateKeyPair", "generateKeyPair"),
    ("generateKeyPairSync", "cryptoGenerateKeyPairSync", "generateKeyPairSync"),
    ("generateKey", "cryptoGenerateKey", "generateKey"),
    ("generateKeySync", "cryptoGenerateKeySync", "generateKeySync"),
    ("privateDecrypt", "cryptoPrivateDecrypt", ""),
    ("privateEncrypt", "cryptoPrivateEncrypt", ""),
    ("publicDecrypt", "cryptoPublicDecrypt", ""),
    ("publicEncrypt", "cryptoPublicEncrypt", ""),
    ("randomBytes", "cryptoRandomBytesApi", "randomBytes"),
    ("randomFill", "cryptoRandomFill", "randomFill"),
    ("randomFillSync", "cryptoRandomFillSync", "randomFillSync"),
    ("randomInt", "cryptoRandomInt", "randomInt"),
    ("randomUUID", "cryptoRandomUuid", "randomUUID"),
    ("scrypt", "cryptoScryptAsync", "scrypt"),
    ("scryptSync", "cryptoScryptSync", "scryptSync"),
    ("sign", "cryptoSignOneShot", "signOneShot"),
    ("setEngine", "cryptoSetEngineApi", "setEngine"),
    ("timingSafeEqual", "cryptoTimingSafeEqual", ""),
    ("getFips", "cryptoGetFips", "getFips"),
    ("setFips", "cryptoSetFips", "setFips"),
    ("verify", "cryptoVerifyOneShot", "verifyOneShot"),
    ("hash", "cryptoHashOneShot", "hash"),
];

/// The module's class members, by export name; the removed legacy `Cipher` and
/// `Decipher` remain own members holding `undefined`.
const CLASS_EXPORTS: [(&str, Option<Class>); 14] = [
    ("Certificate", Some(Class::Certificate)),
    ("Cipher", None),
    ("Cipheriv", Some(Class::Cipheriv)),
    ("Decipher", None),
    ("Decipheriv", Some(Class::Decipheriv)),
    ("DiffieHellman", Some(Class::DiffieHellman)),
    ("DiffieHellmanGroup", Some(Class::DiffieHellmanGroup)),
    ("ECDH", Some(Class::Ecdh)),
    ("Hash", Some(Class::Hash)),
    ("Hmac", Some(Class::Hmac)),
    ("KeyObject", Some(Class::KeyObject)),
    ("Sign", Some(Class::Sign)),
    ("Verify", Some(Class::Verify)),
    ("X509Certificate", Some(Class::X509Certificate)),
];

/// The class exports Node wraps in `util.deprecate`: `(name, message, code)`.
const DEPRECATED_EXPORTS: [(&str, &str, &str); 2] = [
    ("Hash", "crypto.Hash constructor is deprecated.", "DEP0179"),
    ("Hmac", "crypto.Hmac constructor is deprecated.", "DEP0181"),
];

/// The `crypto` module, built on first use.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    if let Some(intrinsics) = state(c).borrow().crypto.intrinsics {
        return Ok(intrinsics.module);
    }
    let undefined = c.undefined();
    let handle = c.private_symbol_rooted(Some("kHandle"));
    let handle = c.retain(handle)?;
    let key_type = c.symbol_rooted(Some("kKeyType"));
    let key_type = c.retain(key_type)?;
    let module = c.object_rooted()?;
    let module = c.retain(module)?;
    let classes = [undefined; Class::COUNT];
    // Publish the symbol and module first: the classes' builders read them.
    state(c).borrow_mut().crypto.intrinsics = Some(Intrinsics { handle, key_type, classes, module, webcrypto: undefined });
    match build(c, module) {
        Ok(()) => Ok(module),
        Err(error) => {
            state(c).borrow_mut().crypto.intrinsics = None;
            Err(error)
        }
    }
}

fn build(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let transform = stream_class(c, "Transform")?;
    let writable = stream_class(c, "Writable")?;
    let mut built: Vec<(Class, RootId)> = Vec::new();
    let lazy = hash::lazy_transform(c, transform)?;
    built.extend(hash::build(c, lazy)?);
    built.extend(cipher::build(c, lazy)?);
    built.extend(sign::build(c, writable)?);
    built.extend(keys::build(c)?);
    built.extend(dh::build(c)?);
    built.extend(x509::build(c)?);
    built.extend(web::build(c)?);
    let mut classes = [c.undefined(); Class::COUNT];
    for (class, constructor) in built {
        classes[class as usize] = c.retain(constructor)?;
    }
    if let Some(intrinsics) = state(c).borrow_mut().crypto.intrinsics.as_mut() {
        intrinsics.classes = classes;
    }
    let webcrypto = web::create_crypto(c)?;
    let webcrypto = c.retain(webcrypto)?;
    if let Some(intrinsics) = state(c).borrow_mut().crypto.intrinsics.as_mut() {
        intrinsics.webcrypto = webcrypto;
    }
    for &(name, operation, own_name) in FUNCTIONS {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), own_name)?;
        put(c, module, name, function)?;
    }
    let util = crate::modules::util_shared_vm::module(c)?;
    for (name, class_id) in CLASS_EXPORTS {
        let mut constructor = match class_id {
            Some(class_id) => classes[class_id as usize],
            None => c.undefined(),
        };
        if let Some((_, message, code)) = DEPRECATED_EXPORTS.iter().find(|(deprecated, ..)| *deprecated == name) {
            let message = c.string_rooted(message);
            let code = c.string_rooted(code);
            constructor = call_method(c, util, "deprecate", &[constructor, message, code])?;
        }
        put(c, module, name, constructor)?;
    }
    let secure_heap_used = c.host_function_named(crate::host::shared_vm::operation!("cryptoSecureHeapUsed"), "secureHeapUsed")?;
    put(c, module, "secureHeapUsed", secure_heap_used)?;
    let get_fips = get(c, module, "getFips")?;
    let set_fips = get(c, module, "setFips")?;
    let key = c.string_rooted("fips");
    let fields = DescriptorFields {
        get: Some(get_fips),
        set: Some(set_fips),
        enumerable: Some(false),
        configurable: Some(false),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(module, key, fields)?;
    let constants = crate::modules::crypto_constants::binding(c)?;
    let key = c.string_rooted("constants");
    c.define_data_property_rooted(module, key, constants, false, true, false)?;
    for (name, operation) in [("webcrypto", "cryptoWebcryptoGetter"), ("subtle", "cryptoSubtleGetter"), ("getRandomValues", "cryptoGetRandomValuesGetter")] {
        let getter = c.host_function_named(crate::host::shared_vm::operation(operation), "get")?;
        let key = c.string_rooted(name);
        let fields = DescriptorFields {
            get: Some(getter),
            set: Some(c.undefined()),
            enumerable: Some(true),
            configurable: Some(false),
            ..DescriptorFields::default()
        };
        c.define_property_fields_rooted(module, key, fields)?;
    }
    let random_bytes = get(c, module, "randomBytes")?;
    for alias in RANDOM_BYTES_ALIASES {
        let name = c.string_rooted(alias);
        let getter = c.host_function_named_with_data(crate::host::shared_vm::operation!("cryptoAliasGet"), "get", random_bytes)?;
        let setter = c.host_function_named_with_data(crate::host::shared_vm::operation!("cryptoAliasSet"), "set", name)?;
        let fields = DescriptorFields {
            get: Some(getter),
            set: Some(setter),
            enumerable: Some(false),
            configurable: Some(true),
            ..DescriptorFields::default()
        };
        c.define_property_fields_rooted(module, name, fields)?;
    }
    Ok(())
}

/// The legacy aliases of `randomBytes`, kept as accessors the ecosystem still reads.
const RANDOM_BYTES_ALIASES: [&str; 3] = ["prng", "pseudoRandomBytes", "rng"];

/// An alias's getter: the function it names.
pub(crate) fn alias_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    c.host_function_data()
}

/// An alias or lazy property setter: an own data property replaces the accessor.
pub(crate) fn alias_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let name = c.host_function_data()?;
    let value = arg(c, args, 0);
    c.define_data_property_rooted(this, name, value, true, true, true)?;
    Ok(c.undefined())
}

pub(crate) fn webcrypto(c: &mut Context<'_>) -> Outcome {
    Ok(intrinsics(c)?.webcrypto)
}

/// `crypto.webcrypto` (and `globalThis.crypto`).
pub(crate) fn webcrypto_getter(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    webcrypto(c)
}

/// `crypto.subtle`.
pub(crate) fn subtle_getter(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let crypto = webcrypto(c)?;
    get(c, crypto, "subtle")
}

/// `crypto.getRandomValues`: a function that needs no `Crypto` receiver.
pub(crate) fn get_random_values_getter(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    c.host_function_named(crate::host::shared_vm::operation!("cryptoGetRandomValuesUnbound"), "getRandomValues")
}

/// `crypto.secureHeapUsed()`: no secure heap is configured.
pub(crate) fn secure_heap_used(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let result = c.object_rooted()?;
    for name in ["total", "used", "utilization", "min"] {
        let zero = c.number(0.0);
        put(c, result, name, zero)?;
    }
    Ok(result)
}

/// `crypto.getFips()`.
pub(crate) fn get_fips(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let enabled = crate::modules::crypto_provider_facts::fips_enabled();
    Ok(c.number(f64::from(u8::from(enabled))))
}

/// `crypto.setFips(enabled)`.
pub(crate) fn set_fips(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let enable = c.truthy_rooted(value)?;
    if enable && !crate::modules::crypto_provider_facts::set_fips(true) {
        return Err(errors::throw(c, CryptoError::FeatureUnavailable("OpenSSL FIPS mode")));
    }
    if !enable {
        crate::modules::crypto_provider_facts::set_fips(false);
    }
    Ok(c.undefined())
}

/// `crypto.setEngine(id[, flags])`: no engine is loadable under OpenSSL 3's providers.
pub(crate) fn set_engine(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (id, flags) = (arg(c, args, 0), arg(c, args, 1));
    let id = string_argument(c, id, "id")?;
    if c.truthy_rooted(flags)? && c.rooted_value(flags).and_then(|value| value.as_number()).is_none() {
        return Err(errors::invalid_type(c, flags, "flags", "of type number"));
    }
    Err(errors::throw(c, CryptoError::EngineUnknown(&id)))
}

/// Installs `globalThis.crypto` and the lazy WebCrypto class globals.
pub(crate) fn install_global(c: &mut Context<'_>) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let exceptions = c.host_function(crate::host::shared_vm::operation!("webDomExceptionInitialize"))?;
    c.define_lazy_data_properties_rooted(global, &["DOMException"], false, exceptions)?;
    let initialize = c.host_function(crate::host::shared_vm::operation!("cryptoWebClassGlobals"))?;
    c.define_lazy_data_properties_rooted(global, &["Crypto", "SubtleCrypto", "CryptoKey"], false, initialize)?;
    let getter = c.host_function_named(crate::host::shared_vm::operation!("get crypto"), "get crypto")?;
    let key = c.string_rooted("crypto");
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(c.undefined()),
        enumerable: Some(true),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(global, key, fields)?;
    Ok(())
}

/// The lazy `Crypto`, `SubtleCrypto` and `CryptoKey` globals' initializer.
pub(crate) fn web_class_globals(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let global = c.global_root()?;
    for (name, class_id) in [("Crypto", Class::Crypto), ("SubtleCrypto", Class::SubtleCrypto), ("CryptoKey", Class::CryptoKey)] {
        let constructor = class(c, class_id)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(global, key, constructor, true, false, true)?;
    }
    Ok(c.undefined())
}
