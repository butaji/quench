//! NaN-boxed `JSValue` for the shadow-tree interpreter.
//!
//! Encoding:
//! - IEEE-754 doubles that are not quiet-NaN payloads are stored as-is.
//! - `int32` uses the canonical quiet-NaN header `0x7FF8` with the i32 in the
//!   low 32 bits.
//! - Heap references (`ObjectId`, interned strings, well-known symbols) use
//!   distinct quiet-NaN headers and store the 48-bit id in the payload.
//! - Special values (`undefined`, `null`, `true`, `false`, `hole`) share one
//!   header and use a small discriminator.

use std::fmt;

use crate::arena::ObjectId;
use crate::interner::Symbol;

/// A single 64-bit NaN-boxed JavaScript value.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct JSValue(u64);

const TAG_INT32: u64 = 0x7FF8_0000_0000_0000;
const TAG_OBJECT: u64 = 0x7FF9_0000_0000_0000;
const TAG_STRING: u64 = 0x7FFA_0000_0000_0000;
const TAG_SYMBOL: u64 = 0x7FFB_0000_0000_0000;
const TAG_SPECIAL: u64 = 0x7FFC_0000_0000_0000;

const TAG_MASK: u64 = 0x7FFF_0000_0000_0000;
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

const SPECIAL_UNDEFINED: u64 = 0;
const SPECIAL_NULL: u64 = 1;
const SPECIAL_FALSE: u64 = 2;
const SPECIAL_TRUE: u64 = 3;
const SPECIAL_HOLE: u64 = 4;

impl JSValue {
    /// The JavaScript `undefined` value.
    #[inline]
    pub fn undefined() -> Self {
        JSValue(TAG_SPECIAL | SPECIAL_UNDEFINED)
    }

    /// The JavaScript `null` value.
    #[inline]
    pub fn null() -> Self {
        JSValue(TAG_SPECIAL | SPECIAL_NULL)
    }

    /// A JavaScript boolean.
    #[inline]
    pub fn bool(v: bool) -> Self {
        if v {
            JSValue(TAG_SPECIAL | SPECIAL_TRUE)
        } else {
            JSValue(TAG_SPECIAL | SPECIAL_FALSE)
        }
    }

    /// A 32-bit signed integer.
    #[inline]
    pub fn int32(v: i32) -> Self {
        JSValue(TAG_INT32 | (v as u32 as u64))
    }

    /// An IEEE-754 double.
    #[inline]
    pub fn double(v: f64) -> Self {
        let bits = v.to_bits();
        // Quiet NaNs share the same exponent bits as our boxed tags.  To avoid
        // misclassifying a NaN double as a tagged value, canonicalize all quiet
        // NaNs to a payload that does not collide with any reserved tag.
        if bits & 0x7FF8_0000_0000_0000 == 0x7FF8_0000_0000_0000 {
            JSValue(0x7FFF_FFFF_FFFF_FFFF)
        } else {
            JSValue(bits)
        }
    }

    /// A heap object reference.
    #[inline]
    pub fn object(id: ObjectId) -> Self {
        JSValue(TAG_OBJECT | (id.0 as u64 & PAYLOAD_MASK))
    }

    /// An interned string value.
    #[inline]
    pub fn string(sym: Symbol) -> Self {
        JSValue(TAG_STRING | (sym.0 as u64 & PAYLOAD_MASK))
    }

    /// A well-known symbol value.
    #[inline]
    pub fn symbol(sym: Symbol) -> Self {
        JSValue(TAG_SYMBOL | (sym.0 as u64 & PAYLOAD_MASK))
    }

    /// The `hole` sentinel used for deleted array elements.
    #[inline]
    pub fn hole() -> Self {
        JSValue(TAG_SPECIAL | SPECIAL_HOLE)
    }

    #[inline]
    fn tag_bits(&self) -> u64 {
        self.0 & TAG_MASK
    }

    #[inline]
    fn payload(&self) -> u64 {
        self.0 & PAYLOAD_MASK
    }

    #[inline]
    pub fn is_undefined(&self) -> bool {
        self.0 == (TAG_SPECIAL | SPECIAL_UNDEFINED)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.0 == (TAG_SPECIAL | SPECIAL_NULL)
    }

    #[inline]
    pub fn is_bool(&self) -> bool {
        self.tag_bits() == TAG_SPECIAL
            && (self.payload() == SPECIAL_FALSE || self.payload() == SPECIAL_TRUE)
    }

    #[inline]
    pub fn is_true(&self) -> bool {
        self.0 == (TAG_SPECIAL | SPECIAL_TRUE)
    }

    #[inline]
    pub fn is_false(&self) -> bool {
        self.0 == (TAG_SPECIAL | SPECIAL_FALSE)
    }

    #[inline]
    pub fn is_int32(&self) -> bool {
        self.tag_bits() == TAG_INT32
    }

    /// Return the int32 payload. Caller must check `is_int32()` first.
    #[inline]
    pub fn as_int32_unchecked(&self) -> i32 {
        (self.0 as u32) as i32
    }

    /// True for any IEEE-754 double value, including NaNs.
    ///
    /// A value is classified as a double when it does not carry one of the
    /// reserved NaN-box tags used by this module.
    #[inline]
    pub fn is_double(&self) -> bool {
        !self.is_int32()
            && !self.is_object()
            && !self.is_string()
            && !self.is_symbol()
            && !self.is_special()
    }

