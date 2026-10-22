//! Private-name identity carries immutable declaring-class diagnostics across eval.
//! A source path can contain colons; diagnostic identifiers cannot, so decode
//! the fixed suffix from the right rather than interpreting the source path.
pub(crate) const PREFIX: &str = "\0quench:private:";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Receiver {
    Field,
    Instance(String),
    Static(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Name {
    pub(crate) id: u32,
    pub(crate) label: String,
    pub(crate) receiver: Receiver,
}
impl Name {
    pub(crate) fn storage(id: u32) -> Self {
        Self { id, label: String::new(), receiver: Receiver::Field }
    }
    pub(crate) fn identity(&self, source: &str) -> String {
        let (kind, class) = match &self.receiver {
            Receiver::Field => ("field", ""),
            Receiver::Instance(class) => ("instance", class.as_str()),
            Receiver::Static(class) => ("static", class.as_str()),
        };
        format!("{PREFIX}{source}:{}:{kind}:{class}:{}", self.id, self.label)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiverRef<'a> {
    Field,
    Instance(&'a str),
    Static(&'a str),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Diagnostic<'a> {
    pub(crate) label: &'a str,
    pub(crate) receiver: ReceiverRef<'a>,
}
pub(crate) fn diagnostic(identity: &str) -> Option<Diagnostic<'_>> {
    let text = identity.strip_prefix(PREFIX)?;
    let mut parts = text.rsplitn(5, ':');
    let label = parts.next()?;
    let class = parts.next()?;
    let kind = parts.next()?;
    let id = parts.next()?;
    let _source = parts.next()?;
    id.parse::<u32>().ok()?;
    let receiver = match (kind, class) {
        ("field", "") => ReceiverRef::Field,
        ("instance", class) if !class.is_empty() => ReceiverRef::Instance(class),
        ("static", class) if !class.is_empty() => ReceiverRef::Static(class),
        _ => return None,
    };
    Some(Diagnostic { label, receiver })
}
/// Eval aliases also accept names compiled before diagnostic suffixes existed.
pub(crate) fn label(identity: &str) -> Option<&str> {
    let text = identity.strip_prefix(PREFIX)?;
    let (_, label) = text.rsplit_once(':')?;
    (!label.is_empty()).then_some(label)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_preserves_source_colons_and_lexical_receiver_diagnostics() {
        for receiver in [Receiver::Field, Receiver::Instance("Named".into()), Receiver::Static("匿名".into())] {
            let name = Name { id: 17, label: "value".into(), receiver };
            let identity = name.identity("file:///tmp/source:with:colons.js");
            assert_eq!(label(&identity), Some("value"));
            let parsed = diagnostic(&identity).unwrap();
            assert_eq!(parsed.label, "value");
            assert_eq!(parsed.receiver, match &name.receiver {
                Receiver::Field => ReceiverRef::Field,
                Receiver::Instance(class) => ReceiverRef::Instance(class),
                Receiver::Static(class) => ReceiverRef::Static(class),
            });
        }
    }
    #[test]
    fn hidden_storage_and_legacy_eval_aliases_keep_distinct_visibility() {
        let storage = Name::storage(1).identity("source");
        assert_eq!(label(&storage), None);
        assert!(diagnostic(&storage).is_some());
        assert_eq!(label("\0quench:private:old:source.js:3:field"), Some("field"));
        assert!(diagnostic("\0quench:private:old:source.js:3:field").is_none());
        assert!(label("public").is_none());
        assert!(diagnostic("\0quench:private:source:bad:instance:Class:value").is_none());
    }
}
