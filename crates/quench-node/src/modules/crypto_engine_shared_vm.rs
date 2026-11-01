//! Shared OpenSSL engine loading and errors for crypto and TLS.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) const ENGINE_METHOD_ALL: u32 = 0xffff;

pub(crate) const SUPPORTED: bool = cfg!(quench_openssl_engine);

#[cfg_attr(not(quench_openssl_engine), allow(dead_code))]
pub(crate) enum Failure {
    #[cfg(not(quench_openssl_engine))]
    Unsupported,
    Search(openssl::error::ErrorStack, String),
    Operation(openssl::error::ErrorStack),
    Initialize,
}

pub(crate) fn error(context: &mut NativeContext<'_, NodeHost>, failure: Failure) -> RootedError {
    match failure {
        Failure::Search(stack, name) if stack.errors().is_empty() => {
            match context.error_rooted(&format!("Engine \"{name}\" was not found")) {
                Ok(error) => context.throw(error),
                Err(error) => error,
            }
        }
        Failure::Search(stack, _) => super::crypto_error_shared_vm::list(context, stack),
        Failure::Operation(stack) => super::crypto_error_shared_vm::decorated(context, stack),
        Failure::Initialize => {
            super::tls_shared_vm::operation_error(context, "Failure to initialize engine")
        }
        #[cfg(not(quench_openssl_engine))]
        Failure::Unsupported => {
            let result = (|| {
                let error = context.error_rooted("Custom engines not supported by this OpenSSL")?;
                let key = context.string_rooted("code");
                let code = context.string_rooted("ERR_CRYPTO_CUSTOM_ENGINE_NOT_SUPPORTED");
                context.define_data_property_rooted(error, key, code, true, true, true)?;
                Ok::<_, RootedError>(context.throw(error))
            })();
            result.unwrap_or_else(|error| error)
        }
    }
}

