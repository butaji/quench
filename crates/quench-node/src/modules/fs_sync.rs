//! `fs` synchronous operations — real filesystem I/O with coded
//! Node errors. Async variants wrap these and defer the callback.

use std::cell::RefCell;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::fs::{parse_mkdir_options, parse_options, path_arg, FsOptions};

fn split(args: &[Value]) -> Result<(String, FsOptions), VmError> {
    let path = path_arg(args.first())?;
    let options = parse_options(args.get(1))?;
    Ok((path, options))
}

fn string_data(data: &Value, encoding: Option<&str>) -> Result<Vec<u8>, VmError> {
    match data {
        Value::String(s) => Ok(crate::modules::buffer_enc::encode_str(
            s,
            encoding.unwrap_or("utf8"),
        )),
        Value::Float64Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 8),
        Value::Float32Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 4),
        Value::Int8Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length),
        Value::Int16Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 2),
        Value::Int32Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 4),
        Value::BigInt64Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 8),
        Value::BigUint64Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 8),
        Value::Uint32Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 4),
        Value::Uint8Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length),
        Value::Uint8ClampedArray(view) => view_bytes(&view.buffer, view.byte_offset, view.length),
        Value::Uint16Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 2),
        Value::DataView(view) => view_bytes(&view.buffer, view.byte_offset, view.byte_length),
        other => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

fn view_bytes(
    buffer: &Rc<quench_runtime::value::ArrayBufferData>,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, VmError> {
    let bytes = buffer.bytes.borrow();
    let end = offset.checked_add(length).ok_or_else(|| {
        crate::modules::buffer_enc::invalid_arg_type(
            "The \"data\" argument contains an invalid view".to_string(),
        )
    })?;
    bytes
        .get(offset..end)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_type(
                "The \"data\" argument contains an invalid view".to_string(),
            )
        })
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
    let bytes = if let Some(flags) = options.flag.as_deref() {
        let mut file = super::fs::open_options(flags)?
            .open(path)
            .map_err(|e| super::fs_error::fs_error("open", Some(path), &e))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| super::fs_error::fs_error("read", Some(path), &e))?;
        bytes
    } else {
        let meta = std::fs::metadata(Path::new(path))
            .map_err(|e| super::fs_error::fs_error("open", Some(path), &e))?;
        if meta.is_dir() {
            let err = std::io::Error::from_raw_os_error(21);
            return Err(super::fs_error::fs_error("read", Some(path), &err));
        }
        std::fs::read(Path::new(path))
            .map_err(|e| super::fs_error::fs_error("open", Some(path), &e))?
    };
    decode_bytes(bytes, options)
}

fn decode_bytes(bytes: Vec<u8>, options: &FsOptions) -> Result<Value, VmError> {
    let target = if let Some(buffer) = options.buffer.clone() {
        if quench_runtime::is_callable(&buffer) {
            quench_runtime::execute::call(
                &buffer,
                &Value::Undefined,
                &[Value::Number(bytes.len() as f64)],
            )?
        } else {
            buffer
        }
    } else {
        Value::Undefined
    };
    if !matches!(target, Value::Undefined) {
        let Some((buffer, offset, length)) = view_parts(&target) else {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"options.buffer\" property must be an instance of Buffer, TypedArray, or DataView".into(),
            ));
        };
        if length < bytes.len() {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The \"buffer\" argument must be at least as large as the file".into(),
            ));
        }
        buffer.bytes.borrow_mut()[offset..offset + bytes.len()].copy_from_slice(&bytes);
        if let Some(encoding) = &options.encoding {
            return Ok(crate::modules::buffer_enc::decode_str(&bytes, encoding));
        }
        let start = Value::Number(0.0);
        let end = Value::Number(bytes.len() as f64);
        let subarray = quench_runtime::execute::get_property(&target, "subarray");
        if quench_runtime::is_callable(&subarray) {
            return Ok(quench_runtime::execute::call(
                &subarray,
                &target,
                &[start, end],
            )?);
        }
        return Ok(super::buffer_proto::make_view(buffer, offset, bytes.len()));
    }
    Ok(match &options.encoding {
        Some(encoding) => crate::modules::buffer_enc::decode_str(&bytes, encoding),
        None => crate::modules::buffer_proto::make_buffer(&bytes),
    })
}

