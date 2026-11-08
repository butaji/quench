//! The Fetch standard as Node exposes it (undici's `lib/web/fetch`): `Headers`, `Request`,
//! `Response`, `FormData`, their body mixin and `fetch()` itself, which issues requests
//! through the `http` and `https` clients.

pub(crate) mod body;
pub(crate) mod fetch;
pub(crate) mod form_data;
pub(crate) mod headers;
pub(crate) mod request;
pub(crate) mod response;

use crate::host::NodeHost;
use crate::modules::host_members::Members;
use crate::modules::webstreams_shared_vm::{self as web, get, put, record};
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) type Outcome = Result<RootId, RootedError>;

/// The `builtin_cache` key of the realm's fetch classes.
const CLASSES: &str = "\0fetch:classes";

/// The classes table's members.
pub(crate) mod table {
    pub(crate) const HEADERS: &str = "Headers";
    pub(crate) const REQUEST: &str = "Request";
    pub(crate) const RESPONSE: &str = "Response";
    pub(crate) const FORM_DATA: &str = "FormData";
    pub(crate) const HEADERS_ITERATOR: &str = "HeadersIterator";
    pub(crate) const FORM_DATA_ITERATOR: &str = "FormDataIterator";
}

/// The private slots the fetch classes keep their state in.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    Headers,
    HeadersIterator,
    FormData,
    FormDataIterator,
    Request,
    Response,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Self::Headers => "\0slot:Headers",
            Self::HeadersIterator => "\0slot:HeadersIterator",
            Self::FormData => "\0slot:FormData",
            Self::FormDataIterator => "\0slot:FormDataIterator",
            Self::Request => "\0slot:Request",
            Self::Response => "\0slot:Response",
        }
    }
}

pub(crate) mod private_slot {
    use super::{Context, Slot};
    use quench_runtime::{RootId, RootedError};

    pub(crate) fn read(c: &mut Context<'_>, object: RootId, slot: Slot) -> Result<Option<RootId>, RootedError> {
        crate::modules::private_slots::read(c, object, slot.name())
    }

    pub(crate) fn write(c: &mut Context<'_>, object: RootId, slot: Slot, value: RootId) -> Result<(), RootedError> {
        crate::modules::private_slots::write(c, object, slot.name(), value)
    }
}

