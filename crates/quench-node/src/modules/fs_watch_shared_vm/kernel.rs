//! Platform transport shares event facts; guest policy remains in the Node adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WatchDescriptor(pub(crate) i32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventKind {
    Change,
    Rename,
}
pub(crate) struct KernelEvent {
    pub(crate) descriptor: WatchDescriptor,
    pub(crate) kind: EventKind,
    /// No kernel name: use the first subscription's basename.
    pub(crate) name: Option<Vec<u8>>,
}
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "darwin.rs"]
mod platform;
pub(crate) use platform::Kernel;