fn view_parts(
    value: &Value,
) -> Option<(
    std::rc::Rc<quench_runtime::value::ArrayBufferData>,
    usize,
    usize,
)> {
    macro_rules! view {
        ($view:expr, $size:expr) => {
            Some((
                $view.buffer.clone(),
                $view.byte_offset,
                $view.length * $size,
            ))
        };
    }
    match value {
        Value::Float64Array(view) => view!(view, 8),
        Value::Float32Array(view) => view!(view, 4),
        Value::Int8Array(view) => view!(view, 1),
        Value::Int16Array(view) => view!(view, 2),
        Value::Int32Array(view) => view!(view, 4),
        Value::BigInt64Array(view) => view!(view, 8),
        Value::BigUint64Array(view) => view!(view, 8),
        Value::Uint32Array(view) => view!(view, 4),
        Value::Uint8Array(view) => view!(view, 1),
        Value::Uint8ClampedArray(view) => view!(view, 1),
        Value::Uint16Array(view) => view!(view, 2),
        Value::DataView(view) => Some((view.buffer.clone(), view.byte_offset, view.byte_length)),
        _ => None,
    }
}

pub fn read_file_sync(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if matches!(args.first(), Some(Value::Number(_))) {
        let fd = super::fs::descriptor_arg(args.first())?;
        let options = parse_options(args.get(1))?;
        let bytes = {
            let mut host = state.borrow_mut();
            let descriptor = host.fs.descriptors.get_mut(&fd).ok_or_else(|| {
                crate::modules::fs_error::fs_error(
                    "read",
                    None,
                    &std::io::Error::from_raw_os_error(9),
                )
            })?;
            let mut bytes = Vec::new();
            descriptor
                .file
                .read_to_end(&mut bytes)
                .map_err(|e| super::fs_error::fs_error("read", Some(&descriptor.path), &e))?;
            bytes
        };
        return decode_bytes(bytes, &options);
    }
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
            } else if options.encoding.as_deref() == Some("hex") {
                crate::modules::buffer_enc::decode_str(name.as_bytes(), "hex")
            } else if options.encoding.as_deref() == Some("buffer") {
                super::buffer_proto::make_buffer(name.as_bytes())
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
    let options = super::fs::parse_options(args.get(1))?;
    let canon = std::fs::canonicalize(Path::new(&path))
        .map_err(|e| super::fs_error::fs_error("realpath", Some(&path), &e))?;
    let canon = canon.to_string_lossy().into_owned();
    Ok(match options.encoding.as_deref() {
        Some("buffer") => super::buffer_proto::make_buffer(canon.as_bytes()),
        Some(encoding) => crate::modules::buffer_enc::decode_str(canon.as_bytes(), encoding),
        None => Value::String(canon),
    })
}

pub fn mkdir_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let options = parse_mkdir_options(args.get(1))?;
    let first_created = options.recursive.then(|| first_missing_path(&path));
    let result = if options.recursive {
        std::fs::create_dir_all(Path::new(&path))
    } else {
        std::fs::create_dir(Path::new(&path))
    };
    result.map_err(|e| super::fs_error::fs_error("mkdir", Some(&path), &e))?;
    apply_mode(&path, options.mode);
    Ok(first_created
        .flatten()
        .map_or(Value::Undefined, Value::String))
}

fn first_missing_path(path: &str) -> Option<String> {
    let mut candidate = PathBuf::from(path);
    if candidate.exists() {
        return None;
    }
    while candidate
        .parent()
        .is_some_and(|parent| !parent.exists() && parent != Path::new(""))
    {
        candidate = candidate.parent()?.to_path_buf();
    }
    Some(candidate.to_string_lossy().into_owned())
}

pub fn unlink_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    std::fs::remove_file(Path::new(&path))
        .map_err(|e| super::fs_error::fs_error("unlink", Some(&path), &e))?;
    Ok(Value::Undefined)
}

pub fn rmdir_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    std::fs::remove_dir(Path::new(&path))
        .map_err(|e| super::fs_error::fs_error("rmdir", Some(&path), &e))?;
    Ok(Value::Undefined)
}

pub fn rm_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    let target = Path::new(&path);
    let result = if options.recursive {
        std::fs::remove_dir_all(target)
    } else if target.is_dir() && !target.is_symlink() {
        std::fs::remove_dir(target)
    } else {
        std::fs::remove_file(target)
    };
    match result {
        Ok(()) => Ok(Value::Undefined),
        Err(e) if options.force && e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Undefined),
        Err(e) => Err(super::fs_error::fs_error("rm", Some(&path), &e)),
    }
}

pub fn rename_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let from = path_arg(args.first())?;
    let to = path_arg(args.get(1))?;
    std::fs::rename(Path::new(&from), Path::new(&to))
        .map_err(|e| super::fs_error::fs_error("rename", Some(&from), &e))?;
    Ok(Value::Undefined)
}

