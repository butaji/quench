//! The `tty` module: `isatty` and the terminal streams `ReadStream` and `WriteStream`,
//! duplex streams over a terminal descriptor, with their window size, color depth and
//! cursor helpers.

use super::stream_shared_vm::{arg, call_method, captures, get, inherit, is_undefined, put, put_bool, Context, Outcome};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};

const MODULE_CACHE: &str = "\0builtin:tty";

/// The color depths `getColorDepth` reports: bits per color.
const COLORS_2: f64 = 1.0;
const COLORS_16: f64 = 4.0;
const COLORS_256: f64 = 8.0;
const COLORS_16M: f64 = 24.0;

/// The colors `hasColors()` asks about by default.
const DEFAULT_COLOR_COUNT: f64 = 16.0;
/// The fewest colors `hasColors(count)` accepts.
const MINIMUM_COLOR_COUNT: f64 = 2.0;

/// `TERM` values with a known depth.
const TERM_DEPTHS: &[(&str, f64)] = &[
    ("eterm", COLORS_16),
    ("cons25", COLORS_16),
    ("console", COLORS_16),
    ("cygwin", COLORS_16),
    ("dtterm", COLORS_16),
    ("gnome", COLORS_16),
    ("hurd", COLORS_16),
    ("jfbterm", COLORS_16),
    ("konsole", COLORS_16),
    ("kterm", COLORS_16),
    ("mlterm", COLORS_16),
    ("mosh", COLORS_16M),
    ("putty", COLORS_16),
    ("st", COLORS_16),
    ("rxvt-unicode-24bit", COLORS_16M),
    ("terminator", COLORS_16M),
    ("xterm-kitty", COLORS_16M),
];

/// The CI services whose variable names a depth.
const CI_DEPTHS: &[(&str, f64)] = &[
    ("APPVEYOR", COLORS_256),
    ("BUILDKITE", COLORS_256),
    ("CIRCLECI", COLORS_16M),
    ("DRONE", COLORS_256),
    ("GITEA_ACTIONS", COLORS_16M),
    ("GITHUB_ACTIONS", COLORS_16M),
    ("GITLAB_CI", COLORS_256),
    ("TRAVIS", COLORS_256),
];