fn engine_arguments(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<(String, u32), RootedError> {
    let id = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(name) = context.string_text(id)? else {
        return Err(super::util_shared_vm::argument_type_error(
            context, id, "id", "string",
        )?);
    };
    let flags = args.get(1).copied().unwrap_or_else(|| context.undefined());
    if context.truthy_rooted(flags)?
        && context
            .rooted_value(flags)
            .and_then(|value| value.as_number())
            .is_none()
    {
        return Err(super::util_shared_vm::argument_type_error(
            context, flags, "flags", "number",
        )?);
    }
    let zero = context.number(0.0);
    // The canonical VM operation performs the same integer coercion; reinterpret
    // its signed result as the unsigned OpenSSL method bitmask.
    let flags = context.bitwise_or_rooted(flags, zero)?;
    let flags = context
        .rooted_value(flags)
        .and_then(|value| value.as_number())
        .expect("integer coercion result") as i32 as u32;
    Ok((name, flags))
}

pub(crate) fn set_engine(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (name, flags) = engine_arguments(context, args)?;
    let flags = if flags == 0 { ENGINE_METHOD_ALL } else { flags };
    if let Err(_failure) = native::set_default(&name, flags) {
        #[cfg(not(quench_openssl_engine))]
        if matches!(_failure, Failure::Unsupported) {
            return Err(error(context, _failure));
        }
        let error = context.error_rooted(&format!("Engine \"{name}\" was not found"))?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_CRYPTO_ENGINE_UNKNOWN");
        context.define_data_property_rooted(error, key, code, true, true, true)?;
        return Err(context.throw(error));
    }
    Ok(context.undefined())
}

/// The private binding returns the actual native engine-selection result;
/// the configured standard library owns the public error and warning policy.
pub(crate) fn set_engine_binding(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (name, flags) = engine_arguments(context, args)?;
    let result = match native::set_default(&name, flags) {
        Ok(()) => true,
        #[cfg(not(quench_openssl_engine))]
        Err(Failure::Unsupported) => return Err(error(context, Failure::Unsupported)),
        Err(_) => false,
    };
    Ok(context.boolean(result))
}

pub(crate) fn set_client(
    configuration: &mut openssl::ssl::SslContextBuilder,
    name: &str,
) -> Result<(), Failure> {
    native::set_client(configuration, name)
}

pub(crate) fn set_key(
    configuration: &mut openssl::ssl::SslContextBuilder,
    name: &str,
    engine: &str,
) -> Result<(), Failure> {
    native::set_key(configuration, name, engine)
}

#[cfg(quench_openssl_engine)]
mod native {
    use super::Failure;
    use foreign_types::ForeignType;
    use std::{
        ffi::{c_char, c_int, c_uint, c_void, CString},
        ptr::NonNull,
        sync::{Once, OnceLock},
    };

    enum Reference {
        Structural,
        Functional,
    }
    struct Engine {
        pointer: NonNull<openssl_sys::ENGINE>,
        reference: Reference,
    }
    // OpenSSL owns synchronization and reference counting for ENGINE objects.
    // Rust only transfers owned references and invokes its documented C APIs.
    unsafe impl Send for Engine {}
    unsafe impl Sync for Engine {}
    impl Drop for Engine {
        fn drop(&mut self) {
            unsafe {
                match self.reference {
                    Reference::Structural => {
                        ENGINE_free(self.pointer.as_ptr());
                    }
                    Reference::Functional => {
                        ENGINE_finish(self.pointer.as_ptr());
                    }
                }
            }
        }
    }
    fn name(value: &str) -> CString {
        // Node's Utf8Value is passed as a nul-terminated C string.
        CString::new(super::super::crypto_strings::null_terminated_prefix(value))
            .expect("nul-free prefix")
    }
    fn load(id: &str) -> Result<Engine, Failure> {
        static INITIALIZE: Once = Once::new();
        INITIALIZE.call_once(|| unsafe {
            ENGINE_load_builtin_engines();
            ENGINE_register_all_complete();
        });
        // Isolate this operation's native errors from earlier OpenSSL calls.
        let _ = openssl::error::ErrorStack::get();
        let id = name(id);
        let mut pointer = unsafe { ENGINE_by_id(id.as_ptr()) };
        if pointer.is_null() {
            pointer = unsafe { ENGINE_by_id(c"dynamic".as_ptr()) };
            if !pointer.is_null() {
                let loaded = unsafe {
                    ENGINE_ctrl_cmd_string(pointer, c"SO_PATH".as_ptr(), id.as_ptr(), 0) != 0
                        && ENGINE_ctrl_cmd_string(pointer, c"LOAD".as_ptr(), std::ptr::null(), 0)
                            != 0
                };
                if !loaded {
                    unsafe {
                        ENGINE_free(pointer);
                    }
                    pointer = std::ptr::null_mut();
                }
            }
        }
        match NonNull::new(pointer) {
            Some(pointer) => {
                let _ = openssl::error::ErrorStack::get();
                Ok(Engine {
                    pointer,
                    reference: Reference::Structural,
                })
            }
            None => Err(Failure::Search(
                openssl::error::ErrorStack::get(),
                id.to_string_lossy().into_owned(),
            )),
        }
    }
    pub(super) fn set_default(id: &str, flags: u32) -> Result<(), Failure> {
        let engine = load(id)?;
        if unsafe { ENGINE_set_default(engine.pointer.as_ptr(), flags) } != 0 {
            Ok(())
        } else {
            Err(Failure::Operation(openssl::error::ErrorStack::get()))
        }
    }
    pub(super) fn set_client(
        configuration: &mut openssl::ssl::SslContextBuilder,
        id: &str,
    ) -> Result<(), Failure> {
        let engine = load(id)?;
        // SSL_CTX retains its own functional engine reference on success.
        if unsafe {
            SSL_CTX_set_client_cert_engine(configuration.as_ptr(), engine.pointer.as_ptr())
        } == 1
        {
            Ok(())
        } else {
            Err(Failure::Operation(openssl::error::ErrorStack::get()))
        }
    }
    pub(super) fn set_key(
        configuration: &mut openssl::ssl::SslContextBuilder,
        id: &str,
        engine_id: &str,
    ) -> Result<(), Failure> {
        let mut engine = load(engine_id)?;
        if unsafe { ENGINE_init(engine.pointer.as_ptr()) } != 1 {
            return Err(Failure::Initialize);
        }
        engine.reference = Reference::Functional;
        // ENGINE_init acquired a functional reference; relinquish the loader's
        // structural reference. ENGINE_finish later releases the retained one.
        unsafe {
            ENGINE_free(engine.pointer.as_ptr());
        }
        let id = name(id);
        let key = unsafe {
            ENGINE_load_private_key(
                engine.pointer.as_ptr(),
                id.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if key.is_null() {
            return Err(Failure::Operation(openssl::error::ErrorStack::get()));
        }
        let key: openssl::pkey::PKey<openssl::pkey::Private> =
            unsafe { openssl::pkey::PKey::from_ptr(key) };
        configuration
            .set_private_key(&key)
            .map_err(Failure::Operation)?;
        type OwnerIndex = openssl::ex_data::Index<openssl::ssl::SslContext, Engine>;
        static INDEX: OnceLock<Result<OwnerIndex, openssl::error::ErrorStack>> = OnceLock::new();
        let index = *INDEX
            .get_or_init(openssl::ssl::SslContext::new_ex_index::<Engine>)
            .as_ref()
            .map_err(|error| Failure::Operation(error.clone()))?;
        configuration.set_ex_data(index, engine);
        Ok(())
    }
    unsafe extern "C" {
        fn ENGINE_load_builtin_engines();
        fn ENGINE_register_all_complete() -> c_int;
        fn ENGINE_by_id(id: *const c_char) -> *mut openssl_sys::ENGINE;
        fn ENGINE_ctrl_cmd_string(
            engine: *mut openssl_sys::ENGINE,
            command: *const c_char,
            argument: *const c_char,
            optional: c_int,
        ) -> c_int;
        fn ENGINE_free(engine: *mut openssl_sys::ENGINE) -> c_int;
        fn ENGINE_finish(engine: *mut openssl_sys::ENGINE) -> c_int;
        fn ENGINE_init(engine: *mut openssl_sys::ENGINE) -> c_int;
        fn ENGINE_set_default(engine: *mut openssl_sys::ENGINE, flags: c_uint) -> c_int;
        fn ENGINE_load_private_key(
            engine: *mut openssl_sys::ENGINE,
            id: *const c_char,
            ui: *mut c_void,
            data: *mut c_void,
        ) -> *mut openssl_sys::EVP_PKEY;
        fn SSL_CTX_set_client_cert_engine(
            context: *mut openssl_sys::SSL_CTX,
            engine: *mut openssl_sys::ENGINE,
        ) -> c_int;
    }
}

#[cfg(not(quench_openssl_engine))]
mod native {
    use super::Failure;
    pub(super) fn set_default(_: &str, _: u32) -> Result<(), Failure> {
        Err(Failure::Unsupported)
    }
    pub(super) fn set_client(
        _: &mut openssl::ssl::SslContextBuilder,
        _: &str,
    ) -> Result<(), Failure> {
        Err(Failure::Unsupported)
    }
    pub(super) fn set_key(
        _: &mut openssl::ssl::SslContextBuilder,
        _: &str,
        _: &str,
    ) -> Result<(), Failure> {
        Err(Failure::Unsupported)
    }
}