pub fn copy_file_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let from = path_arg(args.first())?;
    let to = path_arg(args.get(1))?;
    let exclusive = matches!(args.get(2), Some(Value::Number(m)) if *m as u32 & 1 != 0);
    let result = if exclusive {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(Path::new(&to))
            .and_then(|mut dest| {
                use std::io::Write;
                let bytes = std::fs::read(Path::new(&from))?;
                dest.write_all(&bytes)
            })
            .map(|_| 0)
    } else {
        std::fs::copy(Path::new(&from), Path::new(&to))
    };
    result.map_err(|e| super::fs_error::fs_error("copyfile", Some(&from), &e))?;
    Ok(Value::Undefined)
}

pub fn access_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let mode = match args.get(1) {
        Some(Value::Number(m)) => *m as u32,
        _ => 0,
    };
    check_access(&path, mode).map_err(|e| super::fs_error::fs_error("access", Some(&path), &e))?;
    Ok(Value::Undefined)
}

fn check_access(path: &str, mode: u32) -> std::io::Result<()> {
    let meta = std::fs::metadata(Path::new(path))?;
    if mode & 2 != 0 && meta.permissions().readonly() {
        return Err(std::io::Error::from_raw_os_error(13));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if mode & 1 != 0 && meta.mode() & 0o111 == 0 {
            return Err(std::io::Error::from_raw_os_error(13));
        }
    }
    Ok(())
}

pub fn mkdtemp_sync(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let prefix = path_arg(args.first())?;
    super::fs::parse_options(args.get(1))?;
    if prefix.ends_with('X') {
        crate::modules::process::emit_warning(
            s,
            "Warning",
            "mkdtemp() templates ending with X are not portable. For details see: https://nodejs.org/api/fs.html",
            None,
            true,
        );
    }
    for attempt in 0..100u32 {
        let candidate = format!("{prefix}{:06x}", random_suffix(attempt));
        match std::fs::create_dir(Path::new(&candidate)) {
            Ok(()) => return Ok(Value::String(candidate)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(super::fs_error::fs_error("mkdtemp", Some(&prefix), &e)),
        }
    }
    let err = std::io::Error::from_raw_os_error(17);
    Err(super::fs_error::fs_error("mkdtemp", Some(&prefix), &err))
}

fn random_suffix(attempt: u32) -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (now ^ std::process::id().wrapping_mul(2654435761) ^ attempt.wrapping_mul(40503)) & 0x00ff_ffff
}

pub fn readlink_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    let target = std::fs::read_link(Path::new(&path))
        .map_err(|e| super::fs_error::fs_error("readlink", Some(&path), &e))?;
    let text = target.to_string_lossy().into_owned();
    Ok(match &options.encoding {
        Some(encoding) if encoding != "utf8" => {
            crate::modules::buffer_enc::decode_str(text.as_bytes(), encoding)
        }
        Some(_) | None => Value::String(text),
    })
}

pub fn symlink_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = path_arg(args.first())?;
    let link = path_arg(args.get(1))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link)
        .map_err(|e| super::fs_error::fs_error("symlink", Some(&link), &e))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target, &link)
        .map_err(|e| super::fs_error::fs_error("symlink", Some(&link), &e))?;
    Ok(Value::Undefined)
}

pub fn chmod_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let mode = match args.get(1) {
        Some(Value::Number(m)) => *m as u32,
        Some(Value::String(m)) => u32::from_str_radix(m, 8).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_value(format!(
                "The \"mode\" argument is invalid: {m}"
            ))
        })?,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"mode\" argument must be of type number.".to_string(),
            ));
        }
    };
    chmod_impl(&path, mode)
}

fn chmod_impl(path: &str, mode: u32) -> Result<Value, VmError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(Path::new(path), std::fs::Permissions::from_mode(mode))
            .map_err(|e| super::fs_error::fs_error("chmod", Some(path), &e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        let err = std::io::Error::from_raw_os_error(78);
        return Err(super::fs_error::fs_error("chmod", Some(path), &err));
    }
    Ok(Value::Undefined)
}

pub fn truncate_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let len = match args.get(1) {
        Some(Value::Number(n)) => *n as u64,
        _ => 0,
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(Path::new(&path))
        .map_err(|e| super::fs_error::fs_error("truncate", Some(&path), &e))?;
    file.set_len(len)
        .map_err(|e| super::fs_error::fs_error("truncate", Some(&path), &e))?;
    Ok(Value::Undefined)
}
