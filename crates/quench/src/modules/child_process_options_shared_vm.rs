//! Child options are snapshotted once, validated in Node order, then projected
//! into syscall-safe identity values. No guest access runs in pre-exec.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::process::Command;

#[derive(Clone, Copy)]
enum Rule {
    Boolean,
    Int32,
    Shell,
    String,
}
const RULES: &[(&str, Rule)] = &[
    ("detached", Rule::Boolean),
    ("uid", Rule::Int32),
    ("gid", Rule::Int32),
    ("shell", Rule::Shell),
    ("argv0", Rule::String),
    ("windowsHide", Rule::Boolean),
    ("windowsVerbatimArguments", Rule::Boolean),
];

pub(crate) fn prepare(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let synchronous = args
        .get(1)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_bool())
        == Some(true);
    let undefined = context.undefined();
    let original = args.first().copied().unwrap_or(undefined);
    if context
        .rooted_value(original)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(undefined);
    }
    if !context.is_object_rooted(original)?
        || context.is_array_rooted(original)?
        || context.is_callable_rooted(original)?
    {
        return Err(super::util_shared_vm::argument_type_error(
            context, original, "options", "object",
        )?);
    }
    let options = snapshot(context, original)?;
    let cwd = get(context, options, "cwd")?;
    if !nullish(context, cwd) {
        let cwd = super::child_process_shared_vm::validate_cwd(context, cwd)?;
        let key = context.string_rooted("cwd");
        context.define_data_property_rooted(options, key, cwd, true, true, true)?;
    }
    for (name, rule) in RULES {
        let value = get(context, options, name)?;
        if nullish(context, value) {
            continue;
        }
        let property = format!("options.{name}");
        match rule {
            Rule::Boolean => {
                if context
                    .rooted_value(value)
                    .and_then(|value| value.as_bool())
                    .is_none()
                {
                    return Err(super::util_shared_vm::argument_type_error(
                        context, value, &property, "boolean",
                    )?);
                }
            }
            Rule::Int32 => {
                let Some(number) = context
                    .rooted_value(value)
                    .and_then(|value| value.as_number())
                else {
                    return Err(super::util_shared_vm::argument_type_error(
                        context, value, &property, "number",
                    )?);
                };
                let range = if !number.is_finite() || number.fract() != 0.0 {
                    Some("an integer".to_owned())
                } else if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&number) {
                    Some(format!(">= {} && <= {}", i32::MIN, i32::MAX))
                } else {
                    None
                };
                if let Some(range) = range {
                    let received = super::util_shared_vm::range_error_received(context, value)?;
                    let error = context.range_error_rooted(&format!("The value of \"{property}\" is out of range. It must be {range}. Received {received}"))?;
                    return Err(coded(context, error, "ERR_OUT_OF_RANGE")?);
                }
            }
            Rule::Shell => {
                if context.string_text(value)?.is_some() {
                    null_check(context, value, &property)?;
                } else if context
                    .rooted_value(value)
                    .and_then(|value| value.as_bool())
                    .is_none()
                {
                    return Err(super::util_shared_vm::argument_type_error_with_expected(
                        context,
                        value,
                        &property,
                        "one of type boolean or string",
                    )?);
                }
            }
            Rule::String => {
                if context.string_text(value)?.is_none() {
                    return Err(super::util_shared_vm::argument_type_error(
                        context, value, &property, "string",
                    )?);
                }
                null_check(context, value, &property)?;
            }
        }
    }
    let timeout = get(context, options, "timeout")?;
    super::child_process_exec_shared_vm::validate_limit(context, timeout, "timeout", true)?;
    #[cfg(unix)]
    {
        let signal = get(context, options, "killSignal")?;
        if !nullish(context, signal) {
            let number = super::child_process_shared_vm::sync_kill_signal(context, Some(options))?;
            let number = context.number(f64::from(number));
            let key = context.string_rooted("killSignal");
            context.define_data_property_rooted(options, key, number, true, true, true)?;
        }
    }
    if !synchronous {
        let serialization = get(context, options, "serialization")?;
        validate_serialization(context, serialization)?;
    }
    Ok(options)
}

fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
fn coded(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &str,
) -> Result<RootedError, RootedError> {
    let key = context.string_rooted("code");
    let code = context.string_rooted(code);
    context.define_data_property_rooted(error, key, code, true, true, true)?;
    Ok(context.throw(error))
}
pub(crate) fn null_check(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(), RootedError> {
    if !context
        .string_text(value)?
        .is_some_and(|value| value.contains('\0'))
    {
        return Ok(());
    }
    let inspect = super::util_shared_vm::internal_inspect(context)?;
    let undefined = context.undefined();
    let detail = context.call_rooted(inspect, undefined, &[value])?;
    let detail = context.to_string(detail)?;
    let kind = if name.contains('.') {
        "property"
    } else {
        "argument"
    };
    let error = context.type_error_rooted(&format!(
        "The {kind} '{name}' must be a string without null bytes. Received {detail}"
    ))?;
    Err(coded(context, error, "ERR_INVALID_ARG_VALUE")?)
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Identity {
    detached: bool,
    uid: Option<u32>,
    gid: Option<u32>,
}
impl Identity {
    pub(crate) fn from_options(
        context: &mut NativeContext<'_, NodeHost>,
        options: Option<RootId>,
    ) -> Result<Self, RootedError> {
        let Some(options) = options else {
            return Ok(Self::default());
        };
        // These fields are consumed at the process-launch boundary. Ignore
        // inherited values so Object.prototype pollution cannot change the
        // child identity or make an otherwise valid spawn fail with EPERM.
        let detached = own_option(context, options, "detached")?;
        let uid = own_option(context, options, "uid")?;
        let gid = own_option(context, options, "gid")?;
        Ok(Self {
            detached: context
                .rooted_value(detached)
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            uid: context
                .rooted_value(uid)
                .and_then(|value| value.as_number())
                .map(|value| value as i32 as u32),
            gid: context
                .rooted_value(gid)
                .and_then(|value| value.as_number())
                .map(|value| value as i32 as u32),
        })
    }
    pub(crate) fn configure(self, command: &mut Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            if !self.detached && self.uid.is_none() && self.gid.is_none() {
                return;
            }
            unsafe {
                command.pre_exec(move || {
                    if self.detached && libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if self.uid.is_some() || self.gid.is_some() {
                        libc::setgroups(0, std::ptr::null());
                    }
                    if let Some(gid) = self.gid {
                        if libc::setgid(gid) < 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    if let Some(uid) = self.uid {
                        if libc::setuid(uid) < 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
        }
        #[cfg(not(unix))]
        let _ = (self, command);
    }
}

fn own_option(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    let descriptor = context.own_property_descriptor_rooted(options, key)?;
    if context
        .rooted_value(descriptor)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(context.undefined());
    }
    context.get_property_rooted(options, key)
}

pub(crate) fn validate_file(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let value = args.first().copied().unwrap_or(undefined);
    file_text(context, value)?;
    Ok(value)
}
pub(crate) fn file_text(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    let Some(text) = context.string_text(value)? else {
        return Err(super::util_shared_vm::argument_type_error(
            context, value, "file", "string",
        )?);
    };
    null_check(context, value, "file")?;
    if text.is_empty() {
        let error =
            context.type_error_rooted("The argument 'file' cannot be empty. Received ''")?;
        return Err(coded(context, error, "ERR_INVALID_ARG_VALUE")?);
    }
    Ok(text)
}

fn snapshot(
    context: &mut NativeContext<'_, NodeHost>,
    original: RootId,
) -> Result<RootId, RootedError> {
    let options = context.null_object_rooted()?;
    for key in context.own_keys_rooted(original)? {
        let descriptor = context.own_property_descriptor_rooted(original, key)?;
        if context
            .rooted_value(descriptor)
            .is_some_and(|value| value.is_undefined())
        {
            continue;
        }
        let enumerable = get(context, descriptor, "enumerable")?;
        if !context.truthy_rooted(enumerable)? {
            continue;
        }
        let value = context.get_property_rooted(original, key)?;
        context.define_data_property_rooted(options, key, value, true, true, true)?;
    }
    Ok(options)
}

pub(crate) fn sync_options(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let original = args.first().copied().unwrap_or(undefined);
    let options = if nullish(context, original) {
        context.null_object_rooted()?
    } else {
        if !context.is_object_rooted(original)? || context.is_array_rooted(original)? {
            return Err(super::util_shared_vm::argument_type_error(
                context, original, "options", "object",
            )?);
        }
        snapshot(context, original)?
    };
    let force_shell = args
        .get(1)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_bool())
        == Some(true);
    if force_shell {
        let shell = get(context, options, "shell")?;
        if context.string_text(shell)?.is_none() {
            let key = context.string_rooted("shell");
            let shell = context.boolean(true);
            context.define_data_property_rooted(options, key, shell, true, true, true)?;
        }
    }
    Ok(options)
}

pub(super) fn validate_serialization(
    context: &mut NativeContext<'_, NodeHost>,
    serialization: RootId,
) -> Result<(), RootedError> {
    if !context
        .rooted_value(serialization)
        .is_some_and(|value| value.is_undefined())
        && !matches!(
            context.string_text(serialization)?.as_deref(),
            Some("json" | "advanced")
        )
    {
        let inspect = super::util_shared_vm::internal_inspect(context)?;
        let undefined = context.undefined();
        let detail = context.call_rooted(inspect, undefined, &[serialization])?;
        let detail = context.to_string(detail)?;
        let error = context.type_error_rooted(&format!("The property 'options.serialization' must be one of: undefined, 'json', 'advanced'. Received {detail}"))?;
        return Err(coded(context, error, "ERR_INVALID_ARG_VALUE")?);
    }
    Ok(())
}
