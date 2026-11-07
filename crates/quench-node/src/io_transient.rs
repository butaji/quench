//! Which I/O failures mean "try again later" rather than a broken stream.

/// Whether a non-blocking operation should simply be retried: no data or room yet, or a
/// signal interrupted the call.
pub(crate) fn is_transient(kind: std::io::ErrorKind) -> bool {
    matches!(kind, std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted)
}
