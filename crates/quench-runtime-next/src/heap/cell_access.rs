use super::cell::{Cell, Object};

impl Cell {
    pub(crate) fn typed_array_backing(&self) -> Option<(&Object, crate::Value)> {
        match self {
            Self::TypedArray { object, buffer, .. } => Some((object, *buffer)),
            _ => None,
        }
    }

    pub(crate) fn object(&self) -> Option<&Object> {
        match self {
            Self::Object(object)
            | Self::Array { object, .. }
            | Self::ArrayBuffer { object, .. }
            | Self::TypedArray { object, .. }
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
            | Self::TypedArray { object, .. }
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
