use super::*;

/// Room for `[object <brand>]` of every builtin brand (`builtin_brand_string`).
const BRAND_TEXT_CAPACITY: usize = 48;

impl Vm {
    pub(super) fn is_string(&self, v: Value) -> bool {
        self.heap.is_string(v)
    }

    /// The ECMAScript language type of `value`, with callable objects distinguished.
    #[inline]
    pub(crate) fn value_type(&self, value: Value) -> crate::ValueType {
        use crate::ValueType as T;
        if let Some(kind) = immediate_value_type(value) {
            kind
        } else if self.heap.is_string(value) {
            T::String
        } else if value.direct_function_index().is_some() {
            // A function record is callable; the other direct object families are not.
            T::Function
        } else if value.is_direct_object_family() {
            T::Object
        } else {
            match self.heap.get(value) {
                Some(Cell::BigInt(_)) => T::BigInt,
                _ if self.heap.is_symbol(value) => T::Symbol,
                _ if self.is_function(value) => T::Function,
                _ => T::Object,
            }
        }
    }

    /// `[object <brand>]` (`object_prototype_to_string`): of a builtin brand or a constant
    /// `@@toStringTag`, one of a few texts, so its atom's string, shared as strings may be,
    /// without formatting a new one.
    pub(super) fn builtin_brand_string(&mut self, brand: &str) -> Value {
        let (prefix, suffix) = (b"[object ", b"]");
        let length = prefix.len() + brand.len() + suffix.len();
        if length > BRAND_TEXT_CAPACITY {
            return self.heap.alloc_string(format!("[object {brand}]").into());
        }
        let mut text = [0u8; BRAND_TEXT_CAPACITY];
        text[..prefix.len()].copy_from_slice(prefix);
        text[prefix.len()..prefix.len() + brand.len()].copy_from_slice(brand.as_bytes());
        text[length - suffix.len()..length].copy_from_slice(suffix);
        let atom = self.intern_atom(std::str::from_utf8(&text[..length]).unwrap_or_default());
        self.atom_string(atom)
    }
}

#[inline]
pub(super) fn immediate_value_type(value: Value) -> Option<crate::ValueType> {
    use crate::ValueType as T;
    Some(if value.is_undefined() || value.is_deleted() {
        T::Undefined
    } else if value.is_null() {
        T::Null
    } else if value.as_bool().is_some() {
        T::Boolean
    } else if value.as_number().is_some() {
        T::Number
    } else {
        return None;
    })
}
