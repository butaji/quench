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
        let fd = state.next_fd;
        let next_fd = fd.checked_add(1).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "file descriptor space exhausted")
        })?;
        state.next_fd = next_fd;
        state.descriptors.insert(fd, FileDescriptor { file, path });
        Ok(fd)
    }

    pub(crate) fn write_descriptor(
        &self,
        fd: i32,
        bytes: &[u8],
        position: Option<u64>,
    ) -> std::io::Result<usize> {
        let mut state = self.0.borrow_mut();
        let descriptor = state
            .descriptors
            .get_mut(&fd)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
        let original = if position.is_some() {
            Some(descriptor.file.stream_position()?)
        } else {
            None
        };
        if let Some(position) = position {
            if let Err(error) = descriptor.file.seek(SeekFrom::Start(position)) {
                return Err(if position > i32::MAX as u64 {
                    std::io::Error::from_raw_os_error(libc::EFBIG)
                } else {
                    error
                });
            }
        }
        let result = descriptor.file.write_all(bytes).map(|()| bytes.len());
        if let Some(original) = original {
            descriptor.file.seek(SeekFrom::Start(original))?;
        }
        result
    }

    pub(crate) fn sync_descriptor(&self, fd: i32, data_only: bool) -> std::io::Result<()> {
        let mut state = self.0.borrow_mut();
        let descriptor = state
            .descriptors
            .get_mut(&fd)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
        if data_only {
            descriptor.file.sync_data()
        } else {
            descriptor.file.sync_all()
        }
    }

    pub(crate) fn read_descriptor(
        &self,
        fd: i32,
        size: usize,
        position: Option<u64>,
    ) -> std::io::Result<Vec<u8>> {
        let mut state = self.0.borrow_mut();
        let descriptor = state
            .descriptors
            .get_mut(&fd)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
        let original = if position.is_some() {
            Some(descriptor.file.stream_position()?)
        } else {
            None
        };
        if let Some(position) = position {
            if let Err(error) = descriptor.file.seek(SeekFrom::Start(position)) {
                return Err(if position > i32::MAX as u64 {
                    std::io::Error::from_raw_os_error(libc::EFBIG)
                } else {
                    error
                });
            }
        }
        let mut bytes = vec![0; size];
        let result = descriptor.file.read(&mut bytes).map(|read| {
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
            descriptor.file.seek(SeekFrom::Start(original))?;
        }
        result
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
    ) -> std::io::Result<Option<Vec<u8>>> {
        let mut state = self.0.borrow_mut();
        let descriptor = state
            .descriptors
            .get_mut(&fd)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
        let mut bytes = vec![0; size];
        let read = descriptor.file.read(&mut bytes)?;
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
