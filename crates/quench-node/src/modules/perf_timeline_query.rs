//! Select original user-timing entries before invoking the shared stable sort.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

/// Private facade protocol; public argument values never choose a query kind.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum QueryKind {
    All = 0,
    Name = 1,
    Type = 2,
}

impl QueryKind {
    fn from_number(value: f64) -> Result<Self, RootedError> {
        for kind in [Self::All, Self::Name, Self::Type] {
            if value == f64::from(kind as u8) {
                return Ok(kind);
            }
        }
        Err(RootedError::host("unknown private performance query kind"))
    }
}

fn coded_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
    code: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(message)?;
    let key = context.string_rooted("code");
    let value = context.string_rooted(code);
    context.set_property_rooted(error, key, value, error)?;
    Err(context.throw(error))
}

pub(crate) fn entries(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let brand = get(context, data, "brand")?;
    let branded = if context.is_object_rooted(args[0])?
        && !context
            .rooted_value(args[0])
            .is_some_and(|value| value.is_null())
    {
        let descriptor = context.own_property_descriptor_rooted(args[0], brand)?;
        !context
            .rooted_value(descriptor)
            .is_some_and(|value| value.is_undefined())
    } else {
        false
    };
    if !branded {
        return coded_type_error(
            context,
            "Value of \"this\" must be of type Performance",
            "ERR_INVALID_THIS",
        );
    }
    let mode = context
        .rooted_value(args[5])
        .and_then(|v| v.as_number())
        .ok_or_else(|| RootedError::host("private performance query kind missing"))?;
    let mode = QueryKind::from_number(mode)?;
    let provided = context
        .rooted_value(args[8])
        .and_then(|v| v.as_number())
        .unwrap_or(0.0);
    if mode != QueryKind::All && provided == 0.0 {
        return coded_type_error(
            context,
            if mode == QueryKind::Name {
                "The \"name\" argument must be specified"
            } else {
                "The \"type\" argument must be specified"
            },
            "ERR_MISSING_ARGS",
        );
    }
    let coerce = get(context, data, "coerce")?;
    let undefined = context.undefined();
    let name = if mode == QueryKind::Name {
        Some(context.call_rooted(coerce, undefined, &[args[6]])?)
    } else {
        None
    };
    let type_input = if mode == QueryKind::Type {
        args[6]
    } else {
        args[7]
    };
    let kind = if mode == QueryKind::Type
        || !context
            .rooted_value(type_input)
            .is_some_and(|v| v.is_undefined())
    {
        Some(context.call_rooted(coerce, undefined, &[type_input])?)
    } else {
        None
    };
    let name_key = context.string_rooted("name");
    let length_key = context.string_rooted("length");
    let mut selected = Vec::new();
    for (array, entry_type) in [
        (args[1], "mark"),
        (args[2], "measure"),
        (args[3], "resource"),
        (args[4], "function"),
    ] {
        if let Some(kind) = kind {
            let label = context.string_rooted(entry_type);
            let include = context.strict_equal_rooted(kind, label)?;
            context.release_root(label);
            if !include {
                continue;
            }
        }
        let length = context.get_property_rooted(array, length_key)?;
        let length_value = context
            .rooted_value(length)
            .and_then(|v| v.as_number())
            .unwrap_or(0.0) as u64;
        context.release_root(length);
        for index in 0..length_value {
            let key = context.string_rooted(&index.to_string());
            let entry = context.get_property_rooted(array, key)?;
            context.release_root(key);
            let include = if let Some(name) = name {
                let entry_name = context.get_property_rooted(entry, name_key)?;
                let matches = context.strict_equal_rooted(entry_name, name)?;
                context.release_root(entry_name);
                matches
            } else {
                true
            };
            if include {
                selected.push(entry);
            } else {
                context.release_root(entry);
            }
        }
    }
    let result = context.array_rooted(&selected)?;
    let sort = get(context, data, "sort")?;
    let compare = get(context, data, "compare")?;
    context.call_rooted(sort, result, &[compare])
}

pub(crate) fn compare(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let first = get(context, args[0], "startTime")?;
    let second = get(context, args[1], "startTime")?;
    context.subtract_rooted(first, second)
}