    /// Return the raw double bits. Caller must check `is_double()` first.
    #[inline]
    pub fn as_double_unchecked(&self) -> f64 {
        f64::from_bits(self.0)
    }

    #[inline]
    pub fn is_object(&self) -> bool {
        self.tag_bits() == TAG_OBJECT
    }

    #[inline]
    pub fn as_object(&self) -> Option<ObjectId> {
        if self.is_object() {
            Some(ObjectId(self.payload() as usize))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_string(&self) -> bool {
        self.tag_bits() == TAG_STRING
    }

    #[inline]
    pub fn as_string(&self) -> Option<Symbol> {
        if self.is_string() {
            Some(Symbol(self.payload() as u32))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_symbol(&self) -> bool {
        self.tag_bits() == TAG_SYMBOL
    }

    #[inline]
    pub fn as_symbol(&self) -> Option<Symbol> {
        if self.is_symbol() {
            Some(Symbol(self.payload() as u32))
        } else {
            None
        }
    }

    #[inline]
    pub fn is_hole(&self) -> bool {
        self.0 == (TAG_SPECIAL | SPECIAL_HOLE)
    }

    #[inline]
    fn is_special(&self) -> bool {
        self.tag_bits() == TAG_SPECIAL
    }
}

impl fmt::Debug for JSValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_int32() {
            write!(f, "JSValue::Int32({})", self.as_int32_unchecked())
        } else if self.is_double() {
            write!(f, "JSValue::Double({})", self.as_double_unchecked())
        } else if self.is_object() {
            write!(f, "JSValue::Object({:?})", self.as_object().unwrap())
        } else if self.is_string() {
            write!(f, "JSValue::String({:?})", self.as_string().unwrap())
        } else if self.is_symbol() {
            write!(f, "JSValue::Symbol({:?})", self.as_symbol().unwrap())
        } else if self.is_undefined() {
            write!(f, "JSValue::Undefined")
        } else if self.is_null() {
            write!(f, "JSValue::Null")
        } else if self.is_true() {
            write!(f, "JSValue::True")
        } else if self.is_false() {
            write!(f, "JSValue::False")
        } else if self.is_hole() {
            write!(f, "JSValue::Hole")
        } else {
            write!(f, "JSValue({:#018x})", self.0)
        }
    }
}

impl fmt::Display for JSValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_int32() {
            write!(f, "{}", self.as_int32_unchecked())
        } else if self.is_double() {
            let v = self.as_double_unchecked();
            if v.is_nan() {
                write!(f, "NaN")
            } else if v == f64::INFINITY {
                write!(f, "Infinity")
            } else if v == f64::NEG_INFINITY {
                write!(f, "-Infinity")
            } else if v.fract() == 0.0 && v.abs() < 1e15 {
                write!(f, "{:.0}", v)
            } else {
                write!(f, "{}", v)
            }
        } else if self.is_object() {
            write!(f, "[object Object]")
        } else if self.is_string() || self.is_symbol() {
            // Without access to the interner we cannot resolve the string here.
            write!(f, "[string]")
        } else if self.is_undefined() {
            write!(f, "undefined")
        } else if self.is_null() {
            write!(f, "null")
        } else if self.is_true() {
            write!(f, "true")
        } else if self.is_false() {
            write!(f, "false")
        } else if self.is_hole() {
            write!(f, "<hole>")
        } else {
            write!(f, "JSValue({:#018x})", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_int32() {
        for v in [0, 1, -1, i32::MAX, i32::MIN, 42, -1234567] {
            let js = JSValue::int32(v);
            assert!(js.is_int32());
            assert!(!js.is_double());
            assert_eq!(js.as_int32_unchecked(), v);
        }
    }

    #[test]
    fn round_trip_double() {
        for v in [0.0, 1.5, -std::f64::consts::PI, f64::NAN, f64::INFINITY, 1e300] {
            let js = JSValue::double(v);
            assert!(js.is_double());
            assert!(!js.is_int32());
            let back = js.as_double_unchecked();
            if v.is_nan() {
                assert!(back.is_nan());
            } else {
                assert_eq!(back, v);
            }
        }
    }

    #[test]
    fn round_trip_object() {
        for id in [0, 1, 42, 10000] {
            let js = JSValue::object(ObjectId(id));
            assert!(js.is_object());
            assert_eq!(js.as_object(), Some(ObjectId(id)));
        }
    }

    #[test]
    fn specials_distinct() {
        let vals = [
            JSValue::undefined(),
            JSValue::null(),
            JSValue::bool(false),
            JSValue::bool(true),
            JSValue::hole(),
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "special values should be distinct");
            }
        }
        assert!(JSValue::undefined().is_undefined());
        assert!(JSValue::null().is_null());
        assert!(JSValue::bool(true).is_true());
        assert!(JSValue::bool(false).is_false());
        assert!(JSValue::hole().is_hole());
    }

    #[test]
    fn int32_not_confused_with_double() {
        let i = JSValue::int32(0);
        assert!(i.is_int32());
        assert!(!i.is_double());

        let d = JSValue::double(0.0);
        assert!(d.is_double());
        assert!(!d.is_int32());
    }
}
