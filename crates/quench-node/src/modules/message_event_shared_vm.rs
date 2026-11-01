//! `MessageEvent` (undici's `lib/web/websocket/events.js`): an `Event` carrying a message's
//! data, origin, last event id, source port and transferred ports.

use crate::host::NodeHost;
use crate::modules::host_members::{current, members};
use crate::modules::webstreams_shared_vm::{self as web, arg, get, install, is_nullish, is_undefined, put, record, ClassMember};
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Member, "messageEvent", [Construct, Data, Origin, LastEventId, Source, Ports, InitMessageEvent]);

/// The `builtin_cache` key of the class, and its private slot.
const CLASS: &str = "\0messageEvent:class";
const SLOT: &str = "\0slot:MessageEvent";
const NAME: &str = "MessageEvent";

/// The event's fields, which are also its init dictionary's members.
mod field {
    pub(super) const DATA: &str = "data";
    pub(super) const ORIGIN: &str = "origin";
    pub(super) const LAST_EVENT_ID: &str = "lastEventId";
    pub(super) const SOURCE: &str = "source";
    pub(super) const PORTS: &str = "ports";
}
use field::*;

const MEMBERS: [ClassMember<Member>; 6] = [
    ClassMember::Getter("data", Member::Data),
    ClassMember::Getter("origin", Member::Origin),
    ClassMember::Getter("lastEventId", Member::LastEventId),
    ClassMember::Getter("source", Member::Source),
    ClassMember::Getter("ports", Member::Ports),
    ClassMember::Method("initMessageEvent", Member::InitMessageEvent, 1),
];

/// The class, built once over `Event`.
pub(crate) fn class(c: &mut Context<'_>) -> Outcome {
    if let Some(class) = c.host_mut().shared_state().borrow().builtin_cache.get(CLASS).copied() {
        return Ok(class);
    }
    let class = c.with_startup_intrinsics(|c| {
        let family = crate::modules::events_shared_vm::event_family(c)?;
        let event = get(c, family, "eventCtor")?;
        let event_prototype = get(c, event, "prototype")?;
        let constructor = crate::modules::host_members::constructor(c, Member::Construct, NAME)?;
        web::set_length(c, constructor, 1)?;
        let prototype = get(c, constructor, "prototype")?;
        c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
        install(c, prototype, &MEMBERS)?;
        c.set_prototype_rooted(prototype, event_prototype)?;
        c.set_prototype_rooted(constructor, event)?;
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let tag = get(c, symbol, "toStringTag")?;
        let name = c.string_rooted(NAME);
        c.define_data_property_rooted(prototype, tag, name, false, false, true)?;
        Ok(constructor)
    })?;
    let retained = c.retain(class)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(CLASS.to_owned(), retained);
    Ok(class)
}

/// Defines the `MessageEvent` global.
pub(crate) fn define_globals(c: &mut Context<'_>, global: RootId) -> Result<(), RootedError> {
    let class = class(c)?;
    let key = c.string_rooted(NAME);
    c.define_data_property_rooted(global, key, class, true, false, true)?;
    Ok(())
}

fn type_error(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// The init dictionary's members, defaulted and validated.
fn fields(c: &mut Context<'_>, init: RootId) -> Outcome {
    let member = |c: &mut Context<'_>, name: &str| -> Outcome { if is_nullish(c, init) { Ok(c.undefined()) } else { get(c, init, name) } };
    let data = member(c, DATA)?;
    let data = if is_undefined(c, data) { c.null() } else { data };
    let mut strings = Vec::new();
    for name in [ORIGIN, LAST_EVENT_ID] {
        let value = member(c, name)?;
        let text = if is_undefined(c, value) { String::new() } else { String::from_utf16_lossy(&c.to_string_code_units(value)?) };
        strings.push(c.string_rooted(&text));
    }
    let source = member(c, SOURCE)?;
    let source = if is_nullish(c, source) {
        c.null()
    } else if c.is_object_rooted(source)? {
        source
    } else {
        let shown = c.to_string(source)?;
        return Err(type_error(c, &format!("MessageEvent constructor: Expected eventInitDict.source (\"{shown}\") to be an instance of MessagePort.")));
    };
    let ports = member(c, PORTS)?;
    let ports = if is_undefined(c, ports) {
        Vec::new()
    } else {
        let iterable = c.is_object_rooted(ports)? && {
            let global = c.global_root()?;
            let symbol = get(c, global, "Symbol")?;
            let key = get(c, symbol, "iterator")?;
            let method = c.get_property_rooted(ports, key)?;
            c.is_callable_rooted(method)?
        };
        if !iterable {
            let shown = crate::modules::console_format_shared_vm::inspect(c, ports, Default::default())?;
            return Err(type_error(c, &format!("MessageEvent constructor: eventInitDict.ports ({shown}) is not iterable.")));
        }
        let mut iterator = c.iterator_rooted(ports)?;
        let mut values = Vec::new();
        while let Some(port) = c.iterator_step_rooted(&mut iterator)? {
            values.push(port);
        }
        values
    };
    let ports = c.array_rooted(&ports)?;
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let freeze = get(c, object, "freeze")?;
    c.call_rooted(freeze, object, &[ports])?;
    record(c, &[(DATA, data), (ORIGIN, strings[0]), (LAST_EVENT_ID, strings[1]), (SOURCE, source), (PORTS, ports)])
}

fn slots(c: &mut Context<'_>, this: RootId) -> Outcome {
    match crate::modules::private_slots::read(c, this, SLOT)? {
        Some(state) => Ok(state),
        None => {
            let error = c.type_error_rooted("Illegal invocation")?;
            crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
            Err(c.throw(error))
        }
    }
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            web::require_new(c, NAME)?;
            if args.is_empty() {
                return Err(type_error(c, "MessageEvent constructor: 1 argument required, but 0 found."));
            }
            let (kind, init) = (args[0], arg(c, args, 1));
            if !is_nullish(c, init) && !c.is_object_rooted(init)? && !c.is_callable_rooted(init)? {
                let shown = c.to_string(init)?;
                return Err(type_error(c, &format!("MessageEvent constructor: Expected {shown} to be one of: Null, Undefined, Object.")));
            }
            let state = fields(c, init)?;
            let family = crate::modules::events_shared_vm::event_family(c)?;
            let event = get(c, family, "eventCtor")?;
            let new_target = c.new_target_rooted().unwrap_or(event);
            let object = c.construct_rooted(event, new_target, &[kind, init])?;
            crate::modules::private_slots::write(c, object, SLOT, state)?;
            Ok(object)
        }
        Member::Data | Member::Origin | Member::LastEventId | Member::Source | Member::Ports => {
            let state = slots(c, this)?;
            let name = match member {
                Member::Data => DATA,
                Member::Origin => ORIGIN,
                Member::LastEventId => LAST_EVENT_ID,
                Member::Source => SOURCE,
                _ => PORTS,
            };
            get(c, state, name)
        }
        Member::InitMessageEvent => {
            let state = slots(c, this)?;
            let (data, origin, last_event_id, source, ports) =
                (arg(c, args, 3), arg(c, args, 4), arg(c, args, 5), arg(c, args, 6), arg(c, args, 7));
            let init = record(c, &[(DATA, data), (ORIGIN, origin), (LAST_EVENT_ID, last_event_id), (SOURCE, source), (PORTS, ports)])?;
            let fresh = fields(c, init)?;
            for name in [DATA, ORIGIN, LAST_EVENT_ID, SOURCE, PORTS] {
                let value = get(c, fresh, name)?;
                put(c, state, name, value)?;
            }
            Ok(c.undefined())
        }
    }
}
