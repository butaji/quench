use crate::host::NodeHost;
use crate::modules::fs_ops;
use quench_runtime::{NativeContext, RootId, RootedError};

// Node's internal fs readFile limit, independent of Buffer's larger capacity.
pub(super) const MAX_READ_FILE_BYTES: u64 = i32::MAX as u64;

const PERMISSION_GUARD: &str = quench_js_check::checked_js!(
    r#"(fs) => {
  const readScope = 'fs.read';
  const writeScope = 'fs.write';
  const permissionName = (scope) => scope === readScope ? 'FileSystemRead' : 'FileSystemWrite';
  function check(scope, path) {
    let resource;
    if (typeof path === 'string') resource = path;
    else if (typeof path === 'number') return;
    else if (path instanceof URL && path.protocol === 'file:') {
      // The WHATWG URL path validator used by fs rejects encoded separators
      // before any asynchronous work is queued. Preserve the original URL
      // spelling here because URL.pathname may normalize some escapes.
      const hrefPath = typeof path.href === 'string'
        ? path.href.replace(/^file:\/\/[^/]*(\/[^?#]*)?.*$/i, '$1')
        : '';
      if (/%2f/i.test(path.pathname) || /%2f/i.test(hrefPath) ||
          (process.platform === 'win32' && (/%5c/i.test(path.pathname) || /%5c/i.test(hrefPath)))) {
        throw Object.assign(new TypeError('The URL must not contain encoded / or \\ characters.'), {
          code: 'ERR_INVALID_FILE_URL_PATH', input: path,
        });
      }
      resource = process.getBuiltinModule('url').fileURLToPath(path);
    } else if (ArrayBuffer.isView(path)) {
      resource = new TextDecoder().decode(path);
    } else {
      // Let the underlying fs entry point report its canonical argument error.
      return;
    }
    if (typeof resource === 'string' && resource.includes('\0')) {
      throw Object.assign(new TypeError("The argument 'path' must be a string, Uint8Array, or URL without null bytes"), {
        code: 'ERR_INVALID_ARG_VALUE', input: resource,
      });
    }
    const permission = globalThis.process?.permission;
    const allowed = !permission || permission.has(scope, resource);
    const audit = (globalThis.process?.execArgv || []).includes('--permission-audit');
    if (allowed || audit) return;
    const error = Object.assign(new Error(`Access to this API has been restricted. Use --allow-${scope.replace('.', '-')} to manage permissions.`), {
      code: 'ERR_ACCESS_DENIED', permission: permissionName(scope), resource,
    });
    // Node's uncaught-exception formatter includes these fields for native
    // ERR_ACCESS_DENIED errors. Quench's generic Error formatter only prints
    // the message and stack, so retain the resource detail in the stack too.
    if (typeof error.stack === 'string') {
      const quotedResource = `'${resource.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/\r/g, '\\r').replace(/\n/g, '\\n')}'`;
      error.stack += `\n    permission: '${permissionName(scope)}',\n    resource: ${quotedResource}`;
    }
    throw error;
  }
  function checkMode(path, flags) {
    const writes = typeof flags === 'number'
      ? (flags & ((fs.constants?.O_WRONLY || 1) | (fs.constants?.O_RDWR || 2) |
          (fs.constants?.O_CREAT || 64) | (fs.constants?.O_TRUNC || 512) |
          (fs.constants?.O_APPEND || 1024))) !== 0
      : /[wa+]/.test(typeof flags === 'string' ? flags : 'r');
    check(writes ? writeScope : readScope, path);
  }
  function validateOpenAsBlobArgs(path, options) {
    if (typeof path !== 'string' && !(path instanceof Uint8Array) && !(path instanceof URL)) {
      throw Object.assign(new TypeError('The "path" argument must be of type string or an instance of Buffer or URL.'), {
        code: 'ERR_INVALID_ARG_TYPE',
      });
    }
    if (options !== undefined && (options === null || typeof options !== 'object')) {
      throw Object.assign(new TypeError('The "options" argument must be of type object.'), {
        code: 'ERR_INVALID_ARG_TYPE',
      });
    }
    if (options?.type !== undefined && typeof options.type !== 'string') {
      throw Object.assign(new TypeError('The "options.type" property must be of type string.'), {
        code: 'ERR_INVALID_ARG_TYPE',
      });
    }
  }
  function checkSymlinkPermission() {
    const permission = globalThis.process?.permission;
    const audit = (globalThis.process?.execArgv || []).includes('--permission-audit');
    if (!permission) return;
    const canRead = permission.has(readScope);
    const canWrite = permission.has(writeScope);
    if (canRead && canWrite) return;
    if (audit) {
      try {
        globalThis.process.getBuiltinModule('diagnostics_channel')
          .channel('node:permission-model:fs').publish({ permission: 'FileSystem' });
      } catch {}
      return;
    }
    throw Object.assign(new Error('fs.symlink API requires full fs.read and fs.write permissions.'), {
      code: 'ERR_ACCESS_DENIED',
    });
  }
  function wrapSync(name, scope, indexes = [0]) {
    const original = fs[name];
    if (typeof original !== 'function') return;
    const nativeOriginal = name === 'realpathSync'
      ? fs.__quenchInternalRealpathSyncNative
      : original.native;
    const wrap = (target) => function(...args) {
      for (const index of indexes) {
        if (args[index] === undefined) continue;
        try {
          check(scope, args[index]);
        } catch (error) {
          // fs.existsSync() is deliberately forgiving: Node returns false for
          // malformed paths such as strings containing NUL bytes.
          if (name === 'existsSync' && error?.code === 'ERR_INVALID_ARG_VALUE') return false;
          throw error;
        }
      }
      return Reflect.apply(target, this, args);
    };
    const wrapped = wrap(original);
    if (typeof nativeOriginal === 'function') wrapped.native = wrap(nativeOriginal);
    fs[name] = wrapped;
  }
  function wrapCallback(name, scope, indexes = [0]) {
    const original = fs[name];
    if (typeof original !== 'function') return;
    const nativeOriginal = name === 'realpath'
      ? fs.__quenchInternalRealpathNative
      : original.native;
    const wrap = (target) => function(...args) {
      const callback = args[args.length - 1];
      if (typeof callback !== 'function') return Reflect.apply(target, this, args);
      try { for (const index of indexes) if (args[index] !== undefined) check(scope, args[index]); }
      catch (error) {
        // Node rejects malformed paths synchronously even when a callback is
        // supplied. Permission denials remain callback errors.
        if (error && (error.code === 'ERR_INVALID_ARG_VALUE' ||
            error.code === 'ERR_INVALID_FILE_URL_PATH' ||
            error.code === 'ERR_INVALID_FILE_URL_HOST')) throw error;
        if (['mkdir', 'chmod', 'utimes', 'lutimes', 'futimes'].includes(name)) throw error;
        process.nextTick(callback, error);
        return;
      }
      return Reflect.apply(target, this, args);
    };
    const wrapped = wrap(original);
    if (typeof nativeOriginal === 'function') wrapped.native = wrap(nativeOriginal);
    fs[name] = wrapped;
  }
  for (const name of ['readFileSync', 'statSync', 'lstatSync', 'readdirSync', 'readlinkSync', 'realpathSync', 'accessSync', 'existsSync', 'opendirSync', 'statfsSync', 'openAsBlob', 'watch', 'watchFile']) wrapSync(name, readScope);
  if (typeof fs.openAsBlob !== 'function') fs.openAsBlob = function(path, options = {}) {
    validateOpenAsBlobArgs(path, options);
    check(readScope, path);
    fs.statSync(path);
    return Promise.resolve(new Blob([fs.readFileSync(path)], { type: options.type || '' }));
  };
  const __quenchOpenAsBlob = fs.openAsBlob;
  fs.openAsBlob = function(...args) {
    const path = args[0];
    validateOpenAsBlobArgs(path, args[1]);
    const stat = fs.statSync(path);
    return Promise.resolve(Reflect.apply(__quenchOpenAsBlob, this, args)).then((blob) => {
      Object.defineProperty(blob, '\0quench:file-backed:clone-unsupported', {
        value: true,
      });
      const ensureReadable = () => {
        let current;
        try { current = fs.statSync(path); } catch {}
        if (!current || Number(current.size) !== Number(stat.size) ||
            Math.trunc(Number(current.mtimeMs)) !== Math.trunc(Number(stat.mtimeMs))) {
          throw new DOMException('The file has been modified', 'NotReadableError');
        }
      };
      for (const method of ['text', 'arrayBuffer', 'bytes']) {
        const native = blob[method];
        if (typeof native !== 'function') continue;
        Object.defineProperty(blob, method, {
          configurable: true,
          value(...methodArgs) {
            try { ensureReadable(); } catch (error) { return Promise.reject(error); }
            return Reflect.apply(native, this, methodArgs);
          },
        });
      }
      if (typeof path === 'string') {
        Object.defineProperties(blob, {
          '\0quench:file-backed:path': { value: path },
          '\0quench:file-backed:size': { value: Number(stat.size) },
          '\0quench:file-backed:mtime': { value: Math.trunc(Number(stat.mtimeMs)) },
        });
        // Keep the file check on the returned Blob instance itself. Some
        // embedded Blob implementations have a distinct prototype from the
        // global Blob wrapped during bootstrap, which otherwise lets stream()
        // read the original snapshot after its backing file has changed.
        const nativeStream = blob.stream;
        if (typeof nativeStream === 'function') {
          Object.defineProperty(blob, 'stream', {
            configurable: true,
            value(...streamArgs) {
              const source = Reflect.apply(nativeStream, this, streamArgs);
              // The fallback stream may expose async iteration without the
              // WHATWG getReader surface. Guard that path too, since Node's
              // file-backed Blob contract is checked when the consumer reads.
              const iteratorKey = Symbol.asyncIterator;
              const iteratorFactory = source?.[iteratorKey];
              if (typeof iteratorFactory === 'function') {
                try {
                  Object.defineProperty(source, iteratorKey, {
                    configurable: true,
                    value() {
                      const iterator = Reflect.apply(iteratorFactory, this, []);
                      return {
                        next(...nextArgs) {
                          try { ensureReadable(); } catch (error) { return Promise.reject(error); }
                          return Reflect.apply(iterator.next, iterator, nextArgs);
                        },
                        return(...returnArgs) {
                          return typeof iterator.return === 'function'
                            ? Reflect.apply(iterator.return, iterator, returnArgs)
                            : Promise.resolve({ done: true, value: undefined });
                        },
                        [iteratorKey]() { return this; },
                      };
                    },
                  });
                } catch {}
              }
              const Stream = globalThis.ReadableStream || source?.constructor;
              if (typeof source?.getReader !== 'function' || typeof Stream !== 'function') return source;
              const reader = source.getReader();
              return new Stream({
                async pull(controller) {
                  try {
                    ensureReadable();
                    const item = await reader.read();
                    if (item.done) controller.close();
                    else controller.enqueue(item.value);
                  } catch (error) { controller.error(error); }
                },
                cancel(reason) { return reader.cancel(reason); },
              }, { highWaterMark: 0 });
            },
          });
        }
      }
      return blob;
    });
  };
  const __quenchOpenAsBlobSync = fs.openAsBlobSync;
  fs.openAsBlobSync = function(path, options) {
    if (options === undefined) options = {};
    validateOpenAsBlobArgs(path, options);
    check(readScope, path);
    const stat = fs.statSync(path);
    const blob = typeof __quenchOpenAsBlobSync === 'function'
      ? Reflect.apply(__quenchOpenAsBlobSync, this, [path, options])
      : new Blob([fs.readFileSync(path)], { type: options.type || '' });
    const ensureReadable = () => {
      let current;
      try { current = fs.statSync(path); } catch {}
      if (!current || Number(current.size) !== Number(stat.size) ||
          Math.trunc(Number(current.mtimeMs)) !== Math.trunc(Number(stat.mtimeMs))) {
        throw new DOMException('The file has been modified', 'NotReadableError');
      }
    };
    for (const method of ['text', 'arrayBuffer', 'bytes']) {
      const native = blob[method];
      if (typeof native !== 'function') continue;
      Object.defineProperty(blob, method, {
        configurable: true,
        value(...args) {
          try { ensureReadable(); } catch (error) { return Promise.reject(error); }
          return Reflect.apply(native, this, args);
        },
      });
    }
    return blob;
  };
  for (const name of ['writeFileSync', 'appendFileSync', 'mkdirSync', 'mkdtempDisposableSync', 'rmdirSync', 'rmSync', 'unlinkSync', 'truncateSync', 'chmodSync', 'lchmodSync', 'chownSync', 'lchownSync', 'utimesSync', 'lutimesSync']) wrapSync(name, writeScope);
  if (typeof fs.mkdtempSync === 'function') {
    const original = fs.mkdtempSync;
    fs.mkdtempSync = function(prefix, ...args) {
      check(writeScope, `${prefix}XXXXXX`);
      return Reflect.apply(original, this, [prefix, ...args]);
    };
  }
  function permissionDisabledFdApiError(syscall) {
    const execArgs = globalThis.process?.execArgv || [];
    if (!execArgs.includes('--permission') || execArgs.includes('--permission-audit')) return null;
    return Object.assign(new Error(`${syscall} API is disabled when Permission Model is enabled.`), {
      code: 'ERR_ACCESS_DENIED',
    });
  }
  for (const [name, syscall] of [
    ['fsyncSync', 'fsync'], ['fdatasyncSync', 'fdatasync'],
    ['fchmodSync', 'fchmod'], ['fchownSync', 'fchown'], ['futimesSync', 'futimes'],
  ]) {
    const original = fs[name];
    if (typeof original === 'function') fs[name] = function(...args) {
      const error = permissionDisabledFdApiError(syscall);
      if (error) throw error;
      return Reflect.apply(original, this, args);
    };
  }
  for (const [name, syscall] of [
    ['fchmod', 'fchmod'], ['fchown', 'fchown'], ['futimes', 'futimes'],
  ]) {
    const original = fs[name];
    if (typeof original === 'function') fs[name] = function(...args) {
      const callback = args[args.length - 1];
      if (typeof callback !== 'function') return Reflect.apply(original, this, args);
      const error = permissionDisabledFdApiError(syscall);
      if (error) { process.nextTick(callback, error); return; }
      return Reflect.apply(original, this, args);
    };
  }
  if (typeof fs.linkSync === 'function') {
    const original = fs.linkSync;
    fs.linkSync = function(from, to, ...rest) {
      // The source is read and the destination is created, so enforce the
      // corresponding permissions in Node's source-then-destination order.
      check(readScope, from);
      check(writeScope, to);
      return Reflect.apply(original, this, [from, to, ...rest]);
    };
  }
  if (typeof fs.symlinkSync === 'function') {
    const original = fs.symlinkSync;
    const validateSymlinkPath = (value, name) => {
      let path = value;
      if (Buffer.isBuffer(value)) path = value.toString();
      else if (value instanceof URL) {
        try { path = decodeURIComponent(value.pathname); }
        catch { path = value.pathname; }
      }
      if (typeof path === 'string' && path.includes('\0')) {
        const error = new TypeError(`The "${name}" argument must be a string, Uint8Array, or URL without null bytes`);
        error.code = 'ERR_INVALID_ARG_VALUE';
        throw error;
      }
    };
    fs.symlinkSync = function(target, path, ...rest) {
      validateSymlinkPath(target, 'target');
      validateSymlinkPath(path, 'path');
      checkSymlinkPermission();
      return Reflect.apply(original, this, [target, path, ...rest]);
    };
  }
  if (typeof fs.symlink === 'function') {
    const original = fs.symlink;
    fs.symlink = function(...args) {
      const callback = args[args.length - 1];
      try { checkSymlinkPermission(); }
      catch (error) {
        if (typeof callback === 'function') { process.nextTick(callback, error); return; }
        throw error;
      }
      return Reflect.apply(original, this, args);
    };
  }
  if (fs.promises && typeof fs.promises.symlink === 'function') {
    const original = fs.promises.symlink;
    fs.promises.symlink = function(...args) {
      try { checkSymlinkPermission(); }
      catch (error) { return Promise.reject(error); }
      return Reflect.apply(original, this, args);
    };
  }
  if (typeof fs.renameSync === 'function') {
    const original = fs.renameSync;
    fs.renameSync = function(from, to, ...rest) { check(readScope, from); check(writeScope, from); check(writeScope, to); return Reflect.apply(original, this, [from, to, ...rest]); };
  }
  for (const name of ['copyFileSync', 'cpSync']) {
    const original = fs[name];
    if (typeof original === 'function') fs[name] = function(from, to, ...rest) {
      check(readScope, from); check(writeScope, to);
      return Reflect.apply(original, this, [from, to, ...rest]);
    };
  }
  for (const name of ['readFile', 'stat', 'lstat', 'readdir', 'readlink', 'realpath', 'access', 'opendir', 'statfs']) wrapCallback(name, readScope);
  if (typeof fs.realpath === 'function' && typeof fs.realpath.native !== 'function') fs.realpath.native = fs.realpath;
  if (typeof fs.realpathSync === 'function' && typeof fs.realpathSync.native !== 'function') fs.realpathSync.native = fs.realpathSync;
  delete fs.__quenchInternalRealpathSyncNative;
  delete fs.__quenchInternalRealpathNative;
  if (typeof fs.exists === 'function') {
    const exists = fs.exists;
    const wrappedExists = function(path, callback) {
      if (typeof callback !== 'function') return Reflect.apply(exists, this, [path, callback]);
      if (globalThis.process?.permission?.has(readScope, path) === false) { process.nextTick(callback, false); return; }
      return Reflect.apply(exists, this, [path, callback]);
    };
    // Keep util.promisify's special value-first contract while ensuring the
    // permission check still runs through the wrapped callback API.
    wrappedExists[Symbol.for('nodejs.util.promisify.custom')] = (path) =>
      new Promise((resolve) => wrappedExists(path, resolve));
    fs.exists = wrappedExists;
  }
  for (const name of ['writeFile', 'appendFile', 'mkdir', 'rmdir', 'rm', 'unlink', 'truncate', 'chmod', 'lchmod', 'chown', 'lchown', 'utimes', 'lutimes']) wrapCallback(name, writeScope);
  if (typeof fs.mkdtemp === 'function') {
    const original = fs.mkdtemp;
    fs.mkdtemp = function(prefix, ...args) {
      try { check(writeScope, `${prefix}XXXXXX`); }
      catch (error) {
        const callback = args[args.length - 1];
        if (typeof callback === 'function') { process.nextTick(callback, error); return; }
        throw error;
      }
      return Reflect.apply(original, this, [prefix, ...args]);
    };
  }
  if (typeof fs.link === 'function') {
    const original = fs.link;
    fs.link = function(from, to, ...rest) {
      const callback = rest[rest.length - 1];
      if (typeof callback !== 'function') return Reflect.apply(original, this, [from, to, ...rest]);
      try { check(readScope, from); check(writeScope, to); }
      catch (error) {
        // Invalid paths are rejected synchronously by Node before an async
        // callback can be scheduled. Permission failures keep callback style.
        if (error?.code === 'ERR_INVALID_ARG_VALUE') throw error;
        process.nextTick(callback, error);
        return;
      }
      return Reflect.apply(original, this, [from, to, ...rest]);
    };
  }
  if (typeof fs.rename === 'function') {
    const original = fs.rename;
    fs.rename = function(from, to, callback) {
      if (typeof callback !== 'function') return Reflect.apply(original, this, [from, to, callback]);
      try { check(readScope, from); check(writeScope, from); check(writeScope, to); }
      catch (error) {
        if (error?.code === 'ERR_INVALID_ARG_VALUE' ||
            error?.code === 'ERR_INVALID_FILE_URL_PATH' ||
            error?.code === 'ERR_INVALID_FILE_URL_HOST') throw error;
        process.nextTick(callback, error);
        return;
      }
      return Reflect.apply(original, this, [from, to, callback]);
    };
  }
  for (const name of ['copyFile', 'cp']) {
    const original = fs[name];
    if (typeof original === 'function') fs[name] = function(from, to, ...rest) {
      const callback = rest[rest.length - 1];
      if (typeof callback !== 'function') return Reflect.apply(original, this, [from, to, ...rest]);
      try { check(readScope, from); check(writeScope, to); }
      catch (error) {
        if (error?.code === 'ERR_INVALID_ARG_VALUE' ||
            error?.code === 'ERR_INVALID_FILE_URL_PATH' ||
            error?.code === 'ERR_INVALID_FILE_URL_HOST') throw error;
        process.nextTick(callback, error);
        return;
      }
      return Reflect.apply(original, this, [from, to, ...rest]);
    };
  }
  const openSync = fs.openSync;
  if (typeof openSync === 'function') fs.openSync = function(path, flags, ...rest) { checkMode(path, flags); return Reflect.apply(openSync, this, [path, flags, ...rest]); };
  const open = fs.open;
  if (typeof open === 'function') fs.open = function(path, flags, ...rest) {
    const callback = rest[rest.length - 1];
    if (typeof callback !== 'function') return Reflect.apply(open, this, [path, flags, ...rest]);
    try { checkMode(path, flags); }
    catch (error) {
      if (error && (error.code === 'ERR_INVALID_ARG_VALUE' ||
          error.code === 'ERR_INVALID_FILE_URL_PATH' ||
          error.code === 'ERR_INVALID_FILE_URL_HOST')) throw error;
      process.nextTick(callback, error);
      return;
    }
    return Reflect.apply(open, this, [path, flags, ...rest]);
  };
  for (const [name, scope] of [['createReadStream', readScope], ['createWriteStream', writeScope]]) {
    const original = fs[name];
    if (typeof original === 'function') fs[name] = function(path, ...rest) {
      const options = rest.find((value) => value && typeof value === 'object' && !ArrayBuffer.isView(value));
      try { if (!(path === null && options?.fd !== undefined)) check(scope, path); }
      catch (error) {
        const streams = process.getBuiltinModule('stream');
        const stream = name === 'createReadStream'
          ? new streams.Readable({ read() {} })
          : new streams.Writable({ write(_chunk, _encoding, callback) { callback(); } });
        stream.path = path;
        process.nextTick(() => stream.destroy(error));
        return stream;
      }
      return Reflect.apply(original, this, [path, ...rest]);
    };
  }
  const promises = fs.promises;
  if (promises) {
    for (const name of ['readFile', 'stat', 'lstat', 'readdir', 'readlink', 'realpath', 'access', 'opendir', 'statfs']) {
      const original = promises[name];
      if (typeof original === 'function') promises[name] = async function(path, ...rest) { check(readScope, path); return Reflect.apply(original, this, [path, ...rest]); };
    }
    for (const name of ['writeFile', 'appendFile', 'mkdir', 'rmdir', 'rm', 'unlink', 'truncate', 'chmod', 'lchmod', 'chown', 'lchown', 'utimes', 'lutimes', 'mkdtemp']) {
      const original = promises[name];
      if (typeof original === 'function') promises[name] = async function(path, ...rest) { check(writeScope, path); return Reflect.apply(original, this, [path, ...rest]); };
    }
    if (typeof promises.mkdtempDisposable === 'function') {
      const original = promises.mkdtempDisposable;
      promises.mkdtempDisposable = function(prefix, ...rest) {
        try { check(writeScope, `${prefix}XXXXXX`); }
        catch (error) { return Promise.reject(error); }
        return Reflect.apply(original, this, [prefix, ...rest]);
      };
    }
    for (const name of ['link']) {
      const original = promises[name];
      if (typeof original === 'function') promises[name] = async function(from, to, ...rest) { check(readScope, from); check(writeScope, to); return Reflect.apply(original, this, [from, to, ...rest]); };
    }
    if (typeof promises.rename === 'function') {
      const original = promises.rename;
      promises.rename = async function(from, to, ...rest) { check(readScope, from); check(writeScope, from); check(writeScope, to); return Reflect.apply(original, this, [from, to, ...rest]); };
    }
    for (const name of ['copyFile', 'cp']) {
      const original = promises[name];
      if (typeof original === 'function') promises[name] = async function(from, to, ...rest) { check(readScope, from); check(writeScope, to); return Reflect.apply(original, this, [from, to, ...rest]); };
    }
    const promiseOpen = promises.open;
    if (typeof promiseOpen === 'function') promises.open = async function(path, flags, ...rest) {
      checkMode(path, flags);
      const handle = await Reflect.apply(promiseOpen, this, [path, flags, ...rest]);
      for (const [name, scope] of [
        ['read', readScope], ['readv', readScope], ['readFile', readScope],
        ['createReadStream', readScope], ['stat', readScope],
        ['readLines', readScope],
        ['write', writeScope], ['writev', writeScope], ['writeFile', writeScope],
        ['appendFile', writeScope], ['chmod', writeScope], ['utimes', writeScope],
        ['truncate', writeScope],
      ]) {
        const original = handle[name];
        if (typeof original === 'function') handle[name] = function(...args) {
          // FileHandle methods expose promise APIs. Permission failures must
          // reject that promise, rather than throw before the method returns,
          // so callers can handle them with `assert.rejects()` / `.catch()`.
          try { check(scope, path); }
          catch (error) { return Promise.reject(error); }
          return Reflect.apply(original, this, args);
        };
      }
      return handle;
    };
  }
  return fs;
}"#
);

use fs_ops::ReadFileFailure;

#[path = "fs_shared_vm/file_handle_read.rs"]
pub(crate) mod file_handle_read;
#[path = "fs_shared_vm/stat.rs"]
pub(crate) mod stat;
#[path = "fs_shared_vm/sync.rs"]
pub(crate) mod sync;
#[path = "fs_shared_vm/write_stream.rs"]
pub(crate) mod write_stream;

#[path = "fs_shared_vm/write_public.rs"]
pub(crate) mod write_public;

pub(crate) mod callback_public;
#[path = "fs_shared_vm/file_handle_write.rs"]
pub(crate) mod file_handle_write;
#[path = "fs_shared_vm/metadata_public.rs"]
pub(crate) mod metadata_public;
pub(crate) mod promise_public;

const PROMISES_FACTORY: &str = quench_js_check::checked_js!(
    r#"(readFile, stat, lstat, readdir, readlink, realpath, openSync, closeSync, readSync, writeSync, fstatSync, fchmodSync, fsyncSync, fdatasyncSync, writeFileSync, appendFileSync, readvSync, writevSync, createReadStream, normalizeRead, apply, registerHandle, closeHandle) => {
  const abortError = () => {
    const error = new Error('The operation was aborted');
    error.name = 'AbortError';
    error.code = 'ABORT_ERR';
    return error;
  };
  const throwIfAborted = (options) => {
    if (options?.signal?.aborted) throw abortError();
  };
  const collectWriteData = async (data, options) => {
    if (typeof data === 'string' || ArrayBuffer.isView(data)) return data;
    if (data == null ||
        (typeof data[Symbol.asyncIterator] !== 'function' &&
         typeof data[Symbol.iterator] !== 'function')) {
      const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, DataView, or Iterable.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const encoding = typeof options === 'string' ? options : (options?.encoding || 'utf8');
    const chunks = [];
    for await (const chunk of data) {
      throwIfAborted(options);
      if (typeof chunk === 'string') chunks.push(Buffer.from(chunk, encoding));
      else if (ArrayBuffer.isView(chunk)) {
        chunks.push(Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength));
      } else {
        const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView.');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    }
    return Buffer.concat(chunks);
  };
  const snapshotWriteView = (data) => {
    if (!ArrayBuffer.isView(data)) return data;
    return Buffer.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
  };
  return ({
  readFile: async (...args) => {
    const handle = args[0];
    if (handle && typeof handle.readFile === 'function') return handle.readFile(args[1]);
    await new Promise((resolve) => setImmediate(resolve));
    throwIfAborted(args[1]);
    return readFile(...args);
  },
  writeFile: async (...args) => {
    // Preserve the caller's bytes before the first async boundary. The
    // callback facade does the same for ArrayBufferViews.
    args[1] = snapshotWriteView(args[1]);
    await new Promise((resolve) => setImmediate(resolve));
    throwIfAborted(args[2]);
    args[1] = await collectWriteData(args[1], args[2]);
    throwIfAborted(args[2]);
    return writeFileSync(...args);
  },
  appendFile: async (...args) => {
    args[1] = snapshotWriteView(args[1]);
    await new Promise((resolve) => setImmediate(resolve));
    throwIfAborted(args[2]);
    args[1] = await collectWriteData(args[1], args[2]);
    throwIfAborted(args[2]);
    return appendFileSync(...args);
  },
  stat: (...args) => Promise.resolve().then(() => stat(...args)),
  lstat: (...args) => Promise.resolve().then(() => lstat(...args)),
  readdir: (...args) => {
    // Snapshot Node's relevant option fields before the promise boundary;
    // callers may mutate the original options object immediately afterward.
    const options = args[1];
    if (options !== null && typeof options === 'object') {
      args[1] = {
        encoding: options.encoding,
        withFileTypes: options.withFileTypes,
        recursive: options.recursive,
      };
    }
    return new Promise((resolve, reject) => {
      readdir(...args, (error, entries) => {
        if (error) reject(error);
        else resolve(entries);
      });
    });
  },
  readlink: (...args) => Promise.resolve().then(() => readlink(...args)),
  realpath: (...args) => Promise.resolve().then(() => realpath(...args)),
  open: (...args) => Promise.resolve().then(() => {
    if (Buffer.isBuffer(args[0])) args[0] = args[0].toString();
    else if (args[0] instanceof URL) args[0] = args[0].pathname;
    if (typeof args[2] === "string") {
      args[2] = Number.parseInt(args[2], 8);
      if (Number.isNaN(args[2])) {
        const error = new TypeError('The "mode" argument must be a valid integer');
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
    } else if (args[2] != null && typeof args[2] !== "number") {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = typeof args[2] === "string" ? "ERR_INVALID_ARG_VALUE" : "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    // VFS replaces these functions after the promises module is initialized.
    // Keep FileHandles routed through the live fs object.
    const fs = globalThis.__nodeFs;
    const fd = fs.openSync(...args);
    let closePromise;
    let closed = false;
    let readableWebStreamController;
    let readableWebStreamActive = false;
    let writerActive = false;
    const ensureOpen = (syscall) => {
      if (closed) {
        throw Object.assign(new Error(`EBADF: bad file descriptor, ${syscall}`), {
          code: "EBADF",
          syscall,
        });
      }
    };
    const listeners = new Map();
    const addListener = (event, listener, once) => {
      if (typeof listener !== "function") throw new TypeError('The "listener" argument must be of type function.');
      const records = listeners.get(event) || [];
      records.push({ listener, once });
      listeners.set(event, records);
    };
    const handle = {
      fd,
      on(event, listener) { addListener(event, listener, false); return this; },
      addListener(event, listener) { addListener(event, listener, false); return this; },
      once(event, listener) { addListener(event, listener, true); return this; },
      removeListener(event, listener) {
        const records = listeners.get(event) || [];
        const index = records.findIndex((record) => record.listener === listener);
        if (index !== -1) records.splice(index, 1);
        if (records.length) listeners.set(event, records); else listeners.delete(event);
        return this;
      },
      off(event, listener) { return this.removeListener(event, listener); },
      emit(event, ...args) {
        const records = listeners.get(event) || [];
        for (const record of [...records]) {
          if (record.once) this.removeListener(event, record.listener);
          Reflect.apply(record.listener, this, args);
        }
        return records.length !== 0;
      },
      close: () => {
        closePromise ||= Promise.resolve().then(() => {
          fs.closeSync(fd);
          closed = true;
          closeHandle(handle);
          handle.fd = -1;
          try { readableWebStreamController?.close(); } catch {}
          handle.emit("close");
        });
        return closePromise;
      },
      readableWebStream(options) {
        if (closed || closePromise || readableWebStreamActive) {
          const error = new Error('The FileHandle is already in use');
          error.code = 'ERR_INVALID_STATE';
          throw error;
        }
        if (options !== undefined && (options === null || typeof options !== 'object')) {
          const error = new TypeError('The "options" argument must be of type object.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        readableWebStreamActive = true;
        const autoClose = options?.autoClose === true;
        if (options?.type !== undefined && options.type !== 'bytes') {
          process.emitWarning(
            'A non-"bytes" options.type has no effect. A byte-oriented steam is always created.',
            { type: 'ExperimentalWarning' },
          );
        }
        return new ReadableStream({
          type: 'bytes',
          // Capture the controller during stream setup, before the first pull.
          // FileHandle.close() can race ahead of reader acquisition (or before
          // a pull is requested), and must still close reader.closed.
          start(controller) {
            readableWebStreamController = controller;
          },
          pull(controller) {
            readableWebStreamController = controller;
            if (closed) {
              controller.close();
              return;
            }
            const request = controller.byobRequest;
            const view = request?.view || new Uint8Array(64 * 1024);
            const bytesRead = fs.readSync(fd, view, 0, view.byteLength, null);
            if (bytesRead === 0) {
              controller.close();
              if (request) request.respond(0);
              if (autoClose) handle.close();
              return;
            }
            if (request) request.respond(bytesRead);
            else controller.enqueue(view.subarray(0, bytesRead));
          },
          cancel() {
            if (autoClose) return handle.close();
          },
        });
      },
      read: (...readArgs) => Promise.resolve().then(() => {
        ensureOpen("read");
        const { buffer, offset, length, position } = apply(normalizeRead, undefined, readArgs);
        const bytesRead = fs.readSync(fd, buffer, offset, length, position);
        return { __proto__: null, bytesRead, buffer };
      }),
      readFile: async (options) => {
        ensureOpen("read");
        const settings = typeof options === 'string' ? { encoding: options } : (options || {});
        const signal = settings.signal;
        const checkAbort = () => {
          if (!signal?.aborted) return;
          const error = new Error('The operation was aborted');
          error.name = 'AbortError';
          error.code = 'ABORT_ERR';
          throw error;
        };
        checkAbort();
        await new Promise((resolve) => setImmediate(resolve));
        checkAbort();
        const size = fs.fstatSync(fd).size;
        if (size > 0x7FFFFFFF) {
          const error = new RangeError('File size is greater than 2 GiB');
          error.code = 'ERR_FS_FILE_TOO_LARGE';
          throw error;
        }
        const chunks = [];
        let total = 0;
        while (true) {
          checkAbort();
          const remaining = 0x7FFFFFFF - total;
          const buffer = Buffer.alloc(Math.min(64 * 1024, remaining || 1));
          const bytesRead = fs.readSync(fd, buffer, 0, buffer.length, null);
          if (bytesRead === 0) break;
          if (bytesRead > remaining) {
            const error = new RangeError('File size is greater than 2 GiB');
            error.code = 'ERR_FS_FILE_TOO_LARGE';
            throw error;
          }
          chunks.push(buffer.subarray(0, bytesRead));
          total += bytesRead;
        }
        const result = Buffer.concat(chunks, total);
        return settings.encoding ? result.toString(settings.encoding) : result;
      },
      pull: (...args) => {
        if (closed || closePromise || writerActive || readableWebStreamActive) {
          throw Object.assign(new Error('The FileHandle is already closed or locked'), { code: 'ERR_INVALID_STATE' });
        }
        ensureOpen("read");
        const last = args[args.length - 1];
        const lastIsTransform = typeof last === "function" ||
          (last !== null && typeof last === "object" && typeof last.transform === "function");
        const options = args.length > 0 && !lastIsTransform ? (args.pop() ?? {}) : {};
        if (options === null || typeof options !== "object") {
          const error = new TypeError('The "options" argument must be of type object.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const autoClose = options.autoClose ?? false;
        if (typeof autoClose !== "boolean") {
          const error = new TypeError('The "options.autoClose" property must be of type boolean.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const readSize = options.chunkSize ?? 131072;
        if (typeof readSize !== "number") {
          const error = new TypeError('The "options.chunkSize" property must be of type number.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isSafeInteger(readSize) || readSize < 1) {
          const error = new RangeError('The value of "options.chunkSize" is out of range.');
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        let position = options.start ?? null;
        let remaining = options.limit ?? -1;
        if (position !== null && typeof position !== "number") {
          const error = new TypeError('The "options.start" property must be of type number.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (remaining !== -1 && typeof remaining !== "number") {
          const error = new TypeError('The "options.limit" property must be of type number.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (position !== null && (!Number.isSafeInteger(position) || position < 0)) {
          const error = new RangeError('The value of "options.start" is out of range.');
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        if (remaining !== -1 && (!Number.isSafeInteger(remaining) || remaining < 1)) {
          const error = new RangeError('The value of "options.limit" is out of range.');
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        const signal = options.signal;
        if (signal !== undefined && !(signal instanceof AbortSignal)) {
          const error = new TypeError('The "options.signal" property must be an instance of AbortSignal.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const source = {
          async *[Symbol.asyncIterator]() {
            ensureOpen("read");
            if (writerActive || readableWebStreamActive) {
              throw Object.assign(new Error('The FileHandle is already locked'), { code: 'ERR_INVALID_STATE' });
            }
            writerActive = true;
            try {
              while (remaining !== 0) {
                if (signal?.aborted) throw signal.reason;
                const toRead = remaining > 0 ? Math.min(readSize, remaining) : readSize;
                const buffer = Buffer.allocUnsafe(toRead);
                const result = await handle.read(buffer, 0, toRead, position);
                if (result.bytesRead === 0) break;
                if (position !== null) position += result.bytesRead;
                if (remaining > 0) remaining -= result.bytesRead;
                yield [result.bytesRead < toRead ? buffer.subarray(0, result.bytesRead) : buffer];
              }
            } finally {
              writerActive = false;
              if (autoClose) await handle.close();
            }
          },
        };
        if (args.length > 0) {
          const iter = process.getBuiltinModule("stream/iter");
          return iter.pull(source, ...args);
        }
        return source;
      },
      pullSync: (...args) => {
        if (closed || closePromise || writerActive || readableWebStreamActive) {
          throw Object.assign(new Error('The FileHandle is already closed or locked'), { code: 'ERR_INVALID_STATE' });
        }
        ensureOpen("read");
        const last = args[args.length - 1];
        const lastIsTransform = typeof last === "function" ||
          (last !== null && typeof last === "object" && typeof last.transform === "function");
        const options = args.length > 0 && !lastIsTransform ? (args.pop() ?? {}) : {};
        if (options === null || typeof options !== "object") {
          const error = new TypeError('The "options" argument must be of type object.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const autoClose = options.autoClose ?? false;
        const readSize = options.chunkSize ?? 131072;
        let position = options.start ?? null;
        let remaining = options.limit ?? -1;
        for (const [name, value] of [["options.chunkSize", readSize], ["options.start", position], ["options.limit", remaining]]) {
          if (value !== null && typeof value !== "number") {
            const error = new TypeError(`The "${name}" property must be of type number.`);
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
        }
        if (typeof autoClose !== "boolean") {
          const error = new TypeError('The "options.autoClose" property must be of type boolean.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const signal = options.signal;
        if (signal !== undefined && !(signal instanceof AbortSignal)) {
          const error = new TypeError('The "options.signal" property must be an instance of AbortSignal.');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isSafeInteger(readSize) || readSize < 1) {
          const error = new RangeError('The value of "options.chunkSize" is out of range.');
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        if (position !== null && (!Number.isSafeInteger(position) || position < 0)) {
          const error = new RangeError('The value of "options.start" is out of range.');
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        if (remaining !== -1 && (!Number.isSafeInteger(remaining) || remaining < 1)) {
          const error = new RangeError('The value of "options.limit" is out of range.');
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        const source = {
          *[Symbol.iterator]() {
            ensureOpen("read");
            if (writerActive || readableWebStreamActive) {
              throw Object.assign(new Error('The FileHandle is already locked'), { code: 'ERR_INVALID_STATE' });
            }
            writerActive = true;
            let started = false;
            try {
              while (remaining !== 0) {
                const toRead = remaining > 0 ? Math.min(readSize, remaining) : readSize;
                const buffer = Buffer.allocUnsafe(toRead);
                const bytesRead = fs.readSync(fd, buffer, 0, toRead, position);
                started = true;
                if (bytesRead === 0) break;
                if (position !== null) position += bytesRead;
                if (remaining > 0) remaining -= bytesRead;
                yield [bytesRead < toRead ? buffer.subarray(0, bytesRead) : buffer];
              }
            } finally {
              writerActive = false;
              if (started && autoClose) {
                fs.closeSync(fd);
                closed = true;
                closePromise = Promise.resolve();
                handle.fd = -1;
                closeHandle(handle);
                handle.emit("close");
              }
            }
          },
        };
        if (args.length > 0) {
          const iter = process.getBuiltinModule("stream/iter");
          return iter.pullSync(source, ...args);
        }
        return source;
      },
      createReadStream: (options) => {
        ensureOpen("read");
        const settings = typeof options === "string" ? { encoding: options } : { ...(options || {}) };
        settings.fd = fd;
        settings.autoClose = false;
        const stream = fs.createReadStream(null, settings);
        handle.once("close", () => {
          if (!stream.destroyed) stream.destroy();
        });
        return stream;
      },
      createWriteStream: (options) => {
        ensureOpen("write");
        const settings = typeof options === "string" ? { encoding: options } : { ...(options || {}) };
        settings.fd = fd;
        settings.autoClose = false;
        return fs.createWriteStream(null, settings);
      },
      writer: (options = {}) => {
        ensureOpen("write");
        if (writerActive || readableWebStreamActive) {
          throw Object.assign(new Error('The FileHandle is already locked'), { code: 'ERR_INVALID_STATE' });
        }
        if (options === null || typeof options !== 'object') {
          const error = new TypeError('The "options" argument must be of type object.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        const chunkSize = options.chunkSize ?? 16 * 1024;
        if (typeof chunkSize !== 'number') {
          const error = new TypeError('The "options.chunkSize" property must be of type number.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        if (!Number.isSafeInteger(chunkSize) || chunkSize < 1) {
          const error = new RangeError('The value of "options.chunkSize" is out of range.');
          error.code = 'ERR_OUT_OF_RANGE';
          throw error;
        }
        const start = options.start;
        if (start !== undefined && (typeof start !== 'number' || !Number.isSafeInteger(start) || start < 0)) {
          const error = new RangeError('The value of "start" is out of range. It must be a non-negative integer.');
          error.code = 'ERR_OUT_OF_RANGE';
          throw error;
        }
        writerActive = true;
        let position = start === undefined ? null : start;
        let bytesWritten = 0;
        let writerClosed = false;
        let queuedWrites = Promise.resolve();
        let queuedWriteCount = 0;
        const checkWriter = (writeOptions) => {
          if (writerClosed) throw new TypeError('The writer is closed');
          ensureOpen('write');
          const signal = writeOptions?.signal;
          if (signal?.aborted) {
            if (signal.reason !== undefined) throw signal.reason;
            const error = new Error('The operation was aborted');
            error.name = 'AbortError';
            error.code = 'ABORT_ERR';
            throw error;
          }
        };
        const writeChunk = async (chunk, writeOptions) => {
          checkWriter(writeOptions);
          const buffer = typeof chunk === 'string'
            ? Buffer.from(chunk, writeOptions?.encoding || options.encoding || 'utf8')
            : ArrayBuffer.isView(chunk)
              ? Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength)
              : chunk instanceof ArrayBuffer
                ? Buffer.from(chunk)
                : null;
          if (buffer === null) {
            const error = new TypeError('The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView.');
            error.code = 'ERR_INVALID_ARG_TYPE';
            throw error;
          }
          let offset = 0;
          while (offset < buffer.byteLength) {
            checkWriter(writeOptions);
            const result = await handle.write(buffer, offset, buffer.byteLength - offset, position);
            if (result.bytesWritten === 0) break;
            offset += result.bytesWritten;
            bytesWritten += result.bytesWritten;
            if (position !== null) position += result.bytesWritten;
          }
          return bytesWritten;
        };
        const queueWrite = (chunk, writeOptions) => {
          queuedWriteCount++;
          const task = queuedWrites.then(() => writeChunk(chunk, writeOptions));
          queuedWrites = task.finally(() => { queuedWriteCount--; });
          return task;
        };
        const writeSync = (chunk, writeOptions) => {
          checkWriter(writeOptions);
          const buffer = typeof chunk === 'string'
            ? Buffer.from(chunk, writeOptions?.encoding || options.encoding || 'utf8')
            : ArrayBuffer.isView(chunk)
              ? Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength)
              : chunk instanceof ArrayBuffer
                ? Buffer.from(chunk)
                : null;
          if (buffer === null) {
            const error = new TypeError('The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView.');
            error.code = 'ERR_INVALID_ARG_TYPE';
            throw error;
          }
          if (buffer.byteLength > chunkSize || queuedWriteCount !== 0) {
            queueWrite(buffer, writeOptions);
            return false;
          }
          let offset = 0;
          while (offset < buffer.byteLength) {
            const written = fs.writeSync(fd, buffer, offset, buffer.byteLength - offset, position);
            if (written === 0) break;
            offset += written;
            bytesWritten += written;
            if (position !== null) position += written;
          }
          return offset === buffer.byteLength;
        };
        const finish = async (writeOptions) => {
          checkWriter(writeOptions);
          await queuedWrites;
          writerClosed = true;
          writerActive = false;
          if (options.autoClose) await handle.close();
          return bytesWritten;
        };
        const fail = async (error) => {
          if (writerClosed) return;
          writerClosed = true;
          writerActive = false;
          if (options.autoClose) await handle.close();
          return error;
        };
        return {
          write: queueWrite,
          writeSync,
          async writev(chunks, writeOptions) {
            if (!Array.isArray(chunks)) {
              const error = new TypeError('The "chunks" argument must be an array.');
              error.code = 'ERR_INVALID_ARG_TYPE';
              throw error;
            }
            for (const chunk of chunks) await writeChunk(chunk, writeOptions);
            return bytesWritten;
          },
          end: finish,
          fail,
          [Symbol.asyncDispose]: () => writerClosed ? undefined : finish(),
          [Symbol.dispose]: () => {
            if (writerClosed) return;
            writerClosed = true;
            writerActive = false;
            if (options.autoClose) void handle.close();
          },
        };
      },
      readLines: (options) => {
        ensureOpen("read");
        const settings = typeof options === "string" ? { encoding: options } : { ...(options || {}) };
        settings.fd = fd;
        settings.autoClose = false;
        const input = fs.createReadStream(null, settings);
        return process.getBuiltinModule('readline').createInterface({ input, crlfDelay: Infinity });
      },
      write: (...writeArgs) => Promise.resolve().then(() => {
        ensureOpen("write");
        let [data, offset = 0, length, position = null] = writeArgs;
        let buffer = data;
        if (typeof data === 'string') {
          position = offset ?? null;
          buffer = Buffer.from(data, typeof length === 'string' ? length : 'utf8');
          offset = 0;
          length = buffer.length;
        } else if (offset && typeof offset === 'object') {
          const options = offset;
          offset = options.offset ?? 0;
          length = options.length;
          position = options.position ?? null;
        }
        if (!ArrayBuffer.isView(buffer)) {
          const error = new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        length ??= buffer.byteLength - offset;
        const bytesWritten = fs.writeSync(fd, buffer, offset, length, position);
        return { bytesWritten, buffer: data };
      }),
      writeFile: async (data, options) => {
        ensureOpen("write");
        const settings = typeof options === 'string' ? { encoding: options } : (options || {});
        const encoding = settings.encoding || 'utf8';
        const signal = settings.signal;
        const checkAbort = () => {
          if (!signal?.aborted) return;
          const error = new Error('The operation was aborted');
          error.name = 'AbortError';
          error.code = 'ABORT_ERR';
          throw error;
        };
        const chunks = [];
        if (typeof data === 'string' || ArrayBuffer.isView(data)) {
          chunks.push(data);
        } else if (data && (typeof data[Symbol.asyncIterator] === 'function' || typeof data[Symbol.iterator] === 'function')) {
          for await (const chunk of data) chunks.push(chunk);
        } else {
          const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, DataView, or Iterable.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        await new Promise((resolve) => setImmediate(resolve));
        checkAbort();
        for (const chunk of chunks) {
          checkAbort();
          if (typeof chunk !== 'string' && !ArrayBuffer.isView(chunk)) {
            const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView.');
            error.code = 'ERR_INVALID_ARG_TYPE';
            throw error;
          }
          const buffer = typeof chunk === 'string'
            ? Buffer.from(chunk, encoding)
            : Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength);
          let offset = 0;
          while (offset < buffer.length) {
            checkAbort();
            const written = fs.writeSync(fd, buffer, offset, buffer.length - offset, null);
            if (written === 0) break;
            offset += written;
          }
        }
      },
      appendFile: (...args) => handle.writeFile(...args),
      writev: (buffers, position) => Promise.resolve().then(() => ({
        bytesWritten: (ensureOpen("writev"), fs.writevSync(fd, buffers, position)),
        buffers,
      })),
      readv: (buffers, position) => Promise.resolve().then(() => ({
        bytesRead: (ensureOpen("readv"), fs.readvSync(fd, buffers, position)),
        buffers,
      })),
      stat: (...statArgs) => Promise.resolve().then(() => (ensureOpen("fstat"), fs.fstatSync(fd, ...statArgs))),
      chmod: (...chmodArgs) => Promise.resolve().then(() => (ensureOpen("fchmod"), fs.fchmodSync(fd, ...chmodArgs))),
      utimes: (...timeArgs) => Promise.resolve().then(() => (ensureOpen("futimes"), fs.futimesSync(fd, ...timeArgs))),
      truncate: (length = 0) => Promise.resolve().then(() => (ensureOpen("ftruncate"), fs.ftruncateSync(fd, length))),
      sync: () => Promise.resolve().then(() => (ensureOpen("fsync"), fs.fsyncSync(fd))),
      datasync: () => Promise.resolve().then(() => (ensureOpen("fdatasync"), fs.fdatasyncSync(fd))),
    };
    registerHandle(handle, fd);
    handle[Symbol.asyncDispose] = handle.close;
    handle[Symbol.dispose] = handle.close;
    return handle;
  }),
});
}"#
);

const ASYNC_SYNC_API: &str = r#"(fsyncSync, fdatasyncSync) => {
  const create = (sync, syscall) => function(fd, callback) {
    if (typeof fd !== 'number') {
      const received = fd === null || fd === undefined ? ` Received ${fd}` : ` Received type ${typeof fd} (${String(fd)})`;
      const error = new TypeError(`The "fd" argument must be of type number.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(fd) || fd < 0 || fd > 0x7FFFFFFF) {
      const error = new RangeError(`The value of "fd" is out of range. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { sync(fd); callback(null); }
      catch (error) { callback(error); }
    });
  };
  return { fsync: create(fsyncSync, 'fsync'), fdatasync: create(fdatasyncSync, 'fdatasync') };
}"#;

const OPEN_CLOSE_FACTORY: &str = quench_js_check::checked_js!(
    r#"(openSync, closeSync, readSync, fstatSync) => {
  const assertBuffer = (buffer) => {
    if (ArrayBuffer.isView(buffer)) return;
    const received = buffer === null || buffer === undefined
      ? ` Received ${buffer}`
      : typeof buffer === "number" || typeof buffer === "boolean" || typeof buffer === "string"
        ? ` Received type ${typeof buffer} (${typeof buffer === "string" ? `'${buffer}'` : String(buffer)})`
        : ` Received an instance of ${Array.isArray(buffer) ? "Array" : "Object"}`;
    const error = new TypeError(`The "buffer" argument must be an instance of Buffer, TypedArray, or DataView.${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  };
  const validateReadRange = (buffer, offset, length, position) => {
    for (const [name, value] of [["offset", offset], ["length", length]]) {
      if (value != null && typeof value !== "number") {
        const error = new TypeError(`The "${name}" argument must be of type number.`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    }
    offset ??= 0;
    length ??= buffer.byteLength - offset;
    if (!Number.isInteger(offset) || offset < 0 || offset > buffer.byteLength) {
      const message = offset > buffer.byteLength
        ? `The value of "offset" is out of range. It must be <= ${buffer.byteLength}. Received ${offset}`
        : `The value of "offset" is out of range. It must be an integer. Received ${String(offset)}`;
      const error = new RangeError(message);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isInteger(length) || length < 0 || length > buffer.byteLength - offset) {
      const message = length < 0
        ? `The value of "length" is out of range. It must be >= 0. Received ${length}`
        : length > buffer.byteLength - offset
          ? `The value of "length" is out of range. It must be <= ${buffer.byteLength - offset}. Received ${length}`
        : `The value of "length" is out of range. Received ${String(length)}`;
      const error = new RangeError(message);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (position != null && typeof position !== "number" && typeof position !== "bigint") {
      const error = new TypeError('The "position" argument must be of type number or bigint.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof position === "bigint" && (position < -1n || position > 9223372036854775807n - BigInt(length))) {
      const error = new RangeError(`The value of "position" is out of range. Received ${String(position)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (typeof position === "number" && (!Number.isInteger(position) || position < 0 || position > Number.MAX_SAFE_INTEGER)) {
      const error = new RangeError(`The value of "position" is out of range. Received ${String(position)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return [offset, length];
  };
  const validatePath = (path) => {
    if (typeof path !== "string" && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  };
  const normalizeMode = (mode) => {
    if (typeof mode === "string") {
      const parsed = Number.parseInt(mode, 8);
      if (Number.isNaN(parsed)) {
        const error = new TypeError('The "mode" argument must be a valid integer');
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      mode = parsed;
    }
    if (mode != null && typeof mode !== "number") {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (mode != null && (!Number.isInteger(mode) || mode < 0 || mode > 0xFFFFFFFF)) {
      const requirement = Number.isInteger(mode) ? '>= 0 && <= 4294967295' : 'an integer';
      const error = new RangeError(`The value of "mode" is out of range. It must be ${requirement}. Received ${String(mode)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return mode;
  };
  const validateFlags = (flags) => {
    if (typeof flags !== "number") return;
    if (!Number.isInteger(flags) || flags < -0x80000000 || flags > 0x7FFFFFFF) {
      const requirement = Number.isInteger(flags)
        ? '>= -2147483648 && <= 2147483647'
        : 'an integer';
      const error = new RangeError(`The value of "flags" is out of range. It must be ${requirement}. Received ${String(flags)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  };
  const validateFd = (fd) => {
    if (typeof fd === "number") return;
    let received;
    if (fd === null || fd === undefined) received = ` Received ${fd}`;
    else if (typeof fd === "object") received = ` Received an instance of ${Array.isArray(fd) ? "Array" : "Object"}`;
    else received = ` Received type ${typeof fd} (${typeof fd === "string" ? `'${fd}'` : String(fd)})`;
    const error = new TypeError(`The "fd" argument must be of type number.${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  };
  const readInto = (fd, buffer, offset, length, position) => {
    assertBuffer(buffer);
    if (position === -1) position = null;
    length ??= buffer.byteLength - (offset ?? 0);
    if (buffer.byteLength === 0 && length > 0) {
      const name = buffer.constructor?.name || "TypedArray";
      const error = new TypeError(`The argument 'buffer' is empty and cannot be written. Received ${name}(0) []`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    [offset, length] = validateReadRange(buffer, offset, length, position);
    const bytesRead = readSync(fd, buffer, offset ?? 0, length, position ?? null);
    return { bytesRead, buffer };
  };
  const api = {
  open(path, flags, mode, callback) {
    if (typeof flags === "function") {
      callback = flags;
      flags = undefined;
      mode = undefined;
    } else if (typeof mode === "function") {
      callback = mode;
      mode = undefined;
    }
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    validateFlags(flags);
    validatePath(path);
    mode = normalizeMode(mode);
    const activeRequests = globalThis.process.__quenchActiveRequests ||
      (globalThis.process.__quenchActiveRequests = []);
    const request = {};
    const AsyncResource = globalThis.process.getBuiltinModule?.('async_hooks')?.AsyncResource;
    const asyncResource = typeof AsyncResource === 'function'
      ? new AsyncResource('FSREQCALLBACK')
      : undefined;
    const invokeCallback = (...callbackArgs) => {
      try { return Reflect.apply(callback, undefined, callbackArgs); }
      catch (callbackError) {
        // Native FSREQCALLBACK dispatch invokes the process fatal-exception
        // route before leaving the async resource scope. Preserve that order
        // for this host-backed operation so domains can observe the active
        // process.domain while handling a callback throw.
        if (globalThis.process.domain &&
            typeof globalThis.process._fatalException === 'function' &&
            globalThis.process._fatalException(callbackError, false) === true) {
          return undefined;
        }
        throw callbackError;
      }
    };
    activeRequests.push(request);
    queueMicrotask(() => {
      const requestIndex = activeRequests.indexOf(request);
      if (requestIndex !== -1) activeRequests.splice(requestIndex, 1);
      let error;
      let fd;
      try { fd = openSync(path, flags, mode); }
      catch (caught) { error = caught; }
      try {
        const callbackArgs = error ? [error] : [null, fd];
        if (asyncResource) asyncResource.runInAsyncScope(invokeCallback, undefined, ...callbackArgs);
        else invokeCallback(...callbackArgs);
      } finally {
        asyncResource?.emitDestroy();
      }
    });
  },
  close(fd, callback) {
    validateFd(fd);
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    queueMicrotask(() => {
      try {
        closeSync(fd);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  },
  fstat(fd, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    validateFd(fd);
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, fstatSync(fd, options)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  },
  read(fd, bufferOrOptions, offsetOrOptions, length, position, callback) {
    validateFd(fd);
    let buffer = bufferOrOptions;
    let offset = 0;
    let readLength;
    let readPosition = null;
    if (!ArrayBuffer.isView(bufferOrOptions)) {
      if (bufferOrOptions && typeof bufferOrOptions === "object" && ArrayBuffer.isView(bufferOrOptions.buffer)) {
        buffer = bufferOrOptions.buffer;
        offset = bufferOrOptions.offset ?? 0;
        readLength = bufferOrOptions.length;
        readPosition = bufferOrOptions.position ?? null;
        callback = offsetOrOptions;
      } else {
        if (bufferOrOptions === undefined || bufferOrOptions === null ||
            (typeof bufferOrOptions === "object" && !Array.isArray(bufferOrOptions))) {
          const options = bufferOrOptions || {};
          buffer = Buffer.alloc(16384);
          offset = options.offset ?? 0;
          readLength = options.length;
          readPosition = options.position ?? null;
          callback = offsetOrOptions;
        } else if (typeof bufferOrOptions === "function") {
          buffer = Buffer.alloc(16384);
          callback = bufferOrOptions;
        } else {
          buffer = bufferOrOptions;
          callback = offsetOrOptions;
        }
      }
    } else if (offsetOrOptions && typeof offsetOrOptions === "object") {
      offset = offsetOrOptions.offset ?? 0;
      readLength = offsetOrOptions.length;
      readPosition = offsetOrOptions.position ?? null;
      callback = length;
    } else {
      if (typeof callback === "function") {
        offset = offsetOrOptions ?? 0;
        readLength = length;
        readPosition = position ?? null;
      } else if (typeof offsetOrOptions === "function") {
        callback = offsetOrOptions;
      } else if (typeof length === "function") {
        offset = offsetOrOptions ?? 0;
        callback = length;
      } else if (typeof position === "function") {
        offset = offsetOrOptions ?? 0;
        readLength = length;
        callback = position;
      } else {
        offset = offsetOrOptions ?? 0;
        readLength = length;
        readPosition = position ?? null;
      }
    }
    assertBuffer(buffer);
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    readLength ??= buffer.byteLength - offset;
    const empty = readLength !== 0 && buffer?.byteLength === 0;
    if (empty) {
      const name = buffer.constructor?.name || "TypedArray";
      const error = new TypeError(`The argument 'buffer' is empty and cannot be written. Received ${name}(0) []`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (readPosition === -1) readPosition = null;
    [offset, readLength] = validateReadRange(buffer, offset, readLength, readPosition);
    queueMicrotask(() => {
      try {
        const result = readInto(fd, buffer, offset, readLength, readPosition);
        Reflect.apply(callback, undefined, [null, result.bytesRead, result.buffer]);
      } catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  },
  };
  api.read[Symbol.for("nodejs.util.promisify.custom")] = (fd, ...args) => new Promise((resolve, reject) => {
    api.read(fd, ...args, (error, bytesRead, buffer) => {
      if (error) reject(error);
      else resolve({ bytesRead, buffer });
    });
  });
  return api;
}"#
);

const WRITEV_API: &str = quench_js_check::checked_js!(
    r#"(writeSync) => {
  const validateBuffers = (buffers) => {
    if (!Array.isArray(buffers) || buffers.some((buffer) => !Buffer.isBuffer(buffer) && !(buffer instanceof Uint8Array))) {
      const error = new TypeError('The "buffers" argument must be an Array of Buffer or Uint8Array instances');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  function writevSync(fd, buffers, position = null) {
    validateBuffers(buffers);
    const bytes = Buffer.concat(buffers);
    if (bytes.length === 0) {
      writeSync(fd, bytes, 0, 0, position);
      return 0;
    }
    return writeSync(fd, bytes, 0, bytes.length, position);
  }
  function writev(fd, buffers, position, callback) {
    if (typeof position === 'function') { callback = position; position = null; }
    if (callback === undefined && typeof position !== 'function') callback = undefined;
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
      throw error;
    }
    validateBuffers(buffers);
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, writevSync(fd, buffers, position), buffers]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  }
  return { writev, writevSync };
}"#
);

const WRITE_API: &str = quench_js_check::checked_js!(
    r#"(writeSync) => {
  const typeError = (name, message = `The \"${name}\" argument must be of type number`) => {
    const error = new TypeError(message); error.code = 'ERR_INVALID_ARG_TYPE'; return error;
  };
  const rangeError = (name, value) => {
    const error = new RangeError(`The \"${name}\" argument is out of range`);
    error.code = 'ERR_OUT_OF_RANGE'; error.actual = value; return error;
  };
  function write(fd, data, ...args) {
    if (!Number.isInteger(fd) || fd < 0) throw typeError('fd', 'The \"fd\" argument must be of type number');
    if (typeof data !== 'string' && !ArrayBuffer.isView(data)) {
      throw typeError('buffer', 'The \"buffer\" argument must be an instance of Buffer or Uint8Array');
    }
    const callback = args.pop();
    if (typeof callback !== 'function') throw typeError('callback', 'The \"callback\" argument must be of type function');
    let buffer, offset = 0, length, position = null;
    if (typeof data === 'string') {
      position = args[0] == null ? null : args[0];
      const encoding = typeof args[1] === 'string' ? args[1] : 'utf8';
      buffer = Buffer.from(data, encoding);
      length = buffer.length;
    } else {
      buffer = data instanceof Uint8Array ? data : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      const options = args[0];
      if (options === null || (options !== undefined && typeof options === 'object' && !Array.isArray(options))) {
        if (options !== null) {
          offset = options.offset == null ? 0 : options.offset;
          length = options.length == null ? buffer.byteLength - offset : options.length;
          position = options.position == null ? null : options.position;
        } else {
          length = buffer.byteLength;
        }
      } else {
        if (options !== undefined && typeof options !== 'number') throw typeError('options');
        offset = args[0] === undefined ? 0 : args[0];
        length = args[1] === undefined ? buffer.byteLength - offset : args[1];
        position = args[2] == null ? null : args[2];
      }
      if (typeof offset !== 'number') throw typeError('offset');
      if (!Number.isInteger(offset)) {
        const error = rangeError('offset', offset);
        error.message = `The value of "offset" is out of range. It must be an integer. Received ${String(offset)}`;
        throw error;
      }
      if (typeof length !== 'number') throw typeError('length');
      if (!Number.isInteger(length)) throw rangeError('length', length);
      if (position !== null && (typeof position !== 'number' || !Number.isInteger(position))) throw typeError('position');
      if (offset < 0 || offset > buffer.byteLength) throw rangeError('offset', offset);
      if (length < 0 || length > 0x7fffffff) {
        const error = new RangeError(`The value of "length" is out of range. It must be >= 0 && <= 2147483647. Received ${String(length)}`);
        error.code = 'ERR_OUT_OF_RANGE';
        error.actual = length;
        throw error;
      }
      if (offset + length > buffer.byteLength) {
        const maxLength = buffer.byteLength - offset;
        const error = new RangeError(`The value of "length" is out of range. It must be <= ${maxLength}. Received ${String(length)}`);
        error.code = 'ERR_OUT_OF_RANGE';
        error.actual = length;
        throw error;
      }
      if (position !== null && position < -1) throw rangeError('position', position);
    }
    process.nextTick(() => {
      try {
        const bytesWritten = writeSync(fd, buffer, offset, length, position);
        callback(null, bytesWritten, data);
      } catch (error) { callback(error); }
    });
  }
  write[Symbol.for('nodejs.util.promisify.custom')] = (fd, data, ...args) =>
    new Promise((resolve, reject) => write(fd, data, ...args, (error, bytesWritten, buffer) =>
      error ? reject(error) : resolve({ bytesWritten, buffer })));
  return write;
}"#
);

const READV_API: &str = quench_js_check::checked_js!(
    r#"(readSync) => {
  const validateBuffers = (buffers) => {
    if (!Array.isArray(buffers) || buffers.some((buffer) => !Buffer.isBuffer(buffer) && !(buffer instanceof Uint8Array))) {
      const error = new TypeError('The "buffers" argument must be an Array of Buffer or Uint8Array instances');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  function readvSync(fd, buffers, position = null) {
    validateBuffers(buffers);
    let bytesRead = 0;
    for (const buffer of buffers) {
      const count = readSync(fd, buffer, 0, buffer.byteLength, position == null ? null : position + bytesRead);
      bytesRead += count;
      if (count < buffer.byteLength) break;
    }
    return bytesRead;
  }
  function readv(fd, buffers, position, callback) {
    if (typeof position === 'function') { callback = position; position = null; }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
      throw error;
    }
    validateBuffers(buffers);
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, readvSync(fd, buffers, position), buffers]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  }
  readv[Symbol.for('nodejs.util.promisify.custom')] = (fd, buffers, position = null) =>
    new Promise((resolve, reject) => readv(fd, buffers, position, (error, bytesRead, readBuffers) =>
      error ? reject(error) : resolve({ bytesRead, buffers: readBuffers })));
  return { readv, readvSync };
}"#
);

const ASYNC_READDIR_FACTORY: &str = quench_js_check::checked_js!(
    r#"(readdirSync) => function readdir(path, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof path !== "string" && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const received = path === null ? "null" : path === undefined ? "undefined" : `type ${typeof path}`;
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  // Node normalizes the options before starting the asynchronous operation.
  // Callers may mutate the original object as soon as readdir() returns.
  if (options !== null && typeof options === "object") {
    options = {
      encoding: options.encoding,
      withFileTypes: options.withFileTypes,
      recursive: options.recursive,
    };
  }
  if (path instanceof URL) path = path.pathname;
  let entries;
  let failure;
  try {
    // Start the directory operation before returning, as Node does. Besides
    // preserving asynchronous callback delivery, this keeps subsequent
    // internalBinding('fs').readdir replacements from changing an operation
    // that was already initiated by an earlier call.
    entries = readdirSync(path, options);
  } catch (error) {
    failure = error;
  }
  queueMicrotask(() => {
    if (failure) Reflect.apply(callback, undefined, [failure]);
    else Reflect.apply(callback, undefined, [null, entries]);
  });
}"#
);

const OPENDIR_API: &str = quench_js_check::checked_js!(
    r#"(readdirSync) => {
  const states = new WeakMap();
  const invalidThis = () => {
    const error = new TypeError('Receiver must be an instance of class Dir');
    error.code = 'ERR_INVALID_THIS';
    throw error;
  };
  const dirError = (code, message) => Object.assign(new Error(message), { code });
  const invalidCallback = () => {
    const error = new TypeError('The "callback" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
    return error;
  };
  class Dir {
    constructor(path, entries) {
      if (arguments.length < 2) {
        const error = new TypeError('The "handle" argument must be specified');
        error.code = 'ERR_MISSING_ARGS';
        throw error;
      }
      states.set(this, { path, entries, index: 0, closed: false, busy: false, pending: 0 });
    }
    get path() {
      const state = states.get(this);
      if (!state) return invalidThis();
      return state.path;
    }
    readSync() {
      const state = states.get(this);
      if (!state) return invalidThis();
      if (state.busy) throw dirError('ERR_DIR_CONCURRENT_OPERATION', 'Directory is already processing a request');
      if (state.closed) throw dirError('ERR_DIR_CLOSED', 'Directory handle was closed');
      const entry = state.entries[state.index++];
      return entry === undefined ? null : entry;
    }
    closeSync() {
      const state = states.get(this);
      if (!state) return invalidThis();
      if (state.busy) throw dirError('ERR_DIR_CONCURRENT_OPERATION', 'Directory is already processing a request');
      if (state.closed) throw dirError('ERR_DIR_CLOSED', 'Directory handle was closed');
      state.closed = true;
    }
    [Symbol.dispose]() {
      const state = states.get(this);
      if (!state) return invalidThis();
      if (state.closed) return;
      this.closeSync();
    }
    async [Symbol.asyncDispose]() {
      const state = states.get(this);
      if (!state) return invalidThis();
      if (state.closed) return;
      await this.close();
    }
    read(callback) {
      if (callback === undefined) return new Promise((resolve, reject) => this.read((error, entry) => error ? reject(error) : resolve(entry)));
      if (typeof callback !== 'function') throw invalidCallback();
      const state = states.get(this);
      if (!state) return invalidThis();
      state.pending++;
      state.busy = true;
      queueMicrotask(() => {
        state.pending--;
        state.busy = state.pending > 0;
        try {
          if (state.closed) throw dirError('ERR_DIR_CLOSED', 'Directory handle was closed');
          const entry = state.entries[state.index++];
          callback(null, entry === undefined ? null : entry);
        } catch (error) { callback(error); }
      });
    }
    close(callback) {
      if (callback === undefined) return new Promise((resolve, reject) => this.close((error) => error ? reject(error) : resolve()));
      if (callback !== undefined && typeof callback !== 'function') throw invalidCallback();
      const state = states.get(this);
      if (!state) return invalidThis();
      const finish = () => {
        if (state.pending > 0) { queueMicrotask(finish); return; }
        try { this.closeSync(); callback(null); } catch (error) { callback(error); }
      };
      queueMicrotask(finish);
    }
    async next() {
      const value = await new Promise((resolve, reject) => this.read((error, entry) => error ? reject(error) : resolve(entry)));
      if (value === null) { await this.close(); return { done: true, value: undefined }; }
      return { done: false, value };
    }
    async return() {
      const state = states.get(this);
      if (!state.closed) await this.close();
      return { done: true, value: undefined };
    }
    [Symbol.asyncIterator]() { return this; }
  }
  const makeDir = (path, options) => {
    if (options != null && typeof options !== 'object') throw Object.assign(new TypeError('The "options" argument must be of type object'), { code: 'ERR_INVALID_ARG_TYPE' });
    const bufferSize = options && Object.prototype.hasOwnProperty.call(options, 'bufferSize') ? options.bufferSize : 32;
    if (typeof bufferSize !== 'number') throw Object.assign(new TypeError('The "bufferSize" argument must be of type number.'), { code: 'ERR_INVALID_ARG_TYPE' });
    if (!Number.isInteger(bufferSize) || bufferSize < 1) throw Object.assign(new RangeError('The value of "bufferSize" is out of range. It must be >= 1'), { code: 'ERR_OUT_OF_RANGE' });
    const entries = readdirSync(path, { withFileTypes: true });
    return new Dir(path, entries);
  };
  function opendirSync(path, options) { return makeDir(path, options); }
  function opendir(path, options, callback) {
    if (typeof options === 'function') { callback = options; options = undefined; }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
      throw error;
    }
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) throw Object.assign(new TypeError('The "path" argument must be of type string, Buffer, or URL.'), { code: 'ERR_INVALID_ARG_TYPE' });
    if (Buffer.isBuffer(path)) path = path.toString();
    else if (path instanceof URL) path = decodeURIComponent(path.pathname);
    queueMicrotask(() => { try { callback(null, makeDir(path, options)); } catch (error) { callback(error); } });
  }
  const opendirPromise = (path, options) => Promise.resolve().then(() => makeDir(path, options));
  return { Dir, opendir, opendirSync, opendirPromise };
}"#
);

const ASYNC_READ_FILE_FACTORY: &str = quench_js_check::checked_js!(
    r#"(readFileSync, validateSignal, completeRead, toPathIfFileURL) => function readFile(path, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof path !== 'number' && typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : typeof path === 'function'
          ? ` Received function ${path.name}`
          : ` Received type ${typeof path} (${typeof path === 'string' ? `'${path}'` : String(path)})`;
    const error = new TypeError(`The "path" argument must be of type string or an instance of Buffer or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  const encoding = typeof options === "string" ? options : options?.encoding;
  if (encoding != null && !Buffer.isEncoding(encoding)) {
    const error = new TypeError(`The "${typeof options === "string" ? "encoding" : "options.encoding"}" argument is invalid. Received ${String(encoding)}`);
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (path !== null && typeof path === 'object' &&
      typeof path.pathname === 'string' && typeof path.protocol === 'string') {
    const filePath = path.pathname;
    const hrefPath = typeof path.href === 'string'
      ? path.href.replace(/^file:\/\/[^/]*(\/[^?#]*)?.*$/i, '$1')
      : '';
    if (/%2f/i.test(filePath) || /%2f/i.test(hrefPath) ||
        (process.platform === "win32" && (/%5c/i.test(filePath) || /%5c/i.test(hrefPath)))) {
      const error = new TypeError(process.platform === "win32"
        ? "File URL path must not include encoded \\ or / characters"
        : "File URL path must not include encoded / characters");
      error.code = "ERR_INVALID_FILE_URL_PATH";
      error.input = path;
      throw error;
    }
  }
  if (typeof path !== 'number') path = toPathIfFileURL(path);
  if (Buffer.isBuffer(path)) path = path.toString();
  if (typeof path === 'string' && path.includes('\0')) {
    const error = new TypeError('The "path" argument must be a string, Uint8Array, or URL without null bytes');
    error.code = 'ERR_INVALID_ARG_VALUE';
    throw error;
  }
  const signal = options?.signal;
  validateSignal(signal);
  queueMicrotask(() => completeRead(readFileSync, callback, path, options, signal));
}"#
);

const ASYNC_WRITE_FILE_FACTORY: &str = quench_js_check::checked_js!(
    r#"(writeFileSync, fsModule) => function writeFile(path, data, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  // libuv copies typed-array and Buffer data before returning from Node's
  // asynchronous writeFile API. Snapshot views here before deferring work so
  // detaching or resizing their backing store cannot change the pending write.
  if (ArrayBuffer.isView(data)) {
    data = Buffer.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = path.pathname;
  queueMicrotask(() => {
    if (options?.signal?.aborted) {
      const error = new Error('The operation was aborted');
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      Reflect.apply(callback, undefined, [error]);
      return;
    }
    if (options?.flush === true) {
      const settings = typeof options === "string" ? { encoding: options } : options;
      const flags = settings?.flag || "w";
      const mode = settings?.mode;
      fsModule.open(path, flags, mode, (openError, fd) => {
        if (openError) {
          Reflect.apply(callback, undefined, [openError]);
          return;
        }
        try {
          writeFileSync(fd, data, { ...settings, flush: false });
        } catch (error) {
          fsModule.close(fd, () => Reflect.apply(callback, undefined, [error]));
          return;
        }
        fsModule.fsync(fd, (syncError) => {
          fsModule.close(fd, (closeError) =>
            Reflect.apply(callback, undefined, [syncError || closeError || null]));
        });
      });
      return;
    }
    try {
      writeFileSync(path, data, options);
      Reflect.apply(callback, undefined, [null]);
    } catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#
);

const ASYNC_APPEND_FILE_FACTORY: &str = quench_js_check::checked_js!(
    r#"(appendFileSync, fsModule) => function appendFile(path, data, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (ArrayBuffer.isView(data)) {
    data = Buffer.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
  }
  if (typeof data !== "string" && !ArrayBuffer.isView(data)) {
    const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = path.pathname;
  if (typeof path === "string" && path.includes("\0")) {
    const received = path.replace(/\0/g, "\\x00");
    const error = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${received}'`);
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  queueMicrotask(() => {
    if (options?.flush === true) {
      const settings = typeof options === "string" ? { encoding: options } : options;
      const flags = settings?.flag || "a";
      const mode = settings?.mode;
      fsModule.open(path, flags, mode, (openError, fd) => {
        if (openError) {
          Reflect.apply(callback, undefined, [openError]);
          return;
        }
        try {
          appendFileSync(fd, data, { ...settings, flush: false });
        } catch (error) {
          fsModule.close(fd, () => Reflect.apply(callback, undefined, [error]));
          return;
        }
        fsModule.fsync(fd, (syncError) => {
          fsModule.close(fd, (closeError) =>
            Reflect.apply(callback, undefined, [syncError || closeError || null]));
        });
      });
      return;
    }
    try {
      appendFileSync(path, data, options);
      Reflect.apply(callback, undefined, [null]);
    } catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#
);

const REALPATH_SYNC_API: &str = r#"(hostRealpathSync) => {
  function createRealpathSync(native) {
    return function realpathSync(path, options) {
      if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
        const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${typeof path === 'string' ? `'${path}'` : String(path)})`}`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      if (Buffer.isBuffer(path)) path = path.toString();
      else if (path instanceof URL) path = decodeURIComponent(path.pathname);
      const resolved = hostRealpathSync(path, native);
      const encoding = typeof options === 'string' ? options : options?.encoding;
      if (encoding === 'buffer') return Buffer.from(resolved);
      return encoding === undefined ? resolved : Buffer.from(resolved).toString(encoding);
    };
  }
  const realpathSync = createRealpathSync(false);
  realpathSync.native = createRealpathSync(true);
  return realpathSync;
}"#;

const REALPATH_FACTORY: &str = quench_js_check::checked_js!(
    r#"(realpathSync) => {
  function createRealpath(native) {
    return function realpath(path, options, callback) {
      if (typeof options === "function") callback = options;
      if (typeof callback !== "function") {
        const error = new TypeError('The "cb" argument must be of type function');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      queueMicrotask(() => {
        try { Reflect.apply(callback, undefined, [null, native ? realpathSync.native(path, options) : realpathSync(path, options)]); }
        catch (error) { Reflect.apply(callback, undefined, [error]); }
      });
    };
  }
  const realpath = createRealpath(false);
  realpath.native = createRealpath(true);
  return realpath;
}"#
);

const CREATE_READ_STREAM: &str = r#"(openFile, readFileChunk, closeFile, Readable, defaultFs) => {
  function ReadStream(path, options) {
    if (!(this instanceof ReadStream)) return new ReadStream(path, options);

    if (typeof options !== "string" && options != null && typeof options !== "object") {
      const error = new TypeError('The "options" argument must be of type object or string.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const settings = typeof options === "string" ? { encoding: options } : options || {};
    if (settings.signal !== undefined && !(settings.signal instanceof AbortSignal)) {
      const error = new TypeError('The "options.signal" property must be an instance of AbortSignal.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (settings.windowsHandle !== undefined) {
      if (settings.fd !== undefined && settings.fd !== null) {
        const error = new Error('The "fd" and "windowsHandle" options cannot be used together.');
        error.code = "ERR_INCOMPATIBLE_OPTION_PAIR";
        throw error;
      }
      if (process.platform !== "win32") {
        const error = new Error('The "windowsHandle" option is only supported on Windows.');
        error.code = "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM";
        throw error;
      }
    }
    if (settings.fs !== undefined) {
      for (const name of ["open", "close", "read"]) {
        if (typeof settings.fs?.[name] !== "function") {
          const value = settings.fs?.[name];
          const received = value === null ? "null" : typeof value === "string"
            ? `type string ('${value}')`
            : typeof value === "object" ? `an instance of ${Array.isArray(value) ? "Array" : "Object"}`
              : `type ${typeof value} (${String(value)})`;
          const error = new TypeError(`The "options.fs.${name}" property must be of type function. Received ${received}`);
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
      }
    }
    const fileHandleFd = settings.fd && typeof settings.fd === "object" &&
      typeof settings.fd.fd === "number" ? settings.fd.fd : null;
    const fileHandle = fileHandleFd === null ? null : settings.fd;
    const suppliedFd = typeof settings.fd === "number" || fileHandleFd !== null;
    const descriptor = typeof settings.fd === "number" ? settings.fd : fileHandleFd;
    if (fileHandle !== null && settings.fs !== undefined) {
      const error = new Error('The FileHandle with fs method is not implemented');
      error.code = 'ERR_METHOD_NOT_IMPLEMENTED';
      throw error;
    }
    if (typeof path !== "string" &&
        !Buffer.isBuffer(path) && !(path instanceof URL)) {
      if (!(path == null && suppliedFd)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
      }
    }
    for (const name of ["start", "end"]) {
      const value = settings[name];
      if (value === undefined || (name === "end" && value === Infinity)) continue;
      if (typeof value !== "number") {
        const error = new TypeError(`The "${name}" option must be of type number.`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isSafeInteger(value) || value < 0) {
        const error = new RangeError(`The value of "${name}" is out of range.`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
    }
    if (settings.start !== undefined && settings.end !== undefined && settings.start > settings.end) {
      const error = new RangeError(`The value of "start" is out of range. It must be <= "end" (here: ${settings.end}). Received ${settings.start}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const autoClose = settings.autoClose !== false;
    const fileSystem = settings.fs || defaultFs;
    const start = settings.start;
    const end = settings.end;
    let position = start;
    let bytesRead = 0;
    let explicitClose = false;
    let phase = suppliedFd ? "open" : "idle";
    let fd = suppliedFd ? descriptor : null;
    let pendingReadSize;
    const stream = this;
    stream.start = start;
    stream.end = end;
    stream.autoClose = autoClose;
    // Node leaves `.path` undefined when a caller supplies only an open file
    // descriptor. Preserve `null` as an omitted path in that case.
    stream.path = path === null && suppliedFd ? undefined : path;

    const closeFileOnce = () => {
      if (!autoClose && !explicitClose) return false;
      if (fd === null) return false;
      const current = fd;
      fd = null;
      stream.fd = null;
      if (fileHandle !== null) {
        // A stream created with a FileHandle owns it by default, just like a
        // stream created with a numeric descriptor owns that descriptor.
        // Going through the handle also preserves its close event and state.
        return fileHandle.close();
      } else {
        return new Promise((resolve, reject) => {
          fileSystem.close(current, (error) => error ? reject(error) : resolve());
        });
      }
      return true;
    };

    const fail = (error) => {
      phase = "failed";
      stream.pending = false;
      // With a caller-owned descriptor and autoClose:false, Node reports an
      // asynchronous read error while leaving the stream and descriptor open.
      if (suppliedFd && !autoClose) {
        stream.emit("error", error);
        return;
      }
      stream.destroy(error);
    };

    const scheduleRead = (size) => {
      if (phase !== "open") return;
      const currentPosition = position === undefined ? bytesRead : position;
      const remaining = end !== undefined
        ? end - currentPosition + 1
        : undefined;
      if (remaining !== undefined && remaining <= 0) {
        phase = "ended";
        stream.push(null);
        return;
      }
      const requestedSize = Number.isSafeInteger(size) && size > 0
        ? Math.min(size, 64 * 1024)
        : 64 * 1024;
      const readSize = remaining === undefined ? requestedSize : Math.min(requestedSize, remaining);
      phase = "reading";
      setImmediate(async () => {
        if (stream.destroyed) {
          phase = "destroyed";
          return;
        }
        try {
          let chunk;
          if (fileHandle !== null) {
            const buffer = Buffer.alloc(readSize);
            const result = await fileHandle.read(
              buffer,
              0,
              readSize,
              position === undefined ? null : position,
            );
            chunk = result.bytesRead === 0 ? null : buffer.subarray(0, result.bytesRead);
          } else {
            const buffer = Buffer.alloc(readSize);
            const result = await new Promise((resolve, reject) => {
              fileSystem.read(fd, buffer, 0, readSize, position, (error, bytesRead, resultBuffer) => {
                if (error) reject(error);
                else resolve({ bytesRead, buffer: resultBuffer || buffer });
              });
            });
            chunk = result.bytesRead === 0 ? null : result.buffer.subarray(0, result.bytesRead);
          }
          if (chunk === null) {
            phase = "ended";
            stream.push(null);
            return;
          }
          bytesRead += chunk.byteLength;
          stream.bytesRead = bytesRead;
          if (position !== undefined) position += chunk.byteLength;
          phase = "open";
          stream.push(chunk);
        } catch (error) {
          fail(error);
        }
      });
    };

    // Opening is part of ReadStream construction, even when nobody has asked
    // the stream for data yet. Node streams can report an open failure through
    // their error event before the consumer starts reading.
    const beginOpen = (size) => {
      if (phase !== "idle") return;
      phase = "opening";
      setImmediate(() => {
        if (stream.destroyed) {
          phase = "destroyed";
          return;
        }
        if (typeof stream.open === "function") {
          // Preserve Node's legacy open() override hook. A patched open method
          // owns the continuation, just as ReadStream._construct does in Node.
          stream.open();
          return;
        }
        fileSystem.open(path, settings.flags === undefined ? "r" : settings.flags,
          settings.mode === undefined ? 0o666 : settings.mode, (error, openedFd) => {
            if (error) { fail(error); return; }
            fd = openedFd;
            stream.fd = fd;
            stream.pending = false;
            phase = "open";
            stream.emit("open", fd);
            stream.emit("ready");
            const requestedSize = pendingReadSize === undefined ? size : pendingReadSize;
            if (requestedSize !== undefined) scheduleRead(requestedSize);
          });
      });
    };

    Readable.call(stream, {
      highWaterMark: settings.highWaterMark,
      autoDestroy: autoClose,
      read(size) {
        if (phase === "open") {
          scheduleRead(size);
        } else if (phase === "idle") {
          beginOpen(size);
        } else if (phase === "opening") {
          pendingReadSize = size;
        }
      },
      destroy(error, callback) {
        if (phase !== "ended" && phase !== "failed") phase = "destroyed";
        try {
          const closing = closeFileOnce();
          if (closing && typeof closing.then === "function") {
            closing.then(() => callback(error), (closeError) => callback(error || closeError));
          } else {
            callback(error);
          }
        } catch (closeError) {
          callback(error || closeError);
        }
      },
    });
    stream.fd = fd;
    stream.pending = fd === null;
    stream.bytesRead = 0;
    stream.close = function close(callback) {
      explicitClose = true;
      if (callback) {
        if (stream.closed) setImmediate(callback);
        else stream.once("close", callback);
      }
      stream.destroy();
      return stream;
    };
    if (fileHandle !== null && typeof fileHandle.once === "function") {
      fileHandle.once("close", () => {
        fd = null;
        stream.fd = null;
        if (!stream.destroyed) stream.destroy();
      });
    }
    if (settings.signal !== undefined) {
      const signal = settings.signal;
      const onAbort = () => {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        if (signal.reason !== undefined) error.cause = signal.reason;
        stream.destroy(error);
      };
      if (signal.aborted) setImmediate(onAbort);
      else signal.addEventListener("abort", onAbort, { once: true });
      stream.once("close", () => signal.removeEventListener("abort", onAbort));
    }
    if (!suppliedFd) beginOpen(settings.highWaterMark);
    if (suppliedFd) setImmediate(() => {
      if (!stream.destroyed) stream.emit("ready");
    });
    if (settings.encoding !== undefined) stream.setEncoding(settings.encoding);
    return stream;
  }

  Object.setPrototypeOf(ReadStream, Readable);
  Object.setPrototypeOf(ReadStream.prototype, Readable.prototype);
  ReadStream.prototype.constructor = ReadStream;

  return {
    ReadStream,
    createReadStream(path, options) {
      return ReadStream(path, options);
    },
  };
}"#;

const EXISTS_API: &str = r#"(statSync, accessHost, chmodHost, fchmodHost) => {
  const validatePath = (path) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${typeof path === 'string' ? `'${path}'` : String(path)})`;
    const error = new TypeError(`The "path" argument must be of type string or an instance of Buffer or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  const validatePathNullBytes = (path) => {
    let value = path;
    if (path instanceof URL) value = process.getBuiltinModule('url').fileURLToPath(path);
    else if (Buffer.isBuffer(path)) value = path.toString();
    if (typeof value === 'string' && value.includes('\0')) {
      const error = new TypeError('The "path" argument must be a string, Uint8Array, or URL without null bytes');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
  };
  const normalizePath = (path) =>
    typeof path === 'string' ? path : Buffer.isBuffer(path) ? path.toString() : path instanceof URL ? decodeURIComponent(path.pathname) : path;
  const normalizeMode = (mode) => {
    if (typeof mode === 'string') {
      const parsed = Number.parseInt(mode, 8);
      if (Number.isNaN(parsed) || !/^[0-7]+$/.test(mode)) {
        const error = new TypeError('The "mode" argument must be a valid integer');
        error.code = 'ERR_INVALID_ARG_VALUE';
        throw error;
      }
      mode = parsed;
    }
    if (typeof mode !== 'number') {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(mode) || !Number.isFinite(mode)) {
      const error = new RangeError(`The value of "mode" is out of range. It must be an integer. Received ${String(mode)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (mode < 0 || mode > 0xFFFFFFFF) {
      const error = new RangeError(`The value of "mode" is out of range. It must be >= 0 && <= 4294967295. Received ${mode}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    return mode;
  };
  const validateFd = (fd) => {
    if (typeof fd !== 'number') {
      const received = fd === null || fd === undefined
        ? ` Received ${fd}`
        : typeof fd === 'object'
          ? ` Received an instance of ${Array.isArray(fd) ? 'Array' : 'Object'}`
          : ` Received type ${typeof fd} (${typeof fd === 'string' ? `'${fd}'` : String(fd)})`;
      const error = new TypeError(`The "fd" argument must be of type number.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(fd) || !Number.isFinite(fd)) {
      const error = new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (fd < 0 || fd > 0x7FFFFFFF) {
      const error = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${fd}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  };
  function chmodSync(path, mode) {
    validatePath(path);
    validatePathNullBytes(path);
    return chmodHost(normalizePath(path), normalizeMode(mode));
  }
  function chmod(path, mode, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(path);
    validatePathNullBytes(path);
    const normalizedMode = normalizeMode(mode);
    queueMicrotask(() => {
      try {
        chmodHost(normalizePath(path), normalizedMode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  }
  function fchmodSync(fd, mode) {
    validateFd(fd);
    return fchmodHost(fd, normalizeMode(mode));
  }
  function fchmod(fd, mode, callback) {
    validateFd(fd);
    const normalizedMode = normalizeMode(mode);
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        fchmodHost(fd, normalizedMode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  }
  function accessSync(path, mode = 0) {
    validatePath(path);
    validatePathNullBytes(path);
    if (typeof mode !== 'number') {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isFinite(mode) || mode < 0 || mode > 7) {
      const error = new RangeError('mode is out of range');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    return accessHost(normalizePath(path), mode);
  }
  function existsSync(path) {
    try {
      statSync(path);
      return true;
    } catch {
      return false;
    }
  }
  function exists(path, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => callback(existsSync(path)));
  }
  exists[Symbol.for('nodejs.util.promisify.custom')] = (path) =>
    new Promise((resolve) => exists(path, resolve));
  function access(path, mode, callback) {
    if (typeof mode === 'function') {
      callback = mode;
      mode = 0;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(path);
    validatePathNullBytes(path);
    queueMicrotask(() => {
      try {
        accessSync(path, mode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  }
  return { access, accessSync, chmod, chmodSync, exists, existsSync, fchmod, fchmodSync };
}"#;

const ASYNC_MKDIR_API: &str = r#"(mkdirSync) => {
  const normalizePath = (path) =>
    typeof path === 'string' ? path : Buffer.isBuffer(path) ? path.toString() : path instanceof URL ? path.pathname : path;
  const validatePath = (path) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${String(path)})`;
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  const validateOptions = (options) => {
    if (options == null || typeof options === 'number' || typeof options === 'string') return;
    if (typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object or number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options.recursive !== undefined && typeof options.recursive !== 'boolean') {
      const value = options.recursive;
      const received = value === null ? ' Received null' : typeof value === 'object'
        ? ` Received an instance of ${Array.isArray(value) ? 'Array' : 'Object'}`
        : typeof value === 'function'
          ? ` Received function ${value.name}`
        : ` Received type ${typeof value} (${typeof value === 'string' ? `'${value}'` : String(value)})`;
      const error = new TypeError(`The "options.recursive" property must be of type boolean.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  return function mkdir(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(path);
    validateOptions(options);
    queueMicrotask(() => {
      try {
        const createdPath = mkdirSync(normalizePath(path), options);
        Reflect.apply(callback, undefined, [null, createdPath]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const ASYNC_RMDIR_API: &str = r#"(rmdirSync) => {
  return function rmdir(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
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
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        rmdirSync(path);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const ASYNC_MKDTEMP_API: &str = r#"(mkdtempSync) => {
  return function mkdtemp(prefix, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof prefix !== 'string' && !Buffer.isBuffer(prefix) && !(prefix instanceof URL) &&
        !(ArrayBuffer.isView(prefix) && !(prefix instanceof DataView))) {
      const error = new TypeError('The "prefix" argument must be of type string, Buffer, URL, or Uint8Array.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        Reflect.apply(callback, undefined, [null, mkdtempSync(prefix)]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const ASYNC_COPYFILE_API: &str = r#"(copyFileSync) => {
  return function copyFile(source, destination, mode, callback) {
    if (typeof mode === 'function') {
      callback = mode;
      mode = 0;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
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
    validatePath(source, 'src');
    validatePath(destination, 'dest');
    const validateNoNulPath = (path) => {
      if (Buffer.isBuffer(path)) path = path.toString();
      else if (path instanceof URL) path = decodeURIComponent(path.pathname);
      if (typeof path !== 'string' || !path.includes('\0')) return;
      const received = path.replace(/\0/g, '\\x00');
      const error = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${received}'`);
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    };
    validateNoNulPath(source);
    validateNoNulPath(destination);
    if (mode === undefined || mode === null) mode = 0;
    if (typeof mode !== 'number') {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(mode) || mode < 0 || mode > 7) {
      const error = new RangeError('The value of "mode" is out of range. It must be >= 0 and <= 7.');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        copyFileSync(source, destination, mode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const CP_API: &str = r#"(existsSync, statSync, lstatSync, readdirSync, mkdirSync, rmdirSync, chmodSync, unlinkSync, copyFileSync, symlinkSync, readlinkSync, realpathSync, utimesSync) => {
  const exists = (path) => {
    try { statSync(path); return true; } catch { return false; }
  };
  const fail = (code, message, path) => {
    const error = new Error(message);
    error.code = code;
    if (path !== undefined) error.path = path;
    throw error;
  };
  const normalizePath = (path) => path instanceof URL ? decodeURIComponent(path.pathname) : Buffer.isBuffer(path) ? path.toString() : path;
  const resolveSymlinkTarget = (source, target) => {
    if (/^(?:[A-Za-z]:[\\/]|[\\/]{1,2})/.test(target)) return target;
    const sourcePath = String(source);
    const separator = Math.max(sourcePath.lastIndexOf('/'), sourcePath.lastIndexOf('\\'));
    let base = separator < 0 ? '.' : sourcePath.slice(0, separator);
    if (base === '') base = '/';
    if (!/^(?:[A-Za-z]:[\\/]|[\\/]{1,2})/.test(base)) {
      base = `${process.cwd()}/${base}`;
    }
    const windows = /^[A-Za-z]:[\\/]/.test(base);
    const separatorChar = windows ? '\\' : '/';
    const prefix = windows ? base.slice(0, 3) : '/';
    const rest = windows ? base.slice(3) : base.slice(1);
    const parts = rest.split(/[\\/]+/).filter(Boolean);
    for (const part of target.split(/[\\/]+/)) {
      if (!part || part === '.') continue;
      if (part === '..') {
        if (parts.length > 0) parts.pop();
      } else {
        parts.push(part);
      }
    }
    return `${prefix}${parts.join(separatorChar)}`;
  };
  const validatePathLength = (path) => {
    const value = String(path);
    const tooLong = Buffer.byteLength(value) > 4096 || value.split(/[\\/]/).some(part => Buffer.byteLength(part) > 255);
    if (!tooLong) return;
    const error = new Error(`ENAMETOOLONG: name too long, stat '${value}'`);
    error.code = 'ENAMETOOLONG';
    error.errno = -36;
    error.syscall = 'stat';
    error.path = value;
    throw error;
  };
  const validateOptions = (options) => {
    if (options == null || typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options.mode !== undefined && (!Number.isInteger(options.mode) || options.mode < 0 || options.mode > 0o777)) {
      const error = new RangeError('The value of "mode" is out of range.');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    for (const name of ['recursive', 'force', 'errorOnExist', 'dereference', 'preserveTimestamps', 'verbatimSymlinks']) {
      if (Object.prototype.hasOwnProperty.call(options, name) && typeof options[name] !== 'boolean') {
        const error = new TypeError(`The "options.${name}" property must be of type boolean.`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    }
    if (options.filter !== undefined && typeof options.filter !== 'function') {
      const error = new TypeError('The "options.filter" property must be of type function.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  const samePath = (left, right) => String(left).replace(/[\\/]$/, '') === String(right).replace(/[\\/]$/, '');
  const isWithin = (parent, child) => {
    const prefix = `${String(parent).replace(/[\\/]$/, '')}/`;
    return String(child).startsWith(prefix);
  };
  const resolvedPath = (path) => {
    try { return realpathSync(path); } catch {
      const value = String(path);
      const separator = Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\'));
      if (separator < 0) return path;
      const parent = value.slice(0, separator) || value.slice(0, 1);
      try { return `${realpathSync(parent)}${value.slice(separator)}`; } catch { return path; }
    }
  };
  const invalidCopyTarget = (source, destination, dereference) => {
    if (!dereference && lstatSync(source).isSymbolicLink()) {
      if (!exists(destination) && !lstatExists(destination)) return undefined;
      if (!statSync(source).isDirectory()) return undefined;
      const resolvedSource = resolvedPath(source);
      const resolvedDestination = resolvedPath(destination);
      if (samePath(resolvedSource, resolvedDestination)) return 'ERR_FS_CP_EINVAL';
      if (isWithin(resolvedDestination, resolvedSource)) return 'ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY';
      return undefined;
    }
    const resolvedSource = resolvedPath(source);
    const resolvedDestination = resolvedPath(destination);
    return samePath(resolvedSource, resolvedDestination) || isWithin(resolvedSource, resolvedDestination)
      ? 'ERR_FS_CP_EINVAL'
      : undefined;
  };
  function cpSync(source, destination, options = {}) {
    source = normalizePath(source);
    destination = normalizePath(destination);
    validateOptions(options);
    if (typeof options.filter === 'function') {
      const accepted = options.filter(source, destination);
      if (accepted && typeof accepted.then === 'function') {
        const error = new TypeError('The "filter" function must return a boolean.');
        error.code = 'ERR_INVALID_RETURN_VALUE';
        throw error;
      }
      if (accepted === false) return undefined;
    }
    validatePathLength(source);
    validatePathLength(destination);
    if (options.dereference === true && options.verbatimSymlinks === true) {
      fail('ERR_INCOMPATIBLE_OPTION_PAIR', "Option 'dereference' and 'verbatimSymlinks' cannot be used together");
    }
    const invalidTarget = invalidCopyTarget(source, destination, options.dereference === true);
    if (invalidTarget) fail(invalidTarget, `Cannot copy '${source}' to a subdirectory of self '${destination}'`, destination);
    const sourceLstat = lstatSync(source);
    // Check socket file types before statSync. Some host backends surface a
    // socket's stat as ENXIO, while Node's fs.cp contract reports its own
    // stable ERR_FS_CP_SOCKET error.
    if (sourceLstat.isSocket()) fail('ERR_FS_CP_SOCKET', `Cannot copy socket '${source}'`);
    if (sourceLstat.isSymbolicLink() && options.dereference !== true) {
      let dest = destination;
      if (exists(destination) && !lstatSync(destination).isSymbolicLink() && statSync(destination).isDirectory()) {
        const sourceParts = String(source).split(/[\\/]/);
        dest = `${destination.replace(/[\\/]$/, '')}/${sourceParts[sourceParts.length - 1]}`;
      }
      if (exists(dest) || lstatExists(dest)) {
        if (!lstatSync(dest).isSymbolicLink()) fail('EEXIST', `EEXIST: file already exists, symlink '${dest}'`, dest);
        if (options.errorOnExist && options.force === false) {
          fail('ERR_FS_CP_EEXIST', `Target already exists: ${dest}`, dest);
        }
        if (options.force === false) return undefined;
        unlinkSync(dest);
      }
      let target = readlinkSync(source);
      if (options.verbatimSymlinks !== true && !/^(?:[A-Za-z]:[\\/]|[\\/]{1,2})/.test(target)) {
        target = resolveSymlinkTarget(source, target);
      }
      ensureParent(dest);
      symlinkSync(target, dest);
      return undefined;
    }
    const sourceStats = statSync(source);
    const sourceIsDereferencedSymlink =
      options.dereference === true && sourceLstat.isSymbolicLink();
    // When both source and destination resolve to directories, follow the
    // destination link and merge into its target. Only replace an existing
    // destination symlink when the source resolves to a non-directory.
    if (options.dereference === true && !sourceStats.isDirectory() &&
        lstatExists(destination) && lstatSync(destination).isSymbolicLink()) {
      if (options.force === false) {
        if (options.errorOnExist === true) fail('ERR_FS_CP_EEXIST', `Target already exists: ${destination}`, destination);
        return undefined;
      }
      unlinkSync(destination);
    }
    if (sourceStats.isSocket()) fail('ERR_FS_CP_SOCKET', `Cannot copy socket '${source}'`);
    if (sourceStats.isDirectory()) {
      if (exists(destination) && !statSync(destination).isDirectory()) {
        fail('ERR_FS_CP_DIR_TO_NON_DIR', `Cannot overwrite non-directory ${destination} with directory ${source}`);
      }
      if (exists(destination) && options.errorOnExist === true && options.force === false) {
        fail('ERR_FS_CP_EEXIST', `Target already exists: ${destination}`, destination);
      }
      if (options.recursive !== true) {
        fail('ERR_FS_EISDIR', `Recursive option not enabled, cannot copy a directory: ${source}`);
      }
      const destinationExisted = exists(destination);
      mkdirSync(destination, { recursive: true });
      for (const name of readdirSync(source)) {
        const src = `${source.replace(/[\\/]$/, '')}/${name}`;
        const dest = `${destination.replace(/[\\/]$/, '')}/${name}`;
        const childStats = lstatSync(src);
        // Node's sync walker ignores special files inside a directory tree;
        // only a socket passed as the top-level source throws.
        if (childStats.isSocket() || childStats.isFIFO()) continue;
        cpSync(src, dest, options);
      }
      if (options.preserveTimestamps) {
        utimesSync(destination, sourceStats.atime.getTime() / 1000, sourceStats.mtime.getTime() / 1000);
      }
      if (!destinationExisted) chmodSync(destination, sourceStats.mode & 0o777);
      return undefined;
    }
    let dest = destination;
    if (exists(destination) && lstatSync(destination).isSymbolicLink() && options.dereference !== true) {
      unlinkSync(destination);
    }
    if (exists(destination) && statSync(destination).isDirectory()) {
      if (sourceIsDereferencedSymlink) {
        if (options.force === false) {
          if (options.errorOnExist === true) {
            fail('ERR_FS_CP_EEXIST', `Target already exists: ${destination}`, destination);
          }
          return undefined;
        }
        if (readdirSync(destination).length === 0) {
          rmdirSync(destination);
        } else {
          fail('ERR_FS_CP_NON_DIR_TO_DIR', `Cannot overwrite directory ${destination} with non-directory ${source}`);
        }
      } else {
        fail('ERR_FS_CP_NON_DIR_TO_DIR', `Cannot overwrite directory with non-directory: ${destination}`);
      }
    }
    if (exists(dest)) {
      if (options.errorOnExist && options.force === false) {
        fail('ERR_FS_CP_EEXIST', `Target already exists: ${dest}`, dest);
      }
      if (options.force === false) return undefined;
    }
    if ((options.mode & 4) !== 0) {
      const error = new Error(`ENOTSUP: operation not supported, copyfile '${source}' -> '${dest}'`);
      error.errno = -95;
      error.code = 'ENOTSUP';
      error.syscall = 'copyfile';
      error.path = source;
      error.dest = dest;
      throw error;
    }
    ensureParent(dest);
    try {
      copyFileSync(source, dest, options.mode || 0);
    } catch (error) {
      // Preserve the Node copyFile error shape even when the host operation
      // only provides a code and message for reflink or platform failures.
      if (error && typeof error === 'object') {
        if (error.syscall === undefined) error.syscall = 'copyfile';
        if (error.path === undefined) error.path = source;
        if (error.dest === undefined) error.dest = dest;
      }
      throw error;
    }
    if (options.preserveTimestamps) {
      utimesSync(dest, sourceStats.atime.getTime() / 1000, sourceStats.mtime.getTime() / 1000);
    }
    return undefined;
  }
  function lstatExists(path) {
    try { lstatSync(path); return true; } catch { return false; }
  }
  function ensureParent(path) {
    const parent = String(path).replace(/[\\/][^\\/]*$/, '');
    if (parent && !exists(parent)) mkdirSync(parent, { recursive: true });
  }
  function cp(source, destination, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = {};
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    source = normalizePath(source);
    destination = normalizePath(destination);
    validateOptions(options === undefined ? {} : options);
    if (options && options.dereference === true && options.verbatimSymlinks === true) {
      const error = new Error("Option 'dereference' and 'verbatimSymlinks' cannot be used together");
      error.code = 'ERR_INCOMPATIBLE_OPTION_PAIR';
      throw error;
    }
    const destinationExistedAtStart = lstatExists(destination) || exists(destination);
    const AsyncResource = process.getBuiltinModule?.('async_hooks')?.AsyncResource;
    const request = typeof AsyncResource === 'function'
      ? new AsyncResource('FSREQCALLBACK')
      : undefined;
    const copy = async (src, dest, opts) => {
      validateOptions(opts);
      if (typeof opts.filter === 'function' && await opts.filter(src, dest) === false) return;
      validatePathLength(src);
      validatePathLength(dest);
      const invalidTarget = invalidCopyTarget(src, dest, opts.dereference === true);
      if (invalidTarget) fail(invalidTarget, `Cannot copy '${src}' to a subdirectory of self '${dest}'`, dest);
      const sourceLstat = lstatSync(src);
      if (sourceLstat.isSocket()) fail('ERR_FS_CP_SOCKET', `Cannot copy socket '${src}'`);
      if (sourceLstat.isFIFO()) fail('ERR_FS_CP_FIFO_PIPE', `Cannot copy FIFO pipe '${src}'`);
      const stats = opts.dereference === true ? statSync(src) : sourceLstat;
      if (stats.isSymbolicLink() && opts.dereference !== true) {
        cpSync(src, dest, { ...opts, filter: undefined });
        return;
      }
      if (stats.isDirectory()) {
        if (exists(dest) && !statSync(dest).isDirectory()) fail('ERR_FS_CP_DIR_TO_NON_DIR', `Cannot overwrite non-directory ${dest} with directory ${src}`);
        if (exists(dest) && opts.errorOnExist === true && opts.force === false) fail('ERR_FS_CP_EEXIST', `Target already exists: ${dest}`, dest);
        if (opts.recursive !== true) fail('ERR_FS_EISDIR', `Recursive option not enabled, cannot copy a directory: ${src}`);
        if (src === source && !destinationExistedAtStart && lstatExists(dest)) {
          const error = new Error(`EEXIST: file already exists, mkdir '${dest}'`);
          error.code = 'EEXIST';
          error.errno = -17;
          error.syscall = 'mkdir';
          error.path = dest;
          throw error;
        }
        const destinationExisted = exists(dest);
        mkdirSync(dest, { recursive: true });
        for (const name of readdirSync(src)) {
          await copy(`${String(src).replace(/[\\/]$/, '')}/${name}`, `${String(dest).replace(/[\\/]$/, '')}/${name}`, opts);
        }
        if (opts.preserveTimestamps) {
          utimesSync(dest, stats.atime.getTime() / 1000, stats.mtime.getTime() / 1000);
        }
        if (!destinationExisted) chmodSync(dest, stats.mode & 0o777);
        return;
      }
      cpSync(src, dest, { ...opts, filter: undefined });
    };
    queueMicrotask(async () => {
      let error;
      try { await copy(source, destination, options || {}); }
      catch (caught) { error = caught; }
      try {
        if (request) request.runInAsyncScope(callback, undefined, error || null);
        else Reflect.apply(callback, undefined, [error || null]);
      } finally {
        request?.emitDestroy();
      }
    });
  }
  const cpPromise = (source, destination, options) => new Promise((resolve, reject) => {
    if (options === undefined) options = {};
    validateOptions(options);
    if (options.dereference === true && options.verbatimSymlinks === true) {
      const error = new Error("Option 'dereference' and 'verbatimSymlinks' cannot be used together");
      error.code = 'ERR_INCOMPATIBLE_OPTION_PAIR';
      throw error;
    }
    cp(source, destination, options, (error) => error ? reject(error) : resolve());
  });
  return { cp, cpSync, cpPromise };
}"#;

const ASYNC_SYMLINK_API: &str = r#"(symlinkSync) => {
  const validatePath = (value, name) => {
    if (typeof value === 'string' || Buffer.isBuffer(value) || value instanceof URL) return;
    const received = value === null || value === undefined
      ? ` Received ${value}`
      : typeof value === 'object'
        ? ` Received an instance of ${Array.isArray(value) ? 'Array' : 'Object'}`
        : ` Received type ${typeof value} (${String(value)})`;
    const error = new TypeError(`The "${name}" argument must be of type string, Buffer, or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  const validateNoNulPath = (value, name) => {
    let path = value;
    if (Buffer.isBuffer(value)) path = value.toString();
    else if (value instanceof URL) {
      // Decode the URL path before the OS path conversion; fileURLToPath()
      // may surface an EIO for an encoded NUL, while Node rejects it as an
      // invalid argument synchronously.
      try { path = decodeURIComponent(value.pathname); }
      catch { path = value.pathname; }
    }
    if (typeof path === 'string' && path.includes('\0')) {
      const error = new TypeError(`The "${name}" argument must be a string, Uint8Array, or URL without null bytes`);
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
  };
  return function symlink(target, path, type, callback) {
    if (typeof type === 'function') {
      callback = type;
      type = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(target, 'target');
    validatePath(path, 'path');
    validateNoNulPath(target, 'target');
    validateNoNulPath(path, 'path');
    if (type !== undefined && type !== 'file' && type !== 'dir' && type !== 'junction') {
      const error = new TypeError('The "type" argument must be one of: "dir", "file", "junction".');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    queueMicrotask(() => {
      try { symlinkSync(target, path, type); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_LINK_API: &str = r#"(linkSync) => {
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
    let value = path;
    if (path instanceof URL) value = process.getBuiltinModule('url').fileURLToPath(path);
    else if (Buffer.isBuffer(path)) value = path.toString();
    if (typeof value === 'string' && value.includes('\0')) {
      const error = new TypeError('The "path" argument must be a string, Uint8Array, or URL without null bytes');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
  };
  return function link(existingPath, newPath, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(existingPath, 'existingPath');
    validatePath(newPath, 'newPath');
    validateNoNulPath(existingPath);
    validateNoNulPath(newPath);
    queueMicrotask(() => {
      try { linkSync(existingPath, newPath); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_RENAME_API: &str = r#"(renameSync) => {
  const validatePath = (path, name) => {
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
  return function rename(oldPath, newPath, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(oldPath, 'oldPath');
    validatePath(newPath, 'newPath');
    queueMicrotask(() => {
      try { renameSync(oldPath, newPath); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_UNLINK_API: &str = r#"(unlinkSync) => {
  return function unlink(path, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { unlinkSync(path); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_OWNER_API: &str = r#"(ownerSync, descriptor) => {
  const validateOwner = (name, value) => {
    if (typeof value !== 'number') {
      const error = new TypeError(`The "${name}" argument must be of type number.`);
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
  const validateDescriptor = (fd) => {
    if (typeof fd !== 'number') {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(fd)) {
      const error = new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (fd < 0 || fd > 0x7FFFFFFF) {
      const error = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  };
  return function owner(path, uid, gid, callback) {
    if (descriptor) {
      validateDescriptor(path);
      validateOwner('uid', uid);
      validateOwner('gid', gid);
      if (typeof callback !== 'function') {
        const error = new TypeError('The "cb" argument must be of type function');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    } else {
      if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
        const error = new TypeError(`The "path" argument must be of type string or an instance of Buffer or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      validateOwner('uid', uid);
      validateOwner('gid', gid);
      if (typeof callback !== 'function') {
        const error = new TypeError('The "cb" argument must be of type function');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    }
    queueMicrotask(() => {
      try { ownerSync(path, uid, gid); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_LSTAT_API: &str = r#"(lstatSync, toPathIfFileURL) => {
  return function lstat(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    path = toPathIfFileURL(path);
    if (typeof path !== 'string' && !Buffer.isBuffer(path)) {
      const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (Buffer.isBuffer(path)) path = path.toString();
    queueMicrotask(() => {
      let stats;
      try { stats = lstatSync(path, options); }
      catch (error) { Reflect.apply(callback, undefined, [error]); return; }
      Reflect.apply(callback, undefined, [null, stats]);
    });
  };
}"#;

const ASYNC_STAT_API: &str = r#"(statSync, toPathIfFileURL) => {
  return function stat(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    path = toPathIfFileURL(path);
    if (typeof path !== 'string' && !Buffer.isBuffer(path)) {
      const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (Buffer.isBuffer(path)) path = path.toString();
    process.nextTick(() => {
      let stats;
      try { stats = statSync(path, options); }
      catch (error) { Reflect.apply(callback, undefined, [error]); return; }
      Reflect.apply(callback, undefined, [null, stats]);
    });
  };
}"#;

const STAT_SYNC_API: &str = r#"(hostStatSync) => function wrappedStatSync(path, options = undefined) {
  return hostStatSync(path, options);
}"#;

const STATFS_SYNC_API: &str = r#"(hostStatfsSync) => function statfsSync(path, options = undefined) {
  return hostStatfsSync(path, options);
}"#;

const ASYNC_STATFS_API: &str = r#"(statfsSync, toPathIfFileURL) => function statfs(path, options, callback) {
  if (typeof options === 'function') {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== 'function') {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  path = toPathIfFileURL(path);
  if (typeof path !== 'string' && !Buffer.isBuffer(path)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  queueMicrotask(() => {
    let stats;
    try { stats = statfsSync(path, options); }
    catch (error) { Reflect.apply(callback, undefined, [error]); return; }
    Reflect.apply(callback, undefined, [null, stats]);
  });
}"#;

const ASYNC_TRUNCATE_API: &str = r#"(truncateSync) => function truncate(path, length, callback) {
  if (typeof length === 'function') {
    callback = length;
    length = undefined;
  }
  if (length !== undefined && typeof length !== 'number') {
    const received = length === null ? ' Received null' : typeof length === 'object'
      ? ` Received an instance of ${Array.isArray(length) ? 'Array' : 'Object'}`
      : ` Received type ${typeof length} (${typeof length === 'string' ? `'${length}'` : String(length)})`;
    const error = new TypeError(`The "len" argument must be of type number.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (typeof length === 'number' && !Number.isSafeInteger(length)) {
    const error = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${String(length)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (typeof callback !== 'function') {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  queueMicrotask(() => {
    try { truncateSync(path, length); Reflect.apply(callback, undefined, [null]); }
    catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#;

const ASYNC_FTRUNCATE_API: &str = r#"(ftruncateSync) => function ftruncate(fd, length, callback) {
  if (typeof length === 'function') {
    callback = length;
    length = undefined;
  }
  if (typeof fd !== 'number') {
    const received = fd === null || fd === undefined ? ` Received ${fd}` : typeof fd === 'object'
      ? ` Received an instance of ${Array.isArray(fd) ? 'Array' : 'Object'}`
      : ` Received type ${typeof fd} (${typeof fd === 'string' ? `'${fd}'` : String(fd)})`;
    const error = new TypeError(`The "fd" argument must be of type number.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (!Number.isInteger(fd)) {
    const error = new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${String(fd)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (fd < 0 || fd > 0x7FFFFFFF) {
    const error = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${String(fd)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (length !== undefined && typeof length !== 'number') {
    const received = length === null ? ' Received null' : typeof length === 'object'
      ? ` Received an instance of ${Array.isArray(length) ? 'Array' : 'Object'}`
      : ` Received type ${typeof length} (${typeof length === 'string' ? `'${length}'` : String(length)})`;
    const error = new TypeError(`The "len" argument must be of type number.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (typeof length === 'number' && !Number.isSafeInteger(length)) {
    const error = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${String(length)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (typeof callback !== 'function') {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  queueMicrotask(() => {
    try { ftruncateSync(fd, length); Reflect.apply(callback, undefined, [null]); }
    catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#;

const READLINK_SYNC_API: &str = r#"(hostReadlinkSync) => function readlinkSync(path, options) {
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = decodeURIComponent(path.pathname);
  return hostReadlinkSync(path);
}"#;

const ASYNC_READLINK_API: &str = r#"(readlinkSync) => {
  return function readlink(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (Buffer.isBuffer(path)) path = path.toString();
    else if (path instanceof URL) path = decodeURIComponent(path.pathname);
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, readlinkSync(path)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const UV_FS_SYMLINK_DIR: i32 = 1;
const UV_FS_SYMLINK_JUNCTION: i32 = 2;
pub(super) const UV_DIRENT_UNKNOWN: i32 = 0;
pub(super) const UV_DIRENT_FILE: i32 = 1;
pub(super) const UV_DIRENT_DIR: i32 = 2;
pub(super) const UV_DIRENT_LINK: i32 = 3;
pub(super) const UV_DIRENT_FIFO: i32 = 4;
pub(super) const UV_DIRENT_SOCKET: i32 = 5;
pub(super) const UV_DIRENT_CHAR: i32 = 6;
pub(super) const UV_DIRENT_BLOCK: i32 = 7;
const UV_FS_O_FILEMAP: i32 = 0;
const UV_FS_COPYFILE_EXCL: i32 = 1;
const UV_FS_COPYFILE_FICLONE: i32 = 2;
const UV_FS_COPYFILE_FICLONE_FORCE: i32 = 4;
#[cfg(not(target_os = "linux"))]
const S_IRUSR: i32 = 0o400;
#[cfg(not(target_os = "linux"))]
const S_IWUSR: i32 = 0o200;

#[cfg(target_os = "linux")]
const FS_CONSTANT_VALUES: &[(&str, i32)] = &[
    ("UV_FS_SYMLINK_DIR", UV_FS_SYMLINK_DIR),
    ("UV_FS_SYMLINK_JUNCTION", UV_FS_SYMLINK_JUNCTION),
    ("O_RDONLY", libc::O_RDONLY),
    ("O_WRONLY", libc::O_WRONLY),
    ("O_RDWR", libc::O_RDWR),
    ("UV_DIRENT_UNKNOWN", UV_DIRENT_UNKNOWN),
    ("UV_DIRENT_FILE", UV_DIRENT_FILE),
    ("UV_DIRENT_DIR", UV_DIRENT_DIR),
    ("UV_DIRENT_LINK", UV_DIRENT_LINK),
    ("UV_DIRENT_FIFO", UV_DIRENT_FIFO),
    ("UV_DIRENT_SOCKET", UV_DIRENT_SOCKET),
    ("UV_DIRENT_CHAR", UV_DIRENT_CHAR),
    ("UV_DIRENT_BLOCK", UV_DIRENT_BLOCK),
    ("S_IFMT", libc::S_IFMT as i32),
    ("S_IFREG", libc::S_IFREG as i32),
    ("S_IFDIR", libc::S_IFDIR as i32),
    ("S_IFCHR", libc::S_IFCHR as i32),
    ("S_IFBLK", libc::S_IFBLK as i32),
    ("S_IFIFO", libc::S_IFIFO as i32),
    ("S_IFLNK", libc::S_IFLNK as i32),
    ("S_IFSOCK", libc::S_IFSOCK as i32),
    ("O_CREAT", libc::O_CREAT),
    ("O_EXCL", libc::O_EXCL),
    ("UV_FS_O_FILEMAP", UV_FS_O_FILEMAP),
    ("O_NOCTTY", libc::O_NOCTTY),
    ("O_TRUNC", libc::O_TRUNC),
    ("O_APPEND", libc::O_APPEND),
    ("O_DIRECTORY", libc::O_DIRECTORY),
    ("O_NOATIME", libc::O_NOATIME),
    ("O_NOFOLLOW", libc::O_NOFOLLOW),
    ("O_SYNC", libc::O_SYNC),
    ("O_DSYNC", libc::O_DSYNC),
    ("O_DIRECT", libc::O_DIRECT),
    ("O_NONBLOCK", libc::O_NONBLOCK),
    ("S_IRWXU", libc::S_IRWXU as i32),
    ("S_IRUSR", libc::S_IRUSR as i32),
    ("S_IWUSR", libc::S_IWUSR as i32),
    ("S_IXUSR", libc::S_IXUSR as i32),
    ("S_IRWXG", libc::S_IRWXG as i32),
    ("S_IRGRP", libc::S_IRGRP as i32),
    ("S_IWGRP", libc::S_IWGRP as i32),
    ("S_IXGRP", libc::S_IXGRP as i32),
    ("S_IRWXO", libc::S_IRWXO as i32),
    ("S_IROTH", libc::S_IROTH as i32),
    ("S_IWOTH", libc::S_IWOTH as i32),
    ("S_IXOTH", libc::S_IXOTH as i32),
    ("F_OK", libc::F_OK),
    ("R_OK", libc::R_OK),
    ("W_OK", libc::W_OK),
    ("X_OK", libc::X_OK),
    ("UV_FS_COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("UV_FS_COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("UV_FS_COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
    ("COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
];

#[cfg(not(target_os = "linux"))]
const FS_CONSTANT_VALUES: &[(&str, i32)] = &[
    ("UV_FS_SYMLINK_DIR", UV_FS_SYMLINK_DIR),
    ("UV_FS_SYMLINK_JUNCTION", UV_FS_SYMLINK_JUNCTION),
    ("UV_DIRENT_UNKNOWN", UV_DIRENT_UNKNOWN),
    ("UV_DIRENT_FILE", UV_DIRENT_FILE),
    ("UV_DIRENT_DIR", UV_DIRENT_DIR),
    ("UV_DIRENT_LINK", UV_DIRENT_LINK),
    ("UV_DIRENT_FIFO", UV_DIRENT_FIFO),
    ("UV_DIRENT_SOCKET", UV_DIRENT_SOCKET),
    ("UV_DIRENT_CHAR", UV_DIRENT_CHAR),
    ("UV_DIRENT_BLOCK", UV_DIRENT_BLOCK),
    ("S_IRUSR", S_IRUSR),
    ("S_IWUSR", S_IWUSR),
    ("UV_FS_O_FILEMAP", UV_FS_O_FILEMAP),
    ("UV_FS_COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("UV_FS_COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("UV_FS_COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
    ("COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
];

pub(crate) fn fs_constants(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0builtin:fs/constants";
    const DIRENT_TYPES: &[(&str, i32)] = &[
        ("UV_DIRENT_UNKNOWN", 0),
        ("UV_DIRENT_FILE", 1),
        ("UV_DIRENT_DIR", 2),
        ("UV_DIRENT_LINK", 3),
        ("UV_DIRENT_FIFO", 4),
        ("UV_DIRENT_SOCKET", 5),
        ("UV_DIRENT_CHAR", 6),
        ("UV_DIRENT_BLOCK", 7),
    ];
    let shared = context.host_mut().shared_state();
    if let Some(constants) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(constants);
    }
    let constants = context.null_object_rooted()?;
    for &(name, value) in FS_CONSTANT_VALUES.iter().chain(DIRENT_TYPES.iter()) {
        let key = context.string_rooted(name);
        let value = context.number(value as f64);
        context.define_data_property_rooted(constants, key, value, false, true, false)?;
    }
    let retained = context.retain(constants)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), retained);
    Ok(constants)
}

fn make_readdir_sync(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.host_function(crate::host::shared_vm::operation("fsReaddirPolicy"))
}

/// Existing filesystem adapters retain this indirection during module bootstrap.
/// Once initialized, every captured reader uses the same configured SDK policy.
pub(crate) fn readdir_policy(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let policy = directory_policy(context)?;
    let function = get(context, policy, "readdirSync")?;
    let undefined = context.undefined();
    context.call_rooted(function, undefined, args)
}

fn make_readdir_async(
    context: &mut NativeContext<'_, NodeHost>,
    readdir_sync: RootId,
) -> Result<RootId, RootedError> {
    let factory =
        context.evaluate_script_rooted(ASYNC_READDIR_FACTORY, "node:fs/shared-async-readdir.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[readdir_sync])
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    super::fs_watch_shared_vm::install(context, module)?;
    let opendir_promise;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    set(context, module, "readFileSync", read_file)?;
    let read_file_factory = context
        .evaluate_script_rooted(ASYNC_READ_FILE_FACTORY, "node:fs/shared-async-read-file.js")?;
    let validate =
        context.host_function(crate::host::shared_vm::operation("fsValidateAbortSignal"))?;
    let complete =
        context.host_function(crate::host::shared_vm::operation("fsReadFileComplete"))?;
    let to_path_if_file_url =
        context.host_function(crate::host::shared_vm::operation("toPathIfFileURL"))?;
    let undefined = context.undefined();
    let read_file_async = context.call_rooted(
        read_file_factory,
        undefined,
        &[read_file, validate, complete, to_path_if_file_url],
    )?;
    set(context, module, "readFile", read_file_async)?;
    let stat_host = context.host_function(crate::host::shared_vm::operation("fsStatSync"))?;
    let stat_factory =
        context.evaluate_script_rooted(STAT_SYNC_API, "node:fs/shared-stat-sync.js")?;
    let undefined = context.undefined();
    let stat_continuation = context.call_rooted(stat_factory, undefined, &[stat_host])?;
    let stat_sync = metadata_public::wrap(context, stat_continuation, "statSync")?;
    set(context, module, "statSync", stat_sync)?;
    let statfs_host = context.host_function(crate::host::shared_vm::operation("fsStatfsSync"))?;
    let statfs_factory =
        context.evaluate_script_rooted(STATFS_SYNC_API, "node:fs/shared-statfs-sync.js")?;
    let statfs_continuation = context.call_rooted(statfs_factory, undefined, &[statfs_host])?;
    let statfs_sync = metadata_public::wrap(context, statfs_continuation, "statfsSync")?;
    set(context, module, "statfsSync", statfs_sync)?;
    let statfs_factory =
        context.evaluate_script_rooted(ASYNC_STATFS_API, "node:fs/shared-async-statfs.js")?;
    let statfs = context.call_rooted(
        statfs_factory,
        undefined,
        &[statfs_continuation, to_path_if_file_url],
    )?;
    set(context, module, "statfs", statfs)?;
    let exists_factory = context.evaluate_script_rooted(EXISTS_API, "node:fs/shared-exists.js")?;
    let undefined = context.undefined();
    let access_host = context.host_function(crate::host::shared_vm::operation("fsAccessSync"))?;
    let chmod_host = context.host_function(crate::host::shared_vm::operation("fsChmodSync"))?;
    let fchmod_host = context.host_function(crate::host::shared_vm::operation("fsFchmodSync"))?;
    let exists_api = context.call_rooted(
        exists_factory,
        undefined,
        // existsSync falls through to the filesystem, not another VFS dispatch.
        &[stat_continuation, access_host, chmod_host, fchmod_host],
    )?;
    let exists = get(context, exists_api, "exists")?;
    let exists_sync = get(context, exists_api, "existsSync")?;
    let exists_sync = context.host_function_with_data(
        crate::host::shared_vm::operation("fsVfsExistsSync"),
        exists_sync,
    )?;
    let name_key = context.string_rooted("name");
    let name_value = context.string_rooted("existsSync");
    context.define_data_property_rooted(exists_sync, name_key, name_value, false, false, true)?;
    let access = get(context, exists_api, "access")?;
    let access_sync = get(context, exists_api, "accessSync")?;
    let access_sync = metadata_public::wrap(context, access_sync, "accessSync")?;
    let chmod = get(context, exists_api, "chmod")?;
    let chmod_sync = get(context, exists_api, "chmodSync")?;
    let chmod_sync = metadata_public::wrap(context, chmod_sync, "chmodSync")?;
    let fchmod = get(context, exists_api, "fchmod")?;
    let fchmod_sync = get(context, exists_api, "fchmodSync")?;
    let fchmod_sync = metadata_public::wrap(context, fchmod_sync, "fchmodSync")?;
    set(context, module, "exists", exists)?;
    set(context, module, "existsSync", exists_sync)?;
    set(context, module, "access", access)?;
    set(context, module, "accessSync", access_sync)?;
    set(context, module, "chmod", chmod)?;
    set(context, module, "chmodSync", chmod_sync)?;
    set(context, module, "fchmod", fchmod)?;
    set(context, module, "fchmodSync", fchmod_sync)?;
    let lstat_host = context.host_function(crate::host::shared_vm::operation("fsLstatSync"))?;
    let lstat_factory =
        context.evaluate_script_rooted(STAT_SYNC_API, "node:fs/shared-lstat-sync.js")?;
    let undefined = context.undefined();
    let lstat_sync = context.call_rooted(lstat_factory, undefined, &[lstat_host])?;
    let lstat_sync = metadata_public::wrap(context, lstat_sync, "lstatSync")?;
    set(context, module, "lstatSync", lstat_sync)?;
    let lstat_factory =
        context.evaluate_script_rooted(ASYNC_LSTAT_API, "node:fs/shared-async-lstat.js")?;
    let lstat =
        context.call_rooted(lstat_factory, undefined, &[lstat_sync, to_path_if_file_url])?;
    set(context, module, "lstat", lstat)?;
    let readdir_sync = make_readdir_sync(context)?;
    set(context, module, "readdirSync", readdir_sync)?;
    let readdir = make_readdir_async(context, readdir_sync)?;
    set(context, module, "readdir", readdir)?;
    let opendir_factory =
        context.evaluate_script_rooted(OPENDIR_API, "node:fs/shared-opendir.js")?;
    let opendir_api = context.call_rooted(opendir_factory, undefined, &[readdir_sync])?;
    let dir_constructor = get(context, opendir_api, "Dir")?;
    let opendir = get(context, opendir_api, "opendir")?;
    let opendir_sync = get(context, opendir_api, "opendirSync")?;
    opendir_promise = get(context, opendir_api, "opendirPromise")?;
    set(context, module, "Dir", dir_constructor)?;
    set(context, module, "opendir", opendir)?;
    set(context, module, "opendirSync", opendir_sync)?;
    let readlink_host =
        context.host_function(crate::host::shared_vm::operation("fsReadlinkSync"))?;
    let readlink_factory =
        context.evaluate_script_rooted(READLINK_SYNC_API, "node:fs/shared-readlink-sync.js")?;
    let undefined = context.undefined();
    let readlink_sync = context.call_rooted(readlink_factory, undefined, &[readlink_host])?;
    let readlink_sync = metadata_public::wrap(context, readlink_sync, "readlinkSync")?;
    set(context, module, "readlinkSync", readlink_sync)?;
    let readlink_factory =
        context.evaluate_script_rooted(ASYNC_READLINK_API, "node:fs/shared-async-readlink.js")?;
    let readlink = context.call_rooted(readlink_factory, undefined, &[readlink_sync])?;
    set(context, module, "readlink", readlink)?;
    let realpath_host =
        context.host_function(crate::host::shared_vm::operation("fsRealpathSync"))?;
    let realpath_sync_factory =
        context.evaluate_script_rooted(REALPATH_SYNC_API, "node:fs/shared-realpath-sync.js")?;
    let undefined = context.undefined();
    let realpath_sync = context.call_rooted(realpath_sync_factory, undefined, &[realpath_host])?;
    let realpath_sync =
        super::fs_policy_shared_vm::wrap_vfs_pair(context, realpath_sync, "realpathSync")?;
    let realpath_sync_native = get(context, realpath_sync, "native")?;
    set(context, module, "realpathSync", realpath_sync)?;
    let key = context.string_rooted("__quenchInternalRealpathSyncNative");
    context.define_data_property_rooted(module, key, realpath_sync_native, false, false, true)?;
    let realpath_factory =
        context.evaluate_script_rooted(REALPATH_FACTORY, "node:fs/shared-realpath.js")?;
    let undefined = context.undefined();
    let realpath = context.call_rooted(realpath_factory, undefined, &[realpath_sync])?;
    let realpath_native = get(context, realpath, "native")?;
    set(context, module, "realpath", realpath)?;
    let key = context.string_rooted("__quenchInternalRealpathNative");
    context.define_data_property_rooted(module, key, realpath_native, false, false, true)?;
    let constants = fs_constants(context)?;
    let key = context.string_rooted("constants");
    context.define_data_property_rooted(module, key, constants, false, true, false)?;
    stat::install(context, module)?;
    super::fs_stat_watch_shared_vm::install(context, module)?;
    let rm = context.host_function(crate::host::shared_vm::operation("fsRm"))?;
    let factory = context.evaluate_script_rooted("native => function rm(path, options, callback) { 'use strict'; return native(path, options, callback); }", "node:fs/rm-facade.js")?;
    let undefined = context.undefined();
    let rm = context.call_rooted(factory, undefined, &[rm])?;
    set(context, module, "rm", rm)?;
    sync::install(context, module)?;
    for name in [
        "fstatSync",
        "closeSync",
        "renameSync",
        "linkSync",
        "copyFileSync",
        "rmSync",
        "rmdirSync",
        "unlinkSync",
        "symlinkSync",
        "truncateSync",
        "mkdtempSync",
        "chownSync",
        "lchownSync",
        "utimesSync",
        "fchownSync",
        "ftruncateSync",
        "fsyncSync",
        "fdatasyncSync",
    ] {
        let original = get(context, module, name)?;
        let function = metadata_public::wrap(context, original, name)?;
        set(context, module, name, function)?;
    }
    // SDK policy runs before data/path conversion. Replace the legacy public
    // byte shim before callbacks, promises and cp capture these functions.
    for (name, operation) in [
        ("writeFileSync", "fsWriteFilePublic"),
        ("appendFileSync", "fsAppendFilePublic"),
    ] {
        let function =
            context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        set(context, module, name, function)?;
    }
    let mkdir_sync = get(context, module, "mkdirSync")?;
    let mkdir_sync = super::fs_policy_shared_vm::wrap_vfs_pair(context, mkdir_sync, "mkdirSync")?;
    set(context, module, "mkdirSync", mkdir_sync)?;
    let fsync_sync = get(context, module, "fsyncSync")?;
    let fdatasync_sync = get(context, module, "fdatasyncSync")?;
    let async_sync_factory =
        context.evaluate_script_rooted(ASYNC_SYNC_API, "node:fs/shared-async-sync.js")?;
    let async_sync =
        context.call_rooted(async_sync_factory, undefined, &[fsync_sync, fdatasync_sync])?;
    let fsync_async = get(context, async_sync, "fsync")?;
    let fdatasync_async = get(context, async_sync, "fdatasync")?;
    set(context, module, "fsync", fsync_async)?;
    set(context, module, "fdatasync", fdatasync_async)?;
    let write_sync = get(context, module, "writeSync")?;
    let write_factory = context.evaluate_script_rooted(WRITE_API, "node:fs/shared-write.js")?;
    let write = context.call_rooted(write_factory, undefined, &[write_sync])?;
    set(context, module, "write", write)?;
    let writev_factory = context.evaluate_script_rooted(WRITEV_API, "node:fs/shared-writev.js")?;
    let writev_api = context.call_rooted(writev_factory, undefined, &[write_sync])?;
    let writev = get(context, writev_api, "writev")?;
    let writev_sync = get(context, writev_api, "writevSync")?;
    set(context, module, "writev", writev)?;
    set(context, module, "writevSync", writev_sync)?;
    let readv_read_sync = get(context, module, "readSync")?;
    let readv_factory = context.evaluate_script_rooted(READV_API, "node:fs/shared-readv.js")?;
    let readv_api = context.call_rooted(readv_factory, undefined, &[readv_read_sync])?;
    let readv = get(context, readv_api, "readv")?;
    let readv_sync = get(context, readv_api, "readvSync")?;
    set(context, module, "readv", readv)?;
    set(context, module, "readvSync", readv_sync)?;
    let truncate_sync = get(context, module, "truncateSync")?;
    let truncate_factory =
        context.evaluate_script_rooted(ASYNC_TRUNCATE_API, "node:fs/shared-async-truncate.js")?;
    let truncate = context.call_rooted(truncate_factory, undefined, &[truncate_sync])?;
    set(context, module, "truncate", truncate)?;
    let ftruncate_sync = get(context, module, "ftruncateSync")?;
    let ftruncate_factory =
        context.evaluate_script_rooted(ASYNC_FTRUNCATE_API, "node:fs/shared-async-ftruncate.js")?;
    let ftruncate = context.call_rooted(ftruncate_factory, undefined, &[ftruncate_sync])?;
    set(context, module, "ftruncate", ftruncate)?;
    let stat_factory =
        context.evaluate_script_rooted(ASYNC_STAT_API, "node:fs/shared-async-stat.js")?;
    let undefined = context.undefined();
    let stat = context.call_rooted(stat_factory, undefined, &[stat_sync, to_path_if_file_url])?;
    set(context, module, "stat", stat)?;
    let mkdir_sync = get(context, module, "mkdirSync")?;
    let mkdir_factory =
        context.evaluate_script_rooted(ASYNC_MKDIR_API, "node:fs/shared-async-mkdir.js")?;
    let undefined = context.undefined();
    let mkdir = context.call_rooted(mkdir_factory, undefined, &[mkdir_sync])?;
    set(context, module, "mkdir", mkdir)?;
    let rmdir_sync = get(context, module, "rmdirSync")?;
    let rmdir_factory =
        context.evaluate_script_rooted(ASYNC_RMDIR_API, "node:fs/shared-async-rmdir.js")?;
    let rmdir = context.call_rooted(rmdir_factory, undefined, &[rmdir_sync])?;
    set(context, module, "rmdir", rmdir)?;
    let mkdtemp_sync = get(context, module, "mkdtempSync")?;
    let mkdtemp_factory =
        context.evaluate_script_rooted(ASYNC_MKDTEMP_API, "node:fs/shared-async-mkdtemp.js")?;
    let mkdtemp = context.call_rooted(mkdtemp_factory, undefined, &[mkdtemp_sync])?;
    set(context, module, "mkdtemp", mkdtemp)?;
    let copy_file_sync = get(context, module, "copyFileSync")?;
    let copy_file_factory =
        context.evaluate_script_rooted(ASYNC_COPYFILE_API, "node:fs/shared-async-copyfile.js")?;
    let copy_file = context.call_rooted(copy_file_factory, undefined, &[copy_file_sync])?;
    set(context, module, "copyFile", copy_file)?;
    let cp_factory = context.evaluate_script_rooted(CP_API, "node:fs/shared-cp.js")?;
    let lstat_sync = get(context, module, "lstatSync")?;
    let unlink_sync = get(context, module, "unlinkSync")?;
    let symlink_sync_for_cp = get(context, module, "symlinkSync")?;
    let readlink_sync_for_cp = get(context, module, "readlinkSync")?;
    let realpath_sync_for_cp = get(context, module, "realpathSync")?;
    let utimes_sync_for_cp = get(context, module, "utimesSync")?;
    let cp_api = context.call_rooted(
        cp_factory,
        undefined,
        &[
            exists_sync,
            stat_sync,
            lstat_sync,
            readdir_sync,
            mkdir_sync,
            rmdir_sync,
            chmod_sync,
            unlink_sync,
            copy_file_sync,
            symlink_sync_for_cp,
            readlink_sync_for_cp,
            realpath_sync_for_cp,
            utimes_sync_for_cp,
        ],
    )?;
    let cp_sync = get(context, cp_api, "cpSync")?;
    let cp = get(context, cp_api, "cp")?;
    let cp_promise = get(context, cp_api, "cpPromise")?;
    set(context, module, "cpSync", cp_sync)?;
    set(context, module, "cp", cp)?;
    let symlink_sync = get(context, module, "symlinkSync")?;
    let symlink_factory =
        context.evaluate_script_rooted(ASYNC_SYMLINK_API, "node:fs/shared-async-symlink.js")?;
    let symlink = context.call_rooted(symlink_factory, undefined, &[symlink_sync])?;
    set(context, module, "symlink", symlink)?;
    let link_sync = get(context, module, "linkSync")?;
    let link_factory =
        context.evaluate_script_rooted(ASYNC_LINK_API, "node:fs/shared-async-link.js")?;
    let link = context.call_rooted(link_factory, undefined, &[link_sync])?;
    set(context, module, "link", link)?;
    let rename_sync = get(context, module, "renameSync")?;
    let rename_factory =
        context.evaluate_script_rooted(ASYNC_RENAME_API, "node:fs/shared-async-rename.js")?;
    let rename = context.call_rooted(rename_factory, undefined, &[rename_sync])?;
    set(context, module, "rename", rename)?;
    let unlink_sync = get(context, module, "unlinkSync")?;
    let unlink_factory =
        context.evaluate_script_rooted(ASYNC_UNLINK_API, "node:fs/shared-async-unlink.js")?;
    let unlink = context.call_rooted(unlink_factory, undefined, &[unlink_sync])?;
    set(context, module, "unlink", unlink)?;
    for (name, sync_name, descriptor) in [
        ("chown", "chownSync", false),
        ("lchown", "lchownSync", false),
        ("fchown", "fchownSync", true),
    ] {
        let owner_sync = get(context, module, sync_name)?;
        let owner_factory =
            context.evaluate_script_rooted(ASYNC_OWNER_API, "node:fs/shared-async-owner.js")?;
        let descriptor_value = context.boolean(descriptor);
        let owner =
            context.call_rooted(owner_factory, undefined, &[owner_sync, descriptor_value])?;
        set(context, module, name, owner)?;
    }
    let open_sync = get(context, module, "openSync")?;
    let close_sync = get(context, module, "closeSync")?;
    let fstat_sync = get(context, module, "fstatSync")?;
    let factory =
        context.evaluate_script_rooted(OPEN_CLOSE_FACTORY, "node:fs/shared-open-close.js")?;
    let undefined = context.undefined();
    let read_sync = get(context, module, "readSync")?;
    let open_close = context.call_rooted(
        factory,
        undefined,
        &[open_sync, close_sync, read_sync, fstat_sync],
    )?;
    let open = get(context, open_close, "open")?;
    let close = get(context, open_close, "close")?;
    let read = get(context, open_close, "read")?;
    let fstat = get(context, open_close, "fstat")?;
    set(context, module, "open", open)?;
    set(context, module, "close", close)?;
    set(context, module, "read", read)?;
    set(context, module, "fstat", fstat)?;
    let write_file_sync = get(context, module, "writeFileSync")?;
    let write_file_factory = context.evaluate_script_rooted(
        ASYNC_WRITE_FILE_FACTORY,
        "node:fs/shared-async-write-file.js",
    )?;
    let write_file =
        context.call_rooted(write_file_factory, undefined, &[write_file_sync, module])?;
    set(context, module, "writeFile", write_file)?;
    let append_file_sync = get(context, module, "appendFileSync")?;
    let append_file_factory = context.evaluate_script_rooted(
        ASYNC_APPEND_FILE_FACTORY,
        "node:fs/shared-async-append-file.js",
    )?;
    let append_file =
        context.call_rooted(append_file_factory, undefined, &[append_file_sync, module])?;
    set(context, module, "appendFile", append_file)?;
    let streams = crate::host::shared_vm::commonjs::stream_module(context)?;
    let readable = get(context, streams, "Readable")?;
    let read_stream_factory = context
        .evaluate_script_rooted(CREATE_READ_STREAM, "node:fs/shared-create-read-stream.js")?;
    let read_stream_open =
        context.host_function(crate::host::shared_vm::operation("fsReadStreamOpen"))?;
    let read_stream_read =
        context.host_function(crate::host::shared_vm::operation("fsReadStreamRead"))?;
    let read_stream_close =
        context.host_function(crate::host::shared_vm::operation("fsReadStreamClose"))?;
    let read_stream_api = context.call_rooted(
        read_stream_factory,
        undefined,
        &[
            read_stream_open,
            read_stream_read,
            read_stream_close,
            readable,
            module,
        ],
    )?;
    let create_read_stream = get(context, read_stream_api, "createReadStream")?;
    let read_stream = get(context, read_stream_api, "ReadStream")?;
    set(context, module, "createReadStream", create_read_stream)?;
    set(context, module, "ReadStream", read_stream)?;
    let rm_sync = get(context, module, "rmSync")?;
    let promises = promises_module(
        context,
        constants,
        open_sync,
        close_sync,
        read_sync,
        write_sync,
        fstat_sync,
        fchmod_sync,
        fsync_sync,
        fdatasync_sync,
        write_file_sync,
        append_file_sync,
        mkdir_sync,
        rmdir_sync,
        rm_sync,
        mkdtemp_sync,
        copy_file_sync,
        readv_sync,
        writev_sync,
        create_read_stream,
    )?;
    set(context, promises, "opendir", opendir_promise)?;
    for name in ["stat", "lstat"] {
        let sync = get(context, module, &format!("{name}Sync"))?;
        let promise_factory = context.evaluate_script_rooted(
            "(sync) => (...args) => Promise.resolve().then(() => sync(...args))",
            "node:fs/promises/shared-stat.js",
        )?;
        let promise = context.call_rooted(promise_factory, undefined, &[sync])?;
        set(context, promises, name, promise)?;
    }
    let realpath_sync = get(context, module, "realpathSync")?;
    let realpath_promise_factory = context.evaluate_script_rooted(
        "(realpathSync) => (...args) => Promise.resolve().then(() => realpathSync(...args))",
        "node:fs/promises/shared-realpath.js",
    )?;
    let realpath_promise =
        context.call_rooted(realpath_promise_factory, undefined, &[realpath_sync])?;
    set(context, promises, "realpath", realpath_promise)?;
    let statfs_sync = get(context, module, "statfsSync")?;
    let statfs_promise_factory = context.evaluate_script_rooted(
        "(statfsSync) => (...args) => Promise.resolve().then(() => statfsSync(...args))",
        "node:fs/promises/shared-statfs.js",
    )?;
    let statfs_promise = context.call_rooted(statfs_promise_factory, undefined, &[statfs_sync])?;
    set(context, promises, "statfs", statfs_promise)?;
    let mkdtemp_disposable_factory = context.evaluate_script_rooted(
        "(mkdtempSync, rmSync) => async (prefix) => { const absolutePath = await mkdtempSync(prefix); const normalizedPrefix = Buffer.isBuffer(prefix) ? prefix.toString() : prefix instanceof URL ? decodeURIComponent(prefix.pathname) : prefix; const path = normalizedPrefix.startsWith('/') ? absolutePath : `${normalizedPrefix}${absolutePath.slice(-6)}`; const remove = () => Promise.resolve().then(() => rmSync(absolutePath, { recursive: true, force: true })); const disposable = { path, remove }; disposable[Symbol.asyncDispose] = remove; return disposable; }",
        "node:fs/promises/shared-mkdtemp-disposable.js",
    )?;
    let mkdtemp_sync = get(context, module, "mkdtempSync")?;
    let rm_sync = get(context, module, "rmSync")?;
    let mkdtemp_disposable = context.call_rooted(
        mkdtemp_disposable_factory,
        undefined,
        &[mkdtemp_sync, rm_sync],
    )?;
    set(context, promises, "mkdtempDisposable", mkdtemp_disposable)?;
    let truncate_promise_factory = context.evaluate_script_rooted(
        "(truncateSync) => (...args) => Promise.resolve().then(() => truncateSync(...args))",
        "node:fs/promises/shared-truncate.js",
    )?;
    let truncate_promise =
        context.call_rooted(truncate_promise_factory, undefined, &[truncate_sync])?;
    set(context, promises, "truncate", truncate_promise)?;
    set(context, promises, "cp", cp_promise)?;
    let symlink_promise_factory = context.evaluate_script_rooted(
        "(symlinkSync) => (...args) => Promise.resolve().then(() => symlinkSync(...args))",
        "node:fs/promises/shared-symlink.js",
    )?;
    let symlink_promise =
        context.call_rooted(symlink_promise_factory, undefined, &[symlink_sync])?;
    set(context, promises, "symlink", symlink_promise)?;
    let link_sync = get(context, module, "linkSync")?;
    let link_promise_factory = context.evaluate_script_rooted(
        "(linkSync) => (...args) => Promise.resolve().then(() => linkSync(...args))",
        "node:fs/promises/shared-link.js",
    )?;
    let link_promise = context.call_rooted(link_promise_factory, undefined, &[link_sync])?;
    set(context, promises, "link", link_promise)?;
    let rename_promise_factory = context.evaluate_script_rooted(
        "(renameSync) => (...args) => Promise.resolve().then(() => renameSync(...args))",
        "node:fs/promises/shared-rename.js",
    )?;
    let rename_promise = context.call_rooted(rename_promise_factory, undefined, &[rename_sync])?;
    set(context, promises, "rename", rename_promise)?;
    let unlink_promise_factory = context.evaluate_script_rooted(
        "(unlinkSync) => (...args) => Promise.resolve().then(() => unlinkSync(...args))",
        "node:fs/promises/shared-unlink.js",
    )?;
    let unlink_promise = context.call_rooted(unlink_promise_factory, undefined, &[unlink_sync])?;
    set(context, promises, "unlink", unlink_promise)?;
    for (name, sync_name) in [("chown", "chownSync"), ("lchown", "lchownSync")] {
        let owner_sync = get(context, module, sync_name)?;
        let owner_promise_factory = context.evaluate_script_rooted(
            "(ownerSync) => (...args) => Promise.resolve().then(() => ownerSync(...args))",
            "node:fs/promises/shared-owner.js",
        )?;
        let owner_promise = context.call_rooted(owner_promise_factory, undefined, &[owner_sync])?;
        set(context, promises, name, owner_promise)?;
    }
    let access_promise_factory = context.evaluate_script_rooted(
        "(accessSync) => async function access(...args) { await Promise.resolve(); try { return accessSync(...args); } catch (error) { if (typeof error.stack === 'string') error.stack += '\\n    at async Object.access (node:fs/promises)'; throw error; } }",
        "node:fs/shared-access-promise.js",
    )?;
    let access_promise = context.call_rooted(access_promise_factory, undefined, &[access_sync])?;
    set(context, promises, "access", access_promise)?;
    let chmod_promise_factory = context.evaluate_script_rooted(
        "(chmodSync) => (...args) => Promise.resolve().then(() => chmodSync(...args))",
        "node:fs/shared-chmod-promise.js",
    )?;
    let chmod_promise = context.call_rooted(chmod_promise_factory, undefined, &[chmod_sync])?;
    set(context, promises, "chmod", chmod_promise)?;
    let metadata_promise_factory = context.evaluate_script_rooted(
        "(name) => (...args) => Promise.resolve().then(() => { const promises = globalThis.__nodeFs.promises; if (name === 'lchmod' && typeof promises?.lchmod === 'function') return promises.lchmod(...args); const sync = globalThis.__nodeFs[`${name}Sync`] || (name === 'lchmod' ? globalThis.__nodeFs.chmodSync : undefined); return sync(...args); })",
        "node:fs/promises/shared-metadata-mutations.js",
    )?;
    for name in ["lchmod", "utimes", "lutimes"] {
        let name_root = context.string_rooted(name);
        let promise = context.call_rooted(metadata_promise_factory, undefined, &[name_root])?;
        set(context, promises, name, promise)?;
        context.release_root(name_root);
        context.release_root(promise);
    }
    promise_public::install(context, promises)?;
    set(context, module, "promises", promises)?;

    let streams = crate::host::shared_vm::commonjs::stream_module(context)?;
    let writable = get(context, streams, "Writable")?;
    write_stream::install(context, module, writable)?;

    let install_missing_fs_methods = context.evaluate_script_rooted(
        "(fs) => { for (const name of ['utimesSync', 'lutimesSync', 'futimesSync', 'utimes', 'lutimes', 'futimes', 'rm', 'write']) { if (typeof fs[name] === 'function') continue; Object.defineProperty(fs, name, { configurable: true, writable: true, value() { const error = new Error(`${name} is unavailable outside a mounted VFS`); error.code = 'ENOSYS'; throw error; } }); } }",
        "node:bootstrap/vfs-fs-methods.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(install_missing_fs_methods, undefined, &[module])?;
    let install_time_callbacks = context.evaluate_script_rooted(
        "(fs, process) => { for (const name of ['utimes', 'lutimes', 'futimes']) { if (typeof fs[name] === 'function' && !String(fs[name]).includes('unavailable outside a mounted VFS')) continue; const syncName = `${name}Sync`; const sync = fs[syncName]; Object.defineProperty(fs, name, { configurable: true, writable: true, value(path, atime, mtime, callback) { let error; try { sync(path, atime, mtime); } catch (caught) { error = caught; } process.nextTick(callback, error || null); } }); } }",
        "node:bootstrap/fs-time-callbacks.js",
    )?;
    let process_module = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("Node process is not initialized"))?;
    context.call_rooted(install_time_callbacks, undefined, &[module, process_module])?;
    for name in [
        "lutimesSync",
        "futimesSync",
        "createReadStream",
        "createWriteStream",
    ] {
        let original = get(context, module, name)?;
        let function = metadata_public::wrap(context, original, name)?;
        set(context, module, name, function)?;
    }
    for name in [
        "access",
        "chmod",
        "fchmod",
        "statfs",
        "exists",
        "chown",
        "lchown",
        "fchown",
        "utimes",
        "lutimes",
        "futimes",
        "fsync",
        "fdatasync",
        "ftruncate",
        "lstat",
        "rm",
    ] {
        let original = get(context, module, name)?;
        let function = callback_public::wrap(context, original, name)?;
        set(context, module, name, function)?;
    }
    let validate_encodings = context.evaluate_script_rooted(
        "(fs) => {\n\
          const assertEncoding = (options, allowBuffer) => {\n\
            const encoding = typeof options === 'string' ? options : options?.encoding;\n\
            if (encoding && !(allowBuffer && encoding === 'buffer') && !Buffer.isEncoding(encoding)) {\n\
              const error = new TypeError(`The argument '${encoding}' is an invalid encoding.`);\n\
              error.code = 'ERR_INVALID_ARG_VALUE';\n\
              throw error;\n\
            }\n\
          };\n\
          const wrap = (name, index, allowBuffer = false) => {\n\
            const original = fs[name];\n\
            if (typeof original !== 'function') return;\n\
            Object.defineProperty(fs, name, { configurable: true, writable: true, value: function(...args) {\n\
              assertEncoding(args[index], allowBuffer);\n\
              return Reflect.apply(original, this, args);\n\
            } });\n\
          };\n\
          for (const name of ['readFileSync', 'readFile', 'createReadStream', 'createWriteStream']) wrap(name, 1);\n\
          for (const name of ['readdirSync', 'readdir', 'opendirSync', 'opendir', 'readlinkSync', 'readlink', 'watch', 'realpathSync', 'realpath', 'mkdtempSync', 'mkdtemp']) wrap(name, 1, true);\n\
          for (const name of ['writeFileSync', 'writeFile', 'appendFileSync', 'appendFile']) wrap(name, 2);\n\
          for (const name of ['ReadStream', 'WriteStream']) {\n\
            const Original = fs[name];\n\
            if (typeof Original !== 'function') continue;\n\
            const Wrapped = function(path, options) {\n\
              assertEncoding(options);\n\
              return Reflect.construct(Original, [path, options], new.target || Original);\n\
            };\n\
            Wrapped.prototype = Original.prototype;\n\
            Object.setPrototypeOf(Wrapped, Original);\n\
            Object.defineProperty(Wrapped, 'name', { value: name });\n\
            fs[name] = Wrapped;\n\
          }\n\
        }",
        "node:fs/shared-encoding-validation.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(validate_encodings, undefined, &[module])?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_vfs_fs_fallbacks");
    let global = context.global_root()?;
    let key = context.string_rooted("__nodeFs");
    if !context.define_data_property_rooted(global, key, module, false, false, true)? {
        return Err(RootedError::host("cannot install canonical Node Fs alias"));
    }

    Ok(module)
}

pub(crate) fn promises_module(
    context: &mut NativeContext<'_, NodeHost>,
    constants: RootId,
    open_sync: RootId,
    close_sync: RootId,
    read_sync: RootId,
    write_sync: RootId,
    fstat_sync: RootId,
    fchmod_sync: RootId,
    fsync_sync: RootId,
    fdatasync_sync: RootId,
    write_file_sync: RootId,
    append_file_sync: RootId,
    mkdir_sync: RootId,
    rmdir_sync: RootId,
    rm_sync: RootId,
    mkdtemp_sync: RootId,
    copy_file_sync: RootId,
    readv_sync: RootId,
    writev_sync: RootId,
    create_read_stream: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(PROMISES_FACTORY, "node:fs/promises/shared.js")?;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    let stat = context.host_function(crate::host::shared_vm::operation("fsStatSync"))?;
    let lstat = context.host_function(crate::host::shared_vm::operation("fsLstatSync"))?;
    let readdir_sync = make_readdir_sync(context)?;
    let readdir = make_readdir_async(context, readdir_sync)?;
    let readlink = context.host_function(crate::host::shared_vm::operation("fsReadlinkSync"))?;
    let realpath = context.host_function(crate::host::shared_vm::operation("fsRealpathSync"))?;
    let undefined = context.undefined();
    let normalize_read = context.host_function(crate::host::shared_vm::operation(
        "fsFileHandleReadParameters",
    ))?;
    let global = context.global_root()?;
    let reflect = get(context, global, "Reflect")?;
    let apply = get(context, reflect, "apply")?;
    let (handle_bridge, register_handle, close_handle) = file_handle_write::bridge(context)?;
    let promises = context.call_rooted(
        factory,
        undefined,
        &[
            read_file,
            stat,
            lstat,
            readdir,
            readlink,
            realpath,
            open_sync,
            close_sync,
            read_sync,
            write_sync,
            fstat_sync,
            fchmod_sync,
            fsync_sync,
            fdatasync_sync,
            write_file_sync,
            append_file_sync,
            readv_sync,
            writev_sync,
            create_read_stream,
            normalize_read,
            apply,
            register_handle,
            close_handle,
        ],
    )?;
    file_handle_write::install(context, promises, handle_bridge)?;
    set(context, promises, "constants", constants)?;
    let mkdir_factory = context.evaluate_script_rooted(
        "(mkdirSync) => (...args) => Promise.resolve().then(() => mkdirSync(...args))",
        "node:fs/promises/shared-mkdir.js",
    )?;
    let mkdir = context.call_rooted(mkdir_factory, undefined, &[mkdir_sync])?;
    set(context, promises, "mkdir", mkdir)?;
    let rmdir_factory = context.evaluate_script_rooted(
        "(rmdirSync) => (...args) => Promise.resolve().then(() => rmdirSync(...args))",
        "node:fs/promises/shared-rmdir.js",
    )?;
    let rmdir = context.call_rooted(rmdir_factory, undefined, &[rmdir_sync])?;
    set(context, promises, "rmdir", rmdir)?;
    let native_watch =
        context.host_function(crate::host::shared_vm::operation("fsRecursivePromiseWatch"))?;
    let watch_factory = context.evaluate_script_rooted(
        "native => async function* _watch(filename, options = undefined) { yield* native(filename, options); }",
        "node:fs/promises/watch-facade.js",
    )?;
    let watch = context.call_rooted(watch_factory, undefined, &[native_watch])?;
    set(context, promises, "watch", watch)?;
    let rm_factory = context.evaluate_script_rooted(
        "(rmSync) => (...args) => Promise.resolve().then(() => rmSync(...args))",
        "node:fs/promises/shared-rm.js",
    )?;
    let rm = context.call_rooted(rm_factory, undefined, &[rm_sync])?;
    set(context, promises, "rm", rm)?;
    let mkdtemp_factory = context.evaluate_script_rooted(
        "(mkdtempSync) => (...args) => Promise.resolve().then(() => mkdtempSync(...args))",
        "node:fs/promises/shared-mkdtemp.js",
    )?;
    let mkdtemp = context.call_rooted(mkdtemp_factory, undefined, &[mkdtemp_sync])?;
    set(context, promises, "mkdtemp", mkdtemp)?;
    let copy_file_factory = context.evaluate_script_rooted(
        "(copyFileSync) => (...args) => Promise.resolve().then(() => copyFileSync(...args))",
        "node:fs/promises/shared-copyfile.js",
    )?;
    let copy_file = context.call_rooted(copy_file_factory, undefined, &[copy_file_sync])?;
    set(context, promises, "copyFile", copy_file)?;
    Ok(promises)
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

pub(crate) fn resolve_shared_path(
    context: &mut NativeContext<'_, NodeHost>,
    path: String,
) -> String {
    let candidate = std::path::Path::new(&path);
    if candidate.is_absolute() {
        return path;
    }
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
    cwd.join(candidate).to_string_lossy().into_owned()
}

pub(super) fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if !context.set_property_rooted(object, key, value, object)? {
        return Err(RootedError::host("cannot install shared fs binding"));
    }
    Ok(())
}

pub(crate) fn validate_read_signal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let signal = args.first().copied().unwrap_or_else(|| context.undefined());
    super::util_shared_vm::validate_abort_signal(context, signal, "options.signal")?;
    Ok(context.undefined())
}

/// Callback invocation lives outside the read-error branch: an exception
/// thrown by the callback escapes once rather than calling it a second time.
pub(crate) fn complete_read_file(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let signal = args.get(4).copied().unwrap_or(undefined);
    let aborted = if context
        .rooted_value(signal)
        .is_some_and(|value| value.is_undefined())
    {
        false
    } else {
        let aborted = get(context, signal, "aborted")?;
        context.truthy_rooted(aborted)?
    };
    let completion = if aborted {
        let error = context.error_rooted("The operation was aborted")?;
        let name = context.string_rooted("AbortError");
        set(context, error, "name", name)?;
        let code = context.string_rooted("ABORT_ERR");
        set(context, error, "code", code)?;
        let cause = get(context, signal, "reason")?;
        let key = context.string_rooted("cause");
        context.define_data_property_rooted(error, key, cause, true, false, true)?;
        vec![error]
    } else {
        match context.call_rooted(args[0], undefined, &[args[2], args[3]]) {
            Ok(data) => vec![context.null(), data],
            Err(error) => {
                let exception = match error.exception {
                    Some(exception) => exception,
                    None => context.error_rooted(&error.error.to_string())?,
                };
                vec![exception]
            }
        }
    };
    context.call_rooted(args[1], undefined, &completion)?;
    Ok(undefined)
}

pub(crate) fn read_file_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let undefined = context.undefined();
    let provided_options = args.get(1).copied().unwrap_or(undefined);
    if let Some(result) =
        super::fs_policy_shared_vm::vfs_read(context, "readFileSync", &[input, provided_options])?
    {
        return Ok(result);
    }
    let defaults = context.object_rooted()?;
    let key = context.string_rooted("flag");
    let read_flag = context.string_rooted("r");
    context.define_data_property_rooted(defaults, key, read_flag, true, true, true)?;
    let options =
        super::fs_policy_shared_vm::call(context, "getOptions", &[provided_options, defaults])?;
    super::fs_policy_shared_vm::call(context, "validateReadFileBufferOptions", &[options])?;
    let user_buffer = get(context, options, "buffer")?;
    let has_user_buffer = !context
        .rooted_value(user_buffer)
        .is_some_and(|value| value.is_undefined());
    let encoding = get(context, options, "encoding")?;
    let utf8 = if context.string_text(encoding)?.as_deref() == Some("utf8") {
        true
    } else {
        let encoding = get(context, options, "encoding")?;
        context.string_text(encoding)?.as_deref() == Some("utf-8")
    };
    let fast_utf8 = utf8 && !has_user_buffer;
    // Node's isInt32 intentionally performs ToInt32 before strict equality,
    // including observable valueOf/toString on non-numeric paths.
    let zero = context.number(0.0);
    let integer = context.bitwise_or_rooted(input, zero)?;
    let fd = if context.strict_equal_rooted(input, integer)? {
        Some(context.to_number(integer)? as i32)
    } else {
        None
    };
    if !fast_utf8 {
        return super::fs_read_file_shared_vm::read_general(
            context,
            input,
            options,
            fd.is_some(),
            has_user_buffer,
        );
    }
    let mut filename = None;
    let bytes = if let Some(fd) = fd {
        let flag = get(context, options, "flag")?;
        super::fs_policy_shared_vm::call(context, "stringToFlags", &[flag])?;
        let fs = context.host_mut().shared_state().borrow().fs.clone();
        fs.with_descriptor(fd, |file| Ok(fs_ops::read_file(file, MAX_READ_FILE_BYTES)))
            .unwrap_or_else(|error| {
                Err(ReadFileFailure::Io {
                    syscall: "read",
                    error,
                })
            })
    } else {
        let input = super::fs_path::validate(context, input, "path")?;
        let flag = get(context, options, "flag")?;
        let flags = super::fs_policy_shared_vm::call(context, "stringToFlags", &[flag])?;
        let flags = context.to_number(flags)? as i32;
        let path = super::fs_path::resolve(context, input)?;
        filename = Some(super::fs_path::display(context, input)?);
        #[cfg(unix)]
        let opened = fs_ops::open_numeric(&path, flags, None);
        #[cfg(not(unix))]
        let opened = {
            let _ = flags;
            fs_ops::open(&path, context.string_text(flag)?.as_deref(), None)
        };
        match opened {
            Ok(mut file) => fs_ops::read_file(&mut file, MAX_READ_FILE_BYTES),
            Err(error) => Err(ReadFileFailure::Io {
                syscall: "open",
                error,
            }),
        }
    };
    match bytes {
        Ok(bytes) => super::buffer_string_shared_vm::decode_bytes(context, &bytes, "utf8"),
        Err(ReadFileFailure::TooLarge(size)) => {
            let error =
                context.range_error_rooted(&format!("File size ({size}) is greater than 2 GiB"))?;
            super::node_errors_shared_vm::decorate_formatted_range_error(
                context,
                error,
                "ERR_FS_FILE_TOO_LARGE",
            )?;
            Err(context.throw(error))
        }
        Err(ReadFileFailure::Io { syscall, error }) => {
            let path = if syscall == "open" {
                filename.as_deref()
            } else {
                None
            };
            sync::throw_io_error(context, syscall, path, &error)
        }
    }
}

pub(crate) fn read_stream_open(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = resolve_shared_path(context, path);
    let result = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .open_read_stream(path.clone());
    match result {
        Ok(fd) => Ok(context.number(fd as f64)),
        Err(error) => Err(stream_io_error(context, error, "open", &path)?),
    }
}

pub(crate) fn read_stream_read(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = integer_arg(context, args.first().copied(), "file descriptor")?;
    let requested = args
        .get(1)
        .copied()
        .and_then(|arg| context.rooted_value(arg))
        .and_then(|value| value.as_number())
        .filter(|size| size.is_finite() && *size > 0.0)
        .unwrap_or(READ_STREAM_CHUNK_LIMIT as f64) as usize;
    let size = requested.min(READ_STREAM_CHUNK_LIMIT);
    let position = args
        .get(2)
        .copied()
        .and_then(|arg| context.rooted_value(arg))
        .and_then(|value| value.as_number())
        .filter(|position| position.is_finite() && *position >= 0.0 && position.fract() == 0.0)
        .and_then(|position| u64::try_from(position as u128).ok());
    let (result, path) = {
        let shared_state = context.host_mut().shared_state();
        let state = shared_state.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        let result = state.fs.read_stream_chunk(fd, size, position);
        (result, path)
    };
    match result {
        Ok(Some(bytes)) => crate::modules::buffer_shared_vm::from_bytes(context, &bytes),
        Ok(None) => Ok(context.null()),
        Err(error) => Err(stream_io_error(context, error, "read", &path)?),
    }
}

pub(crate) fn read_stream_close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = integer_arg(context, args.first().copied(), "file descriptor")?;
    context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .close_read_stream(fd);
    Ok(context.undefined())
}

const READ_STREAM_CHUNK_LIMIT: usize = 64 * 1024;

pub(super) fn integer_arg(
    context: &mut NativeContext<'_, NodeHost>,
    argument: Option<RootId>,
    name: &str,
) -> Result<i32, RootedError> {
    let number = argument
        .and_then(|argument| context.rooted_value(argument))
        .and_then(|value| value.as_number())
        .filter(|number| number.is_finite() && number.fract() == 0.0);
    match number.and_then(|number| i32::try_from(number as i64).ok()) {
        Some(number) => Ok(number),
        None => {
            let error = context.type_error_rooted(&format!("invalid internal fs {name}"))?;
            Err(context.throw(error))
        }
    }
}

pub(super) fn stream_io_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: std::io::Error,
    syscall: &str,
    path: &str,
) -> Result<RootedError, RootedError> {
    let (code, _) = super::fs_error_codes::code_for(&error);
    let description = super::fs_error_codes::strerror(code);
    let message = if path.is_empty() {
        format!("{code}: {description}, {syscall}")
    } else {
        format!("{code}: {description}, {syscall} '{path}'")
    };
    let exception = context.error_rooted(&message)?;
    let code_value = context.string_rooted(code);
    set(context, exception, "code", code_value)?;
    let syscall_value = context.string_rooted(syscall);
    set(context, exception, "syscall", syscall_value)?;
    if !path.is_empty() {
        let path_value = context.string_rooted(path);
        set(context, exception, "path", path_value)?;
    }
    if let Some(errno) = error.raw_os_error() {
        let errno_value = context.number(-f64::from(errno));
        set(context, exception, "errno", errno_value)?;
    }
    Ok(context.throw(exception))
}

/// Install unchanged Node glob exports when the native fs module is first
/// required. The original policy retains its own lazy traversal-module import.
pub(crate) fn initialize_glob_policy(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    // Node 26 exposes the bundled fast UTF-8 writer from `node:fs`. Load it
    // only after the public fs module has been cached because the implementation
    // itself requires fs and captures its ordinary write operations.
    let utf8_stream = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "fs.js",
        "internal/streams/fast-utf8-stream.js",
    )?;
    set(context, module, "Utf8Stream", utf8_stream)?;
    // Keep Node's legacy aliases aligned with the stream constructors already
    // installed on the public fs object, and expose its timestamp coercion
    // helper for callers that rely on the historical underscored API.
    for (alias, constructor) in [
        ("FileReadStream", "ReadStream"),
        ("FileWriteStream", "WriteStream"),
    ] {
        let value = get(context, module, constructor)?;
        set(context, module, alias, value)?;
    }
    let to_unix_timestamp = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/fs/utils.js",
        "toUnixTimestamp",
    )?;
    set(context, module, "_toUnixTimestamp", to_unix_timestamp)?;
    let policy = crate::host::shared_vm::commonjs::node_library_module(context, "fs.js", "fs.js")?;
    for name in ["glob", "globSync", "readdirSync"] {
        let function = get(context, policy, name)?;
        set(context, module, name, function)?;
    }
    let directory = directory_policy(context)?;
    let readdir = get(context, policy, "readdirSync")?;
    set(context, directory, "readdirSync", readdir)?;
    // Node's public fs module does not load its promises implementation until
    // requested. In particular its readline dependencies must not eagerly
    // capture the OS binding while merely requiring fs.
    let promises = get(context, module, "promises")?;
    let initializer = context.host_function(crate::host::shared_vm::operation(
        "fsPromisesGlobInitialize",
    ))?;
    context.define_lazy_data_properties_rooted(promises, &["glob"], true, initializer)?;
    let permission_guard =
        context.evaluate_script_rooted(PERMISSION_GUARD, "node:fs/permission-guard.js")?;
    let undefined = context.undefined();
    context.call_rooted(permission_guard, undefined, &[module])?;
    Ok(())
}

pub(crate) fn initialize_promises_glob_policy(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let policy = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "fs.js",
        "internal/fs/promises.js",
    )?;
    let exports = get(context, policy, "exports")?;
    let glob = get(context, exports, "glob")?;
    let module = crate::host::shared_vm::commonjs::fs_module(context)?;
    let promises = get(context, module, "promises")?;
    set(context, promises, "glob", glob)?;
    Ok(context.undefined())
}

/// The native filesystem and original fs utilities import one another. Keep the
/// constructor in private policy and initialize it after the native module is
/// cached, so both imports observe the same completed filesystem object.
fn directory_policy(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(
        context,
        "\0binding:fs-directory-policy",
        |context| context.null_object_rooted(),
    )
}

pub(crate) fn initialize_directory_policy(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    let dirent = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/fs/utils.js",
        "Dirent",
    )?;
    let policy = directory_policy(context)?;
    set(context, policy, "Dirent", dirent)?;
    set(context, module, "Dirent", dirent)
}
