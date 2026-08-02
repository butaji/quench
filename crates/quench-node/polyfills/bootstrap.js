/* Small, dependency-free globals available before user code. */
globalThis.global = globalThis;
globalThis.globalThis = globalThis;

globalThis.__nodeFormat = (args) => args.map((value) => {
  try { return typeof value === 'string' ? value : JSON.stringify(value); }
  catch (_) { return String(value); }
}).join(' ');
globalThis.console = globalThis.console || {};
for (const method of ['log', 'info', 'warn', 'error', 'debug']) {
  globalThis.console[method] = (...args) => undefined;
}

globalThis.process = {
  env: new Proxy({}, { get: (_, key) => typeof key === 'string' ? globalThis.__quench_env_get(key) : undefined }),
  argv: ['quench-node'],
  platform: 'unknown',
  arch: 'unknown',
  version: 'v0.1.0',
  versions: { node: '0.1.0' },
  cwd: () => globalThis.__quench_cwd,
  nextTick: (callback, ...args) => queueMicrotask(() => callback(...args)),
  hrtime: { bigint: () => BigInt(Date.now()) * 1000000n },
};

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

class Buffer extends Uint8Array {
  static from(value, encoding) {
    if (typeof value === 'string') {
      if (encoding === 'hex') {
        const output = new Buffer(value.length / 2);
        for (let i = 0; i < output.length; i++) output[i] = parseInt(value.slice(i * 2, i * 2 + 2), 16);
        return output;
      }
      const output = new Buffer(value.length);
      for (let i = 0; i < value.length; i++) output[i] = value.charCodeAt(i) & 255;
      return output;
    }
    return new Buffer(value);
  }
  static alloc(size, fill = 0) { return new Buffer(size).fill(fill); }
  toString(encoding = 'utf8') {
    if (encoding === 'hex') return Array.from(this, (byte) => byte.toString(16).padStart(2, '0')).join('');
    return String.fromCharCode(...this);
  }
}
globalThis.Buffer = Buffer;

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
globalThis.__nodeAssert.match = (value, expression) => {
  if (!expression.test(String(value))) throw new Error('Value did not match expression');
};

globalThis.__nodePath = {
  sep: '/',
  basename: (value) => String(value).replace(/\\/g, '/').split('/').pop(),
  dirname: (value) => { const parts = String(value).replace(/\\/g, '/').split('/'); parts.pop(); return parts.join('/') || '.'; },
  extname: (value) => { const name = globalThis.__nodePath.basename(value); const i = name.lastIndexOf('.'); return i > 0 ? name.slice(i) : ''; },
  join: (...parts) => parts.join('/').replace(/\/+/g, '/'),
  resolve: (...parts) => globalThis.__nodePath.join(...parts),
};

globalThis.__nodeCommon = {
  mustCall: (fn) => fn,
  mustNotCall: () => () => { throw new Error('Unexpected call'); },
  noop: () => {},
  expectWarning: () => {},
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
  existsSync: (value) => globalThis.__quench_fs_exists(String(value)),
  mkdtempSync: (prefix) => globalThis.__quench_fs_mkdtemp(String(prefix)),
  readFileSync: (value) => globalThis.__quench_fs_read_file(String(value)),
  writeFileSync: (value, data) => globalThis.__quench_fs_write_file(String(value), String(data)),
  statSync: () => ({ isFile: () => true, isDirectory: () => false }),
};
globalThis.__nodeFs.readFile = (value, options, callback) => {
  if (typeof options === 'function') { callback = options; options = undefined; }
  queueMicrotask(() => {
    try { callback(null, globalThis.__nodeFs.readFileSync(value, options)); }
    catch (error) { callback(error); }
  });
};
globalThis.__nodeFs.writeFile = (value, data, options, callback) => {
  if (typeof options === 'function') callback = options;
  queueMicrotask(() => {
    try { globalThis.__nodeFs.writeFileSync(value, data); callback(null); }
    catch (error) { callback(error); }
  });
};
globalThis.__nodeFs.promises = {
  readFile: (value, options) => new Promise((resolve, reject) => globalThis.__nodeFs.readFile(value, options, (error, data) => error ? reject(error) : resolve(data))),
  writeFile: (value, data, options) => new Promise((resolve, reject) => globalThis.__nodeFs.writeFile(value, data, options, (error) => error ? reject(error) : resolve())),
};
globalThis.__nodeOs = {
  EOL: '\n',
  platform: () => process.platform,
  arch: () => process.arch,
  tmpdir: () => '/tmp',
  homedir: () => '/',
  type: () => 'Quench',
  endianness: () => 'LE',
  hostname: () => 'quench-node',
  cpus: () => [],
  userInfo: () => ({ username: '', homedir: '/' }),
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
    if (base && !/^[a-z][a-z0-9+.-]*:/.test(value)) value = String(base).replace(/\/$/, '') + '/' + value.replace(/^\//, '');
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
  get href() { const query = this.searchParams.toString(); return `${this.origin === 'null' ? '' : this.origin}${this.pathname}${query ? `?${query}` : this.search}${this.hash}`; }
  toString() { return this.href; }
};
globalThis.URL = globalThis.__nodeURL;
globalThis.URLSearchParams = globalThis.__nodeURLSearchParams;
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, '');
  if (name === 'assert') return globalThis.__nodeAssert;
  if (name === 'path' || name === 'path/posix') return globalThis.__nodePath;
  if (name === 'util') return globalThis.__nodeUtil;
  if (name === 'os') return globalThis.__nodeOs;
  if (name === 'querystring') return globalThis.__nodeQuerystring;
  if (name === 'events') return { EventEmitter: globalThis.__nodeEventEmitter };
  if (name === 'stream') return globalThis.__nodeStream;
  if (name === 'timers') return globalThis.__nodeTimers;
  if (name === 'timers/promises') return globalThis.__nodeTimersPromises;
  if (name === '../common' || name.endsWith('/common')) return globalThis.__nodeCommon;
  if (name === 'buffer') return { Buffer, kMaxLength: 0x7fffffff };
  if (name === 'fs' || name === 'fs/promises') return globalThis.__nodeFs;
  throw new Error(`Cannot find module '${specifier}'`);
};
