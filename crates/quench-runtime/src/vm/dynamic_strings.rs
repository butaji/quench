use super::wtf16::{str_text_hash, TextHash, Units};
use super::*;

impl<H: Host> Vm<H> {
    /// The interned string value of ASCII `text`, allocated only when no live one exists.
    pub(super) fn intern_dynamic_ascii(&mut self, text: &str) -> Value {
        debug_assert!(text.is_ascii());
        let hash = str_text_hash(text);
        if let Some(value) = self.dynamic_string(hash)
            && self.heap.string(value).is_some_and(|candidate| {
                matches!(candidate.units(), Units::Ascii(bytes) if bytes == text.as_bytes())
            })
        {
            return value;
        }
        self.insert_dynamic_string(JsString::from_str(text), hash)
    }

    pub(super) fn intern_dynamic_value(&mut self, text: JsString) -> Value {
        let hash = text.units().text_hash();
        if let Some(value) = self.dynamic_string(hash)
            && self.heap.string(value).is_some_and(|candidate| candidate == &text)
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.dynamic_string(true);
            return value;
        }
        self.insert_dynamic_string(text, hash)
    }

    /// The live interned string last made with text hash `hash`.
    fn dynamic_string(&self, hash: TextHash) -> Option<Value> {
        self.dynamic_strings.as_ref()?.get(&u64::from(hash)).copied()
    }

    /// Allocates and interns `text`, whose text hash is `hash`; the record keeps the hash
    /// unless its text is inline, whose hash is made on each use.
    fn insert_dynamic_string(&mut self, text: JsString, hash: TextHash) -> Value {
        #[cfg(feature = "profile-aggregate")]
        self.profile.dynamic_string(false);
        let kept = !text.hashes_inline();
        let value = self.heap.alloc_string(text);
        if kept {
            self.heap.remember_string_hash(value, hash);
        }
        self.dynamic_strings
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
            .insert(u64::from(hash), value);
        value
    }
}
