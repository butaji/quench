use super::cell::{Cell, Object};

impl Cell {
    pub(crate) fn typed_array_backing(&self) -> Option<(&Object, crate::Value)> {
        match self {
            Self::Uint8Array { object, buffer, .. }
            | Self::Uint8ClampedArray { object, buffer, .. }
            | Self::Uint16Array { object, buffer, .. }
            | Self::Uint32Array { object, buffer, .. }
            | Self::Int8Array { object, buffer, .. }
            | Self::Int16Array { object, buffer, .. }
            | Self::Int32Array { object, buffer, .. }
            | Self::Float32Array { object, buffer, .. }
            | Self::Float64Array { object, buffer, .. } => Some((object, *buffer)),
            _ => None,
        }
    }

    pub(crate) fn object(&self) -> Option<&Object> {
        match self {
            Self::Object(object)
            | Self::Array { object, .. }
            | Self::ArrayBuffer { object, .. }
            | Self::Uint8Array { object, .. }
            | Self::Uint8ClampedArray { object, .. }
            | Self::Uint16Array { object, .. }
            | Self::Uint32Array { object, .. }
            | Self::Int8Array { object, .. }
            | Self::Int16Array { object, .. }
            | Self::Int32Array { object, .. }
            | Self::Float32Array { object, .. }
            | Self::Float64Array { object, .. }
            | Self::DataView { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::Iterator { object, .. } => Some(object),
            Self::Function { object, .. } => Some(object),
            _ => None,
        }
    }

    pub(crate) fn object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Self::Object(object)
            | Self::Array { object, .. }
            | Self::ArrayBuffer { object, .. }
            | Self::Uint8Array { object, .. }
            | Self::Uint8ClampedArray { object, .. }
            | Self::Uint16Array { object, .. }
            | Self::Uint32Array { object, .. }
            | Self::Int8Array { object, .. }
            | Self::Int16Array { object, .. }
            | Self::Int32Array { object, .. }
            | Self::Float32Array { object, .. }
            | Self::Float64Array { object, .. }
            | Self::DataView { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::Iterator { object, .. } => Some(object),
            Self::Function { object, .. } => Some(object),
            _ => None,
        }
    }
}
