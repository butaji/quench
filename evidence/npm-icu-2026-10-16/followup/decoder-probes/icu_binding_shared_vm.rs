//! Native ICU handles consumed by Node's canonical encoding policy.
use super::text_decoder_icu::Converter;
use crate::host::NodeHost;
use quench_runtime::{BuiltinData, HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
type Context<'a> = NativeContext<'a, NodeHost>;
const FLUSH: u32 = 1;
const FATAL: u32 = 2;
const IGNORE_BOM: u32 = 4;
#[derive(Default)]
pub(crate) struct State {
    entries: Vec<Entry>,
}
struct Entry {
    identity: HostWeakRoot,
    converter: Converter,
}
fn put(c: &mut Context<'_>, o: RootId, name: &str, v: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(o, key, v, true, true, true)?;
    Ok(())
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    const CACHE: &str = "\0binding:icu";
    let shared = c.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(CACHE).copied() {
        return Ok(value);
    }
    let result = c.object_rooted()?;
    for (name, op) in [
        ("getConverter", "icuGetConverter"),
        ("decode", "icuDecode"),
        ("getStringWidth", "icuStringWidth"),
        ("transcode", "icuTranscode"),
        ("icuErrName", "icuErrorName"),
    ] {
        let f = c.host_function(crate::host::shared_vm::operation(op))?;
        let key = c.string_rooted("name");
        let value = c.string_rooted(name);
        c.define_data_property_rooted(f, key, value, false, false, true)?;
        let key = c.string_rooted("length");
        let zero = c.number(0.0);
        c.define_data_property_rooted(f, key, zero, false, false, true)?;
        put(c, result, name, f)?;
    }
    let retained = c.retain(result)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE.into(), retained);
    Ok(result)
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .icu_converters
        .entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| (!runtime.host_weak_root_is_live(e.identity)).then_some(i))
        .collect::<Vec<_>>();
    for i in dead.into_iter().rev() {
        let entry = shared.borrow_mut().icu_converters.entries.remove(i);
        runtime.release_host_weak_root(entry.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().icu_converters.entries);
    for entry in entries {
        runtime.release_host_weak_root(entry.identity);
    }
}
pub(crate) fn get_converter(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = c.to_string(args[0])?;
    let flags = c.to_number(args[1])? as u32;
    let Ok(converter) = Converter::new(&name, flags & FATAL != 0, flags & IGNORE_BOM != 0) else {
        return Ok(c.undefined());
    };
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .icu_converters
        .entries
        .iter()
        .map(|e| e.identity)
        .collect::<Vec<_>>();
    let dead = identities
        .iter()
        .enumerate()
        .filter_map(|(i, id)| c.resolve_host_weak_root(*id).is_none().then_some(i))
        .collect::<Vec<_>>();
    for i in dead.into_iter().rev() {
        let e = shared.borrow_mut().icu_converters.entries.remove(i);
        c.release_host_weak_root(e.identity);
    }
    let object = c.object_rooted()?;
    let identity = c.host_weak_root(object)?;
    shared.borrow_mut().icu_converters.entries.push(Entry {
        identity,
        converter,
    });
    Ok(object)
}
pub(crate) fn decode(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = args[0];
    let Some(BuiltinData::Bytes { bytes, .. }) = c.builtin_data_rooted(args[1])? else {
        let e = c.type_error_rooted("The \"list\" argument must be an instance of SharedArrayBuffer, ArrayBuffer or ArrayBufferView.")?;
        super::node_errors_shared_vm::decorate_type_error(c, e, "ERR_INVALID_ARG_TYPE")?;
        return Err(c.throw(e));
    };
    let flags = c.to_number(args[2])? as u32;
    let shared = c.host_mut().shared_state();
    let identities = shared
        .borrow()
        .icu_converters
        .entries
        .iter()
        .map(|e| e.identity)
        .collect::<Vec<_>>();
    let mut owner = None;
    for (i, id) in identities.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(id) {
            if c.strict_equal_rooted(value, handle)? {
                owner = Some(i);
                break;
            }
        }
    }
    let i = owner.ok_or_else(|| RootedError::host("ICU converter handle is not live"))?;
    let result = shared.borrow_mut().icu_converters.entries[i]
        .converter
        .decode(&bytes, flags & FLUSH != 0);
    match result {
        Ok(units) => Ok(c.string_units_rooted(&units)),
        Err(()) => {
            let encoding = c.to_string(args[3])?;
            let e = c.type_error_rooted(&format!(
                "The encoded data was not valid for encoding {encoding}"
            ))?;
            super::node_errors_shared_vm::decorate_type_error(
                c,
                e,
                "ERR_ENCODING_INVALID_ENCODED_DATA",
            )?;
            Err(c.throw(e))
        }
    }
}
pub(crate) fn string_width(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let units = c
        .string_code_units(args[0])?
        .ok_or_else(|| RootedError::host("ICU width requires a string"))?;
    let boolean = |c: &Context<'_>, i| {
        args.get(i)
            .and_then(|r| c.rooted_value(*r))
            .and_then(|v| v.as_bool())
    };
    let width = super::text_decoder_icu::string_width(
        &units,
        boolean(c, 1) == Some(true),
        boolean(c, 2) != Some(false),
    );
    Ok(c.number(f64::from(width)))
}
pub(crate) fn transcode(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(BuiltinData::Bytes { bytes, .. }) = c.builtin_data_rooted(args[0])? else {
        return Err(RootedError::host("ICU transcode requires a byte view"));
    };
    let from = c.to_string(args[1])?;
    let to = c.to_string(args[2])?;
    match super::text_decoder_icu::transcode(&bytes, &from, &to) {
        Ok(bytes) => super::buffer_shared_vm::from_bytes(c, &bytes),
        Err(status) => Ok(c.number(f64::from(status))),
    }
}
pub(crate) fn error_name(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let status = c.to_number(args[0])? as i32;
    Ok(c.string_rooted(&super::text_decoder_icu::status_name(status)))
}
