//! The message-carrying events of undici's `lib/web/websocket/events.js`: `MessageEvent`,
//! `CloseEvent` and `ErrorEvent`. Each is an `Event` with read-only fields from its init
//! dictionary; one table describes every interface's fields and their Web IDL types.

use crate::host::NodeHost;
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::webstreams_shared_vm::{self as web, arg, get, is_nullish, is_undefined, put, record};
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Member, "messageEvent", [ConstructMessage, ConstructClose, ConstructError, Field, InitMessageEvent]);

/// The `builtin_cache` key of the classes table, and the events' private slot.
const CLASSES: &str = "\0webEvents:classes";
const SLOT: &str = "\0slot:webEvent";

/// The Web IDL type of an init member, which says its default and conversion.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `any`, defaulting to `null`.
    Any,
    /// `any`, defaulting to `undefined`.
    Undefined,
    /// `DOMString`/`USVString`, defaulting to the empty string.
    String,
    Boolean,
    /// `unsigned short`, defaulting to zero.
    UnsignedShort,
    /// `unsigned long`, defaulting to zero.
    UnsignedLong,
    /// `MessagePort?`, defaulting to `null`.
    Source,
    /// `sequence<MessagePort>`, frozen, defaulting to empty.
    Ports,
}

/// An event interface: its name, constructor and fields in prototype order.
struct Interface {
    name: &'static str,
    construct: Member,
    fields: &'static [(&'static str, Kind)],
}

/// The event interfaces, by their index in `INTERFACES`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    Message,
    Close,
    Error,
}

const INTERFACES: [Interface; 3] = [
    Interface {
        name: "MessageEvent",
        construct: Member::ConstructMessage,
        fields: &[("data", Kind::Any), ("origin", Kind::String), ("lastEventId", Kind::String), ("source", Kind::Source), ("ports", Kind::Ports)],
    },
    Interface {
        name: "CloseEvent",
        construct: Member::ConstructClose,
        fields: &[("wasClean", Kind::Boolean), ("code", Kind::UnsignedShort), ("reason", Kind::String)],
    },
    Interface {
        name: "ErrorEvent",
        construct: Member::ConstructError,
        fields: &[("message", Kind::String), ("filename", Kind::String), ("lineno", Kind::UnsignedLong), ("colno", Kind::UnsignedLong), ("error", Kind::Undefined)],
    },
];
const EVENTS: [Event; 3] = [Event::Message, Event::Close, Event::Error];

/// Web IDL's integer moduli.
pub(crate) const UNSIGNED_SHORT_MODULUS: f64 = 65536.0;
const UNSIGNED_LONG_MODULUS: f64 = 4294967296.0;
/// `initMessageEvent(type, bubbles, cancelable, data, origin, lastEventId, source, ports)`:
/// where its init members start.
const INIT_MESSAGE_FIELDS_START: usize = 3;
const INIT_MESSAGE_LENGTH: u32 = 1;

impl Event {
    fn interface(self) -> &'static Interface {
        &INTERFACES[self as usize]
    }
}

/// The classes table by interface name, built once over `Event`.
fn classes(c: &mut Context<'_>) -> Outcome {
    if let Some(table) = c.host_mut().shared_state().borrow().builtin_cache.get(CLASSES).copied() {
        return Ok(table);
    }
    let table = c.with_startup_intrinsics(build)?;
    let retained = c.retain(table)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(CLASSES.to_owned(), retained);
    Ok(table)
}

