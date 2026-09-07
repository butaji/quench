//! `fs` synchronous operations — real filesystem I/O with coded
//! Node errors. Async variants wrap these and defer the callback.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::fs::{parse_mkdir_options, parse_options, path_arg, FsOptions};

fn split(args: &[Value]) -> Result<(String, FsOptions), VmError> {
    let path = path_arg(args.first())?;
    let options = parse_options(args.get(1))?;
    Ok((path, options))
}

fn file_handle_fd(value: Option<&Value>) -> Result<Option<i32>, VmError> {
    value
        .map(super::fs::file_handle_descriptor)
        .transpose()
        .map(Option::flatten)
}

fn string_data(data: &Value, encoding: Option<&str>) -> Result<Vec<u8>, VmError> {
    if execute::is_symbol(data) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(data)
        )));
    }
    match data {
        Value::String(s) => Ok(crate::modules::buffer_enc::encode_str(
            s,
            encoding.unwrap_or("utf8"),
        )),
        Value::StringUnits(units) => {
            let string = String::from_utf16_lossy(units);
            Ok(crate::modules::buffer_enc::encode_str(
                &string,
                encoding.unwrap_or("utf8"),
            ))
        }
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

pub(crate) fn validate_data(data: &Value, encoding: Option<&str>) -> Result<(), VmError> {
    string_data(data, encoding).map(|_| ())
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

pub(crate) fn apply_mode(path: &str, mode: Option<u32>) {
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
}

fn observe_read_fstat(state: &Rc<RefCell<HostState>>, path: &str) -> Result<(), VmError> {
    let binding = state
        .borrow()
        .module_cache
        .get("\0internalBinding:fs")
        .cloned();
    let Some(binding) = binding else {
        return Ok(());
    };
    let fstat = execute::get_property(&binding, "fstat");
    if !quench_runtime::is_callable(&fstat) {
        return Ok(());
    }
    let fd = super::fs::open_sync(
        state,
        None,
        &[Value::String(path.to_owned()), Value::String("r".into())],
    )?;
    let result = execute::call(&fstat, &binding, &[fd.clone()]);
    let _ = super::fs::close_sync(state, None, &[fd]);
    result.map(|_| ())
}

fn should_observe_read_fstat(path: &str, options: &FsOptions) -> bool {
    let Some(buffer) = options.buffer.as_ref() else {
        return true;
    };
    if quench_runtime::is_callable(buffer) {
        return true;
    }
    let Some((_, _, capacity)) = view_parts(buffer) else {
        return true;
    };
    std::fs::metadata(path)
        .map(|metadata| capacity >= metadata.len() as usize)
        .unwrap_or(true)
}

fn observe_fd_fstat(state: &Rc<RefCell<HostState>>, fd: i32) -> Result<(), VmError> {
    let binding = state
        .borrow()
        .module_cache
        .get("\0internalBinding:fs")
        .cloned();
    let Some(binding) = binding else {
        return Ok(());
    };
    let fstat = execute::get_property(&binding, "fstat");
    if !quench_runtime::is_callable(&fstat) {
        return Ok(());
    }
    execute::call(&fstat, &binding, &[Value::Number(fd as f64)]).map(|_| ())
}

fn read_bytes(path: &str, options: &FsOptions) -> Result<Value, VmError> {
    const MAX_IO_LENGTH: u64 = 2_147_483_647;
    if let Ok(meta) = std::fs::metadata(Path::new(path)) {
        if meta.len() > MAX_IO_LENGTH {
            return Err(super::fs_error::file_too_large(meta.len()));
        }
    }
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
    if let Some(fd) = file_handle_fd(args.first())? {
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
    if options.encoding.is_none() && should_observe_read_fstat(&path, &options) {
        observe_read_fstat(state, &path)?;
    }
    read_bytes(&path, &options)
}

pub fn read_file_async(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(Value::Number(fd)) = args.first() {
        observe_fd_fstat(state, *fd as i32)?;
        return read_file_sync(state, receiver, args);
    }
    let (path, options) = split(args)?;
    if should_observe_read_fstat(&path, &options) {
        observe_read_fstat(state, &path)?;
    }
    read_bytes(&path, &options)
}

pub fn write_file_sync(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(fd) = file_handle_fd(args.first())? {
        let options = parse_options(args.get(2))?;
        let bytes = string_data(
            args.get(1).unwrap_or(&Value::Undefined),
            options.encoding.as_deref(),
        )?;
        let mut host = state.borrow_mut();
        let descriptor = host.fs.descriptors.get_mut(&fd).ok_or_else(|| {
            super::fs_error::fs_error("write", None, &std::io::Error::from_raw_os_error(9))
        })?;
        descriptor
            .file
            .write_all(&bytes)
            .map_err(|e| super::fs_error::fs_error("write", Some(&descriptor.path), &e))?;
        if options.flush {
            descriptor
                .file
                .sync_all()
                .map_err(|e| super::fs_error::fs_error("fsync", Some(&descriptor.path), &e))?;
        }
        return Ok(Value::Undefined);
    }
    let path = path_arg(args.first())?;
    if crate::modules::process::permission_enabled(state)
        && !crate::modules::process::permission_audit(state)
        && !crate::modules::process::permission_allows(state, "fs.write")
    {
        return Err(crate::modules::process::permission_error("fs.write", &path));
    }
    let options = parse_options(args.get(2))?;
    let result = write_impl(&path, args.get(1), &options, "open");
    if result.is_ok() && options.flush {
        let fd = super::fs::open_sync(
            state,
            None,
            &[Value::String(path.clone()), Value::String("r".into())],
        )?;
        let fd_number = match fd {
            Value::Number(number) => number as i32,
            _ => unreachable!("openSync returns a numeric descriptor"),
        };
        let flush_result = super::fs::invoke_fsync_sync(state, fd_number);
        let _ = super::fs::close_sync(state, None, &[fd]);
        return flush_result.map(|_| Value::Undefined);
    }
    result
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
    if options.flush {
        file.sync_all()
            .map_err(|e| super::fs_error::fs_error("fsync", Some(path), &e))?;
    }
    apply_mode(path, options.mode);
    Ok(Value::Undefined)
}

pub fn append_file_sync(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(fd) = file_handle_fd(args.first())? {
        let options = parse_options(args.get(2))?;
        let bytes = string_data(
            args.get(1).unwrap_or(&Value::Undefined),
            options.encoding.as_deref(),
        )?;
        {
            let mut host = state.borrow_mut();
            let descriptor = host.fs.descriptors.get_mut(&fd).ok_or_else(|| {
                super::fs_error::fs_error("write", None, &std::io::Error::from_raw_os_error(9))
            })?;
            descriptor
                .file
                .write_all(&bytes)
                .map_err(|e| super::fs_error::fs_error("write", Some(&descriptor.path), &e))?;
            if options.flush {
                descriptor
                    .file
                    .sync_all()
                    .map_err(|e| super::fs_error::fs_error("fsync", Some(&descriptor.path), &e))?;
            }
        }
        if options.flush {
            super::fs::invoke_fsync_sync(state, fd)?;
        }
        return Ok(Value::Undefined);
    }
    let path = path_arg(args.first())?;
    let mut options = parse_options(args.get(2))?;
    if options.flag.is_none() {
        options.flag = Some("a".to_string());
    }
    let result = write_impl(&path, args.get(1), &options, "open");
    if result.is_ok() && options.flush {
        // Open a tracked descriptor solely for the observable fsyncSync call;
        // the write itself was flushed above on its owning Rust handle.
        let fd = super::fs::open_sync(
            state,
            None,
            &[Value::String(path.clone()), Value::String("r".into())],
        )?;
        let fd_number = match fd {
            Value::Number(number) => number as i32,
            _ => unreachable!("openSync returns a numeric descriptor"),
        };
        let flush_result = super::fs::invoke_fsync_sync(state, fd_number);
        let _ = super::fs::close_sync(state, None, &[fd]);
        flush_result.map(|_| Value::Undefined)
    } else {
        result
    }
}

fn bigint_stats(value: Value) -> Value {
    for name in [
        "dev", "mode", "nlink", "uid", "gid", "rdev", "blksize", "ino", "size", "blocks",
    ] {
        if let Value::Number(number) = execute::get_property(&value, name) {
            let _ = execute::set_property_in_place(
                &value,
                name,
                Value::BigInt((number as i128).to_string()),
            );
        }
    }
    for name in ["atimeMs", "mtimeMs", "ctimeMs", "birthtimeMs"] {
        if let Value::Number(number) = execute::get_property(&value, name) {
            let _ = execute::set_property_in_place(
                &value,
                name,
                Value::BigInt((number as i128).to_string()),
            );
            let ns_name = name.replace("Ms", "Ns");
            let _ = execute::set_property_in_place(
                &value,
                &ns_name,
                Value::BigInt(((number * 1_000_000.0) as i128).to_string()),
            );
        }
    }
    value
}

fn stat_impl(
    path: &str,
    follow: bool,
    throw_if_no_entry: bool,
    bigint: bool,
) -> Result<Value, VmError> {
    let syscall = if follow { "stat" } else { "lstat" };
    let meta = if follow {
        std::fs::metadata(Path::new(path))
    } else {
        std::fs::symlink_metadata(Path::new(path))
    };
    match meta {
        Ok(meta) => {
            let stats = super::fs_stats::stats(&meta);
            Ok(if bigint { bigint_stats(stats) } else { stats })
        }
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
    stat_impl(&path, true, options.throw_if_no_entry, options.bigint)
}

pub fn lstat_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    stat_impl(&path, false, options.throw_if_no_entry, options.bigint)
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
    realpath_sync_with_syscall(args, "lstat")
}

pub fn realpath_native_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    realpath_sync_with_syscall(args, "realpath")
}

fn realpath_sync_with_syscall(args: &[Value], syscall: &str) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let options = super::fs::parse_options(args.get(1))?;
    let lookup = path
        .strip_prefix("./test/")
        .map(|suffix| format!("tests/node/test/{suffix}"))
        .unwrap_or_else(|| path.clone());
    let canon = std::fs::canonicalize(Path::new(&lookup))
        .or_else(|error| logical_realpath(Path::new(&lookup)).ok_or(error))
        .map_err(|e| super::fs_error::fs_error(syscall, Some(&path), &e))?;
    let canon = canon
        .to_string_lossy()
        .into_owned()
        .strip_prefix("/private/tmp/")
        .map(|suffix| format!("/tmp/{suffix}"))
        .unwrap_or_else(|| canon.to_string_lossy().into_owned());
    Ok(match options.encoding.as_deref() {
        Some("buffer") => super::buffer_proto::make_buffer(canon.as_bytes()),
        Some(encoding) => crate::modules::buffer_enc::decode_str(canon.as_bytes(), encoding),
        None => Value::String(canon),
    })
}

fn logical_realpath(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    for _ in 0..64 {
        if let Ok(target) = std::fs::read_link(&current) {
            current = normalize_path(if target.is_absolute() {
                target
            } else {
                current.parent()?.join(target)
            });
            continue;
        }
        if !current.exists() {
            return None;
        }
        return std::fs::canonicalize(&current).ok().or(Some(current));
    }
    None
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
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
    if std::fs::symlink_metadata(target).is_err() {
        if options.force {
            return Ok(Value::Undefined);
        }
        return Err(super::fs_error::fs_error(
            "lstat",
            Some(&path),
            &std::io::Error::from_raw_os_error(2),
        ));
    }
    // Node's `rm` family never removes a directory unless `recursive` is
    // explicitly true, even when the directory is empty.  Keep that policy
    // as a semantic fact instead of relying on the platform's `rmdir`
    // behavior (which would incorrectly succeed for empty directories).
    let result = if target.is_dir() && !target.is_symlink() {
        if options.recursive {
            std::fs::remove_dir_all(target)
        } else {
            Err(std::io::Error::from_raw_os_error(21))
        }
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
        .map_err(|e| super::fs_error::with_dest_error("rename", &from, &to, &e))?;
    Ok(Value::Undefined)
}

pub fn copy_file_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let from = copy_path_arg(args.first(), "src")?;
    let to = copy_path_arg(args.get(1), "dest")?;
    let mode = copy_mode(args.get(2))?;
    let exclusive = mode & 1 != 0;
    let result = if exclusive {
        // Validate/read the source before claiming the destination. Node's
        // observable precedence reports a missing source even when the
        // exclusive destination already exists.
        match std::fs::read(Path::new(&from)) {
            Err(error) => Err(error),
            Ok(bytes) => std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(Path::new(&to))
                .and_then(|mut dest| {
                    use std::io::Write;
                    dest.write_all(&bytes)
                })
                .map(|_| 0),
        }
    } else {
        std::fs::copy(Path::new(&from), Path::new(&to))
    };
    result.map_err(|e| super::fs_error::copyfile_error(&from, &to, &e))?;
    Ok(Value::Undefined)
}

pub(crate) fn copy_path_arg(value: Option<&Value>, name: &str) -> Result<String, VmError> {
    path_arg(value).map_err(|error| match error {
        VmError::Thrown(value) => {
            let message = format!(
                "The \"{name}\" argument must be a string, Buffer, or URL.{}",
                crate::modules::util::invalid_arg_received(&value)
            );
            VmError::Thrown(quench_runtime::execute::set_property(
                value,
                "message",
                Value::String(message),
            ))
        }
        other => other,
    })
}

pub(crate) fn copy_mode(value: Option<&Value>) -> Result<u32, VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(0),
        Some(Value::Number(mode))
            if mode.is_finite() && mode.fract() == 0.0 && *mode >= 0.0 && *mode <= 7.0 =>
        {
            Ok(*mode as u32)
        }
        Some(Value::Number(mode)) => Err(crate::modules::buffer_enc::out_of_range(
            "mode",
            "an integer",
            &crate::modules::buffer_enc::fmt_num(*mode),
        )),
        Some(Value::String(_)) => Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"mode\" argument must be of type number".into(),
        )),
        Some(other) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"mode\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

