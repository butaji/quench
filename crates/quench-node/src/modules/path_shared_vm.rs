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
    IsAbsolute,
    ToNamespacedPath,
    Parse,
    Format,
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
    const ALL: [Self; 11] = [
        Self::Join,
        Self::Resolve,
        Self::Relative,
        Self::Basename,
        Self::Dirname,
        Self::Extname,
        Self::Normalize,
        Self::IsAbsolute,
        Self::ToNamespacedPath,
        Self::Parse,
        Self::Format,
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
            Self::IsAbsolute => "isAbsolute",
            Self::ToNamespacedPath => "toNamespacedPath",
            Self::Parse => "parse",
            Self::Format => "format",
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
            Self::IsAbsolute => "pathIsAbsolute",
            Self::ToNamespacedPath => "pathToNamespacedPath",
            Self::Parse => "pathParse",
            Self::Format => "pathFormat",
        }
    }

    fn argument_count(self, available: usize) -> usize {
        match self {
            Self::Join | Self::Resolve => available,
            Self::IsAbsolute => available.min(1),
            Self::ToNamespacedPath => available.min(1),
            Self::Parse | Self::Format => available.min(1),
            Self::Relative | Self::Basename => available.min(2),
            Self::Dirname | Self::Extname | Self::Normalize => available.min(1),
        }
    }

    fn path_count(self, available: usize) -> usize {
        match self {
            Self::Basename
            | Self::Dirname
            | Self::Extname
            | Self::Normalize
            | Self::IsAbsolute => available.min(1),
            Self::ToNamespacedPath => available.min(1),
            Self::Parse => available.min(1),
            Self::Format => 0,
            Self::Join | Self::Resolve | Self::Relative => available,
        }
    }
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(module) = context.host_mut().shared_state().borrow().path_module {
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
            if matches!(method, Method::ToNamespacedPath) {
                set(context, namespace, "_makeLong", function)?;
            }
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
    context.host_mut().shared_state().borrow_mut().path_module = Some(retained);
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

pub(crate) fn is_absolute_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::IsAbsolute)
}

pub(crate) fn to_namespaced_path_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::ToNamespacedPath)
}

pub(crate) fn parse_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Parse)
}

