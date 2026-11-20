use std::{collections::{BTreeMap, BTreeSet}, ops::Range};

/// Exact UTF-8 inputs, shared across source identities, diagnostics and loader assets.
/// Longer texts own contained inputs so wrappers and their original bodies share bytes.
pub(super) struct SourcePool<'a> {
    text: String,
    ranges: BTreeMap<&'a str, Range<usize>>,
}

impl<'a> SourcePool<'a> {
    pub(super) fn new(inputs: impl IntoIterator<Item = &'a str>) -> Self {
        let mut inputs: Vec<_> = inputs.into_iter().collect::<BTreeSet<_>>().into_iter().collect();
        inputs.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        let mut pool = Self { text: String::new(), ranges: BTreeMap::new() };
        for text in inputs {
            let start = pool.text.find(text).unwrap_or_else(|| {
                let start = pool.text.len();
                pool.text.push_str(text);
                start
            });
            pool.ranges.insert(text, start..start + text.len());
        }
        pool
    }

    pub(super) fn text(&self) -> &str { &self.text }

    pub(super) fn reference(&self, text: &str) -> String {
        let range = self.ranges.get(text).expect("native source collected before emission");
        format!("quench_native_source_text({}, {})", range.start, range.len())
    }
}

#[cfg(test)]
mod tests {
    use super::SourcePool;

    #[test]
    fn source_identity_and_assets_share_exact_unicode_and_wrapped_bytes() {
        let body = "const text = 'λ\0';\n";
        let wrapped = format!("function wrapper() {{\n{body}}}");
        let first = SourcePool::new([body, wrapped.as_str(), body, ""]);
        let second = SourcePool::new(["", body, wrapped.as_str()]);
        assert_eq!(first.text(), wrapped);
        assert_eq!(first.text(), second.text());
        assert_eq!(first.reference(body), second.reference(body));
        for text in [body, wrapped.as_str(), ""] {
            let range = first.ranges.get(text).unwrap();
            assert_eq!(&first.text()[range.clone()], text);
        }
    }
}
