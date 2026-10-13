//! One SSL session owns the original raw stream and its nonblocking retries.
use super::RawStream;
use openssl::ssl::{Error, ErrorCode, ShutdownResult, Ssl, SslStream};
use std::io;
#[derive(Clone, Copy)]
pub(crate) enum Role {
    Client,
    Server,
}
enum Phase {
    Handshaking(Role),
    AwaitAuthorization,
    Ready,
}
pub(crate) struct EncryptedStream {
    stream: SslStream<RawStream>,
    phase: Phase,
    write_end: Option<usize>,
}
impl EncryptedStream {
    pub(crate) fn new(
        mut ssl: Ssl,
        stream: RawStream,
        role: Role,
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
            stream: SslStream::new(ssl, stream)?,
            phase: Phase::Handshaking(role),
            write_end: None,
        })
    }
    pub(crate) fn raw(&self) -> &RawStream {
        self.stream.get_ref()
    }
    pub(crate) fn ssl(&self) -> &openssl::ssl::SslRef {
        self.stream.ssl()
    }
    pub(crate) fn ready(&self) -> bool {
        matches!(self.phase, Phase::Ready)
    }
    pub(crate) fn authorize(&mut self) {
        if matches!(self.phase, Phase::AwaitAuthorization) {
            self.phase = Phase::Ready;
        }
    }
    pub(crate) fn handshake(&mut self) -> Result<bool, Error> {
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
            Err(error) => Err(error),
        }
    }
    pub(crate) fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self.stream.ssl_read(bytes) {
            Ok(count) => Ok(count),
            Err(error) if error.code() == ErrorCode::ZERO_RETURN => Ok(0),
            Err(error) => Err(io_error(error)),
        }
    }
    pub(crate) fn write(&mut self, bytes: &[u8], offset: usize) -> io::Result<usize> {
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
    pub(crate) fn shutdown_write(&mut self) -> io::Result<bool> {
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
