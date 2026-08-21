//! `fs` synchronous operations — real filesystem I/O with coded
//! Node errors. Async variants wrap these and defer the callback.

#[path = "fs_sync_extra.rs"]
mod fs_sync_extra;
pub(crate) use fs_sync_extra::{
    access_sync, chmod_sync, chown_sync, copy_file_sync, lchmod_sync, lchown_sync, link_sync,
    lutimes_sync, mkdtemp_sync, readlink_sync, rename_sync, rm_sync, rmdir_sync, symlink_sync,
    truncate_sync, unlink_sync, utimes_sync, utimes_time,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::fs::{parse_options, path_arg, FsOptions};

fn split(args: &[Value]) -> Result<(String, FsOptions), VmError> {
    let path = path_arg(args.first())?;
    let options = parse_options(args.get(1))?;
    Ok((path, options))
}
#[cfg(unix)]
pub fn statfs_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = super::fs::path_arg(args.first())?;
    let cpath = std::ffi::CString::new(path.as_bytes()).map_err(|_| {
        crate::modules::buffer_enc::invalid_arg_value(
            "Path must be a string without null bytes".into(),
        )
    })?;
    let mut st = unsafe { std::mem::zeroed::<libc::statvfs>() };
    let rc = unsafe { libc::statvfs(cpath.as_ptr(), &mut st) };
    if rc != 0 {
        return Err(crate::modules::fs_error::fs_error(
            "statfs",
            Some(&path),
            &std::io::Error::last_os_error(),
        ));
    }
    let n = |v: u128| Value::Number(v as f64);
    Ok(host_api::object(vec![
        ("type".into(), n(st.f_flag as u128)),
        ("bsize".into(), n(st.f_bsize as u128)),
        ("frsize".into(), n(st.f_frsize as u128)),
        ("blocks".into(), n(st.f_blocks as u128)),
        ("bfree".into(), n(st.f_bfree as u128)),
        ("bavail".into(), n(st.f_bavail as u128)),
        ("files".into(), n(st.f_files as u128)),
        ("ffree".into(), n(st.f_ffree as u128)),
    ]))
}

#[cfg(not(unix))]
pub fn statfs_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(crate::modules::buffer_enc::invalid_arg_value(
        "statfs is not supported on this platform".into(),
    ))
}

fn string_data(data: &Value, encoding: Option<&str>) -> Result<Vec<u8>, VmError> {
    match data {
        Value::String(s) => Ok(crate::modules::buffer_enc::encode_str(
            s,
            encoding.unwrap_or("utf8"),
        )),
        Value::Uint8Array(view) => Ok(view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.length]
            .to_vec()),
        other => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

fn write_open(path: &str, flag: Option<&str>, syscall: &str) -> Result<std::fs::File, VmError> {
    let mut open = std::fs::OpenOptions::new();
    match flag.unwrap_or("w") {
        "w" | "w+" => {
            open.write(true).create(true).truncate(true);
        }
        "a" | "a+" => {
            open.append(true).create(true);
        }
        "wx" | "ax" => {
            open.write(true).create_new(true);
        }
        "r" => {
            open.read(true);
        }
        other => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The argument 'flag' is invalid. Received {other:?}"
            )));
        }
    }
    open.open(Path::new(path))
        .map_err(|e| super::fs_error::fs_error(syscall, Some(path), &e))
}

fn apply_mode(path: &str, mode: Option<u32>) {
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
}

fn read_bytes(path: &str, options: &FsOptions) -> Result<Value, VmError> {
    let meta = std::fs::metadata(Path::new(path))
        .map_err(|e| super::fs_error::fs_error("open", Some(path), &e))?;
    if meta.is_dir() {
        let err = std::io::Error::from_raw_os_error(21);
        return Err(super::fs_error::fs_error("read", Some(path), &err));
    }
    let bytes = std::fs::read(Path::new(path))
        .map_err(|e| super::fs_error::fs_error("open", Some(path), &e))?;
    Ok(match &options.encoding {
        Some(encoding) => crate::modules::buffer_enc::decode_str(&bytes, encoding),
        None => crate::modules::buffer_proto::make_buffer(&bytes),
    })
}

pub fn read_file_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    read_bytes(&path, &options)
}

pub fn write_file_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let options = parse_options(args.get(2))?;
    write_impl(&path, args.get(1), &options, "open")
}

fn write_impl(
    path: &str,
    data: Option<&Value>,
    options: &FsOptions,
    syscall: &str,
) -> Result<Value, VmError> {
    let bytes = string_data(
        data.unwrap_or(&Value::Undefined),
        options.encoding.as_deref(),
    )?;
    use std::io::Write;
    let mut file = write_open(path, options.flag.as_deref(), syscall)?;
    file.write_all(&bytes)
        .map_err(|e| super::fs_error::fs_error("write", Some(path), &e))?;
    apply_mode(path, options.mode);
    Ok(Value::Undefined)
}

pub fn append_file_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let mut options = parse_options(args.get(2))?;
    if options.flag.is_none() {
        options.flag = Some("a".to_string());
    }
    write_impl(&path, args.get(1), &options, "open")
}

