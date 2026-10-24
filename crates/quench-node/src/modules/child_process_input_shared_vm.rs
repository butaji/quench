//! Nonblocking child stdin owns each accepted byte until OS write completion.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::VecDeque;
#[cfg(unix)]
use std::fs::File as Pipe;
#[cfg(not(unix))]
use std::process::ChildStdin as Pipe;

const COMPLETION_SUCCESS: i32 = 0;
#[cfg(not(unix))]
use std::io::Write;

fn write_pipe(pipe: &mut Pipe, bytes: &[u8]) -> std::io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // Per-operation flags preserve a blocking endpoint when inherited by
        // another child, without letting a parent write block the VM.
        let result = unsafe {
            libc::send(
                pipe.as_raw_fd(),
                bytes.as_ptr().cast(),
                bytes.len(),
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            )
        };
        if result < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(result as usize)
        }
    }
    #[cfg(not(unix))]
    pipe.write(bytes)
}

enum Phase {
    Open(Pipe),
    Ending(Pipe),
    Closed,
}
struct Pending {
    bytes: Vec<u8>,
    offset: usize,
    callback: RootId,
}
pub(crate) struct Input {
    phase: Phase,
    pending: VecDeque<Pending>,
    final_callback: Option<RootId>,
}
pub(crate) struct Completion {
    pub(crate) callback: RootId,
    pub(crate) errno: Option<i32>,
}
pub(crate) struct Progress {
    pub(crate) progressed: bool,
    pub(crate) callbacks: Vec<Completion>,
}

impl Input {
    pub(crate) fn close(&mut self) {
        self.phase = Phase::Closed;
    }
    pub(crate) fn new(pipe: Option<Pipe>) -> Self {
        let phase = if let Some(pipe) = pipe {
            Phase::Open(pipe)
        } else {
            Phase::Closed
        };
        Self {
            phase,
            pending: VecDeque::new(),
            final_callback: None,
        }
    }
    #[cfg(unix)]
    pub(crate) fn descriptor(&self) -> Option<i32> {
        use std::os::fd::AsRawFd;
        match &self.phase {
            Phase::Open(pipe) | Phase::Ending(pipe) => Some(pipe.as_raw_fd()),
            Phase::Closed => None,
        }
    }
    fn pipe(&mut self) -> Option<&mut Pipe> {
        match &mut self.phase {
            Phase::Open(pipe) | Phase::Ending(pipe) => Some(pipe),
            Phase::Closed => None,
        }
    }
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty() || self.final_callback.is_some()
    }
    /// True means the caller can complete synchronously and retains no callback.
    pub(crate) fn write(&mut self, bytes: Vec<u8>, callback: RootId) -> std::io::Result<bool> {
        if !matches!(self.phase, Phase::Open(_)) {
            return Err(std::io::Error::from_raw_os_error(libc::EPIPE));
        }
        let mut offset = 0;
        if self.pending.is_empty() {
            while offset < bytes.len() {
                match write_pipe(self.pipe().expect("open stdin"), &bytes[offset..]) {
                    Ok(0) => return Err(std::io::Error::from_raw_os_error(libc::EPIPE)),
                    Ok(length) => offset += length,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                }
            }
        }
        if offset == bytes.len() {
            return Ok(true);
        }
        self.pending.push_back(Pending {
            bytes,
            offset,
            callback,
        });
        Ok(false)
    }
    pub(crate) fn end(&mut self, callback: RootId) -> bool {
        if self.pending.is_empty() {
            self.phase = Phase::Closed;
            return true;
        }
        self.phase = match std::mem::replace(&mut self.phase, Phase::Closed) {
            Phase::Open(pipe) | Phase::Ending(pipe) => Phase::Ending(pipe),
            Phase::Closed => Phase::Closed,
        };
        self.final_callback = Some(callback);
        false
    }
    pub(crate) fn progress(&mut self) -> Progress {
        let mut result = Progress {
            progressed: false,
            callbacks: Vec::new(),
        };
        let mut failure = None;
        while let Some(pending) = self.pending.front() {
            let bytes = &pending.bytes[pending.offset..];
            let written = match &mut self.phase {
                Phase::Open(pipe) | Phase::Ending(pipe) => write_pipe(pipe, bytes),
                Phase::Closed => Err(std::io::Error::from_raw_os_error(libc::EPIPE)),
            };
            match written {
                Ok(0) => {
                    failure = Some(libc::EPIPE);
                    break;
                }
                Ok(length) => {
                    result.progressed = true;
                    let pending = self.pending.front_mut().expect("pending stdin write");
                    pending.offset += length;
                    if pending.offset == pending.bytes.len() {
                        let pending = self.pending.pop_front().expect("completed stdin write");
                        result.callbacks.push(Completion {
                            callback: pending.callback,
                            errno: None,
                        });
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    failure = Some(error.raw_os_error().unwrap_or(libc::EIO));
                    break;
                }
            }
        }
        if let Some(errno) = failure {
            self.phase = Phase::Closed;
            result.progressed = true;
            result
                .callbacks
                .extend(self.pending.drain(..).map(|pending| Completion {
                    callback: pending.callback,
                    errno: Some(errno),
                }));
            if let Some(callback) = self.final_callback.take() {
                result.callbacks.push(Completion {
                    callback,
                    errno: Some(errno),
                });
            }
        } else if self.pending.is_empty() && matches!(self.phase, Phase::Ending(_)) {
            self.phase = Phase::Closed;
            result.progressed = true;
            if let Some(callback) = self.final_callback.take() {
                result.callbacks.push(Completion {
                    callback,
                    errno: None,
                });
            }
        }
        result
    }
}

pub(crate) fn write_error(
    context: &mut NativeContext<'_, NodeHost>,
    errno: i32,
) -> Result<RootId, RootedError> {
    let code = super::util_system_error_shared_vm::system_error_name(-f64::from(errno));
    let error = context.error_rooted(&format!("write {code}"))?;
    for (name, value) in [
        ("code", context.string_rooted(&code)),
        ("errno", context.number(-f64::from(errno))),
        ("syscall", context.string_rooted("write")),
    ] {
        let key = context.string_rooted(name);
        context.set_property_rooted(error, key, value, error)?;
    }
    Ok(error)
}

pub(crate) fn complete(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("stdin completion callback missing"))?;
    let errno = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .unwrap_or(f64::from(COMPLETION_SUCCESS)) as i32;
    let undefined = context.undefined();
    if errno == COMPLETION_SUCCESS {
        context.call_rooted(callback, undefined, &[])
    } else {
        let error = write_error(context, errno)?;
        context.call_rooted(callback, undefined, &[error])
    }
}

pub(crate) fn queue(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    callbacks: Vec<Completion>,
) -> Result<(), String> {
    for completion in callbacks {
        let callback = runtime
            .host_function(crate::host::shared_vm::operation(
                "childProcessInputComplete",
            ))
            .map_err(|error| runtime.format_error(program, &error.error))?;
        let receiver = runtime.root(quench_runtime::Value::UNDEFINED);
        let errno = runtime.root(quench_runtime::Value::number(f64::from(
            completion.errno.unwrap_or(COMPLETION_SUCCESS),
        )));
        runtime
            .host_mut()
            .shared_state()
            .borrow_mut()
            .scheduler
            .queue_shared_next_tick(super::shared_event_loop::SharedCallback {
                execution: None,
                callback,
                receiver,
                args: vec![completion.callback, errno],
            });
    }
    Ok(())
}
