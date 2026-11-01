//! The DOM `Event`, `CustomEvent` and `EventTarget` classes (`lib/internal/event_target.js`).
//!
//! An event's state is one private slot (`EVENT_SLOT`), a target's listeners one private
//! table (`TARGET_SLOT`) from event type to a list root. Each root heads a doubly linked
//! list of listener records in registration order, as Node keeps them: a removed record
//! keeps its `next`, so a dispatch walking the list continues past it, and a listener added
//! during a dispatch is reached unless it follows the record running.
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The event group's members (`SharedNodeState::event_targets_api`) naming each private slot.
pub(super) const EVENT_SLOT: &str = "eventSlot";
pub(super) const TARGET_SLOT: &str = "targetSlot";
pub(super) const DETAIL_SLOT: &str = "detailSlot";

/// `Event.NONE` .. `Event.BUBBLING_PHASE`.
const PHASES: [(&str, f64); 4] = [("NONE", 0.0), ("CAPTURING_PHASE", 1.0), ("AT_TARGET", 2.0), ("BUBBLING_PHASE", 3.0)];
const PHASE_NONE: f64 = 0.0;
const PHASE_AT_TARGET: f64 = 2.0;

/// The slot an event's fields live in.
mod field {
    pub(super) const TYPE: &str = "type";
    pub(super) const BUBBLES: &str = "bubbles";
    pub(super) const CANCELABLE: &str = "cancelable";
    pub(super) const COMPOSED: &str = "composed";
    pub(super) const PREVENTED: &str = "defaultPrevented";
    pub(super) const TIME_STAMP: &str = "timeStamp";
    pub(super) const TRUSTED: &str = "isTrusted";
    pub(super) const TARGET: &str = "target";
    pub(super) const DISPATCHING: &str = "dispatching";
    pub(super) const PROPAGATION_STOPPED: &str = "propagationStopped";
    pub(super) const STOPPED: &str = "stopped";
    pub(super) const PASSIVE: &str = "passive";
}

/// A listener record's and list root's fields.
mod record {
    pub(super) const NEXT: &str = "next";
    pub(super) const PREVIOUS: &str = "previous";
    pub(super) const SIZE: &str = "size";
    pub(super) const LISTENER: &str = "listener";
    pub(super) const ONCE: &str = "once";
    pub(super) const CAPTURE: &str = "capture";
    pub(super) const PASSIVE: &str = "passive";
    pub(super) const RESIST: &str = "resistStopPropagation";
    pub(super) const REMOVED: &str = "removed";
}

pub(super) fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

pub(super) fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

pub(super) fn flag(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    c.truthy_rooted(value)
}

pub(super) fn put_flag(c: &mut Context<'_>, object: RootId, name: &str, value: bool) -> Result<(), RootedError> {
    let value = c.boolean(value);
    put(c, object, name, value)
}

pub(super) fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}

pub(super) fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

/// An object or a function: not a primitive.
pub(super) fn is_object_like(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(c.is_object_rooted(value)? || c.is_callable_rooted(value)?)
}

pub(super) fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

/// The event group's shared data.
pub(super) fn api(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventTarget API missing"))
}

/// The private slot `slot` names on `object`, when `object` carries one.
pub(super) fn slot(c: &mut Context<'_>, object: RootId, slot: &str) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(None);
    }
    let data = api(c)?;
    let key = get(c, data, slot)?;
    Ok(c.own_property_fields_rooted(object, key)?.and_then(|fields| fields.value))
}

pub(super) fn attach_slot(c: &mut Context<'_>, object: RootId, slot: &str, value: RootId) -> Result<(), RootedError> {
    let data = api(c)?;
    let key = get(c, data, slot)?;
    c.define_data_property_rooted(object, key, value, true, false, true)?;
    Ok(())
}

/// `ERR_INVALID_THIS`: `Value of "this" must be of type <name>`.
pub(super) fn invalid_this(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => match super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS") {
            Ok(()) => c.throw(error),
            Err(error) => error,
        },
        Err(error) => error,
    }
}

fn invalid_this_of(c: &mut Context<'_>, name: &str) -> RootedError {
    invalid_this(c, &format!("Value of \"this\" must be of type {name}"))
}

/// The slot `slot` of the receiver, or `ERR_INVALID_THIS` naming `class`.
pub(super) fn receiver_slot(c: &mut Context<'_>, receiver: RootId, slot_name: &str, class: &str) -> Result<RootId, RootedError> {
    slot(c, receiver, slot_name)?.ok_or_else(|| invalid_this_of(c, class))
}

/// `ERR_MISSING_ARGS` for `names`, in Node's list form.
pub(super) fn missing_args(c: &mut Context<'_>, names: &[&str]) -> RootedError {
    let quoted = names.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>();
    let list = match quoted.as_slice() {
        [one] => format!("{one} argument"),
        [first, second] => format!("{first} and {second} arguments"),
        [rest @ .., last] => format!("{}, and {last} arguments", rest.join(", ")),
        [] => String::new(),
    };
    let message = format!("The {list} must be specified");
    match c.type_error_rooted(&message) {
        Ok(error) => match super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_MISSING_ARGS") {
            Ok(()) => c.throw(error),
            Err(error) => error,
        },
        Err(error) => error,
    }
}

/// The TypeError a class constructor called without `new` throws.
pub(super) fn require_new(c: &mut Context<'_>, class: &str) -> Result<(), RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!("Class constructor {class} cannot be invoked without 'new'"))?;
    Err(c.throw(error))
}

