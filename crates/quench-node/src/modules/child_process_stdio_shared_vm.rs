//! One normalized stdio specification for OS descriptors and guest projections.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::process::Stdio;

pub(crate) const STANDARD_CHANNELS: usize = 3;

#[derive(Clone, Copy)]
pub(crate) enum Specification {
    Pipe,
    Ignore,
    Inherit,
    Descriptor(i32),
    #[cfg(unix)]
    Stream {
        fd: i32,
        owner: u64,
        channel: Channel,
    },
    Ipc,
}

#[cfg(unix)]
#[derive(Clone, Copy)]
pub(crate) enum Channel {
    Stdin,
    Stdout,
    Stderr,
}

impl Specification {
    pub(crate) fn piped(self) -> bool {
        matches!(self, Self::Pipe)
    }

    pub(crate) fn os(self) -> std::io::Result<Stdio> {
        match self {
            Self::Pipe => Ok(Stdio::piped()),
            Self::Ignore => Ok(Stdio::null()),
            Self::Inherit => Ok(Stdio::inherit()),
            Self::Descriptor(fd) => descriptor(fd),
            #[cfg(unix)]
            Self::Stream { fd, .. } => descriptor(fd),
            Self::Ipc => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "IPC requires a duplex channel",
            )),
        }
    }
}

#[cfg(unix)]
fn descriptor(fd: i32) -> std::io::Result<Stdio> {
    use std::os::fd::{FromRawFd, OwnedFd};
    // Command owns the duplicate; the guest's original descriptor stays open.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Stdio::from(unsafe { OwnedFd::from_raw_fd(duplicate) }))
}

#[cfg(not(unix))]
fn descriptor(_: i32) -> std::io::Result<Stdio> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "numeric child descriptors are unavailable on this platform",
    ))
}

pub(crate) fn normalize(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<Vec<Specification>, RootedError> {
    normalize_for(context, options, false)
}

pub(crate) fn normalize_sync(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<Vec<Specification>, RootedError> {
    normalize_for(context, options, true)
}

fn normalize_for(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
    synchronous: bool,
) -> Result<Vec<Specification>, RootedError> {
    let mut result = vec![Specification::Pipe; STANDARD_CHANNELS];
    let Some(options) = options else {
        return Ok(result);
    };
    let key = context.string_rooted("stdio");
    let value = context.get_property_rooted(options, key)?;
    if !context.truthy_rooted(value)? {
        return Ok(result);
    }
    if let Some(name) = context.string_text(value)? {
        let specification = match name.as_str() {
            "pipe" | "overlapped" => Specification::Pipe,
            "ignore" => Specification::Ignore,
            "inherit" => Specification::Inherit,
            _ => return Err(invalid(context, value, false)?),
        };
        return Ok(vec![specification; STANDARD_CHANNELS]);
    }
    if !context.is_array_rooted(value)? {
        return Err(invalid(context, value, false)?);
    }
    let key = context.string_rooted("length");
    let length = context.get_property_rooted(value, key)?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default() as usize;
    result.resize(length.max(STANDARD_CHANNELS), Specification::Ignore);
    let mut ipc = false;
    for (index, specification) in result.iter_mut().enumerate() {
        let key = context.string_rooted(&index.to_string());
        let item = if index >= length {
            let undefined = context.undefined();
            context.set_property_rooted(value, key, undefined, value)?;
            undefined
        } else {
            context.get_property_rooted(value, key)?
        };
        *specification = if index >= STANDARD_CHANNELS
            && context
                .rooted_value(item)
                .is_some_and(|value| value.is_null() || value.is_undefined())
        {
            Specification::Ignore
        } else {
            channel(context, item)?
        };
        if matches!(*specification, Specification::Ipc) {
            if synchronous {
                let error = context.error_rooted("IPC cannot be used with synchronous forks")?;
                let key = context.string_rooted("code");
                let code = context.string_rooted("ERR_IPC_SYNC_FORK");
                context.define_data_property_rooted(error, key, code, true, true, true)?;
                return Err(context.throw(error));
            }
            if ipc {
                let error = context.error_rooted("Child process can have only one IPC pipe")?;
                let key = context.string_rooted("code");
                let code = context.string_rooted("ERR_IPC_ONE_PIPE");
                context.set_property_rooted(error, key, code, error)?;
                return Err(context.throw(error));
            }
            ipc = true;
        }
    }
    Ok(result)
}

fn channel(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Specification, RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(Specification::Pipe);
    }
    if let Some(name) = context.string_text(value)? {
        return match name.as_str() {
            "pipe" | "overlapped" => Ok(Specification::Pipe),
            "ignore" => Ok(Specification::Ignore),
            "inherit" => Ok(Specification::Inherit),
            "ipc" => Ok(Specification::Ipc),
            _ => Err(invalid(context, value, true)?),
        };
    }
    if let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    {
        return if number < 0.0 {
            Ok(Specification::Pipe)
        } else {
            numeric_descriptor(context, value)
        };
    }
    if context.is_object_rooted(value)? {
        let key = context.string_rooted("fd");
        let fd = context.get_property_rooted(value, key)?;
        if context
            .rooted_value(fd)
            .is_some_and(|value| value.as_number().is_some())
        {
            // Node's type test and descriptor projection are separate observable gets.
            let key = context.string_rooted("fd");
            let fd = context.get_property_rooted(value, key)?;
            return numeric_descriptor(context, fd);
        }
    }
    #[cfg(unix)]
    if let Some(specification) = inherited_stream_descriptor(context, value)? {
        return Ok(specification);
    }
    if let Some(fd) = super::net_shared_vm::child_stdio_socket_descriptor(context, value)? {
        return Ok(Specification::Descriptor(fd));
    }
    let bytes =
        context.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::ArrayBufferView)?;
    Err(invalid(context, value, bytes)?)
}