fn build(c: &mut Context<'_>) -> Outcome {
    let family = crate::modules::events_shared_vm::event_family(c)?;
    let event = get(c, family, "eventCtor")?;
    let event_prototype = get(c, event, "prototype")?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let tag = get(c, symbol, "toStringTag")?;
    let table = record(c, &[], &[])?;
    for kind in EVENTS {
        let interface = kind.interface();
        let constructor = crate::modules::host_members::constructor(c, interface.construct, interface.name)?;
        web::set_length(c, constructor, 1)?;
        let prototype = get(c, constructor, "prototype")?;
        c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
        let interface_index = c.number(kind as usize as f64);
        for (field_index, &(name, _)) in interface.fields.iter().enumerate() {
            let field_index = c.number(field_index as f64);
            let getter = function(c, Member::Field, &format!("get {name}"), &[interface_index, field_index])?;
            web::set_length(c, getter, 0)?;
            let undefined = c.undefined();
            let key = c.string_rooted(name);
            let fields = DescriptorFields { get: Some(getter), set: Some(undefined), enumerable: Some(true), configurable: Some(true), ..Default::default() };
            c.define_property_fields_rooted(prototype, key, fields)?;
        }
        if kind == Event::Message {
            let method = web::method_function(c, Member::InitMessageEvent, "initMessageEvent", INIT_MESSAGE_LENGTH)?;
            let key = c.string_rooted("initMessageEvent");
            c.define_data_property_rooted(prototype, key, method, true, true, true)?;
        }
        c.set_prototype_rooted(prototype, event_prototype)?;
        c.set_prototype_rooted(constructor, event)?;
        let name = c.string_rooted(interface.name);
        c.define_data_property_rooted(prototype, tag, name, false, false, true)?;
        put(c, table, interface.name, constructor)?;
    }
    Ok(table)
}

/// The class of `event`.
pub(crate) fn class(c: &mut Context<'_>, event: Event) -> Outcome {
    let table = classes(c)?;
    get(c, table, event.interface().name)
}

/// Defines the `MessageEvent` global.
pub(crate) fn define_globals(c: &mut Context<'_>, global: RootId) -> Result<(), RootedError> {
    let class = class(c, Event::Message)?;
    let key = c.string_rooted(Event::Message.interface().name);
    c.define_data_property_rooted(global, key, class, true, false, true)?;
    Ok(())
}

/// A new `event` of `type` from the module, with its init dictionary.
pub(crate) fn create(c: &mut Context<'_>, event: Event, kind: &str, init: RootId) -> Outcome {
    let class = class(c, event)?;
    let kind = c.string_rooted(kind);
    c.construct_rooted(class, class, &[kind, init])
}