/// `${value}`: ToString, a Symbol throwing.
pub(super) fn to_dom_string(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    if c.is_symbol_rooted(value)? {
        let error = c.type_error_rooted("Cannot convert a Symbol value to a string")?;
        return Err(c.throw(error));
    }
    let units = c.to_string_code_units(value)?;
    Ok(c.string_units_rooted(&units))
}

/// A class member the installer defines on a prototype or constructor.
pub(super) enum Member {
    /// A method: name, operation, data index.
    Method(&'static str, &'static str, f64),
    /// An accessor: name, getter operation, optional setter operation, data index.
    Accessor(&'static str, &'static str, Option<&'static str>, f64),
}

/// Defines `members` on `holder` as Node's classes define them: enumerable methods and
/// accessors, each operation's function data its index (`Member`).
pub(super) fn install_members(c: &mut Context<'_>, holder: RootId, members: &[Member], enumerable: bool) -> Result<(), RootedError> {
    let operation = crate::host::shared_vm::operation;
    for member in members {
        match *member {
            Member::Method(name, op, index) => {
                let data = c.number(index);
                let function = c.host_function_named_with_data(operation(op), name, data)?;
                let key = c.string_rooted(name);
                c.define_data_property_rooted(holder, key, function, true, enumerable, true)?;
            }
            Member::Accessor(name, getter, setter, index) => {
                let data = c.number(index);
                let get = c.host_function_named_with_data(operation(getter), &format!("get {name}"), data)?;
                let set = match setter {
                    Some(setter) => c.host_function_named_with_data(operation(setter), &format!("set {name}"), data)?,
                    None => c.undefined(),
                };
                let key = c.string_rooted(name);
                c.define_property_fields_rooted(
                    holder,
                    key,
                    DescriptorFields { get: Some(get), set: Some(set), enumerable: Some(enumerable), configurable: Some(true), ..Default::default() },
                )?;
            }
        }
    }
    Ok(())
}

/// `Symbol.toStringTag` and `util.inspect.custom` on a class prototype, whose constructor's
/// `prototype` then becomes read-only, as a class declaration's is.
pub(super) fn install_tag_and_inspect(c: &mut Context<'_>, prototype: RootId, tag: &str, inspect_operation: Option<&str>) -> Result<(), RootedError> {
    let constructor = get(c, prototype, "constructor")?;
    c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    if let Some(operation) = inspect_operation {
        let registry = get(c, symbol, "for")?;
        let description = c.string_rooted("nodejs.util.inspect.custom");
        let key = c.call_rooted(registry, symbol, &[description])?;
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), "[nodejs.util.inspect.custom]")?;
        c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    let key = get(c, symbol, "toStringTag")?;
    let value = c.string_rooted(tag);
    c.define_data_property_rooted(prototype, key, value, false, false, true)?;
    Ok(())
}

/// How a class's custom inspection writes an instance below the depth limit.
pub(super) enum Shallow {
    /// Its constructor's name.
    Name,
    /// Itself, which `inspect` then writes as `[Name]`.
    Receiver,
}

/// Node's custom inspection of a DOM class instance: `${constructor.name} ${inspect(body)}`
/// one level deeper, or `shallow` past the depth limit. `args` are `(depth, options, inspect)`.
pub(super) fn custom_inspect(c: &mut Context<'_>, receiver: RootId, body: RootId, args: &[RootId], shallow: Shallow) -> Result<RootId, RootedError> {
    let constructor = get(c, receiver, "constructor")?;
    let name = if is_object_like(c, constructor)? { get(c, constructor, "name")? } else { c.undefined() };
    let name = c.to_string(name)?;
    let depth = argument(c, args, 0);
    if c.rooted_value(depth).and_then(|value| value.as_number()).is_some_and(|depth| depth < 0.0) {
        return Ok(match shallow {
            Shallow::Name => c.string_rooted(&name),
            Shallow::Receiver => receiver,
        });
    }
    let options = argument(c, args, 1);
    let copy = c.object_rooted()?;
    if is_object_like(c, options)? {
        for (key, value) in c.plain_enumerable_entries_rooted(options).unwrap_or_default() {
            put(c, copy, &key, value)?;
        }
        let depth = get(c, options, "depth")?;
        let next = match c.rooted_value(depth).and_then(|value| value.as_number()) {
            Some(depth) if depth.fract() == 0.0 => c.number(depth - 1.0),
            _ => depth,
        };
        put(c, copy, "depth", next)?;
    }
    let inspect = argument(c, args, 2);
    let inspect = if c.is_callable_rooted(inspect)? { inspect } else { super::util_shared_vm::internal_inspect(c)? };
    let undefined = c.undefined();
    let text = c.call_rooted(inspect, undefined, &[body, copy])?;
    let text = c.to_string(text)?;
    Ok(c.string_rooted(&format!("{name} {text}")))
}