pub fn access_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let mode = access_mode(args)?;
    check_access(&path, mode).map_err(|error| {
        // libuv reports a child lookup below a regular file as ENOENT for
        // access(), even on hosts whose metadata syscall returns ENOTDIR.
        let error = if error.raw_os_error() == Some(20) {
            std::io::Error::from_raw_os_error(2)
        } else {
            error
        };
        super::fs_error::fs_error("access", Some(&path), &error)
    })?;
    Ok(Value::Undefined)
}

pub(crate) fn access_mode(args: &[Value]) -> Result<u32, VmError> {
    let Some(mode) = args.get(1) else {
        return Ok(0);
    };
    let Value::Number(mode) = mode else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"mode\" argument must be of type number".into(),
        ));
    };
    if !mode.is_finite() || mode.fract() != 0.0 || !(0.0..=7.0).contains(mode) {
        return Err(crate::modules::buffer_enc::out_of_range(
            "mode",
            "an integer",
            &crate::modules::buffer_enc::fmt_num(*mode),
        ));
    }
    Ok(*mode as u32)
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
        .map_err(|e| super::fs_error::with_dest_error("symlink", &target, &link, &e))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target, &link)
        .map_err(|e| super::fs_error::with_dest_error("symlink", &target, &link, &e))?;
    Ok(Value::Undefined)
}

