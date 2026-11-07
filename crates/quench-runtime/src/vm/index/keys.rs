//! Property keys that name array indices: the integer form element reads take.
use super::*;

impl<H: Host> Vm<H> {
    /// `key`, or the integer naming the same property when `key` is a flat string holding
    /// an array index in canonical decimal form (`object_static::array_index`), so the
    /// integer-keyed element reads answer `base["0"]` as they answer `base[0]`.
    #[inline(always)]
    pub(in crate::vm) fn canonical_index_key(&self, key: Value) -> Value {
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

    pub(in crate::vm) fn array_index_key(&self, key: Value) -> Option<u32> {
        if let Some(index) = key.as_int() {
            return u32::try_from(index).ok();
        }
        let key = self.heap.string(key).filter(|text| may_name_index(text))?;
        super::object_static::array_index(key.host_string())
    }
}

/// Whether `text` may spell an array index: only one leading with a digit does. A flat
/// string answers from its first unit, so a name key costs no host conversion.
fn may_name_index(text: &super::wtf16::JsString) -> bool {
    text.existing_flat()
        .and_then(|units| units.unit(0))
        .is_none_or(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(&unit))
}