/// Builds the classes and defines `Event`, `CustomEvent` and `EventTarget` on `api`.
pub(crate) fn install(c: &mut Context<'_>, data: RootId) -> Result<(), RootedError> {
    let operation = crate::host::shared_vm::operation;
    let event = c.host_constructor_named(operation("eventConstructor"), "Event")?;
    let length = c.number(1.0);
    c.define_named_data_property_rooted(event, "length", length, false, false, true)?;
    for (name, phase) in PHASES {
        let value = c.number(phase);
        c.define_named_data_property_rooted(event, name, value, false, true, false)?;
    }
    let prototype = get(c, event, "prototype")?;
    install_members(c, prototype, &EVENT_MEMBERS, true)?;
    let trusted = c.host_function_named(operation("eventIsTrusted"), "isTrusted")?;
    let key = c.string_rooted("isTrusted");
    let undefined = c.undefined();
    c.define_property_fields_rooted(
        prototype,
        key,
        DescriptorFields { get: Some(trusted), set: Some(undefined), enumerable: Some(true), configurable: Some(false), ..Default::default() },
    )?;
    install_tag_and_inspect(c, prototype, "Event", Some("eventInspect"))?;
    put(c, data, "eventCtor", event)?;

    let custom = c.host_constructor_named(operation("customEventConstructor"), "CustomEvent")?;
    c.define_named_data_property_rooted(custom, "length", length, false, false, true)?;
    c.set_prototype_rooted(custom, event)?;
    let custom_prototype = get(c, custom, "prototype")?;
    c.set_prototype_rooted(custom_prototype, prototype)?;
    install_members(c, custom_prototype, &[Member::Accessor("detail", "customEventDetail", None, 0.0)], true)?;
    install_tag_and_inspect(c, custom_prototype, "CustomEvent", None)?;
    put(c, data, "customEventCtor", custom)?;

    let target = c.host_constructor_named(operation("eventTargetConstructor"), "EventTarget")?;
    let target_prototype = get(c, target, "prototype")?;
    install_members(c, target_prototype, &TARGET_MEMBERS, true)?;
    install_tag_and_inspect(c, target_prototype, "EventTarget", Some("eventTargetInspect"))?;
    let brand = get(c, data, "brandKey")?;
    let yes = c.boolean(true);
    c.define_data_property_rooted(target, brand, yes, true, true, true)?;
    put(c, data, "eventTargetCtor", target)?;
    Ok(())
}

/// `Event.prototype`'s members in Node's order; accessor data indexes `EventView`.
const EVENT_MEMBERS: [Member; 17] = [
    Member::Method("initEvent", "eventInitEvent", 0.0),
    Member::Method("stopImmediatePropagation", "eventStopImmediatePropagation", 0.0),
    Member::Method("preventDefault", "eventPreventDefault", 0.0),
    Member::Accessor("target", "eventGet", None, EventView::Target as u8 as f64),
    Member::Accessor("currentTarget", "eventGet", None, EventView::CurrentTarget as u8 as f64),
    Member::Accessor("srcElement", "eventGet", None, EventView::Target as u8 as f64),
    Member::Accessor("type", "eventGet", None, EventView::Type as u8 as f64),
    Member::Accessor("cancelable", "eventGet", None, EventView::Cancelable as u8 as f64),
    Member::Accessor("defaultPrevented", "eventGet", None, EventView::DefaultPrevented as u8 as f64),
    Member::Accessor("timeStamp", "eventGet", None, EventView::TimeStamp as u8 as f64),
    Member::Method("composedPath", "eventComposedPath", 0.0),
    Member::Accessor("returnValue", "eventGet", Some("eventSetReturnValue"), EventView::ReturnValue as u8 as f64),
    Member::Accessor("bubbles", "eventGet", None, EventView::Bubbles as u8 as f64),
    Member::Accessor("composed", "eventGet", None, EventView::Composed as u8 as f64),
    Member::Accessor("eventPhase", "eventGet", None, EventView::EventPhase as u8 as f64),
    Member::Accessor("cancelBubble", "eventGet", Some("eventSetCancelBubble"), EventView::CancelBubble as u8 as f64),
    Member::Method("stopPropagation", "eventStopPropagation", 0.0),
];

const TARGET_MEMBERS: [Member; 3] = [
    Member::Method("addEventListener", "eventTargetAddEventListener", 0.0),
    Member::Method("removeEventListener", "eventTargetRemoveEventListener", 0.0),
    Member::Method("dispatchEvent", "eventTargetDispatchEvent", 0.0),
];

/// What an `Event.prototype` getter reads (its function data).
#[derive(Clone, Copy)]
#[repr(u8)]
enum EventView {
    Type,
    Bubbles,
    Cancelable,
    Composed,
    DefaultPrevented,
    TimeStamp,
    Target,
    CurrentTarget,
    EventPhase,
    ReturnValue,
    CancelBubble,
}

impl EventView {
    const ALL: [EventView; 11] = [
        EventView::Type,
        EventView::Bubbles,
        EventView::Cancelable,
        EventView::Composed,
        EventView::DefaultPrevented,
        EventView::TimeStamp,
        EventView::Target,
        EventView::CurrentTarget,
        EventView::EventPhase,
        EventView::ReturnValue,
        EventView::CancelBubble,
    ];
}

fn event_slot(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    receiver_slot(c, receiver, EVENT_SLOT, "Event")
}

/// Validates `new Event(type, options)`'s options: absent, null or any object.
fn event_options(c: &mut Context<'_>, options: RootId) -> Result<(), RootedError> {
    if is_nullish(c, options) || is_object_like(c, options)? {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?)
}

