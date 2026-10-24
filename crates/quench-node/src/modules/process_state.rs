//! Runtime-neutral process inputs and scalar state shared by Node adapters.

use std::cell::Cell;
use std::rc::Rc;

/// Compatibility target of the actual bundled Node standard library.
pub(crate) const NODE_VERSION: &str = env!("QUENCH_NODE_COMPAT_VERSION");

pub(crate) fn openssl_version() -> String {
    #[cfg(quench_openssl_provider)]
    {
        let version = openssl::version::version();
        version
            .strip_prefix("OpenSSL ")
            .and_then(|version| version.split_whitespace().next())
            .unwrap_or("3.0.0")
            .to_owned()
    }
    #[cfg(not(quench_openssl_provider))]
    {
        "3.0.0".to_owned()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ProcessFact {
    SharedOpenSsl,
    Boolean(bool),
    Number(f64),
    String(&'static str),
    StringArray(&'static [&'static str]),
}

pub(crate) fn config_variable_facts() -> &'static [(&'static str, ProcessFact)] {
    &[
        ("v8_enable_i18n_support", ProcessFact::Number(1.0)),
        ("node_module_version", ProcessFact::Number(127.0)),
        (
            "napi_build_version",
            ProcessFact::String(env!("QUENCH_NODE_API_SUPPORTED_VERSION_MAX")),
        ),
        (
            "node_builtin_shareable_builtins",
            ProcessFact::StringArray(&["deps/undici/undici.js"]),
        ),
        ("node_use_lief", ProcessFact::Boolean(false)),
        ("node_use_amaro", ProcessFact::Boolean(false)),
        ("node_use_ffi", ProcessFact::Boolean(false)),
        ("node_shared", ProcessFact::Boolean(false)),
        ("node_shared_openssl", ProcessFact::SharedOpenSsl),
    ]
}

pub(crate) fn feature_facts() -> &'static [(&'static str, ProcessFact)] {
    &[
        ("inspector", ProcessFact::Boolean(false)),
        ("debug", ProcessFact::Boolean(false)),
        ("uv", ProcessFact::Boolean(true)),
        ("ipv6", ProcessFact::Boolean(true)),
        ("openssl_is_boringssl", ProcessFact::Boolean(false)),
        ("dtls", ProcessFact::Boolean(false)),
        ("quic", ProcessFact::Boolean(false)),
        ("tls_alpn", ProcessFact::Boolean(true)),
        ("tls_sni", ProcessFact::Boolean(true)),
        ("tls_ocsp", ProcessFact::Boolean(true)),
        ("tls", ProcessFact::Boolean(true)),
        ("cached_builtins", ProcessFact::Boolean(true)),
        ("require_module", ProcessFact::Boolean(true)),
        ("typescript", ProcessFact::String("strip")),
    ]
}

pub(crate) fn version_facts() -> &'static [(&'static str, &'static str)] {
    &[
        ("node", NODE_VERSION),
        ("acorn", "8.19.0"),
        ("ada", "2.7.8"),
        #[cfg(quench_node_cares)]
        ("ares", super::cares_channel_shared_vm::VERSION),
        #[cfg(not(quench_node_cares))]
        ("ares", "1.0.0"),
        ("brotli", "1.1.0"),
        ("cldr", "45.0"),
        #[cfg(quench_icu)]
        ("icu", env!("QUENCH_ICU_VERSION")),
        ("llhttp", "9.2.1"),
        ("merve", "1.0.0"),
        ("modules", "127"),
        ("napi", env!("QUENCH_NODE_API_SUPPORTED_VERSION_MAX")),
        ("nbytes", "1.0.0"),
        ("ncrypto", "1.0.0"),
        ("nghttp2", "1.61.0"),
        ("nghttp3", "1.3.0"),
        ("ngtcp2", "1.4.0"),
        ("openssl", "3.0.0"),
        ("simdjson", "1.0.0"),
        ("simdutf", "5.2.4"),
        ("tz", "2024a"),
        ("undici", "8.12.2"),
        ("unicode", "15.1"),
        ("uv", "1.48.0"),
        ("uvwasi", "1.0.0"),
        ("v8", "12.4.254.21-node.20"),
        ("zlib", "1.3.0"),
        ("zstd", "1.0.0"),
    ]
}

