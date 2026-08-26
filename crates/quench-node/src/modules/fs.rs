//! `fs` module — real filesystem operations with Node's coded
//! errors, `Stats`/`Dirent` values, and async variants whose
//! callbacks run on the host event loop.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub struct FsState;

impl Default for FsState {
    fn default() -> Self {
        Self
    }
}

impl FsState {
    pub fn new() -> Self {
        Self
    }
}

/// Parsed `options` argument shared by the sync and async families.
#[derive(Default)]
pub(crate) struct FsOptions {
    pub encoding: Option<String>,
    pub flag: Option<String>,
    pub mode: Option<u32>,
    pub recursive: bool,
    pub force: bool,
    pub with_file_types: bool,
    pub throw_if_no_entry: bool,
    pub signal_aborted: bool,
}

/// `path` argument: string only (Buffer/URL paths unsupported).
pub(crate) fn path_arg(value: Option<&Value>) -> Result<String, VmError> {
    crate::modules::path::validate_string(value.unwrap_or(&Value::Undefined), "path")
}

/// Parse the trailing `options` argument (string encoding or object).
pub(crate) fn parse_options(value: Option<&Value>) -> Result<FsOptions, VmError> {
    let mut options = FsOptions::default();
    match value {
        None | Some(Value::Undefined) | Some(Value::Null) => {}
        Some(Value::String(encoding)) => set_encoding(&mut options, encoding)?,
        Some(object @ Value::Object(_)) => parse_option_object(&mut options, object)?,
        Some(other) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options\" argument must be of type string or an instance of Object.{}",
                crate::modules::util::invalid_arg_received(other)
            )));
        }
    }
    Ok(options)
}

fn set_encoding(options: &mut FsOptions, encoding: &str) -> Result<(), VmError> {
    match crate::modules::buffer_enc::canonical_encoding(encoding) {
        Some(canonical) => {
            options.encoding = Some(canonical.to_string());
            Ok(())
        }
        None => Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The argument 'options' is invalid. Received {encoding:?}"
        ))),
    }
}

fn parse_option_object(options: &mut FsOptions, object: &Value) -> Result<(), VmError> {
    let get = |key: &str| quench_runtime::vm::get_property(object, key);
    if let Value::String(encoding) = get("encoding") {
        match crate::modules::buffer_enc::canonical_encoding(&encoding) {
            Some(canonical) => options.encoding = Some(canonical.to_string()),
            None => {
                return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                    "The argument 'encoding' is invalid encoding. Received '{encoding}'"
                )))
            }
        }
    }
    if let Value::String(flag) = get("flag") {
        options.flag = Some(flag);
    }
    if let Value::Number(mode) = get("mode") {
        options.mode = Some(mode as u32);
    }
    options.recursive = truthy(&get("recursive"));
    options.force = truthy(&get("force"));
    options.with_file_types = truthy(&get("withFileTypes"));
    options.throw_if_no_entry = truthy(&get("throwIfNoEntry"));
    if let signal @ Value::Object(_) = get("signal") {
        options.signal_aborted = truthy(&quench_runtime::vm::get_property(&signal, "aborted"));
    }
    Ok(())
}

pub(crate) fn truthy(value: &Value) -> bool {
    !matches!(
        value,
        Value::Undefined | Value::Null | Value::Boolean(false) | Value::Number(0.0)
    ) && !matches!(value, Value::String(s) if s.is_empty())
}

/// Node's `fs.exists` callback takes a single boolean, no error.
pub(crate) fn require_callback(value: Option<&Value>) -> Result<Value, VmError> {
    match value {
        Some(cb) if quench_runtime::is_callable(cb) => Ok(cb.clone()),
        Some(other) => Err(callback_type_error(other)),
        None => Err(callback_type_error(&Value::Undefined)),
    }
}

fn callback_type_error(value: &Value) -> VmError {
    crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"callback\" argument must be of type function.{}",
        crate::modules::util::invalid_arg_received(value)
    ))
}

/// Queue an async fs callback on the event loop's immediate queue.
pub(crate) fn defer(state: &Rc<RefCell<HostState>>, cb: &Value, args: Vec<Value>) {
    state.borrow().event_loop.queue_immediate(cb.clone(), args);
}

/// Split `args` into `(leading, callback)` for the async family: the
/// callback is always the last argument.
pub(crate) fn async_args(args: &[Value]) -> Result<(&[Value], Value), VmError> {
    let (callback, leading) = match args.split_last() {
        Some((cb, rest)) => (Some(cb), rest),
        None => (None, &[][..]),
    };
    Ok((leading, require_callback(callback)?))
}

/// The error half of an async callback result.
pub(crate) fn err_value(result: &Result<Value, VmError>) -> Value {
    match result {
        Ok(_) => Value::Null,
        Err(VmError::Thrown(value)) => value.clone(),
        Err(_) => host_api::object(vec![(
            "message".to_string(),
            Value::String("I/O error".to_string()),
        )]),
    }
}

