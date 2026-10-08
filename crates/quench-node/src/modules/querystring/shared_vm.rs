//! Shared-VM adapter for the existing query-string parser.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let parse = context.host_function(crate::host::shared_vm::operation("querystringParse"))?;
    set(context, module, "parse", parse)?;
    set(context, module, "decode", parse)?;
    Ok(module)
}

pub(crate) fn parse(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let object = context.null_object_rooted()?;
    let Some(input) = args.first().copied() else {
        return Ok(object);
    };
    let Some(input) = context.string_text(input)? else {
        return Ok(object);
    };
    for (key, values) in crate::modules::querystring_parse::parse_default_entries(&input) {
        let value = if values.len() == 1 {
            context.string_rooted(&values[0])
        } else {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)?
        };
        set(context, object, &key, value)?;
    }
    Ok(object)
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
            "cannot install shared querystring property {name}"
        )))
    }
}
