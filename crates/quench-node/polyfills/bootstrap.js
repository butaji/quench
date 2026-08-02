/* Small, dependency-free globals available before user code. */
globalThis.global = globalThis;
globalThis.globalThis = globalThis;

const format = (args) => args.map((value) => {
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
