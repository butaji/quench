//! Component `Props` — the bag of inputs passed to a
//! root component.
//!
//! In Ink, the `render(element, props)` entry point accepts
//! a `Props` object that the renderer forwards to the root
//! component on every re-render. The shape of `Props` is
//! application-defined; runts-ink just carries a
//! `serde_json::Value` so the renderer can serialise /
//! deserialise props across FFI or the HIR boundary.
//!
//! In practice most runts-ink components don't use props
//! (the component is a function that takes no args, the
//! `useState`/`useEffect` hooks carry their own state).
//! But props are still useful for: rendering the same
//! component twice with different inputs, threading
//! configuration (e.g. an environment) from the host
//! process into the TUI, and writing testable components.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Properties passed to a root component.
///
/// `Props` is a typed wrapper around a JSON object. The
/// `T` parameter is the type the root component expects
/// to receive; it's an associated type alias so a
/// `Props<MyConfig>` can decode `MyConfig` directly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Props {
    /// The JSON object holding the props. Empty by default.
    inner: JsonValue,
}

impl Props {
    /// An empty props bag.
    pub fn new() -> Self {
        Self { inner: JsonValue::Null }
    }

    /// A props bag built from a `serde_json::Value`.
    pub fn from_json(value: JsonValue) -> Self {
        Self { inner: value }
    }

    /// A props bag built from any `Serialize` value.
    pub fn from_serialize<T: Serialize>(value: T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            inner: serde_json::to_value(value)?,
        })
    }

    /// Decode the props as a `T`. Useful for typed root
    /// components.
    pub fn decode<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.inner.clone())
    }

    /// The raw JSON value.
    pub fn raw(&self) -> &JsonValue {
        &self.inner
    }

    /// Set a top-level key.
    pub fn with<K: Into<String>, V: Serialize>(mut self, key: K, value: V) -> Self {
        if !self.inner.is_object() {
            self.inner = serde_json::json!({});
        }
        if let Some(obj) = self.inner.as_object_mut() {
            obj.insert(key.into(), serde_json::to_value(value).unwrap_or(JsonValue::Null));
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct MyConfig {
        name: String,
        count: u32,
    }

    #[test]
    fn empty_props_default_to_null() {
        let p = Props::new();
        assert_eq!(p.raw(), &JsonValue::Null);
    }

    #[test]
    fn from_serialize_round_trips() {
        let cfg = MyConfig { name: "alice".into(), count: 3 };
        let p = Props::from_serialize(cfg.clone()).unwrap();
        assert_eq!(p.decode::<MyConfig>().unwrap(), cfg);
    }

    #[test]
    fn with_key_appends_to_an_object() {
        let p = Props::new().with("a", 1).with("b", "two");
        let obj = p.raw().as_object().expect("object");
        assert_eq!(obj.get("a"), Some(&serde_json::json!(1)));
        assert_eq!(obj.get("b"), Some(&serde_json::json!("two")));
    }

    #[test]
    fn with_key_replaces_null_with_object() {
        // Props::new() is JsonValue::Null; calling `with`
        // upgrades it to a JSON object.
        let p = Props::new().with("a", 1);
        assert!(p.raw().is_object());
    }
}
