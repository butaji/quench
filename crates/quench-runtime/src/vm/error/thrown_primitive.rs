//! A thrown primitive's error, whose message is made only when read: a caught throw of a
//! Number never formats it.
use super::{ErrorDescription, JsError};
use crate::Value;

/// The diagnostic text of a Number, Boolean, `null` or `undefined` (`Vm::detail_string`'s
/// primitive cases, which read nothing else; `JsError::thrown_primitive` admits the same
/// values); `None` for any other value.
pub(in crate::vm) fn primitive_detail(value: Value) -> Option<String> {
    if let Some(number) = value.as_number() {
        return Some(crate::number_to_string::format(number));
    }
    if let Some(boolean) = value.as_bool() {
        return Some(boolean.to_string());
    }
    if value.is_null() {
        return Some("null".into());
    }
    value.is_undefined().then(|| "undefined".into())
}

impl JsError {
    /// The throw of `value` when it is a primitive whose text needs no VM
    /// (`primitive_detail`), with that text made only when read; `None` for any other value.
    pub fn thrown_primitive(value: Value) -> Option<Self> {
        let plain = value.as_number().is_some() || value.as_bool().is_some() || value.is_null() || value.is_undefined();
        plain.then(|| Self::described(ErrorDescription::ThrownPrimitive, Some(value)))
    }

    /// The message of a `ThrownPrimitive` throw.
    pub(super) fn primitive_message(&self) -> String {
        self.0.payload.thrown.and_then(primitive_detail).unwrap_or_default()
    }

    /// Whether a `ThrownPrimitive` throw's message is `text`. A primitive's text holds no
    /// space, so a message with one is not it, and is answered without making it.
    pub(super) fn primitive_message_is(&self, text: &str) -> bool {
        !text.contains(' ') && self.primitive_message() == text
    }
}
