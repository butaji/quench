//! The readable and writable streams' states. A stream's numeric `state` slot holds the
//! state's position, and each state's row names it as the specification does.

macro_rules! stream_state {
    ($(#[$meta:meta])* $name:ident { $($state:ident => $label:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(crate) enum $name { $($state),+ }

        impl $name {
            const ALL: [Self; [$($label),+].len()] = [$(Self::$state),+];

            /// The state a `state` slot holds; a number past the last state reads as the last.
            pub(super) fn from(value: f64) -> Self {
                Self::ALL[(value as usize).min(Self::ALL.len() - 1)]
            }

            /// The state's specification name.
            pub(super) fn name(self) -> &'static str {
                [$($label),+][self as usize]
            }
        }
    };
}

stream_state!(
    /// A readable stream's state.
    ReadableState { Readable => "readable", Closed => "closed", Errored => "errored" }
);

stream_state!(
    /// A writable stream's state.
    WritableState { Writable => "writable", Erroring => "erroring", Closed => "closed", Errored => "errored" }
);

impl WritableState {
    /// Whether the stream has settled: closed or errored.
    pub(super) fn is_finished(self) -> bool {
        matches!(self, Self::Closed | Self::Errored)
    }
}