/// Registered guest streams identify their owned native pipe by identity.
/// Public `fd` projection is handled first, matching Node's stdio validation.
#[cfg(unix)]
fn inherited_stream_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Option<Specification>, RootedError> {
    let state = context.host_mut().shared_state();
    let streams = state
        .borrow()
        .child_processes
        .iter()
        .flat_map(|(owner, process)| {
            [
                (
                    process.stdin_stream,
                    process.stdin.descriptor(),
                    *owner,
                    Channel::Stdin,
                ),
                (
                    process.stdout,
                    process.stdout_pipe.descriptor(),
                    *owner,
                    Channel::Stdout,
                ),
                (
                    process.stderr,
                    process.stderr_pipe.descriptor(),
                    *owner,
                    Channel::Stderr,
                ),
            ]
        })
        .filter_map(|(stream, fd, owner, channel)| {
            stream.map(|stream| (stream, fd, owner, channel))
        })
        .collect::<Vec<_>>();
    for (stream, fd, owner, channel) in streams {
        if context.strict_equal_rooted(value, stream)? {
            return Ok(fd.map(|fd| Specification::Stream { fd, owner, channel }));
        }
    }
    Ok(None)
}

pub(crate) fn activate_inherited_streams(
    context: &mut NativeContext<'_, NodeHost>,
    specifications: &[Specification],
) -> Result<(), RootedError> {
    #[cfg(unix)]
    for specification in specifications {
        let Specification::Stream { owner, channel, .. } = specification else {
            continue;
        };
        let state = context.host_mut().shared_state();
        let stream = {
            let mut state = state.borrow_mut();
            state
                .child_processes
                .get_mut(owner)
                .and_then(|process| match channel {
                    Channel::Stdin => None,
                    Channel::Stdout => {
                        process.stdout_pipe.stop();
                        process.stdout
                    }
                    Channel::Stderr => {
                        process.stderr_pipe.stop();
                        process.stderr
                    }
                })
        };
        if let Some(stream) = stream {
            let key = context.string_rooted("pause");
            let pause = context.get_property_rooted(stream, key)?;
            context.call_rooted(pause, stream, &[])?;
            if let Some(process) = state.borrow_mut().child_processes.get_mut(owner) {
                match channel {
                    Channel::Stdin => {}
                    Channel::Stdout => process.stdout_pipe.inherit(),
                    Channel::Stderr => process.stderr_pipe.inherit(),
                }
            };
        }
    }
    #[cfg(not(unix))]
    let _ = (context, specifications);
    Ok(())
}

fn numeric_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Specification, RootedError> {
    let zero = context.number(0.0);
    let integer = context.bitwise_or_rooted(value, zero)?;
    let fd = context
        .rooted_value(integer)
        .and_then(|value| value.as_number())
        .expect("numeric descriptor coercion") as i32;
    Ok(Specification::Descriptor(fd))
}