/// A fresh event slot for `type` with the options' flags.
fn new_event_slot(c: &mut Context<'_>, kind: RootId, options: RootId, trusted: bool) -> Result<RootId, RootedError> {
    let state = c.null_object_rooted()?;
    put(c, state, field::TYPE, kind)?;
    for name in [field::BUBBLES, field::CANCELABLE, field::COMPOSED] {
        let value = !is_nullish(c, options) && flag(c, options, name)?;
        put_flag(c, state, name, value)?;
    }
    for name in [field::PREVENTED, field::DISPATCHING, field::PROPAGATION_STOPPED, field::STOPPED, field::PASSIVE] {
        put_flag(c, state, name, false)?;
    }
    put_flag(c, state, field::TRUSTED, trusted)?;
    let undefined = c.undefined();
    let now = super::perf_hooks::now(c, undefined, &[])?;
    put(c, state, field::TIME_STAMP, now)?;
    let null = c.null();
    put(c, state, field::TARGET, null)?;
    Ok(state)
}

/// `new Event(type, options)`.
pub(crate) fn event_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "Event")?;
    if args.is_empty() {
        return Err(missing_args(c, &["type"]));
    }
    let options = argument(c, args, 1);
    event_options(c, options)?;
    let kind = to_dom_string(c, args[0])?;
    let state = new_event_slot(c, kind, options, false)?;
    attach_slot(c, receiver, EVENT_SLOT, state)?;
    Ok(c.undefined())
}

/// A trusted event of `type` made by the host (the `abort` event of an AbortSignal).
pub(super) fn trusted_event(c: &mut Context<'_>, kind: &str) -> Result<RootId, RootedError> {
    let data = api(c)?;
    let constructor = get(c, data, "eventCtor")?;
    let prototype = get(c, constructor, "prototype")?;
    let event = c.object_rooted()?;
    c.set_prototype_rooted(event, prototype)?;
    let kind = c.string_rooted(kind);
    let undefined = c.undefined();
    let state = new_event_slot(c, kind, undefined, true)?;
    attach_slot(c, event, EVENT_SLOT, state)?;
    Ok(event)
}

/// The `Event.prototype` getters.
pub(crate) fn event_get(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    let view = c.host_function_data()?;
    let view = EventView::ALL[c.to_number(view)? as usize];
    let dispatching = flag(c, state, field::DISPATCHING)?;
    Ok(match view {
        EventView::Type => get(c, state, field::TYPE)?,
        EventView::Bubbles => get(c, state, field::BUBBLES)?,
        EventView::Cancelable => get(c, state, field::CANCELABLE)?,
        EventView::Composed => get(c, state, field::COMPOSED)?,
        EventView::DefaultPrevented => get(c, state, field::PREVENTED)?,
        EventView::TimeStamp => get(c, state, field::TIME_STAMP)?,
        EventView::Target => get(c, state, field::TARGET)?,
        EventView::CurrentTarget if dispatching => get(c, state, field::TARGET)?,
        EventView::CurrentTarget => c.null(),
        EventView::EventPhase => c.number(if dispatching { PHASE_AT_TARGET } else { PHASE_NONE }),
        EventView::ReturnValue => {
            let prevented = flag(c, state, field::PREVENTED)?;
            c.boolean(!prevented)
        }
        EventView::CancelBubble => get(c, state, field::PROPAGATION_STOPPED)?,
    })
}

/// `Event.prototype.isTrusted`.
pub(crate) fn event_is_trusted(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    match slot(c, receiver, EVENT_SLOT)? {
        Some(state) => get(c, state, field::TRUSTED),
        None => Ok(c.boolean(false)),
    }
}

fn prevent_default(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    if flag(c, state, field::CANCELABLE)? && !flag(c, state, field::PASSIVE)? {
        put_flag(c, state, field::PREVENTED, true)?;
    }
    Ok(())
}

/// `set returnValue(value)`: a falsy value prevents the default.
pub(crate) fn event_set_return_value(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    let value = argument(c, args, 0);
    if !c.truthy_rooted(value)? {
        prevent_default(c, state)?;
    }
    Ok(c.undefined())
}

/// `set cancelBubble(value)`: a truthy value stops propagation.
pub(crate) fn event_set_cancel_bubble(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    let value = argument(c, args, 0);
    if c.truthy_rooted(value)? {
        put_flag(c, state, field::PROPAGATION_STOPPED, true)?;
    }
    Ok(c.undefined())
}

pub(crate) fn event_prevent_default(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    prevent_default(c, state)?;
    Ok(c.undefined())
}

pub(crate) fn event_stop_propagation(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    put_flag(c, state, field::PROPAGATION_STOPPED, true)?;
    Ok(c.undefined())
}

pub(crate) fn event_stop_immediate_propagation(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    put_flag(c, state, field::PROPAGATION_STOPPED, true)?;
    put_flag(c, state, field::STOPPED, true)?;
    Ok(c.undefined())
}

/// `composedPath()`: the current target while dispatching.
pub(crate) fn event_composed_path(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    if flag(c, state, field::DISPATCHING)? {
        let target = get(c, state, field::TARGET)?;
        c.array_rooted(&[target])
    } else {
        c.array_rooted(&[])
    }
}

/// `initEvent(type, bubbles = false, cancelable = false)`, ignored while dispatching.
pub(crate) fn event_init_event(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    if args.is_empty() {
        return Err(missing_args(c, &["type"]));
    }
    if flag(c, state, field::DISPATCHING)? {
        return Ok(c.undefined());
    }
    let kind = to_dom_string(c, args[0])?;
    put(c, state, field::TYPE, kind)?;
    for (index, name) in [(1, field::BUBBLES), (2, field::CANCELABLE)] {
        let value = argument(c, args, index);
        let value = c.truthy_rooted(value)?;
        put_flag(c, state, name, value)?;
    }
    Ok(c.undefined())
}

