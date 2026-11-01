//! `BroadcastChannel` (lib/internal/worker/io.js): an `EventTarget` that delivers each
//! posted message, structured-cloned, to every other open channel of the same name, as a
//! `message` event in a later turn of the event loop.
//!
//! Channels of this thread are registered by name; a message reaches the channels open
//! when it was posted that are still open when it arrives.

use crate::host::NodeHost;
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::webstreams_shared_vm::{self as web, arg, custom_inspect, get, install, is_undefined, literal, put, record, ClassMember};
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Member, "broadcastChannel", [
    Construct,
    Name,
    Close,
    PostMessage,
    Ref,
    Unref,
    GetOnMessage,
    SetOnMessage,
    GetOnMessageError,
    SetOnMessageError,
    Inspect,
    Deliver,
]);

/// The `builtin_cache` keys of the class and of the registry of open channels by name.
const CLASS: &str = "\0broadcastChannel:class";
const REGISTRY: &str = "\0broadcastChannel:registry";
const SLOT: &str = "\0slot:BroadcastChannel";
const INTERFACE: &str = "BroadcastChannel";
/// Registry keys: the channel name behind a prefix, so no name is an array index.
const KEY_PREFIX: &str = "c:";
const MESSAGE: &str = "message";
const MESSAGE_ERROR: &str = "messageerror";

/// A channel's fields.
mod field {
    pub(super) const NAME: &str = "name";
    pub(super) const CLOSED: &str = "closed";
    pub(super) const ON_MESSAGE: &str = "onmessage";
    pub(super) const ON_MESSAGE_ERROR: &str = "onmessageerror";
    pub(super) const SET_IMMEDIATE: &str = "setImmediate";
    pub(super) const CLASS: &str = "class";
}
use field::*;

const MEMBERS: [ClassMember<Member>; 5] = [
    ClassMember::Getter("name", Member::Name),
    ClassMember::Method("close", Member::Close, 0),
    ClassMember::Method("postMessage", Member::PostMessage, 1),
    ClassMember::Method("ref", Member::Ref, 0),
    ClassMember::Method("unref", Member::Unref, 0),
];

/// The class over `EventTarget`, built once.
pub(crate) fn class(c: &mut Context<'_>) -> Outcome {
    let table = table(c)?;
    get(c, table, field::CLASS)
}

/// The cached `{ class, setImmediate }`, `setImmediate` as the class found it.
fn table(c: &mut Context<'_>) -> Outcome {
    if let Some(table) = c.host_mut().shared_state().borrow().builtin_cache.get(CLASS).copied() {
        return Ok(table);
    }
    let table = c.with_startup_intrinsics(|c| {
        let family = crate::modules::events_shared_vm::event_family(c)?;
        let target = get(c, family, "eventTargetCtor")?;
        let target_prototype = get(c, target, "prototype")?;
        let constructor = crate::modules::host_members::constructor(c, Member::Construct, INTERFACE)?;
        web::set_length(c, constructor, 1)?;
        let prototype = get(c, constructor, "prototype")?;
        c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
        install(c, prototype, &MEMBERS)?;
        for (name, getter, setter) in
            [(ON_MESSAGE, Member::GetOnMessage, Member::SetOnMessage), (ON_MESSAGE_ERROR, Member::GetOnMessageError, Member::SetOnMessageError)]
        {
            let get_function = web::method_function(c, getter, &format!("get {name}"), 0)?;
            let set_function = web::method_function(c, setter, &format!("set {name}"), 1)?;
            let key = c.string_rooted(name);
            let fields = quench_runtime::DescriptorFields {
                get: Some(get_function),
                set: Some(set_function),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            };
            c.define_property_fields_rooted(prototype, key, fields)?;
        }
        c.set_prototype_rooted(prototype, target_prototype)?;
        c.set_prototype_rooted(constructor, target)?;
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let registry = get(c, symbol, "for")?;
        let description = c.string_rooted("nodejs.util.inspect.custom");
        let inspect_key = c.call_rooted(registry, symbol, &[description])?;
        let inspect = web::method_function(c, Member::Inspect, "[nodejs.util.inspect.custom]", 2)?;
        c.define_data_property_rooted(prototype, inspect_key, inspect, true, false, true)?;
        let set_immediate = get(c, global, SET_IMMEDIATE)?;
        record(c, &[(field::CLASS, constructor), (SET_IMMEDIATE, set_immediate)])
    })?;
    let retained = c.retain(table)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(CLASS.to_owned(), retained);
    Ok(table)
}

/// The `BroadcastChannel` global's lazy initializer.
pub(crate) fn initialize_global(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let class = class(c)?;
    let global = c.global_root()?;
    let key = c.string_rooted(INTERFACE);
    c.define_data_property_rooted(global, key, class, true, false, true)?;
    Ok(c.undefined())
}

fn registry(c: &mut Context<'_>) -> Outcome {
    if let Some(registry) = c.host_mut().shared_state().borrow().builtin_cache.get(REGISTRY).copied() {
        return Ok(registry);
    }
    let registry = c.null_object_rooted()?;
    let retained = c.retain(registry)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(REGISTRY.to_owned(), retained);
    Ok(registry)
}

fn members_named(c: &mut Context<'_>, name: &str) -> Result<Vec<RootId>, RootedError> {
    let registry = registry(c)?;
    let list = get(c, registry, &format!("{KEY_PREFIX}{name}"))?;
    if is_undefined(c, list) {
        return Ok(Vec::new());
    }
    c.array_like_values_rooted(list)
}

