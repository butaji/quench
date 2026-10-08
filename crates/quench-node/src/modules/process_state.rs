//! Runtime-neutral process inputs and scalar state shared by Node adapters.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub(crate) const NODE_VERSION: &str = "22.0.0";

#[derive(Clone, Copy)]
pub(crate) enum ProcessFact {
    Boolean(bool),
    Number(f64),
    String(&'static str),
    EmptyArray,
}

pub(crate) fn config_variable_facts() -> &'static [(&'static str, ProcessFact)] {
    &[
        ("v8_enable_i18n_support", ProcessFact::Number(1.0)),
        ("node_module_version", ProcessFact::Number(127.0)),
        ("napi_build_version", ProcessFact::String("9")),
        ("node_builtin_shareable_builtins", ProcessFact::EmptyArray),
        ("node_use_lief", ProcessFact::Boolean(false)),
        ("node_use_amaro", ProcessFact::Boolean(false)),
        ("node_use_ffi", ProcessFact::Boolean(false)),
        ("node_shared", ProcessFact::Boolean(false)),
        ("node_shared_openssl", ProcessFact::Boolean(false)),
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
        ("acorn", "8.18.0"),
        ("ada", "2.7.8"),
        ("ares", "1.0.0"),
        ("brotli", "1.1.0"),
        ("cldr", "45.0"),
        ("icu", "75.1"),
        ("llhttp", "9.2.1"),
        ("merve", "1.0.0"),
        ("modules", "127"),
        ("napi", "9"),
        ("nbytes", "1.0.0"),
        ("ncrypto", "1.0.0"),
        ("nghttp2", "1.61.0"),
        ("nghttp3", "1.3.0"),
        ("ngtcp2", "1.4.0"),
        ("openssl", "3.0.0"),
        ("simdjson", "1.0.0"),
        ("simdutf", "5.2.4"),
        ("tz", "2024a"),
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
    cwd.set_path(requested);
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
pub struct ProcessCwd(Rc<RefCell<std::path::PathBuf>>);

impl ProcessCwd {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self(Rc::new(RefCell::new(path)))
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.0.borrow().clone()
    }

    pub fn set_path(&self, path: std::path::PathBuf) {
        *self.0.borrow_mut() = path;
    }

    pub fn join(&self, path: impl AsRef<std::path::Path>) -> std::path::PathBuf {
        self.0.borrow().join(path)
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
    exit_code: Cell<Option<i32>>,
    requested_exit_code: Cell<Option<i32>>,
    exit_emitting: Cell<bool>,
    umask: Cell<u32>,
}

impl ProcessControl {
    pub(crate) fn new() -> Self {
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
            exit_code: Cell::new(None),
            requested_exit_code: Cell::new(None),
            exit_emitting: Cell::new(false),
            umask: Cell::new(umask),
        }))
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

    pub(crate) fn request_exit(&self, code: i32) {
        self.0.exit_code.set(Some(code));
        self.0.requested_exit_code.set(Some(code));
    }

    pub(crate) fn requested_exit_code(&self) -> Option<i32> {
        self.0.requested_exit_code.get()
    }

    pub(crate) fn exit_emitting(&self) -> bool {
        self.0.exit_emitting.get()
    }

    pub(crate) fn begin_exit_emission(&self) {
        self.0.exit_emitting.set(true);
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
