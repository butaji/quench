//! `process.execve`'s system call. Node's `wrapProcessMethods` validates the path, the
//! arguments and the environment and emits the experimental warning and the diagnostics
//! channel message; this native then replaces the process image as Node's `Execve` does.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::ffi::CString;
type Context<'a> = NativeContext<'a, NodeHost>;

/// The descriptors the new image inherits: stdin, stdout and stderr.
const INHERITED_STDIO: [libc::c_int; 3] = [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO];

/// `execve(execPath, args, envPairs)`: `args` and `envPairs` are arrays of strings without
/// NUL, `envPairs` already as `key=value`. Returns only by throwing Node's errno exception.
pub(crate) fn execve(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [path, arguments, environment] = args else {
        return Err(RootedError::host("execve takes a path, arguments and an environment"));
    };
    let path = c.to_string(*path)?;
    let arguments = super::child_process_shared_vm::string_array(c, *arguments)?;
    let environment = super::child_process_shared_vm::string_array(c, *environment)?;
    let c_string = |text: &str| {
        CString::new(text).map_err(|_| RootedError::host("execve strings are validated without NUL"))
    };
    let executable = c_string(&path)?;
    let arguments = arguments.iter().map(|text| c_string(text)).collect::<Result<Vec<_>, _>>()?;
    let environment = environment.iter().map(|text| c_string(text)).collect::<Result<Vec<_>, _>>()?;
    let null_terminated = |strings: &[CString]| {
        strings
            .iter()
            .map(|text| text.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect::<Vec<_>>()
    };
    let (argv, envp) = (null_terminated(&arguments), null_terminated(&environment));
    // Whatever the guest wrote must reach the descriptors before the image goes away.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    for descriptor in INHERITED_STDIO {
        // Like Node, a descriptor that cannot be updated is left as it is: the current
        // process is still in a valid state.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        if flags >= 0 {
            unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) };
        }
    }
    // SAFETY: every pointer names a NUL-terminated string that outlives the call, and both
    // vectors end with a null pointer.
    unsafe { libc::execve(executable.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EINVAL);
    let error = super::process_shared_vm::errno_exception(c, errno, "execve", Some(&path))?;
    Err(c.throw(error))
}
