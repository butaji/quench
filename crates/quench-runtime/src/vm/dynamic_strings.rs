use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn intern_dynamic_value(&mut self, text: JsString) -> Value {
        let mut hasher = rustc_hash::FxHasher::default();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(value) = self
            .dynamic_strings
            .as_ref()
            .and_then(|strings| strings.get(&hash))
            .copied()
            && matches!(self.heap.get(value), Some(Cell::String(candidate)) if candidate == &text)
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.dynamic_string(true);
            return value;
        }
        #[cfg(feature = "profile-aggregate")]
        self.profile.dynamic_string(false);
        let value = self.heap.alloc(Cell::String(text));
        self.dynamic_strings
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
            .insert(hash, value);
        value
    }
}
