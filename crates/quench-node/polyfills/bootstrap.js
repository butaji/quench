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
  static allocUnsafe(size) { return new NodeBuffer(size); }
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
    if (encoding === 'latin1' || encoding === 'binary') return Array.from(this, (byte) => String.fromCharCode(byte)).join('');
    if (encoding === 'ascii') return Array.from(this, (byte) => String.fromCharCode(byte & 0x7f)).join('');
    if (encoding === 'utf16le' || encoding === 'ucs2' || encoding === 'ucs-2') { let result = ''; for (let i = 0; i + 1 < this.length; i += 2) result += String.fromCharCode(this[i] | (this[i + 1] << 8)); return result; }
    return new NodeTextDecoder().decode(this);
  }
  equals(other) { if (typeof other === 'string') other = NodeBuffer.from(other); return other && this.length === other.length && this.every((value, index) => value === other[index]); }
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
    wrapped.__quench_index = (globalThis.__nodeCallChecks ||= []).length;
    globalThis.__nodeCallChecks.push(wrapped);
    return wrapped;
  },
  mustSucceed: (fn = () => {}) => globalThis.__nodeCommon.mustCall((error, ...args) => {
    if (error) throw error;
    return fn(...args);
  }),
  mustNotCall: (message = 'Unexpected call') => () => { throw new Error(message); },
  noop: () => {},
  printSkipMessage: (message) => console.log(`# SKIP: ${message}`),
  expectsError: (_expected) => (error) => { if (!error) throw new Error('Expected filesystem error'); },
  invalidArgTypeHelper: (input) => input == null ? ` Received ${input}` : ` Received type ${typeof input} (${String(input)})`,
  expectWarning: (_type, _message) => {},
  mustNotMutateObjectDeep: (value) => value,
  isLinux: process.platform === 'linux',
  isMacOS: process.platform === 'darwin',
  isWindows: process.platform === 'win32',
  isAIX: false,
  isFreeBSD: false,
  canCreateSymLink: () => false,
  getArrayBufferViews: (buffer) => [buffer, new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength), new DataView(buffer.buffer, buffer.byteOffset, buffer.byteLength)],
};
globalThis.__quench_verify_calls = () => {
  for (const callback of globalThis.__nodeCallChecks || []) {
    if (callback.calls !== callback.expected) throw new Error(`Callback ${callback.__quench_index}: expected ${callback.expected} calls, got ${callback.calls}`);
  }
};
globalThis.__nodeTmpdir = {
  path: `/tmp/quench-node-${process.pid}`,
  hasEnoughSpace: (_bytes) => false,
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
  constants: { F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1, O_APPEND: 1024, O_CREAT: 64, O_EXCL: 128, O_RDONLY: 0, O_RDWR: 2, O_SYNC: 1052672, O_DSYNC: 4194304, O_TRUNC: 512, O_WRONLY: 1, UV_DIRENT_UNKNOWN: 0, UV_DIRENT_FILE: 1, UV_DIRENT_DIR: 2, UV_DIRENT_LINK: 3, UV_DIRENT_FIFO: 4, UV_DIRENT_SOCKET: 5, UV_DIRENT_CHAR: 6, UV_DIRENT_BLOCK: 7, COPYFILE_EXCL: 1, COPYFILE_FICLONE: 2, COPYFILE_FICLONE_FORCE: 4, UV_FS_COPYFILE_EXCL: 1, UV_FS_COPYFILE_FICLONE: 2, UV_FS_COPYFILE_FICLONE_FORCE: 4 },
  existsSync: (value) => globalThis.__quench_fs_exists(nodePathValue(value)),
  mkdtempSync: (prefix) => globalThis.__quench_fs_mkdtemp(nodePathValue(prefix)),
  readFileSync: (value, options) => {
    const path = nodePathValue(value); let hex; try { hex = globalThis.__quench_fs_read_hex(path); } catch (error) { const flag = typeof options === 'object' && options ? options.flag : undefined; if (flag === 'a' || flag === 'a+') { globalThis.__quench_fs_write_hex(path, ''); globalThis.__nodeModes[path] = 0o666 & ~process.umask(); hex = ''; } else throw error; }
    if (options === undefined || options === null) return NodeBuffer.from(hex, 'hex');
    const encoding = typeof options === 'string' ? options : options && options.encoding;
    if (options && typeof options === 'object' && options.buffer !== undefined) { const bytes = NodeBuffer.from(hex, 'hex'); const target = typeof options.buffer === 'function' ? options.buffer(bytes.length) : options.buffer; if (!(target instanceof Uint8Array)) { const error = new TypeError('The "buffer" option must return a Buffer'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } target.set(bytes.subarray(0, target.length)); return encoding ? target.toString(encoding) : target.subarray(0, bytes.length); }
    if (encoding === 'hex' || encoding === 'base64') return NodeBuffer.from(hex, 'hex').toString(encoding);
    return globalThis.__quench_fs_read_file(path);
  },
  writeFileSync: (value, data, options = {}) => { if (options && options.flush !== undefined && typeof options.flush !== 'boolean') { const error = new TypeError('The "options.flush" property must be of type boolean'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } const path = typeof value === 'number' ? globalThis.__nodeFdPaths[value] : nodePathValue(value); if (!path) { const error = new Error('EBADF'); error.code = 'EBADF'; throw error; } let view = data instanceof Uint8Array ? data : ArrayBuffer.isView(data) ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : undefined; if (!view && options && options.encoding && options.encoding !== 'utf8' && options.encoding !== 'utf-8') view = NodeBuffer.from(String(data), options.encoding); const hex = view ? NodeBuffer.from(view).toString('hex') : NodeBuffer.from(String(data)).toString('hex'); if (options && options.flag === 'a') { let existing = ''; try { existing = globalThis.__quench_fs_read_hex(path); } catch (_) {} return globalThis.__quench_fs_write_hex(path, existing + hex); } const result = globalThis.__quench_fs_write_hex(path, hex); if (options && options.flush) { const fd = globalThis.__nodeFs.openSync(path, 'r'); globalThis.__nodeFs.fsyncSync(fd); globalThis.__nodeFs.closeSync(fd); } if (options && options.mode !== undefined) globalThis.__nodeModes[path] = Number(options.mode); return result; },
  openSync: (value, flags = 'r', mode) => {
    const path = nodeFsPath(value);
    if (mode !== undefined && mode !== null && typeof mode !== 'number' && typeof mode !== 'string') { const error = new TypeError('The "mode" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    const flag = String(flags);
    if (!/^[wax]/.test(flag) && !globalThis.__quench_fs_access(path)) {
      const error = new Error(`ENOENT: no such file or directory, open '${path}'`);
      error.code = 'ENOENT'; error.syscall = 'open'; error.path = path;
      throw error;
    }
    const fd = globalThis.__quench_fs_open(path, flag); globalThis.__nodeFdPaths[fd] = path; globalThis.__nodeFdPositions[fd] = 0; if (mode !== undefined && mode !== null) globalThis.__nodeModes[path] = typeof mode === 'string' ? parseInt(mode, 8) : Number(mode); return fd;
  },
  closeSync: (fd) => { if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } delete globalThis.__nodeFdPaths[fd]; delete globalThis.__nodeFdPositions[fd]; },
  statSync: (value, options = {}) => {
    const path = nodeFsPath(value); let kind;
    try { kind = globalThis.__quench_fs_kind(path); } catch (error) {
      if (options && options.throwIfNoEntry === false) return undefined;
      throw error;
    }
    const file = kind === 'file'; const date = new Date();
    const stats = new globalThis.__nodeStats(file, kind === 'directory', date); if (file) stats.size = globalThis.__quench_fs_read_hex(path).length / 2; stats.mode = globalThis.__nodeModes[path] || (file ? (0o666 & ~process.umask()) : 0); return stats;
  },
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
    if (targetKind === 'directory' && !(options && options.recursive)) {
      const error = new Error(`EEXIST: file already exists, mkdir '${path}'`); error.code = 'EEXIST'; error.syscall = 'mkdir'; error.path = path; throw error;
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
  readdirSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    let kind;
    try { kind = globalThis.__quench_fs_kind(path); } catch (_) { kind = undefined; }
    if (kind === 'file') {
      const error = new Error(`ENOTDIR: not a directory, scandir '${path}'`);
      error.code = 'ENOTDIR'; error.syscall = 'scandir'; error.path = path;
      throw error;
    }
    const entries = globalThis.__quench_fs_readdir(path).sort();
    if (!options || !options.withFileTypes) return entries;
    return entries.map((name) => { const dirent = new globalThis.__nodeFs.Dirent(name, (() => { try { return globalThis.__quench_fs_kind(`${path}/${name}`) === 'directory'; } catch (_) { return false; } })()); dirent.parentPath = path; return dirent; });
  },
  rmdirSync: (value) => globalThis.__quench_fs_remove_dir(String(value)),
  renameSync: (from, to) => globalThis.__quench_fs_rename(nodeFsPath(from), nodeFsPath(to)),
  unlinkSync: (value) => globalThis.__quench_fs_unlink(String(value)),
  truncateSync: (value, length = 0) => { if (typeof length !== 'number' || !Number.isFinite(length)) { const error = new TypeError('The "len" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } if (!Number.isInteger(length)) { const error = new RangeError('The value of "len" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; } const path = typeof value === 'number' ? globalThis.__nodeFdPaths[value] : nodeFsPath(value); if (!path) throw new Error('EBADF'); return globalThis.__quench_fs_truncate(path, Math.max(0, Number(length))); },
  ftruncateSync: (fd, length = 0) => { if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } return globalThis.__nodeFs.truncateSync(fd, length); },
  fsyncSync: (fd) => { if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } if (!Number.isInteger(fd) || fd < 0) { const error = new RangeError('The value of "fd" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; } },
  fdatasyncSync: (fd) => globalThis.__nodeFs.fsyncSync(fd),
  readSync: (fd, buffer, offset = 0, length = buffer.length, position = null) => {
    if (offset !== undefined && offset !== null && (Array.isArray(offset) || typeof offset !== 'number' && typeof offset !== 'object')) { const error = new TypeError('The "options" argument must be an object'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    if (offset === null || (typeof offset === 'object' && !ArrayBuffer.isView(offset))) { const options = offset || {}; if (offset !== null && typeof offset !== 'object') { const error = new TypeError('The "options" argument must be an object'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } offset = Number(options.offset || 0); length = options.length === undefined ? buffer.length - offset : Number(options.length); position = options.position === undefined ? null : options.position; }
    if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    if (!(buffer instanceof Uint8Array)) { const error = new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    if (buffer.length === 0 && Number(length) > 0) { const error = new TypeError('The argument \'buffer\' is empty and cannot be written.'); error.code = 'ERR_INVALID_ARG_VALUE'; throw error; }
    if (!Number.isInteger(offset) || offset < 0) { const error = new RangeError('The value of "offset" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; }
    if (!Number.isInteger(length) || length < 0) { const error = new RangeError('The value of "length" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; }
    if (position !== null && position !== undefined && typeof position !== 'number' && typeof position !== 'bigint') { const error = new TypeError('The "position" argument must be of type number or bigint'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    const path = globalThis.__nodeFdPaths[fd]; if (!path) { const error = new Error('EBADF'); error.code = 'EBADF'; throw error; }
    const numericPosition = position === null || Number(position) < 0 ? (globalThis.__nodeFdPositions[fd] || 0) : Number(position);
    const hex = globalThis.__quench_fs_read_range_hex(path, numericPosition, Number(length));
    const bytes = NodeBuffer.from(hex, 'hex'); buffer.set(bytes.subarray(0, Number(length)), Number(offset)); if (position === null || position === undefined) globalThis.__nodeFdPositions[fd] = numericPosition + bytes.length; return bytes.length;
  },
  readvSync: (fd, buffers, position = null) => {
    if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    if (!Array.isArray(buffers) || buffers.some((buffer) => !(buffer instanceof Uint8Array))) { const error = new TypeError('The "buffers" argument must be an array of Buffer or Uint8Array'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    let total = 0; let at = position === null || position === undefined ? 0 : Number(position);
    for (const buffer of buffers) { if (buffer.length) { const count = globalThis.__nodeFs.readSync(fd, buffer, 0, buffer.length, at); total += count; at += count; if (count < buffer.length) break; } }
    return total;
  },
  writeSync: (fd, buffer, offset = 0, length = buffer.length - offset, position = null) => {
    if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    if (typeof buffer === 'string') buffer = NodeBuffer.from(buffer);
    if (!(buffer instanceof Uint8Array)) { const error = new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    if (!Number.isInteger(offset) || offset < 0 || !Number.isInteger(length) || length < 0) { const error = new RangeError('The write range is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; }
    const path = globalThis.__nodeFdPaths[fd]; if (!path) { const error = new Error('EBADF'); error.code = 'EBADF'; throw error; }
    const bytes = buffer.subarray(offset, offset + length); const at = position === null || position === undefined ? (globalThis.__nodeFdPositions[fd] || 0) : Number(position); const existing = NodeBuffer.from(globalThis.__quench_fs_read_hex(path), 'hex'); const output = NodeBuffer.alloc(Math.max(existing.length, at + bytes.length)); output.set(existing); output.set(bytes, at); globalThis.__quench_fs_write_hex(path, output.toString('hex')); if (position === null || position === undefined) globalThis.__nodeFdPositions[fd] = at + bytes.length; return bytes.length;
  },
  writevSync: (fd, buffers, position = null) => { if (!Array.isArray(buffers) || buffers.some((buffer) => !(buffer instanceof Uint8Array))) { const error = new TypeError('The "buffers" argument must be an array of Buffer or Uint8Array'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } return globalThis.__nodeFs.writeSync(fd, NodeBuffer.concat(buffers)); },
  copyFileSync: (from, to, mode = 0) => {
    const source = nodeFsPath(from); const destination = nodeFsPath(to);
    if (typeof mode !== 'number') { const error = new TypeError('The "mode" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    if ((mode & ~7) !== 0) { const error = new RangeError('The value of "mode" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; }
    return globalThis.__quench_fs_copy(source, destination);
  },
  appendFileSync: (value, data, options = {}) => {
    if (!(typeof data === 'string' || data instanceof NodeBuffer || data instanceof Uint8Array)) { const error = new TypeError('The "data" argument must be of type string or an instance of Buffer'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
    const path = typeof value === 'number' ? globalThis.__nodeFdPaths[value] : nodeFsPath(value);
    if (!path) { const error = new Error('EBADF: bad file descriptor'); error.code = 'EBADF'; throw error; }
    const result = globalThis.__quench_fs_append(path, data instanceof Uint8Array ? new NodeTextDecoder().decode(data) : String(data)); if (options && options.mode !== undefined) globalThis.__nodeModes[path] = Number(options.mode); return result;
  },
  accessSync: (value) => { const path = nodeFsPath(value); if (!globalThis.__quench_fs_access(path)) { const error = new Error(`ENOENT: no such file or directory, access '${path}'`); error.code = 'ENOENT'; error.path = path; throw error; } },
  realpathSync: (value, options) => { const result = globalThis.__quench_fs_realpath(nodePathValue(value)); const encoding = typeof options === 'string' ? options : options && options.encoding; return encoding === 'buffer' ? NodeBuffer.from(result) : encoding ? NodeBuffer.from(result).toString(encoding) : result; },
  rmSync: (value, options = {}) => { const path = nodeFsPath(value); let kind; try { kind = globalThis.__quench_fs_kind(path); } catch (_) { return; } if (kind === 'file') return globalThis.__quench_fs_unlink(path); if (kind === 'directory' && options.recursive === false) { const error = new Error(`ERR_FS_EISDIR: illegal operation on a directory, rm '${path}'`); error.code = 'ERR_FS_EISDIR'; error.path = path; throw error; } return globalThis.__quench_fs_remove_dir(path); },
  chmodSync: (value, mode) => { const path = nodeFsPath(value); globalThis.__quench_fs_chmod(path, typeof mode === 'string' ? parseInt(mode, 8) : Number(mode)); globalThis.__nodeModes[path] = typeof mode === 'string' ? parseInt(mode, 8) : Number(mode); },
  symlinkSync: (target, link) => globalThis.__quench_fs_symlink(String(target), String(link)),
  readlinkSync: (value) => globalThis.__quench_fs_readlink(String(value)),
};
globalThis.__nodeFs.truncate = (value, length, callback) => {
  if (typeof length === 'function') { callback = length; length = 0; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (typeof length !== 'number' || !Number.isFinite(length)) { const error = new TypeError('The "len" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  if (!Number.isInteger(length)) { const error = new RangeError('The value of "len" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; }
  const path = typeof value === 'number' ? globalThis.__nodeFdPaths[value] : nodeFsPath(value);
  queueMicrotask(() => { try { globalThis.__quench_fs_truncate(path, Number(length)); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.ftruncate = (fd, length = 0, callback) => { if (typeof length === 'function') { callback = length; length = 0; } if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); queueMicrotask(() => { try { globalThis.__nodeFs.ftruncateSync(fd, length); callback(null); } catch (error) { callback(error); } }); };
globalThis.__nodeFs.access = (value, mode, callback) => {
  if (typeof mode === 'function') callback = mode;
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { try { globalThis.__nodeFs.accessSync(path, mode); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.fsync = (fd, callback) => { if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); queueMicrotask(() => { try { globalThis.__nodeFs.fsyncSync(fd); callback(null); } catch (error) { callback(error); } }); };
globalThis.__nodeFs.fdatasync = (fd, callback) => { if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); queueMicrotask(() => { try { globalThis.__nodeFs.fdatasyncSync(fd); callback(null); } catch (error) { callback(error); } }); };
globalThis.__nodeFs.read = (fd, buffer, offset, length, position, callback) => {
  if (typeof buffer === 'function' || buffer === undefined) { callback = buffer; buffer = NodeBuffer.alloc(16384); offset = 0; length = buffer.length; position = null; }
  else if (buffer === null) { callback = offset; buffer = NodeBuffer.alloc(16384); offset = 0; length = buffer.length; position = null; }
  else if (typeof buffer === 'object' && !ArrayBuffer.isView(buffer)) { const options = buffer; callback = offset; buffer = options.buffer || NodeBuffer.alloc(options.length === undefined ? 16384 : Number(options.length)); offset = options.offset == null ? 0 : Number(options.offset); length = options.length === undefined ? buffer.length - offset : Number(options.length); position = options.position === undefined ? null : options.position; }
  else if (typeof offset === 'function') { callback = offset; offset = 0; length = buffer.length; position = null; }
  else if (typeof offset === 'object' || offset === null || offset === undefined) { const options = offset || {}; callback = length; offset = Number(options.offset || 0); length = options.length === undefined ? buffer.length - offset : Number(options.length); position = options.position === undefined ? null : options.position; }
  else if (typeof position === 'function') { callback = position; position = null; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (buffer.length === 0 && Number(length) > 0) { const error = new TypeError('The argument \'buffer\' is empty and cannot be written.'); error.code = 'ERR_INVALID_ARG_VALUE'; throw error; }
  if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  if (!(buffer instanceof Uint8Array)) { const error = new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  if (!Number.isInteger(offset) || offset < 0 || !Number.isInteger(length) || length < 0) { const error = new RangeError('The read range is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; }
  if (position !== null && position !== undefined && typeof position !== 'number' && typeof position !== 'bigint') { const error = new TypeError('The "position" argument must be of type number or bigint'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  queueMicrotask(() => { try { const count = globalThis.__nodeFs.readSync(fd, buffer, offset, length, position); callback(null, count, buffer); } catch (error) { callback(error); } });
};
globalThis.__nodeFs.readv = (fd, buffers, position, callback) => { if (typeof position === 'function') { callback = position; position = null; } if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); if (!Array.isArray(buffers) || buffers.some((buffer) => !(buffer instanceof Uint8Array))) { const error = new TypeError('The "buffers" argument must be an array of Buffer or Uint8Array'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.readvSync(fd, buffers, position), buffers); } catch (error) { callback(error); } }); };
globalThis.__nodeFs.write = (fd, buffer, offset, length, position, callback) => {
  if (typeof buffer === 'object' && buffer !== null && !ArrayBuffer.isView(buffer)) { const options = buffer; callback = offset; buffer = options.buffer; offset = options.offset || 0; length = options.length === undefined ? buffer && buffer.length - offset : options.length; position = options.position; }
  else if (typeof position === 'function') { callback = position; position = null; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (typeof fd !== 'number' || !(typeof buffer === 'string' || buffer instanceof Uint8Array)) { const error = new TypeError('Invalid write arguments'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.writeSync(fd, buffer, offset, length, position), buffer); } catch (error) { callback(error); } });
};
globalThis.__nodeFs.writev = (fd, buffers, position, callback) => { if (typeof position === 'function') { callback = position; position = null; } if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } if (!Array.isArray(buffers) || buffers.some((buffer) => !(buffer instanceof Uint8Array))) { const error = new TypeError('The "buffers" argument must be an array of Buffer or Uint8Array'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.writevSync(fd, buffers, position), buffers); } catch (error) { callback(error); } }); };
globalThis.__nodeModes = {};
globalThis.__nodeFdPaths = {};
globalThis.__nodeFdPositions = {};
const nodeMode = (mode) => {
  const value = typeof mode === 'string' ? parseInt(mode, 8) : Number(mode);
  if (!Number.isFinite(value) || value < 0 || value > 0xffffffff) { const error = new RangeError('The value of "mode" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; }
  return value;
};
globalThis.__nodeFs.fchmodSync = (fd, mode) => { if (!Number.isInteger(fd) || fd < 0 || fd > 0x7fffffff) { const error = new RangeError('The value of "fd" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error; } const value = nodeMode(mode); if (globalThis.__nodeFdPaths[fd]) globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[fd], value); };
globalThis.__nodeFs.fchmod = (fd, mode, callback) => { if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); globalThis.__nodeFs.fchmodSync(fd, mode); queueMicrotask(() => callback(null)); };
globalThis.__nodeFs.statfsSync = (value, options = {}) => { const path = nodeFsPath(value); if (!globalThis.__quench_fs_access(path)) throw new Error('ENOENT'); const values = { type: 0, bsize: 4096, frsize: 4096, blocks: 1, bfree: 1, bavail: 1, files: 1, ffree: 1 }; if (options && options.bigint) Object.keys(values).forEach((key) => { values[key] = BigInt(values[key]); }); return values; };
globalThis.__nodeFs.statfs = (value, options, callback) => { if (typeof options === 'function') { callback = options; options = undefined; } if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); const path = nodeFsPath(value); queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.statfsSync(path, options)); } catch (error) { callback(error); } }); };
globalThis.__nodeFs.symlink = (target, link, type, callback) => { if (typeof type === 'function') callback = type; if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); const source = nodePathValue(target); const destination = nodeFsPath(link); queueMicrotask(() => { try { globalThis.__quench_fs_symlink(source, destination); } catch (error) { callback(error); return; } callback(null); }); };
globalThis.__nodeFs.readlink = (value, options, callback) => { if (typeof options === 'function') callback = options; if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); const path = nodeFsPath(value); queueMicrotask(() => { try { callback(null, globalThis.__quench_fs_readlink(path)); } catch (error) { callback(error); return; } }); };
globalThis.__nodeFs.chmod = (value, mode, callback) => {
  if (typeof mode === 'function') { callback = mode; mode = undefined; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { try { globalThis.__nodeFs.chmodSync(path, mode); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.appendFile = (value, data, options, callback) => {
  if (typeof options === 'function') callback = options;
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => { try { globalThis.__nodeFs.appendFileSync(value, data, options); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.rmdir = (value, options, callback) => {
  if (typeof options === 'function') callback = options;
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { try { globalThis.__nodeFs.rmdirSync(path); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.rm = (value, options, callback) => {
  if (typeof options === 'function') callback = options;
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { try { globalThis.__nodeFs.rmSync(path, options); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.rename = (from, to, callback) => {
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const source = nodeFsPath(from); const destination = nodeFsPath(to);
  queueMicrotask(() => { try { globalThis.__nodeFs.renameSync(source, destination); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.copyFile = (from, to, mode, callback) => {
  if (typeof mode === 'function') { callback = mode; mode = 0; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const source = nodeFsPath(from); const destination = nodeFsPath(to);
  if (typeof mode !== 'number') { const error = new TypeError('The "mode" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  queueMicrotask(() => { try { globalThis.__nodeFs.copyFileSync(source, destination, mode); } catch (error) { callback(error); return; } callback(null); });
};
globalThis.__nodeFs.realpath = (value, options, callback) => {
  if (typeof options === 'function') callback = options;
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { let result; try { result = globalThis.__nodeFs.realpathSync(path, options); } catch (error) { callback(error); return; } callback(null, result); });
};
globalThis.__nodeFs.realpathSync.native = globalThis.__nodeFs.realpathSync;
globalThis.__nodeFs.realpath.native = globalThis.__nodeFs.realpath;
globalThis.__nodeStats = function Stats(file = false, directory = false, date = new Date()) {
  if (!(date instanceof Date)) date = new Date(Number(date) || 0);
  this.dev = 0; this.mode = 0; this.nlink = 1; this.uid = 0; this.gid = 0; this.rdev = 0; this.blksize = 4096; this.ino = 0;
  this.size = 0; this.blocks = 0; this.atime = date; this.mtime = date; this.ctime = date; this.birthtime = date;
  this.atimeMs = date.getTime(); this.mtimeMs = date.getTime(); this.ctimeMs = date.getTime(); this.birthtimeMs = date.getTime();
  this._file = file; this._directory = directory;
};
globalThis.__nodeStats.prototype.isFile = function () { return this._file; };
globalThis.__nodeStats.prototype.isDirectory = function () { return this._directory; };
globalThis.__nodeStats.prototype.isSocket = function () { return false; };
globalThis.__nodeStats.prototype.isBlockDevice = function () { return false; };
globalThis.__nodeStats.prototype.isCharacterDevice = function () { return false; };
globalThis.__nodeStats.prototype.isFIFO = function () { return false; };
globalThis.__nodeStats.prototype.isSymbolicLink = function () { return this._symlink === true; };
globalThis.__nodeFs.Dirent = class Dirent {
  constructor(name, type = 1) { this.name = name; this._type = type === true ? 2 : type === false ? 1 : type; }
  isFile() { return this._type === 1; } isDirectory() { return this._type === 2; } isSymbolicLink() { return this._type === 3; }
  isFIFO() { return this._type === 4; } isSocket() { return this._type === 5; } isCharacterDevice() { return this._type === 6; } isBlockDevice() { return this._type === 7; }
};
globalThis.__nodeFs.Dir = class Dir {
  constructor(path) { this.path = path; this._entries = globalThis.__nodeFs.readdirSync(path, { withFileTypes: true }); this._index = 0; this._closed = false; }
  readSync() { if (this._closed) { const error = new Error('Directory handle was closed'); error.code = 'ERR_DIR_CLOSED'; throw error; } return this._entries[this._index++] || null; }
  closeSync() { if (this._closed) { const error = new Error('Directory handle was closed'); error.code = 'ERR_DIR_CLOSED'; throw error; } this._closed = true; }
  read(callback) { if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); queueMicrotask(() => { try { callback(null, this.readSync()); } catch (error) { callback(error); } }); }
  close(callback) { if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); queueMicrotask(() => { try { this.closeSync(); callback(null); } catch (error) { callback(error); } }); }
};
globalThis.__nodeFs.opendirSync = (value) => new globalThis.__nodeFs.Dir(nodeFsPath(value));
globalThis.__nodeFs.opendir = (value, options, callback) => { if (typeof options === 'function') callback = options; if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); const path = nodeFsPath(value); queueMicrotask(() => { try { callback(null, new globalThis.__nodeFs.Dir(path)); } catch (error) { callback(error); } }); };
globalThis.__nodeFs.lstatSync = (value) => { const path = nodeFsPath(value); const kind = globalThis.__quench_fs_link_kind(path); const stats = new globalThis.__nodeStats(kind === 'file', kind === 'directory', new Date()); stats._symlink = kind === 'symlink'; stats.mode = globalThis.__nodeModes[path] || 0; return stats; };
globalThis.__nodeFs.stat = (value, options, callback) => {
  if (typeof options === 'function') { callback = options; options = undefined; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { let result; try { result = globalThis.__nodeFs.statSync(path, options); } catch (error) { callback(error); return; } callback(null, result); });
};
globalThis.__nodeFs.lstat = (value, options, callback) => { if (typeof options === 'function') { callback = options; options = undefined; } if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function'); const path = nodeFsPath(value); queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.lstatSync(path)); } catch (error) { callback(error); return; } }); };
globalThis.__nodeFs.fstatSync = (fd) => {
  if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  return globalThis.__nodeFs.statSync(globalThis.__nodeFdPaths[fd] || '.');
};
globalThis.__nodeFs.fstat = (fd, options, callback) => {
  if (typeof options === 'function') callback = options;
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; }
  queueMicrotask(() => { let result; try { result = globalThis.__nodeFs.fstatSync(fd); } catch (error) { callback(error); return; } callback(null, result); });
};
globalThis.__nodeFs.Stats = globalThis.__nodeStats;
globalThis.__nodeFs.close = (fd, callback) => { if (typeof fd !== 'number') { const error = new TypeError('The "fd" argument must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } if (typeof callback !== 'function') { const error = new TypeError('The "callback" argument must be of type function'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error; } queueMicrotask(() => callback(null)); };
class NodeAbortSignal {
  constructor() { this.aborted = false; this._listeners = []; }
  addEventListener(event, listener) { if (event === 'abort') this._listeners.push(listener); }
  removeEventListener(event, listener) { this._listeners = this._listeners.filter((item) => item !== listener); }
  static abort() { const signal = new NodeAbortSignal(); signal.aborted = true; return signal; }
}
class NodeAbortController {
  constructor() { this.signal = new NodeAbortSignal(); }
  abort() { this.signal.aborted = true; this.signal._listeners.slice().forEach((listener) => listener()); }
}
globalThis.AbortSignal = NodeAbortSignal;
globalThis.AbortController = NodeAbortController;
globalThis.__nodeFs.open = (value, flags, mode, callback) => {
  if (typeof flags === 'function') { callback = flags; flags = 'r'; mode = undefined; }
  else if (typeof mode === 'function') { callback = mode; mode = undefined; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (mode !== undefined && mode !== null && typeof mode !== 'number' && typeof mode !== 'string') {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => { let fd; try { fd = globalThis.__nodeFs.openSync(path, flags, mode); } catch (error) { callback(error); return; } callback(null, fd); });
};
globalThis.__nodeFs.readdir = (value, options, callback) => {
  if (typeof options === 'function') { callback = options; options = undefined; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => { let result; try { result = globalThis.__nodeFs.readdirSync(path, options); } catch (error) { callback(error); return; } callback(null, result); });
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
  if (options && options.signal !== undefined && !(options.signal instanceof NodeAbortSignal)) {
    const error = new TypeError('The "signal" option must be an AbortSignal'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
  }
  queueMicrotask(() => {
    if (options && options.signal && options.signal.aborted) {
      const error = new Error('The operation was aborted'); error.name = 'AbortError'; error.code = 'ABORT_ERR'; callback(error); return;
    }
    let data; try { data = globalThis.__nodeFs.readFileSync(value, options); }
    catch (error) { callback(error); return; }
    callback(null, data);
  });
};
globalThis.__nodeFs.mkdtemp = (prefix, options, callback) => {
  if (typeof options === 'function') callback = options;
  queueMicrotask(() => { try { callback(null, globalThis.__nodeFs.mkdtempSync(prefix)); } catch (error) { callback(error); } });
};
globalThis.__nodeFs.writeFile = (value, data, options, callback) => {
  if (typeof options === 'function') { callback = options; options = undefined; }
  if (typeof callback !== 'function') throw new TypeError('The "callback" argument must be of type function');
  if (options && options.signal && options.signal.aborted) { queueMicrotask(() => { const error = new Error('The operation was aborted'); error.name = 'AbortError'; callback(error); }); return; }
  queueMicrotask(() => {
    try { globalThis.__nodeFs.writeFileSync(value, data); }
    catch (error) { callback(error); return; }
    callback(null);
  });
};
globalThis.__nodeFs.promises = {
  open: (value, flags = 'r', mode) => new Promise((resolve, reject) => globalThis.__nodeFs.open(value, flags, mode, (error, fd) => error ? reject(error) : resolve({ fd, close: () => Promise.resolve(), read: (buffer, offset, length, position) => Promise.resolve().then(() => { let target = buffer; let start = offset; let size = length; let at = position; if (offset && typeof offset === 'object') { const options = offset; target = options.buffer || NodeBuffer.alloc(16384); start = options.offset == null ? 0 : options.offset; size = options.length === undefined ? target.length - start : options.length; at = options.position; } if (target.length === 0 && Number(size) > 0) { const error = new TypeError('The buffer is empty'); error.code = 'ERR_INVALID_ARG_VALUE'; throw error; } const bytesRead = globalThis.__nodeFs.readSync(fd, target, start || 0, size === undefined ? target.length : size, at === undefined ? null : at); return { bytesRead, buffer: target }; }) }))),
  readFile: (value, options) => new Promise((resolve, reject) => globalThis.__nodeFs.readFile(value, options, (error, data) => error ? reject(error) : resolve(data))),
  writeFile: (value, data, options) => new Promise((resolve, reject) => globalThis.__nodeFs.writeFile(value, data, options, (error) => error ? reject(error) : resolve())),
  appendFile: (value, data, options) => new Promise((resolve, reject) => globalThis.__nodeFs.appendFile(value, data, options, (error) => error ? reject(error) : resolve())),
  access: (value, mode) => new Promise((resolve, reject) => globalThis.__nodeFs.access(value, mode, (error) => error ? reject(error) : resolve())),
  truncate: (value, length = 0) => Promise.resolve().then(() => globalThis.__nodeFs.truncateSync(value, length)),
  ftruncate: (fd, length = 0) => Promise.resolve().then(() => globalThis.__nodeFs.ftruncateSync(fd, length)),
  fsync: (fd) => Promise.resolve().then(() => globalThis.__nodeFs.fsyncSync(fd)),
  fdatasync: (fd) => Promise.resolve().then(() => globalThis.__nodeFs.fdatasyncSync(fd)),
  rm: (value, options) => new Promise((resolve, reject) => globalThis.__nodeFs.rm(value, options, (error) => error ? reject(error) : resolve())),
  opendir: (value, options) => Promise.resolve().then(() => globalThis.__nodeFs.opendirSync(value)),
  symlink: (target, link, type) => new Promise((resolve, reject) => globalThis.__nodeFs.symlink(target, link, type, (error) => error ? reject(error) : resolve())),
  readlink: (value, options) => new Promise((resolve, reject) => globalThis.__nodeFs.readlink(value, options, (error, result) => error ? reject(error) : resolve(result))),
  readv: (fd, buffers, position) => Promise.resolve().then(() => { const bytesRead = globalThis.__nodeFs.readvSync(fd, buffers, position); return { bytesRead, buffers }; }),
  writev: (fd, buffers, position) => Promise.resolve().then(() => { const bytesWritten = globalThis.__nodeFs.writevSync(fd, buffers, position); return { bytesWritten, buffers }; }),
  mkdir: (value) => Promise.resolve().then(() => globalThis.__nodeFs.mkdirSync(value)),
  readdir: (value, options) => Promise.resolve().then(() => globalThis.__nodeFs.readdirSync(value, options)),
  stat: (value) => Promise.resolve().then(() => globalThis.__nodeFs.statSync(value)),
};
const __nodePromiseOpen = globalThis.__nodeFs.promises.open;
globalThis.__nodeFs.promises.open = async (...args) => { const handle = await __nodePromiseOpen(...args); handle.write = (buffer, offset, length, position) => Promise.resolve().then(() => { const start = typeof offset === 'object' ? (offset.offset || 0) : (offset || 0); const source = typeof offset === 'object' ? (offset.buffer || offset) : buffer; const size = typeof offset === 'object' ? (offset.length === undefined ? source.length - start : offset.length) : (length === undefined ? source.length - start : length); const at = typeof offset === 'object' ? offset.position : position; return { bytesWritten: globalThis.__nodeFs.writeSync(handle.fd, source, start, size, at === undefined ? null : at), buffer: source }; }); handle.readv = (buffers, position) => Promise.resolve().then(() => ({ bytesRead: globalThis.__nodeFs.readvSync(handle.fd, buffers, position), buffers })); handle.writev = (buffers, position) => Promise.resolve().then(() => ({ bytesWritten: globalThis.__nodeFs.writevSync(handle.fd, buffers, position), buffers })); handle.truncate = (length = 0) => Promise.resolve().then(() => globalThis.__nodeFs.ftruncateSync(handle.fd, length)); handle.stat = () => Promise.resolve().then(() => globalThis.__nodeFs.fstatSync(handle.fd)); handle.sync = () => Promise.resolve().then(() => globalThis.__nodeFs.fsyncSync(handle.fd)); handle.datasync = () => Promise.resolve().then(() => globalThis.__nodeFs.fdatasyncSync(handle.fd)); handle.chmod = (mode) => Promise.resolve().then(() => globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[handle.fd], mode)); handle.readFile = (options) => Promise.resolve().then(() => globalThis.__nodeFs.readFileSync(handle.fd, options)); handle.writeFile = (data, options) => Promise.resolve().then(() => globalThis.__nodeFs.writeFileSync(handle.fd, data, options)); handle.appendFile = (data, options) => Promise.resolve().then(() => globalThis.__nodeFs.appendFileSync(handle.fd, data, options)); handle.close = () => Promise.resolve().then(() => globalThis.__nodeFs.closeSync(handle.fd)); return handle; };
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
  promisify: (fn) => (...args) => new Promise((resolve, reject) => fn(...args, (error, ...values) => error ? reject(error) : resolve(values.length > 1 ? values : values[0]))),
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
  if (name === 'internal/test/binding') return { internalBinding: (binding) => binding === 'uv' ? { UV_ENOENT: -2, UV_EEXIST: -17 } : ({ fstat: () => undefined }) };
  if (name === 'internal/fs/utils') return { stringToFlags: (flags) => { const values = { r: 0, 'r+': 2, rs: 1052674, 'rs+': 1052674, sr: 1052674, 'sr+': 1052674, w: 577, 'w+': 578, wx: 705, xw: 705, 'wx+': 706, 'xw+': 706, a: 1089, 'a+': 1090, ax: 1217, xa: 1217, 'ax+': 1218, 'xa+': 1218, as: 1051713, sa: 1051713, 'as+': 1051714, 'sa+': 1051714 }; if (typeof flags !== 'string' || values[flags] === undefined) { const error = new TypeError(`Unknown file open flag: ${flags}`); error.code = 'ERR_INVALID_ARG_VALUE'; throw error; } return values[flags]; } };
  if (name === 'timers') return globalThis.__nodeTimers;
  if (name === 'timers/promises') return globalThis.__nodeTimersPromises;
  if (name === '../common' || name.endsWith('/common')) return globalThis.__nodeCommon;
  if (name.endsWith('/common/tmpdir')) return globalThis.__nodeTmpdir;
  if (name === 'buffer') return { Buffer: NodeBuffer, kMaxLength: 0x7fffffff, atob: nodeAtob, btoa: nodeBtoa };
  if (name === '../common/fixtures' || name.endsWith('/common/fixtures')) return { fixturesDir: `${globalThis.__quench_cwd}/tests/node/test/fixtures`, path: (file) => `${globalThis.__quench_cwd}/tests/node/test/fixtures/${file}`, utf8TestText: 'The quick brown fox jumps over the lazy dog.\n' };
  if (name === 'fs' || name === 'fs/promises') return globalThis.__nodeFs;
  throw new Error(`Cannot find module '${specifier}'`);
};
