//! The WebIDL `DOMException` class (`lib/internal/per_context/domexception.js`).
use super::event_target_shared_vm::{
    api, argument, attach_slot, get, install_members, install_tag_and_inspect, invalid_this, put, require_new, slot,
    to_dom_string, Member,
};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

pub(super) const SLOT: &str = "domExceptionSlot";
const NAME: &str = "name";
const MESSAGE: &str = "message";
const DEFAULT_NAME: &str = "Error";
/// The receiver error of the getters, with Node's wording.
const FOREIGN_RECEIVER: &str = "Value of \"this\" must be of DOMException";

/// The legacy error names with a code, in code order from 1, and their constant names.
const LEGACY_CODES: [(&str, &str); 25] = [
    ("IndexSizeError", "INDEX_SIZE_ERR"),
    ("DOMStringSizeError", "DOMSTRING_SIZE_ERR"),
    ("HierarchyRequestError", "HIERARCHY_REQUEST_ERR"),
    ("WrongDocumentError", "WRONG_DOCUMENT_ERR"),
    ("InvalidCharacterError", "INVALID_CHARACTER_ERR"),
    ("NoDataAllowedError", "NO_DATA_ALLOWED_ERR"),
    ("NoModificationAllowedError", "NO_MODIFICATION_ALLOWED_ERR"),
    ("NotFoundError", "NOT_FOUND_ERR"),
    ("NotSupportedError", "NOT_SUPPORTED_ERR"),
    ("InUseAttributeError", "INUSE_ATTRIBUTE_ERR"),
    ("InvalidStateError", "INVALID_STATE_ERR"),
    ("SyntaxError", "SYNTAX_ERR"),
    ("InvalidModificationError", "INVALID_MODIFICATION_ERR"),
    ("NamespaceError", "NAMESPACE_ERR"),
    ("InvalidAccessError", "INVALID_ACCESS_ERR"),
    ("ValidationError", "VALIDATION_ERR"),
    ("TypeMismatchError", "TYPE_MISMATCH_ERR"),
    ("SecurityError", "SECURITY_ERR"),
    ("NetworkError", "NETWORK_ERR"),
    ("AbortError", "ABORT_ERR"),
    ("URLMismatchError", "URL_MISMATCH_ERR"),
    ("QuotaExceededError", "QUOTA_EXCEEDED_ERR"),
    ("TimeoutError", "TIMEOUT_ERR"),
    ("InvalidNodeTypeError", "INVALID_NODE_TYPE_ERR"),
    ("DataCloneError", "DATA_CLONE_ERR"),
];
/// A name outside `LEGACY_CODES` has this code.
const NO_LEGACY_CODE: f64 = 0.0;

fn legacy_code(name: &str) -> f64 {
    LEGACY_CODES.iter().position(|(legacy, _)| *legacy == name).map_or(NO_LEGACY_CODE, |index| (index + 1) as f64)
}

/// What a getter reads (its function data).
#[derive(Clone, Copy)]
#[repr(u8)]
enum View {
    Name,
    Message,
    Code,
}

impl View {
    const ALL: [View; 3] = [View::Name, View::Message, View::Code];
}

const MEMBERS: [Member; 3] = [
    Member::Accessor(NAME, "domExceptionGet", None, View::Name as u8 as f64),
    Member::Accessor(MESSAGE, "domExceptionGet", None, View::Message as u8 as f64),
    Member::Accessor("code", "domExceptionGet", None, View::Code as u8 as f64),
];

/// The class, built once into the event group's data.
pub(crate) fn class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let data = api(c)?;
    let cached = get(c, data, "domExceptionCtor")?;
    if c.is_callable_rooted(cached)? {
        return Ok(cached);
    }
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("domExceptionConstructor"), "DOMException")?;
    let prototype = get(c, constructor, "prototype")?;
    let global = c.global_root()?;
    let error = get(c, global, "Error")?;
    let error_prototype = get(c, error, "prototype")?;
    c.set_prototype_rooted(prototype, error_prototype)?;
    install_members(c, prototype, &MEMBERS, true)?;
    for holder in [constructor, prototype] {
        for (index, (_, constant)) in LEGACY_CODES.iter().enumerate() {
            let value = c.number((index + 1) as f64);
            c.define_named_data_property_rooted(holder, constant, value, false, true, false)?;
        }
    }
    install_tag_and_inspect(c, prototype, "DOMException", None)?;
    put(c, data, "domExceptionCtor", constructor)?;
    Ok(constructor)
}

/// `new DOMException(message, name)`.
pub(crate) fn create(c: &mut Context<'_>, message: RootId, name: RootId) -> Result<RootId, RootedError> {
    let constructor = class(c)?;
    c.construct_rooted(constructor, constructor, &[message, name])
}

/// `new DOMException(message = '', options = 'Error')`: an options object gives `name` and
/// `cause`, anything else is the name.
pub(crate) fn constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "DOMException")?;
    let message = argument(c, args, 0);
    let message = if c.rooted_value(message).is_some_and(|value| value.is_undefined()) {
        c.string_rooted("")
    } else {
        to_dom_string(c, message)?
    };
    let options = argument(c, args, 1);
    let name = if c.rooted_value(options).is_some_and(|value| value.is_undefined()) {
        c.string_rooted(DEFAULT_NAME)
    } else if c.is_object_rooted(options)? && !c.is_callable_rooted(options)? {
        let cause_key = c.string_rooted("cause");
        if c.has_property_rooted(options, cause_key)? {
            let cause = c.get_property_rooted(options, cause_key)?;
            c.define_data_property_rooted(receiver, cause_key, cause, true, false, true)?;
        }
        let name = get(c, options, NAME)?;
        to_dom_string(c, name)?
    } else {
        to_dom_string(c, options)?
    };
    let state = c.null_object_rooted()?;
    put(c, state, NAME, name)?;
    put(c, state, MESSAGE, message)?;
    attach_slot(c, receiver, SLOT, state)?;
    let global = c.global_root()?;
    let error = get(c, global, "Error")?;
    let capture = get(c, error, "captureStackTrace")?;
    c.call_rooted(capture, error, &[receiver])?;
    Ok(c.undefined())
}

/// The `name`, `message` and `code` getters.
pub(crate) fn getter(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let Some(state) = slot(c, receiver, SLOT)? else {
        return Err(invalid_this(c, FOREIGN_RECEIVER));
    };
    let view = c.host_function_data()?;
    match View::ALL[c.to_number(view)? as usize] {
        View::Name => get(c, state, NAME),
        View::Message => get(c, state, MESSAGE),
        View::Code => {
            let name = get(c, state, NAME)?;
            let name = c.to_string(name)?;
            Ok(c.number(legacy_code(&name)))
        }
    }
}

/// The lazy global's initializer.
pub(crate) fn initialize(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let constructor = class(c)?;
    let global = c.global_root()?;
    let key = c.string_rooted("DOMException");
    c.define_data_property_rooted(global, key, constructor, true, false, true)?;
    Ok(c.undefined())
}
