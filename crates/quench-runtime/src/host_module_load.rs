//! Why a host could not load a module, as the module graph reports it.

/// The error constructor a host's module-load failure is reported with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleLoadErrorKind {
    Error,
    TypeError,
}

/// A host's failure to resolve or load a module, thrown as `kind` with `message` and,
/// when present, a `code` property (as embedders such as Node identify their errors)
/// followed by any further string `fields`, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleLoadError {
    pub kind: ModuleLoadErrorKind,
    pub code: Option<&'static str>,
    pub message: String,
    pub fields: Vec<(&'static str, String)>,
}

impl ModuleLoadError {
    pub fn new(kind: ModuleLoadErrorKind, code: &'static str, message: impl Into<String>) -> Self {
        Self { kind, code: Some(code), message: message.into(), fields: Vec::new() }
    }

    /// Add an own string property after `code`.
    #[must_use]
    pub fn with_field(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.fields.push((name, value.into()));
        self
    }
}

/// An uncoded `TypeError`, the report of a host without its own error identities.
impl From<String> for ModuleLoadError {
    fn from(message: String) -> Self {
        Self { kind: ModuleLoadErrorKind::TypeError, code: None, message, fields: Vec::new() }
    }
}

impl From<&str> for ModuleLoadError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