fn invalid(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    input: bool,
) -> Result<RootedError, RootedError> {
    let util = crate::host::shared_vm::commonjs::util_module(context)?;
    let key = context.string_rooted("inspect");
    let inspect = context.get_property_rooted(util, key)?;
    let undefined = context.undefined();
    let text = context.call_rooted(inspect, undefined, &[value])?;
    let text = context.to_string(text)?;
    let (code, message) = if input {
        (
            "ERR_INVALID_SYNC_FORK_INPUT",
            format!(
                "Asynchronous forks do not support Buffer, TypedArray, DataView or string input: {text}"
            ),
        )
    } else {
        (
            "ERR_INVALID_ARG_VALUE",
            format!("The argument 'stdio' is invalid. Received {text}"),
        )
    };
    let error = context.type_error_rooted(&message)?;
    let key = context.string_rooted("code");
    let code = context.string_rooted(code);
    context.set_property_rooted(error, key, code, error)?;
    Ok(context.throw(error))
}

use super::child_process_ipc_shared_vm::IpcChannel;

pub(crate) struct Configured {
    pub(crate) ipc: Option<IpcChannel>,
    #[cfg(unix)]
    pub(crate) pipes: [Option<std::os::fd::OwnedFd>; STANDARD_CHANNELS],
    #[cfg(unix)]
    pub(crate) extra_pipes: Vec<(usize, std::os::fd::OwnedFd, std::os::fd::OwnedFd)>,
}

