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
            // The hot object kinds keep their object inline; the rest box it so the
            // cell stays small.
            Self::Object(object) | Self::Array { object, .. } => Some(object),
            Self::ArrayBuffer { object, .. }
            | Self::TypedArray { object, .. }
            | Self::DataView { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::ShadowRealm { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::FinalizationRegistry { object, .. }
            | Self::Iterator { object, .. }
            | Self::Date { object, .. }
            | Self::RegExp { object, .. }
            | Self::Proxy { object, .. }
            | Self::Function { object, .. }
            | Self::TemporalDuration { object, .. }
            | Self::TemporalPlainDate { object, .. }
            | Self::TemporalPlainDateTime { object, .. }
            | Self::TemporalPlainMonthDay { object, .. }
            | Self::TemporalPlainYearMonth { object, .. }
            | Self::TemporalZonedDateTime { object, .. }
            | Self::TemporalInstant { object, .. } => Some(object),
            _ => None,
        }
    }

    pub(crate) fn object_mut(&mut self) -> Option<&mut Object> {
        match self {
            // The hot object kinds keep their object inline; the rest box it so the
            // cell stays small.
            Self::Object(object) | Self::Array { object, .. } => Some(object),
            Self::ArrayBuffer { object, .. }
            | Self::TypedArray { object, .. }
            | Self::DataView { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::ShadowRealm { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::FinalizationRegistry { object, .. }
            | Self::Iterator { object, .. }
            | Self::Date { object, .. }
            | Self::RegExp { object, .. }
            | Self::Proxy { object, .. }
            | Self::Function { object, .. }
            | Self::TemporalDuration { object, .. }
            | Self::TemporalPlainDate { object, .. }
            | Self::TemporalPlainDateTime { object, .. }
            | Self::TemporalPlainMonthDay { object, .. }
            | Self::TemporalPlainYearMonth { object, .. }
            | Self::TemporalZonedDateTime { object, .. }
            | Self::TemporalInstant { object, .. } => Some(object),
            _ => None,
        }
    }
}