pub(crate) fn platform() -> String {
    std::env::var("QUENCH_PLATFORM").unwrap_or_else(|_| current_platform().into())
}

pub(crate) fn architecture() -> &'static str {
    current_architecture()
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        "unknown"
    }
}

fn current_architecture() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    }
}

pub(crate) struct ChdirError {
    pub code: &'static str,
    pub errno: i32,
    pub path: String,
    pub destination: String,
    pub message: String,
}

pub(crate) fn change_directory_cwd(cwd: &ProcessCwd, destination: &str) -> Result<(), ChdirError> {
    let previous = cwd.path();
    let destination_path = std::path::Path::new(destination);
    let requested = lexical_cwd(&previous, destination_path);
    let metadata = std::fs::metadata(&requested)
        .map_err(|error| chdir_error(&previous, destination, error))?;
    if !metadata.is_dir() {
        return Err(chdir_error(
            &previous,
            destination,
            std::io::Error::from_raw_os_error(libc::ENOTDIR),
        ));
    }
    cwd.set_path(requested)
        .map_err(|error| chdir_error(&previous, destination, error))?;
    Ok(())
}

pub(crate) fn chdir_error(
    previous: &std::path::Path,
    destination: &str,
    error: std::io::Error,
) -> ChdirError {
    let (code, errno) = crate::modules::fs_error_codes::code_for(&error);
    let message = format!(
        "{code}: {}, chdir '{}' -> '{destination}'",
        crate::modules::fs_error_codes::strerror(code),
        previous.display(),
    );
    ChdirError {
        code,
        errno,
        path: previous.to_string_lossy().into_owned(),
        destination: destination.to_owned(),
        message,
    }
}

pub(crate) fn lexical_cwd(
    previous: &std::path::Path,
    requested: &std::path::Path,
) -> std::path::PathBuf {
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        previous.join(requested)
    };
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnhandledRejectionMode {
    Throw,
    Strict,
    Warn,
    None,
}

impl UnhandledRejectionMode {
    pub fn from_exec_argv(exec_argv: &[String]) -> Result<Self, String> {
        let mode = exec_argv
            .iter()
            .filter_map(|argument| argument.strip_prefix("--unhandled-rejections="))
            .next_back();
        match mode {
            None | Some("throw") => Ok(Self::Throw),
            Some("strict") => Ok(Self::Strict),
            Some("warn") => Ok(Self::Warn),
            Some("none") => Ok(Self::None),
            Some(value) => Err(format!("invalid --unhandled-rejections mode: {value}")),
        }
    }
}

/// The logical working directory used by the shared Node host.
///
/// Process and path consumers observe the same owner, so cwd changes remain
/// visible without passing runtime values across the host boundary.
#[derive(Clone)]
pub struct ProcessCwd(std::sync::Arc<std::sync::Mutex<WorkingDirectory>>);

struct WorkingDirectory {
    path: std::path::PathBuf,
    observed_path: Option<std::path::PathBuf>,
    #[cfg(target_os = "linux")]
    directory: Option<std::sync::Arc<std::fs::File>>,
}

#[cfg(target_os = "linux")]
fn open_directory(path: &std::path::Path) -> std::io::Result<std::sync::Arc<std::fs::File>> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_PATH retains directory identity without requiring read permission.
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_DIRECTORY)
        .open(path)
        .map(std::sync::Arc::new)
}

impl ProcessCwd {
    pub fn from_current_directory() -> Self {
        let path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
        Self(std::sync::Arc::new(std::sync::Mutex::new(WorkingDirectory {
            path,
            observed_path: None,
            // The inherited directory can remain valid after its pathname is removed.
            #[cfg(target_os = "linux")]
            directory: open_directory(std::path::Path::new(".")).ok(),
        })))
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.current_path()
            .unwrap_or_else(|_| self.0.lock().expect("cwd lock poisoned").path.clone())
    }