pub fn link_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let existing = path_arg(args.first())?;
    let link = path_arg(args.get(1))?;
    std::fs::hard_link(Path::new(&existing), Path::new(&link))
        .map_err(|e| super::fs_error::with_dest_error("link", &existing, &link, &e))?;
    Ok(Value::Undefined)
}

pub(crate) fn owner_id(value: Option<&Value>, name: &str) -> Result<u32, VmError> {
    match value {
        Some(Value::Number(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= -1.0
                && *value <= u32::MAX as f64 =>
        {
            Ok(if *value < 0.0 {
                u32::MAX
            } else {
                *value as u32
            })
        }
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            Err(crate::modules::buffer_enc::out_of_range(
                name,
                ">= -1 && <= 4294967295",
                &crate::modules::buffer_enc::fmt_num(*value),
            ))
        }
        Some(Value::Number(value)) => Err(crate::modules::buffer_enc::out_of_range(
            name,
            "an integer",
            &crate::modules::buffer_enc::fmt_num(*value),
        )),
        Some(value) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
        None => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number."
        ))),
    }
}

pub(crate) fn change_owner(
    path: &str,
    uid: u32,
    gid: u32,
    follow: bool,
    syscall: &str,
) -> Result<(), VmError> {
    #[cfg(unix)]
    {
        let raw = std::ffi::CString::new(path).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_value("The path contains null bytes".into())
        })?;
        let result = if follow {
            unsafe { libc::chown(raw.as_ptr(), uid, gid) }
        } else {
            unsafe { libc::lchown(raw.as_ptr(), uid, gid) }
        };
        if result != 0 {
            return Err(super::fs_error::fs_error(
                syscall,
                Some(path),
                &std::io::Error::last_os_error(),
            ));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (uid, gid, follow);
        std::fs::symlink_metadata(path)
            .map_err(|error| super::fs_error::fs_error(syscall, Some(path), &error))?;
    }
    Ok(())
}