fn stat_impl(path: &str, follow: bool, throw_if_no_entry: bool) -> Result<Value, VmError> {
    let syscall = if follow { "stat" } else { "lstat" };
    let meta = if follow {
        std::fs::metadata(Path::new(path))
    } else {
        std::fs::symlink_metadata(Path::new(path))
    };
    match meta {
        Ok(meta) => Ok(super::fs_stats::stats(&meta)),
        Err(e) if !throw_if_no_entry && e.kind() == std::io::ErrorKind::NotFound => {
            Ok(Value::Undefined)
        }
        Err(e) => Err(super::fs_error::fs_error(syscall, Some(path), &e)),
    }
}

pub fn stat_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    stat_impl(&path, true, options.throw_if_no_entry)
}

pub fn open_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = super::fs::path_arg(args.first())?;
    // Second argument is `flags` (a string) or an options object; mode
    // (third) is accepted but unused. Matches Node's openSync signature.
    let flag: String = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => super::fs::parse_options(Some(v))
            .map(|o| o.flag.unwrap_or_else(|| "r".to_string()))
            .unwrap_or_else(|_| "r".to_string()),
        None => "r".to_string(),
    };
    super::fs::fd_open(&path, Some(&flag)).map(|n| Value::Number(n as f64))
}
pub fn fstat_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match args.first() {
        Some(Value::Number(n)) => super::fs::fd_stat(*n as i32),
        _ => Err(crate::modules::buffer_enc::invalid_arg_type(
            "fd must be a number".into(),
        )),
    }
}
pub fn close_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match args.first() {
        Some(Value::Number(n)) => super::fs::fd_close(*n as i32),
        _ => Err(crate::modules::buffer_enc::invalid_arg_type(
            "fd must be a number".into(),
        )),
    }
}

/// Deprecated `fs.Stats(...)` constructor: builds a `Stats` value from
/// the 14 date/group fields and emits a DEP0180 deprecation warning.
pub fn stats_constructor(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let n = |i: usize| -> f64 {
        match args.get(i) {
            Some(Value::Number(num)) => *num,
            _ => 0.0,
        }
    };
    let value = super::fs_stats::stats_from_values(
        n(0),
        n(1),
        n(2),
        n(3),
        n(4),
        n(5),
        n(6),
        n(7),
        n(8),
        n(9),
        n(10),
        n(11),
        n(12),
        n(13),
    );
    super::process::emit_warning(
        state,
        "DeprecationWarning",
        "fs.Stats constructor is deprecated.",
        Some("DEP0180"),
        true,
    );
    Ok(value)
}

pub fn lstat_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    stat_impl(&path, false, options.throw_if_no_entry)
}

pub fn readdir_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    let entries = read_dir_entries(&path, options.recursive)?;
    let values = entries
        .iter()
        .map(|(name, mode)| {
            if options.with_file_types {
                super::fs_stats::dirent(name, *mode)
            } else {
                Value::String(name.clone())
            }
        })
        .collect();
    Ok(host_api::array(values))
}

/// List `(name, mode)` pairs, sorted by name; `recursive` descends
/// into real directories (never symlinks) and reports relative paths.
fn read_dir_entries(path: &str, recursive: bool) -> Result<Vec<(String, u32)>, VmError> {
    let mut out = Vec::new();
    read_dir_into(Path::new(path), "", recursive, &mut out)
        .map_err(|e| super::fs_error::fs_error("scandir", Some(path), &e))?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn read_dir_into(
    dir: &Path,
    prefix: &str,
    recursive: bool,
    out: &mut Vec<(String, u32)>,
) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}{name}");
        let file_type = entry.file_type()?;
        out.push((rel.clone(), super::fs_stats::mode_of(&file_type)));
        if recursive && file_type.is_dir() {
            read_dir_into(&entry.path(), &format!("{rel}/"), true, out)?;
        }
    }
    Ok(())
}

pub fn exists_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Node: invalid path types are deprecated but yield `false`.
    Ok(Value::Boolean(match args.first() {
        Some(Value::String(path)) => PathBuf::from(path).exists(),
        other => {
            deprecation_warning(_s, other);
            false
        }
    }))
}

/// DEP0187: invalid argument types passed to `fs.exists`/`existsSync`.
pub(crate) fn deprecation_warning(state: &Rc<RefCell<HostState>>, path: Option<&Value>) {
    if matches!(path, Some(Value::String(_))) {
        return;
    }
    crate::modules::process::emit_warning(
        state,
        "DeprecationWarning",
        "Passing invalid argument types to fs.existsSync is deprecated",
        Some("DEP0187"),
        true,
    );
}

pub fn realpath_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let canon = std::fs::canonicalize(Path::new(&path))
        .map_err(|e| super::fs_error::fs_error("realpath", Some(&path), &e))?;
    Ok(Value::String(canon.to_string_lossy().into_owned()))
}

pub fn mkdir_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    let result = if options.recursive {
        std::fs::create_dir_all(Path::new(&path))
    } else {
        std::fs::create_dir(Path::new(&path))
    };
    result.map_err(|e| super::fs_error::fs_error("mkdir", Some(&path), &e))?;
    apply_mode(&path, options.mode);
    Ok(Value::Undefined)
}

