//! Runtime-neutral ownership for host filesystem descriptors shared by Node
//! adapters.

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::rc::Rc;

/// A handle to the one host-owned descriptor table shared by Node adapters.
#[derive(Clone)]
pub struct FsState(Rc<RefCell<FsStateData>>);

struct FsStateData {
    #[cfg(not(unix))]
    next_fd: i32,
    descriptors: HashMap<i32, FileDescriptor>,
}

pub(crate) struct FileDescriptor {
    pub(crate) file: std::fs::File,
    pub(crate) path: String,
}

impl Default for FsState {
    fn default() -> Self {
        Self::new()
    }
}

impl FsState {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(FsStateData {
            #[cfg(not(unix))]
            next_fd: 3,
            descriptors: HashMap::new(),
        })))
    }

    pub(crate) fn descriptors(&self) -> Ref<'_, HashMap<i32, FileDescriptor>> {
        Ref::map(self.0.borrow(), |state| &state.descriptors)
    }

    pub(crate) fn open_read_stream(&self, path: String) -> std::io::Result<i32> {
        let file = std::fs::File::open(&path)?;
        self.insert_descriptor(file, path)
    }

    pub(crate) fn insert_descriptor(
        &self,
        file: std::fs::File,
        path: String,
    ) -> std::io::Result<i32> {
        let mut state = self.0.borrow_mut();
        #[cfg(unix)]
        let fd = {
            use std::os::fd::AsRawFd;
            file.as_raw_fd()
        };
        #[cfg(not(unix))]
        let fd = {
            let fd = state.next_fd;
            let next_fd = fd.checked_add(1).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::Other, "file descriptor space exhausted")
            })?;
            state.next_fd = next_fd;
            fd
        };
        state.descriptors.insert(fd, FileDescriptor { file, path });
        Ok(fd)
    }

    pub(crate) fn with_descriptor<T>(
        &self,
        fd: i32,
        operation: impl FnOnce(&mut std::fs::File) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let mut state = self.0.borrow_mut();
        if let Some(descriptor) = state.descriptors.get_mut(&fd) {
            return operation(&mut descriptor.file);
        }
        drop(state);
        #[cfg(unix)]
        {
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
        #[cfg(not(unix))]
        Err(std::io::Error::from_raw_os_error(libc::EBADF))
    }

    pub(crate) fn write_all_descriptor(
        &self,
        fd: i32,
        bytes: &[u8],
        position: Option<u64>,
    ) -> std::io::Result<usize> {
        self.with_descriptor(fd, |file| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileExt;
                match position {
                    Some(position) => file.write_all_at(bytes, position),
                    None => file.write_all(bytes),
                }
                .map(|()| bytes.len())
            }
            #[cfg(not(unix))]
            {
                let original = if position.is_some() {
                    Some(file.stream_position()?)
                } else {
                    None
                };
                if let Some(position) = position {
                    if let Err(error) = file.seek(SeekFrom::Start(position)) {
                        return Err(if position > i32::MAX as u64 {
                            std::io::Error::from_raw_os_error(libc::EFBIG)
                        } else {
                            error
                        });
                    }
                }
                let result = file.write_all(bytes).map(|()| bytes.len());
                if let Some(original) = original {
                    file.seek(SeekFrom::Start(original))?;
                }
                result
            }
        })
    }

    /// One native write, including partial progress; positional I/O must not move
    /// the shared open-file offset.
    pub(crate) fn write_descriptor(
        &self,
        fd: i32,
        bytes: &[u8],
        position: Option<u64>,
    ) -> std::io::Result<usize> {
        self.with_descriptor(fd, |file| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileExt;
                match position {
                    Some(position) => file.write_at(bytes, position),
                    None => file.write(bytes),
                }
            }
            #[cfg(not(unix))]
            {
                if let Some(position) = position {
                    let original = file.stream_position()?;
                    file.seek(SeekFrom::Start(position))?;
                    let result = file.write(bytes);
                    file.seek(SeekFrom::Start(original))?;
                    result
                } else {
                    file.write(bytes)
                }
            }
        })
    }

    pub(crate) fn sync_descriptor(&self, fd: i32, data_only: bool) -> std::io::Result<()> {
        self.with_descriptor(fd, |file| {
            if data_only {
                file.sync_data()
            } else {
                file.sync_all()
            }
        })
    }

    pub(crate) fn read_descriptor(
        &self,
        fd: i32,
        size: usize,
        position: Option<u64>,
    ) -> std::io::Result<Vec<u8>> {
        self.with_descriptor(fd, |file| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileExt;
                let mut bytes = vec![0; size];
                let read = match position {
                    Some(position) => file.read_at(&mut bytes, position),
                    None => file.read(&mut bytes),
                }?;
                bytes.truncate(read);
                Ok(bytes)
            }
            #[cfg(not(unix))]
            {
                let original = if position.is_some() {
                    Some(file.stream_position()?)
                } else {
                    None
                };
                if let Some(position) = position {
                    if let Err(error) = file.seek(SeekFrom::Start(position)) {
                        return Err(if position > i32::MAX as u64 {
                            std::io::Error::from_raw_os_error(libc::EFBIG)
                        } else {
                            error
                        });
                    }
                }
                let mut bytes = vec![0; size];
                let result = file.read(&mut bytes).map(|read| {
                    bytes.truncate(read);
                    bytes
                });
                let result = match (result, position) {
                    (Err(_), Some(position)) if position > i32::MAX as u64 => {
                        Err(std::io::Error::from_raw_os_error(libc::EFBIG))
                    }
                    (result, _) => result,
                };
                if let Some(original) = original {
                    file.seek(SeekFrom::Start(original))?;
                }
                result
            }
        })
    }

    pub(crate) fn close_stream(&self, fd: i32) -> std::io::Result<String> {
        self.0
            .borrow_mut()
            .descriptors
            .remove(&fd)
            .map(|descriptor| descriptor.path)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
    }

    pub(crate) fn read_stream_chunk(
        &self,
        fd: i32,
        size: usize,
        position: Option<u64>,
    ) -> std::io::Result<Option<Vec<u8>>> {
        let mut state = self.0.borrow_mut();
        let descriptor = state
            .descriptors
            .get_mut(&fd)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
        let mut bytes = vec![0; size];
        let file = &mut descriptor.file;
        let original_position = if position.is_some() {
            Some(file.stream_position()?)
        } else {
            None
        };
        if let Some(position) = position {
            file.seek(SeekFrom::Start(position))?;
        }
        let result = file.read(&mut bytes);
        if let Some(original_position) = original_position {
            file.seek(SeekFrom::Start(original_position))?;
        }
        let read = result?;
        if read == 0 {
            return Ok(None);
        }
        bytes.truncate(read);
        Ok(Some(bytes))
    }

    pub(crate) fn close_read_stream(&self, fd: i32) {
        self.0.borrow_mut().descriptors.remove(&fd);
    }
}