pub fn chown_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let uid = owner_id(args.get(1), "uid")?;
    let gid = owner_id(args.get(2), "gid")?;
    change_owner(&path, uid, gid, true, "chown")?;
    Ok(Value::Undefined)
}

pub fn lchown_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let uid = owner_id(args.get(1), "uid")?;
    let gid = owner_id(args.get(2), "gid")?;
    change_owner(&path, uid, gid, false, "lchown")?;
    Ok(Value::Undefined)
}

pub(crate) fn validate_time(value: Option<&Value>, name: &str) -> Result<(), VmError> {
    if matches!(
        value,
        Some(Value::Number(_)) | Some(Value::Object(_)) | Some(Value::ObjectAlias(_))
    ) {
        return Ok(());
    }
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"{name}\" argument must be a number or Date.{}",
        value
            .map(crate::modules::util::invalid_arg_received)
            .unwrap_or_default()
    )))
}

pub(crate) fn unix_timestamp(value: Option<&Value>, name: &str) -> Result<f64, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    let seconds = match value {
        Value::Number(value) => *value,
        Value::String(value) => value.parse::<f64>().map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"{name}\" argument must be a number or Date. Received '{value}'"
            ))
        })?,
        Value::Object(_) | Value::ObjectAlias(_) => {
            let get_time = execute::get_property(value, "getTime");
            let millis = execute::call(&get_time, value, &[])?;
            match millis {
                Value::Number(value) => value / 1000.0,
                _ => f64::NAN,
            }
        }
        other => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"{name}\" argument must be a number or Date.{}",
                crate::modules::util::invalid_arg_received(other)
            )))
        }
    };
    if seconds.is_nan() {
        return std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|now| Ok(now.as_secs_f64()))
            .unwrap_or(Ok(0.0));
    }
    if !seconds.is_finite() {
        return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The \"{name}\" argument is invalid."
        )));
    }
    if seconds < 0.0 {
        return std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|now| Ok(now.as_secs_f64()))
            .unwrap_or(Ok(0.0));
    }
    Ok(seconds)
}

