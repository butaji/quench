//! One SSL session owns the original raw stream and its nonblocking retries.
use super::RawStream;
use openssl::ssl::{Error, ErrorCode, ShutdownResult, Ssl, SslStream};
use std::io::{self, Read, Write};
struct BufferedRaw {
    raw: RawStream,
    received: std::collections::VecDeque<u8>,
    saw_eof: bool,
}
impl Read for BufferedRaw {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.received.is_empty() {
            let count = self.raw.read(bytes)?;
            if count == 0 && !bytes.is_empty() {
                self.saw_eof = true;
            }
            return Ok(count);
        }
        let count = bytes.len().min(self.received.len());
        for byte in &mut bytes[..count] {
            *byte = self.received.pop_front().unwrap();
        }
        Ok(count)
    }
}
impl Write for BufferedRaw {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.raw.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.raw.flush()
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Role {
    Client,
    Server,
}
enum Phase {
    Dormant(Role),
    Handshaking(Role),
    AwaitAuthorization,
    Ready,
}
pub(crate) struct EncryptedStream {
    stream: SslStream<BufferedRaw>,
    phase: Phase,
    write_end: Option<usize>,
}
impl EncryptedStream {
    pub(crate) fn new(
        mut ssl: Ssl,
        stream: RawStream,
        role: Role,
        started: bool,
    ) -> Result<Self, openssl::error::ErrorStack> {
        match role {
            Role::Client => ssl.set_connect_state(),
            Role::Server => ssl.set_accept_state(),
        }
        // Queue storage may move; retry extent remains fixed until SSL_write completes.
        unsafe {
            use foreign_types::ForeignType;
            openssl_sys::SSL_ctrl(
                ssl.as_ptr(),
                openssl_sys::SSL_CTRL_MODE,
                openssl_sys::SSL_MODE_ACCEPT_MOVING_WRITE_BUFFER,
                std::ptr::null_mut(),
            );
        }
        Ok(Self {
            stream: SslStream::new(
                ssl,
                BufferedRaw {
                    raw: stream,
                    received: Default::default(),
                    saw_eof: false,
                },
            )?,
            phase: if started {
                Phase::Handshaking(role)
            } else {
                Phase::Dormant(role)
            },
            write_end: None,
        })
    }
}

/// An encrypted stream, as the transport drives it: only the code that encrypts a socket makes
/// one (`EncryptedStream::new`), so an image whose program encrypts none links no OpenSSL for
/// the transport's polls and drops.
pub(crate) trait Encrypted {
    fn raw(&self) -> &RawStream;
    fn ssl(&self) -> &openssl::ssl::SslRef;
    /// The sessions the peer issued since the last call, oldest first.
    fn take_sessions(&self) -> Vec<Vec<u8>>;
    /// The key log lines the session wrote since the last call, oldest first.
    fn take_keylog(&self) -> Vec<Vec<u8>>;
    fn receive(&mut self, bytes: &[u8]);
    fn ready(&self) -> bool;
    fn dormant(&self) -> bool;
    fn start(&mut self);
    fn authorize(&mut self);
    fn handshake(&mut self) -> Result<bool, io::Error>;
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize>;
    fn write(&mut self, bytes: &[u8], offset: usize) -> io::Result<usize>;
    fn write_raw(&mut self, bytes: &[u8]) -> io::Result<usize>;
    fn shutdown_write(&mut self) -> io::Result<bool>;
}

impl Encrypted for EncryptedStream {
    fn raw(&self) -> &RawStream {
        &self.stream.get_ref().raw
    }
    fn ssl(&self) -> &openssl::ssl::SslRef {
        self.stream.ssl()
    }
    fn take_sessions(&self) -> Vec<Vec<u8>> {
        crate::modules::tls_shared_vm::contexts::with_announcements(self.stream.ssl(), |announcements| std::mem::take(&mut announcements.sessions))
            .unwrap_or_default()
    }
    fn take_keylog(&self) -> Vec<Vec<u8>> {
        crate::modules::tls_shared_vm::contexts::with_announcements(self.stream.ssl(), |announcements| {
            announcements.keylog.as_mut().map(std::mem::take).unwrap_or_default()
        }).unwrap_or_default()
    }
    fn receive(&mut self, bytes: &[u8]) {
        self.stream.get_mut().received.extend(bytes);
    }
    fn ready(&self) -> bool {
        matches!(self.phase, Phase::Ready)
    }
    fn dormant(&self) -> bool {
        matches!(self.phase, Phase::Dormant(_))
    }
    fn start(&mut self) {
        if let Phase::Dormant(role) = self.phase {
            self.phase = Phase::Handshaking(role);
        }
    }
    fn authorize(&mut self) {
        if matches!(self.phase, Phase::AwaitAuthorization) {
            self.phase = Phase::Ready;
        }
    }
    fn handshake(&mut self) -> Result<bool, io::Error> {
        let Phase::Handshaking(role) = self.phase else {
            return Ok(false);
        };
        let result = match role {
            Role::Client => self.stream.connect(),
            Role::Server => self.stream.accept(),
        };
        match result {
            Ok(()) => {
                self.phase = Phase::AwaitAuthorization;
                Ok(true)
            }
            Err(error) if readiness(&error) => Ok(false),
            Err(_error) if self.stream.get_ref().saw_eof => {
                // OpenSSL may classify an incomplete handshake EOF as either
                // SSL_ERROR_SYSCALL (sometimes with EIO) or SSL_ERROR_SSL.
                // Translate only when the underlying stream actually returned
                // EOF; a genuine read EIO never sets saw_eof and stays EIO.
                Err(io::Error::from_raw_os_error(libc::ECONNRESET))
            }
            Err(error) => Err(io_error(error)),
        }
    }
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self.stream.ssl_read(bytes) {
            Ok(count) => Ok(count),
            Err(error) if error.code() == ErrorCode::ZERO_RETURN => Ok(0),
            // Node's TLSWrap forwards physical EOF after handshake as stream
            // EOF. OpenSSL's socket BIO reports that same empty-error EOF as
            // SSL_ERROR_SYSCALL; preserve real syscall/SSL failures separately.
            Err(error)
                if error.code() == ErrorCode::SYSCALL
                    && error.io_error().is_none()
                    && error.ssl_error().is_none() =>
            {
                Ok(0)
            }
            Err(error) => Err(io_error(error)),
        }
    }
    fn write(&mut self, bytes: &[u8], offset: usize) -> io::Result<usize> {
        let end = *self.write_end.get_or_insert(bytes.len());
        match self.stream.ssl_write(&bytes[offset..end]) {
            Ok(count) => {
                self.write_end = None;
                Ok(count)
            }
            Err(error) if readiness(&error) => Err(io_error(error)),
            Err(error) => {
                self.write_end = None;
                Err(io_error(error))
            }
        }
    }
    fn write_raw(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.get_mut().raw.write(bytes)
    }
    fn shutdown_write(&mut self) -> io::Result<bool> {
        match self.stream.shutdown() {
            Ok(ShutdownResult::Sent | ShutdownResult::Received) => Ok(true),
            Err(error) if readiness(&error) => Ok(false),
            Err(error) => Err(io_error(error)),
        }
    }
}

fn readiness(error: &Error) -> bool {
    matches!(error.code(), ErrorCode::WANT_READ | ErrorCode::WANT_WRITE)
}
// Retain native SSL errors as typed payloads for VM error projection; no flattening.
pub(crate) fn io_error(error: Error) -> io::Error {
    let kind = if readiness(&error) {
        io::ErrorKind::WouldBlock
    } else {
        error
            .io_error()
            .map(io::Error::kind)
            .unwrap_or(io::ErrorKind::Other)
    };
    io::Error::new(kind, error)
}
