//! Synchronous child pipes have one OS owner and one bounded poll loop.
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    process::{Child, Output},
    time::{Duration, Instant},
};

// libuv's synchronous read allocation: retain the observed block on overflow.
const PIPE_READ_BYTES: usize = 64 * 1024;
// A child without parent pipe endpoints still needs deadline-aware reaping.
const EXIT_POLL_MILLIS: i32 = 1;
const NANOS_PER_MILLI: u32 = Duration::from_millis(1).subsec_nanos();
const STDIN: usize = 0;
const STDOUT: usize = 1;
const STDERR: usize = 2;
const CHANNELS: usize = super::child_process_stdio_shared_vm::STANDARD_CHANNELS;

pub(crate) struct CollectedOutput {
    pub(crate) output: Output,
    pub(crate) error: Option<i32>,
}

pub(crate) fn collect(
    mut child: Child,
    max_buffer: f64,
    timeout: Option<Duration>,
    kill_signal: i32,
    input: &[u8],
) -> io::Result<CollectedOutput> {
    let mut stdin = child.stdin.take();
    let mut stdout_pipe = child
        .stdout
        .take()
        .map(|pipe| std::fs::File::from(std::os::fd::OwnedFd::from(pipe)));
    let mut stderr_pipe = child
        .stderr
        .take()
        .map(|pipe| std::fs::File::from(std::os::fd::OwnedFd::from(pipe)));
    if input.is_empty() {
        stdin = None;
    }
    let initialized = (|| {
        if let Some(pipe) = &stdin {
            nonblocking(pipe.as_raw_fd())?;
        }
        if let Some(pipe) = &stdout_pipe {
            nonblocking(pipe.as_raw_fd())?;
        }
        if let Some(pipe) = &stderr_pipe {
            nonblocking(pipe.as_raw_fd())?;
        }
        Ok::<_, io::Error>(())
    })();
    if let Err(error) = initialized {
        let mut ignored = None;
        terminate(&child, kill_signal, &mut ignored);
        drop(stdin);
        drop(stdout_pipe);
        drop(stderr_pipe);
        let _ = child.wait();
        return Err(error);
    }
    let mut input_offset = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let deadline = timeout.and_then(|duration| Instant::now().checked_add(duration));
    // Node's runner errors take precedence over a pipe's first error.
    let mut error = None;
    let mut pipe_error = None;
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(failure) => {
                error = Some(failure.raw_os_error().unwrap_or(libc::EIO));
                break;
            }
        };
        if stdin.is_none() && stdout_pipe.is_none() && stderr_pipe.is_none() {
            if let Some(status) = status {
                return Ok(CollectedOutput {
                    output: Output {
                        status,
                        stdout,
                        stderr,
                    },
                    error: error.or(pipe_error),
                });
            }
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            error = Some(libc::ETIMEDOUT);
            break;
        }
        let mut poll = [libc::pollfd {
            fd: -1,
            events: 0,
            revents: 0,
        }; CHANNELS];
        if let Some(pipe) = &stdin {
            poll[STDIN].fd = pipe.as_raw_fd();
            poll[STDIN].events = libc::POLLOUT;
        }
        if let Some(pipe) = &stdout_pipe {
            poll[STDOUT].fd = pipe.as_raw_fd();
            poll[STDOUT].events = libc::POLLIN;
        }
        if let Some(pipe) = &stderr_pipe {
            poll[STDERR].fd = pipe.as_raw_fd();
            poll[STDERR].events = libc::POLLIN;
        }
        let no_pipes = poll.iter().all(|pipe| pipe.fd < 0);
        if no_pipes && deadline.is_none() {
            let status = child.wait()?;
            return Ok(CollectedOutput {
                output: Output {
                    status,
                    stdout,
                    stderr,
                },
                error: error.or(pipe_error),
            });
        }
        let wait = if no_pipes {
            EXIT_POLL_MILLIS
        } else {
            deadline.map_or(-1, |deadline| {
                let remaining = deadline.saturating_duration_since(Instant::now());
                remaining
                    .as_millis()
                    .saturating_add(u128::from(remaining.subsec_nanos() % NANOS_PER_MILLI != 0))
                    .min(i32::MAX as u128) as i32
            })
        };
        if unsafe { libc::poll(poll.as_mut_ptr(), poll.len() as libc::nfds_t, wait) } < 0 {
            let failure = io::Error::last_os_error();
            if failure.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            error = Some(failure.raw_os_error().unwrap_or(libc::EIO));
            break;
        }
        if poll[STDIN].revents != 0 {
            if let Some(pipe) = &mut stdin {
                match pipe.write(&input[input_offset..]) {
                    Ok(length) => {
                        input_offset += length;
                        if input_offset == input.len() {
                            stdin = None;
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                        ) => {}
                    Err(failure) => {
                        pipe_error.get_or_insert(failure.raw_os_error().unwrap_or(libc::EIO));
                        stdin = None;
                    }
                }
            }
        }
        for index in [STDOUT, STDERR] {
            if poll[index].revents == 0 {
                continue;
            }
            let (pipe, bytes) = if index == STDOUT {
                (&mut stdout_pipe, &mut stdout)
            } else {
                (&mut stderr_pipe, &mut stderr)
            };
            let Some(reader) = pipe.as_mut() else {
                continue;
            };
            let mut buffer = [0; PIPE_READ_BYTES];
            match reader.read(&mut buffer) {
                Ok(0) => *pipe = None,
                Ok(length) => bytes.extend_from_slice(&buffer[..length]),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) => {}
                Err(failure) => {
                    pipe_error.get_or_insert(failure.raw_os_error().unwrap_or(libc::EIO));
                    *pipe = None;
                }
            }
            if max_buffer > 0.0 && stdout.len().saturating_add(stderr.len()) as f64 > max_buffer {
                error = Some(libc::ENOBUFS);
                break;
            }
        }
        if error.is_some() {
            break;
        }
    }
    // Node closes every parent endpoint on timeout/overflow, including when a
    // grandchild holds a pipe after the direct child exits. Reap before return.
    if child.try_wait().unwrap_or(None).is_none() {
        terminate(&child, kill_signal, &mut error);
    }
    drop(stdin);
    drop(stdout_pipe);
    drop(stderr_pipe);
    let status = child.wait()?;
    Ok(CollectedOutput {
        output: Output {
            status,
            stdout,
            stderr,
        },
        error: error.or(pipe_error),
    })
}

fn nonblocking(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn terminate(child: &Child, signal: i32, error: &mut Option<i32>) {
    if unsafe { libc::kill(child.id() as libc::pid_t, signal) } < 0 {
        let errno = io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO);
        if errno != libc::ESRCH {
            error.get_or_insert(errno);
            // Match Node's Kill fallback only when the requested signal fails.
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGKILL);
            }
        }
    }
}
