//! JS layer on the Wasm VM: QuickJS facts, Native | Fast | Dynamic climb.
//!
//! The Wasm store is Native|Fast|Dynamic with Arena|GC. This module is the
//! slower JS layer: JSValue, atoms, shapes, Runtime/Context, RC+cycle, stack
//! bytecode. It is not the Wasm GC heap.

mod atom;
mod jsvalue;
mod opcode;
mod rt;
mod shape;

pub use atom::{Atom, AtomTable};
pub use jsvalue::{JsValue, Tag};
pub use opcode::{Bytecode, Op, StackUse};
pub use rt::{Context, GcHeader, JsString, Object, Runtime};
pub use shape::{
    Shape, ShapeId, ShapeTable, PROP_CONFIGURABLE, PROP_CWE, PROP_ENUMERABLE, PROP_WRITABLE,
};

/// Dynamic payload. One representation: a QuickJS JSValue. Not `value::Value`.
#[derive(Clone, Debug, PartialEq)]
pub struct Dynamic {
    inner: JsValue,
}

impl Dynamic {
    pub fn from_js(inner: JsValue) -> Self {
        Self { inner }
    }

    pub fn from_number(value: f64) -> Self {
        Self {
            inner: JsValue::from_number(value),
        }
    }

    pub fn undefined() -> Self {
        Self {
            inner: JsValue::Undefined,
        }
    }

    pub fn as_js(&self) -> &JsValue {
        &self.inner
    }

    pub fn as_number(&self) -> Option<f64> {
        self.inner.as_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::{Dynamic, Tag};

    #[test]
    fn number_round_trip() {
        assert_eq!(Dynamic::from_number(3.0).as_number(), Some(3.0));
        assert_eq!(Dynamic::from_number(3.0).as_js().tag(), Tag::Int);
        assert_eq!(Dynamic::undefined().as_number(), None);
    }
}
