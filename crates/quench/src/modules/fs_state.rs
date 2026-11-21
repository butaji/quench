//! Runtime-neutral ownership for host filesystem descriptors shared by Node
//! adapters.

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::rc::Rc;

/// A handle to the one host-owned descriptor table shared by Node adapters: each file `fs`
/// opened, keyed by its descriptor.
#[derive(Clone, Default)]
pub struct FsState(Rc<RefCell<HashMap<i32, std::fs::File>>>);

impl FsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert_descriptor(&self, file: std::fs::File) -> i32 {
        let fd = file.as_raw_fd();
        self.0.borrow_mut().insert(fd, file);
        fd
    }

    /// Run `operation` on `fd`: a file the table owns, or a descriptor the process inherited.
    pub(crate) fn with_descriptor<T>(
        &self,
        fd: i32,
        operation: impl FnOnce(&mut std::fs::File) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let mut state = self.0.borrow_mut();
        if let Some(file) = state.get_mut(&fd) {
            return operation(file);
        }
        drop(state);
        use std::os::fd::{FromRawFd, OwnedFd};
        // Borrow via a checked duplicate, so a guest-supplied invalid fd
        // never violates BorrowedFd's validity contract or transfers ownership.
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let owned = unsafe { OwnedFd::from_raw_fd(duplicate) };
        operation(&mut std::fs::File::from(owned))
    }

    /// One native write, including partial progress; positional I/O must not move
    /// the shared open-file offset.
    pub(crate) fn write_descriptor(
        &self,
        fd: i32,
        bytes: &[u8],
        position: Option<u64>,
    ) -> std::io::Result<usize> {
        use std::io::Write;
        use std::os::unix::fs::FileExt;
        self.with_descriptor(fd, |file| match position {
            Some(position) => file.write_at(bytes, position),
            None => file.write(bytes),
        })
    }

    pub(crate) fn close_descriptor(&self, fd: i32) -> std::io::Result<()> {
        self.0
            .borrow_mut()
            .remove(&fd)
            .map(drop)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
    }

    pub(crate) fn read_stream_chunk(
        &self,
        fd: i32,
        size: usize,
        position: Option<u64>,
    ) -> std::io::Result<Option<Vec<u8>>> {
        let mut state = self.0.borrow_mut();
        let file = state
            .get_mut(&fd)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
        // The chunk becomes a Buffer's backing store, so read straight into
        // uninitialized capacity rather than zeroing bytes the read overwrites. A positioned
        // read (`pread`) leaves the descriptor's own offset where it was, as Node's does.
        let mut bytes = Vec::<u8>::with_capacity(size);
        let fd = file.as_raw_fd();
        let read = loop {
            // SAFETY: the kernel writes at most `size` bytes into the reserved capacity.
            let read = unsafe {
                match position {
                    Some(position) => libc::pread(
                        fd,
                        bytes.as_mut_ptr().cast(),
                        size,
                        libc::off_t::try_from(position)
                            .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?,
                    ),
                    None => libc::read(fd, bytes.as_mut_ptr().cast(), size),
                }
            };
            if read >= 0 {
                break read as usize;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        };
        if read == 0 {
            return Ok(None);
        }
        // SAFETY: the kernel initialized the first `read` bytes.
        unsafe { bytes.set_len(read) };
        Ok(Some(bytes))
    }

    /// Whether no descriptor is open (`startup_snapshot`).
    pub(crate) fn snapshot_quiescent(&self) -> bool {
        self.0.borrow().is_empty()
    }
}
