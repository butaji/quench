//! Native owned exception packets. This is not a V8 serializer or public API.
//! Error reflection follows lib/internal/error_serdes.js. Data graph admission
//! excludes observable accessor traversal and unsupported exotic native kinds.
use super::worker_owned_clone::Message;
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
// Protocol recursion is bounded independently of the guest call stack.
const MAX_PACKET_DEPTH: usize = 64;
pub(crate) enum Packet {
    Value(Message),
    Sdk(Message),
    Error {
        constructor: String,
        properties: Message,
        cause: Option<Box<Packet>>,
    },
    Unsupported(&'static str),
}
enum Rejected {
    Guest(RootedError),
    Kind(&'static str),
}
impl From<RootedError> for Rejected {
    fn from(error: RootedError) -> Self {
        Self::Guest(error)
    }
}
type Result<T> = std::result::Result<T, Rejected>;
fn get(c: &mut Context<'_>, o: RootId, name: &str) -> std::result::Result<RootId, RootedError> {
    let k = c.string_rooted(name);
    c.get_property_rooted(o, k)
}
fn undefined(c: &Context<'_>, v: RootId) -> bool {
    c.rooted_value(v).is_some_and(|v| v.is_undefined())
}
fn data(
    c: &mut Context<'_>,
    o: RootId,
    name: &str,
) -> std::result::Result<Option<RootId>, RootedError> {
    let k = c.string_rooted(name);
    let d = c.own_property_descriptor_rooted(o, k)?;
    if undefined(c, d) {
        return Ok(None);
    }
    let value = c.string_rooted("value");
    let field = c.own_property_descriptor_rooted(d, value)?;
    if undefined(c, field) {
        Ok(None)
    } else {
        get(c, field, "value").map(Some)
    }
}
fn put(
    c: &mut Context<'_>,
    o: RootId,
    name: &str,
    v: RootId,
) -> std::result::Result<(), RootedError> {
    let k = c.string_rooted(name);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
fn primordials(c: &mut Context<'_>) -> std::result::Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_primordials(c)?
        .ok_or_else(|| RootedError::host("Worker error packets require Node primordials"))
}
fn release(c: &mut Context<'_>, e: RootedError) {
    if let Some(r) = e.exception {
        c.release_root(r);
    }
}
fn sdk(c: &mut Context<'_>, v: RootId, seen: Option<&[RootId]>) -> Result<Packet> {
    let f = crate::host::shared_vm::commonjs::node_library_export(
        c,
        "internal/worker.js",
        "internal/error_serdes.js",
        "serializeError",
    )?;
    let mut args = vec![v];
    if let Some(seen) = seen {
        let p = primordials(c)?;
        let constructor = get(c, p, "SafeSet")?;
        let set = c.construct_rooted(constructor, constructor, &[])?;
        let add = get(c, set, "add")?;
        for old in seen {
            c.check_execution_interrupt()?;
            c.call_rooted(add, set, &[*old])?;
        }
        args.push(set);
    }
    let u = c.undefined();
    let bytes = c.call_rooted(f, u, &args)?;
    Ok(Packet::Sdk(c.capture_owned_clone_rooted(bytes, &[])?))
}
fn validate_data(
    c: &mut Context<'_>,
    v: RootId,
    seen: &mut Vec<RootId>,
    depth: usize,
) -> Result<()> {
    c.check_execution_interrupt()?;
    if depth > MAX_PACKET_DEPTH {
        return Err(Rejected::Kind("data graph recursion limit"));
    }
    if !c.is_object_rooted(v)? {
        if c.is_symbol_rooted(v)? {
            return Err(Rejected::Kind("symbol in data graph"));
        }
        return Ok(());
    }
    if c.is_callable_rooted(v)? {
        return Err(Rejected::Kind("callable in data graph"));
    }
    if c.has_builtin_brand_rooted(v, BuiltinBrand::Proxy)? {
        return Err(Rejected::Kind("Proxy in data graph"));
    }
    for old in seen.iter() {
        if c.strict_equal_rooted(v, *old)? {
            return Ok(());
        }
    }
    seen.push(v);
    if c.builtin_data_rooted(v)?.is_some() {
        return Err(Rejected::Kind("exotic builtin in data graph"));
    }
    for key in c.enumerable_own_keys_rooted(v, false, true)? {
        c.check_execution_interrupt()?;
        let d = c.own_property_descriptor_rooted(v, key)?;
        if undefined(c, d) {
            continue;
        }
        let value = data(c, d, "value")?.ok_or(Rejected::Kind("accessor in data graph"))?;
        validate_data(c, value, seen, depth + 1)?;
    }
    Ok(())
}
fn packet(c: &mut Context<'_>, v: RootId, seen: &mut Vec<RootId>, depth: usize) -> Result<Packet> {
    c.check_execution_interrupt()?;
    if depth > MAX_PACKET_DEPTH {
        return Err(Rejected::Kind("error cause recursion limit"));
    }
    if c.is_symbol_rooted(v)? {
        return sdk(c, v, None);
    }
    if !c.is_object_rooted(v)? {
        return Ok(Packet::Value(c.capture_owned_clone_rooted(v, &[])?));
    }
    if c.is_callable_rooted(v)? {
        return sdk(c, v, None);
    }
    if c.has_builtin_brand_rooted(v, BuiltinBrand::Proxy)? {
        return Err(Rejected::Kind("Proxy exception"));
    }
    let p = primordials(c)?;
    let to_string = get(c, p, "ObjectPrototypeToString")?;
    let u = c.undefined();
    let tag = c.call_rooted(to_string, u, &[v])?;
    if c.string_text(tag)?.as_deref() != Some("[object Error]") {
        validate_data(c, v, &mut Vec::new(), 0)?;
        return Ok(Packet::Value(c.capture_owned_clone_rooted(v, &[])?));
    }
    for old in seen.iter() {
        if c.strict_equal_rooted(v, *old)? {
            return sdk(c, v, Some(seen));
        }
    }
    seen.push(v);
    let mut chain = Vec::new();
    let mut current = v;
    while !c.rooted_value(current).is_some_and(|v| v.is_null()) {
        c.check_execution_interrupt()?;
        if c.has_builtin_brand_rooted(current, BuiltinBrand::Proxy)? {
            return Err(Rejected::Kind("Proxy in Error prototype chain"));
        }
        chain.push(current);
        current = c.prototype_rooted(current)?;
    }
    let mut constructor = None;
    for object in &chain {
        let Some(ctor) = data(c, *object, "constructor")? else {
            continue;
        };
        if !c.truthy_rooted(ctor)? {
            continue;
        }
        let Some(name) = data(c, ctor, "name")? else {
            continue;
        };
        let Some(name) = c.string_text(name)? else {
            continue;
        };
        if matches!(
            name.as_str(),
            "AggregateError"
                | "Error"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
        ) {
            constructor = Some(name);
            break;
        }
    }
    let constructor = constructor.ok_or(Rejected::Kind("unrecognized Error constructor"))?;
    let properties = c.null_object_rooted()?;
    let mut cause = None;
    for object in chain.into_iter().rev() {
        for key in c.own_keys_rooted(object)? {
            c.check_execution_interrupt()?;
            if c.is_symbol_rooted(key)? {
                continue;
            }
            let name = c
                .string_text(key)?
                .ok_or(Rejected::Kind("non-string Error property"))?;
            let descriptor = match c.own_property_descriptor_rooted(object, key) {
                Ok(d) => d,
                Err(error) => {
                    if error.error.is_execution_terminated() {
                        return Err(error.into());
                    }
                    release(c, error);
                    continue;
                }
            };
            if undefined(c, descriptor) {
                continue;
            }
            if let Some(getter) = data(c, descriptor, "get")? {
                if c.truthy_rooted(getter)? && name != "__proto__" {
                    let value = match c.call_rooted(getter, v, &[]) {
                        Ok(value) => value,
                        Err(error) => {
                            if error.error.is_execution_terminated() {
                                return Err(error.into());
                            }
                            release(c, error);
                            continue;
                        }
                    };
                    put(c, descriptor, "value", value)?;
                    for field in ["get", "set"] {
                        let field = c.string_rooted(field);
                        let p = primordials(c)?;
                        let delete = get(c, p, "ReflectDeleteProperty")?;
                        let u = c.undefined();
                        c.call_rooted(delete, u, &[descriptor, field])?;
                    }
                }
            }
            if name == "cause" {
                let value = data(c, descriptor, "value")?.unwrap_or_else(|| c.undefined());
                cause = Some(Box::new(packet(c, value, seen, depth + 1)?));
                let u = c.undefined();
                put(c, descriptor, "value", u)?;
            } else {
                let Some(value) = data(c, descriptor, "value")? else {
                    continue;
                };
                // Exact SDK policy: direct callable and symbol data values are excluded.
                if c.is_callable_rooted(value)? || c.is_symbol_rooted(value)? {
                    continue;
                }
            }
            c.define_data_property_rooted(properties, key, descriptor, true, true, true)?;
        }
    }
    validate_data(c, properties, &mut Vec::new(), 0)?;
    Ok(Packet::Error {
        constructor,
        properties: c.capture_owned_clone_rooted(properties, &[])?,
        cause,
    })
}
pub(crate) fn capture(
    c: &mut Context<'_>,
    value: RootId,
) -> std::result::Result<Packet, RootedError> {
    match packet(c, value, &mut Vec::new(), 0) {
        Ok(packet) => Ok(packet),
        Err(Rejected::Kind(kind)) => Ok(Packet::Unsupported(kind)),
        Err(Rejected::Guest(error)) => {
            if error.error.is_execution_terminated() {
                return Err(error);
            }
            release(c, error);
            Ok(Packet::Unsupported(
                "exception while preparing error packet",
            ))
        }
    }
}
pub(crate) fn import(
    c: &mut Context<'_>,
    packet: &Packet,
) -> std::result::Result<RootId, RootedError> {
    match packet {
        Packet::Value(graph) => c.import_owned_clone_rooted(graph),
        Packet::Sdk(graph) => {
            let bytes = c.import_owned_clone_rooted(graph)?;
            let decoder = crate::host::shared_vm::commonjs::node_library_export(
                c,
                "internal/worker.js",
                "internal/error_serdes.js",
                "deserializeError",
            )?;
            let u = c.undefined();
            c.call_rooted(decoder, u, &[bytes])
        }
        Packet::Unsupported(kind) => {
            let codes = crate::host::shared_vm::commonjs::node_library_export(
                c,
                "internal/worker.js",
                "internal/errors.js",
                "codes",
            )?;
            let constructor = get(c, codes, "ERR_WORKER_UNSERIALIZABLE_ERROR")?;
            let error = c.construct_rooted(constructor, constructor, &[])?;
            let kind = c.string_rooted(kind);
            put(c, error, "unsupportedKind", kind)?;
            Ok(error)
        }
        Packet::Error {
            constructor,
            properties,
            cause,
        } => {
            let properties = c.import_owned_clone_rooted(properties)?;
            if let Some(cause) = cause {
                let key = c.string_rooted("cause");
                let descriptor = c.get_property_rooted(properties, key)?;
                let cause = import(c, cause)?;
                put(c, descriptor, "value", cause)?;
            }
            let prototype = c.with_startup_intrinsics(|c| {
                let global = c.global_root()?;
                let constructor = get(c, global, constructor)?;
                get(c, constructor, "prototype")
            })?;
            let error = c.object_rooted()?;
            c.set_prototype_rooted(error, prototype)?;
            for key in c.own_keys_rooted(properties)? {
                let descriptor = c.get_property_rooted(properties, key)?;
                c.define_property_descriptor_rooted(error, key, descriptor)?;
            }
            let p = primordials(c)?;
            let tag = get(c, p, "SymbolToStringTag")?;
            let value = c.string_rooted("Error");
            c.define_data_property_rooted(error, tag, value, false, false, true)?;
            Ok(error)
        }
    }
}
