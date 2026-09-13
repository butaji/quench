//! QuickJS-inspired `JSValue`: one fixed-width tag/payload pair. Negative
//! tags are refcounted; the payload is a runtime-local handle or scalar bits.

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
/// Fixed-width tagged value. The tag is the semantic discriminant and the
/// payload is interpreted only by that tag; heap identities remain runtime
/// local handles. This is the documented 16-byte tagged-union view.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JsValue {
    tag: Tag,
    payload: u64,
}

// Keep the representation contract enforced at compile time as well as by
// the dynamic-layer tests.  A future field or alignment change must be
// deliberate because every dynamic register and heap slot depends on this
// width.
const _: () = {
    assert!(std::mem::size_of::<JsValue>() == 16);
    assert!(std::mem::align_of::<JsValue>() == 8);
};

#[allow(non_upper_case_globals)]
impl JsValue {
    pub const Null: Self = Self::new(Tag::Null, 0);
    pub const Undefined: Self = Self::new(Tag::Undefined, 0);
    pub const Uninitialized: Self = Self::new(Tag::Uninitialized, 0);
    pub const Exception: Self = Self::new(Tag::Exception, 0);

    const fn new(tag: Tag, payload: u64) -> Self {
        Self { tag, payload }
    }

    #[allow(non_snake_case)]
    pub const fn Int(value: i32) -> Self {
        Self::new(Tag::Int, value as u32 as u64)
    }

    #[allow(non_snake_case)]
    pub const fn Bool(value: bool) -> Self {
        Self::new(Tag::Bool, value as u64)
    }

    #[allow(non_snake_case)]
    pub const fn CatchOffset(value: i32) -> Self {
        Self::new(Tag::CatchOffset, value as u32 as u64)
    }

    #[allow(non_snake_case)]
    pub const fn ShortBigInt(value: i64) -> Self {
        Self::new(Tag::ShortBigInt, value as u64)
    }

    #[allow(non_snake_case)]
    pub const fn Float64(value: f64) -> Self {
        Self::new(Tag::Float64, value.to_bits())
    }

    /// Heap object. `id` is a Runtime-local handle; RC lives on the object.
    pub const fn ptr(tag: Tag, id: u32) -> Self {
        Self::new(tag, id as u64)
    }

    pub fn pointer(self) -> Option<(Tag, u32)> {
        self.tag
            .has_ref_count()
            .then_some((self.tag, self.payload as u32))
    }

    pub(crate) const fn payload_bits(self) -> u64 {
        self.payload
    }
}

impl std::fmt::Debug for JsValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.tag {
            Tag::Int => formatter
                .debug_tuple("Int")
                .field(&(self.payload as i32))
                .finish(),
            Tag::Bool => formatter
                .debug_tuple("Bool")
                .field(&(self.payload != 0))
                .finish(),
            Tag::CatchOffset => formatter
                .debug_tuple("CatchOffset")
                .field(&(self.payload as i32))
                .finish(),
            Tag::ShortBigInt => formatter
                .debug_tuple("ShortBigInt")
                .field(&(self.payload as i64))
                .finish(),
            Tag::Float64 => formatter
                .debug_tuple("Float64")
                .field(&f64::from_bits(self.payload))
                .finish(),
            tag if tag.has_ref_count() => formatter
                .debug_struct("Ptr")
                .field("tag", &tag)
                .field("id", &(self.payload as u32))
                .finish(),
            Tag::Null => formatter.write_str("Null"),
            Tag::Undefined => formatter.write_str("Undefined"),
            Tag::Uninitialized => formatter.write_str("Uninitialized"),
            Tag::Exception => formatter.write_str("Exception"),
            tag => formatter.debug_tuple("Tagged").field(&tag).finish(),
        }
    }
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        if self.tag != other.tag {
            return false;
        }
        if self.tag == Tag::Float64 {
            return f64::from_bits(self.payload) == f64::from_bits(other.payload);
        }
        self.payload == other.payload
    }
}

impl std::hash::Hash for JsValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.tag.hash(state);
        let payload = if self.tag == Tag::Float64 {
            let value = f64::from_bits(self.payload);
            if value == 0.0 {
                0
            } else if value.is_nan() {
                f64::NAN.to_bits()
            } else {
                self.payload
            }
        } else {
            self.payload
        };
        payload.hash(state);
    }
}

impl JsValue {
    pub fn tag(&self) -> Tag {
        self.tag
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
        matches!(self.tag, Tag::Int | Tag::Float64)
    }

    pub fn as_i32(&self) -> Option<i32> {
        (self.tag == Tag::Int).then_some(self.payload as i32)
    }

    pub fn as_i64(&self) -> Option<i64> {
        (self.tag == Tag::ShortBigInt).then_some(self.payload as i64)
    }

    pub fn as_bool(&self) -> Option<bool> {
        (self.tag == Tag::Bool).then_some(self.payload != 0)
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self.tag {
            Tag::Int => Some(self.payload as i32 as f64),
            Tag::Float64 => Some(f64::from_bits(self.payload)),
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
    fn representation_is_one_fixed_width_tagged_union() {
        assert_eq!(std::mem::size_of::<JsValue>(), 16);
        assert_eq!(std::mem::align_of::<JsValue>(), 8);
    }

    #[test]
    fn payload_bits_preserve_nan_and_pointer_identity() {
        let nan_a = JsValue::Float64(f64::from_bits(0x7ff8_0000_0000_0001));
        let nan_b = JsValue::Float64(f64::from_bits(0x7ff8_0000_0000_0002));
        assert_ne!(nan_a, nan_b);
        let pointer = JsValue::ptr(Tag::Object, 41);
        assert_eq!(pointer.pointer(), Some((Tag::Object, 41)));
        assert_eq!(pointer, JsValue::ptr(Tag::Object, 41));
    }

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