/// How a `TERM` pattern matches a lowercased `TERM`.
enum Pattern {
    Contains(&'static str),
    Prefix(&'static str),
    /// `/^con[0-9]*x[0-9]/`.
    ConsoleSize,
}

/// The `TERM` patterns of 16-color terminals (`TERM_ENVS_REG_EXP`).
const SIXTEEN_COLOR_TERMS: &[Pattern] = &[
    Pattern::Contains("ansi"),
    Pattern::Contains("color"),
    Pattern::Contains("linux"),
    Pattern::Contains("direct"),
    Pattern::ConsoleSize,
    Pattern::Prefix("rxvt"),
    Pattern::Prefix("screen"),
    Pattern::Prefix("xterm"),
    Pattern::Prefix("vt100"),
    Pattern::Prefix("vt220"),
];

impl Pattern {
    fn matches(&self, term: &str) -> bool {
        match self {
            Self::Contains(part) => term.contains(part),
            Self::Prefix(prefix) => term.starts_with(prefix),
            Self::ConsoleSize => term
                .strip_prefix("con")
                .map(|rest| rest.trim_start_matches(|ch: char| ch.is_ascii_digit()))
                .and_then(|rest| rest.strip_prefix('x'))
                .is_some_and(|rest| rest.starts_with(|ch: char| ch.is_ascii_digit())),
        }
    }
}

/// The environment `getColorDepth` reads: each variable, or `None` when unset.
trait Environment {
    fn variable(&self, name: &str) -> Option<String>;
    fn has(&self, name: &str) -> bool;
}

/// `getColorDepth(env)` (internal/tty.js) on a Unix platform.
fn color_depth(env: &impl Environment) -> f64 {
    if let Some(force) = env.variable("FORCE_COLOR") {
        return match force.as_str() {
            "" | "1" | "true" => COLORS_16,
            "2" => COLORS_256,
            "3" => COLORS_16M,
            _ => COLORS_2,
        };
    }
    if env.variable("NODE_DISABLE_COLORS").is_some()
        || env.variable("NO_COLOR").is_some()
        || env.variable("TERM").as_deref() == Some("dumb")
    {
        return COLORS_2;
    }
    if env.variable("TMUX").is_some_and(|value| !value.is_empty()) {
        return COLORS_16M;
    }
    if env.has("TF_BUILD") && env.has("AGENT_NAME") {
        return COLORS_16;
    }
    if env.has("CI") {
        if let Some((_, depth)) = CI_DEPTHS.iter().find(|(name, _)| env.has(name)) {
            return *depth;
        }
        return if env.variable("CI_NAME").as_deref() == Some("codeship") { COLORS_256 } else { COLORS_2 };
    }
    if let Some(version) = env.variable("TEAMCITY_VERSION") {
        return if teamcity_supports_color(&version) { COLORS_16 } else { COLORS_2 };
    }
    match env.variable("TERM_PROGRAM").as_deref() {
        Some("iTerm.app") => {
            let version = env.variable("TERM_PROGRAM_VERSION").unwrap_or_default();
            let old = version.is_empty() || (version.len() > 1 && matches!(&version[..2], "0." | "1." | "2."));
            return if old { COLORS_256 } else { COLORS_16M };
        }
        Some("HyperTerm" | "MacTerm") => return COLORS_16M,
        Some("Apple_Terminal") => return COLORS_256,
        _ => {}
    }
    if matches!(env.variable("COLORTERM").as_deref(), Some("truecolor" | "24bit")) {
        return COLORS_16M;
    }
    if let Some(term) = env.variable("TERM").filter(|term| !term.is_empty()) {
        if term.contains("truecolor") {
            return COLORS_16M;
        }
        if term.starts_with("xterm-256") {
            return COLORS_256;
        }
        let lower = term.to_lowercase();
        if let Some((_, depth)) = TERM_DEPTHS.iter().find(|(name, _)| *name == lower) {
            return *depth;
        }
        if SIXTEEN_COLOR_TERMS.iter().any(|pattern| pattern.matches(&lower)) {
            return COLORS_16;
        }
    }
    if env.variable("COLORTERM").is_some_and(|value| !value.is_empty()) {
        return COLORS_16;
    }
    COLORS_2
}

/// `/^(9\.(0*[1-9]\d*)\.|\d{2,}\.)/`.
fn teamcity_supports_color(version: &str) -> bool {
    let digits = version.chars().take_while(char::is_ascii_digit).count();
    let after = &version[digits..];
    if digits >= 2 && after.starts_with('.') {
        return true;
    }
    if digits == 1 && version.starts_with('9') {
        let minor = after.strip_prefix('.').unwrap_or("");
        let zeros = minor.chars().take_while(|ch| *ch == '0').count();
        let rest = &minor[zeros..];
        let significant = rest.chars().take_while(char::is_ascii_digit).count();
        return after.starts_with('.') && significant > 0 && !rest.starts_with('0') && rest[significant..].starts_with('.');
    }
    false
}

/// A JavaScript object read as an environment.
struct ObjectEnvironment<'c, 'a> {
    context: std::cell::RefCell<&'c mut Context<'a>>,
    object: RootId,
}

impl Environment for ObjectEnvironment<'_, '_> {
    fn variable(&self, name: &str) -> Option<String> {
        let mut context = self.context.borrow_mut();
        let value = context.get_named_property_rooted(self.object, name).ok()?;
        if is_undefined(&context, value) {
            return None;
        }
        context.to_string(value).ok()
    }

    fn has(&self, name: &str) -> bool {
        let mut context = self.context.borrow_mut();
        context.has_own_property_rooted(self.object, name).unwrap_or(false)
    }
}

fn environment_object(c: &mut Context<'_>, given: RootId) -> Outcome {
    if !is_undefined(c, given) {
        return Ok(given);
    }
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    get(c, process, "env")
}

fn depth_of(c: &mut Context<'_>, env: RootId) -> Result<f64, RootedError> {
    let env = environment_object(c, env)?;
    let environment = ObjectEnvironment { context: std::cell::RefCell::new(c), object: env };
    Ok(color_depth(&environment))
}

/// `WriteStream.prototype.getColorDepth([env])`.
pub(crate) fn get_color_depth(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let env = arg(c, args, 0);
    let depth = depth_of(c, env)?;
    Ok(c.number(depth))
}

/// `WriteStream.prototype.hasColors([count][, env])`.
pub(crate) fn has_colors(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (count, mut env) = (arg(c, args, 0), arg(c, args, 1));
    let count = if is_undefined(c, env) && (is_undefined(c, count) || c.is_object_rooted(count)?) {
        env = count;
        DEFAULT_COLOR_COUNT
    } else {
        let maximum = c.number(super::buffer_module::MAX_LENGTH);
        super::util_shared_vm::validate_integer(c, count, "count", MINIMUM_COLOR_COUNT, maximum)?
    };
    let depth = depth_of(c, env)?;
    Ok(c.boolean(count <= depth.exp2()))
}

// ---------------------------------------------------------------------------------------
// Descriptors
// ---------------------------------------------------------------------------------------

/// `isatty(fd)`.
pub(crate) fn isatty(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Outcome {
    let terminal = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|fd| fd.fract() == 0.0 && (0.0..=f64::from(i32::MAX)).contains(fd))
        .is_some_and(|fd| super::tty_state::is_terminal_fd(fd as i32));
    Ok(context.boolean(terminal))
}

/// The descriptor a stream is made over: `fd >> 0 === fd && fd >= 0`, and a terminal.
fn terminal_descriptor(c: &mut Context<'_>, value: RootId) -> Result<i32, RootedError> {
    let number = c.rooted_value(value).and_then(|value| value.as_number());
    let Some(fd) = number.filter(|fd| fd.fract() == 0.0 && (0.0..=f64::from(i32::MAX)).contains(fd)) else {
        let shown = c.to_string(value)?;
        let built = c.range_error_rooted(&format!("\"fd\" must be a positive integer: {shown}")).and_then(|error| {
            super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_INVALID_FD")?;
            Ok(error)
        });
        return Err(thrown(c, built));
    };
    let fd = fd as i32;
    if !super::tty_state::is_terminal_fd(fd) {
        return Err(init_failed(c));
    }
    Ok(fd)
}

fn thrown(c: &mut Context<'_>, built: Outcome) -> RootedError {
    match built {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// `ERR_TTY_INIT_FAILED`: `uv_tty_init` refused a descriptor that is not a terminal.
fn init_failed(c: &mut Context<'_>) -> RootedError {
    let built = (|| {
        let (errno, code, description) = super::util_system_error_shared_vm::UV_ERRORS
            .iter()
            .copied()
            .find(|(errno, ..)| *errno == -libc::EINVAL)
            .ok_or_else(|| RootedError::host("EINVAL missing from the uv error table"))?;
        const SYSCALL: &str = "uv_tty_init";
        let error = c.error_rooted(&format!("TTY initialization failed: {SYSCALL} returned {code} ({description})"))?;
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_TTY_INIT_FAILED")?;
        let name = c.string_rooted("SystemError");
        let key = c.string_rooted("name");
        c.define_data_property_rooted(error, key, name, true, false, true)?;
        let info = c.object_rooted()?;
        let errno = c.number(f64::from(errno));
        put(c, info, "errno", errno)?;
        let code = c.string_rooted(code);
        put(c, info, "code", code)?;
        let message = c.string_rooted(description);
        put(c, info, "message", message)?;
        let syscall = c.string_rooted(SYSCALL);
        put(c, info, "syscall", syscall)?;
        put(c, error, "info", info)?;
        put(c, error, "errno", errno)?;
        put(c, error, "syscall", syscall)?;
        Ok(error)
    })();
    thrown(c, built)
}

/// The terminal's `[columns, rows]`, or `None` when the size is unknown.
fn window_size(fd: i32) -> Option<(u16, u16)> {
    // SAFETY: TIOCGWINSZ writes one `winsize` into the pointer it is given.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) };
    (result == 0).then_some((size.ws_col, size.ws_row))
}

fn descriptor(c: &mut Context<'_>, stream: RootId) -> Result<i32, RootedError> {
    let fd = get(c, stream, "fd")?;
    Ok(c.to_number(fd)? as i32)
}

/// The `Duplex` the streams extend.
fn duplex(c: &mut Context<'_>) -> Outcome {
    let stream = super::stream_shared_vm::module(c)?;
    get(c, stream, "Duplex")
}

/// `Duplex.call(this, { readableHighWaterMark: 0, ...options })`.
fn initialize_duplex(c: &mut Context<'_>, this: RootId, options: RootId) -> Result<(), RootedError> {
    let defaults = c.object_rooted()?;
    super::stream_shared_vm::put_number(c, defaults, "readableHighWaterMark", 0.0)?;
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let merged = c.object_rooted()?;
    let merged = call_method(c, object, "assign", &[merged, defaults, options])?;
    let class = duplex(c)?;
    c.call_rooted(class, this, &[merged])?;
    Ok(())
}

fn needs_new(c: &mut Context<'_>, this: RootId) -> Result<Option<RootId>, RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(None);
    }
    let [class] = captures(c)?;
    if c.is_object_rooted(this)? && c.instance_of_rooted(this, class)? {
        return Ok(None);
    }
    Ok(Some(class))
}

/// `new ReadStream(fd[, options])`.
pub(crate) fn read_stream(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (fd, options) = (arg(c, args, 0), arg(c, args, 1));
    if let Some(class) = needs_new(c, this)? {
        return c.construct_rooted(class, class, &[fd, options]);
    }
    let fd = terminal_descriptor(c, fd)?;
    initialize_duplex(c, this, options)?;
    let fd = c.number(f64::from(fd));
    put(c, this, "fd", fd)?;
    put_bool(c, this, "isRaw", false)?;
    put_bool(c, this, "isTTY", true)?;
    Ok(c.undefined())
}

/// `new WriteStream(fd)`.
pub(crate) fn write_stream(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let fd = arg(c, args, 0);
    if let Some(class) = needs_new(c, this)? {
        return c.construct_rooted(class, class, &[fd]);
    }
    let fd = terminal_descriptor(c, fd)?;
    let options = c.object_rooted()?;
    initialize_duplex(c, this, options)?;
    let fd_value = c.number(f64::from(fd));
    put(c, this, "fd", fd_value)?;
    if let Some((columns, rows)) = window_size(fd) {
        super::stream_shared_vm::put_number(c, this, "columns", f64::from(columns))?;
        super::stream_shared_vm::put_number(c, this, "rows", f64::from(rows))?;
    }
    Ok(c.undefined())
}

/// The terminal settings a stream had before it entered raw mode.
#[cfg(unix)]
fn saved_terminals() -> &'static std::sync::Mutex<std::collections::HashMap<i32, libc::termios>> {
    static SAVED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<i32, libc::termios>>> =
        std::sync::OnceLock::new();
    SAVED.get_or_init(Default::default)
}

/// Enter or leave raw mode on `fd`, as `uv_tty_set_mode` does; the OS error on failure.
fn set_raw(fd: i32, raw: bool) -> Result<(), std::io::Error> {
    let mut saved = saved_terminals().lock().unwrap_or_else(|error| error.into_inner());
    if raw {
        // SAFETY: tcgetattr writes one termios for an open descriptor.
        let mut current: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut current) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let original = *saved.entry(fd).or_insert(current);
        let mut settings = original;
        settings.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        settings.c_oflag |= libc::ONLCR;
        settings.c_cflag |= libc::CS8;
        settings.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        settings.c_cc[libc::VMIN] = 1;
        settings.c_cc[libc::VTIME] = 0;
        // SAFETY: tcsetattr reads the termios it is given.
        if unsafe { libc::tcsetattr(fd, libc::TCSADRAIN, &settings) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    } else if let Some(original) = saved.remove(&fd) {
        // SAFETY: as above.
        if unsafe { libc::tcsetattr(fd, libc::TCSADRAIN, &original) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// `ReadStream.prototype.setRawMode(flag)`.
pub(crate) fn set_raw_mode(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let flag = arg(c, args, 0);
    let flag = c.truthy_rooted(flag)?;
    let fd = descriptor(c, this)?;
    match set_raw(fd, flag) {
        Ok(()) => put_bool(c, this, "isRaw", flag)?,
        Err(error) => {
            let error = super::util_system_error_shared_vm::UV_ERRORS
                .iter()
                .find(|(errno, ..)| Some(-*errno) == error.raw_os_error())
                .map(|(_, code, description)| format!("setRawMode {code} ({description})"))
                .unwrap_or_else(|| format!("setRawMode {error}"));
            let error = c.error_rooted(&error)?;
            super::stream_shared_vm::emit(c, this, "error", &[error])?;
        }
    }
    Ok(this)
}

/// The bytes a terminal read takes at most.
pub(crate) const READ_CHUNK_BYTES: usize = 64 * 1024;
/// How long a terminal read waits before polling again, in milliseconds.
const READ_POLL_INTERVAL_MS: f64 = 10.0;

/// `ReadStream.prototype._read()`: push what the terminal has; poll again until it has some.
pub(crate) fn read(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let fd = descriptor(c, this)?;
    let mut ready = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    // SAFETY: poll reads and writes the one pollfd it is given.
    let count = unsafe { libc::poll(&mut ready, 1, 0) };
    if count <= 0 {
        let retry = c.host_function_named_with_data(crate::host::shared_vm::operation!("ttyReadRetry"), "", this)?;
        let global = c.global_root()?;
        let delay = c.number(READ_POLL_INTERVAL_MS);
        call_method(c, global, "setTimeout", &[retry, delay])?;
        return Ok(c.undefined());
    }
    let mut bytes = vec![0; READ_CHUNK_BYTES];
    // SAFETY: read writes at most `bytes.len()` bytes into the buffer.
    let length = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
    if length < 0 {
        let error = std::io::Error::last_os_error();
        let error = c.error_rooted(&error.to_string())?;
        call_method(c, this, "destroy", &[error])?;
        return Ok(c.undefined());
    }
    bytes.truncate(length as usize);
    let chunk = if bytes.is_empty() { c.null() } else { super::buffer_shared_vm::from_vec(c, bytes)? };
    call_method(c, this, "push", &[chunk])?;
    Ok(c.undefined())
}

pub(crate) fn read_retry(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let stream = c.host_function_data()?;
    if super::stream_shared_vm::truthy_member(c, stream, "destroyed")? {
        return Ok(c.undefined());
    }
    let size = c.number(READ_CHUNK_BYTES as f64);
    call_method(c, stream, "_read", &[size])
}

/// `WriteStream.prototype._write(chunk, encoding, callback)`: write it all, blocking, as a
/// terminal handle in blocking mode does.
pub(crate) fn write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, callback) = (arg(c, args, 0), arg(c, args, 2));
    let fd = descriptor(c, this)?;
    let bytes = c.view_bytes_rooted(chunk).unwrap_or_default();
    let mut written = 0;
    let mut failure = None;
    while written < bytes.len() {
        // SAFETY: write reads the remaining bytes of the buffer.
        let count = unsafe { libc::write(fd, bytes[written..].as_ptr().cast(), bytes.len() - written) };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            failure = Some(error);
            break;
        }
        written += count as usize;
    }
    let undefined = c.undefined();
    match failure {
        Some(error) => {
            let error = c.error_rooted(&error.to_string())?;
            c.call_rooted(callback, undefined, &[error])?;
        }
        None => {
            c.call_rooted(callback, undefined, &[])?;
        }
    }
    Ok(undefined)
}

/// `WriteStream.prototype._refreshSize()`: emit `resize` when the window changed.
pub(crate) fn refresh_size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let fd = descriptor(c, this)?;
    let Some((columns, rows)) = window_size(fd) else {
        let error = std::io::Error::last_os_error();
        let error = c.error_rooted(&format!("getWindowSize {error}"))?;
        super::stream_shared_vm::emit(c, this, "error", &[error])?;
        return Ok(c.undefined());
    };
    let (columns, rows) = (f64::from(columns), f64::from(rows));
    let old_columns = get(c, this, "columns")?;
    let old_rows = get(c, this, "rows")?;
    let same = |c: &Context<'_>, value: RootId, now: f64| c.rooted_value(value).and_then(|value| value.as_number()) == Some(now);
    if !same(c, old_columns, columns) || !same(c, old_rows, rows) {
        super::stream_shared_vm::put_number(c, this, "columns", columns)?;
        super::stream_shared_vm::put_number(c, this, "rows", rows)?;
        super::stream_shared_vm::emit(c, this, "resize", &[])?;
    }
    Ok(c.undefined())
}

/// `getWindowSize()`: `[columns, rows]`.
pub(crate) fn get_window_size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let columns = get(c, this, "columns")?;
    let rows = get(c, this, "rows")?;
    c.array_rooted(&[columns, rows])
}

