use super::get;
use crate::host::NodeHost;
use crate::modules::{fs_error_details, fs_ops as ops, fs_shared_vm as shared_vm};
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

const SYNC_API: &str = r#"(mkdirSync, rmdirSync, mkdtempSync, rmSync, copyFileSync, symlinkSync, linkSync, renameSync, unlinkSync, chownSync, lchownSync, fchownSync, truncateSync, ftruncateSync, openSync, closeSync, fstatSync, readSyncNative, writeSyncNative, syncDescriptor) => {
  let warnedMkdtempX = false;
  const normalizePath = (path) =>
    typeof path === 'string' ? path : Buffer.isBuffer(path) ? path.toString() : ArrayBuffer.isView(path) && !(path instanceof DataView) ? Buffer.from(path).toString() : path instanceof URL ? decodeURIComponent(path.pathname) : path;
  const validatePath = (path, name) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${String(path)})`;
    const error = new TypeError(`The "${name}" argument must be of type string, Buffer, or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  const validateNoNulPath = (path) => {
    const normalized = normalizePath(path);
    if (typeof normalized !== 'string' || !normalized.includes('\0')) return;
    const received = normalized.replace(/\0/g, '\\x00');
    const error = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${received}'`);
    error.code = 'ERR_INVALID_ARG_VALUE';
    throw error;
  };
  const validateSymlinkType = (type) => {
    if (type === undefined || type === 'file' || type === 'dir' || type === 'junction') return;
    const error = new TypeError('The "type" argument must be one of: "dir", "file", "junction".');
    error.code = typeof type === 'string' ? 'ERR_INVALID_ARG_VALUE' : 'ERR_INVALID_ARG_VALUE';
    throw error;
  };
  const validateRenamePath = (path, name) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${String(path)})`;
    const error = new TypeError(`The "${name}" argument must be of type string or an instance of Buffer or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  const validateOwner = (name, value) => {
    if (typeof value !== 'number') {
      const received = value === null || value === undefined
        ? ` Received ${value}`
        : typeof value === 'object'
          ? ` Received an instance of ${Array.isArray(value) ? 'Array' : 'Object'}`
          : ` Received type ${typeof value} (${String(value)})`;
      const error = new TypeError(`The "${name}" argument must be of type number.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(value) || !Number.isFinite(value) || value < -1 || value > 0xFFFFFFFF) {
      const range = Number.isInteger(value) ? '>= -1 && <= 4294967295' : 'an integer';
      const error = new RangeError(`The value of "${name}" is out of range. It must be ${range}. Received ${String(value)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  };
  const validateLength = (length) => {
    if (length === undefined) return 0;
    if (typeof length !== 'number') {
      const received = length === null ? ' Received null' : typeof length === 'object'
        ? ` Received an instance of ${Array.isArray(length) ? 'Array' : 'Object'}`
        : ` Received type ${typeof length} (${typeof length === 'string' ? `'${length}'` : String(length)})`;
      const error = new TypeError(`The "len" argument must be of type number.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isSafeInteger(length)) {
      const error = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${String(length)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    return Math.max(0, length);
  };
  const makeMkdtemp = (prefix) => {
    const normalized = normalizePath(prefix);
    if (!warnedMkdtempX && typeof normalized === 'string' && normalized.endsWith('X')) {
      warnedMkdtempX = true;
      process.emitWarning('mkdtemp() templates ending with X are not portable. For details see: https://nodejs.org/api/fs.html');
    }
    return mkdtempSync(normalized);
  };
  const normalizeMode = (mode) => {
    if (typeof mode === "string") {
      const parsed = Number.parseInt(mode, 8);
      if (Number.isNaN(parsed)) {
        const error = new TypeError('The "mode" argument must be a valid integer');
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      return parsed;
    }
    if (mode != null && typeof mode !== "number") {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return mode;
  };
  const validateFd = (fd) => {
    if (typeof fd !== "number") {
      let received;
      if (fd === null || fd === undefined) received = ` Received ${fd}`;
      else if (typeof fd === "object") received = ` Received an instance of ${Array.isArray(fd) ? "Array" : "Object"}`;
      else received = ` Received type ${typeof fd} (${typeof fd === "string" ? `'${fd}'` : String(fd)})`;
      const error = new TypeError(`The "fd" argument must be of type number.${received}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(fd)) {
      const error = new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${String(fd)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (fd < 0 || fd > 0x7FFFFFFF) {
      const error = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${String(fd)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  };
  const validateReadWriteRange = (buffer, offset, length) => {
    for (const [name, value] of [["offset", offset], ["length", length]]) {
      if (value != null && typeof value !== "number") {
        const error = new TypeError(`The "${name}" argument must be of type number.`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    }
    offset ??= 0;
    length ??= buffer.byteLength - offset;
    if (!Number.isInteger(offset)) {
      const error = new RangeError(`The value of "offset" is out of range. It must be an integer. Received ${String(offset)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset < 0 || offset > buffer.byteLength) {
      const error = new RangeError(offset > buffer.byteLength
        ? `The value of "offset" is out of range. It must be <= ${buffer.byteLength}. Received ${offset}`
        : `The value of "offset" is out of range. Received ${offset}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isInteger(length) || length < 0 || length > buffer.byteLength - offset) {
      const error = new RangeError(length < 0
        ? `The value of "length" is out of range. It must be >= 0. Received ${length}`
        : length > buffer.byteLength - offset
          ? `The value of "length" is out of range. It must be <= ${buffer.byteLength - offset}. Received ${length}`
        : `The value of "length" is out of range. Received ${String(length)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return [offset, length];
  };
  return {
    mkdirSync(path, options) {
      return mkdirSync(normalizePath(path), options);
    },
    rmdirSync(path, options) {
      if (options != null && typeof options !== 'object') {
        const error = new TypeError('The "options" argument must be of type object.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (options?.recursive === true) {
        const error = new TypeError('The recursive option is no longer supported.');
        error.code = 'ERR_INVALID_ARG_VALUE';
        throw error;
      }
      return rmdirSync(normalizePath(path));
    },
    mkdtempSync(prefix) {
      return makeMkdtemp(prefix);
    },
    mkdtempDisposableSync(prefix) {
      const absolutePath = makeMkdtemp(prefix);
      const normalizedPrefix = normalizePath(prefix);
      const path = normalizedPrefix.startsWith('/')
        ? absolutePath
        : `${normalizedPrefix}${absolutePath.slice(-6)}`;
      const remove = () => rmSync(absolutePath, { recursive: true, force: true });
      const disposable = { path, remove };
      disposable[Symbol.dispose] = remove;
      return disposable;
    },
    rmSync(path, options) {
      return rmSync(normalizePath(path), options);
    },
    copyFileSync(source, destination, mode) {
      validatePath(source, 'src');
      validatePath(destination, 'dest');
      validateNoNulPath(source);
      validateNoNulPath(destination);
      if (mode == null) mode = 0;
      if (typeof mode !== 'number') {
        const error = new TypeError('The "mode" argument must be of type number.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (!Number.isInteger(mode) || mode < 0 || mode > 7) {
        const error = new RangeError('The "mode" argument is out of range.');
        error.code = 'ERR_OUT_OF_RANGE';
        throw error;
      }
      return copyFileSync(normalizePath(source), normalizePath(destination), mode);
    },
    symlinkSync(target, path, type) {
      validatePath(target, 'target');
      validatePath(path, 'path');
      validateSymlinkType(type);
      return symlinkSync(normalizePath(target), normalizePath(path), type);
    },
    linkSync(existingPath, newPath) {
      validatePath(existingPath, 'existingPath');
      validatePath(newPath, 'newPath');
      return linkSync(normalizePath(existingPath), normalizePath(newPath));
    },
    renameSync(oldPath, newPath) {
      validateRenamePath(oldPath, 'oldPath');
      validateRenamePath(newPath, 'newPath');
      return renameSync(normalizePath(oldPath), normalizePath(newPath));
    },
    unlinkSync(path) {
      return unlinkSync(normalizePath(path));
    },
    chownSync(path, uid, gid) {
      validateRenamePath(path, 'path');
      validateOwner('uid', uid);
      validateOwner('gid', gid);
      return chownSync(normalizePath(path), uid, gid);
    },
    lchownSync(path, uid, gid) {
      validateRenamePath(path, 'path');
      validateOwner('uid', uid);
      validateOwner('gid', gid);
      return lchownSync(normalizePath(path), uid, gid);
    },
    fchownSync(fd, uid, gid) {
      validateFd(fd);
      validateOwner('uid', uid);
      validateOwner('gid', gid);
      return fchownSync(fd, uid, gid);
    },
    truncateSync(path, length) {
      validatePath(path, 'path');
      return truncateSync(normalizePath(path), validateLength(length));
    },
    ftruncateSync(fd, length) {
      validateFd(fd);
      return ftruncateSync(fd, validateLength(length));
    },
    openSync(path, flags, mode) {
      return openSync(path, flags, mode);
    },
    closeSync(fd) {
      validateFd(fd);
      return closeSync(fd);
    },
    fstatSync(fd, options) {
      validateFd(fd);
      return fstatSync(fd, options);
    },
    readSync(fd, buffer, offset, length, position) {
      return readSyncNative(fd, buffer, offset, length, position, arguments.length);
    },
    writeSync(fd, buffer, offset, length, position) {
      return writeSyncNative(fd, buffer, offset, length, position);
    },
    fsyncSync(fd) { validateFd(fd); return syncDescriptor(fd, false); },
    fdatasyncSync(fd) { validateFd(fd); return syncDescriptor(fd, true); },
  };
}"#;

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    let mkdir = context.host_function(crate::host::shared_vm::operation("fsMkdirSync"))?;
    let rmdir = context.host_function(crate::host::shared_vm::operation("fsRmdirSync"))?;
    let mkdtemp = context.host_function(crate::host::shared_vm::operation("fsMkdtempSync"))?;
    let copy_file = context.host_function(crate::host::shared_vm::operation("fsCopyFileSync"))?;
    let symlink = context.host_function(crate::host::shared_vm::operation("fsSymlinkSync"))?;
    let link = context.host_function(crate::host::shared_vm::operation("fsLinkSync"))?;
    let rename = context.host_function(crate::host::shared_vm::operation("fsRenameSync"))?;
    let unlink = context.host_function(crate::host::shared_vm::operation("fsUnlinkSync"))?;
    let chown = context.host_function(crate::host::shared_vm::operation("fsChownSync"))?;
    let lchown = context.host_function(crate::host::shared_vm::operation("fsLchownSync"))?;
    let fchown = context.host_function(crate::host::shared_vm::operation("fsFchownSync"))?;
    let truncate = context.host_function(crate::host::shared_vm::operation("fsTruncateSync"))?;
    let ftruncate = context.host_function(crate::host::shared_vm::operation("fsFtruncateSync"))?;
    let rm = context.host_function(crate::host::shared_vm::operation("fsRmSync"))?;
    let open = context.host_function(crate::host::shared_vm::operation("fsOpenSync"))?;
    let close = context.host_function(crate::host::shared_vm::operation("fsCloseSync"))?;
    let fstat = context.host_function(crate::host::shared_vm::operation("fsFstatSync"))?;
    let read_sync = context.host_function(crate::host::shared_vm::operation("fsReadSync"))?;
    let write_sync = context.host_function(crate::host::shared_vm::operation("fsWriteSync"))?;
    let sync_descriptor =
        context.host_function(crate::host::shared_vm::operation("fsSyncDescriptor"))?;
    let factory = context.evaluate_script_rooted(SYNC_API, "node:fs/shared-sync.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(
        factory,
        undefined,
        &[
            mkdir,
            rmdir,
            mkdtemp,
            rm,
            copy_file,
            symlink,
            link,
            rename,
            unlink,
            chown,
            lchown,
            fchown,
            truncate,
            ftruncate,
            open,
            close,
            fstat,
            read_sync,
            write_sync,
            sync_descriptor,
        ],
    )?;
    for (name, method) in [
        ("mkdirSync", "mkdirSync"),
        ("rmdirSync", "rmdirSync"),
        ("mkdtempSync", "mkdtempSync"),
        ("mkdtempDisposableSync", "mkdtempDisposableSync"),
        ("copyFileSync", "copyFileSync"),
        ("symlinkSync", "symlinkSync"),
        ("linkSync", "linkSync"),
        ("renameSync", "renameSync"),
        ("unlinkSync", "unlinkSync"),
        ("chownSync", "chownSync"),
        ("lchownSync", "lchownSync"),
        ("fchownSync", "fchownSync"),
        ("truncateSync", "truncateSync"),
        ("ftruncateSync", "ftruncateSync"),
        ("rmSync", "rmSync"),
        ("openSync", "openSync"),
        ("closeSync", "closeSync"),
        ("fstatSync", "fstatSync"),
        ("readSync", "readSync"),
        ("writeSync", "writeSync"),
        ("fsyncSync", "fsyncSync"),
        ("fdatasyncSync", "fdatasyncSync"),
    ] {
        let key = context.string_rooted(method);
        let function = context.get_property_rooted(api, key)?;
        shared_vm::set(context, module, name, function)?;
    }
    Ok(())
}

pub(crate) fn open_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let value = args.first().copied().unwrap_or(undefined);
    let flag = args.get(1).copied().unwrap_or(undefined);
    let mode = args.get(2).copied().unwrap_or(undefined);
    let flags = crate::modules::fs_policy_shared_vm::call(context, "stringToFlags", &[flag])?;
    let name = context.string_rooted("mode");
    let default_mode = context.number(0o666 as f64);
    let mode = crate::modules::fs_policy_shared_vm::call(
        context,
        "parseFileMode",
        &[mode, name, default_mode],
    )?;
    // SDK fs.js openSync dispatches after flag/mode policy and before path
    // validation. Keep the original path and the numeric policy roots intact.
    if let Some(result) =
        crate::modules::fs_policy_shared_vm::vfs_read(context, "openSync", &[value, flags, mode])?
    {
        return Ok(result);
    }
    let flags = context.to_number(flags)? as i32;
    let mode = context.to_number(mode)? as u32;
    let value = crate::modules::fs_path::validate(context, value, "path")?;
    let path = crate::modules::fs_path::resolve(context, value)?;
    let display = crate::modules::fs_path::display(context, value)?;
    #[cfg(unix)]
    let opened = ops::open_numeric(&path, flags, Some(mode));
    #[cfg(not(unix))]
    let opened = {
        let _ = flags;
        ops::open(&path, context.string_text(flag)?.as_deref(), Some(mode))
    };
    let file = match opened {
        Ok(file) => file,
        Err(error) => return throw_io_error(context, "open", Some(&display), &error),
    };
    let inserted = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .insert_descriptor(file, display.clone());
    match inserted {
        Ok(fd) => Ok(context.number(fd as f64)),
        Err(error) => throw_io_error(context, "open", Some(&display), &error),
    }
}

pub(crate) fn close_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let result = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .close_stream(fd);
    match result {
        Ok(_) => Ok(context.undefined()),
        Err(error) => throw_io_error(context, "close", None, &error),
    }
}

const READ_SYNC_POSITIONAL_ARITY: usize = 5;
const READ_SYNC_OPTIONS_ARITY: usize = 3;

/// Release-specific overload and validation policy executes through the configured
/// Node validators; the actual byte copy remains a native/Core operation.
pub(crate) fn read_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use crate::modules::fs_policy_shared_vm as policy;
    let undefined = context.undefined();
    let zero = context.number(0.0);
    let fd = args.first().copied().unwrap_or(undefined);
    let buffer = args.get(1).copied().unwrap_or(undefined);
    policy::call(context, "validateBuffer", &[buffer])?;
    let mut offset = args.get(2).copied().unwrap_or(undefined);
    let mut length = args.get(3).copied().unwrap_or(undefined);
    let mut position = args.get(4).copied().unwrap_or(undefined);
    // readSync(fd, buffer, options) is selected by the original caller arity.
    let count = args
        .get(READ_SYNC_POSITIONAL_ARITY)
        .copied()
        .ok_or_else(|| RootedError::host("readSync caller arity missing"))?;
    let count = context.to_number(count)?;
    if count <= READ_SYNC_OPTIONS_ARITY as f64
        || context.rooted_value(offset).is_some_and(Value::is_null)
        || context.is_object_rooted(offset)?
    {
        let options = offset;
        if !context
            .rooted_value(options)
            .is_some_and(Value::is_undefined)
        {
            let name = context.string_rooted("options");
            let nullable = policy::export(context, "kValidateObjectAllowNullable")?;
            policy::call(context, "validateObject", &[options, name, nullable])?;
        }
        let empty = context
            .rooted_value(options)
            .is_some_and(|v| v.is_null() || v.is_undefined());
        offset = if empty {
            undefined
        } else {
            get(context, options, "offset")?
        };
        if context
            .rooted_value(offset)
            .is_some_and(Value::is_undefined)
        {
            offset = zero;
        }
        length = if empty {
            undefined
        } else {
            get(context, options, "length")?
        };
        if context
            .rooted_value(length)
            .is_some_and(Value::is_undefined)
        {
            let capacity = get(context, buffer, "byteLength")?;
            length = context.subtract_rooted(capacity, offset)?;
        }
        position = if empty {
            undefined
        } else {
            get(context, options, "position")?
        };
        if context
            .rooted_value(position)
            .is_some_and(Value::is_undefined)
        {
            position = context.null();
        }
    }
    if context
        .rooted_value(offset)
        .is_some_and(Value::is_undefined)
    {
        offset = zero;
    } else {
        let name = context.string_rooted("offset");
        policy::call(context, "validateInteger", &[offset, name, zero])?;
    }
    length = context.bitwise_or_rooted(length, zero)?;
    if context
        .rooted_value(position)
        .is_some_and(|v| v.is_null() || v.is_undefined())
    {
        position = context.number(-1.0);
    } else {
        let name = context.string_rooted("position");
        policy::call(context, "validatePosition", &[position, name, length])?;
    }
    if context.strict_equal_rooted(length, zero)? {
        return Ok(zero);
    }
    let capacity = get(context, buffer, "byteLength")?;
    if context.strict_equal_rooted(capacity, zero)? {
        let name = context.string_rooted("buffer");
        let reason = context.string_rooted("is empty and cannot be written");
        return Err(policy::invalid_value(context, name, buffer, reason)?);
    }
    let capacity = get(context, buffer, "byteLength")?;
    policy::call(
        context,
        "validateOffsetLengthRead",
        &[offset, length, capacity],
    )?;
    if let Some(result) =
        policy::vfs_read(context, "readSync", &[fd, buffer, offset, length, position])?
    {
        return Ok(result);
    }
    let maximum = context.number(i32::MAX as f64);
    crate::modules::util_shared_vm::validate_binding_integer(context, fd, "fd", 0.0, maximum)?;
    read_into(context, undefined, &[fd, buffer, offset, length, position])
}

fn read_into(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let buffer = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let offset = super::integer_arg(context, args.get(2).copied(), "offset")? as usize;
    let size = super::integer_arg(context, args.get(3).copied(), "length")? as usize;
    let position_value = args[4];
    let position = if let Some(number) = context
        .rooted_value(position_value)
        .and_then(Value::as_number)
    {
        (number >= 0.0).then_some(number as u64)
    } else {
        // validatePosition has already established an in-range BigInt primitive.
        let integer = context
            .to_string(position_value)?
            .parse::<i128>()
            .map_err(|_| RootedError::host("validated file position is not an integer"))?;
        (integer >= 0).then_some(integer as u64)
    };
    let data_view =
        context.has_builtin_brand_rooted(buffer, quench_runtime::BuiltinBrand::DataView)?;
    if !data_view
        && !context.has_builtin_brand_rooted(buffer, quench_runtime::BuiltinBrand::TypedArray)?
    {
        return Err(
            crate::modules::util_shared_vm::argument_type_error_with_expected(
                context,
                buffer,
                "buffer",
                "an instance of Buffer, TypedArray, or DataView",
            )?,
        );
    }
    let (constructor, getters, set) = context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let constructor = get(context, global, "Uint8Array")?;
        let prototype = get(context, constructor, "prototype")?;
        let typed = context.prototype_rooted(prototype)?;
        let prototype = if data_view {
            let constructor = get(context, global, "DataView")?;
            get(context, constructor, "prototype")?
        } else {
            typed
        };
        let mut getters = Vec::new();
        for name in ["buffer", "byteOffset", "byteLength"] {
            let key = context.string_rooted(name);
            let descriptor = context.own_property_descriptor_rooted(prototype, key)?;
            getters.push(get(context, descriptor, "get")?);
        }
        Ok((constructor, getters, get(context, typed, "set")?))
    })?;
    let mut parts = Vec::new();
    for getter in getters {
        parts.push(context.call_rooted(getter, buffer, &[])?);
    }
    let target = context.construct_rooted(constructor, constructor, &parts)?;
    let state = context.host_mut().shared_state().borrow().fs.clone();
    let bytes = match state.read_descriptor(fd, size, position) {
        Ok(bytes) => bytes,
        Err(error) => return throw_io_error(context, "read", None, &error),
    };
    if !bytes.is_empty() {
        let source = context.uint8_array_rooted(&bytes)?;
        let offset = context.number(offset as f64);
        context.call_rooted(set, target, &[source, offset])?;
    }
    Ok(context.number(bytes.len() as f64))
}

pub(crate) fn write_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let bytes = byte_view(context, args.get(1).copied())?;
    let position = descriptor_position(context, args.get(2).copied());
    let (result, path) = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        (state.fs.write_all_descriptor(fd, &bytes, position), path)
    };
    match result {
        Ok(written) => Ok(context.number(written as f64)),
        Err(error) => Err(super::stream_io_error(context, error, "write", &path)?),
    }
}

pub(crate) fn sync_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let data_only = args
        .get(1)
        .and_then(|value| context.rooted_value(*value))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (result, path) = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        (state.fs.sync_descriptor(fd, data_only), path)
    };
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(super::stream_io_error(
            context,
            error,
            if data_only { "fdatasync" } else { "fsync" },
            &path,
        )?),
    }
}

fn descriptor_position(
    context: &NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> Option<u64> {
    value
        .and_then(|value| context.rooted_value(value))
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as u64)
}

pub(crate) fn mkdir_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    let options = mkdir_options(context, args.get(1).copied())?;
    let requested_path = args
        .first()
        .copied()
        .map(|path| context.string_text(path))
        .transpose()?
        .flatten();
    let relative_path = requested_path
        .as_deref()
        .is_some_and(|path| !std::path::Path::new(path).is_absolute());
    let cwd = context.host_mut().shared_state().borrow().cwd.path();
    if relative_path && !cwd.is_dir() {
        return throw_operation_error(
            context,
            &ops::OperationError::Io {
                syscall: "mkdir",
                path,
                error: std::io::Error::from_raw_os_error(libc::ENOENT),
            },
        );
    }
    let first_created = match ops::mkdir(&path, options) {
        Ok(path) => path,
        Err(error) => return throw_operation_error(context, &error),
    };
    match first_created {
        Some(path) => Ok(context.string_rooted(&path)),
        None => Ok(context.undefined()),
    }
}

pub(crate) fn rmdir_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    match std::fs::remove_dir(&path) {
        Ok(()) => Ok(context.undefined()),
        Err(error) => throw_operation_error(
            context,
            &ops::OperationError::Io {
                syscall: "rmdir",
                path,
                error,
            },
        ),
    }
}

pub(crate) fn mkdtemp_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let prefix = path_argument(context, args.first().copied())?;
    for _ in 0..100 {
        let suffix = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed) & 0x00FF_FFFF;
        let path = format!("{prefix}{suffix:06x}");
        match std::fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
                }
                return Ok(context.string_rooted(&path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return throw_operation_error(
                    context,
                    &ops::OperationError::Io {
                        syscall: "mkdtemp",
                        path,
                        error,
                    },
                );
            }
        }
    }
    let path = prefix;
    throw_operation_error(
        context,
        &ops::OperationError::Io {
            syscall: "mkdtemp",
            path,
            error: std::io::Error::from(std::io::ErrorKind::AlreadyExists),
        },
    )
}

pub(crate) fn copy_file_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let source = path_argument(context, args.first().copied())?;
    let destination = path_argument(context, args.get(1).copied())?;
    let mode = args
        .get(2)
        .copied()
        .and_then(|mode| context.rooted_value(mode))
        .and_then(Value::as_number)
        .unwrap_or(0.0) as i32;
    // Validate/open the source before COPYFILE_EXCL checks the destination.
    // libuv reports ENOENT for a missing source even when the destination
    // already exists; checking the destination first incorrectly masks it as
    // EEXIST.
    let result = std::fs::metadata(&source).and_then(|_| {
        if mode & 1 != 0 && std::path::Path::new(&destination).exists() {
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        } else {
            std::fs::copy(&source, &destination).map(|_| ())
        }
    });
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => {
            let mut details = fs_error_details::error_details("copyfile", Some(&source), &error);
            details.message = format!("{} -> '{destination}'", details.message);
            let exception = context.error_rooted(&details.message)?;
            let errno = context.number(details.errno as f64);
            set(context, exception, "errno", errno)?;
            set_string(context, exception, "code", details.code)?;
            let syscall = context.string_rooted(&details.syscall);
            set(context, exception, "syscall", syscall)?;
            set_string(context, exception, "path", &source)?;
            set_string(context, exception, "dest", &destination)?;
            Err(context.throw(exception))
        }
    }
}

pub(crate) fn symlink_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args
        .first()
        .copied()
        .map(|target| context.to_string(target))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let link_path = path_argument(context, args.get(1).copied())?;
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&target, &link_path);
    #[cfg(windows)]
    let result = {
        let link_type = args
            .get(2)
            .and_then(|value| context.rooted_value(*value))
            .and_then(Value::as_string)
            .unwrap_or("file");
        if link_type == "dir" || link_type == "junction" {
            std::os::windows::fs::symlink_dir(&target, &link_path)
        } else {
            std::os::windows::fs::symlink_file(&target, &link_path)
        }
    };
    #[cfg(not(any(unix, windows)))]
    let result: std::io::Result<()> = Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => throw_io_error_with_dest(context, "symlink", &target, &link_path, &error),
    }
}

pub(crate) fn link_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let existing_path = path_argument(context, args.first().copied())?;
    let new_path = path_argument(context, args.get(1).copied())?;
    match std::fs::hard_link(&existing_path, &new_path) {
        Ok(()) => Ok(context.undefined()),
        Err(error) => throw_io_error_with_dest(context, "link", &existing_path, &new_path, &error),
    }
}

pub(crate) fn rename_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let old_path = path_argument(context, args.first().copied())?;
    let new_path = path_argument(context, args.get(1).copied())?;
    match std::fs::rename(&old_path, &new_path) {
        Ok(()) => Ok(context.undefined()),
        Err(error) => throw_io_error_with_dest(context, "rename", &old_path, &new_path, &error),
    }
}

pub(crate) fn unlink_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(context.undefined()),
        Err(error) => throw_operation_error(
            context,
            &ops::OperationError::Io {
                syscall: "unlink",
                path,
                error,
            },
        ),
    }
}

pub(crate) fn rm_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    let options = rm_options(context, args.get(1).copied())?;
    if let Err(error) = ops::rm(&path, options) {
        return throw_operation_error(context, &error);
    }
    Ok(context.undefined())
}

pub(crate) fn rm_async(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args.first().copied().unwrap_or_else(|| context.undefined());
    let mut options = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let mut callback = args.get(2).copied().unwrap_or_else(|| context.undefined());
    if context.is_callable_rooted(options)? {
        callback = options;
        options = context.undefined();
    }
    let undefined = context.undefined();
    let path = crate::modules::url_shared_vm::to_path_if_file_url(context, undefined, &[path])?;
    let path = path_argument(context, Some(path))?;
    if !context.is_callable_rooted(callback)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(
            context, callback, "cb", "function",
        )?);
    }
    let options = rm_options(context, Some(options))?;
    let data = context.object_rooted()?;
    let path = context.string_rooted(&path);
    set(context, data, "path", path)?;
    set(context, data, "callback", callback)?;
    let saved = context.object_rooted()?;
    let recursive = context.boolean(options.recursive);
    let force = context.boolean(options.force);
    set(context, saved, "recursive", recursive)?;
    set(context, saved, "force", force)?;
    set(context, data, "options", saved)?;
    let complete =
        context.host_function_with_data(crate::host::shared_vm::operation("fsRmComplete"), data)?;
    let undefined = context.undefined();
    crate::modules::shared_event_loop::queue_next_tick(context, complete, undefined, &[])?;
    Ok(undefined)
}

pub(crate) fn rm_complete(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let path = property(context, data, "path")?;
    let options = property(context, data, "options")?;
    let callback = property(context, data, "callback")?;
    let undefined = context.undefined();
    let error = match rm_sync(context, undefined, &[path, options]) {
        Ok(_) => context.null(),
        Err(error) => error
            .exception
            .ok_or_else(|| RootedError::host("rm lost its exception"))?,
    };
    context.call_rooted(callback, undefined, &[error])
}

fn path_argument(
    context: &mut NativeContext<'_, NodeHost>,
    path: Option<RootId>,
) -> Result<String, RootedError> {
    let Some(path) = path else {
        return Err(invalid_path(context, "undefined")?);
    };
    let path_text = context.string_text(path)?;
    let Some(path) = path_text else {
        let received = received_type(context, path)?;
        return Err(invalid_path(context, &received)?);
    };
    if path.contains('\0') {
        return Err(invalid_path_value(context, &path)?);
    }
    Ok(shared_vm::resolve_shared_path(context, path))
}

fn mkdir_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<ops::MkdirOptions, RootedError> {
    let Some(value) = options else {
        return Ok(ops::MkdirOptions::default());
    };
    let rooted = context.rooted_value(value).unwrap_or(Value::UNDEFINED);
    if rooted.is_undefined() || rooted.is_null() {
        return Ok(ops::MkdirOptions::default());
    }
    if let Some(mode) = rooted.as_number() {
        return Ok(ops::MkdirOptions {
            mode: Some(mode as u32),
            ..ops::MkdirOptions::default()
        });
    }
    if let Some(mode_text) = context.string_text(value)? {
        if let Ok(mode) = u32::from_str_radix(&mode_text, 8) {
            return Ok(ops::MkdirOptions {
                mode: Some(mode),
                ..ops::MkdirOptions::default()
            });
        }
        return Err(invalid_value(context, "options", &mode_text)?);
    }
    if !context.is_object_rooted(value)? {
        return Err(invalid_options_type(context, value)?);
    }

    let mode = property(context, value, "mode")?;
    let recursive = property(context, value, "recursive")?;
    let recursive_value = context.rooted_value(recursive).unwrap_or(Value::UNDEFINED);
    let recursive = match recursive_value.as_bool() {
        Some(value) => value,
        None if recursive_value.is_undefined() => false,
        None => return Err(option_type_error(context, "recursive", recursive)?),
    };
    let mode = context
        .rooted_value(mode)
        .and_then(Value::as_number)
        .map(|mode| mode as u32);
    Ok(ops::MkdirOptions { mode, recursive })
}

fn rm_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<ops::RmOptions, RootedError> {
    let Some(value) = options else {
        return Ok(ops::RmOptions::default());
    };
    let rooted = context.rooted_value(value).unwrap_or(Value::UNDEFINED);
    if rooted.is_undefined() || rooted.is_null() {
        return Ok(ops::RmOptions::default());
    }
    if !context.is_object_rooted(value)? {
        return Err(invalid_options_type(context, value)?);
    }

    let recursive = property(context, value, "recursive")?;
    let recursive_value = context.rooted_value(recursive).unwrap_or(Value::UNDEFINED);
    let recursive = match recursive_value.as_bool() {
        Some(value) => value,
        None if recursive_value.is_undefined() => false,
        None => return Err(option_type_error(context, "recursive", recursive)?),
    };
    let force = property(context, value, "force")?;
    let force = context.truthy_rooted(force)?;
    Ok(ops::RmOptions { recursive, force })
}

pub(super) fn byte_view(
    context: &mut NativeContext<'_, NodeHost>,
    data: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let Some(data) = data else {
        return Err(invalid_data(context)?);
    };
    if let Some(bytes) = context.view_bytes_rooted(data) {
        return Ok(bytes);
    }
    let length_root = property(context, data, "byteLength")?;
    let Some(length) = context
        .rooted_value(length_root)
        .and_then(Value::as_number)
        .filter(|length| length.is_finite() && *length >= 0.0 && *length <= usize::MAX as f64)
    else {
        return Err(invalid_data(context)?);
    };
    let length = length as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let byte = property(context, data, &index.to_string())?;
        let value = context
            .rooted_value(byte)
            .and_then(Value::as_number)
            .filter(|value| value.is_finite() && (0.0..=255.0).contains(value))
            .ok_or_else(|| RootedError::host("shared fs byte view contained a non-byte"))?;
        bytes.push(value as u8);
    }
    Ok(bytes)
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn invalid_data(context: &mut NativeContext<'_, NodeHost>) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(
        "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView",
    )?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn invalid_options_type(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootedError, RootedError> {
    let received = received_type(context, value)?;
    let error = context.type_error_rooted(&format!(
        "The \"options\" argument must be of type object. Received {received}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn option_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    value: RootId,
) -> Result<RootedError, RootedError> {
    let received = received_type(context, value)?;
    let error = context.type_error_rooted(&format!(
        "The \"options.{name}\" property must be of type boolean. Received {received}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn invalid_path(
    context: &mut NativeContext<'_, NodeHost>,
    received: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"path\" argument must be of type string, Buffer, or URL. Received {received}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn invalid_path_value(
    context: &mut NativeContext<'_, NodeHost>,
    path: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"path\" argument must be a string, Buffer, or URL without null bytes. Received {path:?}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_VALUE")?;
    Ok(context.throw(error))
}

fn invalid_value(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    value: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The argument '{name}' is invalid. Received {value:?}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_VALUE")?;
    Ok(context.throw(error))
}

fn received_type(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    let rooted = context.rooted_value(value).unwrap_or(Value::UNDEFINED);
    if rooted.is_undefined() {
        return Ok("undefined".to_owned());
    }
    if rooted.is_null() {
        return Ok("null".to_owned());
    }
    if let Some(boolean) = rooted.as_bool() {
        return Ok(format!("type boolean ({boolean})"));
    }
    if let Some(number) = rooted.as_number() {
        return Ok(format!("type number ({number})"));
    }
    if let Some(string) = context.string_text(value)? {
        let inspected = string.replace('\\', "\\\\").replace('\'', "\\'");
        return Ok(format!("type string ('{inspected}')"));
    }
    if context.is_symbol_rooted(value)? {
        let global = context.global_root()?;
        let string_key = context.string_rooted("String");
        let string_constructor = context.get_property_rooted(global, string_key)?;
        let description = context.call_rooted(string_constructor, global, &[value])?;
        let description = context.string_text(description)?.unwrap_or_default();
        return Ok(format!("type symbol ({description})"));
    }
    let global = context.global_root()?;
    let array_key = context.string_rooted("Array");
    let array_constructor = context.get_property_rooted(global, array_key)?;
    let is_array_key = context.string_rooted("isArray");
    let is_array = context.get_property_rooted(array_constructor, is_array_key)?;
    let is_array = context.call_rooted(is_array, array_constructor, &[value])?;
    if context
        .rooted_value(is_array)
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok("an instance of Array".to_owned());
    }
    if context.is_callable_rooted(value)? {
        let name_key = context.string_rooted("name");
        let name = context.get_property_rooted(value, name_key)?;
        let name = context.string_text(name)?.unwrap_or_default();
        return Ok(format!("function {name}"));
    }
    Ok("an instance of Object".to_owned())
}

fn throw_operation_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: &ops::OperationError,
) -> Result<RootId, RootedError> {
    let details = fs_error_details::operation_error_details(error);
    throw_error_details(context, details)
}

pub(crate) fn throw_io_error(
    context: &mut NativeContext<'_, NodeHost>,
    syscall: &str,
    path: Option<&str>,
    error: &std::io::Error,
) -> Result<RootId, RootedError> {
    throw_error_details(
        context,
        fs_error_details::error_details(syscall, path, error),
    )
}

pub(crate) fn throw_io_error_with_dest(
    context: &mut NativeContext<'_, NodeHost>,
    syscall: &str,
    path: &str,
    dest: &str,
    error: &std::io::Error,
) -> Result<RootId, RootedError> {
    let mut details = fs_error_details::error_details(syscall, Some(path), error);
    details.message = format!("{} -> '{dest}'", details.message);
    details.dest = Some(dest.to_owned());
    throw_error_details(context, details)
}

fn throw_error_details(
    context: &mut NativeContext<'_, NodeHost>,
    details: fs_error_details::FsErrorDetails,
) -> Result<RootId, RootedError> {
    let exception = context.error_rooted(&details.message)?;
    if let Some(name) = details.name {
        set_string(context, exception, "name", name)?;
    }
    let errno = context.number(details.errno as f64);
    set(context, exception, "errno", errno)?;
    set_string(context, exception, "code", details.code)?;
    let syscall = context.string_rooted(&details.syscall);
    set(context, exception, "syscall", syscall)?;
    if let Some(path) = details.path {
        let path = context.string_rooted(&path);
        set(context, exception, "path", path)?;
    }
    if let Some(dest) = details.dest {
        let dest = context.string_rooted(&dest);
        set(context, exception, "dest", dest)?;
    }
    Err(context.throw(exception))
}

fn set_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host("cannot set shared fs error property"))
    }
}