pub fn build() -> Value {
    use crate::registry::*;
    let mut props: Vec<(&str, Value)> = vec![
        ("readFile", crate::host::capability(SPEC_FS_READFILE)),
        ("writeFile", crate::host::capability(SPEC_FS_WRITEFILE)),
        ("stat", crate::host::capability(SPEC_FS_STAT)),
        ("lstat", crate::host::capability(SPEC_FS_LSTAT)),
        ("readdir", crate::host::capability(SPEC_FS_READDIR)),
        ("exists", crate::host::capability(SPEC_FS_EXISTS)),
        ("mkdir", crate::host::capability(SPEC_FS_MKDIR)),
        ("unlink", crate::host::capability(SPEC_FS_UNLINK)),
        ("rmdir", crate::host::capability(SPEC_FS_RMDIR)),
        ("rm", crate::host::capability(SPEC_FS_RM)),
        ("rename", crate::host::capability(SPEC_FS_RENAME)),
        ("appendFile", crate::host::capability(SPEC_FS_APPENDFILE)),
        ("copyFile", crate::host::capability(SPEC_FS_COPYFILE)),
        ("access", crate::host::capability(SPEC_FS_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FS_MKDTEMP)),
        ("realpath", crate::host::capability(SPEC_FS_REALPATH)),
        ("watch", crate::host::capability(SPEC_FS_WATCH)),
        ("ReadStream", crate::host::capability(SPEC_FS_READSTREAM)),
        ("WriteStream", crate::host::capability(SPEC_FS_WRITESTREAM)),
        ("opendir", crate::host::capability(SPEC_FS_OPENDIR)),
        ("readlink", crate::host::capability(SPEC_FS_READLINK)),
        ("chmod", crate::host::capability(SPEC_FS_CHMOD)),
        ("truncate", crate::host::capability(SPEC_FS_TRUNCATE)),
    ];
    props.extend(sync_props());
    props.push(("constants", constants()));
    props.push(("promises", promises()));
    crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined)
}

pub fn validate_stream_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    parse_options(args.get(1))?;
    Ok(host_api::object(vec![]))
}

pub fn validate_watch_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    parse_options(args.get(1))?;
    Ok(host_api::object(vec![("close".into(), Value::Undefined)]))
}

pub fn validate_directory_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    parse_options(args.get(1))?;
    Ok(host_api::object(vec![]))
}

fn sync_props() -> Vec<(&'static str, Value)> {
    use crate::registry::*;
    let mut props = vec![
        (
            "readFileSync",
            crate::host::capability(SPEC_FS_READFILESYNC),
        ),
        (
            "writeFileSync",
            crate::host::capability(SPEC_FS_WRITEFILESYNC),
        ),
        ("statSync", crate::host::capability(SPEC_FS_STATSYNC)),
        ("lstatSync", crate::host::capability(SPEC_FS_LSTATSYNC)),
        ("readdirSync", crate::host::capability(SPEC_FS_READDIRSYNC)),
        ("existsSync", crate::host::capability(SPEC_FS_EXISTSSYNC)),
        ("realpathSync", crate::host::capability(SPEC_FS_REALSYNC)),
        ("opendirSync", crate::host::capability(SPEC_FS_OPENDIRSYNC)),
        ("mkdirSync", crate::host::capability(SPEC_FS_MKDIRSYNC)),
        ("unlinkSync", crate::host::capability(SPEC_FS_UNLINKSYNC)),
        ("rmdirSync", crate::host::capability(SPEC_FS_RMDIRSYNC)),
    ];
    props.extend(sync_props_more());
    props
}

fn sync_props_more() -> Vec<(&'static str, Value)> {
    use crate::registry::*;
    vec![
        ("rmSync", crate::host::capability(SPEC_FS_RMSYNC)),
        ("renameSync", crate::host::capability(SPEC_FS_RENAMESYNC)),
        (
            "appendFileSync",
            crate::host::capability(SPEC_FS_APPENDFILESYNC),
        ),
        (
            "copyFileSync",
            crate::host::capability(SPEC_FS_COPYFILESYNC),
        ),
        ("accessSync", crate::host::capability(SPEC_FS_ACCESSSYNC)),
        ("mkdtempSync", crate::host::capability(SPEC_FS_MKDTEMPSYNC)),
        (
            "readlinkSync",
            crate::host::capability(SPEC_FS_READLINKSYNC),
        ),
        ("chmodSync", crate::host::capability(SPEC_FS_CHMODSYNC)),
        ("symlinkSync", crate::host::capability(SPEC_FS_SYMLINKSYNC)),
        (
            "truncateSync",
            crate::host::capability(SPEC_FS_TRUNCATESYNC),
        ),
    ]
}

