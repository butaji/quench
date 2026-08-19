//! Canonical host-side object envelope.
//!
//! Every Node API object is a `NodeObject<T>` — a Rust-side record
//! that is exposed to JavaScript through the runtime's ordinary
//! object semantics. The envelope carries the Rust state and a
//! `Value::Object` whose identity is preserved across host calls.
//!
//! The envelope is the only place host objects live. Constructors
//! return a fresh `Value::Object` per call; receivers are recovered
//! from the runtime's `Value` via the `NodeAny` trait object.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::value::Value;

/// Trait every host-side Node type implements. Provides a single
/// canonical `as_any`/`as_any_mut` so callers can recover the
/// concrete type from a `dyn NodeAny`.
pub trait NodeAny: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> NodeAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// One Node host object. Holds the Rust state and the JS-visible
/// `Value`. The `Value` is constructed via `host_api::object` so
/// the runtime's heap owns the JS-shape; we never poke the
/// `ObjectData` private fields directly.
#[derive(Clone)]
pub struct NodeObject<T: NodeAny> {
    state: Rc<RefCell<T>>,
    value: Value,
}

impl<T: NodeAny> NodeObject<T> {
    pub fn new_with(state: T, value: Value) -> Self {
        let state = Rc::new(RefCell::new(state));
        Self { state, value }
    }

    pub fn value(&self) -> Value {
        self.value.clone()
    }

    pub fn state(&self) -> Rc<RefCell<T>> {
        Rc::clone(&self.state)
    }
}

impl<T: NodeAny> PartialEq for NodeObject<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl<T: NodeAny> Eq for NodeObject<T> {}

/// Convenience wrapper for shared weakly-typed host state.
pub type NodeShared = Rc<RefCell<dyn Any>>;