pub(crate) fn format_operation(
    context: &mut NativeContext<'_, NodeHost>,
    function: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    apply(context, function, args, Method::Format)
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
    if matches!(method, Method::ToNamespacedPath) {
        let Some(argument) = args.first().copied() else {
            return Ok(context.undefined());
        };
        let Some(path) = context.string_text(argument)? else {
            return Ok(argument);
        };
        if path.is_empty() || matches!(flavor, Flavor::Posix) {
            return Ok(argument);
        }
        let cwd = process_cwd(context)?;
        let resolved = crate::modules::path_algorithms::win32::resolve_strings(
            &[path],
            &cwd,
            |device| process_drive_cwd(context, device, &cwd).unwrap_or_else(|_| cwd.clone()),
        );
        let result =
            crate::modules::path_algorithms::win32_normalize::to_namespaced_path(&resolved);
        return Ok(context.string_rooted(&result));
    }
    if matches!(method, Method::Parse) {
        let Some(argument) = args.first().copied() else {
            return Err(path_type_error(context, "path", "undefined")?);
        };
        let path = argument_string(context, argument, 0, method)?;
        let parts = crate::modules::path_algorithms::parts::parse_str(
            &path,
            matches!(flavor, Flavor::Win32),
        );
        let object = context.object_rooted()?;
        for (key, value) in [
            ("root", parts.root),
            ("dir", parts.dir),
            ("base", parts.base),
            ("ext", parts.ext),
            ("name", parts.name),
        ] {
            set_text(context, object, key, &value)?;
        }
        return Ok(object);
    }
    if matches!(method, Method::Format) {
        return format_operation_result(context, args.first().copied(), flavor);
    }
    let arguments = &args[..method.argument_count(args.len())];
    let path_arguments = &arguments[..method.path_count(arguments.len())];
    if matches!(
        method,
        Method::Basename
            | Method::Dirname
            | Method::Extname
            | Method::Normalize
            | Method::IsAbsolute
            | Method::Parse
    ) && path_arguments.is_empty()
    {
        return Err(path_type_error(context, "path", "undefined")?);
    }
    let paths = path_arguments
        .iter()
        .enumerate()
        .map(|(index, argument)| argument_string(context, *argument, index, method))
        .collect::<Result<Vec<_>, _>>()?;
    if matches!(method, Method::IsAbsolute) {
        let windows = matches!(flavor, Flavor::Win32);
        let result = crate::modules::path_algorithms::common::is_absolute(&paths[0], windows);
        return Ok(context.boolean(result));
    }
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
        (Flavor::Posix, Method::Join) => {
            crate::modules::path_algorithms::posix::join_strings(&paths)
        }
        (Flavor::Win32, Method::Join) => {
            crate::modules::path_algorithms::win32_extra::join_strings(&paths)
        }
        (Flavor::Posix, Method::Resolve) => {
            let cwd = process_cwd(context)?;
            crate::modules::path_algorithms::posix::resolve_strings(&paths, &cwd)
        }
        (Flavor::Win32, Method::Resolve) => {
            let cwd = process_cwd(context)?;
            crate::modules::path_algorithms::win32::resolve_strings(&paths, &cwd, |device| {
                process_drive_cwd(context, device, &cwd).unwrap_or_else(|_| cwd.clone())
            })
        }
        (Flavor::Posix, Method::Relative) => {
            let cwd = process_cwd(context)?;
            let from = paths.first().map(String::as_str).unwrap_or("undefined");
            let to = paths.get(1).map(String::as_str).unwrap_or("undefined");
            crate::modules::path_algorithms::posix::relative_strings(from, to, &cwd)
        }
        (Flavor::Win32, Method::Relative) => {
            let cwd = process_cwd(context)?;
            let from = paths.first().map(String::as_str).unwrap_or("undefined");
            let to = paths.get(1).map(String::as_str).unwrap_or("undefined");
            crate::modules::path_algorithms::win32_extra::relative_strings(
                from,
                to,
                &cwd,
                |device| process_drive_cwd(context, device, &cwd).unwrap_or_else(|_| cwd.clone()),
            )
        }
        (Flavor::Posix, Method::Basename) => crate::modules::path_algorithms::parts::basename_str(
            &paths[0],
            suffix.as_deref(),
            false,
        ),
        (Flavor::Win32, Method::Basename) => {
            crate::modules::path_algorithms::parts::basename_str(&paths[0], suffix.as_deref(), true)
        }
        (Flavor::Posix, Method::Dirname) => {
            crate::modules::path_algorithms::posix::dirname_str(&paths[0])
        }
        (Flavor::Win32, Method::Dirname) => {
            crate::modules::path_algorithms::win32_extra::dirname_str(&paths[0])
        }
        (Flavor::Posix, Method::Extname) => {
            crate::modules::path_algorithms::parts::extname_str(&paths[0], false)
        }
        (Flavor::Win32, Method::Extname) => {
            crate::modules::path_algorithms::parts::extname_str(&paths[0], true)
        }
        (Flavor::Posix, Method::Normalize) => {
            crate::modules::path_algorithms::posix::normalize_str(&paths[0])
        }
        (Flavor::Win32, Method::Normalize) => {
            crate::modules::path_algorithms::win32_normalize::normalize_str(&paths[0])
        }
        (
            _,
            Method::IsAbsolute | Method::ToNamespacedPath | Method::Parse | Method::Format,
        ) => {
            unreachable!("handled before string path dispatch")
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
        (Method::IsAbsolute, 0) => "path",
        (Method::Parse, 0) => "path",
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

fn format_operation_result(
    context: &mut NativeContext<'_, NodeHost>,
    argument: Option<RootId>,
    flavor: Flavor,
) -> Result<RootId, RootedError> {
    let object = argument.unwrap_or_else(|| context.undefined());
    if !context.is_object_rooted(object)? {
        let value = context.rooted_value(object);
        let received = if value.is_some_and(|value| value.is_null()) {
            "null".to_owned()
        } else if value.is_some_and(|value| value.is_undefined()) {
            "undefined".to_owned()
        } else if let Some(boolean) = value.and_then(|value| value.as_bool()) {
            format!("type boolean ({boolean})")
        } else if value.is_some_and(|value| value.as_number().is_some()) {
            format!("type number ({})", context.to_string(object)?)
        } else if let Some(string) = context.string_text(object)? {
            format!("type string ('{}')", string.replace('\'', "\\'"))
        } else {
            "an instance of Object".to_owned()
        };
        let error = context.type_error_rooted(&format!(
            "The \"pathObject\" argument must be of type object. Received {received}"
        ))?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
        let _ = context.set_property_rooted(error, key, code, error)?;
        return Err(context.throw(error));
    }

    let key = context.string_rooted("dir");
    let dir = context.get_property_rooted(object, key)?;
    let key = context.string_rooted("root");
    let root = context.get_property_rooted(object, key)?;
    let directory = if context.truthy_rooted(dir)? { dir } else { root };

    let key = context.string_rooted("base");
    let base = context.get_property_rooted(object, key)?;
    let base = if context.truthy_rooted(base)? {
        context.to_string(base)?
    } else {
        let key = context.string_rooted("name");
        let name = context.get_property_rooted(object, key)?;
        let name = if context.truthy_rooted(name)? {
            context.to_string(name)?
        } else {
            String::new()
        };
        let key = context.string_rooted("ext");
        let ext = context.get_property_rooted(object, key)?;
        if !context.truthy_rooted(ext)? {
            name
        } else {
            let ext_text = context.to_string(ext)?;
            let key = context.string_rooted("0");
            let first = context.get_property_rooted(ext, key)?;
            if context.string_text(first)?.as_deref() == Some(".") {
                format!("{name}{ext_text}")
            } else {
                format!("{name}.{ext_text}")
            }
        }
    };
    if !context.truthy_rooted(directory)? {
        return Ok(context.string_rooted(&base));
    }
    let directory_text = context.to_string(directory)?;
    let same_root = context.same_value_rooted(directory, root)?;
    let result = if same_root {
        format!("{directory_text}{base}")
    } else {
        format!("{directory_text}{}{base}", flavor.separator())
    };
    Ok(context.string_rooted(&result))
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
        .shared_state()
        .borrow()
        .cwd
        .path()
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