fn type_error(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// An init member converted to its Web IDL type.
fn convert(c: &mut Context<'_>, interface: &Interface, name: &str, kind: Kind, value: RootId) -> Outcome {
    let undefined = is_undefined(c, value);
    Ok(match kind {
        Kind::Any => if undefined { c.null() } else { value },
        Kind::Undefined => value,
        Kind::String => {
            let text = if undefined { String::new() } else { String::from_utf16_lossy(&c.to_string_code_units(value)?) };
            c.string_rooted(&text)
        }
        Kind::Boolean => {
            let truthy = c.truthy_rooted(value)?;
            c.boolean(truthy)
        }
        Kind::UnsignedShort | Kind::UnsignedLong => {
            let modulus = if kind == Kind::UnsignedShort { UNSIGNED_SHORT_MODULUS } else { UNSIGNED_LONG_MODULUS };
            let number = if undefined { 0.0 } else { c.to_number(value)? };
            let number = if number.is_finite() { number.trunc().rem_euclid(modulus) } else { 0.0 };
            c.number(number)
        }
        Kind::Source => {
            if is_nullish(c, value) {
                c.null()
            } else if c.is_object_rooted(value)? {
                value
            } else {
                let shown = c.to_string(value)?;
                return Err(type_error(c, &format!("{} constructor: Expected eventInitDict.{name} (\"{shown}\") to be an instance of MessagePort.", interface.name)));
            }
        }
        Kind::Ports => {
            let mut ports = Vec::new();
            if !undefined {
                let iterable = c.is_object_rooted(value)? && {
                    let global = c.global_root()?;
                    let symbol = get(c, global, "Symbol")?;
                    let key = get(c, symbol, "iterator")?;
                    let method = c.get_property_rooted(value, key)?;
                    c.is_callable_rooted(method)?
                };
                if !iterable {
                    let shown = crate::modules::console_format_shared_vm::inspect(c, value, Default::default())?;
                    return Err(type_error(c, &format!("{} constructor: eventInitDict.{name} ({shown}) is not iterable.", interface.name)));
                }
                let iterator = c.iterator_rooted(value)?;
                while let Some(port) = c.iterator_step_rooted(&iterator)? {
                    ports.push(port);
                }
            }
            let ports = c.array_rooted(&ports)?;
            let global = c.global_root()?;
            let object = get(c, global, "Object")?;
            let freeze = get(c, object, "freeze")?;
            c.call_rooted(freeze, object, &[ports])?;
            ports
        }
    })
}

/// The interface's fields from `init`, as the event's slots.
fn fields(c: &mut Context<'_>, interface: &Interface, init: RootId) -> Outcome {
    let state = record(c, &[], &[])?;
    for &(name, kind) in interface.fields {
        let value = if is_nullish(c, init) { c.undefined() } else { get(c, init, name)? };
        let value = convert(c, interface, name, kind, value)?;
        put(c, state, name, value)?;
    }
    Ok(state)
}

fn slots(c: &mut Context<'_>, this: RootId, event: Event) -> Outcome {
    if let Some(state) = crate::modules::private_slots::read(c, this, SLOT)? {
        let brand = get(c, state, web::BRAND)?;
        if c.to_number(brand)? == event as usize as f64 {
            return Ok(state);
        }
    }
    let error = c.type_error_rooted("Illegal invocation")?;
    crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
    Err(c.throw(error))
}

fn construct(c: &mut Context<'_>, event: Event, args: &[RootId]) -> Outcome {
    let interface = event.interface();
    web::require_new(c, interface.name)?;
    if args.is_empty() {
        return Err(type_error(c, &format!("{} constructor: 1 argument required, but 0 found.", interface.name)));
    }
    let (kind, init) = (args[0], arg(c, args, 1));
    if !is_nullish(c, init) && !c.is_object_rooted(init)? && !c.is_callable_rooted(init)? {
        let shown = c.to_string(init)?;
        return Err(type_error(c, &format!("{} constructor: Expected {shown} to be one of: Null, Undefined, Object.", interface.name)));
    }
    let state = fields(c, interface, init)?;
    let brand = c.number(event as usize as f64);
    put(c, state, web::BRAND, brand)?;
    let family = crate::modules::events_shared_vm::event_family(c)?;
    let base = get(c, family, "eventCtor")?;
    let new_target = c.new_target_rooted().unwrap_or(base);
    let object = c.construct_rooted(base, new_target, &[kind, init])?;
    crate::modules::private_slots::write(c, object, SLOT, state)?;
    Ok(object)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::ConstructMessage => construct(c, Event::Message, args),
        Member::ConstructClose => construct(c, Event::Close, args),
        Member::ConstructError => construct(c, Event::Error, args),
        Member::Field => {
            let [interface_index, field_index] = captured(c)?;
            let event = EVENTS[c.to_number(interface_index)? as usize];
            let state = slots(c, this, event)?;
            let (name, _) = event.interface().fields[c.to_number(field_index)? as usize];
            get(c, state, name)
        }
        Member::InitMessageEvent => {
            let state = slots(c, this, Event::Message)?;
            let interface = Event::Message.interface();
            let init = record(c, &[], &[])?;
            for (index, &(name, _)) in interface.fields.iter().enumerate() {
                let value = arg(c, args, INIT_MESSAGE_FIELDS_START + index);
                put(c, init, name, value)?;
            }
            let fresh = fields(c, interface, init)?;
            for &(name, _) in interface.fields {
                let value = get(c, fresh, name)?;
                put(c, state, name, value)?;
            }
            Ok(c.undefined())
        }
    }
}