/// A plain `TypeError`, as undici's Web IDL layer throws.
pub(crate) fn type_error(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// `TypeError [ERR_INVALID_THIS]: Illegal invocation`, undici's brand check failure.
pub(crate) fn illegal_invocation(c: &mut Context<'_>) -> RootedError {
    let built = c.type_error_rooted("Illegal invocation").and_then(|error| {
        crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
        Ok(error)
    });
    match built {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// `webidl.argumentLengthCheck(args, count, context)`.
pub(crate) fn required(c: &mut Context<'_>, args: &[RootId], count: usize, context: &str) -> Result<(), RootedError> {
    if args.len() >= count {
        return Ok(());
    }
    let plural = if count == 1 { "" } else { "s" };
    let found = if args.is_empty() { format!("{} found", args.len()) } else { format!("only {} found", args.len()) };
    Err(type_error(c, &format!("{context}: {count} argument{plural} required, but {found}.")))
}

/// Web IDL's `ByteString` conversion: every code unit must fit a byte.
pub(crate) fn usv_or_byte_string(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let units = c.to_string_code_units(value)?;
    if let Some((index, unit)) = units.iter().enumerate().find(|(_, unit)| **unit > u16::from(u8::MAX)) {
        return Err(type_error(
            c,
            &format!("Cannot convert argument to a ByteString because the character at index {index} has a value of {unit} which is greater than 255."),
        ));
    }
    Ok(units.iter().map(|&unit| char::from(unit as u8)).collect())
}

/// Web IDL's `USVString` conversion.
pub(crate) fn usv_string(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let units = c.to_string_code_units(value)?;
    Ok(String::from_utf16_lossy(&units))
}

/// A class named `name` constructed by `member`, with `Symbol.toStringTag` and custom
/// inspection.
pub(crate) fn class<M: Members>(c: &mut Context<'_>, member: M, name: &str, length: u32, inspect: M) -> Outcome {
    let constructor = crate::modules::host_members::constructor(c, member, name)?;
    web::set_length(c, constructor, length)?;
    let prototype = get(c, constructor, "prototype")?;
    c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
    tag(c, prototype, name)?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let registry = get(c, symbol, "for")?;
    let description = c.string_rooted("nodejs.util.inspect.custom");
    let key = c.call_rooted(registry, symbol, &[description])?;
    let function = web::method_function(c, inspect, "[nodejs.util.inspect.custom]", 2)?;
    c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    Ok(constructor)
}

fn tag(c: &mut Context<'_>, target: RootId, name: &str) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = get(c, symbol, "toStringTag")?;
    let value = c.string_rooted(name);
    c.define_data_property_rooted(target, key, value, false, false, true)?;
    Ok(())
}

/// An iterator prototype tagged `tag` over `%IteratorPrototype%`, whose `next` is `next`.
pub(crate) fn iterator_class<M: Members>(c: &mut Context<'_>, next: M, tag_name: &str) -> Outcome {
    let array = c.array_rooted(&[])?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator_key = get(c, symbol, "iterator")?;
    let values = c.get_property_rooted(array, iterator_key)?;
    let array_iterator = c.call_rooted(values, array, &[])?;
    let array_iterator_prototype = c.prototype_rooted(array_iterator)?;
    let iterator_prototype = c.prototype_rooted(array_iterator_prototype)?;
    let prototype = c.object_rooted()?;
    c.set_prototype_rooted(prototype, iterator_prototype)?;
    let method = web::method_function(c, next, "next", 0)?;
    let key = c.string_rooted("next");
    c.define_data_property_rooted(prototype, key, method, true, true, true)?;
    tag(c, prototype, tag_name)?;
    Ok(prototype)
}

/// The fetch classes, by name, built once.
pub(crate) fn classes(c: &mut Context<'_>) -> Outcome {
    if let Some(table) = c.host_mut().shared_state().borrow().builtin_cache.get(CLASSES).copied() {
        return Ok(table);
    }
    let table = c.with_startup_intrinsics(|c| {
        let table = record(c, &[], &[])?;
        let retained = c.retain(table)?;
        c.host_mut().shared_state().borrow_mut().builtin_cache.insert(CLASSES.to_owned(), retained);
        let built = (|| {
            let headers = headers::build(c, table)?;
            put(c, table, table::HEADERS, headers)?;
            let form_data = form_data::build(c, table)?;
            put(c, table, table::FORM_DATA, form_data)?;
            let request = request::build(c)?;
            put(c, table, table::REQUEST, request)?;
            let response = response::build(c)?;
            put(c, table, table::RESPONSE, response)
        })();
        if built.is_err() {
            c.host_mut().shared_state().borrow_mut().builtin_cache.remove(CLASSES);
        }
        built.map(|()| table)
    })?;
    Ok(table)
}

/// The class `name` of the table.
pub(crate) fn class_of(c: &mut Context<'_>, name: &str) -> Outcome {
    let table = classes(c)?;
    get(c, table, name)
}

/// A new object of the class `name`, without running its constructor.
pub(crate) fn bare_instance(c: &mut Context<'_>, name: &str) -> Outcome {
    let class = class_of(c, name)?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    Ok(object)
}

/// The globals `initialize_globals` defines.
pub(crate) const GLOBALS: [&str; 4] = [table::FORM_DATA, table::HEADERS, table::REQUEST, table::RESPONSE];

/// The fetch globals' lazy initializer.
pub(crate) fn initialize_globals(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let table = classes(c)?;
    let global = c.global_root()?;
    for name in GLOBALS {
        let class = get(c, table, name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(global, key, class, true, false, true)?;
    }
    crate::modules::message_event_shared_vm::define_globals(c, global)?;
    Ok(c.undefined())
}

/// A plain object of the `fields` a caller lists at run time (inspection views), in order.
pub(crate) fn literal_fields(c: &mut Context<'_>, fields: &[(&str, RootId)]) -> Outcome {
    let object = c.object_rooted()?;
    for &(name, value) in fields {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(object, key, value, true, true, true)?;
    }
    Ok(object)
}
