//! Bounded synchronous child pipe collection on Unix.
use std::{
    io::{self, Read},
    process::{Child, Output},
    sync::mpsc,
};

// libuv's synchronous pipe read allocation; retain the observed block on
// overflow, rather than applying execFile's asynchronous string truncation.
const PIPE_READ_BYTES: usize = 64 * 1024;
const OUTPUT_PIPE_COUNT: usize = 2;

pub(crate) struct CollectedOutput {
    pub(crate) output: Output,
    pub(crate) limit_exceeded: bool,
}

pub(crate) fn collect(mut child: Child, max_buffer: f64) -> io::Result<CollectedOutput> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing stdout pipe"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing stderr pipe"))?;
    let (sender, receiver) = mpsc::sync_channel(OUTPUT_PIPE_COUNT);
    read_pipe(stdout, sender.clone(), true);
    read_pipe(stderr, sender, false);
    let mut open_pipes = OUTPUT_PIPE_COUNT;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut limit_exceeded = false;
    while open_pipes != 0 {
        let event = match receiver.recv() {
            Ok(event) => event,
            Err(_) => {
                terminate(&child);
                let _ = child.wait();
                return Err(io::Error::other("child pipe readers disconnected"));
            }
        };
        let (is_stdout, bytes) = match event {
            Ok(event) => event,
            Err(error) => {
                terminate(&child);
                let _ = child.wait();
                return Err(error);
            }
        };
        if bytes.is_empty() {
            open_pipes -= 1;
            continue;
        }
        if is_stdout {
            stdout.extend_from_slice(&bytes);
        } else {
            stderr.extend_from_slice(&bytes);
        }
        let total = stdout.len().saturating_add(stderr.len());
        if max_buffer > 0.0 && total as f64 > max_buffer {
            limit_exceeded = true;
            terminate(&child);
            break;
        }
    }
    drop(receiver);
    let status = child.wait()?;
    Ok(CollectedOutput {
        output: Output {
            status,
            stdout,
            stderr,
        },
        limit_exceeded,
    })
}

fn terminate(child: &Child) {
    // Node's default synchronous overflow signal is SIGTERM.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
}

fn read_pipe<R: Read + Send + 'static>(
    mut pipe: R,
    sender: mpsc::SyncSender<io::Result<(bool, Vec<u8>)>>,
    stdout: bool,
) {
    std::thread::spawn(move || {
        let mut buffer = [0; PIPE_READ_BYTES];
        loop {
            let event = match pipe.read(&mut buffer) {
                Ok(length) => Ok((stdout, buffer[..length].to_vec())),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => Err(error),
            };
            let done = match &event {
                Ok((_, bytes)) => bytes.is_empty(),
                Err(_) => true,
            };
            if sender.send(event).is_err() || done {
                break;
            }
        }
    });
}