    /// Node caches successful process.cwd observations until process.chdir.
    /// Relative filesystem and child operations continue to use directory identity.
    pub(crate) fn query_path(&self) -> std::io::Result<std::path::PathBuf> {
        if let Some(path) = self.0.lock().expect("cwd lock poisoned").observed_path.clone() {
            return Ok(path);
        }
        let path = self.current_path()?;
        self.0.lock().expect("cwd lock poisoned").observed_path = Some(path.clone());
        Ok(path)
    }

    pub(crate) fn current_path(&self) -> std::io::Result<std::path::PathBuf> {
        let state = self.0.lock().expect("cwd lock poisoned");
        #[cfg(target_os = "linux")]
        if let Some(directory) = &state.directory {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::MetadataExt;
            if directory.metadata()?.nlink() == 0 {
                return Err(std::io::Error::from_raw_os_error(libc::ENOENT));
            }
            // Resolve the live inode after a rename; a missing procfs preserves
            // the cached path used on other platforms.
            if let Ok(path) = std::fs::read_link(format!("/proc/self/fd/{}", directory.as_raw_fd()))
            {
                return Ok(path);
            }
        }
        Ok(state.path.clone())
    }

    pub fn set_path(&self, path: std::path::PathBuf) -> std::io::Result<()> {
        #[cfg(target_os = "linux")]
        let directory = Some(open_directory(&path)?);
        *self.0.lock().expect("cwd lock poisoned") = WorkingDirectory {
            path,
            observed_path: None,
            #[cfg(target_os = "linux")]
            directory,
        };
        Ok(())
    }

    pub fn join(&self, path: impl AsRef<std::path::Path>) -> std::path::PathBuf {
        self.0.lock().expect("cwd lock poisoned").path.join(path)
    }

