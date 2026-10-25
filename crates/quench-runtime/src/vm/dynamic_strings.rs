use super::*;

impl<H: Host> Vm<H> {
    /// The interned string value of ASCII `text`, allocated only when no live one exists.
    pub(super) fn intern_dynamic_ascii(&mut self, text: &str) -> Value {
        debug_assert!(text.is_ascii());
        let mut hasher = rustc_hash::FxHasher::default();
        // An ASCII string hashes as its bytes (`JsString`'s canonical one-byte storage).
        text.as_bytes().hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(value) = self
            .dynamic_strings
            .as_ref()
            .and_then(|strings| strings.get(&hash))
            .copied()
            && self.heap.string(value).is_some_and(|candidate| {
                matches!(candidate.units(), super::wtf16::Units::Ascii(bytes) if bytes == text.as_bytes())
            })
        {
            return value;
        }
        self.intern_dynamic_value(JsString::from_str(text))
    }

    pub(super) fn intern_dynamic_value(&mut self, text: JsString) -> Value {
        let mut hasher = rustc_hash::FxHasher::default();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(value) = self
            .dynamic_strings
            .as_ref()
            .and_then(|strings| strings.get(&hash))
            .copied()
            && self
                .heap
                .string(value)
                .is_some_and(|candidate| candidate == &text)
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.dynamic_string(true);
            return value;
        }
        #[cfg(feature = "profile-aggregate")]
        self.profile.dynamic_string(false);
        let value = self.heap.alloc_string(text);
        self.dynamic_strings
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
            .insert(hash, value);
        value
    }
}
