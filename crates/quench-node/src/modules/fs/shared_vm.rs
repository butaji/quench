use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let function = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    let key = context.string_rooted("readFileSync");
    if !context.set_property_rooted(module, key, function, module)? {
        return Err(RootedError::host("cannot install shared fs binding"));
    }
    Ok(module)
}

pub(crate) fn read_file_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    match std::fs::read(&path) {
        Ok(bytes) => {
            let encoding = if let Some(options) = args.get(1).copied() {
                if let Some(encoding) = context.string_text(options)? {
                    Some(encoding)
                } else {
                    let key = context.string_rooted("encoding");
                    let encoding = context.get_property_rooted(options, key)?;
                    context.string_text(encoding)?
                }
            } else {
                None
            };
            match encoding.as_deref() {
                Some("utf8" | "utf-8") => match String::from_utf8(bytes) {
                    Ok(text) => Ok(context.string_rooted(&text)),
                    Err(_) => {
                        let error = context.error_rooted("The input is not valid UTF-8")?;
                        Err(context.throw(error))
                    }
                },
                _ => {
                    let error = context.type_error_rooted(
                        "shared fs.readFileSync currently requires a UTF-8 encoding",
                    )?;
                    let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
                    let property = context.string_rooted("code");
                    if !context.set_property_rooted(error, property, code, error)? {
                        return Err(RootedError::host("cannot set fs error code"));
                    }
                    Err(context.throw(error))
                }
            }
        }
        Err(error) => {
            let code = fs_error_code(error.kind());
            let error = context.error_rooted(&format!(
                "{code}: {}, open '{path}'",
                std::io::Error::from_raw_os_error(error.raw_os_error().unwrap_or(5))
            ))?;
            let code_value = context.string_rooted(code);
            let property = context.string_rooted("code");
            if !context.set_property_rooted(error, property, code_value, error)? {
                return Err(RootedError::host("cannot set fs error code"));
            }
            Err(context.throw(error))
        }
    }
}

fn fs_error_code(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        _ => "EIO",
    }
}
