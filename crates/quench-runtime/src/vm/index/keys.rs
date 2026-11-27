//! Property keys that name array indices: the integer form element reads take.
use super::*;

impl Vm {
    /// `key`, or the integer naming the same property when `key` is a flat string holding
    /// an array index in canonical decimal form (`object_static::array_index`), so the
    /// integer-keyed element reads answer `base["0"]` as they answer `base[0]`.
    #[inline(always)]
    pub fn canonical_index_key(&self, key: Value) -> Value {
        let Some(text) = self.heap.string(key) else {
            return key;
        };
        if !may_name_index(text) {
            return key;
        }
        super::object_static::array_index(text.host_string())
            .and_then(|index| i32::try_from(index).ok())
            .map_or(key, Value::integer)
    }

    pub fn array_index_key(&self, key: Value) -> Option<u32> {
        if let Some(index) = key.as_int() {
            return u32::try_from(index).ok();
        }
        // Fallback only: a double-encoded integer key (decision 4.1, R4-M2) normalizes here, never on the
        // int32 arm above, and before any string is made of it.
        let Some(text) = self.heap.string(key) else {
            return key.numeric_key().and_then(|index| u32::try_from(index).ok());
        };
        if !may_name_index(text) {
            return None;
        }
        super::object_static::array_index(text.host_string())
    }
}

/// Whether `text` may spell an array index: only one leading with a digit does. A flat
/// string answers from its first unit, so a name key costs no host conversion.
fn may_name_index(text: &super::wtf16::JsString) -> bool {
    text.existing_flat()
        .and_then(|units| units.unit(0))
        .is_none_or(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(&unit))
}
