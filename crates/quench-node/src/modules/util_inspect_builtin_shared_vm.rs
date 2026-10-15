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
    let hidden = get(c, settings, "showHidden")?;
    let hidden = c.truthy_rooted(hidden)?;
    let next = c.number(
        c.rooted_value(depth)
            .and_then(|v| v.as_number())
            .unwrap_or_default()
            + 1.0,
    );
    let mut entries = Vec::new();
    for key in c.own_keys_rooted(object)? {
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
    Ok(c.string_rooted(&if entries.is_empty() {
        base
    } else {
        format!("{base} {{ {} }}", entries.join(", "))
    }))
}
