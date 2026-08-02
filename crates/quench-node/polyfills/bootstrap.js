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
  resolve: (...parts) => path.join(...parts),
};

globalThis.__nodeCommon = {
  mustCall: (fn) => fn,
  mustNotCall: () => () => { throw new Error('Unexpected call'); },
  noop: () => {},
  expectWarning: () => {},
};
globalThis.__nodeFs = {
  existsSync: (value) => globalThis.__quench_fs_exists(String(value)),
  mkdtempSync: (prefix) => globalThis.__quench_fs_mkdtemp(String(prefix)),
};
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, '');
  if (name === 'assert') return globalThis.__nodeAssert;
  if (name === 'path' || name === 'path/posix') return globalThis.__nodePath;
  if (name === 'util') return { format: globalThis.__nodeFormat };
  if (name === 'events') return { EventEmitter: class {} };
  if (name === '../common' || name.endsWith('/common')) return globalThis.__nodeCommon;
  if (name === 'buffer') return { Buffer, kMaxLength: 0x7fffffff };
  if (name === 'fs' || name === 'fs/promises') return globalThis.__nodeFs;
  throw new Error(`Cannot find module '${specifier}'`);
};
