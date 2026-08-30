//! QuickJS `JSValue`: one tagged word. Negative tags are refcounted.

/// QuickJS tags from `quickjs.h`. Refcounted iff `tag < 0`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(i8)]
pub enum Tag {
    BigInt = -9,
    Symbol = -8,
    String = -7,
    StringRope = -6,
    Module = -3,
    FunctionBytecode = -2,
    Object = -1,
    Int = 0,
    Bool = 1,
    Null = 2,
    Undefined = 3,
    Uninitialized = 4,
    CatchOffset = 5,
    Exception = 6,
    ShortBigInt = 7,
    Float64 = 8,
}

impl Tag {
    pub fn has_ref_count(self) -> bool {
        (self as i8) < 0
    }
}

/// Canonical Dynamic payload. Matches QuickJS: INT fast path, heap for the rest.
#[derive(Clone, Debug, PartialEq)]
pub enum JsValue {
    Int(i32),
    Bool(bool),
    Null,
    Undefined,
    Uninitialized,
    Exception,
    CatchOffset(i32),
    ShortBigInt(i64),
    Float64(f64),
    /// Heap object. `id` is a Runtime-local handle; RC lives on the object.
    Ptr { tag: Tag, id: u32 },
}

impl JsValue {
    pub fn tag(&self) -> Tag {
        match self {
            Self::Int(_) => Tag::Int,
            Self::Bool(_) => Tag::Bool,
            Self::Null => Tag::Null,
            Self::Undefined => Tag::Undefined,
            Self::Uninitialized => Tag::Uninitialized,
            Self::Exception => Tag::Exception,
            Self::CatchOffset(_) => Tag::CatchOffset,
            Self::ShortBigInt(_) => Tag::ShortBigInt,
            Self::Float64(_) => Tag::Float64,
            Self::Ptr { tag, .. } => *tag,
        }
    }

    /// QuickJS `JS_NewFloat64`: INT when the bits are a 32-bit integer, else FLOAT64.
    pub fn from_number(d: f64) -> Self {
        if d >= i32::MIN as f64 && d <= i32::MAX as f64 {
            let val = d as i32;
            if (val as f64).to_bits() == d.to_bits() {
                return Self::Int(val);
            }
        }
        Self::Float64(d)
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Self::Int(_) | Self::Float64(_))
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(v) => Some(*v as f64),
            Self::Float64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn both_int(a: &Self, b: &Self) -> bool {
        a.tag() as i8 | b.tag() as i8 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::{JsValue, Tag};

    #[test]
    fn int_fast_path_matches_quickjs() {
        let v = JsValue::from_number(7.0);
        assert_eq!(v.tag(), Tag::Int);
        assert_eq!(v.as_i32(), Some(7));
        assert!(JsValue::both_int(&v, &JsValue::Int(1)));
    }

    #[test]
    fn minus_zero_is_float() {
        let v = JsValue::from_number(-0.0);
        assert_eq!(v.tag(), Tag::Float64);
    }

    #[test]
    fn heap_tags_are_refcounted() {
        assert!(Tag::Object.has_ref_count());
        assert!(Tag::String.has_ref_count());
        assert!(!Tag::Int.has_ref_count());
        assert!(!Tag::Float64.has_ref_count());
    }
}
