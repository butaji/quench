//! Builtin labels and properties rendered within the canonical inspector's cycle state.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand as Brand, BuiltinData, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
const PRIMITIVE_BRANDS: [Brand; 6] = [
    Brand::Date,
    Brand::NumberObject,
    Brand::StringObject,
    Brand::BooleanObject,
    Brand::BigIntObject,
    Brand::SymbolObject,
];
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
pub(crate) fn function(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let date = get(c, global, "Date")?;
    let prototype = get(c, date, "prototype")?;
    let iso = get(c, prototype, "toISOString")?;
    let string = get(c, global, "String")?;
    let data = c.array_rooted(&[iso, string])?;
    let u8 = get(c, global, "Uint8Array")?;
    let u8 = get(c, u8, "prototype")?;
    let typed = c.prototype_rooted(u8)?;
    let dv = get(c, global, "DataView")?;
    let dv = get(c, dv, "prototype")?;
    let ab = get(c, global, "ArrayBuffer")?;
    let ab = get(c, ab, "prototype")?;
    let shared = get(c, global, "SharedArrayBuffer")?;
    let shared = get(c, shared, "prototype")?;
    for (holder, prefix, names) in [
        (
            typed,
            "typed",
            &["length", "byteLength", "byteOffset", "buffer"][..],
        ),
        (dv, "view", &["byteLength", "byteOffset", "buffer"][..]),
        (ab, "array", &["byteLength", "detached"][..]),
        (shared, "shared", &["byteLength"][..]),
    ] {
        for name in names {
            let key = c.string_rooted(name);
            let descriptor = c.own_property_descriptor_rooted(holder, key)?;
            let getter = get(c, descriptor, "get")?;
            let key = c.string_rooted(&format!("{prefix}:{name}"));
            c.define_data_property_rooted(data, key, getter, false, false, false)?;
        }
    }

    c.host_function_with_data(
        crate::host::shared_vm::operation("utilInspectBuiltin"),
        data,
    )
}
pub(crate) fn render(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let object = args.first().copied().unwrap_or(missing);
    if let Some(text) = binary(c, object, args)? {
        return Ok(text);
    }
    let mut builtin = false;
    for brand in PRIMITIVE_BRANDS {
        if c.has_builtin_brand_rooted(object, brand)? {
            builtin = true;
            break;
        }
    }
    if !builtin {
        return Ok(missing);
    }
    let Some(BuiltinData::Primitive { brand, value }) = c.builtin_data_rooted(object)? else {
        return Ok(missing);
    };
    let settings = args.get(1).copied().unwrap_or(missing);
    let depth = args.get(2).copied().unwrap_or(missing);
    let render = args.get(3).copied().unwrap_or(missing);
    let property_name = args.get(4).copied().unwrap_or(missing);
    let base = if brand == Brand::Date {
        if c.rooted_value(value)
            .and_then(|v| v.as_number())
            .is_some_and(f64::is_finite)
        {
            let data = c.host_function_data()?;
            let iso = get(c, data, "0")?;
            let text = c.call_rooted(iso, object, &[])?;
            c.to_string(text)?
        } else {
            "Invalid Date".to_owned()
        }
    } else {
        let label = match brand {
            Brand::NumberObject => "Number",
            Brand::StringObject => "String",
            Brand::BooleanObject => "Boolean",
            Brand::BigIntObject => "BigInt",
            Brand::SymbolObject => "Symbol",
            _ => unreachable!(),
        };
        let text = c.call_rooted(render, missing, &[value, depth])?;
        format!("[{label}: {}]", c.to_string(text)?)
    };
    let string_length = if brand == Brand::StringObject {
        c.string_code_units(value)?.map(|v| v.len())
    } else {
        None
    };
    let entries = properties(
        c,
        object,
        settings,
        depth,
        render,
        property_name,
        string_length,
    )?;
    Ok(c.string_rooted(&if entries.is_empty() {
        base
    } else {
        format!("{base} {{ {} }}", entries.join(", "))
    }))
}

