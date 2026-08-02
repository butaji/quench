/* Small, dependency-free globals available before user code. */
globalThis.global = globalThis;
globalThis.globalThis = globalThis;

globalThis.__nodeFormat = (args) => args.map((value) => {
  try { return typeof value === 'string' ? value : JSON.stringify(value); }
  catch (_) { return String(value); }
}).join(' ');
globalThis.console = globalThis.console || {};
for (const method of ['log', 'info', 'warn', 'error', 'debug']) {
  globalThis.console[method] = (...args) => globalThis.__quench_console_write(globalThis.__nodeFormat(args));
}
globalThis.console.dir = (value) => globalThis.__quench_console_write(globalThis.__nodeFormat([value]));
globalThis.console.assert = (condition, ...args) => { if (!condition) globalThis.console.error(...args); };
const consoleTimers = {};
const consoleCounts = {};
globalThis.console.count = (label = 'default') => { consoleCounts[label] = (consoleCounts[label] || 0) + 1; globalThis.__quench_console_write(`${label}: ${consoleCounts[label]}`); };
globalThis.console.countReset = (label = 'default') => { consoleCounts[label] = 0; };
globalThis.console.clear = () => undefined;
globalThis.console.time = (label = 'default') => { consoleTimers[label] = BigInt(globalThis.__quench_now_ns()); };
globalThis.console.timeLog = (label = 'default', ...args) => {
  if (consoleTimers[label] === undefined) return;
  globalThis.__quench_console_write(`${label}: ${Number(BigInt(globalThis.__quench_now_ns()) - consoleTimers[label]) / 1e6} ms ${globalThis.__nodeFormat(args)}`);
};
globalThis.console.timeEnd = (label = 'default') => {
  if (consoleTimers[label] === undefined) return;
  globalThis.__quench_console_write(`${label}: ${Number(BigInt(globalThis.__quench_now_ns()) - consoleTimers[label]) / 1e6} ms`);
  delete consoleTimers[label];
};

globalThis.process = {
  env: new Proxy({}, {
    get: (_, key) => typeof key === 'string' ? globalThis.__quench_env_get(key) : undefined,
    set: (_, key, value) => { globalThis.__quench_env_set(String(key), String(value)); globalThis.__quench_env_keys = [...new Set([...globalThis.__quench_env_keys, String(key)])]; return true; },
    deleteProperty: (_, key) => { globalThis.__quench_env_delete(String(key)); globalThis.__quench_env_keys = globalThis.__quench_env_keys.filter((item) => item !== String(key)); return true; },
    has: (_, key) => typeof key === 'string' && globalThis.__quench_env_get(key) !== undefined,
    ownKeys: () => globalThis.__quench_env_keys,
    getOwnPropertyDescriptor: (_, key) => ({ enumerable: true, configurable: true, value: globalThis.__quench_env_get(String(key)) }),
  }),
  argv: [globalThis.__quench_exec_path, ...globalThis.__quench_argv.slice(1)],
  execPath: globalThis.__quench_exec_path,
  pid: globalThis.__quench_pid,
  ppid: globalThis.__quench_ppid,
  getuid: () => globalThis.__quench_getuid,
  geteuid: () => globalThis.__quench_geteuid,
  getgid: () => globalThis.__quench_getgid,
  getegid: () => globalThis.__quench_getegid,
  platform: globalThis.__quench_platform === 'macos' ? 'darwin' : globalThis.__quench_platform,
  arch: globalThis.__quench_arch === 'aarch64' ? 'arm64' : globalThis.__quench_arch,
  version: 'v20.0.0',
  versions: { node: '20.0.0', v8: '0.0.0-quench', uv: '0.0.0' },
  release: { name: 'node', lts: 'Quench' },
  config: { variables: { v8_enable_i18n_support: false, v8_enable_temporal_support: false, node_shared: false, node_use_ffi: false } },
  features: { inspector: false, tls: false, quic: false, dtls: false },
  cwd: () => globalThis.__quench_cwd_get(),
  chdir: (value) => globalThis.__quench_chdir(String(value)),
  exitCode: 0,
  umask: (mask) => globalThis.__quench_umask(mask === undefined ? undefined : Number(mask)),
  nextTick: (callback, ...args) => queueMicrotask(() => callback(...args)),
  hrtime: (previous) => {
    const ns = BigInt(globalThis.__quench_now_ns());
    const current = [Number(ns / 1000000000n), Number(ns % 1000000000n)];
    if (!previous) return current;
    let seconds = current[0] - previous[0]; let nanos = current[1] - previous[1];
    if (nanos < 0) { seconds--; nanos += 1000000000; }
    return [seconds, nanos];
  },
};
process.hrtime.bigint = () => BigInt(globalThis.__quench_now_ns());