fn set_members(c: &mut Context<'_>, name: &str, channels: &[RootId]) -> Result<(), RootedError> {
    let registry = registry(c)?;
    let list = c.array_rooted(channels)?;
    put(c, registry, &format!("{KEY_PREFIX}{name}"), list)
}

fn missing_argument(c: &mut Context<'_>, name: &str) -> RootedError {
    let built = c.type_error_rooted(&format!("The \"{name}\" argument must be specified")).and_then(|error| {
        crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_MISSING_ARGS")?;
        Ok(error)
    });
    match built {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

fn slots(c: &mut Context<'_>, this: RootId) -> Outcome {
    match crate::modules::private_slots::read(c, this, SLOT)? {
        Some(state) => Ok(state),
        None => {
            let error = c.type_error_rooted("Value of \"this\" must be of type BroadcastChannel")?;
            crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
            Err(c.throw(error))
        }
    }
}

fn name_of(c: &mut Context<'_>, state: RootId) -> Result<String, RootedError> {
    let name = get(c, state, NAME)?;
    c.to_string(name)
}

fn close(c: &mut Context<'_>, this: RootId, state: RootId) -> Result<(), RootedError> {
    if web::flag(c, state, CLOSED)? {
        return Ok(());
    }
    web::put_flag(c, state, CLOSED, true)?;
    let name = name_of(c, state)?;
    let mut remaining = Vec::new();
    for channel in members_named(c, &name)? {
        if !web::same(c, channel, this)? {
            remaining.push(channel);
        }
    }
    set_members(c, &name, &remaining)
}

fn post(c: &mut Context<'_>, this: RootId, state: RootId, message: RootId) -> Result<(), RootedError> {
    if web::flag(c, state, CLOSED)? {
        let message = c.string_rooted("BroadcastChannel is closed.");
        let name = c.string_rooted("InvalidStateError");
        let error = crate::modules::dom_exception_shared_vm::create(c, message, name)?;
        return Err(c.throw(error));
    }
    let name = name_of(c, state)?;
    let undefined = c.undefined();
    let clone = crate::modules::clone_shared_vm::structured_clone(c, undefined, &[message])?;
    let table = table(c)?;
    let set_immediate = get(c, table, SET_IMMEDIATE)?;
    for channel in members_named(c, &name)? {
        if web::same(c, channel, this)? {
            continue;
        }
        let data = crate::modules::clone_shared_vm::structured_clone(c, undefined, &[clone])?;
        let deliver = function(c, Member::Deliver, "", &[channel, data])?;
        c.call_rooted(set_immediate, undefined, &[deliver])?;
    }
    Ok(())
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            web::require_new(c, INTERFACE)?;
            if args.is_empty() {
                return Err(missing_argument(c, "name"));
            }
            let name = c.to_string(args[0])?;
            let family = crate::modules::events_shared_vm::event_family(c)?;
            let target = get(c, family, "eventTargetCtor")?;
            let new_target = c.new_target_rooted().unwrap_or(target);
            let object = c.construct_rooted(target, new_target, &[])?;
            let name_value = c.string_rooted(&name);
            let state = record(c, &[(NAME, name_value)])?;
            web::put_flag(c, state, CLOSED, false)?;
            crate::modules::private_slots::write(c, object, SLOT, state)?;
            let mut channels = members_named(c, &name)?;
            channels.push(object);
            set_members(c, &name, &channels)?;
            Ok(object)
        }
        Member::Name => {
            let state = slots(c, this)?;
            get(c, state, NAME)
        }
        Member::Close => {
            let state = slots(c, this)?;
            close(c, this, state)?;
            Ok(c.undefined())
        }
        Member::PostMessage => {
            let state = slots(c, this)?;
            if args.is_empty() {
                return Err(missing_argument(c, "message"));
            }
            post(c, this, state, args[0])?;
            Ok(c.undefined())
        }
        Member::Ref | Member::Unref => {
            slots(c, this)?;
            Ok(this)
        }
        Member::GetOnMessage | Member::GetOnMessageError => {
            let state = slots(c, this)?;
            let name = if member == Member::GetOnMessage { ON_MESSAGE } else { ON_MESSAGE_ERROR };
            crate::modules::event_target_shared_vm::event_handler(c, state, name)
        }
        Member::SetOnMessage | Member::SetOnMessageError => {
            let state = slots(c, this)?;
            let (name, kind) = if member == Member::SetOnMessage { (ON_MESSAGE, MESSAGE) } else { (ON_MESSAGE_ERROR, MESSAGE_ERROR) };
            let value = arg(c, args, 0);
            crate::modules::event_target_shared_vm::set_event_handler(c, this, state, name, kind, value)?;
            Ok(c.undefined())
        }
        Member::Inspect => {
            let state = slots(c, this)?;
            let name = get(c, state, NAME)?;
            let active = !web::flag(c, state, CLOSED)?;
            let active = c.boolean(active);
            let data = literal(c, &[("name", name), ("active", active)])?;
            custom_inspect(c, this, INTERFACE, data, args)
        }
        Member::Deliver => {
            let [channel, data] = captured(c)?;
            let state = slots(c, channel)?;
            if web::flag(c, state, CLOSED)? {
                return Ok(c.undefined());
            }
            let init = literal(c, &[("data", data)])?;
            let event = crate::modules::message_event_shared_vm::create(c, crate::modules::message_event_shared_vm::Event::Message, MESSAGE, init)?;
            let dispatch = get(c, channel, "dispatchEvent")?;
            c.call_rooted(dispatch, channel, &[event])?;
            Ok(c.undefined())
        }
    }
}