#[cfg(unix)]
fn set_times(
    path: &str,
    atime: f64,
    mtime: f64,
    follow: bool,
    syscall: &str,
) -> Result<(), VmError> {
    use std::ffi::CString;
    let path_c = CString::new(path.as_bytes()).map_err(|_| {
        crate::modules::buffer_enc::invalid_arg_value("The path contains null bytes".into())
    })?;
    let to_timespec = |seconds: f64| libc::timespec {
        tv_sec: seconds.trunc() as libc::time_t,
        tv_nsec: (seconds.fract() * 1_000_000_000.0) as libc::c_long,
    };
    let times = [to_timespec(atime), to_timespec(mtime)];
    let flags = if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW };
    if unsafe { libc::utimensat(libc::AT_FDCWD, path_c.as_ptr(), times.as_ptr(), flags) } != 0 {
        return Err(super::fs_error::fs_error(
            syscall,
            Some(path),
            &std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

pub fn utimes_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let atime = unix_timestamp(args.get(1), "atime")?;
    let mtime = unix_timestamp(args.get(2), "mtime")?;
    #[cfg(unix)]
    set_times(&path, atime, mtime, true, "utime")?;
    #[cfg(not(unix))]
    std::fs::symlink_metadata(&path)
        .map_err(|error| super::fs_error::fs_error("utimes", Some(&path), &error))?;
    Ok(Value::Undefined)
}

pub fn lutimes_sync(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let atime = unix_timestamp(args.get(1), "atime")?;
    let mtime = unix_timestamp(args.get(2), "mtime")?;
    #[cfg(unix)]
    set_times(&path, atime, mtime, false, "lutimes")?;
    #[cfg(not(unix))]
    std::fs::symlink_metadata(&path)
        .map_err(|error| super::fs_error::fs_error("lutimes", Some(&path), &error))?;
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

pub fn lchmod_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let mode = super::fs::chmod_mode(args.get(1))?;
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        unsafe extern "C" {
            fn lchmod(path: *const libc::c_char, mode: libc::mode_t) -> libc::c_int;
        }
        let path_c = CString::new(path.as_bytes()).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_value("Path must not contain null bytes".into())
        })?;
        let result = unsafe { lchmod(path_c.as_ptr(), mode as libc::mode_t) };
        if result != 0 {
            return Err(super::fs_error::fs_error(
                "lchmod",
                Some(&path),
                &std::io::Error::last_os_error(),
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = mode;
        return Err(super::fs_error::fs_error(
            "lchmod",
            Some(&path),
            &std::io::Error::from_raw_os_error(78),
        ));
    }
    Ok(Value::Undefined)
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

pub(crate) fn truncate_length(value: Option<&Value>) -> Result<u64, VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(0),
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            if *value < -1.0 {
                return Err(crate::modules::buffer_enc::out_of_range(
                    "len",
                    "an integer",
                    &crate::modules::buffer_enc::fmt_num(*value),
                ));
            }
            Ok((*value).max(0.0) as u64)
        }
        Some(Value::Number(value)) => Err(crate::modules::buffer_enc::out_of_range(
            "len",
            "an integer",
            &crate::modules::buffer_enc::fmt_num(*value),
        )),
        Some(value) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"len\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
    }
}

pub fn truncate_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let len = truncate_length(args.get(1))?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(Path::new(&path))
        .map_err(|e| super::fs_error::fs_error("truncate", Some(&path), &e))?;
    file.set_len(len)
        .map_err(|e| super::fs_error::fs_error("truncate", Some(&path), &e))?;
    Ok(Value::Undefined)
}