globalThis.setImmediate = (callback, ...args) => queueMicrotask(() => callback(...args));
globalThis.clearImmediate = () => undefined;
globalThis.setTimeout = (callback, _delay = 0, ...args) => {
  const id = { active: true };
  queueMicrotask(() => { if (id.active) callback(...args); });
  return id;
};
globalThis.clearTimeout = (id) => { if (id) id.active = false; };
globalThis.setInterval = (callback, _delay = 0, ...args) => setTimeout(callback, _delay, ...args);
globalThis.clearInterval = globalThis.clearTimeout;
globalThis.__nodeTimers = {
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
  setImmediate,
  clearImmediate,
};
globalThis.__nodeTimersPromises = {
  setTimeout: (_delay = 0, value) => new Promise((resolve) => queueMicrotask(() => resolve(value))),
  setImmediate: (value) => new Promise((resolve) => queueMicrotask(() => resolve(value))),
};

const processListeners = {};
process.on = (event, listener) => {
  (processListeners[event] ||= []).push(listener);
  return process;
};
process.once = (event, listener) => {
  const once = (...args) => { process.removeListener(event, once); listener(...args); };
  return process.on(event, once);
};
process.removeListener = (event, listener) => {
  processListeners[event] = (processListeners[event] || []).filter((item) => item !== listener);
  return process;
};
process.removeAllListeners = (event) => { if (event) delete processListeners[event]; else Object.keys(processListeners).forEach((key) => delete processListeners[key]); };
process.emit = (event, ...args) => {
  const listeners = processListeners[event] || [];
  listeners.forEach((listener) => listener(...args));
  return listeners.length > 0;
};
process.emitWarning = (warning, options = {}) => {
  const message = warning instanceof Error ? warning.message : String(warning);
  process.emit('warning', { name: options.name || 'Warning', message, code: options.code });
};

