//! Actual Unix IPC pipe ownership, separated from the SDK framing policy.
//! The eventual PipeWrap projection refers to this owner; it never keeps a
//! second socket reader or rewrites NODE_HANDLE/ACK messages.
#![cfg(unix)]
use super::unix_rights::{self as rights, PendingHandle};
use std::collections::VecDeque;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;

const IPC_READ_BYTES: usize = 16 * 1024;

struct Write<T> {
    bytes: Vec<u8>,
    offset: usize,
    descriptor: PendingHandle,
    request: T,
}

enum Phase {
    Open(UnixStream),
    Closed,
}

pub(crate) struct Pipe<T> {
    phase: Phase,
    writes: VecDeque<Write<T>>,
    reading: bool,
    referenced: bool,
}

pub(crate) struct Chunk {
    pub(crate) bytes: Vec<u8>,
    pub(crate) descriptor: Option<OwnedFd>,
}
pub(crate) struct Completion<T> {
    pub(crate) request: T,
    pub(crate) error: Option<io::Error>,
}
pub(crate) struct Progress<T> {
    pub(crate) reads: Vec<Chunk>,
    pub(crate) writes: Vec<Completion<T>>,
    pub(crate) error: Option<io::Error>,
    pub(crate) eof: bool,
}
impl<T> Default for Progress<T> {
    fn default() -> Self {
        Self {
            reads: Vec::new(),
            writes: Vec::new(),
            error: None,
            eof: false,
        }
    }
}
impl<T> Pipe<T> {
    pub(crate) fn new(socket: UnixStream) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            phase: Phase::Open(socket),
            writes: VecDeque::new(),
            reading: false,
            referenced: true,
        })
    }
    fn socket(&self) -> Option<&UnixStream> {
        match &self.phase {
            Phase::Open(socket) => Some(socket),
            Phase::Closed => None,
        }
    }
    pub(crate) fn descriptor(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        self.socket().map(|socket| socket.as_fd())
    }
    pub(crate) fn reference(&mut self, referenced: bool) {
        self.referenced = referenced;
    }
    pub(crate) fn read_start(&mut self) -> io::Result<()> {
        if self.socket().is_none() {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        if self.reading {
            return Err(io::Error::from_raw_os_error(libc::EALREADY));
        }
        self.reading = true;
        Ok(())
    }
    pub(crate) fn read_stop(&mut self) {
        self.reading = false;
    }
    pub(crate) fn has_work(&self) -> bool {
        self.referenced && self.reading || !self.writes.is_empty()
    }
    pub(crate) fn queued_bytes(&self) -> usize {
        self.writes
            .iter()
            .map(|write| write.bytes.len() - write.offset)
            .sum()
    }
    /// Try ordinary bytes before allocating a native request, as StreamBase does.
    /// A queued write owns ordering; only its front may touch the kernel.
    pub(crate) fn try_write(&self, bytes: &[u8]) -> io::Result<usize> {
        if !matches!(self.phase, Phase::Open(_)) {
            return Err(io::Error::from_raw_os_error(libc::EPIPE));
        }
        if !self.writes.is_empty() || bytes.is_empty() {
            return Ok(0);
        }
        let socket = self.socket().expect("open Pipe owns socket");
        let mut plain = PendingHandle::from_optional(None);
        loop {
            match plain.send(socket.as_fd(), bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(0),
                outcome => return outcome,
            }
        }
    }
    pub(crate) fn write(
        &mut self,
        bytes: Vec<u8>,
        handle: Option<OwnedFd>,
        request: T,
    ) -> Result<(), (T, io::Error)> {
        if !matches!(self.phase, Phase::Open(_)) {
            return Err((request, io::Error::from_raw_os_error(libc::EPIPE)));
        }
        if bytes.is_empty() && handle.is_some() {
            return Err((
                request,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "IPC write must contain a payload byte",
                ),
            ));
        }
        self.writes.push_back(Write {
            bytes,
            offset: 0,
            descriptor: PendingHandle::from_optional(handle),
            request,
        });
        Ok(())
    }
    pub(crate) fn close(&mut self) -> Vec<Completion<T>> {
        self.phase = Phase::Closed;
        self.reading = false;
        self.writes
            .drain(..)
            .map(|write| Completion {
                request: write.request,
                error: Some(io::Error::from_raw_os_error(libc::EPIPE)),
            })
            .collect()
    }
    pub(crate) fn progress(&mut self) -> Progress<T> {
        let mut progress = Progress::default();
        while let Some(write) = self.writes.front_mut() {
            if write.bytes.is_empty() {
                let write = self.writes.pop_front().unwrap();
                progress.writes.push(Completion {
                    request: write.request,
                    error: None,
                });
                continue;
            }
            let socket = match &self.phase {
                Phase::Open(socket) => socket,
                Phase::Closed => break,
            };
            match write
                .descriptor
                .send(socket.as_fd(), &write.bytes[write.offset..])
            {
                Ok(0) => {
                    progress.error = Some(io::Error::from_raw_os_error(libc::EPIPE));
                    break;
                }
                Ok(bytes) => {
                    write.offset += bytes;
                    if write.offset == write.bytes.len() {
                        let write = self.writes.pop_front().unwrap();
                        progress.writes.push(Completion {
                            request: write.request,
                            error: None,
                        });
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    progress.error = Some(error);
                    break;
                }
            }
        }
        if progress.error.is_some() {
            progress.writes.extend(self.close());
            return progress;
        }
        if self.reading {
            loop {
                let Some(socket) = self.socket() else { break };
                let mut bytes = vec![0; IPC_READ_BYTES];
                match rights::receive(socket.as_fd(), &mut bytes) {
                    Ok(read) if read.bytes == 0 => {
                        progress.eof = true;
                        self.reading = false;
                        // The unchanged SDK onread policy decides whether to
                        // close the write side; native EOF only stops reading.
                        break;
                    }
                    Ok(read) => {
                        bytes.truncate(read.bytes);
                        progress.reads.push(Chunk {
                            bytes,
                            descriptor: read.descriptor,
                        });
                        // Return demand to guest readStop/close before another recvmsg.
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => {
                        progress.error = Some(error);
                        progress.writes.extend(self.close());
                        break;
                    }
                }
            }
        }
        progress
    }
}