/// `[util.inspect.custom]`: `Event { type, defaultPrevented, cancelable, timeStamp }`.
pub(crate) fn event_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = event_slot(c, receiver)?;
    let body = c.object_rooted()?;
    for name in [field::TYPE, field::PREVENTED, field::CANCELABLE, field::TIME_STAMP] {
        let value = get(c, state, name)?;
        put(c, body, name, value)?;
    }
    custom_inspect(c, receiver, body, args, Shallow::Name)
}

/// `new CustomEvent(type, options)`: an event carrying `options.detail ?? null`.
pub(crate) fn custom_event_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "CustomEvent")?;
    if args.is_empty() {
        return Err(missing_args(c, &["type"]));
    }
    event_constructor(c, receiver, args)?;
    let options = argument(c, args, 1);
    let detail = if is_nullish(c, options) { c.undefined() } else { get(c, options, "detail")? };
    let detail = if is_nullish(c, detail) { c.null() } else { detail };
    attach_slot(c, receiver, DETAIL_SLOT, detail)?;
    Ok(c.undefined())
}

pub(crate) fn custom_event_detail(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    receiver_slot(c, receiver, DETAIL_SLOT, "CustomEvent")
}

/// Gives `target` an EventTarget's state: an empty listener table and the default listener
/// limit (`initEventTarget`).
pub(super) fn initialize_target(c: &mut Context<'_>, target: RootId, maximum: RootId) -> Result<(), RootedError> {
    let table = c.null_object_rooted()?;
    attach_slot(c, target, TARGET_SLOT, table)?;
    let data = api(c)?;
    let max_key = get(c, data, "maxKey")?;
    c.define_data_property_rooted(target, max_key, maximum, true, true, true)?;
    let warned_key = get(c, data, "warnedKey")?;
    let no = c.boolean(false);
    c.define_data_property_rooted(target, warned_key, no, true, true, true)?;
    Ok(())
}

/// `new EventTarget()`.
pub(crate) fn event_target_constructor(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "EventTarget")?;
    let data = api(c)?;
    let maximum = get(c, data, "defaultMaxListeners")?;
    initialize_target(c, receiver, maximum)?;
    Ok(c.undefined())
}

/// The receiver's listener table, when it is an EventTarget (`isEventTarget`: its
/// constructor carries the brand).
fn target_table(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    if !is_event_target(c, receiver)? {
        return Err(invalid_this_of(c, "EventTarget"));
    }
    receiver_slot(c, receiver, TARGET_SLOT, "EventTarget")
}

/// Node's `isEventTarget`: `obj?.constructor?.[kIsEventTarget]`.
pub(super) fn is_event_target(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !is_object_like(c, value)? {
        return Ok(false);
    }
    let constructor = get(c, value, "constructor")?;
    if is_nullish(c, constructor) {
        return Ok(false);
    }
    let data = api(c)?;
    let brand = get(c, data, "brandKey")?;
    let branded = c.get_property_rooted(constructor, brand)?;
    c.truthy_rooted(branded)
}

/// The list root of `type` in `target`'s table, if any listener was ever kept for it.
fn list_root(c: &mut Context<'_>, table: RootId, kind: RootId) -> Result<Option<RootId>, RootedError> {
    Ok(c.own_property_fields_rooted(table, kind)?.and_then(|fields| fields.value))
}

/// The records of a list, in order, skipping removed ones.
fn records(c: &mut Context<'_>, root: RootId) -> Result<Vec<RootId>, RootedError> {
    let mut found = Vec::new();
    let mut current = get(c, root, record::NEXT)?;
    while !is_undefined(c, current) {
        if !flag(c, current, record::REMOVED)? {
            found.push(current);
        }
        current = get(c, current, record::NEXT)?;
    }
    Ok(found)
}

/// The listeners of `kind` on EventTarget `target`, for `getEventListeners`.
pub(super) fn listeners(c: &mut Context<'_>, target: RootId, kind: RootId) -> Result<Vec<RootId>, RootedError> {
    let Some(table) = slot(c, target, TARGET_SLOT)? else { return Ok(Vec::new()) };
    let kind = to_dom_string(c, kind)?;
    let Some(root) = list_root(c, table, kind)? else { return Ok(Vec::new()) };
    let mut listeners = Vec::new();
    for entry in records(c, root)? {
        listeners.push(get(c, entry, record::LISTENER)?);
    }
    Ok(listeners)
}

/// The listener options Node's `validateListenerOptions` derives.
struct ListenerOptions {
    once: bool,
    capture: bool,
    passive: bool,
    resist: bool,
    signal: RootId,
}