class NodeBuffer extends Uint8Array {
  static from(value, encoding) {
    if (typeof value === 'string') {
      if (encoding === 'hex') {
        const output = new NodeBuffer(value.length / 2);
        for (let i = 0; i < output.length; i++) output[i] = parseInt(value.slice(i * 2, i * 2 + 2), 16);
        return output;
      }
      if (encoding === 'base64') {
        const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
        const clean = value.replace(/=+$/, ''); const output = new NodeBuffer(Math.floor(clean.length * 6 / 8));
        let buffer = 0; let bits = 0; let index = 0;
        for (const char of clean) { buffer = (buffer << 6) | alphabet.indexOf(char); bits += 6; if (bits >= 8) { bits -= 8; output[index++] = (buffer >> bits) & 255; } }
        return output;
      }
      return new NodeBuffer(new NodeTextEncoder().encode(value));
    }
    return new NodeBuffer(value);
  }
  static alloc(size, fill = 0) { return new NodeBuffer(size).fill(fill); }
  static isBuffer(value) { return value instanceof NodeBuffer; }
  static byteLength(value) { return new NodeTextEncoder().encode(String(value)).length; }
  static concat(list, totalLength) {
    const length = totalLength === undefined ? list.reduce((sum, item) => sum + item.length, 0) : totalLength;
    const output = new NodeBuffer(length); let offset = 0;
    list.forEach((item) => { output.set(item.subarray(0, length - offset), offset); offset += item.length; });
    return output;
  }
  toString(encoding = 'utf8') {
    if (encoding === 'hex') return Array.from(this, (byte) => byte.toString(16).padStart(2, '0')).join('');
    if (encoding === 'base64') {
      const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'; let result = '';
      for (let i = 0; i < this.length; i += 3) { const n = (this[i] << 16) | ((this[i + 1] || 0) << 8) | (this[i + 2] || 0); result += alphabet[(n >> 18) & 63] + alphabet[(n >> 12) & 63] + (i + 1 < this.length ? alphabet[(n >> 6) & 63] : '=') + (i + 2 < this.length ? alphabet[n & 63] : '='); }
      return result;
    }
    return new NodeTextDecoder().decode(this);
  }
}
globalThis.Buffer = NodeBuffer;
const nodeAtob = (value) => NodeBuffer.from(String(value), 'base64').toString();
const nodeBtoa = (value) => NodeBuffer.from(String(value)).toString('base64');
class NodeTextEncoder {
  encode(value) {
    const output = [];
    for (const character of String(value)) {
      const code = character.codePointAt(0);
      if (code < 0x80) output.push(code);
      else if (code < 0x800) output.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
      else if (code < 0x10000) output.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
      else output.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 0x3f), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    }
    return new Uint8Array(output);
  }
}
globalThis.TextEncoder = NodeTextEncoder;
class NodeTextDecoder {
  decode(bytes) {
    let result = '';
    for (let i = 0; i < bytes.length;) {
      const first = bytes[i++];
      if (first < 0x80) result += String.fromCodePoint(first);
      else if (first < 0xe0) result += String.fromCodePoint(((first & 0x1f) << 6) | (bytes[i++] & 0x3f));
      else if (first < 0xf0) result += String.fromCodePoint(((first & 0x0f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f));
      else result += String.fromCodePoint(((first & 7) << 18) | ((bytes[i++] & 0x3f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f));
    }
    return result;
  }
}
globalThis.TextDecoder = NodeTextDecoder;
const nodePathValue = (value) => value instanceof NodeBuffer ? value.toString() : value instanceof Uint8Array ? new NodeTextDecoder().decode(value) : value instanceof globalThis.__nodeURL ? globalThis.__nodeUrlModule.fileURLToPath(value) : String(value);
const nodeFsPath = (value) => {
  if (typeof value === 'string' || value instanceof NodeBuffer || value instanceof Uint8Array || value instanceof globalThis.__nodeURL) return nodePathValue(value);
  const error = new TypeError('The "path" argument must be of type string or an instance of Buffer or URL');
  error.code = 'ERR_INVALID_ARG_TYPE';
  throw error;
};

globalThis.__nodeAssert = (value, message) => { if (!value) throw new Error(message || 'Assertion failed'); };
globalThis.__nodeAssert.strictEqual = (actual, expected, message) => {
  if (!Object.is(actual, expected)) throw new Error(message || `${actual} !== ${expected}`);
};
globalThis.__nodeAssert.equal = (actual, expected, message) => {
  if (actual != expected) throw new Error(message || `${actual} != ${expected}`);
};
globalThis.__nodeAssert.notStrictEqual = (actual, expected, message) => {
  if (Object.is(actual, expected)) throw new Error(message || `${actual} === ${expected}`);
};
globalThis.__nodeAssert.notEqual = (actual, expected, message) => {
  if (actual == expected) throw new Error(message || `${actual} == ${expected}`);
};
globalThis.__nodeAssert.ok = globalThis.__nodeAssert;
globalThis.__nodeAssert.deepStrictEqual = (actual, expected, message) => {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(message || 'values differ');
};
globalThis.__nodeAssert.throws = (fn, expected) => {
  let thrown = false;
  try { fn(); } catch (error) {
    thrown = true;
    if (expected && expected.name && error.name !== expected.name) throw error;
  }
  if (!thrown) throw new Error('Missing expected exception');
};
globalThis.__nodeAssert.ifError = (error) => { if (error) throw error; };
globalThis.__nodeAssert.doesNotThrow = (fn, message) => {
  try { fn(); } catch (error) { throw new Error(message || `Unexpected exception: ${error}`); }
};
globalThis.__nodeAssert.rejects = (promiseOrFn, expected) => Promise.resolve().then(() => typeof promiseOrFn === 'function' ? promiseOrFn() : promiseOrFn).then(
  () => { throw new Error('Missing expected rejection'); },
  (error) => { if (expected && expected.name && error.name !== expected.name) throw error; return error; },
);
globalThis.__nodeAssert.doesNotReject = (promiseOrFn, message) => Promise.resolve().then(() => typeof promiseOrFn === 'function' ? promiseOrFn() : promiseOrFn).catch((error) => { throw new Error(message || `Unexpected rejection: ${error}`); });
globalThis.__nodeAssert.match = (value, expression) => {
  if (!expression.test(String(value))) throw new Error('Value did not match expression');
};
globalThis.__nodeAssert.strict = globalThis.__nodeAssert;

globalThis.__nodePath = {
  sep: '/',
  isAbsolute: (value) => String(value).startsWith('/'),
  normalize: (value) => {
    const absolute = String(value).startsWith('/');
    const parts = String(value).split('/').filter((part) => part && part !== '.');
    const output = [];
    parts.forEach((part) => { if (part === '..' && output.length && output[output.length - 1] !== '..') output.pop(); else if (part !== '..') output.push(part); });
    const result = (absolute ? '/' : '') + output.join('/');
    return result || (absolute ? '/' : '.');
  },
  basename: (value) => String(value).replace(/\\/g, '/').split('/').pop(),
  dirname: (value) => { const parts = String(value).replace(/\\/g, '/').split('/'); parts.pop(); return parts.join('/') || '.'; },
  extname: (value) => { const name = globalThis.__nodePath.basename(value); const i = name.lastIndexOf('.'); return i > 0 ? name.slice(i) : ''; },
  join: (...parts) => globalThis.__nodePath.normalize(parts.join('/')),
  resolve: (...parts) => globalThis.__nodePath.normalize(parts.filter(Boolean).join('/')),
  relative: (from, to) => {
    const a = globalThis.__nodePath.normalize(from).split('/').filter(Boolean);
    const b = globalThis.__nodePath.normalize(to).split('/').filter(Boolean);
    while (a.length && a[0] === b[0]) { a.shift(); b.shift(); }
    return [...a.map(() => '..'), ...b].join('/') || '';
  },
  parse: (value) => {
    const input = String(value); const base = globalThis.__nodePath.basename(input); const dir = globalThis.__nodePath.dirname(input); const ext = globalThis.__nodePath.extname(base);
    return { root: input.startsWith('/') ? '/' : '', dir, base, ext, name: ext ? base.slice(0, -ext.length) : base };
  },
  format: (parts) => globalThis.__nodePath.join(parts.dir || parts.root || '', parts.base || `${parts.name || ''}${parts.ext || ''}`),
};

globalThis.__nodeCommon = {
  mustCall: (fn, exact = 1) => {
    let calls = 0;
    const wrapped = function (...args) { calls++; wrapped.calls = calls; return fn(...args); };
    wrapped.calls = 0; wrapped.expected = exact;
    (globalThis.__nodeCallChecks ||= []).push(wrapped);
    return wrapped;
  },
  mustSucceed: (fn = () => {}) => globalThis.__nodeCommon.mustCall((error, ...args) => {
    if (error) throw error;
    return fn(...args);
  }),
  mustNotCall: (message = 'Unexpected call') => () => { throw new Error(message); },
  noop: () => {},
  expectWarning: (_type, _message) => {},
  mustNotMutateObjectDeep: (value) => value,
  isLinux: process.platform === 'linux',
  isMacOS: process.platform === 'darwin',
};
globalThis.__quench_verify_calls = () => {
  for (const callback of globalThis.__nodeCallChecks || []) {
    if (callback.calls !== callback.expected) throw new Error(`Expected ${callback.expected} calls, got ${callback.calls}`);
  }
};
globalThis.__nodeTmpdir = {
  path: `/tmp/quench-node-${process.pid}`,
  refresh: () => { try { globalThis.__quench_fs_mkdir(globalThis.__nodeTmpdir.path); } catch (_) {} },
  resolve: (name = '') => globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name)),
  fileURL: (name = '') => new globalThis.__nodeURL(`file://${globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name))}`),
};
class NodeEventEmitter {
  constructor() { this._events = {}; }
  on(event, listener) { (this._events[event] ||= []).push(listener); return this; }
  addListener(event, listener) { return this.on(event, listener); }
  once(event, listener) {
    const wrapped = (...args) => { this.removeListener(event, wrapped); listener(...args); };
    return this.on(event, wrapped);
  }
  emit(event, ...args) {
    const listeners = this._events[event] || [];
    listeners.slice().forEach((listener) => listener(...args));
    return listeners.length > 0;
  }
  removeListener(event, listener) {
    this._events[event] = (this._events[event] || []).filter((item) => item !== listener);
    return this;
  }
  off(event, listener) { return this.removeListener(event, listener); }
  removeAllListeners(event) {
    if (event === undefined) this._events = {};
    else delete this._events[event];
    return this;
  }
  listeners(event) { return (this._events[event] || []).slice(); }
  listenerCount(event) { return (this._events[event] || []).length; }
}
globalThis.__nodeEventEmitter = NodeEventEmitter;
globalThis.__nodeEventEmitter.once = (emitter, event) => new Promise((resolve) => emitter.once(event, (...args) => resolve(args)));
globalThis.__nodeEventEmitter.on = async function* (emitter, event) {
  const queue = []; let wake;
  emitter.on(event, (...args) => { queue.push(args); if (wake) { wake(); wake = undefined; } });
  while (true) { if (!queue.length) await new Promise((resolve) => { wake = resolve; }); yield queue.shift(); }
};
class NodeReadable extends NodeEventEmitter {
  static from(iterable) {
    const stream = new NodeReadable();
    queueMicrotask(() => { for (const chunk of iterable) stream.emit('data', chunk); stream.emit('end'); });
    return stream;
  }
  pipe(destination) { this.on('data', (chunk) => destination.write(chunk)); this.on('end', () => destination.end()); return destination; }
}
class NodeWritable extends NodeEventEmitter {
  write(chunk, encoding, callback) {
    if (typeof encoding === 'function') callback = encoding;
    this.emit('data', chunk);
    if (callback) queueMicrotask(callback);
    return true;
  }
  end(chunk, encoding, callback) { if (chunk !== undefined) this.write(chunk, encoding); if (callback) callback(); this.emit('finish'); }
}
class NodeTransform extends NodeWritable {
  constructor(options = {}) { super(); this._transform = options.transform; }
  write(chunk, encoding, callback) {
    if (this._transform) this._transform.call(this, chunk, encoding, () => callback && callback());
    else super.write(chunk, encoding, callback);
    return true;
  }
}
globalThis.__nodeStream = { Readable: NodeReadable, Writable: NodeWritable, Transform: NodeTransform, PassThrough: NodeTransform };
globalThis.__nodeFs = {
  constants: { F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1 },
  existsSync: (value) => globalThis.__quench_fs_exists(nodePathValue(value)),
  mkdtempSync: (prefix) => globalThis.__quench_fs_mkdtemp(nodePathValue(prefix)),
  readFileSync: (value) => globalThis.__quench_fs_read_file(nodePathValue(value)),
  writeFileSync: (value, data) => globalThis.__quench_fs_write_file(nodePathValue(value), String(data)),
  openSync: (value, flags = 'r', mode) => {
    const path = nodeFsPath(value);
    if (mode !== undefined && mode !== null && typeof mode !== 'number') {
      const error = new TypeError('The "mode" argument must be of type number');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const flag = String(flags);
    if (!/^[wax]/.test(flag) && !globalThis.__quench_fs_access(path)) {
      const error = new Error(`ENOENT: no such file or directory, open '${path}'`);
      error.code = 'ENOENT'; error.syscall = 'open'; error.path = path;
      throw error;
    }
    return globalThis.__quench_fs_open(path, flag);
  },
  closeSync: (_fd) => {},
  statSync: (value) => { const kind = globalThis.__quench_fs_kind(nodePathValue(value)); return { isFile: () => kind === 'file', isDirectory: () => kind === 'directory' }; },
  mkdirSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    if (options && Object.prototype.hasOwnProperty.call(options, 'recursive') && typeof options.recursive !== 'boolean') {
      const error = new TypeError('The "options.recursive" property must be of type boolean.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    let targetKind;
    try { targetKind = globalThis.__quench_fs_kind(path); } catch (_) { targetKind = undefined; }
    if (targetKind === 'file') {
      const error = new Error(`EEXIST: file already exists, mkdir '${path}'`);
      error.code = 'EEXIST'; error.syscall = 'mkdir'; error.path = path;
      throw error;
    }
    const parts = path.split('/').filter(Boolean);
    let prefix = path.startsWith('/') ? '' : '.';
    for (const part of parts.slice(0, -1)) {
      prefix += `/${part}`;
      let kind;
      try { kind = globalThis.__quench_fs_kind(prefix); } catch (_) { kind = undefined; }
      if (kind === 'file') {
        const error = new Error(`ENOTDIR: not a directory, mkdir '${path}'`);
        error.code = 'ENOTDIR'; error.syscall = 'mkdir'; error.path = path;
        throw error;
      }
    }
    try { return globalThis.__quench_fs_mkdir(path); }
    catch (_) {
      const error = new Error(`ENOENT: no such file or directory, mkdir '${path}'`);
      error.code = 'ENOENT'; error.syscall = 'mkdir'; error.path = path;
      throw error;
    }
  },
  readdirSync: (value) => {
    const path = nodeFsPath(value);
    let kind;
    try { kind = globalThis.__quench_fs_kind(path); } catch (_) { kind = undefined; }
    if (kind === 'file') {
      const error = new Error(`ENOTDIR: not a directory, scandir '${path}'`);
      error.code = 'ENOTDIR'; error.syscall = 'scandir'; error.path = path;
      throw error;
    }
    return globalThis.__quench_fs_readdir(path);
  },
  rmdirSync: (value) => globalThis.__quench_fs_remove_dir(String(value)),
  renameSync: (from, to) => globalThis.__quench_fs_rename(String(from), String(to)),
  unlinkSync: (value) => globalThis.__quench_fs_unlink(String(value)),
  copyFileSync: (from, to) => globalThis.__quench_fs_copy(String(from), String(to)),
  appendFileSync: (value, data) => globalThis.__quench_fs_append(String(value), String(data)),
  accessSync: (value) => { if (!globalThis.__quench_fs_access(String(value))) throw new Error('ENOENT'); },
  realpathSync: (value) => globalThis.__quench_fs_realpath(String(value)),
  rmSync: (value) => globalThis.__quench_fs_remove_dir(String(value)),
  chmodSync: (value, mode) => globalThis.__quench_fs_chmod(String(value), Number(mode)),
  symlinkSync: (target, link) => globalThis.__quench_fs_symlink(String(target), String(link)),
  readlinkSync: (value) => globalThis.__quench_fs_readlink(String(value)),
};
globalThis.__nodeFs.open = (value, flags, mode, callback) => {
  if (typeof flags === 'function') { callback = flags; flags = 'r'; mode = undefined; }
  else if (typeof mode === 'function') { callback = mode; mode = undefined; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (mode !== undefined && mode !== null && typeof mode !== 'number') {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.openSync(path, flags, mode)); } catch (error) { callback(error); } });
};
globalThis.__nodeFs.readdir = (value, options, callback) => {
  if (typeof options === 'function') callback = options;
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.readdirSync(path)); } catch (error) { callback(error); } });
};
globalThis.__nodeFs.mkdir = (value, options, callback) => {
  if (typeof options === 'function') { callback = options; options = {}; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (options && Object.prototype.hasOwnProperty.call(options, 'recursive') && typeof options.recursive !== 'boolean') {
    const error = new TypeError('The "options.recursive" property must be of type boolean.');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => { try { globalThis.__nodeFs.mkdirSync(path, options); callback(null); } catch (error) { callback(error); } });
};
globalThis.__nodeFs.readFile = (value, options, callback) => {
  if (typeof options === 'function') { callback = options; options = undefined; }
  queueMicrotask(() => {
    try { callback(null, globalThis.__nodeFs.readFileSync(value, options)); }
    catch (error) { callback(error); }
  });
};
globalThis.__nodeFs.mkdtemp = (prefix, options, callback) => {
  if (typeof options === 'function') callback = options;
  queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.mkdtempSync(prefix)); } catch (error) { callback(error); } });
};
globalThis.__nodeFs.writeFile = (value, data, options, callback) => {
  if (typeof options === 'function') callback = options;
  queueMicrotask(() => {
    try { globalThis.__nodeFs.writeFileSync(value, data); callback(null); }
    catch (error) { callback(error); }
  });
};
globalThis.__nodeFs.promises = {
  open: (value, flags = 'r', mode) => new Promise((resolve, reject) => globalThis.__nodeFs.open(value, flags, mode, (error, fd) => error ? reject(error) : resolve({ fd, close: () => Promise.resolve() }))),
  readFile: (value, options) => new Promise((resolve, reject) => globalThis.__nodeFs.readFile(value, options, (error, data) => error ? reject(error) : resolve(data))),
  writeFile: (value, data, options) => new Promise((resolve, reject) => globalThis.__nodeFs.writeFile(value, data, options, (error) => error ? reject(error) : resolve())),
  mkdir: (value) => Promise.resolve().then(() => globalThis.__nodeFs.mkdirSync(value)),
  readdir: (value) => Promise.resolve().then(() => globalThis.__nodeFs.readdirSync(value)),
  stat: (value) => Promise.resolve().then(() => globalThis.__nodeFs.statSync(value)),
};
globalThis.__nodeOs = {
  EOL: '\n',
  platform: () => process.platform,
  arch: () => process.arch,
  tmpdir: () => globalThis.__quench_tmpdir,
  homedir: () => globalThis.__quench_homedir,
  type: () => 'Quench',
  endianness: () => 'LE',
  hostname: () => globalThis.__quench_hostname,
  cpus: () => Array.from({ length: globalThis.__quench_cpu_count }, () => ({ model: 'unknown', speed: 0, times: { user: 0, nice: 0, sys: 0, idle: 0, irq: 0 } })),
  userInfo: () => ({ username: '', homedir: '/' }),
  constants: { signals: { SIGTERM: 15, SIGINT: 2 }, errno: { ENOENT: -2, EACCES: -13 } },
};
globalThis.__nodeUtil = {
  format: (...args) => {
    if (!args.length) return '';
    let index = 1;
    return String(args[0]).replace(/%[sdijo%]/g, (token) => {
      if (token === '%%') return '%';
      const value = args[index++];
      if (token === '%s') return String(value);
      if (token === '%d') return Number(value).toString();
      if (token === '%j') return JSON.stringify(value);
      return token === '%o' ? JSON.stringify(value) : String(value);
    }) + args.slice(index).map((value) => ` ${String(value)}`).join('');
  },
  inspect: (value) => JSON.stringify(value),
  types: {
    isDate: (value) => value instanceof Date,
    isPromise: (value) => value instanceof Promise,
  },
};
globalThis.__nodeQuerystring = {
  escape: (value) => encodeURIComponent(String(value)),
  unescape: (value) => decodeURIComponent(String(value)),
  stringify: (object, sep = '&', eq = '=') => Object.keys(object).map((key) => {
    const value = object[key];
    return (Array.isArray(value) ? value : [value]).map((item) =>
      encodeURIComponent(key) + eq + encodeURIComponent(String(item))).join(sep);
  }).join(sep),
  parse: (input, sep = '&', eq = '=') => String(input).split(sep).filter(Boolean).reduce((result, part) => {
    const index = part.indexOf(eq);
    const key = decodeURIComponent(index < 0 ? part : part.slice(0, index));
    const value = decodeURIComponent(index < 0 ? '' : part.slice(index + eq.length));
    result[key] = result[key] === undefined ? value : Array.isArray(result[key]) ? result[key].concat(value) : [result[key], value];
    return result;
  }, {}),
};
class NodeURLSearchParams {
  constructor(init = '') {
    this._pairs = [];
    if (typeof init === 'string') {
      init.replace(/^\?/, '').split('&').filter(Boolean).forEach((part) => {
        const i = part.indexOf('=');
        this.append(decodeURIComponent(i < 0 ? part : part.slice(0, i)), decodeURIComponent(i < 0 ? '' : part.slice(i + 1)));
      });
    } else Object.keys(init).forEach((key) => this.append(key, init[key]));
  }
  append(key, value) { this._pairs.push([String(key), String(value)]); }
  set(key, value) { this.delete(key); this.append(key, value); }
  get(key) { const pair = this._pairs.find(([name]) => name === String(key)); return pair ? pair[1] : null; }
  getAll(key) { return this._pairs.filter(([name]) => name === String(key)).map(([, value]) => value); }
  has(key) { return this._pairs.some(([name]) => name === String(key)); }
  delete(key) { this._pairs = this._pairs.filter(([name]) => name !== String(key)); }
  toString() { return this._pairs.map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`).join('&'); }
}
globalThis.__nodeURLSearchParams = NodeURLSearchParams;
globalThis.__nodeURL = class NodeURL {
  constructor(input, base) {
    let value = String(input);
    if (base && !/^[a-z][a-z0-9+.-]*:/.test(value)) {
      const baseUrl = new NodeURL(base);
      value = value.startsWith('/') ? baseUrl.origin + value : baseUrl.origin + baseUrl.pathname.replace(/\/[^/]*$/, '/') + value;
    }
    const match = value.match(/^([a-z][a-z0-9+.-]*:)?(?:\/\/([^/?#]*))?([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i);
    if (!match) throw new TypeError('Invalid URL');
    this.protocol = match[1] || '';
    this.host = match[2] || '';
    this.hostname = this.host.replace(/^.*@/, '').split(':')[0];
    this.port = this.host.includes(':') ? this.host.slice(this.host.lastIndexOf(':') + 1) : '';
    this.pathname = match[3] || '/';
    this.search = match[4] ? `?${match[4]}` : '';
    this.hash = match[5] ? `#${match[5]}` : '';
    this.origin = this.protocol && this.host ? `${this.protocol}//${this.host}` : 'null';
    this.searchParams = new NodeURLSearchParams(match[4] || '');
  }
  get href() { const query = this.searchParams.toString(); const prefix = this.protocol === 'file:' ? 'file://' : (this.origin === 'null' ? '' : this.origin); return `${prefix}${this.pathname}${query ? `?${query}` : this.search}${this.hash}`; }
  toString() { return this.href; }
};
globalThis.URL = globalThis.__nodeURL;
globalThis.URLSearchParams = globalThis.__nodeURLSearchParams;
globalThis.__nodeUrlModule = {
  URL: globalThis.__nodeURL,
  URLSearchParams: globalThis.__nodeURLSearchParams,
  fileURLToPath: (value) => {
    const href = String(value); if (!href.startsWith('file://')) throw new TypeError('URL must be a file URL');
    return decodeURIComponent(href.slice('file://'.length)) || '/';
  },
  pathToFileURL: (value) => new globalThis.__nodeURL(`file://${globalThis.__nodePath.resolve(String(value))}`),
  format: (value) => value instanceof globalThis.__nodeURL ? value.href : String(value),
  resolve: (from, to) => new globalThis.__nodeURL(to, from).href,
};
globalThis.__nodeCrypto = {
  randomUUID: () => globalThis.__quench_random_uuid(),
  createHash: (algorithm) => {
    if (algorithm !== 'sha256') throw new Error(`Unsupported hash: ${algorithm}`);
    let input = '';
    const hash = {
      update: (value) => { input += String(value); return hash; },
      digest: (encoding = 'hex') => {
        const result = globalThis.__quench_sha256(input);
        if (encoding === 'hex') return result;
        throw new Error(`Unsupported digest encoding: ${encoding}`);
      },
    };
    return hash;
  },
};
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, '');
  if (name === 'assert') return globalThis.__nodeAssert;
  if (name === 'path' || name === 'path/posix') return globalThis.__nodePath;
  if (name === 'util') return globalThis.__nodeUtil;
  if (name === 'os') return globalThis.__nodeOs;
  if (name === 'querystring') return globalThis.__nodeQuerystring;
  if (name === 'url') return globalThis.__nodeUrlModule;
  if (name === 'crypto') return globalThis.__nodeCrypto;
  if (name === 'events') return { EventEmitter: globalThis.__nodeEventEmitter, once: globalThis.__nodeEventEmitter.once, on: globalThis.__nodeEventEmitter.on };
  if (name === 'stream') return globalThis.__nodeStream;
  if (name === 'worker_threads') return { isMainThread: true };
  if (name === 'timers') return globalThis.__nodeTimers;
  if (name === 'timers/promises') return globalThis.__nodeTimersPromises;
  if (name === '../common' || name.endsWith('/common')) return globalThis.__nodeCommon;
  if (name.endsWith('/common/tmpdir')) return globalThis.__nodeTmpdir;
  if (name === 'buffer') return { Buffer: NodeBuffer, kMaxLength: 0x7fffffff, atob: nodeAtob, btoa: nodeBtoa };
  if (name === 'fs' || name === 'fs/promises') return globalThis.__nodeFs;
  throw new Error(`Cannot find module '${specifier}'`);
};
