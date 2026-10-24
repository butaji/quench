//! Raw directory scanning for the configured Node filesystem policy.
use super::fs_shared_vm::{self as fs, UV_DIRENT_DIR, UV_DIRENT_LINK, UV_DIRENT_UNKNOWN};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Path, PathBuf};
type Context<'a> = NativeContext<'a, NodeHost>;
struct Entry {
    name: std::ffi::OsString,
    kind: i32,
}

#[cfg(target_os = "linux")]
fn scan(path: &Path) -> std::io::Result<Vec<Entry>> {
    use std::{
        ffi::{CStr, CString},
        os::unix::ffi::{OsStrExt, OsStringExt},
    };
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let path = CString::new(path.as_os_str().as_bytes())?;
    // The guard owns the directory; each returned name is copied before readdir
    // reuses its storage. errno distinguishes EOF from an actual scan failure.
    let pointer = unsafe { libc::opendir(path.as_ptr()) };
    if pointer.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let directory = Directory(pointer);
    let mut entries = Vec::new();
    loop {
        unsafe {
            *libc::__errno_location() = 0;
        }
        let entry = unsafe { libc::readdir(directory.0) };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error);
            }
            break;
        }
        let entry = unsafe { &*entry };
        let name = unsafe { CStr::from_ptr(entry.d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        let kind = match entry.d_type {
            libc::DT_REG => fs::UV_DIRENT_FILE,
            libc::DT_DIR => UV_DIRENT_DIR,
            libc::DT_LNK => UV_DIRENT_LINK,
            libc::DT_FIFO => fs::UV_DIRENT_FIFO,
            libc::DT_SOCK => fs::UV_DIRENT_SOCKET,
            libc::DT_CHR => fs::UV_DIRENT_CHAR,
            libc::DT_BLK => fs::UV_DIRENT_BLOCK,
            _ => UV_DIRENT_UNKNOWN,
        };
        entries.push(Entry {
            name: std::ffi::OsString::from_vec(name.to_vec()),
            kind,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}
#[cfg(not(target_os = "linux"))]
fn scan(path: &Path) -> std::io::Result<Vec<Entry>> {
    let mut entries = std::fs::read_dir(path)?
        .map(|entry| {
            let entry = entry?;
            Ok(Entry {
                name: entry.file_name(),
                kind: fs::stat::directory_entry_type(entry.file_type()?),
            })
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}
fn bytes(path: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn encode(
    c: &mut Context<'_>,
    name: &std::ffi::OsStr,
    encoding: &str,
) -> Result<RootId, RootedError> {
    let bytes = bytes(name);
    if encoding == "buffer" {
        super::buffer_shared_vm::from_bytes(c, &bytes)
    } else {
        super::buffer_string_shared_vm::decode_bytes(c, &bytes, encoding)
    }
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:fs", |c| {
        let binding = c.object_rooted()?;
        for (name, operation) in [
            ("writeFileUtf8", "fsBindingWriteFileUtf8"),
            ("readdir", "fsBindingReaddir"),
            ("readdirRecursive", "fsBindingReaddirRecursive"),
            ("internalModuleStat", "fsBindingInternalModuleStat"),
        ] {
            let function = c.host_function(crate::host::shared_vm::operation(operation))?;
            let key = c.string_rooted("name");
            let value = c.string_rooted(name);
            c.define_data_property_rooted(function, key, value, false, false, true)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(binding, key, function, true, true, true)?;
        }
        Ok(binding)
    })
}
/// The SDK's allocation-free module/directory classifier follows symlinks and
/// returns libuv status instead of constructing Stat or Error objects.
pub(crate) fn internal_module_stat(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("internalModuleStat requires a path"))?;
    let mut path = c
        .string_text(input)?
        .ok_or_else(|| RootedError::host("internalModuleStat requires a string"))?;
    // The native SDK consumes BufferValue's C string, including its NUL boundary.
    if let Some(end) = path.find('\0') {
        path.truncate(end);
    }
    let path = c
        .host_mut()
        .shared_state()
        .borrow()
        .cwd
        .resolve_io_path(Path::new(&path));
    let status = match std::fs::metadata(path) {
        Ok(metadata) => i32::from(metadata.is_dir()),
        Err(error) => super::fs_error_codes::code_for(&error).1,
    };
    Ok(c.number(f64::from(status)))
}

fn read(c: &mut Context<'_>, args: &[RootId], recursive: bool) -> Result<RootId, RootedError> {
    if args.len() != 3 {
        return Err(RootedError::host(
            "asynchronous directory binding is unavailable",
        ));
    }
    let input = super::fs_path::native_path(c, args[0])?;
    let root = super::fs_path::resolve(c, args[0])?;
    let encoding = c.string_text(args[1])?.unwrap_or_else(|| "utf8".into());
    let encoding = if encoding == "buffer" {
        "buffer"
    } else {
        super::buffer_codec::canonical_encoding(&encoding).unwrap_or("utf8")
    };
    let enabled = c.boolean(true);
    let with_types = c.strict_equal_rooted(args[2], enabled)?;
    let mut directories = vec![PathBuf::new()];
    let mut names = Vec::new();
    let mut types = Vec::new();
    let mut counts = Vec::new();
    let mut dirs = Vec::new();
    let mut index = 0;
    while index < directories.len() {
        let relative = directories[index].clone();
        let path = root.join(&relative);
        let entries = scan(&path).map_err(|error| error_at(c, error, &input, &relative))?;
        if recursive && with_types {
            counts.push(entries.len() as u32);
            dirs.push(encode(c, relative.as_os_str(), encoding)?);
        }
        for mut entry in entries {
            if recursive {
                let child = path.join(&entry.name);
                if entry.kind == UV_DIRENT_UNKNOWN {
                    if let Ok(metadata) = std::fs::symlink_metadata(&child) {
                        entry.kind = fs::stat::directory_entry_type(metadata.file_type());
                    }
                }
                let is_directory = entry.kind == UV_DIRENT_DIR
                    || (entry.kind == UV_DIRENT_LINK
                        && std::fs::metadata(&child).is_ok_and(|m| m.is_dir()));
                if is_directory {
                    directories.push(relative.join(&entry.name));
                }
            }
            let name = if recursive && !with_types {
                relative.join(&entry.name).into_os_string()
            } else {
                entry.name
            };
            names.push(encode(c, &name, encoding)?);
            if with_types {
                types.push(entry.kind as u8);
            }
        }
        index += 1;
        if !recursive {
            break;
        }
    }
    let names = c.array_rooted(&names)?;
    if !with_types {
        return Ok(names);
    }
    if !recursive {
        let values = types
            .into_iter()
            .map(|kind| c.number(f64::from(kind)))
            .collect::<Vec<_>>();
        let types = c.array_rooted(&values)?;
        return c.array_rooted(&[names, types]);
    }
    // Node's recursive ABI shares one backing store for the byte type tags and
    // naturally aligned uint32 directory counts.
    let alignment = std::mem::size_of::<u32>();
    let offset = types.len().div_ceil(alignment) * alignment;
    let type_count = types.len();
    let mut storage = types;
    storage.resize(offset, 0);
    for count in &counts {
        storage.extend_from_slice(&count.to_ne_bytes());
    }
    let (types, counts) = c.with_startup_intrinsics(|c| {
        let storage = c.uint8_array_rooted(&storage)?;
        let buffer = get(c, storage, "buffer")?;
        let global = c.global_root()?;
        let u8_constructor = get(c, global, "Uint8Array")?;
        let u32_constructor = get(c, global, "Uint32Array")?;
        let zero = c.number(0.0);
        let length = c.number(type_count as f64);
        let types = c.construct_rooted(u8_constructor, u8_constructor, &[buffer, zero, length])?;
        let offset = c.number(offset as f64);
        let length = c.number(counts.len() as f64);
        let counts =
            c.construct_rooted(u32_constructor, u32_constructor, &[buffer, offset, length])?;
        Ok((types, counts))
    })?;
    let dirs = c.array_rooted(&dirs)?;
    c.array_rooted(&[names, types, counts, dirs])
}
fn error_at(
    c: &mut Context<'_>,
    error: std::io::Error,
    root: &Path,
    relative: &Path,
) -> RootedError {
    let path = if relative.as_os_str().is_empty() {
        root.to_owned()
    } else {
        root.join(relative)
    };
    fs::stream_io_error(c, error, "scandir", &path.to_string_lossy()).unwrap_or_else(|error| error)
}
pub(crate) fn readdir(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    read(c, args, false)
}
pub(crate) fn readdir_recursive(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    read(c, args, true)
}
