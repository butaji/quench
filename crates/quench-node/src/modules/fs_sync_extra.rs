use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::fs::{parse_options, path_arg, FsOptions};
use crate::modules::fs_error;

fn split(args: &[Value]) -> Result<(String, FsOptions), VmError> {
    let path = path_arg(args.first())?;
    let options = parse_options(args.get(1))?;
    Ok((path, options))
}

pub fn readlink_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, options) = split(args)?;
    let target = std::fs::read_link(Path::new(&path))
        .map_err(|e| fs_error::fs_error("readlink", Some(&path), &e))?;
    let text = target.to_string_lossy().into_owned();
    Ok(match &options.encoding {
        Some(encoding) if encoding != "utf8" => {
            crate::modules::buffer_enc::decode_str(text.as_bytes(), encoding)
        }
        Some(_) | None => Value::String(text),
    })
}

pub fn chmod_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let mode = match args.get(1) {
        Some(Value::Number(m)) => *m as u32,
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
            .map_err(|e| fs_error::fs_error("chmod", Some(path), &e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        let err = std::io::Error::from_raw_os_error(78);
        return Err(fs_error::fs_error("chmod", Some(path), &err));
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
        .map_err(|e| fs_error::fs_error("truncate", Some(&path), &e))?;
    file.set_len(len)
        .map_err(|e| fs_error::fs_error("truncate", Some(&path), &e))?;
    Ok(Value::Undefined)
}

/// `chown(path, uid, gid)` — change ownership.
pub fn chown_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let uid = nonneg_u32(args, 1, "uid")?;
    let gid = nonneg_u32(args, 2, "gid")?;
    chown_apply(&path, uid, gid, false, "chown")
}

fn nonneg_u32(args: &[Value], at: usize, name: &str) -> Result<u32, VmError> {
    match args.get(at) {
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => Ok(*n as u32),
        _ => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number."
        ))),
    }
}

fn chown_apply(
    path: &str,
    uid: u32,
    gid: u32,
    follow_symlinks: bool,
    op: &str,
) -> Result<Value, VmError> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let cpath = CString::new(path.as_bytes()).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_type("path must not contain null bytes".into())
        })?;
        let raw = unsafe {
            if follow_symlinks {
                libc::lchown(cpath.as_ptr(), uid as libc::uid_t, gid as libc::gid_t)
            } else {
                libc::chown(cpath.as_ptr(), uid as libc::uid_t, gid as libc::gid_t)
            }
        };
        if raw == -1 {
            return Err(fs_error::fs_error(
                op,
                Some(path),
                &std::io::Error::last_os_error(),
            ));
        }
        Ok(Value::Undefined)
    }
    #[cfg(not(unix))]
    {
        let _ = (uid, gid, follow_symlinks);
        Err(fs_error::fs_error(
            op,
            Some(path),
            &std::io::Error::from_raw_os_error(78),
        ))
    }
}

pub fn utimes_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let atime = utimes_time(args.get(1), "atime")?;
    let mtime = utimes_time(args.get(2), "mtime")?;
    utimensat_apply(&path, atime, mtime, 0, "utimes")
}
pub fn link_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let old = path_arg(args.first())?;
    let new = path_arg(args.get(1))?;
    std::fs::hard_link(Path::new(&old), Path::new(&new))
        .map_err(|e| fs_error::fs_error("link", Some(&old), &e))?;
    Ok(Value::Undefined)
}

pub fn symlink_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = path_arg(args.first())?;
    let path = path_arg(args.get(1))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &path)
        .map_err(|e| fs_error::fs_error("symlink", Some(&path), &e))?;
    #[cfg(not(unix))]
    return Err(fs_error::fs_error(
        "symlink",
        Some(&path),
        &std::io::Error::from_raw_os_error(78),
    ));
    Ok(Value::Undefined)
}

