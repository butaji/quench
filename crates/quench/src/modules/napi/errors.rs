//! Errors and the pending exception.

use super::abi::node_api;
use super::{api, c_text, js_api, out, Context, Env, NapiValue, Status, Thrown};
use quench_runtime::{RootId, RootedError, ValueType};
use std::ffi::c_char;

/// The error constructors Node-API creates, each an intrinsic of the current realm.
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Error,
    TypeError,
    RangeError,
    SyntaxError,
}

fn construct(c: &mut Context<'_>, kind: Kind, message: &str) -> Result<RootId, RootedError> {
    match kind {
        Kind::Error => c.error_rooted(message),
        Kind::TypeError => c.type_error_rooted(message),
        Kind::RangeError => c.range_error_rooted(message),
        Kind::SyntaxError => c.syntax_error_rooted(message),
    }
}

fn set_code(c: &mut Context<'_>, error: RootId, code: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted("code");
    c.set_property_rooted(error, key, code, error).map(drop)
}

/// A host error carrying Node's `code` property, as C++ `THROW_ERR_*` creates it.
pub(super) fn coded_error(c: &mut Context<'_>, code: &str, message: &str) -> RootedError {
    let built = construct(c, Kind::Error, message).and_then(|error| {
        let code = c.string_rooted(code);
        set_code(c, error, code)?;
        Ok(error)
    });
    match built {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// Make a coded error of `kind` the pending exception (Node's `napi_throw_*_error`).
pub(super) fn throw_coded(
    env: &Env,
    c: &mut Context<'_>,
    kind: Kind,
    code: Option<&str>,
    message: &str,
) -> Status {
    let built = construct(c, kind, message).and_then(|error| {
        if let Some(code) = code {
            let code = c.string_rooted(code);
            set_code(c, error, code)?;
        }
        Ok(error)
    });
    match built {
        Ok(error) => env.throw_value(c, error),
        Err(error) => env.fail(c, error),
    }
}

pub(super) fn throw_range_error(
    env: &Env,
    c: &mut Context<'_>,
    code: &str,
    message: &str,
) -> Status {
    throw_coded(env, c, Kind::RangeError, Some(code), message)
}

fn throw(env: *mut Env, kind: Kind, code: *const c_char, message: *const c_char) -> Status {
    js_api(env, |env, c| {
        let message = c_text(message, super::AUTO_LENGTH)?;
        let code = if code.is_null() {
            None
        } else {
            Some(c_text(code, super::AUTO_LENGTH)?)
        };
        match throw_coded(env, c, kind, code.as_deref(), &message) {
            Status::PendingException => Ok(()),
            status => Err(status),
        }
    })
}

fn create(
    env: *mut Env,
    kind: Kind,
    code: NapiValue,
    message: NapiValue,
    result: *mut NapiValue,
) -> Status {
    api(env, |env, c| {
        let message = env.value(message)?;
        let message = c
            .string_text(message)
            .ok()
            .flatten()
            .ok_or(Status::StringExpected)?;
        let error = construct(c, kind, &message).napi(env, c)?;
        if !code.is_null() {
            let code = env.value(code)?;
            if c.value_type_rooted(code) != Some(ValueType::String) {
                return Err(Status::StringExpected);
            }
            set_code(c, error, code).napi(env, c)?;
        }
        out(result, env.handle(error))
    })
}

node_api! {
    fn napi_throw(env: *mut Env, error: NapiValue) -> Status {
        js_api(env, |env, c| {
            let error = env.value(error)?;
            env.throw_value(c, error);
            Ok(())
        })
    }

    fn napi_throw_error(env: *mut Env, code: *const c_char, message: *const c_char) -> Status {
        throw(env, Kind::Error, code, message)
    }

    fn napi_throw_type_error(env: *mut Env, code: *const c_char, message: *const c_char) -> Status {
        throw(env, Kind::TypeError, code, message)
    }

    fn napi_throw_range_error(env: *mut Env, code: *const c_char, message: *const c_char) -> Status {
        throw(env, Kind::RangeError, code, message)
    }

    fn node_api_throw_syntax_error(env: *mut Env, code: *const c_char, message: *const c_char) -> Status {
        throw(env, Kind::SyntaxError, code, message)
    }

    fn napi_create_error(env: *mut Env, code: NapiValue, message: NapiValue, result: *mut NapiValue) -> Status {
        create(env, Kind::Error, code, message, result)
    }

    fn napi_create_type_error(env: *mut Env, code: NapiValue, message: NapiValue, result: *mut NapiValue) -> Status {
        create(env, Kind::TypeError, code, message, result)
    }

    fn napi_create_range_error(env: *mut Env, code: NapiValue, message: NapiValue, result: *mut NapiValue) -> Status {
        create(env, Kind::RangeError, code, message, result)
    }

    fn node_api_create_syntax_error(env: *mut Env, code: NapiValue, message: NapiValue, result: *mut NapiValue) -> Status {
        create(env, Kind::SyntaxError, code, message, result)
    }

    fn napi_is_error(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            let brand = quench_runtime::BuiltinBrand::NativeError;
            out(result, c.has_builtin_brand_rooted(value, brand).napi(env, c)?)
        })
    }

    fn napi_is_exception_pending(env: *mut Env, result: *mut bool) -> Status {
        api(env, |env, _| out(result, env.pending_exception.get().is_some()))
    }

    fn napi_get_and_clear_last_exception(env: *mut Env, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let value = match env.take_exception() {
                Some(exception) => {
                    let local = c.scope_root(exception).ok_or(Status::GenericFailure);
                    c.release_root(exception);
                    local?
                }
                None => c.undefined(),
            };
            out(result, env.handle(value))
        })
    }

    fn napi_fatal_exception(env: *mut Env, error: NapiValue) -> Status {
        api(env, |env, c| {
            let error = env.value(error)?;
            super::work::report_uncaught(env, c, error);
            Ok(())
        })
    }
}
