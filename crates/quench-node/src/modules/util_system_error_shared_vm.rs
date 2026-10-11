//! Platform error names shared by util and child-process failures.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn get_system_error_name(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(super::util_shared_vm::argument_type_error(
            context, value, "err", "number",
        )?);
    };
    if !number.is_finite()
        || number >= 0.0
        || number.fract() != 0.0
        || number.abs() > super::util_shared_vm::MAX_SAFE_INTEGER
    {
        let received = super::util_shared_vm::range_error_received(context, value)?;
        let error = context.range_error_rooted(&format!("The value of \"err\" is out of range. It must be a negative integer. Received {received}"))?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        context.set_property_rooted(error, key, code, error)?;
        return Err(context.throw(error));
    }
    Ok(context.string_rooted(&system_error_name(number)))
}

pub(crate) fn system_error_name(number: f64) -> String {
    // libuv's non-errno errors have stable values across platforms.
    const UV_ERRORS: &[(i32, &str)] = &[
        (-4094, "UNKNOWN"),
        (-4095, "EOF"),
        (-3000, "EAI_ADDRFAMILY"),
        (-3001, "EAI_AGAIN"),
        (-3002, "EAI_BADFLAGS"),
        (-3003, "EAI_CANCELED"),
        (-3004, "EAI_FAIL"),
        (-3005, "EAI_FAMILY"),
        (-3006, "EAI_MEMORY"),
        (-3007, "EAI_NODATA"),
        (-3008, "EAI_NONAME"),
        (-3009, "EAI_OVERFLOW"),
        (-3010, "EAI_SERVICE"),
        (-3011, "EAI_SOCKTYPE"),
        (-3013, "EAI_BADHINTS"),
        (-3014, "EAI_PROTOCOL"),
    ];
    if let Some((_, name)) = UV_ERRORS
        .iter()
        .find(|(code, _)| f64::from(*code) == number)
    {
        return (*name).to_owned();
    }
    #[cfg(unix)]
    {
        macro_rules! error_names {
            ($($name:ident),* $(,)?) => { &[$((-libc::$name, stringify!($name))),*] };
        }
        let names: &[(i32, &str)] = error_names![
            E2BIG,
            EACCES,
            EADDRINUSE,
            EADDRNOTAVAIL,
            EAFNOSUPPORT,
            EAGAIN,
            EALREADY,
            EBADF,
            EBUSY,
            ECANCELED,
            ECONNABORTED,
            ECONNREFUSED,
            ECONNRESET,
            EDESTADDRREQ,
            EEXIST,
            EFAULT,
            EFBIG,
            EHOSTUNREACH,
            EHOSTDOWN,
            EILSEQ,
            EINTR,
            EINVAL,
            EIO,
            EISCONN,
            EISDIR,
            ELOOP,
            EMFILE,
            EMLINK,
            EMSGSIZE,
            ENAMETOOLONG,
            ENETDOWN,
            ENETUNREACH,
            ENFILE,
            ENOBUFS,
            ENODEV,
            ENOENT,
            ENOEXEC,
            ENOMEM,
            ENOPROTOOPT,
            ENOSPC,
            ENOSYS,
            ENOTCONN,
            ENOTDIR,
            ENOTEMPTY,
            ENOTSOCK,
            ENOTSUP,
            ENOTTY,
            ENXIO,
            EOVERFLOW,
            EPERM,
            EPIPE,
            EPROTO,
            EPROTONOSUPPORT,
            EPROTOTYPE,
            ERANGE,
            EROFS,
            ESRCH,
            ESHUTDOWN,
            ESPIPE,
            ESOCKTNOSUPPORT,
            ETIMEDOUT,
            ETXTBSY,
            EXDEV,
        ];
        #[cfg(target_os = "linux")]
        {
            let extra: &[(i32, &str)] = error_names![ENODATA, ENONET, EUNATCH, EREMOTEIO];
            if let Some((_, name)) = extra.iter().find(|(code, _)| f64::from(*code) == number) {
                return (*name).to_owned();
            }
        }
        if let Some((_, name)) = names.iter().find(|(code, _)| f64::from(*code) == number) {
            return (*name).to_owned();
        }
    }
    #[cfg(not(unix))]
    {
        // Preserve the existing surface on other hosts pending their libuv mapping.
        let names = [
            (-2, "ENOENT"),
            (-8, "ENOEXEC"),
            (-9, "EBADF"),
            (-13, "EACCES"),
            (-22, "EINVAL"),
            (-88, "ENOTSOCK"),
            (-98, "EADDRINUSE"),
            (-99, "EADDRNOTAVAIL"),
            (-111, "ECONNREFUSED"),
            (-113, "EHOSTUNREACH"),
            (-101, "ENETUNREACH"),
            (-110, "ETIMEDOUT"),
            (-32, "EPIPE"),
        ];
        if let Some((_, name)) = names.iter().find(|(code, _)| f64::from(*code) == number) {
            return (*name).to_owned();
        }
    }
    format!("Unknown system error {number}")
}