fn listener_options(c: &mut Context<'_>, options: RootId) -> Result<ListenerOptions, RootedError> {
    let undefined = c.undefined();
    if let Some(capture) = c.rooted_value(options).and_then(|value| value.as_bool()) {
        return Ok(ListenerOptions { once: false, capture, passive: false, resist: false, signal: undefined });
    }
    if is_nullish(c, options) {
        return Ok(ListenerOptions { once: false, capture: false, passive: false, resist: false, signal: undefined });
    }
    if !is_object_like(c, options)? {
        return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let data = api(c)?;
    let resist_key = get(c, data, "resistKey")?;
    let resist = c.get_property_rooted(options, resist_key)?;
    let parsed = ListenerOptions {
        once: flag(c, options, "once")?,
        capture: flag(c, options, "capture")?,
        passive: flag(c, options, "passive")?,
        resist: c.truthy_rooted(resist)?,
        signal: get(c, options, "signal")?,
    };
    if !is_undefined(c, parsed.signal) && !super::abort_controller_shared_vm::is_abort_signal(c, parsed.signal)? {
        return Err(super::util_shared_vm::argument_type_error(c, parsed.signal, "options.signal", "AbortSignal")?);
    }
    Ok(parsed)
}

/// Node's `validateEventListener`: a function or an object (whose `handleEvent` is read at
/// dispatch) registers; null and undefined have no effect.
fn listener_registers(c: &mut Context<'_>, listener: RootId) -> Result<bool, RootedError> {
    if is_nullish(c, listener) {
        return Ok(false);
    }
    if is_object_like(c, listener)? {
        return Ok(true);
    }
    Err(super::util_shared_vm::argument_type_error(c, listener, "listener", "EventListener")?)
}

/// `process.emitWarning(warning)`.
pub(super) fn emit_warning(c: &mut Context<'_>, warning: RootId) -> Result<(), RootedError> {
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    let emit = get(c, process, "emitWarning")?;
    c.call_rooted(emit, process, &[warning])?;
    Ok(())
}

/// A warning `Error` named `name` with `fields` set on it.
fn warning(c: &mut Context<'_>, name: &str, message: &str, fields: &[(&str, RootId)]) -> Result<RootId, RootedError> {
    let warning = c.error_rooted(message)?;
    let name = c.string_rooted(name);
    put(c, warning, "name", name)?;
    for &(field, value) in fields {
        put(c, warning, field, value)?;
    }
    Ok(warning)
}

/// `kNewListener`: the first registration past the target's limit warns once.
fn new_listener(c: &mut Context<'_>, target: RootId, size: f64, kind: RootId) -> Result<(), RootedError> {
    let data = api(c)?;
    let max_key = get(c, data, "maxKey")?;
    let maximum = c.get_property_rooted(target, max_key)?;
    let maximum = c.rooted_value(maximum).and_then(|value| value.as_number()).unwrap_or_default();
    let warned_key = get(c, data, "warnedKey")?;
    let warned = c.get_property_rooted(target, warned_key)?;
    if maximum <= 0.0 || size <= maximum || c.truthy_rooted(warned)? {
        return Ok(());
    }
    let yes = c.boolean(true);
    c.set_property_rooted(target, warned_key, yes, target)?;
    let shallow = if super::abort_controller_shared_vm::is_abort_signal(c, target)? {
        "[AbortSignal]".to_owned()
    } else {
        let constructor = get(c, target, "constructor")?;
        let name = get(c, constructor, "name")?;
        c.to_string(name)?
    };
    let kind_text = c.to_string(kind)?;
    let message = format!(
        "Possible EventTarget memory leak detected. {size} {kind_text} listeners added to {shallow}. MaxListeners is {maximum}. Use events.setMaxListeners() to increase limit"
    );
    let count = c.number(size);
    let warning = warning(c, "MaxListenersExceededWarning", &message, &[("target", target), ("type", kind), ("count", count)])?;
    emit_warning(c, warning)
}

/// Registers `listener` for `kind` on `target`'s `table` unless an equal registration
/// exists; returns whether it was added.
fn add_listener(c: &mut Context<'_>, target: RootId, table: RootId, kind: RootId, listener: RootId, options: &ListenerOptions) -> Result<bool, RootedError> {
    let entry = c.null_object_rooted()?;
    put(c, entry, record::LISTENER, listener)?;
    put_flag(c, entry, record::ONCE, options.once)?;
    put_flag(c, entry, record::CAPTURE, options.capture)?;
    put_flag(c, entry, record::PASSIVE, options.passive)?;
    put_flag(c, entry, record::RESIST, options.resist)?;
    put_flag(c, entry, record::REMOVED, false)?;
    let root = match list_root(c, table, kind)? {
        Some(root) => root,
        None => {
            let root = c.null_object_rooted()?;
            let zero = c.number(0.0);
            put(c, root, record::SIZE, zero)?;
            put_flag(c, root, record::RESIST, options.resist)?;
            c.define_data_property_rooted(table, kind, root, true, true, true)?;
            root
        }
    };
    let mut last = root;
    loop {
        let next = get(c, last, record::NEXT)?;
        if is_undefined(c, next) {
            break;
        }
        let same = get(c, next, record::LISTENER)?;
        if c.strict_equal_rooted(same, listener)? && flag(c, next, record::CAPTURE)? == options.capture {
            return Ok(false);
        }
        last = next;
    }
    put(c, last, record::NEXT, entry)?;
    put(c, entry, record::PREVIOUS, last)?;
    let size = get(c, root, record::SIZE)?;
    let size = c.to_number(size)? + 1.0;
    let size_value = c.number(size);
    put(c, root, record::SIZE, size_value)?;
    new_listener(c, target, size, kind)?;
    Ok(true)
}

/// Unlinks `entry` from its list (`Listener.remove`) and keeps the root's size; an emptied
/// type leaves the table.
fn remove_record(c: &mut Context<'_>, table: RootId, kind: RootId, root: RootId, entry: RootId) -> Result<(), RootedError> {
    let previous = get(c, entry, record::PREVIOUS)?;
    let next = get(c, entry, record::NEXT)?;
    if !is_undefined(c, previous) {
        put(c, previous, record::NEXT, next)?;
    }
    if !is_undefined(c, next) {
        put(c, next, record::PREVIOUS, previous)?;
    }
    put_flag(c, entry, record::REMOVED, true)?;
    let size = get(c, root, record::SIZE)?;
    let size = c.to_number(size)? - 1.0;
    let size_value = c.number(size);
    put(c, root, record::SIZE, size_value)?;
    if size == 0.0 {
        c.delete_property_rooted(table, kind)?;
    }
    Ok(())
}

/// `addEventListener(type, listener, options)`.
pub(crate) fn add_event_listener(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let table = target_table(c, receiver)?;
    if args.len() < 2 {
        return Err(missing_args(c, &["type", "listener"]));
    }
    let listener = args[1];
    let options = argument(c, args, 2);
    let options = listener_options(c, options)?;
    if !listener_registers(c, listener)? {
        let text = c.to_string(listener)?;
        let message = format!("addEventListener called with {text} which has no effect.");
        let warning = warning(c, "AddEventListenerArgumentTypeWarning", &message, &[("target", receiver), ("type", args[0])])?;
        emit_warning(c, warning)?;
        return Ok(c.undefined());
    }
    let kind = to_dom_string(c, args[0])?;
    if !is_undefined(c, options.signal) {
        if super::abort_controller_shared_vm::aborted(c, options.signal)? {
            return Ok(c.undefined());
        }
        let capture = c.boolean(options.capture);
        let registration = c.array_rooted(&[receiver, kind, listener, capture])?;
        let remover = c.host_function_with_data(crate::host::shared_vm::operation("eventTargetSignalRemove"), registration)?;
        let signal_options = ListenerOptions { once: true, capture: false, passive: false, resist: true, signal: c.undefined() };
        let signal_table = target_table(c, options.signal)?;
        let abort = c.string_rooted("abort");
        add_listener(c, options.signal, signal_table, abort, remover, &signal_options)?;
    }
    add_listener(c, receiver, table, kind, listener, &options)?;
    Ok(c.undefined())
}

/// The abort listener of a registration made with `options.signal`: removes it.
pub(crate) fn signal_remove(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let remover = c.host_function_data()?;
    let values = c.array_like_values_rooted(remover)?;
    let [target, kind, listener, capture] = values[..] else {
        return Err(RootedError::host("signal removal record malformed"));
    };
    let capture = c.truthy_rooted(capture)?;
    remove_listener(c, target, kind, listener, capture)?;
    Ok(c.undefined())
}

fn remove_listener(c: &mut Context<'_>, target: RootId, kind: RootId, listener: RootId, capture: bool) -> Result<(), RootedError> {
    let table = receiver_slot(c, target, TARGET_SLOT, "EventTarget")?;
    let Some(root) = list_root(c, table, kind)? else { return Ok(()) };
    for entry in records(c, root)? {
        let same = get(c, entry, record::LISTENER)?;
        if c.strict_equal_rooted(same, listener)? && flag(c, entry, record::CAPTURE)? == capture {
            remove_record(c, table, kind, root, entry)?;
            break;
        }
    }
    Ok(())
}

/// `removeEventListener(type, listener, options)`: only `options.capture === true` selects
/// a capturing registration, as in Node.
pub(crate) fn remove_event_listener(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    target_table(c, receiver)?;
    if args.len() < 2 {
        return Err(missing_args(c, &["type", "listener"]));
    }
    let listener = args[1];
    if !listener_registers(c, listener)? {
        return Ok(c.undefined());
    }
    let kind = to_dom_string(c, args[0])?;
    let options = argument(c, args, 2);
    let capture = if is_object_like(c, options)? {
        let capture = get(c, options, "capture")?;
        c.rooted_value(capture).and_then(|value| value.as_bool()) == Some(true)
    } else {
        false
    };
    remove_listener(c, receiver, kind, listener, capture)?;
    Ok(c.undefined())
}

/// `dispatchEvent(event)`.
pub(crate) fn dispatch_event(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if !is_event_target(c, receiver)? {
        return Err(invalid_this_of(c, "EventTarget"));
    }
    if args.is_empty() {
        return Err(missing_args(c, &["event"]));
    }
    let event = args[0];
    let Some(state) = slot(c, event, EVENT_SLOT)? else {
        return Err(super::util_shared_vm::argument_type_error(c, event, "event", "Event")?);
    };
    if flag(c, state, field::DISPATCHING)? {
        let kind = get(c, state, field::TYPE)?;
        let kind = c.to_string(kind)?;
        let error = c.error_rooted(&format!("The event \"{kind}\" is already being dispatched"))?;
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_EVENT_RECURSION")?;
        return Err(c.throw(error));
    }
    dispatch(c, receiver, event, state)?;
    let prevented = flag(c, state, field::PREVENTED)?;
    Ok(c.boolean(!prevented))
}

/// `kHybridDispatch` for a DOM event: walks the type's list, removing `once` records before
/// their call. A listener's throw, or its returned promise's rejection, is reported as an
/// uncaught exception on the next tick. After the first call the event is no longer
/// "being dispatched", as Node clears the flag per listener.
pub(super) fn dispatch(c: &mut Context<'_>, target: RootId, event: RootId, state: RootId) -> Result<(), RootedError> {
    put(c, state, field::TARGET, target)?;
    put_flag(c, state, field::DISPATCHING, true)?;
    let table = receiver_slot(c, target, TARGET_SLOT, "EventTarget")?;
    let kind = get(c, state, field::TYPE)?;
    let Some(root) = list_root(c, table, kind)? else {
        return put_flag(c, state, field::DISPATCHING, false);
    };
    let root_resists = flag(c, root, record::RESIST)?;
    let mut handler = get(c, root, record::NEXT)?;
    while !is_undefined(c, handler) {
        let stopped = flag(c, state, field::STOPPED)?;
        if !(root_resists || flag(c, handler, record::PASSIVE)? || !stopped) {
            break;
        }
        let next = get(c, handler, record::NEXT)?;
        if flag(c, handler, record::REMOVED)? || (stopped && !flag(c, handler, record::RESIST)?) {
            handler = next;
            continue;
        }
        if flag(c, handler, record::ONCE)? {
            remove_record(c, table, kind, root, handler)?;
        }
        let listener = get(c, handler, record::LISTENER)?;
        let passive = flag(c, handler, record::PASSIVE)?;
        put_flag(c, state, field::PASSIVE, passive)?;
        let delivery = invoke(c, target, listener, event);
        put_flag(c, state, field::PASSIVE, false)?;
        put_flag(c, state, field::DISPATCHING, false)?;
        match delivery {
            Ok(result) => report_rejection(c, result)?,
            Err(error) => {
                let Some(reason) = error.exception else { return Err(error) };
                queue_uncaught(c, reason)?;
            }
        }
        handler = next;
    }
    put_flag(c, state, field::DISPATCHING, false)
}

/// A function listener is called with the target as `this`; an object's `handleEvent`, when
/// truthy, with the object.
fn invoke(c: &mut Context<'_>, target: RootId, listener: RootId, event: RootId) -> Result<RootId, RootedError> {
    if c.is_callable_rooted(listener)? {
        return c.call_rooted(listener, target, &[event]);
    }
    let handle = get(c, listener, "handleEvent")?;
    if !c.truthy_rooted(handle)? {
        return Ok(c.undefined());
    }
    c.call_rooted(handle, listener, &[event])
}

/// Node's `addCatch`: a thenable result's rejection becomes an uncaught exception.
fn report_rejection(c: &mut Context<'_>, result: RootId) -> Result<(), RootedError> {
    if is_nullish(c, result) || !is_object_like(c, result)? {
        return Ok(());
    }
    let then = get(c, result, "then")?;
    if c.is_callable_rooted(then)? {
        let report = c.host_function(crate::host::shared_vm::operation("eventTargetQueueError"))?;
        let undefined = c.undefined();
        c.call_rooted(then, result, &[undefined, report])?;
    }
    Ok(())
}

/// `process.nextTick(() => { throw reason; })`: `reason` reaches the uncaught-exception path.
pub(super) fn queue_uncaught(c: &mut Context<'_>, reason: RootId) -> Result<(), RootedError> {
    let callback = c.host_function_named_with_data(crate::host::shared_vm::operation("eventTargetRethrow"), "", reason)?;
    let undefined = c.undefined();
    super::shared_event_loop::queue_next_tick(c, callback, undefined, &[])
}

/// `emitUncaughtException(error)`: throws `error` on the next tick.
pub(crate) fn queue_listener_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let reason = argument(c, args, 0);
    queue_uncaught(c, reason)?;
    Ok(c.undefined())
}