/// Coerce a `utimes`/`lutimes` time argument (a `Date` or a numeric
/// seconds value) to seconds since the epoch, matching Node. Rejects
/// anything else with `ERR_INVALID_ARG_TYPE`.
pub(crate) fn utimes_time(value: Option<&Value>, name: &str) -> Result<f64, VmError> {
    match value {
        Some(Value::Number(n)) if n.is_finite() => Ok(*n),
        Some(v) => match v {
            Value::Object(_) => {
                let ms = quench_runtime::date::extract_time(Some(v));
                if ms.is_nan() {
                    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                        "The {name} argument must be a number or Date"
                    )))
                } else {
                    Ok(ms / 1000.0)
                }
            }
            _ => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The {name} argument must be a number or Date"
            ))),
        },
        _ => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The {name} argument must be a number or Date"
        ))),
    }
}

pub fn lutimes_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let atime = utimes_time(args.get(1), "atime")?;
    let mtime = utimes_time(args.get(2), "mtime")?;
    utimensat_apply(&path, atime, mtime, libc::AT_SYMLINK_NOFOLLOW, "lutimes")
}

fn utimensat_apply(
    path: &str,
    atime: f64,
    mtime: f64,
    flags: i32,
    op: &str,
) -> Result<Value, VmError> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let cpath = CString::new(path.as_bytes()).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_type("path must not contain null bytes".into())
        })?;
        let ts = |t: f64| {
            let sec = t.floor() as libc::time_t;
            let nsec = ((t - sec as f64) * 1_000_000_000.0) as libc::c_long;
            libc::timespec {
                tv_sec: sec,
                tv_nsec: nsec,
            }
        };
        let times = [ts(atime), ts(mtime)];
        if unsafe { libc::utimensat(libc::AT_FDCWD, cpath.as_ptr(), times.as_ptr(), flags) } == -1 {
            return Err(fs_error::fs_error(
                op,
                Some(path),
                &std::io::Error::last_os_error(),
            ));
        }
        Ok(Value::Undefined)
    }
    #[cfg(not(unix))]
    {
        let _ = (atime, mtime, flags);
        Err(fs_error::fs_error(
            op,
            Some(path),
            &std::io::Error::from_raw_os_error(78),
        ))
    }
}

pub fn lchown_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let uid = nonneg_u32(args, 1, "uid")?;
    let gid = nonneg_u32(args, 2, "gid")?;
    chown_apply(&path, uid, gid, true, "lchown")
}
pub fn lchmod_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let mode = match args.get(1) {
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as u32,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"mode\" argument must be a number.".into(),
            ))
        }
    };
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(Path::new(&path), std::fs::Permissions::from_mode(mode))
            .map_err(|e| fs_error::fs_error("lchmod", Some(&path), &e))?;
        Ok(Value::Undefined)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = mode;
        Err(fs_error::fs_error(
            "lchmod",
            Some(&path),
            &std::io::Error::from_raw_os_error(78),
        ))
    }
}

pub fn mkdtemp_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let prefix = path_arg(args.first())?;
    for attempt in 0..100u32 {
        let candidate = format!("{prefix}{:06x}", random_suffix(attempt));
        match std::fs::create_dir(Path::new(&candidate)) {
            Ok(()) => return Ok(Value::String(candidate)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(fs_error::fs_error("mkdtemp", Some(&prefix), &e)),
        }
    }
    let err = std::io::Error::from_raw_os_error(17);
    Err(fs_error::fs_error("mkdtemp", Some(&prefix), &err))
}

fn random_suffix(attempt: u32) -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    now ^ std::process::id().wrapping_mul(2654435761) ^ attempt.wrapping_mul(40503)
}

pub fn unlink_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    std::fs::remove_file(Path::new(&path))
        .map_err(|e| fs_error::fs_error("unlink", Some(&path), &e))?;
    Ok(Value::Undefined)
}

pub fn rmdir_sync(
    _s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    std::fs::remove_dir(Path::new(&path))
        .map_err(|e| fs_error::fs_error("rmdir", Some(&path), &e))?;
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
        Err(e) => Err(fs_error::fs_error("rm", Some(&path), &e)),
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
        .map_err(|e| fs_error::fs_error("rename", Some(&from), &e))?;
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
    result.map_err(|e| fs_error::fs_error("copyfile", Some(&from), &e))?;
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
    check_access(&path, mode).map_err(|e| fs_error::fs_error("access", Some(&path), &e))?;
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
