//! Linux watch transport. Guest callbacks and encoding belong to the Node adapter.
use std::collections::VecDeque;
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

// Match libuv's one-page read batch; it fits a complete maximum-name record.
const EVENT_READ_BUFFER_BYTES: usize = 4096;
const WATCH_EVENTS: u32 = libc::IN_ATTRIB
    | libc::IN_CREATE
    | libc::IN_MODIFY
    | libc::IN_DELETE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO;
const CHANGE_EVENTS: u32 = libc::IN_ATTRIB | libc::IN_MODIFY;

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
    /// None means libuv uses the first subscription's basename.
    pub(crate) name: Option<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct Kernel {
    descriptor: Option<OwnedFd>,
    pending: VecDeque<KernelEvent>,
}

impl Kernel {
    fn descriptor(&mut self) -> io::Result<&OwnedFd> {
        if self.descriptor.is_none() {
            // SAFETY: inotify_init1 returns a newly owned descriptor or -1.
            let descriptor = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            if descriptor < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: ownership of the successful syscall result is transferred once.
            self.descriptor = Some(unsafe { OwnedFd::from_raw_fd(descriptor) });
        }
        Ok(self
            .descriptor
            .as_ref()
            .expect("initialized inotify descriptor"))
    }

    pub(crate) fn subscribe(&mut self, path: &Path) -> io::Result<WatchDescriptor> {
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "watch path contains a null byte",
            )
        })?;
        let descriptor = self.descriptor()?.as_raw_fd();
        // SAFETY: path is terminated and the descriptor remains owned by self.
        let watch = unsafe { libc::inotify_add_watch(descriptor, path.as_ptr(), WATCH_EVENTS) };
        if watch < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(WatchDescriptor(watch))
        }
    }

    /// The adapter calls this only when a kernel subscription loses its last owner.
    pub(crate) fn unsubscribe(&mut self, watch: WatchDescriptor) {
        if let Some(descriptor) = &self.descriptor {
            // SAFETY: descriptor is live. Like libuv, stop ignores an already removed watch.
            unsafe {
                libc::inotify_rm_watch(descriptor.as_raw_fd(), watch.0);
            }
        }
    }

    /// Return one event so the adapter observes subscription changes between callbacks.
    pub(crate) fn next_event(&mut self) -> io::Result<Option<KernelEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let Some(descriptor) = &self.descriptor else {
            return Ok(None);
        };
        let mut buffer = [0u8; EVENT_READ_BUFFER_BYTES];
        let count = loop {
            // SAFETY: buffer is writable for its full length and the descriptor is live.
            let count = unsafe {
                libc::read(
                    descriptor.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if count >= 0 {
                break count as usize;
            }
            let error = io::Error::last_os_error();
            match error.kind() {
                io::ErrorKind::Interrupted => continue,
                io::ErrorKind::WouldBlock => return Ok(None),
                _ => return Err(error),
            }
        };
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "inotify descriptor reached EOF",
            ));
        }
        let mut offset = 0;
        while offset < count {
            let header_end = offset
                .checked_add(std::mem::size_of::<libc::inotify_event>())
                .filter(|end| *end <= count)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "incomplete inotify header")
                })?;
            // SAFETY: header bounds were checked; the byte buffer need not be aligned.
            let event = unsafe {
                std::ptr::read_unaligned(buffer.as_ptr().add(offset).cast::<libc::inotify_event>())
            };
            let end = header_end
                .checked_add(event.len as usize)
                .filter(|end| *end <= count)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "incomplete inotify name")
                })?;
            let name = if event.len == 0 {
                None
            } else {
                let name = &buffer[header_end..end];
                let length = name
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(name.len());
                Some(name[..length].to_vec())
            };
            let kind = if event.mask & !CHANGE_EVENTS != 0 {
                EventKind::Rename
            } else {
                EventKind::Change
            };
            self.pending.push_back(KernelEvent {
                descriptor: WatchDescriptor(event.wd),
                kind,
                name,
            });
            offset = end;
        }
        Ok(self.pending.pop_front())
    }
}

impl Kernel {
    pub(crate) fn reset(&mut self) {
        self.pending.clear();
        self.descriptor = None;
    }
}
