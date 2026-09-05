/// Typed decoding of the C-ABI status word. Keeping this state machine in one
/// place prevents baseline and composed entries from assigning different
/// retry semantics to the same post-entry outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeStatus {
    Ok,
    SemanticError,
    CommittedError,
    Interrupt,
    Unknown(u64),
}

impl From<u64> for NativeStatus {
    fn from(status: u64) -> Self {
        match status {
            NATIVE_DISPATCH_OK => Self::Ok,
            NATIVE_DISPATCH_SEMANTIC_ERROR => Self::SemanticError,
            NATIVE_DISPATCH_COMMITTED_ERROR => Self::CommittedError,
            NATIVE_DISPATCH_INTERRUPT => Self::Interrupt,
            other => Self::Unknown(other),
        }
    }
}