fn properties(
    c: &mut Context<'_>,
    object: RootId,
    settings: RootId,
    depth: RootId,
    render: RootId,
    property_name: RootId,
    string_length: Option<usize>,
) -> Result<Vec<String>, RootedError> {
    let missing = c.undefined();
    let hidden = get(c, settings, "showHidden")?;
    let hidden = c.truthy_rooted(hidden)?;
    let next = c.number(
        c.rooted_value(depth)
            .and_then(|v| v.as_number())
            .unwrap_or_default()
            + 1.0,
    );
    let mut entries = Vec::new();
    let keys = if string_length.is_some() {
        c.own_non_index_keys_rooted(object, !hidden)?
    } else {
        c.own_keys_rooted(object)?
    };
    for key in keys {
        if let (Some(length), Some(text)) = (string_length, c.string_text(key)?) {
            if text
                .parse::<usize>()
                .ok()
                .is_some_and(|index| index < length && index.to_string() == text)
            {
                continue;
            }
        }
        let descriptor = c.own_property_descriptor_rooted(object, key)?;
        let enumerable = get(c, descriptor, "enumerable")?;
        let enumerable = c.truthy_rooted(enumerable)?;
        if !enumerable && !hidden {
            continue;
        }
        let mut label = if c.is_symbol_rooted(key)? {
            let data = c.host_function_data()?;
            let string = get(c, data, "1")?;
            let text = c.call_rooted(string, missing, &[key])?;
            format!("[{}]", c.to_string(text)?)
        } else {
            let label = c.call_rooted(property_name, missing, &[key])?;
            c.to_string(label)?
        };
        if !enumerable {
            label = format!("[{label}]");
        }
        let value_key = c.string_rooted("value");
        let own_value = c.own_property_descriptor_rooted(descriptor, value_key)?;
        let text = if !c.rooted_value(own_value).is_some_and(|v| v.is_undefined()) {
            let value = c.get_property_rooted(descriptor, value_key)?;
            let text = c.call_rooted(render, missing, &[value, next])?;
            c.to_string(text)?
        } else {
            let getter = get(c, descriptor, "get")?;
            let setter = get(c, descriptor, "set")?;
            match (c.is_callable_rooted(getter)?, c.is_callable_rooted(setter)?) {
                (true, true) => "[Getter/Setter]",
                (true, false) => "[Getter]",
                (false, true) => "[Setter]",
                _ => "undefined",
            }
            .to_owned()
        };
        entries.push(format!("{label}: {text}"));
    }
    Ok(entries)
}
fn intrinsic(
    c: &mut Context<'_>,
    data: RootId,
    name: &str,
    receiver: RootId,
) -> Result<RootId, RootedError> {
    let getter = get(c, data, name)?;
    c.call_rooted(getter, receiver, &[])
}
pub(crate) const DEFAULT_MAX_ARRAY_LENGTH: f64 = 100.0;
pub(crate) const DEFAULT_BREAK_LENGTH: f64 = 80.0;
fn option_limit(
    c: &mut Context<'_>,
    settings: RootId,
    name: &str,
    default: f64,
) -> Result<usize, RootedError> {
    let value = get(c, settings, name)?;
    if c.rooted_value(value).is_some_and(|v| v.is_null()) {
        return Ok(usize::MAX);
    }
    let value = if c.rooted_value(value).is_some_and(|v| v.is_undefined()) {
        default
    } else {
        c.to_number(value)?
    };
    Ok(value.max(0.0) as usize)
}
fn binary(
    c: &mut Context<'_>,
    object: RootId,
    args: &[RootId],
) -> Result<Option<RootId>, RootedError> {
    let typed = c.has_builtin_brand_rooted(object, Brand::TypedArray)?;
    let view = c.has_builtin_brand_rooted(object, Brand::DataView)?;
    let array = c.has_builtin_brand_rooted(object, Brand::AnyArrayBuffer)?;
    if !typed && !view && !array {
        return Ok(None);
    }
    let missing = c.undefined();
    let settings = args.get(1).copied().unwrap_or(missing);
    let depth = args.get(2).copied().unwrap_or(missing);
    let render = args.get(3).copied().unwrap_or(missing);
    let property = args.get(4).copied().unwrap_or(missing);
    let data = c.host_function_data()?;
    let next = c.number(
        c.rooted_value(depth)
            .and_then(|v| v.as_number())
            .unwrap_or_default()
            + 1.0,
    );
    let limit = option_limit(c, settings, "maxArrayLength", DEFAULT_MAX_ARRAY_LENGTH)?;
    if typed {
        let length = intrinsic(c, data, "typed:length", object)?;
        let length = c
            .rooted_value(length)
            .and_then(|v| v.as_number())
            .unwrap_or_default() as usize;
        let name = super::util_shared_vm::typed_array_name(c, object)?;
        let mut entries = Vec::new();
        for index in 0..length.min(limit) {
            let key = c.number(index as f64);
            let value = c.get_property_rooted(object, key)?;
            let text = c.call_rooted(render, missing, &[value, next])?;
            entries.push(c.to_string(text)?);
        }
        if length > limit {
            let remaining = length - limit;
            entries.push(format!(
                "... {remaining} more item{}",
                if remaining == 1 { "" } else { "s" }
            ));
        }
        entries.extend(properties(
            c,
            object,
            settings,
            depth,
            render,
            property,
            Some(length),
        )?);
        let text = if entries.is_empty() {
            format!("{name}({length}) []")
        } else {
            format!("{name}({length}) [ {} ]", entries.join(", "))
        };
        return Ok(Some(c.string_rooted(&text)));
    }
    if view {
        let length = intrinsic(c, data, "view:byteLength", object)?;
        let offset = intrinsic(c, data, "view:byteOffset", object)?;
        let backing = intrinsic(c, data, "view:buffer", object)?;
        let buffer = c.call_rooted(render, missing, &[backing, next])?;
        let mut entries = vec![
            format!("[byteLength]: {}", c.to_string(length)?),
            format!("[byteOffset]: {}", c.to_string(offset)?),
            format!("[buffer]: {}", c.to_string(buffer)?),
        ];
        entries.extend(properties(
            c, object, settings, depth, render, property, None,
        )?);
        return Ok(Some(c.string_rooted(&format!(
            "DataView {{\n  {}\n}}",
            entries.join(",\n  ")
        ))));
    }
    let shared = c.has_builtin_brand_rooted(object, Brand::SharedArrayBuffer)?;
    let name = if shared {
        "SharedArrayBuffer"
    } else {
        "ArrayBuffer"
    };
    let length = intrinsic(
        c,
        data,
        if shared {
            "shared:byteLength"
        } else {
            "array:byteLength"
        },
        object,
    )?;
    let count = c
        .rooted_value(length)
        .and_then(|v| v.as_number())
        .unwrap_or_default() as usize;
    let detached = if shared {
        false
    } else {
        let value = intrinsic(c, data, "array:detached", object)?;
        c.truthy_rooted(value)?
    };
    let mut entries = if detached {
        vec!["(detached)".to_owned()]
    } else {
        let bytes = c.bytes_prefix_rooted(object, limit).unwrap_or_default();
        let mut text = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        if count > limit {
            let remaining = count - limit;
            text.push_str(&format!(
                " ... {remaining} more byte{}",
                if remaining == 1 { "" } else { "s" }
            ));
        }
        vec![format!("[Uint8Contents]: <{text}>")]
    };
    entries.push(format!("[byteLength]: {count}"));
    entries.extend(properties(
        c, object, settings, depth, render, property, None,
    )?);
    let single = format!("{name} {{ {} }}", entries.join(", "));
    let width = option_limit(c, settings, "breakLength", DEFAULT_BREAK_LENGTH)?;
    let text = if single.chars().count() > width {
        format!("{name} {{\n  {}\n}}", entries.join(",\n  "))
    } else {
        single
    };
    Ok(Some(c.string_rooted(&text)))
}