/// The cursor helpers: `readline.<name>(this, ...args)`; the helper closes over its name.
pub(crate) fn cursor_helper(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let name = c.host_function_data()?;
    let name = c.to_string(name)?;
    let readline = super::readline_shared_vm::module(c)?;
    let function = get(c, readline, &name)?;
    let mut values = vec![this];
    values.extend_from_slice(args);
    let undefined = c.undefined();
    c.call_rooted(function, undefined, &values)
}

/// The cursor helpers, each `readline`'s function of that name.
const CURSOR_HELPERS: [&str; 4] = ["cursorTo", "moveCursor", "clearLine", "clearScreenDown"];

fn method(c: &mut Context<'_>, target: RootId, name: &str, operation: &str, data: Option<RootId>) -> Result<(), RootedError> {
    let function = match data {
        Some(data) => c.host_function_named_with_data(crate::host::shared_vm::operation(operation), "", data)?,
        None => c.host_function_named(crate::host::shared_vm::operation(operation), "")?,
    };
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, function, true, true, true)?;
    Ok(())
}

fn class(c: &mut Context<'_>, name: &str, operation: &str, parent: RootId) -> Outcome {
    let data = c.array_rooted(&[])?;
    let constructor = c.host_constructor_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
    put(c, data, "0", constructor)?;
    inherit(c, constructor, parent)?;
    Ok(constructor)
}

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let shared = c.host_mut().shared_state();
    if let Some(module) = shared.borrow().builtin_cache.get(MODULE_CACHE).copied() {
        return Ok(module);
    }
    let parent = duplex(c)?;
    let read_class = class(c, "ReadStream", "ttyReadStream", parent)?;
    let write_class = class(c, "WriteStream", "ttyWriteStream", parent)?;
    let read_prototype = get(c, read_class, "prototype")?;
    method(c, read_prototype, "setRawMode", "ttySetRawMode", None)?;
    let key = c.string_rooted("_read");
    let reader = c.host_function_named(crate::host::shared_vm::operation!("ttyRead"), "_read")?;
    c.define_data_property_rooted(read_prototype, key, reader, true, false, true)?;
    let write_prototype = get(c, write_class, "prototype")?;
    let yes = c.boolean(true);
    let key = c.string_rooted("isTTY");
    c.define_data_property_rooted(write_prototype, key, yes, true, true, true)?;
    method(c, write_prototype, "getColorDepth", "ttyGetColorDepth", None)?;
    method(c, write_prototype, "hasColors", "ttyHasColors", None)?;
    method(c, write_prototype, "_refreshSize", "ttyRefreshSize", None)?;
    for name in CURSOR_HELPERS {
        let data = c.string_rooted(name);
        method(c, write_prototype, name, "ttyCursorHelper", Some(data))?;
    }
    method(c, write_prototype, "getWindowSize", "ttyGetWindowSize", None)?;
    let key = c.string_rooted("_write");
    let writer = c.host_function_named(crate::host::shared_vm::operation!("ttyWrite"), "_write")?;
    c.define_data_property_rooted(write_prototype, key, writer, true, false, true)?;
    let module = c.object_rooted()?;
    let isatty = c.host_function_named(crate::host::shared_vm::operation!("ttyIsatty"), "isatty")?;
    for (name, value) in [("isatty", isatty), ("ReadStream", read_class), ("WriteStream", write_class)] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(module, key, value, true, true, true)?;
    }
    let retained = c.retain(module)?;
    shared.borrow_mut().builtin_cache.insert(MODULE_CACHE.to_owned(), retained);
    Ok(module)
}
