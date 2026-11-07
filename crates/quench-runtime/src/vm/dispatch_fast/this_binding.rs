//! How a lane call target binds `this`.

/// How a call of a `ScriptCallTarget` binds `this`, one byte so the entry stays one cache
/// line: the receiver as is (strict), the receiver boxed or the global for nullish
/// (sloppy), or for an arrow its environment's `this` (`ScriptCallTarget::lexical_this`),
/// or the caller's receiver when its environment binds none. An arrow whose binding is
/// still uninitialized (before `super()`) is never resolved, and an initialized binding
/// never changes.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum ThisBinding {
    Sloppy,
    Strict,
    Lexical,
    CallerReceiver,
}

impl ThisBinding {
    /// Whether the callee binds `this` to the call's receiver.
    pub(super) fn binds_receiver(self) -> bool {
        matches!(self, ThisBinding::Sloppy | ThisBinding::Strict)
    }
}
