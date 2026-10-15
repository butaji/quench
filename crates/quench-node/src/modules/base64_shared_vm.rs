//! WHATWG binary-string base64 adapters over the shared byte codec.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const BASE64_QUANTUM: usize = 4;
const PADDING: u16 = b'=' as u16;

pub(crate) fn install(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted("DOMException");
    let dom_exception = context.get_property_rooted(global, key)?;
    for name in ["atob", "btoa"] {
        let function = context
            .host_constructor_with_data(crate::host::shared_vm::operation(name), dom_exception)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, function, true, true, true)?;
    }
    Ok(())
}

fn input(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<Vec<u16>, RootedError> {
    let Some(value) = args.first().copied() else {
        let error = context.type_error_rooted("The \"input\" argument must be specified")?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_MISSING_ARGS");
        context.define_data_property_rooted(error, key, code, true, true, true)?;
        return Err(context.throw(error));
    };
    context.to_string_code_units(value)
}

fn invalid_character(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootedError, RootedError> {
    let constructor = context.host_function_data()?;
    let message = context.string_rooted(message);
    let name = context.string_rooted("InvalidCharacterError");
    let error = context.construct_rooted(constructor, constructor, &[message, name])?;
    Ok(context.throw(error))
}

pub(crate) fn btoa(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let units = input(context, args)?;
    let bytes: Option<Vec<u8>> = units
        .into_iter()
        .map(|unit| u8::try_from(unit).ok())
        .collect();
    let Some(bytes) = bytes else {
        return Err(invalid_character(context, "Invalid character")?);
    };
    let encoded = crate::modules::buffer_codec::base64_encode(&bytes, true, false);
    Ok(context.string_rooted(&encoded))
}

pub(crate) fn atob(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mut units = input(context, args)?;
    // Infra's ASCII whitespace deliberately excludes vertical tab and non-ASCII spaces.
    units.retain(|unit| !matches!(*unit, 0x09 | 0x0a | 0x0c | 0x0d | 0x20));
    if units.len().is_multiple_of(BASE64_QUANTUM) {
        if units.last() == Some(&PADDING) {
            units.pop();
        }
        if units.last() == Some(&PADDING) {
            units.pop();
        }
    }
    if units.iter().any(|unit| {
        !u8::try_from(*unit)
            .ok()
            .is_some_and(crate::modules::buffer_codec::is_standard_base64_digit)
    }) {
        return Err(invalid_character(context, "Invalid character")?);
    }
    if units.len() % BASE64_QUANTUM == 1 {
        return Err(invalid_character(
            context,
            "The string to be decoded is not correctly encoded.",
        )?);
    }
    let bytes: Vec<_> = units.into_iter().map(|unit| unit as u8).collect();
    let decoded = crate::modules::buffer_codec::base64_decode(&bytes);
    Ok(context.string_units_rooted(&decoded.into_iter().map(u16::from).collect::<Vec<_>>()))
}
