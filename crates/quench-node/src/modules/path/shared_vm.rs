//! Shared-VM adapter for the canonical Node Path string algorithms.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

#[derive(Clone, Copy)]
enum Flavor {
    Posix,
    Win32,
}

#[derive(Clone, Copy)]
enum Method {
    Join,
    Resolve,
    Relative,
}

impl Flavor {
    const ALL: [Self; 2] = [Self::Posix, Self::Win32];

    fn name(self) -> &'static str {
        match self {
            Self::Posix => "posix",
            Self::Win32 => "win32",
        }
    }

    fn separator(self) -> &'static str {
        match self {
            Self::Posix => "/",
            Self::Win32 => "\\",
        }
    }

    fn delimiter(self) -> &'static str {
        match self {
            Self::Posix => ":",
            Self::Win32 => ";",
        }
    }
}

impl Method {
    const ALL: [Self; 3] = [Self::Join, Self::Resolve, Self::Relative];

    fn name(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Resolve => "resolve",
            Self::Relative => "relative",
        }
    }

    fn operation_name(self) -> &'static str {
        match self {
            Self::Join => "pathJoin",
            Self::Resolve => "pathResolve",
            Self::Relative => "pathRelative",
        }
    }
}

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    if let Some(module) = context.host_mut().state().borrow().path_module {
        return Ok(module);
    }

    let mut namespaces = Vec::with_capacity(Flavor::ALL.len());
    for flavor in Flavor::ALL {
        let namespace = context.object_rooted()?;
        for method in Method::ALL {
            let data = context.string_rooted(flavor.name());
            let operation = crate::host::shared_vm::operation(method.operation_name());
            let function = context.host_function_with_data(operation, data)?;
            set(context, namespace, method.name(), function)?;
        }
        set_text(context, namespace, "sep", flavor.separator())?;
        set_text(context, namespace, "delimiter", flavor.delimiter())?;
        namespaces.push(namespace);
    }
    let [posix, win32] = namespaces.as_slice() else {
        return Err(RootedError::host("invalid Path namespace table"));
    };
    set(context, *posix, "posix", *posix)?;
    set(context, *posix, "win32", *win32)?;
    set(context, *win32, "posix", *posix)?;
    set(context, *win32, "win32", *win32)?;

    let platform = if cfg!(windows) { *win32 } else { *posix };
    let retained = context.retain(platform)?;
    context.host_mut().state().borrow_mut().path_module = Some(retained);
    Ok(platform)
}

fn namespace(
    context: &mut NativeContext<'_, NodeHost>,
    flavor: Flavor,
) -> Result<RootId, RootedError> {
    let module = module(context)?;
    get(context, module, flavor.name())
}

pub(crate) fn posix(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    namespace(context, Flavor::Posix)
}

pub(crate) fn win32(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    namespace(context, Flavor::Win32)
}

pub(crate) fn join_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Join)
}

pub(crate) fn resolve_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Resolve)
}

pub(crate) fn relative_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Relative)
}

fn apply(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
    method: Method,
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let flavor = context
        .string_text(data)?
        .and_then(parse_flavor)
        .ok_or_else(|| RootedError::host("invalid Path operation data"))?;
    let paths = args
        .iter()
        .enumerate()
        .map(|(index, argument)| argument_string(context, *argument, index))
        .collect::<Result<Vec<_>, _>>()?;

    let result = match (flavor, method) {
        (Flavor::Posix, Method::Join) => crate::modules::path_posix::join_strings(&paths),
        (Flavor::Win32, Method::Join) => crate::modules::path_win32_extra::join_strings(&paths),
        (Flavor::Posix, Method::Resolve) => {
            let cwd = process_cwd(context)?;
            crate::modules::path_posix::resolve_strings(&paths, &cwd)
        }
        (Flavor::Win32, Method::Resolve) => {
            let cwd = process_cwd(context)?;
            crate::modules::path_win32::resolve_strings(&paths, &cwd, |device| {
                process_drive_cwd(context, device, &cwd).unwrap_or_else(|_| cwd.clone())
            })
        }
        (Flavor::Posix, Method::Relative) => {
            let cwd = process_cwd(context)?;
            let from = paths.first().map(String::as_str).unwrap_or("undefined");
            let to = paths.get(1).map(String::as_str).unwrap_or("undefined");
            crate::modules::path_posix::relative_strings(from, to, &cwd)
        }
        (Flavor::Win32, Method::Relative) => {
            let cwd = process_cwd(context)?;
            let from = paths.first().map(String::as_str).unwrap_or("undefined");
            let to = paths.get(1).map(String::as_str).unwrap_or("undefined");
            crate::modules::path_win32_extra::relative_strings(
                from,
                to,
                &cwd,
                |device| process_drive_cwd(context, device, &cwd).unwrap_or_else(|_| cwd.clone()),
            )
        }
    };
    Ok(context.string_rooted(&result))
}

fn parse_flavor(name: String) -> Option<Flavor> {
    match name.as_str() {
        "posix" => Some(Flavor::Posix),
        "win32" => Some(Flavor::Win32),
        _ => None,
    }
}

fn argument_string(
    context: &mut NativeContext<'_, NodeHost>,
    argument: RootId,
    index: usize,
) -> Result<String, RootedError> {
    if let Some(value) = context.string_text(argument)? {
        return Ok(value);
    }
    let name = match index {
        0 => "paths[0]",
        1 => "paths[1]",
        _ => "path",
    };
    let error = context.type_error_rooted(&format!(
        "The \"{name}\" argument must be of type string. Received an instance of Object"
    ))?;
    let key = context.string_rooted("code");
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let _ = context.set_property_rooted(error, key, code, error)?;
    Err(context.throw(error))
}

fn process_cwd(context: &mut NativeContext<'_, NodeHost>) -> Result<String, RootedError> {
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let cwd = get(context, process, "cwd")?;
    if context.is_callable_rooted(cwd)? {
        let value = context.call_rooted(cwd, process, &[])?;
        return context
            .string_text(value)?
            .ok_or_else(|| RootedError::host("process.cwd() returned a non-string"));
    }
    Ok(context
        .host_mut()
        .state()
        .borrow()
        .process
        .cwd
        .to_string_lossy()
        .into_owned())
}

fn process_drive_cwd(
    context: &mut NativeContext<'_, NodeHost>,
    device: &str,
    fallback: &str,
) -> Result<String, RootedError> {
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let env = get(context, process, "env")?;
    let key = context.string_rooted(&format!("={device}"));
    let value = context.get_property_rooted(env, key)?;
    let path = context
        .string_text(value)?
        .unwrap_or_else(|| fallback.to_owned());
    let chars: Vec<char> = path.chars().collect();
    let drive_matches = chars.len() >= 2
        && chars[..2]
            .iter()
            .collect::<String>()
            .eq_ignore_ascii_case(device);
    if !drive_matches && chars.get(2) == Some(&'\\') {
        return Ok(format!("{device}\\"));
    }
    Ok(path)
}

fn set_text(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install Path property {name}")))
    }
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
