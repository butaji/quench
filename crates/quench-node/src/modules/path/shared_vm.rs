//! Shared-VM adapter for the canonical Node Path string algorithms.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

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
    Basename,
    Dirname,
    Extname,
    Normalize,
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
    const ALL: [Self; 7] = [
        Self::Join,
        Self::Resolve,
        Self::Relative,
        Self::Basename,
        Self::Dirname,
        Self::Extname,
        Self::Normalize,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Resolve => "resolve",
            Self::Relative => "relative",
            Self::Basename => "basename",
            Self::Dirname => "dirname",
            Self::Extname => "extname",
            Self::Normalize => "normalize",
        }
    }

    fn operation_name(self) -> &'static str {
        match self {
            Self::Join => "pathJoin",
            Self::Resolve => "pathResolve",
            Self::Relative => "pathRelative",
            Self::Basename => "pathBasename",
            Self::Dirname => "pathDirname",
            Self::Extname => "pathExtname",
            Self::Normalize => "pathNormalize",
        }
    }

    fn argument_count(self, available: usize) -> usize {
        match self {
            Self::Join | Self::Resolve => available,
            Self::Relative | Self::Basename => available.min(2),
            Self::Dirname | Self::Extname | Self::Normalize => available.min(1),
        }
    }

    fn path_count(self, available: usize) -> usize {
        match self {
            Self::Basename | Self::Dirname | Self::Extname | Self::Normalize => available.min(1),
            Self::Join | Self::Resolve | Self::Relative => available,
        }
    }
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
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

pub(crate) fn posix(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    namespace(context, Flavor::Posix)
}

pub(crate) fn win32(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
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

pub(crate) fn basename_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Basename)
}

pub(crate) fn dirname_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Dirname)
}

pub(crate) fn extname_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Extname)
}

pub(crate) fn normalize_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Normalize)
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
    let arguments = &args[..method.argument_count(args.len())];
    let path_arguments = &arguments[..method.path_count(arguments.len())];
    if matches!(
        method,
        Method::Basename | Method::Dirname | Method::Extname | Method::Normalize
    ) && path_arguments.is_empty()
    {
        return Err(path_type_error(context, "path", "undefined")?);
    }
    let paths = path_arguments
        .iter()
        .enumerate()
        .map(|(index, argument)| argument_string(context, *argument, index, method))
        .collect::<Result<Vec<_>, _>>()?;
    let suffix = if matches!(method, Method::Basename) {
        match arguments.get(1).copied() {
            Some(argument)
                if context
                    .rooted_value(argument)
                    .is_some_and(|value| value.is_undefined()) =>
            {
                None
            }
            Some(argument) => Some(argument_string(context, argument, 1, method)?),
            None => None,
        }
    } else {
        None
    };

    let result = match (flavor, method) {
        (Flavor::Posix, Method::Join) => crate::modules::path_algorithms::posix_join(&paths),
        (Flavor::Win32, Method::Join) => crate::modules::path_algorithms::win32_join(&paths),
        (Flavor::Posix, Method::Resolve) => {
            let cwd = process_cwd(context)?;
            crate::modules::path_algorithms::posix_resolve(&paths, &cwd)
        }
        (Flavor::Win32, Method::Resolve) => {
            let cwd = process_cwd(context)?;
            crate::modules::path_algorithms::win32_resolve(&paths, &cwd)
        }
        (Flavor::Posix, Method::Relative) => {
            let cwd = process_cwd(context)?;
            let from = paths.first().map(String::as_str).unwrap_or("undefined");
            let to = paths.get(1).map(String::as_str).unwrap_or("undefined");
            crate::modules::path_algorithms::posix_relative(from, to, &cwd)
        }
        (Flavor::Win32, Method::Relative) => {
            let cwd = process_cwd(context)?;
            let from = paths.first().map(String::as_str).unwrap_or("undefined");
            let to = paths.get(1).map(String::as_str).unwrap_or("undefined");
            crate::modules::path_algorithms::win32_relative(from, to, &cwd)
        }
        (Flavor::Posix, Method::Basename) => {
            crate::modules::path_algorithms::basename(&paths[0], suffix.as_deref(), false)
        }
        (Flavor::Win32, Method::Basename) => {
            crate::modules::path_algorithms::basename(&paths[0], suffix.as_deref(), true)
        }
        (Flavor::Posix, Method::Dirname) => crate::modules::path_algorithms::posix_dirname(&paths[0]),
        (Flavor::Win32, Method::Dirname) => {
            crate::modules::path_algorithms::win32_dirname(&paths[0])
        }
        (Flavor::Posix, Method::Extname) => {
            crate::modules::path_algorithms::extname(&paths[0], false)
        }
        (Flavor::Win32, Method::Extname) => {
            crate::modules::path_algorithms::extname(&paths[0], true)
        }
        (Flavor::Posix, Method::Normalize) => crate::modules::path_algorithms::posix_normalize(&paths[0]),
        (Flavor::Win32, Method::Normalize) => {
            crate::modules::path_algorithms::win32_normalize(&paths[0])
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
    method: Method,
) -> Result<String, RootedError> {
    if let Some(value) = context.string_text(argument)? {
        return Ok(value);
    }
    let name = match (method, index) {
        (Method::Basename | Method::Dirname | Method::Extname | Method::Normalize, 0) => "path",
        (Method::Basename, _) => "suffix",
        (_, 0) => "paths[0]",
        (_, 1) => "paths[1]",
        _ => "path",
    };
    Err(path_type_error(context, name, "an instance of Object")?)
}

fn path_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    received: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"{name}\" argument must be of type string. Received {received}"
    ))?;
    let key = context.string_rooted("code");
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let _ = context.set_property_rooted(error, key, code, error)?;
    Ok(context.throw(error))
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
        Err(RootedError::host(format!(
            "cannot install Path property {name}"
        )))
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