/// `fs.promises` — each op runs the sync implementation and returns
/// an already-settled Promise (fulfilled with the result, rejected
/// with the coded error).
fn promises() -> Value {
    use crate::registry::*;
    let props: Vec<(&str, Value)> = vec![
        ("readFile", crate::host::capability(SPEC_FSP_READFILE)),
        ("writeFile", crate::host::capability(SPEC_FSP_WRITEFILE)),
        ("appendFile", crate::host::capability(SPEC_FSP_APPENDFILE)),
        ("stat", crate::host::capability(SPEC_FSP_STAT)),
        ("lstat", crate::host::capability(SPEC_FSP_LSTAT)),
        ("readdir", crate::host::capability(SPEC_FSP_READDIR)),
        ("mkdir", crate::host::capability(SPEC_FSP_MKDIR)),
        ("unlink", crate::host::capability(SPEC_FSP_UNLINK)),
        ("rmdir", crate::host::capability(SPEC_FSP_RMDIR)),
        ("rm", crate::host::capability(SPEC_FSP_RM)),
        ("rename", crate::host::capability(SPEC_FSP_RENAME)),
        ("copyFile", crate::host::capability(SPEC_FSP_COPYFILE)),
        ("access", crate::host::capability(SPEC_FSP_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FSP_MKDTEMP)),
        ("readlink", crate::host::capability(SPEC_FSP_READLINK)),
        ("chmod", crate::host::capability(SPEC_FSP_CHMOD)),
        ("truncate", crate::host::capability(SPEC_FSP_TRUNCATE)),
        ("realpath", crate::host::capability(SPEC_FSP_REALPATH)),
    ];
    crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined)
}

fn constants() -> Value {
    let entries: Vec<(String, Value)> = CONSTANT_ENTRIES
        .iter()
        .map(|(name, value)| (name.to_string(), Value::Number(*value)))
        .collect();
    host_api::object(entries)
}

#[cfg(target_os = "macos")]
mod flags {
    pub const O_CREAT: f64 = 0x200 as f64;
    pub const O_EXCL: f64 = 0x800 as f64;
    pub const O_TRUNC: f64 = 0x400 as f64;
    pub const O_DIRECTORY: f64 = 0x100000 as f64;
    pub const O_NOFOLLOW: f64 = 0x100 as f64;
}

#[cfg(all(unix, not(target_os = "macos")))]
mod flags {
    pub const O_CREAT: f64 = 0x40 as f64;
    pub const O_EXCL: f64 = 0x80 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0x10000 as f64;
    pub const O_NOFOLLOW: f64 = 0x20000 as f64;
}

#[cfg(not(unix))]
mod flags {
    pub const O_CREAT: f64 = 0x100 as f64;
    pub const O_EXCL: f64 = 0x400 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0.0;
    pub const O_NOFOLLOW: f64 = 0.0;
}

const CONSTANT_ENTRIES: &[(&str, f64)] = &[
    ("F_OK", 0.0),
    ("R_OK", 4.0),
    ("W_OK", 2.0),
    ("X_OK", 1.0),
    ("COPYFILE_EXCL", 1.0),
    ("COPYFILE_FICLONE", 2.0),
    ("COPYFILE_FICLONE_FORCE", 4.0),
    ("O_RDONLY", 0.0),
    ("O_WRONLY", 1.0),
    ("O_RDWR", 2.0),
    ("O_CREAT", flags::O_CREAT),
    ("O_EXCL", flags::O_EXCL),
    ("O_TRUNC", flags::O_TRUNC),
    ("O_APPEND", 8.0),
    ("O_DIRECTORY", flags::O_DIRECTORY),
    ("O_NOFOLLOW", flags::O_NOFOLLOW),
    ("S_IFMT", 0o170000 as f64),
    ("S_IFREG", 0o100000 as f64),
    ("S_IFDIR", 0o40000 as f64),
    ("S_IFCHR", 0o20000 as f64),
    ("S_IFBLK", 0o60000 as f64),
    ("S_IFIFO", 0o10000 as f64),
    ("S_IFLNK", 0o120000 as f64),
    ("S_IFSOCK", 0o140000 as f64),
    ("S_IRWXU", 0o700 as f64),
    ("S_IRUSR", 0o400 as f64),
    ("S_IWUSR", 0o200 as f64),
    ("S_IXUSR", 0o100 as f64),
    ("S_IRWXG", 0o70 as f64),
    ("S_IRWXO", 0o7 as f64),
];

/// Dispatch table reused by the async and promises families.
pub(crate) type Op =
    fn(&Rc<RefCell<HostState>>, Option<&Value>, &[Value]) -> Result<Value, VmError>;

pub(crate) fn sync_op(name: &str) -> Option<Op> {
    use super::fs_sync as sync;
    Some(match name {
        "readFile" => sync::read_file_sync,
        "writeFile" => sync::write_file_sync,
        "appendFile" => sync::append_file_sync,
        "stat" => sync::stat_sync,
        "lstat" => sync::lstat_sync,
        "readdir" => sync::readdir_sync,
        "mkdir" => sync::mkdir_sync,
        "unlink" => sync::unlink_sync,
        "rmdir" => sync::rmdir_sync,
        "rm" => sync::rm_sync,
        "rename" => sync::rename_sync,
        "copyFile" => sync::copy_file_sync,
        "access" => sync::access_sync,
        "mkdtemp" => sync::mkdtemp_sync,
        "readlink" => sync::readlink_sync,
        "chmod" => sync::chmod_sync,
        "truncate" => sync::truncate_sync,
        "realpath" => sync::realpath_sync,
        _ => return None,
    })
}
