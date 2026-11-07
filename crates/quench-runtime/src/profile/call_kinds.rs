//! The kinds of call the profile counts.

/// How the general loop reached a call (the report's `source_names`).
#[derive(Clone, Copy)]
pub(crate) enum CallSource { Dynamic, Known, Method, ThisMethod, Construct }

/// A call in tail position, by the kind of call.
#[derive(Clone, Copy)]
pub(crate) enum TerminalCall { Function, Method, ThisMethod }

#[cfg(feature = "profile-aggregate")]
impl CallSource {
    pub(crate) const COUNT: usize = Self::Construct as usize + 1;
}

#[cfg(feature = "profile-aggregate")]
impl TerminalCall {
    pub(crate) const COUNT: usize = Self::ThisMethod as usize + 1;
}
