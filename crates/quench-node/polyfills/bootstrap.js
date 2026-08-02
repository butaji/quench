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
  env: {},
  argv: ['quench-node'],
  platform: 'unknown',
  arch: 'unknown',
  version: 'v0.1.0',
  versions: { node: '0.1.0' },
  cwd: () => '.',
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
globalThis.__nodeFs = {
  existsSync: (value) => globalThis.__quench_fs_exists(String(value)),
  mkdtempSync: (prefix) => globalThis.__quench_fs_mkdtemp(String(prefix)),
  readFileSync: (value) => globalThis.__quench_fs_read_file(String(value)),
  writeFileSync: (value, data) => globalThis.__quench_fs_write_file(String(value), String(data)),
  statSync: () => ({ isFile: () => true, isDirectory: () => false }),
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
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, '');
  if (name === 'assert') return globalThis.__nodeAssert;
  if (name === 'path' || name === 'path/posix') return globalThis.__nodePath;
  if (name === 'util') return globalThis.__nodeUtil;
  if (name === 'os') return globalThis.__nodeOs;
  if (name === 'querystring') return globalThis.__nodeQuerystring;
  if (name === 'events') return { EventEmitter: globalThis.__nodeEventEmitter };
  if (name === '../common' || name.endsWith('/common')) return globalThis.__nodeCommon;
  if (name === 'buffer') return { Buffer, kMaxLength: 0x7fffffff };
  if (name === 'fs' || name === 'fs/promises') return globalThis.__nodeFs;
  throw new Error(`Cannot find module '${specifier}'`);
};