    /// Resolve an I/O path against the retained directory inode, including after rename.
    pub(crate) fn resolve_io_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        if path.is_absolute() || path.as_os_str().is_empty() {
            return path.to_owned();
        }
        #[cfg(target_os = "linux")]
        if let Some(directory) = &self.0.lock().expect("cwd lock poisoned").directory {
            use std::os::fd::AsRawFd;
            let directory =
                std::path::PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
            if directory.is_dir() {
                return directory.join(path);
            }
        }
        self.path().join(path)
    }

    /// Resolve relative child cwd against the retained directory at the OS boundary.
    pub(crate) fn configure_child_command(
        &self,
        command: &mut std::process::Command,
        requested: Option<&str>,
    ) -> std::io::Result<()> {
        let requested = requested.filter(|path| !path.is_empty());
        if let Some(path) = requested.filter(|path| std::path::Path::new(path).is_absolute()) {
            command.current_dir(path);
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        if let Some(directory) = self.0.lock().expect("cwd lock poisoned").directory.clone() {
            use std::os::fd::AsRawFd;
            use std::os::unix::process::CommandExt;
            let relative = requested
                .map(std::ffi::CString::new)
                .transpose()
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "cwd contains a null byte",
                    )
                })?;
            // Only async-signal-safe directory syscalls run between fork and exec.
            // The captured CLOEXEC descriptor stays owned until command completion.
            unsafe {
                command.pre_exec(move || {
                    if libc::fchdir(directory.as_raw_fd()) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if let Some(relative) = &relative {
                        if libc::chdir(relative.as_ptr()) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
            return Ok(());
        }
        command.current_dir(match requested {
            Some(path) => self.join(path),
            None => self.path(),
        });
        Ok(())
    }
}

/// Immutable startup arguments supplied to the shared Node process adapter.
/// The exposed JavaScript `process.argv` array is materialized from this host
/// input and remains independently mutable.
#[derive(Clone)]
pub struct ProcessArgs(Rc<[String]>);

impl ProcessArgs {
    pub fn new(args: Vec<String>) -> Self {
        Self(Rc::from(args))
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// Process-wide scalar state for `SharedNodeState`. The exposed JavaScript
/// process object remains adapter-owned.
#[derive(Clone, Debug)]
pub(crate) struct ProcessControl(Rc<ProcessControlCells>);

#[derive(Debug)]
struct ProcessControlCells {
    started: std::time::Instant,
    performance_origins: Option<(f64, f64)>,
    loop_start_ns: Cell<Option<f64>>,
    loop_idle_ns: Cell<f64>,
    loop_count: Cell<u64>,
    exit_code: Cell<Option<i32>>,
    requested_exit: Cell<Option<RequestedExit>>,
    umask: Cell<u32>,
}

#[derive(Clone, Copy, Debug)]
enum RequestedExit {
    Immediate(i32),
    FatalExceptionHandler(i32),
    FatalUncaught(i32),
}

impl ProcessControl {
    pub(crate) fn new() -> Self {
        Self::with_performance_origins(sample_performance_origins())
    }

    /// Create a worker's process control using the owning process's performance
    /// origin. Node workers share one process time origin even though each
    /// worker owns its own runtime and uptime counter.
    pub(crate) fn with_performance_origins(performance_origins: Option<(f64, f64)>) -> Self {
        #[cfg(unix)]
        let umask = unsafe {
            let current = libc::umask(0);
            libc::umask(current);
            current as u32
        };
        #[cfg(not(unix))]
        let umask = INITIAL_UMASK;
        Self(Rc::new(ProcessControlCells {
            started: std::time::Instant::now(),
            performance_origins,
            loop_start_ns: Cell::new(None),
            loop_idle_ns: Cell::new(0.0),
            loop_count: Cell::new(0),
            exit_code: Cell::new(None),
            requested_exit: Cell::new(None),
            umask: Cell::new(umask),
        }))
    }

    pub(crate) fn performance_origins(&self) -> Option<(f64, f64)> {
        self.0.performance_origins
    }

    /// Return the process-wide monotonic clock coordinate used by Node's
    /// `process.hrtime()` and performance milestones. Unlike `uptime()`, this
    /// remains aligned for workers that inherit the parent process origin.
    pub(crate) fn monotonic_now_ns(&self) -> Option<f64> {
        monotonic_time_ns()
    }

    pub(crate) fn performance_elapsed_ms(&self) -> f64 {
        self.0
            .performance_origins
            .and_then(|(origin_ns, _)| {
                monotonic_time_ns().map(|now_ns| (now_ns - origin_ns) / 1_000_000.0)
            })
            .unwrap_or_else(|| self.uptime() * 1_000.0)
    }

    pub(crate) fn start_event_loop(&self) {
        if self.0.loop_start_ns.get().is_none() {
            // Workers inherit the process time origin, but their uptime starts
            // when the worker host is created. Combining those values shifts
            // a worker's loop start back to before that worker existed.
            let start = self.0.performance_origins.and_then(|_| monotonic_time_ns());
            self.0.loop_start_ns.set(start);
        }
    }

    pub(crate) fn loop_start_ns(&self) -> Option<f64> {
        self.0.loop_start_ns.get()
    }

    pub(crate) fn record_event_loop_iteration(&self) {
        self.0
            .loop_count
            .set(self.0.loop_count.get().saturating_add(1));
    }

    pub(crate) fn event_loop_iterations(&self) -> u64 {
        self.0.loop_count.get()
    }

    pub(crate) fn record_event_loop_idle(&self, duration: std::time::Duration) {
        self.0
            .loop_idle_ns
            .set(self.0.loop_idle_ns.get() + duration.as_secs_f64() * 1_000_000_000.0);
    }

    pub(crate) fn event_loop_idle_ms(&self) -> f64 {
        self.0.loop_idle_ns.get() / 1_000_000.0
    }

    pub(crate) fn uptime(&self) -> f64 {
        self.0.started.elapsed().as_secs_f64()
    }

    pub(crate) fn exit_code(&self) -> Option<i32> {
        self.0.exit_code.get()
    }

    pub(crate) fn set_exit_code(&self, code: Option<i32>) {
        self.0.exit_code.set(code);
    }

    pub(crate) fn request_really_exit(&self, code: i32) {
        self.0.exit_code.set(Some(code));
        self.0
            .requested_exit
            .set(Some(RequestedExit::Immediate(code)));
    }

    pub(crate) fn request_fatal_uncaught(&self, code: i32) {
        self.0.exit_code.set(Some(code));
        self.0
            .requested_exit
            .set(Some(RequestedExit::FatalUncaught(code)));
    }
    pub(crate) fn fatal_uncaught_requested(&self) -> bool {
        matches!(
            self.0.requested_exit.get(),
            Some(RequestedExit::FatalUncaught(_))
        )
    }

    pub(crate) fn fail_fatal_exception_handler(&self) {
        let code =
            super::node_protocol_constants::ExitCode::ExceptionInFatalExceptionHandler as i32;
        self.0.exit_code.set(Some(code));
        self.0
            .requested_exit
            .set(Some(RequestedExit::FatalExceptionHandler(code)));
    }

    pub(crate) fn fatal_exception_handler_failed(&self) -> bool {
        matches!(
            self.0.requested_exit.get(),
            Some(RequestedExit::FatalExceptionHandler(_))
        )
    }

    pub(crate) fn requested_exit_code(&self) -> Option<i32> {
        self.0.requested_exit.get().map(|request| match request {
            RequestedExit::Immediate(code)
            | RequestedExit::FatalExceptionHandler(code)
            | RequestedExit::FatalUncaught(code) => code,
        })
    }

    pub(crate) fn exit_without_event(&self) -> bool {
        matches!(
            self.0.requested_exit.get(),
            Some(
                RequestedExit::Immediate(_)
                    | RequestedExit::FatalExceptionHandler(_)
                    | RequestedExit::FatalUncaught(_)
            )
        )
    }

    pub(crate) fn umask(&self) -> u32 {
        self.0.umask.get()
    }

    pub(crate) fn update_umask(&self, mask: u32) -> u32 {
        let mask = mask & UMASK_BITS;
        #[cfg(unix)]
        unsafe {
            libc::umask(mask as libc::mode_t);
        }
        self.0.umask.replace(mask)
    }
}

#[cfg(not(unix))]
const INITIAL_UMASK: u32 = 0o022;
const UMASK_BITS: u32 = 0o777;

/// Derive linkage from loaded images, without guessing from a library name or
/// a build environment that may differ from the executable being run.
pub(crate) fn shared_openssl() -> bool {
    #[cfg(unix)]
    unsafe {
        let mut host: libc::Dl_info = std::mem::zeroed();
        let mut ssl: libc::Dl_info = std::mem::zeroed();
        libc::dladdr(
            shared_openssl as *const () as *const libc::c_void,
            &mut host,
        ) != 0
            && libc::dladdr(
                openssl_sys::SSL_CTX_new as *const () as *const libc::c_void,
                &mut ssl,
            ) != 0
            && host.dli_fbase != ssl.dli_fbase
    }
    #[cfg(not(unix))]
    false
}

// Capture actual clock coordinates at environment creation, rather than synthesizing
// a zero origin when a performance module happens to be imported later.
fn sample_performance_origins() -> Option<(f64, f64)> {
    #[cfg(unix)]
    {
        const NANOS_PER_SECOND: f64 = 1_000_000_000.0;
        const MICROS_PER_SECOND: f64 = 1_000_000.0;
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
            return None;
        }
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some((
            time.tv_sec as f64 * NANOS_PER_SECOND + time.tv_nsec as f64,
            wall.as_secs_f64() * MICROS_PER_SECOND,
        ))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn monotonic_time_ns() -> Option<f64> {
    #[cfg(unix)]
    {
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
            return None;
        }
        Some(time.tv_sec as f64 * 1_000_000_000.0 + time.tv_nsec as f64)
    }
    #[cfg(not(unix))]
    {
        None
    }
}