pub(crate) fn rethrow_listener_error(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let reason = c.host_function_data()?;
    Err(c.throw(reason))
}

/// `[util.inspect.custom]`: `EventTarget {}`.
pub(crate) fn event_target_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    target_table(c, receiver)?;
    let body = c.object_rooted()?;
    custom_inspect(c, receiver, body, args, Shallow::Name)
}

/// Adds `listener` for `kind` on EventTarget `target` with `options`' flags, as the
/// public method would; used by host registrations (`events.addAbortListener`).
pub(super) fn add_host_listener(c: &mut Context<'_>, target: RootId, kind: &str, listener: RootId, once: bool, resist: bool) -> Result<(), RootedError> {
    let table = target_table(c, target)?;
    let kind = c.string_rooted(kind);
    let undefined = c.undefined();
    let options = ListenerOptions { once, capture: false, passive: false, resist, signal: undefined };
    add_listener(c, target, table, kind, listener, &options)?;
    Ok(())
}

/// Grows (`delta` 1) or shrinks (-1) `kind`'s listener count without a record, as Node's
/// event handler attribute does when its handler changes between function and not.
pub(super) fn adjust_size(c: &mut Context<'_>, target: RootId, kind: &str, delta: f64) -> Result<(), RootedError> {
    let table = receiver_slot(c, target, TARGET_SLOT, "EventTarget")?;
    let kind_value = c.string_rooted(kind);
    let Some(root) = list_root(c, table, kind_value)? else { return Ok(()) };
    let size = get(c, root, record::SIZE)?;
    let size = c.to_number(size)? + delta;
    let size_value = c.number(size);
    put(c, root, record::SIZE, size_value)?;
    if delta > 0.0 {
        new_listener(c, target, size, kind_value)?;
    }
    Ok(())
}