pub(crate) fn configure(
    command: &mut std::process::Command,
    specifications: &[Specification],
) -> std::io::Result<Configured> {
    // The mode is owned by this IPC endpoint and must be selected before its
    // reader starts parsing frames. Preserve the Node spawn adapter's mode in
    // both the child environment and the parent-side channel state.
    let serialization_mode = command
        .get_envs()
        .find_map(|(key, value)| {
            (key == std::ffi::OsStr::new("NODE_CHANNEL_SERIALIZATION_MODE")).then(|| {
                value
                    .and_then(|value| value.to_str())
                    .unwrap_or("json")
                    .to_owned()
            })
        })
        .unwrap_or_else(|| "json".to_owned());
    let ipc_index = specifications
        .iter()
        .position(|value| matches!(value, Specification::Ipc));
    let channel = match ipc_index {
        Some(index) => Some(ipc_channel(
            command,
            index,
            specifications.len(),
            serialization_mode == "advanced",
        )?),
        None => None,
    };
    #[cfg(unix)]
    let mut pipes = std::array::from_fn(|_| None);
    #[cfg(unix)]
    let mut extra_pipes = Vec::new();
    #[cfg(unix)]
    let mut inherited_pipe_sources = Vec::new();
    for (index, specification) in specifications.iter().copied().enumerate() {
        if matches!(specification, Specification::Ipc) {
            continue;
        }
        #[cfg(unix)]
        let descriptor = if matches!(specification, Specification::Pipe) {
            let (parent, child) = spawn_socket_pair()?;
            if index < STANDARD_CHANNELS {
                pipes[index] = Some(parent.into());
                Stdio::from(std::os::fd::OwnedFd::from(child))
            } else {
                use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
                parent.set_nonblocking(true)?;
                let output = parent.as_fd().try_clone_to_owned()?;
                let source_floor = i32::try_from(specifications.len())
                    .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
                let source_fd =
                    unsafe { libc::fcntl(child.as_raw_fd(), libc::F_DUPFD_CLOEXEC, source_floor) };
                if source_fd < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let source = unsafe { OwnedFd::from_raw_fd(source_fd) };
                drop(child);
                inherited_pipe_sources.push((source, index));
                extra_pipes.push((index, parent.into(), output));
                // The extra descriptor is installed in pre_exec below.
                continue;
            }
        } else {
            if index >= STANDARD_CHANNELS {
                // Extra entries that are not pipes require no parent-side
                // stream; their descriptor policies are handled elsewhere.
                continue;
            }
            specification.os()?
        };
        #[cfg(not(unix))]
        let descriptor = {
            if index >= STANDARD_CHANNELS {
                continue;
            }
            specification.os()?
        };
        match index {
            0 => {
                command.stdin(descriptor);
            }
            1 => {
                command.stdout(descriptor);
            }
            2 => {
                command.stderr(descriptor);
            }
            _ => unreachable!(),
        }
    }
    #[cfg(unix)]
    if !inherited_pipe_sources.is_empty() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        // All source descriptors are duplicated above the child fd layout,
        // so none can be overwritten by another target during dup2.
        unsafe {
            command.pre_exec(move || {
                for (source, target) in &inherited_pipe_sources {
                    let target = i32::try_from(*target)
                        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
                    if libc::dup2(source.as_raw_fd(), target) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    // NODE_CHANNEL_FD is a child bootstrap contract, never inherited into a
    // grandchild that was spawned without its own channel. Node's JS spawn
    // adapter puts the selected IPC serialization mode into envPairs; preserve
    // it here so advanced IPC (used by cluster) reaches both processes.
    command.env_remove("NODE_CHANNEL_FD");
    command.env_remove("NODE_CHANNEL_SERIALIZATION_MODE");
    if let Some(index) = ipc_index {
        command.env("NODE_CHANNEL_FD", index.to_string());
        command.env("NODE_CHANNEL_SERIALIZATION_MODE", serialization_mode);
        // The test harness selects Quench through this environment variable
        // while embedding it in run-parallel. A fork with an explicit
        // `execPath` (including a copied Quench binary) must report the
        // executable it actually launched inside the child, rather than
        // inheriting the parent's selected path.
        if std::env::var_os("QUENCH_NODE_TEST_EXECUTABLE").is_some() {
            let executable = command.get_program().to_os_string();
            command.env("QUENCH_NODE_TEST_EXECUTABLE", executable);
        }
    }
    Ok(Configured {
        ipc: channel,
        #[cfg(unix)]
        pipes,
        #[cfg(unix)]
        extra_pipes,
    })
}

#[cfg(unix)]
fn spawn_socket_pair() -> std::io::Result<(
    std::os::unix::net::UnixStream,
    std::os::unix::net::UnixStream,
)> {
    use std::os::{fd::AsRawFd, unix::net::UnixStream};
    let (parent, child) = UnixStream::pair()?;
    // Match libuv's spawned pipe endpoints, including the Linux kernel's
    // doubling of these requested socket buffer capacities.
    const SPAWN_PIPE_SOCKET_BUFFER_BYTES: libc::c_int = 64 * 1024;
    for socket in [&parent, &child] {
        for option in [libc::SO_RCVBUF, libc::SO_SNDBUF] {
            let result = unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    option,
                    (&SPAWN_PIPE_SOCKET_BUFFER_BYTES as *const libc::c_int).cast(),
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                )
            };
            if result != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok((parent, child))
}

#[cfg(unix)]
fn ipc_channel(
    command: &mut std::process::Command,
    index: usize,
    count: usize,
    advanced: bool,
) -> std::io::Result<IpcChannel> {
    use std::os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    };
    let (parent, child) = spawn_socket_pair()?;
    parent.set_nonblocking(true)?;
    if index < STANDARD_CHANNELS {
        let descriptor: OwnedFd = child.into();
        match index {
            0 => {
                command.stdin(Stdio::from(descriptor));
            }
            1 => {
                command.stdout(Stdio::from(descriptor));
            }
            2 => {
                command.stderr(Stdio::from(descriptor));
            }
            _ => unreachable!(),
        }
    } else {
        // Move the source beyond the entire child descriptor layout, so dup2
        // cannot clobber it or collide with a standard-stream mapping.
        let minimum =
            i32::try_from(count).map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
        let target =
            i32::try_from(index).map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
        let duplicate = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_DUPFD_CLOEXEC, minimum) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let source = unsafe { OwnedFd::from_raw_fd(duplicate) };
        // The closure owns the descriptor until spawn finishes. Only dup2,
        // an async-signal-safe syscall, runs in the forked child.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(source.as_raw_fd(), target) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    Ok(IpcChannel::new(parent, advanced))
}

#[cfg(not(unix))]
fn ipc_channel(
    _: &mut std::process::Command,
    _: usize,
    _: usize,
    _: bool,
) -> std::io::Result<IpcChannel> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "IPC channels are unavailable on this platform",
    ))
}
