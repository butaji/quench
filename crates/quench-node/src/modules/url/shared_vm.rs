//! Shared-VM URL constructors and Node's legacy URL parser.

use crate::host::NodeHost;
use crate::modules::url::{LegacyUrlField, LegacyUrlParseError};
use quench_runtime_next::{NativeContext, RootId, RootedError};

const URL_CONSTRUCTOR_SOURCE: &str = "(class Url {})";
const WHATWG_URL_FACTORY: &str = quench_js_check::checked_js!(
    r#"(parse) => {
  const state = new WeakMap();
  const fields = [
    "href", "origin", "protocol", "username", "password", "host",
    "hostname", "port", "pathname", "search", "hash",
  ];
  const data = (receiver) => {
    const value = state.get(receiver);
    if (value === undefined) throw new TypeError("Illegal invocation");
    return value;
  };
  class URL {
    constructor(input, base) {
      state.set(this, parse(String(input), base === undefined ? undefined : String(base)));
    }
    toString() { return data(this).href; }
    toJSON() { return data(this).href; }
  }
  for (const field of fields) {
    Object.defineProperty(URL.prototype, field, {
      configurable: true,
      enumerable: true,
      get() { return data(this)[field]; },
    });
  }
  return URL;
}"#
);

const WHATWG_PARSE_MARKER: &str = "\0quench:node:url:whatwg-parse";

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let constructor = url_constructor(context)?;
    let legacy_constructor =
        context.evaluate_script_rooted(URL_CONSTRUCTOR_SOURCE, "node:url/Url")?;
    let module = context.object_rooted()?;
    let parse = context.host_function_with_data(
        crate::host::shared_vm::operation("urlParse"),
        legacy_constructor,
    )?;
    set(context, module, "parse", parse)?;
    set(context, module, "URL", constructor)?;
    set(context, module, "Url", legacy_constructor)?;
    Ok(module)
}

/// Install the realm's one URL constructor as the global; `node:url` reads the
/// same rooted constructor from host state.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let constructor = url_constructor(context)?;
    let install = context.evaluate_script_rooted(
        "(value) => Object.defineProperty(globalThis, 'URL', { value, writable: true, configurable: true })",
        "node:url/install-global.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(install, undefined, &[constructor])?;
    Ok(())
}

fn url_constructor(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let roots = context.host_mut().shared_state();
    if let Some(constructor) = roots.borrow().url_constructor {
        return Ok(constructor);
    }

    let parser_data = context.string_rooted(WHATWG_PARSE_MARKER);
    let parser = context
        .host_function_with_data(crate::host::shared_vm::operation("urlParse"), parser_data)?;
    let factory = context.evaluate_script_rooted(WHATWG_URL_FACTORY, "node:url/whatwg.js")?;
    let undefined = context.undefined();
    let constructor = context.call_rooted(factory, undefined, &[parser])?;
    let retained = context.retain(constructor)?;
    roots.borrow_mut().url_constructor = Some(retained);
    Ok(constructor)
}

pub(crate) fn parse(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    if context.string_text(data)?.as_deref() == Some(WHATWG_PARSE_MARKER) {
        return parse_whatwg(context, args);
    }

    let Some(input) = args.first().copied() else {
        return Err(type_error(
            context,
            "The \"url\" argument must be of type string",
        ));
    };
    let Some(input) = context.string_text(input)? else {
        return Err(type_error(
            context,
            "The \"url\" argument must be of type string",
        ));
    };
    let parsed = crate::modules::url::parse_legacy_parts(&input).map_err(|error| match error {
        LegacyUrlParseError::Invalid { code, input } => coded_type_error(context, &code, &input),
        LegacyUrlParseError::MalformedUri => uri_error(context),
    })?;
    let query_requested = args
        .get(1)
        .copied()
        .map(|argument| context.truthy_rooted(argument))
        .transpose()?
        .unwrap_or(false);
    let query_entries = if query_requested && parsed.query_object {
        Some(query_object(
            context,
            parsed.query.as_deref().unwrap_or_default(),
        )?)
    } else {
        None
    };
    let constructor = context.host_function_data()?;
    let instance = context.construct_rooted(constructor, constructor, &[])?;
    for (name, value) in parsed.fields {
        let value = if name == "query" {
            query_entries.unwrap_or_else(|| legacy_value(context, value))
        } else {
            legacy_value(context, value)
        };
        set(context, instance, name, value)?;
    }
    Ok(instance)
}

fn parse_whatwg(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args
        .first()
        .copied()
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let base = args
        .get(1)
        .copied()
        .filter(|root| {
            context
                .rooted_value(*root)
                .is_some_and(|value| !value.is_undefined())
        })
        .map(|root| context.to_string(root))
        .transpose()?;

    let parsed = match base.as_deref() {
        Some(base) => url::Url::parse(base).and_then(|base| base.join(&input)),
        None => url::Url::parse(&input),
    }
    .map_err(|_| invalid_url(context, &input))?;
    let parsed = crate::modules::url_whatwg::Parsed::Url(parsed);
    let fields = [
        "href", "origin", "protocol", "username", "password", "host", "hostname", "port",
        "pathname", "search", "hash",
    ];
    let object = context.object_rooted()?;
    for name in fields {
        let value = parsed.get(name);
        set_text(context, object, name, &value)?;
    }
    Ok(object)
}

fn query_object(
    context: &mut NativeContext<'_, NodeHost>,
    query: &str,
) -> Result<RootId, RootedError> {
    let object = context.null_object_rooted()?;
    for (key, values) in crate::modules::querystring_parse::parse_default_entries(query) {
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

fn legacy_value(context: &mut NativeContext<'_, NodeHost>, value: LegacyUrlField) -> RootId {
    match value {
        LegacyUrlField::Null => context.null(),
        LegacyUrlField::Boolean(value) => context.boolean(value),
        LegacyUrlField::Text(value) => context.string_rooted(&value),
    }
}

fn type_error(context: &mut NativeContext<'_, NodeHost>, message: &str) -> RootedError {
    match context.type_error_rooted(message) {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

fn coded_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    code: &str,
    input: &str,
) -> RootedError {
    let exception = match context.type_error_rooted("Invalid URL") {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    let code = context.string_rooted(code);
    let input = context.string_rooted(input);
    if let Err(error) = set(context, exception, "code", code) {
        return error;
    }
    if let Err(error) = set(context, exception, "input", input) {
        return error;
    }
    context.throw(exception)
}

fn invalid_url(context: &mut NativeContext<'_, NodeHost>, input: &str) -> RootedError {
    coded_type_error(context, "ERR_INVALID_URL", input)
}

fn uri_error(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    let exception = match context.error_rooted("URI malformed") {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    let name = context.string_rooted("URIError");
    let code = context.string_rooted("ERR_INVALID_URI");
    if let Err(error) = set(context, exception, "name", name) {
        return error;
    }
    if let Err(error) = set(context, exception, "code", code) {
        return error;
    }
    context.throw(exception)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
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
